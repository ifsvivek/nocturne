//! Radarr and Sonarr (v3 API) client: read the service's profiles, root folders, lookups, queue and
//! library state, and add a movie or a series. The two add calls are the only writes; they live in
//! `write.rs` and take an [`ArrConfirmed`]. Nothing here ever talks to a media server. Writes are
//! tested only against throwaway services from `scripts/test-arr.sh` (tests use
//! [`guard::ArrTestGuard`]). The API key is sent as `X-Api-Key`, is redacted from `Debug` and is
//! never stored here (the app keeps it in the keyring, `crates/session`).

mod client;
pub mod guard;
mod model;
mod write;

pub use client::{ArrClient, normalize_url, secret_name};
pub use model::*;
pub use write::{add_movie_body, add_series_body};

/// Proof that the user confirmed adding a title to their Radarr or Sonarr. Zero-size; the private
/// field means it can only be built via [`ArrConfirmed::i_have_asked_the_user`], which only the
/// request dialog's confirmation handler calls.
///
/// ```compile_fail
/// let _ = nocturne_arr::ArrConfirmed(());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ArrConfirmed(());

impl ArrConfirmed {
    /// Call this only from the code path that runs after the user clicked "Request" in the dialog.
    pub fn i_have_asked_the_user() -> Self {
        Self(())
    }
}

/// Which service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    Radarr,
    Sonarr,
}

impl Service {
    pub fn name(self) -> &'static str {
        match self {
            Self::Radarr => "Radarr",
            Self::Sonarr => "Sonarr",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Radarr => "radarr",
            Self::Sonarr => "sonarr",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),
    #[error("http: {0}")]
    Http(reqwest::Error),
    /// 401: the API key is wrong.
    #[error("the API key was not accepted")]
    Unauthorized,
    /// 400 with the service's own message ("This movie has already been added", ...).
    #[error("{0}")]
    Rejected(String),
    #[error("not found")]
    NotFound,
    #[error("server returned status {status} for {path}")]
    Status { status: u16, path: String },
}

pub type Result<T> = std::result::Result<T, Error>;
