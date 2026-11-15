//! The SyncPlay state machine, with no I/O and no clock of its own: every method takes `now`
//! (local epoch ms) and returns what the caller should do. The rules follow the official web
//! client (command scheduling, SpeedToSync / SkipToSync drift correction, stale-update
//! rejection), re-implemented from its documented behaviour; see docs/syncplay.md.
use super::model::*;
use super::time::TimeSync;

/// Drift-correction thresholds. Defaults are the web client's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// Below this drift nothing is done.
    pub min_speed_ms: f64,
    /// At or above this drift SpeedToSync gives way to SkipToSync.
    pub max_speed_ms: f64,
    /// How long a speed nudge lasts.
    pub speed_ms: f64,
    /// Smallest drift worth a seek.
    pub min_skip_ms: f64,
    pub enabled: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            min_speed_ms: 60.0,
            max_speed_ms: 3000.0,
            speed_ms: 1000.0,
            min_skip_ms: 400.0,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drift {
    /// Play at `factor` times normal speed for `for_ms`, then back to 1.0.
    Speed {
        factor: f64,
        for_ms: f64,
    },
    Skip {
        to_ticks: i64,
    },
}

/// Pure decision. `diff_ms` is where the group is minus where we are (positive: we are behind).
pub fn drift_decision(diff_ms: f64, t: &Tuning) -> Option<Drift> {
    let abs = diff_ms.abs();
    if abs >= t.min_speed_ms && abs < t.max_speed_ms {
        // Being ahead by more than 80% of the nudge would need a speed near zero or below.
        let mut dur = t.speed_ms;
        if diff_ms <= -dur * 0.2 {
            dur = abs / 0.8;
        }
        return Some(Drift::Speed {
            factor: 1.0 + diff_ms / dur,
            for_ms: dur,
        });
    }
    (abs >= t.min_skip_ms).then_some(Drift::Skip { to_ticks: 0 })
}

/// What the local player is doing right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerSnap {
    pub pos_ticks: i64,
    pub playing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Unpause { pos_ticks: i64 },
    Pause { pos_ticks: i64 },
    Seek { pos_ticks: i64 },
    Stop,
}

/// What the caller must do.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// Run `op` in `in_ms` (0 = now), replacing any earlier scheduled op. `pre_seek`: jump there
    /// right away so the player is in place when `op` fires.
    Apply {
        pre_seek: Option<i64>,
        in_ms: f64,
        op: Op,
    },
    /// A new accepted queue. `switch`: the playing item changed (or it is a new playlist), so the
    /// player must load `queue.playing()`.
    Queue {
        queue: PlayQueue,
        switch: bool,
    },
    Joined(GroupInfo),
    /// No longer in a group (server said so, or it is gone).
    Left,
    Notice(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub when_ms: f64,
    pub pos_ticks: i64,
    pub playing: bool,
    pub playlist_item_id: String,
}

#[derive(Debug, Default)]
pub struct Engine {
    pub tuning: Tuning,
    pub time: TimeSync,
    group: Option<GroupInfo>,
    queue: Option<PlayQueue>,
    /// Server time when we joined: older commands are ignored.
    joined_at: f64,
    last_cmd: Option<SendCommand>,
    /// A command that arrived before the first clock sample.
    parked: Option<SendCommand>,
    /// Drift correction may run from this local time on (0: not yet).
    drift_from: f64,
    drift_next: f64,
    /// A speed nudge ends at this local time.
    speed_until: Option<f64>,
    /// Last measured drift (ms), for the stats overlay.
    pub diff_ms: f64,
}

impl Engine {
    pub fn new(tuning: Tuning) -> Self {
        Self {
            tuning,
            ..Default::default()
        }
    }

    pub fn group(&self) -> Option<&GroupInfo> {
        self.group.as_ref()
    }

    pub fn in_group(&self) -> bool {
        self.group.is_some()
    }

    pub fn queue(&self) -> Option<&PlayQueue> {
        self.queue.as_ref()
    }

    pub fn last_command(&self) -> Option<&SendCommand> {
        self.last_cmd.as_ref()
    }

    pub fn playlist_item_id(&self) -> Option<&str> {
        self.queue
            .as_ref()
            .and_then(PlayQueue::playing)
            .map(|i| i.playlist_item_id.as_str())
    }

