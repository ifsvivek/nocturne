//! Buttons, chips, switch, avatar, tooltip: ports of the GPUI `ui.rs` controls (same sizes,
//! borders, radii and hover states). Every pressable goes through [`Press`] for the eased hover.
use super::press::Press;
use super::text::{initial, mono, tracked};
use crate::style::{self, border, glow_shadow};
use crate::theme as t;
use iced::widget::button::{Status, Style};
use iced::widget::image::Handle;
use iced::widget::{Stack, container, image, row, space, svg, text, tooltip};
use iced::{Alignment, Background, Color, ContentFit, Element, Length, Padding, Shadow};

const R_CHIP: f32 = 4.0;
const R_CTL: f32 = 6.0;

/// Embedded icon (drawn as a mask tinted by `color`).
pub fn icon<'a>(name: &str, size: f32, color: Color) -> svg::Svg<'a> {
    let bytes: &'static [u8] = match name {
        "caret_down" => include_bytes!("../../assets/icons/caret_down.svg"),
        "caret_up" => include_bytes!("../../assets/icons/caret_up.svg"),
        "heart" => include_bytes!("../../assets/icons/heart.svg"),
        "heart_on" => include_bytes!("../../assets/icons/heart_on.svg"),
        "gear" => include_bytes!("../../assets/icons/gear.svg"),
        "folder" => include_bytes!("../../assets/icons/folder.svg"),
        "server" => include_bytes!("../../assets/icons/server.svg"),
        "layers" => include_bytes!("../../assets/icons/layers.svg"),
        "lock" => include_bytes!("../../assets/icons/lock.svg"),
        "files" => include_bytes!("../../assets/icons/files.svg"),
        "check" => include_bytes!("../../assets/icons/check.svg"),
        "arrow_right" => include_bytes!("../../assets/icons/arrow_right.svg"),
        "play" => include_bytes!("../../assets/icons/play.svg"),
        _ => include_bytes!("../../assets/icons/stop.svg"),
    };
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(style::icon(color))
}

/// Centre `el` vertically inside a fixed-height parent.
fn mid<'a, M: 'a>(el: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(el).center_y(Length::Fill).into()
}

fn st(bg: Option<Color>, ink: Color, edge: Option<Color>, radius: f32, shadow: Shadow) -> Style {
    Style {
        background: bg.map(Background::Color),
        text_color: ink,
        border: border(edge.unwrap_or(t::NONE), edge.is_some() as u8 as f32, radius),
        shadow,
        snap: false,
    }
}

/// Outlined status chip: 1 px outline, faint tinted fill, mono label.
pub fn chip<'a, M: 'a>(s: &str, accent: bool) -> Element<'a, M> {
    container(
        text(tracked(s))
            .shaping(iced::widget::text::Shaping::Advanced)
            .font(t::MONO)
            .size(t::MICRO)
            .color(if accent {
                t::accent_text()
            } else {
                t::text_secondary()
            }),
    )
    .height(20)
    .padding([0.0, t::S2])
    .center_y(20)
    .style(move |_| iced::widget::container::Style {
        background: Some(if accent { t::accent_tint() } else { t::GLASS }.into()),
        border: border(
            if accent {
                t::accent()
            } else {
                t::hairline_strong()
            },
            1.0,
            R_CHIP,
        ),
        ..Default::default()
    })
    .into()
}

/// Value control of a Settings row: filled, 1 px `EDGE` outline (>= 3:1), 13 px text, `CONTROL_H`
/// tall. `caret` marks a drop-down (flips while `open`); `focused` is the keyboard ring.
pub fn value_chip<'a, M: Clone + 'a>(
    value: &str,
    caret: bool,
    focused: bool,
    open: bool,
    on_press: M,
) -> Element<'a, M> {
    let lit = focused || open;
    let mut r = row![
        container(
            text(value.to_string())
                .size(t::CONTROL_TEXT)
                .font(t::medium())
                .wrapping(text::Wrapping::None)
        )
        .max_width(180)
        .clip(true)
    ]
    .spacing(t::S2)
    .align_y(Alignment::Center);
    if caret {
        r = r.push(icon(
            if open { "caret_up" } else { "caret_down" },
            12.0,
            if open {
                t::accent_text()
            } else {
                t::text_secondary()
            },
        ));
    }
    Press::new(mid(r))
        .height(t::CONTROL_H)
        .padding([0.0, t::S3])
        .on_press(on_press)
        .style(move |s| {
            let shadow = if focused {
                glow_shadow(0.7)
            } else {
                Shadow::default()
            };
            let (mut bg, mut edge) = (
                if open { t::accent_tint() } else { t::control() },
                if lit { t::accent() } else { t::edge() },
            );
            match s {
                Status::Hovered if !open => (bg, edge) = (t::control_hover(), t::text_secondary()),
                Status::Pressed => (bg, edge) = (t::accent_tint(), t::accent()),
                _ => {}
            }
            st(Some(bg), t::text(), Some(edge), R_CTL, shadow)
        })
        .into()
}

