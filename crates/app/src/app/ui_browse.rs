//! Library / collection / favourites pages: tabs, toolbar (sort, filters, view type), A-Z bar,
//! and a virtualised, lazily paged grid or list. State is `Browse`; logic that has no window
//! lives in `libview.rs`.
use super::*;
use crate::libview::{self as lv, FAct, LibPrefs, Scope, SortKey, Tab, ViewKind};
use nocturne_jellyfin::{BrowseQuery, FilterOptions};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Menu {
    None,
    Sort,
    Filter,
}

pub(super) struct Browse {
    pub scope: Scope,
    pub tabs: Vec<Tab>,
    pub tab: Tab,
    pub prefs: LibPrefs,
    pub letter: Option<char>,
    /// Bumped by every reload; answers carrying an older value are dropped.
    pub epoch: u64,
    pub total: usize,
    pub slots: Vec<Option<Media>>,
    pub pending: HashSet<usize>,
    pub failed: bool,
    pub options: Option<FilterOptions>,
    pub studios: Option<Vec<(String, String)>>,
    pub names: Vec<lv::NameEntry>,
    pub menu: Menu,
    pub msel: usize,
    pub fscroll: ScrollHandle,
}

impl Browse {
    pub fn new(scope: Scope, tabs: Vec<Tab>, prefs: LibPrefs) -> Self {
        Self {
            scope,
            tabs,
            tab: Tab::Library,
            prefs,
            letter: None,
            epoch: 0,
            total: 0,
            slots: vec![],
            pending: HashSet::new(),
            failed: false,
            options: None,
            studios: None,
            names: vec![],
            menu: Menu::None,
            msel: 0,
            fscroll: ScrollHandle::new(),
        }
    }

    /// Entries the selection can land on.
    pub fn len(&self) -> usize {
        if self.tab.is_names() {
            self.names.len()
        } else {
            self.slots.len()
        }
    }

    pub fn item(&self, i: usize) -> Option<&Media> {
        self.slots.get(i)?.as_ref()
    }

    fn query(&self, start: usize) -> BrowseQuery {
        lv::build_query(&self.scope, self.tab, &self.prefs, self.letter, start)
    }

    fn rows(&self) -> Vec<lv::FRow> {
        lv::filter_rows(&self.prefs.filters, self.options.as_ref())
    }

    fn show_letters(&self) -> bool {
        self.prefs.sort == SortKey::Name
            && matches!(
                self.tab,
                Tab::Library | Tab::Favourites | Tab::Collections | Tab::Artists
            )
    }
}

/// Which global page a header link opens.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Global {
    Collections,
    Playlists,
    Favourites,
}

pub(super) const GLOBALS: [(Global, &str); 3] = [
    (Global::Collections, "Collections"),
    (Global::Playlists, "Playlists"),
    (Global::Favourites, "Favourites"),
];

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

pub(super) fn scope_library(m: &Media) -> Scope {
    let types = lv::types_for(m.collection.as_deref());
    Scope {
        key: m.id.clone(),
        title: m.title.clone(),
        subtitle: "Everything in this library.".into(),
        collection: m.collection.clone(),
        base: BrowseQuery {
            parent_id: Some(m.id.clone()),
            include_types: types.clone().map_or(vec![], |t| names(&t)),
            recursive: types.is_some(),
            ..Default::default()
        },
    }
}

