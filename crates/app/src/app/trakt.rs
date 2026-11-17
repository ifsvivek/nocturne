//! Trakt (opt-in): the connect dialog (device-code sign-in with the user's own Trakt app), scrobbling
//! from the player's reports and the watched-history import. Protocol and rules are in
//! `nocturne_core::trakt` (docs/TRAKT.md); this file is only the glue.
use super::App;
use super::message::Message;
use crate::player::{PlayEvent, Reported};
use crate::style;
use crate::task::{after, run};
use crate::theme as t;
use crate::widget::{action_button, mono};
use iced::widget::{column, container, opaque, row, text, text_input};
use iced::{Alignment, Element, Length, Task};
use nocturne_core::source::{Kind, Media};
use nocturne_core::trakt::{self as tk, DeviceCode, Ev, History, Poll, Scrobbler, Trakt};
use nocturne_core::watched::Via;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

type El<'a> = Element<'a, Message>;

const PORTAL: &str = "https://trakt.tv/oauth/applications";

#[derive(Clone)]
pub enum Msg {
    Open,
    Close,
    Id(String),
    Secret(String),
    Connect,
    Code(u64, Result<DeviceCode, tk::Error>),
    Poll(u64, u64),
    Polled(u64, Poll, u64),
    /// The saved sign-in was read at launch: (client, username when it still works).
    Loaded(Option<(Arc<Trakt>, Option<String>)>),
    /// A fresh sign-in was granted and its user name read.
    Connected(Arc<Trakt>, Option<String>),
    Disconnect,
    Import,
    Imported(Result<Vec<Media>, ()>),
    OpenUrl(&'static str),
    OpenCode,
}

enum Stage {
    Form,
    /// Waiting for the user to enter the code on trakt.tv.
    Code(DeviceCode),
}

pub struct State {
    open: bool,
    id: String,
    secret: String,
    stage: Stage,
    note: Option<String>,
    epoch: u64,
    client: Option<Arc<Trakt>>,
    worker: Option<tk::Worker>,
    scrobbler: Scrobbler,
    pub user: Option<String>,
    importing: bool,
    loaded: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            id: String::new(),
            secret: String::new(),
            stage: Stage::Form,
            note: None,
            epoch: 0,
            client: None,
            worker: None,
            scrobbler: Scrobbler::default(),
            user: None,
            importing: false,
            loaded: false,
        }
    }
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The Settings chip: whether a sign-in is in place.
    pub fn label(&self) -> String {
        match (&self.user, &self.client) {
            (Some(u), _) => format!("Connected as {u}"),
            (None, Some(_)) => "Reconnect".into(),
            _ => "Set up".into(),
        }
    }
}

impl App {
    pub(super) fn trakt_chip_label(&self) -> String {
        self.trakt.label()
    }

    /// Read the saved sign-in once, after launch, when Trakt is switched on in Settings.
    pub(super) fn trakt_load(&mut self) -> Task<Message> {
        if self.trakt.loaded || !(self.settings.trakt_scrobble || self.settings.trakt_history) {
            return Task::none();
        }
        self.trakt.loaded = true;
        run(
            async {
                let c = Arc::new(Trakt::load().await?);
                let user = c.username().await.ok();
                Some((c, user))
            },
            |r| Message::Trakt(Msg::Loaded(r)),
        )
    }

