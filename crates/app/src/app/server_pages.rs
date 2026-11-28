//! The active NOCTURNE server's module pages (Downloads, Library, Requests, Storage), each shown only while its module runs. State and calls are in `servers.rs`; this file paints, in the Settings cards' style.
use super::App;
use super::message::Message;
use super::servers::{Msg, Tab};
use crate::style;
use crate::theme as t;
use crate::widget::{action_button, chip, empty_state, mono, progress_bar, switch, tab};
use iced::widget::{Column, column, container, row, scrollable as scroll, space, text, text_input};
use iced::{Alignment, Element, Length, Padding};
use nocturne_core::download::bytes;
use nocturne_core::server::types::{MediaKind, TorrentState};
use nocturne_core::server::{Candidate, GrabState, QueueItem, RequestStatus, Title};
use nocturne_core::server_client::{QueueAct, StorageRoot, eta};

type El<'a> = Element<'a, Message>;

fn m(x: Msg) -> Message {
    Message::Servers(x)
}

fn small<'a>(s: impl Into<String>) -> iced::widget::Text<'a> {
    text(s.into())
        .size(t::CONTROL_TEXT)
        .color(t::text_secondary())
}

/// Rows stacked in one raised card with hairline dividers (the Settings card).
fn card<'a>(items: Vec<El<'a>>) -> El<'a> {
    let n = items.len();
    let mut col = Column::new();
    for (k, it) in items.into_iter().enumerate() {
        col = col.push(
            container(it)
                .padding([t::S3 + 2.0, t::S4 + 2.0])
                .width(Length::Fill),
        );
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

fn kind_label(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Movie => "Movies",
        MediaKind::Series => "Shows",
        MediaKind::Music => "Music",
        MediaKind::Book => "Books",
    }
}

fn year(y: Option<u16>) -> String {
    y.map_or_else(String::new, |y| format!(" ({y})"))
}

