//! CSS value forms the theme engine understands: colours, lengths, numbers and simple linear
//! gradients. Every function returns `Err(message)` for anything else; the caller turns that into
//! a warning. Pure string work, never panics.
use serde::{Deserialize, Serialize};

/// One CSS pixel is the base for `rem`/`em` (the browser default root size).
pub const BASE_PX: f32 = 16.0;
/// Iced allows at most 8 gradient stops.
pub const MAX_STOPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    /// Channels as 0..=1 floats, the shape Iced's `Color` wants.
    pub fn to_f32(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a].map(|c| c as f32 / 255.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Fill {
    Solid(Color),
    /// CSS angle in degrees (0 = towards the top, 90 = towards the right, 180 = towards the bottom)
    /// and at most [`MAX_STOPS`] stops with positions in 0..=1.
    Linear {
        angle: f32,
        stops: Vec<(Color, f32)>,
    },
}

/// Split at `sep` (or at whitespace when `None`) outside parentheses and quotes.
pub fn split_top(s: &str, sep: Option<char>) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (vec![], 0i32, None::<char>, 0);
    for (i, ch) in s.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth = (depth - 1).max(0),
            (None, c) if depth == 0 && sep.map_or(c.is_whitespace(), |s| s == c) => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

fn num(s: &str) -> Option<f32> {
    let s = s.trim();
    let ok = s.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'));
    s.parse::<f32>().ok().filter(|v| ok && v.is_finite())
}

fn func<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    s.strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')
}

fn hex(h: &str) -> Option<Color> {
    if !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let nib = |i: usize| {
        u8::from_str_radix(h.get(i..i + 1)?, 16)
            .ok()
            .map(|n| n * 17)
    };
    Some(match h.len() {
        3 => Color::rgba(nib(0)?, nib(1)?, nib(2)?, 255),
        4 => Color::rgba(nib(0)?, nib(1)?, nib(2)?, nib(3)?),
        6 => Color::rgba(byte(0)?, byte(2)?, byte(4)?, 255),
        8 => Color::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?),
        _ => return None,
    })
}

const NAMED: &[(&str, [u8; 3])] = &[
    ("black", [0, 0, 0]),
    ("white", [255, 255, 255]),
    ("red", [255, 0, 0]),
    ("green", [0, 128, 0]),
    ("blue", [0, 0, 255]),
    ("yellow", [255, 255, 0]),
    ("orange", [255, 165, 0]),
    ("purple", [128, 0, 128]),
    ("pink", [255, 192, 203]),
    ("gray", [128, 128, 128]),
    ("grey", [128, 128, 128]),
    ("silver", [192, 192, 192]),
    ("lightgray", [211, 211, 211]),
    ("lightgrey", [211, 211, 211]),
    ("darkgray", [169, 169, 169]),
    ("darkgrey", [169, 169, 169]),
    ("dimgray", [105, 105, 105]),
    ("dimgrey", [105, 105, 105]),
    ("whitesmoke", [245, 245, 245]),
    ("cyan", [0, 255, 255]),
    ("aqua", [0, 255, 255]),
    ("magenta", [255, 0, 255]),
    ("fuchsia", [255, 0, 255]),
    ("lime", [0, 255, 0]),
    ("navy", [0, 0, 128]),
    ("teal", [0, 128, 128]),
    ("maroon", [128, 0, 0]),
    ("olive", [128, 128, 0]),
    ("crimson", [220, 20, 60]),
    ("gold", [255, 215, 0]),
    ("indigo", [75, 0, 130]),
    ("violet", [238, 130, 238]),
    ("brown", [165, 42, 42]),
    ("coral", [255, 127, 80]),
    ("tomato", [255, 99, 71]),
];

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let k = |n: f32| (n + h / 30.0).rem_euclid(12.0);
    let a = s * l.min(1.0 - l);
    let f = |n: f32| l - a * (k(n) - 3.0).min(9.0 - k(n)).clamp(-1.0, 1.0);
    [f(0.0), f(8.0), f(4.0)]
}

