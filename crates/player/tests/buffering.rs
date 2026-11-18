//! Watermark buffering against a local range-capable HTTP server (std only), headless mpv child.
//! Needs `ffmpeg` and `mpv` on PATH (skips without ffmpeg). Scaled-down policy: high 30 s / low 12 s
//! on a 10 minute clip, played at 4x. `cargo test -p nocturne-player --test buffering -- --nocapture`
//! prints the numbers recorded in docs/buffering.md.

use nocturne_player::{Buffering, Options, Player};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Stats {
    requests: AtomicU64,
    bytes: AtomicU64,
}

/// Serves `path` with `Range` support; ~13 MB/s so the fill is quick but not instantaneous.
fn serve(path: std::path::PathBuf) -> (String, Arc<Stats>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/clip.mp4", l.local_addr().unwrap());
    let st = Arc::new(Stats::default());
    let st2 = st.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let (p, st) = (path.clone(), st2.clone());
            std::thread::spawn(move || {
                let _ = conn(c, &p, &st);
            });
        }
    });
    (url, st)
}

fn conn(s: TcpStream, path: &std::path::Path, st: &Stats) -> std::io::Result<()> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut w = s.try_clone()?;
    let mut r = BufReader::new(s);
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let head = line.starts_with("HEAD");
        let mut start = 0u64;
        let mut ranged = false;
        loop {
            line.clear();
            r.read_line(&mut line)?;
            if line.trim().is_empty() {
                break;
            }
            if let Some(v) = line.to_lowercase().strip_prefix("range: bytes=") {
                start = v.trim().split('-').next().unwrap().parse().unwrap_or(0);
                ranged = true;
            }
        }
        st.requests.fetch_add(1, Relaxed);
        let n = len.saturating_sub(start);
        let status = if ranged {
            "206 Partial Content"
        } else {
            "200 OK"
        };
        let mut h = format!(
            "HTTP/1.1 {status}\r\nAccept-Ranges: bytes\r\nContent-Type: video/mp4\r\nContent-Length: {n}\r\n"
        );
        if ranged {
            h += &format!("Content-Range: bytes {start}-{}/{len}\r\n", len - 1);
        }
        w.write_all((h + "\r\n").as_bytes())?;
        if head {
            continue;
        }
        f.seek(SeekFrom::Start(start))?;
        let mut buf = [0u8; 32 << 10];
        loop {
            let k = f.read(&mut buf)?;
            if k == 0 {
                break;
            }
            w.write_all(&buf[..k])?;
            st.bytes.fetch_add(k as u64, Relaxed);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn clip() -> Option<std::path::PathBuf> {
    let p = std::env::temp_dir().join("nocturne-buffering-600s.mp4");
    if p.exists() {
        return Some(p);
    }
    let tmp = p.with_extension("tmp.mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x240:rate=10:duration=600")
        .args([
            "-c:v",
            "libx264",
            "-b:v",
            "400k",
            "-g",
            "20",
            "-movflags",
            "+faststart",
        ])
        .arg(&tmp)
        .status()
        .ok()?
        .success();
    ok.then(|| {
        std::fs::rename(&tmp, &p).unwrap();
        p
    })
}

struct Report {
    max_buf: f64,
    lowest: f64,
    refills: u32,
    read_events: u32,
    server_reqs: u64,
    server_bytes: u64,
    samples: u32,
    live: Vec<(String, f64)>,
}

/// Play to media position 70 s at 4x; `pre` are options overridden after startup.
fn run(url: &str, st: &Stats, b: Buffering, pre: &[(&str, &str)]) -> Report {
    let p = Player::new(Options {
        headless: true,
        buffering: b,
        ..Default::default()
    })
    .unwrap();
    // (c) live values on the real child process, read back over IPC before anything loads.
    let names = [
        "cache-secs",
        "demuxer-readahead-secs",
        "demuxer-hysteresis-secs",
        "demuxer-max-bytes",
        "demuxer-max-back-bytes",
    ];
    let live: Vec<_> = names
        .iter()
        .map(|n| {
            (
                n.to_string(),
                p.prop(n).and_then(|v| v.as_f64()).unwrap_or(-1.0),
            )
        })
        .collect();
    assert_eq!(p.prop("cache").unwrap(), true); // yes -> true
    for (k, v) in pre {
        p.command(&["set", k, v]).unwrap();
    }
    p.command(&["set", "speed", "4"]).unwrap();
    p.load(url, &[]).unwrap();

    let t0 = Instant::now();
    let (mut max_buf, mut last_bytes, mut read_events, mut samples) = (0f64, 0u64, 0, 0);
    // Sawtooth tracking after the first fill: `armed` once the high mark was reached.
    let (mut armed, mut lowest, mut lo, mut refills) = (false, f64::MAX, f64::MAX, 0);
    while p.state().position.unwrap_or(0.0) < 70.0 {
        assert!(t0.elapsed() < Duration::from_secs(60), "timeout");
        std::thread::sleep(Duration::from_millis(50));
        let buf = p
            .prop("demuxer-cache-duration")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let tb = p
            .prop("demuxer-cache-state")
            .and_then(|v| v["total-bytes"].as_u64())
            .unwrap_or(0);
        samples += 1;
        // A "read event": the demuxer cache grew since the previous 50 ms sample.
        if tb > last_bytes && armed {
            read_events += 1;
        }
        last_bytes = tb;
        max_buf = max_buf.max(buf);
        if buf >= b.high_secs - 2.0 {
            armed = true;
        }
        if armed {
            lowest = lowest.min(buf);
            lo = lo.min(buf);
            // Rising 3 s off a trough that was well below the high mark = one refill.
            if buf > lo + 3.0 && lo < b.high_secs - 3.0 {
                refills += 1;
                lo = f64::MAX;
            }
        }
    }
    Report {
        max_buf,
        lowest,
        refills,
        read_events,
        server_reqs: st.requests.load(Relaxed),
        server_bytes: st.bytes.load(Relaxed),
        samples,
        live,
    }
}

#[test]
fn watermarks_vs_continuous_topup() {
    let Some(path) = clip() else {
        eprintln!("skip: ffmpeg unavailable");
        return;
    };
    let b = Buffering {
        high_secs: 30.0,
        low_secs: 12.0,
        ..Default::default()
    };
    let (url, st) = serve(path.clone());
    let wm = run(&url, &st, b, &[]);
    let (url, st) = serve(path);
    let old = run(&url, &st, b, &[("demuxer-hysteresis-secs", "0")]);
    // Why cache-secs is set: with cache=yes its 3600000 s default overrides the readahead.
    let (url, st) = serve(clip().unwrap());
    let nocs = run(&url, &st, b, &[("cache-secs", "3600000")]);

    for (n, r) in [
        ("watermark 30/12", &wm),
        ("hysteresis=0", &old),
        ("cache-secs default", &nocs),
    ] {
        eprintln!(
            "{n}: max_buf={:.1}s lowest={:.1}s refills={} read_events={}/{} samples http_reqs={} http_bytes={}",
            r.max_buf, r.lowest, r.refills, r.read_events, r.samples, r.server_reqs, r.server_bytes
        );
    }
    eprintln!("live options (watermark run): {:?}", wm.live);

    // (c) live values on the child mpv: readahead 30, hysteresis 12 (= low mark), 1.5 GiB, 64 MiB.
    let l: std::collections::HashMap<_, _> = wm.live.iter().cloned().collect();
    assert_eq!(l["cache-secs"], 30.0);
    assert_eq!(l["demuxer-readahead-secs"], 30.0);
    assert_eq!(l["demuxer-hysteresis-secs"], 12.0);
    assert_eq!(l["demuxer-max-bytes"], (3u64 << 29) as f64);
    assert_eq!(l["demuxer-max-back-bytes"], (64u64 << 20) as f64);
    // Stops reading near the high mark and resumes near the low mark, repeatedly.
    assert!(wm.max_buf <= 33.0, "overshoot {}", wm.max_buf);
    assert!(wm.max_buf >= 28.0, "never filled {}", wm.max_buf);
    assert!((9.0..=15.0).contains(&wm.lowest), "low mark {}", wm.lowest);
    assert!(wm.refills >= 2, "refills {}", wm.refills);
    assert!(
        nocs.max_buf > 60.0,
        "cache-secs no longer overrides readahead: {}",
        nocs.max_buf
    );
    // Old behaviour: constant top-up (far more read events, buffer pinned at the high mark).
    assert!(old.lowest >= 27.0, "old min {}", old.lowest);
    assert!(
        wm.read_events * 3 < old.read_events,
        "{} vs {}",
        wm.read_events,
        old.read_events
    );
}