    /// Back to "not in a group". Clock samples are kept.
    pub fn reset(&mut self) {
        *self = Self {
            tuning: self.tuning,
            time: std::mem::take(&mut self.time),
            ..Default::default()
        };
    }

    /// We are in `g`. `now`: local epoch ms (used when the server gave no timestamp).
    pub fn join(&mut self, g: GroupInfo, now: f64) {
        self.joined_at = g
            .last_updated_ms()
            .unwrap_or_else(|| self.time.local_to_server(now));
        self.group = Some(g);
        self.queue = None;
        self.last_cmd = None;
        self.parked = None;
        self.drift_from = 0.0;
        self.speed_until = None;
    }

    /// Estimated group position at `now`, from a position that was true at server time `at`.
    pub fn estimate_ticks(&self, ticks: i64, at_server_ms: f64, now: f64) -> i64 {
        ticks + ((self.time.local_to_server(now) - at_server_ms) * TICKS_PER_MS) as i64
    }

    /// Where a freshly loaded item should start so a late joiner lands on the group's position.
    pub fn start_ticks(&self, now: f64) -> i64 {
        let Some(q) = &self.queue else { return 0 };
        match &self.last_cmd {
            Some(c)
                if c.emitted_at_ms >= q.last_update_ms
                    && c.playlist_item_id == self.playlist_item_id().unwrap_or("") =>
            {
                if c.command == CommandKind::Unpause {
                    self.estimate_ticks(c.position_ticks, c.when_ms, now).max(0)
                } else {
                    c.position_ticks
                }
            }
            _ if q.is_playing => self
                .estimate_ticks(q.start_position_ticks, q.last_update_ms, now)
                .max(0),
            _ => q.start_position_ticks,
        }
    }

    pub fn group_update(&mut self, u: GroupUpdate, now: f64) -> Vec<Out> {
        match u {
            GroupUpdate::GroupJoined(g) => {
                if self
                    .group
                    .as_ref()
                    .is_some_and(|c| c.group_id == g.group_id)
                {
                    return vec![];
                }
                self.join(g.clone(), now);
                vec![Out::Joined(g)]
            }
            GroupUpdate::GroupLeft | GroupUpdate::NotInGroup => {
                if self.group.is_none() {
                    return vec![];
                }
                self.reset();
                vec![Out::Left]
            }
            GroupUpdate::UserJoined(n) => {
                if let Some(g) = &mut self.group {
                    g.participants.push(n.clone());
                }
                vec![Out::Notice(format!("{n} joined the group"))]
            }
            GroupUpdate::UserLeft(n) => {
                if let Some(g) = &mut self.group
                    && let Some(i) = g.participants.iter().position(|p| *p == n)
                {
                    g.participants.remove(i);
                }
                vec![Out::Notice(format!("{n} left the group"))]
            }
            GroupUpdate::StateUpdate { state, .. } => {
                if let Some(g) = &mut self.group {
                    g.state = state;
                }
                vec![]
            }
            GroupUpdate::GroupDoesNotExist => {
                vec![Out::Notice("That group no longer exists".into())]
            }
            GroupUpdate::LibraryAccessDenied => vec![Out::Notice(
                "You do not have access to everything this group is watching".into(),
            )],
            GroupUpdate::PlayQueue(q) => self.accept_queue(q),
        }
    }

    fn accept_queue(&mut self, q: PlayQueue) -> Vec<Out> {
        if self.group.is_none() {
            return vec![];
        }
        if self
            .queue
            .as_ref()
            .is_some_and(|o| q.last_update_ms <= o.last_update_ms)
        {
            return vec![]; // stale or duplicate
        }
        let was = self.playlist_item_id().map(str::to_owned);
        let switch = q.reason == QueueReason::NewPlaylist
            || (matches!(
                q.reason,
                QueueReason::SetCurrentItem
                    | QueueReason::NextItem
                    | QueueReason::PreviousItem
                    | QueueReason::RemoveItems
            ) && q.playing().map(|i| i.playlist_item_id.clone()) != was);
        self.queue = Some(q.clone());
        self.speed_until = None;
        if switch {
            self.last_cmd = None;
            self.drift_from = 0.0;
        }
        vec![Out::Queue { queue: q, switch }]
    }

