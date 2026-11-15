//! Watch together (SyncPlay): the screen that lists, creates, joins and leaves groups, the pill
//! shown while in one, and the glue between the group's state machine (`nocturne-jellyfin-user`)
//! and the player page. Every server change goes through `sp_write` / `sp_group_client`, the two
//! places that check the master switch and mint `UserConfirmed`.
use super::*;
use crate::player_view::PlayerView;
use crate::syncplay::{self as spx, Gate, GroupReq, Stall, StallEvent};
use nocturne_jellyfin_user::UserConfirmed;
use nocturne_jellyfin_user::syncplay::ws::{self, SocketEvent};
use nocturne_jellyfin_user::syncplay::{
    Drift, Engine, GroupInfo, GroupUpdate, Incoming, NewQueue, Op, Out, PlayQueue, PlayerSnap,
    QueueReason, SyncAccess, TICKS_PER_MS, Tuning, now_ms,
};
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Link {
    Off,
    Connecting,
    Up,
    /// The socket dropped at this time; it is being re-established.
    Down(Instant),
}

pub(super) struct Sp {
    pub engine: Engine,
    pub open: bool,
    pub sel: usize,
    pub name_focus: bool,
    pub name: String,
    pub access: Option<SyncAccess>,
    pub groups: Load<Vec<GroupInfo>>,
    pub link: Link,
    /// Create / join request in flight.
    pub busy: bool,
    /// Follow the group's playback (false after closing the video while staying in the group).
    pub following: bool,
    api: Option<UserClient>,
    rx: Option<Receiver<SocketEvent>>,
    task: Option<tokio::task::JoinHandle<()>>,
    /// Bumped to cancel scheduled commands and stop the poll loop of an old connection.
    epoch: u64,
    looping: bool,
    stall: Stall,
    clock: Instant,
    last_player_poll: Instant,
    sync_busy: bool,
    sync_n: u32,
    next_sync: Instant,
    rejoining: bool,
}

impl Sp {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
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
            api: None,
            rx: None,
            task: None,
            epoch: 0,
            looping: false,
            stall: Stall::default(),
            clock: now,
            last_player_poll: now,
            sync_busy: false,
            sync_n: 0,
            next_sync: now,
            rejoining: false,
        }
    }
}

fn secs(ticks: i64) -> f64 {
    ticks as f64 / (TICKS_PER_MS * 1000.0)
}

fn to_ticks(secs: f64) -> i64 {
    (secs.max(0.0) * TICKS_PER_MS * 1000.0) as i64
}

impl Root {
    // ---- access, writes ----

    fn sp_api(&self) -> Option<UserClient> {
        self.sp.api.clone().or_else(|| {
            self.session
                .as_ref()
                .and_then(|(u, s)| UserClient::from_session(u, s).ok())
        })
    }

    pub(super) fn sp_gate(&self) -> Gate {
        spx::gate(self.dev, self.hooks.can_write(), self.sp.access)
    }

    /// The one place a user-initiated group change is allowed: needs a real login and the master
    /// switch. Says why and returns `None` otherwise.
    fn sp_write(&mut self, cx: &mut Context<Self>) -> Option<(UserClient, UserConfirmed)> {
        if self.dev || !self.hooks.can_write() {
            self.toast(self.writes_off(), cx);
            return None;
        }
        Some((self.sp_api()?, UserConfirmed::i_have_asked_the_user()))
    }

    /// Traffic of a group we are already in (Ready, Buffering, Ping, transport requests). The
    /// switch being turned off leaves the group (see `sp_writes_changed`), so this is only ever
    /// used while writes are allowed.
    fn sp_group_client(&self) -> Option<(UserClient, UserConfirmed)> {
        if self.dev || !self.hooks.can_write() || !self.sp.engine.in_group() {
            return None;
        }
        Some((self.sp_api()?, UserConfirmed::i_have_asked_the_user()))
    }

    /// Settings changed: no group without the master switch.
    pub(super) fn sp_writes_changed(&mut self, cx: &mut Context<Self>) {
        if self.sp.engine.in_group() && !self.hooks.can_write() {
            self.sp_leave(cx);
        }
    }

    // ---- screen ----

    pub(super) fn sp_open_screen(&mut self, cx: &mut Context<Self>) {
        self.help_open = false;
        self.sp.open = true;
        self.sp.sel = 0;
        self.sp.name_focus = false;
        self.sp.groups = Load::Loading;
        if self.dev {
            return;
        }
        self.sp_connect(cx);
        self.sp_refresh(cx);
    }

