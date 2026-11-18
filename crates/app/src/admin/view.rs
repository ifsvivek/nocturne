//! Admin rendering: sidebar, source picker, dashboard cells, list + detail panel, confirmation
//! dialog, toast. Port of the GPUI `admin/view.rs` (see `docs/screenshots/admin*.webp`).
use super::pick::{self, Probe};
use super::{Admin, GROUP_H, LIST_ID, Msg, ROW_H, Scope};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{
    action_button, avatar, band, chip, mono, pill_button_dim, progress_bar, rule, section, tracked,
};
use iced::widget::button::{Status, Style};
use iced::widget::{
    Column, Space, column, container, mouse_area, opaque, row, scrollable, space, stack, text,
    text_input,
};
use iced::{Alignment, Color, Element, Length, Padding, Shadow};
use nocturne_core::admin::act::Act;
use nocturne_core::admin::fmt::{short, when};
use nocturne_core::admin::model::{self, Btn, Item, Model};
use nocturne_core::admin::state::{L, POLL_SECS, Row, SIDEBAR, Section};

type El<'a> = Element<'a, Msg>;

/// Space the root's header and breadcrumb strip cover.
pub const TOP: f32 = t::HEADER_H + 29.0;
const SIDE_W: f32 = 248.0;
const DETAIL_W: f32 = 440.0;

fn tr<'a>(s: &str) -> iced::widget::Text<'a> {
    mono(tracked(s))
}

fn kv<'a>(k: &str, v: String) -> El<'a> {
    row![
        mono(k.to_string()).width(96),
        text(v)
            .font(t::MONO)
            .size(t::CAPTION)
            .color(t::TEXT_SECONDARY)
            .width(Length::Fill),
    ]
    .spacing(t::S3)
    .into()
}

fn vrule<'a>() -> El<'a> {
    container(Space::new().width(1).height(Length::Fill))
        .style(style::rule)
        .into()
}

/// Dashboard cell: mono caption strip on top, content below.
fn cell<'a>(label: &str, body: impl Into<El<'a>>) -> El<'a> {
    column![
        container(tr(label))
            .height(36)
            .padding([0.0, t::S4])
            .center_y(36),
        rule(),
        container(body).padding(t::S4).width(Length::Fill),
    ]
    .width(Length::Fill)
    .into()
}

fn big<'a>(n: impl ToString, caption: &str) -> El<'a> {
    column![
        text(n.to_string()).size(40).font(t::BOLD),
        mono(caption.to_string())
    ]
    .spacing(t::S1)
    .into()
}

fn loading<'a, T>(l: &L<T>) -> El<'a> {
    match l {
        L::Ready(_) => space().into(),
        L::Loading => mono("loading").into(),
        L::Failed => mono("unavailable").color(t::WARN).into(),
    }
}

fn block<'a>(label: &str, body: impl Into<El<'a>>) -> El<'a> {
    column![
        container(tr(label))
            .height(36)
            .padding([0.0, t::S4])
            .center_y(36),
        rule(),
        body.into(),
        rule(),
    ]
    .into()
}

fn buttons<'a>(a: &Admin, bs: &[Btn], focus: Option<usize>, scope: Scope) -> El<'a> {
    let mut r = row![].spacing(t::S2);
    for (i, b) in bs.iter().enumerate() {
        // Views are reads: they work with server changes off.
        let read = matches!(b.act, Act::ViewLog(_) | Act::ViewPrefs(_));
        r = r.push(action_button(
            &b.label,
            b.primary,
            b.danger,
            focus == Some(i),
            !a.writes && !read,
            Msg::Press(scope, i),
        ));
    }
    r.wrap().vertical_spacing(t::S2).into()
}

