//! NZBGet JSON-RPC (`/jsonrpc`, HTTP basic auth): `append`, `listgroups`, `history`.

use super::{MB, UsenetItem, UsenetState, num, read_json, text};
use crate::indexer::client;
use crate::{Error, Result};
use base64::Engine;
use reqwest::Url;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct Nzbget {
    rpc: Url,
    user: String,
    password: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for Nzbget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nzbget")
            .field("rpc", &self.rpc.as_str())
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .finish()
    }
}

fn state(s: &str) -> UsenetState {
    let s = s.to_ascii_uppercase();
    match s.split('/').next().unwrap_or_default() {
        "QUEUED" | "FETCHING" => UsenetState::Queued,
        "PAUSED" => UsenetState::Paused,
        "DOWNLOADING" => UsenetState::Downloading,
        "SUCCESS" | "WARNING" => UsenetState::Completed,
        "FAILURE" | "DELETED" => UsenetState::Failed,
        _ => UsenetState::Processing,
    }
}

impl Nzbget {
    /// `url`: NZBGet's address (`http://host:6789`); `user`/`password`: its control account.
    pub fn new(url: &str, user: &str, password: &str) -> Result<Self> {
        let mut rpc = Url::parse(url.trim())
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
            .ok_or_else(|| Error::Config("enter an http(s) address".into()))?;
        rpc.set_query(None);
        rpc.set_fragment(None);
        let p = rpc
            .path()
            .trim_end_matches('/')
            .trim_end_matches("/jsonrpc")
            .to_string();
        rpc.set_path(&format!("{p}/jsonrpc"));
        Ok(Self {
            rpc,
            user: user.to_string(),
            password: password.to_string(),
            http: client()?,
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let r = self
            .http
            .post(self.rpc.clone())
            .basic_auth(&self.user, Some(&self.password))
            .json(&json!({ "version": "1.1", "method": method, "params": params, "id": 1 }))
            .send()
            .await
            .map_err(|e| Error::Http(e.without_url()))?;
        let mut v = read_json(r).await?;
        if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
            return Err(Error::Indexer {
                code: e["code"].as_u64().unwrap_or(0) as u32,
                description: text(&e["message"]).unwrap_or_default(),
            });
        }
        Ok(v["result"].take())
    }

    pub async fn add_nzb(&self, name: &str, nzb: &[u8], category: Option<&str>) -> Result<String> {
        let file = if name.to_ascii_lowercase().ends_with(".nzb") {
            name.to_string()
        } else {
            format!("{name}.nzb")
        };
        let content = base64::engine::general_purpose::STANDARD.encode(nzb);
        // NZBFilename, Content, Category, Priority, AddToTop, AddPaused, DupeKey, DupeScore, DupeMode, PPParameters (NZBGet 16+).
        let params = json!([
            file,
            content,
            category.unwrap_or(""),
            0,
            false,
            false,
            "",
            0,
            "SCORE",
            []
        ]);
        let id = self.call("append", params).await?.as_i64().unwrap_or(0);
        if id <= 0 {
            return Err(Error::Indexer {
                code: 0,
                description: "NZBGet refused the NZB".into(),
            });
        }
        Ok(id.to_string())
    }

    pub async fn queue(&self) -> Result<Vec<UsenetItem>> {
        let v = self.call("listgroups", json!([0])).await?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| {
                let size = num(&g["FileSizeMB"]).unwrap_or(0.0);
                let left = num(&g["RemainingSizeMB"]).unwrap_or(0.0);
                Some(UsenetItem {
                    id: num(&g["NZBID"])?.to_string(),
                    name: text(&g["NZBName"]).unwrap_or_default(),
                    state: state(g["Status"].as_str().unwrap_or_default()),
                    progress: if size > 0.0 {
                        ((size - left) / size).clamp(0.0, 1.0) as f32
                    } else {
                        0.0
                    },
                    size: Some((size * MB) as u64),
                    left: Some((left * MB) as u64),
                    category: text(&g["Category"]),
                    path: None,
                    error: None,
                })
            })
            .collect())
    }

    pub async fn history(&self) -> Result<Vec<UsenetItem>> {
        let v = self.call("history", json!([false])).await?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| {
                let status = h["Status"].as_str().unwrap_or_default();
                let st = state(status);
                Some(UsenetItem {
                    id: num(&h["NZBID"])?.to_string(),
                    name: text(&h["Name"]).unwrap_or_default(),
                    state: st,
                    progress: if st == UsenetState::Completed {
                        1.0
                    } else {
                        0.0
                    },
                    size: num(&h["FileSizeMB"]).map(|m| (m * MB) as u64),
                    left: None,
                    category: text(&h["Category"]),
                    path: text(&h["FinalDir"]).or_else(|| text(&h["DestDir"])),
                    error: (st == UsenetState::Failed).then(|| status.to_string()),
                })
            })
            .collect())
    }
}
