//! Watch together / SyncPlay (step 4e): the screen that lists, creates, joins and leaves groups,
//! the pill shown while in one, the socket drain and the glue between the group's state machine
//! (`nocturne-jellyfin-user`) and the player page. Port of the GPUI `ui_syncplay.rs`.
//!
//! SyncPlay acts on ONE server: the source of the item being played when a group starts or is
//! joined (`use_source`), otherwise the one picked at the top of the screen (default: the first
//! signed-in Jellyfin source). While in a group the server is locked.
//!
//! ## Root hook (for the master)
//!
//! ```ignore
//! // construct once, keep for the session; refresh when sources change / sign in or out:
//! sp: syncplay::SyncPlay::new(targets, dev_token_mode)  // Target per usable signed-in Jellyfin source
//! sp.set_targets(targets)                               // (while in a group the old one stays until it ends)
//! // every update/key below gets the player (None when no player page is up):
//! Message::SyncPlay(m)  => { let t = self.sp.update(m, self.player_mut()); self.drain_sp(); t.map(Message::SyncPlay) }
//! subscription          => self.sp.subscription().map(Message::SyncPlay)     // 50 ms drain, only while a socket exists
//! view                  => self.sp.view(win).map(Message::SyncPlay)           // overlay above the page, when sp.is_open()
//! pill (header, player) => self.sp.pill().map(|e| e.map(Message::SyncPlay))  // Some while in a group
//! keys                  => if sp.is_open() { sp.key(&k, player) }            // the screen owns the keyboard
//!                          `G` anywhere (when sp.can_open()) => Message::SyncPlay(Msg::OpenScreen)
//! account menu          => AcctAct::Together => Msg::OpenScreen
//! settings change       => sp.set_writes(on, player)   // switching off leaves the group
//! item about to play    => if sp.in_group() { sp.play(media, rest) /* group queue for everyone */ } else { normal play }
//!                          and before opening the screen from a playing item: sp.use_source(origin)
//! player closed         => sp.player_closed();  sign-out / switch account / quit => sp.shutdown(player)
//! // drain sp.outcomes after every update/key/call:
//! Outcome::Toast(s)                          -> toast
//! Outcome::OpenSettings                      -> open Settings
//! Outcome::OpenPlayer{origin,list,idx,start} -> PlayerCtx for list[idx] through the backend of `origin`
//!                                               (queue = list), Player::open_group(ctx, start); then
//!                                               player.queue = list; player.qi = idx
//! Outcome::Player(task)                      -> task.map(Message::Player)   (group_load / close results)
//! Outcome::ClosePlayer                       -> player.close(false)  (the group said Stop)
//! ```
//! Every group change goes through `write_client` (explicit user action, needs the master switch),
//! `group_client` (traffic of a group we are already in) or `leave` (always allowed); the three
//! share the one `user_confirmed` helper (a test asserts it is the only place the token is made).
//!
//! Debug preview: `NOCTURNE_PREVIEW=syncplay` (see `preview.rs`).
#[cfg(debug_assertions)]
pub mod preview;
mod view;

use crate::keys::Key;
use crate::player::{self, Player};
use crate::task::{after, run};
use iced::widget::{self, operation};
use iced::{Element, Size, Subscription, Task};

/// Id of the "name a new group" input.
pub(crate) const NAME_ID: &str = "sp-name";
use nocturne_core::bridge;
use nocturne_core::source::{Media, Source, SourceId};
use nocturne_core::sync::NEED_WRITES;
use nocturne_core::syncplay::{self as spx, Gate, GroupReq, Stall, StallEvent};
use nocturne_jellyfin_user::syncplay::ws::{self, SocketEvent};
use nocturne_jellyfin_user::syncplay::{
    Drift, Engine, GroupInfo, GroupUpdate, Incoming, NewQueue, Op, Out, PlayQueue, PlayerSnap,
    QueueReason, Sample, SyncAccess, TICKS_PER_MS, Tuning, now_ms,
};
use nocturne_jellyfin_user::{UserClient, UserConfirmed};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// A Jellyfin source the account is signed in to.
#[derive(Clone)]
pub struct Target {
    pub origin: SourceId,
    pub label: String,
    pub client: UserClient,
    pub user_name: String,
    /// Reads the group's items (GET only).
    pub source: Source,
}

pub enum Outcome {
    Toast(String),
    OpenSettings,
    OpenPlayer {
        origin: SourceId,
        list: Vec<Media>,
        idx: usize,
        start: f64,
    },
    Player(Task<player::Msg>),
    ClosePlayer,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Link {
    Off,
    Connecting,
    Up,
    /// The socket dropped at this time; it is being re-established.
    Down(Instant),
}

#[derive(Clone)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed,
}

