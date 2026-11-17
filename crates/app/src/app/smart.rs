//! Smart collections: the header menu, the manager/editor dialog and the loading behind them. The
//! rules and their evaluation are `nocturne_core::smart`; this file only loads each kind's merged
//! list (once, progressively, shared by every page and the editor's live count) and paints.
use super::browse::{Browse, Mode};
use super::message::Message;
use super::{App, View};
use crate::style;
use crate::task::run;
use crate::theme as t;
use crate::widget::overlay;
use crate::widget::popover::Popover;
use crate::widget::{action_button, label, menu_row, mono, nav_link};
use iced::widget::{column, container, row, scrollable, space, text, text_input};
use iced::{Element, Length, Task};
use nocturne_core::libview::{Res, Scope, SortKey};
use nocturne_core::smart::{Rule, Store, Watch};
use nocturne_core::source::SourceId;
use nocturne_core::unified::{MERGED_PAGE, Merged};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type El<'a> = Element<'a, Message>;

/// A loaded list is reused this long before a page asks the sources again.
const FRESH: Duration = Duration::from_secs(60);

/// What one merged load fetches: the kind (and resolution, which the server answers) over these sources.
pub type Key = (nocturne_core::unified::UKind, Option<Res>, Vec<SourceId>);

#[derive(Clone)]
pub enum Msg {
    Menu,
    Open(u64),
    Manage,
    New,
    Edit(u64),
    Delete(u64),
    Shift(u64, bool),
    Close,
    Name(String),
    Kind(nocturne_core::unified::UKind),
    Watch(Watch),
    Genres(String),
    YearFrom(String),
    YearTo(String),
    RuntimeMin(String),
    RuntimeMax(String),
    Rating(Option<f32>),
    Added(Option<u32>),
    Res(Option<Res>),
    Source(SourceId),
    Sort(SortKey),
    Desc,
    Limit(Option<u32>),
    Save,
    /// A page of one source of the loading list `key` (slot `epoch`).
    Page {
        key: Key,
        epoch: u64,
        origin: SourceId,
        start: usize,
        r: Option<(Vec<nocturne_core::source::Media>, usize)>,
    },
}

struct Slot {
    merged: Merged,
    at: Instant,
}

/// The dialog's draft; numbers and genres stay text until saved.
struct Editor {
    /// `None` while the dialog shows the list, `Some(None)` for a new rule.
    edit: Option<Option<u64>>,
    name: String,
    rule: Rule,
    genres: String,
    year_from: String,
    year_to: String,
    rt_min: String,
    rt_max: String,
    count: Option<usize>,
}

#[derive(Default)]
pub struct State {
    pub store: Store,
    menu: bool,
    cache: HashMap<Key, Slot>,
    epoch: u64,
    dlg: Option<Editor>,
}

impl State {
    pub fn load() -> Self {
        Self {
            store: Store::default_path().map_or_else(Store::default, |p| Store::load_from(&p)),
            ..Self::default()
        }
    }

    pub fn is_open(&self) -> bool {
        self.dlg.is_some()
    }

    pub fn menu_open(&self) -> bool {
        self.menu
    }

    pub fn close_menu(&mut self) {
        self.menu = false;
    }

    /// The catalog changed: loaded lists are stale.
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Editor {
    fn new(id: Option<u64>, name: String, rule: Rule) -> Self {
        let n = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
        Self {
            edit: Some(id),
            name,
            genres: rule.genres.join(", "),
            year_from: n(rule.year_from),
            year_to: n(rule.year_to),
            rt_min: n(rule.min_runtime),
            rt_max: n(rule.max_runtime),
            rule,
            count: None,
        }
    }

    /// The text fields folded into the rule.
    fn rule(&self) -> Rule {
        let n = |s: &str| s.trim().parse::<u32>().ok();
        Rule {
            genres: self
                .genres
                .split(',')
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty())
                .collect(),
            year_from: n(&self.year_from),
            year_to: n(&self.year_to),
            min_runtime: n(&self.rt_min),
            max_runtime: n(&self.rt_max),
            ..self.rule.clone()
        }
    }
}

pub fn scope_of(id: u64, name: &str, rule: &Rule) -> Scope {
    Scope {
        key: format!("smart-{id}"),
        title: name.into(),
        subtitle:
            "A smart collection: a saved rule over every source. Stored on this computer only."
                .into(),
        collection: None,
        base: rule.kind.scope().base,
    }
}

