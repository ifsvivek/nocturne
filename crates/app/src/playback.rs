//! What the now-playing bar shows. The `Player` itself lives on `Root` (UI thread only).
use crate::source::Media;
use nocturne_player::State;
use std::time::{Duration, Instant};

pub struct Now {
    pub media: Media,
    pub state: Option<State>,
    pub loaded: bool,
    pub started: Instant,
    /// Saved position to seek to once the file is loaded.
    pub resume_at: Option<f64>,
    /// Next episode, offered when this one ends.
    pub next: Option<Media>,
    pub ended: bool,
}

impl Now {
    pub fn new(media: Media) -> Self {
        let resume_at = media.resume_s.filter(|_| !media.played);
        Self {
            media,
            state: None,
            loaded: false,
            started: Instant::now(),
            resume_at,
            next: None,
            ended: false,
        }
    }

    /// Position/duration in seconds, when known.
    pub fn times(&self) -> (f64, Option<f64>) {
        let s = self.state.as_ref();
        (
            s.and_then(|s| s.position).unwrap_or(0.0),
            s.and_then(|s| s.duration),
        )
    }

    pub fn frac(&self) -> f32 {
        match self.times() {
            (p, Some(d)) if d > 0.0 => (p / d).clamp(0.0, 1.0) as f32,
            _ => 0.0,
        }
    }

    /// Never became playable: mpv is idle again after a grace period (calm failure).
    pub fn stalled(&self, idle: bool) -> bool {
        !self.loaded && idle && self.started.elapsed() > Duration::from_secs(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Now {
        Now::new(crate::source::Media::from_item(&Default::default()))
    }

    #[test]
    fn frac_and_stall() {
        let mut n = now();
        assert_eq!(n.frac(), 0.0);
        n.state = Some(State {
            position: Some(30.0),
            duration: Some(120.0),
            paused: false,
            volume: 100.0,
            eof: false,
            idle: false,
        });
        assert_eq!(n.frac(), 0.25);
        assert!(!n.stalled(true), "inside grace period");
        n.started -= Duration::from_secs(6);
        assert!(n.stalled(true) && !n.stalled(false));
        n.loaded = true;
        assert!(!n.stalled(true));
    }
}
