//! The Calendar page as an element tree for the theme engine (`nocturne-style`): the scope `.page-calendar` with a `.page-head` (title, the view switch and the previous / today / next buttons), a `.calendar-bar` (the period, the legend, the key hints) and a scrolling `.calendar-body` holding the month grid, the week columns or the agenda. Stable parts for a stylesheet: `.calendar` (with `.calendar-month`, `.calendar-weekview` or `.calendar-agenda`), `.calendar-day` (`.today`, `.outside`), `.calendar-daynum`, `.calendar-entry` (`.status-on-server`, `.status-soon`, `.status-elsewhere`; `:focus` is the selection), `.calendar-pic`, `.calendar-show`, `.calendar-meta`, `.calendar-more`, `.calendar-note`. A click is a `Message::Calendar`; the keyboard is `cal_key`.
use super::App;
use super::calendar::{Item, Msg, month_geo, month_split};
use super::header_tree::{Tree, dims, px};
use super::message::Message;
use crate::theme as t;
use crate::widget::cell::Pic;
use nocturne_core::calendar::{self as cal, Entry, Status, View};
use nocturne_core::source::Art;
use nocturne_style::Id;
use std::collections::HashMap;

/// Where an entry is drawn; the same entry looks different in each.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Month,
    Week,
    Agenda,
}

/// A cell or column narrower than this drops its poster and the episode title.
const NARROW: f32 = 150.0;

