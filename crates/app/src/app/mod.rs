//! Root: connection state, navigation stack, keyboard and the screens. Port of the GPUI `Root`
//! (`crates/app/src/app.rs`).
mod actions;
mod browse;
mod cascade;
mod catalog;
mod cells;
mod couch;
mod details;
mod downloads;
mod extras;
mod folders;
mod header;
mod help;
mod home;
mod images;
pub mod message;
mod onboard;
mod online;
mod play;
mod preview;
mod remote;
mod search;
mod server_ui;
mod settings;
pub(crate) mod smart;
mod sources;
pub(crate) mod themes;
pub(crate) mod trakt;

use crate::account::{self, Accounts, Event as AcctEvent, devices, picker};
use crate::keys::Key;
use crate::player::{self, Player};
use crate::task::run;
use crate::theme as t;
use crate::widget::{self, label, pulse, tex, wordmark};
use iced::widget::image::Handle;
use iced::widget::{Stack, column, container, image, space, text};
use iced::{
    Alignment, ContentFit, Element, Length, Size, Subscription, Task, event, mouse, window,
};
use message::{AcctAct, Message, SwitchFail};
use nocturne_core::account::picker::{DefaultChange, Picker};
use nocturne_core::cache::Cache;
use nocturne_core::catalog::{Catalog, Status};
use nocturne_core::dropdown::Dropdown;
use nocturne_core::imgcache::DiskCache;
use nocturne_core::merge::{Preferred, SeriesIds};
use nocturne_core::nav;
use nocturne_core::profile::Sources;
use nocturne_core::settings::Settings;
use nocturne_core::source::SourceId;
use nocturne_core::source::{ConnectError, Media, Source, sniff, user_image_key};
use nocturne_core::sources::{Fanout, FirstRun, SourceFilter, Step};
use nocturne_core::sync::SyncHooks;
use nocturne_core::unified::Link;
use nocturne_jellyfin_admin::AdminClient;
use nocturne_jellyfin_user::{Session, UserClient};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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
    /// Home only: the libraries (header nav links), merged from every source as answers arrive.
    pub libs: Load<Vec<Media>>,
    pub fan: Fanout,
    /// Home only.
    pub home: Option<Box<home::Home>>,
    /// Library / collection / favourites pages.
    pub br: Option<Box<browse::Browse>>,
    /// Detail pages.
    pub det: Option<Box<crate::detail::Detail>>,
    /// Grid selection.
    pub sel: usize,
}

/// Library-like destinations that are not libraries (header links after the libraries).
/// `(page key, label)`.
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
    /// The saved sources could not be read (never overwritten); the reason is shown.
    Fatal(String),
    Ready,
}

/// Where Esc on an account screen leads back to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AccountBack {
    /// The app (the screens were opened from Sources).
    Ready,
}

