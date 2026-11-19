//! Native theming from CSS: a small, offline CSS reader and a translator from the supported subset
//! into [`Theme`] tokens (only the fields the stylesheet sets), plus a [`Warning`] for everything
//! that was not understood. No browser engine, no network, no code execution; see
//! `docs/dev/CUSTOMIZATION_PLAN.md` for the token model, selector vocabulary and mapping table.
mod css;
mod layout;
mod store;
mod value;

pub use layout::{
    Arrows, Aspect, Badge, BadgePos, Brand, BrandContent, Card, CardImage, Controls, EpisodeFormat,
    Header, HeaderAlign, HeaderPosition, Hero, HeroStyle, Layout, Page, Parts, Progress,
    ProgressPos, Row, RowLayout, Scope, Section, Shadow, Text, TextAlign, TextTransform, TileLabel,
    TitlePosition, Visibility,
};
pub use store::{Saved, Store, content_type_ok, fetch, import_name, validate_url};
pub use value::{Color, Fill};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Larger stylesheets are rejected before any parsing.
pub const MAX_BYTES: usize = 256 * 1024;
pub(crate) const MAX_RULES: usize = 5_000;
pub(crate) const MAX_DECLS: usize = 50_000;
const MAX_WARNINGS: usize = 500;
/// Longest value after `var()` substitution; also bounds exponential `var()` fan-out.
const MAX_VALUE: usize = 4_096;
const MAX_VAR_DEPTH: usize = 16;
/// `%` radii are taken of a control-sized box, so `50%` is a pill.
const RADIUS_REF_PX: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningKind {
    SyntaxError,
    UnsupportedAtRule,
    UnsupportedSelector,
    UnsupportedProperty,
    UnsupportedValue,
    /// `url()` or `@import`: never fetched.
    RemoteResource,
    UnresolvedVar,
    VarCycle,
    FontNotDownloaded,
    /// Understood only in part (first of several values, stops dropped, ...).
    Approximated,
    /// A value was outside the allowed range and was clamped.
    Clamped,
    /// A size limit stopped the reader or the warning list.
    LimitReached,
}

impl WarningKind {
    /// Plural phrase for the editor's summary ("434 selectors not supported").
    pub fn label(self) -> &'static str {
        match self {
            Self::SyntaxError => "syntax errors",
            Self::UnsupportedAtRule => "at-rules not supported",
            Self::UnsupportedSelector => "selectors not supported",
            Self::UnsupportedProperty => "properties not supported",
            Self::UnsupportedValue => "values not supported",
            Self::RemoteResource => "links not fetched",
            Self::UnresolvedVar => "unknown variables",
            Self::VarCycle => "variable cycles",
            Self::FontNotDownloaded => "fonts only if installed",
            Self::Approximated => "approximated",
            Self::Clamped => "clamped to the allowed range",
            Self::LimitReached => "limits reached",
        }
    }

    /// Notes that need no action; shown dimmed.
    pub fn informational(self) -> bool {
        matches!(
            self,
            Self::FontNotDownloaded | Self::Clamped | Self::Approximated
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    /// 1-based.
    pub line: u32,
    pub col: u32,
    pub kind: WarningKind,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    TooLarge { bytes: usize },
    NotUtf8,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooLarge { bytes } => {
                write!(f, "stylesheet is {bytes} bytes; the limit is {MAX_BYTES}")
            }
            Error::NotUtf8 => f.write_str("stylesheet is not valid UTF-8 text"),
        }
    }
}

impl std::error::Error for Error {}

/// Collects warnings with line/column lookup and a hard cap.
pub(crate) struct Warnings {
    line_starts: Vec<usize>,
    list: Vec<Warning>,
    dropped: usize,
}

impl Warnings {
    fn new(chars: &[char]) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            chars
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == '\n')
                .map(|(i, _)| i + 1),
        );
        Self {
            line_starts,
            list: vec![],
            dropped: 0,
        }
    }

    pub(crate) fn push(&mut self, pos: usize, kind: WarningKind, message: impl Into<String>) {
        if self.list.len() >= MAX_WARNINGS {
            self.dropped += 1;
            return;
        }
        let line = self.line_starts.partition_point(|&s| s <= pos).max(1);
        let col = pos - self.line_starts[line - 1] + 1;
        self.list.push(Warning {
            line: line as u32,
            col: col as u32,
            kind,
            message: message.into(),
        });
    }
}

