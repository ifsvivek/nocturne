//! The local library as a source: maps `nocturne_local` records to `Media` and answers the same
//! questions a server does, in memory. Queries block on the index lock, so the async wrappers run them
//! on the blocking pool.
//!
//! Ids inside a local source: an item is its `LocalId` in decimal; a series is `show:<name>`; a season
//! `show:<name>#<n>`; a library (one per root folder) `root:<path>`.
use crate::detail::Full;
use crate::source::{Art, Failed, Kind, Media, SourceId};
use nocturne_jellyfin::BrowseQuery;
use nocturne_local as nl;
use nocturne_local::{Library, LocalId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Largest art file read into memory.
const ART_MAX: u64 = 16 << 20;

#[derive(Clone)]
pub struct LocalBackend {
    lib: Arc<RwLock<Library>>,
    origin: SourceId,
}

impl LocalBackend {
    pub fn new(origin: SourceId, lib: Library) -> Self {
        Self {
            lib: Arc::new(RwLock::new(lib)),
            origin,
        }
    }

    pub fn open(origin: SourceId, data_dir: &Path) -> Result<Self, nl::Error> {
        Library::open(data_dir).map(|l| Self::new(origin, l))
    }

    pub fn origin(&self) -> SourceId {
        self.origin
    }

    /// Blocking read access (tests, and code already on a worker thread).
    pub fn with<T>(&self, f: impl FnOnce(&Library) -> T) -> T {
        f(&self.lib.read().unwrap_or_else(|e| e.into_inner()))
    }

    /// Blocking write access (scans, state, overrides).
    /// Subtitles downloaded for a local item (id in decimal), for the player to add as external tracks.
    pub fn subtitles_of(&self, id: &str) -> Vec<PathBuf> {
        id.parse()
            .map_or(vec![], |n| self.with(|l| l.subtitles(LocalId(n))))
    }

    /// Keep a downloaded subtitle in the library's data directory.
    pub fn save_subtitle(&self, id: &str, name: &str, bytes: &[u8]) -> Result<PathBuf, nl::Error> {
        let n = id
            .parse()
            .map_err(|_| nl::Error::Corrupt("not a local item id"))?;
        self.with(|l| l.save_subtitle(LocalId(n), name, bytes))
    }

    pub fn with_mut<T>(&self, f: impl FnOnce(&mut Library) -> T) -> T {
        f(&mut self.lib.write().unwrap_or_else(|e| e.into_inner()))
    }

    async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Library, SourceId) -> T + Send + 'static,
    ) -> Result<T, Failed> {
        let (me, o) = (self.clone(), self.origin);
        tokio::task::spawn_blocking(move || me.with(|l| f(l, o)))
            .await
            .map_err(|_| Failed)
    }

    pub async fn libraries(&self) -> Result<Vec<Media>, Failed> {
        self.read(libraries).await
    }

    pub async fn latest(&self, parent: &Media) -> Result<Vec<Media>, Failed> {
        let root = root_of(&parent.id);
        self.read(move |l, o| latest(l, o, root.as_deref(), 16))
            .await
    }

    pub async fn resume(&self) -> Result<Vec<Media>, Failed> {
        self.read(|l, o| {
            l.continue_watching(12)
                .iter()
                .map(|v| view_media(l, o, v))
                .collect()
        })
        .await
    }

    pub async fn next_up(&self) -> Result<Vec<Media>, Failed> {
        self.read(|l, o| next_up(l, o, None, 12)).await
    }

    pub async fn series_next_up(&self, series: &str) -> Result<Vec<Media>, Failed> {
        let show = show_of(series).unwrap_or_default().to_string();
        self.read(move |l, o| next_up(l, o, Some(&show), 1)).await
    }

    pub async fn browse(&self, q: &BrowseQuery) -> Result<(Vec<Media>, usize), Failed> {
        let q = q.clone();
        self.read(move |l, o| browse(l, o, &q)).await
    }

    pub async fn full(&self, id: &str) -> Result<(Media, Full), Failed> {
        let id = id.to_string();
        let m = self.read(move |l, o| one(l, o, &id)).await?.ok_or(Failed)?;
        Ok((m, Full::default()))
    }

    pub async fn seasons(&self, series: &str) -> Result<Vec<Media>, Failed> {
        let show = show_of(series).ok_or(Failed)?.to_string();
        self.read(move |l, o| seasons(l, o, &show)).await
    }

    pub async fn episodes(&self, series: &str, season: &str) -> Result<Vec<Media>, Failed> {
        let show = show_of(series).ok_or(Failed)?.to_string();
        let n = season_of(season).ok_or(Failed)?;
        self.read(move |l, o| {
            l.episodes(&show, n)
                .iter()
                .map(|v| view_media(l, o, v))
                .collect()
        })
        .await
    }

    pub async fn search(&self, term: &str) -> Result<Vec<Media>, Failed> {
        let term = term.to_string();
        self.read(move |l, o| search(l, o, &term)).await
    }

    /// The art file's bytes; `None` when the item has none, or it cannot be read.
    pub async fn image(&self, m: &Media, art: Art) -> Option<Vec<u8>> {
        let (id, kind) = (m.id.clone(), m.kind);
        let path = self
            .read(move |l, _| art_path(l, &id, kind, art))
            .await
            .ok()
            .flatten()?;
        tokio::task::spawn_blocking(move || read_capped(&path))
            .await
            .ok()
            .flatten()
    }

    /// Identity of the art file (path, size, modification time): the key of a shrunken copy in the
    /// disk cache, so it follows the file and never an item id that a re-scan may reuse.
    pub async fn image_sig(&self, m: &Media, art: Art) -> Option<String> {
        let (id, kind) = (m.id.clone(), m.kind);
        let path = self
            .read(move |l, _| art_path(l, &id, kind, art))
            .await
            .ok()
            .flatten()?;
        tokio::task::spawn_blocking(move || {
            let meta = std::fs::metadata(&path).ok()?;
            let t = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?;
            Some(format!(
                "{}|{}|{}",
                path.display(),
                meta.len(),
                t.as_nanos()
            ))
        })
        .await
        .ok()
        .flatten()
    }

    pub async fn provider_ids(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, nl::ProviderIds>, Failed> {
        let ids = ids.to_vec();
        self.read(move |l, _| {
            ids.into_iter()
                .filter_map(|id| {
                    let p = if let Some(show) = show_of(&id) {
                        l.series()
                            .into_iter()
                            .find(|s| nl::fold(&s.show) == nl::fold(show))?
                            .ids
                    } else {
                        l.provider_ids(parse_id(&id)?)?
                    };
                    Some((id, p))
                })
                .collect()
        })
        .await
    }

    /// Where the player should open the file, or why it cannot.
    pub async fn resolve(&self, m: &Media) -> Result<PathBuf, nl::ResolveError> {
        let id = parse_id(&m.id).ok_or(nl::ResolveError::Unknown)?;
        let me = self.clone();
        tokio::task::spawn_blocking(move || me.with(|l| l.resolve_path(id)))
            .await
            .unwrap_or(Err(nl::ResolveError::Unreadable))
    }

    /// Local state only; no server is involved.
    pub async fn set_played(&self, m: &Media, played: bool) -> Result<(), Failed> {
        let id = parse_id(&m.id).ok_or(Failed)?;
        let me = self.clone();
        tokio::task::spawn_blocking(move || me.with_mut(|l| l.set_played(id, played)))
            .await
            .map_err(|_| Failed)?
            .map_err(|_| Failed)
    }

    pub async fn set_progress(&self, m: &Media, secs: u64) -> Result<(), Failed> {
        let id = parse_id(&m.id).ok_or(Failed)?;
        let me = self.clone();
        tokio::task::spawn_blocking(move || me.with_mut(|l| l.set_progress(id, secs)))
            .await
            .map_err(|_| Failed)?
            .map_err(|_| Failed)
    }
}

