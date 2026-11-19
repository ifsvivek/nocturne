//! The theme engine inside the app (feature `theme-engine`): Home and the shared header are element trees (`home_tree.rs`, `header_tree.rs`) styled by the default sheet and laid out by `nocturne-style`, painted by `widget/styled.rs`. This file owns the engine, the scenes and their interaction state (hover, focus, scroll), builds a scene when the data changes (never per frame), maps pointer and keyboard events to the same messages the hand-written Home sends, and chooses what to show: the engine when the feature is on and the theme only uses what the builders reproduce, the hand-written page otherwise (also with `NOCTURNE_ENGINE_UI=0`).
use super::header_tree::{Slot, Tree};
use super::home::HRow;
use super::message::{Message, Which};
use super::{App, Phase, View};
use crate::theme as t;
use crate::widget::cell::Look;
use crate::widget::styled::{Nav, Styled, StyledEvent};
use crate::widget::styled_preview::shape;
use iced::widget::{column, container, space};
use iced::{Element, Length, Task};
use nocturne_core::customize::{self as cz, Page};
use nocturne_style::{Builders, Dir, Engine, Id, Interaction, MeasureCache, Scene};
use std::time::Instant;

const DEFAULT_CSS: &str = include_str!("../../../style/themes/default.css");

/// One scene with the tree it was built from and its interaction state.
pub struct Hosted {
    pub scene: Scene,
    pub tree: Tree,
    pub it: Interaction,
    /// The hero item of the build: the scene is rebuilt when the focus makes it change.
    hero: Option<String>,
}

pub struct EngineUi {
    pub enabled: bool,
    engine: Engine,
    css: String,
    cache: MeasureCache,
    pub home: Option<Hosted>,
    pub hdr: Option<Hosted>,
    /// Home's scroll while another page is in view.
    saved: Option<nocturne_style::layout::Scroll>,
    pub dirty: bool,
    pub stats: bool,
    warmed: bool,
}

impl Default for EngineUi {
    fn default() -> Self {
        Self {
            enabled: std::env::var("NOCTURNE_ENGINE_UI").map_or(true, |v| v != "0"),
            engine: Engine::new(DEFAULT_CSS),
            css: String::new(),
            cache: MeasureCache::new(),
            home: None,
            hdr: None,
            saved: None,
            dirty: true,
            stats: std::env::var("NOCTURNE_ENGINE_STATS").is_ok(),
            warmed: false,
        }
    }
}

