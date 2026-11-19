//! The Theme Studio (feature `theme-engine`): pick any element of the live app, change it with controls, and the change becomes a rule in
//! the theme's CSS that stays editable by hand. The state here is the working copy of the CSS (with undo and redo), the selection
//! (an element of one of the engine's scenes, its inspection and the two ways to select "like it") and the plumbing: pick-mode events,
//! the keyboard, saving. The panel is in `studio_view.rs`; the inspector and the CSS editing are in `nocturne-style`
//! (`inspect.rs`, `css_edit.rs`), so everything that decides what is written is unit-tested without a window.
//! The Studio changes nothing outside the theme: the engine layer shows the working copy, safe look (Ctrl+Shift+0) still restores
//! the default, and the essential floor still applies (the panel explains why "hide" is off for an essential element).
use super::App;
use super::message::{Message, Which};
use crate::keys::Key as Pressed;
use crate::task::run;
use crate::widget::styled::{Mark, Nav, StyledEvent};
use iced::Task;
use iced::widget::text_editor;
use nocturne_style::css_edit::Css;
use nocturne_style::inspect::{self, Computed, Inspection, Like, Step};
use nocturne_style::{Dir, Id};
use std::time::{Duration, Instant};

/// Width of the docked panel.
pub const PANEL_W: f32 = 400.0;
/// First line of a theme made with the Studio: tells the app to layer it over the engine's default sheet.
pub const MARKER: &str = "/* nocturne:engine */";
const UNDO_CAP: usize = 200;
/// Edits of the same control within this time are one undo step (a slider drag).
const COALESCE: Duration = Duration::from_millis(700);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Edit,
    Css,
    Starters,
    Save,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    This,
    Like,
}

/// Every control of the edit panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Color,
    Background,
    BorderColor,
    Width,
    Height,
    PadY,
    PadX,
    MarY,
    MarX,
    Gap,
    Radius,
    BorderW,
    ShX,
    ShY,
    ShBlur,
    ShSpread,
    ShColor,
    Opacity,
    FontSize,
    Weight,
    Spacing,
    LineH,
    Transform,
    Align,
    Display,
    Direction,
    Wrap,
    AlignItems,
    Justify,
    Order,
    Position,
    Visibility,
    HoverScale,
    HoverBg,
    Transition,
    BarW,
    BarGap,
    BarColor,
}

impl Key {
    /// (min, max, step, unit) of a slider control.
    pub fn range(self) -> (f32, f32, f32, &'static str) {
        use Key::*;
        match self {
            Width => (0.0, 1600.0, 1.0, "px"),
            Height => (0.0, 1200.0, 1.0, "px"),
            PadY | PadX | MarY | MarX => (0.0, 120.0, 1.0, "px"),
            Gap => (0.0, 80.0, 1.0, "px"),
            Radius => (0.0, 60.0, 1.0, "px"),
            BorderW => (0.0, 12.0, 1.0, "px"),
            ShX => (-40.0, 40.0, 1.0, "px"),
            ShY => (-40.0, 60.0, 1.0, "px"),
            ShBlur => (0.0, 120.0, 1.0, "px"),
            ShSpread => (-20.0, 40.0, 1.0, "px"),
            Opacity => (0.0, 1.0, 0.01, ""),
            FontSize => (8.0, 96.0, 1.0, "px"),
            Weight => (100.0, 900.0, 100.0, ""),
            Spacing => (-2.0, 12.0, 0.1, "px"),
            LineH => (0.8, 2.5, 0.05, ""),
            Order => (-5.0, 10.0, 1.0, ""),
            HoverScale => (1.0, 1.4, 0.01, ""),
            Transition => (0.0, 800.0, 10.0, "ms"),
            BarW => (0.0, 24.0, 1.0, "px"),
            BarGap => (0.0, 40.0, 1.0, "px"),
            _ => (0.0, 1.0, 0.01, ""),
        }
    }

    /// The CSS property a control writes (for "another rule overrides this").
    pub fn prop(self) -> &'static str {
        use Key::*;
        match self {
            Color => "color",
            Background | HoverBg => "background-color",
            BorderColor => "border-color",
            Width => "width",
            Height => "height",
            PadY | PadX => "padding",
            MarY | MarX => "margin",
            Gap => "gap",
            Radius => "border-radius",
            BorderW => "border-width",
            ShX | ShY | ShBlur | ShSpread | ShColor => "box-shadow",
            Opacity => "opacity",
            FontSize => "font-size",
            Weight => "font-weight",
            Spacing => "letter-spacing",
            LineH => "line-height",
            Transform => "text-transform",
            Align => "text-align",
            Display => "display",
            Direction => "flex-direction",
            Wrap => "flex-wrap",
            AlignItems => "align-items",
            Justify => "justify-content",
            Order => "order",
            Position => "position",
            Visibility => "visibility",
            HoverScale => "transform",
            Transition => "transition",
            BarW | BarGap | BarColor => "border-left-width",
        }
    }
}

