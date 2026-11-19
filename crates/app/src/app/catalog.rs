//! The root's catalog: loading the saved sources after the first frame, launch routing, per-source
//! health (one source failing never blocks another), the progressive libraries request and the
//! local scans. Everything slow runs in tasks; nothing here is on the path to the first frame.
use super::message::{Message, ScanMsg};
use super::{App, Phase, View, folders};
use crate::account::{Accounts, picker};
use crate::task::{Handoff, run};
use iced::Task;
use nocturne_core::account::picker::{DefaultChange, Picker, Start};
use nocturne_core::backend::Backend;
use nocturne_core::bridge;
use nocturne_core::catalog::{Catalog, Env, Slot, Status, data_dir};
use nocturne_core::merge::Preferred;
use nocturne_core::profile::{Profile, ProfileKind, Sources, keyring_token};
use nocturne_core::source::{Failed, Media, SourceId};
use nocturne_core::sources::{Fanout, Step};
use nocturne_core::sync::SyncHooks;
use nocturne_local::Progress;
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The transient developer-token source (`NOCTURNE_JF_URL` + `NOCTURNE_JF_TOKEN`): never saved,
/// read-only. Real profiles start at id 1.
pub const DEV: SourceId = SourceId(0);

/// Local scans start this long after process start, i.e. after the first frame.
const SCAN_AFTER: Duration = Duration::from_millis(800);

/// What the load task hands to the root.
pub struct Loaded {
    pub sources: io::Result<Sources>,
    pub catalog: Catalog,
    pub prefs: Preferred,
    /// The sources were just built from the 0.2 servers.toml and keyring login.
    pub migrated: bool,
}

/// A running scan of a local source.
pub struct Scan {
    cancel: Arc<AtomicBool>,
    run_id: u64,
    pub progress: Progress,
}

pub fn app_data_dir() -> std::path::PathBuf {
    data_dir().unwrap_or_else(|| std::env::temp_dir().join("nocturne"))
}

impl App {
    /// Read `sources.toml` (migrating the old files once), the preferred-copy choices, and build the
    /// catalog: keyring token lookups and opening local indexes happen here, on a blocking thread.
    pub(super) fn load_sources(&self) -> Task<Message> {
        let data = self.data.clone();
        run(
            async move {
                tokio::task::spawn_blocking(move || {
                    let existed = Sources::default_path().is_some_and(|p| p.exists());
                    let sources = Sources::load();
                    let migrated =
                        !existed && sources.as_ref().is_ok_and(|s| !s.profiles.is_empty());
                    let profiles = sources
                        .as_ref()
                        .map(|s| s.profiles.clone())
                        .unwrap_or_default();
                    nocturne_core::profile::load_extra_headers(&profiles);
                    let token = |u: &str, id: &str| keyring_token(u, id);
                    let catalog = Catalog::from_profiles(
                        &profiles,
                        &Env {
                            token: &token,
                            data_dir: &data,
                            disk: None,
                        },
                    );
                    let prefs = Preferred::default_path()
                        .map(|p| Preferred::load_from(&p))
                        .unwrap_or_default();
                    Loaded {
                        sources,
                        catalog,
                        prefs,
                        migrated,
                    }
                })
                .await
                .unwrap_or_else(|_| Loaded {
                    sources: Err(io::Error::other("loading the sources failed")),
                    catalog: Catalog::default(),
                    prefs: Preferred::default(),
                    migrated: false,
                })
            },
            |l| Message::Loaded(Handoff::new(l)),
        )
    }

    pub(super) fn on_loaded(&mut self, l: Loaded) -> Task<Message> {
        self.prefs = l.prefs;
        match l.sources {
            Err(e) => {
                // A damaged or newer file is never overwritten: say so instead of starting over.
                self.phase = Phase::Fatal(e.to_string());
                return Task::none();
            }
            Ok(s) => self.sources = s,
        }
        self.catalog = l.catalog;
        self.apply_disk();
        if self.dev {
            return self.connect();
        }
        if let Some(mode) = self.onboarding_due(l.migrated) {
            let t = self.start_onboarding(mode);
            if self.sources.profiles.is_empty() {
                self.phase = Phase::Account;
                return t;
            }
            // Upgraders: the app starts behind the walkthrough.
            return Task::batch([t, self.launch()]);
        }
        self.launch()
    }

