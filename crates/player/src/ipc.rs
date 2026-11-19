//! Transport of the external player's JSON IPC: a unix socket on Linux/macOS, a named pipe on
//! Windows (`mpv --input-ipc-server` accepts both). Everything else in `lib.rs` is transport-neutral.
use std::path::{Path, PathBuf};

/// Windows pipe name. Pure so the naming is tested on every host. `seq` is the mpv instance of
/// this player: a respawn gets a fresh name, so it never meets a predecessor still shutting down.
pub fn pipe_name(pid: u32, n: u64, seq: u64) -> String {
    format!(r"\\.\pipe\nocturne-mpv-{pid}-{n}-{seq}")
}

/// Unix socket path inside the private per-player directory.
pub fn socket_path(dir: &Path, seq: u64) -> PathBuf {
    dir.join(format!("mpv{seq}.sock"))
}

/// The `--input-ipc-server` value for this OS.
pub fn endpoint(dir: &Path, pid: u32, n: u64, seq: u64) -> String {
    if cfg!(windows) {
        pipe_name(pid, n, seq)
    } else {
        socket_path(dir, seq).display().to_string()
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

pub use imp::{Stream, clear, connect, private_dir, reader, shutdown};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_names() {
        assert_eq!(pipe_name(42, 3, 1), r"\\.\pipe\nocturne-mpv-42-3-1");
        assert_eq!(
            socket_path(Path::new("/run/user/1000/nocturne-mpv-42-3"), 2),
            Path::new("/run/user/1000/nocturne-mpv-42-3/mpv2.sock")
        );
        let ep = endpoint(Path::new("/x"), 42, 3, 0);
        if cfg!(windows) {
            assert!(ep.starts_with(r"\\.\pipe\"));
        } else {
            assert_eq!(ep, "/x/mpv0.sock");
        }
    }
}

/// Windows only: a fake mpv pipe server, to prove the client side the app depends on (connect,
/// a reader thread polling while the owner writes, partial reads, request ids) without mpv.
#[cfg(all(test, windows))]
mod pipe_tests {
    use super::*;
    use std::ffi::c_void;
    use std::fs::File;
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::io::FromRawHandle;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            open: u32,
            mode: u32,
            max: u32,
            out: u32,
            inn: u32,
            timeout: u32,
            sec: *const c_void,
        ) -> *mut c_void;
        fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
    }

    #[test]
    fn requests_and_split_replies_round_trip_over_a_named_pipe() {
        let name = pipe_name(std::process::id(), 9999, 0);
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: plain Win32 calls; the handle is owned by the `File` made from it.
        let server = unsafe {
            // PIPE_ACCESS_DUPLEX, byte mode, one instance
            let h = CreateNamedPipeW(wide.as_ptr(), 3, 0, 1, 4096, 4096, 0, std::ptr::null());
            assert!(h as isize != -1, "pipe created");
            h as usize
        };
        let srv = std::thread::spawn(move || {
            let h = server as *mut c_void;
            // SAFETY: as above.
            unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
            let mut f = unsafe { File::from_raw_handle(h) };
            let mut line = String::new();
            BufReader::new(&f).read_line(&mut line).unwrap();
            let id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"]
                .as_u64()
                .unwrap();
            let reply =
                format!("{{\"data\":\"matroska\",\"request_id\":{id},\"error\":\"success\"}}\n");
            let (a, b) = reply.split_at(10);
            f.write_all(b"{\"event\":\"idle\"}\n").unwrap();
            f.write_all(a.as_bytes()).unwrap(); // half a line
            std::thread::sleep(std::time::Duration::from_millis(60));
            f.write_all(b.as_bytes()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(200));
        });

        let mut w = connect(&name).expect("connect to the fake mpv");
        let rd = reader(&w).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let t = std::thread::spawn(move || {
            for l in BufReader::new(rd).lines() {
                let _ = tx.send(l.unwrap());
            }
        });
        // The reader is already polling when this write happens: it must not be starved or blocked.
        w.write_all(b"{\"command\":[\"get_property\",\"file-format\"],\"request_id\":5}\n")
            .unwrap();
        let first = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(first.contains("idle"));
        let second = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&second).unwrap();
        assert_eq!(
            (v["request_id"].as_u64(), v["data"].as_str()),
            (Some(5), Some("matroska"))
        );
        srv.join().unwrap();
        // The server end closed: the polling reader reports EOF instead of hanging.
        t.join().unwrap();
    }
}
