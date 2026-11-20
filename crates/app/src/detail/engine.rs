//! The detail page on the theme engine (feature `theme-engine`): owns the engine, the scene built from `detail_tree.rs` and its interaction state (hover, focus, scroll); builds the scene when the page's data changes (never per frame), and maps pointer and keyboard events to the page's own messages and keys, so the page keeps one focus model (`Focus`) whether it is drawn by the engine or by the hand-written widgets. `NOCTURNE_ENGINE_UI=0` shows the hand-written page.
use super::detail_tree::DTree;
use super::model::DRow;
use super::{Detail, Msg, Outcome};
use crate::keys::Key;
use crate::widget::styled::{Nav, Styled, StyledEvent};
use crate::widget::styled_preview::shape;
use iced::{Element, Task};
use nocturne_style::{Builders, Dir, Engine, Interaction, MeasureCache, Scene, TextStyle};
use std::time::Instant;

pub(super) struct Host {
    engine: Engine,
    /// The sheet the engine was last built from (tokens of the active theme and the default rules).
    sheet: String,
    user: Option<String>,
    /// The base sheet's address and the text that went over it, when the layout mode built the user layer.
    jf_key: Option<(usize, String)>,
    cache: MeasureCache,
    scene: Option<Scene>,
    tree: Option<DTree>,
    it: Interaction,
    /// The last page focus the scene was moved to (a change scrolls it into view).
    at: Option<(DRow, usize)>,
    /// Tab put the focus on the description toggle, which is outside the page's focus model.
    on_toggle: bool,
}

pub(super) fn enabled() -> bool {
    std::env::var("NOCTURNE_ENGINE_UI").map_or(true, |v| v != "0")
}

impl Host {
    pub(super) fn new() -> Self {
        let sheet = sheet();
        let user = std::env::var("NOCTURNE_ENGINE_CSS")
            .ok()
            .filter(|_| cfg!(debug_assertions))
            .and_then(|p| std::fs::read_to_string(p).ok());
        let mut engine = Engine::new(&sheet);
        engine.set_user_css(user.as_deref());
        Self {
            engine,
            sheet,
            user,
            jf_key: None,
            cache: MeasureCache::new(),
            scene: None,
            tree: None,
            it: Interaction {
                focus_visible: false,
                ..Interaction::default()
            },
            at: None,
            on_toggle: false,
        }
    }
}

/// The app's sheet (tokens of the active theme, default rules, hover and focus rules) plus the page's own derived tokens.
fn sheet() -> String {
    crate::app::engine_ui::sheet_css()
}

fn warm() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    // The first measurement starts the font system: do it before the first scene is timed.
    ONCE.call_once(|| {
        let st = TextStyle {
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
    });
}

impl Detail {
    /// The user stylesheet of the active theme (layered over the default sheet), or none.
    pub fn set_user_css(&mut self, css: Option<String>) {
        let changed = match self.eng.as_mut() {
            Some(h) if h.user != css => {
                h.engine.set_user_css(css.as_deref());
                h.user = css;
                true
            }
            _ => false,
        };
        if changed {
            self.eng_sync(false);
        }
    }

    /// The fetched font families the page's text may name (the theme's web fonts, the web client's icon font).
    pub fn set_fams(&mut self, fams: Vec<String>) {
        if self.fams != fams {
            self.fams = fams;
            self.eng_sync(false);
        }
    }

    /// Jellyfin layout mode for this page: the server's base sheet and the branding and theme text (`None` leaves the mode).
    pub fn set_jf(&mut self, jf: Option<(std::sync::Arc<String>, String)>) {
        let same = match (&self.jf, &jf) {
            (Some((a, x)), Some((b, y))) => std::sync::Arc::ptr_eq(a, b) && x == y,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.jf = jf;
            self.eng_sync(false);
        }
    }

    /// Is the page drawn by the engine right now?
    pub(super) fn eng_active(&self) -> bool {
        self.eng.as_ref().is_some_and(|h| h.scene.is_some())
    }

    /// The `(row, index)` the page's focus points at.
    fn focus_cell(&self) -> (DRow, usize) {
        let shape = self.shape();
        let cur = shape.current(&self.focus);
        let i = match cur {
            DRow::Actions | DRow::Chips | DRow::Strip(_) => self.focus.col,
            DRow::Seasons => self.focus.season,
            DRow::List => self.focus.ep,
        };
        (cur, i)
    }