/// The `:root` tokens of the active theme, and the translator's rules (hover scale and transitions of the strips and buttons) as CSS, in front of the default sheet.
pub(super) fn sheet_css() -> String {
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
    s += &format!(
        " --nocturne-r-card: {}px; --nocturne-page-pad: {}px; }}\n",
        k.r_card,
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

    /// The header runs on the engine whenever the page below can be drawn with it (every page but Home has its own header scene).
    pub(super) fn eng_header_active(&self) -> bool {
        self.eng.enabled
            && matches!(self.phase, Phase::Ready)
            && self.eng.hdr.is_some()
            && !self.eng_home_active()
    }

    /// Rebuild the scenes if something they show changed. Called at the end of every update.
    pub(super) fn eng_sync(&mut self, msg_dirties: bool) {
        if !cfg!(debug_assertions) && false {
            return;
        }
        if msg_dirties {
            self.eng.dirty = true;
        }
        if !self.eng.enabled || !self.eng.dirty || !matches!(self.phase, Phase::Ready) {
            return;
        }
        self.eng.dirty = false;
        let css = sheet_css();
        if css != self.eng.css {
            let safe = self.eng.engine.safe_mode();
            self.eng.engine = Engine::new(&css);
            self.eng.engine.set_safe_mode(safe);
            self.eng.css = css;
        }
        self.eng.engine.set_safe_mode(self.th.safe);
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
    }

    fn eng_build_home(&mut self) {
        let Some(tree) = self.home_tree(true) else {
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
        let mut s = self.eng_scene(&tree, it, (self.win.width, self.win.height));
        if let Some(scroll) = keep {
            s.scroll = scroll;
            s.clamp_scroll();
        }
        self.eng.home = Some(Hosted {
            scene: s,
            tree,
            it,
            hero,
        });
    }

    fn eng_build_header(&mut self) {
        let mut tree = Tree::new();
        self.header_nodes(&mut tree, 0, false);
        let it = Interaction::default();
        let s = self.eng_scene(&tree, it, (self.win.width, t::header_h()));
        self.eng.hdr = Some(Hosted {
            scene: s,
            tree,
            it,
            hero: None,
        });
    }

    fn eng_scene(&mut self, tree: &Tree, it: Interaction, size: (f32, f32)) -> Scene {
        let e = &mut self.eng;
        let t0 = Instant::now();
        let cache = &mut e.cache;
        let s = e.engine.build(
            &tree.doc,
            size,
            it,
            &Builders::new(),
            &mut |text, st, max| cache.measure(text, st, max, &mut shape),
        );
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
                        "engine: #{i} {} rect {:?} bg {} disp {:?} fixed {} vis {}",
                        s.doc.describe(i),
                        s.rects[i],
                        s.style(i).bg.len(),
                        s.style(i).layout.display,
                        s.style(i).fixed,
                        s.style(i).visible
                    );
                }
            }
            for l in s.warnings().lines() {
                eprintln!("engine:   {l}");
            }
        }
        s
    }

    fn eng_hosted(&mut self, w: Which) -> Option<&mut Hosted> {
        match w {
            Which::Home => self.eng.home.as_mut(),
            Which::Header => self.eng.hdr.as_mut(),
        }
    }

    fn eng_restyle(&mut self, w: Which) {
        let stats = self.eng.stats;
        let cache = &mut self.eng.cache;
        let h = match w {
            Which::Home => self.eng.home.as_mut(),
            Which::Header => self.eng.hdr.as_mut(),
        };
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
            self.eng_sync(false);
        }
    }

    /// An event of one of the engine's widgets.
    pub(super) fn eng_event(&mut self, w: Which, e: StyledEvent) -> Task<Message> {
        match e {
            StyledEvent::Resize(s) => {
                let cache = &mut self.eng.cache;
                if let Some(h) = match w {
                    Which::Home => self.eng.home.as_mut(),
                    Which::Header => self.eng.hdr.as_mut(),
                } && h.scene.viewport != (s.width, s.height)
                {
                    h.scene.resize((s.width, s.height), &mut |t, st, m| {
                        cache.measure(t, st, m, &mut shape)
                    });
                }
                Task::none()
            }
            StyledEvent::Hover(n) => {
                let Some(h) = self.eng_hosted(w) else {
                    return Task::none();
                };
                if h.it.hover == n {
                    return Task::none();
                }
                h.it.hover = n;
                // The pointer on a card moves the keyboard focus there (as the hand-written cells do).
                let cell = n.and_then(|n| ancestor_cell(&h.tree, &h.scene, n));
                self.eng_restyle(w);
                match (w, cell) {
                    (Which::Home, Some((row, col))) => self.eng_focus(row, col),
                    _ => Task::none(),
                }
            }
            StyledEvent::Active(n) => {
                if let Some(h) = self.eng_hosted(w) {
                    h.it.active = n;
                }
                self.eng_restyle(w);
                Task::none()
            }
            StyledEvent::Click(n) => {
                let Some(h) = self.eng_hosted(w) else {
                    return Task::none();
                };
                let mut at = Some(n);
                while let Some(i) = at {
                    if let Some(m) = h.tree.acts.get(&i) {
                        return Task::done(m.clone());
                    }
                    at = h.scene.doc.nodes[i].parent;
                }
                Task::none()
            }
            StyledEvent::Wheel { target, dx, dy } => {
                let cache = &mut self.eng.cache;
                let Some(h) = (match w {
                    Which::Home => self.eng.home.as_mut(),
                    Which::Header => self.eng.hdr.as_mut(),
                }) else {
                    return Task::none();
                };
                // A plain wheel over a strip scrolls the page; shift (or a sideways gesture) scrolls the strip.
                let moved = h.scene.wheel(target, dx, dy, &mut |t, st, m| {
                    cache.measure(t, st, m, &mut shape)
                });
                if moved && w == Which::Home {
                    return self.eng_scrolled();
                }
                Task::none()
            }
            StyledEvent::Nav(nav) => match nav {
                Nav::Dir(d) => self.home_key(match d {
                    Dir::Up => "up",
                    Dir::Down => "down",
                    Dir::Left => "left",
                    Dir::Right => "right",
                }),
                Nav::Activate => self.home_key("enter"),
                Nav::Next => self.eng_tab(true),
                Nav::Prev => self.eng_tab(false),
            },
            StyledEvent::SafeMode => self.safe_theme(),
            StyledEvent::Frame(now) => {
                if let Some(h) = self.eng_hosted(w) {
                    h.scene.tick(now);
                }
                Task::none()
            }
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
            self.eng_sync(false);
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
                None => Task::none(),
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

    /// The engine's Home as the page, or `None`.
    pub(super) fn eng_home_view(&self) -> Option<Element<'_, Message>> {
        if !self.eng_home_active() {
            return None;
        }
        let h = self.eng.home.as_ref()?;
        Some(self.eng_widget(h, Which::Home))
    }

    /// The engine's header for a page other than Home, or `None`.
    pub(super) fn eng_header_view(&self) -> Option<Element<'_, Message>> {
        if !self.eng_header_active() {
            return None;
        }
        let h = self.eng.hdr.as_ref()?;
        Some(
            column![
                container(self.eng_widget(h, Which::Header)).height(t::header_h()),
                space().height(Length::Fill)
            ]
            .into(),
        )
    }

    fn eng_widget<'a>(&'a self, h: &'a Hosted, w: Which) -> Element<'a, Message> {
        let mut s = Styled::new(&h.scene, move |e| Message::Eng(w, e)).images(&h.tree.images);
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
        }
    }

    /// Does this message change what Home or the header show? Frames, ticks and the engine's own pointer events do not.
    pub(super) fn eng_message_dirties(m: &Message) -> bool {
        !matches!(
            m,
            Message::Tick(_)
                | Message::Frame(_)
                | Message::Eng(..)
                | Message::Noop
                | Message::NavScrolled(..)
        )
    }
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