fn dashboard<'a>(a: &Admin, m: &Model) -> El<'a> {
    let d = &a.st.data;
    let st = &a.st;
    let focus_body = !st.in_side;
    let server: El = match d.system.get() {
        Some(s) => {
            let os = if s.operating_system_display_name.is_empty() {
                &s.operating_system
            } else {
                &s.operating_system_display_name
            };
            let mut c = column![
                text(s.server_name.clone()).size(18).font(t::SEMIBOLD),
                kv("VERSION", format!("{} {}", s.product_name, s.version)),
                kv("OS", format!("{os} {}", s.system_architecture)),
                kv("ADDRESS", s.local_address.clone().unwrap_or_default()),
                kv("WEBSOCKET", s.web_socket_port_number.to_string()),
                kv("SERVER ID", short(&s.id)),
            ]
            .spacing(t::S2);
            if s.has_pending_restart {
                c = c.push(mono("RESTART PENDING").color(t::WARN));
            }
            if s.has_update_available {
                c = c.push(mono("UPDATE AVAILABLE").color(t::WARN));
            }
            c.push(
                container(buttons(
                    a,
                    &m.toolbar,
                    (focus_body && st.on_toolbar).then_some(st.act),
                    Scope::Toolbar,
                ))
                .padding([t::S2, 0.0]),
            )
            .into()
        }
        None => loading(&d.system),
    };

    let counts: El = match d.counts.get() {
        Some(c) => {
            let all = [
                ("movies", c.movie_count),
                ("series", c.series_count),
                ("episodes", c.episode_count),
                ("albums", c.album_count),
                ("songs", c.song_count),
                ("music videos", c.music_video_count),
                ("box sets", c.box_set_count),
                ("books", c.book_count),
                ("trailers", c.trailer_count),
                ("programs", c.program_count),
            ];
            let cells: Vec<El> = all
                .into_iter()
                .enumerate()
                .filter(|(i, (_, n))| *n > 0 || *i < 3)
                .map(|(_, (label, n))| container(big(n, label)).padding(t::S4).width(180).into())
                .collect();
            row(cells).wrap().into()
        }
        None => container(loading(&d.counts)).padding(t::S4).into(),
    };

    let mut cards: Vec<El> = vec![];
    if m.items.is_empty() {
        cards.push(container(mono("no active sessions")).padding(t::S4).into());
    }
    for (i, it) in m.items.iter().enumerate() {
        let on = focus_body && !st.on_toolbar && st.sel == i;
        let mut c = column![
            row![
                text(it.title.clone())
                    .size(t::BODY)
                    .font(t::SEMIBOLD)
                    .width(Length::Fill)
                    .wrapping(text::Wrapping::None),
                row(it
                    .chips
                    .iter()
                    .map(|(c, ac)| chip(c, *ac))
                    .collect::<Vec<_>>())
                .spacing(t::S1),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
            mono(it.sub.clone()),
        ]
        .spacing(t::S2);
        for (k, v) in &it.detail {
            c = c.push(kv(k, v.clone()));
        }
        if !it.acts.is_empty() {
            c = c.push(buttons(a, &it.acts, on.then_some(st.act), Scope::Row(i)));
        }
        cards.push(
            Press::new(c)
                .width(320)
                .padding(t::S4)
                .on_press(Msg::Row(i, true))
                .style(move |_| Style {
                    background: on.then_some(t::ACCENT_TINT.into()),
                    text_color: t::TEXT,
                    border: border(if on { t::ACCENT } else { t::HAIRLINE }, 1.0, 0.0),
                    shadow: Shadow::default(),
                    snap: false,
                })
                .into(),
        );
    }
    let cards = row(cards).wrap();

    let tasks: El = match d.tasks.get() {
        Some(v) => {
            let run: Vec<_> = v.iter().filter(|t| t.state == "Running").collect();
            if run.is_empty() {
                mono("no tasks running").into()
            } else {
                column(
                    run.into_iter()
                        .map(|r| {
                            let p = r.current_progress_percentage.unwrap_or(0.0) as f32;
                            column![
                                row![
                                    text(r.name.clone())
                                        .size(t::BODY)
                                        .width(Length::Fill)
                                        .wrapping(text::Wrapping::None),
                                    mono(format!("{p:.0}%")),
                                ],
                                progress_bar(p / 100.0, 3.0),
                            ]
                            .spacing(t::S1)
                            .into()
                        })
                        .collect::<Vec<El>>(),
                )
                .spacing(t::S3)
                .into()
            }
        }
        None => loading(&d.tasks),
    };
    let libs: El = match d.folders.get() {
        Some(v) => {
            let busy = v
                .iter()
                .filter(|f| f.refresh_status.as_deref().is_some_and(|s| s != "Idle"))
                .count();
            column![
                big(v.len(), "libraries"),
                mono(format!("{busy} refreshing"))
            ]
            .spacing(t::S1)
            .into()
        }
        None => loading(&d.folders),
    };
    let act: El = match d.activity.get() {
        Some(p) => column(
            p.items
                .iter()
                .take(10)
                .map(|e| {
                    let warn = matches!(e.severity.as_str(), "Warning" | "Error" | "Critical");
                    column![
                        container(
                            row![
                                mono(when(Some(&e.date))).width(90),
                                text(e.name.clone())
                                    .size(t::CAPTION + 1.0)
                                    .color(t::TEXT_SECONDARY)
                                    .width(Length::Fill)
                                    .wrapping(text::Wrapping::None),
                                mono(e.kind.clone()).color(if warn {
                                    t::WARN
                                } else {
                                    t::TEXT_TERTIARY
                                }),
                            ]
                            .spacing(t::S4)
                            .align_y(Alignment::Center)
                        )
                        .height(32)
                        .padding([0.0, t::S4])
                        .center_y(32),
                        rule(),
                    ]
                    .into()
                })
                .collect::<Vec<El>>(),
        )
        .into(),
        None => container(loading(&d.activity)).padding(t::S4).into(),
    };
    column![
        rule(),
        row![
            cell("Server", server),
            vrule(),
            cell("Libraries", libs),
            vrule(),
            cell("Running tasks", tasks),
        ],
        rule(),
        block("Items", counts),
        block("Sessions", cards),
        block("Recent activity", act),
    ]
    .into()
}

fn list_row<'a>(a: &Admin, i: usize, it: &Item, on: bool) -> El<'a> {
    let mut r = row![].spacing(t::S3).align_y(Alignment::Center);
    if it.avatar {
        let pic = it
            .pic
            .as_ref()
            .and_then(|k| a.pics.get(k))
            .and_then(Option::as_ref);
        r = r.push(avatar(
            32.0,
            &it.title,
            pic,
            t::HAIRLINE_STRONG,
            1.0,
            t::CAPTION + 1.0,
            t::TEXT_SECONDARY,
        ));
    }
    r = r.push(
        column![
            text(it.title.clone())
                .size(t::BODY)
                .font(t::SEMIBOLD)
                .color(if on { t::TEXT } else { t::TEXT_SECONDARY })
                .wrapping(text::Wrapping::None),
            mono(it.sub.clone()).wrapping(text::Wrapping::None),
        ]
        .spacing(2)
        .width(Length::Fill),
    );
    if let Some(p) = it.progress {
        r = r.push(container(progress_bar(p, 3.0)).width(96));
    }
    for (c, accent) in &it.chips {
        r = r.push(chip(c, *accent));
    }
    column![
        Press::new(container(r).center_y(Length::Fill).clip(true))
            .width(Length::Fill)
            .height(ROW_H - 1.0)
            .padding([0.0, t::PAGE_PAD])
            .on_press(Msg::Row(i, false))
            .style(move |s| Style {
                background: if on {
                    Some(t::ACCENT_TINT.into())
                } else if matches!(s, Status::Hovered) {
                    Some(t::GLASS.into())
                } else {
                    None
                },
                text_color: t::TEXT,
                border: border(t::NONE, 0.0, 0.0),
                shadow: Shadow::default(),
                snap: false,
            }),
        rule(),
    ]
    .into()
}

fn list<'a>(a: &Admin, m: &Model) -> El<'a> {
    let st = &a.st;
    let body_focus = !st.in_side;
    let mut rows: Column<Msg> = column![];
    if let Some(s) = m.status {
        rows = rows.push(container(mono(s)).padding(t::PAGE_PAD));
    }
    for (i, it) in m.items.iter().enumerate() {
        if m.group_start(i) {
            rows = rows.push(column![
                container(tr(it.group.as_deref().unwrap_or("")))
                    .height(GROUP_H - 1.0)
                    .padding([0.0, t::PAGE_PAD])
                    .center_y(GROUP_H - 1.0),
                rule(),
            ]);
        }
        rows = rows.push(list_row(
            a,
            i,
            it,
            body_focus && !st.on_toolbar && st.sel == i,
        ));
    }
    let mut left = column![].width(Length::Fill).height(Length::Fill);
    if !m.toolbar.is_empty() {
        left = left.push(column![
            container(buttons(
                a,
                &m.toolbar,
                (body_focus && st.on_toolbar).then_some(st.act),
                Scope::Toolbar,
            ))
            .padding([t::S3, t::PAGE_PAD]),
            rule(),
        ]);
    }
    left = left.push(
        scrollable(rows)
            .id(LIST_ID)
            .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y, v.bounds().height))
            .style(style::scroll)
            .width(Length::Fill)
            .height(Length::Fill),
    );

    let mut panel = column![].spacing(t::S4);
    if let Some(it) = m.items.get(st.sel) {
        panel = panel
            .push(tr("Detail"))
            .push(text(it.title.clone()).size(t::HEADING).font(t::BOLD))
            .push(
                column(
                    it.detail
                        .iter()
                        .map(|(k, v)| kv(k, v.clone()))
                        .collect::<Vec<El>>(),
                )
                .spacing(t::S2),
            );
        let f = (body_focus && !st.on_toolbar).then_some(st.act);
        if it.acts.is_empty() && it.text.is_none() && it.detail.is_empty() {
            panel = panel.push(mono("View only in NOCTURNE."));
        }
        panel = panel.push(buttons(a, &it.acts, f, Scope::Row(st.sel)));
        if let Some(txt) = &it.text {
            let lines: Vec<El> = txt
                .lines()
                .map(|l| {
                    text(l.to_string())
                        .font(t::MONO)
                        .size(t::MICRO)
                        .color(t::TEXT_SECONDARY)
                        .wrapping(text::Wrapping::None)
                        .into()
                })
                .collect();
            panel = panel.push(
                container(scrollable(column(lines)).style(style::scroll))
                    .height(360)
                    .width(Length::Fill)
                    .padding(t::S2)
                    .style(|_| iced::widget::container::Style {
                        background: Some(t::SURFACE.into()),
                        border: border(t::HAIRLINE, 1.0, t::R_XS),
                        ..Default::default()
                    }),
            );
        }
    }
    row![
        container(left).width(Length::Fill).height(Length::Fill),
        vrule(),
        scrollable(container(panel).padding(t::S6).width(DETAIL_W))
            .style(style::scroll)
            .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}

