//! The Calendar page in the app: month, week and agenda views of upcoming episodes from every signed-in server and, when connected, the Trakt calendar. The merging, dates and ranges are pure and live in `nocturne_core::calendar`; this file holds the page state, the reads (one task per server, one for Trakt) and the keyboard. The page is an element tree (`calendar_tree.rs`) so a theme can restyle it. Read only.
use super::message::Message;
use super::{App, View};
use crate::keys::Key;
use crate::task::run;
use nocturne_core::calendar::{self as cal, Day, Entry, Status, Upcoming};
use nocturne_core::source::{Kind, Media, ProviderIds, SourceId};
use nocturne_core::trakt::{Airing, Trakt};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

/// Episodes a month cell lists before "+N more" (a taller window fits more).
pub const MONTH_CAP_MIN: usize = 1;

#[derive(Clone, Debug)]
pub enum Msg {
    Open,
    /// One server's upcoming episodes and the provider ids of their series (`None`: it did not answer).
    Server(
        u64,
        SourceId,
        Option<(Vec<Upcoming>, Vec<(String, ProviderIds)>)>,
    ),
    /// Trakt's calendar for the chunks starting at these days (`Ok(None)`: not connected).
    Trakt(u64, Vec<i64>, Result<Option<Vec<Airing>>, ()>),
    /// One server's series, once Trakt has answered (`None`: it did not).
    Shows(u64, SourceId, Option<Vec<Media>>),
    View(cal::View),
    Step(i64),
    Today,
    /// The entry (or "+N more") with this index among the navigable items.
    Pick(usize),
}

/// What the keyboard and a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Entry(usize),
    /// A month cell's overflow link, which opens that day in the week view.
    More(Day),
}

pub struct Cal {
    pub view: cal::View,
    pub anchor: Day,
    pub today: Day,
    pub epoch: u64,
    /// Servers still to answer.
    pub pending: usize,
    pub failed: bool,
    servers: BTreeMap<SourceId, Vec<Upcoming>>,
    trakt: Vec<Airing>,
    /// The servers' series, to tell a Trakt show a server has from one it has not.
    shows: Vec<Media>,
    shows_asked: bool,
    chunks: HashSet<i64>,
    /// `Some(false)`: Trakt is not connected, so nothing more is asked.
    pub trakt_on: Option<bool>,
    pub trakt_failed: bool,
    pub entries: Vec<Entry>,
    pub sel: usize,
    /// The first rebuild puts the selection on the first entry from today on.
    auto_sel: bool,
}

impl Default for Cal {
    fn default() -> Self {
        Self::new(cal::today())
    }
}

impl Cal {
    pub fn new(today: Day) -> Self {
        Self {
            view: cal::View::Month,
            anchor: today,
            today,
            epoch: 0,
            pending: 0,
            failed: false,
            servers: BTreeMap::new(),
            trakt: vec![],
            shows: vec![],
            shows_asked: false,
            chunks: HashSet::new(),
            trakt_on: None,
            trakt_failed: false,
            entries: vec![],
            sel: 0,
            auto_sel: true,
        }
    }

    /// The window of days the page shows.
    pub fn range(&self) -> (Day, Day) {
        cal::range(self.view, self.anchor)
    }

    /// The items the keyboard moves over, in reading order. A month cell lists `cap` entries and then "+N more".
    pub fn items(&self, cap: usize) -> Vec<Item> {
        let (s, e) = self.range();
        let mut out = vec![];
        let by = cal::by_day(&self.entries, s, e);
        let at = |d: Day| self.entries.partition_point(|x| x.day < d);
        for (day, list) in by {
            let first = at(day);
            let (shown, hidden) = if self.view == cal::View::Month {
                month_split(list.len(), cap)
            } else {
                (list.len(), 0)
            };
            out.extend((0..shown).map(|i| Item::Entry(first + i)));
            if hidden > 0 {
                out.push(Item::More(day));
            }
        }
        out
    }

    fn day_of(&self, it: Item) -> Day {
        match it {
            Item::Entry(i) => self.entries[i].day,
            Item::More(d) => d,
        }
    }

    /// Server lists best source first, as `cal::build` wants them.
    fn ordered(&self, prio: &[SourceId]) -> Vec<(SourceId, Vec<Upcoming>)> {
        prio.iter()
            .filter_map(|o| Some((*o, self.servers.get(o)?.clone())))
            .collect()
    }
}

/// The height of a month cell and how many entries it lists, for a window `h` px tall and a month of `weeks` rows.
pub fn month_geo(h: f32, weeks: i64) -> (f32, usize) {
    let cell = ((h - 300.0) / weeks as f32).clamp(116.0, 176.0);
    let cap = (((cell - 36.0) / 42.0).floor() as usize).clamp(MONTH_CAP_MIN, 4);
    (cell, cap)
}

