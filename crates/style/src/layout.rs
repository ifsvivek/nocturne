//! Taffy layout of the styled tree. The tree is rebuilt on a state change and recomputed (not rebuilt) on a resize; nothing
//! here runs per frame. Text leaves are measured through a caller-supplied function, so the app can use the real text shaper.
use crate::cascade::{self, Report, Sheet};
use crate::doc::{Doc, Id};
use crate::props::{Computed, Ctx, Transform};
use std::borrow::Cow;
use std::time::{Duration, Instant};
use taffy::geometry::Size;
use taffy::prelude::TaffyMaxContent;
use taffy::style::{AvailableSpace, Dimension, Display};
use taffy::{NodeId, TaffyTree};

#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub size: f32,
    pub weight: u16,
    pub line_height: f32,
    pub lines: Option<u16>,
}

/// `(text, style, max width)`: `None` is "as wide as it wants", `Some(0.0)` is min-content. Returns the size in px.
pub type Measure<'a> = dyn FnMut(&str, &TextStyle, Option<f32>) -> (f32, f32) + 'a;

/// A cheap stand-in for a text shaper: used by tests and benchmarks.
pub fn approx(text: &str, st: &TextStyle, max: Option<f32>) -> (f32, f32) {
    let cw = st.size * if st.weight >= 600 { 0.56 } else { 0.5 };
    let (mut widest, mut line, mut lines) = (0.0f32, 0.0f32, 1u32);
    for w in text.split_whitespace() {
        let ww = w.chars().count() as f32 * cw;
        let sp = if line > 0.0 { cw } else { 0.0 };
        if let Some(m) = max
            && line > 0.0
            && line + sp + ww > m
        {
            widest = widest.max(line);
            line = ww;
            lines += 1;
        } else {
            line += sp + ww;
        }
    }
    widest = widest.max(line);
    if let Some(l) = st.lines {
        lines = lines.min(l as u32);
    }
    (widest, lines as f32 * st.size * st.line_height)
}

/// The page is a document: as wide as the window, at least as tall, as tall as its content otherwise (the widget scrolls it).
fn root_size(st: &mut taffy::Style, vp: (f32, f32)) {
    st.size = Size {
        width: Dimension::length(vp.0),
        height: Dimension::auto(),
    };
    st.min_size.height = taffy::style::LengthPercentageAuto::length(vp.1);
}

pub fn shown<'a>(text: &'a str, c: &Computed) -> Cow<'a, str> {
    match c.text_transform {
        Transform::None => Cow::Borrowed(text),
        Transform::Upper => Cow::Owned(text.to_uppercase()),
        Transform::Lower => Cow::Owned(text.to_lowercase()),
    }
}

pub fn text_style(c: &Computed) -> TextStyle {
    TextStyle {
        size: c.font_size,
        weight: c.weight,
        line_height: c.line_height,
        lines: c.lines,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    pub cascade: Duration,
    pub layout: Duration,
}

pub struct Scene {
    pub styles: Vec<Computed>,
    /// Border box in the page's coordinates, before any scrolling: x, y, width, height.
    pub rects: Vec<[f32; 4]>,
    /// Scrollable content size (for `overflow: scroll`).
    pub content: Vec<[f32; 2]>,
    /// Border plus padding per side (left, top, right, bottom): where the content box starts.
    pub insets: Vec<[f32; 4]>,
    /// Border widths per side (left, top, right, bottom).
    pub borders: Vec<[f32; 4]>,
    /// Children in visual order (`order`, then source order): painting, hit testing and focus all use this.
    pub kids: Vec<Vec<Id>>,
    /// The same children in paint order (`z-index`, stable).
    pub paint: Vec<Vec<Id>>,
    pub report: Report,
    pub timing: Timing,
    tree: TaffyTree<Id>,
    nodes: Vec<NodeId>,
    root: NodeId,
}

fn run(
    tree: &mut TaffyTree<Id>,
    root: NodeId,
    doc: &Doc,
    styles: &[Computed],
    measure: &mut Measure,
) {
    let _ = tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |inputs, _, ctx, style| {
        taffy::compute_leaf_layout(
            inputs,
            style,
            |_, _| 0.0,
            |known, avail| {
                let Some(&mut i) = ctx else {
                    return Size::ZERO;
                };
                let (Some(t), c) = (&doc.nodes[i].text, &styles[i]) else {
                    return Size::ZERO;
                };
                // `known` is the border box (Taffy applies it itself); `avail` is already the content box, so wrap to that.
                let _ = known;
                let w = match avail.width {
                    AvailableSpace::Definite(w) => Some(w),
                    AvailableSpace::MinContent => Some(0.0),
                    AvailableSpace::MaxContent => None,
                };
                let (tw, th) = measure(&shown(t, c), &text_style(c), w);
                Size {
                    width: tw.ceil(),
                    height: th.ceil(),
                }
            },
        )
    });
}

