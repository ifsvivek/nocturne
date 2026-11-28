//! NOCTURNE servers in the app (docs/dev/SERVER_PLAN.md): Settings > NOCTURNE Servers lists this computer's server (found by its unix socket) and the servers added by URL, signs in to them, picks the active one and drives the local service through `nocturne-server install|start|stop`. The active server's event stream feeds the module pages (`server_pages.rs`). The wire and the server list are `nocturne_core::server_client`; tokens are only in the keyring. Nothing runs while no server is configured and no local socket exists.
use super::App;
use super::message::Message;
use crate::style;
use crate::task::run;
use crate::theme as t;
use crate::widget::{action_button, chip};
use iced::widget::{Column, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Subscription, Task};
use nocturne_core::server::types::{MediaKind, Release};
use nocturne_core::server::{Candidate, Event, MediaRequest, QueueItem, Status, Title};
use nocturne_core::server_client::{
    self as sc, Client, Error, Feed, LOCAL, QueueAct, ServerList, StorageRoot, Target,
};
use std::collections::HashMap;
use std::time::Duration;

type El<'a> = Element<'a, Message>;
/// An interactive search's answer.
pub(super) type Found = Result<Vec<Candidate>, String>;

const BINARY: &str = "nocturne-server";
/// Interactive searches ask every indexer: give them time.
const SLOW: Duration = Duration::from_secs(120);
const QUICK: Duration = Duration::from_secs(15);

