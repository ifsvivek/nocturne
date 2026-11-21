//! mpv playback control, two backends with the same API (`new/load/play/pause/toggle/stop/seek_to/
//! seek_by/set_volume/state/events`):
//! - [`EmbeddedPlayer`]: in-process libmpv (OpenGL render API) in a Wayland subsurface inside the app
//!   window. Preferred.
//! - [`Player`]: mpv as a CHILD PROCESS (own window, own GPU context) driven over its JSON IPC on a
//!   private unix socket, so a driver crash or hang in mpv can never take the app down. Fallback.
//!
//! libmpv is loaded at runtime (see [`libmpv_available`]); a missing library is an error, not a startup crash.
//!
//! No UI-framework or Jellyfin dependencies.
//!
//! Env override: `NOCTURNE_VO` picks the windowed video output (default `gpu-next`).

#[cfg(target_os = "linux")]
mod calls;
mod deps;
#[cfg(target_os = "linux")]
mod embedded;
#[cfg(not(target_os = "linux"))]
#[path = "embedded_stub.rs"]
mod embedded;
mod external;
mod ipc;
mod mpv_ffi;
mod surface;
mod vlc;
pub use deps::{install_hint, install_hint_for, libmpv_available, mpv_binary_available};
pub use embedded::{EmbeddedPlayer, colour_management_available};

static LEFT_BEHIND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A teardown gave up waiting on a stuck mpv thread and left it running: the process must end with
/// `_exit` (libmpv's and the audio driver's exit handlers could wait on the same stuck thread).
pub fn left_behind() -> bool {
    LEFT_BEHIND.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(target_os = "linux")]
pub(crate) fn mark_left_behind() {
    eprintln!("video: a stuck mpv thread was left behind at teardown");
    LEFT_BEHIND.store(true, std::sync::atomic::Ordering::SeqCst);
}
#[cfg(target_os = "linux")]
pub use embedded::{Geometry, geometry};
pub use external::Player;
pub use surface::VideoSurface;
pub use vlc::{VlcPlayer, vlc_available, vlc_url};

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mpv: {0}")]
    Mpv(String),
    #[error("VLC: {0}")]
    Vlc(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn e(err: impl std::fmt::Display) -> Error {
    Error::Mpv(err.to_string())
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// No window, no audio device (vo=null, ao=null). For tests and CI.
    pub headless: bool,
    /// Network read-ahead policy (both backends). See [`Buffering`].
    pub buffering: Buffering,
    /// Ask the compositor for an HDR output (`target-colorspace-hint=yes`): the window shows HDR
    /// content as HDR where the display path allows it (docs/HDR.md). Windowed backend only.
    pub hdr_output: bool,
    /// Start fullscreen (the HDR pop-out).
    pub fullscreen: bool,
    /// Draw into this existing window (`--wid`; a Windows HWND from [`VideoSurface`]) instead of
    /// opening mpv's own.
    pub wid: Option<isize>,
}

/// Watermark read-ahead: the demuxer reads until `high_secs` are buffered, then stops reading
/// entirely, and only resumes once buffered time drains to `low_secs` (instead of topping up every
/// few ms). Maps to mpv `demuxer-readahead-secs` = high, `demuxer-hysteresis-secs` = low. (mpv's hysteresis is the
/// absolute remaining-buffer level at which reading resumes, NOT `readahead - hysteresis`: measured
/// on mpv 0.41, see docs/buffering.md. 0 disables it, i.e. continuous top-up.)
///
/// RAM tradeoff: `max_bytes` caps the buffer regardless of seconds. A 4K remux at ~80 Mbit/s fills
/// 1.5 GiB in about 150 s, so it never reaches 300 s; a 5 Mbit/s 1080p stream needs ~190 MiB for
/// 300 s. Peak RAM is about `max_bytes + back_bytes` per player. When the byte cap is hit first, the
/// effective high mark is whatever fits in `max_bytes` (the demuxer stops at the first limit hit).
/// `back_bytes` is the already-played part kept for
/// backward seeks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Buffering {
    pub high_secs: f64,
    pub low_secs: f64,
    pub max_bytes: u64,
    pub back_bytes: u64,
}

impl Default for Buffering {
    fn default() -> Self {
        Self {
            high_secs: 300.0,
            low_secs: 120.0,
            max_bytes: 3 << 29, // 1.5 GiB
            back_bytes: 64 << 20,
        }
    }
}