pub fn color(s: &str) -> Result<Color, String> {
    let bad = || format!("`{}` is not a supported colour", s.trim());
    let s = s.trim().to_ascii_lowercase();
    if let Some(h) = s.strip_prefix('#') {
        return hex(h).ok_or_else(bad);
    }
    if s == "transparent" {
        return Ok(Color::rgba(0, 0, 0, 0));
    }
    if let Some((_, rgb)) = NAMED.iter().find(|(n, _)| *n == s) {
        return Ok(Color::rgba(rgb[0], rgb[1], rgb[2], 255));
    }
    let Some((name, args)) = s.strip_suffix(')').and_then(|x| x.split_once('(')) else {
        return Err(bad());
    };
    // NOCTURNE extensions so a sheet can derive colours from its tokens: `alpha(<colour>, <alpha>)` sets the alpha (`alpha(var(--nocturne-frame), 0.8)`), `mix(<colour>, <colour>, <0..1>)` blends towards the second.
    if name == "alpha" || name == "mix" {
        let p = split_top(args, Some(','));
        let frac = |v: &str| match v.trim().strip_suffix('%') {
            Some(x) => num(x).map(|v| v / 100.0),
            None => num(v),
        };
        return match (name, p.as_slice()) {
            ("alpha", [c, a]) => {
                let c = color(c)?;
                let a = frac(a).ok_or_else(bad)?.clamp(0.0, 1.0);
                Ok(Color::rgba(c.r, c.g, c.b, (a * 255.0).round() as u8))
            }
            ("mix", [a, b, t]) => {
                let (a, b) = (color(a)?, color(b)?);
                let t = frac(t).ok_or_else(bad)?.clamp(0.0, 1.0);
                let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
                Ok(Color::rgba(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), a.a))
            }
            _ => Err(bad()),
        };
    }
    let parts: Vec<&str> = if args.contains(',') {
        args.split(',').map(str::trim).collect()
    } else {
        args.split(|c: char| c.is_whitespace() || c == '/')
            .filter(|p| !p.is_empty())
            .collect()
    };
    if !(3..=4).contains(&parts.len()) {
        return Err(bad());
    }
    let alpha = match parts.get(3) {
        None => 255,
        Some(p) => {
            let a = match p.strip_suffix('%') {
                Some(x) => num(x).map(|v| v / 100.0),
                None => num(p),
            };
            (a.ok_or_else(bad)?.clamp(0.0, 1.0) * 255.0).round() as u8
        }
    };
    let pct = |p: &str| -> Option<f32> { num(p.strip_suffix('%')?) };
    match name {
        "rgb" | "rgba" => {
            let chan = |p: &str| -> Option<u8> {
                let v = match pct(p) {
                    Some(v) => v * 2.55,
                    None => num(p)?,
                };
                Some(v.clamp(0.0, 255.0).round() as u8)
            };
            Ok(Color::rgba(
                chan(parts[0]).ok_or_else(bad)?,
                chan(parts[1]).ok_or_else(bad)?,
                chan(parts[2]).ok_or_else(bad)?,
                alpha,
            ))
        }
        "hsl" | "hsla" => {
            let h = match (parts[0].strip_suffix("deg"), parts[0].strip_suffix("turn")) {
                (Some(d), _) => num(d),
                (_, Some(t)) => num(t).map(|t| t * 360.0),
                _ => num(parts[0]),
            };
            let (h, sat, lig) = (
                h.ok_or_else(bad)?,
                pct(parts[1]).ok_or_else(bad)? / 100.0,
                pct(parts[2]).ok_or_else(bad)? / 100.0,
            );
            let [r, g, b] = hsl_to_rgb(h, sat.clamp(0.0, 1.0), lig.clamp(0.0, 1.0))
                .map(|c| (c * 255.0).round() as u8);
            Ok(Color::rgba(r, g, b, alpha))
        }
        _ => Err(bad()),
    }
}

/// A non-negative length in px. `%` is taken of `pct_of`; `rem`/`em` of [`BASE_PX`].
pub fn length(s: &str, pct_of: f32) -> Result<f32, String> {
    let s = s.trim().to_ascii_lowercase();
    if s == "0" {
        return Ok(0.0);
    }
    if s.contains('(') {
        return Err(format!("`{s}`: functions such as calc() are not supported"));
    }
    let at = s
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .filter(|&i| i > 0);
    let Some(at) = at else {
        return Err(format!("`{s}` is not a length (a unit is required)"));
    };
    let n = num(&s[..at]).ok_or_else(|| format!("`{s}` is not a length"))?;
    let px = match &s[at..] {
        "px" => n,
        "rem" | "em" => n * BASE_PX,
        "pt" => n * 4.0 / 3.0,
        "%" => n * pct_of / 100.0,
        u => return Err(format!("the unit `{u}` is not supported")),
    };
    if px < 0.0 {
        return Err(format!("`{s}`: negative lengths are not supported"));
    }
    Ok(px)
}

/// A plain number or a percentage as a fraction (`50%` is 0.5).
pub fn number(s: &str) -> Result<f32, String> {
    let t = s.trim();
    match t.strip_suffix('%') {
        Some(p) => num(p).map(|v| v / 100.0),
        None => num(t),
    }
    .ok_or_else(|| format!("`{t}` is not a number"))
}

