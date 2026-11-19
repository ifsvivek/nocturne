//! Scrolling, hit testing, focus order and spatial (arrow-key) navigation, all computed from the laid-out scene so they follow
//! whatever a theme did to the layout. Which elements can take focus is the app's decision (`Node::focusable`), never CSS.
use crate::doc::Id;
use crate::layout::Scene;
use crate::text::Measure;
use taffy::style::{Display, FlexDirection, Overflow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// A rectangle as `[x, y, w, h]`.
type R = [f32; 4];

impl Scene {
    // ---- scrolling ----

    pub fn clamp_scroll(&mut self) {
        let max = (self.rects[0][3] - self.viewport.1).max(0.0);
        self.scroll.page_y = self.scroll.page_y.clamp(0.0, max);
        let offs: Vec<Id> = self.scroll.offsets.keys().copied().collect();
        for n in offs {
            if n >= self.rects.len() {
                self.scroll.offsets.remove(&n);
                continue;
            }
            let [_, _, w, h] = self.rects[n];
            let [cw, ch] = self.content[n];
            if let Some(o) = self.scroll.offsets.get_mut(&n) {
                o[0] = o[0].clamp(0.0, (cw - w).max(0.0));
                o[1] = o[1].clamp(0.0, (ch - h).max(0.0));
            }
        }
    }

    /// Jump the page to scroll offset `y` (clamped); virtual lists follow.
    pub fn set_page_scroll(&mut self, y: f32, measure: &mut Measure) {
        self.scroll.page_y = y;
        self.clamp_scroll();
        if !self.virt.containers().is_empty() {
            self.layout_all(measure);
        }
    }

    /// A wheel or touchpad scroll over `target`: the innermost scroll container that can move on that axis takes it, else the
    /// page. Returns whether anything moved.
    pub fn wheel(&mut self, target: Option<Id>, dx: f32, dy: f32, measure: &mut Measure) -> bool {
        let before = (self.scroll.page_y, self.scroll.offsets.clone());
        let mut at = target;
        let mut done = false;
        while let Some(i) = at {
            let st = &self.style(i).layout;
            let (ox, oy) = (
                st.overflow.x == Overflow::Scroll,
                st.overflow.y == Overflow::Scroll,
            );
            if i != 0 {
                let [_, _, w, h] = self.rects[i];
                let [cw, ch] = self.content[i];
                let can_x = ox && dx != 0.0 && cw > w + 0.5;
                let can_y = oy && dy != 0.0 && ch > h + 0.5;
                if can_x || can_y {
                    let o = self.scroll.offsets.entry(i).or_default();
                    if can_x {
                        o[0] -= dx;
                    }
                    if can_y {
                        o[1] -= dy;
                    }
                    done = true;
                    break;
                }
            }
            at = self.doc.nodes[i].parent;
        }
        if !done {
            self.scroll.page_y -= dy;
        }
        self.clamp_scroll();
        let moved = before.0 != self.scroll.page_y || before.1 != self.scroll.offsets;
        if moved && !self.virt.containers().is_empty() {
            self.layout_all(measure);
        }
        moved
    }

    /// Scroll the page and the containers so that `i` is inside the window.
    pub fn reveal(&mut self, i: Id, measure: &mut Measure) {
        let [x, y, w, h] = self.rects[i];
        let _ = (x, w);
        if self.style(i).fixed {
            return;
        }
        // Containers first (innermost), then the page.
        let mut at = self.doc.nodes[i].parent;
        while let Some(a) = at {
            if a != 0 && self.style(a).layout.overflow.y == Overflow::Scroll {
                let [_, ay, _, ah] = self.rects[a];
                let off = self.scroll.offsets.entry(a).or_default();
                let top = y - ay;
                if top < off[1] {
                    off[1] = top.max(0.0);
                } else if top + h > off[1] + ah {
                    off[1] = top + h - ah;
                }
            }
            at = self.doc.nodes[a].parent;
        }
        let on_page = self.nav_rect(i);
        let (py, vh) = (self.scroll.page_y, self.viewport.1);
        if on_page[1] < py {
            self.scroll.page_y = on_page[1];
        } else if on_page[1] + on_page[3] > py + vh {
            self.scroll.page_y = on_page[1] + on_page[3] - vh;
        }
        self.clamp_scroll();
        if !self.virt.containers().is_empty() {
            self.layout_all(measure);
        }
    }

    // ---- hit testing ----

    /// The topmost element under `(x, y)` in window coordinates, honouring scroll offsets, clipping, `position: fixed`,
    /// `visibility` and `pointer-events`.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<Id> {
        let clip = [0.0, 0.0, self.viewport.0, self.viewport.1];
        self.hit(0, (x, y), (0.0, -self.scroll.page_y), clip)
    }

    fn hit(&self, n: Id, p: (f32, f32), sh: (f32, f32), clip: R) -> Option<Id> {
        let c = self.style(n);
        if c.layout.display == Display::None {
            return None;
        }
        // Fixed elements ignore the page and container scroll.
        let sh = if c.fixed { (0.0, 0.0) } else { sh };
        let [x, y, w, h] = self.rects[n];
        let rect = [x + sh.0, y + sh.1, w, h];
        let clips =
            c.layout.overflow.x != Overflow::Visible || c.layout.overflow.y != Overflow::Visible;
        let inner = if clips { intersect(clip, rect)? } else { clip };
        let off = self.scroll.offsets.get(&n).copied().unwrap_or_default();
        let child_sh = (sh.0 - off[0], sh.1 - off[1]);
        for &k in self.paint[n].iter().rev() {
            if let Some(f) = self.hit(k, p, child_sh, inner) {
                return Some(f);
            }
        }
        (c.visible && c.pointer_events && contains(clip, p) && contains(rect, p)).then_some(n)
    }

    // ---- focus ----

    fn shown(&self, i: Id) -> bool {
        let c = self.style(i);
        c.layout.display != Display::None && c.visible
    }

    /// Focusable elements in visual order: depth first over the children as laid out (`order`, `*-reverse`), not source order.
    pub fn focus_order(&self) -> Vec<Id> {
        let mut out = vec![];
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            if self.style(n).layout.display == Display::None {
                continue;
            }
            if self.doc.nodes[n].focusable && self.shown(n) {
                out.push(n);
            }
            let rev = matches!(
                self.style(n).layout.flex_direction,
                FlexDirection::RowReverse | FlexDirection::ColumnReverse
            ) && self.style(n).layout.display == Display::Flex;
            let kids = &self.kids[n];
            // The stack pops from the end, so push in reverse of the wanted visit order.
            if rev {
                stack.extend(kids.iter().copied());
            } else {
                stack.extend(kids.iter().rev().copied());
            }
        }
        out
    }

    /// Tab: the next (or previous) focusable element after `from`, wrapping around.
    pub fn focus_next(&self, from: Option<Id>, forward: bool) -> Option<Id> {
        let order = self.focus_order();
        if order.is_empty() {
            return None;
        }
        let pos = from.and_then(|f| order.iter().position(|&x| x == f));
        Some(match (pos, forward) {
            (None, true) => order[0],
            (None, false) => order[order.len() - 1],
            (Some(p), true) => order[(p + 1) % order.len()],
            (Some(p), false) => order[(p + order.len() - 1) % order.len()],
        })
    }

    /// Rectangle in page space: scroll offsets of containers removed, fixed elements moved by the page scroll.
    pub fn nav_rect(&self, i: Id) -> R {
        let [x, y, w, h] = self.rects[i];
        let (mut dx, mut dy) = (0.0, 0.0);
        if self.style(i).fixed {
            return [x, y + self.scroll.page_y, w, h];
        }
        let mut up = self.doc.nodes[i].parent;
        while let Some(a) = up {
            if let Some(o) = self.scroll.offsets.get(&a) {
                dx -= o[0];
                dy -= o[1];
            }
            if self.style(a).fixed {
                return [x + dx, y + dy + self.scroll.page_y, w, h];
            }
            up = self.doc.nodes[a].parent;
        }
        [x + dx, y + dy, w, h]
    }

    /// Rectangle in window coordinates (what is on screen right now), before clipping.
    pub fn screen_rect(&self, i: Id) -> R {
        let r = self.nav_rect(i);
        // `nav_rect` puts fixed elements in page space too, so one shift covers both.
        [r[0], r[1] - self.scroll.page_y, r[2], r[3]]
    }

    /// The part of the window where `i` can show: the window itself cut by every clipping ancestor; `None` when nothing of it
    /// can be seen.
    pub fn clip_of(&self, i: Id) -> Option<R> {
        let mut clip = [0.0, 0.0, self.viewport.0, self.viewport.1];
        let mut up = self.doc.nodes[i].parent;
        while let Some(a) = up {
            let c = self.style(a);
            if c.layout.overflow.x != Overflow::Visible || c.layout.overflow.y != Overflow::Visible
            {
                clip = intersect(clip, self.screen_rect(a))?;
            }
            up = self.doc.nodes[a].parent;
        }
        Some(clip)
    }

    /// Arrow keys: the nearest focusable element in that direction. A candidate that overlaps `from` on the other axis (same row
    /// or column) wins over one that is only in the cone; ties go to the closer one.
    pub fn spatial(&self, from: Id, dir: Dir) -> Option<Id> {
        let a = self.nav_rect(from);
        let (acx, acy) = (a[0] + a[2] / 2.0, a[1] + a[3] / 2.0);
        let horizontal = matches!(dir, Dir::Left | Dir::Right);
        // (primary gap, other-axis centre distance, overlaps the other axis)
        let mut best: Option<(bool, f32, f32, Id)> = None;
        for c in self.focus_order() {
            if c == from {
                continue;
            }
            let b = self.nav_rect(c);
            let (bcx, bcy) = (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0);
            // Primary-axis distance from the edge of `from` to the facing edge of the candidate.
            let (gap, along, other) = match dir {
                Dir::Right => (b[0] - (a[0] + a[2]), bcx - acx, (bcy - acy).abs()),
                Dir::Left => (a[0] - (b[0] + b[2]), acx - bcx, (bcy - acy).abs()),
                Dir::Down => (b[1] - (a[1] + a[3]), bcy - acy, (bcx - acx).abs()),
                Dir::Up => (a[1] - (b[1] + b[3]), acy - bcy, (bcx - acx).abs()),
            };
            if along <= 0.0 {
                continue;
            }
            let overlap = if horizontal {
                a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
            } else {
                a[0] < b[0] + b[2] && b[0] < a[0] + a[2]
            };
            if !overlap && other > along {
                continue; // outside the 45 degree cone
            }
            let gap = gap.max(0.0);
            let score = if overlap { gap } else { gap + 2.0 * other };
            let cand = (!overlap, score, other, c);
            if best.is_none_or(|b| (cand.0, cand.1, cand.2) < (b.0, b.1, b.2)) {
                best = Some(cand);
            }
        }
        best.map(|b| b.3)
    }
}

fn contains(r: R, p: (f32, f32)) -> bool {
    p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3]
}

fn intersect(a: R, b: R) -> Option<R> {
    let (x0, y0) = (a[0].max(b[0]), a[1].max(b[1]));
    let (x1, y1) = (
        (a[0] + a[2]).min(b[0] + b[2]),
        (a[1] + a[3]).min(b[1] + b[3]),
    );
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}
