//! VLC as an external player: a child process with its own window, controlled over VLC's HTTP
//! interface on loopback (random port, random password). Same surface as [`crate::Player`] for
//! what the app needs (load, pause, seek, volume, state, events); everything mpv-specific
//! (tracks, filters, properties) is not available and answers "no".
//!
//! One worker thread owns the child and does all the HTTP (short timeouts), so the UI thread only
//! pushes to a channel and reads a snapshot, like the mpv IPC.

use crate::{Error, Event, Result, State, lock};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// VLC's volume scale: 256 is 100 %.
const VLC_FULL: f64 = 256.0;
const POLL: Duration = Duration::from_millis(250);
/// The HTTP interface must answer within this after start.
const START_WITHIN: Duration = Duration::from_secs(20);
/// A load that never plays stops counting as "opening" after this (the app then reports failure).
const OPEN_FOR: Duration = Duration::from_secs(6);

/// The `vlc` program: `PATH` first, then the usual install places on Windows and macOS.
pub(crate) fn vlc_path() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = vec![];
    let exe = if cfg!(windows) { "vlc.exe" } else { "vlc" };
    c.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths)
            .map(|d| d.join(exe)),
    );
    if cfg!(windows) {
        for v in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(d) = std::env::var_os(v) {
                c.push(PathBuf::from(d).join(r"VideoLAN\VLC\vlc.exe"));
            }
        }
    }
    if cfg!(target_os = "macos") {
        c.push("/Applications/VLC.app/Contents/MacOS/VLC".into());
    }
    c.into_iter().find(|p| crate::deps::is_executable(p))
}

/// True when VLC is installed (see [`vlc_path`]).
pub fn vlc_available() -> bool {
    vlc_path().is_some()
}

/// What `status.json` tells us.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Status {
    /// `playing`, `paused` or `stopped`.
    pub state: String,
    pub time: f64,
    pub length: f64,
    /// 0..=100+ on the mpv scale.
    pub volume: f64,
}

pub(crate) fn parse_status(body: &str) -> Option<Status> {
    let v: Value = serde_json::from_str(body).ok()?;
    let n = |k: &str| v[k].as_f64().unwrap_or(0.0);
    Some(Status {
        state: v["state"].as_str()?.to_string(),
        time: n("time").max(0.0),
        length: n("length").max(0.0),
        volume: n("volume") / VLC_FULL * 100.0,
    })
}

/// The stream URL VLC is given. VLC cannot send request headers, so a Jellyfin token travels as
/// `api_key` in the URL; the URL only goes to VLC over the authenticated loopback interface (never
/// on a command line). Any other header line cannot be sent at all: `Err`.
pub fn vlc_url(url: &str, headers: &[&str]) -> std::result::Result<String, &'static str> {
    let mut key = None;
    for h in headers {
        let (name, val) = h.split_once(':').unwrap_or((h, ""));
        if !name.eq_ignore_ascii_case("authorization") {
            return Err("VLC cannot send this server's extra header");
        }
        key = Some(
            val.split_once("Token=\"")
                .and_then(|(_, t)| t.split_once('"'))
                .ok_or("VLC cannot send this sign-in")?
                .0
                .to_string(),
        );
    }
    Ok(match key {
        Some(k) => format!(
            "{url}{}api_key={}",
            if url.contains('?') { '&' } else { '?' },
            enc(&k)
        ),
        None => url.to_string(),
    })
}

fn enc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn b64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                o.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                o.push('=');
            }
        }
    }
    o
}

/// 128 random bits as hex. `RandomState` is seeded from the OS; hashing distinct inputs with it
/// gives independent-looking words.
fn random_hex() -> String {
    use std::hash::{BuildHasher, Hasher};
    let s = std::collections::hash_map::RandomState::new();
    (0..2)
        .map(|i| {
            let mut h = s.build_hasher();
            h.write_u64(i);
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos()),
            );
            format!("{:016x}", h.finish())
        })
        .collect()
}

struct Http {
    addr: SocketAddr,
    auth: String,
}

