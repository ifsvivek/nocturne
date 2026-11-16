//! "Your devices" overlay: the sessions this account has open on one server (read only; port of
//! `ui_help.rs::devices_overlay`). The root opens it with the `UserClient` of the server the
//! account menu belongs to.
//!
//! ## Root hook
//!
//! ```ignore
//! AcctAct::Devices => { let (d, t) = devices::Devices::open(client, server_label); self.devices = Some(d); t.map(Message::Devices) }
//! Message::Devices(m) => if let Some(d) = &mut self.devices { if d.update(m) { self.devices = None } }
//! view  => self.devices.as_ref().map(|d| d.view(win).map(Message::Devices))   // above the page
//! key   => if d.key(&k) { self.devices = None }                                // owns the keyboard
//! ```
use crate::keys::Key;
use crate::task::run;
use crate::theme as t;
use crate::widget::{mono, overlay};
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Size, Task};
use nocturne_core::admin::fmt::when;
use nocturne_jellyfin_user::{OwnSession, UserClient};

#[derive(Clone)]
pub enum Msg {
    Loaded(Option<Vec<OwnSession>>),
    Close,
}

enum Load {
    Loading,
    Ready(Vec<OwnSession>),
    Failed,
}

pub struct Devices {
    load: Load,
    server: String,
}

impl Devices {
    pub fn open(client: UserClient, server: String) -> (Self, Task<Msg>) {
        (
            Self {
                load: Load::Loading,
                server,
            },
            run(async move { client.own_sessions().await.ok() }, Msg::Loaded),
        )
    }

    /// True when the overlay should close.
    pub fn update(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Loaded(r) => {
                self.load = r.map_or(Load::Failed, Load::Ready);
                false
            }
            Msg::Close => true,
        }
    }

    pub fn key(&self, k: &Key) -> bool {
        k.name == "escape"
    }

    pub fn view(&self, win: Size) -> Element<'_, Msg> {
        let mut body: Vec<Element<Msg>> = vec![
            text("Sessions this account has open on the server. To end one, sign out on that device.")
                .size(t::BODY)
                .color(t::TEXT_SECONDARY)
                .into(),
            mono(self.server.clone()).into(),
        ];
        match &self.load {
            Load::Loading => body.push(mono("Loading").into()),
            Load::Failed => body.push(mono("This couldn't be loaded. Close and try again.").into()),
            Load::Ready(v) => body.extend(v.iter().map(device_row)),
        }
        body.push(
            container(mono("Press Esc to close"))
                .padding([t::S2, 0.0])
                .into(),
        );
        overlay::panel("Your devices", 560.0, win, body, Msg::Close)
    }
}

fn device_row<'a>(s: &OwnSession) -> Element<'a, Msg> {
    let last = if s.last_activity_date.is_empty() {
        "now".to_string()
    } else {
        when(Some(&s.last_activity_date))
    };
    let mut r = row![
        column![
            text(format!("{} on {}", s.client, s.device_name))
                .size(t::BODY)
                .font(t::SEMIBOLD),
            mono(format!("{} \u{00B7} active {last}", s.application_version)),
        ]
        .spacing(2)
        .width(Length::Fill)
    ];
    if s.current {
        r = r.push(mono("this device"));
    }
    column![crate::widget::rule(), container(r).padding([t::S2, 0.0])].into()
}
