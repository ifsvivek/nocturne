//! The two-row controls bar: transport, title and status, menus; seek bar with chapters, marked
//! segments and buffered range, remaining/total clock. Plain functions over `&Player`.
use super::{Act, BAR_H, Drag, Msg, Player};
use crate::style::{border, glow_shadow};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::text::mono;
use crate::widget::track::track;
use iced::widget::button::{Status, Style};
use iced::widget::{column, container, mouse_area, row, svg, text};
use iced::{Alignment, Background, Color, Element, Length, Shadow};
use nocturne_core::source::fmt_clock;
use nocturne_core::video::{self, Pop, Repeat};

const R_CTL: f32 = 6.0;
/// Fixed widths of row 2 around the seek track, so the hover tip can find the track without
/// measuring it: `[S6][clock 52][S3][track][S3][remaining 60][S6]`.
const CLOCK_W: f32 = 52.0;
const REMAIN_W: f32 = 60.0;

/// Left edge and width of the seek track in a window `win_w` wide.
pub fn track_rect(win_w: f32) -> (f32, f32) {
    let left = t::S6 + CLOCK_W + t::S3;
    (left, (win_w - left - t::S3 - REMAIN_W - t::S6).max(1.0))
}

pub fn glyph<'a>(name: &str, size: f32, color: Color) -> svg::Svg<'a> {
    let bytes: &'static [u8] = match name {
        "prev" => include_bytes!("../../assets/icons/prev.svg"),
        "back" => include_bytes!("../../assets/icons/back.svg"),
        "play" => include_bytes!("../../assets/icons/play.svg"),
        "pause" => include_bytes!("../../assets/icons/pause.svg"),
        "fwd" => include_bytes!("../../assets/icons/fwd.svg"),
        "next" => include_bytes!("../../assets/icons/next.svg"),
        "mute" => include_bytes!("../../assets/icons/mute.svg"),
        "volume" => include_bytes!("../../assets/icons/volume.svg"),
        "queue" => include_bytes!("../../assets/icons/queue.svg"),
        "cc" => include_bytes!("../../assets/icons/cc.svg"),
        "audio" => include_bytes!("../../assets/icons/audio.svg"),
        "gear" => include_bytes!("../../assets/icons/gear.svg"),
        "pip" => include_bytes!("../../assets/icons/pip.svg"),
        "fullscreen" => include_bytes!("../../assets/icons/fullscreen.svg"),
        "heart" => include_bytes!("../../assets/icons/heart.svg"),
        "heart_on" => include_bytes!("../../assets/icons/heart_on.svg"),
        _ => include_bytes!("../../assets/icons/stop.svg"),
    };
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(crate::style::icon(color))
}

fn look(s: Status, primary: bool, dim: bool) -> Style {
    let hot = matches!(s, Status::Hovered | Status::Pressed);
    let (ink, edge) = if primary {
        (t::ON_ACCENT, t::ACCENT)
    } else {
        (t::TEXT, t::EDGE)
    };
    let mut bg = if primary {
        Some(if hot { t::ACCENT_HOVER } else { t::ACCENT })
    } else if hot {
        Some(t::GLASS)
    } else {
        None
    };
    if s == Status::Pressed {
        bg = bg.map(|c| Color { a: c.a * 0.85, ..c });
    }
    let k = if dim { 0.4 } else { 1.0 };
    Style {
        background: bg.map(|c| Background::Color(Color { a: c.a * k, ..c })),
        text_color: Color {
            a: ink.a * k,
            ..ink
        },
        border: border(
            Color {
                a: edge.a * k,
                ..edge
            },
            1.0,
            R_CTL,
        ),
        shadow: Shadow::default(),
        snap: false,
    }
}

/// Icon (and optional mono label) button of the controls bar. `primary` = lit (the play button,
/// an open menu); `dim` = unavailable but shown.
pub fn ctl_button<'a>(
    icon: &str,
    label: &'a str,
    primary: bool,
    dim: bool,
    on: Option<Msg>,
) -> Element<'a, Msg> {
    let ink = if primary { t::ON_ACCENT } else { t::TEXT };
    let mut r = row![glyph(icon, 16.0, ink)]
        .spacing(t::S1)
        .align_y(Alignment::Center);
    if !label.is_empty() {
        r = r.push(mono(label).color(ink));
    }
    let mut b = Press::new(container(r).center(Length::Fill))
        .height(34)
        .padding([0.0, t::S2])
        .on_press_maybe(on)
        .style(move |s| look(s, primary, dim));
    b = b.width(if label.is_empty() { 36.0 } else { 58.0 });
    b.into()
}

