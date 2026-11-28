//! The engine: one librqbit session plus this crate's own per-torrent bookkeeping and scheduler.
//!
//! librqbit persists torrents and fast-resume bitfields under `data_dir/session`. This crate keeps what librqbit does not know (labels, seeding goals, user pause, upload totals, completion time, pending magnets) in `data_dir/torrents.json`. Every torrent is handed to librqbit paused; the scheduler (`tick`) alone starts and stops them, so queue limits, seeding goals and the kill switch share one code path.

use std::collections::{BTreeMap, HashSet};
use std::net::{Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions,
    ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig, TorrentStatsState,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast};
use tokio::task::AbortHandle;

use crate::config::{Config, GoalAction, Limits, SeedGoal};
use crate::paths::{MAX_TORRENT_BYTES, check_magnet, content_dir, resolve_save_path};
use crate::types::{
    AddOptions, Event, Facts, FileInfo, Phase, Source, State, TorrentInfo, state_of,
};
use crate::{Error, Result};

const TICK: Duration = Duration::from_secs(1);
/// Upload totals and seeding time are saved at least this often while torrents run.
const SAVE_EVERY: u32 = 30;

/// A running torrent engine. Cheap to clone; all clones drive the same session. Call [`Engine::shutdown`] before exit so resume data is flushed.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    session: Arc<Session>,
    root: PathBuf,
    state_file: PathBuf,
    /// Interface name and the index it had at start. A different index later means the interface was recreated and librqbit's sockets are bound to a dead one, so it counts as down.
    iface: Option<(String, u32)>,
    cfg: Mutex<Config>,
    store: Mutex<Store>,
    events: broadcast::Sender<Event>,
    wake: Arc<Notify>,
    ticker: Mutex<Option<AbortHandle>>,
    ticks: AtomicU32,
    #[cfg(test)]
    fake_iface_gone: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct Store {
    torrents: BTreeMap<String, Meta>,
    iface_down: bool,
    alt_on: Option<bool>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Meta {
    name: Option<String>,
    /// Set while the metadata of a magnet is still being fetched (and re-fetched after a restart).
    magnet: Option<String>,
    save_path: PathBuf,
    category: Option<String>,
    tags: Vec<String>,
    sequential: bool,
    only_files: Option<Vec<usize>>,
    peers: Vec<SocketAddr>,
    seed_goal: Option<SeedGoal>,
    user_paused: bool,
    goal_reached: bool,
    added_at: u64,
    completed_at: Option<u64>,
    /// Uploaded bytes from earlier live sessions (librqbit's counter restarts with each one).
    uploaded: u64,
    seeding_ms: u64,
    seq: u64,
    #[serde(skip)]
    live_up: u64,
    #[serde(skip)]
    queued: bool,
    #[serde(skip)]
    error: Option<String>,
    #[serde(skip)]
    task: Option<AbortHandle>,
}

impl Meta {
    fn uploaded_total(&self) -> u64 {
        self.uploaded + self.live_up
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn eng(e: impl std::fmt::Display) -> Error {
    Error::Engine(format!("{e:#}"))
}

fn lib_limits(l: Limits) -> LimitsConfig {
    LimitsConfig {
        download_bps: l.download_bps.and_then(NonZeroU32::new),
        upload_bps: l.upload_bps.and_then(NonZeroU32::new),
    }
}

fn id20(id: &str) -> Result<Id20> {
    Id20::from_str(id).map_err(|_| Error::NotFound(id.to_owned()))
}

/// Interface index, `None` if it does not exist.
#[cfg(unix)]
fn if_index(name: &str) -> Option<u32> {
    let c = std::ffi::CString::new(name).ok()?;
    // SAFETY: `c` is a valid NUL-terminated string that outlives the call; if_nametoindex only reads it.
    let i = unsafe { libc::if_nametoindex(c.as_ptr()) };
    (i != 0).then_some(i)
}

/// librqbit cannot bind to a device here either; reporting it missing makes `start` fail closed.
#[cfg(not(unix))]
fn if_index(_: &str) -> Option<u32> {
    None
}

/// First port in the range that is free right now. A port taken between this probe and librqbit's bind makes `start` fail; retry.
// ponytail: probes IPv4 only; librqbit listens dual-stack.
fn pick_port(range: &std::ops::RangeInclusive<u16>) -> Result<u16> {
    if *range.start() == 0 {
        return Ok(0);
    }
    range
        .clone()
        .find(|p| std::net::TcpListener::bind(("0.0.0.0", *p)).is_ok())
        .ok_or_else(|| {
            Error::Engine(format!(
                "no free port in {}..={}",
                range.start(),
                range.end()
            ))
        })
}

/// Name and file count of validated metadata; also runs librqbit's path checks (`..`, separators in names).
fn inspect(bytes: &[u8]) -> Result<(String, Option<String>, usize)> {
    let bad = |e: &dyn std::fmt::Display| Error::InvalidTorrent(format!("{e:#}"));
    let t = librqbit::torrent_from_bytes(bytes).map_err(|e| bad(&e))?;
    let hash = t.info_hash.as_string();
    let info = t.info.data.validate().map_err(|e| bad(&e))?;
    let name = info.name().map(|n| n.into_owned());
    Ok((hash, name, info.iter_file_details().count()))
}

impl Engine {
    /// Starts the session, restores torrents from `data_dir` (fast resume: intact data is not re-hashed) and re-fetches metadata for magnets that had none yet.
    pub async fn start(cfg: Config) -> Result<Engine> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        std::fs::create_dir_all(&cfg.download_dir)?;
        let root = cfg.download_dir.canonicalize()?;
        let iface = match &cfg.bind_interface {
            Some(n) => Some((
                n.clone(),
                if_index(n).ok_or_else(|| Error::InterfaceDown(n.clone()))?,
            )),
            None => None,
        };
        let port = pick_port(&cfg.listen_ports)?;
        let opts = SessionOptions {
            dht: cfg.dht.then(|| DhtSessionConfig {
                persistence: Some(DhtPersistenceConfig {
                    config_filename: Some(cfg.data_dir.join("dht.json")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            bind_device_name: cfg.bind_interface.clone(),
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(cfg.data_dir.join("session")),
            }),
            listen: Some(ListenerOptions {
                listen_addr: (Ipv6Addr::UNSPECIFIED, port).into(),
                enable_upnp_port_forwarding: cfg.upnp,
                ..Default::default()
            }),
            disable_local_service_discovery: !cfg.lsd,
            ratelimits: lib_limits(cfg.limits),
            ..Default::default()
        };
        let session = Session::new_with_opts(root.clone(), opts)
            .await
            .map_err(eng)?;

        let state_file = cfg.data_dir.join("torrents.json");
        let mut torrents: BTreeMap<String, Meta> = match std::fs::read(&state_file) {
            Ok(b) => serde_json::from_slice(&b)
                .map_err(|e| eng(format!("{}: {e}", state_file.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        let restored: Vec<(String, PathBuf, Option<String>)> = session.with_torrents(|it| {
            it.map(|(_, t)| {
                (
                    t.info_hash().as_string(),
                    t.output_folder().to_owned(),
                    t.name(),
                )
            })
            .collect()
        });
        let known: HashSet<&String> = restored.iter().map(|(h, ..)| h).collect();
        // Entries librqbit lost (and that are not pending magnets) cannot be resumed: drop them.
        torrents.retain(|h, m| known.contains(h) || m.magnet.is_some());
        let mut seq = torrents.values().map(|m| m.seq).max().unwrap_or(0);
        for (h, dir, name) in &restored {
            torrents.entry(h.clone()).or_insert_with(|| {
                seq += 1;
                Meta {
                    name: name.clone(),
                    save_path: dir.clone(),
                    added_at: now_secs(),
                    seq,
                    ..Default::default()
                }
            });
        }

        let wake = Arc::new(Notify::new());
        let inner = Arc::new(Inner {
            session,
            root,
            state_file,
            iface,
            cfg: Mutex::new(cfg),
            store: Mutex::new(Store {
                torrents,
                ..Default::default()
            }),
            events: broadcast::channel(256).0,
            wake: wake.clone(),
            ticker: Mutex::new(None),
            ticks: AtomicU32::new(1),
            #[cfg(test)]
            fake_iface_gone: Default::default(),
        });
        inner.save();
        let pending: Vec<String> = inner
            .lock()
            .torrents
            .iter()
            .filter(|(_, m)| m.magnet.is_some())
            .map(|(h, _)| h.clone())
            .collect();
        for h in pending {
            inner.spawn_resolve(h);
        }
        let weak = Arc::downgrade(&inner);
        let ticker = tokio::spawn(async move {
            let mut last = Instant::now();
            loop {
                let Some(inner) = weak.upgrade() else { return };
                let busy = inner.tick(last.elapsed()).await;
                last = Instant::now();
                drop(inner);
                if busy {
                    tokio::select! { _ = tokio::time::sleep(TICK) => {}, _ = wake.notified() => {} }
                } else {
                    wake.notified().await;
                }
            }
        });
        *inner.ticker.lock().unwrap() = Some(ticker.abort_handle());
        Ok(Engine { inner })
    }

    /// Adds a torrent and returns its id (info-hash hex). Adding one that is already present returns its id and changes nothing. A magnet is listed as `Metadata` until peers deliver its metadata.
    pub async fn add(&self, source: Source, opts: AddOptions) -> Result<String> {
        let inner = &self.inner;
        let save_path = resolve_save_path(&inner.root, opts.save_path.as_deref())?;
        let (hash, magnet, bytes) = match source {
            Source::Magnet(uri) => (check_magnet(&uri)?, Some(uri), None),
            Source::TorrentFile(bytes) => {
                if bytes.len() > MAX_TORRENT_BYTES {
                    return Err(Error::InvalidTorrent(format!(
                        "larger than {MAX_TORRENT_BYTES} bytes"
                    )));
                }
                (inspect(&bytes)?.0, None, Some(bytes))
            }
        };
        {
            let mut s = inner.lock();
            if s.torrents.contains_key(&hash) {
                return Ok(hash);
            }
            let seq = s.torrents.values().map(|m| m.seq).max().unwrap_or(0) + 1;
            let name = magnet
                .as_deref()
                .and_then(|m| librqbit::Magnet::parse(m).ok())
                .and_then(|m| m.name);
            s.torrents.insert(
                hash.clone(),
                Meta {
                    name,
                    magnet: magnet.clone(),
                    save_path: save_path.clone(),
                    category: opts.category,
                    tags: opts.tags,
                    sequential: opts.sequential,
                    only_files: opts.only_files.clone(),
                    peers: opts.peers.clone(),
                    seed_goal: opts.seed_goal,
                    user_paused: opts.paused,
                    added_at: now_secs(),
                    seq,
                    ..Default::default()
                },
            );
        }
        if let Some(bytes) = bytes
            && let Err(e) = inner
                .add_metadata(&hash, bytes.into(), &save_path, opts.only_files, opts.peers)
                .await
        {
            inner.lock().torrents.remove(&hash);
            return Err(e);
        }
        if magnet.is_some() {
            inner.spawn_resolve(hash.clone());
        }
        inner.save();
        inner.emit(Event::Added { id: hash.clone() });
        inner.wake.notify_one();
        Ok(hash)
    }

    /// Stops a torrent until `resume`. Kept across restarts.
    pub async fn pause(&self, id: &str) -> Result<()> {
        self.inner
            .lock()
            .torrents
            .get_mut(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?
            .user_paused = true;
        if let Some(h) = self.inner.handle(id)
            && matches!(h.stats().state, TorrentStatsState::Live)
        {
            self.inner.session.pause(&h).await.map_err(eng)?;
        }
        self.inner.save();
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Lets a torrent run again (subject to the queue limits). Also clears a reached seeding goal and retries a torrent in error. Refused while the bound interface is down.
    pub async fn resume(&self, id: &str) -> Result<()> {
        let inner = &self.inner;
        let retry_magnet = {
            let mut s = inner.lock();
            if s.iface_down {
                return Err(Error::InterfaceDown(
                    inner
                        .iface
                        .as_ref()
                        .map(|i| i.0.clone())
                        .unwrap_or_default(),
                ));
            }
            let m = s
                .torrents
                .get_mut(id)
                .ok_or_else(|| Error::NotFound(id.to_owned()))?;
            m.user_paused = false;
            m.goal_reached = false;
            m.magnet.is_some() && m.error.take().is_some()
        };
        if retry_magnet {
            inner.spawn_resolve(id.to_owned());
        }
        if let Some(h) = inner.handle(id)
            && matches!(h.stats().state, TorrentStatsState::Error)
        {
            inner.session.unpause(&h).await.map_err(eng)?;
        }
        inner.save();
        inner.wake.notify_one();
        Ok(())
    }

    /// Removes a torrent; with `delete_files` also its downloaded files (never anything outside the torrent's own files and its now-empty folder).
    pub async fn remove(&self, id: &str, delete_files: bool) -> Result<()> {
        let inner = &self.inner;
        let meta = inner
            .lock()
            .torrents
            .remove(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?;
        if let Some(t) = meta.task {
            t.abort();
        }
        if inner.handle(id).is_some() {
            inner
                .session
                .delete(TorrentIdOrHash::Hash(id20(id)?), delete_files)
                .await
                .map_err(eng)?;
        }
        inner.save();
        inner.emit(Event::Removed { id: id.to_owned() });
        inner.wake.notify_one();
        Ok(())
    }

    /// Verifies all data on disk again (full hash check, ignoring fast-resume data).
    pub async fn recheck(&self, id: &str) -> Result<()> {
        let inner = &self.inner;
        let h = inner
            .handle(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?;
        let bytes = h.with_metadata(|m| m.torrent_bytes.clone()).map_err(eng)?;
        let dir = h.output_folder().to_owned();
        let only_files = h.only_files();
        drop(h);
        inner
            .session
            .delete(TorrentIdOrHash::Hash(id20(id)?), false)
            .await
            .map_err(eng)?;
        // Forgetting the torrent dropped its fast-resume bitfield, so adding it back hashes everything.
        inner.add_at(bytes, &dir, only_files, Vec::new()).await?;
        inner.wake.notify_one();
        Ok(())
    }

    /// Chooses which files to download (indices from `TorrentInfo::files`); the others are skipped. librqbit has no finer priorities. Not possible while the torrent is checking or fetching metadata. A torrent that gains files will report `Completed` again.
    pub async fn set_file_selection(&self, id: &str, files: &[usize]) -> Result<()> {
        let inner = &self.inner;
        let h = inner
            .handle(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?;
        let set: HashSet<usize> = files.iter().copied().collect();
        inner
            .session
            .update_only_files(&h, &set)
            .await
            .map_err(eng)?;
        if let Some(m) = inner.lock().torrents.get_mut(id) {
            m.only_files = Some(files.to_vec());
            m.completed_at = None;
        }
        inner.save();
        inner.wake.notify_one();
        Ok(())
    }

    /// Changes the global rate limits (the normal ones; the alternative ones apply while their schedule is active).
    pub fn set_limits(&self, limits: Limits) {
        self.inner.cfg.lock().unwrap().limits = limits;
        self.inner.lock().alt_on = None;
        self.inner.apply_limits();
    }

    pub fn list(&self) -> Vec<TorrentInfo> {
        let s = self.inner.lock();
        s.torrents
            .iter()
            .map(|(h, m)| self.inner.info(h, m))
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<TorrentInfo> {
        let s = self.inner.lock();
        s.torrents.get(id).map(|m| self.inner.info(id, m))
    }

    /// Subscribes to engine events. Subscribe before `add` to be sure not to miss that torrent's events.
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// The port librqbit listens on for peers.
    pub fn listen_port(&self) -> Option<u16> {
        self.inner.session.listen_addr().map(|a| a.port())
    }

    /// Saves state, pauses everything (flushing fast-resume data) and stops the session.
    pub async fn shutdown(&self) {
        let inner = &self.inner;
        if let Some(t) = inner.ticker.lock().unwrap().take() {
            t.abort();
        }
        {
            let mut s = inner.lock();
            for m in s.torrents.values_mut() {
                m.uploaded += std::mem::take(&mut m.live_up);
                if let Some(t) = m.task.take() {
                    t.abort();
                }
            }
        }
        inner.save();
        inner.session.stop().await;
    }

    #[cfg(test)]
    fn fake_iface_gone(&self, gone: bool) {
        self.inner.fake_iface_gone.store(gone, Ordering::Relaxed);
        self.inner.wake.notify_one();
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    fn handle(&self, id: &str) -> Option<Arc<ManagedTorrent>> {
        self.session
            .get(TorrentIdOrHash::Hash(Id20::from_str(id).ok()?))
    }

    fn iface_index(&self, name: &str) -> Option<u32> {
        #[cfg(test)]
        if self.fake_iface_gone.load(Ordering::Relaxed) {
            return None;
        }
        if_index(name)
    }

    /// Writes `torrents.json` atomically. Failures are reported as an event, not fatal: librqbit's own resume data is separate.
    fn save(&self) {
        let json = {
            let s = self.lock();
            serde_json::to_vec_pretty(&s.torrents)
        };
        let tmp = self.state_file.with_extension("json.tmp");
        let res = json
            .map_err(std::io::Error::other)
            .and_then(|j| std::fs::write(&tmp, j))
            .and_then(|()| std::fs::rename(&tmp, &self.state_file));
        if let Err(e) = res {
            self.emit(Event::Error {
                id: String::new(),
                message: format!("saving {}: {e}", self.state_file.display()),
            });
        }
    }

    /// Hands metadata to librqbit, paused (the scheduler starts it), into the torrent's own folder under `save`.
    async fn add_metadata(
        &self,
        hash: &str,
        bytes: Bytes,
        save: &Path,
        only_files: Option<Vec<usize>>,
        peers: Vec<SocketAddr>,
    ) -> Result<()> {
        let (h, name, count) = inspect(&bytes)?;
        if h != hash {
            return Err(Error::InvalidTorrent(
                "metadata does not match the info hash".into(),
            ));
        }
        let dir = content_dir(save, name.as_deref(), count, hash)?;
        if let Some(m) = self.lock().torrents.get_mut(hash) {
            m.name = name;
        }
        self.add_at(bytes, &dir, only_files, peers).await
    }

    async fn add_at(
        &self,
        bytes: Bytes,
        dir: &Path,
        only_files: Option<Vec<usize>>,
        peers: Vec<SocketAddr>,
    ) -> Result<()> {
        let dir = dir
            .to_str()
            .ok_or_else(|| Error::PathEscape(dir.to_owned()))?
            .to_owned();
        let opts = AddTorrentOptions {
            paused: true,
            output_folder: Some(dir),
            only_files,
            // Needed to resume or seed existing data; the files are this torrent's own.
            overwrite: true,
            initial_peers: (!peers.is_empty()).then_some(peers),
            ..Default::default()
        };
        self.session
            .add_torrent(AddTorrent::TorrentFileBytes(bytes), Some(opts))
            .await
            .map_err(eng)?;
        Ok(())
    }

    /// Fetches a magnet's metadata from peers in the background, then adds it like a .torrent file.
    fn spawn_resolve(self: &Arc<Self>, hash: String) {
        let this = self.clone();
        let h = hash.clone();
        let task = tokio::spawn(async move {
            let Some((uri, save, only, peers)) = this.lock().torrents.get(&h).and_then(|m| {
                Some((
                    m.magnet.clone()?,
                    m.save_path.clone(),
                    m.only_files.clone(),
                    m.peers.clone(),
                ))
            }) else {
                return;
            };
            let opts = AddTorrentOptions {
                list_only: true,
                initial_peers: (!peers.is_empty()).then_some(peers),
                ..Default::default()
            };
            let res = match this
                .session
                .add_torrent(AddTorrent::from_url(uri), Some(opts))
                .await
            {
                Ok(AddTorrentResponse::ListOnly(r)) => {
                    let mut peers = r.seen_peers;
                    peers.sort();
                    peers.dedup();
                    this.add_metadata(&h, r.torrent_bytes, &save, only.or(r.only_files), peers)
                        .await
                }
                Ok(_) => Err(Error::Engine(
                    "unexpected response while fetching metadata".into(),
                )),
                Err(e) => Err(eng(e)),
            };
            let ev = {
                let mut s = this.lock();
                let Some(m) = s.torrents.get_mut(&h) else {
                    return;
                };
                m.task = None;
                match res {
                    Ok(()) => {
                        m.magnet = None;
                        Event::MetadataReady { id: h.clone() }
                    }
                    Err(e) => {
                        m.error = Some(e.to_string());
                        Event::Error {
                            id: h.clone(),
                            message: e.to_string(),
                        }
                    }
                }
            };
            this.save();
            this.emit(ev);
            this.wake.notify_one();
        });
        if let Some(m) = self.lock().torrents.get_mut(&hash) {
            m.task = Some(task.abort_handle());
        }
    }

    fn apply_limits(&self) {
        let (normal, alt) = {
            let c = self.cfg.lock().unwrap();
            (c.limits, c.alt_speed)
        };
        let alt_on = alt.is_some_and(|a| a.schedule.is_active_now());
        let mut s = self.lock();
        if s.alt_on == Some(alt_on) {
            return;
        }
        s.alt_on = Some(alt_on);
        let l = lib_limits(if alt_on {
            alt.map_or(normal, |a| a.limits)
        } else {
            normal
        });
        self.session.ratelimits.set_download_bps(l.download_bps);
        self.session.ratelimits.set_upload_bps(l.upload_bps);
    }

    /// One scheduler pass: kill switch, speed schedule, accounting, completion and goal checks, then start/stop torrents to match the queue. Returns whether anything is still meant to run (if not, the caller sleeps until woken).
    async fn tick(self: &Arc<Self>, dt: Duration) -> bool {
        if let Some((name, idx)) = &self.iface {
            let down = self.iface_index(name) != Some(*idx);
            let changed = std::mem::replace(&mut self.lock().iface_down, down) != down;
            if changed {
                self.emit(if down {
                    Event::InterfaceDown { name: name.clone() }
                } else {
                    Event::InterfaceUp { name: name.clone() }
                });
            }
        }
        self.apply_limits();

        let (goal, max_dl, max_up) = {
            let c = self.cfg.lock().unwrap();
            (c.seed_goal, c.max_active_downloads, c.max_active_seeds)
        };
        let handles: Vec<(String, Arc<ManagedTorrent>)> = self.session.with_torrents(|it| {
            it.map(|(_, t)| (t.info_hash().as_string(), t.clone()))
                .collect()
        });
        let mut start = Vec::new();
        let mut stop = Vec::new();
        let mut remove = Vec::new();
        let mut events = Vec::new();
        let mut busy = false;
        let mut dirty = false;
        {
            let mut s = self.lock();
            let iface_down = s.iface_down;
            let mut order: Vec<_> = handles
                .iter()
                .filter_map(|(h, t)| Some((s.torrents.get(h)?.seq, h, t)))
                .collect();
            order.sort_by_key(|(seq, ..)| *seq);
            let (mut n_dl, mut n_up) = (0, 0);
            for (_, hash, t) in order {
                let m = s.torrents.get_mut(hash).expect("filtered above");
                let st = t.stats();
                let live = matches!(st.state, TorrentStatsState::Live);
                if live {
                    if st.uploaded_bytes < m.live_up {
                        m.uploaded += m.live_up;
                    }
                    m.live_up = st.uploaded_bytes;
                } else {
                    m.uploaded += std::mem::take(&mut m.live_up);
                }
                if st.finished && m.completed_at.is_none() {
                    m.completed_at = Some(now_secs());
                    events.push(Event::Completed { id: hash.clone() });
                    dirty = true;
                }
                if st.finished && live {
                    m.seeding_ms += dt.as_millis() as u64;
                    let ratio = ratio(m.uploaded_total(), st.progress_bytes);
                    let g = m.seed_goal.unwrap_or(goal);
                    if !m.goal_reached && g.reached(ratio, Duration::from_millis(m.seeding_ms)) {
                        match g.action {
                            GoalAction::Pause => m.goal_reached = true,
                            GoalAction::Remove => remove.push(hash.clone()),
                        }
                        dirty = true;
                    }
                }
                let errored = matches!(st.state, TorrentStatsState::Error);
                if errored && m.error.is_none() {
                    let message = st.error.clone().unwrap_or_default();
                    events.push(Event::Error {
                        id: hash.clone(),
                        message: message.clone(),
                    });
                    m.error = Some(message);
                } else if !errored {
                    m.error = None;
                }
                let eligible =
                    !m.user_paused && !m.goal_reached && !errored && !remove.contains(hash);
                let slots = if st.finished {
                    (&mut n_up, max_up)
                } else {
                    (&mut n_dl, max_dl)
                };
                let want = eligible && !iface_down && take_slot(slots.0, slots.1);
                m.queued = eligible && !want;
                busy |= eligible;
                match st.state {
                    TorrentStatsState::Paused if want => start.push(t.clone()),
                    TorrentStatsState::Live if !want => stop.push(t.clone()),
                    _ => {}
                }
            }
        }
        for t in stop {
            if let Err(e) = self.session.pause(&t).await {
                events.push(Event::Error {
                    id: t.info_hash().as_string(),
                    message: format!("{e:#}"),
                });
            }
        }
        for t in start {
            if let Err(e) = self.session.unpause(&t).await {
                events.push(Event::Error {
                    id: t.info_hash().as_string(),
                    message: format!("{e:#}"),
                });
            }
        }
        for id in remove {
            self.lock().torrents.remove(&id);
            match id20(&id) {
                Ok(h) => match self.session.delete(TorrentIdOrHash::Hash(h), false).await {
                    Ok(()) => events.push(Event::Removed { id }),
                    Err(e) => events.push(Event::Error {
                        id,
                        message: format!("{e:#}"),
                    }),
                },
                Err(e) => events.push(Event::Error {
                    id,
                    message: e.to_string(),
                }),
            }
        }
        let save_now = self
            .ticks
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(SAVE_EVERY);
        if dirty || !events.is_empty() || save_now {
            self.save();
        }
        for e in events {
            self.emit(e);
        }
        busy
    }

    fn info(&self, id: &str, m: &Meta) -> TorrentInfo {
        let mut info = TorrentInfo {
            id: id.to_owned(),
            name: m.name.clone().unwrap_or_else(|| id.to_owned()),
            state: m.error.clone().map_or(State::Metadata, State::Error),
            progress: 0.0,
            size: 0,
            downloaded: 0,
            uploaded: m.uploaded_total(),
            download_rate: 0,
            upload_rate: 0,
            eta: None,
            ratio: 0.0,
            peers: 0,
            seeders: 0,
            leechers: 0,
            save_path: m.save_path.clone(),
            content_path: m.save_path.clone(),
            category: m.category.clone(),
            tags: m.tags.clone(),
            sequential: m.sequential,
            files: Vec::new(),
            added: UNIX_EPOCH + Duration::from_secs(m.added_at),
            completed: m.completed_at.map(|c| UNIX_EPOCH + Duration::from_secs(c)),
        };
        let Some(t) = self.handle(id) else {
            return info;
        };
        let st = t.stats();
        let phase = match st.state {
            TorrentStatsState::Initializing { .. } => Phase::Checking,
            TorrentStatsState::Live => Phase::Live,
            TorrentStatsState::Paused => Phase::Paused,
            TorrentStatsState::Error => Phase::Paused,
        };
        if let Some(live) = &st.live {
            info.download_rate = live.download_speed.as_bytes();
            info.upload_rate = live.upload_speed.as_bytes();
            let p = &live.snapshot.peer_stats;
            info.peers = p.live;
            info.seeders = p.live_seeders;
            info.leechers = p.live.saturating_sub(p.live_seeders);
        }
        info.state = match &st.error {
            Some(e) => State::Error(e.clone()),
            None => state_of(Facts {
                phase,
                finished: st.finished,
                user_paused: m.user_paused,
                goal_reached: m.goal_reached,
                queued: m.queued,
                download_rate: info.download_rate,
            }),
        };
        info.size = st.total_bytes;
        info.downloaded = st.progress_bytes;
        if st.total_bytes > 0 {
            info.progress = st.progress_bytes as f64 / st.total_bytes as f64;
        }
        info.ratio = ratio(info.uploaded, st.progress_bytes);
        if info.download_rate > 0 && !st.finished {
            info.eta = Some(Duration::from_secs(
                st.total_bytes.saturating_sub(st.progress_bytes) / info.download_rate,
            ));
        }
        let dir = t.output_folder().to_owned();
        let selected = t.only_files();
        let files = t
            .with_metadata(|md| {
                md.file_infos
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| !f.attrs.padding)
                    .map(|(i, f)| FileInfo {
                        index: i,
                        path: f.relative_filename.clone(),
                        size: f.len,
                        downloaded: st.file_progress.get(i).copied().unwrap_or(0),
                        selected: selected.as_ref().is_none_or(|s| s.contains(&i)),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        info.content_path = match files.as_slice() {
            [one] => dir.join(&one.path),
            _ => dir,
        };
        info.files = files;
        info
    }
}

fn ratio(uploaded: u64, have: u64) -> f64 {
    if have == 0 {
        0.0
    } else {
        uploaded as f64 / have as f64
    }
}

fn take_slot(n: &mut usize, max: Option<usize>) -> bool {
    if max.is_some_and(|m| *n >= m) {
        return false;
    }
    *n += 1;
    true
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Kill switch: bound to `lo`, then the interface "disappears" (simulated: removing a real one needs root). Everything must stop and stay stopped until it is back.
    #[tokio::test(flavor = "multi_thread")]
    async fn interface_loss_pauses_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("f.bin"), vec![7u8; 50_000]).unwrap();
        let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
        let opts = librqbit::CreateTorrentOptions {
            name: None,
            trackers: vec![],
            piece_length: Some(16384),
        };
        let bytes = librqbit::create_torrent(&src.join("f.bin"), opts, &spawner)
            .await
            .unwrap()
            .as_bytes()
            .unwrap();

        let mut cfg = Config::new(tmp.path().join("data"), &src);
        (cfg.listen_ports, cfg.dht, cfg.lsd, cfg.bind_interface) =
            (0..=0, false, false, Some("lo".into()));
        let engine = Engine::start(cfg).await.unwrap();
        let mut events = engine.events();
        let id = engine
            .add(Source::TorrentFile(bytes.to_vec()), AddOptions::default())
            .await
            .unwrap();
        let state = || engine.get(&id).unwrap().state;
        let wait = |want: State| async move {
            for _ in 0..300 {
                if state() == want {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("never reached {want:?}, still {:?}", state());
        };
        wait(State::Seeding).await;

        engine.fake_iface_gone(true);
        wait(State::Queued).await;
        assert!(matches!(
            engine.inner.handle(&id).unwrap().stats().state,
            TorrentStatsState::Paused
        ));
        assert!(matches!(
            engine.resume(&id).await,
            Err(Error::InterfaceDown(_))
        ));
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(
            state(),
            State::Queued,
            "stays stopped while the interface is gone"
        );

        engine.fake_iface_gone(false);
        wait(State::Seeding).await;
        let mut seen = Vec::new();
        while let Ok(e) = events.try_recv() {
            seen.push(e);
        }
        let name = "lo".to_owned();
        assert!(
            seen.contains(&Event::InterfaceDown { name: name.clone() })
                && seen.contains(&Event::InterfaceUp { name }),
            "{seen:?}"
        );
        engine.shutdown().await;
    }
}
