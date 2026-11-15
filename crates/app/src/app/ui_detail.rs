//! Item detail pages: movies, shows, seasons, episodes, collections, playlists, people and
//! albums. One page, a list of focusable rows (actions, chips, episode/track list, carousels);
//! the data comes from `detail.rs`.
use super::ui_browse::{caption, scope_tag};
use super::*;
use crate::detail::{self, Full};
use crate::sync::NEED_WRITES;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Sec {
    Cast,
    Chapters,
    Similar,
    Extras,
    Kids,
    Kids2,
}

impl Sec {
    const ALL: [Sec; 6] = [
        Sec::Kids,
        Sec::Kids2,
        Sec::Chapters,
        Sec::Cast,
        Sec::Extras,
        Sec::Similar,
    ];

    fn idx(self) -> usize {
        match self {
            Sec::Cast => 0,
            Sec::Chapters => 1,
            Sec::Similar => 2,
            Sec::Extras => 3,
            Sec::Kids => 4,
            Sec::Kids2 => 5,
        }
    }

    fn title(self, kind: Kind) -> (&'static str, &'static str) {
        match (self, kind) {
            (Sec::Cast, _) => ("Cast & crew", "Select a person for their filmography."),
            (Sec::Chapters, _) => ("Chapters", "Jump straight to a scene."),
            (Sec::Similar, _) => (
                "More like this",
                "Chosen by the server from what this resembles.",
            ),
            (Sec::Extras, _) => ("Extras", "Trailers and special features."),
            (Sec::Kids, Kind::Person) => ("Movies", "Films with this person."),
            (Sec::Kids2, Kind::Person) => ("Shows", "Series with this person."),
            _ => ("Related", ""),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DRow {
    Actions,
    Chips,
    Seasons,
    List,
    Strip(Sec),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DAct {
    Play,
    PlayAll,
    Shuffle,
    Watched,
    Fav,
    Prev,
    Next,
    Series,
}

#[derive(Clone)]
pub(super) enum Chip {
    Genre(String),
    Studio(String, String),
    Link(String, String),
}

/// Buttons a page offers, by kind.
pub(super) fn actions(m: &Media, has_prev: bool, has_next: bool) -> Vec<DAct> {
    use DAct::*;
    let mut v = match m.kind {
        Kind::Movie | Kind::Video | Kind::Audio => vec![Play, Watched, Fav],
        Kind::Episode => {
            let mut v = vec![Play, Watched, Fav];
            if has_prev {
                v.push(Prev);
            }
            if has_next {
                v.push(Next);
            }
            if m.series_id.is_some() {
                v.push(Series);
            }
            v
        }
        Kind::Series => vec![Play, Watched, Fav],
        Kind::Season => {
            let mut v = vec![Play, Watched];
            if m.series_id.is_some() {
                v.push(Series);
            }
            v
        }
        Kind::BoxSet | Kind::Playlist | Kind::MusicAlbum => vec![PlayAll, Shuffle, Fav],
        _ => vec![Fav],
    };
    if m.kind == Kind::Series || m.kind == Kind::Season {
        // Watched toggles on a whole show/season are not offered: the server flags episodes.
        v.retain(|a| *a != Watched);
    }
    v
}

fn spec(a: DAct, m: &Media) -> (&'static str, &'static str, bool) {
    match a {
        DAct::Play if m.progress().is_some() => ("d-play", "Resume", true),
        DAct::Play => ("d-play", "Play", true),
        DAct::PlayAll => ("d-all", "Play all", true),
        DAct::Shuffle => ("d-shuffle", "Shuffle", false),
        DAct::Watched if m.played => ("d-seen", "Watched \u{2713}", false),
        DAct::Watched => ("d-seen", "Mark watched", false),
        DAct::Fav if m.favorite => ("d-fav", "\u{2665} Favourite", false),
        DAct::Fav => ("d-fav", "\u{2661} Favourite", false),
        DAct::Prev => ("d-prev", "\u{2039} Previous", false),
        DAct::Next => ("d-next", "Next \u{203A}", false),
        DAct::Series => ("d-series", "Go to series", false),
    }
}

/// Order for shuffling a queue; xorshift is plenty for picking tracks.
pub(super) fn shuffled(mut v: Vec<Media>) -> Vec<Media> {
    let mut s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9, |d| d.as_nanos() as u64)
        | 1;
    for i in (1..v.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        v.swap(i, (s % (i as u64 + 1)) as usize);
    }
    v
}

impl Page {
    pub(super) fn sec_items(&self, s: Sec) -> &[Media] {
        match s {
            Sec::Cast => self.full.as_ref().map_or(&[], |f| &f.people),
            Sec::Chapters => self.full.as_ref().map_or(&[], |f| &f.chapters),
            Sec::Similar => self.similar.items(),
            Sec::Extras => self.extras.items(),
            Sec::Kids => self.kids.items(),
            Sec::Kids2 => self.kids2.items(),
        }
    }

    /// Episodes are shown as a card grid; other lists as rows.
    fn card_grid(&self) -> bool {
        self.media
            .as_ref()
            .is_some_and(|m| matches!(m.kind, Kind::Series | Kind::Season))
    }

    fn has_list(&self) -> bool {
        self.media.as_ref().is_some_and(|m| {
            matches!(
                m.kind,
                Kind::Series
                    | Kind::Season
                    | Kind::BoxSet
                    | Kind::Playlist
                    | Kind::MusicAlbum
                    | Kind::MusicArtist
            )
        })
    }

    pub(super) fn chips_list(&self) -> Vec<Chip> {
        let Some(m) = &self.media else { return vec![] };
        let mut v: Vec<Chip> = m.genres.iter().cloned().map(Chip::Genre).collect();
        if let Some(f) = &self.full {
            v.extend(
                f.studios
                    .iter()
                    .map(|(i, n)| Chip::Studio(i.clone(), n.clone())),
            );
            v.extend(
                f.links
                    .iter()
                    .map(|(n, u)| Chip::Link(n.clone(), u.clone())),
            );
        }
        v
    }

    /// Focusable rows, top to bottom.
    pub(super) fn drows(&self) -> Vec<DRow> {
        let mut v = vec![DRow::Actions];
        if !self.chips_list().is_empty() {
            v.push(DRow::Chips);
        }
        if self.media.as_ref().is_some_and(|m| m.is_series()) && !self.seasons.items().is_empty() {
            v.push(DRow::Seasons);
        }
        if self.has_list() && !self.episodes.items().is_empty() {
            v.push(DRow::List);
        }
        v.extend(
            Sec::ALL
                .into_iter()
                .filter(|s| !self.sec_items(*s).is_empty())
                .map(DRow::Strip),
        );
        v
    }

    fn dacts(&self) -> Vec<DAct> {
        self.media.as_ref().map_or(vec![], |m| {
            actions(m, self.neigh.0.is_some(), self.neigh.1.is_some())
        })
    }

    fn drow_len(&self, r: DRow) -> usize {
        match r {
            DRow::Actions => self.dacts().len(),
            DRow::Chips => self.chips_list().len(),
            DRow::Seasons => self.seasons.items().len(),
            DRow::List => self.episodes.items().len(),
            DRow::Strip(s) => self.sec_items(s).len(),
        }
    }

    /// Index among the page's direct children (for scrolling the focused row into view).
    fn child_of(&self, r: DRow) -> usize {
        const BEFORE: usize = 5; // hero, facts, chips, about, list heading
        let n = self.episodes.items().len();
        let list_n = if !self.has_list() {
            0
        } else if self.card_grid() {
            lv_rows(n, self.dcols)
        } else {
            n
        };
        match r {
            DRow::Actions => 0,
            DRow::Chips => 2,
            DRow::Seasons => 4,
            DRow::List if self.card_grid() => BEFORE + self.ep_sel / self.dcols.max(1),
            DRow::List => BEFORE + self.ep_sel,
            DRow::Strip(s) => {
                let before = self
                    .drows()
                    .iter()
                    .take_while(|x| **x != DRow::Strip(s))
                    .filter(|x| matches!(x, DRow::Strip(_)))
                    .count();
                BEFORE + list_n + before
            }
        }
    }
}

fn lv_rows(n: usize, cols: usize) -> usize {
    crate::libview::rows(n, cols)
}

type Setter = Box<dyn FnOnce(&mut Page, Load<Vec<Media>>)>;
type ListFut =
    std::pin::Pin<Box<dyn Future<Output = Result<Vec<Media>, crate::source::Failed>> + Send>>;

impl Root {
    /// Open whatever `m` is: libraries and genres browse, everything else gets a detail page.
    pub(super) fn open_any(&mut self, m: Media, cx: &mut Context<Self>) {
        match m.kind {
            Kind::Library => self.open_library(m, cx),
            Kind::Genre => self.open_browse(scope_tag("genre", &m.title, &m.title), cx),
            Kind::Studio => self.open_browse(scope_tag("studio", &m.id, &m.title), cx),
            _ => self.push(View::Detail, Some(m), cx),
        }
    }

    pub(super) fn load_detail(&mut self, pid: u64, m: Media, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let id = m.id.clone();
        let s = src.clone();
        self.run(cx, async move { s.full(&id).await }, move |t, r, _| {
            if let (Some(p), Ok((full_m, full))) = (t.page_mut(pid), r) {
                let keep = p.media.take();
                p.media = Some(match keep {
                    // Keep what the list knew (playback position) if the detail lacks it.
                    Some(old) => Media {
                        resume_s: full_m.resume_s.or(old.resume_s),
                        ..full_m
                    },
                    None => full_m,
                });
                p.full = Some(full);
            }
        });
        let (kind, id) = (m.kind, m.id.clone());
        let load = |t: &mut Self, f: Setter, fut: ListFut, cx: &mut Context<Self>| {
            t.run(cx, fut, move |t, r, _| {
                if let Some(p) = t.page_mut(pid) {
                    f(p, r.map_or(Load::Failed, Load::Ready));
                }
            });
        };
        if matches!(
            kind,
            Kind::Movie | Kind::Series | Kind::MusicAlbum | Kind::MusicArtist
        ) {
            let (s, i) = (src.clone(), id.clone());
            load(
                self,
                Box::new(|p, l| p.similar = l),
                Box::pin(async move { s.similar(&i).await }),
                cx,
            );
        }
        if matches!(kind, Kind::Movie | Kind::Series) {
            let (s, i) = (src.clone(), id.clone());
            load(
                self,
                Box::new(|p, l| p.extras = l),
                Box::pin(async move { s.extras(&i).await }),
                cx,
            );
        }
        let (s, i) = (src.clone(), id.clone());
        match kind {
            Kind::Series => {
                self.run(cx, async move { s.seasons(&i).await }, move |t, r, cx| {
                    let first = r.as_ref().ok().and_then(|v| v.first().cloned());
                    if let Some(p) = t.page_mut(pid) {
                        p.seasons = r.map_or(Load::Failed, Load::Ready);
                    }
                    if let Some(s) = first {
                        t.load_episodes(pid, s.id, cx);
                    }
                });
            }
            Kind::Season => {
                let series = m.series_id.clone().unwrap_or_default();
                load(
                    self,
                    Box::new(|p, l| p.episodes = l),
                    Box::pin(async move { s.episodes(&series, &i).await }),
                    cx,
                );
            }
            Kind::BoxSet | Kind::MusicAlbum => {
                load(
                    self,
                    Box::new(|p, l| p.episodes = l),
                    Box::pin(async move { s.ordered_children(&i).await }),
                    cx,
                );
            }
            Kind::MusicArtist => {
                load(
                    self,
                    Box::new(|p, l| p.episodes = l),
                    Box::pin(async move { s.artist_albums(&i).await }),
                    cx,
                );
            }
            Kind::Playlist => {
                load(
                    self,
                    Box::new(|p, l| p.episodes = l),
                    Box::pin(async move { s.playlist_items(&i).await }),
                    cx,
                );
            }
            Kind::Person => {
                let s2 = s.clone();
                let i2 = i.clone();
                load(
                    self,
                    Box::new(|p, l| p.kids = l),
                    Box::pin(async move { s.filmography(&i, &["Movie"]).await }),
                    cx,
                );
                load(
                    self,
                    Box::new(|p, l| p.kids2 = l),
                    Box::pin(async move { s2.filmography(&i2, &["Series"]).await }),
                    cx,
                );
            }
            Kind::Episode => {
                let ep = m.clone();
                self.run(
                    cx,
                    async move { s.episode_around(&ep).await },
                    move |t, n, _| {
                        if let Some(p) = t.page_mut(pid) {
                            p.neigh = n;
                        }
                    },
                );
            }
            _ => {}
        }
    }

    // ---- actions ----

    /// Play every playable item of a list, in order or shuffled; the rest queue behind it.
    fn play_list(&mut self, items: Vec<Media>, shuffle: bool, cx: &mut Context<Self>) {
        let mut v: Vec<Media> = items.into_iter().filter(Media::playable).collect();
        if shuffle {
            v = shuffled(v);
        }
        if v.is_empty() {
            return self.toast("Nothing to play here yet.", cx);
        }
        let first = v.remove(0);
        self.play_media(first, v, cx);
    }

    fn do_act(&mut self, a: DAct, cx: &mut Context<Self>) {
        let Some(p) = self.stack.last() else { return };
        let Some(m) = p.media.clone() else { return };
        let (list, neigh) = (p.episodes.items().to_vec(), p.neigh.clone());
        match a {
            DAct::Play if m.kind == Kind::Season => match crate::source::pick_episode(&list) {
                Some(e) => self.play_item(e.clone(), cx),
                None => self.toast("Nothing to play here yet.", cx),
            },
            DAct::Play => self.play_item(m, cx),
            DAct::PlayAll => self.play_list(list, false, cx),
            DAct::Shuffle => self.play_list(list, true, cx),
            DAct::Watched => self.toggle_played(&m, cx),
            DAct::Fav => self.toggle_fav(&m, cx),
            DAct::Prev => {
                if let Some(e) = neigh.0 {
                    self.open_any(e, cx)
                }
            }
            DAct::Next => {
                if let Some(e) = neigh.1 {
                    self.open_any(e, cx)
                }
            }
            DAct::Series => {
                if let Some(id) = m.series_id.clone() {
                    let mut s = Media::from_item(&nocturne_jellyfin::Item::default());
                    s.id = id;
                    s.kind = Kind::Series;
                    s.title = m.series_name.clone().unwrap_or_default();
                    self.open_any(s, cx);
                }
            }
        }
        cx.notify();
    }

    fn do_chip(&mut self, c: Chip, cx: &mut Context<Self>) {
        match c {
            Chip::Genre(g) => self.open_browse(scope_tag("genre", &g, &g), cx),
            Chip::Studio(id, n) => self.open_browse(scope_tag("studio", &id, &n), cx),
            Chip::Link(_, url) => {
                // Only ever http(s) (checked when mapped); handed to the desktop's opener.
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            }
        }
    }

    fn do_sec(&mut self, s: Sec, m: Media, cx: &mut Context<Self>) {
        match s {
            Sec::Chapters => {
                if let Some(mut pm) = self.stack.last().and_then(|p| p.media.clone()) {
                    pm.resume_s = m.resume_s;
                    pm.played = false;
                    self.play_item(pm, cx);
                }
            }
            Sec::Extras => self.play_item(m, cx),
            _ => self.open_any(m, cx),
        }
    }

    /// Row under the selection, activated (Enter / click).
    fn do_row(&mut self, r: DRow, col: usize, cx: &mut Context<Self>) {
        let Some(p) = self.stack.last() else { return };
        match r {
            DRow::Actions => {
                if let Some(a) = p.dacts().get(col).copied() {
                    self.do_act(a, cx);
                }
            }
            DRow::Chips => {
                if let Some(c) = p.chips_list().get(col).cloned() {
                    self.do_chip(c, cx);
                }
            }
            DRow::Seasons => {}
            DRow::List => {
                if let Some(e) = p.episodes.items().get(p.ep_sel).cloned() {
                    self.list_open(e, cx);
                }
            }
            DRow::Strip(s) => {
                if let Some(m) = p.sec_items(s).get(col).cloned() {
                    self.do_sec(s, m, cx);
                }
            }
        }
    }

    /// Enter / click on a list row: playables play, containers open.
    fn list_open(&mut self, e: Media, cx: &mut Context<Self>) {
        if e.playable() {
            self.play_item(e, cx)
        } else {
            self.open_any(e, cx)
        }
    }

    pub(super) fn detail_key(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(p) = self.stack.last_mut() else {
            return;
        };
        let rows = p.drows();
        let cur = rows.get(p.drow).copied().unwrap_or(DRow::Actions);
        let pid = p.pid;
        let dir = match key {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        let n = p.episodes.items().len();
        let cols = if p.card_grid() { p.dcols.max(1) } else { 1 };
        // 2D move inside the list/grid; at its top or bottom edge the page row changes.
        let mut leave = dir.filter(|d| matches!(d, Dir::Up | Dir::Down));
        if let (DRow::List, Some(d)) = (cur, dir) {
            match nav::grid_move(p.ep_sel, cols, n, d) {
                nav::Move::Inside(i) => {
                    p.ep_sel = i;
                    leave = None;
                }
                nav::Move::Leave(_) => {}
            }
            if matches!(d, Dir::Left | Dir::Right) {
                leave = None;
            }
        }
        match (dir, leave) {
            (Some(d), Some(_)) => {
                p.drow = match d {
                    Dir::Up => p.drow.saturating_sub(1),
                    _ => (p.drow + 1).min(rows.len() - 1),
                };
                let r = rows[p.drow];
                p.dcol = p.dcol.min(p.drow_len(r).saturating_sub(1));
                if r == DRow::List {
                    let c = cols.min(n.max(1));
                    p.ep_sel = if d == Dir::Up {
                        (n.saturating_sub(1) / cols * cols + p.ep_sel % cols.max(1))
                            .min(n.saturating_sub(1))
                    } else {
                        p.ep_sel.min(c.saturating_sub(1))
                    };
                }
            }
            (Some(d), None) if cur == DRow::Seasons => {
                let ns = p.seasons.items().len();
                let s = nav::step(p.season_sel, ns.max(1), ns, d);
                if s != p.season_sel {
                    p.season_sel = s;
                    let id = p.seasons.items()[s].id.clone();
                    self.load_episodes(pid, id, cx);
                }
            }
            (Some(d), None) if cur != DRow::List => {
                let len = p.drow_len(cur);
                p.dcol = nav::step(p.dcol, len.max(1), len, d);
                if let DRow::Strip(s) = cur {
                    p.dscroll[s.idx()].scroll_to_item(p.dcol);
                }
            }
            (None, _) if key == "enter" => {
                let col = p.dcol;
                self.do_row(cur, col, cx);
                return cx.notify();
            }
            (None, _) if key == "i" && cur == DRow::List => {
                if let Some(e) = p.episodes.items().get(p.ep_sel).cloned() {
                    self.open_any(e, cx);
                }
                return cx.notify();
            }
            _ => {}
        }
        let Some(p) = self.stack.last_mut() else {
            return;
        };
        let r = p.drows().get(p.drow).copied().unwrap_or(DRow::Actions);
        p.page_scroll.scroll_to_item(p.child_of(r));
        cx.notify();
    }

    // ---- render ----

    pub(super) fn detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.stack.last() else {
            return div().into_any_element();
        };
        let Some(m) = p.media.clone() else {
            return div().into_any_element();
        };
        let (pid, scroll) = (p.pid, p.page_scroll.clone());
        let rows = p.drows();
        let (drow, dcol) = (p.drow, p.dcol);
        let focus_of = |r: DRow| (rows.get(drow) == Some(&r)).then_some(dcol);
        let full = p.full.clone().unwrap_or_default();
        let (acts, chips) = (p.dacts(), p.chips_list());

        // Hero with the action buttons.
        let mut buttons = div().flex().flex_wrap().gap(px(t::S3));
        let af = focus_of(DRow::Actions);
        let writes = self.hooks.can_write();
        for (i, a) in acts.into_iter().enumerate() {
            let (id, label, primary) = spec(a, &m);
            let off = !writes && matches!(a, DAct::Watched | DAct::Fav);
            buttons = buttons.child(
                ui::pill_button(
                    id,
                    label,
                    primary,
                    af == Some(i),
                    cx.listener(move |t, _, _, cx| t.do_act(a, cx)),
                )
                .when(off, |d| d.opacity(0.4).tooltip(ui::tip(NEED_WRITES))),
            );
        }
        let eyebrow = if m.kind == Kind::Episode {
            format!(
                "0x01 · {} · {}",
                m.series_name.clone().unwrap_or_default(),
                m.episode_label()
            )
        } else {
            format!("0x01 · {}", m.kind_label())
        };
        let hero = self.hero_band(&m, eyebrow, buttons, 0.55, cx);

        let mut page = div()
            .id("detail")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .flex()
            .flex_col()
            .child(ui::band(hero))
            .child(ui::band(self.facts_band(&m, &full)))
            .child(ui::band(self.chips_band(chips, focus_of(DRow::Chips), cx)));
        let poster = if m.backdrop_tag.is_none()
            && matches!(
                m.kind,
                Kind::Person | Kind::MusicAlbum | Kind::MusicArtist | Kind::Playlist | Kind::BoxSet
            ) {
            Some(self.pic(&m, Art::Poster, 480, cx))
        } else {
            None
        };
        page = page.child(ui::band(self.about_band(&m, &full, poster)));

        if self.stack.last().is_some_and(Page::has_list) {
            page = self.list_children(page, &m, rows.get(drow).copied(), pid, cx);
        } else {
            page = page.child(div().flex_none());
        }
        for s in Sec::ALL {
            let items = self
                .stack
                .last()
                .map_or(vec![], |p| p.sec_items(s).to_vec());
            if items.is_empty() {
                continue;
            }
            let (title, text) = s.title(m.kind);
            let handle = self
                .stack
                .last()
                .map(|p| p.dscroll[s.idx()].clone())
                .unwrap_or_default();
            let land = s == Sec::Chapters || s == Sec::Extras;
            let dims = if land {
                self.row_dims(300.0, 9.0 / 16.0)
            } else {
                self.row_dims(170.0, 1.5)
            };
            let n = items.len();
            let strip =
                self.detail_strip(s, items, &handle, focus_of(DRow::Strip(s)), land, dims, cx);
            page = page.child(Self::strip_band(title, text, format!("{n}"), strip));
        }
        if m.kind == Kind::Person
            && let Some(p) = self.stack.last()
        {
            let all = [&p.kids, &p.kids2];
            let any_items = all.iter().any(|l| !l.items().is_empty());
            if !any_items {
                page = if all.iter().any(|l| matches!(l, Load::Loading)) {
                    page.child(ui::band(div().p(px(t::S6)).child(ui::mono("loading"))))
                } else if all.iter().any(|l| matches!(l, Load::Failed)) {
                    page.child(ui::band(
                        div()
                            .h(px(220.0))
                            .child(self.failed_view("This couldn't be loaded.", cx)),
                    ))
                } else {
                    page.child(ui::band(
                        div()
                            .p(px(t::S6))
                            .text_color(t::text_secondary())
                            .child("Nothing in your library features this person."),
                    ))
                };
            }
        }
        if !full.info.is_empty() {
            page = page.child(ui::band(info_table(&full)));
        }
        page.child(div().h(px(t::S8 * 2.0)).flex_none())
            .into_any_element()
    }

    fn facts_band(&self, m: &Media, f: &Full) -> Div {
        let mut line = div().flex().flex_wrap().items_center().gap(px(t::S3));
        if let Some(r) = m.rating {
            let (full, half, empty) = detail::stars(r);
            let mut st = div()
                .flex()
                .text_size(px(t::BODY))
                .text_color(t::accent_text());
            st = st.child("\u{2605}".repeat(full as usize));
            if half {
                st = st.child(div().opacity(0.5).child("\u{2605}"));
            }
            st = st.child(
                div()
                    .text_color(t::text_tertiary())
                    .child("\u{2606}".repeat(empty as usize)),
            );
            line = line.child(st).child(ui::mono(format!("{r:.1}")));
        }
        if let Some(c) = f.critic {
            line = line.child(ui::mono(format!("{c:.0}% critics")));
        }
        if let Some(r) = f.official_rating.as_deref() {
            line = line.child(ui::chip(r, false));
        }
        if let Some(y) = m.year {
            line = line.child(ui::mono(y.to_string()));
        }
        if let Some(r) = m.runtime_min {
            line = line.child(ui::mono(crate::source::fmt_runtime(r)));
            if matches!(m.kind, Kind::Movie | Kind::Episode | Kind::Video) {
                let left = m
                    .progress()
                    .map_or(r, |p| ((1.0 - p) * r as f32).round() as u32);
                line = line.child(ui::mono(format!(
                    "ends {}",
                    detail::ends_at(detail::local_minutes(), left)
                )));
            }
        }
        if m.kind == Kind::Series
            && let Some(n) = m.unplayed.filter(|n| *n > 0)
        {
            line = line.child(ui::mono(format!("{n} unwatched")));
        }
        if let Some(n) = m
            .child_count
            .filter(|_| matches!(m.kind, Kind::BoxSet | Kind::Playlist | Kind::MusicAlbum))
        {
            line = line.child(ui::mono(format!("{n} items")));
        }
        div().px(px(t::PAGE_PAD)).py(px(t::S4)).child(line)
    }

    fn chips_band(&self, chips: Vec<Chip>, focus: Option<usize>, cx: &mut Context<Self>) -> Div {
        if chips.is_empty() {
            return div();
        }
        let mut row = div()
            .px(px(t::PAGE_PAD))
            .pb(px(t::S4))
            .flex()
            .flex_wrap()
            .gap(px(t::S2));
        for (i, c) in chips.into_iter().enumerate() {
            let (label, link) = match &c {
                Chip::Genre(g) => (g.clone(), false),
                Chip::Studio(_, n) => (n.clone(), false),
                Chip::Link(n, _) => (format!("{n} \u{2197}"), true),
            };
            let on = focus == Some(i);
            row = row.child(
                div()
                    .id(("chip", i))
                    .px(px(t::S3))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .rounded(px(14.0))
                    .border_1()
                    .border_color(if on {
                        t::accent()
                    } else {
                        t::hairline_strong()
                    })
                    .bg(if on { t::accent_tint() } else { t::glass() })
                    .text_size(px(t::CAPTION + 1.0))
                    .text_color(if link {
                        t::accent_text()
                    } else {
                        t::text_secondary()
                    })
                    .cursor_pointer()
                    .hover(|s| s.border_color(t::accent()))
                    .on_click(cx.listener(move |t, _, _, cx| t.do_chip(c.clone(), cx)))
                    .child(label),
            );
        }
        row
    }

    fn about_band(&self, m: &Media, f: &Full, poster: Option<Pic>) -> Div {
        let mut left = div().flex_1().min_w_0().flex().flex_col().gap(px(t::S3));
        if let Some(tag) = &f.tagline {
            left = left.child(
                div()
                    .text_size(px(t::BODY_LG))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t::accent_text())
                    .child(tag.clone()),
            );
        }
        let text = m
            .overview
            .clone()
            .unwrap_or_else(|| "No description from the server.".into());
        left = left.child(
            div()
                .max_w(px(760.0))
                .line_height(px(t::BODY_LG * 1.6))
                .text_size(px(t::BODY_LG))
                .text_color(t::text_secondary())
                .child(text),
        );
        let mut facts: Vec<(&str, String)> = vec![];
        let mut put = |k: &'static str, v: Option<String>| {
            if let Some(v) = v.filter(|v| !v.is_empty()) {
                facts.push((k, v));
            }
        };
        put("Rated", f.official_rating.clone());
        let date = f
            .premiere
            .as_deref()
            .map(|d| d.chars().take(10).collect::<String>());
        put(
            if m.kind == Kind::Person {
                "Born"
            } else {
                "Premiere"
            },
            date,
        );
        put("Place", f.places.first().cloned());
        put("Studio", f.studios.first().map(|s| s.1.clone()));
        put(
            "Genres",
            (!m.genres.is_empty()).then(|| m.genres.join(", ")),
        );
        put(
            "Added",
            m.added.as_deref().map(|d| d.chars().take(10).collect()),
        );
        let mut right = div()
            .w(px(300.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(t::S2));
        for (k, v) in facts {
            right = right.child(
                div()
                    .flex()
                    .justify_between()
                    .gap(px(t::S4))
                    .child(ui::mono(k.to_uppercase()))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(t::CAPTION + 1.0))
                            .text_color(t::text_secondary())
                            .child(v),
                    ),
            );
        }
        let mut row = div()
            .px(px(t::PAGE_PAD))
            .py(px(t::S6))
            .flex()
            .gap(px(t::S8));
        if let Some(pic) = poster {
            row = row.child(
                ui::tile(pic, &m.title, "about-poster")
                    .w(px(180.0))
                    .h(px(240.0))
                    .flex_none(),
            );
        }
        row.child(left).child(right)
    }

