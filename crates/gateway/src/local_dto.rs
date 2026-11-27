//! Local library items as Jellyfin items (plan sections 1 and 3, decision 3): direct play only, files opened read-only and only through the index, never by a client-supplied path.
//! The gateway opens the same `$XDG_DATA_HOME/nocturne/local/<source id>/` the app maintains; it never scans and never writes anywhere but that library's own `state.json` (resume and watched).
//!
//! Member ids (the id map's `Member::id` for source `"local"`): `<src>:<local id>` for a file, `series:<src>:<show>`, `season:<src>:<show>:<n>`, `view:<src>:<movies|shows|music>`.
use crate::idmap::{Gid, IdMap, Member, key_local};
use crate::rewrite::Aliases;
use crate::state::State;
use nocturne_core::source::SourceId;
use nocturne_local::{Kind, Library, LibraryKind, LocalId, Series, View, fold};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// The id-map source key of every local source.
pub const SRC: &str = "local";
const TICKS: i64 = 10_000_000;

struct Loaded {
    stamp: Option<SystemTime>,
    lib: Library,
}

/// The opened libraries, one per local source, reopened when the app rescanned (the index file's mtime changed).
#[derive(Default)]
pub struct Libs(Mutex<HashMap<u32, Arc<Mutex<Loaded>>>>);

pub fn dir(st: &State, id: SourceId) -> PathBuf {
    st.paths
        .data
        .parent()
        .unwrap_or(&st.paths.data)
        .join("local")
        .join(id.0.to_string())
}

/// Run `f` on the library of `id` (blocking: call from `spawn_blocking` or accept the stall for a small index). `None` when it cannot be opened.
pub fn with_lib<T>(st: &State, id: SourceId, f: impl FnOnce(&mut Library) -> T) -> Option<T> {
    let d = dir(st, id);
    let stamp = std::fs::metadata(d.join("index.json"))
        .and_then(|m| m.modified())
        .ok();
    let libs = st.ext::<Libs>();
    let slot = {
        let mut g = libs.0.lock().unwrap();
        match g.get(&id.0) {
            Some(s) if s.lock().unwrap().stamp == stamp => s.clone(),
            _ => {
                let lib = Library::open(&d).ok()?;
                let s = Arc::new(Mutex::new(Loaded { stamp, lib }));
                g.insert(id.0, s.clone());
                s
            }
        }
    };
    let mut l = slot.lock().unwrap();
    Some(f(&mut l.lib))
}

pub fn item_id(src: SourceId, lid: LocalId) -> String {
    format!("{}:{}", src.0, lid.0)
}

/// `<src>:<local id>`.
pub fn parse_item(s: &str) -> Option<(SourceId, LocalId)> {
    let (a, b) = s.split_once(':')?;
    Some((SourceId(a.parse().ok()?), LocalId(b.parse().ok()?)))
}

pub enum Target {
    File(SourceId, LocalId),
    Series(SourceId, String),
    Season(SourceId, String, u32),
    View(SourceId, LibraryKind),
}

pub fn src_of(t: &Target) -> SourceId {
    match t {
        Target::File(s, _) | Target::Series(s, _) | Target::Season(s, ..) | Target::View(s, _) => {
            *s
        }
    }
}

pub fn parse_member(s: &str) -> Option<Target> {
    if let Some(r) = s.strip_prefix("series:") {
        let (a, show) = r.split_once(':')?;
        return Some(Target::Series(SourceId(a.parse().ok()?), show.into()));
    }
    if let Some(r) = s.strip_prefix("season:") {
        let (a, r) = r.split_once(':')?;
        let (show, n) = r.rsplit_once(':')?;
        return Some(Target::Season(
            SourceId(a.parse().ok()?),
            show.into(),
            n.parse().ok()?,
        ));
    }
    if let Some(r) = s.strip_prefix("view:") {
        let (a, k) = r.split_once(':')?;
        let k = match k {
            "movies" => LibraryKind::Movies,
            "shows" => LibraryKind::Shows,
            "music" => LibraryKind::Music,
            _ => return None,
        };
        return Some(Target::View(SourceId(a.parse().ok()?), k));
    }
    parse_item(s).map(|(a, b)| Target::File(a, b))
}

pub struct Ctx<'a> {
    pub ids: &'a IdMap,
    pub alias: &'a Aliases,
    pub server_id: &'a str,
    pub src: SourceId,
}

