//! A library page as jellyfin-web 12 builds it: `main > .skinBody > .page.libraryPage.pageWithAbsoluteTabs.withTabs > .padded-bottom-page` with the A-Z picker (`.alphaPicker-fixed-right`, a vertical MUI toggle group) and `.itemsContainer.padded-left.padded-right.vertical-wrap` holding `.card.portraitCard`s. The cards are the engine's virtual container (the page scrolls inside it, as the window scrolls in a browser); the header with the library toolbar is its own scene (`jf_header_nodes`).
use super::super::browse::{BMsg, Browse, metrics};
use super::super::browse_tree::{GRID_PIC, GridData, GridRef, blank_card, grid_geo};
use super::super::header_tree::Tree;
use super::super::home_tree::CardIn;
use super::super::message::Message;
use super::super::{App, View};
use super::card::{Look, Shape, card_doc};
use crate::widget::cell::Pic;
use nocturne_core::libview::{self as lv, ViewKind};
use nocturne_core::source::Kind;
use nocturne_style::{Doc, Id, VirtBuilder};
use std::cell::RefCell;
use std::rc::Rc;

fn look_of(view: ViewKind, kind: &str) -> Look {
    Look {
        shape: match view {
            ViewKind::Landscape => Shape::Backdrop,
            _ if kind == "album" => Shape::Square,
            _ => Shape::Portrait,
        },
        overflow: false,
        second: true,
        folder: matches!(kind, "series" | "season" | "folder"),
        person: false,
    }
}

fn builder(data: Rc<RefCell<GridData>>, view: ViewKind) -> VirtBuilder {
    Rc::new(move |d: &mut Doc, parent: Id, idx: usize| {
        let g = data.borrow();
        let blank;
        let c: &CardIn = match g.cards.get(idx).and_then(Option::as_ref) {
            Some(c) => c,
            None => {
                blank = blank_card(idx);
                &blank
            }
        };
        let n = card_doc(d, parent, c, look_of(view, c.kind), &c.meta);
        d.attr(n.play, "data-act", "play");
        d.attr(n.played, "data-act", "played");
        d.attr(n.fav, "data-act", "fav");
        d.focusable(n.card);
    })
}

impl App {
    /// The height of the jellyfin-web header on the page in view: the toolbar row, plus the library row on a library page.
    pub(in crate::app) fn jf_header_h(&self) -> f32 {
        if self.stack.last().is_some_and(|p| p.view == View::Grid) {
            96.0
        } else {
            48.0
        }
    }

    /// The header on its own (every page but Home): a tree of the one `header`.
    pub(in crate::app) fn jf_header_tree(&self) -> Tree {
        let mut tree = Tree::screen("header");
        let body = super::jf_root(&mut tree, "libraryDocument.withSectionTabs");
        let lib = self
            .stack
            .last()
            .filter(|p| p.view == View::Grid)
            .and_then(|p| Some((p.br.as_deref()?, p.pid)));
        self.jf_header_nodes(&mut tree, body, lib);
        tree
    }

    /// The library page, or `None` when the hand-written page has to be used.
    pub(in crate::app) fn jf_browse_tree(
        &self,
        old: Option<&Rc<RefCell<GridData>>>,
    ) -> Option<Tree> {
        if !self.engine_supports_browse() {
            return None;
        }
        let p = self.stack.last()?;
        let br = p.br.as_deref()?;
        let pid = p.pid;
        let view = br.prefs.view;
        let m = metrics(self.win.width, self.settings.density.card(), view, false);
        let geo = grid_geo(&m, view);
        let n = br.len();
        let mut tree = Tree::screen("library");
        tree.doc.attr(0, "data-page", &pid.to_string());
        let body = super::jf_root(&mut tree, "libraryDocument.withSectionTabs");
        let d = &mut tree.doc;
        let spacer = d.add(body, "div.jf-spacer");
        d.inline(spacer, "height: 96px");
        let main = d.add(body, "main.MuiBox-root");
        let skin = d.add(main, "div.skinBody");
        let page = d.add(
            skin,
            "div.page.mainAnimatedPage.libraryPage.pageWithAbsoluteTabs.withTabs",
        );
        d.attr(page, "data-role", "page");
        let wrap = d.add(page, "div.padded-bottom-page.MuiBox-root");
        let mut letters = vec![];
        if br.show_letters() {
            let a = d.add(wrap, "div.alphaPicker-fixed-right.MuiBox-root");
            let paper = d.add(a, "div.MuiPaper-root.MuiPaper-elevation");
            let grp = d.add(
                paper,
                "div.MuiToggleButtonGroup-root.MuiToggleButtonGroup-vertical",
            );
            for ch in lv::letters() {
                let on = br.letter == Some(ch);
                let b = d.add(
                    grp,
                    if on {
                        "button.MuiButtonBase-root.MuiToggleButtonGroup-grouped.Mui-selected"
                    } else {
                        "button.MuiButtonBase-root.MuiToggleButtonGroup-grouped"
                    },
                );
                d.text(b, &ch.to_string());
                letters.push((
                    b,
                    Message::Browse(pid, BMsg::Letter(if on { None } else { Some(ch) })),
                ));
            }
        }
        let loading = Self::browse_loading(br);
        let items = d.add(
            wrap,
            "div.itemsContainer.padded-left.padded-right.vertical-wrap.jf-virtual",
        );
        if br.failed || loading || n == 0 {
            let note = d.add(items, "div.jf-note");
            d.text(
                note,
                if br.failed {
                    "This couldn't be loaded."
                } else if loading {
                    "Loading"
                } else {
                    "No items found."
                },
            );
            if br.failed {
                let b = d.add(note, "a.button-flat.emby-button.jf-retry");
                d.text(b, "Try again");
                tree.act(b, Message::PageRetry);
            }
        } else {
            d.virtual_list(items, n);
            let proto = card_doc(d, items, &blank_card(0), look_of(view, "movie"), "x").card;
            d.virtual_proto(proto);
            let data = self.grid_data(br, &m, geo, old, true);
            tree.builders.insert(items, builder(data.clone(), view));
            for c in data.borrow().cards.iter().flatten() {
                if let Pic::Ready(h) | Pic::Blur(h) = &c.pic {
                    tree.images.insert(c.pic_id, h.clone());
                    if matches!(c.pic, Pic::Ready(_)) {
                        tree.done.insert(c.pic_id);
                    }
                }
            }
            tree.grid = Some(GridRef { node: items, data });
        }
        for (b, m) in letters {
            tree.act(b, m);
        }
        let _ = (GRID_PIC, Kind::Movie);
        Some(tree)
    }
}

/// Unused here: the engine's own `Browse` import keeps the module's types together.
#[allow(dead_code)]
fn _types(_: &Browse) {}
