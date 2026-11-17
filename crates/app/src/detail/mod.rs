//! Detail pages (step 4c): movie, series (season tabs, episode cards), season, episode, person
//! (filmography), collection, playlist, music album / artist, photo viewer; hero with backdrop and
//! logo, facts, chips, about, cast, chapters, extras, similar, media info, the More menu and the
//! "Versions and sources" picker. Port of the GPUI `ui_detail.rs` / `ui_playlists.rs` on the
//! catalog model. Self-contained: it owns its data, images and keyboard focus and talks to the
//! root only through [`Outcome`]s.
//!
//! ## Root hook (for the master)
//!
//! ```ignore
//! // open (any non-library, non-folder item; Kind::Library/Folder/Genre/Studio browse instead)
//! let (d, task) = detail::Detail::new(entry.primary, entry.alternates, backends, win, writes, marks);
//! page.detail = Some(d);            // Page::Detail(detail::Detail); task.map(Message::Detail)
//! // forwarding
//! Message::Detail(msg)  => { let t = d.update(msg); self.drain_detail(); t.map(Message::Detail) }
//! view                  => d.view().map(Message::Detail)          // body only; header/crumbs are overlays
//! key (page current)    => if let Some(t) = d.key(&key) { t.map(Message::Detail) } else { /* Esc/Backspace: back */ }
//! d.fullscreen()        => photo viewer is up: draw d.view() ABOVE the header and breadcrumbs
//! d.capturing()         => true while a text field or overlay owns the keys (no global shortcuts)
//! each tick / change    => d.set_win(size); d.set_writes(allow_server_writes); d.set_marks(marks)
//! after player returns  => d.patch(&media.id, |m| { m.played = ..; m.resume_s = ..; })
//! // drain d.outcomes after every update/key:
//! Outcome::Open(m)           -> open_any(m)         (Library/Folder/Genre/Studio browse instead)
//! Outcome::Play{media,rest,source_id} -> target = backend_of(media.origin).play_target_for(&media, source_id.as_deref())
//!                                        ; PlayerCtx { media, queue: rest-with-media, target, source_id, .. }
//! Outcome::Browse{kind,key,label} -> open the browse scope (genre / studio)
//! Outcome::Toast(s)          -> toast
//! Outcome::Write(w)          -> Write::Played/Favourite(m, v): catalog.write_slot(m.origin, allow_server_writes)
//!                               (server: UserClient mark_played/unplayed, favourite; local: catalog.local_for(&m)
//!                               set_played_local); Write::Playlist(op): UserClient of the signed-in account,
//!                               task::run(detail::playlist::perform(client, op.clone()), ..).
//!                               ALWAYS answer with Msg::Wrote(w, Result<(), &'static str>) (Err reverts + toasts).
//! Outcome::Prefer(m)         -> detail::versions_prefer: nocturne_core::versions::prefer(&mut prefs, &series_ids,
//!                               &m, Preferred::default_path().as_deref())
//! Outcome::Queue{tracks,next}-> queue_insert into the session music queue (nothing is sent to a server)
//! Outcome::OpenUrl(u)        -> xdg-open (http/https only, already checked)
//! Outcome::Back              -> history back (a playlist the user deleted)
//! ```
//!
//! Images: the page fetches its own through `Backend::image` into a private cache keyed by
//! (origin, item, art, width); a root-level image cache can replace it later by feeding `Msg::Pic`.
mod cells;
mod geom;
pub mod model;
mod overlays;
mod photo;
pub mod playlist;
#[cfg(debug_assertions)]
pub mod preview;
mod view;

use crate::keys::Key;
use crate::task::run;
use iced::widget::image::Handle;
use iced::widget::{operation, scrollable::AbsoluteOffset};
use iced::{Color, Size, Task};
use model::{DAct, DRow, Focus, MoreAct, Sec, Step};
use nocturne_core::backend::{Backend, PlayError};
use nocturne_core::detail::{Full, InfoBlock};
use nocturne_core::nav::Dir;
use nocturne_core::source::{Art, Failed, Kind, Media, SourceId, pick_episode, sniff};
use nocturne_core::sync::NEED_WRITES;
use nocturne_core::versions::{self, Pick, Version};
use playlist::PlaylistOp;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[allow(unused_imports)]
pub use playlist::perform as perform_playlist;

static EPOCHS: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed,
}

impl Load<Vec<Media>> {
    pub fn items(&self) -> &[Media] {
        match self {
            Load::Ready(v) => v,
            _ => &[],
        }
    }

    fn of(r: Result<Vec<Media>, Failed>) -> Self {
        r.map_or(Load::Failed, Load::Ready)
    }
}

/// Where a source's items come from, for the small mark on versions and sources.
#[derive(Clone)]
pub struct Mark {
    pub origin: SourceId,
    /// "LOCAL" or the server's label, as `widget::source::mark_text` gives it.
    pub label: String,
    /// `widget::source::status_color` of the slot.
    pub color: Color,
}

/// A change the page asks the root to make (see the hook notes above).
#[derive(Clone, Debug)]
pub enum Write {
    Played(Media, bool),
    Favourite(Media, bool),
    Playlist(PlaylistOp),
}

pub enum Outcome {
    Open(Media),
    Play {
        media: Media,
        rest: Vec<Media>,
        /// Jellyfin media source id of the chosen version.
        source_id: Option<String>,
    },
    Browse {
        kind: &'static str,
        key: String,
        label: String,
    },
    Toast(String),
    Write(Write),
    /// Make this copy's source the one shown first for the title.
    Prefer(Media),
    Queue {
        tracks: Vec<Media>,
        next: bool,
    },
    OpenUrl(String),
    /// "Find subtitles online" for this local item (the root opens the finder).
    FindSubs(Media),
    /// "Download to this computer" for this server item (the root asks where to).
    Download(Media),
    Back,
}

#[derive(Clone)]
enum Img {
    Loading,
    Ready(Handle),
    Missing,
}

#[derive(Clone)]
enum DlgKind {
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
        name: String,
    },
    Add {
        ids: Vec<String>,
        own: Vec<(String, String)>,
    },
}

struct Dialog {
    kind: DlgKind,
    text: String,
    msg: Option<&'static str>,
    busy: bool,
}

struct Photo {
    items: Vec<Media>,
    i: usize,
    z: f32,
}

