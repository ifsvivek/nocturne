//! Design tokens: near-black base, hairline structure, one electric blue accent, monochrome
//! chrome (library art is the only rich colour). Dark only. Raw hex lives here and nowhere else.
use iced::Font;
use iced::font::{Family, Weight};
use iced::gradient::{self, Gradient};
use iced::{Background, Color, Degrees};
use nocturne_core::customize::{self as cz, Fill};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Faces embedded in the binary (loaded in `main`): the UI must not depend on what the OS has
/// installed. Without them Windows has neither family, and text falls back to mismatched glyphs.
pub const BUNDLED_FONTS: [&[u8]; 5] = [
    include_bytes!("../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../assets/fonts/NotoSans-Medium.ttf"),
    include_bytes!("../assets/fonts/NotoSans-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
];
pub const MONO: Font = Font::with_name("JetBrains Mono");
const FAMILY_DEFAULT: &str = "Noto Sans";

/// The family of the explicit `t::base()`-style fonts. Iced's default font (text without an explicit
/// font) is fixed at launch, so a theme's family reaches the headings, labels and buttons that name
/// their font, not every run of text.
static FAMILY: RwLock<&'static str> = RwLock::new(FAMILY_DEFAULT);

/// `font-family` (lower case) -> the name inside the font file, for fonts a theme brought along.
static FONT_MAP: RwLock<Vec<(String, &'static str)>> = RwLock::new(Vec::new());
/// Family names handed to Iced (`Font::with_name` wants `&'static str`): bounded, each leaked once.
static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
const MAX_NAMES: usize = 48;

fn intern(n: &str) -> &'static str {
    let mut v = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = v.iter().find(|s| **s == n) {
        return s;
    }
    if v.len() >= MAX_NAMES {
        return FAMILY_DEFAULT;
    }
    let s: &'static str = Box::leak(n.to_owned().into_boxed_str());
    v.push(s);
    s
}

/// Remember which loaded fonts answer to which CSS family (replaces the previous map).
pub fn map_fonts(fonts: &[cz::import::LoadedFont]) {
    let map = fonts
        .iter()
        .map(|f| {
            let internal = f.internal_family.as_deref().unwrap_or(&f.family);
            (f.family.to_lowercase(), intern(internal))
        })
        .collect();
    *FONT_MAP.write().unwrap_or_else(|e| e.into_inner()) = map;
}

/// Is `family` one of the fonts the theme brought along (so it is not "only if installed")?
pub fn font_is_loaded(family: &str) -> bool {
    let l = family.to_lowercase();
    FONT_MAP
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|(f, _)| *f == l)
}

fn family() -> &'static str {
    *FAMILY.read().unwrap_or_else(|e| e.into_inner())
}

/// The body font at `weight` (Noto Sans unless the active theme names an installed family).
pub fn font(weight: Weight) -> Font {
    Font {
        family: Family::Name(family()),
        weight,
        ..Font::DEFAULT
    }
}
pub fn base() -> Font {
    font(Weight::Normal)
}
pub fn medium() -> Font {
    font(Weight::Medium)
}
pub fn semibold() -> Font {
    font(Weight::Semibold)
}
pub fn bold() -> Font {
    font(Weight::Bold)
}

const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}
const fn rgba(hex: u32) -> Color {
    Color {
        r: ((hex >> 24) & 0xff) as f32 / 255.0,
        g: ((hex >> 16) & 0xff) as f32 / 255.0,
        b: ((hex >> 8) & 0xff) as f32 / 255.0,
        a: (hex & 0xff) as f32 / 255.0,
    }
}

/// Fully transparent (the GPUI `rgba(0)`).
pub const NONE: Color = Color::TRANSPARENT;
/// Header / breadcrumb fill: the frame colour at 90%.
pub const HEADER_BG: Color = rgba(0x07080ce6);
/// Tile / cell chrome that is neither text nor a control, kept here so no raw hex leaks out.
pub const PROGRESS_TRACK: Color = rgba(0xffffff1f);
pub const POSTER_DIM: Color = rgba(0x07080c40);
pub const INDEX_TAG: Color = rgba(0xffffffb0);
pub const INITIAL_TINT: Color = rgba(0x8fb0ff38);
/// Backdrop texture tints (RGB; alpha is procedural).
pub const DOT: [u8; 3] = [0xc8, 0xd4, 0xff];
pub const BACKDROP_GLOW: [u8; 3] = [0x3b, 0x6b, 0xff];
/// Modal scrim.
pub const SCRIM: Color = rgba(0x000000b8);

