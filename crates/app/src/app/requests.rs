//! Seerr requests in the UI: sign-in (Quick Connect or username and password), the Request dialog,
//! the Requests page, search results for titles that are not in the catalog, and the badge on a
//! detail page. The rules (permissions, badges, seasons, wording) are `nocturne_core::requests`;
//! the wire is `nocturne-seerr`; this file is glue and state, `requests_view.rs` paints.
//! A request goes to the Seerr server only, never to a media server, and only after the dialog's
//! confirmation: `RequestConfirmed` is minted in `rq_confirm` and `cancel_confirmed`, nowhere else.
use super::message::Message;
use super::{App, Load};
use crate::task::{after, run};
use iced::Task;
use iced::widget::image::Handle;
use nocturne_core::requests::{self as rq, Badge, Picker, Tab};
use nocturne_core::source::Media;
use nocturne_seerr::{
    Auth, Detail, Error, Kind, Me, MediaInfo, NewRequest, PublicSettings, QuickConnect, Quota,
    Request, RequestConfirmed, RequestsPage, SearchHit, SeerrClient, SessionCookie, Status,
    normalize_url, secret_name,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Details and posters fetched at once (a page of requests must not start fifty calls).
static FETCH: Semaphore = Semaphore::const_new(4);
/// How often Quick Connect is asked whether the code was authorised, and for how long.
const POLL: Duration = Duration::from_secs(2);
const POLL_MAX: u32 = 150;
/// A fetched title is reused this long.
const FRESH: Duration = Duration::from_secs(60);

pub type Res<T> = Result<T, Arc<Error>>;

fn arc<T>(r: nocturne_seerr::Result<T>) -> Res<T> {
    r.map_err(Arc::new)
}

/// A title to request: what a search hit, a detail page or the Calendar knows of it.
#[derive(Clone, Debug)]
pub struct Target {
    pub kind: Kind,
    pub tmdb: u64,
    pub title: String,
    pub year: Option<u32>,
    pub poster: Option<String>,
}

/// The message that opens the Request dialog for a title (the Calendar's "Request" button).
pub fn ask(kind: Kind, tmdb: u64, title: String, year: Option<u32>) -> Message {
    Message::Requests(Msg::Ask(Box::new(Target {
        kind,
        tmdb,
        title,
        year,
        poster: None,
    })))
}

/// A finished sign-in: the cookie is already in the keyring (when it could be saved).
#[derive(Clone)]
pub struct Signed {
    pub url: String,
    pub me: Me,
    pub client: Arc<SeerrClient>,
    pub saved: bool,
}

#[derive(Clone)]
pub enum Msg {
    // sign-in dialog (Settings > Seerr server)
    Open,
    Close,
    Url(String),
    User(String),
    Pass(String),
    SignIn,
    Quick,
    QuickStarted(u64, Res<QuickConnect>),
    QuickPoll(u64),
    QuickChecked(u64, Res<bool>),
    Signed(u64, Res<Signed>),
    SignOut,
    /// Debug builds only: fill the form from `NOCTURNE_SEERR_URL`, `_USER` and `_PASS` (headless QA).
    Fill,
    /// The saved sign-in, read at launch.
    Loaded(Option<(Arc<SeerrClient>, Res<Me>)>),
    Refreshed(Res<Me>),
    Info(Res<(Status, PublicSettings)>),
    QuotaIs(Res<Quota>),
    // search
    Hits(u64, Res<Vec<SearchHit>>),
    // the Request dialog
    Ask(Box<Target>),
    Detail(u64, Res<Box<(Detail, PublicSettings)>>),
    Tick(u32),
    AllSeasons,
    Confirm,
    AskClose,
    Sent(u64, Res<Box<Request>>),
    // the Requests page
    PageOpen,
    PageClose,
    Fetched(u64, Res<RequestsPage>),
    Tab(Tab),
    Title(Kind, u64, Option<Box<Detail>>),
    Cancel(u64),
    CancelYes,
    CancelNo,
    Cancelled(u64, Res<()>),
    OpenCatalog(Kind, u64, String, Option<u32>),
    Found(Option<Box<Media>>),
    Poster(String, Option<Handle>),
    // a detail page
    Chip(u64, Kind, u64, Res<Box<Detail>>),
}

enum Stage {
    Form,
    /// Waiting for the code to be authorised in a Jellyfin app: (code, secret, polls so far).
    Quick(String, String, u32),
}

pub(super) struct Dlg {
    pub url: String,
    pub user: String,
    pub pass: String,
    stage: Stage,
    pub note: Option<String>,
    epoch: u64,
    pub busy: bool,
    /// A sign-in just finished and the keyring could not keep it.
    pub unsaved: bool,
}

impl Dlg {
    pub(super) fn quick_code(&self) -> Option<&str> {
        match &self.stage {
            Stage::Quick(c, ..) => Some(c),
            Stage::Form => None,
        }
    }
}

pub(super) enum AskStage {
    Form,
    Sending,
    Done(Box<Request>),
    Failed(String),
}

pub(super) struct Ask {
    pub target: Target,
    epoch: u64,
    pub detail: Load<Detail>,
    pub picker: Option<Picker>,
    pub stage: AskStage,
}

pub(super) enum Title {
    Loading,
    Ready(Box<Detail>, Instant),
    Failed,
}

pub(super) enum Poster {
    Loading,
    Ready(Handle),
    Missing,
}

pub(super) struct PageSt {
    epoch: u64,
    pub tab: Tab,
    pub list: Load<Vec<Request>>,
    pub cancel: Option<u64>,
    pub busy: Option<u64>,
    pub note: Option<String>,
}

pub(super) struct Hits {
    epoch: u64,
    pub hits: Vec<SearchHit>,
}

/// The badge (and button) shown over a detail page.
pub(super) struct Chip {
    pub pid: u64,
    pub target: Target,
    pub badge: Option<Badge>,
    /// A show with seasons nobody asked for yet.
    pub more: bool,
}

#[derive(Default)]
pub struct State {
    loaded: bool,
    pub(super) client: Option<Arc<SeerrClient>>,
    pub(super) me: Option<Me>,
    pub(super) public: PublicSettings,
    pub(super) version: Option<String>,
    pub(super) quota: Option<Quota>,
    pub(super) dlg: Option<Dlg>,
    pub(super) ask: Option<Ask>,
    pub(super) page: Option<PageSt>,
    pub(super) titles: HashMap<(Kind, u64), Title>,
    pub(super) posters: HashMap<String, Poster>,
    /// What this session sent, so a card says Requested at once.
    pub(super) sent: HashMap<(Kind, u64), Badge>,
    pub(super) hits: Option<Hits>,
    pub(super) chip: Option<Chip>,
    /// Discover, the native request list, Radarr / Sonarr (`discover.rs`).
    pub(super) disc: super::discover::State,
    epoch: u64,
}

impl State {
    pub fn signed_in(&self) -> bool {
        self.client.is_some()
    }

    pub fn page_open(&self) -> bool {
        self.page.is_some()
    }
}

impl App {
    /// Requests can be sent: the setting is on, someone is signed in, and it is not developer mode.
    pub fn requests_on(&self) -> bool {
        self.settings.seerr_requests && self.rq.client.as_ref().is_some_and(|c| !c.is_api_key())
    }

    pub(super) fn rq_can(&self, kind: Kind) -> bool {
        self.native_on()
            || (self.requests_on()
                && self
                    .rq
                    .me
                    .as_ref()
                    .is_some_and(|m| rq::can_request(m.permissions, kind)))
    }

    /// There is a Requests page: a Seerr server is signed in, or requests without Seerr are on.
    pub fn requests_available(&self) -> bool {
        self.rq.signed_in() || self.settings.native_requests
    }

    /// Requests are kept in NOCTURNE's own list (and Radarr / Sonarr): the setting is on and no Seerr
    /// server is signed in (Seerr wins).
    pub fn native_on(&self) -> bool {
        self.settings.native_requests && !self.rq.signed_in()
    }

    /// The Settings chip.
    pub(super) fn rq_chip_label(&self) -> String {
        match (&self.rq.client, &self.rq.me) {
            (Some(c), _) if c.is_api_key() => "Developer key".into(),
            (Some(_), Some(m)) => format!("Signed in as {}", m.name()),
            _ if self.settings.seerr.url.is_empty() => "Set up".into(),
            _ => "Sign in".into(),
        }
    }

    // ---- launch ----

    /// Read the saved sign-ins (Seerr, Radarr, Sonarr) and the native request list once, after launch.
    pub(super) fn rq_load(&mut self) -> Task<Message> {
        Task::batch([self.rq_load_seerr(), self.disc_boot()])
    }

    fn rq_load_seerr(&mut self) -> Task<Message> {
        if self.rq.loaded {
            return Task::none();
        }
        self.rq.loaded = true;
        let dev = std::env::var("NOCTURNE_SEERR_KEY")
            .ok()
            .filter(|k| !k.is_empty());
        let mut url = self.settings.seerr.url.clone();
        if url.is_empty() && dev.is_some() {
            url = std::env::var("NOCTURNE_SEERR_URL").unwrap_or_default();
        }
        if url.is_empty() {
            return Task::none();
        }
        run(
            async move {
                let auth = match dev {
                    Some(k) => Auth::ApiKey(k),
                    None => {
                        let name = secret_name(&url);
                        let c = tokio::task::spawn_blocking(move || {
                            nocturne_session::load_secret(&name)
                        })
                        .await
                        .ok()?
                        .ok()??;
                        Auth::Cookie(SessionCookie::from_value(c))
                    }
                };
                let client = Arc::new(SeerrClient::new(&url, auth).ok()?);
                let me = arc(client.me().await);
                Some((client, me))
            },
            |r| Message::Requests(Msg::Loaded(r)),
        )
    }

    /// Version, public settings and the quota: after sign-in, at launch and when the page opens.
    fn rq_refresh(&self) -> Task<Message> {
        let (Some(c), Some(me)) = (self.rq.client.clone(), self.rq.me.as_ref()) else {
            return Task::none();
        };
        let uid = me.id;
        let c2 = c.clone();
        Task::batch([
            run(
                async move {
                    let s = arc(c.status().await)?;
                    Ok((s, arc(c.public_settings().await)?))
                },
                |r| Message::Requests(Msg::Info(r)),
            ),
            run(async move { arc(c2.quota(uid).await) }, |r| {
                Message::Requests(Msg::QuotaIs(r))
            }),
        ])
    }

    /// A call answered "not signed in": drop the session (the cookie is a bearer credential, so it
    /// leaves the keyring too) and say so once. Never retried in a loop.
    pub(super) fn rq_ended(&mut self, e: &Error) -> bool {
        if !matches!(e, Error::Unauthorized)
            || self.rq.client.as_ref().is_none_or(|c| c.is_api_key())
        {
            return false;
        }
        self.rq_forget();
        self.rq.page = None;
        self.rq.ask = None;
        self.toast("Your Seerr sign-in ended. Sign in again in Settings.");
        true
    }

    fn rq_forget(&mut self) {
        self.rq.client = None;
        self.rq.me = None;
        self.rq.chip = None;
        self.rq.hits = None;
        self.settings.seerr.user_id = 0;
        self.settings.seerr.user_name.clear();
        self.settings.seerr.permissions = 0;
        self.settings.save();
        let name = secret_name(&self.settings.seerr.url);
        nocturne_core::bridge::spawn(async move {
            let _ =
                tokio::task::spawn_blocking(move || nocturne_session::delete_secret(&name)).await;
        });
    }

    fn rq_adopt(&mut self, client: Arc<SeerrClient>, me: Me) -> Task<Message> {
        if !client.is_api_key() {
            let s = &mut self.settings.seerr;
            let (url, name) = (client.base_url(), me.name());
            if (&s.url, s.user_id, &s.user_name, s.permissions)
                != (&url, me.id, &name, me.permissions)
            {
                (s.url, s.user_id, s.user_name, s.permissions) = (url, me.id, name, me.permissions);
                self.settings.save();
            }
        }
        self.rq.client = Some(client);
        self.rq.me = Some(me);
        self.rq_refresh()
    }

    // ---- messages ----

    pub(super) fn rq_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Open => self.rq_open_dialog(),
            Msg::Close => {
                self.rq.dlg = None;
                self.rq.ask = None;
                Task::none()
            }
            Msg::Url(s) => self.rq_field(|d| d.url = s),
            Msg::User(s) => self.rq_field(|d| d.user = s),
            Msg::Pass(s) => self.rq_field(|d| d.pass = s),
            Msg::Fill => self.rq_fill(),
            Msg::SignIn => self.rq_sign_in(),
            Msg::Quick => self.rq_quick(),
            Msg::QuickStarted(e, r) => self.rq_quick_started(e, r),
            Msg::QuickPoll(e) => self.rq_quick_poll(e),
            Msg::QuickChecked(e, r) => self.rq_quick_checked(e, r),
            Msg::Signed(e, r) => self.rq_signed(e, r),
            Msg::SignOut => self.rq_sign_out(),
            Msg::Loaded(None) => Task::none(),
            Msg::Loaded(Some((c, Ok(me)))) => self.rq_adopt(c, me),
            Msg::Loaded(Some((c, Err(e)))) => {
                if matches!(*e, Error::Unauthorized) {
                    // The stored session is gone or expired: not signed in; the row says Sign in.
                    self.rq_forget();
                    return Task::none();
                }
                // The server may only be down: keep the session and what we knew of the account.
                let s = &self.settings.seerr;
                let cached = Me {
                    id: s.user_id,
                    permissions: s.permissions,
                    display_name: (!s.user_name.is_empty()).then(|| s.user_name.clone()),
                    ..Default::default()
                };
                self.rq_adopt(c, cached)
            }
            Msg::Refreshed(Ok(me)) => {
                if let Some(c) = self.rq.client.clone() {
                    return self.rq_adopt(c, me);
                }
                Task::none()
            }
            Msg::Refreshed(Err(e)) => {
                self.rq_ended(&e);
                Task::none()
            }
            Msg::Info(Ok((s, p))) => {
                self.rq.version = Some(s.version);
                self.rq.public = p;
                Task::none()
            }
            Msg::Info(Err(e)) => {
                self.rq_ended(&e);
                Task::none()
            }
            Msg::QuotaIs(r) => {
                match r {
                    Ok(q) => self.rq.quota = Some(q),
                    Err(e) => {
                        self.rq_ended(&e);
                    }
                }
                Task::none()
            }
            Msg::Hits(e, r) => self.rq_hits(e, r),
            Msg::Ask(t) => self.rq_ask(*t),
            Msg::Detail(e, r) => self.rq_detail(e, r),
            Msg::Tick(n) => {
                if let Some(p) = self.rq.ask.as_mut().and_then(|a| a.picker.as_mut()) {
                    p.toggle(n);
                }
                Task::none()
            }
            Msg::AllSeasons => {
                if let Some(p) = self.rq.ask.as_mut().and_then(|a| a.picker.as_mut()) {
                    p.toggle_all();
                }
                Task::none()
            }
            Msg::Confirm => self.rq_confirm(),
            Msg::AskClose => {
                self.rq.ask = None;
                Task::none()
            }
            Msg::Sent(e, r) => self.rq_sent(e, r),
            Msg::PageOpen => self.rq_open_page(),
            Msg::PageClose => {
                self.rq.page = None;
                Task::none()
            }
            Msg::Fetched(e, r) => self.rq_fetched(e, r),
            Msg::Tab(t) => {
                if let Some(p) = self.rq.page.as_mut() {
                    p.tab = t;
                }
                Task::none()
            }
            Msg::Title(k, id, d) => self.rq_title(k, id, d),
            Msg::Cancel(id) => {
                if let Some(p) = self.rq.page.as_mut() {
                    p.cancel = Some(id);
                    p.note = None;
                }
                Task::none()
            }
            Msg::CancelNo => {
                if let Some(p) = self.rq.page.as_mut() {
                    p.cancel = None;
                }
                Task::none()
            }
            Msg::CancelYes => self.cancel_confirmed(),
            Msg::Cancelled(id, r) => self.rq_cancelled(id, r),
            Msg::OpenCatalog(k, id, title, year) => self.rq_open_catalog(k, id, title, year),
            Msg::Found(Some(m)) => {
                self.rq.page = None;
                self.open_any(*m)
            }
            Msg::Found(None) => {
                self.toast("It is not in your library yet.");
                Task::none()
            }
            Msg::Poster(path, h) => {
                self.rq
                    .posters
                    .insert(path, h.map_or(Poster::Missing, Poster::Ready));
                Task::none()
            }
            Msg::Chip(pid, k, id, r) => self.rq_chip(pid, k, id, r),
        }
    }

    fn rq_field(&mut self, f: impl FnOnce(&mut Dlg)) -> Task<Message> {
        if let Some(d) = self.rq.dlg.as_mut() {
            f(d);
        }
        Task::none()
    }

    // ---- sign-in ----

    fn rq_open_dialog(&mut self) -> Task<Message> {
        let url = self.settings.seerr.url.clone();
        // The Jellyfin name of this session is the likeliest username on a Seerr that sits on that server.
        let user = match (&self.rq.me, self.session()) {
            (Some(m), _) => m.name(),
            (None, Some((_, s))) if !self.dev => s.user_name.clone(),
            _ => self.settings.seerr.user_name.clone(),
        };
        self.rq.dlg = Some(Dlg {
            url,
            user,
            pass: String::new(),
            stage: Stage::Form,
            note: None,
            epoch: 0,
            busy: false,
            unsaved: false,
        });
        self.rq_refresh()
    }

    fn rq_fill(&mut self) -> Task<Message> {
        if cfg!(debug_assertions)
            && let Some(d) = self.rq.dlg.as_mut()
        {
            let env = |k| std::env::var(k).unwrap_or_default();
            (d.url, d.user, d.pass) = (
                env("NOCTURNE_SEERR_URL"),
                env("NOCTURNE_SEERR_USER"),
                env("NOCTURNE_SEERR_PASS"),
            );
        }
        Task::none()
    }

    /// The address typed in the dialog, or why it can't be used.
    fn rq_dialog_url(&mut self) -> Option<String> {
        let d = self.rq.dlg.as_mut()?;
        match normalize_url(&d.url) {
            Ok(u) => Some(u),
            Err(_) => {
                d.note = Some(
                    "Enter the address of your Seerr server, like https://seerr.example.com."
                        .into(),
                );
                None
            }
        }
    }

    fn rq_sign_in(&mut self) -> Task<Message> {
        let Some(url) = self.rq_dialog_url() else {
            return Task::none();
        };
        let Some(d) = self.rq.dlg.as_mut().filter(|d| !d.busy) else {
            return Task::none();
        };
        if d.user.trim().is_empty() || d.pass.is_empty() {
            d.note = Some("Enter your username and password.".into());
            return Task::none();
        }
        d.epoch += 1;
        d.busy = true;
        d.note = None;
        let (e, user) = (d.epoch, d.user.trim().to_string());
        // The password is used for this one call and not kept anywhere.
        let pass = std::mem::take(&mut d.pass);
        run(
            async move {
                let anon = SeerrClient::new(&url, Auth::None).map_err(Arc::new)?;
                let (me, cookie) = arc(anon.sign_in_password(&user, &pass).await)?;
                finish(url, me, cookie).await
            },
            move |r| Message::Requests(Msg::Signed(e, r)),
        )
    }

    fn rq_quick(&mut self) -> Task<Message> {
        let Some(url) = self.rq_dialog_url() else {
            return Task::none();
        };
        let Some(d) = self.rq.dlg.as_mut().filter(|d| !d.busy) else {
            return Task::none();
        };
        d.epoch += 1;
        d.busy = true;
        d.note = None;
        let e = d.epoch;
        run(
            async move {
                let anon = SeerrClient::new(&url, Auth::None).map_err(Arc::new)?;
                arc(anon.quick_connect_initiate().await)
            },
            move |r| Message::Requests(Msg::QuickStarted(e, r)),
        )
    }

    fn rq_quick_started(&mut self, e: u64, r: Res<QuickConnect>) -> Task<Message> {
        let Some(d) = self.rq.dlg.as_mut().filter(|d| d.epoch == e) else {
            return Task::none();
        };
        d.busy = false;
        match r {
            Ok(q) => {
                d.stage = Stage::Quick(q.code, q.secret, 0);
                after(POLL, Message::Requests(Msg::QuickPoll(e)))
            }
            Err(err) => {
                d.note = Some(rq::error_text(&err));
                Task::none()
            }
        }
    }

    fn rq_quick_poll(&mut self, e: u64) -> Task<Message> {
        let Some(d) = self.rq.dlg.as_mut().filter(|d| d.epoch == e) else {
            return Task::none();
        };
        let Stage::Quick(_, secret, n) = &mut d.stage else {
            return Task::none();
        };
        *n += 1;
        if *n > POLL_MAX {
            d.stage = Stage::Form;
            d.note = Some("The code was not used in time. Try again.".into());
            return Task::none();
        }
        let secret = secret.clone();
        let Ok(url) = normalize_url(&d.url) else {
            return Task::none();
        };
        run(
            async move {
                let anon = SeerrClient::new(&url, Auth::None).map_err(Arc::new)?;
                arc(anon.quick_connect_check(&secret).await)
            },
            move |r| Message::Requests(Msg::QuickChecked(e, r)),
        )
    }

    fn rq_quick_checked(&mut self, e: u64, r: Res<bool>) -> Task<Message> {
        let Some(d) = self.rq.dlg.as_mut().filter(|d| d.epoch == e) else {
            return Task::none();
        };
        let Stage::Quick(_, secret, _) = &d.stage else {
            return Task::none();
        };
        match r {
            Ok(false) => after(POLL, Message::Requests(Msg::QuickPoll(e))),
            Ok(true) => {
                let (secret, Ok(url)) = (secret.clone(), normalize_url(&d.url)) else {
                    return Task::none();
                };
                d.busy = true;
                run(
                    async move {
                        let anon = SeerrClient::new(&url, Auth::None).map_err(Arc::new)?;
                        let (me, cookie) = arc(anon.quick_connect_complete(&secret).await)?;
                        finish(url, me, cookie).await
                    },
                    move |r| Message::Requests(Msg::Signed(e, r)),
                )
            }
            Err(err) => {
                d.stage = Stage::Form;
                d.note = Some(if matches!(*err, Error::NotFound) {
                    "The code expired. Try again.".into()
                } else {
                    rq::error_text(&err)
                });
                Task::none()
            }
        }
    }

    fn rq_signed(&mut self, e: u64, r: Res<Signed>) -> Task<Message> {
        let Some(d) = self.rq.dlg.as_mut().filter(|d| d.epoch == e) else {
            return Task::none();
        };
        d.busy = false;
        d.stage = Stage::Form;
        match r {
            Ok(s) => {
                d.unsaved = !s.saved;
                d.url = s.url;
                self.rq.quota = None;
                self.rq.public = PublicSettings::default();
                let t = self.rq_adopt(s.client, s.me);
                self.toast("Signed in to Seerr.");
                t
            }
            Err(err) => {
                d.note = Some(match *err {
                    Error::InvalidCredentials => {
                        "Seerr did not accept that username and password.".into()
                    }
                    ref other => rq::error_text(other),
                });
                Task::none()
            }
        }
    }

    fn rq_sign_out(&mut self) -> Task<Message> {
        let c = self.rq.client.clone();
        self.rq_forget();
        self.rq.page = None;
        self.rq.quota = None;
        if let Some(d) = self.rq.dlg.as_mut() {
            d.note = None;
            d.user.clear();
        }
        self.toast("Signed out of Seerr.");
        // Ends the session on the server too; the stored copy is already gone either way.
        run(
            async move {
                if let Some(c) = c {
                    c.sign_out().await;
                }
            },
            |_| Message::Noop,
        )
    }

    // ---- search ----

    /// Ask Seerr about what was typed (called with the catalog search; only when signed in).
    pub(super) fn rq_search(&self, q: String, epoch: u64) -> Task<Message> {
        let Some(c) = self.rq.client.clone() else {
            return Task::none();
        };
        run(
            async move {
                let mut p = arc(c.search(&q).await)?;
                if p.results.is_empty() {
                    // TMDB, through Seerr, now and then answers a first search with nothing at all: ask once more.
                    tokio::time::sleep(Duration::from_millis(600)).await;
                    p = arc(c.search(&q).await)?;
                }
                Ok(p.results)
            },
            move |r| Message::Requests(Msg::Hits(epoch, r)),
        )
    }

    fn rq_hits(&mut self, epoch: u64, r: Res<Vec<SearchHit>>) -> Task<Message> {
        if !self.search.as_ref().is_some_and(|u| u.is_request(epoch)) {
            return Task::none();
        }
        match r {
            Ok(hits) => {
                let paths: Vec<String> = hits
                    .iter()
                    .filter(|h| h.kind().is_some())
                    .take(rq_rows_max())
                    .filter_map(|h| h.poster_path.clone())
                    .collect();
                self.rq.hits = Some(Hits { epoch, hits });
                Task::batch(paths.into_iter().map(|p| self.rq_poster(&p)))
            }
            Err(e) => {
                self.rq_ended(&e);
                self.rq.hits = None;
                Task::none()
            }
        }
    }

    /// The Seerr hits to show under the catalog's own: movies and shows not in the catalog.
    pub(super) fn rq_rows(&self) -> Vec<&SearchHit> {
        let (Some(ui), Some(h)) = (self.search.as_ref(), self.rq.hits.as_ref()) else {
            return vec![];
        };
        if !ui.is_request(h.epoch) {
            return vec![];
        }
        let have: Vec<&Media> = ui.s.flat();
        let mut v = rq::not_in_catalog(&h.hits, &have);
        v.truncate(rq_rows_max());
        v
    }

    pub(super) fn rq_target_of(h: &SearchHit) -> Option<Target> {
        Some(Target {
            kind: h.kind()?,
            tmdb: h.id,
            title: h.display_title(),
            year: h.year(),
            poster: h.poster_path.clone(),
        })
    }

    /// What a title wears: what this session sent, else what Seerr says.
    pub(super) fn rq_badge(
        &self,
        kind: Kind,
        tmdb: u64,
        info: Option<&MediaInfo>,
    ) -> Option<Badge> {
        self.rq
            .sent
            .get(&(kind, tmdb))
            .copied()
            .or_else(|| self.native_badge(kind, tmdb))
            .or_else(|| info.and_then(rq::badge_of_info))
    }

    fn rq_poster(&mut self, path: &str) -> Task<Message> {
        let Some(c) = self.rq.client.clone() else {
            return Task::none();
        };
        if path.is_empty() || self.rq.posters.contains_key(path) {
            return Task::none();
        }
        self.rq.posters.insert(path.to_string(), Poster::Loading);
        let key = path.to_string();
        run(
            async move {
                let _permit = FETCH.acquire().await.ok()?;
                let bytes = c.poster(&key).await.ok()?;
                tokio::task::spawn_blocking(move || super::images::decode(&bytes, 192))
                    .await
                    .ok()
                    .flatten()
                    .map(|h| (key, h))
            },
            move |r| match r {
                Some((k, h)) => Message::Requests(Msg::Poster(k, Some(h))),
                None => Message::Noop,
            },
        )
    }

    // ---- the Request dialog ----

    pub(super) fn rq_ask(&mut self, t: Target) -> Task<Message> {
        if self.native_on() {
            return self.nat_ask(t);
        }
        if !self.rq_can(t.kind) {
            self.toast(if !self.settings.seerr_requests {
                "Turn on Allow requests in Settings first."
            } else {
                "Your Seerr account may not request this."
            });
            return Task::none();
        }
        let Some(c) = self.rq.client.clone() else {
            return Task::none();
        };
        self.rq.epoch += 1;
        let e = self.rq.epoch;
        let (kind, id, poster) = (t.kind, t.tmdb, t.poster.clone());
        self.rq.ask = Some(Ask {
            target: t,
            epoch: e,
            detail: Load::Loading,
            picker: None,
            stage: AskStage::Form,
        });
        let c2 = c.clone();
        let mut tasks = vec![
            run(
                async move {
                    let d = arc(c.detail(kind, id).await)?;
                    let p = arc(c.public_settings().await)?;
                    Ok(Box::new((d, p)))
                },
                move |r| Message::Requests(Msg::Detail(e, r)),
            ),
            // The account may have changed since sign-in: its rights and quota are asked again.
            run(async move { arc(c2.me().await) }, |r| {
                Message::Requests(Msg::Refreshed(r))
            }),
        ];
        if let Some(p) = poster {
            tasks.push(self.rq_poster(&p));
        }
        Task::batch(tasks)
    }

    fn rq_detail(&mut self, e: u64, r: Res<Box<(Detail, PublicSettings)>>) -> Task<Message> {
        let Some(a) = self.rq.ask.as_mut().filter(|a| a.epoch == e) else {
            return Task::none();
        };
        match r {
            Ok(b) => {
                let (d, p) = *b;
                a.picker = (a.target.kind == Kind::Tv).then(|| {
                    Picker::new(
                        rq::season_rows(&d, p.enable_special_episodes),
                        p.partial_requests_enabled,
                    )
                });
                a.detail = Load::Ready(d.clone());
                self.rq.public = p;
                let poster = d.poster_path.clone();
                self.rq.titles.insert(
                    (a.target.kind, a.target.tmdb),
                    Title::Ready(Box::new(d), Instant::now()),
                );
                poster.map_or_else(Task::none, |p| self.rq_poster(&p))
            }
            Err(err) => {
                a.detail = Load::Failed;
                a.stage = AskStage::Failed(rq::error_text(&err));
                self.rq_ended(&err);
                Task::none()
            }
        }
    }

    /// The dialog's Request button: the only place a request is sent, and the only place its token is minted.
    fn rq_confirm(&mut self) -> Task<Message> {
        let Some(c) = self.rq.client.clone() else {
            return Task::none();
        };
        let Some(kind) = self.rq.ask.as_ref().map(|a| a.target.kind) else {
            return Task::none();
        };
        let can = self.rq_can(kind);
        let Some(a) = self.rq.ask.as_mut() else {
            return Task::none();
        };
        let (Load::Ready(d), AskStage::Form | AskStage::Failed(_)) = (&a.detail, &a.stage) else {
            return Task::none();
        };
        if !can || !rq::requestable(kind, d.media_info.as_ref()) {
            return Task::none();
        }
        let seasons = match &a.picker {
            Some(p) => match p.seasons() {
                Some(s) => s,
                None => return Task::none(),
            },
            None => nocturne_seerr::Seasons::All,
        };
        let req = NewRequest {
            kind,
            tmdb_id: a.target.tmdb,
            seasons,
        };
        a.stage = AskStage::Sending;
        let e = a.epoch;
        let ok = RequestConfirmed::i_have_asked_the_user();
        run(
            async move { arc(c.create_request(&req, ok).await).map(Box::new) },
            move |r| Message::Requests(Msg::Sent(e, r)),
        )
    }

    fn rq_sent(&mut self, e: u64, r: Res<Box<Request>>) -> Task<Message> {
        let Some(a) = self.rq.ask.as_mut().filter(|a| a.epoch == e) else {
            return Task::none();
        };
        match r {
            Ok(req) => {
                let key = (a.target.kind, a.target.tmdb);
                if let Some(b) = rq::badge_of_request(req.status) {
                    self.rq.sent.insert(key, b);
                }
                // What Seerr now says about the title is read again the next time it is shown.
                self.rq.titles.remove(&key);
                a.stage = AskStage::Done(req);
                self.toast("Requested on your Seerr server.");
                self.rq_refresh()
            }
            Err(err) => {
                a.stage = AskStage::Failed(rq::error_text(&err));
                self.rq_ended(&err);
                Task::none()
            }
        }
    }

    // ---- the Requests page ----

    fn rq_open_page(&mut self) -> Task<Message> {
        if !self.requests_available() {
            return Task::none();
        }
        self.close_admin();
        self.rq.epoch += 1;
        let e = self.rq.epoch;
        self.rq.disc.view = super::discover::View::Mine;
        let seerr = self
            .rq
            .client
            .clone()
            .zip(self.rq.me.as_ref().map(|m| m.id));
        self.rq.page = Some(PageSt {
            epoch: e,
            tab: Tab::All,
            list: if seerr.is_some() {
                Load::Loading
            } else {
                Load::Ready(vec![])
            },
            cancel: None,
            busy: None,
            note: None,
        });
        let mut tasks = vec![self.disc_on_open()];
        if let Some((c, me)) = seerr {
            tasks.push(run(
                async move { arc(c.my_requests(me, 50, 0).await) },
                move |r| Message::Requests(Msg::Fetched(e, r)),
            ));
            tasks.push(self.rq_refresh());
        }
        Task::batch(tasks)
    }

    fn rq_fetched(&mut self, e: u64, r: Res<RequestsPage>) -> Task<Message> {
        let Some(p) = self.rq.page.as_mut().filter(|p| p.epoch == e) else {
            return Task::none();
        };
        match r {
            Ok(page) => {
                p.list = Load::Ready(page.results.clone());
                let mut tasks = vec![];
                for q in &page.results {
                    if let (Some(k), Some(id)) = (q.media_kind(), q.tmdb_id()) {
                        tasks.push(self.rq_want_title(k, id));
                    }
                }
                Task::batch(tasks)
            }
            Err(err) => {
                p.list = Load::Failed;
                self.rq_ended(&err);
                Task::none()
            }
        }
    }

    /// Fetch a title's details (name, year, poster) unless they are cached and fresh.
    pub(super) fn rq_want_title(&mut self, k: Kind, id: u64) -> Task<Message> {
        let Some(c) = self.rq.client.clone() else {
            return Task::none();
        };
        match self.rq.titles.get(&(k, id)) {
            Some(Title::Loading) => return Task::none(),
            Some(Title::Ready(_, at)) if at.elapsed() < FRESH => return Task::none(),
            _ => {}
        }
        self.rq.titles.insert((k, id), Title::Loading);
        run(
            async move {
                let _permit = FETCH.acquire().await.ok()?;
                // A cold Seerr often fails its first lookup (it fetches from TMDB itself), so try once more.
                if let Ok(d) = c.detail(k, id).await {
                    return Some(Box::new(d));
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                c.detail(k, id).await.ok().map(Box::new)
            },
            move |d| Message::Requests(Msg::Title(k, id, d)),
        )
    }

    fn rq_title(&mut self, k: Kind, id: u64, d: Option<Box<Detail>>) -> Task<Message> {
        match d {
            Some(d) => {
                let poster = d.poster_path.clone();
                self.rq
                    .titles
                    .insert((k, id), Title::Ready(d, Instant::now()));
                poster.map_or_else(Task::none, |p| self.rq_poster(&p))
            }
            None => {
                self.rq.titles.insert((k, id), Title::Failed);
                Task::none()
            }
        }
    }

    /// Cancel a pending request of your own: the dialog's "Cancel request". The second place a token is minted.
    fn cancel_confirmed(&mut self) -> Task<Message> {
        let Some(c) = self.rq.client.clone().filter(|_| self.requests_on()) else {
            return Task::none();
        };
        let Some(p) = self.rq.page.as_mut() else {
            return Task::none();
        };
        let Some(id) = p.cancel.take() else {
            return Task::none();
        };
        p.busy = Some(id);
        let ok = RequestConfirmed::i_have_asked_the_user();
        run(
            async move { arc(c.delete_request(id, ok).await) },
            move |r| Message::Requests(Msg::Cancelled(id, r)),
        )
    }

    fn rq_cancelled(&mut self, id: u64, r: Res<()>) -> Task<Message> {
        if let Err(e) = &r {
            self.rq_ended(e);
        }
        let Some(p) = self.rq.page.as_mut() else {
            return Task::none();
        };
        p.busy = None;
        match r {
            Ok(()) => {
                let mut gone = None;
                if let Load::Ready(v) = &mut p.list {
                    gone = v
                        .iter()
                        .find(|q| q.id == id)
                        .and_then(|q| Some((q.media_kind()?, q.tmdb_id()?)));
                    v.retain(|q| q.id != id);
                }
                if let Some(k) = gone {
                    self.rq.sent.remove(&k);
                    self.rq.titles.remove(&k);
                }
                self.toast("Request cancelled.");
                self.rq_refresh()
            }
            Err(e) => {
                p.note = Some(rq::error_text(&e));
                Task::none()
            }
        }
    }

    /// "Open" on an available entry: find it in the catalog by its TMDB id and open its page.
    fn rq_open_catalog(
        &mut self,
        k: Kind,
        id: u64,
        title: String,
        year: Option<u32>,
    ) -> Task<Message> {
        let backends: Vec<_> = self
            .catalog
            .online()
            .filter_map(|s| s.backend.clone())
            .collect();
        run(
            async move {
                for b in backends {
                    let Ok(found) = b.search(&title).await else {
                        continue;
                    };
                    if let Some(m) = found
                        .iter()
                        .find(|m| rq::in_catalog(id, k, &title, year, &[m]))
                    {
                        return Some(Box::new(m.clone()));
                    }
                }
                None
            },
            |m| Message::Requests(Msg::Found(m)),
        )
    }

    // ---- a detail page ----

    /// A movie or show page of the catalog: its Seerr state (a badge, and "Request" for seasons not asked for yet).
    pub(super) fn rq_on_detail(&mut self, pid: u64, m: &Media) -> Task<Message> {
        self.rq.chip = None;
        let kind = match m.kind {
            nocturne_core::source::Kind::Movie => Kind::Movie,
            nocturne_core::source::Kind::Series => Kind::Tv,
            _ => return Task::none(),
        };
        let (Some(c), Some(id)) = (
            self.rq.client.clone(),
            m.ids.tmdb.as_deref().and_then(|t| t.parse::<u64>().ok()),
        ) else {
            return Task::none();
        };
        run(
            async move { arc(c.detail(kind, id).await).map(Box::new) },
            move |r| Message::Requests(Msg::Chip(pid, kind, id, r)),
        )
    }

    fn rq_chip(&mut self, pid: u64, kind: Kind, id: u64, r: Res<Box<Detail>>) -> Task<Message> {
        if self.stack.last().is_none_or(|p| p.pid != pid) {
            return Task::none();
        }
        let d = match r {
            Ok(d) => d,
            Err(e) => {
                self.rq_ended(&e);
                return Task::none();
            }
        };
        let badge = self.rq_badge(kind, id, d.media_info.as_ref());
        let more = kind == Kind::Tv
            && rq::requestable(kind, d.media_info.as_ref())
            && rq::season_rows(&d, self.rq.public.enable_special_episodes)
                .iter()
                .any(|s| s.taken.is_none());
        let target = Target {
            kind,
            tmdb: id,
            title: d.display_title(),
            year: d.year(),
            poster: d.poster_path.clone(),
        };
        self.rq.chip = (badge.is_some() || more).then_some(Chip {
            pid,
            target,
            badge,
            more,
        });
        Task::none()
    }

    /// Keys while a dialog or the page of this feature is open. `None`: not ours.
    pub(super) fn rq_key(&mut self, key: &str) -> Option<Task<Message>> {
        if let Some(t) = self.disc_key(key) {
            return Some(t);
        }
        if self.rq.page.as_ref().is_some_and(|p| p.cancel.is_some()) {
            return Some(match key {
                "escape" | "backspace" => self.rq_msg(Msg::CancelNo),
                "enter" => self.rq_msg(Msg::CancelYes),
                _ => Task::none(),
            });
        }
        if self.rq.ask.is_some() {
            return Some(match key {
                "escape" | "backspace" => self.rq_msg(Msg::AskClose),
                "enter" => self.rq_msg(Msg::Confirm),
                _ => Task::none(),
            });
        }
        if self.rq.dlg.is_some() {
            return Some(match key {
                "escape" => self.rq_msg(Msg::Close),
                _ => Task::none(),
            });
        }
        // In Discover the search box is a text field: only Escape leaves (Backspace deletes).
        let typing = self.rq.disc.typing();
        if self.rq.page.is_some() && (key == "escape" || (key == "backspace" && !typing)) {
            return Some(self.rq_msg(Msg::PageClose));
        }
        None
    }
}

fn rq_rows_max() -> usize {
    8
}

/// Keep the cookie in the keyring (off the UI thread) and build the signed-in client.
async fn finish(url: String, me: Me, cookie: SessionCookie) -> Res<Signed> {
    let value = cookie.value().to_string();
    let name = secret_name(&url);
    let saved = tokio::task::spawn_blocking(move || nocturne_session::save_secret(&name, &value))
        .await
        .is_ok_and(|r| r.is_ok());
    let client = Arc::new(SeerrClient::new(&url, Auth::Cookie(cookie)).map_err(Arc::new)?);
    Ok(Signed {
        url,
        me,
        client,
        saved,
    })
}
