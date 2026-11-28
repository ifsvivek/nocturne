//! The storage module: storage roots (local folders, SMB/NFS shares, remote agents) with role, space and health, user-space SMB mounting through GVfs, mount instructions for what needs root, and a mount watch that announces a lost share (`mount_lost` / `mount_back` module events) so dependent work pauses. Other modules use [`StorageHandle`] through `sh.ext::<StorageHandle>()`.
use crate::api::{ApiErr, ApiResult, ok, saved};
use crate::auth::Caller;
use crate::module::{ApiRouter, BoxFut, Module};
use crate::rec;
use crate::state::Shared;
use axum::extract::{Path as UrlPath, Query, State};
use axum::{Extension, Json};
use nocturne_core::gateway::Binding;
use nocturne_storage::agent::{AgentClient, valid_name};
use nocturne_storage::mounts::{self, Mount, SmbLogin};
use nocturne_storage::{Entry, Health, Kind, Role, Root, Space, fs};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub use super::storage_agent::agent_main;

rec!(Root, "storage_roots");

const NAME: &str = "storage";

/// `[storage]` in `server.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub enabled: bool,
    /// GVfs's `gio`, for mounting SMB shares without root.
    pub gio: PathBuf,
    /// The mount table read and watched.
    pub mountinfo: PathBuf,
    /// `nocturne-server agent` on this machine.
    pub agent: AgentConfig,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gio: "gio".into(),
            mountinfo: mounts::MOUNTINFO.into(),
            agent: AgentConfig::default(),
        }
    }
}

/// `[storage.agent]`: what this machine exports when run as `nocturne-server agent`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// `local` or `lan` (plain HTTP; the agent has no TLS of its own).
    pub binding: Binding,
    pub port: u16,
    /// SHA-256 (hex) of the agent's API key; `nocturne-server agent --new-key` sets it.
    pub key_sha256: String,
    pub roots: Vec<AgentRootConfig>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            binding: Binding::Lan,
            port: 8101,
            key_sha256: String::new(),
            roots: vec![],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentRootConfig {
    pub name: String,
    pub path: PathBuf,
}

/// Blocking file-system work (a dead network mount can hang it): off the runtime, given up after a few seconds.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::time::timeout(Duration::from_secs(5), tokio::task::spawn_blocking(f))
        .await
        .ok()?
        .ok()
}

const NOT_ANSWERING: &str = "the folder is not answering (a dead network mount?)";

#[derive(Default)]
struct St {
    on: bool,
    roots: Vec<Root>,
    /// Agent roots: their API keys (from the keyring).
    keys: HashMap<u64, String>,
    /// Roots whose share or disk is gone.
    lost: HashSet<u64>,
    mountinfo: PathBuf,
}

/// The storage module's state, for other modules: `sh.ext::<StorageHandle>()`. Empty while the module is off.
#[derive(Default)]
pub struct StorageHandle(Mutex<St>);

impl StorageHandle {
    pub fn roots(&self) -> Vec<Root> {
        self.0.lock().unwrap().roots.clone()
    }

    pub fn root(&self, id: u64) -> Option<Root> {
        self.roots().into_iter().find(|r| r.id == id)
    }

    /// The local, SMB or NFS root holding a path (the deepest one), e.g. a library folder or an import target.
    pub fn root_of(&self, path: &Path) -> Option<Root> {
        self.roots()
            .into_iter()
            .filter(|r| r.kind != Kind::Agent && path.starts_with(&r.path))
            .max_by_key(|r| r.path.components().count())
    }

    /// The module runs and the root's share or disk was there at the last mount change. No I/O: check before starting work on a root, and pause on a `mount_lost` event.
    pub fn available(&self, id: u64) -> bool {
        let st = self.0.lock().unwrap();
        st.on && st.roots.iter().any(|r| r.id == id) && !st.lost.contains(&id)
    }

