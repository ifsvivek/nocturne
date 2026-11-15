//! Read-only Jellyfin client.
//!
//! **This layer is intentionally read-only.** It issues GET requests only: no POST, PUT,
//! DELETE or PATCH, ever. The server holds the user's personal media and this client must not
//! be able to change it (no watched-state writes, no deletes, no metadata edits).
//! `tests/readonly.rs` fails the build if a write verb appears in `src/`.
//! Adding any write call requires an explicit decision recorded in `STATUS.md` first.

mod client;
mod config;
mod model;

pub use client::{Client, ImageType, ItemsQuery};
pub use config::Config;
pub use model::{Item, Items, PublicInfo, User, UserData};

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
