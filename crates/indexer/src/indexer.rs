//! The Torznab/Newznab HTTP client.

use crate::xml::{parse_caps, parse_results};
use crate::{Error, MediaKind, Protocol, Release, Result};
use reqwest::{StatusCode, Url, redirect};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Largest response read (RSS page, caps, `.torrent`, `.nzb`).
pub const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_BACKOFF: Duration = Duration::from_secs(3600);

/// What a feed can do (`t=caps`). A mode is `None` when the feed does not offer it; otherwise it lists the supported parameters (lowercase: `q`, `imdbid`, `tmdbid`, `tvdbid`, `season`, `ep`, `artist`, `album`, `author`, `title`, ...).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Caps {
    pub limit_max: Option<u32>,
    pub limit_default: Option<u32>,
    pub search: Option<Vec<String>>,
    pub tv: Option<Vec<String>>,
    pub movie: Option<Vec<String>>,
    pub music: Option<Vec<String>>,
    pub book: Option<Vec<String>>,
    pub categories: Vec<Category>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub id: u32,
    pub name: String,
    pub subcats: Vec<Category>,
}

/// Torznab search function (`t=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    Search,
    Tv,
    Movie,
    Music,
    Book,
}

impl SearchMode {
    fn t(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Tv => "tvsearch",
            Self::Movie => "movie",
            Self::Music => "music",
            Self::Book => "book",
        }
    }

    fn of(kind: MediaKind) -> Self {
        match kind {
            MediaKind::Movie => Self::Movie,
            MediaKind::Tv => Self::Tv,
            MediaKind::Music => Self::Music,
            MediaKind::Book => Self::Book,
        }
    }
}

impl Caps {
    /// Parameters of a mode, `None` if the feed lacks it.
    pub fn params(&self, mode: SearchMode) -> Option<&[String]> {
        match mode {
            SearchMode::Search => self.search.as_deref(),
            SearchMode::Tv => self.tv.as_deref(),
            SearchMode::Movie => self.movie.as_deref(),
            SearchMode::Music => self.music.as_deref(),
            SearchMode::Book => self.book.as_deref(),
        }
    }

    pub fn supports(&self, mode: SearchMode, param: &str) -> bool {
        self.params(mode)
            .is_some_and(|p| p.iter().any(|x| x == param))
    }
}

/// A search. Fill what you know; [`Indexer::search`] picks the parameters the feed supports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchQuery {
    pub kind: Option<MediaKind>,
    /// Free text: the movie or series title, or a whole query for a plain search.
    pub text: Option<String>,
    pub year: Option<u16>,
    /// `tt` + digits.
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<u32>,
    pub tvdb_id: Option<u32>,
    pub season: Option<u32>,
    /// Episode number, or `MM/DD` for daily shows (with `season` = year).
    pub episode: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub author: Option<String>,
    pub title: Option<String>,
    /// Empty: the kind's standard categories.
    pub categories: Vec<u32>,
    pub offset: u32,
    pub limit: Option<u32>,
}

/// One page of results. More pages exist while `offset + count < total` (if the feed reports a total) or while a page came back full.
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub releases: Vec<Release>,
    pub offset: u32,
    pub count: u32,
    pub total: Option<u32>,
}

impl Page {
    /// Offset of the next page, `None` at the end. `limit`: the page size that was requested.
    pub fn next_offset(&self, limit: u32) -> Option<u32> {
        let next = self.offset + self.count;
        match self.total {
            Some(t) => (next < t && self.count > 0).then_some(next),
            None => (self.count >= limit && limit > 0).then_some(next),
        }
    }
}

/// A fetched download link.
#[derive(Clone, PartialEq)]
pub enum Download {
    /// `.torrent` or `.nzb` contents.
    File(Vec<u8>),
    /// The feed redirected to a magnet link (Jackett and Prowlarr do for magnet-only trackers).
    Magnet(String),
}

