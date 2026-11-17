//! Everything that floats over the video besides the bar: the track/settings menus, queue panel,
//! context menu, scrub tooltip, skip chip, next-episode card, resume prompt, OSD, stats, key help
//! and notes. Each returns an element sized to fill the page (aligned inside) or a pinned one;
//! none is opaque except the panels themselves, so clicks elsewhere reach the video.
use super::controls::{chip_button, track_rect};
use super::logic;
use super::{Act, BAR_H, CtxAct, Msg, Player, Sheet};
use crate::style::border;
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::text::mono;
use iced::widget::button::{Status, Style};
use iced::widget::{column, container, image, opaque, pin, row, scrollable, space, text};
use iced::{Alignment, Background, Color, Element, Length, Padding, Rectangle, Shadow};
use nocturne_core::hdr;
use nocturne_core::player_logic as pl;
use nocturne_core::source::fmt_clock;
use nocturne_core::video::{self, MenuAct, Repeat};

fn panel_style(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(
            Color {
                a: 0.93,
                ..t::SURFACE
            }
            .into(),
        ),
        border: border(t::HAIRLINE_STRONG, 1.0, t::R_PANEL),
        text_color: Some(t::TEXT),
        ..Default::default()
    }
}

fn panel<'a>(c: impl Into<Element<'a, Msg>>) -> container::Container<'a, Msg> {
    container(c).style(panel_style)
}

/// `c` aligned inside a page-sized, click-through frame.
fn placed<'a>(
    c: impl Into<Element<'a, Msg>>,
    h: iced::alignment::Horizontal,
    v: iced::alignment::Vertical,
    pad: Padding,
) -> Element<'a, Msg> {
    container(c)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(h)
        .align_y(v)
        .padding(pad)
        .into()
}

use iced::alignment::{Horizontal as H, Vertical as V};

fn row_look(s: Status, on: bool, kb: bool) -> Style {
    let hot = matches!(s, Status::Hovered | Status::Pressed);
    let bg = if on {
        Some(t::ACCENT)
    } else if kb || hot {
        Some(t::GLASS)
    } else {
        None
    };
    Style {
        background: bg.map(Background::Color),
        text_color: if on { t::ON_ACCENT } else { t::TEXT },
        border: border(Color::TRANSPARENT, 0.0, 6.0),
        shadow: Shadow::default(),
        snap: false,
    }
}

