//! The Requests page's Discover view, requests without a Seerr server (a list kept on this computer)
//! and the optional Radarr / Sonarr connection. Rules are `nocturne_core::{discover, wishlist}`;
//! the wire is `nocturne-core`'s `discover_fetch` and `nocturne-arr`; this file is glue and state,
//! `discover_view.rs` paints. With a Seerr server signed in, requests go to Seerr as before; this
//! file is used only without one. A request is added to Radarr / Sonarr only after the request
//! dialog's confirmation: `ArrConfirmed` is minted in `nat_confirm`, nowhere else. Discovery and
//! polling only read. Nothing here ever contacts a media server.
use super::message::Message;
use super::requests::{self, Poster, Target};
use super::{App, Load};
use crate::task::{after, run};
use iced::Task;
use nocturne_arr::{
    ArrClient, ArrConfirmed, Error as ArrError, NewMovie, NewSeries, Profile, Progress, RootFolder,
    Service, secret_name,
};
use nocturne_core::discover::{self as dc, Card, Filter, List, Scope, Src};
use nocturne_core::discover_fetch::{self as df, Fail, Sources, TvInfo};
use nocturne_core::requests::{Badge, Picker, SeasonRow};
use nocturne_core::settings::ArrCfg;
use nocturne_core::source::Media;
use nocturne_core::trakt::Trakt;
use nocturne_core::wishlist::{self, Entry, Found, Wishlist};
use nocturne_seerr::Kind;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Posters and catalog probes fetched at once.
static POSTERS: Semaphore = Semaphore::const_new(4);
static PROBES: Semaphore = Semaphore::const_new(3);
/// Cards of one page that are looked up in the catalog.
const PROBE_MAX: usize = 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// The user's own requests (Seerr's list, or the native one).
    #[default]
    Mine,
    Discover,
}

/// What a successful connection test found.
#[derive(Clone)]
pub struct Tested {
    pub version: String,
    pub profiles: Vec<Profile>,
    pub folders: Vec<RootFolder>,
    pub client: Arc<ArrClient>,
}

#[derive(Clone)]
pub enum Msg {
    // the page
    View(View),
    Scope(Scope),
    List(List),
    Genre(Option<u32>),
    Year(String),
    Query(String),
    Search,
    ClearSearch,
    More,
    Retry,
    Sources(Option<String>, Option<Arc<Trakt>>),
    Loaded(u64, u32, Result<Vec<Card>, Fail>),
    Probed(String, Option<Box<Media>>),
    Open(String),
    Ask(Box<Card>),
    // the native request dialog
    Seasons(u64, Option<TvInfo>),
    Tick(u32),
    All,
    Confirm,
    Close,
    Done(u64, Result<Box<Entry>, String>),
    Remove(bool, u64),
    // the list on this computer
    Booted(Box<Wishlist>, Vec<(Service, Arc<ArrClient>)>),
    Poll(u64),
    Found(bool, u64, Found),
    Progress(bool, u64, Progress),
    // Settings > Radarr / Sonarr
    ArrOpen(Service),
    ArrClose,
    ArrUrl(String),
    ArrKey(String),
    ArrTest,
    ArrTested(u64, Result<Box<Tested>, String>),
    ArrProfile,
    ArrFolder,
    ArrSave,
    ArrDisconnect,
}

enum Stage {
    Form,
    Sending,
    Done(String),
    Failed(String),
}

pub(super) struct NatAsk {
    pub target: Target,
    pub card: Option<Card>,
    epoch: u64,
    pub picker: Option<Picker>,
    pub seasons_loading: bool,
    tvdb: Option<u64>,
    stage: Stage,
    /// The service the request will also be added to.
    pub via: Option<Service>,
}

impl NatAsk {
    pub(super) fn done(&self) -> Option<&str> {
        match &self.stage {
            Stage::Done(s) => Some(s),
            _ => None,
        }
    }
    pub(super) fn failed(&self) -> Option<&str> {
        match &self.stage {
            Stage::Failed(s) => Some(s),
            _ => None,
        }
    }
    pub(super) fn sending(&self) -> bool {
        matches!(self.stage, Stage::Sending)
    }
}

pub(super) struct ArrDlg {
    pub service: Service,
    pub url: String,
    pub key: String,
    pub note: Option<String>,
    pub busy: bool,
    epoch: u64,
    pub tested: Option<Tested>,
    pub profile: usize,
    pub folder: usize,
    /// A key was typed (else the stored one is reused).
    typed: bool,
}

#[derive(Default)]
pub struct State {
    pub(super) wish: Wishlist,
    pub(super) view: View,
    pub(super) filter: Filter,
    pub(super) query: String,
    pub(super) year_text: String,
    epoch: u64,
    page: u32,
    pub(super) cards: Option<Load<Vec<Card>>>,
    pub(super) more: bool,
    pub(super) loading_more: bool,
    pub(super) src: Option<Src>,
    pub(super) note: Option<String>,
    tmdb_key: Option<String>,
    trakt: Option<Arc<Trakt>>,
    sources_ready: bool,
    pub(super) avail: HashMap<String, Option<Media>>,
    pub(super) arr: HashMap<Service, Arc<ArrClient>>,
    pub(super) ask: Option<NatAsk>,
    pub(super) dlg: Option<ArrDlg>,
    poll_gen: u64,
    ask_epoch: u64,
    dlg_epoch: u64,
    booted: bool,
}

