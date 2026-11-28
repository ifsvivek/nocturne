//! The storage agent's wire format and a client for it. An agent (`nocturne-server agent` on another machine) exports named folders over HTTP with one API key: list, stat, ranged reads, writes and moves inside a folder, free space. No delete, no overwrite.
use crate::{Entry, Space};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Route prefix on the agent.
pub const PREFIX: &str = "/agent/v1";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRoot {
    pub name: String,
    pub writable: bool,
    pub space: Option<Space>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Move {
    pub from: String,
    pub to: String,
}

/// Root names are part of URLs: letters, digits, `-` and `_`.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Clone)]
pub struct AgentClient {
    base: String,
    key: String,
    http: reqwest::Client,
}

const QUICK: Duration = Duration::from_secs(15);

async fn check(r: reqwest::Response) -> Result<reqwest::Response, String> {
    let s = r.status();
    if s.is_success() {
        return Ok(r);
    }
    if s == reqwest::StatusCode::UNAUTHORIZED {
        return Err("the agent refused the key".into());
    }
    #[derive(Deserialize)]
    struct E {
        error: String,
    }
    Err(match r.json::<E>().await {
        Ok(e) => format!("agent: {}", e.error),
        Err(_) => format!("agent: HTTP {s}"),
    })
}

impl AgentClient {
    /// `url` is the agent's address, `http://host:port`.
    pub fn new(url: &str, key: &str) -> Self {
        Self {
            base: url.trim_end_matches('/').to_string(),
            key: key.to_string(),
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }

    fn req(&self, m: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(m, format!("{}{PREFIX}{path}", self.base))
            .header("x-api-key", &self.key)
    }

    pub async fn roots(&self) -> Result<Vec<AgentRoot>, String> {
        let r = self
            .req(reqwest::Method::GET, "/roots")
            .timeout(QUICK)
            .send()
            .await;
        check(r.map_err(|e| format!("agent: {e}"))?)
            .await?
            .json()
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        root: &str,
        what: &str,
        path: &str,
    ) -> Result<T, String> {
        let r = self
            .req(reqwest::Method::GET, &format!("/roots/{root}/{what}"))
            .query(&[("path", path)])
            .timeout(QUICK)
            .send()
            .await
            .map_err(|e| format!("agent: {e}"))?;
        check(r).await?.json().await.map_err(|e| e.to_string())
    }

    pub async fn list(&self, root: &str, path: &str) -> Result<Vec<Entry>, String> {
        self.get_json(root, "list", path).await
    }

    pub async fn stat(&self, root: &str, path: &str) -> Result<Entry, String> {
        self.get_json(root, "stat", path).await
    }

    /// A file, or the inclusive byte range `first..=last` (`last` `None` = to the end). Read the body with `chunk()` or `bytes_stream()`.
    pub async fn read(
        &self,
        root: &str,
        path: &str,
        range: Option<(u64, Option<u64>)>,
    ) -> Result<reqwest::Response, String> {
        let mut rb = self
            .req(reqwest::Method::GET, &format!("/roots/{root}/file"))
            .query(&[("path", path)]);
        if let Some((a, b)) = range {
            let b = b.map_or(String::new(), |b| b.to_string());
            rb = rb.header("range", format!("bytes={a}-{b}"));
        }
        check(rb.send().await.map_err(|e| format!("agent: {e}"))?).await
    }

    /// Create a file (refused if it exists). `body`: bytes, or a `tokio::fs::File` to stream.
    pub async fn write(
        &self,
        root: &str,
        path: &str,
        body: impl Into<reqwest::Body>,
    ) -> Result<(), String> {
        let r = self
            .req(reqwest::Method::PUT, &format!("/roots/{root}/file"))
            .query(&[("path", path)])
            .body(body)
            .send()
            .await
            .map_err(|e| format!("agent: {e}"))?;
        check(r).await.map(drop)
    }

    /// Rename inside one root (refused if the target exists).
    pub async fn rename(&self, root: &str, from: &str, to: &str) -> Result<(), String> {
        let r = self
            .req(reqwest::Method::POST, &format!("/roots/{root}/move"))
            .json(&Move {
                from: from.into(),
                to: to.into(),
            })
            .timeout(QUICK)
            .send()
            .await
            .map_err(|e| format!("agent: {e}"))?;
        check(r).await.map(drop)
    }
}
