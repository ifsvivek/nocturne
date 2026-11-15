//! Root view: connection state, navigation stack, keyboard, and the screens.
use crate::nav::{self, Dir};
use crate::source::{Art, ConnectError, Kind, Media, Source, sniff};
use crate::ui::{self, Cell, Pic};
use crate::{bridge, cache::Cache, theme as t};
use gpui::prelude::*;
use gpui::*;
use std::future::Future;
use std::sync::{Arc, mpsc::Sender};

/// Handed to the player (later task). Carries only what playback needs.
#[derive(Clone, Debug)]
#[allow(dead_code)] // read by the player in a later task
pub struct PlayRequest {
    pub item_id: String,
}

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

struct Page {
    pid: u64,
    view: View,
    media: Option<Media>,
    // Home: libraries + a "latest" row per library. row: -1 nav, 0 hero, 1.. rows.
    libs: Load<Vec<Media>>,
    rows: Vec<HomeRow>,
    row: isize,
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
    /// Item shown in the hero: the focused poster, else the newest of the first row.
    fn hero_item(&self) -> Option<Media> {
        let focused = (self.row >= 1)
            .then(|| self.rows.get(self.row as usize - 1))
            .flatten()
            .and_then(|r| r.items.items().get(self.col).cloned());
        focused.or_else(|| {
            self.rows
                .iter()
                .find_map(|r| r.items.items().first().cloned())
        })
    }
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
    play_tx: Sender<PlayRequest>,
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
        play_tx: Sender<PlayRequest>,
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
            play_tx,
            grain: ui::grain(),
            dots: ui::dots(),
            glow: ui::glow(),
            cols: 4,
            win: size(px(1280.0), px(800.0)),
            note: None,
            note_gen: 0,
        };
        me.connect(cx);
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
            row: 0,
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

    fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    fn play(&mut self, item_id: String, cx: &mut Context<Self>) {
        let _ = self.play_tx.send(PlayRequest { item_id });
        self.note = Some("Play request sent");
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

    /// Play for the focused thing. A series has no single stream: it opens its detail page.
    fn play_item(&mut self, m: Media, cx: &mut Context<Self>) {
        if m.is_series() {
            self.open(m, cx);
        } else {
            self.play(m.id, cx);
        }
    }

    fn play_current(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.stack.last() else { return };
        let m = if p.view == View::Home {
            p.hero_item()
        } else {
            p.media.clone()
        };
        let Some(m) = m else { return };
        if p.view == View::Detail && m.is_series() {
            if let Some(e) = p.episodes.items().get(p.ep_sel) {
                let id = e.id.clone();
                self.play(id, cx);
            }
        } else {
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

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        if let Phase::Error(e) = self.phase {
            if matches!(key, "enter" | "r") && e != ConnectError::NotConfigured {
                self.connect(cx);
            }
            return;
        }
        if matches!(key, "escape" | "backspace") {
            self.back();
            cx.notify();
            return;
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
                if let Some(d) = dir {
                    let nrows = p.rows.len() as isize;
                    match d {
                        Dir::Up => p.row = (p.row - 1).max(-1),
                        Dir::Down => p.row = (p.row + 1).min(nrows),
                        Dir::Left | Dir::Right => {
                            let n = match p.row {
                                -1 => p.libs.items().len(),
                                0 => 2,
                                r => p.rows[r as usize - 1].items.items().len(),
                            };
                            p.col = nav::step(p.col, n.max(1), n, d);
                        }
                    }
                    // Keep the column valid for the row we landed on.
                    let n = match p.row {
                        -1 => p.libs.items().len(),
                        0 => 2,
                        r => p.rows[r as usize - 1].items.items().len(),
                    };
                    p.col = p.col.min(n.saturating_sub(1));
                    // children: hero(0), libraries(1), latest rows(2..)
                    p.page_scroll
                        .scroll_to_item(if p.row <= 0 { 0 } else { p.row as usize + 1 });
                    if p.row >= 1 {
                        p.rows[p.row as usize - 1].scroll.scroll_to_item(p.col);
                    }
                } else if enter {
                    match p.row {
                        -1 => {
                            if let Some(l) = p.libs.items().get(p.col).cloned() {
                                self.open_library(l, cx);
                            }
                        }
                        0 if p.col == 0 => self.play_current(cx),
                        0 => {
                            if let Some(m) = p.hero_item() {
                                self.open(m, cx);
                            }
                        }
                        r => {
                            if let Some(m) =
                                p.rows[r as usize - 1].items.items().get(p.col).cloned()
                            {
                                self.open(m, cx);
                            }
                        }
                    }
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

    fn hover_home(&mut self, row: isize, col: usize, cx: &mut Context<Self>) {
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
                    .filter(|p| p.view == View::Home && p.row == -1)
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
            .filter(|p| p.view == View::Home && p.row == -1)
            .map(|p| p.col);
        let mut nav = div().flex().items_center().gap(px(t::S1));
        nav = nav.child(ui::nav_link(
            "nav-home".into(),
            "Home",
            at_home && focus_col.is_none(),
            false,
            cx.listener(|t, _, _, cx| {
                t.stack.truncate(1);
                cx.notify();
            }),
        ));
        for (i, l) in libs.iter().enumerate() {
            let m = l.clone();
            nav = nav.child(ui::nav_link(
                format!("nav-{i}"),
                &l.title,
                top.map(|(a, _)| a == i).unwrap_or(false) && !at_home,
                focus_col == Some(i),
                cx.listener(move |t, _, _, cx| {
                    t.open_library(m.clone(), cx);
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

    // ---- hero (shared by Home and Detail) ----

    fn colw(&self) -> f32 {
        f32::from(self.win.width).min(t::COL_W) - 2.0
    }

    /// Centered hero band: chips, big headline, muted subline, buttons, then the backdrop
    /// framed like an app window (hairline frame, dim title-bar dots, soft blue glow).
    fn hero_band(
        &mut self,
        m: &Media,
        eyebrow: String,
        buttons: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let frame_w = (self.colw() - 2.0 * t::PAGE_PAD).min(1040.0);
        let frame_h = (frame_w * 0.5).round();
        let backdrop = match self.pic(m, Art::Backdrop, 1600, cx) {
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
            .justify_center()
            .gap(px(t::S2))
            .child(ui::chip(&m.kind_label(), true));
        if let Some(y) = m.year {
            chips = chips.child(ui::chip(&y.to_string(), false));
        }
        if let Some(r) = m.runtime_min {
            chips = chips.child(ui::chip(&crate::source::fmt_runtime(r), false));
        }
        let sub = m.overview.clone().unwrap_or_else(|| m.meta_line());
        let text = div()
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .gap(px(t::S4))
            .child(ui::mono(eyebrow))
            .child(chips)
            .child(
                div()
                    .max_w(px(860.0))
                    .line_clamp(2)
                    .text_size(px(t::DISPLAY))
                    .line_height(px(t::DISPLAY * 1.08))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t::text())
                    .child(m.title.clone()),
            )
            .child(
                div()
                    .max_w(px(600.0))
                    .line_clamp(2)
                    .line_height(px(t::BODY_LG * 1.6))
                    .text_size(px(t::BODY_LG))
                    .text_color(t::text_secondary())
                    .child(sub),
            )
            .child(buttons);
        let dots = div()
            .flex()
            .gap(px(6.0))
            .children((0..3).map(|_| div().size(px(8.0)).rounded_full().bg(t::hairline_strong())));
        let window_frame = div()
            .w(px(frame_w))
            .flex_none()
            .border_1()
            .border_color(t::hairline_strong())
            .rounded(px(t::R_PANEL))
            .overflow_hidden()
            .bg(t::surface())
            .shadow(vec![BoxShadow {
                color: {
                    let mut c: Hsla = t::glow().into();
                    c.a = 0.28;
                    c
                },
                offset: point(px(0.0), px(24.0)),
                blur_radius: px(80.0),
                spread_radius: px(0.0),
            }])
            .child(
                div()
                    .h(px(32.0))
                    .px(px(t::S3))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(t::hairline())
                    .child(dots)
                    .child(ui::mono(format!("nocturne · {}", m.kind_label())))
                    .child(div().w(px(36.0))),
            )
            .child(div().w_full().h(px(frame_h)).child(backdrop));
        div()
            .pt(px(t::HEADER_H + t::S8))
            .pb(px(t::S8 * 1.5))
            .px(px(t::PAGE_PAD))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(t::S8))
            .child(ui::fade_in(text, format!("herotext-{}", m.id), t::SLOW))
            .child(window_frame)
    }

    // ---- Home ----

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
        let hero = p.hero_item();
        let rows: Vec<(Media, Load<Vec<Media>>, ScrollHandle)> = p
            .rows
            .iter()
            .map(|r| (r.lib.clone(), r.items.clone(), r.scroll.clone()))
            .collect();
        let cw = ((self.colw() - 2.0 * t::PAGE_PAD) / 7.0)
            .clamp(140.0, 210.0)
            .floor();
        let ch = (cw * 1.5).round();

        let hero_el = match hero {
            Some(m) => {
                let eyebrow = format!("0x00 · {}", m.kind_label());
                let (m1, m2) = (m.clone(), m.clone());
                let buttons = div()
                    .flex()
                    .gap(px(t::S3))
                    .child(ui::pill_button(
                        "hero-play",
                        "Play",
                        true,
                        row == 0 && col == 0,
                        cx.listener(move |t, _, _, cx| {
                            t.play_item(m1.clone(), cx);
                            cx.notify();
                        }),
                    ))
                    .child(ui::pill_button(
                        "hero-more",
                        "More info",
                        false,
                        row == 0 && col == 1,
                        cx.listener(move |t, _, _, cx| {
                            t.open(m2.clone(), cx);
                            cx.notify();
                        }),
                    ));
                self.hero_band(&m, eyebrow, buttons, cx)
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

        // Library cells (3-up, shared borders). Mouse-driven; the top bar covers the keyboard.
        let lib_cells: Vec<Stateful<Div>> = {
            let libs: Vec<(Media, usize)> = rows
                .iter()
                .map(|(l, i, _)| (l.clone(), i.items().len()))
                .collect();
            let mut v = vec![];
            for (i, (lib, n)) in libs.into_iter().enumerate() {
                let pic = self.pic(&lib, Art::Poster, 640, cx);
                let target = lib.clone();
                v.push(
                    ui::feature_cell(ui::Feature {
                        key: &format!("lib-{i}"),
                        top_l: format!("0x{:02x}", i + 1),
                        top_r: lib.kind_label(),
                        pic: Some(pic),
                        title: lib.title.clone(),
                        text: format!("Browse everything in {}, newest first.", lib.title),
                        foot: if n > 0 {
                            format!("{n} recent titles")
                        } else {
                            String::new()
                        },
                    })
                    .on_click(cx.listener(move |t, _, _, cx| {
                        t.open_library(target.clone(), cx);
                        cx.notify();
                    })),
                );
            }
            v
        };
        if !lib_cells.is_empty() {
            let n = lib_cells.len();
            let mut grid = div()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(t::hairline());
            let mut it = lib_cells.into_iter();
            loop {
                let chunk: Vec<_> = it.by_ref().take(3).collect();
                if chunk.is_empty() {
                    break;
                }
                let missing = 3 - chunk.len();
                let mut r = div().flex();
                for c in chunk {
                    r = r.child(c);
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
                grid = grid.child(r);
            }
            page = page.child(ui::band(
                div()
                    .pt(px(t::S8 * 1.5))
                    .flex()
                    .flex_col()
                    .gap(px(t::S6))
                    .child(ui::section(
                        "Your libraries",
                        "Everything you have, one tap away.",
                        format!("{n} libraries"),
                    ))
                    .child(
                        div()
                            .mx(px(t::PAGE_PAD))
                            .mb(px(t::S8 * 1.5))
                            .border_l_1()
                            .border_color(t::hairline())
                            .child(grid),
                    ),
            ));
        }

        for (ri, (lib, items, handle)) in rows.into_iter().enumerate() {
            let list = items.items().to_vec();
            let meta = match &items {
                Load::Ready(_) => format!("{} · {} titles", lib.kind_label(), list.len()),
                _ => lib.kind_label(),
            };
            let mut strip = div()
                .id(ui::name(format!("strip-{ri}")))
                .flex()
                .overflow_x_scroll()
                .track_scroll(&handle);
            if list.is_empty() {
                let loading = matches!(items, Load::Loading);
                for i in 0..8 {
                    strip = strip.child(ui::poster_cell(Cell {
                        key: &format!("sk-{ri}-{i}"),
                        pic: if loading { Pic::Loading } else { Pic::Missing },
                        title: "",
                        meta: String::new(),
                        index: i,
                        w: cw,
                        h: ch,
                        focused: false,
                    }));
                }
            }
            for (ci, m) in list.iter().enumerate() {
                let pic = self.pic(m, Art::Poster, (cw * 2.0) as u32, cx);
                let target = m.clone();
                let focused = row == ri as isize + 1 && col == ci;
                strip = strip.child(
                    ui::poster_cell(Cell {
                        key: &format!("{ri}-{}", m.id),
                        pic,
                        title: &m.title,
                        meta: [m.kind_label(), m.meta_line()]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · "),
                        index: ci,
                        w: cw,
                        h: ch,
                        focused,
                    })
                    .on_hover(cx.listener(move |t, hov: &bool, _, cx| {
                        if *hov {
                            t.hover_home(ri as isize + 1, ci, cx);
                        }
                    }))
                    .on_click(cx.listener(move |t, _, _, cx| {
                        t.open(target.clone(), cx);
                        cx.notify();
                    })),
                );
            }
            page = page.child(ui::band(
                div()
                    .pt(px(t::S8 * 1.5))
                    .pb(px(t::S8 * 1.5))
                    .flex()
                    .flex_col()
                    .gap(px(t::S6))
                    .child(ui::section(
                        &format!("Latest in {}", lib.title),
                        "Recently added, newest first.",
                        meta,
                    ))
                    .child(
                        div()
                            .mx(px(t::PAGE_PAD))
                            .border_t_1()
                            .border_l_1()
                            .border_color(t::hairline())
                            .rounded(px(t::R_PANEL))
                            .overflow_hidden()
                            .child(strip),
                    ),
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
        let column = div()
            .w_full()
            .max_w(px(t::COL_W))
            .h_full()
            .border_l_1()
            .border_r_1()
            .border_color(t::hairline())
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
            );
        div()
            .size_full()
            .flex()
            .justify_center()
            .child(column)
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
        let hero = self.hero_band(&m, format!("0x01 · {}", m.kind_label()), buttons, cx);

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
                                        t.play(id.clone(), cx);
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
        let mut root = div()
            .id("root")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|t, ev, _, cx| t.on_key(ev, cx)))
            .size_full()
            .relative()
            .font_family(t::FONT)
            .text_color(t::text())
            .bg(t::frame())
            .child(layer(self.glow.clone(), 1.0))
            .child(layer(self.dots.clone(), 0.09))
            .child(div().size_full().child(body));
        if matches!(self.phase, Phase::Ready) {
            root = root.child(self.header(cx));
        }
        root = root.child(layer(self.grain.clone(), 0.02));
        if let Some(n) = self.note {
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
        root
    }
}
