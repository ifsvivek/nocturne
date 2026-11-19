//! Read-only Jellyfin client.
//!
//! **This crate is strictly read-only.** It issues GET requests only: no POST, PUT, DELETE or
//! PATCH. The server holds the user's personal media, so the browsing/playback layer must not
//! be able to change it. `tests/readonly.rs` fails the build if a write verb appears in `src/`.
//!
//! Write/admin calls (users, libraries, tasks, sessions, settings) will live in a separate,
//! clearly named crate, `crates/jellyfin-admin` (to be created later), so this crate can never
//! grow writes by accident. Every write action there requires an explicit UI confirmation and
//! is tested only against a throwaway Jellyfin instance, never the user's real server.

mod browse;
mod client;
mod config;
pub mod header;
mod model;
mod play;
mod server_ui;
mod web;

pub use browse::{
    BrowseQuery, Chapter, ExternalUrl, FilterOptions, ItemExtra, MediaSource, NameId, Person,
    SearchHint, Stream,
};
pub use client::{Client, Health, ImageType, ItemsQuery};
pub use config::Config;
pub use model::{Item, Items, PublicInfo, User, UserData};
pub use play::{MediaStream, PlayItem, Segment, SegmentKind, Transcode, TrickplayInfo};
pub use server_ui::{Branding, DisplayPrefs, UserConfig};
pub use web::{WebFiles, css_chunks, index_stylesheets};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),
    #[error("http: {0}")]
    Http(reqwest::Error),
    #[error("server returned status {status} for {path}")]
    Status { status: u16, path: String },
    #[error("server has no users")]
    NoUsers,
}

pub type Result<T> = std::result::Result<T, Error>;
