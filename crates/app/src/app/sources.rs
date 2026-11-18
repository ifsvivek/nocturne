//! Sources page (Settings > Sources): one row per source with its status and actions, the removal
//! dialog, and the quiet banner for a source that is offline or signed out. Port target of
//! `docs/SOURCES.md` "UI". Removing a server forgets only that profile's login; removing the local
//! library deletes only NOCTURNE's own index and state, never a media file.
use super::catalog::DEV;
use super::message::{Message, SrcAct};
use super::{AccountBack, App, Phase};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::{action_button, dot, icon, mono, rule, section, status_color};
use iced::widget::{column, container, opaque, row, scrollable as scroll, space, text, text_input};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::catalog::{Slot, Status};
use nocturne_core::profile::{ProfileKind, label_for};
use nocturne_core::source::SourceId;
use nocturne_core::sources::status_text;
use nocturne_core::sync::SyncHooks;
use std::sync::Arc;
use std::time::Instant;

type El<'a> = Element<'a, Message>;

/// The extra-header dialog (Sources > Header). The value may be a secret: masked, kept in the keyring.
pub(super) struct HeaderDlg {
    id: SourceId,
    url: String,
    name: String,
    value: String,
    err: Option<String>,
    busy: bool,
}

/// Width of the buttons column of a source row (sign in again, folders, rescan, disable, up, down, remove).
const ACTS_W: f32 = 560.0;

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl App {
    pub(super) fn open_sources(&mut self) -> Task<Message> {
        self.close_admin();
        self.src_open = true;
        Task::none()
    }

    pub(super) fn src_act(&mut self, a: SrcAct) -> Task<Message> {
        match a {
            SrcAct::Close => self.src_open = false,
            SrcAct::AddServer => {
                self.src_open = false;
                self.account_back = Some(AccountBack::Ready);
                self.phase = Phase::Account;
                let t = self.ensure_accounts();
                if let Some(a) = self.accounts.as_mut() {
                    a.switch_server();
                }
                return t;
            }
            SrcAct::AddFolder => return self.open_folders(None),
            SrcAct::SignIn(id) => return self.sign_in_again(id),
            SrcAct::Use(id) => return self.load_session(id),
            SrcAct::Rescan(id) if !self.scans.contains_key(&id) => return self.rescan_now(id),
            SrcAct::Rescan(_) => {}
            SrcAct::Folders(id) => return self.open_folders(Some(id)),
            SrcAct::Toggle(id) => return self.toggle_source(id),
            SrcAct::Move(id, by) => {
                if self.sources.shift(id, by) {
                    self.save_sources();
                    self.catalog.reorder(&self.sources.ids());
                    return self.retry_page();
                }
            }
            SrcAct::Header(id) => return self.open_header(id),
            SrcAct::HeaderCancel => self.hdr = None,
            SrcAct::HeaderClear => return self.save_header(true),
            SrcAct::HeaderSave => return self.save_header(false),
            SrcAct::AskRemove(id) => self.confirm = Some(id),
            SrcAct::CancelRemove => self.confirm = None,
            SrcAct::ConfirmRemove => {
                if let Some(id) = self.confirm.take() {
                    return self.remove_source(id);
                }
            }
        }
        Task::none()
    }

    fn open_header(&mut self, id: SourceId) -> Task<Message> {
        let Some(ProfileKind::Jellyfin { url, .. }) = self.sources.get(id).map(|p| p.kind.clone())
        else {
            return Task::none();
        };
        self.hdr = Some(HeaderDlg {
            id,
            url: url.clone(),
            name: String::new(),
            value: String::new(),
            err: None,
            busy: true,
        });
        super::run(
            async move {
                tokio::task::spawn_blocking(move || nocturne_core::profile::extra_header(&url))
                    .await
                    .ok()
                    .flatten()
            },
            move |l| Message::HeaderLoaded(id, l),
        )
    }

    pub(super) fn header_loaded(&mut self, id: SourceId, line: Option<String>) {
        if let Some(d) = self.hdr.as_mut().filter(|d| d.id == id) {
            d.busy = false;
            if let Some((n, v)) = line.as_deref().and_then(|l| l.split_once(':')) {
                d.name = n.trim().into();
                d.value = v.trim().into();
            }
        }
    }

    pub(super) fn header_input(&mut self, value: bool, s: String) {
        if let Some(d) = self.hdr.as_mut() {
            d.err = None;
            if value {
                d.value = s;
            } else {
                d.name = s;
            }
        }
    }

    fn save_header(&mut self, clear: bool) -> Task<Message> {
        let Some(d) = self.hdr.as_mut().filter(|d| !d.busy) else {
            return Task::none();
        };
        let (id, url) = (d.id, d.url.clone());
        let line = (!clear).then(|| {
            format!(
                "{}: {}",
                d.name.trim(),
                nocturne_core::secret::clean(&d.value)
            )
        });
        d.busy = true;
        super::run(
            async move {
                tokio::task::spawn_blocking(move || {
                    nocturne_core::profile::set_extra_header(&url, line.as_deref())
                })
                .await
                .unwrap_or_else(|_| Err("Couldn't save the header.".into()))
            },
            move |r| Message::HeaderDone(id, r),
        )
    }

    pub(super) fn header_done(&mut self, id: SourceId, r: Result<(), String>) -> Task<Message> {
        match r {
            Ok(()) => {
                self.hdr = None;
                self.toast("Header saved. Restart NOCTURNE so every part of the app uses it.");
                self.rebuild_slot(id)
            }
            Err(e) => {
                if let Some(d) = self.hdr.as_mut() {
                    d.busy = false;
                    d.err = Some(e);
                }
                Task::none()
            }
        }
    }

    pub(super) fn save_sources(&mut self) {
        // ponytail: a small atomic file write on the UI thread; a task if it ever shows in a trace.
        if self.sources.save().is_err() {
            self.toast("Couldn't save your sources.");
        }
    }

    /// The server's account state stops being the active one (disabled, removed).
    fn drop_active(&mut self, id: SourceId) {
        self.sessions.remove(&id);
        if self.active == Some(id) {
            self.active = None;
            self.admin = false;
            self.hooks = Arc::new(SyncHooks::new(None, false, false));
        }
    }

    fn toggle_source(&mut self, id: SourceId) -> Task<Message> {
        let Some(on) = self.sources.get(id).map(|p| !p.enabled) else {
            return Task::none();
        };
        self.sources.set_enabled(id, on);
        self.save_sources();
        if on {
            return self.rebuild_slot(id);
        }
        self.cancel_scan(id);
        self.drop_active(id);
        if let Some(s) = self.catalog.slot_mut(id) {
            s.profile.enabled = false;
            s.backend = None;
            s.status = Status::Disabled;
        }
        self.retry_page()
    }

    /// Forget a source. A server: its profile and that account's saved login. The local library: its
    /// index and state under the data directory, once any running scan has stopped. Media is untouched.
    pub(super) fn remove_source(&mut self, id: SourceId) -> Task<Message> {
        let Some(p) = self.sources.get(id).cloned() else {
            return Task::none();
        };
        let name = p.label.clone();
        match p.kind {
            ProfileKind::Jellyfin { url, user_id, .. } => self.spawn_forget(url, user_id),
            ProfileKind::Local { .. } => {
                self.cancel_scan(id);
                self.scans.remove(&id);
                self.rescan.remove(&id);
                if self.folders.as_ref().is_some_and(|f| f.id == id) {
                    self.folders = None;
                }
                if let Some(l) = self.catalog.backend(id).and_then(|b| b.local()).cloned() {
                    let data = self.data.clone();
                    nocturne_core::bridge::spawn(async move {
                        let _ = tokio::task::spawn_blocking(move || {
                            // Taking the library for writing waits for a stopping scan to finish.
                            l.with_mut(|_| ());
                            drop(l);
                            nocturne_core::catalog::drop_local_state(&data, id)
                        })
                        .await;
                    });
                } else {
                    let _ = nocturne_core::catalog::drop_local_state(&self.data, id);
                }
            }
        }
        self.sources.remove(id);
        self.save_sources();
        self.catalog.remove(id);
        self.drop_active(id);
        if self.sources.needs_welcome() && !self.dev {
            // Every source is gone: back to the first-run choice.
            self.stack.clear();
            self.src_open = false;
            self.settings_open = false;
            self.setup = None;
            let t = self.start_onboarding(crate::onboarding::Mode::Fresh);
            self.phase = Phase::Account;
            return t;
        }
        self.toast(format!("Removed {name}."));
        self.retry_page()
    }

    // ---- views ----

    fn source_row<'a>(&'a self, s: &'a Slot) -> El<'a> {
        let local = matches!(s.profile.kind, ProfileKind::Local { .. });
        let dev = s.id == DEV;
        let id = s.id;
        let account = match &s.profile.kind {
            ProfileKind::Jellyfin { .. } if dev => "Developer token, read only".to_string(),
            ProfileKind::Jellyfin { url, user_name, .. } => {
                format!("{user_name} on {}", label_for(url))
            }
            ProfileKind::Local { roots } => plural(roots.len(), "folder", "folders"),
        };
        let badge = container(icon(
            if local { "folder" } else { "server" },
            18.0,
            t::TEXT_SECONDARY,
        ))
        .center(38)
        .style(|_| container::Style {
            background: Some(t::GLASS.into()),
            border: border(t::HAIRLINE_STRONG, 1.0, 19.0),
            ..Default::default()
        });
        let who = column![
            text(s.profile.label.clone())
                .size(t::BODY)
                .font(t::SEMIBOLD),
            text(account).size(t::CONTROL_TEXT).color(t::TEXT_SECONDARY),
        ]
        .spacing(2)
        .width(Length::Fill);
        let status = container(
            row![
                dot(status_color(&s.status)),
                text(status_text(&s.status, Instant::now()))
                    .size(t::CONTROL_TEXT)
                    .color(t::TEXT_SECONDARY),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        )
        .width(200);
        let mut acts = row![].spacing(t::S2).align_y(Alignment::Center);
        let btn = |l: &str, danger: bool, off: bool, a: SrcAct| {
            action_button(l, false, danger, false, off, Message::Src(a))
        };
        if !dev {
            let pos = self.sources.ids().iter().position(|i| *i == id);
            let last = self.sources.profiles.len().saturating_sub(1);
            if s.status == Status::SignedOut {
                acts = acts.push(action_button(
                    "Sign in again",
                    true,
                    false,
                    false,
                    false,
                    Message::Src(SrcAct::SignIn(id)),
                ));
            }
            if local {
                acts = acts.push(btn("Folders", false, false, SrcAct::Folders(id)));
                if s.profile.enabled {
                    let busy = matches!(s.status, Status::Indexing { .. });
                    acts = acts.push(btn("Rescan", false, busy, SrcAct::Rescan(id)));
                }
            } else {
                if s.backend.is_some() && self.active != Some(id) {
                    acts = acts.push(btn("Use account", false, false, SrcAct::Use(id)));
                }
                acts = acts.push(btn("Header", false, false, SrcAct::Header(id)));
            }
            acts = acts
                .push(btn(
                    if s.profile.enabled {
                        "Disable"
                    } else {
                        "Enable"
                    },
                    false,
                    false,
                    SrcAct::Toggle(id),
                ))
                .push(btn("\u{2191}", false, pos == Some(0), SrcAct::Move(id, -1)))
                .push(btn(
                    "\u{2193}",
                    false,
                    pos == Some(last),
                    SrcAct::Move(id, 1),
                ))
                .push(btn("Remove", true, false, SrcAct::AskRemove(id)));
        }
        // Fixed-width columns: the status of every row lines up whatever buttons a row has.
        let acts = container(acts).width(ACTS_W).align_x(Alignment::End);
        row![badge, who, status, acts]
            .spacing(t::S4)
            .align_y(Alignment::Center)
            .padding([t::S3, t::S6])
            .into()
    }

    pub(super) fn sources_page(&self) -> El<'_> {
        let mut page = column![section(
            "Sources",
            "Where your media comes from. The order is the priority: a title found in more than one place is shown from the highest one that is available.",
            "Esc to go back".into(),
        )];
        page = page.push(
            container(
                row![
                    action_button(
                        "Add server",
                        true,
                        false,
                        false,
                        false,
                        Message::Src(SrcAct::AddServer)
                    ),
                    action_button(
                        "Add local folder",
                        false,
                        false,
                        false,
                        false,
                        Message::Src(SrcAct::AddFolder)
                    ),
                ]
                .spacing(t::S2),
            )
            .padding(Padding {
                top: t::S6,
                bottom: t::S4,
                left: t::PAGE_PAD,
                right: t::PAGE_PAD,
            }),
        );
        page = page.push(container(rule()).padding([0.0, t::PAGE_PAD]));
        if self.catalog.slots.is_empty() {
            page = page.push(
                container(
                    text("No sources yet. Add a server or a folder on this computer.")
                        .size(t::BODY)
                        .color(t::TEXT_SECONDARY),
                )
                .padding([t::S6, t::PAGE_PAD]),
            );
        }
        for s in &self.catalog.slots {
            page = page
                .push(container(column![self.source_row(s), rule()]).padding([0.0, t::PAGE_PAD]));
        }
        scroll(page.padding(Padding {
            top: t::HEADER_H + t::S8,
            bottom: t::S8,
            ..Padding::ZERO
        }))
        .style(style::scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// "Extra header" for a server behind a gating proxy: name and (masked) value.
    pub(super) fn header_dialog(&self) -> El<'_> {
        let d = self.hdr.as_ref().expect("dialog is open");
        let field = |ph: &str, v: &str, value: bool| {
            let i = text_input(ph, v)
                .on_input(move |s| Message::HeaderInput(value, s))
                .on_submit(Message::Src(SrcAct::HeaderSave))
                .size(t::BODY)
                .padding([t::S2, t::S3])
                .style(style::input);
            if value { i.secure(true) } else { i }
        };
        let mut card = column![
            mono("EXTRA HEADER"),
            text("Sent with every request to this server")
                .size(t::HEADING)
                .font(t::BOLD),
            text("For a server behind a gateway that asks for a header. The value is kept in your system keyring, never in a file. Leave both empty and press Remove to stop sending one.")
                .size(t::BODY)
                .line_height(1.5)
                .color(t::TEXT_SECONDARY),
            field("Header name, e.g. X-Access-Key", &d.name, false),
            field("Value", &d.value, true),
        ]
        .spacing(t::S3);
        if let Some(e) = &d.err {
            card = card.push(text(e.clone()).size(t::CONTROL_TEXT).color(t::DANGER));
        }
        let go = d.busy || d.name.trim().is_empty() || d.value.trim().is_empty();
        card = card.push(
            container(
                row![
                    action_button(
                        "Cancel",
                        false,
                        false,
                        true,
                        false,
                        Message::Src(SrcAct::HeaderCancel)
                    ),
                    action_button(
                        "Remove",
                        false,
                        true,
                        false,
                        d.busy,
                        Message::Src(SrcAct::HeaderClear)
                    ),
                    action_button(
                        "Save",
                        true,
                        false,
                        false,
                        go,
                        Message::Src(SrcAct::HeaderSave)
                    ),
                ]
                .spacing(t::S2),
            )
            .align_right(Length::Fill),
        );
        opaque(
            container(
                container(card)
                    .width(480)
                    .padding(t::S6)
                    .style(style::panel),
            )
            .center(Length::Fill)
            .style(style::scrim),
        )
    }

    /// "Remove this source?" Says what is and is not deleted.
    pub(super) fn confirm_dialog(&self, id: SourceId) -> El<'_> {
        let (title, body) = match self.sources.get(id) {
            Some(p) => match &p.kind {
                ProfileKind::Jellyfin {
                    url, user_name, ..
                } => (
                    format!("Remove {}?", p.label),
                    format!(
                        "This forgets the saved login of {user_name} on {} on this device and takes the server out of NOCTURNE. The server, its library and your account on it are not changed.",
                        label_for(url)
                    ),
                ),
                ProfileKind::Local { .. } => (
                    format!("Remove {}?", p.label),
                    "This deletes NOCTURNE's own index of your folders, your manual matches and your resume points for them. Your media files are never touched, moved or deleted.".to_string(),
                ),
            },
            None => ("Remove this source?".into(), String::new()),
        };
        let card = column![
            mono("PLEASE CONFIRM"),
            text(title).size(t::HEADING).font(t::BOLD),
            text(body)
                .size(t::BODY)
                .line_height(1.5)
                .color(t::TEXT_SECONDARY),
            space().height(t::S2),
            container(
                row![
                    action_button(
                        "Cancel",
                        false,
                        false,
                        true,
                        false,
                        Message::Src(SrcAct::CancelRemove)
                    ),
                    action_button(
                        "Remove",
                        false,
                        true,
                        false,
                        false,
                        Message::Src(SrcAct::ConfirmRemove)
                    ),
                ]
                .spacing(t::S2),
            )
            .align_right(Length::Fill),
        ]
        .spacing(t::S3);
        opaque(
            container(
                container(card)
                    .width(480)
                    .padding(t::S6)
                    .style(style::panel),
            )
            .center(Length::Fill)
            .style(style::scrim),
        )
    }

    /// A quiet strip under the header for the first source that is unreachable or signed out;
    /// the page underneath stays usable.
    pub(super) fn source_banner(&self) -> Option<El<'_>> {
        let now = Instant::now();
        let problem = |s: &&Slot| {
            matches!(s.status, Status::Offline { .. })
                || (s.status == Status::SignedOut && s.profile.enabled)
        };
        let s = self
            .catalog
            .slots
            .iter()
            .filter(problem)
            .min_by_key(|s| matches!(s.status, Status::SignedOut))?;
        let more = self.catalog.slots.iter().filter(problem).count() - 1;
        let name = &s.profile.label;
        let (msg, act): (String, El) = match &s.status {
            Status::Offline { retry_at, .. } => {
                let wait = retry_at.saturating_duration_since(now).as_secs() + 1;
                (
                    format!(
                        "Can't reach {name}. Showing what's already loaded. Retrying in {wait}s."
                    ),
                    action_button("Retry now", true, false, false, false, Message::RetryNow),
                )
            }
            _ if s.id == DEV => (format!("{name} was declined."), space().into()),
            _ => (
                format!("{name} needs you to sign in again."),
                action_button(
                    "Sign in",
                    true,
                    false,
                    false,
                    false,
                    Message::Src(SrcAct::SignIn(s.id)),
                ),
            ),
        };
        let msg = if more > 0 {
            format!("{msg} (+{more} more)")
        } else {
            msg
        };
        Some(
            container(
                container(
                    row![text(msg).size(t::BODY), act]
                        .spacing(t::S4)
                        .align_y(Alignment::Center),
                )
                .padding([t::S2, t::S4])
                .style(style::raised_card),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .padding(Padding {
                top: t::HEADER_H + t::S2,
                ..Padding::ZERO
            })
            .into(),
        )
    }

    /// "Update available": the exact command for a package-managed install, or a link to the release page.
    /// Shares the source banner's slot (a source problem wins) and is dismissible for the session.
    pub(super) fn update_banner(&self) -> Option<El<'_>> {
        use nocturne_core::update::{Action, UpdateState};
        let UpdateState::Available {
            version, action, ..
        } = self.update.as_ref()?
        else {
            return None;
        };
        if !self.settings.check_updates {
            return None;
        }
        let (msg, act): (String, El) = match action {
            Action::Command(cmd) => (
                format!("NOCTURNE {version} is available. Update with: {cmd}"),
                action_button(
                    "Copy command",
                    true,
                    false,
                    false,
                    false,
                    Message::UpdateCopy(cmd),
                ),
            ),
            Action::Download(url) => (
                format!("NOCTURNE {version} is available."),
                action_button(
                    "Download",
                    true,
                    false,
                    false,
                    false,
                    Message::UpdateOpen(url.clone()),
                ),
            ),
        };
        let dismiss = action_button(
            "Dismiss",
            false,
            false,
            false,
            false,
            Message::UpdateDismiss,
        );
        Some(
            container(
                container(
                    row![text(msg).size(t::BODY), act, dismiss]
                        .spacing(t::S4)
                        .align_y(Alignment::Center),
                )
                .padding([t::S2, t::S4])
                .style(style::raised_card),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .padding(Padding {
                top: t::HEADER_H + t::S2,
                ..Padding::ZERO
            })
            .into(),
        )
    }
}
