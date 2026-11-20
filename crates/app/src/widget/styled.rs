//! The paint widget of the theme engine (`nocturne-style`): it draws a laid-out [`Scene`] (backgrounds with layered gradients,
//! per-side borders, per-corner radii, outer and inset shadows, opacity, text with shadow, tracking and ellipsis, images with
//! `object-fit`, transforms, clipping, scroll offsets, culling) and hosts **slots**: real Iced elements (a `text_input`, the
//! video hole, a virtualised list) placed at the Taffy rectangle of the element that carries the slot and drawn in tree order,
//! so z-order and clipping follow the CSS. Interaction goes out as [`StyledEvent`]s; the app owns the state (hover, focus,
//! scroll) in the scene and rebuilds the view, so the widget itself keeps almost nothing.
use crate::theme;
use iced::advanced::image::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{Alignment, Renderer as _, Text};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget, mouse, overlay};
use iced::border;
use iced::font::{Family, Style as FontStyle, Weight};
use iced::gradient::{self, Gradient};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::image::Handle;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{
    Background, Border, Color, Degrees, Element, Event, Font, Length, Pixels, Point, Rectangle,
    Shadow, Size, Transformation, Vector, alignment, window,
};
use nocturne_core::customize::value::Color as Rgba;
use nocturne_style::paint::{self, Fill};
use nocturne_style::taffy::style::Display;
use nocturne_style::{Dir, FontChoice, Id, Scene};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// What the widget reports; the app maps it into its own message type.
#[derive(Clone, Debug, PartialEq)]
pub enum StyledEvent {
    /// The element under the pointer changed (drives `:hover`).
    Hover(Option<Id>),
    /// The pressed element (drives `:active`), `None` on release.
    Active(Option<Id>),
    /// Pressed and released on the same element.
    Click(Id),
    /// A scroll gesture over `target` (give it to `Scene::wheel`).
    Wheel {
        target: Option<Id>,
        dx: f32,
        dy: f32,
    },
    /// The widget got a new size (give it to `Scene::resize`).
    Resize(Size),
    /// Keyboard navigation the slots did not use (Tab, arrows, Enter or Space).
    Nav(Nav),
    /// The safe-mode chord (`nocturne_style::SAFE_CHORD`) was pressed; handled before any slot sees the key.
    SafeMode,
    /// A frame while the scene animates (give the time to `Scene::tick`).
    Frame(Instant),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Next,
    Prev,
    Dir(Dir),
    Activate,
}

/// An outline the Theme Studio draws over an element (pick mode): `selected` is the chosen one, the other the element under the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub node: Id,
    pub label: Option<String>,
    pub selected: bool,
}

/// What the Theme Studio asks of a scene hosted outside `app/engine_ui.rs` (the detail page, the player's bar): the outlines to draw, whether picking is on (hosted widgets are then inert) and the width the docked panel takes from the window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub marks: Vec<Mark>,
    pub pick: bool,
    pub dock: f32,
}

pub struct Styled<'a, M> {
    scene: SceneRef<'a>,
    slots: Vec<Element<'a, M>>,
    /// Scene node of each slot, aligned with `slots`.
    slot_nodes: Vec<Option<Id>>,
    by_node: HashMap<Id, usize>,
    images: Option<&'a HashMap<u32, Handle>>,
    on_event: Box<dyn Fn(StyledEvent) -> M + 'a>,
    /// Tab, arrows, Enter, Space and the safe-look chord are this widget's (default); off, the keyboard belongs to the page around it.
    keys: bool,
    marks: Vec<Mark>,
    /// Hosted slot widgets get no events (the Studio's pick mode selects them instead).
    inert: bool,
}

#[derive(Default)]
struct State {
    hover: Option<Id>,
    pressed: Option<Id>,
    size: Size,
    /// A scrollbar thumb being dragged: the container, its axis (vertical) and the last cursor coordinate along it.
    drag: Option<(Id, bool, f32)>,
    /// When the pointer settled on the element it is over (a `title` tooltip shows after a short rest).
    rest: Option<Instant>,
}

/// How long the pointer rests on an element with a `title` before its tooltip shows (the hand-written tooltips' delay).
const TIP_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

/// A scrollbar the app asked for with `data-scrollbar` on a scroll container: `vertical` is always shown while the content overflows (like the hand-written grids' bar), `hover-x` is the quiet 4 px bar of a horizontal strip, shown while the pointer is over it. A theme cannot hide either (the app's decision, like focusability).
struct Bar {
    node: Id,
    vertical: bool,
    /// The thumb in widget coordinates.
    thumb: Rectangle,
    /// Content pixels per thumb pixel.
    ratio: f32,
}

/// The scene a widget draws: borrowed from the app's state, or shared when the page builds it while it builds its view.
enum SceneRef<'a> {
    Borrowed(&'a Scene),
    Shared(std::rc::Rc<Scene>),
}

impl std::ops::Deref for SceneRef<'_> {
    type Target = Scene;
    fn deref(&self) -> &Scene {
        match self {
            SceneRef::Borrowed(s) => s,
            SceneRef::Shared(s) => s,
        }
    }
}

impl<'a, M: 'a> Styled<'a, M> {
    pub fn new(scene: &'a Scene, on_event: impl Fn(StyledEvent) -> M + 'a) -> Self {
        Self::of(SceneRef::Borrowed(scene), on_event)
    }

    /// Like [`Styled::new`] for a scene built during `view` (the widget keeps it alive).
    pub fn shared(scene: std::rc::Rc<Scene>, on_event: impl Fn(StyledEvent) -> M + 'a) -> Self {
        Self::of(SceneRef::Shared(scene), on_event)
    }

    fn of(scene: SceneRef<'a>, on_event: impl Fn(StyledEvent) -> M + 'a) -> Self {
        Self {
            scene,
            slots: vec![],
            slot_nodes: vec![],
            by_node: HashMap::new(),
            images: None,
            on_event: Box::new(on_event),
            keys: true,
            marks: vec![],
            inert: false,
        }
    }

    /// Host `element` at the rectangle of the node that `Doc::slot` marked with `slot`.
    pub fn slot(mut self, slot: u32, element: impl Into<Element<'a, M>>) -> Self {
        let node = (0..self.scene.doc.len()).find(|&i| self.scene.doc.nodes[i].slot == Some(slot));
        if let Some(n) = node {
            self.by_node.insert(n, self.slots.len());
        }
        self.slots.push(element.into());
        self.slot_nodes.push(node);
        self
    }

    /// Whether the widget takes the keyboard (Tab, arrows, Enter, Space, the safe-look chord). The player's controls leave it to the page.
    pub fn keys(mut self, on: bool) -> Self {
        self.keys = on;
        self
    }

    /// Slot widgets stop taking events, so a click on one selects its element (the Theme Studio's pick mode).
    pub fn inert(mut self, on: bool) -> Self {
        self.inert = on;
        self
    }

    /// Marks and inertness from a [`Probe`].
    pub fn probe(self, p: Option<&Probe>) -> Self {
        match p {
            Some(p) => self.marks(p.marks.clone()).inert(p.pick),
            None => self,
        }
    }

    /// Outlines drawn over the scene (the Theme Studio's pick mode).
    pub fn marks(mut self, marks: Vec<Mark>) -> Self {
        self.marks = marks;
        self
    }

    /// Pixels for the `Doc::image` nodes, by image id.
    pub fn images(mut self, images: &'a HashMap<u32, Handle>) -> Self {
        self.images = Some(images);
        self
    }
}

