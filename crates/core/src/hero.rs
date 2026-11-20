//! The home hero carousel: which titles become slides and when the banner moves on. Pure and
//! time-injected (the root tick passes `Instant::now()`), so it is tested without a window.

use crate::source::{Kind, Media, SourceId};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Where slides come from, in the order the user's home layout lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroSource {
    ContinueWatching,
    NextUp,
    LatestMovies,
    LatestShows,
    Favourites,
}

pub const DEFAULT_ORDER: [HeroSource; 5] = [
    HeroSource::ContinueWatching,
    HeroSource::NextUp,
    HeroSource::LatestMovies,
    HeroSource::LatestShows,
    HeroSource::Favourites,
];

pub const PER_SOURCE: usize = 3;
pub const TOTAL_CAP: usize = 8;

/// Identity of a slide for de-duplication: an episode stands for its show, so "next up" and
/// "continue watching" for one show make one slide.
fn key(m: &Media) -> (SourceId, String) {
    match (m.kind, &m.series_id) {
        (Kind::Episode | Kind::Season, Some(s)) => (m.origin, s.clone()),
        _ => (m.origin, m.id.clone()),
    }
}

/// Slides from the fetched lists: sources in `order`, at most `per_source` of each, the same
/// title only once, `cap` in total. Lists missing from `lists` are skipped.
pub fn collect(
    order: &[HeroSource],
    lists: &[(HeroSource, Vec<Media>)],
    per_source: usize,
    cap: usize,
) -> Vec<Media> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    for src in order {
        let Some((_, items)) = lists.iter().find(|(s, _)| s == src) else {
            continue;
        };
        let mut taken = 0;
        for m in items {
            if out.len() >= cap || taken >= per_source {
                break;
            }
            if seen.insert(key(m)) {
                out.push(m.clone());
                taken += 1;
            }
        }
    }
    out
}

/// Reasons the banner must hold still. Any one set pauses auto-advance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pauses {
    /// Window minimised, on another workspace or unfocused.
    pub window_inactive: bool,
    pub pointer_down: bool,
    /// The slide's description is expanded.
    pub description_open: bool,
    /// Keyboard focus is on a control inside the banner.
    pub focus_inside: bool,
    /// A dialog or menu is open.
    pub dialog: bool,
    pub playback: bool,
    /// The Theme Studio is open: the slide must not change under the element being edited.
    pub studio: bool,
}

impl Pauses {
    pub fn any(&self) -> bool {
        let Self {
            window_inactive,
            pointer_down,
            description_open,
            focus_inside,
            dialog,
            playback,
            studio,
        } = *self;
        window_inactive
            || pointer_down
            || description_open
            || focus_inside
            || dialog
            || playback
            || studio
    }
}

#[derive(Clone, Debug)]
pub struct Hero {
    len: usize,
    index: usize,
    interval: Duration,
    autoplay: bool,
    pub pauses: Pauses,
    /// Time shown on the current slide, frozen while paused.
    shown: Duration,
    last: Option<Instant>,
}