/// A colour or a `linear-gradient()`. The second value lists approximations that were made.
pub fn fill(s: &str) -> Result<(Fill, Vec<String>), String> {
    let l = s.trim().to_ascii_lowercase();
    let Some(args) = func(&l, "linear-gradient") else {
        if l.contains("gradient(") {
            return Err("only linear-gradient() is supported".into());
        }
        return Ok((Fill::Solid(color(&l)?), vec![]));
    };
    let mut parts = split_top(args, Some(','));
    let mut angle = 180.0;
    if let Some(first) = parts.first() {
        let words: Vec<&str> = first.split_whitespace().collect();
        let deg = if let Some(d) = first.strip_suffix("deg") {
            num(d)
        } else {
            first.strip_suffix("turn").and_then(num).map(|t| t * 360.0)
        };
        if let Some(d) = deg {
            angle = d.rem_euclid(360.0);
            parts.remove(0);
        } else if words.first() == Some(&"to") {
            let has = |w: &str| words.contains(&w);
            // Corners use the diagonal angle (exact only for square boxes).
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
            angle = match (v, h) {
                (Some(true), None) => 0.0,
                (Some(true), Some(true)) => 45.0,
                (None, Some(true)) => 90.0,
                (Some(false), Some(true)) => 135.0,
                (Some(false), None) => 180.0,
                (Some(false), Some(false)) => 225.0,
                (None, Some(false)) => 270.0,
                (Some(true), Some(false)) => 315.0,
                (None, None) => return Err("unrecognised gradient direction".into()),
            };
            parts.remove(0);
        }
    }
    let mut stops: Vec<(Color, Option<f32>)> = vec![];
    for p in parts {
        let toks = split_top(p, None);
        let c = color(toks.first().ok_or("empty gradient stop")?)?;
        let pos = match toks.get(1) {
            None => None,
            Some(t) => Some(
                t.strip_suffix('%')
                    .and_then(num)
                    .ok_or("gradient stop positions must be percentages")?
                    / 100.0,
            ),
        };
        stops.push((c, pos));
    }
    if stops.len() < 2 {
        return Err("a gradient needs at least two stops".into());
    }
    // Missing positions: the ends default to 0 and 1, gaps are spread evenly.
    let n = stops.len();
    stops[0].1.get_or_insert(0.0);
    stops[n - 1].1.get_or_insert(1.0);
    let mut i = 1;
    while i < n {
        if stops[i].1.is_none() {
            let lo = i - 1;
            let hi = (i..n).find(|&j| stops[j].1.is_some()).unwrap_or(n - 1);
            let (a, b) = (stops[lo].1.unwrap_or(0.0), stops[hi].1.unwrap_or(1.0));
            for (k, stop) in stops.iter_mut().enumerate().take(hi).skip(i) {
                stop.1 = Some(a + (b - a) * (k - lo) as f32 / (hi - lo) as f32);
            }
            i = hi;
        }
        i += 1;
    }
    let mut prev = 0.0f32;
    let mut out: Vec<(Color, f32)> = stops
        .into_iter()
        .map(|(c, p)| {
            prev = p.unwrap_or(prev).clamp(0.0, 1.0).max(prev);
            (c, prev)
        })
        .collect();
    let mut notes = vec![];
    if out.len() > MAX_STOPS {
        notes.push(format!(
            "gradient has {} stops; reduced to {MAX_STOPS}",
            out.len()
        ));
        let last = out.len() - 1;
        out = (0..MAX_STOPS)
            .map(|k| out[(k * last + (MAX_STOPS - 1) / 2) / (MAX_STOPS - 1)])
            .collect();
    }
    Ok((Fill::Linear { angle, stops: out }, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_colour_functions() {
        assert_eq!(
            color("alpha(#ff0000, 0.5)"),
            Ok(Color::rgba(255, 0, 0, 128))
        );
        assert_eq!(
            color("alpha(rgb(1, 2, 3), 50%)"),
            Ok(Color::rgba(1, 2, 3, 128))
        );
        assert_eq!(
            color("mix(#000, rgb(100, 200, 50), 0.5)"),
            Ok(Color::rgba(50, 100, 25, 255))
        );
        assert!(color("alpha(#fff)").is_err() && color("mix(#fff, #000)").is_err());
    }

    #[test]
    fn hex_forms() {
        assert_eq!(color("#fff"), Ok(Color::rgba(255, 255, 255, 255)));
        assert_eq!(color("#1234"), Ok(Color::rgba(17, 34, 51, 68)));
        assert_eq!(color("#5d55e7"), Ok(Color::rgba(0x5d, 0x55, 0xe7, 255)));
        assert_eq!(color("#5D55E780"), Ok(Color::rgba(0x5d, 0x55, 0xe7, 0x80)));
        assert!(color("#12345").is_err() && color("#ggg").is_err() && color("#é12").is_err());
    }

    #[test]
    fn functional_forms() {
        assert_eq!(color("rgb(1, 2, 3)"), Ok(Color::rgba(1, 2, 3, 255)));
        assert_eq!(
            color("rgba(17,24,39,.85)"),
            Ok(Color::rgba(17, 24, 39, 217))
        );
        assert_eq!(color("rgb(1 2 3 / 50%)"), Ok(Color::rgba(1, 2, 3, 128)));
        assert_eq!(
            color("rgb(8.627% 2.353% 18.431%)"),
            Ok(Color::rgba(22, 6, 47, 255))
        );
        assert_eq!(
            color("hsla(0,0%,100%,.2)"),
            Ok(Color::rgba(255, 255, 255, 51))
        );
        assert_eq!(
            color("hsl(120deg 100% 25%)"),
            Ok(Color::rgba(0, 128, 0, 255))
        );
        assert_eq!(
            color("hsl(0.5turn, 100%, 50%)"),
            Ok(Color::rgba(0, 255, 255, 255))
        );
        assert_eq!(color("Transparent"), Ok(Color::rgba(0, 0, 0, 0)));
        assert_eq!(color("WHITE"), Ok(Color::rgba(255, 255, 255, 255)));
        assert!(color("rgb(1,2)").is_err());
        assert!(color("currentcolor").is_err());
        assert!(color("lab(1 2 3)").is_err());
    }

    #[test]
    fn lengths() {
        assert_eq!(length("0", 40.0), Ok(0.0));
        assert_eq!(length("12px", 40.0), Ok(12.0));
        assert_eq!(length("1.25em", 40.0), Ok(20.0));
        assert_eq!(length("0.5rem", 40.0), Ok(8.0));
        assert_eq!(length("50%", 40.0), Ok(20.0));
        assert_eq!(length("3pt", 40.0), Ok(4.0));
        assert!(length("5", 40.0).is_err());
        assert!(length("-4px", 40.0).is_err());
        assert!(length("2vw", 40.0).unwrap_err().contains("vw"));
        assert!(
            length("calc(1px + 2px)", 40.0)
                .unwrap_err()
                .contains("calc")
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(number("0.5"), Ok(0.5));
        assert_eq!(number("25%"), Ok(0.25));
        assert!(number("nan").is_err());
        assert!(number("inf").is_err());
        assert!(number("abc").is_err());
    }

    #[test]
    fn gradients() {
        let (f, n) = fill("linear-gradient(90deg, #000 0%, #fff)").unwrap();
        assert!(n.is_empty());
        assert_eq!(
            f,
            Fill::Linear {
                angle: 90.0,
                stops: vec![
                    (Color::rgba(0, 0, 0, 255), 0.0),
                    (Color::rgba(255, 255, 255, 255), 1.0)
                ]
            }
        );
        let Fill::Linear { angle, stops } = fill("linear-gradient(to right, red, blue, lime)")
            .unwrap()
            .0
        else {
            panic!()
        };
        assert_eq!((angle, stops[1].1), (90.0, 0.5));
        let Fill::Linear { angle, .. } = fill("linear-gradient(red, blue)").unwrap().0 else {
            panic!()
        };
        assert_eq!(angle, 180.0);
        for (dir, want) in [
            ("top", 0.0),
            ("bottom right", 135.0),
            ("left bottom", 225.0),
            ("top left", 315.0),
        ] {
            let Fill::Linear { angle, .. } = fill(&format!("linear-gradient(to {dir}, red, blue)"))
                .unwrap()
                .0
            else {
                panic!()
            };
            assert_eq!(angle, want, "{dir}");
        }
        assert_eq!(
            fill("#101010").unwrap().0,
            Fill::Solid(Color::rgba(16, 16, 16, 255))
        );
        assert!(fill("radial-gradient(red, blue)").is_err());
        assert!(fill("linear-gradient(red)").is_err());
        assert!(fill("linear-gradient(red, 10px, blue)").is_err());
        assert!(fill("linear-gradient(red 10px, blue)").is_err());
    }

    #[test]
    fn long_gradients_are_reduced_to_eight_stops() {
        let stops: Vec<String> = (0..17)
            .map(|i| format!("rgb({i} 0 0) {}%", i * 100 / 16))
            .collect();
        let (f, notes) = fill(&format!("linear-gradient(90deg, {})", stops.join(", "))).unwrap();
        let Fill::Linear { stops, .. } = f else {
            panic!()
        };
        assert_eq!(stops.len(), 8);
        assert_eq!((stops[0].1, stops[7].1), (0.0, 1.0));
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn splitting_respects_parens_and_quotes() {
        assert_eq!(
            split_top("a, rgb(1,2,3), 'x,y'", Some(',')),
            ["a", "rgb(1,2,3)", "'x,y'"]
        );
        assert_eq!(
            split_top("1px  rgb(1 2 3) x", None),
            ["1px", "rgb(1 2 3)", "x"]
        );
    }
}
