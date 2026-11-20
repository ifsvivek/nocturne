//! Home: hero band, Continue watching, Next up and one Latest row per library of every source.
//! Rows render as their sources answer; an offline source adds nothing but a quiet note. Port of
//! the GPUI `home` / `hero_band` / `strip` (`crates/app/src/app.rs`).
use super::cells::{badge, mark_for};
use super::images::Want;
use super::message::{At, CellAct, FanKind, Message};
use super::{App, Load, View};
use crate::style::{self, gradient};
use crate::task::run;
use crate::theme as t;
use crate::virt;
use crate::widget::cell::{Cell, Look, Pic, poster_cell, tile};
use crate::widget::layout::section_with;
use crate::widget::press::Press;
use crate::widget::{band, chip, mono, pill_button};
use iced::widget::Id;
use iced::widget::operation;
use iced::widget::scrollable::{self, AbsoluteOffset};
use iced::widget::{Stack, column, container, image, row, space, text};
use iced::{Alignment, Color, ContentFit, Element, Length, Padding, Task};
use nocturne_core::customize::{self as cz, Page, Parts, RowLayout};
use nocturne_core::merge::Entry;
use nocturne_core::nav::{self, Dir};
use nocturne_core::server_ui::{Section, ServerUi};
use nocturne_core::source::{Art, Kind, Media, SourceId};
use nocturne_core::sources::{Fanout, status_text};
use std::collections::HashMap;

type El<'a> = Element<'a, Message>;

pub const PAGE_ID: &str = "home-scroll";

/// Home rows, top to bottom. `Home::row` indexes [`Home::layout`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HRow {
    Nav,
    Hero,
    Resume,
    NextUp,
    Latest(usize),
    /// Server UI: the "My media" library tiles.
    Tiles,
}

impl HRow {
    /// Landscape cards (16:9) instead of posters.
    pub fn wide(self) -> bool {
        matches!(self, Self::Resume | Self::NextUp | Self::Tiles)
    }
}

/// The stylesheet section a row belongs to (`.section-resume` ...).
pub fn section_of(k: HRow) -> Option<cz::Section> {
    match k {
        HRow::Resume => Some(cz::Section::Resume),
        HRow::NextUp => Some(cz::Section::NextUp),
        HRow::Latest(_) => Some(cz::Section::Latest),
        _ => None,
    }
}

/// Order the strips by `order` (stable: ties keep the built-in order) and drop hidden ones; Nav
/// and Hero stay in front. `reorder` is off for a server's own layout, which has its own order.
fn arrange(
    v: &mut Vec<HRow>,
    order: [cz::Section; 3],
    hidden: impl Fn(HRow) -> bool,
    reorder: bool,
) {
    v.retain(|k| !hidden(*k));
    if reorder {
        let rank = |k: &HRow| match section_of(*k) {
            Some(sec) => 1 + order.iter().position(|o| *o == sec).unwrap_or(0),
            None => 0,
        };
        v.sort_by_key(rank);
    }
}

/// The hero band for these Home settings: its height and whether it is a banner below the header
/// (`None`: no hero). Billboard is the built-in look.
pub fn hero_cfg(p: &Parts, win_w: f32, win_h: f32) -> Option<(f32, bool)> {
    if p.hero.hidden == Some(true) {
        return None;
    }
    let banner = p.hero.style == Some(cz::HeroStyle::Banner);
    let h = p
        .hero
        .height
        .unwrap_or(if banner { 320.0 } else { hero_h(win_w, win_h) });
    Some((h, banner))
}

/// Vertical space the hero (and the header, when it pushes) takes above the first strip.
pub fn hero_extent(cfg: Option<(f32, bool)>) -> f32 {
    match cfg {
        None => t::header_h(),
        Some((h, banner)) if banner || t::header_pushes() => t::header_h() + h,
        Some((h, _)) => h,
    }
}

/// Resolved geometry of one kind of strip for a window.
#[derive(Clone, Debug)]
pub struct Geo {
    pub look: Look,
    pub dims: (f32, f32),
    pub art: Art,
    pub grid: bool,
    pub cols: usize,
    /// Horizontal inset of the strip (the page padding unless `.row padding` says otherwise).
    pub inset: f32,
    pub hidden: bool,
    pub title_t: t::Typo,
    /// The paragraph and count beside the row title (`.row-title --row-details`).
    pub details: bool,
    /// The strip sits in a hairline panel with a fade at its right edge (the built-in look).
    pub framed: bool,
    /// The previous / next buttons at the right of the title (`.row-arrows`).
    pub arrows: cz::Arrows,
}

impl Geo {
    pub fn cell_w(&self) -> f32 {
        self.dims.0 + 2.0 * self.look.pad + 1.0
    }

    pub(super) fn head_h(&self) -> f32 {
        if self.title_t.hidden {
            0.0
        } else {
            self.head_title_h() + t::S6
        }
    }

    /// Height of the heading block: the title and its paragraph, or the title alone.
    pub(super) fn head_title_h(&self) -> f32 {
        if self.details {
            SECTION_H
        } else {
            (self.title_t.size() * 4.0 / 3.0)
                .round()
                .max(self.arrow_size())
        }
    }

    pub(super) fn arrow_size(&self) -> f32 {
        if self.arrows.shown() && !self.grid {
            self.arrows.size.unwrap_or(22.0) + 8.0
        } else {
            0.0
        }
    }

    /// Rows of cards for `n` items (always one for a carousel).
    pub fn rows(&self, n: usize) -> usize {
        if self.grid {
            n.div_ceil(self.cols).max(1)
        } else {
            1
        }
    }

    /// Height of one band holding `n` items.
    pub fn band(&self, n: usize) -> f32 {
        2.0 * band_pad() + self.head_h() + self.rows(n) as f32 * self.look.cell_h(self.dims.1) + 2.0
    }
}

pub(super) fn band_pad() -> f32 {
    t::S8 * 1.5 * t::density()
}

/// Geometry of a strip kind from its resolved settings (`card` is the card-size setting in px).
pub fn geo_for(win_w: f32, card: f32, k: HRow, p: &Parts) -> Geo {
    let look = if k == HRow::Tiles {
        Look::tiles(p)
    } else {
        Look::of(p)
    };
    let kk = card / 176.0;
    let (def_w, def_r, def_art) = if k.wide() {
        (300.0 * kk, 9.0 / 16.0, Art::Landscape)
    } else {
        (190.0 * kk, 1.5, Art::Poster)
    };
    let (target, ratio, art) = if look.aspect.is_some() {
        let r = look.ratio(def_r);
        (
            look.width
                .unwrap_or(if r < 1.0 { 300.0 * kk } else { 190.0 * kk }),
            r,
            look.art(def_art),
        )
    } else {
        (look.width.unwrap_or(def_w), def_r, def_art)
    };
    let inset = p.row.padding.unwrap_or(t::page_pad());
    let grid = p.row.layout == Some(RowLayout::Grid);
    let avail = win_w - 2.0 * inset - 1.0;
    let cols = match (grid, p.row.columns) {
        (true, Some(n)) => n as usize,
        _ => nav::columns(avail, target + 2.0 * look.pad, 0.0),
    };
    let w = ((avail / cols as f32).floor() - 2.0 * look.pad).max(40.0);
    Geo {
        dims: (w, (w * ratio).round()),
        art,
        grid,
        cols,
        inset,
        hidden: p.row.hidden == Some(true) || p.card.hidden == Some(true),
        title_t: t::typo(&p.row_title, 28.0, iced::font::Weight::Bold),
        details: p.row.details != Some(false),
        framed: p.row.framed != Some(false),
        arrows: p.row_arrows.clone(),
        look,
    }
}

