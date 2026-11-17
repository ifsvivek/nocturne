//! Library / collection / favourites pages: tabs, toolbar (sort, filters, view type), A-Z bar and
//! a virtualised grid or list. A page lists one source's items lazily paged (only the pages near
//! the viewport are requested), or, when several sources are in play, a merged list: every source's
//! pages load in the background and duplicates collapse into one cell. Port of the GPUI
//! `ui_browse.rs`; the state logic is in core (`libview`, `unified`).
use super::cells::{badge, caption, mark_for};
use super::images::Want;
use super::message::{At, CellAct, Message};
use super::{App, View};
use crate::style::{self, border};
use crate::task::run;
use crate::theme as t;
use crate::virt;
use crate::widget::cell::{Cell, Pic, cell_h, poster_cell, tile};
use crate::widget::popover::Popover;
use crate::widget::press::Press;
use crate::widget::{action_button, label, menu_row, mono, rule, section, tab};
use iced::widget::Id;
use iced::widget::operation;
use iced::widget::scrollable::{self, AbsoluteOffset};
use iced::widget::{Column, column, container, mouse_area, row, space, text};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::backend::Backend;
use nocturne_core::libview::{self as lv, FAct, LibPrefs, Scope, SortKey, Tab, ViewKind};
use nocturne_core::merge::Entry;
use nocturne_core::nav::{self, Dir};
use nocturne_core::source::{Art, Kind, Media, SourceId};
use nocturne_core::unified::{MERGED_PAGE, Merged, UKind};
use nocturne_jellyfin::{BrowseQuery, FilterOptions};
use std::collections::HashSet;

type El<'a> = Element<'a, Message>;

pub const GRID_ID: &str = "grid-scroll";
const LIST_ROW_H: f32 = 64.0;
const NAME_ROW_H: f32 = 56.0;
/// Rows built beyond each edge of the viewport.
const OVERSCAN: usize = 2;
/// What sits above the grid panel (header, title, tabs, toolbar): the viewport guess before the
/// first scroll event reports the real one.
const CHROME_H: f32 = 330.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    None,
    Sort,
    Filter,
}

#[derive(Clone)]
pub enum Mode {
    /// One source, pages fetched as they scroll into view.
    Paged(SourceId),
    /// Several sources, loaded in full and merged.
    Merged(Vec<SourceId>),
}

#[derive(Clone)]
pub enum BMsg {
    /// A page of a single source's list (`None`: the request failed).
    Page {
        epoch: u64,
        page: usize,
        r: Option<(Vec<Media>, usize)>,
    },
    /// A page of one source of a merged list.
    Merged {
        epoch: u64,
        origin: SourceId,
        start: usize,
        r: Option<(Vec<Media>, usize)>,
    },
    Options(Option<FilterOptions>, Option<Vec<(String, String)>>),
    Scrolled(f32, f32),
    Tab(Tab),
    Sort(SortKey),
    View(ViewKind),
    Letter(Option<char>),
    Filter(FAct),
    Menu(Menu),
    Pick(usize),
}

pub struct Browse {
    pub scope: Scope,
    pub mode: Mode,
    pub tabs: Vec<Tab>,
    pub tab: Tab,
    pub prefs: LibPrefs,
    pub letter: Option<char>,
    /// Bumped by every reload; answers carrying an older value are dropped.
    pub epoch: u64,
    pub total: usize,
    pub slots: Vec<Option<Media>>,
    pub merged: Option<Merged>,
    /// A smart collection page: its id and whether its sources are still answering.
    pub smart: Option<(u64, bool)>,
    pub list: Vec<Entry>,
    pub pending: HashSet<usize>,
    pub failed: bool,
    pub options: Option<FilterOptions>,
    pub studios: Option<Vec<(String, String)>>,
    pub names: Vec<lv::NameEntry>,
    pub menu: Menu,
    pub msel: usize,
    /// Scroll offset and viewport height of the grid (0 until the first scroll event).
    pub off: f32,
    pub vh: f32,
}

impl Browse {
    pub fn new(scope: Scope, mode: Mode, tabs: Vec<Tab>, prefs: LibPrefs) -> Self {
        Self {
            scope,
            mode,
            tabs,
            tab: Tab::Library,
            prefs,
            letter: None,
            epoch: 0,
            total: 0,
            slots: vec![],
            merged: None,
            smart: None,
            list: vec![],
            pending: HashSet::new(),
            failed: false,
            options: None,
            studios: None,
            names: vec![],
            menu: Menu::None,
            msel: 0,
            off: 0.0,
            vh: 0.0,
        }
    }

    pub fn is_merged(&self) -> bool {
        matches!(self.mode, Mode::Merged(_))
    }

    /// Entries the selection can land on.
    pub fn len(&self) -> usize {
        if self.tab.is_names() {
            self.names.len()
        } else if self.is_merged() {
            self.list.len()
        } else {
            self.slots.len()
        }
    }

    /// The item at `i` and how many sources have a copy of it.
    pub fn item(&self, i: usize) -> Option<(&Media, usize)> {
        if self.is_merged() {
            let e = self.list.get(i)?;
            Some((&e.primary, 1 + e.alternates.len()))
        } else {
            self.slots.get(i)?.as_ref().map(|m| (m, 1))
        }
    }

    fn query(&self, start: usize) -> BrowseQuery {
        let mut q = lv::build_query(&self.scope, self.tab, &self.prefs, self.letter, start);
        if self.is_merged() {
            q.limit = Some(MERGED_PAGE as u32);
        }
        q
    }

    fn rows(&self) -> Vec<lv::FRow> {
        lv::filter_rows(&self.prefs.filters, self.options.as_ref())
    }

    fn show_letters(&self) -> bool {
        self.smart.is_none()
            && self.prefs.sort == SortKey::Name
            && matches!(
                self.tab,
                Tab::Library | Tab::Favourites | Tab::Collections | Tab::Artists
            )
    }

