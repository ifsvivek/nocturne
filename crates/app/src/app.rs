//! Root view: connection state, navigation stack, keyboard, and the screens.
mod ui_browse;
mod ui_detail;
mod ui_help;
mod ui_playlists;
mod ui_search;
mod ui_syncplay;

use crate::account::picker::{self, DefaultChange, Picker, Start};
use crate::account::{self, Accounts};
use crate::admin::{self, Admin};
use crate::detail::Full;
use crate::dropdown::{Dropdown, Outcome};
use crate::imgcache::DiskCache;
use crate::nav::{self, Dir};
use crate::player_view::{PlayerCtx, PlayerEvent, PlayerView};
use crate::search::Search;
use crate::settings::{self, Settings};
use crate::source::{Art, ConnectError, Kind, Media, Source, image_key, sniff};
use crate::sync::{NEED_WRITES, Reporter, SyncHooks, Tick, ticks};
use crate::ui::{self, Cell, Pic};
use crate::video::PlayerHooks;
use crate::{bridge, cache::Cache, theme as t};
use gpui::prelude::*;
use gpui::*;
use nocturne_jellyfin_admin::AdminClient;
use nocturne_jellyfin_user::{Session, UserClient};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ui_browse::{Browse, GLOBALS, Global};
use ui_detail::DRow;
use ui_syncplay::Sp;

#[derive(Clone)]
enum Load<T> {
    Loading,
    Ready(T),
    Failed,
}

impl<T: AsRef<[Media]>> Load<T> {
    fn items(&self) -> &[Media] {
        match self {
            Load::Ready(v) => v.as_ref(),
            _ => &[],
        }
    }
}

enum Img {
    Loading,
    Ready(Arc<Image>),
    /// When it failed: the slot is tried again after [`RETRY_FAILED`].
    Failed(std::time::Instant),
}

/// Profile pictures are fetched once at this size (square) for every avatar.
const AVATAR_PX: u32 = 96;

/// Slot key of a profile picture in the in-memory image cache.
fn user_pic_key(uid: &str, tag: &str) -> String {
    crate::source::user_image_key("", uid, tag, AVATAR_PX)
}

/// Ready picture for this user and tag. No tag (none set), still loading, failed, or only an
/// older tag cached: None, and the avatar shows the initial.
fn ready_pic(images: &Cache<String, Img>, uid: &str, tag: Option<&str>) -> Option<Arc<Image>> {
    match images.get(&user_pic_key(uid, tag?)) {
        Some(Img::Ready(i)) => Some(i.clone()),
        _ => None,
    }
}

const RETRY_FAILED: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(PartialEq, Clone, Copy)]
enum View {
    Home,
    Grid,
    Detail,
}

struct HomeRow {
    lib: Media,
    items: Load<Vec<Media>>,
    scroll: ScrollHandle,
}

/// Home rows, top to bottom. `Page::row` indexes `Page::layout()`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HRow {
    Nav,
    Hero,
    Resume,
    NextUp,
    Latest(usize),
}

struct Page {
    pid: u64,
    view: View,
    media: Option<Media>,
    // Home: libraries + a "latest" row per library. row: -1 nav, 0 hero, 1.. rows.
    libs: Load<Vec<Media>>,
    rows: Vec<HomeRow>,
    resume: Load<Vec<Media>>,
    resume_scroll: ScrollHandle,
    nextup: Load<Vec<Media>>,
    nextup_scroll: ScrollHandle,
    /// An `Admin` entry follows the libraries in the nav row.
    admin: bool,
    /// Home sections switched on in Settings: continue watching, next up, latest.
    show: (bool, bool, bool),
    row: usize,
    col: usize,
    // Grid.
    items: Load<Vec<Media>>,
    sel: usize,
    scroll: UniformListScrollHandle,
    // Detail (and Home) vertical scroll.
    page_scroll: ScrollHandle,
    seasons: Load<Vec<Media>>,
    season_sel: usize,
    /// Episodes (series/season), tracks, members: the page's main list.
    episodes: Load<Vec<Media>>,
    ep_sel: usize,
    /// Library / collection browsing state (Grid pages).
    br: Option<Box<Browse>>,
    // Detail extras.
    full: Option<Full>,
    similar: Load<Vec<Media>>,
    extras: Load<Vec<Media>>,
    kids: Load<Vec<Media>>,
    kids2: Load<Vec<Media>>,
    neigh: (Option<Media>, Option<Media>),
    /// Focused detail row / column (see `ui_detail::DRow`).
    drow: usize,
    dcol: usize,
    dscroll: [ScrollHandle; 8],
    /// Episode-card columns as last drawn.
    dcols: usize,
}

impl Page {
    fn new(
        pid: u64,
        view: View,
        media: Option<Media>,
        admin: bool,
        show: (bool, bool, bool),
    ) -> Self {
        Self {
            pid,
            view,
            media,
            libs: Load::Loading,
            rows: vec![],
            resume: Load::Loading,
            resume_scroll: ScrollHandle::new(),
            nextup: Load::Loading,
            nextup_scroll: ScrollHandle::new(),
            admin,
            show,
            row: 1,
            col: 0,
            items: Load::Loading,
            sel: 0,
            scroll: UniformListScrollHandle::new(),
            page_scroll: ScrollHandle::new(),
            seasons: Load::Loading,
            season_sel: 0,
            episodes: Load::Loading,
            ep_sel: 0,
            br: None,
            full: None,
            similar: Load::Loading,
            extras: Load::Loading,
            kids: Load::Loading,
            kids2: Load::Loading,
            neigh: (None, None),
            drow: 0,
            dcol: 0,
            dscroll: Default::default(),
            dcols: 3,
        }
    }

    /// Home was reached with the mouse: the keyboard focus must not stay on a nav entry the
    /// keys left it on (the header would then light up that library instead of Home).
    fn drop_nav_focus(&mut self) {
        if self.row == 0 {
            (self.row, self.col) = (1, 0);
        }
    }

    fn layout(&self) -> Vec<HRow> {
        let mut v = vec![HRow::Nav, HRow::Hero];
        if self.show.0 && !self.resume.items().is_empty() {
            v.push(HRow::Resume);
        }
        if self.show.1 && !self.nextup.items().is_empty() {
            v.push(HRow::NextUp);
        }
        if self.show.2 {
            v.extend((0..self.rows.len()).map(HRow::Latest));
        }
        v
    }

    fn row_items(&self, k: HRow) -> &[Media] {
        match k {
            HRow::Nav => self.libs.items(),
            HRow::Hero => &[],
            HRow::Resume => self.resume.items(),
            HRow::NextUp => self.nextup.items(),
            HRow::Latest(i) => self.rows.get(i).map_or(&[], |r| r.items.items()),
        }
    }

    fn row_len(&self, k: HRow) -> usize {
        if k == HRow::Hero {
            2
        } else if k == HRow::Nav {
            self.libs.items().len() + GLOBALS.len() + self.admin as usize
        } else {
            self.row_items(k).len()
        }
    }

    /// Item shown in the hero: the focused poster, else the newest of the first row.
    fn hero_item(&self) -> Option<Media> {
        let k = self.layout().get(self.row).copied()?;
        let focused = matches!(k, HRow::Resume | HRow::NextUp | HRow::Latest(_))
            .then(|| self.row_items(k).get(self.col).cloned())
            .flatten();
        focused.or_else(|| {
            self.rows
                .iter()
                .find_map(|r| r.items.items().first().cloned())
        })
    }
}

enum Act {
    PlayHero,
    Play(Media),
    Open(Media),
}

enum Phase {
    /// Login mode: the account screens (connect, sign in, restore) own the window.
    Account,
    /// "Who's watching?": several remembered accounts and no default (or Switch profile).
    Picker,
    Connecting,
    Error(ConnectError),
    Ready,
}

pub struct Root {
    cfg: Option<nocturne_jellyfin::Config>,
    source: Option<Source>,
    phase: Phase,
    stack: Vec<Page>,
    next_pid: u64,
    images: Cache<String, Img>,
    pub focus: FocusHandle,
    admin: Option<Entity<Admin>>,
    _admin_sub: Option<Subscription>,
    admin_open: bool,
    /// The player page, while something plays.
    player: Option<Entity<PlayerView>>,
    _player_sub: Option<Subscription>,
    /// Leave fullscreen on the next render (the player entered it).
    fs_exit: bool,
    grain: Arc<Image>,
    dots: Arc<Image>,
    glow: Arc<Image>,
    cols: usize,
    win: Size<Pixels>,
    note: Option<SharedString>,
    note_gen: u64,
    /// Developer-token mode (`NOCTURNE_JF_TOKEN`): no login, read-only.
    dev: bool,
    accounts: Option<Entity<Accounts>>,
    session: Option<(String, Session)>,
    hooks: Arc<SyncHooks>,
    reporter: Reporter,
    settings: Settings,
    settings_open: bool,
    settings_sel: usize,
    /// Open Settings drop-down and its scroll.
    dd: Option<Dropdown>,
    /// Entry still to be scrolled into view once the open menu has been laid out.
    dd_reveal: std::cell::Cell<Option<usize>>,
    dd_scroll: ScrollHandle,
    set_scroll: ScrollHandle,
    menu: bool,
    menu_sel: usize,
    audio_queue: ui_playlists::AudioQueue,
    other_accounts: Vec<nocturne_session::Account>,
    /// The profile picker, while it is (or can be returned to) on screen.
    picker: Option<Picker>,
    /// Token-less source that only fetches the profile pictures shown by the picker.
    anon_src: Option<Source>,
    /// `PrimaryImageTag` of the signed-in user (None: no profile picture, or not known yet).
    my_tag: Option<String>,
    /// Pages left by Back, restored by Forward (cleared by any new navigation).
    fwd: Vec<Page>,
    blurs: Cache<String, Arc<Image>>,
    search: Option<Search>,
    search_sel: usize,
    search_scroll: ScrollHandle,
    disk: Option<Arc<DiskCache>>,
    /// The server stopped answering: banner, retry with backoff, cached pages stay up.
    offline: Option<Offline>,
    /// Detail page More menu: selected entry; and the media info panel.
    more: Option<usize>,
    info_open: bool,
    /// Photo viewer: the photos to step through, the shown one and the zoom factor.
    photo: Option<(Vec<Media>, usize, f32)>,
    /// Shortcut reference overlay (`?`).
    help_open: bool,
    /// This account's own sessions overlay (account menu).
    devices: Option<Load<Vec<nocturne_jellyfin_user::OwnSession>>>,
    /// Watch together (SyncPlay).
    sp: Sp,
}

/// Reconnect state while the server is unreachable.
struct Offline {
    /// Failed reconnect attempts so far (drives the backoff).
    tries: u32,
    next: Instant,
    probing: bool,
}

/// Why switching to a remembered account failed.
enum SwitchFail {
    /// No token, or the server no longer accepts it: sign in again.
    Gone,
    Offline,
}

/// Account menu entries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AcctAct {
    Settings,
    Password,
    Authorize,
    Devices,
    Together,
    Switch,
    SignOut,
    /// Switch to remembered account `i` (index into `Root::other_accounts`).
    SwitchTo(usize),
    /// Back to the "Who's watching?" screen (stays signed in until a profile is chosen).
    SwitchProfile,
    AddAccount,
    PlayQueue,
    ClearQueue,
    ProfileImage,
    RemoveImage,
    Language,
}

/// Image cell chrome: padding both sides + caption block, used for row heights.
const CELL_EXTRA: f32 = 2.0 * t::S3 + t::S3 + 20.0 + 2.0 + 16.0 + 1.0;