    /// Build the scene again from the page's data (keeping the scroll), and move the scene's focus to the page's. `reveal`: scroll the focus into view when it moved.
    pub(super) fn eng_sync(&mut self, reveal: bool) {
        let Some(mut h) = self.eng.take() else { return };
        warm();
        let jf = self.jf.is_some();
        let sheet = if jf {
            crate::app::jf::UA.to_string()
        } else {
            sheet()
        };
        if sheet != h.sheet {
            let safe = h.engine.safe_mode();
            h.engine = Engine::new(&sheet);
            h.engine.set_safe_mode(safe);
            h.engine.set_compat(!jf);
            h.engine.set_user_css(h.user.as_deref());
            h.jf_key = None;
            h.sheet = sheet;
        }
        for f in &self.fams {
            if !h.engine.fonts_mut().has(f) {
                h.engine.fonts_mut().register(f);
            }
        }
        if let Some((base, text)) = &self.jf {
            let key = (std::sync::Arc::as_ptr(base) as usize, text.clone());
            if h.jf_key.as_ref() != Some(&key) {
                h.engine.set_user_css(Some(&format!("{base}\n{text}")));
                h.jf_key = Some(key);
            }
        }
        let tree = if jf { self.jf_tree() } else { self.tree() };
        let cell = self.focus_cell();
        let toggle = h.on_toggle.then_some(tree.toggle).flatten();
        h.on_toggle = toggle.is_some();
        h.it.focus = toggle.or_else(|| tree.by_cell.get(&cell).copied());
        let keep = h.scene.take().map(|s| s.scroll);
        let size = (self.win.width, self.win.height);
        let cache = &mut h.cache;
        let mut scene = h.engine.build(
            &tree.doc,
            size,
            h.it,
            &Builders::new(),
            &mut |text, st, max| cache.measure(text, st, max, &mut shape),
        );
        for n in h.engine.take_notices() {
            eprintln!("engine: {n}");
        }
        if let Some(sc) = keep {
            scene.scroll = sc;
            scene.clamp_scroll();
        }
        h.scene = Some(scene);
        let moved = h.at != Some(cell);
        h.at = Some(cell);
        h.tree = Some(tree);
        self.eng = Some(h);
        if reveal && moved {
            self.eng_reveal();
        }
    }

    /// Scroll the focused element into view, below the floating header.
    fn eng_reveal(&mut self) {
        let Some(h) = self.eng.as_mut() else { return };
        let (Some(scene), Some(tree)) = (h.scene.as_mut(), h.tree.as_ref()) else {
            return;
        };
        let Some(n) = h.it.focus else { return };
        let cache = &mut h.cache;
        let m =
            &mut |t: &str, st: &TextStyle, mx: Option<f32>| cache.measure(t, st, mx, &mut shape);
        if tree.cells.get(&n).is_some_and(|c| c.0 == DRow::Actions) {
            scene.set_page_scroll(0.0, m);
            return;
        }
        // A cell inside a strip: the cell comes into the strip's view, then the page shows the whole strip block.
        scene.reveal(n, m);
        let mut target = n;
        if matches!(tree.cells.get(&n), Some((DRow::Strip(_), _))) {
            let mut at = scene.doc.nodes[n].parent;
            while let Some(a) = at {
                if scene.doc.nodes[a].classes.iter().any(|c| c == "section") {
                    target = a;
                    break;
                }
                at = scene.doc.nodes[a].parent;
            }
            scene.reveal(target, m);
        }
        let (r, vh) = (scene.nav_rect(target), scene.viewport.1);
        if r[1] + r[3] + crate::theme::S4 > scene.scroll.page_y + vh {
            scene.set_page_scroll(r[1] + r[3] + crate::theme::S4 - vh, m);
        }
        // Keep clear of the header and breadcrumb strip floating over the page.
        let clear = crate::theme::HEADER_H + 40.0;
        let r = scene.nav_rect(n);
        if r[1] < scene.scroll.page_y + clear {
            let y = (r[1] - clear - crate::theme::S4).max(0.0);
            scene.set_page_scroll(y, m);
        }
    }

    fn eng_restyle(&mut self) {
        let Some(h) = self.eng.as_mut() else { return };
        let Some(scene) = h.scene.as_mut() else {
            return;
        };
        let cache = &mut h.cache;
        let r = scene.update(h.it, Instant::now(), &mut |text, st, max| {
            cache.measure(text, st, max, &mut shape)
        });
        if r.is_err() {
            self.eng_sync(false);
        }
    }

    /// The scene's focus follows the page's after a message that moved it.
    fn eng_follow_focus(&mut self, reveal: bool) {
        let cell = self.focus_cell();
        let Some(h) = self.eng.as_mut() else { return };
        let node = h.tree.as_ref().and_then(|t| t.by_cell.get(&cell).copied());
        if h.at == Some(cell) && !h.on_toggle {
            return;
        }
        h.on_toggle = false;
        h.at = Some(cell);
        h.it.focus = node;
        self.eng_restyle();
        if reveal {
            self.eng_reveal();
        }
    }

