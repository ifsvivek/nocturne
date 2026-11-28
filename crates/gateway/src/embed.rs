//! The gateway inside another process (the server's `gateway` module): no control socket, no instance lock, no reading of the app's files. The host gives the settings and the sources and manages it through [`crate::control::handle`], the same requests the socket carries.
use crate::config::{Config, Paths};
use crate::state::{State, Upstream};
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::JoinHandle;

pub struct Embedded {
    pub st: State,
    task: JoinHandle<std::io::Result<()>>,
}

/// Open the gateway's files under `data` (created 0700), take `cfg` and `upstreams`, and serve until [`Embedded::stop`]. Fails when the first bind fails (port in use, missing certificate).
pub async fn start(
    data: PathBuf,
    cfg: Config,
    upstreams: Vec<Upstream>,
) -> Result<Embedded, String> {
    cfg.gw.validate()?;
    let paths = Paths {
        data,
        runtime: None,
        settings: None,
        sources: None,
    };
    let st = State::open(paths, cfg).map_err(|e| e.to_string())?;
    st.set_upstreams(upstreams);
    let mut task = tokio::spawn(crate::server::run(st.clone()));
    tokio::select! {
        r = &mut task => Err(match r {
            Ok(Err(e)) => e.to_string(),
            _ => "the gateway ended at start".into(),
        }),
        _ = tokio::time::timeout(Duration::from_secs(10), st.wait_listening()) => {
            if st.bound().is_empty() {
                st.stop();
                task.abort();
                return Err("the gateway did not start listening".into());
            }
            Ok(Embedded { st, task })
        }
    }
}

impl Embedded {
    /// End the listeners and write the token and id files.
    pub async fn stop(self) {
        self.st.stop();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}
