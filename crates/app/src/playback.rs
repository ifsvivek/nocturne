//! What the now-playing bar shows. The `Player` itself lives on `Root` (UI thread only).
use crate::source::Media;
use nocturne_player::State;
use std::time::{Duration, Instant};

/// How often a playing item is reported to the server.
const REPORT_EVERY: Duration = Duration::from_secs(10);
/// A jump larger than this against the expected position counts as a seek.
const SEEK_JUMP: f64 = 3.0;

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Started,
    Progress,
}

struct Last {
    id: String,
    at: Instant,
    pos: f64,
    paused: bool,
    /// Latest position seen (reported or not), for the final "stopped" report.
    seen: f64,
}

/// Decides when a playing item is reported: once at start, every 10 s, and at once on pause,
/// resume or seek. Pure (the caller owns the clock and does the sending).
#[derive(Default)]
pub struct Reporter(Option<Last>);

impl Reporter {
    pub fn tick(&mut self, now: Instant, id: &str, pos: f64, paused: bool) -> Option<Tick> {
        let cur = Last {
            id: id.into(),
            at: now,
            pos,
            paused,
            seen: pos,
        };
        let Some(l) = self.0.as_mut().filter(|l| l.id == id) else {
            self.0 = Some(cur);
            return Some(Tick::Started);
        };
        l.seen = pos;
        let dt = now.duration_since(l.at);
        let expect = if l.paused {
            l.pos
        } else {
            l.pos + dt.as_secs_f64()
        };
        if l.paused != paused || (pos - expect).abs() > SEEK_JUMP || dt >= REPORT_EVERY {
            self.0 = Some(cur);
            return Some(Tick::Progress);
        }
        None
    }

    /// The item reported as started and its last position (seconds), forgotten.
    pub fn stop(&mut self) -> Option<(String, f64)> {
        self.0.take().map(|l| (l.id, l.seen))
    }
}

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

    #[test]
    fn reporter_start_interval_pause_seek_stop() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut r = Reporter::default();
        assert_eq!(r.tick(at(0), "a", 5.0, false), Some(Tick::Started));
        assert_eq!(r.tick(at(1), "a", 6.0, false), None);
        assert_eq!(
            r.tick(at(9), "a", 14.0, false),
            None,
            "quiet between reports"
        );
        assert_eq!(
            r.tick(at(10), "a", 15.0, false),
            Some(Tick::Progress),
            "every 10 s"
        );
        assert_eq!(
            r.tick(at(11), "a", 16.0, true),
            Some(Tick::Progress),
            "pause"
        );
        assert_eq!(r.tick(at(15), "a", 16.0, true), None, "paused and still");
        assert_eq!(
            r.tick(at(16), "a", 16.0, false),
            Some(Tick::Progress),
            "resume"
        );
        assert_eq!(
            r.tick(at(17), "a", 90.0, false),
            Some(Tick::Progress),
            "seek forward"
        );
        assert_eq!(
            r.tick(at(18), "a", 10.0, false),
            Some(Tick::Progress),
            "seek back"
        );
        assert_eq!(
            r.tick(at(19), "b", 0.0, false),
            Some(Tick::Started),
            "new item"
        );
        assert_eq!(r.tick(at(20), "b", 1.5, false), None);
        assert_eq!(
            r.stop(),
            Some(("b".into(), 1.5)),
            "stop carries the last position seen"
        );
        assert_eq!(r.stop(), None);
        assert_eq!(
            r.tick(at(21), "b", 0.0, false),
            Some(Tick::Started),
            "after stop it starts again"
        );
    }

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
