//! Thin libmpv wrapper. mpv owns its own native window; the UI drives it through [`Player`].
//! No GPUI or Jellyfin dependencies.

use libmpv2::{Format, Mpv, events::Event as MpvEvent};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mpv: {0}")]
    Mpv(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn e(err: libmpv2::Error) -> Error {
    Error::Mpv(err.to_string())
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// No window, no audio device (vo=null, ao=null). For tests and CI.
    pub headless: bool,
}

/// Snapshot of playback state. `None` position/duration means "not known yet / nothing loaded".
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: f64,
    /// Reached end of file (the player keeps the file open, paused at the end).
    pub eof: bool,
    /// Nothing loaded.
    pub idle: bool,
}

/// Pushed on [`Player::events`] when something changes. Position is not streamed; poll [`Player::state`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Loaded,
    Paused(bool),
    Eof,
    /// Load failed or stream errored. Message is mpv's error text only (never the URL).
    Failed(String),
    /// mpv core shut down (e.g. the user closed the window).
    Closed,
}

pub struct Player {
    mpv: Mpv,
    rx: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
}

impl Player {
    pub fn new(opts: Options) -> Result<Self> {
        let mpv = Mpv::with_initializer(|i| {
            if opts.headless {
                i.set_option("vo", "null")?;
                i.set_option("ao", "null")?;
                i.set_option("force-window", "no")?;
            } else {
                // KNOWN ISSUE: in-process GPU output (gpu/gpu-next) corrupts GPUI's Vulkan state
                // on NVIDIA when mpv shuts down (SIGSEGV in the host app). Software output is
                // the safe default until playback moves out of process or is embedded.
                // `NOCTURNE_VO=gpu-next` opts back in.
                let vo = std::env::var("NOCTURNE_VO").unwrap_or_else(|_| "wlshm".into());
                i.set_option("vo", vo.as_str())?;
                i.set_option("hwdec", "auto-safe")?;
                i.set_option("force-window", "yes")?;
            }
            i.set_option("keep-open", "yes")?;
            Ok(())
        })
        .map_err(e)?;

        let client = mpv.create_client(Some("nocturne_events")).map_err(e)?;
        client
            .observe_property("pause", Format::Flag, 1)
            .map_err(e)?;
        client
            .observe_property("eof-reached", Format::Flag, 2)
            .map_err(e)?;
        let (tx, rx) = channel();
        let worker = std::thread::spawn(move || pump(client, tx));
        Ok(Self {
            mpv,
            rx,
            worker: Some(worker),
        })
    }

    /// Non-blocking event source: `try_recv` / `try_iter`.
    pub fn events(&self) -> &Receiver<Event> {
        &self.rx
    }

    /// Replace the current file. `headers` is a full header line (e.g. `Authorization: ...`)
    /// sent as `http-header-fields`, so credentials never go in the URL.
    pub fn load(&self, url: &str, headers: Option<&str>) -> Result<()> {
        self.mpv
            .command("change-list", &["http-header-fields", "clr", ""])
            .map_err(e)?;
        if let Some(h) = headers {
            // `append` adds one list item verbatim (no splitting on ',').
            self.mpv
                .command("change-list", &["http-header-fields", "append", h])
                .map_err(e)?;
        }
        self.mpv.command("loadfile", &[url, "replace"]).map_err(e)
    }

    pub fn play(&self) -> Result<()> {
        self.set_paused(false)
    }

    pub fn pause(&self) -> Result<()> {
        self.set_paused(true)
    }

    pub fn toggle(&self) -> Result<()> {
        self.set_paused(!self.state().paused)
    }

    pub fn seek_to(&self, secs: f64) -> Result<()> {
        self.mpv
            .command("seek", &[&secs.to_string(), "absolute"])
            .map_err(e)
    }

    pub fn seek_by(&self, secs: f64) -> Result<()> {
        self.mpv
            .command("seek", &[&secs.to_string(), "relative"])
            .map_err(e)
    }

    /// 0..=130 (mpv's range); values outside are clamped.
    pub fn set_volume(&self, vol: f64) -> Result<()> {
        self.mpv
            .set_property("volume", vol.clamp(0.0, 130.0))
            .map_err(e)
    }

    pub fn stop(&self) -> Result<()> {
        self.mpv.command("stop", &[]).map_err(e)
    }

    pub fn state(&self) -> State {
        let f = |n| self.mpv.get_property::<f64>(n).ok();
        let b = |n| self.mpv.get_property::<bool>(n).unwrap_or(false);
        State {
            position: f("time-pos"),
            duration: f("duration"),
            paused: b("pause"),
            volume: f("volume").unwrap_or(100.0),
            eof: b("eof-reached"),
            idle: b("idle-active"),
        }
    }

    fn set_paused(&self, p: bool) -> Result<()> {
        self.mpv.set_property("pause", p).map_err(e)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.mpv.command("quit", &[]);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

fn pump(client: Mpv, tx: Sender<Event>) {
    use libmpv2::events::PropertyData::Flag;
    loop {
        let ev = match client.wait_event(1.0) {
            None => continue,
            Some(Ok(MpvEvent::Shutdown)) => {
                let _ = tx.send(Event::Closed);
                return;
            }
            Some(Ok(MpvEvent::FileLoaded)) => Event::Loaded,
            Some(Ok(MpvEvent::PropertyChange {
                name: "pause",
                change: Flag(p),
                ..
            })) => Event::Paused(p),
            Some(Ok(MpvEvent::PropertyChange {
                name: "eof-reached",
                change: Flag(true),
                ..
            })) => Event::Eof,
            Some(Err(err)) => Event::Failed(err.to_string()),
            Some(Ok(_)) => continue,
        };
        if tx.send(ev).is_err() {
            return; // receiver gone; Drop's quit still ends the core
        }
    }
}