#[derive(Clone)]
pub enum Msg {
    /// Every 50 ms while a socket exists: drain it, drive the clock, the player and the outbox.
    Tick,
    OpenScreen,
    CloseScreen,
    OpenSettings,
    Refresh,
    Access(u64, Option<SyncAccess>),
    Groups(u64, Option<Vec<GroupInfo>>),
    Time(u64, Option<Sample>),
    /// A scheduled group command is due.
    Run(u64, Op),
    Resume(u64, Option<GroupInfo>),
    Rejoined(u64, bool),
    Items {
        conn: u64,
        stamp: f64,
        ids: Vec<String>,
        idx: i32,
        switch: bool,
        reason: QueueReason,
        got: Option<Vec<Media>>,
    },
    Created(u64, Result<Option<GroupInfo>, ()>),
    Joined(u64, bool),
    Reached(bool),
    Started(bool),
    Select(usize),
    Join(String),
    Create,
    Leave,
    Follow,
    NameInput(String),
    Choose(usize),
}

pub struct SyncPlay {
    targets: Vec<Target>,
    chosen: usize,
    dev: bool,
    writes: bool,
    engine: Engine,
    open: bool,
    sel: usize,
    name_focus: bool,
    name: String,
    access: Option<SyncAccess>,
    groups: Load<Vec<GroupInfo>>,
    link: Link,
    /// Create / join request in flight.
    busy: bool,
    /// Follow the group's playback (false after closing the video while staying in the group).
    following: bool,
    rx: Option<Receiver<SocketEvent>>,
    task: Option<tokio::task::JoinHandle<()>>,
    /// Bumped to cancel scheduled commands of an old connection.
    epoch: u64,
    /// Bumped on every connect, disconnect and server change: answers for an old one are dropped.
    conn: u64,
    stall: Stall,
    clock: Instant,
    last_player_poll: Instant,
    sync_busy: bool,
    sync_n: u32,
    next_sync: Instant,
    rejoining: bool,
    /// A player page is up (seen at the last `update` / `key`).
    in_player: bool,
    pub outcomes: Vec<Outcome>,
}

fn secs(ticks: i64) -> f64 {
    ticks as f64 / (TICKS_PER_MS * 1000.0)
}

fn to_ticks(secs: f64) -> i64 {
    (secs.max(0.0) * TICKS_PER_MS * 1000.0) as i64
}

/// The only place a `UserConfirmed` is made in this module (see the module docs).
fn user_confirmed() -> UserConfirmed {
    UserConfirmed::i_have_asked_the_user()
}

/// What a key does on the screen (pure, so it tests without a window).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyAct {
    Close,
    SwitchFocus,
    Refresh,
    Leave,
    Follow,
    Create,
    JoinSelected,
    Up,
    Down,
    Server(bool),
    None,
}

pub fn key_act(k: &Key, in_group: bool, name_focus: bool) -> KeyAct {
    match k.name.as_str() {
        "escape" => KeyAct::Close,
        "tab" if !in_group => KeyAct::SwitchFocus,
        "r" if !name_focus || in_group => KeyAct::Refresh,
        "l" if in_group => KeyAct::Leave,
        "enter" if in_group => KeyAct::Follow,
        "enter" if name_focus => KeyAct::Create,
        "enter" => KeyAct::JoinSelected,
        "up" if !in_group => KeyAct::Up,
        "down" if !in_group => KeyAct::Down,
        "[" if !in_group && !name_focus => KeyAct::Server(false),
        "]" if !in_group && !name_focus => KeyAct::Server(true),
        _ => KeyAct::None,
    }
}

impl SyncPlay {
    pub fn new(targets: Vec<Target>, dev: bool) -> Self {
        let now = Instant::now();
        Self {
            targets,
            chosen: 0,
            dev,
            writes: false,
            engine: Engine::new(Tuning::default()),
            open: false,
            sel: 0,
            name_focus: false,
            name: String::new(),
            access: None,
            groups: Load::Loading,
            link: Link::Off,
            busy: false,
            following: true,
            rx: None,
            task: None,
            epoch: 0,
            conn: 0,
            stall: Stall::default(),
            clock: now,
            last_player_poll: now,
            sync_busy: false,
            sync_n: 0,
            next_sync: now,
            rejoining: false,
            in_player: false,
            outcomes: vec![],
        }
    }

    // ---- state the root asks about ----

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn in_group(&self) -> bool {
        self.engine.in_group()
    }

    pub fn following(&self) -> bool {
        self.following
    }

    /// There is a server to take part on (a signed-in Jellyfin source) or a reason to explain.
    pub fn can_open(&self) -> bool {
        self.dev || !self.targets.is_empty()
    }

    pub fn origin(&self) -> Option<SourceId> {
        self.targets.get(self.chosen).map(|t| t.origin)
    }

    pub fn gate(&self) -> Gate {
        spx::gate(
            self.dev || self.targets.is_empty(),
            self.writes,
            self.access,
        )
    }