impl App {
    fn smart_sources(&self, r: &Rule) -> Vec<SourceId> {
        self.catalog
            .online()
            // A local folder cannot answer a resolution rule (docs/SOURCES.md), so it sits it out.
            .filter(|s| r.res.is_none() || !s.backend.as_ref().is_some_and(|b| b.is_local()))
            .filter(|s| {
                self.filter.admits(s.id) && (r.sources.is_empty() || r.sources.contains(&s.id))
            })
            .map(|s| s.id)
            .collect()
    }

    fn smart_key(&self, r: &Rule) -> Key {
        (r.kind, r.res, self.smart_sources(r))
    }

    /// Start loading `key` unless a fresh (or still loading) list exists.
    fn smart_load(&mut self, key: &Key) -> Task<Message> {
        if self
            .smart
            .cache
            .get(key)
            .is_some_and(|s| s.merged.loading() || s.at.elapsed() < FRESH)
        {
            return Task::none();
        }
        self.smart.epoch += 1;
        let epoch = self.smart.epoch;
        self.smart.cache.insert(
            key.clone(),
            Slot {
                merged: Merged::new(epoch, &key.2),
                at: Instant::now(),
            },
        );
        Task::batch(key.2.iter().map(|o| self.smart_ask(key, epoch, *o, 0)))
    }

    fn smart_ask(&self, key: &Key, epoch: u64, origin: SourceId, start: usize) -> Task<Message> {
        let Some(b) = self.catalog.backend(origin).cloned() else {
            return Task::none();
        };
        let mut q = key.0.scope().base;
        q.sort_by = SortKey::Name.params();
        q.is_4k = key.1 == Some(Res::Uhd4k);
        q.is_hd = key.1 == Some(Res::Hd);
        q.limit = Some(MERGED_PAGE as u32);
        q.start_index = Some(start as u32);
        let key = key.clone();
        run(async move { b.browse(&q).await.ok() }, move |r| {
            Message::Smart(Msg::Page {
                key,
                epoch,
                origin,
                start,
                r,
            })
        })
    }

    /// What `rule` selects out of what its list has loaded so far.
    fn smart_entries(&self, rule: &Rule) -> Option<Vec<nocturne_core::merge::Entry>> {
        let slot = self.smart.cache.get(&self.smart_key(rule))?;
        let all = slot.merged.entries(
            &self.catalog.priority(),
            &self.prefs,
            &self.series_ids,
            SortKey::Name,
            false,
        );
        Some(rule.apply(all, now()))
    }

    /// Open a collection as a normal browse page over the merged entries.
    pub(super) fn open_smart(&mut self, id: u64) -> Task<Message> {
        let Some(s) = self.smart.store.get(id).cloned() else {
            return Task::none();
        };
        self.smart.menu = false;
        self.close_admin();
        self.stack.truncate(1);
        self.fwd.clear();
        let sources = self.smart_sources(&s.rule);
        if sources.is_empty() {
            self.toast("No source is available for this collection right now.");
            return Task::none();
        }
        let scope = scope_of(id, &s.name, &s.rule);
        let pid = self.push_page(View::Grid, None);
        let mut br = Browse::new(scope, Mode::Merged(sources), vec![], Default::default());
        br.smart = Some((id, true));
        if let Some(p) = self.page_mut(pid) {
            p.br = Some(Box::new(br));
        }
        self.browse_reload(pid)
    }

    /// Reload of a smart page: (re)start the list's load and show what is there.
    pub(super) fn smart_reload(&mut self, id: u64) -> Task<Message> {
        let Some(s) = self.smart.store.get(id) else {
            return Task::none();
        };
        let key = self.smart_key(&s.rule);
        let task = self.smart_load(&key);
        self.smart_refresh();
        Task::batch([task, self.browse_visible()])
    }

