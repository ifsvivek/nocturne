//! Step 4e on the root: Admin, Watch together (SyncPlay), the music queue and "Your devices".
//! Each owns its state and keyboard; the root routes messages and keys, draws them as overlays and
//! carries out the outcomes (hook notes at the top of `admin/`, `syncplay/`, `queue.rs`,
//! `account/devices.rs`).
use super::catalog::DEV;
use super::message::Message;
use super::{App, Phase};
use crate::account::devices;
use crate::admin;
use crate::keys::Key;
use crate::queue;
use crate::syncplay;
use crate::task::run;
use iced::{Alignment, Element, Length, Task};
use nocturne_core::backend::{Backend, PlayError, PlayTarget};
use nocturne_core::source::{Media, SourceId};
use nocturne_jellyfin_user::UserClient;

impl App {
    // ---- Admin ----

    /// One target per signed-in Jellyfin source whose account is an administrator.
    fn admin_targets(&self) -> Vec<admin::Target> {
        if self.dev {
            let (Some(cfg), Some(Backend::Jellyfin(s))) = (
                self.cfg.clone(),
                self.catalog.slot(DEV).and_then(|s| s.backend.clone()),
            ) else {
                return vec![];
            };
            return vec![admin::Target {
                origin: DEV,
                label: "Developer token".into(),
                cfg,
                pics: Some(s),
            }];
        }
        self.catalog
            .priority()
            .into_iter()
            .filter_map(|id| {
                let (url, s) = self.sessions.get(&id).filter(|(_, s)| s.is_admin)?;
                let slot = self.catalog.slot(id).filter(|s| s.status.usable())?;
                let Some(Backend::Jellyfin(src)) = slot.backend.clone() else {
                    return None;
                };
                Some(admin::Target {
                    origin: id,
                    label: slot.profile.label.clone(),
                    cfg: nocturne_jellyfin::Config {
                        base_url: url.clone(),
                        token: s.access_token.clone(),
                    },
                    pics: Some(src),
                })
            })
            .collect()
    }

    pub(super) fn open_admin(&mut self) -> Task<Message> {
        if !self.admin {
            return Task::none();
        }
        // Settings, Themes and Sources are drawn above the Admin page: step out of them first, or the header lights up Admin while they stay on screen.
        self.settings_open = false;
        self.src_open = false;
        self.th.open = false;
        self.folders = None;
        self.menu = false;
        if self.adm.is_some() {
            self.admin_open = true;
            return Task::none();
        }
        let targets = self.admin_targets();
        if targets.is_empty() {
            return Task::none();
        }
        let (a, t) = admin::Admin::open(targets, self.admin_writes());
        self.adm = Some(Box::new(a));
        self.admin_open = true;
        t.map(Message::Admin)
    }

    fn admin_writes(&self) -> bool {
        self.settings.allow_server_writes && !self.dev
    }

    pub(super) fn admin_msg(&mut self, m: admin::Msg) -> Task<Message> {
        let Some(a) = self.adm.as_deref_mut() else {
            return Task::none();
        };
        let t = a.update(m).map(Message::Admin);
        Task::batch([t, self.drain_admin()])
    }

    /// Admin gets every key while it is open.
    pub(super) fn admin_key(&mut self, k: &Key) -> Task<Message> {
        let Some(a) = self.adm.as_deref_mut() else {
            return Task::none();
        };
        let t = a.key(k).map(Message::Admin);
        Task::batch([t, self.drain_admin()])
    }

    pub(super) fn admin_dialog(&self) -> bool {
        self.adm.as_deref().is_some_and(|a| a.dialog_open())
    }

