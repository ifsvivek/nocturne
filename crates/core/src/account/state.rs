//! Account flow as a pure state machine: Connect -> Login (password or Quick Connect) -> Active,
//! plus session restore and sign out. No I/O here. Every request carries the `epoch` it was made
//! in; any navigation bumps the epoch, so a late answer for a screen the user already left is dropped.

/// One text input (password fields are masked when drawn).
#[derive(Clone, Default)]
pub struct Fld {
    pub label: &'static str,
    pub value: String,
    pub secret: bool,
}

/// Inputs followed by buttons; `focus` indexes inputs first, then buttons.
#[derive(Clone, Default)]
pub struct Form {
    pub fields: Vec<Fld>,
    pub buttons: usize,
    pub focus: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Ev {
    Stay,
    /// Enter on the last input.
    Submit,
    /// Enter on button `n`.
    Press(usize),
}

impl Form {
    pub fn new(fields: Vec<Fld>, buttons: usize) -> Self {
        Self {
            fields,
            buttons,
            focus: 0,
        }
    }

    fn slots(&self) -> usize {
        self.fields.len() + self.buttons
    }

    pub fn value(&self, i: usize) -> &str {
        self.fields.get(i).map_or("", |f| f.value.trim())
    }

    /// Moves between inputs and buttons; typing, backspace and paste belong to the text inputs.
    pub fn key(&mut self, key: &str) -> Ev {
        let n = self.slots().max(1);
        match key {
            "tab" | "down" => self.focus = (self.focus + 1) % n,
            "up" => self.focus = (self.focus + n - 1) % n,
            "enter" if self.focus < self.fields.len() => {
                if self.focus + 1 < self.fields.len() {
                    self.focus += 1;
                } else {
                    return Ev::Submit;
                }
            }
            "enter" => return Ev::Press(self.focus - self.fields.len()),
            _ => {}
        }
        Ev::Stay
    }

