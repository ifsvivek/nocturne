//! The shared header as an element tree for the theme engine (`nocturne-style`): the same pieces as `header.rs`, emitted with the stage-1 vocabulary as classes (`.header`, `.header-brand`, `.header-nav`, `.header-link`, `.header-icons`, `.header-search`) so a selector keeps matching. Controls that carry their own widget behaviour (history buttons with tooltips, the library menus with their pop-overs, the avatar, the icon buttons) are slots: real Iced elements hosted at the rectangle of their node.
use super::header::link_icon;
use super::home::HRow;
use super::message::Message;
use super::{App, GLOBALS};
use crate::theme as t;
use crate::widget::text::tracked;
use nocturne_core::customize::HeaderAlign;
use nocturne_core::unified::Link;
use nocturne_style::{Doc, Id};
use std::collections::HashMap;

/// A real Iced element hosted at a node (built at view time from the app state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Back,
    Forward,
    Libraries {
        active: bool,
        focused: bool,
    },
    Smart {
        active: bool,
        focused: bool,
    },
    People,
    Filter,
    Avatar,
    /// The library page's Sort button, Filters button and one button of the view switch (poster, thumb, list), with their pop-overs.
    Sort,
    Filters,
    View(nocturne_core::libview::ViewKind),
    /// The search overlay's query field.
    SearchInput,
    /// The small icon in front of a header link (`--header-link-icons: on`).
    Icon {
        name: &'static str,
        side: u16,
        lit: bool,
    },
}

/// What a tree builder produces besides the elements: what a click does, how focusable nodes map to the Home focus model, the images and the slots.
pub struct Tree {
    pub doc: Doc,
    pub images: HashMap<u32, iced::widget::image::Handle>,
    pub acts: HashMap<Id, Message>,
    /// Focusable node to the `(row, col)` of `Home` (row 0 is the header links).
    pub cells: HashMap<Id, (usize, usize)>,
    pub by_cell: HashMap<(usize, usize), Id>,
    /// Scroll container of a strip, by layout row.
    pub strips: HashMap<usize, Id>,
    /// The `.row` element of a strip, by layout row.
    pub bands: HashMap<usize, Id>,
    pub nav: Option<Id>,
    /// The element a list's selection sits on (the search overlay's current hit).
    pub sel: Option<Id>,
    /// The scrolling list of a screen with one (the search results).
    pub list: Option<Id>,
    pub slots: Vec<Slot>,
    /// The picture nodes of the tree by cache key, so an image that arrives later is a targeted update (no rebuild).
    pub pics: Vec<PicSlot>,
    /// Picture ids that already show the real image (a late blurhash stand-in must not replace it).
    pub done: std::collections::HashSet<u32>,
    /// Item builders of the virtual containers (the library grid).
    pub builders: nocturne_style::Builders,
    /// The library grid: its container and the card data its builder reads.
    pub grid: Option<super::browse_tree::GridRef>,
    /// The hero carousel's nodes the app edits in place (the fading-out layer, the progress fill).
    pub hero_fx: HeroFx,
    next_image: u32,
}

/// Nodes of Home's hero that change while the scene stays (so their transitions run).
#[derive(Clone, Copy, Default)]
pub struct HeroFx {
    pub fade: Option<Id>,
    pub progress: Option<Id>,
}

/// One picture node waiting for (or showing) pixels: the cache key of the real image and of its blurhash stand-in.
#[derive(Clone, Debug)]
pub struct PicSlot {
    pub id: u32,
    pub key: String,
    pub blur: Option<String>,
}

impl Tree {
    pub fn new() -> Self {
        Self::screen("home")
    }

    /// An empty tree for the screen named `name` (`data-screen` on the root).
    pub fn screen(name: &str) -> Self {
        let mut doc = Doc::new("page");
        doc.attr(0, "data-screen", name);
        doc.attr(
            0,
            "data-layout",
            if crate::couch::on() { "tv" } else { "desktop" },
        );
        Self {
            doc,
            images: HashMap::new(),
            acts: HashMap::new(),
            cells: HashMap::new(),
            by_cell: HashMap::new(),
            strips: HashMap::new(),
            bands: HashMap::new(),
            nav: None,
            sel: None,
            list: None,
            slots: vec![],
            pics: vec![],
            done: Default::default(),
            builders: nocturne_style::Builders::new(),
            grid: None,
            hero_fx: HeroFx::default(),
            next_image: 1,
        }
    }

    pub fn image(&mut self, h: iced::widget::image::Handle) -> u32 {
        let id = self.next_image;
        self.next_image += 1;
        self.images.insert(id, h);
        id
    }

    /// A picture node's id: its handle (when decoded) goes in `images`, and `key` (with the blurhash stand-in's key) is remembered so that `Hosted::image_arrived` can fill it in later.
    pub fn pic(
        &mut self,
        key: Option<(String, Option<String>)>,
        pic: &crate::widget::cell::Pic,
    ) -> u32 {
        use crate::widget::cell::Pic;
        let id = self.next_image;
        self.next_image += 1;
        if let Pic::Ready(h) | Pic::Blur(h) = pic {
            self.images.insert(id, h.clone());
        }
        if matches!(pic, Pic::Ready(_)) {
            self.done.insert(id);
        }
        if let Some((key, blur)) = key {
            self.pics.push(PicSlot { id, key, blur });
        }
        id
    }