impl Http {
    /// `GET path` and the body; any non-200 is an error.
    fn get(&self, path: &str) -> std::io::Result<String> {
        let bad = |m: &str| std::io::Error::other(m.to_string());
        let mut s = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1))?;
        s.set_read_timeout(Some(Duration::from_secs(2)))?;
        s.set_write_timeout(Some(Duration::from_secs(2)))?;
        write!(
            s,
            "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Basic {}\r\nConnection: close\r\n\r\n",
            self.auth
        )?;
        let mut raw = vec![];
        s.read_to_end(&mut raw)?;
        let raw = String::from_utf8_lossy(&raw);
        let (head, body) = raw.split_once("\r\n\r\n").ok_or_else(|| bad("no reply"))?;
        if head.split_whitespace().nth(1) != Some("200") {
            return Err(bad("refused"));
        }
        Ok(body.to_string())
    }

    /// `status.json` with an optional `command=` query (already encoded).
    fn cmd(&self, q: &str) -> std::io::Result<String> {
        if q.is_empty() {
            self.get("/requests/status.json")
        } else {
            self.get(&format!("/requests/status.json?command={q}"))
        }
    }
}

enum Cmd {
    Load(String),
    /// A `command=` query (already encoded).
    Do(String),
    Quit,
}

struct Shared {
    state: Mutex<State>,
    tx: Sender<Event>,
}

pub struct VlcPlayer {
    shared: Arc<Shared>,
    rx: Receiver<Event>,
    cmd: Sender<Cmd>,
    done: Arc<AtomicBool>,
}

impl VlcPlayer {
    /// Returns at once; VLC is started and connected on a worker thread. A start that fails
    /// arrives as [`Event::Failed`] then [`Event::Closed`].
    pub fn new() -> Result<Self> {
        let path = vlc_path().ok_or_else(|| Error::Vlc("not found".into()))?;
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|l| l.local_addr())
            .map_err(|e| Error::Vlc(e.to_string()))?
            .port();
        Self::start(Command::new(path), port, random_hex())
    }

    /// `c` is VLC (or a stand-in in tests); the interface arguments are added here. The password
    /// is on the command line, as VLC offers no other way; it only opens a loopback port.
    fn start(mut c: Command, port: u16, pass: String) -> Result<Self> {
        let private = privacy_flags(c.get_program());
        c.args([
            "--extraintf=http",
            "--http-host=127.0.0.1",
            &format!("--http-port={port}"),
            &format!("--http-password={pass}"),
            // Our own instance (never hand over to the user's running VLC), no first-run question, no lookups.
            "--no-one-instance",
            "--no-qt-privacy-ask",
            "--no-metadata-network-access",
        ])
        // The stream URL carries the server token (`api_key`): never let VLC write it to its recent list, its resume list or its media library.
        .args(private)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        let http = Http {
            addr: (Ipv4Addr::LOCALHOST, port).into(),
            auth: b64(format!(":{pass}").as_bytes()),
        };
        let (tx, rx) = channel();
        let (ctx, crx) = channel();
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            tx,
        });
        let done = Arc::new(AtomicBool::new(false));
        let (s2, d2) = (shared.clone(), done.clone());
        std::thread::spawn(move || {
            run(c, http, crx, &s2);
            d2.store(true, Ordering::SeqCst);
        });
        Ok(Self {
            shared,
            rx,
            cmd: ctx,
            done,
        })
    }

    pub fn events(&self) -> &Receiver<Event> {
        &self.rx
    }

    /// Open `url` (see [`vlc_url`] for the headers).
    pub fn load(&self, url: &str, headers: &[&str]) -> Result<()> {
        let u = vlc_url(url, headers).map_err(|m| Error::Vlc(m.into()))?;
        lock(&self.shared.state).idle = false;
        self.send(Cmd::Load(u))
    }

    pub fn toggle(&self) -> Result<()> {
        self.send(Cmd::Do("pl_pause".into()))
    }

    pub fn seek_to(&self, secs: f64) -> Result<()> {
        self.send(Cmd::Do(format!(
            "seek&val={}",
            secs.max(0.0).floor() as u64
        )))
    }

    pub fn seek_by(&self, secs: f64) -> Result<()> {
        let s = secs.round() as i64;
        self.send(Cmd::Do(format!(
            "seek&val={}{}S",
            if s < 0 { "-" } else { "%2B" },
            s.abs()
        )))
    }

    /// 0..=130 on the mpv scale.
    pub fn set_volume(&self, vol: f64) -> Result<()> {
        let v = (vol.clamp(0.0, 130.0) / 100.0 * VLC_FULL).round() as u32;
        self.send(Cmd::Do(format!("volume&val={v}")))
    }

    pub fn state(&self) -> State {
        lock(&self.shared.state).clone()
    }

    /// mpv properties do not exist here: always unknown.
    pub fn prop(&self, _name: &str) -> Option<Value> {
        None
    }

    /// The few mpv commands the player page sends that VLC has an answer to; the rest is `Err`.
    pub fn command(&self, args: &[&str]) -> Result<()> {
        match args {
            ["set", "pause", v] => {
                if self.state().paused != matches!(*v, "yes" | "true") {
                    return self.toggle();
                }
                Ok(())
            }
            ["set", "speed", v] => self.send(Cmd::Do(format!("rate&val={}", enc(v)))),
            ["cycle", "fullscreen"] => self.send(Cmd::Do("fullscreen".into())),
            ["stop"] => self.send(Cmd::Do("pl_stop".into())),
            _ => Err(Error::Vlc("not supported".into())),
        }
    }

    fn send(&self, c: Cmd) -> Result<()> {
        self.cmd
            .send(c)
            .map_err(|_| Error::Vlc("not running".into()))
    }
}

