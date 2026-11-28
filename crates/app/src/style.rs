//! Style functions for containers, scrollables, text inputs, svgs and tooltips. Only theme tokens
//! go in; no colour literals here.
use crate::theme as t;
use iced::widget::{container, scrollable, svg, text_input};
use iced::{Background, Border, Color, Shadow, Vector};

pub fn border(color: Color, width: f32, radius: f32) -> Border {
    Border {
        color,
        width,
        radius: radius.into(),
    }
}

/// Soft focus glow; the GPUI app replaced the token's alpha with `a`.
pub fn glow_shadow(a: f32) -> Shadow {
    Shadow {
        color: Color { a, ..t::glow() },
        offset: Vector::ZERO,
        blur_radius: 24.0,
    }
}

/// Flat fill.
pub fn fill(c: Color) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(c)),
        ..Default::default()
    }
}

/// The window background of the active theme (a colour or a gradient).
pub fn page(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(t::bg()),
        ..Default::default()
    }
}

/// Overlay level: panel on the surface colour with the strong hairline and the deep shadow
/// (dialogs, menus, account panel).
pub fn panel(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(t::surface().into()),
        border: border(t::hairline_strong(), 1.0, t::r_panel()),
        shadow: t::SHADOW_OVERLAY,
        ..Default::default()
    }
}

/// Raised level: a card that groups content on the page (Settings groups, Gateway status).
pub fn card(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(t::surface().into()),
        border: border(t::hairline(), 1.0, t::r_panel()),
        shadow: t::SHADOW_RAISED,
        ..Default::default()
    }
}

/// Overlay level on the raised colour: toasts, the offline banner.
pub fn raised_card(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(t::raised().into()),
        border: border(t::hairline_strong(), 1.0, t::r_panel()),
        text_color: Some(t::text()),
        shadow: t::SHADOW_OVERLAY,
        ..Default::default()
    }
}

pub fn scrim(_: &iced::Theme) -> container::Style {
    fill(t::scrim())(&iced::Theme::Dark)
}

/// Tooltip bubble for icon-only buttons.
pub fn tip(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(t::raised().into()),
        border: border(t::hairline_strong(), 1.0, t::r_xs()),
        text_color: Some(t::text()),
        shadow: t::SHADOW_RAISED,
        ..Default::default()
    }
}

/// One hairline rule (1 px).
pub fn rule(_: &iced::Theme) -> container::Style {
    fill(t::hairline())(&iced::Theme::Dark)
}

/// Tint an icon.
pub fn icon(color: Color) -> impl Fn(&iced::Theme, svg::Status) -> svg::Style {
    move |_, _| svg::Style { color: Some(color) }
}

/// Thin, quiet scrollbar: transparent rail, edge-coloured thumb.
pub fn scroll(_: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let hot = matches!(
        status,
        scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
    );
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(if hot { t::edge() } else { t::hairline_strong() }),
            border: border(t::NONE, 0.0, 3.0),
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: t::raised().into(),
            border: border(t::hairline_strong(), 1.0, 12.0),
            shadow: Shadow::default(),
            icon: t::text(),
        },
    }
}

/// Hairline-outlined input, accent ring while focused (used from step 4b on).
pub fn input(_: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let ring = match status {
        text_input::Status::Focused { .. } => t::accent(),
        text_input::Status::Hovered => t::text_secondary(),
        _ => t::edge(),
    };
    text_input::Style {
        background: Background::Color(t::NONE),
        border: border(ring, 1.0, t::R_CTL),
        icon: t::text_secondary(),
        placeholder: t::text_tertiary(),
        value: t::text(),
        selection: t::accent_tint(),
    }
}

/// Linear gradient fill; `deg` as in CSS (90 = left to right, 180 = top to bottom).
pub fn gradient(deg: f32, stops: &[(f32, Color)]) -> Background {
    let mut g = iced::gradient::Linear::new(iced::Radians(deg.to_radians()));
    for (at, c) in stops {
        g = g.add_stop(*at, *c);
    }
    Background::Gradient(iced::Gradient::Linear(g))
}

/// Like [`scroll`], but the thumb only shows while the pointer is over the strip (poster rows).
pub fn scroll_quiet(theme: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let mut s = scroll(theme, status);
    if matches!(status, scrollable::Status::Active { .. }) {
        s.horizontal_rail.scroller.background = Background::Color(t::NONE);
        s.vertical_rail.scroller.background = Background::Color(t::NONE);
    }
    s
}
