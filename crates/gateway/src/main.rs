//! The `nocturne-gateway` process. Started by the app (Settings > Gateway) or by a systemd user unit; no arguments.
use nocturne_gateway::config::{Config, Paths};
use nocturne_gateway::{control, glog, server, state::State};

/// Core dumps off and no ptrace: the process holds server tokens in memory.
fn harden() {
    #[cfg(unix)]
    // SAFETY: plain syscalls with constant arguments.
    unsafe {
        let zero = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        libc::setrlimit(libc::RLIMIT_CORE, &zero);
        #[cfg(target_os = "linux")]
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
}

fn main() {
    harden();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Err(e) = rt.block_on(real_main()) {
        glog!("{e}");
        std::process::exit(1);
    }
}

async fn real_main() -> std::io::Result<()> {
    let paths = Paths::system();
    let _lock = control::acquire_lock(&paths)?;
    let cfg = Config::load(&paths);
    cfg.gw.validate().map_err(std::io::Error::other)?;
    let st = State::open(paths, cfg)?;
    st.refresh_upstreams().await;
    // Ctrl-C and SIGTERM (the app stopping us) end the server cleanly.
    let s2 = st.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            if let Ok(mut term) = signal(SignalKind::terminate()) {
                tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
            }
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
        s2.stop();
    });
    server::run(st).await
}