impl App {
    pub(super) fn ns_page(&self) -> El<'_> {
        let s = &self.ns;
        let tab_now = s.page.unwrap_or(Tab::Downloads);
        let tabs = s.tabs().into_iter().fold(row![].spacing(t::S1), |r, x| {
            r.push(tab(x.label(), x == tab_now, m(Msg::Open(x))))
        });
        let live = if s.live {
            ("\u{25CF} Live", t::accent_text())
        } else {
            ("\u{25CB} Connecting", t::text_tertiary())
        };
        let head = column![
            row![
                column![
                    text(tab_now.label()).size(t::TITLE).font(t::bold()),
                    text(format!("NOCTURNE server \u{00B7} {}", s.active_name()))
                        .size(t::BODY)
                        .color(t::text_secondary()),
                ]
                .spacing(t::S1)
                .width(Length::Fill),
                text(live.0).size(t::CONTROL_TEXT).color(live.1),
            ]
            .align_y(Alignment::Center),
            tabs,
        ]
        .spacing(t::S4);
        let mut col = column![head].spacing(t::S6).max_width(1000);
        if let Some(n) = &s.page_note {
            col = col.push(
                container(small(n.clone()).color(t::warn()))
                    .padding([t::S3, t::S4])
                    .width(Length::Fill)
                    .style(style::card),
            );
        }
        col = col.push(match tab_now {
            Tab::Downloads => self.ns_downloads(),
            Tab::Library => self.ns_library(),
            Tab::Requests => self.ns_requests(),
            Tab::Storage => self.ns_storage(),
        });
        let body = scroll(container(col).center_x(Length::Fill).padding(Padding {
            top: t::S8,
            bottom: t::S8 * 2.0,
            left: t::S8 + t::S4,
            right: t::S8 + t::S4,
        }))
        .style(style::scroll)
        .width(Length::Fill)
        .height(Length::Fill);
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: t::header_h(),
                ..Padding::ZERO
            })
            .into()
    }

    fn ns_downloads(&self) -> El<'_> {
        let s = &self.ns;
        if s.queue.is_empty() {
            return empty_state(
                "download",
                if s.loading {
                    "Loading the queue"
                } else {
                    "Nothing is downloading"
                },
                "Releases you grab, and the ones the server finds by itself, show here with their progress.",
            );
        }
        card(s.queue.iter().map(|q| self.ns_queue_row(q)).collect())
    }

    fn ns_queue_row<'a>(&'a self, q: &'a QueueItem) -> El<'a> {
        let g = &q.grab;
        let (frac, facts) = match &q.status {
            Some(st) => {
                let frac = if st.size > 0 {
                    st.done as f32 / st.size as f32
                } else {
                    0.0
                };
                let mut f = vec![
                    format!("{:.0} %", frac * 100.0),
                    format!("{} of {}", bytes(st.done), bytes(st.size)),
                ];
                if st.state == TorrentState::Downloading {
                    f.push(format!("\u{2193} {}/s", bytes(st.down_rate)));
                    if let Some(e) = st.eta_secs {
                        f.push(format!("{} left", eta(e)));
                    }
                    f.push(format!("{} peers", st.peers));
                }
                if st.state == TorrentState::Seeding {
                    f.push(format!("\u{2191} {}/s", bytes(st.up_rate)));
                }
                (frac, f)
            }
            None => (
                if g.state == GrabState::Downloading {
                    0.0
                } else {
                    1.0
                },
                vec![],
            ),
        };
        let state = match (&q.status, g.state) {
            (_, GrabState::ImportBlocked) => ("Import blocked", true),
            (_, GrabState::Failed) => ("Failed", true),
            (_, GrabState::Done) => ("Done", false),
            (Some(st), _) => (
                match st.state {
                    TorrentState::Queued => "Queued",
                    TorrentState::Metadata => "Fetching metadata",
                    TorrentState::Checking => "Checking",
                    TorrentState::Downloading => "Downloading",
                    TorrentState::Seeding => "Seeding",
                    TorrentState::Paused => "Paused",
                    TorrentState::Error => "Error",
                },
                st.state == TorrentState::Error,
            ),
            (None, GrabState::Seeding) => ("Seeding", false),
            (None, _) => ("Waiting", false),
        };
        let paused = q
            .status
            .as_ref()
            .is_some_and(|st| st.state == TorrentState::Paused);
        let mut buttons = row![].spacing(t::S2);
        if g.state == GrabState::ImportBlocked {
            buttons = buttons.push(action_button(
                "Retry import",
                true,
                false,
                false,
                false,
                m(Msg::Act(g.id, QueueAct::Retry)),
            ));
        } else if q.status.is_some() {
            buttons = buttons.push(if paused {
                action_button(
                    "Resume",
                    false,
                    false,
                    false,
                    false,
                    m(Msg::Act(g.id, QueueAct::Resume)),
                )
            } else {
                action_button(
                    "Pause",
                    false,
                    false,
                    false,
                    false,
                    m(Msg::Act(g.id, QueueAct::Pause)),
                )
            });
        }
        buttons = buttons.push(if self.ns.armed == Some(g.id) {
            action_button(
                "Remove and delete data",
                false,
                true,
                false,
                false,
                m(Msg::Act(g.id, QueueAct::Remove(false))),
            )
        } else {
            action_button("Remove", false, true, false, false, m(Msg::Arm(g.id)))
        });
        let mut col = column![
            row![
                column![
                    text(q.title_name.clone()).size(t::BODY).font(t::semibold()),
                    small(g.release.title.clone()).wrapping(text::Wrapping::None),
                ]
                .spacing(3)
                .width(Length::Fill)
                .clip(true),
                chip(state.0, !state.1),
                buttons,
            ]
            .spacing(t::S4)
            .align_y(Alignment::Center),
            progress_bar(frac, 4.0),
        ]
        .spacing(t::S2);
        if !facts.is_empty() {
            col = col.push(mono(facts.join("  \u{00B7}  ")));
        }
        if let Some(msg) = &g.message {
            col = col.push(small(msg.clone()).color(t::warn()));
        }
        col.into()
    }

    fn ns_library(&self) -> El<'_> {
        let s = &self.ns;
        let kinds = [
            MediaKind::Movie,
            MediaKind::Series,
            MediaKind::Music,
            MediaKind::Book,
        ]
        .into_iter()
        .fold(row![].spacing(t::S2), |r, k| {
            r.push(action_button(
                kind_label(k),
                s.kind == Some(k),
                false,
                false,
                false,
                m(Msg::Kind(k)),
            ))
        });
        let filter = text_input("Filter titles", &s.filter)
            .on_input(|v| m(Msg::Filter(v)))
            .size(t::BODY)
            .padding([t::S2, t::S3])
            .width(260)
            .style(style::input);
        let bar = row![kinds, space().width(Length::Fill), filter].align_y(Alignment::Center);
        let q = s.filter.trim().to_lowercase();
        let shown: Vec<&Title> = s
            .titles
            .iter()
            .filter(|x| q.is_empty() || x.name.to_lowercase().contains(&q))
            .collect();
        let list: El = if shown.is_empty() {
            empty_state(
                "film",
                if s.loading {
                    "Loading titles"
                } else {
                    "No titles here yet"
                },
                "Titles you request or add in the dashboard are listed here; the server watches them and downloads what is missing.",
            )
        } else {
            card(shown.into_iter().map(|x| self.ns_title_row(x)).collect())
        };
        column![bar, list].spacing(t::S4).into()
    }

    fn ns_title_row<'a>(&'a self, x: &'a Title) -> El<'a> {
        let open = self.ns.releases.as_ref().filter(|(id, _)| *id == x.id);
        let line = row![
            column![
                text(format!("{}{}", x.name, year(x.year)))
                    .size(t::BODY)
                    .font(t::semibold()),
                small(if x.monitored {
                    "Monitored: missing parts are searched for"
                } else {
                    "Not monitored"
                }),
            ]
            .spacing(3)
            .width(Length::Fill),
            action_button(
                "Search now",
                false,
                false,
                false,
                !x.monitored,
                m(Msg::Search(x.id))
            ),
            if open.is_some() {
                action_button(
                    "Hide releases",
                    false,
                    false,
                    false,
                    false,
                    m(Msg::HideReleases),
                )
            } else {
                action_button(
                    "Releases",
                    false,
                    false,
                    false,
                    false,
                    m(Msg::Releases(x.id)),
                )
            },
            switch(x.monitored, false, m(Msg::Monitor(x.id, !x.monitored))),
        ]
        .spacing(t::S3)
        .align_y(Alignment::Center);
        let Some((id, found)) = open else {
            return line.into();
        };
        let body: El = match found {
            None => small("Asking the indexers\u{2026}").into(),
            Some(Err(e)) => small(e.clone()).color(t::warn()).into(),
            Some(Ok(v)) if v.is_empty() => small("No releases found.").into(),
            Some(Ok(v)) => v
                .iter()
                .take(50)
                .fold(column![].spacing(t::S3), |c, r| c.push(release_row(*id, r)))
                .into(),
        };
        column![
            line,
            container(body)
                .padding(Padding {
                    top: t::S3,
                    left: t::S4,
                    ..Padding::ZERO
                })
                .width(Length::Fill)
        ]
        .into()
    }

    fn ns_requests(&self) -> El<'_> {
        let s = &self.ns;
        if s.requests.is_empty() {
            return empty_state(
                "inbox",
                if s.loading {
                    "Loading requests"
                } else {
                    "No requests"
                },
                "Requests from you and the server's other users show here until they are available.",
            );
        }
        card(
            s.requests
                .iter()
                .map(|r| {
                    let (st, ok) = match r.status {
                        RequestStatus::Pending => ("Pending", false),
                        RequestStatus::Approved => ("Approved", true),
                        RequestStatus::Downloading => ("Downloading", true),
                        RequestStatus::Available => ("Available", true),
                        RequestStatus::Declined => ("Declined", false),
                    };
                    let mut b = row![].spacing(t::S2);
                    if r.status == RequestStatus::Pending {
                        b = b.push(action_button(
                            "Approve",
                            true,
                            false,
                            false,
                            false,
                            m(Msg::Approve(r.id)),
                        ));
                    }
                    let key = u64::MAX - r.id;
                    b = b.push(if self.ns.armed == Some(key) {
                        action_button(
                            "Cancel request",
                            false,
                            true,
                            false,
                            false,
                            m(Msg::CancelRequest(r.id)),
                        )
                    } else {
                        action_button("Cancel", false, true, false, false, m(Msg::Arm(key)))
                    });
                    row![
                        column![
                            text(format!("{}{}", r.name, year(r.year)))
                                .size(t::BODY)
                                .font(t::semibold()),
                            small(format!(
                                "{} \u{00B7} requested by {}",
                                kind_label(r.kind),
                                r.requested_by
                            )),
                        ]
                        .spacing(3)
                        .width(Length::Fill),
                        chip(st, ok),
                        b,
                    ]
                    .spacing(t::S4)
                    .align_y(Alignment::Center)
                    .into()
                })
                .collect(),
        )
    }

    fn ns_storage(&self) -> El<'_> {
        let s = &self.ns;
        if s.storage.is_empty() {
            return empty_state(
                "folder",
                if s.loading {
                    "Loading storage"
                } else {
                    "No storage added"
                },
                "Folders, network shares and storage agents the server writes to; add them in the dashboard.",
            );
        }
        card(s.storage.iter().map(storage_row).collect())
    }
}