fn sidebar<'a>(a: &Admin) -> El<'a> {
    let mut c = column![].padding([t::S3, 0.0]);
    for r in SIDEBAR.iter() {
        c = match r {
            Row::Group(g) => c.push(container(tr(g)).padding(Padding {
                top: t::S4,
                bottom: t::S2,
                left: t::S6,
                right: 0.0,
            })),
            Row::Item(s, indent) => {
                let s = *s;
                let on = a.st.section == s;
                let focus = on && a.st.in_side;
                c.push(
                    Press::new(
                        container(text(s.label()).size(t::BODY).font(t::MEDIUM)).center_y(36),
                    )
                    .width(Length::Fill)
                    .height(36)
                    .padding(Padding {
                        left: t::S6 + if *indent { t::S4 } else { 0.0 },
                        right: t::S4,
                        ..Padding::ZERO
                    })
                    .on_press(Msg::Side(s))
                    .style(move |st| Style {
                        background: on.then_some(t::ACCENT_TINT.into()),
                        text_color: if on {
                            t::ACCENT_TEXT
                        } else if matches!(st, Status::Hovered) {
                            t::TEXT
                        } else {
                            t::TEXT_SECONDARY
                        },
                        border: iced::Border {
                            color: if focus { t::ACCENT } else { t::NONE },
                            width: 0.0,
                            radius: 0.0.into(),
                        },
                        shadow: Shadow::default(),
                        snap: false,
                    }),
                )
            }
        };
    }
    row![
        scrollable(c)
            .style(style::scroll)
            .width(SIDE_W)
            .height(Length::Fill),
        vrule(),
    ]
    .into()
}

