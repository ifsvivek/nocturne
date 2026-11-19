//! The virtual container: a long list (a library grid) that lays out only the items near the viewport.
//! The container's size comes from a prototype item (one example child the app marks with `Doc::virtual_proto`, styled by the
//! theme like every item but never shown): its computed width, height or aspect ratio, margins and the container's `gap` give
//! the cell metrics, so the theme controls the grid through ordinary CSS. Items are uniform cells laid out left to right.
//! The scene instantiates real subtrees (through the app's builder) only for the rows in or near the view, styles them with the
//! shared cascade cache (a new row costs almost nothing) and lays them out as absolutely positioned children.
use crate::doc::{Doc, Id};
use crate::layout::{Env, Leaf, Scene, run};
use crate::text::Measure;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use taffy::TaffyTree;
use taffy::geometry::Size;
use taffy::style::{Dimension, LengthPercentageAuto, Position, Style};
use taffy::style::{ExpandedDimension, ExpandedLengthPercentage, ExpandedLengthPercentageAuto};

/// Builds item `index` as one new child of the container.
pub type VirtBuilder = Rc<dyn Fn(&mut Doc, Id, usize)>;
pub type Builders = HashMap<Id, VirtBuilder>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grid {
    pub cols: usize,
    /// Cell content size.
    pub item_w: f32,
    pub item_h: f32,
    /// Distance between the left (top) edges of neighbouring cells.
    pub pitch_x: f32,
    pub pitch_y: f32,
    /// Margin of the first cell.
    pub ml: f32,
    pub mt: f32,
    pub rows: usize,
    pub total: f32,
}

#[derive(Default)]
pub struct State {
    builders: Builders,
    containers: Vec<Id>,
    info: HashMap<Id, Info>,
    /// Item node -> (x, y, w, h) inside the container's item area.
    pub(crate) pos: HashMap<Id, [f32; 4]>,
    /// Item node -> its index in the list.
    pub index: HashMap<Id, usize>,
    proto_h: RefCell<HashMap<(Id, u32), f32>>,
}

#[derive(Default)]
struct Info {
    range: Range<usize>,
    grid: Grid,
}

impl State {
    pub fn new(doc: &Doc, builders: &Builders) -> Self {
        Self {
            builders: builders.clone(),
            containers: (0..doc.len())
                .filter(|&i| doc.nodes[i].virt.is_some())
                .collect(),
            ..Self::default()
        }
    }

    pub fn reset(&mut self) {
        self.info.clear();
        self.pos.clear();
        self.index.clear();
        self.proto_h.borrow_mut().clear();
    }

    pub fn containers(&self) -> &[Id] {
        &self.containers
    }

    pub fn grid(&self, c: Id) -> Option<Grid> {
        self.info.get(&c).map(|i| i.grid)
    }

    /// Indices currently instantiated.
    pub fn range(&self, c: Id) -> Range<usize> {
        self.info.get(&c).map_or(0..0, |i| i.range.clone())
    }
}

fn proto_of(doc: &Doc, c: Id) -> Option<Id> {
    doc.nodes[c]
        .children
        .iter()
        .copied()
        .find(|&k| doc.nodes[k].virtual_proto)
}

fn lp(v: taffy::style::LengthPercentage, w: f32) -> f32 {
    match v.expand() {
        ExpandedLengthPercentage::Length(l) => l,
        ExpandedLengthPercentage::Percent(p) => p * w,
        #[allow(unreachable_patterns)]
        _ => 0.0,
    }
}

fn lpa(v: LengthPercentageAuto, w: f32) -> f32 {
    match v.expand() {
        ExpandedLengthPercentageAuto::Length(l) => l,
        ExpandedLengthPercentageAuto::Percent(p) => p * w,
        _ => 0.0,
    }
}

/// Height of the prototype laid out standalone at width `w` (cached).
fn proto_height(env: &Env, c: Id, proto: Id, w: f32, measure: &mut Measure) -> f32 {
    let key = (c, w.to_bits());
    if let Some(h) = env.virt.proto_h.borrow().get(&key) {
        return *h;
    }
    let st = &env.styles[proto].layout;
    let h = match st.size.height.expand() {
        ExpandedDimension::Length(h) => h,
        _ => match st.aspect_ratio {
            Some(r) if r > 0.0 => w / r,
            _ => mini_layout(env, proto, w, measure),
        },
    };
    let h = if h.is_finite() { h.max(0.0) } else { 0.0 };
    env.virt.proto_h.borrow_mut().insert(key, h);
    h
}

