//! Transitions: when a restyle changes a paint property that has a `transition`, the old value is eased into the new one over
//! the declared time with the declared curve. Only paint properties animate (opacity, transform, colours, backgrounds), never
//! layout. The app drives the clock: call [`Anims::tick`] once per frame while [`Anims::active`] is true.
use crate::doc::Id;
use crate::paint::{AProp, ALL_APROPS, Ease, Fill, Stop, StopPos, Xf};
use crate::props::Computed;
use nocturne_core::customize::value::Color;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum AV {
    F(f32),
    X(Xf),
    C(Color),
    Bg(Rc<[Fill]>),
    B([Color; 4]),
}

/// The animated values currently overriding a node's computed style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Eff {
    pub opacity: Option<f32>,
    pub xf: Option<Xf>,
    pub color: Option<Color>,
    pub bg: Option<Rc<[Fill]>>,
    pub border: Option<[Color; 4]>,
}

struct Anim {
    prop: AProp,
    from: AV,
    to: AV,
    start: Instant,
    dur: f32,
    delay: f32,
    ease: Ease,
}

#[derive(Default)]
pub struct Anims {
    by_node: HashMap<Id, Vec<Anim>>,
    now: Option<Instant>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_c(a: Color, b: Color, t: f32) -> Color {
    // Premultiplied-free straight lerp: matches how browsers blend a colour toward `transparent` closely enough.
    let l = |x: u8, y: u8| lerp(x as f32, y as f32, t).round().clamp(0.0, 255.0) as u8;
    Color::rgba(l(a.r, b.r), l(a.g, b.g), l(a.b, b.b), l(a.a, b.a))
}

fn lerp_fill(a: &Fill, b: &Fill, t: f32) -> Option<Fill> {
    match (a, b) {
        (Fill::Solid(x), Fill::Solid(y)) => Some(Fill::Solid(lerp_c(*x, *y, t))),
        (
            Fill::Linear { angle: a1, stops: s1 },
            Fill::Linear { angle: a2, stops: s2 },
        ) if s1.len() == s2.len() => {
            let stops = s1
                .iter()
                .zip(s2)
                .map(|(p, q)| {
                    let pos = match (p.pos, q.pos) {
                        (Some(StopPos::Frac(x)), Some(StopPos::Frac(y))) => Some(StopPos::Frac(lerp(x, y, t))),
                        (Some(StopPos::Px(x)), Some(StopPos::Px(y))) => Some(StopPos::Px(lerp(x, y, t))),
                        (x, y) if x == y => x,
                        _ => return None,
                    };
                    Some(Stop {
                        color: lerp_c(p.color, q.color, t),
                        pos,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Fill::Linear {
                angle: lerp(*a1, *a2, t),
                stops,
            })
        }
        _ => None,
    }
}

fn mix(a: &AV, b: &AV, t: f32) -> Option<AV> {
    Some(match (a, b) {
        (AV::F(x), AV::F(y)) => AV::F(lerp(*x, *y, t)),
        (AV::X(x), AV::X(y)) => AV::X(x.lerp(y, t)),
        (AV::C(x), AV::C(y)) => AV::C(lerp_c(*x, *y, t)),
        (AV::B(x), AV::B(y)) => AV::B([0, 1, 2, 3].map(|i| lerp_c(x[i], y[i], t))),
        (AV::Bg(x), AV::Bg(y)) if x.len() == y.len() => {
            let v: Option<Vec<Fill>> = x.iter().zip(y.iter()).map(|(p, q)| lerp_fill(p, q, t)).collect();
            AV::Bg(v?.into())
        }
        _ => return None,
    })
}

fn value_of(c: &Computed, p: AProp) -> AV {
    match p {
        AProp::Opacity => AV::F(c.opacity),
        AProp::Transform => AV::X(c.xf),
        AProp::Color => AV::C(c.color),
        AProp::Background => AV::Bg(c.bg.clone()),
        AProp::BorderColor => AV::B(c.border_color),
    }
}

impl Anims {
    pub fn active(&self) -> bool {
        !self.by_node.is_empty()
    }

    pub fn clear(&mut self) {
        self.by_node.clear();
    }

    /// Drop animations of nodes `from..` (they were removed).
    pub fn clear_from(&mut self, from: Id) {
        self.by_node.retain(|k, _| *k < from);
    }

    pub fn forget(&mut self, id: Id) {
        self.by_node.remove(&id);
    }

    /// Advance the clock; finished animations are dropped. Returns whether any is still running.
    pub fn tick(&mut self, now: Instant) -> bool {
        self.now = Some(now);
        self.by_node.retain(|_, v| {
            v.retain(|a| {
                (now.saturating_duration_since(a.start).as_secs_f32() * 1000.0) < a.delay + a.dur
            });
            !v.is_empty()
        });
        self.active()
    }

    fn sample(&self, a: &Anim) -> Option<AV> {
        let now = self.now?;
        let ms = now.saturating_duration_since(a.start).as_secs_f32() * 1000.0 - a.delay;
        let t = if a.dur <= 0.0 { 1.0 } else { (ms / a.dur).clamp(0.0, 1.0) };
        mix(&a.from, &a.to, a.ease.at(t))
    }

    /// The animated overrides of a node (empty when nothing runs on it).
    pub fn eff(&self, id: Id) -> Eff {
        let mut e = Eff::default();
        let Some(list) = self.by_node.get(&id) else {
            return e;
        };
        for a in list {
            match (a.prop, self.sample(a)) {
                (AProp::Opacity, Some(AV::F(v))) => e.opacity = Some(v),
                (AProp::Transform, Some(AV::X(v))) => e.xf = Some(v),
                (AProp::Color, Some(AV::C(v))) => e.color = Some(v),
                (AProp::Background, Some(AV::Bg(v))) => e.bg = Some(v),
                (AProp::BorderColor, Some(AV::B(v))) => e.border = Some(v),
                _ => {}
            }
        }
        e
    }

    /// The node's style went from `old` to `new`: start a transition for each animatable property that changed and has one.
    pub fn changed(&mut self, id: Id, old: &Computed, new: &Computed, now: Instant) {
        if new.trans.is_none() {
            // A removed `transition` ends running ones at their target.
            self.by_node.remove(&id);
            return;
        }
        let current = self.eff(id);
        for p in ALL_APROPS {
            let (a, b) = (value_of(old, p), value_of(new, p));
            // A running animation of this property determines where the new one starts.
            let from = match (p, &current) {
                (AProp::Opacity, Eff { opacity: Some(v), .. }) => AV::F(*v),
                (AProp::Transform, Eff { xf: Some(v), .. }) => AV::X(*v),
                (AProp::Color, Eff { color: Some(v), .. }) => AV::C(*v),
                (AProp::Background, Eff { bg: Some(v), .. }) => AV::Bg(v.clone()),
                (AProp::BorderColor, Eff { border: Some(v), .. }) => AV::B(*v),
                _ => a.clone(),
            };
            if a == b {
                continue;
            }
            let Some((dur, delay, ease)) = new.trans.for_prop(p) else {
                // No transition for this property: drop any running one so the new value shows at once.
                if let Some(l) = self.by_node.get_mut(&id) {
                    l.retain(|x| x.prop != p);
                }
                continue;
            };
            // Only values that can be blended animate (matching gradient shapes, numbers, colours).
            if mix(&from, &b, 0.5).is_none() {
                continue;
            }
            let list = self.by_node.entry(id).or_default();
            list.retain(|x| x.prop != p);
            list.push(Anim {
                prop: p,
                from,
                to: b,
                start: now,
                dur,
                delay,
                ease,
            });
        }
        if self.by_node.get(&id).is_some_and(Vec::is_empty) {
            self.by_node.remove(&id);
        }
        self.now.get_or_insert(now);
    }

    /// Time until the longest running animation ends (for scheduling a last redraw).
    pub fn remaining(&self) -> Duration {
        let Some(now) = self.now else {
            return Duration::ZERO;
        };
        self.by_node
            .values()
            .flatten()
            .map(|a| {
                let done = now.saturating_duration_since(a.start).as_secs_f32() * 1000.0;
                Duration::from_secs_f32(((a.delay + a.dur - done).max(0.0)) / 1000.0)
            })
            .max()
            .unwrap_or_default()
    }
}
