//! NOCTURNE local library: scans the user's media folders into an on-disk index, with stable ids, Kodi-style
//! sidecar metadata, user overrides and per-item playback state. No UI and no server code.
//!
//! Everything under a media root is opened read-only; the only files this crate writes are its own JSON files in
//! the data directory passed to [`Library::open`] (enforced by `tests/readonly.rs`). The API is blocking: run
//! [`Library::scan`] on a thread.
//!
//! Metadata precedence per field: user override > `.nfo` > sidecar art (art fields only) > file name. `ffprobe`
//! (only when the caller asks via [`Library::probe`]) fills the runtime when nothing else has it.

pub mod naming;
pub mod nfo;
pub mod probe;
mod scan;
mod store;

pub use naming::Parsed;
pub use probe::Probe;
pub use scan::{Progress, ScanReport};

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

/// Missing files are forgotten after this long (seconds), unless purged earlier on request.
pub const PURGE_AFTER_SECS: u64 = 30 * 24 * 3600;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("bad JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("index was written by a newer NOCTURNE (schema {0})")]
    NewerSchema(u32),
    #[error("corrupt data: {0}")]
    Corrupt(&'static str),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ResolveError {
    #[error("unknown item")]
    Unknown,
    #[error("the file is gone from its folder")]
    Missing,
    #[error("the folder is not available (not mounted or unreadable)")]
    Unavailable,
    #[error("the file cannot be read")]
    Unreadable,
}

/// Stable identity of a local file, assigned at first sight and kept across moves and renames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Media {
    Video,
    Audio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Movie,
    Episode,
    Track,
    Unmatched,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderIds {
    pub imdb: Option<String>,
    pub tmdb: Option<String>,
    pub tvdb: Option<String>,
}

impl ProviderIds {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    fn overlay(&mut self, o: &ProviderIds) {
        self.imdb = o.imdb.clone().or(self.imdb.take());
        self.tmdb = o.tmdb.clone().or(self.tmdb.take());
        self.tvdb = o.tvdb.clone().or(self.tvdb.take());
    }
}

/// What a Kodi `.nfo` told us.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub title: Option<String>,
    pub year: Option<u16>,
    pub plot: Option<String>,
    pub genres: Vec<String>,
    pub runtime_min: Option<u32>,
    pub rating: Option<f32>,
    pub ids: ProviderIds,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Art {
    pub poster: Option<PathBuf>,
    pub backdrop: Option<PathBuf>,
}

/// One indexed file (the on-disk record).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: LocalId,
    pub path: PathBuf,
    pub root: PathBuf,
    pub media: Media,
    pub size: u64,
    pub mtime_ns: i64,
    pub dev: u64,
    pub ino: u64,
    /// Size + first/last 64 KiB; 0 if the file could not be read.
    pub hash: u64,
    pub first_seen: u64,
    pub missing_since: Option<u64>,
    pub unavailable: bool,
    pub parsed: Parsed,
    pub nfo: Option<Meta>,
    pub art: Art,
    pub side_sig: u64,
    pub probe: Option<Probe>,
}

impl Item {
    pub(crate) fn new(id: LocalId, now: u64) -> Item {
        Item {
            id,
            path: PathBuf::new(),
            root: PathBuf::new(),
            media: Media::Video,
            size: 0,
            mtime_ns: 0,
            dev: 0,
            ino: 0,
            hash: 0,
            first_seen: now,
            missing_since: None,
            unavailable: false,
            parsed: Parsed::Unmatched {
                title: String::new(),
            },
            nfo: None,
            art: Art::default(),
            side_sig: 0,
            probe: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Root {
    pub path: PathBuf,
    pub available: bool,
}

/// Series-level sidecar data (`tvshow.nfo`, show and season art), keyed by the series folder.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShowInfo {
    pub dir: PathBuf,
    pub meta: Option<Meta>,
    pub art: Art,
    pub season_posters: BTreeMap<u32, PathBuf>,
}

/// `index.json`. Rebuilt by scans; never holds user edits.
#[derive(Debug, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub next_id: u64,
    #[serde(default)]
    pub roots: Vec<Root>,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub shows: Vec<ShowInfo>,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            version: store::INDEX_VERSION,
            next_id: 1,
            roots: vec![],
            items: vec![],
            shows: vec![],
        }
    }
}