/// A folder or photo album: its direct children, in a grid.
pub(super) fn scope_folder(m: &Media) -> Scope {
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

pub(super) fn scope_global(g: Global) -> Scope {
    let (key, title, sub, types, fav): (_, _, _, &[&str], _) = match g {
        Global::Collections => (
            "collections",
            "Collections",
            "Movies and shows grouped together.",
            &["BoxSet"],
            false,
        ),
        Global::Playlists => (
            "playlists",
            "Playlists",
            "Your lists. Press N to start a new one.",
            &["Playlist"],
            false,
        ),
        Global::Favourites => (
            "favourites",
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
    };
    Scope {
        key: key.into(),
        title: title.into(),
        subtitle: sub.into(),
        collection: None,
        base: BrowseQuery {
            include_types: names(types),
            filters: if fav { names(&["IsFavorite"]) } else { vec![] },
            recursive: true,
            ..Default::default()
        },
    }
}

/// Titles under a genre or studio, server-wide.
pub(super) fn scope_tag(kind: &str, key: &str, label: &str) -> Scope {
    let mut base = BrowseQuery {
        include_types: names(&["Movie", "Series"]),
        recursive: true,
        ..Default::default()
    };
    match kind {
        "studio" => base.studio_ids = vec![key.into()],
        "music" => {
            base.include_types = names(&["MusicAlbum"]);
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

pub(super) fn caption(m: &Media) -> String {
    let second = match m.kind {
        Kind::Episode => m.episode_label(),
        Kind::MusicAlbum | Kind::Audio => m.sub.clone().unwrap_or_default(),
        _ => m.kind_label(),
    };
    [second, m.meta_line()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Unwatched count badge for shows and seasons.
pub(super) fn badge(m: &Media) -> Option<String> {
    m.unplayed.filter(|n| *n > 0).map(|n| n.to_string())
}

impl Root {
    // ---- opening ----

    pub(super) fn open_browse(&mut self, scope: Scope, cx: &mut Context<Self>) {
        let tabs = if scope.collection.is_some() {
            lv::tabs_for(scope.collection.as_deref())
        } else {
            vec![]
        };
        self.open_browse_with(scope, tabs, cx);
    }

    fn open_browse_with(&mut self, scope: Scope, tabs: Vec<Tab>, cx: &mut Context<Self>) {
        let prefs = self
            .settings
            .libs
            .get(&scope.key)
            .cloned()
            .unwrap_or_default();
        let media = (!scope.key.is_empty()).then(|| Media {
            id: scope.key.clone(),
            title: scope.title.clone(),
            collection: scope.collection.clone(),
            kind: Kind::Library,
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        });
        let pid = self.push_page(View::Grid, media);
        if let Some(p) = self.page_mut(pid) {
            p.br = Some(Box::new(Browse::new(scope, tabs, prefs)));
        }
        self.browse_reload(pid, cx);
    }

    pub(super) fn open_global(&mut self, g: Global, cx: &mut Context<Self>) {
        self.close_admin(cx);
        self.stack.truncate(1);
        self.fwd.clear();
        self.open_browse(scope_global(g), cx);
    }

    // ---- loading ----

    fn br_mut(&mut self, pid: u64) -> Option<&mut Browse> {
        self.page_mut(pid)?.br.as_deref_mut()
    }

    /// Start over with the current tab/sort/filters/letter.
    pub(super) fn browse_reload(&mut self, pid: u64, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let Some(p) = self.page_mut(pid) else { return };
        p.sel = 0;
        p.scroll.scroll_to_item(0, ScrollStrategy::Top);
        let Some(br) = p.br.as_deref_mut() else {
            return;
        };
        br.epoch += 1;
        let epoch = br.epoch;
        br.slots.clear();
        br.total = 0;
        br.pending.clear();
        br.failed = false;
        if br.tab.is_names() {
            return self.ensure_options(pid, cx);
        }
        let q = br.query(0);
        br.pending.insert(0);
        self.run(cx, async move { src.browse(&q).await }, move |t, r, _| {
            let Some(br) = t.br_mut(pid).filter(|b| b.epoch == epoch) else {
                return;
            };
            br.pending.remove(&0);
            match r {
                Ok((items, total)) => {
                    br.total = total;
                    br.slots = vec![None; total];
                    lv::fill(&mut br.slots, 0, items);
                }
                Err(_) => br.failed = true,
            }
        });
    }

    fn browse_page(&mut self, pid: u64, page: usize, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let Some(br) = self.br_mut(pid) else { return };
        br.pending.insert(page); // stays set on failure: no retry storm while scrolling
        let (epoch, q) = (br.epoch, br.query(page * lv::PAGE));
        self.run(cx, async move { src.browse(&q).await }, move |t, r, _| {
            if let (Some(br), Ok((items, _))) = (t.br_mut(pid).filter(|b| b.epoch == epoch), r) {
                lv::fill(&mut br.slots, page * lv::PAGE, items);
                br.pending.remove(&page);
            }
        });
    }

    /// Fetch whatever pages the rows on screen (plus one row of lookahead) still lack.
    fn browse_visible(
        &mut self,
        rows: std::ops::Range<usize>,
        cols: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self.stack.last() else { return };
        let Some(br) = p.br.as_deref() else { return };
        let (a, b) = (rows.start.saturating_sub(1) * cols, (rows.end + 1) * cols);
        let pid = p.pid;
        for page in lv::missing_pages(&br.slots, &br.pending, a..b) {
            self.browse_page(pid, page, cx);
        }
    }

    /// Filter options and studios, once per page (name tabs and the Filters panel need them).
    fn ensure_options(&mut self, pid: u64, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let Some(br) = self.br_mut(pid) else { return };
        if let Some(o) = &br.options {
            br.names = lv::name_entries(br.tab, o, br.studios.as_deref().unwrap_or(&[]));
            return;
        }
        let parent = br.scope.base.parent_id.clone();
        let types = br.scope.base.include_types.clone();
        self.run(
            cx,
            async move {
                let o = src.filter_options(parent.as_deref(), &types).await;
                let s = src.studios(parent.as_deref()).await;
                (o, s)
            },
            move |t, (o, s), _| {
                if let Some(br) = t.br_mut(pid) {
                    br.options = Some(o.unwrap_or_default());
                    br.studios = Some(s.unwrap_or_default());
                    br.names = lv::name_entries(
                        br.tab,
                        br.options.as_ref().unwrap(),
                        br.studios.as_deref().unwrap_or(&[]),
                    );
                }
            },
        );
    }

    fn save_prefs(&mut self, pid: u64) {
        if let Some(br) = self.page_mut(pid).and_then(|p| p.br.as_deref()) {
            let (k, v) = (br.scope.key.clone(), br.prefs.clone());
            self.settings.libs.insert(k, v);
            self.settings.save();
        }
    }

    // ---- toolbar actions ----

    fn browse_tab(&mut self, pid: u64, tab: Tab, cx: &mut Context<Self>) {
        if let Some(br) = self.br_mut(pid) {
            br.tab = tab;
            br.letter = None;
            br.menu = Menu::None;
        }
        self.browse_reload(pid, cx);
    }

    fn browse_sort(&mut self, pid: u64, key: SortKey, cx: &mut Context<Self>) {
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
        self.browse_reload(pid, cx);
    }

    fn browse_view(&mut self, pid: u64, v: ViewKind, cx: &mut Context<Self>) {
        if let Some(br) = self.br_mut(pid) {
            br.prefs.view = v;
        }
        self.save_prefs(pid);
        cx.notify();
    }

    fn browse_letter(&mut self, pid: u64, c: Option<char>, cx: &mut Context<Self>) {
        if let Some(br) = self.br_mut(pid) {
            br.letter = c;
        }
        self.browse_reload(pid, cx);
    }

    fn browse_filter(&mut self, pid: u64, act: FAct, cx: &mut Context<Self>) {
        if let Some(br) = self.br_mut(pid) {
            lv::toggle(&mut br.prefs.filters, &act);
        }
        self.save_prefs(pid);
        self.browse_reload(pid, cx);
    }

    fn browse_menu(&mut self, pid: u64, m: Menu, cx: &mut Context<Self>) {
        if let Some(br) = self.br_mut(pid) {
            br.menu = if br.menu == m { Menu::None } else { m };
            br.msel = match br.menu {
                Menu::Sort => SortKey::ALL
                    .iter()
                    .position(|k| *k == br.prefs.sort)
                    .unwrap_or(0),
                _ => 0,
            };
        }
        if m == Menu::Filter {
            self.ensure_options(pid, cx);
            // ensure_options may have replaced `names` for a name tab; harmless for the panel.
        }
        cx.notify();
    }

    fn browse_pick(&mut self, pid: u64, i: usize, cx: &mut Context<Self>) {
        if let Some(br) = self.br_mut(pid)
            && let Some(e) = br.names.get(i).cloned()
        {
            e.pick.apply(&mut br.prefs.filters);
            br.tab = Tab::Library;
        } else {
            return;
        }
        self.browse_reload(pid, cx);
    }

    /// Keys for a browse page. True when consumed.
    pub(super) fn browse_key(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) -> bool {
        let Some(p) = self.stack.last() else {
            return false;
        };
        let (pid, cols) = (p.pid, self.cols);
        let Some(br) = p.br.as_deref() else {
            return false;
        };
        if br.menu != Menu::None {
            let rows = br.rows();
            let n = if br.menu == Menu::Sort {
                SortKey::ALL.len()
            } else {
                rows.len()
            };
            let (menu, sel) = (br.menu, br.msel);
            // Next selectable row in direction d (headings have no action).
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
                "escape" | "o" | "l" => {
                    self.browse_menu(pid, Menu::None, cx);
                    return true;
                }
                "enter" | "space" => {
                    if menu == Menu::Sort {
                        self.browse_sort(pid, SortKey::ALL[sel.min(n - 1)], cx);
                    } else if let Some(a) = rows.get(sel).and_then(|r| r.act.clone()) {
                        self.browse_filter(pid, a, cx);
                    }
                    return true;
                }
                _ => return true, // modal: swallow everything else
            };
            if let Some(br) = self.br_mut(pid) {
                br.msel = i;
                br.fscroll.scroll_to_item(i);
            }
            return true;
        }
        match key {
            "n" if br.scope.key == "playlists" => {
                self.playlist_dialog(
                    crate::account::DlgKind::NewPlaylist,
                    crate::account::Target::None,
                    cx,
                );
            }
            "o" => self.browse_menu(pid, Menu::Sort, cx),
            "l" => self.browse_menu(pid, Menu::Filter, cx),
            "v" => {
                let all = ViewKind::ALL;
                let cur = all.iter().position(|v| *v == br.prefs.view).unwrap_or(0);
                self.browse_view(pid, all[(cur + 1) % all.len()], cx);
            }
            "tab" if br.tabs.len() > 1 => {
                let cur = br.tabs.iter().position(|t| *t == br.tab).unwrap_or(0) as i32;
                let n = br.tabs.len() as i32;
                let next = br.tabs[(cur + if shift { n - 1 } else { 1 }).rem_euclid(n) as usize];
                self.browse_tab(pid, next, cx);
            }
            "[" | "]" if br.show_letters() => {
                let c = lv::step_letter(br.letter, if key == "]" { 1 } else { -1 });
                self.browse_letter(pid, c, cx);
            }
            "r" if br.tab == Tab::Random || br.prefs.sort == SortKey::Random => {
                self.browse_reload(pid, cx)
            }
            "up" | "down" | "left" | "right" | "enter" => {
                let n = br.len();
                let dir = match key {
                    "up" => Some(Dir::Up),
                    "down" => Some(Dir::Down),
                    "left" => Some(Dir::Left),
                    "right" => Some(Dir::Right),
                    _ => None,
                };
                let names = br.tab.is_names();
                let sel = p.sel;
                if let Some(d) = dir {
                    let list = br.prefs.view == ViewKind::List && !names;
                    let c = if list { 1 } else { cols };
                    let Some(p) = self.stack.last_mut() else {
                        return true;
                    };
                    p.sel = nav::step(p.sel, c, n, d);
                    p.scroll
                        .scroll_to_item(p.sel / c.max(1), ScrollStrategy::Center);
                } else if names {
                    self.browse_pick(pid, sel, cx);
                } else if let Some(m) = br.item(sel).cloned() {
                    self.open_any(m, cx);
                }
            }
            _ => return false,
        }
        true
    }

    // ---- render ----

    pub(super) fn grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.stack.last() else {
            return div().into_any_element();
        };
        let Some(br) = p.br.as_deref() else {
            return div().into_any_element();
        };
        let pid = p.pid;
        let (view, names_tab) = (br.prefs.view, br.tab.is_names());
        let avail = self.colw() - 2.0 * t::PAGE_PAD - 1.0;
        let k = self.settings.density.card() / 176.0;
        let (card, ratio) = match view {
            ViewKind::Poster => (self.settings.density.card(), 1.5),
            ViewKind::Landscape => (300.0 * k, 9.0 / 16.0),
            ViewKind::List => (avail, 0.0),
        };
        let cols = match (view, names_tab) {
            (_, true) => nav::columns(avail, 260.0, 0.0),
            (ViewKind::List, _) => 1,
            _ => nav::columns(avail, card + 2.0 * t::S3, 0.0),
        };
        self.cols = cols;
        let cell_w = (avail / cols as f32).floor();
        let (w, h) = (
            cell_w - 2.0 * t::S3 - 1.0,
            ((cell_w - 2.0 * t::S3 - 1.0) * ratio).round(),
        );
        let n = br.len();
        // An empty answer must not look like "still loading": epoch 0 = never requested.
        let loading = br.slots.is_empty()
            && !br.failed
            && !names_tab
            && (br.epoch == 0 || br.pending.contains(&0));
        let count = if br.failed {
            "unavailable".to_string()
        } else if names_tab {
            format!("{} entries", lv::thousands(n))
        } else if loading {
            "loading".to_string()
        } else {
            format!("{} items", lv::thousands(br.total))
        };
        let toolbar = self.browse_toolbar(pid, count, cx);
        let br = self.stack.last().and_then(|p| p.br.as_deref()).unwrap();
        let (title, sub) = (br.scope.title.clone(), br.scope.subtitle.clone());
        let tabs = br.tabs.clone();
        let (cur_tab, coll) = (br.tab, br.scope.collection.clone());
        let body: AnyElement = if br.failed {
            self.failed_view("This couldn't be loaded.", cx)
                .into_any_element()
        } else if loading {
            div()
                .flex()
                .children((0..cols.min(8)).map(|i| {
                    ui::poster_cell(Cell {
                        key: &format!("gsk-{i}"),
                        pic: Pic::Loading,
                        title: "",
                        meta: String::new(),
                        index: i,
                        w,
                        h: h.max(60.0),
                        focused: false,
                        progress: None,
                        badge: None,
                    })
                }))
                .into_any_element()
        } else if n == 0 {
            Self::note_text(if names_tab && br.options.is_none() {
                "Loading"
            } else if br.prefs.filters.active() > 0 || br.letter.is_some() {
                "Nothing matches. Try clearing a filter."
            } else {
                "Nothing here yet."
            })
            .into_any_element()
        } else if names_tab {
            let rows = lv::rows(n, cols);
            uniform_list(
                "names",
                rows,
                cx.processor(move |this, range: std::ops::Range<usize>, _w, cx| {
                    range
                        .map(|r| this.names_row(r, cols, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(p_scroll(self))
            .size_full()
            .into_any_element()
        } else {
            let (row_h, list) = (
                if view == ViewKind::List {
                    LIST_ROW_H
                } else {
                    h + CELL_EXTRA
                },
                view == ViewKind::List,
            );
            let grid = uniform_list(
                "grid",
                lv::rows(n, cols),
                cx.processor(move |this, range: std::ops::Range<usize>, _w, cx| {
                    this.browse_visible(range.clone(), cols, cx);
                    range
                        .map(|r| this.browse_row(r, cols, (w, h), row_h, view, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(p_scroll(self))
            .size_full();
            if list {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(list_header())
                    .child(div().flex_1().min_h_0().child(grid))
                    .into_any_element()
            } else {
                grid.into_any_element()
            }
        };
        let tab_row = (tabs.len() > 1).then(|| {
            let mut r = div()
                .px(px(t::PAGE_PAD))
                .flex()
                .gap(px(t::S1))
                .border_b_1()
                .border_color(t::hairline());
            for (i, tb) in tabs.iter().enumerate() {
                let tb = *tb;
                r = r.child(ui::tab(
                    format!("tab-{i}"),
                    tb.label(coll.as_deref()),
                    tb == cur_tab,
                    cx.listener(move |t, _, _, cx| t.browse_tab(pid, tb, cx)),
                ));
            }
            r
        });
        div()
            .size_full()
            .pt(px(t::HEADER_H + t::S8))
            .flex()
            .flex_col()
            .child(ui::section(&title, &sub, String::new()))
            .child(div().h(px(t::S4)).flex_none())
            .children(tab_row)
            .child(toolbar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .mx(px(t::PAGE_PAD))
                    .mb(px(t::S6))
                    .border_t_1()
                    .border_l_1()
                    .border_color(t::hairline())
                    .rounded(px(t::R_PANEL))
                    .overflow_hidden()
                    .child(body),
            )
            .into_any_element()
    }

    fn browse_toolbar(&mut self, pid: u64, count: String, cx: &mut Context<Self>) -> Div {
        let Some(br) = self.stack.last().and_then(|p| p.br.as_deref()) else {
            return div();
        };
        let (sort, desc, view, menu) = (br.prefs.sort, br.prefs.desc, br.prefs.view, br.menu);
        let fixed = matches!(br.tab, Tab::Recent | Tab::Random);
        let nfilters = br.prefs.filters.active();
        let letters = br.show_letters().then_some(br.letter);
        let rows = (menu == Menu::Filter).then(|| br.rows());
        let (msel, fscroll) = (br.msel, br.fscroll.clone());
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
        let br_names = br.tab.is_names();
        let mut bar = div()
            .relative()
            .px(px(t::PAGE_PAD))
            .py(px(t::S3))
            .flex()
            .flex_col()
            .gap(px(t::S3))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(t::S3))
                    // The entry lists (genres, studios, years) have no sort, filter or view type.
                    .when(!br_names, |d| {
                        d.child(
                            ui::action_button(
                                "tb-sort".into(),
                                &format!("Sort: {sort_label}"),
                                false,
                                false,
                                menu == Menu::Sort,
                                fixed,
                                cx.listener(move |t, _, _, cx| {
                                    if !fixed {
                                        t.browse_menu(pid, Menu::Sort, cx)
                                    }
                                }),
                            )
                            .tooltip(ui::tip("Sort order (O)")),
                        )
                        .child(
                            ui::action_button(
                                "tb-filter".into(),
                                &if nfilters > 0 {
                                    format!("Filters ({nfilters})")
                                } else {
                                    "Filters".to_string()
                                },
                                nfilters > 0,
                                false,
                                menu == Menu::Filter,
                                false,
                                cx.listener(move |t, _, _, cx| {
                                    t.browse_menu(pid, Menu::Filter, cx)
                                }),
                            )
                            .tooltip(ui::tip("Filter (L)")),
                        )
                        .child({
                            let mut seg = div().flex().gap(px(t::S1));
                            for v in ViewKind::ALL {
                                seg = seg.child(
                                    ui::action_button(
                                        format!("tb-view-{}", v.label()),
                                        v.label(),
                                        v == view,
                                        false,
                                        false,
                                        false,
                                        cx.listener(move |t, _, _, cx| t.browse_view(pid, v, cx)),
                                    )
                                    .tooltip(ui::tip("View type (V)")),
                                );
                            }
                            seg
                        })
                    })
                    .child(div().flex_1())
                    .child(ui::mono(count)),
            );
        if let Some(cur) = letters {
            let mut az = div().flex().flex_wrap().gap(px(2.0));
            for (i, c) in lv::letters().into_iter().enumerate() {
                let on = cur == Some(c);
                az = az.child(
                    div()
                        .id(("az", i))
                        .w(px(24.0))
                        .h(px(22.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(t::R_XS))
                        .font_family(t::MONO)
                        .text_size(px(t::MICRO + 1.0))
                        .cursor_pointer()
                        .text_color(if on {
                            t::on_accent()
                        } else {
                            t::text_secondary()
                        })
                        .when(on, |d| d.bg(t::accent()))
                        .hover(|s| s.bg(t::glass()))
                        .on_click(cx.listener(move |t, _, _, cx| {
                            t.browse_letter(pid, if on { None } else { Some(c) }, cx)
                        }))
                        .child(c.to_string()),
                );
            }
            bar = bar.child(az);
        }
        if menu == Menu::Sort {
            let mut list = panel("sort-menu")
                .left(px(t::PAGE_PAD))
                .top(px(46.0))
                .w(px(240.0));
            for (i, k) in SortKey::ALL.into_iter().enumerate() {
                let (on, cur) = (i == msel, k == sort);
                list = list.child(
                    menu_row(("sort-opt", i), on)
                        .on_click(cx.listener(move |t, _, _, cx| t.browse_sort(pid, k, cx)))
                        .child(
                            div()
                                .text_color(if cur { t::accent_text() } else { t::text() })
                                .child(k.label()),
                        )
                        .child(ui::mono(if cur && k != SortKey::Random {
                            if desc { "\u{2193}" } else { "\u{2191}" }
                        } else {
                            ""
                        })),
                );
            }
            // Deferred: the menu must paint over the A-Z strip and the grid below the toolbar.
            bar = bar
                .child(deferred(away(pid, cx)).with_priority(1))
                .child(deferred(list).with_priority(2));
        }
        if let Some(rows) = rows {
            let mut list = panel("filter-menu")
                .right(px(t::PAGE_PAD))
                .top(px(46.0))
                .w(px(300.0))
                .max_h(px(420.0))
                .overflow_y_scroll()
                .track_scroll(&fscroll);
            for (i, r) in rows.into_iter().enumerate() {
                let Some(act) = r.act.clone() else {
                    list = list.child(
                        div()
                            .px(px(t::S3))
                            .pt(px(t::S3))
                            .pb(px(t::S1))
                            .child(ui::label(&r.label)),
                    );
                    continue;
                };
                list = list.child(
                    menu_row(("filter-opt", i), i == msel)
                        .on_click(
                            cx.listener(move |t, _, _, cx| t.browse_filter(pid, act.clone(), cx)),
                        )
                        .child(
                            div()
                                .text_color(if r.on { t::accent_text() } else { t::text() })
                                .child(r.label),
                        )
                        .child(ui::mono(if r.on { "\u{2713}" } else { "" })),
                );
            }
            // Deferred: the menu must paint over the A-Z strip and the grid below the toolbar.
            bar = bar
                .child(deferred(away(pid, cx)).with_priority(1))
                .child(deferred(list).with_priority(2));
        }
        bar
    }

    fn browse_row(
        &mut self,
        r: usize,
        cols: usize,
        (w, h): (f32, f32),
        row_h: f32,
        view: ViewKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(br) = self.stack.last().and_then(|p| p.br.as_deref()) else {
            return div().into_any_element();
        };
        let sel = self.stack.last().map_or(0, |p| p.sel);
        let end = ((r + 1) * cols).min(br.slots.len());
        let items: Vec<(usize, Option<Media>)> =
            (r * cols..end).map(|i| (i, br.item(i).cloned())).collect();
        let mut row = div().w_full().h(px(row_h)).flex();
        for (idx, m) in items {
            let hover = cx.listener(move |t, hov: &bool, _, cx| {
                if *hov
                    && let Some(p) = t.stack.last_mut()
                    && p.sel != idx
                {
                    p.sel = idx;
                    cx.notify();
                }
            });
            let Some(m) = m else {
                row = row.child(ui::poster_cell(Cell {
                    key: &format!("sk-{idx}"),
                    pic: Pic::Loading,
                    title: "",
                    meta: String::new(),
                    index: idx,
                    w,
                    h: h.max(60.0),
                    focused: false,
                    progress: None,
                    badge: None,
                }));
                continue;
            };
            let target = m.clone();
            if view == ViewKind::List {
                row = row.child(self.list_row(&m, idx, idx == sel, hover, target, cx));
                continue;
            }
            let art = if view == ViewKind::Landscape {
                Art::Landscape
            } else {
                Art::Poster
            };
            let pic = self.pic(&m, art, (w * 2.0) as u32, cx);
            row = row.child(
                ui::poster_cell(Cell {
                    key: &format!("g-{}", m.id),
                    pic,
                    title: &m.title,
                    meta: caption(&m),
                    index: idx,
                    w,
                    h,
                    focused: idx == sel,
                    progress: m.progress(),
                    badge: badge(&m),
                })
                .group("card")
                .child(self.chips(&m, cx))
                .on_hover(hover)
                .on_click(cx.listener(move |t, _, _, cx| {
                    t.open_any(target.clone(), cx);
                    cx.notify();
                })),
            );
        }
        row.into_any_element()
    }

    fn list_row(
        &mut self,
        m: &Media,
        idx: usize,
        on: bool,
        hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
        target: Media,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pic = self.pic(m, Art::Poster, 120, cx);
        let col = |s: String, w: f32| div().w(px(w)).flex_none().truncate().child(ui::mono(s));
        let rating = m
            .rating
            .map_or(String::new(), |r| format!("\u{2605} {r:.1}"));
        let date = m
            .added
            .as_deref()
            .or(m.premiere.as_deref())
            .map(|d| d.chars().take(10).collect::<String>())
            .unwrap_or_default();
        div()
            .id(ui::name(format!("lr-{}", m.id)))
            .w_full()
            .h(px(LIST_ROW_H))
            .px(px(t::S4))
            .flex()
            .items_center()
            .gap(px(t::S4))
            .border_b_1()
            .border_r_1()
            .border_color(t::hairline())
            .bg(if on { t::accent_tint() } else { rgba(0) })
            .cursor_pointer()
            .hover(|s| s.bg(t::glass()))
            .on_hover(hover)
            .on_click(cx.listener(move |t, _, _, cx| {
                t.open_any(target.clone(), cx);
                cx.notify();
            }))
            .child(col(format!("0x{:02x}", idx + 1), 48.0))
            .child(
                ui::tile(pic, &m.title, &format!("l-{}", m.id))
                    .w(px(34.0))
                    .h(px(50.0))
                    .flex_none(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(t::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if on { t::text() } else { t::text_secondary() })
                            .child(m.title.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .child(ui::mono(m.sub.clone().unwrap_or_else(|| m.kind_label()))),
                    ),
            )
            .child(col(m.year.map(|y| y.to_string()).unwrap_or_default(), 56.0))
            .child(col(m.official_rating.clone().unwrap_or_default(), 64.0))
            .child(col(rating, 72.0))
            .child(col(
                m.runtime_min
                    .map(crate::source::fmt_runtime)
                    .unwrap_or_default(),
                72.0,
            ))
            .child(col(date, 92.0))
            .child(col(
                if m.played {
                    "\u{2713}".into()
                } else if m.favorite {
                    "\u{2665}".into()
                } else {
                    String::new()
                },
                24.0,
            ))
            .into_any_element()
    }

    fn names_row(&mut self, r: usize, cols: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.stack.last() else {
            return div().into_any_element();
        };
        let (pid, sel) = (p.pid, p.sel);
        let Some(br) = p.br.as_deref() else {
            return div().into_any_element();
        };
        let end = ((r + 1) * cols).min(br.names.len());
        let mut row = div().h(px(NAME_ROW_H)).flex();
        for (i, item) in br.names.iter().enumerate().take(end).skip(r * cols) {
            let on = i == sel;
            row = row.child(
                div()
                    .id(("name", i))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px(px(t::S4))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(t::S3))
                    .border_r_1()
                    .border_b_1()
                    .border_color(t::hairline())
                    .bg(if on { t::accent_tint() } else { rgba(0) })
                    .cursor_pointer()
                    .hover(|s| s.bg(t::glass()))
                    .on_hover(cx.listener(move |t, hov: &bool, _, cx| {
                        if *hov
                            && let Some(p) = t.stack.last_mut()
                            && p.sel != i
                        {
                            p.sel = i;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |t, _, _, cx| t.browse_pick(pid, i, cx)))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(t::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if on { t::text() } else { t::text_secondary() })
                            .child(item.label.clone()),
                    )
                    .child(ui::mono(format!("0x{:02x}", i + 1))),
            );
        }
        row.into_any_element()
    }
}

const LIST_ROW_H: f32 = 64.0;
const NAME_ROW_H: f32 = 56.0;

fn p_scroll(r: &Root) -> UniformListScrollHandle {
    r.stack.last().map(|p| p.scroll.clone()).unwrap_or_default()
}

fn list_header() -> Div {
    let col = |s: &str, w: f32| div().w(px(w)).flex_none().child(ui::mono(s.to_string()));
    div()
        .w_full()
        .h(px(32.0))
        .px(px(t::S4))
        .flex()
        .items_center()
        .gap(px(t::S4))
        .border_b_1()
        .border_color(t::hairline_strong())
        .child(col("#", 48.0))
        .child(div().w(px(34.0)).flex_none())
        .child(div().flex_1().child(ui::mono("TITLE")))
        .child(col("YEAR", 56.0))
        .child(col("RATED", 64.0))
        .child(col("SCORE", 72.0))
        .child(col("RUNTIME", 72.0))
        .child(col("ADDED", 92.0))
        .child(div().w(px(24.0)).flex_none())
}

fn panel(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .p(px(t::S2))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .rounded(px(t::R_PANEL))
        .border_1()
        .border_color(t::hairline_strong())
        .bg(t::surface())
        .occlude()
}

fn menu_row(id: (&'static str, usize), on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(t::S3))
        .py(px(t::S2))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(t::S3))
        .rounded(px(6.0))
        .text_size(px(t::BODY))
        .cursor_pointer()
        .when(on, |d| d.bg(t::accent_tint()))
        .hover(|s| s.bg(t::glass()))
}

/// Click anywhere else closes the open toolbar menu.
fn away(pid: u64, cx: &mut Context<Root>) -> Stateful<Div> {
    div()
        .id("tb-away")
        .absolute()
        .top(px(-3000.0))
        .left(px(-3000.0))
        .w(px(9000.0))
        .h(px(9000.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |t, _, _, cx| t.browse_menu(pid, Menu::None, cx)),
        )
}
