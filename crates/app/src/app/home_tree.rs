//! Home as an element tree for the theme engine (`nocturne-style`). The builders emit the stage-1 vocabulary as classes (`.hero`, `.hero-title`, `.row`, `.row-title`, `.row-arrows`, `.card`, `.card-image`, `.card-title`, `.card-meta`, `.card-badge`, `.card-progress`, `.button`, scopes `.page-home` and `.section-resume`, `.section-next-up`, `.section-latest`) with data attributes (`data-kind`, `data-watched`, `data-source`). The stage-1 translator is this file: the resolved `Geo` / `Look` of each strip (the same numbers the hand-written Home uses) become inline styles, so every existing theme keeps its geometry; the default sheet (`crates/style/themes/default.css`) draws everything else from the `--nocturne-*` tokens. The class lists are open for the conformance aliases of a later step.
use super::cells::{badge, mark_for};
use super::header_tree::{Tree, dims, px};
use super::home::{Geo, HRow, Home, band_pad, hero_cfg};
use super::message::{At, CellAct, Message};
use super::{App, Load};
use crate::theme as t;
use crate::widget::cell::Pic;
use crate::widget::text::tracked;
use nocturne_core::customize::{self as cz, Page, TitlePosition};
use nocturne_core::source::{Art, Kind, Media};
use nocturne_style::{Doc, Id};

fn kind_attr(k: Kind) -> &'static str {
    match k {
        Kind::Movie => "movie",
        Kind::Series => "series",
        Kind::Season => "season",
        Kind::Episode => "episode",
        Kind::Photo => "photo",
        Kind::Book => "book",
        Kind::Folder => "folder",
        Kind::Person => "person",
        _ => "other",
    }
}

fn section_class(k: HRow) -> &'static str {
    match k {
        HRow::Resume => "section-resume",
        HRow::NextUp => "section-next-up",
        HRow::Tiles => "section-tiles",
        _ => "section-latest",
    }
}

/// A stylesheet fill as a CSS background value.
fn fill_css(f: &cz::Fill) -> String {
    match f {
        cz::Fill::Solid(c) => rgba(*c),
        cz::Fill::Linear { angle, stops } => {
            let s: Vec<String> = stops
                .iter()
                .map(|(c, at)| format!("{} {}%", rgba(*c), (at * 1000.0).round() / 10.0))
                .collect();
            format!("linear-gradient({angle}deg, {})", s.join(", "))
        }
    }
}

fn rgba(c: cz::Color) -> String {
    let [r, g, b, a] = c.to_f32();
    t::css_color(iced::Color { r, g, b, a })
}

impl App {
    /// Whether the active theme only uses things the tree builders reproduce; otherwise Home stays hand-written (so no theme can look different on the engine).
    pub(super) fn engine_supports_home(&self) -> bool {
        let hdr = t::parts(None, None);
        let sections = [
            cz::Section::Resume,
            cz::Section::NextUp,
            cz::Section::Latest,
        ];
        // Link icons are SVG (the engine draws raster images); the library tiles' centred label is drawn over the image by the hand-written cell.
        hdr.header.link_icons != Some(true)
            && sections.iter().all(|s| {
                let p = t::parts(Some(Page::Home), Some(*s));
                p.card.tile_label != Some(cz::TileLabel::OverlayCenter)
            })
    }