// ---- fonts ----

fn weight_of(w: u16) -> Weight {
    match w {
        ..=199 => Weight::Thin,
        200..=299 => Weight::ExtraLight,
        300..=399 => Weight::Light,
        400..=499 => Weight::Normal,
        500..=599 => Weight::Medium,
        600..=699 => Weight::Semibold,
        700..=799 => Weight::Bold,
        800..=899 => Weight::ExtraBold,
        _ => Weight::Black,
    }
}

/// Iced wants a `'static` family name: names are interned (a bounded few; more fall back to the interface font).
fn intern(name: &str) -> Option<&'static str> {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut v = NAMES.lock().ok()?;
    if let Some(n) = v.iter().find(|n| **n == name) {
        return Some(*n);
    }
    if v.len() >= 64 {
        return None;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    v.push(leaked);
    Some(leaked)
}

/// The Iced font for a resolved text style (public so the app's measure function shapes with the same face).
pub fn font_for(weight: u16, italic: bool, choice: &FontChoice) -> Font {
    let w = weight_of(weight);
    let mut f = match choice {
        FontChoice::Default => theme::font(w),
        FontChoice::Serif => Font {
            family: Family::Serif,
            weight: w,
            ..Font::DEFAULT
        },
        FontChoice::Monospace => Font {
            weight: w,
            ..theme::MONO
        },
        FontChoice::Named(n) => match intern(n) {
            Some(n) => Font {
                family: Family::Name(n),
                weight: w,
                ..Font::DEFAULT
            },
            None => theme::font(w),
        },
    };
    if italic {
        f.style = FontStyle::Italic;
    }
    f
}

// ---- colours and shapes ----

fn col(c: Rgba, opacity: f32) -> Color {
    let [r, g, b, a] = c.to_f32();
    Color {
        r,
        g,
        b,
        a: a * opacity,
    }
}

fn gradient_bg(f: &Fill, w: f32, h: f32, opacity: f32) -> Option<Background> {
    match f {
        Fill::Solid(c) => Some(Background::Color(col(*c, opacity))),
        Fill::Linear { .. } => {
            let r = paint::resolve(f, w, h)?;
            let mut g = gradient::Linear::new(Degrees(r.angle));
            for (c, at) in r.stops {
                g = g.add_stop(at, col(c, opacity));
            }
            Some(Background::Gradient(Gradient::Linear(g)))
        }
    }
}

fn radius(r: [f32; 4]) -> border::Radius {
    border::Radius {
        top_left: r[0],
        top_right: r[1],
        bottom_right: r[2],
        bottom_left: r[3],
    }
}

fn inter(a: Rectangle, b: Rectangle) -> Option<Rectangle> {
    a.intersection(&b)
}

fn tr(r: [f32; 4], o: Point) -> Rectangle {
    Rectangle::new(Point::new(o.x + r[0], o.y + r[1]), Size::new(r[2], r[3]))
}

/// Iced draws a layer's quads first, then its images, then its text, and a layer created later is above an earlier one. A page where a quad must sit above an image (a scrim over a backdrop, a badge over a poster) is therefore painted in passes, one layer each: pass `2k` holds the quads and text that come after `k` images in paint order and before the next one, pass `2k + 1` holds image `k`. `ord[n]` is the number of images before node `n` in paint order, `end[n]` the number after its subtree.
struct Pass<'a> {
    k: usize,
    ord: &'a [u32],
    end: &'a [u32],
    /// `layered[n]`: drawing `n`'s subtree opens an Iced layer (a clipping container with children, or a transform). Layers are stacked in the order they were opened, not in tree order, so a later sibling that overlaps such a subtree has to open a layer of its own to stay above it.
    layered: &'a [bool],
}

/// Which subtrees open a layer while they are drawn (children come after their parents in node order, so one pass from the end is enough).
fn layered_subtrees<M>(w: &Styled<'_, M>) -> Vec<bool> {
    let s = &*w.scene;
    let mut has = vec![false; s.doc.len()];
    for n in (0..s.doc.len()).rev() {
        let c = s.style(n);
        let own = !s.paint[n].is_empty()
            && (c.layout.overflow.x != nocturne_style::taffy::style::Overflow::Visible
                || c.layout.overflow.y != nocturne_style::taffy::style::Overflow::Visible)
            || !s.eff(n).xf.unwrap_or(c.xf).is_identity();
        has[n] |= own;
        if has[n]
            && let Some(p) = s.doc.nodes[n].parent
        {
            has[p] = true;
        }
    }
    has
}

/// The loaded background picture of node `n`.
fn bg_pic(s: &Scene, n: Id) -> Option<std::sync::Arc<super::pics::Pic>> {
    super::pics::get(s.style(n).pic_url.as_deref()?)
}

fn image_order<M>(w: &Styled<'_, M>) -> (Vec<u32>, Vec<u32>, usize) {
    let s: &Scene = &w.scene;
    let n = s.doc.len();
    let mut ord = vec![0u32; n];
    let mut end = vec![0u32; n];
    let mut count = 0u32;
    // Depth-first over the paint order (an explicit stack: trees are deep but bounded).
    let mut stack: Vec<(Id, bool)> = vec![(0, false)];
    while let Some((i, leaving)) = stack.pop() {
        if leaving {
            end[i] = count;
            continue;
        }
        ord[i] = count;
        if s.style(i).layout.display == Display::None || s.doc.nodes[i].virtual_proto {
            end[i] = count;
            continue;
        }
        let node = &s.doc.nodes[i];
        let has_img = matches!((node.image, w.images), (Some(img), Some(images)) if images.contains_key(&img.id));
        if has_img || (node.image.is_none() && bg_pic(s, i).is_some()) {
            count += 1;
        }
        stack.push((i, true));
        for &k in s.paint[i].iter().rev() {
            stack.push((k, false));
        }
    }
    (ord, end, count as usize)
}