// ---- ids ---------------------------------------------------------------------------------------------

fn parse_id(id: &str) -> Option<LocalId> {
    id.parse().ok().map(LocalId)
}

pub fn series_id(show: &str) -> String {
    format!("show:{show}")
}

fn season_id(show: &str, n: u32) -> String {
    format!("show:{show}#{n}")
}

/// The show a series id (or season id) refers to.
fn show_of(id: &str) -> Option<&str> {
    let s = id.strip_prefix("show:")?;
    Some(s.rsplit_once('#').map_or(
        s,
        |(show, n)| {
            if n.parse::<u32>().is_ok() { show } else { s }
        },
    ))
}

fn season_of(id: &str) -> Option<u32> {
    id.strip_prefix("show:")?.rsplit_once('#')?.1.parse().ok()
}

fn root_of(id: &str) -> Option<PathBuf> {
    id.strip_prefix("root:").map(PathBuf::from)
}

// ---- mapping -----------------------------------------------------------------------------------------

/// UTC `YYYY-MM-DDTHH:MM:SSZ` of unix seconds (the string form servers use, sortable as text).
fn iso(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn blank(origin: SourceId, id: String, title: String, kind: Kind) -> Media {
    Media {
        origin,
        id,
        title,
        kind,
        collection: None,
        year: None,
        overview: None,
        runtime_min: None,
        rating: None,
        number: None,
        season_number: None,
        poster_tag: None,
        backdrop_tag: None,
        genres: vec![],
        series_id: None,
        series_name: None,
        resume_s: None,
        duration_s: None,
        played: false,
        favorite: false,
        can_delete: false,
        official_rating: None,
        unplayed: None,
        child_count: None,
        blur_primary: None,
        blur_backdrop: None,
        sub: None,
        added: None,
        premiere: None,
        logo: None,
        backdrop_id: None,
        ids: nl::ProviderIds::default(),
        available: true,
        details_from: None,
    }
}

/// Art presence marks double as cache tags (the UI treats "no tag" as "no image").
fn art_tags(m: &mut Media, art: &nl::Art) {
    m.poster_tag = art.poster.as_ref().map(|_| "local".into());
    m.backdrop_tag = art.backdrop.as_ref().map(|_| "local".into());
}

fn view_media(lib: &Library, origin: SourceId, v: &nl::View) -> Media {
    let kind = match v.kind {
        nl::Kind::Movie => Kind::Movie,
        nl::Kind::Episode => Kind::Episode,
        nl::Kind::Track => Kind::Audio,
        nl::Kind::Unmatched => Kind::Video,
    };
    let mut m = blank(origin, v.id.0.to_string(), v.title.clone(), kind);
    let st = lib.state(v.id);
    m.year = v.year.map(u32::from);
    m.overview = v.plot.clone().filter(|s| !s.trim().is_empty());
    m.runtime_min = v.runtime_min.filter(|r| *r > 0);
    m.duration_s = v.runtime_min.map(|r| f64::from(r) * 60.0);
    m.rating = v.rating;
    m.genres = v.genres.clone();
    m.ids = v.ids.clone();
    m.available = v.status == nl::Status::Available;
    m.details_from = v.details_from.map(String::from);
    m.played = st.played;
    m.resume_s = (st.resume_secs > 0 && !st.played).then_some(st.resume_secs as f64);
    m.added = Some(iso(v.added));
    match v.kind {
        nl::Kind::Episode => {
            (m.number, m.season_number) = (v.episode, v.season);
            if let Some(show) = &v.show {
                m.series_id = Some(series_id(show));
                m.series_name = Some(show.clone());
                m.sub = Some(show.clone());
            }
        }
        nl::Kind::Track => {
            m.number = v.track;
            m.sub = v.artist.clone().or(v.album.clone());
        }
        _ => {}
    }
    art_tags(&mut m, &v.art);
    m
}

/// Per-show numbers gathered in one pass over the episodes.
#[derive(Default)]
struct Agg {
    total: u32,
    unplayed: u32,
    newest: u64,
    last_played: u64,
    roots: Vec<PathBuf>,
}

fn aggregate(lib: &Library) -> HashMap<String, Agg> {
    let mut m: HashMap<String, Agg> = HashMap::new();
    for v in lib.items(nl::Kind::Episode) {
        let Some(show) = &v.show else { continue };
        let a = m.entry(nl::fold(show)).or_default();
        let st = lib.state(v.id);
        a.total += 1;
        a.unplayed += u32::from(!st.played);
        a.newest = a.newest.max(v.added);
        a.last_played = a.last_played.max(st.last_played);
        if let Some(r) = root_for(lib, &v.path)
            && !a.roots.contains(&r)
        {
            a.roots.push(r);
        }
    }
    m
}

fn root_for(lib: &Library, path: &Path) -> Option<PathBuf> {
    lib.libraries()
        .into_iter()
        .map(|l| l.root)
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.as_os_str().len())
}