impl App {
    /// The page as a tree, or `None` when the Calendar is not in view.
    pub(super) fn calendar_tree(&self) -> Option<Tree> {
        // In a server's own layout the page is the hand-written list (that layout has no stylesheet for it).
        if !self.at_calendar() || self.jf_base().is_some() {
            return None;
        }
        let c = &self.cal;
        let pad = t::page_pad();
        let (start, end) = c.range();
        let weeks = (end.0 - start.0) / 7;
        let (cell_h, cap) = month_geo(self.win.height, weeks);
        let items = c.items(cap);
        let index: HashMap<Item, usize> =
            items.iter().enumerate().map(|(i, it)| (*it, i)).collect();
        let sel = c.sel.min(items.len().saturating_sub(1));
        let mut tree = Tree::screen("calendar");
        tree.doc.nodes[0].classes.push("page-calendar".into());
        tree.doc.inline(
            0,
            &format!(
                "height: {}; padding-top: {}",
                px(self.win.height),
                px(t::header_h() + t::S4)
            ),
        );
        let in_range = cal::within(&c.entries, start, end);
        let d = &mut tree.doc;
        // Title, sub line and the controls.
        let head = d.add(0, "div.page-head.calendar-head");
        d.inline(head, &format!("padding: 0 {}", px(pad)));
        let titles = d.add(head, "div.page-titles");
        let ttl = d.add(titles, "div.page-title");
        d.text(ttl, "Calendar");
        let sub = d.add(titles, "div.page-sub");
        let mut line = String::from(if c.trakt_on == Some(true) {
            "Upcoming episodes from your servers and your Trakt calendar."
        } else {
            "Upcoming episodes from your servers."
        });
        if c.failed {
            line.push_str(" A server did not answer.");
        }
        if c.trakt_failed {
            line.push_str(" Trakt could not be read.");
        }
        d.text(sub, &line);
        let ctl = d.add(head, "div.calendar-controls");
        let seg = d.add(ctl, "div.calendar-seg");
        let mut acts: Vec<(Id, Message)> = vec![];
        for v in View::ALL {
            let b = d.add(
                seg,
                if v == c.view {
                    "a.calendar-button.on"
                } else {
                    "a.calendar-button"
                },
            );
            d.attr(b, "data-view", v.key());
            let l = d.add(b, "span.calendar-button-label");
            d.text(l, v.label());
            acts.push((b, Message::Calendar(Msg::View(v))));
        }
        let nav = d.add(ctl, "div.calendar-seg");
        for (label, m, act) in [
            ("\u{2039}", Msg::Step(-1), "prev"),
            ("Today", Msg::Today, "today"),
            ("\u{203A}", Msg::Step(1), "next"),
        ] {
            let b = d.add(nav, "a.calendar-button");
            d.attr(b, "data-action", act);
            let l = d.add(b, "span.calendar-button-label");
            d.text(l, label);
            acts.push((b, Message::Calendar(m)));
        }
        d.add(0, "div.page-gap");
        // The period, the legend, the keys.
        let bar = d.add(0, "div.calendar-bar");
        d.inline(bar, &format!("padding: 0 {}", px(pad)));
        let period = d.add(bar, "div.calendar-period");
        d.text(period, &cal::title(c.view, c.anchor));
        let legend = d.add(bar, "div.calendar-legend");
        for s in [Status::OnServer, Status::Soon, Status::NotOnServers] {
            if s == Status::NotOnServers && c.trakt_on != Some(true) {
                continue;
            }
            let k = d.add(legend, &format!("span.calendar-key.status-{}", s.key()));
            d.add(k, "i.calendar-swatch");
            let l = d.add(k, "span.calendar-key-label");
            d.text(l, s.label());
        }
        let hint = d.add(bar, "div.calendar-hint");
        d.text(
            hint,
            "Arrows move \u{b7} Enter opens \u{b7} M W L views \u{b7} PgUp PgDn period \u{b7} T today",
        );
        d.add(0, "div.rule");
        let body = d.add(0, "div.calendar-body");
        d.attr(body, "data-scrollbar", "vertical");
        d.inline(
            body,
            &format!("padding: {} {} {}", px(t::S4), px(pad), px(t::S8)),
        );
        if in_range.is_empty() {
            let n = d.add(body, "div.calendar-note");
            d.text(
                n,
                if c.pending > 0 {
                    "Loading upcoming episodes"
                } else {
                    "Nothing airing in this range"
                },
            );
        }
        let inner = (self.eng_width() - 2.0 * pad).max(300.0);
        let mut sel_node: Option<Id> = None;
        let mut hits: Vec<(Id, Message)> = vec![];
        let mut pick = |node: Id, it: Item, tree: &mut Tree| {
            if let Some(&i) = index.get(&it) {
                if i == sel {
                    sel_node = Some(node);
                }
                hits.push((node, Message::Calendar(Msg::Pick(i))));
            }
            tree.doc.focusable(node);
        };
        match c.view {
            View::Month => {
                let cell_w = inner / 7.0;
                let grid = tree.doc.add(body, "div.calendar.calendar-month");
                let wd = tree.doc.add(grid, "div.calendar-weekdays");
                for i in 0..7 {
                    let w = tree.doc.add(wd, "span.calendar-weekday");
                    tree.doc.text(w, &cal::weekday_name(i)[..3]);
                }
                let anchor_month = c.anchor.ymd().1;
                for wk in 0..weeks {
                    let row = tree.doc.add(grid, "div.calendar-week");
                    tree.doc.inline(row, &format!("height: {}", px(cell_h)));
                    for i in 0..7 {
                        let day = start.plus(wk * 7 + i);
                        let (_, m, dn) = day.ymd();
                        let mut cls = String::from("div.calendar-day");
                        if day == c.today {
                            cls.push_str(".today");
                        }
                        if m != anchor_month {
                            cls.push_str(".outside");
                        }
                        let cellid = tree.doc.add(row, &cls);
                        let num = tree.doc.add(cellid, "div.calendar-daynum");
                        let label = if dn == 1 {
                            format!("{} {dn}", &cal::month_name(m)[..3])
                        } else {
                            dn.to_string()
                        };
                        tree.doc.text(num, &label);
                        let list: Vec<usize> = (0..c.entries.len())
                            .filter(|&k| c.entries[k].day == day)
                            .collect();
                        let (shown, hidden) = month_split(list.len(), cap);
                        for (n, &k) in list.iter().enumerate() {
                            if n >= shown {
                                let more = tree.doc.add(cellid, "a.calendar-more");
                                tree.doc.text(more, &format!("+{hidden} more"));
                                pick(more, Item::More(day), &mut tree);
                                break;
                            }
                            let node = self.cal_entry(
                                &mut tree,
                                cellid,
                                &c.entries[k],
                                Mode::Month,
                                cell_w,
                            );
                            pick(node, Item::Entry(k), &mut tree);
                        }
                    }
                }
            }
            View::Week => {
                let col_w = inner / 7.0;
                let grid = tree.doc.add(body, "div.calendar.calendar-weekview");
                for i in 0..7 {
                    let day = start.plus(i);
                    let mut cls = String::from("div.calendar-day");
                    if day == c.today {
                        cls.push_str(".today");
                    }
                    let col = tree.doc.add(grid, &cls);
                    // The columns run to the foot of the window.
                    tree.doc.inline(
                        col,
                        &format!("min-height: {}", px((self.win.height - 300.0).max(320.0))),
                    );
                    let (_, _, dn) = day.ymd();
                    let num = tree.doc.add(col, "div.calendar-daynum");
                    tree.doc.text(
                        num,
                        &format!("{} {dn}", &cal::weekday_name(day.weekday())[..3]),
                    );
                    for k in (0..c.entries.len()).filter(|&k| c.entries[k].day == day) {
                        let node = self.cal_entry(&mut tree, col, &c.entries[k], Mode::Week, col_w);
                        pick(node, Item::Entry(k), &mut tree);
                    }
                }
            }
            View::Agenda => {
                let list = tree.doc.add(body, "div.calendar.calendar-agenda");
                for (day, group) in cal::by_day(&c.entries, start, end) {
                    let g = tree.doc.add(list, "div.calendar-agenda-day");
                    let h = tree.doc.add(
                        g,
                        if day == c.today {
                            "div.calendar-agenda-head.today"
                        } else {
                            "div.calendar-agenda-head"
                        },
                    );
                    tree.doc.text(h, &cal::day_heading(day, c.today));
                    for e in group {
                        let k = c
                            .entries
                            .iter()
                            .position(|x| std::ptr::eq(x, e))
                            .unwrap_or(0);
                        let node = self.cal_entry(&mut tree, g, e, Mode::Agenda, inner);
                        pick(node, Item::Entry(k), &mut tree);
                    }
                }
            }
        }
        for (n, m) in acts {
            tree.act(n, m);
        }
        for (n, m) in hits {
            tree.act(n, m);
        }
        tree.sel = sel_node;
        tree.list = Some(body);
        Some(tree)
    }

