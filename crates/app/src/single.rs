//! One running instance per config dir.
//!
//! The first instance takes an exclusive `flock` on `$XDG_CONFIG_HOME/nocturne/instance.lock` (the
//! kernel drops it on any exit, so crashes leave nothing stale) and listens on a unix socket in
//! `$XDG_RUNTIME_DIR` for "activate". A later instance with the same config dir sends that line and
//! exits before any window or GPU context exists. Different config dirs (isolated test runs, two
//! SyncPlay clients) do not conflict. Debug previews (`NOCTURNE_PREVIEW`) bypass the guard.
//!
//! Root hook (the only thing the app needs), in `App::tick`:
//! `if crate::single::take_activate() { return crate::single::bring_forward(); }`
//!
//! Linux only for now; other OSes compile to "always first, never activated".
use iced::Task;
use iced::window::{self, UserAttention};
use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVATE: AtomicBool = AtomicBool::new(false);

/// Did another launch ask us to come forward since the last call?
pub fn take_activate() -> bool {
    ACTIVATE.swap(false, Ordering::Relaxed)
}

/// Un-minimise and focus the main window; where the compositor refuses focus stealing (Wayland)
/// the attention request is what the user sees.
pub fn bring_forward<T: Send + 'static>() -> Task<T> {
    window::oldest().and_then(|id| {
        Task::batch([
            window::minimize(id, false),
            window::gain_focus(id),
            window::request_user_attention(id, Some(UserAttention::Informational)),
        ])
    })
}

/// Held for the life of the process; dropping it removes the socket (the lock goes with the fd).
pub struct Guard {
    #[cfg(target_os = "linux")]
    _lock: std::fs::File,
    #[cfg(target_os = "linux")]
    sock: Option<std::path::PathBuf>,
}

#[cfg(target_os = "linux")]
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(p) = &self.sock {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// `Some(guard)`: we are the instance, keep the guard. `None`: another one was told, just exit.
pub fn acquire() -> Option<Guard> {
    if std::env::var_os("NOCTURNE_PREVIEW").is_some() {
        return Some(Guard::none());
    }
    #[cfg(target_os = "linux")]
    {
        let Some(cfg) = config_dir() else {
            return Some(Guard::none());
        };
        let run = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
        imp::acquire_in(&cfg, run.as_deref())
    }
    #[cfg(not(target_os = "linux"))]
    Some(Guard::none())
}

impl Guard {
    #[cfg(target_os = "linux")]
    fn none() -> Self {
        // /dev/null stands in for a lock nobody needs.
        Guard {
            _lock: std::fs::File::open("/dev/null").expect("/dev/null"),
            sock: None,
        }
    }
    #[cfg(not(target_os = "linux"))]
    fn none() -> Self {
        Guard {}
    }
}

#[cfg(target_os = "linux")]
fn config_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
    Some(base.join("nocturne"))
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::time::Duration;

    /// Socket path from a stable hash of the config dir; `None` if it would not fit `sun_path`.
    fn sock_path(cfg: &Path, run: Option<&Path>) -> Option<std::path::PathBuf> {
        let mut h: u64 = 0xcbf29ce484222325; // FNV-1a: stable across builds, unlike DefaultHasher
        for b in cfg.as_os_str().as_encoded_bytes() {
            h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
        }
        let p = run.unwrap_or(cfg).join(format!("nocturne-{h:016x}.sock"));
        (p.as_os_str().len() < 100).then_some(p)
    }

    pub fn acquire_in(cfg: &Path, run: Option<&Path>) -> Option<Guard> {
        // Cannot lock (read-only config dir...): run unguarded rather than refuse to start.
        let Ok(lock) = std::fs::create_dir_all(cfg).and_then(|()| {
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(cfg.join("instance.lock"))
        }) else {
            return Some(Guard::none());
        };
        // SAFETY: plain flock on an fd we own.
        let got = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        let sock = sock_path(cfg, run);
        if !got {
            match sock.as_deref().map(forward) {
                Some(true) => {}
                Some(false) => eprintln!("nocturne: already running, but it did not answer"),
                None => eprintln!("nocturne: already running"),
            }
            return None;
        }
        let sock = sock.and_then(|p| {
            let _ = std::fs::remove_file(&p); // we hold the lock, so any socket here is stale
            let l = UnixListener::bind(&p).ok()?;
            std::thread::spawn(move || {
                for mut c in l.incoming().flatten() {
                    let _ = c.set_read_timeout(Some(Duration::from_secs(1)));
                    let mut line = String::new();
                    if BufReader::new(&mut c).read_line(&mut line).is_ok()
                        && line.trim() == "activate"
                    {
                        ACTIVATE.store(true, Ordering::Relaxed);
                        let _ = c.write_all(b"ok\n");
                    }
                }
            });
            Some(p)
        });
        Some(Guard { _lock: lock, sock })
    }

    /// The holder may still be between `flock` and `bind`: retry briefly.
    fn forward(p: &Path) -> bool {
        for _ in 0..20 {
            if let Ok(mut s) = UnixStream::connect(p) {
                let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
                let mut ack = String::new();
                return s.write_all(b"activate\n").is_ok()
                    && BufReader::new(s).read_line(&mut ack).is_ok();
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn second_instance_forwards_then_stale_lock_is_reclaimed() {
            let d = tempfile::tempdir().unwrap();
            let cfg = d.path().join("cfg");
            let run = d.path().join("run");
            std::fs::create_dir_all(&run).unwrap();
            let first = acquire_in(&cfg, Some(&run)).expect("first runs");
            assert!(!take_activate());
            // Separate open file description: conflicts even inside one process.
            assert!(acquire_in(&cfg, Some(&run)).is_none(), "second must defer");
            assert!(take_activate(), "first was told to activate");
            // Another config dir is independent.
            assert!(acquire_in(&d.path().join("other"), Some(&run)).is_some());
            // "Crash": lock freed, socket file left behind.
            let sock = sock_path(&cfg, Some(&run)).unwrap();
            drop(first);
            std::fs::write(&sock, b"stale").unwrap();
            assert!(
                acquire_in(&cfg, Some(&run)).is_some(),
                "stale socket reclaimed"
            );
        }

        #[test]
        fn long_paths_skip_the_socket() {
            let long = Path::new("/").join("x".repeat(120));
            assert!(sock_path(&long, None).is_none());
        }
    }
}