fn series_media(origin: SourceId, s: &nl::Series, a: Option<&Agg>) -> Media {
    let mut m = blank(origin, series_id(&s.show), s.name.clone(), Kind::Series);
    m.year = s.year.map(u32::from);
    m.overview = s.plot.clone().filter(|p| !p.trim().is_empty());
    m.genres = s.genres.clone();
    m.rating = s.rating;
    m.ids = s.ids.clone();
    m.details_from = s.details_from.map(String::from);
    m.child_count = Some(s.seasons.len() as u32);
    m.unplayed = a.map(|a| a.unplayed);
    m.played = a.is_some_and(|a| a.total > 0 && a.unplayed == 0);
    m.added = a.map(|a| iso(a.newest));
    m.series_name = Some(s.name.clone());
    art_tags(&mut m, &s.art);
    m
}

/// `(folded show name, series, last played)` for the shows in `root` (or all).
fn series_list(lib: &Library, origin: SourceId, root: Option<&Path>) -> Vec<(String, Media, u64)> {
    let agg = aggregate(lib);
    lib.series()
        .iter()
        .filter_map(|s| {
            let a = agg.get(&nl::fold(&s.show));
            if let Some(r) = root
                && !a.is_some_and(|a| a.roots.iter().any(|x| x == r))
            {
                return None;
            }
            Some((
                nl::fold(&s.show),
                series_media(origin, s, a),
                a.map_or(0, |a| a.last_played),
            ))
        })
        .collect()
}

