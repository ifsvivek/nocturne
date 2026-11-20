//! The theme engine inside the app (feature `theme-engine`): Home, the shared header, the library page and the search overlay are element trees (`home_tree.rs`, `header_tree.rs`, `browse_tree.rs`, `search_tree.rs`) styled by the default sheet and laid out by `nocturne-style`, painted by `widget/styled.rs`. This file owns the engine, the scenes and their interaction state (hover, focus, scroll), builds a scene when the data changes (never per frame, never per arriving picture), maps pointer and keyboard events to the same messages the hand-written screens send, and chooses what to show: the engine when the feature is on and the theme only uses what the builders reproduce, the hand-written page otherwise (also with `NOCTURNE_ENGINE_UI=0`).
use super::header_tree::{Slot, Tree, dims};
use super::home::HRow;
use super::message::{At, CellAct, Message, Which};
use super::{App, Phase, View};
use crate::theme as t;
use crate::widget::cell::{Look, Pic};
use crate::widget::styled::{Nav, Styled, StyledEvent};
use crate::widget::styled_preview::shape;
use iced::widget::image::Handle;
use iced::widget::{column, container, space};
use iced::{Element, Length, Task};
use nocturne_core::customize::{self as cz, Page};
use nocturne_style::{Dir, Engine, Id, Interaction, MeasureCache, Scene};
use std::time::Instant;

/// Page scroll (px) past which `data-scrolled` is set on Home's root.
const SCROLLED: f32 = 20.0;

const DEFAULT_CSS: &str = include_str!("../../../style/themes/default.css");

/// One scene with the tree it was built from and its interaction state.
pub struct Hosted {
    pub scene: Scene,
    pub tree: Tree,
    pub it: Interaction,
    /// The hero item of the build: the scene is rebuilt when the focus makes it change.
    hero: Option<String>,
    /// The sheet generation the scene was built with (a new theme or safe mode means a new scene).
    sheet_gen: u32,
    /// The library page: which list it shows (page id, epoch) and the selection already applied to the scene.
    page: (u64, u64),
    sel: usize,
}

impl Hosted {
    fn new(scene: Scene, tree: Tree, it: Interaction, sheet_gen: u32) -> Self {
        Self {
            scene,
            tree,
            it,
            hero: None,
            sheet_gen,
            page: (0, 0),
            sel: 0,
        }
    }

    /// A decoded picture arrived under cache key `key`: set its handle and size on the node that waits for it. No rebuild, no relayout (a picture's box never depends on its pixels).
    fn image_arrived(&mut self, key: &str, h: &Handle) {
        let mut ids: Vec<u32> = vec![];
        for s in &self.tree.pics {
            if s.key == key || (s.blur.as_deref() == Some(key) && !self.tree.done.contains(&s.id)) {
                ids.push(s.id);
            }
        }
        for &id in &ids {
            if self.tree.pics.iter().any(|s| s.id == id && s.key == key) {
                self.tree.done.insert(id);
            }
        }
        if let Some(g) = &self.tree.grid {
            let mut d = g.data.borrow_mut();
            if let Some(idxs) = d.waiting.get(key).cloned() {
                let mut real = false;
                for idx in idxs {
                    let Some(Some(c)) = d.cards.get(idx) else {
                        continue;
                    };
                    let id = c.pic_id;
                    if c.key.as_ref().is_some_and(|k| k.0 == key) {
                        real = true;
                        self.tree.done.insert(id);
                        d.set_pic(idx, Pic::Ready(h.clone()));
                        ids.push(id);
                    } else if !self.tree.done.contains(&id) {
                        d.set_pic(idx, Pic::Blur(h.clone()));
                        ids.push(id);
                    }
                }
                if real {
                    d.waiting.remove(key);
                }
            }
        }
        if ids.is_empty() {
            return;
        }
        let (w, ht) = dims(h);
        for &id in &ids {
            self.tree.images.insert(id, h.clone());
        }
        for n in &mut self.scene.doc.nodes {
            if let Some(img) = &mut n.image
                && ids.contains(&img.id)
            {
                (img.w, img.h) = (w, ht);
            }
        }
    }
}

pub struct EngineUi {
    pub enabled: bool,
    engine: Engine,
    css: String,
    /// Families of fetched theme fonts registered with the engine.
    fetched_fams: Vec<String>,
    /// Servers whose web-client icon font is loaded in the text shaper.
    pub(super) icons_ready: Vec<nocturne_core::source::SourceId>,
    user: Option<String>,
    cache: MeasureCache,
    pub home: Option<Hosted>,
    pub hdr: Option<Hosted>,
    /// The body of the library page, and the search overlay.
    pub page: Option<Hosted>,
    pub search: Option<Hosted>,
    /// Bumped whenever the sheet or safe mode changes: older scenes are stale.
    sheet_gen: u32,
    /// Home's scroll while another page is in view.
    saved: Option<nocturne_style::layout::Scroll>,
    pub dirty: bool,
    pub stats: bool,
    warmed: bool,
    /// The installed font families (read once) and whether the current engine knows them.
    families: Option<Vec<String>>,
    fonts_set: bool,
    /// Jellyfin layout mode: what the user layer was last built from (the base sheet's address and the branding and theme text).
    jf_key: Option<(usize, String)>,
}

impl EngineUi {
    /// The scene an event belongs to, and the shared measure cache.
    fn host(&mut self, w: Which) -> (Option<&mut Hosted>, &mut MeasureCache) {
        let Self {
            home,
            hdr,
            page,
            search,
            cache,
            ..
        } = self;
        let h = match w {
            Which::Home => home,
            Which::Header => hdr,
            Which::Page => page,
            Which::Search => search,
            Which::Detail | Which::Bar => return (None, cache),
        };
        (h.as_mut(), cache)
    }
}

