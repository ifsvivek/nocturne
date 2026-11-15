//! Root view: connection state, navigation stack, keyboard, and the screens.
use crate::admin::{self, Admin};
use crate::nav::{self, Dir};
use crate::playback::Now;
use crate::source::fmt_clock;
use crate::source::{Art, ConnectError, Kind, Media, Source, sniff};
use crate::ui::{self, Cell, Pic};
use crate::video::{self, AutoHide, Playback};
use crate::{bridge, cache::Cache, theme as t};
use gpui::prelude::*;
use gpui::*;
use nocturne_player::{EmbeddedPlayer, Event, Options, Player};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::cell::Cell as StdCell;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Height of the docked now-playing bar.
const BAR_H: f32 = 76.0;

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
    season_open: bool,
    episodes: Load<Vec<Media>>,
    ep_sel: usize,
}

impl Page {
    fn layout(&self) -> Vec<HRow> {
        let mut v = vec![HRow::Nav, HRow::Hero];
        if !self.resume.items().is_empty() {
            v.push(HRow::Resume);
        }
        if !self.nextup.items().is_empty() {
            v.push(HRow::NextUp);
        }
        v.extend((0..self.rows.len()).map(HRow::Latest));
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
            self.libs.items().len() + self.admin as usize
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
        };
        me.connect(cx);
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
                this.update(cx, |t, _| t.playback = None).ok();
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
                t.source = Some(s);
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

    fn page_mut(&mut self, pid: u64) -> Option<&mut Page> {
        self.stack.iter_mut().find(|p| p.pid == pid)
    }

    fn push(&mut self, view: View, media: Option<Media>, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        self.next_pid += 1;
        let pid = self.next_pid;
        self.stack.push(Page {
            pid,
            view,
            media: media.clone(),
            libs: Load::Loading,
            rows: vec![],
            resume: Load::Loading,
            resume_scroll: ScrollHandle::new(),
            nextup: Load::Loading,
            nextup_scroll: ScrollHandle::new(),
            admin: self.admin.is_some(),
            row: 1,
            col: 0,
            items: Load::Loading,
            sel: 0,
            scroll: UniformListScrollHandle::new(),
            page_scroll: ScrollHandle::new(),
            seasons: Load::Loading,
            season_sel: 0,
            season_open: true,
            episodes: Load::Loading,
            ep_sel: 0,
        });
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
            (View::Grid, Some(m)) => {
                self.run(cx, async move { src.children(&m).await }, move |t, r, _| {
                    if let Some(p) = t.page_mut(pid) {
                        p.items = r.map_or(Load::Failed, Load::Ready);
                    }
                })
            }
            (View::Detail, Some(m)) => {
                let (s2, id) = (src.clone(), m.id.clone());
                // Fuller record (overview) than the list entry carried.
                self.run(cx, async move { s2.detail(&id).await }, move |t, r, _| {
                    if let (Some(p), Ok(full)) = (t.page_mut(pid), r) {
                        p.media = Some(full);
                    }
                });
                if m.is_series() {
                    self.run(
                        cx,
                        async move { src.seasons(&m.id).await },
                        move |t, r, cx| {
                            let first = r.as_ref().ok().and_then(|v| v.first().cloned());
                            if let Some(p) = t.page_mut(pid) {
                                p.seasons = r.map_or(Load::Failed, Load::Ready);
                            }
                            if let Some(s) = first {
                                t.load_episodes(pid, s.id, cx);
                            }
                        },
                    );
                }
            }
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
        match m.kind {
            Kind::Library => self.push(View::Grid, Some(m), cx),
            _ => self.push(View::Detail, Some(m), cx),
        }
    }

    /// Library tabs always replace whatever is above Home.
    fn open_library(&mut self, m: Media, cx: &mut Context<Self>) {
        self.stack.truncate(1);
        self.push(View::Grid, Some(m), cx);
    }