/// User edits for one item (`overrides.json`). Any `Some` field beats everything else.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Override {
    pub kind: Option<Kind>,
    pub title: Option<String>,
    pub year: Option<u16>,
    pub plot: Option<String>,
    pub show: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub poster: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Overrides {
    version: u32,
    items: BTreeMap<LocalId, Override>,
}

/// Playback state for one item (`state.json`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemState {
    pub resume_secs: u64,
    pub played: bool,
    pub last_played: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct States {
    version: u32,
    items: BTreeMap<LocalId, ItemState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Available,
    /// The file vanished from a readable folder.
    Missing,
    /// Its folder is not mounted or not readable right now.
    Unavailable,
}

/// An item with all metadata layers applied. Built on demand; cheap to clone.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub id: LocalId,
    pub kind: Kind,
    pub media: Media,
    pub title: String,
    pub year: Option<u16>,
    pub plot: Option<String>,
    pub genres: Vec<String>,
    pub runtime_min: Option<u32>,
    pub rating: Option<f32>,
    pub ids: ProviderIds,
    pub show: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub episode_end: Option<u32>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track: Option<u32>,
    pub art: Art,
    pub status: Status,
    pub added: u64,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryKind {
    Movies,
    Shows,
    Music,
    Mixed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LibraryInfo {
    pub root: PathBuf,
    pub name: String,
    pub kind: LibraryKind,
    pub items: usize,
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    /// Display name (`tvshow.nfo` title if any).
    pub name: String,
    /// The show name its episodes carry (`View::show`); pass this to `seasons` / `episodes`.
    pub show: String,
    pub year: Option<u16>,
    pub plot: Option<String>,
    pub genres: Vec<String>,
    pub rating: Option<f32>,
    pub ids: ProviderIds,
    pub art: Art,
    pub seasons: Vec<u32>,
    pub episodes: usize,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Case- and diacritic-insensitive form for matching.
/// ponytail: covers Latin-1 / Latin Extended-A and combining marks; no full Unicode normalisation (no ICU dependency).
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .map(|c| match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'ď' => 'd',
            'è'..='ë' | 'ē' | 'ę' | 'ě' => 'e',
            'ğ' => 'g',
            'ì'..='ï' | 'ī' => 'i',
            'ł' => 'l',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' | 'ō' | 'ő' => 'o',
            'ř' => 'r',
            'ś' | 'š' | 'ş' => 's',
            'ť' => 't',
            'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            c => c,
        })
        .collect()
}

/// The library: index + overrides + state, persisted under one directory
/// (`$XDG_DATA_HOME/nocturne/local/` in the app).
pub struct Library {
    dir: PathBuf,
    index: Index,
    overrides: Overrides,
    state: States,
}