#[derive(Clone)]
pub enum Msg {
    /// Open the Studio over the live app (from the Themes page).
    Open,
    /// Leave the Studio (back to the Themes page).
    Close,
    /// Pick mode on or off.
    Pick(bool),
    Tab(Tab),
    Scope(Scope),
    LikeIx(usize),
    Num(Key, f32),
    NumText(Key, String),
    Choice(Key, &'static str),
    Color(Key, [u8; 4]),
    /// A colour typed as hex.
    Hex(Key, String),
    /// A colour from the theme's own tokens (`accent`, `text`...).
    Token(Key, &'static str),
    /// Which colour control shows its sliders.
    Expand(Key),
    Family(String),
    FamilyFilter(String),
    /// Back to the value the cascade gave (remove the declaration this scope wrote).
    Clear(Key),
    Hide(bool),
    Undo,
    Redo,
    ResetElement,
    Edit(text_editor::Action),
    Starter(usize),
    /// Select the parent, first child or a sibling of the selection.
    Move(Step),
    /// Make the property win with `!important` (when another rule overrides it).
    Force,
    Name(String),
    Save,
    Apply,
    UseHere,
    Export,
    Exported(Result<String, String>),
    /// QA (debug builds): select the first element matching a selector, in any scene.
    #[cfg(debug_assertions)]
    DebugSelect(String),
    /// QA (debug builds): run the commands of the file in `NOCTURNE_STUDIO_SCRIPT`.
    #[cfg(debug_assertions)]
    Script,
    /// QA (debug builds): the pointer over (or clicking) the first element matching a selector, as the paint widget would report it (the headless session has no pointer device).
    #[cfg(debug_assertions)]
    DebugPointer(String, bool),
}

impl Msg {
    /// Does this change what the scenes show? Only those rebuild them.
    pub fn dirties(&self) -> bool {
        matches!(
            self,
            Msg::Open
                | Msg::Close
                | Msg::Num(..)
                | Msg::NumText(..)
                | Msg::Choice(..)
                | Msg::Color(..)
                | Msg::Hex(..)
                | Msg::Token(..)
                | Msg::Family(_)
                | Msg::Clear(_)
                | Msg::Hide(_)
                | Msg::Undo
                | Msg::Redo
                | Msg::ResetElement
                | Msg::Edit(_)
                | Msg::Starter(_)
                | Msg::Force
        )
    }
}

/// The selection: an element of one scene, with what the panel shows about it.
pub struct Sel {
    pub w: Which,
    pub id: Id,
    pub insp: Inspection,
    pub this: String,
    pub likes: Vec<Like>,
    pub like_ix: usize,
}

/// Hover look of the scope, read from the working CSS.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Hover {
    pub scale: Option<f32>,
    pub bg: Option<[u8; 4]>,
    pub ms: Option<f32>,
    pub bar: Bar,
}

/// A bar at the start of an element (its left border and the padding after it): the accent rail of a section title. Width 0 means none. A border works on text elements too, which a generated leaf beside their own text cannot.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub w: f32,
    pub gap: f32,
    /// A CSS colour (hex or a theme token).
    pub color: String,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            w: 0.0,
            gap: 12.0,
            color: "var(--nocturne-accent)".into(),
        }
    }
}

/// The bar the working CSS gives `sel`.
pub fn read_bar(css: &Css, sel: &str) -> Bar {
    let d = Bar::default();
    let len = |v: Option<String>, d: f32| {
        v.and_then(|v| v.trim_end_matches("px").trim().parse::<f32>().ok())
            .unwrap_or(d)
    };
    Bar {
        w: len(css.get(sel, "border-left-width"), 0.0),
        gap: len(css.get(sel, "padding-left"), d.gap),
        color: css.get(sel, "border-left-color").unwrap_or(d.color),
    }
}

/// Write the bar. Width 0 takes it away.
pub fn write_bar(css: &mut Css, sel: &str, bar: &Bar) {
    if bar.w <= 0.0 {
        for p in ["border-left-width", "border-left-color", "padding-left"] {
            css.remove(sel, p);
        }
        return;
    }
    css.set(sel, "border-left-width", &px(bar.w), false);
    css.set(sel, "border-left-color", &bar.color, false);
    css.set(sel, "padding-left", &px(bar.gap), false);
}

