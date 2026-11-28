//! The NOCTURNE server as its clients see it (docs/dev/SERVER_PLAN.md, API: docs/api/openapi.yaml): its configuration (`server.toml`), the library manager's model and the event stream. Pure and dependency-light, like `gateway.rs`: the app never links the server (torrent engine, database, HTTP server).
use crate::gateway::Binding;
pub use nocturne_arr_types as types;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use types::{Ids, Limits, MediaKind, Quality, Release, TorrentStatus};

pub const DEFAULT_PORT: u16 = 8100;
/// Bump with a migration in [`ServerConfig::load_from`] when a field changes meaning.
pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    pub binding: Binding,
    pub port: u16,
    /// PEM files; required for [`Binding::Remote`].
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
    /// `X-Forwarded-For` is believed only from this peer (a reverse proxy).
    pub trusted_proxy: Option<IpAddr>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            binding: Binding::Local,
            port: DEFAULT_PORT,
            cert_path: None,
            key_path: None,
            trusted_proxy: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Switch {
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadsConfig {
    /// The torrent engine. Off by default: no torrent is started and no peer is contacted.
    pub enabled: bool,
    /// `None` = `$XDG_DATA_HOME/nocturne/server/downloads`. Put it on the libraries' file system so imports are hardlinks.
    pub dir: Option<PathBuf>,
    /// Stop seeding an imported torrent at this ratio (0 = no ratio limit)...
    pub seed_ratio: f32,
    /// ...or after this many hours (0 = no time limit). Both 0: seed until removed.
    pub seed_hours: u32,
    pub limits: Limits,
}

impl Default for DownloadsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: None,
            seed_ratio: 1.0,
            seed_hours: 168,
            limits: Limits::default(),
        }
    }
}

impl DownloadsConfig {
    pub fn dir_or_default(&self) -> Option<PathBuf> {
        self.dir.clone().or_else(|| {
            nocturne_session::paths::data_dir().map(|d| d.join("server").join("downloads"))
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestsConfig {
    pub enabled: bool,
    /// A new request becomes a monitored title at once (needs the library module).
    pub auto_approve: bool,
}

impl Default for RequestsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_approve: true,
        }
    }
}

/// `$XDG_CONFIG_HOME/nocturne/server.toml`. Every module is off by default; with all off the server only answers the API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub version: u32,
    pub http: HttpConfig,
    /// The library manager: libraries, profiles, indexers, monitored titles, search, import, scheduled jobs.
    pub library: Switch,
    pub downloads: DownloadsConfig,
    pub requests: RequestsConfig,
    /// The sections of the other modules (`[gateway]`, `[tunnel]`, `[discovery]`, `[storage]`, ...), each owned and typed by its module: read with [`ServerConfig::section`], write with [`ServerConfig::set_section`]. Unknown sections survive a load and save untouched.
    #[serde(flatten)]
    pub sections: toml::Table,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            http: HttpConfig::default(),
            library: Switch::default(),
            downloads: DownloadsConfig::default(),
            requests: RequestsConfig::default(),
            sections: toml::Table::new(),
        }
    }
}

impl ServerConfig {
    pub fn default_path() -> Option<PathBuf> {
        nocturne_session::paths::config_dir().map(|d| d.join("server.toml"))
    }