    pub fn clear(&mut self) {
        self.fields.iter_mut().for_each(|f| f.value.clear());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Connect,
    Login,
    Active,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectFail {
    Unreachable,
    NotServer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginFail {
    Invalid,
    Unreachable,
}

/// What a connect check learned (the session, when the keyring had a working one, is handed on
/// by the caller; the state machine only needs to know it exists).
#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub name: String,
    pub quick_connect: bool,
    /// The administrator's login disclaimer (public branding), plain text.
    pub disclaimer: Option<String>,
    pub restored: bool,
    /// A stored session was rejected (401) and has been deleted.
    pub expired: bool,
    pub keyring_down: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Qc {
    Off,
    Starting,
    Waiting(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    Pending,
    Authorized,
    /// The server no longer knows the code (expired or cancelled elsewhere).
    Gone,
    /// Network hiccup: keep trying a few times.
    Hiccup,
}

#[derive(Debug, PartialEq, Eq)]
pub enum QcNext {
    Wait,
    Exchange,
    Stop,
}

pub const MSG_URL: &str = "Enter your server address, for example http://192.168.1.20:8096";
pub const MSG_UNREACHABLE: &str =
    "Couldn't reach that server. Check the address and that the server is running.";
pub const MSG_NOT_SERVER: &str = "That address doesn't look like a media server.";
pub const MSG_USER: &str = "Enter your username.";
pub const MSG_CREDS: &str = "That username or password didn't match. Try again.";
pub const MSG_LOGIN_NET: &str = "Couldn't reach the server. Try again in a moment.";
pub const MSG_EXPIRED: &str = "Your session ended. Please sign in again.";
pub const MSG_QC_OFF: &str = "Quick Connect isn't available right now.";
pub const MSG_QC_GONE: &str = "That code expired. Start Quick Connect again for a new one.";
pub const KEYRING_NOTE: &str =
    "No system keyring found. You'll be asked to sign in again next time.";
const QC_HICCUPS: u8 = 4;

pub struct Account {
    pub stage: Stage,
    pub epoch: u64,
    pub servers: Vec<String>,
    /// The server in use (set once a check passed).
    pub url: String,
    pub server_name: String,
    pub disclaimer: Option<String>,
    pub connect: Form,
    pub login: Form,
    pub busy: bool,
    /// The busy check is a session restore at launch.
    pub restoring: bool,
    pub msg: Option<&'static str>,
    pub qc: Qc,
    pub qc_available: bool,
    pub keyring_down: bool,
    /// Username to put in the sign-in form once the server has been checked (profile picker).
    prefill: Option<String>,
    hiccups: u8,
}

/// `192.168.1.5:8096`, `jelly.lan`, `https://x/` -> a clean `scheme://host[:port]`, or `None`.
pub fn normalize_url(raw: &str) -> Option<String> {
    let t = raw.trim();
    let (scheme, rest) = match t.split_once("://") {
        Some((s, r)) if matches!(s.to_ascii_lowercase().as_str(), "http" | "https") => {
            (s.to_ascii_lowercase(), r)
        }
        Some(_) => return None,
        None => ("http".into(), t),
    };
    let rest = rest.trim_end_matches('/');
    let host = rest.split('/').next().unwrap_or("");
    (!host.is_empty() && !host.starts_with(':') && !t.contains(char::is_whitespace))
        .then(|| format!("{scheme}://{rest}"))
}

impl Account {
    pub fn new(servers: Vec<String>, last: Option<String>) -> Self {
        let mut a = Self {
            stage: Stage::Connect,
            epoch: 0,
            servers,
            url: String::new(),
            server_name: String::new(),
            disclaimer: None,
            connect: Form::default(),
            login: Form::default(),
            busy: false,
            restoring: false,
            msg: None,
            qc: Qc::Off,
            qc_available: false,
            keyring_down: false,
            prefill: None,
            hiccups: 0,
        };
        a.reset_connect(last.as_deref().unwrap_or(""));
        a
    }

    fn reset_connect(&mut self, url: &str) {
        let url_f = Fld {
            label: "Server address",
            value: url.into(),
            secret: false,
        };
        self.connect = Form::new(vec![url_f], 1 + self.servers.len());
    }

    pub fn bump_epoch(&mut self) {
        self.bump();
    }

    fn bump(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    /// Start a connect check for the typed address. `None` (with a message) if it isn't an address.
    pub fn start_connect(&mut self) -> Option<(u64, String)> {
        let Some(url) = normalize_url(self.connect.value(0)) else {
            self.msg = Some(MSG_URL);
            return None;
        };
        self.begin_check(url, false)
    }

    /// Sign-in form of `url` for the profile picker: check the server, then show the form with
    /// `name` already typed (empty for a new profile).
    pub fn start_form(&mut self, url: &str, name: Option<String>) -> Option<(u64, String)> {
        self.prefill = name;
        self.begin_check(url.to_string(), false)
    }

    /// Launch: try the last used server (restore its session if the keyring has one).
    pub fn start_restore(&mut self, url: &str) -> Option<(u64, String)> {
        self.begin_check(url.to_string(), true)
    }

    fn begin_check(&mut self, url: String, restoring: bool) -> Option<(u64, String)> {
        self.stage = Stage::Connect;
        self.connect.fields[0].value = url.clone();
        self.busy = true;
        self.restoring = restoring;
        self.msg = None;
        Some((self.bump(), url))
    }

    /// Pick a remembered server (button `i` after the "Connect" button).
    pub fn pick_server(&mut self, i: usize) -> Option<(u64, String)> {
        let url = self.servers.get(i)?.clone();
        self.begin_check(url, false)
    }

    /// Result of a connect check. Returns `true` when it applied (not stale).
    /// A restored session means the caller should now emit "signed in" (`stage` becomes Active).
    pub fn probed(&mut self, epoch: u64, r: Result<Probe, ConnectFail>, url: &str) -> bool {
        if epoch != self.epoch {
            return false;
        }
        self.busy = false;
        self.restoring = false;
        match r {
            Ok(p) => {
                self.url = url.into();
                self.server_name = p.name;
                self.disclaimer = p.disclaimer;
                self.qc_available = p.quick_connect;
                self.keyring_down |= p.keyring_down;
                if p.restored {
                    self.stage = Stage::Active;
                } else {
                    self.msg = p.expired.then_some(MSG_EXPIRED);
                    self.enter_login();
                }
            }
            Err(e) => {
                self.msg = Some(match e {
                    ConnectFail::Unreachable => MSG_UNREACHABLE,
                    ConnectFail::NotServer => MSG_NOT_SERVER,
                });
            }
        }
        true
    }

    fn enter_login(&mut self) {
        let prefill = self.prefill.take();
        self.stage = Stage::Login;
        self.qc = Qc::Off;
        self.bump(); // drop any Quick Connect poll from a previous visit
        self.login = Form::new(
            vec![
                Fld {
                    label: "Username",
                    ..Default::default()
                },
                Fld {
                    label: "Password",
                    secret: true,
                    ..Default::default()
                },
            ],
            2 + self.qc_available as usize,
        );
        if let Some(n) = prefill {
            self.login.fields[0].value = n;
            self.login.focus = 1; // straight to the password
            self.msg = Some(MSG_EXPIRED);
        }
    }

    /// Sign-in button / Enter on the password.
    pub fn start_login(&mut self) -> Option<(u64, String, String, String)> {
        if self.busy {
            return None;
        }
        let user = self.login.value(0).to_string();
        if user.is_empty() {
            self.msg = Some(MSG_USER);
            self.login.focus = 0;
            return None;
        }
        // Passwords are not trimmed.
        let pass = self.login.fields[1].value.clone();
        self.busy = true;
        self.msg = None;
        Some((self.bump(), self.url.clone(), user, pass))
    }

    pub fn logged_in(&mut self, epoch: u64, r: Result<(), LoginFail>) -> bool {
        if epoch != self.epoch {
            return false;
        }
        self.busy = false;
        match r {
            Ok(()) => {
                self.stage = Stage::Active;
                self.qc = Qc::Off;
                self.login.clear();
                self.msg = None;
            }
            Err(e) => {
                self.msg = Some(match e {
                    LoginFail::Invalid => MSG_CREDS,
                    LoginFail::Unreachable => MSG_LOGIN_NET,
                });
                self.login.fields[1].value.clear();
                self.login.focus = 1;
                self.qc = Qc::Off;
            }
        }
        true
    }

    // ---- Quick Connect ----

    pub fn qc_start(&mut self) -> Option<u64> {
        if !self.qc_available || self.busy {
            return None;
        }
        self.msg = None;
        self.qc = Qc::Starting;
        self.hiccups = 0;
        Some(self.bump())
    }

    /// The server issued a code (shown big); `ok == false` means it refused.
    pub fn qc_code(&mut self, epoch: u64, code: Option<String>) -> bool {
        if epoch != self.epoch || self.qc != Qc::Starting {
            return false;
        }
        match code {
            Some(c) => self.qc = Qc::Waiting(c),
            None => {
                self.qc = Qc::Off;
                self.msg = Some(MSG_QC_OFF);
            }
        }
        true
    }

    pub fn qc_polled(&mut self, epoch: u64, p: Poll) -> QcNext {
        if epoch != self.epoch || !matches!(self.qc, Qc::Waiting(_)) {
            return QcNext::Stop;
        }
        match p {
            Poll::Pending => {
                self.hiccups = 0;
                QcNext::Wait
            }
            Poll::Authorized => {
                self.busy = true;
                QcNext::Exchange
            }
            Poll::Hiccup => {
                self.hiccups += 1;
                if self.hiccups < QC_HICCUPS {
                    return QcNext::Wait;
                }
                self.qc = Qc::Off;
                self.msg = Some(MSG_LOGIN_NET);
                QcNext::Stop
            }
            Poll::Gone => {
                self.qc = Qc::Off;
                self.msg = Some(MSG_QC_GONE);
                QcNext::Stop
            }
        }
    }

    pub fn qc_cancel(&mut self) {
        self.qc = Qc::Off;
        self.busy = false;
        self.msg = None;
        self.bump();
    }

    // ---- navigation ----

    /// Back to the address screen (switch server). The stored session stays in the keyring.
    pub fn switch_server(&mut self, servers: Vec<String>) {
        self.servers = servers;
        self.bump();
        self.busy = false;
        self.qc = Qc::Off;
        self.msg = None;
        self.stage = Stage::Connect;
        self.reset_connect(&self.url.clone());
    }

    /// After sign out: the login form of the same server.
    pub fn signed_out(&mut self) {
        self.msg = None;
        self.enter_login();
        self.busy = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct() -> Account {
        Account::new(vec!["http://a:1".into(), "http://b:2".into()], None)
    }

    fn type_into(a: &mut Account, text: &str) {
        a.connect.fields[0].value.push_str(text);
    }

    fn probe(qc: bool) -> Probe {
        Probe {
            name: "Home".into(),
            quick_connect: qc,
            ..Default::default()
        }
    }

    #[test]
    fn url_normalisation() {
        assert_eq!(
            normalize_url(" 192.168.1.5:8096/ ").as_deref(),
            Some("http://192.168.1.5:8096")
        );
        assert_eq!(
            normalize_url("HTTPS://jelly.lan").as_deref(),
            Some("https://jelly.lan")
        );
        assert_eq!(
            normalize_url("http://h:1/jf").as_deref(),
            Some("http://h:1/jf")
        );
        for bad in ["", "   ", "ftp://x", "http://", "http://:80", "a b"] {
            assert_eq!(normalize_url(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn connect_then_login_then_active() {
        let mut a = acct();
        assert_eq!(a.start_connect(), None, "empty address is a calm message");
        assert_eq!(a.msg, Some(MSG_URL));
        type_into(&mut a, "10.0.0.2:8096");
        let (e, url) = a.start_connect().unwrap();
        assert_eq!(url, "http://10.0.0.2:8096");
        assert!(a.busy && a.msg.is_none());
        assert!(a.probed(e, Ok(probe(true)), &url));
        assert_eq!(
            (a.stage, a.server_name.as_str(), a.login.buttons),
            (Stage::Login, "Home", 3)
        );
        // Empty username: message, no request.
        assert!(a.start_login().is_none() && a.msg == Some(MSG_USER));
        a.login.fields[0].value = "alice".into();
        a.login.fields[1].value = " pw ".into();
        let (e, _, user, pw) = a.start_login().unwrap();
        assert_eq!(
            (user.as_str(), pw.as_str()),
            ("alice", " pw "),
            "password is not trimmed"
        );
        assert!(a.start_login().is_none(), "no double submit while busy");
        assert!(a.logged_in(e, Ok(())));
        assert_eq!(a.stage, Stage::Active);
        assert_eq!(a.login.value(1), "", "password wiped");
    }

    #[test]
    fn wrong_credentials_and_unreachable_are_calm() {
        let mut a = acct();
        a.probed(a.epoch, Ok(probe(false)), "http://a:1");
        assert_eq!(
            a.login.buttons, 2,
            "no Quick Connect button when the server has it off"
        );
        a.login.fields[0].value = "alice".into();
        a.login.fields[1].value = "nope".into();
        let (e, ..) = a.start_login().unwrap();
        assert!(a.logged_in(e, Err(LoginFail::Invalid)));
        assert_eq!((a.stage, a.msg), (Stage::Login, Some(MSG_CREDS)));
        assert_eq!((a.login.value(1), a.login.focus), ("", 1));
        let (e, ..) = a.start_login().unwrap();
        a.logged_in(e, Err(LoginFail::Unreachable));
        assert_eq!(a.msg, Some(MSG_LOGIN_NET));
        let mut a = acct();
        type_into(&mut a, "x.lan");
        let (e, u) = a.start_connect().unwrap();
        a.probed(e, Err(ConnectFail::NotServer), &u);
        assert_eq!(
            (a.stage, a.msg, a.busy),
            (Stage::Connect, Some(MSG_NOT_SERVER), false)
        );
    }

    #[test]
    fn the_servers_login_disclaimer_is_kept_for_its_sign_in_page() {
        let mut a = acct();
        let (e, u) = a.begin_check("http://a:1".into(), false).unwrap();
        a.probed(
            e,
            Ok(Probe {
                disclaimer: Some("Members only".into()),
                ..probe(false)
            }),
            &u,
        );
        assert_eq!(a.disclaimer.as_deref(), Some("Members only"));
    }

    #[test]
    fn restore_paths() {
        // Valid session: straight to Active.
        let mut a = acct();
        let (e, u) = a.start_restore("http://a:1").unwrap();
        assert!(a.busy && a.restoring);
        a.probed(
            e,
            Ok(Probe {
                restored: true,
                ..probe(true)
            }),
            &u,
        );
        assert_eq!(
            (a.stage, a.busy, a.url.as_str()),
            (Stage::Active, false, "http://a:1")
        );
        // Signing out later shows this server's login, Quick Connect included.
        a.signed_out();
        assert_eq!((a.server_name.as_str(), a.login.buttons), ("Home", 3));
        // 401: cleared, Login with a calm note.
        let mut a = acct();
        let (e, u) = a.start_restore("http://a:1").unwrap();
        a.probed(
            e,
            Ok(Probe {
                expired: true,
                ..probe(true)
            }),
            &u,
        );
        assert_eq!((a.stage, a.msg), (Stage::Login, Some(MSG_EXPIRED)));
        // Keyring unavailable: Login, notice flag set (session stays in memory only).
        let mut a = acct();
        let (e, u) = a.start_restore("http://a:1").unwrap();
        a.probed(
            e,
            Ok(Probe {
                keyring_down: true,
                ..probe(false)
            }),
            &u,
        );
        assert!(a.keyring_down && a.stage == Stage::Login);
        // Server down at launch: Connect with the address kept, not an error screen.
        let mut a = acct();
        let (e, u) = a.start_restore("http://a:1").unwrap();
        a.probed(e, Err(ConnectFail::Unreachable), &u);
        assert_eq!(
            (a.stage, a.connect.value(0)),
            (Stage::Connect, "http://a:1")
        );
    }

    #[test]
    fn stale_responses_are_dropped() {
        let mut a = acct();
        type_into(&mut a, "slow.lan");
        let (old, u) = a.start_connect().unwrap();
        // User picks a remembered server meanwhile.
        let (new, u2) = a.pick_server(1).unwrap();
        assert_eq!(u2, "http://b:2");
        assert!(!a.probed(old, Ok(probe(false)), &u), "late answer ignored");
        assert_eq!(a.stage, Stage::Connect);
        assert!(a.probed(new, Ok(probe(false)), &u2));
        assert_eq!(a.url, "http://b:2");
        // A login answer after going back to the address screen is ignored too.
        a.login.fields[0].value = "alice".into();
        let (e, ..) = a.start_login().unwrap();
        a.switch_server(vec![]);
        assert!(!a.logged_in(e, Ok(())));
        assert_eq!(a.stage, Stage::Connect);
    }

    #[test]
    fn quick_connect_states() {
        let mut a = acct();
        a.probed(a.epoch, Ok(probe(true)), "http://a:1");
        let e = a.qc_start().unwrap();
        assert_eq!(a.qc, Qc::Starting);
        assert!(a.qc_code(e, Some("123456".into())));
        assert_eq!(a.qc, Qc::Waiting("123456".into()));
        assert_eq!(a.qc_polled(e, Poll::Pending), QcNext::Wait);
        // Transient failures tolerated, then given up calmly.
        for _ in 0..3 {
            assert_eq!(a.qc_polled(e, Poll::Hiccup), QcNext::Wait);
        }
        assert_eq!(a.qc_polled(e, Poll::Hiccup), QcNext::Stop);
        assert_eq!((a.qc.clone(), a.msg), (Qc::Off, Some(MSG_LOGIN_NET)));
        // Authorized -> exchange.
        let e = a.qc_start().unwrap();
        a.qc_code(e, Some("654321".into()));
        assert_eq!(a.qc_polled(e, Poll::Authorized), QcNext::Exchange);
        assert!(a.logged_in(e, Ok(())) && a.stage == Stage::Active);
    }

    #[test]
    fn quick_connect_cancel_expiry_and_refusal() {
        let mut a = acct();
        a.probed(a.epoch, Ok(probe(true)), "http://a:1");
        let e = a.qc_start().unwrap();
        a.qc_code(e, Some("111111".into()));
        a.qc_cancel();
        assert_eq!(a.qc, Qc::Off);
        assert_eq!(
            a.qc_polled(e, Poll::Authorized),
            QcNext::Stop,
            "poll in flight at cancel is stale"
        );
        let e = a.qc_start().unwrap();
        assert!(a.qc_code(e, None));
        assert_eq!(a.msg, Some(MSG_QC_OFF));
        let e = a.qc_start().unwrap();
        a.qc_code(e, Some("222222".into()));
        assert_eq!(a.qc_polled(e, Poll::Gone), QcNext::Stop);
        assert_eq!(a.msg, Some(MSG_QC_GONE));
        // Not offered when the server has it off.
        let mut b = acct();
        b.probed(b.epoch, Ok(probe(false)), "http://a:1");
        assert!(b.qc_start().is_none());
    }

    #[test]
    fn sign_out_then_switch_server() {
        let mut a = acct();
        a.probed(a.epoch, Ok(probe(true)), "http://a:1");
        a.login.fields[0].value = "alice".into();
        let (e, ..) = a.start_login().unwrap();
        a.logged_in(e, Ok(()));
        a.signed_out();
        assert_eq!(
            (a.stage, a.login.value(0), a.busy),
            (Stage::Login, "", false)
        );
        a.switch_server(vec!["http://a:1".into()]);
        assert_eq!(
            (a.stage, a.connect.value(0), a.connect.buttons),
            (Stage::Connect, "http://a:1", 2)
        );
    }

    #[test]
    fn form_keys() {
        let mut f = Form::new(
            vec![
                Fld {
                    label: "u",
                    ..Default::default()
                },
                Fld {
                    label: "p",
                    secret: true,
                    ..Default::default()
                },
            ],
            2,
        );
        assert_eq!(f.key("enter"), Ev::Stay, "enter moves to the next input");
        assert_eq!(f.focus, 1);
        assert_eq!(f.key("enter"), Ev::Submit);
        f.key("tab");
        assert_eq!(f.key("enter"), Ev::Press(0));
        f.key("down");
        assert_eq!(f.key("enter"), Ev::Press(1));
        f.key("down");
        assert_eq!(f.focus, 0, "wraps");
    }

    #[test]
    fn picker_form_prefills_the_name_and_skips_the_restore_message_for_new_profiles() {
        let mut a = acct();
        let (e, url) = a.start_form("http://a:1", Some("alice".into())).unwrap();
        assert!(a.probed(e, Ok(probe(false)), &url));
        assert_eq!(a.stage, Stage::Login);
        assert_eq!(
            (a.login.value(0), a.login.focus),
            ("alice", 1),
            "password next"
        );
        assert_eq!(a.msg, Some(MSG_EXPIRED));
        let (e, url) = a.start_form("http://a:1", None).unwrap();
        assert!(a.probed(e, Ok(probe(false)), &url));
        assert_eq!((a.login.value(0), a.login.focus, a.msg), ("", 0, None));
    }
}
