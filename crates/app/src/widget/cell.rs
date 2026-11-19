//! Poster cells and image tiles (ports of the GPUI `poster_cell` / `tile`). A cell is a fixed-size
//! element (`cell_h`), so virtualised grids can compute row positions without measuring.
use super::press::Press;
use super::text::{initial, mono};
use crate::style::{border, glow_shadow};
use crate::theme as t;
use iced::widget::button::{Status, Style};
use iced::widget::image::Handle;
use iced::widget::{Stack, column, container, image, mouse_area, row, space, text};
use iced::{
    Alignment, Background, Color, ContentFit, Element, Gradient, Length, Shadow, Vector, gradient,
};
use nocturne_core::customize::{self as cz, TitlePosition};

/// What an image slot can show.
#[derive(Clone)]
pub enum Pic {
    Ready(Handle),
    /// The real image is still loading: its blurhash, upscaled.
    Blur(Handle),
    Loading,
    Missing,
}

/// Caption block under the image: gap + title line + mono line.
pub const CAPTION_H: f32 = t::S3 + 20.0 + 2.0 + 16.0;

/// Line height of a caption text of `size` (15 px -> 20, 12 px -> 16, the built-in lines).
fn line(size: f32) -> f32 {
    (size * 4.0 / 3.0).round()
}

/// A card's resolved look for one view: the `.card`, `.card-image`, `.card-title`, `.card-meta`
/// and `.row gap` settings over the built-in cell. The default is exactly the built-in cell.
#[derive(Clone, Debug)]
pub struct Look {
    /// Padding on each side of the image; two of them make the gap between cards.
    pub pad: f32,
    pub title: TitlePosition,
    pub aspect: Option<cz::Aspect>,
    pub width: Option<f32>,
    pub radius: Option<f32>,
    pub shadow: Option<cz::Shadow>,
    pub hover: Option<f32>,
    pub ms: Option<u32>,
    pub(crate) title_t: t::Typo,
    pub(crate) meta_t: t::Typo,
    pub(crate) meta_set: bool,
    /// `.card-subtitle` over `.card-meta`: the line under the title of an episode.
    pub(crate) sub_t: t::Typo,
    pub(crate) sub_set: bool,
    /// `text-align: center` on the title / the details lines.
    pub center_title: bool,
    pub center_meta: bool,
    pub episode_format: Option<cz::EpisodeFormat>,
    /// The library name is drawn large and centred over the image (My media tiles).
    pub tile_center: bool,
    /// No hairlines between cards and no index tag (`.row --row-frame: none`).
    pub flat: bool,
    pub(crate) badge: cz::Badge,
    pub(crate) progress: cz::Progress,
}

fn ic(c: cz::Color) -> Color {
    let [r, g, b, a] = c.to_f32();
    Color { r, g, b, a }
}

impl Default for Look {
    fn default() -> Self {
        Self::of(&cz::Parts::default())
    }
}

impl Look {
    pub fn of(p: &cz::Parts) -> Self {
        Self {
            pad: p.row.gap.map_or(t::S3, |g| g / 2.0),
            title: p.card.title_position.unwrap_or(TitlePosition::Below),
            aspect: p.card.aspect,
            width: p.card.width,
            radius: p.card_image.radius,
            shadow: p.card_image.shadow,
            hover: p.card.hover_scale,
            ms: p.card.transition_ms,
            title_t: t::typo(&p.card_title, t::BODY, iced::font::Weight::Semibold),
            meta_t: t::typo(&p.card_meta, t::MICRO, iced::font::Weight::Normal),
            meta_set: p.card_meta != cz::Text::default(),
            sub_t: t::typo(
                &p.card_meta.merged(&p.card_subtitle),
                t::MICRO,
                iced::font::Weight::Normal,
            ),
            sub_set: p.card_meta.merged(&p.card_subtitle) != cz::Text::default(),
            center_title: p.card_title.align == Some(cz::TextAlign::Center),
            center_meta: p.card_meta.align == Some(cz::TextAlign::Center),
            episode_format: p.card.episode_format,
            tile_center: false,
            flat: p.row.framed == Some(false),
            badge: p.card_badge.clone(),
            progress: p.card_progress.clone(),
        }
    }

