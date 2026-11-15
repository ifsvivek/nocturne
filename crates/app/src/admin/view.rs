//! Admin rendering: tab row, dashboard cells, list + detail panel, confirmation dialog, toast.
use super::act::Act;
use super::fmt::when;
use super::model::{Btn, Item, Model};
use super::state::{L, Row, SIDEBAR, Section};
use super::{Admin, OpenSettings};
use crate::{theme as t, ui};
use gpui::prelude::*;
use gpui::*;

fn kv(k: &str, v: String) -> Div {
    div()
        .flex()
        .gap(px(t::S3))
        .child(ui::mono(k.to_string()).w(px(96.0)).flex_none())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(t::MONO)
                .text_size(px(t::CAPTION))
                .text_color(t::text_secondary())
                .child(v),
        )
}

/// Dashboard cell: mono caption strip on top, content below, shared hairlines.
fn cell(label: &str, body: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .border_r_1()
        .border_b_1()
        .border_color(t::hairline())
        .child(
            div()
                .px(px(t::S4))
                .h(px(36.0))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(t::hairline())
                .child(ui::mono(ui::tracked(label))),
        )
        .child(
            div()
                .p(px(t::S4))
                .flex()
                .flex_col()
                .gap(px(t::S2))
                .child(body),
        )
}

fn big(n: impl ToString, caption: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(t::S1))
        .child(
            div()
                .text_size(px(40.0))
                .font_weight(FontWeight::BOLD)
                .text_color(t::text())
                .child(n.to_string()),
        )
        .child(ui::mono(caption.to_string()))
}

fn loading<T>(l: &L<T>) -> Option<Div> {
    match l {
        L::Ready(_) => None,
        L::Loading => Some(ui::mono("loading")),
        L::Failed => Some(ui::mono("unavailable").text_color(t::warn())),
    }
}

impl Admin {
    fn dashboard(&self, m: &Model, cx: &mut Context<Self>) -> Div {
        let d = &self.st.data;
        let st = &self.st;
        let focus_body = !st.in_side;
        let server = match d.system.get() {
            Some(s) => {
                let os = if s.operating_system_display_name.is_empty() {
                    &s.operating_system
                } else {
                    &s.operating_system_display_name
                };
                let mut c = div()
                    .flex()
                    .flex_col()
                    .gap(px(t::S2))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(s.server_name.clone()),
                    )
                    .child(kv("VERSION", format!("{} {}", s.product_name, s.version)))
                    .child(kv("OS", format!("{os} {}", s.system_architecture)))
                    .child(kv("ADDRESS", s.local_address.clone().unwrap_or_default()))
                    .child(kv("WEBSOCKET", s.web_socket_port_number.to_string()))
                    .child(kv("SERVER ID", super::fmt::short(&s.id)));
                if s.has_pending_restart {
                    c = c.child(ui::mono("RESTART PENDING").text_color(t::warn()));
                }
                if s.has_update_available {
                    c = c.child(ui::mono("UPDATE AVAILABLE").text_color(t::warn()));
                }
                c
            }
            None => div().child(loading(&d.system).unwrap_or_else(|| ui::mono(""))),
        }
        .child(div().pt(px(t::S2)).child(self.buttons(
            &m.toolbar,
            (focus_body && st.on_toolbar).then_some(st.act),
            "tb",
            cx,
        )));

