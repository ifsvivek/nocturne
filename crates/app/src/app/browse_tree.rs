//! The library, collection and favourites pages as an element tree for the theme engine (`nocturne-style`). Classes follow the stage-1 vocabulary: the scope `.page-library`, `.card*` for the cells (the same `card_doc` as Home's strips), plus `.page-head`, `.tabs`, `.tab`, `.toolbar`, `.az-letter`, `.library-panel` and `.library-grid` for the page's own parts. The grid is the engine's virtual container: one prototype card gives the cell metrics, and the scene asks [`GridData`] for the card of each row that comes into view. Sort, Filters and the view switch are slots (their pop-overs are the hand-written ones). The list view and the name tabs (genres, studios) stay hand-written.
use super::browse::{BMsg, Browse, Metrics, grid_look, metrics};
use super::cells::{badge, caption, mark_for};
use super::header_tree::{Slot, Tree, px, text_w};
use super::home::Geo;
use super::home_tree::{CardIn, card_doc, kind_attr};
use super::message::Message;
use super::{App, View};
use crate::theme as t;
use crate::widget::cell::Pic;
use nocturne_core::libview::{self as lv, ViewKind};
use nocturne_core::source::Art;
use nocturne_style::{Doc, Id, VirtBuilder};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// The picture ids of the grid's cards are `GRID_PIC + index`, far from the ids the static part of a tree hands out.
pub const GRID_PIC: u32 = 1 << 20;

/// What the scene's item builder reads for each card of the grid. Shared (`Rc`) between the scene's builder and the app, which fills in pictures as they arrive.
pub struct GridData {
    /// Card data by item index (`None`: not loaded yet, drawn as a skeleton).
    pub cards: Vec<Option<CardIn>>,
    pub geo: Geo,
    /// Cache keys the cards still wait for (the real picture, or the real one behind a blurhash), by key.
    pub waiting: HashMap<String, Vec<usize>>,
    /// What the cards were built from; equal stamps mean the cards are still right.
    pub stamp: u64,
}

pub struct GridRef {
    pub node: Id,
    pub data: Rc<RefCell<GridData>>,
}

impl GridData {
    /// The card of `idx` takes this picture (a stand-in or the real one).
    pub fn set_pic(&mut self, idx: usize, pic: Pic) {
        if let Some(Some(c)) = self.cards.get_mut(idx) {
            c.pic = pic;
        }
    }
}

fn blank_card(i: usize) -> CardIn {
    CardIn {
        pic: Pic::Loading,
        pic_id: GRID_PIC + i as u32,
        key: None,
        title: String::new(),
        meta: String::new(),
        index: i,
        progress: None,
        badge: None,
        subtitle: false,
        mark: None,
        fav: false,
        played: false,
        writes: false,
        at: None,
        play: false,
        kind: "other",
        source: 0,
    }
}

/// The page's tabs and its parts as the stamp of what the grid's cards are built from.
fn stamp(br: &Browse, m: &Metrics, extra: (u32, u32, bool, bool)) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (br.len(), br.epoch, br.prefs.view as u8).hash(&mut h);
    (m.w.to_bits(), m.h.to_bits(), m.cols, extra).hash(&mut h);
    for i in 0..br.len() {
        if let Some((md, copies)) = br.item(i) {
            (
                md.origin.0,
                &md.id,
                md.favorite,
                md.played,
                md.progress().map(f32::to_bits),
                copies,
            )
                .hash(&mut h);
        }
    }
    h.finish()
}

impl App {
    /// Does the page in view run on the engine? A browse page does unless it shows the list view or the name tabs.
    pub(super) fn engine_supports_browse(&self) -> bool {
        self.stack
            .last()
            .filter(|p| p.view == View::Grid)
            .and_then(|p| p.br.as_deref())
            .is_some_and(|br| br.prefs.view != ViewKind::List && !br.tab.is_names())
    }

