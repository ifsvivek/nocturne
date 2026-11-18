//! Section scaffolding: bands, section headings, hairline rules.
use super::text::mono;
use crate::{style, theme as t};
use iced::widget::{column, container, row, space, text};
use iced::{Alignment, Element, Length};

/// One 1 px hairline.
pub fn rule<'a, M: 'a>() -> Element<'a, M> {
    container(space().height(1))
        .width(Length::Fill)
        .style(style::rule)
        .into()
}

/// Full-width band: hairline divider below, content spans the window.
pub fn band<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    column![content.into(), rule()].width(Length::Fill).into()
}

/// Section heading: bold title + muted paragraph on the left, mono annotation on the right.
pub fn section<'a, M: 'a>(title: &str, body: &str, meta: String) -> Element<'a, M> {
    container(
        row![
            column![
                text(title.to_string()).size(28).font(t::bold()),
                container(
                    text(body.to_string())
                        .size(t::BODY)
                        .line_height(1.55)
                        .color(t::text_secondary())
                )
                .max_width(520),
            ]
            .spacing(t::S2),
            space().width(Length::Fill),
            mono(meta),
        ]
        .align_y(Alignment::End)
        .spacing(t::S6),
    )
    .padding([0.0, t::PAGE_PAD])
    .width(Length::Fill)
    .into()
}