fn libraries(lib: &Library, origin: SourceId) -> Vec<Media> {
    lib.libraries()
        .into_iter()
        .map(|l| {
            let mut m = blank(
                origin,
                format!("root:{}", l.root.display()),
                l.name,
                Kind::Library,
            );
            m.collection = match l.kind {
                nl::LibraryKind::Movies => Some("movies".into()),
                nl::LibraryKind::Shows => Some("tvshows".into()),
                nl::LibraryKind::Music => Some("music".into()),
                nl::LibraryKind::Mixed => None,
            };
            m.child_count = Some(l.items as u32);
            m
        })
        .collect()
}

/// Any record by id (item, series, season).
fn one(lib: &Library, origin: SourceId, id: &str) -> Option<Media> {
    if let Some(show) = show_of(id) {
        let key = nl::fold(show);
        let agg = aggregate(lib);
        let s = lib
            .series()
            .into_iter()
            .find(|s| nl::fold(&s.show) == key)?;
        let m = series_media(origin, &s, agg.get(&key));
        return Some(match season_of(id) {
            Some(n) => season_media(lib, origin, &m, n),
            None => m,
        });
    }
    lib.view(parse_id(id)?).map(|v| view_media(lib, origin, &v))
}

fn season_media(lib: &Library, origin: SourceId, series: &Media, n: u32) -> Media {
    let show = show_of(&series.id).unwrap_or_default();
    let eps = lib.episodes(show, n);
    let mut m = blank(
        origin,
        season_id(show, n),
        if n == 0 {
            "Specials".into()
        } else {
            format!("Season {n}")
        },
        Kind::Season,
    );
    m.number = Some(n);
    m.series_id = Some(series.id.clone());
    m.series_name = series.series_name.clone();
    m.child_count = Some(eps.len() as u32);
    m.unplayed = Some(eps.iter().filter(|v| !lib.state(v.id).played).count() as u32);
    m.poster_tag = lib.season_poster(show, n).map(|_| "local".into());
    m.ids = series.ids.clone();
    m
}

fn seasons(lib: &Library, origin: SourceId, show: &str) -> Vec<Media> {
    let key = nl::fold(show);
    let Some(s) = lib.series().into_iter().find(|s| nl::fold(&s.show) == key) else {
        return vec![];
    };
    let series = series_media(origin, &s, None);
    lib.seasons(show)
        .into_iter()
        .map(|n| season_media(lib, origin, &series, n))
        .collect()
}

/// Newest first: movies and tracks as themselves, episodes as their show.
fn latest(lib: &Library, origin: SourceId, root: Option<&Path>, n: usize) -> Vec<Media> {
    let mut shows: HashMap<String, Media> = series_list(lib, origin, root)
        .into_iter()
        .map(|(k, m, _)| (k, m))
        .collect();
    let mut out = vec![];
    for v in lib.recently_added(usize::MAX) {
        if root.is_some_and(|r| !v.path.starts_with(r)) {
            continue;
        }
        if v.kind == nl::Kind::Episode {
            // `remove`: each show once, at its newest episode.
            if let Some(m) = v.show.as_deref().and_then(|s| shows.remove(&nl::fold(s))) {
                out.push(m);
            }
        } else {
            out.push(view_media(lib, origin, &v));
        }
        if out.len() >= n {
            break;
        }
    }
    out
}

