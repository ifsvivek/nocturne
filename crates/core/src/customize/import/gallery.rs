//! The themes gallery: an index (`index.json`) and, per theme, a stylesheet and a preview, all served by the NOCTURNE website (`scripts/themes-index.py` writes them from `themes/`). Same download rules as the rest of this module (https only, no credentials, cookies or `Referer`, generic User-Agent, 10 s, 3 redirects each checked again) plus: only with `remote_css` on, every file on the index's own host (the index lists relative addresses), the index at most 256 KB, a stylesheet at most 256 KB and its sha256 must match the index, a preview at most 512 KB and must be a WebP, PNG or JPEG. The index and previews are cached on disk; the index is read again after a day or on a manual refresh, and the cached copy stands in while offline. Run through `bridge::spawn`.
use super::{AGENT, Url, cache_read, cache_write, check_url, default_cache_dir, describe, norm};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_URL: &str = "https://nocturne.ifsvivek.in/themes/index.json";
const MAX_INDEX: usize = 256 * 1024;
const MAX_CSS: usize = 256 * 1024;
const MAX_PREVIEW: usize = 512 * 1024;
const MAX_AGE: u64 = 24 * 3600;
pub const OFF: &str = "The gallery downloads from the web, and Settings > Themes > Themes may load from the web is off.";

/// One theme of the gallery, as the index lists it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub licence: String,
    /// `official` or `community`.
    pub kind: String,
    pub min_version: String,
    /// Relative to the index address.
    pub css: String,
    pub preview: String,
    pub sha256: String,
}

impl Entry {
    pub fn official(&self) -> bool {
        self.kind == "official"
    }

