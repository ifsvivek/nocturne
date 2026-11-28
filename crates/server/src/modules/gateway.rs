//! `gateway`: the Jellyfin-compatible gateway (crate `nocturne-gateway`) inside this process, on its own listener (default port 8099), over the servers of the `sources` module and the local folders of its section. Managed through `/api/v1/gateway/...` with the same requests the 0.7 control socket carried ([`nocturne_gateway::control::handle`]); there is no gateway socket in the server build.
//!
//! Its files stay where 0.7 kept them (`$XDG_DATA_HOME/nocturne/gateway/`: user, devices, id map, overlay), so phones stay signed in and ids stay stable across the move; [`migrate`] takes the 0.7 settings into the `[gateway]` section once.
use crate::api::{ApiErr, ApiResult, ok};
use crate::auth::Caller;
use crate::module::{ApiRouter, BoxFut, Module};
use crate::modules::sources::{self, SourceKind};
use crate::state::Shared;
use axum::extract::{Path, State};
use axum::{Extension, Json};
use nocturne_core::gateway::{GatewaySettings, Request, Response};
use nocturne_core::source::SourceId;
use nocturne_gateway::config::Config;
use nocturne_gateway::embed::{self, Embedded};
use nocturne_gateway::state::{Secret, UpKind, UpState, Upstream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::sync::{Mutex, watch};

const NAME: &str = "gateway";
/// Server sources get ids above every local folder id of the app (the local index directory is named by the id).
const SERVER_ID_BASE: u32 = 1_000_000;

/// A local folder source of the gateway: the app's local source id (its index lives in `$XDG_DATA_HOME/nocturne/local/<id>`) and its folders.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalSource {
    pub id: u32,
    pub label: String,
    pub roots: Vec<PathBuf>,
}

/// `[gateway]` in `server.toml`: the 0.7 gateway settings plus the write switches (docs/WRITES.md, all off by default) and the local folders.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayCfg {
    #[serde(flatten)]
    pub gw: GatewaySettings,
    pub allow_server_writes: bool,
    pub sync_progress: bool,
    pub sync_watched: bool,
    pub local: Vec<LocalSource>,
}

impl GatewayCfg {
    fn config(&self) -> Config {
        Config {
            gw: self.gw.clone(),
            allow_server_writes: self.allow_server_writes,
            sync_progress: self.sync_progress,
            sync_watched: self.sync_watched,
        }
    }
}

/// `$XDG_DATA_HOME/nocturne/gateway` in production (the 0.7 directory); `<dir>/gateway` under a test's `Paths::under`.
pub fn data_dir(sh: &Shared) -> PathBuf {
    sh.paths
        .data
        .parent()
        .unwrap_or(&sh.paths.data)
        .join("gateway")
}

/// The gateway's sources: the enabled Jellyfin servers of `sources` in id order, then the local folders.
pub fn upstreams(sh: &Shared, cfg: &GatewayCfg) -> Vec<Upstream> {
    let mut out: Vec<Upstream> = sources::list(sh)
        .into_iter()
        .filter(|s| s.rec.enabled && s.rec.kind == SourceKind::Jellyfin)
        .map(|s| {
            let state = if s.token.is_some() {
                UpState::Ready
            } else {
                UpState::SignedOut
            };
            Upstream {
                id: SourceId(SERVER_ID_BASE + s.rec.id as u32),
                label: s.rec.label,
                kind: UpKind::Server {
                    url: s.rec.url,
                    user_id: s.rec.user_id,
                    source_key: s.rec.server_id,
                    token: s.token.map(|t| Secret::new(t.expose().to_string())),
                },
                state,
            }
        })
        .collect();
    out.extend(cfg.local.iter().map(|l| Upstream {
        id: SourceId(l.id),
        label: l.label.clone(),
        kind: UpKind::Local {
            roots: l.roots.clone(),
        },
        state: UpState::Ready,
    }));
    out
}