// Surfaces.
pub const FRAME: Color = rgb(0x07080c);
pub const SURFACE: Color = rgb(0x0c0e16);
pub const RAISED: Color = rgb(0x12151f);

// Structure: hairlines ~8% white, faint glass fill.
pub const HAIRLINE: Color = rgba(0xffffff14);
pub const HAIRLINE_STRONG: Color = rgba(0xffffff29);
pub const GLASS: Color = rgba(0xffffff0a);
/// Boundary of an interactive control (chip, button, switch track, avatar): opaque, >= 3:1 on every
/// surface (WCAG 1.4.11). Hairlines stay decorative dividers and panel outlines.
pub const EDGE: Color = rgb(0x747a91);
/// Quiet filled background of a value control; the edge carries the boundary.
pub const CONTROL: Color = rgb(0x1a1e2c);
pub const CONTROL_HOVER: Color = rgb(0x232838);

// Text.
pub const TEXT: Color = rgb(0xeceef5);
pub const TEXT_SECONDARY: Color = rgb(0xa4aabf);
pub const TEXT_TERTIARY: Color = rgb(0x8990a9);

// Single accent: electric blue.
pub const ACCENT: Color = rgb(0x4c8dff);
pub const ACCENT_HOVER: Color = rgb(0x6ba0ff);
pub const ACCENT_TEXT: Color = rgb(0x9cc0ff);
pub const ACCENT_TINT: Color = rgba(0x4c8dff1f);
pub const GLOW: Color = rgba(0x4c8dff59);
pub const ON_ACCENT: Color = rgb(0x050816);

// Destructive confirmations and warnings only; never decoration.
pub const DANGER: Color = rgb(0xf0716f);
pub const DANGER_TINT: Color = rgba(0xf0716f1f);
pub const WARN: Color = rgb(0xe3b341);

// Type scale (px). Only mono micro-annotations may use MICRO; everything else >= 12.
pub const MICRO: f32 = 11.0;
pub const CAPTION: f32 = 12.0;
pub const BODY: f32 = 15.0;
pub const BODY_LG: f32 = 16.0;
pub const HEADING: f32 = 22.0;
pub const DISPLAY: f32 = 56.0;

// Spacing (4px scale) and radii: panels ~10, cells square-ish.
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S6: f32 = 24.0;
pub const S8: f32 = 32.0;
pub const R_XS: f32 = 3.0;
pub const R_PANEL: f32 = 10.0;
pub const PAGE_PAD: f32 = 48.0;
pub const HEADER_H: f32 = 56.0;

/// Couch mode grows the whole UI by this factor (a 10-foot layout from the same tokens).
pub const COUCH_SCALE: f32 = 1.35;

// Motion: short, ease-out, never gratuitous.
pub const HOVER: Duration = Duration::from_millis(140);
pub const BASE: Duration = Duration::from_millis(220);
pub const SLOW: Duration = Duration::from_millis(300);

/// Type size of control values (Settings chips) and the minimum control height.
pub const CONTROL_TEXT: f32 = 13.0;
pub const CONTROL_H: f32 = 32.0;

/// The tokens a theme can change, resolved once per theme change (never while drawing). The `pub
/// const`s above are the built-in values and the source of truth for the default look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub bg: Background,
    pub frame: Color,
    pub surface: Color,
    pub raised: Color,
    pub header_bg: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_text: Color,
    pub accent_tint: Color,
    pub glow: Color,
    pub on_accent: Color,
    pub edge: Color,
    pub hairline: Color,
    pub hairline_strong: Color,
    pub control: Color,
    pub control_hover: Color,
    pub danger: Color,
    pub danger_tint: Color,
    pub warn: Color,
    pub scrim: Color,
    pub r_panel: f32,
    pub r_xs: f32,
    pub r_card: f32,
    /// Backdrop artwork opacity and blur radius (px).
    pub backdrop_opacity: f32,
    pub backdrop_blur: f32,
}

