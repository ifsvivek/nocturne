//! Contract with the player (video.rs): `PlayerPrefs` flows in from Settings, `PlayerHooks` is how
//! the player reports back. The three prefs types below are PLACEHOLDERS with the exact shape the
//! player engineer defines in video.rs; the orchestrator removes them when both halves land.
//! `SyncHooks` is the real implementation: user-scope writes on the tokio bridge, never the UI thread.
#![allow(dead_code)]
use crate::bridge;
use nocturne_jellyfin_user::{PlaybackReport, UserClient};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubMode {
    #[default]
    Off,
    ForeignOnly,
    Always,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerPrefs {
    pub audio_lang: Option<String>,
    pub sub_lang: Option<String>,
    pub sub_mode: SubMode,
    pub auto_skip_intro: bool,
    pub autoplay_next: bool,
}

pub trait PlayerHooks {
    fn started(&self, item_id: &str, position_ticks: i64);
    fn progress(&self, item_id: &str, position_ticks: i64, paused: bool);
    fn stopped(&self, item_id: &str, position_ticks: i64);
    fn set_favorite(&self, item_id: &str, fav: bool);
    fn is_favorite(&self, item_id: &str) -> bool;
}

/// 1 tick = 100 ns.
pub fn ticks(secs: f64) -> i64 {
    (secs.max(0.0) * 10_000_000.0) as i64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Start,
    Progress,
    Stop,
}

/// A write that failed: put the optimistic value back (`None` = leave that flag alone).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revert {
    pub id: String,
    pub played: Option<bool>,
    pub fav: Option<bool>,
}

pub struct SyncHooks {
    /// `None` in developer-token mode: the real server is read-only there, every write is refused.
    client: Option<UserClient>,
    /// Settings: "sync watch progress and watched state".
    pub sync: AtomicBool,
    favs: Arc<Mutex<HashMap<String, bool>>>,
    notes: Arc<Mutex<Vec<Revert>>>,
}

impl SyncHooks {
    pub fn new(client: Option<UserClient>, sync: bool) -> Self {
        Self {
            client,
            sync: AtomicBool::new(sync),
            favs: Default::default(),
            notes: Default::default(),
        }
    }

    pub fn can_write(&self) -> bool {
        self.client.is_some()
    }

    pub fn sync_on(&self) -> bool {
        self.can_write() && self.sync.load(Ordering::Relaxed)
    }

    /// The request to send, or `None` when syncing is off / there is no login.
    fn plan(
        &self,
        kind: Kind,
        item: &str,
        ticks: i64,
        paused: bool,
    ) -> Option<(UserClient, Kind, PlaybackReport)> {
        if !self.sync_on() {
            return None;
        }
        let report = PlaybackReport {
            item_id: item.into(),
            position_ticks: ticks.max(0) as u64,
            is_paused: paused,
            ..Default::default()
        };
        Some((self.client.clone()?, kind, report))
    }

    fn send(&self, kind: Kind, item: &str, ticks: i64, paused: bool) {
        if let Some((c, k, r)) = self.plan(kind, item, ticks, paused) {
            bridge::spawn(async move {
                // Progress reports are best effort; a missed one is replaced by the next.
                let _ = match k {
                    Kind::Start => c.report_playback_start(&r).await,
                    Kind::Progress => c.report_playback_progress(&r).await,
                    Kind::Stop => c.report_playback_stopped(&r).await,
                };
            });
        }
    }

    /// Optimistic played / unplayed. Failure queues a [`Revert`].
    pub fn set_played(&self, id: &str, played: bool) {
        let (notes, id) = (self.notes.clone(), id.to_string());
        let Some(c) = self.client.clone() else {
            return self.notes.lock().unwrap().push(Revert {
                id,
                played: Some(!played),
                fav: None,
            });
        };
        bridge::spawn(async move {
            let r = if played {
                c.mark_played(&id).await
            } else {
                c.mark_unplayed(&id).await
            };
            if r.is_err() {
                notes.lock().unwrap().push(Revert {
                    id,
                    played: Some(!played),
                    fav: None,
                });
            }
        });
    }

