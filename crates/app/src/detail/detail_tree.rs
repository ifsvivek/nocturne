//! The detail page as an element tree for the theme engine (`nocturne-style`). The tree carries the vocabulary a theme can style: `.page-detail`, `.detail` (the hero) with `.detail-backdrop`, `.detail-logo`, `.detail-title`, `.detail-meta`, `.detail-genres`, `.detail-text`, `.detail-actions`, the buttons (`.button`, `.button-primary`, `.button-icon`), `.detail-facts`, `.detail-tags`, `.detail-about`, `.section` with `.section-title` (seasons, episodes, cast and crew, chapters, similar items), `.episode`, `.poster`, `.detail-info`, with data attributes (`data-kind`, `data-region`, `data-watched`, `data-expanded`, `data-section`). The default sheet (`crates/style/themes/default.css`) draws today's page from the `--nocturne-*` tokens; geometry that the hand-written page fixes (hero height, band heights, card sizes) comes from `geom` as inline styles, so both pages agree. Interactive leaves keep the page's own focus model: every focusable node maps to a `(DRow, index)` of `Focus`, so the arrows, Enter and the hover behave as before.
use super::geom;
use super::model::{self, DAct, DRow, Sec};
use super::{Detail, Load, Msg, pic_key};
use crate::theme as t;
use crate::widget::text::{initial, tracked};
use iced::widget::image::Handle;
use nocturne_core::backend::PlayError;
use nocturne_core::detail::{self, Full};
use nocturne_core::expand;
use nocturne_core::region::{self, Region};
use nocturne_core::source::{Art, Kind, Media, fmt_runtime};
use nocturne_core::sync::NEED_WRITES;
use nocturne_style::{Doc, Id};
use std::collections::HashMap;

/// What the tree builder produces besides the elements.
pub struct DTree {
    pub doc: Doc,
    pub images: HashMap<u32, Handle>,
    /// What a click on a node (or its nearest ancestor with an entry) does.
    pub acts: HashMap<Id, Msg>,
    /// Focusable node to the page's `(row, index)` of `Focus`.
    pub cells: HashMap<Id, (DRow, usize)>,
    pub by_cell: HashMap<(DRow, usize), Id>,
    /// The pointer on these nodes moves the page's focus (poster cells, episode cards and rows), as the hand-written cells do.
    pub hover: HashMap<Id, Msg>,
    /// The "more / less" toggle of the description, when the text is longer than its clamp.
    pub toggle: Option<Id>,
    next_image: u32,
}

impl DTree {
    pub(super) fn new(kind: &str) -> Self {
        let mut doc = Doc::new("page");
        doc.attr(0, "data-screen", "detail");
        doc.attr(
            0,
            "data-layout",
            if crate::couch::on() { "tv" } else { "desktop" },
        );
        doc.attr(0, "data-kind", kind);
        doc.nodes[0].classes.push("page-detail".into());
        Self {
            doc,
            images: HashMap::new(),
            acts: HashMap::new(),
            cells: HashMap::new(),
            by_cell: HashMap::new(),
            hover: HashMap::new(),
            toggle: None,
            next_image: 1,
        }
    }

    pub(super) fn cell(&mut self, n: Id, rc: (DRow, usize)) {
        self.cells.insert(n, rc);
        self.by_cell.insert(rc, n);
        self.doc.focusable(n);
    }

    /// An `<img>` node of `class` under `parent` for `h`, `None` when the picture is not usable.
    pub(super) fn pic(&mut self, parent: Id, class: &str, h: &Handle) -> Id {
        self.pic_inner(parent, class, h)
    }

    /// Register a decoded picture for a node of a card built elsewhere; returns its id in the widget's image table.
    pub(super) fn reg(&mut self, h: &Handle) -> u32 {
        let id = self.next_image;
        self.next_image += 1;
        self.images.insert(id, h.clone());
        id
    }

    fn pic_inner(&mut self, parent: Id, class: &str, h: &Handle) -> Id {
        let id = self.next_image;
        self.next_image += 1;
        self.images.insert(id, h.clone());
        let n = self.doc.add(parent, class);
        let (w, hh) = dims(h);
        self.doc.image(n, id, w, hh);
        n
    }
}

fn px(v: f32) -> String {
    format!("{}px", (v * 1000.0).round() / 1000.0)
}

/// Pixel size of a picture (the page keeps encoded bytes; the header says how big they are). Unknown: a square, fitted by its box.
fn dims(h: &Handle) -> (u32, u32) {
    match h {
        Handle::Rgba { width, height, .. } => (*width, *height),
        Handle::Bytes(_, b) => image::ImageReader::new(std::io::Cursor::new(b.as_ref()))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_dimensions().ok())
            .unwrap_or((1, 1)),
        _ => (1, 1),
    }
}

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
        Kind::MusicAlbum => "album",
        Kind::MusicArtist => "artist",
        Kind::Audio => "audio",
        Kind::Playlist => "playlist",
        Kind::BoxSet => "collection",
        _ => "other",
    }
}

fn sec_name(s: Sec) -> &'static str {
    match s {
        Sec::Next => "next-up",
        Sec::Cast => "cast",
        Sec::Chapters => "chapters",
        Sec::Similar => "similar",
        Sec::Extras => "extras",
        Sec::Collections => "collections",
        Sec::Kids => "kids",
        Sec::Kids2 => "kids2",
        Sec::Seasons => "seasons",
    }
}

