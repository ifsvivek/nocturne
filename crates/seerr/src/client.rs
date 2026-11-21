use crate::model::*;
use crate::{Error, Result};
use reqwest::{RequestBuilder, Response, StatusCode, Url, header};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::time::Duration;

/// The `connect.sid` session cookie: a bearer credential for the user's own Seerr account. Never
/// printed (`Debug` is redacted); lives in memory and in the system keyring only.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionCookie(String);

impl SessionCookie {
    pub fn from_value(v: impl Into<String>) -> Self {
        Self(v.into())
    }

    /// The raw value, for the keyring.
    pub fn value(&self) -> &str {
        &self.0
    }

    fn header(&self) -> Option<header::HeaderValue> {
        let mut h = header::HeaderValue::from_str(&format!("connect.sid={}", self.0)).ok()?;
        h.set_sensitive(true);
        Some(h)
    }
}

impl std::fmt::Debug for SessionCookie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionCookie(<redacted>)")
    }
}

/// How requests prove who is asking.
#[derive(Clone, Debug)]
pub enum Auth {
    /// Sign-in calls and the unauthenticated reads.
    None,
    Cookie(SessionCookie),
    /// Developer mode (`NOCTURNE_SEERR_KEY`): the global key acts as Seerr's first admin, so this
    /// mode is read only here: every write is refused.
    ApiKey(String),
}

#[derive(Clone)]
pub struct SeerrClient {
    pub(crate) http: reqwest::Client,
    base: Url,
    auth: Auth,
}

impl std::fmt::Debug for SeerrClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeerrClient")
            .field("base", &self.base.as_str())
            .field("auth", &"<redacted>")
            .finish()
    }
}

fn http_err(e: reqwest::Error) -> Error {
    Error::Http(e.without_url())
}

/// `http` for an address that is plainly on this network (an IP, `localhost`, `.local`, `.lan`),
/// `https` otherwise: what someone means when they type a bare host.
fn default_scheme(s: &str) -> &'static str {
    let host = s.split(['/', ':']).next().unwrap_or_default();
    let lan = host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".lan")
        || host.parse::<std::net::Ipv4Addr>().is_ok();
    if lan { "http" } else { "https" }
}

/// The server address as stored and compared: scheme and host lowercase, no trailing slash, no
/// `/api/v1` suffix, no query or fragment.
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
    let path = path.strip_suffix("/api/v1").unwrap_or(path).to_string();
    u.set_path(&path);
    Ok(u.as_str().trim_end_matches('/').to_string())
}

/// Keyring name of the cookie for a server: `seerr:<normalised url>` (the keyring service stays
/// the app's own, so `NOCTURNE_KEYRING_SERVICE` isolates tests unchanged).
pub fn secret_name(url: &str) -> String {
    format!(
        "seerr:{}",
        normalize_url(url).unwrap_or_else(|_| url.trim().to_string())
    )
}

/// Seerr's `message` of an error body, when there is one.
fn message_of(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

/// The `connect.sid` value out of the `Set-Cookie` headers of a sign-in reply.
fn cookie_of(r: &Response) -> Option<SessionCookie> {
    r.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| {
            let v = c.strip_prefix("connect.sid=")?.split(';').next()?;
            (!v.is_empty()).then(|| SessionCookie::from_value(v))
        })
}

impl SeerrClient {
    pub fn new(base_url: &str, auth: Auth) -> Result<Self> {
        let base = Url::parse(&normalize_url(base_url)?)
            .map_err(|_| Error::Config("that is not a web address".into()))?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("NOCTURNE/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(http_err)?;
        Ok(Self { http, base, auth })
    }

    pub fn base_url(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_string()
    }

    pub fn is_api_key(&self) -> bool {
        matches!(self.auth, Auth::ApiKey(_))
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base.as_str().trim_end_matches('/'))
    }

    pub(crate) fn authed(&self, rb: RequestBuilder) -> RequestBuilder {
        match &self.auth {
            Auth::None => rb,
            Auth::Cookie(c) => match c.header() {
                Some(h) => rb.header(header::COOKIE, h),
                None => rb,
            },
            Auth::ApiKey(k) => match header::HeaderValue::from_str(k) {
                Ok(mut h) => {
                    h.set_sensitive(true);
                    rb.header("X-Api-Key", h)
                }
                Err(_) => rb,
            },
        }
    }