pub struct Studio {
    pub open: bool,
    pub pick: bool,
    pub tab: Tab,
    pub scope: Scope,
    pub css: String,
    pub content: text_editor::Content,
    undo: Vec<String>,
    redo: Vec<String>,
    /// The element under the pointer (or the keyboard's cursor) while picking.
    pub cur: Option<(Which, Id)>,
    pub sel: Option<Sel>,
    /// A field being typed into: its text until it parses.
    pub field: Option<(Key, String)>,
    pub expanded: Option<Key>,
    last: Option<(String, Instant)>,
    /// The property last edited and the selector it went to (to notice an overriding rule).
    pub last_edit: Option<(String, &'static str)>,
    pub hover: Hover,
    pub name: String,
    pub saved: Option<u32>,
    pub note: Option<String>,
    pub family_filter: String,
    pub fonts: Vec<String>,
    /// Lines of what the engine said about the working CSS.
    pub warnings: Vec<String>,
}

impl Default for Studio {
    fn default() -> Self {
        Self {
            open: false,
            pick: false,
            tab: Tab::Edit,
            scope: Scope::This,
            css: String::new(),
            content: text_editor::Content::new(),
            undo: vec![],
            redo: vec![],
            cur: None,
            sel: None,
            field: None,
            expanded: None,
            last: None,
            last_edit: None,
            hover: Hover::default(),
            name: String::new(),
            saved: None,
            note: None,
            family_filter: String::new(),
            fonts: vec![],
            warnings: vec![],
        }
    }
}

impl Studio {
    /// Set the working copy; edits tagged alike within [`COALESCE`] share one undo step (a slider drag). `true` when the text changed.
    pub fn commit(&mut self, css: String, tag: Option<String>) -> bool {
        if css == self.css {
            return false;
        }
        let same = tag
            .as_ref()
            .zip(self.last.as_ref())
            .is_some_and(|(t, (l, at))| t == l && at.elapsed() < COALESCE);
        if !same {
            self.undo.push(std::mem::take(&mut self.css));
            if self.undo.len() > UNDO_CAP {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last = tag.map(|t| (t, Instant::now()));
        self.css = css;
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        let now = std::mem::replace(&mut self.css, prev);
        self.redo.push(now);
        self.content = text_editor::Content::with_text(&self.css);
        self.last = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let now = std::mem::replace(&mut self.css, next);
        self.undo.push(now);
        self.content = text_editor::Content::with_text(&self.css);
        self.last = None;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

// ---- values ----

/// `#rrggbbaa`, the form the CSS reader takes back exactly.
pub fn hex_of(c: [u8; 4]) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

pub fn parse_color(s: &str) -> Option<[u8; 4]> {
    let t = s.trim();
    let h = t.strip_prefix('#').unwrap_or(t);
    if h.len() > 8 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return nocturne_core::customize::value::color(t)
            .ok()
            .map(|c| [c.r, c.g, c.b, c.a]);
    }
    let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).map(|v| v * 17);
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16);
    match h.len() {
        3 => Some([d(0).ok()?, d(1).ok()?, d(2).ok()?, 255]),
        4 => Some([d(0).ok()?, d(1).ok()?, d(2).ok()?, d(3).ok()?]),
        6 => Some([p(0).ok()?, p(2).ok()?, p(4).ok()?, 255]),
        8 => Some([p(0).ok()?, p(2).ok()?, p(4).ok()?, p(6).ok()?]),
        _ => None,
    }
}

/// A number the way CSS wants it: no trailing zeros.
pub fn fmt(v: f32) -> String {
    let s = format!("{:.2}", (v * 100.0).round() / 100.0);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn px(v: f32) -> String {
    format!("{}px", fmt(v))
}

fn quoted(family: &str) -> String {
    format!("\"{}\"", family.replace(['"', '\\'], ""))
}

/// The value of `scale(x)` / `scale(x, y)`.
fn scale_of(v: &str) -> Option<f32> {
    let inner = v.trim().strip_prefix("scale(")?.strip_suffix(')')?;
    inner.split(',').next()?.trim().parse().ok()
}

/// The first duration (in ms) of a `transition` value.
fn ms_of(v: &str) -> Option<f32> {
    v.split_whitespace().find_map(|t| {
        t.strip_suffix("ms")
            .and_then(|n| n.parse().ok())
            .or_else(|| {
                t.strip_suffix('s')
                    .and_then(|n| n.parse::<f32>().ok())
                    .map(|s| s * 1000.0)
            })
    })
}

/// The hover look written for `sel` in `css`.
pub fn read_hover(css: &Css, sel: &str) -> Hover {
    let h = format!("{sel}:hover");
    Hover {
        scale: css.get(&h, "transform").as_deref().and_then(scale_of),
        bg: css
            .get(&h, "background-color")
            .as_deref()
            .and_then(parse_color),
        ms: css.get(sel, "transition").as_deref().and_then(ms_of),
        bar: read_bar(css, sel),
    }
}

// ---- the edits, as pure functions of the CSS text ----

/// Write one numeric control for `sel`. `c` is the element's computed style (for the controls that write a combined value).
pub fn write_num(css: &mut Css, sel: &str, c: &Computed, k: Key, v: f32) {
    let v = v.clamp(-10_000.0, 100_000.0);
    match k {
        Key::Width => css.set(sel, "width", &px(v), false),
        Key::Height => css.set(sel, "height", &px(v), false),
        Key::PadY => css.set(
            sel,
            "padding",
            &format!("{} {}", px(v), px(c.padding.1)),
            false,
        ),
        Key::PadX => css.set(
            sel,
            "padding",
            &format!("{} {}", px(c.padding.0), px(v)),
            false,
        ),
        Key::MarY => css.set(
            sel,
            "margin",
            &format!("{} {}", px(v), px(c.margin.1)),
            false,
        ),
        Key::MarX => css.set(
            sel,
            "margin",
            &format!("{} {}", px(c.margin.0), px(v)),
            false,
        ),
        Key::Gap => css.set(sel, "gap", &px(v), false),
        Key::Radius => css.set(sel, "border-radius", &px(v), false),
        Key::BorderW => css.set(sel, "border-width", &px(v), false),
        Key::ShX | Key::ShY | Key::ShBlur | Key::ShSpread => {
            let (mut x, mut y, mut b, mut s, col) =
                c.shadow.unwrap_or((0.0, 8.0, 24.0, 0.0, [0, 0, 0, 128]));
            match k {
                Key::ShX => x = v,
                Key::ShY => y = v,
                Key::ShBlur => b = v.max(0.0),
                _ => s = v,
            }
            css.set(sel, "box-shadow", &shadow(x, y, b, s, col), false);
        }
        Key::Opacity => css.set(sel, "opacity", &fmt(v.clamp(0.0, 1.0)), false),
        Key::FontSize => css.set(sel, "font-size", &px(v), false),
        Key::Weight => css.set(
            sel,
            "font-weight",
            &format!("{}", (v / 100.0).round() as i32 * 100),
            false,
        ),
        Key::Spacing => css.set(sel, "letter-spacing", &px(v), false),
        Key::LineH => css.set(sel, "line-height", &fmt(v), false),
        Key::Order => css.set(sel, "order", &format!("{}", v.round() as i32), false),
        Key::HoverScale => {
            let h = format!("{sel}:hover");
            if v <= 1.001 {
                css.remove(&h, "transform");
            } else {
                css.set(&h, "transform", &format!("scale({})", fmt(v)), false);
                ensure_transition(css, sel);
            }
        }
        Key::BarW | Key::BarGap => {
            let mut bar = read_bar(css, sel);
            if bar.w <= 0.0 && k != Key::BarW {
                bar.w = 4.0;
            }
            match k {
                Key::BarW => bar.w = v,
                _ => bar.gap = v,
            }
            write_bar(css, sel, &bar);
        }
        Key::Transition => {
            if v <= 0.0 {
                css.set(sel, "transition", "none", false);
            } else {
                css.set(
                    sel,
                    "transition",
                    &format!("all {}ms ease-out", fmt(v)),
                    false,
                );
            }
        }
        _ => {}
    }
}

fn shadow(x: f32, y: f32, b: f32, s: f32, col: [u8; 4]) -> String {
    format!("{} {} {} {} {}", px(x), px(y), px(b), px(s), hex_of(col))
}

/// A hover effect without a transition would jump: give the scope one.
fn ensure_transition(css: &mut Css, sel: &str) {
    if css.get(sel, "transition").is_none() {
        css.set(sel, "transition", "all 160ms ease-out", false);
    }
}

/// Write a colour control (`value` is a CSS colour: hex or `var(--nocturne-...)`).
pub fn write_color(css: &mut Css, sel: &str, c: &Computed, k: Key, value: &str) {
    match k {
        Key::Color => css.set(sel, "color", value, false),
        Key::Background => css.set(sel, "background-color", value, false),
        Key::BorderColor => {
            css.set(sel, "border-color", value, false);
            // A colour on a border that is 0 px wide would show nothing.
            if c.border_width < 0.5 {
                css.set(sel, "border-width", "1px", false);
            }
        }
        Key::ShColor => {
            let (x, y, b, s, _) = c.shadow.unwrap_or((0.0, 8.0, 24.0, 0.0, [0, 0, 0, 128]));
            css.set(
                sel,
                "box-shadow",
                &format!("{} {} {} {} {value}", px(x), px(y), px(b), px(s)),
                false,
            );
        }
        Key::BarColor => {
            let mut bar = read_bar(css, sel);
            if bar.w <= 0.0 {
                bar.w = 4.0;
            }
            bar.color = value.to_owned();
            write_bar(css, sel, &bar);
        }
        Key::HoverBg => {
            css.set(&format!("{sel}:hover"), "background-color", value, false);
            ensure_transition(css, sel);
        }
        _ => {}
    }
}

/// Write a keyword control.
pub fn write_choice(css: &mut Css, sel: &str, k: Key, v: &str) {
    css.set(sel, k.prop(), v, false);
}

/// The panel's way of hiding: `display: none`, undone by removing it.
pub fn write_hide(css: &mut Css, sel: &str, hide: bool) {
    if hide {
        css.set(sel, "display", "none", false);
    } else if css.get(sel, "display").as_deref() == Some("none") {
        css.remove(sel, "display");
    }
}

pub fn write_family(css: &mut Css, sel: &str, name: &str) {
    css.set(
        sel,
        "font-family",
        &format!("{}, sans-serif", quoted(name)),
        false,
    );
}

/// Put the engine marker on top of a text that has none.
pub fn with_marker(css: &str) -> String {
    if css.contains("nocturne:engine") {
        css.to_owned()
    } else {
        format!("{MARKER}\n{css}")
    }
}

// ---- the app side ----

impl App {
    pub(super) fn studio_picking(&self) -> bool {
        self.studio.open && self.studio.pick
    }

    /// Width the engine's scenes get: the window less the docked panel.
    pub(super) fn eng_width(&self) -> f32 {
        if self.studio.open {
            (self.win.width - PANEL_W).max(320.0)
        } else {
            self.win.width
        }
    }

    pub(super) fn studio_open(&mut self) -> Task<Message> {
        let fonts = self.eng_font_names();
        let st = &mut self.studio;
        st.open = true;
        st.pick = true;
        st.tab = Tab::Edit;
        st.css = self.th.content.text();
        st.content = text_editor::Content::with_text(&st.css);
        (st.undo, st.redo, st.last, st.sel, st.cur, st.field) =
            (vec![], vec![], None, None, None, None);
        st.name = self.th.name.clone();
        st.saved = self.th.sel;
        st.note = None;
        st.fonts = fonts;
        self.th.open = false;
        self.settings_open = false;
        self.studio_derive();
        Task::none()
    }

    fn studio_close(&mut self) -> Task<Message> {
        self.studio.open = false;
        self.studio.pick = false;
        self.studio.sel = None;
        self.studio.cur = None;
        // Back to the Themes page with the working copy in its editor, where it can still be previewed, saved or applied.
        self.th.open = true;
        self.eng.dirty = true;
        let css = self.studio.css.clone();
        self.set_text(&css)
    }

    fn scope_sel(&self) -> Option<String> {
        let s = self.studio.sel.as_ref()?;
        Some(match (self.studio.scope, s.likes.get(s.like_ix)) {
            (Scope::Like, Some(l)) => l.selector.clone(),
            _ => s.this.clone(),
        })
    }

    /// Set the working copy; edits of one control within [`COALESCE`] share an undo step.
    fn studio_commit(&mut self, css: String, tag: Option<String>) {
        if self.studio.commit(css, tag) {
            self.eng.dirty = true;
        }
    }

    /// Edit the CSS for the current scope with `f(css, selector, computed)`.
    fn studio_edit(
        &mut self,
        tag: String,
        prop: &'static str,
        f: impl FnOnce(&mut Css, &str, &Computed),
    ) {
        let (Some(sel), Some(s)) = (self.scope_sel(), self.studio.sel.as_ref()) else {
            return;
        };
        let mut css = Css::parse(&self.studio.css);
        f(&mut css, &sel, &s.insp.computed);
        let out = with_marker(&css.to_css());
        self.studio.last_edit = Some((sel, prop));
        self.studio_commit(out, Some(tag));
        self.studio.content = text_editor::Content::with_text(&self.studio.css);
    }

    /// Hover values and the text of the CSS tab follow the working copy.
    fn studio_derive(&mut self) {
        let css = Css::parse(&self.studio.css);
        self.studio.hover = self
            .scope_sel()
            .map_or_else(Hover::default, |s| read_hover(&css, &s));
    }

    pub(super) fn studio_msg(&mut self, m: Msg) -> Task<Message> {
        let task = self.studio_msg_inner(m);
        self.studio_derive();
        task
    }

    fn studio_msg_inner(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Open => return self.studio_open(),
            Msg::Close => return self.studio_close(),
            Msg::Pick(on) => {
                self.studio.pick = on;
                self.studio.cur = None;
            }
            Msg::Tab(t) => self.studio.tab = t,
            Msg::Scope(s) => self.studio.scope = s,
            Msg::LikeIx(i) => {
                if let Some(s) = self.studio.sel.as_mut() {
                    s.like_ix = i.min(s.likes.len().saturating_sub(1));
                }
            }
            Msg::Num(k, v) => {
                self.studio.field = None;
                self.studio_num(k, v);
            }
            Msg::NumText(k, t) => {
                match t
                    .trim()
                    .trim_end_matches("px")
                    .trim_end_matches("ms")
                    .parse::<f32>()
                {
                    Ok(v) if v.is_finite() => self.studio_num(k, v),
                    _ => {}
                }
                self.studio.field = Some((k, t));
            }
            Msg::Choice(k, v) => self.studio_edit(format!("{k:?}"), k.prop(), |c, s, _| {
                write_choice(c, s, k, v)
            }),
            Msg::Color(k, c) => self.studio_color(k, &hex_of(c)),
            Msg::Hex(k, t) => {
                if let Some(c) = parse_color(&t) {
                    self.studio_color(k, &hex_of(c));
                }
                self.studio.field = Some((k, t));
            }
            Msg::Token(k, name) => self.studio_color(k, &format!("var(--nocturne-{name})")),
            Msg::Expand(k) => {
                self.studio.expanded = (self.studio.expanded != Some(k)).then_some(k);
                self.studio.field = None;
            }
            Msg::Family(f) => self.studio_edit("Family".into(), "font-family", move |c, s, _| {
                write_family(c, s, &f)
            }),
            Msg::FamilyFilter(f) => self.studio.family_filter = f,
            Msg::Clear(k) => self.studio_edit(format!("Clear{k:?}"), k.prop(), move |c, s, _| {
                c.remove(s, k.prop());
                if matches!(k, Key::BarW | Key::BarGap | Key::BarColor) {
                    write_bar(c, s, &Bar::default());
                }
                if matches!(k, Key::HoverScale | Key::HoverBg) {
                    c.remove(&format!("{s}:hover"), k.prop());
                }
            }),
            Msg::Hide(h) => {
                self.studio_edit("Hide".into(), "display", move |c, s, _| write_hide(c, s, h))
            }
            Msg::Undo => {
                if self.studio.undo() {
                    self.eng.dirty = true;
                }
            }
            Msg::Redo => {
                if self.studio.redo() {
                    self.eng.dirty = true;
                }
            }
            Msg::ResetElement => {
                if let Some(sel) = self.scope_sel() {
                    let mut css = Css::parse(&self.studio.css);
                    if css.reset(&sel) {
                        self.studio_commit(css.to_css(), None);
                        self.studio.content = text_editor::Content::with_text(&self.studio.css);
                    }
                }
            }
            Msg::Edit(a) => {
                let edit = a.is_edit();
                self.studio.content.perform(a);
                if edit {
                    let text = self.studio.content.text();
                    self.studio_commit(text, Some("css".into()));
                }
            }
            Msg::Starter(i) => {
                let Some(css) = starter_css(i) else {
                    return Task::none();
                };
                self.studio_commit(with_marker(&css), None);
                self.studio.content = text_editor::Content::with_text(&self.studio.css);
                self.studio.tab = Tab::Edit;
                self.studio.note = Some(
                    "Started from a built-in look. Undo brings your previous work back.".into(),
                );
            }
            Msg::Move(s) => self.studio_move(s),
            Msg::Force => {
                if let Some((sel, prop)) = self.studio.last_edit.clone() {
                    let mut css = Css::parse(&self.studio.css);
                    if let Some(v) = css.get(&sel, prop) {
                        css.set(&sel, prop, &v, true);
                        self.studio_commit(css.to_css(), None);
                        self.studio.content = text_editor::Content::with_text(&self.studio.css);
                    }
                }
            }
            Msg::Name(n) => self.studio.name = n.chars().take(48).collect(),
            Msg::Save => {
                self.studio_save();
                self.studio.note = Some("Saved.".into());
            }
            Msg::Apply => {
                let id = self.studio_save();
                self.themes.set_global(Some(id));
                self.themes.save();
                self.th.safe = false;
                self.studio.note = Some("Applied to the whole app.".into());
            }
            Msg::UseHere => {
                if let Some(src) = self.theme_source() {
                    let id = self.studio_save();
                    self.themes.set_override(src, Some(id));
                    self.themes.save();
                    self.studio.note = Some("Used for this server only.".into());
                } else {
                    self.studio.note =
                        Some("Pick a single server in the source filter first.".into());
                }
            }
            Msg::Export => {
                let css = with_marker(&self.studio.css);
                let name = format!("{}.css", file_stem(&self.studio.name));
                return run(
                    async move {
                        let Some(h) = rfd::AsyncFileDialog::new()
                            .set_title("Export the theme")
                            .set_file_name(name)
                            .add_filter("CSS", &["css"])
                            .save_file()
                            .await
                        else {
                            return Ok(String::new());
                        };
                        std::fs::write(h.path(), css).map_err(|e| e.to_string())?;
                        Ok(h.path().display().to_string())
                    },
                    |r| Message::Studio(Msg::Exported(r)),
                );
            }
            #[cfg(debug_assertions)]
            Msg::DebugSelect(sel) if sel.is_empty() => {
                self.studio.sel = None;
                self.studio.cur = None;
            }
            #[cfg(debug_assertions)]
            Msg::DebugSelect(sel) => {
                let found = [Which::Search, Which::Page, Which::Home, Which::Header]
                    .into_iter()
                    .find_map(|w| {
                        let sc = self.eng_scene_of(w)?;
                        inspect::matches_of(&sc.doc, &sel).first().map(|&n| (w, n))
                    });
                if let Some((w, n)) = found {
                    self.studio_select(w, n);
                }
            }
            #[cfg(debug_assertions)]
            Msg::DebugPointer(sel, click) => {
                let found = [Which::Search, Which::Page, Which::Home, Which::Header]
                    .into_iter()
                    .find_map(|w| {
                        let sc = self.eng_scene_of(w)?;
                        inspect::matches_of(&sc.doc, &sel).first().map(|&n| (w, n))
                    });
                if let Some((w, n)) = found {
                    let e = if click {
                        StyledEvent::Click(n)
                    } else {
                        StyledEvent::Hover(Some(n))
                    };
                    return Task::done(Message::Eng(w, e));
                }
            }
            #[cfg(debug_assertions)]
            Msg::Script => {
                let text = std::env::var("NOCTURNE_STUDIO_SCRIPT")
                    .ok()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .unwrap_or_default();
                let mut t = Task::none();
                for m in script_msgs(&text) {
                    t = t.chain(Task::done(Message::Studio(m)));
                }
                return t;
            }
            Msg::Exported(r) => {
                self.studio.note = Some(match r {
                    Ok(p) if p.is_empty() => "Export cancelled.".into(),
                    Ok(p) => format!("Exported to {p}."),
                    Err(e) => format!("Could not export: {e}"),
                });
            }
        }
        Task::none()
    }

    fn studio_num(&mut self, k: Key, v: f32) {
        self.studio_edit(format!("{k:?}"), k.prop(), move |c, s, comp| {
            write_num(c, s, comp, k, v)
        });
    }

    fn studio_color(&mut self, k: Key, value: &str) {
        let v = value.to_owned();
        self.studio_edit(format!("{k:?}"), k.prop(), move |c, s, comp| {
            write_color(c, s, comp, k, &v)
        });
    }

    fn studio_save(&mut self) -> u32 {
        let css = with_marker(&self.studio.css);
        let name = if self.studio.name.trim().is_empty() {
            "My theme".to_owned()
        } else {
            self.studio.name.trim().to_owned()
        };
        let id = match self.studio.saved {
            Some(id) if self.themes.update(id, &name, &css, None) => id,
            _ => self.themes.add(&name, &css, None),
        };
        self.studio.saved = Some(id);
        self.studio.name = name;
        self.themes.save();
        self.th_reapply();
        id
    }

    // ---- selection ----

    pub(super) fn studio_select(&mut self, w: Which, id: Id) {
        let Some(scene) = self.eng_scene_of(w) else {
            return;
        };
        if id >= scene.doc.len() {
            return;
        }
        let id = inspect::owner(&scene.doc, id);
        let sel = Sel {
            w,
            id,
            insp: inspect::inspect(scene, id),
            this: inspect::this_selector(&scene.doc, id),
            likes: inspect::like_selectors(&scene.doc, &scene.sheet, id),
            like_ix: 0,
        };
        let warnings = scene.warnings().lines();
        self.studio.sel = Some(sel);
        self.studio.cur = Some((w, id));
        self.studio.field = None;
        self.studio.last = None;
        self.studio.warnings = warnings;
        self.studio_derive();
    }

    /// The scenes were rebuilt: look at the selection again (the same element when it is still there, found by its selector otherwise).
    pub(super) fn studio_refresh(&mut self) {
        if !self.studio.open {
            return;
        }
        let Some(old) = self.studio.sel.as_ref() else {
            return;
        };
        let (w, id, this, ix, label) = (
            old.w,
            old.id,
            old.this.clone(),
            old.like_ix,
            old.insp.info.path.last().cloned(),
        );
        let Some(scene) = self.eng_scene_of(w) else {
            return;
        };
        let same = id < scene.doc.len() && Some(inspect::label(&scene.doc, id)) == label;
        let found = if same {
            Some(id)
        } else {
            inspect::matches_of(&scene.doc, &this).first().copied()
        };
        let Some(n) = found else {
            self.studio.sel = None;
            return;
        };
        let sel = Sel {
            w,
            id: n,
            insp: inspect::inspect(scene, n),
            this,
            likes: inspect::like_selectors(&scene.doc, &scene.sheet, n),
            like_ix: ix,
        };
        let warnings = scene.warnings().lines();
        self.studio.sel = Some(Sel {
            like_ix: ix.min(sel.likes.len().saturating_sub(1)),
            ..sel
        });
        self.studio.warnings = warnings;
        self.studio_derive();
    }

    fn studio_move(&mut self, to: Step) {
        let base = self
            .studio
            .sel
            .as_ref()
            .map(|s| (s.w, s.id))
            .or(self.studio.cur);
        let target = match base {
            Some((w, id)) => self
                .eng_scene_of(w)
                .and_then(|sc| inspect::step(&sc.doc, id, to))
                .map(|n| (w, n)),
            // Keyboard only, nothing yet: start at the first element of the main scene.
            None => self.eng_main_scene().and_then(|w| {
                let sc = self.eng_scene_of(w)?;
                inspect::step(&sc.doc, 0, Step::Child).map(|n| (w, n))
            }),
        };
        if let Some((w, n)) = target {
            self.studio_select(w, n);
        }
    }

    /// Events of the scenes while picking: the pointer outlines and selects, keys move the selection. `None` leaves the event to the normal path (scrolling, resizing, frames).
    pub(super) fn studio_event(&mut self, w: Which, e: &StyledEvent) -> Option<Task<Message>> {
        if !self.studio_picking() {
            return None;
        }
        match e {
            StyledEvent::Hover(n) => {
                self.studio.cur = n.map(|n| (w, n));
                Some(Task::none())
            }
            StyledEvent::Click(n) => {
                self.studio_select(w, *n);
                Some(Task::none())
            }
            StyledEvent::Active(_) => Some(Task::none()),
            StyledEvent::Nav(nav) => {
                let name = match nav {
                    Nav::Dir(Dir::Up) => "up",
                    Nav::Dir(Dir::Down) => "down",
                    Nav::Dir(Dir::Left) => "left",
                    Nav::Dir(Dir::Right) => "right",
                    Nav::Activate => "enter",
                    Nav::Next | Nav::Prev => "",
                };
                self.studio_pick_key(name);
                Some(Task::none())
            }
            _ => None,
        }
    }

    fn studio_pick_key(&mut self, name: &str) -> bool {
        match name {
            "up" => self.studio_move(Step::Parent),
            "down" => self.studio_move(Step::Child),
            "left" => self.studio_move(Step::Prev),
            "right" => self.studio_move(Step::Next),
            "enter" => {
                if let Some((w, n)) = self.studio.cur {
                    self.studio_select(w, n);
                } else {
                    self.studio_move(Step::Child);
                }
            }
            "escape" => self.studio.pick = false,
            _ => return false,
        }
        self.studio_derive();
        true
    }

    /// Keys while the Studio is open. Esc leaves pick mode; the arrows and Enter move and choose while picking; Ctrl+Z / Ctrl+Y undo and redo.
    pub(super) fn studio_key(&mut self, k: &Pressed) -> Option<Task<Message>> {
        if !self.studio.open {
            return None;
        }
        if k.ctrl && !k.alt && matches!(k.name.as_str(), "z" | "y") {
            let m = if k.name == "y" || k.shift {
                Msg::Redo
            } else {
                Msg::Undo
            };
            return Some(self.studio_msg(m));
        }
        if self.studio.pick && k.plain() && self.studio_pick_key(&k.name) {
            return Some(Task::none());
        }
        None
    }

    /// The outlines to draw on scene `w`: the element under the pointer and the selection.
    pub(super) fn studio_marks(&self, w: Which) -> Vec<Mark> {
        let mut v = vec![];
        if !self.studio.open {
            return v;
        }
        let Some(scene) = self.eng_scene_of(w) else {
            return v;
        };
        let sel = self
            .studio
            .sel
            .as_ref()
            .filter(|s| s.w == w && s.id < scene.doc.len());
        if let Some((cw, id)) = self.studio.cur
            && cw == w
            && id < scene.doc.len()
            && self.studio.pick
            && sel.is_none_or(|s| s.id != id)
        {
            v.push(Mark {
                node: id,
                label: Some(inspect::label(&scene.doc, id)),
                selected: false,
            });
        }
        if let Some(s) = sel {
            v.push(Mark {
                node: s.id,
                label: Some(inspect::label(&scene.doc, s.id)),
                selected: true,
            });
        }
        v
    }
}

/// The starting points: Spotlight, then the stage-1 examples (Marquee, Feed, Classic).
pub fn starters() -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<(&str, &str)> = nocturne_style::STARTERS.to_vec();
    v.extend(nocturne_core::customize::PRESETS.iter().copied());
    v
}

pub fn starter_css(i: usize) -> Option<String> {
    starters().get(i).map(|(_, c)| (*c).to_owned())
}

fn file_stem(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.trim_matches('-').is_empty() {
        "theme".into()
    } else {
        s
    }
}

// ---- QA scripting (debug builds only) ----

/// `NOCTURNE_KEYS=studio-script@4000` runs the commands of the file in `NOCTURNE_STUDIO_SCRIPT`, one per line, through the same messages the panel sends: `open`, `select <selector>`, `scope this|like`, `like <n>`, `num <Key> <value>`, `color <Key> <hex>`, `choice <Key> <keyword>`, `hide 0|1`, `family <name>`, `tab edit|css|starters|save`, `starter <n>`, `undo`, `redo`, `reset`, `pick 0|1`, `name <text>`, `save`.
#[cfg(debug_assertions)]
pub fn script_msgs(text: &str) -> Vec<Msg> {
    const KEYS: &[Key] = &[
        Key::Color,
        Key::Background,
        Key::BorderColor,
        Key::Width,
        Key::Height,
        Key::PadY,
        Key::PadX,
        Key::MarY,
        Key::MarX,
        Key::Gap,
        Key::Radius,
        Key::BorderW,
        Key::ShX,
        Key::ShY,
        Key::ShBlur,
        Key::ShSpread,
        Key::ShColor,
        Key::Opacity,
        Key::FontSize,
        Key::Weight,
        Key::Spacing,
        Key::LineH,
        Key::Transform,
        Key::Align,
        Key::Display,
        Key::Direction,
        Key::Wrap,
        Key::AlignItems,
        Key::Justify,
        Key::Order,
        Key::Position,
        Key::Visibility,
        Key::HoverScale,
        Key::HoverBg,
        Key::Transition,
        Key::BarW,
        Key::BarGap,
        Key::BarColor,
    ];
    let key = |s: &str| KEYS.iter().copied().find(|k| format!("{k:?}") == s);
    let leak = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (cmd, rest) = l.split_once(' ').unwrap_or((l, ""));
            let mut it = rest.split_whitespace();
            Some(match cmd {
                "open" => Msg::Open,
                "select" => Msg::DebugSelect(rest.to_owned()),
                "scope" => Msg::Scope(if rest == "like" {
                    Scope::Like
                } else {
                    Scope::This
                }),
                "like" => Msg::LikeIx(rest.parse().ok()?),
                "num" => Msg::Num(key(it.next()?)?, it.next()?.parse().ok()?),
                "color" => Msg::Hex(key(it.next()?)?, it.next()?.to_owned()),
                "choice" => Msg::Choice(key(it.next()?)?, leak(it.next()?)),
                "hide" => Msg::Hide(rest == "1"),
                "family" => Msg::Family(rest.to_owned()),
                "tab" => Msg::Tab(match rest {
                    "css" => Tab::Css,
                    "starters" => Tab::Starters,
                    "save" => Tab::Save,
                    _ => Tab::Edit,
                }),
                "starter" => Msg::Starter(rest.parse().ok()?),
                "undo" => Msg::Undo,
                "redo" => Msg::Redo,
                "reset" => Msg::ResetElement,
                "pick" => Msg::Pick(rest == "1"),
                "name" => Msg::Name(rest.to_owned()),
                "save" => Msg::Save,
                "hover" => Msg::DebugPointer(rest.to_owned(), false),
                "click" => Msg::DebugPointer(rest.to_owned(), true),
                "apply" => Msg::Apply,
                "close" => Msg::Close,
                "clear" => Msg::DebugSelect(String::new()),
                _ => return None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp() -> Computed {
        Computed {
            padding: (4.0, 8.0),
            margin: (0.0, 2.0),
            border_width: 0.0,
            ..Computed::default()
        }
    }

    #[test]
    fn colours_round_trip() {
        for c in [[1, 2, 3, 255], [255, 128, 0, 17], [0, 0, 0, 0]] {
            assert_eq!(parse_color(&hex_of(c)), Some(c));
        }
        assert_eq!(parse_color("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(parse_color("abc"), Some([170, 187, 204, 255]));
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color("nope"), None);
        assert!(parse_color("rgba(0, 0, 0, 0.5)").is_some());
    }

    #[test]
    fn numbers_print_without_noise() {
        assert_eq!(fmt(12.0), "12");
        assert_eq!(fmt(1.5), "1.5");
        assert_eq!(fmt(0.1 + 0.2), "0.3");
        assert_eq!(fmt(-2.0), "-2");
    }

    #[test]
    fn padding_controls_share_one_declaration() {
        let mut css = Css::default();
        write_num(&mut css, ".b", &comp(), Key::PadY, 10.0);
        assert_eq!(css.get(".b", "padding").as_deref(), Some("10px 8px"));
        write_num(&mut css, ".b", &comp(), Key::PadX, 20.0);
        assert_eq!(css.get(".b", "padding").as_deref(), Some("4px 20px"));
    }

    #[test]
    fn shadow_defaults_and_updates() {
        let mut css = Css::default();
        write_num(&mut css, ".b", &comp(), Key::ShBlur, 30.0);
        assert_eq!(
            css.get(".b", "box-shadow").as_deref(),
            Some("0px 8px 30px 0px #00000080")
        );
        let mut c = comp();
        c.shadow = Some((1.0, 2.0, 3.0, 4.0, [255, 0, 0, 255]));
        write_color(&mut css, ".b", &c, Key::ShColor, "#00ff00");
        assert_eq!(
            css.get(".b", "box-shadow").as_deref(),
            Some("1px 2px 3px 4px #00ff00")
        );
    }

    #[test]
    fn border_colour_gives_the_border_a_width() {
        let mut css = Css::default();
        write_color(&mut css, ".b", &comp(), Key::BorderColor, "#fff");
        assert_eq!(css.get(".b", "border-width").as_deref(), Some("1px"));
        let mut c = comp();
        c.border_width = 3.0;
        let mut css = Css::default();
        write_color(&mut css, ".b", &c, Key::BorderColor, "#fff");
        assert_eq!(css.get(".b", "border-width"), None);
    }

    #[test]
    fn hover_effects_write_a_hover_rule_and_a_transition() {
        let mut css = Css::default();
        write_num(&mut css, ".card", &comp(), Key::HoverScale, 1.08);
        assert_eq!(
            css.get(".card:hover", "transform").as_deref(),
            Some("scale(1.08)")
        );
        assert!(css.get(".card", "transition").is_some());
        write_color(&mut css, ".card", &comp(), Key::HoverBg, "#ffffff22");
        let h = read_hover(&css, ".card");
        assert_eq!(h.scale, Some(1.08));
        assert_eq!(h.bg, Some([255, 255, 255, 0x22]));
        assert_eq!(h.ms, Some(160.0));
        write_num(&mut css, ".card", &comp(), Key::Transition, 300.0);
        assert_eq!(read_hover(&css, ".card").ms, Some(300.0));
        write_num(&mut css, ".card", &comp(), Key::HoverScale, 1.0);
        assert_eq!(css.get(".card:hover", "transform"), None);
    }

    #[test]
    fn the_accent_bar_is_a_left_border_with_room_after_it() {
        let mut css = Css::default();
        write_num(&mut css, ".t", &comp(), Key::BarW, 4.0);
        assert_eq!(css.get(".t", "border-left-width").as_deref(), Some("4px"));
        assert_eq!(css.get(".t", "padding-left").as_deref(), Some("12px"));
        write_color(&mut css, ".t", &comp(), Key::BarColor, "#ff0000");
        write_num(&mut css, ".t", &comp(), Key::BarGap, 20.0);
        let b = read_bar(&css, ".t");
        assert_eq!((b.w, b.gap, b.color.as_str()), (4.0, 20.0, "#ff0000"));
        write_num(&mut css, ".t", &comp(), Key::BarW, 0.0);
        assert!(!css.has(".t"), "no bar left: {}", css.to_css());
    }

    #[test]
    fn hide_toggles_only_its_own_declaration() {
        let mut css = Css::parse(".a { display: flex; }");
        write_hide(&mut css, ".a", false);
        assert_eq!(css.get(".a", "display").as_deref(), Some("flex"));
        write_hide(&mut css, ".a", true);
        assert_eq!(css.get(".a", "display").as_deref(), Some("none"));
        write_hide(&mut css, ".a", false);
        assert_eq!(css.get(".a", "display"), None);
    }

    #[test]
    fn marker_is_added_once() {
        let m = with_marker(".a{}");
        assert!(m.starts_with(MARKER));
        assert_eq!(with_marker(&m), m);
        assert!(nocturne_style::is_engine_sheet(&m));
    }

    #[test]
    fn family_is_quoted_with_a_fallback() {
        let mut css = Css::default();
        write_family(&mut css, ".a", "Fira \"Sans\"");
        assert_eq!(
            css.get(".a", "font-family").as_deref(),
            Some("\"Fira Sans\", sans-serif")
        );
    }

    #[test]
    fn history_undoes_redoes_and_coalesces_a_drag() {
        let mut st = Studio::default();
        assert!(st.commit("a".into(), None));
        assert!(!st.commit("a".into(), None), "no change, no step");
        // A slider drag: the same tag in quick succession is one step.
        st.commit("b".into(), Some("Width".into()));
        st.commit("c".into(), Some("Width".into()));
        st.commit("d".into(), Some("Width".into()));
        assert_eq!(st.css, "d");
        assert!(st.undo());
        assert_eq!(st.css, "a", "the whole drag is one undo step");
        assert!(st.redo());
        assert_eq!(st.css, "d");
        assert!(st.undo() && st.undo());
        assert_eq!(st.css, "");
        assert!(!st.undo());
        // A new edit clears what could be redone.
        assert!(st.can_redo());
        st.commit("x".into(), None);
        assert!(!st.can_redo());
        // Another control is another step.
        st.commit("y".into(), Some("Radius".into()));
        st.commit("z".into(), Some("Gap".into()));
        assert!(st.undo());
        assert_eq!(st.css, "y");
        assert_eq!(
            st.content.text().trim_end(),
            "y",
            "the CSS tab follows undo"
        );
    }

    #[test]
    fn starters_include_the_four_looks() {
        let names: Vec<_> = starters().iter().map(|s| s.0).collect();
        for n in ["Spotlight", "Marquee", "Feed", "Classic"] {
            assert!(names.contains(&n), "{n}");
        }
    }
}
