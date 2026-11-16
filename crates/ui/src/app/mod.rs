//! Root: connection state, navigation stack, keyboard and the screens. Port of the GPUI `Root`
//! (`crates/app/src/app.rs`); step 4a covers connect / accounts / header / settings, the other
//! pages render a "Not ported yet" panel.
mod header;
mod help;
pub mod message;
mod preview;
mod settings;
mod welcome;

use crate::account::{self, Accounts, Event as AcctEvent, picker};
use crate::keys::Key;
use crate::task::run;
use crate::theme as t;
use crate::widget::{self, action_button, label, pulse, tex, wordmark};
use iced::widget::image::Handle;
use iced::widget::{Stack, column, container, image, row, space, text};
use iced::{
    Alignment, ContentFit, Element, Length, Padding, Size, Subscription, Task, event, mouse, window,
};
use message::{AcctAct, Message, SwitchFail};
use nocturne_core::account::picker::{DefaultChange, Picker, Start};
use nocturne_core::cache::Cache;
use nocturne_core::dropdown::Dropdown;
use nocturne_core::imgcache::DiskCache;
use nocturne_core::nav;
use nocturne_core::settings::Settings;
use nocturne_core::source::{ConnectError, Media, Source, sniff, user_image_key};
use nocturne_core::sync::{SyncHooks, backoff_secs};
use nocturne_jellyfin_admin::AdminClient;
use nocturne_jellyfin_user::{Session, UserClient};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed,
}

impl Load<Vec<Media>> {
    pub fn items(&self) -> &[Media] {
        match self {
            Load::Ready(v) => v,
            _ => &[],
        }
    }
}

enum Img {
    Loading,
    Ready(Handle),
    /// When it failed: the slot is tried again after [`RETRY_FAILED`].
    Failed(Instant),
}

/// Profile pictures are fetched once at this size (square) for every avatar.
const AVATAR_PX: u32 = 96;
const RETRY_FAILED: Duration = Duration::from_secs(30);
const TOAST: Duration = Duration::from_millis(2500);

/// Slot key of a profile picture in the in-memory image cache.
fn user_pic_key(uid: &str, tag: &str) -> String {
    user_image_key("", uid, tag, AVATAR_PX)
}