/// The theme tokens a stylesheet can set. Every field is optional: `None` keeps the built-in value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    /// Window background: a colour or a linear gradient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raised: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_secondary: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_tertiary: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_hover: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_text: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_accent: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hairline: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_hover: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub danger: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warn: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrim: Option<Color>,
    /// Corner radii in px.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_panel: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_small: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_card: Option<f32>,
    /// Spacing multiplier (1.0 = built-in 4 px scale).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density: Option<f32>,
    /// Body text size in px; the whole type scale follows proportionally (built-in body is 15).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    /// First named family; used only when installed (fonts are never downloaded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Backdrop artwork opacity 0..=1 (1 = no extra dimming).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop_opacity: Option<f32>,
    /// Backdrop blur radius in px.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop_blur: Option<f32>,
}

impl Theme {
    /// How many tokens the theme sets.
    pub fn count(&self) -> usize {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_object().map(|o| o.len()))
            .unwrap_or(0)
    }

    /// Lay `top` over `self`: every field `top` sets wins.
    pub fn overlay(&mut self, top: &Theme) {
        macro_rules! ov {
            ($($f:ident),*) => {$( if top.$f.is_some() { self.$f = top.$f.clone(); } )*};
        }
        ov!(
            background,
            surface,
            raised,
            header,
            text,
            text_secondary,
            text_tertiary,
            accent,
            accent_hover,
            accent_text,
            on_accent,
            edge,
            hairline,
            control,
            control_hover,
            danger,
            warn,
            scrim,
            radius_panel,
            radius_small,
            radius_card,
            density,
            font_size,
            font_family,
            backdrop_opacity,
            backdrop_blur
        );
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub theme: Theme,
    /// Layout of the named parts; empty for a colour-only stylesheet.
    pub layout: Layout,
    /// Sorted by position.
    pub warnings: Vec<Warning>,
    /// Warnings beyond the cap of 500 that were counted but not kept.
    pub dropped_warnings: usize,
}

pub fn parse_bytes(bytes: &[u8]) -> Result<Parsed, Error> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::TooLarge { bytes: bytes.len() });
    }
    parse(std::str::from_utf8(bytes).map_err(|_| Error::NotUtf8)?)
}

pub fn parse(source: &str) -> Result<Parsed, Error> {
    if source.len() > MAX_BYTES {
        return Err(Error::TooLarge {
            bytes: source.len(),
        });
    }
    let mut chars: Vec<char> = source.chars().collect();
    let mut w = Warnings::new(&chars);
    css::blank_comments(&mut chars, &mut w);
    let rules = css::parse(&chars, &mut w);
    let (theme, layout) = translate(&rules, &mut w);
    w.list.sort_by_key(|x| (x.line, x.col));
    Ok(Parsed {
        theme,
        layout,
        warnings: w.list,
        dropped_warnings: w.dropped,
    })
}

// ---- selectors -------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Page,
    Card,
    Header,
    Button,
    Panel,
    Backdrop,
}

/// (role, whether the selector is global scope and so may declare custom properties).
fn classify(sel: &str) -> Result<(Role, bool), String> {
    match sel {
        ":root" | "html" | "body" | "*" => return Ok((Role::Page, true)),
        "button" => return Ok((Role::Button, false)),
        "header" => return Ok((Role::Header, false)),
        _ => {}
    }
    let ident = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if let Some(class) = sel.strip_prefix('.').filter(|c| ident(c)) {
        let role = match class {
            "backgroundContainer" => Role::Page,
            "card" | "cardBox" | "cardImageContainer" | "cardScalable" => Role::Card,
            "header" | "skinHeader" => Role::Header,
            "button" | "emby-button" => Role::Button,
            "panel" | "dialog" | "formDialog" => Role::Panel,
            "backdrop" | "detail-backdrop" | "backdropImage" | "itemBackdrop" => Role::Backdrop,
            _ => return Err("not a NOCTURNE or known jellyfin-web class".into()),
        };
        return Ok((role, false));
    }
    Err(if sel.contains("::") {
        "pseudo-elements are not supported"
    } else if sel.contains(':') {
        "pseudo-classes are not supported"
    } else if sel.contains('[') {
        "attribute selectors are not supported"
    } else if sel.starts_with('#') {
        "id selectors are not supported"
    } else if sel.contains(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~')) {
        "descendant and sibling selectors are not supported"
    } else if sel.matches('.').count() > 1 {
        "compound selectors are not supported"
    } else {
        "only the NOCTURNE selector vocabulary is supported"
    }
    .into())
}