    /// The whole Home page (hero, strips, notes) and, when `with_header`, the header as one tree. `None` when Home shows its failure panel or the theme needs the hand-written page.
    pub(super) fn home_tree(&self, with_header: bool) -> Option<Tree> {
        let p = self.stack.last()?;
        if matches!(p.libs, Load::Failed) || !self.engine_supports_home() {
            return None;
        }
        let h = p.home.as_deref()?;
        let mut tree = Tree::new();
        let hp = t::parts(Some(Page::Home), None);
        let hh = t::header_h();
        let pushes = t::header_pushes();
        if with_header {
            self.header_nodes(&mut tree, 0, !pushes);
        }
        let lay = h.layout();
        let hero_focus = (lay.get(h.row) == Some(&HRow::Hero)).then_some(h.col);
        let band = |tree: &mut Tree| tree.doc.add(0, "div.band");
        // The hero, or the room the header leaves.
        match hero_cfg(&hp, self.win.width, self.win.height) {
            Some((hgt, banner)) => {
                let b = band(&mut tree);
                if banner && !pushes {
                    let sp = tree.doc.add(b, "div.hero-pad");
                    tree.doc.inline(sp, &format!("height: {}", px(hh)));
                }
                let hero = tree.doc.add(b, "section.hero.hero-billboard");
                if banner {
                    tree.doc.nodes[hero].classes.push("hero-banner".into());
                }
                match h.hero() {
                    Some(m) => self.hero_nodes(&mut tree, hero, m, hgt, hero_focus),
                    None => {
                        let hgt = 420f32.min(hgt);
                        tree.doc.inline(
                            hero,
                            &format!("height: {}; padding-top: {}", px(hgt), px(hh)),
                        );
                        let n = tree.doc.add(hero, "div.row-note");
                        tree.doc.text(
                            n,
                            if h.loading() {
                                "Loading your library"
                            } else {
                                "Nothing to show yet."
                            },
                        );
                    }
                }
                tree.doc.add(b, "div.band-rule");
            }
            None => {
                let b = band(&mut tree);
                if !(with_header && pushes) {
                    let sp = tree.doc.add(b, "div.hero-pad");
                    tree.doc.inline(sp, &format!("height: {}", px(hh)));
                }
                tree.doc.add(b, "div.band-rule");
            }
        }
        if !lay.iter().any(|k| !matches!(k, HRow::Nav | HRow::Hero))
            && !lay.contains(&HRow::Hero)
            && h.loading()
        {
            let n = tree.doc.add(0, "div.row-note");
            tree.doc.text(n, "Loading your library");
        }
        let gs = self.geos();
        let several = self.several_sources();
        for (ri, k) in lay.iter().enumerate() {
            if matches!(k, HRow::Nav | HRow::Hero) {
                continue;
            }
            self.strip_nodes(&mut tree, h, ri, *k, gs.of(*k), several);
        }
        let down: Vec<String> = self
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
        if !down.is_empty() {
            let n = tree.doc.add(0, "div.row-down");
            tree.doc
                .text(n, &format!("Not showing {}", down.join("  \u{b7}  ")));
        }
        let end = tree.doc.add(0, "div.page-end");
        tree.doc
            .inline(end, &format!("height: {}", px(t::S8 * 2.0)));
        tree.doc.attr(0, "class-scope", "page-home");
        tree.doc.nodes[0].classes.push("page-home".into());
        Some(tree)
    }

    fn pic_node(&self, tree: &mut Tree, parent: Id, class: &str, pic: &Pic) -> Option<Id> {
        let handle = match pic {
            Pic::Ready(h) | Pic::Blur(h) => h.clone(),
            _ => return None,
        };
        let id = tree.image(handle.clone());
        let n = tree.doc.add(parent, class);
        let (w, h) = dims(&handle);
        tree.doc.image(n, id, w, h);
        Some(n)
    }