/// After the most recently watched episode of each show, the first unwatched one.
fn next_up(lib: &Library, origin: SourceId, only: Option<&str>, n: usize) -> Vec<Media> {
    let mut by_show: HashMap<String, Vec<nl::View>> = HashMap::new();
    for v in lib.items(nl::Kind::Episode) {
        if let Some(show) = &v.show {
            by_show.entry(nl::fold(show)).or_default().push(v);
        }
    }
    let only = only.map(nl::fold);
    let mut out: Vec<(u64, Media)> = vec![];
    for (key, mut eps) in by_show {
        if only.as_ref().is_some_and(|o| *o != key) {
            continue;
        }
        eps.sort_by_key(|v| (v.season, v.episode, v.id));
        let last = eps
            .iter()
            .enumerate()
            .filter(|(_, v)| lib.state(v.id).played)
            .max_by_key(|(_, v)| lib.state(v.id).last_played);
        let Some((i, _)) = last else { continue };
        if let Some(next) = eps[i + 1..].iter().find(|v| !lib.state(v.id).played) {
            out.push((
                lib.state(eps[i].id).last_played,
                view_media(lib, origin, next),
            ));
        }
    }
    out.sort_by_key(|(t, m)| std::cmp::Reverse((*t, m.id.clone())));
    out.into_iter().map(|(_, m)| m).take(n).collect()
}

fn search(lib: &Library, origin: SourceId, term: &str) -> Vec<Media> {
    let q = nl::fold(term.trim());
    if q.is_empty() {
        return vec![];
    }
    let mut out: Vec<Media> = series_list(lib, origin, None)
        .into_iter()
        .map(|(_, m, _)| m)
        .filter(|m| nl::fold(&m.title).contains(&q))
        .collect();
    out.extend(lib.search(term).iter().map(|v| view_media(lib, origin, v)));
    out.truncate(60);
    out
}

fn browse(lib: &Library, origin: SourceId, q: &BrowseQuery) -> (Vec<Media>, usize) {
    // Scope: one library (root folder), one show, or everything.
    let root = q.parent_id.as_deref().and_then(root_of);
    if let Some(p) = &q.parent_id
        && root.is_none()
    {
        // A show or season: its episodes.
        let Some(show) = show_of(p) else {
            return (vec![], 0);
        };
        let seasons = season_of(p).map_or_else(|| lib.seasons(show), |n| vec![n]);
        let v: Vec<Media> = seasons
            .into_iter()
            .flat_map(|n| lib.episodes(show, n))
            .map(|v| view_media(lib, origin, &v))
            .collect();
        return page(v, q);
    }
    let kinds = wanted_kinds(lib, root.as_deref(), &q.include_types);
    let mut items: Vec<(Media, u64)> = vec![];
    for k in [
        nl::Kind::Movie,
        nl::Kind::Episode,
        nl::Kind::Track,
        nl::Kind::Unmatched,
    ] {
        let t = match k {
            nl::Kind::Movie => "Movie",
            nl::Kind::Episode => "Episode",
            nl::Kind::Track => "Audio",
            nl::Kind::Unmatched => "Video",
        };
        if !kinds.iter().any(|w| w == t) {
            continue;
        }
        items.extend(
            lib.items(k)
                .iter()
                .filter(|v| root.as_ref().is_none_or(|r| v.path.starts_with(r)))
                .map(|v| (view_media(lib, origin, v), lib.state(v.id).last_played)),
        );
    }
    if kinds.iter().any(|w| w == "Series") {
        items.extend(
            series_list(lib, origin, root.as_deref())
                .into_iter()
                .map(|(_, m, p)| (m, p)),
        );
    }
    // ponytail: no favourites, studios, people, tags, ratings or stream filters locally (has_subtitles,
    // is_4k, ... are ignored); the first four select nothing, the rest do not narrow.
    if q.filters.iter().any(|f| f == "IsFavorite")
        || !q.studio_ids.is_empty()
        || !q.person_ids.is_empty()
        || !q.album_artist_ids.is_empty()
        || !q.tags.is_empty()
        || !q.official_ratings.is_empty()
    {
        return (vec![], 0);
    }
    let want_played = |f: &str| q.filters.iter().any(|x| x == f);
    let genres: Vec<String> = q.genres.iter().map(|g| nl::fold(g)).collect();
    let term = q.search_term.as_deref().map(nl::fold);
    items.retain(|(m, _)| {
        let name = nl::fold(&m.title);
        (!want_played("IsPlayed") || m.played)
            && (!want_played("IsUnplayed") || !m.played)
            && (genres.is_empty() || m.genres.iter().any(|g| genres.contains(&nl::fold(g))))
            && (q.years.is_empty() || m.year.is_some_and(|y| q.years.contains(&y)))
            && q.name_starts_with
                .as_deref()
                .is_none_or(|l| name.starts_with(&nl::fold(l)))
            && q.name_less_than
                .as_deref()
                .is_none_or(|l| name.as_str() < nl::fold(l).as_str())
            && term.as_ref().is_none_or(|t| name.contains(t))
    });
    let key = q.sort_by.first().map(String::as_str).unwrap_or("SortName");
    let salt = nl::now_secs();
    let sort_key = |m: &Media, played_at: u64| -> (i64, String) {
        let n = match key {
            "DateCreated" => m.added.as_deref().map_or(0, iso_rank),
            "PremiereDate" | "ProductionYear" => m.year.map_or(0, i64::from),
            "CommunityRating" => m.rating.map_or(0, |r| (r * 100.0) as i64),
            "Runtime" => m.runtime_min.map_or(0, i64::from),
            "DatePlayed" => played_at as i64,
            "Random" => {
                let mut h = salt;
                for b in m.id.bytes() {
                    h = h.wrapping_mul(0x100_0000_01b3) ^ u64::from(b);
                }
                (h >> 1) as i64
            }
            _ => 0,
        };
        (n, nl::fold(&m.title))
    };
    items.sort_by_cached_key(|(m, p)| sort_key(m, *p));
    if q.descending {
        items.reverse();
    }
    page(items.into_iter().map(|(m, _)| m).collect(), q)
}

