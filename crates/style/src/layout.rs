//! A laid-out screen: the styled tree, the Taffy layout, text blocks, scroll state and animations. Built on a state change and
//! relaid out (not rebuilt) on a resize; nothing here runs per frame except [`Scene::tick`].
use crate::anim::{Anims, Eff};
use crate::cascade::{Budget, Cx, Failure, Interaction, Styler, flags};
use crate::doc::{Doc, Id};
use crate::fonts::Fonts;
use crate::props::{Computed, Ctx, Op, Transform};
use crate::sheet::{Sheet, Val};
use crate::text::{self, Block, Measure, TextStyle};
use crate::virt::{self, Builders};
use crate::warn::{Kind, Report};
use nocturne_core::customize::value::Color;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use taffy::geometry::Size;
use taffy::prelude::TaffyMaxContent;
use taffy::style::{AvailableSpace, Dimension, Display, LengthPercentageAuto, Position, Style};
use taffy::{NodeId, TaffyTree};

pub use crate::text::approx;

/// Smallest an essential control can be made by a theme.
pub const FLOOR_MIN_PX: f32 = 24.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    pub cascade: Duration,
    pub layout: Duration,
}

/// What a Taffy node stands for (the measure function needs to know).
#[derive(Clone, Copy, Debug)]
pub enum Leaf {
    Node(Id),
    /// The in-flow spacer that gives a virtual container its full height.
    Spacer(Id),
    /// The element's own text, as a box of its own between its `::before` and `::after` boxes.
    Own(Id),
}

#[derive(Clone, Debug, Default)]
pub struct Scroll {
    pub page_y: f32,
    pub offsets: HashMap<Id, [f32; 2]>,
}

/// The data a measure callback needs (borrowed from the scene while its Taffy tree is borrowed mutably).
pub struct Env<'a> {
    pub doc: &'a Doc,
    pub styles: &'a [Rc<Computed>],
    pub fonts: &'a Fonts,
    pub virt: &'a virt::State,
}

pub fn text_style(c: &Computed, fonts: &Fonts) -> TextStyle {
    TextStyle {
        size: c.font_size,
        weight: c.weight,
        italic: c.italic,
        font: fonts.resolve(&c.family),
        line_height: c.line_height_ratio(),
        lines: c.lines,
        letter_spacing: c.letter_spacing,
        ellipsis: c.ellipsis,
    }
}

pub fn shown<'a>(text: &'a str, c: &Computed) -> Cow<'a, str> {
    match c.text_transform {
        Transform::None => Cow::Borrowed(text),
        Transform::Upper => Cow::Owned(text.to_uppercase()),
        Transform::Lower => Cow::Owned(text.to_lowercase()),
    }
}

/// The natural size of a replaced element: the app's image, or the picture an `img` shows through `content: url()` once it is loaded.
fn natural(doc: &Doc, styles: &[Rc<Computed>], i: Id) -> Option<(u32, u32)> {
    let node = &doc.nodes[i];
    node.image.map(|m| (m.w, m.h)).or_else(|| {
        if node.tag == "img" {
            super::paint::pic_dims(styles[i].pic_url.as_deref()?)
        } else {
            None
        }
    })
}

fn node_measure(
    env: &Env,
    i: Id,
    known: Size<Option<f32>>,
    avail: Size<AvailableSpace>,
    measure: &mut Measure,
) -> Size<f32> {
    let node = &env.doc.nodes[i];
    if let Some((iw, ih)) = natural(env.doc, env.styles, i) {
        let (iw, ih) = (iw.max(1) as f32, ih.max(1) as f32);
        // The sizes the style asks for are folded into the available space, not into `known`: read them from the style.
        let st = &env.styles[i].layout.size;
        let want = |d: Dimension, k: Option<f32>, a: AvailableSpace| {
            k.or(match (d.expand(), a) {
                (taffy::style::ExpandedDimension::Length(l), _) => Some(l),
                (taffy::style::ExpandedDimension::Percent(_), AvailableSpace::Definite(v)) => {
                    Some(v)
                }
                _ => None,
            })
        };
        let known = Size {
            width: want(st.width, known.width, avail.width),
            height: want(st.height, known.height, avail.height),
        };
        return match (known.width, known.height) {
            (Some(w), Some(h)) => Size {
                width: w,
                height: h,
            },
            (Some(w), None) => Size {
                width: w,
                height: w * ih / iw,
            },
            (None, Some(h)) => Size {
                width: h * iw / ih,
                height: h,
            },
            _ => Size {
                width: iw,
                height: ih,
            },
        };
    }
    let Some(t) = &node.text else {
        return Size::ZERO;
    };
    let c = &env.styles[i];
    // `avail` is already the content box, so wrap to that.
    let w = match avail.width {
        AvailableSpace::Definite(w) => Some(w),
        AvailableSpace::MinContent => Some(0.0),
        AvailableSpace::MaxContent => None,
    };
    let ts = text_style(c, env.fonts);
    let t = shown(t, c);
    let (tw, th) = if ts.needs_block() {
        let b = text::block(&t, &ts, w, measure);
        (b.width, b.height)
    } else {
        measure(&t, &ts, w)
    };
    // Not rounded up: Iced lays text out at its exact measured size, and so must the pages that replace its widgets.
    Size {
        width: tw,
        height: th,
    }
}