    pub fn set_targets(&mut self, targets: Vec<Target>) {
        if self.in_group() {
            return; // the group's server stays until it ends
        }
        let keep = self.origin();
        self.targets = targets;
        self.chosen = keep
            .and_then(|o| self.targets.iter().position(|t| t.origin == o))
            .unwrap_or(0);
    }

    /// Act on the server of the item being played (before opening the screen or starting a group).
    /// Ignored while in a group (its server is fixed).
    pub fn use_source(&mut self, origin: SourceId) {
        if self.in_group() {
            return;
        }
        if let Some(i) = self.targets.iter().position(|t| t.origin == origin)
            && i != self.chosen
        {
            self.pick(i);
        }
    }

    fn api(&self) -> Option<UserClient> {
        self.targets.get(self.chosen).map(|t| t.client.clone())
    }

    fn pick(&mut self, i: usize) {
        self.disconnect();
        self.chosen = i;
        self.access = None;
        self.groups = Load::Loading;
        self.sel = 0;
        self.engine.reset();
    }

    // ---- access, writes ----

    /// The one place a user-initiated group change is allowed: needs a real login and the master
    /// switch. Says why and returns `None` otherwise.
    fn write_client(&mut self) -> Option<(UserClient, UserConfirmed)> {
        if self.dev || !self.writes {
            self.toast(NEED_WRITES);
            return None;
        }
        Some((self.api()?, user_confirmed()))
    }

    /// Traffic of a group we are already in (Ready, Buffering, Ping, transport requests). The
    /// switch being turned off leaves the group (see `set_writes`).
    fn group_client(&self) -> Option<(UserClient, UserConfirmed)> {
        if self.dev || !self.writes || !self.engine.in_group() {
            return None;
        }
        Some((self.api()?, user_confirmed()))
    }

    fn toast(&mut self, s: impl Into<String>) {
        self.outcomes.push(Outcome::Toast(s.into()));
    }

    /// Settings changed: no group without the master switch.
    pub fn set_writes(&mut self, on: bool, p: Option<&mut Player>) -> Task<Msg> {
        self.writes = on;
        if !on && self.engine.in_group() {
            return self.leave(p);
        }
        Task::none()
    }

    // ---- screen ----

    fn open_screen(&mut self) -> Task<Msg> {
        self.open = true;
        self.sel = 0;
        self.name_focus = false;
        self.groups = Load::Loading;
        if self.dev || self.targets.is_empty() {
            return Task::none();
        }
        self.connect();
        self.refresh()
    }

    fn close_screen(&mut self) {
        self.open = false;
        if !self.engine.in_group() {
            self.disconnect();
        }
    }

    fn refresh(&mut self) -> Task<Msg> {
        let Some(api) = self.api() else {
            return Task::none();
        };
        let (a2, c) = (api.clone(), self.conn);
        Task::batch([
            run(async move { a2.syncplay_access().await.ok() }, move |r| {
                Msg::Access(c, r)
            }),
            run(async move { api.syncplay_list().await.ok() }, move |r| {
                Msg::Groups(c, r)
            }),
        ])
    }

    // ---- socket ----

    fn connect(&mut self) {
        if self.rx.is_some() {
            return;
        }
        let Some(api) = self.api() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        self.task = Some(bridge::spawn(ws::run(api, tx)));
        self.rx = Some(rx);
        self.link = Link::Connecting;
        self.sync_n = 0;
        self.next_sync = Instant::now();
        self.epoch += 1;
        self.conn += 1;
    }

    fn disconnect(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
        self.rx = None;
        self.link = Link::Off;
        self.epoch += 1;
        self.conn += 1;
        self.sync_busy = false;
        self.engine.time.reset();
    }

    /// 50 ms drain while a socket exists (an Iced subscription; nothing runs without one).
    pub fn subscription(&self) -> Subscription<Msg> {
        if self.rx.is_some() {
            iced::time::every(Duration::from_millis(50)).map(|_| Msg::Tick)
        } else {
            Subscription::none()
        }
    }

    fn poll(&mut self, mut p: Option<&mut Player>) -> Task<Msg> {
        let Some(rx) = &self.rx else {
            return Task::none();
        };
        let evs: Vec<SocketEvent> = rx.try_iter().collect();
        let mut tasks = vec![];
        for e in evs {
            tasks.push(self.event(e, p.as_deref_mut()));
        }
        tasks.push(self.time());
        self.give_up(p.as_deref_mut());
        if self.engine.in_group() {
            tasks.push(self.drain_outbox(p.as_deref_mut()));
            if self.last_player_poll.elapsed() >= Duration::from_millis(100) {
                self.last_player_poll = Instant::now();
                tasks.push(self.poll_player(p));
            }
        }
        Task::batch(tasks)
    }

