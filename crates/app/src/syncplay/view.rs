//! Watch together: the overlay and the "watching together" pill (port of the GPUI
//! `sp_overlay` / `sp_pill`).
use super::{Link, Load, Msg, SyncPlay};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{action_button, mono, overlay, tip, tracked};
use iced::widget::button::Style;
use iced::widget::{column, container, row, space, text, text_input};
use iced::{Alignment, Element, Length, Shadow, Size};
use nocturne_core::syncplay::Gate;
use nocturne_jellyfin_user::syncplay::GroupInfo;

type El<'a> = Element<'a, Msg>;

pub fn pill(sp: &SyncPlay) -> Option<El<'_>> {
    let g = sp.engine.group()?;
    let status = match sp.link {
        Link::Down(_) => "reconnecting".to_string(),
        _ => format!("{} watching", g.participants.len()),
    };
    let label = format!("{} \u{00B7} {status}", g.group_name);
    let body = Press::new(
        row![
            container(space().width(6).height(6)).style(|_| iced::widget::container::Style {
                background: Some(t::accent().into()),
                border: border(t::NONE, 0.0, 3.0),
                ..Default::default()
            }),
            text(label)
                .size(t::CAPTION + 1.0)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(t::S2)
        .align_y(Alignment::Center),
    )
    .height(30)
    .padding([0.0, t::S2])
    .on_press(Msg::OpenScreen)
    .style(|_| Style {
        background: None,
        text_color: t::text(),
        border: border(t::NONE, 0.0, 0.0),
        shadow: Shadow::default(),
        snap: false,
    });
    Some(
        container(
            row![
                tip(body, "Watch together (G)"),
                action_button("Leave", false, false, false, false, Msg::Leave),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        )
        .max_width(380)
        .padding([0.0, t::S1])
        .style(|_| iced::widget::container::Style {
            background: Some(t::accent_tint().into()),
            border: border(t::accent(), 1.0, 6.0),
            ..Default::default()
        })
        .into(),
    )
}

fn state_word(g: &GroupInfo) -> String {
    format!("{:?}", g.state).to_lowercase()
}

pub fn screen(sp: &SyncPlay, win: Size) -> El<'_> {
    let gate = sp.gate();
    let mut body: Vec<El> = vec![
        text("Play, pause, seek and skip stay in step for everyone in the group.")
            .size(t::BODY)
            .color(t::text_secondary())
            .into(),
    ];
    if let Some(p) = server_row(sp) {
        body.push(p);
    }
    if let Some(r) = gate.reason() {
        let mut row = row![text(r).size(t::BODY).width(Length::Fill)]
            .spacing(t::S3)
            .align_y(Alignment::Center);
        if gate == Gate::NeedWrites {
            row = row.push(action_button(
                "Open Settings",
                true,
                false,
                false,
                false,
                Msg::OpenSettings,
            ));
        }
        body.push(
            container(row)
                .padding(t::S3)
                .style(|_| iced::widget::container::Style {
                    background: None,
                    border: border(t::hairline_strong(), 1.0, t::r_panel()),
                    ..Default::default()
                })
                .into(),
        );
    }
    body.push(match sp.engine.group() {
        Some(g) => in_group(sp, g),
        None => browse(sp, gate),
    });
    body.push(
        mono(if sp.engine.in_group() {
            "Esc closes  \u{00B7}  L leaves  \u{00B7}  Enter plays what the group is on"
        } else {
            "Esc closes  \u{00B7}  Tab switches list and name  \u{00B7}  Enter joins or creates  \u{00B7}  R reloads"
        })
        .into(),
    );
    overlay::panel("Watch together", 640.0, win, body, Msg::CloseScreen)
}