impl std::fmt::Debug for Download {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(b) => write!(f, "File({} bytes)", b.len()),
            Self::Magnet(_) => f.write_str("Magnet(..)"),
        }
    }
}

#[derive(Debug)]
struct Limiter {
    next: Instant,
    failures: u32,
    blocked_until: Option<Instant>,
}

/// One user-added Torznab or Newznab feed. Cheap to clone (clones share the rate limiter and the caps cache).
#[derive(Clone)]
pub struct Indexer {
    id: i64,
    protocol: Protocol,
    api: Url,
    key: String,
    http: reqwest::Client,
    min_interval: Duration,
    limiter: Arc<Mutex<Limiter>>,
    caps: Arc<Mutex<Option<Caps>>>,
}

impl std::fmt::Debug for Indexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Indexer")
            .field("id", &self.id)
            .field("protocol", &self.protocol)
            .field("api", &self.api.as_str())
            .field("key", &"<redacted>")
            .finish()
    }
}

fn http_err(e: reqwest::Error) -> Error {
    if e.is_redirect() {
        return Error::Redirect;
    }
    Error::Http(e.without_url())
}

/// The feed's API endpoint: the URL as given if its path ends in `/api`, otherwise with `/api` appended (Jackett `.../results/torznab/`, Prowlarr `http://host:9696/1/`, a Newznab site's root).
pub(crate) fn api_url(url: &str) -> Result<Url> {
    let mut u = Url::parse(url.trim())
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
        .ok_or_else(|| Error::Config("enter an http(s) address".into()))?;
    u.set_query(None);
    u.set_fragment(None);
    let p = u.path().trim_end_matches('/').to_string();
    if !p.ends_with("/api") {
        u.set_path(&format!("{p}/api"));
    }
    Ok(u)
}

/// Follows at most 5 redirects, only to http(s) on the same host; stops at a magnet so its address can be read.
pub(crate) fn redirect_policy() -> redirect::Policy {
    redirect::Policy::custom(|a| {
        let same = a.previous().first().and_then(|u| u.host_str()) == a.url().host_str();
        if a.url().scheme() == "magnet" {
            a.stop()
        } else if a.previous().len() > 5 || !same || !matches!(a.url().scheme(), "http" | "https") {
            a.error("redirect refused")
        } else {
            a.follow()
        }
    })
}