    /// The account screens exist from the first need on (they read a small file when created).
    pub(super) fn ensure_accounts(&mut self) -> Task<Message> {
        if self.accounts.is_none() {
            let (a, t) = Accounts::new(false);
            self.accounts = Some(a);
            return t.map(Message::Account);
        }
        Task::none()
    }

    /// Launch with saved sources: the first server follows today's rules (picker, default
    /// profile, restore); the rest of the sources just start.
    fn launch(&mut self) -> Task<Message> {
        let mut tasks = vec![self.ensure_accounts()];
        let first = self
            .sources
            .profiles
            .iter()
            .find(|p| p.enabled && matches!(p.kind, ProfileKind::Jellyfin { .. }))
            .cloned();
        let Some(p) = first else {
            tasks.push(self.ready());
            return Task::batch(tasks);
        };
        let ProfileKind::Jellyfin { url, user_name, .. } = &p.kind else {
            unreachable!()
        };
        let list =
            picker::accounts_of(&nocturne_session::Accounts::load().unwrap_or_default(), url);
        match picker::decide(false, &list) {
            Start::Restore => {
                let signed_out = self
                    .catalog
                    .slot(p.id)
                    .is_some_and(|s| s.status == Status::SignedOut);
                if signed_out && self.catalog.online().next().is_none() {
                    // Nothing to show without signing in: the sign-in form of that server.
                    self.phase = Phase::Account;
                    if let Some(a) = self.accounts.as_mut() {
                        tasks.push(
                            a.sign_in_form(url, Some(user_name.clone()))
                                .map(Message::Account),
                        );
                    }
                } else {
                    self.active = Some(p.id);
                    tasks.push(self.load_session(p.id));
                    tasks.push(self.ready());
                }
            }
            Start::Direct(i) => {
                self.picker = Some(Picker::new(url, list.clone(), None, 1));
                self.phase = Phase::Connecting;
                tasks.push(self.switch_account(list[i].clone(), true, DefaultChange::Keep));
            }
            Start::Picker => {
                self.picker = Some(Picker::new(url, list, None, 1));
                tasks.push(self.show_picker());
            }
        }
        Task::batch(tasks)
    }

    /// Show the shell: Home with every source's libraries, first contact with servers not asked yet,
    /// and (after the first frame) a rescan of the local folders.
    pub(super) fn ready(&mut self) -> Task<Message> {
        self.phase = Phase::Ready;
        let mut tasks = vec![];
        if self.stack.is_empty() {
            tasks.push(self.push(View::Home, None));
        }
        for s in &self.catalog.slots {
            if let (Some(Backend::Jellyfin(j)), Status::Connecting) = (&s.backend, &s.status) {
                let (j, id) = (j.clone(), s.id);
                tasks.push(run(async move { j.ping().await }, move |ok| {
                    Message::Pinged(id, ok)
                }));
            }
        }
        // Servers still labelled by their address are asked for their name, once; the login the
        // write hooks need is read for every server that is not the active one.
        for s in &self.catalog.slots {
            let Some(Backend::Jellyfin(j)) = &s.backend else {
                continue;
            };
            if s.id == DEV {
                continue;
            }
            if let ProfileKind::Jellyfin { url, .. } = &s.profile.kind
                && s.profile.label == nocturne_core::profile::label_for(url)
            {
                let (j, id) = (j.clone(), s.id);
                tasks.push(run(async move { j.server_name().await }, move |n| {
                    Message::ServerName(id, n)
                }));
            }
            if Some(s.id) != self.active && !self.hooks_by.contains_key(&s.id) {
                tasks.push(self.load_hooks(s.id));
            }
        }
        self.launch_scan = true;
        Task::batch(tasks)
    }

    /// A server announced its name: it becomes the label unless the user chose one.
    pub(super) fn server_named(&mut self, id: SourceId, name: Option<String>) {
        let Some(name) = name else { return };
        if self.sources.name_server(id, &name) {
            self.save_sources();
            if let Some(s) = self.catalog.slot_mut(id) {
                s.profile.label = name.trim().to_string();
            }
        }
    }