    /// A playback command. `snap`: the local player, `None` when nothing plays.
    pub fn command(&mut self, c: SendCommand, now: f64, snap: Option<PlayerSnap>) -> Vec<Out> {
        if self.group.is_none() || c.emitted_at_ms < self.joined_at {
            return vec![];
        }
        if !self.time.ready() {
            self.parked = Some(c);
            return vec![];
        }
        // Remembered even with no player yet: a late joiner starts from it.
        let prev = self.last_cmd.replace(c.clone());
        self.speed_until = None;
        self.drift_from = 0.0;
        let Some(snap) = snap else { return vec![] };
        // Commands for another item than the one playing would spoil it (except Stop).
        if c.command != CommandKind::Stop
            && Some(c.playlist_item_id.as_str()) != self.playlist_item_id()
        {
            return vec![];
        }
        let in_ms = self.time.server_to_local(c.when_ms) - now;
        let dup = prev.is_some_and(|l| {
            l.when_ms == c.when_ms
                && l.position_ticks == c.position_ticks
                && l.command == c.command
                && l.playlist_item_id == c.playlist_item_id
        });
        if dup {
            // Server resent it ("client got lost"): only act if our player disagrees.
            if in_ms > 0.0 {
                return vec![];
            }
            let off = (snap.pos_ticks - c.position_ticks).abs() as f64 / TICKS_PER_MS
                > self.tuning.min_skip_ms;
            let bad = match c.command {
                CommandKind::Unpause => !snap.playing,
                CommandKind::Pause | CommandKind::Seek => snap.playing || off,
                CommandKind::Stop => snap.playing,
            };
            if !bad {
                return vec![];
            }
        }
        vec![self.plan(&c, in_ms, snap, now)]
    }

    fn plan(&self, c: &SendCommand, in_ms: f64, snap: PlayerSnap, now: f64) -> Out {
        let future = in_ms > 0.0;
        let far = |a: i64, b: i64| (a - b).abs() as f64 / TICKS_PER_MS > self.tuning.min_skip_ms;
        let (pre_seek, op) = match c.command {
            CommandKind::Unpause if future => (
                far(snap.pos_ticks, c.position_ticks).then_some(c.position_ticks),
                Op::Unpause {
                    pos_ticks: c.position_ticks,
                },
            ),
            // The group already started: jump to where it is now and go.
            CommandKind::Unpause => (
                None,
                Op::Unpause {
                    pos_ticks: self.estimate_ticks(c.position_ticks, c.when_ms, now),
                },
            ),
            CommandKind::Pause => (
                None,
                Op::Pause {
                    pos_ticks: c.position_ticks,
                },
            ),
            CommandKind::Seek => (
                None,
                Op::Seek {
                    pos_ticks: c.position_ticks,
                },
            ),
            CommandKind::Stop => (None, Op::Stop),
        };
        Out::Apply {
            pre_seek,
            in_ms: in_ms.max(0.0),
            op,
        }
    }

    /// Call when the first clock sample arrived: replays a command that was waiting for it.
    pub fn clock_ready(&mut self, now: f64, snap: Option<PlayerSnap>) -> Vec<Out> {
        match self.parked.take() {
            Some(c) => self.command(c, now, snap),
            None => vec![],
        }
    }

    /// The scheduled `Unpause` just ran: drift correction may start after a settling time.
    pub fn unpaused(&mut self, now: f64) {
        self.drift_from = now + self.tuning.max_speed_ms / 2.0;
        self.drift_next = self.drift_from;
    }

    /// Cancel scheduled work (the caller resets the playback speed).
    pub fn cancel_scheduled(&mut self) {
        self.drift_from = 0.0;
        self.speed_until = None;
    }

    /// True once when a speed nudge has run its course: set speed back to 1.0.
    pub fn speed_reset_due(&mut self, now: f64) -> bool {
        match self.speed_until {
            Some(t) if now >= t => {
                self.speed_until = None;
                true
            }
            _ => false,
        }
    }

    /// Drift between the group and the local player, ms (positive: we are behind). `None` unless
    /// the group is playing an item we also play.
    pub fn drift_ms(&self, now: f64, snap: PlayerSnap) -> Option<f64> {
        let c = self.last_cmd.as_ref()?;
        if c.command != CommandKind::Unpause
            || Some(c.playlist_item_id.as_str()) != self.playlist_item_id()
        {
            return None;
        }
        let want = self.estimate_ticks(c.position_ticks, c.when_ms, now);
        Some((want - snap.pos_ticks) as f64 / TICKS_PER_MS)
    }