    /// An event of the paint widget.
    pub(super) fn eng_event(&mut self, e: StyledEvent) -> Task<Msg> {
        match e {
            StyledEvent::Resize(s) => {
                if let Some(h) = self.eng.as_mut() {
                    let cache = &mut h.cache;
                    if let Some(sc) = h.scene.as_mut()
                        && sc.viewport != (s.width, s.height)
                    {
                        sc.resize((s.width, s.height), &mut |t, st, m| {
                            cache.measure(t, st, m, &mut shape)
                        });
                    }
                }
                Task::none()
            }
            StyledEvent::Hover(n) => {
                let Some(h) = self.eng.as_mut() else {
                    return Task::none();
                };
                if h.it.hover == n {
                    return Task::none();
                }
                h.it.hover = n;
                let msg = n.and_then(|n| {
                    let (sc, tr) = (h.scene.as_ref()?, h.tree.as_ref()?);
                    ancestor(sc, n, |i| tr.hover.get(&i).cloned())
                });
                self.eng_restyle();
                match msg {
                    // The pointer on a cell moves the page's focus there (no scrolling).
                    Some(m) => {
                        let t = self.update_inner(m);
                        self.eng_follow_focus(false);
                        t
                    }
                    None => Task::none(),
                }
            }
            StyledEvent::Active(n) => {
                if let Some(h) = self.eng.as_mut() {
                    h.it.active = n;
                }
                self.eng_restyle();
                Task::none()
            }
            StyledEvent::Click(n) => {
                let msg = self.eng.as_ref().and_then(|h| {
                    let (sc, tr) = (h.scene.as_ref()?, h.tree.as_ref()?);
                    ancestor(sc, n, |i| tr.acts.get(&i).cloned())
                });
                match msg {
                    Some(m) => self.update(m),
                    None => Task::none(),
                }
            }
            StyledEvent::Wheel { target, dx, dy } => {
                if let Some(h) = self.eng.as_mut()
                    && let Some(sc) = h.scene.as_mut()
                {
                    let cache = &mut h.cache;
                    sc.wheel(target, dx, dy, &mut |t, st, m| {
                        cache.measure(t, st, m, &mut shape)
                    });
                }
                Task::none()
            }
            StyledEvent::Nav(nav) => self.eng_nav(nav),
            StyledEvent::SafeMode => {
                self.outcomes.push(Outcome::SafeLook);
                Task::none()
            }
            StyledEvent::Frame(now) => {
                if let Some(sc) = self.eng.as_mut().and_then(|h| h.scene.as_mut()) {
                    sc.tick(now);
                }
                Task::none()
            }
        }
    }

    fn eng_nav(&mut self, nav: Nav) -> Task<Msg> {
        let named = |n: &str| Key {
            name: n.into(),
            ..Key::default()
        };
        match nav {
            Nav::Dir(d) => {
                if let Some(h) = self.eng.as_mut() {
                    h.on_toggle = false;
                }
                let k = named(match d {
                    Dir::Up => "up",
                    Dir::Down => "down",
                    Dir::Left => "left",
                    Dir::Right => "right",
                });
                self.key(&k).unwrap_or_else(Task::none)
            }
            Nav::Activate => {
                // The description toggle is outside the page's focus model: it has its own click action.
                let on_toggle = self.eng.as_ref().is_some_and(|h| h.on_toggle);
                if on_toggle {
                    return self.update(Msg::Expand);
                }
                self.key(&named("enter")).unwrap_or_else(Task::none)
            }
            Nav::Next | Nav::Prev => {
                let Some(h) = self.eng.as_ref() else {
                    return Task::none();
                };
                let (Some(sc), Some(tr)) = (h.scene.as_ref(), h.tree.as_ref()) else {
                    return Task::none();
                };
                let Some(n) = sc.focus_next(h.it.focus, matches!(nav, Nav::Next)) else {
                    return Task::none();
                };
                if Some(n) == tr.toggle {
                    let h = self.eng.as_mut().expect("host");
                    h.on_toggle = true;
                    h.it.focus = Some(n);
                    self.eng_restyle();
                    self.eng_reveal();
                    return Task::none();
                }
                let Some((row, i)) = tr.cells.get(&n).copied() else {
                    return Task::none();
                };
                if let Some(h) = self.eng.as_mut() {
                    h.on_toggle = false;
                }
                self.focus_on(row, i);
                self.eng_follow_focus(true);
                self.want()
            }
        }
    }

    /// The page as the engine draws it, or `None` (the hand-written page is used).
    pub(super) fn eng_view(&self) -> Option<Element<'_, Msg>> {
        let h = self.eng.as_ref()?;
        let (scene, tree) = (h.scene.as_ref()?, h.tree.as_ref()?);
        Some(Styled::new(scene, Msg::Eng).images(&tree.images).into())
    }
}

