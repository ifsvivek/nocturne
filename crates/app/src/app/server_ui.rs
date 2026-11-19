//! Server UI mode glue: the per-server switch (Settings > Sources), the read of a server's own
//! layout and branding, and the home-sections editor. The rules are in `nocturne_core::server_ui`;
//! the layout itself is `Home::layout` in `home.rs`. The editor writes through `nocturne-jellyfin-user`
//! and only with `allow_server_writes` on and never with a developer token.
use super::catalog::DEV;
use super::message::{Message, SrcAct};
use super::{App, run};
use crate::style;
use crate::theme as t;
use crate::widget::{action_button, mono};
use iced::widget::{column, container, opaque, row, space, text};
use iced::{Alignment, Element, Length, Task};
use nocturne_core::backend::Backend;
use nocturne_core::profile::ProfileKind;
use nocturne_core::server_ui::{Editor, ServerUi};
use nocturne_core::source::SourceId;
use nocturne_core::sources::SourceFilter;

type El<'a> = Element<'a, Message>;

/// The home-sections dialog of one server.
pub(super) struct SecDlg {
    id: SourceId,
    label: String,
    ed: Editor,
    busy: bool,
    err: Option<String>,
}

impl App {
    /// Server UI is on for this source (and possible: it is a signed-in server, not the developer
    /// token, which has no user).
    pub(super) fn wants_server_ui(&self, id: SourceId) -> bool {
        id != DEV
            && !self.dev
            && self.sources.get(id).is_some_and(|p| {
                p.server_ui && p.enabled && matches!(p.kind, ProfileKind::Jellyfin { .. })
            })
    }

    /// The cached layout of the server that alone is selected, if its mode is on.
    pub(super) fn server_ui_now(&self) -> Option<ServerUi> {
        match self.filter {
            SourceFilter::One(id) if self.wants_server_ui(id) => self.server_ui.get(&id).cloned(),
            _ => None,
        }
    }

    /// Read the server's layout and branding (three small GETs).
    pub(super) fn fetch_server_ui(&self, id: SourceId) -> Task<Message> {
        if !self.wants_server_ui(id) {
            return Task::none();
        }
        let Some(b @ Backend::Jellyfin(_)) = self.catalog.backend(id).cloned() else {
            return Task::none();
        };
        run(async move { b.server_ui().await.ok() }, move |r| {
            Message::ServerUi(id, r)
        })
    }

    /// An answer: cached per source, its branding becomes that server's theme layer, and Home
    /// takes the layout when that server is the one in view. Dropped if the mode was switched off
    /// while the request was out.
    pub(super) fn server_ui_answer(&mut self, id: SourceId, r: Option<ServerUi>) {
        let Some(ui) = r.filter(|_| self.wants_server_ui(id)) else {
            return;
        };
        self.themes.set_branding(id, ui.css.clone());
        if self.filter == SourceFilter::One(id) {
            for h in self.stack.iter_mut().filter_map(|p| p.home.as_deref_mut()) {
                h.server = Some(ui.clone());
                h.row = h.row.min(h.layout().len() - 1);
            }
        }
        self.server_ui.insert(id, ui);
    }

    /// Settings > Sources "Layout": NOCTURNE (default) or the server's own.
    pub(super) fn toggle_server_ui(&mut self, id: SourceId) -> Task<Message> {
        let Some(on) = self.sources.get(id).map(|p| !p.server_ui) else {
            return Task::none();
        };
        self.sources.set_server_ui(id, on);
        if let Some(s) = self.catalog.slot_mut(id) {
            s.profile.server_ui = on;
        }
        self.save_sources();
        if on {
            return self.fetch_server_ui(id);
        }
        self.server_ui.remove(&id);
        self.themes.set_branding(id, None);
        if self.filter == SourceFilter::One(id) {
            for h in self.stack.iter_mut().filter_map(|p| p.home.as_deref_mut()) {
                h.server = None;
                h.row = h.row.min(h.layout().len() - 1);
            }
        }
        Task::none()
    }

    pub(super) fn open_sections(&mut self, id: SourceId) {
        let Some(ui) = self.server_ui.get(&id) else {
            self.toast("The server's layout is not loaded yet.");
            return;
        };
        let label = self.sources.get(id).map(|p| p.label.clone());
        self.sec = Some(SecDlg {
            id,
            label: label.unwrap_or_default(),
            ed: Editor::new(&ui.sections),
            busy: false,
            err: None,
        });
    }

    fn can_edit_server(&self) -> bool {
        self.settings.allow_server_writes && !self.dev
    }

