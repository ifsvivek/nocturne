use crate::{Config, Error, Item, Items, PublicInfo, Result, User};
use reqwest::{Url, header};
use serde::de::DeserializeOwned;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Primary,
    Backdrop,
    Logo,
    Thumb,
    Banner,
}

impl ImageType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Backdrop => "Backdrop",
            Self::Logo => "Logo",
            Self::Thumb => "Thumb",
            Self::Banner => "Banner",
        }
    }
}

/// Parameters for `GET /Items`.
#[derive(Debug, Clone, Default)]
pub struct ItemsQuery {
    pub parent_id: Option<String>,
    /// e.g. `["Movie", "Series"]`.
    pub include_types: Vec<String>,
    /// e.g. `"SortName"`, `"DateCreated"`.
    pub sort_by: Option<String>,
    pub descending: bool,
    pub start_index: Option<u32>,
    pub limit: Option<u32>,
    pub recursive: bool,
}

impl ItemsQuery {
    fn pairs(&self, user_id: &str) -> Vec<(&'static str, String)> {
        let mut p = vec![("userId", user_id.to_string())];
        if let Some(v) = &self.parent_id {
            p.push(("parentId", v.clone()));
        }
        if !self.include_types.is_empty() {
            p.push(("includeItemTypes", self.include_types.join(",")));
        }
        if let Some(v) = &self.sort_by {
            p.push(("sortBy", v.clone()));
            p.push((
                "sortOrder",
                if self.descending {
                    "Descending"
                } else {
                    "Ascending"
                }
                .into(),
            ));
        }
        if let Some(v) = self.start_index {
            p.push(("startIndex", v.to_string()));
        }
        if let Some(v) = self.limit {
            p.push(("limit", v.to_string()));
        }
        if self.recursive {
            p.push(("recursive", "true".into()));
        }
        p
    }
}

/// Read-only Jellyfin client. **Strictly GET-only**: every request goes through GET (the private `get` and
/// `image_bytes`); no write verb is reachable from this type (enforced by
/// `tests/readonly.rs`). Write/admin calls belong in the future `crates/jellyfin-admin`
/// crate (explicit UI confirmation, throwaway-server tests only), never here.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: Url,
    token: String,
    device_id: String,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.base.as_str())
            .field("token", &"<redacted>")
            .finish()
    }
}

impl Client {
    pub fn new(cfg: Config) -> Result<Self> {
        let base = Url::parse(&cfg.base_url)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
            .ok_or_else(|| Error::Config("base_url is not a valid http(s) URL".into()))?;
        let device_id = device_id();
        let mut auth = header::HeaderValue::from_str(&auth_header(&device_id, &cfg.token))
            .map_err(|_| Error::Config("token contains invalid header characters".into()))?;
        auth.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .default_headers(headers)
            .build()
            .map_err(http_err)?;
        Ok(Self {
            http,
            base,
            token: cfg.token,
            device_id,
        })
    }

    /// Full `Authorization` header value, for handing to mpv (`http-header-fields`).
    pub fn auth_header(&self) -> String {
        auth_header(&self.device_id, &self.token)
    }

    // ---- reads ----

    /// `GET /System/Info/Public` (no auth required).
    pub async fn public_info(&self) -> Result<PublicInfo> {
        self.get(&["System", "Info", "Public"], &[]).await
    }

    /// `GET /Users`. API-key auth has no implicit user, so callers pick one id.
    pub async fn users(&self) -> Result<Vec<User>> {
        self.get(&["Users"], &[]).await
    }

    /// First user id on the server.
    pub async fn first_user_id(&self) -> Result<String> {
        self.users()
            .await?
            .into_iter()
            .next()
            .map(|u| u.id)
            .ok_or(Error::NoUsers)
    }

    /// Libraries visible to the user (`GET /UserViews`).
    pub async fn libraries(&self, user_id: &str) -> Result<Items> {
        self.get(&["UserViews"], &[("userId", user_id.to_string())])
            .await
    }

    /// `GET /Items`.
    pub async fn items(&self, user_id: &str, q: &ItemsQuery) -> Result<Items> {
        self.get(&["Items"], &q.pairs(user_id)).await
    }

    /// `GET /Items/{id}` for one user (includes overview).
    pub async fn item(&self, user_id: &str, id: &str) -> Result<Item> {
        self.get(&["Items", id], &[("userId", user_id.to_string())])
            .await
    }

    /// `GET /Shows/{id}/Seasons`.
    pub async fn seasons(&self, user_id: &str, series_id: &str) -> Result<Items> {
        self.get(
            &["Shows", series_id, "Seasons"],
            &[("userId", user_id.to_string())],
        )
        .await
    }

    /// `GET /Shows/{id}/Episodes`, optionally limited to one season.
    pub async fn episodes(
        &self,
        user_id: &str,
        series_id: &str,
        season_id: Option<&str>,
    ) -> Result<Items> {
        let mut p = vec![("userId", user_id.to_string())];
        if let Some(s) = season_id {
            p.push(("seasonId", s.to_string()));
        }
        self.get(&["Shows", series_id, "Episodes"], &p).await
    }