/// Lay the prototype subtree out alone to learn its content height.
fn mini_layout(env: &Env, root: Id, w: f32, measure: &mut Measure) -> f32 {
    let mut tree: TaffyTree<Leaf> = TaffyTree::new();
    let mut ids = vec![];
    let mut order = vec![root];
    while let Some(n) = order.pop() {
        ids.push(n);
        order.extend(env.doc.nodes[n].children.iter().copied());
        order.extend(env.doc.nodes[n].before);
        order.extend(env.doc.nodes[n].after);
    }
    let mut map: HashMap<Id, taffy::NodeId> = HashMap::new();
    for &n in &ids {
        let mut st = env.styles[n].layout.clone();
        if n == root {
            st.size.width = Dimension::length(w);
            st.position = Position::Relative;
            st.margin = zero_margin();
        }
        if let Ok(t) = tree.new_leaf_with_context(st, Leaf::Node(n)) {
            map.insert(n, t);
        }
    }
    for &n in &ids {
        let node = &env.doc.nodes[n];
        let mut ch: Vec<taffy::NodeId> = vec![];
        ch.extend(node.before.and_then(|b| map.get(&b)));
        ch.extend(node.children.iter().filter_map(|k| map.get(k)));
        ch.extend(node.after.and_then(|a| map.get(&a)));
        if !ch.is_empty()
            && let Some(t) = map.get(&n)
        {
            let _ = tree.set_children(*t, &ch);
        }
    }
    let Some(&r) = map.get(&root) else { return 0.0 };
    run(&mut tree, r, env, measure);
    tree.layout(r).map_or(0.0, |l| l.size.height)
}

/// Cell metrics for container `c` whose content box is `width` wide.
pub fn grid_for(env: &Env, c: Id, width: f32, measure: &mut Measure) -> Grid {
    let count = env.doc.nodes[c].virt.map_or(0, |v| v.count);
    let Some(proto) = proto_of(env.doc, c) else {
        return Grid::default();
    };
    let cs = &env.styles[c].layout;
    let ps = &env.styles[proto].layout;
    let (gx, gy) = (lp(cs.gap.width, width), lp(cs.gap.height, width));
    let (ml, mr, mt, mb) = (
        lpa(ps.margin.left, width),
        lpa(ps.margin.right, width),
        lpa(ps.margin.top, width),
        lpa(ps.margin.bottom, width),
    );
    let item_w = match ps.size.width.expand() {
        ExpandedDimension::Length(l) => l,
        ExpandedDimension::Percent(p) => p * width,
        _ => (width - ml - mr).max(1.0),
    }
    .clamp(1.0, width.max(1.0));
    let item_h = proto_height(env, c, proto, item_w, measure);
    let pitch_x = item_w + ml + mr + gx;
    let cols = (((width + gx) / pitch_x.max(1.0)).floor() as usize).max(1);
    let pitch_y = item_h + mt + mb + gy;
    let rows = count.div_ceil(cols);
    let total = if rows == 0 {
        0.0
    } else {
        rows as f32 * pitch_y - gy
    };
    Grid {
        cols,
        item_w,
        item_h,
        pitch_x,
        pitch_y,
        ml,
        mt,
        rows,
        total: total.max(0.0),
    }
}

/// Height of the whole list (what the container's spacer measures to).
pub fn total_height(env: &Env, c: Id, width: f32, measure: &mut Measure) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    grid_for(env, c, width, measure).total
}

fn zero_margin() -> taffy::geometry::Rect<LengthPercentageAuto> {
    let z = LengthPercentageAuto::length(0.0);
    taffy::geometry::Rect {
        left: z,
        right: z,
        top: z,
        bottom: z,
    }
}

/// Position and size of a virtual item as an absolute child of the container's item area.
pub fn item_style(s: &Scene, i: Id, st: &mut Style) {
    let Some(p) = s.virt.pos.get(&i) else { return };
    st.position = Position::Absolute;
    st.inset.left = LengthPercentageAuto::length(p[0]);
    st.inset.top = LengthPercentageAuto::length(p[1]);
    st.inset.right = LengthPercentageAuto::auto();
    st.inset.bottom = LengthPercentageAuto::auto();
    st.size = Size {
        width: Dimension::length(p[2]),
        height: Dimension::length(p[3]),
    };
    st.margin = zero_margin();
}