    /// The card data of every loaded item, reusing `old` when its stamp still holds.
    fn grid_data(
        &self,
        br: &Browse,
        m: &Metrics,
        geo: Geo,
        old: Option<&Rc<RefCell<GridData>>>,
    ) -> Rc<RefCell<GridData>> {
        let several = self.several_sources();
        let writes = self.hooks.can_write();
        let st = stamp(
            br,
            m,
            (
                self.scale.to_bits(),
                t::page_pad().to_bits(),
                several,
                writes,
            ),
        );
        if let Some(o) = old {
            let mut d = o.borrow_mut();
            if d.stamp == st {
                d.geo = geo;
                drop(d);
                return o.clone();
            }
        }
        let art = geo.look.art(if br.prefs.view == ViewKind::Landscape {
            Art::Landscape
        } else {
            Art::Poster
        });
        let mut waiting: HashMap<String, Vec<usize>> = HashMap::new();
        let cards = (0..br.len())
            .map(|idx| {
                let (md, copies) = br.item(idx)?;
                let pic = self.pic(md, art, m.w as u32);
                let (key, blur) = self.pic_keys(md, art, m.w as u32);
                if !matches!(pic, Pic::Ready(_) | Pic::Missing) {
                    waiting.entry(key.clone()).or_default().push(idx);
                    if let (Some(b), Pic::Loading) = (&blur, &pic) {
                        waiting.entry(b.clone()).or_default().push(idx);
                    }
                }
                Some(CardIn {
                    pic,
                    pic_id: GRID_PIC + idx as u32,
                    key: Some((key, blur)),
                    title: md.title.clone(),
                    meta: caption(md),
                    index: idx,
                    progress: md.progress(),
                    badge: badge(md),
                    subtitle: false,
                    mark: several
                        .then(|| mark_for(&self.catalog, md, copies))
                        .flatten(),
                    fav: md.favorite,
                    played: md.played,
                    writes,
                    at: None,
                    play: false,
                    kind: kind_attr(md.kind),
                    source: md.origin.0 as usize,
                })
            })
            .collect();
        Rc::new(RefCell::new(GridData {
            cards,
            geo,
            waiting,
            stamp: st,
        }))
    }