pub fn run(tree: &mut TaffyTree<Leaf>, root: NodeId, env: &Env, measure: &mut Measure) {
    let _ = tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |inputs, _, ctx, style| {
        taffy::compute_leaf_layout(
            inputs,
            style,
            |_, _| 0.0,
            |known, avail| match ctx {
                Some(&mut Leaf::Node(i) | &mut Leaf::Own(i)) => {
                    node_measure(env, i, known, avail, measure)
                }
                Some(&mut Leaf::Spacer(c)) => {
                    let w = match avail.width {
                        AvailableSpace::Definite(w) => w,
                        _ => known.width.unwrap_or(0.0),
                    };
                    Size {
                        width: w,
                        height: virt::total_height(env, c, w, measure),
                    }
                }
                None => Size::ZERO,
            },
        )
    });
}

pub struct Scene {
    pub doc: Doc,
    pub sheet: Rc<Sheet>,
    pub(crate) st: Styler,
    pub fonts: Rc<Fonts>,
    fonts_gen: u32,
    pub viewport: (f32, f32),
    /// The box an absolutely positioned node is placed against when it is not its parent (the nearest positioned ancestor).
    cb: HashMap<Id, Id>,
    pub interaction: Interaction,
    /// Border box in the page's coordinates, before any scrolling: x, y, width, height. Fixed elements are in window coordinates.
    pub rects: Vec<[f32; 4]>,
    /// Scrollable content size (for `overflow: scroll`).
    pub content: Vec<[f32; 2]>,
    /// Border plus padding per side (left, top, right, bottom): where the content box starts.
    pub insets: Vec<[f32; 4]>,
    /// Border widths per side (left, top, right, bottom).
    pub borders: Vec<[f32; 4]>,
    /// Children in visual order (`order`, then source order), generated leaves at the ends: painting, hit testing and focus use this.
    pub kids: Vec<Vec<Id>>,
    /// The same children in paint order (`z-index`, stable).
    pub paint: Vec<Vec<Id>>,
    blocks: HashMap<Id, Rc<Block>>,
    pub scroll: Scroll,
    pub anims: Anims,
    /// Notes from the scene itself (essential floor, limits); [`Scene::warnings`] adds the sheet's and the cascade's.
    pub report: Report,
    pub timing: Timing,
    tree: TaffyTree<Leaf>,
    nodes: Vec<NodeId>,
    vp_node: NodeId,
    pub(crate) vwrap: HashMap<Id, (NodeId, NodeId)>,
    /// Elements whose own text sits in a box between their generated boxes.
    own: HashMap<Id, NodeId>,
    /// Origin (page coordinates) of each virtual container's item area.
    pub(crate) vorigin: HashMap<Id, (f32, f32)>,
    pub(crate) tree_dirty: bool,
    /// Nodes whose layout style changed since the last layout: their Taffy styles are replaced in place (no rebuild).
    style_dirty: Vec<Id>,
    needs_layout: bool,
    touched: Vec<Id>,
    budget: Option<Duration>,
    floor_min: HashSet<Id>,
    floor_reported: HashSet<Id>,
    pub(crate) virt: virt::State,
    /// Nodes `0..base_len` are the app's tree plus generated leaves; the rest are virtual items.
    pub(crate) base_len: usize,
    /// Nodes `0..real_len` are the app's own tree.
    real_len: usize,
}

/// What changed in an update.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Update {
    pub restyled: usize,
    pub relaid: bool,
}

fn px(v: f32) -> Dimension {
    Dimension::length(v)
}

