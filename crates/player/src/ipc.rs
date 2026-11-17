//! Transport of the external player's JSON IPC: a unix socket on Linux/macOS, a named pipe on
//! Windows (`mpv --input-ipc-server` accepts both). Everything else in `lib.rs` is transport-neutral.
use std::path::{Path, PathBuf};

/// Windows pipe name. Pure so the naming is tested on every host.
pub fn pipe_name(pid: u32, n: u64) -> String {
    format!(r"\\.\pipe\nocturne-mpv-{pid}-{n}")
}

/// Unix socket path inside the private per-player directory.
pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join("mpv.sock")
}

/// The `--input-ipc-server` value for this OS.
pub fn endpoint(dir: &Path, pid: u32, n: u64) -> String {
    if cfg!(windows) {
        pipe_name(pid, n)
    } else {
        socket_path(dir).display().to_string()
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io;
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::net::UnixStream;

    pub type Stream = UnixStream;
    pub type Reader = UnixStream;

    pub fn private_dir(dir: &Path) -> io::Result<()> {
        std::fs::DirBuilder::new().mode(0o700).create(dir)
    }
    /// Removes a stale socket before mpv binds it.
    pub fn clear(ep: &str) {
        let _ = std::fs::remove_file(ep);
    }
    pub fn connect(ep: &str) -> io::Result<Stream> {
        let s = UnixStream::connect(ep)?;
        let _ = s.set_write_timeout(Some(std::time::Duration::from_secs(1)));
        Ok(s)
    }
    pub fn reader(s: &Stream) -> io::Result<Reader> {
        s.try_clone()
    }
    /// Unblocks the reader thread.
    pub fn shutdown(s: &Stream) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read};
    use std::os::windows::io::AsRawHandle;

    pub type Stream = File;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            pipe: *mut c_void,
            buf: *mut c_void,
            size: u32,
            read: *mut u32,
            avail: *mut u32,
            left: *mut u32,
        ) -> i32;
    }

    /// A synchronous pipe handle serialises I/O: a blocked `ReadFile` would stall every write on
    /// the same pipe. So this only calls `read` once `PeekNamedPipe` says bytes are waiting.
    // ponytail: 10 ms poll in the reader thread; overlapped I/O if the idle wakeups ever matter.
    pub struct Reader(File);

    impl Read for Reader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            loop {
                let mut avail = 0u32;
                // SAFETY: valid pipe handle; only `avail` is written.
                let ok = unsafe {
                    PeekNamedPipe(
                        self.0.as_raw_handle(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        &mut avail,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    return Ok(0); // broken pipe: mpv is gone, report EOF
                }
                if avail > 0 {
                    return self.0.read(buf);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }

    pub fn private_dir(dir: &Path) -> io::Result<()> {
        std::fs::create_dir(dir) // the per-user temp dir is already private; the pipe needs no file
    }
    pub fn clear(_: &str) {}
    pub fn connect(ep: &str) -> io::Result<Stream> {
        OpenOptions::new().read(true).write(true).open(ep)
    }
    pub fn reader(s: &Stream) -> io::Result<Reader> {
        s.try_clone().map(Reader)
    }
    pub fn shutdown(_: &Stream) {}
}

pub use imp::{Reader, Stream, clear, connect, private_dir, reader, shutdown};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_names() {
        assert_eq!(pipe_name(42, 3), r"\\.\pipe\nocturne-mpv-42-3");
        assert_eq!(
            socket_path(Path::new("/run/user/1000/nocturne-mpv-42-3")),
            Path::new("/run/user/1000/nocturne-mpv-42-3/mpv.sock")
        );
        let ep = endpoint(Path::new("/x"), 42, 3);
        if cfg!(windows) {
            assert!(ep.starts_with(r"\\.\pipe\"));
        } else {
            assert_eq!(ep, "/x/mpv.sock");
        }
    }
}
