//! Transitions: when a restyle changes a paint property that has a `transition`, the old value is eased into the new one over
//! the declared time with the declared curve. Only paint properties animate (opacity, transform, colours, backgrounds), never
//! layout. The app drives the clock: call [`Anims::tick`] once per frame while [`Anims::active`] is true.
use crate::doc::Id;
use crate::paint::{ALL_APROPS, AProp, AnimSpec, Ease, Fill, Stop, StopPos, Xf};
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

/// A `@keyframes` animation running on a node: the values it sets, per property, at their offsets (0..=1, with the element's own value standing in at the ends).
struct Run {
    spec: Rc<AnimSpec>,
    start: Instant,
    opacity: Vec<(f32, f32)>,
    xf: Vec<(f32, Xf)>,
    color: Vec<(f32, Color)>,
    /// A finite animation that has ended (kept only while `fill-mode: forwards` holds its last frame).
    done: bool,
}

#[derive(Default)]
pub struct Anims {
    by_node: HashMap<Id, Vec<Anim>>,
    runs: HashMap<Id, Run>,
    now: Option<Instant>,
}

static REDUCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Reduced motion: `@keyframes` animations are not started (the element shows its plain style).
pub fn set_reduced(on: bool) {
    REDUCED.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Frames of one property with the element's own value standing in at 0 and 1 when the keyframes do not set them.
fn ends<T: Copy>(mut v: Vec<(f32, T)>, base: T) -> Vec<(f32, T)> {
    if v.first().is_none_or(|f| f.0 > 0.0) {
        v.insert(0, (0.0, base));
    }
    if v.last().is_none_or(|f| f.0 < 1.0) {
        v.push((1.0, base));
    }
    v
}

/// The values of one property along the animation: linear between the surrounding offsets, the timing function applied per interval.
fn along<T: Copy>(
    list: &[(f32, T)],
    p: f32,
    ease: Ease,
    mix: impl Fn(T, T, f32) -> T,
) -> Option<T> {
    let (first, last) = (list.first()?, list.last()?);
    if p <= first.0 {
        return Some(first.1);
    }
    if p >= last.0 {
        return Some(last.1);
    }
    let k = list.iter().position(|e| e.0 > p)?;
    let (a, b) = (list[k - 1], list[k]);
    let t = if b.0 > a.0 {
        (p - a.0) / (b.0 - a.0)
    } else {
        1.0
    };
    Some(mix(a.1, b.1, ease.at(t)))
}

impl Run {
    fn elapsed_ms(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.start).as_secs_f32() * 1000.0
    }

    fn ends_ms(&self) -> f32 {
        self.spec.delay + self.spec.dur * self.spec.iters
    }

    /// Progress through the keyframes (0..=1) now, or `None` when the animation sets nothing (before its delay without `backwards`, after its end without `forwards`).
    fn progress(&self, now: Instant) -> Option<f32> {
        let a = &*self.spec;
        let ms = self.elapsed_ms(now) - a.delay;
        let (iter, t) = if ms < 0.0 {
            if !a.fill_back {
                return None;
            }
            (0.0, 0.0)
        } else if a.dur <= 0.0 || ms >= a.dur * a.iters {
            if !a.fill_fwd {
                return None;
            }
            let it = a.iters.max(1.0);
            let end = if it.fract() == 0.0 { 1.0 } else { it.fract() };
            ((it.ceil() - 1.0).max(0.0), end)
        } else {
            let x = ms / a.dur;
            (x.floor(), x.fract())
        };
        let rev = a.reverse ^ (a.alternate && iter % 2.0 == 1.0);
        Some(if rev { 1.0 - t } else { t })
    }
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
            Fill::Linear {
                angle: a1,
                stops: s1,
            },
            Fill::Linear {
                angle: a2,
                stops: s2,
            },
        ) if s1.len() == s2.len() => {
            let stops = s1
                .iter()
                .zip(s2)
                .map(|(p, q)| {
                    let pos = match (p.pos, q.pos) {
                        (Some(StopPos::Frac(x)), Some(StopPos::Frac(y))) => {
                            Some(StopPos::Frac(lerp(x, y, t)))
                        }
                        (Some(StopPos::Px(x)), Some(StopPos::Px(y))) => {
                            Some(StopPos::Px(lerp(x, y, t)))
                        }
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
            let v: Option<Vec<Fill>> = x
                .iter()
                .zip(y.iter())
                .map(|(p, q)| lerp_fill(p, q, t))
                .collect();
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
    /// Frames are needed: a transition or a keyframe animation is still moving.
    pub fn active(&self) -> bool {
        !self.by_node.is_empty() || self.runs.values().any(|r| !r.done)
    }

    /// Does anything override a style (a held `forwards` animation counts)?
    pub fn any(&self) -> bool {
        !self.by_node.is_empty() || !self.runs.is_empty()
    }

    /// Only endless keyframe animations are running: decorative, so 30 frames a second are enough.
    pub fn decorative_only(&self) -> bool {
        self.by_node.is_empty()
            && self.runs.values().any(|r| !r.done)
            && self
                .runs
                .values()
                .all(|r| r.done || r.spec.iters.is_infinite())
    }

    /// Start (or keep) the keyframe animation `spec` on `id`; `None` removes it. `frames` are (offset, the element's style with that frame's declarations applied, which properties the frame sets: opacity, transform, colour).
    pub fn set_run(
        &mut self,
        id: Id,
        spec: Option<&Rc<AnimSpec>>,
        frames: impl FnOnce() -> Vec<(f32, Eff)>,
        base: &Eff,
        now: Instant,
    ) {
        let Some(spec) = spec else {
            self.runs.remove(&id);
            return;
        };
        if REDUCED.load(std::sync::atomic::Ordering::Relaxed) {
            self.runs.remove(&id);
            return;
        }
        if self.runs.get(&id).is_some_and(|r| r.spec == *spec) {
            return;
        }
        let frames = frames();
        let col = |pick: &dyn Fn(&Eff) -> bool| frames.iter().any(|f| pick(&f.1));
        let (has_o, has_x, has_c) = (
            col(&|e| e.opacity.is_some()),
            col(&|e| e.xf.is_some()),
            col(&|e| e.color.is_some()),
        );
        let mut run = Run {
            spec: spec.clone(),
            start: now,
            opacity: vec![],
            xf: vec![],
            color: vec![],
            done: false,
        };
        if has_o {
            let v = frames
                .iter()
                .filter_map(|f| f.1.opacity.map(|o| (f.0, o)))
                .collect();
            run.opacity = ends(v, base.opacity.unwrap_or(1.0));
        }
        if has_x {
            let v = frames
                .iter()
                .filter_map(|f| f.1.xf.map(|o| (f.0, o)))
                .collect();
            run.xf = ends(v, base.xf.unwrap_or(Xf::IDENTITY));
        }
        if has_c {
            let v = frames
                .iter()
                .filter_map(|f| f.1.color.map(|o| (f.0, o)))
                .collect();
            if let Some(b) = base.color {
                run.color = ends(v, b);
            }
        }
        if run.opacity.is_empty() && run.xf.is_empty() && run.color.is_empty() {
            self.runs.remove(&id);
            return;
        }
        self.runs.insert(id, run);
        self.now.get_or_insert(now);
    }

    pub fn clear(&mut self) {
        self.by_node.clear();
        self.runs.clear();
    }

    /// Drop animations of nodes `from..` (they were removed).
    pub fn clear_from(&mut self, from: Id) {
        self.by_node.retain(|k, _| *k < from);
        self.runs.retain(|k, _| *k < from);
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
        for r in self.runs.values_mut() {
            if !r.done && r.spec.iters.is_finite() && r.elapsed_ms(now) >= r.ends_ms() {
                r.done = true;
            }
        }
        // A finished run that holds nothing is dropped.
        self.runs.retain(|_, r| !r.done || r.spec.fill_fwd);
        self.active()
    }

    fn sample(&self, a: &Anim) -> Option<AV> {
        let now = self.now?;
        let ms = now.saturating_duration_since(a.start).as_secs_f32() * 1000.0 - a.delay;
        let t = if a.dur <= 0.0 {
            1.0
        } else {
            (ms / a.dur).clamp(0.0, 1.0)
        };
        mix(&a.from, &a.to, a.ease.at(t))
    }

    /// The animated overrides of a node (empty when nothing runs on it).
    pub fn eff(&self, id: Id) -> Eff {
        let mut e = Eff::default();
        let (Some(now), Some(run)) = (self.now, self.runs.get(&id)) else {
            return self.eff_transitions(id, e);
        };
        e = self.eff_transitions(id, e);
        if let Some(p) = run.progress(now) {
            let ease = run.spec.ease;
            if !run.opacity.is_empty() {
                e.opacity = along(&run.opacity, p, ease, lerp);
            }
            if !run.xf.is_empty() {
                e.xf = along(&run.xf, p, ease, |a, b, t| a.lerp(&b, t));
            }
            if !run.color.is_empty() {
                e.color = along(&run.color, p, ease, lerp_c);
            }
        }
        e
    }

    fn eff_transitions(&self, id: Id, mut e: Eff) -> Eff {
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
                (
                    AProp::Opacity,
                    Eff {
                        opacity: Some(v), ..
                    },
                ) => AV::F(*v),
                (AProp::Transform, Eff { xf: Some(v), .. }) => AV::X(*v),
                (AProp::Color, Eff { color: Some(v), .. }) => AV::C(*v),
                (AProp::Background, Eff { bg: Some(v), .. }) => AV::Bg(v.clone()),
                (
                    AProp::BorderColor,
                    Eff {
                        border: Some(v), ..
                    },
                ) => AV::B(*v),
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
