//! Global search overlay: Ctrl+K or `/`, as-you-type with a debounce, every online source asked at
//! once and the answers merged (duplicates collapse), results grouped by type, keyboard navigation,
//! recent searches. The state machine is core's `search.rs`; the query is a real `text_input`.
use super::cells::mark_for;
use super::message::Message;
use super::{App, Phase};
use crate::style::{self, border};
use crate::task::{after, run};
use crate::theme as t;
use crate::widget::cell::tile;
use crate::widget::press::Press;
use crate::widget::{label, mono};
use iced::widget::Id;
use iced::widget::button::{Status, Style};
use iced::widget::operation;
use iced::widget::scrollable::{self, AbsoluteOffset};
use iced::widget::{Stack, column, container, mouse_area, opaque, row, space, text, text_input};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::merge::merge;
use nocturne_core::search::{self, Search};
use nocturne_core::source::{Art, Media, SourceId};
use std::collections::HashMap;

type El<'a> = Element<'a, Message>;

const INPUT_ID: &str = "search-input";
const LIST_ID: &str = "search-list";
const ROW_H: f32 = 62.0;
const HEAD_H: f32 = 36.0;
const LIST_H: f32 = 464.0;

#[derive(Clone)]
pub enum SMsg {
    Input(String),
    Due(u64),
    Ans {
        epoch: u64,
        q: String,
        origin: SourceId,
        r: Option<Vec<Media>>,
    },
    Enter,
    Pick(usize),
    Recent(usize),
    Close,
}

#[derive(Default)]
pub struct SearchUi {
    pub s: Search,
    pub recent_sel: usize,
    /// Epoch of the request whose answers are being collected.
    pub(super) req: u64,
    pending: usize,
    failed: usize,
    answers: Vec<(SourceId, Vec<Media>)>,
    /// How many sources have a copy of each hit.
    pub(super) copies: HashMap<(SourceId, String), usize>,
    off: f32,
}

impl SearchUi {
    /// Is `epoch` the request the overlay is collecting answers for (Seerr's share of it)?
    pub(super) fn is_request(&self, epoch: u64) -> bool {
        self.req == epoch
    }
}

/// Rows above hit `i` in the list: one group heading per group up to and including its own.
fn headings_before(s: &Search, i: usize) -> usize {
    let mut seen = 0;
    for (n, (_, v)) in s.results.iter().enumerate() {
        seen += v.len();
        if i < seen {
            return n + 1;
        }
    }
    // Past the catalog's own groups: the Seerr group's heading.
    s.results.len() + 1
}

impl App {
    pub(super) fn open_search(&mut self) -> Task<Message> {
        if !matches!(self.phase, Phase::Ready) || self.player.is_some() {
            return Task::none();
        }
        self.close_admin();
        self.search = Some(Box::default());
        operation::focus(Id::from(INPUT_ID))
    }