impl State {
    /// Discover is up: Backspace is for the search box, not for leaving the page.
    pub(super) fn typing(&self) -> bool {
        self.view == View::Discover
    }

    pub(super) fn sources_ready(&self) -> bool {
        self.sources_ready
    }

    pub(super) fn has_tmdb(&self) -> bool {
        self.tmdb_key.is_some()
    }

    pub(super) fn has_trakt(&self) -> bool {
        self.trakt.is_some()
    }
}

fn res<T>(r: Result<T, ArrError>) -> Result<T, String> {
    r.map_err(|e| arr_text(&e))
}

/// Plain words for a Radarr / Sonarr failure.
fn arr_text(e: &ArrError) -> String {
    match e {
        ArrError::Unauthorized => {
            "The API key was not accepted. Check it in the service's settings under General.".into()
        }
        ArrError::Rejected(m) => m.clone(),
        ArrError::Http(_) => "Could not reach it. Check the address and that it is running.".into(),
        ArrError::Config(m) => {
            let mut c = m.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>() + ".")
                .unwrap_or_default()
        }
        ArrError::NotFound => "That address does not look like Radarr or Sonarr.".into(),
        ArrError::Status { .. } => "It gave an unexpected answer.".into(),
    }
}

impl App {
    /// The state of a title on the native list, as a badge.
    pub(super) fn native_badge(&self, kind: Kind, tmdb: u64) -> Option<Badge> {
        if !self.native_on() {
            return None;
        }
        self.rq
            .disc
            .wish
            .get(kind == Kind::Tv, tmdb)
            .map(|e| dc::native_badge(e.status))
    }

    /// The service a native request of `kind` is also added to: allowed, connected, and with a
    /// quality profile and root folder chosen.
    fn arr_via(&self, kind: Kind) -> Option<(Arc<ArrClient>, ArrCfg)> {
        if !self.settings.arr_requests {
            return None;
        }
        let s = match kind {
            Kind::Movie => Service::Radarr,
            Kind::Tv => Service::Sonarr,
        };
        let cfg = self.arr_cfg(s).clone();
        (cfg.profile_id > 0 && !cfg.root_folder.is_empty())
            .then(|| self.rq.disc.arr.get(&s).cloned().map(|c| (c, cfg)))
            .flatten()
    }

    fn arr_cfg(&self, s: Service) -> &ArrCfg {
        match s {
            Service::Radarr => &self.settings.radarr,
            Service::Sonarr => &self.settings.sonarr,
        }
    }

    /// The Settings chip of Radarr / Sonarr.
    pub(super) fn arr_chip_label(&self, s: Service) -> String {
        match (
            self.arr_cfg(s).url.is_empty(),
            self.rq.disc.arr.contains_key(&s),
        ) {
            (true, _) => "Not set up".into(),
            (false, true) => "Connected".into(),
            (false, false) => "Key missing".into(),
        }
    }

    fn sources(&self) -> Sources {
        let d = &self.rq.disc;
        Sources {
            seerr: self.rq.client.clone(),
            tmdb_key: d.tmdb_key.clone(),
            trakt: d.trakt.clone(),
            arr: d.arr.values().cloned().collect(),
        }
    }

    // ---- launch ----

