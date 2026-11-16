//! Accounts: connect, sign in (password or Quick Connect), restore, sign out, own-profile dialogs.
//! One struct owns the state (`state::Account`); the root forwards keys to it while an account
//! screen or dialog is up and drains its [`Event`]s. All I/O goes through `task::run`.
pub mod devices;
pub mod picker;
mod view;

use crate::keys::Key;
use crate::task::{after, run};
use iced::{Element, Task, clipboard};
use nocturne_core::account::ops::*;
pub use nocturne_core::account::state;
use nocturne_jellyfin_user::{Error, Session, UserClient, UserConfirmed};
use nocturne_session::Servers;
use state::*;
use std::time::{Duration, Instant};

pub use state::KEYRING_NOTE;

pub enum Event {
    /// A working login (typed, Quick Connect, or restored from the keyring).
    SignedIn { url: String, session: Session },
    /// Calm one-liner for the toast.
    Notice(&'static str),
    /// The profile picture changed: the root reads it again.
    Reload,
    /// Esc on the sign-in form that the profile picker opened: show the picker again.
    BackToPicker,
}

/// What the account dialog is for (playlist dialogs arrive with step 4c).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DlgKind {
    Password,
    /// Approve another device's Quick Connect code with this account.
    Authorize,
    /// Upload a profile image from a file on this computer.
    ProfileImage,
    RemoveImage,
    /// Display language (a code the server offers, such as `de`).
    Language,
}

struct Dialog {
    kind: DlgKind,
    client: UserClient,
    form: Form,
    busy: bool,
    msg: Option<&'static str>,
}

/// Which text field a click focuses.
#[derive(Clone, Copy, Debug)]
pub enum Where {
    Connect,
    Login(usize),
    Dialog(usize),
}