    /// Put the current result into the smart page in view and the editor's count.
    fn smart_refresh(&mut self) {
        if let Some(id) = self
            .stack
            .last()
            .and_then(|p| p.br.as_deref())
            .and_then(|b| b.smart)
            .map(|s| s.0)
            && let Some(s) = self.smart.store.get(id)
        {
            let rule = s.rule.clone();
            let list = self.smart_entries(&rule);
            let (loading, failed) = self
                .smart
                .cache
                .get(&self.smart_key(&rule))
                .map_or((false, true), |s| (s.merged.loading(), s.merged.failed()));
            if let Some(br) = self.stack.last_mut().and_then(|p| p.br.as_deref_mut()) {
                br.list = list.unwrap_or_default();
                br.total = br.list.len();
                br.failed = failed;
                br.smart = Some((id, loading));
            }
        }
        if let Some(d) = &self.smart.dlg
            && d.edit.is_some()
        {
            let count = self.smart_entries(&d.rule()).map(|v| v.len());
            if let Some(d) = self.smart.dlg.as_mut() {
                d.count = count;
            }
        }
    }

    fn smart_save(&self) {
        if let Some(p) = nocturne_core::smart::Store::default_path()
            && let Err(e) = self.smart.store.save_to(&p)
        {
            eprintln!("could not save smart collections: {e}");
        }
    }

