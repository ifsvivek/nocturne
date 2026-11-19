//! Layout vocabulary: named app parts (`.card`, `.row`, `.hero`, ...), the few properties each one
//! accepts, and the typed [`Layout`] they translate into. Like the colour tokens every field is
//! optional (`None` keeps the built-in value) and layers merge with [`Layout::overlay`]. Stage 1 of
//! the layout work; `docs/dev/CUSTOMIZATION_PLAN.md` section 14 is the user-facing reference.
use super::value::{self, BASE_PX, Color, Fill};
use super::{Notes, WarningKind, clamp};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aspect {
    /// 2:3, the poster image.
    Poster,
    /// 16:9, the thumbnail or backdrop image.
    Thumb,
    /// 1:1.
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitlePosition {
    Below,
    Overlay,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowLayout {
    Carousel,
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroStyle {
    /// Full-bleed artwork behind the header.
    Billboard,
    /// A shorter band below the header.
    Banner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderPosition {
    /// Overlays the content (which then starts at the top of the window).
    Fixed,
    /// Takes its own height and pushes the content down.
    Static,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextTransform {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgePos {
    TopRight,
    TopLeft,
    /// The built-in place.
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressPos {
    /// The built-in bar on the bottom edge of the image.
    InsideBottom,
    /// A bar between the image and the caption.
    Below,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpisodeFormat {
    /// "S1:E2 - Name".
    Short,
    /// "Season 1, Episode 2 - Name".
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileLabel {
    /// Library name large and centred over the image.
    OverlayCenter,
    /// The built-in caption under the image.
    Below,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderAlign {
    /// Links centred in the header.
    Center,
    /// Links right after the brand (the built-in).
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrandContent {
    Server,
    User,
    App,
}

/// Pages a rule can be scoped to (`.page-<name> .card`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Page {
    Home,
    Library,
    Detail,
    Search,
}

/// Home sections (`.section-<name>`), in their built-in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Resume,
    NextUp,
    /// Every "latest in library" row.
    Latest,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Resume, Section::NextUp, Section::Latest];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Base,
    Page(Page),
    Section(Section),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartId {
    Card,
    CardImage,
    CardTitle,
    CardMeta,
    CardSubtitle,
    CardBadge,
    CardProgress,
    LibraryTile,
    Row,
    RowTitle,
    RowArrows,
    Hero,
    HeroTitle,
    HeroMeta,
    Header,
    HeaderLink,
    HeaderBrand,
    HeaderIcons,
    HeaderNav,
    HeaderSearch,
    HeaderHistory,
    HeaderAccount,
    Button,
    DetailTitle,
    DetailText,
    PlayerControls,
}

const PARTS: &[(&str, PartId)] = &[
    ("card", PartId::Card),
    ("card-image", PartId::CardImage),
    ("card-title", PartId::CardTitle),
    ("card-meta", PartId::CardMeta),
    ("card-subtitle", PartId::CardSubtitle),
    ("card-badge", PartId::CardBadge),
    ("card-progress", PartId::CardProgress),
    ("library-tile", PartId::LibraryTile),
    ("row", PartId::Row),
    ("row-title", PartId::RowTitle),
    ("row-arrows", PartId::RowArrows),
    ("hero", PartId::Hero),
    ("hero-title", PartId::HeroTitle),
    ("hero-meta", PartId::HeroMeta),
    ("header", PartId::Header),
    ("header-link", PartId::HeaderLink),
    ("header-brand", PartId::HeaderBrand),
    ("header-icons", PartId::HeaderIcons),
    ("header-nav", PartId::HeaderNav),
    ("header-search", PartId::HeaderSearch),
    ("header-history", PartId::HeaderHistory),
    ("header-account", PartId::HeaderAccount),
    ("button", PartId::Button),
    ("detail-title", PartId::DetailTitle),
    ("detail-text", PartId::DetailText),
    ("player-controls", PartId::PlayerControls),
];

/// Declares structs of optional fields, each with `overlay` and `count`.
macro_rules! opt_struct {
    ($($(#[$m:meta])* $name:ident { $($f:ident: $t:ty),* $(,)? })*) => {$(
        $(#[$m])*
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $name { $(pub $f: Option<$t>),* }
        impl $name {
            fn overlay(&mut self, top: &Self) {
                $( if top.$f.is_some() { self.$f = top.$f.clone(); } )*
            }
            fn count(&self) -> usize { 0 $(+ usize::from(self.$f.is_some()))* }
        }
    )*};
}

opt_struct! {
    /// Typography of a text part.
    Text {
        hidden: bool, font_size: f32, font_weight: u16, text_transform: TextTransform,
        letter_spacing: f32, line_height: f32, color: Color, align: TextAlign,
    }
    /// The whole cell: image and caption.
    Card {
        hidden: bool, aspect: Aspect, width: f32, title_position: TitlePosition,
        hover_scale: f32, transition_ms: u32, episode_format: EpisodeFormat, tile_label: TileLabel,
    }
    /// The unplayed-count badge on a card.
    Badge {
        hidden: bool, position: BadgePos, background: Color, color: Color, font_size: f32,
        radius: f32,
    }
    /// The resume progress bar on a card.
    Progress { hidden: bool, position: ProgressPos, height: f32, background: Color, color: Color }
    /// The previous / next buttons at the right of a row title (`hidden: Some(false)` shows them).
    Arrows { hidden: bool, size: f32, color: Color }
    /// The server or user name at the left of the header (`hidden: Some(false)` shows it).
    Brand { hidden: bool, content: BrandContent, font_size: f32, font_weight: u16, color: Color }
    CardImage { radius: f32, shadow: Shadow }
    Row {
        hidden: bool, layout: RowLayout, gap: f32, columns: u8, padding: f32, order: i32,
        details: bool,
    }
    Hero { hidden: bool, style: HeroStyle, height: f32, overlay: Fill }
    Header { position: HeaderPosition, height: f32, align: HeaderAlign, link_icons: bool }
    Visibility { hidden: bool }
    Button { hover_scale: f32, transition_ms: u32 }
    Controls { background: Fill }
}

impl Text {
    /// `top` laid over `self` (the subtitle over the details line).
    pub fn merged(&self, top: &Text) -> Text {
        let mut m = self.clone();
        m.overlay(top);
        m
    }
}

impl Arrows {
    pub fn shown(&self) -> bool {
        self.hidden == Some(false)
    }
}

impl Brand {
    pub fn shown(&self) -> bool {
        self.hidden == Some(false)
    }
}

macro_rules! parts_struct {
    ($($f:ident: $t:ident),* $(,)?) => {
        /// Every part's settings for one scope.
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct Parts { $(pub $f: $t),* }
        impl Parts {
            fn overlay(&mut self, top: &Parts) { $( self.$f.overlay(&top.$f); )* }
            fn count(&self) -> usize { 0 $(+ self.$f.count())* }
        }
    };
}

parts_struct! {
    card: Card, card_image: CardImage, card_title: Text, card_meta: Text, card_subtitle: Text,
    card_badge: Badge, card_progress: Progress, library_tile: Text,
    row: Row, row_title: Text, row_arrows: Arrows, hero: Hero, hero_title: Text, hero_meta: Text,
    header: Header, header_link: Text, header_brand: Brand, header_icons: Visibility,
    header_nav: Visibility, header_search: Visibility,
    header_history: Visibility, header_account: Visibility, button: Button,
    detail_title: Text, detail_text: Text, player_controls: Controls,
}

/// What the stylesheet says about layout: a base set plus per-page and per-section overrides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub base: Parts,
    pub pages: BTreeMap<Page, Parts>,
    pub sections: BTreeMap<Section, Parts>,
}

impl Layout {
    /// How many settings the stylesheet sets.
    pub fn count(&self) -> usize {
        self.base.count()
            + self.pages.values().map(Parts::count).sum::<usize>()
            + self.sections.values().map(Parts::count).sum::<usize>()
    }

    /// Lay `top` over `self`: every field `top` sets wins.
    pub fn overlay(&mut self, top: &Layout) {
        self.base.overlay(&top.base);
        for (k, v) in &top.pages {
            self.pages.entry(*k).or_default().overlay(v);
        }
        for (k, v) in &top.sections {
            self.sections.entry(*k).or_default().overlay(v);
        }
    }

    /// Settings for a view: the base, then the page's, then the section's.
    pub fn parts(&self, page: Option<Page>, section: Option<Section>) -> Parts {
        let mut p = self.base.clone();
        if let Some(x) = page.and_then(|k| self.pages.get(&k)) {
            p.overlay(x);
        }
        if let Some(x) = section.and_then(|k| self.sections.get(&k)) {
            p.overlay(x);
        }
        p
    }

    /// Home sections in display order (`order`, ties keep the built-in order). Hidden ones stay
    /// in the list; check `parts(..).row.hidden`.
    pub fn section_order(&self) -> [Section; 3] {
        let mut s = Section::ALL;
        s.sort_by_key(|k| self.sections.get(k).and_then(|p| p.row.order).unwrap_or(0));
        s
    }

    fn scope_mut(&mut self, s: Scope) -> &mut Parts {
        match s {
            Scope::Base => &mut self.base,
            Scope::Page(p) => self.pages.entry(p).or_default(),
            Scope::Section(k) => self.sections.entry(k).or_default(),
        }
    }

    /// Drop scopes nothing was set in (a rejected declaration may have created one).
    pub(super) fn prune(&mut self) {
        self.pages.retain(|_, p| p.count() > 0);
        self.sections.retain(|_, p| p.count() > 0);
    }

    /// Apply one declaration. `Ok(false)`: this part does not take the property.
    pub(super) fn set(
        &mut self,
        t: &Target,
        name: &str,
        v: &str,
        notes: &mut Notes,
    ) -> Result<bool, String> {
        set_part(self.scope_mut(t.scope), t, name, v.trim(), notes)
    }
}

/// A parsed layout selector.
pub(super) struct Target {
    pub scope: Scope,
    pub part: PartId,
    pub hover: bool,
}

fn scope_of(class: &str) -> Option<Result<Scope, String>> {
    let name = class.strip_prefix('.')?;
    if let Some(p) = name.strip_prefix("page-") {
        return Some(match p {
            "home" => Ok(Scope::Page(Page::Home)),
            "library" => Ok(Scope::Page(Page::Library)),
            "detail" => Ok(Scope::Page(Page::Detail)),
            "search" => Ok(Scope::Page(Page::Search)),
            _ => Err(format!(
                "unknown page `{class}`; pages are home, library, detail, search"
            )),
        });
    }
    let s = name.strip_prefix("section-")?;
    Some(match s {
        "resume" => Ok(Scope::Section(Section::Resume)),
        "next-up" => Ok(Scope::Section(Section::NextUp)),
        "latest" => Ok(Scope::Section(Section::Latest)),
        _ => Err(format!(
            "unknown section `{class}`; sections are resume, next-up, latest"
        )),
    })
}

fn part_of(sel: &str) -> Option<Result<(PartId, bool), String>> {
    let (name, hover) = match sel.strip_suffix(":hover") {
        Some(n) => (n, true),
        None => (sel, false),
    };
    let key = name.strip_prefix('.').or(match name {
        "button" | "header" => Some(name),
        _ => None,
    })?;
    let (_, id) = PARTS.iter().find(|(n, _)| *n == key)?;
    if hover && !matches!(id, PartId::Card | PartId::Button) {
        return Some(Err(
            ":hover is only supported on .card and .button (transform: scale())".into(),
        ));
    }
    Some(Ok((*id, hover)))
}

/// `None`: not a layout selector (the colour vocabulary may still know it).
pub(super) fn parse_selector(sel: &str) -> Option<Result<Target, String>> {
    let toks: Vec<&str> = sel.split_whitespace().collect();
    match toks.as_slice() {
        [one] => {
            if let Some(sc) = scope_of(one) {
                return Some(sc.and_then(|scope| match scope {
                    Scope::Section(_) => Ok(Target {
                        scope,
                        part: PartId::Row,
                        hover: false,
                    }),
                    _ => Err(format!(
                        "`{one}` is a scope: write `{one} .card` to say which part"
                    )),
                }));
            }
            Some(part_of(one)?.map(|(part, hover)| Target {
                scope: Scope::Base,
                part,
                hover,
            }))
        }
        [scope, part] => {
            let sc = scope_of(scope)?;
            Some(sc.and_then(|scope| match part_of(part) {
                Some(r) => r.map(|(part, hover)| Target { scope, part, hover }),
                None => Err(format!("unknown part `{part}`; see the part reference")),
            }))
        }
        _ => None,
    }
}

// ---- values ----------------------------------------------------------------------------------

fn first<'a>(v: &'a str, notes: &mut Notes) -> &'a str {
    let parts = value::split_top(v, None);
    if parts.len() > 1 {
        notes.push((
            WarningKind::Approximated,
            format!("only the first of {} values is used", parts.len()),
        ));
    }
    parts.first().copied().unwrap_or("")
}

fn px(v: &str, lo: f32, hi: f32, what: &str, notes: &mut Notes) -> Result<f32, String> {
    let f = value::length(first(v, notes), BASE_PX)?;
    Ok(clamp(f, lo, hi, what, notes))
}

fn signed_px(v: &str) -> Result<f32, String> {
    match v.strip_prefix('-') {
        Some(r) => value::length(r, BASE_PX).map(|x| -x),
        None => value::length(v, BASE_PX),
    }
}

fn time_ms(tok: &str) -> Option<f32> {
    let t = tok.trim().to_ascii_lowercase();
    if t == "0" {
        return Some(0.0);
    }
    let (n, k) = match t.strip_suffix("ms") {
        Some(n) => (n, 1.0),
        None => (t.strip_suffix('s')?, 1000.0),
    };
    n.parse::<f32>()
        .ok()
        .filter(|x| x.is_finite())
        .map(|x| x * k)
}

fn duration(v: &str, shorthand: bool, notes: &mut Notes) -> Result<u32, String> {
    let seg = value::split_top(v, Some(','));
    if seg.len() > 1 {
        notes.push((
            WarningKind::Approximated,
            "only the first transition is used".into(),
        ));
    }
    let toks = value::split_top(seg.first().copied().unwrap_or(""), None);
    let ms = toks
        .iter()
        .find_map(|t| time_ms(t))
        .ok_or("no duration such as `200ms` or `0.2s`")?;
    if shorthand {
        notes.push((
            WarningKind::Approximated,
            "only the duration of `transition` is read".into(),
        ));
    }
    Ok(clamp(ms, 0.0, 1000.0, "duration (ms)", notes).round() as u32)
}

fn int(v: &str, lo: f32, hi: f32, what: &str, notes: &mut Notes) -> Result<f32, String> {
    let n = value::number(v)?;
    Ok(clamp(n.round(), lo, hi, what, notes))
}

fn aspect(v: &str, notes: &mut Notes) -> Result<Aspect, String> {
    let n = |s: &str| value::number(s.trim());
    let r = match v.split_once('/') {
        Some((a, b)) => n(a)? / n(b)?,
        None => n(v)?,
    };
    if !r.is_finite() || r <= 0.0 {
        return Err(format!("`{v}` is not a usable ratio"));
    }
    let (kind, want) = [
        (Aspect::Poster, 2.0 / 3.0),
        (Aspect::Thumb, 16.0 / 9.0),
        (Aspect::Square, 1.0),
    ]
    .into_iter()
    .min_by(|a, b| (r / a.1).ln().abs().total_cmp(&(r / b.1).ln().abs()))
    .expect("three candidates");
    if (r / want).ln().abs() > 0.05 {
        notes.push((
            WarningKind::Approximated,
            format!("ratio {r:.2} is not 2/3, 16/9 or 1/1; the nearest ({kind:?}) is used"),
        ));
    }
    Ok(kind)
}

fn shadow(v: &str, notes: &mut Notes) -> Result<Shadow, String> {
    if v.eq_ignore_ascii_case("none") {
        return Ok(Shadow {
            x: 0.0,
            y: 0.0,
            blur: 0.0,
            color: Color::rgba(0, 0, 0, 0),
        });
    }
    let (mut lens, mut color) = (vec![], None);
    for tok in value::split_top(v, None) {
        if tok.eq_ignore_ascii_case("inset") {
            return Err("inset shadows are not supported".into());
        }
        match signed_px(tok) {
            Ok(l) => lens.push(l),
            Err(_) => color = Some(value::color(tok)?),
        }
    }
    if !(2..=4).contains(&lens.len()) {
        return Err("a shadow needs x and y offsets, optionally blur, spread and colour".into());
    }
    if lens.len() == 4 {
        notes.push((WarningKind::Approximated, "shadow spread is ignored".into()));
    }
    Ok(Shadow {
        x: clamp(lens[0], -32.0, 32.0, "shadow offset", notes),
        y: clamp(lens[1], -32.0, 32.0, "shadow offset", notes),
        blur: clamp(
            lens.get(2).copied().unwrap_or(0.0),
            0.0,
            64.0,
            "shadow blur",
            notes,
        ),
        color: color.unwrap_or(Color::rgba(0, 0, 0, 102)),
    })
}

fn hover_scale(v: &str, notes: &mut Notes) -> Result<f32, String> {
    if v.eq_ignore_ascii_case("none") {
        return Ok(1.0);
    }
    let lower = v.to_ascii_lowercase();
    let args = lower
        .strip_prefix("scale(")
        .and_then(|r| r.strip_suffix(')'))
        .ok_or("only `scale()` is supported under :hover")?;
    let parts = value::split_top(args, Some(','));
    if parts.len() > 1 {
        notes.push((
            WarningKind::Approximated,
            "scale() takes one factor; the first is used".into(),
        ));
    }
    let f = value::number(parts.first().ok_or("empty scale()")?)?;
    Ok(clamp(f, 1.0, 1.3, "scale", notes))
}

fn one_of<T: Copy>(v: &str, opts: &[(&str, T)]) -> Result<T, String> {
    let l = v.trim().to_ascii_lowercase();
    opts.iter()
        .find(|(k, _)| *k == l)
        .map(|(_, t)| *t)
        .ok_or_else(|| {
            let names: Vec<&str> = opts.iter().map(|(k, _)| *k).collect();
            format!("`{v}` is not one of {}", names.join(", "))
        })
}

/// `display: none` hides; any other keyword shows (so a later layer can undo a hide).
fn display(v: &str) -> bool {
    v.eq_ignore_ascii_case("none")
}

fn fill(v: &str, notes: &mut Notes) -> Result<Fill, String> {
    let (f, n) = value::fill(v)?;
    notes.extend(n.into_iter().map(|m| (WarningKind::Approximated, m)));
    Ok(f)
}

fn text(t: &mut Text, name: &str, v: &str, notes: &mut Notes) -> Result<bool, String> {
    match name {
        "display" => t.hidden = Some(display(v)),
        "font-size" => t.font_size = Some(px(v, 8.0, 96.0, "font size", notes)?),
        "font-weight" => {
            let w = match v.to_ascii_lowercase().as_str() {
                "normal" => 400.0,
                "bold" | "bolder" => 700.0,
                "lighter" => 300.0,
                _ => value::number(v)?,
            };
            t.font_weight = Some(clamp(w, 100.0, 900.0, "font weight", notes).round() as u16);
        }
        "text-transform" => {
            t.text_transform = Some(one_of(
                v,
                &[
                    ("none", TextTransform::None),
                    ("uppercase", TextTransform::Uppercase),
                    ("lowercase", TextTransform::Lowercase),
                    ("capitalize", TextTransform::Capitalize),
                ],
            )?)
        }
        "letter-spacing" => {
            let s = if v.eq_ignore_ascii_case("normal") {
                0.0
            } else {
                signed_px(first(v, notes))?
            };
            t.letter_spacing = Some(clamp(s, -2.0, 10.0, "letter spacing", notes));
        }
        "line-height" => {
            t.line_height = Some(clamp(value::number(v)?, 0.8, 2.5, "line height", notes))
        }
        "color" => t.color = Some(value::color(v)?),
        _ => return Ok(false),
    }
    Ok(true)
}

fn episode_format(v: &str) -> Result<EpisodeFormat, String> {
    one_of(
        v,
        &[
            ("short", EpisodeFormat::Short),
            ("long", EpisodeFormat::Long),
        ],
    )
}

/// Typography plus `text-align` (card captions only).
fn card_text(t: &mut Text, name: &str, v: &str, notes: &mut Notes) -> Result<bool, String> {
    if name == "text-align" {
        t.align = Some(one_of(
            v,
            &[("left", TextAlign::Left), ("center", TextAlign::Center)],
        )?);
        return Ok(true);
    }
    text(t, name, v, notes)
}

fn set_part(
    p: &mut Parts,
    t: &Target,
    name: &str,
    v: &str,
    notes: &mut Notes,
) -> Result<bool, String> {
    let name = name.to_ascii_lowercase();
    let name = name.as_str();
    // Under :hover only the scale is read, on the two parts that allow it.
    if t.hover {
        if name != "transform" {
            return Ok(false);
        }
        let s = Some(hover_scale(v, notes)?);
        match t.part {
            PartId::Card => p.card.hover_scale = s,
            _ => p.button.hover_scale = s,
        }
        return Ok(true);
    }
    let is_ms = matches!(name, "transition" | "transition-duration");
    let ms = |notes: &mut Notes| duration(v, name == "transition", notes).map(Some);
    match t.part {
        PartId::Card => match name {
            "display" => p.card.hidden = Some(display(v)),
            "width" | "--card-size" => {
                p.card.width = Some(px(v, 80.0, 640.0, "card width", notes)?)
            }
            "aspect-ratio" => p.card.aspect = Some(aspect(v, notes)?),
            "--episode-format" => p.card.episode_format = Some(episode_format(v)?),
            "--title-position" => {
                p.card.title_position = Some(one_of(
                    v,
                    &[
                        ("below", TitlePosition::Below),
                        ("overlay", TitlePosition::Overlay),
                        ("none", TitlePosition::None),
                    ],
                )?)
            }
            _ if is_ms => p.card.transition_ms = ms(notes)?,
            "transform" => return Err("`transform` is only read under :hover".into()),
            "box-shadow" => p.card_image.shadow = Some(shadow(v, notes)?),
            // At base scope the colour engine already sets the global card radius.
            "border-radius" if t.scope != Scope::Base => {
                p.card_image.radius = Some(px(v, 0.0, 48.0, "radius", notes)?)
            }
            _ => return Ok(false),
        },
        PartId::CardImage => match name {
            "box-shadow" => p.card_image.shadow = Some(shadow(v, notes)?),
            "border-radius" => p.card_image.radius = Some(px(v, 0.0, 48.0, "radius", notes)?),
            _ => return Ok(false),
        },
        PartId::CardTitle => return card_text(&mut p.card_title, name, v, notes),
        PartId::CardMeta => return card_text(&mut p.card_meta, name, v, notes),
        PartId::CardSubtitle => match name {
            "--episode-format" => p.card.episode_format = Some(episode_format(v)?),
            _ => return card_text(&mut p.card_subtitle, name, v, notes),
        },
        PartId::LibraryTile => match name {
            "--tile-label" => {
                p.card.tile_label = Some(one_of(
                    v,
                    &[
                        ("overlay-center", TileLabel::OverlayCenter),
                        ("below", TileLabel::Below),
                    ],
                )?)
            }
            _ => return text(&mut p.library_tile, name, v, notes),
        },
        PartId::CardBadge => match name {
            "display" => p.card_badge.hidden = Some(display(v)),
            "--badge-position" => match v.to_ascii_lowercase().as_str() {
                "none" => p.card_badge.hidden = Some(true),
                _ => {
                    p.card_badge.position = Some(one_of(
                        v,
                        &[
                            ("top-right", BadgePos::TopRight),
                            ("top-left", BadgePos::TopLeft),
                            ("bottom-right", BadgePos::BottomRight),
                        ],
                    )?);
                    p.card_badge.hidden = Some(false);
                }
            },
            "background" | "background-color" => p.card_badge.background = Some(value::color(v)?),
            "color" => p.card_badge.color = Some(value::color(v)?),
            "font-size" => p.card_badge.font_size = Some(px(v, 8.0, 32.0, "font size", notes)?),
            "border-radius" => p.card_badge.radius = Some(px(v, 0.0, 24.0, "radius", notes)?),
            _ => return Ok(false),
        },
        PartId::CardProgress => match name {
            "display" => p.card_progress.hidden = Some(display(v)),
            "--progress-position" => {
                p.card_progress.position = Some(one_of(
                    v,
                    &[
                        ("inside-bottom", ProgressPos::InsideBottom),
                        ("below", ProgressPos::Below),
                    ],
                )?)
            }
            "height" => p.card_progress.height = Some(px(v, 1.0, 16.0, "bar height", notes)?),
            "background" | "background-color" => {
                p.card_progress.background = Some(value::color(v)?)
            }
            "color" => p.card_progress.color = Some(value::color(v)?),
            _ => return Ok(false),
        },
        PartId::RowArrows => match name {
            "display" => p.row_arrows.hidden = Some(display(v)),
            "font-size" | "width" | "height" => {
                p.row_arrows.size = Some(px(v, 12.0, 48.0, "arrow size", notes)?)
            }
            "color" => p.row_arrows.color = Some(value::color(v)?),
            _ => return Ok(false),
        },
        PartId::HeaderBrand => match name {
            "display" => p.header_brand.hidden = Some(display(v)),
            "content" => {
                p.header_brand.content = Some(one_of(
                    v,
                    &[
                        ("server", BrandContent::Server),
                        ("user", BrandContent::User),
                        ("app", BrandContent::App),
                    ],
                )?);
                p.header_brand.hidden = Some(false);
            }
            "font-size" => p.header_brand.font_size = Some(px(v, 8.0, 48.0, "font size", notes)?),
            "font-weight" => {
                let w = match v.to_ascii_lowercase().as_str() {
                    "normal" => 400.0,
                    "bold" => 700.0,
                    _ => value::number(v)?,
                };
                p.header_brand.font_weight =
                    Some(clamp(w, 100.0, 900.0, "font weight", notes).round() as u16);
            }
            "color" => p.header_brand.color = Some(value::color(v)?),
            _ => return Ok(false),
        },
        PartId::HeaderIcons => {
            if name != "display" {
                return Ok(false);
            }
            p.header_icons.hidden = Some(display(v));
        }
        PartId::RowTitle => match name {
            "--row-details" => p.row.details = Some(one_of(v, &[("on", true), ("off", false)])?),
            _ => return text(&mut p.row_title, name, v, notes),
        },
        PartId::HeroTitle => return text(&mut p.hero_title, name, v, notes),
        PartId::HeroMeta => return text(&mut p.hero_meta, name, v, notes),
        PartId::HeaderLink => return text(&mut p.header_link, name, v, notes),
        PartId::DetailTitle => return text(&mut p.detail_title, name, v, notes),
        PartId::DetailText => return text(&mut p.detail_text, name, v, notes),
        PartId::Row => match name {
            "display" => {
                let (hide, lay) = match v.to_ascii_lowercase().as_str() {
                    "none" => (true, None),
                    "carousel" | "flex" => (false, Some(RowLayout::Carousel)),
                    "grid" => (false, Some(RowLayout::Grid)),
                    _ => (false, None),
                };
                p.row.hidden = Some(hide);
                if lay.is_some() {
                    p.row.layout = lay;
                }
            }
            "--row-layout" => {
                p.row.layout = Some(one_of(
                    v,
                    &[("carousel", RowLayout::Carousel), ("grid", RowLayout::Grid)],
                )?)
            }
            "gap" => p.row.gap = Some(px(v, 0.0, 64.0, "gap", notes)?),
            "padding" => p.row.padding = Some(px(v, 0.0, 96.0, "padding", notes)?),
            "--row-columns" => p.row.columns = Some(int(v, 1.0, 12.0, "columns", notes)? as u8),
            "order" => match t.scope {
                Scope::Section(_) => {
                    p.row.order = Some(int(v, -100.0, 100.0, "order", notes)? as i32)
                }
                _ => return Err("`order` is only read on .section-* rules".into()),
            },
            _ => return Ok(false),
        },
        PartId::Hero => match name {
            "display" => p.hero.hidden = Some(display(v)),
            "--hero-style" if v.eq_ignore_ascii_case("none") => p.hero.hidden = Some(true),
            "--hero-style" => {
                p.hero.style = Some(one_of(
                    v,
                    &[
                        ("billboard", HeroStyle::Billboard),
                        ("banner", HeroStyle::Banner),
                    ],
                )?);
                p.hero.hidden = Some(false);
            }
            "height" => p.hero.height = Some(px(v, 120.0, 1200.0, "hero height", notes)?),
            "background" | "background-color" => p.hero.overlay = Some(fill(v, notes)?),
            _ => return Ok(false),
        },
        PartId::Header => match name {
            "position" => {
                p.header.position = Some(one_of(
                    v,
                    &[
                        ("fixed", HeaderPosition::Fixed),
                        ("absolute", HeaderPosition::Fixed),
                        ("static", HeaderPosition::Static),
                        ("relative", HeaderPosition::Static),
                        ("sticky", HeaderPosition::Static),
                    ],
                )?)
            }
            "height" => p.header.height = Some(px(v, 32.0, 120.0, "header height", notes)?),
            "--header-align" => {
                p.header.align = Some(one_of(
                    v,
                    &[("center", HeaderAlign::Center), ("left", HeaderAlign::Left)],
                )?)
            }
            "--header-link-icons" => {
                p.header.link_icons = Some(one_of(v, &[("on", true), ("off", false)])?)
            }
            "--header-items" => {
                let mut shown = [false; 4];
                let words = v
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|w| !w.is_empty());
                for w in words {
                    match w.to_ascii_lowercase().as_str() {
                        "nav" => shown[0] = true,
                        "search" => shown[1] = true,
                        "history" => shown[2] = true,
                        "account" => shown[3] = true,
                        "none" => {}
                        o => notes.push((
                            WarningKind::UnsupportedValue,
                            format!(
                                "header item `{o}` is not one of nav, search, history, account"
                            ),
                        )),
                    }
                }
                p.header_nav.hidden = Some(!shown[0]);
                p.header_search.hidden = Some(!shown[1]);
                p.header_history.hidden = Some(!shown[2]);
                p.header_account.hidden = Some(!shown[3]);
            }
            _ => return Ok(false),
        },
        PartId::HeaderNav
        | PartId::HeaderSearch
        | PartId::HeaderHistory
        | PartId::HeaderAccount => {
            if name != "display" {
                return Ok(false);
            }
            let vis = match t.part {
                PartId::HeaderNav => &mut p.header_nav,
                PartId::HeaderSearch => &mut p.header_search,
                PartId::HeaderHistory => &mut p.header_history,
                _ => &mut p.header_account,
            };
            vis.hidden = Some(display(v));
        }
        PartId::Button => match name {
            _ if is_ms => p.button.transition_ms = ms(notes)?,
            "transform" => return Err("`transform` is only read under :hover".into()),
            _ => return Ok(false),
        },
        PartId::PlayerControls => match name {
            "background" | "background-color" => {
                p.player_controls.background = Some(fill(v, notes)?)
            }
            "display" if display(v) => return Err("the player controls cannot be hidden".into()),
            "display" => {}
            _ => return Ok(false),
        },
    }
    Ok(true)
}