    fn event(&mut self, e: SocketEvent, p: Option<&mut Player>) -> Task<Msg> {
        match e {
            SocketEvent::Connected => {
                let was_down = matches!(self.link, Link::Down(_));
                self.link = Link::Up;
                self.next_sync = Instant::now();
                if was_down && self.engine.in_group() {
                    return self.resume_group();
                }
                Task::none()
            }
            SocketEvent::Disconnected => {
                self.link = Link::Down(Instant::now());
                Task::none()
            }
            SocketEvent::Message(Incoming::Command(c)) => {
                let snap = snap(p.as_deref());
                let outs = self.engine.command(c, now_ms(), snap);
                self.outs(outs, p)
            }
            SocketEvent::Message(Incoming::Group(u)) => {
                let mut t = vec![];
                if matches!(u, GroupUpdate::GroupDoesNotExist) {
                    self.busy = false;
                    t.push(self.refresh());
                }
                let outs = self.engine.group_update(u, now_ms());
                t.push(self.outs(outs, p));
                Task::batch(t)
            }
            SocketEvent::Message(_) => Task::none(),
        }
    }

    /// Clock samples: four quick ones after connecting, then every 30 s; each is followed by a
    /// `Ping` to the group so the server leads its commands by our real latency.
    fn time(&mut self) -> Task<Msg> {
        if self.link != Link::Up || self.sync_busy || Instant::now() < self.next_sync {
            return Task::none();
        }
        let Some(api) = self.api() else {
            return Task::none();
        };
        self.sync_busy = true;
        let c = self.conn;
        run(
            async move { api.syncplay_time_sample().await.ok() },
            move |r| Msg::Time(c, r),
        )
    }

    fn on_time(&mut self, r: Option<Sample>, p: Option<&mut Player>) -> Task<Msg> {
        self.sync_busy = false;
        let Some(s) = r else {
            self.next_sync = Instant::now() + Duration::from_secs(2);
            return Task::none();
        };
        let first = !self.engine.time.ready();
        self.engine.time.add(s);
        self.next_sync = Instant::now() + spx::sample_gap(self.sync_n);
        self.sync_n += 1;
        let mut tasks = vec![];
        if first {
            let snap = snap(p.as_deref());
            let outs = self.engine.clock_ready(now_ms(), snap);
            tasks.push(self.outs(outs, p));
        }
        if let Some((api, c)) = self.group_client() {
            let ping = self.engine.time.ping();
            bridge::spawn(async move {
                let _ = api.syncplay_ping(ping, c).await;
            });
        }
        Task::batch(tasks)
    }

    /// The socket has been down too long: the server has dropped us, so stop pretending.
    fn give_up(&mut self, p: Option<&mut Player>) {
        if let Link::Down(since) = self.link
            && self.engine.in_group()
            && since.elapsed() > spx::GIVE_UP_AFTER
        {
            self.local_reset(p);
            self.toast("Lost the connection to the group. You are no longer in it.");
        }
    }

    /// The socket is back while we think we are in a group: is it still there, and are we in it?
    fn resume_group(&mut self) -> Task<Msg> {
        let (Some(api), Some(g)) = (self.api(), self.engine.group().cloned()) else {
            return Task::none();
        };
        let c = self.conn;
        run(
            async move { api.syncplay_group(&g.group_id).await.ok() },
            move |r| Msg::Resume(c, r),
        )
    }

    fn on_resume(&mut self, r: Option<GroupInfo>, p: Option<&mut Player>) -> Task<Msg> {
        let me = self.targets.get(self.chosen).map(|t| t.user_name.clone());
        let Some(g) = self.engine.group().cloned() else {
            return Task::none();
        };
        match r {
            Some(info) if me.is_some_and(|m| info.participants.contains(&m)) => {
                // Still a member but we missed messages: leave and join again, which makes the
                // server send the queue and position afresh.
                self.rejoining = true;
                self.rejoin(g.group_id)
            }
            _ => {
                self.local_reset(p);
                self.toast("The group is gone. You are no longer in it.");
                Task::none()
            }
        }
    }

    fn rejoin(&mut self, id: String) -> Task<Msg> {
        let Some((api, c)) = self.group_client() else {
            return Task::none();
        };
        let conn = self.conn;
        run(
            async move {
                let _ = api.syncplay_leave(c).await;
                api.syncplay_join(&id, c).await.is_ok()
            },
            move |ok| Msg::Rejoined(conn, ok),
        )
    }

    // ---- player side ----