    pub(super) fn smart_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Menu => {
                self.smart.menu = !self.smart.menu;
                Task::none()
            }
            Msg::Open(id) => self.open_smart(id),
            Msg::Manage => {
                self.smart.menu = false;
                self.smart.dlg = Some(Editor {
                    edit: None,
                    ..Editor::new(None, String::new(), Rule::default())
                });
                Task::none()
            }
            Msg::New => self.smart_edit(None, "New collection".into(), Rule::default()),
            Msg::Edit(id) => match self.smart.store.get(id).cloned() {
                Some(s) => self.smart_edit(Some(id), s.name, s.rule),
                None => Task::none(),
            },
            Msg::Delete(id) => {
                self.smart.store.delete(id);
                self.smart_save();
                Task::none()
            }
            Msg::Shift(id, up) => {
                self.smart.store.shift(id, up);
                self.smart_save();
                Task::none()
            }
            Msg::Close => {
                // From the editor back to the list, from the list away.
                match self.smart.dlg.as_mut() {
                    Some(d) if d.edit.is_some() => d.edit = None,
                    _ => self.smart.dlg = None,
                }
                Task::none()
            }
            Msg::Save => {
                if let Some(d) = self.smart.dlg.as_mut() {
                    let (rule, name) = (d.rule(), d.name.trim().to_string());
                    let name = if name.is_empty() {
                        "Untitled".into()
                    } else {
                        name
                    };
                    match d.edit.flatten() {
                        Some(id) => self.smart.store.update(id, name, rule),
                        None => {
                            self.smart.store.add(name, rule);
                        }
                    }
                    d.edit = None;
                    self.smart_save();
                    // A page showing the edited rule follows it.
                    return self.reload_browse();
                }
                Task::none()
            }
            Msg::Page {
                key,
                epoch,
                origin,
                start,
                r,
            } => {
                let next = self
                    .smart
                    .cache
                    .get_mut(&key)
                    .and_then(|s| s.merged.accept(epoch, origin, start, r));
                self.smart_refresh();
                let more = next.map_or_else(Task::none, |n| self.smart_ask(&key, epoch, origin, n));
                Task::batch([more, self.browse_visible()])
            }
            draft => self.smart_draft(draft),
        }
    }

    fn smart_edit(&mut self, id: Option<u64>, name: String, rule: Rule) -> Task<Message> {
        self.smart.dlg = Some(Editor::new(id, name, rule));
        self.smart_changed()
    }

    /// The draft changed: make sure its list is loading and refresh the count.
    fn smart_changed(&mut self) -> Task<Message> {
        let Some(d) = &self.smart.dlg else {
            return Task::none();
        };
        let key = self.smart_key(&d.rule());
        let task = self.smart_load(&key);
        self.smart_refresh();
        task
    }

    /// Edits of the draft's fields.
    fn smart_draft(&mut self, m: Msg) -> Task<Message> {
        let Some(d) = self.smart.dlg.as_mut() else {
            return Task::none();
        };
        match m {
            Msg::Name(v) => d.name = v,
            Msg::Kind(k) => d.rule.kind = k,
            Msg::Watch(w) => d.rule.watch = w,
            Msg::Genres(v) => d.genres = v,
            Msg::YearFrom(v) => d.year_from = v,
            Msg::YearTo(v) => d.year_to = v,
            Msg::RuntimeMin(v) => d.rt_min = v,
            Msg::RuntimeMax(v) => d.rt_max = v,
            Msg::Rating(r) => d.rule.min_rating = r,
            Msg::Added(a) => d.rule.added_days = a,
            Msg::Res(r) => d.rule.res = r,
            Msg::Source(s) => {
                let v = &mut d.rule.sources;
                match v.iter().position(|x| *x == s) {
                    Some(i) => {
                        v.remove(i);
                    }
                    None => v.push(s),
                }
            }
            Msg::Sort(k) => d.rule.sort = k,
            Msg::Desc => d.rule.desc = !d.rule.desc,
            Msg::Limit(l) => d.rule.limit = l,
            _ => {}
        }
        self.smart_changed()
    }

    // ---- views ----

    /// The header's "Collections" menu (named Smart to stay apart from the server's box sets).
    pub(super) fn smart_menu(&self, active: bool, focused: bool) -> El<'_> {
        let link = nav_link(
            "Smart \u{25BE}",
            active || self.smart.menu,
            focused,
            Message::Smart(Msg::Menu),
        );
        let popup = self.smart.menu.then(|| {
            let mut items: Vec<El> = self
                .smart
                .store
                .items
                .iter()
                .map(|s| {
                    menu_row(
                        text(s.name.clone()).size(t::BODY),
                        false,
                        false,
                        Message::Smart(Msg::Open(s.id)),
                    )
                })
                .collect();
            items.push(menu_row(
                text("Manage smart collections\u{2026}")
                    .size(t::BODY)
                    .color(t::TEXT_SECONDARY),
                false,
                false,
                Message::Smart(Msg::Manage),
            ));
            container(scrollable(column(items).spacing(2)).style(style::scroll))
                .width(280)
                .max_height(420)
                .padding(t::S2)
                .style(style::panel)
                .into()
        });
        Popover::new(link, popup, Message::Smart(Msg::Menu))
            .left()
            .into()
    }

    pub(super) fn smart_dialog(&self) -> El<'_> {
        let Some(d) = &self.smart.dlg else {
            return space().into();
        };
        let away = Message::Smart(Msg::Close);
        let (title, body) = match d.edit {
            None => ("Smart collections", self.smart_list()),
            Some(_) => ("Collection", self.smart_form(d)),
        };
        overlay::panel(title, 560.0, self.win, body, away)
    }

    fn smart_list(&self) -> Vec<El<'_>> {
        let n = self.smart.store.items.len();
        let mut v: Vec<El> = vec![
            text("Saved rules over everything you have, from every source. They live on this computer and are never sent to a server.")
                .size(t::CONTROL_TEXT)
                .color(t::TEXT_SECONDARY)
                .into(),
        ];
        for (i, s) in self.smart.store.items.iter().enumerate() {
            let id = s.id;
            v.push(
                row![
                    text(s.name.clone()).size(t::BODY).width(Length::Fill),
                    action_button(
                        "Up",
                        false,
                        false,
                        false,
                        i == 0,
                        Message::Smart(Msg::Shift(id, true))
                    ),
                    action_button(
                        "Down",
                        false,
                        false,
                        false,
                        i + 1 == n,
                        Message::Smart(Msg::Shift(id, false))
                    ),
                    action_button(
                        "Edit",
                        false,
                        false,
                        false,
                        false,
                        Message::Smart(Msg::Edit(id))
                    ),
                    action_button(
                        "Delete",
                        false,
                        true,
                        false,
                        false,
                        Message::Smart(Msg::Delete(id))
                    ),
                ]
                .spacing(t::S2)
                .align_y(iced::Alignment::Center)
                .into(),
            );
        }
        v.push(
            row![
                action_button(
                    "New collection",
                    true,
                    false,
                    true,
                    false,
                    Message::Smart(Msg::New)
                ),
                action_button(
                    "Close",
                    false,
                    false,
                    false,
                    false,
                    Message::Smart(Msg::Close)
                ),
            ]
            .spacing(t::S3)
            .into(),
        );
        v
    }

    fn smart_form<'a>(&'a self, d: &'a Editor) -> Vec<El<'a>> {
        let r = &d.rule;
        let input = |ph: &'static str, v: &str, f: fn(String) -> Msg| -> El<'a> {
            text_input(ph, v)
                .on_input(move |s| Message::Smart(f(s)))
                .size(t::BODY)
                .padding([t::S2, t::S3])
                .style(style::input)
                .into()
        };
        let chip = |l: &str, on: bool, m: Msg| {
            action_button(l, on, false, false, false, Message::Smart(m))
        };
        let chips = |c: Vec<El<'a>>| -> El<'a> { row(c).spacing(t::S2).wrap().into() };
        let field = |name: &str, body: El<'a>| -> El<'a> {
            column![label(name), body].spacing(t::S1).into()
        };
        let sources: Vec<El> = self
            .catalog
            .online()
            .map(|s| {
                chip(
                    &s.profile.label,
                    r.sources.contains(&s.id),
                    Msg::Source(s.id),
                )
            })
            .collect();
        let rating = [None, Some(6.0), Some(7.0), Some(8.0), Some(9.0)];
        let added = [None, Some(7), Some(30), Some(90), Some(365)];
        let limit = [None, Some(25), Some(50), Some(100), Some(250)];
        let mut v: Vec<El> = vec![
            field("NAME", input("Name", &d.name, Msg::Name)),
            field(
                "KIND",
                chips(
                    [
                        nocturne_core::unified::UKind::Movies,
                        nocturne_core::unified::UKind::Shows,
                    ]
                    .into_iter()
                    .map(|k| chip(k.label(), r.kind == k, Msg::Kind(k)))
                    .collect(),
                ),
            ),
            field(
                "WATCHED",
                chips(
                    Watch::ALL
                        .into_iter()
                        .map(|w| chip(w.label(), r.watch == w, Msg::Watch(w)))
                        .collect(),
                ),
            ),
            field(
                "GENRES (ANY OF, COMMA SEPARATED)",
                input("Drama, Comedy", &d.genres, Msg::Genres),
            ),
            field(
                "YEAR",
                row![
                    input("From", &d.year_from, Msg::YearFrom),
                    input("To", &d.year_to, Msg::YearTo)
                ]
                .spacing(t::S2)
                .into(),
            ),
            field(
                "RATING AT LEAST",
                chips(
                    rating
                        .into_iter()
                        .map(|x| {
                            chip(
                                &x.map_or("Any".into(), |x| format!("{x:.0}+")),
                                r.min_rating == x,
                                Msg::Rating(x),
                            )
                        })
                        .collect(),
                ),
            ),
            field(
                "ADDED WITHIN",
                chips(
                    added
                        .into_iter()
                        .map(|x| {
                            chip(
                                &x.map_or("Any time".into(), |x| format!("{x} days")),
                                r.added_days == x,
                                Msg::Added(x),
                            )
                        })
                        .collect(),
                ),
            ),
            field(
                "RESOLUTION (SERVERS ONLY)",
                chips(
                    [
                        (None, "Any"),
                        (Some(Res::Hd), "HD"),
                        (Some(Res::Uhd4k), "4K"),
                    ]
                    .into_iter()
                    .map(|(x, l)| chip(l, r.res == x, Msg::Res(x)))
                    .collect(),
                ),
            ),
            field(
                "RUNTIME IN MINUTES",
                row![
                    input("At least", &d.rt_min, Msg::RuntimeMin),
                    input("At most", &d.rt_max, Msg::RuntimeMax)
                ]
                .spacing(t::S2)
                .into(),
            ),
        ];
        if !sources.is_empty() {
            v.push(field("ONLY FROM (NONE CHOSEN = ALL)", chips(sources)));
        }
        v.push(field(
            "ORDER",
            chips(
                SortKey::ALL
                    .into_iter()
                    .map(|k| chip(k.label(), r.sort == k, Msg::Sort(k)))
                    .chain([chip(
                        if r.desc { "Descending" } else { "Ascending" },
                        false,
                        Msg::Desc,
                    )])
                    .collect(),
            ),
        ));
        v.push(field(
            "SHOW AT MOST",
            chips(
                limit
                    .into_iter()
                    .map(|x| {
                        chip(
                            &x.map_or("All".into(), |x| x.to_string()),
                            r.limit == x,
                            Msg::Limit(x),
                        )
                    })
                    .collect(),
            ),
        ));
        v.push(
            row![
                action_button("Save", true, false, true, false, Message::Smart(Msg::Save)),
                action_button(
                    "Back",
                    false,
                    false,
                    false,
                    false,
                    Message::Smart(Msg::Close)
                ),
                space().width(Length::Fill),
                mono(match d.count {
                    Some(n) => format!("{n} MATCH NOW"),
                    None => "LOADING".into(),
                }),
            ]
            .spacing(t::S3)
            .align_y(iced::Alignment::Center)
            .into(),
        );
        v
    }
}