impl Root {
    pub fn new(
        cfg: Option<nocturne_jellyfin::Config>,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut me = Self {
            cfg,
            source: None,
            phase: Phase::Connecting,
            stack: vec![],
            next_pid: 0,
            images: Cache::new(600),
            focus: cx.focus_handle(),
            admin: None,
            _admin_sub: None,
            admin_open: false,
            player: None,
            _player_sub: None,
            fs_exit: false,
            grain: ui::grain(),
            dots: ui::dots(),
            glow: ui::glow(),
            cols: 4,
            win: size(px(1280.0), px(800.0)),
            note: None,
            sp: Sp::new(),
            note_gen: 0,
            dev: false,
            accounts: None,
            session: None,
            my_tag: None,
            hooks: Arc::new(SyncHooks::new(None, false, false)),
            reporter: Reporter::default(),
            settings: Settings::load(),
            settings_open: false,
            settings_sel: 0,
            dd: None,
            dd_reveal: Default::default(),
            dd_scroll: ScrollHandle::new(),
            set_scroll: ScrollHandle::new(),
            menu: false,
            menu_sel: 0,
            audio_queue: Default::default(),
            other_accounts: vec![],
            picker: None,
            anon_src: None,
            fwd: vec![],
            blurs: Cache::new(400),
            search: None,
            search_sel: 0,
            search_scroll: ScrollHandle::new(),
            disk: None,
            offline: None,
            more: None,
            info_open: false,
            photo: None,
            help_open: false,
            devices: None,
        };
        me.disk = DiskCache::default_dir().map(|d| {
            Arc::new(DiskCache::open(
                d,
                u64::from(me.settings.image_cache_mb) * 1024 * 1024,
            ))
        });
        me.dev = me.cfg.is_some();
        if me.dev {
            me.connect(cx);
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
            let acct = cx.new(|cx| Accounts::new(matches!(start, Start::Restore), cx));
            cx.subscribe(&acct, |t, _, ev: &account::Event, cx| match ev {
                account::Event::SignedIn { url, session } => {
                    t.activate(url.clone(), session.clone(), cx)
                }
                account::Event::Notice(n) => t.toast(*n, cx),
                account::Event::Reload => {
                    t.refresh_profile(cx);
                    t.retry_page(cx)
                }
                account::Event::Deleted => {
                    t.back();
                    t.retry_page(cx);
                }
                account::Event::BackToPicker => t.show_picker(cx),
            })
            .detach();
            me.accounts = Some(acct);
            if let (Some(server), true) = (last, !matches!(start, Start::Restore)) {
                me.picker = Some(Picker::new(&server, list.clone(), None, 1));
                match start {
                    Start::Direct(i) => {
                        me.phase = Phase::Connecting;
                        me.switch_account(list[i].clone(), true, DefaultChange::Keep, cx);
                    }
                    _ => me.show_picker(cx),
                }
            }
        }
        // Teardown order: the video subsurface must go before the window, and not from inside the
        // compositor's close event (joining the render thread there deadlocks). So while a video
        // is up, veto the close, drop the video from a task, then remove the window ourselves.
        let this = cx.weak_entity();
        _w.on_window_should_close(cx, move |w, cx| {
            let handle = Window::window_handle(w);
            let this = this.clone();
            // Settings are saved on every change; one last save covers anything pending.
            let idle = this
                .read_with(cx, |t, _| {
                    t.settings.save();
                    t.player.is_none() && !t.sp.engine.in_group()
                })
                .unwrap_or(true);
            if idle {
                return true;
            }
            cx.spawn(async move |cx| {
                this.update(cx, |t, cx| {
                    t.report_stop();
                    t.sp_shutdown(cx);
                    if let Some(p) = t.player.take() {
                        p.update(cx, |p, _| p.shutdown());
                    }
                })
                .ok();
                // Let the final progress report leave before the process exits.
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                cx.update(|cx| cx.quit()).ok();
                let _ = handle;
            })
            .detach();
            false
        });
        // Reporting and failed-write reverts, a few times a second.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                if this.update(cx, |t, cx| t.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        me
    }