    /// Read the stored login of a server's profile (keyring) and make it the account in use.
    pub(super) fn load_session(&self, id: SourceId) -> Task<Message> {
        let Some(Profile {
            kind: ProfileKind::Jellyfin { url, user_id, .. },
            ..
        }) = self.sources.get(id).cloned()
        else {
            return Task::none();
        };
        run(
            async move {
                tokio::task::spawn_blocking(move || {
                    nocturne_session::load_account(&url, &user_id)
                        .ok()
                        .flatten()
                        .map(|s| (url, s))
                })
                .await
                .ok()
                .flatten()
            },
            move |r| Message::SessionLoaded(id, r),
        )
    }

    /// Switch the account menu to server `id` with its stored login.
    pub(super) fn session_loaded(
        &mut self,
        id: SourceId,
        r: Option<(String, nocturne_jellyfin_user::Session)>,
    ) -> Task<Message> {
        let Some((url, s)) = r else {
            // Never signed in on this device (or the keyring is locked): sign in again.
            return self.sign_in_again(id);
        };
        if self.sources.get(id).is_none() {
            return Task::none(); // removed meanwhile
        }
        self.hooks = Arc::new(SyncHooks::new(
            nocturne_jellyfin_user::UserClient::from_session(&url, &s).ok(),
            self.settings.allow_server_writes,
            self.settings.sync_progress,
        ));
        self.admin = s.is_admin;
        self.active = Some(id);
        self.sessions.insert(id, (url, s));
        self.my_tag = None;
        self.apply_write_settings();
        self.refresh_profile()
    }

    /// Open the sign-in form for a server slot (its login expired, or it was signed out).
    pub(super) fn sign_in_again(&mut self, id: SourceId) -> Task<Message> {
        let Some(Profile {
            kind: ProfileKind::Jellyfin { url, user_name, .. },
            ..
        }) = self.sources.get(id).cloned()
        else {
            return Task::none();
        };
        self.src_open = false;
        self.th.open = false;
        self.menu = false;
        self.account_back = Some(super::AccountBack::Ready);
        self.phase = Phase::Account;
        let t = self.ensure_accounts();
        match self.accounts.as_mut() {
            Some(a) => Task::batch([
                t,
                a.sign_in_form(&url, Some(user_name)).map(Message::Account),
            ]),
            None => t,
        }
    }

    // ---- fan-out ----

    /// One request per online source, answers arrive as `Message::Fan` for page `pid`.
    pub(super) fn fan<F, Fut>(
        &mut self,
        pid: u64,
        kind: super::message::FanKind,
        ask: F,
    ) -> (Fanout, Task<Message>)
    where
        F: Fn(Backend) -> Fut,
        Fut: Future<Output = Result<Vec<Media>, Failed>> + Send + 'static,
    {
        self.epoch += 1;
        let epoch = self.epoch;
        let slots: Vec<(SourceId, Backend)> = self
            .catalog
            .online()
            .filter(|s| self.filter.admits(s.id))
            .filter_map(|s| Some((s.id, s.backend.clone()?)))
            .collect();
        let fan = Fanout::new(epoch, slots.iter().map(|(i, _)| *i).collect());
        let tasks = slots.into_iter().map(|(id, b)| {
            let fut = ask(b);
            run(async move { fut.await.ok() }, move |r| {
                Message::Fan(pid, kind, epoch, id, r)
            })
        });
        (fan, Task::batch(tasks))
    }

    // ---- health ----