impl<M> Styled<'_, M> {
    #[allow(clippy::too_many_arguments)]
    fn paint(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        r: &mut iced::Renderer,
        th: &iced::Theme,
        style: &renderer::Style,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        n: Id,
        o: Point,
        sh: (f32, f32),
        clip: Rectangle,
        ps: &Pass,
    ) {
        let s: &Scene = &self.scene;
        let c = s.style(n);
        if c.layout.display == Display::None || s.doc.nodes[n].virtual_proto {
            return;
        }
        // This node's subtree has nothing in this pass.
        let (lo, hi) = (2 * ps.ord[n] as usize, 2 * ps.end[n] as usize);
        if ps.k < lo || ps.k > hi {
            return;
        }
        let draws_image = s.doc.nodes[n]
            .image
            .is_some_and(|i| self.images.is_some_and(|m| m.contains_key(&i.id)));
        let draws_pic = !draws_image && bg_pic(s, n).is_some();
        let here = ps.k == lo;
        let eff = s.eff(n);
        let opacity = eff.opacity.unwrap_or(c.opacity);
        // Fixed elements ignore the page and container scroll.
        let sh = if c.fixed { (0.0, 0.0) } else { sh };
        let rect = tr(s.rects[n], Point::new(o.x + sh.0, o.y + sh.1));
        // Culling: a subtree whose box and scrollable extent miss the clip is not visited at all.
        let [cw, ch] = s.content[n];
        let reach = Rectangle {
            width: rect.width.max(cw),
            height: rect.height.max(ch),
            ..rect
        };
        let xf = eff.xf.unwrap_or(c.xf);
        let moved = !xf.is_identity();
        // Only a box that clips can hide its whole subtree: an overflowing child of a zero-height wrapper (`main`, with an absolutely positioned page inside) still shows.
        let culls = c.layout.overflow.x != nocturne_style::taffy::style::Overflow::Visible
            || c.layout.overflow.y != nocturne_style::taffy::style::Overflow::Visible;
        if !moved && culls && inter(clip, reach).is_none() {
            return;
        }
        let ox = rect.x + rect.width * xf.origin.0;
        let oy = rect.y + rect.height * xf.origin.1;
        let body = |r: &mut iced::Renderer| {
            if c.visible {
                if here {
                    self.paint_box(r, n, rect, opacity, &eff, clip);
                    if let Some(&k) = self.by_node.get(&n) {
                        self.paint_slot(tree, layout, r, th, style, cursor, viewport, k, n);
                    }
                }
                // An image is its own pass; the text of a node is drawn with the quads before it.
                if draws_image && ps.k == lo + 1 {
                    self.paint_content(r, n, rect, opacity, &eff, clip, true);
                } else if draws_pic && ps.k == lo + 1 {
                    self.paint_pic(r, n, rect, opacity, clip);
                } else if here && !draws_pic || draws_pic && ps.k == lo + 2 {
                    self.paint_content(r, n, rect, opacity, &eff, clip, false);
                }
            }
            let kids = &s.paint[n];
            if !kids.is_empty() {
                let ov = c.layout.overflow;
                let clips = ov.x != nocturne_style::taffy::style::Overflow::Visible
                    || ov.y != nocturne_style::taffy::style::Overflow::Visible;
                let off = s.scroll.offsets.get(&n).copied().unwrap_or_default();
                let child_sh = (sh.0 - off[0].round(), sh.1 - off[1].round());
                let go = |r: &mut iced::Renderer, clip: Rectangle| {
                    // Boxes of the earlier siblings that opened layers: a sibling overlapping one of them is drawn in a layer of its own, so that it lands above (a focus ring over the chips of a poster).
                    let mut opened: Vec<Rectangle> = vec![];
                    for &k in kids {
                        let kr = tr(s.rects[k], Point::new(o.x + child_sh.0, o.y + child_sh.1));
                        // Boxes that only touch (fractional layout) do not overlap.
                        let above = opened.iter().any(|l| {
                            l.intersection(&kr)
                                .is_some_and(|i| i.width > 0.5 && i.height > 0.5)
                        });
                        if above {
                            r.with_layer(clip, |r| {
                                self.paint(
                                    tree, layout, r, th, style, cursor, viewport, k, o, child_sh,
                                    clip, ps,
                                )
                            });
                        } else {
                            self.paint(
                                tree, layout, r, th, style, cursor, viewport, k, o, child_sh, clip,
                                ps,
                            );
                        }
                        if above || ps.layered[k] {
                            opened.push(kr);
                        }
                    }
                };
                // A container that clips gets its own layer; one whose clip is empty draws nothing.
                match clips.then(|| inter(clip, rect)) {
                    Some(Some(inner)) => r.with_layer(inner, |r| go(r, inner)),
                    Some(None) => {}
                    None => go(r, clip),
                }
            }
            if c.visible && ps.k == hi {
                self.paint_after(r, n, rect, opacity, &eff);
            }
        };
        if moved {
            let (tx, ty, k) = xf.resolve(rect.width, rect.height);
            let t = Transformation::translate(ox + tx, oy + ty)
                * Transformation::scale(k)
                * Transformation::translate(-ox, -oy);
            r.with_layer(clip, |r| r.with_transformation(t, body));
        } else {
            body(r);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_slot(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        r: &mut iced::Renderer,
        th: &iced::Theme,
        style: &renderer::Style,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        k: usize,
        n: Id,
    ) {
        let Some(child) = layout.children().nth(k) else {
            return;
        };
        let Some(vis) = self.scene.clip_of(n) else {
            return;
        };
        let vis = Rectangle::new(
            Point::new(layout.bounds().x + vis[0], layout.bounds().y + vis[1]),
            Size::new(vis[2], vis[3]),
        );
        if let Some(vis) = inter(vis, *viewport) {
            r.with_layer(vis, |r| {
                self.slots[k]
                    .as_widget()
                    .draw(&tree.children[k], r, th, style, child, cursor, &vis)
            });
        }
    }

    /// Shadows, background layers, inset shadows, borders: everything behind the content.
    fn paint_box(
        &self,
        r: &mut iced::Renderer,
        n: Id,
        rect: Rectangle,
        opacity: f32,
        eff: &nocturne_style::Eff,
        clip: Rectangle,
    ) {
        let s: &Scene = &self.scene;
        let c = s.style(n);
        let rad = c.radius;
        let [bl, bt, br, bb] = s.borders[n];
        // Layout is fractional (text keeps its exact size), but a box that is not being transformed is snapped to physical pixels as the hand-written pages' quads are: a 2 px underline or a hairline at y = 207.4 would otherwise straddle two rows and fade.
        let crisp = eff.xf.unwrap_or(c.xf).is_identity();
        // Outer shadows first (an invisible quad that only casts).
        for sd in c.shadows.iter().filter(|d| !d.inset) {
            let b = Rectangle {
                x: rect.x - sd.spread,
                y: rect.y - sd.spread,
                width: (rect.width + 2.0 * sd.spread).max(0.0),
                height: (rect.height + 2.0 * sd.spread).max(0.0),
            };
            r.fill_quad(
                Quad {
                    bounds: b,
                    border: Border {
                        radius: radius(rad.map(|v| (v + sd.spread).max(0.0))),
                        ..Border::default()
                    },
                    shadow: Shadow {
                        color: col(sd.color, opacity),
                        offset: Vector::new(sd.dx, sd.dy),
                        blur_radius: sd.blur,
                    },
                    snap: false,
                },
                Background::Color(Color::TRANSPARENT),
            );
        }
        if let (Some(sd), Some(_)) = (c.drop_shadow, self.scene.doc.nodes[n].image) {
            // Approximation: a soft shadow of the box (an image's own alpha is not known here).
            r.fill_quad(
                Quad {
                    bounds: rect,
                    border: Border {
                        radius: radius(rad),
                        ..Border::default()
                    },
                    shadow: Shadow {
                        color: col(sd.color, opacity),
                        offset: Vector::new(sd.dx, sd.dy),
                        blur_radius: sd.blur,
                    },
                    snap: false,
                },
                Background::Color(Color::TRANSPARENT),
            );
        }
        let layers = eff.bg.as_ref().unwrap_or(&c.bg);
        // Layers written above a loaded picture are drawn after it (`paint_pic`).
        let above = if bg_pic(s, n).is_some() {
            c.pic_above.min(layers.len())
        } else {
            0
        };
        for f in layers[above..].iter().rev() {
            if let Some(bg) = gradient_bg(f, rect.width, rect.height, opacity) {
                r.fill_quad(
                    Quad {
                        bounds: rect,
                        border: Border {
                            radius: radius(rad),
                            ..Border::default()
                        },
                        shadow: Shadow::default(),
                        snap: crisp,
                    },
                    bg,
                );
            }
        }
        // Inset shadows as strips along the edges they fall from (no blur: a hairline highlight stays a hairline).
        for sd in c.shadows.iter().filter(|d| d.inset) {
            let colr = col(sd.color, opacity);
            let mut inset = |b: Rectangle| {
                r.fill_quad(
                    Quad {
                        bounds: b,
                        ..Quad::default()
                    },
                    colr,
                );
            };
            let (rt, rb) = (
                rad[0].max(rad[1]).min(rect.width / 2.0),
                rad[2].max(rad[3]).min(rect.width / 2.0),
            );
            if sd.dy > 0.0 {
                inset(Rectangle {
                    x: rect.x + rt * 0.5,
                    y: rect.y + bt,
                    width: (rect.width - rt).max(0.0),
                    height: sd.dy,
                });
            } else if sd.dy < 0.0 {
                inset(Rectangle {
                    x: rect.x + rb * 0.5,
                    y: rect.y + rect.height - bb + sd.dy,
                    width: (rect.width - rb).max(0.0),
                    height: -sd.dy,
                });
            }
            if sd.dx > 0.0 {
                inset(Rectangle {
                    x: rect.x + bl,
                    y: rect.y + rad[0] * 0.5,
                    width: sd.dx,
                    height: (rect.height - rad[0]).max(0.0),
                });
            } else if sd.dx < 0.0 {
                inset(Rectangle {
                    x: rect.x + rect.width - br + sd.dx,
                    y: rect.y + rad[1] * 0.5,
                    width: -sd.dx,
                    height: (rect.height - rad[1]).max(0.0),
                });
            }
        }
        let bc = eff.border.unwrap_or(c.border_color);
        let uniform_w = bl > 0.0 && bl == bt && bt == br && br == bb;
        if uniform_w && bc.iter().all(|x| *x == bc[0]) {
            r.fill_quad(
                Quad {
                    bounds: rect,
                    border: Border {
                        color: col(bc[0], opacity),
                        width: bl,
                        radius: radius(rad),
                    },
                    shadow: Shadow::default(),
                    snap: crisp,
                },
                Background::Color(Color::TRANSPARENT),
            );
        } else {
            for (bw, color, q) in [
                (bt, bc[0], Rectangle { height: bt, ..rect }),
                (
                    bb,
                    bc[2],
                    Rectangle {
                        y: rect.y + rect.height - bb,
                        height: bb,
                        ..rect
                    },
                ),
                (bl, bc[3], Rectangle { width: bl, ..rect }),
                (
                    br,
                    bc[1],
                    Rectangle {
                        x: rect.x + rect.width - br,
                        width: br,
                        ..rect
                    },
                ),
            ] {
                if bw > 0.0 {
                    r.fill_quad(
                        Quad {
                            bounds: q,
                            ..Quad::default()
                        },
                        col(color, opacity),
                    );
                }
            }
        }
        let _ = clip;
    }

    /// The `url()` picture of the node's background: placed by `background-size`, `-position` and `-repeat`, clipped to the box.
    fn paint_pic(
        &self,
        r: &mut iced::Renderer,
        n: Id,
        rect: Rectangle,
        opacity: f32,
        clip: Rectangle,
    ) {
        use iced::advanced::svg::Renderer as _;
        let s: &Scene = &self.scene;
        let c = s.style(n);
        let (Some(p), Some(pic), Some(vis)) = (c.pic(), bg_pic(s, n), inter(clip, rect)) else {
            return;
        };
        let (iw, ih) = match &pic.data {
            super::pics::Data::Raster(_) => (pic.w as f32, pic.h as f32),
            super::pics::Data::Vector(h) => {
                let z = r.measure_svg(h);
                (z.width as f32, z.height as f32)
            }
        };
        let layers = s.eff(n).bg.clone();
        let layers = layers.as_ref().unwrap_or(&c.bg);
        for [x, y, w, h] in paint::pic_rects(&p, rect.width, rect.height, iw, ih) {
            let at = Rectangle::new(Point::new(rect.x + x, rect.y + y), Size::new(w, h));
            match &pic.data {
                super::pics::Data::Raster(hd) => r.draw_image(
                    iced::advanced::image::Image {
                        border_radius: radius(c.radius),
                        ..iced::advanced::image::Image::new(hd.clone()).opacity(opacity)
                    },
                    at,
                    vis,
                ),
                super::pics::Data::Vector(hd) => r.draw_svg(
                    iced::advanced::svg::Svg::new(hd.clone()).opacity(opacity),
                    at,
                    vis,
                ),
            }
        }
        for f in layers[..c.pic_above.min(layers.len())].iter().rev() {
            if let Some(bg) = gradient_bg(f, rect.width, rect.height, opacity) {
                r.fill_quad(
                    Quad {
                        bounds: rect,
                        border: Border {
                            radius: radius(c.radius),
                            ..Border::default()
                        },
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    bg,
                );
            }
        }
    }

    /// Image and text.
    #[allow(clippy::too_many_arguments)]
    fn paint_content(
        &self,
        r: &mut iced::Renderer,
        n: Id,
        rect: Rectangle,
        opacity: f32,
        eff: &nocturne_style::Eff,
        clip: Rectangle,
        image_pass: bool,
    ) {
        let s: &Scene = &self.scene;
        let c = s.style(n);
        let node = &s.doc.nodes[n];
        if image_pass {
            let (Some(img), Some(images)) = (node.image, self.images) else {
                return;
            };
            let Some(handle) = images.get(&img.id) else {
                return;
            };
            let [x, y, w, h] = paint::fit_rect(
                c.fit,
                c.object_pos,
                rect.width,
                rect.height,
                img.w as f32,
                img.h as f32,
            );
            let fitted = Rectangle::new(Point::new(rect.x + x, rect.y + y), Size::new(w, h));
            let image = iced::advanced::image::Image {
                border_radius: radius(c.radius),
                // Pictures sit on whole physical pixels like the hand-written pages' (a fractional layout must not blur them).
                snap: eff.xf.unwrap_or(c.xf).is_identity(),
                ..iced::advanced::image::Image::new(handle.clone()).opacity(opacity)
            };
            if let Some(vis) = inter(clip, rect) {
                r.draw_image(image, fitted, vis);
            }
            return;
        }
        let Some(t) = &node.text else { return };
        let [il, it, ir, ib] = s.insets[n];
        let area = Rectangle::new(
            Point::new(rect.x + il, rect.y + it),
            Size::new(
                (rect.width - il - ir).max(0.0),
                (rect.height - it - ib).max(0.0),
            ),
        );
        let vis = clip;
        let ts = s.text_style(n);
        let font = font_for(ts.weight, ts.italic, &ts.font);
        let color = col(eff.color.unwrap_or(c.color), opacity);
        let align = match c.align {
            nocturne_style::props::Align::Left => Alignment::Left,
            nocturne_style::props::Align::Center => Alignment::Center,
            nocturne_style::props::Align::Right => Alignment::Right,
        };
        let line_h = ts.line_px();
        let draw_at = |r: &mut iced::Renderer,
                       content: String,
                       at: Point,
                       w: f32,
                       color: Color,
                       align: Alignment| {
            let shaping = crate::widget::styled_preview::shaping_of(&content, &ts.font);
            r.fill_text(
                Text {
                    content,
                    bounds: Size::new(w, line_h),
                    size: Pixels(c.font_size),
                    line_height: LineHeight::Relative(ts.line_height),
                    font,
                    align_x: align,
                    align_y: alignment::Vertical::Top,
                    shaping,
                    wrapping: Wrapping::None,
                },
                at,
                color,
                vis,
            );
        };
        // The shadow first: a few offset copies stand in for the blur (Iced has no text blur).
        let shadow_copies =
            |r: &mut iced::Renderer, f: &mut dyn FnMut(&mut iced::Renderer, Vector, Color)| {
                if let Some(sd) = c.text_shadow {
                    let base = col(sd.color, opacity);
                    if sd.blur <= 0.5 {
                        f(r, Vector::new(sd.dx, sd.dy), base);
                    } else {
                        let a = Color {
                            a: base.a / 5.0,
                            ..base
                        };
                        let k = sd.blur / 3.0;
                        for (ox, oy) in [(0.0, 0.0), (k, 0.0), (-k, 0.0), (0.0, k), (0.0, -k)] {
                            f(r, Vector::new(sd.dx + ox, sd.dy + oy), a);
                        }
                    }
                }
            };
        if let Some(b) = s.block(n) {
            for (k, line) in b.lines.iter().enumerate() {
                let y = area.y + k as f32 * line_h;
                let x0 = match c.align {
                    nocturne_style::props::Align::Left => area.x,
                    nocturne_style::props::Align::Center => {
                        area.x + (area.width - line.width) / 2.0
                    }
                    nocturne_style::props::Align::Right => area.x + area.width - line.width,
                };
                let mut draw_line = |r: &mut iced::Renderer, d: Vector, color: Color| {
                    if line.x.is_empty() {
                        draw_at(
                            r,
                            line.text.clone(),
                            Point::new(x0 + d.x, y + d.y),
                            line.width + 2.0,
                            color,
                            Alignment::Left,
                        );
                    } else {
                        for (ch, dx) in line.text.chars().zip(&line.x) {
                            if ch != ' ' {
                                draw_at(
                                    r,
                                    ch.to_string(),
                                    Point::new(x0 + dx + d.x, y + d.y),
                                    c.font_size * 2.0,
                                    color,
                                    Alignment::Left,
                                );
                            }
                        }
                    }
                };
                shadow_copies(r, &mut draw_line);
                draw_line(r, Vector::new(0.0, 0.0), color);
                if c.underline {
                    underline(r, Point::new(x0, y), line.width, c.font_size, line_h, color);
                }
            }
        } else {
            let shown = nocturne_style::layout::shown(t, c).into_owned();
            let (anchor, al) = match align {
                Alignment::Center => (area.x + area.width / 2.0, Alignment::Center),
                Alignment::Right => (area.x + area.width, Alignment::Right),
                _ => (area.x, Alignment::Left),
            };
            // A box one line high never wraps: the shaper's width and the renderer's can differ by a hair, which would break the last letter onto a second line.
            let one_line = area.height < line_h * 1.5;
            let draw_wrapped = |r: &mut iced::Renderer, d: Vector, color: Color| {
                r.fill_text(
                    Text {
                        content: shown.clone(),
                        // The shaper's widest line and the renderer's can differ by a hair: the box is a little wider than the measured text, or the renderer breaks one more line than the box has room for and draws nothing (a long overview in a clamped hero).
                        bounds: Size::new(area.width + 2.0, area.height),
                        size: Pixels(c.font_size),
                        line_height: LineHeight::Relative(ts.line_height),
                        font,
                        align_x: al,
                        align_y: alignment::Vertical::Top,
                        shaping: crate::widget::styled_preview::shaping_of(&shown, &ts.font),
                        wrapping: if one_line {
                            Wrapping::None
                        } else {
                            Wrapping::Word
                        },
                    },
                    Point::new(anchor + d.x, area.y + d.y),
                    color,
                    vis,
                );
            };
            let mut shadow =
                |r: &mut iced::Renderer, d: Vector, color: Color| draw_wrapped(r, d, color);
            shadow_copies(r, &mut shadow);
            draw_wrapped(r, Vector::new(0.0, 0.0), color);
            if c.underline {
                underline(
                    r,
                    Point::new(area.x, area.y),
                    area.width,
                    c.font_size,
                    line_h,
                    color,
                );
            }
        }
    }

    /// Mask scrim and the focus ring: on top of the element and its children.
    fn paint_after(
        &self,
        r: &mut iced::Renderer,
        n: Id,
        rect: Rectangle,
        opacity: f32,
        _eff: &nocturne_style::Eff,
    ) {
        let s: &Scene = &self.scene;
        let c = s.style(n);
        if let Some(Fill::Linear { angle, stops }) = &c.mask {
            // `mask-image` fades the element to the surface behind it: draw that surface colour with the inverse alpha.
            let surface = s.scrim_color(n);
            let inv: Vec<paint::Stop> = stops
                .iter()
                .map(|st| paint::Stop {
                    color: Rgba::rgba(surface.r, surface.g, surface.b, 255 - st.color.a),
                    pos: st.pos,
                })
                .collect();
            let f = Fill::Linear {
                angle: *angle,
                stops: inv,
            };
            if let Some(bg) = gradient_bg(&f, rect.width, rect.height, opacity) {
                r.fill_quad(
                    Quad {
                        bounds: rect,
                        ..Quad::default()
                    },
                    bg,
                );
            }
        }
        // The focus ring can never be removed by a theme; it may only be recoloured (`outline-color`, `--nocturne-focus`).
        let it = s.interaction;
        if it.focus == Some(n) && it.focus_visible {
            let ring = c.outline_color.map_or(theme::ACCENT, |c| col(c, 1.0));
            let w = 2.0;
            r.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: rect.x - w,
                        y: rect.y - w,
                        width: rect.width + 2.0 * w,
                        height: rect.height + 2.0 * w,
                    },
                    border: Border {
                        color: ring,
                        width: w,
                        radius: radius(c.radius.map(|v| v + w)),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                Background::Color(Color::TRANSPARENT),
            );
        }
    }
}

fn underline(r: &mut iced::Renderer, at: Point, w: f32, size: f32, line_h: f32, color: Color) {
    let y = at.y + (line_h - size) / 2.0 + size * 0.92;
    r.fill_quad(
        Quad {
            bounds: Rectangle::new(Point::new(at.x, y), Size::new(w, (size / 14.0).max(1.0))),
            ..Quad::default()
        },
        color,
    );
}

/// `NOCTURNE_ENGINE_STATS=1`: print what a frame costs (the CPU side of recording the draw calls).
fn stats_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("NOCTURNE_ENGINE_STATS").is_some())
}

