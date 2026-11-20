//! Declarations compiled once per rule into typed [`Op`]s, and the computed style they build.
//! `compile` reads the text (colours and gradients through `customize::value`, enums and grid tracks through Taffy's CSS parser)
//! and never looks at an element; `Op::apply` then writes the value into a [`Computed`], resolving the relative units
//! (`em`, `vw`, ...) against the element and the window. A declaration that contains `var()` is compiled after substitution.
use crate::paint::{self, Ease, Fill, Fit, Shadow, TP, TransSpec, Xf};
use crate::warn::Kind;
use nocturne_core::customize::value::{self, Color, split_top};
use std::collections::HashMap;
use std::rc::Rc;
use taffy::geometry::{Line, Rect, Size};
use taffy::style::{
    AlignContent, AlignItems, BoxSizing, Dimension, Display, FlexDirection, FlexWrap, GridAutoFlow,
    GridAutoTracks, GridPlacement, GridTemplateArea, GridTemplateAreas, GridTemplateComponent,
    GridTemplateTracks, LengthPercentage, LengthPercentageAuto, Overflow, Position, Style,
};

/// Lengths are clamped to this magnitude and non-finite results become zero (a hostile `1e38px` cannot reach Taffy).
const MAX_PX: f32 = 100_000.0;

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

/// Line height as written: a multiple of the font size, or absolute pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineH {
    Mult(f32),
    Px(f32),
}

impl LineH {
    pub fn ratio(self, font_size: f32) -> f32 {
        match self {
            LineH::Mult(m) => m,
            LineH::Px(p) => p / font_size.max(1.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    pub layout: Style,
    pub order: i32,
    pub z: i32,
    /// `position: fixed`: laid out against the window, painted at a fixed place on the screen while the page scrolls.
    pub fixed: bool,
    /// `position` other than `static`: the box absolute descendants are placed against.
    pub positioned: bool,
    /// Background layers, topmost first.
    pub bg: Rc<[Fill]>,
    /// The parts `bg` is made of, so that `background-color` and `background-image` set theirs without resetting the other: the image and gradient layers, and the colour layer.
    pub bg_img: Rc<[Fill]>,
    pub bg_col: Option<Fill>,
    /// The `url()` picture of the background (the address as written), and its `background-size`, `-position` and `-repeat`.
    pub pic_url: Option<Rc<str>>,
    pub pic_size: paint::BgSize,
    pub pic_pos: paint::BgPos,
    pub pic_repeat: (bool, bool),
    /// How many of `bg`'s layers are drawn over the picture.
    pub pic_above: usize,
    /// Border colours: top, right, bottom, left.
    pub border_color: [Color; 4],
    /// Corner radii: top-left, top-right, bottom-right, bottom-left.
    pub radius: [f32; 4],
    /// Box shadows (outer and inset), at most four.
    pub shadows: Rc<[Shadow]>,
    pub drop_shadow: Option<Shadow>,
    pub text_shadow: Option<Shadow>,
    /// `mask-image` alpha gradient; drawn as a scrim of the surface colour (see `Scene::scrim_color`).
    pub mask: Option<Fill>,
    pub fit: Fit,
    pub object_pos: (f32, f32),
    pub xf: Xf,
    pub trans: Rc<TransSpec>,
    /// `animation` (a `@keyframes` run), when one is named.
    pub anim: Option<Rc<paint::AnimSpec>>,
    /// `filter: brightness()` below 1: the element is dimmed with a black layer of that alpha.
    pub brightness: f32,
    pub opacity: f32,
    pub visible: bool,
    pub pointer_events: bool,
    /// Focus ring colour (`outline-color` or `--nocturne-focus`); the ring itself can never be removed.
    pub outline_color: Option<Color>,
    /// Text of a `::before` / `::after` (`content: "..."`).
    pub content: Option<Rc<str>>,
    // Inherited text properties.
    pub color: Color,
    pub font_size: f32,
    pub weight: u16,
    pub italic: bool,
    pub family: Rc<[String]>,
    pub line_height: LineH,
    pub letter_spacing: f32,
    pub underline: bool,
    pub text_transform: Transform,
    pub align: Align,
    /// `text-overflow: ellipsis` (not inherited).
    pub ellipsis: bool,
    /// `-webkit-line-clamp` / `white-space: nowrap` (1).
    pub lines: Option<u16>,
    /// Custom properties, inherited; shared with the parent unless the element declares its own.
    pub vars: Rc<HashMap<String, String>>,
}

impl Computed {
    /// The background picture with its size, position and repeat.
    pub fn pic(&self) -> Option<paint::Pic> {
        Some(paint::Pic {
            url: self.pic_url.clone()?,
            size: self.pic_size,
            pos: self.pic_pos,
            repeat: self.pic_repeat,
        })
    }

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
            positioned: false,
            bg: Rc::from(Vec::new()),
            bg_img: Rc::from(Vec::new()),
            bg_col: None,
            pic_url: None,
            pic_size: paint::BgSize::Auto,
            pic_pos: paint::BG_POS0,
            pic_repeat: (true, true),
            pic_above: 0,
            border_color: [Color::rgba(255, 255, 255, 40); 4],
            radius: [0.0; 4],
            shadows: Rc::from(Vec::new()),
            drop_shadow: None,
            text_shadow: None,
            mask: None,
            fit: Fit::Fill,
            object_pos: (0.5, 0.5),
            xf: Xf::IDENTITY,
            trans: Rc::default(),
            anim: None,
            brightness: 1.0,
            opacity: 1.0,
            visible: true,
            pointer_events: true,
            outline_color: None,
            content: None,
            color: Color::rgba(238, 238, 244, 255),
            font_size: 16.0,
            weight: 400,
            italic: false,
            family: Rc::from(Vec::new()),
            line_height: LineH::Mult(1.3),
            letter_spacing: 0.0,
            underline: false,
            text_transform: Transform::None,
            align: Align::Left,
            ellipsis: false,
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
            italic: p.italic,
            family: p.family.clone(),
            line_height: p.line_height,
            letter_spacing: p.letter_spacing,
            underline: p.underline,
            text_shadow: p.text_shadow,
            text_transform: p.text_transform,
            align: p.align,
            visible: p.visible,
            pointer_events: p.pointer_events,
            vars: p.vars.clone(),
            ..Computed::root()
        }
    }

    pub fn line_height_ratio(&self) -> f32 {
        self.line_height.ratio(self.font_size)
    }
}

/// Context for relative units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ctx {
    pub vw: f32,
    pub vh: f32,
    /// The root element's font size: what `rem` means (16 until the root's own style is known).
    pub rem: f32,
}

/// A length as written; resolved against the element's font size and the window when applied.
#[derive(Clone, Debug, PartialEq)]
pub enum Len {
    Auto,
    Px(f32),
    Pct(f32),
    Em(f32),
    /// `rem`: a multiple of the root element's font size.
    Rem(f32),
    Vw(f32),
    Vh(f32),
    Vmin(f32),
    Vmax(f32),
    /// `calc()`, `min()`, `max()`, `clamp()`; the flag says the result is a percentage (otherwise pixels).
    Expr(Rc<crate::calc::Expr>, bool),
}

fn clean(px: f32) -> f32 {
    if px.is_finite() {
        px.clamp(-MAX_PX, MAX_PX)
    } else {
        0.0
    }
}

impl Len {
    /// Pixels, for everything but `auto` and percentages.
    fn px(&self, font: f32, cx: Ctx) -> Option<f32> {
        Some(clean(match self {
            Len::Px(p) => *p,
            Len::Em(e) => e * font,
            Len::Rem(r) => r * cx.rem,
            Len::Vw(v) => v * cx.vw / 100.0,
            Len::Vh(v) => v * cx.vh / 100.0,
            Len::Vmin(v) => v * cx.vw.min(cx.vh) / 100.0,
            Len::Vmax(v) => v * cx.vw.max(cx.vh) / 100.0,
            Len::Expr(e, false) => e.eval(font, cx),
            Len::Auto | Len::Pct(_) | Len::Expr(_, true) => return None,
        }))
    }

    fn pct(&self, font: f32, cx: Ctx) -> Option<f32> {
        match self {
            Len::Pct(p) => Some(clean(*p) / 100.0),
            Len::Expr(e, true) => Some(clean(e.eval(font, cx)) / 100.0),
            _ => None,
        }
    }

    fn dim(&self, font: f32, cx: Ctx) -> Dimension {
        if *self == Len::Auto {
            return Dimension::auto();
        }
        match self.pct(font, cx) {
            Some(p) => Dimension::percent(p),
            None => Dimension::length(self.px(font, cx).unwrap_or(0.0)),
        }
    }

    fn lpa(&self, font: f32, cx: Ctx) -> LengthPercentageAuto {
        if *self == Len::Auto {
            return LengthPercentageAuto::auto();
        }
        match self.pct(font, cx) {
            Some(p) => LengthPercentageAuto::percent(p),
            None => LengthPercentageAuto::length(self.px(font, cx).unwrap_or(0.0)),
        }
    }

    fn lp(&self, font: f32, cx: Ctx) -> LengthPercentage {
        match self.pct(font, cx) {
            Some(p) => LengthPercentage::percent(p),
            None => LengthPercentage::length(self.px(font, cx).unwrap_or(0.0)),
        }
    }

    pub fn is_viewport(&self) -> bool {
        match self {
            Len::Vw(_) | Len::Vh(_) | Len::Vmin(_) | Len::Vmax(_) | Len::Rem(_) => true,
            Len::Expr(e, _) => e.uses_viewport(),
            _ => false,
        }
    }
}

type Sides = [Option<Len>; 4];