    /// Per server: 401 signs that source out (quietly, others carry on), no answer marks it offline
    /// and retries with backoff, an answer brings it back. Runs on the 200 ms tick.
    pub(super) fn watch_connection(&mut self) -> Task<Message> {
        use nocturne_jellyfin::Health;
        let now = Instant::now();
        let servers: Vec<_> = self
            .catalog
            .slots
            .iter()
            .filter_map(|s| match &s.backend {
                Some(Backend::Jellyfin(j)) => Some((s.id, j.clone(), s.status.clone())),
                _ => None,
            })
            .collect();
        let mut tasks = vec![];
        for (id, src, st) in servers {
            match (src.health(), st) {
                (Health::Unauthorized, _) => tasks.push(self.slot_rejected(id)),
                (Health::Unreachable, Status::Online) => {
                    self.catalog.set_status(id, Status::offline(0, now));
                }
                (Health::Unreachable, Status::Offline { retry_at, .. })
                    if now >= retry_at && self.probing.insert(id) =>
                {
                    tasks.push(run(async move { src.ping().await }, move |ok| {
                        Message::Pinged(id, ok)
                    }));
                }
                (Health::Ok, Status::Offline { .. }) => {
                    self.catalog.set_status(id, Status::Online);
                    let name = self.label_of(id);
                    self.toast(format!("{name} is back online."));
                    tasks.push(self.retry_page());
                }
                _ => {}
            }
        }
        if self.launch_scan && crate::platform::since_start() > SCAN_AFTER {
            self.launch_scan = false;
            let locals: Vec<SourceId> = self
                .catalog
                .online()
                .filter(|s| s.backend.as_ref().is_some_and(Backend::is_local))
                .map(|s| s.id)
                .collect();
            for id in locals {
                tasks.push(self.start_scan(id));
            }
        }
        Task::batch(tasks)
    }

    pub(super) fn pinged(&mut self, id: SourceId, ok: bool) -> Task<Message> {
        self.probing.remove(&id);
        let tries = match self.catalog.slot(id).map(|s| s.status.clone()) {
            Some(Status::Offline { tries, .. }) => tries + 1,
            Some(Status::Connecting) if ok => {
                self.catalog.set_status(id, Status::Online);
                self.retry_server_ui(id);
                return Task::none();
            }
            Some(Status::Connecting) => 0,
            _ => return Task::none(),
        };
        if !ok {
            self.catalog
                .set_status(id, Status::offline(tries, Instant::now()));
        }
        // A good answer shows up as Health::Ok on the next tick.
        Task::none()
    }

    /// The server refused this source's token: it is signed out, nothing else changes.
    pub(super) fn slot_rejected(&mut self, id: SourceId) -> Task<Message> {
        let Some(slot) = self.catalog.slot_mut(id) else {
            return Task::none();
        };
        slot.backend = None;
        slot.status = Status::SignedOut;
        let name = slot.profile.label.clone();
        let was_active = self.active == Some(id);
        let session = self.sessions.remove(&id);
        if was_active {
            self.active = None;
            self.admin = false;
            self.hooks = Arc::new(SyncHooks::new(None, false, false));
        }
        if self.catalog.online().next().is_some() {
            self.toast(format!("{name} needs you to sign in again."));
            return Task::none();
        }
        // Nothing left to show: the same screens as before.
        if self.dev && id == DEV {
            self.phase = Phase::Error(nocturne_core::source::ConnectError::Rejected);
            return Task::none();
        }
        let (url, uid) = match (&self.sources.get(id).map(|p| &p.kind), session) {
            (Some(ProfileKind::Jellyfin { url, user_id, .. }), s) => (
                url.clone(),
                s.map_or_else(|| user_id.clone(), |(_, s)| s.user_id),
            ),
            _ => return Task::none(),
        };
        self.stack.clear();
        self.settings_open = false;
        self.src_open = false;
        self.th.open = false;
        self.menu = false;
        self.phase = Phase::Account;
        let t = self.ensure_accounts();
        if let Some(a) = self.accounts.as_mut() {
            a.point_at(&url);
            a.session_expired(Some(uid));
        }
        t
    }

    pub(super) fn label_of(&self, id: SourceId) -> String {
        self.catalog
            .slot(id)
            .map_or_else(String::new, |s| s.profile.label.clone())
    }

    /// Images are cached on disk once the cache is open (after the first frame).
    pub(super) fn apply_disk(&mut self) {
        let Some(d) = self.disk.clone() else { return };
        for s in &mut self.catalog.slots {
            s.backend = s.backend.take().map(|b| match b {
                Backend::Jellyfin(j) => Backend::Jellyfin(j.with_disk(d.clone())),
                o => o,
            });
        }
    }