pub(crate) fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(redirect_policy())
        .user_agent(concat!("NOCTURNE/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(http_err)
}

/// Body of a response, at most [`MAX_RESPONSE`] bytes.
pub(crate) async fn read_capped(mut r: reqwest::Response) -> Result<Vec<u8>> {
    if r.content_length().is_some_and(|l| l > MAX_RESPONSE as u64) {
        return Err(Error::TooLarge(MAX_RESPONSE));
    }
    let mut out = Vec::new();
    while let Some(c) = r.chunk().await.map_err(http_err)? {
        if out.len() + c.len() > MAX_RESPONSE {
            return Err(Error::TooLarge(MAX_RESPONSE));
        }
        out.extend_from_slice(&c);
    }
    Ok(out)
}

impl Indexer {
    /// `url`: the feed address as the user entered it; `api_key`: its key. `min_interval`: least time between two requests to this feed (Prowlarr and Jackett feeds of public trackers want a few seconds).
    pub fn new(
        id: i64,
        url: &str,
        api_key: &str,
        protocol: Protocol,
        min_interval: Duration,
    ) -> Result<Self> {
        let key = api_key.trim();
        if key.is_empty() {
            return Err(Error::Config("enter the API key".into()));
        }
        Ok(Self {
            id,
            protocol,
            api: api_url(url)?,
            key: key.to_string(),
            http: client()?,
            min_interval,
            limiter: Arc::new(Mutex::new(Limiter {
                next: Instant::now(),
                failures: 0,
                blocked_until: None,
            })),
            caps: Arc::default(),
        })
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Wait for this feed's slot; fail fast while backing off.
    async fn slot(&self) -> Result<()> {
        let wait = {
            let mut l = self.limiter.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            if let Some(b) = l.blocked_until.filter(|b| *b > now) {
                return Err(Error::Backoff(b - now));
            }
            let at = l.next.max(now);
            l.next = at + self.min_interval;
            at - now
        };
        tokio::time::sleep(wait).await;
        Ok(())
    }

    /// Record a response status; 429 and 5xx back off (the server's `Retry-After` seconds, else 1, 2, 4 ... minutes up to an hour).
    fn record(&self, status: StatusCode, retry_after: Option<u64>) {
        let mut l = self.limiter.lock().unwrap_or_else(|e| e.into_inner());
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            l.failures = (l.failures + 1).min(16);
            let exp = Duration::from_secs(60 << (l.failures - 1).min(6));
            let d = retry_after
                .map_or(exp, Duration::from_secs)
                .min(MAX_BACKOFF);
            l.blocked_until = Some(Instant::now() + d);
        } else {
            l.failures = 0;
            l.blocked_until = None;
        }
    }

    /// Error text from the server with the key removed, in case a feed echoes it.
    fn scrub(&self, e: Error) -> Error {
        match e {
            Error::Indexer { code, description } => Error::Indexer {
                code,
                description: description.replace(&self.key, "<redacted>"),
            },
            e => e,
        }
    }

    async fn get(&self, query: &[(&str, String)]) -> Result<String> {
        self.slot().await?;
        let mut u = self.api.clone();
        {
            let mut q = u.query_pairs_mut();
            for (k, v) in query {
                q.append_pair(k, v);
            }
            q.append_pair("apikey", &self.key);
        }
        let r = self.http.get(u).send().await.map_err(http_err)?;
        let status = r.status();
        let retry = r
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok());
        self.record(status, retry);
        match status {
            s if s.is_success() => {}
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(Error::Unauthorized),
            s => return Err(Error::Status(s.as_u16())),
        }
        let body = read_capped(r).await?;
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    /// The feed's capabilities, fetched once and cached.
    pub async fn caps(&self) -> Result<Caps> {
        if let Some(c) = self.caps.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(c);
        }
        let body = self.get(&[("t", "caps".into())]).await?;
        let c = parse_caps(&body).map_err(|e| self.scrub(e))?;
        *self.caps.lock().unwrap_or_else(|e| e.into_inner()) = Some(c.clone());
        Ok(c)
    }

    async fn page(
        &self,
        t: SearchMode,
        mut params: Vec<(&str, String)>,
        q: &SearchQuery,
    ) -> Result<Page> {
        params.insert(0, ("t", t.t().into()));
        let cats: Vec<u32> = if q.categories.is_empty() {
            q.kind
                .map(|k| k.default_categories().to_vec())
                .unwrap_or_default()
        } else {
            q.categories.clone()
        };
        if !cats.is_empty() {
            let c: Vec<String> = cats.iter().map(u32::to_string).collect();
            params.push(("cat", c.join(",")));
        }
        params.push(("extended", "1".into()));
        if q.offset > 0 {
            params.push(("offset", q.offset.to_string()));
        }
        if let Some(l) = q.limit {
            params.push(("limit", l.to_string()));
        }
        let body = self.get(&params).await?;
        parse_results(&body, self.id, self.protocol).map_err(|e| self.scrub(e))
    }

    /// Search: by ids first when the feed supports one the query has, then (on the first page, if that found nothing) by text. Falls back to `t=search` when the feed lacks the kind's mode.
    pub async fn search(&self, q: &SearchQuery) -> Result<Page> {
        let caps = self.caps().await?;
        let mode = q
            .kind
            .map(SearchMode::of)
            .filter(|m| caps.params(*m).is_some())
            .unwrap_or(SearchMode::Search);
        let (ids, text) = plan(&caps, mode, q);
        if !ids.is_empty() {
            let p = self.page(mode, ids, q).await?;
            if !p.releases.is_empty() || q.offset > 0 || text.is_empty() {
                return Ok(p);
            }
        }
        self.page(mode, text, q).await
    }

    /// Newest releases (the RSS feed): `t=search` with no query.
    pub async fn latest(
        &self,
        categories: &[u32],
        offset: u32,
        limit: Option<u32>,
    ) -> Result<Page> {
        let q = SearchQuery {
            categories: categories.to_vec(),
            offset,
            limit,
            ..SearchQuery::default()
        };
        self.page(SearchMode::Search, Vec::new(), &q).await
    }

    /// Fetch a release's `.torrent`/`.nzb` link as the feed gave it (same rate limit, size cap and redirect rules: http(s) only, redirects only within the link's host), or the magnet it redirects to.
    pub async fn download(&self, url: &str) -> Result<Download> {
        if url.starts_with("magnet:") {
            return Ok(Download::Magnet(url.to_string()));
        }
        let u = Url::parse(url)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"))
            .ok_or_else(|| Error::Config("bad download link".into()))?;
        self.slot().await?;
        let r = self.http.get(u).send().await.map_err(http_err)?;
        let status = r.status();
        self.record(status, None);
        if status.is_redirection() {
            let loc = r
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .filter(|l| l.starts_with("magnet:"));
            return loc
                .map(|l| Download::Magnet(l.to_string()))
                .ok_or(Error::Redirect);
        }
        match status {
            s if s.is_success() => Ok(Download::File(read_capped(r).await?)),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(Error::Unauthorized),
            s => Err(Error::Status(s.as_u16())),
        }
    }
}