/// Columns a description of `width` px holds at `size` px (a character is about half the size wide).
fn cols_of(width: f32, size: f32) -> usize {
    (width / (size * 0.5)).floor().max(10.0) as usize
}

impl Detail {
    /// The key of this item's description in the open set.
    pub(super) fn expand_key(&self) -> String {
        format!("{}:{}", self.media.origin.0, self.media.id)
    }

    /// The page's tree (a pure function of the page's data, window and focus-independent state).
    pub(super) fn tree(&self) -> DTree {
        let m = &self.media;
        let mut t_ = DTree::new(kind_attr(m.kind));
        let full = self.full.as_ref().cloned().unwrap_or_default();
        t_.doc.add(0, "div.detail-top");
        // The sizes the hand-written page fixes are tokens on the page, so a theme can change the rule that uses them.
        t_.doc.inline(
            0,
            &format!(
                "--nocturne-detail-top: {}; --nocturne-detail-hero: {}",
                px(geom::top()),
                px(geom::hero_h(self.win))
            ),
        );
        self.hero_nodes(&mut t_);
        t_.doc.add(0, "div.band-rule");
        self.facts_nodes(&mut t_, &full);
        t_.doc.add(0, "div.band-rule");
        let chips = self.chips();
        if !chips.is_empty() {
            self.tags_nodes(&mut t_, chips);
            t_.doc.add(0, "div.band-rule");
        }
        self.about_nodes(&mut t_, &full);
        t_.doc.add(0, "div.band-rule");
        if model::has_list(m.kind) {
            self.list_nodes(&mut t_);
        }
        for s in Sec::ALL {
            let items = self.sec_items(s);
            if !items.is_empty() {
                self.strip_nodes(&mut t_, s, items);
            } else if self.sec_region(s).skeleton() {
                self.skeleton_nodes(&mut t_, s);
            }
        }
        if m.kind == Kind::Person {
            let all = [&self.kids, &self.kids2];
            if all.iter().all(|l| l.items().is_empty()) {
                let n = t_.doc.add(0, "div.detail-note");
                let msg = if all.iter().any(|l| matches!(l, Load::Loading)) {
                    t_.doc.nodes[n].classes.push("detail-mono".into());
                    "loading"
                } else if all.iter().any(|l| matches!(l, Load::Failed)) {
                    "This couldn't be loaded."
                } else {
                    "Nothing in your library features this person."
                };
                t_.doc.text(n, msg);
                t_.doc.add(0, "div.band-rule");
            }
        }
        if !full.info.is_empty() {
            self.info_nodes(&mut t_, &full);
        }
        let end = t_.doc.add(0, "div.page-end");
        t_.doc.inline(end, &format!("height: {}", px(t::S8 * 2.0)));
        t_
    }

    /// Loading state of the cells of a strip (a placeholder row shows while it is `Loading`).
    fn sec_region(&self, s: Sec) -> Region {
        let kind = self.media.kind;
        let (loading, failed, shown) = match s {
            Sec::Cast => (self.full.is_none(), false, 0),
            Sec::Similar => (
                matches!(self.similar, Load::Loading),
                matches!(self.similar, Load::Failed),
                0,
            ),
            Sec::Next if kind == Kind::Series => (
                matches!(self.nextup, Load::Loading),
                matches!(self.nextup, Load::Failed),
                0,
            ),
            Sec::Kids if kind == Kind::Person => (
                matches!(self.kids, Load::Loading),
                matches!(self.kids, Load::Failed),
                0,
            ),
            Sec::Kids2 if kind == Kind::Person => (
                matches!(self.kids2, Load::Loading),
                matches!(self.kids2, Load::Failed),
                0,
            ),
            _ => (false, false, 0),
        };
        Region::of(loading, failed, shown)
    }

    // ---- hero ----

