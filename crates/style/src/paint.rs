//! Paint-side value types: layered backgrounds with linear gradients (own parser: many stops, `px` stops, double-position
//! stops, hex with alpha), shadows (outer, inset, spread), 2D transforms, `object-fit`, easing and transition specs.
use nocturne_core::customize::value::{self, Color, split_top};

/// Iced draws at most this many gradient stops.
pub const MAX_STOPS: usize = 8;
/// Background layers kept per element.
pub const MAX_LAYERS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StopPos {
    Frac(f32),
    /// Length along the gradient line; resolved against the box at paint time.
    Px(f32),
    /// `vw`/`vh` lengths: turned into `Px` when the style is computed (the window is known then).
    Vw(f32),
    Vh(f32),
}

impl StopPos {
    fn at(self, vw: f32, vh: f32) -> Self {
        match self {
            Self::Vw(v) => Self::Px(v * vw / 100.0),
            Self::Vh(v) => Self::Px(v * vh / 100.0),
            o => o,
        }
    }
}

/// The fills with their `vw`/`vh` stop positions in pixels for a window of `vw` x `vh`.
pub fn at_viewport(fills: &[Fill], vw: f32, vh: f32) -> Vec<Fill> {
    fills
        .iter()
        .map(|f| match f {
            Fill::Linear { angle, stops } if f.uses_viewport() => Fill::Linear {
                angle: *angle,
                stops: stops
                    .iter()
                    .map(|s| Stop {
                        pos: s.pos.map(|p| p.at(vw, vh)),
                        ..*s
                    })
                    .collect(),
            },
            o => o.clone(),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    pub color: Color,
    pub pos: Option<StopPos>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    Solid(Color),
    /// CSS angle in degrees (0 = towards the top, 90 = right, 180 = bottom).
    Linear {
        angle: f32,
        stops: Vec<Stop>,
    },
    /// Radial and repeating gradients: Iced draws only plain linear ones, so the app rasterises these once per box size.
    Raster(std::sync::Arc<Raster>),
}

/// How far a radial gradient reaches (CSS `<radial-extent>` or explicit radii).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extent {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    /// `circle 40px` (second value unused) or `ellipse 30% 50%`.
    Len(StopPos, StopPos),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RKind {
    /// `repeating-linear-gradient` (the plain one stays [`Fill::Linear`]).
    Linear { angle: f32 },
    Radial {
        circle: bool,
        extent: Extent,
        /// Centre as a fraction of the box or pixels from its left/top.
        at: (StopPos, StopPos),
    },
}

#[derive(Debug, PartialEq)]
pub struct Raster {
    pub kind: RKind,
    pub repeat: bool,
    pub stops: Vec<Stop>,
    /// Cache key: a hash of everything above.
    pub id: u64,
}

impl Raster {
    fn fill(kind: RKind, repeat: bool, stops: Vec<Stop>) -> Fill {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{kind:?}{repeat}{stops:?}").hash(&mut h);
        Fill::Raster(std::sync::Arc::new(Raster {
            kind,
            repeat,
            stops,
            id: h.finish(),
        }))
    }
}

impl Fill {
    pub fn uses_viewport(&self) -> bool {
        matches!(self, Fill::Linear { stops, .. } if stops.iter().any(|s| matches!(s.pos, Some(StopPos::Vw(_) | StopPos::Vh(_)))))
    }
}

fn num(s: &str) -> Option<f32> {
    let s = s.trim();
    s.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn pos(t: &str) -> Result<StopPos, String> {
    if let Some(p) = t.strip_suffix('%').and_then(num) {
        return Ok(StopPos::Frac(p / 100.0));
    }
    if let Some(p) = t.strip_suffix("px").and_then(num) {
        return Ok(StopPos::Px(p));
    }
    if let Some(p) = t.strip_suffix("vh").and_then(num) {
        return Ok(StopPos::Vh(p));
    }
    if let Some(p) = t.strip_suffix("vw").and_then(num) {
        return Ok(StopPos::Vw(p));
    }
    if t == "0" {
        return Ok(StopPos::Px(0.0));
    }
    Err(format!("`{t}`: gradient stop positions must be % or px"))
}

fn angle(first: &str) -> Option<Result<f32, String>> {
    let f = first.trim().to_ascii_lowercase();
    if let Some(d) = f.strip_suffix("deg").and_then(num) {
        return Some(Ok(d.rem_euclid(360.0)));
    }
    if let Some(t) = f.strip_suffix("turn").and_then(num) {
        return Some(Ok((t * 360.0).rem_euclid(360.0)));
    }
    if let Some(r) = f.strip_suffix("rad").and_then(num) {
        return Some(Ok(r.to_degrees().rem_euclid(360.0)));
    }
    if f == "0" {
        return Some(Ok(0.0));
    }
    if let Some(rest) = f.strip_prefix("to ") {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let has = |w: &str| words.contains(&w);
        let v = if has("top") {
            Some(true)
        } else if has("bottom") {
            Some(false)
        } else {
            None
        };
        let h = if has("right") {
            Some(true)
        } else if has("left") {
            Some(false)
        } else {
            None
        };
        return Some(match (v, h) {
            (Some(true), None) => Ok(0.0),
            (Some(true), Some(true)) => Ok(45.0),
            (None, Some(true)) => Ok(90.0),
            (Some(false), Some(true)) => Ok(135.0),
            (Some(false), None) => Ok(180.0),
            (Some(false), Some(false)) => Ok(225.0),
            (None, Some(false)) => Ok(270.0),
            (Some(true), Some(false)) => Ok(315.0),
            (None, None) => Err("unrecognised gradient direction".into()),
        });
    }
    None
}

fn gradient(args: &str) -> Result<Fill, String> {
    let mut parts = split_top(args, Some(','));
    let mut ang = 180.0;
    if let Some(a) = parts.first().and_then(|f| angle(f)) {
        ang = a?;
        parts.remove(0);
    }
    let stops = stop_list(parts)?;
    Ok(Fill::Linear { angle: ang, stops })
}

/// The colour stops of a gradient (everything after the direction / shape part).
fn stop_list(parts: Vec<&str>) -> Result<Vec<Stop>, String> {
    let mut stops: Vec<Stop> = vec![];
    // A colour hint (`a, 17vh, b`) moves the middle of the transition; Iced has no such stop, so it becomes a stop at that position with the middle colour (the curve through the same midpoint, drawn linearly on both sides).
    let mut hints: Vec<(usize, StopPos)> = vec![];
    for p in parts {
        let toks = split_top(p, None);
        let first = toks.first().ok_or("empty gradient stop")?;
        let Ok(c) = value::color(first) else {
            match (toks.len(), pos(first)) {
                (1, Ok(h)) if !stops.is_empty() => {
                    hints.push((stops.len(), h));
                    continue;
                }
                (1, Err(e)) => return Err(e),
                _ => return Err("colour hints in gradients are not supported".into()),
            }
        };
        // `color 0% 58%` is two stops of the same colour.
        match toks.len() {
            1 => stops.push(Stop {
                color: c,
                pos: None,
            }),
            2 | 3 => {
                for t in &toks[1..] {
                    stops.push(Stop {
                        color: c,
                        pos: Some(pos(t)?),
                    });
                }
            }
            _ => return Err(format!("`{p}` is not a gradient stop")),
        }
    }
    for (at, h) in hints.into_iter().rev() {
        let (Some(a), Some(b)) = (stops.get(at - 1), stops.get(at)) else {
            return Err("a colour hint needs a stop on both sides".into());
        };
        let mid = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
        let color = Color::rgba(
            mid(a.color.r, b.color.r),
            mid(a.color.g, b.color.g),
            mid(a.color.b, b.color.b),
            mid(a.color.a, b.color.a),
        );
        stops.insert(
            at,
            Stop {
                color,
                pos: Some(h),
            },
        );
    }
    if stops.len() < 2 {
        return Err("a gradient needs at least two stops".into());
    }
    Ok(stops)
}

fn center(t: &str) -> Option<(StopPos, bool)> {
    // (position, is a vertical-only keyword)
    Some(match t {
        "left" => (StopPos::Frac(0.0), false),
        "right" => (StopPos::Frac(1.0), false),
        "top" => (StopPos::Frac(0.0), true),
        "bottom" => (StopPos::Frac(1.0), true),
        "center" => (StopPos::Frac(0.5), false),
        _ => (pos(t).ok()?, false),
    })
}

/// `[circle | ellipse] [<extent> | <length>{1,2}] [at <position>]`; `None` when the first part is a colour stop.
fn radial_prelude(first: &str) -> Option<(bool, Extent, (StopPos, StopPos))> {
    let toks = split_top(first, None);
    if value::color(toks.first()?).is_ok() {
        return None;
    }
    let (mut circle, mut extent, mut lens) = (false, Extent::FarthestCorner, vec![]);
    let mut at = vec![];
    let mut in_at = false;
    for t in toks {
        match t {
            "circle" => circle = true,
            "ellipse" => circle = false,
            "closest-side" => extent = Extent::ClosestSide,
            "farthest-side" => extent = Extent::FarthestSide,
            "closest-corner" => extent = Extent::ClosestCorner,
            "farthest-corner" => extent = Extent::FarthestCorner,
            "at" => in_at = true,
            _ if in_at => at.push(center(t)?),
            _ => lens.push(pos(t).ok()?),
        }
    }
    if let Some(&a) = lens.first() {
        extent = Extent::Len(a, *lens.get(1).unwrap_or(&a));
    }
    let c = StopPos::Frac(0.5);
    let at = match at.as_slice() {
        [] => (c, c),
        [(x, true)] => (c, *x),
        [(x, _)] => (*x, c),
        [(a, true), (b, _)] => (*b, *a),
        [(a, _), (b, _)] => (*a, *b),
        _ => (c, c),
    };
    Some((circle, extent, at))
}

fn radial(args: &str, repeat: bool) -> Result<Fill, String> {
    let mut parts = split_top(args, Some(','));
    let (circle, extent, at) = match parts.first().and_then(|f| radial_prelude(f)) {
        Some(p) => {
            parts.remove(0);
            p
        }
        None => (
            false,
            Extent::FarthestCorner,
            (StopPos::Frac(0.5), StopPos::Frac(0.5)),
        ),
    };
    Ok(Raster::fill(
        RKind::Radial { circle, extent, at },
        repeat,
        stop_list(parts)?,
    ))
}

fn repeating_linear(args: &str) -> Result<Fill, String> {
    let Fill::Linear { angle, stops } = gradient(args)? else {
        unreachable!()
    };
    Ok(Raster::fill(RKind::Linear { angle }, true, stops))
}

/// The address in a layer's first `url(...)` and the byte range of the whole call.
pub fn url_call(layer: &str) -> Option<(String, std::ops::Range<usize>)> {
    let lower = layer.to_ascii_lowercase();
    let start = lower.find("url(")?;
    let inner = start + 4;
    let rest = &layer[inner..];
    let (mut quote, mut end) = (None::<char>, None);
    for (i, ch) in rest.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, ')') => {
                end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let end = end?;
    let url = rest[..end].trim().trim_matches(['"', '\'']).trim();
    Some((url.to_string(), start..inner + end + 1))
}

/// A `background` value: comma-separated layers, each a colour or `linear-gradient()` (other words in a layer, such as
/// `center / cover no-repeat`, are ignored). The first layer is on top. A layer with `url()` contributes only its colour here: the picture is [`pic`].
pub fn layers(v: &str) -> Result<Vec<Fill>, String> {
    if v.trim() == "none" {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for layer in split_top(v, Some(',')) {
        let stripped;
        let has_pic = url_call(layer).is_some();
        let layer = if let Some((_, r)) = url_call(layer) {
            stripped = format!("{} {}", &layer[..r.start], &layer[r.end..]);
            stripped.as_str()
        } else {
            layer
        };
        let l = layer.to_ascii_lowercase();
        // Conic gradients cannot be drawn: the layer is left out (the rest of the value still applies).
        let kinds = [
            ("repeating-linear-gradient(", 0),
            ("repeating-radial-gradient(", 1),
            ("radial-gradient(", 2),
            ("linear-gradient(", 3),
        ];
        let found = kinds
            .iter()
            .find_map(|&(n, k)| l.find(n).map(|i| (n, k, i)));
        let fill = if l.contains("conic-gradient(") {
            continue;
        } else if let Some((name, kind, i)) = found {
            // Take the balanced call.
            let args_start = i + name.len();
            let mut depth = 1;
            let mut end = None;
            for (k, ch) in l[args_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(args_start + k);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let end = end.ok_or("unclosed gradient")?;
            let args = &l[args_start..end];
            match kind {
                0 => repeating_linear(args)?,
                1 => radial(args, true)?,
                2 => radial(args, false)?,
                _ => gradient(args)?,
            }
        } else if l.contains("gradient(") {
            return Err("only linear-gradient() is supported".into());
        } else {
            let toks = split_top(&l, None);
            match toks.iter().find_map(|t| value::color(t).ok()) {
                Some(c) => Fill::Solid(c),
                // A picture layer with only position, size and repeat words.
                None if has_pic => continue,
                // Only position, size and repeat words (`background: 0 0`): no fill, but the value still resets the old one.
                None if toks.iter().all(|t| {
                    t.trim_end_matches("px")
                        .trim_end_matches('%')
                        .parse::<f32>()
                        .is_ok()
                        || matches!(
                            *t,
                            "/" | "center"
                                | "left"
                                | "right"
                                | "top"
                                | "bottom"
                                | "auto"
                                | "cover"
                                | "contain"
                                | "repeat"
                                | "no-repeat"
                                | "repeat-x"
                                | "repeat-y"
                                | "scroll"
                                | "fixed"
                                | "local"
                        )
                }) =>
                {
                    continue;
                }
                None => return Err(format!("`{layer}` is not a supported background")),
            }
        };
        out.push(fill);
        if out.len() > MAX_LAYERS {
            return Err("more than 4 background layers".into());
        }
    }
    Ok(out)
}

/// One side of `background-size`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BgDim {
    Auto,
    Px(f32),
    Pct(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BgSize {
    Auto,
    Cover,
    Contain,
    Dims(BgDim, BgDim),
}

/// `background-position` per axis as `(fraction of the free space, extra px)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BgPos {
    pub x: (f32, f32),
    pub y: (f32, f32),
}

pub const BG_POS0: BgPos = BgPos {
    x: (0.0, 0.0),
    y: (0.0, 0.0),
};

/// A picture layer (`url(...)`) of a `background` value: the address as written and its size, position and repeat.
#[derive(Clone, Debug, PartialEq)]
pub struct Pic {
    pub url: std::rc::Rc<str>,
    pub size: BgSize,
    pub pos: BgPos,
    /// Repeat along x and y.
    pub repeat: (bool, bool),
}

fn bg_dim(t: &str) -> Option<BgDim> {
    if t == "auto" {
        return Some(BgDim::Auto);
    }
    if let Some(n) = t.strip_suffix('%') {
        return n.trim().parse().ok().map(BgDim::Pct);
    }
    if let Some(n) = t.strip_suffix("px") {
        return n.trim().parse().ok().map(BgDim::Px);
    }
    (t == "0").then_some(BgDim::Px(0.0))
}

/// `center`, `left 10px`, `50% 0`, `top`: per axis.
pub fn bg_pos(toks: &[&str]) -> Option<BgPos> {
    let one = |t: &str, horiz: bool| -> Option<(f32, f32)> {
        match t {
            "left" if horiz => Some((0.0, 0.0)),
            "right" if horiz => Some((1.0, 0.0)),
            "top" if !horiz => Some((0.0, 0.0)),
            "bottom" if !horiz => Some((1.0, 0.0)),
            "center" => Some((0.5, 0.0)),
            _ => match bg_dim(t)? {
                BgDim::Pct(p) => Some((p / 100.0, 0.0)),
                BgDim::Px(p) => Some((0.0, p)),
                BgDim::Auto => None,
            },
        }
    };
    match toks {
        [a] if matches!(*a, "top" | "bottom") => Some(BgPos {
            x: (0.5, 0.0),
            y: one(a, false)?,
        }),
        [a] => Some(BgPos {
            x: one(a, true)?,
            y: (0.5, 0.0),
        }),
        [a, b] if matches!(*a, "top" | "bottom") && matches!(*b, "left" | "right") => Some(BgPos {
            x: one(b, true)?,
            y: one(a, false)?,
        }),
        [a, b] => Some(BgPos {
            x: one(a, true)?,
            y: one(b, false)?,
        }),
        _ => None,
    }
}

pub fn bg_size(toks: &[&str]) -> Option<BgSize> {
    match toks {
        ["cover"] => Some(BgSize::Cover),
        ["contain"] => Some(BgSize::Contain),
        [a] => Some(BgSize::Dims(bg_dim(a)?, BgDim::Auto)),
        [a, b] => Some(BgSize::Dims(bg_dim(a)?, bg_dim(b)?)),
        _ => None,
    }
}

pub fn bg_repeat(toks: &[&str]) -> Option<(bool, bool)> {
    let on = |t: &str| t != "no-repeat";
    match toks {
        ["repeat-x"] => Some((true, false)),
        ["repeat-y"] => Some((false, true)),
        [a] if matches!(*a, "repeat" | "no-repeat" | "space" | "round") => Some((on(a), on(a))),
        [a, b] => Some((on(a), on(b))),
        _ => None,
    }
}

fn pic_dims_map() -> std::sync::MutexGuard<'static, std::collections::HashMap<String, (u32, u32)>> {
    static M: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, (u32, u32)>>> =
        std::sync::OnceLock::new();
    M.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// The app tells the engine how big a loaded picture is (by the address as written in the stylesheet), so that an `img` whose picture comes from `content: url()` gets its natural size in layout.
pub fn set_pic_dims(url: &str, w: u32, h: u32) {
    let mut m = pic_dims_map();
    if m.len() < 512 {
        m.insert(url.to_string(), (w, h));
    }
}

pub fn pic_dims(url: &str) -> Option<(u32, u32)> {
    pic_dims_map()
        .get(url)
        .copied()
        .filter(|d| d.0 > 0 && d.1 > 0)
}

/// How many fills of `layers(v)` are listed above the first picture layer (gradients written before the `url()` paint over it).
pub fn pic_above(v: &str) -> usize {
    let mut n = 0;
    for layer in split_top(v, Some(',')) {
        if url_call(layer).is_some() {
            break;
        }
        n += layers(layer).map_or(0, |l| l.len());
    }
    n
}

/// The first picture layer of a `background` / `background-image` value, with the position, size and repeat words of the layer (the longhands set them otherwise).
pub fn pic(v: &str) -> Option<Pic> {
    for layer in split_top(v, Some(',')) {
        let Some((url, r)) = url_call(layer) else {
            continue;
        };
        if url.is_empty() {
            return None;
        }
        let rest = format!("{} {}", &layer[..r.start], &layer[r.end..]).to_ascii_lowercase();
        let rest = rest.replace('/', " / ");
        let (mut pos, mut size, mut rep) = (vec![], vec![], vec![]);
        let mut after = false;
        for t in split_top(&rest, None) {
            match t {
                "/" => after = true,
                "repeat" | "no-repeat" | "repeat-x" | "repeat-y" | "space" | "round" => {
                    rep.push(t);
                    after = false;
                }
                "scroll" | "fixed" | "local" | "border-box" | "padding-box" | "content-box" => {
                    after = false
                }
                _ if value::color(t).is_ok() && !matches!(t, "center" | "top" | "left") => {}
                _ if after => size.push(t),
                _ => pos.push(t),
            }
        }
        return Some(Pic {
            url: url.as_str().into(),
            size: bg_size(&size).unwrap_or(BgSize::Auto),
            pos: bg_pos(&pos).unwrap_or(BG_POS0),
            repeat: bg_repeat(&rep).unwrap_or((true, true)),
        });
    }
    None
}

/// Where a picture of `iw` x `ih` is drawn in a box of `bw` x `bh` (relative to the box; callers clip to it): one rectangle, or the tiles that cover the box (at most 400).
pub fn pic_rects(p: &Pic, bw: f32, bh: f32, iw: f32, ih: f32) -> Vec<[f32; 4]> {
    if iw <= 0.0 || ih <= 0.0 || bw <= 0.0 || bh <= 0.0 {
        return vec![];
    }
    let dim = |d: BgDim, b: f32| match d {
        BgDim::Auto => None,
        BgDim::Px(p) => Some(p),
        BgDim::Pct(p) => Some(b * p / 100.0),
    };
    let (w, h) = match p.size {
        BgSize::Auto => (iw, ih),
        BgSize::Cover => {
            let s = (bw / iw).max(bh / ih);
            (iw * s, ih * s)
        }
        BgSize::Contain => {
            let s = (bw / iw).min(bh / ih);
            (iw * s, ih * s)
        }
        BgSize::Dims(a, b) => match (dim(a, bw), dim(b, bh)) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, ih * w / iw),
            (None, Some(h)) => (iw * h / ih, h),
            (None, None) => (iw, ih),
        },
    };
    if w < 0.5 || h < 0.5 {
        return vec![];
    }
    let x = (bw - w) * p.pos.x.0 + p.pos.x.1;
    let y = (bh - h) * p.pos.y.0 + p.pos.y.1;
    let axis = |at: f32, len: f32, bound: f32, rep: bool| -> Vec<f32> {
        if !rep {
            return vec![at];
        }
        let mut s = at - (at / len).ceil() * len;
        if s > 0.0 {
            s -= len;
        }
        let mut v = vec![];
        while s < bound && v.len() < 200 {
            v.push(s);
            s += len;
        }
        v
    };
    let xs = axis(x, w, bw, p.repeat.0);
    let ys = axis(y, h, bh, p.repeat.1);
    let mut out = vec![];
    for &yy in &ys {
        for &xx in &xs {
            if out.len() < 400 {
                out.push([xx, yy, w, h]);
            }
        }
    }
    out
}

/// A gradient ready to draw: angle and at most [`MAX_STOPS`] stops with ascending fractions.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub angle: f32,
    pub stops: Vec<(Color, f32)>,
}

/// Length of the gradient line for a box (CSS: `|w sin a| + |h cos a|`).
pub fn line_length(angle: f32, w: f32, h: f32) -> f32 {
    let a = angle.to_radians();
    (w * a.sin()).abs() + (h * a.cos()).abs()
}

/// Resolve stop positions (percent, px, missing) for a box of `w` x `h`, make them monotonic and cap the count.
pub fn resolve(fill: &Fill, w: f32, h: f32) -> Option<Resolved> {
    let Fill::Linear { angle, stops } = fill else {
        return None;
    };
    let len = line_length(*angle, w, h).max(1.0);
    let mut out = stop_fracs(stops, len);
    for o in &mut out {
        o.1 = o.1.clamp(0.0, 1.0);
    }
    if out.len() > MAX_STOPS {
        // Keep the ends and spread the rest: the shape of a long scrim survives.
        let m = out.len();
        out = (0..MAX_STOPS)
            .map(|k| out[k * (m - 1) / (MAX_STOPS - 1)])
            .collect();
    }
    Some(Resolved {
        angle: *angle,
        stops: out,
    })
}

/// Stop positions as fractions of a gradient line `len` long (percent, px, missing), made monotonic; not clamped to 0..1.
pub fn stop_fracs(stops: &[Stop], len: f32) -> Vec<(Color, f32)> {
    let mut p: Vec<Option<f32>> = stops
        .iter()
        .map(|s| {
            s.pos.map(|q| match q {
                StopPos::Frac(f) => f,
                StopPos::Px(px) => px / len,
                StopPos::Vw(_) | StopPos::Vh(_) => 0.0,
            })
        })
        .collect();
    let n = p.len();
    p[0].get_or_insert(0.0);
    p[n - 1].get_or_insert(1.0);
    let mut i = 1;
    while i < n {
        if p[i].is_none() {
            let lo = i - 1;
            let hi = (i..n).find(|&j| p[j].is_some()).unwrap_or(n - 1);
            let (a, b) = (p[lo].unwrap_or(0.0), p[hi].unwrap_or(1.0));
            #[allow(clippy::needless_range_loop)]
            for k in i..hi {
                p[k] = Some(a + (b - a) * (k - lo) as f32 / (hi - lo) as f32);
            }
            i = hi;
        }
        i += 1;
    }
    let mut out: Vec<(Color, f32)> = vec![];
    let mut top = f32::NEG_INFINITY;
    for (s, q) in stops.iter().zip(p) {
        let q = q.unwrap_or(0.0).max(top);
        top = q;
        out.push((s.color, q));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub inset: bool,
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
}

pub const MAX_SHADOWS: usize = 4;

/// `box-shadow`, `text-shadow`: `[inset] x y [blur [spread]] color`, comma separated (at most [`MAX_SHADOWS`]).
pub fn shadows(v: &str, allow_inset: bool, allow_spread: bool) -> Result<Vec<Shadow>, String> {
    if v.trim() == "none" {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for one in split_top(v, Some(',')) {
        let (mut nums, mut color, mut inset) = (vec![], None, false);
        for t in split_top(one, None) {
            if t == "inset" {
                if !allow_inset {
                    return Err("`inset` is not allowed here".into());
                }
                inset = true;
                continue;
            }
            match t
                .strip_suffix("px")
                .and_then(num)
                .or_else(|| (t == "0").then_some(0.0))
            {
                Some(n) => nums.push(n),
                None => color = Some(value::color(t)?),
            }
        }
        let color = color.unwrap_or(Color::rgba(0, 0, 0, 128));
        let (dx, dy, blur, spread) = match nums[..] {
            [dx, dy] => (dx, dy, 0.0, 0.0),
            [dx, dy, b] => (dx, dy, b, 0.0),
            [dx, dy, b, s] if allow_spread => (dx, dy, b, s),
            _ => return Err(format!("`{one}` is not a shadow")),
        };
        out.push(Shadow {
            inset,
            dx,
            dy,
            blur: blur.max(0.0),
            spread,
            color,
        });
        if out.len() >= MAX_SHADOWS {
            break;
        }
    }
    Ok(out)
}

/// A 2D transform at paint time: translation (px plus a fraction of the element's own box), scale, origin as box fractions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub tx: (f32, f32),
    pub ty: (f32, f32),
    pub sx: f32,
    pub sy: f32,
    pub origin: (f32, f32),
}

impl Xf {
    pub const IDENTITY: Xf = Xf {
        tx: (0.0, 0.0),
        ty: (0.0, 0.0),
        sx: 1.0,
        sy: 1.0,
        origin: (0.5, 0.5),
    };

    pub fn is_identity(&self) -> bool {
        self.tx == (0.0, 0.0) && self.ty == (0.0, 0.0) && self.sx == 1.0 && self.sy == 1.0
    }

    pub fn lerp(&self, o: &Xf, t: f32) -> Xf {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Xf {
            tx: (l(self.tx.0, o.tx.0), l(self.tx.1, o.tx.1)),
            ty: (l(self.ty.0, o.ty.0), l(self.ty.1, o.ty.1)),
            sx: l(self.sx, o.sx),
            sy: l(self.sy, o.sy),
            origin: o.origin,
        }
    }

    /// Translation in px and the (uniform) scale for a box of `w` x `h`: Iced's transformation has no non-uniform scale,
    /// so the larger of the two scale factors is used.
    pub fn resolve(&self, w: f32, h: f32) -> (f32, f32, f32) {
        let s = if (self.sx - 1.0).abs() >= (self.sy - 1.0).abs() {
            self.sx
        } else {
            self.sy
        };
        (self.tx.0 + self.tx.1 * w, self.ty.0 + self.ty.1 * h, s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

/// The rectangle an image of `iw` x `ih` is drawn into inside a box of `bw` x `bh` (relative to the box; it may overflow it,
/// the caller clips to the box). `pos` is `object-position` as fractions of the free space.
pub fn fit_rect(fit: Fit, pos: (f32, f32), bw: f32, bh: f32, iw: f32, ih: f32) -> [f32; 4] {
    if iw <= 0.0 || ih <= 0.0 {
        return [0.0, 0.0, bw, bh];
    }
    let (w, h) = match fit {
        Fit::Fill => (bw, bh),
        Fit::None => (iw, ih),
        Fit::Contain | Fit::Cover | Fit::ScaleDown => {
            let (sx, sy) = (bw / iw, bh / ih);
            let s = match fit {
                Fit::Cover => sx.max(sy),
                Fit::Contain => sx.min(sy),
                _ => sx.min(sy).min(1.0),
            };
            (iw * s, ih * s)
        }
    };
    [(bw - w) * pos.0, (bh - h) * pos.1, w, h]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    Bezier(f32, f32, f32, f32),
}

impl Ease {
    /// `t` and the result are in 0..=1.
    pub fn at(self, t: f32) -> f32 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        let (x1, y1, x2, y2) = match self {
            Ease::Linear => return t,
            Ease::Ease => (0.25, 0.1, 0.25, 1.0),
            Ease::EaseIn => (0.42, 0.0, 1.0, 1.0),
            Ease::EaseOut => (0.0, 0.0, 0.58, 1.0),
            Ease::EaseInOut => (0.42, 0.0, 0.58, 1.0),
            Ease::Bezier(a, b, c, d) => (a, b, c, d),
        };
        let bez = |a: f32, b: f32, s: f32| {
            let u = 1.0 - s;
            3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
        };
        // Solve x(s) = t by bisection (x is monotonic for x1, x2 in 0..=1), then read y(s).
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if bez(x1, x2, mid) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        bez(y1, y2, (lo + hi) / 2.0)
    }

    pub fn parse(t: &str) -> Option<Ease> {
        Some(match t {
            "linear" => Ease::Linear,
            "ease" => Ease::Ease,
            "ease-in" => Ease::EaseIn,
            "ease-out" => Ease::EaseOut,
            "ease-in-out" => Ease::EaseInOut,
            _ => {
                let a = t.strip_prefix("cubic-bezier(")?.strip_suffix(')')?;
                let n: Vec<f32> = a.split(',').filter_map(num).collect();
                match n[..] {
                    [x1, y1, x2, y2] if (0.0..=1.0).contains(&x1) && (0.0..=1.0).contains(&x2) => {
                        Ease::Bezier(x1, y1, x2, y2)
                    }
                    _ => return None,
                }
            }
        })
    }
}

/// The properties a transition can animate (paint-only: never layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AProp {
    Opacity,
    Transform,
    Color,
    Background,
    BorderColor,
}

pub const ALL_APROPS: [AProp; 5] = [
    AProp::Opacity,
    AProp::Transform,
    AProp::Color,
    AProp::Background,
    AProp::BorderColor,
];

/// One entry of `transition-property`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TP {
    All,
    One(AProp),
    /// A property that cannot animate here (layout, shadows): its slot keeps the timing lists aligned and does nothing.
    Other,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransSpec {
    pub props: Vec<TP>,
    pub durs: Vec<f32>,
    pub delays: Vec<f32>,
    pub eases: Vec<Ease>,
}

impl TransSpec {
    /// (duration ms, delay ms, easing) for the property, if it transitions.
    pub fn for_prop(&self, p: AProp) -> Option<(f32, f32, Ease)> {
        let k = self
            .props
            .iter()
            .position(|x| *x == TP::All)
            .or_else(|| self.props.iter().position(|x| *x == TP::One(p)))?;
        let pick = |v: &[f32]| if v.is_empty() { 0.0 } else { v[k % v.len()] };
        let dur = pick(&self.durs);
        if dur <= 0.0 && pick(&self.delays) <= 0.0 {
            return None;
        }
        let ease = if self.eases.is_empty() {
            Ease::Ease
        } else {
            self.eases[k % self.eases.len()]
        };
        Some((dur, pick(&self.delays), ease))
    }

    /// Nothing can transition.
    pub fn is_none(&self) -> bool {
        self.props.iter().all(|p| *p == TP::Other)
    }
}

fn time(t: &str) -> Option<f32> {
    if let Some(ms) = t.strip_suffix("ms") {
        return num(ms);
    }
    t.strip_suffix('s').and_then(num).map(|s| s * 1000.0)
}

pub fn aprop(name: &str) -> Option<AProp> {
    Some(match name {
        "opacity" => AProp::Opacity,
        "transform" => AProp::Transform,
        "color" => AProp::Color,
        "background" | "background-color" | "background-image" => AProp::Background,
        "border-color"
        | "border"
        | "border-top-color"
        | "border-bottom-color"
        | "border-left-color"
        | "border-right-color" => AProp::BorderColor,
        _ => return None,
    })
}

/// `transition` shorthand: `opacity 260ms cubic-bezier(...) 50ms, transform 0.3s`.
pub fn transition(v: &str) -> Result<TransSpec, String> {
    let mut spec = TransSpec::default();
    if v.trim() == "none" {
        return Ok(spec);
    }
    for item in split_top(v, Some(',')) {
        let (mut prop, mut times, mut ease) = (None, vec![], None);
        for t in split_top(item, None) {
            if let Some(ms) = time(t) {
                times.push(ms);
            } else if let Some(e) = Ease::parse(t) {
                ease = Some(e);
            } else if t.chars().all(|c| c.is_ascii_alphabetic() || c == '-') {
                prop = Some(t);
            } else {
                return Err(format!("`{t}` is not supported in a transition"));
            }
        }
        spec.durs
            .push(times.first().copied().unwrap_or(0.0).max(0.0));
        spec.delays.push(times.get(1).copied().unwrap_or(0.0));
        spec.eases.push(ease.unwrap_or(Ease::Ease));
        spec.props.push(match prop.unwrap_or("all") {
            "all" => TP::All,
            p => aprop(p).map_or(TP::Other, TP::One),
        });
    }
    if let Some(k) = spec.props.iter().position(|p| *p == TP::All) {
        // With `all` present its timing applies to every property.
        spec.props = vec![TP::All];
        spec.durs = vec![spec.durs[k]];
        spec.delays = vec![spec.delays[k]];
        spec.eases = vec![spec.eases[k]];
    }
    Ok(spec)
}

/// A resolved `animation` (the first of a list): what runs, how long, how often.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimSpec {
    pub name: String,
    pub dur: f32,
    pub delay: f32,
    /// `f32::INFINITY` for `infinite`.
    pub iters: f32,
    pub ease: Ease,
    pub fill_fwd: bool,
    pub fill_back: bool,
    pub alternate: bool,
    pub reverse: bool,
}

impl Default for AnimSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            dur: 0.0,
            delay: 0.0,
            iters: 1.0,
            ease: Ease::Ease,
            fill_fwd: false,
            fill_back: false,
            alternate: false,
            reverse: false,
        }
    }
}

