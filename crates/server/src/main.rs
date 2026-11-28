//! `nocturne-server`: `serve` (what the systemd unit runs; also the default without arguments) and the CLI commands of `cli.rs`.
use nocturne_core::server::ServerConfig;
use nocturne_server::module::Registry;
use nocturne_server::state::{Paths, Secrets, Shared};

/// Core dumps off and no ptrace: the process holds tokens and keys in memory.
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(r) = nocturne_server::cli::run(&args) {
        if let Err(e) = r {
            eprintln!("nocturne-server: {e}");
            std::process::exit(1);
        }
        return;
    }
    harden();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Err(e) = rt.block_on(serve()) {
        eprintln!("nocturne-server: {e}");
        std::process::exit(1);
    }
}

async fn serve() -> Result<(), String> {
    let paths = Paths::system()?;
    let _lock = nocturne_server::http::acquire_lock(&paths).map_err(|e| e.to_string())?;
    let cfg = ServerConfig::load_from(&paths.config)?;
    let sh = Shared::new(
        paths,
        cfg,
        Secrets::Keyring,
        Registry::new(nocturne_server::modules::all()),
    )?;
    let s2 = sh.clone();
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
        }
        s2.stop();
    });
    nocturne_server::http::run(sh, None)
        .await
        .map_err(|e| e.to_string())
}
