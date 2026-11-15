//! Root view: connection state, navigation stack, keyboard, and the screens.
mod ui_browse;
mod ui_detail;
mod ui_search;

use crate::account::{self, Accounts};
use crate::admin::{self, Admin};
use crate::detail::Full;
use crate::hooks::{PlayerHooks, SyncHooks, ticks};
use crate::imgcache::DiskCache;
use crate::nav::{self, Dir};
use crate::playback::{Now, Reporter, Tick};
use crate::search::Search;
use crate::settings::{ROWS, Settings};
use crate::source::fmt_clock;
use crate::source::{Art, ConnectError, Kind, Media, Source, sniff};
use crate::ui::{self, Cell, Pic};
use crate::video::{self, Aspect, AutoHide, MenuAct, Playback, Pop};
use crate::{bridge, cache::Cache, theme as t};
use gpui::prelude::*;
use gpui::*;
use nocturne_jellyfin_admin::AdminClient;
use nocturne_jellyfin_user::{Session, UserClient};
use nocturne_player::{EmbeddedPlayer, Event, Options, Player};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::cell::Cell as StdCell;
use std::collections::VecDeque;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ui_browse::{Browse, GLOBALS, Global};
use ui_detail::DRow;

/// Height of the docked now-playing bar.
const BAR_H: f32 = 96.0;

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
    Failed,
}

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
    dscroll: [ScrollHandle; 6],
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

/// Pointer drags on the now-playing bar (fraction 0..1 along the track).
#[derive(Clone, Copy)]
enum Drag {
    Seek(f32),
    Vol(f32),
}

enum Act {
    PlayHero,
    Play(Media),
    Open(Media),
}