/// The parts of an `animation` declaration that were written; the rest keeps the element's earlier value (longhands) or its default (the shorthand).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimPatch {
    pub name: Option<String>,
    pub dur: Option<f32>,
    pub delay: Option<f32>,
    pub iters: Option<f32>,
    pub ease: Option<Ease>,
    pub fill: Option<(bool, bool)>,
    pub dir: Option<(bool, bool)>,
    pub all: bool,
}

impl AnimPatch {
    pub fn apply(&self, a: &mut AnimSpec) {
        if self.all {
            *a = AnimSpec::default();
        }
        if let Some(n) = &self.name {
            a.name = n.clone();
        }
        if let Some(v) = self.dur {
            a.dur = v;
        }
        if let Some(v) = self.delay {
            a.delay = v;
        }
        if let Some(v) = self.iters {
            a.iters = v;
        }
        if let Some(v) = self.ease {
            a.ease = v;
        }
        if let Some((f, b)) = self.fill {
            (a.fill_fwd, a.fill_back) = (f, b);
        }
        if let Some((alt, rev)) = self.dir {
            (a.alternate, a.reverse) = (alt, rev);
        }
    }
}

/// `animation` shorthand: the first item of a comma list (several animations on one element are not run).
pub fn animation(v: &str) -> Result<AnimPatch, String> {
    let item = split_top(v, Some(',')).into_iter().next().unwrap_or("");
    let mut p = AnimPatch {
        all: true,
        name: Some(String::new()),
        ..Default::default()
    };
    let mut times = vec![];
    for t in split_top(item, None) {
        if let Some(ms) = time(t) {
            times.push(ms);
        } else if let Some(e) = Ease::parse(t) {
            p.ease = Some(e);
        } else if t == "infinite" {
            p.iters = Some(f32::INFINITY);
        } else if let Some(n) = num(t) {
            p.iters = Some(n.max(0.0));
        } else if let Some(d) = anim_dir(t) {
            p.dir = Some(d);
        } else if let Some(f) = anim_fill(t) {
            p.fill = Some(f);
        } else if matches!(t, "running" | "paused") {
        } else if t != "none" {
            p.name = Some(t.to_string());
        }
    }
    p.dur = Some(times.first().copied().unwrap_or(0.0).max(0.0));
    p.delay = Some(times.get(1).copied().unwrap_or(0.0));
    Ok(p)
}

