//! Server writes from the app: playback reports, played and favourite. `SyncHooks` is the real
//! `PlayerHooks`: user-scope writes on the tokio bridge, never the UI thread. `Reporter` decides when
//! a playing item is reported (pure; the caller owns the clock).
use crate::bridge;
use crate::video::PlayerHooks;
use nocturne_jellyfin_user::{PlaybackReport, UserClient};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// Shown wherever a write control is disabled.
pub const NEED_WRITES: &str = "Turn on server changes in Settings";

/// The whole write policy in one place: (may write played/favourite/admin, may report progress).
/// Writes need the master switch AND a real login (developer-token mode never writes); progress
/// reporting additionally needs its own switch.
pub fn write_policy(master: bool, sync: bool, dev: bool) -> (bool, bool) {
    let w = master && !dev;
    (w, w && sync)
}

pub struct SyncHooks {
    /// `None` in developer-token mode: the real server is read-only there, every write is refused.
    client: Option<UserClient>,
    /// Settings: "Allow NOCTURNE to change data on the server" (default off).
    pub master: AtomicBool,
    /// Settings: "sync watch progress"; only counts while `master` is on.
    pub sync: AtomicBool,
    favs: Arc<Mutex<HashMap<String, bool>>>,
    notes: Arc<Mutex<Vec<Revert>>>,
}

impl SyncHooks {
    pub fn new(client: Option<UserClient>, master: bool, sync: bool) -> Self {
        Self {
            client,
            master: AtomicBool::new(master),
            sync: AtomicBool::new(sync),
            favs: Default::default(),
            notes: Default::default(),
        }
    }

    pub fn can_write(&self) -> bool {
        write_policy(
            self.master.load(Ordering::Relaxed),
            self.sync.load(Ordering::Relaxed),
            self.client.is_none(),
        )
        .0
    }

    pub fn sync_on(&self) -> bool {
        write_policy(
            self.master.load(Ordering::Relaxed),
            self.sync.load(Ordering::Relaxed),
            self.client.is_none(),
        )
        .1
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
        let Some(c) = self.client.clone().filter(|_| self.can_write()) else {
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
            Some(c) if self.can_write() => {
                let id = item_id.to_string();
                bridge::spawn(async move {
                    if c.set_favorite(&id, fav).await.is_err() {
                        revert();
                    }
                });
            }
            Some(_) => revert(),
        }
    }

    fn can_write(&self) -> bool {
        SyncHooks::can_write(self)
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

/// How often a playing item is reported to the server.
const REPORT_EVERY: Duration = Duration::from_secs(10);
/// A jump larger than this against the expected position counts as a seek.
const SEEK_JUMP: f64 = 3.0;

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Started,
    Progress,
}

struct Last {
    id: String,
    at: Instant,
    pos: f64,
    paused: bool,
    /// Latest position seen (reported or not), for the final "stopped" report.
    seen: f64,
}

/// Decides when a playing item is reported: once at start, every 10 s, and at once on pause,
/// resume or seek. Pure (the caller owns the clock and does the sending).
#[derive(Default)]
pub struct Reporter(Option<Last>);

impl Reporter {
    pub fn tick(&mut self, now: Instant, id: &str, pos: f64, paused: bool) -> Option<Tick> {
        let cur = Last {
            id: id.into(),
            at: now,
            pos,
            paused,
            seen: pos,
        };
        let Some(l) = self.0.as_mut().filter(|l| l.id == id) else {
            self.0 = Some(cur);
            return Some(Tick::Started);
        };
        l.seen = pos;
        let dt = now.duration_since(l.at);
        let expect = if l.paused {
            l.pos
        } else {
            l.pos + dt.as_secs_f64()
        };
        if l.paused != paused || (pos - expect).abs() > SEEK_JUMP || dt >= REPORT_EVERY {
            self.0 = Some(cur);
            return Some(Tick::Progress);
        }
        None
    }

    /// The item currently reported on.
    pub fn id(&self) -> Option<&str> {
        self.0.as_ref().map(|l| l.id.as_str())
    }