impl Default for EngineUi {
    fn default() -> Self {
        Self {
            enabled: std::env::var("NOCTURNE_ENGINE_UI").map_or(true, |v| v != "0"),
            engine: Engine::new(DEFAULT_CSS),
            css: String::new(),
            fetched_fams: vec![],
            icons_ready: vec![],
            user: None,
            cache: MeasureCache::new(),
            home: None,
            hdr: None,
            page: None,
            search: None,
            sheet_gen: 0,
            saved: None,
            dirty: true,
            stats: std::env::var("NOCTURNE_ENGINE_STATS").is_ok(),
            warmed: false,
            families: None,
            fonts_set: false,
            jf_key: None,
        }
    }
}

/// The `:root` tokens of the active theme, and the translator's rules (hover scale and transitions of the strips and buttons) as CSS, in front of the default sheet.
pub(crate) fn sheet_css() -> String {
    let k = t::active();
    let c = t::css_color;
    let mut s = String::from(":root {");
    for (name, v) in [
        ("frame", k.frame),
        ("surface", k.surface),
        ("raised", k.raised),
        ("header-bg", k.header_bg),
        ("text", k.text),
        ("text-secondary", k.text_secondary),
        ("text-tertiary", k.text_tertiary),
        ("accent", k.accent),
        ("accent-hover", k.accent_hover),
        ("accent-text", k.accent_text),
        ("accent-tint", k.accent_tint),
        ("on-accent", k.on_accent),
        ("edge", k.edge),
        ("hairline", k.hairline),
        ("hairline-strong", k.hairline_strong),
    ] {
        s += &format!(" --nocturne-{name}: {};", c(v));
    }
    // The colour of scrims and washes over pictures: the built-in frame whatever stage-1 theme is active (as the hand-written pages draw them); a Jellyfin theme's palette mirrors its background onto it.
    s += &format!(" --nocturne-ink: {};", c(t::FRAME));
    s += &format!(
        " --nocturne-r-card: {}px; --nocturne-r-panel: {}px; --nocturne-page-pad: {}px; }}\n",
        k.r_card,
        t::r_panel(),
        t::page_pad()
    );
    s += DEFAULT_CSS;
    for (class, sec) in [
        ("section-resume", cz::Section::Resume),
        ("section-next-up", cz::Section::NextUp),
        ("section-latest", cz::Section::Latest),
    ] {
        let look = Look::of(&t::parts(Some(Page::Home), Some(sec)));
        if let Some(h) = look.hover {
            s += &format!(
                ".{class} .card:hover {{ transform: scale({h}); z-index: 5; }} .{class} .card {{ transition: transform {}ms ease-out; }}\n",
                look.ms.unwrap_or(140)
            );
        }
    }
    // The library grid's cards zoom like Home's.
    let look = Look::of(&t::parts(Some(Page::Library), None));
    if let Some(h) = look.hover {
        s += &format!(
            ".page-library .card:hover {{ transform: scale({h}); z-index: 5; }} .page-library .card {{ transition: transform {}ms ease-out; }}\n",
            look.ms.unwrap_or(140)
        );
    }
    // Focus glow and ring width follow the theme's glow colour and couch mode (the hand-written cells' `glow_shadow` and `couch::ring`).
    let glow = |a: f32| c(iced::Color { a, ..k.glow });
    let couch = crate::couch::on();
    s += &format!(
        ".button:focus {{ box-shadow: 0 0 24px {}; }} .card-ring {{ border-width: {}px; box-shadow: 0 0 24px {}; }}\n",
        glow(0.7),
        crate::couch::ring(),
        glow(if couch { 1.0 } else { 0.5 })
    );
    let (dur, scale) = t::button();
    s += &format!(
        ".button {{ transition: transform {}ms ease-out; }}\n",
        dur.as_millis()
    );
    if let Some(h) = scale {
        s += &format!(".button:hover {{ transform: scale({h}); }}\n");
    }
    s
}

impl App {
    /// Is Home on the engine right now (the feature is on, the theme is supported and a scene exists)?
    pub(super) fn eng_home_active(&self) -> bool {
        self.eng.enabled
            && matches!(self.phase, Phase::Ready)
            && self.eng_overlay_free()
            && self.stack.len() == 1
            && self.stack.last().is_some_and(|p| p.view == View::Home)
            && self.eng.home.is_some()
    }

    /// Is the library page's body on the engine right now?
    pub(super) fn eng_page_active(&self) -> bool {
        self.eng.enabled
            && matches!(self.phase, Phase::Ready)
            && self.eng_overlay_free()
            && self.stack.last().is_some_and(|p| p.view == View::Grid)
            && self.eng.page.is_some()
    }

    /// No full-screen page (settings, themes, sources...) covers the main view.
    fn eng_overlay_free(&self) -> bool {
        self.folders.is_none()
            && !self.th.open
            && !self.src_open
            && !self.settings_open
            && self.adm.is_none()
            && self.player.is_none()
            && self.ob.is_none()
    }

    /// The height of the engine's header scene: the layout mode's own, NOCTURNE's otherwise.
    pub(super) fn eng_header_h(&self) -> f32 {
        if self.jf_base().is_some() {
            self.jf_header_h()
        } else {
            t::header_h()
        }
    }

    /// The header runs on the engine whenever the page below can be drawn with it (every page but Home has its own header scene).
    pub(super) fn eng_header_active(&self) -> bool {
        self.eng.enabled
            && matches!(self.phase, Phase::Ready)
            && self.eng.hdr.is_some()
            && !self.eng_home_active()
    }