/// Token a property sets on a role, if any.
fn role_prop(role: Role, prop: &str) -> Option<&'static str> {
    let bg = matches!(prop, "background" | "background-color");
    Some(match (role, prop) {
        (Role::Page, _) if bg => "background",
        (Role::Page, "color") => "text",
        (Role::Page, "font-family") => "font-family",
        (Role::Page, "font-size") => "font-size",
        (Role::Card, "border-radius") => "radius-card",
        (Role::Card, _) if bg => "raised",
        (Role::Header, _) if bg => "header",
        (Role::Button, _) if bg => "control",
        (Role::Button, "border-color") => "edge",
        (Role::Button, "border-radius") => "radius-small",
        (Role::Panel, _) if bg => "surface",
        (Role::Panel, "border-color") => "hairline",
        (Role::Panel, "border-radius") => "radius-panel",
        (Role::Backdrop, "opacity") => "backdrop-opacity",
        (Role::Backdrop, "filter") => "backdrop-blur",
        _ => return None,
    })
}

const TOKENS: &[&str] = &[
    "background",
    "surface",
    "raised",
    "header",
    "text",
    "text-secondary",
    "text-tertiary",
    "accent",
    "accent-hover",
    "accent-text",
    "on-accent",
    "edge",
    "hairline",
    "control",
    "control-hover",
    "danger",
    "warn",
    "scrim",
    "radius-panel",
    "radius-small",
    "radius-card",
    "density",
    "font-size",
    "font-family",
    "backdrop-opacity",
    "backdrop-blur",
];

/// Custom property to token: `--nocturne-<token>` plus common jellyfin-web theme variables.
fn var_token(name: &str) -> Option<&'static str> {
    if let Some(t) = name.strip_prefix("--nocturne-") {
        return TOKENS.iter().copied().find(|x| *x == t);
    }
    Some(match name {
        "--accentColor" | "--accent-color" => "accent",
        "--accentHoverColor" => "accent-hover",
        "--textColor" => "text",
        "--dimTextColor" => "text-secondary",
        "--borderColor" => "edge",
        "--headerColor" => "header",
        "--drawerColor" => "surface",
        "--selectorBackgroundColor" => "control",
        "--btnDeleteColor" => "danger",
        "--smallRadius" => "radius-small",
        "--largeRadius" => "radius-panel",
        "--rounding" => "radius-card",
        _ => return None,
    })
}

fn unsupported_reason(prop: &str) -> String {
    let p = prop.trim_start_matches('-').to_string();
    let starts = |s: &[&str]| s.iter().any(|x| p.starts_with(x));
    if starts(&[
        "transition",
        "animation",
        "webkit-transition",
        "webkit-animation",
    ]) {
        "animations and transitions are not supported".into()
    } else if starts(&[
        "box-shadow",
        "text-shadow",
        "backdrop-filter",
        "filter",
        "webkit-backdrop",
    ]) {
        format!("`{prop}`: shadows and blur effects are not supported here")
    } else if starts(&[
        "margin",
        "padding",
        "width",
        "height",
        "min-",
        "max-",
        "display",
        "position",
        "top",
        "left",
        "right",
        "bottom",
        "flex",
        "grid",
        "gap",
        "overflow",
        "z-index",
        "transform",
        "content",
    ]) {
        format!(
            "`{prop}`: arbitrary layout is not supported; set it through the parts (`.row`, `.card`, `.hero`, `.header`)"
        )
    } else {
        format!("`{prop}` is not supported here")
    }
}

// ---- var() -----------------------------------------------------------------------------------

enum VarErr {
    Undefined(String),
    Cycle(String),
    TooComplex,
    Syntax,
}