enum Phase {
    /// Login mode: the account screens (connect, sign in, restore) own the window.
    Account,
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
    admin_open: bool,
    playback: Option<Playback>,
    /// Player page active (embedded video): the window is a hole for the video, controls overlay it.
    video: bool,
    /// Embedded player waiting for the next render (needs `&Window` for the raw handles): (url, header).
    pending: Option<(String, String)>,
    /// Backend decision, made at the first play. `false` = child-process mpv.
    embed: Option<bool>,
    vbounds: Option<Bounds<Pixels>>,
    hide: AutoHide,
    ctl_shown: bool,
    volume: f64,
    /// We entered fullscreen from the player page; leave it when playback ends.
    fs_from_video: bool,
    tracks: Vec<video::Track>,
    chapters: Vec<(f64, String)>,
    pop: Option<Pop>,
    pop_sel: usize,
    speed: f64,
    aspect: Aspect,
    repeat: bool,
    /// Right-hand time shows total instead of remaining.
    show_total: bool,
    osd: Option<(String, Instant)>,
    stats: bool,
    stats_rows: Vec<(&'static str, String)>,
    help: bool,
    /// Buffered-ahead position (seconds) for the scrub track.
    cache_end: Option<f64>,
    now: Option<Now>,
    /// Bar kept alive briefly after stop so it can slide out.
    leaving: Option<Now>,
    bar_gen: u64,
    track_b: Rc<StdCell<Bounds<Pixels>>>,
    vol_b: Rc<StdCell<Bounds<Pixels>>>,
    drag: Option<Drag>,
    hover_frac: Option<f32>,
    bar_hover: bool,
    muted_from: Option<f64>,
    grain: Arc<Image>,
    dots: Arc<Image>,
    glow: Arc<Image>,
    cols: usize,
    win: Size<Pixels>,
    note: Option<&'static str>,
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
    menu: bool,
    menu_sel: usize,
    /// Pages left by Back, restored by Forward (cleared by any new navigation).
    fwd: Vec<Page>,
    blurs: Cache<String, Arc<Image>>,
    search: Option<Search>,
    search_sel: usize,
    search_scroll: ScrollHandle,
    disk: Option<Arc<DiskCache>>,
    /// Items waiting behind the one playing (Play all / Shuffle).
    queue: VecDeque<Media>,
}

/// Account menu entries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AcctAct {
    Settings,
    Password,
    Switch,
    SignOut,
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
            admin_open: false,
            playback: None,
            video: false,
            pending: None,
            embed: None,
            vbounds: None,
            hide: AutoHide::new(Instant::now()),
            ctl_shown: true,
            volume: 100.0,
            fs_from_video: false,
            tracks: vec![],
            chapters: vec![],
            pop: None,
            pop_sel: 0,
            speed: 1.0,
            aspect: Aspect::Auto,
            repeat: false,
            show_total: false,
            osd: None,
            stats: false,
            stats_rows: vec![],
            help: false,
            cache_end: None,
            now: None,
            leaving: None,
            bar_gen: 0,
            track_b: Rc::default(),
            vol_b: Rc::default(),
            drag: None,
            hover_frac: None,
            bar_hover: false,
            muted_from: None,
            grain: ui::grain(),
            dots: ui::dots(),
            glow: ui::glow(),
            cols: 4,
            win: size(px(1280.0), px(800.0)),
            note: None,
            note_gen: 0,
            dev: false,
            accounts: None,
            session: None,
            hooks: Arc::new(SyncHooks::new(None, true)),
            reporter: Reporter::default(),
            settings: Settings::load(),
            settings_open: false,
            settings_sel: 0,
            menu: false,
            menu_sel: 0,
            fwd: vec![],
            blurs: Cache::new(400),
            search: None,
            search_sel: 0,
            search_scroll: ScrollHandle::new(),
            disk: None,
            queue: VecDeque::new(),
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
            let acct = cx.new(|cx| Accounts::new(true, cx));
            cx.subscribe(&acct, |t, _, ev: &account::Event, cx| match ev {
                account::Event::SignedIn { url, session } => {
                    t.activate(url.clone(), session.clone(), cx)
                }
                account::Event::Notice(n) => t.toast(n, cx),
            })
            .detach();
            me.accounts = Some(acct);
        }
        // Teardown order: the video subsurface must go before the window, and not from inside the
        // compositor's close event (joining the render thread there deadlocks). So while a video
        // is up, veto the close, drop the video from a task, then remove the window ourselves.
        let this = cx.weak_entity();
        _w.on_window_should_close(cx, move |w, cx| {
            let handle = Window::window_handle(w);
            let this = this.clone();
            if this
                .read_with(cx, |t, _| t.playback.is_none())
                .unwrap_or(true)
            {
                return true;
            }
            cx.spawn(async move |cx| {
                this.update(cx, |t, _| {
                    t.report_stop();
                    t.playback = None;
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
        // Poll the player a few times a second; cheap when nothing is playing.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
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
                    t.admin = c.map(|c| cx.new(|_| Admin::new(c)));
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
            .map(|c| cx.new(|_| Admin::new(c)));
        self.session = Some((url, s));
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

    /// Back to the account screens: stop playback, drop everything tied to the login.
    fn leave_session(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        self.leaving = None;
        self.source = None;
        self.stack.clear();
        self.admin = None;
        self.admin_open = false;
        self.settings_open = false;
        self.menu = false;
        self.session = None;
        self.hooks = Arc::new(SyncHooks::new(None, true));
        self.phase = Phase::Account;
    }

    fn sign_out(&mut self, cx: &mut Context<Self>) {
        let client = self
            .session
            .as_ref()
            .and_then(|(u, s)| UserClient::from_session(u, s).ok());
        self.leave_session(cx);
        if let Some(a) = self.accounts.clone() {
            a.update(cx, |a, cx| a.sign_out(client, cx));
        }
    }

    fn switch_server(&mut self, cx: &mut Context<Self>) {
        self.leave_session(cx);
        if let Some(a) = self.accounts.clone() {
            a.update(cx, |a, cx| a.switch_server(cx));
        }
    }

    fn menu_items(&self) -> Vec<(&'static str, AcctAct)> {
        let mut v = vec![("Settings", AcctAct::Settings)];
        if !self.dev {
            v.extend([
                ("Change password", AcctAct::Password),
                ("Switch server", AcctAct::Switch),
                ("Sign out", AcctAct::SignOut),
            ]);
        }
        v
    }

    fn menu_act(&mut self, act: AcctAct, cx: &mut Context<Self>) {
        self.menu = false;
        match act {
            AcctAct::Settings => {
                self.close_admin(cx);
                self.settings_open = true;
                self.settings_sel = 0;
            }
            AcctAct::Password => {
                let client = self
                    .session
                    .as_ref()
                    .and_then(|(u, s)| UserClient::from_session(u, s).ok());
                if let (Some(c), Some(a)) = (client, self.accounts.clone()) {
                    a.update(cx, |a, cx| a.open_password(c, cx));
                }
            }
            AcctAct::Switch => self.switch_server(cx),
            AcctAct::SignOut => self.sign_out(cx),
        }
        cx.notify();
    }

    /// Settings row `i` stepped; applies at once and is saved.
    fn cycle_setting(&mut self, i: usize, dir: i32, cx: &mut Context<Self>) {
        self.settings.cycle(i, dir);
        self.settings.save();
        self.hooks.sync.store(
            self.settings.sync_progress,
            std::sync::atomic::Ordering::Relaxed,
        );
        let show = self.settings.home();
        for p in &mut self.stack {
            p.show = show;
            p.row = p.row.min(p.layout().len() - 1);
        }
        cx.notify();
    }

    fn settings_key(&mut self, key: &str, cx: &mut Context<Self>) {
        match key {
            "escape" | "backspace" => self.settings_open = false,
            "up" => self.settings_sel = self.settings_sel.saturating_sub(1),
            "down" => self.settings_sel = (self.settings_sel + 1).min(ROWS - 1),
            "left" => self.cycle_setting(self.settings_sel, -1, cx),
            "right" | "enter" | "space" => self.cycle_setting(self.settings_sel, 1, cx),
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
        if let Some(n) = self.now.as_mut() {
            hit(&mut n.media);
        }
    }

    fn toggle_played(&mut self, m: &Media, cx: &mut Context<Self>) {
        if !self.hooks.sync_on() {
            let why = if self.hooks.can_write() {
                "Watch sync is off. Turn it on in Settings."
            } else {
                "Sign in to keep watched state on the server."
            };
            return self.toast(why, cx);
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
            return self.toast("Sign in to save favourites.", cx);
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

    /// Progress reporting: started once, every 10 s, and on pause/resume/seek. Gated by Settings.
    fn sync_tick(&mut self) {
        let Some(n) = self.now.as_ref().filter(|n| n.loaded) else {
            return;
        };
        let Some(pos) = n.state.as_ref().and_then(|s| s.position) else {
            return;
        };
        let paused = n.state.as_ref().is_some_and(|s| s.paused);
        let id = n.media.id.clone();
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

    /// Heart and check chips on a poster: always shown when set, on hover otherwise.
    fn chips(&self, m: &Media, cx: &mut Context<Self>) -> Div {
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
                .border_color(if on {
                    t::accent()
                } else {
                    t::hairline_strong()
                })
                .text_size(px(13.0))
                .text_color(if on {
                    t::accent_text()
                } else {
                    t::text_secondary()
                })
                .cursor_pointer()
                .occlude()
                .hover(|s| s.border_color(t::accent()))
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

    fn push(&mut self, view: View, media: Option<Media>, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let pid = self.push_page(view, media.clone());
        match (view, media) {
            (View::Home, _) => {
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

    fn toast(&mut self, msg: &'static str, cx: &mut Context<Self>) {
        self.note = Some(msg);
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

    /// Play for a focused thing. A series plays its first unwatched (else first) episode.
    fn play_item(&mut self, m: Media, cx: &mut Context<Self>) {
        match m.kind {
            Kind::Library | Kind::Season => {}
            Kind::Series => {
                let Some(src) = self.source.clone() else {
                    return;
                };
                self.run(
                    cx,
                    async move { src.series_target(&m.id).await },
                    |t, r, cx| match r {
                        Ok(e) => t.play_media(e, cx),
                        Err(_) => t.toast("Nothing to play here yet.", cx),
                    },
                );
            }
            _ => self.play_media(m, cx),
        }
    }

    /// Hands one playable item to the player. Nothing here is ever logged.
    fn play_media(&mut self, m: Media, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let (url, header) = src.stream(&m.id);
        if let Some(p) = &self.playback {
            if p.load(&url, Some(&header)).is_err() {
                self.stop(cx);
                return self.toast("This couldn't be played.", cx);
            }
        } else if self.embed_wanted() {
            // Created on the next render, which has the window handles.
            self.pending = Some((url, header));
            self.video = true;
            self.hide.poke(Instant::now());
        } else if !self.start_external(&url, &header) {
            return self.toast("This couldn't be played.", cx);
        }
        let ep = m.clone();
        self.report_stop(); // a previous item ends here (no-op when nothing was playing)
        self.hooks.seed_favorite(&m.id, m.favorite);
        self.leaving = None;
        if self.now.is_none() {
            self.bar_gen += 1;
        }
        self.muted_from = None;
        self.now = Some(Now::new(m));
        if let Some(q) = self.queue.pop_front() {
            if let Some(n) = self.now.as_mut() {
                n.next = Some(q);
            }
        } else if ep.kind == Kind::Episode {
            self.run(
                cx,
                async move { (ep.id.clone(), src.next_episode(&ep).await) },
                |t, (id, next), _| {
                    if let Some(n) = t.now.as_mut().filter(|n| n.media.id == id) {
                        n.next = next;
                    }
                },
            );
        }
    }

    fn embed_wanted(&mut self) -> bool {
        *self.embed.get_or_insert_with(|| {
            let env = std::env::var("NOCTURNE_VIDEO").ok();
            let wl = std::env::var_os("WAYLAND_DISPLAY").is_some();
            video::choose(env.as_deref(), wl) == video::Choice::Embedded
        })
    }

    /// Child-process mpv in its own window. False (and everything reset) if it can't play.
    fn start_external(&mut self, url: &str, header: &str) -> bool {
        self.video = false;
        let ok = Player::new(Options::default())
            .ok()
            .filter(|p| p.load(url, Some(header)).is_ok());
        self.playback = ok.map(Playback::External);
        self.fresh_session();
        self.playback.is_some()
    }

    /// Render-time: create the embedded player now that we have the window.
    fn start_embedded(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some((url, header)) = self.pending.take() else {
            return;
        };
        let handles = match (
            HasDisplayHandle::display_handle(window).map(|h| h.as_raw()),
            HasWindowHandle::window_handle(window).map(|h| h.as_raw()),
        ) {
            (Ok(RawDisplayHandle::Wayland(d)), Ok(RawWindowHandle::Wayland(s))) => {
                Some((d.display.as_ptr(), s.surface.as_ptr()))
            }
            _ => None,
        };
        let p = handles
            .and_then(|(d, s)| EmbeddedPlayer::new(d, s).ok())
            .filter(|p| p.load(&url, Some(&header)).is_ok());
        match p {
            Some(p) => {
                let _ = p.set_volume(self.volume);
                self.vbounds = None; // push bounds to the new surface on the next paint
                self.playback = Some(Playback::Embedded(Box::new(p)));
                self.fresh_session();
            }
            None => {
                // Calm, counts only: no URLs, no ids.
                eprintln!("video: embedded unavailable, using the external player");
                self.embed = Some(false);
                if !self.start_external(&url, &header) {
                    self.stop(cx);
                    self.toast("This couldn't be played.", cx);
                }
            }
        }
    }

    /// A new mpv instance starts with default speed/aspect/repeat.
    fn fresh_session(&mut self) {
        self.speed = 1.0;
        self.aspect = Aspect::Auto;
        self.repeat = false;
        self.pop = None;
        self.stats = false;
        self.help = false;
    }

    fn osd(&mut self, text: impl Into<String>) {
        self.osd = Some((text.into(), Instant::now()));
    }

    /// Tracks and chapters of the loaded file (also refreshed when a menu opens: selection changes).
    fn refresh_meta(&mut self) {
        let Some(p) = &self.playback else { return };
        self.tracks = p
            .prop("track-list")
            .map(|v| video::parse_tracks(&v))
            .unwrap_or_default();
        self.chapters = p
            .prop("chapter-list")
            .map(|v| video::parse_chapters(&v))
            .unwrap_or_default();
    }

    fn num(&self, name: &str) -> Option<f64> {
        self.playback.as_ref()?.prop(name)?.as_f64()
    }

    fn set_speed(&mut self, sp: f64) {
        if let Some(p) = &self.playback {
            let _ = p.set("speed", &sp.to_string());
        }
        self.speed = sp;
        self.osd(format!("Speed {}", video::fmt_speed(sp)));
    }

    fn set_aspect(&mut self, a: Aspect) {
        if let Some(p) = &self.playback {
            for (k, v) in a.props() {
                let _ = p.set(k, v);
            }
        }
        self.aspect = a;
        self.osd(format!("Aspect {}", a.label()));
    }

    fn set_repeat(&mut self, on: bool) {
        if let Some(p) = &self.playback {
            let _ = p.set("loop-file", if on { "inf" } else { "no" });
        }
        self.repeat = on;
        self.osd(if on { "Repeat one" } else { "Repeat off" });
    }

    fn set_track(&mut self, prop: &str, id: i64, label: &str) {
        if let Some(p) = &self.playback {
            let _ = p.set(prop, &if id == 0 { "no".into() } else { id.to_string() });
        }
        self.refresh_meta();
        self.osd(label);
    }

    fn add_delay(&mut self, prop: &str, name: &str, d: f64) {
        let Some(p) = &self.playback else { return };
        let _ = p.command(&["add", prop, &d.to_string()]);
        let v = self.num(prop).unwrap_or(0.0);
        self.osd(format!("{name} delay {}", video::fmt_delay(v)));
    }

    fn pop_act(&mut self, act: MenuAct, cx: &mut Context<Self>) {
        match act {
            MenuAct::Sub(id) => {
                self.set_track(
                    "sid",
                    id,
                    if id == 0 {
                        "Subtitles off"
                    } else {
                        "Subtitles on"
                    },
                );
                self.pop = None;
            }
            MenuAct::Audio(id) => {
                self.set_track("aid", id, "Audio track changed");
                self.pop = None;
            }
            MenuAct::Speed(s) => {
                self.set_speed(s);
                self.pop = Some(Pop::Settings);
                self.pop_sel = 1;
            }
            MenuAct::Aspect(a) => {
                self.set_aspect(a);
                self.pop = Some(Pop::Settings);
                self.pop_sel = 0;
            }
            MenuAct::Repeat(r) => {
                self.set_repeat(r);
                self.pop = Some(Pop::Settings);
                self.pop_sel = 2;
            }
            MenuAct::Open(p) => {
                self.pop = Some(p);
                self.pop_sel = 0;
            }
            MenuAct::Info => {
                self.pop = None;
                self.stats = true;
            }
        }
        cx.notify();
    }

    fn toggle_pop(&mut self, p: Pop, cx: &mut Context<Self>) {
        if self.pop == Some(p) {
            self.pop = None;
        } else {
            self.refresh_meta();
            let items = video::items(p, &self.tracks, self.speed, self.aspect, self.repeat);
            self.pop_sel = items.iter().position(|i| i.on).unwrap_or(0);
            self.pop = Some(p);
        }
        self.hide.poke(Instant::now());
        cx.notify();
    }

    fn screenshot(&mut self) {
        let Some(p) = &self.playback else { return };
        let dir = std::env::var_os("XDG_PICTURES_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Pictures")))
            .filter(|d| d.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let file = dir.join(format!("nocturne-{secs}.png"));
        let ok = p
            .command(&["screenshot-to-file", &file.to_string_lossy(), "video"])
            .is_ok();
        self.osd(if ok {
            "Screenshot saved"
        } else {
            "Screenshot failed"
        });
    }

    /// Popover keys. True when consumed.
    fn pop_key(&mut self, k: &str, cx: &mut Context<Self>) -> bool {
        let Some(pop) = self.pop else { return false };
        let items = video::items(pop, &self.tracks, self.speed, self.aspect, self.repeat);
        match k {
            "up" => self.pop_sel = self.pop_sel.saturating_sub(1),
            "down" => self.pop_sel = (self.pop_sel + 1).min(items.len().saturating_sub(1)),
            "enter" => {
                if let Some(i) = items.get(self.pop_sel) {
                    self.pop_act(i.act, cx);
                }
            }
            // Sub-menus go back to Settings; top-level menus close.
            "escape" => {
                self.pop =
                    matches!(pop, Pop::Speed | Pop::Aspect | Pop::Repeat).then_some(Pop::Settings);
                self.pop_sel = 0;
            }
            _ => {}
        }
        cx.notify();
        true
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.report_stop();
        self.queue.clear();
        self.playback = None; // dropping closes mpv / the video surface
        self.video = false;
        self.pending = None;
        self.pop = None;
        self.tracks.clear();
        self.chapters.clear();
        self.cache_end = None;
        self.drag = None;
        self.hover_frac = None;
        let Some(n) = self.now.take() else { return };
        self.refresh_resume(cx);
        self.leaving = Some(n);
        self.bar_gen += 1;
        let g = self.bar_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(220))
                .await;
            this.update(cx, |t, cx| {
                if t.bar_gen == g {
                    t.leaving = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn track_frac(&self, x: Pixels, b: Bounds<Pixels>) -> f32 {
        if b.size.width <= px(0.0) {
            0.0
        } else {
            ((x - b.origin.x) / b.size.width).clamp(0.0, 1.0)
        }
    }

    fn drag_move(&mut self, x: Pixels, cx: &mut Context<Self>) {
        match self.drag {
            Some(Drag::Seek(_)) => {
                self.drag = Some(Drag::Seek(self.track_frac(x, self.track_b.get())))
            }
            Some(Drag::Vol(_)) => {
                let f = self.track_frac(x, self.vol_b.get());
                self.drag = Some(Drag::Vol(f));
                self.muted_from = None;
                if let Some(p) = &self.playback {
                    let _ = p.set_volume(f as f64 * 100.0);
                }
            }
            None => return,
        }
        cx.notify();
    }

    fn drag_end(&mut self, cx: &mut Context<Self>) {
        if let (Some(Drag::Seek(f)), Some(p), Some(d)) = (
            self.drag.take(),
            &self.playback,
            self.now.as_ref().and_then(|n| n.times().1),
        ) {
            let _ = p.seek_to(f as f64 * d);
        }
        cx.notify();
    }

    fn mute(&mut self) {
        let v = self
            .now
            .as_ref()
            .and_then(|n| n.state.as_ref())
            .map_or(100.0, |s| s.volume);
        let Some(p) = &self.playback else { return };
        match self.muted_from.take() {
            Some(prev) => drop(p.set_volume(prev)),
            None if v > 0.0 => {
                self.muted_from = Some(v);
                let _ = p.set_volume(0.0);
            }
            None => drop(p.set_volume(100.0)),
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.drain_reverts(cx);
        let shown = self.video_controls();
        if self.video && shown != self.ctl_shown {
            self.ctl_shown = shown;
            cx.notify();
        }
        let Some(pl) = &self.playback else { return };
        let (mut failed, mut gone, mut meta) = (false, false, false);
        if let Some(now) = &mut self.now {
            for ev in pl.events().try_iter() {
                match ev {
                    Event::Loaded => {
                        meta = true;
                        now.loaded = true;
                        if let Some(r) = now.resume_at.take() {
                            let _ = pl.seek_to(r);
                        }
                    }
                    Event::Failed(_) => failed = true,
                    Event::Closed => gone = true,
                    _ => {}
                }
            }
            let st = pl.state();
            self.volume = st.volume;
            failed |= now.stalled(st.idle);
            now.ended = st.eof;
            if now.state.as_ref() != Some(&st) {
                now.state = Some(st);
                cx.notify();
            }
        } else {
            gone = true;
        }
        self.sync_tick();
        if meta {
            self.refresh_meta();
        }
        self.poll_extras(cx);
        if failed {
            self.stop(cx);
            self.toast("This couldn't be played.", cx);
            cx.notify();
        } else if gone {
            self.stop(cx);
            cx.notify();
        }
    }

    /// Buffered position, stats rows and OSD expiry; cheap, runs from the 200 ms tick.
    fn poll_extras(&mut self, cx: &mut Context<Self>) {
        let cache = self.num("demuxer-cache-time");
        if cache != self.cache_end {
            self.cache_end = cache;
            cx.notify();
        }
        if self
            .osd
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > Duration::from_millis(1600))
        {
            self.osd = None;
            cx.notify();
        }
        if self.stats && self.playback.is_some() {
            self.stats_rows = self.stats_snapshot();
            cx.notify();
        }
    }

    fn stats_snapshot(&self) -> Vec<(&'static str, String)> {
        let Some(p) = &self.playback else {
            return vec![];
        };
        let g = |n: &str| {
            p.prop(n)
                .map(|v| match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Number(n) => n.as_f64().map_or(n.to_string(), |f| {
                        if f.fract() == 0.0 {
                            format!("{f:.0}")
                        } else {
                            format!("{f:.2}")
                        }
                    }),
                    v => v.to_string(),
                })
                .unwrap_or_else(|| "-".into())
        };
        let kbps = |n: &str| {
            p.prop(n)
                .and_then(|v| v.as_f64())
                .map_or("-".into(), |b| format!("{:.0} kbps", b / 1000.0))
        };
        vec![
            ("Play method", "Direct Play".into()),
            ("Container", g("file-format")),
            (
                "Video",
                format!(
                    "{} {}x{} @ {} fps",
                    g("video-format"),
                    g("width"),
                    g("height"),
                    g("container-fps")
                ),
            ),
            ("Video bitrate", kbps("video-bitrate")),
            (
                "Audio",
                format!(
                    "{} {}",
                    g("audio-codec-name"),
                    g("audio-params/hr-channels")
                ),
            ),
            ("Audio bitrate", kbps("audio-bitrate")),
            ("Hardware decode", g("hwdec-current")),
            (
                "Dropped frames",
                format!(
                    "{} (decoder {})",
                    g("frame-drop-count"),
                    g("decoder-frame-drop-count")
                ),
            ),
            ("Buffered", format!("{} s", g("demuxer-cache-duration"))),
        ]
    }

    fn toggle(&mut self) {
        if let Some(p) = &self.playback {
            let _ = p.toggle();
        }
    }

    fn seek(&mut self, secs: f64) {
        if let Some(p) = &self.playback {
            let _ = p.seek_by(secs);
        }
    }

    fn next_episode(&mut self, cx: &mut Context<Self>) {
        if let Some(n) = self.now.as_ref().and_then(|n| n.next.clone()) {
            self.play_media(n, cx);
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
        let key = format!("{}:{:?}:{width}:{extra}", m.id, art);
        let loading = |me: &mut Self| match m.blur_for(art).and_then(|h| me.blur(h)) {
            Some(i) => Pic::Blur(i),
            None => Pic::Loading,
        };
        match self.images.get(&key) {
            Some(Img::Ready(i)) => return Pic::Ready(i.clone()),
            Some(Img::Loading) => return loading(self),
            Some(Img::Failed) => return Pic::Missing,
            None => {}
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
                    None => Img::Failed,
                };
                t.images.insert(key, state);
            },
        );
        loading(self)
    }

    // ---- keyboard ----

    fn video_controls(&self) -> bool {
        let hold = self.bar_hover || self.drag.is_some();
        self.hide.visible(Instant::now(), hold)
    }

    fn toggle_fullscreen(&mut self, window: &mut Window) {
        self.fs_from_video = !window.is_fullscreen();
        window.toggle_fullscreen();
    }

    /// Player-page keys. True when consumed.
    fn video_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use video::Key;
        self.hide.poke(Instant::now());
        let k = ev.keystroke.key.as_str();
        if self.pop_key(k, cx) {
            return true;
        }
        let m = &ev.keystroke.modifiers;
        let Some(key) = video::key(k, m.shift, m.control) else {
            return false;
        };
        let pos = self.now.as_ref().map_or(0.0, |n| n.times().0);
        match key {
            Key::Toggle => self.toggle(),
            Key::Seek(s) => self.seek(s),
            Key::Volume(d) => {
                self.muted_from = None;
                let v = video::bump_volume(self.volume, d);
                if let Some(p) = &self.playback {
                    let _ = p.set_volume(v);
                }
                self.osd(format!("Volume {}%", v.round()));
            }
            Key::Mute => self.mute(),
            Key::Stop => self.stop(cx),
            Key::Fullscreen => self.toggle_fullscreen(window),
            Key::Back if self.help || self.stats => (self.help, self.stats) = (false, false),
            Key::Back if window.is_fullscreen() => self.toggle_fullscreen(window),
            Key::Back => self.stop(cx),
            Key::Next => self.next_episode(cx),
            Key::Speed(d) => self.set_speed(video::step_speed(self.speed, d)),
            Key::Frame(d) => {
                if let Some(p) = &self.playback {
                    let _ = p.command(&[if d > 0 {
                        "frame-step"
                    } else {
                        "frame-back-step"
                    }]);
                }
            }
            Key::Chapter(d) => {
                if let (Some(t), Some(p)) = (
                    video::chapter_target(&self.chapters, pos, d),
                    &self.playback,
                ) {
                    let _ = p.seek_to(t);
                    let title = video::chapter_at(&self.chapters, t)
                        .map(|i| self.chapters[i].1.clone())
                        .filter(|s| !s.is_empty());
                    self.osd(title.unwrap_or_else(|| "Chapter".into()));
                }
            }
            Key::SubDelay(d) => self.add_delay("sub-delay", "Subtitle", d),
            Key::AudioDelay(d) => self.add_delay("audio-delay", "Audio", d),
            Key::CycleSubs => {
                self.refresh_meta();
                let cur = self
                    .tracks
                    .iter()
                    .find(|t| t.kind == video::TrackKind::Sub && t.selected)
                    .map_or(0, |t| t.id);
                let next = video::next_sub(&self.tracks, cur);
                self.set_track(
                    "sid",
                    next,
                    if next == 0 {
                        "Subtitles off"
                    } else {
                        "Subtitles on"
                    },
                );
            }
            Key::Stats => self.stats = !self.stats,
            Key::Help => self.help = !self.help,
            Key::Screenshot => self.screenshot(),
            Key::Loop => self.set_repeat(!self.repeat),
        }
        cx.notify();
        true
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        if self.video && self.video_key(ev, window, cx) {
            return;
        }
        if let Some(a) = self.accounts.clone()
            && (matches!(self.phase, Phase::Account) || a.read(cx).dialog_open())
        {
            a.update(cx, |a, cx| a.on_key(ev, cx));
            cx.notify();
            return;
        }
        if self.settings_open {
            return self.settings_key(key, cx);
        }
        if self.menu {
            return self.menu_key(key, cx);
        }
        if key == "a" && matches!(self.phase, Phase::Ready) && !self.admin_open {
            self.menu = true;
            self.menu_sel = 0;
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
            if (m.control && key == "k") || (key == "/" && !m.control && !m.alt) {
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
        if self.now.is_some() {
            let spatial = self
                .stack
                .last()
                .is_some_and(|p| matches!(p.view, View::Home | View::Grid | View::Detail));
            match key {
                "space" => self.toggle(),
                "s" => self.stop(cx),
                "n" => self.next_episode(cx),
                // Left/Right navigate grids; elsewhere they seek.
                "left" if !spatial => self.seek(-10.0),
                "right" if !spatial => self.seek(30.0),
                _ => {}
            }
            if matches!(key, "space" | "s" | "n") || (!spatial && matches!(key, "left" | "right")) {
                cx.notify();
                return;
            }
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
            .bg(rgba(0x07080cb3))
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
                            .border_color(t::hairline_strong())
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
            .gap(px(t::S2));
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
        div()
            .id("avatar")
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(if self.menu {
                t::accent()
            } else {
                t::hairline_strong()
            })
            .bg(t::raised())
            .font_family(t::MONO)
            .text_size(px(t::CAPTION + 1.0))
            .text_color(t::accent_text())
            .cursor_pointer()
            .hover(|s| s.border_color(t::accent()))
            .on_click(cx.listener(|t, _, _, cx| {
                t.menu = !t.menu;
                t.menu_sel = 0;
                cx.notify();
            }))
            .child(ui::initial(name))
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
            )
            .child(div().h(px(1.0)).bg(t::hairline()));
        for (i, (label, act)) in self.menu_items().into_iter().enumerate() {
            let on = i == self.menu_sel;
            p = p.child(
                div()
                    .id(("acct-item", i))
                    .px(px(t::S3))
                    .py(px(t::S2))
                    .rounded(px(6.0))
                    .text_size(px(t::BODY))
                    .cursor_pointer()
                    .when(on, |d| d.bg(t::accent_tint()).text_color(t::accent_text()))
                    .hover(|s| s.bg(t::glass()))
                    .on_click(cx.listener(move |t, _, _, cx| t.menu_act(act, cx)))
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

    /// Settings screen: one row per preference; click or Enter steps it, changes apply and save at once.
    fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().flex().flex_col();
        for (i, r) in self.settings.rows().into_iter().enumerate() {
            let on = i == self.settings_sel;
            list = list.child(
                div()
                    .id(("set-row", i))
                    .px(px(t::S6))
                    .py(px(t::S4))
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
                        t.cycle_setting(i, 1, cx);
                    }))
                    .child(
                        div()
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
                                    .text_size(px(t::CAPTION + 1.0))
                                    .text_color(t::text_secondary())
                                    .child(r.hint),
                            ),
                    )
                    .child(ui::chip(&r.value, on)),
            );
        }
        div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .pt(px(t::HEADER_H + t::S8))
            .pb(px(t::S8))
            .flex()
            .flex_col()
            .gap(px(t::S6))
            .child(ui::section(
                "Settings",
                "Changes apply immediately and are saved on this device.",
                "Esc to go back".into(),
            ))
            .child(
                div()
                    .mx(px(t::PAGE_PAD))
                    .border_t_1()
                    .border_l_1()
                    .border_color(t::hairline())
                    .rounded(px(t::R_PANEL))
                    .overflow_hidden()
                    .child(list),
            )
            .into_any_element()
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
            .on_mouse_move(cx.listener(|t, e: &MouseMoveEvent, _, cx| {
                if t.video {
                    t.hide.poke(Instant::now());
                    if !t.ctl_shown {
                        t.ctl_shown = true;
                        cx.notify();
                    }
                }
                t.drag_move(e.position.x, cx)
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|t, _, _, cx| {
                    if !t.video {
                        t.back();
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|t, _, _, cx| {
                    if !t.video {
                        t.forward();
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|t, _, _, cx| {
                    if t.drag.is_some() {
                        t.drag_end(cx);
                    }
                }),
            )
            .size_full()
            .relative()
            .font_family(t::FONT)
            .text_color(t::text())
    }

    /// Player page: video fills the window (transparent hole), controls overlay it and auto-hide.
    fn player_page(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let shown = self.video_controls();
        self.ctl_shown = shown;
        let view = cx.entity();
        let loaded = self.now.as_ref().is_some_and(|n| n.loaded);
        let bar = shown.then(|| self.now_bar(cx)).flatten();
        let mut page = self
            .shell(cx)
            // Black until the first frame is on its way, else the desktop shows through the hole.
            .when(!loaded, |d| d.bg(rgb(0x000000)))
            .when(!shown, |d| d.cursor(CursorStyle::None))
            .child(
                canvas(
                    |b, _, _| b,
                    move |b, _, window, cx| {
                        let sf = window.scale_factor();
                        view.update(cx, |t, cx| {
                            if t.vbounds == Some(b) {
                                return;
                            }
                            t.vbounds = Some(b);
                            if let Some(Playback::Embedded(p)) = t.playback.as_mut() {
                                let f = |v: Pixels| f32::from(v);
                                p.set_bounds(
                                    f(b.origin.x),
                                    f(b.origin.y),
                                    f(b.size.width),
                                    f(b.size.height),
                                    sf,
                                );
                                cx.notify(); // parent commit applies the new position
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(div().id("video-hit").absolute().size_full().on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, e: &MouseDownEvent, w, cx| {
                    // Each press toggles pause; the second press of a double-click undoes the
                    // first and goes fullscreen.
                    t.toggle();
                    if e.click_count == 2 {
                        t.toggle_fullscreen(w);
                    }
                    cx.notify();
                }),
            ));
        for o in self.overlays() {
            page = page.child(o);
        }
        if let Some(bar) = bar {
            page = page.child(bar);
        }
        if let Some(n) = self.note {
            page = page.child(
                div()
                    .absolute()
                    .top(px(t::S6))
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
        page
    }

    // ---- now playing ----

    fn now_bar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let leaving = self.now.is_none();
        let n = self.now.as_ref().or(self.leaving.as_ref())?;
        let media = n.media.clone();
        let (mut pos, dur) = n.times();
        let st = n.state.as_ref();
        let paused = st.is_some_and(|s| s.paused);
        let mut vol = st.map_or(100.0, |s| s.volume);
        let (loaded, ended, has_next, mut frac) = (n.loaded, n.ended, n.next.is_some(), n.frac());
        if let (Some(Drag::Seek(f)), Some(d)) = (self.drag, dur) {
            (frac, pos) = (f, f as f64 * d);
        }
        if let Some(Drag::Vol(f)) = self.drag {
            vol = f as f64 * 100.0;
        }
        let tip = self.hover_frac.zip(dur).map(|(f, d)| {
            let t = f as f64 * d;
            let mut text = fmt_clock(t);
            if let Some(i) = video::chapter_at(&self.chapters, t)
                && !self.chapters[i].1.is_empty()
            {
                text = format!("{text} · {}", self.chapters[i].1);
            }
            (f, text)
        });
        let status = if !loaded {
            "loading".to_string()
        } else if ended {
            "finished".to_string()
        } else {
            [
                media.series_name.clone().unwrap_or_default(),
                media.episode_label(),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
        };
        let right_time = match dur {
            Some(d) if !self.show_total => format!("-{}", fmt_clock((d - pos).max(0.0))),
            Some(d) => fmt_clock(d),
            None => "--:--".into(),
        };
        let ends = dur
            .filter(|_| loaded && !ended)
            .map(|d| video::ends_at(video::local_hm(), (d - pos).max(0.0), self.speed));
        let cache_frac = self
            .cache_end
            .zip(dur)
            .map_or(0.0, |(c, d)| (c / d).clamp(0.0, 1.0) as f32);
        let tick_fracs: Vec<f32> = dur
            .map(|d| {
                self.chapters
                    .iter()
                    .filter(|c| c.0 > 0.5)
                    .map(|c| (c.0 / d) as f32)
                    .collect()
            })
            .unwrap_or_default();

        let tb = self.track_b.clone();
        let big = self.bar_hover || matches!(self.drag, Some(Drag::Seek(_)));
        let th = if big { 8.0 } else { 4.0 };
        let track = div()
            .id("np-track")
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(16.0))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .top(px(6.0))
                    .left_0()
                    .w_full()
                    .child(ui::progress_bar(frac, th)),
            )
            .child(
                // Buffered-ahead segment, lighter than the played part.
                div()
                    .absolute()
                    .top(px(6.0))
                    .left(relative(frac))
                    .w(relative((cache_frac - frac).max(0.0)))
                    .h(px(th))
                    .bg(rgba(0xffffff33)),
            )
            .children(tick_fracs.into_iter().map(|f| {
                div()
                    .absolute()
                    .top(px(6.0))
                    .left(relative(f))
                    .w(px(2.0))
                    .h(px(th))
                    .bg(rgba(0x07080cd0))
            }))
            .child(
                canvas(move |b, _, _| tb.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .on_hover(cx.listener(|t, h: &bool, _, cx| {
                t.bar_hover = *h;
                if !*h {
                    t.hover_frac = None;
                }
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|t, e: &MouseMoveEvent, _, cx| {
                t.hover_frac = Some(t.track_frac(e.position.x, t.track_b.get()));
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, e: &MouseDownEvent, _, cx| {
                    t.drag = Some(Drag::Seek(t.track_frac(e.position.x, t.track_b.get())));
                    cx.notify();
                }),
            );

        let mut ctl = div()
            .flex()
            .items_center()
            .gap(px(t::S2))
            .child(ui::ctl_button(
                "np-back",
                "icons/back.svg",
                "10",
                false,
                cx.listener(|t, _, _, cx| {
                    t.seek(-10.0);
                    cx.notify();
                }),
            ))
            .child(ui::ctl_button(
                "np-pause",
                if paused {
                    "icons/play.svg"
                } else {
                    "icons/pause.svg"
                },
                "",
                true,
                cx.listener(|t, _, _, cx| {
                    t.toggle();
                    cx.notify();
                }),
            ))
            .child(ui::ctl_button(
                "np-fwd",
                "icons/fwd.svg",
                "30",
                false,
                cx.listener(|t, _, _, cx| {
                    t.seek(30.0);
                    cx.notify();
                }),
            ));
        if has_next {
            ctl = ctl.child(ui::ctl_button(
                "np-next",
                "icons/play.svg",
                "next",
                ended,
                cx.listener(|t, _, _, cx| {
                    t.next_episode(cx);
                    cx.notify();
                }),
            ));
        }
        let vb = self.vol_b.clone();
        let volume = div()
            .flex()
            .items_center()
            .gap(px(t::S2))
            .child(ui::ctl_button(
                "np-mute",
                if vol <= 0.0 {
                    "icons/mute.svg"
                } else {
                    "icons/volume.svg"
                },
                "",
                false,
                cx.listener(|t, _, _, cx| {
                    t.mute();
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .id("np-vol")
                    .relative()
                    .w(px(84.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .w_full()
                            .child(ui::progress_bar((vol / 130.0) as f32, 3.0)),
                    )
                    .child(
                        canvas(move |b, _, _| vb.set(b), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|t, e: &MouseDownEvent, _, cx| {
                            t.drag = Some(Drag::Vol(0.0));
                            t.drag_move(e.position.x, cx);
                        }),
                    ),
            )
            .when(vol > 100.0, |d| {
                d.child(ui::mono(format!("{}%", vol.round())))
            });
        let video = self.video;
        let menus = div()
            .flex()
            .items_center()
            .gap(px(t::S2))
            .child(ui::ctl_button(
                "np-cc",
                "icons/cc.svg",
                "",
                self.pop == Some(Pop::Subs),
                cx.listener(|t, _, _, cx| t.toggle_pop(Pop::Subs, cx)),
            ))
            .child(ui::ctl_button(
                "np-audio",
                "icons/audio.svg",
                "",
                self.pop == Some(Pop::Audio),
                cx.listener(|t, _, _, cx| t.toggle_pop(Pop::Audio, cx)),
            ))
            .child(volume)
            .child(ui::ctl_button(
                "np-gear",
                "icons/gear.svg",
                "",
                matches!(
                    self.pop,
                    Some(Pop::Settings | Pop::Speed | Pop::Aspect | Pop::Repeat)
                ),
                cx.listener(|t, _, _, cx| t.toggle_pop(Pop::Settings, cx)),
            ))
            .when(video, |d| {
                d.child(ui::ctl_button(
                    "np-fs",
                    "icons/fullscreen.svg",
                    "",
                    false,
                    cx.listener(|t, _, w, cx| {
                        t.toggle_fullscreen(w);
                        cx.notify();
                    }),
                ))
            })
            .child(ui::ctl_button(
                "np-stop",
                "icons/stop.svg",
                "",
                false,
                cx.listener(|t, _, _, cx| {
                    t.stop(cx);
                    cx.notify();
                }),
            ));
        let mut glow = t::glow();
        glow.a = 0.10;
        let row1 = div()
            .h(px(52.0))
            .flex_none()
            .px(px(t::S6))
            .flex()
            .items_center()
            .gap(px(t::S4))
            .child(ctl)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(t::S3))
                    .child(ends.map_or_else(div, |e| ui::mono(e).flex_none()))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t::text())
                            .child(media.title.clone()),
                    )
                    .child(ui::mono(status).truncate()),
            )
            .child(menus);
        let row2 = div()
            .h(px(36.0))
            .flex_none()
            .px(px(t::S6))
            .flex()
            .items_center()
            .gap(px(t::S3))
            .child(ui::mono(fmt_clock(pos)).flex_none().w(px(52.0)))
            .child(track)
            .child(
                div()
                    .id("np-remaining")
                    .flex_none()
                    .w(px(60.0))
                    .cursor_pointer()
                    .child(ui::mono(right_time).text_right())
                    .on_click(cx.listener(|t, _, _, cx| {
                        t.show_total = !t.show_total;
                        cx.notify();
                    })),
            );
        let bar = div()
            .absolute()
            .bottom_0()
            .left_0()
            .w_full()
            .h(px(BAR_H))
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(t::hairline_strong())
            .occlude()
            .bg(rgba(if self.video { 0x07080cc4 } else { 0x07080cf5 }))
            .shadow(vec![BoxShadow {
                color: glow.into(),
                offset: point(px(0.0), px(-10.0)),
                blur_radius: px(36.0),
                spread_radius: px(0.0),
            }])
            .child(row1)
            .child(row2)
            .when_some(tip, |d, (f, text)| {
                let b = self.track_b.get();
                d.child(
                    div()
                        .absolute()
                        .bottom(px(BAR_H - 52.0 - 4.0 + 4.0))
                        .left(b.origin.x + b.size.width * f - px(22.0))
                        .px(px(t::S2))
                        .py(px(3.0))
                        .rounded(px(4.0))
                        .bg(t::raised())
                        .border_1()
                        .border_color(t::hairline_strong())
                        .child(ui::mono(text).text_color(t::text())),
                )
            })
            .when_some(self.pop, |d, p| d.child(self.popover(p, cx)));
        Some(
            bar.with_animation(
                ui::name(format!("np-bar-{}-{leaving}", self.bar_gen)),
                Animation::new(std::time::Duration::from_millis(200)).with_easing(ease_out_quint()),
                move |d, v| {
                    let k = if leaving { v } else { 1.0 - v };
                    d.bottom(px(-BAR_H * k)).opacity(1.0 - k)
                },
            )
            .into_any_element(),
        )
    }

    /// Popover above the controls: translucent panel, hairline border, selected row = accent fill.
    fn popover(&self, pop: Pop, cx: &mut Context<Self>) -> Stateful<Div> {
        let rows = video::items(pop, &self.tracks, self.speed, self.aspect, self.repeat);
        div()
            .id("np-pop")
            .occlude()
            .absolute()
            .bottom(px(BAR_H + 6.0))
            .right(px(t::S6))
            .w(px(340.0))
            .max_h(px(380.0))
            .overflow_y_scroll()
            .p(px(t::S2))
            .rounded(px(t::R_PANEL))
            .bg(rgba(0x0b0d14ee))
            .border_1()
            .border_color(t::hairline_strong())
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .px(px(t::S3))
                    .py(px(t::S2))
                    .child(ui::mono(pop.title().to_uppercase())),
            )
            .children(rows.into_iter().enumerate().map(|(i, it)| {
                let act = it.act;
                let kb = i == self.pop_sel;
                div()
                    .id(("pop-row", i))
                    .px(px(t::S3))
                    .py(px(t::S2))
                    .rounded(px(6.0))
                    .flex()
                    .justify_between()
                    .gap(px(t::S3))
                    .text_size(px(13.0))
                    .cursor_pointer()
                    .when(it.on, |d| d.bg(t::accent()).text_color(t::on_accent()))
                    .when(!it.on && kb, |d| d.bg(t::glass()))
                    .when(!it.on, |d| d.hover(|s| s.bg(t::glass())))
                    .on_click(cx.listener(move |t, _, _, cx| t.pop_act(act, cx)))
                    .child(div().truncate().child(it.label))
                    .child(
                        div()
                            .flex_none()
                            .text_color(t::text_secondary())
                            .child(it.value),
                    )
            }))
    }

    /// Top-left OSD chip, stats panel and key help over the video.
    fn overlays(&self) -> Vec<AnyElement> {
        let panel = || {
            div()
                .absolute()
                .rounded(px(t::R_PANEL))
                .bg(rgba(0x0b0d14e0))
                .border_1()
                .border_color(t::hairline_strong())
                .px(px(t::S4))
                .py(px(t::S3))
                .text_size(px(13.0))
                .text_color(t::text())
        };
        let mut v = vec![];
        if let Some((text, _)) = &self.osd {
            v.push(
                panel()
                    .top(px(t::S6))
                    .left(px(t::S6))
                    .py(px(t::S2))
                    .child(text.clone())
                    .into_any_element(),
            );
        }
        if self.stats {
            v.push(
                panel()
                    .top(px(t::S6 + 48.0))
                    .left(px(t::S6))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .children(self.stats_rows.iter().map(|(k, val)| {
                        div()
                            .flex()
                            .gap(px(t::S4))
                            .child(div().w(px(130.0)).text_color(t::text_secondary()).child(*k))
                            .child(val.clone())
                    }))
                    .into_any_element(),
            );
        }
        if self.help {
            let keys = [
                ("Space / click", "Play / pause"),
                ("Left / Right", "Seek 10 s"),
                ("Up / Down", "Volume"),
                ("m", "Mute"),
                ("f / double-click", "Fullscreen"),
                ("Esc", "Close menu, leave fullscreen, stop"),
                ("[ ]  \\", "Speed down / up / reset"),
                (", .", "Frame back / forward"),
                ("PgUp / PgDn", "Previous / next chapter"),
                ("c", "Cycle subtitles"),
                ("z x", "Subtitle delay -/+ 100 ms"),
                ("- =", "Audio delay -/+ 100 ms"),
                ("l", "Repeat one"),
                ("n", "Next episode"),
                ("i", "Playback info"),
                ("Ctrl+S", "Screenshot"),
                ("s", "Stop"),
                ("?", "This help"),
            ];
            v.push(
                panel()
                    .top(px(t::S6 + 48.0))
                    .right(px(t::S6))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .children(keys.iter().map(|(k, d)| {
                        div()
                            .flex()
                            .gap(px(t::S4))
                            .child(
                                div()
                                    .w(px(130.0))
                                    .font_family(t::MONO)
                                    .text_color(t::accent_text())
                                    .child(*k),
                            )
                            .child(*d)
                    }))
                    .into_any_element(),
            );
        }
        v
    }

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
            .max(340.0)
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
                    .child(img(i).h(px(110.0)).object_fit(ObjectFit::Contain)),
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
        if list.is_empty() {
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
            return Self::note_text(
                "This couldn't be loaded. Check your connection, then restart.",
            )
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
                let buttons = div()
                    .flex()
                    .gap(px(t::S3))
                    .child(ui::pill_button(
                        "hero-play",
                        "Play",
                        true,
                        hero_focus == Some(0),
                        cx.listener(move |t, _, _, cx| {
                            t.play_item(m1.clone(), cx);
                            cx.notify();
                        }),
                    ))
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
        self.start_embedded(window, cx);
        if !self.video && self.fs_from_video {
            self.fs_from_video = false;
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        }
        if self.video {
            return self.player_page(cx).into_any_element();
        }
        let body = match self.phase {
            Phase::Account => match self.accounts.clone() {
                Some(a) => a.into_any_element(),
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
            .child(
                div()
                    .size_full()
                    .when(self.now.is_some() || self.leaving.is_some(), |d| {
                        d.pb(px(BAR_H))
                    })
                    .child(body),
            );
        if matches!(self.phase, Phase::Ready) {
            root = root.child(self.header(cx));
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
        if let Some(bar) = self.now_bar(cx) {
            root = root.child(bar);
        }
        if let Some(a) = self.accounts.clone().filter(|a| a.read(cx).dialog_open()) {
            root = root.child(a);
        }
        if let Some(n) = self.note {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(t::S6 + if self.now.is_some() { BAR_H } else { 0.0 }))
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
