//! The search overlay as an element tree for the theme engine (`nocturne-style`): the scope `.page-search`, a `.search-layer` (the scrim, closing on a click outside) holding the `.search-panel` with the query field (a real `text_input`, hosted as a slot) and the `.search-list` of `.hit` rows under `.hit-group` headings (or the recent searches). The selection is the focused row, so a theme styles `.hit:focus`. The keyboard stays with `search_key` (arrows move the selection, Enter picks, Escape closes); the overlay only reveals the selected row.
use super::App;
use super::cells::mark_for;
use super::header_tree::{Slot, Tree, dims};
use super::message::Message;
use super::search::SMsg;
use crate::widget::cell::Pic;
use crate::widget::text::tracked;
use nocturne_core::source::Art;
use nocturne_style::Id;

impl App {
    /// The overlay as a tree, or `None` when no search is open.
    pub(super) fn search_tree(&self) -> Option<Tree> {
        let ui = self.search.as_deref()?;
        let s = &ui.s;
        let several = self.several_sources();
        let mut tree = Tree::screen("search");
        tree.doc.nodes[0].classes.push("page-search".into());
        let d = &mut tree.doc;
        let layer = d.add(0, "div.search-layer");
        let panel = d.add(layer, "div.search-panel");
        let head = d.add(panel, "div.search-head");
        let tag = d.add(head, "span.search-tag");
        d.text(tag, "SEARCH");
        let input = d.add(head, "div.slot.search-input");
        d.essential(input);
        let state = d.add(head, "span.search-tag");
        d.text(state, if s.loading { "searching" } else { "esc" });
        d.add(panel, "div.rule");
        let list = d.add(panel, "div.search-list");
        d.attr(list, "data-scrollbar", "vertical");
        let mut sel: Option<Id> = None;
        let mut hits: Vec<(Id, Message)> = vec![];
        let note = |d: &mut nocturne_style::Doc, text: &str| {
            let n = d.add(list, "div.search-note");
            d.text(n, text);
        };
        if s.query.trim().is_empty() {
            let g = d.add(list, "div.hit-group.recent-head");
            d.text(g, &tracked("Recent searches"));
            if self.settings.recent_searches.is_empty() {
                note(d, "Type to search your library. Press Esc to close.");
            }
            for (i, r) in self.settings.recent_searches.iter().enumerate() {
                let row = d.add(list, "a.hit.hit-recent");
                let g = d.add(row, "span.hit-glyph");
                d.text(g, "\u{21BB}");
                let l = d.add(row, "span.hit-title");
                d.text(l, r);
                d.focusable(row);
                if i == ui.recent_sel {
                    sel = Some(row);
                }
                hits.push((row, Message::Search(SMsg::Recent(i))));
            }
        } else if s.failed {
            note(
                d,
                "Search couldn't reach your sources. Keep typing to try again.",
            );
        } else if s.empty() {
            note(d, "No results. Try fewer or different words.");
        } else if s.results.is_empty() {
            note(d, if s.loading { "Searching" } else { "" });
        }
        let mut flat = 0usize;
        for (g, items) in &s.results {
            let gh = tree.doc.add(list, "div.hit-group");
            tree.doc.text(gh, &tracked(g.label()));
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
                let row = tree.doc.add(list, "a.hit");
                tree.doc.focusable(row);
                let pic_box = tree.doc.add(row, "div.hit-pic");
                let pic = self.pic(m, Art::Poster, 32);
                match &pic {
                    Pic::Missing => {
                        let init = crate::widget::text::initial(&m.title);
                        if !init.is_empty() {
                            let n = tree.doc.add(pic_box, "div.card-initial");
                            let g = tree.doc.add(n, "span.card-initial-glyph");
                            tree.doc.text(g, &init);
                        }
                    }
                    _ => {
                        let id = tree.pic(Some(self.pic_keys(m, Art::Poster, 32)), &pic);
                        let n = tree.doc.add(pic_box, "img.card-img");
                        let (w, h) = match &pic {
                            Pic::Ready(h) | Pic::Blur(h) => dims(h),
                            _ => (1, 1),
                        };
                        tree.doc.image(n, id, w, h);
                    }
                }
                let text = tree.doc.add(row, "div.hit-text");
                let title = tree.doc.add(text, "div.hit-title");
                tree.doc.text(title, &m.title);
                let meta = tree.doc.add(text, "div.hit-meta");
                tree.doc.text(meta, &sub.join(" \u{b7} "));
                if i == s.sel {
                    sel = Some(row);
                }
                hits.push((row, Message::Search(SMsg::Pick(i))));
            }
        }
        // The hairline frame lies over the content and takes no room (as the hand-written panel's border).
        tree.doc.add(panel, "div.search-frame");
        // A click on the scrim closes; the panel absorbs its own clicks.
        tree.act(layer, Message::Search(SMsg::Close));
        tree.act(panel, Message::Noop);
        for (row, m) in hits {
            tree.act(row, m);
        }
        tree.sel = sel;
        tree.list = Some(list);
        tree.slot(input, Slot::SearchInput);
        Some(tree)
    }
}