pub fn anim_dir(t: &str) -> Option<(bool, bool)> {
    Some(match t {
        "normal" => (false, false),
        "reverse" => (false, true),
        "alternate" => (true, false),
        "alternate-reverse" => (true, true),
        _ => return None,
    })
}

pub fn anim_fill(t: &str) -> Option<(bool, bool)> {
    Some(match t {
        "none" => (false, false),
        "forwards" => (true, false),
        "backwards" => (false, true),
        "both" => (true, true),
        _ => return None,
    })
}

/// One animation longhand.
pub fn animation_part(name: &str, v: &str) -> Result<AnimPatch, String> {
    let first = split_top(v, Some(','))
        .into_iter()
        .next()
        .unwrap_or("")
        .trim();
    let mut p = AnimPatch::default();
    let bad = || format!("`{first}` is not a valid {name}");
    match name {
        "animation-name" => {
            p.name = Some(if first == "none" {
                String::new()
            } else {
                first.to_string()
            })
        }
        "animation-duration" => p.dur = Some(time(first).ok_or_else(bad)?.max(0.0)),
        "animation-delay" => p.delay = Some(time(first).ok_or_else(bad)?),
        "animation-iteration-count" => {
            p.iters = Some(if first == "infinite" {
                f32::INFINITY
            } else {
                num(first).ok_or_else(bad)?.max(0.0)
            })
        }
        "animation-timing-function" => p.ease = Some(Ease::parse(first).ok_or_else(bad)?),
        "animation-fill-mode" => p.fill = Some(anim_fill(first).ok_or_else(bad)?),
        "animation-direction" => p.dir = Some(anim_dir(first).ok_or_else(bad)?),
        _ => return Err(bad()),
    }
    Ok(p)
}

