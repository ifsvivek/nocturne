//! Account screens in the app's design language: centred panel on the grain/glow backdrop,
//! hairline fields, mono annotations, one primary action per screen.
use super::state::*;
use super::{Accounts, Dialog, DlgKind, Msg, Where};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{action_button, label, mono, pulse, tip, tracked, wordmark};
use iced::widget::button::Status;
use iced::widget::{column, container, opaque, row, space, text, text_input};
use iced::{Alignment, Color, Element, Length};
use std::time::Instant;

/// Where the README's getting-started section lives.
pub const DOCS_URL: &str = "https://github.com/ifsvivek/nocturne#readme";

type El<'a> = Element<'a, Msg>;

fn frame<'a>(content: impl Into<El<'a>>) -> El<'a> {
    container(
        column![
            wordmark(t::DISPLAY),
            label("Your media. Your night."),
            space().height(t::S4),
            content.into(),
        ]
        .spacing(t::S4)
        .align_x(Alignment::Center),
    )
    .center(Length::Fill)
    .into()
}

fn panel<'a>(w: f32, items: Vec<El<'a>>) -> El<'a> {
    container(column(items).spacing(t::S4))
        .width(w)
        .padding(t::S6)
        .style(style::panel)
        .into()
}

fn heading<'a>(eyebrow: &str, title: String, body: Option<String>) -> El<'a> {
    let mut c = column![
        mono(tracked(eyebrow)),
        text(title).size(t::HEADING).font(t::bold()),
    ]
    .spacing(t::S2);
    if let Some(b) = body {
        c = c.push(
            text(b)
                .size(t::BODY)
                .line_height(1.5)
                .color(t::text_secondary()),
        );
    }
    c.into()
}

/// Three short steps for a first launch.
fn steps<'a>() -> El<'a> {
    let line = |n: &'static str, s: &'static str| -> El<'a> {
        row![
            mono(n),
            text(s)
                .size(t::BODY)
                .color(t::text_secondary())
                .width(Length::Fill)
        ]
        .spacing(t::S3)
        .into()
    };
    column![
        line(
            "01",
            "Type your server's address, like http://192.168.1.10:8096."
        ),
        line(
            "02",
            "Sign in with your account, or approve with Quick Connect."
        ),
        line("03", "Pick something and press play."),
    ]
    .spacing(t::S1)
    .into()
}

fn note<'a>(msg: Option<&'static str>) -> Option<El<'a>> {
    msg.map(|m| {
        text(m)
            .size(t::BODY)
            .line_height(1.4)
            .color(t::warn())
            .into()
    })
}

fn rule<'a>() -> El<'a> {
    crate::widget::rule()
}

fn field<'a>(f: &Fld, w: Where) -> El<'a> {
    let input = text_input("", &f.value)
        .id(w.id())
        .on_input(move |s| Msg::Input(w, s))
        .on_submit(Msg::Submit(w))
        .secure(f.secret)
        .font(t::MONO)
        .size(t::BODY)
        .padding([t::S3, t::S3])
        .style(style::input);
    column![mono(f.label), input].spacing(t::S1).into()
}

fn pulsing<'a>(s: &'static str, now: Instant, period: u128) -> El<'a> {
    mono(s)
        .color(Color {
            a: pulse(now, period),
            ..t::text_tertiary()
        })
        .into()
}

fn connect_screen(a: &Accounts, now: Instant) -> El<'_> {
    let st = &a.st;
    let form = &st.connect;
    let mut items: Vec<El> = vec![
        heading(
            "Connect",
            "Where is your media?".into(),
            Some("Enter the address of your server.".into()),
        ),
        steps(),
        field(&form.fields[0], Where::Connect),
    ];
    items.extend(note(st.msg));
    let mut go = row![action_button(
        "Connect",
        true,
        false,
        form.focus == 1,
        st.busy,
        Msg::ConnectGo,
    )]
    .spacing(t::S3)
    .align_y(Alignment::Center);
    if st.busy {
        go = go.push(pulsing("CHECKING", now, 1200));
    }
    items.push(go.into());
    if !st.servers.is_empty() {
        items.push(rule());
        items.push(mono(tracked("Remembered servers")).into());
        for (i, s) in st.servers.iter().enumerate() {
            let on = form.focus == form.fields.len() + 1 + i;
            let forget = tip(
                Press::new(text("Forget").size(t::CAPTION + 1.0))
                    .padding([0.0, t::S2])
                    .on_press(Msg::Forget(i))
                    .style(|s| iced::widget::button::Style {
                        text_color: if matches!(s, Status::Hovered | Status::Pressed) {
                            t::danger()
                        } else {
                            t::text_secondary()
                        },
                        ..Default::default()
                    }),
                "Forget this server and its saved sign-in (Delete)",
            );
            items.push(
                Press::new(
                    row![
                        mono(s.clone()).color(t::text()),
                        space().width(Length::Fill),
                        forget
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .height(36)
                .padding([0.0, t::S3])
                .on_press(Msg::Pick(i))
                .style(move |st| iced::widget::button::Style {
                    background: if on {
                        Some(t::accent_tint().into())
                    } else if matches!(st, Status::Hovered) {
                        Some(t::GLASS.into())
                    } else {
                        None
                    },
                    text_color: t::text(),
                    border: border(if on { t::accent() } else { t::edge() }, 1.0, 6.0),
                    ..Default::default()
                })
                .into(),
            );
        }
    }
    items.push(rule());
    items.push(tip(
        Press::new(text("New here? Read the setup guide in the README.").size(t::CAPTION + 1.0))
            .on_press(Msg::OpenDocs)
            .style(|s| iced::widget::button::Style {
                text_color: if matches!(s, Status::Hovered | Status::Pressed) {
                    t::accent_text()
                } else {
                    t::text_secondary()
                },
                ..Default::default()
            }),
        "Open the setup guide in your browser",
    ));
    frame(panel(440.0, items))
}

fn login_screen(a: &Accounts, now: Instant) -> El<'_> {
    let st = &a.st;
    if let Qc::Waiting(code) = &st.qc {
        return frame(qc_panel(Some(code.clone()), now));
    }
    if st.qc == Qc::Starting {
        return frame(qc_panel(None, now));
    }
    let f = &st.login;
    let mut items: Vec<El> = vec![
        heading("Sign in", st.server_name.clone(), Some(st.url.clone())),
        field(&f.fields[0], Where::Login(0)),
        field(&f.fields[1], Where::Login(1)),
    ];
    items.extend(note(st.msg));
    let mut r = row![action_button(
        "Sign in",
        true,
        false,
        f.focus == 2,
        st.busy,
        Msg::LoginGo
    )]
    .spacing(t::S2);
    if st.qc_available {
        r = r.push(action_button(
            "Quick Connect",
            false,
            false,
            f.focus == 3,
            st.busy,
            Msg::QcGo,
        ));
    }
    r = r.push(action_button(
        "Change server",
        false,
        false,
        f.focus == 2 + f.buttons - 1,
        false,
        Msg::BackToServers,
    ));
    items.push(r.into());
    if st.keyring_down {
        items.push(mono(KEYRING_NOTE).into());
    }
    frame(panel(440.0, items))
}