    /// Reachable, writable, mounted, free space. Asks the agent for agent roots.
    pub async fn health(&self, id: u64) -> Option<Health> {
        let root = self.root(id)?;
        if root.kind == Kind::Agent {
            let client = self.agent(id)?;
            return Some(match client.roots().await {
                Ok(rs) => match rs.into_iter().find(|r| r.name == root.remote) {
                    Some(r) => Health {
                        reachable: true,
                        writable: r.writable,
                        mounted: true,
                        space: r.space,
                        error: None,
                    },
                    None => Health {
                        reachable: true,
                        error: Some(format!("the agent has no root {:?}", root.remote)),
                        ..Health::default()
                    },
                },
                Err(e) => Health {
                    error: Some(e),
                    ..Health::default()
                },
            });
        }
        let info = self.0.lock().unwrap().mountinfo.clone();
        Some(
            blocking(move || fs::health(&root, &mounts::read(&info)))
                .await
                .unwrap_or_else(|| Health {
                    error: Some(NOT_ANSWERING.into()),
                    ..Health::default()
                }),
        )
    }

    pub async fn space(&self, id: u64) -> Option<Space> {
        self.health(id).await?.space
    }

    /// `rel` inside a local, SMB or NFS root as a canonical path; refuses escapes (`..`, links out of the root) and lost mounts. The target need not exist; its folder must.
    pub async fn resolve(&self, id: u64, rel: &str) -> Result<PathBuf, String> {
        let root = self.root(id).ok_or("no such storage root")?;
        if root.kind == Kind::Agent {
            return Err("an agent root has no local path; use StorageHandle::agent".into());
        }
        if !self.available(id) {
            return Err(format!("{} is not mounted", root.label));
        }
        let rel = rel.to_string();
        blocking(move || fs::resolve(&root.path, &rel))
            .await
            .unwrap_or_else(|| Err(NOT_ANSWERING.into()))
    }

    /// A client for an agent root and the root's name on the agent.
    pub fn agent(&self, id: u64) -> Option<AgentClient> {
        let st = self.0.lock().unwrap();
        let r = st
            .roots
            .iter()
            .find(|r| r.id == id && r.kind == Kind::Agent)?;
        Some(AgentClient::new(&r.url, st.keys.get(&id)?))
    }

    /// Read the mount table now and announce changes (the watch does this on every mount change; call it before work if in doubt).
    pub fn recheck(&self, sh: &Shared) {
        let info = self.0.lock().unwrap().mountinfo.clone();
        self.apply_mounts(sh, &mounts::read(&info));
    }

    fn apply_mounts(&self, sh: &Shared, m: &[Mount]) {
        let changed: Vec<(Root, bool)> = {
            let mut st = self.0.lock().unwrap();
            let St { roots, lost, .. } = &mut *st;
            roots
                .iter()
                .filter(|r| r.kind != Kind::Agent)
                .filter_map(|r| {
                    let up = mounts::mounted(r, m);
                    let flip = if up {
                        lost.remove(&r.id)
                    } else {
                        lost.insert(r.id)
                    };
                    flip.then(|| (r.clone(), up))
                })
                .collect()
        };
        for (r, up) in changed {
            let kind = if up { "mount_back" } else { "mount_lost" };
            sh.module_event(
                NAME,
                kind,
                json!({"root": r.id, "label": r.label, "path": r.path}),
            );
        }
    }

    /// Roots from the database, keys from the keyring.
    fn load(&self, sh: &Shared, on: bool) -> Result<(), String> {
        let roots: Vec<Root> = sh.db.all()?;
        let keys = roots
            .iter()
            .filter(|r| r.kind == Kind::Agent)
            .filter_map(|r| Some((r.id, sh.secrets.get(&agent_secret(r.id))?)))
            .collect();
        let cfg: StorageConfig = sh.cfg().section(NAME);
        let mut st = self.0.lock().unwrap();
        st.lost.retain(|id| roots.iter().any(|r| r.id == *id));
        *st = St {
            on,
            roots,
            keys,
            lost: std::mem::take(&mut st.lost),
            mountinfo: cfg.mountinfo,
        };
        Ok(())
    }
}

fn agent_secret(id: u64) -> String {
    format!("storage:agent:{id}")
}

fn smb_secret(id: u64) -> String {
    format!("storage:smb:{id}")
}

