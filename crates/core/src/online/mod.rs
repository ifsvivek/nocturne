//! Online details for the local library (opt-in, off by default): looks titles up at TMDB, TVmaze and
//! MusicBrainz / Cover Art Archive and stores the answers as a provider layer in the library's data directory
//! (`nocturne_local::online`). Also finds subtitles at OpenSubtitles (`subs`). Terms, limits and the privacy
//! statement: docs/METADATA.md.
//!
//! What leaves the machine: the parsed title, year, provider ids, season / episode numbers and the album and
//! artist names, plus the user's IP address. Never a file path, folder name or server address.
//! Everything runs on the tokio bridge in the background; every provider has a request gate and 429 / 503 back-off.

pub mod http;
pub mod matching;
pub mod musicbrainz;
pub mod subs;
pub mod tmdb;
pub mod tvmaze;

use crate::local::LocalBackend;
use http::{Err, Gate, Http};
use matching::{Decision, Hit, decide};
use nocturne_local::{Candidate, Library, OnlineMeta, Target, TargetKind, now_secs};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Per local source, in its profile (`sources.toml`): switches only, never a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Flags {
    /// Master switch for lookups of this source's titles. Off by default.
    pub details: bool,
    pub tmdb: bool,
    pub tvmaze: bool,
    pub musicbrainz: bool,
    /// Offer "Find subtitles online" on this source's items.
    pub subtitles: bool,
}

pub const KEY_TMDB: &str = "tmdb";
pub const KEY_SUBS: &str = "opensubtitles";
pub const KEY_SUBS_LOGIN: &str = "opensubtitles-login";

/// The statement shown next to the master switch.
pub const PRIVACY: &str = "Looking up details sends each title's name, year, ids and season or episode number, and the album and artist name for music, to the providers you switch on, together with your IP address. File names, folders and paths are never sent.";
/// TMDB's required notice.
pub const TMDB_NOTICE: &str =
    "This product uses the TMDB API but is not endorsed or certified by TMDB.";
pub const TVMAZE_CREDIT: &str = "TV data from TVmaze.com, licensed CC BY-SA 4.0.";

/// Where the providers live; the tests point these at a loopback server.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub tmdb: String,
    pub tmdb_img: String,
    pub tvmaze: String,
    pub musicbrainz: String,
    pub caa: String,
    pub subs: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            tmdb: "https://api.themoviedb.org/3".into(),
            tmdb_img: "https://image.tmdb.org/t/p/".into(),
            tvmaze: "https://api.tvmaze.com".into(),
            musicbrainz: "https://musicbrainz.org/ws/2".into(),
            caa: "https://coverartarchive.org".into(),
            subs: "https://api.opensubtitles.com/api/v1".into(),
        }
    }
}

/// What is switched on, with the user's TMDB key (read from the keyring by the caller).
#[derive(Clone, Debug, Default)]
pub struct Providers {
    pub tmdb_key: Option<String>,
    pub tvmaze: bool,
    pub musicbrainz: bool,
}

impl Providers {
    pub fn from_flags(f: &Flags, tmdb_key: Option<String>) -> Self {
        Self {
            tmdb_key: tmdb_key.filter(|k| f.details && f.tmdb && !k.trim().is_empty()),
            tvmaze: f.details && f.tvmaze,
            musicbrainz: f.details && f.musicbrainz,
        }
    }
    pub fn any(&self) -> bool {
        self.tmdb_key.is_some() || self.tvmaze || self.musicbrainz
    }
}

/// A provider answer before its pictures are fetched.
pub struct Fetched {
    pub meta: OnlineMeta,
    /// TMDB: a path under the image base; TVmaze: a full URL.
    pub poster: Option<String>,
    pub backdrop: Option<String>,
}

/// Client plus one gate per provider (minimum spacing between its requests).
pub struct Engine {
    pub http: Http,
    pub ep: Endpoints,
    pub(crate) tmdb: Gate,
    pub(crate) tvmaze: Gate,
    pub(crate) mb: Gate,
    pub(crate) caa: Gate,
    pub(crate) subs: Gate,
}

impl Engine {
    pub fn new(ep: Endpoints, cancel: Arc<AtomicBool>) -> Self {
        Self::paced(ep, cancel, 1.0)
    }