    /// Rebuild the scenes if something they show changed. Called at the end of every update; the task fetches what a new view needs (pages and pictures of the library grid).
    pub(super) fn eng_sync(&mut self, msg_dirties: bool) -> Task<Message> {
        if msg_dirties {
            self.eng.dirty = true;
        }
        if !self.eng.enabled || !self.eng.dirty || !matches!(self.phase, Phase::Ready) {
            return Task::none();
        }
        self.eng.dirty = false;
        let t0 = Instant::now();
        // Jellyfin layout mode: the engine's base layer is the browser's defaults, the server's stylesheets go in the user layer.
        let jf = self.jf_base();
        crate::detail::JF_MODE.store(jf.is_some(), std::sync::atomic::Ordering::Relaxed);
        let icons = self
            .theme_ui()
            .is_some_and(|u| self.eng.icons_ready.contains(&u));
        crate::detail::JF_ICONS.store(icons && jf.is_some(), std::sync::atomic::Ordering::Relaxed);
        let css = if jf.is_some() {
            super::jf::UA.to_string()
        } else {
            sheet_css()
        };
        let css_us = t0.elapsed().as_micros();
        if css != self.eng.css {
            let safe = self.eng.engine.safe_mode();
            self.eng.engine = Engine::new(&css);
            self.eng.fonts_set = false;
            self.eng.engine.set_safe_mode(safe);
            // The jellyfin-web selectors are native in the layout mode: no translation.
            self.eng.engine.set_compat(jf.is_none());
            self.eng.user = None;
            self.eng.jf_key = None;
            self.eng.fetched_fams.clear();
            self.eng.css = css;
            self.eng.sheet_gen += 1;
        }
        if self.eng.engine.safe_mode() != self.th.safe {
            self.eng.sheet_gen += 1;
        }
        self.eng.engine.set_safe_mode(self.th.safe);
        if !self.eng.fonts_set {
            // Font families the system has, so that `font-family: Name` finds them (and the Studio can list them).
            let fams = self
                .eng
                .families
                .get_or_insert_with(installed_families)
                .clone();
            self.eng.engine.fonts_mut().set_installed(fams);
            self.eng.fonts_set = true;
        }
        // The active theme's own rules, layered over the default sheet; the Theme Studio's working copy while it is open.
        #[cfg(feature = "theme-engine")]
        let studio_css = self.studio.open.then(|| self.studio.css.clone());
        #[cfg(not(feature = "theme-engine"))]
        let studio_css: Option<String> = None;
        let user = match studio_css {
            Some(raw) => Some(
                self.themes
                    .resolved_for(&raw)
                    .map_or(raw, |r| r.css.clone()),
            ),
            None => self.engine_css(),
        };
        let fams = self.engine_fonts();
        if fams != self.eng.fetched_fams {
            for f in &self.eng.fetched_fams {
                self.eng.engine.fonts_mut().unregister(f);
            }
            for f in &fams {
                self.eng.engine.fonts_mut().register(f);
            }
            self.eng.fetched_fams = fams;
            self.eng.sheet_gen += 1;
        }
        if let Some(b) = &jf {
            let key = (std::sync::Arc::as_ptr(b) as usize, self.jf_user_text());
            if self.eng.jf_key.as_ref() != Some(&key) {
                let text = format!("{b}\n{}", key.1);
                self.eng.engine.set_user_css(Some(&text));
                self.eng.jf_key = Some(key);
                self.eng.sheet_gen += 1;
            }
        } else if user != self.eng.user {
            self.eng.engine.set_user_css(user.as_deref());
            self.eng.user = user;
            // Scenes built with the old rules are stale (a header that "did not change" must still be restyled).
            self.eng.sheet_gen += 1;
        }
        if !self.eng.warmed {
            // The first measurement starts the font system (about 18 ms in release): do it before the first scene is timed.
            let st = nocturne_style::TextStyle {
                size: 15.0,
                weight: 400,
                italic: false,
                font: nocturne_style::FontChoice::Default,
                line_height: 1.3,
                lines: None,
                letter_spacing: 0.0,
                ellipsis: false,
            };
            let _ = shape("Warm", &st, None);
            self.eng.warmed = true;
        }
        let at_home =
            self.stack.len() == 1 && self.stack.last().is_some_and(|p| p.view == View::Home);
        if at_home && self.eng_overlay_free() {
            self.eng_build_home();
        } else if let Some(h) = self.eng.home.take() {
            // Another page is in view: Home comes back where it was left.
            self.eng.saved = Some(h.scene.scroll);
        }
        if self.eng_overlay_free() && !(at_home && self.eng.home.is_some()) {
            self.eng_build_header();
        } else {
            self.eng.hdr = None;
        }
        let t1 = Instant::now();
        let on_grid = self.stack.last().is_some_and(|p| p.view == View::Grid);
        let task = if on_grid && self.eng_overlay_free() {
            self.eng_build_page()
        } else {
            self.eng.page = None;
            Task::none()
        };
        if self.search.is_some() {
            self.eng_build_search();
        } else {
            self.eng.search = None;
        }
        if self.eng.stats {
            eprintln!(
                "engine: sync: sheet css {css_us} us, pages and search {} us, all {} us",
                t1.elapsed().as_micros(),
                t0.elapsed().as_micros()
            );
        }
        self.studio_refresh();
        task
    }

    /// Edit the Home scene in place: `edit` changes nodes of the document and returns which, then the scene restyles from the new state, so a transition on them runs (a rebuild would not).
    pub(super) fn eng_home_edit(
        &mut self,
        edit: impl FnOnce(&mut nocturne_style::Doc, &Tree) -> Vec<nocturne_style::Id>,
    ) {
        let cache = &mut self.eng.cache;
        let Some(h) = self.eng.home.as_mut() else {
            return;
        };
        let ids = edit(&mut h.scene.doc, &h.tree);
        if ids.is_empty() {
            return;
        }
        h.scene.touch(&ids);
        let _ = h.scene.refresh(Instant::now(), &mut |t, st, m| {
            cache.measure(t, st, m, &mut shape)
        });
    }

    fn eng_build_home(&mut self) {
        let tree = if self.jf_base().is_some() {
            self.jf_home_tree()
        } else {
            self.home_tree(true)
        };
        let Some(mut tree) = tree else {
            self.eng.home = None;
            return;
        };
        let (row, col) = self
            .stack
            .last()
            .and_then(|p| p.home.as_deref())
            .map_or((1, 0), |h| (h.row, h.col));
        let hero = self
            .stack
            .last()
            .and_then(|p| p.home.as_deref())
            .and_then(|h| h.hero().map(|m| m.id.clone()));
        let it = Interaction {
            focus: tree.by_cell.get(&(row, col)).copied(),
            // The default sheet draws its own focus styles (`.card:focus`, `.button:focus`); the engine's mandatory ring is for themes that remove them.
            focus_visible: false,
            ..Interaction::default()
        };
        let keep = self
            .eng
            .home
            .take()
            .map(|h| h.scene.scroll)
            .or_else(|| self.eng.saved.take());
        // A theme can draw a scrim behind the header once the page has scrolled (`page[data-scrolled="true"]`).
        tree.doc.attr(
            0,
            "data-scrolled",
            if keep.as_ref().is_some_and(|k| k.page_y > SCROLLED) {
                "true"
            } else {
                "false"
            },
        );
        let mut s = self.eng_scene(&tree, it, (self.eng_width(), self.win.height));
        if let Some(scroll) = keep {
            s.scroll = scroll;
            s.clamp_scroll();
        }
        let mut h = Hosted::new(s, tree, it, self.eng.sheet_gen);
        h.hero = hero;
        self.eng.home = Some(h);
        // A slide that is leaving fades out from here on.
        self.hero_settle();
    }