impl Library {
    /// Load (and migrate) what is in `dir`; an empty library if nothing is there yet.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Library, Error> {
        let dir = dir.into();
        let index = match store::load_value(&dir.join("index.json"))? {
            Some(v) => serde_json::from_value(store::migrate(v)?)?,
            None => Index::default(),
        };
        Ok(Library {
            overrides: store::load(&dir.join("overrides.json"))?.unwrap_or_default(),
            state: store::load(&dir.join("state.json"))?.unwrap_or_default(),
            dir,
            index,
        })
    }

    /// Scan `roots` (the complete current list of folders), update the index and save it. Auto-purges entries
    /// missing for more than [`PURGE_AFTER_SECS`]. Blocking: call from a worker thread.
    pub fn scan(
        &mut self,
        roots: &[PathBuf],
        cancel: &AtomicBool,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<ScanReport, Error> {
        let now = now_secs();
        let rep = scan::scan(&mut self.index, roots, now, cancel, &mut on_progress);
        if !rep.cancelled {
            self.purge(|m| now.saturating_sub(m) > PURGE_AFTER_SECS)?;
        }
        self.save_index()?;
        Ok(rep)
    }

    /// Forget every missing file now, with its overrides and state. Returns how many.
    pub fn purge_missing(&mut self) -> Result<usize, Error> {
        let n = self.purge(|_| true)?;
        self.save_index()?;
        Ok(n)
    }

    fn purge(&mut self, old: impl Fn(u64) -> bool) -> Result<usize, Error> {
        let gone: Vec<LocalId> = self
            .index
            .items
            .iter()
            .filter(|i| i.missing_since.is_some_and(&old))
            .map(|i| i.id)
            .collect();
        if gone.is_empty() {
            return Ok(0);
        }
        self.index.items.retain(|i| !gone.contains(&i.id));
        for id in &gone {
            self.overrides.items.remove(id);
            self.state.items.remove(id);
        }
        self.save_overrides()?;
        self.save_state()?;
        Ok(gone.len())
    }

    fn save_index(&self) -> Result<(), Error> {
        store::save(&self.dir.join("index.json"), &self.index)
    }
    fn save_overrides(&self) -> Result<(), Error> {
        store::save(&self.dir.join("overrides.json"), &self.overrides)
    }
    fn save_state(&self) -> Result<(), Error> {
        store::save(&self.dir.join("state.json"), &self.state)
    }

    // ---- views -------------------------------------------------------------------------------------------

    fn item(&self, id: LocalId) -> Option<&Item> {
        self.index.items.iter().find(|i| i.id == id)
    }

    fn in_roots(&self, it: &Item) -> bool {
        self.index.roots.iter().any(|r| r.path == it.root)
    }

    fn make_view(&self, it: &Item) -> View {
        let mut v = View {
            id: it.id,
            kind: Kind::Unmatched,
            media: it.media,
            title: String::new(),
            year: None,
            plot: None,
            genres: vec![],
            runtime_min: None,
            rating: None,
            ids: ProviderIds::default(),
            show: None,
            season: None,
            episode: None,
            episode_end: None,
            artist: None,
            album: None,
            track: None,
            art: it.art.clone(),
            status: if it.unavailable {
                Status::Unavailable
            } else if it.missing_since.is_some() {
                Status::Missing
            } else {
                Status::Available
            },
            added: it.first_seen,
            path: it.path.clone(),
        };
        match &it.parsed {
            Parsed::Movie { title, year, ids } => {
                (v.kind, v.title, v.year, v.ids) = (Kind::Movie, title.clone(), *year, ids.clone());
            }
            Parsed::Episode {
                show,
                show_year,
                season,
                episode,
                episode_end,
                title,
                ids,
            } => {
                v.kind = Kind::Episode;
                v.title = title
                    .clone()
                    .unwrap_or_else(|| format!("Episode {episode}"));
                (v.show, v.year, v.ids) = (Some(show.clone()), *show_year, ids.clone());
                (v.season, v.episode, v.episode_end) =
                    (Some(*season), Some(*episode), *episode_end);
            }
            Parsed::Track {
                artist,
                album,
                track,
                title,
            } => {
                v.kind = Kind::Track;
                (v.title, v.artist, v.album, v.track) = (
                    title.clone(),
                    Some(artist.clone()),
                    Some(album.clone()),
                    *track,
                );
            }
            Parsed::Unmatched { title } => v.title = title.clone(),
        }
        if let Some(m) = &it.nfo {
            if let Some(t) = &m.title {
                v.title = t.clone();
            }
            v.year = m.year.or(v.year);
            v.plot = m.plot.clone();
            v.genres = m.genres.clone();
            v.runtime_min = m.runtime_min;
            v.rating = m.rating;
            v.ids.overlay(&m.ids);
        }
        if v.runtime_min.is_none() {
            v.runtime_min = it
                .probe
                .as_ref()
                .and_then(|p| p.runtime_secs)
                .map(|s| (s / 60.0).round() as u32);
        }
        if let Some(o) = self.overrides.items.get(&it.id) {
            v.kind = o.kind.unwrap_or(v.kind);
            if let Some(t) = &o.title {
                v.title = t.clone();
            }
            v.year = o.year.or(v.year);
            v.plot = o.plot.clone().or(v.plot);
            v.show = o.show.clone().or(v.show);
            v.season = o.season.or(v.season);
            v.episode = o.episode.or(v.episode);
            v.artist = o.artist.clone().or(v.artist);
            v.album = o.album.clone().or(v.album);
            v.art.poster = o.poster.clone().or(v.art.poster);
        }
        v
    }

    /// Browsable items: in a configured root and not missing (unavailable ones stay, flagged by `status`).
    fn views(&self) -> impl Iterator<Item = View> + '_ {
        self.index
            .items
            .iter()
            .filter(|i| i.missing_since.is_none() && self.in_roots(i))
            .map(|i| self.make_view(i))
    }

    // ---- queries -----------------------------------------------------------------------------------------

    /// One library per configured root, typed by the majority of its items.
    pub fn libraries(&self) -> Vec<LibraryInfo> {
        self.index
            .roots
            .iter()
            .map(|r| {
                let mut n = [0usize; 3];
                let mut items = 0;
                for i in self
                    .index
                    .items
                    .iter()
                    .filter(|i| i.root == r.path && i.missing_since.is_none())
                {
                    items += 1;
                    match self.make_view(i).kind {
                        Kind::Movie => n[0] += 1,
                        Kind::Episode => n[1] += 1,
                        Kind::Track => n[2] += 1,
                        Kind::Unmatched => {}
                    }
                }
                let kind = [LibraryKind::Movies, LibraryKind::Shows, LibraryKind::Music]
                    .into_iter()
                    .zip(n)
                    .find(|(_, c)| *c * 2 > items)
                    .map_or(LibraryKind::Mixed, |(k, _)| k);
                LibraryInfo {
                    name: r.path.file_name().map_or_else(
                        || r.path.display().to_string(),
                        |n| n.to_string_lossy().into(),
                    ),
                    root: r.path.clone(),
                    kind,
                    items,
                    available: r.available,
                }
            })
            .collect()
    }

    /// Any item, including missing ones (so a detail page can say why it cannot play).
    pub fn view(&self, id: LocalId) -> Option<View> {
        self.item(id).map(|i| self.make_view(i))
    }

    /// Items of one kind, sorted by title. `Kind::Unmatched` is the list for manual sorting.
    pub fn items(&self, kind: Kind) -> Vec<View> {
        let mut v: Vec<View> = self.views().filter(|v| v.kind == kind).collect();
        v.sort_by_cached_key(|v| (fold(&v.title), v.id));
        v
    }

    pub fn series(&self) -> Vec<Series> {
        let mut groups: BTreeMap<String, Vec<(View, &Item)>> = BTreeMap::new();
        for it in self
            .index
            .items
            .iter()
            .filter(|i| i.missing_since.is_none() && self.in_roots(i))
        {
            let v = self.make_view(it);
            if let (Kind::Episode, Some(show)) = (v.kind, &v.show) {
                groups.entry(fold(show)).or_default().push((v, it));
            }
        }
        let mut out: Vec<Series> = groups
            .into_values()
            .map(|eps| {
                let info = eps.iter().find_map(|(_, it)| {
                    let d = naming::show_dir(&it.path)?;
                    self.index.shows.iter().find(|s| s.dir == d)
                });
                let meta = info.and_then(|s| s.meta.clone()).unwrap_or_default();
                let first = &eps[0].0;
                let mut ids = first.ids.clone();
                ids.overlay(&meta.ids);
                let mut seasons: Vec<u32> = eps.iter().filter_map(|(v, _)| v.season).collect();
                seasons.sort_unstable();
                seasons.dedup();
                Series {
                    name: meta
                        .title
                        .clone()
                        .or(first.show.clone())
                        .unwrap_or_default(),
                    show: first.show.clone().unwrap_or_default(),
                    year: meta.year.or(first.year),
                    plot: meta.plot,
                    genres: meta.genres,
                    rating: meta.rating,
                    ids,
                    art: info.map(|s| s.art.clone()).unwrap_or_default(),
                    seasons,
                    episodes: eps.len(),
                }
            })
            .collect();
        out.sort_by_cached_key(|s| fold(&s.name));
        out
    }

    fn episode_views<'a>(&'a self, show: &'a str) -> impl Iterator<Item = View> + 'a {
        let key = fold(show);
        self.views().filter(move |v| {
            v.kind == Kind::Episode && v.show.as_deref().is_some_and(|s| fold(s) == key)
        })
    }

    pub fn seasons(&self, show: &str) -> Vec<u32> {
        let mut s: Vec<u32> = self.episode_views(show).filter_map(|v| v.season).collect();
        s.sort_unstable();
        s.dedup();
        s
    }

    pub fn episodes(&self, show: &str, season: u32) -> Vec<View> {
        let mut v: Vec<View> = self
            .episode_views(show)
            .filter(|v| v.season == Some(season))
            .collect();
        v.sort_by_key(|v| (v.episode, v.id));
        v
    }

    /// Poster for one season of a series, if a `seasonNN-poster.jpg` sidecar exists.
    pub fn season_poster(&self, show: &str, season: u32) -> Option<PathBuf> {
        let key = fold(show);
        self.index
            .items
            .iter()
            .filter(|i| self.make_view(i).show.is_some_and(|s| fold(&s) == key))
            .filter_map(|i| naming::show_dir(&i.path))
            .find_map(|d| {
                self.index
                    .shows
                    .iter()
                    .find(|s| s.dir == d)?
                    .season_posters
                    .get(&season)
                    .cloned()
            })
    }

    /// Case- and diacritic-insensitive substring match on title, show, artist and album.
    pub fn search(&self, query: &str) -> Vec<View> {
        let q = fold(query.trim());
        if q.is_empty() {
            return vec![];
        }
        let hit = |s: &Option<String>| s.as_deref().is_some_and(|s| fold(s).contains(&q));
        let mut v: Vec<View> = self
            .views()
            .filter(|v| {
                fold(&v.title).contains(&q) || hit(&v.show) || hit(&v.artist) || hit(&v.album)
            })
            .collect();
        v.sort_by_cached_key(|v| (fold(&v.title), v.id));
        v
    }

    pub fn recently_added(&self, n: usize) -> Vec<View> {
        let mut v: Vec<View> = self.views().collect();
        v.sort_by_key(|v| std::cmp::Reverse((v.added, v.id)));
        v.truncate(n);
        v
    }

    /// Partly watched items, most recent first.
    pub fn continue_watching(&self, n: usize) -> Vec<View> {
        let mut v: Vec<(u64, View)> = self
            .views()
            .filter_map(|v| {
                let s = self.state.items.get(&v.id)?;
                (s.resume_secs > 0 && !s.played).then_some((s.last_played, v))
            })
            .collect();
        v.sort_by_key(|(t, v)| std::cmp::Reverse((*t, v.id)));
        v.into_iter().map(|(_, v)| v).take(n).collect()
    }

    /// Absolute path of the media file, or why it cannot be played right now.
    pub fn resolve_path(&self, id: LocalId) -> Result<PathBuf, ResolveError> {
        let it = self.item(id).ok_or(ResolveError::Unknown)?;
        if it.unavailable {
            return Err(ResolveError::Unavailable);
        }
        if it.missing_since.is_some() {
            return Err(ResolveError::Missing);
        }
        match std::fs::File::open(&it.path) {
            Ok(_) => Ok(it.path.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ResolveError::Missing),
            Err(_) => Err(ResolveError::Unreadable),
        }
    }

    /// Provider ids (name tags and nfo merged) for duplicate matching against server items.
    pub fn provider_ids(&self, id: LocalId) -> Option<ProviderIds> {
        self.view(id).map(|v| v.ids)
    }

    pub fn art(&self, id: LocalId) -> Option<Art> {
        self.view(id).map(|v| v.art)
    }

    // ---- edits and state ---------------------------------------------------------------------------------

    pub fn set_override(&mut self, id: LocalId, o: Override) -> Result<(), Error> {
        self.overrides.version = 1;
        if o == Override::default() {
            self.overrides.items.remove(&id);
        } else {
            self.overrides.items.insert(id, o);
        }
        self.save_overrides()
    }

    pub fn override_of(&self, id: LocalId) -> Option<&Override> {
        self.overrides.items.get(&id)
    }

    pub fn state(&self, id: LocalId) -> ItemState {
        self.state.items.get(&id).copied().unwrap_or_default()
    }

    /// Record a resume point. `played` is sticky only through [`Library::set_played`].
    pub fn set_progress(&mut self, id: LocalId, resume_secs: u64) -> Result<(), Error> {
        let s = self.state.items.entry(id).or_default();
        s.resume_secs = resume_secs;
        s.last_played = now_secs();
        self.state.version = 1;
        self.save_state()
    }

    pub fn set_played(&mut self, id: LocalId, played: bool) -> Result<(), Error> {
        let s = self.state.items.entry(id).or_default();
        s.played = played;
        if played {
            s.resume_secs = 0;
            s.last_played = now_secs();
        }
        self.state.version = 1;
        self.save_state()
    }

    /// Optional `ffprobe` (runtime, streams) for one item; `Ok(false)` if ffprobe is missing or failed.
    /// The caller decides when to call this; nothing else in the crate needs ffprobe.
    pub fn probe(&mut self, id: LocalId) -> Result<bool, Error> {
        let Some(it) = self.item(id) else {
            return Ok(false);
        };
        let Some(p) = probe::probe(&it.path) else {
            return Ok(false);
        };
        if let Some(it) = self.index.items.iter_mut().find(|i| i.id == id) {
            it.probe = Some(p);
        }
        self.save_index()?;
        Ok(true)
    }

    pub fn data_dir(&self) -> &Path {
        &self.dir
    }
}
