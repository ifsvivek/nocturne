//! [`Player`]: mpv as a child process driven over its JSON IPC.
//!
//! Nothing here ever blocks the caller (the UI thread). Starting mpv, connecting to its socket or
//! pipe, every write and every read happen on two worker threads per mpv instance: the *session*
//! thread (spawn, connect with retries, then writes queued lines) and the *reader* thread (parses
//! mpv's output into [`State`], the property cache and [`Event`]s). The caller only pushes lines
//! into a channel and reads snapshots. Properties come from a cache kept current with
//! `observe_property`, so [`Player::prop`] is a map lookup: a slow mpv (a virtual machine with a
//! software GPU can take many seconds to open its window) can delay values but never stalls the UI.
use crate::{Error, Event, Options, Result, State, deps, e, ipc, lock};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A VM can need a long time to start mpv and create its pipe.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// State properties (observe ids 1..).
const STATE_PROPS: [&str; 7] = [
    "pause",
    "time-pos",
    "duration",
    "volume",
    "eof-reached",
    "idle-active",
    "demuxer-cache-duration",
];

/// Properties the UI reads through [`Player::prop`]: observed from the start (ids 100..) so the
/// first read after they exist already finds a value. Anything else is observed on first use.
const WATCH: [&str; 29] = [
    "track-list",
    "chapter-list",
    "secondary-sid",
    "sid",
    "aid",
    "audio-device-list",
    "audio-device",
    "paused-for-cache",
    "seeking",
    "cache-buffering-state",
    "file-format",
    "video-format",
    "width",
    "height",
    "container-fps",
    "video-bitrate",
    "audio-codec-name",
    "audio-params/hr-channels",
    "audio-bitrate",
    "hwdec-current",
    "frame-drop-count",
    "decoder-frame-drop-count",
    "mpv-version",
    "video-params/gamma",
    "video-params/primaries",
    "video-target-params/gamma",
    "sub-delay",
    "audio-delay",
    "speed",
];

/// Changes of these make the UI re-read its track and chapter lists ([`Event::Tracks`]).
const TRACKS: [&str; 3] = ["track-list", "chapter-list", "secondary-sid"];

enum Out {
    Line(String),
    Quit,
}

/// What both worker threads and the owner share.
struct Shared {
    state: Mutex<State>,
    /// Latest value of every observed property (absent = unavailable).
    props: Mutex<HashMap<String, Value>>,
    /// Names observed on first use (beyond `STATE_PROPS` and `WATCH`).
    extra: Mutex<Vec<String>>,
    /// Replies awaited by [`Player::prop_blocking`], by request id.
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    req: AtomicU64,
    /// The instance whose output counts; older ones (being shut down) are ignored.
    seq: AtomicU64,
    /// A `loadfile` was sent and mpv has not answered yet: the state must not read as "idle".
    loading: AtomicBool,
    tx: Sender<Event>,
}

/// One mpv instance as seen by the owner.
struct Link {
    out: Sender<Out>,
    /// False once mpv is gone or never came up.
    alive: Arc<AtomicBool>,
    /// The session thread has finished (mpv reaped).
    done: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
}