    /// Record what the server said about an item so `is_favorite` is right before any toggle.
    pub fn seed_favorite(&self, id: &str, fav: bool) {
        self.favs.lock().unwrap().insert(id.into(), fav);
    }

    pub fn drain(&self) -> Vec<Revert> {
        std::mem::take(&mut *self.notes.lock().unwrap())
    }
}

impl PlayerHooks for SyncHooks {
    fn started(&self, item_id: &str, position_ticks: i64) {
        self.send(Kind::Start, item_id, position_ticks, false);
    }
    fn progress(&self, item_id: &str, position_ticks: i64, paused: bool) {
        self.send(Kind::Progress, item_id, position_ticks, paused);
    }
    fn stopped(&self, item_id: &str, position_ticks: i64) {
        self.send(Kind::Stop, item_id, position_ticks, false);
    }

    fn set_favorite(&self, item_id: &str, fav: bool) {
        let old = self
            .favs
            .lock()
            .unwrap()
            .insert(item_id.into(), fav)
            .unwrap_or(!fav);
        let (favs, notes, id) = (self.favs.clone(), self.notes.clone(), item_id.to_string());
        let revert = move || {
            favs.lock().unwrap().insert(id.clone(), old);
            notes.lock().unwrap().push(Revert {
                id,
                played: None,
                fav: Some(old),
            });
        };
        match self.client.clone() {
            None => revert(),
            Some(c) => {
                let id = item_id.to_string();
                bridge::spawn(async move {
                    if c.set_favorite(&id, fav).await.is_err() {
                        revert();
                    }
                });
            }
        }
    }

    fn is_favorite(&self, item_id: &str) -> bool {
        self.favs
            .lock()
            .unwrap()
            .get(item_id)
            .copied()
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_jellyfin_user::Session;

    fn client() -> UserClient {
        let s = Session {
            access_token: "t".into(),
            user_id: "u".into(),
            server_id: "s".into(),
            user_name: "n".into(),
            is_admin: false,
        };
        UserClient::from_session("http://127.0.0.1:1", &s).unwrap()
    }

    #[test]
    fn tick_conversion() {
        assert_eq!(ticks(1.0), 10_000_000);
        assert_eq!(ticks(90.5), 905_000_000);
        assert_eq!(ticks(-3.0), 0);
        assert_eq!(ticks(0.0), 0);
    }

    #[test]
    fn sync_gating() {
        let h = SyncHooks::new(Some(client()), true);
        let (_, k, r) = h.plan(Kind::Progress, "i", 50_000_000, true).unwrap();
        assert_eq!(
            (k, r.position_ticks, r.is_paused),
            (Kind::Progress, 50_000_000, true)
        );
        h.sync.store(false, Ordering::Relaxed);
        assert!(
            h.plan(Kind::Start, "i", 0, false).is_none(),
            "sync off sends nothing"
        );
        // No login (developer token): never writes, even with sync on.
        let h = SyncHooks::new(None, true);
        assert!(h.plan(Kind::Stop, "i", 0, false).is_none());
        assert!(!h.sync_on());
    }

    #[test]
    fn favorite_without_login_reverts_and_notifies() {
        let h = SyncHooks::new(None, true);
        h.seed_favorite("a", false);
        h.set_favorite("a", true);
        assert!(!h.is_favorite("a"), "rolled back");
        assert_eq!(
            h.drain(),
            [Revert {
                id: "a".into(),
                played: None,
                fav: Some(false)
            }]
        );
        assert!(h.drain().is_empty());
    }

    #[test]
    fn played_without_login_reverts() {
        let h = SyncHooks::new(None, true);
        h.set_played("a", true);
        assert_eq!(h.drain()[0].played, Some(false));
    }
}