#[derive(Clone)]
pub enum Msg {
    Full(u64, Result<Box<(Media, Full)>, Failed>),
    Similar(u64, Result<Vec<Media>, Failed>),
    Extras(u64, Result<Vec<Media>, Failed>),
    Colls(u64, Result<Vec<Media>, Failed>),
    NextUp(u64, Result<Vec<Media>, Failed>),
    Seasons(u64, Result<Vec<Media>, Failed>),
    Episodes(u64, String, Result<Vec<Media>, Failed>),
    Kids(u64, Result<Vec<Media>, Failed>),
    Kids2(u64, Result<Vec<Media>, Failed>),
    Neigh(u64, Box<(Option<Media>, Option<Media>)>),
    AltFull(u64, SourceId, String, Result<Box<(Media, Full)>, Failed>),
    PlayCheck(u64, Option<PlayError>),
    SeriesTarget(u64, Result<Media, Failed>),
    OwnPlaylists(u64, Vec<(String, String)>),
    Pic(String, Option<Handle>),
    Scrolled(f32, f32),
    // Pointer.
    Act(DAct),
    Chip(usize),
    Sec(Sec, usize),
    ListOpen(usize),
    ListInfo(usize),
    ListSeen(usize),
    ListFav(usize),
    SeasonTab(usize),
    Hover(DRow, usize),
    MoreOpen,
    MoreAct(MoreAct),
    VersionsOpen,
    VersionPick(usize),
    Prefer,
    DlgInput(String),
    DlgSubmit,
    /// A photo-viewer / overlay click-away.
    Dismiss,
    /// The root's answer to an [`Outcome::Write`].
    Wrote(Write, Result<(), &'static str>),
    Noop,
}

pub struct Detail {
    epoch: u64,
    backends: Vec<(SourceId, Backend)>,
    marks: Vec<Mark>,
    writes: bool,
    win: Size,
    pub media: Media,
    alternates: Vec<Media>,
    alt_info: HashMap<(SourceId, String), Vec<InfoBlock>>,
    full: Option<Full>,
    similar: Load<Vec<Media>>,
    extras: Load<Vec<Media>>,
    colls: Load<Vec<Media>>,
    kids: Load<Vec<Media>>,
    kids2: Load<Vec<Media>>,
    nextup: Load<Vec<Media>>,
    seasons: Load<Vec<Media>>,
    episodes: Load<Vec<Media>>,
    neigh: (Option<Media>, Option<Media>),
    play_err: Option<PlayError>,
    focus: Focus,
    pics: HashMap<String, Img>,
    blurs: HashMap<String, Handle>,
    scroll_y: f32,
    view_h: f32,
    more: Option<usize>,
    info_open: bool,
    /// The versions picker, with its keyboard cursor.
    ver_open: Option<usize>,
    pick: Option<Pick>,
    dialog: Option<Dialog>,
    photo: Option<Photo>,
    /// The photos of the grid the page was opened from (for the viewer's arrows).
    siblings: Vec<Media>,
    pub outcomes: Vec<Outcome>,
    /// The "Find subtitles online" entry is offered (local source with the switch on).
    pub subs_ok: bool,
    /// "Download to this computer" is offered (an item on a server).
    pub dl_ok: bool,
}

pub fn pic_key(m: &Media, art: Art, w: u32) -> String {
    let extra = if art == Art::Chapter {
        m.number.unwrap_or(0)
    } else {
        0
    };
    format!("{}|{}|{art:?}|{w}|{extra}", m.origin.0, m.id)
}

impl Detail {
    pub fn new(
        media: Media,
        alternates: Vec<Media>,
        backends: Vec<(SourceId, Backend)>,
        win: Size,
        writes: bool,
        marks: Vec<Mark>,
    ) -> (Self, Task<Msg>) {
        let mut d = Self {
            epoch: EPOCHS.fetch_add(1, Ordering::Relaxed),
            backends,
            marks,
            writes,
            win,
            media,
            alternates,
            alt_info: HashMap::new(),
            full: None,
            similar: Load::Loading,
            extras: Load::Loading,
            colls: Load::Loading,
            kids: Load::Loading,
            kids2: Load::Loading,
            nextup: Load::Loading,
            seasons: Load::Loading,
            episodes: Load::Loading,
            neigh: (None, None),
            play_err: None,
            focus: Focus::default(),
            pics: HashMap::new(),
            blurs: HashMap::new(),
            scroll_y: 0.0,
            view_h: 0.0,
            more: None,
            info_open: false,
            ver_open: None,
            pick: None,
            dialog: None,
            photo: None,
            siblings: vec![],
            outcomes: vec![],
            subs_ok: false,
            dl_ok: false,
        };
        let t = d.load();
        (d, t)
    }

    /// The photos of the grid this page came from, so the viewer can step through them.
    pub fn set_siblings(&mut self, photos: Vec<Media>) {
        self.siblings = photos;
    }

    /// Open the photo viewer straight away (a photo opened from a grid).
    pub fn open_photo(&mut self) -> Task<Msg> {
        let m = self.media.clone();
        let items: Vec<Media> = self
            .siblings
            .iter()
            .filter(|x| x.kind == Kind::Photo)
            .cloned()
            .collect();
        let at = items.iter().position(|x| x.id == m.id);
        self.photo = Some(match at {
            Some(i) => Photo { items, i, z: 1.0 },
            None => Photo {
                items: vec![m],
                i: 0,
                z: 1.0,
            },
        });
        self.want()
    }

    /// Read everything again (after playback, which moves resume and watched state).
    pub fn reload(&mut self) -> Task<Msg> {
        self.load()
    }

    pub fn set_win(&mut self, win: Size) {
        self.win = win;
    }

    pub fn set_writes(&mut self, on: bool) {
        self.writes = on;
    }

    pub fn set_marks(&mut self, marks: Vec<Mark>) {
        self.marks = marks;
    }

    /// The photo viewer covers the whole window, header included: draw `view()` above the header
    /// while this is true.
    pub fn fullscreen(&self) -> bool {
        self.photo.is_some()
    }

    /// A text field or an overlay owns the keyboard.
    pub fn capturing(&self) -> bool {
        self.dialog.is_some()
            || self.photo.is_some()
            || self.more.is_some()
            || self.info_open
            || self.ver_open.is_some()
    }

    /// Change an item the page shows (watched state after playback, a favourite toggled elsewhere).
    pub fn patch(&mut self, id: &str, f: impl Fn(&mut Media)) {
        let hit = |m: &mut Media| {
            if m.id == id {
                f(m);
            }
        };
        hit(&mut self.media);
        self.alternates.iter_mut().for_each(hit);
        for l in [
            &mut self.similar,
            &mut self.extras,
            &mut self.colls,
            &mut self.kids,
            &mut self.kids2,
            &mut self.nextup,
            &mut self.seasons,
            &mut self.episodes,
        ] {
            if let Load::Ready(v) = l {
                v.iter_mut().for_each(hit);
            }
        }
        self.neigh.0.iter_mut().for_each(hit);
        self.neigh.1.iter_mut().for_each(hit);
    }

    fn backend(&self, o: SourceId) -> Option<Backend> {
        self.backends
            .iter()
            .find(|(i, _)| *i == o)
            .map(|(_, b)| b.clone())
    }

    fn is_local(&self, o: SourceId) -> bool {
        self.backends.iter().any(|(i, b)| *i == o && b.is_local())
    }

    /// May `m` be changed now? A server item needs server changes on; the local library's own
    /// state is not a server.
    fn can_write(&self, m: &Media) -> bool {
        self.writes || self.is_local(m.origin)
    }

    fn toast(&mut self, s: impl Into<String>) {
        self.outcomes.push(Outcome::Toast(s.into()));
    }

    fn mark_of(&self, o: SourceId) -> Option<&Mark> {
        self.marks.iter().find(|m| m.origin == o)
    }

    // ---- loading ----

