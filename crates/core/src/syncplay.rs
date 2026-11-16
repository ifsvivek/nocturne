//! Pure rules around "Watch together" that need no window: what the screen may offer, how the
//! group's queue maps onto our items, buffering detection and the clock-sync / reconnect timing.
//! The protocol itself (state machine, clock, REST, socket) lives in `nocturne-jellyfin-user`.
use crate::source::Media;
use nocturne_jellyfin_user::syncplay::SyncAccess;
use std::time::Duration;

/// A transport request from the player while it is in a group (sent to the group, never applied
/// locally: the group's command comes back and is applied for everyone, us included).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GroupReq {
    Pause,
    Unpause,
    Seek(f64),
    Next,
    Prev,
    /// Jump to queue entry `i`.
    SetIndex(usize),
    Stop,
}

/// What the Watch together screen may offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Developer API-key mode: no login, nothing that changes the server.
    Dev,
    /// The account's policy says SyncPlay is off for it.
    NoAccess,
    /// Allowed, but the master switch is off.
    NeedWrites,
    /// Joining only (policy `JoinGroups`).
    Join,
    /// Create and join.
    Create,
}

pub fn gate(dev: bool, writes: bool, access: Option<SyncAccess>) -> Gate {
    match (dev, access) {
        (true, _) => Gate::Dev,
        (_, Some(SyncAccess::None)) => Gate::NoAccess,
        _ if !writes => Gate::NeedWrites,
        (_, Some(SyncAccess::JoinGroups)) => Gate::Join,
        _ => Gate::Create,
    }
}

impl Gate {
    pub fn can_create(self) -> bool {
        self == Gate::Create
    }
    pub fn can_join(self) -> bool {
        matches!(self, Gate::Create | Gate::Join)
    }
    /// One line saying why something is unavailable (`None` when everything is).
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Gate::Dev => Some("This connection is read-only, so it cannot take part in a group."),
            Gate::NoAccess => {
                Some("Your account is not allowed to use SyncPlay. Ask the server's administrator.")
            }
            Gate::NeedWrites => Some(
                "Joining or creating a group changes things on the server. Turn on server changes in Settings first.",
            ),
            Gate::Join => Some("Your account may join groups but not create them."),
            Gate::Create => None,
        }
    }
}

/// The group's items in queue order. `Err(n)`: `n` of them were not returned by the server
/// (removed, or not visible to this user), so the group cannot be followed.
pub fn order_items(ids: &[String], got: Vec<Media>) -> Result<Vec<Media>, usize> {
    let out: Vec<Media> = ids
        .iter()
        .filter_map(|id| got.iter().find(|m| &m.id == id).cloned())
        .collect();
    if out.len() == ids.len() {
        Ok(out)
    } else {
        Err(ids.len() - out.len())
    }
}

/// mpv's `paused-for-cache` flickers; only a stall that lasts is reported to the group.
pub const STALL_AFTER: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallEvent {
    /// Report `Buffering`.
    Began,
    /// Report `Ready`.
    Ended,
}

#[derive(Debug, Default)]
pub struct Stall {
    since: Option<Duration>,
    reported: bool,
}

impl Stall {
    /// `now`: monotonic time; `stalled`: the player is waiting for data.
    pub fn update(&mut self, now: Duration, stalled: bool) -> Option<StallEvent> {
        if stalled {
            let t0 = *self.since.get_or_insert(now);
            if !self.reported && now.saturating_sub(t0) >= STALL_AFTER {
                self.reported = true;
                return Some(StallEvent::Began);
            }
            return None;
        }
        self.since = None;
        std::mem::take(&mut self.reported).then_some(StallEvent::Ended)
    }

    pub fn reported(&self) -> bool {
        self.reported
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A loaded item may be reported Ready once it sits near where the group wants it.
pub fn settled(target_secs: f64, pos_secs: f64) -> bool {
    (target_secs - pos_secs).abs() < 1.5
}

/// Delay before clock sample number `n` (0-based): four quick ones, then every 30 s.
pub fn sample_gap(n: u32) -> Duration {
    if n < 4 {
        Duration::from_millis(400)
    } else {
        Duration::from_secs(30)
    }
}

/// After this long without a socket a group is given up locally (the server drops the session
/// at about 60 s of silence anyway).
pub const GIVE_UP_AFTER: Duration = Duration::from_secs(45);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_follows_policy_switch_and_dev_mode() {
        use SyncAccess::*;
        assert_eq!(gate(true, true, Some(CreateAndJoinGroups)), Gate::Dev);
        assert_eq!(gate(false, true, Some(None)), Gate::NoAccess);
        assert_eq!(
            gate(false, false, Some(None)),
            Gate::NoAccess,
            "policy first"
        );
        assert_eq!(
            gate(false, false, Some(CreateAndJoinGroups)),
            Gate::NeedWrites
        );
        assert_eq!(gate(false, true, Some(JoinGroups)), Gate::Join);
        assert_eq!(gate(false, true, Some(CreateAndJoinGroups)), Gate::Create);
        assert_eq!(
            gate(false, true, Option::None),
            Gate::Create,
            "policy not known yet"
        );
        assert!(Gate::Create.can_create() && Gate::Create.can_join());
        assert!(!Gate::Join.can_create() && Gate::Join.can_join());
        assert!(
            !Gate::NeedWrites.can_join() && !Gate::NoAccess.can_join() && !Gate::Dev.can_join()
        );
        assert!(Gate::Create.reason().is_none() && Gate::NoAccess.reason().is_some());
    }

    fn media(id: &str) -> Media {
        Media::from_item(&nocturne_jellyfin::Item {
            id: id.into(),
            ..Default::default()
        })
    }

    #[test]
    fn group_items_come_back_in_queue_order_or_not_at_all() {
        let ids: Vec<String> = ["b", "a", "b"].map(String::from).into();
        let got = vec![media("a"), media("b")];
        let r = order_items(&ids, got.clone()).unwrap();
        assert_eq!(
            r.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["b", "a", "b"]
        );
        assert_eq!(order_items(&["a".into(), "zz".into()], got).err(), Some(1));
    }

    #[test]
    fn only_a_lasting_stall_is_reported_and_recovery_follows() {
        let ms = Duration::from_millis;
        let mut s = Stall::default();
        assert_eq!(s.update(ms(0), true), None);
        assert_eq!(s.update(ms(200), true), None);
        assert_eq!(s.update(ms(250), false), None, "a blip is not reported");
        assert_eq!(s.update(ms(300), true), None);
        assert_eq!(s.update(ms(750), true), Some(StallEvent::Began));
        assert_eq!(s.update(ms(900), true), None, "once");
        assert!(s.reported());
        assert_eq!(s.update(ms(1000), false), Some(StallEvent::Ended));
        assert_eq!(s.update(ms(1100), false), None);
    }

    #[test]
    fn settling_and_timing() {
        assert!(settled(10.0, 10.9) && settled(10.0, 9.0));
        assert!(!settled(10.0, 12.0));
        assert_eq!(sample_gap(0), Duration::from_millis(400));
        assert_eq!(sample_gap(4), Duration::from_secs(30));
    }
}