fn step_btn<'a>(glyph: &'static str, a: MenuAct) -> Element<'a, Msg> {
    Press::new(container(text(glyph).size(13)).center(Length::Fill))
        .width(24)
        .height(24)
        .on_press(Msg::Act(Act::Menu(a)))
        .style(|s| {
            let hot = matches!(s, Status::Hovered | Status::Pressed);
            Style {
                background: hot.then_some(Background::Color(t::GLASS)),
                text_color: t::TEXT,
                border: border(t::EDGE, 1.0, 4.0),
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .into()
}

/// A menu (subtitles, audio, settings and its sub-menus), anchored above the bar at the right.
pub fn popover(p: &Player, pop: video::Pop) -> Element<'_, Msg> {
    let rows = video::items(pop, &p.tracks, &p.set);
    let mut col =
        column![container(mono(pop.title().to_uppercase())).padding([t::S2, t::S3])].spacing(2);
    for (i, it) in rows.into_iter().enumerate() {
        if it.head {
            col = col.push(container(mono(it.label.to_uppercase())).padding([t::S3, t::S3]));
            continue;
        }
        let kb = i == p.pop_sel;
        let value_ink = if it.on {
            t::ON_ACCENT
        } else {
            t::TEXT_SECONDARY
        };
        let label = text(it.label.clone())
            .size(13)
            .wrapping(text::Wrapping::None);
        let el: Element<Msg> = match it.dec.clone() {
            None => Press::new(
                row![
                    container(label).width(Length::Fill).clip(true),
                    text(it.value.clone()).size(13).color(value_ink)
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([t::S2, t::S3])
            .on_press(Msg::Act(Act::Menu(it.act.clone())))
            .style(move |s| row_look(s, it.on, kb))
            .into(),
            Some(dec) => container(
                row![
                    container(label).width(Length::Fill).clip(true),
                    step_btn("\u{2212}", dec),
                    container(text(it.value.clone()).size(13).color(t::TEXT_SECONDARY))
                        .width(44)
                        .align_x(H::Center),
                    step_btn("+", it.act.clone()),
                ]
                .spacing(t::S2)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([t::S2, t::S3])
            .style(move |_| container::Style {
                background: kb.then_some(Background::Color(t::GLASS)),
                border: border(Color::TRANSPARENT, 0.0, 6.0),
                ..Default::default()
            })
            .into(),
        };
        col = col.push(el);
    }
    let body = panel(scrollable(col.padding(t::S2)).style(crate::style::scroll))
        .width(360)
        .max_height(420);
    placed(
        opaque(body),
        H::Right,
        V::Bottom,
        Padding {
            right: t::S6,
            bottom: BAR_H + 6.0,
            ..Padding::ZERO
        },
    )
}

fn small_chip<'a>(label: &str, on: bool, msg: Msg) -> Element<'a, Msg> {
    let ink = if on {
        t::ACCENT_TEXT
    } else {
        t::TEXT_SECONDARY
    };
    Press::new(text(label.to_string()).size(t::CAPTION).color(ink))
        .padding([3.0, t::S2])
        .on_press(msg)
        .style(move |s| {
            let hot = matches!(s, Status::Hovered | Status::Pressed);
            Style {
                background: None,
                text_color: ink,
                border: border(if on || hot { t::ACCENT } else { t::EDGE }, 1.0, 6.0),
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .into()
}

pub fn queue_panel(p: &Player, shown: bool) -> Option<Element<'_, Msg>> {
    if !p.queue_open {
        return None;
    }
    let bottom = if shown { BAR_H + 8.0 } else { 8.0 };
    let head = row![
        mono(format!("QUEUE  {}", p.queue.len())),
        space().width(Length::Fill),
        small_chip(
            if p.shuffle {
                "Shuffle: on"
            } else {
                "Shuffle: off"
            },
            p.shuffle,
            Msg::Act(Act::Shuffle)
        ),
        small_chip(
            super::controls::repeat_label(p.set.repeat),
            p.set.repeat != Repeat::Off,
            Msg::Act(Act::Repeat)
        ),
    ]
    .spacing(t::S2)
    .align_y(Alignment::Center);
    let mut col = column![
        container(head).padding([t::S2, t::S3]),
        container(mono("Enter play  [ ] move  X remove  Z shuffle  R repeat"))
            .padding([0.0, t::S3]),
    ]
    .spacing(2);
    for (i, m) in p.queue.iter().enumerate() {
        let (cur, kb) = (i == p.qi, i == p.queue_sel);
        let n = m.short_ep();
        let ink = if cur { t::ON_ACCENT } else { t::TEXT };
        col = col.push(
            Press::new(
                row![
                    container(mono(n).color(ink)).width(48),
                    container(
                        text(m.title.clone())
                            .size(13)
                            .wrapping(text::Wrapping::None)
                    )
                    .width(Length::Fill)
                    .clip(true),
                ]
                .spacing(t::S3),
            )
            .width(Length::Fill)
            .padding([t::S2, t::S3])
            .on_press(Msg::Act(Act::Jump(i)))
            .style(move |s| row_look(s, cur, kb)),
        );
    }
    let body = panel(scrollable(col.padding(t::S2)).style(crate::style::scroll))
        .width(360)
        .height(Length::Fill);
    Some(placed(
        opaque(body),
        H::Right,
        V::Top,
        Padding {
            top: t::S6,
            right: t::S6,
            bottom,
            left: 0.0,
        },
    ))
}

/// Rows of the right-click menu.
pub fn ctx_rows(p: &Player) -> Vec<(&'static str, CtxAct)> {
    use CtxAct::{Key, Pop};
    let mut v = vec![
        (
            if p.paused() { "Play" } else { "Pause" },
            Key(video::Key::Toggle),
        ),
        ("Next", Key(video::Key::Next)),
        ("Previous", Key(video::Key::Prev)),
        ("Queue", Key(video::Key::Queue)),
        ("Subtitles", Pop(video::Pop::Subs)),
        ("Audio", Pop(video::Pop::Audio)),
        ("Settings", Pop(video::Pop::Settings)),
        ("Fullscreen", Key(video::Key::Fullscreen)),
        ("Mini player", Key(video::Key::Mini)),
        ("Screenshot", Key(video::Key::Screenshot)),
        ("Playback info", Key(video::Key::Stats)),
        ("Stop", Key(video::Key::Stop)),
    ];
    if p.can_favorite() {
        v.insert(
            4,
            (
                if p.fav {
                    "Remove favourite"
                } else {
                    "Add favourite"
                },
                Key(video::Key::Favorite),
            ),
        );
    }
    v
}

pub fn ctx_panel(p: &Player) -> Option<Element<'_, Msg>> {
    let at = p.ctx_menu?;
    let rows = ctx_rows(p);
    let h = rows.len() as f32 * 35.0 + 16.0;
    let o = logic::menu_origin(at, p.win, 220.0, h);
    let mut col = column![].spacing(2).padding(t::S2);
    for (i, (label, a)) in rows.into_iter().enumerate() {
        let kb = i == p.pop_sel;
        col = col.push(
            Press::new(text(label).size(13))
                .width(Length::Fill)
                .padding([6.0, t::S3])
                .on_press(Msg::Act(Act::Ctx(a)))
                .style(move |s| row_look(s, false, kb)),
        );
    }
    Some(pin(opaque(panel(col).width(220))).x(o.x).y(o.y).into())
}

/// Top-left OSD chip, stats panel and key help over the video.
pub fn overlays(p: &Player) -> Vec<Element<'_, Msg>> {
    let mut v: Vec<Element<Msg>> = vec![];
    if let Some((s, _)) = &p.osd {
        v.push(placed(
            panel(text(s.clone()).size(13)).padding([t::S2, t::S4]),
            H::Left,
            V::Top,
            Padding::new(t::S6),
        ));
    }
    if p.stats {
        let mut col = column![].spacing(2);
        for (k, val) in &p.stats_rows {
            col = col.push(
                row![
                    container(text(*k).size(13).color(t::TEXT_SECONDARY)).width(130),
                    text(val.clone()).size(13)
                ]
                .spacing(t::S4),
            );
        }
        v.push(placed(
            panel(col).padding([t::S3, t::S4]),
            H::Left,
            V::Top,
            Padding {
                top: t::S6 + 48.0,
                left: t::S6,
                ..Padding::ZERO
            },
        ));
    }
    if p.help {
        const KEYS: [(&str, &str); 26] = [
            ("Space / click", "Play / pause"),
            ("Left / Right", "Seek 10 s"),
            ("Up / Down", "Volume"),
            ("m", "Mute"),
            ("f / double-click", "Fullscreen"),
            ("Esc", "Close menu, cancel autoplay, leave fullscreen, stop"),
            ("Enter", "Skip intro / play next now"),
            ("n  p", "Next / previous item"),
            ("u", "Queue"),
            ("[ ]  \\", "Speed down / up / reset"),
            (", .", "Frame back / forward"),
            ("PgUp / PgDn", "Previous / next chapter"),
            ("t  v  o  q", "Subtitles / audio / settings / quality menus"),
            ("c", "Cycle subtitles"),
            ("Shift+C", "Cycle the secondary subtitle"),
            ("z x", "Subtitle delay -/+ 100 ms"),
            ("- =", "Audio delay -/+ 100 ms"),
            ("a", "A-B loop"),
            ("l", "Repeat one"),
            ("h", "Favourite"),
            ("i", "Playback info"),
            ("Shift+M", "Mini player"),
            ("Right-click", "Context menu"),
            ("Ctrl+S", "Screenshot"),
            ("s", "Stop (everyone, in a group)"),
            ("?", "This help"),
        ];
        let mut col = column![].spacing(2);
        for (k, d) in KEYS {
            col = col.push(
                row![
                    container(text(k).font(t::MONO).size(13).color(t::ACCENT_TEXT)).width(130),
                    text(d).size(13)
                ]
                .spacing(t::S4),
            );
        }
        v.push(placed(
            panel(col).padding([t::S3, t::S4]),
            H::Right,
            V::Top,
            Padding {
                top: t::S6 + 48.0,
                right: t::S6,
                ..Padding::ZERO
            },
        ));
    }
    if let Some((n, _)) = &p.note {
        v.push(placed(
            panel(text(n.clone()).size(13))
                .padding([t::S3, t::S4])
                .max_width(360),
            H::Right,
            V::Top,
            Padding::new(t::S6),
        ));
    }
    v
}

/// Prompts above the bar: skip chip, next-episode card.
pub fn floaters(p: &Player, shown: bool) -> Vec<Element<'_, Msg>> {
    let mut v: Vec<Element<Msg>> = vec![];
    let bottom = if shown { BAR_H + 16.0 } else { 24.0 };
    let pad = |extra: f32| Padding {
        right: t::S6,
        bottom: bottom + extra,
        ..Padding::ZERO
    };
    if let Some(s) = p.active_seg() {
        v.push(placed(
            container(chip_button(
                &format!("{}  \u{21B5}", s.label()),
                true,
                Msg::Act(Act::Skip),
            )),
            H::Right,
            V::Bottom,
            pad(if p.next_card().is_some() { 150.0 } else { 0.0 }),
        ));
    }
    if let Some((title, left)) = p.next_card() {
        let label = match left {
            Some(n) => format!("Next episode in {n}"),
            None => "Next episode".into(),
        };
        let body = panel(
            column![
                mono(label.to_uppercase()),
                text(title)
                    .size(13)
                    .font(t::SEMIBOLD)
                    .wrapping(text::Wrapping::None),
                row![
                    chip_button("Play now", true, Msg::Act(Act::NextPlay)),
                    chip_button("Cancel", false, Msg::Act(Act::NextCancel)),
                ]
                .spacing(t::S2),
            ]
            .spacing(t::S2),
        )
        .padding(t::S3)
        .width(300);
        v.push(placed(opaque(body), H::Right, V::Bottom, pad(0.0)));
    }
    v
}

pub fn resume_prompt(p: &Player) -> Option<Element<'_, Msg>> {
    let r = p.resume_prompt?;
    let body = panel(
        column![
            text(p.media.title.clone())
                .size(t::BODY_LG)
                .font(t::SEMIBOLD),
            row![
                chip_button(
                    &format!("Resume from {}", fmt_clock(r)),
                    true,
                    Msg::Act(Act::Resume(true))
                ),
                chip_button("Start over", false, Msg::Act(Act::Resume(false))),
            ]
            .spacing(t::S3),
            mono("Enter to resume, Esc to start over"),
        ]
        .spacing(t::S4)
        .align_x(Alignment::Center),
    )
    .padding(t::S6);
    Some(placed(opaque(body), H::Center, V::Center, Padding::ZERO))
}

/// The HDR question (docs/HDR.md): same calm panel as the resume prompt.
pub fn hdr_prompt(p: &Player) -> Option<Element<'_, Msg>> {
    let q = p.hdr_prompt?;
    let body = panel(
        column![
            text(hdr::PROMPT_TITLE).size(t::BODY_LG).font(t::SEMIBOLD),
            text(hdr::PROMPT_BODY)
                .size(t::BODY)
                .color(t::TEXT_SECONDARY)
                .width(460),
            if q.capable {
                Element::from(space())
            } else {
                text(hdr::PROMPT_NO_DISPLAY)
                    .size(t::BODY)
                    .color(t::ACCENT_TEXT)
                    .width(460)
                    .into()
            },
            row![
                chip_button("Play here in SDR", q.sel == 0, Msg::Act(Act::Hdr(false))),
                chip_button(
                    "Open in HDR pop-out window",
                    q.sel == 1,
                    Msg::Act(Act::Hdr(true))
                ),
            ]
            .spacing(t::S3),
            Press::new(
                text(format!(
                    "{}  Remember my choice",
                    if q.remember { "[x]" } else { "[ ]" }
                ))
                .size(t::BODY)
                .font(t::MONO)
            )
            .padding([t::S1, t::S2])
            .on_press(Msg::Act(Act::HdrRemember))
            .style(|s| {
                let hot = matches!(s, Status::Hovered | Status::Pressed);
                Style {
                    background: hot.then_some(Background::Color(t::GLASS)),
                    text_color: t::TEXT,
                    border: border(Color::TRANSPARENT, 0.0, 6.0),
                    shadow: Shadow::default(),
                    snap: false,
                }
            }),
            mono("Left / Right choose, Enter confirm, R remember, Esc plays here"),
        ]
        .spacing(t::S4)
        .align_x(Alignment::Center),
    )
    .padding(t::S6);
    Some(placed(opaque(body), H::Center, V::Center, Padding::ZERO))
}

/// Calm full-page message when there is no way to play (no mpv).
pub fn problem(p: &Player) -> Option<Element<'_, Msg>> {
    let msg = p.problem.as_ref()?;
    let body = panel(
        column![
            text("Cannot play right now")
                .size(t::BODY_LG)
                .font(t::SEMIBOLD),
            text(msg.clone())
                .size(t::BODY)
                .color(t::TEXT_SECONDARY)
                .width(520),
            chip_button("Back", true, Msg::Act(Act::Stop)),
        ]
        .spacing(t::S4)
        .align_x(Alignment::Center),
    )
    .padding(t::S6);
    Some(placed(opaque(body), H::Center, V::Center, Padding::ZERO))
}