fn qc_panel<'a>(code: Option<String>, now: Instant) -> El<'a> {
    let big = container(
        text(code.clone().unwrap_or_else(|| "\u{2026}".into()))
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..t::MONO
            })
            .size(48),
    )
    .center_x(Length::Fill)
    .center_y(88)
    .style(|_| iced::widget::container::Style {
        background: Some(t::accent_tint().into()),
        border: border(t::accent(), 1.0, 6.0),
        ..Default::default()
    });
    panel(
        440.0,
        vec![
            heading(
                "Quick Connect",
                "Enter this code".into(),
                Some(
                    "On a device where you're already signed in, open Quick Connect and type the code."
                        .into(),
                ),
            ),
            big.into(),
            pulsing(
                if code.is_some() {
                    "WAITING FOR APPROVAL"
                } else {
                    "GETTING A CODE"
                },
                now,
                1600,
            ),
            container(action_button("Cancel", false, false, true, false, Msg::QcStop))
                .align_right(Length::Fill)
                .into(),
        ],
    )
}

fn pw_dialog(d: &Dialog) -> El<'_> {
    let f = &d.form;
    let (title, body, ok_label) = match d.kind {
        DlgKind::Password => (
            "Change your password",
            "You'll stay signed in on this device.",
            "Change password",
        ),
        DlgKind::Authorize => (
            "Authorize a device",
            "On the other device choose Quick Connect, then type the code it shows. It signs in as you.",
            "Authorize",
        ),
        DlgKind::ProfileImage => (
            "Profile image",
            "Uploads a picture from this computer as your profile image on the server.",
            "Upload",
        ),
        DlgKind::RemoveImage => (
            "Remove profile image",
            "Your profile image is deleted from the server.",
            "Remove image",
        ),
        DlgKind::Language => (
            "Display language",
            "The language the server keeps for your account. Other Jellyfin apps you use read it too.",
            "Save",
        ),
    };
    let mut items: Vec<El> = vec![heading("Please confirm", title.into(), Some(body.into()))];
    for (i, fld) in f.fields.iter().enumerate() {
        items.push(field(fld, Where::Dialog(i)));
    }
    let (ci, oi) = (f.fields.len(), f.fields.len() + 1);
    items.extend(note(d.msg));
    items.push(
        container(
            row![
                action_button(
                    "Cancel",
                    false,
                    false,
                    f.focus == ci,
                    false,
                    Msg::DialogPress(0)
                ),
                action_button(
                    ok_label,
                    true,
                    false,
                    f.focus == oi,
                    d.busy,
                    Msg::DialogPress(1)
                ),
            ]
            .spacing(t::S2),
        )
        .align_right(Length::Fill)
        .into(),
    );
    opaque(
        container(panel(480.0, items))
            .center(Length::Fill)
            .style(style::scrim),
    )
}

pub fn dialog(a: &Accounts) -> Option<El<'_>> {
    a.dialog.as_ref().map(pw_dialog)
}

pub fn view(a: &Accounts, now: Instant) -> El<'_> {
    if let Some(d) = &a.dialog {
        return pw_dialog(d);
    }
    if a.st.restoring && a.st.busy {
        return frame(pulsing("RESTORING YOUR SESSION", now, 1600));
    }
    match a.st.stage {
        Stage::Connect => connect_screen(a, now),
        Stage::Login => login_screen(a, now),
        Stage::Active => space().into(),
    }
}