    fn eng_build_header(&mut self) {
        let jf = self.jf_base().is_some();
        let tree = if jf {
            self.jf_header_tree()
        } else {
            let mut tree = Tree::new();
            self.header_nodes(&mut tree, 0, false);
            tree
        };
        let size = (self.eng_width(), self.eng_header_h());
        // The same header (nothing it shows changed) keeps its scene, hover and animations.
        if self.eng.hdr.as_ref().is_some_and(|h| {
            h.sheet_gen == self.eng.sheet_gen
                && h.scene.viewport == size
                && h.tree.slots == tree.slots
                && h.scene.same_tree(&tree.doc)
        }) {
            return;
        }
        let it = Interaction::default();
        let s = self.eng_scene(&tree, it, size);
        self.eng.hdr = Some(Hosted::new(s, tree, it, self.eng.sheet_gen));
    }

    /// The library page: a new scene when what it shows changed, the same one (with its scroll, hover and animations) when only things happened that the page does not show.
    fn eng_build_page(&mut self) -> Task<Message> {
        let t0 = Instant::now();
        let task = self.eng_build_page_inner();
        if self.eng.stats {
            eprintln!("engine: page sync {} us", t0.elapsed().as_micros());
        }
        task
    }

    fn eng_build_page_inner(&mut self) -> Task<Message> {
        let old_data = self
            .eng
            .page
            .as_ref()
            .and_then(|h| h.tree.grid.as_ref().map(|g| g.data.clone()));
        let tree = if self.jf_base().is_some() {
            self.jf_browse_tree(old_data.as_ref())
        } else {
            self.browse_tree(old_data.as_ref())
        };
        let Some(tree) = tree else {
            self.eng.page = None;
            return Task::none();
        };
        let Some(p) = self.stack.last() else {
            return Task::none();
        };
        let br = p.br.as_deref();
        let key = (p.pid, br.map_or(0, |b| b.epoch));
        let (sel, off) = (p.sel, br.map_or(0.0, |b| b.off));
        let size = (self.eng_width(), self.win.height);
        let same_data = match (&old_data, &tree.grid) {
            (Some(a), Some(b)) => std::rc::Rc::ptr_eq(a, &b.data),
            (None, None) => true,
            _ => false,
        };
        let unchanged = self.eng.page.as_ref().is_some_and(|h| {
            h.sheet_gen == self.eng.sheet_gen
                && h.page == key
                && h.scene.viewport == size
                && same_data
                && h.tree.slots == tree.slots
                && h.scene.same_tree(&tree.doc)
        });
        if self.eng.stats && !unchanged {
            eprintln!(
                "engine: page rebuilt (gen {}, same list {}, size {}, data {same_data}, slots {}, tree {})",
                self.eng
                    .page
                    .as_ref()
                    .is_none_or(|h| h.sheet_gen == self.eng.sheet_gen),
                self.eng.page.as_ref().is_some_and(|h| h.page == key),
                self.eng
                    .page
                    .as_ref()
                    .is_some_and(|h| h.scene.viewport == size),
                self.eng
                    .page
                    .as_ref()
                    .is_some_and(|h| h.tree.slots == tree.slots),
                self.eng
                    .page
                    .as_ref()
                    .is_some_and(|h| h.scene.same_tree(&tree.doc)),
            );
        }
        if !unchanged {
            // The same list keeps its place; a page that was left and comes back starts where the page last reported.
            let y = self
                .eng
                .page
                .as_ref()
                .filter(|h| h.page == key)
                .and_then(|h| {
                    let g = h.tree.grid.as_ref()?;
                    h.scene.scroll.offsets.get(&g.node).map(|o| o[1])
                })
                .unwrap_or(off);
            let mut s = self.eng_scene(&tree, Interaction::default(), size);
            if let Some(g) = &tree.grid
                && y > 0.0
            {
                s.scroll.offsets.insert(g.node, [0.0, y]);
                s.clamp_scroll();
                let cache = &mut self.eng.cache;
                s.set_page_scroll(0.0, &mut |t, st, m| cache.measure(t, st, m, &mut shape));
            }
            // The selection the old scene had applied: if the page moved it since, `eng_page_follow` scrolls the new scene there.
            let applied = self
                .eng
                .page
                .as_ref()
                .filter(|h| h.page == key)
                .map_or(sel, |h| h.sel);
            let mut h = Hosted::new(s, tree, Interaction::default(), self.eng.sheet_gen);
            h.page = key;
            h.sel = applied;
            self.eng.page = Some(h);
        }
        self.eng_page_follow()
    }

    /// Bring the scene in step with the page's selection (a key moved it) and report the grid's scroll to the page so that the rows in view get their pages and pictures.
    fn eng_page_follow(&mut self) -> Task<Message> {
        let Some(sel) = self.stack.last().map(|p| p.sel) else {
            return Task::none();
        };
        let cache = &mut self.eng.cache;
        let Some(h) = self.eng.page.as_mut() else {
            return Task::none();
        };
        let node = h.tree.grid.as_ref().map(|g| g.node);
        if h.sel != sel {
            h.sel = sel;
            if let Some(c) = node {
                h.scene
                    .scroll_to_item(c, sel, &mut |t, st, m| cache.measure(t, st, m, &mut shape));
            }
        }
        h.it = h.scene.interaction;
        let want = h.scene.item_node(sel);
        if h.it.focus != want {
            h.it.focus = want;
            let it = h.it;
            let _ = h.scene.update(it, Instant::now(), &mut |t, st, m| {
                cache.measure(t, st, m, &mut shape)
            });
        }
        self.eng_page_scrolled()
    }