    /// Runs `fut` on the tokio bridge, then applies the result on the UI thread.
    fn run<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        fut: impl Future<Output = T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let h = bridge::spawn(fut);
        cx.spawn(async move |this, cx| {
            if let Ok(v) = h.await {
                this.update(cx, |t, cx| {
                    done(t, v, cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Connecting;
        let cfg = self.cfg.clone();
        self.run(cx, Source::connect(cfg), |t, r, cx| match r {
            Ok(s) => {
                t.source = Some(match t.disk.clone() {
                    Some(d) => s.with_disk(d),
                    None => s,
                });
                t.phase = Phase::Ready;
                t.push(View::Home, None, cx);
                t.run(cx, admin::probe(t.cfg.clone()), |t, c, cx| {
                    t.admin = c.map(|c| t.make_admin(c, cx));
                    t.apply_write_settings(cx);
                    for p in &mut t.stack {
                        p.admin = t.admin.is_some();
                    }
                });
            }
            Err(e) => t.phase = Phase::Error(e),
        });
    }

    // ---- accounts and sync ----

    /// A login is ready (typed, Quick Connect or restored): build the source and show Home.
    fn activate(&mut self, url: String, s: Session, cx: &mut Context<Self>) {
        let cfg = nocturne_jellyfin::Config {
            base_url: url.clone(),
            token: s.access_token.clone(),
        };
        let Ok(src) = Source::with_user(cfg.clone(), &s.user_id) else {
            return self.toast("This server address can't be used.", cx);
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
        // The admin section follows the account's own rights, with the session's token.
        self.admin = s
            .is_admin
            .then(|| AdminClient::new(cfg).ok())
            .flatten()
            .map(|c| self.make_admin(c, cx));
        self.apply_write_settings(cx);
        self.session = Some((url, s));
        self.my_tag = None;
        self.refresh_profile(cx);
        self.phase = Phase::Ready;
        self.push(View::Home, None, cx);
        if self
            .accounts
            .as_ref()
            .is_some_and(|a| a.read(cx).keyring_down())
        {
            self.toast(account::KEYRING_NOTE, cx);
        }
    }

    /// Read the own profile picture tag (`GET /Users/Me`), then the picture itself. Runs on sign-in,
    /// account switch and after the profile image is set or removed, so the avatar never goes stale.
    fn refresh_profile(&mut self, cx: &mut Context<Self>) {
        let Some((url, s)) = self.session.as_ref() else {
            return;
        };
        let (Ok(c), uid) = (UserClient::from_session(url, s), s.user_id.clone()) else {
            return;
        };
        self.run(
            cx,
            async move { c.profile().await.ok() },
            move |t, p, cx| {
                // Switched away meanwhile, or the server did not answer: keep what is shown.
                let same = t.session.as_ref().is_some_and(|(_, s)| s.user_id == uid);
                if let (true, Some(p)) = (same, p) {
                    t.my_tag = p.primary_image_tag;
                    if let Some((url, _)) = &t.session {
                        nocturne_session::remember_image_tag(url, &uid, t.my_tag.clone());
                    }
                    t.load_user_pic(&uid, t.my_tag.clone(), cx);
                }
            },
        );
    }

    /// Start fetching a user's profile picture (once per tag; a failure is retried later).
    fn load_user_pic(&mut self, uid: &str, tag: Option<String>, cx: &mut Context<Self>) {
        let (Some(tag), Some(src)) = (tag, self.source.clone().or_else(|| self.anon_src.clone()))
        else {
            return;
        };
        let key = user_pic_key(uid, &tag);
        match self.images.get(&key) {
            Some(Img::Ready(_) | Img::Loading) => return,
            Some(Img::Failed(at)) if at.elapsed() < RETRY_FAILED => return,
            _ => {}
        }
        self.images.insert(key.clone(), Img::Loading);
        let uid = uid.to_string();
        self.run(
            cx,
            async move { src.user_image(&uid, &tag, AVATAR_PX).await },
            move |t, r, _| {
                let state = match r
                    .ok()
                    .and_then(|b| sniff(&b).map(|f| Image::from_bytes(f, b)))
                {
                    Some(i) => Img::Ready(Arc::new(i)),
                    None => Img::Failed(Instant::now()),
                };
                t.images.insert(key, state);
            },
        );
    }

    /// The loaded profile picture, if any; otherwise the caller shows the initial.
    fn user_pic(&self, uid: &str, tag: Option<&str>) -> Option<Arc<Image>> {
        ready_pic(&self.images, uid, tag)
    }

    /// Back to the account screens: stop playback, drop everything tied to the login.
    fn leave_session(&mut self, cx: &mut Context<Self>) {
        self.sp_shutdown(cx);
        self.close_player(cx);
        self.source = None;
        self.stack.clear();
        self.admin = None;
        self.admin_open = false;
        self.settings_open = false;
        self.menu = false;
        self.session = None;
        self.my_tag = None;
        self.more = None;
        self.info_open = false;
        self.photo = None;
        self.hooks = Arc::new(SyncHooks::new(None, false, false));
        self.phase = Phase::Account;
    }

    fn sign_out(&mut self, cx: &mut Context<Self>) {
        let client = self
            .session
            .as_ref()
            .and_then(|(u, s)| UserClient::from_session(u, s).ok());
        let uid = self.session.as_ref().map(|(_, s)| s.user_id.clone());
        self.leave_session(cx);
        if let Some(a) = self.accounts.clone() {
            a.update(cx, |a, cx| a.sign_out(client, uid, cx));
        }
    }

    fn switch_server(&mut self, cx: &mut Context<Self>) {
        self.leave_session(cx);
        if let Some(a) = self.accounts.clone() {
            a.update(cx, |a, cx| a.switch_server(cx));
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
            if !self.audio_queue.items.is_empty() {
                let n = self.audio_queue.items.len();
                v.push((format!("Play music queue ({n})"), AcctAct::PlayQueue));
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
    fn refresh_accounts(&mut self, cx: &mut Context<Self>) {
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
        for (uid, tag) in self
            .other_accounts
            .iter()
            .map(|a| (a.user_id.clone(), a.image_tag.clone()))
            .collect::<Vec<_>>()
        {
            self.load_user_pic(&uid, tag, cx);
        }
    }

    /// Switch to a remembered account of the account menu.
    fn switch_to(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(acct) = self.other_accounts.get(i).cloned() {
            self.switch_account(acct, false, DefaultChange::Keep, cx);
        }
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
        cx: &mut Context<Self>,
    ) {
        if matches!(self.phase, Phase::Ready) {
            self.toast("Switching account.", cx);
        }
        if let Some(p) = &mut self.picker {
            p.busy = true;
        }
        let a2 = acct.clone();
        self.run(
            cx,
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
            move |t, r, cx| match r {
                Ok(s) => {
                    t.leave_session(cx);
                    t.picker = None;
                    t.anon_src = None;
                    let (u, s2) = (acct.server.clone(), s.clone());
                    // Make it the active login (keyring) so a relaunch restores this account.
                    bridge::spawn(async move {
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
                    t.activate(acct.server.clone(), s, cx);
                }
                Err(SwitchFail::Gone) if via_picker => {
                    t.phase = Phase::Account;
                    if let Some(a) = t.accounts.clone() {
                        a.update(cx, |a, cx| {
                            a.sign_in_form(&acct.server, Some(acct.user_name.clone()), cx)
                        });
                    }
                }
                Err(SwitchFail::Gone) => {
                    let (u, id) = (acct.server.clone(), acct.user_id.clone());
                    bridge::spawn(async move {
                        let _ = tokio::task::spawn_blocking(move || {
                            nocturne_session::forget_account(&u, &id)
                        })
                        .await;
                    });
                    t.toast(
                        "That account's sign-in has expired. Add it again with Add another account.",
                        cx,
                    );
                }
                Err(SwitchFail::Offline) => {
                    t.toast("Couldn't reach the server. Try again.", cx);
                    if t.picker.is_some() {
                        t.show_picker(cx); // also covers a failed launch with a default profile
                    }
                }
            },
        );
    }

    /// Show "Who's watching?" for `picker` (kept up to date with the window width) and fetch the
    /// pictures the server has for its profiles.
    fn show_picker(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &mut self.picker else {
            return;
        };
        p.busy = false;
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
        for (uid, tag) in pics {
            self.load_user_pic(&uid, tag, cx);
        }
        cx.notify();
    }

    /// Menu "Switch profile": leave the session (the login stays remembered) and ask who is watching.
    fn switch_profile(&mut self, cx: &mut Context<Self>) {
        let Some((url, s)) = self.session.clone() else {
            return;
        };
        let list = picker::accounts_of(
            &nocturne_session::Accounts::load().unwrap_or_default(),
            &url,
        );
        if list.len() < 2 {
            return;
        }
        self.leave_session(cx);
        self.picker = Some(Picker::new(&url, list, Some(&s.user_id), 1));
        self.show_picker(cx);
    }

    fn picker_act(&mut self, act: picker::Act, cx: &mut Context<Self>) {
        use picker::Act;
        let Some(p) = &mut self.picker else {
            return;
        };
        match act {
            Act::Stay => {}
            Act::Toggle if !p.busy => p.toggle(),
            Act::Toggle => {}
            Act::Add if !p.busy => {
                let server = p.server.clone();
                self.phase = Phase::Account;
                if let Some(a) = self.accounts.clone() {
                    a.update(cx, |a, cx| a.sign_in_form(&server, None, cx));
                }
            }
            Act::Add => {}
            Act::Pick(i) => {
                if p.busy {
                    return;
                }
                p.sel = i;
                let (acct, change) = (p.accounts[i].clone(), p.default_change(i));
                self.switch_account(acct, true, change, cx);
            }
        }
        cx.notify();
    }

    fn picker_key(&mut self, key: &str, cx: &mut Context<Self>) {
        let cols = picker::columns(f32::from(self.win.width));
        if let Some(p) = &mut self.picker {
            p.cols = cols;
            let act = p.key(key);
            self.picker_act(act, cx);
        }
        cx.notify();
    }

    /// Shortcut help sits above everything: drop open menus first.
    fn close_admin_menus(&mut self) {
        self.menu = false;
        self.more = None;
        self.info_open = false;
    }

    fn menu_act(&mut self, act: AcctAct, cx: &mut Context<Self>) {
        self.menu = false;
        match act {
            AcctAct::Settings => {
                self.close_admin(cx);
                self.refresh_profiles();
                self.settings_open = true;
                self.settings_sel = 0;
                self.dd = None;
            }
            AcctAct::Devices => self.open_devices(cx),
            AcctAct::Together => self.sp_open_screen(cx),
            AcctAct::Authorize
            | AcctAct::Password
            | AcctAct::ProfileImage
            | AcctAct::RemoveImage
            | AcctAct::Language
                if !self.hooks.can_write() =>
            {
                self.toast(self.writes_off(), cx);
            }
            AcctAct::ProfileImage | AcctAct::RemoveImage | AcctAct::Language => {
                let client = self
                    .session
                    .as_ref()
                    .and_then(|(u, s)| UserClient::from_session(u, s).ok());
                if let (Some(c), Some(a)) = (client, self.accounts.clone()) {
                    if act == AcctAct::Language {
                        // Show what is set now, then let the user change it.
                        let cc = c.clone();
                        self.run(
                            cx,
                            async move { cc.display_language().await.ok().flatten() },
                            move |t, cur, cx| {
                                if let Some(a) = t.accounts.clone() {
                                    a.update(cx, |a, cx| {
                                        a.open_language(c, &cur.unwrap_or_default(), cx)
                                    });
                                }
                            },
                        );
                    } else {
                        a.update(cx, |a, cx| {
                            if act == AcctAct::ProfileImage {
                                a.open_profile_image(c, cx)
                            } else {
                                a.open_remove_image(c, cx)
                            }
                        });
                    }
                }
            }
            AcctAct::SwitchTo(i) => self.switch_to(i, cx),
            AcctAct::SwitchProfile => self.switch_profile(cx),
            AcctAct::PlayQueue => self.play_queue(cx),
            AcctAct::ClearQueue => self.clear_queue(cx),
            AcctAct::AddAccount => {
                self.leave_session(cx);
                if let Some(a) = self.accounts.clone() {
                    a.update(cx, |a, cx| a.add_account(cx));
                }
            }
            AcctAct::Password | AcctAct::Authorize => {
                let client = self
                    .session
                    .as_ref()
                    .and_then(|(u, s)| UserClient::from_session(u, s).ok());
                if let (Some(c), Some(a)) = (client, self.accounts.clone()) {
                    a.update(cx, |a, cx| {
                        if act == AcctAct::Authorize {
                            a.open_authorize(c, cx)
                        } else {
                            a.open_password(c, cx)
                        }
                    });
                }
            }
            AcctAct::Switch => self.switch_server(cx),
            AcctAct::SignOut => self.sign_out(cx),
        }
        cx.notify();
    }

    /// Push the master and sync switches to the hooks and the admin section.
    fn apply_write_settings(&mut self, cx: &mut Context<Self>) {
        use std::sync::atomic::Ordering::Relaxed;
        self.hooks
            .master
            .store(self.settings.allow_server_writes, Relaxed);
        self.hooks.sync.store(self.settings.sync_progress, Relaxed);
        let w = self.hooks.can_write();
        if let Some(a) = self.admin.clone() {
            a.update(cx, |a, cx| {
                a.writes = w;
                cx.notify();
            });
        }
        self.sp_writes_changed(cx);
    }

    /// An admin client for this login, view-only until the master switch says otherwise.
    fn make_admin(&mut self, c: AdminClient, cx: &mut Context<Self>) -> Entity<Admin> {
        let src = self.source.clone();
        let a = cx.new(|_| Admin::new(c).with_pics(src));
        self._admin_sub = Some(cx.subscribe(&a, |t, _, _: &admin::OpenSettings, cx| {
            t.menu_act(AcctAct::Settings, cx);
        }));
        a
    }

    /// Settings row `i` stepped; applies at once and is saved.
    fn cycle_setting(&mut self, i: usize, dir: i32, cx: &mut Context<Self>) {
        if i == settings::PROFILE_ROW {
            let n = self.settings.profiles.list.len() + 1;
            if n > 2 {
                let at = (self.settings.profiles.current() + n)
                    .wrapping_add_signed(dir.signum() as isize)
                    % n;
                self.set_default_profile(at, cx);
            }
            return;
        }
        self.settings.cycle(i, dir);
        self.settings_changed(cx);
    }

    /// Entry `idx` of row `i`'s drop-down chosen.
    fn choose_setting(&mut self, i: usize, idx: usize, cx: &mut Context<Self>) {
        if i == settings::PROFILE_ROW {
            return self.set_default_profile(idx, cx);
        }
        self.settings.choose(i, idx);
        self.settings_changed(cx);
    }

    /// Remembered profiles of this server and the default one, for the Settings row.
    fn refresh_profiles(&mut self) {
        self.settings.profiles = match &self.session {
            Some((url, _)) if !self.dev => {
                let all = nocturne_session::Accounts::load().unwrap_or_default();
                settings::Profiles {
                    list: all
                        .for_server(url)
                        .iter()
                        .map(|a| (a.user_id.clone(), a.user_name.clone()))
                        .collect(),
                    default: all.default_for(url).map(|a| a.user_id.clone()),
                }
            }
            _ => settings::Profiles::default(),
        };
    }

    /// "Default profile" drop-down entry `idx`: 0 asks every time, otherwise that profile opens at launch.
    fn set_default_profile(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some((url, _)) = &self.session {
            let id = self.settings.profiles.id_at(idx).map(str::to_string);
            nocturne_session::remember_default(url, id.as_deref());
        }
        self.refresh_profiles();
        cx.notify();
    }

    fn settings_changed(&mut self, cx: &mut Context<Self>) {
        self.settings.save();
        self.apply_write_settings(cx);
        let show = self.settings.home();
        for p in &mut self.stack {
            p.show = show;
            p.row = p.row.min(p.layout().len() - 1);
        }
        cx.notify();
    }

    /// Open the drop-down of a multi-choice row (a toggle row is flipped instead).
    fn open_setting(&mut self, i: usize, cx: &mut Context<Self>) {
        self.settings_sel = i;
        self.set_scroll.scroll_to_item(settings::scroll_child(i));
        let Some((labels, cur)) = self.settings.choices(i) else {
            return self.cycle_setting(i, 1, cx);
        };
        self.dd = Dropdown::open(i, labels.len(), cur);
        self.dd_reveal.set(Some(cur));
        cx.notify();
    }

    fn settings_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(d) = &mut self.dd {
            match d.key(key) {
                Outcome::Stay => self.dd_scroll.scroll_to_item(d.sel),
                Outcome::Close => self.dd = None,
                Outcome::Pick(idx) => {
                    let row = d.row;
                    self.dd = None;
                    self.choose_setting(row, idx, cx);
                }
            }
            return cx.notify();
        }
        match key {
            "escape" | "backspace" => self.settings_open = false,
            "up" | "down" | "pageup" | "pagedown" | "home" | "end" => {
                if let Some(r) = settings::nav_row(self.settings_sel, key) {
                    self.settings_sel = r;
                    self.set_scroll.scroll_to_item(settings::scroll_child(r));
                }
            }
            "left" => self.cycle_setting(self.settings_sel, -1, cx),
            "right" => self.cycle_setting(self.settings_sel, 1, cx),
            "enter" | "space" => self.open_setting(self.settings_sel, cx),
            _ => {}
        }
        cx.notify();
    }

    fn menu_key(&mut self, key: &str, cx: &mut Context<Self>) {
        let items = self.menu_items();
        match key {
            "escape" | "a" => self.menu = false,
            "up" => self.menu_sel = self.menu_sel.saturating_sub(1),
            "down" => self.menu_sel = (self.menu_sel + 1).min(items.len() - 1),
            "enter" => {
                if let Some((_, act)) = items.get(self.menu_sel) {
                    self.menu_act(*act, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// Apply `f` to every copy of item `id` the UI holds, so optimistic changes show everywhere.
    fn patch(&mut self, id: &str, f: impl Fn(&mut Media)) {
        let hit = |m: &mut Media| {
            if m.id == id {
                f(m)
            }
        };
        let list = |l: &mut Load<Vec<Media>>| {
            if let Load::Ready(v) = l {
                v.iter_mut().for_each(hit);
            }
        };
        for p in &mut self.stack {
            p.media.iter_mut().for_each(hit);
            for l in [
                &mut p.resume,
                &mut p.nextup,
                &mut p.items,
                &mut p.seasons,
                &mut p.episodes,
                &mut p.similar,
                &mut p.extras,
                &mut p.kids,
                &mut p.kids2,
            ] {
                list(l);
            }
            if let Some(br) = p.br.as_deref_mut() {
                br.slots.iter_mut().flatten().for_each(hit);
            }
            p.rows.iter_mut().for_each(|r| list(&mut r.items));
        }
    }

    /// Calm explanation for a refused write.
    fn writes_off(&self) -> &'static str {
        if self.dev {
            "This connection is read-only."
        } else {
            "Server changes are off. Turn them on in Settings to do that."
        }
    }

    fn toggle_played(&mut self, m: &Media, cx: &mut Context<Self>) {
        if !self.hooks.can_write() {
            return self.toast(self.writes_off(), cx);
        }
        let v = !m.played;
        self.patch(&m.id, |x| {
            x.played = v;
            if v {
                x.resume_s = None;
            }
        });
        self.hooks.set_played(&m.id, v);
        self.toast(
            if v {
                "Marked as watched."
            } else {
                "Marked as unwatched."
            },
            cx,
        );
    }

    fn toggle_fav(&mut self, m: &Media, cx: &mut Context<Self>) {
        if !self.hooks.can_write() {
            return self.toast(self.writes_off(), cx);
        }
        let v = !m.favorite;
        self.hooks.seed_favorite(&m.id, m.favorite);
        self.hooks.set_favorite(&m.id, v);
        self.patch(&m.id, |x| x.favorite = v);
        self.toast(
            if v {
                "Added to favourites."
            } else {
                "Removed from favourites."
            },
            cx,
        );
    }

    /// Failed writes come back from the bridge: undo the optimistic change and say so calmly.
    fn drain_reverts(&mut self, cx: &mut Context<Self>) {
        for r in self.hooks.drain() {
            self.patch(&r.id, |m| {
                m.played = r.played.unwrap_or(m.played);
                m.favorite = r.fav.unwrap_or(m.favorite);
            });
            self.toast("Couldn't save that change, so it was undone.", cx);
            cx.notify();
        }
    }

    /// The item under the focus ring / selection on the current page.
    fn focus_media(&self) -> Option<Media> {
        let p = self.stack.last()?;
        match p.view {
            View::Home => match p.layout().get(p.row).copied()? {
                HRow::Hero => p.hero_item(),
                HRow::Nav => None,
                k => p.row_items(k).get(p.col).cloned(),
            },
            View::Grid => p.br.as_deref()?.item(p.sel).cloned(),
            View::Detail => {
                let rows = p.drows();
                match rows.get(p.drow) {
                    Some(DRow::List) => p.episodes.items().get(p.ep_sel).cloned(),
                    _ => p.media.clone(),
                }
            }
        }
    }

    /// After playback the Home rows move (a new Continue Watching entry, a next episode):
    /// look again shortly, once the server has the final report.
    fn refresh_resume(&mut self, cx: &mut Context<Self>) {
        let (Some(src), Some(pid)) = (self.source.clone(), self.stack.first().map(|p| p.pid))
        else {
            return;
        };
        if !self.hooks.sync_on() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(900))
                .await;
            let (s1, s2) = (src.clone(), src);
            let h = bridge::spawn(async move { (s1.resume().await, s2.next_up().await) });
            if let Ok((r, n)) = h.await {
                this.update(cx, |t, cx| {
                    if let Some(p) = t.page_mut(pid) {
                        p.resume = r.map_or(Load::Failed, Load::Ready);
                        p.nextup = n.map_or(Load::Failed, Load::Ready);
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if crate::TERM.load(std::sync::atomic::Ordering::Relaxed) {
            return cx.quit();
        }
        self.drain_reverts(cx);
        self.sync_tick(cx);
        self.watch_connection(cx);
    }

    /// 401 mid-session goes back to sign-in; no answer shows the banner and reconnects with backoff.
    fn watch_connection(&mut self, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        // The admin client has its own connection; its 401 means the same token is dead.
        if self.admin.as_ref().is_some_and(|a| a.read(cx).rejected()) {
            return self.session_rejected(cx);
        }
        match src.health() {
            nocturne_jellyfin::Health::Unauthorized => self.session_rejected(cx),
            nocturne_jellyfin::Health::Unreachable => {
                let off = self.offline.get_or_insert_with(|| Offline {
                    tries: 0,
                    next: Instant::now() + Duration::from_secs(crate::sync::backoff_secs(0)),
                    probing: false,
                });
                cx.notify();
                if off.probing || Instant::now() < off.next {
                    return;
                }
                off.probing = true;
                self.run(cx, async move { src.ping().await }, |t, ok, _| {
                    if let Some(o) = t.offline.as_mut() {
                        o.probing = false;
                        o.tries += 1;
                        o.next = Instant::now()
                            + Duration::from_secs(crate::sync::backoff_secs(o.tries));
                    }
                    let _ = ok; // success shows up as Health::Ok on the next tick
                });
            }
            nocturne_jellyfin::Health::Ok => {
                if self.offline.take().is_some() {
                    self.toast("Back online.", cx);
                    self.retry_page(cx);
                    cx.notify();
                }
            }
        }
    }

    /// The token is no longer valid: drop the session and say so calmly.
    fn session_rejected(&mut self, cx: &mut Context<Self>) {
        self.offline = None;
        if self.session.is_none() {
            // Developer-token mode has no login to return to.
            self.source = None;
            self.phase = Phase::Error(ConnectError::Rejected);
            return cx.notify();
        }
        let uid = self.session.as_ref().map(|(_, s)| s.user_id.clone());
        self.leave_session(cx);
        if let Some(a) = self.accounts.clone() {
            a.update(cx, |a, cx| a.session_expired(uid, cx));
        }
        cx.notify();
    }

    /// Load the current page again (after a failure or a reconnect).
    fn retry_page(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.stack.last() else { return };
        let (pid, view, media) = (p.pid, p.view, p.media.clone());
        match (view, media) {
            (View::Home, _) => self.load_home(pid, cx),
            (View::Grid, _) => self.browse_reload(pid, cx),
            (View::Detail, Some(m)) => self.load_detail(pid, m, cx),
            _ => {}
        }
    }

    /// A centred message with a way to try again.
    fn failed_view(&self, msg: &str, cx: &mut Context<Self>) -> Div {
        self.centered()
            .child(
                div()
                    .text_size(px(t::BODY))
                    .text_color(t::text_secondary())
                    .child(msg.to_string()),
            )
            .child(ui::pill_button(
                "retry-page",
                "Try again",
                true,
                false,
                cx.listener(|t, _, _, cx| {
                    t.retry_page(cx);
                    cx.notify();
                }),
            ))
    }

    /// Non-blocking: the page underneath stays usable.
    fn offline_banner(&self, cx: &mut Context<Self>) -> Div {
        let wait = self.offline.as_ref().map_or(0, |o| {
            o.next.saturating_duration_since(Instant::now()).as_secs() + 1
        });
        div()
            .absolute()
            .top(px(t::HEADER_H + t::S2))
            .left_0()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .px(px(t::S4))
                    .py(px(t::S2))
                    .flex()
                    .items_center()
                    .gap(px(t::S4))
                    .rounded(px(t::R_PANEL))
                    .bg(t::raised())
                    .border_1()
                    .border_color(t::hairline_strong())
                    .text_size(px(t::BODY))
                    .child(format!(
                        "Can't reach your server. Showing what's already loaded. Retrying in {wait}s."
                    ))
                    .child(ui::action_button(
                        "offline-retry".into(),
                        "Retry now",
                        true,
                        false,
                        false,
                        false,
                        cx.listener(|t, _, _, cx| {
                            if let Some(o) = t.offline.as_mut() {
                                o.next = Instant::now();
                            }
                            cx.notify();
                        }),
                    )),
            )
    }

    /// Progress reporting: started once, every 10 s, and on pause/resume/seek. Gated by Settings.
    fn sync_tick(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.player.clone() else {
            return;
        };
        let (id, pos, paused) = {
            let p = p.read(cx);
            let pos = p.state.as_ref().and_then(|s| s.position);
            match (p.loaded, pos) {
                (true, Some(pos)) => (p.media.id.clone(), pos, p.paused()),
                _ => return,
            }
        };
        // The queue moved on: the previous item ends before the next one starts.
        if self.reporter.id().is_some_and(|r| r != id) {
            self.report_stop();
        }
        match self.reporter.tick(Instant::now(), &id, pos, paused) {
            Some(Tick::Started) => self.hooks.started(&id, ticks(pos)),
            Some(Tick::Progress) => self.hooks.progress(&id, ticks(pos), paused),
            None => {}
        }
    }

    fn report_stop(&mut self) {
        if let Some((id, pos)) = self.reporter.stop() {
            self.hooks.stopped(&id, ticks(pos));
        }
    }

    /// Ends playback now (sign-out, switch server): report, release the player.
    fn close_player(&mut self, cx: &mut Context<Self>) {
        self.report_stop();
        if let Some(p) = self.player.clone() {
            let q = p.read(cx).music_queue();
            self.keep_music(q);
        }
        if let Some(p) = self.player.take() {
            p.update(cx, |p, _| p.shutdown());
        }
        self._player_sub = None;
    }

    /// Heart and check chips on a poster: always shown when set, on hover otherwise.
    fn chips(&self, m: &Media, cx: &mut Context<Self>) -> Div {
        let writes = self.hooks.can_write();
        let chip = |id: String, glyph: &'static str, on: bool, t_: Media, fav: bool| {
            div()
                .id(ui::name(id))
                .size(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgba(0x07080ccc))
                .border_1()
                .border_color(if on { t::accent() } else { t::edge() })
                .text_size(px(13.0))
                .text_color(if on {
                    t::accent_text()
                } else {
                    t::text_secondary()
                })
                .cursor_pointer()
                .occlude()
                .hover(|s| s.border_color(t::accent()))
                .when(!writes, |d| d.opacity(0.4).tooltip(ui::tip(NEED_WRITES)))
                .when(!on, |d| d.invisible().group_hover("card", |s| s.visible()))
                .on_click(cx.listener(move |t, _, _, cx| {
                    if fav {
                        t.toggle_fav(&t_, cx);
                    } else {
                        t.toggle_played(&t_, cx);
                    }
                    cx.notify();
                }))
                .child(glyph)
        };
        let pad = t::S3 + t::S2;
        div()
            .absolute()
            .top(px(pad))
            .right(px(pad))
            .flex()
            .gap(px(t::S1))
            .child(chip(
                format!("fav-{}", m.id),
                if m.favorite { "\u{2665}" } else { "\u{2661}" },
                m.favorite,
                m.clone(),
                true,
            ))
            .child(chip(
                format!("seen-{}", m.id),
                "\u{2713}",
                m.played,
                m.clone(),
                false,
            ))
    }

    fn page_mut(&mut self, pid: u64) -> Option<&mut Page> {
        self.stack.iter_mut().find(|p| p.pid == pid)
    }

    /// A new page on top of the stack; whatever Back had parked is forgotten.
    fn push_page(&mut self, view: View, media: Option<Media>) -> u64 {
        self.next_pid += 1;
        let pid = self.next_pid;
        self.fwd.clear();
        self.stack.push(Page::new(
            pid,
            view,
            media,
            self.admin.is_some(),
            self.settings.home(),
        ));
        pid
    }

    /// The Home rows. Safe to call again: it starts every row over.
    fn load_home(&mut self, pid: u64, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        if let Some(p) = self.page_mut(pid) {
            (p.libs, p.resume, p.nextup) = (Load::Loading, Load::Loading, Load::Loading);
            p.rows.clear();
        }
        let s2 = src.clone();
        let s4 = src.clone();
        let s5 = src.clone();
        self.run(cx, async move { s5.next_up().await }, move |t, r, _| {
            if let Some(p) = t.page_mut(pid) {
                p.nextup = r.map_or(Load::Failed, Load::Ready);
            }
        });
        self.run(cx, async move { s4.resume().await }, move |t, r, _| {
            if let Some(p) = t.page_mut(pid) {
                p.resume = r.map_or(Load::Failed, Load::Ready);
            }
        });
        self.run(cx, async move { src.libraries().await }, move |t, r, cx| {
            let Ok(libs) = r else {
                if let Some(p) = t.page_mut(pid) {
                    p.libs = Load::Failed;
                }
                return;
            };
            if let Some(p) = t.page_mut(pid) {
                p.rows = libs
                    .iter()
                    .map(|l| HomeRow {
                        lib: l.clone(),
                        items: Load::Loading,
                        scroll: ScrollHandle::new(),
                    })
                    .collect();
                p.libs = Load::Ready(libs.clone());
            }
            for lib in libs {
                let (s3, id) = (s2.clone(), lib.id.clone());
                t.run(cx, async move { s3.latest(&lib).await }, move |t, r, _| {
                    if let Some(row) = t
                        .page_mut(pid)
                        .and_then(|p| p.rows.iter_mut().find(|x| x.lib.id == id))
                    {
                        row.items = r.map_or(Load::Failed, Load::Ready);
                    }
                });
            }
        });
    }

    fn push(&mut self, view: View, media: Option<Media>, cx: &mut Context<Self>) {
        if self.source.is_none() {
            return;
        }
        let pid = self.push_page(view, media.clone());
        match (view, media) {
            (View::Home, _) => self.load_home(pid, cx),
            (View::Detail, Some(m)) => self.load_detail(pid, m, cx),
            _ => {}
        }
    }

    fn load_episodes(&mut self, pid: u64, season_id: String, cx: &mut Context<Self>) {
        let (Some(src), Some(series)) = (
            self.source.clone(),
            self.page_mut(pid)
                .and_then(|p| p.media.as_ref().map(|m| m.id.clone())),
        ) else {
            return;
        };
        if let Some(p) = self.page_mut(pid) {
            p.episodes = Load::Loading;
            p.ep_sel = 0;
        }
        self.run(
            cx,
            async move { src.episodes(&series, &season_id).await },
            move |t, r, _| {
                if let Some(p) = t.page_mut(pid) {
                    // Start the selection on the first unwatched episode.
                    p.ep_sel = r
                        .as_ref()
                        .ok()
                        .and_then(|v| v.iter().position(|e| !e.played))
                        .unwrap_or(0);
                    p.episodes = r.map_or(Load::Failed, Load::Ready);
                }
            },
        );
    }

    fn open(&mut self, m: Media, cx: &mut Context<Self>) {
        self.open_any(m, cx);
    }

    /// Library tabs always replace whatever is above Home.
    fn open_library(&mut self, m: Media, cx: &mut Context<Self>) {
        self.stack.truncate(1);
        self.open_browse(ui_browse::scope_library(&m), cx);
    }

    fn open_admin(&mut self, cx: &mut Context<Self>) {
        if let Some(a) = self.admin.clone() {
            self.admin_open = true;
            a.update(cx, |a, cx| a.open(cx));
        }
    }

    /// Leave Admin (if open) before navigating elsewhere.
    fn close_admin(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.menu = false;
        if self.admin_open {
            self.admin_open = false;
            if let Some(a) = self.admin.clone() {
                a.update(cx, |a, _| a.close());
            }
        }
    }

    fn back(&mut self) {
        nav::back(&mut self.stack, &mut self.fwd);
    }

    fn forward(&mut self) {
        nav::forward(&mut self.stack, &mut self.fwd);
    }

    fn toast(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.note = Some(msg.into());
        self.note_gen += 1;
        let g = self.note_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(2500))
                .await;
            this.update(cx, |t, cx| {
                if t.note_gen == g {
                    t.note = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Hands one playable item to the player page, with the items queued behind it (Play all /
    /// Shuffle; empty = the player finds the next episode itself). Nothing here is ever logged.
    fn play_media(&mut self, m: Media, rest: Vec<Media>, cx: &mut Context<Self>) {
        if self.sp.engine.in_group() {
            return self.sp_play(m, rest, cx);
        }
        let Some(ctx) = self.player_ctx() else {
            return;
        };
        self.hooks.seed_favorite(&m.id, m.favorite);
        self.report_stop();
        let view = cx.new(|cx| {
            let mut v = PlayerView::new(ctx, m.clone(), cx);
            if !rest.is_empty() {
                v.queue = std::iter::once(m).chain(rest).collect();
            }
            v
        });
        self.attach_player(view, cx);
    }

    fn player_ctx(&self) -> Option<PlayerCtx> {
        let (client, user) = self.source.as_ref()?.client();
        Some(PlayerCtx {
            client,
            user,
            hooks: Some(self.hooks.clone() as Arc<dyn PlayerHooks>),
            prefs: self.settings.prefs(),
            buffering: self.settings.buffering(),
        })
    }

    /// Makes `view` the player page and reacts when it ends.
    fn attach_player(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        self._player_sub = Some(cx.subscribe(&view, |t, _, ev: &PlayerEvent, cx| {
            let PlayerEvent::Closed { failed, leave_fs } = ev;
            t.report_stop();
            t.refresh_resume(cx);
            if let Some(p) = t.player.clone() {
                let p = p.read(cx).music_queue();
                t.keep_music(p);
            }
            t.player = None;
            t._player_sub = None;
            t.fs_exit = *leave_fs;
            if *failed {
                t.toast("This couldn't be played.", cx);
            }
            t.sp_player_closed(cx);
            cx.notify();
        }));
        self.player = Some(view);
    }

    /// Play for a focused thing. A series plays its first unwatched (else first) episode.
    fn play_item(&mut self, m: Media, cx: &mut Context<Self>) {
        match m.kind {
            Kind::Library | Kind::Season => {}
            Kind::Photo => self.photo_open(m),
            Kind::Book | Kind::Folder | Kind::Person | Kind::Other => {
                self.toast("Not playable in NOCTURNE yet.", cx)
            }
            Kind::Series => {
                let Some(src) = self.source.clone() else {
                    return;
                };
                self.run(
                    cx,
                    async move { src.series_target(&m.id).await },
                    |t, r, cx| match r {
                        Ok(e) => t.play_media(e, vec![], cx),
                        Err(_) => t.toast("Nothing to play here yet.", cx),
                    },
                );
            }
            _ => self.play_media(m, vec![], cx),
        }
    }

    fn play_current(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.stack.last() else { return };
        let m = if p.view == View::Home {
            p.hero_item()
        } else if p.media.as_ref().is_some_and(|m| m.is_series()) {
            p.episodes.items().get(p.ep_sel).cloned()
        } else {
            p.media.clone()
        };
        if let Some(m) = m {
            self.play_item(m, cx);
        }
    }

    /// Blurhash placeholder (decoded once, kept small).
    fn blur(&mut self, hash: &str) -> Option<Arc<Image>> {
        if let Some(i) = self.blurs.get(&hash.to_string()) {
            return Some(i.clone());
        }
        let px = crate::blurhash::decode(hash, 32, 32)?;
        let i = ui::png(&px, 32, 32);
        self.blurs.insert(hash.to_string(), i.clone());
        Some(i)
    }

    /// Cached image lookup; starts a fetch the first time a slot is actually drawn (lazy).
    /// While it loads the item's blurhash (when the server has one) stands in.
    fn pic(&mut self, m: &Media, art: Art, width: u32, cx: &mut Context<Self>) -> Pic {
        let extra = if art == Art::Chapter {
            m.number.unwrap_or(0)
        } else {
            0
        };
        let key = image_key(m, art, width, extra);
        let loading = |me: &mut Self| match m.blur_for(art).and_then(|h| me.blur(h)) {
            Some(i) => Pic::Blur(i),
            None => Pic::Loading,
        };
        match self.images.get(&key) {
            Some(Img::Ready(i)) => return Pic::Ready(i.clone()),
            Some(Img::Loading) => return loading(self),
            Some(Img::Failed(at)) if at.elapsed() < RETRY_FAILED => return Pic::Missing,
            Some(Img::Failed(_)) | None => {}
        }
        let Some(src) = self.source.clone() else {
            return Pic::Loading;
        };
        self.images.insert(key.clone(), Img::Loading);
        let mc = m.clone();
        self.run(
            cx,
            async move { src.image(&mc, art, width).await },
            move |t, r, _| {
                let state = match r
                    .ok()
                    .and_then(|b| sniff(&b).map(|f| Image::from_bytes(f, b)))
                {
                    Some(i) => Img::Ready(Arc::new(i)),
                    None => Img::Failed(std::time::Instant::now()),
                };
                t.images.insert(key, state);
            },
        );
        loading(self)
    }

    // ---- keyboard ----

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        if self.sp.open {
            return self.sp_key(ev, cx);
        }
        if let Some(p) = self.player.clone() {
            let m = &ev.keystroke.modifiers;
            if key == "g" && !m.control && !m.alt && !m.shift && !self.dev {
                self.sp_open_screen(cx);
                return cx.notify();
            }
            p.update(cx, |p, cx| p.on_key(ev, window, cx));
            return;
        }
        if matches!(self.phase, Phase::Picker) {
            return self.picker_key(key, cx);
        }
        if let Some(a) = self.accounts.clone()
            && (matches!(self.phase, Phase::Account) || a.read(cx).dialog_open())
        {
            a.update(cx, |a, cx| a.on_key(ev, cx));
            cx.notify();
            return;
        }
        if self.sp.open {
            return self.sp_key(ev, cx);
        }
        if self.help_open || self.devices.is_some() {
            if matches!(key, "escape" | "backspace")
                || ev.keystroke.key_char.as_deref() == Some("?")
            {
                self.close_overlays();
            }
            return cx.notify();
        }
        if self.photo.is_some() {
            return self.photo_key(key, cx);
        }
        if self.more.is_some() || self.info_open {
            return self.more_key(key, cx);
        }
        if self.settings_open {
            return self.settings_key(key, cx);
        }
        if self.menu {
            return self.menu_key(key, cx);
        }
        if opens_account_menu(
            key,
            matches!(self.phase, Phase::Ready),
            self.admin_open,
            self.search.is_some(),
        ) {
            self.menu = true;
            self.menu_sel = 0;
            self.refresh_accounts(cx);
            cx.notify();
            return;
        }
        // A typed value ("/media/films", "192.168.1.0/24") must not trigger the global shortcuts.
        if self.admin_open
            && let Some(a) = self.admin.clone()
            && a.read(cx).dialog_open()
        {
            a.update(cx, |a, cx| a.on_key(ev, cx));
            cx.notify();
            return;
        }
        if let Phase::Error(e) = self.phase {
            if matches!(key, "enter" | "r") && e != ConnectError::NotConfigured {
                self.connect(cx);
            }
            return;
        }
        if matches!(self.phase, Phase::Ready) {
            if self.search.is_some() {
                return self.search_key(ev, cx);
            }
            let m = &ev.keystroke.modifiers;
            if ev.keystroke.key_char.as_deref() == Some("?") {
                self.close_admin_menus();
                self.help_open = true;
                return cx.notify();
            }
            if key == "g" && !m.control && !m.alt && !m.shift && !self.dev {
                self.close_admin_menus();
                self.sp_open_screen(cx);
                return cx.notify();
            }
            if (m.control && key == "k") || (key == "/" && !m.control && !m.alt && !m.shift) {
                self.close_admin(cx);
                return self.open_search(cx);
            }
            if m.alt && matches!(key, "left" | "right") {
                if key == "left" {
                    self.back();
                } else {
                    self.forward();
                }
                return cx.notify();
            }
            if self.stack.last().is_some_and(|p| {
                p.br.as_deref()
                    .is_some_and(|b| b.menu != ui_browse::Menu::None)
            }) {
                self.browse_key(key, m.shift, cx);
                return cx.notify();
            }
        }
        if self.admin_open
            && let Some(a) = self.admin.clone()
        {
            if a.update(cx, |a, cx| a.on_key(ev, cx)) {
                self.close_admin(cx);
            }
            cx.notify();
            return;
        }
        if matches!(key, "escape" | "backspace") {
            self.back();
            cx.notify();
            return;
        }
        if matches!(key, "w" | "f")
            && matches!(self.phase, Phase::Ready)
            && let Some(m) = self.focus_media()
        {
            if key == "w" {
                self.toggle_played(&m, cx);
            } else {
                self.toggle_fav(&m, cx);
            }
            cx.notify();
            return;
        }
        match self.stack.last().map(|p| p.view) {
            Some(View::Grid) if self.browse_key(key, ev.keystroke.modifiers.shift, cx) => {
                return cx.notify();
            }
            Some(View::Detail) => return self.detail_key(key, cx),
            _ => {}
        }
        let dir = match key {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        let enter = key == "enter";
        let Some(p) = self.stack.last_mut() else {
            return;
        };
        match p.view {
            View::Home => {
                let lay = p.layout();
                let kind = lay.get(p.row).copied().unwrap_or(HRow::Hero);
                let mut act = None;
                let nlibs = p.libs.items().len();
                let admin_col = kind == HRow::Nav && p.admin && p.col == nlibs + GLOBALS.len();
                if enter && admin_col {
                    self.open_admin(cx);
                    cx.notify();
                    return;
                }
                if enter && kind == HRow::Nav && (nlibs..nlibs + GLOBALS.len()).contains(&p.col) {
                    let g = GLOBALS[p.col - nlibs].0;
                    self.open_global(g, cx);
                    cx.notify();
                    return;
                }
                if let Some(d) = dir {
                    let n = p.row_len(kind);
                    let c = nav::step(p.col, n.max(1), n, d);
                    if matches!(d, Dir::Left | Dir::Right) {
                        p.col = c;
                    } else {
                        p.row = match d {
                            Dir::Up => p.row.saturating_sub(1),
                            _ => (p.row + 1).min(lay.len() - 1),
                        };
                    }
                    let kind = lay[p.row];
                    p.col = p.col.min(p.row_len(kind).saturating_sub(1));
                    // page children: hero(0), then the rows after Nav/Hero in order
                    p.page_scroll.scroll_to_item(p.row.saturating_sub(1));
                    match kind {
                        HRow::Resume => p.resume_scroll.scroll_to_item(p.col),
                        HRow::NextUp => p.nextup_scroll.scroll_to_item(p.col),
                        HRow::Latest(i) => p.rows[i].scroll.scroll_to_item(p.col),
                        _ => {}
                    }
                } else if enter {
                    act = match kind {
                        HRow::Hero if p.col == 0 => Some(Act::PlayHero),
                        HRow::Hero => p.hero_item().map(Act::Open),
                        HRow::Nav => p.row_items(kind).get(p.col).cloned().map(Act::Open),
                        HRow::Resume | HRow::NextUp => {
                            p.row_items(kind).get(p.col).cloned().map(Act::Play)
                        }
                        HRow::Latest(_) => p.row_items(kind).get(p.col).cloned().map(Act::Open),
                    };
                }
                match act {
                    Some(Act::PlayHero) => self.play_current(cx),
                    Some(Act::Play(m)) => self.play_item(m, cx),
                    Some(Act::Open(m)) if m.kind == Kind::Library => self.open_library(m, cx),
                    Some(Act::Open(m)) => self.open(m, cx),
                    None => {}
                }
            }
            View::Grid | View::Detail => {}
        }
        cx.notify();
    }

    fn hover_home(&mut self, row: usize, col: usize, cx: &mut Context<Self>) {
        if let Some(p) = self.stack.last_mut()
            && p.view == View::Home
            && (p.row, p.col) != (row, col)
        {
            (p.row, p.col) = (row, col);
            cx.notify();
        }
    }

    // ---- chrome ----

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let (libs, top): (Vec<Media>, Option<(usize, bool)>) = match self.stack.first() {
            Some(home) => {
                let cur = self
                    .stack
                    .get(1)
                    .and_then(|p| p.media.as_ref())
                    .map(|m| m.id.clone());
                let libs = home.libs.items().to_vec();
                let active = libs.iter().position(|l| Some(&l.id) == cur.as_ref());
                let focus = self
                    .stack
                    .last()
                    .filter(|p| p.view == View::Home && p.row == 0)
                    .map(|p| p.col);
                (
                    libs,
                    active
                        .map(|a| (a, focus == Some(a)))
                        .or(focus.map(|f| (f, true))),
                )
            }
            None => (vec![], None),
        };
        let at_home = self.stack.len() <= 1;
        let focus_col = self
            .stack
            .last()
            .filter(|p| p.view == View::Home && p.row == 0)
            .map(|p| p.col);
        let mut nav = div().flex().items_center().gap(px(t::S1));
        nav = nav.child(ui::nav_link(
            "nav-home".into(),
            "Home",
            at_home && focus_col.is_none() && !self.admin_open,
            false,
            cx.listener(|t, _, _, cx| {
                t.close_admin(cx);
                t.stack.truncate(1);
                if let Some(home) = t.stack.first_mut() {
                    home.drop_nav_focus();
                }
                cx.notify();
            }),
        ));
        for (i, l) in libs.iter().enumerate() {
            let m = l.clone();
            nav = nav.child(ui::nav_link(
                format!("nav-{i}"),
                &l.title,
                top.map(|(a, _)| a == i).unwrap_or(false) && !at_home && !self.admin_open,
                focus_col == Some(i) && !self.admin_open,
                cx.listener(move |t, _, _, cx| {
                    t.close_admin(cx);
                    t.open_library(m.clone(), cx);
                    cx.notify();
                }),
            ));
        }
        let cur_key = self
            .stack
            .get(1)
            .and_then(|p| p.media.as_ref())
            .map(|m| m.id.as_str());
        for (gi, (g, label)) in GLOBALS.into_iter().enumerate() {
            let key = match g {
                Global::Collections => "collections",
                Global::Playlists => "playlists",
                Global::Favourites => "favourites",
            };
            nav = nav.child(ui::nav_link(
                format!("nav-g{gi}"),
                label,
                cur_key == Some(key) && !at_home && !self.admin_open,
                focus_col == Some(libs.len() + gi) && !self.admin_open,
                cx.listener(move |t, _, _, cx| {
                    t.open_global(g, cx);
                    cx.notify();
                }),
            ));
        }
        if self.admin.is_some() {
            nav = nav.child(ui::nav_link(
                "nav-admin".into(),
                "Admin",
                self.admin_open,
                focus_col == Some(libs.len() + GLOBALS.len()) && !self.admin_open,
                cx.listener(|t, _, _, cx| {
                    t.open_admin(cx);
                    cx.notify();
                }),
            ));
        }
        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(t::HEADER_H))
            .px(px(t::PAGE_PAD))
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(t::hairline())
            .bg(rgba(0x07080ce6))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(t::S8))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(t::S1))
                            .child(self.hist_button(
                                "hist-back",
                                "\u{2039}",
                                "Back (Alt+Left)",
                                self.stack.len() > 1,
                                true,
                                cx,
                            ))
                            .child(self.hist_button(
                                "hist-fwd",
                                "\u{203A}",
                                "Forward (Alt+Right)",
                                !self.fwd.is_empty(),
                                false,
                                cx,
                            ))
                            .child(div().w(px(t::S3)))
                            .child(ui::wordmark(t::CAPTION + 1.0)),
                    )
                    .child(nav),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(t::S4))
                    .children(self.sp_pill(cx))
                    .child(
                        div()
                            .id("search-btn")
                            .h(px(32.0))
                            .px(px(t::S3))
                            .flex()
                            .items_center()
                            .gap(px(t::S3))
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(t::edge())
                            .cursor_pointer()
                            .hover(|s| s.border_color(t::accent()))
                            .tooltip(ui::tip("Search (Ctrl+K or /)"))
                            .on_click(cx.listener(|t, _, _, cx| {
                                t.open_search(cx);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(t::CAPTION + 1.0))
                                    .text_color(t::text_secondary())
                                    .child("Search"),
                            )
                            .child(ui::mono("/")),
                    )
                    .child(self.avatar(cx)),
            )
    }

    fn hist_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        tip: &'static str,
        enabled: bool,
        back: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .size(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .text_size(px(t::BODY_LG + 4.0))
            .text_color(if enabled {
                t::text()
            } else {
                t::text_tertiary()
            })
            .opacity(if enabled { 1.0 } else { 0.5 })
            .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(t::glass())))
            .tooltip(ui::tip(tip))
            .on_click(cx.listener(move |t, _, _, cx| {
                if back {
                    t.back()
                } else {
                    t.forward()
                }
                cx.notify();
            }))
            .child(glyph)
    }

    /// Breadcrumbs for deep pages: Home > ... > current; every crumb but the last goes back to it.
    fn crumbs(&self, cx: &mut Context<Self>) -> Option<Div> {
        let n = self.stack.len();
        if n < 3 {
            return None;
        }
        let title = |p: &Page| {
            if p.view == View::Home {
                "Home".to_string()
            } else {
                p.media.as_ref().map_or(String::new(), |m| m.title.clone())
            }
        };
        let (start, elided) = nav::crumb_window(n, 4);
        let mut row = div()
            .absolute()
            .top(px(t::HEADER_H))
            .left_0()
            .w_full()
            .h(px(28.0))
            .px(px(t::PAGE_PAD))
            .flex()
            .items_center()
            .gap(px(t::S2))
            // Same fill as the header: scrolled content must not show through behind the trail.
            .bg(rgba(0x07080ce6));
        let shown: Vec<usize> = if elided {
            std::iter::once(0).chain(start..n).collect()
        } else {
            (0..n).collect()
        };
        for (k, i) in shown.into_iter().enumerate() {
            if k > 0 {
                row = row.child(ui::mono(if elided && k == 1 {
                    "\u{2026}  \u{203A}"
                } else {
                    "\u{203A}"
                }));
            }
            let last = i == n - 1;
            row = row.child(
                div()
                    .id(("crumb", i))
                    .max_w(px(240.0))
                    .truncate()
                    .text_size(px(t::CAPTION))
                    .text_color(if last { t::text() } else { t::text_secondary() })
                    .when(!last, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.text_color(t::accent_text()))
                            .on_click(cx.listener(move |t, _, _, cx| {
                                while t.stack.len() > i + 1 {
                                    t.back();
                                }
                                cx.notify();
                            }))
                    })
                    .child(title(&self.stack[i])),
            );
        }
        Some(row)
    }

    /// Initial in a hairline circle; opens the account menu.
    fn avatar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let name = self
            .session
            .as_ref()
            .map_or("dev", |(_, s)| s.user_name.as_str());
        let pic = self.my_pic();
        ui::avatar(32.0, name, pic)
            .id("avatar")
            .border_1()
            .border_color(if self.menu { t::accent() } else { t::edge() })
            .bg(t::raised())
            .font_family(t::MONO)
            .text_size(px(t::CAPTION + 1.0))
            .text_color(t::accent_text())
            .cursor_pointer()
            .hover(|s| s.border_color(t::accent()))
            .on_click(cx.listener(|t, _, _, cx| {
                t.menu = !t.menu;
                t.menu_sel = 0;
                t.refresh_accounts(cx);
                cx.notify();
            }))
    }

    fn my_pic(&self) -> Option<Arc<Image>> {
        let uid = &self.session.as_ref()?.1.user_id;
        self.user_pic(uid, self.my_tag.as_deref())
    }

    /// Dropdown under the avatar: who and where, then the account actions.
    fn account_menu(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let (name, server) = self
            .session
            .as_ref()
            .map_or(("Developer token".into(), "read-only".into()), |(u, s)| {
                (s.user_name.clone(), u.clone())
            });
        let mut p = div()
            .id("acct-menu")
            .absolute()
            .top(px(t::HEADER_H + 4.0))
            .right(px(t::PAGE_PAD))
            .w(px(260.0))
            .p(px(t::S2))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .rounded(px(t::R_PANEL))
            .border_1()
            .border_color(t::hairline_strong())
            .bg(t::surface())
            .occlude()
            .child(
                div()
                    .px(px(t::S3))
                    .py(px(t::S2))
                    .flex()
                    .items_center()
                    .gap(px(t::S3))
                    .child(
                        ui::avatar(40.0, &name, self.my_pic())
                            .border_1()
                            .border_color(t::hairline_strong())
                            .text_size(px(t::BODY))
                            .font_family(t::MONO)
                            .text_color(t::accent_text()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(t::BODY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .child(div().truncate().child(ui::mono(server))),
                    ),
            )
            .child(div().h(px(1.0)).bg(t::hairline()));
        let writes = self.hooks.can_write();
        for (i, (label, act)) in self.menu_items().into_iter().enumerate() {
            let on = i == self.menu_sel;
            let face = match act {
                AcctAct::SwitchTo(n) => self.other_accounts.get(n).map(|a| {
                    let pic = self.user_pic(&a.user_id, a.image_tag.as_deref());
                    ui::avatar(22.0, &a.user_name, pic)
                        .border_1()
                        .border_color(t::hairline_strong())
                        .text_size(px(t::CAPTION))
                        .text_color(t::text_secondary())
                }),
                _ => None,
            };
            p = p.child(
                div()
                    .id(("acct-item", i))
                    .flex()
                    .items_center()
                    .gap(px(t::S2))
                    .px(px(t::S3))
                    .py(px(t::S2))
                    .rounded(px(6.0))
                    .text_size(px(t::BODY))
                    .cursor_pointer()
                    .when(on, |d| d.bg(t::accent_tint()).text_color(t::accent_text()))
                    .when(
                        matches!(
                            act,
                            AcctAct::Password
                                | AcctAct::Authorize
                                | AcctAct::ProfileImage
                                | AcctAct::RemoveImage
                                | AcctAct::Language
                        ) && !writes,
                        |d| d.opacity(0.55).tooltip(ui::tip(NEED_WRITES)),
                    )
                    .hover(|s| s.bg(t::glass()))
                    .on_click(cx.listener(move |t, _, _, cx| t.menu_act(act, cx)))
                    .children(face)
                    .child(label),
            );
        }
        // Click anywhere else closes it.
        div()
            .id("acct-away")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, _, _, cx| {
                    t.menu = false;
                    cx.notify();
                }),
            )
            .child(p)
    }

    /// Settings screen: one row per preference; a toggle flips on click, a multi-choice row opens a drop-down. Changes apply and save at once.
    fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut page = div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.set_scroll)
            .pt(px(t::HEADER_H + t::S8))
            .pb(px(t::S8))
            .flex()
            .flex_col()
            .child(div().flex_none().child(ui::section(
                "Settings",
                "Changes apply immediately and are saved on this device.",
                "Esc to go back".into(),
            )));
        for (i, r) in self.settings.rows().into_iter().enumerate() {
            let on = i == self.settings_sel;
            if let Some(h) = settings::head(i) {
                page = page.child(
                    div()
                        .flex_none()
                        .mx(px(t::PAGE_PAD))
                        .px(px(t::S6))
                        .pt(px(if i == 0 { t::S6 } else { t::S8 }))
                        .pb(px(t::S3))
                        .flex()
                        .flex_col()
                        .gap(px(t::S2))
                        .border_b_1()
                        .border_color(t::hairline())
                        .child(
                            div()
                                .text_size(px(t::CONTROL_TEXT))
                                .font_weight(FontWeight::BOLD)
                                .text_color(t::text_secondary())
                                .child(ui::tracked(h)),
                        )
                        .when(i == 0, |d| {
                            d.child(
                                div()
                                    .max_w(px(720.0))
                                    .text_size(px(t::BODY))
                                    .line_height(px(t::BODY * 1.5))
                                    .text_color(t::text_secondary())
                                    .child(settings::WRITES_NOTE),
                            )
                        }),
                );
            }
            page = page.child(
                div()
                    .id(("set-row", i))
                    .relative()
                    .flex_none()
                    .mx(px(t::PAGE_PAD))
                    .px(px(t::S6))
                    .py(px(t::S3))
                    .min_h(px(64.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(t::S6))
                    .border_b_1()
                    .border_color(t::hairline())
                    .bg(if on { t::accent_tint() } else { rgba(0) })
                    .cursor_pointer()
                    .hover(|s| s.bg(t::glass()))
                    .on_click(cx.listener(move |t, _, _, cx| {
                        t.settings_sel = i;
                        if t.dd.is_some() {
                            t.dd = None; // clicking out of an open menu only closes it
                            return cx.notify();
                        }
                        t.open_setting(i, cx);
                    }))
                    .when(on, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.0))
                                .bg(t::accent()),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(t::BODY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(r.label),
                            )
                            .child(
                                div()
                                    .max_w(px(640.0))
                                    .text_size(px(t::CONTROL_TEXT))
                                    .line_height(px(t::CONTROL_TEXT * 1.45))
                                    .text_color(t::text_secondary())
                                    .child(r.hint),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(220.0))
                            .flex()
                            .justify_end()
                            .child(self.setting_chip(i, &r.value, on, cx)),
                    ),
            );
        }
        page.into_any_element()
    }

    /// The value chip of Settings row `i`; for a multi-choice row it carries a caret and, while open,
    /// its drop-down menu. The menu and its click-away layer are deferred so they paint over the rows below.
    fn setting_chip(&self, i: usize, value: &str, on: bool, cx: &mut Context<Self>) -> Div {
        let Some((labels, cur)) = self.settings.choices(i) else {
            return div().child(ui::switch(("set-ctl", i), value == "On", on));
        };
        let open = self.dd.as_ref().is_some_and(|d| d.row == i);
        let mut wrap =
            div()
                .relative()
                .child(ui::value_chip(("set-ctl", i), value, true, on, open));
        if open {
            let sel = self.dd.as_ref().map_or(0, |d| d.sel);
            let n = labels.len();
            if let Some(k) = self.dd_reveal.get() {
                if self.dd_scroll.bounds_for_item(k).is_some() {
                    self.dd_scroll.scroll_to_item(k);
                    self.dd_reveal.set(None);
                }
                cx.notify(); // one more frame: the list has no bounds before its first paint
            }
            let mut list = ui_browse::panel("set-menu")
                .right(px(0.0))
                .w(px(220.0))
                .max_h(px(300.0))
                .overflow_y_scroll()
                .track_scroll(&self.dd_scroll);
            // Open below the chip unless that runs off the page and there is more room above.
            let (page, row) = (
                self.set_scroll.bounds(),
                self.set_scroll.bounds_for_item(settings::scroll_child(i)),
            );
            let need = px((n as f32 * 42.0 + 16.0).min(300.0) + 30.0);
            let up = row
                .is_some_and(|r| r.bottom() + need > page.bottom() && r.top() - page.top() > need);
            list = if up {
                list.bottom(px(t::CONTROL_H + 4.0))
            } else {
                list.top(px(t::CONTROL_H + 4.0))
            };
            for (k, name) in labels.into_iter().enumerate() {
                list = list.child(
                    ui_browse::menu_row(("set-opt", k), k == sel)
                        .on_click(cx.listener(move |t, _, _, cx| {
                            t.dd = None;
                            t.choose_setting(i, k, cx);
                        }))
                        .child(
                            div()
                                .text_color(if k == cur {
                                    t::accent_text()
                                } else {
                                    t::text()
                                })
                                .child(name),
                        )
                        .child(ui::mono(if k == cur { "\u{2713}" } else { "" })),
                );
            }
            let away = div()
                .id("set-away")
                .absolute()
                .top(px(-3000.0))
                .left(px(-3000.0))
                .w(px(9000.0))
                .h(px(9000.0))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|t, _, _, cx| {
                        t.dd = None;
                        cx.notify();
                    }),
                );
            wrap = wrap
                .child(deferred(away).with_priority(1))
                .child(deferred(list).with_priority(2));
        }
        wrap
    }

    fn centered(&self) -> Div {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(t::S4))
    }

    fn splash(&self, title: &str, body: &str, retry: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut d = self
            .centered()
            .child(ui::wordmark(t::DISPLAY))
            .child(ui::label("Your media. Your night."))
            .child(div().h(px(t::S6)))
            .child(
                div()
                    .text_size(px(t::HEADING))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t::text())
                    .child(title.to_string()),
            )
            .child(
                div()
                    .max_w(px(460.0))
                    .text_center()
                    .text_size(px(t::BODY))
                    .text_color(t::text_secondary())
                    .child(body.to_string()),
            );
        if retry {
            d = d.child(div().h(px(t::S2))).child(ui::pill_button(
                "retry",
                "Try again",
                true,
                false,
                cx.listener(|t, _, _, cx| {
                    t.connect(cx);
                    cx.notify();
                }),
            ));
        }
        d.into_any_element()
    }

    fn connecting(&self) -> AnyElement {
        self.centered()
            .child(ui::wordmark(t::DISPLAY))
            .child(
                div().child(ui::label("Connecting")).with_animation(
                    "pulse",
                    Animation::new(std::time::Duration::from_millis(1600))
                        .repeat()
                        .with_easing(ease_in_out),
                    |d, v| d.opacity(0.35 + 0.65 * (1.0 - (2.0 * v - 1.0).abs())),
                ),
            )
            .into_any_element()
    }

    fn note_text(msg: &str) -> Div {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(t::BODY))
            .text_color(t::text_secondary())
            .child(msg.to_string())
    }

