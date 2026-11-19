//! Preview of the theme engine (feature `theme-engine`, never in a normal build): `NOCTURNE_PREVIEW=theme-engine` shows a static
//! Home screen as an element tree, styled by the CSS cascade and laid out by Taffy (`crates/style`), painted by the `styled`
//! widget, with a `text_input` slot in the header (typing works), a video-hole slot, keyboard navigation and an optional
//! virtualised library grid. Launch through `scripts/run-isolated.sh`. Environment:
//! - `NOCTURNE_ENGINE_THEME=streaming|videosite|rail|dock` (a bundled stylesheet) or `NOCTURNE_ENGINE_CSS=<file>` (re-read when it changes)
//! - `NOCTURNE_ENGINE_VIRTUAL=<n>` a library grid of n items instead of Home, `NOCTURNE_ENGINE_HOLE=1` a video-hole slot
//! - `NOCTURNE_ENGINE_SCROLL_Y=<px>` initial page scroll, `NOCTURNE_ENGINE_HOVER=<class>:<n>` pretend the pointer is over the n-th such element
//! - `NOCTURNE_ENGINE_STATS=1` print restyle and layout timings, `NOCTURNE_SAFE_THEME=1` start in safe mode (Ctrl+Shift+0 toggles)
//! - `NOCTURNE_SNAP=/tmp/x.png`, `NOCTURNE_SNAP_AFTER_MS=1500,3000` window captures (debug builds), `NOCTURNE_PREVIEW_SIZE=1920x1080`
use super::hole::hole;
use super::styled::{Nav, Styled, StyledEvent, font_for};
use crate::theme;
use iced::advanced::graphics::text::Paragraph;
use iced::advanced::text::{Alignment, Paragraph as _, Text};
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::widget::{Id as WidgetId, operation, text_input};
use iced::{Element, Pixels, Rectangle, Size, Subscription, Task, alignment, window};
use nocturne_style::{
    Builders, Doc, Engine, Id, Interaction, MeasureCache, Scene, TextStyle, VirtBuilder, demo,
};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone)]
enum M {
    Win(window::Id),
    Ev(StyledEvent),
    Typed(String),
    Hole(Rectangle),
    Poll,
    FocusSearch,
    Snap(usize),
    Shot(window::Screenshot, usize),
}

impl std::fmt::Debug for M {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("M")
    }
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// Plain ASCII text is shaped the way the hand-written pages' `text()` widgets shape it (Iced's basic shaping), so a label measures and lands on the same pixels; anything else (accents, emoji, scripts, the tracked labels' hair spaces) needs the advanced shaper.
pub(crate) fn shaping_of(s: &str) -> Shaping {
    if s.is_ascii() {
        Shaping::Basic
    } else {
        Shaping::Advanced
    }
}

/// The real text shaper (cosmic-text through Iced); the cache sits in front of it in `Preview::measure`.
pub(crate) fn shape(text: &str, st: &TextStyle, max: Option<f32>) -> (f32, f32) {
    let p = Paragraph::with_text(Text {
        content: text,
        bounds: Size::new(max.unwrap_or(f32::INFINITY), f32::INFINITY),
        size: Pixels(st.size),
        line_height: LineHeight::Relative(st.line_height),
        font: font_for(st.weight, st.italic, &st.font),
        align_x: Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: shaping_of(text),
        wrapping: Wrapping::Word,
    });
    let b = p.min_bounds();
    (b.width, b.height)
}

struct Preview {
    engine: Engine,
    doc: Doc,
    builders: Builders,
    scene: Option<Scene>,
    css_path: Option<PathBuf>,
    mtime: Option<SystemTime>,
    size: Size,
    it: Interaction,
    cache: MeasureCache,
    typed: String,
    hole: Rectangle,
    win: Option<window::Id>,
    stats: bool,
    pinned_hover: bool,
}

