//! One source's operations, enum-dispatched: a Jellyfin server (`Source`) or the local library.
//! Every method answers in `Media` stamped with the backend's own origin. What a local source cannot
//! do (filmography, studios, playlists, similar, extras) answers empty; admin and SyncPlay are
//! Jellyfin-only and reached through [`Backend::jellyfin`].
use crate::detail::Full;
pub use crate::local::LocalBackend;
use crate::source::{Art, Failed, Media, ProviderIds, Source, SourceId};
use nocturne_jellyfin::BrowseQuery;
use nocturne_local::ResolveError;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone)]
pub enum Backend {
    Jellyfin(Source),
    Local(LocalBackend),
}

/// What to hand the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayTarget {
    /// A stream URL (no credentials inside) and the header lines (`Name: value`) to send with it.
    Url {
        url: String,
        headers: Vec<String>,
    },
    File(PathBuf),
}

/// Why an item cannot be played right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayError {
    /// The file is gone from its folder.
    Missing,
    /// Its folder is not mounted or not readable.
    Unavailable,
    /// The file exists but cannot be opened.
    Unreadable,
    /// Not something that can be played (a show, a library, an unknown id).
    NotPlayable,
}

impl PlayError {
    /// Plain words for the detail page.
    pub fn message(self) -> &'static str {
        match self {
            Self::Missing => "This file is no longer in its folder.",
            Self::Unavailable => "This folder is not available right now. Is the drive mounted?",
            Self::Unreadable => "This file cannot be opened. Check its permissions.",
            Self::NotPlayable => "This item cannot be played.",
        }
    }
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl From<ResolveError> for PlayError {
    fn from(e: ResolveError) -> Self {
        match e {
            ResolveError::Unknown => Self::NotPlayable,
            ResolveError::Missing => Self::Missing,
            ResolveError::Unavailable => Self::Unavailable,
            ResolveError::Unreadable => Self::Unreadable,
        }
    }
}

impl Backend {
    pub fn origin(&self) -> SourceId {
        match self {
            Self::Jellyfin(s) => s.origin(),
            Self::Local(l) => l.origin(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    /// The server session, for admin, SyncPlay, playlists and the rest of what only a server has.
    pub fn jellyfin(&self) -> Option<&Source> {
        match self {
            Self::Jellyfin(s) => Some(s),
            Self::Local(_) => None,
        }
    }

    pub fn local(&self) -> Option<&LocalBackend> {
        match self {
            Self::Local(l) => Some(l),
            Self::Jellyfin(_) => None,
        }
    }

    pub async fn libraries(&self) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.libraries().await,
            Self::Local(l) => l.libraries().await,
        }
    }

    /// Like [`Self::libraries`], with the Collections and Playlists views kept.
    pub async fn libraries_all(&self) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.libraries_all().await,
            Self::Local(l) => l.libraries().await,
        }
    }

    /// Server UI data; a local source has none.
    pub async fn server_ui(&self) -> Result<crate::server_ui::ServerUi, Failed> {
        match self {
            Self::Jellyfin(s) => s.server_ui().await,
            Self::Local(_) => Err(Failed),
        }
    }

    pub async fn latest(&self, parent: &Media) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.latest(parent).await,
            Self::Local(l) => l.latest(parent).await,
        }
    }

    /// Items with a saved position, most recently watched first.
    pub async fn resume(&self) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.resume().await,
            Self::Local(l) => l.resume().await,
        }
    }

    pub async fn next_up(&self) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.next_up().await,
            Self::Local(l) => l.next_up().await,
        }
    }

    /// Upcoming episodes (servers only: a local library has no air dates to wait for).
    pub async fn upcoming(&self) -> Result<Vec<crate::calendar::Upcoming>, Failed> {
        match self {
            Self::Jellyfin(s) => s.upcoming().await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    pub async fn series_next_up(&self, series: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.series_next_up(series).await,
            Self::Local(l) => l.series_next_up(series).await,
        }
    }

    /// One page of a filtered/sorted query plus the total.
    pub async fn browse(&self, q: &BrowseQuery) -> Result<(Vec<Media>, usize), Failed> {
        match self {
            Self::Jellyfin(s) => s.browse(q).await,
            Self::Local(l) => l.browse(q).await,
        }
    }

    pub async fn full(&self, id: &str) -> Result<(Media, Full), Failed> {
        match self {
            Self::Jellyfin(s) => s.full(id).await,
            Self::Local(l) => l.full(id).await,
        }
    }

    pub async fn seasons(&self, series: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.seasons(series).await,
            Self::Local(l) => l.seasons(series).await,
        }
    }

    pub async fn episodes(&self, series: &str, season: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.episodes(series, season).await,
            Self::Local(l) => l.episodes(series, season).await,
        }
    }

    pub async fn search(&self, term: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.search(term).await,
            Self::Local(l) => l.search(term).await,
        }
    }

    /// Image bytes; `None` when the item has no such image (or it could not be fetched).
    pub async fn image(&self, m: &Media, art: Art, width: u32) -> Option<Vec<u8>> {
        match self {
            Self::Jellyfin(s) => s.image(m, art, width).await.ok(),
            Self::Local(l) => l.image(m, art).await,
        }
    }

    /// Provider ids of items by id (a batched read on a server, the index locally).
    pub async fn provider_ids(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, ProviderIds>, Failed> {
        match self {
            Self::Jellyfin(s) => s.provider_ids(ids).await,
            Self::Local(l) => l.provider_ids(ids).await,
        }
    }

    pub async fn play_target(&self, m: &Media) -> Result<PlayTarget, PlayError> {
        match self {
            Self::Jellyfin(s) => {
                if !m.playable() {
                    return Err(PlayError::NotPlayable);
                }
                let (c, _) = s.client();
                // The token header stays first; the server's extra header (if any) follows.
                let mut headers = vec![format!("Authorization: {}", c.auth_header())];
                headers.extend(c.extra_header_line());
                Ok(PlayTarget::Url {
                    url: c.stream_url(&m.id),
                    headers,
                })
            }
            Self::Local(l) => Ok(PlayTarget::File(l.resolve(m).await?)),
        }
    }

    // ---- server-only reads: empty on a local source ----

    pub async fn filmography(&self, person: &str, types: &[&str]) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.filmography(person, types).await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    pub async fn studios(&self, parent: Option<&str>) -> Result<Vec<(String, String)>, Failed> {
        match self {
            Self::Jellyfin(s) => s.studios(parent).await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    pub async fn own_playlists(&self) -> Result<Vec<(String, String)>, Failed> {
        match self {
            Self::Jellyfin(s) => s.own_playlists().await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    pub async fn similar(&self, id: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.similar(id).await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    /// Collections this item is part of (server only; local items have none).
    pub async fn collections_of(&self, id: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.collections_of(id).await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    // ---- local state (no server is involved) ----

    /// Mark a local item (un)played. A server item is changed by the user client of its own source,
    /// only with server writes on; here it is refused.
    pub async fn set_played_local(&self, m: &Media, played: bool) -> Result<(), Failed> {
        match self {
            Self::Local(l) => l.set_played(m, played).await,
            Self::Jellyfin(_) => Err(Failed),
        }
    }

    pub async fn set_progress_local(&self, m: &Media, secs: u64) -> Result<(), Failed> {
        match self {
            Self::Local(l) => l.set_progress(m, secs).await,
            Self::Jellyfin(_) => Err(Failed),
        }
    }
}