    fn hero_nodes(&self, t_: &mut DTree) {
        let m = &self.media;
        let w = self.win.width;
        let hero = t_.doc.add(0, "section.detail.detail-hero");
        t_.doc.attr(hero, "data-kind", kind_attr(m.kind));
        let art = geom::hero_art(m);
        // Iced cannot blur an image: a theme's backdrop blur shows the item's blurhash stand-in.
        let (sharp, soft) = (self.img(m, art, 1920), self.blur(m, art));
        let sharp = if t::backdrop_blur() > 0.0 && soft.is_some() {
            None
        } else {
            sharp
        };
        let a = t_.doc.add(hero, "div.detail-art");
        t_.doc.add(a, "div.detail-fill");
        match (sharp, soft) {
            (Some(i), _) | (None, Some(i)) => {
                let i = t_.pic(a, "img.detail-backdrop", i);
                t_.doc
                    .inline(i, &format!("opacity: {}", t::backdrop_opacity()));
            }
            _ => {
                t_.doc.add(a, "div.detail-backdrop.detail-backdrop-empty");
            }
        }
        t_.doc.add(hero, "div.detail-scrim-side");
        t_.doc.add(hero, "div.detail-scrim-foot");
        let body = t_.doc.add(hero, "div.detail-body");
        let col = t_.doc.add(body, "div.detail-col");
        // Eyebrow and the source mark.
        let eyebrow = if m.kind == Kind::Episode {
            format!(
                "0x01 \u{b7} {} \u{b7} {}",
                m.series_name.clone().unwrap_or_default(),
                m.episode_label()
            )
        } else {
            format!("0x01 \u{b7} {}", m.kind_label())
        };
        let head = t_.doc.add(col, "div.detail-head");
        let e = t_.doc.add(head, "div.detail-eyebrow");
        t_.doc.text(e, &eyebrow);
        if self.marks.len() > 1
            && let Some(k) = self.mark_of(m.origin)
        {
            let mk = t_.doc.add(head, "div.detail-mark");
            let dot = t_.doc.add(mk, "div.detail-dot");
            t_.doc
                .inline(dot, &format!("background: {}", t::css_color(k.color)));
            let l = t_.doc.add(mk, "div.detail-mark-label");
            t_.doc.text(l, &k.label);
        }
        // The line of facts: kind, rating, year, runtime, certification; and the genres.
        let chips = t_.doc.add(col, "div.detail-chips");
        let meta = t_.doc.add(chips, "div.detail-meta");
        // The text twice: tracked capitals (the default look) and plain, for themes that set the facts as a line of text (the default sheet hides the plain one and the rating's star).
        let chip = |d: &mut Doc, parent: Id, class: &str, s: &str| {
            let c = d.add(parent, class);
            let l = d.add(c, "span.chip-label");
            d.text(l, &tracked(s));
            let (star, plain) = match s.strip_prefix('\u{2605}') {
                Some(r) => (true, r.trim_start()),
                None => (false, s),
            };
            if star {
                let st = d.add(c, "span.chip-star");
                d.text(st, "\u{2605}");
            }
            let mut cs = plain.chars();
            let cap: String = cs
                .next()
                .map(|f| f.to_uppercase().chain(cs).collect())
                .unwrap_or_default();
            let p = d.add(c, "span.chip-plain");
            d.text(p, &cap);
        };
        chip(
            &mut t_.doc,
            meta,
            "div.chip.accent.detail-kind",
            &m.kind_label(),
        );
        if let Some(r) = m.rating {
            chip(
                &mut t_.doc,
                meta,
                "div.chip.detail-rating",
                &format!("\u{2605} {r:.1}"),
            );
        }
        if let Some(y) = m.year {
            chip(&mut t_.doc, meta, "div.chip.detail-year", &y.to_string());
        }
        if let Some(r) = m.runtime_min {
            chip(
                &mut t_.doc,
                meta,
                "div.chip.detail-runtime",
                &fmt_runtime(r),
            );
        }
        if let Some(c) = self
            .full
            .as_ref()
            .and_then(|f| f.official_rating.as_deref())
        {
            chip(&mut t_.doc, meta, "div.chip.detail-cert", c);
        }
        let genres = t_.doc.add(chips, "div.detail-genres");
        for gname in m.genres.iter().take(3) {
            chip(&mut t_.doc, genres, "div.chip.detail-genre", gname);
        }
        // The parent's name above the title (the series of an episode); the eyebrow already says it, so the default sheet hides this line.
        if let Some(parent) = m.series_name.as_deref().filter(|_| m.kind != Kind::Series) {
            let n = t_.doc.add(col, "div.detail-parent");
            t_.doc.text(n, parent);
        }
        // The title: the logo art when the item has one, else text.
        let size = (w / 26.0).clamp(40.0, 72.0);
        let logo = m
            .logo
            .is_some()
            .then(|| self.img(m, Art::Logo, 800))
            .flatten();
        match (&m.logo, logo) {
            (_, Some(l)) => {
                t_.pic(col, "img.detail-logo", l);
            }
            (Some(_), None) if self.pics.contains_key(&pic_key(m, Art::Logo, 800)) => {
                self.title_node(t_, col, size)
            }
            (Some(_), None) => {
                t_.doc.add(col, "div.detail-logo-gap");
            }
            _ => self.title_node(t_, col, size),
        }
        if m.logo.is_some() && matches!(m.kind, Kind::Season | Kind::Episode) {
            let n = t_.doc.add(col, "div.detail-season");
            t_.doc.text(n, &m.title);
        }
        // The description: clamped to three lines, opened by the toggle.
        let sub = m.overview.clone().unwrap_or_else(|| m.meta_line());
        let key = self.expand_key();
        let open = self.expanded.is_open(&key);
        let (_, cut) = expand::clamp(&sub, cols_of(560.0f32.min(w), t::BODY_LG), 3);
        let n = t_.doc.add(col, "div.detail-text.detail-summary");
        t_.doc.text(n, &sub);
        t_.doc
            .attr(n, "data-expanded", if open { "true" } else { "false" });
        if cut {
            t_.acts.insert(n, Msg::Expand);
            let b = t_.doc.add(col, "a.detail-more");
            t_.doc
                .attr(b, "data-expanded", if open { "true" } else { "false" });
            t_.doc.text(b, if open { "Show less" } else { "Show more" });
            t_.doc.focusable(b);
            t_.acts.insert(b, Msg::Expand);
            t_.toggle = Some(b);
        }
        self.actions_nodes(t_, col);
        if let Some(e) = self.play_err {
            self.error_node(t_, col, e.message());
        } else if !m.available {
            self.error_node(t_, col, PlayError::Unavailable.message());
        }
    }

