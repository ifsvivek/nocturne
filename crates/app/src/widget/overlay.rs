//! Modal overlay: scrim + centred titled panel (Devices, Watch together, music queue; the same
//! look as the shortcut help). A click on the scrim (outside the panel) sends `away`.
use crate::{style, theme as t};
use iced::widget::{column, container, mouse_area, opaque, scrollable, text};
use iced::{Alignment, Element, Length, Size};

/// Titled panel `width` wide (shrunk to fit `win`), scrolling when taller than the window.
pub fn panel<'a, M: Clone + 'a>(
    title: &str,
    width: f32,
    win: Size,
    body: Vec<Element<'a, M>>,
    away: M,
) -> Element<'a, M> {
    let inner = column![text(title.to_string()).size(t::HEADING).font(t::bold())]
        .extend(body)
        .spacing(t::S3);
    let panel = container(scrollable(inner).style(style::scroll))
        .width(width.min((win.width - 48.0).max(300.0)))
        .max_height((win.height - 96.0).max(200.0))
        .padding(t::S6)
        .style(style::panel);
    scrim(panel, away)
}

/// Any element on the scrim; clicking outside it sends `away`.
pub fn scrim<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>, away: M) -> Element<'a, M> {
    mouse_area(
        container(opaque(content))
            .center(Length::Fill)
            .align_x(Alignment::Center)
            .style(style::scrim),
    )
    .on_press(away)
    .into()
}