    /// The look of the My media tiles: `.library-tile` over the card.
    pub fn tiles(p: &cz::Parts) -> Self {
        let mut l = Self::of(p);
        if p.card.tile_label == Some(cz::TileLabel::OverlayCenter) {
            l.tile_center = true;
            l.title_t = t::typo(&p.library_tile, 26.0, iced::font::Weight::Semibold);
        } else if p.library_tile != cz::Text::default() {
            l.title_t = t::typo(&p.library_tile, t::BODY, iced::font::Weight::Semibold);
        }
        l
    }

    /// Details line typography (the subtitle's for an episode line).
    pub(crate) fn meta_typo(&self, subtitle: bool) -> (&t::Typo, bool) {
        if subtitle {
            (&self.sub_t, self.sub_set || self.meta_set)
        } else {
            (&self.meta_t, self.meta_set)
        }
    }

    pub(crate) fn badge_shown(&self) -> bool {
        self.badge.hidden != Some(true)
    }

    /// Height of a bar under the image (`--progress-position: below`), reserved on every card.
    pub(crate) fn bar_h(&self) -> f32 {
        match (self.progress.position, self.progress.hidden) {
            (Some(cz::ProgressPos::Below), h) if h != Some(true) => {
                self.progress.height.unwrap_or(3.0)
            }
            _ => 0.0,
        }
    }

    /// The caption (title and details) is drawn under the image.
    pub(crate) fn caption(&self) -> bool {
        self.title == TitlePosition::Below && !self.tile_center
    }

    pub(crate) fn title_h(&self) -> f32 {
        if self.title_t.hidden {
            0.0
        } else {
            line(self.title_t.size())
        }
    }

    pub(crate) fn meta_h(&self) -> f32 {
        if self.meta_t.hidden {
            0.0
        } else if self.meta_set || self.sub_set {
            line(self.meta_t.size().max(self.sub_t.size()))
        } else {
            16.0
        }
    }

    /// Height of the block under the image, gap included (the built-in `CAPTION_H`).
    pub fn caption_h(&self) -> f32 {
        if !self.caption() {
            return 0.0;
        }
        let lines: Vec<f32> = [self.title_h(), self.meta_h()]
            .into_iter()
            .filter(|h| *h > 0.0)
            .collect();
        match lines.len() {
            0 => 0.0,
            n => t::S3 + lines.iter().sum::<f32>() + 2.0 * (n - 1) as f32,
        }
    }

    /// Total height of a cell whose image is `h` tall (padding, caption and the bottom hairline).
    pub fn cell_h(&self, h: f32) -> f32 {
        h + 2.0 * self.pad + self.caption_h() + self.bar_h() + 1.0
    }

    /// Image height for an image `w` wide: the card's aspect, else `default` (height / width).
    pub fn ratio(&self, default: f32) -> f32 {
        match self.aspect {
            Some(cz::Aspect::Poster) => 1.5,
            Some(cz::Aspect::Thumb) => 9.0 / 16.0,
            Some(cz::Aspect::Square) => 1.0,
            None => default,
        }
    }

    /// The artwork that fits the card's aspect, else `default`.
    pub fn art(&self, default: nocturne_core::source::Art) -> nocturne_core::source::Art {
        use nocturne_core::source::Art;
        match self.aspect {
            Some(cz::Aspect::Thumb) => Art::Landscape,
            Some(_) => Art::Poster,
            None => default,
        }
    }
}

/// Total height of a cell whose image is `h` tall (padding, caption and the bottom hairline).
pub fn cell_h(h: f32) -> f32 {
    Look::default().cell_h(h)
}

