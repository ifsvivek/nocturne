//! Manual: windowed gpu-next start/stop cycles on a generated clip. Needs a display.
//! Run: `cargo test -p nocturne-player --test windowed -- --ignored`.

use nocturne_player::{Options, Player};
use std::time::{Duration, Instant};

#[test]
#[ignore]
fn windowed_cycles_exit_cleanly() {
    let clip = std::env::temp_dir().join(format!("nocturne-win-{}.mkv", std::process::id()));
    let ok = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .args(["testsrc2=size=1280x720:rate=30:duration=3"])
        .arg(&clip)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg");
    for i in 0..10 {
        let p = Player::new(Options::default()).unwrap();
        let pid = p.child_pid().unwrap();
        p.load(clip.to_str().unwrap(), &[]).unwrap();
        let t = Instant::now();
        while !p.state().position.is_some_and(|x| x > 0.3) {
            assert!(
                t.elapsed() < Duration::from_secs(15),
                "cycle {i}: no playback"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let t = Instant::now();
        drop(p);
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "cycle {i}: child left"
        );
        println!("cycle {i} ok, drop took {:?}", t.elapsed());
    }
    let _ = std::fs::remove_file(clip);
}
