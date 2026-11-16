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

mod deps;
mod embedded;
mod mpv_ffi;
pub use deps::{install_hint, install_hint_for, libmpv_available, mpv_binary_available};
pub use embedded::{EmbeddedPlayer, Geometry, geometry};

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mpv: {0}")]
    Mpv(String),
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
}

struct Conn {
    child: Child,
    writer: UnixStream,
    alive: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

pub struct Player {
    opts: Options,
    dir: PathBuf,
    state: Arc<Mutex<State>>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    conn: Mutex<Option<Conn>>,
    req: AtomicU64,
    /// Replies awaited by `prop`, keyed by request id (filled by the reader thread).
    pending: Pending,
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Value>>>>;

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl Player {
    pub fn new(opts: Options) -> Result<Self> {
        // Private 0700 dir holding the IPC socket (pid + counter in the name).
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/tmp".into());
        let dir = base.join(format!(
            "nocturne-mpv-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(e)?;
        let (tx, rx) = channel();
        let p = Self {
            opts,
            dir,
            state: Arc::default(),
            tx,
            rx,
            conn: Mutex::new(None),
            req: AtomicU64::new(1),
            pending: Arc::default(),
        };
        *p.conn.lock().unwrap() = Some(p.spawn()?);
        Ok(p)
    }

    /// Non-blocking event source: `try_recv` / `try_iter`.
    pub fn events(&self) -> &Receiver<Event> {
        &self.rx
    }

    /// Replace the current file. `headers` is a full header line (e.g. `Authorization: ...`)
    /// sent over IPC as `http-header-fields`, so credentials never reach the command line or URL.
    /// Respawns mpv if it died.
    pub fn load(&self, url: &str, headers: Option<&str>) -> Result<()> {
        let mut cmds = vec![json!(["change-list", "http-header-fields", "clr", ""])];
        if let Some(h) = headers {
            // `append` adds one list item verbatim (no splitting on ',').
            cmds.push(json!(["change-list", "http-header-fields", "append", h]));
        }
        cmds.push(json!(["loadfile", url, "replace"]));
        let mut g = self.conn.lock().unwrap();
        if !g.as_ref().is_some_and(|c| c.alive.load(Ordering::SeqCst)) {
            self.respawn(&mut g)?;
        }
        if self.send_all(g.as_mut().unwrap(), &cmds).is_err() {
            // Died between the liveness check and the write: one retry on a fresh child.
            self.respawn(&mut g)?;
            self.send_all(g.as_mut().unwrap(), &cmds).map_err(e)?;
        }
        Ok(())
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
        self.cmd(json!(["seek", secs, "absolute"]))
    }

    pub fn seek_by(&self, secs: f64) -> Result<()> {
        self.cmd(json!(["seek", secs, "relative"]))
    }

    /// 0..=130 (mpv's range); values outside are clamped.
    pub fn set_volume(&self, vol: f64) -> Result<()> {
        self.cmd(json!(["set_property", "volume", vol.clamp(0.0, 130.0)]))
    }

    pub fn stop(&self) -> Result<()> {
        self.cmd(json!(["stop"]))
    }

    /// Any mpv command, e.g. `command(&["set", "speed", "1.5"])`.
    pub fn command(&self, args: &[&str]) -> Result<()> {
        self.cmd(json!(args))
    }

    /// Reads one mpv property (JSON). `None` when unavailable or mpv is gone. Blocks up to 300 ms.
    pub fn prop(&self, name: &str) -> Option<Value> {
        let id = self.req.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        let line = json!({"command": ["get_property", name], "request_id": id}).to_string() + "\n";
        let sent = match self.conn.lock().unwrap().as_mut() {
            Some(c) => c.writer.write_all(line.as_bytes()).is_ok(),
            None => false,
        };
        let v = sent
            .then(|| rx.recv_timeout(Duration::from_millis(300)).ok())
            .flatten();
        self.pending.lock().unwrap().remove(&id);
        v.filter(|v| !v.is_null())
    }

    /// Cheap, non-blocking: a snapshot maintained by the IPC reader thread.
    pub fn state(&self) -> State {
        lock(&self.state).clone()
    }

    /// Child mpv pid (tests only).
    #[doc(hidden)]
    pub fn child_pid(&self) -> Option<u32> {
        self.conn.lock().unwrap().as_ref().map(|c| c.child.id())
    }

    fn set_paused(&self, p: bool) -> Result<()> {
        self.cmd(json!(["set_property", "pause", p]))
    }

    fn cmd(&self, c: Value) -> Result<()> {
        let mut g = self.conn.lock().unwrap();
        match g.as_mut() {
            Some(conn) => self.send_all(conn, &[c]).map_err(e),
            None => Err(Error::Mpv("not running".into())),
        }
    }

    fn send_all(&self, conn: &mut Conn, cmds: &[Value]) -> std::io::Result<()> {
        for c in cmds {
            let id = self.req.fetch_add(1, Ordering::Relaxed);
            let line = json!({"command": c, "request_id": id}).to_string() + "\n";
            conn.writer.write_all(line.as_bytes())?;
        }
        Ok(())
    }

    fn respawn(&self, g: &mut MutexGuard<Option<Conn>>) -> Result<()> {
        if let Some(old) = g.take() {
            reap(old);
        }
        **g = Some(self.spawn()?);
        Ok(())
    }

    fn spawn(&self) -> Result<Conn> {
        let sock = self.dir.join("mpv.sock");
        let _ = std::fs::remove_file(&sock);
        let mut cmd = Command::new("mpv");
        for (k, v) in self.opts.buffering.mpv_options() {
            cmd.arg(format!("--{k}={v}"));
        }
        cmd.args([
            "--no-config",
            "--no-terminal",
            "--idle=yes",
            "--keep-open=yes",
            "--title=NOCTURNE",
        ]);
        if self.opts.headless {
            cmd.args(["--vo=null", "--ao=null", "--force-window=no"]);
        } else {
            let vo = std::env::var("NOCTURNE_VO").unwrap_or_else(|_| "gpu-next".into());
            cmd.args([format!("--vo={vo}"), "--hwdec=auto-safe".into()])
                .arg("--force-window=yes");
        }
        let mut child = cmd
            .arg(format!("--volume={}", lock(&self.state).volume))
            .arg(format!("--input-ipc-server={}", sock.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(e)?;

        let t = Instant::now();
        let writer = loop {
            match UnixStream::connect(&sock) {
                Ok(s) => break s,
                Err(err) => {
                    let dead = child.try_wait().ok().flatten().is_some();
                    if dead || t.elapsed() > Duration::from_secs(10) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(e(format!("ipc connect: {err}")));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        let _ = writer.set_write_timeout(Some(Duration::from_secs(1)));
        *lock(&self.state) = State::default();

        let alive = Arc::new(AtomicBool::new(true));
        let rd = writer.try_clone().map_err(e)?;
        let (st, tx, al, pe) = (
            self.state.clone(),
            self.tx.clone(),
            alive.clone(),
            self.pending.clone(),
        );
        let reader = Some(std::thread::spawn(move || read_loop(rd, st, tx, al, pe)));
        let mut conn = Conn {
            child,
            writer,
            alive,
            reader,
        };
        let obs: Vec<Value> = [
            "pause",
            "time-pos",
            "duration",
            "volume",
            "eof-reached",
            "idle-active",
            "demuxer-cache-duration",
        ]
        .iter()
        .enumerate()
        .map(|(i, n)| json!(["observe_property", i + 1, n]))
        .collect();
        self.send_all(&mut conn, &obs).map_err(e)?;
        Ok(conn)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(c) = self.conn.get_mut().unwrap().take() {
            reap(c);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub(crate) fn lock(s: &Mutex<State>) -> MutexGuard<'_, State> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

/// Ask mpv to quit, wait briefly, then kill; join the reader.
fn reap(mut c: Conn) {
    let _ = c.writer.write_all(b"{\"command\":[\"quit\"]}\n");
    let t = Instant::now();
    while c.child.try_wait().ok().flatten().is_none() {
        if t.elapsed() > Duration::from_secs(2) {
            let _ = c.child.kill();
            let _ = c.child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = c.writer.shutdown(std::net::Shutdown::Both);
    if let Some(r) = c.reader.take() {
        let _ = r.join();
    }
}

fn read_loop(
    sock: UnixStream,
    state: Arc<Mutex<State>>,
    tx: Sender<Event>,
    alive: Arc<AtomicBool>,
    pending: Pending,
) {
    for line in BufReader::new(sock).lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(ev) = v["event"].as_str() else {
            // Command reply: hand `data` to a waiting `prop` call.
            if let Some(tx) = v["request_id"]
                .as_u64()
                .and_then(|id| pending.lock().unwrap().remove(&id))
            {
                let _ = tx.send(if v["error"] == "success" {
                    v["data"].clone()
                } else {
                    Value::Null
                });
            }
            continue;
        };
        let out = match ev {
            "file-loaded" => Some(Event::Loaded),
            "shutdown" => break,
            "end-file" if v["reason"] == "error" => Some(Event::Failed(
                v["file_error"].as_str().unwrap_or("playback failed").into(),
            )),
            "property-change" => {
                let d = &v["data"];
                let mut s = lock(&state);
                match v["name"].as_str().unwrap_or("") {
                    "pause" => {
                        let p = d.as_bool().unwrap_or(false);
                        let changed = std::mem::replace(&mut s.paused, p) != p;
                        changed.then_some(Event::Paused(p))
                    }
                    "eof-reached" => {
                        let b = d.as_bool().unwrap_or(false);
                        let rose = b && !s.eof;
                        s.eof = b;
                        rose.then_some(Event::Eof)
                    }
                    "idle-active" => {
                        s.idle = d.as_bool().unwrap_or(true);
                        None
                    }
                    "time-pos" => {
                        s.position = d.as_f64();
                        None
                    }
                    "duration" => {
                        s.duration = d.as_f64();
                        None
                    }
                    "volume" => {
                        s.volume = d.as_f64().unwrap_or(100.0);
                        None
                    }
                    "demuxer-cache-duration" => {
                        s.buffered = d.as_f64().unwrap_or(0.0);
                        None
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if out.is_some_and(|ev| tx.send(ev).is_err()) {
            return;
        }
    }
    alive.store(false, Ordering::SeqCst);
    lock(&state).idle = true;
    let _ = tx.send(Event::Closed);
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