#[derive(Default)]
pub struct Storage {
    watch: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// Wait for mount changes: `/proc/self/mountinfo` signals POLLPRI on every mount or unmount (read through the same descriptor to re-arm). No polling. Without it (a plain file in tests) only explicit rechecks happen.
async fn watch(sh: Shared, path: PathBuf) {
    use std::io::{Read, Seek};
    use tokio::io::Interest;
    use tokio::io::unix::AsyncFd;
    let Ok(f) = std::fs::File::open(&path) else {
        return;
    };
    // SAFETY: the AsyncFd owns the File, which keeps its descriptor open and unchanged until both drop.
    let Ok(fd) = (unsafe { AsyncFd::register_with_interest(f, Interest::PRIORITY) }) else {
        return;
    };
    let h = sh.ext::<StorageHandle>();
    loop {
        let Ok(mut guard) = fd.ready(Interest::PRIORITY).await else {
            return;
        };
        guard.clear_ready();
        let mut text = String::new();
        let mut file = fd.get_ref();
        if file.rewind().is_err() || file.read_to_string(&mut text).is_err() {
            return;
        }
        h.apply_mounts(&sh, &mounts::parse(&text));
    }
}

impl Module for Storage {
    fn name(&self) -> &'static str {
        NAME
    }

    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("/roots", list_roots)
            .post("/roots", add_root)
            .get("/roots/{id}", get_root)
            .put("/roots/{id}", change_root)
            .delete("/roots/{id}", delete_root)
            .post("/roots/{id}/mount", mount_root)
            .get("/roots/{id}/browse", browse)
            .get("/mounts", list_mounts)
    }

    fn start<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async move {
            let h = sh.ext::<StorageHandle>();
            h.load(sh, true)?;
            h.recheck(sh);
            let path = h.0.lock().unwrap().mountinfo.clone();
            *self.watch.lock().unwrap() = Some(tokio::spawn(watch(sh.clone(), path)));
            Ok(())
        })
    }

    fn stop<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(async move {
            if let Some(t) = self.watch.lock().unwrap().take() {
                t.abort();
            }
            sh.ext::<StorageHandle>().0.lock().unwrap().on = false;
        })
    }
}

fn admin(c: &Caller) -> Result<(), ApiErr> {
    if c.admin() {
        Ok(())
    } else {
        Err(ApiErr::forbidden("storage is for administrators"))
    }
}

#[derive(Serialize)]
struct RootView {
    #[serde(flatten)]
    root: Root,
    available: bool,
    health: Health,
}

async fn view(h: &StorageHandle, root: Root) -> RootView {
    RootView {
        available: h.available(root.id),
        health: h.health(root.id).await.unwrap_or_default(),
        root,
    }
}

async fn list_roots(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
) -> ApiResult<Vec<RootView>> {
    admin(&c)?;
    let h = sh.ext::<StorageHandle>();
    let tasks: Vec<_> = h
        .roots()
        .into_iter()
        .map(|r| {
            let h = h.clone();
            tokio::spawn(async move { view(&h, r).await })
        })
        .collect();
    let mut out = Vec::with_capacity(tasks.len());
    for t in tasks {
        out.extend(t.await.ok());
    }
    Ok(Json(out))
}

fn find(sh: &Shared, id: u64) -> Result<Root, ApiErr> {
    sh.ext::<StorageHandle>()
        .root(id)
        .ok_or_else(ApiErr::not_found)
}