    /// Send and map the status to an [`Error`]; `sign_in` makes a 401 mean wrong credentials.
    pub(crate) async fn send(
        &self,
        rb: RequestBuilder,
        path: &str,
        sign_in: bool,
    ) -> Result<Response> {
        let r = rb.send().await.map_err(http_err)?;
        let status = r.status();
        if status.is_success() {
            return Ok(r);
        }
        let msg = message_of(&r.text().await.unwrap_or_default());
        Err(match status {
            StatusCode::UNAUTHORIZED if sign_in => Error::InvalidCredentials,
            StatusCode::UNAUTHORIZED => Error::Unauthorized,
            // An unknown or expired cookie is answered 403 `{"status":403,"error":"You do not have
            // permission to access this endpoint"}` (no `message`); the refusals worth showing
            // ("You do not have permission to make movie requests.", quotas) carry a `message`.
            StatusCode::FORBIDDEN if msg.is_empty() && !sign_in => Error::Unauthorized,
            StatusCode::FORBIDDEN if msg.to_lowercase().contains("quota") => {
                Error::QuotaExceeded(msg)
            }
            StatusCode::FORBIDDEN => Error::Forbidden(msg),
            StatusCode::CONFLICT => Error::Conflict,
            StatusCode::NOT_FOUND => Error::NotFound,
            s => Error::Status {
                status: s.as_u16(),
                path: path.to_string(),
            },
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let rb = self.authed(self.http.get(self.url(path)).query(query));
        self.send(rb, path, false)
            .await?
            .json()
            .await
            .map_err(http_err)
    }

    // ---- no sign-in needed ----

    pub async fn status(&self) -> Result<Status> {
        self.get("/status", &[]).await
    }

    pub async fn public_settings(&self) -> Result<PublicSettings> {
        self.get("/settings/public", &[]).await
    }

    // ---- sign-in (explicit user actions; none of them is a write in the sense of `RequestConfirmed`) ----

    /// Jellyfin username and password, forwarded by Seerr to its own Jellyfin. The password is used
    /// for this one call and not kept.
    pub async fn sign_in_password(
        &self,
        user: &str,
        password: &str,
    ) -> Result<(Me, SessionCookie)> {
        let path = "/auth/jellyfin";
        let rb = self
            .http
            .post(self.url(path))
            .json(&json!({ "username": user, "password": password }));
        self.signed(rb, path).await
    }

    pub async fn quick_connect_initiate(&self) -> Result<QuickConnect> {
        let path = "/auth/jellyfin/quickconnect/initiate";
        let rb = self.http.post(self.url(path));
        self.send(rb, path, true)
            .await?
            .json()
            .await
            .map_err(http_err)
    }

    /// Has the code been authorised yet? An expired secret is `NotFound`.
    pub async fn quick_connect_check(&self, secret: &str) -> Result<bool> {
        let path = "/auth/jellyfin/quickconnect/check";
        let rb = self.http.get(self.url(path)).query(&[("secret", secret)]);
        let c: QuickCheck = self
            .send(rb, path, true)
            .await?
            .json()
            .await
            .map_err(http_err)?;
        Ok(c.authenticated)
    }

    pub async fn quick_connect_complete(&self, secret: &str) -> Result<(Me, SessionCookie)> {
        let path = "/auth/jellyfin/quickconnect/authenticate";
        let rb = self
            .http
            .post(self.url(path))
            .json(&json!({ "secret": secret }));
        self.signed(rb, path).await
    }

    async fn signed(&self, rb: RequestBuilder, path: &str) -> Result<(Me, SessionCookie)> {
        let r = self.send(rb, path, true).await?;
        let cookie = cookie_of(&r).ok_or(Error::Status {
            status: 200,
            path: path.to_string(),
        })?;
        let me: Me = r.json().await.map_err(http_err)?;
        Ok((me, cookie))
    }

    /// Ends the session on the server; the caller deletes the stored cookie either way.
    pub async fn sign_out(&self) {
        let path = "/auth/logout";
        let _ = self.authed(self.http.post(self.url(path))).send().await;
    }

    // ---- reads as the signed-in user ----

    pub async fn me(&self) -> Result<Me> {
        self.get("/auth/me", &[]).await
    }

    pub async fn search(&self, query: &str) -> Result<SearchPage> {
        self.get(
            "/search",
            &[("query", query.to_string()), ("page", "1".into())],
        )
        .await
    }

    pub async fn detail(&self, kind: Kind, tmdb_id: u64) -> Result<Detail> {
        self.get(&format!("/{}/{tmdb_id}", kind.as_str()), &[])
            .await
    }

    pub async fn my_requests(&self, user_id: u64, take: u32, skip: u32) -> Result<RequestsPage> {
        self.get(
            &format!("/user/{user_id}/requests"),
            &[("take", take.to_string()), ("skip", skip.to_string())],
        )
        .await
    }

    pub async fn quota(&self, user_id: u64) -> Result<Quota> {
        self.get(&format!("/user/{user_id}/quota"), &[]).await
    }

    /// A poster from TMDB's image CDN (`poster_path` as Seerr gives it). Sent without any Seerr
    /// credential: the cookie never leaves for another host.
    pub async fn poster(&self, poster_path: &str) -> Result<Vec<u8>> {
        if !poster_path.starts_with('/')
            || poster_path.contains(['?', '#'])
            || poster_path.contains("..")
        {
            return Err(Error::NotFound);
        }
        let r = self
            .http
            .get(format!("https://image.tmdb.org/t/p/w185{poster_path}"))
            .send()
            .await
            .map_err(http_err)?;
        if !r.status().is_success() {
            return Err(Error::NotFound);
        }
        Ok(r.bytes().await.map_err(http_err)?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_normalised() {
        let n = |s| normalize_url(s).unwrap();
        assert_eq!(n("HTTP://Seerr.Example.com/"), "http://seerr.example.com");
        assert_eq!(n("seerr.example.com"), "https://seerr.example.com");
        assert_eq!(n("192.168.1.5:5055"), "http://192.168.1.5:5055");
        assert_eq!(n("localhost:5055/"), "http://localhost:5055");
        assert_eq!(n("https://x.org/seerr/api/v1/"), "https://x.org/seerr");
        assert_eq!(n("https://x.org/?a=1#b"), "https://x.org");
        assert!(normalize_url("ftp://x").is_err());
        assert!(normalize_url("").is_err());
    }

    #[test]
    fn keyring_name_is_per_server_and_stable() {
        assert_eq!(secret_name("HTTPS://A.org/"), secret_name("https://a.org"));
        assert_ne!(secret_name("https://a.org"), secret_name("https://b.org"));
        assert!(secret_name("https://a.org").starts_with("seerr:"));
    }

    #[test]
    fn secrets_do_not_print() {
        let c = SessionCookie::from_value("s%3Asecret");
        assert!(!format!("{c:?}").contains("secret"));
        let k = SeerrClient::new("http://127.0.0.1:1", Auth::Cookie(c)).unwrap();
        assert!(!format!("{k:?}").contains("secret"));
    }
}