    /// `slow` scales every gate (tests use a small factor so they do not wait on real limits).
    pub fn paced(ep: Endpoints, cancel: Arc<AtomicBool>, slow: f32) -> Self {
        let g = |ms: u64| Gate::new(Duration::from_millis((ms as f32 * slow) as u64));
        Self {
            http: Http::with_cancel(cancel),
            ep,
            // TMDB documents no number; 4 per second is far below what it tolerates.
            tmdb: g(250),
            // TVmaze: at least 20 per 10 s per IP.
            tvmaze: g(600),
            // MusicBrainz: about 1 per second.
            mb: g(1100),
            caa: g(500),
            subs: g(300),
        }
    }

    pub(crate) fn tmdb_client<'a>(&'a self, key: &'a str) -> tmdb::Tmdb<'a> {
        tmdb::Tmdb {
            http: &self.http,
            gate: &self.tmdb,
            ep: &self.ep,
            key,
        }
    }
    pub(crate) fn tvmaze_client(&self) -> tvmaze::Tvmaze<'_> {
        tvmaze::Tvmaze {
            http: &self.http,
            gate: &self.tvmaze,
            ep: &self.ep,
        }
    }
    pub(crate) fn mb_client(&self) -> musicbrainz::MusicBrainz<'_> {
        musicbrainz::MusicBrainz {
            http: &self.http,
            gate: &self.mb,
            caa_gate: &self.caa,
            ep: &self.ep,
        }
    }

    /// Does TMDB accept this key? One request.
    pub async fn validate_tmdb(&self, key: &str) -> Result<(), Err> {
        self.tmdb_client(key).validate().await
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub matched: usize,
    pub asked: usize,
    pub nothing: usize,
    pub errors: usize,
    /// TMDB refused the key during the run.
    pub bad_key: bool,
    pub cancelled: bool,
}

enum Out {
    Found(Box<OnlineMeta>, Vec<(String, Vec<u8>)>),
    Ask(Vec<Candidate>),
    Nothing,
    /// Not decidable now (e.g. an episode whose show is not looked up yet): record nothing.
    Skip,
}

const IMG_MAX: usize = 8 << 20;

/// FNV-1a: a stable file name for a key without putting its text in a path.
fn art_name(key: &str, what: &str) -> String {
    let h = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    format!("{h:016x}-{what}.jpg")
}

async fn read<T: Send + 'static>(
    b: &LocalBackend,
    f: impl FnOnce(&Library) -> T + Send + 'static,
) -> T {
    let b = b.clone();
    tokio::task::spawn_blocking(move || b.with(f))
        .await
        .expect("blocking task")
}

async fn write<T: Send + 'static>(
    b: &LocalBackend,
    f: impl FnOnce(&mut Library) -> T + Send + 'static,
) -> T {
    let b = b.clone();
    tokio::task::spawn_blocking(move || b.with_mut(f))
        .await
        .expect("blocking task")
}

fn wanted(t: &Target, p: &Providers) -> bool {
    match t.kind {
        TargetKind::Movie => p.tmdb_key.is_some(),
        TargetKind::Show => p.tmdb_key.is_some() || p.tvmaze,
        TargetKind::Episode => p.tmdb_key.is_some() || p.tvmaze,
        TargetKind::Album => p.musicbrainz,
    }
}

fn label(t: &Target) -> String {
    match (&t.artist, t.year) {
        (Some(a), _) => format!("{a} - {}", t.title),
        (_, Some(y)) => format!("{} ({y})", t.title),
        _ => t.title.clone(),
    }
}