/// The geometry of each kind of strip: Continue watching, Next up, Latest (every library) and the
/// server's library tiles.
pub struct Geos([Geo; 4]);

impl Geos {
    pub fn of(&self, k: HRow) -> &Geo {
        &self.0[match k {
            HRow::Resume => 0,
            HRow::NextUp => 1,
            HRow::Tiles => 3,
            _ => 2,
        }]
    }
}

pub struct LatestRow {
    pub origin: SourceId,
    pub lib: Media,
    pub items: Load<Vec<Media>>,
}

impl LatestRow {
    /// Rows that are still loading or have something; empty and failed ones leave no hole.
    fn shown(&self) -> bool {
        match &self.items {
            Load::Loading => true,
            Load::Ready(v) => !v.is_empty(),
            Load::Failed => false,
        }
    }
}

pub struct Home {
    pub resume: Fanout,
    pub nextup: Fanout,
    pub resume_e: Vec<Entry>,
    pub nextup_e: Vec<Entry>,
    pub latest: Vec<LatestRow>,
    /// The Collections and Playlists views of the servers: no nav link or Latest row, but My Media tiles.
    pub views: Vec<Media>,
    /// The ids of the libraries in the order the servers listed them (views included): My Media's order without a saved one.
    pub lib_order: Vec<String>,
    /// Epoch of the libraries request the `latest` rows belong to.
    pub latest_epoch: u64,
    /// Settings: continue watching, next up, latest.
    pub show: (bool, bool, bool),
    pub row: usize,
    pub col: usize,
    /// Page scroll offset and viewport height.
    pub off: f32,
    pub vh: f32,
    /// Horizontal offset of each strip (by layout row).
    pub strips: HashMap<usize, f32>,
    /// Server UI mode, one server selected: that server's own layout. `None` is the NOCTURNE
    /// layout (also while the server's answer is still on its way).
    pub server: Option<ServerUi>,
    /// The hero carousel (engine Home): its slides (empty until the engine computes them, so the hand-written page keeps its single hero), the timer, whether the user has moved the focus yet (an untouched Home keeps rotating under the initial focus), the descriptions the user opened and the slide that is fading out.
    pub slides: Vec<Media>,
    pub carousel: nocturne_core::hero::Hero,
    pub touched: bool,
    pub expanded: nocturne_core::expand::Expanded,
    pub fade: Option<(Media, std::time::Instant)>,
}

impl Home {
    pub fn new(show: (bool, bool, bool), resume: Fanout, nextup: Fanout) -> Self {
        Self {
            resume,
            nextup,
            resume_e: vec![],
            nextup_e: vec![],
            latest: vec![],
            views: vec![],
            lib_order: vec![],
            latest_epoch: 0,
            show,
            row: 1,
            col: 0,
            off: 0.0,
            vh: 0.0,
            strips: HashMap::new(),
            server: None,
            slides: vec![],
            carousel: nocturne_core::hero::Hero::new(0, 10, true),
            touched: false,
            expanded: Default::default(),
            fade: None,
        }
    }

