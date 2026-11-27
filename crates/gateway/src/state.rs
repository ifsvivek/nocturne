//! The shared state every route handler receives (`State<State>`): configuration snapshot, the gateway user, tokens, id map, limits, and the upstream sources with the server tokens read from the keyring.
use crate::auth::{QuickConnect, Tokens, Users};
use crate::config::{Config, Paths};
use crate::idmap::IdMap;
use crate::limits::Limits;
use nocturne_core::gateway::{SourceStatus, Status};
use nocturne_core::profile::{ProfileKind, Sources};
use nocturne_core::source::SourceId;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use tokio::sync::watch;

/// A server token. `Debug` and `Display` never show it; the one way to the text is [`Secret::expose`], used only to build an `Authorization` header for a request to that server.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: String) -> Self {
        Self(s)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpState {
    Ready,
    /// No stored login, or the server answered 401 ([`State::mark_signed_out`]).
    SignedOut,
    /// The keyring could not be read; the other sources keep working.
    KeyringLocked,
}

#[derive(Clone, Debug)]
pub enum UpKind {
    Server {
        url: String,
        user_id: String,
        /// Stable key of the server for the id map: the Jellyfin `ServerId` of the stored login (survives an address change and a reorder). Empty until signed in.
        source_key: String,
        token: Option<Secret>,
    },
    Local {
        roots: Vec<PathBuf>,
    },
}

/// One configured source, in priority order (first is the preferred copy of a duplicate).
#[derive(Clone, Debug)]
pub struct Upstream {
    pub id: SourceId,
    pub label: String,
    pub kind: UpKind,
    pub state: UpState,
}

/// Read `sources.toml` and the keyring. Blocking (Secret Service round trips): call from `spawn_blocking`.
pub fn load_upstreams(paths: &Paths) -> Vec<Upstream> {
    let sources = paths
        .sources
        .as_ref()
        .and_then(|p| Sources::load_from(p).ok().flatten())
        .unwrap_or_default();
    let mut out = vec![];
    for p in sources.profiles.iter().filter(|p| p.enabled) {
        match &p.kind {
            ProfileKind::Local { roots } => out.push(Upstream {
                id: p.id,
                label: p.label.clone(),
                kind: UpKind::Local {
                    roots: roots.clone(),
                },
                state: UpState::Ready,
            }),
            ProfileKind::Jellyfin { url, user_id, .. } => {
                let (state, session) = match session_of(url, user_id) {
                    Ok(Some(s)) => (UpState::Ready, Some(s)),
                    Ok(None) => (UpState::SignedOut, None),
                    Err(()) => (UpState::KeyringLocked, None),
                };
                out.push(Upstream {
                    id: p.id,
                    label: p.label.clone(),
                    kind: UpKind::Server {
                        url: url.clone(),
                        user_id: user_id.clone(),
                        source_key: session
                            .as_ref()
                            .map(|s| s.server_id.clone())
                            .unwrap_or_default(),
                        token: session.map(|s| Secret::new(s.access_token)),
                    },
                    state,
                });
            }
        }
    }
    out
}

/// The stored login of (server, user): `Err` only when the keyring itself is unusable.
fn session_of(url: &str, user_id: &str) -> Result<Option<nocturne_jellyfin_user::Session>, ()> {
    use nocturne_session::Error;
    match nocturne_session::load_account(url, user_id) {
        Ok(Some(s)) => return Ok(Some(s)),
        Err(Error::Unavailable(_)) => return Err(()),
        _ => {}
    }
    match nocturne_session::load(url) {
        Ok(s) => Ok(s.filter(|s| s.user_id == user_id)),
        Err(Error::Unavailable(_)) => Err(()),
        Err(_) => Ok(None),
    }
}

pub struct Inner {
    pub paths: Paths,
    cfg: RwLock<Arc<Config>>,
    pub users: Users,
    pub tokens: Tokens,
    pub qc: QuickConnect,
    pub ids: IdMap,
    pub limits: Limits,
    upstreams: RwLock<Arc<Vec<Upstream>>>,
    pub started: Instant,
    stop: watch::Sender<bool>,
    relisten: watch::Sender<u64>,
    bound: watch::Sender<Vec<SocketAddr>>,
    error: Mutex<Option<String>>,
    ext: Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

/// Cheap to clone. Dereferences to [`Inner`], so `state.users`, `state.tokens`, `state.ids`, `state.limits` work directly.
#[derive(Clone)]
pub struct State(Arc<Inner>);

impl Deref for State {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

/// The settings that decide what the listeners look like; a change of any of them rebinds.
fn listener_part(
    c: &Config,
) -> (
    nocturne_core::gateway::Binding,
    u16,
    Option<PathBuf>,
    Option<PathBuf>,
    bool,
    Option<std::net::IpAddr>,
) {
    let g = &c.gw;
    (
        g.binding,
        g.port,
        g.cert_path.clone(),
        g.key_path.clone(),
        g.announce,
        g.trusted_proxy,
    )
}

impl State {
    /// Open (creating on first start) the gateway's files. Upstreams start empty: call [`State::refresh_upstreams`].
    pub fn open(paths: Paths, cfg: Config) -> std::io::Result<Self> {
        Self::open_with(paths, cfg, Limits::default())
    }