/// Radius (rx, ry) of a radial gradient in a `bw` x `bh` box around `(cx, cy)`.
fn radial_radii(circle: bool, e: Extent, bw: f32, bh: f32, cx: f32, cy: f32) -> (f32, f32) {
    let (nx, ny) = (cx.min(bw - cx).max(0.0), cy.min(bh - cy).max(0.0));
    let (fx, fy) = (cx.max(bw - cx), cy.max(bh - cy));
    let (rx, ry) = match e {
        Extent::Len(a, b) => {
            let l = |p: StopPos, whole: f32| match p {
                StopPos::Frac(f) => f * whole,
                StopPos::Px(v) => v,
                _ => 0.0,
            };
            if circle {
                let r = l(a, bw);
                (r, r)
            } else {
                (l(a, bw), l(b, bh))
            }
        }
        _ if circle => {
            let r = match e {
                Extent::ClosestSide => nx.min(ny),
                Extent::FarthestSide => fx.max(fy),
                Extent::ClosestCorner => nx.hypot(ny),
                _ => fx.hypot(fy),
            };
            (r, r)
        }
        Extent::ClosestSide => (nx, ny),
        Extent::FarthestSide => (fx, fy),
        Extent::ClosestCorner => (nx * 2f32.sqrt(), ny * 2f32.sqrt()),
        _ => (fx * 2f32.sqrt(), fy * 2f32.sqrt()),
    };
    (rx.max(0.01), ry.max(0.01))
}