    pub(super) fn search_msg(&mut self, m: SMsg) -> Task<Message> {
        match m {
            SMsg::Close => {
                self.search = None;
                Task::none()
            }
            SMsg::Input(q) => {
                let Some(ui) = self.search.as_mut() else {
                    return Task::none();
                };
                ui.s.query = q;
                let epoch = ui.s.edited();
                ui.recent_sel = 0;
                after(search::DEBOUNCE, Message::Search(SMsg::Due(epoch)))
            }
            SMsg::Due(epoch) => {
                let Some(q) = self.search.as_mut().and_then(|u| u.s.due(epoch)) else {
                    return Task::none();
                };
                let slots: Vec<_> = self
                    .catalog
                    .online()
                    .filter(|s| self.filter.admits(s.id))
                    .filter_map(|s| Some((s.id, s.backend.clone()?)))
                    .collect();
                if let Some(ui) = self.search.as_mut() {
                    ui.req = epoch;
                    ui.pending = slots.len();
                    ui.failed = 0;
                    ui.answers.clear();
                    if slots.is_empty() {
                        ui.s.failed(epoch);
                    }
                }
                let seerr = self.rq_search(q.clone(), epoch);
                Task::batch(
                    slots
                        .into_iter()
                        .map(|(origin, b)| {
                            let (term, q2) = (q.clone(), q.clone());
                            run(async move { b.search(&term).await.ok() }, move |r| {
                                Message::Search(SMsg::Ans {
                                    epoch,
                                    q: q2,
                                    origin,
                                    r,
                                })
                            })
                        })
                        .chain([seerr]),
                )
            }
            SMsg::Ans {
                epoch,
                q,
                origin,
                r,
            } => {
                let prio = self.catalog.priority();
                let (prefs, series) = (&self.prefs, &self.series_ids);
                let Some(ui) = self.search.as_mut().filter(|u| u.req == epoch) else {
                    return Task::none();
                };
                ui.pending = ui.pending.saturating_sub(1);
                match r {
                    Some(v) => ui.answers.push((origin, v)),
                    None => ui.failed += 1,
                }
                if ui.answers.is_empty() && ui.pending == 0 {
                    ui.s.failed(epoch);
                    return Task::none();
                }
                // One hit per title: duplicates across sources collapse into the preferred copy.
                let entries = merge(ui.answers.clone(), &prio, prefs, series);
                ui.copies.clear();
                let items: Vec<Media> = entries
                    .iter()
                    .map(|e| {
                        ui.copies.insert(
                            (e.primary.origin, e.primary.id.clone()),
                            1 + e.alternates.len(),
                        );
                        e.primary.clone()
                    })
                    .collect();
                ui.s.arrived(epoch, &q, items);
                ui.s.loading = ui.pending > 0;
                let wants: Vec<_> =
                    ui.s.flat()
                        .into_iter()
                        .map(|m| (m.clone(), Art::Poster, 32))
                        .collect();
                self.want(&wants)
            }
            SMsg::Enter => {
                let Some(ui) = self.search.as_mut() else {
                    return Task::none();
                };
                if ui.s.query.trim().is_empty() {
                    let i = ui.recent_sel;
                    return self.search_msg(SMsg::Recent(i));
                }
                let i = ui.s.sel;
                self.search_msg(SMsg::Pick(i))
            }
            SMsg::Recent(i) => {
                let Some(r) = self.settings.recent_searches.get(i).cloned() else {
                    return Task::none();
                };
                let t = self.search_msg(SMsg::Input(r));
                Task::batch([t, operation::move_cursor_to_end(Id::from(INPUT_ID))])
            }
            SMsg::Pick(i) => {
                // Rows past the catalog's own are Seerr's ("Request").
                let n_core = self.search.as_ref().map_or(0, |u| u.s.flat().len());
                if i >= n_core {
                    let Some(t) = self
                        .rq_rows()
                        .get(i - n_core)
                        .and_then(|h| Self::rq_target_of(h))
                    else {
                        return Task::none();
                    };
                    self.search = None;
                    return self.rq_msg(super::requests::Msg::Ask(Box::new(t)));
                }
                let Some(ui) = self.search.take() else {
                    return Task::none();
                };
                let Some(hit) = ui.s.flat().get(i).map(|m| (*m).clone()) else {
                    self.search = Some(ui);
                    return Task::none();
                };
                search::push_recent(&mut self.settings.recent_searches, &ui.s.query);
                self.settings.save();
                self.open_any(hit)
            }
        }
    }