impl Ctx<'_> {
    /// The gateway id of a local thing (aliased to the merged id once a group has been merged).
    fn gid(&self, member: &str) -> Gid {
        let g = self.ids.register(
            &key_local(member),
            &[Member {
                source: SRC.into(),
                id: member.into(),
            }],
        );
        self.alias.get(g)
    }
}

fn iso(unix: u64) -> String {
    crate::auth::iso(unix)
}

fn ids_json(p: &nocturne_local::ProviderIds) -> Value {
    let mut m = Map::new();
    for (k, v) in [("Imdb", &p.imdb), ("Tmdb", &p.tmdb), ("Tvdb", &p.tvdb)] {
        if let Some(v) = v {
            m.insert(k.into(), v.clone().into());
        }
    }
    Value::Object(m)
}

fn base(c: &Ctx, member: &str, name: &str, kind: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("Name".into(), name.into());
    m.insert("SortName".into(), fold(name).into());
    m.insert("Id".into(), member.into());
    m.insert("ServerId".into(), c.server_id.into());
    m.insert("Type".into(), kind.into());
    m.insert("LocationType".into(), "FileSystem".into());
    m.insert("ImageTags".into(), json!({}));
    m.insert("BackdropImageTags".into(), json!([]));
    m
}

fn art(m: &mut Map<String, Value>, a: &nocturne_local::Art) {
    let mut tags = Map::new();
    if a.poster.is_some() {
        tags.insert("Primary".into(), "local".into());
    }
    if a.logo.is_some() {
        tags.insert("Logo".into(), "local".into());
    }
    m.insert("ImageTags".into(), Value::Object(tags));
    if a.backdrop.is_some() {
        m.insert("BackdropImageTags".into(), json!(["local"]));
    }
}

/// A file: movie, episode or track. `full` adds the media source (needs a `stat` of the file).
pub fn file_dto(lib: &Library, v: &View, c: &Ctx, full: bool) -> Value {
    let member = item_id(c.src, v.id);
    let kind = match v.kind {
        Kind::Movie => "Movie",
        Kind::Episode => "Episode",
        _ => "Audio",
    };
    let mut m = base(c, &member, &v.title, kind);
    m.insert(
        "MediaType".into(),
        if v.media == nocturne_local::Media::Audio {
            "Audio"
        } else {
            "Video"
        }
        .into(),
    );
    m.insert("IsFolder".into(), false.into());
    if let Some(y) = v.year {
        m.insert("ProductionYear".into(), y.into());
    }
    if let Some(p) = &v.plot {
        m.insert("Overview".into(), p.clone().into());
    }
    if let Some(r) = v.rating {
        m.insert("CommunityRating".into(), r.into());
    }
    if let Some(r) = v.runtime_min {
        m.insert("RunTimeTicks".into(), (i64::from(r) * 60 * TICKS).into());
    }
    m.insert("Genres".into(), json!(v.genres));
    m.insert("ProviderIds".into(), ids_json(&v.ids));
    m.insert("DateCreated".into(), iso(v.added).into());
    art(&mut m, &v.art);
    if let (Kind::Episode, Some(show)) = (v.kind, &v.show) {
        let sm = format!("series:{}:{show}", c.src.0);
        m.insert("SeriesName".into(), show.clone().into());
        m.insert("SeriesId".into(), c.gid(&sm).to_string().into());
        if let Some(s) = v.season {
            m.insert("ParentIndexNumber".into(), s.into());
            let seasonm = format!("season:{}:{show}:{s}", c.src.0);
            m.insert("SeasonId".into(), c.gid(&seasonm).to_string().into());
            m.insert("ParentId".into(), c.gid(&seasonm).to_string().into());
        }
        if let Some(e) = v.episode {
            m.insert("IndexNumber".into(), e.into());
        }
    }
    if v.kind == Kind::Track {
        if let Some(a) = &v.album {
            m.insert("Album".into(), a.clone().into());
        }
        if let Some(a) = &v.artist {
            m.insert("Artists".into(), json!([a]));
            m.insert("AlbumArtist".into(), a.clone().into());
        }
        if let Some(t) = v.track {
            m.insert("IndexNumber".into(), t.into());
        }
    }
    let st = lib.state(v.id);
    let mut ud = json!({
        "PlaybackPositionTicks": st.resume_secs as i64 * TICKS,
        "PlayCount": u8::from(st.played),
        "IsFavorite": false,
        "Played": st.played,
        "Key": member,
    });
    if st.last_played > 0 {
        ud["LastPlayedDate"] = iso(st.last_played).into();
    }
    m.insert("UserData".into(), ud);
    if full {
        let ext = v
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let size = std::fs::metadata(&v.path).map_or(0, |m| m.len());
        let mut s = json!({
            "Id": member, "Protocol": "File", "Container": ext, "Size": size, "Name": v.title,
            "IsRemote": false, "Type": "Default", "ReadAtNativeFramerate": false,
            "SupportsDirectPlay": true, "SupportsDirectStream": true, "SupportsTranscoding": false,
            "MediaStreams": [], "MediaAttachments": [], "Formats": [], "RequiredHttpHeaders": {},
        });
        if let Some(r) = m.get("RunTimeTicks") {
            s["RunTimeTicks"] = r.clone();
        }
        m.insert("MediaSources".into(), json!([s]));
        m.insert("Container".into(), ext.into());
    }
    Value::Object(m)
}

