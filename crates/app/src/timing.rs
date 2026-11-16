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

/// Scroll cadence for QA: the gap between consecutive scroll events of a page while it is being
/// scrolled is the time between rendered frames. `NOCTURNE_TIMING=1` prints a summary at exit.
static GAPS: std::sync::Mutex<(Option<std::time::Instant>, Vec<f32>)> =
    std::sync::Mutex::new((None, Vec::new()));

pub fn scroll_event() {
    if !on() {
        return;
    }
    let now = std::time::Instant::now();
    if let Ok(mut g) = GAPS.lock() {
        if let Some(prev) = g.0
            && now.duration_since(prev).as_millis() < 250
        {
            g.1.push(now.duration_since(prev).as_secs_f32() * 1e3);
        }
        g.0 = Some(now);
    }
}

/// Print count / mean / p95 / max of the scroll gaps (ms) and how many exceeded two 60 Hz frames.
pub fn report() {
    let Ok(g) = GAPS.lock() else { return };
    let mut v = g.1.clone();
    if !on() || v.is_empty() {
        return;
    }
    v.sort_by(f32::total_cmp);
    let mean = v.iter().sum::<f32>() / v.len() as f32;
    let p95 = v[(v.len() * 95 / 100).min(v.len() - 1)];
    let slow = v.iter().filter(|g| **g > 33.4).count();
    eprintln!(
        "timing: scroll gaps n={} mean={mean:.1} ms p95={p95:.1} ms max={:.1} ms over-33ms={slow}",
        v.len(),
        v[v.len() - 1]
    );
}