    /// Host `slot` at `n` (a node that already has its size from the stylesheet or inline).
    pub fn slot(&mut self, n: Id, slot: Slot) {
        self.slots.push(slot);
        self.doc.slot(n, self.slots.len() as u32);
    }

    pub fn act(&mut self, n: Id, m: Message) {
        self.acts.insert(n, m);
    }

    pub fn cell(&mut self, n: Id, rc: (usize, usize)) {
        self.cells.insert(n, rc);
        self.by_cell.insert(rc, n);
        self.doc.focusable(n);
    }
}

/// Inline `px` with at most three decimals (so equal values share a style).
pub fn px(v: f32) -> String {
    format!("{}px", (v * 1000.0).round() / 1000.0)
}

impl App {
    /// The header as part of `tree.doc` under `parent`. `fixed`: it floats over the page (the built-in); otherwise it takes its own row.
    pub(super) fn header_nodes(&self, tree: &mut Tree, parent: Id, fixed: bool) -> Id {
        let home = self.stack.first();
        let libs = home.map_or(&[][..], |h| h.libs.items());
        let cur = self
            .stack
            .get(1)
            .and_then(|p| p.media.as_ref())
            .map(|m| (m.origin, m.id.as_str()));
        let at_home = self.stack.len() <= 1;
        let blur = self.admin_open;
        let nav_focus = self
            .stack
            .last()
            .filter(|_| at_home)
            .and_then(|p| p.home.as_deref())
            .filter(|h| h.layout().get(h.row) == Some(&HRow::Nav))
            .map(|h| h.col);
        let hp = t::parts(None, None);
        let icons = hp.header.link_icons == Some(true);
        let hh = t::header_h();
        let d = &mut tree.doc;
        let header = d.add(
            parent,
            if fixed {
                "header.header.header-fixed"
            } else {
                "header.header"
            },
        );
        d.inline(header, &format!("height: {}", px(hh)));
        if fixed {
            // Home's header lies over the hero and the strips: once the page has scrolled, this layer (the header's own fill again, so a transparent header stays transparent) hides what passes behind it, as the hand-written header does.
            d.add(header, "div.header-backdrop");
        }
        let bar = d.add(header, "div.header-bar");
        d.inline(bar, &format!("height: {}", px(hh - 1.0)));
        d.add(header, "div.header-rule");
        let left = d.add(bar, "div.header-left");
        let brand = d.add(left, "div.header-brand");
        let mut slots: Vec<(Id, Slot)> = vec![];
        let mut icon_slots: Vec<(Id, Slot)> = vec![];
        if !crate::couch::on() && t::header_item(t::HeaderItem::History) {
            for (s, last) in [(Slot::Back, false), (Slot::Forward, true)] {
                let n = d.add(brand, "div.slot");
                d.inline(
                    n,
                    &format!(
                        "width: 28px; height: 28px{}",
                        if last { "; margin-right: 16px" } else { "" }
                    ),
                );
                slots.push((n, s));
            }
        }
        if hp.header_brand.shown() {
            let b = &hp.header_brand;
            let ty = t::typo(
                &nocturne_core::customize::Text {
                    font_size: b.font_size,
                    font_weight: b.font_weight,
                    color: b.color,
                    ..Default::default()
                },
                t::BODY_LG,
                iced::font::Weight::Bold,
            );
            let n = d.add(brand, "div.header-name");
            d.text(n, &self.brand_text(b));
            d.inline(n, &ty.css());
        } else {
            let n = d.add(brand, "div.header-wordmark");
            d.text(n, &tracked("Nocturne"));
            d.inline(n, &format!("font-size: {}", px(t::CAPTION + 1.0)));
        }
        let nav = d.add(
            left,
            if hp.header.align == Some(HeaderAlign::Center) {
                "nav.header-nav.header-nav-center"
            } else {
                "nav.header-nav"
            },
        );
        tree.nav = Some(nav);
        // Chevrons and fades at the ends of the strip when links are hidden past them.
        d.attr(nav, "data-chevrons", "");
        for (n, link) in self.nav_links().into_iter().enumerate() {
            let focused = nav_focus == Some(n);
            let on = |b: bool| b && !at_home && !blur;
            let (label, active, msg): (String, bool, Option<Message>) = match link {
                Link::Home => ("Home".into(), at_home && !blur, Some(Message::GoHome)),
                Link::Kind(k) => (
                    k.label().into(),
                    on(cur.is_some_and(|c| c.1 == k.scope().key)),
                    Some(Message::OpenKind(k)),
                ),
                Link::Lib(i) => (
                    libs[i].title.clone(),
                    on(Some((libs[i].origin, libs[i].id.as_str())) == cur),
                    Some(Message::OpenLibrary(i)),
                ),
                Link::Global(gi) => (
                    GLOBALS[gi].1.into(),
                    on(cur.is_some_and(|c| c.1 == GLOBALS[gi].0)),
                    Some(Message::OpenGlobal(gi)),
                ),
                Link::Admin => ("Admin".into(), self.admin_open, Some(Message::OpenAdmin)),
                Link::Libraries | Link::Smart => (String::new(), false, None),
            };
            let d = &mut tree.doc;
            let a = d.add(
                nav,
                match (active, msg.is_some()) {
                    (true, _) => "a.header-link.active",
                    (false, true) => "a.header-link",
                    (false, false) => "div.header-link",
                },
            );
            d.inline(a, &format!("height: {}", px(hh)));
            if msg.is_none() {
                // The menus keep their pop-over: the whole link is a slot.
                let which = link;
                let ty = t::typo(&hp.header_link, t::BODY, iced::font::Weight::Medium);
                let w = text_w(
                    &self.nav_label(&which),
                    ty.size(),
                    hp.header_link.font_weight.unwrap_or(500),
                ) + 2.0 * t::S3;
                d.inline(
                    a,
                    &format!("height: {}; width: {}; padding: 0", px(hh), px(w)),
                );
                // As the hand-written links: Libraries lights for one of the libraries, Smart only for a smart collection.
                let act = on(cur.is_some_and(|c| match which {
                    Link::Libraries => libs.iter().any(|l| (l.origin, l.id.as_str()) == c),
                    _ => c.1.starts_with("smart-"),
                }));
                let slot = match which {
                    Link::Libraries => Slot::Libraries {
                        active: act,
                        focused,
                    },
                    _ => Slot::Smart {
                        active: act,
                        focused,
                    },
                };
                slots.push((a, slot));
                tree.cell(a, (0, n));
                continue;
            }
            let ty = t::typo(&hp.header_link, t::BODY, iced::font::Weight::Medium);
            if icons && let Some(name) = link_icon(&link, libs) {
                // The icon is SVG: a slot, tinted like the hand-written link does (lit when active or focused).
                let side = ty.size() + 3.0;
                let i = d.add(a, "span.header-icon");
                d.inline(i, &format!("width: {0}; height: {0}; flex: none", px(side)));
                icon_slots.push((
                    i,
                    Slot::Icon {
                        name,
                        side: side.round() as u16,
                        lit: active || focused,
                    },
                ));
            }
            let l = d.add(a, "span.header-label");
            d.text(l, &label);
            d.inline(l, &ty.css());
            d.add(a, "i.header-dot");
            if let Some(m) = msg {
                tree.act(a, m);
            }
            tree.cell(a, (0, n));
        }
        let mut d = &mut tree.doc;
        let right = d.add(bar, "div.header-icons");
        if self.catalog.online().count() > 1 {
            let n = d.add(right, "div.slot.header-filter");
            let w = text_w(&self.filter_label(), t::CONTROL_TEXT, 500).min(180.0)
                + 2.0 * t::S3
                + t::S2
                + 12.0;
            d.inline(n, &format!("width: {}; height: 32px", px(w.min(220.0))));
            slots.push((n, Slot::Filter));
        }
        if hp.header_icons.hidden == Some(false) {
            let n = d.add(right, "div.slot.header-people");
            d.inline(n, "width: 22px; height: 22px");
            slots.push((n, Slot::People));
        }
        if t::header_item(t::HeaderItem::Search) {
            let s = d.add(right, "a.header-search");
            d.essential(s);
            d.focusable(s);
            d.attr(s, "title", "Search (Ctrl+K or /)");
            let l = d.add(s, "span.header-search-label");
            d.text(l, "Search");
            d.inline(l, "font-size: 13px; color: var(--nocturne-text-secondary)");
            let h = d.add(s, "span.header-search-hint");
            d.text(h, "/");
            tree.act(s, Message::OpenSearch);
            d = &mut tree.doc;
        }
        if t::header_item(t::HeaderItem::Account) {
            let n = d.add(right, "div.slot.header-account");
            d.inline(n, "width: 32px; height: 32px");
            d.essential(n);
            slots.push((n, Slot::Avatar));
        }
        for (n, s) in slots.into_iter().chain(icon_slots) {
            tree.slot(n, s);
        }
        header
    }
}

/// Pixel size of a decoded image (the app only hands out RGBA handles; anything else is measured as 1x1 and fitted by its box).
pub fn dims(h: &iced::widget::image::Handle) -> (u32, u32) {
    match h {
        iced::widget::image::Handle::Rgba { width, height, .. } => (*width, *height),
        _ => (1, 1),
    }
}

/// Width of `s` as the real shaper measures it (for slots whose Iced element sizes itself).
pub(super) fn text_w(s: &str, size: f32, weight: u16) -> f32 {
    use nocturne_style::{FontChoice, TextStyle};
    crate::widget::styled_preview::shape(
        s,
        &TextStyle {
            size,
            weight,
            italic: false,
            font: FontChoice::Default,
            line_height: 1.3,
            lines: None,
            letter_spacing: 0.0,
            ellipsis: false,
        },
        None,
    )
    .0
}
