//! Computed style of one element and the declaration table: CSS text in, a [`taffy::Style`] plus paint fields out.
//! Values that Taffy understands (`display`, `flex-*`, `grid-*`, lengths, alignment) are read by Taffy's own CSS parser.
use nocturne_core::customize::value::{self, Color, Fill, split_top};
use std::collections::HashMap;
use std::rc::Rc;
use taffy::geometry::{Line, Rect, Size};
use taffy::style::{
    AlignContent, AlignItems, BoxSizing, Dimension, Display, FlexDirection, FlexWrap, GridAutoFlow,
    GridAutoTracks, GridPlacement, GridTemplateComponent, GridTemplateTracks, LengthPercentage,
    LengthPercentageAuto, Overflow, Position, Style,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    None,
    Upper,
    Lower,
}

#[derive(Clone, Debug)]
pub struct Computed {
    pub layout: Style,
    pub order: i32,
    pub z: i32,
    /// `position: fixed`: laid out like absolute, painted at a fixed place on the screen while the page scrolls.
    pub fixed: bool,
    pub bg: Option<Fill>,
    pub border_color: Color,
    pub radius: f32,
    pub shadow: Option<Shadow>,
    pub opacity: f32,
    // Inherited text properties.
    pub color: Color,
    pub font_size: f32,
    pub weight: u16,
    pub line_height: f32,
    pub text_transform: Transform,
    pub align: Align,
    /// `-webkit-line-clamp` / `white-space: nowrap` (1).
    pub lines: Option<u16>,
    /// Custom properties, inherited; shared with the parent unless the element declares its own.
    pub vars: Rc<HashMap<String, String>>,
}

impl Computed {
    pub fn root() -> Self {
        Self {
            layout: Style {
                display: Display::Block,
                box_sizing: BoxSizing::BorderBox,
                ..Style::default()
            },
            order: 0,
            z: 0,
            fixed: false,
            bg: None,
            border_color: Color::rgba(255, 255, 255, 40),
            radius: 0.0,
            shadow: None,
            opacity: 1.0,
            color: Color::rgba(238, 238, 244, 255),
            font_size: 16.0,
            weight: 400,
            line_height: 1.3,
            text_transform: Transform::None,
            align: Align::Left,
            lines: None,
            vars: Rc::default(),
        }
    }

    /// Initial values for a child; only the inherited properties and the variables come from the parent.
    pub fn child_of(p: &Computed) -> Self {
        Self {
            color: p.color,
            font_size: p.font_size,
            weight: p.weight,
            line_height: p.line_height,
            text_transform: p.text_transform,
            align: p.align,
            vars: p.vars.clone(),
            ..Computed::root()
        }
    }
}

/// Context for relative units.
#[derive(Clone, Copy)]
pub struct Ctx {
    pub vw: f32,
    pub vh: f32,
}

fn num_prefix(s: &str) -> usize {
    s.char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+'))))
        .map_or(s.len(), |(i, _)| i)
}

/// Rewrite `vh`, `vw`, `vmin`, `vmax`, `rem`, `em` and `pt` numbers as `px` so Taffy's parser (which knows only px and %) can read them.
fn units(v: &str, cx: Ctx, font: f32) -> String {
    let mut out = String::with_capacity(v.len());
    let mut rest = v;
    while !rest.is_empty() {
        let n = num_prefix(rest);
        let starts_num = n > 0 && rest.as_bytes()[0] != b'+' && !rest[..n].ends_with(['-', '+']);
        if starts_num {
            let tail = &rest[n..];
            let u = tail
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(tail.len());
            let k: f32 = rest[..n].parse().unwrap_or(0.0);
            let px = match &tail[..u] {
                "vh" => Some(k * cx.vh / 100.0),
                "vw" => Some(k * cx.vw / 100.0),
                "vmin" => Some(k * cx.vw.min(cx.vh) / 100.0),
                "vmax" => Some(k * cx.vw.max(cx.vh) / 100.0),
                "rem" => Some(k * 16.0),
                "em" => Some(k * font),
                "pt" => Some(k * 4.0 / 3.0),
                _ => None,
            };
            match px {
                Some(p) => {
                    out.push_str(&format!("{p:.2}px"));
                    rest = &tail[u..];
                }
                None => {
                    out.push_str(&rest[..n + u]);
                    rest = &tail[u..];
                }
            }
        } else {
            // Copy one identifier or one character, so `h1` or `2fr` never lose their digits to a unit.
            let w = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                .filter(|&w| w > 0)
                .unwrap_or_else(|| rest.chars().next().map_or(1, char::len_utf8));
            out.push_str(&rest[..w]);
            rest = &rest[w..];
        }
    }
    out
}