/// Ready picture for this user and tag. No tag (none set), still loading, failed, or only an
/// older tag cached: None, and the avatar shows the initial.
fn ready_pic(images: &Cache<String, Img>, uid: &str, tag: Option<&str>) -> Option<Handle> {
    match images.get(&user_pic_key(uid, tag?)) {
        Some(Img::Ready(h)) => Some(h.clone()),
        _ => None,
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum View {
    Home,
    Grid,
    Detail,
}

/// One entry of the history stack. Later steps add the per-page state of Home, library and
/// detail pages here.
pub struct Page {
    pub pid: u64,
    pub view: View,
    pub media: Option<Media>,
    /// Home only: the libraries (header nav links).
    pub libs: Load<Vec<Media>>,
}

/// Library-like destinations that are not libraries (header links after the libraries).
pub const GLOBALS: [(&str, &str); 3] = [
    ("collections", "Collections"),
    ("playlists", "Playlists"),
    ("favourites", "Favourites"),
];

enum Phase {
    /// Login mode: the account screens (connect, sign in, restore) own the window.
    Account,
    /// "Who's watching?": several remembered accounts and no default (or Switch profile).
    Picker,
    Connecting,
    Error(ConnectError),
    Ready,
}

/// Reconnect state while the server is unreachable.
struct Offline {
    tries: u32,
    next: Instant,
    probing: bool,
}

pub struct App {
    cfg: Option<nocturne_jellyfin::Config>,
    source: Option<Source>,
    phase: Phase,
    stack: Vec<Page>,
    /// Pages left by Back, restored by Forward (cleared by any new navigation).
    fwd: Vec<Page>,
    next_pid: u64,
    images: Cache<String, Img>,
    grain: Handle,
    dots: Handle,
    glow: Handle,
    win: Size,
    now: Instant,
    note: Option<(String, Instant)>,
    /// Developer-token mode (`NOCTURNE_JF_TOKEN`): no login, read-only.
    dev: bool,
    /// The token / account has admin rights (an `Admin` entry follows the libraries).
    admin: bool,
    admin_open: bool,
    accounts: Option<Accounts>,
    session: Option<(String, Session)>,
    hooks: Arc<SyncHooks>,
    settings: Settings,
    settings_open: bool,
    settings_sel: usize,
    /// Open Settings drop-down.
    dd: Option<Dropdown>,
    /// Settings page scroll: absolute offset and viewport height.
    set_off: f32,
    set_h: f32,
    menu: bool,
    menu_sel: usize,
    other_accounts: Vec<nocturne_session::Account>,
    picker: Option<Picker>,
    /// Token-less source that only fetches the profile pictures shown by the picker.
    anon_src: Option<Source>,
    /// `PrimaryImageTag` of the signed-in user (None: no profile picture, or not known yet).
    my_tag: Option<String>,
    disk: Option<Arc<DiskCache>>,
    offline: Option<Offline>,
    help_open: bool,
    /// First run (no source yet): the welcome choice, until one is made.
    welcome: Option<welcome::Welcome>,
    /// Debug-only preview of the player loading overlay.
    preview: Option<preview::Preview>,
}

impl App {
    pub fn boot(cfg: Option<nocturne_jellyfin::Config>) -> (Self, Task<Message>) {
        crate::timing::mark("boot");
        let settings = Settings::load();
        crate::motion::set_reduced(
            settings
                .reduce_motion
                .reduced(crate::platform::system_reduces_motion()),
        );
        // Opening the image cache scans its directory; that happens after the first frame.
        let cache_max = u64::from(settings.image_cache_mb) * 1024 * 1024;
        let open_disk = run(
            async move {
                tokio::task::spawn_blocking(move || {
                    DiskCache::default_dir().map(|d| Arc::new(DiskCache::open(d, cache_max)))
                })
                .await
                .ok()
                .flatten()
            },
            Message::DiskReady,
        );
        let welcome = (cfg.is_none()
            && nocturne_session::Servers::load().is_ok_and(|s| s.servers.is_empty()))
        .then(welcome::Welcome::new);
        let mut me = Self {
            dev: cfg.is_some(),
            cfg,
            source: None,
            phase: Phase::Connecting,
            stack: vec![],
            fwd: vec![],
            next_pid: 0,
            images: Cache::new(600),
            grain: tex::grain(),
            dots: tex::dots(),
            glow: tex::glow(),
            win: Size::new(1280.0, 800.0),
            now: Instant::now(),
            note: None,
            admin: false,
            admin_open: false,
            accounts: None,
            session: None,
            hooks: Arc::new(SyncHooks::new(None, false, false)),
            settings,
            settings_open: false,
            settings_sel: 0,
            dd: None,
            set_off: 0.0,
            set_h: 600.0,
            menu: false,
            menu_sel: 0,
            other_accounts: vec![],
            picker: None,
            anon_src: None,
            my_tag: None,
            disk: None,
            offline: None,
            help_open: false,
            welcome,
            preview: preview::Preview::from_env(),
        };
        let size = window::oldest()
            .and_then(window::size)
            .map(Message::Resized);
        let task = if me.dev {
            me.connect()
        } else {
            me.phase = Phase::Account;
            // Several remembered accounts on the last used server: ask who is watching (or open
            // the default profile) instead of restoring the last token.
            let last = nocturne_session::Servers::load()
                .ok()
                .and_then(|s| s.last_used);
            let list = last
                .as_deref()
                .map(|u| {
                    picker::accounts_of(&nocturne_session::Accounts::load().unwrap_or_default(), u)
                })
                .unwrap_or_default();
            let start = picker::decide(false, &list);
            let (acct, t0) = Accounts::new(matches!(start, Start::Restore));
            me.accounts = Some(acct);
            let mut tasks = vec![t0.map(Message::Account)];
            if let (Some(server), true) = (last, !matches!(start, Start::Restore)) {
                me.picker = Some(Picker::new(&server, list.clone(), None, 1));
                match start {
                    Start::Direct(i) => {
                        me.phase = Phase::Connecting;
                        tasks.push(me.switch_account(list[i].clone(), true, DefaultChange::Keep));
                    }
                    _ => tasks.push(me.show_picker()),
                }
            }
            Task::batch(tasks)
        };
        crate::timing::mark("boot done");
        (me, Task::batch([size, task, open_disk]))
    }

    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    pub fn title(&self) -> String {
        "NOCTURNE".into()
    }

    // ---- subscriptions ----

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            iced::keyboard::listen().filter_map(|e| crate::keys::from_event(&e).map(Message::Key)),
            iced::time::every(Duration::from_millis(200)).map(Message::Tick),
            window::resize_events().map(|(_, s)| Message::Resized(s)),
            window::close_requests().map(Message::CloseRequested),
            event::listen_with(|e, _, _| match e {
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => {
                    Some(Message::Back)
                }
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => {
                    Some(Message::Forward)
                }
                _ => None,
            }),
        ];
        // Frames only while something pulses (and once, to time the first painted frame).
        if self.animating() || crate::timing::first_frame_pending() {
            subs.push(window::frames().map(Message::Frame));
        }
        Subscription::batch(subs)
    }

    fn animating(&self) -> bool {
        self.preview
            .as_ref()
            .is_some_and(|p| p.animating(crate::motion::reduced()))
            || matches!(self.phase, Phase::Connecting)
            || (matches!(self.phase, Phase::Account)
                && self.accounts.as_ref().is_some_and(|a| a.animating()))
    }

    // ---- update ----

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Key(k) => self.on_key(k),
            Message::Tick(now) => {
                self.now = now;
                self.tick()
            }
            Message::Frame(now) => {
                self.now = now;
                crate::timing::first_frame();
                if let Some(p) = self.preview.as_mut() {
                    p.tick(Instant::now());
                }
                Task::none()
            }
            Message::Resized(s) => {
                self.win = s;
                if let Some(p) = &mut self.picker {
                    p.cols = picker::columns(s.width);
                }
                Task::none()
            }
            Message::CloseRequested(_) => {
                // Settings are saved on every change; one last save covers anything pending.
                self.settings.save();
                crate::platform::remove_art_file();
                iced::exit()
            }
            Message::Back => {
                self.back();
                Task::none()
            }
            Message::Forward => {
                self.forward();
                Task::none()
            }
            Message::Connected(r) => self.connected(r),
            Message::AdminProbed(yes) => {
                self.admin = yes;
                self.apply_write_settings();
                Task::none()
            }
            Message::Retry => self.connect(),
            Message::RetryNow => {
                if let Some(o) = self.offline.as_mut() {
                    o.next = Instant::now();
                }
                Task::none()
            }
            Message::Account(m) => match self.accounts.as_mut() {
                Some(a) => {
                    let t = a.update(m).map(Message::Account);
                    Task::batch([t, self.drain_account_events()])
                }
                None => Task::none(),
            },
            Message::PickerAct(a) => self.picker_act(a),
            Message::Switched {
                acct,
                via_picker,
                default,
                r,
            } => self.switched(acct, via_picker, default, r),
            Message::Libraries(pid, r) => {
                if let Some(p) = self.page_mut(pid) {
                    p.libs = r.map_or(Load::Failed, Load::Ready);
                    crate::timing::shell_ready();
                }
                Task::none()
            }
            Message::Profile(uid, p) => {
                let same = self.session.as_ref().is_some_and(|(_, s)| s.user_id == uid);
                // Switched away meanwhile, or the server did not answer: keep what is shown.
                if let (true, Some(tag)) = (same, p) {
                    self.my_tag = tag;
                    if let Some((url, _)) = &self.session {
                        nocturne_session::remember_image_tag(url, &uid, self.my_tag.clone());
                    }
                    let tag = self.my_tag.clone();
                    return self.load_user_pic(&uid, tag);
                }
                Task::none()
            }
            Message::UserPic(key, h) => {
                let state = match h {
                    Some(h) => Img::Ready(h),
                    None => Img::Failed(Instant::now()),
                };
                self.images.insert(key, state);
                Task::none()
            }
            Message::Pinged => {
                if let Some(o) = self.offline.as_mut() {
                    o.probing = false;
                    o.tries += 1;
                    o.next = Instant::now() + Duration::from_secs(backoff_secs(o.tries));
                }
                // Success shows up as Health::Ok on the next tick.
                Task::none()
            }
            Message::GoHome => {
                self.close_admin();
                self.stack.truncate(1);
                Task::none()
            }
            Message::OpenLibrary(i) => {
                let lib = self
                    .stack
                    .first()
                    .and_then(|h| h.libs.items().get(i).cloned());
                self.close_admin();
                if let Some(m) = lib {
                    self.open_library(m);
                }
                Task::none()
            }
            Message::OpenGlobal(i) => {
                self.close_admin();
                self.open_global(i);
                Task::none()
            }
            Message::OpenAdmin => {
                self.open_admin();
                Task::none()
            }
            Message::OpenSearch => self.open_search(),
            Message::ToggleMenu => {
                self.menu = !self.menu;
                self.menu_sel = 0;
                self.refresh_accounts()
            }
            Message::CloseMenu => {
                self.menu = false;
                Task::none()
            }
            Message::MenuAct(a) => self.menu_act(a),
            Message::Crumb(i) => {
                while self.stack.len() > i + 1 {
                    self.back();
                }
                Task::none()
            }
            Message::SetRow(i) => {
                self.settings_sel = i;
                if self.dd.is_some() {
                    self.dd = None; // clicking out of an open menu only closes it
                    return Task::none();
                }
                self.open_setting(i)
            }
            Message::SetChoose(row, idx) => {
                self.dd = None;
                self.choose_setting(row, idx);
                Task::none()
            }
            Message::SetClose => {
                self.dd = None;
                Task::none()
            }
            Message::SetScrolled(off, h) => {
                (self.set_off, self.set_h) = (off, h);
                Task::none()
            }
            Message::CloseOverlays => {
                self.help_open = false;
                Task::none()
            }
            Message::PageRetry => self.retry_page(),
            Message::Welcome(m) => {
                if let Some(c) = self.welcome.as_mut().and_then(|w| w.update(m)) {
                    self.first_run(c);
                }
                Task::none()
            }
            Message::DiskReady(d) => {
                self.disk = d;
                if let (Some(d), Some(s)) = (self.disk.clone(), self.source.take()) {
                    self.source = Some(s.with_disk(d));
                }
                Task::none()
            }
            Message::Noop => Task::none(),
        }
    }

    /// 5 times a second: quit request, toast expiry, connection watch.
    fn tick(&mut self) -> Task<Message> {
        if crate::platform::term_requested() {
            self.settings.save();
            crate::platform::remove_art_file();
            return iced::exit();
        }
        if self
            .note
            .as_ref()
            .is_some_and(|(_, until)| self.now >= *until)
        {
            self.note = None;
        }
        self.watch_connection()
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.note = Some((msg.into(), Instant::now() + TOAST));
    }

    // ---- connect ----

    fn connect(&mut self) -> Task<Message> {
        self.phase = Phase::Connecting;
        run(Source::connect(self.cfg.clone()), Message::Connected)
    }

    fn connected(&mut self, r: Result<Source, ConnectError>) -> Task<Message> {
        match r {
            Ok(s) => {
                self.source = Some(match self.disk.clone() {
                    Some(d) => s.with_disk(d),
                    None => s,
                });
                self.phase = Phase::Ready;
                let push = self.push(View::Home, None);
                let cfg = self.cfg.clone();
                let probe = run(
                    async move {
                        let c = AdminClient::new(cfg?).ok()?;
                        nocturne_core::admin::state::admin_visible(&c.system_info().await)
                            .then_some(())
                    },
                    |a| Message::AdminProbed(a.is_some()),
                );
                Task::batch([push, probe])
            }
            Err(e) => {
                self.phase = Phase::Error(e);
                Task::none()
            }
        }
    }

    // ---- accounts and sync ----

    fn drain_account_events(&mut self) -> Task<Message> {
        let evs: Vec<AcctEvent> = self
            .accounts
            .as_mut()
            .map(|a| std::mem::take(&mut a.events))
            .unwrap_or_default();
        let mut tasks = vec![];
        for ev in evs {
            match ev {
                AcctEvent::SignedIn { url, session } => tasks.push(self.activate(url, session)),
                AcctEvent::Notice(n) => self.toast(n),
                AcctEvent::Reload => {
                    tasks.push(self.refresh_profile());
                    tasks.push(self.retry_page());
                }
                AcctEvent::BackToPicker => tasks.push(self.show_picker()),
            }
        }
        Task::batch(tasks)
    }

    /// A login is ready (typed, Quick Connect or restored): build the source and show Home.
    fn activate(&mut self, url: String, s: Session) -> Task<Message> {
        let cfg = nocturne_jellyfin::Config {
            base_url: url.clone(),
            token: s.access_token.clone(),
        };
        let Ok(src) = Source::with_user(cfg, &s.user_id) else {
            self.toast("This server address can't be used.");
            return Task::none();
        };
        self.hooks = Arc::new(SyncHooks::new(
            UserClient::from_session(&url, &s).ok(),
            self.settings.allow_server_writes,
            self.settings.sync_progress,
        ));
        self.source = Some(match self.disk.clone() {
            Some(d) => src.with_disk(d),
            None => src,
        });
        self.images = Cache::new(600);
        self.stack.clear();
        self.fwd.clear();
        self.admin_open = false;
        // The admin section follows the account's own rights.
        self.admin = s.is_admin;
        self.apply_write_settings();
        self.session = Some((url, s));
        self.my_tag = None;
        let profile = self.refresh_profile();
        self.phase = Phase::Ready;
        let home = self.push(View::Home, None);
        if self.accounts.as_ref().is_some_and(|a| a.keyring_down()) {
            self.toast(account::KEYRING_NOTE);
        }
        Task::batch([profile, home])
    }

    /// Read the own profile picture tag (`GET /Users/Me`), then the picture itself. Runs on
    /// sign-in, account switch and after the profile image is set or removed.
    fn refresh_profile(&mut self) -> Task<Message> {
        let Some((url, s)) = self.session.as_ref() else {
            return Task::none();
        };
        let (Ok(c), uid) = (UserClient::from_session(url, s), s.user_id.clone()) else {
            return Task::none();
        };
        run(
            async move { c.profile().await.ok().map(|p| p.primary_image_tag) },
            move |p| Message::Profile(uid, p),
        )
    }

    /// Start fetching a user's profile picture (once per tag; a failure is retried later).
    fn load_user_pic(&mut self, uid: &str, tag: Option<String>) -> Task<Message> {
        let (Some(tag), Some(src)) = (tag, self.source.clone().or_else(|| self.anon_src.clone()))
        else {
            return Task::none();
        };
        let key = user_pic_key(uid, &tag);
        match self.images.get(&key) {
            Some(Img::Ready(_) | Img::Loading) => return Task::none(),
            Some(Img::Failed(at)) if at.elapsed() < RETRY_FAILED => return Task::none(),
            _ => {}
        }
        self.images.insert(key.clone(), Img::Loading);
        let uid = uid.to_string();
        run(
            async move { src.user_image(&uid, &tag, AVATAR_PX).await },
            move |r| {
                let h = r
                    .ok()
                    .and_then(|b| sniff(&b).map(|_| Handle::from_bytes(b)));
                Message::UserPic(key, h)
            },
        )
    }

    fn user_pic(&self, uid: &str, tag: Option<&str>) -> Option<Handle> {
        ready_pic(&self.images, uid, tag)
    }

    fn my_pic(&self) -> Option<Handle> {
        let uid = &self.session.as_ref()?.1.user_id;
        self.user_pic(uid, self.my_tag.as_deref())
    }

    /// Back to the account screens: drop everything tied to the login.
    fn leave_session(&mut self) {
        self.source = None;
        self.stack.clear();
        self.admin = false;
        self.admin_open = false;
        self.settings_open = false;
        self.menu = false;
        self.session = None;
        self.my_tag = None;
        self.hooks = Arc::new(SyncHooks::new(None, false, false));
        self.phase = Phase::Account;
    }

    fn sign_out(&mut self) {
        let client = self
            .session
            .as_ref()
            .and_then(|(u, s)| UserClient::from_session(u, s).ok());
        let uid = self.session.as_ref().map(|(_, s)| s.user_id.clone());
        self.leave_session();
        if let Some(a) = self.accounts.as_mut() {
            a.sign_out(client, uid);
        }
    }

    fn switch_server(&mut self) {
        self.leave_session();
        if let Some(a) = self.accounts.as_mut() {
            a.switch_server();
        }
    }

    fn menu_items(&self) -> Vec<(String, AcctAct)> {
        let mut v = vec![("Settings".to_string(), AcctAct::Settings)];
        if !self.dev {
            let mut add = |l: &str, a| v.push((l.to_string(), a));
            add("Change password", AcctAct::Password);
            add("Profile image", AcctAct::ProfileImage);
            add("Remove profile image", AcctAct::RemoveImage);
            add("Display language", AcctAct::Language);
            add("Authorize a device", AcctAct::Authorize);
            add("Watch together", AcctAct::Together);
            add("Your devices", AcctAct::Devices);
            for (i, a) in self.other_accounts.iter().enumerate() {
                v.push((format!("Switch to {}", a.user_name), AcctAct::SwitchTo(i)));
            }
            if !self.other_accounts.is_empty() {
                v.push(("Switch profile".to_string(), AcctAct::SwitchProfile));
            }
            v.push(("Add another account".to_string(), AcctAct::AddAccount));
            v.push(("Switch server".to_string(), AcctAct::Switch));
            v.push(("Sign out".to_string(), AcctAct::SignOut));
        }
        v
    }

    /// Remembered accounts of this server other than the one in use (names only; a tiny file).
    fn refresh_accounts(&mut self) -> Task<Message> {
        self.other_accounts = match &self.session {
            Some((url, s)) => nocturne_session::Accounts::load()
                .unwrap_or_default()
                .for_server(url)
                .into_iter()
                .filter(|a| a.user_id != s.user_id)
                .cloned()
                .collect(),
            None => vec![],
        };
        // Their pictures come from the tags remembered with each account.
        let pics: Vec<_> = self
            .other_accounts
            .iter()
            .map(|a| (a.user_id.clone(), a.image_tag.clone()))
            .collect();
        Task::batch(
            pics.into_iter()
                .map(|(uid, tag)| self.load_user_pic(&uid, tag)),
        )
    }

    /// Sign in as a remembered account: its token comes from the keyring and is checked with one
    /// read; no password is asked and nothing is written to the server. A dead token from the menu
    /// removes that account and says so; from the picker it opens the sign-in form with the name
    /// typed. `default` is what choosing in the picker does to the stored default profile.
    fn switch_account(
        &mut self,
        acct: nocturne_session::Account,
        via_picker: bool,
        default: DefaultChange,
    ) -> Task<Message> {
        if matches!(self.phase, Phase::Ready) {
            self.toast("Switching account.");
        }
        if let Some(p) = &mut self.picker {
            p.busy = true;
        }
        let a2 = acct.clone();
        run(
            async move {
                let (url, id) = (a2.server.clone(), a2.user_id.clone());
                let stored =
                    tokio::task::spawn_blocking(move || nocturne_session::load_account(&url, &id))
                        .await;
                let Ok(Ok(Some(s))) = stored else {
                    return Err(SwitchFail::Gone);
                };
                let c = UserClient::from_session(&a2.server, &s).map_err(|_| SwitchFail::Gone)?;
                match c.me(s).await {
                    Ok(s) => Ok(s),
                    Err(nocturne_jellyfin_user::Error::Status { status: 401, .. }) => {
                        Err(SwitchFail::Gone)
                    }
                    Err(_) => Err(SwitchFail::Offline),
                }
            },
            move |r| Message::Switched {
                acct,
                via_picker,
                default,
                r,
            },
        )
    }

    fn switched(
        &mut self,
        acct: nocturne_session::Account,
        via_picker: bool,
        default: DefaultChange,
        r: Result<Session, SwitchFail>,
    ) -> Task<Message> {
        match r {
            Ok(s) => {
                self.leave_session();
                self.picker = None;
                self.anon_src = None;
                let (u, s2) = (acct.server.clone(), s.clone());
                // Make it the active login (keyring) so a relaunch restores this account.
                nocturne_core::bridge::spawn(async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = nocturne_session::remember_login(&u, &s2);
                        match default {
                            DefaultChange::Set => {
                                nocturne_session::remember_default(&u, Some(&s2.user_id))
                            }
                            DefaultChange::Clear => nocturne_session::remember_default(&u, None),
                            DefaultChange::Keep => {}
                        }
                    })
                    .await;
                });
                self.activate(acct.server.clone(), s)
            }
            Err(SwitchFail::Gone) if via_picker => {
                self.phase = Phase::Account;
                match self.accounts.as_mut() {
                    Some(a) => a
                        .sign_in_form(&acct.server, Some(acct.user_name.clone()))
                        .map(Message::Account),
                    None => Task::none(),
                }
            }
            Err(SwitchFail::Gone) => {
                let (u, id) = (acct.server.clone(), acct.user_id.clone());
                nocturne_core::bridge::spawn(async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        nocturne_session::forget_account(&u, &id)
                    })
                    .await;
                });
                self.toast(
                    "That account's sign-in has expired. Add it again with Add another account.",
                );
                Task::none()
            }
            Err(SwitchFail::Offline) => {
                self.toast("Couldn't reach the server. Try again.");
                if self.picker.is_some() {
                    return self.show_picker(); // also covers a failed launch with a default profile
                }
                Task::none()
            }
        }
    }

    /// Show "Who's watching?" for `picker` and fetch the pictures the server has for its profiles.
    fn show_picker(&mut self) -> Task<Message> {
        let Some(p) = &mut self.picker else {
            return Task::none();
        };
        p.busy = false;
        p.cols = picker::columns(self.win.width);
        let server = p.server.clone();
        let pics: Vec<_> = p
            .accounts
            .iter()
            .map(|a| (a.user_id.clone(), a.image_tag.clone()))
            .collect();
        if self.anon_src.is_none() {
            let cfg = nocturne_jellyfin::Config {
                base_url: server,
                token: String::new(),
            };
            self.anon_src = Source::with_user(cfg, "")
                .ok()
                .map(|s| match self.disk.clone() {
                    Some(d) => s.with_disk(d),
                    None => s,
                });
        }
        self.phase = Phase::Picker;
        Task::batch(
            pics.into_iter()
                .map(|(uid, tag)| self.load_user_pic(&uid, tag)),
        )
    }

    /// Menu "Switch profile": leave the session (the login stays remembered) and ask who is watching.
    fn switch_profile(&mut self) -> Task<Message> {
        let Some((url, s)) = self.session.clone() else {
            return Task::none();
        };
        let list = picker::accounts_of(
            &nocturne_session::Accounts::load().unwrap_or_default(),
            &url,
        );
        if list.len() < 2 {
            return Task::none();
        }
        self.leave_session();
        self.picker = Some(Picker::new(&url, list, Some(&s.user_id), 1));
        self.show_picker()
    }

    fn picker_act(&mut self, act: picker::Act) -> Task<Message> {
        use picker::Act;
        let Some(p) = &mut self.picker else {
            return Task::none();
        };
        match act {
            Act::Stay => {}
            Act::Toggle if !p.busy => p.toggle(),
            Act::Toggle => {}
            Act::Add if !p.busy => {
                let server = p.server.clone();
                self.phase = Phase::Account;
                return match self.accounts.as_mut() {
                    Some(a) => a.sign_in_form(&server, None).map(Message::Account),
                    None => Task::none(),
                };
            }
            Act::Add => {}
            Act::Pick(i) => {
                if p.busy {
                    return Task::none();
                }
                p.sel = i;
                let (acct, change) = (p.accounts[i].clone(), p.default_change(i));
                return self.switch_account(acct, true, change);
            }
        }
        Task::none()
    }

    fn picker_key(&mut self, key: &str) -> Task<Message> {
        let cols = picker::columns(self.win.width);
        let Some(p) = &mut self.picker else {
            return Task::none();
        };
        p.cols = cols;
        let act = p.key(key);
        self.picker_act(act)
    }

    /// Shortcut help sits above everything: drop open menus first.
    fn close_menus(&mut self) {
        self.menu = false;
    }

    fn client(&self) -> Option<UserClient> {
        self.session
            .as_ref()
            .and_then(|(u, s)| UserClient::from_session(u, s).ok())
    }

    fn menu_act(&mut self, act: AcctAct) -> Task<Message> {
        self.menu = false;
        match act {
            AcctAct::Settings => {
                self.close_admin();
                self.refresh_profiles();
                self.settings_open = true;
                self.settings_sel = 0;
                self.dd = None;
                self.set_off = 0.0;
            }
            // TODO(4e/4c): devices overlay, Watch together, music queue.
            AcctAct::Devices | AcctAct::Together | AcctAct::PlayQueue | AcctAct::ClearQueue => {
                self.toast("Not ported yet.");
            }
            AcctAct::Authorize
            | AcctAct::Password
            | AcctAct::ProfileImage
            | AcctAct::RemoveImage
            | AcctAct::Language
                if !self.hooks.can_write() =>
            {
                self.toast(self.writes_off());
            }
            AcctAct::Language => {
                let (Some(c), true) = (self.client(), self.accounts.is_some()) else {
                    return Task::none();
                };
                // Show what is set now, then let the user change it.
                let cc = c.clone();
                return run(
                    async move { cc.display_language().await.ok().flatten() },
                    move |cur| {
                        Message::Account(account::Msg::OpenLanguage(c, cur.unwrap_or_default()))
                    },
                );
            }
            AcctAct::ProfileImage
            | AcctAct::RemoveImage
            | AcctAct::Password
            | AcctAct::Authorize => {
                if let (Some(c), Some(a)) = (self.client(), self.accounts.as_mut()) {
                    match act {
                        AcctAct::ProfileImage => a.open_profile_image(c),
                        AcctAct::RemoveImage => a.open_remove_image(c),
                        AcctAct::Authorize => a.open_authorize(c),
                        _ => a.open_password(c),
                    }
                }
            }
            AcctAct::SwitchTo(i) => {
                if let Some(acct) = self.other_accounts.get(i).cloned() {
                    return self.switch_account(acct, false, DefaultChange::Keep);
                }
            }
            AcctAct::SwitchProfile => return self.switch_profile(),
            AcctAct::AddAccount => {
                self.leave_session();
                if let Some(a) = self.accounts.as_mut() {
                    a.add_account();
                }
            }
            AcctAct::Switch => self.switch_server(),
            AcctAct::SignOut => self.sign_out(),
        }
        Task::none()
    }

    fn writes_off(&self) -> &'static str {
        if self.dev {
            "Developer token mode is read-only."
        } else {
            nocturne_core::sync::NEED_WRITES
        }
    }

    /// Push the master and sync switches to the hooks.
    fn apply_write_settings(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.hooks
            .master
            .store(self.settings.allow_server_writes, Relaxed);
        self.hooks.sync.store(self.settings.sync_progress, Relaxed);
        // TODO(4e): push `can_write()` to the admin section; 4d: SyncPlay writes.
    }

    // ---- connection watch ----

    /// 401 mid-session goes back to sign-in; no answer shows the banner and reconnects with backoff.
    fn watch_connection(&mut self) -> Task<Message> {
        let Some(src) = self.source.clone() else {
            return Task::none();
        };
        match src.health() {
            nocturne_jellyfin::Health::Unauthorized => self.session_rejected(),
            nocturne_jellyfin::Health::Unreachable => {
                let off = self.offline.get_or_insert_with(|| Offline {
                    tries: 0,
                    next: Instant::now() + Duration::from_secs(backoff_secs(0)),
                    probing: false,
                });
                if off.probing || Instant::now() < off.next {
                    return Task::none();
                }
                off.probing = true;
                run(async move { src.ping().await }, |_| Message::Pinged)
            }
            nocturne_jellyfin::Health::Ok => {
                if self.offline.take().is_some() {
                    self.toast("Back online.");
                    return self.retry_page();
                }
                Task::none()
            }
        }
    }

    /// The token is no longer valid: drop the session and say so calmly.
    fn session_rejected(&mut self) -> Task<Message> {
        self.offline = None;
        if self.session.is_none() {
            // Developer-token mode has no login to return to.
            self.source = None;
            self.phase = Phase::Error(ConnectError::Rejected);
            return Task::none();
        }
        let uid = self.session.as_ref().map(|(_, s)| s.user_id.clone());
        self.leave_session();
        if let Some(a) = self.accounts.as_mut() {
            a.session_expired(uid);
        }
        Task::none()
    }

    /// Load the current page again (after a failure or a reconnect).
    fn retry_page(&mut self) -> Task<Message> {
        match self.stack.last() {
            Some(p) if p.view == View::Home => self.load_home(p.pid),
            // TODO(4b/4c): grid and detail reloads.
            _ => Task::none(),
        }
    }

    // ---- pages ----

    fn page_mut(&mut self, pid: u64) -> Option<&mut Page> {
        self.stack.iter_mut().find(|p| p.pid == pid)
    }

    /// A new page on top of the stack; whatever Back had parked is forgotten.
    fn push_page(&mut self, view: View, media: Option<Media>) -> u64 {
        self.next_pid += 1;
        let pid = self.next_pid;
        self.fwd.clear();
        self.stack.push(Page {
            pid,
            view,
            media,
            libs: Load::Loading,
        });
        pid
    }

    fn push(&mut self, view: View, media: Option<Media>) -> Task<Message> {
        if self.source.is_none() {
            return Task::none();
        }
        let pid = self.push_page(view, media);
        if view == View::Home {
            return self.load_home(pid);
        }
        Task::none()
    }

    /// Home 4a: only the libraries (header links). Rows, hero and resume arrive with step 4b.
    fn load_home(&mut self, pid: u64) -> Task<Message> {
        let Some(src) = self.source.clone() else {
            return Task::none();
        };
        if let Some(p) = self.page_mut(pid) {
            p.libs = Load::Loading;
        }
        run(async move { src.libraries().await.ok() }, move |r| {
            Message::Libraries(pid, r)
        })
    }

    /// Library tabs always replace whatever is above Home.
    fn open_library(&mut self, m: Media) {
        self.stack.truncate(1);
        self.push_page(View::Grid, Some(m));
    }

    fn open_global(&mut self, i: usize) {
        use nocturne_core::source::Kind;
        let Some((key, title)) = GLOBALS.get(i) else {
            return;
        };
        self.stack.truncate(1);
        self.fwd.clear();
        let media = Media {
            id: (*key).into(),
            title: (*title).into(),
            kind: Kind::Library,
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        };
        self.push_page(View::Grid, Some(media));
    }

    fn open_admin(&mut self) {
        if self.admin {
            self.admin_open = true;
        }
    }

    /// Leave Admin (if open) before navigating elsewhere.
    fn close_admin(&mut self) {
        self.settings_open = false;
        self.menu = false;
        self.admin_open = false;
    }

    fn open_search(&mut self) -> Task<Message> {
        // TODO(4b): search overlay.
        self.toast("Not ported yet.");
        Task::none()
    }

    fn back(&mut self) {
        nav::back(&mut self.stack, &mut self.fwd);
    }

    fn forward(&mut self) {
        nav::forward(&mut self.stack, &mut self.fwd);
    }

    // ---- keyboard ----

    fn on_key(&mut self, k: Key) -> Task<Message> {
        let key = k.name.as_str();
        // TODO(4d/4e): SyncPlay screen and player page take the keys first.
        if matches!(self.phase, Phase::Picker) {
            return self.picker_key(key);
        }
        if let Some(w) = self.welcome.as_mut() {
            if let Some(c) = w.key(key) {
                self.first_run(c);
            }
            return Task::none();
        }
        if let Some(a) = self.accounts.as_mut()
            && (matches!(self.phase, Phase::Account) || a.dialog_open())
        {
            let t = a.on_key(&k).map(Message::Account);
            return Task::batch([t, self.drain_account_events()]);
        }
        if self.help_open {
            if matches!(key, "escape" | "backspace") || k.text.as_deref() == Some("?") {
                self.help_open = false;
            }
            return Task::none();
        }
        // TODO(4c): photo viewer, More menu and media info panel.
        if self.settings_open {
            return self.settings_key(key);
        }
        if self.menu {
            return self.menu_key(key);
        }
        let ready = matches!(self.phase, Phase::Ready);
        if opens_account_menu(key, ready, self.admin_open, false) {
            self.menu = true;
            self.menu_sel = 0;
            return self.refresh_accounts();
        }
        // TODO(4e): admin dialogs get typed text before the global shortcuts.
        if let Phase::Error(e) = self.phase {
            if matches!(key, "enter" | "r") && e != ConnectError::NotConfigured {
                return self.connect();
            }
            return Task::none();
        }
        if ready {
            // TODO(4b): search overlay takes the keys while open.
            if k.text.as_deref() == Some("?") {
                self.close_menus();
                self.help_open = true;
                return Task::none();
            }
            // TODO(4d): `g` opens Watch together.
            if (k.ctrl && key == "k") || (key == "/" && k.plain()) {
                self.close_admin();
                return self.open_search();
            }
            if k.alt && matches!(key, "left" | "right") {
                if key == "left" {
                    self.back();
                } else {
                    self.forward();
                }
                return Task::none();
            }
            // TODO(4b): open browse menus take keys here.
        }
        if self.admin_open {
            // TODO(4e): the admin screen's own keys; until then Esc leaves it.
            if matches!(key, "escape" | "backspace") {
                self.close_admin();
            }
            return Task::none();
        }
        if matches!(key, "escape" | "backspace") {
            self.back();
            return Task::none();
        }
        // TODO(4b/4c): `w` / `f` toggles and the Home / grid / detail page keys.
        Task::none()
    }

    // ---- view ----

    pub fn view(&self) -> Element<'_, Message> {
        let body: Element<Message> = match &self.phase {
            Phase::Account if self.welcome.is_some() => {
                welcome::view(self.welcome.as_ref().expect("checked")).map(Message::Welcome)
            }
            Phase::Account => match &self.accounts {
                Some(a) => a.view(self.now).map(Message::Account),
                None => self.connecting(),
            },
            Phase::Picker => match &self.picker {
                Some(p) => picker::view(
                    p,
                    &|a| self.user_pic(&a.user_id, a.image_tag.as_deref()),
                    Message::PickerAct,
                ),
                None => self.connecting(),
            },
            Phase::Connecting => self.connecting(),
            Phase::Error(ConnectError::NotConfigured) => self.splash(
                "Connect a media source",
                "Add your server address and access token to the .env file, then restart NOCTURNE.",
                false,
            ),
            Phase::Error(ConnectError::Unreachable) => self.splash(
                "Can't reach your server",
                "Check that it is running and that the address is right. Your library appears as soon as it answers.",
                true,
            ),
            Phase::Error(ConnectError::Rejected) => self.splash(
                "Your server declined the access token",
                "Create a new token on the server, update your .env file, then try again.",
                true,
            ),
            Phase::Ready if self.settings_open => self.settings_page(),
            Phase::Ready => self.page_view(),
        };
        let ready = matches!(self.phase, Phase::Ready);
        let layer = |h: &Handle, o: f32| -> Element<Message> {
            image(h.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Cover)
                .opacity(o)
                .into()
        };
        let mut layers: Vec<Element<Message>> = vec![
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(crate::style::fill(t::FRAME))
                .into(),
            layer(&self.glow, 1.0),
            layer(&self.dots, 0.09),
            body,
        ];
        if ready {
            layers.push(self.header());
            if self.offline.is_some() {
                layers.push(self.offline_banner());
            }
            if !self.admin_open
                && !self.settings_open
                && let Some(c) = self.crumbs()
            {
                layers.push(c);
            }
        }
        if let Some(p) = &self.preview {
            layers.push(p.view(Instant::now(), crate::motion::reduced(), self.win));
        }
        layers.push(layer(&self.grain, 0.02));
        if self.menu && ready {
            layers.push(self.account_menu());
        }
        if self.help_open {
            layers.push(self.help_overlay());
        }
        if let Some(d) = self
            .accounts
            .as_ref()
            .filter(|a| a.dialog_open())
            .and_then(|a| a.dialog_view())
        {
            layers.push(d.map(Message::Account));
        }
        if let Some((n, _)) = &self.note {
            layers.push(
                container(
                    container(text(n.clone()).size(t::BODY).color(t::TEXT))
                        .padding([t::S3, t::S4])
                        .style(crate::style::raised_card),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::End)
                .padding(t::S6)
                .into(),
            );
        }
        Stack::with_children(layers).into()
    }

    /// The current page: Home / library / detail are not ported yet.
    fn page_view(&self) -> Element<'_, Message> {
        let title = self
            .stack
            .last()
            .filter(|p| p.view != View::Home)
            .and_then(|p| p.media.as_ref())
            .map_or("Home".to_string(), |m| m.title.clone());
        container(
            column![
                text(title).size(t::HEADING).font(t::BOLD),
                text("Not ported yet")
                    .size(t::BODY)
                    .color(t::TEXT_SECONDARY),
            ]
            .spacing(t::S2)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    }

    fn centered<'a>(&self, items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
        container(column(items).spacing(t::S4).align_x(Alignment::Center))
            .center(Length::Fill)
            .into()
    }

    fn splash(&self, title: &str, body: &str, retry: bool) -> Element<'_, Message> {
        let mut v: Vec<Element<Message>> = vec![
            wordmark(t::DISPLAY).into(),
            label("Your media. Your night.").into(),
            space().height(t::S6).into(),
            text(title.to_string())
                .size(t::HEADING)
                .font(t::BOLD)
                .into(),
            container(
                text(body.to_string())
                    .size(t::BODY)
                    .color(t::TEXT_SECONDARY)
                    .align_x(Alignment::Center),
            )
            .max_width(460)
            .into(),
        ];
        if retry {
            v.push(space().height(t::S2).into());
            v.push(
                container(widget::pill_button(
                    "Try again",
                    true,
                    false,
                    Message::Retry,
                ))
                .width(160)
                .into(),
            );
        }
        self.centered(v)
    }

    fn connecting(&self) -> Element<'_, Message> {
        self.centered(vec![
            wordmark(t::DISPLAY).into(),
            label("Connecting")
                .color(iced::Color {
                    a: pulse(self.now, 1600),
                    ..t::TEXT_TERTIARY
                })
                .into(),
        ])
    }

    /// A first-run choice. Server and Both go to the server-address screen; Local waits for the
    /// local folders screen (catalog milestone), so for now it says so.
    fn first_run(&mut self, c: welcome::FirstRun) {
        match c {
            welcome::FirstRun::Local => {
                if let Some(w) = self.welcome.as_mut() {
                    w.local_note = true;
                }
            }
            welcome::FirstRun::Server | welcome::FirstRun::Both => self.welcome = None,
        }
    }

    /// Non-blocking: the page underneath stays usable.
    fn offline_banner(&self) -> Element<'_, Message> {
        let wait = self.offline.as_ref().map_or(0, |o| {
            o.next.saturating_duration_since(Instant::now()).as_secs() + 1
        });
        container(
            container(
                row![
                    text(format!(
                        "Can't reach your server. Showing what's already loaded. Retrying in {wait}s."
                    ))
                    .size(t::BODY),
                    action_button("Retry now", true, false, false, false, Message::RetryNow),
                ]
                .spacing(t::S4)
                .align_y(Alignment::Center),
            )
            .padding([t::S2, t::S4])
            .style(crate::style::raised_card),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .padding(Padding {
            top: t::HEADER_H + t::S2,
            ..Padding::ZERO
        })
        .into()
    }
}

