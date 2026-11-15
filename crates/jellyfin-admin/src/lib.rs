//! Jellyfin admin client: reads that need admin rights, and every write.
//!
//! `nocturne-jellyfin` stays strictly read-only; all mutation lives here, in [`write`].
//! Destructive writes (`delete_*`, `remove_*`, `set_network`, `remove_plugin_repository`, `set_user_password`, `reset_user_password`, `restart_server`)
//! take a [`Confirmed`] token that UI code may only mint after the user said yes.
//! Writes are tested ONLY against the throwaway server from `scripts/test-jellyfin.sh`
//! (see [`guard::TestServerGuard`]), never the user's real server.

mod client;
pub mod edit;
pub mod guard;
#[cfg(test)]
mod mock;
mod model;
mod write;

pub use client::{AdminClient, tail_lines};
pub use edit::NetworkEdit;
pub use model::*;
pub use nocturne_jellyfin::Config;
pub use write::ConfigPatch;

/// Proof that the user explicitly confirmed a destructive action. Zero-size; the private field
/// means it cannot be built by struct literal, only via [`Confirmed::i_have_asked_the_user`].
///
/// ```compile_fail
/// let _ = nocturne_jellyfin_admin::Confirmed(());
/// ```
/// Destructive calls do not compile without it:
/// ```compile_fail
/// async fn f(c: &nocturne_jellyfin_admin::AdminClient) { let _ = c.delete_user("id").await; }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Confirmed(());

impl Confirmed {
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
    #[error("server returned status {status} for {path}")]
    Status { status: u16, path: String },
    /// A value was refused before anything was sent to the server.
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("test guard: {0}")]
    Guard(String),
}

pub type Result<T> = std::result::Result<T, Error>;