fn mtime(p: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn library(n: usize) -> (Doc, Builders) {
    fn card(d: &mut Doc, parent: Id, i: usize) -> Id {
        let c = d.add(parent, "a.card");
        d.attr(c, "data-kind", "movie");
        d.focusable(c);
        let t = d.add(c, "div.thumb");
        d.inline(
            t,
            &format!(
                "background: linear-gradient(135deg, hsl({}, 55%, 42%), hsl({}, 60%, 16%))",
                (i * 37) % 360,
                (i * 37 + 50) % 360
            ),
        );
        let ti = d.add(c, "div.card-title");
        d.text(ti, &format!("Library item {i}"));
        c
    }
    let mut d = Doc::new("page");
    let header = d.add(0, "header.header");
    let logo = d.add(header, "a.logo");
    d.text(logo, "NOCTURNE");
    let nav = d.add(header, "nav.nav");
    for (i, l) in ["Home", "Movies", "Shows"].iter().enumerate() {
        let a = d.add(
            nav,
            if i == 1 {
                "a.nav-link.active"
            } else {
                "a.nav-link"
            },
        );
        d.text(a, l);
    }
    d.add(header, "div.search");
    let main = d.add(0, "main.main");
    let grid = d.add(main, "div.library");
    d.virtual_list(grid, n);
    let proto = card(&mut d, grid, 0);
    d.virtual_proto(proto);
    let mut b = Builders::new();
    b.insert(
        grid,
        Rc::new(|d: &mut Doc, p: Id, i: usize| {
            card(d, p, i);
        }) as VirtBuilder,
    );
    (d, b)
}

fn boot() -> (Preview, Task<M>) {
    let mut engine = Engine::new("");
    let (user_css, css_path) = if let Some(p) = env("NOCTURNE_ENGINE_CSS") {
        (
            std::fs::read_to_string(&p).unwrap_or_default(),
            Some(PathBuf::from(p)),
        )
    } else {
        let name = env("NOCTURNE_ENGINE_THEME").unwrap_or_else(|| "streaming".into());
        let css = nocturne_style::THEMES
            .iter()
            .find(|t| t.0 == name)
            .map_or("", |t| t.1);
        (css.to_owned(), None)
    };
    let extra = if env("NOCTURNE_ENGINE_VIRTUAL").is_some() {
        ".library { padding: 24px 48px } .card { display: block; width: 180px; margin-right: 12px; margin-bottom: 20px } .thumb { aspect-ratio: 2 / 3; border-radius: 8px } .card-title { height: 18px }"
    } else {
        ""
    };
    // A debug build is slow; the budget exists to stop a hostile theme, not to time this preview.
    engine.set_budget(Duration::from_millis(1500));
    engine.set_user_css(Some(&format!("{user_css}\n{extra}")));
    let (mut doc, builders) = match env("NOCTURNE_ENGINE_VIRTUAL").and_then(|n| n.parse().ok()) {
        Some(n) => library(n),
        None => (demo::home(), Builders::new()),
    };
    // The header search box hosts a real text input; the app marks controls essential and focusable.
    for i in 0..doc.len() {
        if doc.nodes[i].classes.iter().any(|c| c == "search") {
            doc.nodes[i].text = None;
            doc.inline(
                i,
                "width: 280px; height: 34px; padding: 0 10px; display: flex; align-items: center",
            );
            doc.slot(i, 1);
            doc.essential(i);
        }
        if doc.nodes[i]
            .classes
            .iter()
            .any(|c| c == "logo" || c == "button" || c == "nav-link" || c == "card")
        {
            doc.focusable(i);
        }
        if doc.nodes[i].classes.iter().any(|c| c == "logo") {
            doc.essential(i);
        }
    }
    if env("NOCTURNE_ENGINE_HOLE").is_some()
        && let Some(main) = doc.nodes[0]
            .children
            .iter()
            .copied()
            .find(|&c| doc.nodes[c].tag == "main")
    {
        let v = doc.add(main, "div.video");
        doc.inline(v, "height: 220px; margin: 0 48px 40px");
        doc.slot(v, 2);
    }
    let hover = env("NOCTURNE_ENGINE_HOVER").and_then(|h| {
        let (class, n) = h.split_once(':')?;
        (0..doc.len())
            .filter(|&i| doc.nodes[i].classes.iter().any(|c| c == class))
            .nth(n.parse().ok()?)
    });
    let mut tasks = vec![window::oldest().and_then(|id| Task::done(M::Win(id)))];
    // Headless sessions have no pointer: `NOCTURNE_ENGINE_FOCUS_SEARCH=1` focuses the text slot so typing can be tested.
    if env("NOCTURNE_ENGINE_FOCUS_SEARCH").is_some() {
        tasks.push(crate::task::after(
            Duration::from_millis(800),
            M::FocusSearch,
        ));
    }
    if env("NOCTURNE_SNAP").is_some() {
        let ms: Vec<u64> = env("NOCTURNE_SNAP_AFTER_MS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        for (n, ms) in if ms.is_empty() { vec![1500] } else { ms }
            .into_iter()
            .enumerate()
        {
            tasks.push(crate::task::after(Duration::from_millis(ms), M::Snap(n)));
        }
    }
    let mut p = Preview {
        engine,
        doc,
        builders,
        scene: None,
        mtime: css_path.as_ref().and_then(mtime),
        css_path,
        size: Size::ZERO,
        it: Interaction {
            hover,
            ..Interaction::default()
        },
        cache: MeasureCache::new(),
        typed: String::new(),
        hole: Rectangle::default(),
        win: None,
        stats: env("NOCTURNE_ENGINE_STATS").is_some(),
        pinned_hover: hover.is_some(),
    };
    // The first frame needs a scene to host the widget that reports the real size.
    p.size = window_size();
    p.rebuild();
    (p, Task::batch(tasks))
}

fn window_size() -> Size {
    env("NOCTURNE_PREVIEW_SIZE")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some(Size::new(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(Size::new(1920.0, 1080.0))
}

impl Preview {
    fn rebuild(&mut self) {
        if self.size.width < 1.0 {
            return;
        }
        let cache = &mut self.cache;
        let t = Instant::now();
        let mut s = self.engine.build(
            &self.doc,
            (self.size.width, self.size.height),
            self.it,
            &self.builders,
            &mut |text, st, max| cache.measure(text, st, max, &mut shape),
        );
        if let Some(y) = env("NOCTURNE_ENGINE_SCROLL_Y").and_then(|s| s.parse().ok()) {
            s.set_page_scroll(y, &mut |text, st, max| {
                cache.measure(text, st, max, &mut shape)
            });
        }
        for n in self.engine.take_notices() {
            eprintln!("engine: {n}");
        }
        if self.stats {
            eprintln!(
                "engine: {} nodes, {} rules: cascade {} us, layout {} us, total {} us; text cache {} hits {} misses",
                s.doc.len(),
                s.sheet.rule_count(),
                s.timing.cascade.as_micros(),
                s.timing.layout.as_micros(),
                t.elapsed().as_micros(),
                self.cache.hits,
                self.cache.misses
            );
            for l in s.warnings().lines() {
                eprintln!("engine:   {l}");
            }
        }
        self.scene = Some(s);
    }

    fn restyle(&mut self) {
        let Some(s) = &mut self.scene else { return };
        let cache = &mut self.cache;
        let t = Instant::now();
        let r = s.update(self.it, Instant::now(), &mut |text, st, max| {
            cache.measure(text, st, max, &mut shape)
        });
        if self.stats {
            eprintln!("engine: update {r:?} in {} us", t.elapsed().as_micros());
        }
        if r.is_err() {
            self.rebuild();
        }
    }

    fn update(&mut self, m: M) -> Task<M> {
        match m {
            M::Win(id) => self.win = Some(id),
            M::FocusSearch => return operation::focus(WidgetId::new("engine-search")),
            M::Typed(t) => {
                if self.stats {
                    eprintln!("engine: typed {t:?}");
                }
                self.typed = t;
            }
            M::Hole(b) => self.hole = b,
            M::Ev(e) => return self.event(e),
            M::Poll => {
                if let Some(p) = &self.css_path {
                    let t = mtime(p);
                    if t != self.mtime {
                        self.mtime = t;
                        self.engine
                            .set_user_css(Some(&std::fs::read_to_string(p).unwrap_or_default()));
                        self.rebuild();
                    }
                }
            }
            M::Snap(n) => {
                if let Some(id) = self.win {
                    return window::screenshot(id).map(move |s| M::Shot(s, n));
                }
            }
            M::Shot(s, n) => {
                #[cfg(debug_assertions)]
                crate::snap::save(&s, n);
                #[cfg(not(debug_assertions))]
                let _ = (&s, n);
                let total =
                    env("NOCTURNE_SNAP_AFTER_MS").map_or(1, |s| s.split(',').count().max(1));
                if n + 1 >= total {
                    return match self.win {
                        Some(id) => window::close(id).chain(iced::exit()),
                        None => iced::exit(),
                    };
                }
            }
        }
        Task::none()
    }

    fn event(&mut self, e: StyledEvent) -> Task<M> {
        if self.stats && !matches!(e, StyledEvent::Frame(_)) {
            eprintln!("engine: event {e:?}");
        }
        match e {
            StyledEvent::Resize(s) => {
                if s != self.size {
                    self.size = s;
                    match &mut self.scene {
                        Some(sc) => {
                            let cache = &mut self.cache;
                            let took = sc.resize((s.width, s.height), &mut |t, st, m| {
                                cache.measure(t, st, m, &mut shape)
                            });
                            if self.stats {
                                eprintln!(
                                    "engine: resize to {}x{} in {} us",
                                    s.width,
                                    s.height,
                                    took.as_micros()
                                );
                            }
                        }
                        None => self.rebuild(),
                    }
                }
            }
            StyledEvent::Hover(h) => {
                if !self.pinned_hover && h != self.it.hover {
                    self.it.hover = h;
                    self.restyle();
                }
            }
            StyledEvent::Active(a) => {
                self.it.active = a;
                self.restyle();
            }
            StyledEvent::Click(n) => {
                let focusable = self
                    .scene
                    .as_ref()
                    .is_some_and(|s| s.doc.nodes[n].focusable);
                if focusable {
                    self.it.focus = Some(n);
                    self.it.focus_visible = false;
                    self.restyle();
                }
            }
            StyledEvent::Wheel { target, dx, dy } => {
                if let Some(s) = &mut self.scene {
                    let cache = &mut self.cache;
                    s.wheel(target, dx, dy, &mut |t, st, m| {
                        cache.measure(t, st, m, &mut shape)
                    });
                }
            }
            StyledEvent::Nav(nav) => self.navigate(nav),
            StyledEvent::SafeMode => {
                let on = self.engine.toggle_safe_mode();
                eprintln!("engine: safe mode {}", if on { "on" } else { "off" });
                self.rebuild();
            }
            StyledEvent::Frame(now) => {
                if let Some(s) = &mut self.scene {
                    s.tick(now);
                }
            }
        }
        Task::none()
    }

    fn navigate(&mut self, nav: Nav) {
        let Some(s) = &mut self.scene else { return };
        let from = self.it.focus;
        let next = match nav {
            Nav::Next => s.focus_next(from, true),
            Nav::Prev => s.focus_next(from, false),
            Nav::Dir(d) => from.map_or_else(|| s.focus_next(None, true), |f| s.spatial(f, d)),
            Nav::Activate => None,
        };
        if let Some(n) = next {
            self.it.focus = Some(n);
            self.it.focus_visible = true;
            let cache = &mut self.cache;
            s.reveal(n, &mut |t, st, m| cache.measure(t, st, m, &mut shape));
            self.restyle();
        }
    }

    fn subscription(&self) -> Subscription<M> {
        if self.css_path.is_some() {
            iced::time::every(Duration::from_secs(1)).map(|_| M::Poll)
        } else {
            Subscription::none()
        }
    }

    fn view(&self) -> Element<'_, M> {
        match &self.scene {
            Some(s) => Styled::new(s, M::Ev)
                .slot(
                    1,
                    text_input("Search", &self.typed)
                        .id(WidgetId::new("engine-search"))
                        .on_input(M::Typed)
                        .size(13)
                        .padding(0),
                )
                .slot(2, hole(self.hole, true, M::Hole))
                .into(),
            None => iced::widget::Space::new().into(),
        }
    }
}

pub fn run() -> iced::Result {
    let size = window_size();
    iced::application(boot, Preview::update, Preview::view)
        .title("NOCTURNE theme engine preview")
        .subscription(Preview::subscription)
        .theme(|_: &Preview| iced::Theme::Dark)
        .default_font(theme::base())
        .window(window::Settings {
            size,
            ..Default::default()
        })
        .run()
}
