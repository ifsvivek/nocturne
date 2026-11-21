//! Seerr (the merged Jellyseerr / Overseerr) client: sign-in, search, details, quota and the user's
//! own media requests. See `docs/dev/SEERR_PLAN.md`.
//!
//! The only things that change anything on the Seerr server are creating and cancelling a request,
//! both in `write.rs` and both taking a [`RequestConfirmed`]. Nothing here ever talks to a media
//! server. Writes are tested only against the throwaway Seerr from `scripts/test-seerr.sh`
//! (tests use [`guard::SeerrTestGuard`]).

mod client;
pub mod guard;
mod model;
mod write;

pub use client::{Auth, SeerrClient, SessionCookie, normalize_url, secret_name};
pub use model::*;

/// Permission bit flags of `Me::permissions` (Seerr's `Permission` enum). ADMIN implies all.
pub mod perm {
    pub const ADMIN: u64 = 2;
    pub const MANAGE_REQUESTS: u64 = 16;
    pub const REQUEST: u64 = 32;
    pub const REQUEST_MOVIE: u64 = 262_144;
    pub const REQUEST_TV: u64 = 524_288;

    pub fn has(perms: u64, flag: u64) -> bool {
        perms & ADMIN != 0 || perms & flag != 0
    }
}

/// Proof that the user confirmed sending (or cancelling) a request. Zero-size; the private field
/// means it can only be built via [`RequestConfirmed::i_have_asked_the_user`], which only the
/// Request dialog's and the Requests page's confirmation handlers call.
///
/// ```compile_fail
/// let _ = nocturne_seerr::RequestConfirmed(());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RequestConfirmed(());

impl RequestConfirmed {
    /// Call this only from the code path that runs after the user clicked "Request" or "Cancel request".
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
    /// Wrong username or password, or an unknown Quick Connect secret. No cookie is produced.
    #[error("invalid username or password")]
    InvalidCredentials,
    /// No valid session: sign in again (never retried in a loop).
    #[error("signed out")]
    Unauthorized,
    /// 403 with Seerr's own message ("You do not have permission to make movie requests.").
    #[error("{0}")]
    Forbidden(String),
    /// 403 "Movie Quota exceeded.": a Forbidden that is worth its own wording.
    #[error("{0}")]
    QuotaExceeded(String),
    /// 409: the title is already requested.
    #[error("already requested")]
    Conflict,
    #[error("not found")]
    NotFound,
    /// Developer API-key mode is read only.
    #[error("read only")]
    ReadOnly,
    #[error("server returned status {status} for {path}")]
    Status { status: u16, path: String },
}

pub type Result<T> = std::result::Result<T, Error>;