    /// The grid's scroll offset and height go to the page (they decide which rows are fetched); returns the fetch.
    fn eng_page_scrolled(&mut self) -> Task<Message> {
        let Some(h) = self.eng.page.as_ref() else {
            return Task::none();
        };
        let Some(g) = h.tree.grid.as_ref() else {
            return Task::none();
        };
        let off = h.scene.scroll.offsets.get(&g.node).map_or(0.0, |o| o[1]);
        let vh = h.scene.rects[g.node][3];
        if self.eng.stats {
            eprintln!(
                "engine: grid scroll {off} of a {vh} px view at y {}",
                h.scene.rects[g.node][1]
            );
        }
        if let Some(br) = self.stack.last_mut().and_then(|p| p.br.as_deref_mut()) {
            (br.off, br.vh) = (off, vh);
        }
        self.browse_visible()
    }

    /// The pointer is on item `i`: it becomes the selection (as the hand-written cells do), without scrolling.
    fn eng_page_select(&mut self, i: usize) -> Task<Message> {
        if let Some(p) = self.stack.last_mut() {
            if p.sel == i {
                return Task::none();
            }
            p.sel = i;
        }
        if let Some(h) = self.eng.page.as_mut() {
            h.sel = i;
            h.it.focus = h.scene.item_node(i);
        }
        self.eng_restyle(Which::Page);
        Task::none()
    }

    fn eng_build_search(&mut self) {
        let Some(tree) = self.search_tree() else {
            self.eng.search = None;
            return;
        };
        let it = Interaction {
            focus: tree.sel.and_then(|n| (n < tree.doc.len()).then_some(n)),
            ..Interaction::default()
        };
        // The list keeps its scroll across rebuilds (typing, a key moving the selection); the selection is brought into view.
        let keep = self.eng.search.take().and_then(|h| {
            let n = h.tree.list?;
            h.scene.scroll.offsets.get(&n).copied()
        });
        let mut s = self.eng_scene(&tree, it, (self.eng_width(), self.win.height));
        let cache = &mut self.eng.cache;
        let m = &mut |t: &str, st: &nocturne_style::TextStyle, mx: Option<f32>| {
            cache.measure(t, st, mx, &mut shape)
        };
        if let (Some(list), Some(o)) = (tree.list, keep) {
            s.scroll.offsets.insert(list, o);
            s.clamp_scroll();
            s.set_page_scroll(0.0, m);
        }
        if let Some(n) = tree.sel {
            s.reveal(n, m);
        }
        self.eng.search = Some(Hosted::new(s, tree, it, self.eng.sheet_gen));
    }

    fn eng_scene(&mut self, tree: &Tree, it: Interaction, size: (f32, f32)) -> Scene {
        let e = &mut self.eng;
        let t0 = Instant::now();
        let cache = &mut e.cache;
        let s = e
            .engine
            .build(&tree.doc, size, it, &tree.builders, &mut |text, st, max| {
                cache.measure(text, st, max, &mut shape)
            });
        for n in e.engine.take_notices() {
            eprintln!("engine: {n}");
        }
        if e.stats {
            eprintln!(
                "engine: {} nodes: cascade {} us, layout {} us, total {} us",
                s.doc.len(),
                s.timing.cascade.as_micros(),
                s.timing.layout.as_micros(),
                t0.elapsed().as_micros()
            );
            if std::env::var("NOCTURNE_ENGINE_DUMP").is_ok() {
                for i in 0..s.doc.len().min(80) {
                    eprintln!(
                        "engine: #{i} {} rect {:?} bg {} disp {:?} fixed {} vis {} opacity {} color {:?} overflow {:?}",
                        s.doc.describe(i),
                        s.rects[i],
                        s.style(i).bg.len(),
                        s.style(i).layout.display,
                        s.style(i).fixed,
                        s.style(i).visible,
                        s.style(i).opacity,
                        s.style(i).color,
                        s.style(i).layout.overflow
                    );
                }
            }
            for l in s.warnings().lines() {
                eprintln!("engine:   {l}");
            }
        }
        s
    }

    fn eng_restyle(&mut self, w: Which) {
        let stats = self.eng.stats;
        let (h, cache) = self.eng.host(w);
        let Some(h) = h else { return };
        let t0 = Instant::now();
        let r = h.scene.update(h.it, Instant::now(), &mut |text, st, max| {
            cache.measure(text, st, max, &mut shape)
        });
        if stats {
            eprintln!("engine: restyle {r:?} in {} us", t0.elapsed().as_micros());
        }
        if r.is_err() {
            self.eng.dirty = true;
            let _ = self.eng_sync(false);
        }
    }