    /// Read the native list and the saved Radarr / Sonarr keys once, after launch.
    pub(super) fn disc_boot(&mut self) -> Task<Message> {
        if self.rq.disc.booted {
            return Task::none();
        }
        self.rq.disc.booted = true;
        let want: Vec<(Service, String)> = [
            (Service::Radarr, self.settings.radarr.url.clone()),
            (Service::Sonarr, self.settings.sonarr.url.clone()),
        ]
        .into_iter()
        .filter(|(_, u)| !u.is_empty())
        .collect();
        run(
            async move {
                let list = tokio::task::spawn_blocking(|| {
                    Wishlist::default_path()
                        .map(|p| Wishlist::load_from(&p))
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                let mut clients = vec![];
                for (s, url) in want {
                    let name = secret_name(s, &url);
                    let key =
                        tokio::task::spawn_blocking(move || nocturne_session::load_secret(&name))
                            .await
                            .ok()
                            .and_then(|r| r.ok())
                            .flatten();
                    if let Some(c) = key.and_then(|k| ArrClient::new(s, &url, &k).ok()) {
                        clients.push((s, Arc::new(c)));
                    }
                }
                (Box::new(list), clients)
            },
            |(l, c)| Message::Discover(Msg::Booted(l, c)),
        )
    }

    fn wish_save(&self) {
        let list = self.rq.disc.wish.clone();
        nocturne_core::bridge::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                if let Some(p) = Wishlist::default_path() {
                    let _ = list.save_to(&p);
                }
            })
            .await;
        });
    }

    /// Start (or restart) the chain of looks at the open requests.
    fn wish_kick(&mut self, delay: std::time::Duration) -> Task<Message> {
        let d = &mut self.rq.disc;
        d.poll_gen += 1;
        if d.wish.open().next().is_none() {
            return Task::none();
        }
        let g = d.poll_gen;
        after(delay, Message::Discover(Msg::Poll(g)))
    }

    /// Look for the open requests in the catalog, and ask Radarr / Sonarr how theirs are doing.
    fn wish_refresh(&mut self) -> Task<Message> {
        if !self.native_on() {
            return Task::none();
        }
        let open: Vec<Entry> = self.rq.disc.wish.open().cloned().collect();
        let backends: Vec<_> = self
            .catalog
            .online()
            .filter_map(|s| s.backend.clone())
            .collect();
        let mut tasks = vec![];
        for e in open {
            let svc = match e.via.as_deref() {
                Some("Radarr") => Some(Service::Radarr),
                Some("Sonarr") => Some(Service::Sonarr),
                _ => None,
            };
            if let Some(c) = svc.and_then(|s| self.rq.disc.arr.get(&s)) {
                let id = if e.tv { e.tvdb } else { Some(e.tmdb) };
                if let Some(id) = id {
                    let (c, tv, tmdb) = (c.clone(), e.tv, e.tmdb);
                    tasks.push(run(
                        async move { c.progress(id).await.ok() },
                        move |p| match p {
                            Some(p) => Message::Discover(Msg::Progress(tv, tmdb, p)),
                            None => Message::Noop,
                        },
                    ));
                }
            }
            let backends = backends.clone();
            tasks.push(run(
                async move {
                    let _p = PROBES.acquire().await.ok();
                    let card = Card {
                        tv: e.tv,
                        tmdb: Some(e.tmdb),
                        tvdb: e.tvdb,
                        imdb: e.imdb.clone(),
                        title: e.title.clone(),
                        year: e.year,
                        ..Default::default()
                    };
                    let mut f = Found::default();
                    for b in backends {
                        let Ok(found) = b.search(&e.title).await else {
                            continue;
                        };
                        for m in found.iter().filter(|m| dc::matches(&card, m)) {
                            f.present = true;
                            if e.tv
                                && !e.seasons.is_empty()
                                && let Ok(seasons) = b.seasons(&m.id).await
                            {
                                f.seasons.extend(seasons.iter().filter_map(|s| s.number));
                            }
                        }
                    }
                    (e.tv, e.tmdb, f)
                },
                |(tv, id, f)| Message::Discover(Msg::Found(tv, id, f)),
            ));
        }
        Task::batch(tasks)
    }

    /// The Requests page opened: look at the requests now and keep looking while it is open.
    pub(super) fn disc_on_open(&mut self) -> Task<Message> {
        let mut tasks = vec![];
        if self.native_on() {
            tasks.push(self.wish_refresh());
            tasks.push(self.wish_kick(wishlist::poll_every(true)));
        }
        if self.rq.disc.view == View::Discover {
            tasks.push(self.disc_start());
        }
        Task::batch(tasks)
    }

    // ---- messages ----

    pub(super) fn disc_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::View(v) => {
                self.rq.disc.view = v;
                if v == View::Discover {
                    return self.disc_start();
                }
                Task::none()
            }
            Msg::Scope(s) => {
                let f = &mut self.rq.disc.filter;
                f.scope = s;
                // Genres differ between movies and shows: a chosen one may not exist in the new scope.
                if let Some(g) = f.genre
                    && !dc::genres(s).iter().any(|x| x.id == g)
                {
                    f.genre = None;
                }
                self.disc_reload()
            }
            Msg::List(l) => {
                let f = &mut self.rq.disc.filter;
                f.list = l;
                (f.genre, f.year) = (None, None);
                self.rq.disc.year_text.clear();
                self.disc_reload()
            }
            Msg::Genre(g) => {
                let f = &mut self.rq.disc.filter;
                f.genre = g;
                if f.narrowed() {
                    f.list = List::Popular;
                }
                self.disc_reload()
            }
            Msg::Year(s) => {
                let d = &mut self.rq.disc;
                let digits: String = s.chars().filter(char::is_ascii_digit).take(4).collect();
                d.year_text = digits.clone();
                let year = digits
                    .parse::<u32>()
                    .ok()
                    .filter(|y| (1900..=2100).contains(y));
                if digits.is_empty() || year.is_some() {
                    d.filter.year = year;
                    if year.is_some() {
                        d.filter.list = List::Popular;
                    }
                    return self.disc_reload();
                }
                Task::none()
            }
            Msg::Query(q) => {
                self.rq.disc.query = q;
                Task::none()
            }
            Msg::Search => {
                let d = &mut self.rq.disc;
                d.filter.query = d.query.trim().to_string();
                self.disc_reload()
            }
            Msg::ClearSearch => {
                let d = &mut self.rq.disc;
                d.query.clear();
                d.filter.query.clear();
                self.disc_reload()
            }
            Msg::More => self.disc_more(),
            Msg::Retry => self.disc_reload(),
            Msg::Sources(key, trakt) => {
                let d = &mut self.rq.disc;
                (d.tmdb_key, d.trakt, d.sources_ready) = (key, trakt, true);
                self.disc_reload()
            }
            Msg::Loaded(e, page, r) => self.disc_loaded(e, page, r),
            Msg::Probed(key, m) => {
                self.rq.disc.avail.insert(key, m.map(|b| *b));
                Task::none()
            }
            Msg::Open(key) => match self.rq.disc.avail.get(&key).cloned().flatten() {
                Some(m) => {
                    self.rq.page = None;
                    self.open_any(m)
                }
                None => Task::none(),
            },
            Msg::Ask(c) => self.disc_ask(*c),
            Msg::Seasons(e, info) => self.nat_seasons(e, info),
            Msg::Tick(n) => {
                if let Some(p) = self.rq.disc.ask.as_mut().and_then(|a| a.picker.as_mut()) {
                    p.toggle(n);
                }
                Task::none()
            }
            Msg::All => {
                if let Some(p) = self.rq.disc.ask.as_mut().and_then(|a| a.picker.as_mut()) {
                    p.toggle_all();
                }
                Task::none()
            }
            Msg::Confirm => self.nat_confirm(),
            Msg::Close => {
                self.rq.disc.ask = None;
                Task::none()
            }
            Msg::Done(e, r) => self.nat_done(e, r),
            Msg::Remove(tv, id) => {
                let d = &mut self.rq.disc;
                let via = d.wish.get(tv, id).and_then(|e| e.via.clone());
                if d.wish.cancel(tv, id) {
                    self.wish_save();
                    self.toast(match via {
                        Some(v) => format!("Removed from your requests. It stays in {v}."),
                        None => "Request removed.".to_string(),
                    });
                }
                Task::none()
            }
            Msg::Booted(list, clients) => {
                let d = &mut self.rq.disc;
                d.wish = *list;
                d.arr.extend(clients);
                self.wish_kick(std::time::Duration::from_secs(20))
            }
            Msg::Poll(g) => {
                if g != self.rq.disc.poll_gen {
                    return Task::none();
                }
                let page_open = self.rq.page_open();
                let t = self.wish_refresh();
                Task::batch([t, self.wish_kick(wishlist::poll_every(page_open))])
            }
            Msg::Found(tv, id, f) => {
                let d = &mut self.rq.disc;
                let title = d.wish.get(tv, id).map(|e| e.title.clone());
                if d.wish.apply_found(tv, id, &f) {
                    self.wish_save();
                    self.toast(format!(
                        "\u{201C}{}\u{201D} is now available on your servers.",
                        title.unwrap_or_default()
                    ));
                }
                Task::none()
            }
            Msg::Progress(tv, id, p) => {
                let d = &mut self.rq.disc;
                let before = d.wish.get(tv, id).map(|e| e.status);
                let title = d.wish.get(tv, id).map(|e| e.title.clone());
                let now = d.wish.apply_progress(tv, id, p);
                if d.wish.get(tv, id).map(|e| e.status) != before {
                    self.wish_save();
                }
                if now {
                    self.toast(format!(
                        "\u{201C}{}\u{201D} is ready.",
                        title.unwrap_or_default()
                    ));
                }
                Task::none()
            }
            Msg::ArrOpen(s) => self.arr_open(s),
            Msg::ArrClose => {
                self.rq.disc.dlg = None;
                Task::none()
            }
            Msg::ArrUrl(s) => self.arr_field(|d| d.url = s),
            Msg::ArrKey(s) => self.arr_field(|d| {
                d.key = s;
                d.typed = true;
            }),
            Msg::ArrTest => self.arr_test(),
            Msg::ArrTested(e, r) => self.arr_tested(e, r),
            Msg::ArrProfile => self.arr_field(|d| {
                if let Some(t) = &d.tested
                    && !t.profiles.is_empty()
                {
                    d.profile = (d.profile + 1) % t.profiles.len();
                }
            }),
            Msg::ArrFolder => self.arr_field(|d| {
                if let Some(t) = &d.tested
                    && !t.folders.is_empty()
                {
                    d.folder = (d.folder + 1) % t.folders.len();
                }
            }),
            Msg::ArrSave => self.arr_save(),
            Msg::ArrDisconnect => self.arr_disconnect(),
        }
    }

    /// Keys while one of this file's dialogs is open. `None`: not ours.
    pub(super) fn disc_key(&mut self, key: &str) -> Option<Task<Message>> {
        if self.rq.disc.ask.is_some() {
            return Some(match key {
                "escape" | "backspace" => self.disc_msg(Msg::Close),
                "enter" => self.disc_msg(Msg::Confirm),
                _ => Task::none(),
            });
        }
        if self.rq.disc.dlg.is_some() {
            return Some(match key {
                "escape" => self.disc_msg(Msg::ArrClose),
                _ => Task::none(),
            });
        }
        None
    }

    // ---- Discover ----

    /// Entering the view: read the TMDB key and the Trakt connection once, then the first list.
    fn disc_start(&mut self) -> Task<Message> {
        if self.rq.disc.sources_ready {
            return if self.rq.disc.cards.is_none() {
                self.disc_reload()
            } else {
                Task::none()
            };
        }
        run(
            async {
                let key = tokio::task::spawn_blocking(nocturne_core::online::tmdb_key)
                    .await
                    .ok()
                    .flatten();
                let trakt = Trakt::load().await.map(Arc::new);
                (key, trakt)
            },
            |(k, t)| Message::Discover(Msg::Sources(k, t)),
        )
    }

    fn disc_reload(&mut self) -> Task<Message> {
        let have = self.sources().have();
        let d = &mut self.rq.disc;
        d.epoch += 1;
        d.page = 1;
        d.more = false;
        d.loading_more = false;
        d.note = None;
        d.src = dc::pick(have, d.filter.searching());
        let Some(src) = d.src else {
            d.cards = None;
            return Task::none();
        };
        d.cards = Some(Load::Loading);
        self.disc_fetch(src, 1)
    }

    fn disc_more(&mut self) -> Task<Message> {
        let d = &mut self.rq.disc;
        let Some(src) = d.src.filter(|_| d.more && !d.loading_more) else {
            return Task::none();
        };
        d.loading_more = true;
        d.page += 1;
        let page = d.page;
        self.disc_fetch(src, page)
    }

    fn disc_fetch(&self, src: Src, page: u32) -> Task<Message> {
        let (e, f, s) = (
            self.rq.disc.epoch,
            self.rq.disc.filter.clone(),
            self.sources(),
        );
        run(async move { df::load(src, &s, &f, page).await }, move |r| {
            Message::Discover(Msg::Loaded(e, page, r))
        })
    }

    fn disc_loaded(&mut self, e: u64, page: u32, r: Result<Vec<Card>, Fail>) -> Task<Message> {
        if e != self.rq.disc.epoch {
            return Task::none();
        }
        self.rq.disc.loading_more = false;
        match r {
            Ok(fresh) => {
                let d = &mut self.rq.disc;
                d.more = !fresh.is_empty() && d.src != Some(Src::Arr);
                let mut all = match (page, d.cards.take()) {
                    (p, Some(Load::Ready(v))) if p > 1 => v,
                    _ => vec![],
                };
                let seen = all.len();
                all.extend(fresh);
                let all = dc::dedup(all);
                // A page that added nothing new ends the list (a source repeating itself).
                d.more &= all.len() > seen;
                let new: Vec<Card> = all[seen.min(all.len())..].to_vec();
                d.cards = Some(Load::Ready(all));
                let mut tasks: Vec<Task<Message>> = vec![];
                for c in &new {
                    if let Some(p) = c.poster.clone() {
                        tasks.push(self.disc_poster(p));
                    }
                }
                tasks.push(self.disc_probe(&new));
                Task::batch(tasks)
            }
            Err(Fail::SignedOut) => {
                self.rq_ended(&nocturne_seerr::Error::Unauthorized);
                self.rq.disc.cards = None;
                Task::none()
            }
            Err(Fail::Other(m)) => {
                let d = &mut self.rq.disc;
                if page == 1 {
                    d.cards = Some(Load::Failed);
                } else {
                    d.page -= 1;
                }
                d.note = Some(m);
                Task::none()
            }
        }
    }

    fn disc_poster(&mut self, spec: String) -> Task<Message> {
        if self.rq.posters.contains_key(&spec) {
            return Task::none();
        }
        self.rq.posters.insert(spec.clone(), Poster::Loading);
        run(
            async move {
                let _p = POSTERS.acquire().await.ok()?;
                let bytes = df::poster(&spec).await?;
                let h = tokio::task::spawn_blocking(move || super::images::decode(&bytes, 280))
                    .await
                    .ok()
                    .flatten();
                Some((spec, h))
            },
            |r| match r {
                Some((k, h)) => Message::Requests(requests::Msg::Poster(k, h)),
                None => Message::Noop,
            },
        )
    }

    /// Is the title on any source? Looked up by name, matched by ids.
    fn disc_probe(&mut self, cards: &[Card]) -> Task<Message> {
        let backends: Vec<_> = self
            .catalog
            .online()
            .filter_map(|s| s.backend.clone())
            .collect();
        if backends.is_empty() {
            return Task::none();
        }
        let mut tasks = vec![];
        for c in cards.iter().take(PROBE_MAX) {
            let key = c.key();
            if self.rq.disc.avail.contains_key(&key) {
                continue;
            }
            let (card, backends) = (c.clone(), backends.clone());
            tasks.push(run(
                async move {
                    let _p = PROBES.acquire().await.ok();
                    for b in backends {
                        if let Ok(found) = b.search(&card.title).await
                            && let Some(m) = dc::find_in(&card, &found)
                        {
                            return (card.key(), Some(Box::new(m.clone())));
                        }
                    }
                    (card.key(), None)
                },
                |(k, m)| Message::Discover(Msg::Probed(k, m)),
            ));
        }
        Task::batch(tasks)
    }

    /// What a card wears: on your servers, on the list, Seerr's state, or nothing.
    pub(super) fn card_badge(&self, c: &Card) -> Option<Badge> {
        let on = matches!(self.rq.disc.avail.get(&c.key()), Some(Some(_)));
        let native = c
            .tmdb
            .and_then(|id| self.rq.disc.wish.get(c.tv, id))
            .filter(|_| self.native_on())
            .map(|e| e.status);
        let sent = c
            .tmdb
            .and_then(|id| self.rq.sent.get(&(c.kind(), id)).copied());
        dc::standing(on, native, c.info.as_ref()).or(sent)
    }

    pub(super) fn card_can_ask(&self, c: &Card) -> bool {
        c.requestable() && dc::can_ask(self.card_badge(c)) && self.rq_can(c.kind())
    }

    fn disc_ask(&mut self, c: Card) -> Task<Message> {
        let Some(tmdb) = c.tmdb else {
            return Task::none();
        };
        let t = Target {
            kind: c.kind(),
            tmdb,
            title: c.title.clone(),
            year: c.year,
            poster: c.poster.clone(),
        };
        if self.native_on() {
            self.nat_ask_with(t, Some(c))
        } else {
            self.rq_ask(t)
        }
    }

    // ---- the native request dialog ----

    pub(super) fn nat_ask(&mut self, t: Target) -> Task<Message> {
        let card = self
            .rq
            .disc
            .cards
            .as_ref()
            .and_then(|l| match l {
                Load::Ready(v) => v
                    .iter()
                    .find(|c| c.tmdb == Some(t.tmdb) && c.kind() == t.kind),
                _ => None,
            })
            .cloned();
        self.nat_ask_with(t, card)
    }

    fn nat_ask_with(&mut self, t: Target, card: Option<Card>) -> Task<Message> {
        let d = &mut self.rq.disc;
        d.ask_epoch += 1;
        let e = d.ask_epoch;
        let tv = t.kind == Kind::Tv;
        let via = self.arr_via(t.kind).map(|(c, _)| c.service());
        let (tmdb, title, year) = (t.tmdb, t.title.clone(), t.year);
        self.rq.disc.ask = Some(NatAsk {
            target: t,
            tvdb: card.as_ref().and_then(|c| c.tvdb),
            card,
            epoch: e,
            picker: None,
            seasons_loading: tv,
            stage: Stage::Form,
            via,
        });
        if !tv {
            return Task::none();
        }
        let s = self.sources();
        // The key is read here for a request made outside Discover (the Calendar).
        run(
            async move {
                let mut s = s;
                if s.tmdb_key.is_none() {
                    s.tmdb_key = tokio::task::spawn_blocking(nocturne_core::online::tmdb_key)
                        .await
                        .ok()
                        .flatten();
                }
                df::tv_info(&s, tmdb, &title, year).await
            },
            move |i| Message::Discover(Msg::Seasons(e, i)),
        )
    }

    fn nat_seasons(&mut self, e: u64, info: Option<TvInfo>) -> Task<Message> {
        let wish = self.rq.disc.wish.clone();
        let Some(a) = self.rq.disc.ask.as_mut().filter(|a| a.epoch == e) else {
            return Task::none();
        };
        a.seasons_loading = false;
        let Some(info) = info else {
            return Task::none();
        };
        a.tvdb = a.tvdb.or(info.tvdb);
        let asked = wish.get(true, a.target.tmdb);
        let rows: Vec<SeasonRow> = info
            .seasons
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, eps)| SeasonRow {
                number: *n,
                episodes: *eps,
                taken: asked
                    .filter(|x| x.seasons.is_empty() || x.seasons.contains(n))
                    .map(|x| dc::native_badge(x.status)),
            })
            .collect();
        if !rows.is_empty() {
            a.picker = Some(Picker::new(rows, true));
        }
        Task::none()
    }

    /// The dialog's Request button: the only place a request is added to Radarr / Sonarr, and the
    /// only place its token is minted.
    fn nat_confirm(&mut self) -> Task<Message> {
        let Some(kind) = self.rq.disc.ask.as_ref().map(|a| a.target.kind) else {
            return Task::none();
        };
        let plan = self.arr_via(kind);
        let Some(a) = self.rq.disc.ask.as_mut() else {
            return Task::none();
        };
        if !matches!(a.stage, Stage::Form | Stage::Failed(_)) || a.seasons_loading {
            return Task::none();
        }
        let seasons: Vec<u32> = match &a.picker {
            Some(p) => {
                if p.seasons().is_none() {
                    return Task::none();
                }
                if p.all_picked() && p.rows.iter().all(|r| r.taken.is_none()) {
                    vec![]
                } else {
                    p.picked.iter().copied().collect()
                }
            }
            None => vec![],
        };
        let c = a.card.clone().unwrap_or_default();
        let entry = Entry {
            tv: kind == Kind::Tv,
            tmdb: a.target.tmdb,
            tvdb: a.tvdb.or(c.tvdb),
            imdb: c.imdb.clone(),
            title: a.target.title.clone(),
            year: a.target.year,
            poster: a.target.poster.clone(),
            seasons: seasons.clone(),
            requested: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            ..Default::default()
        };
        a.stage = Stage::Sending;
        let e = a.epoch;
        let sources = self.sources();
        // Minted here, after the dialog's confirmation, and nowhere else.
        let ok = ArrConfirmed::i_have_asked_the_user();
        run(
            async move { add_it(entry, plan, sources, ok).await.map(Box::new) },
            move |r| Message::Discover(Msg::Done(e, r)),
        )
    }

    fn nat_done(&mut self, e: u64, r: Result<Box<Entry>, String>) -> Task<Message> {
        let Some(a) = self.rq.disc.ask.as_mut().filter(|a| a.epoch == e) else {
            return Task::none();
        };
        match r {
            Ok(entry) => {
                let via = entry.via.clone();
                a.stage = Stage::Done(match &via {
                    Some(v) => format!("Requested. It is on your list and added to {v}, which will look for it."),
                    None => "Requested. It is on your list and turns Available when it shows up on one of your sources.".into(),
                });
                self.rq.disc.wish.add(*entry);
                self.wish_save();
                self.toast("Added to your requests.");
                let d = wishlist::poll_every(self.rq.page_open());
                let t = self.wish_kick(d);
                Task::batch([t, self.wish_refresh()])
            }
            Err(m) => {
                a.stage = Stage::Failed(m);
                Task::none()
            }
        }
    }

    // ---- Settings > Radarr / Sonarr ----

    fn arr_field(&mut self, f: impl FnOnce(&mut ArrDlg)) -> Task<Message> {
        if let Some(d) = self.rq.disc.dlg.as_mut() {
            f(d);
        }
        Task::none()
    }

    fn arr_open(&mut self, s: Service) -> Task<Message> {
        let cfg = self.arr_cfg(s).clone();
        let known = self.rq.disc.arr.get(&s).cloned();
        self.rq.disc.dlg_epoch += 1;
        let e = self.rq.disc.dlg_epoch;
        self.rq.disc.dlg = Some(ArrDlg {
            service: s,
            url: cfg.url,
            key: String::new(),
            note: None,
            busy: known.is_some(),
            epoch: e,
            tested: None,
            profile: 0,
            folder: 0,
            typed: false,
        });
        // Already connected: read what the service lists again, with the saved choice selected.
        match known {
            Some(c) => run(test_it(c), move |r| Message::Discover(Msg::ArrTested(e, r))),
            None => Task::none(),
        }
    }

    fn arr_test(&mut self) -> Task<Message> {
        let existing = self
            .rq
            .disc
            .dlg
            .as_ref()
            .and_then(|d| self.rq.disc.arr.get(&d.service).cloned());
        let Some(d) = self.rq.disc.dlg.as_mut().filter(|d| !d.busy) else {
            return Task::none();
        };
        let client = match existing.filter(|_| !d.typed) {
            Some(c) => c,
            None => {
                match ArrClient::new(d.service, &d.url, &nocturne_core::secret::clean(&d.key)) {
                    Ok(c) => Arc::new(c),
                    Err(e) => {
                        d.note = Some(if d.key.trim().is_empty() {
                            "Enter the API key (in the service under Settings, General).".into()
                        } else {
                            arr_text(&e)
                        });
                        return Task::none();
                    }
                }
            }
        };
        d.busy = true;
        d.note = None;
        d.epoch += 1;
        let e = d.epoch;
        run(test_it(client), move |r| {
            Message::Discover(Msg::ArrTested(e, r))
        })
    }

    fn arr_tested(&mut self, e: u64, r: Result<Box<Tested>, String>) -> Task<Message> {
        let cfg = self
            .rq
            .disc
            .dlg
            .as_ref()
            .map(|d| self.arr_cfg(d.service).clone());
        let Some(d) = self.rq.disc.dlg.as_mut().filter(|d| d.epoch == e) else {
            return Task::none();
        };
        d.busy = false;
        match r {
            Ok(t) => {
                let cfg = cfg.unwrap_or_default();
                d.profile = t
                    .profiles
                    .iter()
                    .position(|p| p.id == cfg.profile_id)
                    .unwrap_or(0);
                d.folder = t
                    .folders
                    .iter()
                    .position(|f| f.path == cfg.root_folder)
                    .unwrap_or(0);
                d.tested = Some(*t);
            }
            Err(m) => {
                d.tested = None;
                d.note = Some(m);
            }
        }
        Task::none()
    }

    fn arr_save(&mut self) -> Task<Message> {
        let Some(d) = self.rq.disc.dlg.as_ref() else {
            return Task::none();
        };
        let Some(t) = d.tested.clone() else {
            return Task::none();
        };
        let (s, typed, typed_key) = (d.service, d.typed, nocturne_core::secret::clean(&d.key));
        let (Some(p), Some(f)) = (t.profiles.get(d.profile), t.folders.get(d.folder)) else {
            return Task::none();
        };
        let url = t.client.base_url();
        let cfg = ArrCfg {
            url: url.clone(),
            profile_id: p.id,
            profile_name: p.name.clone(),
            root_folder: f.path.clone(),
        };
        match s {
            Service::Radarr => self.settings.radarr = cfg,
            Service::Sonarr => self.settings.sonarr = cfg,
        }
        self.settings.save();
        self.rq.disc.arr.insert(s, t.client.clone());
        self.rq.disc.dlg = None;
        self.toast(format!("{} connected.", s.name()));
        // The key goes to the keyring (when it was typed) and nowhere else.
        if typed {
            let name = secret_name(s, &url);
            nocturne_core::bridge::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    nocturne_session::save_secret(&name, &typed_key)
                })
                .await;
            });
        }
        Task::none()
    }

    fn arr_disconnect(&mut self) -> Task<Message> {
        let Some(d) = self.rq.disc.dlg.take() else {
            return Task::none();
        };
        let s = d.service;
        let url = self.arr_cfg(s).url.clone();
        match s {
            Service::Radarr => self.settings.radarr = ArrCfg::default(),
            Service::Sonarr => self.settings.sonarr = ArrCfg::default(),
        }
        self.settings.save();
        self.rq.disc.arr.remove(&s);
        let name = secret_name(s, &url);
        nocturne_core::bridge::spawn(async move {
            let _ =
                tokio::task::spawn_blocking(move || nocturne_session::delete_secret(&name)).await;
        });
        self.toast(format!("{} disconnected.", s.name()));
        Task::none()
    }
}

