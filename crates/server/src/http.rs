//! Listeners: TCP on the configured binding (loopback by default, LAN peers only in `lan`, HTTPS with the user's certificate in `remote`), and the unix socket `server.sock` (0600 in a 0700 directory, peer uid checked) for local clients. Plus the single-instance lock and the run loop.
use crate::api::{self, Peer};
use crate::state::Shared;
use axum::Router;
use axum::body::Body;
use hyper_util::rt::{TokioIo, TokioTimer};
use nocturne_core::gateway::{Binding, bind_addrs, peer_allowed};
use nocturne_gateway::limits::{ConnLimits, MAX_CONNECTIONS};
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

const HEADER_TIMEOUT: Duration = Duration::from_secs(20);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Create `dir` (and parents) with mode 0700.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Hold the returned file while running: an exclusive `flock`, so a second server fails here.
#[cfg(unix)]
pub fn acquire_lock(sh_paths: &crate::state::Paths) -> io::Result<Option<std::fs::File>> {
    use std::os::unix::io::AsRawFd;
    let (Some(dir), Some(lock)) = (&sh_paths.runtime, sh_paths.lock()) else {
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
                "nocturne-server is already running",
            )
        } else {
            e
        });
    }
    Ok(Some(f))
}

async fn serve_conn<S>(io: S, peer: Peer, router: Router)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
        let mut req = req.map(Body::new);
        req.extensions_mut().insert(peer);
        let r = router.clone();
        async move { r.oneshot(req).await }
    });
    let _ = hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT)
        .serve_connection(TokioIo::new(io), svc)
        .with_upgrades()
        .await;
}

/// Bind the TCP listeners for the configuration. IPv6 is best effort.
pub async fn bind(binding: Binding, port: u16) -> io::Result<Vec<tokio::net::TcpListener>> {
    let [v4, v6] = bind_addrs(binding);
    let order = if binding == Binding::Local {
        [v4, v6]
    } else {
        [v6, v4]
    };
    let (mut out, mut last, mut port) = (vec![], None, port);
    for ip in order {
        match tokio::net::TcpListener::bind((ip, port)).await {
            Ok(l) => {
                port = l.local_addr()?.port();
                out.push(l);
            }
            Err(e) => last = Some(e),
        }
    }
    match (out.is_empty(), last) {
        (true, Some(e)) => Err(e),
        _ => Ok(out),
    }
}

async fn accept_tcp(
    l: tokio::net::TcpListener,
    binding: Binding,
    router: Router,
    tls: Option<tokio_rustls::TlsAcceptor>,
    conns: Arc<ConnLimits>,
) {
    loop {
        let Ok((stream, peer)) = l.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        if !peer_allowed(binding, peer.ip()) {
            continue;
        }
        // 64 connections per address, 256 in all (the gateway's limits): one peer cannot hold every socket.
        let Some(slot) = conns.try_acquire(peer.ip()) else {
            continue;
        };
        let _ = stream.set_nodelay(true);
        let (router, tls) = (router.clone(), tls.clone());
        tokio::spawn(async move {
            let _slot = slot;
            match tls {
                None => serve_conn(stream, Peer::Tcp(peer), router).await,
                Some(a) => {
                    if let Ok(Ok(s)) =
                        tokio::time::timeout(HANDSHAKE_TIMEOUT, a.accept(stream)).await
                    {
                        serve_conn(s, Peer::Tcp(peer), router).await
                    }
                }
            }
        });
    }
}

#[cfg(unix)]
async fn accept_unix(l: tokio::net::UnixListener, router: Router) {
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    loop {
        let Ok((s, _)) = l.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        if s.peer_cred().map_or(true, |c| c.uid() != me) {
            continue;
        }
        tokio::spawn(serve_conn(s, Peer::Unix, router.clone()));
    }
}

/// Serve until stopped. Returns the TCP addresses actually bound through `bound` (tests bind port 0).
pub async fn run(
    sh: Shared,
    bound: Option<tokio::sync::oneshot::Sender<Vec<SocketAddr>>>,
) -> io::Result<()> {
    let cfg = sh.cfg();
    cfg.validate().map_err(io::Error::other)?;
    let (router, _) = api::build(&sh);
    let tls = match (cfg.http.binding, &cfg.http.cert_path, &cfg.http.key_path) {
        (Binding::Remote, Some(c), Some(k)) => {
            Some(crate::tls::load(c, k).map_err(io::Error::other)?)
        }
        _ => None,
    };
    let listeners = bind(cfg.http.binding, cfg.http.port).await.map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("cannot listen on port {}: {e}", cfg.http.port),
        )
    })?;
    crate::auth::prepare_setup_code(&sh).map_err(io::Error::other)?;
    if crate::auth::setup_needed(&sh) {
        crate::slog!(
            "first-run setup is waiting: run `nocturne-server open` on this machine (the setup code is in {})",
            crate::auth::setup_code_path(&sh.paths).display()
        );
    }
    let addrs: Vec<SocketAddr> = listeners
        .iter()
        .filter_map(|l| l.local_addr().ok())
        .collect();
    crate::slog!(
        "listening on {}",
        addrs
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if let Some(b) = bound {
        let _ = b.send(addrs);
    }
    let conns = Arc::new(ConnLimits::new(MAX_CONNECTIONS));
    let mut tasks: Vec<_> = listeners
        .into_iter()
        .map(|l| {
            tokio::spawn(accept_tcp(
                l,
                cfg.http.binding,
                router.clone(),
                tls.clone(),
                conns.clone(),
            ))
        })
        .collect();
    #[cfg(unix)]
    if let (Some(dir), Some(sock)) = (sh.paths.runtime.clone(), sh.paths.socket()) {
        use std::os::unix::fs::PermissionsExt;
        private_dir(&dir)?;
        let _ = std::fs::remove_file(&sock); // stale from a crash; the instance lock says nobody owns it
        let l = tokio::net::UnixListener::bind(&sock)?;
        std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))?;
        tasks.push(tokio::spawn(accept_unix(l, router.clone())));
    }
    sh.modules().apply(&sh).await;
    sh.stopped().await;
    for t in &tasks {
        t.abort();
    }
    sh.modules().stop_all(&sh).await;
    if let Some(s) = sh.paths.socket() {
        let _ = std::fs::remove_file(s);
    }
    Ok(())
}