    fn error_node(&self, t_: &mut DTree, parent: Id, msg: &str) {
        let n = t_.doc.add(parent, "div.detail-error");
        t_.doc.text(n, msg);
        t_.doc
            .inline(n, &format!("color: {}", t::css_color(t::warn())));
    }

    fn title_node(&self, t_: &mut DTree, parent: Id, size: f32) {
        let p = t::parts(Some(nocturne_core::customize::Page::Detail), None);
        let ty = t::typo(&p.detail_title, size, iced::font::Weight::Bold).with_line(1.08);
        let n = t_.doc.add(parent, "div.detail-title");
        t_.doc.text(n, &self.media.title);
        t_.doc.inline(n, &ty.css());
    }

    fn actions_nodes(&self, t_: &mut DTree, parent: Id) {
        let m = &self.media;
        let list = self.versions();
        let row = t_.doc.add(parent, "div.detail-actions");
        for (i, a) in self.dacts().into_iter().enumerate() {
            let (label, primary) = model::spec(a, m);
            let label = if a == DAct::Versions {
                let v = list.get(self.picked(&list));
                // Quality and codec only: the full line is in the picker.
                let s = v.map_or(String::new(), |v| {
                    if v.detail.is_empty() {
                        self.mark_of(v.origin)
                            .map_or(String::new(), |k| k.label.clone())
                    } else {
                        v.detail
                            .split(" \u{b7} ")
                            .take(2)
                            .collect::<Vec<_>>()
                            .join(" \u{b7} ")
                    }
                });
                format!(
                    "{} \u{25BE}",
                    if s.is_empty() { label.to_string() } else { s }
                )
            } else {
                label.to_string()
            };
            let off = model::is_write(a) && !self.can_write(m);
            let mut class = String::from("a.button");
            if primary {
                class += ".button-primary";
            }
            if off {
                class += ".dim";
            }
            let b = t_.doc.add(row, &class);
            t_.doc.attr(b, "data-action", action_name(a));
            if off {
                t_.doc.attr(b, "title", NEED_WRITES);
            }
            if primary {
                let ic = t_.doc.add(b, "span.button-icon");
                t_.doc.text(ic, "\u{25B6}");
            }
            let l = t_.doc.add(b, "span.button-label");
            t_.doc.text(l, &label);
            t_.acts.insert(b, Msg::Act(a));
            t_.cell(b, (DRow::Actions, i));
        }
        if m.kind == Kind::Book {
            let n = t_.doc.add(row, "div.detail-note-inline");
            t_.doc.text(n, "Not playable in NOCTURNE yet.");
        }
        // The chosen version as a line of its own for themes that show the track block (hidden by default: the Versions button says it).
        if list.len() > 1
            && let Some(v) = list.get(self.picked(&list))
            && !v.detail.is_empty()
        {
            let tracks = t_.doc.add(parent, "div.detail-tracks");
            let n = t_.doc.add(tracks, "div.detail-track");
            t_.doc.attr(n, "data-track", "version");
            t_.doc.text(n, &v.detail);
        }
    }

    // ---- bands ----

    fn facts_nodes(&self, t_: &mut DTree, f: &Full) {
        let m = &self.media;
        let line = t_.doc.add(0, "div.detail-facts");
        let mono = |t_: &mut DTree, s: &str| {
            let n = t_.doc.add(line, "div.detail-mono");
            t_.doc.text(n, s);
        };
        if let Some(r) = m.rating {
            let (full, half, empty) = detail::stars(r);
            let st = t_.doc.add(line, "div.detail-stars");
            let n = t_.doc.add(st, "div.star-full");
            t_.doc.text(n, &"\u{2605}".repeat(full as usize));
            if half {
                let n = t_.doc.add(st, "div.star-half");
                t_.doc.text(n, "\u{2605}");
                t_.doc
                    .inline(n, "color: alpha(var(--nocturne-accent-text), 0.5)");
            }
            let n = t_.doc.add(st, "div.star-empty");
            t_.doc.text(n, &"\u{2606}".repeat(empty as usize));
            mono(t_, &format!("{r:.1}"));
        }
        if let Some(c) = f.critic {
            mono(t_, &format!("{c:.0}% critics"));
        }
        if let Some(r) = f.official_rating.as_deref() {
            let c = t_.doc.add(line, "div.chip");
            let l = t_.doc.add(c, "span.chip-label");
            t_.doc.text(l, &tracked(r));
        }
        if let Some(y) = m.year {
            mono(t_, &y.to_string());
        }
        if let Some(r) = m.runtime_min {
            mono(t_, &fmt_runtime(r));
            if matches!(m.kind, Kind::Movie | Kind::Episode | Kind::Video) {
                let left = m
                    .progress()
                    .map_or(r, |p| ((1.0 - p) * r as f32).round() as u32);
                mono(
                    t_,
                    &format!("ends {}", detail::ends_at(detail::local_minutes(), left)),
                );
            }
        }
        if m.kind == Kind::Series
            && let Some(n) = m.unplayed.filter(|n| *n > 0)
        {
            mono(t_, &format!("{n} unwatched"));
        }
        if let Some(n) = m
            .child_count
            .filter(|_| matches!(m.kind, Kind::BoxSet | Kind::Playlist | Kind::MusicAlbum))
        {
            mono(t_, &format!("{n} items"));
        }
    }