    fn load(&mut self) -> Task<Msg> {
        self.epoch = EPOCHS.fetch_add(1, Ordering::Relaxed);
        let ep = self.epoch;
        let m = self.media.clone();
        let Some(b) = self.backend(m.origin) else {
            return Task::none();
        };
        let id = m.id.clone();
        let mut tasks = vec![];
        let (b1, i1) = (b.clone(), id.clone());
        tasks.push(run(async move { b1.full(&i1).await }, move |r| {
            Msg::Full(ep, r.map(Box::new))
        }));
        macro_rules! list {
            ($f:expr, $msg:expr) => {{
                let (b, i) = (b.clone(), id.clone());
                tasks.push(run(async move { $f(b, i).await }, move |r| $msg(ep, r)));
            }};
        }
        if matches!(
            m.kind,
            Kind::Movie | Kind::Series | Kind::MusicAlbum | Kind::MusicArtist
        ) {
            list!(
                |b: Backend, i: String| async move { b.similar(&i).await },
                Msg::Similar
            );
        } else {
            self.similar = Load::Ready(vec![]);
        }
        if matches!(m.kind, Kind::Movie | Kind::Series) {
            list!(
                |b: Backend, i: String| async move { b.extras(&i).await },
                Msg::Extras
            );
        } else {
            self.extras = Load::Ready(vec![]);
        }
        if matches!(m.kind, Kind::Movie | Kind::Series) {
            list!(
                |b: Backend, i: String| async move { b.collections_of(&i).await },
                Msg::Colls
            );
        } else {
            self.colls = Load::Ready(vec![]);
        }
        match m.kind {
            Kind::Series => {
                list!(
                    |b: Backend, i: String| async move { b.series_next_up(&i).await },
                    Msg::NextUp
                );
                list!(
                    |b: Backend, i: String| async move { b.seasons(&i).await },
                    Msg::Seasons
                );
            }
            Kind::Season => {
                let series = m.series_id.clone().unwrap_or_default();
                let (b, i) = (b.clone(), id.clone());
                let key = String::new();
                tasks.push(run(
                    async move { b.episodes(&series, &i).await },
                    move |r| Msg::Episodes(ep, key, r),
                ));
            }
            Kind::BoxSet | Kind::MusicAlbum => {
                let (b, i) = (b.clone(), id.clone());
                tasks.push(run(async move { b.ordered_children(&i).await }, move |r| {
                    Msg::Episodes(ep, String::new(), r)
                }));
            }
            Kind::MusicArtist => {
                let (b, i) = (b.clone(), id.clone());
                tasks.push(run(async move { b.artist_albums(&i).await }, move |r| {
                    Msg::Episodes(ep, String::new(), r)
                }));
            }
            Kind::Playlist => {
                let (b, i) = (b.clone(), id.clone());
                tasks.push(run(async move { b.playlist_items(&i).await }, move |r| {
                    Msg::Episodes(ep, String::new(), r)
                }));
            }
            Kind::Person => {
                let (b2, i2) = (b.clone(), id.clone());
                list!(
                    |b: Backend, i: String| async move { b.filmography(&i, &["Movie"]).await },
                    Msg::Kids
                );
                tasks.push(run(
                    async move { b2.filmography(&i2, &["Series"]).await },
                    move |r| Msg::Kids2(ep, r),
                ));
            }
            Kind::Episode => {
                let (b, e) = (b.clone(), m.clone());
                tasks.push(run(async move { b.episode_around(&e).await }, move |n| {
                    Msg::Neigh(ep, Box::new(n))
                }));
            }
            _ => {}
        }
        if !model::has_list(m.kind) {
            self.episodes = Load::Ready(vec![]);
        }
        if !matches!(m.kind, Kind::Series) {
            self.nextup = Load::Ready(vec![]);
            self.seasons = Load::Ready(vec![]);
        }
        if m.kind != Kind::Person {
            self.kids = Load::Ready(vec![]);
            self.kids2 = Load::Ready(vec![]);
        }
        // A local file can be missing or unreadable: say so instead of offering Play.
        if b.is_local() && m.playable() {
            let (b, m2) = (b.clone(), m.clone());
            tasks.push(run(
                async move { b.play_target(&m2).await.err() },
                move |e| Msg::PlayCheck(ep, e),
            ));
        }
        // What the other copies are like, for the versions picker.
        for a in self.alternates.clone() {
            let Some(ab) = self.backend(a.origin) else {
                continue;
            };
            let (o, i) = (a.origin, a.id.clone());
            tasks.push(run(async move { ab.full(&a.id).await }, move |r| {
                Msg::AltFull(ep, o, i, r.map(Box::new))
            }));
        }
        tasks.push(self.want());
        Task::batch(tasks)
    }

    /// Start fetching the season's episodes (the tabs of a series).
    fn load_season(&mut self, at: usize) -> Task<Msg> {
        let Some(s) = self.seasons.items().get(at).cloned() else {
            return Task::none();
        };
        let (ep, series) = (self.epoch, self.media.id.clone());
        let Some(b) = self.backend(self.media.origin) else {
            return Task::none();
        };
        self.episodes = Load::Loading;
        self.focus.ep = 0;
        let key = s.id.clone();
        run(async move { b.episodes(&series, &s.id).await }, move |r| {
            Msg::Episodes(ep, key, r)
        })
    }

    // ---- images ----

    fn img(&self, m: &Media, art: Art, w: u32) -> Option<&Handle> {
        match self.pics.get(&pic_key(m, art, w)) {
            Some(Img::Ready(h)) => Some(h),
            _ => None,
        }
    }

    fn blur(&self, m: &Media, art: Art) -> Option<&Handle> {
        self.blurs.get(m.blur_for(art)?)
    }

    fn fetch(&mut self, m: &Media, art: Art, w: u32) -> Task<Msg> {
        let key = pic_key(m, art, w);
        if self.pics.contains_key(&key) {
            return Task::none();
        }
        if let Some(h) = m.blur_for(art)
            && !self.blurs.contains_key(h)
            && let Some(px) = nocturne_core::blurhash::decode(h, 32, 32)
        {
            self.blurs
                .insert(h.to_string(), Handle::from_rgba(32, 32, px));
        }
        let Some(b) = self.backend(m.origin) else {
            self.pics.insert(key, Img::Missing);
            return Task::none();
        };
        self.pics.insert(key.clone(), Img::Loading);
        let m = m.clone();
        run(async move { b.image(&m, art, w).await }, move |bytes| {
            Msg::Pic(
                key,
                bytes.filter(|b| sniff(b).is_some()).map(Handle::from_bytes),
            )
        })
    }