impl Engine {
    /// Look up everything in `b` that wants it and store the answers. Cancel through the flag given to
    /// [`Engine::new`]. Safe to call after every scan: finished entries are not asked again until they age.
    pub async fn run(
        &self,
        b: &LocalBackend,
        providers: &Providers,
        mut on: impl FnMut(Progress),
    ) -> Report {
        let mut p = providers.clone();
        let mut rep = Report::default();
        let now = now_secs();
        // The six-month rule applies even with TMDB switched off.
        let _ = write(b, move |l| l.online_prune(now)).await;
        if !p.any() {
            return rep;
        }
        let todo: Vec<Target> = read(b, move |l| l.online_targets(now)).await;
        let todo: Vec<Target> = todo.into_iter().filter(|t| wanted(t, &p)).collect();
        let total = todo.len();
        let mut strikes = 0;
        for (done, t) in todo.into_iter().enumerate() {
            if self.http.cancel.load(Ordering::Relaxed) {
                rep.cancelled = true;
                break;
            }
            on(Progress { done, total });
            let out = match self.lookup(b, &p, &t).await {
                Ok(o) => {
                    strikes = 0;
                    o
                }
                Err(Err::Cancelled) => {
                    rep.cancelled = true;
                    break;
                }
                Err(Err::Auth) => {
                    // A refused key: stop using TMDB for the rest of this run and tell the user.
                    p.tmdb_key = None;
                    rep.bad_key = true;
                    if !p.any() {
                        break;
                    }
                    continue;
                }
                Err(_) => {
                    rep.errors += 1;
                    strikes += 1;
                    // Offline or blocked: do not hammer the providers; the next scan tries again.
                    if strikes >= 3 {
                        break;
                    }
                    continue;
                }
            };
            let (key, lab) = (t.key.clone(), label(&t));
            match out {
                Out::Found(m, art) => {
                    rep.matched += 1;
                    let _ =
                        write(b, move |l| l.put_online(&key, &lab, Some(*m), vec![], &art)).await;
                }
                Out::Ask(c) => {
                    rep.asked += 1;
                    let _ = write(b, move |l| l.put_online(&key, &lab, None, c, &[])).await;
                }
                Out::Nothing => {
                    rep.nothing += 1;
                    let _ = write(b, move |l| l.put_online(&key, &lab, None, vec![], &[])).await;
                }
                Out::Skip => {}
            }
        }
        on(Progress { done: total, total });
        rep
    }

    async fn lookup(&self, b: &LocalBackend, p: &Providers, t: &Target) -> Result<Out, Err> {
        match t.kind {
            TargetKind::Movie => match p.tmdb_key.as_deref() {
                Some(k) => self.tmdb_title(k, false, t).await,
                None => Ok(Out::Skip),
            },
            TargetKind::Show => {
                // A pick made from the TVmaze list stays with TVmaze.
                let tvmaze_pick = t
                    .chosen
                    .as_ref()
                    .is_some_and(|c| c.provider == tvmaze::NAME);
                if let (Some(k), false) = (p.tmdb_key.as_deref(), tvmaze_pick) {
                    match self.tmdb_title(k, true, t).await? {
                        Out::Nothing if p.tvmaze => {}
                        o => return Ok(o),
                    }
                }
                if p.tvmaze {
                    self.tvmaze_show(t).await
                } else {
                    Ok(Out::Skip)
                }
            }
            TargetKind::Episode => self.episode(b, p, t).await,
            TargetKind::Album => {
                if p.musicbrainz {
                    self.album(t).await
                } else {
                    Ok(Out::Skip)
                }
            }
        }
    }

    async fn pictures(
        &self,
        gate: &Gate,
        f: &Fetched,
        key: &str,
        base: impl Fn(&str, bool) -> String,
    ) -> Out {
        let mut meta = f.meta.clone();
        let mut art = vec![];
        for (path, what, back) in [(&f.poster, "p", false), (&f.backdrop, "b", true)] {
            let Some(path) = path else { continue };
            let rb = self.http.get(&base(path, back));
            if let Ok(bytes) = self.http.bytes(gate, rb, IMG_MAX).await {
                let name = art_name(key, what);
                if back {
                    meta.backdrop = Some(name.clone())
                } else {
                    meta.poster = Some(name.clone())
                }
                art.push((name, bytes));
            }
        }
        Out::Found(Box::new(meta), art)
    }

    async fn tmdb_title(&self, key: &str, tv: bool, t: &Target) -> Result<Out, Err> {
        let tm = self.tmdb_client(key);
        let chosen = t
            .chosen
            .as_ref()
            .filter(|c| c.provider == tmdb::NAME)
            .map(|c| c.id.clone());
        let id = match chosen.or_else(|| t.ids.tmdb.clone()) {
            Some(id) => id,
            None => {
                let hits = match &t.ids.imdb {
                    Some(i) => tm.find_imdb(tv, i).await?,
                    None => vec![],
                };
                let hits = if hits.len() == 1 {
                    hits
                } else {
                    tm.search(tv, &t.title, t.year).await?
                };
                match self.pick(tmdb::NAME, t, hits) {
                    Ok(h) => h.id,
                    Err(o) => return Ok(o),
                }
            }
        };
        let f = match tm.details(tv, &id).await {
            Ok(f) => f,
            // A stale id from a file name or .nfo: fall back to nothing rather than failing the run.
            Err(Err::NotFound) => return Ok(Out::Nothing),
            Err(e) => return Err(e),
        };
        Ok(self
            .pictures(&self.tmdb, &f, &t.key, |p, back| {
                if back {
                    tm.backdrop_url(p)
                } else {
                    tm.poster_url(p)
                }
            })
            .await)
    }