fn base_fill(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Gradient(Gradient::Linear(
            gradient::Linear::new(std::f32::consts::PI * 0.9)
                .add_stop(0.0, t::raised())
                .add_stop(1.0, t::surface()),
        ))),
        border: border(t::NONE, 0.0, radius),
        ..Default::default()
    }
}

/// Image slot: loaded image, blur stand-in, quiet gradient while loading, or the title's initial.
/// Never a flat empty box.
pub fn tile<'a, M: 'a>(pic: &Pic, title: &str, w: Length, h: Length) -> Element<'a, M> {
    tile_r(pic, title, w, h, t::r_card())
}

/// [`tile`] with its own corner radius.
pub fn tile_r<'a, M: 'a>(
    pic: &Pic,
    title: &str,
    w: Length,
    h: Length,
    radius: f32,
) -> Element<'a, M> {
    let inner: Element<'a, M> = match pic {
        Pic::Ready(hd) | Pic::Blur(hd) => image(hd.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .border_radius(radius)
            .into(),
        Pic::Loading => space().into(),
        Pic::Missing => container(
            text(initial(title))
                .size(48)
                .font(t::bold())
                .color(t::INITIAL_TINT),
        )
        .center(Length::Fill)
        .into(),
    };
    container(inner)
        .width(w)
        .height(h)
        .clip(true)
        .style(base_fill(radius))
        .into()
}

pub struct Cell<M> {
    pub pic: Pic,
    pub title: String,
    pub meta: String,
    pub index: usize,
    pub w: f32,
    pub h: f32,
    pub focused: bool,
    pub progress: Option<f32>,
    /// Unwatched count (bottom-right of the image unless `.card-badge` moves it).
    pub badge: Option<String>,
    /// The details line is an episode line (`.card-subtitle` styles it).
    pub subtitle: bool,
    /// Where the item lives, when several sources exist ("LOCAL", "2 SOURCES"; bottom-left).
    pub mark: Option<String>,
    /// `(favourite, played, writes allowed)`; `None` hides the chips.
    pub chips: Option<(bool, bool, bool)>,
    pub on_open: M,
    pub on_hover: M,
    pub on_fav: M,
    pub on_played: M,
    pub look: Look,
}

fn chip_button<'a, M: Clone + 'a>(
    glyph: &'static str,
    on: bool,
    live: bool,
    m: M,
) -> Element<'a, M> {
    Press::new(container(text(glyph).size(13)).center(Length::Fill))
        .width(24)
        .height(24)
        .on_press(m)
        .style(move |s| {
            let ink = if on {
                t::accent_text()
            } else {
                t::text_secondary()
            };
            Style {
                background: Some(Color::from_rgba8(0x07, 0x08, 0x0c, 0.8).into()),
                text_color: Color {
                    a: if live { 1.0 } else { 0.4 },
                    ..ink
                },
                border: border(
                    if on || matches!(s, Status::Hovered | Status::Pressed) {
                        t::accent()
                    } else {
                        t::edge()
                    },
                    1.0,
                    12.0,
                ),
                ..Default::default()
            }
        })
        .into()
}

/// Centre a caption text across the cell when the stylesheet says `text-align: center`.
fn align<'a>(x: text::Text<'a>, center: bool) -> text::Text<'a> {
    if center {
        x.width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
    } else {
        x
    }
}