    /// Everything on screen that needs a picture and does not have one yet.
    fn want(&mut self) -> Task<Msg> {
        let mut wants: Vec<(Media, Art, u32)> = vec![];
        let m = self.media.clone();
        wants.push((m.clone(), geom::hero_art(&m), 1920));
        if m.logo.is_some() {
            wants.push((m.clone(), Art::Logo, 800));
        }
        if geom::about_poster(&m) {
            wants.push((m.clone(), Art::Poster, 480));
        }
        if let Some(p) = &self.photo {
            wants.extend(p.items.get(p.i).map(|x| (x.clone(), Art::Poster, 2560)));
        }
        if model::has_list(m.kind) {
            for e in self.episodes.items() {
                wants.push(if model::card_grid(m.kind) {
                    (e.clone(), Art::Landscape, geom::CARD_PX)
                } else {
                    (e.clone(), Art::Poster, 320)
                });
            }
        }
        for s in Sec::ALL {
            let (art, w) = geom::strip_art(s);
            wants.extend(self.sec_items(s).iter().map(|e| (e.clone(), art, w)));
        }
        Task::batch(
            wants
                .iter()
                .map(|(m, a, w)| self.fetch(m, *a, *w))
                .collect::<Vec<_>>(),
        )
    }

    // ---- data views ----

    pub(crate) fn sec_items(&self, s: Sec) -> &[Media] {
        match s {
            Sec::Cast => self.full.as_ref().map_or(&[], |f| &f.people),
            Sec::Chapters => self.full.as_ref().map_or(&[], |f| &f.chapters),
            Sec::Similar => self.similar.items(),
            Sec::Extras => self.extras.items(),
            Sec::Collections => self.colls.items(),
            Sec::Kids => self.kids.items(),
            Sec::Kids2 => self.kids2.items(),
            Sec::Next => self.nextup.items(),
            // Redundant for a single season: the tabs already cover it.
            Sec::Seasons if self.seasons.items().len() > 1 => self.seasons.items(),
            Sec::Seasons => &[],
        }
    }

    pub(crate) fn chips(&self) -> Vec<model::Chip> {
        model::chips(&self.media, self.full.as_ref())
    }

    /// The copies and their files, in the order the picker lists them.
    pub(crate) fn versions(&self) -> Vec<Version> {
        let info = |m: &Media| {
            if m.origin == self.media.origin && m.id == self.media.id {
                self.full.as_ref().map(|f| f.info.clone())
            } else {
                self.alt_info.get(&(m.origin, m.id.clone())).cloned()
            }
            .unwrap_or_default()
        };
        let copies: Vec<(Media, Vec<InfoBlock>)> = std::iter::once(&self.media)
            .chain(&self.alternates)
            .map(|m| (m.clone(), info(m)))
            .collect();
        versions::build(&copies)
    }

    /// Index of the chosen version in [`Self::versions`].
    pub(crate) fn picked(&self, list: &[Version]) -> usize {
        versions::selected(list, self.pick.as_ref())
    }

    /// What Play hands over: the chosen version's copy and media source.
    fn play_choice(&self) -> (Media, Option<String>) {
        let list = self.versions();
        let Some(v) = list.get(self.picked(&list)) else {
            return (self.media.clone(), None);
        };
        let m = std::iter::once(&self.media)
            .chain(&self.alternates)
            .find(|m| m.origin == v.origin && m.id == v.item_id)
            .cloned()
            .unwrap_or_else(|| self.media.clone());
        (m, v.source_id.clone())
    }

    pub(crate) fn dacts(&self) -> Vec<DAct> {
        let mut v = model::actions(&self.media, self.neigh.0.is_some(), self.neigh.1.is_some());
        if self.play_err.is_some() {
            v.retain(|a| *a != DAct::Play);
        }
        if versions::worth_showing(&self.versions()) && self.media.playable() {
            let at = v.iter().position(|a| *a == DAct::Play).map_or(0, |i| i + 1);
            v.insert(at, DAct::Versions);
        }
        v
    }

    pub(crate) fn shape(&self) -> model::Shape {
        let mut rows = vec![DRow::Actions];
        let mut lens = vec![self.dacts().len()];
        let chips = self.chips().len();
        if chips > 0 {
            rows.push(DRow::Chips);
            lens.push(chips);
        }
        let ns = self.seasons.items().len();
        if self.media.kind == Kind::Series && ns > 0 {
            rows.push(DRow::Seasons);
            lens.push(ns);
        }
        let ne = self.episodes.items().len();
        if model::has_list(self.media.kind) && ne > 0 {
            rows.push(DRow::List);
            lens.push(ne);
        }
        for s in Sec::ALL {
            let n = self.sec_items(s).len();
            if n > 0 {
                rows.push(DRow::Strip(s));
                lens.push(n);
            }
        }
        model::Shape {
            rows,
            lens,
            cols: if model::card_grid(self.media.kind) {
                geom::card_cols(self.win.width)
            } else {
                1
            },
        }
    }

    fn owned_playlist(&self) -> bool {
        self.media.kind == Kind::Playlist && self.media.can_delete
    }

    /// The playable things on the page: the item itself, or the items of a list page.
    fn playables(&self) -> Vec<Media> {
        match self.media.kind {
            k if self.media.playable() && k != Kind::Series => vec![self.media.clone()],
            Kind::MusicAlbum | Kind::Playlist | Kind::BoxSet => self
                .episodes
                .items()
                .iter()
                .filter(|e| e.playable())
                .cloned()
                .collect(),
            _ => vec![],
        }
    }

    pub(crate) fn more_items(&self) -> Vec<(&'static str, MoreAct)> {
        let m = &self.media;
        let writes = self.can_write(m);
        let mut v = vec![];
        let playable = matches!(
            m.kind,
            Kind::Movie | Kind::Episode | Kind::Video | Kind::Audio
        );
        if playable && self.play_err.is_none() {
            v.push(("Play from beginning", MoreAct::FromStart));
        }
        if playable && writes {
            v.push((
                if m.played {
                    "Mark as unwatched"
                } else {
                    "Mark as watched"
                },
                MoreAct::Watched,
            ));
        }
        if writes {
            v.push((
                if m.favorite {
                    "Remove from favourites"
                } else {
                    "Add to favourites"
                },
                MoreAct::Fav,
            ));
        }
        let tracks = self.playables().iter().any(|t| t.kind == Kind::Audio);
        if tracks {
            v.push(("Play next", MoreAct::PlayNext));
            v.push(("Add to queue", MoreAct::AddToQueue));
        }
        if self.writes && !self.playables().is_empty() && m.kind != Kind::Playlist {
            v.push(("Add to playlist\u{2026}", MoreAct::AddToPlaylist));
        }
        if self.writes && self.owned_playlist() {
            v.push(("Rename playlist\u{2026}", MoreAct::RenamePlaylist));
            v.push(("Delete playlist\u{2026}", MoreAct::DeletePlaylist));
        }
        if self.full.as_ref().is_some_and(|f| !f.info.is_empty()) {
            v.push(("Media info", MoreAct::Info));
        }
        if self.subs_ok && matches!(m.kind, Kind::Movie | Kind::Episode) {
            v.push(("Find subtitles online\u{2026}", MoreAct::FindSubs));
        }
        if self.dl_ok
            && matches!(
                m.kind,
                Kind::Movie | Kind::Episode | Kind::Season | Kind::Series
            )
        {
            v.push(("Download to this computer\u{2026}", MoreAct::Download));
        }
        v
    }

