//! Theme-engine spike (feature `theme-spike`, never in a normal build): `NOCTURNE_PREVIEW=theme-spike` shows a static Home screen
//! as an element tree, styled by a real CSS cascade and laid out by Taffy (`crates/style`), painted by one custom Iced widget.
//! Launch through `scripts/run-isolated.sh`. Environment:
//! - `NOCTURNE_SPIKE_THEME=streaming|videosite|rail|dock` (a bundled stylesheet) or `NOCTURNE_SPIKE_CSS=<file>` (re-read when it changes)
//! - `NOCTURNE_SPIKE_SCROLL_Y=<px>` initial page scroll, `NOCTURNE_SPIKE_HOVER=<class>:<n>` pretend the pointer is over the n-th such element
//! - `NOCTURNE_SPIKE_BIG=<n>` use a generated tree of about n elements, `NOCTURNE_SPIKE_STATS=1` print build and draw timings
//! - `NOCTURNE_SNAP=/tmp/x.png`, `NOCTURNE_SNAP_AFTER_MS=1500,3000` window captures (debug builds), `NOCTURNE_PREVIEW_SIZE=1920x1080`
use crate::theme;
use iced::advanced::graphics::text::Paragraph;
use iced::advanced::text::{Alignment, Paragraph as _, Renderer as _, Text};
use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, renderer};
use iced::font::Weight;
use iced::gradient::{self, Gradient};
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{
    Background, Border, Color, Degrees, Element, Event, Font, Length, Pixels, Point, Rectangle,
    Shadow, Size, Subscription, Task, Vector, alignment, window,
};
use nocturne_core::customize::{Color as Rgba, Fill};
use nocturne_style::props::Align;
use nocturne_style::taffy::style::{Display, Overflow};
use nocturne_style::{Doc, Id, Scene, Sheet, TextStyle, demo, flags};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone)]
enum M {
    Win(window::Id),
    Size(Size),
    Hover(Option<Id>),
    Wheel(Option<Id>, f32, f32),
    Poll,
    Frame(Instant),
    Snap(usize),
    Shot(window::Screenshot, usize),
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn font_for(weight: u16) -> Font {
    theme::font(match weight {
        ..=199 => Weight::Thin,
        200..=299 => Weight::ExtraLight,
        300..=399 => Weight::Light,
        400..=499 => Weight::Normal,
        500..=599 => Weight::Medium,
        600..=699 => Weight::Semibold,
        700..=799 => Weight::Bold,
        800..=899 => Weight::ExtraBold,
        _ => Weight::Black,
    })
}

/// The real text shaper (cosmic-text through Iced), cached: layout asks for the same string at several widths.
fn measure(
    cache: &mut HashMap<u64, (f32, f32)>,
    text: &str,
    st: &TextStyle,
    max: Option<f32>,
) -> (f32, f32) {
    let mut h = DefaultHasher::new();
    (
        text,
        st.size.to_bits(),
        st.weight,
        st.line_height.to_bits(),
        st.lines,
        max.map(f32::to_bits),
    )
        .hash(&mut h);
    let key = h.finish();
    if let Some(v) = cache.get(&key) {
        return *v;
    }
    let p = Paragraph::with_text(Text {
        content: text,
        bounds: Size::new(max.unwrap_or(f32::INFINITY), f32::INFINITY),
        size: Pixels(st.size),
        line_height: LineHeight::Relative(st.line_height),
        font: font_for(st.weight),
        align_x: Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::Word,
    });
    let b = p.min_bounds();
    let height = st.lines.map_or(b.height, |l| {
        b.height.min(l as f32 * st.size * st.line_height)
    });
    let v = (b.width, height);
    cache.insert(key, v);
    v
}

struct Spike {
    doc: Doc,
    sheet: Sheet,
    css_path: Option<PathBuf>,
    mtime: Option<SystemTime>,
    scene: Option<Scene>,
    size: Size,
    hover: Option<Id>,
    scroll: HashMap<Id, [f32; 2]>,
    page_y: f32,
    win: Option<window::Id>,
    cache: HashMap<u64, (f32, f32)>,
    stats: bool,
    dir: f32,
}

fn sheet() -> (Sheet, Option<PathBuf>) {
    if let Some(p) = env("NOCTURNE_SPIKE_CSS") {
        let css = std::fs::read_to_string(&p).unwrap_or_default();
        return (Sheet::parse(&css), Some(p.into()));
    }
    let name = env("NOCTURNE_SPIKE_THEME").unwrap_or_else(|| "streaming".into());
    let css = nocturne_style::THEMES
        .iter()
        .find(|t| t.0 == name)
        .map_or("", |t| t.1);
    (Sheet::parse(css), None)
}

fn mtime(p: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn boot() -> (Spike, Task<M>) {
    let (sheet, css_path) = sheet();
    for w in &sheet.warnings {
        eprintln!("spike: css: {w}");
    }
    let doc = match env("NOCTURNE_SPIKE_BIG").and_then(|n| n.parse().ok()) {
        Some(n) => demo::big(n),
        None => demo::home(),
    };
    let hover = env("NOCTURNE_SPIKE_HOVER").and_then(|h| {
        let (class, n) = h.split_once(':')?;
        (0..doc.len())
            .filter(|&i| doc.nodes[i].classes.iter().any(|c| c == class))
            .nth(n.parse().ok()?)
    });
    let mut tasks = vec![window::oldest().and_then(|id| Task::done(M::Win(id)))];
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
    (
        Spike {
            doc,
            mtime: css_path.as_ref().and_then(mtime),
            css_path,
            sheet,
            scene: None,
            size: Size::ZERO,
            hover,
            scroll: HashMap::new(),
            page_y: env("NOCTURNE_SPIKE_SCROLL_Y")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            win: None,
            cache: HashMap::new(),
            stats: env("NOCTURNE_SPIKE_STATS").is_some(),
            dir: 1.0,
        },
        Task::batch(tasks),
    )
}

impl Spike {
    /// Cascade and layout: runs when the stylesheet, the window size or the hover target changes, never per frame.
    fn rebuild(&mut self) {
        if self.size.width < 1.0 {
            return;
        }
        let f = flags(&self.doc, self.hover, None);
        let cache = &mut self.cache;
        let t = Instant::now();
        let s = Scene::build(
            &self.doc,
            &self.sheet,
            &f,
            (self.size.width, self.size.height),
            &mut |text, st, max| measure(cache, text, st, max),
        );
        if self.stats {
            eprintln!(
                "spike: {} nodes, {} rules: cascade {} us, layout (incl. text shaping) {} us, total {} us, {} unsupported declarations",
                self.doc.len(),
                self.sheet.rule_count(),
                s.timing.cascade.as_micros(),
                s.timing.layout.as_micros(),
                t.elapsed().as_micros(),
                s.report.unsupported
            );
            for m in &s.report.messages {
                eprintln!("spike:   {m}");
            }
        }
        self.scene = Some(s);
        self.clamp();
    }

    fn clamp(&mut self) {
        let Some(s) = &self.scene else { return };
        self.page_y = self
            .page_y
            .clamp(0.0, (s.rects[0][3] - self.size.height).max(0.0));
        for (n, o) in &mut self.scroll {
            let [_, _, w, h] = s.rects[*n];
            let [cw, ch] = s.content[*n];
            o[0] = o[0].clamp(0.0, (cw - w).max(0.0));
            o[1] = o[1].clamp(0.0, (ch - h).max(0.0));
        }
    }

    fn update(&mut self, m: M) -> Task<M> {
        match m {
            M::Win(id) => self.win = Some(id),
            M::Size(s) => {
                if s != self.size {
                    self.size = s;
                    self.rebuild();
                }
            }
            M::Hover(h) => {
                if env("NOCTURNE_SPIKE_HOVER").is_none() && h != self.hover {
                    self.hover = h;
                    self.rebuild();
                }
            }
            M::Wheel(n, dx, dy) => {
                // The innermost scroll container that can move on that axis takes it, else the page.
                let mut at = n;
                let mut done = false;
                while let (Some(i), Some(s)) = (at, &self.scene) {
                    let st = &s.styles[i].layout;
                    let (ox, oy) = (
                        st.overflow.x == Overflow::Scroll,
                        st.overflow.y == Overflow::Scroll,
                    );
                    if i != 0 && ((ox && dx != 0.0) || (oy && dy != 0.0)) {
                        let o = self.scroll.entry(i).or_default();
                        o[0] -= dx;
                        o[1] -= dy;
                        done = true;
                        break;
                    }
                    at = self.doc.nodes[i].parent;
                }
                if !done {
                    self.page_y -= dy;
                }
                self.clamp();
            }
            M::Frame(_) => {
                // Benchmark: scroll the page down and up by itself, one redraw per frame.
                let max = self
                    .scene
                    .as_ref()
                    .map_or(0.0, |s| s.rects[0][3] - self.size.height)
                    .max(1.0);
                self.dir = if self.page_y >= max {
                    -1.0
                } else if self.page_y <= 0.0 {
                    1.0
                } else {
                    self.dir
                };
                self.page_y = (self.page_y + 14.0 * self.dir).clamp(0.0, max);
            }
            M::Poll => {
                if let Some(p) = &self.css_path {
                    let t = mtime(p);
                    if t != self.mtime {
                        self.mtime = t;
                        self.sheet = Sheet::parse(&std::fs::read_to_string(p).unwrap_or_default());
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

    fn subscription(&self) -> Subscription<M> {
        let mut s = vec![];
        if self.css_path.is_some() {
            s.push(iced::time::every(Duration::from_secs(1)).map(|_| M::Poll));
        }
        if env("NOCTURNE_SPIKE_ANIM").is_some() {
            s.push(window::frames().map(M::Frame));
        }
        Subscription::batch(s)
    }

    fn view(&self) -> Element<'_, M> {
        Element::new(Page { app: self })
    }
}

/// The widget: paints a [`Scene`] and turns pointer input into messages. Interactive leaves (text inputs, the video hole) would be
/// real Iced widgets placed at their Taffy rectangles; this spike only draws boxes and text.
struct Page<'a> {
    app: &'a Spike,
}

#[derive(Default)]
struct State {
    hover: Option<Id>,
    size: Size,
    draws: u32,
    draw_time: Duration,
}

fn col(c: Rgba, opacity: f32) -> Color {
    let [r, g, b, a] = c.to_f32();
    Color {
        r,
        g,
        b,
        a: a * opacity,
    }
}

fn background(f: &Fill, opacity: f32) -> Background {
    match f {
        Fill::Solid(c) => Background::Color(col(*c, opacity)),
        Fill::Linear { angle, stops } => {
            let mut g = gradient::Linear::new(Degrees(*angle));
            for (c, at) in stops {
                g = g.add_stop(*at, col(*c, opacity));
            }
            Background::Gradient(Gradient::Linear(g))
        }
    }
}

impl Page<'_> {
    fn scene(&self) -> Option<&Scene> {
        self.app.scene.as_ref()
    }

    /// The node under `p` (widget coordinates), searching paint order backwards.
    fn hit(&self, s: &Scene, n: Id, p: Point, sh: (f32, f32), clip: Rectangle) -> Option<Id> {
        let c = &s.styles[n];
        if c.layout.display == Display::None {
            return None;
        }
        let mut sh = sh;
        if c.fixed {
            sh.1 += self.app.page_y;
        }
        let [x, y, w, h] = s.rects[n];
        let rect = Rectangle::new(Point::new(x + sh.0, y + sh.1), Size::new(w, h));
        let clips =
            c.layout.overflow.x != Overflow::Visible || c.layout.overflow.y != Overflow::Visible;
        let inner = if clips {
            clip.intersection(&rect)?
        } else {
            clip
        };
        let off = self.app.scroll.get(&n).copied().unwrap_or_default();
        let child_sh = (sh.0 - off[0], sh.1 - off[1]);
        for &k in s.paint[n].iter().rev() {
            if let Some(f) = self.hit(s, k, p, child_sh, inner) {
                return Some(f);
            }
        }
        (clip.contains(p) && rect.contains(p)).then_some(n)
    }

    fn root_clip(&self, b: Rectangle) -> Rectangle {
        Rectangle::new(Point::ORIGIN, b.size())
    }

    #[allow(clippy::too_many_arguments)]
    fn paint(
        &self,
        r: &mut iced::Renderer,
        s: &Scene,
        n: Id,
        o: Point,
        sh: (f32, f32),
        clip: Rectangle,
    ) {
        let c = &s.styles[n];
        if c.layout.display == Display::None {
            return;
        }
        let mut sh = sh;
        if c.fixed {
            sh.1 += self.app.page_y;
        }
        let [x, y, w, h] = s.rects[n];
        let rect = Rectangle::new(Point::new(o.x + x + sh.0, o.y + y + sh.1), Size::new(w, h));
        // Virtualisation by culling: a subtree whose box and scrollable extent miss the clip is not visited at all.
        let [cw, ch] = s.content[n];
        let reach = Rectangle {
            width: w.max(cw),
            height: h.max(ch),
            ..rect
        };
        if clip.intersection(&reach).is_none() {
            return;
        }
        let op = c.opacity;
        if let Some(bg) = &c.bg {
            r.fill_quad(
                renderer::Quad {
                    bounds: rect,
                    border: Border {
                        radius: c.radius.into(),
                        ..Border::default()
                    },
                    shadow: c.shadow.map_or(Shadow::default(), |sd| Shadow {
                        color: col(sd.color, op),
                        offset: Vector::new(sd.dx, sd.dy),
                        blur_radius: sd.blur,
                    }),
                    snap: false,
                },
                background(bg, op),
            );
        }
        let [bl, bt, br, bb] = s.borders[n];
        let line = col(c.border_color, op);
        if bl > 0.0 && bl == bt && bt == br && br == bb {
            r.fill_quad(
                renderer::Quad {
                    bounds: rect,
                    border: Border {
                        color: line,
                        width: bl,
                        radius: c.radius.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                Background::Color(Color::TRANSPARENT),
            );
        } else {
            for (bw, q) in [
                (bt, Rectangle { height: bt, ..rect }),
                (
                    bb,
                    Rectangle {
                        y: rect.y + h - bb,
                        height: bb,
                        ..rect
                    },
                ),
                (bl, Rectangle { width: bl, ..rect }),
                (
                    br,
                    Rectangle {
                        x: rect.x + w - br,
                        width: br,
                        ..rect
                    },
                ),
            ] {
                if bw > 0.0 {
                    r.fill_quad(
                        renderer::Quad {
                            bounds: q,
                            ..renderer::Quad::default()
                        },
                        line,
                    );
                }
            }
        }
        if let Some(t) = &self.app.doc.nodes[n].text {
            let [il, it, ir, ib] = s.insets[n];
            let area = Rectangle::new(
                Point::new(rect.x + il, rect.y + it),
                Size::new((w - il - ir).max(0.0), (h - it - ib).max(0.0)),
            );
            let (anchor, align_x) = match c.align {
                Align::Left => (area.x, Alignment::Left),
                Align::Center => (area.x + area.width / 2.0, Alignment::Center),
                Align::Right => (area.x + area.width, Alignment::Right),
            };
            if let Some(vis) = clip.intersection(&area) {
                r.fill_text(
                    Text {
                        content: nocturne_style::layout::shown(t, c).into_owned(),
                        bounds: area.size(),
                        size: Pixels(c.font_size),
                        line_height: LineHeight::Relative(c.line_height),
                        font: font_for(c.weight),
                        align_x,
                        align_y: alignment::Vertical::Top,
                        shaping: Shaping::Advanced,
                        wrapping: Wrapping::Word,
                    },
                    Point::new(anchor, area.y),
                    col(c.color, op),
                    vis,
                );
            }
        }
        let kids = &s.paint[n];
        if kids.is_empty() {
            return;
        }
        let ov = c.layout.overflow;
        let clips = ov.x != Overflow::Visible || ov.y != Overflow::Visible;
        let off = self.app.scroll.get(&n).copied().unwrap_or_default();
        let child_sh = (sh.0 - off[0], sh.1 - off[1]);
        let go = |r: &mut iced::Renderer, clip: Rectangle| {
            for &k in kids {
                self.paint(r, s, k, o, child_sh, clip);
            }
        };
        match clips.then(|| clip.intersection(&rect)).flatten() {
            Some(inner) => r.with_layer(inner, |r| go(r, inner)),
            None if clips => {}
            None => go(r, clip),
        }
    }
}

impl<'a> Widget<M, iced::Theme, iced::Renderer> for Page<'a> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &iced::Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _: &Rectangle,
    ) {
        let st = tree.state.downcast_mut::<State>();
        let b = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                let hit = cursor.position_in(b).and_then(|p| {
                    let s = self.scene()?;
                    self.hit(s, 0, p, (0.0, -self.app.page_y), self.root_clip(b))
                });
                if hit != st.hover {
                    st.hover = hit;
                    shell.publish(M::Hover(hit));
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let (dx, dy) = match *delta {
                    mouse::ScrollDelta::Lines { x, y } => (x * 48.0, y * 48.0),
                    mouse::ScrollDelta::Pixels { x, y } => (x, y),
                };
                let target = st.hover;
                shell.publish(M::Wheel(target, dx, dy));
                shell.capture_event();
            }
            Event::Window(window::Event::RedrawRequested(_)) if b.size() != st.size => {
                st.size = b.size();
                shell.publish(M::Size(b.size()));
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        r: &mut iced::Renderer,
        _: &iced::Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let Some(s) = self.scene() else { return };
        let t = Instant::now();
        let b = layout.bounds();
        r.with_layer(b, |r| {
            self.paint(
                r,
                s,
                0,
                b.position(),
                (0.0, -self.app.page_y),
                self.root_clip(b) + Vector::new(b.x, b.y),
            );
        });
        if self.app.stats {
            // Single-threaded draw: interior state through the tree is read-only here, so keep a process-wide tally.
            let _ = tree;
            DRAWS.with(|d| {
                let mut d = d.borrow_mut();
                d.0 += 1;
                d.1 += t.elapsed();
                if d.0 % 120 == 0 {
                    eprintln!(
                        "spike: draw {} us average over {} frames",
                        d.1.as_micros() / d.0 as u128,
                        d.0
                    );
                }
            });
        }
    }
}

thread_local! {
    static DRAWS: std::cell::RefCell<(u32, Duration)> = const { std::cell::RefCell::new((0, Duration::ZERO)) };
}

impl<'a> From<Page<'a>> for Element<'a, M> {
    fn from(p: Page<'a>) -> Self {
        Element::new(p)
    }
}

pub fn run() -> iced::Result {
    let size = env("NOCTURNE_PREVIEW_SIZE")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some(Size::new(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(Size::new(1920.0, 1080.0));
    iced::application(boot, Spike::update, Spike::view)
        .title("NOCTURNE theme engine spike")
        .subscription(Spike::subscription)
        .theme(|_: &Spike| iced::Theme::Dark)
        .default_font(theme::base())
        .window(window::Settings {
            size,
            ..Default::default()
        })
        .run()
}