// ---------------------------------------------------------------- migration from 0.7

const MIGRATED: &str = "import:gateway070";

/// Whether the one-time import of 0.7 has run (the installer removes the old gateway unit after it).
pub fn imported(sh: &Shared) -> bool {
    sh.db.meta(MIGRATED).ok().flatten().is_some()
}

/// Once, at server start: the 0.7 gateway settings (the app's `settings.toml` `[gateway]` table, its write switches, and its local folders from `sources.toml`) become the `[gateway]` section unless the server already has one, and the app's Jellyfin accounts become sources ([`sources::import_app_accounts`]). Reads the app's files and keyring only; changes nothing of them. The gateway's own files need no copy: the module uses the 0.7 directory. Blocking.
pub fn migrate(sh: &Shared) -> Result<(), String> {
    if imported(sh) && crate::cli::gateway_marker(&sh.paths.data).exists() {
        return Ok(());
    }
    let mut cfg = sh.cfg();
    if !cfg.sections.contains_key(NAME) {
        let settings = sh.paths.config.with_file_name("settings.toml");
        if settings.exists() {
            let app = nocturne_core::settings::Settings::load_from(&settings);
            let srcs = nocturne_core::profile::Sources::load_from(
                &sh.paths.config.with_file_name("sources.toml"),
            )
            .ok()
            .flatten()
            .unwrap_or_default();
            let g = GatewayCfg {
                gw: GatewaySettings::load_from(&settings),
                allow_server_writes: app.allow_server_writes,
                sync_progress: app.sync_progress,
                sync_watched: app.sync_watched,
                local: srcs
                    .profiles
                    .iter()
                    .filter(|p| p.enabled)
                    .filter_map(|p| match &p.kind {
                        nocturne_core::profile::ProfileKind::Local { roots } => Some(LocalSource {
                            id: p.id.0,
                            label: p.label.clone(),
                            roots: roots.clone(),
                        }),
                        _ => None,
                    })
                    .collect(),
            };
            cfg.set_section(NAME, &g)?;
            sh.save_cfg(cfg)?;
        }
    }
    sources::import_app_accounts(sh);
    sh.db.set_meta(MIGRATED, 1)?;
    // `nocturne-server install` removes the 0.7 gateway unit only once this marker exists.
    let marker = crate::cli::gateway_marker(&sh.paths.data);
    std::fs::create_dir_all(&sh.paths.data)
        .and_then(|()| std::fs::write(&marker, b""))
        .map_err(|e| format!("{}: {e}", marker.display()))
}

// ---------------------------------------------------------------- module

struct Running {
    gw: Embedded,
    /// Ends the task that follows source changes.
    stop: watch::Sender<bool>,
}

/// The running gateway, in `sh.ext` so the routes reach it.
#[derive(Default)]
struct Run(Mutex<Option<Running>>);

/// The module. Its state is in `sh.ext::<Run>()`; the handle is kept for [`Module::health`], which gets no `Shared`.
#[derive(Default)]
pub struct Gateway {
    sh: std::sync::OnceLock<Shared>,
}

impl Run {
    async fn up(&self, sh: &Shared) -> Result<(), String> {
        let cfg: GatewayCfg = sh.cfg().section(NAME);
        sources::load(sh).await;
        let gw = embed::start(data_dir(sh), cfg.config(), upstreams(sh, &cfg)).await?;
        let (tx, mut rx) = watch::channel(false);
        let (st, sh2) = (gw.st.clone(), sh.clone());
        let mut changes = sources::subscribe(sh);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = sh2.stopped() => return,
                    _ = rx.changed() => return,
                    r = changes.changed() => {
                        if r.is_err() { return }
                        let cfg: GatewayCfg = sh2.cfg().section(NAME);
                        let _ = st.apply(cfg.config(), upstreams(&sh2, &cfg));
                    }
                }
            }
        });
        *self.0.lock().await = Some(Running { gw, stop: tx });
        Ok(())
    }

    async fn down(&self) {
        if let Some(r) = self.0.lock().await.take() {
            let _ = r.stop.send(true);
            r.gw.stop().await;
        }
    }

    async fn ask(&self, req: Request) -> Result<Response, ApiErr> {
        let st = self
            .0
            .lock()
            .await
            .as_ref()
            .map(|r| r.gw.st.clone())
            .ok_or_else(|| ApiErr::bad("the gateway is not running"))?;
        Ok(nocturne_gateway::control::handle(&st, req).await.0)
    }
}

