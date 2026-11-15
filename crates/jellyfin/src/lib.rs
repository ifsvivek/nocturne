//! Read-only Jellyfin client. Issues GET requests only; see `tests/readonly.rs`.

mod client;
mod config;
mod model;

pub use client::{Client, ImageType, ItemsQuery};
pub use config::Config;
pub use model::{Item, Items, PublicInfo, User};

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
