//! The hero carousel of the engine's Home (feature `theme-engine`): the slides, the timer fed from the root tick, the pause reasons, manual steps and the description's open state. The pure parts (`hero::collect`, `Hero`, `Pauses`, `Expanded`) live in `nocturne-core`; this is their wiring. The timer runs only while `Hero::running`; a slide change rebuilds the scene (a new tree), and the outgoing slide's picture fades out from an in-place edit of the new scene, so its transition runs.
use super::App;
use super::home::HRow;
use super::message::Message;
use iced::Task;
use nocturne_core::hero::{DEFAULT_ORDER, Pauses};
use nocturne_core::source::Media;
use nocturne_style::{Dir, Doc, Id};
use std::time::{Duration, Instant};

/// A fade layer older than this is not drawn again by a later rebuild of the same slide.
const FADE_LIFE: Duration = Duration::from_millis(500);

/// The key a hero description's open state is kept under.
pub(super) fn hero_key(m: &Media) -> String {
    format!("{}:{}", m.origin.0, m.id)
}

/// Set (or add) attribute `k` of node `n`.
pub(super) fn set_attr(doc: &mut Doc, n: Id, k: &str, v: &str) {
    let attrs = &mut doc.nodes[n].attrs;
    match attrs.iter_mut().find(|(name, _)| name == k) {
        Some(a) => a.1 = v.to_string(),
        None => attrs.push((k.to_string(), v.to_string())),
    }
}

impl App {
    /// A stylesheet is active: slides follow the spec order (continue watching first) and the description is a keyboard stop. With no theme the first slide stays what the hero showed before the carousel.
    pub(super) fn hero_themed(&self) -> bool {
        self.themes
            .active_css(self.theme_source(), self.theme_ui())
            .is_some()
    }

    /// 5 times a second from the root tick: keep the slides current, feed the pauses, advance the timer and move the progress hairline.
    pub(super) fn hero_tick(&mut self, now: Instant) -> Task<Message> {
        if !self.eng.enabled {
            return Task::none();
        }
        let active = self.eng_home_active();
        let themed = self.hero_themed();
        let autoplay = self.settings.hero_autoplay && !crate::motion::reduced();
        let secs = self.settings.hero_interval_secs;
        let (focused, pointer, playback) =
            (self.win_focused, self.pointer_down, self.player.is_some());
        let dialog = !active || self.search.is_some() || self.menu;
        let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) else {
            return Task::none();
        };
        let in_use = h.touched
            && h.layout()
                .get(h.row)
                .is_some_and(|k| !matches!(k, HRow::Nav));
        let slides = h.compute_slides(&DEFAULT_ORDER, !themed);
        let mut dirty = false;
        let same = slides.len() == h.slides.len()
            && slides
                .iter()
                .zip(&h.slides)
                .all(|(a, b)| (a.origin, &a.id) == (b.origin, &b.id));
        if !same {
            let cur = h
                .slides
                .get(h.carousel.index())
                .map(|m| (m.origin, m.id.clone()));
            h.slides = slides;
            h.carousel.set_len(h.slides.len());
            if let Some(c) = cur
                && let Some(i) = h.slides.iter().position(|m| (m.origin, m.id.clone()) == c)
            {
                h.carousel.goto(i);
            }
            dirty = true;
        }
        h.carousel.set_autoplay(autoplay, secs);
        h.carousel.pauses = Pauses {
            window_inactive: !focused,
            pointer_down: pointer,
            description_open: h.expanded.any_open(),
            focus_inside: in_use,
            dialog,
            playback,
        };
        let before = h.carousel.index();
        if h.carousel.tick(now)
            && let Some(prev) = h.slides.get(before).cloned()
        {
            h.fade = Some((prev, now));
            dirty = true;
        }
        let (progress, running) = (h.carousel.progress(), h.carousel.running());
        if dirty {
            self.eng.dirty = true;
            return self.home_wants();
        }
        if running && active {
            self.eng_home_edit(|doc, tree| match tree.hero_fx.progress {
                Some(n) => {
                    doc.nodes[n].inline = Some(format!("width: {:.1}%", progress * 100.0));
                    vec![n]
                }
                None => vec![],
            });
        }
        Task::none()
    }

    /// Run `f` on the carousel and rebuild Home for the new slide (the old one fades out).
    fn hero_change(&mut self, f: impl FnOnce(&mut nocturne_core::hero::Hero)) -> Task<Message> {
        let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) else {
            return Task::none();
        };
        if h.slides.len() < 2 {
            return Task::none();
        }
        let before = h.carousel.index();
        f(&mut h.carousel);
        if h.carousel.index() == before {
            return Task::none();
        }
        h.fade = h.slides.get(before).cloned().map(|m| (m, Instant::now()));
        self.eng.dirty = true;
        self.home_wants()
    }

    pub(super) fn hero_step(&mut self, d: isize) -> Task<Message> {
        self.hero_change(|c| if d < 0 { c.prev() } else { c.next() })
    }

    pub(super) fn hero_goto(&mut self, i: usize) -> Task<Message> {
        self.hero_change(|c| c.goto(i))
    }

    /// Open or close the shown slide's description.
    pub(super) fn hero_toggle(&mut self) -> Task<Message> {
        let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) else {
            return Task::none();
        };
        if let Some(key) = h.hero().map(hero_key) {
            h.expanded.toggle(&key);
            self.eng.dirty = true;
        }
        Task::none()
    }

    /// Left or Right at the end of the hero's buttons: the previous or next slide, focus on its last or first button.
    pub(super) fn hero_edge(&mut self, d: Dir) -> Task<Message> {
        if !matches!(d, Dir::Left | Dir::Right) {
            return Task::none();
        }
        let in_hero = self
            .stack
            .first()
            .and_then(|p| p.home.as_deref())
            .is_some_and(|h| h.layout().get(h.row) == Some(&HRow::Hero));
        if !in_hero {
            return Task::none();
        }
        let left = d == Dir::Left;
        let t = self.hero_step(if left { -1 } else { 1 });
        if let Some(h) = self.stack.first_mut().and_then(|p| p.home.as_deref_mut()) {
            h.col = usize::from(left);
        }
        t
    }

    /// After Home's scene was built: the slide that is leaving starts to fade (its layer was built opaque).
    pub(super) fn hero_settle(&mut self) {
        self.eng_home_edit(|doc, tree| match tree.hero_fx.fade {
            Some(n) => {
                set_attr(doc, n, "data-phase", "b");
                vec![n]
            }
            None => vec![],
        });
    }

    /// The fading-out slide of this build, if it is young enough to still be drawn.
    pub(super) fn hero_fading(h: &super::home::Home) -> Option<&Media> {
        h.fade
            .as_ref()
            .filter(|(_, at)| at.elapsed() < FADE_LIFE)
            .map(|(m, _)| m)
    }
}