/// On / off switch: track + knob (on = accent, knob right) and the word ON / OFF beside it so the
/// state never rests on colour alone.
pub fn switch<'a, M: Clone + 'a>(on: bool, focused: bool, on_press: M) -> Element<'a, M> {
    let knob =
        container(space().width(16).height(16)).style(move |_| iced::widget::container::Style {
            background: Some(
                if on {
                    t::on_accent()
                } else {
                    t::text_secondary()
                }
                .into(),
            ),
            border: border(t::NONE, 0.0, 8.0),
            ..Default::default()
        });
    let track = container(knob)
        .width(40)
        .height(22)
        .padding(Padding {
            top: 3.0,
            left: if on { 21.0 } else { 3.0 },
            ..Padding::ZERO
        })
        .style(move |_| iced::widget::container::Style {
            background: Some(if on { t::accent() } else { t::control() }.into()),
            border: border(if on { t::accent() } else { t::edge() }, 1.0, 11.0),
            ..Default::default()
        });
    let word = text(if on { "ON" } else { "OFF" })
        .size(t::CONTROL_TEXT)
        .font(t::semibold())
        .color(if on {
            t::accent_text()
        } else {
            t::text_secondary()
        })
        .width(28);
    Press::new(mid(row![word, track]
        .spacing(t::S3)
        .align_y(Alignment::Center)))
    .height(t::CONTROL_H)
    .padding([0.0, t::S2])
    .on_press(on_press)
    .style(move |s| {
        let bg = if focused {
            Some(t::raised())
        } else if matches!(s, Status::Hovered | Status::Pressed) {
            Some(t::GLASS)
        } else {
            None
        };
        let shadow = if focused {
            glow_shadow(0.7)
        } else {
            Shadow::default()
        };
        let mut x = st(
            bg,
            t::text(),
            Some(if focused { t::accent() } else { t::NONE }),
            R_CTL,
            shadow,
        );
        x.border.width = 1.0;
        x
    })
    .into()
}

/// Primary: solid blue, dark label. Secondary: outline. `focused` = soft glow.
pub fn pill_button<'a, M: Clone + 'a>(
    label: &str,
    primary: bool,
    focused: bool,
    on_press: M,
) -> Element<'a, M> {
    pill(label, primary, focused, false, false, on_press)
}

/// [`pill_button`] as wide as its label (for rows of buttons) that can be dimmed (`off`: the
/// action is not available, e.g. server changes are switched off) while still answering the press
/// so the caller can say why.
pub fn pill_button_dim<'a, M: Clone + 'a>(
    label: &str,
    primary: bool,
    focused: bool,
    off: bool,
    on_press: M,
) -> Element<'a, M> {
    pill(label, primary, focused, off, true, on_press)
}

