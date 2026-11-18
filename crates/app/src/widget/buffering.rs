//! Player loading / buffering overlay (wired in by the player page, step 4d).
//!
//! Full variant (opening a stream): the item's backdrop, cover-fit and darkened by gradients and a
//! vignette, with a very slow zoom; the title's clear logo (or, without one, the title as a
//! wide-tracked wordmark) centred and revealed left to right by the buffer fill, the unrevealed
//! part ghosted, a block of thin slanted accent hairlines at the wipe front; below, a low flare
//! line and a mono status label. While the fill is stalled the logo breathes gently so it never
//! looks frozen.
//!
//! Light variant (a stall during playback): no backdrop, the video only dimmed, a small logo or
//! title, the line and the label.
//!
//! Reduce motion: no wipe, breathing, zoom or flare; the logo shows in full and a plain thin
//! progress line carries the fill. The player page cross-fades the overlay out with [`fade_out`].
use crate::motion;
use crate::theme as t;
use iced::advanced::text as adv;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text, gradient::Linear};
use iced::widget::image::Handle;
use iced::widget::text::Alignment;
use iced::widget::{Stack, canvas as canvas_widget, column, container, image, space};
use iced::{Color, ContentFit, Element, Length, Pixels, Point, Rectangle, Size, alignment};
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// Opacity of the part of the title the wipe has not reached.
const GHOST: f32 = 0.18;
/// Slow backdrop zoom: 1.00 to 1.03 over this long.
const ZOOM_OVER: Duration = Duration::from_secs(40);

/// The wipe position: eases toward the target fill, never backwards. Pure (time passed in).
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    from: f32,
    to: f32,
    since: Instant,
    /// When the target last moved (for the stalled-breathing).
    moved: Instant,
}

impl Progress {
    pub fn new(now: Instant) -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            since: now,
            moved: now,
        }
    }

    /// New buffer fill (0..=1). A lower value is ignored: the wipe never goes back.
    pub fn set(&mut self, target: f32, now: Instant) {
        let t = target.clamp(0.0, 1.0);
        if t > self.to {
            self.from = self.value(now, false);
            self.to = t;
            self.since = now;
            self.moved = now;
        }
    }

    /// Where the wipe is. `reduced`: no easing, the fill itself.
    pub fn value(&self, now: Instant, reduced: bool) -> f32 {
        if reduced {
            return self.to;
        }
        let k = now.saturating_duration_since(self.since).as_secs_f32() / t::BASE.as_secs_f32();
        self.from + (self.to - self.from) * motion::ease_out(k)
    }

    /// Has the fill not moved for a while (waiting on the server)?
    pub fn stalled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.moved) > Duration::from_millis(900)
    }
}

/// "Buffering 42 %" for a fill 0..=1.
pub fn percent_label(frac: f32) -> String {
    format!(
        "Buffering {} %",
        (frac.clamp(0.0, 1.0) * 100.0).round() as u32
    )
}

/// Opacity factor of the logo: a slow breath (0.78..1.0, 3.2 s) while stalled, steady otherwise.
pub fn breath(now_secs: f32, stalled: bool, reduced: bool) -> f32 {
    if reduced || !stalled {
        return 1.0;
    }
    let v = (now_secs / 3.2 * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    0.78 + 0.22 * v
}

/// Overlay opacity while leaving (playback started): 1 -> 0 over SLOW, instantly when reduced.
pub fn fade_out(since: Instant, now: Instant, reduced: bool) -> f32 {
    if reduced {
        return 0.0;
    }
    1.0 - motion::ease_out(
        now.saturating_duration_since(since).as_secs_f32() / t::SLOW.as_secs_f32(),
    )
}

/// Backdrop zoom factor, 1.00 -> 1.03 very slowly; fixed when reduced.
pub fn zoom(elapsed: Duration, reduced: bool) -> f32 {
    if reduced {
        return 1.0;
    }
    1.0 + 0.03 * (elapsed.as_secs_f32() / ZOOM_OVER.as_secs_f32()).min(1.0)
}

/// Everything the overlay shows.
pub struct Splash<'a> {
    pub title: &'a str,
    pub backdrop: Option<&'a Handle>,
    /// Clear logo of the title; the wordmark is drawn from `title` without one.
    pub logo: Option<&'a Handle>,
    pub status: String,
    /// Eased wipe position (from [`Progress::value`]).
    pub reveal: f32,
    /// [`breath`] factor.
    pub breath: f32,
    /// [`zoom`] factor.
    pub zoom: f32,
    /// Overall opacity (1, or [`fade_out`]).
    pub fade: f32,
    pub reduced: bool,
    /// Mid-playback stall: lighter, no backdrop takeover.
    pub light: bool,
    pub win: Size,
}