/// Which server the group lives on: named, with a chooser while several are signed in and no
/// group has been joined (a group's server cannot change).
fn server_row(sp: &SyncPlay) -> Option<El<'_>> {
    if sp.dev || sp.targets.is_empty() {
        return None;
    }
    let mut r = row![mono(tracked("Server"))]
        .spacing(t::S2)
        .align_y(Alignment::Center);
    if sp.targets.len() > 1 && !sp.engine.in_group() {
        for (i, t) in sp.targets.iter().enumerate() {
            r = r.push(action_button(
                &t.label,
                i == sp.chosen,
                false,
                false,
                false,
                Msg::Choose(i),
            ));
        }
        r = r.push(mono("[ ] switch"));
    } else if let Some(t) = sp.targets.get(sp.chosen) {
        r = r.push(text(t.label.clone()).size(t::BODY).font(t::semibold()));
    }
    Some(r.into())
}

fn in_group<'a>(sp: &'a SyncPlay, g: &GroupInfo) -> El<'a> {
    let link = match sp.link {
        Link::Up => "connected",
        Link::Down(_) => "reconnecting",
        _ => "connecting",
    };
    let mut btns = row![].spacing(t::S3);
    if !sp.following || !sp.in_player {
        btns = btns.push(action_button(
            "Play what the group is on",
            true,
            false,
            false,
            sp.engine.queue().is_none(),
            Msg::Follow,
        ));
    }
    btns = btns.push(action_button(
        "Leave group",
        false,
        false,
        false,
        false,
        Msg::Leave,
    ));
    column![
        text(g.group_name.clone())
            .size(t::HEADING)
            .font(t::semibold()),
        mono(format!(
            "{} \u{00B7} {link} \u{00B7} {} watching",
            state_word(g),
            g.participants.len()
        )),
        text(g.participants.join(", "))
            .size(t::BODY)
            .color(t::text_secondary()),
        text("Pick anything to play: it starts for everyone.")
            .size(t::BODY)
            .color(t::text_secondary()),
        btns,
    ]
    .spacing(t::S3)
    .into()
}

fn browse(sp: &SyncPlay, gate: Gate) -> El<'_> {
    let mut d = column![].spacing(t::S3);
    if !sp.dev {
        d = d.push(mono(tracked("Groups on this server")));
    }
    match &sp.groups {
        _ if sp.dev => {}
        Load::Loading => d = d.push(mono("Loading")),
        Load::Failed => d = d.push(mono("The groups could not be loaded. Press R.")),
        Load::Ready(v) if v.is_empty() => {
            d = d.push(mono(
                "No groups yet. Create one below and ask others to join.",
            ));
        }
        Load::Ready(v) => {
            for (i, g) in v.iter().enumerate() {
                let sel = i == sp.sel && !sp.name_focus;
                let id = g.group_id.clone();
                let line = row![
                    column![
                        text(g.group_name.clone()).size(t::BODY).font(t::semibold()),
                        mono(format!(
                            "{} \u{00B7} {}",
                            state_word(g),
                            g.participants.join(", ")
                        )),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    action_button(
                        "Join",
                        true,
                        false,
                        false,
                        !gate.can_join() || sp.busy,
                        Msg::Join(id)
                    ),
                ]
                .spacing(t::S4)
                .align_y(Alignment::Center);
                d = d.push(
                    Press::new(line)
                        .width(Length::Fill)
                        .padding([t::S2, t::S3])
                        .on_press(Msg::Select(i))
                        .style(move |_| Style {
                            background: sel.then_some(t::accent_tint().into()),
                            text_color: t::text(),
                            border: border(
                                if sel { t::accent() } else { t::hairline() },
                                1.0,
                                t::r_panel(),
                            ),
                            shadow: Shadow::default(),
                            snap: false,
                        }),
                );
            }
        }
    }
    if gate == Gate::Create || gate == Gate::NeedWrites {
        let field = text_input("Name a new group", &sp.name)
            .id(super::NAME_ID)
            .on_input(Msg::NameInput)
            .on_submit(Msg::Create)
            .size(t::BODY)
            .padding([t::S2, t::S3])
            .style(style::input);
        d = d.push(
            container(
                row![
                    field,
                    action_button(
                        "Create group",
                        true,
                        false,
                        false,
                        !gate.can_create() || sp.busy,
                        Msg::Create
                    ),
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            )
            .padding([t::S3, 0.0]),
        );
    }
    let _ = style::rule;
    d.into()
}
