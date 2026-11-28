//! Public data types: what goes into `Engine::add` and what `list`/`get` and the event stream report.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::config::SeedGoal;

/// Where a torrent comes from. HTTP URLs and local paths are deliberately not accepted: the caller fetches .torrent files itself.
#[derive(Debug, Clone)]
pub enum Source {
    /// A `magnet:?xt=urn:btih:...` link.
    Magnet(String),
    /// The bytes of a .torrent file (at most `MAX_TORRENT_BYTES`).
    TorrentFile(Vec<u8>),
}

#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    /// Relative to `Config::download_dir`, or absolute inside it. `None` = the download dir. Multi-file torrents get a sub-folder named after the torrent.
    pub save_path: Option<PathBuf>,
    /// Free-form labels for the caller (the daemon's category); the engine only stores and reports them.
    pub category: Option<String>,
    pub tags: Vec<String>,
    /// Add without starting.
    pub paused: bool,
    /// Stored and reported. librqbit always fetches the pieces of each file in order (first and last piece first), so downloads are effectively sequential either way.
    pub sequential: bool,
    /// Indices of the files to download (see `TorrentInfo::files`); `None` = all. A magnet's `so=` selection is used when this is `None`.
    pub only_files: Option<Vec<usize>>,
    /// Overrides `Config::seed_goal` for this torrent.
    pub seed_goal: Option<SeedGoal>,
    /// Peers to contact directly, in addition to DHT/trackers/LSD.
    pub peers: Vec<SocketAddr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Waiting for a download or seed slot (`Config::max_active_*`), or held while the bound interface is down.
    Queued,
    /// Verifying data on disk.
    Checking,
    /// A magnet whose metadata is still being fetched from peers.
    Metadata,
    Downloading,
    /// Downloading but nothing arrives.
    Stalled,
    Seeding,
    /// Paused by the user before it finished.
    Paused,
    /// Finished and not seeding (seeding goal reached, or paused after finishing).
    Completed,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub index: usize,
    /// Relative to `TorrentInfo::content_path` for multi-file torrents; the file name for single-file ones.
    pub path: PathBuf,
    pub size: u64,
    pub downloaded: u64,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct TorrentInfo {
    /// Info-hash, lowercase hex. Stable across restarts.
    pub id: String,
    pub name: String,
    pub state: State,
    /// 0.0..=1.0 of the selected files.
    pub progress: f64,
    /// Bytes of the selected files.
    pub size: u64,
    pub downloaded: u64,
    /// Total uploaded, across restarts.
    pub uploaded: u64,
    /// Bytes per second.
    pub download_rate: u64,
    pub upload_rate: u64,
    pub eta: Option<Duration>,
    pub ratio: f64,
    /// Connected peers, and how many of them are seeders. Tracker scrape counts are not available from librqbit.
    pub peers: u32,
    pub seeders: u32,
    pub leechers: u32,
    pub save_path: PathBuf,
    /// The torrent's folder (multi-file) or file (single-file): what an importer should pick up.
    pub content_path: PathBuf,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub sequential: bool,
    pub files: Vec<FileInfo>,
    pub added: SystemTime,
    pub completed: Option<SystemTime>,
}

/// Engine notifications, from `Engine::events`. Lagging receivers lose old events (tokio broadcast); `list` is always the truth.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Added {
        id: String,
    },
    /// A magnet's metadata arrived; names and files are now known.
    MetadataReady {
        id: String,
    },
    /// All selected files are downloaded and verified. Sent once per torrent (again only if the file selection changes).
    Completed {
        id: String,
    },
    /// A torrent failed (or, with an empty `id`, the engine could not save its state file).
    Error {
        id: String,
        message: String,
    },
    Removed {
        id: String,
    },
    /// The bound interface is gone: every torrent was paused. No traffic flows until `InterfaceUp`.
    InterfaceDown {
        name: String,
    },
    InterfaceUp {
        name: String,
    },
}

/// What the engine knows about a torrent at one moment; `state_of` turns it into a `State`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Phase {
    Checking,
    Live,
    Paused,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Facts {
    pub phase: Phase,
    pub finished: bool,
    pub user_paused: bool,
    pub goal_reached: bool,
    pub queued: bool,
    pub download_rate: u64,
}

pub(crate) fn state_of(f: Facts) -> State {
    match (f.phase, f.finished) {
        (Phase::Checking, _) => State::Checking,
        (Phase::Live, true) => State::Seeding,
        (Phase::Live, false) if f.download_rate == 0 => State::Stalled,
        (Phase::Live, false) => State::Downloading,
        (Phase::Paused, _) if f.queued => State::Queued,
        (Phase::Paused, true) => State::Completed,
        (Phase::Paused, false) if f.user_paused || f.goal_reached => State::Paused,
        // Added paused and about to be started by the scheduler.
        (Phase::Paused, false) => State::Queued,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(phase: Phase, finished: bool) -> Facts {
        Facts {
            phase,
            finished,
            user_paused: false,
            goal_reached: false,
            queued: false,
            download_rate: 0,
        }
    }

    #[test]
    fn state_mapping() {
        assert_eq!(state_of(facts(Phase::Checking, true)), State::Checking);
        assert_eq!(state_of(facts(Phase::Live, true)), State::Seeding);
        assert_eq!(state_of(facts(Phase::Live, false)), State::Stalled);
        assert_eq!(
            state_of(Facts {
                download_rate: 10,
                ..facts(Phase::Live, false)
            }),
            State::Downloading
        );
        assert_eq!(
            state_of(Facts {
                user_paused: true,
                ..facts(Phase::Paused, false)
            }),
            State::Paused
        );
        assert_eq!(
            state_of(Facts {
                user_paused: true,
                ..facts(Phase::Paused, true)
            }),
            State::Completed
        );
        assert_eq!(
            state_of(Facts {
                goal_reached: true,
                ..facts(Phase::Paused, true)
            }),
            State::Completed
        );
        assert_eq!(
            state_of(Facts {
                queued: true,
                ..facts(Phase::Paused, true)
            }),
            State::Queued
        );
        assert_eq!(
            state_of(Facts {
                queued: true,
                ..facts(Phase::Paused, false)
            }),
            State::Queued
        );
        assert_eq!(state_of(facts(Phase::Paused, false)), State::Queued);
    }
}