impl Scene {
    /// The rows of each virtual container that are in or near the view, as item indices.
    fn wanted(&self, c: Id, g: &Grid) -> Range<usize> {
        let count = self.doc.nodes[c].virt.map_or(0, |v| v.count);
        let over = self.doc.nodes[c].virt.map_or(2, |v| v.overscan_rows);
        if g.cols == 0 || g.pitch_y <= 0.0 || count == 0 {
            return 0..0;
        }
        let [_, y, _, h] = self.rects[c];
        let off = self.scroll.offsets.get(&c).copied().unwrap_or_default();
        let (vh, py) = (self.viewport.1, self.scroll.page_y);
        // Visible part of the container (page coordinates), then in item-area coordinates.
        let top = y.max(py);
        let bottom = (y + h).min(py + vh);
        let origin = self.vorigin.get(&c).map_or(y, |o| o.1);
        let (a, b) = if self.st.styles[c].fixed {
            (0.0, vh)
        } else {
            (top - origin + off[1], bottom - origin + off[1])
        };
        let (a, b) = if bottom <= top { (0.0, 0.0) } else { (a, b) };
        let first = ((a / g.pitch_y).floor().max(0.0) as usize).saturating_sub(over);
        let last = (((b / g.pitch_y).ceil().max(0.0) as usize) + over).min(g.rows);
        (first * g.cols)..(last * g.cols).min(count)
    }

    /// Instantiate the items for the current view. Returns true when the item set changed (the caller lays out again).
    pub(crate) fn sync_virtual(&mut self, measure: &mut Measure) -> bool {
        if self.virt.containers().is_empty() {
            return false;
        }
        let mut plan: Vec<(Id, Range<usize>, Grid)> = vec![];
        let mut changed = false;
        for &c in &self.virt.containers().to_vec() {
            let [_, _, w, _] = self.rects[c];
            let ins = self.insets[c];
            let width = (w - ins[0] - ins[2]).max(0.0);
            if width <= 0.0 || self.is_hidden(c) {
                plan.push((c, 0..0, Grid::default()));
                continue;
            }
            let env = Env {
                doc: &self.doc,
                styles: &self.st.styles,
                fonts: &self.fonts,
                virt: &self.virt,
            };
            let g = grid_for(&env, c, width, measure);
            let want = self.wanted(c, &g);
            let have = self.virt.info.get(&c);
            if have.is_none_or(|h| h.range != want || h.grid != g) {
                changed = true;
            }
            plan.push((c, want, g));
        }
        if !changed {
            return false;
        }
        // Rebuild the item region from scratch: cheap, because only visible rows exist and styles come from the shared cache.
        let base = self.base_len;
        self.doc.truncate(base);
        self.st.truncate(base);
        self.virt.pos.clear();
        self.virt.index.clear();
        self.anims.clear_from(base);
        for (c, want, g) in &plan {
            let Some(b) = self.virt.builders.get(c).cloned() else {
                continue;
            };
            for idx in want.clone() {
                let before = self.doc.len();
                b(&mut self.doc, *c, idx);
                let (col, row) = (idx % g.cols.max(1), idx / g.cols.max(1));
                let at = [
                    g.ml + col as f32 * g.pitch_x,
                    g.mt + row as f32 * g.pitch_y,
                    g.item_w,
                    g.item_h,
                ];
                // Every direct child the builder added to the container is an item.
                for n in before..self.doc.len() {
                    if self.doc.nodes[n].parent == Some(*c) {
                        self.doc.nodes[n].virtual_item = true;
                        self.virt.pos.insert(n, at);
                        self.virt.index.insert(n, idx);
                    }
                }
            }
        }
        let f = crate::cascade::flags(&self.doc, &self.interaction);
        let mut cx = crate::cascade::Cx {
            sheet: &self.sheet,
            vp: self.viewport,
            budget: crate::cascade::Budget::unlimited(),
        };
        if self
            .st
            .style_range(&mut self.doc, &mut cx, &f, base)
            .is_err()
        {
            return false;
        }
        for (c, want, g) in plan {
            self.virt.info.insert(
                c,
                Info {
                    range: want,
                    grid: g,
                },
            );
        }
        self.tree_dirty = true;
        true
    }

    /// Scroll offset (page or container) that brings item `index` of container `c` into view; applies it.
    pub fn scroll_to_item(&mut self, c: Id, index: usize, measure: &mut Measure) {
        let Some(g) = self.virt.grid(c) else { return };
        let row = index / g.cols.max(1);
        let item_top = g.mt + row as f32 * g.pitch_y;
        let item_bot = item_top + g.item_h;
        let ov = self.st.styles[c].layout.overflow.y == taffy::style::Overflow::Scroll;
        let origin = self.vorigin.get(&c).map_or(self.rects[c][1], |o| o.1);
        if ov {
            let view = self.rects[c][3];
            let off = self.scroll.offsets.entry(c).or_default();
            if item_top < off[1] {
                off[1] = item_top;
            } else if item_bot > off[1] + view {
                off[1] = item_bot - view;
            }
        } else {
            let (py, vh) = (self.scroll.page_y, self.viewport.1);
            let (top, bot) = (origin + item_top, origin + item_bot);
            if top < py {
                self.scroll.page_y = top;
            } else if bot > py + vh {
                self.scroll.page_y = bot - vh;
            }
        }
        self.clamp_scroll();
        self.layout_all(measure);
    }
}