    /// The item reported as started and its last position (seconds), forgotten.
    pub fn stop(&mut self) -> Option<(String, f64)> {
        self.0.take().map(|l| (l.id, l.seen))
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
    fn write_policy_matrix() {
        // (master, sync, dev) -> (can_write, can_report)
        for master in [false, true] {
            for sync in [false, true] {
                for dev in [false, true] {
                    let want = (master && !dev, master && !dev && sync);
                    assert_eq!(
                        write_policy(master, sync, dev),
                        want,
                        "{master} {sync} {dev}"
                    );
                }
            }
        }
        // The default install (everything off) writes nothing.
        assert_eq!(write_policy(false, false, false), (false, false));
        // Sync alone is not enough, and dev mode refuses even with both on.
        assert_eq!(write_policy(false, true, false), (false, false));
        assert_eq!(write_policy(true, true, true), (false, false));
    }

    #[test]
    fn hooks_follow_the_policy() {
        let h = SyncHooks::new(Some(client()), true, true);
        let (_, k, r) = h.plan(Kind::Progress, "i", 50_000_000, true).unwrap();
        assert_eq!(
            (k, r.position_ticks, r.is_paused),
            (Kind::Progress, 50_000_000, true)
        );
        h.sync.store(false, Ordering::Relaxed);
        assert!(h.plan(Kind::Start, "i", 0, false).is_none(), "sync off");
        assert!(
            h.can_write(),
            "played/favourite only need the master switch"
        );
        h.sync.store(true, Ordering::Relaxed);
        h.master.store(false, Ordering::Relaxed);
        assert!(h.plan(Kind::Start, "i", 0, false).is_none(), "master off");
        assert!(!h.can_write() && !h.sync_on());
        // Developer token (no login): never writes, even with both on.
        let h = SyncHooks::new(None, true, true);
        assert!(h.plan(Kind::Stop, "i", 0, false).is_none());
        assert!(!h.sync_on() && !h.can_write());
    }

    #[test]
    fn master_off_reverts_favourite_and_played() {
        let h = SyncHooks::new(Some(client()), false, true);
        h.seed_favorite("a", false);
        h.set_favorite("a", true);
        h.set_played("a", true);
        assert!(!h.is_favorite("a"));
        let n = h.drain();
        assert_eq!((n[0].fav, n[1].played), (Some(false), Some(false)));
    }

    #[test]
    fn favorite_without_login_reverts_and_notifies() {
        let h = SyncHooks::new(None, true, true);
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
        let h = SyncHooks::new(None, true, true);
        h.set_played("a", true);
        assert_eq!(h.drain()[0].played, Some(false));
    }

    #[test]
    fn reporter_start_interval_pause_seek_stop() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut r = Reporter::default();
        assert_eq!(r.tick(at(0), "a", 5.0, false), Some(Tick::Started));
        assert_eq!(r.tick(at(1), "a", 6.0, false), None);
        assert_eq!(
            r.tick(at(9), "a", 14.0, false),
            None,
            "quiet between reports"
        );
        assert_eq!(
            r.tick(at(10), "a", 15.0, false),
            Some(Tick::Progress),
            "every 10 s"
        );
        assert_eq!(
            r.tick(at(11), "a", 16.0, true),
            Some(Tick::Progress),
            "pause"
        );
        assert_eq!(r.tick(at(15), "a", 16.0, true), None, "paused and still");
        assert_eq!(
            r.tick(at(16), "a", 16.0, false),
            Some(Tick::Progress),
            "resume"
        );
        assert_eq!(
            r.tick(at(17), "a", 90.0, false),
            Some(Tick::Progress),
            "seek forward"
        );
        assert_eq!(
            r.tick(at(18), "a", 10.0, false),
            Some(Tick::Progress),
            "seek back"
        );
        assert_eq!(
            r.tick(at(19), "b", 0.0, false),
            Some(Tick::Started),
            "new item"
        );
        assert_eq!(r.tick(at(20), "b", 1.5, false), None);
        assert_eq!(
            r.stop(),
            Some(("b".into(), 1.5)),
            "stop carries the last position seen"
        );
        assert_eq!(r.stop(), None);
        assert_eq!(
            r.tick(at(21), "b", 0.0, false),
            Some(Tick::Started),
            "after stop it starts again"
        );
    }
}