    /// An event of one of the engine's widgets.
    pub(super) fn eng_event(&mut self, w: Which, e: StyledEvent) -> Task<Message> {
        // The Theme Studio's pick mode takes the pointer and the keys (scrolling and resizing still work).
        if let Some(t) = self.studio_event(w, &e) {
            return t;
        }
        match e {
            StyledEvent::Resize(s) => {
                let (h, cache) = self.eng.host(w);
                let mut resized = false;
                if let Some(h) = h
                    && h.scene.viewport != (s.width, s.height)
                {
                    h.scene.resize((s.width, s.height), &mut |t, st, m| {
                        cache.measure(t, st, m, &mut shape)
                    });
                    h.it = h.scene.interaction;
                    resized = true;
                }
                if resized && w == Which::Page {
                    return self.eng_page_scrolled();
                }
                Task::none()
            }
            StyledEvent::Hover(n) => {
                let (h, _) = self.eng.host(w);
                let Some(h) = h else {
                    return Task::none();
                };
                if h.it.hover == n {
                    return Task::none();
                }
                h.it.hover = n;
                // The pointer on a card moves the keyboard focus there (as the hand-written cells do).
                let cell = match w {
                    Which::Home => n.and_then(|n| ancestor_cell(&h.tree, &h.scene, n)),
                    _ => None,
                };
                let item = match w {
                    Which::Page => n.and_then(|n| h.scene.item_index(n)),
                    _ => None,
                };
                self.eng_restyle(w);
                match (w, cell, item) {
                    (Which::Home, Some((row, col)), _) => self.eng_focus(row, col),
                    (Which::Page, _, Some(i)) => self.eng_page_select(i),
                    _ => Task::none(),
                }
            }
            StyledEvent::Active(n) => {
                if let (Some(h), _) = self.eng.host(w) {
                    h.it.active = n;
                }
                self.eng_restyle(w);
                Task::none()
            }
            StyledEvent::Click(n) => {
                let (h, _) = self.eng.host(w);
                let Some(h) = h else {
                    return Task::none();
                };
                let mut at = Some(n);
                let mut chip: Option<CellAct> = None;
                while let Some(i) = at {
                    if let Some(m) = h.tree.acts.get(&i) {
                        return Task::done(m.clone());
                    }
                    let node = &h.scene.doc.nodes[i];
                    if w == Which::Page {
                        match node
                            .attrs
                            .iter()
                            .find(|(k, _)| k == "data-act")
                            .map(|(_, v)| v.as_str())
                        {
                            Some("fav") => chip = Some(CellAct::Fav),
                            Some("played") => chip = Some(CellAct::Played),
                            Some("play") => chip = Some(CellAct::Play),
                            _ => {}
                        }
                        if node.virtual_item
                            && let Some(idx) = h.scene.item_index(i)
                        {
                            return Task::done(Message::Cell(
                                At::Grid(idx),
                                chip.unwrap_or(CellAct::Open),
                            ));
                        }
                    }
                    at = node.parent;
                }
                Task::none()
            }
            StyledEvent::Wheel { target, dx, dy } => {
                let (h, cache) = self.eng.host(w);
                let Some(h) = h else {
                    return Task::none();
                };
                // A plain wheel over a strip scrolls the page; shift (or a sideways gesture) scrolls the strip.
                let moved = h.scene.wheel(target, dx, dy, &mut |t, st, m| {
                    cache.measure(t, st, m, &mut shape)
                });
                h.it.hover = h.scene.interaction.hover;
                h.it.focus = h.scene.interaction.focus;
                match (moved, w) {
                    (true, Which::Home) => self.eng_scrolled(),
                    (true, Which::Page) => self.eng_page_scrolled(),
                    _ => Task::none(),
                }
            }
            StyledEvent::Nav(nav) => match (w, nav) {
                (Which::Home, Nav::Dir(d)) => self.home_key(match d {
                    Dir::Up => "up",
                    Dir::Down => "down",
                    Dir::Left => "left",
                    Dir::Right => "right",
                }),
                (Which::Home, Nav::Activate) => self.home_key("enter"),
                (Which::Home, Nav::Next) => self.eng_tab(true),
                (Which::Home, Nav::Prev) => self.eng_tab(false),
                // Every other scene hands the key to the app's own key routing: the page's selection, the open menus and the search overlay already know what each key does.
                _ => {
                    let name = match nav {
                        Nav::Dir(Dir::Up) => "up",
                        Nav::Dir(Dir::Down) => "down",
                        Nav::Dir(Dir::Left) => "left",
                        Nav::Dir(Dir::Right) => "right",
                        Nav::Activate => "enter",
                        Nav::Next | Nav::Prev => "tab",
                    };
                    // `Message::Eng` does not mark the scenes stale on its own, and a key moves the selection the trees show (the search overlay's highlighted row).
                    self.eng.dirty = true;
                    self.on_key(crate::keys::Key {
                        name: name.into(),
                        shift: nav == Nav::Prev,
                        ..Default::default()
                    })
                }
            },
            StyledEvent::SafeMode => self.safe_theme(),
            StyledEvent::Frame(now) => {
                if let (Some(h), _) = self.eng.host(w) {
                    h.scene.tick(now);
                }
                Task::none()
            }
        }
    }

    /// A picture was decoded (`Some`) or failed (`None`): the nodes that wait for it are filled in place, no scene is rebuilt. A failure changes what a card shows (its initial), which takes a rebuild.
    pub(super) fn eng_image(&mut self, key: &str, big: bool) {
        let h = match if big {
            self.heroes.get(&key.to_string())
        } else {
            self.images.get(&key.to_string())
        } {
            Some(super::Img::Ready(h)) => h.clone(),
            Some(super::Img::Failed(_)) => {
                self.eng.dirty = true;
                return;
            }
            _ => return,
        };
        let e = &mut self.eng;
        for s in [&mut e.home, &mut e.hdr, &mut e.page, &mut e.search]
            .into_iter()
            .flatten()
        {
            s.image_arrived(key, &h);
        }
    }

    /// The page scrolled: keep the legacy scroll fields (they drive which images load) in step.
    fn eng_scrolled(&mut self) -> Task<Message> {
        let off = self
            .eng
            .home
            .as_ref()
            .map_or(0.0, |h| h.scene.scroll.page_y);
        let vh = self.win.height;
        let scrolled = off > SCROLLED;
        self.eng_home_edit(|doc, _| {
            let now = doc.nodes[0]
                .attrs
                .iter()
                .any(|(k, v)| k == "data-scrolled" && v == "true");
            if now == scrolled {
                return vec![];
            }
            super::hero_ui::set_attr(
                doc,
                0,
                "data-scrolled",
                if scrolled { "true" } else { "false" },
            );
            vec![0]
        });
        if let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) {
            (h.off, h.vh) = (off, vh);
            // Horizontal offsets of the strips.
        }
        let strips: Vec<(usize, f32)> = self
            .eng
            .home
            .as_ref()
            .map(|h| {
                h.tree
                    .strips
                    .iter()
                    .map(|(ri, n)| (*ri, h.scene.scroll.offsets.get(n).map_or(0.0, |o| o[0])))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) {
            for (ri, x) in strips {
                h.strips.insert(ri, x);
            }
        }
        self.home_wants()
    }