/// One typed change to a [`Computed`].
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Display(Display),
    /// The position, `fixed`, and whether it is anything but `static`.
    Position(Position, bool, bool),
    /// `inherit` of one of the inherited properties: the parent's value (named by the property).
    Inherit(&'static str),
    /// `float` and `clear`: Taffy's block layout floats boxes against the line boxes of their siblings (there is no inline text to wrap, so the boxes after a float flow beside it only as blocks do).
    Float(taffy::style::Float),
    Clear(taffy::style::Clear),
    Inset(Sides),
    Margin(Sides),
    Padding(Sides),
    BorderWidth(Sides),
    /// 0 width, 1 height, 2 min-width, 3 min-height, 4 max-width, 5 max-height.
    Size(u8, Len),
    Aspect(Option<f32>),
    BoxSizing(BoxSizing),
    /// Which sides (top, right, bottom, left) get this colour.
    BorderColor([bool; 4], Option<Color>),
    Radius([Option<f32>; 4]),
    FlexDir(FlexDirection),
    FlexWrap(FlexWrap),
    Grow(f32),
    Shrink(f32),
    Basis(Len),
    Justify(AlignContent),
    AlignContent(AlignContent),
    AlignItems(AlignItems),
    AlignSelf(AlignItems),
    JustifyItems(AlignItems),
    JustifySelf(AlignItems),
    /// (row gap, column gap)
    Gap(Option<Len>, Option<Len>),
    Tracks(bool, Vec<GridTemplateComponent<String>>),
    /// Tracks with `em`/`vw`/... in them: read again with the element's context.
    TracksRaw(bool, String),
    AutoTracks(bool, Vec<taffy::style::TrackSizingFunction>),
    AutoFlow(GridAutoFlow),
    /// (row start, column start, row end, column end); `None` leaves that edge alone.
    GridLines([Option<GridPlacement>; 4]),
    Areas(Option<GridTemplateAreas<String>>),
    Overflow(Option<Overflow>, Option<Overflow>),
    Order(i32),
    Z(i32),
    Bg(Rc<[Fill]>),
    /// `background-color` alone / `background-image` alone: the other part stays.
    BgColor(Option<Fill>),
    BgImage(Rc<[Fill]>),
    PicUrl(Option<Rc<str>>),
    PicSize(paint::BgSize),
    PicPos(paint::BgPos),
    PicRepeat((bool, bool)),
    PicAbove(usize),
    Color(Color),
    Opacity(f32),
    Shadows(Rc<[Shadow]>),
    TextShadow(Option<Shadow>),
    DropShadow(Option<Shadow>),
    Mask(Option<Fill>),
    ObjectFit(Fit),
    ObjectPos(f32, f32),
    LetterSpacing(Len),
    Ellipsis(bool),
    Underline(bool),
    Transform {
        tx: Len,
        ty: Len,
        sx: f32,
        sy: f32,
    },
    Origin(f32, f32),
    Anim(Rc<paint::AnimPatch>),
    Brightness(f32),
    TransAll(Rc<TransSpec>),
    TransProps(Vec<TP>),
    TransDur(Vec<f32>),
    TransDelay(Vec<f32>),
    TransEase(Vec<Ease>),
    FontSize(Len, bool),
    Weight(Weight),
    Italic(bool),
    Family(Rc<[String]>),
    LineHeight(LineHeightValue),
    TextTransform(Transform),
    TextAlign(Align),
    Lines(Option<u16>),
    /// `-webkit-line-clamp`: lines plus an ellipsis.
    Clamp(Option<u16>),
    Visible(bool),
    Pointer(bool),
    Outline(Option<Color>),
    Content(Option<Rc<str>>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Weight {
    Abs(u16),
    Bolder,
    Lighter,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LineHeightValue {
    Normal,
    Mult(f32),
    Len(Len),
}

/// A declaration that did not compile: what kind of problem, and the message.
#[derive(Debug, PartialEq)]
pub struct Issue {
    pub kind: Kind,
    pub msg: String,
}

fn bad<T>(msg: impl Into<String>) -> Result<T, Issue> {
    Err(Issue {
        kind: Kind::BadValue,
        msg: msg.into(),
    })
}

/// The result of compiling one declaration.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Ops(Vec<Op>),
    /// Valid, and meaningless here (browser-only): nothing to apply.
    NoEffect,
    /// Drawn through a documented fallback (`backdrop-filter`, `mask-image`): the ops to apply and what the fallback is.
    Approx(Vec<Op>, &'static str),
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
                "rem" => Some(k * cx.rem),
                "em" => Some(k * font),
                "pt" => Some(k * 4.0 / 3.0),
                _ => None,
            };
            match px {
                Some(p) => {
                    out.push_str(&format!("{:.2}px", clean(p)));
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

/// Does reading this text depend on the element's font size or the window size?
fn context_dependent(v: &str) -> bool {
    let a = units(
        v,
        Ctx {
            vw: 100.0,
            vh: 100.0,
            rem: 10.0,
        },
        10.0,
    );
    let b = units(
        v,
        Ctx {
            vw: 700.0,
            vh: 300.0,
            rem: 30.0,
        },
        30.0,
    );
    a != b
}

pub const MAX_VAR_LEN: usize = 4096;
pub const MAX_VAR_DEPTH: u8 = 8;

/// Replace `var(--x)` and `var(--x, fallback)`; bounded in depth and length so a hostile sheet cannot blow up, and a cycle
/// ends as an error at the depth limit.
pub fn substitute(v: &str, vars: &HashMap<String, String>, depth: u8) -> Result<String, String> {
    if !v.contains("var(") {
        return Ok(v.to_owned());
    }
    if depth > MAX_VAR_DEPTH {
        return Err("var() nested too deeply (or a cycle)".into());
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
        let val = match vars.get(name) {
            Some(v) => substitute(v, vars, depth + 1),
            None => match fb {
                Some(f) => substitute(f, vars, depth + 1),
                None => Err(format!("`{name}` is not defined")),
            },
        }?;
        out.push_str(&val);
        if out.len() > MAX_VAR_LEN {
            return Err("value too long after var()".into());
        }
        rest = &args[end + 1..];
    }
    out.push_str(rest);
    if out.len() > MAX_VAR_LEN {
        return Err("value too long after var()".into());
    }
    Ok(out)
}

fn p<T: std::str::FromStr>(v: &str) -> Result<T, Issue>
where
    T::Err: std::fmt::Display,
{
    v.trim()
        .parse::<T>()
        .or_else(|e| bad(format!("`{v}`: {e}")))
}

/// `auto`, `0`, a number with `px`, `%`, `em`, `rem`, `vw`, `vh`, `vmin`, `vmax`, `pt`, `ch`.
pub fn len(v: &str) -> Result<Len, Issue> {
    let v = v.trim();
    if v == "auto" {
        return Ok(Len::Auto);
    }
    if v.contains("env(") {
        return len(&crate::calc::strip_env(v));
    }
    if crate::calc::is_expr(v) {
        return crate::calc::parse(v);
    }
    if v.contains('(') {
        return bad(format!("`{v}`: this function is not supported in a length"));
    }
    let n = num_prefix(v);
    if n == 0 {
        return bad(format!("`{v}` is not a length"));
    }
    let k: f32 = v[..n]
        .parse()
        .or_else(|_| bad(format!("`{v}` is not a length")))?;
    if !k.is_finite() {
        return bad(format!("`{v}` is not a length"));
    }
    Ok(match v[n..].to_ascii_lowercase().as_str() {
        "px" => Len::Px(k),
        "" if k == 0.0 => Len::Px(0.0),
        "%" => Len::Pct(k),
        "em" => Len::Em(k),
        "rem" => Len::Rem(k),
        "ch" => Len::Em(k * 0.5),
        "pt" => Len::Px(k * 4.0 / 3.0),
        "vw" => Len::Vw(k),
        "vh" | "dvh" | "svh" | "lvh" => Len::Vh(k),
        "vmin" => Len::Vmin(k),
        "vmax" => Len::Vmax(k),
        u => return bad(format!("`{v}`: unit `{u}` is not supported")),
    })
}

fn sides(v: &str, auto: bool) -> Result<Sides, Issue> {
    let a: Vec<Len> = split_top(v, None)
        .into_iter()
        .map(len)
        .collect::<Result<_, _>>()?;
    if !auto && a.contains(&Len::Auto) {
        return bad(format!("`{v}`: `auto` is not allowed here"));
    }
    let (t, r, b, l) = match a.as_slice() {
        [x] => (x.clone(), x.clone(), x.clone(), x.clone()),
        [y, x] => (y.clone(), x.clone(), y.clone(), x.clone()),
        [t, x, b] => (t.clone(), x.clone(), b.clone(), x.clone()),
        [t, r, b, l] => (t.clone(), r.clone(), b.clone(), l.clone()),
        _ => return bad(format!("`{v}`: expected 1 to 4 values")),
    };
    Ok([Some(t), Some(r), Some(b), Some(l)])
}

fn one_side(v: &str, auto: bool, idx: usize) -> Result<Sides, Issue> {
    let l = len(v)?;
    if !auto && l == Len::Auto {
        return bad("`auto` is not allowed here");
    }
    let mut s: Sides = std::array::from_fn(|_| None);
    s[idx] = Some(l);
    Ok(s)
}

fn gridline(v: &str) -> Result<Line<GridPlacement>, Issue> {
    let (a, b) = match v.split_once('/') {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (v.trim(), None),
    };
    let start: GridPlacement = p(a)?;
    let end: GridPlacement = match b {
        Some(b) => p(b)?,
        None => GridPlacement::Auto,
    };
    Ok(Line { start, end })
}

fn weight(v: &str) -> Result<Weight, Issue> {
    match v.trim() {
        "normal" => Ok(Weight::Abs(400)),
        "bold" => Ok(Weight::Abs(700)),
        "lighter" => Ok(Weight::Lighter),
        "bolder" => Ok(Weight::Bolder),
        n => n
            .parse::<f32>()
            .ok()
            .filter(|w| (1.0..=1000.0).contains(w))
            .map(|w| Weight::Abs(w.round() as u16))
            .map_or_else(|| bad(format!("`{v}` is not a font weight")), Ok),
    }
}

impl Weight {
    fn resolve(self, inherited: u16) -> u16 {
        match self {
            Weight::Abs(w) => w,
            Weight::Bolder => match inherited {
                ..=399 => 400,
                400..=599 => 700,
                _ => 900,
            },
            Weight::Lighter => match inherited {
                ..=599 => 100,
                600..=799 => 400,
                _ => 700,
            },
        }
    }
}

fn color(v: &str) -> Result<Color, Issue> {
    value::color(v).or_else(bad)
}

/// A colour, or `None` for `currentcolor` (the element's own text colour, resolved when applied).
fn color_cur(v: &str) -> Result<Option<Color>, Issue> {
    if v.eq_ignore_ascii_case("currentcolor") {
        Ok(None)
    } else {
        color(v).map(Some)
    }
}

fn border_width_token(t: &str) -> Option<Len> {
    match t {
        "thin" => Some(Len::Px(1.0)),
        "medium" => Some(Len::Px(3.0)),
        "thick" => Some(Len::Px(5.0)),
        _ => len(t).ok().filter(|l| *l != Len::Auto),
    }
}

const BORDER_STYLES: &[&str] = &[
    "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset", "none", "hidden",
];

/// `border`, `border-top`...: width, style and colour in any order. A style of `none` (or none given, as in CSS) means no border.
fn border_ops(v: &str, mask: [bool; 4]) -> Result<Vec<Op>, Issue> {
    let (mut w, mut style, mut col) = (None, None, None::<Option<Color>>);
    for t in split_top(v, None) {
        if BORDER_STYLES.contains(&t) {
            style = Some(t);
        } else if let Some(l) = border_width_token(t) {
            w = Some(l);
        } else if let Ok(c) = color_cur(t) {
            col = Some(c);
        } else {
            return bad(format!("`{t}` is not supported in a border"));
        }
    }
    let on = !matches!(style, None | Some("none" | "hidden"));
    let width = if on {
        w.unwrap_or(Len::Px(3.0))
    } else {
        Len::Px(0.0)
    };
    let mut s: Sides = std::array::from_fn(|_| None);
    for (i, m) in mask.iter().enumerate() {
        if *m {
            s[i] = Some(width.clone());
        }
    }
    let mut ops = vec![Op::BorderWidth(s)];
    if let Some(c) = col {
        ops.push(Op::BorderColor(mask, c));
    }
    Ok(ops)
}

fn radius(v: &str) -> Result<[Option<f32>; 4], Issue> {
    if v.contains('/') {
        return bad("elliptical radii (`/`) are not supported");
    }
    let a: Vec<f32> = split_top(v, None)
        .into_iter()
        .map(|t| match t.strip_suffix('%') {
            // A percentage radius is shape-relative; 50% (a circle or a pill) is the case worth supporting.
            Some(n) if n.trim().parse::<f32>().is_ok_and(|n| n >= 50.0) => Ok(9999.0),
            Some(_) => bad(format!(
                "`{t}`: only 50% or more is supported as a radius percentage"
            )),
            None => match len(t)? {
                Len::Px(px) => Ok(px.max(0.0)),
                Len::Em(e) => Ok((e * 16.0).max(0.0)),
                Len::Rem(r) => Ok((r * 16.0).max(0.0)),
                _ => bad(format!("`{t}` is not a radius")),
            },
        })
        .collect::<Result<_, _>>()?;
    let [tl, tr, br, bl] = match a.as_slice() {
        [x] => [*x; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return bad(format!("`{v}`: expected 1 to 4 values")),
    };
    Ok([Some(tl), Some(tr), Some(br), Some(bl)])
}

fn overflow1(v: &str) -> Result<Overflow, Issue> {
    match v {
        // Scrollable: auto and scroll both scroll when the content is larger (the widget decides about bars).
        "auto" | "overlay" => Ok(Overflow::Scroll),
        // `clip` and `hidden` both clip.
        "clip" => Ok(Overflow::Hidden),
        _ => p(v),
    }
}

fn string_arg(v: &str) -> Option<String> {
    let q = v.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let body = v[1..].strip_suffix(q)?;
    let mut out = String::new();
    let mut it = body.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.peek().copied() {
            Some(h) if h.is_ascii_hexdigit() => {
                let mut hex = String::new();
                while hex.len() < 6 && it.peek().is_some_and(char::is_ascii_hexdigit) {
                    hex.push(it.next().unwrap_or('0'));
                }
                if it.peek() == Some(&' ') {
                    it.next();
                }
                out.push(
                    u32::from_str_radix(&hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or('\u{fffd}'),
                );
            }
            Some(o) => {
                out.push(o);
                it.next();
            }
            None => {}
        }
    }
    Some(out)
}

/// Longest generated text.
pub const MAX_CONTENT: usize = 200;

fn areas(v: &str) -> Result<Option<GridTemplateAreas<String>>, Issue> {
    if v == "none" {
        return Ok(None);
    }
    let mut rows: Vec<Vec<String>> = vec![];
    let mut rest = v.trim();
    while !rest.is_empty() {
        let q = rest.chars().next().unwrap_or(' ');
        if !matches!(q, '"' | '\'') {
            return bad("grid-template-areas expects quoted rows");
        }
        let Some(end) = rest[1..].find(q) else {
            return bad("unterminated string in grid-template-areas");
        };
        rows.push(
            rest[1..1 + end]
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        );
        rest = rest[end + 2..].trim_start();
    }
    let cols = rows.first().map_or(0, Vec::len);
    if rows.is_empty() || cols == 0 || rows.iter().any(|r| r.len() != cols) {
        return bad("grid-template-areas rows must all have the same number of cells");
    }
    if rows.len() > 64 || cols > 64 {
        return bad("grid-template-areas is too large");
    }
    let mut names: Vec<&str> = vec![];
    for n in rows.iter().flatten() {
        if n != "." && !names.contains(&n.as_str()) {
            names.push(n);
        }
    }
    let mut out = vec![];
    for name in names {
        let (mut r0, mut r1, mut c0, mut c1) = (usize::MAX, 0, usize::MAX, 0);
        let mut cells = 0;
        for (r, row) in rows.iter().enumerate() {
            for (c, n) in row.iter().enumerate() {
                if n == name {
                    r0 = r0.min(r);
                    r1 = r1.max(r);
                    c0 = c0.min(c);
                    c1 = c1.max(c);
                    cells += 1;
                }
            }
        }
        if cells != (r1 - r0 + 1) * (c1 - c0 + 1) {
            return bad(format!("area `{name}` is not a rectangle"));
        }
        out.push(GridTemplateArea {
            name: name.to_owned(),
            row_start: (r0 + 1) as u16,
            row_end: (r1 + 2) as u16,
            column_start: (c0 + 1) as u16,
            column_end: (c1 + 2) as u16,
        });
    }
    Ok(Some(GridTemplateAreas {
        areas: out.into_iter().collect(),
        row_count: rows.len() as u16,
        column_count: cols as u16,
    }))
}

fn tracks(v: &str) -> Result<Vec<GridTemplateComponent<String>>, Issue> {
    if v == "none" {
        return Ok(vec![]);
    }
    // `repeat(100000, 1fr)` would make Taffy allocate that many tracks.
    let mut r = v;
    while let Some(i) = r.find("repeat(") {
        let n: String = r[i + 7..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if n.parse::<u32>().is_ok_and(|n| n > 100) {
            return bad("repeat() count above 100 is not supported");
        }
        r = &r[i + 7..];
    }
    let t = p::<GridTemplateTracks<String, GridTemplateComponent<String>>>(v)?.tracks;
    if t.len() > 256 {
        return bad("more than 256 grid tracks");
    }
    Ok(t.into_iter().collect())
}

/// Properties that are valid and mean nothing in a desktop app.
const NO_EFFECT: &[&str] = &[
    "clip",
    "border-fit",
    "-webkit-border-fit",
    "-ms-overflow-style",
    "-ms-user-select",
    "-moz-user-select",
    "-webkit-user-select",
    "-webkit-text-size-adjust",
    "-webkit-tap-highlight-color",
    "color-scheme",
    "touch-action",
    "user-select",
    "-webkit-user-select",
    "-moz-user-select",
    "cursor",
    "scroll-behavior",
    "-webkit-overflow-scrolling",
    "appearance",
    "-webkit-appearance",
    "-moz-appearance",
    "will-change",
    "contain",
    "contain-intrinsic-size",
    "content-visibility",
    "scrollbar-width",
    "scrollbar-color",
    "scrollbar-gutter",
    "scroll-snap-type",
    "scroll-snap-align",
    "scroll-snap-stop",
    "scroll-padding",
    "scroll-margin",
    "overscroll-behavior",
    "overscroll-behavior-x",
    "overscroll-behavior-y",
    "tab-size",
    "text-size-adjust",
    "-webkit-text-size-adjust",
    "-moz-text-size-adjust",
    "-webkit-box-orient",
    "-webkit-print-color-adjust",
    "print-color-adjust",
    "forced-color-adjust",
    "caret-color",
    "accent-color",
    "resize",
    "isolation",
    "backface-visibility",
    "-webkit-backface-visibility",
    "perspective",
    "orphans",
    "widows",
    "page-break-inside",
    "break-inside",
    "list-style",
    "list-style-type",
    "list-style-position",
    "table-layout",
    "border-collapse",
    "border-spacing",
    "quotes",
    "-webkit-font-smoothing",
    "-moz-osx-font-smoothing",
    "font-optical-sizing",
    "font-display",
    "text-rendering",
    "font-synthesis",
    "font-kerning",
    "font-variant",
    "font-variant-ligatures",
    "font-variant-numeric",
    "font-variant-caps",
    "font-feature-settings",
    "font-variation-settings",
    "font-stretch",
    "image-rendering",
    "-webkit-touch-callout",
    "outline",
    "outline-style",
    "outline-width",
    "outline-offset",
];

const WEBKIT_ALIASES: &[&str] = &[
    "backdrop-filter",
    "mask-image",
    "mask",
    "box-shadow",
    "border-radius",
    "transition",
    "transform",
    "transform-origin",
    "filter",
    "flex",
    "flex-direction",
    "flex-wrap",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "align-items",
    "align-self",
    "justify-content",
    "box-sizing",
    "text-decoration",
    "order",
    "flex-flow",
    "align-content",
    "animation",
    "animation-name",
    "animation-duration",
    "animation-delay",
    "animation-iteration-count",
    "animation-timing-function",
    "animation-fill-mode",
    "user-select",
    "appearance",
    "text-size-adjust",
    "overflow-style",
];

/// Valid CSS that the engine plans to draw but does not yet.
const PLANNED: &[&str] = &[
    "word-spacing",
    "text-decoration-color",
    "background-attachment",
    "background-clip",
    "background-origin",
    "mix-blend-mode",
    "clip-path",
    "word-break",
    "overflow-wrap",
    "word-wrap",
    "grid-template",
    "text-wrap",
    "vertical-align",
    "columns",
    "column-count",
];

fn logical_sides(name: &str) -> Option<(&str, [usize; 2], bool)> {
    // `margin-inline` -> left/right; `padding-block-start` -> top; start/end follow left-to-right text.
    let (base, rest) = name.split_once('-')?;
    if !matches!(base, "margin" | "padding" | "inset") {
        return None;
    }
    let (axis_sides, single): ([usize; 2], Option<usize>) = match rest {
        "inline" => ([3, 1], None),
        "block" => ([0, 2], None),
        "inline-start" => ([3, 3], Some(0)),
        "inline-end" => ([1, 1], Some(0)),
        "block-start" => ([0, 0], Some(0)),
        "block-end" => ([2, 2], Some(0)),
        _ => return None,
    };
    Some((base, axis_sides, single.is_some()))
}

/// The initial value of a property that is not inherited, as CSS text (the common box properties only).
fn initial_of(name: &str) -> Option<&'static str> {
    Some(match name {
        "margin" | "padding" | "border-radius" | "gap" | "row-gap" | "column-gap" => "0",
        n if n.starts_with("margin-") || n.starts_with("padding-") => "0",
        "width" | "height" | "top" | "right" | "bottom" | "left" | "inset" | "flex-basis"
        | "min-width" | "min-height" | "z-index" | "aspect-ratio" => "auto",
        "max-width" | "max-height" | "transform" | "box-shadow" | "filter" | "mask-image"
        | "background-image" | "animation-name" => "none",
        "background" => "none",
        "background-color" => "transparent",
        "opacity" => "1",
        "position" => "static",
        "overflow" | "overflow-x" | "overflow-y" => "visible",
        "flex-grow" => "0",
        "flex-shrink" => "1",
        _ => return None,
    })
}

/// Compile one declaration (the value must not contain `var()`).
pub fn compile(name: &str, value: &str) -> Result<Outcome, Issue> {
    // Vendor-prefixed spellings of properties that are standard here.
    let name = match ["-webkit-", "-moz-", "-ms-"]
        .iter()
        .find_map(|pre| name.strip_prefix(pre))
    {
        Some(n) if WEBKIT_ALIASES.contains(&n) => n,
        _ => name,
    };
    let v = value.trim();
    // CSS-wide keywords: `inherit` (and `unset`, which is `inherit` for the inherited properties) takes the parent's value; on a property that is not inherited and not tracked here it is silently dropped.
    // `initial` (and `unset` on a property that is not inherited) puts back the default: a theme that writes `margin-top: unset` takes away jellyfin-web's own negative margin.
    if matches!(v, "initial" | "unset" | "revert")
        && let Some(init) = initial_of(name)
    {
        return compile(name, init);
    }
    if matches!(v, "inherit" | "unset") {
        let kind: Option<&'static str> = match name {
            "color" => Some("color"),
            "font-size" => Some("font-size"),
            "font-weight" => Some("font-weight"),
            "font-style" => Some("font-style"),
            "font-family" => Some("font-family"),
            "line-height" => Some("line-height"),
            "letter-spacing" => Some("letter-spacing"),
            "text-align" => Some("text-align"),
            "text-transform" => Some("text-transform"),
            _ => None,
        };
        return Ok(match kind {
            Some(k) => Outcome::Ops(vec![Op::Inherit(k)]),
            None => Outcome::NoEffect,
        });
    }
    // Prefixed values of the flex displays.
    let v = match v {
        "-webkit-flex" | "-ms-flexbox" => "flex",
        "-webkit-inline-flex" | "-ms-inline-flexbox" => "inline-flex",
        _ => v,
    };
    if let Some((base, [a, b], single)) = logical_sides(name) {
        let auto = base != "padding";
        let l = if single {
            one_side(v, auto, a)?
        } else {
            let two = split_top(v, None);
            let (x, y) = match two.as_slice() {
                [x] => (*x, *x),
                [x, y] => (*x, *y),
                _ => return bad(format!("`{v}`: expected 1 or 2 values")),
            };
            let mut s: Sides = std::array::from_fn(|_| None);
            s[a] = Some(len(x)?);
            s[b] = Some(len(y)?);
            s
        };
        return Ok(Outcome::Ops(vec![match base {
            "margin" => Op::Margin(l),
            "padding" => Op::Padding(l),
            _ => Op::Inset(l),
        }]));
    }
    let one = |op: Op| Ok(Outcome::Ops(vec![op]));
    match name {
        // The stage-1 layout dialect (the built-in Marquee and Classic examples): the app's layout layer reads these, the engine draws what its own rules say.
        "display" if v == "carousel" => Ok(Outcome::Ops(vec![])),
        "content" if v == "server" => Ok(Outcome::Ops(vec![])),
        "display" => one(Op::Display(match v {
            // Approximations: Taffy has no inline formatting and no `contents`.
            "inline" | "inline-block" | "contents" | "list-item" | "table" | "-webkit-box" => {
                Display::Block
            }
            "inline-flex" | "-webkit-inline-box" => Display::Flex,
            "inline-grid" => Display::Grid,
            _ => p(v)?,
        })),
        "float" => one(Op::Float(p(v)?)),
        "clear" => one(Op::Clear(p(v)?)),
        "position" => one(Op::Position(
            match v {
                "fixed" | "sticky" => Position::Absolute,
                _ => p(v)?,
            },
            v == "fixed",
            v != "static",
        )),
        "top" => one(Op::Inset(one_side(v, true, 0)?)),
        "right" => one(Op::Inset(one_side(v, true, 1)?)),
        "bottom" => one(Op::Inset(one_side(v, true, 2)?)),
        "left" => one(Op::Inset(one_side(v, true, 3)?)),
        "inset" => one(Op::Inset(sides(v, true)?)),
        "width" => one(Op::Size(0, len(v)?)),
        "height" => one(Op::Size(1, len(v)?)),
        "min-width" => one(Op::Size(2, len(v)?)),
        "min-height" => one(Op::Size(3, len(v)?)),
        "max-width" | "max-height" => {
            let l = if v == "none" { Len::Auto } else { len(v)? };
            one(Op::Size(if name == "max-width" { 4 } else { 5 }, l))
        }
        "aspect-ratio" => one(Op::Aspect(
            match v.split_once('/') {
                Some((a, b)) => {
                    let (a, b) = (p::<f32>(a)?, p::<f32>(b)?);
                    if b == 0.0 {
                        return bad("aspect-ratio with a zero side");
                    }
                    Some(a / b)
                }
                None if v == "auto" => None,
                None => Some(p(v)?),
            }
            .filter(|r: &f32| r.is_finite() && *r > 0.0),
        )),
        "box-sizing" => one(Op::BoxSizing(p(v)?)),
        "margin" => one(Op::Margin(sides(v, true)?)),
        "margin-top" => one(Op::Margin(one_side(v, true, 0)?)),
        "margin-right" => one(Op::Margin(one_side(v, true, 1)?)),
        "margin-bottom" => one(Op::Margin(one_side(v, true, 2)?)),
        "margin-left" => one(Op::Margin(one_side(v, true, 3)?)),
        "padding" => one(Op::Padding(sides(v, false)?)),
        "padding-top" => one(Op::Padding(one_side(v, false, 0)?)),
        "padding-right" => one(Op::Padding(one_side(v, false, 1)?)),
        "padding-bottom" => one(Op::Padding(one_side(v, false, 2)?)),
        "padding-left" => one(Op::Padding(one_side(v, false, 3)?)),
        "border" => Ok(Outcome::Ops(border_ops(v, [true; 4])?)),
        "border-top" => Ok(Outcome::Ops(border_ops(v, [true, false, false, false])?)),
        "border-right" => Ok(Outcome::Ops(border_ops(v, [false, true, false, false])?)),
        "border-bottom" => Ok(Outcome::Ops(border_ops(v, [false, false, true, false])?)),
        "border-left" => Ok(Outcome::Ops(border_ops(v, [false, false, false, true])?)),
        "border-width" => {
            let a: Vec<Len> = split_top(v, None)
                .into_iter()
                .map(|t| {
                    border_width_token(t)
                        .map_or_else(|| bad(format!("`{t}` is not a border width")), Ok)
                })
                .collect::<Result<_, _>>()?;
            let s = match a.as_slice() {
                [x] => [x.clone(), x.clone(), x.clone(), x.clone()],
                [y, x] => [y.clone(), x.clone(), y.clone(), x.clone()],
                [t, x, b] => [t.clone(), x.clone(), b.clone(), x.clone()],
                [t, r, b, l] => [t.clone(), r.clone(), b.clone(), l.clone()],
                _ => return bad(format!("`{v}`: expected 1 to 4 values")),
            };
            one(Op::BorderWidth(s.map(Some)))
        }
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => {
            let idx = ["top", "right", "bottom", "left"]
                .iter()
                .position(|s| name.contains(s))
                .unwrap_or(0);
            let l = border_width_token(v)
                .map_or_else(|| bad(format!("`{v}` is not a border width")), Ok)?;
            let mut s: Sides = std::array::from_fn(|_| None);
            s[idx] = Some(l);
            one(Op::BorderWidth(s))
        }
        "border-style" => {
            if v.split_whitespace().all(|t| matches!(t, "none" | "hidden")) {
                one(Op::BorderWidth(std::array::from_fn(|_| Some(Len::Px(0.0)))))
            } else {
                Ok(Outcome::NoEffect)
            }
        }
        "border-color" => {
            let a: Vec<Color> = split_top(v, None)
                .into_iter()
                .map(color)
                .collect::<Result<_, _>>()?;
            let (t, r, b, l) = match a.as_slice() {
                [x] => (*x, *x, *x, *x),
                [y, x] => (*y, *x, *y, *x),
                [t, x, b] => (*t, *x, *b, *x),
                [t, r, b, l] => (*t, *r, *b, *l),
                _ => return bad(format!("`{v}`: expected 1 to 4 values")),
            };
            Ok(Outcome::Ops(vec![
                Op::BorderColor([true, false, false, false], Some(t)),
                Op::BorderColor([false, true, false, false], Some(r)),
                Op::BorderColor([false, false, true, false], Some(b)),
                Op::BorderColor([false, false, false, true], Some(l)),
            ]))
        }
        "border-top-color" => one(Op::BorderColor([true, false, false, false], color_cur(v)?)),
        "border-right-color" => one(Op::BorderColor([false, true, false, false], color_cur(v)?)),
        "border-bottom-color" => one(Op::BorderColor([false, false, true, false], color_cur(v)?)),
        "border-left-color" => one(Op::BorderColor([false, false, false, true], color_cur(v)?)),
        "border-radius" => one(Op::Radius(radius(v)?)),
        "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-right-radius"
        | "border-bottom-left-radius" => {
            let r = radius(v.split_whitespace().next().unwrap_or(v))?[0];
            let mut a = [None; 4];
            let idx = match name {
                "border-top-left-radius" => 0,
                "border-top-right-radius" => 1,
                "border-bottom-right-radius" => 2,
                _ => 3,
            };
            a[idx] = r;
            one(Op::Radius(a))
        }
        "flex-direction" => one(Op::FlexDir(p(v)?)),
        "flex-wrap" => one(Op::FlexWrap(p(v)?)),
        "flex-flow" => {
            let mut ops = vec![];
            for t in split_top(v, None) {
                match (p::<FlexDirection>(t), p::<FlexWrap>(t)) {
                    (Ok(d), _) => ops.push(Op::FlexDir(d)),
                    (_, Ok(w)) => ops.push(Op::FlexWrap(w)),
                    _ => return bad(format!("`{t}` is not a flex-flow value")),
                }
            }
            Ok(Outcome::Ops(ops))
        }
        "flex-grow" => one(Op::Grow(p::<f32>(v)?.max(0.0))),
        "flex-shrink" => one(Op::Shrink(p::<f32>(v)?.max(0.0))),
        "flex-basis" => one(Op::Basis(if v == "content" { Len::Auto } else { len(v)? })),
        "flex" => {
            let t = split_top(v, None);
            let ops = match t.as_slice() {
                ["none"] => vec![Op::Grow(0.0), Op::Shrink(0.0), Op::Basis(Len::Auto)],
                ["auto"] => vec![Op::Grow(1.0), Op::Shrink(1.0), Op::Basis(Len::Auto)],
                ["initial"] => vec![Op::Grow(0.0), Op::Shrink(1.0), Op::Basis(Len::Auto)],
                [g] if p::<f32>(g).is_ok() => {
                    vec![Op::Grow(p(g)?), Op::Shrink(1.0), Op::Basis(Len::Px(0.0))]
                }
                [b] => vec![Op::Grow(1.0), Op::Shrink(1.0), Op::Basis(len(b)?)],
                [g, s] => match p::<f32>(s) {
                    Ok(s) => vec![Op::Grow(p(g)?), Op::Shrink(s), Op::Basis(Len::Px(0.0))],
                    Err(_) => vec![Op::Grow(p(g)?), Op::Shrink(1.0), Op::Basis(len(s)?)],
                },
                [g, s, b] => vec![Op::Grow(p(g)?), Op::Shrink(p(s)?), Op::Basis(len(b)?)],
                _ => return bad(format!("`{v}` is not a flex value")),
            };
            Ok(Outcome::Ops(ops))
        }
        "justify-content" => one(Op::Justify(p(v)?)),
        "align-content" => one(Op::AlignContent(p(v)?)),
        "align-items" => one(Op::AlignItems(p(v)?)),
        "align-self" => one(Op::AlignSelf(p(v)?)),
        "justify-items" => one(Op::JustifyItems(p(v)?)),
        "justify-self" => one(Op::JustifySelf(p(v)?)),
        "gap" | "grid-gap" => {
            let t = split_top(v, None);
            let ok = |l: Len| {
                if l == Len::Auto {
                    bad("`auto` is not a gap")
                } else {
                    Ok(l)
                }
            };
            match t.as_slice() {
                [a] => one(Op::Gap(Some(ok(len(a)?)?), Some(ok(len(a)?)?))),
                [a, b] => one(Op::Gap(Some(ok(len(a)?)?), Some(ok(len(b)?)?))),
                _ => bad(format!("`{v}` is not a gap")),
            }
        }
        "row-gap" | "grid-row-gap" => one(Op::Gap(Some(len(v)?), None)),
        "column-gap" | "grid-column-gap" => one(Op::Gap(None, Some(len(v)?))),
        "grid-template-columns" | "grid-template-rows" => {
            let cols = name.ends_with("columns");
            if context_dependent(v) {
                // Validate now with a stand-in context so a broken value is reported once, at the sheet.
                tracks(&units(
                    v,
                    Ctx {
                        vw: 1000.0,
                        vh: 1000.0,
                        rem: 16.0,
                    },
                    16.0,
                ))?;
                one(Op::TracksRaw(cols, v.to_owned()))
            } else {
                one(Op::Tracks(
                    cols,
                    tracks(&units(
                        v,
                        Ctx {
                            vw: 0.0,
                            vh: 0.0,
                            rem: 16.0,
                        },
                        16.0,
                    ))?,
                ))
            }
        }
        "grid-template-areas" => one(Op::Areas(areas(v)?)),
        "grid-auto-columns" => one(Op::AutoTracks(
            true,
            p::<GridAutoTracks>(&units(
                v,
                Ctx {
                    vw: 0.0,
                    vh: 0.0,
                    rem: 16.0,
                },
                16.0,
            ))?
            .0,
        )),
        "grid-auto-rows" => one(Op::AutoTracks(
            false,
            p::<GridAutoTracks>(&units(
                v,
                Ctx {
                    vw: 0.0,
                    vh: 0.0,
                    rem: 16.0,
                },
                16.0,
            ))?
            .0,
        )),
        "grid-auto-flow" => one(Op::AutoFlow(p(v)?)),
        "grid-column" | "grid-row" => {
            let l = gridline(v)?;
            let mut a: [Option<GridPlacement>; 4] = [None, None, None, None];
            if name == "grid-column" {
                (a[1], a[3]) = (Some(l.start), Some(l.end));
            } else {
                (a[0], a[2]) = (Some(l.start), Some(l.end));
            }
            one(Op::GridLines(a))
        }
        "grid-row-start" => one(Op::GridLines([Some(p(v)?), None, None, None])),
        "grid-column-start" => one(Op::GridLines([None, Some(p(v)?), None, None])),
        "grid-row-end" => one(Op::GridLines([None, None, Some(p(v)?), None])),
        "grid-column-end" => one(Op::GridLines([None, None, None, Some(p(v)?)])),
        "grid-area" => {
            let parts = split_top(v, Some('/'));
            if let [one_name] = parts.as_slice()
                && one_name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                && !matches!(*one_name, "auto" | "span")
                && !one_name.starts_with("span ")
            {
                let n = || GridPlacement::NamedLine((*one_name).to_owned(), 0);
                return one(Op::GridLines([Some(n()), Some(n()), Some(n()), Some(n())]));
            }
            let mut a: [Option<GridPlacement>; 4] = [None, None, None, None];
            if parts.len() > 4 {
                return bad(format!("`{v}` is not a grid-area"));
            }
            for (i, t) in parts.iter().enumerate() {
                a[i] = Some(p(t)?);
            }
            one(Op::GridLines(a))
        }
        "overflow" | "overflow-x" | "overflow-y" => {
            let (x, y) = match split_top(v, None).as_slice() {
                [a] => (overflow1(a)?, overflow1(a)?),
                [a, b] => (overflow1(a)?, overflow1(b)?),
                _ => return bad(format!("`{v}` is not an overflow")),
            };
            one(match name {
                "overflow" => Op::Overflow(Some(x), Some(y)),
                "overflow-x" => Op::Overflow(Some(x), None),
                _ => Op::Overflow(None, Some(y)),
            })
        }
        "order" => one(Op::Order(p::<i32>(v)?.clamp(-1000, 1000))),
        "z-index" => one(Op::Z(if v == "auto" {
            0
        } else {
            p::<i32>(v)?.clamp(-1000, 1000)
        })),
        "background" => {
            let mut ops = vec![Op::Bg(paint::layers(v).or_else(bad)?.into())];
            let p = paint::pic(v);
            ops.push(Op::PicUrl(p.as_ref().map(|p| p.url.clone())));
            ops.push(Op::PicSize(
                p.as_ref().map_or(paint::BgSize::Auto, |p| p.size),
            ));
            ops.push(Op::PicPos(p.as_ref().map_or(paint::BG_POS0, |p| p.pos)));
            ops.push(Op::PicRepeat(p.as_ref().map_or((true, true), |p| p.repeat)));
            ops.push(Op::PicAbove(paint::pic_above(v)));
            Ok(Outcome::Ops(ops))
        }
        "background-size" | "background-position" | "background-repeat" => {
            let l = v.to_ascii_lowercase();
            let first = split_top(&l, Some(',')).into_iter().next().unwrap_or("");
            let toks = split_top(first, None);
            let op = match name {
                "background-size" => paint::bg_size(&toks).map(Op::PicSize),
                "background-position" => paint::bg_pos(&toks).map(Op::PicPos),
                _ => paint::bg_repeat(&toks).map(Op::PicRepeat),
            };
            match op {
                Some(op) => one(op),
                None => bad(format!("`{v}` is not supported for {name}")),
            }
        }
        "background-color" => one(Op::BgColor(
            paint::layers(v)
                .or_else(bad)?
                .into_iter()
                .find(|f| matches!(f, Fill::Solid(_))),
        )),
        "background-image" => Ok(Outcome::Ops(vec![
            Op::BgImage(paint::layers(v).or_else(bad)?.into()),
            Op::PicUrl(paint::pic(v).map(|p| p.url)),
            Op::PicAbove(paint::pic_above(v)),
        ])),
        "color" => one(Op::Color(color(v)?)),
        "opacity" => one(Op::Opacity(
            match v.strip_suffix('%') {
                Some(n) => p::<f32>(n)? / 100.0,
                None => value::number(v).or_else(bad)?,
            }
            .clamp(0.0, 1.0),
        )),
        "box-shadow" => one(Op::Shadows(
            paint::shadows(v, true, true).or_else(bad)?.into(),
        )),
        "text-shadow" => one(Op::TextShadow(
            paint::shadows(v, false, false)
                .or_else(bad)?
                .into_iter()
                .next(),
        )),
        "filter" | "-webkit-filter" => {
            let (mut ds, mut bright, mut other) = (None, 1.0, None);
            for f in split_top(v, None) {
                if let Some(a) = f
                    .strip_prefix("drop-shadow(")
                    .and_then(|x| x.strip_suffix(')'))
                {
                    ds = paint::shadows(a, false, false)
                        .or_else(bad)?
                        .into_iter()
                        .next();
                } else if let Some(a) = f
                    .strip_prefix("brightness(")
                    .and_then(|x| x.strip_suffix(')'))
                {
                    bright = match a.trim().strip_suffix('%') {
                        Some(n) => p::<f32>(n)? / 100.0,
                        None => value::number(a).or_else(bad)?,
                    }
                    .clamp(0.0, 1.0);
                } else if f != "none" {
                    other = Some(f);
                }
            }
            let ops = vec![Op::DropShadow(ds), Op::Brightness(bright)];
            match other {
                Some(f) => Ok(Outcome::Approx(
                    ops,
                    // Leaked once per distinct filter would be overkill: a fixed text.
                    if f.starts_with("blur(") {
                        "filter: blur() is not drawn (drop-shadow() and brightness() are)"
                    } else {
                        "filter: only drop-shadow() and brightness() are drawn"
                    },
                )),
                None => Ok(Outcome::Ops(ops)),
            }
        }
        "backdrop-filter" => Ok(Outcome::Approx(
            vec![],
            "backdrop-filter has no blur here: the element keeps its own translucent fill",
        )),
        "mask-image" | "mask" => {
            let f = paint::layers(v).or_else(bad)?;
            match f.into_iter().next() {
                Some(g @ Fill::Linear { .. }) => Ok(Outcome::Approx(
                    vec![Op::Mask(Some(g))],
                    "mask-image is drawn as a scrim of the surface colour behind the element",
                )),
                _ => Ok(Outcome::Approx(
                    vec![Op::Mask(None)],
                    "mask-image other than a linear gradient is ignored",
                )),
            }
        }
        "object-fit" => one(Op::ObjectFit(match v {
            "fill" => Fit::Fill,
            "contain" => Fit::Contain,
            "cover" => Fit::Cover,
            "none" => Fit::None,
            "scale-down" => Fit::ScaleDown,
            _ => return bad(format!("`{v}` is not an object-fit")),
        })),
        "object-position" => {
            let (x, y) = position(v)?;
            one(Op::ObjectPos(x, y))
        }
        "letter-spacing" => one(Op::LetterSpacing(if v == "normal" {
            Len::Px(0.0)
        } else {
            len(v)?
        })),
        "text-overflow" => one(Op::Ellipsis(match v {
            "ellipsis" => true,
            "clip" => false,
            _ => return bad(format!("`{v}` is not a text-overflow")),
        })),
        "text-decoration" | "text-decoration-line" => {
            if v.split_whitespace().any(|t| t == "underline") {
                one(Op::Underline(true))
            } else if v.split_whitespace().any(|t| t == "none") {
                one(Op::Underline(false))
            } else {
                Ok(Outcome::NoEffect)
            }
        }
        "transform" => one(transform(v)?),
        "transform-origin" => {
            let (x, y) = position(v)?;
            one(Op::Origin(x, y))
        }
        "animation" => one(Op::Anim(Rc::new(paint::animation(v).or_else(bad)?))),
        "animation-name"
        | "animation-duration"
        | "animation-delay"
        | "animation-iteration-count"
        | "animation-timing-function"
        | "animation-fill-mode"
        | "animation-direction" => one(Op::Anim(Rc::new(
            paint::animation_part(name, v).or_else(bad)?,
        ))),
        "transition" => one(Op::TransAll(Rc::new(paint::transition(v).or_else(bad)?))),
        "transition-property" => one(Op::TransProps(
            split_top(v, Some(','))
                .into_iter()
                .filter(|t| *t != "none")
                .map(|t| {
                    if t == "all" {
                        TP::All
                    } else {
                        paint::aprop(t).map_or(TP::Other, TP::One)
                    }
                })
                .collect(),
        )),
        "transition-duration" | "transition-delay" => {
            let l: Vec<f32> = split_top(v, Some(','))
                .into_iter()
                .map(|t| time_ms(t).map_or_else(|| bad(format!("`{t}` is not a time")), Ok))
                .collect::<Result<_, _>>()?;
            one(if name.ends_with("duration") {
                Op::TransDur(l)
            } else {
                Op::TransDelay(l)
            })
        }
        "transition-timing-function" => one(Op::TransEase(
            split_top(v, Some(','))
                .into_iter()
                .map(|t| Ease::parse(t).map_or_else(|| bad(format!("`{t}` is not an easing")), Ok))
                .collect::<Result<_, _>>()?,
        )),
        "font" => Ok(Outcome::Ops(font_shorthand(v)?)),
        "font-size" => {
            let (l, rel) = match v {
                "xx-small" => (Len::Px(9.0), false),
                "x-small" => (Len::Px(10.0), false),
                "small" => (Len::Px(13.0), false),
                "medium" => (Len::Px(16.0), false),
                "large" => (Len::Px(18.0), false),
                "x-large" => (Len::Px(24.0), false),
                "xx-large" => (Len::Px(32.0), false),
                "larger" => (Len::Em(1.2), false),
                "smaller" => (Len::Em(1.0 / 1.2), false),
                _ => match len(v)? {
                    Len::Pct(n) => (Len::Em(n / 100.0), false),
                    Len::Auto => return bad("`auto` is not a font size"),
                    l => (l, false),
                },
            };
            one(Op::FontSize(l, rel))
        }
        "font-weight" => one(Op::Weight(weight(v)?)),
        "font-style" => one(Op::Italic(match v {
            "italic" | "oblique" => true,
            "normal" => false,
            _ if v.starts_with("oblique ") => true,
            _ => return bad(format!("`{v}` is not a font style")),
        })),
        "font-family" => {
            let list: Vec<String> = split_top(v, Some(','))
                .into_iter()
                .map(|f| f.trim_matches(['"', '\'']).trim().to_owned())
                .filter(|f| !f.is_empty())
                .take(16)
                .collect();
            if list.is_empty() {
                return bad("empty font-family");
            }
            one(Op::Family(list.into()))
        }
        "line-height" => one(Op::LineHeight(match v {
            "normal" => LineHeightValue::Normal,
            _ => match len(v) {
                Ok(Len::Px(0.0)) if v == "0" => LineHeightValue::Mult(0.0),
                Ok(Len::Pct(n)) => LineHeightValue::Mult(n / 100.0),
                Ok(Len::Em(n)) => LineHeightValue::Mult(n),
                Ok(l) if l != Len::Auto => LineHeightValue::Len(l),
                _ => LineHeightValue::Mult(value::number(v).or_else(bad)?),
            },
        })),
        "text-transform" => one(Op::TextTransform(match v {
            "uppercase" => Transform::Upper,
            "lowercase" => Transform::Lower,
            "none" | "capitalize" => Transform::None,
            _ => return bad(format!("`{v}` is not a text-transform")),
        })),
        "text-align" => one(Op::TextAlign(match v {
            "left" | "start" | "justify" => Align::Left,
            "right" | "end" => Align::Right,
            "center" => Align::Center,
            _ => return bad(format!("`{v}` is not a text-align")),
        })),
        "white-space" => one(Op::Lines(match v {
            "nowrap" | "pre" => Some(1),
            "normal" | "pre-wrap" | "pre-line" | "break-spaces" => None,
            _ => return bad(format!("`{v}` is not a white-space value")),
        })),
        "-webkit-line-clamp" | "line-clamp" => one(Op::Clamp(if v == "none" {
            None
        } else {
            Some(p::<u16>(v)?.clamp(1, 12))
        })),
        "visibility" => one(Op::Visible(match v {
            "visible" => true,
            "hidden" | "collapse" => false,
            _ => return bad(format!("`{v}` is not a visibility")),
        })),
        "pointer-events" => one(Op::Pointer(!matches!(v, "none"))),
        "outline-color" => one(Op::Outline(Some(color(v)?))),
        // A picture as the content of an element (an `img` showing its logo): drawn like a background picture, contained.
        "content" if paint::url_call(v).is_some_and(|(_, r)| r.start == 0) => {
            let (url, _) = paint::url_call(v).unwrap_or_default();
            Ok(Outcome::Ops(vec![
                Op::Content(None),
                Op::PicUrl(Some(url.as_str().into())),
                Op::PicSize(paint::BgSize::Contain),
                Op::PicPos(paint::BgPos {
                    x: (0.0, 0.0),
                    y: (0.5, 0.0),
                }),
                Op::PicRepeat((false, false)),
                Op::PicAbove(0),
            ]))
        }
        "content" => one(Op::Content(match v {
            "none" | "normal" => None,
            // `attr(title)`: kept as a marker (NUL + name); the cascade reads the attribute of the element that owns the pseudo-element.
            a if a.to_ascii_lowercase().starts_with("attr(") && a.ends_with(')') => {
                let name = a[5..a.len() - 1].trim();
                if name.is_empty()
                    || !name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return bad("only `attr(name)` with a plain attribute name is supported");
                }
                Some(Rc::from(format!("\u{0}{name}")))
            }
            _ => match string_arg(v) {
                Some(s) => Some(Rc::from(s.chars().take(MAX_CONTENT).collect::<String>())),
                None => return bad("only `content: \"text\"` is supported"),
            },
        })),
        n if NO_EFFECT.contains(&n) => Ok(Outcome::NoEffect),
        n if PLANNED.contains(&n) => Err(Issue {
            kind: Kind::Planned,
            msg: format!("`{n}` is planned but not drawn yet"),
        }),
        n => Err(Issue {
            kind: Kind::UnsupportedProperty,
            msg: format!("`{n}` is not a supported property"),
        }),
    }
}

fn time_ms(t: &str) -> Option<f32> {
    let t = t.trim();
    if let Some(ms) = t.strip_suffix("ms") {
        return ms.trim().parse().ok();
    }
    t.strip_suffix('s')?
        .trim()
        .parse::<f32>()
        .ok()
        .map(|s| s * 1000.0)
}

/// `center`, `left top`, `50% 25%` as fractions of the box.
fn position(v: &str) -> Result<(f32, f32), Issue> {
    let t = split_top(v, None);
    let frac = |w: &str, horizontal: bool| -> Option<f32> {
        match w {
            "left" if horizontal => Some(0.0),
            "right" if horizontal => Some(1.0),
            "top" if !horizontal => Some(0.0),
            "bottom" if !horizontal => Some(1.0),
            "center" => Some(0.5),
            _ => w
                .strip_suffix('%')?
                .trim()
                .parse::<f32>()
                .ok()
                .map(|p| p / 100.0),
        }
    };
    let (x, y) = match t.as_slice() {
        [a] => match *a {
            "top" | "bottom" => (Some(0.5), frac(a, false)),
            _ => (frac(a, true), Some(0.5)),
        },
        [a, b] => {
            // `top left` is allowed in either order when both are keywords.
            if matches!(*a, "top" | "bottom") && matches!(*b, "left" | "right") {
                (frac(b, true), frac(a, false))
            } else {
                (frac(a, true), frac(b, false))
            }
        }
        _ => (None, None),
    };
    match (x, y) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => bad(format!(
            "`{v}`: only keywords and percentages are supported here"
        )),
    }
}

fn transform(v: &str) -> Result<Op, Issue> {
    let (mut tx, mut ty, mut sx, mut sy) = (Len::Px(0.0), Len::Px(0.0), 1.0f32, 1.0f32);
    if v == "none" {
        return Ok(Op::Transform { tx, ty, sx, sy });
    }
    let add = |a: Len, b: Len| -> Result<Len, Issue> {
        match (&a, &b) {
            (Len::Px(x), Len::Px(y)) => Ok(Len::Px(x + y)),
            (Len::Pct(x), Len::Pct(y)) => Ok(Len::Pct(x + y)),
            (Len::Px(z), o) | (o, Len::Px(z)) if *z == 0.0 => Ok(o.clone()),
            _ => bad("transform: mixing % and px translations is not supported"),
        }
    };
    let mut rest = v;
    while !rest.trim().is_empty() {
        let r = rest.trim_start();
        let Some(open) = r.find('(') else {
            return bad(format!("`{v}` is not a transform"));
        };
        let Some(close) = r.find(')') else {
            return bad("unclosed `(` in transform");
        };
        let name = r[..open].trim().to_ascii_lowercase();
        let args: Vec<&str> = r[open + 1..close].split(',').map(str::trim).collect();
        let n = |s: &str| -> Result<f32, Issue> { p::<f32>(s) };
        match (name.as_str(), args.as_slice()) {
            ("translatex", [x]) => tx = add(tx, len(x)?)?,
            ("translatey", [y]) => ty = add(ty, len(y)?)?,
            ("translate" | "translate3d", [x, more @ ..]) => {
                tx = add(tx, len(x)?)?;
                if let Some(y) = more.first() {
                    ty = add(ty, len(y)?)?;
                }
            }
            ("scale", [k]) => {
                let k = n(k)?;
                (sx, sy) = (sx * k, sy * k);
            }
            ("scale", [a, b]) => (sx, sy) = (sx * n(a)?, sy * n(b)?),
            ("scalex", [k]) => sx *= n(k)?,
            ("scaley", [k]) => sy *= n(k)?,
            ("scale3d", [a, b, _]) => (sx, sy) = (sx * n(a)?, sy * n(b)?),
            (
                "rotate" | "rotatex" | "rotatey" | "rotatez" | "skew" | "skewx" | "skewy"
                | "matrix" | "perspective",
                _,
            ) => {
                return Err(Issue {
                    kind: Kind::Planned,
                    msg: format!(
                        "`{name}()` is not supported (Iced draws translation and uniform scale)"
                    ),
                });
            }
            _ => return bad(format!("`{name}()` is not a supported transform")),
        }
        rest = &r[close + 1..];
    }
    Ok(Op::Transform {
        tx,
        ty,
        sx: sx.clamp(0.0, 20.0),
        sy: sy.clamp(0.0, 20.0),
    })
}

fn ops_of(name: &str, v: &str) -> Result<Vec<Op>, Issue> {
    Ok(match compile(name, v)? {
        Outcome::Ops(o) | Outcome::Approx(o, _) => o,
        Outcome::NoEffect => vec![],
    })
}

/// `font: [style] [weight] size[/line-height] family`.
fn font_shorthand(v: &str) -> Result<Vec<Op>, Issue> {
    let toks = split_top(v, None);
    let (mut ops, mut i) = (vec![], 0);
    while i < toks.len() {
        let t = toks[i];
        // The size is the first token that carries a unit (or a `/`); a bare number is a weight.
        if t.contains('/')
            || t.ends_with(|c: char| c.is_ascii_alphabetic() || c == '%')
                && t.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        {
            break;
        }
        match t {
            "italic" | "oblique" => ops.push(Op::Italic(true)),
            "normal" | "small-caps" => {}
            w if weight(w).is_ok() => ops.push(Op::Weight(weight(w)?)),
            _ => break,
        }
        i += 1;
    }
    let Some(size) = toks.get(i) else {
        return bad("`font` needs a size and a family");
    };
    let (sz, mut lh) = size
        .split_once('/')
        .map_or((*size, None), |(a, b)| (a, Some(b.to_owned())));
    let mut next = i + 1;
    if lh.as_deref() == Some("") {
        lh = toks.get(next).map(|s| (*s).to_owned());
        next += 1;
    } else if lh.is_none() && toks.get(next) == Some(&"/") {
        lh = toks.get(next + 1).map(|s| (*s).to_owned());
        next += 2;
    }
    ops.extend(ops_of("font-size", sz)?);
    if let Some(l) = lh {
        ops.extend(ops_of("line-height", &l)?);
    }
    let fam = toks[next.min(toks.len())..].join(" ");
    if fam.is_empty() {
        return bad("`font` needs a family");
    }
    ops.extend(ops_of("font-family", &fam)?);
    Ok(ops)
}

fn set_sides(dst: &mut Rect<LengthPercentageAuto>, s: &Sides, font: f32, cx: Ctx) {
    let f = |l: &Len| l.lpa(font, cx);
    if let Some(l) = &s[0] {
        dst.top = f(l);
    }
    if let Some(l) = &s[1] {
        dst.right = f(l);
    }
    if let Some(l) = &s[2] {
        dst.bottom = f(l);
    }
    if let Some(l) = &s[3] {
        dst.left = f(l);
    }
}

impl Op {
    /// Does applying this depend on the window size?
    pub fn uses_viewport(&self) -> bool {
        let any = |s: &Sides| s.iter().flatten().any(|l| l.is_viewport());
        match self {
            Op::Inset(s) | Op::Margin(s) | Op::Padding(s) | Op::BorderWidth(s) => any(s),
            Op::Bg(f) | Op::BgImage(f) => f.iter().any(paint::Fill::uses_viewport),
            Op::Size(_, l) | Op::Basis(l) | Op::FontSize(l, _) => l.is_viewport(),
            Op::Gap(a, b) => {
                a.as_ref().is_some_and(Len::is_viewport) || b.as_ref().is_some_and(Len::is_viewport)
            }
            Op::LineHeight(LineHeightValue::Len(l)) => l.is_viewport(),
            Op::LetterSpacing(l) => l.is_viewport(),
            Op::Transform { tx, ty, .. } => tx.is_viewport() || ty.is_viewport(),
            Op::TracksRaw(..) => true,
            _ => false,
        }
    }

    pub fn apply(&self, c: &mut Computed, cx: Ctx, base: &Computed) {
        let font = c.font_size;
        let st = &mut c.layout;
        match self {
            Op::Display(d) => st.display = *d,
            Op::Float(f) => st.float = *f,
            Op::Clear(k) => st.clear = *k,
            Op::Inherit(k) => match *k {
                "color" => c.color = base.color,
                "font-size" => c.font_size = base.font_size,
                "font-weight" => c.weight = base.weight,
                "font-style" => c.italic = base.italic,
                "font-family" => c.family = base.family.clone(),
                "line-height" => c.line_height = base.line_height,
                "letter-spacing" => c.letter_spacing = base.letter_spacing,
                "text-align" => c.align = base.align,
                "text-transform" => c.text_transform = base.text_transform,
                _ => {}
            },
            Op::Position(pos, fixed, positioned) => {
                st.position = *pos;
                c.fixed = *fixed;
                c.positioned = *positioned;
            }
            Op::Inset(s) => set_sides(&mut st.inset, s, font, cx),
            Op::Margin(s) => set_sides(&mut st.margin, s, font, cx),
            Op::Padding(s) => {
                for (i, l) in s.iter().enumerate() {
                    if let Some(l) = l {
                        let v = l.lp(font, cx);
                        match i {
                            0 => st.padding.top = v,
                            1 => st.padding.right = v,
                            2 => st.padding.bottom = v,
                            _ => st.padding.left = v,
                        }
                    }
                }
            }
            Op::BorderWidth(s) => {
                for (i, l) in s.iter().enumerate() {
                    if let Some(l) = l {
                        let v = l.lp(font, cx);
                        match i {
                            0 => st.border.top = v,
                            1 => st.border.right = v,
                            2 => st.border.bottom = v,
                            _ => st.border.left = v,
                        }
                    }
                }
            }
            Op::Size(which, l) => match which {
                0 => st.size.width = l.dim(font, cx),
                1 => st.size.height = l.dim(font, cx),
                2 => st.min_size.width = l.lpa(font, cx),
                3 => st.min_size.height = l.lpa(font, cx),
                4 => st.max_size.width = l.lpa(font, cx),
                _ => st.max_size.height = l.lpa(font, cx),
            },
            Op::Aspect(r) => st.aspect_ratio = *r,
            Op::BoxSizing(b) => st.box_sizing = *b,
            Op::BorderColor(mask, col) => {
                let col = col.unwrap_or(c.color);
                for (i, m) in mask.iter().enumerate() {
                    if *m {
                        c.border_color[i] = col;
                    }
                }
            }
            Op::Radius(r) => {
                for (i, v) in r.iter().enumerate() {
                    if let Some(v) = v {
                        c.radius[i] = *v;
                    }
                }
            }
            Op::FlexDir(d) => st.flex_direction = *d,
            Op::FlexWrap(w) => st.flex_wrap = *w,
            Op::Grow(g) => st.flex_grow = *g,
            Op::Shrink(s) => st.flex_shrink = *s,
            Op::Basis(b) => st.flex_basis = b.dim(font, cx),
            Op::Justify(a) => st.justify_content = *a,
            Op::AlignContent(a) => st.align_content = *a,
            Op::AlignItems(a) => st.align_items = *a,
            Op::AlignSelf(a) => st.align_self = Some(*a),
            Op::JustifyItems(a) => st.justify_items = *a,
            Op::JustifySelf(a) => st.justify_self = Some(*a),
            Op::Gap(r, col) => {
                if let Some(r) = r {
                    st.gap.height = r.lp(font, cx);
                }
                if let Some(col) = col {
                    st.gap.width = col.lp(font, cx);
                }
            }
            Op::Tracks(cols, t) => {
                if *cols {
                    st.grid_template_columns = t.clone();
                } else {
                    st.grid_template_rows = t.clone();
                }
            }
            Op::TracksRaw(cols, raw) => {
                if let Ok(t) = tracks(&units(raw, cx, font)) {
                    if *cols {
                        st.grid_template_columns = t;
                    } else {
                        st.grid_template_rows = t;
                    }
                }
            }
            Op::AutoTracks(cols, t) => {
                if *cols {
                    st.grid_auto_columns = t.to_vec();
                } else {
                    st.grid_auto_rows = t.to_vec();
                }
            }
            Op::AutoFlow(f) => st.grid_auto_flow = *f,
            Op::GridLines(l) => {
                if let Some(x) = &l[0] {
                    st.grid_row.start = x.clone();
                }
                if let Some(x) = &l[1] {
                    st.grid_column.start = x.clone();
                }
                if let Some(x) = &l[2] {
                    st.grid_row.end = x.clone();
                }
                if let Some(x) = &l[3] {
                    st.grid_column.end = x.clone();
                }
            }
            Op::Areas(a) => st.grid_template_areas = a.clone(),
            Op::Overflow(x, y) => {
                if let Some(x) = x {
                    st.overflow.x = *x;
                }
                if let Some(y) = y {
                    st.overflow.y = *y;
                }
            }
            Op::Order(o) => c.order = *o,
            Op::Z(z) => c.z = *z,
            Op::Bg(b) => {
                // The shorthand resets everything: a trailing solid layer is the colour.
                let b: std::rc::Rc<[Fill]> = if b.iter().any(Fill::uses_viewport) {
                    paint::at_viewport(b, cx.vw, cx.vh).into()
                } else {
                    b.clone()
                };
                let mut v: Vec<Fill> = b.to_vec();
                c.bg_col = match v.last() {
                    Some(Fill::Solid(_)) => v.pop(),
                    _ => None,
                };
                c.bg_img = v.into();
                c.bg = b.clone();
            }
            Op::PicUrl(u) => c.pic_url = u.clone(),
            Op::PicSize(x) => c.pic_size = *x,
            Op::PicPos(x) => c.pic_pos = *x,
            Op::PicRepeat(x) => c.pic_repeat = *x,
            Op::PicAbove(x) => c.pic_above = *x,
            Op::BgColor(col) => {
                c.bg_col = col.clone();
                c.bg = c.bg_img.iter().cloned().chain(c.bg_col.clone()).collect();
            }
            Op::BgImage(img) => {
                c.bg_img = if img.iter().any(Fill::uses_viewport) {
                    paint::at_viewport(img, cx.vw, cx.vh).into()
                } else {
                    img.clone()
                };
                c.bg = c.bg_img.iter().cloned().chain(c.bg_col.clone()).collect();
            }
            Op::Color(col) => c.color = *col,
            Op::Opacity(o) => c.opacity = *o,
            Op::Shadows(s) => c.shadows = s.clone(),
            Op::TextShadow(s) => c.text_shadow = *s,
            Op::DropShadow(s) => c.drop_shadow = *s,
            Op::Mask(m) => c.mask = m.clone(),
            Op::ObjectFit(f) => c.fit = *f,
            Op::ObjectPos(x, y) => c.object_pos = (*x, *y),
            Op::LetterSpacing(l) => {
                c.letter_spacing = l.px(font, cx).unwrap_or(0.0).clamp(-20.0, 50.0)
            }
            Op::Ellipsis(e) => c.ellipsis = *e,
            Op::Underline(u) => c.underline = *u,
            Op::Transform { tx, ty, sx, sy } => {
                let part = |l: &Len| -> (f32, f32) {
                    match l.pct(font, cx) {
                        Some(p) => (0.0, p),
                        None => (l.px(font, cx).unwrap_or(0.0), 0.0),
                    }
                };
                c.xf = Xf {
                    tx: part(tx),
                    ty: part(ty),
                    sx: *sx,
                    sy: *sy,
                    origin: c.xf.origin,
                };
            }
            Op::Origin(x, y) => c.xf.origin = (*x, *y),
            Op::Anim(p) => {
                let mut a = c.anim.as_deref().cloned().unwrap_or_default();
                p.apply(&mut a);
                c.anim = (!a.name.is_empty()).then(|| Rc::new(a));
            }
            Op::Brightness(b) => c.brightness = *b,
            Op::TransAll(t) => c.trans = t.clone(),
            Op::TransProps(p) => {
                let mut t = (*c.trans).clone();
                t.props = p.clone();
                c.trans = Rc::new(t);
            }
            Op::TransDur(d) => {
                let mut t = (*c.trans).clone();
                t.durs = d.clone();
                c.trans = Rc::new(t);
            }
            Op::TransDelay(d) => {
                let mut t = (*c.trans).clone();
                t.delays = d.clone();
                c.trans = Rc::new(t);
            }
            Op::TransEase(e) => {
                let mut t = (*c.trans).clone();
                t.eases = e.clone();
                c.trans = Rc::new(t);
            }
            Op::FontSize(l, _) => {
                let px = match l {
                    // `em` (and `%`) of a font size is the parent's: an earlier `font-size` on the same element (`.material-icons { 24px }` then `.starIcon { 1.4em }`) must not compound.
                    Len::Em(e) => e * base.font_size,
                    other => other.px(font, cx).unwrap_or(font),
                };
                c.font_size = clean(px).clamp(6.0, 200.0);
            }
            Op::Weight(w) => c.weight = w.resolve(c.weight),
            Op::Italic(i) => c.italic = *i,
            Op::Family(f) => c.family = f.clone(),
            Op::LineHeight(l) => {
                c.line_height = match l {
                    LineHeightValue::Normal => LineH::Mult(1.3),
                    LineHeightValue::Mult(m) => LineH::Mult(m.clamp(0.8, 3.0)),
                    LineHeightValue::Len(l) => match l {
                        Len::Em(e) => LineH::Mult(e.clamp(0.8, 3.0)),
                        other => {
                            LineH::Px(other.px(font, cx).unwrap_or(font * 1.3).clamp(8.0, 400.0))
                        }
                    },
                }
            }
            Op::TextTransform(t) => c.text_transform = *t,
            Op::TextAlign(a) => c.align = *a,
            Op::Lines(l) => c.lines = *l,
            Op::Clamp(l) => {
                c.lines = *l;
                c.ellipsis = l.is_some();
            }
            Op::Visible(v) => c.visible = *v,
            Op::Pointer(v) => c.pointer_events = *v,
            Op::Outline(o) => c.outline_color = *o,
            Op::Content(t) => c.content = t.clone(),
        }
    }
}

/// Zero-size helper kept for symmetry with Taffy's `Size` in layout code.
pub fn size_zero() -> Size<f32> {
    Size {
        width: 0.0,
        height: 0.0,
    }
}
