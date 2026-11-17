//! Remote control from other Jellyfin apps (opt-in; docs/WRITES.md). While the setting and the
//! server-changes switch are on, each signed-in server gets this session's capabilities and one
//! WebSocket whose `Play` / `Playstate` / `GeneralCommand` frames drive the player
//! (`nocturne_core::remote` maps them). Commands only ever control playback: no writes of
//! their own, no admin actions, no confirmations. Playback reports (separately gated) are what
//! lets the controlling app see state.
use super::message::Message;
use super::{App, Phase};
use crate::task::run;
use iced::Task;
use nocturne_core::backend::{PlayError, PlayTarget};
use nocturne_core::bridge;
use nocturne_core::remote::{self, RemoteCmd};
use nocturne_core::source::{Media, SourceId};
use nocturne_core::video::TrackKind;
use nocturne_jellyfin_user::UserClient;
use nocturne_jellyfin_user::syncplay::Incoming;
use nocturne_jellyfin_user::syncplay::ws::{self, SocketEvent};
use std::sync::mpsc::Receiver;

pub(super) struct RemoteLink {
    task: tokio::task::JoinHandle<()>,
    rx: Receiver<SocketEvent>,
    client: UserClient,
}

impl Drop for RemoteLink {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn post(client: UserClient, on: bool) -> Task<Message> {
    run(
        async move { client.post_capabilities(remote::capabilities(on)).await },
        |_| Message::Noop,
    )
}

impl App {
    /// Called every tick: open or close the sockets to match the switches, then act on what
    /// arrived.
    pub(super) fn sync_remote(&mut self) -> Task<Message> {
        let writes = self.settings.allow_server_writes && !self.dev;
        let want = self.settings.remote_control && writes && matches!(self.phase, Phase::Ready);
        let mut tasks = vec![];
        // Gone sources, or the setting turned off: close. Off while writes are still allowed:
        // tell the server this session is no longer controllable.
        let stale: Vec<SourceId> = self
            .remote
            .keys()
            .filter(|id| !want || !self.sessions.contains_key(id))
            .copied()
            .collect();
        for id in stale {
            if let Some(l) = self.remote.remove(&id)
                && writes
                && !self.settings.remote_control
            {
                tasks.push(post(l.client.clone(), false));
            }
        }
        if !want {
            return Task::batch(tasks);
        }
        for (id, (url, s)) in &self.sessions {
            if self.remote.contains_key(id) {
                continue;
            }
            let Ok(client) = UserClient::from_session(url, s) else {
                continue;
            };
            let (tx, rx) = std::sync::mpsc::channel();
            let task = bridge::spawn(ws::run(client.clone(), tx));
            self.remote.insert(*id, RemoteLink { task, rx, client });
        }
        let mut cmds = vec![];
        for (id, l) in &self.remote {
            for e in l.rx.try_iter() {
                match e {
                    // Capabilities belong to the session; post again whenever the socket is back.
                    SocketEvent::Connected => tasks.push(post(l.client.clone(), true)),
                    SocketEvent::Message(Incoming::Remote(t, d)) => {
                        cmds.extend(remote::parse(&t, &d).map(|c| (*id, c)));
                    }
                    _ => {}
                }
            }
        }
        for (id, c) in cmds {
            tasks.push(self.remote_cmd(id, c));
        }
        Task::batch(tasks)
    }

    pub(super) fn remote_shutdown(&mut self) {
        self.remote.clear();
    }

    fn remote_cmd(&mut self, origin: SourceId, c: RemoteCmd) -> Task<Message> {
        if let RemoteCmd::Play {
            ids,
            start_index,
            start_secs,
            queue_only,
        } = c
        {
            let Some(b) = self.catalog.backend(origin).cloned() else {
                return Task::none();
            };
            return run(
                async move {
                    let mut list = vec![];
                    for id in ids {
                        if let Ok((m, _)) = b.full(&id).await {
                            list.push(m);
                        }
                    }
                    list
                },
                move |list| Message::RemoteItems(list, start_index, start_secs, queue_only),
            );
        }
        let Some(p) = self.player.as_mut() else {
            return Task::none();
        };
        let mut next = Task::none();
        let mut note = None;
        match c {
            RemoteCmd::PlayPause => p.toggle(),
            RemoteCmd::Pause => p.remote_pause(true),
            RemoteCmd::Unpause => p.remote_pause(false),
            RemoteCmd::SeekTo(s) => p.seek_to(s),
            RemoteCmd::SeekBy(s) => p.seek(s),
            RemoteCmd::Volume(v) => p.remote_volume(v),
            RemoteCmd::Mute(w) => p.remote_mute(w),
            RemoteCmd::Audio(i) => p.remote_stream(i, TrackKind::Audio),
            RemoteCmd::Subtitle(i) => p.remote_stream(i, TrackKind::Sub),
            RemoteCmd::Message(t) => note = Some(t),
            RemoteCmd::Stop => next = p.stop(),
            RemoteCmd::Next => next = p.step(1),
            RemoteCmd::Previous => next = p.step(-1),
            RemoteCmd::Play { .. } => {}
        }
        if let Some(t) = note {
            self.toast(t);
        }
        self.after_player(next)
    }

    /// The items a remote `Play` named, resolved: queue them or start playing.
    pub(super) fn remote_items(
        &mut self,
        list: Vec<Media>,
        idx: usize,
        start: f64,
        queue_only: bool,
    ) -> Task<Message> {
        let idx = idx.min(list.len().saturating_sub(1));
        let Some(m) = list.get(idx).cloned() else {
            return Task::none();
        };
        if let Some(p) = self.player.as_mut() {
            if queue_only {
                p.queue.extend(list);
                self.toast("Added to the queue from another device.");
                return Task::none();
            }
            let t = p.remote_load(list, idx, start);
            return self.after_player(t);
        }
        if !matches!(self.phase, Phase::Ready) {
            return Task::none();
        }
        let Some(b) = self.catalog.backend(m.origin).cloned() else {
            return Task::none();
        };
        run(async move { b.play_target(&m).await }, move |r| {
            Message::RemoteTarget(list, idx, start, r)
        })
    }

    pub(super) fn remote_target(
        &mut self,
        list: Vec<Media>,
        idx: usize,
        start: f64,
        r: Result<PlayTarget, PlayError>,
    ) -> Task<Message> {
        let (Ok(target), Some(m)) = (r, list.get(idx).cloned()) else {
            self.toast("A video from another device couldn't be opened.");
            return Task::none();
        };
        if self.player.is_some() || !matches!(self.phase, Phase::Ready) {
            return Task::none();
        }
        let ctx = self.player_ctx(m, list.clone(), None, target);
        self.menu = false;
        let (mut p, open) = crate::player::Player::open(ctx);
        p.remote_start(start);
        p.queue = list;
        p.qi = idx;
        self.player = Some(p);
        open.map(Message::Player)
    }
}