/// A module page of the active server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Downloads,
    Library,
    Requests,
    Storage,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Downloads, Tab::Library, Tab::Requests, Tab::Storage];

    /// The module the page needs.
    pub fn module(self) -> &'static str {
        match self {
            Tab::Downloads => "downloads",
            Tab::Library => "library",
            Tab::Requests => "requests",
            Tab::Storage => "storage",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tab::Downloads => "Downloads",
            Tab::Library => "Library",
            Tab::Requests => "Requests",
            Tab::Storage => "Storage",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Ask every known server for its status.
    Refresh,
    Probed(String, Result<Box<Status>, String>),
    // The add-a-server card.
    Url(String),
    User(String),
    Pass(String),
    Key(String),
    UseKey(bool),
    Check,
    /// Debug QA: the add card from `NOCTURNE_SERVER_URL` / `_USER` / `_PASS`.
    Fill,
    Checked(String, Result<bool, String>),
    SignIn,
    Signed(String, Result<(String, Option<String>), String>),
    Remove(String),
    Activate(String),
    Dashboard(String),
    /// `nocturne-server <cmd>` for this computer's server.
    Ctl(&'static str),
    CtlDone(&'static str, Result<String, String>),
    Feed(Feed),
    // Module pages.
    Open(Tab),
    Close,
    Queue(Result<Vec<QueueItem>, String>),
    Titles(Result<Vec<Title>, String>),
    Requests(Result<Vec<MediaRequest>, String>),
    Storage(Result<Vec<StorageRoot>, String>),
    Act(u64, QueueAct),
    /// First press of a destructive button: it asks once more.
    Arm(u64),
    Done(Result<(), String>),
    Kind(MediaKind),
    Filter(String),
    Monitor(u64, bool),
    Search(u64),
    Releases(u64),
    Found(u64, Result<Vec<Candidate>, String>),
    Grab(u64, Box<Release>),
    HideReleases,
    Approve(u64),
    CancelRequest(u64),
}

#[derive(Default)]
pub struct State {
    list: ServerList,
    /// Tokens of the remote servers, read from the keyring once.
    tokens: HashMap<String, String>,
    /// Last answer per server key ([`LOCAL`] or URL): its status, or why there is none.
    pub(super) status: HashMap<String, Result<Status, String>>,
    url: String,
    user: String,
    pass: String,
    key: String,
    use_key: bool,
    /// The address checked in the add card and whether it still needs its first-run setup.
    checked: Option<(String, bool)>,
    form_note: Option<String>,
    busy: bool,
    /// Output of the last `nocturne-server` command.
    ctl_note: Option<String>,
    /// The event stream of the active server is connected.
    pub(super) live: bool,
    pub(super) page: Option<Tab>,
    pub(super) queue: Vec<QueueItem>,
    pub(super) titles: Vec<Title>,
    pub(super) kind: Option<MediaKind>,
    pub(super) filter: String,
    /// The title whose releases are shown, and them (`None` while searching).
    pub(super) releases: Option<(u64, Option<Found>)>,
    pub(super) requests: Vec<MediaRequest>,
    pub(super) storage: Vec<StorageRoot>,
    pub(super) page_note: Option<String>,
    pub(super) loading: bool,
    pub(super) armed: Option<u64>,
}

fn err(e: Error) -> String {
    e.to_string()
}

impl State {
    pub fn load() -> Self {
        let list = ServerList::load();
        // The keyring is asked only when there is something to ask for.
        let tokens = list
            .servers
            .iter()
            .filter_map(|s| {
                let t = nocturne_session::load_secret(&sc::secret_name(&s.url)).ok()??;
                Some((s.url.clone(), t))
            })
            .collect();
        State {
            list,
            tokens,
            kind: Some(MediaKind::Movie),
            ..Default::default()
        }
    }

    fn target(&self, key: &str) -> Option<Target> {
        if key == LOCAL {
            return Target::local();
        }
        self.list
            .servers
            .iter()
            .any(|s| s.url == key)
            .then(|| Target::Remote {
                url: key.into(),
                token: self.tokens.get(key).cloned(),
            })
    }

    /// The server the module pages talk to: the one the user made active, if it can be reached at all.
    pub fn active(&self) -> Option<Target> {
        self.target(self.list.active.as_deref()?)
    }

    fn active_status(&self) -> Option<&Status> {
        self.status.get(self.list.active.as_deref()?)?.as_ref().ok()
    }

    /// Module `m` runs on the active server.
    pub fn module_on(&self, m: &str) -> bool {
        self.active_status().is_some_and(|s| sc::module_on(s, m))
    }

    /// The module pages the active server offers.
    pub fn tabs(&self) -> Vec<Tab> {
        Tab::ALL
            .into_iter()
            .filter(|t| self.module_on(t.module()))
            .collect()
    }

    /// The display name of the active server.
    pub fn active_name(&self) -> String {
        match self.list.active.as_deref() {
            Some(LOCAL) => "This computer".into(),
            Some(u) => u
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .into(),
            None => String::new(),
        }
    }
}

/// `nocturne-server <cmd>` (blocking; never with sudo, the service is a user unit).
fn ctl(cmd: &'static str) -> Result<String, String> {
    let bin = super::gateway::locate(BINARY).ok_or_else(|| {
        "nocturne-server is not installed next to NOCTURNE or on your PATH.".to_string()
    })?;
    let out = std::process::Command::new(bin)
        .arg(cmd)
        .output()
        .map_err(|e| e.to_string())?;
    let say = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    let (o, e) = (say(&out.stdout), say(&out.stderr));
    if out.status.success() {
        Ok(o)
    } else {
        Err(if e.is_empty() { o } else { e })
    }
}

/// Opens `u` in the browser (http and https only).
fn open_url(u: &str) {
    if u.starts_with("http://") || u.starts_with("https://") {
        let _ = std::process::Command::new("xdg-open").arg(u).spawn();
    }
}

fn client(t: &Target, timeout: Duration) -> Result<Client, String> {
    Client::new(t, timeout).map_err(err)
}

/// One call to the active server, its result as a message.
fn call<T, F>(
    t: Option<Target>,
    timeout: Duration,
    f: impl FnOnce(Client) -> F + Send + 'static,
    m: impl FnOnce(Result<T, String>) -> Msg + Send + 'static,
) -> Task<Message>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, Error>> + Send + 'static,
{
    let Some(t) = t else {
        return Task::none();
    };
    run(
        async move {
            match client(&t, timeout) {
                Ok(c) => f(c).await.map_err(err),
                Err(e) => Err(e),
            }
        },
        move |r| Message::Servers(m(r)),
    )
}

/// The event stream of the active server, through the bridge runtime.
fn feed(t: &Target) -> impl iced::futures::Stream<Item = Msg> + use<> {
    let t = t.clone();
    iced::stream::channel(
        64,
        async move |mut out: iced::futures::channel::mpsc::Sender<Msg>| {
            use iced::futures::SinkExt;
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let _job = nocturne_core::bridge::spawn(sc::events(t, tx));
            while let Some(f) = rx.recv().await {
                if out.send(Msg::Feed(f)).await.is_err() {
                    return;
                }
            }
        },
    )
}

impl App {
    /// At start: the active server's status (nothing when none is chosen).
    pub(super) fn ns_boot(&self) -> Task<Message> {
        match self.ns.list.active.clone() {
            Some(k) => self.ns_probe(k),
            None => Task::none(),
        }
    }

    fn ns_probe(&self, key: String) -> Task<Message> {
        let Some(t) = self.ns.target(&key) else {
            return Task::none();
        };
        run(
            async move {
                let c = client(&t, QUICK)?;
                c.status().await.map(Box::new).map_err(err)
            },
            move |r| Message::Servers(Msg::Probed(key, r)),
        )
    }

    /// Settings > NOCTURNE Servers opened: ask everyone.
    pub(super) fn ns_refresh(&mut self) -> Task<Message> {
        let mut keys: Vec<String> = self.ns.list.servers.iter().map(|s| s.url.clone()).collect();
        if Target::local().is_some() {
            keys.push(LOCAL.into());
        } else {
            self.ns.status.remove(LOCAL);
        }
        Task::batch(keys.into_iter().map(|k| self.ns_probe(k)))
    }

    /// Live events only while a server is active and answered.
    pub(super) fn ns_subscription(&self) -> Subscription<Message> {
        match self.ns.active() {
            Some(t) if self.ns.active_status().is_some() => {
                Subscription::run_with(t, feed).map(Message::Servers)
            }
            _ => Subscription::none(),
        }
    }

    fn ns_save(&mut self) {
        if let Err(e) = self.ns.list.save() {
            self.toast(format!("The server list could not be saved: {e}"));
        }
    }

    pub(super) fn ns_msg(&mut self, m: Msg) -> Task<Message> {
        let s = &mut self.ns;
        match m {
            Msg::Refresh => return self.ns_refresh(),
            Msg::Probed(k, r) => {
                let active = s.list.active.as_deref() == Some(k.as_str());
                if active && let Err(e) = &r {
                    s.live = false;
                    s.page_note = Some(e.clone());
                }
                s.status.insert(k, r.map(|b| *b));
                // A page whose module went away closes.
                if let Some(p) = s.page
                    && !s.module_on(p.module())
                    && active
                {
                    s.page = None;
                }
            }
            Msg::Url(v) => {
                s.url = v;
                s.checked = None;
                s.form_note = None;
            }
            Msg::User(v) => s.user = v,
            Msg::Pass(v) => s.pass = v,
            Msg::Key(v) => s.key = v,
            Msg::UseKey(v) => s.use_key = v,
            Msg::Fill => {
                if cfg!(debug_assertions) {
                    let env = |k: &str| std::env::var(k).unwrap_or_default();
                    (s.url, s.user, s.pass) = (
                        env("NOCTURNE_SERVER_URL"),
                        env("NOCTURNE_SERVER_USER"),
                        env("NOCTURNE_SERVER_PASS"),
                    );
                }
            }
            Msg::Check => {
                let url = match sc::normalize_url(&s.url) {
                    Ok(u) => u,
                    Err(e) => {
                        s.form_note = Some(e);
                        return Task::none();
                    }
                };
                s.busy = true;
                s.form_note = None;
                let t = Target::Remote {
                    url: url.clone(),
                    token: None,
                };
                return run(
                    async move { client(&t, QUICK)?.setup_needed().await.map_err(err) },
                    move |r| Message::Servers(Msg::Checked(url, r)),
                );
            }
            Msg::Checked(url, r) => {
                s.busy = false;
                match r {
                    Ok(needed) => {
                        s.url = url.clone();
                        s.checked = Some((url, needed));
                    }
                    Err(e) => s.form_note = Some(e),
                }
            }
            Msg::SignIn => {
                let Some((url, false)) = s.checked.clone() else {
                    return Task::none();
                };
                s.busy = true;
                s.form_note = None;
                let (user, pass, key, use_key) = (
                    s.user.clone(),
                    s.pass.clone(),
                    s.key.trim().to_string(),
                    s.use_key,
                );
                let u2 = url.clone();
                return run(
                    async move {
                        if use_key {
                            let t = Target::Remote {
                                url,
                                token: Some(key.clone()),
                            };
                            client(&t, QUICK)?.me().await.map_err(err)?;
                            Ok((key, None))
                        } else {
                            let t = Target::Remote { url, token: None };
                            let l = client(&t, QUICK)?.login(&user, &pass).await.map_err(err)?;
                            Ok((l.token, Some(l.user.name)))
                        }
                    },
                    move |r| Message::Servers(Msg::Signed(u2, r)),
                );
            }
            Msg::Signed(url, r) => {
                s.busy = false;
                s.pass.clear();
                match r {
                    Ok((token, user)) => {
                        if let Err(e) =
                            nocturne_session::save_secret(&sc::secret_name(&url), &token)
                        {
                            s.form_note =
                                Some(format!("Signed in, but the keyring refused the token: {e}"));
                            return Task::none();
                        }
                        s.key.clear();
                        s.tokens.insert(url.clone(), token);
                        s.list.add(&url, user);
                        s.url.clear();
                        s.checked = None;
                        self.ns_save();
                        self.ns_reset_page();
                        return self.ns_probe(url);
                    }
                    Err(e) => s.form_note = Some(e),
                }
            }
            Msg::Remove(k) => {
                // Revoke a session token on the way out; an API key stays valid (it was made on the server).
                let revoke = s
                    .list
                    .servers
                    .iter()
                    .find(|x| x.url == k && x.user.is_some())
                    .and_then(|_| s.target(&k));
                let _ = nocturne_session::delete_secret(&sc::secret_name(&k));
                s.tokens.remove(&k);
                s.status.remove(&k);
                s.list.remove(&k);
                self.ns_save();
                self.ns_reset_page();
                return call(
                    revoke,
                    QUICK,
                    |c| async move { c.logout().await },
                    |_| Msg::Done(Ok(())),
                );
            }
            Msg::Activate(k) => {
                s.list.active = Some(k.clone());
                self.ns_save();
                self.ns_reset_page();
                return self.ns_probe(k);
            }
            Msg::Dashboard(k) => {
                if let Some(t) = s.target(&k) {
                    open_url(&t.dashboard());
                }
            }
            Msg::Ctl(cmd) => {
                s.busy = true;
                s.ctl_note = None;
                return run(
                    async move {
                        tokio::task::spawn_blocking(move || ctl(cmd))
                            .await
                            .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    move |r| Message::Servers(Msg::CtlDone(cmd, r)),
                );
            }
            Msg::CtlDone(cmd, r) => {
                s.busy = false;
                s.ctl_note = Some(match r {
                    Ok(o) if o.is_empty() => format!("{cmd}: done."),
                    Ok(o) => o,
                    Err(e) => e,
                });
                // The socket appears or goes away a moment after start / stop.
                return crate::task::after(
                    Duration::from_millis(800),
                    Message::Servers(Msg::Refresh),
                );
            }
            Msg::Feed(f) => return self.ns_feed(f),
            Msg::Open(tab) => {
                self.close_admin();
                self.ns.page = Some(tab);
                self.ns.page_note = None;
                self.ns.armed = None;
                return self.ns_load(tab);
            }
            Msg::Close => s.page = None,
            Msg::Queue(r) => {
                s.loading = false;
                match r {
                    Ok(v) => s.queue = v,
                    Err(e) => s.page_note = Some(e),
                }
            }
            Msg::Titles(r) => {
                s.loading = false;
                match r {
                    Ok(v) => s.titles = v,
                    Err(e) => s.page_note = Some(e),
                }
            }
            Msg::Requests(r) => {
                s.loading = false;
                match r {
                    Ok(v) => s.requests = v,
                    Err(e) => s.page_note = Some(e),
                }
            }
            Msg::Storage(r) => {
                s.loading = false;
                match r {
                    Ok(v) => s.storage = v,
                    Err(e) => s.page_note = Some(e),
                }
            }
            Msg::Arm(id) => s.armed = Some(id),
            Msg::Act(id, act) => {
                s.armed = None;
                let t = s.active();
                return call(
                    t,
                    QUICK,
                    move |c| async move { c.queue_act(id, act).await },
                    Msg::Done,
                )
                .chain(self.ns_load(Tab::Downloads));
            }
            Msg::Done(r) => {
                if let Err(e) = r {
                    self.toast(e);
                }
            }
            Msg::Kind(k) => {
                s.kind = Some(k);
                s.releases = None;
                return self.ns_load(Tab::Library);
            }
            Msg::Filter(v) => s.filter = v,
            Msg::Monitor(id, on) => {
                if let Some(x) = s.titles.iter_mut().find(|x| x.id == id) {
                    x.monitored = on;
                }
                let t = s.active();
                return call(
                    t,
                    QUICK,
                    move |c| async move { c.monitor(id, on).await },
                    Msg::Done,
                );
            }
            Msg::Search(id) => {
                self.toast("Searching. Anything found appears in Downloads.");
                let t = self.ns.active();
                return call(
                    t,
                    QUICK,
                    move |c| async move { c.search(id).await },
                    Msg::Done,
                );
            }
            Msg::Releases(id) => {
                s.releases = Some((id, None));
                let t = s.active();
                return call(
                    t,
                    SLOW,
                    move |c| async move { c.releases(id).await },
                    move |r| Msg::Found(id, r),
                );
            }
            Msg::Found(id, r) => {
                if s.releases.as_ref().is_some_and(|(x, _)| *x == id) {
                    s.releases = Some((id, Some(r)));
                }
            }
            Msg::Grab(id, rel) => {
                self.toast("Grabbed. It shows up in Downloads.");
                let t = self.ns.active();
                return call(
                    t,
                    QUICK,
                    move |c| async move { c.grab(id, &rel).await.map(drop) },
                    Msg::Done,
                );
            }
            Msg::HideReleases => s.releases = None,
            Msg::Approve(id) => {
                let t = s.active();
                return call(
                    t,
                    QUICK,
                    move |c| async move { c.approve(id).await },
                    Msg::Done,
                )
                .chain(self.ns_load(Tab::Requests));
            }
            Msg::CancelRequest(id) => {
                s.armed = None;
                let t = s.active();
                return call(
                    t,
                    QUICK,
                    move |c| async move { c.cancel_request(id).await },
                    Msg::Done,
                )
                .chain(self.ns_load(Tab::Requests));
            }
        }
        Task::none()
    }

    /// The active server changed: its pages start empty.
    fn ns_reset_page(&mut self) {
        let s = &mut self.ns;
        s.page = None;
        s.live = false;
        s.queue.clear();
        s.titles.clear();
        s.requests.clear();
        s.storage.clear();
        s.releases = None;
    }

    /// Fetch what page `tab` shows.
    fn ns_load(&mut self, tab: Tab) -> Task<Message> {
        let t = self.ns.active();
        self.ns.loading = true;
        match tab {
            Tab::Downloads => call(t, QUICK, |c| async move { c.queue().await }, Msg::Queue),
            Tab::Library => {
                let k = self.ns.kind;
                call(
                    t,
                    QUICK,
                    move |c| async move { c.titles(k).await },
                    Msg::Titles,
                )
            }
            Tab::Requests => call(
                t,
                QUICK,
                |c| async move { c.requests().await },
                Msg::Requests,
            ),
            Tab::Storage => call(t, QUICK, |c| async move { c.storage().await }, Msg::Storage),
        }
    }

    fn ns_feed(&mut self, f: Feed) -> Task<Message> {
        let s = &mut self.ns;
        match f {
            Feed::Connected => {
                s.live = true;
                // Whatever changed while the socket was down.
                let k = s.list.active.clone();
                let page = s.page;
                return Task::batch([
                    k.map_or_else(Task::none, |k| self.ns_probe(k)),
                    page.map_or_else(Task::none, |p| self.ns_load(p)),
                ]);
            }
            Feed::Disconnected => s.live = false,
            Feed::Auth => {
                s.live = false;
                if let Some(k) = s.list.active.clone() {
                    s.status.insert(
                        k,
                        Err("Sign in again: the server refused the saved credential.".into()),
                    );
                }
            }
            Feed::Event(e) => match *e {
                Event::Queue(items) => s.queue = items,
                Event::Title(_) if s.page == Some(Tab::Library) => {
                    return self.ns_load(Tab::Library);
                }
                Event::Request(_) if s.page == Some(Tab::Requests) => {
                    return self.ns_load(Tab::Requests);
                }
                Event::Module(m) if m.module == "storage" && s.page == Some(Tab::Storage) => {
                    return self.ns_load(Tab::Storage);
                }
                Event::Health(m) => {
                    if let Some(Ok(st)) = s.list.active.clone().and_then(|k| s.status.get_mut(&k)) {
                        match st.modules.iter_mut().find(|x| x.name == m.name) {
                            Some(x) => *x = m,
                            None => st.modules.push(m),
                        }
                    }
                    if let Some(p) = s.page
                        && !s.module_on(p.module())
                    {
                        s.page = None;
                    }
                }
                _ => {}
            },
        }
        Task::none()
    }

    /// Settings > NOCTURNE Servers: this computer, the added servers, and the add card.
    pub(super) fn servers_panel(&self) -> El<'_> {
        let s = &self.ns;
        let mut col = column![].spacing(t::S4).max_width(900);
        col = col.push(self.ns_local_card());
        for sv in &s.list.servers {
            col = col.push(
                self.ns_card(
                    &sv.url,
                    sv.url.clone(),
                    sv.user
                        .as_deref()
                        .map_or("Signed in with an API key".to_string(), |u| {
                            format!("Signed in as {u}")
                        }),
                    vec![action_button(
                        "Remove",
                        false,
                        true,
                        false,
                        false,
                        Message::Servers(Msg::Remove(sv.url.clone())),
                    )],
                ),
            );
        }
        col.push(self.ns_add_card()).into()
    }

    fn ns_local_card(&self) -> El<'_> {
        let s = &self.ns;
        let sock = Target::local().is_some();
        let installed = nocturne_core::user_unit::installed(&nocturne_core::user_unit::SERVER);
        let found = super::gateway::locate(BINARY).is_some();
        let sub = match (sock, installed, found) {
            (true, _, _) => "Runs on this computer. You are its administrator here, without a password.".to_string(),
            (false, true, _) => "Installed as a service, not running.".into(),
            (false, false, true) => "Not installed. Install runs it as a service of your user (no administrator rights needed).".into(),
            (false, false, false) => "nocturne-server was not found next to NOCTURNE or on your PATH.".into(),
        };
        let off = s.busy || !found;
        let ctl_btn = |label: &str, cmd: &'static str| {
            action_button(
                label,
                false,
                false,
                false,
                off,
                Message::Servers(Msg::Ctl(cmd)),
            )
        };
        let mut buttons = vec![];
        if !installed {
            buttons.push(ctl_btn("Install", "install"));
        } else if sock {
            buttons.push(ctl_btn("Stop", "stop"));
        } else {
            buttons.push(ctl_btn("Start", "start"));
        }
        let mut card = self.ns_card(LOCAL, "This computer".into(), sub, buttons);
        if let Some(n) = &s.ctl_note {
            card = column![card, container(note(n.clone())).padding([0.0, t::S4])]
                .spacing(t::S2)
                .into();
        }
        card
    }

    /// A server's card: name, one line, status with version and modules, and its buttons.
    fn ns_card<'a>(
        &'a self,
        key: &str,
        name: String,
        sub: String,
        mut extra: Vec<El<'a>>,
    ) -> El<'a> {
        let s = &self.ns;
        let active = s.list.active.as_deref() == Some(key);
        let st = s.status.get(key);
        let (dot, line) = match st {
            Some(Ok(st)) if st.setup_needed => (
                t::warn(),
                "Online, waiting for its first-run setup".to_string(),
            ),
            Some(Ok(st)) => (
                t::accent(),
                format!(
                    "Online \u{00B7} version {} \u{00B7} up {}",
                    st.version,
                    uptime(st.uptime_secs)
                ),
            ),
            Some(Err(e)) => (t::danger(), e.clone()),
            None if key == LOCAL && Target::local().is_none() => {
                (t::text_tertiary(), "Not running".into())
            }
            None => (t::text_secondary(), "Not checked yet".into()),
        };
        let mut head = row![
            column![
                text(name).size(t::BODY).font(t::semibold()),
                text(sub).size(t::CONTROL_TEXT).color(t::text_secondary()),
            ]
            .spacing(3)
            .width(Length::Fill),
        ]
        .spacing(t::S3)
        .align_y(Alignment::Center);
        if active {
            head = head.push(chip("Active", true));
        }
        let mut body = Column::new().spacing(t::S3).push(head).push(
            row![
                text("\u{25CF}").size(t::CONTROL_TEXT).color(dot),
                text(line)
                    .size(t::CONTROL_TEXT + 1.0)
                    .color(t::text_secondary()),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        );
        if let Some(Ok(st)) = st
            && !st.modules.is_empty()
        {
            let chips = st
                .modules
                .iter()
                .fold(row![].spacing(t::S2), |r, m| {
                    r.push(chip(&m.name, m.running))
                })
                .wrap()
                .vertical_spacing(t::S2);
            body = body.push(chips);
        }
        let online = matches!(st, Some(Ok(_)));
        let mut buttons = row![].spacing(t::S2);
        if !active {
            buttons = buttons.push(action_button(
                "Use this server",
                true,
                false,
                false,
                !online,
                Message::Servers(Msg::Activate(key.to_string())),
            ));
        }
        buttons = buttons.push(action_button(
            "Open dashboard",
            false,
            false,
            false,
            !online,
            Message::Servers(Msg::Dashboard(key.to_string())),
        ));
        for e in extra.drain(..) {
            buttons = buttons.push(e);
        }
        container(body.push(buttons.wrap().vertical_spacing(t::S2)))
            .padding(t::S4 + 2.0)
            .width(Length::Fill)
            .style(style::card)
            .into()
    }

    fn ns_add_card(&self) -> El<'_> {
        let s = &self.ns;
        let field = |ph: &str, v: &str, f: fn(String) -> Msg| {
            text_input(ph, v)
                .on_input(move |x| Message::Servers(f(x)))
                .size(t::BODY)
                .padding([t::S2, t::S3])
                .style(style::input)
        };
        let mut col = column![
            text("Add a server").size(t::BODY).font(t::semibold()),
            text("Another computer running nocturne-server, by its address (for example http://nas.local:8100).")
                .size(t::CONTROL_TEXT)
                .color(t::text_secondary()),
            row![
                field("Server address", &s.url, Msg::Url).on_submit(Message::Servers(Msg::Check)),
                action_button("Check", s.checked.is_none(), false, false, s.busy, Message::Servers(Msg::Check)),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        ]
        .spacing(t::S3);
        match &s.checked {
            Some((url, true)) => {
                col = col.push(note(
                    "This server has no administrator yet. Create one in its dashboard, then come back and sign in.".into(),
                ));
                col = col.push(
                    row![
                        action_button(
                            "Open first-run setup",
                            true,
                            false,
                            false,
                            false,
                            Message::Servers(Msg::Dashboard(url.clone())),
                        ),
                        action_button(
                            "Check again",
                            false,
                            false,
                            false,
                            s.busy,
                            Message::Servers(Msg::Check)
                        )
                    ]
                    .spacing(t::S2),
                );
            }
            Some((_, false)) => {
                let how = row![
                    action_button(
                        "User and password",
                        !s.use_key,
                        false,
                        false,
                        false,
                        Message::Servers(Msg::UseKey(false))
                    ),
                    action_button(
                        "API key",
                        s.use_key,
                        false,
                        false,
                        false,
                        Message::Servers(Msg::UseKey(true))
                    ),
                ]
                .spacing(t::S2);
                col = col.push(how);
                if s.use_key {
                    col = col.push(
                        field("API key (nsk_...)", &s.key, Msg::Key)
                            .secure(true)
                            .on_submit(Message::Servers(Msg::SignIn)),
                    );
                } else {
                    col = col.push(
                        row![
                            field("User name", &s.user, Msg::User),
                            field("Password", &s.pass, Msg::Pass)
                                .secure(true)
                                .on_submit(Message::Servers(Msg::SignIn)),
                        ]
                        .spacing(t::S2),
                    );
                }
                col = col.push(row![action_button(
                    "Sign in",
                    true,
                    false,
                    false,
                    s.busy,
                    Message::Servers(Msg::SignIn)
                )]);
            }
            None => {}
        }
        if let Some(n) = &s.form_note {
            col = col.push(note(n.clone()));
        }
        container(col)
            .padding(t::S4 + 2.0)
            .width(Length::Fill)
            .style(style::card)
            .into()
    }
}

fn note<'a>(s: String) -> El<'a> {
    text(s)
        .size(t::CONTROL_TEXT)
        .color(t::text_secondary())
        .into()
}

fn uptime(secs: u64) -> String {
    match secs {
        0..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} days", secs / 86_400),
    }
}