    fn outs(&mut self, outs: Vec<Out>, mut p: Option<&mut Player>) -> Task<Msg> {
        let mut tasks = vec![];
        for o in outs {
            match o {
                Out::Joined(g) => {
                    self.busy = false;
                    self.following = true;
                    self.toast(format!("Joined {}", g.group_name));
                    tasks.push(self.refresh());
                }
                Out::Left => {
                    self.local_reset(p.as_deref_mut());
                    self.toast("The group ended.");
                }
                Out::Notice(s) => self.toast(s),
                Out::Apply {
                    pre_seek,
                    in_ms,
                    op,
                } => tasks.push(self.schedule(pre_seek, in_ms, op, p.as_deref_mut())),
                Out::Queue { queue, switch } => {
                    tasks.push(self.queue(queue, switch, p.as_deref_mut()))
                }
            }
        }
        Task::batch(tasks)
    }

    fn schedule(
        &mut self,
        pre_seek: Option<i64>,
        in_ms: f64,
        op: Op,
        mut p: Option<&mut Player>,
    ) -> Task<Msg> {
        self.epoch += 1;
        if let Some(p) = p.as_deref_mut() {
            p.group_speed(1.0);
            if let Some(t) = pre_seek {
                p.group_seek(secs(t));
            }
        }
        if in_ms <= 2.0 {
            self.run_op(op, true, p);
            return Task::none();
        }
        after(
            Duration::from_micros((in_ms * 1000.0) as u64),
            Msg::Run(self.epoch, op),
        )
    }

    /// `late`: the moment has already passed, so also jump to where the group is.
    fn run_op(&mut self, op: Op, late: bool, p: Option<&mut Player>) {
        let Some(p) = p.filter(|p| p.group) else {
            return;
        };
        match op {
            Op::Unpause { pos_ticks } => {
                let far = (p.pos() - secs(pos_ticks)).abs() > 0.4;
                if late && far {
                    p.group_seek(secs(pos_ticks));
                }
                p.group_unpause();
                self.engine.unpaused(now_ms());
                // Answer promptly even when already ready: the server counts every member.
                self.ready(false, Some(p));
            }
            Op::Pause { pos_ticks } => p.group_pause(secs(pos_ticks)),
            Op::Seek { pos_ticks } => {
                p.group_pause(secs(pos_ticks));
                p.ready_due = Some(secs(pos_ticks));
            }
            Op::Stop => self.outcomes.push(Outcome::ClosePlayer),
        }
    }

    /// `Ready` (or `Buffering`) with our position and the item the group is on.
    fn ready(&mut self, buffering: bool, p: Option<&Player>) {
        let Some(snap) = snap(p) else { return };
        let (Some(rep), Some((api, c))) = (self.engine.report(now_ms(), snap), self.group_client())
        else {
            return;
        };
        bridge::spawn(async move {
            let _ = api.syncplay_report(buffering, &rep, c).await;
        });
    }

    /// Ready after a load or seek once settled, Buffering / Ready around stalls, drift correction.
    fn poll_player(&mut self, p: Option<&mut Player>) -> Task<Msg> {
        let Some(p) = p else {
            return Task::none();
        };
        let (group, loaded, due, gs, (seeking, cache)) = (
            p.group,
            p.loaded,
            p.ready_due,
            p.group_snap(),
            p.group_flags(),
        );
        let Some((pos, playing)) = gs.filter(|_| group && loaded) else {
            return Task::none();
        };
        if let Some(t) = due
            && !seeking
            && !cache
            && spx::settled(t, pos)
        {
            p.ready_due = None;
            self.ready(false, Some(p));
        }
        match self.stall.update(self.clock.elapsed(), cache && playing) {
            Some(StallEvent::Began) => self.ready(true, Some(p)),
            Some(StallEvent::Ended) => self.ready(false, Some(p)),
            None => {}
        }
        let (now, s) = (
            now_ms(),
            PlayerSnap {
                pos_ticks: to_ticks(pos),
                playing,
            },
        );
        if self.engine.speed_reset_due(now) {
            p.group_speed(1.0);
        }
        let act = self.engine.drift(now, s, self.stall.reported());
        p.group_diff = self.engine.drift_ms(now, s);
        match act {
            Some(Drift::Speed { factor, .. }) => p.group_speed(factor),
            Some(Drift::Skip { to_ticks }) => p.group_seek(secs(to_ticks)),
            None => {}
        }
        Task::none()
    }

    fn drain_outbox(&mut self, p: Option<&mut Player>) -> Task<Msg> {
        let Some(p) = p else { return Task::none() };
        let reqs = p.take_outbox();
        Task::batch(reqs.into_iter().map(|r| self.req(r)).collect::<Vec<_>>())
    }

