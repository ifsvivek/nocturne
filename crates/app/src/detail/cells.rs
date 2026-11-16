//! Cells of the detail page: picture tile, poster / still cell for the strips, episode card, list
//! row, small round buttons. Ports of the GPUI `ui::tile`, `poster_cell` and the episode card.
use super::Msg;
use crate::style::{self, border, glow_shadow};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::text::initial;
use crate::widget::{mono, progress_bar, tip};
use iced::widget::button::{Status, Style};
use iced::widget::image::Handle;
use iced::widget::text::{IntoFragment, Shaping};
use iced::widget::{Stack, column, container, image, mouse_area, row, space, text};
use iced::{Alignment, Background, Color, ContentFit, Element, Length, Shadow};

type El<'a> = Element<'a, Msg>;

/// Text that may use glyphs the main face lacks (check marks, hearts, arrows).
pub(super) fn g<'a>(s: impl IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(s).shaping(Shaping::Advanced)
}

fn flat(bg: Option<Color>, ink: Color, edge: Option<Color>, radius: f32) -> Style {
    Style {
        background: bg.map(Background::Color),
        text_color: ink,
        border: border(edge.unwrap_or(t::NONE), edge.is_some() as u8 as f32, radius),
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A picture filling its box: the image, else its blurhash, else the title's initial on the raised
/// surface.
pub(super) fn tile<'a>(
    img: Option<&Handle>,
    blur: Option<&Handle>,
    title: &str,
    radius: f32,
) -> El<'a> {
    let base = container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(t::RAISED.into()),
            border: border(t::NONE, 0.0, radius),
            ..Default::default()
        });
    let pic = |h: &Handle| -> El<'a> {
        image(h.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .border_radius(radius)
            .into()
    };
    let top: El<'a> = match (img, blur) {
        (Some(h), _) | (None, Some(h)) => pic(h),
        _ => container(
            g(initial(title))
                .font(t::MONO)
                .size(28)
                .color(t::INITIAL_TINT),
        )
        .center(Length::Fill)
        .into(),
    };
    Stack::with_children([base.into(), top])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn progress_over<'a>(p: f32) -> El<'a> {
    container(progress_bar(p, 3.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::End)
        .into()
}

pub(super) struct Cell<'a> {
    pub idx: usize,
    pub w: f32,
    pub h: f32,
    pub img: Option<&'a Handle>,
    pub blur: Option<&'a Handle>,
    pub title: &'a str,
    pub meta: String,
    pub focused: bool,
    pub progress: Option<f32>,
    pub badge: Option<String>,
    pub on_press: Msg,
    pub on_enter: Msg,
}

/// Poster or still with its title and caption (home rows, strips).
pub(super) fn poster_cell<'a>(c: Cell<'a>) -> El<'a> {
    let focused = c.focused;
    let mut layers: Vec<El<'a>> = vec![tile(c.img, c.blur, c.title, t::R_XS)];
    if !focused {
        layers.push(
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(style::fill(t::POSTER_DIM))
                .into(),
        );
    }
    layers.push(
        container(mono(format!("0x{:02x}", c.idx + 1)).color(t::INDEX_TAG))
            .padding(t::S2)
            .into(),
    );
    if let Some(b) = c.badge {
        layers.push(
            container(
                container(text(b).font(t::MONO).size(t::MICRO).color(t::ON_ACCENT))
                    .padding([0.0, t::S2])
                    .style(|_| container::Style {
                        background: Some(t::ACCENT.into()),
                        border: border(t::NONE, 0.0, 4.0),
                        ..Default::default()
                    }),
            )
            .padding(t::S2)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .into(),
        );
    }
    if let Some(p) = c.progress {
        layers.push(progress_over(p));
    }
    let art = Stack::with_children(layers).width(c.w).height(c.h);
    let body = column![
        art,
        container(
            column![
                g(c.title.to_string())
                    .size(t::BODY)
                    .font(t::SEMIBOLD)
                    .color(if focused { t::TEXT } else { t::TEXT_SECONDARY })
                    .wrapping(text::Wrapping::None),
                container(mono(c.meta).wrapping(text::Wrapping::None)).height(16),
            ]
            .spacing(2),
        )
        .width(c.w)
        .clip(true)
        .padding([t::S3, 0.0]),
    ];
    let cell = container(body)
        .padding(t::S3)
        .style(move |_| container::Style {
            background: focused.then_some(t::ACCENT_TINT.into()),
            border: border(if focused { t::ACCENT } else { t::NONE }, 1.0, 0.0),
            shadow: if focused {
                glow_shadow(0.5)
            } else {
                Shadow::default()
            },
            ..Default::default()
        });
    let pressable = Press::new(cell)
        .on_press(c.on_press)
        .style(|_| flat(None, t::TEXT, None, 0.0));
    row![
        mouse_area(pressable).on_enter(c.on_enter),
        container(space().width(1).height(Length::Fill)).style(style::rule)
    ]
    .into()
}

