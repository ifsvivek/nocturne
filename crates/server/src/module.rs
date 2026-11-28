//! Modules: every feature of the server (library manager, downloads, requests, gateway, tunnel, discovery, storage) is a [`Module`] with its own `server.toml` section, its own routes under `/api/v1/<name>/` and its own lifecycle. A module runs only while its section says `enabled = true`; nothing of it runs otherwise (lazy start, no idle polling).
//!
//! # Adding a module
//!
//! 1. A struct implementing [`Module`] in `src/modules/<name>.rs`, one line in `modules::all()`.
//! 2. Config: a `#[serde(default)]` struct with `pub enabled: bool`, read with `sh.cfg().section::<MyCfg>("<name>")` (written back with `set_section`).
//! 3. Routes: [`Module::routes`] with [`ApiRouter`]; paths are relative to `/api/v1/<name>` (`""` is the prefix itself). They sit behind the shared auth (handlers get `Extension<Caller>`) and answer 503 while the module is not running. Document each in `docs/api/openapi.yaml` (the `openapi` test fails otherwise) and note it in `docs/api/CHANGES.md`.
//! 4. State: fields of the struct (behind a `Mutex`), or `sh.ext::<T>()` for something other modules or routes need, `sh.db` with your own [`crate::db::Rec`] tables (`rec!`), `sh.secrets` for keys.
//! 5. Events: `sh.publish(Event::...)` or `sh.module_event(name, kind, data)`; they reach every `/api/v1/events` client.
//! 6. Background work: spawn tasks in `start` that end on `sh.stopped()` or on your own stop signal from `stop`.
use crate::state::Shared;
use axum::Router;
use axum::handler::Handler;
use axum::routing::{MethodRouter, delete, get, post, put};
use nocturne_core::server::{Event, ModuleStatus};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Module: Send + Sync + 'static {
    /// Section name in `server.toml` and route prefix (`/api/v1/<name>`). Lowercase, no slash.
    fn name(&self) -> &'static str;

    /// Add the module's routes. Default: none.
    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r
    }

    /// Bring the module up (its section is enabled). An error is reported in status and health; the module counts as not running.
    fn start<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>>;

    /// Bring it down (disabled, or the server stops). Must not fail.
    fn stop<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, ()>;

    /// Its section changed while running. Default: stop, then start.
    fn reload<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async move {
            self.stop(sh).await;
            self.start(sh).await
        })
    }

    /// A problem while running (shown in status; `None` = healthy).
    fn health(&self) -> Option<String> {
        None
    }
}

/// Running, last start error, section as applied.
type ModState = (bool, Option<String>, Option<toml::Value>);

/// The modules and what each is doing.
pub struct Registry {
    mods: Vec<Arc<dyn Module>>,
    /// name -> (running, last error, section as applied).
    state: Mutex<HashMap<&'static str, ModState>>,
    /// One apply at a time.
    apply: tokio::sync::Mutex<()>,
}

/// The section of a module as a value, to notice changes (typed sections included).
fn section(cfg: &nocturne_core::server::ServerConfig, name: &str) -> Option<toml::Value> {
    toml::Value::try_from(cfg).ok()?.get(name).cloned()
}

impl Registry {
    pub fn new(mods: Vec<Arc<dyn Module>>) -> Self {
        Self {
            mods,
            state: Mutex::default(),
            apply: tokio::sync::Mutex::new(()),
        }
    }

    pub fn all(&self) -> &[Arc<dyn Module>] {
        &self.mods
    }

    pub fn running(&self, name: &str) -> bool {
        self.state.lock().unwrap().get(name).is_some_and(|s| s.0)
    }

    pub fn status(&self, sh: &Shared) -> Vec<ModuleStatus> {
        let cfg = sh.cfg();
        let st = self.state.lock().unwrap();
        self.mods
            .iter()
            .map(|m| {
                let (running, err) = st
                    .get(m.name())
                    .map_or((false, None), |s| (s.0, s.1.clone()));
                ModuleStatus {
                    name: m.name().into(),
                    enabled: cfg.enabled(m.name()),
                    running,
                    error: if running { m.health() } else { err },
                }
            })
            .collect()
    }

    /// Make every module match the configuration: start the newly enabled, stop the disabled, reload the running ones whose section changed. Publishes a health event per change.
    pub async fn apply(&self, sh: &Shared) {
        let _one = self.apply.lock().await;
        let cfg = sh.cfg();
        for m in &self.mods {
            let name = m.name();
            let want = cfg.enabled(name);
            let sec = section(&cfg, name);
            let (running, applied) = {
                let st = self.state.lock().unwrap();
                st.get(name).map_or((false, None), |s| (s.0, s.2.clone()))
            };
            let result = match (running, want) {
                (false, true) => Some(m.start(sh).await),
                (true, false) => {
                    m.stop(sh).await;
                    Some(Err(String::new()))
                }
                (true, true) if applied != sec => Some(m.reload(sh).await),
                _ => None,
            };
            let Some(r) = result else { continue };
            let (run, err) = match r {
                Ok(()) => (true, None),
                Err(e) if e.is_empty() => (false, None),
                Err(e) => {
                    crate::slog!("module {name}: {e}");
                    (false, Some(e))
                }
            };
            self.state
                .lock()
                .unwrap()
                .insert(name, (run, err.clone(), sec));
            sh.publish(Event::Health(ModuleStatus {
                name: name.into(),
                enabled: want,
                running: run,
                error: err,
            }));
        }
    }

    pub async fn stop_all(&self, sh: &Shared) {
        let _one = self.apply.lock().await;
        for m in self.mods.iter().rev() {
            if self.running(m.name()) {
                m.stop(sh).await;
                self.state.lock().unwrap().remove(m.name());
            }
        }
    }
}

/// Routes plus the list of what was registered (checked against the OpenAPI document by a test).
pub struct ApiRouter {
    base: String,
    pub(crate) router: Router<Shared>,
    pub(crate) routes: Vec<(&'static str, String)>,
}

impl ApiRouter {
    pub(crate) fn new(base: &str) -> Self {
        Self {
            base: base.into(),
            router: Router::new(),
            routes: vec![],
        }
    }

    fn add(mut self, method: &'static str, path: &str, mr: MethodRouter<Shared>) -> Self {
        let full = format!("{}{path}", self.base);
        self.router = self.router.route(&full, mr);
        self.routes.push((method, full));
        self
    }

    pub fn get<H: Handler<T, Shared>, T: 'static>(self, path: &str, h: H) -> Self {
        self.add("get", path, get(h))
    }
    pub fn post<H: Handler<T, Shared>, T: 'static>(self, path: &str, h: H) -> Self {
        self.add("post", path, post(h))
    }
    pub fn put<H: Handler<T, Shared>, T: 'static>(self, path: &str, h: H) -> Self {
        self.add("put", path, put(h))
    }
    pub fn delete<H: Handler<T, Shared>, T: 'static>(self, path: &str, h: H) -> Self {
        self.add("delete", path, delete(h))
    }
}