/// How many of a month cell's `n` entries are listed and how many hide behind "+N more": all of them when they fit `cap`, else one slot less (the link takes it).
pub fn month_split(n: usize, cap: usize) -> (usize, usize) {
    if n <= cap {
        (n, 0)
    } else {
        let shown = cap.saturating_sub(1);
        (shown, n - shown)
    }
}

impl App {
    pub(super) fn cal_cap(&self) -> usize {
        let (s, e) = self.cal.range();
        month_geo(self.win.height, (e.0 - s.0) / 7).1
    }

    /// Is the Calendar the page in view?
    pub(super) fn at_calendar(&self) -> bool {
        self.stack.last().is_some_and(|p| p.view == View::Calendar)
    }

    pub(super) fn cal_msg(&mut self, m: Msg) -> iced::Task<Message> {
        use iced::Task;
        match m {
            Msg::Open => self.cal_open(),
            Msg::Server(epoch, origin, r) => {
                if epoch != self.cal.epoch {
                    return Task::none();
                }
                self.cal.pending = self.cal.pending.saturating_sub(1);
                match r {
                    Some((up, ids)) => {
                        for (id, p) in ids {
                            self.series_ids.set(origin, id, p);
                        }
                        self.cal.servers.insert(origin, up);
                    }
                    None => self.cal.failed = true,
                }
                self.cal_rebuild();
                self.cal_wants()
            }
            Msg::Trakt(epoch, starts, r) => {
                if epoch != self.cal.epoch {
                    return Task::none();
                }
                match r {
                    Ok(None) => self.cal.trakt_on = Some(false),
                    Ok(Some(v)) => {
                        self.cal.trakt_on = Some(true);
                        for a in v {
                            if !self.cal.trakt.contains(&a) {
                                self.cal.trakt.push(a);
                            }
                        }
                        if !self.cal.shows_asked {
                            return Task::batch([self.cal_shows(), self.cal_after()]);
                        }
                    }
                    Err(()) => {
                        // Ask again the next time these days are shown.
                        for s in starts {
                            self.cal.chunks.remove(&s);
                        }
                        self.cal.trakt_failed = true;
                    }
                }
                self.cal_after()
            }
            Msg::Shows(epoch, origin, r) => {
                if epoch != self.cal.epoch {
                    return Task::none();
                }
                for m in r.into_iter().flatten() {
                    if !m.ids.is_empty() {
                        self.series_ids.set(origin, m.id.clone(), m.ids.clone());
                    }
                    self.cal.shows.push(m);
                }
                self.cal_after()
            }
            Msg::View(v) => {
                self.cal.view = v;
                self.cal_moved()
            }
            Msg::Step(d) => {
                self.cal.anchor = cal::step(self.cal.view, self.cal.anchor, d);
                self.cal_moved()
            }
            Msg::Today => {
                self.cal.today = cal::today();
                self.cal.anchor = self.cal.today;
                self.cal_moved()
            }
            Msg::Pick(i) => {
                self.cal.sel = i;
                self.cal_activate()
            }
        }
    }

    /// Show the page from today, and read every server's upcoming list.
    fn cal_open(&mut self) -> iced::Task<Message> {
        use iced::Task;
        if !matches!(self.phase, super::Phase::Ready) || self.catalog.slots.is_empty() {
            return Task::none();
        }
        self.close_admin();
        self.stack.truncate(1);
        self.fwd.clear();
        let page = Media::from_item(&nocturne_jellyfin::Item {
            id: "calendar".into(),
            name: "Calendar".into(),
            ..Default::default()
        });
        self.push_page(View::Calendar, Some(page));
        let epoch = self.cal.epoch + 1;
        self.cal = Cal {
            epoch,
            ..Cal::default()
        };
        let mut tasks = vec![];
        for s in self.catalog.online() {
            let (Some(b), origin) = (s.backend.clone(), s.id) else {
                continue;
            };
            if b.is_local() {
                continue;
            }
            self.cal.pending += 1;
            tasks.push(run(
                async move {
                    let up = b.upcoming().await.ok()?;
                    let mut sids: Vec<String> = up
                        .iter()
                        .filter_map(|u| u.media.series_id.clone())
                        .collect();
                    sids.sort();
                    sids.dedup();
                    let found = b.provider_ids(&sids).await.unwrap_or_default();
                    Some((up, found.into_iter().collect::<Vec<_>>()))
                },
                move |r| Message::Calendar(Msg::Server(epoch, origin, r)),
            ));
        }
        tasks.push(self.cal_trakt());
        Task::batch(tasks)
    }

