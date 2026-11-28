//! Jellyfin user-scope client: login, playback reporting, played/favorite state, the user's own
//! playlists and own password.
//!
//! `nocturne-jellyfin` stays strictly read-only and `nocturne-jellyfin-admin` holds admin writes;
//! everything here acts as the logged-in user only. Writes are tested ONLY against the throwaway
//! server from `scripts/test-jellyfin.sh` (tests use `nocturne_jellyfin_admin::guard::TestServerGuard`).

mod client;
#[cfg(test)]
mod mock;
mod model;
pub mod raw;
pub mod syncplay;

pub use client::{UserClient, set_device_id};
pub use model::*;

/// Proof that the user explicitly confirmed a sensitive action (changing their password).
/// Zero-size; the private field means it can only be built via [`UserConfirmed::i_have_asked_the_user`].
///
/// ```compile_fail
/// let _ = nocturne_jellyfin_user::UserConfirmed(());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct UserConfirmed(());

impl UserConfirmed {
    /// Call this only from the code path that runs after the user clicked "confirm".
    pub fn i_have_asked_the_user() -> Self {
        Self(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),
    #[error("http: {0}")]
    Http(reqwest::Error),
    /// Login rejected (wrong username or password). No token is ever produced.
    #[error("invalid username or password")]
    InvalidCredentials,
    #[error("server returned status {status} for {path}")]
    Status { status: u16, path: String },
}

pub type Result<T> = std::result::Result<T, Error>;