    /// A transport request from the local player goes to the group.
    fn req(&mut self, r: GroupReq) -> Task<Msg> {
        let Some((api, c)) = self.group_client() else {
            self.toast(NEED_WRITES);
            return Task::none();
        };
        let pli = self.engine.playlist_item_id().map(str::to_owned);
        let entry = |i: usize| {
            self.engine
                .queue()
                .and_then(|q| q.playlist.get(i))
                .map(|x| x.playlist_item_id.clone())
        };
        let item = match r {
            GroupReq::SetIndex(i) => entry(i),
            _ => pli,
        };
        run(
            async move {
                match (r, item) {
                    (GroupReq::Pause, _) => api.syncplay_pause(c).await,
                    (GroupReq::Unpause, _) => api.syncplay_unpause(c).await,
                    (GroupReq::Stop, _) => api.syncplay_stop(c).await,
                    (GroupReq::Seek(s), _) => api.syncplay_seek(to_ticks(s), c).await,
                    (GroupReq::Next, Some(i)) => api.syncplay_next(&i, c).await,
                    (GroupReq::Prev, Some(i)) => api.syncplay_previous(&i, c).await,
                    (GroupReq::SetIndex(_), Some(i)) => api.syncplay_set_item(&i, c).await,
                    _ => Ok(()),
                }
                .is_ok()
            },
            Msg::Reached,
        )
    }

    // ---- queue ----

