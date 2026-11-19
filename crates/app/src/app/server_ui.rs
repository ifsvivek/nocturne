//! Server UI glue: the "Use the UI of" choice (Settings > Sources), the read of the servers' layouts
//! and branding, and the home-sections editor. The rules are in `nocturne_core::server_ui`;
//! the layout itself is `Home::layout` in `home.rs`. The editor writes through `nocturne-jellyfin-user`
//! and only with `allow_server_writes` on and never with a developer token.
use super::message::{Message, SrcAct};
use super::{App, run};
use crate::style;
use crate::theme as t;
use crate::widget::{action_button, mono};
use iced::widget::{column, container, opaque, row, space, text};
use iced::{Alignment, Element, Length, Task};
use nocturne_core::backend::Backend;
use nocturne_core::catalog::Status;
use nocturne_core::profile::UiFrom;
use nocturne_core::server_ui::{Avail, Editor, Pick, ServerUi, effective, pick, wanted};
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
    /// What the app knows about a server whose UI might be used (feeds `server_ui::pick`).
    fn ui_avail(&self, id: SourceId) -> Avail {
        match self.catalog.slot(id).map(|s| &s.status) {
            Some(Status::SignedOut) => Avail::SignedOut,
            Some(Status::Offline { .. }) => Avail::Unreachable,
            _ if self.server_ui.contains_key(&id) => Avail::Ready,
            _ => match self.ui_state.get(&id) {
                Some(false) => Avail::Unreachable,
                _ => Avail::Loading,
            },
        }
    }

    /// Whose UI applies, for the source in view (or "All") and, for "All" with each item's own UI,
    /// the server of the item at hand. The developer token has no user, so no server UI.
    pub(super) fn ui_pick(&self, item_server: Option<SourceId>) -> Pick {
        let ctx = match self.filter {
            SourceFilter::One(id) => Some(id),
            SourceFilter::All => None,
        };
        self.ui_pick_in(ctx, item_server)
    }

    fn ui_pick_in(&self, ctx: Option<SourceId>, item_server: Option<SourceId>) -> Pick {
        if self.dev {
            return Pick::default();
        }
        pick(&self.sources, ctx, item_server, &|i| self.ui_avail(i))
    }

    /// May the home-sections of `id` be edited (from Settings > Sources): browsed with its own UI.
    pub(super) fn sections_editable(&self, id: SourceId) -> bool {
        self.ui_pick_in(Some(id), None).editable() == Some(id)
    }

    /// The layout to use on Home (the item-less case: "All" with each item's own UI shows NOCTURNE's).
    pub(super) fn server_ui_now(&self) -> Option<ServerUi> {
        effective(&self.ui_pick(None), &self.server_ui)
    }

    /// Why the NOCTURNE layout shows although another UI was chosen for `ctx`, as one line.
    pub(super) fn ui_note(&self, ctx: Option<SourceId>) -> Option<String> {
        let p = self.ui_pick_in(ctx, None);
        let label = |i: SourceId| {
            self.sources
                .get(i)
                .map_or("A server".into(), |p| p.label.clone())
        };
        p.note.map(|n| n.message(&label))
    }

    /// Load every wanted server's UI that is online and not yet loaded (three small GETs each), and
    /// apply what changed to Home. Cheap when nothing changed; called after every message.
    pub(super) fn sync_ui(&mut self) -> Task<Message> {
        let mut tasks = vec![];
        if !self.dev {
            for id in wanted(&self.sources) {
                let online = self
                    .catalog
                    .slot(id)
                    .is_some_and(|s| s.status == Status::Online);
                if !online || self.server_ui.contains_key(&id) || self.ui_state.contains_key(&id) {
                    continue;
                }
                let Some(b @ Backend::Jellyfin(_)) = self.catalog.backend(id).cloned() else {
                    continue;
                };
                self.ui_state.insert(id, true);
                tasks.push(run(async move { b.server_ui().await.ok() }, move |r| {
                    Message::ServerUi(id, r)
                }));
            }
        }
        let key = (self.ui_pick(None), self.ui_gen);
        if self.ui_applied != Some(key) {
            self.ui_applied = Some(key);
            let ui = self.server_ui_now();
            for h in self.stack.iter_mut().filter_map(|p| p.home.as_deref_mut()) {
                h.server = ui.clone();
                h.row = h.row.min(h.layout().len() - 1);
            }
        }
        Task::batch(tasks)
    }

    /// A connect or reconnect forgets an earlier failure, so the next sync loads again.
    pub(super) fn retry_server_ui(&mut self, id: SourceId) {
        if self.ui_state.get(&id) == Some(&false) {
            self.ui_state.remove(&id);
        }
    }

    /// An answer: cached per source, its branding becomes that server's theme layer. Whether it is
    /// used is `ui_pick`'s call, so a choice that changed meanwhile needs no special case.
    pub(super) fn server_ui_answer(&mut self, id: SourceId, r: Option<ServerUi>) {
        let Some(ui) = r else {
            self.ui_state.insert(id, false);
            return;
        };
        self.ui_state.remove(&id);
        self.themes.set_branding(id, ui.css.clone());
        self.server_ui.insert(id, ui);
        self.ui_gen += 1;
    }

    /// Settings > Sources "Use the UI of": the next choice in `ui_choices`, saved with the sources.
    pub(super) fn cycle_ui(&mut self, ctx: Option<SourceId>) {
        let choices = self.sources.ui_choices(ctx);
        let at = choices
            .iter()
            .position(|c| *c == self.sources.ui_of(ctx))
            .unwrap_or(0);
        let next = choices[(at + 1) % choices.len()];
        if self.sources.set_ui_from(ctx, next) {
            if let Some(id) = ctx {
                let on = self.sources.get(id).is_some_and(|p| p.server_ui);
                if let Some(s) = self.catalog.slot_mut(id) {
                    s.profile.server_ui = on;
                }
            }
            self.save_sources();
        }
    }

    /// A source was removed: forget its UI (the choices that pointed at it were reset by
    /// `Sources::remove`, so the next `ui_pick` is the NOCTURNE layout).
    pub(super) fn forget_server_ui(&mut self, id: SourceId) {
        self.server_ui.remove(&id);
        self.ui_state.remove(&id);
        self.themes.set_branding(id, None);
        self.ui_gen += 1;
    }

    /// The label of one choice, for the button.
    pub(super) fn ui_choice_label(&self, ctx: Option<SourceId>, c: UiFrom) -> String {
        match c {
            UiFrom::Nocturne => "NOCTURNE".into(),
            UiFrom::SelfServer if ctx.is_none() => "Each item's server".into(),
            UiFrom::SelfServer => "This server".into(),
            UiFrom::Source(i) => self
                .sources
                .get(i)
                .map_or("Removed".into(), |p| p.label.clone()),
        }
    }

    pub(super) fn open_sections(&mut self, id: SourceId) {
        if !self.sections_editable(id) {
            return;
        }
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

    /// Only the server being browsed with its own UI is ever edited, and only with server changes on.
    fn can_edit_server(&self) -> bool {
        self.settings.allow_server_writes
            && !self.dev
            && self
                .sec
                .as_ref()
                .is_some_and(|d| self.sections_editable(d.id))
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
        self.server_ui.remove(&id);
        self.ui_state.remove(&id);
        self.ui_gen += 1;
        Task::none()
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