/// Where the logo sits (also where the wipe's edge runs).
fn logo_box(win: Size, light: bool) -> Rectangle {
    let w = (win.width * if light { 0.22 } else { 0.40 }).clamp(200.0, 520.0);
    let h = w * 0.34;
    let cy = win.height * if light { 0.46 } else { 0.55 };
    Rectangle::new(
        Point::new(win.width / 2.0 - w / 2.0, cy - h / 2.0),
        Size::new(w, h),
    )
}

struct Overlay {
    title: String,
    status: String,
    reveal: f32,
    /// Overall opacity.
    alpha: f32,
    /// Extra opacity of the wordmark letters (breathing).
    breath: f32,
    reduced: bool,
    light: bool,
    plain: bool,
    has_logo: bool,
}

/// Glyph advances of the title at 100 px, measured once per title.
#[derive(Default)]
struct Metrics(RefCell<Option<(String, Vec<f32>)>>);

fn scale(c: Color, k: f32) -> Color {
    Color {
        a: (c.a * k).clamp(0.0, 1.0),
        ..c
    }
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    let k = k.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * k,
        g: a.g + (b.g - a.g) * k,
        b: a.b + (b.b - a.b) * k,
        a: a.a + (b.a - a.a) * k,
    }
}

fn glyph_width(c: char, size: f32) -> f32 {
    let p = adv::paragraph::Plain::<<iced::Renderer as adv::Renderer>::Paragraph>::new(adv::Text {
        content: c.to_string(),
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: adv::LineHeight::default(),
        font: t::bold(),
        align_x: Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: adv::Shaping::Advanced,
        wrapping: adv::Wrapping::None,
    });
    p.min_width()
}

/// Left edges of the letters and the total width, for glyph advances `w` at `size` with 0.3 em
/// tracking, centred on `cx`.
fn layout(w: &[f32], size: f32, cx: f32) -> (Vec<f32>, f32, f32) {
    let track = size * 0.30;
    let total = w.iter().sum::<f32>() + track * (w.len() as f32 - 1.0).max(0.0);
    let left = cx - total / 2.0;
    let mut x = left;
    let mut xs = vec![];
    for a in w {
        xs.push(x);
        x += a + track;
    }
    (xs, left, total)
}

/// Wordmark size: as large as the window height suggests, shrunk so the title fits the width.
fn fit_size(w100: &[f32], win: Size, light: bool) -> f32 {
    let want = (win.height * 0.085).clamp(36.0, 92.0) * if light { 0.5 } else { 1.0 };
    let track100 = 30.0 * (w100.len() as f32 - 1.0).max(0.0);
    let total100 = w100.iter().sum::<f32>() + track100;
    let max = win.width * if light { 0.5 } else { 0.84 };
    (want.min(max / total100.max(1.0) * 100.0)).max(16.0)
}

fn text(content: &str, at: Point, size: f32, color: Color, font: iced::Font, a: Alignment) -> Text {
    Text {
        content: content.to_string(),
        position: at,
        color,
        size: Pixels(size),
        font,
        align_x: a,
        align_y: alignment::Vertical::Center,
        shaping: adv::Shaping::Advanced,
        ..Text::default()
    }
}

impl<M> canvas::Program<M> for Overlay {
    type State = Metrics;