    /// A slot rebuilt in the background (keyring read): swap it in, keep the profile order.
    pub(super) fn slot_ready(&mut self, mut slot: Slot) -> Task<Message> {
        if self.sources.get(slot.id).is_none() {
            return Task::none();
        }
        if let (Some(Backend::Jellyfin(j)), Some(d)) = (slot.backend.take(), self.disk.clone()) {
            slot.backend = Some(Backend::Jellyfin(j.with_disk(d)));
        }
        let local = slot.backend.as_ref().is_some_and(Backend::is_local);
        let id = slot.id;
        let connecting = slot.status == Status::Connecting;
        self.catalog.upsert(slot);
        self.catalog.reorder(&self.sources.ids());
        let mut tasks = vec![self.retry_page()];
        if connecting && let Some(Backend::Jellyfin(j)) = self.catalog.backend(id).cloned() {
            tasks.push(run(async move { j.ping().await }, move |ok| {
                Message::Pinged(id, ok)
            }));
        }
        if local {
            tasks.push(self.start_scan(id));
        }
        Task::batch(tasks)
    }

    /// Build slot `id` again from its profile (token from the keyring) without blocking the UI.
    pub(super) fn rebuild_slot(&self, id: SourceId) -> Task<Message> {
        let (Some(p), data) = (self.sources.get(id).cloned(), self.data.clone()) else {
            return Task::none();
        };
        run(
            async move {
                tokio::task::spawn_blocking(move || {
                    nocturne_core::profile::load_extra_headers(std::slice::from_ref(&p));
                    let token = |u: &str, id: &str| keyring_token(u, id);
                    Slot::build(
                        &p,
                        &Env {
                            token: &token,
                            data_dir: &data,
                            disk: None,
                        },
                    )
                })
                .await
                .ok()
            },
            |s| match s {
                Some(s) => Message::SlotReady(Handoff::new(s)),
                None => Message::Noop,
            },
        )
    }

    // ---- local scans ----

    /// Scan a local source's folders on a blocking thread; progress comes back as messages. A scan
    /// that is already running is left alone.
    pub(super) fn start_scan(&mut self, id: SourceId) -> Task<Message> {
        if self.scans.contains_key(&id) {
            return Task::none();
        }
        let (Some(Backend::Local(l)), Some(ProfileKind::Local { roots })) = (
            self.catalog.backend(id).cloned(),
            self.sources.get(id).map(|p| p.kind.clone()),
        ) else {
            return Task::none();
        };
        if roots.is_empty() {
            return Task::none();
        }
        self.scan_gen += 1;
        let run_id = self.scan_gen;
        let cancel = Arc::new(AtomicBool::new(false));
        self.scans.insert(
            id,
            Scan {
                cancel: cancel.clone(),
                run_id,
                progress: Progress { done: 0, total: 0 },
            },
        );
        self.catalog
            .set_status(id, Status::Indexing { done: 0, total: 0 });
        let (tx, rx) = iced::futures::channel::mpsc::unbounded();
        std::thread::Builder::new()
            .name("nocturne-scan".into())
            .spawn(move || {
                let mut last = Instant::now() - Duration::from_secs(1);
                let r = l.scan(&roots, &cancel, |p| {
                    // The scan reports per file; the UI needs a few updates a second.
                    if last.elapsed() >= Duration::from_millis(100) {
                        last = Instant::now();
                        let _ = tx.unbounded_send(ScanMsg::Progress(p));
                    }
                });
                let done = match r {
                    Ok(rep) => ScanMsg::Done {
                        cancelled: rep.cancelled,
                        error: None,
                    },
                    Err(e) => ScanMsg::Done {
                        cancelled: false,
                        error: Some(e.to_string()),
                    },
                };
                let _ = tx.unbounded_send(done);
            })
            .ok();
        Task::run(rx, move |m| Message::Scan(id, run_id, m))
    }