    fn tags_nodes(&self, t_: &mut DTree, chips: Vec<model::Chip>) {
        let row = t_.doc.add(0, "div.detail-tags");
        for (i, c) in chips.into_iter().enumerate() {
            let link = matches!(c, model::Chip::Link(..));
            let n = t_.doc.add(
                row,
                match (&c, link) {
                    (model::Chip::Genre(_), _) => "a.detail-tag.detail-tag-genre",
                    (model::Chip::Studio(..), _) => "a.detail-tag.detail-tag-studio",
                    (_, _) => "a.detail-tag.detail-tag-link",
                },
            );
            let l = t_.doc.add(n, "span.detail-tag-label");
            t_.doc.text(l, &c.label());
            t_.acts.insert(n, Msg::Chip(i));
            t_.cell(n, (DRow::Chips, i));
        }
    }

    fn about_nodes(&self, t_: &mut DTree, f: &Full) {
        let m = &self.media;
        let w = self.win.width;
        let h = geom::about_h(m, f, w);
        let band = t_.doc.add(0, "div.detail-about");
        // The hand-written page lays this band out to an estimate of its text height: never shorter, grows with the real text.
        t_.doc.inline(band, &format!("min-height: {}", px(h)));
        if geom::about_poster(m) {
            let p = t_.doc.add(band, "div.detail-poster");
            self.tile_nodes(
                t_,
                p,
                self.img(m, Art::Poster, 480),
                self.blur(m, Art::Poster),
                &m.title,
            );
            t_.doc
                .inline(p, &format!("border-radius: {}", px(t::r_xs())));
        }
        let left = t_.doc.add(band, "div.detail-about-col");
        if let Some(tag) = &f.tagline {
            let n = t_.doc.add(left, "div.detail-tagline");
            t_.doc.text(n, tag);
        }
        let overview = m
            .overview
            .clone()
            .unwrap_or_else(|| "No description available.".into());
        let n = t_.doc.add(left, "div.detail-text.detail-about-text");
        t_.doc.text(n, &overview);
        let ty = t::typo(
            &t::parts(Some(nocturne_core::customize::Page::Detail), None).detail_text,
            t::BODY_LG,
            iced::font::Weight::Normal,
        )
        .with_line(1.6);
        t_.doc.inline(n, &ty.css());
        let right = t_.doc.add(band, "div.detail-facts-table");
        for (k, v) in geom::facts(m, f) {
            let r = t_.doc.add(right, "div.detail-fact");
            let kk = t_.doc.add(r, "div.detail-fact-key");
            t_.doc.text(kk, &k.to_uppercase());
            let vv = t_.doc.add(r, "div.detail-fact-value");
            t_.doc.text(vv, &v);
        }
    }

    /// The picture tile: the image, else its blurhash, else the title's initial on the raised fill.
    fn tile_nodes(
        &self,
        t_: &mut DTree,
        parent: Id,
        img: Option<&Handle>,
        blur: Option<&Handle>,
        title: &str,
    ) {
        match (img, blur) {
            (Some(h), _) | (None, Some(h)) => {
                t_.pic(parent, "img.tile-img", h);
            }
            _ => {
                let i = initial(title);
                if !i.is_empty() {
                    let n = t_.doc.add(parent, "div.tile-initial");
                    let g = t_.doc.add(n, "div.tile-initial-glyph");
                    t_.doc.text(g, &i);
                }
            }
        }
    }

    // ---- sections ----

    fn section_head(
        &self,
        t_: &mut DTree,
        parent: Id,
        name: &str,
        title: &str,
        body: &str,
        meta: &str,
    ) -> Id {
        let head = t_.doc.add(parent, "div.section-head");
        let titles = t_.doc.add(head, "div.section-titles");
        let ti = t_.doc.add(titles, "div.section-title");
        t_.doc.attr(ti, "data-section", name);
        let tx = t_.doc.add(ti, "span.section-title-text");
        t_.doc.text(tx, title);
        if !body.is_empty() {
            let b = t_.doc.add(titles, "div.section-text");
            t_.doc.text(b, body);
        }
        let mt = t_.doc.add(head, "div.section-meta");
        t_.doc.text(mt, meta);
        head
    }

