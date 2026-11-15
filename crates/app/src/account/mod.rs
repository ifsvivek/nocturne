//! Accounts: connect, sign in (password or Quick Connect), restore, sign out, change password.
//! One GPUI entity owns the state (`state::Account`); `Root` forwards keys to it while an account
//! screen is up and reacts to its events. All I/O goes through the tokio bridge, never the UI thread.
mod state;
mod view;

use crate::bridge;
use gpui::*;
use nocturne_jellyfin_user::{Error, QuickConnect, Session, UserClient, UserConfirmed};
use nocturne_session::Servers;
use state::*;
use std::time::Duration;

pub use state::KEYRING_NOTE;

pub enum Event {
    /// A working login (typed, Quick Connect, or restored from the keyring).
    SignedIn { url: String, session: Session },
    /// Calm one-liner for the toast.
    Notice(&'static str),
}

impl EventEmitter<Event> for Accounts {}

struct PwDialog {
    client: UserClient,
    form: Form,
    busy: bool,
    msg: Option<&'static str>,
}

pub struct Accounts {
    st: Account,
    servers: Servers,
    qc_secret: Option<String>,
    dialog: Option<PwDialog>,
}

fn classify(e: &Error) -> ConnectFail {
    match e {
        Error::Http(h) if !h.is_decode() => ConnectFail::Unreachable,
        _ => ConnectFail::NotServer,
    }
}

fn poll_of(r: Result<bool, Error>) -> Poll {
    match r {
        Ok(true) => Poll::Authorized,
        Ok(false) => Poll::Pending,
        Err(Error::Status {
            status: 400 | 401 | 404,
            ..
        }) => Poll::Gone,
        Err(_) => Poll::Hiccup,
    }
}

/// Is `url` a server, what is it called, and does the keyring hold a session that still works?
async fn probe(url: String) -> Result<(Probe, Option<Session>), ConnectFail> {
    let c = UserClient::anonymous(&url).map_err(|e| classify(&e))?;
    let info = c.public_info().await.map_err(|e| classify(&e))?;
    if info.id.is_empty() {
        return Err(ConnectFail::NotServer);
    }
    let mut p = Probe {
        name: info.server_name,
        quick_connect: c.quick_connect_enabled().await.unwrap_or(false),
        ..Default::default()
    };
    let u = url.clone();
    match tokio::task::spawn_blocking(move || nocturne_session::load(&u)).await {
        Ok(Ok(Some(s))) => {
            let me = UserClient::from_session(&url, &s).map_err(|e| classify(&e))?;
            match me.me(s).await {
                Ok(s) => {
                    p.restored = true;
                    return Ok((p, Some(s)));
                }
                Err(Error::Status { status: 401, .. }) => {
                    let _ = nocturne_session::delete(&url);
                    p.expired = true;
                }
                Err(_) => {} // can't tell right now: sign in again, the stored login stays
            }
        }
        Ok(Err(nocturne_session::Error::Unavailable(_))) => p.keyring_down = true,
        _ => {}
    }
    Ok((p, None))
}

/// Keyring write; `false` means no keyring (the session then lives in memory only).
async fn persist(url: &str, s: &Session) -> bool {
    let (url, s) = (url.to_string(), s.clone());
    matches!(
        tokio::task::spawn_blocking(move || nocturne_session::save(&url, &s)).await,
        Ok(Ok(()))
    )
}

async fn finish(url: String, r: Result<Session, Error>) -> Result<(Session, bool), LoginFail> {
    match r {
        Ok(s) => {
            let saved = persist(&url, &s).await;
            Ok((s, saved))
        }
        Err(Error::InvalidCredentials) => Err(LoginFail::Invalid),
        Err(_) => Err(LoginFail::Unreachable),
    }
}

impl Accounts {
    /// `restore`: try the last used server's stored session first (not in developer-token mode).
    pub fn new(restore: bool, cx: &mut Context<Self>) -> Self {
        let servers = Servers::load().unwrap_or_default();
        let last = servers.last_used.clone().filter(|_| restore);
        let mut me = Self {
            st: Account::new(servers.servers.clone(), servers.last_used.clone()),
            servers,
            qc_secret: None,
            dialog: None,
        };
        if let Some(r) = last.and_then(|u| me.st.start_restore(&u)) {
            me.check(r, cx);
        }
        me
    }

    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn keyring_down(&self) -> bool {
        self.st.keyring_down
    }

    fn check(&mut self, (epoch, url): (u64, String), cx: &mut Context<Self>) {
        let u = url.clone();
        bridge::run(cx, probe(u), move |a, r, cx| {
            let (probe, session) = match r {
                Ok((p, s)) => (Ok(p), s),
                Err(e) => (Err(e), None),
            };
            if a.st.probed(epoch, probe, &url)
                && let Some(s) = session
            {
                a.signed_in(url, s, true, cx);
            }
        });
    }