    // ---- update ----

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let cur = self.epoch;
        match msg {
            Msg::Full(e, r) if e == cur => {
                if let Ok((m, full)) = r.map(|b| *b) {
                    // Keep what the list knew if the detail lacks it.
                    let old = std::mem::replace(&mut self.media, m);
                    self.media.resume_s = self.media.resume_s.or(old.resume_s);
                    self.media.available = old.available && self.media.available;
                    self.full = Some(full);
                }
                return self.want();
            }
            Msg::Similar(e, r) if e == cur => self.similar = Load::of(r),
            Msg::Extras(e, r) if e == cur => self.extras = Load::of(r),
            Msg::Colls(e, r) if e == cur => self.colls = Load::of(r),
            Msg::NextUp(e, r) if e == cur => self.nextup = Load::of(r),
            Msg::Kids(e, r) if e == cur => self.kids = Load::of(r),
            Msg::Kids2(e, r) if e == cur => self.kids2 = Load::of(r),
            Msg::Seasons(e, r) if e == cur => {
                self.seasons = Load::of(r);
                return Task::batch([self.load_season(0), self.want()]);
            }
            Msg::Episodes(e, season, r) if e == cur => {
                // A slow answer for a season the user has already left is dropped.
                let wanted = self.seasons.items().get(self.focus.season).map(|s| &s.id);
                if self.media.kind == Kind::Series && wanted != Some(&season) {
                    return Task::none();
                }
                self.episodes = Load::of(r);
            }
            Msg::Neigh(e, n) if e == cur => self.neigh = *n,
            Msg::AltFull(e, o, id, r) if e == cur => {
                if let Ok((_, f)) = r.map(|b| *b) {
                    self.alt_info.insert((o, id), f.info);
                }
            }
            Msg::PlayCheck(e, err) if e == cur => self.play_err = err,
            Msg::SeriesTarget(e, r) if e == cur => match r {
                Ok(ep) => self.outcomes.push(Outcome::Play {
                    media: ep,
                    rest: vec![],
                    source_id: None,
                }),
                Err(_) => self.toast("Nothing to play here yet."),
            },
            Msg::OwnPlaylists(e, own) if e == cur => {
                let ids = self.playables().into_iter().map(|m| m.id).collect();
                self.dialog = Some(Dialog {
                    kind: DlgKind::Add { ids, own },
                    text: String::new(),
                    msg: None,
                    busy: false,
                });
                return operation::focus(overlays::DLG_ID);
            }
            Msg::Full(..)
            | Msg::Similar(..)
            | Msg::Extras(..)
            | Msg::Colls(..)
            | Msg::NextUp(..)
            | Msg::Kids(..)
            | Msg::Kids2(..)
            | Msg::Seasons(..)
            | Msg::Episodes(..)
            | Msg::Neigh(..)
            | Msg::AltFull(..)
            | Msg::PlayCheck(..)
            | Msg::SeriesTarget(..)
            | Msg::OwnPlaylists(..) => return Task::none(), // an answer for a page that was reloaded
            Msg::Pic(key, h) => {
                self.pics.insert(key, h.map_or(Img::Missing, Img::Ready));
                return Task::none();
            }
            Msg::Scrolled(y, h) => {
                self.scroll_y = y;
                self.view_h = h;
                return Task::none();
            }
            Msg::Act(a) => return self.do_act(a),
            Msg::Chip(i) => self.do_chip(i),
            Msg::Sec(s, i) => {
                self.focus_on(DRow::Strip(s), i);
                return self.do_sec(s, i);
            }
            Msg::ListOpen(i) => {
                self.focus_on(DRow::List, i);
                self.list_open(i);
            }
            Msg::ListInfo(i) => {
                if let Some(e) = self.episodes.items().get(i).cloned() {
                    self.outcomes.push(Outcome::Open(e));
                }
            }
            Msg::ListSeen(i) => {
                if let Some(e) = self.episodes.items().get(i).cloned() {
                    self.toggle_played(e);
                }
            }
            Msg::ListFav(i) => {
                if let Some(e) = self.episodes.items().get(i).cloned() {
                    self.toggle_fav(e);
                }
            }
            Msg::SeasonTab(i) => {
                if let Some(r) = self.shape().rows.iter().position(|r| *r == DRow::Seasons) {
                    self.focus.row = r;
                }
                if i != self.focus.season || !matches!(self.episodes, Load::Ready(_)) {
                    self.focus.season = i;
                    return Task::batch([self.load_season(i), self.want()]);
                }
            }
            Msg::Hover(r, i) => {
                self.focus_on(r, i);
                return Task::none();
            }
            Msg::MoreOpen => self.open_more(),
            Msg::MoreAct(a) => return self.more_act(a),
            Msg::VersionsOpen => {
                let l = self.versions();
                self.ver_open = Some(self.picked(&l));
            }
            Msg::VersionPick(i) => self.choose_version(i),
            Msg::Prefer => self.prefer_selected(),
            Msg::DlgInput(s) => {
                if let Some(d) = self.dialog.as_mut().filter(|d| !d.busy) {
                    d.text = s;
                    d.msg = None;
                }
            }
            Msg::DlgSubmit => self.dialog_submit(),
            Msg::Dismiss => {
                self.more = None;
                self.info_open = false;
                self.ver_open = None;
                self.photo = None;
                if self.dialog.as_ref().is_some_and(|d| !d.busy) {
                    self.dialog = None;
                }
            }
            Msg::Wrote(w, r) => return self.wrote(w, r),
            Msg::Noop => {}
        }
        self.want()
    }

    fn focus_on(&mut self, r: DRow, i: usize) {
        let s = self.shape();
        let Some(row) = s.rows.iter().position(|x| *x == r) else {
            return;
        };
        self.focus.row = row;
        if r == DRow::List {
            self.focus.ep = i;
        } else {
            self.focus.col = i;
        }
    }

    fn open_more(&mut self) {
        if self.more_items().is_empty() {
            self.toast("Nothing more to do here.");
        } else {
            self.more = Some(0);
        }
    }

    fn do_chip(&mut self, i: usize) {
        match self.chips().get(i).cloned() {
            Some(model::Chip::Genre(g)) => self.outcomes.push(Outcome::Browse {
                kind: "genre",
                key: g.clone(),
                label: g,
            }),
            Some(model::Chip::Studio(id, n)) => self.outcomes.push(Outcome::Browse {
                kind: "studio",
                key: id,
                label: n,
            }),
            // Only ever http(s): checked when the link was mapped.
            Some(model::Chip::Link(_, url)) => self.outcomes.push(Outcome::OpenUrl(url)),
            None => {}
        }
    }

    fn do_sec(&mut self, s: Sec, i: usize) -> Task<Msg> {
        let Some(m) = self.sec_items(s).get(i).cloned() else {
            return Task::none();
        };
        match s {
            Sec::Chapters => {
                let mut pm = self.media.clone();
                pm.resume_s = m.resume_s;
                pm.played = false;
                self.play_media(pm, vec![], None);
            }
            Sec::Extras | Sec::Next => self.play_media(m, vec![], None),
            _ => self.outcomes.push(Outcome::Open(m)),
        }
        Task::none()
    }

    /// Enter / click on a list row: playables play, containers open.
    fn list_open(&mut self, i: usize) {
        let Some(e) = self.episodes.items().get(i).cloned() else {
            return;
        };
        if e.playable() {
            self.play_media(e, vec![], None);
        } else {
            self.outcomes.push(Outcome::Open(e));
        }
    }