    fn list_nodes(&self, t_: &mut DTree) {
        let m = &self.media;
        let w = self.win.width;
        let (heading, sub) = match m.kind {
            Kind::Series => ("Episodes", "Pick a season, then an episode."),
            Kind::Season => ("Episodes", "Everything in this season."),
            Kind::MusicAlbum => ("Tracks", "In album order."),
            Kind::MusicArtist => ("Albums", "Everything by this artist."),
            Kind::Playlist => ("Playlist", "In the order it plays."),
            _ => ("In this collection", "Everything grouped here."),
        };
        let meta = if m.kind == Kind::Series {
            match self.seasons.items().len() {
                1 => "1 season".to_string(),
                n => format!("{n} seasons"),
            }
        } else {
            self.episodes.items().len().to_string()
        };
        let region = Region::of(
            matches!(self.episodes, Load::Loading),
            matches!(self.episodes, Load::Failed),
            self.episodes.items().len(),
        );
        let sec = t_.doc.add(0, "section.section.section-list");
        t_.doc.attr(sec, "data-section", "list");
        t_.doc.attr(sec, "data-region", region_name(region));
        let head = t_.doc.add(sec, "div.section-box");
        self.section_head(t_, head, "list", heading, sub, &meta);
        if m.kind == Kind::Series {
            let tabs_box = t_.doc.add(head, "div.detail-tabs-box");
            let tabs = t_.doc.add(tabs_box, "div.detail-tabs");
            for (i, s) in self.seasons.items().iter().enumerate() {
                let n = match s.unplayed {
                    Some(0) => " \u{2713}".to_string(),
                    Some(n) => format!("\u{a0}\u{a0}{n}"),
                    None => String::new(),
                };
                let tab = t_.doc.add(
                    tabs,
                    if i == self.focus.season {
                        "a.tab.active"
                    } else {
                        "a.tab"
                    },
                );
                let l = t_.doc.add(tab, "span.tab-label");
                t_.doc.text(l, &format!("{}{n}", s.title));
                t_.acts.insert(tab, Msg::SeasonTab(i));
                t_.cell(tab, (DRow::Seasons, i));
            }
            t_.doc.add(tabs_box, "div.band-rule");
        }
        t_.doc.add(sec, "div.band-rule");
        let msg = |t_: &mut DTree, text: &str, mono: bool| {
            let n = t_.doc.add(sec, "div.detail-note");
            if mono {
                t_.doc.nodes[n].classes.push("detail-mono".into());
            }
            t_.doc.text(n, text);
            t_.doc.add(sec, "div.band-rule");
        };
        match &self.episodes {
            Load::Loading => {
                msg(t_, "loading", true);
                self.skeleton_row(
                    t_,
                    sec,
                    0.0,
                    geom::card_w(w) + 24.0,
                    geom::card_h(w),
                    "episode",
                );
            }
            Load::Failed => msg(t_, "This couldn't be loaded.", false),
            Load::Ready(v) if v.is_empty() => msg(
                t_,
                if m.kind == Kind::Series {
                    "No episodes have been added yet."
                } else {
                    "Nothing in here yet."
                },
                false,
            ),
            Load::Ready(v) if model::card_grid(m.kind) => {
                let (cols, cw) = (geom::card_cols(w), geom::card_w(w));
                let now = detail::local_minutes();
                for (r, chunk) in v.chunks(cols).enumerate() {
                    let line = t_.doc.add(sec, "div.episode-row");
                    t_.doc
                        .inline(line, &format!("height: {}", px(geom::card_h(w))));
                    for (c, e) in chunk.iter().enumerate() {
                        self.episode_nodes(t_, line, e, r * cols + c, cw, now);
                    }
                }
            }
            Load::Ready(v) => {
                for (i, e) in v.iter().enumerate() {
                    self.row_nodes(t_, sec, e, i);
                    t_.doc.add(sec, "div.band-rule");
                }
            }
        }
    }

    /// 16:9 episode card: still with play overlay and progress, title, meta, overview, actions.
    fn episode_nodes(&self, t_: &mut DTree, parent: Id, e: &Media, idx: usize, w: f32, now: u32) {
        use nocturne_core::detail::ends_at;
        let h = (w * 9.0 / 16.0).round();
        let title = match e.number {
            Some(n) => format!("{n}. {}", e.title),
            None => e.title.clone(),
        };
        let mut meta = vec![];
        if let Some(r) = e.runtime_min {
            meta.push(fmt_runtime(r));
            let left = e
                .progress()
                .map_or(r, |p| ((1.0 - p) * r as f32).round() as u32);
            if !e.played {
                meta.push(format!("ends {}", ends_at(now, left)));
            }
        }
        if let Some(r) = e.rating {
            meta.insert(meta.len().min(1), format!("\u{2605} {r:.1}"));
        }
        let card = t_.doc.add(parent, "a.episode");
        t_.doc.attr(card, "data-kind", kind_attr(e.kind));
        t_.doc.attr(
            card,
            "data-watched",
            if e.played { "true" } else { "false" },
        );
        t_.doc.inline(
            card,
            &format!(
                "width: {}; height: {}",
                px(w + 24.0),
                px(h + 8.0 + geom::CARD_TEXT + 24.0)
            ),
        );
        let thumb = t_.doc.add(card, "div.episode-thumb");
        t_.doc
            .inline(thumb, &format!("width: {}; height: {}", px(w), px(h)));
        let img = self.img(e, Art::Landscape, geom::CARD_PX);
        let blur = self.blur(e, Art::Landscape);
        let tile = t_.doc.add(thumb, "div.tile");
        self.tile_nodes(t_, tile, img, blur, &e.title);
        let play = t_.doc.add(thumb, "div.episode-play");
        let g = t_.doc.add(play, "div.episode-glyph");
        t_.doc.text(g, "\u{25B6}");
        if e.played {
            let ck = t_.doc.add(thumb, "div.episode-check");
            let g = t_.doc.add(ck, "div.episode-check-glyph");
            t_.doc.text(g, "\u{2713}");
        }
        if let Some(p) = e.progress() {
            self.progress_node(t_, thumb, p);
        }
        let body = t_.doc.add(card, "div.episode-body");
        let text = t_.doc.add(body, "div.episode-text");
        let ti = t_.doc.add(text, "div.episode-title");
        t_.doc.text(ti, &title);
        let me = t_.doc.add(text, "div.episode-meta");
        t_.doc.text(me, &meta.join("   "));
        let ov = t_.doc.add(text, "div.episode-overview");
        t_.doc.text(ov, e.overview.as_deref().unwrap_or(""));
        let actions = t_.doc.add(body, "div.episode-actions");
        let writes = self.can_write(e);
        for (glyph, on, off, name, msg) in [
            ("i", false, false, "details", Msg::ListInfo(idx)),
            ("\u{2713}", e.played, !writes, "watched", Msg::ListSeen(idx)),
            (
                if e.favorite { "\u{2665}" } else { "\u{2661}" },
                e.favorite,
                !writes,
                "favourite",
                Msg::ListFav(idx),
            ),
        ] {
            let mut class = String::from("a.round");
            if on {
                class += ".on";
            }
            if off {
                class += ".dim";
            }
            let b = t_.doc.add(actions, &class);
            t_.doc.attr(b, "data-action", name);
            let gl = t_.doc.add(b, "span.round-glyph");
            t_.doc.text(gl, glyph);
            t_.acts.insert(b, msg);
        }
        t_.acts.insert(card, Msg::ListOpen(idx));
        t_.hover.insert(card, Msg::Hover(DRow::List, idx));
        t_.cell(card, (DRow::List, idx));
    }