    /// Keys while the overlay is open (typing goes to the text input, not here).
    pub(super) fn search_key(&mut self, key: &str) -> Task<Message> {
        let n_recent = self.settings.recent_searches.len();
        let n_seerr = self.rq_rows().len();
        let Some(ui) = self.search.as_mut() else {
            return Task::none();
        };
        let recents = ui.s.query.trim().is_empty();
        match key {
            "escape" => {
                self.search = None;
                Task::none()
            }
            "down" | "up" => {
                let d = if key == "down" { 1 } else { -1 };
                if recents {
                    let n = n_recent.max(1) as i32;
                    ui.recent_sel = (ui.recent_sel as i32 + d).rem_euclid(n) as usize;
                    return Task::none();
                }
                if n_seerr > 0 {
                    // The Seerr rows follow the catalog's own in one list.
                    let n = (ui.s.flat().len() + n_seerr) as i32;
                    ui.s.sel = (ui.s.sel as i32 + d).rem_euclid(n) as usize;
                } else {
                    ui.s.step(d);
                }
                let top = headings_before(&ui.s, ui.s.sel) as f32 * HEAD_H
                    + ui.s.sel as f32 * ROW_H
                    - HEAD_H;
                let target = if top < ui.off {
                    top
                } else if top + ROW_H + HEAD_H > ui.off + LIST_H {
                    top + ROW_H + HEAD_H - LIST_H
                } else {
                    ui.off
                };
                operation::scroll_to(
                    Id::from(LIST_ID),
                    AbsoluteOffset {
                        x: 0.0,
                        y: target.max(0.0),
                    },
                )
            }
            "enter" => self.search_msg(SMsg::Enter),
            _ => Task::none(),
        }
    }

    pub(super) fn search_scrolled(&mut self, off: f32) {
        if let Some(ui) = self.search.as_mut() {
            ui.off = off;
        }
    }

    /// The query field (a real `text_input`).
    pub(super) fn search_input(&self) -> El<'_> {
        let q = self.search.as_deref().map_or("", |u| u.s.query.as_str());
        text_input("Movies, shows, people, collections", q)
            .id(Id::from(INPUT_ID))
            .on_input(|q| Message::Search(SMsg::Input(q)))
            .on_submit(Message::Search(SMsg::Enter))
            .size(t::BODY_LG)
            .padding(0)
            .style(|_, _| text_input::Style {
                background: iced::Background::Color(t::NONE),
                border: border(t::NONE, 0.0, 0.0),
                icon: t::text_secondary(),
                placeholder: t::text_tertiary(),
                value: t::text(),
                selection: t::accent_tint(),
            })
            .into()
    }

    pub(super) fn search_overlay(&self) -> El<'_> {
        let Some(ui) = self.search.as_deref() else {
            return space().into();
        };
        let s = &ui.s;
        let several = self.several_sources();
        let mut list = column![];
        if s.query.trim().is_empty() {
            list = list.push(container(label("Recent searches")).padding([t::S3, t::S4]));
            if self.settings.recent_searches.is_empty() {
                list = list.push(note("Type to search your library. Press Esc to close."));
            }
            for (i, r) in self.settings.recent_searches.iter().enumerate() {
                list = list.push(hit_row(
                    row![mono("\u{21BB}"), text(r.clone()).size(t::BODY)]
                        .spacing(t::S3)
                        .align_y(Alignment::Center)
                        .into(),
                    i == ui.recent_sel,
                    Message::Search(SMsg::Recent(i)),
                ));
            }
        } else if s.failed {
            list = list.push(note(
                "Search couldn't reach your sources. Keep typing to try again.",
            ));
        } else if s.empty() {
            if self.rq_rows().is_empty() {
                list = list.push(note("No results. Try fewer or different words."));
            }
        } else if s.results.is_empty() {
            list = list.push(note(if s.loading { "Searching" } else { "" }));
        }
        let mut flat = 0usize;
        for (g, items) in &s.results {
            list = list.push(container(label(g.label())).height(HEAD_H).padding(Padding {
                left: t::S4,
                top: t::S3,
                ..Padding::ZERO
            }));
            for m in items {
                let i = flat;
                flat += 1;
                let mut sub: Vec<String> = [
                    m.sub.clone().unwrap_or_default(),
                    m.year.map(|y| y.to_string()).unwrap_or_default(),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
                if several {
                    let copies = ui
                        .copies
                        .get(&(m.origin, m.id.clone()))
                        .copied()
                        .unwrap_or(1);
                    sub.extend(mark_for(&self.catalog, m, copies));
                }
                list = list.push(hit_row(
                    row![
                        tile(
                            &self.pic(m, Art::Poster, 32),
                            &m.title,
                            Length::Fixed(32.0),
                            Length::Fixed(46.0)
                        ),
                        column![
                            text(m.title.clone())
                                .size(t::BODY)
                                .font(t::semibold())
                                .wrapping(text::Wrapping::None),
                            mono(sub.join(" \u{b7} ")).wrapping(text::Wrapping::None),
                        ]
                        .width(Length::Fill)
                        .clip(true),
                    ]
                    .spacing(t::S3)
                    .align_y(Alignment::Center)
                    .into(),
                    i == s.sel,
                    Message::Search(SMsg::Pick(i)),
                ));
            }
        }
        for el in self.rq_search_group(flat, s.sel) {
            list = list.push(el);
        }
        let input = self.search_input();
        let head = container(
            row![
                mono("SEARCH"),
                container(input).width(Length::Fill),
                mono(if s.loading { "searching" } else { "esc" }),
            ]
            .spacing(t::S3)
            .align_y(Alignment::Center),
        )
        .height(56)
        .padding([0.0, t::S4])
        .center_y(56);
        let panel = opaque(
            container(column![
                head,
                container(space().height(1))
                    .width(Length::Fill)
                    .style(style::rule),
                scrollable::Scrollable::new(list.width(Length::Fill))
                    .id(Id::from(LIST_ID))
                    .on_scroll(|v| Message::SearchScrolled(v.absolute_offset().y))
                    .style(style::scroll)
                    .height(Length::Shrink),
            ])
            .width(720)
            .max_height(520)
            .style(style::panel),
        );
        Stack::with_children(vec![
            mouse_area(
                container(space())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(style::fill(iced::Color::from_rgba8(0x07, 0x08, 0x0c, 0.69))),
            )
            .on_press(Message::Search(SMsg::Close))
            .into(),
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .padding(Padding {
                    top: 80.0,
                    ..Padding::ZERO
                })
                .into(),
        ])
        .into()
    }
}