    fn signed_in(&mut self, url: String, session: Session, saved: bool, cx: &mut Context<Self>) {
        self.servers.remember(&url);
        let _ = self.servers.save();
        self.st.keyring_down |= !saved;
        cx.emit(Event::SignedIn { url, session });
    }

    fn on_signed_in(
        &mut self,
        epoch: u64,
        r: Result<(Session, bool), LoginFail>,
        cx: &mut Context<Self>,
    ) {
        let ok = r.as_ref().map(|_| ()).map_err(|e| *e);
        if self.st.logged_in(epoch, ok)
            && let Ok((s, saved)) = r
        {
            self.signed_in(self.st.url.clone(), s, saved, cx);
        }
    }

    // ---- actions (keys and clicks both land here) ----

    fn connect_go(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.st.start_connect() {
            self.check(r, cx);
        }
        cx.notify();
    }

    fn pick(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.st.pick_server(i) {
            self.check(r, cx);
        }
        cx.notify();
    }

    fn login_go(&mut self, cx: &mut Context<Self>) {
        if let Some((epoch, url, user, pw)) = self.st.start_login() {
            let u = url.clone();
            bridge::run(
                cx,
                async move {
                    let r = UserClient::authenticate_by_name(&url, &user, &pw).await;
                    finish(u, r).await
                },
                move |a, r, cx| a.on_signed_in(epoch, r, cx),
            );
        }
        cx.notify();
    }

    fn qc_go(&mut self, cx: &mut Context<Self>) {
        let Some(epoch) = self.st.qc_start() else {
            return;
        };
        let url = self.st.url.clone();
        bridge::run(
            cx,
            async move {
                UserClient::anonymous(&url)
                    .ok()?
                    .quick_connect_initiate()
                    .await
                    .ok()
            },
            move |a, r: Option<QuickConnect>, cx| {
                a.qc_secret = r.as_ref().map(|q| q.secret.clone());
                if a.st.qc_code(epoch, r.map(|q| q.code)) {
                    a.qc_later(epoch, cx);
                }
            },
        );
        cx.notify();
    }