/// Replace `var(--x)` and `var(--x, fallback)`; bounded in depth and length so a hostile sheet cannot blow up.
pub fn substitute(v: &str, vars: &HashMap<String, String>, depth: u8) -> Result<String, String> {
    if !v.contains("var(") {
        return Ok(v.to_owned());
    }
    if depth > 8 {
        return Err("var() nested too deeply".into());
    }
    let mut out = String::new();
    let mut rest = v;
    while let Some(p) = rest.find("var(") {
        out.push_str(&rest[..p]);
        let args = &rest[p + 4..];
        let (mut d, mut end) = (1, None);
        for (i, c) in args.char_indices() {
            match c {
                '(' => d += 1,
                ')' => {
                    d -= 1;
                    if d == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.ok_or("unclosed var(")?;
        let inner = &args[..end];
        let (name, fb) = match inner.split_once(',') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (inner.trim(), None),
        };
        let val = vars
            .get(name)
            .map(String::as_str)
            .or(fb)
            .ok_or_else(|| format!("`{name}` is not defined"))?;
        out.push_str(&substitute(val, vars, depth + 1)?);
        if out.len() > 4096 {
            return Err("value too long after var()".into());
        }
        rest = &args[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn p<T: std::str::FromStr>(v: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    v.trim().parse::<T>().map_err(|e| format!("`{v}`: {e}"))
}

fn four<T: std::str::FromStr + Copy>(v: &str) -> Result<Rect<T>, String>
where
    T::Err: std::fmt::Display,
{
    let a: Vec<T> = split_top(v, None)
        .into_iter()
        .map(p::<T>)
        .collect::<Result<_, _>>()?;
    let (t, r, b, l) = match a.as_slice() {
        [x] => (*x, *x, *x, *x),
        [y, x] => (*y, *x, *y, *x),
        [t, x, b] => (*t, *x, *b, *x),
        [t, r, b, l] => (*t, *r, *b, *l),
        _ => return Err(format!("`{v}`: expected 1 to 4 values")),
    };
    Ok(Rect {
        left: l,
        right: r,
        top: t,
        bottom: b,
    })
}

fn px(s: &str) -> Option<f32> {
    let s = s.trim();
    if s == "0" {
        return Some(0.0);
    }
    s.strip_suffix("px")?.parse().ok()
}

fn line(v: &str) -> Result<Line<GridPlacement>, String> {
    let (a, b) = match v.split_once('/') {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (v.trim(), None),
    };
    let start: GridPlacement = p(a)?;
    let end: GridPlacement = match b {
        Some(b) => p(b)?,
        // `span 2` alone means "span from auto"; a single line means "one track".
        None => GridPlacement::Auto,
    };
    Ok(Line { start, end })
}

fn flex_basis(v: &str) -> Result<Dimension, String> {
    p(v)
}

fn weight(v: &str) -> Result<u16, String> {
    match v.trim() {
        "normal" => Ok(400),
        "bold" => Ok(700),
        "lighter" => Ok(300),
        "bolder" => Ok(800),
        n => n
            .parse::<u16>()
            .ok()
            .filter(|w| (100..=900).contains(w))
            .ok_or_else(|| format!("`{v}` is not a font weight")),
    }
}

fn shadow(v: &str) -> Result<Option<Shadow>, String> {
    let first = split_top(v, Some(',')).into_iter().next().unwrap_or("none");
    if first == "none" {
        return Ok(None);
    }
    let (mut nums, mut color) = (vec![], Color::rgba(0, 0, 0, 128));
    for t in split_top(first, None) {
        if t == "inset" {
            return Err("inset shadows are not supported".into());
        }
        match px(t) {
            Some(n) => nums.push(n),
            None => color = value::color(t)?,
        }
    }
    match nums[..] {
        [dx, dy] => Ok(Some(Shadow {
            dx,
            dy,
            blur: 0.0,
            color,
        })),
        [dx, dy, blur, ..] => Ok(Some(Shadow {
            dx,
            dy,
            blur,
            color,
        })),
        _ => Err(format!("`{v}` is not a shadow")),
    }
}

fn fill(v: &str) -> Result<Option<Fill>, String> {
    if v == "none" {
        return Ok(None);
    }
    if v.contains("url(") {
        return Err("`url()` is never fetched; images come from the app's own data".into());
    }
    if let Ok((f, _)) = value::fill(v) {
        return Ok(Some(f));
    }
    // A `background` shorthand with extra words (`center / cover no-repeat`): take the first part that is a fill.
    split_top(v, None)
        .into_iter()
        .find_map(|t| value::fill(t).ok().map(|(f, _)| f))
        .map(Some)
        .ok_or_else(|| format!("`{v}` is not a supported background"))
}

fn border_side(c: &mut Computed, v: &str, sides: [bool; 4]) -> Result<(), String> {
    for t in split_top(v, None) {
        if let Some(w) = px(t) {
            let b = &mut c.layout.border;
            let w = LengthPercentage::length(w);
            if sides[0] {
                b.top = w;
            }
            if sides[1] {
                b.right = w;
            }
            if sides[2] {
                b.bottom = w;
            }
            if sides[3] {
                b.left = w;
            }
        } else if let Ok(col) = value::color(t) {
            c.border_color = col;
        } else if !matches!(t, "solid" | "none" | "hidden") {
            return Err(format!("`{t}` is not supported in a border"));
        } else if matches!(t, "none" | "hidden") {
            c.layout.border = Rect::zero();
        }
    }
    Ok(())
}

/// Properties that are valid CSS and planned, but not drawn yet by the spike (reported, not an error in the stylesheet).
const LATER: &[&str] = &[
    "transform",
    "transition",
    "transition-duration",
    "animation",
    "filter",
    "backdrop-filter",
    "cursor",
    "content",
    "letter-spacing",
    "text-overflow",
    "pointer-events",
    "outline",
    "outline-offset",
    "object-fit",
    "font-family",
    "text-shadow",
    "user-select",
    "will-change",
];

/// Apply one declaration. The value already had `var()` substituted by the caller.
pub fn apply(c: &mut Computed, name: &str, value: &str, cx: Ctx) -> Result<(), String> {
    let mut v = units(value.trim(), cx, c.font_size);
    if is_length_prop(name) {
        v = zero_px(&v);
    }
    let v = v.as_str();
    let st = &mut c.layout;
    match name {
        "display" => {
            st.display = match v {
                "inline" | "inline-block" | "contents" => Display::Block,
                "inline-flex" => Display::Flex,
                "inline-grid" => Display::Grid,
                _ => p(v)?,
            }
        }
        "position" => {
            c.fixed = v == "fixed";
            st.position = match v {
                "fixed" | "sticky" => Position::Absolute,
                _ => p(v)?,
            }
        }
        "top" => st.inset.top = p(v)?,
        "right" => st.inset.right = p(v)?,
        "bottom" => st.inset.bottom = p(v)?,
        "left" => st.inset.left = p(v)?,
        "inset" => st.inset = four::<LengthPercentageAuto>(v)?,
        "width" => st.size.width = p(v)?,
        "height" => st.size.height = p(v)?,
        "min-width" => st.min_size.width = p(v)?,
        "min-height" => st.min_size.height = p(v)?,
        "max-width" | "max-height" => {
            let m = if v == "none" {
                LengthPercentageAuto::auto()
            } else {
                p(v)?
            };
            if name == "max-width" {
                st.max_size.width = m;
            } else {
                st.max_size.height = m;
            }
        }
        "aspect-ratio" => {
            st.aspect_ratio = match v.split_once('/') {
                Some((a, b)) => Some(p::<f32>(a)? / p::<f32>(b)?),
                None if v == "auto" => None,
                None => Some(p(v)?),
            }
        }
        "box-sizing" => st.box_sizing = p(v)?,
        "margin" => st.margin = four(v)?,
        "margin-top" => st.margin.top = p(v)?,
        "margin-right" => st.margin.right = p(v)?,
        "margin-bottom" => st.margin.bottom = p(v)?,
        "margin-left" => st.margin.left = p(v)?,
        "padding" => st.padding = four(v)?,
        "padding-top" => st.padding.top = p(v)?,
        "padding-right" => st.padding.right = p(v)?,
        "padding-bottom" => st.padding.bottom = p(v)?,
        "padding-left" => st.padding.left = p(v)?,
        "border" => border_side(c, v, [true; 4])?,
        "border-top" => border_side(c, v, [true, false, false, false])?,
        "border-right" => border_side(c, v, [false, true, false, false])?,
        "border-bottom" => border_side(c, v, [false, false, true, false])?,
        "border-left" => border_side(c, v, [false, false, false, true])?,
        "border-width" => {
            st.border = four::<LengthPercentage>(v)?;
        }
        "border-color" => c.border_color = value::color(v)?,
        "border-radius" => {
            c.radius = split_top(v, None)
                .first()
                .and_then(|t| px(t))
                .ok_or_else(|| format!("`{v}` is not a radius"))?
        }
        "flex-direction" => st.flex_direction = p::<FlexDirection>(v)?,
        "flex-wrap" => st.flex_wrap = p::<FlexWrap>(v)?,
        "flex-grow" => st.flex_grow = p(v)?,
        "flex-shrink" => st.flex_shrink = p(v)?,
        "flex-basis" => st.flex_basis = flex_basis(v)?,
        "flex" => {
            let t = split_top(v, None);
            match t.as_slice() {
                ["none"] => {
                    (st.flex_grow, st.flex_shrink, st.flex_basis) = (0.0, 0.0, Dimension::auto())
                }
                ["auto"] => {
                    (st.flex_grow, st.flex_shrink, st.flex_basis) = (1.0, 1.0, Dimension::auto())
                }
                [g] => {
                    (st.flex_grow, st.flex_shrink, st.flex_basis) =
                        (p(g)?, 1.0, Dimension::length(0.0))
                }
                [g, s] => match p::<f32>(s) {
                    Ok(s) => {
                        (st.flex_grow, st.flex_shrink, st.flex_basis) =
                            (p(g)?, s, Dimension::length(0.0))
                    }
                    Err(_) => (st.flex_grow, st.flex_shrink, st.flex_basis) = (p(g)?, 1.0, p(s)?),
                },
                [g, s, b] => (st.flex_grow, st.flex_shrink, st.flex_basis) = (p(g)?, p(s)?, p(b)?),
                _ => return Err(format!("`{v}` is not a flex value")),
            }
        }
        "justify-content" => st.justify_content = p::<AlignContent>(v)?,
        "align-content" => st.align_content = p::<AlignContent>(v)?,
        "align-items" => st.align_items = p::<AlignItems>(v)?,
        "align-self" => st.align_self = Some(p::<AlignItems>(v)?),
        "justify-items" => st.justify_items = p::<AlignItems>(v)?,
        "justify-self" => st.justify_self = Some(p::<AlignItems>(v)?),
        "gap" | "grid-gap" => {
            let t = split_top(v, None);
            let (r, col) = match t.as_slice() {
                [a] => (p(a)?, p(a)?),
                [a, b] => (p(a)?, p(b)?),
                _ => return Err(format!("`{v}` is not a gap")),
            };
            st.gap = Size {
                width: col,
                height: r,
            };
        }
        "row-gap" => st.gap.height = p(v)?,
        "column-gap" => st.gap.width = p(v)?,
        "grid-template-columns" | "grid-template-rows" => {
            let tracks: Vec<GridTemplateComponent<String>> = if v == "none" {
                vec![]
            } else {
                p::<GridTemplateTracks<String, GridTemplateComponent<String>>>(v)?.tracks
            };
            if name.ends_with("columns") {
                st.grid_template_columns = tracks;
            } else {
                st.grid_template_rows = tracks;
            }
        }
        "grid-auto-columns" => st.grid_auto_columns = p::<GridAutoTracks>(v)?.0,
        "grid-auto-rows" => st.grid_auto_rows = p::<GridAutoTracks>(v)?.0,
        "grid-auto-flow" => st.grid_auto_flow = p::<GridAutoFlow>(v)?,
        "grid-column" => st.grid_column = line(v)?,
        "grid-row" => st.grid_row = line(v)?,
        "grid-column-start" => st.grid_column.start = p(v)?,
        "grid-column-end" => st.grid_column.end = p(v)?,
        "grid-row-start" => st.grid_row.start = p(v)?,
        "grid-row-end" => st.grid_row.end = p(v)?,
        "overflow" | "overflow-x" | "overflow-y" => {
            let (x, y) = match split_top(v, None).as_slice() {
                [a] => (ov(a)?, ov(a)?),
                [a, b] => (ov(a)?, ov(b)?),
                _ => return Err(format!("`{v}` is not an overflow")),
            };
            match name {
                "overflow" => (st.overflow.x, st.overflow.y) = (x, y),
                "overflow-x" => st.overflow.x = x,
                _ => st.overflow.y = y,
            }
        }
        "order" => c.order = p::<i32>(v)?.clamp(-1000, 1000),
        "z-index" => c.z = p::<i32>(v)?.clamp(-1000, 1000),
        "background" | "background-color" | "background-image" => c.bg = fill(v)?,
        "color" => c.color = value::color(v)?,
        "opacity" => c.opacity = value::number(v)?.clamp(0.0, 1.0),
        "box-shadow" => c.shadow = shadow(v)?,
        "font-size" => {
            c.font_size = match v.strip_suffix('%') {
                Some(n) => p::<f32>(n)? / 100.0 * c.font_size,
                None => px(v).ok_or_else(|| format!("`{v}` is not a font size"))?,
            }
            .clamp(6.0, 200.0)
        }
        "font-weight" => c.weight = weight(v)?,
        "line-height" => {
            c.line_height = match px(v) {
                Some(l) => l / c.font_size,
                None => value::number(v)?,
            }
            .clamp(0.8, 3.0)
        }
        "text-transform" => {
            c.text_transform = match v {
                "uppercase" => Transform::Upper,
                "lowercase" => Transform::Lower,
                "none" | "capitalize" => Transform::None,
                _ => return Err(format!("`{v}` is not a text-transform")),
            }
        }
        "text-align" => {
            // Taffy's own `text_align` is the legacy `<center>` behaviour for block children, so it stays unset.
            c.align = match v {
                "left" | "start" => Align::Left,
                "right" | "end" => Align::Right,
                "center" => Align::Center,
                _ => return Err(format!("`{v}` is not a text-align")),
            };
        }
        "white-space" => c.lines = (v == "nowrap").then_some(1),
        "-webkit-line-clamp" | "line-clamp" => {
            c.lines = if v == "none" {
                None
            } else {
                Some(p::<u16>(v)?.clamp(1, 12))
            }
        }
        n if LATER.contains(&n) => return Err(format!("`{n}` is planned but not drawn yet")),
        n => return Err(format!("`{n}` is not a supported property")),
    }
    Ok(())
}

fn is_length_prop(n: &str) -> bool {
    matches!(
        n,
        "width"
            | "height"
            | "min-width"
            | "min-height"
            | "max-width"
            | "max-height"
            | "top"
            | "right"
            | "bottom"
            | "left"
            | "inset"
            | "margin"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "padding"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
            | "gap"
            | "grid-gap"
            | "row-gap"
            | "column-gap"
            | "flex-basis"
            | "border-width"
    )
}

/// CSS allows a bare `0` for a length; Taffy's reader wants a unit.
fn zero_px(v: &str) -> String {
    split_top(v, None)
        .into_iter()
        .map(|t| if t == "0" { "0px" } else { t })
        .collect::<Vec<_>>()
        .join(" ")
}

fn ov(v: &str) -> Result<Overflow, String> {
    match v {
        "auto" => Ok(Overflow::Scroll),
        _ => p(v),
    }
}