    /// The Sort, Filters and view-switch buttons of the toolbar (their pop-overs are the hand-written ones).
    pub(super) fn page_slot(&self, s: Slot) -> iced::Element<'_, Message> {
        let Some((pid, br)) = self
            .stack
            .last()
            .and_then(|p| Some((p.pid, p.br.as_deref()?)))
        else {
            return iced::widget::space().into();
        };
        match s {
            Slot::Sort => self.sort_control(br, pid),
            Slot::Filters => self.filter_control(br, pid),
            Slot::View(v) => Self::view_button(br, pid, v),
            _ => iced::widget::space().into(),
        }
    }

    /// The library page as a tree, or `None` when the hand-written page has to be used.
    /// `old`: the card data of the scene being replaced (kept while the items are unchanged).
    pub(super) fn browse_tree(&self, old: Option<&Rc<RefCell<GridData>>>) -> Option<Tree> {
        if !self.engine_supports_browse() {
            return None;
        }
        let p = self.stack.last()?;
        let br = p.br.as_deref()?;
        let pid = p.pid;
        let view = br.prefs.view;
        let m = metrics(self.win.width, self.settings.density.card(), view, false);
        let look = grid_look();
        let geo = Geo {
            art: look.art(if view == ViewKind::Landscape {
                Art::Landscape
            } else {
                Art::Poster
            }),
            dims: (m.w, m.h),
            look,
            grid: true,
            cols: m.cols,
            inset: t::page_pad(),
            hidden: false,
            title_t: t::typo(&Default::default(), 28.0, iced::font::Weight::Bold),
            details: false,
            framed: false,
            arrows: Default::default(),
        };
        let n = br.len();
        let pad = t::page_pad();
        let mut tree = Tree::screen("library");
        tree.doc.nodes[0].classes.push("page-library".into());
        tree.doc.attr(0, "data-page", &pid.to_string());
        tree.doc.inline(
            0,
            &format!(
                "height: {}; padding-top: {}",
                px(self.win.height),
                px(t::header_h() + t::S8)
            ),
        );
        let d = &mut tree.doc;
        // Title and paragraph.
        let head = d.add(0, "div.page-head");
        d.inline(head, &format!("padding: 0 {}", px(pad)));
        let titles = d.add(head, "div.page-titles");
        let ttl = d.add(titles, "div.page-title");
        d.text(ttl, &br.scope.title);
        let sub = d.add(titles, "div.page-sub");
        d.text(sub, &br.scope.subtitle);
        d.add(0, "div.page-gap");
        // Tabs.
        if br.tabs.len() > 1 {
            let row = d.add(0, "div.tabs");
            d.inline(row, &format!("padding: 0 {}", px(pad)));
            let mut acts = vec![];
            for tb in &br.tabs {
                let tab = d.add(
                    row,
                    if *tb == br.tab {
                        "a.tab.active"
                    } else {
                        "a.tab"
                    },
                );
                let l = d.add(tab, "span.tab-label");
                d.text(l, tb.label(br.scope.collection.as_deref()));
                d.add(tab, "div.tab-bar");
                acts.push((tab, Message::Browse(pid, BMsg::Tab(*tb))));
            }
            d.add(0, "div.rule");
            for (tab, m) in acts {
                tree.act(tab, m);
            }
        }
        let d = &mut tree.doc;
        // Toolbar: the controls, the count, the A-Z bar.
        let loading = Self::browse_loading(br);
        let tool = d.add(0, "div.toolbar");
        d.inline(tool, &format!("padding: {} {}", px(t::S3), px(pad)));
        let bar = d.add(tool, "div.toolbar-bar");
        let mut slots: Vec<(Id, Slot)> = vec![];
        let w_of = |s: &str| text_w(s, t::CAPTION + 1.0, 600) + 2.0 * t::S3;
        if br.smart.is_none() {
            let (sort, _) = Self::sort_label(br);
            for (w, s) in [
                (w_of(&sort), Slot::Sort),
                (w_of(&Self::filters_label(br)), Slot::Filters),
            ] {
                let n = d.add(bar, "div.slot.toolbar-button");
                d.inline(n, &format!("width: {}; height: 32px", px(w)));
                slots.push((n, s));
            }
        }
        let seg = d.add(bar, "div.toolbar-seg");
        for v in ViewKind::ALL {
            let n = d.add(seg, "div.slot.toolbar-button");
            d.inline(n, &format!("width: {}; height: 32px", px(w_of(v.label()))));
            slots.push((n, Slot::View(v)));
        }
        d.add(bar, "div.toolbar-fill");
        let c = d.add(bar, "div.library-count");
        d.text(c, &Self::browse_count(br, loading));
        let mut letters = vec![];
        if br.show_letters() {
            let az = d.add(tool, "div.az");
            for ch in lv::letters() {
                let l = d.add(
                    az,
                    if br.letter == Some(ch) {
                        "a.az-letter.on"
                    } else {
                        "a.az-letter"
                    },
                );
                let g = d.add(l, "span.az-glyph");
                d.text(g, &ch.to_string());
                letters.push((
                    l,
                    Message::Browse(
                        pid,
                        BMsg::Letter(if br.letter == Some(ch) {
                            None
                        } else {
                            Some(ch)
                        }),
                    ),
                ));
            }
        }
        for (n, s) in slots {
            tree.slot(n, s);
        }
        for (n, m) in letters {
            tree.act(n, m);
        }
        // The panel: the grid, or a note.
        let d = &mut tree.doc;
        let panel = d.add(0, "div.library-panel");
        d.inline(panel, &format!("margin: 0 {} {}", px(pad), px(t::S6)));
        if br.failed {
            let st = d.add(panel, "div.library-state");
            let note = d.add(st, "div.library-note");
            d.text(note, "This couldn't be loaded.");
            let b = d.add(st, "a.button.library-retry");
            let l = d.add(b, "span.button-label");
            d.text(l, "Try again");
            tree.act(b, Message::PageRetry);
        } else if loading {
            let strip = d.add(panel, "div.library-skeleton");
            for i in 0..m.cols.min(8) {
                card_doc(d, strip, &geo, &blank_card(i));
            }
        } else if n == 0 {
            let note = d.add(panel, "div.library-note");
            d.text(
                note,
                if br.prefs.filters.active() > 0 || br.letter.is_some() {
                    "Nothing matches. Try clearing a filter."
                } else {
                    "Nothing here yet."
                },
            );
        } else {
            let grid = d.add(panel, "div.library-grid");
            d.attr(grid, "data-scrollbar", "vertical");
            d.virtual_list(grid, n);
            let proto = card_doc(d, grid, &geo, &blank_card(0)).card;
            d.virtual_proto(proto);
            let data = self.grid_data(br, &m, geo, old);
            let for_builder = data.clone();
            let build: VirtBuilder = Rc::new(move |d: &mut Doc, parent: Id, idx: usize| {
                let g = for_builder.borrow();
                let n = match g.cards.get(idx).and_then(Option::as_ref) {
                    Some(c) => card_doc(d, parent, &g.geo, c),
                    None => card_doc(d, parent, &g.geo, &blank_card(idx)),
                };
                d.focusable(n.card);
            });
            tree.builders.insert(grid, build);
            tree.grid = Some(GridRef { node: grid, data });
        }
        // The hairline frame lies over the content and takes no room (as the hand-written panel's border).
        tree.doc.add(panel, "div.library-frame");
        Some(tree)
    }
}
