//! Headless smoke test: plays a generated 2s WAV with vo=null/ao=null. No window, no network.
// Unix only: the tests kill the child with `kill -9` and read its command line with `ps`; Windows is covered by the CI smoke test of the installed app.
#![cfg(unix)]

use nocturne_player::{Event, Options, Player};
use std::time::{Duration, Instant};

fn wav(secs: u32) -> Vec<u8> {
    let rate = 8000u32;
    let n = rate * secs;
    let mut v = Vec::new();
    v.extend(b"RIFF");
    v.extend((36 + n).to_le_bytes());
    v.extend(b"WAVEfmt ");
    v.extend(16u32.to_le_bytes());
    v.extend(1u16.to_le_bytes()); // PCM
    v.extend(1u16.to_le_bytes()); // mono
    v.extend(rate.to_le_bytes());
    v.extend(rate.to_le_bytes()); // byte rate (8-bit)
    v.extend(1u16.to_le_bytes());
    v.extend(8u16.to_le_bytes());
    v.extend(b"data");
    v.extend(n.to_le_bytes());
    v.extend((0..n).map(|i| 128 + ((i / 20 % 2) * 20) as u8));
    v
}

fn wait(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(10), "timeout: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn load_seek_pause_eof() {
    let path = std::env::temp_dir().join(format!("nocturne-smoke-{}.wav", std::process::id()));
    std::fs::write(&path, wav(2)).unwrap();
    let p = Player::new(Options {
        headless: true,
        ..Default::default()
    })
    .unwrap();
    assert!(p.state().idle);

    p.load(path.to_str().unwrap(), &["X-Test: a, b"]).unwrap();
    wait("duration", || p.state().duration.is_some());
    let d = p.state().duration.unwrap();
    assert!((1.9..=2.1).contains(&d), "duration {d}");
    assert!(!p.state().idle);

    p.pause().unwrap();
    p.seek_to(1.0).unwrap();
    // While paused, mpv reports the audio clock a little behind the target.
    wait("seek", || {
        p.state().position.is_some_and(|x| (0.7..=1.1).contains(&x))
    });
    wait("paused", || p.state().paused);
    let before = p.state().position.unwrap();
    p.seek_by(-0.5).unwrap();
    wait("seek_by", || {
        p.state().position.is_some_and(|x| x < before - 0.3)
    });
    p.toggle().unwrap();
    wait("unpaused", || !p.state().paused);

    p.set_volume(50.0).unwrap();
    wait("volume", || p.state().volume == 50.0);

    p.seek_to(1.5).unwrap();
    wait("eof", || p.state().eof);
    let evs: Vec<Event> = p.events().try_iter().collect();
    assert!(evs.contains(&Event::Loaded));
    assert!(evs.contains(&Event::Paused(true)));
    assert!(evs.contains(&Event::Eof));

    p.stop().unwrap();
    wait("idle", || p.state().idle);
    drop(p);
    let _ = std::fs::remove_file(path);
}

#[test]
fn headers_reach_server_and_not_url() {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/x.wav", l.local_addr().unwrap());
    let srv = std::thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = c.read(&mut buf).unwrap();
        let _ = c.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        String::from_utf8_lossy(&buf[..n]).into_owned()
    });
    let p = Player::new(Options {
        headless: true,
        ..Default::default()
    })
    .unwrap();
    // Two header lines, both with commas in the value: each arrives whole (no list splitting).
    p.load(
        &url,
        &["Authorization: Tok A=\"b\", C=\"d\"", "X-Gate: one, two"],
    )
    .unwrap();
    let req = srv.join().unwrap();
    assert!(req.contains("Authorization: Tok A=\"b\", C=\"d\"\r\n"));
    assert!(req.contains("X-Gate: one, two\r\n"));
    assert!(!url.contains("Tok"));
    wait("failure event", || {
        p.events().try_iter().any(|e| matches!(e, Event::Failed(_)))
    });
}

#[test]
fn killed_child_emits_closed_and_next_load_respawns() {
    let path = std::env::temp_dir().join(format!("nocturne-kill-{}.wav", std::process::id()));
    std::fs::write(&path, wav(2)).unwrap();
    let p = Player::new(Options {
        headless: true,
        ..Default::default()
    })
    .unwrap();
    wait("spawned", || p.child_pid().is_some()); // mpv starts on a worker thread
    let old = p.child_pid().unwrap();
    assert!(
        std::process::Command::new("kill")
            .args(["-9", &old.to_string()])
            .status()
            .unwrap()
            .success()
    );
    wait("closed", || {
        p.events().try_iter().any(|e| e == Event::Closed)
    });
    assert!(p.state().idle);

    p.load(path.to_str().unwrap(), &[]).unwrap();
    wait("respawned", || p.child_pid().is_some_and(|n| n != old));
    wait("duration after respawn", || p.state().duration.is_some());
    drop(p);
    let _ = std::fs::remove_file(path);
}

#[test]
fn auth_header_not_on_child_command_line() {
    let p = Player::new(Options {
        headless: true,
        ..Default::default()
    })
    .unwrap();
    p.load("/nonexistent.wav", &["Authorization: SECRET-TOKEN-123"])
        .unwrap();
    wait("failure", || {
        p.events().try_iter().any(|e| matches!(e, Event::Failed(_)))
    });
    // `ps` instead of /proc: macOS has no /proc, and the check must hold there too.
    let out = std::process::Command::new("ps")
        .args([
            "-ww",
            "-o",
            "args=",
            "-p",
            &p.child_pid().unwrap().to_string(),
        ])
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(!s.contains("SECRET-TOKEN") && !s.contains("Authorization"));
    assert!(s.contains("--input-ipc-server="));
}

/// The UI reads properties without waiting: they come from the cache, which `Loaded` guarantees
/// already holds the track list, and an unseen property is observed on first use.
#[test]
fn props_come_from_the_cache_without_blocking() {
    let path = std::env::temp_dir().join(format!("nocturne-prop-{}.wav", std::process::id()));
    std::fs::write(&path, wav(2)).unwrap();
    let p = Player::new(Options {
        headless: true,
        ..Default::default()
    })
    .unwrap();
    p.load(path.to_str().unwrap(), &[]).unwrap();
    wait("loaded", || {
        p.events().try_iter().any(|e| e == Event::Loaded)
    });
    assert!(
        p.prop("track-list").is_some_and(|v| v.is_array()),
        "ready at Loaded"
    );
    wait("lazy property", || {
        p.prop("audio-params/samplerate").is_some()
    });
    drop(p);
    let _ = std::fs::remove_file(path);
}