    /// The server's rows in the server's order, reusing the NOCTURNE row kinds.
    fn server_layout(&self, ui: &ServerUi, v: &mut Vec<HRow>) {
        for s in ui.shown() {
            match s {
                Section::MyMedia | Section::LibraryButtons => {
                    if !v.contains(&HRow::Tiles) && !self.tiles().is_empty() {
                        v.push(HRow::Tiles);
                    }
                }
                Section::Resume if !self.resume_e.is_empty() => v.push(HRow::Resume),
                Section::NextUp if !self.nextup_e.is_empty() => v.push(HRow::NextUp),
                Section::Latest => {
                    let rows: Vec<(&str, usize)> = self
                        .latest
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r.shown())
                        .map(|(i, r)| (r.lib.id.as_str(), i))
                        .collect();
                    v.extend(
                        ui.latest_libs(rows, |r| r.0)
                            .into_iter()
                            .map(|r| HRow::Latest(r.1)),
                    );
                }
                _ => {}
            }
        }
    }

    /// Server UI: the libraries as tiles, in the user's order without the hidden ones.
    fn tiles(&self) -> Vec<&Media> {
        match &self.server {
            Some(ui) => {
                let mut libs: Vec<&Media> = self
                    .latest
                    .iter()
                    .map(|r| &r.lib)
                    .chain(&self.views)
                    .collect();
                libs.sort_by_key(|m| {
                    self.lib_order
                        .iter()
                        .position(|i| *i == m.id)
                        .unwrap_or(usize::MAX)
                });
                ui.tiles(libs, |m| m.id.as_str())
            }
            None => vec![],
        }
    }

    /// The rows as the theme shows them: hero removed, strips ordered and hidden ones dropped.
    pub fn layout(&self) -> Vec<HRow> {
        let hidden = |k: HRow| {
            let p = t::parts(Some(Page::Home), section_of(k));
            p.row.hidden == Some(true) || p.card.hidden == Some(true)
        };
        let mut v = self.layout_plain();
        if t::parts(Some(Page::Home), None).hero.hidden == Some(true) {
            v.retain(|k| *k != HRow::Hero);
        }
        arrange(&mut v, t::section_order(), hidden, self.server.is_none());
        v
    }

    fn layout_plain(&self) -> Vec<HRow> {
        let mut v = vec![HRow::Nav, HRow::Hero];
        if let Some(ui) = &self.server {
            self.server_layout(ui, &mut v);
            return v;
        }
        if self.show.0 && !self.resume_e.is_empty() {
            v.push(HRow::Resume);
        }
        if self.show.1 && !self.nextup_e.is_empty() {
            v.push(HRow::NextUp);
        }
        if self.show.2 {
            v.extend(
                self.latest
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.shown())
                    .map(|(i, _)| HRow::Latest(i)),
            );
        }
        v
    }

    /// Items of a strip with how many copies (sources) each has.
    pub fn items(&self, k: HRow) -> Vec<(&Media, usize)> {
        match k {
            HRow::Resume => entries(&self.resume_e),
            HRow::NextUp => entries(&self.nextup_e),
            HRow::Tiles => self.tiles().into_iter().map(|m| (m, 1)).collect(),
            HRow::Latest(i) => self
                .latest
                .get(i)
                .map(|r| r.items.items().iter().map(|m| (m, 1)).collect())
                .unwrap_or_default(),
            _ => vec![],
        }
    }

    /// The hero: the focused poster, else the carousel's slide, else the newest item of the first row that has one.
    pub fn hero(&self) -> Option<&Media> {
        let lay = self.layout();
        let focused = match lay.get(self.row) {
            Some(k @ (HRow::Resume | HRow::NextUp | HRow::Latest(_))) => {
                self.items(*k).get(self.col).map(|(m, _)| *m)
            }
            _ => None,
        };
        focused
            .or_else(|| self.slides.get(self.carousel.index()))
            .or_else(|| self.newest())
    }

    /// The hero's eyebrow: "01 / 05 \u{b7} Episode" while the carousel shows this slide, else just the kind.
    pub fn hero_tag(&self, m: &Media) -> String {
        let kind = m.kind_label();
        let mut c = kind.chars();
        let kind: String = c
            .next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        match self.slides.get(self.carousel.index()) {
            Some(s) if self.slides.len() > 1 && s.id == m.id => format!(
                "{:02} / {:02} \u{b7} {kind}",
                self.carousel.index() + 1,
                self.slides.len()
            ),
            _ => kind,
        }
    }

    /// The first item of the first Latest row: today's hero when nothing is focused.
    fn newest(&self) -> Option<&Media> {
        self.latest.iter().find_map(|r| r.items.items().first())
    }

    /// The carousel's slides: continue watching, next up, the newest movies and shows, in `order`, deduplicated by show and capped (`hero::collect`). Only items with a backdrop count. With `today_first` (no theme: the default look) the item the hero shows today comes first, so the first slide stays what it was.
    pub fn compute_slides(
        &self,
        order: &[nocturne_core::hero::HeroSource],
        today_first: bool,
    ) -> Vec<Media> {
        use nocturne_core::hero::{self as hero, HeroSource as S};
        let art = |m: &&Media| m.backdrop_tag.is_some() || m.backdrop_id.is_some();
        let own = |e: &[Entry]| -> Vec<Media> {
            e.iter().map(|e| &e.primary).filter(art).cloned().collect()
        };
        let latest = |coll: &str| -> Vec<Media> {
            self.latest
                .iter()
                .filter(|r| r.lib.collection.as_deref() == Some(coll))
                .flat_map(|r| {
                    r.items
                        .items()
                        .iter()
                        .filter(art)
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let lists = vec![
            (S::ContinueWatching, own(&self.resume_e)),
            (S::NextUp, own(&self.nextup_e)),
            (S::LatestMovies, latest("movies")),
            (S::LatestShows, latest("tvshows")),
        ];
        let mut v = hero::collect(order, &lists, hero::PER_SOURCE, hero::TOTAL_CAP);
        if today_first && let Some(m) = self.newest().filter(art) {
            let key = (m.origin, m.id.clone());
            v.retain(|x| (x.origin, x.id.clone()) != key);
            v.insert(0, m.clone());
            v.truncate(hero::TOTAL_CAP);
        }
        v
    }

    pub fn loading(&self) -> bool {
        self.latest.is_empty() || self.latest.iter().any(|r| matches!(r.items, Load::Loading))
    }
}

fn entries(v: &[Entry]) -> Vec<(&Media, usize)> {
    v.iter()
        .map(|e| (&e.primary, 1 + e.alternates.len()))
        .collect()
}

/// Card sizes of the strips for a window of `win_w` px: whole cards fill the row exactly.
pub fn row_dims(win_w: f32, target: f32, ratio: f32) -> (f32, f32) {
    let avail = win_w - 2.0 * t::PAGE_PAD - 1.0;
    let cols = nav::columns(avail, target + 2.0 * t::S3, 0.0);
    let w = (avail / cols as f32).floor() - 2.0 * t::S3;
    (w, (w * ratio).round())
}

/// Hero band height for a window.
pub fn hero_h(win_w: f32, win_h: f32) -> f32 {
    (win_w * 9.0 / 16.0).min(win_h * 0.62).max(500.0).round()
}

pub(super) const SECTION_H: f32 = 72.0;

/// Height of one built-in strip band for cards whose image is `ch` tall.
pub fn band_h(ch: f32) -> f32 {
    geo_for(1280.0, 176.0, HRow::Latest(0), &Parts::default()).band_for_height(ch)
}

impl Geo {
    fn band_for_height(&self, ch: f32) -> f32 {
        2.0 * band_pad() + self.head_h() + self.look.cell_h(ch) + 2.0
    }
}

impl App {
    pub(super) fn geos(&self) -> Geos {
        let card = self.settings.density.card();
        let one = |k| {
            geo_for(
                self.win.width,
                card,
                k,
                &t::parts(Some(Page::Home), section_of(k)),
            )
        };
        Geos([
            one(HRow::Resume),
            one(HRow::NextUp),
            one(HRow::Latest(0)),
            one(HRow::Tiles),
        ])
    }

    /// Where the first strip starts: below the hero (and the header).
    fn extent(&self) -> f32 {
        let p = t::parts(Some(Page::Home), None);
        hero_extent(hero_cfg(&p, self.win.width, self.win.height))
    }

    /// Fetch Home: libraries (header links, Latest rows), continue watching and next up, each from
    /// every online source at once.
    pub(super) fn load_home(&mut self, pid: u64) -> Task<Message> {
        let (fan, t1) = self.fan(
            pid,
            FanKind::Libs,
            |b| async move { b.libraries_all().await },
        );
        let (rf, t2) = self.fan(pid, FanKind::Resume, |b| async move { b.resume().await });
        let (nf, t3) = self.fan(pid, FanKind::NextUp, |b| async move { b.next_up().await });
        let show = self.settings.home();
        let server = self.server_ui_now();
        if let Some(p) = self.page_mut(pid) {
            p.libs = if fan.is_empty() {
                Load::Ready(vec![])
            } else {
                Load::Loading
            };
            p.fan = fan;
            let mut home = Home::new(show, rf, nf);
            home.server = server;
            p.home = Some(Box::new(home));
        }
        Task::batch([t1, t2, t3])
    }

    /// An answer of one of Home's fan-outs.
    pub(super) fn fan_answer(
        &mut self,
        pid: u64,
        kind: FanKind,
        epoch: u64,
        origin: SourceId,
        r: Option<Vec<Media>>,
    ) -> Task<Message> {
        let prio = self.catalog.priority();
        let mut tasks = vec![];
        let Some(p) = self.stack.iter_mut().find(|p| p.pid == pid) else {
            return Task::none();
        };
        match kind {
            FanKind::Libs => {
                let listed: Vec<String> = r.iter().flatten().map(|m| m.id.clone()).collect();
                let (views, r) = match r {
                    Some(v) => {
                        let (a, b) = v.into_iter().partition(nocturne_core::source::is_view);
                        (a, Some(b))
                    }
                    None => (vec![], None),
                };
                if !p.fan.accept(epoch, origin, r.clone()) {
                    return Task::none(); // an older request, or a page that is gone
                }
                p.libs = if p.fan.contributors() > 0
                    || p.fan.pending() == 0 && p.fan.failed().is_empty()
                {
                    Load::Ready(p.fan.flat(&prio))
                } else if p.fan.pending() == 0 {
                    Load::Failed
                } else {
                    Load::Loading
                };
                crate::timing::shell_ready();
                if let (Some(libs), Some(h)) = (r, p.home.as_mut()) {
                    h.latest_epoch = epoch;
                    for id in listed {
                        if !h.lib_order.contains(&id) {
                            h.lib_order.push(id);
                        }
                    }
                    h.views.retain(|m| m.origin != origin);
                    h.views.extend(views);
                    for lib in libs {
                        if let Some(b) = self.catalog.backend(origin).cloned() {
                            let (l2, id) = (lib.clone(), lib.id.clone());
                            tasks.push(run(async move { b.latest(&l2).await.ok() }, move |r| {
                                Message::Latest(pid, epoch, origin, id, r)
                            }));
                        }
                        h.latest.push(LatestRow {
                            origin,
                            lib,
                            items: Load::Loading,
                        });
                    }
                    // Source order, not arrival order.
                    h.latest.sort_by_key(|r| {
                        prio.iter()
                            .position(|o| *o == r.origin)
                            .unwrap_or(prio.len())
                    });
                }
            }
            FanKind::Resume | FanKind::NextUp => {
                let Some(h) = p.home.as_mut() else {
                    return Task::none();
                };
                let fan = if kind == FanKind::Resume {
                    &mut h.resume
                } else {
                    &mut h.nextup
                };
                if !fan.accept(epoch, origin, r) {
                    return Task::none();
                }
                let all: Vec<Media> = fan
                    .answers()
                    .iter()
                    .flat_map(|(_, v)| v.iter().cloned())
                    .collect();
                tasks.push(self.learn_series(&all));
            }
        }
        self.rebuild_home();
        tasks.push(self.home_wants());
        Task::batch(tasks)
    }

    pub(super) fn latest_answer(
        &mut self,
        pid: u64,
        epoch: u64,
        origin: SourceId,
        lib: String,
        r: Option<Vec<Media>>,
    ) -> Task<Message> {
        let Some(h) = self.page_mut(pid).and_then(|p| p.home.as_deref_mut()) else {
            return Task::none();
        };
        if h.latest_epoch != epoch {
            return Task::none();
        }
        if let Some(row) = h
            .latest
            .iter_mut()
            .find(|x| x.origin == origin && x.lib.id == lib)
        {
            row.items = r.map_or(Load::Failed, Load::Ready);
        }
        self.home_wants()
    }

    /// Ask each source for the provider ids of the series behind `media`'s episodes, once.
    pub(super) fn learn_series(&mut self, media: &[Media]) -> Task<Message> {
        let mut tasks = vec![];
        for (origin, ids) in self.series_ids.missing(media) {
            let Some(b) = self.catalog.backend(origin).cloned() else {
                continue;
            };
            // Unknown until answered: an empty record keeps the same ids from being asked twice.
            for id in &ids {
                self.series_ids.set(origin, id.clone(), Default::default());
            }
            tasks.push(run(
                async move {
                    let found = b.provider_ids(&ids).await.unwrap_or_default();
                    found.into_iter().collect::<Vec<_>>()
                },
                move |v| Message::SeriesIds(origin, v),
            ));
        }
        Task::batch(tasks)
    }

    pub(super) fn series_arrived(
        &mut self,
        origin: SourceId,
        v: Vec<(String, nocturne_core::source::ProviderIds)>,
    ) -> Task<Message> {
        for (id, ids) in v {
            self.series_ids.set(origin, id, ids);
        }
        self.rebuild_home();
        Task::none()
    }

    /// Recompute the merged Continue watching / Next up lists (answers, priorities, preferred
    /// copies or learned series ids changed).
    pub(super) fn rebuild_home(&mut self) {
        let prio = self.catalog.priority();
        let (prefs, series) = (&self.prefs, &self.series_ids);
        if let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) {
            h.resume_e = h.resume.interleaved(&prio, prefs, series);
            h.nextup_e = h.nextup.interleaved(&prio, prefs, series);
        }
    }

    /// Start the images the Home page needs now.
    pub(super) fn home_wants(&mut self) -> Task<Message> {
        let w = self.home_want_list();
        self.want(&w)
    }

    /// The hero, and the first cards of strips near the viewport (more as a strip is scrolled).
    fn home_want_list(&self) -> Vec<Want> {
        let Some(p) = self.stack.first().filter(|p| p.view == View::Home) else {
            return vec![];
        };
        let Some(h) = p.home.as_deref() else {
            return vec![];
        };
        // Only while Home is the page in view (an open page above it asks for its own).
        if self.stack.len() > 1 {
            return vec![];
        }
        let mut w: Vec<Want> = vec![];
        let lay = h.layout();
        if lay.contains(&HRow::Hero)
            && let Some(m) = h.hero()
        {
            w.extend(self.hero_wants(m));
        }
        let gs = self.geos();
        let vh = if h.vh > 0.0 { h.vh } else { self.win.height };
        let mut top = self.extent();
        for (ri, k) in lay.iter().enumerate().filter(|(_, k)| strip(**k)) {
            let g = gs.of(*k);
            let items = h.items(*k);
            let bh = g.band(items.len());
            let near = top + bh > h.off - vh * 0.5 && top < h.off + vh * 1.5;
            top += bh;
            if !near {
                continue;
            }
            let avail = self.win.width - 2.0 * g.inset;
            let x = h.strips.get(&ri).copied().unwrap_or(0.0);
            let first = (x / g.cell_w()).floor() as usize;
            let n = if g.grid {
                usize::MAX
            } else {
                (avail / g.cell_w()).ceil() as usize + 2
            };
            for (m, _) in items.into_iter().skip(first).take(n) {
                w.push((m.clone(), g.art, g.dims.0 as u32));
            }
        }
        w
    }

    pub(super) fn hero_wants(&self, m: &Media) -> Vec<Want> {
        let art = if m.backdrop_tag.is_none() && m.kind == Kind::Episode {
            Art::Landscape
        } else {
            Art::Backdrop
        };
        let mut v = vec![(m.clone(), art, self.win.width as u32)];
        if m.logo.is_some() {
            v.push((m.clone(), Art::Logo, 460));
        }
        v
    }

    // ---- view ----

    pub(super) fn home_page(&self) -> El<'_> {
        #[cfg(feature = "theme-engine")]
        if let Some(v) = self.eng_home_view() {
            return v;
        }
        let Some(p) = self.stack.last() else {
            return space().into();
        };
        if matches!(p.libs, Load::Failed) {
            return self.failed_view("Your library couldn't be loaded.");
        }
        let Some(h) = p.home.as_deref() else {
            return space().into();
        };
        let lay = h.layout();
        let gs = self.geos();
        let hero_focus = (lay.get(h.row) == Some(&HRow::Hero)).then_some(h.col);
        let hp = t::parts(Some(Page::Home), None);
        let hero: El = match hero_cfg(&hp, self.win.width, self.win.height) {
            Some((hh, banner)) => {
                let inner: El = match h.hero() {
                    Some(m) => self.hero_band(m, h.hero_tag(m), hh, hero_focus),
                    None => container(note(if h.loading() {
                        "Loading your library"
                    } else {
                        "Nothing to show yet."
                    }))
                    .height(420.min(hh as u32) as f32)
                    .padding(Padding {
                        top: t::header_h(),
                        ..Padding::ZERO
                    })
                    .into(),
                };
                if banner || t::header_pushes() {
                    column![space().height(t::header_h()), inner].into()
                } else {
                    inner
                }
            }
            None => space().height(t::header_h()).into(),
        };
        let mut page = column![band(hero)];
        if !lay.iter().any(|k| strip(*k)) && !lay.contains(&HRow::Hero) && h.loading() {
            page = page.push(note("Loading your library"));
        }
        let several = self.several_sources();
        for (ri, k) in lay.iter().enumerate().filter(|(_, k)| strip(**k)) {
            let focus = (h.row == ri).then_some(h.col);
            let g = gs.of(*k);
            let items = h.items(*k);
            let n = items.len();
            let (title, text_, meta, _dims, _art, play, loading) = match k {
                HRow::Resume => (
                    "Continue watching".to_string(),
                    "Pick up where you left off.".to_string(),
                    format!("{n} in progress"),
                    g.dims,
                    g.art,
                    true,
                    false,
                ),
                HRow::NextUp => (
                    "Next up".to_string(),
                    "The next episode of what you're following.".to_string(),
                    format!("{n} up next"),
                    g.dims,
                    g.art,
                    true,
                    false,
                ),
                HRow::Tiles => (
                    "My media".to_string(),
                    "Your libraries.".to_string(),
                    format!("{n} {}", if n == 1 { "library" } else { "libraries" }),
                    g.dims,
                    g.art,
                    false,
                    false,
                ),
                HRow::Latest(i) => {
                    let r = &h.latest[*i];
                    let mut title = format!("Latest in {}", r.lib.title);
                    if several && let Some(s) = self.catalog.slot(r.origin) {
                        title = format!("{title}  \u{b7}  {}", crate::widget::source::mark_text(s));
                    }
                    (
                        title,
                        "Recently added, newest first.".to_string(),
                        match &r.items {
                            Load::Ready(v) => {
                                format!("{} \u{b7} {} titles", r.lib.kind_label(), v.len())
                            }
                            _ => r.lib.kind_label(),
                        },
                        g.dims,
                        g.art,
                        false,
                        matches!(r.items, Load::Loading),
                    )
                }
                _ => continue,
            };
            let strip = self.strip(ri, &items, g, focus, play, loading);
            page = page.push(strip_band(&title, &text_, meta, strip, g, ri));
        }
        // Sources that did not answer add nothing but a quiet line.
        let down: Vec<String> = self
            .catalog
            .slots
            .iter()
            .filter(|s| s.profile.enabled && !(s.backend.is_some() && s.status.usable()))
            .map(|s| {
                format!(
                    "{}: {}",
                    s.profile.label,
                    status_text(&s.status, self.now).to_lowercase()
                )
            })
            .collect();
        if !down.is_empty() {
            page = page.push(
                container(mono(format!("Not showing {}", down.join("  \u{b7}  "))))
                    .padding([t::S4, t::page_pad()]),
            );
        }
        let page = page.push(space().height(t::S8 * 2.0));
        scrollable::Scrollable::new(page.width(Length::Fill))
            .id(Id::from(PAGE_ID))
            .on_scroll(|v| Message::HomeScroll(v.absolute_offset().y, v.bounds().height))
            .style(style::scroll)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Several sources are in play: cells say where they come from.
    pub(super) fn several_sources(&self) -> bool {
        self.catalog.online().count() > 1
    }

    #[allow(clippy::too_many_arguments)]
    fn strip(
        &self,
        ri: usize,
        items: &[(&Media, usize)],
        g: &Geo,
        focus: Option<usize>,
        play: bool,
        loading: bool,
    ) -> El<'_> {
        let writes = self.hooks.can_write();
        let several = self.several_sources();
        let ((cw, ch), art) = (g.dims, g.art);
        let mut cells: Vec<El> = vec![];
        if items.is_empty() {
            for i in 0..if g.grid { g.cols.min(8) } else { 8 } {
                cells.push(poster_cell(Cell {
                    pic: if loading { Pic::Loading } else { Pic::Missing },
                    title: String::new(),
                    meta: String::new(),
                    index: i,
                    w: cw,
                    h: ch,
                    focused: false,
                    progress: None,
                    badge: None,
                    subtitle: false,
                    mark: None,
                    chips: None,
                    on_open: Message::Noop,
                    on_hover: Message::Noop,
                    on_fav: Message::Noop,
                    on_played: Message::Noop,
                    look: g.look.clone(),
                }));
            }
        }
        for (ci, (m, copies)) in items.iter().enumerate() {
            let at = At::Home(ri, ci);
            let episode = art == Art::Landscape && m.kind == Kind::Episode;
            let (title, meta) = if episode {
                (
                    m.series_name.clone().unwrap_or_else(|| m.title.clone()),
                    super::cells::episode_line(m, g.look.episode_format),
                )
            } else {
                (m.title.clone(), super::cells::caption(m))
            };
            cells.push(poster_cell(Cell {
                pic: self.pic(m, art, cw as u32),
                title,
                meta,
                index: ci,
                w: cw,
                h: ch,
                focused: focus == Some(ci),
                progress: m.progress(),
                badge: badge(m),
                subtitle: episode,
                mark: several
                    .then(|| mark_for(&self.catalog, m, *copies))
                    .flatten(),
                chips: Some((m.favorite, m.played, writes)),
                on_open: Message::Cell(at, if play { CellAct::Play } else { CellAct::Open }),
                on_hover: Message::Cell(at, CellAct::Hover),
                on_fav: Message::Cell(at, CellAct::Fav),
                on_played: Message::Cell(at, CellAct::Played),
                look: g.look.clone(),
            }));
        }
        if g.grid {
            let mut rows: Vec<El> = vec![];
            while !cells.is_empty() {
                let rest = cells.split_off(g.cols.min(cells.len()));
                rows.push(row(std::mem::replace(&mut cells, rest)).into());
            }
            return column(rows).width(Length::Fill).into();
        }
        let strip = scrollable::Scrollable::with_direction(
            row(cells),
            scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(4).scroller_width(4),
            ),
        )
        .id(strip_id(ri))
        .on_scroll(move |v| Message::HomeStrip(ri, v.absolute_offset().x))
        .style(style::scroll_quiet)
        .width(Length::Fill);
        strip.into()
    }

    /// Full-bleed hero: backdrop fills the width, scrims fade it into the page, text bottom-left.
    fn hero_band<'a>(&'a self, m: &'a Media, tag: String, h: f32, focus: Option<usize>) -> El<'a> {
        let w = self.win.width;
        let art = if m.backdrop_tag.is_none() && m.kind == Kind::Episode {
            Art::Landscape
        } else {
            Art::Backdrop
        };
        // No initial on a missing backdrop: the title is already the biggest thing on the band.
        let backdrop = tile(&self.pic(m, art, w as u32), "", Length::Fill, Length::Fill);
        let mut chips = row![chip(&m.kind_label(), true)]
            .spacing(t::S2)
            .align_y(Alignment::Center);
        if let Some(y) = m.year {
            chips = chips.push(chip(&y.to_string(), false));
        }
        if let Some(r) = m.runtime_min {
            chips = chips.push(chip(&nocturne_core::source::fmt_runtime(r), false));
        }
        for g in m.genres.iter().take(3) {
            chips = chips.push(chip(g, false));
        }
        let hp = t::parts(Some(Page::Home), None);
        let size = (w / 26.0).clamp(40.0, 72.0);
        let title_ty = t::typo(&hp.hero_title, size, iced::font::Weight::Bold).with_line(1.08);
        let sub_ty = t::typo(&hp.hero_meta, t::BODY_LG, iced::font::Weight::Normal).with_line(1.6);
        let logo = if m.logo.is_some() {
            Some(self.pic(m, Art::Logo, 460))
        } else {
            None
        };
        let title_el: El = match logo {
            Some(Pic::Ready(hd)) => {
                container(image(hd).height(110).content_fit(ContentFit::Contain))
                    .max_width(460)
                    .height(110)
                    .into()
            }
            Some(_) => space().height(110).into(),
            None if title_ty.hidden => space().into(),
            None => title_ty.text(&m.title, t::text()).into(),
        };
        let season_line = (m.logo.is_some() && matches!(m.kind, Kind::Season | Kind::Episode))
            .then(|| text(m.title.clone()).size(t::HEADING).font(t::semibold()));
        let sub = m.overview.clone().unwrap_or_else(|| m.meta_line());
        let verb = if m.kind == Kind::Photo {
            "View"
        } else {
            "Play"
        };
        let mut buttons = row![].spacing(t::S3);
        if !matches!(
            m.kind,
            Kind::Book | Kind::Folder | Kind::Person | Kind::Other
        ) {
            buttons = buttons.push(pill_button(
                verb,
                true,
                focus == Some(0),
                Message::Cell(At::Hero, CellAct::Play),
            ));
        }
        buttons = buttons.push(pill_button(
            "More info",
            false,
            focus == Some(1),
            Message::Cell(At::Hero, CellAct::Open),
        ));
        let mut col = column![mono(tag), chips, title_el]
            .spacing(t::S4)
            .align_x(Alignment::Start);
        if let Some(s) = season_line {
            col = col.push(s);
        }
        col = col
            .push(
                container(if sub_ty.hidden {
                    text("")
                } else {
                    sub_ty.text(&sub, t::text_secondary())
                })
                .max_width(560)
                .max_height(sub_ty.size() * 1.6 * 3.0)
                .clip(true),
            )
            .push(buttons);
        let clear = Color::from_rgba8(0x07, 0x08, 0x0c, 0.0);
        let solid = Color::from_rgb8(0x07, 0x08, 0x0c);
        let scrim = |bg: iced::Background| -> El<'_> {
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(bg),
                    ..Default::default()
                })
                .into()
        };
        Stack::with_children(vec![
            container(space())
                .style(style::fill(t::surface()))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            backdrop,
            scrim(gradient(
                90.0,
                &[
                    (0.0, Color::from_rgba8(0x07, 0x08, 0x0c, 0.94)),
                    (0.65, clear),
                ],
            )),
            scrim(match &hp.hero.overlay {
                Some(f) => t::background(f),
                None => gradient(180.0, &[(0.45, clear), (1.0, solid)]),
            }),
            container(container(col).max_width(680))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(Alignment::End)
                .padding(Padding {
                    left: t::page_pad(),
                    bottom: t::S8,
                    ..Padding::ZERO
                })
                .into(),
        ])
        .width(Length::Fill)
        .height(h)
        .into()
    }

    // ---- keys ----

    /// Arrow keys, Enter on Home. Returns the task for scrolling and image requests.
    pub(super) fn home_key(&mut self, key: &str) -> Task<Message> {
        #[cfg(feature = "theme-engine")]
        if let Some(t) = self.eng_key(key) {
            return t;
        }
        let links = self.nav_links();
        let nav_w = self.nav_widths();
        let (nav_off, nav_vw, _) = self.nav_geom();
        let gs = self.geos();
        let ext = self.extent();
        let (self_w, win_h) = (self.win.width, self.win.height);
        let Some(h) = self.stack.last_mut().and_then(|p| p.home.as_deref_mut()) else {
            return Task::none();
        };
        let lay = h.layout();
        let kind = lay.get(h.row).copied().unwrap_or(HRow::Hero);
        let len = |h: &Home, k: HRow| match k {
            HRow::Nav => links.len(),
            HRow::Hero => 2,
            k => h.items(k).len(),
        };
        let dir = match key {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        let mut tasks = vec![];
        if let Some(d) = dir {
            let n = len(h, kind);
            match d {
                Dir::Left | Dir::Right => h.col = nav::step(h.col, n.max(1), n, d),
                Dir::Up => h.row = h.row.saturating_sub(1),
                Dir::Down => h.row = (h.row + 1).min(lay.len() - 1),
            }
            let k = lay[h.row];
            h.col = h.col.min(len(h, k).saturating_sub(1));
            // Page: bring the focused band into view.
            let mut top = ext;
            let mut want_y = 0.0;
            for (ri, k) in lay.iter().enumerate().filter(|(_, k)| strip(**k)) {
                if ri == h.row {
                    want_y = top;
                }
                top += gs.of(*k).band(h.items(*k).len());
            }
            if strip(k) {
                let vh = if h.vh > 0.0 { h.vh } else { win_h };
                let bh = gs.of(k).band(h.items(k).len());
                let target = if want_y < h.off {
                    want_y
                } else if want_y + bh > h.off + vh {
                    want_y + bh - vh
                } else {
                    h.off
                };
                tasks.push(operation::scroll_to(
                    Id::from(PAGE_ID),
                    AbsoluteOffset {
                        x: 0.0,
                        y: target.max(0.0),
                    },
                ));
            } else {
                tasks.push(operation::scroll_to(
                    Id::from(PAGE_ID),
                    AbsoluteOffset { x: 0.0, y: 0.0 },
                ));
            }
            // Strip: bring the focused card into view (a grid has no sideways scroll).
            if strip(k) && !gs.of(k).grid {
                let g = gs.of(k);
                let cur = h.strips.get(&h.row).copied().unwrap_or(0.0);
                let x = virt::into_view(cur, self_w - 2.0 * g.inset, g.cell_w(), h.col);
                if (x - cur).abs() > 0.5 {
                    h.strips.insert(h.row, x);
                    tasks.push(operation::scroll_to(
                        strip_id(h.row),
                        AbsoluteOffset { x, y: 0.0 },
                    ));
                }
            }
            if matches!(k, HRow::Nav)
                && let Some(x) = super::header::nav_target(&nav_w, h.col, nav_off, nav_vw)
            {
                tasks.push(operation::scroll_to(
                    Id::from(super::header::NAV_ID),
                    AbsoluteOffset { x, y: 0.0 },
                ));
            }
            tasks.push(self.home_wants());
            return Task::batch(tasks);
        }
        if key == "enter" {
            let (row, col) = (h.row, h.col);
            let act = match kind {
                HRow::Hero if col == 0 => Some((At::Hero, CellAct::Play)),
                HRow::Hero => Some((At::Hero, CellAct::Open)),
                HRow::Nav => {
                    return self.activate_link(links.get(col).copied());
                }
                HRow::Resume | HRow::NextUp => Some((At::Home(row, col), CellAct::Play)),
                HRow::Latest(_) | HRow::Tiles => Some((At::Home(row, col), CellAct::Open)),
            };
            if let Some((at, a)) = act {
                return self.update_cell(at, a);
            }
        }
        Task::none()
    }

    /// The pointer moved onto a cell: it takes the keyboard focus (and drives the hero).
    pub(super) fn hover_home(&mut self, row: usize, col: usize) -> Task<Message> {
        match self.stack.last_mut().and_then(|p| p.home.as_deref_mut()) {
            Some(h) if (h.row, h.col) != (row, col) => {
                (h.row, h.col) = (row, col);
                h.touched = true;
                self.home_wants()
            }
            _ => Task::none(),
        }
    }
}