    /// Viewport height to build for: the reported one, else a guess from the window.
    fn viewport(&self, win_h: f32) -> f32 {
        if self.vh > 0.0 {
            self.vh
        } else {
            (win_h - CHROME_H).max(200.0)
        }
    }
}

/// Grid geometry for a window: columns, cell image size and the (fixed) row height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub cols: usize,
    /// Cell image size.
    pub w: f32,
    pub h: f32,
    pub row_h: f32,
    /// Full cell width including its padding.
    pub cell_w: f32,
}

pub fn metrics(win_w: f32, card: f32, view: ViewKind, names: bool) -> Metrics {
    let avail = win_w - 2.0 * t::PAGE_PAD - 1.0;
    let k = card / 176.0;
    let (cardw, ratio) = match view {
        ViewKind::Poster => (card, 1.5),
        ViewKind::Landscape => (300.0 * k, 9.0 / 16.0),
        ViewKind::List => (avail, 0.0),
    };
    let cols = match (view, names) {
        (_, true) => nav::columns(avail, 260.0, 0.0),
        (ViewKind::List, _) => 1,
        _ => nav::columns(avail, cardw + 2.0 * t::S3, 0.0),
    };
    let cell_w = (avail / cols as f32).floor();
    let w = cell_w - 2.0 * t::S3 - 1.0;
    let h = (w * ratio).round();
    let row_h = if names {
        NAME_ROW_H
    } else if view == ViewKind::List {
        LIST_ROW_H
    } else {
        cell_h(h)
    };
    Metrics {
        cols,
        w,
        h,
        row_h,
        cell_w,
    }
}

