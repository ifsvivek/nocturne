//! Accounts: connect, sign in (password or Quick Connect), restore, sign out, change password.
//! One GPUI entity owns the state (`state::Account`); `Root` forwards keys to it while an account
//! screen is up and reacts to its events. All I/O goes through the tokio bridge, never the UI thread.
pub mod picker;
pub use nocturne_core::account::state;
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
    /// A playlist changed: the page showing it should load again.
    Reload,
    /// The playlist on screen was deleted: leave its page.
    Deleted,
    /// Esc on the sign-in form that the profile picker opened: show the picker again.
    BackToPicker,
}

impl EventEmitter<Event> for Accounts {}

/// What the account dialog is for.
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
    /// New playlist, optionally with the items it is made from.
    NewPlaylist,
    RenamePlaylist,
    DeletePlaylist,
    /// Add items to one of the user's playlists by name; an unknown name makes a new one.
    AddToPlaylist,
}

/// What a playlist dialog acts on.
#[derive(Clone, Default)]
pub enum Target {
    #[default]
    None,
    /// An existing playlist: (id, name).
    Playlist(String, String),
    /// Items to put in a playlist, and the playlists the user owns: (id, name).
    Items(Vec<String>, Vec<(String, String)>),
}

struct PwDialog {
    kind: DlgKind,
    client: UserClient,
    form: Form,
    busy: bool,
    msg: Option<&'static str>,
    target: Target,
    /// Extra line under the dialog's text (the user's playlists, for example).
    hint: Option<String>,
}

pub struct Accounts {
    st: Account,
    servers: Servers,
    qc_secret: Option<String>,
    dialog: Option<PwDialog>,
    /// The sign-in form was opened from the profile picker, so Esc returns there.
    from_picker: bool,
}

/// Why a profile change did not go through (calm one-liners for the dialog).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ProfileFail {
    File,
    Image,
    Language,
    NotOwner,
    Server,
}

fn playlist_fail(e: Error) -> ProfileFail {
    match e {
        Error::Status { status: 403, .. } => ProfileFail::NotOwner,
        Error::Config(_) => ProfileFail::Language,
        _ => ProfileFail::Server,
    }
}

impl ProfileFail {
    fn text(self) -> &'static str {
        match self {
            ProfileFail::File => "That file couldn't be read. Check the path.",
            ProfileFail::Image => "That isn't a PNG, JPEG, WebP or GIF image of at most 10 MB.",
            ProfileFail::Language => "The server doesn't offer that language code.",
            ProfileFail::NotOwner => "You can only change playlists you own.",
            ProfileFail::Server => "Couldn't save that. Check the server and try again.",
        }
    }
}