impl Buffering {
    /// The exact mpv options, sanitised: non-finite -> default; high in 2..=86400 s; low in
    /// 1..high-1 (0 would disable the hysteresis); max_bytes >= 16 MiB; back_bytes <= max_bytes.
    /// `cache-secs` must be set too: with `cache=yes` mpv 0.41 uses max(readahead, cache-secs) and
    /// its default is 3600000 s, which would defeat the high mark.
    pub fn mpv_options(&self) -> [(&'static str, String); 6] {
        let d = Self::default();
        let fin = |v: f64, dv: f64| if v.is_finite() { v } else { dv };
        let high = fin(self.high_secs, d.high_secs).clamp(2.0, 86400.0);
        let low = fin(self.low_secs, d.low_secs).clamp(1.0, high - 1.0);
        let max = self.max_bytes.max(16 << 20);
        let back = self.back_bytes.min(max);
        [
            ("cache", "yes".into()),
            ("cache-secs", high.to_string()),
            ("demuxer-readahead-secs", high.to_string()),
            ("demuxer-hysteresis-secs", low.to_string()),
            ("demuxer-max-bytes", max.to_string()),
            ("demuxer-max-back-bytes", back.to_string()),
        ]
    }
}

/// Snapshot of playback state. `None` position/duration means "not known yet / nothing loaded".
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: f64,
    /// Seconds buffered ahead of the playback position (mpv `demuxer-cache-duration`); 0 if unknown.
    pub buffered: f64,
    /// Reached end of file (the player keeps the file open, paused at the end).
    pub eof: bool,
    /// Nothing loaded.
    pub idle: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            position: None,
            duration: None,
            paused: false,
            volume: 100.0,
            buffered: 0.0,
            eof: false,
            idle: true,
        }
    }
}

/// Pushed on [`Player::events`] when something changes. Position is not streamed; poll [`Player::state`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Loaded,
    Paused(bool),
    Eof,
    /// Load failed or stream errored. Message is mpv's error text only (never the URL).
    Failed(String),
    /// mpv went away (window closed, crash, kill). The next `load` respawns it.
    Closed,
    /// The track, chapter or secondary-subtitle list changed: read them again.
    Tracks,
    /// Pointer input over a picture embedded with [`Options::wid`]: `"move x y"`, `"click x y"`, `"dbl x y"`, `"right x y"` (device pixels in the picture).
    Input(String),
}

/// Parent of the per-player IPC directory: the runtime dir on Linux (`/tmp` without one), the
/// temp dir elsewhere.
pub(crate) fn runtime_base() -> PathBuf {
    if cfg!(target_os = "linux") {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/tmp".into())
    } else {
        std::env::temp_dir()
    }
}

pub(crate) fn lock(s: &Mutex<State>) -> MutexGuard<'_, State> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(b: Buffering) -> Vec<(&'static str, String)> {
        b.mpv_options().to_vec()
    }
    fn s(k: &'static str, v: &str) -> (&'static str, String) {
        (k, v.to_string())
    }

    #[test]
    fn default_maps_to_mpv_options() {
        assert_eq!(
            get(Buffering::default()),
            [
                s("cache", "yes"),
                s("cache-secs", "300"),
                s("demuxer-readahead-secs", "300"),
                s("demuxer-hysteresis-secs", "120"), // = low mark
                s("demuxer-max-bytes", "1610612736"),
                s("demuxer-max-back-bytes", "67108864"),
            ]
        );
    }

    #[test]
    fn clamping() {
        let d = Buffering::default();
        let val = |b: Buffering, k: &str| get(b).into_iter().find(|o| o.0 == k).unwrap().1;
        // low >= high -> low = high - 1
        let b = Buffering {
            high_secs: 30.0,
            low_secs: 90.0,
            ..d
        };
        assert_eq!(val(b, "demuxer-hysteresis-secs"), "29");
        // high < 2 -> 2, low < 1 -> 1 (0 would disable hysteresis)
        let b = Buffering {
            high_secs: 0.0,
            low_secs: -5.0,
            ..d
        };
        assert_eq!(val(b, "demuxer-readahead-secs"), "2");
        assert_eq!(val(b, "demuxer-hysteresis-secs"), "1");
        // NaN -> defaults
        let b = Buffering {
            high_secs: f64::NAN,
            low_secs: f64::INFINITY,
            ..d
        };
        assert_eq!(val(b, "demuxer-hysteresis-secs"), "120");
        // byte caps: floor 16 MiB, back <= max
        let b = Buffering {
            max_bytes: 0,
            back_bytes: u64::MAX,
            ..d
        };
        assert_eq!(val(b, "demuxer-max-bytes"), "16777216");
        assert_eq!(val(b, "demuxer-max-back-bytes"), "16777216");
    }
}