fn pill<'a, M: Clone + 'a>(
    label: &str,
    primary: bool,
    focused: bool,
    off: bool,
    shrink: bool,
    on_press: M,
) -> Element<'a, M> {
    let mut r = row![].spacing(t::S2).align_y(Alignment::Center);
    if primary {
        r = r.push(text("\u{25B6}").font(t::MONO).size(t::CAPTION));
    }
    r = r.push(
        text(label.to_string())
            .shaping(iced::widget::text::Shaping::Advanced)
            .size(t::BODY)
            .font(t::semibold()),
    );
    let body = if shrink {
        container(r).center_y(Length::Fill)
    } else {
        container(r).center(Length::Fill)
    };
    let mut btn = Press::new(body).height(44).padding([0.0, t::S6]);
    if shrink {
        btn = btn.width(Length::Shrink);
    }
    btn.on_press(on_press)
        .style(move |s| {
            let shadow = if focused {
                glow_shadow(0.7)
            } else {
                Shadow::default()
            };
            let hot = matches!(s, Status::Hovered | Status::Pressed);
            let mut x = if primary {
                st(
                    Some(if focused || hot {
                        t::accent_hover()
                    } else {
                        t::accent()
                    }),
                    t::on_accent(),
                    Some(t::accent()),
                    R_CTL,
                    shadow,
                )
            } else {
                let bg = if hot {
                    Some(t::GLASS)
                } else if focused {
                    Some(t::accent_tint())
                } else {
                    None
                };
                let edge = if focused {
                    t::accent()
                } else if hot {
                    t::text_tertiary()
                } else {
                    t::edge()
                };
                st(bg, t::text(), Some(edge), R_CTL, shadow)
            };
            if s == Status::Pressed {
                x.background = x.background.map(|b| match b {
                    Background::Color(c) => Background::Color(Color { a: c.a * 0.85, ..c }),
                    o => o,
                });
            }
            if off {
                x.text_color.a *= 0.4;
                x.border.color.a *= 0.4;
                x.background = x.background.map(|b| match b {
                    Background::Color(c) => Background::Color(Color { a: c.a * 0.4, ..c }),
                    o => o,
                });
            }
            x
        })
        .into()
}

/// Compact text button for rows and dialogs. `danger` = red outline (destructive only),
/// `primary` = solid blue, otherwise outline. `focused` adds the glow; `off` dims it.
pub fn action_button<'a, M: Clone + 'a>(
    label: &str,
    primary: bool,
    danger: bool,
    focused: bool,
    off: bool,
    on_press: M,
) -> Element<'a, M> {
    let (ink, edge) = match (primary, danger) {
        (_, true) => (t::danger(), t::danger()),
        (true, _) => (t::on_accent(), t::accent()),
        _ => (t::text(), t::edge()),
    };
    Press::new(mid(text(label.to_string())
        .size(t::CAPTION + 1.0)
        .font(t::semibold())))
    .height(32)
    .padding([0.0, t::S3])
    .on_press(on_press)
    .style(move |s| {
        let shadow = if focused {
            glow_shadow(0.7)
        } else {
            Shadow::default()
        };
        let ring = if focused && !danger {
            t::accent()
        } else {
            edge
        };
        let hot = matches!(s, Status::Hovered | Status::Pressed);
        let bg = if danger {
            Some(t::danger_tint())
        } else if primary {
            Some(if hot { t::accent_hover() } else { t::accent() })
        } else if focused {
            Some(t::accent_tint())
        } else if hot {
            Some(t::GLASS)
        } else {
            None
        };
        let mut x = st(bg, ink, Some(ring), R_CTL, shadow);
        if off {
            for c in [&mut x.text_color, &mut x.border.color] {
                c.a *= 0.4;
            }
            x.background = x.background.map(|b| match b {
                Background::Color(c) => Background::Color(Color { a: c.a * 0.4, ..c }),
                o => o,
            });
        }
        x
    })
    .into()
}

/// A small dot or bar drawn over the bottom of a shrink-sized base.
fn overlay_bottom<'a, M: 'a>(base: Element<'a, M>, top: Element<'a, M>) -> Element<'a, M> {
    Stack::from_vec(vec![base, top])
        .width(Length::Shrink)
        .height(Length::Shrink)
        .into()
}

/// Top-bar text link: active = white with a small blue dot, focused = blue text.
pub fn nav_link<'a, M: Clone + 'a>(
    label: &str,
    active: bool,
    focused: bool,
    on_press: M,
) -> Element<'a, M> {
    let base = container(text(label.to_string()).size(t::BODY).font(t::medium()))
        .height(t::HEADER_H)
        .padding([0.0, t::S3])
        .center_y(t::HEADER_H);
    let dot: Element<'a, M> = if active || focused {
        container(
            container(space().width(4).height(4)).style(|_| iced::widget::container::Style {
                background: Some(t::accent().into()),
                border: border(t::NONE, 0.0, 2.0),
                ..Default::default()
            }),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .align_y(Alignment::End)
        .padding(Padding {
            bottom: 10.0,
            ..Padding::ZERO
        })
        .into()
    } else {
        space().into()
    };
    Press::new(overlay_bottom(base.into(), dot))
        .on_press(on_press)
        .style(move |s| {
            let ink = if focused {
                t::accent_text()
            } else if matches!(s, Status::Hovered | Status::Pressed) || active {
                t::text()
            } else {
                t::text_secondary()
            };
            st(None, ink, None, 0.0, Shadow::default())
        })
        .into()
}

