//! Module `downloads`: the torrent engine. Off by default; while off no torrent runs and no peer is contacted. Starting it starts `nocturne-torrent` (state under `<data>/torrent`, downloads in `[downloads] dir`) and publishes it in [`DlSlot`] for the library module; a config change applies the transfer limits at once (a new download folder takes effect at the next start).
use crate::api::{ApiResult, ok};
use crate::dl::{DlSlot, DynDownloader};
use crate::fake::FakeDownloader;
use crate::module::{ApiRouter, BoxFut, Module};
use crate::real::Torrent;
use crate::state::Shared;
use axum::Json;
use axum::extract::State;
use nocturne_arr_types::{Limits, TorrentStatus};
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub enum Backend {
    Real,
    /// Tests and `NOCTURNE_SERVER_FAKE=1` debug runs.
    Fake(Arc<FakeDownloader>),
}

pub struct Downloads {
    backend: Backend,
    real: Mutex<Option<Arc<Torrent>>>,
}

impl Downloads {
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            real: Mutex::default(),
        }
    }
}

impl Module for Downloads {
    fn name(&self) -> &'static str {
        "downloads"
    }

    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("", list).put("/limits", limits)
    }

    fn start<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async move {
            let cfg = sh.cfg().downloads;
            let dl: Arc<dyn DynDownloader> = match &self.backend {
                Backend::Fake(f) => f.clone(),
                Backend::Real => {
                    let dir = cfg.dir_or_default().ok_or("no download folder")?;
                    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                    let mut c = nocturne_torrent::Config::new(sh.paths.data.join("torrent"), dir);
                    c.limits = crate::real::torrent_limits(cfg.limits);
                    let t = Arc::new(Torrent(
                        nocturne_torrent::Engine::start(c)
                            .await
                            .map_err(|e| e.to_string())?,
                    ));
                    *self.real.lock().unwrap() = Some(t.clone());
                    t
                }
            };
            *sh.ext::<DlSlot>().0.write().unwrap() = Some(dl);
            Ok(())
        })
    }

    fn stop<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(async move {
            *sh.ext::<DlSlot>().0.write().unwrap() = None;
            let real = self.real.lock().unwrap().take();
            if let Some(t) = real {
                t.0.shutdown().await;
            }
        })
    }

    fn reload<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async move {
            let dl = sh.ext::<DlSlot>().get()?;
            dl.set_limits(sh.cfg().downloads.limits)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

async fn list(State(sh): State<Shared>) -> ApiResult<Vec<TorrentStatus>> {
    let dl = sh.ext::<DlSlot>().get()?;
    Ok(Json(dl.list().await.map_err(|e| e.to_string())?))
}

async fn limits(State(sh): State<Shared>, Json(l): Json<Limits>) -> ApiResult<Value> {
    let mut c = sh.cfg();
    c.downloads.limits = l;
    sh.save_cfg(c)?;
    // Applies the limits through `reload` (only this section changed).
    sh.modules().apply(&sh).await;
    ok()
}