    fn hero_nodes(&self, tree: &mut Tree, hero: Id, m: &Media, hgt: f32, focus: Option<usize>) {
        let w = self.win.width;
        let art = if m.backdrop_tag.is_none() && m.kind == Kind::Episode {
            Art::Landscape
        } else {
            Art::Backdrop
        };
        let hp = t::parts(Some(Page::Home), None);
        tree.doc.inline(hero, &format!("height: {}", px(hgt)));
        tree.doc.attr(hero, "data-kind", kind_attr(m.kind));
        let a = tree.doc.add(hero, "div.hero-art");
        tree.doc.add(a, "div.hero-fill");
        self.pic_node(tree, a, "img.hero-img", &self.pic(m, art, w as u32));
        tree.doc.add(hero, "div.hero-scrim-side");
        let foot = tree.doc.add(hero, "div.hero-scrim-foot");
        if let Some(f) = &hp.hero.overlay {
            tree.doc
                .inline(foot, &format!("background: {}", fill_css(f)));
        }
        let body = tree.doc.add(hero, "div.hero-body");
        let col = tree.doc.add(body, "div.hero-col");
        let d = &mut tree.doc;
        let tag = d.add(col, "div.hero-tag");
        d.text(tag, &format!("0x00 \u{b7} {}", m.kind_label()));
        let chips = d.add(col, "div.hero-chips");
        let chip = |d: &mut Doc, s: &str, accent: bool| {
            let c = d.add(
                chips,
                if accent {
                    "div.chip.accent"
                } else {
                    "div.chip"
                },
            );
            d.text(c, &tracked(s));
        };
        chip(d, &m.kind_label(), true);
        if let Some(y) = m.year {
            chip(d, &y.to_string(), false);
        }
        if let Some(r) = m.runtime_min {
            chip(d, &nocturne_core::source::fmt_runtime(r), false);
        }
        for g in m.genres.iter().take(3) {
            chip(d, g, false);
        }
        let size = (w / 26.0).clamp(40.0, 72.0);
        let title_ty = t::typo(&hp.hero_title, size, iced::font::Weight::Bold).with_line(1.08);
        let sub_ty = t::typo(&hp.hero_meta, t::BODY_LG, iced::font::Weight::Normal).with_line(1.6);
        if m.logo.is_some() {
            let pic = self.pic(m, Art::Logo, 460);
            if self.pic_node(tree, col, "img.hero-logo", &pic).is_none() {
                tree.doc.add(col, "div.hero-logo-gap");
            }
        } else if !title_ty.hidden {
            let n = tree.doc.add(col, "div.hero-title");
            tree.doc.text(n, &m.title);
            tree.doc.inline(n, &title_ty.css());
        }
        let d = &mut tree.doc;
        if m.logo.is_some() && matches!(m.kind, Kind::Season | Kind::Episode) {
            let n = d.add(col, "div.hero-season");
            d.text(n, &m.title);
        }
        if !sub_ty.hidden {
            let n = d.add(col, "div.hero-meta");
            d.text(n, &m.overview.clone().unwrap_or_else(|| m.meta_line()));
            d.inline(
                n,
                &format!(
                    "{}; max-height: {}",
                    sub_ty.css(),
                    px(sub_ty.size() * 1.6 * 3.0)
                ),
            );
        }
        let acts = d.add(col, "div.hero-actions");
        if !matches!(
            m.kind,
            Kind::Book | Kind::Folder | Kind::Person | Kind::Other
        ) {
            let b = d.add(acts, "a.button.primary");
            let i = d.add(b, "span.button-icon");
            d.text(i, "\u{25B6}");
            let l = d.add(b, "span.button-label");
            d.text(
                l,
                if m.kind == Kind::Photo {
                    "View"
                } else {
                    "Play"
                },
            );
            tree.act(b, Message::Cell(At::Hero, CellAct::Play));
            tree.cell(b, (1, 0));
        }
        let d = &mut tree.doc;
        let b = d.add(acts, "a.button");
        let l = d.add(b, "span.button-label");
        d.text(l, "More info");
        tree.act(b, Message::Cell(At::Hero, CellAct::Open));
        tree.cell(b, (1, 1));
        let _ = focus;
    }