    /// The row arrows: scroll strip `row` sideways by `dx` px (negative: toward the start), as a wheel gesture would.
    pub(super) fn eng_strip_page(&mut self, row: usize, dx: f32) -> Task<Message> {
        let Some(node) = self
            .eng
            .home
            .as_ref()
            .and_then(|h| h.tree.strips.get(&row).copied())
        else {
            return Task::none();
        };
        let (h, cache) = self.eng.host(Which::Home);
        if let Some(h) = h {
            h.scene.wheel(Some(node), -dx, 0.0, &mut |t, st, m| {
                cache.measure(t, st, m, &mut shape)
            });
        }
        self.eng_scrolled()
    }

    /// Move the Home focus to `(row, col)` (a pointer on a card, or the keyboard).
    pub(super) fn eng_focus(&mut self, row: usize, col: usize) -> Task<Message> {
        let wants = self.hover_home(row, col);
        let hero = self
            .stack
            .last()
            .and_then(|p| p.home.as_deref())
            .and_then(|h| h.hero().map(|m| m.id.clone()));
        let changed = self.eng.home.as_ref().is_some_and(|h| h.hero != hero);
        if changed {
            // The hero follows the focused poster: a new tree.
            self.eng.dirty = true;
            let _ = self.eng_sync(false);
        } else if let Some(h) = self.eng.home.as_mut()
            && let Some(n) = h.tree.by_cell.get(&(row, col)).copied()
            && h.it.focus != Some(n)
        {
            h.it.focus = Some(n);
            self.eng_restyle(Which::Home);
        }
        wants
    }

    fn eng_tab(&mut self, forward: bool) -> Task<Message> {
        let Some(h) = self.eng.home.as_ref() else {
            return Task::none();
        };
        let next = h
            .scene
            .focus_next(h.it.focus, forward)
            .and_then(|n| h.tree.cells.get(&n).copied());
        match next {
            Some((r, c)) => self.eng_move(r, c),
            None => Task::none(),
        }
    }

    /// Keyboard on the engine's Home: arrows move by the laid-out geometry, Enter does what a click does. `None` when Home is not on the engine.
    pub(super) fn eng_key(&mut self, key: &str) -> Option<Task<Message>> {
        if !self.eng_home_active() {
            return None;
        }
        let dir = match key {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        let h = self.eng.home.as_ref()?;
        if let Some(d) = dir {
            let next =
                h.it.focus
                    .and_then(|f| h.scene.spatial(f, d))
                    .and_then(|n| h.tree.cells.get(&n).copied());
            return Some(match next {
                Some((r, c)) => self.eng_move(r, c),
                // Past the hero's last button: the carousel's previous or next slide.
                None => self.hero_edge(d),
            });
        }
        if key == "enter" {
            let mut at = h.it.focus;
            while let Some(i) = at {
                if let Some(m) = h.tree.acts.get(&i) {
                    return Some(Task::done(m.clone()));
                }
                at = h.scene.doc.nodes[i].parent;
            }
            // A menu link (a slot): the hand-written activation knows what it opens.
            let (row, col) = h.it.focus.and_then(|f| h.tree.cells.get(&f).copied())?;
            if row == 0 {
                let links = self.nav_links();
                return Some(self.activate_link(links.get(col).copied()));
            }
        }
        Some(Task::none())
    }

    /// Keyboard focus to `(row, col)`: scroll it into view, then follow the focus model.
    fn eng_move(&mut self, row: usize, col: usize) -> Task<Message> {
        let cache = &mut self.eng.cache;
        if let Some(h) = self.eng.home.as_mut()
            && let Some(&n) = h.tree.by_cell.get(&(row, col))
        {
            let m = &mut |t: &str, st: &nocturne_style::TextStyle, mx: Option<f32>| {
                cache.measure(t, st, mx, &mut shape)
            };
            match h.tree.bands.get(&row) {
                // A strip: the whole band into the page view, the card into the strip's view.
                Some(&band) => {
                    h.scene.reveal(band, m);
                    if let Some(&strip) = h.tree.strips.get(&row) {
                        let [x, _, w, _] = h.scene.rects[n];
                        let [sx, _, sw, _] = h.scene.rects[strip];
                        let off = h.scene.scroll.offsets.entry(strip).or_default();
                        let rel = x - sx;
                        if rel < off[0] {
                            off[0] = rel.max(0.0);
                        } else if rel + w > off[0] + sw {
                            off[0] = rel + w - sw;
                        }
                        h.scene.clamp_scroll();
                    }
                }
                None => h.scene.set_page_scroll(0.0, m),
            }
        }
        let scrolled = self.eng_scrolled();
        Task::batch([self.eng_focus(row, col), scrolled])
    }

    /// The scene of one of the engine's screens, if it exists now.
    pub(super) fn eng_scene_of(&self, w: Which) -> Option<super::studio::SceneRef<'_>> {
        use super::studio::SceneRef;
        let h = match w {
            Which::Home => &self.eng.home,
            Which::Header => &self.eng.hdr,
            Which::Page => &self.eng.page,
            Which::Search => &self.eng.search,
            // The detail page (when it is the page in view) and the player's bar keep their own scenes.
            Which::Detail => {
                let d = self.stack.last()?.det.as_deref()?;
                return d.eng_scene().map(SceneRef::Ref);
            }
            Which::Bar => return self.player.as_ref()?.bar_scene_now().map(SceneRef::Shared),
        };
        h.as_ref().map(|h| SceneRef::Ref(&h.scene))
    }

    /// The scene that is the page in view (the search overlay over a page, the library page, Home), for keyboard-only picking.
    pub(super) fn eng_main_scene(&self) -> Option<Which> {
        [
            Which::Bar,
            Which::Search,
            Which::Detail,
            Which::Page,
            Which::Home,
            Which::Header,
        ]
        .into_iter()
        .find(|&w| self.eng_scene_of(w).is_some())
    }

    /// Families the engine can use by name (for the Studio's font list).
    pub(super) fn eng_font_names(&self) -> Vec<String> {
        self.eng.engine.fonts.names()
    }