impl Scene {
    pub fn build(
        doc: &Doc,
        sheet: &Sheet,
        flags: &[u8],
        viewport: (f32, f32),
        measure: &mut Measure,
    ) -> Scene {
        let t0 = Instant::now();
        let cx = Ctx {
            vw: viewport.0,
            vh: viewport.1,
        };
        let mut report = Report::default();
        let styles = cascade::style_tree(doc, sheet, flags, cx, &Computed::root(), &mut report);
        let cascade_t = t0.elapsed();

        let t1 = Instant::now();
        let mut tree: TaffyTree<Id> = TaffyTree::with_capacity(doc.len());
        let nodes: Vec<NodeId> = (0..doc.len())
            .map(|i| {
                let mut st = styles[i].layout.clone();
                if i == 0 {
                    root_size(&mut st, viewport);
                }
                if doc.nodes[i].text.is_some() {
                    tree.new_leaf_with_context(st, i)
                } else {
                    tree.new_leaf(st)
                }
                .expect("taffy node")
            })
            .collect();
        let kids: Vec<Vec<Id>> = doc
            .nodes
            .iter()
            .map(|n| {
                let mut k = n.children.clone();
                k.sort_by_key(|&c| styles[c].order);
                k
            })
            .collect();
        for (i, k) in kids.iter().enumerate() {
            if !k.is_empty() {
                let ids: Vec<NodeId> = k.iter().map(|&c| nodes[c]).collect();
                tree.set_children(nodes[i], &ids).expect("children");
            }
        }
        let paint = kids
            .iter()
            .map(|k| {
                let mut p = k.clone();
                p.sort_by_key(|&c| styles[c].z);
                p
            })
            .collect();
        let mut s = Scene {
            paint,
            rects: vec![[0.0; 4]; doc.len()],
            content: vec![[0.0; 2]; doc.len()],
            insets: vec![[0.0; 4]; doc.len()],
            borders: vec![[0.0; 4]; doc.len()],
            kids,
            report,
            timing: Timing::default(),
            root: nodes[0],
            nodes,
            tree,
            styles,
        };
        run(&mut s.tree, s.root, doc, &s.styles, measure);
        s.collect(doc);
        s.timing = Timing {
            cascade: cascade_t,
            layout: t1.elapsed(),
        };
        s
    }

    /// New window size: only layout runs again (cascade results do not depend on the viewport except for `vh`/`vw` lengths,
    /// so a theme that uses them rebuilds through `build`).
    pub fn resize(&mut self, doc: &Doc, viewport: (f32, f32), measure: &mut Measure) -> Duration {
        let t = Instant::now();
        let mut st = self.tree.style(self.root).expect("root").clone();
        root_size(&mut st, viewport);
        self.tree.set_style(self.root, st).expect("set_style");
        // Text wraps to the new widths, so every measured leaf is dirty.
        for (i, n) in doc.nodes.iter().enumerate() {
            if n.text.is_some() {
                let _ = self.tree.mark_dirty(self.nodes[i]);
            }
        }
        run(&mut self.tree, self.root, doc, &self.styles, measure);
        self.collect(doc);
        t.elapsed()
    }

    fn collect(&mut self, doc: &Doc) {
        // Parents are stored before children, so one forward pass turns local locations into page coordinates.
        for i in 0..self.nodes.len() {
            let l = self.tree.layout(self.nodes[i]).expect("layout");
            let (px, py) = match doc.nodes[i].parent {
                Some(p) => (self.rects[p][0], self.rects[p][1]),
                None => (0.0, 0.0),
            };
            self.rects[i] = [
                px + l.location.x,
                py + l.location.y,
                l.size.width,
                l.size.height,
            ];
            self.content[i] = [l.scroll_width(), l.scroll_height()];
            self.borders[i] = [l.border.left, l.border.top, l.border.right, l.border.bottom];
            self.insets[i] = [
                l.border.left + l.padding.left,
                l.border.top + l.padding.top,
                l.border.right + l.padding.right,
                l.border.bottom + l.padding.bottom,
            ];
        }
    }

    pub fn is_hidden(&self, i: Id) -> bool {
        self.styles[i].layout.display == Display::None
    }
}