    /// A missing file gives the defaults; an unreadable one is an error (the server must not start with everything reset).
    pub fn load_from(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                let c: Self = toml::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))?;
                if c.version > CONFIG_VERSION {
                    return Err(format!("{} is from a newer NOCTURNE", path.display()));
                }
                Ok(Self {
                    version: CONFIG_VERSION,
                    ..c
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Atomic: a temporary file in the same folder, then a rename.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string(self).map_err(|e| e.to_string())?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// A module's section; missing or unreadable gives its defaults (`T` should be `#[serde(default)]`).
    pub fn section<T: serde::de::DeserializeOwned + Default>(&self, name: &str) -> T {
        self.sections
            .get(name)
            .cloned()
            .and_then(|v| v.try_into().ok())
            .unwrap_or_default()
    }

    /// Replace a module's section.
    pub fn set_section<T: Serialize>(&mut self, name: &str, v: &T) -> Result<(), String> {
        let v = toml::Value::try_from(v).map_err(|e| e.to_string())?;
        self.sections.insert(name.to_string(), v);
        Ok(())
    }

    /// Whether a module is switched on: the `enabled` key of its section (typed sections included).
    pub fn enabled(&self, module: &str) -> bool {
        match module {
            "library" => self.library.enabled,
            "downloads" => self.downloads.enabled,
            "requests" => self.requests.enabled,
            _ => self.section::<Switch>(module).enabled,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.http.binding == Binding::Remote
            && (self.http.cert_path.is_none() || self.http.key_path.is_none())
        {
            return Err("Remote over HTTPS needs a certificate and a key file".into());
        }
        if self.http.port == 0 {
            return Err("port 0 is not allowed".into());
        }
        Ok(())
    }
}

/// `$XDG_RUNTIME_DIR/nocturne/server.sock`: the same API for local clients; a peer with the server's uid is the local admin.
pub fn socket_path() -> Option<PathBuf> {
    nocturne_session::paths::runtime_dir().map(|d| d.join("nocturne").join("server.sock"))
}

/// Where a library's finished downloads go (a radio choice per library).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryMode {
    /// A folder NOCTURNE manages; the app adds it as a local source.
    #[default]
    Managed,
    /// The user's existing folder: imported files are added there under the naming templates. Files NOCTURNE did not import are never overwritten or deleted.
    Existing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    /// 0 when saving a new one.
    #[serde(default)]
    pub id: u64,
    pub name: String,
    pub kind: MediaKind,
    pub mode: LibraryMode,
    /// Absolute. Imports write only below it.
    pub root: PathBuf,
    /// Per title, relative to `root` (tokens: docs/dev/ARR_PLAN.md section 6).
    #[serde(default)]
    pub folder_template: String,
    /// Per file, relative to the title folder, without extension.
    #[serde(default)]
    pub file_template: String,
}

impl Library {
    /// Radarr / Sonarr / Lidarr / Readarr-like defaults: `(folder, file)`.
    pub fn default_templates(kind: MediaKind) -> (&'static str, &'static str) {
        match kind {
            MediaKind::Movie => (
                "{Movie Title} ({Release Year})",
                "{Movie Title} ({Release Year}) {Quality Full}",
            ),
            MediaKind::Series => (
                "{Series Title} ({Series Year})",
                "Season {season:00}/{Series Title} - S{season:00}E{episode:00} - {Episode Title} {Quality Full}",
            ),
            MediaKind::Music => (
                "{Artist Name}",
                "{Album Title} ({Release Year})/{Original Name}",
            ),
            MediaKind::Book => (
                "{Author Name}",
                "{Book Title} ({Release Year})/{Book Title}",
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferred {
    /// Case-insensitive substring of the release name.
    pub term: String,
    pub score: i32,
}

/// A quality profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: u64,
    pub name: String,
    pub kind: MediaKind,
    /// Allowed qualities, worst first. A release of any other quality is rejected.
    pub allowed: Vec<Quality>,
    /// Stop upgrading once a file of this quality (or better) is in place.
    pub cutoff: Quality,
    pub upgrades: bool,
    pub preferred: Vec<Preferred>,
    /// Every term must appear in the release name.
    pub required: Vec<String>,
    /// No term may appear.
    pub ignored: Vec<String>,
    /// Size limits per unit (movie, episode, album, book) in MiB; 0 = none.
    pub min_mb: u64,
    pub max_mb: u64,
    pub min_seeders: u32,
    /// ISO 639-1; empty = any. A release that names no language counts as English.
    pub languages: Vec<String>,
}

impl Default for Profile {
    fn default() -> Self {
        Self::standard(MediaKind::Movie)
    }
}

impl Profile {
    /// The profile a new install starts with for a kind: everything but cam/telesync-grade video, upgrades until the best common quality.
    pub fn standard(kind: MediaKind) -> Self {
        let all = Quality::for_kind(kind);
        let (allowed, cutoff): (Vec<Quality>, Quality) = match kind {
            MediaKind::Movie | MediaKind::Series => (
                all.iter()
                    .copied()
                    .filter(|q| !matches!(q, Quality::Cam | Quality::Telesync | Quality::Telecine))
                    .collect(),
                Quality::Bluray1080,
            ),
            MediaKind::Music => (all.to_vec(), Quality::Flac),
            MediaKind::Book => (all.to_vec(), Quality::Epub),
        };
        Self {
            id: 0,
            name: "Standard".into(),
            kind,
            allowed,
            cutoff,
            upgrades: true,
            preferred: vec![],
            required: vec![],
            ignored: vec![],
            min_mb: 0,
            max_mb: 0,
            min_seeders: 1,
            languages: vec![],
        }
    }
}

/// A monitored movie, series, artist or author.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Title {
    pub id: u64,
    pub kind: MediaKind,
    pub name: String,
    pub year: Option<u16>,
    pub ids: Ids,
    pub profile: u64,
    pub library: u64,
    pub monitored: bool,
    /// Unix seconds.
    pub added: u64,
}

/// A file NOCTURNE imported itself (only such files are ever replaced by an upgrade).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    /// Absolute; an album has several.
    pub paths: Vec<PathBuf>,
    pub quality: Quality,
    pub revision: u8,
    pub size: u64,
    /// The release it came from.
    pub release: String,
    pub imported: u64,
}

/// The wanted leaf of a title: the movie itself, an episode, an album or a book.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub id: u64,
    pub title: u64,
    /// Movie: the title's name; episode, album, book: its own.
    pub name: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub year: Option<u16>,
    /// `YYYY-MM-DD`; a unit that has not aired / been released yet is not searched.
    pub air_date: Option<String>,
    pub ids: Ids,
    pub monitored: bool,
    pub file: Option<FileRecord>,
}

/// Which units a new title monitors.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Monitor {
    #[default]
    All,
    /// Units released after today.
    Future,
    /// Series: these seasons only.
    Seasons(Vec<u32>),
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTitle {
    pub kind: MediaKind,
    pub name: String,
    pub year: Option<u16>,
    pub ids: Ids,
    pub profile: u64,
    pub library: u64,
    pub monitor: Monitor,
    /// Search for the wanted units right away.
    pub search: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrabState {
    Downloading,
    /// Imported, still seeding.
    Seeding,
    /// Finished downloading but could not be imported (see `message`); stays until removed or retried.
    ImportBlocked,
    Failed,
    /// Imported and seeding finished; the torrent and its download data are gone.
    Done,
}

/// One grabbed release in the queue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grab {
    pub id: u64,
    pub title: u64,
    pub units: Vec<u64>,
    pub release: Release,
    /// The downloader's id (info hash).
    pub torrent: String,
    pub state: GrabState,
    pub added: u64,
    /// Bytes done at `progress_at` (stall detection).
    pub done: u64,
    pub progress_at: u64,
    pub seeding_since: Option<u64>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueItem {
    pub grab: Grab,
    pub title_name: String,
    /// Live state from the torrent engine; `None` once the torrent is gone.
    pub status: Option<TorrentStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEvent {
    Grabbed,
    Imported,
    /// An imported file replaced one NOCTURNE had imported before.
    Upgraded,
    ImportFailed,
    DownloadFailed,
    /// Removed from the queue by the user.
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub at: u64,
    pub title: u64,
    pub units: Vec<u64>,
    pub event: HistoryEvent,
    pub release: String,
    pub quality: Quality,
    pub indexer: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockEntry {
    pub id: u64,
    pub at: u64,
    pub title: u64,
    pub release: String,
    pub indexer_id: u64,
    pub guid: String,
    pub info_hash: Option<String>,
    pub reason: String,
}

/// A release with what the decision rules say about it (interactive search).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub release: Release,
    /// The units it would fill.
    pub units: Vec<u64>,
    /// Empty = acceptable.
    pub rejections: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Job {
    /// New releases from every indexer's feed.
    RssSync,
    /// Search for monitored units without a file.
    MissingSearch,
    /// Search for units below their profile's cutoff.
    UpgradeSearch,
    /// Episode, album and book lists from the metadata providers.
    RefreshMetadata,
}

impl Job {
    pub const ALL: [Job; 4] = [
        Job::RssSync,
        Job::MissingSearch,
        Job::UpgradeSearch,
        Job::RefreshMetadata,
    ];

    /// Seconds between runs.
    pub fn interval(self) -> u64 {
        match self {
            Job::RssSync => 15 * 60,
            Job::MissingSearch => 24 * 3600,
            Job::UpgradeSearch => 24 * 3600,
            Job::RefreshMetadata => 12 * 3600,
        }
    }

    /// Whether the job should run now. A job that never ran is due at once, except the searches, which wait one RSS interval after the first start so a fresh install does not hit every indexer at once.
    pub fn due(self, last: Option<u64>, started: u64, now: u64) -> bool {
        match last {
            Some(l) => now >= l.saturating_add(self.interval()),
            None => match self {
                Job::RssSync | Job::RefreshMetadata => true,
                _ => now >= started + Job::RssSync.interval(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInfo {
    pub job: Job,
    pub last_run: Option<u64>,
    pub next_run: u64,
    pub running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleStatus {
    /// The module's name (its config section and route prefix).
    pub name: String,
    pub enabled: bool,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub uptime_secs: u64,
    pub setup_needed: bool,
    pub modules: Vec<ModuleStatus>,
    pub titles: usize,
    /// Monitored, released units without a file.
    pub missing: usize,
    pub downloading: usize,
    pub down_rate: u64,
    pub up_rate: u64,
    pub jobs: Vec<JobInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleDetail {
    pub title: Title,
    pub units: Vec<Unit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Pending,
    Approved,
    Downloading,
    Available,
    Declined,
}

/// A request of the server's requests module (`Request` in the API).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaRequest {
    pub id: u64,
    pub kind: MediaKind,
    pub name: String,
    pub year: Option<u16>,
    #[serde(default)]
    pub ids: Ids,
    #[serde(default)]
    pub seasons: Vec<u32>,
    pub requested_by: String,
    pub created: u64,
    pub status: RequestStatus,
    /// The monitored title it became.
    pub title: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRequest {
    pub kind: MediaKind,
    pub name: String,
    #[serde(default)]
    pub year: Option<u16>,
    #[serde(default)]
    pub ids: Ids,
    #[serde(default)]
    pub seasons: Vec<u32>,
    #[serde(default)]
    pub library: Option<u64>,
    #[serde(default)]
    pub profile: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobEvent {
    pub job: Job,
    pub running: bool,
    pub message: Option<String>,
}

/// One message on `/api/v1/events`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    Queue(Vec<QueueItem>),
    History(HistoryEntry),
    /// A title or one of its units changed.
    Title(u64),
    Job(JobEvent),
    Request(u64),
    Health(ModuleStatus),
    /// Anything a module publishes about itself (gateway sessions, storage mounts, ...).
    Module(ModuleEvent),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModuleEvent {
    pub module: String,
    /// The module's own event name.
    pub kind: String,
    pub data: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_off_on_loopback_8100() {
        let c = ServerConfig::default();
        assert!(!c.library.enabled && !c.downloads.enabled && !c.requests.enabled);
        assert!(!c.enabled("gateway") && c.sections.is_empty());
        assert_eq!((c.http.binding, c.http.port), (Binding::Local, 8100));
        assert_eq!((c.downloads.seed_ratio, c.downloads.seed_hours), (1.0, 168));
        assert!(c.requests.auto_approve && c.validate().is_ok());
    }

    #[test]
    fn config_round_trips_atomically_and_refuses_bad_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("cfg/server.toml");
        assert_eq!(
            ServerConfig::load_from(&p).unwrap(),
            ServerConfig::default()
        );
        let mut c = ServerConfig::default();
        c.library.enabled = true;
        c.downloads.limits.up_bps = Some(1000);
        c.save_to(&p).unwrap();
        assert_eq!(ServerConfig::load_from(&p).unwrap(), c);
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
        std::fs::write(&p, "version = 1\n[library]\nenabled = true\n").unwrap();
        let c = ServerConfig::load_from(&p).unwrap();
        assert!(c.library.enabled && !c.downloads.enabled);
        std::fs::write(&p, "version = 99\n").unwrap();
        assert!(ServerConfig::load_from(&p).unwrap_err().contains("newer"));
        std::fs::write(&p, "[http]\nbinding = \"moon\"\n").unwrap();
        assert!(ServerConfig::load_from(&p).is_err());
        // Another module's section: typed on read, kept verbatim through load and save.
        std::fs::write(
            &p,
            "[gateway]\nenabled = true\nport = 9000\n[storage]\nmounts = [\"a\"]\n",
        )
        .unwrap();
        let mut c = ServerConfig::load_from(&p).unwrap();
        assert!(c.enabled("gateway") && !c.enabled("storage") && !c.enabled("nothing"));
        assert!(c.section::<crate::gateway::GatewaySettings>("gateway").port == 9000);
        c.set_section("tunnel", &Switch { enabled: true }).unwrap();
        c.save_to(&p).unwrap();
        let c = ServerConfig::load_from(&p).unwrap();
        assert!(c.enabled("tunnel") && c.sections["storage"]["mounts"][0].as_str() == Some("a"));
        let j = serde_json::to_string(&c).unwrap();
        assert_eq!(
            serde_json::from_str::<ServerConfig>(&j).unwrap(),
            c,
            "JSON for the API too"
        );
        let mut r = ServerConfig::default();
        r.http.binding = Binding::Remote;
        assert!(r.validate().is_err());
    }

    #[test]
    fn standard_profiles_fit_their_kind() {
        for k in [
            MediaKind::Movie,
            MediaKind::Series,
            MediaKind::Music,
            MediaKind::Book,
        ] {
            let p = Profile::standard(k);
            assert!(p.allowed.contains(&p.cutoff), "{k:?}");
            assert!(p.allowed.iter().all(|q| Quality::for_kind(k).contains(q)));
        }
        assert!(
            !Profile::standard(MediaKind::Movie)
                .allowed
                .contains(&Quality::Cam)
        );
    }

    #[test]
    fn jobs_wait_their_interval_and_searches_wait_after_first_start() {
        assert!(Job::RssSync.due(None, 100, 100));
        assert!(!Job::MissingSearch.due(None, 100, 100));
        assert!(Job::MissingSearch.due(None, 100, 100 + 900));
        assert!(!Job::RssSync.due(Some(1000), 0, 1000 + 899));
        assert!(Job::RssSync.due(Some(1000), 0, 1000 + 900));
        assert!(Job::UpgradeSearch.due(Some(0), 0, 86400));
    }

    #[test]
    fn events_have_the_documented_shape() {
        let j = serde_json::to_string(&Event::Title(5)).unwrap();
        assert_eq!(j, r#"{"type":"title","data":5}"#);
        let e = Event::Job(JobEvent {
            job: Job::RssSync,
            running: true,
            message: None,
        });
        let j = serde_json::to_string(&e).unwrap();
        assert!(
            j.starts_with(r#"{"type":"job","data":{"job":"rss_sync""#),
            "{j}"
        );
        assert_eq!(serde_json::from_str::<Event>(&j).unwrap(), e);
        assert_eq!(
            serde_json::to_string(&Monitor::Seasons(vec![1])).unwrap(),
            r#"{"seasons":[1]}"#
        );
    }
}
