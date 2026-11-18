//! Trakt (opt-in): the connect dialog (browser sign-in with PKCE, using the user's own Trakt app id),
//! scrobbling from the player's reports and the watched-history import. Protocol and rules are in
//! `nocturne_core::trakt` (docs/TRAKT.md); this file is only the glue.
use super::App;
use super::message::Message;
use crate::player::{PlayEvent, Reported};
use crate::style;
use crate::task::run;
use crate::theme as t;
use crate::widget::{action_button, mono};
use iced::widget::{column, container, opaque, row, text, text_input};
use iced::{Alignment, Element, Length, Task};
use nocturne_core::source::{Kind, Media};
use nocturne_core::trakt::{self as tk, Callback, Ev, History, Pkce, Scrobbler, Trakt};
use nocturne_core::watched::Via;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type El<'a> = Element<'a, Message>;

const PORTAL: &str = "https://trakt.tv/oauth/applications";
/// How long the browser sign-in may take.
const SIGN_IN_WAIT: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub enum Msg {
    Open,
    Close,
    Id(String),
    Connect,
    /// The loopback listener ended (epoch of the attempt).
    Callback(u64, Callback),
    /// The code was exchanged: the client with its tokens, and the user name when it could be read.
    Connected(u64, Result<(Arc<Trakt>, Option<String>), tk::Error>),
    /// The saved sign-in was read at launch: (client, username when it still works).
    Loaded(Option<(Arc<Trakt>, Option<String>)>),
    Disconnect,
    Import,
    Imported(Result<Vec<Media>, ()>),
    OpenUrl(&'static str),
    /// Open the authorize page again.
    Reopen,
}

enum Stage {
    Form,
    /// Waiting for the browser sign-in; the URL is kept so it can be opened again.
    Browser(String),
}

pub struct State {
    open: bool,
    id: String,
    stage: Stage,
    note: Option<String>,
    epoch: u64,
    pkce: Option<Pkce>,
    cancel: Arc<AtomicBool>,
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
            stage: Stage::Form,
            note: None,
            epoch: 0,
            pkce: None,
            cancel: Arc::default(),
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

    /// Stop a running sign-in wait (closes the loopback listener).
    fn abort(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.epoch += 1;
        self.pkce = None;
        self.stage = Stage::Form;
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
                // Debug builds only: prefill from the environment (a live test with the maintainer's own app).
                if cfg!(debug_assertions)
                    && self.trakt.id.is_empty()
                    && let Ok(id) = std::env::var("NOCTURNE_TRAKT_CLIENT_ID")
                {
                    self.trakt.id = id;
                }
                if self.trakt.client.is_none() && self.trakt.id.is_empty() {
                    // The client id is not secret under PKCE; the user may have entered it before.
                    return run(
                        async {
                            tokio::task::spawn_blocking(Trakt::saved_app_blocking)
                                .await
                                .ok()
                                .flatten()
                        },
                        |r| Message::Trakt(Msg::Id(r.unwrap_or_default())),
                    );
                }
            }
            Msg::Close => {
                self.trakt.open = false;
                self.trakt.abort();
            }
            Msg::Id(s) => self.trakt.id = nocturne_core::secret::clean(&s),
            Msg::Connect => return self.trakt_connect(),
            Msg::Reopen => {
                if let Stage::Browser(u) = &self.trakt.stage {
                    open_url(u);
                }
            }
            Msg::Callback(e, cb) if e == self.trakt.epoch => {
                let (Some(c), Some(p)) = (self.trakt.client.clone(), self.trakt.pkce.clone())
                else {
                    return Task::none();
                };
                match cb {
                    Callback::Code(code) => {
                        return run(
                            async move {
                                let tokens = c
                                    .exchange(&code, &p, &tk::redirect_uri(tk::REDIRECT_PORT))
                                    .await?;
                                c.set_tokens(tokens).await;
                                let u = c.username().await.ok();
                                Ok((c, u))
                            },
                            move |r| Message::Trakt(Msg::Connected(e, r)),
                        );
                    }
                    Callback::PortBusy => {
                        self.trakt.abort();
                        self.trakt.note = Some(format!(
                            "Port {} is in use, so the sign-in can't finish. Close what uses it and try again.",
                            tk::REDIRECT_PORT
                        ));
                    }
                    Callback::Timeout => {
                        self.trakt.abort();
                        self.trakt.note = Some("The sign-in took too long. Try again.".into());
                    }
                    Callback::Denied => {
                        self.trakt.abort();
                        self.trakt.note = Some("You said no on trakt.tv.".into());
                    }
                    Callback::Cancelled => {}
                }
            }
            Msg::Connected(e, r) if e == self.trakt.epoch => {
                self.trakt.pkce = None;
                self.trakt.stage = Stage::Form;
                match r {
                    Ok((c, user)) => {
                        self.trakt_adopt(c, user);
                        // Connecting is the opt-in to scrobbling; history import stays its own switch.
                        self.settings.trakt_scrobble = true;
                        self.settings.save();
                        self.toast("Trakt connected.");
                        if self.settings.trakt_history {
                            return self.trakt_import();
                        }
                    }
                    Err(tk::Error::Auth) => {
                        self.trakt.note = Some("Trakt didn't accept the sign-in. Check the client id and that the redirect URI matches.".into())
                    }
                    Err(_) => self.trakt.note = Some("Couldn't reach Trakt. Try again.".into()),
                }
            }
            Msg::Loaded(Some((c, user))) => {
                self.trakt_adopt(c, user);
                // History sync runs at launch when it is on.
                if self.settings.trakt_history && self.trakt.user.is_some() {
                    return self.trakt_import();
                }
            }
            Msg::Loaded(None) => {}
            Msg::Disconnect => {
                self.trakt.abort();
                let c = self.trakt.client.take();
                self.trakt.worker = None;
                self.trakt.user = None;
                self.trakt.loaded = false;
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
            _ => {} // a stale epoch: the dialog was closed or restarted
        }
        Task::none()
    }

    fn trakt_adopt(&mut self, c: Arc<Trakt>, user: Option<String>) {
        self.trakt.worker = Some(tk::Worker::spawn(c.clone()));
        self.trakt.client = Some(c);
        self.trakt.user = user;
    }

    /// Start the browser sign-in: listen on the loopback port, open the authorize page, wait for the redirect.
    fn trakt_connect(&mut self) -> Task<Message> {
        let id = self.trakt.id.trim().to_string();
        if id.is_empty() {
            self.trakt.note = Some("Enter the client id of your Trakt app.".into());
            return Task::none();
        }
        self.trakt.abort(); // a previous attempt's listener ends
        self.trakt.cancel = Arc::new(AtomicBool::new(false));
        let (e, cancel, pkce) = (self.trakt.epoch, self.trakt.cancel.clone(), Pkce::new());
        let url = tk::authorize_url(
            &tk::auth_base(),
            &id,
            &tk::redirect_uri(tk::REDIRECT_PORT),
            &pkce,
        );
        self.trakt.client = Some(Arc::new(Trakt::new(
            &tk::base_url(),
            &tk::auth_base(),
            &id,
            None,
            true,
        )));
        self.trakt.stage = Stage::Browser(url.clone());
        self.trakt.note = None;
        let state = pkce.state.clone();
        self.trakt.pkce = Some(pkce);
        run(
            async move {
                let i = id.clone();
                let _ = tokio::task::spawn_blocking(move || Trakt::save_app_blocking(&i)).await;
                // The listener is bound before the browser opens, so the redirect cannot arrive early.
                let wait = tokio::spawn(async move {
                    tk::wait_callback(tk::REDIRECT_PORT, &state, SIGN_IN_WAIT, cancel).await
                });
                tokio::time::sleep(Duration::from_millis(200)).await;
                open_url(&url);
                wait.await.unwrap_or(Callback::Cancelled)
            },
            move |cb| Message::Trakt(Msg::Callback(e, cb)),
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
            dim(format!("NOCTURNE ships no Trakt credentials. Create a free API app at trakt.tv/oauth/applications, register {} as its redirect URI, then paste its Client ID below. There is no client secret: you sign in on trakt.tv in your browser.", tk::redirect_uri(tk::REDIRECT_PORT))),
            dim(tk::PRIVACY.into()),
            text(tk::CREDIT).size(t::CAPTION).color(t::TEXT_TERTIARY),
        ]
        .spacing(t::S3);
        match (&s.stage, &s.user) {
            (Stage::Browser(_), _) => {
                card = card
                    .push(dim("Approve NOCTURNE in the browser tab that opened. This window continues by itself (it gives up after five minutes).".into()))
                    .push(
                        row![
                            action_button("Open the page again", false, false, false, false, Message::Trakt(Msg::Reopen)),
                            action_button("Cancel", true, false, true, false, Message::Trakt(Msg::Close)),
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
                let id = text_input("Client ID", &s.id)
                    .on_input(|x| Message::Trakt(Msg::Id(x)))
                    .on_submit(Message::Trakt(Msg::Connect))
                    .size(t::BODY)
                    .padding([t::S2, t::S3])
                    .style(style::input);
                card = card.push(id).push(
                    row![
                        action_button(
                            "Sign in with Trakt",
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
    if u.starts_with("https://") || u.starts_with("http://127.0.0.1") {
        let _ = std::process::Command::new("xdg-open").arg(u).spawn();
    }
}
