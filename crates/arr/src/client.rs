use crate::model::*;
use crate::{Error, Result, Service};
use reqwest::{RequestBuilder, Response, StatusCode, Url, header};
use serde::de::DeserializeOwned;
use std::time::Duration;

/// One Radarr or Sonarr. The API key is held in memory only and redacted from `Debug`.
#[derive(Clone)]
pub struct ArrClient {
    pub(crate) http: reqwest::Client,
    base: Url,
    service: Service,
    key: String,
}

impl std::fmt::Debug for ArrClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrClient")
            .field("service", &self.service)
            .field("base", &self.base.as_str())
            .field("key", &"<redacted>")
            .finish()
    }
}

fn http_err(e: reqwest::Error) -> Error {
    Error::Http(e.without_url())
}

/// `http` for an address that is plainly on this network, `https` otherwise.
fn default_scheme(s: &str) -> &'static str {
    let host = s.split(['/', ':']).next().unwrap_or_default();
    let lan = host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".lan")
        || host.parse::<std::net::Ipv4Addr>().is_ok();
    if lan { "http" } else { "https" }
}

/// The address as stored and compared: scheme and host lowercase, no trailing slash, no `/api/v3`
/// suffix, no query or fragment.
pub fn normalize_url(s: &str) -> Result<String> {
    let s = s.trim();
    let full = if s.contains("://") {
        s.to_string()
    } else {
        format!("{}://{s}", default_scheme(s))
    };
    let mut u = Url::parse(&full)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
        .ok_or_else(|| Error::Config("that is not a web address".into()))?;
    u.set_query(None);
    u.set_fragment(None);
    let path = u.path().trim_end_matches('/');
    let path = path.strip_suffix("/api/v3").unwrap_or(path).to_string();
    u.set_path(&path);
    Ok(u.as_str().trim_end_matches('/').to_string())
}

/// Keyring name of the API key of a service: `radarr:<normalised url>` / `sonarr:<url>`.
pub fn secret_name(service: Service, url: &str) -> String {
    format!(
        "{}:{}",
        service.key(),
        normalize_url(url).unwrap_or_else(|_| url.trim().to_string())
    )
}

fn message_of(body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    // Validation errors: `[{"errorMessage": "..."}]`; others: `{"message": "..."}`.
    v.as_array()
        .and_then(|a| a.first())
        .and_then(|e| e["errorMessage"].as_str())
        .or_else(|| v["message"].as_str())
        .or_else(|| v["errorMessage"].as_str())
        .unwrap_or_default()
        .to_string()
}

impl ArrClient {
    pub fn new(service: Service, base_url: &str, key: &str) -> Result<Self> {
        let base = Url::parse(&normalize_url(base_url)?)
            .map_err(|_| Error::Config("that is not a web address".into()))?;
        if key.trim().is_empty() {
            return Err(Error::Config("enter the API key".into()));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .user_agent(concat!("NOCTURNE/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(http_err)?;
        Ok(Self {
            http,
            base,
            service,
            key: key.trim().to_string(),
        })
    }

    pub fn service(&self) -> Service {
        self.service
    }

    pub fn base_url(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_string()
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}/api/v3{path}", self.base.as_str().trim_end_matches('/'))
    }

    pub(crate) fn authed(&self, rb: RequestBuilder) -> RequestBuilder {
        match header::HeaderValue::from_str(&self.key) {
            Ok(mut h) => {
                h.set_sensitive(true);
                rb.header("X-Api-Key", h)
            }
            Err(_) => rb,
        }
    }

    pub(crate) async fn send(&self, rb: RequestBuilder, path: &str) -> Result<Response> {
        let r = rb.send().await.map_err(http_err)?;
        let status = r.status();
        if status.is_success() {
            return Ok(r);
        }
        let msg = message_of(&r.text().await.unwrap_or_default());
        Err(match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Error::Unauthorized,
            StatusCode::NOT_FOUND => Error::NotFound,
            StatusCode::BAD_REQUEST | StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY
                if !msg.is_empty() =>
            {
                Error::Rejected(msg)
            }
            s => Error::Status {
                status: s.as_u16(),
                path: path.to_string(),
            },
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let rb = self.authed(self.http.get(self.url(path)).query(query));
        self.send(rb, path).await?.json().await.map_err(http_err)
    }

    // ---- reads ----

    /// Is this address a Radarr/Sonarr and does it accept the key? Also names the version.
    pub async fn status(&self) -> Result<Status> {
        self.get("/system/status", &[]).await
    }

    pub async fn quality_profiles(&self) -> Result<Vec<Profile>> {
        self.get("/qualityprofile", &[]).await
    }

    pub async fn root_folders(&self) -> Result<Vec<RootFolder>> {
        self.get("/rootfolder", &[]).await
    }

    /// A title search through the service (`term` is free text, or `tmdb:603` / `tvdb:81189`).
    pub async fn lookup(&self, term: &str) -> Result<Vec<Lookup>> {
        let path = match self.service {
            Service::Radarr => "/movie/lookup",
            Service::Sonarr => "/series/lookup",
        };
        self.get(path, &[("term", term.to_string())]).await
    }

    /// Where a movie (Radarr, by TMDB id) or a series (Sonarr, by TVDB id) stands in the service.
    pub async fn progress(&self, id: u64) -> Result<Progress> {
        let movie = self.service == Service::Radarr;
        let (path, q) = if movie {
            ("/movie", ("tmdbId", id.to_string()))
        } else {
            ("/series", ("tvdbId", id.to_string()))
        };
        let found: Vec<Existing> = self.get(path, &[q]).await?;
        let Some(e) = found.into_iter().next() else {
            return Ok(Progress::Unknown);
        };
        let queue: QueuePage = self
            .get(
                "/queue",
                &[("page", "1".into()), ("pageSize", "200".into())],
            )
            .await?;
        Ok(progress_of(&e, &queue.records, movie))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_names() {
        assert_eq!(
            normalize_url("HTTP://Radarr.Example.com/api/v3/").unwrap(),
            "http://radarr.example.com"
        );
        assert_eq!(
            normalize_url("192.168.1.5:7878").unwrap(),
            "http://192.168.1.5:7878"
        );
        assert!(normalize_url("ftp://x").is_err());
        assert_eq!(
            secret_name(Service::Radarr, "http://A.lan:7878/"),
            "radarr:http://a.lan:7878"
        );
        assert_ne!(
            secret_name(Service::Radarr, "http://a.lan"),
            secret_name(Service::Sonarr, "http://a.lan")
        );
    }

    #[test]
    fn the_key_never_prints() {
        let c = ArrClient::new(Service::Sonarr, "http://127.0.0.1:1", "s3cr3tkey").unwrap();
        assert!(!format!("{c:?}").contains("s3cr3tkey"));
        assert!(ArrClient::new(Service::Sonarr, "http://127.0.0.1:1", "  ").is_err());
    }

    #[test]
    fn error_messages_are_read_from_both_shapes() {
        assert_eq!(
            message_of(
                r#"[{"propertyName":"TmdbId","errorMessage":"This movie has already been added"}]"#
            ),
            "This movie has already been added"
        );
        assert_eq!(message_of(r#"{"message":"Unauthorized"}"#), "Unauthorized");
        assert_eq!(message_of("not json"), "");
    }
}