    /// Items the user can resume (`GET /UserItems/Resume`), most recent first.
    pub async fn resume(&self, user_id: &str, limit: u32) -> Result<Items> {
        self.get(
            &["UserItems", "Resume"],
            &[
                ("userId", user_id.to_string()),
                ("limit", limit.to_string()),
                ("mediaTypes", "Video".into()),
            ],
        )
        .await
    }

    /// Next unwatched episode of each series being followed (`GET /Shows/NextUp`).
    pub async fn next_up(&self, user_id: &str, limit: u32) -> Result<Items> {
        self.get(&["Shows", "NextUp"], &next_up_pairs(user_id, limit))
            .await
    }

    /// Authenticated image download (GET). The URL is built here from the item id, so the
    /// auth header can only ever go to the configured server.
    pub async fn image_bytes(
        &self,
        id: &str,
        kind: ImageType,
        max_width: Option<u32>,
        tag: Option<&str>,
    ) -> Result<Vec<u8>> {
        let url = self.image_url(id, kind, max_width, tag);
        let path = Url::parse(&url)
            .map(|u| u.path().to_string())
            .unwrap_or_default();
        let resp = self.http.get(url).send().await.map_err(http_err)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                path,
            });
        }
        Ok(resp.bytes().await.map_err(http_err)?.to_vec())
    }

    // ---- URL builders (pure, no network) ----

    /// `/Items/{id}/Images/{type}` with optional width cap and cache-busting tag.
    /// The URL carries no credentials.
    pub fn image_url(
        &self,
        id: &str,
        kind: ImageType,
        max_width: Option<u32>,
        tag: Option<&str>,
    ) -> String {
        let mut p = vec![];
        if let Some(w) = max_width {
            p.push(("maxWidth", w.to_string()));
        }
        if let Some(t) = tag {
            p.push(("tag", t.to_string()));
        }
        self.url(&["Items", id, "Images", kind.as_str()], &p).into()
    }

    /// Direct-play URL (`/Videos/{id}/stream?static=true`) with NO credentials.
    /// Preferred for mpv: pass [`Client::auth_header`] via `http-header-fields`, so the
    /// token never appears in the URL, mpv's log/playlist/`path` property, or process args.
    pub fn stream_url(&self, id: &str) -> String {
        self.url(&["Videos", id, "stream"], &[("static", "true".into())])
            .into()
    }

    /// Same as [`Client::stream_url`] plus `api_key=<token>`. Simpler (works with any
    /// player, no header plumbing) but the token is embedded in the URL and will leak
    /// into logs, `ps` output and history. Treat the returned string as a secret.
    pub fn stream_url_with_key(&self, id: &str) -> String {
        self.url(
            &["Videos", id, "stream"],
            &[("static", "true".into()), ("api_key", self.token.clone())],
        )
        .into()
    }

    // ---- internals ----

    /// Infallible: `base` was validated in `new`; segments are percent-encoded.
    fn url(&self, segs: &[&str], pairs: &[(&str, String)]) -> Url {
        let mut u = self.base.clone();
        u.path_segments_mut()
            .expect("http(s) base can have path segments")
            .pop_if_empty()
            .extend(segs);
        if !pairs.is_empty() {
            u.query_pairs_mut().extend_pairs(pairs);
        }
        u
    }

    async fn get<T: DeserializeOwned>(&self, segs: &[&str], pairs: &[(&str, String)]) -> Result<T> {
        let url = self.url(segs, pairs);
        let path = url.path().to_string();
        let resp = self.http.get(url).send().await.map_err(http_err)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                path,
            });
        }
        resp.json().await.map_err(http_err)
    }
}