pub fn series_dto(s: &Series, c: &Ctx) -> Value {
    let member = format!("series:{}:{}", c.src.0, s.show);
    let mut m = base(c, &member, &s.name, "Series");
    m.insert("IsFolder".into(), true.into());
    if let Some(y) = s.year {
        m.insert("ProductionYear".into(), y.into());
    }
    if let Some(p) = &s.plot {
        m.insert("Overview".into(), p.clone().into());
    }
    if let Some(r) = s.rating {
        m.insert("CommunityRating".into(), r.into());
    }
    m.insert("Genres".into(), json!(s.genres));
    m.insert("ProviderIds".into(), ids_json(&s.ids));
    m.insert("ChildCount".into(), s.seasons.len().into());
    m.insert("RecursiveItemCount".into(), s.episodes.into());
    m.insert(
        "UserData".into(),
        json!({"PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false, "Played": false, "Key": member}),
    );
    art(&mut m, &s.art);
    Value::Object(m)
}

pub fn season_dto(lib: &Library, show: &str, n: u32, c: &Ctx) -> Value {
    let member = format!("season:{}:{show}:{n}", c.src.0);
    let name = if n == 0 {
        "Specials".to_string()
    } else {
        format!("Season {n}")
    };
    let mut m = base(c, &member, &name, "Season");
    m.insert("IsFolder".into(), true.into());
    m.insert("IndexNumber".into(), n.into());
    m.insert("SeriesName".into(), show.into());
    let sg = c.gid(&format!("series:{}:{show}", c.src.0));
    m.insert("SeriesId".into(), sg.to_string().into());
    m.insert("ParentId".into(), sg.to_string().into());
    m.insert("ChildCount".into(), lib.episodes(show, n).len().into());
    m.insert(
        "UserData".into(),
        json!({"PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false, "Played": false, "Key": member}),
    );
    if lib.season_poster(show, n).is_some() {
        m.insert("ImageTags".into(), json!({"Primary": "local"}));
    }
    Value::Object(m)
}

pub fn view_kind_name(k: LibraryKind) -> Option<(&'static str, &'static str)> {
    match k {
        LibraryKind::Movies => Some(("movies", "movies")),
        LibraryKind::Shows => Some(("shows", "tvshows")),
        LibraryKind::Music => Some(("music", "music")),
        LibraryKind::Mixed => None,
    }
}

/// Which view kinds this library has files for.
pub fn view_kinds(lib: &Library) -> Vec<LibraryKind> {
    let mut v = vec![];
    for (kind, k) in [
        (Kind::Movie, LibraryKind::Movies),
        (Kind::Episode, LibraryKind::Shows),
        (Kind::Track, LibraryKind::Music),
    ] {
        if !lib.items(kind).is_empty() {
            v.push(k);
        }
    }
    v
}

/// What a list request asks of the local library (the subset of `GET /Items` that applies to files).
#[derive(Default, Clone, Debug)]
pub struct Filter {
    /// Lowercase `IncludeItemTypes`; empty: by parent.
    pub types: Vec<String>,
    pub search: Option<String>,
    pub genres: Vec<String>,
    pub years: Vec<u16>,
    pub name_starts: Option<String>,
    pub played: Option<bool>,
    /// A `ParentId` that named a local thing: `None` = no parent given (everything), `Some(None)` = a parent that is not local (nothing).
    pub parent: Option<Option<String>>,
    /// Exactly these member ids (`Ids`), when given.
    pub ids: Option<Vec<String>>,
    pub recursive: bool,
}

/// The items of one local source matching `f`, as DTOs, unsorted.
pub fn query(lib: &Library, f: &Filter, c: &Ctx) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    if let Some(ids) = &f.ids {
        for i in ids {
            if let Some(d) = member_dto(lib, i, c, false) {
                out.push(d);
            }
        }
        return filter_rest(out, f);
    }
    let (mut movies, mut shows, mut eps, mut tracks, mut seasons) =
        (false, false, false, false, false);
    let mut show_scope: Option<(String, Option<u32>)> = None;
    match &f.parent {
        Some(None) => return vec![],
        Some(Some(p)) => match parse_member(p) {
            Some(Target::View(_, LibraryKind::Movies)) => movies = true,
            Some(Target::View(_, LibraryKind::Shows)) => {
                shows = true;
                eps = f.recursive;
            }
            Some(Target::View(_, LibraryKind::Music)) => tracks = true,
            Some(Target::Series(_, show)) => {
                show_scope = Some((show, None));
                seasons = true;
                eps = f.recursive;
            }
            Some(Target::Season(_, show, n)) => {
                show_scope = Some((show, Some(n)));
                eps = true;
            }
            _ => return vec![],
        },
        None => {
            movies = true;
            shows = true;
            tracks = true;
            eps = f.recursive;
        }
    }
    if !f.types.is_empty() {
        let has = |t: &str| f.types.iter().any(|x| x == t);
        movies &= has("movie");
        shows &= has("series");
        seasons &= has("season");
        tracks &= has("audio");
        eps &= has("episode");
        // An explicit type list with Recursive asks for those types anywhere below the parent.
        if has("episode") && f.recursive {
            eps = true;
        }
    }
    if movies {
        out.extend(
            lib.items(Kind::Movie)
                .iter()
                .map(|v| file_dto(lib, v, c, false)),
        );
    }
    if shows {
        out.extend(lib.series().iter().map(|s| series_dto(s, c)));
    }
    if seasons && let Some((show, _)) = &show_scope {
        out.extend(
            lib.seasons(show)
                .into_iter()
                .map(|n| season_dto(lib, show, n, c)),
        );
    }
    if eps {
        match &show_scope {
            Some((show, Some(n))) => out.extend(
                lib.episodes(show, *n)
                    .iter()
                    .map(|v| file_dto(lib, v, c, false)),
            ),
            Some((show, None)) => {
                for n in lib.seasons(show) {
                    out.extend(
                        lib.episodes(show, n)
                            .iter()
                            .map(|v| file_dto(lib, v, c, false)),
                    );
                }
            }
            None => out.extend(
                lib.items(Kind::Episode)
                    .iter()
                    .map(|v| file_dto(lib, v, c, false)),
            ),
        }
    }
    if tracks {
        out.extend(
            lib.items(Kind::Track)
                .iter()
                .map(|v| file_dto(lib, v, c, false)),
        );
    }
    filter_rest(out, f)
}

fn filter_rest(v: Vec<Value>, f: &Filter) -> Vec<Value> {
    let q = f.search.as_deref().map(fold);
    v.into_iter()
        .filter(|d| {
            let name = d["Name"].as_str().unwrap_or("");
            q.as_ref().is_none_or(|q| {
                fold(name).contains(q)
                    || d["SeriesName"]
                        .as_str()
                        .is_some_and(|s| fold(s).contains(q))
                    || d["Album"].as_str().is_some_and(|s| fold(s).contains(q))
            }) && f
                .name_starts
                .as_deref()
                .is_none_or(|p| fold(name).starts_with(&fold(p)))
                && (f.years.is_empty()
                    || d["ProductionYear"]
                        .as_u64()
                        .is_some_and(|y| f.years.iter().any(|x| u64::from(*x) == y)))
                && (f.genres.is_empty()
                    || d["Genres"].as_array().is_some_and(|g| {
                        g.iter()
                            .filter_map(Value::as_str)
                            .any(|g| f.genres.iter().any(|x| x.eq_ignore_ascii_case(g)))
                    }))
                && f.played
                    .is_none_or(|p| d["UserData"]["Played"].as_bool().unwrap_or(false) == p)
        })
        .collect()
}

/// One member id as a DTO (detail pages, `Ids`). Files carry their media source.
pub fn member_dto(lib: &Library, member: &str, c: &Ctx, full: bool) -> Option<Value> {
    match parse_member(member)? {
        Target::File(_, lid) => lib
            .view(lid)
            .filter(|v| !matches!(v.kind, Kind::Unmatched))
            .map(|v| file_dto(lib, &v, c, full)),
        Target::Series(_, show) => lib
            .series()
            .iter()
            .find(|s| s.show == show)
            .map(|s| series_dto(s, c)),
        Target::Season(_, show, n) => lib
            .seasons(&show)
            .contains(&n)
            .then(|| season_dto(lib, &show, n, c)),
        Target::View(..) => None,
    }
}

/// Resume list of one library (items partly watched, most recent first).
pub fn resume(lib: &Library, n: usize, c: &Ctx) -> Vec<Value> {
    lib.continue_watching(n)
        .iter()
        .map(|v| file_dto(lib, v, c, false))
        .collect()
}

/// The file behind a member id, for streaming: path from the index only, never from a client.
pub fn file_path(lib: &Library, member: &str) -> Option<PathBuf> {
    let Some(Target::File(_, lid)) = parse_member(member) else {
        return None;
    };
    lib.resolve_path(lid).ok()
}

/// A local image (poster, backdrop, logo) for any member kind.
pub fn image_path(lib: &Library, member: &str, kind: &str, index: usize) -> Option<PathBuf> {
    let pick = |a: &nocturne_local::Art| match kind.to_ascii_lowercase().as_str() {
        "primary" | "thumb" => a.poster.clone(),
        "backdrop" if index == 0 => a.backdrop.clone(),
        "logo" => a.logo.clone(),
        _ => None,
    };
    match parse_member(member)? {
        Target::File(_, lid) => lib.art(lid).and_then(|a| pick(&a)),
        Target::Series(_, show) => lib
            .series()
            .iter()
            .find(|s| s.show == show)
            .and_then(|s| pick(&s.art)),
        Target::Season(_, show, n) => lib
            .season_poster(&show, n)
            .filter(|_| kind.eq_ignore_ascii_case("primary")),
        Target::View(..) => None,
    }
}

/// Record a stop position (seconds) or a watched mark in the library's own `state.json`.
pub fn record(lib: &mut Library, member: &str, resume_secs: Option<u64>, played: Option<bool>) {
    let Some(Target::File(_, lid)) = parse_member(member) else {
        return;
    };
    if let Some(s) = resume_secs {
        let _ = lib.set_progress(lid, s);
    }
    if let Some(p) = played {
        let _ = lib.set_played(lid, p);
    }
}

/// Search hints for the search box.
pub fn search_hints(lib: &Library, term: &str, c: &Ctx) -> Vec<Value> {
    lib.search(term)
        .iter()
        .filter(|v| !matches!(v.kind, Kind::Unmatched))
        .map(|v| {
            let mut d = file_dto(lib, v, c, false);
            d["ItemId"] = d["Id"].clone();
            d
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_ids_round_trip() {
        assert!(matches!(
            parse_member("3:42"),
            Some(Target::File(SourceId(3), LocalId(42)))
        ));
        match parse_member("series:2:The Show: Part 2") {
            Some(Target::Series(SourceId(2), s)) => assert_eq!(s, "The Show: Part 2"),
            _ => panic!(),
        }
        match parse_member("season:2:The Show: Part 2:3") {
            Some(Target::Season(SourceId(2), s, 3)) => assert_eq!(s, "The Show: Part 2"),
            _ => panic!(),
        }
        assert!(matches!(
            parse_member("view:1:shows"),
            Some(Target::View(SourceId(1), LibraryKind::Shows))
        ));
        for bad in ["", "x", "view:1:other", "a:b", "series:x:y"] {
            assert!(parse_member(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn filter_applies_search_year_genre_and_played() {
        let mk = |n: &str, y: u64, g: &str, p: bool| json!({"Name": n, "ProductionYear": y, "Genres": [g], "UserData": {"Played": p}});
        let v = vec![
            mk("Heat", 1995, "Crime", false),
            mk("Alien", 1979, "Horror", true),
            mk("Élan", 2001, "Drama", false),
        ];
        let f = |f: Filter| filter_rest(v.clone(), &f).len();
        assert_eq!(
            f(Filter {
                search: Some("hea".into()),
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            f(Filter {
                search: Some("elan".into()),
                ..Default::default()
            }),
            1,
            "diacritics fold"
        );
        assert_eq!(
            f(Filter {
                years: vec![1979],
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            f(Filter {
                genres: vec!["crime".into()],
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            f(Filter {
                played: Some(false),
                ..Default::default()
            }),
            2
        );
        assert_eq!(
            f(Filter {
                name_starts: Some("a".into()),
                ..Default::default()
            }),
            1
        );
    }
}
