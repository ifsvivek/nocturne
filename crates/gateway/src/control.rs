//! The control socket (`$XDG_RUNTIME_DIR/nocturne/gateway.sock`, mode 0600 in a 0700 directory, peer uid checked) and the single-instance lock. The app drives status, reload, stop, device list and revocation, Quick Connect approval and password regeneration through it. Protocol types: `nocturne_core::gateway`.
use crate::config::{Paths, private_dir};
use crate::state::State;
use nocturne_core::gateway::{MAX_LINE, Request, Response};
use std::io;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Hold the returned file for the life of the process: it holds an exclusive `flock`, so a second gateway fails here instead of fighting for the port.
#[cfg(unix)]
pub fn acquire_lock(paths: &Paths) -> io::Result<Option<std::fs::File>> {
    use std::os::unix::io::AsRawFd;
    let (Some(dir), Some(lock)) = (&paths.runtime, paths.lock()) else {
        return Ok(None);
    };
    private_dir(dir)?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock)?;
    // SAFETY: flock on a descriptor we own.
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = io::Error::last_os_error();
        return Err(if e.kind() == io::ErrorKind::WouldBlock {
            io::Error::new(
                io::ErrorKind::AddrInUse,
                "another nocturne-gateway is already running",
            )
        } else {
            e
        });
    }
    Ok(Some(f))
}

#[cfg(not(unix))]
pub fn acquire_lock(_: &Paths) -> io::Result<Option<std::fs::File>> {
    Ok(None)
}

/// Run one request. The second value asks the caller to stop the server after the answer is written.
pub async fn handle(st: &State, req: Request) -> (Response, bool) {
    let r = match req {
        Request::Status => Response::Status(Box::new(st.status())),
        Request::Reload => match st.reload().await {
            Ok(()) => Response::Ok,
            Err(e) => Response::Err(e),
        },
        Request::Stop => return (Response::Ok, true),
        Request::Tokens => Response::Tokens(st.tokens.list()),
        Request::Revoke(id) => {
            if st.tokens.revoke(&id) {
                Response::Ok
            } else {
                Response::Err("no such signed-in device".into())
            }
        }
        Request::ApproveQuickConnect(code) => {
            if st.qc.approve(&code, Instant::now()) {
                Response::Ok
            } else {
                Response::Err("no pending request with that code".into())
            }
        }
        Request::RegeneratePassword => match st.users.regenerate().await {
            Ok(pw) => {
                st.tokens.revoke_all();
                Response::Password(pw)
            }
            Err(e) => Response::Err(format!("could not save the new password: {e}")),
        },
        Request::RevealPassword => match st.users.take_unseen() {
            Some(pw) => Response::Password(pw),
            None => Response::Err("the password was already shown; regenerate it".into()),
        },
    };
    (r, false)
}

#[cfg(unix)]
pub fn spawn(st: &State) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let (Some(dir), Some(sock)) = (st.paths.runtime.clone(), st.paths.socket()) else {
        return Ok(());
    };
    private_dir(&dir)?;
    let _ = std::fs::remove_file(&sock); // stale from a crashed run; the instance lock says nobody else owns it
    let l = tokio::net::UnixListener::bind(&sock)?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))?;
    let st = st.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = st.stopped() => return,
                r = l.accept() => {
                    let Ok((s, _)) = r else { continue };
                    // SAFETY: geteuid has no preconditions.
                    let me = unsafe { libc::geteuid() };
                    if s.peer_cred().map_or(true, |c| c.uid() != me) {
                        continue;
                    }
                    let st = st.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(30), serve(st, s)).await;
                    });
                }
            }
        }
    });
    Ok(())
}

#[cfg(not(unix))]
pub fn spawn(_: &State) -> io::Result<()> {
    Ok(())
}

/// One request line in, one response line out, then the connection ends.
#[cfg(unix)]
async fn serve(st: State, s: tokio::net::UnixStream) -> io::Result<()> {
    let (r, mut w) = s.into_split();
    let mut line = String::new();
    BufReader::new(r.take(MAX_LINE as u64))
        .read_line(&mut line)
        .await?;
    let (resp, stop) = match serde_json::from_str::<Request>(line.trim()) {
        Ok(req) => handle(&st, req).await,
        Err(e) => (Response::Err(format!("bad request: {e}")), false),
    };
    let mut out = serde_json::to_vec(&resp).map_err(io::Error::other)?;
    out.push(b'\n');
    w.write_all(&out).await?;
    if stop {
        st.stop();
    }
    Ok(())
}

pub fn cleanup(st: &State) {
    if let Some(s) = st.paths.socket() {
        let _ = std::fs::remove_file(s);
    }
}
