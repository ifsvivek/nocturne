//! The contract of NOCTURNE's native *arr stack (docs/dev/ARR_PLAN.md): what the engine daemon (`nocturne-arrd`) needs from the torrent engine (`nocturne-torrent`, [`Downloader`]) and from the indexer client (`nocturne-indexer`, [`Indexer`]), and the release / quality model they share.
//!
//! Data and two traits only: no I/O, no async runtime. The traits use return-position `impl Future + Send`, so an implementation can write `async fn` in its `impl` block; the daemon is generic over them (no `dyn`).
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;

/// What a monitored title is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Movie,
    Series,
    Music,
    Book,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    Torrent,
    Usenet,
}

/// One quality step. Profiles list the allowed ones in their own order (worst first); the declaration order here is only the default order within each family.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    #[default]
    Unknown,
    // Video (movies, series).
    Cam,
    Telesync,
    Telecine,
    Dvd,
    Sdtv,
    WebRip480,
    WebDl480,
    Bluray480,
    Bluray576,
    Hdtv720,
    WebRip720,
    WebDl720,
    Bluray720,
    Hdtv1080,
    WebRip1080,
    WebDl1080,
    Bluray1080,
    Remux1080,
    Hdtv2160,
    WebRip2160,
    WebDl2160,
    Bluray2160,
    Remux2160,
    // Music.
    Mp3Low,
    Mp3_192,
    Mp3Vbr,
    Mp3_256,
    Mp3_320,
    Aac,
    Ogg,
    Alac,
    Flac,
    Flac24,
    // Books (ebooks, then audiobooks).
    Pdf,
    Mobi,
    Azw3,
    Epub,
    AudiobookMp3,
    AudiobookM4b,
}

impl Quality {
    pub const VIDEO: &[Quality] = &[
        Self::Cam,
        Self::Telesync,
        Self::Telecine,
        Self::Dvd,
        Self::Sdtv,
        Self::WebRip480,
        Self::WebDl480,
        Self::Bluray480,
        Self::Bluray576,
        Self::Hdtv720,
        Self::WebRip720,
        Self::WebDl720,
        Self::Bluray720,
        Self::Hdtv1080,
        Self::WebRip1080,
        Self::WebDl1080,
        Self::Bluray1080,
        Self::Remux1080,
        Self::Hdtv2160,
        Self::WebRip2160,
        Self::WebDl2160,
        Self::Bluray2160,
        Self::Remux2160,
    ];
    pub const AUDIO: &[Quality] = &[
        Self::Mp3Low,
        Self::Mp3_192,
        Self::Mp3Vbr,
        Self::Mp3_256,
        Self::Mp3_320,
        Self::Aac,
        Self::Ogg,
        Self::Alac,
        Self::Flac,
        Self::Flac24,
    ];
    pub const BOOK: &[Quality] = &[
        Self::Pdf,
        Self::Mobi,
        Self::Azw3,
        Self::Epub,
        Self::AudiobookMp3,
        Self::AudiobookM4b,
    ];

    /// The qualities that make sense for a media kind, in default order (worst first).
    pub fn for_kind(kind: MediaKind) -> &'static [Quality] {
        match kind {
            MediaKind::Movie | MediaKind::Series => Self::VIDEO,
            MediaKind::Music => Self::AUDIO,
            MediaKind::Book => Self::BOOK,
        }
    }

    /// Radarr-style display name.
    pub fn label(self) -> &'static str {
        use Quality::*;
        match self {
            Unknown => "Unknown",
            Cam => "CAM",
            Telesync => "TELESYNC",
            Telecine => "TELECINE",
            Dvd => "DVD",
            Sdtv => "SDTV",
            WebRip480 => "WEBRip-480p",
            WebDl480 => "WEBDL-480p",
            Bluray480 => "Bluray-480p",
            Bluray576 => "Bluray-576p",
            Hdtv720 => "HDTV-720p",
            WebRip720 => "WEBRip-720p",
            WebDl720 => "WEBDL-720p",
            Bluray720 => "Bluray-720p",
            Hdtv1080 => "HDTV-1080p",
            WebRip1080 => "WEBRip-1080p",
            WebDl1080 => "WEBDL-1080p",
            Bluray1080 => "Bluray-1080p",
            Remux1080 => "Remux-1080p",
            Hdtv2160 => "HDTV-2160p",
            WebRip2160 => "WEBRip-2160p",
            WebDl2160 => "WEBDL-2160p",
            Bluray2160 => "Bluray-2160p",
            Remux2160 => "Remux-2160p",
            Mp3Low => "MP3",
            Mp3_192 => "MP3-192",
            Mp3Vbr => "MP3-VBR",
            Mp3_256 => "MP3-256",
            Mp3_320 => "MP3-320",
            Aac => "AAC",
            Ogg => "OGG Vorbis",
            Alac => "ALAC",
            Flac => "FLAC",
            Flac24 => "FLAC 24bit",
            Pdf => "PDF",
            Mobi => "MOBI",
            Azw3 => "AZW3",
            Epub => "EPUB",
            AudiobookMp3 => "Audiobook MP3",
            AudiobookM4b => "Audiobook M4B",
        }
    }
}

