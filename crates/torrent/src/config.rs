//! Engine configuration.

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Engine settings. `Engine::start` takes them once; limits can change later with `Engine::set_limits`.
#[derive(Debug, Clone)]
pub struct Config {
    /// Engine state: librqbit's session and fast-resume data plus this crate's own torrent list. Must survive restarts.
    pub data_dir: PathBuf,
    /// Default save path and the root every save path must stay inside.
    pub download_dir: PathBuf,
    /// Incoming peer port: the first free one in the range is used. `0..=0` lets the OS pick (tests).
    pub listen_ports: RangeInclusive<u16>,
    /// Bind every socket to this interface (a VPN's `wg0`, `tun0`). Traffic never falls back to another route: if the interface is missing at start, `Engine::start` fails; if it disappears later, every torrent is paused until it is back.
    pub bind_interface: Option<String>,
    /// Mainline DHT. Off means peers come only from trackers, LSD and explicitly given peers.
    pub dht: bool,
    /// Local service discovery (multicast on the LAN).
    pub lsd: bool,
    /// UPnP port forwarding on the router. Off by default.
    pub upnp: bool,
    /// Global rate limits.
    pub limits: Limits,
    /// Optional slower limits on a weekly schedule (local time).
    pub alt_speed: Option<AltSpeed>,
    /// Downloads running at once; the rest wait as `Queued`. `None` = unlimited.
    pub max_active_downloads: Option<usize>,
    /// Finished torrents seeding at once; the rest wait as `Queued`. `None` = unlimited.
    pub max_active_seeds: Option<usize>,
    /// Default seeding goal; `AddOptions::seed_goal` overrides it per torrent.
    pub seed_goal: SeedGoal,
}

impl Config {
    /// Sensible defaults: DHT and LSD on, UPnP off, ports 6881-6889, no limits, no queue limits, seed forever.
    pub fn new(data_dir: impl Into<PathBuf>, download_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            download_dir: download_dir.into(),
            listen_ports: 6881..=6889,
            bind_interface: None,
            dht: true,
            lsd: true,
            upnp: false,
            limits: Limits::default(),
            alt_speed: None,
            max_active_downloads: None,
            max_active_seeds: None,
            seed_goal: SeedGoal::default(),
        }
    }
}

/// Rate limits in bytes per second; `None` (or 0) = unlimited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub download_bps: Option<u32>,
    pub upload_bps: Option<u32>,
}

/// When to stop seeding. Both unset = seed forever. Either reached = goal met.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SeedGoal {
    /// Uploaded bytes divided by downloaded (have) bytes.
    pub ratio: Option<f64>,
    /// Time spent seeding (counted only while the torrent is actually seeding).
    pub seeding_time: Option<Duration>,
    pub action: GoalAction,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalAction {
    /// Stop seeding; the torrent stays listed as `Completed`.
    #[default]
    Pause,
    /// Remove the torrent from the engine and keep the downloaded files.
    Remove,
}

impl SeedGoal {
    pub(crate) fn reached(&self, ratio: f64, seeding: Duration) -> bool {
        self.ratio.is_some_and(|r| ratio >= r) || self.seeding_time.is_some_and(|t| seeding >= t)
    }
}

/// Alternative limits used while the schedule is active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AltSpeed {
    pub limits: Limits,
    pub schedule: Schedule,
}

/// A daily time window on chosen weekdays, local time. `from > to` wraps past midnight (22:00 to 06:00); the part after midnight belongs to the day the window started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    /// Minutes after local midnight, 0..1440.
    pub from_minute: u16,
    pub to_minute: u16,
    /// Monday first.
    pub days: [bool; 7],
}

impl Schedule {
    /// `weekday`: 0 = Monday. `minute`: minutes after midnight.
    pub fn is_active(&self, weekday: usize, minute: u16) -> bool {
        let (from, to) = (self.from_minute, self.to_minute);
        if from <= to {
            self.days[weekday % 7] && (from..to).contains(&minute)
        } else {
            (self.days[weekday % 7] && minute >= from)
                || (self.days[(weekday + 6) % 7] && minute < to)
        }
    }

    pub(crate) fn is_active_now(&self) -> bool {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        let minute = (now.hour() * 60 + now.minute()) as u16;
        self.is_active(now.weekday().num_days_from_monday() as usize, minute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_same_day_window() {
        let s = Schedule {
            from_minute: 9 * 60,
            to_minute: 17 * 60,
            days: [true, true, true, true, true, false, false],
        };
        assert!(s.is_active(0, 9 * 60));
        assert!(!s.is_active(0, 17 * 60));
        assert!(!s.is_active(5, 12 * 60), "Saturday is off");
    }

    #[test]
    fn schedule_wraps_midnight_on_start_day() {
        // Friday night only.
        let mut days = [false; 7];
        days[4] = true;
        let s = Schedule {
            from_minute: 22 * 60,
            to_minute: 6 * 60,
            days,
        };
        assert!(s.is_active(4, 23 * 60));
        assert!(
            s.is_active(5, 60),
            "Saturday 01:00 belongs to Friday's window"
        );
        assert!(
            !s.is_active(4, 60),
            "Friday 01:00 belongs to Thursday's window"
        );
        assert!(!s.is_active(5, 23 * 60));
    }

    #[test]
    fn seed_goal_either_limit() {
        let g = SeedGoal {
            ratio: Some(1.0),
            seeding_time: Some(Duration::from_secs(60)),
            action: GoalAction::Pause,
        };
        assert!(!g.reached(0.5, Duration::from_secs(10)));
        assert!(g.reached(1.0, Duration::ZERO));
        assert!(g.reached(0.0, Duration::from_secs(60)));
        assert!(!SeedGoal::default().reached(100.0, Duration::from_secs(1 << 30)));
    }
}