#[derive(Clone)]
pub enum Msg {
    Checked {
        epoch: u64,
        url: String,
        r: Result<(Probe, Option<Session>), ConnectFail>,
    },
    LoggedIn {
        epoch: u64,
        r: Result<(Session, bool), LoginFail>,
    },
    QcCode {
        epoch: u64,
        r: Option<(String, String)>,
    },
    QcLater(u64),
    QcPolled {
        epoch: u64,
        p: Poll,
    },
    /// Dialog finished: `Ok` or the calm sentence for what went wrong.
    DialogDone(DlgKind, Result<(), &'static str>),
    /// Password dialog failure that also clears the current-password field.
    PwFailed(&'static str),
    Focus(Where),
    ConnectGo,
    Pick(usize),
    Forget(usize),
    LoginGo,
    QcGo,
    QcStop,
    BackToServers,
    DialogPress(usize),
    OpenDocs,
    Paste(Option<String>),
    /// The server's current display language is known: open its dialog.
    OpenLanguage(UserClient, String),
}

pub struct Accounts {
    st: Account,
    servers: Servers,
    qc_secret: Option<String>,
    dialog: Option<Dialog>,
    /// The sign-in form was opened from the profile picker, so Esc returns there.
    from_picker: bool,
    pub events: Vec<Event>,
}

impl Accounts {
    /// `restore`: try the last used server's stored session first (not in developer-token mode).
    pub fn new(restore: bool) -> (Self, Task<Msg>) {
        let servers = Servers::load().unwrap_or_default();
        let last = servers.last_used.clone().filter(|_| restore);
        let mut me = Self {
            st: Account::new(servers.servers.clone(), servers.last_used.clone()),
            servers,
            qc_secret: None,
            dialog: None,
            from_picker: false,
            events: vec![],
        };
        let task = match last.and_then(|u| me.st.start_restore(&u)) {
            Some(r) => me.check(r, true),
            None => Task::none(),
        };
        (me, task)
    }

    /// The address screen is up and idle (Esc may leave it for where the user came from).
    pub fn at_connect(&self) -> bool {
        self.st.stage == Stage::Connect && !self.st.busy && self.dialog.is_none()
    }

    /// Sign-out, add-account and expiry act on `url`: make it the server these screens talk about
    /// (the launch no longer goes through the address screen, which used to set it).
    pub fn point_at(&mut self, url: &str) {
        if self.st.url != url {
            self.st.url = url.to_string();
            self.st.server_name = nocturne_core::profile::label_for(url);
        }
    }

    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn keyring_down(&self) -> bool {
        self.st.keyring_down
    }

    /// Something pulses (checking, restoring, waiting for Quick Connect): the window needs frames.
    pub fn animating(&self) -> bool {
        self.st.busy || self.st.qc != Qc::Off
    }

    /// Sign-in form for the profile picker: no stored session is restored, `name` is prefilled.
    pub fn sign_in_form(&mut self, url: &str, name: Option<String>) -> Task<Msg> {
        self.qc_secret = None;
        self.dialog = None;
        self.from_picker = true;
        match self.st.start_form(url, name) {
            Some(r) => self.check(r, false),
            None => Task::none(),
        }
    }

    fn check(&mut self, (epoch, url): (u64, String), restore: bool) -> Task<Msg> {
        let u = url.clone();
        run(probe(u, restore), move |r| Msg::Checked { epoch, url, r })
    }

    fn signed_in(&mut self, url: String, session: Session, saved: bool) {
        self.from_picker = false;
        self.servers.remember(&url);
        let _ = self.servers.save();
        self.st.keyring_down |= !saved;
        self.events.push(Event::SignedIn { url, session });
    }

    fn on_signed_in(&mut self, epoch: u64, r: Result<(Session, bool), LoginFail>) {
        let ok = r.as_ref().map(|_| ()).map_err(|e| *e);
        if self.st.logged_in(epoch, ok)
            && let Ok((s, saved)) = r
        {
            self.signed_in(self.st.url.clone(), s, saved);
        }
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Checked { epoch, url, r } => {
                let (probe, session) = match r {
                    Ok((p, s)) => (Ok(p), s),
                    Err(e) => (Err(e), None),
                };
                if self.st.probed(epoch, probe, &url)
                    && let Some(s) = session
                {
                    self.signed_in(url, s, true);
                }
            }
            Msg::LoggedIn { epoch, r } => self.on_signed_in(epoch, r),
            Msg::QcCode { epoch, r } => {
                self.qc_secret = r.as_ref().map(|q| q.0.clone());
                if self.st.qc_code(epoch, r.map(|q| q.1)) {
                    return after(Duration::from_secs(2), Msg::QcLater(epoch));
                }
            }
            Msg::QcLater(epoch) => return self.qc_poll(epoch),
            Msg::QcPolled { epoch, p } => match self.st.qc_polled(epoch, p) {
                QcNext::Wait => return after(Duration::from_secs(2), Msg::QcLater(epoch)),
                QcNext::Exchange => return self.qc_exchange(epoch),
                QcNext::Stop => self.qc_secret = None,
            },
            Msg::DialogDone(kind, r) => self.dialog_done(kind, r),
            Msg::PwFailed(m) => {
                if let Some(d) = self.dialog.as_mut() {
                    d.busy = false;
                    d.form.fields[0].value.clear();
                    d.form.focus = 0;
                    d.msg = Some(m);
                }
            }
            Msg::Focus(w) => match w {
                Where::Connect => self.st.connect.focus = 0,
                Where::Login(i) => self.st.login.focus = i,
                Where::Dialog(i) => {
                    if let Some(d) = self.dialog.as_mut() {
                        d.form.focus = i;
                    }
                }
            },
            Msg::ConnectGo => return self.connect_go(),
            Msg::Pick(i) => return self.pick(i),
            Msg::Forget(i) => self.forget(i),
            Msg::LoginGo => return self.login_go(),
            Msg::QcGo => return self.qc_go(),
            Msg::QcStop => self.qc_stop(),
            Msg::BackToServers => self.back_to_servers(),
            Msg::DialogPress(n) => return self.dialog_press(n),
            Msg::OpenDocs => {
                let _ = std::process::Command::new("xdg-open")
                    .arg(view::DOCS_URL)
                    .spawn();
            }
            Msg::Paste(Some(t)) => self.paste(&t),
            Msg::Paste(None) => {}
            Msg::OpenLanguage(c, cur) => self.open_language(c, &cur),
        }
        Task::none()
    }

    fn paste(&mut self, t: &str) {
        if let Some(d) = self.dialog.as_mut() {
            d.form.paste(t);
            return;
        }
        match self.st.stage {
            Stage::Connect => self.st.connect.paste(t),
            Stage::Login => self.st.login.paste(t),
            Stage::Active => {}
        }
    }

    // ---- actions (keys and clicks both land here) ----

    fn connect_go(&mut self) -> Task<Msg> {
        match self.st.start_connect() {
            Some(r) => self.check(r, true),
            None => Task::none(),
        }
    }

    fn pick(&mut self, i: usize) -> Task<Msg> {
        match self.st.pick_server(i) {
            Some(r) => self.check(r, true),
            None => Task::none(),
        }
    }

    fn login_go(&mut self) -> Task<Msg> {
        let Some((epoch, url, user, pw)) = self.st.start_login() else {
            return Task::none();
        };
        let u = url.clone();
        run(
            async move {
                let r = UserClient::authenticate_by_name(&url, &user, &pw).await;
                finish(u, r).await
            },
            move |r| Msg::LoggedIn { epoch, r },
        )
    }

    fn qc_go(&mut self) -> Task<Msg> {
        let Some(epoch) = self.st.qc_start() else {
            return Task::none();
        };
        let url = self.st.url.clone();
        run(
            async move {
                UserClient::anonymous(&url)
                    .ok()?
                    .quick_connect_initiate()
                    .await
                    .ok()
                    .map(|q| (q.secret, q.code))
            },
            move |r| Msg::QcCode { epoch, r },
        )
    }

    fn qc_poll(&mut self, epoch: u64) -> Task<Msg> {
        let (Some(secret), true) = (self.qc_secret.clone(), epoch == self.st.epoch) else {
            return Task::none();
        };
        let url = self.st.url.clone();
        run(
            async move {
                match UserClient::anonymous(&url) {
                    Ok(c) => poll_of(c.quick_connect_poll(&secret).await),
                    Err(_) => Poll::Gone,
                }
            },
            move |p| Msg::QcPolled { epoch, p },
        )
    }

    fn qc_exchange(&mut self, epoch: u64) -> Task<Msg> {
        let (Some(secret), url) = (self.qc_secret.take(), self.st.url.clone()) else {
            return Task::none();
        };
        let u = url.clone();
        run(
            async move {
                let r = match UserClient::anonymous(&url) {
                    Ok(c) => c.authenticate_with_quick_connect(&secret).await,
                    Err(e) => Err(e),
                };
                finish(u, r).await
            },
            move |r| Msg::LoggedIn { epoch, r },
        )
    }

    fn qc_stop(&mut self) {
        self.qc_secret = None;
        self.st.qc_cancel();
    }

    fn back_to_servers(&mut self) {
        if std::mem::take(&mut self.from_picker) {
            self.st.busy = false;
            self.st.bump_epoch();
            self.events.push(Event::BackToPicker);
            return;
        }
        self.switch_server();
    }

    /// Show the address screen again (the stored login for the current server stays in the keyring).
    pub fn switch_server(&mut self) {
        self.from_picker = false;
        self.qc_secret = None;
        self.servers = Servers::load().unwrap_or_default();
        self.st.switch_server(self.servers.servers.clone());
    }

    /// Remove remembered server `i` and its saved login from this device.
    pub fn forget(&mut self, i: usize) {
        let Some(url) = self.st.servers.get(i).cloned() else {
            return;
        };
        self.servers = Servers::load().unwrap_or_default();
        self.servers.forget(&url);
        let _ = self.servers.save(); // calm: a failed save only brings the entry back next launch
        nocturne_core::bridge::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || nocturne_session::delete(&url)).await;
        });
        self.st.servers.remove(i);
        let typed = self.st.connect.fields[0].value.clone();
        self.st.switch_server(self.servers.servers.clone());
        self.st.connect.fields[0].value = typed;
        self.events.push(Event::Notice("Server forgotten."));
    }

    /// The server rejected the stored login mid-session: forget it, show the login form with a
    /// calm note.
    pub fn session_expired(&mut self, user_id: Option<String>) {
        self.from_picker = false;
        let url = self.st.url.clone();
        nocturne_core::bridge::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                let _ = nocturne_session::delete(&url);
                if let Some(id) = user_id {
                    nocturne_session::forget_account(&url, &id);
                }
            })
            .await;
        });
        self.qc_secret = None;
        self.dialog = None;
        self.st.signed_out();
        self.st.msg = Some(MSG_EXPIRED);
    }

    /// Log out on the server, delete the keyring entry, show the login form.
    pub fn sign_out(&mut self, client: Option<UserClient>, user_id: Option<String>) {
        self.from_picker = false;
        let url = self.st.url.clone();
        nocturne_core::bridge::spawn(async move {
            if let Some(c) = client {
                let _ = c.logout().await;
            }
            let _ = tokio::task::spawn_blocking(move || {
                let _ = nocturne_session::delete(&url);
                if let Some(id) = user_id {
                    nocturne_session::forget_account(&url, &id);
                }
            })
            .await;
        });
        self.qc_secret = None;
        self.dialog = None;
        self.st.signed_out();
    }

    /// "Add another account": show the login form of the same server. Nothing is signed out or
    /// forgotten; the current account stays remembered and can be switched back to.
    pub fn add_account(&mut self) {
        self.from_picker = false;
        self.qc_secret = None;
        self.dialog = None;
        self.st.signed_out();
    }

    // ---- own profile dialogs ----

    fn open_dialog(&mut self, kind: DlgKind, client: UserClient, form: Form) {
        self.dialog = Some(Dialog {
            kind,
            client,
            form,
            busy: false,
            msg: None,
        });
    }

    fn one_field(label: &'static str, value: String) -> Form {
        let f = Fld {
            label,
            value,
            ..Default::default()
        };
        Form::new(vec![f], 2)
    }

    /// "Profile image": the path of an image file on this computer.
    pub fn open_profile_image(&mut self, client: UserClient) {
        let f = Self::one_field(
            "Path of the image file (PNG, JPEG, WebP or GIF, up to 10 MB)",
            String::new(),
        );
        self.open_dialog(DlgKind::ProfileImage, client, f);
    }

    pub fn open_remove_image(&mut self, client: UserClient) {
        self.open_dialog(DlgKind::RemoveImage, client, Form::new(vec![], 2));
    }

    /// "Display language": `current` is what the server has for this user now.
    pub fn open_language(&mut self, client: UserClient, current: &str) {
        let f = Self::one_field(
            "Language code, such as en-US or de (empty = default)",
            current.to_string(),
        );
        self.open_dialog(DlgKind::Language, client, f);
    }

    pub fn open_password(&mut self, client: UserClient) {
        let f = |label| Fld {
            label,
            secret: true,
            ..Default::default()
        };
        let form = Form::new(
            vec![
                f("Current password"),
                f("New password"),
                f("Repeat new password"),
            ],
            2,
        );
        self.open_dialog(DlgKind::Password, client, form);
    }

    /// "Authorize a device": the code another device shows on its Quick Connect screen.
    pub fn open_authorize(&mut self, client: UserClient) {
        let f = Self::one_field("Code shown on the other device", String::new());
        self.open_dialog(DlgKind::Authorize, client, f);
    }

    fn dialog_done(&mut self, kind: DlgKind, r: Result<(), &'static str>) {
        match r {
            Ok(()) => {
                self.dialog = None;
                self.events.push(Event::Notice(match kind {
                    DlgKind::Language => "Display language saved.",
                    DlgKind::RemoveImage => "Profile image removed.",
                    DlgKind::ProfileImage => "Profile image updated.",
                    DlgKind::Authorize => "Device approved.",
                    DlgKind::Password => "Password changed.",
                }));
                if matches!(kind, DlgKind::ProfileImage | DlgKind::RemoveImage) {
                    self.events.push(Event::Reload);
                }
            }
            Err(m) => {
                if let Some(d) = self.dialog.as_mut() {
                    d.busy = false;
                    d.msg = Some(m);
                }
            }
        }
    }

    fn authorize_confirm(&mut self) -> Task<Msg> {
        let Some(d) = self.dialog.as_mut().filter(|d| !d.busy) else {
            return Task::none();
        };
        let code = d.form.fields[0].value.trim().to_string();
        if code.is_empty() {
            d.msg = Some("Type the code the other device is showing.");
            return Task::none();
        }
        d.busy = true;
        d.msg = None;
        let c = d.client.clone();
        run(async move { c.quick_connect_authorize(&code).await }, |r| {
            Msg::DialogDone(
                DlgKind::Authorize,
                r.map_err(|e| match e {
                    Error::Status {
                        status: 400 | 404, ..
                    } => "That code wasn't found. Check it and try again.",
                    _ => "Couldn't approve the device. Try again.",
                }),
            )
        })
    }

    fn profile_confirm(&mut self) -> Task<Msg> {
        let Some(d) = self.dialog.as_mut().filter(|d| !d.busy) else {
            return Task::none();
        };
        let value = d.form.value(0).to_string();
        let kind = d.kind;
        if kind == DlgKind::ProfileImage && value.is_empty() {
            d.msg = Some("Type the path of an image file.");
            return Task::none();
        }
        d.busy = true;
        d.msg = None;
        let c = d.client.clone();
        run(
            async move {
                match kind {
                    DlgKind::ProfileImage => {
                        let bytes = tokio::task::spawn_blocking(move || read_image(&value))
                            .await
                            .map_err(|_| ProfileFail::File)??;
                        c.set_own_image(&bytes)
                            .await
                            .map_err(|_| ProfileFail::Server)
                    }
                    // Minted here, after the user pressed the dialog's confirm button.
                    DlgKind::RemoveImage => c
                        .remove_own_image(UserConfirmed::i_have_asked_the_user())
                        .await
                        .map_err(|_| ProfileFail::Server),
                    _ => c.set_display_language(&value).await.map_err(|e| match e {
                        Error::Config(_) => ProfileFail::Language,
                        _ => ProfileFail::Server,
                    }),
                }
            },
            move |r| Msg::DialogDone(kind, r.map_err(ProfileFail::text)),
        )
    }

    /// Enter on "Change password": the only place its confirmation token is minted.
    fn pw_confirm(&mut self) -> Task<Msg> {
        match self.dialog.as_ref().map(|d| d.kind) {
            Some(DlgKind::Authorize) => return self.authorize_confirm(),
            Some(DlgKind::ProfileImage | DlgKind::RemoveImage | DlgKind::Language) => {
                return self.profile_confirm();
            }
            _ => {}
        }
        let Some(d) = self.dialog.as_mut().filter(|d| !d.busy) else {
            return Task::none();
        };
        let (cur, new) = (
            d.form.fields[0].value.clone(),
            d.form.fields[1].value.clone(),
        );
        if cur.is_empty() || new.is_empty() {
            d.msg = Some("Fill in both your current and your new password.");
            return Task::none();
        }
        if new != d.form.fields[2].value {
            d.msg = Some("The new passwords don't match.");
            return Task::none();
        }
        d.busy = true;
        d.msg = None;
        let c = d.client.clone();
        run(
            async move {
                c.change_own_password(&cur, &new, UserConfirmed::i_have_asked_the_user())
                    .await
            },
            |r| match r {
                Ok(()) => Msg::DialogDone(DlgKind::Password, Ok(())),
                Err(Error::Status {
                    status: 400 | 401 | 403,
                    ..
                }) => Msg::PwFailed("That current password wasn't accepted."),
                Err(_) => Msg::PwFailed("Couldn't change the password. Try again."),
            },
        )
    }

    fn dialog_press(&mut self, n: usize) -> Task<Msg> {
        if n == 0 {
            self.dialog = None;
            Task::none()
        } else {
            self.pw_confirm()
        }
    }

    // ---- keyboard ----

    /// Keyboard entry (the root calls this while an account screen or the dialog is up).
    pub fn on_key(&mut self, k: &Key) -> Task<Msg> {
        let key = k.name.as_str();
        let paste = k.is_paste();
        let ch = k.typed();
        if self.dialog.is_some() {
            if key == "escape" {
                self.dialog = None;
            } else if paste {
                return clipboard::read().map(Msg::Paste);
            } else if let Some(d) = self.dialog.as_mut() {
                match d.form.key(key, ch) {
                    // Enter on the last input moves to the button; only Enter on the button confirms.
                    Ev::Submit => d.form.focus = d.form.fields.len() + 1,
                    Ev::Press(n) => return self.dialog_press(n),
                    Ev::Stay => {}
                }
            }
            return Task::none();
        }
        if self.st.busy && key != "escape" {
            return Task::none();
        }
        match self.st.stage {
            Stage::Active => {}
            Stage::Connect
                if key == "delete" && self.st.connect.focus > self.st.connect.fields.len() =>
            {
                self.forget(self.st.connect.focus - self.st.connect.fields.len() - 1);
            }
            Stage::Connect => {
                if paste {
                    return clipboard::read().map(Msg::Paste);
                }
                match self.st.connect.key(key, ch) {
                    Ev::Submit | Ev::Press(0) => return self.connect_go(),
                    Ev::Press(n) => return self.pick(n - 1),
                    Ev::Stay => {}
                }
            }
            Stage::Login if self.st.qc != Qc::Off => {
                if matches!(key, "escape" | "enter") {
                    self.qc_stop();
                }
            }
            Stage::Login if key == "escape" => self.back_to_servers(),
            Stage::Login => {
                if paste {
                    return clipboard::read().map(Msg::Paste);
                }
                let last = self.st.login.buttons - 1;
                match self.st.login.key(key, ch) {
                    Ev::Submit | Ev::Press(0) => return self.login_go(),
                    Ev::Press(n) if n == last => self.back_to_servers(),
                    Ev::Press(_) => return self.qc_go(),
                    Ev::Stay => {}
                }
            }
        }
        Task::none()
    }

    pub fn view(&self, now: Instant) -> Element<'_, Msg> {
        view::view(self, now)
    }

    /// The dialog layer alone (drawn over a signed-in page); `None` when no dialog is open.
    pub fn dialog_view(&self) -> Option<Element<'_, Msg>> {
        view::dialog(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes_the_dialog_and_enter_walks_fields_then_buttons() {
        let mut a = Accounts {
            st: Account::new(vec![], None),
            servers: Servers::default(),
            qc_secret: None,
            dialog: None,
            from_picker: false,
            events: vec![],
        };
        a.dialog = Some(Dialog {
            kind: DlgKind::Authorize,
            client: UserClient::anonymous("http://127.0.0.1:1").unwrap(),
            form: Accounts::one_field("c", String::new()),
            busy: false,
            msg: None,
        });
        let key = |n: &str| Key {
            name: n.into(),
            ..Default::default()
        };
        let _ = a.on_key(&key("enter"));
        assert_eq!(
            a.dialog.as_ref().unwrap().form.focus,
            2,
            "last input -> confirm button"
        );
        let _ = a.on_key(&key("escape"));
        assert!(a.dialog.is_none());
    }
}