pub struct Player {
    opts: Options,
    dir: PathBuf,
    /// Instance counter, part of the Windows pipe name.
    n: u64,
    shared: Arc<Shared>,
    rx: Receiver<Event>,
    link: Mutex<Option<Link>>,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl Player {
    /// Returns at once; mpv is started and connected on a worker thread. A start that fails
    /// arrives as [`Event::Failed`] then [`Event::Closed`].
    pub fn new(opts: Options) -> Result<Self> {
        // Private dir holding the IPC socket (pid + counter in the name).
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = crate::runtime_base().join(format!("nocturne-mpv-{}-{n}", std::process::id()));
        ipc::private_dir(&dir).map_err(e)?;
        let (tx, rx) = channel();
        let p = Self {
            opts,
            dir,
            n,
            shared: Arc::new(Shared {
                state: Mutex::default(),
                props: Mutex::default(),
                extra: Mutex::default(),
                pending: Mutex::default(),
                req: AtomicU64::new(1),
                seq: AtomicU64::new(0),
                loading: AtomicBool::new(false),
                tx,
            }),
            rx,
            link: Mutex::new(None),
        };
        *p.link.lock().unwrap() = Some(p.start_session());
        Ok(p)
    }

    /// Non-blocking event source: `try_recv` / `try_iter`.
    pub fn events(&self) -> &Receiver<Event> {
        &self.rx
    }

    /// Replace the current file. Each of `headers` is a full header line (e.g. `Authorization: ...`)
    /// sent over IPC as `http-header-fields`, so credentials never reach the command line, the URL
    /// or any log (nothing here prints them). Queues the commands and returns; starts a fresh mpv
    /// first if the old one died.
    pub fn load(&self, url: &str, headers: &[&str]) -> Result<()> {
        let mut cmds = vec![json!(["change-list", "http-header-fields", "clr", ""])];
        for h in headers {
            // `append` adds one list item verbatim (no splitting on ',').
            cmds.push(json!(["change-list", "http-header-fields", "append", h]));
        }
        cmds.push(json!(["loadfile", url, "replace"]));
        let lines: Vec<String> = cmds.iter().map(|c| self.line(c)).collect();
        let mut g = self.link.lock().unwrap();
        if !g.as_ref().is_some_and(|l| l.alive.load(Ordering::SeqCst)) {
            self.respawn(&mut g);
        }
        self.shared.loading.store(true, Ordering::SeqCst);
        if send_all(g.as_ref(), &lines).is_err() {
            // The session ended between the liveness check and the send: one retry on a fresh mpv.
            self.respawn(&mut g);
            send_all(g.as_ref(), &lines).map_err(|_| Error::Mpv("not running".into()))?;
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

    /// The latest value of an mpv property, from the cache; `None` when it is unavailable or not
    /// known yet. Never blocks. A property not seen before is observed from now on, so the next
    /// call (the UI polls) finds it.
    pub fn prop(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.shared.props.lock().unwrap().get(name) {
            return Some(v.clone());
        }
        self.watch(name);
        None
    }

    /// Asks mpv and waits up to `timeout` for the answer. For tests and tools; **never call it
    /// from a UI thread**.
    pub fn prop_blocking(&self, name: &str, timeout: Duration) -> Option<Value> {
        let id = self.shared.req.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.shared.pending.lock().unwrap().insert(id, tx);
        let line = request(id, &json!(["get_property", name]));
        let sent = send_all(self.link.lock().unwrap().as_ref(), &[line]).is_ok();
        let v = sent.then(|| rx.recv_timeout(timeout).ok()).flatten();
        self.shared.pending.lock().unwrap().remove(&id);
        v.filter(|v| !v.is_null())
    }

    /// Cheap, non-blocking: a snapshot maintained by the IPC reader thread.
    pub fn state(&self) -> State {
        let mut s = lock(&self.shared.state).clone();
        if self.shared.loading.load(Ordering::SeqCst) {
            s.idle = false; // mpv is still starting or opening: not a failed load
        }
        s
    }

    /// Child mpv pid (tests only).
    #[doc(hidden)]
    pub fn child_pid(&self) -> Option<u32> {
        let pid = self
            .link
            .lock()
            .unwrap()
            .as_ref()?
            .pid
            .load(Ordering::SeqCst);
        (pid != 0).then_some(pid)
    }

    fn set_paused(&self, p: bool) -> Result<()> {
        self.cmd(json!(["set_property", "pause", p]))
    }

    fn line(&self, c: &Value) -> String {
        request(self.shared.req.fetch_add(1, Ordering::Relaxed), c)
    }

    fn cmd(&self, c: Value) -> Result<()> {
        send_all(self.link.lock().unwrap().as_ref(), &[self.line(&c)])
            .map_err(|_| Error::Mpv("not running".into()))
    }

    /// Observes `name` from now on (once).
    fn watch(&self, name: &str) {
        if STATE_PROPS.contains(&name) || WATCH.contains(&name) {
            return;
        }
        let mut x = self.shared.extra.lock().unwrap();
        if x.iter().any(|n| n == name) {
            return;
        }
        x.push(name.to_string());
        let line = observe_line(1000 + x.len() as u64, name);
        let _ = send_all(self.link.lock().unwrap().as_ref(), &[line]);
    }

    fn respawn(&self, g: &mut Option<Link>) {
        if let Some(old) = g.take() {
            retire(old, None);
        }
        *g = Some(self.start_session());
    }

    /// A new mpv instance on its own threads.
    fn start_session(&self) -> Link {
        let seq = self.shared.seq.fetch_add(1, Ordering::SeqCst) + 1;
        *lock(&self.shared.state) = State::default();
        self.shared.props.lock().unwrap().clear();
        let (out, rx) = channel();
        let link = Link {
            out: out.clone(),
            alive: Arc::new(AtomicBool::new(true)),
            done: Arc::new(AtomicBool::new(false)),
            pid: Arc::new(AtomicU32::new(0)),
        };
        let s = Session {
            shared: self.shared.clone(),
            opts: self.opts.clone(),
            dir: self.dir.clone(),
            endpoint: ipc::endpoint(&self.dir, std::process::id(), self.n, seq),
            seq,
            rx,
            out,
            alive: link.alive.clone(),
            done: link.done.clone(),
            pid: link.pid.clone(),
        };
        std::thread::spawn(move || s.run());
        link
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(l) = self.link.get_mut().unwrap().take() {
            // Quit mpv, waiting a moment so it is gone when the owner exits; the rest finishes on
            // its own.
            retire(l, Some(Duration::from_secs(1)));
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn request(id: u64, c: &Value) -> String {
    json!({"command": c, "request_id": id}).to_string() + "\n"
}

fn observe_line(id: u64, name: &str) -> String {
    json!({"command": ["observe_property", id, name]}).to_string() + "\n"
}

fn send_all(link: Option<&Link>, lines: &[String]) -> std::result::Result<(), ()> {
    let l = link.ok_or(())?;
    for s in lines {
        l.out.send(Out::Line(s.clone())).map_err(|_| ())?;
    }
    Ok(())
}

/// Ask the session to quit mpv; optionally wait for it.
fn retire(l: Link, wait: Option<Duration>) {
    l.alive.store(false, Ordering::SeqCst);
    let _ = l.out.send(Out::Quit);
    if let Some(w) = wait {
        let t = Instant::now();
        while !l.done.load(Ordering::SeqCst) && t.elapsed() < w {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct Session {
    shared: Arc<Shared>,
    opts: Options,
    dir: PathBuf,
    endpoint: String,
    seq: u64,
    rx: Receiver<Out>,
    out: Sender<Out>,
    alive: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
}

impl Session {
    fn current(&self) -> bool {
        self.shared.seq.load(Ordering::SeqCst) == self.seq
    }

    fn fail(&self, msg: String) {
        self.alive.store(false, Ordering::SeqCst);
        if self.current() {
            self.shared.loading.store(false, Ordering::SeqCst);
            let _ = self.shared.tx.send(Event::Failed(msg));
            let _ = self.shared.tx.send(Event::Closed);
        }
    }

    fn run(self) {
        ipc::clear(&self.endpoint);
        let mut child = match spawn_child(
            &self.opts,
            &self.dir,
            &lock(&self.shared.state),
            &self.endpoint,
        ) {
            Ok(c) => c,
            Err(err) => {
                self.fail(format!("could not start mpv: {err}"));
                self.done.store(true, Ordering::SeqCst);
                return;
            }
        };
        self.pid.store(child.id(), Ordering::SeqCst);

        // Connect, retrying; lines queued meanwhile wait in `backlog`.
        let mut backlog: Vec<String> = vec![];
        let t = Instant::now();
        let writer = loop {
            match ipc::connect(&self.endpoint) {
                Ok(s) => break s,
                Err(err) => {
                    let quit = self.drain(&mut backlog);
                    let dead = child.try_wait().ok().flatten().is_some();
                    if quit || dead || t.elapsed() > CONNECT_TIMEOUT {
                        let _ = child.kill();
                        let _ = child.wait();
                        if !quit {
                            self.fail(format!("ipc connect: {err}"));
                        }
                        self.done.store(true, Ordering::SeqCst);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        let mut writer = writer;
        let reader = match ipc::reader(&writer) {
            Ok(rd) => {
                let (sh, out, al, seq) = (
                    self.shared.clone(),
                    self.out.clone(),
                    self.alive.clone(),
                    self.seq,
                );
                std::thread::spawn(move || read_loop(rd, &sh, seq, &out, &al))
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                self.fail(format!("ipc: {err}"));
                self.done.store(true, Ordering::SeqCst);
                return;
            }
        };

        let mut ok = self.observe_all(&mut writer).is_ok();
        for l in &backlog {
            ok = ok && writer.write_all(l.as_bytes()).is_ok();
        }
        while ok && self.alive.load(Ordering::SeqCst) {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Out::Line(l)) => ok = writer.write_all(l.as_bytes()).is_ok(),
                Ok(Out::Quit) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }

        // Shut down: ask nicely, then kill.
        self.alive.store(false, Ordering::SeqCst);
        let _ = writer.write_all(b"{\"command\":[\"quit\"]}\n");
        let t = Instant::now();
        while child.try_wait().ok().flatten().is_none() {
            if t.elapsed() > Duration::from_millis(800) {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        ipc::shutdown(&writer);
        let _ = reader.join();
        self.done.store(true, Ordering::SeqCst);
    }

    /// Moves queued lines to `backlog`; true when asked to quit.
    fn drain(&self, backlog: &mut Vec<String>) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(Out::Line(l)) => backlog.push(l),
                Ok(Out::Quit) => return true,
                Err(_) => return false,
            }
        }
    }

    fn observe_all(&self, w: &mut ipc::Stream) -> std::io::Result<()> {
        let extra = self.shared.extra.lock().unwrap().clone();
        let names = STATE_PROPS
            .iter()
            .enumerate()
            .map(|(i, n)| (i as u64 + 1, *n))
            .chain(WATCH.iter().enumerate().map(|(i, n)| (100 + i as u64, *n)))
            .chain(
                extra
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (1001 + i as u64, n.as_str())),
            );
        for (id, n) in names {
            w.write_all(observe_line(id, n).as_bytes())?;
        }
        Ok(())
    }
}

fn spawn_child(opts: &Options, dir: &Path, st: &State, sock: &str) -> std::io::Result<Child> {
    let mut cmd = Command::new(deps::mpv_program());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console next to mpv's window
    }
    for (k, v) in opts.buffering.mpv_options() {
        cmd.arg(format!("--{k}={v}"));
    }
    cmd.args([
        "--no-config",
        "--no-terminal",
        "--idle=yes",
        "--keep-open=yes",
        "--title=NOCTURNE",
    ]);
    if opts.headless {
        cmd.args(["--vo=null", "--ao=null", "--force-window=no"]);
    } else {
        // Windows: fall back to the plain `gpu` output when `gpu-next` cannot start (virtual GPUs).
        let default_vo = if cfg!(windows) {
            "gpu-next,gpu,"
        } else {
            "gpu-next"
        };
        let vo = std::env::var("NOCTURNE_VO").unwrap_or_else(|_| default_vo.into());
        cmd.args([format!("--vo={vo}"), "--hwdec=auto-safe".into()])
            .arg("--force-window=yes");
        if opts.hdr_output {
            cmd.arg("--target-colorspace-hint=yes");
        }
        if opts.fullscreen {
            cmd.arg("--fullscreen");
        }
        if let Some(wid) = opts.wid {
            // Drawn inside the app's child window. The app draws every control, so mpv's own
            // are off, and its mouse input comes back as `client-message` events (the picture's
            // window is not the app's, so the UI toolkit never sees the pointer over it).
            let conf = dir.join("input.conf");
            std::fs::write(&conf, INPUT_CONF)?;
            cmd.arg(format!("--wid={wid}"))
                .args(["--osc=no", "--osd-level=0", "--input-default-bindings=no"])
                .arg("--input-vo-keyboard=no")
                .arg(format!("--input-conf={}", conf.display()));
        }
    }
    cmd.arg(format!("--volume={}", st.volume))
        .arg(format!("--input-ipc-server={sock}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Pointer input of the embedded picture, as `script-message`s the reader turns into [`Event::Input`].
const INPUT_CONF: &str = "MOUSE_MOVE script-message nocturne-in \"move ${mouse-pos/x} ${mouse-pos/y}\"\n\
MBTN_LEFT script-message nocturne-in \"click ${mouse-pos/x} ${mouse-pos/y}\"\n\
MBTN_LEFT_DBL script-message nocturne-in \"dbl ${mouse-pos/x} ${mouse-pos/y}\"\n\
MBTN_RIGHT script-message nocturne-in \"right ${mouse-pos/x} ${mouse-pos/y}\"\n";

/// Parses mpv's output until the stream ends. Generic over the stream so tests drive it with
/// in-memory data and a fake pipe.
fn read_loop(rd: impl Read, sh: &Shared, seq: u64, out: &Sender<Out>, alive: &AtomicBool) {
    let current = || sh.seq.load(Ordering::SeqCst) == seq;
    // Ids of the track/chapter reads after `file-loaded`: `Event::Loaded` waits for them, so the
    // UI's first look at the track list already finds it.
    let mut awaiting: HashMap<u64, &'static str> = HashMap::new();
    for line in BufReader::new(rd).lines() {
        let Ok(line) = line else { break };
        if !current() {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(ev) = v["event"].as_str() else {
            let ok = v["error"] == "success";
            let data = if ok { v["data"].clone() } else { Value::Null };
            let Some(id) = v["request_id"].as_u64() else {
                continue;
            };
            if let Some(name) = awaiting.remove(&id) {
                if !data.is_null() {
                    sh.props.lock().unwrap().insert(name.into(), data);
                }
                if awaiting.is_empty() {
                    sh.loading.store(false, Ordering::SeqCst);
                    if sh.tx.send(Event::Loaded).is_err() {
                        return;
                    }
                }
            } else if let Some(tx) = sh.pending.lock().unwrap().remove(&id) {
                let _ = tx.send(data);
            }
            continue;
        };
        let evt = match ev {
            "file-loaded" => {
                for name in ["track-list", "chapter-list"] {
                    let id = sh.req.fetch_add(1, Ordering::Relaxed);
                    awaiting.insert(id, name);
                    if out
                        .send(Out::Line(request(id, &json!(["get_property", name]))))
                        .is_err()
                    {
                        awaiting.clear();
                    }
                }
                if awaiting.is_empty() {
                    sh.loading.store(false, Ordering::SeqCst);
                    Some(Event::Loaded)
                } else {
                    None
                }
            }
            "shutdown" => break,
            "end-file" => {
                let reason = v["reason"].as_str().unwrap_or("");
                if !matches!(reason, "stop" | "redirect") {
                    sh.loading.store(false, Ordering::SeqCst);
                }
                (reason == "error").then(|| {
                    Event::Failed(v["file_error"].as_str().unwrap_or("playback failed").into())
                })
            }
            "client-message" => match v["args"].as_array().map(Vec::as_slice) {
                Some([a, b, ..]) if a == "nocturne-in" => {
                    b.as_str().map(|s| Event::Input(s.to_string()))
                }
                _ => None,
            },
            "property-change" => property_change(sh, &v),
            _ => None,
        };
        if evt.is_some_and(|ev| sh.tx.send(ev).is_err()) {
            return;
        }
    }
    alive.store(false, Ordering::SeqCst);
    if current() {
        sh.loading.store(false, Ordering::SeqCst);
        lock(&sh.state).idle = true;
        let _ = sh.tx.send(Event::Closed);
    }
}

fn property_change(sh: &Shared, v: &Value) -> Option<Event> {
    let name = v["name"].as_str().unwrap_or("");
    let d = &v["data"];
    {
        let mut p = sh.props.lock().unwrap();
        if d.is_null() {
            p.remove(name);
        } else {
            p.insert(name.to_string(), d.clone());
        }
    }
    let mut s = lock(&sh.state);
    match name {
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
        n if TRACKS.contains(&n) => Some(Event::Tracks),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> (Arc<Shared>, Receiver<Event>) {
        let (tx, rx) = channel();
        let sh = Arc::new(Shared {
            state: Mutex::default(),
            props: Mutex::default(),
            extra: Mutex::default(),
            pending: Mutex::default(),
            req: AtomicU64::new(1),
            seq: AtomicU64::new(1),
            loading: AtomicBool::new(true),
            tx,
        });
        (sh, rx)
    }

    /// Runs the reader over `input` (what mpv would print) and returns what it produced.
    fn feed(sh: &Arc<Shared>, input: &str) -> Vec<Out> {
        let (out, orx) = channel();
        read_loop(input.as_bytes(), sh, 1, &out, &AtomicBool::new(true));
        orx.try_iter().collect()
    }

    #[test]
    fn properties_are_cached_and_events_follow() {
        let (sh, rx) = shared();
        feed(
            &sh,
            concat!(
                "{\"event\":\"property-change\",\"id\":1,\"name\":\"pause\",\"data\":true}\n",
                "{\"event\":\"property-change\",\"id\":2,\"name\":\"file-format\",\"data\":\"matroska\"}\n",
                "{\"event\":\"property-change\",\"id\":3,\"name\":\"track-list\",\"data\":[]}\n",
                "{\"event\":\"client-message\",\"args\":[\"nocturne-in\",\"click\"]}\n",
                "{\"event\":\"property-change\",\"id\":2,\"name\":\"file-format\"}\n",
            ),
        );
        let p = sh.props.lock().unwrap();
        assert_eq!(p.get("pause"), Some(&json!(true)));
        assert!(
            !p.contains_key("file-format"),
            "an unavailable property leaves the cache"
        );
        drop(p);
        let evs: Vec<Event> = rx.try_iter().collect();
        assert_eq!(
            evs,
            [
                Event::Paused(true),
                Event::Tracks,
                Event::Input("click".into()),
                Event::Closed
            ]
        );
    }

    /// Feeds the reader one chunk per `read`, like a pipe delivering partial data.
    struct Chunks(Vec<Vec<u8>>);
    impl Read for Chunks {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let c = self.0.remove(0);
            buf[..c.len()].copy_from_slice(&c);
            Ok(c.len())
        }
    }

    #[test]
    fn loaded_waits_for_the_track_lists_and_split_lines_are_joined() {
        let (sh, rx) = shared();
        let (out, orx) = channel();
        // `Shared` starts its ids at 1: the reader's two reads are requests 1 and 2.
        let r2 = "{\"data\":[2],\"request_id\":2,\"error\":\"success\"}\n";
        let (h1, h2) = r2.split_at(r2.len() / 2); // a partial read in the middle of a line
        let chunks = vec![
            b"{\"event\":\"file-loaded\"}\n".to_vec(),
            b"{\"data\":[1],\"request_id\":1,\"error\":\"success\"}\n".to_vec(),
            h1.as_bytes().to_vec(),
            h2.as_bytes().to_vec(),
        ];
        read_loop(Chunks(chunks), &sh, 1, &out, &AtomicBool::new(true));
        assert_eq!(orx.try_iter().count(), 2, "it asked for both lists");
        assert_eq!(rx.try_recv().unwrap(), Event::Loaded);
        assert_eq!(sh.props.lock().unwrap()["track-list"], json!([1]));
        assert_eq!(sh.props.lock().unwrap()["chapter-list"], json!([2]));
        assert!(!sh.loading.load(Ordering::SeqCst));
    }

    #[test]
    fn replies_reach_a_blocking_asker_by_request_id() {
        let (sh, _rx) = shared();
        let (tx, rx) = channel();
        sh.pending.lock().unwrap().insert(7, tx);
        feed(
            &sh,
            "{\"event\":\"idle\"}\n{\"data\":\"x\",\"request_id\":8,\"error\":\"success\"}\n{\"data\":42,\"request_id\":7,\"error\":\"success\"}\n",
        );
        assert_eq!(rx.try_recv().unwrap(), json!(42));
    }

    #[test]
    fn an_old_instance_is_ignored() {
        let (sh, rx) = shared();
        sh.seq.store(2, Ordering::SeqCst); // a respawn happened
        feed(
            &sh,
            "{\"event\":\"property-change\",\"name\":\"pause\",\"data\":true}\n",
        );
        assert!(
            rx.try_recv().is_err(),
            "no event, not even Closed, from a stale reader"
        );
        assert!(sh.props.lock().unwrap().is_empty());
    }

    /// The point of the whole design: a caller is never held up by mpv, even one that does not exist.
    #[test]
    fn starting_a_missing_mpv_does_not_block_and_reports_by_event() {
        let t = Instant::now();
        let p = Player::new(Options {
            headless: true,
            ..Default::default()
        })
        .unwrap();
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "new() returned at once"
        );
        let t = Instant::now();
        let _ = p.load("file:///nonexistent", &[]);
        let _ = p.prop("file-format");
        let _ = p.state();
        assert!(
            t.elapsed() < Duration::from_millis(100),
            "load/prop/state never wait"
        );
        assert!(p.prop("track-list").is_none());
    }
}
