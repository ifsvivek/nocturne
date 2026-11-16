//! "Who's watching?" view. The model (start decision, selection, default flag) lives in
//! `nocturne_core::account::picker`.
pub use nocturne_core::account::picker::*;

use crate::style::{border, glow_shadow};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{avatar, mono, wordmark};
use iced::widget::button::{Status, Style};
use iced::widget::image::Handle;
use iced::widget::{column, container, mouse_area, row, text};
use iced::{Alignment, Color, Element, Length, Shadow};
use nocturne_session::Account;

pub fn view<'a, M: Clone + 'a>(
    p: &'a Picker,
    pic: &dyn Fn(&Account) -> Option<Handle>,
    on: impl Fn(Act) -> M + Copy + 'a,
) -> Element<'a, M> {
    let n_acc = p.accounts.len();
    let tile = |i: usize, label: String, face: Element<'a, M>| -> Element<'a, M> {
        let on_it = i == p.sel;
        let act = if i < n_acc { Act::Pick(i) } else { Act::Add };
        let ring = Press::new(face).on_press(on(act)).style(move |s| Style {
            background: None,
            text_color: t::ACCENT_TEXT,
            border: border(
                if on_it {
                    t::ACCENT
                } else if matches!(s, Status::Hovered | Status::Pressed) {
                    t::TEXT_TERTIARY
                } else {
                    t::EDGE
                },
                2.0,
                PIC / 2.0,
            ),
            shadow: if on_it {
                glow_shadow(t::GLOW.a)
            } else {
                Shadow::default()
            },
            snap: false,
        });
        let name = text(label)
            .size(t::BODY_LG)
            .font(if on_it { t::SEMIBOLD } else { t::FONT })
            .color(if on_it { t::TEXT } else { t::TEXT_SECONDARY });
        mouse_area(
            container(
                column![ring, name]
                    .spacing(t::S3)
                    .align_x(Alignment::Center),
            )
            .width(TILE_W)
            .align_x(Alignment::Center),
        )
        .on_press(on(act))
        .into()
    };
    let mut tiles: Vec<Element<'a, M>> = p
        .accounts
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let face = avatar(
                PIC - 4.0,
                &a.user_name,
                pic(a).as_ref(),
                t::NONE,
                0.0,
                44.0,
                t::ACCENT_TEXT,
            );
            tile(i, a.user_name.clone(), face)
        })
        .collect();
    let add: Element<'a, M> = container(text("+").size(44).font(t::MONO).color(t::TEXT_SECONDARY))
        .width(PIC - 4.0)
        .height(PIC - 4.0)
        .center(PIC - 4.0)
        .style(|_| iced::widget::container::Style {
            background: Some(t::GLASS.into()),
            border: border(t::NONE, 0.0, PIC / 2.0),
            ..Default::default()
        })
        .into();
    tiles.push(tile(n_acc, "Add profile".into(), add));
    let grid = container(row(tiles).spacing(GAP).wrap().vertical_spacing(GAP))
        .width(p.cols.min(p.tiles()) as f32 * (TILE_W + GAP) - GAP)
        .align_x(Alignment::Center);
    let adding = p.sel == n_acc;
    let checked = p.checked();
    let boxx = container(
        text(if checked { "\u{2713}" } else { "" })
            .size(t::CAPTION)
            .color(t::ON_ACCENT),
    )
    .width(18)
    .height(18)
    .center(18)
    .style(move |_| iced::widget::container::Style {
        background: checked.then(|| t::ACCENT.into()),
        border: border(if checked { t::ACCENT } else { t::EDGE }, 1.0, 4.0),
        ..Default::default()
    });
    let ink = Color {
        a: if adding { 0.4 } else { 1.0 },
        ..t::TEXT_SECONDARY
    };
    let check = Press::new(
        row![
            boxx,
            text("Use as default profile").size(t::BODY).color(ink)
        ]
        .spacing(t::S2)
        .align_y(Alignment::Center),
    )
    .on_press(on(Act::Toggle))
    .style(|_| Style::default());
    container(
        column![
            wordmark(t::HEADING),
            text("Who's watching?").size(36).font(t::BOLD),
            grid,
            check,
            mono("\u{2190} \u{2192} move    ENTER choose    SPACE default profile"),
        ]
        .spacing(t::S8)
        .align_x(Alignment::Center),
    )
    .center(Length::Fill)
    .into()
}
