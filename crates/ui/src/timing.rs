//! Launch timing for QA: `NOCTURNE_TIMING=1` prints each mark once, in ms since the process
//! started, to stderr. Costs one env lookup when off.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("NOCTURNE_TIMING").is_some())
}

pub fn enabled() -> bool {
    on()
}

/// Print `label` with the time since process start.
pub fn mark(label: &str) {
    if on() {
        eprintln!(
            "timing: {label:<14} {:>7.1} ms",
            crate::platform::since_start().as_secs_f64() * 1e3
        );
    }
}

/// First painted frame: the first `window::frames()` tick after the window shows. One shot.
pub fn first_frame_pending() -> bool {
    on() && !FIRST.load(Ordering::Relaxed)
}

pub fn first_frame() {
    if !FIRST.swap(true, Ordering::Relaxed) {
        mark("first frame");
    }
}

static FIRST: AtomicBool = AtomicBool::new(false);

/// The signed-in shell is usable: header, libraries and Home are up. One shot.
pub fn shell_ready() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if !DONE.swap(true, Ordering::Relaxed) {
        mark("shell ready");
    }
}