impl Look {
    /// The unplayed count in its corner (default: bottom-right, accent, 4 px corners).
    fn badge_layer<'a, M: 'a>(
        &self,
        n: String,
        corner: impl Fn(Element<'a, M>, Alignment, Alignment, f32) -> Element<'a, M>,
    ) -> Element<'a, M> {
        let b = &self.badge;
        let bg = b.background.map_or_else(t::accent, ic);
        let ink = b.color.map_or_else(t::on_accent, ic);
        let radius = b.radius.unwrap_or(4.0);
        let n_len = n.chars().count();
        let label: Element<'a, M> = match b.font_size {
            Some(sz) => text(n).size(sz).font(t::semibold()).color(ink).into(),
            None => mono(n).color(ink).into(),
        };
        let (ax, ay) = match b.position {
            Some(cz::BadgePos::TopRight) => (Alignment::End, Alignment::Start),
            Some(cz::BadgePos::TopLeft) => (Alignment::Start, Alignment::Start),
            _ => (Alignment::End, Alignment::End),
        };
        // With a set size the badge is a round chip: as wide as it is tall for one digit.
        let one = n_len == 1;
        let mut bx = container(label).padding([0.0, t::S2]);
        if let Some(sz) = b.font_size {
            let hgt = sz + 10.0;
            bx = bx.height(hgt).center_y(hgt);
            if one {
                bx = bx.width(hgt).center_x(hgt).padding(0);
            }
        }
        let bx = bx.style(move |_| container::Style {
            background: Some(bg.into()),
            border: border(t::NONE, 0.0, radius),
            ..Default::default()
        });
        corner(bx.into(), ax, ay, t::S2)
    }

    /// A progress bar with the stylesheet's track and fill.
    fn bar<'a, M: 'a>(&self, frac: f32, h: f32) -> Element<'a, M> {
        let (track, fill) = (
            self.progress.background.map_or(t::PROGRESS_TRACK, ic),
            self.progress.color.map_or_else(t::accent, ic),
        );
        iced::widget::progress_bar(0.0..=1.0, frac.clamp(0.0, 1.0))
            .girth(h)
            .style(move |_| iced::widget::progress_bar::Style {
                background: track.into(),
                bar: fill.into(),
                border: iced::Border::default(),
            })
            .into()
    }
}