/// Reads work, writes are off: say so once, with the way out.
fn view_only_banner<'a>() -> El<'a> {
    container(
        row![
            text("View only. NOCTURNE is not allowed to change anything on your server, so the actions below are off.")
                .size(t::BODY)
                .width(Length::Fill),
            pill_button_dim("Open settings", true, false, false, Msg::OpenSettings),
        ]
        .spacing(t::S4)
        .align_y(Alignment::Center),
    )
    .padding([t::S3, t::S4])
    .style(|_| iced::widget::container::Style {
        background: Some(t::ACCENT_TINT.into()),
        border: border(t::HAIRLINE_STRONG, 1.0, t::R_PANEL),
        ..Default::default()
    })
    .into()
}

/// Which server is being administered; a chooser when more than one source is an administrator.
fn picker<'a>(a: &Admin) -> Option<El<'a>> {
    let adm = pick::admins(&a.probes);
    let waiting = !pick::settled(&a.probes);
    if adm.len() < 2 && !waiting {
        // One server: name it so it is clear what the screen acts on.
        let i = a.chosen?;
        return Some(
            container(mono(format!(
                "SERVER  {}",
                a.targets[i].label.to_uppercase()
            )))
            .padding([t::S2, t::PAGE_PAD])
            .into(),
        );
    }
    let mut r = row![mono("SERVER")]
        .spacing(t::S2)
        .align_y(Alignment::Center);
    for i in adm {
        r = r.push(action_button(
            &a.targets[i].label,
            a.chosen == Some(i),
            false,
            false,
            false,
            Msg::Choose(i),
        ));
    }
    if waiting {
        r = r.push(mono("checking servers"));
    }
    if a.probes.iter().filter(|p| **p == Probe::Admin).count() > 1 {
        r = r.push(mono("Tab switches"));
    }
    Some(container(r).padding([t::S2, t::PAGE_PAD]).into())
}