        let counts = match d.counts.get() {
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
                let mut row = div().flex().flex_wrap();
                for (i, (label, n)) in all.into_iter().enumerate() {
                    if n > 0 || i < 3 {
                        row = row.child(
                            div()
                                .w(px(180.0))
                                .p(px(t::S4))
                                .border_r_1()
                                .border_b_1()
                                .border_color(t::hairline())
                                .child(big(n, label)),
                        );
                    }
                }
                row
            }
            None => div()
                .p(px(t::S4))
                .child(loading(&d.counts).unwrap_or_else(|| ui::mono(""))),
        };

        let mut cards = div().flex().flex_wrap();
        if m.items.is_empty() {
            cards = cards.child(div().p(px(t::S4)).child(ui::mono("no active sessions")));
        }
        for (i, it) in m.items.iter().enumerate() {
            let on = focus_body && !st.on_toolbar && st.sel == i;
            let mut c = div()
                .id(ui::name(format!("adm-card-{i}")))
                .w(px(320.0))
                .p(px(t::S4))
                .border_r_1()
                .border_b_1()
                .border_color(if on { t::accent() } else { t::hairline() })
                .bg(if on { t::accent_tint() } else { rgba(0) })
                .flex()
                .flex_col()
                .gap(px(t::S2))
                .cursor_pointer()
                .on_click(cx.listener(move |a, _, _, cx| {
                    (a.st.in_side, a.st.on_toolbar, a.st.sel, a.st.act) = (false, false, i, 0);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(t::S2))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(t::BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(it.title.clone()),
                        )
                        .children(it.chips.iter().map(|(c, a)| ui::chip(c, *a))),
                )
                .child(ui::mono(it.sub.clone()))
                .children(it.detail.iter().map(|(k, v)| kv(k, v.clone())));
            if !it.acts.is_empty() {
                c = c.child(self.buttons(&it.acts, on.then_some(st.act), &format!("c{i}"), cx));
            }
            cards = cards.child(c);
        }

        let mut tasks = div().flex().flex_col().gap(px(t::S3));
        match d.tasks.get() {
            Some(v) => {
                let run: Vec<_> = v.iter().filter(|t| t.state == "Running").collect();
                if run.is_empty() {
                    tasks = tasks.child(ui::mono("no tasks running"));
                }
                for r in run {
                    let p = r.current_progress_percentage.unwrap_or(0.0) as f32;
                    tasks = tasks.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(t::S1))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(t::BODY))
                                            .child(r.name.clone()),
                                    )
                                    .child(ui::mono(format!("{p:.0}%"))),
                            )
                            .child(ui::progress_bar(p / 100.0, 3.0)),
                    );
                }
            }
            None => tasks = tasks.child(loading(&d.tasks).unwrap_or_else(|| ui::mono(""))),
        }
        let libs = match d.folders.get() {
            Some(v) => {
                let busy = v
                    .iter()
                    .filter(|f| f.refresh_status.as_deref().is_some_and(|s| s != "Idle"))
                    .count();
                big(v.len(), "libraries").child(ui::mono(format!("{busy} refreshing")))
            }
            None => div().child(loading(&d.folders).unwrap_or_else(|| ui::mono(""))),
        };
        let mut act = div().flex().flex_col();
        match d.activity.get() {
            Some(a) => {
                for e in a.items.iter().take(10) {
                    let warn = matches!(e.severity.as_str(), "Warning" | "Error" | "Critical");
                    act = act.child(
                        div()
                            .px(px(t::S4))
                            .h(px(32.0))
                            .flex()
                            .items_center()
                            .gap(px(t::S4))
                            .border_b_1()
                            .border_color(t::hairline())
                            .child(ui::mono(when(Some(&e.date))).w(px(90.0)).flex_none())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(t::CAPTION + 1.0))
                                    .text_color(t::text_secondary())
                                    .child(e.name.clone()),
                            )
                            .child(
                                ui::mono(e.kind.clone()).when(warn, |m| m.text_color(t::warn())),
                            ),
                    );
                }
            }
            None => {
                act = act.child(
                    div()
                        .p(px(t::S4))
                        .child(loading(&d.activity).unwrap_or_else(|| ui::mono(""))),
                )
            }
        }
        let block = |label: &str, body: Div| {
            div()
                .border_b_1()
                .border_color(t::hairline())
                .child(
                    div()
                        .px(px(t::S4))
                        .h(px(36.0))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(t::hairline())
                        .child(ui::mono(ui::tracked(label))),
                )
                .child(body)
        };
        div()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(t::hairline())
            .child(
                div()
                    .flex()
                    .child(cell("Server", server))
                    .child(cell("Libraries", libs))
                    .child(cell("Running tasks", tasks)),
            )
            .child(block("Items", counts))
            .child(block("Sessions", cards))
            .child(block("Recent activity", act))
    }

    fn row(&self, i: usize, it: &Item, on: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut r = div()
            .id(ui::name(format!("adm-row-{i}")))
            .px(px(t::PAGE_PAD))
            .py(px(t::S3))
            .flex()
            .items_center()
            .gap(px(t::S3))
            .border_b_1()
            .border_color(t::hairline())
            .cursor_pointer()
            .bg(if on { t::accent_tint() } else { rgba(0) })
            .hover(|s| s.bg(t::glass()))
            .on_click(cx.listener(move |a, _, _, cx| {
                (a.st.in_side, a.st.on_toolbar, a.st.sel, a.st.act) = (false, false, i, 0);
                cx.notify();
            }));
        if it.avatar {
            r = r.child(
                div()
                    .size(px(32.0))
                    .flex_none()
                    .rounded_full()
                    .border_1()
                    .border_color(t::hairline_strong())
                    .bg(t::raised())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(t::CAPTION + 1.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t::text_secondary())
                    .child(ui::initial(&it.title)),
            );
        }
        r = r.child(
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
                        .child(it.title.clone()),
                )
                .child(div().truncate().child(ui::mono(it.sub.clone()))),
        );
        if let Some(p) = it.progress {
            r = r.child(
                div()
                    .w(px(96.0))
                    .flex_none()
                    .child(ui::progress_bar(p, 3.0)),
            );
        }
        for (c, accent) in &it.chips {
            r = r.child(ui::chip(c, *accent));
        }
        r
    }

    fn buttons(&self, bs: &[Btn], focus: Option<usize>, key: &str, cx: &mut Context<Self>) -> Div {
        let mut d = div().flex().flex_wrap().gap(px(t::S2));
        let a_writes = self.writes;
        for (i, b) in bs.iter().enumerate() {
            let act = b.act.clone();
            d = d.child(ui::action_button(
                format!("adm-{key}-{i}"),
                &b.label,
                b.primary,
                b.danger,
                focus == Some(i),
                !a_writes && !matches!(b.act, Act::ViewLog(_)),
                cx.listener(move |a, _, _, cx| {
                    a.st.act = i;
                    a.do_act(act.clone(), cx);
                    cx.notify();
                }),
            ));
        }
        d
    }

    fn list(&self, m: &Model, cx: &mut Context<Self>) -> Div {
        let st = &self.st;
        let body_focus = !st.in_side;
        let mut rows = div()
            .id("adm-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll);
        if let Some(s) = m.status {
            rows = rows.child(div().p(px(t::PAGE_PAD)).child(ui::mono(s)));
        }
        for (i, it) in m.items.iter().enumerate() {
            if m.group_start(i) {
                rows = rows.child(
                    div()
                        .px(px(t::PAGE_PAD))
                        .h(px(32.0))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(t::hairline())
                        .child(ui::mono(ui::tracked(it.group.as_deref().unwrap_or("")))),
                );
            }
            rows = rows.child(self.row(i, it, body_focus && !st.on_toolbar && st.sel == i, cx));
        }
        let mut left = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(t::hairline());
        if !m.toolbar.is_empty() {
            left = left.child(
                div()
                    .px(px(t::PAGE_PAD))
                    .py(px(t::S3))
                    .border_b_1()
                    .border_color(t::hairline())
                    .child(self.buttons(
                        &m.toolbar,
                        (body_focus && st.on_toolbar).then_some(st.act),
                        "tb",
                        cx,
                    )),
            );
        }
        let left = left.child(rows);

        let mut panel = div()
            .id("adm-detail")
            .w(px(440.0))
            .flex_none()
            .overflow_y_scroll()
            .p(px(t::S6))
            .flex()
            .flex_col()
            .gap(px(t::S4));
        if let Some(it) = m.items.get(st.sel) {
            panel = panel
                .child(ui::mono(ui::tracked("Detail")))
                .child(
                    div()
                        .text_size(px(t::HEADING))
                        .font_weight(FontWeight::BOLD)
                        .child(it.title.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(t::S2))
                        .children(it.detail.iter().map(|(k, v)| kv(k, v.clone()))),
                );
            let f = (body_focus && !st.on_toolbar).then_some(st.act);
            if it.acts.is_empty() && it.text.is_none() && it.detail.is_empty() {
                panel = panel.child(ui::mono("View only in NOCTURNE.".to_string()));
            }
            panel = panel.child(self.buttons(&it.acts, f, "act", cx));
            if let Some(txt) = &it.text {
                panel = panel.child(
                    div()
                        .id("adm-log")
                        .h(px(360.0))
                        .flex_none()
                        .overflow_y_scroll()
                        .track_scroll(&self.log_scroll)
                        .border_1()
                        .border_color(t::hairline())
                        .rounded(px(t::R_XS))
                        .bg(t::surface())
                        .p(px(t::S2))
                        .children(txt.lines().map(|l| {
                            div()
                                .truncate()
                                .font_family(t::MONO)
                                .text_size(px(t::MICRO))
                                .text_color(t::text_secondary())
                                .child(l.to_string())
                        })),
                );
            }
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .border_b_1()
            .border_color(t::hairline())
            .child(left)
            .child(panel)
    }

    fn dialog_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let d = self.dialog.as_ref()?;
        let valid = d.valid();
        let mut panel = div()
            .id("adm-dialog")
            .w(px(480.0))
            .p(px(t::S6))
            .flex()
            .flex_col()
            .gap(px(t::S4))
            .rounded(px(t::R_PANEL))
            .border_1()
            .border_color(if d.danger {
                t::danger()
            } else {
                t::hairline_strong()
            })
            .bg(t::surface())
            .occlude()
            .child(
                ui::mono(ui::tracked(if d.danger {
                    "Confirm"
                } else {
                    "Please confirm"
                }))
                .when(d.danger, |m| m.text_color(t::danger())),
            )
            .child(
                div()
                    .text_size(px(t::HEADING))
                    .font_weight(FontWeight::BOLD)
                    .child(d.title.clone()),
            )
            .child(
                div()
                    .text_size(px(t::BODY))
                    .line_height(px(t::BODY * 1.5))
                    .text_color(t::text_secondary())
                    .child(d.body.clone()),
            );
        for (i, f) in d.fields.iter().enumerate() {
            let on = d.focus == i;
            let shown = if f.secret {
                "\u{2022}".repeat(f.value.chars().count())
            } else {
                f.value.clone()
            };
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(t::S1))
                    .child(ui::mono(f.label.to_string()))
                    .child(
                        div()
                            .id(ui::name(format!("adm-field-{i}")))
                            .h(px(36.0))
                            .px(px(t::S3))
                            .flex()
                            .items_center()
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(if on {
                                t::accent()
                            } else if !f.value.is_empty() && !f.ok() {
                                t::danger()
                            } else {
                                t::hairline_strong()
                            })
                            .font_family(t::MONO)
                            .text_size(px(t::BODY))
                            .child(format!("{shown}{}", if on { "\u{2595}" } else { "" }))
                            .on_click(cx.listener(move |a, _, _, cx| a.dialog_click(i, cx))),
                    ),
            );
        }
        let (ci, oi) = (d.cancel_ix(), d.confirm_ix());
        let (confirm, danger) = (d.confirm, d.danger);
        panel = panel.child(
            div()
                .flex()
                .justify_end()
                .gap(px(t::S2))
                .child(ui::action_button(
                    "adm-cancel".into(),
                    "Cancel",
                    false,
                    false,
                    d.focus == ci,
                    false,
                    cx.listener(move |a, _, _, cx| a.dialog_click(ci, cx)),
                ))
                .child(ui::action_button(
                    "adm-confirm".into(),
                    confirm,
                    !danger,
                    danger,
                    d.focus == oi,
                    !valid,
                    cx.listener(move |a, _, _, cx| a.dialog_click(oi, cx)),
                )),
        );
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(rgba(0x000000b8))
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .child(panel),
        )
    }
}

