//! Settings: a sidebar of sections (with a search field that filters rows across all of them)
//! beside the selected section, whose rows sit in rounded cards. A toggle flips on click, a
//! multi-choice row opens a drop-down; changes apply and save at once. Narrow windows show the
//! section list and drill into one section (iOS style).
use super::App;
use super::message::Message;
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::popover::Popover;
use crate::widget::press::Press;
use crate::widget::{empty_state, icon, menu_row, mono, switch, tracked, value_chip};
use iced::border::Radius;
use iced::widget::button::{Status, Style};
use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{
    Column, Stack, column, container, row, scrollable as scroll, space, text, text_input,
};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_arr::Service;
use nocturne_core::dropdown::Outcome;
use nocturne_core::settings::{self as s, Section};

type El<'a> = Element<'a, Message>;

const PAGE_ID: &str = "settings-scroll";
const MENU_ID: &str = "settings-menu";
const SEARCH_ID: &str = "settings-search";
/// Below this window width the sidebar and the section share one column (list, then drill in).
const NARROW: f32 = 880.0;
/// Estimated heights used to keep the selected row in view (help text wraps, so approximate).
const ROW_H: f32 = 66.0;
const GROUP_H: f32 = 64.0;
const TITLE_H: f32 = 150.0;
const MENU_ROW_H: f32 = 37.0;

/// Icon of a sidebar section.
fn sec_icon(sec: Section) -> &'static str {
    match sec {
        Section::General => "gear",
        Section::Appearance => "palette",
        Section::Home => "home",
        Section::Playback => "play",
        Section::Sources => "server",
        Section::Sync => "sync",
        Section::Requests => "inbox",
        Section::Gateway => "broadcast",
        Section::Servers => "layers",
        Section::Privacy => "lock",
        Section::Updates => "download",
        Section::About => "info",
    }
}

/// The groups of a section: a new group starts at every row with a `head`. A caption that would
/// only repeat the section title is left out.
fn groups(sec: Section) -> Vec<(Option<&'static str>, Vec<usize>)> {
    let mut out: Vec<(Option<&'static str>, Vec<usize>)> = vec![];
    for &i in sec.rows() {
        let h = s::head(i);
        if out.is_empty() || h.is_some() {
            out.push((h.filter(|h| !sec.title().starts_with(*h)), vec![i]));
        } else if let Some(last) = out.last_mut() {
            last.1.push(i);
        }
    }
    out
}

/// Estimated top of row `i` inside the section's scroll content.
fn row_top(sec: Section, i: usize) -> f32 {
    let mut y = TITLE_H;
    for (k, (_, rows)) in groups(sec).iter().enumerate() {
        if k > 0 {
            y += GROUP_H;
        }
        for r in rows {
            if *r == i {
                return y;
            }
            y += ROW_H;
        }
    }
    y
}

/// The icon tile of a sidebar entry or a section title: accent-filled when selected.
fn tile<'a>(sec: Section, on: bool, size: f32) -> El<'a> {
    container(icon(
        sec_icon(sec),
        size * 0.56,
        if on { t::on_accent() } else { t::text() },
    ))
    .center(size)
    .style(move |_| container::Style {
        background: Some(if on { t::accent() } else { t::raised() }.into()),
        border: border(
            if on {
                t::accent()
            } else {
                t::hairline_strong()
            },
            1.0,
            size * 0.26,
        ),
        ..Default::default()
    })
    .into()
}

/// Small caps caption above a card.
fn caption<'a>(s: &str) -> El<'a> {
    container(
        text(tracked(s))
            .shaping(iced::widget::text::Shaping::Advanced)
            .size(t::CAPTION)
            .font(t::bold())
            .color(t::text_secondary()),
    )
    .padding(Padding {
        left: t::S4,
        bottom: t::S2,
        ..Padding::ZERO
    })
    .into()
}

/// Rows stacked in a raised card with inset dividers.
fn card<'a>(items: Vec<El<'a>>) -> El<'a> {
    let n = items.len();
    let mut col = Column::new();
    for (k, it) in items.into_iter().enumerate() {
        col = col.push(it);
        if k + 1 < n {
            col = col.push(
                container(
                    container(space().height(1))
                        .width(Length::Fill)
                        .style(style::rule),
                )
                .padding([0.0, t::S4]),
            );
        }
    }
    container(col).width(Length::Fill).style(style::card).into()
}

