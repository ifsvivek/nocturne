//! NOCTURNE's torrent engine: a thin, documented layer over [librqbit](https://github.com/ikatson/rqbit) (pure Rust) that adds what a download client for an *arr stack needs: a queue with active-download/seed limits, seeding goals, an alternative speed schedule, a bind-interface kill switch, an event stream, and input checks (magnet links, save paths, torrent names).
//!
//! ```no_run
//! # async fn demo() -> Result<(), nocturne_torrent::Error> {
//! use nocturne_torrent::{AddOptions, Config, Engine, Event, Source};
//! let engine = Engine::start(Config::new("/var/lib/x/torrent", "/data/downloads")).await?;
//! let mut events = engine.events();
//! let id = engine.add(Source::Magnet("magnet:?xt=urn:btih:...".into()), AddOptions::default()).await?;
//! while let Ok(ev) = events.recv().await {
//!     if ev == (Event::Completed { id: id.clone() }) {
//!         let info = engine.get(&id).unwrap();
//!         println!("import {}", info.content_path.display());
//!     }
//! }
//! engine.shutdown().await;
//! # Ok(()) }
//! ```
//!
//! Needs a multi-thread tokio runtime. Idle cost: the engine's scheduler sleeps until something changes when no torrent is running; librqbit keeps one 1 Hz statistics timer.
//!
//! Not supported by librqbit 9.1 and therefore not offered: protocol encryption (MSE/PE), turning PEX off, per-file priorities beyond download/skip, binding to an IP address (use `bind_interface`), tracker scrape counts.

mod config;
mod engine;
mod paths;
mod types;

pub use config::{AltSpeed, Config, GoalAction, Limits, Schedule, SeedGoal};
pub use engine::Engine;
pub use paths::MAX_TORRENT_BYTES;
pub use types::{AddOptions, Event, FileInfo, Source, State, TorrentInfo};

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid magnet link: {0}")]
    InvalidMagnet(String),
    #[error("invalid torrent: {0}")]
    InvalidTorrent(String),
    #[error("path {0:?} would leave the download directory")]
    PathEscape(PathBuf),
    #[error("no torrent {0}")]
    NotFound(String),
    #[error("network interface {0} is not available")]
    InterfaceDown(String),
    #[error("{0}")]
    Engine(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