/// The file's bytes, if it looks like an acceptable image (checked again before upload).
fn read_image(path: &str) -> Result<Vec<u8>, ProfileFail> {
    let p = std::path::Path::new(path);
    let len = std::fs::metadata(p).map_err(|_| ProfileFail::File)?.len();
    if len > nocturne_jellyfin_user::MAX_IMAGE_BYTES as u64 {
        return Err(ProfileFail::Image);
    }
    let bytes = std::fs::read(p).map_err(|_| ProfileFail::File)?;
    nocturne_jellyfin_user::image_mime(&bytes).map_err(|_| ProfileFail::Image)?;
    Ok(bytes)
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
async fn probe(url: String, restore: bool) -> Result<(Probe, Option<Session>), ConnectFail> {
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
    if !restore {
        return Ok((p, None));
    }
    match tokio::task::spawn_blocking(move || nocturne_session::load(&u)).await {
        Ok(Ok(Some(s))) => {
            let me = UserClient::from_session(&url, &s).map_err(|e| classify(&e))?;
            match me.me(s).await {
                Ok(s) => {
                    p.restored = true;
                    // Also list it as a remembered account (older installs only had the active one).
                    let (u, s2) = (url.clone(), s.clone());
                    let _ = tokio::task::spawn_blocking(move || {
                        nocturne_session::remember_login(&u, &s2)
                    })
                    .await;
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
        tokio::task::spawn_blocking(move || nocturne_session::remember_login(&url, &s)).await,
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
            from_picker: false,
        };
        if let Some(r) = last.and_then(|u| me.st.start_restore(&u)) {
            me.check(r, true, cx);
        }
        me
    }

    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn keyring_down(&self) -> bool {
        self.st.keyring_down
    }

    /// Sign-in form for the profile picker: no stored session is restored, `name` is prefilled.
    pub fn sign_in_form(&mut self, url: &str, name: Option<String>, cx: &mut Context<Self>) {
        self.qc_secret = None;
        self.dialog = None;
        self.from_picker = true;
        if let Some(r) = self.st.start_form(url, name) {
            self.check(r, false, cx);
        }
        cx.notify();
    }

    fn check(&mut self, (epoch, url): (u64, String), restore: bool, cx: &mut Context<Self>) {
        let u = url.clone();
        bridge::run(cx, probe(u, restore), move |a, r, cx| {
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
        self.from_picker = false;
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
            self.check(r, true, cx);
        }
        cx.notify();
    }

    fn pick(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.st.pick_server(i) {
            self.check(r, true, cx);
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
        if std::mem::take(&mut self.from_picker) {
            self.st.busy = false;
            self.st.bump_epoch();
            return cx.emit(Event::BackToPicker);
        }
        self.switch_server(cx);
    }

    /// Show the address screen again (the stored login for the current server stays in the keyring).
    pub fn switch_server(&mut self, cx: &mut Context<Self>) {
        self.from_picker = false;
        self.qc_secret = None;
        self.servers = Servers::load().unwrap_or_default();
        self.st.switch_server(self.servers.servers.clone());
        cx.notify();
    }

    /// Remove remembered server `i` and its saved login from this device.
    pub fn forget(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(url) = self.st.servers.get(i).cloned() else {
            return;
        };
        self.servers = Servers::load().unwrap_or_default();
        self.servers.forget(&url);
        let _ = self.servers.save(); // calm: a failed save only brings the entry back next launch
        bridge::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || nocturne_session::delete(&url)).await;
        });
        self.st.servers.remove(i);
        let typed = self.st.connect.fields[0].value.clone();
        self.st.switch_server(self.servers.servers.clone());
        self.st.connect.fields[0].value = typed;
        cx.emit(Event::Notice("Server forgotten."));
        cx.notify();
    }

    /// The server rejected the stored login mid-session: forget it, show the login form with a
    /// calm note.
    pub fn session_expired(&mut self, user_id: Option<String>, cx: &mut Context<Self>) {
        self.from_picker = false;
        let url = self.st.url.clone();
        bridge::spawn(async move {
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
        cx.notify();
    }

    /// Log out on the server, delete the keyring entry, show the login form.
    pub fn sign_out(
        &mut self,
        client: Option<UserClient>,
        user_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.from_picker = false;
        let url = self.st.url.clone();
        bridge::spawn(async move {
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
        cx.notify();
    }

    /// "Add another account": show the login form of the same server. Nothing is signed out or
    /// forgotten; the current account stays remembered and can be switched back to.
    pub fn add_account(&mut self, cx: &mut Context<Self>) {
        self.from_picker = false;
        self.qc_secret = None;
        self.dialog = None;
        self.st.signed_out();
        cx.notify();
    }

    // ---- own profile ----

    fn open_dialog(
        &mut self,
        kind: DlgKind,
        client: UserClient,
        form: Form,
        cx: &mut Context<Self>,
    ) {
        self.dialog = Some(PwDialog {
            kind,
            client,
            form,
            busy: false,
            msg: None,
            target: Target::None,
            hint: None,
        });
        cx.notify();
    }

    /// Playlist dialogs: a one-line form plus what it acts on.
    pub fn open_playlist(
        &mut self,
        kind: DlgKind,
        client: UserClient,
        target: Target,
        cx: &mut Context<Self>,
    ) {
        let (label, value, hint) = match (&kind, &target) {
            (DlgKind::RenamePlaylist, Target::Playlist(_, n)) => ("New name", n.clone(), None),
            (DlgKind::DeletePlaylist, Target::Playlist(_, n)) => (
                "Type the playlist name to confirm",
                String::new(),
                Some(format!(
                    "This deletes {n} for good. The items in it are not touched."
                )),
            ),
            (DlgKind::AddToPlaylist, Target::Items(_, own)) => (
                "Playlist name",
                String::new(),
                Some(if own.is_empty() {
                    "You have no playlists yet; a new one is created.".to_string()
                } else {
                    format!(
                        "Yours: {}.",
                        own.iter()
                            .map(|o| o.1.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }),
            ),
            _ => ("Playlist name", String::new(), None),
        };
        let f = Fld {
            label,
            value,
            ..Default::default()
        };
        self.dialog = Some(PwDialog {
            kind,
            client,
            form: Form::new(vec![f], 2),
            busy: false,
            msg: None,
            target,
            hint,
        });
        cx.notify();
    }

    /// "Profile image": the path of an image file on this computer.
    pub fn open_profile_image(&mut self, client: UserClient, cx: &mut Context<Self>) {
        let f = Fld {
            label: "Path of the image file (PNG, JPEG, WebP or GIF, up to 10 MB)",
            ..Default::default()
        };
        self.open_dialog(DlgKind::ProfileImage, client, Form::new(vec![f], 2), cx);
    }

    pub fn open_remove_image(&mut self, client: UserClient, cx: &mut Context<Self>) {
        self.open_dialog(DlgKind::RemoveImage, client, Form::new(vec![], 2), cx);
    }

    /// "Display language": `current` is what the server has for this user now.
    pub fn open_language(&mut self, client: UserClient, current: &str, cx: &mut Context<Self>) {
        let f = Fld {
            label: "Language code, such as en-US or de (empty = default)",
            value: current.to_string(),
            ..Default::default()
        };
        self.open_dialog(DlgKind::Language, client, Form::new(vec![f], 2), cx);
    }

    fn profile_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dialog.as_mut().filter(|d| !d.busy) else {
            return;
        };
        let value = d.form.value(0).to_string();
        let (kind, target) = (d.kind, d.target.clone());
        if kind == DlgKind::ProfileImage && value.is_empty() {
            d.msg = Some("Type the path of an image file.");
            return;
        }
        if matches!(
            kind,
            DlgKind::NewPlaylist | DlgKind::RenamePlaylist | DlgKind::AddToPlaylist
        ) && value.is_empty()
        {
            d.msg = Some("A playlist needs a name.");
            return;
        }
        if let (DlgKind::DeletePlaylist, Target::Playlist(_, n)) = (kind, &target)
            && value != *n
        {
            d.msg = Some("Type the playlist name exactly to confirm.");
            return;
        }
        d.busy = true;
        d.msg = None;
        let c = d.client.clone();
        bridge::run(
            cx,
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
                    DlgKind::RemoveImage => c
                        .remove_own_image(UserConfirmed::i_have_asked_the_user())
                        .await
                        .map_err(|_| ProfileFail::Server),
                    DlgKind::NewPlaylist => c
                        .create_playlist(&value, &[])
                        .await
                        .map(drop)
                        .map_err(playlist_fail),
                    DlgKind::RenamePlaylist => match &target {
                        Target::Playlist(id, _) => {
                            c.rename_playlist(id, &value).await.map_err(playlist_fail)
                        }
                        _ => Err(ProfileFail::Server),
                    },
                    DlgKind::DeletePlaylist => match &target {
                        Target::Playlist(id, _) => {
                            c.delete_playlist(id).await.map_err(playlist_fail)
                        }
                        _ => Err(ProfileFail::Server),
                    },
                    DlgKind::AddToPlaylist => match &target {
                        Target::Items(ids, own) => {
                            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                            match own.iter().find(|o| o.1.eq_ignore_ascii_case(&value)) {
                                Some((pid, _)) => c.add_to_playlist(pid, &ids).await,
                                None => c.create_playlist(&value, &ids).await.map(drop),
                            }
                            .map_err(playlist_fail)
                        }
                        _ => Err(ProfileFail::Server),
                    },
                    _ => c.set_display_language(&value).await.map_err(|e| match e {
                        Error::Config(_) => ProfileFail::Language,
                        _ => ProfileFail::Server,
                    }),
                }
            },
            move |a, r, cx| match r {
                Ok(()) => {
                    a.dialog = None;
                    cx.emit(Event::Notice(match kind {
                        DlgKind::Language => "Display language saved.",
                        DlgKind::RemoveImage => "Profile image removed.",
                        DlgKind::NewPlaylist => "Playlist created.",
                        DlgKind::RenamePlaylist => "Playlist renamed.",
                        DlgKind::DeletePlaylist => "Playlist deleted.",
                        DlgKind::AddToPlaylist => "Added to the playlist.",
                        DlgKind::ProfileImage => "Profile image updated.",
                        _ => "Saved.",
                    }));
                    cx.emit(if kind == DlgKind::DeletePlaylist {
                        Event::Deleted
                    } else {
                        Event::Reload
                    });
                }
                Err(e) => {
                    if let Some(d) = a.dialog.as_mut() {
                        d.busy = false;
                        d.msg = Some(e.text());
                    }
                }
            },
        );
    }

    // ---- change password ----

    pub fn open_password(&mut self, client: UserClient, cx: &mut Context<Self>) {
        let f = |label| Fld {
            label,
            secret: true,
            ..Default::default()
        };
        self.dialog = Some(PwDialog {
            kind: DlgKind::Password,
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
            target: Target::None,
            hint: None,
        });
        cx.notify();
    }

    /// "Authorize a device": the code another device shows on its Quick Connect screen.
    pub fn open_authorize(&mut self, client: UserClient, cx: &mut Context<Self>) {
        self.dialog = Some(PwDialog {
            kind: DlgKind::Authorize,
            client,
            form: Form::new(
                vec![Fld {
                    label: "Code shown on the other device",
                    ..Default::default()
                }],
                2,
            ),
            busy: false,
            msg: None,
            target: Target::None,
            hint: None,
        });
        cx.notify();
    }

    fn authorize_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dialog.as_mut().filter(|d| !d.busy) else {
            return;
        };
        let code = d.form.fields[0].value.trim().to_string();
        if code.is_empty() {
            d.msg = Some("Type the code the other device is showing.");
            return;
        }
        d.busy = true;
        d.msg = None;
        let c = d.client.clone();
        bridge::run(
            cx,
            async move { c.quick_connect_authorize(&code).await },
            |a, r, cx| match r {
                Ok(()) => {
                    a.dialog = None;
                    cx.emit(Event::Notice("Device approved."));
                }
                Err(e) => {
                    if let Some(d) = a.dialog.as_mut() {
                        d.busy = false;
                        d.msg = Some(match e {
                            Error::Status {
                                status: 400 | 404, ..
                            } => "That code wasn't found. Check it and try again.",
                            _ => "Couldn't approve the device. Try again.",
                        });
                    }
                }
            },
        );
    }

    /// Enter on "Change password": the only place the confirmation token is minted.
    fn pw_confirm(&mut self, cx: &mut Context<Self>) {
        match self.dialog.as_ref().map(|d| d.kind) {
            Some(DlgKind::Authorize) => return self.authorize_confirm(cx),
            Some(
                DlgKind::ProfileImage
                | DlgKind::RemoveImage
                | DlgKind::Language
                | DlgKind::NewPlaylist
                | DlgKind::RenamePlaylist
                | DlgKind::DeletePlaylist
                | DlgKind::AddToPlaylist,
            ) => return self.profile_confirm(cx),
            _ => {}
        }
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
            Stage::Connect
                if key == "delete" && self.st.connect.focus > self.st.connect.fields.len() =>
            {
                self.forget(self.st.connect.focus - self.st.connect.fields.len() - 1, cx);
            }
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
    use super::{ConnectFail, Error, Poll, ProfileFail, classify, poll_of, read_image};

    #[test]
    fn profile_images_are_checked_before_upload() {
        let dir = std::env::temp_dir().join(format!("nocturne-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.png");
        std::fs::write(&png, [0x89, b'P', b'N', b'G', 0, 1]).unwrap();
        let txt = dir.join("a.txt");
        std::fs::write(&txt, "hello").unwrap();
        assert!(read_image(png.to_str().unwrap()).is_ok());
        assert_eq!(read_image(txt.to_str().unwrap()), Err(ProfileFail::Image));
        assert_eq!(read_image("/nonexistent/x.png"), Err(ProfileFail::File));
        std::fs::remove_dir_all(dir).unwrap();
    }

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
