//! Everything OS-specific lives here (with `single.rs`, the player crate and the core MPRIS
//! module); the rest of the UI is OS-neutral. Linux is tested; macOS/Windows are written, not run.
//! MPRIS and the idle inhibit live in `nocturne_core::player_mpris`; the raw window handles the
//! in-window video needs are read here.
#![allow(unsafe_code)]
use iced::Task;
use iced::window::{
    self,
    raw_window_handle::{RawDisplayHandle, RawWindowHandle},
};
use std::sync::atomic::{AtomicBool, Ordering};

/// What the window system gave the video surface to attach to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handles {
    /// Raw `wl_display*` / `wl_surface*` (as addresses, so the value is `Send`).
    Wayland { display: usize, surface: usize },
    /// X11 / XWayland / anything else: the in-window video cannot attach, use the external mpv.
    Other,
}

/// Reads the raw handles of window `id`. The callback runs on the event-loop thread, which is
/// also where `update` runs, so the embedded player may be created from the result directly.
pub fn handles(id: window::Id) -> Task<Handles> {
    window::run(id, |w| {
        let win = w.window_handle().ok()?.as_raw();
        let disp = w.display_handle().ok()?.as_raw();
        Some(match (win, disp) {
            (RawWindowHandle::Wayland(s), RawDisplayHandle::Wayland(d)) => Handles::Wayland {
                display: d.display.as_ptr() as usize,
                surface: s.surface.as_ptr() as usize,
            },
            _ => Handles::Other,
        })
    })
    .map(|h| h.unwrap_or(Handles::Other))
}

/// Linux/BSD: is there no display server to connect to (no `WAYLAND_DISPLAY`, `WAYLAND_SOCKET`
/// or `DISPLAY`)? Other OSes always have one.
pub fn no_graphical_session() -> bool {
    !cfg!(any(target_os = "macos", windows))
        && no_session_in(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
}

fn no_session_in(set: impl Fn(&str) -> bool) -> bool {
    !["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DISPLAY"]
        .iter()
        .any(|k| set(k))
}

/// Per-OS window options: on Linux the Wayland app id / X11 class (matches the .desktop file).
pub fn window_settings() -> window::settings::PlatformSpecific {
    #[cfg(target_os = "linux")]
    return window::settings::PlatformSpecific {
        application_id: "nocturne".into(),
        ..Default::default()
    };
    #[cfg(not(target_os = "linux"))]
    window::settings::PlatformSpecific::default()
}

/// Set by SIGTERM/SIGINT; the app tick turns it into a normal quit.
static TERM: AtomicBool = AtomicBool::new(false);

/// Has the process been asked to terminate?
pub fn term_requested() -> bool {
    TERM.load(Ordering::Relaxed)
}

#[cfg(unix)]
extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::Relaxed);
}

/// Route SIGTERM/SIGINT to [`term_requested`].
#[cfg(unix)]
pub fn install_term_handlers() {
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as *const () as libc::sighandler_t);
    }
}

/// Route Ctrl+C / Ctrl+Break / console close to [`term_requested`]. In a GUI-subsystem build there
/// is no console, so this simply never fires.
#[cfg(windows)]
pub fn install_term_handlers() {
    unsafe extern "system" fn on_ctrl(_: u32) -> i32 {
        TERM.store(true, Ordering::Relaxed);
        1 // handled: the app tick quits normally
    }
    // SAFETY: the handler only stores to an atomic.
    unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(on_ctrl), 1) };
}

/// The now-playing poster the player wrote for desktop widgets (MPRIS is Linux only).
#[cfg(target_os = "linux")]
pub fn remove_art_file() {
    if let Some(d) = nocturne_session::paths::runtime_dir() {
        let _ = std::fs::remove_file(d.join(format!("nocturne-art-{}.img", std::process::id())));
    }
}

/// The poster for the desktop's media widgets: a file in the runtime dir (D-Bus clients cannot
/// send our auth header). Returns its `file://` URL.
#[cfg(target_os = "linux")]
pub fn write_art(bytes: &[u8]) -> Option<String> {
    let dir = nocturne_session::paths::runtime_dir()?;
    let p = dir.join(format!("nocturne-art-{}.img", std::process::id()));
    std::fs::write(&p, bytes).ok()?;
    Some(format!("file://{}", p.display()))
}

#[cfg(not(target_os = "linux"))]
pub fn remove_art_file() {}

#[cfg(not(target_os = "linux"))]
pub fn write_art(_: &[u8]) -> Option<String> {
    None
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

// ponytail: macOS/Windows accessibility "reduce motion" settings are not read yet.
#[cfg(not(target_os = "linux"))]
fn detect_reduced_motion() -> bool {
    false
}

#[cfg(target_os = "linux")]
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
    // `nocturne` is the last component paths::config_dir() adds; kdeglobals sits one level up.
    let home =
        nocturne_session::paths::config_dir().and_then(|d| d.parent().map(|p| p.to_path_buf()));
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
    fn display_detection() {
        assert!(no_session_in(|_| false));
        assert!(!no_session_in(|k| k == "DISPLAY"));
        assert!(!no_session_in(|k| k == "WAYLAND_DISPLAY"));
    }

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