    /// Ask Trakt for the days of the range not asked yet (at most 33 days a call). Nothing when Trakt is not connected.
    fn cal_trakt(&mut self) -> iced::Task<Message> {
        use iced::Task;
        if self.cal.trakt_on == Some(false) {
            return Task::none();
        }
        let (s, e) = self.cal.range();
        let mut starts = vec![];
        let mut d = s;
        while d < e {
            if self.cal.chunks.insert(d.0) {
                starts.push(d.0);
            }
            d = d.plus(33);
        }
        if starts.is_empty() {
            return Task::none();
        }
        let epoch = self.cal.epoch;
        let client = self.trakt_client();
        let asked = starts.clone();
        run(
            async move {
                let c: Arc<Trakt> = match client {
                    Some(c) => c,
                    None => match Trakt::load().await {
                        Some(c) => Arc::new(c),
                        None => return Ok(None),
                    },
                };
                let mut out = vec![];
                for st in &asked {
                    out.extend(c.calendar(&Day(*st).iso(), 33).await.map_err(|_| ())?);
                }
                Ok(Some(out))
            },
            move |r| Message::Calendar(Msg::Trakt(epoch, starts, r)),
        )
    }

    fn cal_after(&mut self) -> iced::Task<Message> {
        self.cal_rebuild();
        self.cal_wants()
    }

    /// Read every server's series once, to find the Trakt shows they have.
    fn cal_shows(&mut self) -> iced::Task<Message> {
        self.cal.shows_asked = true;
        let epoch = self.cal.epoch;
        let tasks: Vec<_> = self
            .catalog
            .online()
            .filter_map(|s| Some((s.id, s.backend.clone().filter(|b| !b.is_local())?)))
            .map(|(origin, b)| {
                run(async move { b.series_list().await.ok() }, move |r| {
                    Message::Calendar(Msg::Shows(epoch, origin, r))
                })
            })
            .collect();
        iced::Task::batch(tasks)
    }

    /// Recompute the entries from what has arrived so far.
    fn cal_rebuild(&mut self) {
        let prio = self.catalog.priority();
        let servers = self.cal.ordered(&prio);
        self.cal.entries = cal::build(
            &servers,
            &self.series_ids,
            &self.cal.shows,
            &self.cal.trakt,
            &cal::local_offset,
        );
        let items = self.cal.items(self.cal_cap());
        if self.cal.auto_sel {
            self.cal.sel = items
                .iter()
                .position(|it| self.cal.day_of(*it) >= self.cal.today)
                .unwrap_or(0);
        }
        self.cal.sel = self.cal.sel.min(items.len().saturating_sub(1));
    }

    /// The range changed: a fresh selection, Trakt's days and the pictures.
    fn cal_moved(&mut self) -> iced::Task<Message> {
        self.cal.auto_sel = true;
        self.cal_rebuild();
        iced::Task::batch([self.cal_trakt(), self.cal_wants()])
    }

    /// The posters of the shows in the range.
    pub(super) fn cal_wants(&mut self) -> iced::Task<Message> {
        let (s, e) = self.cal.range();
        let mut seen = HashSet::new();
        let wants: Vec<_> = cal::within(&self.cal.entries, s, e)
            .into_iter()
            .filter_map(|x| x.series.clone())
            .filter(|m| seen.insert((m.origin, m.id.clone())))
            .map(|m| (m, nocturne_core::source::Art::Poster, 200))
            .collect();
        self.want(&wants)
    }

    /// Enter or a click on the selected item: play what a server has, open the show it will come to, or say that nothing here has it.
    fn cal_activate(&mut self) -> iced::Task<Message> {
        use iced::Task;
        let items = self.cal.items(self.cal_cap());
        let Some(it) = items.get(self.cal.sel).copied() else {
            return Task::none();
        };
        let e = match it {
            Item::More(d) => {
                self.cal.view = cal::View::Week;
                self.cal.anchor = d;
                return self.cal_moved();
            }
            Item::Entry(i) => self.cal.entries[i].clone(),
        };
        match (e.status, e.episode, e.series) {
            (Status::OnServer, Some(ep), _) if ep.kind == Kind::Episode => {
                Task::done(Message::PlayItem(ep))
            }
            (_, _, Some(series)) => self.open_detail(series),
            _ => {
                self.toast("Not on your servers yet.");
                Task::none()
            }
        }
    }