/// Order of an ISO timestamp as a number (digits only; fine for comparing two such strings).
fn iso_rank(s: &str) -> i64 {
    s.bytes()
        .filter(u8::is_ascii_digit)
        .fold(0i64, |a, b| a * 10 + i64::from(b - b'0'))
}

fn page(v: Vec<Media>, q: &BrowseQuery) -> (Vec<Media>, usize) {
    let total = v.len();
    let start = (q.start_index.unwrap_or(0) as usize).min(total);
    let end = q.limit.map_or(total, |l| (start + l as usize).min(total));
    (v[start..end].to_vec(), total)
}

/// Item types a browse asks for; an empty request is answered by what the library holds.
fn wanted_kinds(lib: &Library, root: Option<&Path>, types: &[String]) -> Vec<String> {
    if !types.is_empty() {
        return types.to_vec();
    }
    let kind = root.and_then(|r| lib.libraries().into_iter().find(|l| l.root == r));
    let v: &[&str] = match kind.map(|l| l.kind) {
        Some(nl::LibraryKind::Movies) => &["Movie"],
        Some(nl::LibraryKind::Shows) => &["Series"],
        Some(nl::LibraryKind::Music) => &["Audio"],
        _ => &["Movie", "Series", "Audio", "Video"],
    };
    v.iter().map(|s| s.to_string()).collect()
}

fn art_path(lib: &Library, id: &str, kind: Kind, art: Art) -> Option<PathBuf> {
    let pick = |a: &nl::Art| match art {
        Art::Poster => a.poster.clone(),
        Art::Backdrop => a.backdrop.clone(),
        Art::Landscape => a.backdrop.clone().or(a.poster.clone()),
        Art::Logo | Art::Chapter => None,
    };
    if let Some(show) = show_of(id) {
        let key = nl::fold(show);
        let s = lib
            .series()
            .into_iter()
            .find(|s| nl::fold(&s.show) == key)?;
        if kind == Kind::Season
            && art == Art::Poster
            && let Some(p) = season_of(id).and_then(|n| lib.season_poster(show, n))
        {
            return Some(p);
        }
        return pick(&s.art);
    }
    pick(&lib.art(parse_id(id)?)?)
}