impl Tokens {
    pub const DEFAULT: Tokens = Tokens {
        bg: Background::Color(FRAME),
        frame: FRAME,
        surface: SURFACE,
        raised: RAISED,
        header_bg: HEADER_BG,
        text: TEXT,
        text_secondary: TEXT_SECONDARY,
        text_tertiary: TEXT_TERTIARY,
        accent: ACCENT,
        accent_hover: ACCENT_HOVER,
        accent_text: ACCENT_TEXT,
        accent_tint: ACCENT_TINT,
        glow: GLOW,
        on_accent: ON_ACCENT,
        edge: EDGE,
        hairline: HAIRLINE,
        hairline_strong: HAIRLINE_STRONG,
        control: CONTROL,
        control_hover: CONTROL_HOVER,
        danger: DANGER,
        danger_tint: DANGER_TINT,
        warn: WARN,
        scrim: SCRIM,
        r_panel: R_PANEL,
        r_xs: R_XS,
        r_card: R_XS,
        backdrop_opacity: 1.0,
        backdrop_blur: 0.0,
    };

    /// Overlay a theme on the built-in look. A token the theme leaves out keeps its built-in value;
    /// a token derived from another (hover from accent, tints, secondary text) follows it only when
    /// the theme changed the parent and did not set the derived one itself.
    pub fn resolve(th: &cz::Theme) -> Tokens {
        let c = |c: cz::Color| {
            let [r, g, b, a] = c.to_f32();
            Color { r, g, b, a }
        };
        let mut k = Self::DEFAULT;
        match &th.background {
            Some(Fill::Solid(v)) => {
                k.frame = c(*v);
                k.bg = Background::Color(k.frame);
            }
            Some(Fill::Linear { angle, stops }) if !stops.is_empty() => {
                let mut g = gradient::Linear::new(Degrees(*angle));
                for (col, at) in stops {
                    g = g.add_stop(*at, c(*col));
                }
                k.frame = c(stops[0].0);
                k.bg = Background::Gradient(Gradient::Linear(g));
            }
            _ => {}
        }
        if th.background.is_some() {
            k.header_bg = Color {
                a: HEADER_BG.a,
                ..k.frame
            };
        }
        let set = |dst: &mut Color, v: Option<cz::Color>| {
            if let Some(v) = v {
                *dst = c(v);
            }
        };
        set(&mut k.surface, th.surface);
        set(&mut k.raised, th.raised);
        set(&mut k.header_bg, th.header);
        if let Some(v) = th.text {
            k.text = c(v);
            k.text_secondary = mix(k.text, k.frame, 0.3);
            k.text_tertiary = mix(k.text, k.frame, 0.45);
        }
        set(&mut k.text_secondary, th.text_secondary);
        set(&mut k.text_tertiary, th.text_tertiary);
        if let Some(v) = th.accent {
            k.accent = c(v);
            k.accent_hover = mix(k.accent, Color::WHITE, 0.17);
            k.accent_text = mix(k.accent, Color::WHITE, 0.45);
            k.accent_tint = Color {
                a: ACCENT_TINT.a,
                ..k.accent
            };
            k.glow = Color {
                a: GLOW.a,
                ..k.accent
            };
            // Dark or white label, whichever reads better on the fill.
            k.on_accent = if luma(k.accent) > 0.18 {
                ON_ACCENT
            } else {
                Color::WHITE
            };
        }
        set(&mut k.accent_hover, th.accent_hover);
        set(&mut k.accent_text, th.accent_text);
        set(&mut k.on_accent, th.on_accent);
        set(&mut k.edge, th.edge);
        if let Some(v) = th.hairline {
            k.hairline = c(v);
            k.hairline_strong = Color {
                a: HAIRLINE_STRONG.a,
                ..k.hairline
            };
        }
        if let Some(v) = th.control {
            k.control = c(v);
            k.control_hover = mix(k.control, Color::WHITE, 0.05);
        }
        set(&mut k.control_hover, th.control_hover);
        if let Some(v) = th.danger {
            k.danger = c(v);
            k.danger_tint = Color {
                a: DANGER_TINT.a,
                ..k.danger
            };
        }
        set(&mut k.warn, th.warn);
        set(&mut k.scrim, th.scrim);
        k.r_panel = th.radius_panel.unwrap_or(k.r_panel);
        k.r_xs = th.radius_small.unwrap_or(k.r_xs);
        // Cards keep the small radius unless the theme sets one.
        k.r_card = th.radius_card.or(th.radius_small).unwrap_or(k.r_card);
        k.backdrop_opacity = th.backdrop_opacity.unwrap_or(1.0);
        k.backdrop_blur = th.backdrop_blur.unwrap_or(0.0);
        k
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a,
    }
}