    /// Does this NOCTURNE already know what the theme needs?
    pub fn compatible(&self) -> bool {
        let v = |s: &str| -> Vec<u32> { s.split('.').filter_map(|p| p.parse().ok()).collect() };
        v(&self.min_version) <= v(env!("CARGO_PKG_VERSION"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub entries: Vec<Entry>,
    /// When the index was downloaded (Unix seconds).
    pub fetched: u64,
    /// The download failed and this is the cached copy.
    pub stale: bool,
}

/// Where the gallery is. Debug builds may point `NOCTURNE_THEMES_URL` at another address (http allowed for loopback only); the flag says so.
pub fn index_url() -> (String, bool) {
    #[cfg(debug_assertions)]
    if let Ok(u) = std::env::var("NOCTURNE_THEMES_URL")
        && !u.trim().is_empty()
    {
        return (u.trim().to_string(), true);
    }
    (DEFAULT_URL.to_string(), false)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn parse_index(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    #[derive(Deserialize)]
    struct Doc {
        version: u32,
        themes: Vec<Entry>,
    }
    let d: Doc = serde_json::from_slice(bytes).map_err(|_| "the gallery index is not valid")?;
    if d.version != 1 {
        return Err("the gallery needs a newer NOCTURNE".into());
    }
    let ok = |e: &Entry| {
        !e.id.is_empty()
            && e.id.len() <= 40
            && e.id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && e.sha256.len() == 64
            && e.sha256.chars().all(|c| c.is_ascii_hexdigit())
            && !e.name.is_empty()
            && matches!(e.kind.as_str(), "official" | "community")
    };
    // A bad entry is skipped, not fatal: one broken pull request must not empty the gallery.
    Ok(d.themes.into_iter().filter(ok).collect())
}

fn client(allow: bool) -> Result<reqwest::Client, String> {
    let policy = reqwest::redirect::Policy::custom(move |a| {
        if a.previous().len() > 3 || check_url(a.url(), allow).is_err() {
            a.stop()
        } else {
            a.follow()
        }
    });
    reqwest::Client::builder()
        .user_agent(AGENT)
        .referer(false)
        .redirect(policy)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "no http client".to_string())
}

async fn get(url: &Url, max: usize, allow: bool) -> Result<Vec<u8>, String> {
    check_url(url, allow)?;
    let c = client(allow)?;
    let get = async {
        let mut r = c
            .get(url.clone())
            .send()
            .await
            .map_err(|e| describe(&e).to_string())?;
        if r.status().is_redirection() {
            return Err("redirect refused (only https, at most 3)".to_string());
        }
        if !r.status().is_success() {
            return Err(format!("the server answered {}", r.status().as_u16()));
        }
        let big = || format!("larger than {} KB", max / 1024);
        if r.content_length().is_some_and(|n| n as usize > max) {
            return Err(big());
        }
        let mut body = Vec::new();
        while let Some(chunk) = r.chunk().await.map_err(|e| describe(&e).to_string())? {
            body.extend_from_slice(&chunk);
            if body.len() > max {
                return Err(big());
            }
        }
        Ok(body)
    };
    tokio::time::timeout(Duration::from_secs(10), get)
        .await
        .unwrap_or_else(|_| Err("timed out".into()))
}

fn cache_dir() -> Option<std::path::PathBuf> {
    default_cache_dir().map(|d| d.with_file_name("gallery"))
}

/// The index: from the cache when it is under a day old, else downloaded (the cached copy stands in when that fails). `refresh` always downloads. `cache` off (tests) skips the disk.
pub async fn load_index(
    url: &str,
    refresh: bool,
    remote: bool,
    allow_loopback: bool,
    cache: bool,
) -> Result<Loaded, String> {
    if !remote {
        return Err(OFF.into());
    }
    let u = Url::parse(url).map_err(|_| "the gallery address is not valid".to_string())?;
    let dir = cache.then(cache_dir).flatten();
    let key = norm(&u);
    let cached = dir.as_deref().and_then(|d| cache_read(d, &key));
    let from_cache = |c: crate::customize::import::Cached, stale: bool| {
        parse_index(&c.bytes).map(|entries| Loaded {
            entries,
            fetched: c.fetched,
            stale,
        })
    };
    if !refresh
        && let Some(c) = cached
            .as_ref()
            .filter(|c| now().saturating_sub(c.fetched) < MAX_AGE)
        && let Ok(l) = from_cache(c.clone(), false)
    {
        return Ok(l);
    }
    match get(&u, MAX_INDEX, allow_loopback).await {
        Ok(b) => {
            let entries = parse_index(&b)?;
            if let Some(d) = &dir {
                cache_write(d, &key, &key, &b);
            }
            Ok(Loaded {
                entries,
                fetched: now(),
                stale: false,
            })
        }
        Err(e) => match cached.map(|c| from_cache(c, true)) {
            Some(Ok(l)) => Ok(l),
            _ => Err(format!("Could not reach the gallery: {e}")),
        },
    }
}

/// `rel` on the index's own host, nowhere else.
fn sibling(index: &str, rel: &str) -> Result<Url, String> {
    let base = Url::parse(index).map_err(|_| "the gallery address is not valid".to_string())?;
    let u = base.join(rel).map_err(|_| "not a valid address")?;
    let same = u.scheme() == base.scheme()
        && u.host_str() == base.host_str()
        && u.port_or_known_default() == base.port_or_known_default();
    if same {
        Ok(u)
    } else {
        Err("the index points at another site".into())
    }
}

/// The theme's stylesheet, checked against the index's sha256 (any mismatch is refused: nothing is saved).
pub async fn fetch_css(
    index: &str,
    e: &Entry,
    remote: bool,
    allow_loopback: bool,
) -> Result<String, String> {
    if !remote {
        return Err(OFF.into());
    }
    let b = get(&sibling(index, &e.css)?, MAX_CSS, allow_loopback).await?;
    if !hex(&Sha256::digest(&b)).eq_ignore_ascii_case(&e.sha256) {
        return Err(
            "The download does not match the gallery's checksum, so it was not installed.".into(),
        );
    }
    String::from_utf8(b).map_err(|_| "the stylesheet is not text".to_string())
}

/// The preview picture's bytes (WebP, PNG or JPEG by their first bytes), cached.
pub async fn fetch_preview(
    index: &str,
    e: &Entry,
    refresh: bool,
    remote: bool,
    allow_loopback: bool,
    cache: bool,
) -> Result<Vec<u8>, String> {
    if !remote {
        return Err(OFF.into());
    }
    let u = sibling(index, &e.preview)?;
    let key = norm(&u);
    let dir = cache.then(cache_dir).flatten();
    if !refresh
        && let Some(c) = dir.as_deref().and_then(|d| cache_read(d, &key))
        && c.bytes.len() <= MAX_PREVIEW
    {
        return Ok(c.bytes);
    }
    let b = get(&u, MAX_PREVIEW, allow_loopback).await?;
    if super::pics::sniff(&b) != Some(false) {
        return Err("not a WebP, PNG or JPEG picture".into());
    }
    if let Some(d) = &dir {
        cache_write(d, &key, &key, &b);
    }
    Ok(b)
}
