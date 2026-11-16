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