impl Scene {
    /// Style and lay out a tree. Fails (without touching anything else) when the time budget runs out.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        mut doc: Doc,
        sheet: Rc<Sheet>,
        fonts: Rc<Fonts>,
        viewport: (f32, f32),
        it: Interaction,
        budget: Option<Duration>,
        builders: &Builders,
        measure: &mut Measure,
    ) -> Result<Scene, Failure> {
        let t0 = Instant::now();
        let deadline = budget.map(|b| t0 + b);
        let mut st = Styler::new();
        let real_len = doc.len();
        let f = flags(&doc, &it);
        {
            let mut cx = Cx {
                sheet: &sheet,
                vp: viewport,
                budget: Budget::new(deadline),
            };
            st.style_range(&mut doc, &mut cx, &f, 0)?;
        }
        let cascade = t0.elapsed();
        let n = doc.len();
        let mut tree = TaffyTree::new();
        let dummy = tree.new_leaf(Style::default()).expect("taffy node");
        let mut s = Scene {
            virt: virt::State::new(&doc, builders),
            base_len: n,
            real_len,
            fonts_gen: fonts.generation(),
            doc,
            sheet,
            st,
            fonts,
            viewport,
            cb: HashMap::new(),
            interaction: it,
            rects: vec![[0.0; 4]; n],
            content: vec![[0.0; 2]; n],
            insets: vec![[0.0; 4]; n],
            borders: vec![[0.0; 4]; n],
            kids: vec![],
            paint: vec![],
            blocks: HashMap::new(),
            scroll: Scroll::default(),
            anims: Anims::default(),
            report: Report::default(),
            timing: Timing::default(),
            tree,
            nodes: vec![],
            vp_node: dummy,
            vwrap: HashMap::new(),
            own: HashMap::new(),
            vorigin: HashMap::new(),
            tree_dirty: true,
            style_dirty: vec![],
            needs_layout: true,
            touched: vec![],
            budget,
            floor_min: HashSet::new(),
            floor_reported: HashSet::new(),
        };
        let t1 = Instant::now();
        s.layout_all(measure);
        s.timing = Timing {
            cascade,
            layout: t1.elapsed(),
        };
        s.sync_runs(0..n, Instant::now());
        Ok(s)
    }

    pub fn style(&self, i: Id) -> &Rc<Computed> {
        &self.st.styles[i]
    }

    pub fn text_style(&self, i: Id) -> TextStyle {
        text_style(&self.st.styles[i], &self.fonts)
    }

    /// Lines of text that is placed by the engine (clamp, ellipsis, letter-spacing); `None` for text the renderer wraps.
    pub fn block(&self, i: Id) -> Option<&Rc<Block>> {
        self.blocks.get(&i)
    }

    /// Animated overrides for painting node `i` (empty when nothing animates).
    pub fn eff(&self, i: Id) -> Eff {
        if self.anims.any() {
            self.anims.eff(i)
        } else {
            Eff::default()
        }
    }

    /// Everything noteworthy about the stylesheet and the last styling and layout, grouped by kind.
    pub fn warnings(&self) -> Report {
        let mut r = self.sheet.report.clone();
        r.merge(&self.st.report);
        r.merge(&self.report);
        r
    }

    pub fn is_hidden(&self, i: Id) -> bool {
        self.st.styles[i].layout.display == Display::None || self.doc.nodes[i].virtual_proto
    }

    pub fn page_height(&self) -> f32 {
        self.rects[0][3]
    }

    /// Colour of the surface behind `i`: the nearest ancestor with an opaque solid background, else the page's, else black
    /// (what `mask-image` scrims fade to).
    pub fn scrim_color(&self, i: Id) -> Color {
        let mut up = self.doc.nodes[i].parent;
        while let Some(p) = up {
            for f in self.st.styles[p].bg.iter() {
                if let crate::paint::Fill::Solid(c) = f
                    && c.a == 255
                {
                    return *c;
                }
            }
            up = self.doc.nodes[p].parent;
        }
        Color::rgba(0, 0, 0, 255)
    }

    // ---- tree and layout ----

    fn style_for(&self, i: Id) -> Style {
        let c = &self.st.styles[i];
        let node = &self.doc.nodes[i];
        let mut st = c.layout.clone();
        if i == 0 {
            // The root's percentage height is a share of the window (`html { height: 100% }`), so its children's percentages resolve.
            let height = match c.layout.size.height.expand() {
                taffy::style::ExpandedDimension::Percent(p) => px(self.viewport.1 * p),
                _ => Dimension::auto(),
            };
            st.size = Size {
                width: px(self.viewport.0),
                height,
            };
            st.min_size.height = LengthPercentageAuto::length(self.viewport.1);
        }
        if self.has_own_run(i) && st.display == Display::Block {
            // Generated boxes beside the text of a block (a bullet before a section title): they share one line.
            st.display = Display::Flex;
            st.align_items = taffy::style::AlignItems::CENTER;
        }
        if node.virtual_proto {
            st.display = Display::None;
        }
        if node.virt.is_some() {
            st.display = Display::Block;
        }
        if self.floor_min.contains(&i) {
            st.min_size = Size {
                width: LengthPercentageAuto::length(FLOOR_MIN_PX),
                height: LengthPercentageAuto::length(FLOOR_MIN_PX),
            };
        }
        if let Some((iw, ih)) = natural(&self.doc, &self.st.styles, i)
            && st.size.width == Dimension::auto()
            && st.size.height == Dimension::auto()
            && st.aspect_ratio.is_none()
        {
            // A replaced element with no size keeps its natural size (a block does not stretch it).
            st.size = Size {
                width: px(iw as f32),
                height: px(ih as f32),
            };
        }
        if node.virtual_item {
            virt::item_style(self, i, &mut st);
        }
        st
    }

    /// Text of its own that is a flex item in its own right: next to in-flow `::before`/`::after` boxes (an absolutely positioned one does not take part), or in a flex or grid container, where the browser wraps it in an anonymous item that `justify-content`/`align-items` place (the centred number in a round badge).
    fn has_own_run(&self, i: Id) -> bool {
        let n = &self.doc.nodes[i];
        n.text.is_some()
            && n.children.is_empty()
            && (matches!(
                self.st.styles[i].layout.display,
                Display::Flex | Display::Grid
            ) || [n.before, n.after].into_iter().flatten().any(|g| {
                self.st.styles[g].layout.position != Position::Absolute
                    && self.st.styles[g].layout.display != Display::None
            }))
    }

    fn build_kids(&mut self) {
        let n = self.doc.len();
        self.kids = (0..n)
            .map(|i| {
                let node = &self.doc.nodes[i];
                let mut k: Vec<Id> = node
                    .children
                    .iter()
                    .copied()
                    .filter(|&c| !self.doc.nodes[c].virtual_proto)
                    .collect();
                k.sort_by_key(|&c| self.st.styles[c].order);
                if let Some(b) = node.before {
                    k.insert(0, b);
                }
                if let Some(a) = node.after {
                    k.push(a);
                }
                k
            })
            .collect();
        // `z-index` takes part in the nearest stacking context, not only among siblings: a box without one is painted at the highest `z-index` found below it (a poster with `z-index: 3` inside an unstyled wrapper still lands above the sibling ribbon with 2).
        let mut eff: Vec<i32> = (0..n).map(|i| self.st.styles[i].z).collect();
        for i in (0..n).rev() {
            if self.st.styles[i].z == 0 {
                let below = self.doc.nodes[i]
                    .children
                    .iter()
                    .map(|&c| eff[c])
                    .max()
                    .unwrap_or(0);
                eff[i] = below.max(0);
            }
        }
        self.paint = self
            .kids
            .iter()
            .map(|k| {
                let mut p = k.clone();
                p.sort_by_key(|&c| eff[c]);
                p
            })
            .collect();
    }

    fn build_tree(&mut self) {
        self.build_kids();
        let n = self.doc.len();
        let mut tree: TaffyTree<Leaf> = TaffyTree::with_capacity(n + 8);
        // Fractional boxes like Iced's own layout: the hand-written pages and the engine place the same text at the same pixel.
        tree.disable_rounding();
        let nodes: Vec<NodeId> = (0..n)
            .map(|i| {
                tree.new_leaf_with_context(self.style_for(i), Leaf::Node(i))
                    .expect("taffy node")
            })
            .collect();
        let layer = tree
            .new_leaf(Style {
                position: Position::Absolute,
                inset: taffy::geometry::Rect {
                    top: LengthPercentageAuto::length(0.0),
                    left: LengthPercentageAuto::length(0.0),
                    right: LengthPercentageAuto::length(0.0),
                    bottom: LengthPercentageAuto::length(0.0),
                },
                ..Style::default()
            })
            .expect("taffy node");
        let mut fixed: Vec<NodeId> = vec![];
        let mut vwrap = HashMap::new();
        let mut own = HashMap::new();
        // An absolutely positioned box is placed against its nearest positioned ancestor, not its parent (Taffy's rule): hang it from that ancestor when both axes are pinned by insets (a box with an auto axis keeps its static position under its parent).
        self.cb.clear();
        let mut moved: HashMap<Id, Vec<NodeId>> = HashMap::new();
        #[allow(clippy::needless_range_loop)]
        for k in 1..n {
            let c = &self.st.styles[k];
            if c.fixed || c.layout.position != Position::Absolute || self.doc.nodes[k].virtual_item
            {
                continue;
            }
            let pinned =
                |a: LengthPercentageAuto, b: LengthPercentageAuto| !a.is_auto() || !b.is_auto();
            if !(pinned(c.layout.inset.left, c.layout.inset.right)
                && pinned(c.layout.inset.top, c.layout.inset.bottom))
            {
                continue;
            }
            let Some(mut a) = self.doc.nodes[k].parent else {
                continue;
            };
            while a != 0 && !self.st.styles[a].positioned {
                a = self.doc.nodes[a].parent.unwrap_or(0);
            }
            if Some(a) != self.doc.nodes[k].parent && self.doc.nodes[a].virt.is_none() {
                self.cb.insert(k, a);
                moved.entry(a).or_default().push(nodes[k]);
            }
        }
        for i in 0..n {
            let mut ch: Vec<NodeId> = vec![];
            let mut items: Vec<NodeId> = vec![];
            for &k in &self.kids[i] {
                let kn = &self.doc.nodes[k];
                if self.cb.contains_key(&k) {
                    continue;
                }
                if self.st.styles[k].fixed
                    && self.st.styles[k].layout.position == Position::Absolute
                {
                    fixed.push(nodes[k]);
                } else if kn.virtual_item {
                    items.push(nodes[k]);
                } else {
                    ch.push(nodes[k]);
                }
            }
            if let Some(m) = moved.get(&i) {
                ch.extend(m);
            }
            if self.has_own_run(i) {
                let run = tree
                    .new_leaf_with_context(Style::default(), Leaf::Own(i))
                    .expect("taffy node");
                let at = usize::from(
                    self.doc.nodes[i]
                        .before
                        .is_some_and(|b| ch.contains(&nodes[b])),
                );
                ch.insert(at.min(ch.len()), run);
                own.insert(i, run);
            }
            if self.doc.nodes[i].virt.is_some() {
                let wrapper = tree
                    .new_leaf(Style {
                        position: Position::Relative,
                        size: Size {
                            width: Dimension::percent(1.0),
                            height: Dimension::auto(),
                        },
                        ..Style::default()
                    })
                    .expect("taffy node");
                let spacer = tree
                    .new_leaf_with_context(
                        Style {
                            size: Size {
                                width: Dimension::percent(1.0),
                                height: Dimension::auto(),
                            },
                            ..Style::default()
                        },
                        Leaf::Spacer(i),
                    )
                    .expect("taffy node");
                let mut all = vec![spacer];
                all.extend(items);
                tree.set_children(wrapper, &all).expect("children");
                ch.push(wrapper);
                vwrap.insert(i, (wrapper, spacer));
            }
            if !ch.is_empty() {
                tree.set_children(nodes[i], &ch).expect("children");
            }
        }
        tree.set_children(layer, &fixed).expect("children");
        let vp = tree
            .new_leaf(Style {
                display: Display::Block,
                size: Size {
                    width: px(self.viewport.0),
                    height: px(self.viewport.1),
                },
                ..Style::default()
            })
            .expect("taffy node");
        tree.set_children(vp, &[nodes[0], layer]).expect("children");
        self.tree = tree;
        self.nodes = nodes;
        self.vp_node = vp;
        self.vwrap = vwrap;
        self.own = own;
        self.tree_dirty = false;
    }

    fn run_layout(&mut self, measure: &mut Measure) {
        let env = Env {
            doc: &self.doc,
            styles: &self.st.styles,
            fonts: &self.fonts,
            virt: &self.virt,
        };
        run(&mut self.tree, self.vp_node, &env, measure);
    }

    /// Lay out and collect rectangles; applies the essential floor (a second pass when a size had to be forced).
    pub(crate) fn layout_all(&mut self, measure: &mut Measure) {
        for _ in 0..4 {
            if self.tree_dirty || self.nodes.len() != self.doc.len() {
                self.build_tree();
                self.style_dirty.clear();
            } else {
                // Only some styles changed: replace them in the Taffy tree; clean subtrees keep their cached layout.
                for id in std::mem::take(&mut self.style_dirty) {
                    let st = self.style_for(id);
                    let _ = self.tree.set_style(self.nodes[id], st);
                }
            }
            self.needs_layout = false;
            self.run_layout(measure);
            self.collect(measure);
            let mut again = self.enforce_floor_sizes();
            if self.sync_virtual(measure) {
                again = true;
            }
            if !again {
                break;
            }
            self.tree_dirty = true;
        }
        self.nudge_essentials();
        self.clamp_scroll();
    }

    fn collect(&mut self, measure: &mut Measure) {
        let n = self.doc.len();
        self.rects.resize(n, [0.0; 4]);
        self.content.resize(n, [0.0; 2]);
        self.insets.resize(n, [0.0; 4]);
        self.borders.resize(n, [0.0; 4]);
        self.blocks.clear();
        self.vorigin.clear();
        // Parents are stored before children, so one forward pass turns local locations into page coordinates.
        for i in 0..n {
            let l = *self.tree.layout(self.nodes[i]).expect("layout");
            let node = &self.doc.nodes[i];
            let (ox, oy) = match node.parent {
                _ if self.st.styles[i].fixed
                    && self.st.styles[i].layout.position == Position::Absolute =>
                {
                    (0.0, 0.0)
                }
                Some(p) if node.virtual_item => self.vorigin.get(&p).copied().unwrap_or((0.0, 0.0)),
                Some(_) if self.cb.contains_key(&i) => {
                    let a = self.cb[&i];
                    (self.rects[a][0], self.rects[a][1])
                }
                Some(p) => (self.rects[p][0], self.rects[p][1]),
                None => (0.0, 0.0),
            };
            self.rects[i] = [
                ox + l.location.x,
                oy + l.location.y,
                l.size.width,
                l.size.height,
            ];
            self.content[i] = [
                l.size.width + l.scroll_width(),
                l.size.height + l.scroll_height(),
            ];
            self.borders[i] = [l.border.left, l.border.top, l.border.right, l.border.bottom];
            self.insets[i] = [
                l.border.left + l.padding.left,
                l.border.top + l.padding.top,
                l.border.right + l.padding.right,
                l.border.bottom + l.padding.bottom,
            ];
            if let Some(run) = self.own.get(&i) {
                // The text is drawn in its own box: the insets stand for the gap between it and the element's edges.
                let r = self.tree.layout(*run).expect("layout");
                let (x, y) = (r.location.x, r.location.y);
                self.insets[i] = [
                    x,
                    y,
                    (l.size.width - x - r.size.width).max(0.0),
                    (l.size.height - y - r.size.height).max(0.0),
                ];
            }
            if let Some((w, _)) = self.vwrap.get(&i) {
                let wl = self.tree.layout(*w).expect("layout").location;
                self.vorigin
                    .insert(i, (self.rects[i][0] + wl.x, self.rects[i][1] + wl.y));
            }
        }
        // Text the engine breaks into lines itself.
        for i in 0..n {
            let c = &self.st.styles[i];
            if c.layout.display == Display::None {
                continue;
            }
            let Some(t) = &self.doc.nodes[i].text else {
                continue;
            };
            let ts = text_style(c, &self.fonts);
            if ts.needs_block() {
                let [_, _, w, _] = self.rects[i];
                let ins = self.insets[i];
                let b = text::block(
                    &shown(t, c),
                    &ts,
                    Some((w - ins[0] - ins[2]).max(0.0)),
                    measure,
                );
                self.blocks.insert(i, Rc::new(b));
            }
        }
    }

    /// Essential controls smaller than the floor get a minimum size (the caller lays out again).
    fn enforce_floor_sizes(&mut self) -> bool {
        if !self.sheet.has_user {
            return false;
        }
        let mut again = false;
        for i in 0..self.doc.len() {
            if !self.doc.nodes[i].essential || self.is_hidden(i) {
                continue;
            }
            let [_, _, w, h] = self.rects[i];
            if (w < FLOOR_MIN_PX || h < FLOOR_MIN_PX) && self.floor_min.insert(i) {
                self.floor_note(
                    i,
                    "made it smaller than 24 px; it was given a 24 px minimum",
                );
                again = true;
            }
        }
        again
    }

    fn floor_note(&mut self, i: Id, what: &str) {
        if self.floor_reported.insert(i) || what.contains("moved") {
            let d = self.doc.describe(i);
            self.report.add(
                Kind::Floor,
                format!("a rule {what} on `{d}`, an essential control"),
            );
        }
    }

    /// Essential controls that ended up (mostly) outside the window are moved to the nearest place where they are visible.
    fn nudge_essentials(&mut self) {
        let (vw, vh) = self.viewport;
        let page_h = self.rects[0][3].max(vh);
        for i in 0..self.doc.len() {
            if !self.doc.nodes[i].essential || self.is_hidden(i) {
                continue;
            }
            // Inside a clipping ancestor the position is the container's business.
            let mut clipped = false;
            let mut fixed = false;
            let mut up = Some(i);
            while let Some(a) = up {
                let c = &self.st.styles[a];
                if a != 0
                    && a != i
                    && (c.layout.overflow.x != taffy::style::Overflow::Visible
                        || c.layout.overflow.y != taffy::style::Overflow::Visible)
                {
                    clipped = true;
                }
                fixed |= c.fixed;
                up = self.doc.nodes[a].parent;
            }
            if clipped {
                continue;
            }
            let [x, y, w, h] = self.rects[i];
            let (ymax, yh) = if fixed { (vh, vh) } else { (page_h, page_h) };
            let _ = yh;
            let vis = |a: f32, len: f32, max: f32| -> f32 {
                if len <= 0.0 {
                    return 1.0;
                }
                ((a + len).min(max) - a.max(0.0)).max(0.0) / len
            };
            if vis(x, w, vw) * vis(y, h, ymax) >= 0.5 {
                continue;
            }
            let fit =
                |a: f32, len: f32, max: f32| -> f32 { a.clamp(0.0, (max - len).max(0.0)) - a };
            let (dx, dy) = (fit(x, w, vw), fit(y, h, ymax));
            self.shift_subtree(i, dx, dy);
            self.floor_note(i, "moved it off screen; it was moved back into view");
        }
    }

    fn shift_subtree(&mut self, i: Id, dx: f32, dy: f32) {
        let mut stack = vec![i];
        while let Some(n) = stack.pop() {
            self.rects[n][0] += dx;
            self.rects[n][1] += dy;
            stack.extend(self.kids[n].iter().copied());
        }
    }

    // ---- updates ----

    /// New window size: layout again; the cascade too when the sheet depends on the window.
    pub fn resize(&mut self, viewport: (f32, f32), measure: &mut Measure) -> Duration {
        let t = Instant::now();
        if viewport == self.viewport {
            return t.elapsed();
        }
        self.viewport = viewport;
        // The roots follow the window whatever the sheet does: Taffy's cache keys on the available space.
        let vp = Style {
            display: Display::Block,
            size: Size {
                width: px(viewport.0),
                height: px(viewport.1),
            },
            ..Style::default()
        };
        let _ = self.tree.set_style(self.vp_node, vp);
        self.style_dirty.push(0);
        self.needs_layout = true;
        if self.sheet.uses_viewport {
            let f = flags(&self.doc, &self.interaction);
            if self.restyle_with(&f, Instant::now(), measure).is_err() {
                self.report.add(
                    Kind::Limit,
                    "restyle after a resize ran over its time budget",
                );
            }
        } else {
            self.layout_all(measure);
        }
        t.elapsed()
    }

    /// The pointer or focus moved: restyle what can have changed, relayout only if a layout property did.
    pub fn update(
        &mut self,
        it: Interaction,
        now: Instant,
        measure: &mut Measure,
    ) -> Result<Update, Failure> {
        self.interaction = it;
        let f = flags(&self.doc, &it);
        self.restyle_with(&f, now, measure)
    }

    /// Call after editing classes or attributes through [`Scene::touch`].
    pub fn refresh(&mut self, now: Instant, measure: &mut Measure) -> Result<Update, Failure> {
        let f = flags(&self.doc, &self.interaction);
        self.restyle_with(&f, now, measure)
    }

    /// Mark nodes whose classes, attributes or text the app changed.
    pub fn touch(&mut self, ids: &[Id]) {
        self.touched.extend_from_slice(ids);
        for &i in ids {
            if self.doc.nodes[i].text.is_some() {
                self.tree_dirty = true;
            }
        }
    }

    /// A new font set (a family arrived, or the installed list changed): text is measured again.
    pub fn set_fonts(&mut self, fonts: Rc<Fonts>, measure: &mut Measure) {
        self.fonts = fonts;
        self.fonts_gen = self.fonts.generation();
        self.tree_dirty = true;
        self.layout_all(measure);
    }

    fn restyle_with(
        &mut self,
        f: &[u8],
        now: Instant,
        measure: &mut Measure,
    ) -> Result<Update, Failure> {
        let t = Instant::now();
        let touched = std::mem::take(&mut self.touched);
        let mut cx = Cx {
            sheet: &self.sheet,
            vp: self.viewport,
            budget: Budget::new(self.budget.map(|b| t + b)),
        };
        let r = self.st.restyle(&self.doc, &mut cx, f, &touched)?;
        let mut up = Update {
            restyled: r.changed.len(),
            relaid: false,
        };
        if r.structural {
            self.rebuild_styles(f, measure)?;
            up.relaid = true;
            return Ok(up);
        }
        let mut paint_lists = false;
        self.sync_runs(r.changed.iter().map(|c| c.0), now);
        for (id, old) in &r.changed {
            let new = self.st.styles[*id].clone();
            self.anims.changed(*id, old, &new, now);
            if old.order != new.order || old.fixed != new.fixed {
                // The shape of the Taffy tree (child order, the fixed layer) changed.
                self.tree_dirty = true;
            } else if layout_affecting(old, &new) {
                self.style_dirty.push(*id);
                self.needs_layout = true;
            }
            paint_lists |= old.z != new.z;
        }
        if paint_lists && !self.tree_dirty {
            self.build_kids();
        }
        if self.tree_dirty || self.needs_layout || self.fonts_gen != self.fonts.generation() {
            if self.fonts_gen != self.fonts.generation() {
                // New faces change every text measurement.
                self.tree_dirty = true;
            }
            self.fonts_gen = self.fonts.generation();
            self.layout_all(measure);
            up.relaid = true;
        }
        Ok(up)
    }

    /// A generated leaf appeared or vanished: restyle from scratch (virtual items are rebuilt by the layout).
    fn rebuild_styles(&mut self, f: &[u8], measure: &mut Measure) -> Result<(), Failure> {
        let keep = self.real_len;
        self.doc.truncate(keep);
        self.virt.reset();
        let mut cx = Cx {
            sheet: &self.sheet,
            vp: self.viewport,
            budget: Budget::new(self.budget.map(|b| Instant::now() + b)),
        };
        let flags_now = flags(&self.doc, &self.interaction);
        let _ = f;
        self.st.style_range(&mut self.doc, &mut cx, &flags_now, 0)?;
        self.base_len = self.doc.len();
        self.anims.clear();
        self.tree_dirty = true;
        self.layout_all(measure);
        self.sync_runs(0..self.doc.len(), Instant::now());
        Ok(())
    }

    /// Start, keep or stop the `@keyframes` animation of each node (only opacity, transform and colour are animated; an essential control is never animated).
    fn sync_runs(&mut self, ids: impl IntoIterator<Item = Id>, now: Instant) {
        if self.sheet.keyframes.is_empty() {
            return;
        }
        let ctx = Ctx {
            vw: self.viewport.0,
            vh: self.viewport.1,
            rem: 16.0,
        };
        for id in ids {
            let c = self.st.styles[id].clone();
            // Nothing runs on an element that is not drawn (the web client's base sheet has endless spinners on hidden elements).
            let shown = {
                let mut n = Some(id);
                let mut ok = true;
                while let Some(i) = n {
                    let st = &self.st.styles[i];
                    if st.layout.display == taffy::style::Display::None || !st.visible {
                        ok = false;
                        break;
                    }
                    n = self.doc.nodes[i].parent;
                }
                ok
            };
            let spec = c
                .anim
                .as_ref()
                .filter(|_| shown && !self.doc.nodes[id].essential);
            let kf = spec.and_then(|a| self.sheet.keyframes.get(&a.name).cloned());
            let spec = kf.as_ref().and(spec);
            let base = Eff {
                opacity: Some(c.opacity),
                xf: Some(c.xf),
                color: Some(c.color),
                ..Eff::default()
            };
            self.anims.set_run(
                id,
                spec,
                || {
                    let mut out = vec![];
                    for (at, decls) in kf.iter().flat_map(|k| k.iter()) {
                        let mut f = (*c).clone();
                        let mut e = Eff::default();
                        for d in decls.iter() {
                            let Val::Ops(ops) = &d.val else {
                                continue;
                            };
                            for op in ops {
                                op.apply(&mut f, ctx, &c);
                                match op {
                                    Op::Opacity(_) => e.opacity = Some(f.opacity),
                                    Op::Transform { .. } => e.xf = Some(f.xf),
                                    Op::Color(_) => e.color = Some(f.color),
                                    _ => {}
                                }
                            }
                        }
                        out.push((*at, e));
                    }
                    out
                },
                &base,
                now,
            );
        }
    }

    /// Only endless `@keyframes` animations are running (30 frames a second are enough).
    pub fn decorative_only(&self) -> bool {
        self.anims.decorative_only()
    }

    /// Advance animations to `now`; true while any is still running (keep requesting frames).
    pub fn tick(&mut self, now: Instant) -> bool {
        self.anims.tick(now)
    }

    /// What keeps frames coming, for `NOCTURNE_REDRAW_LOG`.
    pub fn anim_summary(&self) -> String {
        let mut s = self.anims.summary();
        for id in self.anims.nodes() {
            if let Some(n) = self.doc.nodes.get(id) {
                s += &format!(" [{} {:?}]", n.tag, n.classes);
            }
        }
        s
    }

    pub fn animating(&self) -> bool {
        self.anims.active()
    }

    /// Is `doc` the tree this scene was built from (apart from the generated leaves and the instantiated virtual items)? A page whose rebuilt tree equals the one on screen keeps its scene, scroll and animations.
    pub fn same_tree(&self, doc: &Doc) -> bool {
        doc.nodes.len() == self.real_len
            && self.doc.nodes[..self.real_len]
                .iter()
                .zip(&doc.nodes)
                .all(|(a, b)| {
                    a.tag == b.tag
                        && a.id == b.id
                        && a.classes == b.classes
                        && a.attrs == b.attrs
                        && a.text == b.text
                        && a.inline == b.inline
                        && a.parent == b.parent
                        // Virtual items are appended to their container's children beyond the app's own nodes.
                        && a.children.iter().filter(|&&k| k < self.real_len).eq(b.children.iter())
                        && a.focusable == b.focusable
                        && a.essential == b.essential
                        && a.slot == b.slot
                        && a.image == b.image
                        && a.virt == b.virt
                        && a.virtual_proto == b.virtual_proto
                })
    }
}

fn layout_affecting(a: &Computed, b: &Computed) -> bool {
    a.layout != b.layout
        || a.order != b.order
        || a.fixed != b.fixed
        || a.font_size != b.font_size
        || a.weight != b.weight
        || a.italic != b.italic
        || a.family != b.family
        || a.line_height != b.line_height
        || a.letter_spacing != b.letter_spacing
        || a.lines != b.lines
        || a.ellipsis != b.ellipsis
        || a.text_transform != b.text_transform
}