fn names_of(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

pub fn scope_library(m: &Media) -> Scope {
    let types = lv::types_for(m.collection.as_deref());
    Scope {
        key: m.id.clone(),
        title: m.title.clone(),
        subtitle: "Everything in this library.".into(),
        collection: m.collection.clone(),
        base: BrowseQuery {
            parent_id: Some(m.id.clone()),
            include_types: types.clone().map_or(vec![], |t| names_of(&t)),
            recursive: types.is_some(),
            ..Default::default()
        },
    }
}

/// A folder or photo album: its direct children, in a grid.
pub fn scope_folder(m: &Media) -> Scope {
    Scope {
        key: m.id.clone(),
        title: m.title.clone(),
        subtitle: "Everything in this folder.".into(),
        collection: Some("photos".into()),
        base: BrowseQuery {
            parent_id: Some(m.id.clone()),
            ..Default::default()
        },
    }
}

/// Collections, Playlists, Favourites: everything of that kind, from every source.
pub fn scope_global(key: &str) -> Scope {
    let (title, sub, types, fav): (_, _, &[&str], _) = match key {
        "playlists" => ("Playlists", "Your lists.", &["Playlist"], false),
        "favourites" => (
            "Favourites",
            "Everything you have starred.",
            &[
                "Movie",
                "Series",
                "Episode",
                "BoxSet",
                "MusicAlbum",
                "MusicArtist",
                "Audio",
                "Video",
            ],
            true,
        ),
        _ => (
            "Collections",
            "Movies and shows grouped together.",
            &["BoxSet"],
            false,
        ),
    };
    Scope {
        key: key.into(),
        title: title.into(),
        subtitle: sub.into(),
        collection: None,
        base: BrowseQuery {
            include_types: names_of(types),
            filters: if fav {
                names_of(&["IsFavorite"])
            } else {
                vec![]
            },
            recursive: true,
            ..Default::default()
        },
    }
}

/// Titles under a genre or studio.
pub fn scope_tag(kind: &str, key: &str, label: &str) -> Scope {
    let mut base = BrowseQuery {
        include_types: names_of(&["Movie", "Series"]),
        recursive: true,
        ..Default::default()
    };
    match kind {
        "studio" => base.studio_ids = vec![key.into()],
        "music" => {
            base.include_types = names_of(&["MusicAlbum"]);
            base.genres = vec![key.into()];
        }
        _ => base.genres = vec![key.into()],
    }
    Scope {
        key: format!("{kind}-{key}"),
        title: label.into(),
        subtitle: if kind == "studio" {
            "Titles from this studio."
        } else {
            "Titles in this genre."
        }
        .into(),
        collection: None,
        base,
    }
}

fn scroll_top() -> Task<Message> {
    operation::scroll_to(Id::from(GRID_ID), AbsoluteOffset { x: 0.0, y: 0.0 })
}

impl App {
    // ---- opening ----

    /// Open a browse page. `origin` pins it to one source (a library, a folder); without it the
    /// page covers every source the filter admits.
    pub(super) fn open_browse(
        &mut self,
        scope: Scope,
        origin: Option<SourceId>,
        media: Option<Media>,
    ) -> Task<Message> {
        let sources: Vec<SourceId> = self
            .catalog
            .online()
            .filter(|s| self.filter.admits(s.id))
            .map(|s| s.id)
            .collect();
        let mode = match (origin, sources.as_slice()) {
            (Some(o), _) => Mode::Paged(o),
            (None, [one]) => Mode::Paged(*one),
            (None, []) => {
                self.toast("No source is available right now.");
                return Task::none();
            }
            (None, _) => Mode::Merged(sources),
        };
        // Name tabs and the like need one server's filter options.
        let tabs = match (&mode, scope.collection.is_some()) {
            (Mode::Merged(_), _) => vec![Tab::Library, Tab::Favourites],
            (_, true) => lv::tabs_for(scope.collection.as_deref()),
            _ => vec![],
        };
        let prefs = self
            .settings
            .libs
            .get(&scope.key)
            .cloned()
            .unwrap_or_default();
        let media = media.or_else(|| {
            (!scope.key.is_empty()).then(|| Media {
                id: scope.key.clone(),
                title: scope.title.clone(),
                collection: scope.collection.clone(),
                kind: Kind::Library,
                ..Media::from_item(&nocturne_jellyfin::Item::default())
            })
        });
        let pid = self.push_page(View::Grid, media);
        if let Some(p) = self.page_mut(pid) {
            p.br = Some(Box::new(Browse::new(scope, mode, tabs, prefs)));
        }
        self.browse_reload(pid)
    }

    pub(super) fn open_library(&mut self, m: Media) -> Task<Message> {
        self.stack.truncate(1);
        let origin = m.origin;
        self.open_browse(scope_library(&m), Some(origin), Some(m))
    }

    pub(super) fn open_global(&mut self, i: usize) -> Task<Message> {
        let Some((key, _)) = super::GLOBALS.get(i) else {
            return Task::none();
        };
        self.stack.truncate(1);
        self.fwd.clear();
        self.open_browse(scope_global(key), None, None)
    }

    pub(super) fn open_kind(&mut self, k: UKind) -> Task<Message> {
        self.close_admin();
        self.stack.truncate(1);
        self.fwd.clear();
        self.open_browse(k.scope(), None, None)
    }

    // ---- loading ----

    fn br_mut(&mut self, pid: u64) -> Option<&mut Browse> {
        self.page_mut(pid)?.br.as_deref_mut()
    }

    /// Start over with the current tab/sort/filters/letter.
    pub(super) fn browse_reload(&mut self, pid: u64) -> Task<Message> {
        self.epoch += 1;
        let epoch = self.epoch;
        let Some(p) = self.stack.iter_mut().find(|p| p.pid == pid) else {
            return Task::none();
        };
        p.sel = 0;
        let Some(br) = p.br.as_deref_mut() else {
            return Task::none();
        };
        br.epoch = epoch;
        (br.slots, br.list, br.merged) = (vec![], vec![], None);
        (br.total, br.off) = (0, 0.0);
        br.pending.clear();
        br.failed = false;
        if let Some((id, _)) = br.smart {
            return Task::batch([scroll_top(), self.smart_reload(id)]);
        }
        let scroll = scroll_top();
        if br.tab.is_names() {
            return Task::batch([scroll, self.ensure_options(pid)]);
        }
        let mut tasks = vec![scroll];
        let q = br.query(0);
        let ask = |b: Backend, q: BrowseQuery| async move { b.browse(&q).await.ok() };
        match br.mode.clone() {
            Mode::Paged(origin) => {
                br.pending.insert(0);
                match self.catalog.backend(origin).cloned() {
                    Some(b) => tasks.push(run(ask(b, q), move |r| {
                        Message::Browse(pid, BMsg::Page { epoch, page: 0, r })
                    })),
                    None => br.failed = true,
                }
            }
            Mode::Merged(sources) => {
                br.merged = Some(Merged::new(epoch, &sources));
                for origin in sources {
                    if let Some(b) = self.catalog.backend(origin).cloned() {
                        tasks.push(run(ask(b, q.clone()), move |r| {
                            Message::Browse(
                                pid,
                                BMsg::Merged {
                                    epoch,
                                    origin,
                                    start: 0,
                                    r,
                                },
                            )
                        }));
                    }
                }
            }
        }
        Task::batch(tasks)
    }

    fn browse_page(&mut self, pid: u64, page: usize) -> Task<Message> {
        let Some(br) = self.br_mut(pid) else {
            return Task::none();
        };
        let Mode::Paged(origin) = br.mode else {
            return Task::none();
        };
        br.pending.insert(page); // stays set on failure: no retry storm while scrolling
        let (epoch, q) = (br.epoch, br.query(page * lv::PAGE));
        let Some(b) = self.catalog.backend(origin).cloned() else {
            return Task::none();
        };
        run(async move { b.browse(&q).await.ok() }, move |r| {
            Message::Browse(pid, BMsg::Page { epoch, page, r })
        })
    }

    /// Filter options and studios, once per page (name tabs and the Filters panel need them).
    fn ensure_options(&mut self, pid: u64) -> Task<Message> {
        let Some(br) = self.br_mut(pid) else {
            return Task::none();
        };
        if let Some(o) = &br.options {
            br.names = lv::name_entries(br.tab, o, br.studios.as_deref().unwrap_or(&[]));
            return Task::none();
        }
        let Mode::Paged(origin) = br.mode else {
            return Task::none();
        };
        let parent = br.scope.base.parent_id.clone();
        let types = br.scope.base.include_types.clone();
        let Some(src) = self
            .catalog
            .backend(origin)
            .and_then(|b| b.jellyfin())
            .cloned()
        else {
            // A local source has no genres, studios or tags to list.
            if let Some(br) = self.br_mut(pid) {
                br.options = Some(FilterOptions::default());
                br.studios = Some(vec![]);
            }
            return Task::none();
        };
        run(
            async move {
                let o = src.filter_options(parent.as_deref(), &types).await.ok();
                let s = src.studios(parent.as_deref()).await.ok();
                (o, s)
            },
            move |(o, s)| Message::Browse(pid, BMsg::Options(o, s)),
        )
    }

    fn save_prefs(&mut self, pid: u64) {
        if let Some(br) = self.page_mut(pid).and_then(|p| p.br.as_deref()) {
            let (k, v) = (br.scope.key.clone(), br.prefs.clone());
            self.settings.libs.insert(k, v);
            self.settings.save();
        }
    }

    /// Fetch the pages and images the rows in view (plus lookahead) still lack.
    pub(super) fn browse_visible(&mut self) -> Task<Message> {
        let card = self.settings.density.card();
        let (win_w, win_h) = (self.win.width, self.win.height);
        let Some(p) = self.stack.last().filter(|p| p.view == View::Grid) else {
            return Task::none();
        };
        let Some(br) = p.br.as_deref() else {
            return Task::none();
        };
        if br.tab.is_names() || br.failed {
            return Task::none();
        }
        let pid = p.pid;
        let m = metrics(win_w, card, br.prefs.view, false);
        let rows = virt::visible_rows(
            br.off,
            br.viewport(win_h),
            m.row_h,
            lv::rows(br.len(), m.cols),
            1,
        );
        let range = virt::items_of(&rows, m.cols, br.len());
        let pages = if br.is_merged() {
            vec![]
        } else {
            lv::missing_pages(&br.slots, &br.pending, range.clone())
        };
        let art = match br.prefs.view {
            ViewKind::Landscape => Art::Landscape,
            _ => Art::Poster,
        };
        let draw_w = if br.prefs.view == ViewKind::List {
            34
        } else {
            m.w as u32
        };
        let wants: Vec<Want> = range
            .filter_map(|i| br.item(i))
            .map(|(m, _)| (m.clone(), art, draw_w))
            .collect();
        let mut tasks: Vec<Task<Message>> = pages
            .into_iter()
            .map(|pg| self.browse_page(pid, pg))
            .collect();
        tasks.push(self.want(&wants));
        Task::batch(tasks)
    }

    // ---- messages ----

    pub(super) fn browse_msg(&mut self, pid: u64, msg: BMsg) -> Task<Message> {
        let prio = self.catalog.priority();
        match msg {
            BMsg::Page { epoch, page, r } => {
                let Some(br) = self.br_mut(pid).filter(|b| b.epoch == epoch) else {
                    return Task::none();
                };
                match r {
                    Some((items, total)) => {
                        if page == 0 {
                            br.total = total;
                            br.slots = vec![None; total];
                        }
                        lv::fill(&mut br.slots, page * lv::PAGE, items);
                        br.pending.remove(&page);
                    }
                    None if page == 0 => br.failed = true,
                    None => {}
                }
                self.browse_visible()
            }
            BMsg::Merged {
                epoch,
                origin,
                start,
                r,
            } => {
                let (prefs, series) = (&self.prefs, &self.series_ids);
                let Some(p) = self.stack.iter_mut().find(|p| p.pid == pid) else {
                    return Task::none();
                };
                let Some(br) = p.br.as_deref_mut().filter(|b| b.epoch == epoch) else {
                    return Task::none();
                };
                let Some(mg) = br.merged.as_mut() else {
                    return Task::none();
                };
                let next = mg.accept(epoch, origin, start, r);
                br.list = mg.entries(&prio, prefs, series, br.prefs.sort, br.prefs.desc);
                br.total = br.list.len();
                br.failed = mg.failed();
                let mut tasks = vec![];
                if let Some(next) = next {
                    let mut q = br.query(next);
                    q.start_index = Some(next as u32);
                    if let Some(b) = self.catalog.backend(origin).cloned() {
                        tasks.push(run(async move { b.browse(&q).await.ok() }, move |r| {
                            Message::Browse(
                                pid,
                                BMsg::Merged {
                                    epoch,
                                    origin,
                                    start: next,
                                    r,
                                },
                            )
                        }));
                    }
                }
                tasks.push(self.browse_visible());
                Task::batch(tasks)
            }
            BMsg::Options(o, s) => {
                if let Some(br) = self.br_mut(pid) {
                    br.options = Some(o.unwrap_or_default());
                    br.studios = Some(s.unwrap_or_default());
                    br.names = lv::name_entries(
                        br.tab,
                        br.options.as_ref().expect("just set"),
                        br.studios.as_deref().unwrap_or(&[]),
                    );
                }
                Task::none()
            }
            BMsg::Scrolled(off, vh) => {
                crate::timing::scroll_event();
                if let Some(br) = self.br_mut(pid) {
                    (br.off, br.vh) = (off, vh);
                }
                self.browse_visible()
            }
            BMsg::Tab(tab) => {
                if let Some(br) = self.br_mut(pid) {
                    br.tab = tab;
                    br.letter = None;
                    br.menu = Menu::None;
                }
                self.browse_reload(pid)
            }
            BMsg::Sort(key) => {
                if let Some(br) = self.br_mut(pid) {
                    if br.prefs.sort == key && key != SortKey::Random {
                        br.prefs.desc = !br.prefs.desc;
                    } else {
                        br.prefs.sort = key;
                        br.prefs.desc = key.default_desc();
                    }
                    br.letter = br.letter.filter(|_| key == SortKey::Name);
                    br.menu = Menu::None;
                }
                self.save_prefs(pid);
                self.browse_reload(pid)
            }
            BMsg::View(v) => {
                if let Some(br) = self.br_mut(pid) {
                    br.prefs.view = v;
                }
                self.save_prefs(pid);
                self.browse_visible()
            }
            BMsg::Letter(c) => {
                if let Some(br) = self.br_mut(pid) {
                    br.letter = c;
                }
                self.browse_reload(pid)
            }
            BMsg::Filter(a) => {
                if let Some(br) = self.br_mut(pid) {
                    lv::toggle(&mut br.prefs.filters, &a);
                }
                self.save_prefs(pid);
                self.browse_reload(pid)
            }
            BMsg::Menu(m) => {
                let Some(br) = self.br_mut(pid) else {
                    return Task::none();
                };
                br.menu = if br.menu == m { Menu::None } else { m };
                br.msel = match br.menu {
                    Menu::Sort => SortKey::ALL
                        .iter()
                        .position(|k| *k == br.prefs.sort)
                        .unwrap_or(0),
                    _ => 0,
                };
                if m == Menu::Filter && br.menu == Menu::Filter {
                    return self.ensure_options(pid);
                }
                Task::none()
            }
            BMsg::Pick(i) => {
                let Some(br) = self.br_mut(pid) else {
                    return Task::none();
                };
                let Some(e) = br.names.get(i).cloned() else {
                    return Task::none();
                };
                e.pick.apply(&mut br.prefs.filters);
                br.tab = Tab::Library;
                self.browse_reload(pid)
            }
        }
    }

    /// Reload the page in view (a source came back, the filter or the order of sources changed).
    pub(super) fn reload_browse(&mut self) -> Task<Message> {
        self.smart.clear();
        match self.stack.last() {
            Some(p) if p.view == View::Grid => {
                let pid = p.pid;
                self.browse_reload(pid)
            }
            _ => Task::none(),
        }
    }

    // ---- keys ----

    /// Keys for a browse page. `None`: not consumed.
    pub(super) fn browse_key(&mut self, key: &str, shift: bool) -> Option<Task<Message>> {
        let card = self.settings.density.card();
        let win = self.win;
        let p = self.stack.last()?;
        let pid = p.pid;
        let br = p.br.as_deref()?;
        let msg = |m: BMsg| Task::done(Message::Browse(pid, m));
        if br.menu != Menu::None {
            let rows = br.rows();
            let n = if br.menu == Menu::Sort {
                SortKey::ALL.len()
            } else {
                rows.len()
            };
            let (menu, sel) = (br.menu, br.msel);
            let selectable = |i: usize| menu == Menu::Sort || rows[i].act.is_some();
            let step = |i: usize, d: i32| -> usize {
                let mut j = i as i32;
                loop {
                    j += d;
                    if j < 0 || j as usize >= n {
                        return i;
                    }
                    if selectable(j as usize) {
                        return j as usize;
                    }
                }
            };
            let i = match key {
                "up" => step(sel, -1),
                "down" => step(sel, 1),
                "escape" | "o" | "l" => return Some(msg(BMsg::Menu(Menu::None))),
                "enter" | "space" => {
                    return Some(if menu == Menu::Sort {
                        msg(BMsg::Sort(SortKey::ALL[sel.min(n - 1)]))
                    } else if let Some(a) = rows.get(sel).and_then(|r| r.act.clone()) {
                        msg(BMsg::Filter(a))
                    } else {
                        Task::none()
                    });
                }
                _ => return Some(Task::none()), // modal: swallow everything else
            };
            if let Some(br) = self.br_mut(pid) {
                br.msel = i;
            }
            return Some(Task::none());
        }
        let names = br.tab.is_names();
        let m = metrics(win.width, card, br.prefs.view, names);
        Some(match key {
            "o" => msg(BMsg::Menu(Menu::Sort)),
            "l" => msg(BMsg::Menu(Menu::Filter)),
            "v" => {
                let all = ViewKind::ALL;
                let cur = all.iter().position(|v| *v == br.prefs.view).unwrap_or(0);
                msg(BMsg::View(all[(cur + 1) % all.len()]))
            }
            "tab" if br.tabs.len() > 1 => {
                let cur = br.tabs.iter().position(|t| *t == br.tab).unwrap_or(0) as i32;
                let n = br.tabs.len() as i32;
                let next = br.tabs[(cur + if shift { n - 1 } else { 1 }).rem_euclid(n) as usize];
                msg(BMsg::Tab(next))
            }
            "[" | "]" if br.show_letters() => {
                let c = lv::step_letter(br.letter, if key == "]" { 1 } else { -1 });
                msg(BMsg::Letter(c))
            }
            "r" if br.tab == Tab::Random || br.prefs.sort == SortKey::Random => {
                Task::done(Message::PageRetry)
            }
            "up" | "down" | "left" | "right" | "enter" => {
                let n = br.len();
                let sel = p.sel;
                let dir = match key {
                    "up" => Some(Dir::Up),
                    "down" => Some(Dir::Down),
                    "left" => Some(Dir::Left),
                    "right" => Some(Dir::Right),
                    _ => None,
                };
                if let Some(d) = dir {
                    let vh = br.viewport(win.height);
                    let off = br.off;
                    let to = nav::step(sel, m.cols, n, d);
                    if let Some(p) = self.stack.last_mut() {
                        p.sel = to;
                    }
                    let y = virt::into_view(off, vh, m.row_h, to / m.cols.max(1));
                    if (y - off).abs() > 0.5 {
                        return Some(operation::scroll_to(
                            Id::from(GRID_ID),
                            AbsoluteOffset { x: 0.0, y },
                        ));
                    }
                    Task::none()
                } else if names {
                    msg(BMsg::Pick(sel))
                } else if br.item(sel).is_some() {
                    Task::done(Message::Cell(At::Grid(sel), CellAct::Open))
                } else {
                    Task::none()
                }
            }
            _ => return None,
        })
    }

    // ---- view ----

    pub(super) fn grid_page(&self) -> El<'_> {
        let Some(p) = self.stack.last() else {
            return space().into();
        };
        let Some(br) = p.br.as_deref() else {
            return space().into();
        };
        let pid = p.pid;
        let (view, names_tab) = (br.prefs.view, br.tab.is_names());
        let m = metrics(
            self.win.width,
            self.settings.density.card(),
            view,
            names_tab,
        );
        let n = br.len();
        let loading = n == 0
            && !br.failed
            && !names_tab
            && match (&br.merged, br.smart) {
                (_, Some((_, loading))) => loading,
                (Some(g), _) => g.loading(),
                (None, _) => br.epoch == 0 || br.pending.contains(&0),
            };
        let count = if br.failed {
            "unavailable".to_string()
        } else if names_tab {
            format!("{} entries", lv::thousands(n))
        } else if loading {
            "loading".to_string()
        } else if br.merged.as_ref().is_some_and(Merged::loading) || br.smart.is_some_and(|s| s.1) {
            format!("{} items, loading more", lv::thousands(n))
        } else {
            format!(
                "{} items",
                lv::thousands(if br.is_merged() { n } else { br.total })
            )
        };
        let body: El = if br.failed {
            self.failed_view("This couldn't be loaded.")
        } else if loading {
            row((0..m.cols.min(8)).map(|i| skeleton(i, &m))).into()
        } else if n == 0 {
            note(if names_tab && br.options.is_none() {
                "Loading"
            } else if br.prefs.filters.active() > 0 || br.letter.is_some() {
                "Nothing matches. Try clearing a filter."
            } else {
                "Nothing here yet."
            })
        } else {
            let rows = lv::rows(n, m.cols);
            let range = virt::visible_rows(
                br.off,
                br.viewport(self.win.height),
                m.row_h,
                rows,
                OVERSCAN,
            );
            let mut col = Column::new().width(Length::Fill);
            col = col.push(space().height(range.start as f32 * m.row_h));
            for r in range.clone() {
                col = col.push(if names_tab {
                    self.names_row(br, r, &m, p.sel, pid)
                } else {
                    self.browse_row(br, r, &m, p.sel)
                });
            }
            col = col.push(space().height((rows - range.end) as f32 * m.row_h));
            let grid = scrollable::Scrollable::new(col)
                .id(Id::from(GRID_ID))
                .on_scroll(move |v| {
                    Message::Browse(
                        pid,
                        BMsg::Scrolled(v.absolute_offset().y, v.bounds().height),
                    )
                })
                .style(style::scroll)
                .width(Length::Fill)
                .height(Length::Fill);
            if view == ViewKind::List && !names_tab {
                column![list_header(), container(grid).height(Length::Fill)].into()
            } else {
                grid.into()
            }
        };
        let tab_row = (br.tabs.len() > 1).then(|| {
            let mut r = row![].spacing(t::S1);
            for tb in &br.tabs {
                r = r.push(tab(
                    tb.label(br.scope.collection.as_deref()),
                    *tb == br.tab,
                    Message::Browse(pid, BMsg::Tab(*tb)),
                ));
            }
            column![container(r).padding([0.0, t::PAGE_PAD]), rule()]
        });
        let mut page = column![
            container(section(&br.scope.title, &br.scope.subtitle, String::new())),
            space().height(t::S4),
        ];
        if let Some(tr) = tab_row {
            page = page.push(tr);
        }
        page = page.push(self.browse_toolbar(br, pid, count));
        page = page.push(
            container(
                container(body)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .clip(true)
                    .style(|_| container::Style {
                        border: border(t::HAIRLINE, 1.0, t::R_PANEL),
                        ..Default::default()
                    }),
            )
            .padding(Padding {
                left: t::PAGE_PAD,
                right: t::PAGE_PAD,
                bottom: t::S6,
                ..Padding::ZERO
            })
            .height(Length::Fill),
        );
        container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: t::HEADER_H + t::S8,
                ..Padding::ZERO
            })
            .into()
    }

    fn browse_toolbar<'a>(&'a self, br: &'a Browse, pid: u64, count: String) -> El<'a> {
        let (sort, desc, view, menu) = (br.prefs.sort, br.prefs.desc, br.prefs.view, br.menu);
        let fixed = matches!(br.tab, Tab::Recent | Tab::Random);
        let nfilters = br.prefs.filters.active();
        let sort_label = if fixed {
            if br.tab == Tab::Recent {
                "Last played".to_string()
            } else {
                "Random".to_string()
            }
        } else {
            format!(
                "{} {}",
                sort.label(),
                if sort == SortKey::Random {
                    ""
                } else if desc {
                    "\u{2193}"
                } else {
                    "\u{2191}"
                }
            )
        };
        let mut bar = row![].spacing(t::S3).align_y(Alignment::Center);
        if !br.tab.is_names() {
            let sort_btn = action_button(
                &format!("Sort: {sort_label}"),
                false,
                false,
                menu == Menu::Sort,
                fixed,
                Message::Browse(
                    pid,
                    if fixed {
                        BMsg::Menu(Menu::None)
                    } else {
                        BMsg::Menu(Menu::Sort)
                    },
                ),
            );
            let sort_pop = (menu == Menu::Sort).then(|| {
                let items: Vec<El> = SortKey::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(i, k)| {
                        let cur = k == sort;
                        menu_row(
                            row![
                                text(k.label()).size(t::BODY).color(if cur {
                                    t::ACCENT_TEXT
                                } else {
                                    t::TEXT
                                }),
                                space().width(Length::Fill),
                                mono(if cur && k != SortKey::Random {
                                    if desc { "\u{2193}" } else { "\u{2191}" }
                                } else {
                                    ""
                                }),
                            ]
                            .spacing(t::S3),
                            i == br.msel,
                            false,
                            Message::Browse(pid, BMsg::Sort(k)),
                        )
                    })
                    .collect();
                container(column(items).spacing(2))
                    .width(240)
                    .padding(t::S2)
                    .style(style::panel)
                    .into()
            });
            let own_order = br.smart.is_some();
            bar = bar.extend((!own_order).then(|| -> El<'_> {
                Popover::new(
                    sort_btn,
                    sort_pop,
                    Message::Browse(pid, BMsg::Menu(Menu::None)),
                )
                .left()
                .into()
            }));
            let filter_btn = action_button(
                &if nfilters > 0 {
                    format!("Filters ({nfilters})")
                } else {
                    "Filters".to_string()
                },
                nfilters > 0,
                false,
                menu == Menu::Filter,
                false,
                Message::Browse(pid, BMsg::Menu(Menu::Filter)),
            );
            let filter_pop = (menu == Menu::Filter).then(|| {
                let mut items: Vec<El> = vec![];
                for (i, r) in br.rows().into_iter().enumerate() {
                    let Some(act) = r.act.clone() else {
                        items.push(
                            container(label(&r.label))
                                .padding(Padding {
                                    left: t::S3,
                                    top: t::S3,
                                    bottom: t::S1,
                                    ..Padding::ZERO
                                })
                                .into(),
                        );
                        continue;
                    };
                    items.push(menu_row(
                        row![
                            text(r.label).size(t::BODY).color(if r.on {
                                t::ACCENT_TEXT
                            } else {
                                t::TEXT
                            }),
                            space().width(Length::Fill),
                            mono(if r.on { "\u{2713}" } else { "" }),
                        ]
                        .spacing(t::S3),
                        i == br.msel,
                        false,
                        Message::Browse(pid, BMsg::Filter(act)),
                    ));
                }
                container(
                    scrollable::Scrollable::new(column(items).spacing(2)).style(style::scroll),
                )
                .width(300)
                .max_height(420)
                .padding(t::S2)
                .style(style::panel)
                .into()
            });
            bar = bar.extend((!own_order).then(|| -> El<'_> {
                Popover::new(
                    filter_btn,
                    filter_pop,
                    Message::Browse(pid, BMsg::Menu(Menu::None)),
                )
                .left()
                .into()
            }));
            let mut seg = row![].spacing(t::S1);
            for v in ViewKind::ALL {
                seg = seg.push(action_button(
                    v.label(),
                    v == view,
                    false,
                    false,
                    false,
                    Message::Browse(pid, BMsg::View(v)),
                ));
            }
            bar = bar.push(seg);
        }
        bar = bar.push(space().width(Length::Fill)).push(mono(count));
        let mut col = column![bar].spacing(t::S3);
        if br.show_letters() {
            let mut az = row![].spacing(2);
            for c in lv::letters() {
                let on = br.letter == Some(c);
                az = az.push(
                    Press::new(
                        container(text(c.to_string()).font(t::MONO).size(t::MICRO + 1.0))
                            .center(Length::Fill),
                    )
                    .width(24)
                    .height(22)
                    .on_press(Message::Browse(
                        pid,
                        BMsg::Letter(if on { None } else { Some(c) }),
                    ))
                    .style(move |s| iced::widget::button::Style {
                        background: if on {
                            Some(t::ACCENT.into())
                        } else if matches!(
                            s,
                            iced::widget::button::Status::Hovered
                                | iced::widget::button::Status::Pressed
                        ) {
                            Some(t::GLASS.into())
                        } else {
                            None
                        },
                        text_color: if on { t::ON_ACCENT } else { t::TEXT_SECONDARY },
                        border: border(t::NONE, 0.0, t::R_XS),
                        ..Default::default()
                    }),
                );
            }
            col = col.push(az);
        }
        container(col)
            .padding([t::S3, t::PAGE_PAD])
            .width(Length::Fill)
            .into()
    }

    fn browse_row<'a>(&'a self, br: &'a Browse, r: usize, m: &Metrics, sel: usize) -> El<'a> {
        let view = br.prefs.view;
        let writes = self.hooks.can_write();
        let several = self.several_sources();
        let end = ((r + 1) * m.cols).min(br.len());
        let mut cells: Vec<El> = vec![];
        for idx in r * m.cols..end {
            let at = At::Grid(idx);
            let Some((md, copies)) = br.item(idx) else {
                cells.push(skeleton(idx, m));
                continue;
            };
            if view == ViewKind::List {
                cells.push(self.list_row(md, copies, idx, idx == sel));
                continue;
            }
            let art = if view == ViewKind::Landscape {
                Art::Landscape
            } else {
                Art::Poster
            };
            cells.push(poster_cell(Cell {
                pic: self.pic(md, art, m.w as u32),
                title: md.title.clone(),
                meta: caption(md),
                index: idx,
                w: m.w,
                h: m.h,
                focused: idx == sel,
                progress: md.progress(),
                badge: badge(md),
                mark: several
                    .then(|| mark_for(&self.catalog, md, copies))
                    .flatten(),
                chips: Some((md.favorite, md.played, writes)),
                on_open: Message::Cell(at, CellAct::Open),
                on_hover: Message::Cell(at, CellAct::Hover),
                on_fav: Message::Cell(at, CellAct::Fav),
                on_played: Message::Cell(at, CellAct::Played),
            }));
        }
        row(cells).height(m.row_h).into()
    }

    fn list_row<'a>(&'a self, md: &'a Media, copies: usize, idx: usize, on: bool) -> El<'a> {
        let col = |s: String, w: f32| -> El<'a> {
            container(mono(s).wrapping(text::Wrapping::None))
                .width(w)
                .clip(true)
                .into()
        };
        let rating = md
            .rating
            .map_or(String::new(), |r| format!("\u{2605} {r:.1}"));
        let date = md
            .added
            .as_deref()
            .or(md.premiere.as_deref())
            .map(|d| d.chars().take(10).collect::<String>())
            .unwrap_or_default();
        let mut sub = md.sub.clone().unwrap_or_else(|| md.kind_label());
        if self.several_sources()
            && let Some(mk) = mark_for(&self.catalog, md, copies)
        {
            sub = format!("{sub} \u{b7} {mk}");
        }
        let at = At::Grid(idx);
        let line = row![
            col(format!("0x{:02x}", idx + 1), 48.0),
            container(tile(
                &self.pic(md, Art::Poster, 34),
                &md.title,
                Length::Fixed(34.0),
                Length::Fixed(50.0)
            )),
            column![
                text(md.title.clone())
                    .size(t::BODY)
                    .font(t::SEMIBOLD)
                    .wrapping(text::Wrapping::None)
                    .color(if on { t::TEXT } else { t::TEXT_SECONDARY }),
                mono(sub).wrapping(text::Wrapping::None),
            ]
            .width(Length::Fill)
            .clip(true),
            col(md.year.map(|y| y.to_string()).unwrap_or_default(), 56.0),
            col(md.official_rating.clone().unwrap_or_default(), 64.0),
            col(rating, 72.0),
            col(
                md.runtime_min
                    .map(nocturne_core::source::fmt_runtime)
                    .unwrap_or_default(),
                72.0
            ),
            col(date, 92.0),
            col(
                if md.played {
                    "\u{2713}".into()
                } else if md.favorite {
                    "\u{2665}".into()
                } else {
                    String::new()
                },
                24.0
            ),
        ]
        .spacing(t::S4)
        .align_y(Alignment::Center);
        mouse_area(
            container(line)
                .width(Length::Fill)
                .height(LIST_ROW_H)
                .padding([0.0, t::S4])
                .center_y(LIST_ROW_H)
                .style(move |_| container::Style {
                    background: on.then_some(t::ACCENT_TINT.into()),
                    border: border(t::HAIRLINE, 0.0, 0.0),
                    ..Default::default()
                }),
        )
        .on_enter(Message::Cell(at, CellAct::Hover))
        .on_press(Message::Cell(at, CellAct::Open))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
    }

    fn names_row<'a>(
        &'a self,
        br: &'a Browse,
        r: usize,
        m: &Metrics,
        sel: usize,
        pid: u64,
    ) -> El<'a> {
        let end = ((r + 1) * m.cols).min(br.names.len());
        let mut cells: Vec<El> = vec![];
        for (i, item) in br.names.iter().enumerate().take(end).skip(r * m.cols) {
            let on = i == sel;
            let cell = container(
                row![
                    text(item.label.clone())
                        .size(t::BODY)
                        .font(t::SEMIBOLD)
                        .wrapping(text::Wrapping::None)
                        .color(if on { t::TEXT } else { t::TEXT_SECONDARY }),
                    space().width(Length::Fill),
                    mono(format!("0x{:02x}", i + 1)),
                ]
                .align_y(Alignment::Center),
            )
            .width(m.cell_w)
            .height(NAME_ROW_H)
            .padding([0.0, t::S4])
            .center_y(NAME_ROW_H)
            .style(move |_| container::Style {
                background: on.then_some(t::ACCENT_TINT.into()),
                border: border(t::HAIRLINE, 1.0, 0.0),
                ..Default::default()
            });
            cells.push(
                mouse_area(cell)
                    .on_enter(Message::Cell(At::Grid(i), CellAct::Hover))
                    .on_press(Message::Browse(pid, BMsg::Pick(i)))
                    .interaction(iced::mouse::Interaction::Pointer)
                    .into(),
            );
        }
        row(cells).height(m.row_h).into()
    }
}