/// The `a` shortcut opens the account menu, but never while a text field has the keys (search,
/// Admin dialogs).
fn opens_account_menu(key: &str, ready: bool, admin_open: bool, search_open: bool) -> bool {
    key == "a" && ready && !admin_open && !search_open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_a_in_search_does_not_open_the_account_menu() {
        assert!(opens_account_menu("a", true, false, false));
        assert!(!opens_account_menu("a", true, false, true));
        assert!(!opens_account_menu("a", true, true, false));
        assert!(!opens_account_menu("a", false, false, false));
        assert!(!opens_account_menu("b", true, false, false));
    }

    #[test]
    fn avatar_shows_the_picture_only_for_a_ready_current_tag() {
        let mut c: Cache<String, Img> = Cache::new(8);
        assert!(ready_pic(&c, "u", None).is_none(), "no tag: initial");
        assert!(ready_pic(&c, "u", Some("t1")).is_none(), "not fetched yet");
        c.insert(user_pic_key("u", "t1"), Img::Loading);
        assert!(ready_pic(&c, "u", Some("t1")).is_none(), "loading: initial");
        c.insert(user_pic_key("u", "t1"), Img::Failed(Instant::now()));
        assert!(ready_pic(&c, "u", Some("t1")).is_none(), "failed: initial");
        c.insert(
            user_pic_key("u", "t1"),
            Img::Ready(Handle::from_rgba(1, 1, vec![0, 0, 0, 255])),
        );
        assert!(ready_pic(&c, "u", Some("t1")).is_some());
        assert!(
            ready_pic(&c, "u", Some("t2")).is_none(),
            "changed tag: old picture not reused"
        );
        assert!(ready_pic(&c, "u", None).is_none(), "removed: initial");
        assert!(ready_pic(&c, "other", Some("t1")).is_none());
    }
}
