//! [`Shared`]: the one state handle every module and route gets (cheap to clone). Paths, the redb store, secrets, the configuration, the event bus, per-module extension slots, the module registry and the stop signal.
use crate::db::{Db, Res};
use crate::module::Registry;
use nocturne_core::server::{Event, ModuleEvent, ServerConfig};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use tokio::sync::{broadcast, watch};

#[derive(Clone, Debug)]
pub struct Paths {
    /// `$XDG_CONFIG_HOME/nocturne/server.toml`.
    pub config: PathBuf,
    /// `$XDG_DATA_HOME/nocturne/server/`: `state.redb`, downloads by default, module files.
    pub data: PathBuf,
    /// `$XDG_RUNTIME_DIR/nocturne/`: `server.sock`, `server.lock`. None: no socket, no instance lock.
    pub runtime: Option<PathBuf>,
}

impl Paths {
    pub fn system() -> Res<Self> {
        use nocturne_session::paths;
        Ok(Self {
            config: ServerConfig::default_path().ok_or("no config directory")?,
            data: paths::data_dir().ok_or("no data directory")?.join("server"),
            runtime: paths::runtime_dir().map(|d| d.join("nocturne")),
        })
    }

    /// Everything under one directory (tests).
    pub fn under(dir: &std::path::Path) -> Self {
        Self {
            config: dir.join("config/server.toml"),
            data: dir.join("data"),
            runtime: Some(dir.join("run")),
        }
    }

    pub fn socket(&self) -> Option<PathBuf> {
        self.runtime.as_ref().map(|d| d.join("server.sock"))
    }

    pub fn lock(&self) -> Option<PathBuf> {
        self.runtime.as_ref().map(|d| d.join("server.lock"))
    }

    pub fn db(&self) -> PathBuf {
        self.data.join("state.redb")
    }
}

/// Secret storage: the system keyring (production) or a map (tests). Names are prefixed `server:`.
pub enum Secrets {
    Keyring,
    Memory(Mutex<HashMap<String, String>>),
}

impl Secrets {
    pub fn memory() -> Self {
        Self::Memory(Mutex::default())
    }

    pub fn get(&self, name: &str) -> Option<String> {
        match self {
            Self::Keyring => nocturne_session::load_secret(&format!("server:{name}"))
                .ok()
                .flatten(),
            Self::Memory(m) => m.lock().unwrap().get(name).cloned(),
        }
    }

    /// `None` deletes.
    pub fn set(&self, name: &str, value: Option<&str>) -> Res<()> {
        let key = format!("server:{name}");
        match (self, value) {
            (Self::Keyring, Some(v)) => {
                nocturne_session::save_secret(&key, v).map_err(|e| e.to_string())
            }
            (Self::Keyring, None) => {
                nocturne_session::delete_secret(&key).map_err(|e| e.to_string())
            }
            (Self::Memory(m), Some(v)) => {
                m.lock().unwrap().insert(name.into(), v.into());
                Ok(())
            }
            (Self::Memory(m), None) => {
                m.lock().unwrap().remove(name);
                Ok(())
            }
        }
    }
}

pub struct Inner {
    pub paths: Paths,
    pub db: Db,
    pub secrets: Secrets,
    /// Unix seconds.
    pub started: u64,
    cfg: RwLock<ServerConfig>,
    events: broadcast::Sender<Event>,
    ext: Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    stop: watch::Sender<bool>,
    modules: OnceLock<Arc<Registry>>,
}

#[derive(Clone)]
pub struct Shared(Arc<Inner>);

impl Deref for Shared {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Shared {
    pub fn new(paths: Paths, cfg: ServerConfig, secrets: Secrets, modules: Registry) -> Res<Self> {
        let db = Db::open(&paths.db())?;
        let sh = Self(Arc::new(Inner {
            paths,
            db,
            secrets,
            started: now(),
            cfg: RwLock::new(cfg),
            events: broadcast::channel(256).0,
            ext: Mutex::default(),
            stop: watch::channel(false).0,
            modules: OnceLock::new(),
        }));
        let _ = sh.modules.set(Arc::new(modules));
        Ok(sh)
    }

    /// A snapshot of the configuration.
    pub fn cfg(&self) -> ServerConfig {
        self.cfg.read().unwrap().clone()
    }

    /// Validate, write `server.toml` atomically, then swap. Callers then run [`Registry::apply`] (the `PUT /config` route does).
    pub fn save_cfg(&self, c: ServerConfig) -> Res<()> {
        c.validate()?;
        c.save_to(&self.paths.config)?;
        *self.cfg.write().unwrap() = c;
        Ok(())
    }

    pub fn modules(&self) -> &Arc<Registry> {
        self.modules.get().expect("set in new")
    }

    /// Send to every `/api/v1/events` subscriber (dropped when nobody listens).
    pub fn publish(&self, e: Event) {
        let _ = self.events.send(e);
    }

    /// A module's own event: `{"type":"module","data":{"module":..,"kind":..,"data":..}}`.
    pub fn module_event(&self, module: &str, kind: &str, data: impl serde::Serialize) {
        self.publish(Event::Module(ModuleEvent {
            module: module.into(),
            kind: kind.into(),
            data: serde_json::to_value(data).unwrap_or_default(),
        }));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// One shared `Arc<T>` per type, made on first use: per-module state reachable from routes and from other modules without editing this file.
    pub fn ext<T: Default + Send + Sync + 'static>(&self) -> Arc<T> {
        let mut m = self.ext.lock().unwrap();
        m.entry(TypeId::of::<T>())
            .or_insert_with(|| Arc::new(T::default()))
            .clone()
            .downcast::<T>()
            .expect("keyed by its own type")
    }

    pub fn stop(&self) {
        self.stop.send_replace(true);
    }

    pub async fn stopped(&self) {
        let mut rx = self.stop.subscribe();
        let _ = rx.wait_for(|s| *s).await;
    }
}