/// Scrub tooltip: trickplay thumbnail when we have it, else just the time and chapter.
pub fn scrub_tip(p: &Player) -> Option<Element<'_, Msg>> {
    let (f, d) = p.hover_frac.zip(p.dur())?;
    let secs = f as f64 * d;
    let mut text_s = fmt_clock(secs);
    if let Some(i) = video::chapter_at(&p.chapters, secs)
        && !p.chapters[i].1.is_empty()
    {
        text_s = format!("{text_s} \u{00B7} {}", p.chapters[i].1);
    }
    let label = container(mono(text_s).color(t::TEXT))
        .padding([3.0, t::S2])
        .style(crate::style::tip);
    let (tx, tw) = track_rect(p.win.width);
    let x = tx + tw * f;
    let mut col = column![].spacing(4).align_x(Alignment::Center);
    let (mut w, mut h) = (60.0, 22.0);
    if let Some((_, info)) = &p.trick
        && let Some(tile) = pl::tile_at(info, secs)
        && let Some(Sheet::Ready(handle)) = p.sheets.get(&tile.sheet)
    {
        let k = 200.0 / info.width as f32;
        let (iw, ih) = (info.width as f32 * k, info.height as f32 * k);
        let pic = image(handle.clone())
            .crop(Rectangle {
                x: tile.x,
                y: tile.y,
                width: info.width,
                height: info.height,
            })
            .width(iw)
            .height(ih);
        col = col.push(container(pic).padding(1).style(|_| container::Style {
            border: border(t::HAIRLINE_STRONG, 1.0, 4.0),
            ..Default::default()
        }));
        (w, h) = (iw + 2.0, ih + 2.0 + 4.0 + 22.0);
    }
    col = col.push(label);
    Some(
        pin(col)
            .x(logic::tip_left(x, w, p.win.width))
            .y((p.win.height - BAR_H - 4.0 - h).max(0.0))
            .into(),
    )
}
