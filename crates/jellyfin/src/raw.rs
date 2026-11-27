//! A GET-only forwarding primitive for the NOCTURNE gateway: it relays one request to the server and hands back the raw response, so the caller can stream the body.
//! Still strictly read-only (the one verb is GET, `tests/readonly.rs` keeps it so). Unlike [`crate::Client`] there is no total timeout (media streams run for hours) and redirects are never followed.
use crate::{Error, Result};
use reqwest::{Url, header};
use std::time::Duration;

/// Request headers that may be passed on to a server: range and validators for media, `Accept` for content negotiation. Everything else a client sent (cookies, its own token, `Host`, `X-Forwarded-*`) is dropped.
const FORWARD: [&str; 5] = [
    "range",
    "if-range",
    "if-none-match",
    "if-modified-since",
    "accept",
];

#[derive(Clone)]
pub struct Raw {
    http: reqwest::Client,
    base: Url,
}

impl std::fmt::Debug for Raw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Raw")
            .field("base", &self.base.as_str())
            .field("token", &"<redacted>")
            .finish()
    }
}

fn http_err(e: reqwest::Error) -> Error {
    Error::Http(e.without_url())
}

impl Raw {
    /// `device_id` names this process to the server (sessions are keyed by it).
    pub fn new(base_url: &str, token: &str, device_id: &str) -> Result<Self> {
        let base = Url::parse(base_url.trim_end_matches('/'))
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
            .ok_or_else(|| Error::Config("base_url is not a valid http(s) URL".into()))?;
        let mut auth = header::HeaderValue::from_str(&format!(
            "MediaBrowser Client=\"NOCTURNE Gateway\", Device=\"Linux\", DeviceId=\"{device_id}\", Version=\"{}\", Token=\"{token}\"",
            env!("CARGO_PKG_VERSION")
        ))
        .map_err(|_| Error::Config("token contains invalid header characters".into()))?;
        auth.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth);
        if let Some((n, v)) = crate::header::for_url(base.as_str()) {
            headers.insert(n, v);
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(headers)
            .build()
            .map_err(http_err)?;
        Ok(Self { http, base })
    }

    /// The only URL builder: the stored base plus a root-relative path and query. Anything that could leave the server (another host, `..`, an encoded slash) is refused.
    pub fn url(&self, path_and_query: &str) -> Result<Url> {
        let bad = || Error::Config("path not allowed".into());
        let low = path_and_query.to_ascii_lowercase();
        if !path_and_query.starts_with('/')
            || path_and_query.starts_with("//")
            || path_and_query.bytes().any(|b| b < 0x20 || b == b'\\')
            || low.contains("..")
            || low.contains("%2e")
            || low.contains("%2f")
            || low.contains("%5c")
        {
            return Err(bad());
        }
        let u = Url::parse(&format!(
            "{}{}",
            self.base.as_str().trim_end_matches('/'),
            path_and_query
        ))
        .map_err(|_| bad())?;
        if u.scheme() != self.base.scheme()
            || u.host_str() != self.base.host_str()
            || u.port_or_known_default() != self.base.port_or_known_default()
            || !u.username().is_empty()
        {
            return Err(bad());
        }
        Ok(u)
    }

    /// `GET path_and_query` with the stored token in the `Authorization` header and the allow-listed `headers` (see [`FORWARD`]). Any status is an `Ok`: the caller decides.
    pub async fn get(
        &self,
        path_and_query: &str,
        headers: &[(&str, &str)],
    ) -> Result<reqwest::Response> {
        let mut rb = self.http.get(self.url(path_and_query)?);
        for (n, v) in headers {
            if FORWARD.contains(&n.to_ascii_lowercase().as_str()) {
                rb = rb.header(*n, *v);
            }
        }
        rb.send().await.map_err(http_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_stay_on_the_server() {
        let r = Raw::new("http://127.0.0.1:8097/", "t", "d").unwrap();
        assert_eq!(
            r.url("/Items?a=b").unwrap().as_str(),
            "http://127.0.0.1:8097/Items?a=b"
        );
        for bad in [
            "Items",
            "//evil.test/x",
            "/a/../b",
            "/a/%2e%2E/b",
            "/a%2Fb",
            "/a\\b",
            "/a\r\nHost: x",
            "@evil.test/x",
        ] {
            assert!(r.url(bad).is_err(), "{bad}");
        }
        let p = Raw::new("http://h.test/jf", "t", "d").unwrap();
        assert_eq!(p.url("/Items").unwrap().as_str(), "http://h.test/jf/Items");
    }

    #[test]
    fn debug_hides_the_token() {
        let r = Raw::new("http://127.0.0.1:1", "SECRETVALUE", "d").unwrap();
        assert!(!format!("{r:?}").contains("SECRETVALUE"));
    }
}
