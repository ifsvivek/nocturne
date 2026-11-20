//! Home as jellyfin-web 12 builds it: `main > .skinBody > #indexPage.homePage > #homeTab > .homeSectionsContainer > .verticalSection` per section, each with a `.sectionTitle(-cards)` (a link button with a chevron for the sections that have a page) and an `.emby-scroller > .itemsContainer.scrollSlider` of cards. My Media tiles are backdrop cards of the libraries.
use super::super::cells::{badge, episode_line};
use super::super::header_tree::Tree;
use super::super::home::{Geo, HRow, Home};
use super::super::home_tree::{CardIn, kind_attr};
use super::super::message::Message;
use super::super::{App, Load};
use super::card::{Look, Shape, card};
use nocturne_core::source::{Kind, Media};
use nocturne_style::Id;

impl App {
    /// The whole Home page (header, sections) as one tree; `None` when Home shows its failure panel.
    pub(in crate::app) fn jf_home_tree(&self) -> Option<Tree> {
        let p = self.stack.last()?;
        if matches!(p.libs, Load::Failed) {
            return None;
        }
        let h = p.home.as_deref()?;
        let mut tree = Tree::new();
        let body = super::jf_root(&mut tree, "libraryDocument.withSectionTabs.mouseIdle");
        self.jf_header_nodes(&mut tree, body, None);
        let sections = home_frame(&mut tree, body);
        let gs = self.geos();
        let several = self.several_sources();
        let mut n = 0;
        for (ri, k) in h.layout().iter().enumerate() {
            if matches!(k, HRow::Nav | HRow::Hero) {
                continue;
            }
            if self.jf_section(&mut tree, sections, h, ri, *k, gs.of(*k), several, n) {
                n += 1;
            }
        }
        let note: Vec<String> = self
            .catalog
            .slots
            .iter()
            .filter(|s| s.profile.enabled && !(s.backend.is_some() && s.status.usable()))
            .map(|s| {
                format!(
                    "{}: {}",
                    s.profile.label,
                    nocturne_core::sources::status_text(&s.status, self.now).to_lowercase()
                )
            })
            .collect();
        if !note.is_empty() {
            let d = &mut tree.doc;
            let v = d.add(sections, "div.verticalSection");
            let t = d.add(v, "h2.sectionTitle.sectionTitle-cards.padded-left");
            d.text(t, &format!("Not showing {}", note.join("  \u{b7}  ")));
        }
        tree.doc.add(body, "div.jf-page-end");
        Some(tree)
    }

    /// One `.verticalSection`; `false` when the section has nothing to show (jellyfin-web hides it).
    #[allow(clippy::too_many_arguments)]
    fn jf_section(
        &self,
        tree: &mut Tree,
        parent: Id,
        h: &Home,
        ri: usize,
        k: HRow,
        g: &Geo,
        several: bool,
        n: usize,
    ) -> bool {
        let mut items = h.items(k);
        // The web client asks Next Up without the episodes already under Continue Watching.
        if k == HRow::NextUp {
            let resuming = h.items(HRow::Resume);
            items.retain(|(m, _)| {
                !resuming
                    .iter()
                    .any(|(r, _)| r.id == m.id && r.origin == m.origin)
            });
        }
        if items.is_empty() {
            return false;
        }
        let (title, linked, shape) = match k {
            HRow::Resume => ("Continue Watching".to_string(), false, Shape::Backdrop),
            HRow::NextUp => ("Next Up".to_string(), true, Shape::Backdrop),
            HRow::Tiles => ("My Media".to_string(), false, Shape::Backdrop),
            HRow::Latest(i) => {
                let lib = &h.latest[i].lib;
                let music = lib.collection.as_deref() == Some("music");
                let mut t = format!("Recently Added in {}", lib.title);
                if several && let Some(s) = self.catalog.slot(h.latest[i].origin) {
                    t = format!("{t}  \u{b7}  {}", crate::widget::source::mark_text(s));
                }
                (
                    t,
                    true,
                    if music {
                        Shape::Square
                    } else {
                        Shape::Portrait
                    },
                )
            }
            _ => return false,
        };
        let d = &mut tree.doc;
        let sec = d.add(parent, &format!("div.verticalSection.section{n}"));
        if linked {
            let c = d.add(
                sec,
                "div.sectionTitleContainer.sectionTitleContainer-cards.padded-left",
            );
            let a = d.add(
                c,
                "a.button-flat.button-flat-mini.sectionTitleTextButton.emby-button",
            );
            if k == HRow::NextUp {
                d.attr(a, "href", "#/list.html?type=nextup");
            }
            let t = d.add(a, "h2.sectionTitle.sectionTitle-cards");
            d.text(t, &title);
        } else {
            let t = d.add(sec, "h2.sectionTitle.sectionTitle-cards.padded-left");
            d.text(t, &title);
        }
        let scroller = d.add(
            sec,
            "div.padded-top-focusscale.padded-bottom-focusscale.emby-scroller.scrollX",
        );
        let cont = d.add(scroller, "div.itemsContainer.scrollSlider.focuscontainer-x");
        if matches!(k, HRow::Resume | HRow::NextUp) {
            d.attr(cont, "data-monitor", "videoplayback,markplayed");
        }
        tree.strips.insert(ri, scroller);
        tree.bands.insert(ri, sec);
        let writes = self.hooks.can_write();
        let (art, width, kind_tile) = (g.art, g.dims.0 as u32, k == HRow::Tiles);
        for (ci, (m, copies)) in items.iter().enumerate() {
            let _ = copies;
            let episode = m.kind == Kind::Episode;
            let (title, second) = if episode {
                (
                    m.series_name.clone().unwrap_or_else(|| m.title.clone()),
                    episode_line(m, Some(nocturne_core::customize::EpisodeFormat::Short)),
                )
            } else if kind_tile {
                (m.title.clone(), String::new())
            } else {
                (m.title.clone(), second_line(m))
            };
            let c = CardIn {
                pic: self.pic(m, art, width),
                pic_id: 0,
                key: Some(self.pic_keys(m, art, width)),
                title,
                meta: if kind_tile {
                    m.collection.clone().unwrap_or_default()
                } else {
                    String::new()
                },
                index: ci,
                progress: m.progress(),
                badge: badge(m),
                subtitle: episode,
                mark: None,
                fav: m.favorite,
                played: m.played,
                writes,
                at: Some((ri, ci)),
                play: false,
                kind: kind_attr(m.kind),
                source: m.origin.0 as usize,
            };
            let look = Look {
                shape,
                overflow: true,
                second: !kind_tile,
                folder: kind_tile || matches!(m.kind, Kind::Series | Kind::Season | Kind::Folder),
                person: false,
            };
            card(tree, cont, c, look, &second);
        }
        let _ = Message::GoHome;
        true
    }
}