    /// Called every poll while playing. Returns a correction when one is due.
    pub fn drift(&mut self, now: f64, snap: PlayerSnap, buffering: bool) -> Option<Drift> {
        if buffering || !snap.playing {
            return None;
        }
        let diff = self.drift_ms(now, snap)?;
        self.diff_ms = diff;
        if !self.tuning.enabled
            || self.drift_from == 0.0
            || now < self.drift_from.max(self.drift_next)
        {
            return None;
        }
        self.drift_next = now + self.tuning.max_speed_ms / 2.0;
        match drift_decision(diff, &self.tuning)? {
            Drift::Speed { factor, for_ms } => {
                self.speed_until = Some(now + for_ms);
                self.drift_next = now + for_ms + self.tuning.max_speed_ms / 2.0;
                Some(Drift::Speed { factor, for_ms })
            }
            Drift::Skip { .. } => {
                let c = self.last_cmd.as_ref()?;
                Some(Drift::Skip {
                    to_ticks: self.estimate_ticks(c.position_ticks, c.when_ms, now).max(0),
                })
            }
        }
    }

    /// Body of a `Buffering` / `Ready` request.
    pub fn report(&self, now: f64, snap: PlayerSnap) -> Option<Report> {
        Some(Report {
            when_ms: self.time.local_to_server(now),
            pos_ticks: snap.pos_ticks.max(0),
            playing: snap.playing,
            playlist_item_id: self.playlist_item_id()?.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syncplay::time::Sample;

    const T0: f64 = 1_000_000.0; // local "now"
    const OFF: f64 = 5_000.0; // server clock is 5 s ahead

    fn engine() -> Engine {
        let mut e = Engine::new(Tuning::default());
        e.time.add(Sample {
            sent: T0,
            server_received: T0 + OFF + 5.0,
            server_sent: T0 + OFF + 5.0,
            received: T0 + 10.0,
        });
        assert_eq!(e.time.offset(), OFF);
        e
    }

    fn group() -> GroupInfo {
        serde_json::from_str(r#"{"GroupId":"g","GroupName":"n","State":"Idle","Participants":["a"],"LastUpdatedAt":"1970-01-01T00:00:00Z"}"#).unwrap()
    }

    fn queue(reason: QueueReason, at: f64, idx: i32) -> PlayQueue {
        PlayQueue {
            reason,
            last_update_ms: at,
            playlist: vec![
                QueueItem {
                    item_id: "i1".into(),
                    playlist_item_id: "p1".into(),
                },
                QueueItem {
                    item_id: "i2".into(),
                    playlist_item_id: "p2".into(),
                },
            ],
            playing_item_index: idx,
            start_position_ticks: 0,
            is_playing: false,
        }
    }

    fn joined() -> Engine {
        let mut e = engine();
        e.group_update(GroupUpdate::GroupJoined(group()), T0);
        e.group_update(
            GroupUpdate::PlayQueue(queue(QueueReason::NewPlaylist, 10.0, 0)),
            T0,
        );
        e
    }

    fn cmd(kind: CommandKind, when_local: f64, ticks: i64) -> SendCommand {
        SendCommand {
            group_id: "g".into(),
            playlist_item_id: "p1".into(),
            when_ms: when_local + OFF,
            emitted_at_ms: T0 + OFF - 100.0,
            position_ticks: ticks,
            command: kind,
        }
    }

    const PAUSED_AT_0: PlayerSnap = PlayerSnap {
        pos_ticks: 0,
        playing: false,
    };

    #[test]
    fn unpause_in_the_future_is_scheduled_by_server_time() {
        let mut e = joined();
        let out = e.command(
            cmd(CommandKind::Unpause, T0 + 1500.0, 0),
            T0,
            Some(PAUSED_AT_0),
        );
        assert_eq!(
            out,
            vec![Out::Apply {
                pre_seek: None,
                in_ms: 1500.0,
                op: Op::Unpause { pos_ticks: 0 }
            }]
        );
    }

    #[test]
    fn unpause_pre_seeks_only_when_the_player_is_off_position() {
        let mut e = joined();
        let at = PlayerSnap {
            pos_ticks: 600 * 10_000,
            playing: false,
        }; // 600 ms off
        let out = e.command(cmd(CommandKind::Unpause, T0 + 1000.0, 0), T0, Some(at));
        assert_eq!(
            out,
            vec![Out::Apply {
                pre_seek: Some(0),
                in_ms: 1000.0,
                op: Op::Unpause { pos_ticks: 0 }
            }]
        );
        let near = PlayerSnap {
            pos_ticks: 300 * 10_000,
            playing: false,
        };
        let out = e.command(cmd(CommandKind::Unpause, T0 + 1000.0, 1), T0, Some(near));
        assert!(matches!(&out[..], [Out::Apply { pre_seek: None, .. }]));
    }

    #[test]
    fn late_unpause_starts_now_at_the_estimated_position() {
        let mut e = joined();
        // The group started 2 s ago at 10 s.
        let out = e.command(
            cmd(CommandKind::Unpause, T0 - 2000.0, 100_000_000),
            T0,
            Some(PAUSED_AT_0),
        );
        assert_eq!(
            out,
            vec![Out::Apply {
                pre_seek: None,
                in_ms: 0.0,
                op: Op::Unpause {
                    pos_ticks: 120_000_000
                }
            }]
        );
    }

    #[test]
    fn pause_seek_stop_map_to_ops() {
        let mut e = joined();
        let snap = Some(PlayerSnap {
            pos_ticks: 5,
            playing: true,
        });
        let o = e.command(cmd(CommandKind::Pause, T0 + 100.0, 77), T0, snap);
        assert_eq!(
            o,
            vec![Out::Apply {
                pre_seek: None,
                in_ms: 100.0,
                op: Op::Pause { pos_ticks: 77 }
            }]
        );
        let o = e.command(cmd(CommandKind::Seek, T0, 99), T0, snap);
        assert_eq!(
            o,
            vec![Out::Apply {
                pre_seek: None,
                in_ms: 0.0,
                op: Op::Seek { pos_ticks: 99 }
            }]
        );
        let mut s = cmd(CommandKind::Stop, T0, 0);
        s.playlist_item_id = "other".into(); // Stop is allowed for any item
        assert_eq!(
            e.command(s, T0, snap),
            vec![Out::Apply {
                pre_seek: None,
                in_ms: 0.0,
                op: Op::Stop
            }]
        );
    }

    #[test]
    fn commands_for_the_wrong_item_or_before_joining_or_without_a_group_are_ignored() {
        let mut e = joined();
        let mut c = cmd(CommandKind::Pause, T0, 0);
        c.playlist_item_id = "p2".into();
        assert!(e.command(c, T0, Some(PAUSED_AT_0)).is_empty());
        let mut old = cmd(CommandKind::Pause, T0, 0);
        old.emitted_at_ms = -1.0;
        e.joined_at = 0.0;
        assert!(e.command(old, T0, Some(PAUSED_AT_0)).is_empty());
        assert!(
            joined()
                .command(cmd(CommandKind::Pause, T0, 0), T0, None)
                .is_empty(),
            "no player"
        );
        let mut none = engine();
        assert!(
            none.command(cmd(CommandKind::Pause, T0, 0), T0, Some(PAUSED_AT_0))
                .is_empty()
        );
    }

    #[test]
    fn a_command_before_the_clock_is_known_waits_for_it() {
        let mut e = Engine::new(Tuning::default());
        e.group_update(GroupUpdate::GroupJoined(group()), T0);
        e.group_update(
            GroupUpdate::PlayQueue(queue(QueueReason::NewPlaylist, 10.0, 0)),
            T0,
        );
        assert!(
            e.command(cmd(CommandKind::Pause, T0, 0), T0, Some(PAUSED_AT_0))
                .is_empty()
        );
        e.time.add(Sample {
            sent: T0,
            server_received: T0 + 5.0,
            server_sent: T0 + 5.0,
            received: T0 + 10.0,
        });
        assert_eq!(e.clock_ready(T0, Some(PAUSED_AT_0)).len(), 1);
        assert!(e.clock_ready(T0, Some(PAUSED_AT_0)).is_empty(), "only once");
    }

    #[test]
    fn a_resent_command_only_acts_when_the_player_disagrees() {
        let mut e = joined();
        let c = cmd(CommandKind::Unpause, T0 - 1000.0, 0);
        let playing = Some(PlayerSnap {
            pos_ticks: 0,
            playing: true,
        });
        assert_eq!(e.command(c.clone(), T0, playing).len(), 1);
        assert!(
            e.command(c.clone(), T0, playing).is_empty(),
            "already playing"
        );
        assert_eq!(
            e.command(c.clone(), T0, Some(PAUSED_AT_0)).len(),
            1,
            "player is paused: re-apply"
        );
        let fut = cmd(CommandKind::Pause, T0 + 900.0, 5);
        assert_eq!(e.command(fut.clone(), T0, playing).len(), 1);
        assert!(e.command(fut, T0, playing).is_empty(), "already scheduled");
    }

    #[test]
    fn stale_queue_updates_are_dropped_and_item_changes_flagged() {
        let mut e = joined();
        assert!(
            e.group_update(
                GroupUpdate::PlayQueue(queue(QueueReason::NextItem, 10.0, 1)),
                T0
            )
            .is_empty(),
            "same timestamp"
        );
        assert!(
            e.group_update(
                GroupUpdate::PlayQueue(queue(QueueReason::NextItem, 5.0, 1)),
                T0
            )
            .is_empty(),
            "older"
        );
        let o = e.group_update(
            GroupUpdate::PlayQueue(queue(QueueReason::NextItem, 20.0, 1)),
            T0,
        );
        assert!(matches!(&o[..], [Out::Queue { switch: true, .. }]));
        assert_eq!(e.playlist_item_id(), Some("p2"));
        let o = e.group_update(
            GroupUpdate::PlayQueue(queue(QueueReason::MoveItem, 30.0, 1)),
            T0,
        );
        assert!(
            matches!(&o[..], [Out::Queue { switch: false, .. }]),
            "same item, just reordered"
        );
    }

    #[test]
    fn join_leave_and_members() {
        let mut e = engine();
        assert!(!e.in_group());
        let o = e.group_update(GroupUpdate::GroupJoined(group()), T0);
        assert!(matches!(&o[..], [Out::Joined(_)]));
        assert!(
            e.group_update(GroupUpdate::GroupJoined(group()), T0)
                .is_empty(),
            "same group twice"
        );
        e.group_update(GroupUpdate::UserJoined("bob".into()), T0);
        assert_eq!(e.group().unwrap().participants, ["a", "bob"]);
        e.group_update(GroupUpdate::UserLeft("a".into()), T0);
        assert_eq!(e.group().unwrap().participants, ["bob"]);
        e.group_update(
            GroupUpdate::StateUpdate {
                state: GroupState::Playing,
                reason: "Unpause".into(),
            },
            T0,
        );
        assert_eq!(e.group().unwrap().state, GroupState::Playing);
        assert_eq!(e.group_update(GroupUpdate::GroupLeft, T0), vec![Out::Left]);
        assert!(!e.in_group() && e.queue().is_none());
        assert!(e.time.ready(), "clock samples survive a leave");
        assert!(e.group_update(GroupUpdate::NotInGroup, T0).is_empty());
    }

    #[test]
    fn late_joiner_start_position() {
        let mut e = joined();
        // Group is paused at 42 s: start exactly there.
        e.last_cmd = Some(cmd(CommandKind::Pause, T0 - 5000.0, 420_000_000));
        e.last_cmd.as_mut().unwrap().emitted_at_ms = 20.0;
        assert_eq!(e.start_ticks(T0), 420_000_000);
        // Group has been playing since 30 s ago from 10 s: 40 s now.
        e.last_cmd = Some(cmd(CommandKind::Unpause, T0 - 30_000.0, 100_000_000));
        e.last_cmd.as_mut().unwrap().emitted_at_ms = 20.0;
        assert_eq!(e.start_ticks(T0), 400_000_000);
        // No command yet: queue position, advanced when the queue says it is playing.
        e.last_cmd = None;
        e.queue.as_mut().unwrap().start_position_ticks = 50_000_000;
        assert_eq!(e.start_ticks(T0), 50_000_000);
        e.queue.as_mut().unwrap().is_playing = true;
        e.queue.as_mut().unwrap().last_update_ms = T0 + OFF - 2000.0;
        assert_eq!(e.start_ticks(T0), 70_000_000);
    }

    #[test]
    fn drift_decisions_match_the_web_client() {
        let t = Tuning::default();
        assert_eq!(drift_decision(30.0, &t), None);
        assert_eq!(drift_decision(-59.0, &t), None);
        // 200 ms behind: speed up by 20% for 1 s.
        match drift_decision(200.0, &t) {
            Some(Drift::Speed { factor, for_ms }) => {
                assert!((factor - 1.2).abs() < 1e-9 && for_ms == 1000.0);
            }
            d => panic!("{d:?}"),
        }
        // 100 ms ahead: slow down 10%.
        assert!(
            matches!(drift_decision(-100.0, &t), Some(Drift::Speed { factor, .. }) if (factor - 0.9).abs() < 1e-9)
        );
        // 500 ms ahead (> 20% of the nudge): nudge lengthens so speed stays at 0.2.
        match drift_decision(-500.0, &t) {
            Some(Drift::Speed { factor, for_ms }) => {
                assert!((factor - 0.2).abs() < 1e-9 && (for_ms - 625.0).abs() < 1e-9)
            }
            d => panic!("{d:?}"),
        }
        assert!(matches!(
            drift_decision(3000.0, &t),
            Some(Drift::Skip { .. })
        ));
        assert!(matches!(
            drift_decision(-9000.0, &t),
            Some(Drift::Skip { .. })
        ));
        let strict = Tuning {
            min_skip_ms: 4000.0,
            ..t
        };
        assert_eq!(
            drift_decision(3500.0, &strict),
            None,
            "between the two methods nothing fires"
        );
    }

    #[test]
    fn drift_waits_for_settling_then_nudges_then_skips() {
        let mut e = joined();
        let c = cmd(CommandKind::Unpause, T0, 0);
        e.command(c, T0, Some(PAUSED_AT_0));
        e.unpaused(T0);
        // The group is at 3 s * ... we are 200 ms behind at T0+1000 (group 1000 ms in, we 800 ms).
        let behind = PlayerSnap {
            pos_ticks: 800 * 10_000,
            playing: true,
        };
        assert_eq!(
            e.drift(T0 + 1000.0, behind, false),
            None,
            "settling time not over"
        );
        assert!(
            (e.diff_ms - 200.0).abs() < 1e-6,
            "but the drift is measured"
        );
        let d = e.drift(
            T0 + 1600.0,
            PlayerSnap {
                pos_ticks: 1400 * 10_000,
                playing: true,
            },
            false,
        );
        assert!(matches!(d, Some(Drift::Speed { factor, .. }) if (factor - 1.2).abs() < 1e-6));
        assert!(!e.speed_reset_due(T0 + 2000.0));
        assert!(e.speed_reset_due(T0 + 2700.0));
        assert!(!e.speed_reset_due(T0 + 2800.0), "only once");
        let d = e.drift(
            T0 + 6000.0,
            PlayerSnap {
                pos_ticks: 2000 * 10_000,
                playing: true,
            },
            false,
        );
        assert_eq!(
            d,
            Some(Drift::Skip {
                to_ticks: 6000 * 10_000
            })
        );
        assert_eq!(
            e.drift(
                T0 + 9000.0,
                PlayerSnap {
                    pos_ticks: 0,
                    playing: true
                },
                true
            ),
            None,
            "buffering"
        );
    }

    #[test]
    fn no_drift_correction_while_paused_or_when_disabled() {
        let mut e = joined();
        e.command(cmd(CommandKind::Unpause, T0, 0), T0, Some(PAUSED_AT_0));
        e.unpaused(T0);
        let late = PlayerSnap {
            pos_ticks: 0,
            playing: false,
        };
        assert_eq!(e.drift(T0 + 9000.0, late, false), None);
        e.tuning.enabled = false;
        assert_eq!(
            e.drift(
                T0 + 9000.0,
                PlayerSnap {
                    pos_ticks: 0,
                    playing: true
                },
                false
            ),
            None
        );
        let mut e = joined();
        e.command(cmd(CommandKind::Pause, T0, 0), T0, Some(PAUSED_AT_0));
        assert_eq!(
            e.drift_ms(
                T0 + 1000.0,
                PlayerSnap {
                    pos_ticks: 0,
                    playing: true
                }
            ),
            None,
            "group is paused"
        );
    }

    #[test]
    fn ready_report_carries_server_time_and_item() {
        let e = joined();
        let r = e
            .report(
                T0,
                PlayerSnap {
                    pos_ticks: 123,
                    playing: false,
                },
            )
            .unwrap();
        assert_eq!(
            r,
            Report {
                when_ms: T0 + OFF,
                pos_ticks: 123,
                playing: false,
                playlist_item_id: "p1".into()
            }
        );
        assert!(
            engine().report(T0, PAUSED_AT_0).is_none(),
            "no queue, nothing to report"
        );
    }
}