// Strip the URL from errors so nothing URL-borne (ids, api_key) lands in logs.
fn next_up_pairs(user_id: &str, limit: u32) -> [(&'static str, String); 2] {
    [("userId", user_id.into()), ("limit", limit.to_string())]
}

fn http_err(e: reqwest::Error) -> Error {
    Error::Http(e.without_url())
}

fn auth_header(device_id: &str, token: &str) -> String {
    format!(
        "MediaBrowser Client=\"NOCTURNE\", Device=\"Linux\", DeviceId=\"{device_id}\", Version=\"{}\", Token=\"{token}\"",
        env!("CARGO_PKG_VERSION")
    )
}

/// Stable per-machine id: derived from /etc/machine-id (prefix only), else a fixed fallback.
fn device_id() -> String {
    let id = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
    let id: String = id.trim().chars().take(12).collect();
    format!("nocturne-{}", if id.is_empty() { "unknown" } else { &id })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Client {
        Client::new(Config {
            base_url: "http://jf.test:8096".into(),
            token: "tok123".into(),
        })
        .unwrap()
    }

    #[test]
    fn auth_header_format() {
        let h = auth_header("dev", "tok");
        assert_eq!(
            h,
            format!(
                "MediaBrowser Client=\"NOCTURNE\", Device=\"Linux\", DeviceId=\"dev\", Version=\"{}\", Token=\"tok\"",
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    #[test]
    fn debug_redacts_token() {
        let d = format!("{:?}", client());
        assert!(!d.contains("tok123"));
    }

    #[test]
    fn image_urls() {
        let c = client();
        assert_eq!(
            c.image_url("abc", ImageType::Primary, Some(300), Some("t1")),
            "http://jf.test:8096/Items/abc/Images/Primary?maxWidth=300&tag=t1"
        );
        assert_eq!(
            c.image_url("abc", ImageType::Logo, None, None),
            "http://jf.test:8096/Items/abc/Images/Logo"
        );
    }

    #[test]
    fn stream_urls() {
        let c = client();
        let plain = c.stream_url("abc");
        assert_eq!(plain, "http://jf.test:8096/Videos/abc/stream?static=true");
        assert!(!plain.contains("tok123"));
        assert!(c.stream_url_with_key("abc").ends_with("&api_key=tok123"));
    }

    #[test]
    fn invalid_base_url_is_config_error() {
        for bad in ["not a url", "", "ftp://h", "mailto:a@b"] {
            let r = Client::new(Config {
                base_url: bad.into(),
                token: "t".into(),
            });
            assert!(matches!(r, Err(Error::Config(_))), "{bad}");
        }
    }

    #[test]
    fn path_segments_are_encoded() {
        let c = client();
        assert_eq!(
            c.stream_url("a/b?c#d e"),
            "http://jf.test:8096/Videos/a%2Fb%3Fc%23d%20e/stream?static=true"
        );
        let c = Client::new(Config {
            base_url: "http://h/jf/".into(),
            token: "t".into(),
        })
        .unwrap();
        assert_eq!(c.stream_url("x"), "http://h/jf/Videos/x/stream?static=true");
    }

    #[test]
    fn items_query_pairs() {
        let q = ItemsQuery {
            parent_id: Some("p".into()),
            include_types: vec!["Movie".into(), "Series".into()],
            sort_by: Some("SortName".into()),
            descending: true,
            start_index: Some(20),
            limit: Some(10),
            recursive: true,
        };
        let s: Vec<String> = q
            .pairs("u")
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        assert_eq!(
            s,
            [
                "userId=u",
                "parentId=p",
                "includeItemTypes=Movie,Series",
                "sortBy=SortName",
                "sortOrder=Descending",
                "startIndex=20",
                "limit=10",
                "recursive=true"
            ]
        );
        assert_eq!(ItemsQuery::default().pairs("u").len(), 1);
    }

    #[test]
    fn deserializes_resume_fields() {
        let j = r#"{"Items":[{"Id":"1","Type":"Episode","RunTimeTicks":36000000000,
            "Genres":["Drama","Noir"],"BackdropImageTags":["b1"],
            "UserData":{"PlaybackPositionTicks":6000000000,"Played":false,"Key":"x"}},{"Id":"2"}]}"#;
        let r: Items = serde_json::from_str(j).unwrap();
        let a = &r.items[0];
        assert_eq!(a.genres, ["Drama", "Noir"]);
        assert_eq!(a.backdrop_image_tags, ["b1"]);
        assert_eq!(a.run_time_ticks, Some(36_000_000_000));
        let u = a.user_data.as_ref().unwrap();
        assert_eq!(
            (u.playback_position_ticks, u.played),
            (6_000_000_000, false)
        );
        assert!(r.items[1].user_data.is_none() && r.items[1].genres.is_empty());
    }

    #[test]
    fn deserializes_tolerantly() {
        let j = r#"{"Items":[{"Id":"1","Name":"N","Type":"Episode","IndexNumber":3,
            "ImageTags":{"Primary":"tg"},"SomethingNew":{"x":1}},{"Id":"2"}],"TotalRecordCount":2,"Extra":0}"#;
        let r: Items = serde_json::from_str(j).unwrap();
        assert_eq!(r.total_record_count, 2);
        assert_eq!(r.items[0].kind, "Episode");
        assert_eq!(r.items[0].index_number, Some(3));
        assert_eq!(r.items[0].image_tags["Primary"], "tg");
        assert_eq!(r.items[1].name, "");
        let p: PublicInfo = serde_json::from_str(r#"{"ServerName":"s","Version":"10"}"#).unwrap();
        assert_eq!(p.version, "10");
    }

    #[test]
    fn next_up_request_and_fixture() {
        assert_eq!(
            next_up_pairs("u1", 12).map(|(k, v)| format!("{k}={v}")),
            ["userId=u1", "limit=12"]
        );
        let j = r#"{"Items":[{"Id":"e9","Name":"Pilot","Type":"Episode","SeriesName":"Invented Show",
            "ParentIndexNumber":2,"IndexNumber":9,"ImageTags":{"Primary":"t"},"New":1}],"TotalRecordCount":1}"#;
        let r: Items = serde_json::from_str(j).unwrap();
        let e = &r.items[0];
        assert_eq!(
            (e.parent_index_number, e.index_number, e.series_name.as_deref()),
            (Some(2), Some(9), Some("Invented Show"))
        );
    }
}