fn act(a: Act) -> Option<Msg> {
    Some(Msg::Act(a))
}

/// Text button used by the prompts (skip chip, next-episode card, resume prompt).
pub fn chip_button<'a>(label: &str, primary: bool, on: Msg) -> Element<'a, Msg> {
    let ink = if primary { t::ON_ACCENT } else { t::TEXT };
    Press::new(
        container(
            text(label.to_string())
                .size(13)
                .font(t::SEMIBOLD)
                .color(ink),
        )
        .center_y(Length::Fill),
    )
    .height(36)
    .padding([0.0, t::S4])
    .on_press(on)
    .style(move |s| {
        let hot = matches!(s, Status::Hovered | Status::Pressed);
        let (bg, edge) = if primary {
            (if hot { t::ACCENT_HOVER } else { t::ACCENT }, t::ACCENT)
        } else {
            (
                if hot {
                    t::GLASS
                } else {
                    Color {
                        a: 0.93,
                        ..t::SURFACE
                    }
                },
                t::EDGE,
            )
        };
        Style {
            background: Some(Background::Color(bg)),
            text_color: ink,
            border: border(edge, 1.0, R_CTL),
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .into()
}

pub fn bar(p: &Player) -> Element<'_, Msg> {
    let (pos0, dur) = (p.pos(), p.dur());
    let (mut pos, mut frac) = (pos0, p.frac());
    let mut vol = p.state.as_ref().map_or(100.0, |s| s.volume);
    if let (Some(Drag::Seek(f)), Some(d)) = (p.drag, dur) {
        (frac, pos) = (f, f as f64 * d);
    }
    if let Some(Drag::Vol(f)) = p.drag {
        vol = f as f64 * 130.0;
    }
    let paused = p.paused();
    let status = if !p.loaded {
        "loading".to_string()
    } else if p.ended {
        "finished".to_string()
    } else {
        let mut parts = vec![
            p.media.series_name.clone().unwrap_or_default(),
            p.media.episode_label(),
        ];
        if p.set.method != "Direct Play" {
            parts.push(p.set.method.to_string());
        }
        parts
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" \u{00B7} ")
    };
    let right_time = match dur {
        Some(d) if !p.show_total => format!("-{}", fmt_clock((d - pos).max(0.0))),
        Some(d) => fmt_clock(d),
        None => "--:--".into(),
    };
    let ends = dur
        .filter(|_| p.loaded && !p.ended)
        .map(|d| video::ends_at(video::local_hm(), (d - pos).max(0.0), p.set.speed));
    let cache_frac = p
        .cache_end
        .zip(dur)
        .map_or(0.0, |(c, d)| (c / d).clamp(0.0, 1.0) as f32);
    let ticks: Vec<f32> = dur
        .map(|d| {
            p.chapters
                .iter()
                .filter(|c| c.0 > 0.5)
                .map(|c| (c.0 / d) as f32)
                .collect()
        })
        .unwrap_or_default();
    let segs: Vec<(f32, f32)> = dur
        .map(|d| {
            p.segs
                .iter()
                .map(|s| ((s.start / d) as f32, ((s.end - s.start) / d) as f32))
                .collect()
        })
        .unwrap_or_default();
    let seek = track(frac, Msg::Seek)
        .cache(cache_frac)
        .ticks(ticks)
        .segs(segs);

    let (can_prev, can_next) = (p.prev_index().is_some(), p.next_index().is_some());
    let mut ctl = row![].spacing(t::S2).align_y(Alignment::Center);
    if !p.mini {
        ctl = ctl
            .push(ctl_button("prev", "", false, !can_prev, act(Act::Step(-1))))
            .push(ctl_button(
                "back",
                "10",
                false,
                false,
                act(Act::Seek(-10.0)),
            ));
    }
    ctl = ctl.push(ctl_button(
        if paused { "play" } else { "pause" },
        "",
        true,
        false,
        act(Act::Toggle),
    ));
    if !p.mini {
        ctl = ctl
            .push(ctl_button("fwd", "30", false, false, act(Act::Seek(30.0))))
            .push(ctl_button(
                "next",
                "",
                p.ended && can_next,
                !can_next,
                act(Act::Step(1)),
            ));
    }

    let volume = row![
        ctl_button(
            if vol <= 0.0 { "mute" } else { "volume" },
            "",
            false,
            false,
            act(Act::Mute)
        ),
        container(
            track((vol / 130.0) as f32, Msg::Vol)
                .height(24.0)
                .thickness(3.0, 3.0)
        )
        .width(84),
    ]
    .spacing(t::S2)
    .align_y(Alignment::Center);
    let volume = if vol > 100.0 {
        volume.push(mono(format!("{}%", vol.round())))
    } else {
        volume
    };

    let mut menus = row![].spacing(t::S2).align_y(Alignment::Center);
    if !p.mini {
        let can_fav = p.can_favorite();
        menus = menus
            .push(ctl_button(
                if p.fav { "heart_on" } else { "heart" },
                "",
                p.fav,
                !can_fav,
                act(Act::Fav),
            ))
            .push(ctl_button(
                "queue",
                "",
                p.queue_open,
                p.queue.len() <= 1,
                act(Act::Queue),
            ))
            .push(ctl_button(
                "cc",
                "",
                p.pop == Some(Pop::Subs),
                false,
                act(Act::Pop(Pop::Subs)),
            ))
            .push(ctl_button(
                "audio",
                "",
                p.pop == Some(Pop::Audio),
                false,
                act(Act::Pop(Pop::Audio)),
            ))
            .push(volume)
            .push(ctl_button(
                "gear",
                "",
                p.pop.is_some_and(|x| x.in_settings()),
                false,
                act(Act::Pop(Pop::Settings)),
            ));
    }
    menus = menus.push(ctl_button("pip", "", p.mini, false, act(Act::Mini)));
    if p.video && !p.mini {
        menus = menus.push(ctl_button(
            "fullscreen",
            "",
            false,
            false,
            act(Act::Fullscreen),
        ));
    }
    menus = menus.push(ctl_button("stop", "", false, false, act(Act::Stop)));

    let nowrap = text::Wrapping::None;
    let mut mid = row![].spacing(t::S3).align_y(Alignment::Center);
    if let Some(e) = ends {
        mid = mid.push(mono(e));
    }
    mid = mid
        .push(
            text(p.media.title.clone())
                .size(14)
                .font(t::SEMIBOLD)
                .color(t::TEXT)
                .wrapping(nowrap),
        )
        .push(mono(status).wrapping(nowrap));
    let row1 = container(
        row![ctl, container(mid).width(Length::Fill).clip(true), menus]
            .spacing(t::S4)
            .align_y(Alignment::Center),
    )
    .height(52)
    .padding([0.0, t::S6])
    .center_y(52);
    let row2 = container(
        row![
            container(mono(fmt_clock(pos))).width(CLOCK_W),
            container(seek).width(Length::Fill),
            Press::new(
                container(mono(right_time).align_x(iced::alignment::Horizontal::Right))
                    .width(Length::Fill)
            )
            .width(REMAIN_W)
            .on_press(Msg::Act(Act::ShowTotal)),
        ]
        .spacing(t::S3)
        .align_y(Alignment::Center),
    )
    .height(36)
    .padding([0.0, t::S6])
    .center_y(36);

    let body = column![row1, row2];
    let fill = Color {
        a: if p.video { 0.77 } else { 0.96 },
        ..t::FRAME
    };
    let bar = container(body)
        .width(Length::Fill)
        .height(BAR_H)
        .style(move |_| container::Style {
            background: Some(Background::Color(fill)),
            border: iced::Border {
                color: t::HAIRLINE_STRONG,
                width: 1.0,
                radius: 0.0.into(),
            },
            shadow: glow_shadow(0.10),
            ..Default::default()
        });
    mouse_area(iced::widget::opaque(bar))
        .on_enter(Msg::Act(Act::BarHover(true)))
        .on_exit(Msg::Act(Act::BarHover(false)))
        .into()
}

pub fn repeat_label(r: Repeat) -> &'static str {
    match r {
        Repeat::Off => "Repeat: off",
        Repeat::All => "Repeat: all",
        Repeat::One => "Repeat: one",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_track_rect_matches_the_row() {
        let (x, w) = track_rect(1280.0);
        assert_eq!(x, 88.0);
        assert_eq!(x + w + t::S3 + REMAIN_W + t::S6, 1280.0);
        assert_eq!(track_rect(10.0).1, 1.0);
    }
}
