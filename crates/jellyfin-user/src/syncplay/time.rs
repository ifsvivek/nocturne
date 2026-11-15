//! Clock offset between this machine and the server, from `GET /GetUtcTime` round trips.
//! Same estimator as the official web client: keep the last 8 samples and use the one with the
//! smallest network delay (the offset error is bounded by half the delay). Pure; callers supply
//! the timestamps.
use std::collections::VecDeque;

/// How many samples are kept.
pub const TRACKED: usize = 8;

/// One round trip. All values are epoch milliseconds: `sent`/`received` on the local clock,
/// `server_received`/`server_sent` on the server clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub sent: f64,
    pub server_received: f64,
    pub server_sent: f64,
    pub received: f64,
}

impl Sample {
    /// Server clock minus local clock.
    pub fn offset(&self) -> f64 {
        ((self.server_received - self.sent) + (self.server_sent - self.received)) / 2.0
    }
    /// Network time of the round trip (server processing time excluded).
    pub fn delay(&self) -> f64 {
        (self.received - self.sent) - (self.server_sent - self.server_received)
    }
}

#[derive(Debug, Clone, Default)]
pub struct TimeSync {
    samples: VecDeque<Sample>,
}

impl TimeSync {
    pub fn add(&mut self, s: Sample) {
        if !(s.offset().is_finite() && s.delay().is_finite()) {
            return;
        }
        self.samples.push_back(s);
        if self.samples.len() > TRACKED {
            self.samples.pop_front();
        }
    }

    pub fn ready(&self) -> bool {
        !self.samples.is_empty()
    }

    pub fn reset(&mut self) {
        self.samples.clear();
    }

    fn best(&self) -> Option<&Sample> {
        self.samples
            .iter()
            .min_by(|a, b| a.delay().total_cmp(&b.delay()))
    }

    /// Server minus local, ms (0 until the first sample).
    pub fn offset(&self) -> f64 {
        self.best().map_or(0.0, Sample::offset)
    }

    /// One-way latency estimate, ms: what clients report to the server with `Ping`.
    pub fn ping(&self) -> i64 {
        self.best().map_or(0, |s| (s.delay() / 2.0).round() as i64)
    }

    pub fn server_to_local(&self, server_ms: f64) -> f64 {
        server_ms - self.offset()
    }

    pub fn local_to_server(&self, local_ms: f64) -> f64 {
        local_ms + self.offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trip where the server clock is `off` ms ahead, with `up`/`down` ms of latency each way.
    fn trip(sent: f64, off: f64, up: f64, down: f64) -> Sample {
        Sample {
            sent,
            server_received: sent + up + off,
            server_sent: sent + up + off + 1.0,
            received: sent + up + 1.0 + down,
        }
    }

    #[test]
    fn symmetric_trip_gives_the_exact_offset() {
        let s = trip(1000.0, 250.0, 20.0, 20.0);
        assert_eq!(s.offset(), 250.0);
        assert_eq!(s.delay(), 40.0);
    }

    #[test]
    fn asymmetry_error_is_bounded_by_half_the_delay() {
        let s = trip(0.0, 100.0, 5.0, 45.0);
        assert!((s.offset() - 100.0).abs() <= s.delay() / 2.0);
    }

    #[test]
    fn picks_the_lowest_delay_sample() {
        let mut t = TimeSync::default();
        assert!(!t.ready());
        assert_eq!(t.offset(), 0.0);
        t.add(trip(0.0, 100.0, 80.0, 10.0)); // skewed, slow
        t.add(trip(1000.0, 100.0, 4.0, 4.0)); // best
        t.add(trip(2000.0, 100.0, 30.0, 60.0));
        assert!(t.ready());
        assert_eq!(t.offset(), 100.0);
        assert_eq!(t.ping(), 4);
        assert_eq!(t.server_to_local(5100.0), 5000.0);
        assert_eq!(t.local_to_server(5000.0), 5100.0);
    }

    #[test]
    fn old_samples_age_out() {
        let mut t = TimeSync::default();
        t.add(trip(0.0, 500.0, 1.0, 1.0)); // great sample, but stale after 8 more
        for i in 0..TRACKED {
            t.add(trip(i as f64 * 1000.0, 100.0, 20.0, 20.0));
        }
        assert_eq!(t.offset(), 100.0);
        t.reset();
        assert!(!t.ready());
    }

    #[test]
    fn nonsense_is_ignored() {
        let mut t = TimeSync::default();
        t.add(Sample {
            sent: f64::NAN,
            server_received: 0.0,
            server_sent: 0.0,
            received: 0.0,
        });
        assert!(!t.ready());
    }
}
