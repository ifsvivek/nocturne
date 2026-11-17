//! Desktop integration over D-Bus: MPRIS (`org.mpris.MediaPlayer2`, media keys, playerctl) and idle
//! inhibit (`org.freedesktop.ScreenSaver`). One worker thread owns the connection, so the UI thread
//! only sends snapshots and polls commands; it never blocks on D-Bus. Every failure (no session bus,
//! no screensaver service) just means the feature is absent.
//!
//! Linux only (`dbus.rs`); elsewhere `Mpris` is a no-op (SMTC / MPNowPlayingInfoCenter come later,
//! docs/PLATFORMS.md).
use std::sync::mpsc::{Receiver, Sender, channel};

#[cfg(target_os = "linux")]
mod dbus;

/// What a remote control asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Prev,
    SeekBy(f64),
    SeekTo(f64),
}

/// Everything MPRIS reports; the title/art/state part triggers `PropertiesChanged` when it changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snap {
    pub status: &'static str,
    pub title: String,
    pub artist: String,
    pub length_us: i64,
    pub art_url: String,
    pub volume: f64,
    pub can_next: bool,
    pub can_prev: bool,
    pub pos_us: i64,
}

// Nothing reads the payloads where there is no D-Bus worker.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum Out {
    Snap(Snap),
    Inhibit(bool),
    Quit,
}

pub struct Mpris {
    out: Sender<Out>,
    pub cmds: Receiver<Cmd>,
    last: Snap,
    inhibited: bool,
}

impl Mpris {
    /// Starts the worker; the bus connection happens off-thread, so this never blocks or fails.
    pub fn start() -> Self {
        let (tx, cmds) = channel();
        let (out, rx) = channel();
        #[cfg(target_os = "linux")]
        std::thread::Builder::new()
            .name("nocturne-dbus".into())
            .spawn(move || dbus::worker(tx, rx))
            .ok();
        #[cfg(not(target_os = "linux"))]
        drop((tx, rx)); // sends below fail silently: nobody listens
        Self {
            out,
            cmds,
            last: Snap::default(),
            inhibited: false,
        }
    }

    /// Cheap: only sends when something other than the position changed (position is polled by clients).
    pub fn update(&mut self, s: Snap) {
        let pos_only = Snap {
            pos_us: self.last.pos_us,
            ..s.clone()
        } == self.last;
        let playing = s.status == "Playing";
        if !pos_only || s.pos_us.abs_diff(self.last.pos_us) > 1_000_000 {
            self.last = s.clone();
            let _ = self.out.send(Out::Snap(s));
        }
        if playing != self.inhibited {
            self.inhibited = playing;
            let _ = self.out.send(Out::Inhibit(playing));
        }
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        let _ = self.out.send(Out::Inhibit(false));
        let _ = self.out.send(Out::Quit);
    }
}