    /// The episode / track / member list, flat so keyboard scrolling can address each row.
    fn list_children(
        &mut self,
        mut page: Stateful<Div>,
        m: &Media,
        focus: Option<DRow>,
        pid: u64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (heading, sub) = match m.kind {
            Kind::Series => ("Episodes", "Pick a season, then an episode."),
            Kind::Season => ("Episodes", "Everything in this season."),
            Kind::MusicAlbum => ("Tracks", "In album order."),
            Kind::MusicArtist => ("Albums", "Everything by this artist."),
            Kind::Playlist => ("Playlist", "In the order it plays."),
            _ => ("In this collection", "Everything grouped here."),
        };
        let Some(p) = self.stack.last() else {
            return page;
        };
        let (seasons, season_sel, episodes, ep_sel) = (
            p.seasons.clone(),
            p.season_sel,
            p.episodes.clone(),
            p.ep_sel,
        );
        let list_focus = focus == Some(DRow::List);
        let meta = if m.kind == Kind::Series {
            format!("{} seasons", seasons.items().len())
        } else {
            episodes.items().len().to_string()
        };
        let mut head = div()
            .py(px(t::S8))
            .flex()
            .flex_col()
            .gap(px(t::S4))
            .child(ui::section(heading, sub, meta));
        if m.kind == Kind::Series {
            let mut tabs = div()
                .px(px(t::PAGE_PAD))
                .flex()
                .flex_wrap()
                .gap(px(t::S1))
                .border_b_1()
                .border_color(t::hairline());
            for (i, s) in seasons.items().iter().enumerate() {
                let id = s.id.clone();
                let n = match s.unplayed {
                    Some(0) => " \u{2713}".to_string(),
                    Some(n) => format!("  {n}"),
                    None => String::new(),
                };
                let on = focus == Some(DRow::Seasons) && i == season_sel;
                tabs = tabs.child(
                    ui::tab(
                        format!("season-{i}"),
                        &format!("{}{n}", s.title),
                        i == season_sel,
                        cx.listener(move |t, _, _, cx| {
                            if let Some(p) = t.page_mut(pid) {
                                p.season_sel = i;
                            }
                            t.load_episodes(pid, id.clone(), cx);
                            cx.notify();
                        }),
                    )
                    .when(on, |d| d.bg(t::accent_tint())),
                );
            }
            head = head.child(tabs);
        }
        page = page.child(ui::band(head));
        match &episodes {
            Load::Loading => page.child(ui::band(div().p(px(t::S6)).child(ui::mono("loading")))),
            Load::Failed => page.child(ui::band(
                div()
                    .h(px(220.0))
                    .child(self.failed_view("This couldn't be loaded.", cx)),
            )),
            Load::Ready(v) if v.is_empty() => page.child(ui::band(
                div().p(px(t::S6)).text_color(t::text_secondary()).child(
                    if m.kind == Kind::Series {
                        "No episodes have been added yet."
                    } else {
                        "Nothing in here yet."
                    },
                ),
            )),
            Load::Ready(v) if matches!(m.kind, Kind::Series | Kind::Season) => {
                let avail = self.colw() - 2.0 * t::PAGE_PAD;
                let cols = nav::fit_cols(avail, 340.0);
                if let Some(p) = self.stack.last_mut() {
                    p.dcols = cols;
                }
                let w = (avail / cols as f32).floor() - 2.0 * t::S3;
                for (r, chunk) in v.chunks(cols).enumerate() {
                    let mut row = div().px(px(t::PAGE_PAD)).flex().items_start();
                    for (c, e) in chunk.iter().enumerate() {
                        let i = r * cols + c;
                        row = row.child(self.episode_card(
                            e,
                            i,
                            list_focus && i == ep_sel,
                            w,
                            pid,
                            cx,
                        ));
                    }
                    page = page.child(row);
                }
                page
            }
            Load::Ready(v) => {
                for (ei, e) in v.iter().enumerate() {
                    page = page.child(ui::band(self.list_item(
                        e,
                        ei,
                        list_focus && ei == ep_sel,
                        m.kind,
                        pid,
                        cx,
                    )));
                }
                page
            }
        }
    }

