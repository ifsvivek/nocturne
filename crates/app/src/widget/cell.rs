//! Poster cells and image tiles (ports of the GPUI `poster_cell` / `tile`). A cell is a fixed-size
//! element (`cell_h`), so virtualised grids can compute row positions without measuring.
use super::press::Press;
use super::text::{initial, mono};
use crate::style::{border, glow_shadow};
use crate::theme as t;
use iced::widget::button::{Status, Style};
use iced::widget::image::Handle;
use iced::widget::{Stack, column, container, image, mouse_area, row, space, text};
use iced::{Alignment, Background, Color, ContentFit, Element, Gradient, Length, gradient};

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

/// Total height of a cell whose image is `h` tall (padding, caption and the bottom hairline).
pub fn cell_h(h: f32) -> f32 {
    h + 2.0 * t::S3 + CAPTION_H + 1.0
}

fn base_fill(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Gradient(Gradient::Linear(
            gradient::Linear::new(std::f32::consts::PI * 0.9)
                .add_stop(0.0, t::raised())
                .add_stop(1.0, t::surface()),
        ))),
        border: border(t::NONE, 0.0, t::r_card()),
        ..Default::default()
    }
}

/// Image slot: loaded image, blur stand-in, quiet gradient while loading, or the title's initial.
/// Never a flat empty box.
pub fn tile<'a, M: 'a>(pic: &Pic, title: &str, w: Length, h: Length) -> Element<'a, M> {
    let inner: Element<'a, M> = match pic {
        Pic::Ready(hd) | Pic::Blur(hd) => image(hd.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .border_radius(t::r_card())
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
        .style(base_fill)
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
    /// Unwatched count (bottom-right of the image).
    pub badge: Option<String>,
    /// Where the item lives, when several sources exist ("LOCAL", "2 SOURCES"; bottom-left).
    pub mark: Option<String>,
    /// `(favourite, played, writes allowed)`; `None` hides the chips.
    pub chips: Option<(bool, bool, bool)>,
    pub on_open: M,
    pub on_hover: M,
    pub on_fav: M,
    pub on_played: M,
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

pub fn poster_cell<'a, M: Clone + 'a>(c: Cell<M>) -> Element<'a, M> {
    let (w, h) = (c.w, c.h);
    let corner = |x: Element<'a, M>, ax: Alignment, ay: Alignment, pad: f32| -> Element<'a, M> {
        container(x)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(ax)
            .align_y(ay)
            .padding(pad)
            .into()
    };
    let mut layers: Vec<Element<'a, M>> = vec![tile(&c.pic, &c.title, Length::Fill, Length::Fill)];
    if !c.focused {
        layers.push(
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(t::POSTER_DIM.into()),
                    border: border(t::NONE, 0.0, t::r_card()),
                    ..Default::default()
                })
                .into(),
        );
    }
    layers.push(corner(
        mono(format!("0x{:02x}", c.index + 1))
            .color(t::INDEX_TAG)
            .into(),
        Alignment::Start,
        Alignment::Start,
        t::S2,
    ));
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
    if let Some(b) = c.badge {
        layers.push(corner(
            container(mono(b).color(t::on_accent()))
                .padding([0.0, t::S2])
                .style(|_| container::Style {
                    background: Some(t::accent().into()),
                    border: border(t::NONE, 0.0, 4.0),
                    ..Default::default()
                })
                .into(),
            Alignment::End,
            Alignment::End,
            t::S2,
        ));
    }
    if let Some(p) = c.progress {
        layers.push(corner(
            super::progress_bar(p, 3.0),
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
    let image_box = Stack::with_children(layers).width(w).height(h);
    let caption = column![
        text(c.title)
            .size(t::BODY)
            .font(t::semibold())
            .wrapping(text::Wrapping::None)
            .color(if c.focused {
                t::text()
            } else {
                t::text_secondary()
            }),
        container(mono(c.meta).wrapping(text::Wrapping::None))
            .height(16)
            .clip(true),
    ]
    .spacing(2)
    .width(w)
    .height(CAPTION_H - t::S3)
    .clip(true);
    let body = column![image_box, space().height(t::S3), caption];
    let mut layers: Vec<Element<'a, M>> = vec![
        container(body)
            .padding(t::S3)
            .width(w + 2.0 * t::S3)
            .height(cell_h(h) - 1.0)
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
        .width(w + 2.0 * t::S3)
        .height(cell_h(h) - 1.0);
    // Shared edges: a hairline right and below each cell.
    let hair = |w: Length, h: Length| -> Element<'a, M> {
        container(space())
            .width(w)
            .height(h)
            .style(crate::style::rule)
            .into()
    };
    let with_edges = row![
        column![
            cell,
            hair(Length::Fixed(w + 2.0 * t::S3), Length::Fixed(1.0))
        ],
        hair(Length::Fixed(1.0), Length::Fixed(cell_h(h)))
    ];
    mouse_area(with_edges)
        .on_enter(c.on_hover)
        .on_press(c.on_open)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}