/// The nearest node at or above `n` for which `f` gives something.
fn ancestor<T>(scene: &Scene, n: usize, f: impl Fn(usize) -> Option<T>) -> Option<T> {
    let mut at = Some(n);
    while let Some(i) = at {
        if let Some(v) = f(i) {
            return Some(v);
        }
        at = scene.doc.nodes[i].parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;
    use nocturne_core::source::{Media, SourceId};
    use nocturne_jellyfin as jf;

    fn page(overview: &str) -> Detail {
        let mut m = Media::from_item_in(
            SourceId(1),
            &jf::Item {
                id: "a".into(),
                name: "Film".into(),
                kind: "Movie".into(),
                overview: Some(overview.into()),
                ..Default::default()
            },
        );
        m.available = true;
        let (mut d, _) = Detail::new(m, vec![], vec![], Size::new(1280.0, 800.0), false, vec![]);
        d.eng = Some(Box::new(Host::new()));
        d.eng_sync(false);
        d
    }

    fn classes(d: &Detail, c: &str) -> usize {
        let t = d.eng.as_ref().unwrap().tree.as_ref().unwrap();
        (0..t.doc.len())
            .filter(|&i| t.doc.nodes[i].classes.iter().any(|x| x == c))
            .count()
    }

    fn key(name: &str) -> Key {
        Key {
            name: name.into(),
            ..Key::default()
        }
    }

    #[test]
    fn the_tree_has_the_vocabulary_and_one_button_per_action() {
        let d = page("A short line.");
        assert_eq!(classes(&d, "page-detail"), 1);
        assert_eq!(classes(&d, "detail-hero"), 1);
        assert_eq!(classes(&d, "button"), d.dacts().len());
        assert_eq!(
            classes(&d, "button-primary"),
            1,
            "Play is the primary button"
        );
        assert_eq!(
            classes(&d, "detail-title"),
            1,
            "no logo art: the title is text"
        );
        assert!(d.eng_active());
    }

    #[test]
    fn the_scene_focus_follows_the_page_focus() {
        let mut d = page("A short line.");
        let at = |d: &Detail| {
            let h = d.eng.as_ref().unwrap();
            h.tree
                .as_ref()
                .unwrap()
                .cells
                .get(&h.it.focus.unwrap())
                .copied()
        };
        assert_eq!(at(&d), Some((DRow::Actions, 0)));
        d.key(&key("right"));
        assert_eq!(at(&d), Some((DRow::Actions, 1)));
        assert_eq!(d.focus.col, 1);
    }

    #[test]
    fn a_long_description_gets_a_toggle_and_opens() {
        let long = "word ".repeat(200);
        let mut d = page(&long);
        let summary = |d: &Detail| {
            let t = d.eng.as_ref().unwrap().tree.as_ref().unwrap();
            let n = (0..t.doc.len())
                .find(|&i| t.doc.nodes[i].classes.iter().any(|x| x == "detail-summary"))
                .unwrap();
            t.doc.nodes[n]
                .attrs
                .iter()
                .find(|a| a.0 == "data-expanded")
                .map(|a| a.1.clone())
        };
        assert!(
            d.eng
                .as_ref()
                .unwrap()
                .tree
                .as_ref()
                .unwrap()
                .toggle
                .is_some()
        );
        assert_eq!(summary(&d).as_deref(), Some("false"));
        let _ = d.update(Msg::Expand);
        assert_eq!(summary(&d).as_deref(), Some("true"));
        let short = page("Short.");
        assert!(
            short
                .eng
                .as_ref()
                .unwrap()
                .tree
                .as_ref()
                .unwrap()
                .toggle
                .is_none()
        );
    }

    #[test]
    fn tab_reaches_the_description_toggle_and_enter_opens_it() {
        let mut d = page(&"word ".repeat(200));
        // The default look keeps the toggle out of sight (and out of the tab order); a theme shows it.
        d.set_user_css(Some(".detail-more { display: block }".into()));
        // Actions are the first stops, then the toggle follows the visual order (it sits above the buttons).
        for _ in 0..8 {
            if d.eng.as_ref().unwrap().on_toggle {
                break;
            }
            let _ = d.eng_nav(Nav::Next);
        }
        assert!(d.eng.as_ref().unwrap().on_toggle);
        let _ = d.eng_nav(Nav::Activate);
        assert!(d.expanded.is_open(&d.expand_key()));
    }

    #[test]
    fn the_starters_apply_without_a_single_warning() {
        for (name, css) in nocturne_style::STARTERS {
            let mut d = page(&"word ".repeat(200));
            d.set_user_css(Some((*css).into()));
            let r = d.eng.as_ref().unwrap().scene.as_ref().unwrap().warnings();
            assert!(r.is_empty(), "{name}: {r:?}");
            assert_eq!(classes(&d, "detail-hero"), 1);
        }
    }
}
