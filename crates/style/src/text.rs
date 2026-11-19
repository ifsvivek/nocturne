//! Text metrics and the engine's own line breaking. Plain text is shaped and wrapped by the renderer; text that needs more
//! than that (a line clamp with an ellipsis, `letter-spacing`, `nowrap` with `text-overflow`) is broken into lines here, using
//! the caller's measure function for word and character widths, so the layout and the paint agree on every line.
use crate::fonts::FontChoice;

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
    pub font: FontChoice,
    /// Multiple of the font size.
    pub line_height: f32,
    pub lines: Option<u16>,
    /// Extra advance after every character, in px (Iced has no tracking, so the paint places characters itself).
    pub letter_spacing: f32,
    pub ellipsis: bool,
}

impl TextStyle {
    pub fn line_px(&self) -> f32 {
        self.size * self.line_height
    }

    /// Does this text go through [`block`]?
    pub fn needs_block(&self) -> bool {
        self.letter_spacing != 0.0 || self.lines.is_some()
    }

    fn plain(&self) -> TextStyle {
        TextStyle {
            letter_spacing: 0.0,
            lines: None,
            ..self.clone()
        }
    }
}

/// `(text, style, max width)`: `None` is "as wide as it wants", `Some(0.0)` is min-content. Returns the size in px.
pub type Measure<'a> = dyn FnMut(&str, &TextStyle, Option<f32>) -> (f32, f32) + 'a;

/// A cache in front of the real shaper: layout asks for the same string at several widths, so every distinct
/// (text, style, width) is shaped once. Bounded: it starts over when it grows past [`MeasureCache::LIMIT`] entries.
#[derive(Default)]
pub struct MeasureCache {
    map: std::collections::HashMap<u64, (f32, f32)>,
    pub hits: u64,
    pub misses: u64,
}

impl MeasureCache {
    pub const LIMIT: usize = 50_000;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Measure through the cache; `raw` is the shaper (called only on a miss).
    pub fn measure(
        &mut self,
        text: &str,
        ts: &TextStyle,
        max: Option<f32>,
        raw: &mut Measure,
    ) -> (f32, f32) {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        (
            text,
            ts.size.to_bits(),
            ts.weight,
            ts.italic,
            &ts.font,
            ts.line_height.to_bits(),
            ts.lines,
            max.map(f32::to_bits),
        )
            .hash(&mut h);
        let key = h.finish();
        if let Some(v) = self.map.get(&key) {
            self.hits += 1;
            return *v;
        }
        self.misses += 1;
        let v = raw(text, ts, max);
        if self.map.len() >= Self::LIMIT {
            self.map.clear();
        }
        self.map.insert(key, v);
        v
    }
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub width: f32,
    /// x of each character relative to the line start, when the line is placed character by character (letter-spacing).
    pub x: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Block {
    pub lines: Vec<Line>,
    pub width: f32,
    pub height: f32,
    /// The text did not fit and was cut (ellipsis or clip).
    pub truncated: bool,
}

const ELLIPSIS: char = '\u{2026}';

struct W<'a, 'b> {
    ts: &'a TextStyle,
    plain: TextStyle,
    measure: &'a mut Measure<'b>,
}

impl W<'_, '_> {
    fn natural(&mut self, s: &str) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        (self.measure)(s, &self.plain, None).0
    }

    /// Width of `s` with letter-spacing after every character.
    fn width(&mut self, s: &str) -> f32 {
        self.natural(s) + self.ts.letter_spacing * s.chars().count() as f32
    }

    fn space(&mut self) -> f32 {
        // The width of a space is what `a b` adds to `ab` (a lone space may be trimmed by the shaper).
        let d = self.natural("a b") - self.natural("ab");
        let d = if d > 0.0 { d } else { self.ts.size * 0.28 };
        d + self.ts.letter_spacing
    }

    /// Longest prefix of `s` that fits `max` together with an ellipsis; returns the text with the ellipsis and its width.
    fn with_ellipsis(&mut self, s: &str, max: f32) -> (String, f32) {
        let chars: Vec<char> = s.chars().collect();
        let ell_w = self.width("\u{2026}");
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let t: String = chars[..mid].iter().collect();
            if self.width(t.trim_end()) + ell_w <= max {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let t: String = chars[..lo].iter().collect();
        let t = t.trim_end().to_owned();
        let w = self.width(&t) + ell_w;
        (format!("{t}{ELLIPSIS}"), w)
    }
}

/// Break `text` into lines for a content box `max` px wide (`None` = unbounded), honouring `lines`, `ellipsis` and
/// `letter_spacing`. Per-character positions are filled only when there is letter-spacing.
pub fn block(text: &str, ts: &TextStyle, max: Option<f32>, measure: &mut Measure) -> Block {
    let mut w = W {
        plain: ts.plain(),
        ts,
        measure,
    };
    let limit = ts.lines.map(usize::from);
    let space = w.space();
    let mut lines: Vec<(String, f32)> = vec![];
    let (mut cur, mut cur_w) = (String::new(), 0.0f32);
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut truncated = false;
    let mut i = 0;
    while i < words.len() {
        let ww = w.width(words[i]);
        let add = if cur.is_empty() { 0.0 } else { space };
        let fits = max.is_none_or(|m| cur_w + add + ww <= m + 0.01 || cur.is_empty());
        if fits {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(words[i]);
            cur_w += add + ww;
            i += 1;
            continue;
        }
        // The word starts a new line.
        lines.push((std::mem::take(&mut cur), cur_w));
        cur_w = 0.0;
        if limit.is_some_and(|l| lines.len() >= l) {
            truncated = true;
            // The rest of the text belongs on the last line, to be cut by the ellipsis.
            let last = lines.pop().map(|(t, _)| t).unwrap_or_default();
            cur = format!("{last} {}", words[i..].join(" "));
            cur_w = f32::INFINITY;
            break;
        }
    }
    if !cur.is_empty() {
        lines.push((cur, cur_w));
    }
    if let Some(l) = limit
        && lines.len() > l
    {
        lines.truncate(l);
        truncated = true;
    }
    // Cut the last line to the box: with an ellipsis if asked (always for a clamp), else plainly.
    if let (Some(m), Some((last, lw))) = (max, lines.last_mut())
        && (truncated || *lw > m)
        && limit.is_some()
    {
        truncated = true;
        if w.ts.ellipsis {
            let (t, tw) = w.with_ellipsis(last, m);
            *last = t;
            *lw = tw;
        } else {
            // Clip: keep what fits.
            let chars: Vec<char> = last.chars().collect();
            let (mut lo, mut hi) = (0usize, chars.len());
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                let t: String = chars[..mid].iter().collect();
                if w.width(&t) <= m {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            let t: String = chars[..lo].iter().collect();
            *lw = w.width(&t);
            *last = t;
        }
    }
    let mut out = Block {
        truncated,
        ..Block::default()
    };
    for (t, lw) in lines {
        let x = if ts.letter_spacing != 0.0 {
            let mut x = Vec::with_capacity(t.chars().count());
            let mut at = 0.0;
            for ch in t.chars() {
                x.push(at);
                let adv = if ch == ' ' {
                    space
                } else {
                    w.natural(&ch.to_string()) + ts.letter_spacing
                };
                at += adv;
            }
            x
        } else {
            vec![]
        };
        out.width = out.width.max(lw);
        out.lines.push(Line {
            text: t,
            width: lw,
            x,
        });
    }
    out.height = out.lines.len() as f32 * ts.line_px();
    out
}