    /// Row of a track / playlist entry / album: number, still, title, line of facts, overview.
    fn row_nodes(&self, t_: &mut DTree, parent: Id, e: &Media, idx: usize) {
        let kind = self.media.kind;
        let num = match (e.kind, e.number) {
            (Kind::Audio | Kind::Episode, Some(n)) => format!("{n:>2}"),
            _ => format!("0x{:02x}", idx + 1),
        };
        let mut sub = vec![];
        if kind != Kind::Season && e.kind == Kind::Episode {
            sub.push(e.episode_label());
        }
        if let Some(a) = e.sub.clone().filter(|_| e.kind == Kind::Audio) {
            sub.push(a);
        }
        sub.extend(e.runtime_min.map(fmt_runtime));
        if e.played {
            sub.push("watched \u{2713}".into());
        }
        sub.retain(|s| !s.is_empty());
        let row = t_.doc.add(parent, "a.entry");
        t_.doc.attr(row, "data-kind", kind_attr(e.kind));
        t_.doc
            .attr(row, "data-watched", if e.played { "true" } else { "false" });
        let n = t_.doc.add(row, "div.entry-num");
        t_.doc.text(n, &num);
        let still = t_.doc.add(row, "div.entry-still");
        let tile = t_.doc.add(still, "div.tile");
        self.tile_nodes(
            t_,
            tile,
            self.img(e, Art::Poster, 320),
            self.blur(e, Art::Poster),
            &e.title,
        );
        if let Some(p) = e.progress() {
            self.progress_node(t_, still, p);
        }
        let info = t_.doc.add(row, "div.entry-info");
        let ti = t_.doc.add(info, "div.entry-title");
        t_.doc.text(ti, &e.title);
        let me = t_.doc.add(info, "div.entry-meta");
        t_.doc.text(me, &sub.join(" \u{b7} "));
        if let Some(o) = e.overview.clone().filter(|_| e.kind == Kind::Episode) {
            let ov = t_.doc.add(info, "div.entry-overview");
            t_.doc.text(ov, &o);
        }
        t_.acts.insert(row, Msg::ListOpen(idx));
        t_.hover.insert(row, Msg::Hover(DRow::List, idx));
        t_.cell(row, (DRow::List, idx));
    }

    fn progress_node(&self, t_: &mut DTree, parent: Id, p: f32) {
        let b = t_.doc.add(parent, "div.progress");
        let f = t_.doc.add(b, "div.progress-fill");
        t_.doc.inline(
            f,
            &format!("width: {}%", (p.clamp(0.0, 1.0) * 1000.0).round() / 10.0),
        );
    }