    fn draw(
        &self,
        state: &Metrics,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = bounds.size();
        let (w, h) = (size.width, size.height);
        let cx = w * 0.5;
        let a = self.alpha;
        let mut f = Frame::new(renderer, size);
        let vgrad = |stops: &[(f32, Color)]| {
            stops.iter().fold(
                Linear::new(Point::ORIGIN, Point::new(0.0, h)),
                |g, (o, c)| g.add_stop(*o, *c),
            )
        };

        if self.light {
            // The video stays visible, just calmer.
            f.fill_rectangle(Point::ORIGIN, size, scale(t::frame(), 0.38 * a));
        } else {
            if self.plain {
                f.fill_rectangle(
                    Point::ORIGIN,
                    size,
                    vgrad(&[
                        (0.0, scale(t::frame(), a)),
                        (0.7, scale(mix(t::frame(), t::surface(), 0.8), a)),
                        (1.0, scale(mix(t::surface(), t::accent(), 0.06), a)),
                    ]),
                );
            }
            // Dark scrim, then a vignette from the sides.
            f.fill_rectangle(
                Point::ORIGIN,
                size,
                vgrad(&[
                    (0.0, scale(t::frame(), 0.50 * a)),
                    (0.5, scale(t::frame(), 0.58 * a)),
                    (1.0, scale(t::frame(), 0.96 * a)),
                ]),
            );
            let edge = w * 0.30;
            for (x0, x1) in [(0.0, edge), (w, w - edge)] {
                f.fill_rectangle(
                    Point::new(x0.min(x1), 0.0),
                    Size::new(edge, h),
                    Linear::new(Point::new(x0, 0.0), Point::new(x1, 0.0))
                        .add_stop(0.0, scale(t::frame(), 0.75 * a))
                        .add_stop(1.0, scale(t::frame(), 0.0)),
                );
            }
        }

        // The title: an image layer when there is a logo, else letters drawn here.
        let lbox = logo_box(size, self.light);
        let (top, bot, left, total);
        if self.has_logo {
            (top, bot, left, total) = (lbox.y, lbox.y + lbox.height, lbox.x, lbox.width);
        } else {
            let mut m = state.0.borrow_mut();
            if m.as_ref().is_none_or(|(t0, _)| *t0 != self.title) {
                let w100 = self
                    .title
                    .to_uppercase()
                    .chars()
                    .map(|c| {
                        if c == ' ' {
                            40.0
                        } else {
                            glyph_width(c, 100.0)
                        }
                    })
                    .collect();
                *m = Some((self.title.clone(), w100));
            }
            let w100 = &m.as_ref().expect("just set").1;
            let sz = fit_size(w100, size, self.light);
            let widths: Vec<f32> = w100.iter().map(|x| x * sz / 100.0).collect();
            let (xs, l, tot) = layout(&widths, sz, cx);
            let wm_y = lbox.y + lbox.height / 2.0;
            let wx = l + self.reveal.clamp(0.0, 1.0) * tot;
            for ((c, x), lw) in self.title.to_uppercase().chars().zip(&xs).zip(&widths) {
                let lit = if self.reduced {
                    1.0
                } else {
                    ((wx - x) / (lw * 0.9).max(1.0)).clamp(0.0, 1.0)
                };
                let hot = if self.reduced {
                    0.0
                } else {
                    (1.0 - (wx - x) / (sz * 0.9)).clamp(0.0, 1.0) * lit
                };
                let k = (GHOST + (1.0 - GHOST) * lit) * a * self.breath;
                f.fill_text(text(
                    &c.to_string(),
                    Point::new(*x, wm_y),
                    sz,
                    scale(mix(t::text(), t::accent_text(), hot * 0.8), k),
                    t::bold(),
                    Alignment::Left,
                ));
            }
            (top, bot, left, total) = (wm_y - sz * 0.62, wm_y + sz * 0.62, l, tot);
        }
        let wx = left + self.reveal.clamp(0.0, 1.0) * total;
        let flare_y = lbox.y + lbox.height + h * 0.09;

        if self.reduced {
            let len = 240.0f32.min(w * 0.6);
            let x0 = cx - len / 2.0;
            f.fill_rectangle(
                Point::new(x0, flare_y),
                Size::new(len, 2.0),
                scale(t::hairline_strong(), a),
            );
            f.fill_rectangle(
                Point::new(x0, flare_y),
                Size::new(len * self.reveal.clamp(0.0, 1.0), 2.0),
                scale(t::accent(), a),
            );
        } else {
            // Leading edge: a narrow, subtle block of thin slanted accent hairlines.
            let edge = (self.reveal / 0.03)
                .min((1.0 - self.reveal) / 0.03)
                .clamp(0.0, 1.0);
            if edge > 0.0 {
                let slant = (bot - top) * 0.26;
                for i in 0..9 {
                    let x = wx - i as f32 * 3.6;
                    let k = (1.0 - i as f32 / 9.0).powf(1.2);
                    f.stroke(
                        &Path::line(
                            Point::new(x + slant / 2.0, top),
                            Point::new(x - slant / 2.0, bot),
                        ),
                        Stroke::default()
                            .with_width(1.0)
                            .with_color(scale(t::accent(), 0.7 * k * edge * a)),
                    );
                }
            }
            // Flare: bright core fading out to both sides, brighter as the fill grows.
            let fl = (0.55 + 0.45 * self.reveal) * a;
            let half = w * if self.light { 0.22 } else { 0.40 };
            let line = |y: f32, hgt: f32, core: Color, k: f32| {
                let g = Linear::new(Point::new(cx - half, 0.0), Point::new(cx + half, 0.0))
                    .add_stop(0.0, scale(t::accent(), 0.0))
                    .add_stop(0.2, scale(t::accent(), 0.35 * k))
                    .add_stop(0.5, scale(core, k))
                    .add_stop(0.8, scale(t::accent(), 0.35 * k))
                    .add_stop(1.0, scale(t::accent(), 0.0));
                (Point::new(cx - half, y), Size::new(half * 2.0, hgt), g)
            };
            for i in (1..=7).rev() {
                let (p, s, g) = line(
                    flare_y + 1.0 - i as f32 * 1.6,
                    i as f32 * 3.2,
                    t::accent(),
                    0.05 * fl,
                );
                f.fill_rectangle(p, s, g);
            }
            let (p, s, g) = line(flare_y, 2.0, t::text(), 0.95 * fl);
            f.fill_rectangle(p, s, g);
            let (p, s, g) = line(flare_y + 0.5, 1.0, t::text(), fl);
            f.fill_rectangle(p, s, g);
        }
        if !self.status.is_empty() {
            f.fill_text(text(
                &self.status,
                Point::new(cx, flare_y + 28.0),
                t::MICRO,
                scale(t::text_secondary(), a),
                t::MONO,
                Alignment::Center,
            ));
        }
        vec![f.into_geometry()]
    }
}

