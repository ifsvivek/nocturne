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
    section_with(
        title,
        body,
        meta,
        &t::typo(&Default::default(), 28.0, iced::font::Weight::Bold),
        t::page_pad(),
        true,
        None,
    )
}

/// [`section`] with the title's typography (`.row-title`) and the row's horizontal inset.
/// `details` off keeps only the title; `extra` (the row arrows) sits at the right end.
pub fn section_with<'a, M: 'a>(
    title: &str,
    body: &str,
    meta: String,
    title_t: &t::Typo,
    inset: f32,
    details: bool,
    extra: Option<Element<'a, M>>,
) -> Element<'a, M> {
    if !details {
        let mut r = row![title_t.text(title, t::text()), space().width(Length::Fill)]
            .align_y(Alignment::Center)
            .spacing(t::S6);
        if let Some(x) = extra {
            r = r.push(x);
        }
        return container(r)
            .padding([0.0, inset])
            .width(Length::Fill)
            .into();
    }
    container(
        row![
            column![
                title_t.text(title, t::text()),
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
        .extend(extra)
        .align_y(Alignment::End)
        .spacing(t::S6),
    )
    .padding([0.0, inset])
    .width(Length::Fill)
    .into()
}