/// External ids of a title. All optional; whatever is known is sent to indexers that support it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Ids {
    /// `tt0123456`.
    pub imdb: Option<String>,
    pub tmdb: Option<u64>,
    pub tvdb: Option<u64>,
    pub tvmaze: Option<u64>,
    /// MusicBrainz artist or release-group id.
    pub musicbrainz: Option<String>,
    /// Open Library author (`OL123A`) or work (`OL45W`) key.
    pub openlibrary: Option<String>,
}

/// What a release name says (filled by the indexer crate's parser).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParsedRelease {
    /// Clean movie, series, artist or author name.
    pub title: String,
    pub year: Option<u16>,
    pub season: Option<u32>,
    /// Episode numbers (several for a multi-episode file); empty for a season pack or a movie.
    pub episodes: Vec<u32>,
    /// A whole season (`S02`, `Season 2`, `S02.COMPLETE`).
    pub full_season: bool,
    /// Album (music) or book title.
    pub work: Option<String>,
    pub quality: Quality,
    /// `x264`, `x265`, `AV1`, ...
    pub codec: Option<String>,
    pub group: Option<String>,
    pub proper: bool,
    pub repack: bool,
    /// ISO 639-1 codes; empty when the name says nothing (usually English).
    pub languages: Vec<String>,
}

impl ParsedRelease {
    /// 1 normally, 2 for a PROPER or REPACK: a same-quality upgrade.
    pub fn revision(&self) -> u8 {
        1 + u8::from(self.proper || self.repack)
    }
}

/// Where to get the payload of a release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Magnet(String),
    /// An indexer download URL (`.torrent`, `.nzb`, or a redirect to a magnet); fetched through [`Indexer::fetch`].
    Url(String),
}

/// One search or RSS result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// Unique per indexer (the feed's `guid`).
    pub guid: String,
    pub indexer_id: u64,
    /// Indexer display name.
    pub indexer: String,
    pub protocol: Protocol,
    /// The raw release name.
    pub title: String,
    pub size: u64,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub link: Link,
    /// Lowercase hex v1 info hash, when the feed gives it.
    pub info_hash: Option<String>,
    /// Unix seconds.
    pub published: Option<u64>,
    pub categories: Vec<u32>,
    /// Ids the feed attached (`imdbid`, `tvdbid`, ... attributes).
    pub ids: Ids,
    pub parsed: ParsedRelease,
}

/// A user-added Torznab or Newznab endpoint. No site definitions ship with NOCTURNE. The API key is never in here: it lives in the keyring and is passed to each call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexerDef {
    #[serde(default)]
    pub id: u64,
    pub name: String,
    pub protocol: Protocol,
    /// The API endpoint, e.g. `http://127.0.0.1:9117/api/v2.0/indexers/x/results/torznab/` (the client appends `api?t=...`).
    pub url: String,
    /// Newznab category ids to search; empty = the standard ones for the kind (2000 movies, 5000 TV, 3000 audio, 7000 books).
    #[serde(default)]
    pub categories: Vec<u32>,
    /// Kinds this indexer is used for.
    pub kinds: Vec<MediaKind>,
    pub enabled: bool,
    /// Lower is tried first and wins ties.
    #[serde(default)]
    pub priority: u8,
}

/// A search request. The client uses the matching Torznab mode (`movie`, `tvsearch`, `music`, `book`) with the parameters the indexer's caps allow, falling back to `q=` text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub kind: MediaKind,
    /// Free text (title, plus year for movies); always set so a text fallback is possible.
    pub text: String,
    pub ids: Ids,
    pub year: Option<u16>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub author: Option<String>,
    pub book: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: u32,
    pub name: String,
}