    /// Dialog buttons. Editing is gated again here, not only by the disabled buttons.
    pub(super) fn sections_act(&mut self, a: SrcAct) -> Task<Message> {
        let can = self.can_edit_server();
        let Some(d) = self.sec.as_mut().filter(|d| !d.busy) else {
            return Task::none();
        };
        match a {
            SrcAct::SecCancel => self.sec = None,
            SrcAct::SecToggle(i) if can => d.ed.toggle(i),
            SrcAct::SecShift(i, by) if can => d.ed.shift(i, by as isize),
            SrcAct::SecSave if can => {
                let id = d.id;
                let Some((url, s)) = self.sessions.get(&id).cloned() else {
                    d.err = Some("Sign in to this server first.".into());
                    return Task::none();
                };
                let Ok(c) = nocturne_jellyfin_user::UserClient::from_session(&url, &s) else {
                    d.err = Some("Couldn't reach this server.".into());
                    return Task::none();
                };
                let values = d.ed.values();
                d.busy = true;
                d.err = None;
                return run(
                    async move { c.set_home_sections(&values).await.is_ok() },
                    move |ok| Message::SectionsSaved(id, ok),
                );
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn sections_saved(&mut self, id: SourceId, ok: bool) -> Task<Message> {
        let Some(d) = self.sec.as_mut().filter(|d| d.id == id) else {
            return Task::none();
        };
        if !ok {
            d.busy = false;
            d.err = Some("The server did not accept the change.".into());
            return Task::none();
        }
        self.sec = None;
        self.toast("Saved on the server.");
        self.fetch_server_ui(id)
    }

    pub(super) fn sections_dialog(&self) -> El<'_> {
        let d = self.sec.as_ref().expect("dialog is open");
        let can = self.can_edit_server();
        let mut list = column![].spacing(t::S2);
        let n = d.ed.rows.len();
        for (i, (s, on)) in d.ed.rows.iter().enumerate() {
            let name = if s.native() {
                s.label().to_string()
            } else {
                format!("{} (not shown yet)", s.label())
            };
            let ink = if *on { t::text() } else { t::text_secondary() };
            let off = !can || d.busy;
            list = list.push(
                row![
                    text(name).size(t::BODY).color(ink).width(Length::Fill),
                    action_button(
                        if *on { "On" } else { "Off" },
                        *on,
                        false,
                        false,
                        off,
                        Message::Src(SrcAct::SecToggle(i))
                    ),
                    action_button(
                        "\u{2191}",
                        false,
                        false,
                        false,
                        off || i == 0,
                        Message::Src(SrcAct::SecShift(i, -1))
                    ),
                    action_button(
                        "\u{2193}",
                        false,
                        false,
                        false,
                        off || i + 1 == n,
                        Message::Src(SrcAct::SecShift(i, 1))
                    ),
                ]
                .spacing(t::S2)
                .align_y(Alignment::Center),
            );
        }
        let mut card = column![
            mono("HOME SECTIONS"),
            text(format!("Home of {}", d.label))
                .size(t::HEADING)
                .font(t::bold()),
            text("The sections and their order on the home page of this server, for your account. Saving changes your display settings on the server, so the server's own web page follows too.")
                .size(t::BODY)
                .line_height(1.5)
                .color(t::text_secondary()),
            list,
        ]
        .spacing(t::S3);
        if !can {
            card = card.push(
                text(self.writes_off())
                    .size(t::CONTROL_TEXT)
                    .color(t::text_secondary()),
            );
        }
        if let Some(e) = &d.err {
            card = card.push(text(e.clone()).size(t::CONTROL_TEXT).color(t::danger()));
        }
        card = card.push(space().height(t::S2)).push(
            container(
                row![
                    action_button(
                        if can { "Cancel" } else { "Close" },
                        false,
                        false,
                        true,
                        false,
                        Message::Src(SrcAct::SecCancel)
                    ),
                    action_button(
                        "Save",
                        true,
                        false,
                        false,
                        !can || d.busy,
                        Message::Src(SrcAct::SecSave)
                    ),
                ]
                .spacing(t::S2),
            )
            .align_right(Length::Fill),
        );
        opaque(
            container(
                container(card)
                    .width(520)
                    .padding(t::S6)
                    .style(style::panel),
            )
            .center(Length::Fill)
            .style(style::scrim),
        )
    }

    pub(super) fn sections_open(&self) -> bool {
        self.sec.is_some()
    }

    pub(super) fn close_sections(&mut self) {
        self.sec = None;
    }
}