    /// [`State::open`] with explicit limits (tests that fire hundreds of requests from one address).
    pub fn open_with(paths: Paths, cfg: Config, limits: Limits) -> std::io::Result<Self> {
        crate::config::private_dir(&paths.data)?;
        let users = Users::open(Some(paths.data.join("users.toml")))?;
        let (stop, _) = watch::channel(false);
        let (relisten, _) = watch::channel(0);
        let (bound, _) = watch::channel(vec![]);
        Ok(Self(Arc::new(Inner {
            tokens: Tokens::open(Some(paths.data.join("tokens.json"))),
            ids: IdMap::open(paths.data.join("ids.json")),
            paths,
            cfg: RwLock::new(Arc::new(cfg)),
            users,
            qc: QuickConnect::default(),
            limits,
            upstreams: RwLock::new(Arc::new(vec![])),
            started: Instant::now(),
            stop,
            relisten,
            bound,
            error: Mutex::new(None),
            ext: Mutex::new(HashMap::new()),
        })))
    }

    pub fn cfg(&self) -> Arc<Config> {
        self.cfg.read().unwrap().clone()
    }

    /// The sources as of the last refresh, in priority order. Holds server tokens: pass `Secret::expose` only into an `Authorization` header.
    pub fn upstreams(&self) -> Arc<Vec<Upstream>> {
        self.upstreams.read().unwrap().clone()
    }

    /// Replace the sources without the keyring (tests that run the gateway against throwaway servers).
    pub fn set_upstreams(&self, list: Vec<Upstream>) {
        *self.upstreams.write().unwrap() = Arc::new(list);
    }

    pub async fn refresh_upstreams(&self) {
        let paths = self.paths.clone();
        let list = tokio::task::spawn_blocking(move || load_upstreams(&paths))
            .await
            .unwrap_or_default();
        *self.upstreams.write().unwrap() = Arc::new(list);
    }

    /// A server answered 401: that source is signed out until the next refresh (the app is told by status and re-signs in).
    pub fn mark_signed_out(&self, id: SourceId) {
        let mut w = self.upstreams.write().unwrap();
        let mut list = (**w).clone();
        for u in list.iter_mut().filter(|u| u.id == id) {
            u.state = UpState::SignedOut;
            if let UpKind::Server { token, .. } = &mut u.kind {
                *token = None;
            }
        }
        *w = Arc::new(list);
    }

    /// Re-read settings and sources (control `Reload`). Rebinds the listeners when a listener setting changed.
    pub async fn reload(&self) -> Result<(), String> {
        let new = Config::load(&self.paths);
        new.gw.validate()?;
        let changed = listener_part(&self.cfg()) != listener_part(&new);
        *self.cfg.write().unwrap() = Arc::new(new);
        self.refresh_upstreams().await;
        if changed {
            self.relisten.send_modify(|g| *g += 1);
        }
        Ok(())
    }

    pub fn stop(&self) {
        self.stop.send_replace(true);
    }

    pub async fn stopped(&self) {
        let mut rx = self.stop.subscribe();
        let _ = rx.wait_for(|s| *s).await;
    }

    pub fn is_stopped(&self) -> bool {
        *self.stop.borrow()
    }

    pub fn relisten_rx(&self) -> watch::Receiver<u64> {
        self.relisten.subscribe()
    }

    pub fn set_bound(&self, addrs: Vec<SocketAddr>) {
        self.bound.send_replace(addrs);
    }

    pub fn bound(&self) -> Vec<SocketAddr> {
        self.bound.borrow().clone()
    }

    /// The listening addresses once the server is up (tests and the control socket wait on this).
    pub async fn wait_listening(&self) -> Vec<SocketAddr> {
        let mut rx = self.bound.subscribe();
        match rx.wait_for(|b| !b.is_empty()).await {
            Ok(b) => b.clone(),
            Err(_) => vec![],
        }
    }

    pub fn set_error(&self, e: Option<String>) {
        *self.error.lock().unwrap() = e;
    }

    /// Per-module state without editing this file: `state.ext::<MyThing>()` creates `MyThing::default()` once and hands out the same `Arc` ever after.
    pub fn ext<T: Default + Send + Sync + 'static>(&self) -> Arc<T> {
        let mut m = self.ext.lock().unwrap();
        m.entry(TypeId::of::<T>())
            .or_insert_with(|| Arc::new(T::default()))
            .clone()
            .downcast::<T>()
            .expect("type id matches")
    }

    pub fn status(&self) -> Status {
        let cfg = self.cfg();
        let now = Instant::now();
        let bound = self.bound();
        let port = bound.first().map_or(cfg.gw.port, |a| a.port());
        let host = crate::server::lan_host(cfg.gw.binding);
        Status {
            version: nocturne_core::gateway::ADVERTISED_VERSION.into(),
            binding: cfg.gw.binding,
            tls: cfg.gw.tls(),
            listening: bound.iter().map(|a| a.to_string()).collect(),
            address: nocturne_core::gateway::address_url(cfg.gw.binding, &host, port),
            user_name: self.users.name(),
            devices: self.tokens.len(),
            pending_codes: self.qc.pending(now),
            sources: self
                .upstreams()
                .iter()
                .map(|u| SourceStatus {
                    label: u.label.clone(),
                    state: match (&u.kind, u.state) {
                        (UpKind::Local { .. }, _) => "local",
                        (_, UpState::Ready) => "ready",
                        (_, UpState::SignedOut) => "signed_out",
                        (_, UpState::KeyringLocked) => "keyring_locked",
                    }
                    .into(),
                })
                .collect(),
            password_unseen: self.users.unseen(),
            error: self.error.lock().unwrap().clone(),
            uptime_secs: self.started.elapsed().as_secs(),
        }
    }
}