    /// One episode: a poster, the show, the episode and the time, marked by status.
    fn cal_entry(&self, tree: &mut Tree, parent: Id, e: &Entry, mode: Mode, w: f32) -> Id {
        let narrow = w < NARROW && mode != Mode::Agenda;
        let kind = match mode {
            Mode::Month => "month",
            Mode::Week => "week",
            Mode::Agenda => "agenda",
        };
        let a = tree.doc.add(
            parent,
            &format!(
                "a.calendar-entry.calendar-entry-{kind}.status-{}{}",
                e.status.key(),
                if narrow { ".narrow" } else { "" }
            ),
        );
        tree.doc.attr(a, "data-status", e.status.key());
        tree.doc.add(a, "i.calendar-mark");
        if mode == Mode::Agenda {
            let tm = tree.doc.add(a, "div.calendar-time");
            tree.doc.text(tm, &e.time_text().unwrap_or_default());
        }
        if !narrow {
            let pic_box = tree.doc.add(a, "div.calendar-pic");
            let shown = e.series.as_ref();
            let pic = shown.map_or(Pic::Missing, |m| self.pic(m, Art::Poster, 200));
            match (&pic, shown) {
                (Pic::Missing, _) | (_, None) => {
                    let init = crate::widget::text::initial(&e.show);
                    if !init.is_empty() {
                        let n = tree.doc.add(pic_box, "div.card-initial");
                        let g = tree.doc.add(n, "span.card-initial-glyph");
                        tree.doc.text(g, &init);
                    }
                }
                (_, Some(m)) => {
                    let id = tree.pic(Some(self.pic_keys(m, Art::Poster, 200)), &pic);
                    let n = tree.doc.add(pic_box, "img.card-img");
                    let (pw, ph) = match &pic {
                        Pic::Ready(h) | Pic::Blur(h) => dims(h),
                        _ => (1, 1),
                    };
                    tree.doc.image(n, id, pw, ph);
                }
            }
        }
        let text = tree.doc.add(a, "div.calendar-text");
        let show = tree.doc.add(text, "div.calendar-show");
        tree.doc.text(show, &e.show);
        let meta = tree.doc.add(text, "div.calendar-meta");
        let ep = if narrow || e.title.is_empty() {
            e.code()
        } else {
            format!("{} {}", e.code(), e.title)
        };
        let line = match (mode, e.time_text()) {
            (Mode::Month, Some(t)) => format!("{t} \u{b7} {ep}"),
            _ => ep,
        };
        tree.doc.text(meta, &line);
        if mode != Mode::Month {
            let when = match (mode, e.time_text()) {
                (Mode::Week, Some(t)) => format!("{t} \u{b7} {}", e.status.label()),
                _ => e.status.label().to_string(),
            };
            let st = tree.doc.add(text, "div.calendar-state");
            tree.doc.text(st, &when);
        }
        a
    }
}