    /// 16:9 episode card: thumbnail with play overlay and progress, title, meta, overview, actions.
    fn episode_card(
        &mut self,
        e: &Media,
        i: usize,
        on: bool,
        w: f32,
        pid: u64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let h = (w * 9.0 / 16.0).round();
        let pic = self.pic(e, Art::Landscape, (w * 2.0) as u32, cx);
        let (play, info, seen, fav) = (e.clone(), e.clone(), e.clone(), e.clone());
        let writes = self.hooks.can_write();
        let title = match e.number {
            Some(n) => format!("{n}. {}", e.title),
            None => e.title.clone(),
        };
        let mut meta = vec![];
        if let Some(r) = e.runtime_min {
            meta.push(crate::source::fmt_runtime(r));
            let left = e
                .progress()
                .map_or(r, |p| ((1.0 - p) * r as f32).round() as u32);
            if !e.played {
                meta.push(format!(
                    "ends {}",
                    detail::ends_at(detail::local_minutes(), left)
                ));
            }
        }
        if let Some(r) = e.rating {
            meta.insert(meta.len().min(1), format!("\u{2605} {r:.1}"));
        }
        let icon = |id: String, g: &'static str, active: bool, tip: &'static str| {
            div()
                .id(ui::name(id))
                .size(px(26.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(if active { t::accent() } else { t::hairline() })
                .text_size(px(13.0))
                .text_color(if active {
                    t::accent_text()
                } else {
                    t::text_secondary()
                })
                .cursor_pointer()
                .hover(|s| s.border_color(t::accent()))
                .tooltip(ui::tip(tip))
                .child(g)
        };
        div()
            .id(ui::name(format!("card-{}", e.id)))
            .group("card")
            .w(px(w + 2.0 * t::S3))
            .flex_none()
            .p(px(t::S3))
            .flex()
            .flex_col()
            .gap(px(t::S2))
            .cursor_pointer()
            .on_hover(cx.listener(move |t, hov: &bool, _, cx| {
                if *hov
                    && let Some(p) = t.page_mut(pid)
                    && let Some(r) = p.drows().iter().position(|r| *r == DRow::List)
                    && (p.drow, p.ep_sel) != (r, i)
                {
                    (p.drow, p.ep_sel) = (r, i);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |t, _, _, cx| {
                t.play_item(play.clone(), cx);
                cx.notify();
            }))
            .child(
                div()
                    .relative()
                    .w(px(w))
                    .h(px(h))
                    .rounded(px(t::R_XS))
                    .border_1()
                    .border_color(if on { t::accent() } else { rgba(0) })
                    .child(ui::tile(pic, &e.title, &format!("card-{}", e.id)).size_full())
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(44.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(t::accent())
                                    .text_color(t::on_accent())
                                    .opacity(if on { 1.0 } else { 0.55 })
                                    .group_hover("card", |s| s.opacity(1.0))
                                    .child("\u{25B6}"),
                            ),
                    )
                    .when(e.played, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(t::S2))
                                .right(px(t::S2))
                                .size(px(22.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(t::accent())
                                .text_color(t::on_accent())
                                .text_size(px(12.0))
                                .child("\u{2713}"),
                        )
                    })
                    .when_some(e.progress(), |d, p| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_0()
                                .left_0()
                                .w_full()
                                .child(ui::progress_bar(p, 3.0)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(t::S2))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t::text())
                                    .child(title),
                            )
                            .child(div().truncate().child(ui::mono(meta.join("   "))))
                            .when_some(e.overview.clone(), |d, o| {
                                d.child(
                                    div()
                                        .pt(px(t::S1))
                                        .line_clamp(3)
                                        .text_size(px(t::CAPTION + 1.0))
                                        .line_height(px((t::CAPTION + 1.0) * 1.5))
                                        .text_color(t::text_tertiary())
                                        .child(o),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(t::S1))
                            .child(
                                icon(format!("ci-{}", e.id), "i", false, "Details (I)").on_click(
                                    cx.listener(move |t, _, _, cx| {
                                        t.open_any(info.clone(), cx);
                                        cx.stop_propagation();
                                    }),
                                ),
                            )
                            .child(
                                icon(format!("cs-{}", e.id), "\u{2713}", e.played, "Watched")
                                    .when(!writes, |d| d.opacity(0.4).tooltip(ui::tip(NEED_WRITES)))
                                    .on_click(cx.listener(move |t, _, _, cx| {
                                        t.toggle_played(&seen, cx);
                                        cx.notify();
                                        cx.stop_propagation();
                                    })),
                            )
                            .child(
                                icon(
                                    format!("cf-{}", e.id),
                                    if e.favorite { "\u{2665}" } else { "\u{2661}" },
                                    e.favorite,
                                    "Favourite",
                                )
                                .when(!writes, |d| d.opacity(0.4).tooltip(ui::tip(NEED_WRITES)))
                                .on_click(cx.listener(
                                    move |t, _, _, cx| {
                                        t.toggle_fav(&fav, cx);
                                        cx.notify();
                                        cx.stop_propagation();
                                    },
                                )),
                            ),
                    ),
            )
    }

    fn list_item(
        &mut self,
        e: &Media,
        ei: usize,
        on: bool,
        kind: Kind,
        pid: u64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let pic = self.pic(e, Art::Poster, 320, cx);
        let target = e.clone();
        let num = match (e.kind, e.number) {
            (Kind::Audio | Kind::Episode, Some(n)) => format!("{n:>2}"),
            _ => format!("0x{:02x}", ei + 1),
        };
        let mut sub = vec![];
        if kind != Kind::Season && e.kind == Kind::Episode {
            sub.push(e.episode_label());
        }
        if let Some(a) = e.sub.clone().filter(|_| e.kind == Kind::Audio) {
            sub.push(a);
        }
        sub.extend(e.runtime_min.map(crate::source::fmt_runtime));
        if e.played {
            sub.push("watched \u{2713}".into());
        }
        let sub: Vec<String> = sub.into_iter().filter(|s| !s.is_empty()).collect();
        let overview = e.overview.clone().filter(|_| e.kind == Kind::Episode);
        div()
            .id(ui::name(format!("ep-{ei}")))
            .px(px(t::PAGE_PAD))
            .py(px(t::S3))
            .flex()
            .items_center()
            .gap(px(t::S4))
            .bg(if on { t::accent_tint() } else { rgba(0) })
            .cursor_pointer()
            .hover(|s| s.bg(t::glass()))
            .on_click(cx.listener(move |t, _, _, cx| {
                if let Some(p) = t.page_mut(pid) {
                    p.ep_sel = ei;
                    p.drow = p
                        .drows()
                        .iter()
                        .position(|r| *r == DRow::List)
                        .unwrap_or(p.drow);
                }
                t.list_open(target.clone(), cx);
                cx.notify();
            }))
            .child(div().w(px(40.0)).child(ui::mono(num)))
            .child(
                div()
                    .relative()
                    .w(px(128.0))
                    .h(px(72.0))
                    .flex_none()
                    .child(ui::tile(pic, &e.title, &format!("ep-{}", e.id)).size_full())
                    .when_some(e.progress(), |d, p| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_0()
                                .left_0()
                                .w_full()
                                .child(ui::progress_bar(p, 3.0)),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(t::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if on { t::text() } else { t::text_secondary() })
                            .child(e.title.clone()),
                    )
                    .child(ui::mono(sub.join(" · ")))
                    .when_some(overview, |d, o| {
                        d.child(
                            div()
                                .line_clamp(2)
                                .text_size(px(t::CAPTION + 1.0))
                                .text_color(t::text_tertiary())
                                .child(o),
                        )
                    }),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn detail_strip(
        &mut self,
        sec: Sec,
        items: Vec<Media>,
        handle: &ScrollHandle,
        focus: Option<usize>,
        land: bool,
        (cw, ch): (f32, f32),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut strip = div()
            .id(ui::name(format!("dstrip-{}", sec.idx())))
            .flex()
            .overflow_x_scroll()
            .track_scroll(handle);
        for (ci, m) in items.iter().enumerate() {
            let art = match (sec, land) {
                (Sec::Chapters, _) => Art::Chapter,
                (_, true) => Art::Landscape,
                _ => Art::Poster,
            };
            let pic = self.pic(m, art, (cw * 2.0) as u32, cx);
            let target = m.clone();
            let mut cell = ui::poster_cell(Cell {
                key: &format!("d{}-{}-{ci}", sec.idx(), m.id),
                pic,
                title: &m.title,
                meta: caption(m),
                index: ci,
                w: cw,
                h: ch,
                focused: focus == Some(ci),
                progress: m.progress(),
                badge: None,
            })
            .group("card")
            .on_hover(cx.listener(move |t, hov: &bool, _, cx| {
                if *hov
                    && let Some(p) = t.stack.last_mut()
                    && let Some(r) = p.drows().iter().position(|r| *r == DRow::Strip(sec))
                    && (p.drow, p.dcol) != (r, ci)
                {
                    (p.drow, p.dcol) = (r, ci);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |t, _, _, cx| {
                t.do_sec(sec, target.clone(), cx);
                cx.notify();
            }));
            if matches!(m.kind, Kind::Movie | Kind::Series | Kind::Episode) {
                cell = cell.child(self.chips(m, cx));
            }
            strip = strip.child(cell);
        }
        strip
    }
}

fn info_table(f: &Full) -> Div {
    let mut d = div()
        .px(px(t::PAGE_PAD))
        .py(px(t::S8))
        .flex()
        .flex_col()
        .gap(px(t::S4))
        .child(ui::label("Media info"));
    for b in &f.info {
        let head = [b.container.as_str(), b.size.as_str(), b.bitrate.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let mut t_ = div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(t::hairline())
            .rounded(px(t::R_PANEL))
            .child(
                div()
                    .px(px(t::S4))
                    .h(px(36.0))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(t::hairline())
                    .child(ui::mono(head)),
            );
        for s in &b.streams {
            let col = |s: String, w: Option<f32>| {
                let d = div()
                    .truncate()
                    .text_size(px(t::CAPTION + 1.0))
                    .text_color(t::text_secondary())
                    .child(s);
                match w {
                    Some(w) => d.w(px(w)).flex_none(),
                    None => d.flex_1().min_w_0(),
                }
            };
            t_ = t_.child(
                div()
                    .px(px(t::S4))
                    .h(px(36.0))
                    .flex()
                    .items_center()
                    .gap(px(t::S4))
                    .border_b_1()
                    .border_color(t::hairline())
                    .child(
                        div()
                            .w(px(80.0))
                            .flex_none()
                            .child(ui::mono(s.kind.to_uppercase())),
                    )
                    .child(col(s.codec.clone(), Some(72.0)))
                    .child(col(s.title.clone(), None))
                    .child(col(s.detail.clone(), None))
                    .child(
                        div()
                            .w(px(90.0))
                            .flex_none()
                            .child(ui::mono(s.bitrate.clone())),
                    ),
            );
        }
        d = d.child(t_);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test; // gpui::* also exports a `test` macro

    fn m(kind: &str) -> Media {
        Media::from_item(&nocturne_jellyfin::Item {
            id: "x".into(),
            kind: kind.into(),
            series_id: Some("s".into()),
            ..Default::default()
        })
    }

    #[test]
    fn actions_by_kind() {
        use DAct::*;
        assert_eq!(actions(&m("Movie"), false, false), [Play, Watched, Fav]);
        assert_eq!(
            actions(&m("Episode"), true, true),
            [Play, Watched, Fav, Prev, Next, Series]
        );
        assert_eq!(
            actions(&m("Episode"), false, false),
            [Play, Watched, Fav, Series]
        );
        assert_eq!(actions(&m("Series"), false, false), [Play, Fav]);
        assert_eq!(actions(&m("Season"), false, false), [Play, Series]);
        assert_eq!(
            actions(&m("Playlist"), false, false),
            [PlayAll, Shuffle, Fav]
        );
        assert_eq!(actions(&m("Person"), false, false), [Fav]);
    }

    #[test]
    fn shuffle_keeps_every_item() {
        let v: Vec<Media> = (0..50)
            .map(|i| {
                let mut x = m("Audio");
                x.id = i.to_string();
                x
            })
            .collect();
        let mut ids: Vec<usize> = shuffled(v).iter().map(|x| x.id.parse().unwrap()).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..50).collect::<Vec<_>>());
    }
}
