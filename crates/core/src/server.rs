//! The native *arr engine as the app sees it (docs/dev/ARR_PLAN.md): its settings, the catalog model and the control-socket protocol of `nocturne-arrd`. Pure and dependency-light on purpose, like `gateway.rs`: the app never links the daemon (torrent engine, database).
pub use nocturne_arr_types as types;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use types::{Caps, Ids, IndexerDef, Limits, MediaKind, Quality, Release, TorrentStatus};

/// The `[arrd]` table of `settings.toml`; the daemon reads the same table with [`ArrdSettings::load_from`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArrdSettings {
    /// Master switch: when off nothing runs, no indexer is called and no torrent is started.
    pub enabled: bool,
    /// Keep running when the window is closed (systemd user unit, Linux).
    pub keep_running: bool,
    /// Where torrents download to; `None` = `$XDG_DATA_HOME/nocturne/arrd/downloads`. Put it on the libraries' file system so imports are hardlinks.
    pub downloads: Option<PathBuf>,
    /// Stop seeding an imported torrent at this upload ratio (0 = no ratio limit).
    pub seed_ratio: f32,
    /// ...or after this many hours of seeding (0 = no time limit). With both 0 it seeds until removed.
    pub seed_hours: u32,
    pub limits: Limits,
}

impl Default for ArrdSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_running: false,
            downloads: None,
            seed_ratio: 1.0,
            seed_hours: 168,
            limits: Limits::default(),
        }
    }
}

impl ArrdSettings {
    /// The `[arrd]` table of the file at `path`; a missing file, table or field gives the defaults.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
            .and_then(|mut t| t.remove("arrd"))
            .and_then(|v| v.try_into().ok())
            .unwrap_or_default()
    }

    pub fn downloads_dir(&self) -> Option<PathBuf> {
        self.downloads.clone().or_else(|| {
            nocturne_session::paths::data_dir().map(|d| d.join("arrd").join("downloads"))
        })
    }
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
    pub id: u64,
    pub name: String,
    pub kind: MediaKind,
    pub mode: LibraryMode,
    /// Absolute. Imports write only below it.
    pub root: PathBuf,
    /// Per title, relative to `root` (tokens: docs/dev/ARR_PLAN.md section 6).
    pub folder_template: String,
    /// Per file, relative to the title folder, without extension.
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub uptime_secs: u64,
    pub titles: usize,
    /// Monitored units without a file.
    pub missing: usize,
    pub downloading: usize,
    pub down_rate: u64,
    pub up_rate: u64,
    pub downloads: PathBuf,
    pub jobs: Vec<JobInfo>,
    /// Why the torrent engine is unavailable, if it is.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleDetail {
    pub title: Title,
    pub units: Vec<Unit>,
}