    fn pick(&self, provider: &str, t: &Target, hits: Vec<Hit>) -> Result<Hit, Out> {
        match decide(&t.title, t.year, hits) {
            Decision::Match(h) => Ok(h),
            Decision::Ask(v) => Err(Out::Ask(v.iter().map(|h| h.candidate(provider)).collect())),
            Decision::Nothing => Err(Out::Nothing),
        }
    }

    async fn tvmaze_show(&self, t: &Target) -> Result<Out, Err> {
        let tv = self.tvmaze_client();
        let chosen = t
            .chosen
            .as_ref()
            .filter(|c| c.provider == tvmaze::NAME)
            .map(|c| c.id.clone());
        let id = match chosen {
            Some(id) => id,
            None => {
                let mut hits = vec![];
                if let Some(i) = &t.ids.imdb {
                    hits = tv.lookup("imdb", i).await?;
                }
                if hits.is_empty()
                    && let Some(i) = &t.ids.tvdb
                {
                    hits = tv.lookup("thetvdb", i).await?;
                }
                if hits.len() != 1 {
                    hits = tv.search(&t.title).await?;
                }
                match self.pick(tvmaze::NAME, t, hits) {
                    Ok(h) => h.id,
                    Err(o) => return Ok(o),
                }
            }
        };
        let f = tv.show(&id).await?;
        Ok(self
            .pictures(&self.tvmaze, &f, &t.key, |p, _| p.to_string())
            .await)
    }

    async fn episode(&self, b: &LocalBackend, p: &Providers, t: &Target) -> Result<Out, Err> {
        let (Some(sk), Some(s), Some(e)) = (t.show_key.clone(), t.season, t.episode) else {
            return Ok(Out::Skip);
        };
        let Some(show) = read(b, move |l| l.online_entry(&sk).and_then(|e| e.meta.clone())).await
        else {
            return Ok(Out::Skip);
        };
        let Some(id) = show.native_id else {
            return Ok(Out::Skip);
        };
        let r = match (show.provider.as_str(), p.tmdb_key.as_deref()) {
            (tmdb::NAME, Some(k)) => self.tmdb_client(k).episode(&id, s, e).await,
            (tvmaze::NAME, _) if p.tvmaze => self.tvmaze_client().episode(&id, s, e).await,
            _ => return Ok(Out::Skip),
        };
        match r {
            Ok(f) => Ok(Out::Found(Box::new(f.meta), vec![])),
            Err(Err::NotFound) => Ok(Out::Nothing),
            Err(e) => Err(e),
        }
    }

    async fn album(&self, t: &Target) -> Result<Out, Err> {
        let mb = self.mb_client();
        let artist = t.artist.clone().unwrap_or_default();
        let id = match t
            .chosen
            .as_ref()
            .filter(|c| c.provider == musicbrainz::NAME)
        {
            Some(c) => c.id.clone(),
            None => match self.pick(musicbrainz::NAME, t, mb.search(&artist, &t.title).await?) {
                Ok(h) => h.id,
                Err(o) => return Ok(o),
            },
        };
        let mut f = mb.group(&id).await?;
        let mut art = vec![];
        if let Some(bytes) = mb.cover(&id).await {
            let name = art_name(&t.key, "p");
            f.meta.poster = Some(name.clone());
            art.push((name, bytes));
        }
        // The album title stays what the user's files say; MusicBrainz only adds year, tags and cover.
        f.meta.title = None;
        Ok(Out::Found(Box::new(f.meta), art))
    }
}

// ---- keys (keyring only) -----------------------------------------------------------------------------------

pub fn tmdb_key() -> Option<String> {
    nocturne_session::load_secret(KEY_TMDB)
        .ok()
        .flatten()
        .filter(|k| !k.trim().is_empty())
}

/// Store (or with a blank value remove) a named API key. Blocking keyring call.
pub fn set_secret(name: &str, value: &str) -> Result<(), String> {
    let v = value.trim();
    let r = if v.is_empty() {
        nocturne_session::delete_secret(name)
    } else {
        nocturne_session::save_secret(name, v)
    };
    r.map_err(|_| "No system keyring is available to keep the key.".to_string())
}

pub fn has_secret(name: &str) -> bool {
    nocturne_session::load_secret(name)
        .ok()
        .flatten()
        .is_some_and(|k| !k.trim().is_empty())
}

#[cfg(test)]
mod live;
#[cfg(test)]
mod tests;