impl<M> Styled<'_, M> {
    /// The scrollbars of the containers that asked for one, with the thumb geometry (also used to start a drag).
    fn bars(&self, hover: Option<Id>, dragging: Option<Id>) -> Vec<Bar> {
        let s = &*self.scene;
        let mut out = vec![];
        for n in 1..s.doc.len() {
            let mode = s.doc.nodes[n]
                .attrs
                .iter()
                .find(|(k, _)| k == "data-scrollbar")
                .map(|(_, v)| v.as_str());
            let Some(mode) = mode else { continue };
            if s.is_hidden(n) {
                continue;
            }
            let [_, _, w, h] = s.rects[n];
            let [cw, ch] = s.content[n];
            let off = s.scroll.offsets.get(&n).copied().unwrap_or_default();
            let r = s.screen_rect(n);
            if mode == "vertical" && ch > h + 0.5 {
                let len = (h * h / ch).clamp(24.0, h);
                let top = off[1].clamp(0.0, ch - h) / (ch - h) * (h - len);
                out.push(Bar {
                    node: n,
                    vertical: true,
                    thumb: Rectangle::new(
                        Point::new(r[0] + r[2] - 10.0, r[1] + top),
                        Size::new(10.0, len),
                    ),
                    ratio: (ch - h) / (h - len).max(1.0),
                });
            } else if mode == "hover-x" && cw > w + 0.5 {
                let mut up = hover;
                let mut over = dragging == Some(n);
                while let Some(i) = up {
                    over |= i == n;
                    up = s.doc.nodes[i].parent;
                }
                if !over {
                    continue;
                }
                let len = (w * w / cw).clamp(24.0, w);
                let left = off[0].clamp(0.0, cw - w) / (cw - w) * (w - len);
                out.push(Bar {
                    node: n,
                    vertical: false,
                    thumb: Rectangle::new(
                        Point::new(r[0] + left, r[1] + r[3] - 4.0),
                        Size::new(len, 4.0),
                    ),
                    ratio: (cw - w) / (w - len).max(1.0),
                });
            }
        }
        out
    }

    /// The page's own scrollbar thumb (widget coordinates; node 0), for dragging it: the same geometry `paint_scrollbar` draws.
    fn page_bar(&self) -> Option<Bar> {
        let s = &*self.scene;
        let (page, view) = (s.page_height(), s.viewport.1);
        if page <= view + 0.5 {
            return None;
        }
        let len = (view * view / page).clamp(24.0, view);
        let max = page - view;
        let top = s.scroll.page_y.clamp(0.0, max) / max * (view - len);
        Some(Bar {
            node: 0,
            vertical: true,
            thumb: Rectangle::new(Point::new(s.viewport.0 - 10.0, top), Size::new(10.0, len)),
            ratio: max / (view - len).max(1.0),
        })
    }

    /// The `title` of the element under the pointer (or of its nearest ancestor that has one) and the element it sits on.
    pub(super) fn tip_of(&self, hover: Option<Id>) -> Option<(Id, &str)> {
        let mut at = hover;
        while let Some(i) = at {
            if let Some((_, v)) = self.scene.doc.nodes[i]
                .attrs
                .iter()
                .find(|(k, _)| k == "title")
            {
                return Some((i, v.as_str()));
            }
            at = self.scene.doc.nodes[i].parent;
        }
        None
    }

    /// The Theme Studio's outlines: a box around the element and a label with its tag and classes.
    fn paint_marks(&self, r: &mut iced::Renderer, b: Rectangle, vis: Rectangle) {
        for mk in &self.marks {
            if mk.node >= self.scene.doc.len() {
                continue;
            }
            let sr = self.scene.screen_rect(mk.node);
            let rect = Rectangle::new(
                Point::new(b.x + sr[0], b.y + sr[1]),
                Size::new(sr[2].max(2.0), sr[3].max(2.0)),
            );
            let ink = if mk.selected {
                theme::accent()
            } else {
                theme::text()
            };
            r.with_layer(vis, |r| {
                r.fill_quad(
                    Quad {
                        bounds: rect,
                        border: Border {
                            color: ink,
                            width: if mk.selected { 2.0 } else { 1.0 },
                            radius: border::radius(0.0),
                        },
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    Color {
                        a: if mk.selected { 0.10 } else { 0.07 },
                        ..ink
                    },
                );
                let Some(label) = &mk.label else {
                    return;
                };
                let ts = nocturne_style::TextStyle {
                    size: theme::MICRO,
                    weight: 400,
                    italic: false,
                    font: FontChoice::Monospace,
                    line_height: 1.3,
                    lines: None,
                    letter_spacing: 0.0,
                    ellipsis: false,
                };
                let (tw, th) = crate::widget::styled_preview::shape(label, &ts, None);
                let (w, h) = (tw + 8.0, th + 4.0);
                // Above the box when there is room, inside its top edge otherwise.
                let y = if rect.y - h >= vis.y {
                    rect.y - h
                } else {
                    rect.y
                };
                let x = rect.x.clamp(vis.x, (vis.x + vis.width - w).max(vis.x));
                r.fill_quad(
                    Quad {
                        bounds: Rectangle::new(Point::new(x, y), Size::new(w, h)),
                        border: Border::default(),
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    ink,
                );
                r.fill_text(
                    Text {
                        content: label.clone(),
                        bounds: Size::new(tw + 1.0, th),
                        size: Pixels(ts.size),
                        line_height: LineHeight::Relative(ts.line_height),
                        font: font_for(400, false, &FontChoice::Monospace),
                        align_x: Alignment::Left,
                        align_y: alignment::Vertical::Top,
                        shaping: Shaping::Advanced,
                        wrapping: Wrapping::None,
                    },
                    Point::new(x + 4.0, y + 2.0),
                    if mk.selected {
                        theme::on_accent()
                    } else {
                        theme::frame()
                    },
                    vis,
                );
            });
        }
    }

    /// A small tooltip under the element (the look of the hand-written `tip`): drawn over everything in the window, so it can leave the scene's own box (a header is only as tall as its bar).
    fn paint_tip(&self, tree: &Tree, r: &mut iced::Renderer, b: Rectangle, window: Rectangle) {
        let st = tree.state.downcast_ref::<State>();
        if st.rest.is_none_or(|t| t.elapsed() < TIP_DELAY) || st.drag.is_some() {
            return;
        }
        let Some((n, text)) = self.tip_of(st.hover) else {
            return;
        };
        let ts = nocturne_style::TextStyle {
            size: theme::MICRO,
            weight: 400,
            italic: false,
            font: FontChoice::Monospace,
            line_height: 1.3,
            lines: None,
            letter_spacing: 0.0,
            ellipsis: false,
        };
        let (tw, th) = crate::widget::styled_preview::shape(text, &ts, None);
        let (padx, pady) = (theme::S2, theme::S1);
        let (w, h) = (tw + 2.0 * padx + 2.0, th + 2.0 * pady + 2.0);
        let rect = self.scene.screen_rect(n);
        let x = (b.x + rect[0] + (rect[2] - w) / 2.0)
            .clamp(window.x, (window.x + window.width - w).max(window.x));
        let y = b.y + rect[1] + rect[3] + 4.0;
        let body = Rectangle::new(Point::new(x, y), Size::new(w, h));
        r.with_layer(window, |r| {
            r.fill_quad(
                Quad {
                    bounds: body,
                    border: Border {
                        color: theme::hairline_strong(),
                        width: 1.0,
                        radius: border::radius(theme::r_xs()),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                theme::raised(),
            );
            r.fill_text(
                Text {
                    content: text.to_string(),
                    bounds: Size::new(tw + 1.0, th),
                    size: Pixels(ts.size),
                    line_height: LineHeight::Relative(ts.line_height),
                    font: font_for(400, false, &FontChoice::Monospace),
                    align_x: Alignment::Left,
                    align_y: alignment::Vertical::Top,
                    shaping: Shaping::Advanced,
                    wrapping: Wrapping::None,
                },
                Point::new(x + 1.0 + padx, y + 1.0 + pady),
                theme::text(),
                window,
            );
        });
    }

    /// The chevrons at the ends of a scroll strip that asked for them (`data-chevrons`): 48 px wide, over a fade into the frame colour, where links are hidden past the edge. Widget coordinates; `true` is the left one.
    fn chevrons(&self) -> Vec<(Rectangle, bool, Id)> {
        let s = &*self.scene;
        let mut out = vec![];
        for n in 1..s.doc.len() {
            if !s.doc.nodes[n]
                .attrs
                .iter()
                .any(|(k, _)| k == "data-chevrons")
                || s.is_hidden(n)
            {
                continue;
            }
            let [_, _, w, h] = s.rects[n];
            let [cw, _] = s.content[n];
            let off = s.scroll.offsets.get(&n).map_or(0.0, |o| o[0]);
            let r = s.screen_rect(n);
            if cw <= w + 1.0 {
                continue;
            }
            if off > 1.0 {
                out.push((
                    Rectangle::new(Point::new(r[0], r[1]), Size::new(48.0, h)),
                    true,
                    n,
                ));
            }
            if off + w < cw - 1.0 {
                out.push((
                    Rectangle::new(Point::new(r[0] + r[2] - 48.0, r[1]), Size::new(48.0, h)),
                    false,
                    n,
                ));
            }
        }
        out
    }

    fn paint_chevrons(
        &self,
        r: &mut iced::Renderer,
        b: Rectangle,
        cursor: mouse::Cursor,
        vis: Rectangle,
    ) {
        for (rect, left, _) in self.chevrons() {
            let at = Rectangle::new(Point::new(b.x + rect.x, b.y + rect.y), rect.size());
            let hot = cursor.position().is_some_and(|p| at.contains(p));
            let clear = Color {
                a: 0.0,
                ..theme::frame()
            };
            let g = gradient::Linear::new(Degrees(if left { 270.0 } else { 90.0 }))
                .add_stop(0.0, clear)
                .add_stop(1.0, theme::frame());
            r.with_layer(vis, |r| {
                r.fill_quad(
                    Quad {
                        bounds: at,
                        border: Border::default(),
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    Background::Gradient(Gradient::Linear(g)),
                );
                r.fill_text(
                    Text {
                        content: if left { "\u{2039}" } else { "\u{203A}" }.to_string(),
                        bounds: at.size(),
                        size: Pixels(theme::HEADING),
                        line_height: LineHeight::Relative(1.3),
                        font: theme::font(Weight::Normal),
                        align_x: Alignment::Center,
                        align_y: alignment::Vertical::Center,
                        shaping: Shaping::Advanced,
                        wrapping: Wrapping::None,
                    },
                    Point::new(at.center_x(), at.center_y()),
                    if hot {
                        theme::accent_text()
                    } else {
                        theme::text()
                    },
                    at,
                );
            });
        }
    }

    fn paint_bars(&self, tree: &Tree, r: &mut iced::Renderer, b: Rectangle, vis: Rectangle) {
        let st = tree.state.downcast_ref::<State>();
        for bar in self.bars(st.hover, st.drag.map(|d| d.0)) {
            let clip = self
                .scene
                .clip_of(bar.node)
                .map_or(vis, |c| inter(vis, tr(c, b.position())).unwrap_or(vis));
            let thumb = Rectangle::new(
                Point::new(b.x + bar.thumb.x, b.y + bar.thumb.y),
                bar.thumb.size(),
            );
            let hot = st.drag.is_some_and(|d| d.0 == bar.node);
            r.with_layer(clip, |r| {
                r.fill_quad(
                    Quad {
                        bounds: thumb,
                        border: border::rounded(3.0),
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    if hot || !bar.vertical {
                        theme::edge()
                    } else {
                        theme::hairline_strong()
                    },
                );
            });
        }
    }

    /// The page's scrollbar: a quiet thumb at the right edge like the hand-written pages' (10 px wide, hairline colour), below a window-top fixed bar (a header) as that one lay over the scrollbar too. Nothing when the page fits.
    fn paint_scrollbar(&self, r: &mut iced::Renderer, b: Rectangle, vis: Rectangle) {
        let s: &Scene = &self.scene;
        let (page, view) = (s.page_height(), s.viewport.1);
        if page <= view + 0.5 {
            return;
        }
        let len = (view * view / page).clamp(24.0, view);
        let max = page - view;
        let top = s.scroll.page_y.clamp(0.0, max) / max * (view - len);
        // The fixed layer at the top (a header) covers the first pixels of the track.
        let cover = s.doc.nodes[0]
            .children
            .iter()
            .filter(|&&c| s.style(c).fixed && s.rects[c][1] <= 0.5)
            .map(|&c| s.rects[c][1] + s.rects[c][3])
            .fold(0.0f32, f32::max);
        let y = (b.y + top).max(b.y + cover);
        let bottom = b.y + top + len;
        if bottom <= y {
            return;
        }
        r.with_layer(vis, |r| {
            r.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(b.x + b.width - 10.0, y),
                        Size::new(10.0, bottom - y),
                    ),
                    border: border::rounded(3.0),
                    shadow: Shadow::default(),
                    snap: false,
                },
                theme::hairline_strong(),
            );
        });
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Styled<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.slots.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.slots);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let scene: &Scene = &self.scene;
        let nodes = self
            .slots
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(&self.slot_nodes)
            .map(|((el, t), node)| {
                let Some(&n) = node.as_ref() else {
                    return layout::Node::new(Size::ZERO);
                };
                // Hidden, clipped away or removed: nothing to host.
                if scene.is_hidden(n) || scene.clip_of(n).is_none() {
                    return layout::Node::new(Size::ZERO);
                }
                let [x, y, w, h] = scene.screen_rect(n);
                el.as_widget_mut()
                    .layout(
                        t,
                        renderer,
                        &layout::Limits::new(Size::ZERO, Size::new(w, h)),
                    )
                    .move_to(Point::new(x, y))
            })
            .collect();
        layout::Node::with_children(size, nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.slots
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((el, st), l)| el.as_widget_mut().operate(st, l, renderer, operation));
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let b = layout.bounds();
        // The safe-mode chord comes before everything, slots included.
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event
            && self.keys
            && modifiers.control()
            && modifiers.shift()
            && matches!(key.as_ref(), Key::Character("0" | ")"))
        {
            shell.publish((self.on_event)(StyledEvent::SafeMode));
            shell.capture_event();
            return;
        }
        // Slots, topmost first.
        let st = tree.state.downcast_mut::<State>();
        let (hover, pressed, size) = (st.hover, st.pressed, st.size);
        let inert = self.inert;
        for ((el, t), l) in self
            .slots
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
            .rev()
            .filter(|_| !inert)
        {
            el.as_widget_mut()
                .update(t, event, l, cursor, renderer, clipboard, shell, viewport);
            if shell.is_event_captured() {
                break;
            }
        }
        let captured = shell.is_event_captured();
        let st = tree.state.downcast_mut::<State>();
        let _ = (hover, pressed, size);
        let hit = |c: mouse::Cursor| c.position_in(b).and_then(|p| self.scene.hit_test(p.x, p.y));
        // A press on a scrollbar thumb (a little wider than drawn, so the 4 px strip bar can be caught) starts a drag.
        let grab = match (event, cursor.position_in(b)) {
            (Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), Some(p))
                if !captured =>
            {
                self.bars(st.hover, None)
                    .into_iter()
                    .chain(self.page_bar())
                    .find_map(|bar| {
                        let t = bar.thumb;
                        Rectangle::new(
                            Point::new(t.x - 4.0, t.y - 4.0),
                            Size::new(t.width + 8.0, t.height + 8.0),
                        )
                        .contains(p)
                        .then_some((
                            bar.node,
                            bar.vertical,
                            if bar.vertical { p.y } else { p.x },
                        ))
                    })
            }
            _ => None,
        };
        let chev = match (event, cursor.position_in(b)) {
            (Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), Some(p))
                if !captured =>
            {
                self.chevrons()
                    .into_iter()
                    .find(|(r, ..)| r.contains(p))
                    .map(|(_, left, n)| (n, left, self.scene.rects[n][2]))
            }
            _ => None,
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if chev.is_some() => {
                if let Some((n, left, w)) = chev {
                    // The scene's wheel handling moves the strip: a wheel of +dx scrolls toward the start.
                    let d = w * 0.7;
                    shell.publish((self.on_event)(StyledEvent::Wheel {
                        target: Some(n),
                        dx: if left { d } else { -d },
                        dy: 0.0,
                    }));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if grab.is_some() => {
                st.drag = grab;
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if st.drag.is_some() => {
                if let (Some((node, vertical, last)), Some(p)) = (st.drag, cursor.position_in(b)) {
                    let now = if vertical { p.y } else { p.x };
                    let ratio = self
                        .bars(st.hover, Some(node))
                        .into_iter()
                        .chain(self.page_bar())
                        .find(|bar| bar.node == node)
                        .map_or(1.0, |bar| bar.ratio);
                    // The scene's own wheel handling moves the container: dragging the thumb by `d` px is a wheel of `-d * ratio`.
                    let d = (now - last) * ratio;
                    st.drag = Some((node, vertical, now));
                    let (dx, dy) = if vertical { (0.0, -d) } else { (-d, 0.0) };
                    shell.publish((self.on_event)(StyledEvent::Wheel {
                        target: Some(node),
                        dx,
                        dy,
                    }));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                let h = hit(cursor);
                if h != st.hover {
                    st.hover = h;
                    st.rest = Some(Instant::now());
                    if self.tip_of(h).is_some() {
                        shell.request_redraw_at(Instant::now() + TIP_DELAY);
                    }
                    shell.publish((self.on_event)(StyledEvent::Hover(h)));
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if !captured => {
                if let Some(h) = hit(cursor) {
                    st.pressed = Some(h);
                    shell.publish((self.on_event)(StyledEvent::Active(Some(h))));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if st.drag.take().is_some() {
                    shell.capture_event();
                    return;
                }
                if let Some(p) = st.pressed.take() {
                    shell.publish((self.on_event)(StyledEvent::Active(None)));
                    if hit(cursor) == Some(p) && !captured {
                        shell.publish((self.on_event)(StyledEvent::Click(p)));
                    }
                }
            }
            // Only over this widget: a header strip above a page must not swallow the wheel meant for the page below it.
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if !captured && cursor.is_over(b) =>
            {
                let (dx, dy) = match *delta {
                    mouse::ScrollDelta::Lines { x, y } => (x * 48.0, y * 48.0),
                    mouse::ScrollDelta::Pixels { x, y } => (x, y),
                };
                shell.publish((self.on_event)(StyledEvent::Wheel {
                    target: hit(cursor),
                    dx,
                    dy,
                }));
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if !captured && self.keys =>
            {
                let nav = match key.as_ref() {
                    Key::Named(Named::Tab) => Some(if modifiers.shift() {
                        Nav::Prev
                    } else {
                        Nav::Next
                    }),
                    Key::Named(Named::ArrowUp) => Some(Nav::Dir(Dir::Up)),
                    Key::Named(Named::ArrowDown) => Some(Nav::Dir(Dir::Down)),
                    Key::Named(Named::ArrowLeft) => Some(Nav::Dir(Dir::Left)),
                    Key::Named(Named::ArrowRight) => Some(Nav::Dir(Dir::Right)),
                    Key::Named(Named::Enter | Named::Space) => Some(Nav::Activate),
                    _ => None,
                };
                if let Some(nav) = nav {
                    shell.publish((self.on_event)(StyledEvent::Nav(nav)));
                    shell.capture_event();
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                if b.size() != st.size {
                    st.size = b.size();
                    shell.publish((self.on_event)(StyledEvent::Resize(b.size())));
                }
                if self.scene.animating() {
                    shell.publish((self.on_event)(StyledEvent::Frame(*now)));
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let slot = self
            .slots
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .filter(|_| !self.inert)
            .map(|((el, t), l)| {
                el.as_widget()
                    .mouse_interaction(t, l, cursor, viewport, renderer)
            })
            .find(|i| *i != mouse::Interaction::None);
        if let Some(i) = slot {
            return i;
        }
        let st = tree.state.downcast_ref::<State>();
        let mut up = st.hover;
        while let Some(n) = up {
            if self.scene.doc.nodes[n].focusable {
                return mouse::Interaction::Pointer;
            }
            up = self.scene.doc.nodes[n].parent;
        }
        mouse::Interaction::None
    }

    fn draw(
        &self,
        tree: &Tree,
        r: &mut iced::Renderer,
        th: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let b = layout.bounds();
        let Some(vis) = inter(b, *viewport) else {
            return;
        };
        let t0 = std::time::Instant::now();
        let (ord, end, images) = image_order(self);
        let layered = layered_subtrees(self);
        for k in 0..=2 * images {
            let ps = Pass {
                k,
                ord: &ord,
                end: &end,
                layered: &layered,
            };
            r.with_layer(vis, |r| {
                self.paint(
                    tree,
                    layout,
                    r,
                    th,
                    style,
                    cursor,
                    viewport,
                    0,
                    b.position(),
                    (0.0, -self.scene.scroll.page_y.round()),
                    vis,
                    &ps,
                );
            });
        }
        self.paint_scrollbar(r, b, vis);
        self.paint_bars(tree, r, b, vis);
        self.paint_chevrons(r, b, cursor, vis);
        self.paint_tip(tree, r, b, *viewport);
        self.paint_marks(r, b, vis);
        if stats_on() {
            eprintln!(
                "engine: draw {} us ({} images, {} passes)",
                t0.elapsed().as_micros(),
                images,
                2 * images + 1
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        overlay::from_children(
            &mut self.slots,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: Clone + 'a> From<Styled<'a, M>> for Element<'a, M> {
    fn from(s: Styled<'a, M>) -> Self {
        Element::new(s)
    }
}