/// Small round icon button (details, watched, favourite on an episode card).
fn round<'a>(glyph: &'static str, active: bool, off: bool, hint: &'static str, m: Msg) -> El<'a> {
    let dim = if off { 0.4 } else { 1.0 };
    tip(
        Press::new(container(g(glyph).size(13)).center(Length::Fill))
            .width(26)
            .height(26)
            .on_press(m)
            .style(move |s| {
                let hot = matches!(s, Status::Hovered | Status::Pressed);
                let edge = if active || hot {
                    t::ACCENT
                } else {
                    t::HAIRLINE
                };
                let ink = if active {
                    t::ACCENT_TEXT
                } else {
                    t::TEXT_SECONDARY
                };
                let mut x = flat(None, ink, Some(edge), 13.0);
                x.text_color.a *= dim;
                x.border.color.a *= dim;
                x
            }),
        hint,
    )
}

/// 16:9 episode card: still with play overlay and progress, title, meta, overview, actions.
#[allow(clippy::too_many_arguments)]
pub(super) fn episode_card<'a>(
    e: &'a nocturne_core::source::Media,
    idx: usize,
    w: f32,
    on: bool,
    img: Option<&'a Handle>,
    blur: Option<&'a Handle>,
    writes: bool,
    now_min: u32,
) -> El<'a> {
    use nocturne_core::detail::ends_at;
    use nocturne_core::source::fmt_runtime;
    let h = (w * 9.0 / 16.0).round();
    let title = match e.number {
        Some(n) => format!("{n}. {}", e.title),
        None => e.title.clone(),
    };
    let mut meta = vec![];
    if let Some(r) = e.runtime_min {
        meta.push(fmt_runtime(r));
        let left = e
            .progress()
            .map_or(r, |p| ((1.0 - p) * r as f32).round() as u32);
        if !e.played {
            meta.push(format!("ends {}", ends_at(now_min, left)));
        }
    }
    if let Some(r) = e.rating {
        meta.insert(meta.len().min(1), format!("\u{2605} {r:.1}"));
    }
    let mut layers: Vec<El<'a>> = vec![
        tile(img, blur, &e.title, t::R_XS),
        container(
            container(g("\u{25B6}").size(16).color(t::ON_ACCENT))
                .width(44)
                .height(44)
                .center(44)
                .style(move |_| container::Style {
                    background: Some(
                        Color {
                            a: if on { 1.0 } else { 0.6 },
                            ..t::ACCENT
                        }
                        .into(),
                    ),
                    border: border(t::NONE, 0.0, 22.0),
                    ..Default::default()
                }),
        )
        .center(Length::Fill)
        .into(),
    ];
    if e.played {
        layers.push(
            container(
                container(g("\u{2713}").size(12).color(t::ON_ACCENT))
                    .width(22)
                    .height(22)
                    .center(22)
                    .style(|_| container::Style {
                        background: Some(t::ACCENT.into()),
                        border: border(t::NONE, 0.0, 11.0),
                        ..Default::default()
                    }),
            )
            .padding(t::S2)
            .width(Length::Fill)
            .align_x(Alignment::End)
            .into(),
        );
    }
    if let Some(p) = e.progress() {
        layers.push(progress_over(p));
    }
    let thumb = container(Stack::with_children(layers).width(w).height(h)).style(move |_| {
        container::Style {
            border: border(if on { t::ACCENT } else { t::NONE }, 1.0, t::R_XS),
            ..Default::default()
        }
    });
    let overview = e.overview.clone().unwrap_or_default();
    let text_col = column![
        container(
            g(title)
                .size(14)
                .font(t::SEMIBOLD)
                .color(t::TEXT)
                .wrapping(text::Wrapping::None)
        )
        .clip(true),
        container(mono(meta.join("   ")).wrapping(text::Wrapping::None)).clip(true),
        container(
            g(overview)
                .size(t::CAPTION + 1.0)
                .line_height(1.5)
                .color(t::TEXT_TERTIARY)
        )
        .height(3.0 * (t::CAPTION + 1.0) * 1.5)
        .clip(true)
        .padding([t::S1, 0.0]),
    ]
    .spacing(2)
    .width(Length::Fill);
    let actions = column![
        round("i", false, false, "Details (I)", Msg::ListInfo(idx)),
        round("\u{2713}", e.played, !writes, "Watched", Msg::ListSeen(idx)),
        round(
            if e.favorite { "\u{2665}" } else { "\u{2661}" },
            e.favorite,
            !writes,
            "Favourite",
            Msg::ListFav(idx)
        ),
    ]
    .spacing(t::S1);
    let card = column![thumb, row![text_col, actions].spacing(t::S2)]
        .spacing(t::S2)
        .width(w)
        .height(h + t::S2 + super::geom::CARD_TEXT);
    let body = Press::new(container(card).padding(t::S3))
        .on_press(Msg::ListOpen(idx))
        .style(|_| flat(None, t::TEXT, None, 0.0));
    mouse_area(body)
        .on_enter(Msg::Hover(super::DRow::List, idx))
        .into()
}