    fn drain_admin(&mut self) -> Task<Message> {
        let outs = match self.adm.as_deref_mut() {
            Some(a) => std::mem::take(&mut a.outcomes),
            None => return Task::none(),
        };
        let mut tasks = vec![];
        for o in outs {
            match o {
                admin::Outcome::Close => self.close_admin(),
                admin::Outcome::OpenSettings => {
                    self.close_admin();
                    self.settings_open = true;
                    self.settings_sel = 0;
                    self.dd = None;
                    self.set_off = 0.0;
                }
            }
        }
        tasks.push(Task::none());
        Task::batch(tasks)
    }

    // ---- Watch together ----

    pub(super) fn sp_targets(&self) -> Vec<syncplay::Target> {
        if self.dev {
            return vec![];
        }
        self.catalog
            .priority()
            .into_iter()
            .filter_map(|id| {
                let (url, s) = self.sessions.get(&id)?;
                let slot = self.catalog.slot(id).filter(|s| s.status.usable())?;
                let Some(Backend::Jellyfin(source)) = slot.backend.clone() else {
                    return None;
                };
                Some(syncplay::Target {
                    origin: id,
                    label: slot.profile.label.clone(),
                    client: UserClient::from_session(url, s).ok()?,
                    user_name: s.user_name.clone(),
                    source,
                })
            })
            .collect()
    }

    pub(super) fn sp_msg(&mut self, m: syncplay::Msg) -> Task<Message> {
        let t = self.sp.update(m, self.player.as_mut());
        Task::batch([t.map(Message::SyncPlay), self.drain_sp()])
    }

    pub(super) fn sp_key(&mut self, k: &Key) -> Task<Message> {
        let t = self.sp.key(k, self.player.as_mut());
        Task::batch([t.map(Message::SyncPlay), self.drain_sp()])
    }

    pub(super) fn drain_sp(&mut self) -> Task<Message> {
        let outs = std::mem::take(&mut self.sp.outcomes);
        let mut tasks = vec![];
        for o in outs {
            match o {
                syncplay::Outcome::Toast(s) => self.toast(s),
                syncplay::Outcome::OpenSettings => {
                    self.close_admin();
                    self.settings_open = true;
                    self.settings_sel = 0;
                    self.dd = None;
                    self.set_off = 0.0;
                }
                syncplay::Outcome::OpenPlayer {
                    origin,
                    list,
                    idx,
                    start,
                } => {
                    let (Some(b), Some(m)) = (
                        self.catalog.backend(origin).cloned(),
                        list.get(idx).cloned(),
                    ) else {
                        continue;
                    };
                    tasks.push(run(async move { b.play_target(&m).await }, move |r| {
                        Message::GroupPlay(origin, list, idx, start, r)
                    }));
                }
                syncplay::Outcome::Player(t) => tasks.push(t.map(Message::Player)),
                syncplay::Outcome::ClosePlayer => {
                    if let Some(p) = self.player.as_mut() {
                        let t = p.close(false);
                        tasks.push(self.after_player(t));
                    }
                }
            }
        }
        Task::batch(tasks)
    }

    /// A group asked for its item: open the player on it, following the group.
    pub(super) fn group_play(
        &mut self,
        origin: SourceId,
        list: Vec<Media>,
        idx: usize,
        start: f64,
        r: Result<PlayTarget, PlayError>,
    ) -> Task<Message> {
        let (Ok(target), Some(m)) = (r, list.get(idx).cloned()) else {
            self.toast("The group's video couldn't be opened.");
            return Task::none();
        };
        if self.player.is_some() || !matches!(self.phase, Phase::Ready) {
            return Task::none();
        }
        let _ = origin;
        let ctx = self.player_ctx(m, list.clone(), None, target);
        self.menu = false;
        let (mut p, open) = crate::player::Player::open_group(ctx, start);
        p.queue = list;
        p.qi = idx;
        self.player = Some(p);
        open.map(Message::Player)
    }

