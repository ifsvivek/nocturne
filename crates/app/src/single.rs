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
//! Linux and macOS: flock + unix socket (macOS keeps the socket in `$TMPDIR`). Windows: a named
//! mutex decides who is first and a named pipe carries "activate"; both are named after the same
//! hash of the config dir. Only the Linux variant is tested; macOS/Windows are written, not run.
use iced::Task;
use iced::window::{self, UserAttention};
use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVATE: AtomicBool = AtomicBool::new(false);

/// Is an activation waiting (without consuming it)?
pub fn activate_pending() -> bool {
    ACTIVATE.load(Ordering::Relaxed)
}

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
    #[cfg(unix)]
    _lock: std::fs::File,
    #[cfg(unix)]
    sock: Option<std::path::PathBuf>,
    #[cfg(windows)]
    mutex: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(unix)]
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
    let Some(cfg) = nocturne_session::paths::config_dir() else {
        return Some(Guard::none());
    };
    #[cfg(unix)]
    {
        imp::acquire_in(&cfg, nocturne_session::paths::runtime_dir().as_deref())
    }
    #[cfg(windows)]
    {
        win::acquire_in(&cfg)
    }
}

impl Guard {
    #[cfg(unix)]
    fn none() -> Self {
        // /dev/null stands in for a lock nobody needs.
        Guard {
            _lock: std::fs::File::open("/dev/null").expect("/dev/null"),
            sock: None,
        }
    }
    #[cfg(windows)]
    fn none() -> Self {
        Guard {
            mutex: std::ptr::null_mut(),
        }
    }
}

/// Stable hash of the config dir (FNV-1a: unlike `DefaultHasher` it is the same across builds), so
/// every instance of one config dir derives the same socket / mutex / pipe name.
fn cfg_hash(cfg: &std::path::Path) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in cfg.as_os_str().as_encoded_bytes() {
        h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::time::Duration;

    /// Socket path from a stable hash of the config dir; `None` if it would not fit `sun_path`.
    fn sock_path(cfg: &Path, run: Option<&Path>) -> Option<std::path::PathBuf> {
        let h = cfg_hash(cfg);
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

#[cfg(windows)]
mod win {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::io::FromRawHandle;
    use std::path::Path;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;

    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.mutex.is_null() {
                // SAFETY: a handle we created and own.
                unsafe { CloseHandle(self.mutex) };
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn mutex_name(cfg: &Path) -> String {
        format!("Local\\nocturne-{:016x}", cfg_hash(cfg))
    }

    pub fn pipe_name(cfg: &Path) -> String {
        format!(r"\\.\pipe\nocturne-{:016x}", cfg_hash(cfg))
    }

    pub fn acquire_in(cfg: &Path) -> Option<Guard> {
        let name = wide(&mutex_name(cfg));
        // SAFETY: plain Win32 call with a NUL-terminated name.
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if mutex.is_null() {
            return Some(Guard::none()); // cannot decide: run rather than refuse to start
        }
        // SAFETY: reads the thread's last error right after the call above.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: our own handle.
            unsafe { CloseHandle(mutex) };
            match forward(&pipe_name(cfg)) {
                true => {}
                false => eprintln!("nocturne: already running, but it did not answer"),
            }
            return None;
        }
        let pipe = pipe_name(cfg);
        std::thread::spawn(move || serve(&pipe));
        Some(Guard { mutex })
    }

    /// One pipe instance at a time: wait for a client, read its line, answer, start over.
    // ponytail: a local client that connects and never writes parks this thread (no read timeout
    // like the unix socket has); overlapped I/O with a timeout if that ever matters.
    fn serve(pipe: &str) {
        let name = wide(pipe);
        loop {
            // SAFETY: plain Win32 call with a NUL-terminated name; default security (same user).
            let h = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_WAIT,
                    1,
                    512,
                    512,
                    0,
                    std::ptr::null(),
                )
            };
            if h == INVALID_HANDLE_VALUE {
                return;
            }
            // Fails with ERROR_PIPE_CONNECTED when the client was faster: still connected.
            // SAFETY: valid pipe handle, synchronous (no OVERLAPPED).
            unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
            // SAFETY: we own `h`; the File closes it on drop.
            let mut f = unsafe { File::from_raw_handle(h as _) };
            let mut line = String::new();
            if BufReader::new(&f).read_line(&mut line).is_ok() && line.trim() == "activate" {
                ACTIVATE.store(true, Ordering::Relaxed);
                let _ = f.write_all(b"ok\n");
                let _ = f.sync_all(); // FlushFileBuffers: waits until the client has read the ack
            }
        }
    }

    /// The holder may still be between the mutex and the pipe: retry briefly.
    fn forward(pipe: &str) -> bool {
        for _ in 0..20 {
            if let Ok(mut f) = OpenOptions::new().read(true).write(true).open(pipe) {
                let mut ack = String::new();
                return f.write_all(b"activate\n").is_ok()
                    && BufReader::new(f).read_line(&mut ack).is_ok();
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn names_follow_the_config_dir() {
            let a = Path::new(r"C:\Users\a\AppData\Roaming\nocturne");
            let b = Path::new(r"C:\other");
            assert_eq!(mutex_name(a), mutex_name(a));
            assert_ne!(mutex_name(a), mutex_name(b));
            assert!(pipe_name(a).starts_with(r"\\.\pipe\nocturne-"));
        }
    }
}