/// Straight RGBA pixels (`pw` x `ph`) of a radial or repeating gradient painted over a `bw` x `bh` box (the pixel grid may be a scaled copy of the box). Colours blend premultiplied, as CSS does.
pub fn rasterize(r: &Raster, bw: f32, bh: f32, pw: u32, ph: u32) -> Vec<u8> {
    let mut out = vec![0u8; (pw * ph * 4) as usize];
    if bw <= 0.0 || bh <= 0.0 || r.stops.len() < 2 {
        return out;
    }
    // Position of a pixel along the gradient (in stop fractions) is `f(x, y)`; the stop table is built for the line / radius length.
    let (len, f): (f32, Box<dyn Fn(f32, f32) -> f32>) = match r.kind {
        RKind::Linear { angle } => {
            let a = angle.to_radians();
            let (dx, dy) = (a.sin(), -a.cos());
            let len = line_length(angle, bw, bh).max(1.0);
            (
                len,
                Box::new(move |x, y| ((x - bw / 2.0) * dx + (y - bh / 2.0) * dy) / len + 0.5),
            )
        }
        RKind::Radial { circle, extent, at } => {
            let pos_of = |p: StopPos, whole: f32| match p {
                StopPos::Frac(f) => f * whole,
                StopPos::Px(v) => v,
                _ => whole / 2.0,
            };
            let (cx, cy) = (pos_of(at.0, bw), pos_of(at.1, bh));
            let (rx, ry) = radial_radii(circle, extent, bw, bh, cx, cy);
            (
                rx,
                Box::new(move |x, y| ((x - cx) / rx).hypot((y - cy) / ry)),
            )
        }
    };
    let stops = stop_fracs(&r.stops, len);
    let (first, last) = (stops[0].1, stops[stops.len() - 1].1);
    // Premultiplied stop colours.
    let pm: Vec<[f32; 4]> = stops
        .iter()
        .map(|(c, _)| {
            let a = c.a as f32 / 255.0;
            [c.r as f32 * a, c.g as f32 * a, c.b as f32 * a, c.a as f32]
        })
        .collect();
    let (sx, sy) = (bw / pw as f32, bh / ph as f32);
    for py in 0..ph {
        for px in 0..pw {
            let mut t = f((px as f32 + 0.5) * sx, (py as f32 + 0.5) * sy);
            if r.repeat && last > first {
                t = first + (t - first).rem_euclid(last - first);
            }
            let i = stops.iter().position(|s| s.1 > t).unwrap_or(stops.len());
            let c = if i == 0 {
                pm[0]
            } else if i == stops.len() {
                pm[i - 1]
            } else {
                let (a, b) = (stops[i - 1].1, stops[i].1);
                let k = if b > a { (t - a) / (b - a) } else { 1.0 };
                let (p, q) = (pm[i - 1], pm[i]);
                [0, 1, 2, 3].map(|j| p[j] + (q[j] - p[j]) * k)
            };
            let o = ((py * pw + px) * 4) as usize;
            let a = c[3];
            if a > 0.0 {
                let inv = 255.0 / a;
                out[o] = (c[0] * inv).round().clamp(0.0, 255.0) as u8;
                out[o + 1] = (c[1] * inv).round().clamp(0.0, 255.0) as u8;
                out[o + 2] = (c[2] * inv).round().clamp(0.0, 255.0) as u8;
                out[o + 3] = a.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod pic_tests {
    use super::*;

    #[test]
    fn reads_the_picture_layer_of_a_shorthand() {
        let p = pic("url(\"a b.png\") center / cover no-repeat #000, linear-gradient(red, blue)")
            .unwrap();
        assert_eq!(&*p.url, "a b.png");
        assert_eq!(p.size, BgSize::Cover);
        assert_eq!(p.repeat, (false, false));
        assert_eq!(p.pos.x, (0.5, 0.0));
        // The gradient and the colour still come out of `layers`.
        let l = layers("url(x.png) no-repeat, linear-gradient(red, blue)").unwrap();
        assert_eq!(l.len(), 1);
        assert!(pic("linear-gradient(red, blue)").is_none());
        let d = pic("url(data:image/svg+xml,%3Csvg viewBox='0 0 1 1'/%3E)").unwrap();
        assert!(d.url.starts_with("data:image/svg+xml,"));
    }

    #[test]
    fn colour_hints_and_viewport_stops_are_kept() {
        let l = layers("linear-gradient(180deg, transparent, 17vh, #102030 50vh)").unwrap();
        let Fill::Linear { stops, .. } = &l[0] else {
            panic!()
        };
        assert_eq!(stops.len(), 3, "the hint became a stop");
        assert_eq!(stops[1].pos, Some(StopPos::Vh(17.0)));
        assert_eq!(
            layers("linear-gradient(0deg,#111827,45%,rgba(17,24,39,.25))")
                .unwrap()
                .len(),
            1
        );
        let p = at_viewport(&l, 1000.0, 1000.0);
        let Fill::Linear { stops, .. } = &p[0] else {
            panic!()
        };
        assert_eq!(stops[2].pos, Some(StopPos::Px(500.0)));
    }

    #[test]
    fn counts_layers_over_the_picture_and_skips_conic_ones() {
        assert_eq!(
            pic_above("linear-gradient(red, blue), url(x.png) no-repeat"),
            1
        );
        assert_eq!(pic_above("url(x.png), linear-gradient(red, blue)"), 0);
        let l = layers("conic-gradient(red, blue), repeating-linear-gradient(135deg, red 0, blue 4px), #101010").unwrap();
        assert_eq!(l.len(), 2);
        assert!(layers("0 0").unwrap().is_empty());
    }

    #[test]
    fn places_and_tiles() {
        let mut p = pic("url(x.png) 100% 0 / 50px auto no-repeat").unwrap();
        // 100 x 50 image shown 50 wide: 25 high, flush right and top.
        assert_eq!(
            pic_rects(&p, 200.0, 100.0, 100.0, 50.0),
            vec![[150.0, 0.0, 50.0, 25.0]]
        );
        p.size = BgSize::Cover;
        p.pos = bg_pos(&["center"]).unwrap();
        assert_eq!(
            pic_rects(&p, 200.0, 200.0, 100.0, 50.0),
            vec![[-100.0, 0.0, 400.0, 200.0]]
        );
        p.size = BgSize::Auto;
        p.repeat = (true, false);
        p.pos = BG_POS0;
        assert_eq!(pic_rects(&p, 100.0, 50.0, 50.0, 50.0).len(), 2);
    }

    #[test]
    fn radial_and_repeating_rasterise() {
        let l = layers("radial-gradient(circle at 50% 42%, rgba(255,0,0,.26), rgba(255,0,0,0) 62%), repeating-linear-gradient(135deg, #fff 0, #fff 2px, #0000 2px, #0000 11px), linear-gradient(red, blue)").unwrap();
        assert_eq!(l.len(), 3);
        let (Fill::Raster(a), Fill::Raster(b)) = (&l[0], &l[1]) else {
            panic!()
        };
        // Centre of the glow is the first stop, the far corner is clear.
        let px = rasterize(a, 100.0, 100.0, 100, 100);
        assert!((64..=66).contains(&px[(42 * 100 + 50) * 4 + 3]));
        assert_eq!(px[3], 0);
        // The tile repeats with an 11 px period along the line.
        let px = rasterize(b, 100.0, 100.0, 100, 100);
        let at = |x: usize, y: usize| px[(y * 100 + x) * 4 + 3];
        assert!((0..100).any(|x| at(x, 50) == 255) && (0..100).any(|x| at(x, 50) == 0));
    }
}