fn blurb(s: Section) -> &'static str {
    match s {
        Section::Dashboard => "Server status, sessions and activity at a glance.",
        Section::General => "Server name, language, retention and folders.",
        Section::Branding => "Login message, custom styling and splash screen.",
        Section::Users => "Accounts, access and passwords.",
        Section::Access => "Library access, ratings, feature switches and limits for each account.",
        Section::Libraries => "Add and remove libraries, their folders and options.",
        Section::Providers => {
            "Metadata and image providers per library: what the server offers, in the order it tries them."
        }
        Section::Metadata => "Where metadata comes from.",
        Section::Resume => "When playback counts as started or finished.",
        Section::Streaming => "Limits for streaming outside your network.",
        Section::Transcoding => "How the server converts video when a client cannot play it as is.",
        Section::Trickplay => "Seek-bar preview thumbnails: size, interval and quality.",
        Section::Backups => "Server backups (read only).",
        Section::Devices => "Devices that have signed in.",
        Section::Activity => "Recent events on this server.",
        Section::Plugins => "Installed plugins and the plugin catalog.",
        Section::Networking => "Ports, addresses and access. Changes can lock you out.",
        Section::ApiKeys => "Keys that let other apps use this server.",
        Section::Logs => "Server log files and a tail viewer.",
        Section::Tasks => "Scheduled maintenance, grouped by category.",
    }
}

