//! Playback seam (embedded libmpv vs child-process mpv) and the pure logic of the player page:
//! backend choice, control auto-hide, key map. No GPUI, so it is unit-tested without a window.
use nocturne_player::{EmbeddedPlayer, Event, Player, Result, State};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// How long the controls stay up without mouse movement or keys.
pub const HIDE_AFTER: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Embedded,
    External,
}

/// `NOCTURNE_VIDEO=external|embedded` forces a backend; otherwise embed whenever we are on Wayland.
/// (Later failures - no viewporter, EGL - still fall back to external at creation time.)
pub fn choose(env: Option<&str>, wayland: bool) -> Choice {
    match env {
        Some("external") => Choice::External,
        Some("embedded") => Choice::Embedded,
        _ if wayland => Choice::Embedded,
        _ => Choice::External,
    }
}

/// Controls visibility: shown on activity, hidden `HIDE_AFTER` later unless something holds them.
pub struct AutoHide {
    last: Instant,
}

impl AutoHide {
    pub fn new(now: Instant) -> Self {
        Self { last: now }
    }
    pub fn poke(&mut self, now: Instant) {
        self.last = now;
    }
    /// `hold`: pointer over the controls or a drag in progress.
    pub fn visible(&self, now: Instant, hold: bool) -> bool {
        hold || now.saturating_duration_since(self.last) < HIDE_AFTER
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key {
    Toggle,
    Seek(f64),
    Volume(f64),
    Mute,
    Stop,
    Fullscreen,
    /// Esc: leave fullscreen first, then close playback.
    Back,
    Next,
}

pub fn key(k: &str) -> Option<Key> {
    Some(match k {
        "space" => Key::Toggle,
        "left" => Key::Seek(-10.0),
        "right" => Key::Seek(10.0),
        "up" => Key::Volume(5.0),
        "down" => Key::Volume(-5.0),
        "m" => Key::Mute,
        "s" => Key::Stop,
        "f" => Key::Fullscreen,
        "escape" => Key::Back,
        "n" => Key::Next,
        _ => return None,
    })
}

/// mpv's volume range is 0..=130.
pub fn bump_volume(cur: f64, d: f64) -> f64 {
    (cur + d).clamp(0.0, 130.0)
}

pub enum Playback {
    Embedded(Box<EmbeddedPlayer>),
    External(Player),
}

macro_rules! both {
    ($s:expr, $p:ident => $e:expr) => {
        match $s {
            Playback::Embedded($p) => $e,
            Playback::External($p) => $e,
        }
    };
}

impl Playback {
    pub fn load(&self, url: &str, headers: Option<&str>) -> Result<()> {
        both!(self, p => p.load(url, headers))
    }
    pub fn toggle(&self) -> Result<()> {
        both!(self, p => p.toggle())
    }
    pub fn seek_to(&self, s: f64) -> Result<()> {
        both!(self, p => p.seek_to(s))
    }
    pub fn seek_by(&self, s: f64) -> Result<()> {
        both!(self, p => p.seek_by(s))
    }
    pub fn set_volume(&self, v: f64) -> Result<()> {
        both!(self, p => p.set_volume(v))
    }
    pub fn state(&self) -> State {
        both!(self, p => p.state())
    }
    pub fn events(&self) -> &Receiver<Event> {
        both!(self, p => p.events())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_choice() {
        use Choice::*;
        assert_eq!(choose(None, true), Embedded);
        assert_eq!(choose(None, false), External);
        assert_eq!(choose(Some("external"), true), External);
        assert_eq!(choose(Some("embedded"), false), Embedded);
        assert_eq!(choose(Some("junk"), true), Embedded);
    }

    #[test]
    fn controls_hide_and_return() {
        let t0 = Instant::now();
        let mut h = AutoHide::new(t0);
        assert!(h.visible(t0 + Duration::from_millis(2900), false));
        assert!(!h.visible(t0 + HIDE_AFTER, false));
        assert!(
            h.visible(t0 + Duration::from_secs(60), true),
            "held by hover/drag"
        );
        h.poke(t0 + Duration::from_secs(10));
        assert!(
            h.visible(t0 + Duration::from_secs(11), false),
            "motion brings them back"
        );
        assert!(h.visible(t0, false), "clock going backwards never panics");
    }

    #[test]
    fn keys_and_volume() {
        assert_eq!(key("space"), Some(Key::Toggle));
        assert_eq!(key("left"), Some(Key::Seek(-10.0)));
        assert_eq!(key("right"), Some(Key::Seek(10.0)));
        assert_eq!(key("f"), Some(Key::Fullscreen));
        assert_eq!(key("escape"), Some(Key::Back));
        assert_eq!(key("x"), None);
        assert_eq!(bump_volume(128.0, 5.0), 130.0);
        assert_eq!(bump_volume(2.0, -5.0), 0.0);
    }
}