    pub(super) fn sp_close_screen(&mut self) {
        self.sp.open = false;
        if !self.sp.engine.in_group() {
            self.sp_disconnect();
        }
    }

    fn sp_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(api) = self.sp_api() else { return };
        let a2 = api.clone();
        self.run(
            cx,
            async move { a2.syncplay_access().await.ok() },
            |t, r, _| {
                if r.is_some() {
                    t.sp.access = r;
                }
            },
        );
        self.run(
            cx,
            async move { api.syncplay_list().await.ok() },
            |t, r, _| {
                t.sp.groups = r.map_or(Load::Failed, Load::Ready);
                t.sp.sel = 0;
            },
        );
    }

    // ---- socket ----

    fn sp_connect(&mut self, cx: &mut Context<Self>) {
        if self.sp.rx.is_some() {
            return;
        }
        let Some(api) = self.sp_api() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        self.sp.task = Some(bridge::spawn(ws::run(api.clone(), tx)));
        self.sp.api = Some(api);
        self.sp.rx = Some(rx);
        self.sp.link = Link::Connecting;
        self.sp.sync_n = 0;
        self.sp.next_sync = Instant::now();
        self.sp.epoch += 1;
        if !self.sp.looping {
            self.sp.looping = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    if !this.update(cx, |t, cx| t.sp_poll(cx)).unwrap_or(false) {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    fn sp_disconnect(&mut self) {
        if let Some(t) = self.sp.task.take() {
            t.abort();
        }
        self.sp.rx = None;
        self.sp.api = None;
        self.sp.link = Link::Off;
        self.sp.epoch += 1;
        self.sp.engine.time.reset();
    }

    /// One pass every 50 ms while a socket exists. Returns false to end the loop.
    fn sp_poll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = &self.sp.rx else {
            self.sp.looping = false;
            return false;
        };
        let evs: Vec<SocketEvent> = rx.try_iter().collect();
        for e in evs {
            self.sp_event(e, cx);
        }
        self.sp_time(cx);
        self.sp_give_up(cx);
        if self.sp.engine.in_group() {
            self.sp_drain_outbox(cx);
            if self.sp.last_player_poll.elapsed() >= Duration::from_millis(100) {
                self.sp.last_player_poll = Instant::now();
                self.sp_poll_player(cx);
            }
        }
        cx.notify();
        true
    }

    fn sp_event(&mut self, e: SocketEvent, cx: &mut Context<Self>) {
        match e {
            SocketEvent::Connected => {
                let was_down = matches!(self.sp.link, Link::Down(_));
                self.sp.link = Link::Up;
                self.sp.next_sync = Instant::now();
                if was_down && self.sp.engine.in_group() {
                    self.sp_resume_group(cx);
                }
            }
            SocketEvent::Disconnected => self.sp.link = Link::Down(Instant::now()),
            SocketEvent::Message(Incoming::Command(c)) => {
                let snap = self.sp_snap(cx);
                let outs = self.sp.engine.command(c, now_ms(), snap);
                self.sp_outs(outs, cx);
            }
            SocketEvent::Message(Incoming::Group(u)) => {
                if matches!(u, GroupUpdate::GroupDoesNotExist) {
                    self.sp.busy = false;
                    self.sp_refresh(cx);
                }
                let outs = self.sp.engine.group_update(u, now_ms());
                self.sp_outs(outs, cx);
            }
            SocketEvent::Message(_) => {}
        }
    }

    /// Clock samples: four quick ones after connecting, then every 30 s; each is followed by a
    /// `Ping` to the group so the server leads its commands by our real latency.
    fn sp_time(&mut self, cx: &mut Context<Self>) {
        if self.sp.link != Link::Up || self.sp.sync_busy || Instant::now() < self.sp.next_sync {
            return;
        }
        let Some(api) = self.sp_api() else { return };
        self.sp.sync_busy = true;
        self.run(
            cx,
            async move { api.syncplay_time_sample().await.ok() },
            |t, r, cx| {
                t.sp.sync_busy = false;
                let Some(s) = r else {
                    t.sp.next_sync = Instant::now() + Duration::from_secs(2);
                    return;
                };
                let first = !t.sp.engine.time.ready();
                t.sp.engine.time.add(s);
                t.sp.next_sync = Instant::now() + spx::sample_gap(t.sp.sync_n);
                t.sp.sync_n += 1;
                if first {
                    let snap = t.sp_snap(cx);
                    let outs = t.sp.engine.clock_ready(now_ms(), snap);
                    t.sp_outs(outs, cx);
                }
                if let Some((api, c)) = t.sp_group_client() {
                    let ping = t.sp.engine.time.ping();
                    bridge::spawn(async move {
                        let _ = api.syncplay_ping(ping, c).await;
                    });
                }
            },
        );
    }

    /// The socket has been down too long: the server has dropped us, so stop pretending.
    fn sp_give_up(&mut self, cx: &mut Context<Self>) {
        if let Link::Down(since) = self.sp.link
            && self.sp.engine.in_group()
            && since.elapsed() > spx::GIVE_UP_AFTER
        {
            self.sp_local_reset(cx);
            self.toast(
                "Lost the connection to the group. You are no longer in it.",
                cx,
            );
        }
    }

    /// The socket is back while we think we are in a group: is it still there, and are we in it?
    fn sp_resume_group(&mut self, cx: &mut Context<Self>) {
        let (Some(api), Some(g)) = (self.sp_api(), self.sp.engine.group().cloned()) else {
            return;
        };
        let me = self.session.as_ref().map(|(_, s)| s.user_name.clone());
        let gid = g.group_id.clone();
        self.run(
            cx,
            async move { api.syncplay_group(&gid).await.ok() },
            move |t, r, cx| match r {
                Some(info) if me.is_some_and(|m| info.participants.contains(&m)) => {
                    // Still a member but we missed messages: leave and join again, which makes
                    // the server send the queue and position afresh.
                    t.sp.rejoining = true;
                    t.sp_rejoin(g.group_id.clone(), cx);
                }
                _ => {
                    t.sp_local_reset(cx);
                    t.toast("The group is gone. You are no longer in it.", cx);
                }
            },
        );
    }

    fn sp_rejoin(&mut self, id: String, cx: &mut Context<Self>) {
        let Some((api, c)) = self.sp_group_client() else {
            return;
        };
        self.run(
            cx,
            async move {
                let _ = api.syncplay_leave(c).await;
                api.syncplay_join(&id, c).await.is_ok()
            },
            |t, ok, cx| {
                t.sp.rejoining = false;
                if !ok {
                    t.sp_local_reset(cx);
                    t.toast("Could not rejoin the group.", cx);
                }
            },
        );
    }

    // ---- player side ----

    fn sp_snap(&self, cx: &App) -> Option<PlayerSnap> {
        let p = self.player.as_ref()?.read(cx);
        if !p.group {
            return None;
        }
        let (pos, playing) = p.group_snap()?;
        Some(PlayerSnap {
            pos_ticks: to_ticks(pos),
            playing,
        })
    }

    fn sp_outs(&mut self, outs: Vec<Out>, cx: &mut Context<Self>) {
        for o in outs {
            match o {
                Out::Joined(g) => {
                    self.sp.busy = false;
                    self.sp.following = true;
                    self.toast(format!("Joined {}", g.group_name), cx);
                    self.sp_refresh(cx);
                }
                Out::Left => {
                    self.sp_local_reset(cx);
                    self.toast("The group ended.", cx);
                }
                Out::Notice(s) => self.toast(s, cx),
                Out::Apply {
                    pre_seek,
                    in_ms,
                    op,
                } => self.sp_schedule(pre_seek, in_ms, op, cx),
                Out::Queue { queue, switch } => self.sp_queue(queue, switch, cx),
            }
        }
    }

    fn sp_schedule(&mut self, pre_seek: Option<i64>, in_ms: f64, op: Op, cx: &mut Context<Self>) {
        self.sp.epoch += 1;
        let epoch = self.sp.epoch;
        if let Some(p) = self.player.clone() {
            p.update(cx, |p, _| {
                p.group_speed(1.0);
                if let Some(t) = pre_seek {
                    p.group_seek(secs(t));
                }
            });
        }
        if in_ms <= 2.0 {
            return self.sp_run_op(op, true, cx);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_micros((in_ms * 1000.0) as u64))
                .await;
            this.update(cx, |t, cx| {
                if t.sp.epoch == epoch {
                    t.sp_run_op(op, false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// `late`: the moment has already passed, so also jump to where the group is.
    fn sp_run_op(&mut self, op: Op, late: bool, cx: &mut Context<Self>) {
        let Some(p) = self.player.clone().filter(|p| p.read(cx).group) else {
            return;
        };
        match op {
            Op::Unpause { pos_ticks } => {
                p.update(cx, |p, _| {
                    let far = (p.pos() - secs(pos_ticks)).abs() > 0.4;
                    if late && far {
                        p.group_seek(secs(pos_ticks));
                    }
                    p.group_unpause();
                });
                self.sp.engine.unpaused(now_ms());
                // Answer promptly even when already ready: the server counts every member.
                self.sp_ready(false, cx);
            }
            Op::Pause { pos_ticks } => p.update(cx, |p, _| p.group_pause(secs(pos_ticks))),
            Op::Seek { pos_ticks } => p.update(cx, |p, _| {
                p.group_pause(secs(pos_ticks));
                p.ready_due = Some(secs(pos_ticks));
            }),
            Op::Stop => p.update(cx, |p, cx| p.close(false, cx)),
        }
        cx.notify();
    }

    /// `Ready` (or `Buffering`) with our position and the item the group is on.
    fn sp_ready(&mut self, buffering: bool, cx: &mut Context<Self>) {
        let Some(snap) = self.sp_snap(cx) else { return };
        let (Some(rep), Some((api, c))) = (
            self.sp.engine.report(now_ms(), snap),
            self.sp_group_client(),
        ) else {
            return;
        };
        bridge::spawn(async move {
            let _ = api.syncplay_report(buffering, &rep, c).await;
        });
    }

    /// Ready after a load or seek once settled, Buffering / Ready around stalls, drift correction.
    fn sp_poll_player(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.player.clone() else { return };
        let (group, loaded, due, snap, (seeking, cache)) = {
            let v = p.read(cx);
            (
                v.group,
                v.loaded,
                v.ready_due,
                v.group_snap(),
                v.group_flags(),
            )
        };
        let Some((pos, playing)) = snap.filter(|_| group && loaded) else {
            return;
        };
        if let Some(t) = due
            && !seeking
            && !cache
            && spx::settled(t, pos)
        {
            p.update(cx, |p, _| p.ready_due = None);
            self.sp_ready(false, cx);
        }
        match self
            .sp
            .stall
            .update(self.sp.clock.elapsed(), cache && playing)
        {
            Some(StallEvent::Began) => self.sp_ready(true, cx),
            Some(StallEvent::Ended) => self.sp_ready(false, cx),
            None => {}
        }
        let (now, s) = (
            now_ms(),
            PlayerSnap {
                pos_ticks: to_ticks(pos),
                playing,
            },
        );
        if self.sp.engine.speed_reset_due(now) {
            p.update(cx, |p, _| p.group_speed(1.0));
        }
        let act = self.sp.engine.drift(now, s, self.sp.stall.reported());
        let diff = self.sp.engine.drift_ms(now, s);
        p.update(cx, |p, _| {
            p.group_diff = diff;
            match act {
                Some(Drift::Speed { factor, .. }) => p.group_speed(factor),
                Some(Drift::Skip { to_ticks }) => p.group_seek(secs(to_ticks)),
                None => {}
            }
        });
    }

    fn sp_drain_outbox(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.player.clone() else { return };
        let reqs = p.update(cx, |p, _| std::mem::take(&mut p.outbox));
        for r in reqs {
            self.sp_req(r, cx);
        }
    }

    /// A transport request from the local player goes to the group.
    fn sp_req(&mut self, r: GroupReq, cx: &mut Context<Self>) {
        let Some((api, c)) = self.sp_group_client() else {
            return self.toast(self.writes_off(), cx);
        };
        let pli = self.sp.engine.playlist_item_id().map(str::to_owned);
        let entry = |i: usize| {
            self.sp
                .engine
                .queue()
                .and_then(|q| q.playlist.get(i))
                .map(|x| x.playlist_item_id.clone())
        };
        let item = match r {
            GroupReq::SetIndex(i) => entry(i),
            _ => pli,
        };
        self.run(
            cx,
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
            |t, ok, cx| {
                if !ok {
                    t.toast("The group could not be reached.", cx);
                }
            },
        );
    }

    // ---- queue ----

    fn sp_queue(&mut self, q: PlayQueue, switch: bool, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let ids: Vec<String> = q.playlist.iter().map(|i| i.item_id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        let (client, user) = src.client();
        let mut uniq = ids.clone();
        uniq.sort();
        uniq.dedup();
        let (stamp, idx, reason) = (q.last_update_ms, q.playing_item_index, q.reason);
        self.run(
            cx,
            async move {
                let query = nocturne_jellyfin::ItemsQuery {
                    ids: uniq,
                    ..Default::default()
                };
                client
                    .items(&user, &query)
                    .await
                    .ok()
                    .map(|r| r.items.iter().map(Media::from_item).collect::<Vec<Media>>())
            },
            move |t, got, cx| {
                if t.sp.engine.queue().map(|q| q.last_update_ms) != Some(stamp) {
                    return; // a newer queue arrived meanwhile
                }
                match got.and_then(|g| spx::order_items(&ids, g).ok()) {
                    Some(list) => t.sp_apply_queue(list, idx, switch, reason, cx),
                    None => t.sp_unavailable(cx),
                }
            },
        );
    }

    fn sp_apply_queue(
        &mut self,
        list: Vec<Media>,
        idx: i32,
        switch: bool,
        reason: QueueReason,
        cx: &mut Context<Self>,
    ) {
        let Ok(idx) = usize::try_from(idx) else {
            return;
        };
        if !switch {
            if let Some(p) = self.player.clone() {
                p.update(cx, |p, _| p.group_queue(list, idx));
            }
            return;
        }
        if !self.sp.following {
            if reason != QueueReason::NewPlaylist {
                return;
            }
            self.sp.following = true;
            self.sp_ignore_wait(false);
        }
        let start = secs(self.sp.engine.start_ticks(now_ms()));
        self.sp.stall.reset();
        match self.player.clone() {
            Some(p) => p.update(cx, |p, cx| p.group_load(list, idx, start, cx)),
            None => self.sp_open_player(list, idx, start, cx),
        }
    }

    /// Something in the group's queue is not available to this user: stay in the group, but do
    /// not hold the others back.
    fn sp_unavailable(&mut self, cx: &mut Context<Self>) {
        self.sp.following = false;
        self.sp_ignore_wait(true);
        self.toast(
            "Something in the group's queue is not available to you. You stay in the group without holding it back.",
            cx,
        );
    }

    fn sp_ignore_wait(&self, ignore: bool) {
        if let Some((api, c)) = self.sp_group_client() {
            bridge::spawn(async move {
                let _ = api.syncplay_ignore_wait(ignore, c).await;
            });
        }
    }

    fn sp_open_player(&mut self, list: Vec<Media>, idx: usize, start: f64, cx: &mut Context<Self>) {
        let Some(m) = list.get(idx).cloned() else {
            return;
        };
        let Some(ctx) = self.player_ctx() else { return };
        self.hooks.seed_favorite(&m.id, m.favorite);
        self.report_stop();
        let view = cx.new(|cx| {
            let mut v = PlayerView::new_group(ctx, m, start, cx);
            v.queue = list;
            v.qi = idx;
            v
        });
        self.attach_player(view, cx);
    }

    /// The video was closed locally while in a group: stop holding the group back.
    pub(super) fn sp_player_closed(&mut self, cx: &mut Context<Self>) {
        if self.sp.engine.in_group() && self.sp.following {
            self.sp.following = false;
            self.sp_ignore_wait(true);
            self.toast(
                "You left the video. The group plays on; open Watch together to rejoin.",
                cx,
            );
        }
    }

    /// Watch together > rejoin: load whatever the group is on.
    fn sp_follow_again(&mut self, cx: &mut Context<Self>) {
        let Some(q) = self.sp.engine.queue().cloned() else {
            return self.toast("Nothing is playing in the group yet.", cx);
        };
        self.sp.following = true;
        self.sp_ignore_wait(false);
        self.sp_close_screen();
        self.sp_queue(q, true, cx);
    }

    // ---- create, join, leave, play ----

    fn sp_create(&mut self, cx: &mut Context<Self>) {
        if !self.sp_gate().can_create() {
            return self.toast(self.sp_gate().reason().unwrap_or("Not allowed"), cx);
        }
        let name = self.sp.name.trim().to_string();
        if name.is_empty() {
            return self.toast("Give the group a name first.", cx);
        }
        let Some((api, c)) = self.sp_write(cx) else {
            return;
        };
        self.sp.busy = true;
        self.run(
            cx,
            async move { api.syncplay_new(&name, c).await },
            |t, r, cx| match r {
                Ok(Some(g)) => {
                    t.sp.name.clear();
                    let outs =
                        t.sp.engine
                            .group_update(GroupUpdate::GroupJoined(g), now_ms());
                    t.sp_outs(outs, cx);
                }
                Ok(None) => t.sp.name.clear(), // the socket tells us
                Err(_) => {
                    t.sp.busy = false;
                    t.toast("The group could not be created.", cx);
                }
            },
        );
    }

    fn sp_join(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.sp_gate().can_join() {
            return self.toast(self.sp_gate().reason().unwrap_or("Not allowed"), cx);
        }
        let Some((api, c)) = self.sp_write(cx) else {
            return;
        };
        self.sp.busy = true;
        self.run(
            cx,
            async move { api.syncplay_join(&id, c).await.is_ok() },
            |t, ok, cx| {
                if !ok {
                    t.sp.busy = false;
                    t.toast("Could not join. The group may be gone.", cx);
                    t.sp_refresh(cx);
                }
            },
        );
    }

    /// Always allowed, even with server changes off: nobody should be stuck in a group.
    pub(super) fn sp_leave(&mut self, cx: &mut Context<Self>) {
        if !self.sp.engine.in_group() {
            return;
        }
        if let Some(api) = self.sp_api() {
            let c = UserConfirmed::i_have_asked_the_user();
            bridge::spawn(async move {
                let _ = api.syncplay_leave(c).await;
            });
        }
        self.sp_local_reset(cx);
        self.toast("You left the group.", cx);
        self.sp_refresh(cx);
    }

    /// Local control again: the player keeps what it has but follows nobody.
    fn sp_local_reset(&mut self, cx: &mut Context<Self>) {
        self.sp.engine.reset();
        self.sp.epoch += 1;
        self.sp.busy = false;
        self.sp.following = true;
        self.sp.rejoining = false;
        self.sp.stall.reset();
        if let Some(p) = self.player.clone() {
            p.update(cx, |p, _| {
                p.group = false;
                p.outbox.clear();
                p.ready_due = None;
                p.group_diff = None;
                p.group_speed(1.0);
            });
        }
        if !self.sp.open {
            self.sp_disconnect();
        }
    }

    /// Sign-out, switching account or server, quitting: leave quietly and drop the socket.
    pub(super) fn sp_shutdown(&mut self, cx: &mut Context<Self>) {
        if self.sp.engine.in_group()
            && let Some(api) = self.sp_api()
        {
            let c = UserConfirmed::i_have_asked_the_user();
            bridge::spawn(async move {
                let _ = api.syncplay_leave(c).await;
            });
        }
        self.sp.engine.reset();
        self.sp.open = false;
        self.sp.access = None;
        self.sp_disconnect();
        let _ = cx;
    }

    /// Choosing something to play while in a group sets the group's queue for everyone.
    pub(super) fn sp_play(&mut self, m: Media, rest: Vec<Media>, cx: &mut Context<Self>) {
        let Some((api, c)) = self.sp_write(cx) else {
            return;
        };
        let q = NewQueue {
            item_ids: std::iter::once(m.id.clone())
                .chain(rest.into_iter().map(|r| r.id))
                .take(200)
                .collect(),
            start_index: 0,
            start_ticks: 0,
        };
        self.toast("Starting it for the group", cx);
        self.run(
            cx,
            async move { api.syncplay_set_new_queue(&q, c).await.is_ok() },
            |t, ok, cx| {
                if !ok {
                    t.toast("The group could not start that.", cx);
                }
            },
        );
    }

    // ---- keys ----

    pub(super) fn sp_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let m = &ev.keystroke.modifiers;
        let in_group = self.sp.engine.in_group();
        match key {
            "escape" => self.sp_close_screen(),
            "tab" => self.sp.name_focus = !self.sp.name_focus,
            "r" if !self.sp.name_focus || in_group => self.sp_refresh(cx),
            "l" if in_group => self.sp_leave(cx),
            "enter" if in_group => self.sp_follow_again(cx),
            "enter" if self.sp.name_focus => self.sp_create(cx),
            "enter" => {
                let id = match &self.sp.groups {
                    Load::Ready(g) => g.get(self.sp.sel).map(|g| g.group_id.clone()),
                    _ => None,
                };
                if let Some(id) = id {
                    self.sp_join(id, cx);
                }
            }
            "up" if !in_group => self.sp.sel = self.sp.sel.saturating_sub(1),
            "down" if !in_group => {
                let n = match &self.sp.groups {
                    Load::Ready(g) => g.len(),
                    _ => 0,
                };
                self.sp.sel = (self.sp.sel + 1).min(n.saturating_sub(1));
            }
            "backspace" if self.sp.name_focus && !in_group => {
                self.sp.name.pop();
            }
            _ if self.sp.name_focus && !in_group => {
                if let Some(c) = ev
                    .keystroke
                    .key_char
                    .as_deref()
                    .filter(|c| !m.control && !m.platform && !c.chars().any(char::is_control))
                    && self.sp.name.len() < 100
                {
                    self.sp.name.push_str(c);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    // ---- views ----

    /// "Watching together" chip with Leave, shown while in a group (header and player).
    pub(super) fn sp_pill(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let g = self.sp.engine.group()?;
        let status = match self.sp.link {
            Link::Down(_) => "reconnecting".to_string(),
            _ => format!("{} watching", g.participants.len()),
        };
        let label = format!("{} \u{00B7} {status}", g.group_name);
        Some(
            div()
                .id("sp-pill")
                .h(px(32.0))
                .pl(px(t::S3))
                .flex()
                .items_center()
                .gap(px(t::S2))
                .rounded(px(6.0))
                .border_1()
                .border_color(t::accent())
                .bg(t::accent_tint())
                .cursor_pointer()
                .tooltip(ui::tip("Watch together (G)"))
                .on_click(cx.listener(|t, _, _, cx| {
                    t.sp_open_screen(cx);
                    cx.notify();
                }))
                .child(div().size(px(6.0)).rounded_full().bg(t::accent()))
                .child(
                    div()
                        .max_w(px(260.0))
                        .overflow_hidden()
                        .text_size(px(t::CAPTION + 1.0))
                        .text_color(t::text())
                        .child(label),
                )
                .child(
                    ui::action_button(
                        "sp-pill-leave".into(),
                        "Leave",
                        false,
                        false,
                        false,
                        false,
                        cx.listener(|t, _, _, cx| {
                            t.sp_leave(cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .h(px(30.0)),
                ),
        )
    }

    pub(super) fn sp_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.sp.open {
            return None;
        }
        let gate = self.sp_gate();
        let mut p = self.overlay_panel("Watch together").w(px(640.0)).child(
            div()
                .text_size(px(t::BODY))
                .text_color(t::text_secondary())
                .child("Play, pause, seek and skip stay in step for everyone in the group."),
        );
        if let Some(r) = gate.reason() {
            let mut row = div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(t::S3))
                .p(px(t::S3))
                .rounded(px(t::R_PANEL))
                .border_1()
                .border_color(t::hairline_strong())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(t::BODY))
                        .text_color(t::text())
                        .child(r),
                );
            if gate == Gate::NeedWrites {
                row = row.child(ui::action_button(
                    "sp-open-settings".into(),
                    "Open Settings",
                    true,
                    false,
                    false,
                    false,
                    cx.listener(|t, _, _, cx| {
                        t.sp_close_screen();
                        t.menu_act(AcctAct::Settings, cx);
                        cx.notify();
                    }),
                ));
            }
            p = p.child(row);
        }
        p = match self.sp.engine.group() {
            Some(g) => p.child(self.sp_in_group(g, cx)),
            None => p.child(self.sp_browse(gate, cx)),
        };
        p = p.child(ui::mono(
            if self.sp.engine.in_group() {
                "Esc closes  \u{00B7}  L leaves  \u{00B7}  Enter plays what the group is on"
            } else {
                "Esc closes  \u{00B7}  Tab switches list and name  \u{00B7}  Enter joins or creates  \u{00B7}  R reloads"
            }
            .to_string(),
        ));
        Some(self.overlay_backdrop("sp-away", p, cx))
    }

    fn sp_in_group(&self, g: &GroupInfo, cx: &mut Context<Self>) -> Div {
        let link = match self.sp.link {
            Link::Up => "connected",
            Link::Down(_) => "reconnecting",
            _ => "connecting",
        };
        let state = format!("{:?}", g.state).to_lowercase();
        let mut d = div()
            .flex()
            .flex_col()
            .gap(px(t::S3))
            .child(
                div()
                    .text_size(px(t::HEADING))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(g.group_name.clone()),
            )
            .child(ui::mono(format!(
                "{state} \u{00B7} {link} \u{00B7} {} watching",
                g.participants.len()
            )))
            .child(
                div()
                    .text_size(px(t::BODY))
                    .text_color(t::text_secondary())
                    .child(g.participants.join(", ")),
            )
            .child(
                div()
                    .text_size(px(t::BODY))
                    .text_color(t::text_secondary())
                    .child("Pick anything to play: it starts for everyone."),
            );
        let mut row = div().flex().gap(px(t::S3));
        if self.player.is_none() || !self.sp.following {
            row = row.child(ui::action_button(
                "sp-follow".into(),
                "Play what the group is on",
                true,
                false,
                false,
                self.sp.engine.queue().is_none(),
                cx.listener(|t, _, _, cx| {
                    t.sp_follow_again(cx);
                    cx.notify();
                }),
            ));
        }
        row = row.child(ui::action_button(
            "sp-leave".into(),
            "Leave group",
            false,
            false,
            false,
            false,
            cx.listener(|t, _, _, cx| {
                t.sp_leave(cx);
                cx.notify();
            }),
        ));
        d = d.child(row);
        d
    }

    fn sp_browse(&self, gate: Gate, cx: &mut Context<Self>) -> Div {
        let mut d = div().flex().flex_col().gap(px(t::S3));
        if !self.dev {
            d = d.child(ui::mono(ui::tracked("Groups on this server")));
        }
        match &self.sp.groups {
            _ if self.dev => {}
            Load::Loading => d = d.child(ui::mono("Loading".to_string())),
            Load::Failed => {
                d = d.child(ui::mono(
                    "The groups could not be loaded. Press R.".to_string(),
                ))
            }
            Load::Ready(v) if v.is_empty() => {
                d = d.child(ui::mono(
                    "No groups yet. Create one below and ask others to join.".to_string(),
                ))
            }
            Load::Ready(v) => {
                for (i, g) in v.iter().enumerate() {
                    let id = g.group_id.clone();
                    let sel = i == self.sp.sel && !self.sp.name_focus;
                    d = d.child(
                        div()
                            .id(ui::name(format!("sp-g{i}")))
                            .py(px(t::S2))
                            .px(px(t::S3))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(t::S4))
                            .rounded(px(t::R_PANEL))
                            .border_1()
                            .border_color(if sel { t::accent() } else { t::hairline() })
                            .when(sel, |d| d.bg(t::accent_tint()))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(t::BODY))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(g.group_name.clone()),
                                    )
                                    .child(ui::mono(format!(
                                        "{} \u{00B7} {}",
                                        format!("{:?}", g.state).to_lowercase(),
                                        g.participants.join(", ")
                                    ))),
                            )
                            .child(ui::action_button(
                                format!("sp-join{i}"),
                                "Join",
                                true,
                                false,
                                false,
                                !gate.can_join() || self.sp.busy,
                                cx.listener(move |t, _, _, cx| {
                                    t.sp_join(id.clone(), cx);
                                    cx.notify();
                                }),
                            )),
                    );
                }
            }
        }
        if gate == Gate::Create || gate == Gate::NeedWrites {
            let focus = self.sp.name_focus;
            let shown = if self.sp.name.is_empty() && !focus {
                "Name a new group".to_string()
            } else {
                format!("{}{}", self.sp.name, if focus { "\u{2502}" } else { "" })
            };
            d = d.child(
                div()
                    .pt(px(t::S3))
                    .flex()
                    .items_center()
                    .gap(px(t::S3))
                    .child(
                        div()
                            .id("sp-name")
                            .flex_1()
                            .h(px(32.0))
                            .px(px(t::S3))
                            .flex()
                            .items_center()
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(if focus { t::accent() } else { t::edge() })
                            .text_size(px(t::BODY))
                            .text_color(if self.sp.name.is_empty() {
                                t::text_tertiary()
                            } else {
                                t::text()
                            })
                            .cursor_text()
                            .on_click(cx.listener(|t, _, _, cx| {
                                t.sp.name_focus = true;
                                cx.notify();
                            }))
                            .child(shown),
                    )
                    .child(ui::action_button(
                        "sp-create".into(),
                        "Create group",
                        true,
                        false,
                        false,
                        !gate.can_create() || self.sp.busy,
                        cx.listener(|t, _, _, cx| {
                            t.sp_create(cx);
                            cx.notify();
                        }),
                    )),
            );
        }
        d
    }
}