/// Connect, read the version, the quality profiles and the root folders.
async fn test_it(c: Arc<ArrClient>) -> Result<Box<Tested>, String> {
    let status = res(c.status().await)?;
    let profiles = res(c.quality_profiles().await)?;
    let folders = res(c.root_folders().await)?;
    if profiles.is_empty() {
        return Err(format!(
            "{} has no quality profile yet. Add one there first.",
            c.service().name()
        ));
    }
    if folders.is_empty() {
        return Err(format!(
            "{} has no root folder yet. Add one there first.",
            c.service().name()
        ));
    }
    Ok(Box::new(Tested {
        version: status.version,
        profiles,
        folders,
        client: c,
    }))
}

/// Add to the connected service when there is one (the entry then records it), else just hand the
/// entry back for the list.
async fn add_it(
    mut e: Entry,
    plan: Option<(Arc<ArrClient>, ArrCfg)>,
    sources: Sources,
    ok: ArrConfirmed,
) -> Result<Entry, String> {
    let Some((c, cfg)) = plan else {
        return Ok(e);
    };
    let already = |m: &str| m.to_lowercase().contains("already");
    match c.service() {
        Service::Radarr => {
            let m = NewMovie {
                title: e.title.clone(),
                year: e.year.unwrap_or(0),
                tmdb_id: e.tmdb,
                profile_id: cfg.profile_id,
                root_folder: cfg.root_folder.clone(),
            };
            match c.add_movie(&m, ok).await {
                Ok(()) => {}
                Err(ArrError::Rejected(msg)) if already(&msg) => {}
                Err(err) => return Err(arr_text(&err)),
            }
        }
        Service::Sonarr => {
            let mut tvdb = e.tvdb;
            if tvdb.is_none() {
                tvdb = df::tv_info(&sources, e.tmdb, &e.title, e.year)
                    .await
                    .and_then(|i| i.tvdb);
            }
            let Some(tvdb) = tvdb else {
                return Err("Sonarr could not identify this show (it has no TVDB id).".into());
            };
            e.tvdb = Some(tvdb);
            let found = res(c.lookup(&format!("tvdb:{tvdb}")).await)?;
            let Some(l) = found.into_iter().find(|l| l.tvdb_id == tvdb) else {
                return Err("Sonarr does not know this show.".into());
            };
            let s = NewSeries {
                title: l.title.clone(),
                tvdb_id: tvdb,
                profile_id: cfg.profile_id,
                root_folder: cfg.root_folder.clone(),
                seasons: wishlist::arr_seasons(&l.seasons, &e.seasons),
            };
            match c.add_series(&s, ok).await {
                Ok(()) => {}
                Err(ArrError::Rejected(msg)) if already(&msg) => {}
                Err(err) => return Err(arr_text(&err)),
            }
        }
    }
    e.via = Some(c.service().name().to_string());
    Ok(e)
}