fn dialog<'a>(a: &Admin) -> Option<El<'a>> {
    let d = a.dialog.as_ref()?;
    let valid = d.valid();
    let danger = d.danger;
    let mut items: Vec<El> = vec![
        mono(tracked(if danger { "Confirm" } else { "Please confirm" }))
            .color(if danger { t::DANGER } else { t::TEXT_TERTIARY })
            .into(),
        text(d.title.clone()).size(t::HEADING).font(t::BOLD).into(),
        text(d.body.clone())
            .size(t::BODY)
            .line_height(1.5)
            .color(t::TEXT_SECONDARY)
            .into(),
    ];
    for (i, f) in d.fields.iter().enumerate() {
        let bad = !f.value.is_empty() && !f.ok();
        items.push(
            column![
                mono(f.label.to_string()),
                text_input("", &f.value)
                    .id(super::field_id(i))
                    .on_input(move |s| Msg::DialogInput(i, s))
                    .on_submit(Msg::DialogSubmit(i))
                    .secure(f.secret)
                    .font(t::MONO)
                    .size(t::BODY)
                    .padding([t::S2, t::S3])
                    .style(move |th, st| {
                        let mut s = style::input(th, st);
                        if bad && !matches!(st, text_input::Status::Focused { .. }) {
                            s.border.color = t::DANGER;
                        }
                        s
                    }),
            ]
            .spacing(t::S1)
            .into(),
        );
    }
    let (ci, oi) = (d.cancel_ix(), d.confirm_ix());
    items.push(
        container(
            row![
                action_button(
                    "Cancel",
                    false,
                    false,
                    d.focus == ci,
                    false,
                    Msg::DialogClick(ci)
                ),
                action_button(
                    d.confirm,
                    !danger,
                    danger,
                    d.focus == oi,
                    !valid,
                    Msg::DialogClick(oi)
                ),
            ]
            .spacing(t::S2),
        )
        .align_right(Length::Fill)
        .into(),
    );
    let panel = container(column(items).spacing(t::S4))
        .width(480)
        .padding(t::S6)
        .style(move |_| iced::widget::container::Style {
            background: Some(t::SURFACE.into()),
            border: border(
                if danger {
                    t::DANGER
                } else {
                    t::HAIRLINE_STRONG
                },
                1.0,
                t::R_PANEL,
            ),
            ..Default::default()
        });
    Some(
        mouse_area(
            container(opaque(panel))
                .center(Length::Fill)
                .style(style::scrim),
        )
        .into(),
    )
}

pub fn view(a: &Admin) -> El<'_> {
    let m = model::model(&a.st);
    let s = a.st.section;
    let meta = match s {
        Section::Dashboard => format!("refreshes every {POLL_SECS}s"),
        _ => format!("{} items", m.items.len()),
    };
    let body: El = if a.client.is_none() {
        let msg = if pick::settled(&a.probes) {
            "No server you are signed in to lets this account administer it."
        } else {
            "Checking which servers you can administer."
        };
        container(mono(msg)).padding(t::PAGE_PAD).into()
    } else if s == Section::Dashboard {
        scrollable(container(dashboard(a, &m)).padding([0.0, t::PAGE_PAD]))
            .style(style::scroll)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        list(a, &m)
    };
    let mut main = column![band(
        container(section(s.label(), blurb(s), meta)).padding([t::S6, 0.0])
    )];
    if let Some(p) = picker(a) {
        main = main.push(p);
    }
    if !a.writes && a.client.is_some() {
        main = main.push(container(view_only_banner()).padding([t::S3, t::PAGE_PAD]));
    }
    main = main.push(body);
    let base = row![sidebar(a), main.width(Length::Fill).height(Length::Fill)]
        .padding(Padding {
            top: TOP,
            ..Padding::ZERO
        })
        .height(Length::Fill);
    let mut layers: Vec<El> = vec![
        container(base)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    ];
    if let Some(d) = dialog(a) {
        layers.push(d);
    }
    if let Some((n, _)) = &a.note {
        layers.push(
            container(
                container(text(n.clone()).size(t::BODY))
                    .padding([t::S3, t::S4])
                    .style(style::raised_card),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .padding(t::S6)
            .into(),
        );
    }
    let _: Option<Color> = None;
    stack(layers).into()
}
