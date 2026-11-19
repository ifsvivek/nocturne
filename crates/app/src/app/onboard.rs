//! The first-run walkthrough on the root (hook notes: `crate::onboarding`). It owns the window
//! while it is up; "Add folders" / "Add a server" lend the window to the existing local-folders and
//! sign-in screens (`ob_away`) and come back through `returned`.
use super::message::Message;
use super::{AccountBack, App, Phase};
use crate::onboarding::{self, Mode, Onboarding, Outcome, Return, Server, World};
use iced::Task;
use nocturne_core::catalog::Status;
use nocturne_core::profile::ProfileKind;
use nocturne_core::settings::ONBOARDING;
use nocturne_core::video;

impl App {
    /// What the walkthrough shows of the library so far.
    fn world(&self) -> World {
        let mut w = World {
            writes: self.settings.allow_server_writes,
            sync: self.settings.sync_progress,
            embedded_video: video::choose(
                std::env::var("NOCTURNE_VIDEO").ok().as_deref(),
                crate::platform::can_embed(),
            ) == video::Choice::Embedded,
            ..World::default()
        };
        for p in &self.sources.profiles {
            match &p.kind {
                ProfileKind::Local { roots } => {
                    w.folders.extend(roots.iter().map(|r| {
                        r.file_name()
                            .map_or_else(|| r.display().to_string(), |n| n.to_string_lossy().into())
                    }));
                }
                ProfileKind::Jellyfin { user_name, .. } => {
                    let up = self.catalog.slot(p.id).is_some_and(|s| s.status.usable());
                    if up && p.enabled {
                        w.servers.push(Server {
                            name: p.label.clone(),
                            user: user_name.clone(),
                        });
                    }
                }
            }
        }
        for s in &self.catalog.slots {
            if let Status::Indexing { done, .. } = s.status {
                w.indexing = Some(w.indexing.unwrap_or(0) + done);
            }
        }
        w
    }

    pub(super) fn start_onboarding(&mut self, mode: Mode) -> Task<Message> {
        let t = self.ensure_accounts();
        self.ob = Some(Box::new(Onboarding::new(mode, self.world())));
        self.ob_away = None;
        t
    }

    /// Is the walkthrough what the window shows right now?
    pub(super) fn ob_visible(&self) -> bool {
        self.ob.is_some() && self.ob_away.is_none()
    }

    /// Startup decision (after the sources load). `None`: not due.
    pub(super) fn onboarding_due(&mut self, migrated: bool) -> Option<Mode> {
        if self.dev {
            return None;
        }
        onboarding::due(
            self.settings.onboarding,
            !self.sources.profiles.is_empty(),
            migrated,
        )
    }

    pub(super) fn ob_msg(&mut self, m: onboarding::Msg) -> Task<Message> {
        if let Some(o) = self.ob.as_deref_mut() {
            o.update(m);
        }
        self.drain_ob()
    }

    pub(super) fn ob_key(&mut self, k: &crate::keys::Key) -> Task<Message> {
        if self.ob.as_deref_mut().is_some_and(|o| o.key(k)) {
            return self.drain_ob();
        }
        Task::none()
    }

    /// Each tick: keep the walkthrough's view of the world current and notice a finished trip.
    pub(super) fn sync_ob(&mut self) {
        let (win, world) = (self.win, self.world());
        let Some(o) = self.ob.as_deref_mut() else {
            return;
        };
        o.set_win(win);
        o.set_world(world);
        let back = match self.ob_away {
            Some(Return::Folders) => self.folders.is_none(),
            Some(Return::Server) => matches!(self.phase, Phase::Ready),
            None => false,
        };
        if back && let Some(r) = self.ob_away.take() {
            o.returned(r);
        }
    }

    fn drain_ob(&mut self) -> Task<Message> {
        let outs = match self.ob.as_deref_mut() {
            Some(o) => std::mem::take(&mut o.outcomes),
            None => return Task::none(),
        };
        let mut tasks = vec![];
        for o in outs {
            match o {
                Outcome::AddFolders => {
                    self.ob_away = Some(Return::Folders);
                    if self.sources.profiles.is_empty() {
                        self.phase = Phase::Ready;
                    }
                    tasks.push(self.open_folders(None));
                }
                Outcome::AddServer => {
                    self.ob_away = Some(Return::Server);
                    self.account_back = Some(AccountBack::Ready);
                    self.phase = Phase::Account;
                    tasks.push(self.ensure_accounts());
                    if let Some(a) = self.accounts.as_mut() {
                        a.switch_server();
                    }
                }
                Outcome::SetWrites(on) => {
                    self.settings.allow_server_writes = on;
                    self.settings.save();
                    self.apply_write_settings();
                }
                Outcome::SetSync(on) => {
                    self.settings.sync_progress = on;
                    self.settings.save();
                    self.apply_write_settings();
                }
                Outcome::Finish { add_folders, .. } => {
                    self.settings.onboarding = ONBOARDING;
                    self.settings.save();
                    self.ob = None;
                    self.ob_away = None;
                    if self.sources.profiles.is_empty() {
                        self.phase = Phase::Ready;
                        tasks.push(self.open_sources());
                    } else {
                        self.stack.truncate(1);
                        tasks.push(self.ready());
                        if add_folders {
                            tasks.push(self.open_folders(None));
                        }
                    }
                }
            }
        }
        Task::batch(tasks)
    }

    /// Settings > Run setup again.
    pub(super) fn run_setup_again(&mut self) -> Task<Message> {
        self.settings_open = false;
        self.start_onboarding(Mode::Fresh)
    }
}