fn read_capped(p: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(p).ok()?;
    (meta.is_file() && meta.len() <= ART_MAX)
        .then(|| std::fs::read(p).ok())
        .flatten()
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// A temp media tree (tiny unique files) scanned into a library under the same temp dir.
    /// Roots are each top-level directory in `files` (`Films/...`, `Shows/...`).
    pub fn build(origin: SourceId, files: &[&str]) -> (tempfile::TempDir, LocalBackend) {
        let t = tempfile::tempdir().unwrap();
        let media = t.path().join("media");
        let mut roots: Vec<PathBuf> = vec![];
        for rel in files {
            let p = media.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, format!("content of {rel}")).unwrap();
            let root = media.join(rel.split('/').next().unwrap());
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        let mut lib = Library::open(t.path().join("data")).unwrap();
        lib.scan(&roots, &AtomicBool::new(false), |_| {}).unwrap();
        (t, LocalBackend::new(origin, lib))
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::build;
    use super::*;

    const O: SourceId = SourceId(7);

    fn q(parent: &Media) -> BrowseQuery {
        BrowseQuery {
            parent_id: Some(parent.id.clone()),
            ..Default::default()
        }
    }

    const FILES: &[&str] = &[
        "Films/Heat (1995) [imdbid-tt0113277]/Heat (1995) [imdbid-tt0113277].mkv",
        "Films/The.Matrix.1999.1080p.mkv",
        "Films/Alien (1979).mkv",
        "Shows/Show/Season 01/Show - S01E01 - Pilot.mkv",
        "Shows/Show/Season 01/Show - S01E02 - Two.mkv",
        "Shows/Show/Season 02/Show - S02E01 - Back.mkv",
    ];

    async fn m_by(b: &LocalBackend, lib: &Media, title: &str) -> Media {
        b.browse(&q(lib))
            .await
            .unwrap()
            .0
            .into_iter()
            .find(|m| m.title == title)
            .unwrap()
    }

    #[tokio::test]
    async fn libraries_browse_detail_and_play_target() {
        let (_t, b) = build(O, FILES);
        let libs = b.libraries().await.unwrap();
        assert_eq!(libs.len(), 2);
        assert!(
            libs.iter()
                .all(|l| l.origin == O && l.kind == Kind::Library)
        );
        let films = libs.iter().find(|l| l.title == "Films").unwrap();
        let shows = libs.iter().find(|l| l.title == "Shows").unwrap();
        assert_eq!(films.collection.as_deref(), Some("movies"));
        assert_eq!(shows.collection.as_deref(), Some("tvshows"));

        // browse: name order, letter, paging with the total.
        let (m, total) = b.browse(&q(films)).await.unwrap();
        let names: Vec<_> = m.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(names, ["Alien", "Heat", "The Matrix"]);
        assert_eq!(total, 3);
        let mut page = q(films);
        (page.start_index, page.limit) = (Some(1), Some(1));
        let (m, total) = b.browse(&page).await.unwrap();
        assert_eq!((m[0].title.as_str(), total), ("Heat", 3));
        let mut letter = q(films);
        letter.name_starts_with = Some("T".into());
        assert_eq!(b.browse(&letter).await.unwrap().0[0].title, "The Matrix");
        let mut desc = q(films);
        (desc.sort_by, desc.descending) = (vec!["SortName".into()], true);
        assert_eq!(b.browse(&desc).await.unwrap().0[0].title, "The Matrix");

        // the show library lists series, not episodes.
        let (s, _) = b.browse(&q(shows)).await.unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].kind, s[0].origin), (Kind::Series, O));
        assert_eq!(s[0].unplayed, Some(3));

        // detail, seasons, episodes.
        let heat = m_by(&b, films, "Heat").await;
        assert_eq!(heat.ids.imdb.as_deref(), Some("tt0113277"));
        let (full, _) = b.full(&heat.id).await.unwrap();
        assert_eq!((full.title.as_str(), full.year), ("Heat", Some(1995)));
        let seasons = b.seasons(&s[0].id).await.unwrap();
        assert_eq!(
            seasons.iter().map(|x| x.number).collect::<Vec<_>>(),
            [Some(1), Some(2)]
        );
        let eps = b.episodes(&s[0].id, &seasons[0].id).await.unwrap();
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].series_id.as_deref(), Some(s[0].id.as_str()));
        assert_eq!(eps[1].short_ep(), "S1:E2");
        assert_eq!(b.full(&s[0].id).await.unwrap().0.kind, Kind::Series);
        assert_eq!(b.full(&seasons[1].id).await.unwrap().0.kind, Kind::Season);
        assert!(b.full("999999").await.is_err());

        // ids are answered for the episode merge.
        let ids = b
            .provider_ids(&[s[0].id.clone(), heat.id.clone()])
            .await
            .unwrap();
        assert_eq!(ids[&heat.id].imdb.as_deref(), Some("tt0113277"));
        assert!(ids.contains_key(&s[0].id));

        // play target is the file itself; episodes resolve too.
        let be = crate::backend::Backend::Local(b.clone());
        let crate::backend::PlayTarget::File(p) = be.play_target(&heat).await.unwrap() else {
            panic!("file expected")
        };
        assert!(p.is_file());
        assert!(be.play_target(&eps[0]).await.is_ok());
    }

    #[tokio::test]
    async fn missing_and_unreadable_files_give_typed_errors_with_messages() {
        use crate::backend::{Backend, PlayError};
        let (t, b) = build(O, FILES);
        let be = Backend::Local(b.clone());
        let libs = b.libraries().await.unwrap();
        let films = libs.iter().find(|l| l.title == "Films").unwrap();
        let alien = m_by(&b, films, "Alien").await;
        let path = t.path().join("media/Films/Alien (1979).mkv");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(be.play_target(&alien).await, Err(PlayError::Missing));
        assert!(PlayError::Missing.message().contains("no longer"));
        let mut gone = alien.clone();
        gone.id = "424242".into();
        assert_eq!(be.play_target(&gone).await, Err(PlayError::NotPlayable));
        let series = b.search("Show").await.unwrap().remove(0);
        assert_eq!(series.kind, Kind::Series);
        assert_eq!(be.play_target(&series).await, Err(PlayError::NotPlayable));

        // unreadable (skipped where permissions do not apply, e.g. running as root)
        let matrix = m_by(&b, films, "The Matrix").await;
        let p2 = t.path().join("media/Films/The.Matrix.1999.1080p.mkv");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p2, std::fs::Permissions::from_mode(0o000)).unwrap();
        }
        if std::fs::File::open(&p2).is_err() {
            assert_eq!(be.play_target(&matrix).await, Err(PlayError::Unreadable));
        }
    }

    #[tokio::test]
    async fn resume_next_up_search_and_state_stay_local() {
        let (_t, b) = build(O, FILES);
        let libs = b.libraries().await.unwrap();
        let shows = libs.iter().find(|l| l.title == "Shows").unwrap();
        let series = b.browse(&q(shows)).await.unwrap().0.remove(0);
        let seasons = b.seasons(&series.id).await.unwrap();
        let eps = b.episodes(&series.id, &seasons[0].id).await.unwrap();
        assert!(b.next_up().await.unwrap().is_empty(), "nothing watched yet");
        b.set_played(&eps[0], true).await.unwrap();
        let nu = b.next_up().await.unwrap();
        assert_eq!(nu.len(), 1);
        assert_eq!(nu[0].id, eps[1].id, "the episode after the last played");
        assert_eq!(b.series_next_up(&series.id).await.unwrap()[0].id, eps[1].id);
        b.set_progress(&eps[1], 90).await.unwrap();
        let r = b.resume().await.unwrap();
        assert_eq!((r.len(), r[0].resume_s), (1, Some(90.0)));
        let hits = b.search("show").await.unwrap();
        assert_eq!(hits[0].kind, Kind::Series);
        assert!(hits.iter().any(|m| m.kind == Kind::Episode));
        assert!(b.search("zzzz").await.unwrap().is_empty());
        let latest = b.latest(shows).await.unwrap();
        assert_eq!(latest.len(), 1, "a show once, however many new episodes");
    }

    #[tokio::test]
    async fn art_comes_from_the_sidecar_and_missing_art_is_none() {
        let (t, b) = build(
            O,
            &[
                "Films/Heat (1995)/Heat (1995).mkv",
                "Films/Alien (1979).mkv",
            ],
        );
        let films = b.libraries().await.unwrap().remove(0);
        let heat = m_by(&b, &films, "Heat").await;
        assert!(heat.poster_tag.is_none(), "no art scanned yet");
        assert_eq!(b.image(&heat, Art::Poster).await, None);
        let dir = t.path().join("media/Films/Heat (1995)");
        std::fs::write(dir.join("poster.jpg"), [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
        b.with_mut(|l| {
            l.scan(
                &[t.path().join("media/Films")],
                &std::sync::atomic::AtomicBool::new(false),
                |_| {},
            )
        })
        .unwrap();
        let heat = m_by(&b, &films, "Heat").await;
        assert!(heat.poster_tag.is_some());
        assert_eq!(
            b.image(&heat, Art::Poster).await,
            Some(vec![0xFF, 0xD8, 0xFF, 0xE0])
        );
        assert_eq!(b.image(&heat, Art::Logo).await, None);
        let alien = m_by(&b, &films, "Alien").await;
        assert_eq!(b.image(&alien, Art::Poster).await, None);
    }

    #[test]
    fn iso_dates_and_ids() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(show_of("show:A#B"), Some("A#B"));
        assert_eq!(
            (show_of("show:A#2"), season_of("show:A#2")),
            (Some("A"), Some(2))
        );
        assert_eq!(show_of("12"), None);
    }
}