pub fn strip_id(ri: usize) -> Id {
    Id::from(format!("strip-{ri}"))
}

/// Previous / next buttons that scroll the strip by a page.
fn row_arrows<'a>(g: &Geo, ri: usize) -> El<'a> {
    let size = g.arrows.size.unwrap_or(22.0);
    let ink = g.arrows.color.map_or_else(t::text, |c| {
        let [r, g, b, a] = c.to_f32();
        Color { r, g, b, a }
    });
    let btn = |glyph: &'static str, dir: f32| -> El<'a> {
        Press::new(container(text(glyph).size(size)).center(Length::Fill))
            .width(size + 8.0)
            .height(size + 8.0)
            .on_press(Message::HomeStripPage(ri, dir))
            .style(move |s| iced::widget::button::Style {
                text_color: if matches!(
                    s,
                    iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
                ) {
                    t::accent_text()
                } else {
                    ink
                },
                ..Default::default()
            })
            .into()
    };
    row![btn("\u{2039}", -1.0), btn("\u{203A}", 1.0)].into()
}

fn note<'a>(s: &str) -> El<'a> {
    container(text(s.to_string()).size(t::BODY).color(t::text_secondary()))
        .padding([t::S6, t::page_pad()])
        .into()
}

fn strip(k: HRow) -> bool {
    !matches!(k, HRow::Nav | HRow::Hero)
}