async fn get_root(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    UrlPath(id): UrlPath<u64>,
) -> ApiResult<RootView> {
    admin(&c)?;
    let r = find(&sh, id)?;
    Ok(Json(view(&sh.ext::<StorageHandle>(), r).await))
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct NewRoot {
    label: String,
    kind: Kind,
    role: Role,
    path: PathBuf,
    url: String,
    remote: String,
    /// Agent roots: the agent's API key.
    key: String,
    /// SMB login (optional; none = guest).
    user: String,
    domain: String,
    password: String,
}

fn read_mounts(sh: &Shared) -> Vec<Mount> {
    let cfg: StorageConfig = sh.cfg().section(NAME);
    mounts::read(&cfg.mountinfo)
}

/// The root to store and the secret that goes with it.
async fn prepare(
    sh: &Shared,
    n: NewRoot,
) -> Result<(Root, Option<String>, fn(u64) -> String), ApiErr> {
    let mut root = Root {
        label: n.label.trim().to_string(),
        kind: n.kind,
        role: n.role,
        url: n.url.trim().to_string(),
        ..Root::default()
    };
    let given = (!n.path.as_os_str().is_empty()).then_some(n.path);
    if given.as_ref().is_some_and(|p| !p.is_absolute()) {
        return Err(ApiErr::bad("the path must be absolute"));
    }
    let mut secret = None;
    let mut name: fn(u64) -> String = smb_secret;
    match n.kind {
        Kind::Local => {
            let p = given.ok_or("a local root needs a path")?;
            let canon = blocking(move || {
                p.canonicalize()
                    .map_err(|e| format!("{}: {e}", p.display()))
            })
            .await
            .ok_or(NOT_ANSWERING)??;
            if !canon.is_dir() {
                return Err(ApiErr::bad("the path is not a folder"));
            }
            root.mount_point = mounts::mount_of(&read_mounts(sh), &canon)
                .map(|m| m.point.clone())
                .unwrap_or_default();
            root.path = canon;
        }
        Kind::Smb | Kind::Nfs => {
            let smb = n.kind == Kind::Smb;
            if smb != root.url.starts_with("smb://") || mounts::split_remote(&root.url).is_none() {
                return Err(ApiErr::bad(if smb {
                    "an SMB address looks like smb://host/share"
                } else {
                    "an NFS address looks like host:/export"
                }));
            }
            let found = given.or_else(|| mounts::find_existing(&read_mounts(sh), &root.url));
            root.path = match (found, smb) {
                (Some(p), _) => p,
                (None, true) => {
                    let rt = nocturne_session::paths::runtime_dir()
                        .ok_or("no XDG_RUNTIME_DIR for GVfs")?;
                    mounts::gvfs_path(&rt, &root.url)
                        .ok_or("an SMB address looks like smb://host/share")?
                }
                (None, false) => {
                    return Err(ApiErr::conflict(mounts::instructions(Kind::Nfs, &root.url)));
                }
            };
            if smb && !n.user.is_empty() {
                secret = Some(
                    json!({"user": n.user, "domain": n.domain, "password": n.password}).to_string(),
                );
            }
        }
        Kind::Agent => {
            root.remote = n.remote.trim().to_string();
            if !(root.url.starts_with("http://") || root.url.starts_with("https://")) {
                return Err(ApiErr::bad("an agent address looks like http://host:8101"));
            }
            if !valid_name(&root.remote) || n.key.is_empty() {
                return Err(ApiErr::bad(
                    "an agent root needs the root's name on the agent and the agent's key",
                ));
            }
            let roots = AgentClient::new(&root.url, &n.key).roots().await?;
            if !roots.iter().any(|r| r.name == root.remote) {
                return Err(ApiErr::bad(format!(
                    "the agent has no root {:?}",
                    root.remote
                )));
            }
            secret = Some(n.key);
            name = agent_secret;
        }
    }
    if root.label.is_empty() {
        root.label = match n.kind {
            Kind::Local => root.path.display().to_string(),
            Kind::Agent => format!("{} on {}", root.remote, root.url),
            _ => root.url.clone(),
        };
    }
    Ok((root, secret, name))
}

async fn add_root(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(n): Json<NewRoot>,
) -> ApiResult<Value> {
    admin(&c)?;
    let (mut root, secret, name) = prepare(&sh, n).await?;
    let id = sh.db.put(&mut root)?;
    if let Some(s) = secret
        && let Err(e) = sh.secrets.set(&name(id), Some(&s))
    {
        let _ = sh.db.delete::<Root>(&[id]);
        return Err(ApiErr::bad(format!(
            "cannot keep the login in the keyring: {e}"
        )));
    }
    reload(&sh)?;
    saved(id)
}

fn reload(sh: &Shared) -> Result<(), String> {
    let h = sh.ext::<StorageHandle>();
    h.load(sh, true)?;
    h.recheck(sh);
    Ok(())
}

#[derive(Deserialize)]
struct Change {
    label: Option<String>,
    role: Option<Role>,
}

async fn change_root(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    UrlPath(id): UrlPath<u64>,
    Json(ch): Json<Change>,
) -> ApiResult<Value> {
    admin(&c)?;
    let mut r = find(&sh, id)?;
    if let Some(l) = ch.label.filter(|l| !l.trim().is_empty()) {
        r.label = l.trim().into();
    }
    r.role = ch.role.unwrap_or(r.role);
    sh.db.put(&mut r)?;
    reload(&sh)?;
    saved(id)
}

/// Forget a root (its files are untouched; a GVfs mount stays until logout).
async fn delete_root(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    UrlPath(id): UrlPath<u64>,
) -> ApiResult<Value> {
    admin(&c)?;
    find(&sh, id)?;
    sh.db.delete::<Root>(&[id])?;
    let _ = sh.secrets.set(&agent_secret(id), None);
    let _ = sh.secrets.set(&smb_secret(id), None);
    reload(&sh)?;
    ok()
}

#[derive(Serialize)]
struct MountResult {
    mounted: bool,
    path: PathBuf,
    error: Option<String>,
    /// What the administrator can do instead (fstab line).
    instructions: Option<String>,
}

/// Mount an SMB root through GVfs (no root). NFS, or SMB without GVfs: instructions.
async fn mount_root(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    UrlPath(id): UrlPath<u64>,
) -> ApiResult<MountResult> {
    admin(&c)?;
    let mut r = find(&sh, id)?;
    let h = sh.ext::<StorageHandle>();
    if !matches!(r.kind, Kind::Smb | Kind::Nfs) {
        return Err(ApiErr::bad("only SMB and NFS roots are mounted"));
    }
    if h.health(id).await.is_some_and(|x| x.reachable && x.mounted) {
        return Ok(Json(MountResult {
            mounted: true,
            path: r.path,
            error: None,
            instructions: None,
        }));
    }
    let instructions = Some(mounts::instructions(r.kind, &r.url));
    if r.kind == Kind::Nfs {
        return Ok(Json(MountResult {
            mounted: false,
            path: r.path,
            error: Some("NFS shares need the administrator to mount them".into()),
            instructions,
        }));
    }
    let cfg: StorageConfig = sh.cfg().section(NAME);
    let login: Option<HashMap<String, String>> = sh
        .secrets
        .get(&smb_secret(id))
        .and_then(|s| serde_json::from_str(&s).ok());
    let field = |k: &str| {
        login
            .as_ref()
            .and_then(|l| l.get(k))
            .map_or("", String::as_str)
    };
    let result = mounts::gio_mount(
        &cfg.gio,
        &r.url,
        login.as_ref().map(|_| SmbLogin {
            user: field("user"),
            domain: field("domain"),
            password: field("password"),
        }),
    )
    .await;
    if let Err(e) = result {
        return Ok(Json(MountResult {
            mounted: false,
            path: r.path,
            error: Some(e),
            instructions,
        }));
    }
    // GVfs may spell the folder differently from what was guessed at creation.
    if let Some(rt) = nocturne_session::paths::runtime_dir()
        && r.path.starts_with(rt.join("gvfs"))
        && let Some(p) = mounts::gvfs_path(&rt, &r.url)
        && p != r.path
    {
        r.path = p;
        sh.db.put(&mut r)?;
        reload(&sh)?;
    }
    h.recheck(&sh);
    Ok(Json(MountResult {
        mounted: true,
        path: r.path,
        error: None,
        instructions: None,
    }))
}

#[derive(Deserialize)]
struct Browse {
    #[serde(default)]
    path: String,
}

/// A folder of a root (for picking library and download folders).
async fn browse(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    UrlPath(id): UrlPath<u64>,
    Query(q): Query<Browse>,
) -> ApiResult<Vec<Entry>> {
    admin(&c)?;
    let r = find(&sh, id)?;
    let h = sh.ext::<StorageHandle>();
    if r.kind == Kind::Agent {
        let client = h
            .agent(id)
            .ok_or("the agent's key is missing; add the root again")?;
        return Ok(Json(client.list(&r.remote, &q.path).await?));
    }
    let dir = h.resolve(id, &q.path).await?;
    let list = blocking(move || fs::list(&dir).map_err(|e| e.to_string()))
        .await
        .ok_or(NOT_ANSWERING)??;
    Ok(Json(list))
}

/// Network file systems mounted on this machine (candidates for roots).
async fn list_mounts(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
) -> ApiResult<Vec<Mount>> {
    admin(&c)?;
    Ok(Json(
        read_mounts(&sh)
            .into_iter()
            .filter(|m| mounts::is_network(&m.fstype))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    /// The watch relies on epoll accepting the mount table for POLLPRI.
    #[tokio::test]
    async fn mount_table_is_watchable() {
        use tokio::io::Interest;
        use tokio::io::unix::AsyncFd;
        let f = std::fs::File::open(nocturne_storage::mounts::MOUNTINFO).unwrap();
        // SAFETY: the AsyncFd owns the File.
        assert!(unsafe { AsyncFd::register_with_interest(f, Interest::PRIORITY) }.is_ok());
    }
}