fn release_row<'a>(title: u64, c: &'a Candidate) -> El<'a> {
    let r = &c.release;
    let ok = c.rejections.is_empty();
    let mut facts = vec![
        r.parsed.quality.label().to_string(),
        bytes(r.size),
        r.indexer.clone(),
    ];
    if let Some(n) = r.seeders {
        facts.push(format!("{n} seeders"));
    }
    let mut col = column![
        text(r.title.clone())
            .size(t::CONTROL_TEXT + 1.0)
            .wrapping(text::Wrapping::None),
        mono(facts.join("  \u{00B7}  ")),
    ]
    .spacing(3)
    .width(Length::Fill)
    .clip(true);
    if !ok {
        col = col.push(small(c.rejections.join("; ")).color(t::warn()));
    }
    row![
        col,
        action_button(
            "Grab",
            ok,
            false,
            false,
            false,
            m(Msg::Grab(title, Box::new(r.clone())))
        ),
    ]
    .spacing(t::S3)
    .align_y(Alignment::Center)
    .into()
}

fn storage_row(r: &StorageRoot) -> El<'_> {
    let h = &r.health;
    let (ok, word) = match (&h.error, h.reachable, h.writable) {
        (Some(_), _, _) | (_, false, _) => (false, "Unreachable"),
        (_, true, false) => (false, "Read only"),
        _ => (true, "Healthy"),
    };
    let place = if r.path.is_empty() {
        r.url.clone()
    } else {
        r.path.clone()
    };
    let mut col = column![
        row![
            column![
                text(if r.label.is_empty() {
                    place.clone()
                } else {
                    r.label.clone()
                })
                .size(t::BODY)
                .font(t::semibold()),
                small(format!("{} \u{00B7} {} \u{00B7} {}", r.kind, r.role, place)),
            ]
            .spacing(3)
            .width(Length::Fill),
            chip(word, ok),
        ]
        .spacing(t::S4)
        .align_y(Alignment::Center),
    ]
    .spacing(t::S2);
    if let Some(sp) = h.space.filter(|sp| sp.total > 0) {
        col = col
            .push(progress_bar(1.0 - sp.free as f32 / sp.total as f32, 4.0))
            .push(mono(format!(
                "{} free of {}",
                bytes(sp.free),
                bytes(sp.total)
            )));
    }
    if let Some(e) = &h.error {
        col = col.push(small(e.clone()).color(t::warn()));
    }
    col.into()
}