pub(super) fn hit_row<'a>(content: El<'a>, on: bool, m: Message) -> El<'a> {
    Press::new(content)
        .width(Length::Fill)
        .padding([t::S2, t::S4])
        .on_press(m)
        .style(move |s| Style {
            background: if on {
                Some(t::accent_tint().into())
            } else if matches!(s, Status::Hovered | Status::Pressed) {
                Some(t::GLASS.into())
            } else {
                None
            },
            text_color: t::text(),
            ..Default::default()
        })
        .into()
}

fn note<'a>(s: &str) -> El<'a> {
    container(text(s.to_string()).size(t::BODY).color(t::text_secondary()))
        .padding(t::S4)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_core::source::Kind;

    fn m(kind: Kind, id: &str) -> Media {
        Media {
            id: id.into(),
            title: id.into(),
            kind,
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        }
    }

    #[test]
    fn heading_rows_sit_above_the_hit_that_follows_them() {
        let mut s = Search::default();
        s.query = "x".into();
        let e = s.edited();
        s.due(e);
        s.arrived(
            e,
            "x",
            vec![
                m(Kind::Movie, "a"),
                m(Kind::Movie, "b"),
                m(Kind::Series, "c"),
            ],
        );
        // Movies (2 hits) then Shows (1): hit 0 and 1 have one heading above, hit 2 has two.
        assert_eq!(headings_before(&s, 0), 1);
        assert_eq!(headings_before(&s, 1), 1);
        assert_eq!(headings_before(&s, 2), 2);
    }
}