pub struct App {
    cfg: Option<nocturne_jellyfin::Config>,
    /// `$XDG_DATA_HOME/nocturne`: each local source keeps its index under `local/<id>`.
    data: PathBuf,
    /// The saved profiles (priority order) and the catalog built from them.
    sources: Sources,
    catalog: Catalog,
    prefs: Preferred,
    series_ids: SeriesIds,
    /// The server the account menu, hooks and Admin act on.
    active: Option<SourceId>,
    sessions: HashMap<SourceId, (String, Session)>,
    filter: SourceFilter,
    /// Request counter for fan-outs (answers carry theirs; stale ones are dropped).
    epoch: u64,
    probing: HashSet<SourceId>,
    scans: HashMap<SourceId, catalog::Scan>,
    scan_gen: u64,
    /// Scans to start again as soon as the running (cancelled) one ends.
    rescan: HashSet<SourceId>,
    launch_scan: bool,
    src_open: bool,
    /// Saved themes and the Themes page.
    themes: nocturne_core::customize::Store,
    th: themes::State,
    /// Source whose removal waits for the confirmation dialog.
    confirm: Option<SourceId>,
    /// The extra-header dialog of a server.
    hdr: Option<sources::HeaderDlg>,
    /// Server UI: each server's layout as last read, and the open home-sections dialog.
    server_ui: HashMap<SourceId, nocturne_core::server_ui::ServerUi>,
    sec: Option<server_ui::SecDlg>,
    folders: Option<folders::Folders>,
    online: online::State,
    trakt: trakt::State,
    dl: downloads::State,
    smart: smart::State,
    setup: Option<FirstRun>,
    account_back: Option<AccountBack>,
    phase: Phase,
    stack: Vec<Page>,
    /// Pages left by Back, restored by Forward (cleared by any new navigation).
    fwd: Vec<Page>,
    next_pid: u64,
    images: Cache<String, Img>,
    /// Big backdrops, kept apart so they cannot push posters out of `images`.
    heroes: Cache<String, Img>,
    scale: f32,
    /// Gamepads connected now, and the monitor under the window (Automatic couch mode).
    pads: usize,
    /// Measured offset / viewport / content width of the header link strip.
    nav_vp: Option<(f32, f32, f32)>,
    monitor: Option<Size>,
    /// Write hooks of the servers that are not the active login.
    hooks_by: HashMap<SourceId, Arc<SyncHooks>>,
    player: Option<Player>,
    search: Option<Box<search::SearchUi>>,
    lib_menu: bool,
    grain: Handle,
    dots: Handle,
    glow: Handle,
    win: Size,
    now: Instant,
    note: Option<(String, Instant)>,
    /// A newer version, until dismissed (docs/dev/UPDATER_PLAN.md).
    update: Option<nocturne_core::update::UpdateState>,
    /// Developer-token mode (`NOCTURNE_JF_TOKEN`): no login, read-only.
    dev: bool,
    /// The token / account has admin rights (an `Admin` entry follows the libraries).
    admin: bool,
    admin_open: bool,
    /// The Admin screen (when open); `admin_open` mirrors it for the header.
    adm: Option<Box<crate::admin::Admin>>,
    sp: crate::syncplay::SyncPlay,
    /// Sources the SyncPlay targets were built for, and the writes switch it was last given.
    sp_ids: Vec<SourceId>,
    sp_writes: bool,
    /// Remote-control sockets, one per signed-in server (empty unless the setting is on).
    remote: HashMap<SourceId, remote::RemoteLink>,
    queue: crate::queue::AudioQueue,
    queue_open: bool,
    /// The next player opened restores the kept music queue.
    restore_q: bool,
    devices: Option<devices::Devices>,
    accounts: Option<Accounts>,
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
    help_open: bool,
    /// The first-run walkthrough, and the screen it lent the window to (if any).
    ob: Option<Box<crate::onboarding::Onboarding>>,
    ob_away: Option<crate::onboarding::Return>,
    /// This launch built its sources from the 0.2 files.
    migrated: bool,
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
        crate::couch::set(settings.couch == nocturne_core::settings::CouchMode::On);
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
        let me = Self {
            dev: cfg.is_some(),
            cfg,
            data: catalog::app_data_dir(),
            sources: Sources::default(),
            catalog: Catalog::default(),
            prefs: Preferred::default(),
            series_ids: SeriesIds::default(),
            active: None,
            sessions: HashMap::new(),
            filter: SourceFilter::All,
            epoch: 0,
            probing: HashSet::new(),
            scans: HashMap::new(),
            scan_gen: 0,
            rescan: HashSet::new(),
            launch_scan: false,
            src_open: false,
            themes: nocturne_core::customize::Store::load(),
            th: Default::default(),
            confirm: None,
            hdr: None,
            server_ui: HashMap::new(),
            sec: None,
            folders: None,
            online: Default::default(),
            trakt: Default::default(),
            dl: downloads::State::new(&catalog::app_data_dir()),
            smart: smart::State::load(),
            setup: None,
            account_back: None,
            phase: Phase::Connecting,
            stack: vec![],
            fwd: vec![],
            next_pid: 0,
            images: Cache::new(600),
            heroes: Cache::new(6),
            scale: 1.0,
            pads: 0,
            nav_vp: None,
            monitor: None,
            hooks_by: HashMap::new(),
            player: None,
            search: None,
            lib_menu: false,
            grain: tex::grain(),
            dots: tex::dots(),
            glow: tex::glow(),
            win: Size::new(1280.0, 800.0),
            now: Instant::now(),
            note: None,
            update: None,
            admin: false,
            admin_open: false,
            adm: None,
            sp: crate::syncplay::SyncPlay::new(vec![], false),
            sp_ids: vec![],
            sp_writes: false,
            remote: HashMap::new(),
            queue: Default::default(),
            queue_open: false,
            restore_q: false,
            devices: None,
            accounts: None,
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
            help_open: false,
            ob: None,
            ob_away: None,
            migrated: false,
            preview: preview::Preview::from_env(),
        };
        let size = window::oldest()
            .and_then(window::size)
            .map(Message::Resized);
        let scale = window::oldest()
            .and_then(window::scale_factor)
            .map(Message::Scale);
        // Keyring reads, index loading and connections all follow in tasks, after the first frame.
        let check = if me.settings.check_updates {
            run(nocturne_core::update::check(), Message::Update)
        } else {
            Task::none()
        };
        let task = Task::batch([
            size,
            scale,
            check,
            me.load_sources(),
            open_disk,
            crate::snap::on_boot(),
        ]);
        crate::timing::mark("boot done");
        (me, task)
    }

    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    /// Couch mode scales the whole UI (type, targets and spacing together).
    pub fn scale_factor(&self) -> f32 {
        crate::couch::factor() * crate::theme::font_scale()
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
        if let Some(p) = &self.player {
            subs.push(p.subscription().map(Message::Player));
        }
        subs.push(self.sp.subscription().map(Message::SyncPlay));
        if self.settings.couch != nocturne_core::settings::CouchMode::Off {
            subs.push(Subscription::run(crate::pad::events).map(Message::Pad));
        }
        // A focused text field keeps Esc from the keyboard listener; the overlay still has to close.
        if self.search.is_some() {
            subs.push(event::listen_with(|e, status, _| match e {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) if status == iced::event::Status::Captured => {
                    Some(Message::Search(search::SMsg::Close))
                }
                _ => None,
            }));
        }
        // Frames only while something pulses (and once, to time the first painted frame).
        if self.animating() || crate::timing::first_frame_pending() {
            subs.push(window::frames().map(Message::Frame));
        }
        Subscription::batch(subs)
    }

    /// The sign-in screens and the account dialogs put the cursor in their first input themselves.
    fn sync_account_focus(&mut self) -> Task<Message> {
        let shown = matches!(self.phase, Phase::Account)
            || self.accounts.as_ref().is_some_and(|a| a.dialog_open());
        match self.accounts.as_mut() {
            Some(a) => a.focus_task(shown).map(Message::Account),
            None => Task::none(),
        }
    }

    fn animating(&self) -> bool {
        self.preview
            .as_ref()
            .is_some_and(|p| p.animating(crate::motion::reduced()))
            || self.ob_visible()
                && self
                    .ob
                    .as_ref()
                    .is_some_and(|o| o.animating(Instant::now()))
            // The breathing labels are steady with reduced motion: no frames needed for them.
            || !crate::motion::reduced()
                && (matches!(self.phase, Phase::Connecting)
                    || (matches!(self.phase, Phase::Account)
                        && self.accounts.as_ref().is_some_and(|a| a.animating())))
    }

    // ---- update ----

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        let task = self.update_inner(msg);
        self.sync_theme();
        task
    }

    fn update_inner(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Key(k) => self.on_key(k),
            Message::Tick(now) => {
                self.now = now;
                self.theme_tick(now);
                self.sync_detail();
                self.sync_ob();
                Task::batch([
                    self.tick(),
                    self.sync_extras(),
                    self.sync_remote(),
                    self.sync_account_focus(),
                    self.dl_tick(),
                    crate::snap::poll(),
                ])
            }
            Message::Frame(now) => {
                self.now = now;
                crate::timing::first_frame();
                if let Some(p) = self.preview.as_mut() {
                    p.tick(Instant::now());
                }
                Task::none()
            }
            Message::Scale(f) => {
                self.scale = f;
                Task::none()
            }
            Message::Rescale => Task::batch([
                window::oldest()
                    .and_then(window::scale_factor)
                    .map(Message::Scale),
                window::oldest()
                    .and_then(window::size)
                    .map(Message::Resized),
            ]),
            Message::Pad(e) => self.on_pad(e),
            Message::NavScrolled(o, w, c) => {
                self.nav_vp = Some((o, w, c));
                Task::none()
            }
            Message::NavPage(d) => {
                let (off, vw, content) = self.nav_geom();
                let x = (off + d * vw * 0.7).clamp(0.0, (content - vw).max(0.0));
                iced::widget::operation::scroll_to(
                    iced::widget::Id::from(header::NAV_ID),
                    iced::widget::scrollable::AbsoluteOffset { x, y: 0.0 },
                )
            }
            Message::Monitor(m) => {
                self.monitor = m;
                self.refresh_couch()
            }
            Message::Resized(s) => {
                self.win = s;
                if let Some(p) = &mut self.picker {
                    p.cols = picker::columns(s.width);
                }
                Task::batch([
                    self.home_wants(),
                    self.browse_visible(),
                    self.probe_monitor(),
                ])
            }
            Message::CloseRequested(_) => self.quit(),
            Message::Back => {
                self.src_open = false;
                self.th.open = false;
                self.back();
                self.page_wants()
            }
            Message::Forward => {
                self.src_open = false;
                self.th.open = false;
                self.forward();
                self.page_wants()
            }
            Message::Connected(r) => self.connected(r),
            Message::AdminProbed(yes) => {
                self.admin = yes;
                self.apply_write_settings();
                Task::none()
            }
            Message::Retry => self.connect(),
            Message::Update(u) => {
                self.update = u;
                Task::none()
            }
            Message::UpdateCopy(cmd) => iced::clipboard::write(cmd.to_string()),
            Message::UpdateOpen(u) => {
                if u.starts_with("https://") {
                    let _ = std::process::Command::new("xdg-open").arg(u).spawn();
                }
                Task::none()
            }
            Message::UpdateDismiss => {
                self.update = None;
                Task::none()
            }
            Message::RetryNow => {
                let now = Instant::now();
                for s in &mut self.catalog.slots {
                    if let Status::Offline { tries, .. } = s.status {
                        s.status = Status::Offline {
                            tries,
                            retry_at: now,
                        };
                    }
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
            Message::Loaded(h) => match h.take() {
                Some(l) => self.on_loaded(l),
                None => Task::none(),
            },
            Message::Fan(pid, kind, epoch, origin, r) => {
                self.fan_answer(pid, kind, epoch, origin, r)
            }
            Message::ServerUi(id, r) => {
                self.server_ui_answer(id, r);
                Task::none()
            }
            Message::SectionsSaved(id, ok) => self.sections_saved(id, ok),
            Message::Latest(pid, epoch, origin, lib, r) => {
                self.latest_answer(pid, epoch, origin, lib, r)
            }
            Message::SeriesIds(origin, v) => self.series_arrived(origin, v),
            Message::Image(key, big, h) => {
                self.image_arrived(key, big, h);
                Task::none()
            }
            Message::Cell(at, act) => self.update_cell(at, act),
            Message::PlayItem(m) => self.play_item(m),
            Message::Play(m, rest) => self.start_play(m, rest, None),
            Message::PlayTarget(m, rest, src, r) => self.play_target(m, rest, src, r),
            Message::Detail(pid, m) => self.detail_msg(pid, m),
            Message::DetailWrote(pid, w, r) => self.detail_wrote(pid, w, r),
            Message::Player(m) => self.player_msg(m),
            Message::RefreshResume => self.refresh_resume(),
            Message::Toast(s) => {
                self.toast(s);
                Task::none()
            }
            Message::LocalWrite(origin, id, v, ok) => {
                self.local_write(origin, id, v, ok);
                Task::none()
            }
            Message::HomeScroll(off, vh) => {
                crate::timing::scroll_event();
                if let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) {
                    (h.off, h.vh) = (off, vh);
                }
                self.home_wants()
            }
            Message::HomeStrip(row, x) => {
                if let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) {
                    h.strips.insert(row, x);
                }
                self.home_wants()
            }
            Message::Browse(pid, m) => self.browse_msg(pid, m),
            Message::Search(m) => self.search_msg(m),
            Message::SearchScrolled(off) => {
                self.search_scrolled(off);
                Task::none()
            }
            Message::HooksLoaded(id, r) => {
                self.hooks_loaded(id, r);
                Task::none()
            }
            Message::ServerName(id, name) => {
                self.server_named(id, name);
                Task::none()
            }
            Message::ToggleLibMenu => {
                self.lib_menu = !self.lib_menu;
                Task::none()
            }
            Message::OpenKind(k) => {
                self.lib_menu = false;
                self.open_kind(k)
            }
            Message::Pinged(id, ok) => self.pinged(id, ok),
            Message::SlotReady(h) => match h.take() {
                Some(slot) => self.slot_ready(slot),
                None => Task::none(),
            },
            Message::SessionLoaded(id, r) => self.session_loaded(id, r),
            Message::Scan(id, run_id, m) => self.scan_msg(id, run_id, m),
            Message::Src(a) => self.src_act(a),
            Message::HeaderInput(v, s) => {
                self.header_input(v, s);
                Task::none()
            }
            Message::HeaderLoaded(id, l) => {
                self.header_loaded(id, l);
                Task::none()
            }
            Message::HeaderDone(id, r) => self.header_done(id, r),
            Message::Folder(m) => self.folder_msg(m),
            Message::Online(m) => self.online_msg(m),
            Message::Trakt(m) => self.trakt_msg(m),
            Message::Smart(m) => self.smart_msg(m),
            Message::Cascade(m) => self.cascade_msg(m),
            Message::Downloads(m) => self.dl_msg(m),
            Message::CycleFilter => {
                self.filter = self.filter.next(&self.catalog.priority());
                self.stack.truncate(1);
                self.retry_page()
            }
            Message::OpenSources => self.open_sources(),
            Message::Themes(m) => self.themes_msg(m),
            Message::Snapped(shot, n) => {
                crate::snap::save(&shot, n);
                Task::none()
            }
            Message::Profile(uid, p) => {
                let same = self.session().is_some_and(|(_, s)| s.user_id == uid);
                // Switched away meanwhile, or the server did not answer: keep what is shown.
                if let (true, Some(tag)) = (same, p) {
                    self.my_tag = tag;
                    if let Some((url, _)) = self.session() {
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
            Message::GoHome => {
                self.close_admin();
                self.lib_menu = false;
                self.stack.truncate(1);
                if let Some(p) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) {
                    // Reached with the mouse: focus must not stay on a header link.
                    if p.row == 0 {
                        (p.row, p.col) = (1, 0);
                    }
                }
                self.home_wants()
            }
            Message::OpenLibrary(i) => {
                let lib = self
                    .stack
                    .first()
                    .and_then(|h| h.libs.items().get(i).cloned());
                self.close_admin();
                self.lib_menu = false;
                match lib {
                    Some(m) => self.open_library(m),
                    None => Task::none(),
                }
            }
            Message::OpenGlobal(i) => {
                self.close_admin();
                self.open_global(i)
            }
            Message::OpenAdmin => self.open_admin(),
            Message::Admin(m) => self.admin_msg(m),
            Message::SyncPlay(m) => self.sp_msg(m),
            Message::RemoteItems(l, i, s, q) => self.remote_items(l, i, s, q),
            Message::RemoteTarget(l, i, s, r) => self.remote_target(l, i, s, r),
            Message::Queue(m) => self.queue_msg(m),
            Message::Devices(m) => {
                if self.devices.as_mut().is_some_and(|d| d.update(m)) {
                    self.devices = None;
                }
                Task::none()
            }
            Message::GroupPlay(o, l, i, s, r) => self.group_play(o, l, i, s, r),
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
                self.home_wants()
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
                self.choose_setting(row, idx)
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
            Message::Onboarding(m) => self.ob_msg(m),
            Message::RunSetup => self.run_setup_again(),
            Message::DiskReady(d) => {
                self.disk = d;
                self.apply_disk();
                self.trakt_load()
            }
            Message::Noop => Task::none(),
        }
    }

    /// 5 times a second: quit request, toast expiry, connection watch.
    fn tick(&mut self) -> Task<Message> {
        if crate::platform::term_requested() {
            return self.quit();
        }
        if crate::single::take_activate() {
            return crate::single::bring_forward();
        }
        self.drain_reverts();
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
                self.dev_source(s);
                let ready = self.ready();
                let cfg = self.cfg.clone();
                let probe = run(
                    async move {
                        let c = AdminClient::new(cfg?).ok()?;
                        nocturne_core::admin::state::admin_visible(&c.system_info().await)
                            .then_some(())
                    },
                    |a| Message::AdminProbed(a.is_some()),
                );
                Task::batch([ready, probe])
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
                AcctEvent::BackToPicker if self.picker.is_some() => tasks.push(self.show_picker()),
                AcctEvent::BackToPicker => tasks.push(self.leave_account()),
            }
        }
        Task::batch(tasks)
    }

    /// A login is ready (typed, Quick Connect or restored). Its server becomes a source profile (or
    /// updates the one it already has: an account switch), gets a slot using this token, and Home shows.
    fn activate(&mut self, url: String, s: Session) -> Task<Message> {
        let url = url.trim_end_matches('/').to_string();
        let id = self.sources.upsert_server(&url, &s.user_id, &s.user_name);
        // ponytail: a small atomic file write on the UI thread; move to a task if it ever shows up in a trace.
        if self.sources.save().is_err() {
            self.toast("Couldn't save your sources.");
        }
        let profile = self.sources.get(id).cloned().expect("just added");
        let token = s.access_token.clone();
        let tok = move |_: &str, _: &str| Some(token.clone());
        let slot = nocturne_core::catalog::Slot::build(
            &profile,
            &nocturne_core::catalog::Env {
                token: &tok,
                data_dir: &self.data,
                disk: self.disk.clone(),
            },
        );
        if slot.backend.is_none() {
            self.toast("This server address can't be used.");
            return Task::none();
        }
        self.catalog.upsert(slot);
        self.catalog.set_status(id, Status::Online); // it just answered
        self.catalog.reorder(&self.sources.ids());
        self.hooks = Arc::new(SyncHooks::new(
            UserClient::from_session(&url, &s).ok(),
            self.settings.allow_server_writes,
            self.settings.sync_progress,
        ));
        self.images = Cache::new(600);
        self.stack.clear();
        self.fwd.clear();
        self.admin_open = false;
        self.src_open = false;
        self.th.open = false;
        self.settings_open = false;
        self.account_back = None;
        // The admin section follows the account's own rights.
        self.admin = s.is_admin;
        self.apply_write_settings();
        self.sessions.insert(id, (url, s));
        self.active = Some(id);
        self.my_tag = None;
        let profile = self.refresh_profile();
        let ready = self.ready();
        if self.accounts.as_ref().is_some_and(|a| a.keyring_down()) {
            self.toast(account::KEYRING_NOTE);
        }
        let setup = self.setup_after(Step::Server);
        Task::batch([profile, ready, setup])
    }

    /// Read the own profile picture tag (`GET /Users/Me`), then the picture itself. Runs on
    /// sign-in, account switch and after the profile image is set or removed.
    fn refresh_profile(&mut self) -> Task<Message> {
        let Some((url, s)) = self.session() else {
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
        let (Some(tag), Some(src)) = (tag, self.source().or_else(|| self.anon_src.clone())) else {
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
        let uid = &self.session()?.1.user_id;
        self.user_pic(uid, self.my_tag.as_deref())
    }

    /// Back to the account screens: drop everything tied to the login (the source itself stays
    /// until a new login replaces it or it is signed out).
    fn leave_session(&mut self) {
        self.extras_shutdown();
        if let Some(id) = self.active {
            self.sessions.remove(&id);
        }
        self.stack.clear();
        self.admin = false;
        self.admin_open = false;
        self.settings_open = false;
        self.src_open = false;
        self.th.open = false;
        self.menu = false;
        self.my_tag = None;
        self.hooks = Arc::new(SyncHooks::new(None, false, false));
        self.phase = Phase::Account;
    }

    /// Esc on an account screen opened from inside the app (or from the first-run choice).
    fn leave_account(&mut self) -> Task<Message> {
        match self.account_back.take() {
            Some(AccountBack::Ready) => {
                let load = self
                    .active
                    .filter(|_| self.session().is_none())
                    .map(|id| self.load_session(id));
                Task::batch([self.ready(), load.unwrap_or_else(Task::none)])
            }
            None => Task::none(),
        }
    }

    /// Account screens that can be left again need somewhere to go back to.
    fn can_return(&self) -> bool {
        self.catalog.online().next().is_some()
    }

    fn sign_out(&mut self) -> Task<Message> {
        let client = self
            .session()
            .and_then(|(u, s)| UserClient::from_session(u, s).ok());
        let uid = self.session().map(|(_, s)| s.user_id.clone());
        let url = self.session().map(|(u, _)| u.clone());
        let id = self.active.take();
        self.leave_session();
        // This source is signed out; the others carry on.
        if let Some(slot) = id.and_then(|i| self.catalog.slot_mut(i)) {
            slot.backend = None;
            slot.status = Status::SignedOut;
        }
        let mut t = self.ensure_accounts();
        if let (Some(a), Some(url)) = (self.accounts.as_mut(), url) {
            a.point_at(&url);
            a.sign_out(client, uid);
        }
        if self.can_return() {
            self.toast("Signed out.");
            t = Task::batch([t, self.ready()]);
        }
        t
    }

    fn switch_server(&mut self) {
        self.leave_session();
        self.account_back = self.can_return().then_some(AccountBack::Ready);
        if let Some(a) = self.accounts.as_mut() {
            a.switch_server();
        }
    }

    fn menu_items(&self) -> Vec<(String, AcctAct)> {
        let mut v = vec![
            ("Settings".to_string(), AcctAct::Settings),
            ("Sources".to_string(), AcctAct::Sources),
            (
                match self.dl.pending() {
                    0 => "Downloads".to_string(),
                    n => format!("Downloads ({n})"),
                },
                AcctAct::Downloads,
            ),
        ];
        // The header's Admin link can sit past the edge of a narrow window; the menu always shows it.
        if self.admin && !self.dev {
            v.push(("Admin".to_string(), AcctAct::Admin));
        }
        if !self.dev && self.active.is_some() {
            let mut add = |l: &str, a| v.push((l.to_string(), a));
            add("Change password", AcctAct::Password);
            add("Profile image", AcctAct::ProfileImage);
            add("Remove profile image", AcctAct::RemoveImage);
            add("Display language", AcctAct::Language);
            add("Authorize a device", AcctAct::Authorize);
            add("Watch together", AcctAct::Together);
            add("Your devices", AcctAct::Devices);
            if !self.queue.is_empty() {
                v.push((
                    format!("Play music queue ({})", self.queue.items.len()),
                    AcctAct::PlayQueue,
                ));
                v.push(("Clear music queue".to_string(), AcctAct::ClearQueue));
            }
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
        self.other_accounts = match self.session() {
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
        let Some((url, s)) = self.session().cloned() else {
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
        self.session()
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
            AcctAct::Downloads => return self.dl_msg(downloads::Msg::Open),
            AcctAct::Devices => return self.open_devices(),
            AcctAct::Together => return self.sp_msg(crate::syncplay::Msg::OpenScreen),
            AcctAct::PlayQueue => self.queue_open = true,
            AcctAct::ClearQueue => {
                let m = self.queue.clear();
                self.toast(m);
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
                let url = self.session().map(|(u, _)| u.clone());
                self.leave_session();
                self.account_back = self.can_return().then_some(AccountBack::Ready);
                if let (Some(a), Some(url)) = (self.accounts.as_mut(), url) {
                    a.point_at(&url);
                    a.add_account();
                }
            }
            AcctAct::Switch => self.switch_server(),
            AcctAct::SignOut => return self.sign_out(),
            AcctAct::Sources => return self.open_sources(),
            AcctAct::Admin => return self.open_admin(),
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
        for h in self.hooks_by.values() {
            h.master.store(self.settings.allow_server_writes, Relaxed);
            h.sync.store(self.settings.sync_progress, Relaxed);
        }
        let on = self.settings.allow_server_writes && !self.dev;
        if let Some(a) = self.adm.as_deref_mut() {
            a.set_writes(on);
        }
    }

    /// Load the current page again (after a failure or a reconnect).
    fn retry_page(&mut self) -> Task<Message> {
        match self.stack.last() {
            Some(p) if p.view == View::Home => {
                let pid = p.pid;
                self.load_home(pid)
            }
            Some(p) if p.view == View::Grid => self.reload_browse(),
            _ => Task::none(),
        }
    }

    /// Images (and pages) the page in view needs, after navigating back or forward.
    fn page_wants(&mut self) -> Task<Message> {
        match self.stack.last().map(|p| p.view) {
            Some(View::Home) => self.home_wants(),
            Some(View::Grid) => self.browse_visible(),
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
            fan: Fanout::default(),
            home: None,
            br: None,
            det: None,
            sel: 0,
        });
        pid
    }

    fn push(&mut self, view: View, media: Option<Media>) -> Task<Message> {
        if self.catalog.slots.is_empty() {
            return Task::none();
        }
        let pid = self.push_page(view, media);
        if view == View::Home {
            return self.load_home(pid);
        }
        Task::none()
    }

    /// Leave Admin (if open) before navigating elsewhere.
    fn close_admin(&mut self) {
        self.settings_open = false;
        self.src_open = false;
        self.th.open = false;
        self.menu = false;
        self.admin_open = false;
        if let Some(mut a) = self.adm.take() {
            a.close();
        }
    }

    fn back(&mut self) {
        nav::back(&mut self.stack, &mut self.fwd);
    }

    fn forward(&mut self) {
        nav::forward(&mut self.stack, &mut self.fwd);
    }

    // ---- keyboard ----

    fn on_key(&mut self, mut k: Key) -> Task<Message> {
        // The remote's Menu key outside the player opens the account menu.
        if k.name == "menu" && self.player.is_none() {
            k.name = "a".into();
        }
        if self.dl.visible() {
            return match k.name.as_str() {
                "escape" | "backspace" => {
                    self.dl.escape();
                    Task::none()
                }
                "enter" => self.dl_msg(downloads::Msg::Confirm),
                "up" => self.dl_msg(downloads::Msg::Step(-1)),
                "down" => self.dl_msg(downloads::Msg::Step(1)),
                _ => Task::none(),
            };
        }
        // The subtitle finder is modal on every page (over the player and over a detail page): it takes Esc and
        // swallows the rest, so keys never act on the page underneath.
        if self.online.finder_open() {
            return match k.name.as_str() {
                "escape" | "backspace" => self.online_msg(online::Msg::CloseFinder),
                "up" => self.online_msg(online::Msg::Step(-1)),
                "down" => self.online_msg(online::Msg::Step(1)),
                "enter" => self.online_msg(online::Msg::Confirm),
                _ => Task::none(),
            };
        }
        if self.player.is_some() {
            return self.player_msg(player::Msg::Key(k));
        }
        let key = k.name.as_str();
        if let Some(t) = self.extras_key(&k) {
            return t;
        }
        if self.ob_visible() {
            return self.ob_key(&k);
        }
        if matches!(self.phase, Phase::Picker) {
            return self.picker_key(key);
        }
        if key == "escape"
            && self.account_back.is_some()
            && matches!(self.phase, Phase::Account)
            && self.accounts.as_ref().is_some_and(|a| a.at_connect())
        {
            return self.leave_account();
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
        if self.hdr.is_some() {
            if key == "escape" {
                self.hdr = None;
            }
            return Task::none();
        }
        if self.sections_open() {
            if key == "escape" {
                self.close_sections();
            }
            return Task::none();
        }
        if self.confirm.is_some() {
            if matches!(key, "escape" | "backspace") {
                self.confirm = None;
            }
            return Task::none();
        }
        if self.folders.is_some() {
            return self.folders_key(key);
        }
        if self.th.open {
            if key == "escape" {
                return if self.is_previewing() {
                    self.themes_msg(themes::Msg::Revert)
                } else {
                    self.themes_msg(themes::Msg::Close)
                };
            }
            return Task::none();
        }
        if self.src_open {
            if matches!(key, "escape" | "backspace") {
                self.src_open = false;
                self.th.open = false;
            }
            return Task::none();
        }
        if self.smart.is_open() {
            if key == "escape" {
                return self.smart_msg(smart::Msg::Close);
            }
            return Task::none();
        }
        if self.smart.menu_open() && matches!(key, "escape" | "backspace") {
            self.smart.close_menu();
            return Task::none();
        }
        if self.trakt.is_open() {
            if key == "escape" {
                return self.trakt_msg(trakt::Msg::Close);
            }
            return Task::none();
        }
        if self.settings_open {
            return self.settings_key(key);
        }
        if self.menu {
            return self.menu_key(key);
        }
        if self.search.is_some() {
            return self.search_key(key);
        }
        let ready = matches!(self.phase, Phase::Ready);
        // A detail overlay (viewer, dialog, menu) owns the keys: no global shortcuts under it.
        if ready
            && !self.admin_open
            && self.detail_capturing()
            && let Some(t) = self.detail_key(&k)
        {
            return t;
        }
        if opens_account_menu(key, ready, self.admin_open, false) {
            self.menu = true;
            self.menu_sel = 0;
            return self.refresh_accounts();
        }
        if self.admin_dialog() {
            return self.admin_key(&k);
        }
        if let Phase::Error(e) = self.phase {
            if matches!(key, "enter" | "r") && e != ConnectError::NotConfigured {
                return self.connect();
            }
            return Task::none();
        }
        if ready {
            if k.text.as_deref() == Some("?") {
                self.close_menus();
                self.help_open = true;
                return Task::none();
            }
            if key == "g" && k.plain() && self.sp.can_open() {
                self.close_menus();
                return self.sp_msg(crate::syncplay::Msg::OpenScreen);
            }
            if (k.ctrl && key == "k") || (key == "/" && k.plain()) {
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
            if self.lib_menu && matches!(key, "escape" | "backspace") {
                self.lib_menu = false;
                return Task::none();
            }
            if self.stack.last().is_some_and(|p| {
                p.br.as_deref()
                    .is_some_and(|b| b.menu != browse::Menu::None)
            }) && let Some(t) = self.browse_key(key, k.shift)
            {
                return t;
            }
        }
        if self.adm.is_some() {
            return self.admin_key(&k);
        }
        if let Some(t) = self.detail_key(&k) {
            return t;
        }
        if matches!(key, "escape" | "backspace") {
            self.back();
            return self.page_wants();
        }
        if matches!(key, "w" | "f")
            && ready
            && k.plain()
            && let Some(m) = self.focus_media()
        {
            return if key == "w" {
                self.toggle_played(&m)
            } else {
                self.toggle_fav(&m)
            };
        }
        match self.stack.last().map(|p| p.view) {
            Some(View::Grid) => self.browse_key(key, k.shift).unwrap_or_else(Task::none),
            Some(View::Home) => self.home_key(key),
            _ => Task::none(),
        }
    }

    /// The item under the focus ring / selection on the page in view.
    fn focus_media(&self) -> Option<Media> {
        let p = self.stack.last()?;
        match p.view {
            View::Home => {
                let h = p.home.as_deref()?;
                match h.layout().get(h.row).copied()? {
                    home::HRow::Hero => h.hero().cloned(),
                    home::HRow::Nav => None,
                    k => h.items(k).get(h.col).map(|(m, _)| (*m).clone()),
                }
            }
            View::Grid => p.br.as_deref()?.item(p.sel).map(|(m, _)| m.clone()),
            View::Detail => p.media.clone(),
        }
    }

    /// Enter on a header link (Home focus on the Nav row).
    fn activate_link(&mut self, l: Option<Link>) -> Task<Message> {
        match l {
            Some(Link::Home) => self.update(Message::GoHome),
            Some(Link::Kind(k)) => self.update(Message::OpenKind(k)),
            Some(Link::Lib(i)) => self.update(Message::OpenLibrary(i)),
            Some(Link::Libraries) => self.update(Message::ToggleLibMenu),
            Some(Link::Global(i)) => self.update(Message::OpenGlobal(i)),
            Some(Link::Smart) => self.update(Message::Smart(smart::Msg::Menu)),
            Some(Link::Admin) => self.update(Message::OpenAdmin),
            None => Task::none(),
        }
    }

    /// A calm panel for a page whose load failed, with a way to try again.
    fn failed_view(&self, msg: &str) -> Element<'_, Message> {
        container(
            column![
                text(msg.to_string())
                    .size(t::BODY)
                    .color(t::text_secondary()),
                container(widget::pill_button(
                    "Try again",
                    false,
                    false,
                    Message::PageRetry
                ))
                .width(160),
            ]
            .spacing(t::S4)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    }

    // ---- view ----

    pub fn view(&self) -> Element<'_, Message> {
        if let Some(o) = self.ob.as_deref().filter(|_| self.ob_away.is_none()) {
            let layer = |h: &Handle, op: f32| -> Element<Message> {
                image(h.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Cover)
                    .opacity(op)
                    .into()
            };
            let mut layers: Vec<Element<Message>> = vec![
                container(space())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(crate::style::page)
                    .into(),
                layer(&self.glow, 1.0),
                layer(&self.dots, 0.09),
                o.view(Instant::now()).map(Message::Onboarding),
                layer(&self.grain, 0.02),
            ];
            if let Some((n, _)) = &self.note {
                layers.push(
                    container(
                        container(text(n.clone()).size(t::BODY).color(t::text()))
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
            return Stack::with_children(layers).into();
        }
        let body: Element<Message> = match &self.phase {
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
            Phase::Fatal(why) => self.splash(
                "Your sources could not be read",
                &format!(
                    "{why}. The file was left as it is; fix or move it and start NOCTURNE again."
                ),
                false,
            ),
            Phase::Ready if self.folders.is_some() => self.folders_page(),
            Phase::Ready if self.th.open => self.themes_page(),
            Phase::Ready if self.src_open => self.sources_page(),
            Phase::Ready if self.settings_open => self.settings_page(),
            Phase::Ready if self.adm.is_some() => match self.adm.as_deref() {
                Some(a) => a.view().map(Message::Admin),
                None => space().into(),
            },
            Phase::Ready => self.page_view(),
        };
        if let Some(p) = &self.player {
            // The player page is the whole window (it paints its own backdrop and leaves the
            // video hole transparent).
            let mut layers = vec![p.view().map(Message::Player)];
            if let Some((n, _)) = &self.note {
                layers.push(
                    container(
                        container(text(n.clone()).size(t::BODY).color(t::text()))
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
            if self.online.finder_open() {
                layers.push(self.finder_dialog());
            }
            if let Some(h) = self.hints() {
                layers.push(h);
            }
            return Stack::with_children(layers).into();
        }
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
                .style(crate::style::page)
                .into(),
            layer(&self.glow, 1.0),
            layer(&self.dots, 0.09),
            body,
        ];
        let setting_up = self.folders.as_ref().is_some_and(|f| f.setup) || self.ob_away.is_some();
        if ready && !setting_up {
            layers.push(self.header());
            if let Some(b) = self.source_banner().or_else(|| self.update_banner()) {
                layers.push(b);
            }
            if !self.admin_open
                && !self.settings_open
                && let Some(c) = self.crumbs()
            {
                layers.push(c);
            }
            if let Some(p) = self.stack.last()
                && let Some(d) = self.detail_fullscreen()
            {
                let pid = p.pid;
                layers.push(d.view().map(move |m| Message::Detail(pid, m)));
            }
        }
        self.extras_layers(&mut layers);
        if let Some(p) = &self.preview {
            layers.push(p.view(Instant::now(), crate::motion::reduced(), self.win));
        }
        layers.push(layer(&self.grain, 0.02));
        if self.menu && ready {
            layers.push(self.account_menu());
        }
        if self.search.is_some() && ready {
            layers.push(self.search_overlay());
        }
        if let Some(h) = self.hints() {
            layers.push(h);
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
        if let Some(id) = self.confirm {
            layers.push(self.confirm_dialog(id));
        }
        if self.hdr.is_some() {
            layers.push(self.header_dialog());
        }
        if self.sections_open() {
            layers.push(self.sections_dialog());
        }
        if self.online.finder_open() {
            layers.push(self.finder_dialog());
        }
        if self.trakt.is_open() {
            layers.push(self.trakt_dialog());
        }
        if self.dl.visible() {
            layers.push(self.downloads_dialog());
        }
        if self.smart.is_open() {
            layers.push(self.smart_dialog());
        }
        if let Some((n, _)) = &self.note {
            layers.push(
                container(
                    container(text(n.clone()).size(t::BODY).color(t::text()))
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

    /// The current page.
    fn page_view(&self) -> Element<'_, Message> {
        match self.stack.last().map(|p| p.view) {
            Some(View::Home) => self.home_page(),
            Some(View::Grid) => self.grid_page(),
            _ => match self
                .stack
                .last()
                .and_then(|p| Some((p.pid, p.det.as_deref()?)))
            {
                Some((pid, d)) => d.view().map(move |m| Message::Detail(pid, m)),
                None => space().into(),
            },
        }
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
                .font(t::bold())
                .into(),
            container(
                text(body.to_string())
                    .size(t::BODY)
                    .color(t::text_secondary())
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
                    ..t::text_tertiary()
                })
                .into(),
        ])
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