    /// Start playback of `m` unless it cannot be (said calmly instead).
    fn play_media(&mut self, m: Media, rest: Vec<Media>, source_id: Option<String>) {
        match m.kind {
            Kind::Photo => {
                self.photo = Some(Photo {
                    items: vec![m],
                    i: 0,
                    z: 1.0,
                })
            }
            Kind::Book | Kind::Folder | Kind::Person | Kind::Other => {
                self.toast("Not playable in NOCTURNE yet.")
            }
            _ if !m.playable() => {}
            _ => self.outcomes.push(Outcome::Play {
                media: m,
                rest,
                source_id,
            }),
        }
    }

    fn play_list(&mut self, shuffle: bool) {
        let mut v: Vec<Media> = self
            .episodes
            .items()
            .iter()
            .filter(|m| m.playable())
            .cloned()
            .collect();
        if shuffle {
            v = model::shuffled(v);
        }
        if v.is_empty() {
            return self.toast("Nothing to play here yet.");
        }
        let first = v.remove(0);
        self.play_media(first, v, None);
    }

    fn do_act(&mut self, a: DAct) -> Task<Msg> {
        let m = self.media.clone();
        match a {
            DAct::Play if m.kind == Kind::Season => {
                match pick_episode(self.episodes.items()).cloned() {
                    Some(e) => self.play_media(e, vec![], None),
                    None => self.toast("Nothing to play here yet."),
                }
            }
            DAct::Play if m.kind == Kind::Series => {
                let Some(b) = self.backend(m.origin) else {
                    return Task::none();
                };
                let ep = self.epoch;
                return run(async move { b.series_target(&m.id).await }, move |r| {
                    Msg::SeriesTarget(ep, r)
                });
            }
            DAct::Play => match self.play_err {
                Some(e) => self.toast(e.message()),
                None => {
                    let (media, sid) = self.play_choice();
                    self.play_media(media, vec![], sid);
                }
            },
            DAct::View => return self.open_photo(),
            DAct::Versions => {
                let l = self.versions();
                self.ver_open = Some(self.picked(&l));
            }
            DAct::PlayAll => self.play_list(false),
            DAct::Shuffle => self.play_list(true),
            DAct::Watched => self.toggle_played(m),
            DAct::Fav => self.toggle_fav(m),
            DAct::More => self.open_more(),
            DAct::Prev => {
                if let Some(e) = self.neigh.0.clone() {
                    self.outcomes.push(Outcome::Open(e));
                }
            }
            DAct::Next => {
                if let Some(e) = self.neigh.1.clone() {
                    self.outcomes.push(Outcome::Open(e));
                }
            }
            DAct::Series => {
                if let Some(id) = m.series_id.clone() {
                    let mut s = Media::from_item_in(m.origin, &nocturne_jellyfin::Item::default());
                    s.id = id;
                    s.kind = Kind::Series;
                    s.title = m.series_name.clone().unwrap_or_default();
                    self.outcomes.push(Outcome::Open(s));
                }
            }
        }
        Task::none()
    }

    fn more_act(&mut self, a: MoreAct) -> Task<Msg> {
        self.more = None;
        let m = self.media.clone();
        match a {
            MoreAct::FromStart => {
                let (media, sid) = self.play_choice();
                self.play_media(
                    Media {
                        resume_s: None,
                        ..media
                    },
                    vec![],
                    sid,
                );
            }
            MoreAct::Watched => self.toggle_played(m),
            MoreAct::Fav => self.toggle_fav(m),
            MoreAct::Info => self.info_open = true,
            MoreAct::FindSubs => self.outcomes.push(Outcome::FindSubs(m)),
            MoreAct::Download => self.outcomes.push(Outcome::Download(m)),
            MoreAct::AddToPlaylist => {
                if self.playables().is_empty() {
                    self.toast("Nothing to add here.");
                } else if !self.writes {
                    self.toast(NEED_WRITES);
                } else if let Some(b) = self.backend(m.origin) {
                    let ep = self.epoch;
                    return run(
                        async move { b.own_playlists().await.unwrap_or_default() },
                        move |own| Msg::OwnPlaylists(ep, own),
                    );
                }
            }
            MoreAct::RenamePlaylist => {
                self.dialog = Some(Dialog {
                    text: m.title.clone(),
                    kind: DlgKind::Rename {
                        id: m.id,
                        name: m.title,
                    },
                    msg: None,
                    busy: false,
                });
                return operation::focus(overlays::DLG_ID);
            }
            MoreAct::DeletePlaylist => {
                self.dialog = Some(Dialog {
                    text: String::new(),
                    kind: DlgKind::Delete {
                        id: m.id,
                        name: m.title,
                    },
                    msg: None,
                    busy: false,
                });
                return operation::focus(overlays::DLG_ID);
            }
            MoreAct::PlayNext | MoreAct::AddToQueue => {
                let tracks: Vec<Media> = self
                    .playables()
                    .into_iter()
                    .filter(|t| t.kind == Kind::Audio)
                    .collect();
                if tracks.is_empty() {
                    self.toast("No music to queue here.");
                } else {
                    let next = a == MoreAct::PlayNext;
                    self.outcomes.push(Outcome::Queue { tracks, next });
                    self.toast(if next {
                        "Playing next."
                    } else {
                        "Added to the queue."
                    });
                }
            }
        }
        Task::none()
    }

    // ---- writes ----

    fn toggle_played(&mut self, m: Media) {
        if !self.can_write(&m) {
            return self.toast(NEED_WRITES);
        }
        let v = !m.played;
        self.patch(&m.id, |x| {
            x.played = v;
            if v {
                x.resume_s = None;
            }
        });
        self.outcomes.push(Outcome::Write(Write::Played(m, v)));
        self.toast(if v {
            "Marked as watched."
        } else {
            "Marked as unwatched."
        });
    }

    fn toggle_fav(&mut self, m: Media) {
        if !self.can_write(&m) {
            return self.toast(NEED_WRITES);
        }
        let v = !m.favorite;
        self.patch(&m.id, |x| x.favorite = v);
        self.outcomes.push(Outcome::Write(Write::Favourite(m, v)));
        self.toast(if v {
            "Added to favourites."
        } else {
            "Removed from favourites."
        });
    }