    fn open_admin(&mut self, cx: &mut Context<Self>) {
        if let Some(a) = self.admin.clone() {
            self.admin_open = true;
            a.update(cx, |a, cx| a.open(cx));
        }
    }

    /// Leave Admin (if open) before navigating elsewhere.
    fn close_admin(&mut self, cx: &mut Context<Self>) {
        if self.admin_open {
            self.admin_open = false;
            if let Some(a) = self.admin.clone() {
                a.update(cx, |a, _| a.close());
            }
        }
    }

    fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
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
        self.leaving = None;
        if self.now.is_none() {
            self.bar_gen += 1;
        }
        self.muted_from = None;
        self.now = Some(Now::new(m));
        if ep.kind == Kind::Episode {
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

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.playback = None; // dropping closes mpv / the video surface
        self.video = false;
        self.pending = None;
        self.drag = None;
        self.hover_frac = None;
        let Some(n) = self.now.take() else { return };
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
        let shown = self.video_controls();
        if self.video && shown != self.ctl_shown {
            self.ctl_shown = shown;
            cx.notify();
        }
        let Some(pl) = &self.playback else { return };
        let (mut failed, mut gone) = (false, false);
        if let Some(now) = &mut self.now {
            for ev in pl.events().try_iter() {
                match ev {
                    Event::Loaded => {
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
        if failed {
            self.stop(cx);
            self.toast("This couldn't be played.", cx);
            cx.notify();
        } else if gone {
            self.stop(cx);
            cx.notify();
        }
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

    /// Cached image lookup; starts a fetch the first time a slot is actually drawn (lazy).
    fn pic(&mut self, m: &Media, art: Art, width: u32, cx: &mut Context<Self>) -> Pic {
        let key = format!("{}:{:?}:{width}", m.id, art);
        match self.images.get(&key) {
            Some(Img::Ready(i)) => return Pic::Ready(i.clone()),
            Some(Img::Loading) => return Pic::Loading,
            Some(Img::Failed) => return Pic::Missing,
            None => {}
        }
        let Some(src) = self.source.clone() else {
            return Pic::Loading;
        };
        self.images.insert(key.clone(), Img::Loading);
        let m = m.clone();
        self.run(
            cx,
            async move { src.image(&m, art, width).await },
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
        Pic::Loading
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
    fn video_key(&mut self, k: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        use video::Key;
        self.hide.poke(Instant::now());
        let Some(key) = video::key(k) else {
            return false;
        };
        match key {
            Key::Toggle => self.toggle(),
            Key::Seek(s) => self.seek(s),
            Key::Volume(d) => {
                self.muted_from = None;
                if let Some(p) = &self.playback {
                    let _ = p.set_volume(video::bump_volume(self.volume, d));
                }
            }
            Key::Mute => self.mute(),
            Key::Stop => self.stop(cx),
            Key::Fullscreen => self.toggle_fullscreen(window),
            Key::Back if window.is_fullscreen() => self.toggle_fullscreen(window),
            Key::Back => self.stop(cx),
            Key::Next => self.next_episode(cx),
        }
        cx.notify();
        true
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        if self.video && self.video_key(key, window, cx) {
            return;
        }
        if let Phase::Error(e) = self.phase {
            if matches!(key, "enter" | "r") && e != ConnectError::NotConfigured {
                self.connect(cx);
            }
            return;
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
                .is_some_and(|p| matches!(p.view, View::Home | View::Grid));
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
        let dir = match key {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        let enter = key == "enter";
        let cols = self.cols;
        let Some(p) = self.stack.last_mut() else {
            return;
        };
        match p.view {
            View::Home => {
                let lay = p.layout();
                let kind = lay.get(p.row).copied().unwrap_or(HRow::Hero);
                let mut act = None;
                let admin_col = kind == HRow::Nav && p.admin && p.col == p.libs.items().len();
                if enter && admin_col {
                    self.open_admin(cx);
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
            View::Grid => {
                if let Some(d) = dir {
                    let n = p.items.items().len();
                    p.sel = nav::step(p.sel, cols, n, d);
                    p.scroll
                        .scroll_to_item(p.sel / cols.max(1), ScrollStrategy::Center);
                } else if enter && let Some(m) = p.items.items().get(p.sel).cloned() {
                    self.open(m, cx);
                }
            }
            View::Detail => {
                let pid = p.pid;
                match dir {
                    Some(d @ (Dir::Up | Dir::Down)) => {
                        let n = p.episodes.items().len();
                        p.ep_sel = nav::step(p.ep_sel, 1, n, d);
                        // children: hero, facts, "Episodes" heading, then headers/episodes
                        p.page_scroll
                            .scroll_to_item(3 + p.season_sel + 1 + p.ep_sel);
                    }
                    Some(d @ (Dir::Left | Dir::Right)) => {
                        let n = p.seasons.items().len();
                        let s = nav::step(p.season_sel, n.max(1), n, d);
                        if s != p.season_sel {
                            p.season_sel = s;
                            let id = p.seasons.items()[s].id.clone();
                            self.load_episodes(pid, id, cx);
                        }
                    }
                    None if enter => self.play_current(cx),
                    None => {}
                }
            }
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
        if self.admin.is_some() {
            nav = nav.child(ui::nav_link(
                "nav-admin".into(),
                "Admin",
                self.admin_open,
                focus_col == Some(libs.len()) && !self.admin_open,
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
                    .child(ui::wordmark(t::CAPTION + 1.0))
                    .child(nav),
            )
            .child(ui::mono("YOUR MEDIA. YOUR NIGHT."))
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
        let tip = self
            .hover_frac
            .zip(dur)
            .map(|(f, d)| (f, fmt_clock(f as f64 * d)));
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
        let clock = format!(
            "{} / {}",
            fmt_clock(pos),
            dur.map_or("--:--".into(), fmt_clock)
        );
        let thumb = self.pic(&media, Art::Poster, 160, cx);

        let tb = self.track_b.clone();
        let big = self.bar_hover || matches!(self.drag, Some(Drag::Seek(_)));
        let track = div()
            .id("np-track")
            .relative()
            .w_full()
            .h(px(12.0))
            .flex_none()
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .child(ui::progress_bar(frac, if big { 8.0 } else { 4.0 })),
            )
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
        if ended && has_next {
            ctl = ctl.child(ui::ctl_button(
                "np-next",
                "icons/play.svg",
                "next",
                true,
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
                            .child(ui::progress_bar((vol / 100.0) as f32, 3.0)),
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
            );
        let mut glow = t::glow();
        glow.a = 0.10;
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
            .child(track)
            .child(
                div()
                    .flex_1()
                    .px(px(t::PAGE_PAD))
                    .flex()
                    .items_center()
                    .gap(px(t::S6))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(t::S3))
                            .child(
                                ui::tile(thumb, &media.title, "np-thumb")
                                    .w(px(36.0))
                                    .h(px(48.0))
                                    .flex_none(),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(14.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(t::text())
                                            .child(media.title.clone()),
                                    )
                                    .child(ui::mono(status)),
                            )
                            .child(div().w(px(t::S2)).flex_none())
                            .child(ui::mono(clock).flex_none()),
                    )
                    .child(ctl)
                    .child(volume)
                    .child(ui::ctl_button(
                        "np-stop",
                        "icons/stop.svg",
                        "",
                        false,
                        cx.listener(|t, _, _, cx| {
                            t.stop(cx);
                            cx.notify();
                        }),
                    )),
            )
            .when_some(tip, |d, (f, text)| {
                let w = self.track_b.get().size.width;
                d.child(
                    div()
                        .absolute()
                        .top(px(-30.0))
                        .left(w * f - px(22.0))
                        .px(px(t::S2))
                        .py(px(3.0))
                        .rounded(px(4.0))
                        .bg(t::raised())
                        .border_1()
                        .border_color(t::hairline_strong())
                        .child(ui::mono(text).text_color(t::text())),
                )
            });
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
        let backdrop = match self.pic(m, Art::Backdrop, 1920, cx) {
            Pic::Ready(i) => ui::fade_in(
                div()
                    .size_full()
                    .child(img(i).size_full().object_fit(ObjectFit::Cover)),
                format!("hero-{}", m.id),
                t::SLOW,
            )
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
        let sub = m.overview.clone().unwrap_or_else(|| m.meta_line());
        let text = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(t::S4))
            .max_w(px(680.0))
            .child(ui::mono(eyebrow))
            .child(chips)
            .child(
                div()
                    .line_clamp(2)
                    .text_size(px(size))
                    .line_height(px(size * 1.08))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t::text())
                    .child(m.title.clone()),
            )
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
                })
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
        let (row, col, scroll) = (p.row, p.col, p.page_scroll.clone());
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
        let size = self.row_dims(190.0, 1.5);
        let wide = self.row_dims(300.0, 9.0 / 16.0);
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
            if items.items().is_empty() {
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

    // ---- Grid ----

    fn grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.stack.last() else {
            return div().into_any_element();
        };
        let title = p
            .media
            .as_ref()
            .map(|m| m.title.clone())
            .unwrap_or_default();
        let kind = p.media.as_ref().map(|m| m.kind_label()).unwrap_or_default();
        let n = p.items.items().len();
        let avail = self.colw() - 2.0 * t::PAGE_PAD - 1.0;
        self.cols = nav::columns(avail, 176.0 + 2.0 * t::S3, 0.0);
        let cols = self.cols;
        let cell_w = (avail / cols as f32).floor();
        let (w, h) = (
            cell_w - 2.0 * t::S3 - 1.0,
            ((cell_w - 2.0 * t::S3 - 1.0) * 1.5).round(),
        );
        let rows = n.div_ceil(cols);
        let meta = match &p.items {
            Load::Ready(_) => format!("{kind} · {n} titles"),
            _ => kind,
        };
        let body: AnyElement = match &p.items {
            Load::Loading => div()
                .flex()
                .children((0..cols.min(8)).map(|i| {
                    ui::poster_cell(Cell {
                        key: &format!("gsk-{i}"),
                        pic: Pic::Loading,
                        title: "",
                        meta: String::new(),
                        index: i,
                        w,
                        h,
                        focused: false,
                        progress: None,
                    })
                }))
                .into_any_element(),
            Load::Failed => {
                Self::note_text("This couldn't be loaded. Press Esc to go back and try again.")
                    .into_any_element()
            }
            Load::Ready(_) if n == 0 => Self::note_text("Nothing here yet.").into_any_element(),
            Load::Ready(_) => uniform_list(
                "grid",
                rows,
                cx.processor(move |this, range: std::ops::Range<usize>, _w, cx| {
                    range
                        .map(|r| this.grid_row(r, cols, w, h, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(p.scroll.clone())
            .size_full()
            .into_any_element(),
        };
        div()
            .size_full()
            .pt(px(t::HEADER_H + t::S8))
            .flex()
            .flex_col()
            .child(ui::section(
                &title,
                "Everything in this library, sorted by name.",
                meta,
            ))
            .child(div().h(px(t::S6)).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .mx(px(t::PAGE_PAD))
                    .mb(px(t::S6))
                    .border_t_1()
                    .border_l_1()
                    .border_color(t::hairline())
                    .rounded(px(t::R_PANEL))
                    .overflow_hidden()
                    .child(body),
            )
            .into_any_element()
    }

    fn grid_row(
        &mut self,
        r: usize,
        cols: usize,
        w: f32,
        h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (items, sel) = match self.stack.last() {
            Some(p) => (p.items.items().to_vec(), p.sel),
            None => return div().into_any_element(),
        };
        let end = ((r + 1) * cols).min(items.len());
        let mut row = div().h(px(h + CELL_EXTRA)).flex();
        for (idx, m) in items.iter().enumerate().take(end).skip(r * cols) {
            let pic = self.pic(m, Art::Poster, (w * 2.0) as u32, cx);
            let target = m.clone();
            row = row.child(
                ui::poster_cell(Cell {
                    key: &format!("g-{}", m.id),
                    pic,
                    title: &m.title,
                    meta: [m.kind_label(), m.meta_line()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                    index: idx,
                    w,
                    h,
                    focused: idx == sel,
                    progress: m.progress(),
                })
                .on_hover(cx.listener(move |t, hov: &bool, _, cx| {
                    if *hov
                        && let Some(p) = t.stack.last_mut()
                        && p.sel != idx
                    {
                        p.sel = idx;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |t, _, _, cx| {
                    t.open(target.clone(), cx);
                    cx.notify();
                })),
            );
        }
        row.into_any_element()
    }

    // ---- Detail ----

    fn detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.stack.last() else {
            return div().into_any_element();
        };
        let Some(m) = p.media.clone() else {
            return div().into_any_element();
        };
        let (seasons, season_sel, season_open, episodes, ep_sel, scroll, pid) = (
            p.seasons.clone(),
            p.season_sel,
            p.season_open,
            p.episodes.clone(),
            p.ep_sel,
            p.page_scroll.clone(),
            p.pid,
        );
        let buttons = div().flex().gap(px(t::S3)).child(ui::pill_button(
            "play",
            "Play",
            true,
            !m.is_series(),
            cx.listener(move |t, _, _, cx| {
                t.play_current(cx);
                cx.notify();
            }),
        ));
        let hero = self.hero_band(&m, format!("0x01 · {}", m.kind_label()), buttons, 0.55, cx);

        // Metadata as feature cells (only what the server told us).
        let mut facts: Vec<(String, String, String)> = vec![];
        if let Some(y) = m.year {
            facts.push(("year".into(), y.to_string(), "Release year".into()));
        }
        if let Some(r) = m.runtime_min {
            facts.push((
                "runtime".into(),
                crate::source::fmt_runtime(r),
                "Total running time".into(),
            ));
        }
        if let Some(r) = m.rating {
            facts.push(("rating".into(), format!("{r:.1}"), "Community score".into()));
        }
        if m.is_series() {
            facts.push((
                "seasons".into(),
                seasons.items().len().to_string(),
                "Seasons on the server".into(),
            ));
        }
        facts.truncate(3);

        let mut page = div()
            .id("detail")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .flex()
            .flex_col()
            .child(ui::band(hero));
        if !facts.is_empty() {
            let missing = 3 - facts.len();
            let mut r = div().flex().border_t_1().border_color(t::hairline());
            for (i, (k, v, txt)) in facts.into_iter().enumerate() {
                r = r.child(ui::feature_cell(ui::Feature {
                    key: &format!("fact-{k}"),
                    top_l: format!("0x{:02x}", i + 1),
                    top_r: k.clone(),
                    pic: None,
                    title: v,
                    text: txt,
                    foot: m.kind_label(),
                    focused: false,
                }));
            }
            for _ in 0..missing {
                r = r.child(
                    div()
                        .flex_1()
                        .border_r_1()
                        .border_b_1()
                        .border_color(t::hairline()),
                );
            }
            page = page.child(ui::band(
                div()
                    .mx(px(t::PAGE_PAD))
                    .mt(px(t::S8))
                    .mb(px(t::S8))
                    .border_l_1()
                    .border_color(t::hairline())
                    .child(r),
            ));
        }

        if m.is_series() {
            // Accordion: one season open at a time (click the open one to collapse).
            let total = seasons.items().len();
            page = page.child(ui::band(div().py(px(t::S8)).child(ui::section(
                "Episodes",
                "Pick a season to see its episodes.",
                format!("{total} seasons"),
            ))));
            for (i, s) in seasons.items().iter().enumerate() {
                let id = s.id.clone();
                let open = i == season_sel && season_open;
                let header = div()
                    .id(ui::name(format!("season-{i}")))
                    .px(px(t::PAGE_PAD))
                    .h(px(64.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .hover(|s| s.bg(t::glass()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(t::S4))
                            .child(ui::mono(format!("0x{:02x}", i + 1)))
                            .child(
                                div()
                                    .text_size(px(t::BODY_LG))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if open { t::accent_text() } else { t::text() })
                                    .child(s.title.clone()),
                            ),
                    )
                    .child(ui::mono(if open {
                        "collapse  \u{25B4}"
                    } else {
                        "expand  \u{25BE}"
                    }))
                    .on_click(cx.listener(move |t, _, _, cx| {
                        let same = t.page_mut(pid).map(|p| p.season_sel == i).unwrap_or(false);
                        if let Some(p) = t.page_mut(pid) {
                            if same {
                                p.season_open = !p.season_open;
                            } else {
                                p.season_open = true;
                                p.season_sel = i;
                            }
                        }
                        if !same {
                            t.load_episodes(pid, id.clone(), cx);
                        }
                        cx.notify();
                    }));
                page = page.child(ui::band(header));
                if open {
                    match &episodes {
                        Load::Loading => {
                            page = page.child(ui::band(
                                div().p(px(t::S6)).child(ui::mono("loading episodes")),
                            ))
                        }
                        Load::Failed => {
                            page = page.child(ui::band(
                                div()
                                    .p(px(t::S6))
                                    .text_color(t::text_secondary())
                                    .child("Episodes couldn't be loaded."),
                            ))
                        }
                        Load::Ready(v) => {
                            for (ei, e) in v.iter().enumerate() {
                                let pic = self.pic(e, Art::Poster, 320, cx);
                                let id = e.id.clone();
                                let on = ei == ep_sel;
                                let row = div()
                                    .id(ui::name(format!("ep-{ei}")))
                                    .px(px(t::PAGE_PAD))
                                    .py(px(t::S3))
                                    .flex()
                                    .items_center()
                                    .gap(px(t::S4))
                                    .bg(if on { t::accent_tint() } else { rgba(0) })
                                    .cursor_pointer()
                                    .hover(|s| s.bg(t::glass()))
                                    .on_click(cx.listener(move |t, _, _, cx| {
                                        if let Some(e) = t.page_mut(pid).and_then(|p| {
                                            p.episodes.items().iter().find(|e| e.id == id).cloned()
                                        }) {
                                            t.play_item(e, cx);
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .w(px(40.0))
                                            .child(ui::mono(format!("0x{:02x}", ei + 1))),
                                    )
                                    .child(
                                        ui::tile(pic, &e.title, &format!("ep-{}", e.id))
                                            .w(px(128.0))
                                            .h(px(72.0))
                                            .flex_none(),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.0))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(t::BODY))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(if on {
                                                        t::text()
                                                    } else {
                                                        t::text_secondary()
                                                    })
                                                    .child(e.title.clone()),
                                            )
                                            .child(ui::mono(
                                                [
                                                    e.episode_label(),
                                                    e.runtime_min
                                                        .map(crate::source::fmt_runtime)
                                                        .unwrap_or_default(),
                                                ]
                                                .into_iter()
                                                .filter(|s| !s.is_empty())
                                                .collect::<Vec<_>>()
                                                .join(" · "),
                                            )),
                                    );
                                page = page.child(ui::band(row));
                            }
                        }
                    }
                }
            }
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
        }
        root = root.child(layer(self.grain.clone(), 0.02));
        if let Some(bar) = self.now_bar(cx) {
            root = root.child(bar);
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