    fn strip_nodes(&self, tree: &mut Tree, h: &Home, ri: usize, k: HRow, g: &Geo, several: bool) {
        let items = h.items(k);
        let n = items.len();
        let (title, text_, meta, play, loading) = match k {
            HRow::Resume => (
                "Continue watching".to_string(),
                "Pick up where you left off.".to_string(),
                format!("{n} in progress"),
                true,
                false,
            ),
            HRow::NextUp => (
                "Next up".to_string(),
                "The next episode of what you're following.".to_string(),
                format!("{n} up next"),
                true,
                false,
            ),
            HRow::Tiles => (
                "My media".to_string(),
                "Your libraries.".to_string(),
                format!("{n} {}", if n == 1 { "library" } else { "libraries" }),
                false,
                false,
            ),
            HRow::Latest(i) => {
                let r = &h.latest[i];
                let mut title = format!("Latest in {}", r.lib.title);
                if several && let Some(s) = self.catalog.slot(r.origin) {
                    title = format!("{title}  \u{b7}  {}", crate::widget::source::mark_text(s));
                }
                (
                    title,
                    "Recently added, newest first.".to_string(),
                    match &r.items {
                        Load::Ready(v) => {
                            format!("{} \u{b7} {} titles", r.lib.kind_label(), v.len())
                        }
                        _ => r.lib.kind_label(),
                    },
                    false,
                    matches!(r.items, Load::Loading),
                )
            }
            _ => return,
        };
        let d = &mut tree.doc;
        let band = d.add(0, "div.band");
        let row = d.add(band, &format!("section.row.{}", section_class(k)));
        d.inline(row, &format!("padding: {} 0", px(band_pad())));
        tree.bands.insert(ri, row);
        if !g.title_t.hidden {
            self.head_nodes(tree, row, ri, g, &title, &text_, &meta);
        }
        let d = &mut tree.doc;
        let body = d.add(row, "div.row-body");
        d.inline(body, &format!("padding: 0 {}", px(g.inset)));
        let panel = d.add(body, "div.row-panel");
        let cells = d.add(
            panel,
            if g.grid {
                "div.row-cards.row-grid"
            } else {
                "div.row-cards"
            },
        );
        if g.grid {
            d.inline(
                cells,
                &format!(
                    "grid-template-columns: repeat({}, {})",
                    g.cols,
                    px(g.cell_w())
                ),
            );
        }
        tree.strips.insert(ri, cells);
        if g.framed && !g.grid {
            tree.doc.add(panel, "div.row-fade");
        }
        // The hairline frame is drawn over the cards and takes no room (as the hand-written panel's border).
        if g.framed {
            tree.doc.add(panel, "div.row-frame");
        }
        tree.doc.add(band, "div.band-rule");
        let writes = self.hooks.can_write();
        let ((cw, ch), art) = (g.dims, g.art);
        if items.is_empty() {
            for i in 0..if g.grid { g.cols.min(8) } else { 8 } {
                let pic = if loading { Pic::Loading } else { Pic::Missing };
                self.card_nodes(
                    tree,
                    cells,
                    g,
                    CardIn {
                        pic,
                        title: String::new(),
                        meta: String::new(),
                        index: i,
                        progress: None,
                        badge: None,
                        subtitle: false,
                        mark: None,
                        fav: false,
                        played: false,
                        writes,
                        at: None,
                        play,
                        kind: "other",
                        source: 0,
                    },
                );
            }
        }
        for (ci, (m, copies)) in items.iter().enumerate() {
            let episode = art == Art::Landscape && m.kind == Kind::Episode;
            let (title, meta) = if episode {
                (
                    m.series_name.clone().unwrap_or_else(|| m.title.clone()),
                    super::cells::episode_line(m, g.look.episode_format),
                )
            } else {
                (m.title.clone(), super::cells::caption(m))
            };
            self.card_nodes(
                tree,
                cells,
                g,
                CardIn {
                    pic: self.pic(m, art, cw as u32),
                    title,
                    meta,
                    index: ci,
                    progress: m.progress(),
                    badge: badge(m),
                    subtitle: episode,
                    mark: several
                        .then(|| mark_for(&self.catalog, m, *copies))
                        .flatten(),
                    fav: m.favorite,
                    played: m.played,
                    writes,
                    at: Some((ri, ci)),
                    play,
                    kind: kind_attr(m.kind),
                    source: m.origin.0 as usize,
                },
            );
        }
        let _ = ch;
    }

    #[allow(clippy::too_many_arguments)]
    fn head_nodes(
        &self,
        tree: &mut Tree,
        row: Id,
        ri: usize,
        g: &Geo,
        title: &str,
        text_: &str,
        meta: &str,
    ) {
        let mut d = &mut tree.doc;
        let head = d.add(row, "div.row-head");
        d.inline(
            head,
            &format!(
                "height: {}; padding: 0 {}; align-items: {}",
                px(g.head_title_h()),
                px(g.inset),
                if g.details { "flex-start" } else { "center" }
            ),
        );
        let titles = d.add(head, "div.row-titles");
        let ttl = d.add(titles, "div.row-title");
        d.text(ttl, title);
        d.inline(ttl, &g.title_t_css());
        if g.details {
            let b = d.add(titles, "div.row-text");
            d.text(b, text_);
            let m = d.add(head, "div.row-meta");
            d.text(m, meta);
            d.inline(m, "align-self: flex-end");
        }
        if g.arrows.shown() && !g.grid {
            let size = g.arrows.size.unwrap_or(22.0);
            let ink = g
                .arrows
                .color
                .map_or_else(|| "var(--nocturne-text)".to_string(), rgba);
            let arrows = d.add(head, "div.row-arrows");
            for (glyph, dir) in [("\u{2039}", -1.0), ("\u{203A}", 1.0)] {
                let a = d.add(arrows, "a.row-arrow");
                d.inline(
                    a,
                    &format!(
                        "width: {0}; height: {0}; font-size: {1}; color: {ink}",
                        px(size + 8.0),
                        px(size)
                    ),
                );
                let gl = d.add(a, "span.row-arrow-glyph");
                d.text(gl, glyph);
                tree.act(a, Message::HomeStripPage(ri, dir));
                d = &mut tree.doc;
            }
        }
    }

