//! Shortcut reference overlay (`?`). The data lives in `nocturne_core::help`.
use super::App;
use super::message::Message;
use crate::style;
use crate::theme as t;
use crate::widget::{mono, tracked};
use iced::widget::{column, container, mouse_area, opaque, row, scrollable, text};
use iced::{Alignment, Element, Length};
use nocturne_core::help::SHORTCUTS;

impl App {
    pub(super) fn help_overlay(&self) -> Element<'_, Message> {
        let mut secs: Vec<Element<Message>> = vec![];
        for (head, rows) in SHORTCUTS {
            let mut sec = column![mono(tracked(head))].spacing(t::S1).width(500);
            for (k, d) in rows {
                sec = sec.push(
                    row![
                        text(*k)
                            .font(t::MONO)
                            .size(t::BODY)
                            .color(t::ACCENT_TEXT)
                            .width(170),
                        text(*d)
                            .size(t::BODY)
                            .color(t::TEXT_SECONDARY)
                            .width(Length::Fill),
                    ]
                    .spacing(t::S4),
                );
            }
            secs.push(sec.into());
        }
        let cols = row(secs).spacing(t::S8).wrap().vertical_spacing(t::S4);
        let panel = container(
            scrollable(
                column![
                    text("Keyboard shortcuts").size(t::HEADING).font(t::BOLD),
                    cols,
                    mono("Press Esc or ? to close"),
                ]
                .spacing(t::S3),
            )
            .style(style::scroll),
        )
        .width((self.win.width - 48.0).clamp(300.0, 1100.0))
        .max_height((self.win.height - 96.0).max(200.0))
        .padding(t::S6)
        .style(style::panel);
        // Click outside the panel closes it.
        mouse_area(
            container(opaque(panel))
                .center(Length::Fill)
                .align_x(Alignment::Center)
                .style(style::scrim),
        )
        .on_press(Message::CloseOverlays)
        .into()
    }
}