/// An indexer's `t=caps`. Each mode is `None` when unsupported, else the parameters it accepts (`q`, `imdbid`, `season`, `ep`, `artist`, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Caps {
    pub search: Option<Vec<String>>,
    pub movie: Option<Vec<String>>,
    pub tv: Option<Vec<String>>,
    pub music: Option<Vec<String>>,
    pub book: Option<Vec<String>>,
    pub categories: Vec<Category>,
    /// Largest `limit` the indexer accepts.
    pub limit_max: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("the indexer or tracker refused the API key")]
    Auth,
    #[error("rate limited, retry after {0:?} s")]
    RateLimited(Option<u64>),
    #[error("not found")]
    NotFound,
    /// Bad input or an unparsable answer.
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Torznab / Newznab client (`nocturne-indexer`). Stateless per indexer: the definition and key come with every call, so one client (one HTTP pool) serves all indexers.
pub trait Indexer: Send + Sync + 'static {
    fn caps(
        &self,
        def: &IndexerDef,
        api_key: Option<&str>,
    ) -> impl Future<Output = Result<Caps>> + Send;
    /// Results with `indexer_id`, `indexer` and `parsed` filled.
    fn search(
        &self,
        def: &IndexerDef,
        api_key: Option<&str>,
        q: &SearchQuery,
    ) -> impl Future<Output = Result<Vec<Release>>> + Send;
    /// The newest releases of the kind's categories (no query).
    fn rss(
        &self,
        def: &IndexerDef,
        api_key: Option<&str>,
        kind: MediaKind,
    ) -> impl Future<Output = Result<Vec<Release>>> + Send;
    /// Resolve a [`Link::Url`]: the `.torrent` bytes, or the magnet it redirects to.
    fn fetch(
        &self,
        def: &IndexerDef,
        api_key: Option<&str>,
        url: &str,
    ) -> impl Future<Output = Result<TorrentSource>> + Send;
    /// The release-name parser, also used on file names inside a finished download (`Show.S01E02.mkv`).
    fn parse(&self, name: &str) -> ParsedRelease;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TorrentSource {
    Magnet(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddOptions {
    /// Directory the torrent's files go under (created if missing).
    pub save_path: PathBuf,
    /// Free-form tag (`nocturne-movie`, ...); reported back in [`TorrentStatus::category`].
    pub category: String,
    pub paused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TorrentState {
    /// Waiting for a download slot.
    Queued,
    /// Fetching metadata of a magnet.
    Metadata,
    /// Verifying existing data.
    Checking,
    Downloading,
    /// Complete and uploading.
    Seeding,
    Paused,
    /// See [`TorrentStatus::error`].
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentFile {
    /// Relative to [`TorrentStatus::save_path`], including the torrent's top folder if it has one.
    pub path: PathBuf,
    pub size: u64,
    pub done: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentStatus {
    /// Lowercase hex v1 info hash.
    pub id: String,
    pub name: String,
    pub state: TorrentState,
    pub error: Option<String>,
    pub category: String,
    pub save_path: PathBuf,
    /// Total wanted bytes and bytes done.
    pub size: u64,
    pub done: u64,
    pub uploaded: u64,
    /// Bytes per second.
    pub down_rate: u64,
    pub up_rate: u64,
    pub peers: u32,
    pub eta_secs: Option<u64>,
    /// Empty until a magnet's metadata arrives.
    pub files: Vec<TorrentFile>,
    /// Unix seconds.
    pub added: u64,
}

impl TorrentStatus {
    /// 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        if self.size == 0 {
            0.0
        } else {
            (self.done as f64 / self.size as f64) as f32
        }
    }

    pub fn ratio(&self) -> f32 {
        if self.size == 0 {
            0.0
        } else {
            (self.uploaded as f64 / self.size as f64) as f32
        }
    }
}

/// Global transfer limits; `None` = unlimited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub down_bps: Option<u64>,
    pub up_bps: Option<u64>,
}

/// The embedded torrent engine (`nocturne-torrent`). Torrents are identified by info hash. Off by default: the daemon creates one only when the user turned the stack on.
pub trait Downloader: Send + Sync + 'static {
    /// Start (or find, if already present) a torrent; returns its id. Adding the same torrent twice is not an error.
    fn add(
        &self,
        src: TorrentSource,
        opts: AddOptions,
    ) -> impl Future<Output = Result<String>> + Send;
    fn pause(&self, id: &str) -> impl Future<Output = Result<()>> + Send;
    fn resume(&self, id: &str) -> impl Future<Output = Result<()>> + Send;
    /// Stop and forget a torrent; `delete_files` also removes its data under `save_path` (the daemon passes true only for its own download folder).
    fn remove(&self, id: &str, delete_files: bool) -> impl Future<Output = Result<()>> + Send;
    fn list(&self) -> impl Future<Output = Result<Vec<TorrentStatus>>> + Send;
    fn set_limits(&self, limits: Limits) -> impl Future<Output = Result<()>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_are_disjoint_and_cover_all_but_unknown() {
        let all: Vec<_> = [Quality::VIDEO, Quality::AUDIO, Quality::BOOK].concat();
        let mut sorted = all.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
        assert_eq!(all.len(), 23 + 10 + 6);
        assert!(!all.contains(&Quality::Unknown));
        assert_eq!(Quality::for_kind(MediaKind::Series), Quality::VIDEO);
    }

    #[test]
    fn ratio_progress_and_revision() {
        let mut s = TorrentStatus {
            id: "ab".into(),
            name: "n".into(),
            state: TorrentState::Downloading,
            error: None,
            category: "c".into(),
            save_path: "/d".into(),
            size: 0,
            done: 0,
            uploaded: 0,
            down_rate: 0,
            up_rate: 0,
            peers: 0,
            eta_secs: None,
            files: vec![],
            added: 0,
        };
        assert_eq!((s.progress(), s.ratio()), (0.0, 0.0));
        (s.size, s.done, s.uploaded) = (200, 100, 300);
        assert_eq!((s.progress(), s.ratio()), (0.5, 1.5));
        let p = ParsedRelease {
            repack: true,
            ..Default::default()
        };
        assert_eq!((ParsedRelease::default().revision(), p.revision()), (1, 2));
    }
}
