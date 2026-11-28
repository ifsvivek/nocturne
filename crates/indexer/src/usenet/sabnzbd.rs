//! SABnzbd API (`/api?mode=...&output=json`): `addfile`, `queue`, `history`.

use super::{MB, UsenetItem, UsenetState, num, read_json, text};
use crate::indexer::{api_url, client};
use crate::{Error, Result};
use reqwest::Url;
use serde_json::Value;

#[derive(Clone)]
pub struct Sabnzbd {
    api: Url,
    key: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for Sabnzbd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sabnzbd")
            .field("api", &self.api.as_str())
            .field("key", &"<redacted>")
            .finish()
    }
}

fn state(s: &str) -> UsenetState {
    match s.to_ascii_lowercase().as_str() {
        "queued" | "grabbing" | "fetching" | "propagating" => UsenetState::Queued,
        "paused" => UsenetState::Paused,
        "downloading" => UsenetState::Downloading,
        "completed" => UsenetState::Completed,
        "failed" => UsenetState::Failed,
        _ => UsenetState::Processing,
    }
}

impl Sabnzbd {
    /// `url`: SABnzbd's address (`http://host:8080` or `.../sabnzbd`); `api_key`: from its Config > General.
    pub fn new(url: &str, api_key: &str) -> Result<Self> {
        if api_key.trim().is_empty() {
            return Err(Error::Config("enter the API key".into()));
        }
        Ok(Self {
            api: api_url(url)?,
            key: api_key.trim().to_string(),
            http: client()?,
        })
    }

    fn url(&self, mode: &str, extra: &[(&str, &str)]) -> Url {
        let mut u = self.api.clone();
        {
            let mut q = u.query_pairs_mut();
            q.append_pair("mode", mode).append_pair("output", "json");
            for (k, v) in extra {
                q.append_pair(k, v);
            }
            q.append_pair("apikey", &self.key);
        }
        u
    }

    /// SABnzbd answers 200 with `{"status": false, "error": ".."}` for a bad key.
    fn check(&self, v: Value) -> Result<Value> {
        if v["status"] == Value::Bool(false) || v.get("error").is_some_and(|e| !e.is_null()) {
            let e = text(&v["error"]).unwrap_or_default();
            if e.to_ascii_lowercase().contains("api key") {
                return Err(Error::Unauthorized);
            }
            return Err(Error::Indexer {
                code: 0,
                description: e.replace(&self.key, "<redacted>"),
            });
        }
        Ok(v)
    }

    async fn get(&self, mode: &str, extra: &[(&str, &str)]) -> Result<Value> {
        let r = self
            .http
            .get(self.url(mode, extra))
            .send()
            .await
            .map_err(|e| Error::Http(e.without_url()))?;
        self.check(read_json(r).await?)
    }

    pub async fn add_nzb(&self, name: &str, nzb: &[u8], category: Option<&str>) -> Result<String> {
        let file = if name.to_ascii_lowercase().ends_with(".nzb") {
            name.to_string()
        } else {
            format!("{name}.nzb")
        };
        let mut extra = vec![("nzbname", name)];
        if let Some(c) = category {
            extra.push(("cat", c));
        }
        let boundary = "nocturne-nzb-boundary-7d9f3c";
        let safe = file.replace(['"', '\r', '\n'], "_");
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"name\"; filename=\"{safe}\"\r\nContent-Type: application/x-nzb\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(nzb);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let r = self
            .http
            .post(self.url("addfile", &extra))
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await
            .map_err(|e| Error::Http(e.without_url()))?;
        let v = self.check(read_json(r).await?)?;
        v["nzo_ids"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(text)
            .ok_or_else(|| Error::Parse("SABnzbd returned no job id".into()))
    }

    pub async fn queue(&self) -> Result<Vec<UsenetItem>> {
        let v = self.get("queue", &[]).await?;
        Ok(v["queue"]["slots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let mb = num(&s["mb"]);
                let left = num(&s["mbleft"]);
                Some(UsenetItem {
                    id: text(&s["nzo_id"])?,
                    name: text(&s["filename"]).unwrap_or_default(),
                    state: state(s["status"].as_str().unwrap_or_default()),
                    progress: (num(&s["percentage"]).unwrap_or(0.0) / 100.0).clamp(0.0, 1.0) as f32,
                    size: mb.map(|m| (m * MB) as u64),
                    left: left.map(|m| (m * MB) as u64),
                    category: text(&s["cat"]),
                    path: None,
                    error: None,
                })
            })
            .collect())
    }

    pub async fn history(&self) -> Result<Vec<UsenetItem>> {
        let v = self.get("history", &[("limit", "100")]).await?;
        Ok(v["history"]["slots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let st = state(s["status"].as_str().unwrap_or_default());
                Some(UsenetItem {
                    id: text(&s["nzo_id"])?,
                    name: text(&s["name"]).unwrap_or_default(),
                    state: st,
                    progress: if st == UsenetState::Completed {
                        1.0
                    } else {
                        0.0
                    },
                    size: num(&s["bytes"]).map(|b| b as u64),
                    left: None,
                    category: text(&s["category"]),
                    path: text(&s["storage"]),
                    error: text(&s["fail_message"]),
                })
            })
            .collect())
    }
}