    fn card_nodes(&self, tree: &mut Tree, parent: Id, g: &Geo, c: CardIn) {
        let look = &g.look;
        let (cw, ch) = g.dims;
        let radius = look.radius.unwrap_or_else(t::r_card);
        let d = &mut tree.doc;
        let card = d.add(
            parent,
            if look.flat {
                "a.card.card-flat"
            } else {
                "a.card"
            },
        );
        d.attr(card, "data-kind", c.kind);
        d.attr(card, "data-source", &c.source.to_string());
        d.attr(
            card,
            "data-watched",
            if c.played { "true" } else { "false" },
        );
        d.inline(
            card,
            &format!(
                "width: {}; height: {}; padding: {}",
                px(g.cell_w()),
                px(look.cell_h(ch)),
                px(look.pad)
            ),
        );
        let img = d.add(card, "div.card-image");
        let shadow = look.shadow.map_or(String::new(), |s| {
            format!(
                "; box-shadow: {} {} {} {}",
                px(s.x),
                px(s.y),
                px(s.blur),
                rgba(s.color)
            )
        });
        d.inline(
            img,
            &format!(
                "width: {}; height: {}; border-radius: {}{shadow}",
                px(cw),
                px(ch),
                px(radius)
            ),
        );
        match &c.pic {
            Pic::Ready(_) | Pic::Blur(_) => {
                self.pic_node(tree, img, "img.card-img", &c.pic);
            }
            Pic::Missing => {
                let i = crate::widget::text::initial(&c.title);
                if !i.is_empty() {
                    let n = tree.doc.add(img, "div.card-initial");
                    tree.doc.text(n, &i);
                }
            }
            Pic::Loading => {}
        }
        let mut d = &mut tree.doc;
        d.add(img, "div.card-dim");
        if look.title == TitlePosition::Overlay && !look.tile_center {
            let o = d.add(img, "div.card-overlay");
            d.inline(
                o,
                &format!(
                    "position: absolute; left: 0; right: 0; bottom: 0; padding: 12px; display: flex; flex-direction: column; gap: 2px; background: linear-gradient(180deg, {}, {})",
                    t::css_color(iced::Color { a: 0.0, ..t::frame() }),
                    t::css_color(iced::Color { a: 0.92, ..t::frame() }),
                ),
            );
            if !look.title_t.hidden {
                let n = d.add(o, "div.card-title");
                d.text(n, &c.title);
                d.inline(
                    n,
                    &format!("{}; color: var(--nocturne-text)", look.title_t.css()),
                );
            }
        }
        if !look.flat {
            let n = d.add(img, "div.card-index");
            d.text(n, &format!("0x{:02x}", c.index + 1));
        }
        if let Some(mk) = &c.mark {
            let n = d.add(
                img,
                if c.progress.is_some() {
                    "div.card-mark.lifted"
                } else {
                    "div.card-mark"
                },
            );
            d.text(n, mk);
        }
        if let Some(b) = c.badge.as_ref().filter(|_| look.badge_shown()) {
            let n = d.add(img, "div.card-badge");
            d.text(n, b);
            let bs = &look.badge;
            let mut css = String::new();
            if let Some(bg) = bs.background {
                css += &format!("background: {};", rgba(bg));
            }
            if let Some(ink) = bs.color {
                css += &format!("color: {};", rgba(ink));
            }
            css += &format!("border-radius: {};", px(bs.radius.unwrap_or(4.0)));
            match bs.position {
                Some(cz::BadgePos::TopRight) => css += "top: 8px; bottom: auto;",
                Some(cz::BadgePos::TopLeft) => {
                    css += "top: 8px; bottom: auto; left: 8px; right: auto;"
                }
                _ => {}
            }
            if let Some(sz) = bs.font_size {
                let hgt = sz + 10.0;
                css += &format!(
                    "font-size: {}; font-family: inherit; font-weight: 600; height: {}; display: flex; align-items: center; justify-content: center;{}",
                    px(sz),
                    px(hgt),
                    if b.chars().count() == 1 {
                        format!("width: {}; padding: 0;", px(hgt))
                    } else {
                        String::new()
                    }
                );
            }
            d.inline(n, &css);
        }
        let shown_progress = c.progress.filter(|_| look.progress.hidden != Some(true));
        let bar_css = |p: &cz::Progress| {
            let mut s = format!("height: {};", px(p.height.unwrap_or(3.0)));
            if let Some(bg) = p.background {
                s += &format!("background: {};", rgba(bg));
            }
            s
        };
        if let Some(p) = shown_progress
            && look.bar_h() == 0.0
        {
            let n = d.add(img, "div.card-progress");
            d.inline(n, &bar_css(&look.progress));
            let f = d.add(n, "div.card-progress-fill");
            let mut css = format!("width: {}%", (p.clamp(0.0, 1.0) * 1000.0).round() / 10.0);
            if let Some(c) = look.progress.color {
                css += &format!("; background: {}", rgba(c));
            }
            d.inline(f, &css);
        }
        let chips = d.add(
            img,
            if c.fav || c.played {
                "div.card-chips.keep"
            } else {
                "div.card-chips"
            },
        );
        for (glyph, on, act) in [
            (
                if c.fav { "\u{2665}" } else { "\u{2661}" },
                c.fav,
                CellAct::Fav,
            ),
            ("\u{2713}", c.played, CellAct::Played),
        ] {
            let b = d.add(
                chips,
                match (on, c.writes) {
                    (true, true) => "a.card-chip.on",
                    (true, false) => "a.card-chip.on.dim",
                    (false, true) => "a.card-chip",
                    (false, false) => "a.card-chip.dim",
                },
            );
            let gl = d.add(b, "span.card-chip-glyph");
            d.text(gl, glyph);
            if let Some((ri, ci)) = c.at {
                tree.act(b, Message::Cell(At::Home(ri, ci), act));
                d = &mut tree.doc;
            }
        }
        let d = &mut tree.doc;
        if look.bar_h() > 0.0 {
            let slot = d.add(card, "div.card-bar");
            d.inline(slot, &format!("height: {}", px(look.bar_h())));
            if let Some(p) = shown_progress {
                d.inline(slot, &bar_css(&look.progress));
                let f = d.add(slot, "div.card-progress-fill");
                d.inline(
                    f,
                    &format!(
                        "width: {}%; height: 100%",
                        (p.clamp(0.0, 1.0) * 1000.0).round() / 10.0
                    ),
                );
            }
        }
        let cap_h = look.caption_h();
        if cap_h > 0.0 {
            let cap = d.add(card, "div.card-caption");
            d.inline(
                cap,
                &format!("width: {}; height: {}", px(cw), px(cap_h - t::S3)),
            );
            let (meta_t, meta_set) = look.meta_typo(c.subtitle);
            if !look.title_t.hidden {
                let n = d.add(cap, "div.card-title");
                d.text(n, &c.title);
                let mut css = look.title_t.css();
                if look.center_title {
                    css += "; text-align: center";
                }
                d.inline(n, &format!("{css}; height: {}", px(look.title_h())));
            }
            if !meta_t.hidden {
                let n = d.add(cap, "div.card-meta");
                d.text(n, &c.meta);
                let mut css = if meta_set {
                    format!("{}; font-family: inherit", meta_t.css())
                } else {
                    String::new()
                };
                if look.center_meta {
                    css += "; text-align: center";
                }
                d.inline(n, &format!("{css}; height: {}", px(look.meta_h())));
                if c.subtitle {
                    d.nodes[n].classes.push("card-subtitle".into());
                }
            }
        }
        // Last child, so the focus ring and tint lie over everything in the cell.
        tree.doc.add(card, "div.card-ring");
        if let Some((ri, ci)) = c.at {
            tree.act(
                card,
                Message::Cell(
                    At::Home(ri, ci),
                    if c.play { CellAct::Play } else { CellAct::Open },
                ),
            );
            tree.cell(card, (ri, ci));
        }
    }
}

struct CardIn {
    pic: Pic,
    title: String,
    meta: String,
    index: usize,
    progress: Option<f32>,
    badge: Option<String>,
    subtitle: bool,
    mark: Option<String>,
    fav: bool,
    played: bool,
    writes: bool,
    at: Option<(usize, usize)>,
    play: bool,
    kind: &'static str,
    source: usize,
}

impl Geo {
    pub(super) fn title_t_css(&self) -> String {
        self.title_t.css()
    }
}