    /// The engine's Home as the page, or `None`.
    pub(super) fn eng_home_view(&self) -> Option<Element<'_, Message>> {
        if !self.eng_home_active() {
            return None;
        }
        let h = self.eng.home.as_ref()?;
        Some(self.eng_widget(h, Which::Home))
    }

    /// The engine's library page, or `None` (the hand-written page then draws it).
    pub(super) fn eng_page_view(&self) -> Option<Element<'_, Message>> {
        if !self.eng_page_active() {
            return None;
        }
        let h = self.eng.page.as_ref()?;
        Some(self.eng_widget(h, Which::Page))
    }

    /// The engine's search overlay, or `None`.
    pub(super) fn eng_search_view(&self) -> Option<Element<'_, Message>> {
        if !self.eng.enabled || !matches!(self.phase, Phase::Ready) || self.search.is_none() {
            return None;
        }
        let h = self.eng.search.as_ref()?;
        Some(self.eng_widget(h, Which::Search))
    }

    /// The engine's header for a page other than Home, or `None`.
    pub(super) fn eng_header_view(&self) -> Option<Element<'_, Message>> {
        if !self.eng_header_active() {
            return None;
        }
        let h = self.eng.hdr.as_ref()?;
        Some(
            column![
                container(self.eng_widget(h, Which::Header)).height(self.eng_header_h()),
                space().height(Length::Fill)
            ]
            .into(),
        )
    }

    fn eng_widget<'a>(&'a self, h: &'a Hosted, w: Which) -> Element<'a, Message> {
        // The header scene of another page must not take the arrows, Tab and Enter from the page below it.
        let mut s = Styled::new(&h.scene, move |e| Message::Eng(w, e))
            .keys(w != Which::Header)
            .images(&h.tree.images)
            .marks(self.studio_marks(w))
            .inert(self.studio_picking());
        for (i, slot) in h.tree.slots.iter().enumerate() {
            s = s.slot(i as u32 + 1, self.eng_slot(*slot));
        }
        s.into()
    }

    fn eng_slot(&self, s: Slot) -> Element<'_, Message> {
        use crate::widget::icon;
        match s {
            Slot::Back => super::header::hist_button(
                "\u{2039}",
                "Back (Alt+Left)",
                self.stack.len() > 1,
                Message::Back,
            ),
            Slot::Forward => super::header::hist_button(
                "\u{203A}",
                "Forward (Alt+Right)",
                !self.fwd.is_empty(),
                Message::Forward,
            ),
            Slot::Libraries { active, focused } => {
                let libs = self.stack.first().map_or(&[][..], |h| h.libs.items());
                self.libraries_menu(libs, active, focused)
            }
            Slot::Smart { active, focused } => self.smart_menu(active, focused),
            Slot::People => crate::widget::tip(
                crate::widget::press::Press::new(icon("people", 22.0, t::text()))
                    .on_press(Message::SyncPlay(crate::syncplay::Msg::OpenScreen))
                    .style(|_| iced::widget::button::Style::default()),
                "Watch together",
            ),
            Slot::Filter => crate::widget::tip(
                crate::widget::value_chip(
                    &self.filter_label(),
                    true,
                    false,
                    false,
                    Message::CycleFilter,
                ),
                "Show one source or all",
            ),
            Slot::Avatar => self.avatar(),
            Slot::Icon { name, side, lit } => icon(
                name,
                f32::from(side),
                if lit { t::text() } else { t::text_secondary() },
            )
            .into(),
            Slot::Sort | Slot::Filters | Slot::View(_) => self.page_slot(s),
            Slot::SearchInput => self.search_input(),
        }
    }

    /// Does this message change what Home or the header show? Frames, ticks, the engine's own pointer events and arriving pictures (filled in place by `eng_image`) do not.
    pub(super) fn eng_message_dirties(m: &Message) -> bool {
        if let Message::Studio(s) = m {
            return s.dirties();
        }
        !matches!(
            m,
            Message::Tick(_)
                | Message::WinFocus(_)
                | Message::Pointer(_)
                | Message::Frame(_)
                | Message::Eng(..)
                | Message::Noop
                | Message::NavScrolled(..)
                | Message::Image(..)
        )
    }
}

/// The font families the text shaper knows.
fn installed_families() -> Vec<String> {
    let Ok(mut fs) = iced::advanced::graphics::text::font_system().write() else {
        return vec![];
    };
    let mut v: Vec<String> = fs
        .raw()
        .db()
        .faces()
        .flat_map(|f| f.families.iter().map(|(n, _)| n.clone()))
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The `(row, col)` of the focusable node at or above `n`.
fn ancestor_cell(tree: &Tree, scene: &Scene, n: Id) -> Option<(usize, usize)> {
    let mut at = Some(n);
    while let Some(i) = at {
        if let Some(rc) = tree.cells.get(&i) {
            return Some(*rc);
        }
        at = scene.doc.nodes[i].parent;
    }
    None
}

#[allow(dead_code)]
fn _unused(_: HRow) {}

/// The engine's view of a theme for `NOCTURNE_THEME_REPORT`: whether the engine takes the sheet, what the Jellyfin translation did (selectors translated, lost by design, lost because the name is not in the map, variables mirrored) and the warnings of the sheet the engine reads.
pub(crate) fn theme_report(css: &str) -> serde_json::Value {
    let on = nocturne_style::is_engine_sheet(css);
    let (text, c) = nocturne_style::compat::translate(css);
    let rep = nocturne_style::Sheet::parse(&text).report;
    let kinds: std::collections::BTreeMap<&str, usize> = rep
        .groups
        .iter()
        .map(|(k, g)| (k.label(), g.count))
        .collect();
    serde_json::json!({
        "engine_sheet": on,
        "compat_applied": c.applied,
        "selectors": c.selectors,
        "translated": c.translated,
        "by_design": c.selectors - c.translated - c.unmapped(),
        "unmapped": c.unmapped(),
        "decls": c.decls,
        "decls_in_kept_rules": c.decls_reachable,
        "vars_mirrored": c.vars_mirrored,
        "top_reasons": c.groups().iter().take(8).map(|g| format!("{}: {}", g.0, g.1)).collect::<Vec<_>>(),
        "sheet_warnings": kinds,
    })
}