fn resolve(
    text: &str,
    vars: &HashMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<String, VarErr> {
    if !text.contains("var(") {
        return Ok(text.to_string());
    }
    if stack.len() > MAX_VAR_DEPTH {
        return Err(VarErr::TooComplex);
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find("var(") {
        let before = &rest[..p];
        if before.ends_with(|c: char| c.is_alphanumeric() || c == '-' || c == '_') {
            // Part of another identifier such as `somevar(`: copy through the paren.
            out.push_str(&rest[..p + 4]);
            rest = &rest[p + 4..];
            continue;
        }
        out.push_str(before);
        let args = &rest[p + 4..];
        let mut depth = 1;
        let close = args
            .char_indices()
            .find(|(_, c)| {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
            .map(|(i, _)| i)
            .ok_or(VarErr::Syntax)?;
        let inner = &args[..close];
        let (name, fallback) = match value::split_top(inner, Some(',')).as_slice() {
            [] => return Err(VarErr::Syntax),
            [n] => (n.to_string(), None),
            _ => {
                let at = inner.find(',').ok_or(VarErr::Syntax)?;
                (inner[..at].trim().to_string(), Some(inner[at + 1..].trim()))
            }
        };
        if !name.starts_with("--") {
            return Err(VarErr::Syntax);
        }
        let piece = match vars.get(&name) {
            Some(v) => {
                if stack.contains(&name) {
                    return Err(VarErr::Cycle(name));
                }
                stack.push(name);
                let r = resolve(v, vars, stack);
                stack.pop();
                r?
            }
            None => match fallback {
                Some(f) => resolve(f, vars, stack)?,
                None => return Err(VarErr::Undefined(name)),
            },
        };
        out.push_str(&piece);
        if out.len() > MAX_VALUE {
            return Err(VarErr::TooComplex);
        }
        rest = &args[close + 1..];
    }
    out.push_str(rest);
    if out.len() > MAX_VALUE {
        return Err(VarErr::TooComplex);
    }
    Ok(out)
}

// ---- translation -----------------------------------------------------------------------------

type Notes = Vec<(WarningKind, String)>;

fn clamp(v: f32, lo: f32, hi: f32, what: &str, notes: &mut Notes) -> f32 {
    if v < lo || v > hi {
        notes.push((
            WarningKind::Clamped,
            format!("{what} {v} is outside {lo}..{hi}; clamped"),
        ));
    }
    v.clamp(lo, hi)
}

fn blur_px(v: &str, notes: &mut Notes) -> Result<f32, String> {
    if v.trim()
        .starts_with(|c: char| c.is_ascii_digit() || c == '.')
    {
        return value::length(v, 0.0);
    }
    let mut blur = None;
    for f in value::split_top(v, None) {
        match f.strip_prefix("blur(").and_then(|r| r.strip_suffix(')')) {
            Some(arg) => blur = Some(value::length(arg, 0.0)?),
            None => notes.push((
                WarningKind::UnsupportedValue,
                format!("filter `{}` is not supported; only blur() is", css::clip(f)),
            )),
        }
    }
    blur.ok_or_else(|| "no blur() in this filter".into())
}

fn set(t: &mut Theme, tok: &str, v: &str, notes: &mut Notes) -> Result<(), String> {
    let col = |v: &str| {
        value::color(v).map_err(|e| {
            if v.contains("gradient(") {
                "gradients are only supported for the page background".to_string()
            } else {
                e
            }
        })
    };
    let first_len = |v: &str, pct: f32, notes: &mut Notes| {
        let parts = value::split_top(v, None);
        if parts.len() > 1 {
            notes.push((
                WarningKind::Approximated,
                format!("only the first of {} values is used", parts.len()),
            ));
        }
        value::length(parts.first().ok_or("empty value")?, pct)
    };
    match tok {
        "background" => {
            let (f, n) = value::fill(v)?;
            notes.extend(n.into_iter().map(|m| (WarningKind::Approximated, m)));
            t.background = Some(f);
        }
        "surface" => t.surface = Some(col(v)?),
        "raised" => t.raised = Some(col(v)?),
        "header" => t.header = Some(col(v)?),
        "text" => t.text = Some(col(v)?),
        "text-secondary" => t.text_secondary = Some(col(v)?),
        "text-tertiary" => t.text_tertiary = Some(col(v)?),
        "accent" => t.accent = Some(col(v)?),
        "accent-hover" => t.accent_hover = Some(col(v)?),
        "accent-text" => t.accent_text = Some(col(v)?),
        "on-accent" => t.on_accent = Some(col(v)?),
        "edge" => t.edge = Some(col(v)?),
        "hairline" => t.hairline = Some(col(v)?),
        "control" => t.control = Some(col(v)?),
        "control-hover" => t.control_hover = Some(col(v)?),
        "danger" => t.danger = Some(col(v)?),
        "warn" => t.warn = Some(col(v)?),
        "scrim" => t.scrim = Some(col(v)?),
        "radius-panel" | "radius-small" | "radius-card" => {
            let r = first_len(v, RADIUS_REF_PX, notes)?;
            let r = Some(clamp(r, 0.0, 48.0, "radius", notes));
            match tok {
                "radius-panel" => t.radius_panel = r,
                "radius-small" => t.radius_small = r,
                _ => t.radius_card = r,
            }
        }
        "density" => t.density = Some(clamp(value::number(v)?, 0.5, 2.0, "density", notes)),
        "font-size" => {
            let px = first_len(v, value::BASE_PX, notes)?;
            t.font_size = Some(clamp(px, 10.0, 32.0, "font size", notes));
        }
        "font-family" => {
            let name = value::split_top(v, Some(','))
                .into_iter()
                .map(|n| n.trim_matches(|c| c == '"' || c == '\'').trim())
                .find(|n| {
                    !n.is_empty()
                        && !matches!(
                            n.to_ascii_lowercase().as_str(),
                            "serif"
                                | "sans-serif"
                                | "monospace"
                                | "cursive"
                                | "fantasy"
                                | "system-ui"
                                | "ui-sans-serif"
                                | "inherit"
                                | "initial"
                        )
                });
            match name {
                Some(n) if n.len() <= 64 && !n.chars().any(char::is_control) => {
                    notes.push((
                        WarningKind::FontNotDownloaded,
                        format!("font `{n}` is used only if installed; fonts are never downloaded"),
                    ));
                    t.font_family = Some(n.to_string());
                }
                Some(_) => return Err("font name is not usable".into()),
                None => notes.push((
                    WarningKind::Approximated,
                    "only generic font families given; the native font is kept".into(),
                )),
            }
        }
        "backdrop-opacity" => {
            t.backdrop_opacity = Some(clamp(value::number(v)?, 0.0, 1.0, "opacity", notes))
        }
        "backdrop-blur" => {
            let px = blur_px(v, notes)?;
            t.backdrop_blur = Some(clamp(px, 0.0, 40.0, "blur", notes));
        }
        _ => return Err(format!("unknown token `{tok}`")),
    }
    Ok(())
}

/// Substitute `var()` and refuse `url()`; on failure the warning is already recorded.
fn resolved(
    raw: &str,
    pos: usize,
    vars: &HashMap<String, String>,
    w: &mut Warnings,
) -> Option<String> {
    let v = match resolve(raw, vars, &mut vec![]) {
        Ok(v) => v,
        Err(e) => {
            let (kind, msg) = match e {
                VarErr::Undefined(n) => (
                    WarningKind::UnresolvedVar,
                    format!("`{n}` is not defined and has no fallback"),
                ),
                VarErr::Cycle(n) => (
                    WarningKind::VarCycle,
                    format!("`{n}` refers to itself through var()"),
                ),
                VarErr::TooComplex => (
                    WarningKind::UnsupportedValue,
                    "var() nesting is too deep or the value too long".into(),
                ),
                VarErr::Syntax => (WarningKind::SyntaxError, "malformed var()".into()),
            };
            w.push(pos, kind, msg);
            return None;
        }
    };
    if v.contains("url(") {
        w.push(
            pos,
            WarningKind::RemoteResource,
            "url() resources are not fetched; the declaration was ignored",
        );
        return None;
    }
    Some(v)
}

fn apply(
    t: &mut Theme,
    tok: &str,
    raw: &str,
    pos: usize,
    vars: &HashMap<String, String>,
    w: &mut Warnings,
) {
    let Some(v) = resolved(raw, pos, vars, w) else {
        return;
    };
    let mut notes = vec![];
    if let Err(msg) = set(t, tok, &v, &mut notes) {
        w.push(pos, WarningKind::UnsupportedValue, msg);
    }
    for (k, m) in notes {
        w.push(pos, k, m);
    }
}

fn translate(rules: &[css::Rule], w: &mut Warnings) -> (Theme, Layout) {
    // Pass 1: custom properties from global-scope rules, last wins.
    let mut vars: HashMap<String, String> = HashMap::new();
    let classified: Vec<Vec<Result<(Role, bool), String>>> = rules
        .iter()
        .map(|r| r.sels.iter().map(|(s, _)| classify(s)).collect())
        .collect();
    for (rule, sels) in rules.iter().zip(&classified) {
        if sels.iter().any(|s| matches!(s, Ok((_, true)))) {
            for d in rule.decls.iter().filter(|d| d.name.starts_with("--")) {
                vars.insert(d.name.clone(), d.value.clone());
            }
        }
    }
    // Pass 2: apply in source order.
    let mut t = Theme::default();
    let mut lay = Layout::default();
    for (rule, sels) in rules.iter().zip(&classified) {
        let mut roles: Vec<Role> = vec![];
        let mut targets: Vec<layout::Target> = vec![];
        let mut global = false;
        let mut bad: Vec<(usize, &str, String)> = vec![];
        for ((text, pos), c) in rule.sels.iter().zip(sels) {
            let ls = layout::parse_selector(text);
            if let Ok((r, g)) = c {
                global |= g;
                if !roles.contains(r) {
                    roles.push(*r);
                }
            }
            match (c, ls) {
                (_, Some(Ok(tg))) => targets.push(tg),
                (Ok(_), _) => {}
                (Err(_), Some(Err(m))) => bad.push((*pos, text, m)),
                (Err(m), None) => bad.push((*pos, text, m.clone())),
            }
        }
        let tail = if roles.is_empty() && targets.is_empty() {
            format!("; {} declaration(s) skipped", rule.decls.len())
        } else {
            String::new()
        };
        for (pos, text, why) in bad {
            w.push(
                pos,
                WarningKind::UnsupportedSelector,
                format!("selector `{}`: {why}{tail}", css::clip(text)),
            );
        }
        if roles.is_empty() && targets.is_empty() {
            continue;
        }
        for d in &rule.decls {
            let mut handled = false;
            if !targets.is_empty() {
                let Some(v) = resolved(&d.value, d.pos, &vars, w) else {
                    continue;
                };
                for tg in &targets {
                    let mut notes = vec![];
                    match lay.set(tg, &d.name, &v, &mut notes) {
                        Ok(true) => handled = true,
                        Ok(false) => {}
                        Err(m) => {
                            handled = true;
                            w.push(d.pos, WarningKind::UnsupportedValue, m);
                        }
                    }
                    for (k, m) in notes {
                        w.push(d.pos, k, m);
                    }
                }
            }
            if d.name.starts_with("--") {
                if global {
                    if let Some(tok) = var_token(&d.name) {
                        apply(&mut t, tok, &d.value, d.pos, &vars, w);
                    }
                } else if !handled {
                    w.push(
                        d.pos,
                        WarningKind::UnsupportedProperty,
                        "custom properties are only read from :root, html, body and *, or as part settings listed in the reference",
                    );
                }
                continue;
            }
            for r in &roles {
                if let Some(tok) = role_prop(*r, &d.name) {
                    handled = true;
                    apply(&mut t, tok, &d.value, d.pos, &vars, w);
                }
            }
            if !handled {
                let kind = if d.value.contains("url(") {
                    WarningKind::RemoteResource
                } else {
                    WarningKind::UnsupportedProperty
                };
                let msg = if kind == WarningKind::RemoteResource {
                    "url() resources are not fetched".to_string()
                } else {
                    unsupported_reason(&d.name)
                };
                w.push(d.pos, kind, msg);
            }
        }
    }
    lay.prune();
    (t, lay)
}

/// Built-in presets (name, stylesheet) the Themes page can offer.
pub const PRESETS: &[(&str, &str)] = &[
    ("Marquee", include_str!("examples/marquee.css")),
    ("Feed", include_str!("examples/feed.css")),
    ("Classic", include_str!("examples/classic.css")),
];

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;