    pub(super) fn trakt_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Open => {
                self.trakt.open = true;
                self.trakt.note = None;
                if self.trakt.client.is_none() && self.trakt.id.is_empty() {
                    // The id is not secret; the user may have entered it before.
                    return run(
                        async {
                            tokio::task::spawn_blocking(Trakt::saved_app_blocking)
                                .await
                                .ok()
                                .flatten()
                        },
                        |r| Message::Trakt(Msg::Id(r.map(|(i, _)| i).unwrap_or_default())),
                    );
                }
            }
            Msg::Close => {
                self.trakt.open = false;
                self.trakt.epoch += 1;
                self.trakt.stage = Stage::Form;
            }
            Msg::Id(s) => self.trakt.id = s,
            Msg::Secret(s) => self.trakt.secret = s,
            Msg::Connect => return self.trakt_connect(),
            Msg::Code(e, r) if e == self.trakt.epoch => match r {
                Ok(d) => {
                    let wait = d.interval.max(1);
                    self.trakt.stage = Stage::Code(d);
                    self.trakt.note = None;
                    let e = self.trakt.epoch;
                    return after(
                        Duration::from_secs(wait),
                        Message::Trakt(Msg::Poll(e, wait)),
                    );
                }
                Err(tk::Error::Auth) => {
                    self.trakt.note = Some("Trakt didn't accept that client id.".into())
                }
                Err(_) => {
                    self.trakt.note =
                        Some("Couldn't reach Trakt. Check the network and try again.".into())
                }
            },
            Msg::Poll(e, every) if e == self.trakt.epoch => {
                let (Some(c), Stage::Code(d)) = (self.trakt.client.clone(), &self.trakt.stage)
                else {
                    return Task::none();
                };
                let code = d.device_code.clone();
                return run(async move { c.poll(&code).await }, move |p| {
                    Message::Trakt(Msg::Polled(e, p, every))
                });
            }
            Msg::Polled(e, p, every) if e == self.trakt.epoch => match p {
                Poll::Pending | Poll::SlowDown => {
                    let every = if p == Poll::SlowDown {
                        every + 5
                    } else {
                        every
                    };
                    return after(
                        Duration::from_secs(every),
                        Message::Trakt(Msg::Poll(e, every)),
                    );
                }
                Poll::Granted(tokens) => {
                    let Some(c) = self.trakt.client.clone() else {
                        return Task::none();
                    };
                    return run(
                        async move {
                            c.set_tokens(tokens).await;
                            let u = c.username().await.ok();
                            (c, u)
                        },
                        |(c, u)| Message::Trakt(Msg::Connected(c, u)),
                    );
                }
                Poll::Denied => {
                    self.trakt.stage = Stage::Form;
                    self.trakt.note = Some("You denied the request on trakt.tv.".into());
                }
                Poll::Expired | Poll::Failed => {
                    self.trakt.stage = Stage::Form;
                    self.trakt.note =
                        Some("The code expired or Trakt couldn't be reached. Try again.".into());
                }
            },
            Msg::Loaded(Some((c, user))) => {
                self.trakt_adopt(c, user);
                // History sync runs at launch when it is on.
                if self.settings.trakt_history && self.trakt.user.is_some() {
                    return self.trakt_import();
                }
            }
            Msg::Loaded(None) => {}
            Msg::Connected(c, user) => {
                self.trakt.stage = Stage::Form;
                self.trakt.secret.clear();
                self.trakt_adopt(c, user);
                // Connecting is the opt-in to scrobbling; history import stays its own switch.
                self.settings.trakt_scrobble = true;
                self.settings.save();
                self.toast("Trakt connected.");
                if self.settings.trakt_history {
                    return self.trakt_import();
                }
            }
            Msg::Disconnect => {
                self.trakt.epoch += 1;
                let c = self.trakt.client.take();
                self.trakt.worker = None;
                self.trakt.user = None;
                self.trakt.loaded = false;
                self.trakt.stage = Stage::Form;
                self.settings.trakt_scrobble = false;
                self.settings.trakt_history = false;
                self.settings.save();
                self.toast("Trakt disconnected.");
                return run(
                    async move {
                        if let Some(c) = c {
                            c.revoke().await;
                        }
                        let _ = tokio::task::spawn_blocking(Trakt::forget_blocking).await;
                    },
                    |_| Message::Noop,
                );
            }
            Msg::Import => return self.trakt_import(),
            Msg::Imported(r) => return self.trakt_imported(r),
            Msg::OpenUrl(u) => open_url(u),
            Msg::OpenCode => {
                if let Stage::Code(d) = &self.trakt.stage
                    && d.verification_url.starts_with("https://")
                {
                    open_url(&d.verification_url);
                }
            }
            _ => {} // a stale epoch: the dialog was closed or restarted
        }
        Task::none()
    }

    fn trakt_adopt(&mut self, c: Arc<Trakt>, user: Option<String>) {
        self.trakt.worker = Some(tk::Worker::spawn(c.clone()));
        self.trakt.client = Some(c);
        self.trakt.user = user;
    }

    fn trakt_connect(&mut self) -> Task<Message> {
        let (id, secret) = (
            self.trakt.id.trim().to_string(),
            self.trakt.secret.trim().to_string(),
        );
        if id.is_empty() || secret.is_empty() {
            self.trakt.note = Some("Enter both the client id and the client secret.".into());
            return Task::none();
        }
        self.trakt.epoch += 1;
        let e = self.trakt.epoch;
        let c = Arc::new(Trakt::new(&tk::base_url(), &id, &secret, None, true));
        self.trakt.client = Some(c.clone());
        self.trakt.note = Some("Asking Trakt for a code\u{2026}".into());
        run(
            async move {
                let (i, s) = (id.clone(), secret.clone());
                let saved = tokio::task::spawn_blocking(move || Trakt::save_app_blocking(&i, &s))
                    .await
                    .unwrap_or(Err("failed".into()));
                if saved.is_err() {
                    return Err(tk::Error::Failed);
                }
                c.device_code().await
            },
            move |r| Message::Trakt(Msg::Code(e, r)),
        )
    }

    // ---- scrobbling ----

    /// The player's reports for the item that was playing: scrobble start / pause / stop.
    pub(super) fn trakt_event(&mut self, ev: &PlayEvent) {
        if !self.settings.trakt_scrobble || !matches!(ev.media.kind, Kind::Movie | Kind::Episode) {
            return;
        }
        let Some(w) = &self.trakt.worker else { return };
        if w.rejected.swap(false, Ordering::Relaxed) {
            self.toast("Trakt needs you to connect again (Settings, Trakt account).");
            return;
        }
        let e = match ev.what {
            Reported::Started => Ev::Started,
            Reported::Progress => Ev::Progress { paused: ev.paused },
            Reported::Stopped => Ev::Stopped,
        };
        let Some(action) = self.trakt.scrobbler.on(&ev.media.id, e) else {
            return;
        };
        let Some(dur) = ev.dur.filter(|d| *d > 0.0) else {
            return;
        };
        w.send(tk::Job {
            backend: self.catalog.backend(ev.media.origin).cloned(),
            media: ev.media.clone(),
            action,
            progress: (ev.pos / dur * 100.0).clamp(0.0, 100.0),
        });
    }

    // ---- history import ----

    fn trakt_import(&mut self) -> Task<Message> {
        let Some(c) = self.trakt.client.clone().filter(|_| !self.trakt.importing) else {
            return Task::none();
        };
        self.trakt.importing = true;
        let backends: Vec<_> = self
            .catalog
            .online()
            .filter_map(|s| s.backend.clone())
            .collect();
        // Servers are read only when they could also be written to; local sources always.
        let servers_ok = self.settings.allow_server_writes && !self.dev;
        let backends: Vec<_> = backends
            .into_iter()
            .filter(|b| b.is_local() || servers_ok)
            .collect();
        run(
            async move {
                let h: History = c.history().await.map_err(|_| ())?;
                let mut out = vec![];
                for b in &backends {
                    out.extend(tk::unwatched_in(b, &h).await);
                }
                Ok(out)
            },
            |r| Message::Trakt(Msg::Imported(r)),
        )
    }

    /// Mark what Trakt says is watched: local state always, servers only with server changes on. These
    /// writes never cascade to other sources (the state came from Trakt, not from the user's click).
    fn trakt_imported(&mut self, r: Result<Vec<Media>, ()>) -> Task<Message> {
        self.trakt.importing = false;
        let Ok(items) = r else {
            self.toast("Couldn't read your Trakt history.");
            return Task::none();
        };
        let mut tasks = vec![];
        let mut n = 0;
        for m in items.into_iter().take(500) {
            match self.via_of(m.origin) {
                Some(Via::Local) => {
                    let Some(l) = self
                        .catalog
                        .backend(m.origin)
                        .and_then(|b| b.local())
                        .cloned()
                    else {
                        continue;
                    };
                    self.patch(m.origin, &m.id, |x| x.played = true);
                    let (origin, id) = (m.origin, m.id.clone());
                    tasks.push(run(
                        async move { l.set_played(&m, true).await.is_ok() },
                        move |ok| Message::LocalWrite(origin, id, true, ok),
                    ));
                    n += 1;
                }
                Some(Via::Server) => {
                    let Some(h) = self.hooks_for(m.origin).filter(|h| h.can_write()) else {
                        continue;
                    };
                    self.patch(m.origin, &m.id, |x| x.played = true);
                    h.set_played(&m.id, true);
                    n += 1;
                }
                None => {}
            }
        }
        if n > 0 {
            self.toast(format!("Marked {n} items watched from Trakt."));
        }
        Task::batch(tasks)
    }

    // ---- the dialog ----

    pub(super) fn trakt_dialog(&self) -> El<'_> {
        let s = &self.trakt;
        let dim = |x: String| {
            text(x)
                .size(t::CONTROL_TEXT)
                .line_height(1.5)
                .color(t::TEXT_SECONDARY)
        };
        let mut card = column![
            mono("TRAKT"),
            text("Scrobble and sync with Trakt").size(t::HEADING).font(t::BOLD),
            dim("NOCTURNE ships no Trakt credentials. Create a free API app at trakt.tv/oauth/applications (use urn:ietf:wg:oauth:2.0:oob as its redirect URI), then paste its Client ID and Client Secret below. Both stay in your system keyring.".into()),
            dim(tk::PRIVACY.into()),
        ]
        .spacing(t::S3);
        let field = |ph: &str, v: &str, secret: bool, f: fn(String) -> Msg| {
            let i = text_input(ph, v)
                .on_input(move |x| Message::Trakt(f(x)))
                .on_submit(Message::Trakt(Msg::Connect))
                .size(t::BODY)
                .padding([t::S2, t::S3])
                .style(style::input);
            if secret { i.secure(true) } else { i }
        };
        match (&s.stage, &s.user) {
            (Stage::Code(d), _) => {
                card = card
                    .push(dim(
                        "On any device, open the address below and enter this code.".into(),
                    ))
                    .push(
                        text(d.user_code.clone())
                            .size(t::HEADING + 8.0)
                            .font(t::BOLD),
                    )
                    .push(mono(d.verification_url.clone()))
                    .push(dim("Waiting for you to approve it\u{2026}".into()))
                    .push(
                        row![
                            action_button(
                                "Open trakt.tv",
                                true,
                                false,
                                true,
                                false,
                                Message::Trakt(Msg::OpenCode)
                            ),
                            action_button(
                                "Cancel",
                                false,
                                false,
                                false,
                                false,
                                Message::Trakt(Msg::Close)
                            ),
                        ]
                        .spacing(t::S3),
                    );
            }
            (Stage::Form, Some(u)) => {
                card = card
                    .push(
                        text(format!("Connected as {u}."))
                            .size(t::BODY)
                            .color(t::ACCENT_TEXT),
                    )
                    .push(
                        row![
                            action_button(
                                "Import history now",
                                false,
                                false,
                                false,
                                s.importing,
                                Message::Trakt(Msg::Import)
                            ),
                            action_button(
                                "Disconnect",
                                false,
                                true,
                                false,
                                false,
                                Message::Trakt(Msg::Disconnect)
                            ),
                        ]
                        .spacing(t::S3),
                    );
            }
            (Stage::Form, None) => {
                card = card
                    .push(field("Client ID", &s.id, false, Msg::Id))
                    .push(field("Client Secret", &s.secret, true, Msg::Secret))
                    .push(
                        row![
                            action_button(
                                "Connect",
                                true,
                                false,
                                true,
                                false,
                                Message::Trakt(Msg::Connect)
                            ),
                            action_button(
                                "Create a Trakt app",
                                false,
                                false,
                                false,
                                false,
                                Message::Trakt(Msg::OpenUrl(PORTAL))
                            ),
                        ]
                        .spacing(t::S3),
                    );
            }
        }
        if let Some(n) = &s.note {
            card = card.push(text(n.clone()).size(t::CONTROL_TEXT).color(t::DANGER));
        }
        card = card.push(
            container(action_button(
                "Close",
                false,
                false,
                false,
                false,
                Message::Trakt(Msg::Close),
            ))
            .align_right(Length::Fill),
        );
        opaque(
            container(
                container(card)
                    .width(560)
                    .max_height(640)
                    .padding(t::S6)
                    .style(style::panel),
            )
            .align_x(Alignment::Center)
            .center(Length::Fill)
            .style(style::scrim),
        )
    }
}

fn open_url(u: &str) {
    if u.starts_with("https://") {
        let _ = std::process::Command::new("xdg-open").arg(u).spawn();
    }
}