fn skeleton<'a>(i: usize, m: &Metrics) -> El<'a> {
    poster_cell(Cell {
        pic: Pic::Loading,
        title: String::new(),
        meta: String::new(),
        index: i,
        w: m.w,
        h: m.h.max(60.0),
        focused: false,
        progress: None,
        badge: None,
        mark: None,
        chips: None,
        on_open: Message::Noop,
        on_hover: Message::Noop,
        on_fav: Message::Noop,
        on_played: Message::Noop,
    })
}

fn list_header<'a>() -> El<'a> {
    let col = |s: &str, w: f32| -> El<'a> { container(mono(s.to_string())).width(w).into() };
    column![
        container(
            row![
                col("#", 48.0),
                space().width(34),
                container(mono("TITLE")).width(Length::Fill),
                col("YEAR", 56.0),
                col("RATED", 64.0),
                col("SCORE", 72.0),
                col("RUNTIME", 72.0),
                col("ADDED", 92.0),
                space().width(24),
            ]
            .spacing(t::S4)
            .align_y(Alignment::Center),
        )
        .height(32)
        .padding([0.0, t::S4])
        .center_y(32),
        rule(),
    ]
    .into()
}

fn note<'a>(s: &str) -> El<'a> {
    container(text(s.to_string()).size(t::BODY).color(t::TEXT_SECONDARY))
        .padding(t::S6)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poster_grid_has_whole_columns_and_a_fixed_row_height() {
        let m = metrics(1280.0, 176.0, ViewKind::Poster, false);
        assert!(m.cols >= 5);
        assert_eq!(m.row_h, cell_h(m.h));
        // The cells exactly fill the panel: cols x cell_w never exceeds the available width.
        assert!(m.cols as f32 * m.cell_w <= 1280.0 - 2.0 * t::PAGE_PAD);
        assert_eq!(metrics(1280.0, 176.0, ViewKind::List, false).cols, 1);
        assert_eq!(
            metrics(1280.0, 176.0, ViewKind::List, false).row_h,
            LIST_ROW_H
        );
        assert_eq!(
            metrics(1280.0, 176.0, ViewKind::Poster, true).row_h,
            NAME_ROW_H
        );
    }

    #[test]
    fn five_thousand_items_build_only_the_rows_in_view() {
        let m = metrics(1280.0, 176.0, ViewKind::Poster, false);
        let rows = lv::rows(5000, m.cols);
        let r = virt::visible_rows(0.0, 600.0, m.row_h, rows, OVERSCAN);
        let built = (r.end - r.start) * m.cols;
        assert!(built < 60, "built {built} cells for 5000 items");
        // Scrolled to the middle the same few rows are built, and their items map back in range.
        let mid = rows as f32 * m.row_h / 2.0;
        let r = virt::visible_rows(mid, 600.0, m.row_h, rows, OVERSCAN);
        assert!(r.len() <= 6);
        let items = virt::items_of(&r, m.cols, 5000);
        assert!(items.start > 1000 && items.end < 4000);
        // Only the pages under those items are wanted.
        let slots: Vec<Option<Media>> = vec![None; 5000];
        let pages = lv::missing_pages(&slots, &HashSet::new(), items);
        assert!(pages.len() <= 2, "{pages:?}");
    }

    #[test]
    fn scopes_and_tab_lists() {
        assert_eq!(scope_global("favourites").base.filters, ["IsFavorite"]);
        assert!(scope_global("collections").base.include_types == ["BoxSet"]);
        let k = UKind::Movies.scope();
        assert_eq!((k.key.as_str(), k.base.recursive), ("all-movies", true));
    }
}