    /// A strip of poster or still cells (cast, chapters, similar, seasons...).
    fn strip_nodes(&self, t_: &mut DTree, s: Sec, items: &[Media]) {
        let (title, body) = s.title(self.media.kind);
        let (cw, ch) = geom::dims(s);
        let (art, pxw) = geom::strip_art(s);
        let sec = t_.doc.add(0, "section.section");
        t_.doc.nodes[sec]
            .classes
            .push(format!("section-{}", sec_name(s)));
        t_.doc.attr(sec, "data-section", sec_name(s));
        t_.doc.attr(sec, "data-region", "ready");
        let boxed = t_.doc.add(sec, "div.section-box");
        self.section_head(
            t_,
            boxed,
            sec_name(s),
            title,
            body,
            &items.len().to_string(),
        );
        // The hand-written page lays a strip out to a fixed block height (`geom::strip_block`).
        t_.doc.inline(
            boxed,
            &format!("height: {}", px(geom::strip_block(s) - 1.0)),
        );
        let strip = t_.doc.add(boxed, "div.section-strip");
        t_.doc
            .inline(strip, &format!("height: {}", px(geom::strip_h(s))));
        for (i, m) in items.iter().enumerate() {
            let wrap = t_.doc.add(strip, "div.poster-wrap");
            let p = t_.doc.add(wrap, "a.poster");
            t_.doc.attr(p, "data-kind", kind_attr(m.kind));
            t_.doc.attr(
                p,
                "data-shape",
                if ch > cw { "portrait" } else { "backdrop" },
            );
            t_.doc
                .attr(p, "data-watched", if m.played { "true" } else { "false" });
            t_.doc.inline(p, &format!("width: {}", px(cw + 24.0)));
            let a = t_.doc.add(p, "div.poster-art");
            t_.doc
                .inline(a, &format!("width: {}; height: {}", px(cw), px(ch)));
            let tile = t_.doc.add(a, "div.tile");
            self.tile_nodes(t_, tile, self.img(m, art, pxw), self.blur(m, art), &m.title);
            t_.doc.add(a, "div.poster-dim");
            let ix = t_.doc.add(a, "div.poster-index");
            t_.doc.text(ix, &format!("0x{:02x}", i + 1));
            if let Some(pr) = m.progress() {
                self.progress_node(t_, a, pr);
            }
            let cap = t_.doc.add(p, "div.poster-caption");
            t_.doc.inline(cap, &format!("width: {}", px(cw)));
            let ti = t_.doc.add(cap, "div.poster-title");
            t_.doc.text(ti, &m.title);
            let me = t_.doc.add(cap, "div.poster-meta");
            t_.doc.text(me, &super::view::caption(m));
            t_.doc.add(wrap, "div.poster-rule");
            t_.acts.insert(p, Msg::Sec(s, i));
            t_.hover.insert(p, Msg::Hover(DRow::Strip(s), i));
            t_.cell(p, (DRow::Strip(s), i));
        }
        t_.doc.add(sec, "div.band-rule");
    }

    /// Placeholder cells while a strip loads.
    fn skeleton_nodes(&self, t_: &mut DTree, s: Sec) {
        let (title, body) = s.title(self.media.kind);
        let (cw, _) = geom::dims(s);
        let sec = t_.doc.add(0, "section.section.section-skeleton");
        t_.doc.attr(sec, "data-section", sec_name(s));
        t_.doc.attr(sec, "data-region", "loading");
        let boxed = t_.doc.add(sec, "div.section-box");
        self.section_head(t_, boxed, sec_name(s), title, body, "");
        self.skeleton_row(
            t_,
            boxed,
            t::page_pad(),
            cw + 25.0,
            geom::strip_h(s),
            "poster",
        );
        t_.doc.add(sec, "div.band-rule");
    }

    fn skeleton_row(&self, t_: &mut DTree, parent: Id, pad: f32, cell: f32, h: f32, kind: &str) {
        let n = region::skeleton_cells(self.win.width - 2.0 * pad, cell, 0.0, 8);
        let row = t_.doc.add(parent, "div.skeleton-row");
        t_.doc.inline(row, &format!("height: {}", px(h)));
        for _ in 0..n {
            let c = t_.doc.add(row, &format!("div.skeleton.skeleton-{kind}"));
            t_.doc.inline(c, &format!("width: {}", px(cell - 24.0)));
        }
    }

    fn info_nodes(&self, t_: &mut DTree, f: &Full) {
        let info = t_.doc.add(0, "div.detail-info");
        let l = t_.doc.add(info, "div.detail-info-label");
        t_.doc.text(l, &tracked("Media info"));
        for b in &f.info {
            let head = [b.container.as_str(), b.size.as_str(), b.bitrate.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" \u{b7} ");
            let tbl = t_.doc.add(info, "div.detail-info-table");
            let h = t_.doc.add(tbl, "div.detail-info-head");
            t_.doc.text(h, &head);
            for s in &b.streams {
                t_.doc.add(tbl, "div.band-rule");
                let r = t_.doc.add(tbl, "div.detail-info-row");
                for (class, text) in [
                    ("detail-info-kind", s.kind.to_uppercase()),
                    ("detail-info-codec", s.codec.clone()),
                    ("detail-info-title", s.title.clone()),
                    ("detail-info-detail", s.detail.clone()),
                    ("detail-info-bitrate", s.bitrate.clone()),
                ] {
                    let c = t_.doc.add(r, &format!("div.{class}"));
                    t_.doc.text(c, &text);
                }
            }
        }
        t_.doc.add(0, "div.band-rule");
    }
}

fn action_name(a: DAct) -> &'static str {
    match a {
        DAct::Play => "play",
        DAct::PlayAll => "play-all",
        DAct::Shuffle => "shuffle",
        DAct::Watched => "watched",
        DAct::Fav => "favourite",
        DAct::Prev => "previous",
        DAct::Next => "next",
        DAct::Series => "series",
        DAct::More => "more",
        DAct::View => "view",
        DAct::Versions => "versions",
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