    fn queue(&mut self, q: PlayQueue, switch: bool, _p: Option<&mut Player>) -> Task<Msg> {
        let Some(t) = self.targets.get(self.chosen) else {
            return Task::none();
        };
        let ids: Vec<String> = q.playlist.iter().map(|i| i.item_id.clone()).collect();
        if ids.is_empty() {
            return Task::none();
        }
        let (src, origin) = (t.source.clone(), t.origin);
        let (client, user) = src.client();
        let mut uniq = ids.clone();
        uniq.sort();
        uniq.dedup();
        let (stamp, idx, reason, conn) =
            (q.last_update_ms, q.playing_item_index, q.reason, self.conn);
        run(
            async move {
                let query = nocturne_jellyfin::ItemsQuery {
                    ids: uniq,
                    ..Default::default()
                };
                client.items(&user, &query).await.ok().map(|r| {
                    r.items
                        .iter()
                        .map(|i| Media::from_item_in(origin, i))
                        .collect::<Vec<Media>>()
                })
            },
            move |got| Msg::Items {
                conn,
                stamp,
                ids,
                idx,
                switch,
                reason,
                got,
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn on_items(
        &mut self,
        stamp: f64,
        ids: Vec<String>,
        idx: i32,
        switch: bool,
        reason: QueueReason,
        got: Option<Vec<Media>>,
        p: Option<&mut Player>,
    ) -> Task<Msg> {
        if self.engine.queue().map(|q| q.last_update_ms) != Some(stamp) {
            return Task::none(); // a newer queue arrived meanwhile
        }
        match got.and_then(|g| spx::order_items(&ids, g).ok()) {
            Some(list) => self.apply_queue(list, idx, switch, reason, p),
            None => {
                self.unavailable();
                Task::none()
            }
        }
    }

    fn apply_queue(
        &mut self,
        list: Vec<Media>,
        idx: i32,
        switch: bool,
        reason: QueueReason,
        p: Option<&mut Player>,
    ) -> Task<Msg> {
        let Ok(idx) = usize::try_from(idx) else {
            return Task::none();
        };
        if !switch {
            if let Some(p) = p {
                p.group_queue(list, idx);
            }
            return Task::none();
        }
        if !self.following {
            if reason != QueueReason::NewPlaylist {
                return Task::none();
            }
            self.following = true;
            self.ignore_wait(false);
        }
        let start = secs(self.engine.start_ticks(now_ms()));
        self.stall.reset();
        match p {
            Some(p) => self
                .outcomes
                .push(Outcome::Player(p.group_load(list, idx, start))),
            None => {
                if let Some(origin) = self.origin() {
                    self.outcomes.push(Outcome::OpenPlayer {
                        origin,
                        list,
                        idx,
                        start,
                    });
                }
            }
        }
        Task::none()
    }

    /// Something in the group's queue is not available to this user: stay in the group, but do
    /// not hold the others back.
    fn unavailable(&mut self) {
        self.following = false;
        self.ignore_wait(true);
        self.toast(
            "Something in the group's queue is not available to you. You stay in the group without holding it back.",
        );
    }

    fn ignore_wait(&self, ignore: bool) {
        if let Some((api, c)) = self.group_client() {
            bridge::spawn(async move {
                let _ = api.syncplay_ignore_wait(ignore, c).await;
            });
        }
    }

    /// The video was closed locally while in a group: stop holding the group back.
    pub fn player_closed(&mut self) {
        if self.engine.in_group() && self.following {
            self.following = false;
            self.ignore_wait(true);
            self.toast("You left the video. The group plays on; open Watch together to rejoin.");
        }
    }

    /// Watch together > rejoin: load whatever the group is on.
    fn follow_again(&mut self, p: Option<&mut Player>) -> Task<Msg> {
        let Some(q) = self.engine.queue().cloned() else {
            self.toast("Nothing is playing in the group yet.");
            return Task::none();
        };
        self.following = true;
        self.ignore_wait(false);
        self.close_screen();
        self.queue(q, true, p)
    }

    // ---- create, join, leave, play ----

    fn create(&mut self) -> Task<Msg> {
        if !self.gate().can_create() {
            self.toast(self.gate().reason().unwrap_or("Not allowed"));
            return Task::none();
        }
        let name = self.name.trim().to_string();
        if name.is_empty() {
            self.toast("Give the group a name first.");
            return Task::none();
        }
        let Some((api, c)) = self.write_client() else {
            return Task::none();
        };
        self.busy = true;
        let conn = self.conn;
        run(
            async move { api.syncplay_new(&name, c).await.map_err(|_| ()) },
            move |r| Msg::Created(conn, r),
        )
    }

    fn join(&mut self, id: String) -> Task<Msg> {
        if !self.gate().can_join() {
            self.toast(self.gate().reason().unwrap_or("Not allowed"));
            return Task::none();
        }
        let Some((api, c)) = self.write_client() else {
            return Task::none();
        };
        self.busy = true;
        let conn = self.conn;
        run(
            async move { api.syncplay_join(&id, c).await.is_ok() },
            move |ok| Msg::Joined(conn, ok),
        )
    }

    /// Always allowed, even with server changes off: nobody should be stuck in a group.
    pub fn leave(&mut self, p: Option<&mut Player>) -> Task<Msg> {
        if !self.engine.in_group() {
            return Task::none();
        }
        if let Some(api) = self.api() {
            let c = user_confirmed();
            bridge::spawn(async move {
                let _ = api.syncplay_leave(c).await;
            });
        }
        self.local_reset(p);
        self.toast("You left the group.");
        self.refresh()
    }

    /// Local control again: the player keeps what it has but follows nobody.
    fn local_reset(&mut self, p: Option<&mut Player>) {
        self.engine.reset();
        self.epoch += 1;
        self.busy = false;
        self.following = true;
        self.rejoining = false;
        self.stall.reset();
        if let Some(p) = p {
            p.group = false;
            p.outbox.clear();
            p.ready_due = None;
            p.group_diff = None;
            p.group_speed(1.0);
        }
        if !self.open {
            self.disconnect();
        }
    }

    /// Sign-out, switching account or server, quitting: leave quietly and drop the socket.
    pub fn shutdown(&mut self, p: Option<&mut Player>) {
        if self.engine.in_group()
            && let Some(api) = self.api()
        {
            let c = user_confirmed();
            bridge::spawn(async move {
                let _ = api.syncplay_leave(c).await;
            });
        }
        if let Some(p) = p {
            p.group = false;
            p.outbox.clear();
        }
        self.engine.reset();
        self.open = false;
        self.access = None;
        self.disconnect();
    }

    /// Choosing something to play while in a group sets the group's queue for everyone.
    pub fn play(&mut self, m: Media, rest: Vec<Media>) -> Task<Msg> {
        let Some((api, c)) = self.write_client() else {
            return Task::none();
        };
        let q = NewQueue {
            item_ids: std::iter::once(m.id.clone())
                .chain(rest.into_iter().map(|r| r.id))
                .take(200)
                .collect(),
            start_index: 0,
            start_ticks: 0,
        };
        self.toast("Starting it for the group");
        run(
            async move { api.syncplay_set_new_queue(&q, c).await.is_ok() },
            Msg::Started,
        )
    }

    // ---- messages and keys ----

    pub fn update(&mut self, msg: Msg, mut p: Option<&mut Player>) -> Task<Msg> {
        self.in_player = p.is_some();
        match msg {
            Msg::Tick => self.poll(p),
            Msg::OpenScreen => self.open_screen(),
            Msg::CloseScreen => {
                self.close_screen();
                Task::none()
            }
            Msg::OpenSettings => {
                self.close_screen();
                self.outcomes.push(Outcome::OpenSettings);
                Task::none()
            }
            Msg::Refresh => self.refresh(),
            Msg::Access(c, r) => {
                if c == self.conn && r.is_some() {
                    self.access = r;
                }
                Task::none()
            }
            Msg::Groups(c, r) => {
                if c == self.conn {
                    self.groups = r.map_or(Load::Failed, Load::Ready);
                    self.sel = 0;
                }
                Task::none()
            }
            Msg::Time(c, r) => {
                if c != self.conn {
                    return Task::none();
                }
                self.on_time(r, p)
            }
            Msg::Run(epoch, op) => {
                if epoch == self.epoch {
                    self.run_op(op, false, p);
                }
                Task::none()
            }
            Msg::Resume(c, r) => {
                if c != self.conn {
                    return Task::none();
                }
                self.on_resume(r, p)
            }
            Msg::Rejoined(c, ok) => {
                if c == self.conn {
                    self.rejoining = false;
                    if !ok {
                        self.local_reset(p);
                        self.toast("Could not rejoin the group.");
                    }
                }
                Task::none()
            }
            Msg::Items {
                conn,
                stamp,
                ids,
                idx,
                switch,
                reason,
                got,
            } => {
                if conn != self.conn {
                    return Task::none();
                }
                self.on_items(stamp, ids, idx, switch, reason, got, p)
            }
            Msg::Created(c, r) => {
                if c != self.conn {
                    return Task::none();
                }
                match r {
                    Ok(Some(g)) => {
                        self.name.clear();
                        let outs = self
                            .engine
                            .group_update(GroupUpdate::GroupJoined(g), now_ms());
                        self.outs(outs, p.as_deref_mut())
                    }
                    Ok(None) => {
                        self.name.clear(); // the socket tells us
                        Task::none()
                    }
                    Err(()) => {
                        self.busy = false;
                        self.toast("The group could not be created.");
                        Task::none()
                    }
                }
            }
            Msg::Joined(c, ok) => {
                if c == self.conn && !ok {
                    self.busy = false;
                    self.toast("Could not join. The group may be gone.");
                    return self.refresh();
                }
                Task::none()
            }
            Msg::Reached(ok) => {
                if !ok {
                    self.toast("The group could not be reached.");
                }
                Task::none()
            }
            Msg::Started(ok) => {
                if !ok {
                    self.toast("The group could not start that.");
                }
                Task::none()
            }
            Msg::Select(i) => {
                self.sel = i;
                self.name_focus = false;
                Task::none()
            }
            Msg::Join(id) => self.join(id),
            Msg::Create => self.create(),
            Msg::Leave => self.leave(p),
            Msg::Follow => self.follow_again(p),
            Msg::NameInput(s) => {
                self.name_focus = true;
                self.set_name(&s);
                Task::none()
            }
            Msg::Choose(i) => self.choose(i),
        }
    }

    fn choose(&mut self, i: usize) -> Task<Msg> {
        if self.in_group() || i >= self.targets.len() || i == self.chosen {
            return Task::none();
        }
        self.pick(i);
        if self.open {
            self.connect();
            return self.refresh();
        }
        Task::none()
    }

    /// The name field's text: no control characters, at most 100 bytes.
    fn set_name(&mut self, t: &str) {
        self.name.clear();
        for c in t.chars().filter(|c| !c.is_control()) {
            if self.name.len() + c.len_utf8() <= 100 {
                self.name.push(c);
            }
        }
    }

    fn groups_len(&self) -> usize {
        match &self.groups {
            Load::Ready(g) => g.len(),
            _ => 0,
        }
    }

    /// Keyboard while the screen is open (it owns every key).
    pub fn key(&mut self, k: &Key, p: Option<&mut Player>) -> Task<Msg> {
        self.in_player = p.is_some();
        let in_group = self.in_group();
        match key_act(k, in_group, self.name_focus) {
            KeyAct::Close => {
                self.close_screen();
                Task::none()
            }
            KeyAct::SwitchFocus => {
                self.name_focus = !self.name_focus;
                operation::focus(if self.name_focus {
                    widget::Id::from(NAME_ID)
                } else {
                    widget::Id::from("sp-none")
                })
            }
            KeyAct::Refresh => self.refresh(),
            KeyAct::Leave => self.leave(p),
            KeyAct::Follow => self.follow_again(p),
            KeyAct::Create => self.create(),
            KeyAct::JoinSelected => {
                let id = match &self.groups {
                    Load::Ready(g) => g.get(self.sel).map(|g| g.group_id.clone()),
                    _ => None,
                };
                id.map_or_else(Task::none, |id| self.join(id))
            }
            KeyAct::Up => {
                self.sel = self.sel.saturating_sub(1);
                Task::none()
            }
            KeyAct::Down => {
                self.sel = (self.sel + 1).min(self.groups_len().saturating_sub(1));
                Task::none()
            }
            KeyAct::Server(fwd) => {
                let n = self.targets.len();
                if n < 2 {
                    return Task::none();
                }
                let to = if fwd {
                    (self.chosen + 1) % n
                } else {
                    (self.chosen + n - 1) % n
                };
                self.choose(to)
            }
            KeyAct::None => Task::none(),
        }
    }

    pub fn view(&self, win: Size) -> Option<Element<'_, Msg>> {
        self.open.then(|| view::screen(self, win))
    }

    /// "Watching together" chip with Leave, shown while in a group (header and player).
    pub fn pill(&self) -> Option<Element<'_, Msg>> {
        view::pill(self)
    }
}

/// Position and play state of a player that follows a group.
fn snap(p: Option<&Player>) -> Option<PlayerSnap> {
    let p = p?;
    if !p.group {
        return None;
    }
    let (pos, playing) = p.group_snap()?;
    Some(PlayerSnap {
        pos_ticks: to_ticks(pos),
        playing,
    })
}

#[cfg(test)]
mod tests;
