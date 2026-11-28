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

/// An empty state: an icon in a soft ring, a short title and one sentence, centred in the space it
/// is given (instead of a blank area).
pub fn empty_state<'a, M: 'a>(icon: &str, title: &str, body: &str) -> Element<'a, M> {
    let ring = container(super::icon(icon, 24.0, t::text_secondary()))
        .center(56)
        .style(|_| container::Style {
            background: Some(t::raised().into()),
            border: style::border(t::hairline_strong(), 1.0, 28.0),
            shadow: t::SHADOW_RAISED,
            ..Default::default()
        });
    container(
        column![
            ring,
            text(title.to_string())
                .size(t::BODY_LG)
                .font(t::semibold())
                .color(t::text()),
            container(
                text(body.to_string())
                    .size(t::CONTROL_TEXT + 1.0)
                    .line_height(1.5)
                    .color(t::text_secondary())
                    .align_x(Alignment::Center)
            )
            .max_width(420),
        ]
        .spacing(t::S3)
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .padding([t::S8 + t::S6, t::S6])
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