    /// Keys on the Calendar page. Escape and Alt+arrows are the app's own.
    pub(super) fn cal_key(&mut self, k: &Key) -> iced::Task<Message> {
        use iced::Task;
        let items = self.cal.items(self.cal_cap());
        let n = items.len();
        let key = k.name.as_str();
        let to = |c: &mut Cal, i: usize| {
            c.auto_sel = false;
            c.sel = i.min(n.saturating_sub(1));
        };
        match key {
            "left" | "right" | "up" | "down" | "tab" if n > 0 => {
                let back = matches!(key, "left" | "up") || (key == "tab" && k.shift);
                let cur = self.cal.sel.min(n - 1);
                let next = if self.cal.view == cal::View::Month && matches!(key, "up" | "down") {
                    // A row up or down: the first item a week away (or the nearest day with one).
                    let day = self.cal.day_of(items[cur]);
                    let step = if back { -7 } else { 7 };
                    let want = day.plus(step);
                    let hit = if back {
                        items.iter().rposition(|it| self.cal.day_of(*it) <= want)
                    } else {
                        items.iter().position(|it| self.cal.day_of(*it) >= want)
                    };
                    hit.map(|h| {
                        let d = self.cal.day_of(items[h]);
                        items
                            .iter()
                            .position(|it| self.cal.day_of(*it) == d)
                            .unwrap_or(h)
                    })
                    .unwrap_or(cur)
                } else if back {
                    cur.saturating_sub(1)
                } else {
                    (cur + 1).min(n - 1)
                };
                to(&mut self.cal, next);
                Task::none()
            }
            "home" => {
                to(&mut self.cal, 0);
                Task::none()
            }
            "end" => {
                to(&mut self.cal, n.saturating_sub(1));
                Task::none()
            }
            "enter" => self.cal_activate(),
            "pageup" | "[" => self.cal_msg(Msg::Step(-1)),
            "pagedown" | "]" => self.cal_msg(Msg::Step(1)),
            "t" if k.plain() => self.cal_msg(Msg::Today),
            "m" if k.plain() => self.cal_msg(Msg::View(cal::View::Month)),
            "w" if k.plain() => self.cal_msg(Msg::View(cal::View::Week)),
            "l" if k.plain() => self.cal_msg(Msg::View(cal::View::Agenda)),
            _ => Task::none(),
        }
    }
}

impl App {
    /// The hand-written page (no theme engine, or `NOCTURNE_ENGINE_UI=0`): the agenda as plain rows with the same controls.
    pub(super) fn cal_fallback(&self) -> iced::Element<'_, Message> {
        use crate::style;
        use crate::theme as t;
        use crate::widget::{mono, pill_button};
        use iced::widget::{Space, column, container, row, scrollable, text};
        use iced::{Alignment, Length};
        let c = &self.cal;
        let (s, e) = c.range();
        let mut bar = row![].spacing(t::S2).align_y(Alignment::Center);
        for v in cal::View::ALL {
            bar = bar.push(pill_button(
                v.label(),
                v == c.view,
                false,
                Message::Calendar(Msg::View(v)),
            ));
        }
        bar = bar
            .push(Space::new().width(t::S4))
            .push(pill_button(
                "\u{2039}",
                false,
                false,
                Message::Calendar(Msg::Step(-1)),
            ))
            .push(pill_button(
                "Today",
                false,
                false,
                Message::Calendar(Msg::Today),
            ))
            .push(pill_button(
                "\u{203A}",
                false,
                false,
                Message::Calendar(Msg::Step(1)),
            ));
        let items = c.items(self.cal_cap());
        let mut list = column![].spacing(t::S2);
        let shown = cal::by_day(&c.entries, s, e);
        if shown.is_empty() {
            list = list.push(
                text(if c.pending > 0 {
                    "Loading upcoming episodes"
                } else {
                    "Nothing airing in this range"
                })
                .size(t::BODY)
                .color(t::text_secondary()),
            );
        }
        for (day, group) in shown {
            list = list.push(
                text(cal::day_heading(day, c.today))
                    .size(t::BODY)
                    .font(t::bold()),
            );
            for x in group {
                let k = c
                    .entries
                    .iter()
                    .position(|y| std::ptr::eq(y, x))
                    .unwrap_or(0);
                let at = items.iter().position(|it| *it == Item::Entry(k));
                let line = format!(
                    "{}  {}  {}  {}",
                    x.time_text().unwrap_or_default(),
                    x.show,
                    x.code(),
                    x.status.label()
                );
                let on = at == Some(c.sel);
                let r = crate::widget::press::Press::new(text(line).size(t::BODY).color(if on {
                    t::accent_text()
                } else {
                    t::text()
                }))
                .padding([t::S1, t::S2]);
                list = list.push(match at {
                    Some(i) => r.on_press(Message::Calendar(Msg::Pick(i))),
                    None => r,
                });
            }
        }
        container(
            column![
                text("Calendar").size(t::HEADING).font(t::bold()),
                text(cal::title(c.view, c.anchor))
                    .size(t::BODY)
                    .color(t::text_secondary()),
                bar,
                mono("Arrows move, Enter opens, M W L views, PgUp PgDn period, T today"),
                scrollable(list).style(style::scroll).height(Length::Fill),
            ]
            .spacing(t::S3),
        )
        .padding(iced::Padding {
            top: t::header_h() + t::S8,
            left: t::page_pad(),
            right: t::page_pad(),
            bottom: t::S6,
        })
        .into()
    }
}