    /// The root's answer to a write: a failure undoes the optimistic change.
    fn wrote(&mut self, w: Write, r: Result<(), &'static str>) -> Task<Msg> {
        match (w, r) {
            (Write::Played(m, v), Err(e)) => {
                self.patch(&m.id, |x| {
                    x.played = !v;
                    x.resume_s = m.resume_s;
                });
                self.toast(if e.is_empty() {
                    "Couldn't save that change, so it was undone."
                } else {
                    e
                });
            }
            (Write::Favourite(m, v), Err(e)) => {
                self.patch(&m.id, |x| x.favorite = !v);
                self.toast(if e.is_empty() {
                    "Couldn't save that change, so it was undone."
                } else {
                    e
                });
            }
            (Write::Playlist(op), Err(e)) => match self.dialog.as_mut() {
                Some(d) => {
                    d.busy = false;
                    d.msg = Some(e);
                }
                None => {
                    let _ = op;
                    self.toast(e);
                }
            },
            (Write::Playlist(op), Ok(())) => {
                self.dialog = None;
                self.toast(op.done_text());
                if matches!(op, PlaylistOp::Delete { .. }) {
                    self.outcomes.push(Outcome::Back);
                } else {
                    if let PlaylistOp::Move { to, .. } = &op {
                        self.focus.ep = *to;
                    }
                    if let PlaylistOp::Remove { idx, .. } = &op {
                        let n = self.episodes.items().len();
                        self.focus.ep = (*idx).min(n.saturating_sub(2));
                    }
                    let keep = self.focus.ep;
                    let t = self.load();
                    self.focus.ep = keep;
                    return t;
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn playlist_edit(&mut self, e: model::Edit) {
        if !self.owned_playlist() {
            return;
        }
        if !self.writes {
            return self.toast(NEED_WRITES);
        }
        let (idx, items) = (self.focus.ep, self.episodes.items());
        let Some(item) = items.get(idx).map(|m| m.id.clone()) else {
            return;
        };
        let Some(plan) = e.plan(idx, items.len()) else {
            return self.toast("Already at the end.");
        };
        let id = self.media.id.clone();
        let op = match plan {
            None => PlaylistOp::Remove { id, idx, item },
            Some(to) => PlaylistOp::Move { id, idx, item, to },
        };
        self.outcomes.push(Outcome::Write(Write::Playlist(op)));
    }

    fn dialog_submit(&mut self) {
        let Some(d) = self.dialog.as_mut().filter(|d| !d.busy) else {
            return;
        };
        let text = d.text.trim().to_string();
        let op = match &d.kind {
            DlgKind::Rename { .. } | DlgKind::Add { .. } if text.is_empty() => {
                d.msg = Some("A playlist needs a name.");
                return;
            }
            DlgKind::Delete { name, .. } if d.text != *name => {
                d.msg = Some("Type the playlist name exactly to confirm.");
                return;
            }
            DlgKind::Rename { id, .. } => PlaylistOp::Rename {
                id: id.clone(),
                name: text,
            },
            DlgKind::Delete { id, .. } => PlaylistOp::Delete { id: id.clone() },
            DlgKind::Add { ids, own } => PlaylistOp::Add {
                ids: ids.clone(),
                name: text,
                own: own.clone(),
            },
        };
        if !self.writes {
            d.msg = Some(NEED_WRITES);
            return;
        }
        d.busy = true;
        d.msg = None;
        self.outcomes.push(Outcome::Write(Write::Playlist(op)));
    }

    // ---- versions ----

    fn choose_version(&mut self, i: usize) {
        let l = self.versions();
        if let Some(v) = l.get(i) {
            self.pick = Some(v.pick());
            self.ver_open = Some(i);
        }
    }

    /// "Always prefer this source for this title": the chosen copy's source goes first from now on.
    fn prefer_selected(&mut self) {
        let l = self.versions();
        let Some(v) = l.get(self.picked(&l)) else {
            return;
        };
        let m = std::iter::once(&self.media)
            .chain(&self.alternates)
            .find(|m| m.origin == v.origin && m.id == v.item_id)
            .cloned();
        if let Some(m) = m {
            self.outcomes.push(Outcome::Prefer(m));
            self.toast("This source will be shown first for this title.");
            self.ver_open = None;
        }
    }

    // ---- keyboard ----

    /// `None`: not consumed (the root's Esc / Backspace goes back).
    pub fn key(&mut self, k: &Key) -> Option<Task<Msg>> {
        let name = k.name.as_str();
        if self.photo.is_some() {
            return Some(self.photo_key(name));
        }
        if let Some(d) = self.dialog.as_ref() {
            if matches!(name, "escape") && !d.busy {
                self.dialog = None;
            }
            return Some(Task::none());
        }
        if self.info_open {
            if matches!(name, "escape" | "backspace" | "enter" | "i") {
                self.info_open = false;
            }
            return Some(Task::none());
        }
        if let Some(sel) = self.ver_open {
            let n = self.versions().len();
            match name {
                "escape" | "backspace" | "v" => self.ver_open = None,
                "up" => self.ver_open = Some(sel.saturating_sub(1)),
                "down" => self.ver_open = Some((sel + 1).min(n.saturating_sub(1))),
                "enter" | "space" => self.choose_version(sel),
                "p" => {
                    self.choose_version(sel);
                    self.prefer_selected();
                }
                _ => {}
            }
            return Some(Task::none());
        }
        if let Some(sel) = self.more {
            let items = self.more_items();
            match name {
                "escape" | "backspace" => self.more = None,
                "up" => self.more = Some(sel.saturating_sub(1)),
                "down" => self.more = Some((sel + 1).min(items.len().saturating_sub(1))),
                "enter" => {
                    if let Some((_, a)) = items.get(sel) {
                        return Some(self.more_act(*a));
                    }
                }
                _ => {}
            }
            return Some(Task::none());
        }
        let dir = match name {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        let shape = self.shape();
        let cur = shape.current(&self.focus);
        if let Some(d) = dir {
            let at = self.focus;
            let st = model::step(&mut self.focus, d, &shape);
            let mut tasks = vec![];
            if let Step::Season(i) = st {
                tasks.push(self.load_season(i));
                tasks.push(self.want());
            }
            if st != Step::Same || at != self.focus {
                tasks.push(self.reveal());
            }
            return Some(Task::batch(tasks));
        }
        match name {
            "enter" => {
                let t = self.activate(cur);
                return Some(Task::batch([t, self.want()]));
            }
            "v" if versions::worth_showing(&self.versions()) => {
                let l = self.versions();
                self.ver_open = Some(self.picked(&l));
                return Some(Task::none());
            }
            "i" if cur == DRow::List => {
                if let Some(e) = self.episodes.items().get(self.focus.ep).cloned() {
                    self.outcomes.push(Outcome::Open(e));
                }
                return Some(Task::none());
            }
            "i" if self.full.as_ref().is_some_and(|f| !f.info.is_empty()) => {
                self.info_open = true;
                return Some(Task::none());
            }
            _ => {}
        }
        if cur == DRow::List
            && let Some(e) = model::Edit::of(name)
            && self.owned_playlist()
        {
            self.playlist_edit(e);
            return Some(Task::none());
        }
        None
    }

    fn activate(&mut self, r: DRow) -> Task<Msg> {
        let col = self.focus.col;
        match r {
            DRow::Actions => {
                if let Some(a) = self.dacts().get(col).copied() {
                    return self.do_act(a);
                }
            }
            DRow::Chips => self.do_chip(col),
            DRow::Seasons => {}
            DRow::List => self.list_open(self.focus.ep),
            DRow::Strip(s) => return self.do_sec(s, col),
        }
        Task::none()
    }

    /// Bring the focused row into view (heights are laid out to known sizes, see `geom`).
    fn reveal(&self) -> Task<Msg> {
        let shape = self.shape();
        let r = shape.current(&self.focus);
        let g = geom::Geom::of(self);
        let (top, bottom) = g.span(self, r);
        let (off, h) = (self.scroll_y, self.view_h.max(200.0));
        let y = if r == DRow::Actions {
            0.0
        } else if top < off + crate::theme::HEADER_H + 40.0 {
            (top - crate::theme::HEADER_H - 40.0 - crate::theme::S4).max(0.0)
        } else if bottom > off + h {
            bottom - h + crate::theme::S4
        } else {
            return Task::none();
        };
        let mut tasks = vec![operation::scroll_to(
            view::PAGE_ID,
            AbsoluteOffset { x: 0.0, y },
        )];
        if let DRow::Strip(s) = r {
            let (cw, _) = geom::dims(s);
            tasks.push(operation::scroll_to(
                view::strip_id(s),
                AbsoluteOffset {
                    x: (self.focus.col as f32 * (cw + 2.0 * crate::theme::S3)
                        - 2.0 * (cw + 2.0 * crate::theme::S3))
                        .max(0.0),
                    y: 0.0,
                },
            ));
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_core::local::LocalBackend;
    use nocturne_core::source::ProviderIds;
    use nocturne_jellyfin as jf;

    fn movie(origin: u32, id: &str) -> Media {
        let mut m = Media::from_item_in(
            SourceId(origin),
            &jf::Item {
                id: id.into(),
                name: "Film".into(),
                kind: "Movie".into(),
                ..Default::default()
            },
        );
        m.ids = ProviderIds {
            imdb: Some("tt1".into()),
            ..Default::default()
        };
        m
    }

    fn block(id: &str, quality: &str) -> InfoBlock {
        InfoBlock {
            id: id.into(),
            quality: quality.into(),
            ..Default::default()
        }
    }

    fn page(alts: Vec<Media>, backends: Vec<(SourceId, Backend)>, writes: bool) -> Detail {
        let (d, _) = Detail::new(
            movie(1, "a"),
            alts,
            backends,
            Size::new(1280.0, 800.0),
            writes,
            vec![],
        );
        d
    }

    fn outcomes(d: &mut Detail) -> Vec<String> {
        std::mem::take(&mut d.outcomes)
            .into_iter()
            .map(|o| match o {
                Outcome::Play {
                    media, source_id, ..
                } => {
                    format!("play {} {}", media.origin.0, source_id.unwrap_or_default())
                }
                Outcome::Prefer(m) => format!("prefer {}", m.origin.0),
                Outcome::Toast(t) => format!("toast {t}"),
                Outcome::Write(_) => "write".into(),
                _ => "other".into(),
            })
            .collect()
    }

    #[test]
    fn play_uses_the_default_version_then_the_chosen_copy() {
        let mut d = page(vec![movie(2, "b")], vec![], true);
        d.full = Some(Full {
            info: vec![block("s4k", "4K"), block("s1080", "1080p")],
            ..Default::default()
        });
        let list = d.versions();
        assert_eq!(list.len(), 3, "two files here plus the other source");
        assert!(
            d.dacts().contains(&DAct::Versions),
            "a choice shows the picker button"
        );
        let _ = d.do_act(DAct::Play);
        assert_eq!(
            outcomes(&mut d),
            ["play 1 s4k"],
            "server default = its first version"
        );
        d.choose_version(1);
        let _ = d.do_act(DAct::Play);
        assert_eq!(outcomes(&mut d), ["play 1 s1080"]);
        d.choose_version(2);
        let _ = d.do_act(DAct::Play);
        assert_eq!(outcomes(&mut d), ["play 2 "], "the other source's own copy");
    }

    #[test]
    fn a_single_copy_has_no_picker() {
        let d = page(vec![], vec![], true);
        assert!(!d.dacts().contains(&DAct::Versions));
    }

    #[test]
    fn preferring_asks_the_root_to_remember_the_chosen_source() {
        let mut d = page(vec![movie(2, "b")], vec![], true);
        d.choose_version(1);
        d.prefer_selected();
        assert_eq!(outcomes(&mut d)[0], "prefer 2");
        assert!(d.ver_open.is_none(), "the picker closes");
    }

    #[test]
    fn server_changes_wait_for_the_switch_but_local_state_does_not() {
        let t = tempfile::tempdir().unwrap();
        let lib = nocturne_local::Library::open(t.path()).unwrap();
        let local = Backend::Local(LocalBackend::new(SourceId(9), lib));
        let mut d = page(vec![], vec![], false);
        d.toggle_played(d.media.clone());
        assert_eq!(outcomes(&mut d), [format!("toast {NEED_WRITES}")]);
        assert!(!d.media.played);
        let mut d = page(vec![], vec![], true);
        d.toggle_played(d.media.clone());
        assert_eq!(outcomes(&mut d), ["write", "toast Marked as watched."]);
        assert!(d.media.played, "optimistic");
        let _ = d.wrote(Write::Played(d.media.clone(), true), Err(""));
        assert!(!d.media.played, "a failed write is undone");
        let mut loc = movie(9, "l");
        loc.ids = ProviderIds::default();
        let mut d = page(vec![], vec![(SourceId(9), local)], false);
        d.media = loc;
        d.toggle_fav(d.media.clone());
        assert_eq!(
            outcomes(&mut d)[0],
            "write",
            "the local library is not a server"
        );
    }

    #[test]
    fn playlist_edits_need_an_owned_playlist_and_writes() {
        let mut d = page(vec![], vec![], true);
        d.media.kind = Kind::Playlist;
        d.media.can_delete = false;
        d.episodes = Load::Ready(vec![movie(1, "x"), movie(1, "y")]);
        d.playlist_edit(model::Edit::Remove);
        assert!(d.outcomes.is_empty(), "not the user's playlist");
        d.media.can_delete = true;
        d.focus.ep = 1;
        d.playlist_edit(model::Edit::Down);
        assert_eq!(outcomes(&mut d), ["toast Already at the end."]);
        d.playlist_edit(model::Edit::Up);
        assert_eq!(outcomes(&mut d), ["write"]);
        d.writes = false;
        d.playlist_edit(model::Edit::Remove);
        assert_eq!(outcomes(&mut d), [format!("toast {NEED_WRITES}")]);
    }

    #[test]
    fn deleting_a_playlist_needs_its_name_typed() {
        let mut d = page(vec![], vec![], true);
        d.media.kind = Kind::Playlist;
        d.media.title = "Night".into();
        d.media.can_delete = true;
        let _ = d.more_act(MoreAct::DeletePlaylist);
        d.dialog.as_mut().unwrap().text = "night".into();
        d.dialog_submit();
        assert!(d.outcomes.is_empty(), "the name must match exactly");
        d.dialog.as_mut().unwrap().text = "Night".into();
        d.dialog_submit();
        assert_eq!(outcomes(&mut d), ["write"]);
        assert!(d.dialog.as_ref().unwrap().busy);
        let _ = d.wrote(
            Write::Playlist(PlaylistOp::Delete { id: "x".into() }),
            Ok(()),
        );
        assert!(d.dialog.is_none());
        assert!(matches!(d.outcomes.last(), Some(Outcome::Back)));
    }
}