type Params = Vec<(&'static str, String)>;

/// Query parameters for the id search and the text search (either may be empty).
pub(crate) fn plan(caps: &Caps, mode: SearchMode, q: &SearchQuery) -> (Params, Params) {
    let has = |p: &str| caps.supports(mode, p);
    let mut ids = Vec::new();
    if let Some(i) = q.imdb_id.as_deref().filter(|_| has("imdbid")) {
        ids.push(("imdbid", i.trim_start_matches("tt").to_string()));
    }
    if let Some(i) = q.tmdb_id.filter(|_| has("tmdbid")) {
        ids.push(("tmdbid", i.to_string()));
    }
    if let Some(i) = q.tvdb_id.filter(|_| has("tvdbid")) {
        ids.push(("tvdbid", i.to_string()));
    }
    let mut ep = Vec::new();
    if mode == SearchMode::Tv {
        if let Some(s) = q.season.filter(|_| has("season")) {
            ep.push(("season", s.to_string()));
        }
        if let Some(e) = q.episode.as_ref().filter(|_| has("ep")) {
            ep.push(("ep", e.clone()));
        }
    }
    if !ids.is_empty() {
        ids.extend(ep.iter().cloned());
    }

    let mut text = Vec::new();
    let mut words: Vec<String> = q.text.iter().cloned().collect();
    let mut field = |name: &'static str, v: &Option<String>, words: &mut Vec<String>| {
        if let Some(v) = v {
            if has(name) {
                text.push((name, v.clone()));
            } else {
                words.push(v.clone());
            }
        }
    };
    field("artist", &q.artist, &mut words);
    field("album", &q.album, &mut words);
    field("author", &q.author, &mut words);
    field("title", &q.title, &mut words);
    match mode {
        SearchMode::Tv if ep.len() == 2 || (ep.len() == 1 && q.episode.is_none()) => {
            text.extend(ep)
        }
        SearchMode::Tv => {
            // The feed cannot take season/ep: put them in the text the way release names do.
            match (q.season, q.episode.as_deref()) {
                (Some(s), Some(e)) => match e.parse::<u32>() {
                    Ok(e) => words.push(format!("S{s:02}E{e:02}")),
                    Err(_) => words.push(format!("{s} {}", e.replace('/', " "))),
                },
                (Some(s), None) => words.push(format!("S{s:02}")),
                _ => {}
            }
        }
        SearchMode::Movie => {
            if let Some(y) = q.year {
                words.push(y.to_string());
            }
        }
        _ => {}
    }
    let words = words.join(" ");
    if !words.trim().is_empty() {
        text.insert(0, ("q", words.trim().to_string()));
    }
    (ids, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(tv: &str, movie: &str) -> Caps {
        let p = |s: &str| Some(s.split(',').map(str::to_string).collect());
        Caps {
            search: p("q"),
            tv: p(tv),
            movie: p(movie),
            ..Caps::default()
        }
    }

    #[test]
    fn api_urls() {
        let a = |u| api_url(u).map(|u| u.to_string()).ok();
        assert_eq!(
            a("https://nzb.example/").as_deref(),
            Some("https://nzb.example/api")
        );
        assert_eq!(
            a("http://127.0.0.1:9696/1/api").as_deref(),
            Some("http://127.0.0.1:9696/1/api")
        );
        assert_eq!(
            a("http://h:9117/api/v2.0/indexers/x/results/torznab/").as_deref(),
            Some("http://h:9117/api/v2.0/indexers/x/results/torznab/api")
        );
        assert!(a("ftp://x/").is_none());
        assert!(a("file:///etc/passwd").is_none());
    }

    #[test]
    fn plan_ids_first_then_text() {
        let c = caps("q,season,ep,tvdbid", "q,imdbid");
        let q = SearchQuery {
            kind: Some(MediaKind::Tv),
            text: Some("Some Show".into()),
            tvdb_id: Some(42),
            season: Some(2),
            episode: Some("5".into()),
            ..SearchQuery::default()
        };
        let (ids, text) = plan(&c, SearchMode::Tv, &q);
        assert_eq!(
            ids,
            vec![
                ("tvdbid", "42".into()),
                ("season", "2".into()),
                ("ep", "5".into())
            ]
        );
        assert_eq!(
            text,
            vec![
                ("q", "Some Show".into()),
                ("season", "2".into()),
                ("ep", "5".into())
            ]
        );
    }

    #[test]
    fn plan_without_params_uses_release_style_text() {
        let c = caps("q", "q");
        let q = SearchQuery {
            kind: Some(MediaKind::Tv),
            text: Some("Some Show".into()),
            imdb_id: Some("tt1".into()),
            season: Some(1),
            episode: Some("3".into()),
            ..SearchQuery::default()
        };
        let (ids, text) = plan(&c, SearchMode::Tv, &q);
        assert!(ids.is_empty());
        assert_eq!(text, vec![("q", "Some Show S01E03".into())]);
        let m = SearchQuery {
            kind: Some(MediaKind::Movie),
            text: Some("A Film".into()),
            year: Some(1999),
            imdb_id: Some("tt0133093".into()),
            ..SearchQuery::default()
        };
        let (ids, text) = plan(&caps("q", "q,imdbid"), SearchMode::Movie, &m);
        assert_eq!(ids, vec![("imdbid", "0133093".into())]);
        assert_eq!(text, vec![("q", "A Film 1999".into())]);
    }

    #[test]
    fn next_offset() {
        let p = Page {
            offset: 0,
            count: 100,
            total: Some(250),
            ..Page::default()
        };
        assert_eq!(p.next_offset(100), Some(100));
        let p = Page {
            offset: 200,
            count: 50,
            total: Some(250),
            ..Page::default()
        };
        assert_eq!(p.next_offset(100), None);
        let p = Page {
            offset: 0,
            count: 100,
            total: None,
            ..Page::default()
        };
        assert_eq!(p.next_offset(100), Some(100));
        let p = Page {
            offset: 0,
            count: 7,
            total: None,
            ..Page::default()
        };
        assert_eq!(p.next_offset(100), None);
    }
}