    /// Root element shared by every screen: focus, keys, pointer, font. No background.
    fn shell(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("root")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|t, ev, w, cx| t.on_key(ev, w, cx)))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|t, _, _, cx| {
                    t.back();
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|t, _, _, cx| {
                    t.forward();
                    cx.notify();
                }),
            )
            .size_full()
            .relative()
            .font_family(t::FONT)
            .text_color(t::text())
    }

    // ---- now playing ----

    // ---- hero (shared by Home and Detail) ----

    fn colw(&self) -> f32 {
        f32::from(self.win.width)
    }

    /// Full-bleed hero: backdrop fills the width, scrims fade it into the page, text sits
    /// bottom-left. `frac` is the share of window height it may take.
    fn hero_band(
        &mut self,
        m: &Media,
        eyebrow: String,
        buttons: Div,
        frac: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let w = f32::from(self.win.width);
        let h = (w * 9.0 / 16.0)
            .min(f32::from(self.win.height) * frac)
            // Tall enough for the eyebrow, logo, overview and buttons below the nav and breadcrumbs.
            .max(500.0)
            .round();
        let hero_art = if m.backdrop_tag.is_none() && m.kind == Kind::Episode {
            Art::Landscape
        } else {
            Art::Backdrop
        };
        let backdrop = match self.pic(m, hero_art, 1920, cx) {
            Pic::Ready(i) => ui::fade_in(
                div()
                    .size_full()
                    .child(img(i).size_full().object_fit(ObjectFit::Cover)),
                format!("hero-{}", m.id),
                t::SLOW,
            )
            .into_any_element(),
            Pic::Blur(i) => div()
                .size_full()
                .child(img(i).size_full().object_fit(ObjectFit::Cover))
                .into_any_element(),
            Pic::Loading => ui::tile(Pic::Loading, &m.title, "hero-sk")
                .size_full()
                .into_any_element(),
            Pic::Missing => ui::tile(Pic::Missing, &m.title, "hero-miss")
                .size_full()
                .into_any_element(),
        };
        let mut chips = div()
            .flex()
            .items_center()
            .gap(px(t::S2))
            .child(ui::chip(&m.kind_label(), true));
        if let Some(y) = m.year {
            chips = chips.child(ui::chip(&y.to_string(), false));
        }
        if let Some(r) = m.runtime_min {
            chips = chips.child(ui::chip(&crate::source::fmt_runtime(r), false));
        }
        for g in m.genres.iter().take(3) {
            chips = chips.child(ui::chip(g, false));
        }
        let size = (w / 26.0).clamp(40.0, 72.0);
        // The title logo, when the item (or its show) has one; otherwise the title as text.
        let logo = if m.logo.is_some() {
            Some(self.pic(m, Art::Logo, 800, cx))
        } else {
            None
        };
        let title_el: AnyElement = match logo {
            Some(Pic::Ready(i)) => ui::fade_in(
                div()
                    .h(px(110.0))
                    .max_w(px(460.0))
                    // The limit sits on the image itself so a very wide logo is scaled down to fit
                    // (contain) instead of overflowing and being clipped by the hero.
                    .child(
                        img(i)
                            .h(px(110.0))
                            .max_w(px(460.0))
                            .object_fit(ObjectFit::Contain),
                    ),
                format!("logo-{}", m.id),
                t::SLOW,
            )
            .into_any_element(),
            Some(Pic::Blur(_) | Pic::Loading) => div().h(px(110.0)).into_any_element(),
            _ => div()
                .line_clamp(2)
                .text_size(px(size))
                .line_height(px(size * 1.08))
                .font_weight(FontWeight::BOLD)
                .text_color(t::text())
                .child(m.title.clone())
                .into_any_element(),
        };
        let season_line = (m.logo.is_some() && matches!(m.kind, Kind::Season | Kind::Episode))
            .then(|| {
                div()
                    .text_size(px(t::HEADING))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t::text())
                    .child(m.title.clone())
            });
        let sub = m.overview.clone().unwrap_or_else(|| m.meta_line());
        let text = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(t::S4))
            .max_w(px(680.0))
            .child(ui::mono(eyebrow))
            .child(chips)
            .child(title_el)
            .children(season_line)
            .child(
                div()
                    .max_w(px(560.0))
                    .line_clamp(3)
                    .line_height(px(t::BODY_LG * 1.6))
                    .text_size(px(t::BODY_LG))
                    .text_color(t::text_secondary())
                    .child(sub),
            )
            .child(buttons);
        let (clear, solid) = (rgba(0x07080c00), rgba(0x07080cff));
        let layer = || div().absolute().top_0().left_0().size_full();
        div()
            .relative()
            .w_full()
            .h(px(h))
            .overflow_hidden()
            .bg(t::surface())
            .child(layer().child(backdrop))
            .child(layer().bg(linear_gradient(
                90.0,
                linear_color_stop(rgba(0x07080cf0), 0.0),
                linear_color_stop(clear, 0.65),
            )))
            .child(layer().bg(linear_gradient(
                180.0,
                linear_color_stop(clear, 0.45),
                linear_color_stop(solid, 1.0),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(t::PAGE_PAD))
                    .bottom(px(t::S8))
                    .child(ui::fade_in(text, format!("herotext-{}", m.id), t::SLOW)),
            )
    }

    // ---- Home ----

    /// One poster strip. `row` is its layout row (for hover focus); `play` resumes on click.
    #[allow(clippy::too_many_arguments)]
    fn strip(
        &mut self,
        key: &str,
        items: &Load<Vec<Media>>,
        handle: &ScrollHandle,
        row: usize,
        focus: Option<usize>,
        play: bool,
        land: bool,
        (cw, ch): (f32, f32),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let list = items.items().to_vec();
        let mut strip = div()
            .id(ui::name(format!("strip-{key}")))
            .flex()
            .overflow_x_scroll()
            .track_scroll(handle);
        if matches!(items, Load::Ready(_)) && list.is_empty() {
            strip = strip.child(
                div()
                    .h(px(ch.min(120.0)))
                    .flex()
                    .items_center()
                    .px(px(t::S3))
                    .child(ui::mono("nothing here yet".to_string())),
            );
        } else if list.is_empty() {
            let loading = matches!(items, Load::Loading);
            for i in 0..8 {
                strip = strip.child(ui::poster_cell(Cell {
                    key: &format!("sk-{key}-{i}"),
                    pic: if loading { Pic::Loading } else { Pic::Missing },
                    title: "",
                    meta: String::new(),
                    index: i,
                    w: cw,
                    h: ch,
                    focused: false,
                    progress: None,
                    badge: None,
                }));
            }
        }
        for (ci, m) in list.iter().enumerate() {
            let art = if land { Art::Landscape } else { Art::Poster };
            let pic = self.pic(m, art, (cw * 2.0) as u32, cx);
            let target = m.clone();
            // Landscape captions: series name + mono `S2:E9 · episode title` for episodes.
            let (title, meta) = match (land, m.kind) {
                (true, Kind::Episode) => (
                    m.series_name.clone().unwrap_or_else(|| m.title.clone()),
                    [m.short_ep(), m.title.clone()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                ),
                _ => (
                    m.title.clone(),
                    [m.kind_label(), m.meta_line()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                ),
            };
            strip = strip.child(
                ui::poster_cell(Cell {
                    key: &format!("{key}-{}", m.id),
                    pic,
                    title: &title,
                    meta,
                    index: ci,
                    w: cw,
                    h: ch,
                    focused: focus == Some(ci),
                    progress: m.progress(),
                    badge: ui_browse::badge(m),
                })
                .group("card")
                .child(self.chips(m, cx))
                .on_hover(cx.listener(move |t, hov: &bool, _, cx| {
                    if *hov {
                        t.hover_home(row, ci, cx);
                    }
                }))
                .on_click(cx.listener(move |t, _, _, cx| {
                    if play {
                        t.play_item(target.clone(), cx);
                    } else {
                        t.open(target.clone(), cx);
                    }
                    cx.notify();
                })),
            );
        }
        strip
    }

    fn strip_band(title: &str, text: &str, meta: String, strip: Stateful<Div>) -> Div {
        let fade = linear_gradient(
            90.0,
            linear_color_stop(rgba(0x07080c00), 0.0),
            linear_color_stop(rgba(0x07080cf0), 1.0),
        );
        ui::band(
            div()
                .pt(px(t::S8 * 1.5))
                .pb(px(t::S8 * 1.5))
                .flex()
                .flex_col()
                .gap(px(t::S6))
                .child(ui::section(title, text, meta))
                .child(
                    div()
                        .relative()
                        .mx(px(t::PAGE_PAD))
                        .border_t_1()
                        .border_l_1()
                        .border_color(t::hairline())
                        .rounded(px(t::R_PANEL))
                        .overflow_hidden()
                        .child(strip)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .h_full()
                                .w(px(48.0))
                                .bg(fade),
                        ),
                ),
        )
    }

    /// Card size so a whole number of cards fills the row exactly (`ratio` = height/width).
    fn row_dims(&self, target: f32, ratio: f32) -> (f32, f32) {
        let avail = self.colw() - 2.0 * t::PAGE_PAD - 1.0;
        let cols = nav::columns(avail, target + 2.0 * t::S3, 0.0);
        let w = (avail / cols as f32).floor() - 2.0 * t::S3;
        (w, (w * ratio).round())
    }

    fn home(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.stack.last() else {
            return div().into_any_element();
        };
        if matches!(p.libs, Load::Failed) {
            return self
                .failed_view("Your library couldn't be loaded.", cx)
                .into_any_element();
        }
        let (row, col, scroll, show) = (p.row, p.col, p.page_scroll.clone(), p.show);
        let lay = p.layout();
        let focus_of = |k: HRow| (lay.get(row) == Some(&k)).then_some(col);
        let row_of = |k: HRow| lay.iter().position(|x| *x == k).unwrap_or(0);
        let hero = p.hero_item();
        let resume = (p.resume.clone(), p.resume_scroll.clone());
        let rows: Vec<(Media, Load<Vec<Media>>, ScrollHandle)> = p
            .rows
            .iter()
            .map(|r| (r.lib.clone(), r.items.clone(), r.scroll.clone()))
            .collect();
        let k = self.settings.density.card() / 176.0;
        let size = self.row_dims(190.0 * k, 1.5);
        let wide = self.row_dims(300.0 * k, 9.0 / 16.0);
        let nextup = (p.nextup.clone(), p.nextup_scroll.clone());

        let hero_el = match hero {
            Some(m) => {
                let eyebrow = format!("0x00 · {}", m.kind_label());
                let (m1, m2) = (m.clone(), m.clone());
                let hero_focus = focus_of(HRow::Hero);
                let verb = if m.kind == Kind::Photo {
                    "View"
                } else {
                    "Play"
                };
                let buttons = div()
                    .flex()
                    .gap(px(t::S3))
                    .children(
                        (!matches!(
                            m.kind,
                            Kind::Book | Kind::Folder | Kind::Person | Kind::Other
                        ))
                        .then(|| {
                            ui::pill_button(
                                "hero-play",
                                verb,
                                true,
                                hero_focus == Some(0),
                                cx.listener(move |t, _, _, cx| {
                                    t.play_item(m1.clone(), cx);
                                    cx.notify();
                                }),
                            )
                        }),
                    )
                    .child(ui::pill_button(
                        "hero-more",
                        "More info",
                        false,
                        hero_focus == Some(1),
                        cx.listener(move |t, _, _, cx| {
                            t.open(m2.clone(), cx);
                            cx.notify();
                        }),
                    ));
                self.hero_band(&m, eyebrow, buttons, 0.62, cx)
            }
            None => div()
                .h(px(420.0))
                .pt(px(t::HEADER_H))
                .child(Self::note_text(if p_loading(&rows) {
                    "Loading your library"
                } else {
                    "Nothing to show yet."
                })),
        };

        let mut page = div()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .flex()
            .flex_col()
            .child(ui::band(hero_el));

        for (key, title, text, (items, handle), hrow, kind) in [
            (
                "resume",
                "Continue watching",
                "Pick up where you left off.",
                resume,
                HRow::Resume,
                "in progress",
            ),
            (
                "nextup",
                "Next up",
                "The next episode of what you're following.",
                nextup,
                HRow::NextUp,
                "up next",
            ),
        ] {
            let shown = if hrow == HRow::Resume { show.0 } else { show.1 };
            if !shown || items.items().is_empty() {
                continue;
            }
            let n = items.items().len();
            let s = self.strip(
                key,
                &items,
                &handle,
                row_of(hrow),
                focus_of(hrow),
                true,
                true,
                wide,
                cx,
            );
            page = page.child(Self::strip_band(title, text, format!("{n} {kind}"), s));
        }

        for (ri, (lib, items, handle)) in rows.into_iter().enumerate() {
            if !show.2 {
                break;
            }
            let meta = match &items {
                Load::Ready(v) => format!("{} · {} titles", lib.kind_label(), v.len()),
                _ => lib.kind_label(),
            };
            let s = self.strip(
                &ri.to_string(),
                &items,
                &handle,
                row_of(HRow::Latest(ri)),
                focus_of(HRow::Latest(ri)),
                false,
                false,
                size,
                cx,
            );
            page = page.child(Self::strip_band(
                &format!("Latest in {}", lib.title),
                "Recently added, newest first.",
                meta,
                s,
            ));
        }
        page.child(div().h(px(t::S8 * 2.0)).flex_none())
            .into_any_element()
    }
}