impl Module for Gateway {
    fn name(&self) -> &'static str {
        NAME
    }

    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("/status", status)
            .post("/restart", restart)
            .get("/devices", devices)
            .delete("/devices/{id}", revoke)
            .post("/quickconnect/{code}", approve)
            .post("/password", regenerate)
            .post("/password/reveal", reveal)
            .post("/apikeys", create_key)
    }

    fn start<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        let _ = self.sh.set(sh.clone());
        Box::pin(async move { sh.ext::<Run>().up(sh).await })
    }

    fn stop<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(async move { sh.ext::<Run>().down().await })
    }

    /// A changed section is applied in place (devices stay connected); the listener rebinds itself when its settings changed.
    fn reload<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async move {
            let cfg: GatewayCfg = sh.cfg().section(NAME);
            let run = sh.ext::<Run>();
            let st = run.0.lock().await.as_ref().map(|r| r.gw.st.clone());
            match st {
                Some(st) => st.apply(cfg.config(), upstreams(sh, &cfg)),
                None => run.up(sh).await,
            }
        })
    }

    fn health(&self) -> Option<String> {
        let run = self.sh.get()?.ext::<Run>();
        let g = run.0.try_lock().ok()?;
        g.as_ref()?.gw.st.status().error
    }
}

// ---------------------------------------------------------------- routes

fn module(sh: &Shared) -> std::sync::Arc<Run> {
    sh.ext::<Run>()
}

fn admin(c: &Caller) -> Result<(), ApiErr> {
    if c.admin() {
        Ok(())
    } else {
        Err(ApiErr::forbidden(
            "only an administrator manages the gateway",
        ))
    }
}

fn answer(r: Response) -> ApiResult<Value> {
    match r {
        Response::Status(s) => Ok(Json(json!(s))),
        Response::Tokens(t) => Ok(Json(json!(t))),
        Response::Password(p) => Ok(Json(json!({"password": p}))),
        Response::Key(k) => Ok(Json(json!({"key": k}))),
        Response::Ok => ok(),
        Response::Err(e) => Err(ApiErr::bad(e)),
    }
}

async fn status(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<Value> {
    admin(&c)?;
    answer(module(&sh).ask(Request::Status).await?)
}

async fn restart(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<Value> {
    admin(&c)?;
    let m = module(&sh);
    m.down().await;
    m.up(&sh).await?;
    ok()
}

async fn devices(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<Value> {
    admin(&c)?;
    answer(module(&sh).ask(Request::Tokens).await?)
}

async fn revoke(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    admin(&c)?;
    answer(module(&sh).ask(Request::Revoke(id)).await?)
}

async fn approve(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Path(code): Path<String>,
) -> ApiResult<Value> {
    admin(&c)?;
    answer(module(&sh).ask(Request::ApproveQuickConnect(code)).await?)
}

async fn regenerate(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<Value> {
    admin(&c)?;
    answer(module(&sh).ask(Request::RegeneratePassword).await?)
}

async fn reveal(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<Value> {
    admin(&c)?;
    answer(module(&sh).ask(Request::RevealPassword).await?)
}

#[derive(Deserialize)]
struct KeyBody {
    name: String,
}

async fn create_key(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(b): Json<KeyBody>,
) -> ApiResult<Value> {
    admin(&c)?;
    answer(module(&sh).ask(Request::CreateApiKey(b.name)).await?)
}