    pub(super) fn scan_msg(&mut self, id: SourceId, run_id: u64, m: ScanMsg) -> Task<Message> {
        let Some(sc) = self.scans.get_mut(&id).filter(|s| s.run_id == run_id) else {
            return Task::none();
        };
        match m {
            ScanMsg::Progress(p) => {
                sc.progress = p;
                if matches!(
                    self.catalog.slot(id).map(|s| &s.status),
                    Some(Status::Indexing { .. })
                ) {
                    self.catalog.set_status(
                        id,
                        Status::Indexing {
                            done: p.done,
                            total: p.total,
                        },
                    );
                }
                Task::none()
            }
            ScanMsg::Done { cancelled, error } => {
                self.scans.remove(&id);
                if matches!(
                    self.catalog.slot(id).map(|s| &s.status),
                    Some(Status::Indexing { .. })
                ) {
                    self.catalog.set_status(id, Status::Online);
                }
                if error.is_some() {
                    self.toast("A folder could not be indexed.");
                } else if cancelled {
                    self.toast("Indexing stopped.");
                }
                let again = self.rescan.remove(&id);
                let mut tasks = vec![self.retry_page(), self.refresh_folders()];
                if again {
                    tasks.push(self.start_scan(id));
                } else if !cancelled && error.is_none() {
                    // Opt-in details for what the scan found (does nothing unless switched on).
                    tasks.push(self.start_online(id));
                }
                Task::batch(tasks)
            }
        }
    }

    /// Stop a scan (the index keeps what was found so far).
    pub(super) fn cancel_scan(&mut self, id: SourceId) {
        if let Some(s) = self.scans.get(&id) {
            s.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Rescan now: a running scan is stopped and started again with the current folder list.
    pub(super) fn rescan_now(&mut self, id: SourceId) -> Task<Message> {
        if self.scans.contains_key(&id) {
            self.cancel_scan(id);
            self.rescan.insert(id);
            return Task::none();
        }
        self.start_scan(id)
    }

    /// The local folders screen's data (counts, folder state, unmatched), read off the UI thread.
    /// Skipped while a scan holds the library: the screen shows the last numbers until it ends.
    pub(super) fn refresh_folders(&mut self) -> Task<Message> {
        let Some(f) = &self.folders else {
            return Task::none();
        };
        let id = f.id;
        if self.scans.contains_key(&id) {
            return Task::none();
        }
        let (Some(Backend::Local(l)), Some(ProfileKind::Local { roots })) = (
            self.catalog.backend(id).cloned(),
            self.sources.get(id).map(|p| p.kind.clone()),
        ) else {
            return Task::none();
        };
        run(
            async move {
                tokio::task::spawn_blocking(move || l.overview(&roots))
                    .await
                    .ok()
            },
            move |o| match o {
                Some(o) => Message::Folder(folders::Msg::Overview(id, o)),
                None => Message::Noop,
            },
        )
    }

    // ---- developer token ----

    /// Add the transient developer source (read only, never saved).
    pub(super) fn dev_source(&mut self, src: nocturne_core::source::Source) {
        let url = self
            .cfg
            .as_ref()
            .map(|c| c.base_url.clone())
            .unwrap_or_default();
        let src = src.with_origin(DEV);
        self.catalog.upsert(Slot {
            id: DEV,
            profile: Profile {
                id: DEV,
                label: "Developer token".into(),
                enabled: true,
                online: Default::default(),
                server_ui: false,
                kind: ProfileKind::Jellyfin {
                    url,
                    user_id: String::new(),
                    user_name: "dev".into(),
                },
            },
            backend: Some(Backend::Jellyfin(src)),
            status: Status::Online,
        });
        self.apply_disk();
    }

    /// First-run: what comes after `finished`.
    pub(super) fn setup_after(&mut self, finished: Step) -> Task<Message> {
        let Some(plan) = self.setup else {
            return Task::none();
        };
        match plan.after(finished) {
            Step::Folders => self.open_folders_setup(),
            Step::Server | Step::Done => {
                self.setup = None;
                Task::none()
            }
        }
    }

    pub(super) fn source(&self) -> Option<nocturne_core::source::Source> {
        match self.catalog.backend(self.active?)? {
            Backend::Jellyfin(s) => Some(s.clone()),
            Backend::Local(_) => None,
        }
    }

    pub(super) fn session(&self) -> Option<&(String, nocturne_jellyfin_user::Session)> {
        self.sessions.get(&self.active?)
    }

    pub(super) fn spawn_forget(&self, url: String, user_id: String) {
        bridge::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                nocturne_core::profile::forget_login(&url, &user_id)
            })
            .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_developer_source_has_an_id_no_saved_profile_can_have() {
        let mut s = Sources::default();
        let id = s.add("x", ProfileKind::Local { roots: vec![] });
        assert_ne!(id, DEV);
        assert!(s.get(DEV).is_none());
    }
}
