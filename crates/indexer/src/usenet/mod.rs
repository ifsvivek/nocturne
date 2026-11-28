//! Usenet hand-off. NOCTURNE has no NNTP downloader: a Usenet release's NZB is fetched with [`crate::Indexer::download`] and handed to the user's own SABnzbd or NZBGet, whose queue and history the daemon polls. Without a configured client the UI hides Usenet results.

mod nzbget;
mod sabnzbd;

pub use nzbget::Nzbget;
pub use sabnzbd::Sabnzbd;

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsenetState {
    Queued,
    Paused,
    Downloading,
    /// Verifying, repairing, unpacking, moving.
    Processing,
    Completed,
    Failed,
}

/// A job in the client's queue or history.
#[derive(Clone, Debug, PartialEq)]
pub struct UsenetItem {
    /// SABnzbd `nzo_id` or NZBGet `NZBID`.
    pub id: String,
    pub name: String,
    pub state: UsenetState,
    /// 0.0 ..= 1.0
    pub progress: f32,
    pub size: Option<u64>,
    pub left: Option<u64>,
    pub category: Option<String>,
    /// Final folder of a completed job.
    pub path: Option<String>,
    pub error: Option<String>,
}

/// The user's download client.
#[derive(Clone, Debug)]
pub enum UsenetClient {
    Sabnzbd(Sabnzbd),
    Nzbget(Nzbget),
}

impl UsenetClient {
    /// Add an NZB (its contents, not the indexer link, so the indexer key never leaves NOCTURNE). Returns the client's job id.
    pub async fn add_nzb(&self, name: &str, nzb: &[u8], category: Option<&str>) -> Result<String> {
        match self {
            Self::Sabnzbd(c) => c.add_nzb(name, nzb, category).await,
            Self::Nzbget(c) => c.add_nzb(name, nzb, category).await,
        }
    }

    /// Jobs still downloading or processing.
    pub async fn queue(&self) -> Result<Vec<UsenetItem>> {
        match self {
            Self::Sabnzbd(c) => c.queue().await,
            Self::Nzbget(c) => c.queue().await,
        }
    }

    /// Finished and failed jobs, newest first.
    pub async fn history(&self) -> Result<Vec<UsenetItem>> {
        match self {
            Self::Sabnzbd(c) => c.history().await,
            Self::Nzbget(c) => c.history().await,
        }
    }
}

const MB: f64 = 1024.0 * 1024.0;

pub(crate) async fn read_json(r: reqwest::Response) -> Result<serde_json::Value> {
    match r.status() {
        s if s.is_success() => {}
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            return Err(Error::Unauthorized);
        }
        s => return Err(Error::Status(s.as_u16())),
    }
    let body = crate::indexer::read_capped(r).await?;
    serde_json::from_slice(&body).map_err(|e| Error::Parse(e.to_string()))
}

/// A JSON number or numeric string.
pub(crate) fn num(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())
}

pub(crate) fn text(v: &serde_json::Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
