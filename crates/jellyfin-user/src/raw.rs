//! POST and DELETE forwarding for the NOCTURNE gateway: the few user-scope writes a gateway client may cause (playback reports, played and favourite marks, user data, session commands) and the PlaybackInfo query (a POST that only reads).
//! The gateway still decides WHETHER to forward (`allow_server_writes`, `sync_progress`, never to a server other than the item's own); this module only refuses everything that is not on the list below, so a bug upstream of it can never reach an admin or user-management route.
use crate::{Error, Result};
use nocturne_jellyfin::raw::Raw;
use reqwest::{Method, header};
use std::time::Duration;

/// Whether `path` (no query) is one of the routes the gateway may call. Matched by segment shape, case-insensitively.
pub fn allowed(method: &Method, path: &str) -> bool {
    let seg: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .map(str::to_ascii_lowercase)
        .collect();
    let s: Vec<&str> = seg.iter().map(String::as_str).collect();
    let post = method == Method::POST;
    let del = method == Method::DELETE;
    match s.as_slice() {
        ["sessions", "playing"] => post,
        ["sessions", "playing", "progress" | "stopped" | "ping"] => post,
        ["userplayeditems", _] | ["userfavoriteitems", _] => post || del,
        ["useritems", _, "userdata"] => post,
        ["items", _, "playbackinfo"] => post,
        ["sessions", _, "playing"] | ["sessions", _, "command"] | ["sessions", _, "message"] => {
            post
        }
        ["sessions", _, "playing", _] | ["sessions", _, "command", _] => post,
        _ => false,
    }
}

#[derive(Clone)]
pub struct RawUser {
    urls: Raw,
    http: reqwest::Client,
}

impl RawUser {
    pub fn new(base_url: &str, token: &str, device_id: &str) -> Result<Self> {
        let urls =
            Raw::new(base_url, token, device_id).map_err(|e| Error::Config(e.to_string()))?;
        let mut auth = header::HeaderValue::from_str(&format!(
            "MediaBrowser Client=\"NOCTURNE Gateway\", Device=\"Linux\", DeviceId=\"{device_id}\", Version=\"{}\", Token=\"{token}\"",
            env!("CARGO_PKG_VERSION")
        ))
        .map_err(|_| Error::Config("token contains invalid header characters".into()))?;
        auth.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth);
        if let Some((n, v)) = nocturne_jellyfin::header::for_url(base_url) {
            headers.insert(n, v);
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(headers)
            .build()
            .map_err(|e| Error::Http(e.without_url()))?;
        Ok(Self { urls, http })
    }

    /// `POST path_and_query` with an optional JSON body, only for a route [`allowed`] lists. Any status is an `Ok`.
    pub async fn post(
        &self,
        path_and_query: &str,
        json: Option<Vec<u8>>,
    ) -> Result<reqwest::Response> {
        self.send(Method::POST, path_and_query, json).await
    }

    /// `DELETE path_and_query`, only for a route [`allowed`] lists.
    pub async fn delete(&self, path_and_query: &str) -> Result<reqwest::Response> {
        self.send(Method::DELETE, path_and_query, None).await
    }

    async fn send(
        &self,
        method: Method,
        path_and_query: &str,
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::Response> {
        let path = path_and_query.split('?').next().unwrap_or_default();
        if !allowed(&method, path) {
            return Err(Error::Config(
                "route not allowed through the gateway".into(),
            ));
        }
        let url = self
            .urls
            .url(path_and_query)
            .map_err(|e| Error::Config(e.to_string()))?;
        let mut rb = self.http.request(method, url);
        if let Some(b) = body {
            rb = rb.header(header::CONTENT_TYPE, "application/json").body(b);
        }
        rb.send().await.map_err(|e| Error::Http(e.without_url()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_listed_routes_pass() {
        let (p, d) = (Method::POST, Method::DELETE);
        for (m, path) in [
            (&p, "/Sessions/Playing"),
            (&p, "/Sessions/Playing/Progress"),
            (&p, "/sessions/playing/stopped"),
            (&p, "/UserPlayedItems/abc"),
            (&d, "/UserPlayedItems/abc"),
            (&d, "/UserFavoriteItems/abc"),
            (&p, "/UserItems/abc/UserData"),
            (&p, "/Items/abc/PlaybackInfo"),
            (&p, "/Sessions/abc/Playing/Pause"),
            (&p, "/Sessions/abc/Command"),
        ] {
            assert!(allowed(m, path), "{m} {path}");
        }
        for (m, path) in [
            (&p, "/Users/New"),
            (&p, "/Users/abc/Password"),
            (&d, "/Users/abc"),
            (&d, "/Items/abc"),
            (&p, "/Library/Refresh"),
            (&p, "/Sessions/Logout"),
            (&p, "/System/Restart"),
            (&d, "/Sessions/Playing"),
            (&p, "/Auth/Keys"),
            (&p, "/Sessions/abc/User/def"),
            (&p, "/Playlists"),
        ] {
            assert!(!allowed(m, path), "{m} {path}");
        }
    }
}