impl Drop for VlcPlayer {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Quit);
        let t = Instant::now();
        while !self.done.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// The worker: start VLC, wait for its interface, then apply commands and poll the status.
fn run(mut c: Command, http: Http, rx: Receiver<Cmd>, sh: &Shared) {
    let ev = |e| {
        let _ = sh.tx.send(e);
    };
    let Ok(mut child) = c.spawn() else {
        ev(Event::Failed("VLC could not be started".into()));
        ev(Event::Closed);
        return;
    };
    let begun = Instant::now();
    let (mut ready, mut pending) = (false, None::<String>);
    // Per load: when it was sent, whether `Loaded` went out, whether it ended.
    let (mut opened, mut loaded, mut ended) = (None::<Instant>, false, false);
    loop {
        match rx.recv_timeout(POLL) {
            Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Cmd::Load(u)) => pending = Some(u),
            Ok(Cmd::Do(q)) if ready => {
                let _ = http.cmd(&q);
            }
            _ => {}
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            ev(Event::Closed);
            break;
        }
        let Ok(body) = http.cmd("") else {
            if !ready && begun.elapsed() > START_WITHIN {
                ev(Event::Failed("VLC did not start".into()));
                ev(Event::Closed);
                break;
            }
            continue;
        };
        ready = true;
        if let Some(u) = pending.take() {
            let _ = http.cmd("pl_empty");
            let _ = http.cmd(&format!("in_play&input={}", enc(&u)));
            (opened, loaded, ended) = (Some(Instant::now()), false, false);
        }
        let Some(st) = parse_status(&body) else {
            continue;
        };
        let playing = st.state != "stopped";
        if playing && st.length > 0.0 && !loaded {
            loaded = true;
            ev(Event::Loaded);
        }
        if loaded && !playing && !ended {
            ended = true;
            ev(Event::Eof);
        }
        let mut s = lock(&sh.state);
        let paused = st.state == "paused";
        if paused != s.paused {
            ev(Event::Paused(paused));
        }
        *s = State {
            // At the end VLC reports 0: keep where it stopped.
            position: if ended {
                s.position
            } else {
                loaded.then_some(st.time)
            },
            duration: (st.length > 0.0).then_some(st.length).or(s.duration),
            paused,
            volume: st.volume,
            buffered: 0.0,
            eof: ended,
            idle: !playing && !ended && opened.is_none_or(|t| t.elapsed() > OPEN_FOR),
        };
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The flags that keep the stream URL out of VLC's recent list, resume list and media library, limited to the ones this VLC knows: VLC refuses to start on an unknown option, and a VLC without its Qt interface or media library has no `qt-*` or `media-library` options (and keeps no such lists). Probed once per program with a run that quits at once.
fn privacy_flags(program: &std::ffi::OsStr) -> Vec<&'static str> {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashMap<std::ffi::OsString, Vec<&'static str>>>> =
        OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some(v) = seen.lock().unwrap().get(program) {
        return v.clone();
    }
    let accepts = |flags: &[&str]| {
        Command::new(program)
            .args(["--intf=dummy", "--no-one-instance"])
            .args(flags)
            .arg("vlc://quit")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let mut out = vec![];
    for group in [
        &["--no-qt-recentplay", "--qt-continue=0"][..],
        &["--no-media-library"][..],
    ] {
        if accepts(group) {
            out.extend_from_slice(group);
        }
    }
    seen.lock().unwrap().insert(program.to_owned(), out.clone());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_json() {
        let j = r#"{"fullscreen":0,"time":42,"volume":128,"length":3600,"state":"paused","rate":1,"position":0.011,"loop":false,"information":{"category":{"meta":{"filename":"x.mkv"}}}}"#;
        let s = parse_status(j).unwrap();
        assert_eq!(
            s,
            Status {
                state: "paused".into(),
                time: 42.0,
                length: 3600.0,
                volume: 50.0
            }
        );
        // Nothing opened yet: VLC reports stopped with no length (time -1 on some builds).
        let s = parse_status(r#"{"state":"stopped","time":-1,"length":0,"volume":256}"#).unwrap();
        assert_eq!((s.time, s.length, s.volume), (0.0, 0.0, 100.0));
        assert!(parse_status("<html>").is_none());
        assert!(parse_status(r#"{"time":3}"#).is_none());
    }

    #[test]
    fn token_goes_in_the_url_never_other_headers() {
        let auth = r#"Authorization: MediaBrowser Client="N", DeviceId="d", Token="a+b/c""#;
        let u = vlc_url("http://h/Videos/1/stream?static=true", &[auth]).unwrap();
        assert_eq!(u, "http://h/Videos/1/stream?static=true&api_key=a%2Bb%2Fc");
        assert_eq!(
            vlc_url("http://h/v", &[auth]).unwrap(),
            "http://h/v?api_key=a%2Bb%2Fc"
        );
        assert_eq!(vlc_url("/media/a b.mkv", &[]).unwrap(), "/media/a b.mkv");
        assert!(vlc_url("http://h/v", &[auth, "CF-Access: x"]).is_err());
        assert!(vlc_url("http://h/v", &["Authorization: Basic abc"]).is_err());
    }

    #[test]
    fn basic_auth_and_encoding() {
        assert_eq!(b64(b":pw"), "OnB3");
        assert_eq!(b64(b":p"), "OnA=");
        assert_eq!(b64(b":"), "Og==");
        assert_eq!(enc("a b&c"), "a%20b%26c");
        assert_eq!(random_hex().len(), 32);
        assert_ne!(random_hex(), random_hex());
    }

    /// Real VLC, no window (`dummy` interface and outputs): play, pause, seek, end, close. Skips
    /// without `vlc` or `ffmpeg`.
    #[test]
    fn live_vlc_drives_playback() {
        let Some(path) = vlc_path() else {
            eprintln!("skip: vlc not installed");
            return;
        };
        let clip =
            std::env::temp_dir().join(format!("nocturne-vlc-test-{}.mp4", std::process::id()));
        let ok = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x90:rate=10:duration=8",
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=duration=8",
                "-shortest",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&clip)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("skip: ffmpeg unavailable");
            return;
        }
        let mut c = Command::new(path);
        c.args(["--intf=dummy", "--vout=dummy", "--aout=dummy"]);
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let p = VlcPlayer::start(c, port, random_hex()).unwrap();
        let wait = |what: &str, f: &dyn Fn(&State) -> bool| {
            let t = Instant::now();
            while !f(&p.state()) {
                assert!(t.elapsed() < Duration::from_secs(20), "timeout: {what}");
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        p.load(clip.to_str().unwrap(), &[]).unwrap();
        wait("loaded", &|s| s.duration.is_some() && s.position.is_some());
        let evs: Vec<_> = p.events().try_iter().collect();
        assert!(evs.contains(&Event::Loaded), "{evs:?}");
        assert!((p.state().duration.unwrap() - 8.0).abs() < 1.0);
        p.command(&["set", "pause", "yes"]).unwrap();
        wait("paused", &|s| s.paused);
        p.seek_to(5.0).unwrap();
        wait("seeked", &|s| {
            s.position.is_some_and(|t| (4.0..7.0).contains(&t))
        });
        p.seek_by(-3.0).unwrap();
        wait("seeked back", &|s| s.position.is_some_and(|t| t < 4.0));
        p.command(&["set", "pause", "no"]).unwrap();
        p.seek_to(6.0).unwrap();
        wait("eof", &|s| s.eof);
        assert!(
            p.state().position.is_some_and(|t| t > 5.0),
            "{:?}",
            p.state()
        );
        drop(p); // kills VLC
        let _ = std::fs::remove_file(&clip);
    }
}