/// A titled band holding one strip in a hairline panel, fading out at the right edge.
fn strip_band<'a>(
    title: &str,
    text_: &str,
    meta: String,
    strip: El<'a>,
    g: &Geo,
    ri: usize,
) -> El<'a> {
    let fade = container(space())
        .width(48)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(gradient(
                90.0,
                &[
                    (0.0, Color::from_rgba8(0x07, 0x08, 0x0c, 0.0)),
                    (1.0, Color::from_rgba8(0x07, 0x08, 0x0c, 0.94)),
                ],
            )),
            ..Default::default()
        });
    let body: El<'a> = if g.grid || !g.framed {
        strip
    } else {
        Stack::with_children(vec![
            strip,
            container(fade)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .into(),
        ])
        .into()
    };
    let framed = g.framed;
    let panel = container(body).clip(true).style(move |_| container::Style {
        border: if framed {
            style::border(t::hairline(), 1.0, t::r_panel())
        } else {
            style::border(t::NONE, 0.0, 0.0)
        },
        ..Default::default()
    });
    let mut col = column![].spacing(t::S6);
    if !g.title_t.hidden {
        col = col.push(
            container(section_with(
                title,
                text_,
                meta,
                &g.title_t,
                g.inset,
                g.details,
                (g.arrows.shown() && !g.grid).then(|| row_arrows(g, ri)),
            ))
            .height(g.head_title_h()),
        );
    }
    band(
        container(col.push(container(panel).padding([0.0, g.inset]))).padding(Padding {
            top: band_pad(),
            bottom: band_pad(),
            ..Padding::ZERO
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_fill_the_row_exactly() {
        let (w, h) = row_dims(1280.0, 190.0, 1.5);
        let avail = 1280.0 - 2.0 * t::page_pad() - 1.0;
        let cols = ((w + 2.0 * t::S3) * 0.0 + avail / (w + 2.0 * t::S3)).floor();
        assert!(cols >= 5.0 && (w + 2.0 * t::S3) * cols <= avail + 1.0);
        assert_eq!(h, (w * 1.5).round());
    }

    fn media(id: &str) -> Media {
        Media {
            id: id.into(),
            title: id.into(),
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        }
    }

    #[test]
    fn layout_follows_the_toggles_and_skips_rows_with_nothing() {
        let mut h = Home::new((true, true, true), Fanout::default(), Fanout::default());
        assert_eq!(h.layout(), [HRow::Nav, HRow::Hero]);
        h.resume_e = vec![Entry {
            primary: media("r"),
            alternates: vec![],
        }];
        let row = |items| LatestRow {
            origin: SourceId(1),
            lib: media("lib"),
            items,
        };
        h.latest = vec![
            row(Load::Ready(vec![media("a")])),
            row(Load::Ready(vec![])),
            row(Load::Failed),
            row(Load::Loading),
        ];
        assert_eq!(
            h.layout(),
            [
                HRow::Nav,
                HRow::Hero,
                HRow::Resume,
                HRow::Latest(0),
                HRow::Latest(3)
            ]
        );
        // The hero is the newest of the first row that has one, until a poster is focused.
        assert_eq!(h.hero().map(|m| m.id.as_str()), Some("a"));
        (h.row, h.col) = (2, 0);
        assert_eq!(h.hero().map(|m| m.id.as_str()), Some("r"));
        h.show.0 = false;
        assert_eq!(h.layout().len(), 4, "Continue watching switched off");
    }

    #[test]
    fn server_layout_follows_the_servers_sections_libraries_and_exclusions() {
        let mut h = Home::new((false, false, false), Fanout::default(), Fanout::default());
        let row = |id: &str, n: usize| LatestRow {
            origin: SourceId(1),
            lib: media(id),
            items: Load::Ready((0..n).map(|i| media(&format!("{id}{i}"))).collect()),
        };
        h.latest = vec![row("a", 1), row("b", 1), row("c", 0), row("d", 1)];
        h.resume_e = vec![Entry {
            primary: media("r"),
            alternates: vec![],
        }];
        // NOCTURNE layout: toggles decide (latest off), the server data is ignored.
        assert_eq!(h.layout().len(), 2);
        let mut ui = ServerUi {
            sections: vec![
                Section::Latest,
                Section::MyMedia,
                Section::Resume,
                Section::NextUp,
            ],
            ordered_views: vec!["d".into(), "a".into()],
            my_media_excludes: vec!["b".into()],
            latest_excludes: vec!["a".into()],
            ..Default::default()
        };
        h.server = Some(ui.clone());
        // Latest first (d, b; a excluded, c empty), then the tiles, then Continue watching; Next up has nothing.
        assert_eq!(
            h.layout(),
            [
                HRow::Nav,
                HRow::Hero,
                HRow::Latest(3),
                HRow::Latest(1),
                HRow::Tiles,
                HRow::Resume
            ]
        );
        let tiles: Vec<_> = h
            .items(HRow::Tiles)
            .iter()
            .map(|(m, _)| m.id.clone())
            .collect();
        assert_eq!(tiles, ["d", "a", "c"], "user order, hidden one dropped");
        ui.sections = vec![Section::LiveTv, Section::ResumeAudio];
        h.server = Some(ui);
        assert_eq!(
            h.layout(),
            [HRow::Nav, HRow::Hero],
            "unsupported kinds are skipped"
        );
    }

    fn parts(css: &str, sec: Option<cz::Section>) -> Parts {
        cz::parse(css).unwrap().layout.parts(Some(Page::Home), sec)
    }

    #[test]
    fn the_empty_layout_reproduces_todays_geometry() {
        let p = Parts::default();
        let poster = geo_for(1280.0, 176.0, HRow::Latest(0), &p);
        let wide = geo_for(1280.0, 176.0, HRow::Resume, &p);
        assert_eq!(poster.dims, row_dims(1280.0, 190.0, 1.5));
        assert_eq!(wide.dims, row_dims(1280.0, 300.0, 9.0 / 16.0));
        assert_eq!((poster.art, wide.art), (Art::Poster, Art::Landscape));
        // The built-in constants: cell, caption, band, header and page padding.
        assert_eq!(
            crate::widget::cell::cell_h(300.0),
            300.0 + 24.0 + 50.0 + 1.0
        );
        assert_eq!(
            poster.band(10),
            2.0 * 48.0 + 72.0 + 24.0 + poster.look.cell_h(poster.dims.1) + 2.0
        );
        assert_eq!(band_h(300.0), 2.0 * 48.0 + 72.0 + 24.0 + 375.0 + 2.0);
        assert_eq!((t::header_h(), t::page_pad()), (56.0, 48.0));
        assert_eq!(poster.look.pad, t::S3);
        assert_eq!(poster.inset, t::PAGE_PAD);
        assert!(!poster.grid && !poster.hidden);
        assert_eq!(hero_cfg(&p, 1280.0, 800.0), Some((500.0, false)));
    }

    #[test]
    fn card_row_and_hero_settings_change_the_geometry() {
        let css = ".card{aspect-ratio:16/9;width:300px;--title-position:none} .row{display:grid;gap:8px;--row-columns:4;padding:24px} .hero{height:640px;--hero-style:banner}";
        let g = geo_for(
            1280.0,
            176.0,
            HRow::Latest(0),
            &parts(css, Some(cz::Section::Latest)),
        );
        assert_eq!(
            (g.art, g.grid, g.cols, g.inset, g.look.pad),
            (Art::Landscape, true, 4, 24.0, 4.0)
        );
        assert_eq!(g.look.caption_h(), 0.0);
        assert_eq!(g.dims.1, (g.dims.0 * 9.0 / 16.0).round());
        assert_eq!(g.rows(9), 3);
        // No caption: the cell is the image plus its padding.
        assert_eq!(g.look.cell_h(g.dims.1), g.dims.1 + 8.0 + 1.0);
        assert_eq!(
            hero_cfg(&parts(css, None), 1280.0, 800.0),
            Some((640.0, true))
        );
        let none = parts(".hero{display:none}", None);
        assert_eq!(hero_cfg(&none, 1280.0, 800.0), None);
        let hidden = geo_for(
            1280.0,
            176.0,
            HRow::Resume,
            &parts(".section-resume{display:none}", Some(cz::Section::Resume)),
        );
        assert!(hidden.hidden);
    }

    #[test]
    fn sections_follow_order_and_hidden_ones_drop() {
        let order = cz::parse(".section-latest{order:-1} .section-next-up{order:5}")
            .unwrap()
            .layout
            .section_order();
        let mut v = vec![
            HRow::Nav,
            HRow::Hero,
            HRow::Resume,
            HRow::NextUp,
            HRow::Latest(0),
            HRow::Latest(1),
        ];
        arrange(&mut v, order, |k| k == HRow::Latest(1), true);
        assert_eq!(
            v,
            [
                HRow::Nav,
                HRow::Hero,
                HRow::Latest(0),
                HRow::Resume,
                HRow::NextUp
            ]
        );
        let mut v = vec![HRow::Nav, HRow::Resume, HRow::Latest(0)];
        arrange(&mut v, order, |_| false, false);
        assert_eq!(
            v,
            [HRow::Nav, HRow::Resume, HRow::Latest(0)],
            "a server's own order stays"
        );
    }

    #[test]
    fn hero_height_is_bounded() {
        assert_eq!(hero_h(1280.0, 800.0), 500.0);
        assert!(hero_h(2560.0, 1440.0) > 800.0 && hero_h(2560.0, 1440.0) <= 893.0);
    }

    #[test]
    fn slides_keep_todays_hero_first_without_a_theme_and_follow_the_order_with_one() {
        let art = |id: &str| Media {
            backdrop_tag: Some("t".into()),
            ..media(id)
        };
        let mut h = Home::new((true, true, true), Fanout::default(), Fanout::default());
        h.resume_e = vec![Entry {
            primary: art("r"),
            alternates: vec![],
        }];
        let mut lib = media("lib");
        lib.kind = Kind::Library;
        lib.collection = Some("movies".into());
        h.latest = vec![LatestRow {
            origin: SourceId::default(),
            lib,
            items: Load::Ready(vec![art("m")]),
        }];
        let ids = |v: Vec<Media>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();
        let order = &nocturne_core::hero::DEFAULT_ORDER;
        assert_eq!(ids(h.compute_slides(order, false)), ["r", "m"]);
        assert_eq!(ids(h.compute_slides(order, true)), ["m", "r"]);
    }
}