/// Row of a track / playlist entry / album: number, still, title, line of facts, overview.
#[allow(clippy::too_many_arguments)]
pub(super) fn list_row<'a>(
    e: &'a nocturne_core::source::Media,
    idx: usize,
    on: bool,
    parent: nocturne_core::source::Kind,
    img: Option<&'a Handle>,
    blur: Option<&'a Handle>,
) -> El<'a> {
    use nocturne_core::source::{Kind, fmt_runtime};
    let num = match (e.kind, e.number) {
        (Kind::Audio | Kind::Episode, Some(n)) => format!("{n:>2}"),
        _ => format!("0x{:02x}", idx + 1),
    };
    let mut sub = vec![];
    if parent != Kind::Season && e.kind == Kind::Episode {
        sub.push(e.episode_label());
    }
    if let Some(a) = e.sub.clone().filter(|_| e.kind == Kind::Audio) {
        sub.push(a);
    }
    sub.extend(e.runtime_min.map(fmt_runtime));
    if e.played {
        sub.push("watched \u{2713}".into());
    }
    sub.retain(|s| !s.is_empty());
    let mut still: Vec<El<'a>> = vec![tile(img, blur, &e.title, t::R_XS)];
    if let Some(p) = e.progress() {
        still.push(progress_over(p));
    }
    let mut info = column![
        container(
            g(e.title.clone())
                .size(t::BODY)
                .font(t::SEMIBOLD)
                .color(if on { t::TEXT } else { t::TEXT_SECONDARY })
                .wrapping(text::Wrapping::None)
        )
        .clip(true),
        mono(sub.join(" \u{b7} ")),
    ]
    .spacing(2)
    .width(Length::Fill);
    if let Some(o) = e.overview.clone().filter(|_| e.kind == Kind::Episode) {
        info = info.push(
            container(g(o).size(t::CAPTION + 1.0).color(t::TEXT_TERTIARY))
                .height(2.0 * (t::CAPTION + 1.0) * 1.3)
                .clip(true),
        );
    }
    let line = row![
        container(mono(num)).width(40),
        Stack::with_children(still).width(128).height(72),
        info
    ]
    .spacing(t::S4)
    .align_y(Alignment::Center);
    let r = Press::new(line)
        .width(Length::Fill)
        .height(super::geom::ROW_H)
        .padding([t::S3, t::PAGE_PAD])
        .on_press(Msg::ListOpen(idx))
        .style(move |s| {
            let bg = if on {
                Some(t::ACCENT_TINT)
            } else if matches!(s, Status::Hovered | Status::Pressed) {
                Some(t::GLASS)
            } else {
                None
            };
            flat(bg, t::TEXT, None, 0.0)
        });
    mouse_area(r)
        .on_enter(Msg::Hover(super::DRow::List, idx))
        .into()
}
