//! Global search overlay: Ctrl+K or `/`, as-you-type with a debounce, results grouped by type,
//! keyboard navigation, recent searches. The state machine is `search.rs`.
use super::*;
use crate::search::{self, Search};

impl Root {
    pub(super) fn open_search(&mut self, cx: &mut Context<Self>) {
        self.search = Some(Search::default());
        self.search_sel = 0;
        cx.notify();
    }

    fn search_edited(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        let epoch = s.edited();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(search::DEBOUNCE).await;
            this.update(cx, |t, cx| {
                let (Some(s), Some(src)) = (t.search.as_mut(), t.source.clone()) else {
                    return;
                };
                let Some(q) = s.due(epoch) else { return };
                cx.notify();
                t.run(
                    cx,
                    {
                        let q = q.clone();
                        async move { src.search(&q).await }
                    },
                    move |t, r, _| {
                        if let Some(s) = t.search.as_mut() {
                            match r {
                                Ok(items) => drop(s.arrived(epoch, &q, items)),
                                Err(_) => s.failed(epoch),
                            }
                        }
                    },
                );
            })
            .ok();
        })
        .detach();
    }

    /// Open a hit (or a recent query) and remember the query.
    fn search_pick(&mut self, m: Media, cx: &mut Context<Self>) {
        if let Some(s) = self.search.take() {
            search::push_recent(&mut self.settings.recent_searches, &s.query);
            self.settings.save();
        }
        self.open_any(m, cx);
    }

    pub(super) fn search_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let m = &ev.keystroke.modifiers;
        let Some(s) = self.search.as_mut() else {
            return;
        };
        let recents = self.settings.recent_searches.clone();
        let browsing_recents = s.query.trim().is_empty();
        match key {
            "escape" => self.search = None,
            "down" | "up" => {
                let d = if key == "down" { 1 } else { -1 };
                if browsing_recents {
                    let n = recents.len().max(1) as i32;
                    self.search_sel = (self.search_sel as i32 + d).rem_euclid(n) as usize;
                } else {
                    s.step(d);
                    // List children: a heading per group, then its hits.
                    self.search_scroll.scroll_to_item(s.sel + group_index(s));
                }
            }
            "enter" => {
                if browsing_recents {
                    if let Some(r) = recents.get(self.search_sel) {
                        s.query = r.clone();
                        self.search_edited(cx);
                    }
                } else if let Some(hit) = s.flat().get(s.sel).map(|m| (*m).clone()) {
                    self.search_pick(hit, cx);
                }
            }
            "backspace" => {
                if m.control {
                    s.query.clear();
                } else {
                    s.query.pop();
                }
                self.search_edited(cx);
            }
            "v" if m.control => {
                if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    s.query.extend(t.chars().filter(|c| !c.is_control()));
                    self.search_edited(cx);
                }
            }
            _ => {
                if let Some(c) = ev
                    .keystroke
                    .key_char
                    .as_deref()
                    .filter(|c| !m.control && !m.platform && !c.chars().any(char::is_control))
                {
                    s.query.push_str(c);
                    self.search_edited(cx);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn search_overlay(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let failed = self.search.as_ref().is_some_and(|s| s.failed);
        let (query, loading, empty, groups, sel) = match &self.search {
            Some(s) => (
                s.query.clone(),
                s.loading,
                s.empty(),
                s.results.clone(),
                s.sel,
            ),
            None => Default::default(),
        };
        let mut list = div()
            .id("search-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.search_scroll)
            .flex()
            .flex_col();
        if query.trim().is_empty() {
            let rec = self.settings.recent_searches.clone();
            list = list.child(
                div()
                    .px(px(t::S4))
                    .py(px(t::S3))
                    .child(ui::label("Recent searches")),
            );
            if rec.is_empty() {
                list = list.child(note("Type to search your library. Press Esc to close."));
            }
            for (i, r) in rec.into_iter().enumerate() {
                let q = r.clone();
                list = list.child(
                    row_base(("recent", i), i == self.search_sel)
                        .on_click(cx.listener(move |t, _, _, cx| {
                            if let Some(s) = t.search.as_mut() {
                                s.query = q.clone();
                            }
                            t.search_edited(cx);
                        }))
                        .child(ui::mono("\u{21BB}"))
                        .child(div().text_size(px(t::BODY)).child(r)),
                );
            }
        } else if failed {
            list = list.child(note(
                "Search couldn't reach your server. Keep typing to try again.",
            ));
        } else if empty {
            list = list.child(note("No results. Try fewer or different words."));
        } else if groups.is_empty() {
            list = list.child(note(if loading { "Searching" } else { "" }));
        }
        let mut flat = 0usize;
        for (g, items) in groups {
            list = list.child(
                div()
                    .px(px(t::S4))
                    .pt(px(t::S3))
                    .pb(px(t::S1))
                    .child(ui::label(g.label())),
            );
            for m in items {
                let on = flat == sel;
                flat += 1;
                let pic = self.pic(&m, Art::Poster, 96, cx);
                let target = m.clone();
                let sub = [
                    m.sub.clone().unwrap_or_default(),
                    m.year.map(|y| y.to_string()).unwrap_or_default(),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
                list = list.child(
                    row_base(ui::name(format!("sr-{}-{flat}", m.id)), on)
                        .on_click(cx.listener(move |t, _, _, cx| t.search_pick(target.clone(), cx)))
                        .child(
                            ui::tile(pic, &m.title, &format!("sr-{}", m.id))
                                .w(px(32.0))
                                .h(px(46.0))
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
                                        .child(m.title.clone()),
                                )
                                .child(div().truncate().child(ui::mono(sub))),
                        ),
                );
            }
        }
        div()
            .id("search-away")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(rgba(0x07080cb0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, _, _, cx| {
                    t.search = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("search-panel")
                    .absolute()
                    .top(px(80.0))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w(px(720.0))
                            .max_h(px(520.0))
                            .flex()
                            .flex_col()
                            .rounded(px(t::R_PANEL))
                            .border_1()
                            .border_color(t::hairline_strong())
                            .bg(t::surface())
                            .occlude()
                            .child(
                                div()
                                    .h(px(56.0))
                                    .px(px(t::S4))
                                    .flex()
                                    .items_center()
                                    .gap(px(t::S3))
                                    .border_b_1()
                                    .border_color(t::hairline())
                                    .child(ui::mono("SEARCH"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(t::BODY_LG))
                                            .text_color(if query.is_empty() {
                                                t::text_tertiary()
                                            } else {
                                                t::text()
                                            })
                                            .child(if query.is_empty() {
                                                "Movies, shows, people, collections".to_string()
                                            } else {
                                                format!("{query}\u{2502}")
                                            }),
                                    )
                                    .child(ui::mono(if loading { "searching" } else { "esc" })),
                            )
                            .child(list),
                    ),
            )
    }
}

/// Group headings that sit above the selected hit (each takes one child slot in the list).
fn group_index(s: &Search) -> usize {
    let mut seen = 0;
    for (n, (_, v)) in s.results.iter().enumerate() {
        seen += v.len();
        if s.sel < seen {
            return n + 1;
        }
    }
    0
}

fn note(s: &str) -> Div {
    div()
        .px(px(t::S4))
        .py(px(t::S4))
        .text_size(px(t::BODY))
        .text_color(t::text_secondary())
        .child(s.to_string())
}

fn row_base(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(t::S4))
        .py(px(t::S2))
        .flex()
        .items_center()
        .gap(px(t::S3))
        .bg(if on { t::accent_tint() } else { rgba(0) })
        .cursor_pointer()
        .hover(|s| s.bg(t::glass()))
}