    /// Keep the group's targets and the master switch current (cheap compares each tick).
    pub(super) fn sync_extras(&mut self) -> Task<Message> {
        let ids: Vec<SourceId> = self.sessions.keys().copied().collect();
        let mut ids = ids;
        ids.sort_by_key(|i| i.0);
        let mut tasks = vec![];
        if ids != self.sp_ids {
            self.sp_ids = ids;
            let targets = self.sp_targets();
            self.sp.set_targets(targets);
        }
        let on = self.settings.allow_server_writes && !self.dev;
        if on != self.sp_writes {
            self.sp_writes = on;
            let t = self.sp.set_writes(on, self.player.as_mut());
            tasks.push(t.map(Message::SyncPlay));
            tasks.push(self.drain_sp());
        }
        if let Some(a) = self.adm.as_deref_mut() {
            a.set_writes(on);
            if a.rejected() {
                let id = a.origin();
                self.close_admin();
                if let Some(id) = id {
                    tasks.push(self.slot_rejected(id));
                }
            }
        }
        Task::batch(tasks)
    }

    /// Stop everything tied to the login (sign-out, switching server or account).
    pub(super) fn extras_shutdown(&mut self) {
        self.sp.shutdown(self.player.as_mut());
        self.remote_shutdown();
        self.adm = None;
        self.devices = None;
        self.queue_open = false;
    }

    // ---- music queue ----

    pub(super) fn queue_msg(&mut self, m: queue::Msg) -> Task<Message> {
        let o = self.queue.update(m);
        self.queue_outcome(o)
    }

    pub(super) fn queue_outcome(&mut self, o: Option<queue::Outcome>) -> Task<Message> {
        match o {
            Some(queue::Outcome::Toast(s)) => self.toast(s),
            Some(queue::Outcome::Close) => self.queue_open = false,
            Some(queue::Outcome::Play) => {
                self.queue_open = false;
                let Some(first) = self.queue.items.get(self.queue.index).cloned() else {
                    return Task::none();
                };
                let rest = self.queue.items[self.queue.index + 1..].to_vec();
                self.restore_q = true;
                return self.start_play(first, rest, None);
            }
            None => {}
        }
        Task::none()
    }

    // ---- devices ----

    pub(super) fn open_devices(&mut self) -> Task<Message> {
        let Some((url, s)) = self.session() else {
            return Task::none();
        };
        let Ok(client) = UserClient::from_session(url, s) else {
            return Task::none();
        };
        let label = self
            .active
            .and_then(|i| self.catalog.slot(i))
            .map_or_else(String::new, |s| s.profile.label.clone());
        let (d, t) = devices::Devices::open(client, label);
        self.devices = Some(d);
        t.map(Message::Devices)
    }

    // ---- keys and drawing ----

    /// Overlays that own the keyboard, topmost first. `None`: not one of ours.
    pub(super) fn extras_key(&mut self, k: &Key) -> Option<Task<Message>> {
        if self.devices.as_ref().is_some_and(|d| d.key(k)) {
            self.devices = None;
            return Some(Task::none());
        }
        if self.devices.is_some() {
            return Some(Task::none());
        }
        if self.sp.is_open() {
            return Some(self.sp_key(k));
        }
        if self.queue_open {
            let o = self.queue.key(k);
            return Some(self.queue_outcome(o));
        }
        None
    }

    pub(super) fn extras_layers<'a>(&'a self, layers: &mut Vec<Element<'a, Message>>) {
        if let Some(p) = self.sp.pill() {
            layers.push(
                iced::widget::container(p.map(Message::SyncPlay))
                    .width(Length::Fill)
                    .height(Length::Shrink)
                    .align_x(Alignment::End)
                    .padding(iced::Padding {
                        top: 13.0,
                        right: 280.0,
                        ..Default::default()
                    })
                    .into(),
            );
        }
        if self.queue_open {
            layers.push(self.queue.view(self.win).map(Message::Queue));
        }
        if let Some(d) = &self.devices {
            layers.push(d.view(self.win).map(Message::Devices));
        }
        if let Some(v) = self.sp.view(self.win) {
            layers.push(v.map(Message::SyncPlay));
        }
    }
}
