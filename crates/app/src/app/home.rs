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
use crate::widget::cell::{Cell, Pic, cell_h, poster_cell, tile};
use crate::widget::{band, chip, mono, pill_button, section};
use iced::widget::Id;
use iced::widget::operation;
use iced::widget::scrollable::{self, AbsoluteOffset};
use iced::widget::{Stack, column, container, image, row, space, text};
use iced::{Alignment, Color, ContentFit, Element, Length, Padding, Task};
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
}

impl Home {
    pub fn new(show: (bool, bool, bool), resume: Fanout, nextup: Fanout) -> Self {
        Self {
            resume,
            nextup,
            resume_e: vec![],
            nextup_e: vec![],
            latest: vec![],
            latest_epoch: 0,
            show,
            row: 1,
            col: 0,
            off: 0.0,
            vh: 0.0,
            strips: HashMap::new(),
            server: None,
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
            Some(ui) => ui.tiles(self.latest.iter().map(|r| &r.lib).collect(), |m| {
                m.id.as_str()
            }),
            None => vec![],
        }
    }

    pub fn layout(&self) -> Vec<HRow> {
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

    /// The hero: the focused poster, else the newest item of the first row that has one.
    pub fn hero(&self) -> Option<&Media> {
        let lay = self.layout();
        let focused = match lay.get(self.row) {
            Some(k @ (HRow::Resume | HRow::NextUp | HRow::Latest(_))) => {
                self.items(*k).get(self.col).map(|(m, _)| *m)
            }
            _ => None,
        };
        focused.or_else(|| self.latest.iter().find_map(|r| r.items.items().first()))
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

const SECTION_H: f32 = 72.0;

/// Height of one strip band for cards whose image is `ch` tall.
pub fn band_h(ch: f32) -> f32 {
    2.0 * t::S8 * 1.5 + SECTION_H + t::S6 + cell_h(ch) + 2.0
}

impl App {
    pub(super) fn dims(&self) -> ((f32, f32), (f32, f32)) {
        let k = self.settings.density.card() / 176.0;
        (
            row_dims(self.win.width, 190.0 * k, 1.5),
            row_dims(self.win.width, 300.0 * k, 9.0 / 16.0),
        )
    }

    /// Fetch Home: libraries (header links, Latest rows), continue watching and next up, each from
    /// every online source at once.
    pub(super) fn load_home(&mut self, pid: u64) -> Task<Message> {
        let (fan, t1) = self.fan(pid, FanKind::Libs, |b| async move { b.libraries().await });
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
        if let Some(m) = h.hero() {
            w.extend(self.hero_wants(m));
        }
        let (size, wide) = self.dims();
        let hh = hero_h(self.win.width, self.win.height);
        let vh = if h.vh > 0.0 { h.vh } else { self.win.height };
        let mut top = hh;
        for (ri, k) in lay.iter().enumerate().skip(2) {
            let (dims, art) = if k.wide() {
                (wide, Art::Landscape)
            } else {
                (size, Art::Poster)
            };
            let bh = band_h(dims.1);
            let near = top + bh > h.off - vh * 0.5 && top < h.off + vh * 1.5;
            top += bh;
            if !near {
                continue;
            }
            let cell_w = dims.0 + 2.0 * t::S3 + 1.0;
            let avail = self.win.width - 2.0 * t::PAGE_PAD;
            let x = h.strips.get(&ri).copied().unwrap_or(0.0);
            let first = (x / cell_w).floor() as usize;
            let n = (avail / cell_w).ceil() as usize + 2;
            for (m, _) in h.items(*k).into_iter().skip(first).take(n) {
                w.push((m.clone(), art, dims.0 as u32));
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
        let (size, wide) = self.dims();
        let hh = hero_h(self.win.width, self.win.height);
        let hero_focus = (lay.get(h.row) == Some(&HRow::Hero)).then_some(h.col);
        let hero: El = match h.hero() {
            Some(m) => self.hero_band(m, hh, hero_focus),
            None => container(note(if h.loading() {
                "Loading your library"
            } else {
                "Nothing to show yet."
            }))
            .height(420)
            .padding(Padding {
                top: t::HEADER_H,
                ..Padding::ZERO
            })
            .into(),
        };
        let mut page = column![band(hero)];
        let several = self.several_sources();
        for (ri, k) in lay.iter().enumerate().skip(2) {
            let focus = (h.row == ri).then_some(h.col);
            let items = h.items(*k);
            let n = items.len();
            let (title, text_, meta, dims, art, play, loading) = match k {
                HRow::Resume => (
                    "Continue watching".to_string(),
                    "Pick up where you left off.".to_string(),
                    format!("{n} in progress"),
                    wide,
                    Art::Landscape,
                    true,
                    false,
                ),
                HRow::NextUp => (
                    "Next up".to_string(),
                    "The next episode of what you're following.".to_string(),
                    format!("{n} up next"),
                    wide,
                    Art::Landscape,
                    true,
                    false,
                ),
                HRow::Tiles => (
                    "My media".to_string(),
                    "Your libraries.".to_string(),
                    format!("{n} libraries"),
                    wide,
                    Art::Landscape,
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
                        size,
                        Art::Poster,
                        false,
                        matches!(r.items, Load::Loading),
                    )
                }
                _ => continue,
            };
            let strip = self.strip(ri, &items, dims, art, focus, play, loading);
            page = page.push(strip_band(&title, &text_, meta, strip));
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
                    .padding([t::S4, t::PAGE_PAD]),
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
        (cw, ch): (f32, f32),
        art: Art,
        focus: Option<usize>,
        play: bool,
        loading: bool,
    ) -> El<'_> {
        let writes = self.hooks.can_write();
        let several = self.several_sources();
        let mut cells: Vec<El> = vec![];
        if items.is_empty() {
            for i in 0..8 {
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
                    mark: None,
                    chips: None,
                    on_open: Message::Noop,
                    on_hover: Message::Noop,
                    on_fav: Message::Noop,
                    on_played: Message::Noop,
                }));
            }
        }
        for (ci, (m, copies)) in items.iter().enumerate() {
            let at = At::Home(ri, ci);
            let (title, meta) = match (art, m.kind) {
                (Art::Landscape, Kind::Episode) => (
                    m.series_name.clone().unwrap_or_else(|| m.title.clone()),
                    [m.short_ep(), m.title.clone()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" \u{b7} "),
                ),
                _ => (m.title.clone(), super::cells::caption(m)),
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
                mark: several
                    .then(|| mark_for(&self.catalog, m, *copies))
                    .flatten(),
                chips: Some((m.favorite, m.played, writes)),
                on_open: Message::Cell(at, if play { CellAct::Play } else { CellAct::Open }),
                on_hover: Message::Cell(at, CellAct::Hover),
                on_fav: Message::Cell(at, CellAct::Fav),
                on_played: Message::Cell(at, CellAct::Played),
            }));
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
    fn hero_band<'a>(&'a self, m: &'a Media, h: f32, focus: Option<usize>) -> El<'a> {
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
        let size = (w / 26.0).clamp(40.0, 72.0);
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
            None => text(m.title.clone())
                .size(size)
                .line_height(1.08)
                .font(t::bold())
                .color(t::text())
                .into(),
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
        let mut col = column![
            mono(format!("0x00 \u{b7} {}", m.kind_label())),
            chips,
            title_el
        ]
        .spacing(t::S4)
        .align_x(Alignment::Start);
        if let Some(s) = season_line {
            col = col.push(s);
        }
        col = col
            .push(
                container(
                    text(sub)
                        .size(t::BODY_LG)
                        .line_height(1.6)
                        .color(t::text_secondary()),
                )
                .max_width(560)
                .max_height(t::BODY_LG * 1.6 * 3.0)
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
            scrim(gradient(180.0, &[(0.45, clear), (1.0, solid)])),
            container(container(col).max_width(680))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(Alignment::End)
                .padding(Padding {
                    left: t::PAGE_PAD,
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
        let links = self.nav_links();
        let nav_w = self.nav_widths();
        let (nav_off, nav_vw, _) = self.nav_geom();
        let (size, wide) = self.dims();
        let (win_w, win_h) = (self.win.width, self.win.height);
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
            let hh = hero_h(win_w, win_h);
            // Page: bring the focused band into view.
            let mut top = hh;
            let mut want_y = 0.0;
            for (ri, k) in lay.iter().enumerate().skip(2) {
                let dims = if k.wide() { wide } else { size };
                if ri == h.row {
                    want_y = top;
                }
                top += band_h(dims.1);
            }
            if h.row >= 2 {
                let vh = if h.vh > 0.0 { h.vh } else { win_h };
                let bh = band_h(if k.wide() { wide.1 } else { size.1 });
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
            // Strip: bring the focused card into view.
            if h.row >= 2 {
                let dims = if k.wide() { wide } else { size };
                let cell_w = dims.0 + 2.0 * t::S3 + 1.0;
                let cur = h.strips.get(&h.row).copied().unwrap_or(0.0);
                let x = virt::into_view(cur, win_w - 2.0 * t::PAGE_PAD, cell_w, h.col);
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
                self.home_wants()
            }
            _ => Task::none(),
        }
    }
}

pub fn strip_id(ri: usize) -> Id {
    Id::from(format!("strip-{ri}"))
}

fn note<'a>(s: &str) -> El<'a> {
    container(text(s.to_string()).size(t::BODY).color(t::text_secondary()))
        .padding([t::S6, t::PAGE_PAD])
        .into()
}

/// A titled band holding one strip in a hairline panel, fading out at the right edge.
fn strip_band<'a>(title: &str, text_: &str, meta: String, strip: El<'a>) -> El<'a> {
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
    let panel = container(Stack::with_children(vec![
        strip,
        container(fade)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .into(),
    ]))
    .clip(true)
    .style(|_| container::Style {
        border: style::border(t::hairline(), 1.0, t::r_panel()),
        ..Default::default()
    });
    band(
        container(
            column![
                container(section(title, text_, meta)).height(SECTION_H),
                container(panel).padding([0.0, t::PAGE_PAD]),
            ]
            .spacing(t::S6),
        )
        .padding(Padding {
            top: t::S8 * 1.5,
            bottom: t::S8 * 1.5,
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
        let avail = 1280.0 - 2.0 * t::PAGE_PAD - 1.0;
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

    #[test]
    fn hero_height_is_bounded() {
        assert_eq!(hero_h(1280.0, 800.0), 500.0);
        assert!(hero_h(2560.0, 1440.0) > 800.0 && hero_h(2560.0, 1440.0) <= 893.0);
    }
}