fn luma(c: Color) -> f32 {
    let l = |v: f32| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * l(c.r) + 0.7152 * l(c.g) + 0.0722 * l(c.b)
}

/// WCAG contrast ratio of two opaque colours.
fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (luma(a), luma(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Advisory contrast notes for a theme (text 4.5:1, outlines 3:1); never blocking.
pub fn advisories(k: &Tokens) -> Vec<String> {
    let checks = [
        ("Text on the background", k.text, k.frame, 4.5),
        ("Text on panels", k.text, k.surface, 4.5),
        (
            "Secondary text on the background",
            k.text_secondary,
            k.frame,
            4.5,
        ),
        ("Accent on the background", k.accent, k.frame, 3.0),
        ("Labels on the accent", k.on_accent, k.accent, 4.5),
        ("Control outlines on the background", k.edge, k.frame, 3.0),
    ];
    checks
        .into_iter()
        .filter_map(|(what, fg, bg, min)| {
            let r = contrast(fg, bg);
            (r < min)
                .then(|| format!("{what} has contrast {r:.1}:1 (at least {min}:1 is recommended)"))
        })
        .collect()
}

static ACTIVE: RwLock<Tokens> = RwLock::new(Tokens::DEFAULT);

/// What the active theme says about layout, resolved once per change. `None` is the built-in look.
struct Look {
    layout: cz::Layout,
    density: f32,
    font_scale: f32,
}

static LOOK: RwLock<Option<Arc<Look>>> = RwLock::new(None);

/// Make `th` the look of the whole app (call when the theme changes, not per frame).
pub fn set(th: &cz::Theme, layout: &cz::Layout) {
    let name = match th.font_family.as_deref() {
        Some(n) => {
            let mapped = FONT_MAP
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|(f, _)| *f == n.to_lowercase())
                .map(|(_, i)| *i);
            mapped.unwrap_or_else(|| intern(n))
        }
        None => FAMILY_DEFAULT,
    };
    *FAMILY.write().unwrap_or_else(|e| e.into_inner()) = name;
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = Tokens::resolve(th);
    let look = (layout.count() > 0 || th.density.is_some() || th.font_size.is_some()).then(|| {
        Arc::new(Look {
            layout: layout.clone(),
            density: th.density.unwrap_or(1.0),
            font_scale: th.font_size.map_or(1.0, |px| px / BODY),
        })
    });
    *LOOK.write().unwrap_or_else(|e| e.into_inner()) = look;
}

fn look() -> Option<Arc<Look>> {
    LOOK.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Layout settings for a view: base, then the page's, then the section's (all unset by default).
pub fn parts(page: Option<cz::Page>, section: Option<cz::Section>) -> cz::Parts {
    look().map_or_else(Default::default, |l| l.layout.parts(page, section))
}

/// Home sections in display order.
pub fn section_order() -> [cz::Section; 3] {
    look().map_or(cz::Section::ALL, |l| l.layout.section_order())
}

/// `--nocturne-density`: scales the page spacing (padding, header height, section bands).
pub fn density() -> f32 {
    look().map_or(1.0, |l| l.density)
}

/// Hover transition of buttons: `.button` `transition-duration` over the built-in 140 ms, and the
/// hover scale (`.button:hover { transform: scale() }`, none by default).
pub fn button() -> (Duration, Option<f32>) {
    look().map_or((HOVER, None), |l| {
        let b = &l.layout.base.button;
        (
            b.transition_ms
                .map_or(HOVER, |ms| Duration::from_millis(ms as u64)),
            b.hover_scale,
        )
    })
}

/// `font-size` on `:root` relative to the built-in 15 px body. Iced has no per-text scale, so the
/// whole UI is zoomed by this factor (like couch mode) rather than only the type.
pub fn font_scale() -> f32 {
    look().map_or(1.0, |l| l.font_scale)
}

pub fn page_pad() -> f32 {
    PAGE_PAD * density()
}

/// Header height (explicit `height` wins over the density-scaled default).
pub fn header_h() -> f32 {
    look()
        .and_then(|l| l.layout.base.header.height)
        .unwrap_or(HEADER_H * density())
}

/// The header takes its own row and pushes the content down (`position: static`); otherwise it
/// overlays the content, so a billboard can run under it.
pub fn header_pushes() -> bool {
    look().is_some_and(|l| l.layout.base.header.position == Some(cz::HeaderPosition::Static))
}

/// One of the four header items, shown unless the stylesheet hid it.
pub fn header_item(item: HeaderItem) -> bool {
    look().is_none_or(|l| {
        let b = &l.layout.base;
        let v = match item {
            HeaderItem::Nav => &b.header_nav,
            HeaderItem::Search => &b.header_search,
            HeaderItem::History => &b.header_history,
            HeaderItem::Account => &b.header_account,
        };
        v.hidden != Some(true)
    })
}

#[derive(Clone, Copy)]
pub enum HeaderItem {
    Nav,
    Search,
    History,
    Account,
}

/// Fill of a layout property (gradient or solid) as an Iced background.
pub fn background(f: &Fill) -> Background {
    let c = |c: cz::Color| {
        let [r, g, b, a] = c.to_f32();
        Color { r, g, b, a }
    };
    match f {
        Fill::Solid(v) => Background::Color(c(*v)),
        Fill::Linear { angle, stops } => {
            let mut g = gradient::Linear::new(Degrees(*angle));
            for (col, at) in stops {
                g = g.add_stop(*at, c(*col));
            }
            Background::Gradient(Gradient::Linear(g))
        }
    }
}

/// Typography of a text part: what the stylesheet set over the built-in size and weight.
/// ponytail: `letter-spacing` is read but Iced 0.14 text has no tracking; it is ignored.
#[derive(Clone, Debug)]
pub struct Typo {
    pub hidden: bool,
    size: f32,
    font: Font,
    color: Option<Color>,
    line: Option<f32>,
    transform: cz::TextTransform,
}

pub fn typo(tx: &cz::Text, size: f32, weight: Weight) -> Typo {
    let w = match tx.font_weight {
        None => weight,
        Some(n) => match n {
            0..=149 => Weight::Thin,
            150..=249 => Weight::ExtraLight,
            250..=349 => Weight::Light,
            350..=449 => Weight::Normal,
            450..=549 => Weight::Medium,
            550..=649 => Weight::Semibold,
            650..=749 => Weight::Bold,
            750..=849 => Weight::ExtraBold,
            _ => Weight::Black,
        },
    };
    Typo {
        hidden: tx.hidden == Some(true),
        size: tx.font_size.unwrap_or(size),
        font: font(w),
        color: tx.color.map(|c| {
            let [r, g, b, a] = c.to_f32();
            Color { r, g, b, a }
        }),
        line: tx.line_height,
        transform: tx.text_transform.unwrap_or(cz::TextTransform::None),
    }
}

impl Typo {
    /// Line height to use unless the stylesheet set one.
    pub fn with_line(mut self, l: f32) -> Self {
        self.line.get_or_insert(l);
        self
    }

    pub fn size(&self) -> f32 {
        self.size
    }

    /// The typography as CSS declarations for the theme engine's tree builders (colour only when the stylesheet set one).
    pub fn css(&self) -> String {
        use iced::font::Weight as W;
        let w = match self.font.weight {
            W::Thin => 100,
            W::ExtraLight => 200,
            W::Light => 300,
            W::Normal => 400,
            W::Medium => 500,
            W::Semibold => 600,
            W::Bold => 700,
            W::ExtraBold => 800,
            W::Black => 900,
        };
        let mut s = format!("font-size: {}px; font-weight: {w}", self.size);
        if let Some(c) = self.color {
            s += &format!("; color: {}", css_color(c));
        }
        if let Some(l) = self.line {
            s += &format!("; line-height: {l}");
        }
        match self.transform {
            cz::TextTransform::None => {}
            cz::TextTransform::Uppercase => s += "; text-transform: uppercase",
            cz::TextTransform::Lowercase => s += "; text-transform: lowercase",
            cz::TextTransform::Capitalize => s += "; text-transform: capitalize",
        }
        s
    }

    /// Like [`Typo::text`] but the colour is only set when the stylesheet set one, so the text
    /// keeps the colour its container gives it (buttons that change ink on hover).
    pub fn text_inherit<'a>(&self, s: impl AsRef<str>) -> iced::widget::Text<'a> {
        let t = self.text(s, Color::WHITE);
        match self.color {
            Some(_) => t,
            None => t.color_maybe(None::<Color>),
        }
    }

    /// `s` as a text widget with size, weight, line height and colour (`default_color` unless the
    /// stylesheet set one).
    pub fn text<'a>(&self, s: impl AsRef<str>, default_color: Color) -> iced::widget::Text<'a> {
        let s = match self.transform {
            cz::TextTransform::None => s.as_ref().to_owned(),
            cz::TextTransform::Uppercase => s.as_ref().to_uppercase(),
            cz::TextTransform::Lowercase => s.as_ref().to_lowercase(),
            cz::TextTransform::Capitalize => capitalize(s.as_ref()),
        };
        let mut w = iced::widget::text(s)
            .size(self.size)
            .font(self.font)
            .color(self.color.unwrap_or(default_color));
        if let Some(l) = self.line {
            w = w.line_height(l);
        }
        w
    }
}

fn capitalize(s: &str) -> String {
    let mut up = true;
    s.chars()
        .map(|c| {
            let o = if up {
                c.to_uppercase().next().unwrap_or(c)
            } else {
                c
            };
            up = c.is_whitespace();
            o
        })
        .collect()
}

/// A colour as a CSS `rgba()` (for the theme engine's sheets).
pub fn css_color(c: Color) -> String {
    let b = |v: f32| (v * 255.0).round() as u8;
    format!("rgba({}, {}, {}, {:.3})", b(c.r), b(c.g), b(c.b), c.a)
}

pub fn active() -> Tokens {
    *ACTIVE.read().unwrap_or_else(|e| e.into_inner())
}

macro_rules! getters {
    ($($f:ident: $t:ty),* $(,)?) => {$(
        pub fn $f() -> $t {
            ACTIVE.read().unwrap_or_else(|e| e.into_inner()).$f
        }
    )*};
}
getters!(
    bg: Background, frame: Color, surface: Color, raised: Color, header_bg: Color, text: Color,
    text_secondary: Color, text_tertiary: Color, accent: Color, accent_hover: Color,
    accent_text: Color, accent_tint: Color, glow: Color, on_accent: Color, edge: Color,
    hairline: Color, hairline_strong: Color, control: Color, control_hover: Color, danger: Color,
    danger_tint: Color, warn: Color, scrim: Color, r_panel: f32, r_xs: f32, r_card: f32,
    backdrop_opacity: f32, backdrop_blur: f32,
);

#[cfg(test)]
mod tests {
    //! WCAG 2.x contrast audit of the tokens: text >= 4.5:1 on every surface it can sit on, control
    //! edges and focus rings >= 3:1. Semi-transparent fills are composited over the real surfaces.
    use super::*;

    #[test]
    fn a_loaded_font_answers_to_its_css_family() {
        let f = |family: &str, internal: &str| cz::import::LoadedFont {
            family: family.into(),
            weight: 400,
            weight_max: 400,
            style: cz::import::FontStyle::Normal,
            internal_family: Some(internal.into()),
            url: String::new(),
            bytes: Arc::from(vec![]),
        };
        map_fonts(&[f("Test Sans", "Test Sans Text")]);
        assert!(font_is_loaded("test sans"));
        let th = cz::Theme {
            font_family: Some("TEST sans".into()),
            ..Default::default()
        };
        set(&th, &Default::default());
        assert_eq!(family(), "Test Sans Text");
        // Unknown families are interned once, and the intern table is bounded.
        for i in 0..MAX_NAMES + 5 {
            intern(&format!("Family {i}"));
        }
        assert_eq!(intern("Never seen before"), FAMILY_DEFAULT);
        map_fonts(&[]);
        set(&Default::default(), &Default::default());
        assert_eq!(family(), FAMILY_DEFAULT);
    }

    type Rgb = [f32; 3];

    fn over(fg: Color, bg: Rgb) -> Rgb {
        [
            fg.r * fg.a + bg[0] * (1.0 - fg.a),
            fg.g * fg.a + bg[1] * (1.0 - fg.a),
            fg.b * fg.a + bg[2] * (1.0 - fg.a),
        ]
    }
    fn lum(c: Rgb) -> f32 {
        let l = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
    }
    fn ratio(a: Rgb, b: Rgb) -> f32 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
    fn solid(c: Color) -> Rgb {
        [c.r, c.g, c.b]
    }

    /// Every surface a label can sit on: each base, bare and under glass (hover), accent tint
    /// (selected) and danger tint (destructive).
    fn grounds() -> Vec<(String, Rgb)> {
        let mut v = vec![];
        for (n, b) in [("frame", FRAME), ("surface", SURFACE), ("raised", RAISED)] {
            let b = solid(b);
            v.push((n.to_string(), b));
            v.push((format!("{n}+glass"), over(GLASS, b)));
            v.push((format!("{n}+tint"), over(ACCENT_TINT, b)));
            v.push((format!("{n}+danger"), over(DANGER_TINT, b)));
        }
        v.push(("control".into(), solid(CONTROL)));
        v.push(("control-hover".into(), solid(CONTROL_HOVER)));
        v
    }

    #[test]
    fn text_tokens_reach_4_5_on_every_surface() {
        let mut bad = vec![];
        for (tn, c) in [
            ("text", TEXT),
            ("secondary", TEXT_SECONDARY),
            ("tertiary", TEXT_TERTIARY),
            ("accent", ACCENT),
            ("accent_text", ACCENT_TEXT),
            ("danger", DANGER),
            ("warn", WARN),
        ] {
            for (gn, g) in grounds() {
                let r = ratio(solid(c), g);
                if r < 4.5 {
                    bad.push(format!("{tn} on {gn}: {r:.2}"));
                }
            }
        }
        assert!(bad.is_empty(), "below 4.5:1: {bad:#?}");
    }

    #[test]
    fn labels_on_filled_controls_reach_4_5() {
        for fill in [ACCENT, ACCENT_HOVER] {
            let r = ratio(solid(ON_ACCENT), solid(fill));
            assert!(r >= 4.5, "on_accent on accent fill: {r:.2}");
        }
    }

    #[test]
    fn control_edges_and_focus_rings_reach_3() {
        for (tn, c) in [("edge", EDGE), ("accent ring", ACCENT), ("danger", DANGER)] {
            for (gn, g) in grounds() {
                let r = ratio(solid(c), g);
                assert!(r >= 3.0, "{tn} on {gn}: {r:.2}");
            }
        }
        // The control's own fill must also sit apart from the page so the edge reads as a boundary.
        assert!(ratio(solid(EDGE), solid(CONTROL_HOVER)) >= 3.0);
    }

    #[test]
    fn hierarchy_stays_quiet_but_distinct() {
        let (t, s, q) = (solid(TEXT), solid(TEXT_SECONDARY), solid(TEXT_TERTIARY));
        assert!(lum(t) > lum(s) && lum(s) > lum(q));
        assert!(
            ratio(s, q) >= 1.25,
            "secondary vs tertiary: {:.2}",
            ratio(s, q)
        );
        assert!(ratio(t, s) >= 1.5);
    }

    #[test]
    fn contrast_table() {
        // Run with --nocapture to print the numbers quoted in docs/DESIGN.md.
        for (tn, c) in [
            ("text", TEXT),
            ("secondary", TEXT_SECONDARY),
            ("tertiary", TEXT_TERTIARY),
            ("accent", ACCENT),
            ("edge", EDGE),
        ] {
            let all: Vec<f32> = grounds().iter().map(|(_, g)| ratio(solid(c), *g)).collect();
            let (lo, hi) = (
                all.iter().cloned().fold(f32::MAX, f32::min),
                all.iter().cloned().fold(0.0, f32::max),
            );
            println!("{tn}: {lo:.2} - {hi:.2}");
        }
    }

    #[test]
    fn default_theme_resolves_to_the_built_in_look() {
        assert_eq!(Tokens::resolve(&cz::Theme::default()), Tokens::DEFAULT);
    }

    #[test]
    fn accent_override_derives_its_dependents() {
        let th = cz::parse(":root{--nocturne-accent:#ff8800;}")
            .unwrap()
            .theme;
        let k = Tokens::resolve(&th);
        assert_ne!(k.accent, ACCENT);
        assert_eq!(k.accent_tint.a, ACCENT_TINT.a);
        assert_eq!(k.accent_tint.r, k.accent.r);
        assert!(ratio(solid(k.on_accent), solid(k.accent)) >= 4.5);
    }
}
