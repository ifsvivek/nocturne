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
use nocturne_core::expand;
use nocturne_core::region::{self, Region};
use nocturne_core::source::{Art, Kind, Media};
use nocturne_style::{Doc, Id};

pub(super) fn kind_attr(k: Kind) -> &'static str {
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

/// `data-shape` of a card from the art box: what Jellyfin calls portrait, backdrop and square cards.
fn shape_name((w, h): (f32, f32)) -> &'static str {
    if (w - h).abs() <= 0.1 * w.max(h) {
        "square"
    } else if h > w {
        "portrait"
    } else {
        "backdrop"
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
    /// Whether the active theme only uses things the tree builders reproduce; otherwise Home stays hand-written (so no theme can look different on the engine). The library tiles' centred label (server UI mode) is drawn over the image by the hand-written cell.
    pub(super) fn engine_supports_home(&self, lay: &[HRow]) -> bool {
        !lay.contains(&HRow::Tiles)
            || t::parts(Some(Page::Home), None).card.tile_label
                != Some(cz::TileLabel::OverlayCenter)
    }

    /// The whole Home page (hero, strips, notes) and, when `with_header`, the header as one tree. `None` when Home shows its failure panel or the theme needs the hand-written page.
    pub(super) fn home_tree(&self, with_header: bool) -> Option<Tree> {
        let p = self.stack.last()?;
        if matches!(p.libs, Load::Failed) {
            return None;
        }
        let h = p.home.as_deref()?;
        if !self.engine_supports_home(&h.layout()) {
            return None;
        }
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
                    Some(m) => self.hero_nodes(&mut tree, hero, h, m, hgt, hero_focus),
                    None => {
                        let hgt = 420f32.min(hgt);
                        tree.doc.inline(
                            hero,
                            &format!("height: {}; padding-top: {}", px(hgt), px(hh)),
                        );
                        tree.doc.attr(
                            hero,
                            "data-region",
                            region_name(Region::of(h.loading(), false, 0)),
                        );
                        // The placeholder of a theme that draws one (hidden by the default sheet).
                        let sk = tree.doc.add(hero, "div.hero-skeleton");
                        for c in [
                            "hero-sk-title",
                            "hero-sk-line",
                            "hero-sk-line short",
                            "hero-sk-button",
                        ] {
                            tree.doc.add(sk, &format!("div.{}", c.replace(' ', ".")));
                        }
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
        tree.doc.nodes[0].classes.push("page-home".into());
        Some(tree)
    }

    /// A picture node under `parent` (always present, so an image that arrives later only needs its handle and size set): the cache keys let `Hosted::image_arrived` find it.
    fn pic_node(
        tree: &mut Tree,
        parent: Id,
        class: &str,
        pic: &Pic,
        key: (String, Option<String>),
    ) -> Id {
        let id = tree.pic(Some(key), pic);
        let n = tree.doc.add(parent, class);
        let (w, h) = match pic {
            Pic::Ready(h) | Pic::Blur(h) => dims(h),
            _ => (1, 1),
        };
        tree.doc.image(n, id, w, h);
        n
    }

    #[allow(clippy::too_many_arguments)]
    fn hero_nodes(
        &self,
        tree: &mut Tree,
        hero: Id,
        h: &Home,
        m: &Media,
        hgt: f32,
        focus: Option<usize>,
    ) {
        let w = self.win.width;
        let art = if m.backdrop_tag.is_none() && m.kind == Kind::Episode {
            Art::Landscape
        } else {
            Art::Backdrop
        };
        let hp = t::parts(Some(Page::Home), None);
        tree.doc.inline(hero, &format!("height: {}", px(hgt)));
        tree.doc.attr(hero, "data-kind", kind_attr(m.kind));
        tree.doc.attr(hero, "data-region", "ready");
        let a = tree.doc.add(hero, "div.hero-art");
        tree.doc.add(a, "div.hero-fill");
        Self::pic_node(
            tree,
            a,
            "img.hero-img",
            &self.pic(m, art, w as u32),
            self.pic_keys(m, art, w as u32),
        );
        // The slide that is leaving, over the new picture: built opaque, faded out by an in-place edit (`hero_settle`).
        if let Some(prev) = Self::hero_fading(h).filter(|p| (p.origin, &p.id) != (m.origin, &m.id))
        {
            let prev_art = if prev.backdrop_tag.is_none() && prev.kind == Kind::Episode {
                Art::Landscape
            } else {
                Art::Backdrop
            };
            let f = tree.doc.add(hero, "div.hero-fade");
            tree.doc.attr(f, "data-phase", "a");
            Self::pic_node(
                tree,
                f,
                "img.hero-img",
                &self.pic(prev, prev_art, w as u32),
                self.pic_keys(prev, prev_art, w as u32),
            );
            tree.hero_fx.fade = Some(f);
        }
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
        d.text(tag, &h.hero_tag(m));
        let chips = d.add(col, "div.hero-chips");
        // Each chip carries its text twice: tracked capitals (the default look) and plain (for themes that set the facts as a line of text).
        let chip = |d: &mut Doc, s: &str, kind: &str, accent: bool| {
            let c = d.add(
                chips,
                &format!(
                    "div.chip.chip-{kind}{}",
                    if accent { ".accent" } else { "" }
                ),
            );
            let t = d.add(c, "span.chip-text");
            d.text(t, &tracked(s));
            let p = d.add(c, "span.chip-plain");
            let mut cs = s.chars();
            let plain: String = cs
                .next()
                .map(|f| f.to_uppercase().chain(cs).collect())
                .unwrap_or_default();
            d.text(p, &plain);
        };
        // Facts in reading order (rating, year, kind, runtime, genres); the default sheet puts the kind chip first.
        if let Some(r) = m.rating.filter(|r| *r > 0.0) {
            chip(d, &format!("{r:.1}"), "rating", false);
        }
        if let Some(y) = m.year {
            chip(d, &y.to_string(), "year", false);
        }
        chip(d, &m.kind_label(), "kind", true);
        if let Some(r) = m.runtime_min {
            chip(d, &nocturne_core::source::fmt_runtime(r), "runtime", false);
        }
        for g in m.genres.iter().take(3) {
            chip(d, g, "genre", false);
        }
        let size = (w / 26.0).clamp(40.0, 72.0);
        let title_ty = t::typo(&hp.hero_title, size, iced::font::Weight::Bold).with_line(1.08);
        let sub_ty = t::typo(&hp.hero_meta, t::BODY_LG, iced::font::Weight::Normal).with_line(1.6);
        if m.logo.is_some() {
            let pic = self.pic(m, Art::Logo, 460);
            Self::pic_node(
                tree,
                col,
                "img.hero-logo",
                &pic,
                self.pic_keys(m, Art::Logo, 460),
            );
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
        let mut meta_node = None;
        if !sub_ty.hidden {
            let text = m.overview.clone().unwrap_or_else(|| m.meta_line());
            let key = super::hero_ui::hero_key(m);
            let open = h.expanded.is_open(&key);
            // Three lines, then an ellipsis; a click opens the rest.
            let cols = ((560f32.min(w) / (sub_ty.size() * 0.5)).floor().max(10.0)) as usize;
            let (clamped, cut) = expand::clamp(&text, cols, 3);
            let n = d.add(col, "div.hero-meta");
            d.text(n, if cut && !open { &clamped } else { &text });
            d.attr(n, "data-expanded", if open { "true" } else { "false" });
            d.inline(
                n,
                &if open {
                    sub_ty.css()
                } else {
                    format!(
                        "{}; max-height: {}",
                        sub_ty.css(),
                        px(sub_ty.size() * 1.6 * 3.0)
                    )
                },
            );
            if cut {
                d.attr(n, "title", "Click to expand");
                meta_node = Some(n);
            }
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
        // The description opens on a click, and on Enter when a theme makes it a keyboard stop (the default look keeps its two buttons).
        if let Some(n) = meta_node {
            tree.act(n, Message::HeroToggle);
            if self.hero_themed() {
                tree.cell(n, (1, 2));
            }
        }
        // Arrows, dots and a progress hairline for the carousel (hidden by the default sheet); only while the hero shows a slide, not a focused card.
        let slides = h.slides.len();
        let on_slide = h
            .slides
            .get(h.carousel.index())
            .is_some_and(|x| (x.origin, &x.id) == (m.origin, &m.id));
        if slides > 1 && on_slide {
            let d = &mut tree.doc;
            let nav = d.add(hero, "div.hero-nav");
            let mut arrows = vec![];
            for (glyph, dir) in [("\u{2039}", -1isize), ("\u{203A}", 1)] {
                let a = d.add(nav, "a.hero-arrow");
                d.attr(a, "data-dir", if dir < 0 { "prev" } else { "next" });
                let gl = d.add(a, "span.hero-arrow-glyph");
                d.text(gl, glyph);
                arrows.push((a, dir));
            }
            let dots = d.add(hero, "div.hero-dots");
            let mut ds = vec![];
            for i in 0..slides {
                let a = d.add(dots, "a.hero-dot");
                d.attr(
                    a,
                    "data-active",
                    if i == h.carousel.index() {
                        "true"
                    } else {
                        "false"
                    },
                );
                ds.push((a, i));
            }
            let pg = d.add(hero, "div.hero-progress");
            let fill = d.add(pg, "div.hero-progress-fill");
            d.inline(
                fill,
                &format!("width: {:.1}%", h.carousel.progress() * 100.0),
            );
            tree.hero_fx.progress = Some(fill);
            for (a, dir) in arrows {
                tree.act(a, Message::HeroStep(dir));
            }
            for (a, i) in ds {
                tree.act(a, Message::HeroGoto(i));
            }
        }
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
        tree.doc.attr(
            row,
            "data-region",
            region_name(Region::of(loading, false, n)),
        );
        if !g.title_t.hidden {
            Self::head_nodes(tree, row, ri, g, &title, &text_, &meta);
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
        if !g.grid {
            // The quiet 4 px bar of the hand-written strips, shown while the pointer is over the strip.
            tree.doc.attr(cells, "data-scrollbar", "hover-x");
        }
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
            // Placeholders: as many as fit the width while the strip loads, the usual eight otherwise.
            let fit = region::skeleton_cells(self.win.width - 2.0 * g.inset, g.cell_w(), 0.0, 8);
            for i in 0..if g.grid {
                g.cols.min(8)
            } else if loading {
                fit
            } else {
                8
            } {
                let pic = if loading { Pic::Loading } else { Pic::Missing };
                Self::card_nodes(
                    tree,
                    cells,
                    g,
                    CardIn {
                        pic,
                        pic_id: 0,
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
            Self::card_nodes(
                tree,
                cells,
                g,
                CardIn {
                    pic: self.pic(m, art, cw as u32),
                    pic_id: 0,
                    key: Some(self.pic_keys(m, art, cw as u32)),
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
        let tt = d.add(ttl, "span.row-title-text");
        d.text(tt, title);
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

    /// A card of Home's strips: the card nodes from [`card_doc`], its picture registered for a later arrival, its clicks and its focus cell.
    fn card_nodes(tree: &mut Tree, parent: Id, g: &Geo, mut c: CardIn) {
        c.pic_id = tree.pic(c.key.take(), &c.pic);
        let n = card_doc(&mut tree.doc, parent, g, &c);
        if let Some((ri, ci)) = c.at {
            for (chip, act) in n.chips.into_iter().zip([CellAct::Fav, CellAct::Played]) {
                tree.act(chip, Message::Cell(At::Home(ri, ci), act));
            }
            tree.act(n.play, Message::Cell(At::Home(ri, ci), CellAct::Play));
            tree.act(
                n.card,
                Message::Cell(
                    At::Home(ri, ci),
                    if c.play { CellAct::Play } else { CellAct::Open },
                ),
            );
            tree.cell(n.card, (ri, ci));
        }
    }
}

/// The nodes of a card an action can be bound to.
pub(super) struct CardNodes {
    pub card: Id,
    /// The favourite and the played chip.
    pub chips: [Id; 2],
    pub play: Id,
}

/// One card as a subtree of `d` under `parent`: the stage-1 look from the strip's `Geo` and the card's data. Used by Home's strips and the library grid's virtual items (which have only a `Doc` to build into).
pub(super) fn card_doc(d: &mut Doc, parent: Id, g: &Geo, c: &CardIn) -> CardNodes {
    {
        let look = &g.look;
        let (cw, ch) = g.dims;
        let radius = look.radius.unwrap_or_else(t::r_card);
        let card = d.add(
            parent,
            if look.flat {
                "a.card.card-flat"
            } else {
                "a.card"
            },
        );
        d.attr(card, "data-kind", c.kind);
        d.attr(card, "data-shape", shape_name(g.dims));
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
            // The node stays while the picture loads (it draws nothing until its handle is in the widget's table), so an arriving image is filled in without rebuilding the scene.
            Pic::Ready(_) | Pic::Blur(_) | Pic::Loading => {
                let n = d.add(img, "img.card-img");
                let (w, h) = match &c.pic {
                    Pic::Ready(h) | Pic::Blur(h) => dims(h),
                    _ => (1, 1),
                };
                d.image(n, c.pic_id, w, h);
            }
            Pic::Missing => {
                let i = crate::widget::text::initial(&c.title);
                if !i.is_empty() {
                    let n = d.add(img, "div.card-initial");
                    let g = d.add(n, "span.card-initial-glyph");
                    d.text(g, &i);
                }
            }
        }
        d.add(img, "div.card-dim");
        if look.title == TitlePosition::Overlay && !look.tile_center {
            let o = d.add(img, "div.card-overlay");
            d.inline(
                o,
                "position: absolute; left: 0; right: 0; bottom: 0; padding: 12px; display: flex; flex-direction: column; gap: 2px; background: linear-gradient(180deg, alpha(var(--nocturne-frame), 0), alpha(var(--nocturne-frame), 0.92))",
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
        let mut chip_nodes = [0; 2];
        for (k, (glyph, on)) in [
            (if c.fav { "\u{2665}" } else { "\u{2661}" }, c.fav),
            ("\u{2713}", c.played),
        ]
        .into_iter()
        .enumerate()
        {
            let b = d.add(
                chips,
                match (on, c.writes) {
                    (true, true) => "a.card-chip.on",
                    (true, false) => "a.card-chip.on.dim",
                    (false, true) => "a.card-chip",
                    (false, false) => "a.card-chip.dim",
                },
            );
            d.attr(b, "data-act", if k == 0 { "fav" } else { "played" });
            d.attr(
                b,
                "data-action",
                if k == 0 { "favourite" } else { "watched" },
            );
            let gl = d.add(b, "span.card-chip-glyph");
            d.text(gl, glyph);
            chip_nodes[k] = b;
        }
        // A play fab for themes that show one (hidden by the default sheet, so out of focus order and hit tests).
        let play = d.add(img, "a.card-play");
        d.attr(play, "data-act", "play");
        d.attr(play, "data-action", "play");
        let gl = d.add(play, "span.card-play-glyph");
        d.text(gl, "\u{25B6}");
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
        d.add(card, "div.card-ring");
        CardNodes {
            card,
            chips: chip_nodes,
            play,
        }
    }
}

fn region_name(r: Region) -> &'static str {
    match r {
        Region::Loading => "loading",
        Region::Ready => "ready",
        Region::Empty => "empty",
        Region::Error => "error",
    }
}

pub(crate) struct CardIn {
    pub pic: Pic,
    /// The picture node's id in the widget's image table, and the cache keys it waits for.
    pub pic_id: u32,
    pub key: Option<(String, Option<String>)>,
    pub title: String,
    pub meta: String,
    pub index: usize,
    pub progress: Option<f32>,
    pub badge: Option<String>,
    pub subtitle: bool,
    pub mark: Option<String>,
    pub fav: bool,
    pub played: bool,
    pub writes: bool,
    pub at: Option<(usize, usize)>,
    pub play: bool,
    pub kind: &'static str,
    pub source: usize,
}

impl Geo {
    pub(super) fn title_t_css(&self) -> String {
        self.title_t.css()
    }
}

#[cfg(test)]
mod tests {
    //! The existing geometry (`cell_h`, `band_h`, row dims) holds through the engine: the default sheet and the inline sizes of the tree builders lay out to the numbers the hand-written Home computes.
    use super::*;
    use crate::app::engine_ui::sheet_css;
    use crate::app::home::{band_h, geo_for, row_dims};
    use nocturne_core::customize::Parts;
    use nocturne_style::{Builders, Engine, Interaction, Scene, approx};

    fn scene(tree: &Tree, w: f32) -> Scene {
        Engine::new(&sheet_css()).build(
            &tree.doc,
            (w, 900.0),
            Interaction::default(),
            &Builders::new(),
            &mut approx,
        )
    }

    fn find(s: &Scene, class: &str) -> Vec<Id> {
        (0..s.doc.len())
            .filter(|&i| s.doc.nodes[i].classes.iter().any(|c| c == class))
            .collect()
    }

    fn card(i: usize) -> CardIn {
        CardIn {
            pic: Pic::Loading,
            pic_id: 0,
            key: None,
            title: "A title".into(),
            meta: "movie".into(),
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
            kind: "movie",
            source: 0,
        }
    }

    #[test]
    fn a_card_is_as_big_as_the_hand_written_cell() {
        for k in [HRow::Latest(0), HRow::Resume] {
            let g = geo_for(1280.0, 176.0, k, &Parts::default());
            let mut tree = Tree::new();
            let strip = tree.doc.add(0, "div.row-cards");
            App::card_nodes(&mut tree, strip, &g, card(0));
            let s = scene(&tree, 1280.0);
            let c = s.rects[find(&s, "card")[0]];
            let img = s.rects[find(&s, "card-image")[0]];
            assert_eq!((c[2], c[3]), (g.cell_w(), g.look.cell_h(g.dims.1)), "{k:?}");
            assert_eq!((img[2], img[3]), g.dims, "{k:?}");
            assert_eq!(
                img[0] - c[0],
                g.look.pad,
                "the image starts after the padding"
            );
        }
        // Same numbers as the oracle the hand-written tests use.
        assert_eq!(
            geo_for(1280.0, 176.0, HRow::Latest(0), &Parts::default()).dims,
            row_dims(1280.0, 190.0, 1.5)
        );
    }

    #[test]
    fn a_band_is_as_tall_as_the_hand_written_band() {
        let g = geo_for(1280.0, 176.0, HRow::Latest(0), &Parts::default());
        let mut tree = Tree::new();
        let band = tree.doc.add(0, "div.band");
        let row = tree.doc.add(band, "section.row.section-latest");
        tree.doc
            .inline(row, &format!("padding: {} 0", px(band_pad())));
        App::head_nodes(&mut tree, row, 2, &g, "Latest", "Recently added.", "movies");
        let body = tree.doc.add(row, "div.row-body");
        tree.doc
            .inline(body, &format!("padding: 0 {}", px(g.inset)));
        let panel = tree.doc.add(body, "div.row-panel");
        let cells = tree.doc.add(panel, "div.row-cards");
        for i in 0..6 {
            App::card_nodes(&mut tree, cells, &g, card(i));
        }
        tree.doc.add(band, "div.band-rule");
        let s = scene(&tree, 1280.0);
        let b = s.rects[find(&s, "band")[0]];
        // `Geo::band` counts 2 px for the frame and the rule; the frame takes no room in either page (headless captures put the rules of the two pages at the same distance), so the real band is 1 px shorter than the estimate the hand-written page scrolls by.
        assert_eq!(b[3], g.band(6) - 1.0);
        assert_eq!(b[3], band_h(g.dims.1) - 1.0);
        let head = s.rects[find(&s, "row-head")[0]];
        assert_eq!(head[3], g.head_title_h());
    }

    #[test]
    fn the_primary_label_keeps_its_contrast_colour_under_a_theme_colour_on_the_button() {
        let mut tree = Tree::new();
        let b = tree.doc.add(0, "a.button.primary");
        let l = tree.doc.add(b, "span.button-label");
        tree.doc.text(l, "Play");
        let mut engine = Engine::new(&sheet_css());
        engine.set_user_css(Some(".button, .button.primary { color: rgb(255, 0, 0); }"));
        let s = engine.build(
            &tree.doc,
            (800.0, 600.0),
            Interaction::default(),
            &Builders::new(),
            &mut approx,
        );
        let label = find(&s, "button-label")[0];
        let want = t::active().on_accent;
        let got = s.style(label).color;
        assert_eq!(
            (got.r, got.g, got.b),
            (
                (want.r * 255.0).round() as u8,
                (want.g * 255.0).round() as u8,
                (want.b * 255.0).round() as u8
            )
        );
    }

    /// WCAG contrast of the primary label against the solid colour of its button under `user` CSS.
    fn primary_contrast(user: Option<&str>, hero: bool) -> f32 {
        let mut tree = Tree::new();
        let parent = if hero {
            let h = tree.doc.add(0, "section.hero");
            tree.doc.add(h, "div.hero-actions")
        } else {
            tree.doc.add(0, "div.detail-actions")
        };
        let b = tree.doc.add(
            parent,
            if hero {
                "a.button.primary"
            } else {
                "a.button.button-primary"
            },
        );
        let l = tree.doc.add(b, "span.button-label");
        tree.doc.text(l, "Play");
        let mut engine = Engine::new(&sheet_css());
        engine.set_user_css(user);
        let s = engine.build(
            &tree.doc,
            (800.0, 600.0),
            Interaction::default(),
            &Builders::new(),
            &mut approx,
        );
        let lum = |c: nocturne_core::customize::value::Color| {
            let f = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
        };
        let bg = s
            .style(find(&s, "button")[0])
            .bg
            .iter()
            .rev()
            .find_map(|f| match f {
                nocturne_style::paint::Fill::Solid(c) => Some(*c),
                _ => None,
            })
            .expect("a solid button fill");
        let fg = s.style(find(&s, "button-label")[0]).color;
        let (a, b) = (lum(bg), lum(fg));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn the_primary_label_reads_on_its_button_in_every_look() {
        let netflin_like = ":root { --nocturne-accent: #e50914; --nocturne-on-accent: #ffffff; }";
        for (name, css, hero) in [
            ("default", None, true),
            ("default detail", None, false),
            ("glass", Some(nocturne_style::STARTERS[1].1), true),
            ("glass detail", Some(nocturne_style::STARTERS[1].1), false),
            (
                "spotlight detail",
                Some(nocturne_style::STARTERS[0].1),
                false,
            ),
            ("accent theme", Some(netflin_like), true),
        ] {
            let r = primary_contrast(css, hero);
            assert!(r >= 4.5, "{name}: {r:.2}");
        }
    }
}
