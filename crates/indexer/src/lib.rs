//! Indexers and releases for NOCTURNE's native *arr stack.
//!
//! - [`Indexer`]: a Torznab (torrent) or Newznab (Usenet) feed the user added by URL and API key (Jackett and Prowlarr feeds included). NOCTURNE ships no site definitions and scrapes nothing. GET only, http(s) only, no redirects to another host, per-indexer rate limit and backoff, response size cap; the API key is redacted from `Debug` and from every error.
//! - [`parse`]: pure release-name parser (movies, episodes, music, books) and the normalised [`Quality`].
//! - [`profile`]: quality profiles and [`score`], which decides whether a release is acceptable and how it ranks.
//! - [`usenet`]: there is no native NNTP downloader. Usenet releases ([`Release::needs_usenet_client`]) go to the user's SABnzbd or NZBGet if one is configured; without one the UI hides them.

mod indexer;
pub mod parse;
pub mod profile;
pub mod usenet;
mod xml;

pub use indexer::{Caps, Category, Download, Indexer, Page, SearchMode, SearchQuery};
pub use parse::{
    ParsedBook, ParsedEpisode, ParsedMovie, ParsedMusic, Quality, QualityInfo, normalize_title,
    parse_book, parse_episode, parse_movie, parse_music, parse_quality,
};
pub use profile::{Context, Decision, Profile, SizeLimit, Term, quality_of, score, score_with};
pub use xml::{parse_caps, parse_results};

/// Error of an indexer or download-client call. Never contains the API key or a full request URL.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Config(String),
    #[error("network error: {0}")]
    Http(reqwest::Error),
    #[error("the API key was rejected")]
    Unauthorized,
    #[error("HTTP {0}")]
    Status(u16),
    /// Too many requests or a server error recently: no request is sent until the backoff ends.
    #[error("indexer is backing off for {0:?}")]
    Backoff(std::time::Duration),
    #[error("response larger than {0} bytes")]
    TooLarge(usize),
    #[error("redirect to another host refused")]
    Redirect,
    /// `<error code=".." description=".."/>` from a Torznab/Newznab feed (100-199 are credential errors).
    #[error("indexer error {code}: {description}")]
    Indexer { code: u32, description: String },
    #[error("unreadable response: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Transport of a release.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Protocol {
    #[default]
    Torrent,
    Usenet,
}

/// What a search or profile is about. Newznab category ranges: 2000 movies, 5000 TV, 3000 audio (3030 audiobooks), 7000 books.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MediaKind {
    Movie,
    Tv,
    Music,
    Book,
}

impl MediaKind {
    /// Standard parent categories searched when the caller names none.
    pub fn default_categories(self) -> &'static [u32] {
        match self {
            Self::Movie => &[2000],
            Self::Tv => &[5000],
            Self::Music => &[3000],
            Self::Book => &[7000, 3030],
        }
    }

    /// The kind a category id belongs to (`None` for console, PC, XXX, other).
    pub fn of_category(cat: u32) -> Option<Self> {
        match cat {
            3030 => Some(Self::Book),
            2000..=2999 => Some(Self::Movie),
            5000..=5999 => Some(Self::Tv),
            3000..=3999 => Some(Self::Music),
            7000..=7999 => Some(Self::Book),
            _ => None,
        }
    }
}

/// One search result.
#[derive(Clone, PartialEq, Default)]
pub struct Release {
    pub title: String,
    pub guid: String,
    /// `.torrent` or `.nzb` link. Often carries the indexer's key in its query: use [`Indexer::download`], never log it.
    pub download_url: Option<String>,
    pub magnet: Option<String>,
    /// Lowercase hex v1 info hash.
    pub info_hash: Option<String>,
    pub size: Option<u64>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub grabs: Option<u32>,
    /// Unix seconds.
    pub publish_date: Option<i64>,
    pub indexer_id: i64,
    pub categories: Vec<u32>,
    pub protocol: Protocol,
    /// `tt` + digits.
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<u32>,
    pub tvdb_id: Option<u32>,
    /// The indexer's details page (`<comments>`).
    pub info_url: Option<String>,
}

impl Release {
    /// Usenet: hand the NZB to the user's SABnzbd/NZBGet ([`usenet::UsenetClient`]); hide it when none is configured.
    pub fn needs_usenet_client(&self) -> bool {
        self.protocol == Protocol::Usenet
    }

    /// The media kind from the categories, if any category says.
    pub fn kind(&self) -> Option<MediaKind> {
        self.categories
            .iter()
            .find_map(|&c| MediaKind::of_category(c))
    }
}

/// Query strings removed: download links usually carry the indexer key.
pub(crate) fn redact_url(u: &str) -> String {
    match u.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => u.to_string(),
    }
}

impl std::fmt::Debug for Release {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Release")
            .field("title", &self.title)
            .field("guid", &redact_url(&self.guid))
            .field(
                "download_url",
                &self.download_url.as_deref().map(redact_url),
            )
            .field("magnet", &self.magnet.is_some())
            .field("info_hash", &self.info_hash)
            .field("size", &self.size)
            .field("seeders", &self.seeders)
            .field("leechers", &self.leechers)
            .field("grabs", &self.grabs)
            .field("publish_date", &self.publish_date)
            .field("indexer_id", &self.indexer_id)
            .field("categories", &self.categories)
            .field("protocol", &self.protocol)
            .field("imdb_id", &self.imdb_id)
            .field("tmdb_id", &self.tmdb_id)
            .field("tvdb_id", &self.tvdb_id)
            .finish()
    }
}