/// The second caption line of a card: the year (movies, series), the artist (albums), the kind otherwise.
pub(crate) fn second_line(m: &Media) -> String {
    match m.kind {
        Kind::Movie | Kind::Series => m.year.map(|y| y.to_string()).unwrap_or_default(),
        Kind::MusicAlbum | Kind::Audio => m.sub.clone().unwrap_or_default(),
        _ => m.year.map(|y| y.to_string()).unwrap_or_default(),
    }
}

/// Home's page frame under `#reactRoot` (the spacer under the header, `main`, `.skinBody`, the page and its tab); returns `.homeSectionsContainer`.
/// The web client fills the window (`html, body, #reactRoot { height: 100% }`, the page absolute inside `main`) and the browser scrolls the overflow of that viewport. The engine scrolls the height of the root box, which ignores overflow: Home, the one page that scrolls as a whole, grows with its sections instead (the library page scrolls its own grid, the item page is not absolute). Without this the wheel, keys and scrollbar did nothing on Home.
fn home_frame(tree: &mut Tree, root: Id) -> Id {
    let d = &mut tree.doc;
    for n in [0, d.nodes[root].parent.unwrap_or(0), root] {
        d.inline(n, "height: auto !important");
    }
    let spacer = d.add(root, "div.jf-spacer");
    d.inline(spacer, "height: 48px");
    let main = d.add(root, "main.MuiBox-root");
    let skin = d.add(main, "div.skinBody");
    let page = d.add(
        skin,
        "div.page.mainAnimatedPage.homePage.libraryPage.allLibraryPage.pageWithAbsoluteTabs.withTabs",
    );
    d.inline(page, "position: relative !important");
    d.set_id(page, "indexPage");
    d.attr(page, "data-role", "page");
    let tab = d.add(page, "div.tabContent.pageTabContent.is-active");
    d.set_id(tab, "homeTab");
    d.add(tab, "div.sections.homeSectionsContainer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_style::{Builders, Fonts, Interaction, Scene, Sheet, approx};
    use std::rc::Rc;

    /// The web client's own rules that fix the page to the window, over the UA sheet.
    const WEB: &str = "body, html { height: 100%; margin: 0 } #reactRoot { height: 100% } .mainAnimatedPage { position: absolute; top: 0; bottom: 0; left: 0; right: 0 } .skinBody { position: static }";

    /// Home taller than the window must scroll as a whole: the wheel over a section, or over nothing, moves the page.
    #[test]
    fn home_scrolls_under_the_web_clients_fixed_height_page() {
        let mut tree = Tree::new();
        let root = super::super::jf_root(&mut tree, "libraryDocument.withSectionTabs.mouseIdle");
        let sections = home_frame(&mut tree, root);
        let mut rows = vec![];
        for _ in 0..8 {
            let v = tree.doc.add(sections, "div.verticalSection");
            tree.doc.inline(v, "height: 300px");
            rows.push(v);
        }
        let css = format!("{}\n{WEB}", super::super::UA);
        let mut s = Scene::build(
            tree.doc,
            Rc::new(Sheet::parse(&css)),
            Rc::new(Fonts::new()),
            (1280.0, 800.0),
            Interaction::default(),
            None,
            &Builders::new(),
            &mut approx,
        )
        .expect("builds");
        assert!(s.page_height() > 2400.0, "page is {}", s.page_height());
        assert!(s.wheel(Some(rows[0]), 0.0, -120.0, &mut approx));
        assert_eq!(s.scroll.page_y, 120.0);
        assert!(s.wheel(None, 0.0, -120.0, &mut approx));
        assert_eq!(s.scroll.page_y, 240.0);
    }
}