/// Text tab: active = white with a blue underline.
pub fn tab<'a, M: Clone + 'a>(label: &str, active: bool, on_press: M) -> Element<'a, M> {
    let base = container(text(label.to_string()).size(t::BODY).font(t::medium()))
        .height(36)
        .padding([0.0, t::S3])
        .center_y(36);
    let bar = container(
        container(space().width(Length::Fill).height(2)).style(move |_| {
            iced::widget::container::Style {
                background: Some(if active { t::accent() } else { t::NONE }.into()),
                ..Default::default()
            }
        }),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_y(Alignment::End);
    Press::new(overlay_bottom(base.into(), bar.into()))
        .on_press(on_press)
        .style(move |s| {
            let ink = if active || matches!(s, Status::Hovered | Status::Pressed) {
                t::text()
            } else {
                t::text_secondary()
            };
            st(None, ink, None, 0.0, Shadow::default())
        })
        .into()
}

/// Round profile picture of `size` px: the picture when there is one, otherwise the name's
/// initial. `ring` / `ring_w` draw the border; `ink` / `font` style the initial.
pub fn avatar<'a, M: 'a>(
    size: f32,
    name: &str,
    pic: Option<&Handle>,
    ring: Color,
    ring_w: f32,
    font_size: f32,
    ink: Color,
) -> Element<'a, M> {
    let inner: Element<'a, M> = match pic {
        Some(h) => image(h.clone())
            .width(size)
            .height(size)
            .content_fit(ContentFit::Cover)
            .border_radius(size / 2.0)
            .into(),
        None => text(initial(name))
            .font(t::MONO)
            .size(font_size)
            .color(ink)
            .into(),
    };
    container(inner)
        .width(size)
        .height(size)
        .center(size)
        .clip(true)
        .style(move |_| iced::widget::container::Style {
            background: Some(t::raised().into()),
            border: border(ring, ring_w, size / 2.0),
            ..Default::default()
        })
        .into()
}

/// Names an icon-only control for pointer users.
pub fn tip<'a, M: 'a>(content: impl Into<Element<'a, M>>, label: &'static str) -> Element<'a, M> {
    tooltip(
        content,
        container(mono(label).color(t::text()))
            .padding([t::S1, t::S2])
            .style(style::tip),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// Thin progress line: faint track, blue fill. `frac` is 0..1.
pub fn progress_bar<'a, M: 'a>(frac: f32, h: f32) -> Element<'a, M> {
    iced::widget::progress_bar(0.0..=1.0, frac.clamp(0.0, 1.0))
        .girth(h)
        .style(|_| iced::widget::progress_bar::Style {
            background: t::PROGRESS_TRACK.into(),
            bar: t::accent().into(),
            border: iced::Border::default(),
        })
        .into()
}

/// Row of a menu (drop-down, account menu): `on` = highlighted by keyboard, hover = glass.
pub fn menu_row<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    on: bool,
    dim: bool,
    on_press: M,
) -> Element<'a, M> {
    Press::new(content)
        .width(Length::Fill)
        .padding([t::S2, t::S3])
        .on_press(on_press)
        .style(move |s| {
            let bg = if on {
                Some(t::accent_tint())
            } else if matches!(s, Status::Hovered | Status::Pressed) {
                Some(t::GLASS)
            } else {
                None
            };
            let mut x = st(
                bg,
                if on { t::accent_text() } else { t::text() },
                None,
                R_CTL,
                Shadow::default(),
            );
            if dim {
                x.text_color.a = 0.55;
            }
            x
        })
        .into()
}

/// 0.35..1.0 breathing opacity used by "Connecting" style labels, `period_ms` per cycle.
pub fn pulse(now: std::time::Instant, period_ms: u128) -> f32 {
    // Reduce motion: a steady, calm label instead of breathing.
    if crate::motion::reduced() {
        return 0.75;
    }
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(|| now);
    let v =
        (now.saturating_duration_since(start).as_millis() % period_ms) as f32 / period_ms as f32;
    0.35 + 0.65 * (1.0 - (2.0 * v - 1.0).abs())
}