/// Corner radius of item `k` of `n` in a card, so a hover fill follows the card's corners.
fn corners(k: usize, n: usize) -> Radius {
    let r = t::r_panel();
    let mut x = Radius::new(0.0);
    if k == 0 {
        x = x.top(r);
    }
    if k + 1 == n {
        x = x.bottom(r);
    }
    x
}

/// A row of a card that opens another page: title, one line, a chevron.
fn link_row<'a>(title: &str, help: &str, meta: String, k: usize, n: usize, m: Message) -> El<'a> {
    Press::new(
        row![
            column![
                text(title.to_string()).size(t::BODY).font(t::semibold()),
                text(help.to_string())
                    .size(t::CONTROL_TEXT)
                    .color(t::text_secondary()),
            ]
            .spacing(3)
            .width(Length::Fill),
            text(meta).size(t::CONTROL_TEXT).color(t::text_secondary()),
            text("\u{203A}").size(t::HEADING).color(t::text_secondary()),
        ]
        .spacing(t::S4)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([t::S3 + 2.0, t::S4 + 2.0])
    .on_press(m)
    .style(move |st| Style {
        background: match st {
            Status::Hovered => Some(t::GLASS.into()),
            Status::Pressed => Some(t::accent_tint().into()),
            _ => None,
        },
        text_color: t::text(),
        border: iced::Border {
            radius: corners(k, n),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

impl App {
    fn set_narrow(&self) -> bool {
        self.win.width < NARROW
    }

    fn set_searching(&self) -> bool {
        !self.set_q.trim().is_empty()
    }

    /// The rows the arrow keys walk: the search results while searching, else every row in
    /// sidebar order (crossing into the next section shows it).
    fn set_list(&self) -> Vec<usize> {
        if self.set_searching() {
            s::search(&self.settings.rows(), &self.set_q)
        } else {
            s::order()
        }
    }

    /// Settings opened from a menu: first section, no search, nothing open.
    pub(super) fn settings_reset(&mut self) {
        self.settings_open = true;
        self.settings_sel = Section::General.rows()[0];
        self.set_sec = Section::General;
        self.set_q.clear();
        self.set_drill = false;
        self.dd = None;
        self.set_off = 0.0;
    }

    pub(super) fn settings_section(&mut self, sec: Section) -> Task<Message> {
        self.dd = None;
        self.set_q.clear();
        self.set_sec = sec;
        self.set_drill = true;
        if let Some(&r) = sec.rows().first() {
            self.settings_sel = r;
        }
        let top = operation::scroll_to(PAGE_ID, AbsoluteOffset { x: 0.0, y: 0.0 });
        if sec == Section::Servers {
            return Task::batch([top, self.ns_refresh()]);
        }
        top
    }

    pub(super) fn settings_query(&mut self, q: String) -> Task<Message> {
        self.dd = None;
        self.set_q = q;
        if !self.set_searching() {
            self.set_sec = Section::of(self.settings_sel);
        } else if let Some(&r) = self.set_list().first() {
            self.settings_sel = r;
        }
        operation::scroll_to(PAGE_ID, AbsoluteOffset { x: 0.0, y: 0.0 })
    }

    pub(super) fn settings_page(&self) -> El<'_> {
        let rows = self.settings.rows();
        let hits = self.set_searching().then(|| s::search(&rows, &self.set_q));
        let search = self.set_search_field();
        let body: El = if self.set_narrow() {
            let rest = if self.set_drill || hits.is_some() {
                self.set_content(&rows, hits.as_deref(), true)
            } else {
                self.set_sidebar(None, true)
            };
            column![container(search).padding([t::S4, t::S6]), rest].into()
        } else {
            let side = container(column![
                container(text("Settings").size(t::HEADING).font(t::bold())).padding(Padding {
                    top: t::S6,
                    left: t::S4 + 2.0,
                    ..Padding::ZERO
                }),
                container(search).padding(Padding {
                    top: t::S3,
                    bottom: t::S3,
                    left: t::S4,
                    right: t::S4,
                }),
                self.set_sidebar(hits.as_deref(), false),
            ])
            .width((self.win.width * 0.2).clamp(248.0, 300.0))
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(t::surface().scale_alpha(0.6).into()),
                ..Default::default()
            });
            let line = container(space().width(1).height(Length::Fill)).style(style::rule);
            row![side, line, self.set_content(&rows, hits.as_deref(), false)].into()
        };
        // The panes start below the header, so nothing scrolls under it.
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: t::header_h(),
                ..Padding::ZERO
            })
            .into()
    }

    fn set_search_field(&self) -> El<'_> {
        container(
            row![
                icon("search", 15.0, t::text_secondary()),
                text_input("Search settings", &self.set_q)
                    .id(SEARCH_ID)
                    .on_input(Message::SetQuery)
                    .size(t::BODY - 1.0)
                    .padding([t::S2, 0.0])
                    .style(|th, st| {
                        let mut x = style::input(th, st);
                        x.border.width = 0.0;
                        x
                    }),
                mono(if self.set_q.is_empty() { "/" } else { "" }),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        )
        .padding([0.0, t::S3])
        .center_y(38)
        .style(|_| container::Style {
            background: Some(t::control().into()),
            border: border(t::edge(), 1.0, t::R_CTL + 2.0),
            ..Default::default()
        })
        .into()
    }

    /// The section list: icon tile and title per section; while searching, the number of matches
    /// (sections without one are dimmed). `full` is the narrow window's list (chevrons, no
    /// selection mark).
    fn set_sidebar(&self, hits: Option<&[usize]>, full: bool) -> El<'_> {
        let col = column![].spacing(2);
        let mut items: Vec<El> = vec![];
        let count = Section::ALL.len();
        for (k, sec) in Section::ALL.into_iter().enumerate() {
            let n = hits.map(|h| h.iter().filter(|r| Section::of(**r) == sec).count());
            let on = !full && hits.is_none() && sec == self.set_sec;
            let dim = n == Some(0);
            let mut r = row![
                tile(sec, on, if full { 32.0 } else { 28.0 }),
                text(sec.title())
                    .size(if full { t::BODY } else { t::BODY - 1.0 })
                    .font(if on { t::semibold() } else { t::medium() })
                    .color(match (on, dim) {
                        (true, _) => t::text(),
                        (_, true) => t::text_tertiary().scale_alpha(0.6),
                        _ => t::text_secondary(),
                    })
                    .width(Length::Fill),
            ]
            .spacing(t::S3)
            .align_y(Alignment::Center);
            if let Some(n) = n.filter(|n| *n > 0) {
                r = r.push(mono(n.to_string()));
            }
            if full {
                r = r.push(text("\u{203A}").size(t::HEADING).color(t::text_secondary()));
            }
            let item = Press::new(r)
                .width(Length::Fill)
                .padding([t::S2, t::S2 + 2.0])
                .on_press(Message::SetSection(sec))
                .style(move |st| Style {
                    background: match st {
                        _ if on => Some(t::accent_tint().into()),
                        Status::Hovered => Some(t::GLASS.into()),
                        Status::Pressed => Some(t::accent_tint().into()),
                        _ => None,
                    },
                    text_color: t::text(),
                    border: if full {
                        iced::Border {
                            radius: corners(k, count),
                            ..Default::default()
                        }
                    } else {
                        border(t::NONE, 0.0, t::R_CTL + 2.0)
                    },
                    ..Default::default()
                });
            items.push(item.into());
        }
        // The narrow list is one grouped card, iOS style.
        let col = if full {
            col.push(card(items))
        } else {
            col.extend(items)
        };
        scroll(container(col).padding(if full {
            Padding::from([0.0, t::S6])
        } else {
            Padding {
                top: t::S2,
                bottom: t::S6,
                left: t::S3,
                right: t::S3,
            }
        }))
        .style(style::scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// The right pane: the selected section (title, description, cards) or the search results.
    fn set_content(&self, rows: &[s::Row], hits: Option<&[usize]>, narrow: bool) -> El<'_> {
        let mut col = column![].spacing(t::S6).max_width(860);
        if narrow {
            col = col.push(
                Press::new(
                    row![
                        text("\u{2039}").size(t::HEADING).color(t::accent_text()),
                        text("Settings").size(t::BODY).color(t::accent_text()),
                    ]
                    .spacing(t::S2)
                    .align_y(Alignment::Center),
                )
                .width(Length::Shrink)
                .padding([t::S1, t::S2])
                .on_press(Message::SetBack)
                .style(|st| Style {
                    background: matches!(st, Status::Hovered | Status::Pressed)
                        .then_some(t::GLASS.into()),
                    text_color: t::text(),
                    border: border(t::NONE, 0.0, t::R_CTL),
                    ..Default::default()
                }),
            );
        }
        match hits {
            Some(h) => {
                col = col.push(self.set_title(
                    None,
                    "Search",
                    &match h.len() {
                        0 => format!("Nothing matches \u{201C}{}\u{201D}.", self.set_q.trim()),
                        1 => "1 setting matches.".to_string(),
                        n => format!("{n} settings match."),
                    },
                ));
                if h.is_empty() {
                    col = col.push(empty_state(
                        "search",
                        "No settings match",
                        "Try a shorter word, such as subtitles, theme, gateway or Trakt. Esc clears the search.",
                    ));
                }
                for sec in Section::ALL {
                    let rs: Vec<usize> = h
                        .iter()
                        .copied()
                        .filter(|r| Section::of(*r) == sec)
                        .collect();
                    if rs.is_empty() {
                        continue;
                    }
                    let n = rs.len();
                    col = col.push(column![
                        caption(sec.title()),
                        card(
                            rs.iter()
                                .enumerate()
                                .map(|(k, &i)| self.set_row(i, &rows[i], k, n))
                                .collect()
                        ),
                    ]);
                }
            }
            None => {
                let sec = self.set_sec;
                col = col.push(self.set_title(Some(sec), sec.title(), sec.blurb()));
                let links: Vec<(&str, &str, String, Message)> = match sec {
                    Section::General => vec![(
                        "Run setup again",
                        "Walk through where your media lives and how NOCTURNE behaves.",
                        String::new(),
                        Message::RunSetup,
                    )],
                    Section::Appearance => vec![(
                        "Themes",
                        "Colours, corner radii and a font from a CSS file, for the whole app or one server.",
                        String::new(),
                        Message::Themes(super::themes::Msg::Open),
                    )],
                    Section::Sources => vec![(
                        "Sources",
                        "Servers and local folders, their order, and which are on.",
                        self.sources.profiles.len().to_string(),
                        Message::OpenSources,
                    )],
                    _ => vec![],
                };
                if !links.is_empty() {
                    let n = links.len();
                    col = col.push(card(
                        links
                            .into_iter()
                            .enumerate()
                            .map(|(k, (a, b, m, msg))| link_row(a, b, m, k, n, msg))
                            .collect(),
                    ));
                }
                for (head, rs) in groups(sec) {
                    let n = rs.len();
                    let mut g = column![];
                    if let Some(h) = head {
                        g = g.push(caption(h));
                    }
                    if rs.first() == Some(&0) {
                        g = g.push(
                            container(
                                text(s::WRITES_NOTE)
                                    .size(t::CONTROL_TEXT + 1.0)
                                    .line_height(1.5)
                                    .color(t::text_secondary()),
                            )
                            .padding(Padding {
                                left: t::S4,
                                right: t::S4,
                                bottom: t::S3,
                                ..Padding::ZERO
                            }),
                        );
                    }
                    g = g.push(card(
                        rs.iter()
                            .enumerate()
                            .map(|(k, &i)| self.set_row(i, &rows[i], k, n))
                            .collect(),
                    ));
                    col = col.push(g);
                }
                match sec {
                    Section::Gateway => col = col.push(self.gateway_panel()),
                    Section::Servers => col = col.push(self.servers_panel()),
                    Section::About => col = col.push(self.set_about()),
                    _ => {}
                }
            }
        }
        let pad = if narrow { t::S6 } else { t::S8 + t::S4 };
        scroll(container(col).center_x(Length::Fill).padding(Padding {
            top: if narrow { t::S2 } else { t::S8 },
            bottom: t::S8 * 2.0,
            left: pad,
            right: pad,
        }))
        .id(PAGE_ID)
        .on_scroll(|v| Message::SetScrolled(v.absolute_offset().y, v.bounds().height))
        .style(style::scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn set_title<'a>(&self, sec: Option<Section>, title: &str, blurb: &str) -> El<'a> {
        let words = column![
            text(title.to_string())
                .size(t::TITLE)
                .font(t::bold())
                .color(t::text()),
            text(blurb.to_string())
                .size(t::BODY)
                .line_height(1.45)
                .color(t::text_secondary()),
        ]
        .spacing(t::S1);
        let mut r = row![].spacing(t::S4).align_y(Alignment::Center);
        if let Some(sec) = sec {
            r = r.push(tile(sec, false, 48.0));
        }
        container(r.push(words))
            .padding(Padding {
                bottom: t::S2,
                ..Padding::ZERO
            })
            .into()
    }

    /// One settings row inside a card: label and help on the left, the control on the right. The
    /// selected row shows its whole help text and an accent mark.
    fn set_row(&self, i: usize, r: &s::Row, k: usize, n: usize) -> El<'_> {
        let on = i == self.settings_sel;
        let help = text(if on {
            r.hint.to_string()
        } else {
            s::summary(r.hint).to_string()
        })
        .size(t::CONTROL_TEXT)
        .line_height(1.45)
        .color(t::text_secondary());
        let text_col = column![text(r.label).size(t::BODY).font(t::semibold()), help]
            .spacing(3)
            .width(Length::Fill);
        let chip = container(self.setting_chip(i, &r.value, on)).align_x(Alignment::End);
        let content = row![space().width(0).height(40), text_col, chip]
            .spacing(t::S6)
            .align_y(Alignment::Center);
        let press = Press::new(content)
            .width(Length::Fill)
            .padding([t::S3, t::S4 + 2.0])
            .on_press(Message::SetRow(i))
            .style(move |st| Style {
                background: match st {
                    Status::Hovered | Status::Pressed => Some(t::GLASS.into()),
                    _ if on => Some(t::accent_tint().into()),
                    _ => None,
                },
                text_color: t::text(),
                border: iced::Border {
                    radius: corners(k, n),
                    ..Default::default()
                },
                ..Default::default()
            });
        if !on {
            return press.into();
        }
        let mark = container(
            container(space().width(3).height(Length::Fill))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(t::accent().into()),
                    border: border(t::NONE, 0.0, 2.0),
                    ..Default::default()
                }),
        )
        .height(Length::Fill)
        .padding([t::S3, 0.0]);
        Stack::from_vec(vec![press.into(), mark.into()])
            .width(Length::Fill)
            .height(Length::Shrink)
            .into()
    }

    /// Settings > About.
    fn set_about(&self) -> El<'_> {
        let line = |a: &str, b: String| -> El<'_> {
            row![
                text(a.to_string())
                    .size(t::BODY)
                    .color(t::text_secondary())
                    .width(140),
                text(b).size(t::BODY).color(t::text()),
            ]
            .spacing(t::S4)
            .into()
        };
        let items = column![
            line("Version", env!("CARGO_PKG_VERSION").to_string()),
            line("Licence", "GPL-3.0-or-later".into()),
            line("Website", "nocturne.ifsvivek.in".into()),
        ]
        .spacing(t::S3);
        column![
            container(
                column![
                    crate::widget::wordmark(t::HEADING),
                    text("A cinematic home for your own media: local folders and your media servers in one searchable library.")
                        .size(t::BODY)
                        .line_height(1.5)
                        .color(t::text_secondary()),
                    items,
                ]
                .spacing(t::S4)
            )
            .padding(t::S6)
            .width(Length::Fill)
            .style(style::card),
            container(
                text("NOCTURNE is not affiliated with the Jellyfin project. Third-party licences are listed in THIRD_PARTY_NOTICES.md.")
                    .size(t::CONTROL_TEXT)
                    .color(t::text_tertiary())
            )
            .padding([0.0, t::S4]),
        ]
        .spacing(t::S3)
        .into()
    }

    /// The value chip of Settings row `i`; for a multi-choice row it carries a caret and, while
    /// open, its drop-down menu (an overlay, so the page's scroll never clips it).
    fn setting_chip(&self, i: usize, value: &str, on: bool) -> El<'_> {
        if i == s::TRAKT_ROW {
            return value_chip(
                &self.trakt_chip_label(),
                false,
                on,
                false,
                Message::SetRow(i),
            );
        }
        if i == s::SEERR_ROW {
            return value_chip(&self.rq_chip_label(), false, on, false, Message::SetRow(i));
        }
        if i == s::RADARR_ROW || i == s::SONARR_ROW {
            let svc = if i == s::RADARR_ROW {
                Service::Radarr
            } else {
                Service::Sonarr
            };
            return value_chip(
                &self.arr_chip_label(svc),
                false,
                on,
                false,
                Message::SetRow(i),
            );
        }
        let Some((labels, cur)) = self.settings.choices(i) else {
            return switch(value == "On", on, Message::SetRow(i));
        };
        let open = self.dd.as_ref().is_some_and(|d| d.row == i);
        let chip = value_chip(value, true, on, open, Message::SetRow(i));
        let popup = open.then(|| {
            let sel = self.dd.as_ref().map_or(0, |d| d.sel);
            let items: Vec<El> = labels
                .into_iter()
                .enumerate()
                .map(|(k, name)| {
                    menu_row(
                        row![
                            text(name).size(t::BODY).color(if k == cur {
                                t::accent_text()
                            } else {
                                t::text()
                            }),
                            space().width(Length::Fill),
                            mono(if k == cur { "\u{2713}" } else { "" }),
                        ]
                        .spacing(t::S3),
                        k == sel,
                        false,
                        Message::SetChoose(i, k),
                    )
                })
                .collect();
            container(
                scroll(column(items).spacing(2))
                    .id(MENU_ID)
                    .style(style::scroll),
            )
            .width(220)
            .max_height(300)
            .padding(t::S2)
            .style(style::panel)
            .into()
        });
        Popover::new(chip, popup, Message::SetClose).into()
    }

    /// Make sure row `i` is inside the visible part of the section pane.
    fn reveal_row(&self, i: usize) -> Task<Message> {
        let top = if self.set_searching() {
            let at = self.set_list().iter().position(|r| *r == i).unwrap_or(0);
            TITLE_H + at as f32 * ROW_H
        } else {
            row_top(self.set_sec, i)
        };
        // The selected row shows its whole help text: leave room for two rows.
        let bottom = top + ROW_H * 2.0;
        let (off, h) = (self.set_off, self.set_h);
        let y = if top < off {
            (top - t::S8).max(0.0)
        } else if bottom > off + h {
            bottom - h + t::S8
        } else {
            return Task::none();
        };
        operation::scroll_to(PAGE_ID, AbsoluteOffset { x: 0.0, y })
    }

    fn reveal_menu(&self, sel: usize) -> Task<Message> {
        operation::scroll_to(
            MENU_ID,
            AbsoluteOffset {
                x: 0.0,
                y: (sel as f32 * MENU_ROW_H - 3.0 * MENU_ROW_H).max(0.0),
            },
        )
    }

    /// Settings row `i` stepped; applies at once and is saved.
    fn cycle_setting(&mut self, i: usize, dir: i32) -> Task<Message> {
        if i == s::PROFILE_ROW {
            let n = self.settings.profiles.list.len() + 1;
            if n > 2 {
                let at = (self.settings.profiles.current() + n)
                    .wrapping_add_signed(dir.signum() as isize)
                    % n;
                self.set_default_profile(at);
            }
            return Task::none();
        }
        self.settings.cycle(i, dir);
        self.settings_changed()
    }

    /// Entry `idx` of row `i`'s drop-down chosen.
    pub(super) fn choose_setting(&mut self, i: usize, idx: usize) -> Task<Message> {
        if i == s::PROFILE_ROW {
            self.set_default_profile(idx);
            return Task::none();
        }
        self.settings.choose(i, idx);
        self.settings_changed()
    }

    /// Remembered profiles of this server and the default one, for the Settings row.
    pub(super) fn refresh_profiles(&mut self) {
        self.settings.profiles = match self.session() {
            Some((url, _)) if !self.dev => {
                let all = nocturne_session::Accounts::load().unwrap_or_default();
                s::Profiles {
                    list: all
                        .for_server(url)
                        .iter()
                        .map(|a| (a.user_id.clone(), a.user_name.clone()))
                        .collect(),
                    default: all.default_for(url).map(|a| a.user_id.clone()),
                }
            }
            _ => s::Profiles::default(),
        };
    }

    /// "Default profile" drop-down entry `idx`: 0 asks every time, otherwise that profile opens at launch.
    fn set_default_profile(&mut self, idx: usize) {
        if let Some((url, _)) = self.session() {
            let id = self.settings.profiles.id_at(idx).map(str::to_string);
            nocturne_session::remember_default(url, id.as_deref());
        }
        self.refresh_profiles();
    }

    fn settings_changed(&mut self) -> Task<Message> {
        self.settings.save();
        crate::motion::set_reduced(
            self.settings
                .reduce_motion
                .reduced(crate::platform::system_reduces_motion()),
        );
        self.apply_write_settings();
        if self.themes.remote() != self.settings.remote_css {
            self.themes.set_remote(self.settings.remote_css);
            self.th_reapply();
        }
        Task::batch([
            self.refresh_couch(),
            self.probe_monitor(),
            self.gateway_apply(),
        ])
    }

    /// Open the drop-down of a multi-choice row (a toggle row is flipped instead).
    pub(super) fn open_setting(&mut self, i: usize) -> Task<Message> {
        self.settings_sel = i;
        if i == s::TRAKT_ROW {
            return self.trakt_msg(super::trakt::Msg::Open);
        }
        if i == s::SEERR_ROW {
            return self.rq_msg(super::requests::Msg::Open);
        }
        if i == s::RADARR_ROW || i == s::SONARR_ROW {
            let svc = if i == s::RADARR_ROW {
                Service::Radarr
            } else {
                Service::Sonarr
            };
            return self.disc_msg(super::discover::Msg::ArrOpen(svc));
        }
        let Some((labels, cur)) = self.settings.choices(i) else {
            return self.cycle_setting(i, 1);
        };
        self.dd = nocturne_core::dropdown::Dropdown::open(i, labels.len(), cur);
        self.reveal_menu(cur)
    }

    pub(super) fn settings_key(&mut self, key: &str, shift: bool) -> Task<Message> {
        if let Some(d) = &mut self.dd {
            match d.key(key) {
                Outcome::Stay => {
                    let sel = d.sel;
                    return self.reveal_menu(sel);
                }
                Outcome::Close => self.dd = None,
                Outcome::Pick(idx) => {
                    let row = d.row;
                    self.dd = None;
                    return self.choose_setting(row, idx);
                }
            }
            return Task::none();
        }
        let searching = self.set_searching();
        // About has no rows: the value keys do nothing there.
        let has_rows = searching || !self.set_sec.rows().is_empty();
        match key {
            "escape" | "backspace" if searching => {
                self.set_q.clear();
                self.set_sec = Section::of(self.settings_sel);
            }
            "escape" | "backspace" if self.set_narrow() && self.set_drill => self.set_drill = false,
            "escape" | "backspace" => self.settings_open = false,
            "/" => return operation::focus(SEARCH_ID),
            "tab" if !searching => {
                let all = Section::ALL;
                let n = all.len();
                let at = all.iter().position(|x| *x == self.set_sec).unwrap_or(0);
                return self.settings_section(
                    all[if shift {
                        (at + n - 1) % n
                    } else {
                        (at + 1) % n
                    }],
                );
            }
            "up" | "down" | "pageup" | "pagedown" | "home" | "end" => {
                let list = self.set_list();
                let to = if has_rows {
                    s::nav_row(&list, self.settings_sel, key)
                } else {
                    // From a section without rows (Servers, About), up lands on the last row before it, down on the first after it.
                    let at = Section::ALL.iter().position(|x| *x == self.set_sec);
                    let (before, after) = Section::ALL.split_at(at.unwrap_or(0));
                    let mut before = before.iter().flat_map(|x| x.rows().iter().copied());
                    let mut after = after.iter().flat_map(|x| x.rows().iter().copied());
                    match key {
                        "up" | "pageup" => before.next_back(),
                        "down" | "pagedown" => after.next(),
                        "home" => list.first().copied(),
                        _ => list.last().copied(),
                    }
                };
                let Some(r) = to else {
                    return Task::none();
                };
                self.settings_sel = r;
                let sec = Section::of(r);
                if !searching && sec != self.set_sec {
                    // Crossing into another section shows it, from the side the key came from.
                    self.set_sec = sec;
                    self.set_drill = true;
                    let back = matches!(key, "up" | "pageup" | "end");
                    return operation::scroll_to(
                        PAGE_ID,
                        AbsoluteOffset {
                            x: 0.0,
                            y: if back { 1e7 } else { 0.0 },
                        },
                    );
                }
                return self.reveal_row(r);
            }
            "left" if has_rows => return self.cycle_setting(self.settings_sel, -1),
            "right" if has_rows => return self.cycle_setting(self.settings_sel, 1),
            "enter" | "space" if has_rows => return self.open_setting(self.settings_sel),
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_cover_each_section_and_estimates_grow() {
        for sec in Section::ALL {
            let all: Vec<usize> = groups(sec).into_iter().flat_map(|(_, r)| r).collect();
            assert_eq!(all, sec.rows());
        }
        // Playback's first caption would repeat the title, so it is left out.
        assert_eq!(groups(Section::Playback)[0].0, None);
        let rows = Section::Playback.rows();
        assert!(row_top(Section::Playback, rows[1]) > row_top(Section::Playback, rows[0]));
    }
}
