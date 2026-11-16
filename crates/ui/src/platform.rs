//! Everything OS-specific lives here (Linux only for now); the rest of the UI is OS-neutral.
//! Later steps add raw window/display handle extraction and the MPRIS hookup to this module.
#![allow(unsafe_code)]
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by SIGTERM/SIGINT; the app tick turns it into a normal quit.
static TERM: AtomicBool = AtomicBool::new(false);

/// Has the process been asked to terminate?
pub fn term_requested() -> bool {
    TERM.load(Ordering::Relaxed)
}

#[cfg(target_os = "linux")]
extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::Relaxed);
}

/// Route SIGTERM/SIGINT to [`term_requested`].
#[cfg(target_os = "linux")]
pub fn install_term_handlers() {
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as *const () as libc::sighandler_t);
    }
}

/// The now-playing poster the player wrote for desktop widgets.
#[cfg(target_os = "linux")]
pub fn remove_art_file() {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        let _ = std::fs::remove_file(
            std::path::Path::new(&d).join(format!("nocturne-art-{}.img", std::process::id())),
        );
    }
}

/// Desktop "reduce animations" preference, probed on a thread started at launch so the (slow-ish)
/// `gsettings` call never delays the first frame.
static SYSTEM_REDUCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn probe_motion_pref() {
    std::thread::spawn(|| {
        let _ = SYSTEM_REDUCED.set(detect_reduced_motion());
    });
}

/// The probe's answer; unavailable (or not back within 200 ms) means motion stays on.
pub fn system_reduces_motion() -> bool {
    for _ in 0..40 {
        if let Some(v) = SYSTEM_REDUCED.get() {
            return *v;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

fn detect_reduced_motion() -> bool {
    let gnome = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| gnome_reduces(&String::from_utf8_lossy(&o.stdout)));
    if let Some(g) = gnome {
        return g;
    }
    let home = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")));
    home.and_then(|d| std::fs::read_to_string(d.join("kdeglobals")).ok())
        .is_some_and(|t| kde_reduces(&t))
}

/// `gsettings get ... enable-animations` prints `true` / `false`.
fn gnome_reduces(out: &str) -> bool {
    out.trim() == "false"
}

/// `AnimationDurationFactor=0` in the `[KDE]` group of `kdeglobals` turns animations off.
fn kde_reduces(text: &str) -> bool {
    let mut in_kde = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_kde = line == "[KDE]";
        } else if in_kde && let Some(v) = line.strip_prefix("AnimationDurationFactor=") {
            return v.trim().parse::<f32>().is_ok_and(|f| f <= 0.0);
        }
    }
    false
}

static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Called first thing in `main`: the reference for the startup timing numbers.
pub fn mark_start() {
    let _ = START.set(std::time::Instant::now());
}

pub fn since_start() -> std::time::Duration {
    START
        .get()
        .map_or(std::time::Duration::ZERO, |s| s.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_and_kde_detection() {
        assert!(gnome_reduces("false\n") && !gnome_reduces("true\n") && !gnome_reduces(""));
        assert!(kde_reduces(
            "[General]\nx=1\n[KDE]\nAnimationDurationFactor=0\n"
        ));
        assert!(kde_reduces("[KDE]\nAnimationDurationFactor=0.0"));
        assert!(!kde_reduces("[KDE]\nAnimationDurationFactor=0.5\n"));
        assert!(!kde_reduces("[Other]\nAnimationDurationFactor=0\n"));
        assert!(!kde_reduces(""));
    }
}