impl Admin {
    fn sidebar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut d = div()
            .id("adm-side")
            .w(px(248.0))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(t::hairline())
            .py(px(t::S3))
            .flex()
            .flex_col();
        for r in SIDEBAR.iter() {
            d = match r {
                Row::Group(g) => d.child(
                    div()
                        .px(px(t::S6))
                        .pt(px(t::S4))
                        .pb(px(t::S2))
                        .child(ui::mono(ui::tracked(g))),
                ),
                Row::Item(s, indent) => {
                    let s = *s;
                    let on = self.st.section == s;
                    let focus = on && self.st.in_side;
                    d.child(
                        div()
                            .id(ui::name(format!("adm-nav-{}", s.label())))
                            .h(px(36.0))
                            .pl(px(t::S6 + if *indent { t::S4 } else { 0.0 }))
                            .pr(px(t::S4))
                            .flex()
                            .items_center()
                            .border_l_2()
                            .border_color(if focus { t::accent() } else { rgba(0) })
                            .text_size(px(t::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if on {
                                t::accent_text()
                            } else {
                                t::text_secondary()
                            })
                            .bg(if on { t::accent_tint() } else { rgba(0) })
                            .cursor_pointer()
                            .hover(|h| h.text_color(t::text()))
                            .on_click(cx.listener(move |a, _, _, cx| {
                                a.st.in_side = true;
                                if a.st.section != s {
                                    a.go(s, cx);
                                }
                                cx.notify();
                            }))
                            .child(s.label()),
                    )
                }
            };
        }
        d
    }
}