fn p_loading(rows: &[(Media, Load<Vec<Media>>, ScrollHandle)]) -> bool {
    rows.is_empty() || rows.iter().any(|r| matches!(r.1, Load::Loading))
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.win = window.viewport_size();
        if let Some(p) = &mut self.picker {
            p.cols = picker::columns(f32::from(self.win.width));
        }
        if std::mem::take(&mut self.fs_exit) && window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        if let Some(p) = &self.player {
            let mut page = self.shell(cx).bg(rgba(0x00000000)).child(p.clone());
            if let Some(pill) = self.sp_pill(cx) {
                page = page.child(
                    div()
                        .absolute()
                        .top(px(t::S4))
                        .left_0()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(pill),
                );
            }
            if let Some(n) = self.note.clone() {
                page = page.child(
                    div()
                        .absolute()
                        .bottom(px(t::S6))
                        .left(px(t::S6))
                        .px(px(t::S4))
                        .py(px(t::S3))
                        .rounded(px(t::R_PANEL))
                        .bg(t::raised())
                        .border_1()
                        .border_color(t::hairline_strong())
                        .text_size(px(t::BODY))
                        .child(n),
                );
            }
            if let Some(o) = self.sp_overlay(cx) {
                page = page.child(o);
            }
            return page.into_any_element();
        }
        let body = match self.phase {
            Phase::Account => match self.accounts.clone() {
                Some(a) => a.into_any_element(),
                None => self.connecting(),
            },
            Phase::Picker => match &self.picker {
                Some(p) => picker::view(
                    p,
                    &|a| self.user_pic(&a.user_id, a.image_tag.as_deref()),
                    |t: &mut Self, act, cx| t.picker_act(act, cx),
                    cx,
                ),
                None => self.connecting(),
            },
            Phase::Connecting => self.connecting(),
            Phase::Error(ConnectError::NotConfigured) => self.splash(
                "Connect a media source",
                "Add your server address and access token to the .env file, then restart NOCTURNE.",
                false,
                cx,
            ),
            Phase::Error(ConnectError::Unreachable) => self.splash(
                "Can't reach your server",
                "Check that it is running and that the address is right. Your library appears as soon as it answers.",
                true,
                cx,
            ),
            Phase::Error(ConnectError::Rejected) => self.splash(
                "Your server declined the access token",
                "Create a new token on the server, update your .env file, then try again.",
                true,
                cx,
            ),
            Phase::Ready if self.settings_open => self.settings_page(cx),
            Phase::Ready if self.admin_open && self.admin.is_some() => ui::fade_in(
                div().size_full().children(self.admin.clone()),
                "admin-page".into(),
                t::BASE,
            )
            .into_any_element(),
            Phase::Ready => {
                let (pid, view) = self.stack.last().map_or((0, View::Home), |p| (p.pid, p.view));
                let page = match view {
                    View::Home => self.home(cx),
                    View::Grid => self.grid(cx),
                    View::Detail => self.detail(cx),
                };
                ui::fade_in(
                    div().size_full().child(page),
                    format!("page-{pid}-{}", self.stack.len()),
                    t::BASE,
                )
                .into_any_element()
            }
        };
        let layer = |i: Arc<Image>, opacity: f32| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .opacity(opacity)
                .child(img(i).size_full().object_fit(ObjectFit::Cover))
        };
        let mut root = self
            .shell(cx)
            .bg(t::frame())
            .child(layer(self.glow.clone(), 1.0))
            .child(layer(self.dots.clone(), 0.09))
            .child(div().size_full().child(body));
        if matches!(self.phase, Phase::Ready) {
            root = root.child(self.header(cx));
            if self.offline.is_some() {
                root = root.child(self.offline_banner(cx));
            }
            if !self.admin_open
                && !self.settings_open
                && let Some(c) = self.crumbs(cx)
            {
                root = root.child(c);
            }
        }
        root = root.child(layer(self.grain.clone(), 0.02));
        if self.menu && matches!(self.phase, Phase::Ready) {
            root = root.child(self.account_menu(cx));
        }
        if self.search.is_some() && matches!(self.phase, Phase::Ready) {
            root = root.child(self.search_overlay(cx));
        }
        if matches!(self.phase, Phase::Ready)
            && let Some(o) = self.more_overlay(cx)
        {
            root = root.child(o);
        }
        if let Some(o) = self.photo_overlay(cx) {
            root = root.child(o);
        }
        if let Some(o) = self.help_overlay(cx) {
            root = root.child(o);
        }
        if let Some(o) = self.devices_overlay(cx) {
            root = root.child(o);
        }
        if let Some(o) = self.sp_overlay(cx) {
            root = root.child(o);
        }
        if let Some(a) = self.accounts.clone().filter(|a| a.read(cx).dialog_open()) {
            root = root.child(a);
        }
        if let Some(n) = self.note.clone() {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(t::S6))
                    .right(px(t::S6))
                    .px(px(t::S4))
                    .py(px(t::S3))
                    .rounded(px(t::R_PANEL))
                    .bg(t::raised())
                    .border_1()
                    .border_color(t::hairline_strong())
                    .text_size(px(t::BODY))
                    .text_color(t::text())
                    .child(n),
            );
        }
        root.into_any_element()
    }
}
/// The `a` shortcut opens the account menu, but never while a text field has the keys (search, Admin dialogs).
fn opens_account_menu(key: &str, ready: bool, admin_open: bool, search_open: bool) -> bool {
    key == "a" && ready && !admin_open && !search_open
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test; // gpui::* also exports a `test` macro

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
        let png = crate::ui::png(&[0, 0, 0, 255], 1, 1);
        c.insert(user_pic_key("u", "t1"), Img::Ready(png));
        assert!(ready_pic(&c, "u", Some("t1")).is_some());
        assert!(
            ready_pic(&c, "u", Some("t2")).is_none(),
            "changed tag: old picture not reused"
        );
        assert!(ready_pic(&c, "u", None).is_none(), "removed: initial");
        assert!(ready_pic(&c, "other", Some("t1")).is_none());
    }
}