impl Hero {
    /// `autoplay` should already be false when reduce-motion applies.
    pub fn new(len: usize, interval_secs: u32, autoplay: bool) -> Self {
        Self {
            len,
            index: 0,
            interval: Duration::from_secs(u64::from(interval_secs.max(1))),
            autoplay,
            pauses: Pauses::default(),
            shown: Duration::ZERO,
            last: None,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn set_autoplay(&mut self, autoplay: bool, interval_secs: u32) {
        self.autoplay = autoplay;
        self.interval = Duration::from_secs(u64::from(interval_secs.max(1)));
    }

    /// The slide list changed (a late response): keep the position when it still exists.
    pub fn set_len(&mut self, len: usize) {
        self.len = len;
        if self.index >= len {
            self.index = 0;
            self.shown = Duration::ZERO;
        }
    }

    /// Will `tick` ever move on? Lets the root tick skip the hero entirely when it cannot.
    pub fn running(&self) -> bool {
        self.autoplay && self.len > 1 && !self.pauses.any()
    }

    /// 0..1 through the current slide, for a progress hairline.
    pub fn progress(&self) -> f32 {
        (self.shown.as_secs_f32() / self.interval.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// How long until the slide changes, if nothing pauses it (at least 100 ms): when an idle app needs to wake next.
    pub fn next_in(&self, now: Instant) -> Duration {
        let since = self
            .last
            .map_or(Duration::ZERO, |l| now.saturating_duration_since(l));
        self.interval
            .saturating_sub(self.shown + since)
            .max(Duration::from_millis(100))
    }

    /// Advance the clock to `now`; true when the slide changed. Paused time does not count.
    pub fn tick(&mut self, now: Instant) -> bool {
        let dt = self
            .last
            .map_or(Duration::ZERO, |l| now.saturating_duration_since(l));
        if !self.running() {
            // Ticks stop while the carousel is paused: the first one after resuming must not count the gap.
            self.last = None;
            return false;
        }
        self.last = Some(now);
        self.shown += dt;
        if self.shown >= self.interval {
            self.step(1);
            return true;
        }
        false
    }

    fn step(&mut self, d: isize) {
        if self.len > 0 {
            self.index = (self.index as isize + d).rem_euclid(self.len as isize) as usize;
        }
        self.shown = Duration::ZERO;
    }

    /// Manual next (wraps) and restart of the timer.
    pub fn next(&mut self) {
        self.step(1);
    }

    pub fn prev(&mut self) {
        self.step(-1);
    }

    /// Jump to a dot; out of range is ignored.
    pub fn goto(&mut self, i: usize) {
        if i < self.len {
            self.index = i;
            self.shown = Duration::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, kind: Kind, series: Option<&str>) -> Media {
        let mut m = Media::from_item(&Default::default());
        m.id = id.into();
        m.kind = kind;
        m.series_id = series.map(Into::into);
        m
    }

    fn ids(v: &[Media]) -> Vec<&str> {
        v.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn collect_orders_dedups_and_caps() {
        let lists = vec![
            (
                HeroSource::ContinueWatching,
                vec![
                    m("e1", Kind::Episode, Some("s1")),
                    m("a", Kind::Movie, None),
                ],
            ),
            (
                HeroSource::NextUp,
                vec![
                    m("e2", Kind::Episode, Some("s1")),
                    m("e3", Kind::Episode, Some("s2")),
                ],
            ),
            (
                HeroSource::LatestMovies,
                vec![
                    m("a", Kind::Movie, None),
                    m("b", Kind::Movie, None),
                    m("c", Kind::Movie, None),
                ],
            ),
        ];
        let got = collect(&DEFAULT_ORDER, &lists, 2, 10);
        // e2 is the same show as e1, a repeats, per-source cap 2 limits the movies to b and c.
        assert_eq!(ids(&got), ["e1", "a", "e3", "b", "c"]);
        assert_eq!(
            ids(&collect(&DEFAULT_ORDER, &lists, 2, 3)),
            ["e1", "a", "e3"]
        );
        // A different order puts movies first; absent sources are skipped.
        let got = collect(
            &[HeroSource::LatestMovies, HeroSource::Favourites],
            &lists,
            1,
            10,
        );
        assert_eq!(ids(&got), ["a"]);
        // The same id on two servers is two slides.
        let mut other = m("a", Kind::Movie, None);
        other.origin = SourceId(2);
        let lists = vec![(
            HeroSource::LatestMovies,
            vec![m("a", Kind::Movie, None), other],
        )];
        assert_eq!(collect(&DEFAULT_ORDER, &lists, 3, 10).len(), 2);
    }

    fn secs(t0: Instant, s: u64) -> Instant {
        t0 + Duration::from_secs(s)
    }

    #[test]
    fn advances_on_the_interval_and_wraps() {
        let t0 = Instant::now();
        let mut h = Hero::new(3, 10, true);
        assert!(!h.tick(t0));
        assert!(!h.tick(secs(t0, 9)));
        assert!(h.progress() > 0.8);
        assert!(h.tick(secs(t0, 19)));
        assert_eq!(h.index(), 1);
        assert!(h.tick(secs(t0, 29)) && h.tick(secs(t0, 39)));
        assert_eq!(h.index(), 0, "wrapped");
    }

    #[test]
    fn pauses_freeze_the_clock_without_losing_progress() {
        let t0 = Instant::now();
        let mut h = Hero::new(3, 10, true);
        h.tick(t0);
        h.tick(secs(t0, 6));
        h.pauses.pointer_down = true;
        assert!(!h.tick(secs(t0, 100)), "paused time does not count");
        h.pauses.pointer_down = false;
        assert!(
            !h.tick(secs(t0, 103)),
            "the first tick after a pause only restarts the clock"
        );
        assert!(h.tick(secs(t0, 107)), "4 s were left of the 10");
        let setters: [fn(&mut Pauses); 6] = [
            |p| p.window_inactive = true,
            |p| p.description_open = true,
            |p| p.focus_inside = true,
            |p| p.dialog = true,
            |p| p.playback = true,
            |p| p.studio = true,
        ];
        for set in setters {
            let mut h = Hero::new(3, 10, true);
            set(&mut h.pauses);
            h.tick(t0);
            assert!(!h.tick(secs(t0, 50)) && !h.running());
        }
    }

    #[test]
    fn next_in_says_when_an_idle_app_must_wake() {
        let t0 = Instant::now();
        let mut h = Hero::new(3, 10, true);
        h.tick(t0);
        assert_eq!(h.next_in(t0), Duration::from_secs(10));
        h.tick(secs(t0, 4));
        assert_eq!(h.next_in(secs(t0, 4)), Duration::from_secs(6));
        assert_eq!(
            h.next_in(secs(t0, 7)),
            Duration::from_secs(3),
            "time since the last tick counts"
        );
        assert_eq!(
            h.next_in(secs(t0, 60)),
            Duration::from_millis(100),
            "never a busy loop"
        );
    }

    #[test]
    fn manual_moves_wrap_and_restart_the_timer() {
        let t0 = Instant::now();
        let mut h = Hero::new(3, 10, true);
        h.tick(t0);
        h.tick(secs(t0, 8));
        h.prev();
        assert_eq!(h.index(), 2, "wraps backwards");
        assert!(!h.tick(secs(t0, 12)), "timer restarted");
        h.next();
        assert_eq!(h.index(), 0);
        h.goto(1);
        h.goto(9);
        assert_eq!(h.index(), 1);
    }

    #[test]
    fn no_autoplay_when_off_reduced_or_single() {
        let t0 = Instant::now();
        for mut h in [
            Hero::new(3, 10, false),
            Hero::new(1, 10, true),
            Hero::new(0, 10, true),
        ] {
            h.tick(t0);
            assert!(!h.tick(secs(t0, 60)) && !h.running());
        }
        let mut h = Hero::new(3, 10, false);
        h.next();
        assert_eq!(h.index(), 1, "manual still works");
        h.set_len(1);
        assert_eq!(h.index(), 0);
    }
}