/// One request per line, JSON, on the control socket.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "arg", rename_all = "snake_case")]
pub enum Request {
    Status,
    /// Re-read `[arrd]` from settings.toml (limits, seeding rules).
    Reload,
    Stop,
    Libraries,
    /// Id 0 creates. Answers `Saved(id)`.
    SaveLibrary(Library),
    /// Refused while a title uses it.
    DeleteLibrary(u64),
    Profiles,
    SaveProfile(Profile),
    DeleteProfile(u64),
    Indexers,
    /// `api_key`: `None` keeps the stored one, `Some("")` deletes it. Keys go to the keyring only.
    SaveIndexer {
        def: IndexerDef,
        api_key: Option<String>,
    },
    DeleteIndexer(u64),
    /// Fetch the caps of a saved indexer.
    TestIndexer(u64),
    Titles(Option<MediaKind>),
    Title(u64),
    AddTitle(NewTitle),
    /// Forget a title and its units. Files on disk are left alone.
    RemoveTitle(u64),
    /// `units` empty = the title itself.
    Monitor {
        title: u64,
        units: Vec<u64>,
        monitored: bool,
    },
    /// Automatic search and grab; `units` empty = every wanted unit of the title. Results show up as queue and history events.
    Search {
        title: u64,
        units: Vec<u64>,
    },
    /// Interactive search: every release found, with the decision on each.
    Releases {
        title: u64,
        units: Vec<u64>,
    },
    /// Grab one release from [`Request::Releases`] (the user's choice overrides rejections except protocol and blocklist).
    Grab {
        title: u64,
        release: Box<Release>,
    },
    Queue,
    /// Remove from the queue (the torrent and its download data too); optionally blocklist and search again.
    RemoveGrab {
        id: u64,
        blocklist: bool,
        search: bool,
    },
    /// Retry the import of an `ImportBlocked` grab.
    RetryImport(u64),
    PauseGrab(u64),
    ResumeGrab(u64),
    History {
        title: Option<u64>,
        limit: u32,
    },
    Blocklist,
    Unblock(u64),
    RunJob(Job),
    /// Keep the connection open: one `Event` line per change (queue progress at most every 2 s while something downloads).
    Subscribe,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    Queue(Vec<QueueItem>),
    History(HistoryEntry),
    /// A title or one of its units changed (added, monitored, imported).
    Title(u64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    Status(Box<Status>),
    Ok,
    Saved(u64),
    Err(String),
    Libraries(Vec<Library>),
    Profiles(Vec<Profile>),
    Indexers(Vec<IndexerDef>),
    Caps(Box<Caps>),
    Titles(Vec<Title>),
    Title(Box<TitleDetail>),
    Releases(Vec<Candidate>),
    Queue(Vec<QueueItem>),
    History(Vec<HistoryEntry>),
    Blocklist(Vec<BlockEntry>),
    Event(Box<Event>),
}

/// Longest request line the daemon accepts.
pub const MAX_LINE: usize = 1024 * 1024;
/// Longest response line the client accepts (a big catalog).
pub const MAX_RESPONSE: usize = 64 * 1024 * 1024;

/// `$XDG_RUNTIME_DIR/nocturne/arrd.sock`.
pub fn socket_path() -> Option<PathBuf> {
    nocturne_session::paths::runtime_dir().map(|d| d.join("nocturne").join("arrd.sock"))
}

/// Ask the running daemon. Blocking: call it from a worker, not the UI thread. An error means it is not running.
#[cfg(unix)]
pub fn request(req: &Request) -> std::io::Result<Response> {
    let path = socket_path().ok_or_else(|| std::io::Error::other("no runtime directory"))?;
    // Searches ask every indexer: allow them time.
    request_at(&path, req, std::time::Duration::from_secs(120))
}

#[cfg(unix)]
fn connect(
    path: &Path,
    req: &Request,
    timeout: Option<std::time::Duration>,
) -> std::io::Result<std::io::BufReader<std::os::unix::net::UnixStream>> {
    use std::io::Write;
    let mut s = std::os::unix::net::UnixStream::connect(path)?;
    s.set_read_timeout(timeout)?;
    s.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut line = serde_json::to_vec(req).map_err(std::io::Error::other)?;
    line.push(b'\n');
    s.write_all(&line)?;
    Ok(std::io::BufReader::new(s))
}

#[cfg(unix)]
fn read_response(r: &mut impl std::io::BufRead) -> std::io::Result<Response> {
    use std::io::{BufRead, Read};
    let mut out = Vec::new();
    r.take(MAX_RESPONSE as u64).read_until(b'\n', &mut out)?;
    if out.is_empty() {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    serde_json::from_slice(&out).map_err(std::io::Error::other)
}

/// [`request`] against an explicit socket path (tests).
#[cfg(unix)]
pub fn request_at(
    path: &Path,
    req: &Request,
    timeout: std::time::Duration,
) -> std::io::Result<Response> {
    read_response(&mut connect(path, req, Some(timeout))?)
}

/// Subscribe and return the event stream: blocking, ends when the daemon stops. Run it on its own thread.
#[cfg(unix)]
pub fn subscribe_at(path: &Path) -> std::io::Result<impl Iterator<Item = Event>> {
    let mut r = connect(path, &Request::Subscribe, None)?;
    match read_response(&mut r)? {
        Response::Ok => {}
        other => return Err(std::io::Error::other(format!("{other:?}"))),
    }
    Ok(std::iter::from_fn(move || match read_response(&mut r) {
        Ok(Response::Event(e)) => Some(*e),
        _ => None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_seed_a_week_or_ratio_one() {
        let s = ArrdSettings::default();
        assert!(!s.enabled && !s.keep_running && s.downloads.is_none());
        assert_eq!((s.seed_ratio, s.seed_hours), (1.0, 168));
    }

    #[test]
    fn load_reads_only_the_arrd_table() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(
            &p,
            "version = 2\n[gateway]\nenabled = true\n[arrd]\nenabled = true\nseed_ratio = 2.0\n[arrd.limits]\nup_bps = 1000\n",
        )
        .unwrap();
        let s = ArrdSettings::load_from(&p);
        assert!(s.enabled && s.seed_ratio == 2.0 && s.seed_hours == 168);
        assert_eq!(s.limits.up_bps, Some(1000));
        assert_eq!(
            ArrdSettings::load_from(&d.path().join("none")),
            ArrdSettings::default()
        );
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
    fn protocol_round_trips() {
        let reqs = [
            Request::Status,
            Request::Search {
                title: 3,
                units: vec![4],
            },
            Request::Monitor {
                title: 1,
                units: vec![],
                monitored: false,
            },
            Request::RunJob(Job::RssSync),
            Request::Titles(Some(MediaKind::Book)),
            Request::Subscribe,
        ];
        for r in reqs {
            let j = serde_json::to_string(&r).unwrap();
            assert!(!j.contains('\n'));
            assert_eq!(serde_json::from_str::<Request>(&j).unwrap(), r);
        }
        assert_eq!(
            serde_json::to_string(&Request::DeleteIndexer(2)).unwrap(),
            r#"{"cmd":"delete_indexer","arg":2}"#
        );
        let ev = Response::Event(Box::new(Event::Title(5)));
        let j = serde_json::to_string(&ev).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&j).unwrap(), ev);
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"rm_rf"}"#).is_err());
    }
}