/// Draws its child in full size but only the left `frac` of it (the wipe over the logo). The child
/// keeps its layout, so the revealed part lines up exactly with the ghost copy underneath.
struct Wipe<'a, M> {
    content: Element<'a, M>,
    frac: f32,
}

impl<'a, M: 'a> iced::advanced::Widget<M, iced::Theme, iced::Renderer> for Wipe<'a, M> {
    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let b = layout.bounds();
        let clip = Rectangle {
            width: b.width * self.frac.clamp(0.0, 1.0),
            ..b
        };
        renderer.with_layer(clip, |r| {
            self.content.as_widget().draw(
                &tree.children[0],
                r,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        });
    }
}

/// The overlay as one element covering its parent.
pub fn view<'a, M: 'a>(s: Splash<'a>) -> Element<'a, M> {
    let a = s.fade.clamp(0.0, 1.0);
    let mut layers: Vec<Element<M>> = vec![];
    if !s.light {
        layers.push(
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(crate::style::fill(Color { a, ..t::frame() }))
                .into(),
        );
        if let Some(h) = s.backdrop {
            layers.push(
                image(h.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Cover)
                    .scale(s.zoom)
                    .opacity(a)
                    .into(),
            );
        }
    }
    layers.push(
        canvas_widget(Overlay {
            title: s.title.to_string(),
            status: s.status,
            reveal: s.reveal,
            alpha: a,
            breath: s.breath,
            reduced: s.reduced,
            light: s.light,
            plain: s.backdrop.is_none() && !s.light,
            has_logo: s.logo.is_some(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
    );
    if let Some(logo) = s.logo {
        // A ghost copy underneath and the bright copy clipped to the wipe on top. Both fill the
        // same box, so the wipe reveals the artwork in place.
        let b = logo_box(s.win, s.light);
        let op = a * s.breath;
        let art = |o: f32| {
            image(logo.clone())
                .width(b.width)
                .height(b.height)
                .content_fit(ContentFit::Contain)
                .opacity(o)
        };
        let stack: Element<M> = if s.reduced {
            art(op).into()
        } else {
            let lit = Element::new(Wipe {
                content: art(op).into(),
                frac: s.reveal,
            });
            Stack::with_children(vec![art(GHOST * op).into(), lit])
                .width(b.width)
                .height(b.height)
                .into()
        };
        layers.push(
            column![space().height(b.y), container(stack).center_x(Length::Fill)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        );
    }
    Stack::with_children(layers).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(t0: Instant, n: u64) -> Instant {
        t0 + Duration::from_millis(n)
    }

    #[test]
    fn wipe_eases_forward_and_never_goes_back() {
        let t0 = Instant::now();
        let mut p = Progress::new(t0);
        p.set(0.4, t0);
        let (a, b, c) = (
            p.value(t0, false),
            p.value(ms(t0, 80), false),
            p.value(ms(t0, 400), false),
        );
        assert!(a == 0.0 && b > a && b < c && (c - 0.4).abs() < 1e-4);
        p.set(0.1, ms(t0, 500)); // the buffer drained: the wipe stays
        assert!((p.value(ms(t0, 900), false) - 0.4).abs() < 1e-4);
        p.set(0.7, ms(t0, 1000));
        assert!(
            p.value(ms(t0, 1000), false) >= 0.4 - 1e-4,
            "starts from where it was"
        );
    }

    #[test]
    fn stalled_after_a_moment_without_progress() {
        let t0 = Instant::now();
        let mut p = Progress::new(t0);
        p.set(0.2, t0);
        assert!(!p.stalled(ms(t0, 300)) && p.stalled(ms(t0, 1000)));
        p.set(0.3, ms(t0, 1000));
        assert!(!p.stalled(ms(t0, 1200)));
    }

    #[test]
    fn reduced_motion_is_static() {
        let t0 = Instant::now();
        let mut p = Progress::new(t0);
        p.set(0.6, t0);
        assert_eq!(p.value(t0, true), 0.6);
        assert_eq!(breath(1.7, true, true), 1.0);
        assert_eq!(zoom(Duration::from_secs(20), true), 1.0);
        assert_eq!(fade_out(t0, t0, true), 0.0);
    }

    #[test]
    fn breath_only_while_stalled_and_stays_in_range() {
        assert_eq!(breath(1.0, false, false), 1.0);
        for i in 0..64 {
            let b = breath(i as f32 * 0.1, true, false);
            assert!((0.779..=1.001).contains(&b), "{b}");
        }
        assert!(breath(0.0, true, false) != breath(0.8, true, false));
    }

    #[test]
    fn zoom_and_fade_run_their_course() {
        assert_eq!(zoom(Duration::ZERO, false), 1.0);
        assert!((zoom(Duration::from_secs(400), false) - 1.03).abs() < 1e-6);
        let t0 = Instant::now();
        assert_eq!(fade_out(t0, t0, false), 1.0);
        assert!(fade_out(t0, ms(t0, 100), false) < 1.0);
        assert_eq!(fade_out(t0, ms(t0, 400), false), 0.0);
    }

    #[test]
    fn labels_fit_and_layout() {
        assert_eq!(percent_label(0.424), "Buffering 42 %");
        assert_eq!(percent_label(2.0), "Buffering 100 %");
        let win = Size::new(1280.0, 800.0);
        let short = fit_size(&[70.0; 5], win, false);
        let long = fit_size(&[70.0; 60], win, false);
        assert!(
            short >= 60.0 && long < short,
            "long titles shrink: {short} {long}"
        );
        assert!(
            fit_size(&[70.0; 5], win, true) < short,
            "light variant is smaller"
        );
        let (xs, left, total) = layout(&[50.0, 50.0], 100.0, 500.0);
        assert_eq!((xs[0], left, total), (left, 500.0 - 65.0, 130.0));
        let b = logo_box(win, false);
        assert!((b.x + b.width / 2.0 - 640.0).abs() < 1e-3);
    }
}