impl Admin {
    /// Reads work, writes are off: say so once, with the way out.
    fn view_only_banner(&self, cx: &mut Context<Self>) -> Div {
        div()
            .mx(px(t::PAGE_PAD))
            .mb(px(t::S3))
            .px(px(t::S4))
            .py(px(t::S3))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(t::S4))
            .rounded(px(t::R_PANEL))
            .border_1()
            .border_color(t::hairline_strong())
            .bg(t::accent_tint())
            .child(
                div()
                    .text_size(px(t::BODY))
                    .text_color(t::text())
                    .child("View only. NOCTURNE is not allowed to change anything on your server, so the actions below are off."),
            )
            .child(ui::action_button(
                "adm-writes".into(),
                "Open settings",
                true,
                false,
                false,
                false,
                cx.listener(|_, _, _, cx| cx.emit(OpenSettings)),
            ))
    }
}

impl Render for Admin {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model();
        self.st.clamp(m.items.len());
        let section = self.st.section;
        let meta = match section {
            Section::Dashboard => format!("refreshes every {}s", super::state::POLL_SECS),
            _ => format!("{} items", m.items.len()),
        };
        let blurb = match section {
            Section::Dashboard => "Server status, sessions and activity at a glance.",
            Section::General => "Server name, language, retention and folders.",
            Section::Branding => "Login message, custom styling and splash screen.",
            Section::Users => "Accounts, access and passwords.",
            Section::Access => {
                "Library access, ratings, feature switches and limits for each account."
            }
            Section::Libraries => "Add and remove libraries, their folders and options.",
            Section::Providers => {
                "Metadata and image providers per library: what the server offers, in the order it tries them."
            }
            Section::Metadata => "Where metadata comes from.",
            Section::Resume => "When playback counts as started or finished.",
            Section::Streaming => "Limits for streaming outside your network.",
            Section::Transcoding => {
                "How the server converts video when a client cannot play it as is."
            }
            Section::Trickplay => "Seek-bar preview thumbnails: size, interval and quality.",
            Section::Backups => "Server backups (read only).",
            Section::Devices => "Devices that have signed in.",
            Section::Activity => "Recent events on this server.",
            Section::Plugins => "Installed plugins and the plugin catalog.",
            Section::Networking => "Ports, addresses and access. Changes can lock you out.",
            Section::ApiKeys => "Keys that let other apps use this server.",
            Section::Logs => "Server log files and a tail viewer.",
            Section::Tasks => "Scheduled maintenance, grouped by category.",
        };
        let body = if section == Section::Dashboard {
            div()
                .id("adm-dash")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(
                    div()
                        .mx(px(t::PAGE_PAD))
                        .mb(px(t::S8))
                        .child(self.dashboard(&m, cx)),
                )
                .into_any_element()
        } else {
            self.list(&m, cx).into_any_element()
        };
        let main = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(ui::band(div().py(px(t::S6)).child(ui::section(
                section.label(),
                blurb,
                meta,
            ))))
            .children((!self.writes).then(|| self.view_only_banner(cx)))
            .child(body);
        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .pt(px(t::HEADER_H))
            .child(self.sidebar(cx))
            .child(main);
        if let Some(dlg) = self.dialog_view(cx) {
            root = root.child(dlg);
        }
        if let Some(n) = &self.note {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(t::S6))
                    .right(px(t::S6))
                    .px(px(t::S4))
                    .py(px(t::S3))
                    .rounded(px(t::R_PANEL))
                    .bg(t::raised())
                    .border_1()
                    .border_color(t::hairline_strong())
                    .text_size(px(t::BODY))
                    .child(n.clone()),
            );
        }
        root
    }
}