pub fn poster_cell<'a, M: Clone + 'a>(c: Cell<M>) -> Element<'a, M> {
    let (w, h) = (c.w, c.h);
    let (meta_t, meta_set) = c.look.meta_typo(c.subtitle);
    let (meta_t, c_meta) = (meta_t.clone(), c.look.center_meta);
    let look = c.look.clone();
    let pad = look.pad;
    let radius = look.radius.unwrap_or_else(t::r_card);
    let corner = |x: Element<'a, M>, ax: Alignment, ay: Alignment, pad: f32| -> Element<'a, M> {
        container(x)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(ax)
            .align_y(ay)
            .padding(pad)
            .into()
    };
    let mut layers: Vec<Element<'a, M>> =
        vec![tile_r(&c.pic, &c.title, Length::Fill, Length::Fill, radius)];
    if !c.focused {
        layers.push(
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(t::POSTER_DIM.into()),
                    border: border(t::NONE, 0.0, radius),
                    ..Default::default()
                })
                .into(),
        );
    }
    if look.tile_center && !look.title_t.hidden {
        layers.push(
            container(
                look.title_t
                    .text(&c.title, Color::WHITE)
                    .wrapping(text::Wrapping::None)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(t::S3)
            .center_y(Length::Fill)
            .clip(true)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
                ..Default::default()
            })
            .into(),
        );
    }
    if look.title == TitlePosition::Overlay && !look.tile_center {
        let clear = Color {
            a: 0.0,
            ..t::frame()
        };
        let solid = Color {
            a: 0.92,
            ..t::frame()
        };
        let mut col: iced::widget::Column<'a, M> = column![].spacing(2);
        if !look.title_t.hidden {
            col = col.push(align(
                look.title_t
                    .text(&c.title, t::text())
                    .wrapping(text::Wrapping::None),
                look.center_title,
            ));
        }
        if !meta_t.hidden {
            col = col.push(align(
                if meta_set {
                    meta_t
                        .text(&c.meta, t::text_tertiary())
                        .wrapping(text::Wrapping::None)
                } else {
                    mono(c.meta.clone()).wrapping(text::Wrapping::None)
                },
                c_meta,
            ));
        }
        layers.push(
            container(
                container(container(col).padding(t::S3).width(Length::Fill).clip(true))
                    .width(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(crate::style::gradient(
                            180.0,
                            &[(0.0, clear), (1.0, solid)],
                        )),
                        ..Default::default()
                    }),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Alignment::End)
            .into(),
        );
    }
    if !look.flat {
        layers.push(corner(
            mono(format!("0x{:02x}", c.index + 1))
                .color(t::INDEX_TAG)
                .into(),
            Alignment::Start,
            Alignment::Start,
            t::S2,
        ));
    }
    if let Some(mk) = c.mark {
        layers.push(corner(
            container(mono(mk).color(t::text()))
                .padding([1.0, t::S1 + 1.0])
                .style(|_| container::Style {
                    background: Some(Color::from_rgba8(0x07, 0x08, 0x0c, 0.8).into()),
                    border: border(t::hairline_strong(), 1.0, 4.0),
                    ..Default::default()
                })
                .into(),
            Alignment::Start,
            Alignment::End,
            t::S2 + if c.progress.is_some() { 3.0 } else { 0.0 },
        ));
    }
    if let Some(b) = c.badge.clone().filter(|_| look.badge_shown()) {
        layers.push(look.badge_layer(b, corner));
    }
    let shown_progress = c.progress.filter(|_| look.progress.hidden != Some(true));
    if let Some(p) = shown_progress
        && look.bar_h() == 0.0
    {
        layers.push(corner(
            look.bar(p, look.progress.height.unwrap_or(3.0)),
            Alignment::Start,
            Alignment::End,
            0.0,
        ));
    }
    if let Some((fav, played, live)) = c.chips
        && (c.focused || fav || played)
    {
        let chips = row![
            chip_button(
                if fav { "\u{2665}" } else { "\u{2661}" },
                fav,
                live,
                c.on_fav
            ),
            chip_button("\u{2713}", played, live, c.on_played),
        ]
        .spacing(t::S1);
        layers.push(corner(
            chips.into(),
            Alignment::End,
            Alignment::Start,
            t::S2,
        ));
    }
    let shadow = look.shadow;
    let image_box = container(Stack::with_children(layers).width(w).height(h)).style(move |_| {
        container::Style {
            border: border(t::NONE, 0.0, radius),
            shadow: shadow.map_or(Shadow::default(), |s| Shadow {
                color: {
                    let [r, g, b, a] = s.color.to_f32();
                    Color { r, g, b, a }
                },
                offset: Vector::new(s.x, s.y),
                blur_radius: s.blur,
            }),
            ..Default::default()
        }
    });
    let caption_h = look.caption_h();
    let mut body = column![image_box];
    if look.bar_h() > 0.0 {
        // Under the image: a band reserved on every card so the cell height stays fixed.
        body = body.push(match shown_progress {
            Some(p) => look.bar(p, look.bar_h()),
            None => space().height(look.bar_h()).into(),
        });
    }
    if caption_h > 0.0 {
        let mut cap: iced::widget::Column<'a, M> = column![]
            .spacing(2)
            .width(w)
            .height(caption_h - t::S3)
            .clip(true);
        if !look.title_t.hidden {
            cap = cap.push(align(
                look.title_t
                    .text(
                        &c.title,
                        if c.focused {
                            t::text()
                        } else {
                            t::text_secondary()
                        },
                    )
                    .wrapping(text::Wrapping::None),
                look.center_title,
            ));
        }
        if !meta_t.hidden {
            cap = cap.push(
                container(align(
                    if meta_set {
                        meta_t
                            .text(&c.meta, t::text_tertiary())
                            .wrapping(text::Wrapping::None)
                    } else {
                        mono(c.meta).wrapping(text::Wrapping::None)
                    },
                    c_meta,
                ))
                .height(look.meta_h())
                .clip(true),
            );
        }
        body = body.push(space().height(t::S3)).push(cap);
    }
    let mut layers: Vec<Element<'a, M>> = vec![
        container(body)
            .padding(pad)
            .width(w + 2.0 * pad)
            .height(look.cell_h(h) - 1.0)
            .into(),
    ];
    if c.focused {
        layers.push(
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(t::accent_tint().into()),
                    border: border(t::accent(), crate::couch::ring(), 0.0),
                    shadow: glow_shadow(if crate::couch::on() { 1.0 } else { 0.5 }),
                    ..Default::default()
                })
                .into(),
        );
    }
    let cell = Stack::with_children(layers)
        .width(w + 2.0 * pad)
        .height(look.cell_h(h) - 1.0);
    // Shared edges: a hairline right and below each cell.
    let flat = look.flat;
    let hair = move |w: Length, h: Length| -> Element<'a, M> {
        let c = container(space()).width(w).height(h);
        if flat {
            c.into()
        } else {
            c.style(crate::style::rule).into()
        }
    };
    let with_edges = row![
        column![cell, hair(Length::Fixed(w + 2.0 * pad), Length::Fixed(1.0))],
        hair(Length::Fixed(1.0), Length::Fixed(look.cell_h(h)))
    ];
    mouse_area(super::zoom::zoom(with_edges.into(), look.hover, look.ms))
        .on_enter(c.on_hover)
        .on_press(c.on_open)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(css: &str) -> Look {
        Look::of(&cz::parse(css).unwrap().layout.parts(None, None))
    }

    #[test]
    fn the_default_cell_is_the_built_in_one() {
        let l = Look::default();
        assert_eq!((l.pad, l.caption_h(), CAPTION_H), (t::S3, CAPTION_H, 50.0));
        assert_eq!(cell_h(100.0), 100.0 + 24.0 + 50.0 + 1.0);
    }

    #[test]
    fn the_new_parts_change_nothing_until_asked() {
        let l = Look::default();
        assert_eq!(
            (l.bar_h(), l.badge_shown(), l.tile_center),
            (0.0, true, false)
        );
        assert!(!l.center_title && !l.center_meta && l.episode_format.is_none());
        // Same cell height with every web-home part present but the bar inside the image.
        let classic = Look::of(
            &cz::parse(cz::PRESETS[2].1)
                .unwrap()
                .layout
                .parts(None, None),
        );
        assert_eq!(classic.bar_h(), 0.0);
    }

    #[test]
    fn bars_below_tiles_and_subtitles_resize_the_cell() {
        assert_eq!(
            look(".card-progress{--progress-position:below;height:5px}").bar_h(),
            5.0
        );
        assert_eq!(
            look(".card-progress{--progress-position:below;height:5px}").cell_h(90.0),
            Look::default().cell_h(90.0) + 5.0
        );
        let p =
            cz::parse(".card{--title-position:below}.library-tile{--tile-label:overlay-center}")
                .unwrap()
                .layout
                .parts(None, None);
        let tiles = Look::tiles(&p);
        assert!(tiles.tile_center);
        assert_eq!(tiles.caption_h(), 0.0);
        // A bigger subtitle makes the details line taller for every card of the strip.
        let l = look(".card-subtitle{font-size:24px}");
        assert_eq!(l.caption_h(), t::S3 + 20.0 + 2.0 + 32.0);
        assert!(!look(".card-badge{--badge-position:none}").badge_shown());
    }

    #[test]
    fn caption_height_follows_position_and_typography() {
        assert_eq!(look(".card{--title-position:overlay}").caption_h(), 0.0);
        assert_eq!(
            look(".card{--title-position:none}").cell_h(90.0),
            90.0 + 24.0 + 1.0
        );
        // A 24 px title (32 px line) and no details line.
        let l = look(".card-title{font-size:24px} .card-meta{display:none}");
        assert_eq!(l.caption_h(), t::S3 + 32.0);
        assert_eq!(look(".row{gap:8px}").pad, 4.0);
    }
}