    /// Poll again in ~2 s unless the screen moved on.
    fn qc_later(&mut self, epoch: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            this.update(cx, |a, cx| a.qc_poll(epoch, cx)).ok();
        })
        .detach();
    }

    fn qc_poll(&mut self, epoch: u64, cx: &mut Context<Self>) {
        let (Some(secret), true) = (self.qc_secret.clone(), epoch == self.st.epoch) else {
            return;
        };
        let url = self.st.url.clone();
        bridge::run(
            cx,
            async move {
                match UserClient::anonymous(&url) {
                    Ok(c) => poll_of(c.quick_connect_poll(&secret).await),
                    Err(_) => Poll::Gone,
                }
            },
            move |a, p, cx| match a.st.qc_polled(epoch, p) {
                QcNext::Wait => a.qc_later(epoch, cx),
                QcNext::Exchange => a.qc_exchange(epoch, cx),
                QcNext::Stop => a.qc_secret = None,
            },
        );
    }

    fn qc_exchange(&mut self, epoch: u64, cx: &mut Context<Self>) {
        let (Some(secret), url) = (self.qc_secret.take(), self.st.url.clone()) else {
            return;
        };
        let u = url.clone();
        bridge::run(
            cx,
            async move {
                let r = match UserClient::anonymous(&url) {
                    Ok(c) => c.authenticate_with_quick_connect(&secret).await,
                    Err(e) => Err(e),
                };
                finish(u, r).await
            },
            move |a, r, cx| a.on_signed_in(epoch, r, cx),
        );
    }

    fn qc_stop(&mut self, cx: &mut Context<Self>) {
        self.qc_secret = None;
        self.st.qc_cancel();
        cx.notify();
    }

    fn back_to_servers(&mut self, cx: &mut Context<Self>) {
        self.switch_server(cx);
    }

    /// Show the address screen again (the stored login for the current server stays in the keyring).
    pub fn switch_server(&mut self, cx: &mut Context<Self>) {
        self.qc_secret = None;
        self.servers = Servers::load().unwrap_or_default();
        self.st.switch_server(self.servers.servers.clone());
        cx.notify();
    }

    /// Log out on the server, delete the keyring entry, show the login form.
    pub fn sign_out(&mut self, client: Option<UserClient>, cx: &mut Context<Self>) {
        let url = self.st.url.clone();
        bridge::spawn(async move {
            if let Some(c) = client {
                let _ = c.logout().await;
            }
            let _ = tokio::task::spawn_blocking(move || nocturne_session::delete(&url)).await;
        });
        self.qc_secret = None;
        self.dialog = None;
        self.st.signed_out();
        cx.notify();
    }

    // ---- change password ----

    pub fn open_password(&mut self, client: UserClient, cx: &mut Context<Self>) {
        let f = |label| Fld {
            label,
            secret: true,
            ..Default::default()
        };
        self.dialog = Some(PwDialog {
            client,
            form: Form::new(
                vec![
                    f("Current password"),
                    f("New password"),
                    f("Repeat new password"),
                ],
                2,
            ),
            busy: false,
            msg: None,
        });
        cx.notify();
    }

    /// Enter on "Change password": the only place the confirmation token is minted.
    fn pw_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dialog.as_mut().filter(|d| !d.busy) else {
            return;
        };
        let (cur, new) = (
            d.form.fields[0].value.clone(),
            d.form.fields[1].value.clone(),
        );
        if cur.is_empty() || new.is_empty() {
            d.msg = Some("Fill in both your current and your new password.");
            return;
        }
        if new != d.form.fields[2].value {
            d.msg = Some("The new passwords don't match.");
            return;
        }
        d.busy = true;
        d.msg = None;
        let c = d.client.clone();
        bridge::run(
            cx,
            async move {
                c.change_own_password(&cur, &new, UserConfirmed::i_have_asked_the_user())
                    .await
            },
            |a, r, cx| match r {
                Ok(()) => {
                    a.dialog = None;
                    cx.emit(Event::Notice("Password changed."));
                }
                Err(e) => {
                    if let Some(d) = a.dialog.as_mut() {
                        d.busy = false;
                        d.form.fields[0].value.clear();
                        d.form.focus = 0;
                        d.msg = Some(match e {
                            Error::Status {
                                status: 400 | 401 | 403,
                                ..
                            } => "That current password wasn't accepted.",
                            _ => "Couldn't change the password. Try again.",
                        });
                    }
                }
            },
        );
    }

    fn dialog_press(&mut self, n: usize, cx: &mut Context<Self>) {
        if n == 0 {
            self.dialog = None;
        } else {
            self.pw_confirm(cx);
        }
        cx.notify();
    }

    // ---- keyboard ----

    /// Keyboard entry (`Root` calls this while an account screen or the dialog is up).
    pub fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let m = &ev.keystroke.modifiers;
        let ch = ev
            .keystroke
            .key_char
            .as_deref()
            .filter(|_| !m.control && !m.platform);
        let paste = (m.control || m.platform) && key == "v";
        let clip = || cx.read_from_clipboard().and_then(|c| c.text());
        if self.dialog.is_some() {
            let text = paste.then(clip).flatten();
            let Some(d) = self.dialog.as_mut() else {
                return;
            };
            if key == "escape" {
                self.dialog = None;
            } else if let Some(t) = text {
                d.form.paste(&t);
            } else {
                match d.form.key(key, ch) {
                    // Enter on the last input moves to the button; only Enter on the button confirms.
                    Ev::Submit => d.form.focus = d.form.fields.len() + 1,
                    Ev::Press(n) => self.dialog_press(n, cx),
                    Ev::Stay => {}
                }
            }
            return cx.notify();
        }
        if self.st.busy && key != "escape" {
            return;
        }
        match self.st.stage {
            Stage::Active => {}
            Stage::Connect => {
                let text = paste.then(clip).flatten();
                if let Some(t) = text {
                    self.st.connect.paste(&t);
                } else {
                    match self.st.connect.key(key, ch) {
                        Ev::Submit | Ev::Press(0) => self.connect_go(cx),
                        Ev::Press(n) => self.pick(n - 1, cx),
                        Ev::Stay => {}
                    }
                }
            }
            Stage::Login if self.st.qc != Qc::Off => {
                if matches!(key, "escape" | "enter") {
                    self.qc_stop(cx);
                }
            }
            Stage::Login if key == "escape" => self.back_to_servers(cx),
            Stage::Login => {
                let text = paste.then(clip).flatten();
                if let Some(t) = text {
                    self.st.login.paste(&t);
                } else {
                    let last = self.st.login.buttons - 1;
                    match self.st.login.key(key, ch) {
                        Ev::Submit | Ev::Press(0) => self.login_go(cx),
                        Ev::Press(n) if n == last => self.back_to_servers(cx),
                        Ev::Press(_) => self.qc_go(cx),
                        Ev::Stay => {}
                    }
                }
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectFail, Error, Poll, classify, poll_of};

    #[test]
    fn error_classes() {
        let st = |s| Error::Status {
            status: s,
            path: String::new(),
        };
        assert_eq!(classify(&st(404)), ConnectFail::NotServer);
        assert_eq!(
            classify(&Error::Config(String::new())),
            ConnectFail::NotServer
        );
        assert_eq!(poll_of(Ok(true)), Poll::Authorized);
        assert_eq!(poll_of(Ok(false)), Poll::Pending);
        assert_eq!(poll_of(Err(st(404))), Poll::Gone);
        assert_eq!(poll_of(Err(st(500))), Poll::Hiccup);
    }
}
