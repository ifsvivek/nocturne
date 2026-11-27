//! The library routes (plan sections 1 and 2): views, items, Home rows, details, seasons and episodes, filter lists and search, all answered from every source at once.
//! Servers are asked with their own tokens and ids (`Authorization` header only); answers are rewritten ([`crate::rewrite`]), copies of one title merged ([`crate::lists`]), user data merged with the overlay ([`crate::userdata`]), local items added ([`crate::local_dto`]).
use crate::auth::client_info;
use crate::idmap::{Gid, IdMap, Member, key_local, key_merge, key_single};
pub use crate::lists::enc;
use crate::lists::{self, Entry, Group, PageCache, Query, Sort, Window, encode_query};
use crate::local_dto::{self, Filter};
use crate::rewrite::{
    Aliases, JsonCtx, UrlCtx, rewrite_refs, rewrite_source, rewrite_urls_in, scrub,
};
use crate::server::{Access, Routes};
use crate::state::{State, UpKind, UpState};
use crate::userdata::{CopyData, merge_user_data, overlay};
use axum::extract::{Path, State as St};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use nocturne_core::source::SourceId;
use nocturne_jellyfin::raw::Raw;
use nocturne_jellyfin_user::raw::RawUser;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const FIELDS: &str = "ProviderIds,SortName,DateCreated,PremiereDate";
const MAX_JSON: usize = 32 << 20;
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------- sources

/// A server the gateway has a login for, with its clients.
#[derive(Clone)]
pub struct Srv {
    pub id: SourceId,
    /// Position in priority order.
    pub idx: usize,
    pub label: String,
    /// The server's stable key in the id map.
    pub key: String,
    /// `host[:port]`, lowercase.
    pub authority: String,
    /// The account's user id on that server.
    pub user_id: String,
    pub raw: Raw,
    pub write: RawUser,
}

#[derive(Clone)]
pub struct LocalSrc {
    pub id: SourceId,
    pub idx: usize,
    pub label: String,
}

/// Shared state of the library routes (`state.ext::<Hub>()`).
#[derive(Default)]
pub struct Hub {
    clients: Mutex<HashMap<u32, (String, String, Raw, RawUser)>>,
    pub cache: PageCache,
    pub alias: Aliases,
    runtimes: Mutex<HashMap<Gid, i64>>,
    view_gids: Mutex<HashSet<Gid>>,
}

pub fn hub(st: &State) -> Arc<Hub> {
    st.ext::<Hub>()
}

impl Hub {
    pub fn runtime(&self, g: Gid) -> Option<i64> {
        self.runtimes.lock().unwrap().get(&g).copied()
    }
}

pub fn authority_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            let h = u.host_str()?.to_ascii_lowercase();
            Some(match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h,
            })
        })
        .unwrap_or_default()
}

fn device_id() -> String {
    std::env::var("NOCTURNE_DEVICE_ID")
        .ok()
        .filter(|d| !d.is_empty())
        .map_or_else(|| "nocturne-gateway".into(), |d| format!("{d}-gateway"))
}

/// Every server with a usable login, in priority order.
pub fn servers(st: &State) -> Vec<Srv> {
    let h = hub(st);
    let mut out = vec![];
    for (idx, u) in st.upstreams().iter().enumerate() {
        let UpKind::Server {
            url,
            user_id,
            source_key,
            token: Some(t),
        } = &u.kind
        else {
            continue;
        };
        if u.state != UpState::Ready || source_key.is_empty() {
            continue;
        }
        let mut c = h.clients.lock().unwrap();
        let fresh = match c.get(&u.id.0) {
            Some((cu, ct, ..)) => cu != url || ct != t.expose(),
            None => true,
        };
        if fresh {
            let (Ok(raw), Ok(write)) = (
                Raw::new(url, t.expose(), &device_id()),
                RawUser::new(url, t.expose(), &device_id()),
            ) else {
                continue;
            };
            c.insert(u.id.0, (url.clone(), t.expose().to_string(), raw, write));
        }
        let (_, _, raw, write) = c.get(&u.id.0).cloned().expect("just inserted");
        out.push(Srv {
            id: u.id,
            idx,
            label: u.label.clone(),
            key: source_key.clone(),
            authority: authority_of(url),
            user_id: user_id.clone(),
            raw,
            write,
        });
    }
    out
}

pub fn locals(st: &State) -> Vec<LocalSrc> {
    st.upstreams()
        .iter()
        .enumerate()
        .filter(|(_, u)| matches!(u.kind, UpKind::Local { .. }))
        .map(|(idx, u)| LocalSrc {
            id: u.id,
            idx,
            label: u.label.clone(),
        })
        .collect()
}

/// GET and parse JSON from one server; `None` on any failure (a 401 also signs the source out). `cached`: 30 s page cache.
pub async fn get_json(st: &State, s: &Srv, pq: &str, cached: bool) -> Option<Arc<Value>> {
    let h = hub(st);
    let key = format!("{}|{pq}", s.id.0);
    if cached && let Some(v) = h.cache.get(&key) {
        return Some(v);
    }
    let r = tokio::time::timeout(
        FETCH_TIMEOUT,
        s.raw.get(pq, &[("accept", "application/json")]),
    )
    .await
    .ok()?
    .ok()?;
    if r.status() == StatusCode::UNAUTHORIZED {
        st.mark_signed_out(s.id);
        return None;
    }
    if !r.status().is_success() {
        return None;
    }
    let bytes = tokio::time::timeout(FETCH_TIMEOUT, r.bytes())
        .await
        .ok()?
        .ok()?;
    if bytes.len() > MAX_JSON {
        return None;
    }
    let v = Arc::new(serde_json::from_slice::<Value>(&bytes).ok()?);
    if cached {
        h.cache.put(key, v.clone());
    }
    Some(v)
}

/// Run the futures concurrently and return their results in order.
pub async fn fan_out<T: Send + 'static, F: Future<Output = T> + Send + 'static>(
    futs: Vec<F>,
) -> Vec<T> {
    let mut set = tokio::task::JoinSet::new();
    for (i, f) in futs.into_iter().enumerate() {
        set.spawn(async move { (i, f.await) });
    }
    let mut out: Vec<(usize, T)> = vec![];
    while let Some(r) = set.join_next().await {
        if let Ok(x) = r {
            out.push(x);
        }
    }
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, t)| t).collect()
}

/// The client's own gateway token from any accepted spelling: to be put in playlist and stream URLs where the server's was.
pub fn client_token(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    client_info(headers, uri.query()).token
}

/// A JSON response with a last scrub for server tokens and addresses.
pub fn json_out(st: &State, v: &Value) -> Response {
    let mut secrets = vec![];
    for u in st.upstreams().iter() {
        if let UpKind::Server { url, token, .. } = &u.kind {
            secrets.push(url.trim_end_matches('/').to_string());
            if let Some(t) = token {
                secrets.push(t.expose().to_string());
            }
        }
    }
    let text = scrub(v.to_string(), &secrets);
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        text,
    )
        .into_response()
}

fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

fn list_json(items: Vec<Value>, total: u64, start: usize) -> Value {
    json!({"Items": items, "TotalRecordCount": total, "StartIndex": start})
}

// ---------------------------------------------------------------- query translation

const ID_PARAMS: [&str; 12] = [
    "parentid",
    "ids",
    "seriesid",
    "seasonid",
    "albumids",
    "artistids",
    "albumartistids",
    "genreids",
    "studioids",
    "personids",
    "excludeitemids",
    "startitemid",
];
const DROPPED: [&str; 6] = [
    "apikey",
    "api_key",
    "userid",
    "startindex",
    "limit",
    "fields",
];

fn is_id(s: &str) -> bool {
    Gid::parse(s).is_some()
}

/// A gateway id as seen by one source: its id there, or `None` when the source has no copy.
pub fn up_id_for(ids: &IdMap, gid: &str, key: &str) -> Option<String> {
    ids.resolve_str(gid)?
        .1
        .into_iter()
        .find(|m| m.source == key)
        .map(|m| m.id)
}

/// The client's query for one server: gateway ids replaced by that server's ids, the client's token and user, and the window removed (the caller sets those). `None` when an id names something this server does not have (skip the server).
pub fn translate(ids: &IdMap, q: &Query, key: &str) -> Option<Vec<(String, String)>> {
    let mut out = vec![];
    for (k, v) in &q.0 {
        let lk = k.to_ascii_lowercase();
        if DROPPED.contains(&lk.as_str()) {
            continue;
        }
        if ID_PARAMS.contains(&lk.as_str()) {
            let mut mapped = vec![];
            let given: Vec<&str> = v.split(',').filter(|s| !s.is_empty()).collect();
            for g in &given {
                if is_id(g) {
                    if let Some(u) = up_id_for(ids, g, key) {
                        mapped.push(u);
                    }
                } else {
                    mapped.push((*g).to_string());
                }
            }
            if mapped.is_empty() && !given.is_empty() {
                return None;
            }
            if lk != "ids" && lk != "excludeitemids" && mapped.len() < given.len() {
                return None;
            }
            out.push((k.clone(), mapped.join(",")));
        } else {
            out.push((k.clone(), v.clone()));
        }
    }
    Some(out)
}

fn fields_of(q: &Query) -> String {
    match q.get("Fields") {
        Some(f) if !f.is_empty() => format!("{f},{FIELDS}"),
        _ => FIELDS.to_string(),
    }
}

// ---------------------------------------------------------------- entries and merging

pub struct Rw {
    pub server_id: String,
    pub user_id: String,
}

pub fn rw(st: &State) -> Rw {
    Rw {
        server_id: st.users.server_id(),
        user_id: st.users.user_id(),
    }
}

pub fn server_entry(st: &State, rw: &Rw, s: &Srv, mut dto: Value) -> Option<Entry> {
    let up_id = dto["Id"].as_str()?.to_string();
    rewrite_refs(
        &mut dto,
        &JsonCtx {
            ids: &st.ids,
            alias: &hub(st).alias,
            src_key: &s.key,
            server_id: &rw.server_id,
            user_id: &rw.user_id,
        },
    );
    Some(Entry {
        src: s.idx,
        label: s.label.clone(),
        src_key: s.key.clone(),
        local: false,
        up_id,
        authority: s.authority.clone(),
        dto,
    })
}

fn local_entry(l: &LocalSrc, dto: Value) -> Option<Entry> {
    let up_id = dto["Id"].as_str()?.to_string();
    Some(Entry {
        src: l.idx,
        label: l.label.clone(),
        src_key: local_dto::SRC.into(),
        local: true,
        up_id,
        authority: String::new(),
        dto,
    })
}

/// `(items, total)` of an answer that is either `{Items, TotalRecordCount}` or a bare array.
pub fn items_of(v: &Value) -> (Vec<Value>, u64) {
    match v {
        Value::Array(a) => (a.clone(), a.len() as u64),
        o => {
            let items = o["Items"].as_array().cloned().unwrap_or_default();
            let n = o["TotalRecordCount"].as_u64().unwrap_or(items.len() as u64);
            (items, n)
        }
    }
}

fn runtime_of(d: &Value) -> i64 {
    d["RunTimeTicks"].as_i64().unwrap_or(0)
}

/// Turn groups of copies into the DTOs clients get: one id per group (provider-id merge key, or the single origin id), media sources of every copy labelled by origin, user data merged with the overlay.
pub fn finish(st: &State, groups: Vec<Group>, token: Option<&str>) -> Vec<Value> {
    let h = hub(st);
    let ov = overlay(st);
    let mut out = vec![];
    for g in groups {
        let Some(pref) = g.copies.first() else {
            continue;
        };
        let multi = g.copies.len() > 1;
        let members: Vec<Member> = g
            .copies
            .iter()
            .map(|c| Member {
                source: c.src_key.clone(),
                id: c.up_id.clone(),
            })
            .collect();
        let gid = match &g.key {
            Some(k) => st.ids.register(&key_merge(k), &members),
            None if pref.local => st.ids.register(&key_local(&pref.up_id), &members),
            None => st.ids.register_single(&pref.src_key, &pref.up_id),
        };
        for c in &g.copies {
            let single = if c.local {
                Gid::of(&key_local(&c.up_id))
            } else {
                Gid::of(&key_single(&c.src_key, &c.up_id))
            };
            h.alias.set(single, gid);
        }
        let mut dto = pref.dto.clone();
        let own = dto.as_object_mut().and_then(|m| m.remove("MediaSources"));
        fn uc<'a>(st: &'a State, c: &'a Entry, gid: Gid, token: Option<&'a str>) -> UrlCtx<'a> {
            UrlCtx {
                ids: &st.ids,
                src_key: &c.src_key,
                item: gid,
                up_item: &c.up_id,
                authority: &c.authority,
                token,
            }
        }
        if !pref.local {
            rewrite_urls_in(&mut dto, &uc(st, pref, gid, token));
        }
        let mut sources: Vec<Value> = vec![];
        for c in &g.copies {
            let list = if std::ptr::eq(c, pref) {
                own.clone()
            } else {
                c.dto.get("MediaSources").cloned()
            };
            for mut s in list.and_then(|v| v.as_array().cloned()).unwrap_or_default() {
                rewrite_source(
                    &mut s,
                    &uc(st, c, gid, token),
                    multi.then_some(c.label.as_str()),
                );
                sources.push(s);
            }
        }
        let uds: Vec<CopyData> = g
            .copies
            .iter()
            .map(|c| CopyData {
                ud: c.dto.get("UserData"),
                runtime: runtime_of(&c.dto),
            })
            .collect();
        let shown = runtime_of(&dto);
        let ud = merge_user_data(&uds, shown, ov.get(gid).as_ref(), gid);
        let m = dto.as_object_mut().expect("item object");
        m.insert("Id".into(), gid.to_string().into());
        if own.is_some() || !sources.is_empty() {
            m.insert("MediaSourceCount".into(), sources.len().into());
            m.insert("MediaSources".into(), sources.into());
        }
        m.insert("UserData".into(), ud);
        if shown > 0 {
            h.runtimes.lock().unwrap().insert(gid, shown);
        }
        out.push(dto);
    }
    out
}

/// Every DTO is its own group (no merging): lists whose items are not titles (hints, seasons of one source).
pub fn singles(entries: Vec<Entry>) -> Vec<Group> {
    entries
        .into_iter()
        .map(|e| Group {
            copies: vec![e],
            key: None,
        })
        .collect()
}

// ---------------------------------------------------------------- collecting

pub enum Order {
    Sort(Sort),
    /// Alternate between the sources (Next Up: each server's own order, fairly mixed).
    Interleave,
}

fn arrange(mut entries: Vec<Entry>, order: &Order) -> Vec<Entry> {
    match order {
        Order::Sort(s) => entries.sort_by(|a, b| lists::compare(&a.dto, &b.dto, s)),
        Order::Interleave => {
            let mut rank: HashMap<usize, usize> = HashMap::new();
            let mut keyed: Vec<(usize, usize, Entry)> = entries
                .drain(..)
                .map(|e| {
                    let r = rank.entry(e.src).or_default();
                    *r += 1;
                    (*r, e.src, e)
                })
                .collect();
            keyed.sort_by_key(|(r, s, _)| (*r, *s));
            entries = keyed.into_iter().map(|(_, _, e)| e).collect();
        }
    }
    entries
}

pub struct Collected {
    pub entries: Vec<Entry>,
    pub sum: u64,
}

/// What to ask every source.
pub struct Spec<'a> {
    /// Upstream route, without query: `/Items`, `/Items/Latest`, `/UserItems/Resume`, ...
    pub path: &'a str,
    pub q: &'a Query,
    /// `Limit` for each source (`None`: none).
    pub fetch: Option<usize>,
    pub forced: &'a [(&'a str, &'a str)],
    /// How the local library takes part; `None`: not at all.
    pub local: Option<Filter>,
}

pub async fn collect(st: &State, sp: &Spec<'_>) -> Collected {
    let rw = Arc::new(rw(st));
    let fields = fields_of(sp.q);
    let mut futs = vec![];
    for s in servers(st) {
        let Some(mut pairs) = translate(&st.ids, sp.q, &s.key) else {
            continue;
        };
        pairs.push(("userId".into(), s.user_id.clone()));
        pairs.push(("fields".into(), fields.clone()));
        if let Some(n) = sp.fetch {
            pairs.push(("limit".into(), n.to_string()));
            pairs.push(("startIndex".into(), "0".into()));
        }
        pairs.extend(
            sp.forced
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string())),
        );
        let pq = format!("{}?{}", sp.path, encode_query(&pairs));
        let (st2, rw2) = (st.clone(), rw.clone());
        futs.push(async move {
            let v = get_json(&st2, &s, &pq, true).await?;
            let (items, total) = items_of(&v);
            let e: Vec<Entry> = items
                .into_iter()
                .filter_map(|d| server_entry(&st2, &rw2, &s, d))
                .collect();
            Some((e, total))
        });
    }
    let mut c = Collected {
        entries: vec![],
        sum: 0,
    };
    for (e, n) in fan_out(futs).await.into_iter().flatten() {
        c.entries.extend(e);
        c.sum += n;
    }
    if let Some(f) = &sp.local {
        for l in locals(st) {
            let (st2, f2, rw2) = (st.clone(), f.clone(), rw.clone());
            let l2 = l.clone();
            let got = tokio::task::spawn_blocking(move || {
                let mut f = f2;
                if let Some(Some(p)) = &f.parent
                    && !matches!(local_dto::parse_member(p), Some(t) if local_dto::src_of(&t) == l2.id)
                {
                    f.parent = Some(None);
                }
                let h = hub(&st2);
                local_dto::with_lib(&st2, l2.id, |lib| {
                    let cx = local_dto::Ctx { ids: &st2.ids, alias: &h.alias, server_id: &rw2.server_id, src: l2.id };
                    local_dto::query(lib, &f, &cx)
                })
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
            c.sum += got.len() as u64;
            c.entries
                .extend(got.into_iter().filter_map(|d| local_entry(&l, d)));
        }
    }
    c
}

/// The part of a list query the local library can honour. `None` when the query asks for something it cannot (favourites, people, genres by id...), so local items stay out.
pub fn local_filter(st: &State, q: &Query) -> Option<Filter> {
    for k in [
        "GenreIds",
        "StudioIds",
        "PersonIds",
        "ArtistIds",
        "AlbumIds",
        "AlbumArtistIds",
        "Person",
        "Studios",
    ] {
        if q.get(k).is_some_and(|v| !v.is_empty()) {
            return None;
        }
    }
    if q.flag("IsFavorite") == Some(true)
        || q.csv("Filters")
            .iter()
            .any(|f| f.eq_ignore_ascii_case("IsFavorite"))
    {
        return None;
    }
    let local_member = |g: &str| {
        st.ids.resolve_str(g).and_then(|(_, m)| {
            m.into_iter()
                .find(|m| m.source == local_dto::SRC)
                .map(|m| m.id)
        })
    };
    let parent = q
        .get("ParentId")
        .or(q.get("SeriesId"))
        .filter(|s| !s.is_empty())
        .map(local_member);
    let ids = q
        .get("Ids")
        .filter(|s| !s.is_empty())
        .map(|v| v.split(',').filter_map(local_member).collect::<Vec<_>>());
    let mut played = q.flag("IsPlayed");
    for f in q.csv("Filters") {
        match f.to_ascii_lowercase().as_str() {
            "isplayed" => played = Some(true),
            "isunplayed" => played = Some(false),
            _ => {}
        }
    }
    Some(Filter {
        types: q
            .csv("IncludeItemTypes")
            .iter()
            .map(|t| t.to_ascii_lowercase())
            .collect(),
        search: q
            .get("SearchTerm")
            .filter(|s| !s.is_empty())
            .map(String::from),
        genres: q.csv("Genres"),
        years: q
            .csv("Years")
            .iter()
            .filter_map(|y| y.parse().ok())
            .collect(),
        name_starts: q
            .get("NameStartsWith")
            .filter(|s| !s.is_empty())
            .map(String::from),
        played,
        parent,
        ids,
        recursive: q.flag("Recursive").unwrap_or(false),
    })
}

/// Entries to one merged page: arranged, grouped, windowed, finished.
pub fn page(st: &State, c: Collected, order: &Order, w: Window, token: Option<&str>) -> Value {
    let seen = c.entries.len();
    let groups = lists::group(arrange(c.entries, order));
    let n = groups.len();
    let total = lists::merged_total(c.sum, seen, n);
    let groups: Vec<Group> = groups.into_iter().skip(w.start).take(w.size()).collect();
    list_json(
        finish(st, groups, token),
        total.max((w.start + n.min(w.size())) as u64 * u64::from(n > 0)),
        w.start,
    )
}

// ---------------------------------------------------------------- views

const VIEW_TYPES: [(&str, &str); 8] = [
    ("movies", "Movies"),
    ("tvshows", "Shows"),
    ("music", "Music"),
    ("boxsets", "Collections"),
    ("books", "Books"),
    ("photos", "Photos"),
    ("homevideos", "Home Videos"),
    ("musicvideos", "Music Videos"),
];

/// The merged libraries: one view per collection type over every server and local source, plus one per library when `per_server_views` is on.
pub async fn views(st: &State) -> Vec<Value> {
    let h = hub(st);
    let rw = rw(st);
    let mut futs = vec![];
    for s in servers(st) {
        let (st2, pq) = (st.clone(), format!("/UserViews?userId={}", enc(&s.user_id)));
        futs.push(async move { (s.clone(), get_json(&st2, &s, &pq, true).await) });
    }
    // (type, members, first library DTO's image tags)
    let mut by_type: HashMap<String, (Vec<Member>, Value)> = HashMap::new();
    let mut per_server = vec![];
    for (s, v) in fan_out(futs).await {
        for lib in v.map(|v| items_of(&v).0).unwrap_or_default() {
            let (Some(id), Some(ct)) = (lib["Id"].as_str(), lib["CollectionType"].as_str()) else {
                continue;
            };
            let ct = ct.to_ascii_lowercase();
            let e = by_type
                .entry(ct.clone())
                .or_insert_with(|| (vec![], lib["ImageTags"].clone()));
            e.0.push(Member {
                source: s.key.clone(),
                id: id.into(),
            });
            per_server.push((s.clone(), lib.clone(), ct));
        }
    }
    for l in locals(st) {
        let (st2, l2) = (st.clone(), l.clone());
        let kinds = tokio::task::spawn_blocking(move || {
            local_dto::with_lib(&st2, l2.id, |lib| local_dto::view_kinds(lib))
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        for k in kinds {
            if let Some((short, ct)) = local_dto::view_kind_name(k) {
                let e = by_type
                    .entry(ct.into())
                    .or_insert_with(|| (vec![], json!({})));
                e.0.push(Member {
                    source: local_dto::SRC.into(),
                    id: format!("view:{}:{short}", l.id.0),
                });
            }
        }
    }
    let view = |gid: Gid, name: &str, ct: &str, tags: &Value| {
        json!({"Name": name, "SortName": name.to_lowercase(), "ServerId": rw.server_id, "Id": gid.to_string(), "Type": "CollectionFolder",
               "CollectionType": ct, "IsFolder": true, "LocationType": "FileSystem", "ImageTags": tags, "BackdropImageTags": [],
               "UserData": {"PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false, "Played": false, "Key": gid.to_string()}})
    };
    let mut out = vec![];
    let mut gids = HashSet::new();
    for (ct, name) in VIEW_TYPES {
        let Some((members, tags)) = by_type.get(ct) else {
            continue;
        };
        let gid = st.ids.register(&key_merge(&format!("view:{ct}")), members);
        gids.insert(gid);
        out.push(view(gid, name, ct, tags));
    }
    if st.cfg().gw.per_server_views {
        for (s, lib, ct) in per_server {
            let (Some(id), Some(name)) = (lib["Id"].as_str(), lib["Name"].as_str()) else {
                continue;
            };
            let gid = st.ids.register_single(&s.key, id);
            gids.insert(gid);
            out.push(view(
                gid,
                &format!("{} · {name}", s.label),
                &ct,
                &lib["ImageTags"],
            ));
        }
    }
    *h.view_gids.lock().unwrap() = gids;
    out
}

// ---------------------------------------------------------------- details

/// One item with every copy's media sources, or `None` when no source has it.
pub async fn detail(st: &State, gid: Gid, token: Option<&str>) -> Option<Value> {
    let members = st.ids.resolve(gid)?;
    if hub(st).view_gids.lock().unwrap().contains(&gid)
        || members.iter().any(|m| m.id.starts_with("view:"))
    {
        let g = gid.to_string();
        if let Some(v) = views(st).await.into_iter().find(|v| v["Id"] == g) {
            return Some(v);
        }
    }
    let rw = Arc::new(rw(st));
    let all = servers(st);
    let mut futs = vec![];
    for m in members.iter().filter(|m| m.source != local_dto::SRC) {
        let Some(s) = all.iter().find(|s| s.key == m.source).cloned() else {
            continue;
        };
        let pq = format!("/Items/{}?userId={}", enc(&m.id), enc(&s.user_id));
        let (st2, rw2) = (st.clone(), rw.clone());
        futs.push(async move {
            let v = get_json(&st2, &s, &pq, true).await?;
            server_entry(&st2, &rw2, &s, (*v).clone())
        });
    }
    let mut entries: Vec<Entry> = fan_out(futs).await.into_iter().flatten().collect();
    for m in members.iter().filter(|m| m.source == local_dto::SRC) {
        let Some(l) = local_dto::parse_member(&m.id)
            .map(|t| local_dto::src_of(&t))
            .and_then(|s| locals(st).into_iter().find(|l| l.id == s))
        else {
            continue;
        };
        let (st2, rw2, id) = (st.clone(), rw.clone(), m.id.clone());
        let d = tokio::task::spawn_blocking(move || {
            let h = hub(&st2);
            local_dto::with_lib(&st2, l.id, |lib| {
                let cx = local_dto::Ctx {
                    ids: &st2.ids,
                    alias: &h.alias,
                    server_id: &rw2.server_id,
                    src: l.id,
                };
                local_dto::member_dto(lib, &id, &cx, true).map(|d| (l, d))
            })
        })
        .await
        .ok()
        .flatten()
        .flatten();
        entries.extend(d.and_then(|(l, d)| local_entry(&l, d)));
    }
    if entries.is_empty() {
        return None;
    }
    entries.sort_by_key(|e| e.src);
    let keys: Vec<String> = entries
        .iter()
        .flat_map(|e| lists::identity_keys(&e.dto))
        .collect();
    let g = Group {
        key: lists::canonical_key(&keys),
        copies: entries,
    };
    finish(st, vec![g], token).into_iter().next()
}

// ---------------------------------------------------------------- handlers

fn q_of(uri: &Uri) -> Query {
    Query::parse(uri.query())
}

/// `GET /Items` and `/Users/{id}/Items`.
async fn items_list(St(st): St<State>, headers: HeaderMap, uri: Uri) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let w = Window::of(&q);
    let sort = Sort::of(&q);
    let sp = Spec {
        path: "/Items",
        q: &q,
        fetch: (w.limit != Some(0)).then(|| w.fetch()),
        forced: &[],
        local: local_filter(&st, &q),
    };
    let mut c = collect(&st, &sp).await;
    if sort.is_random() {
        // Random is per request: the sort key is derived from the id, so the order is stable inside one page but not shown twice the same way.
        c.entries.reverse();
    }
    json_out(&st, &page(&st, c, &Order::Sort(sort), w, tok.as_deref()))
}

async fn views_handler(St(st): St<State>) -> Response {
    let v = views(&st).await;
    let n = v.len() as u64;
    json_out(&st, &list_json(v, n, 0))
}

fn item_arg(p: &HashMap<String, String>) -> &str {
    p.get("itemId").map_or("", String::as_str)
}

async fn item_detail(
    St(st): St<State>,
    Path(p): Path<HashMap<String, String>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let Some((gid, _)) = st.ids.resolve_str(item_arg(&p)) else {
        return not_found();
    };
    match detail(&st, gid, client_token(&headers, &uri).as_deref()).await {
        Some(d) => json_out(&st, &d),
        None => not_found(),
    }
}

/// `GET /Items/Latest`: a bare array, newest first.
async fn latest(St(st): St<State>, headers: HeaderMap, uri: Uri) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let n = q.usize("Limit").unwrap_or(20).min(lists::MAX_PAGE);
    let mut lq = q.clone();
    lq.0.retain(|(k, _)| !k.eq_ignore_ascii_case("SortBy") && !k.eq_ignore_ascii_case("SortOrder"));
    let local = local_filter(&st, &q).map(|mut f| {
        f.recursive = true;
        if f.types.is_empty() {
            f.types = vec!["movie".into(), "episode".into(), "audio".into()];
        }
        f
    });
    let c = collect(
        &st,
        &Spec {
            path: "/Items/Latest",
            q: &lq,
            fetch: Some(n),
            forced: &[],
            local,
        },
    )
    .await;
    let sort = Sort::of(&Query::parse(Some(
        "SortBy=DateCreated&SortOrder=Descending",
    )));
    let w = Window {
        start: 0,
        limit: Some(n),
    };
    let v = page(&st, c, &Order::Sort(sort), w, tok.as_deref());
    json_out(&st, &v["Items"])
}

/// Continue Watching: servers' resume lists, the local library's, and what gateway clients reported with server writes off.
async fn resume(St(st): St<State>, headers: HeaderMap, uri: Uri) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let n = q.usize("Limit").unwrap_or(12).min(lists::MAX_PAGE);
    let mut c = collect(
        &st,
        &Spec {
            path: "/UserItems/Resume",
            q: &q,
            fetch: Some(n),
            forced: &[("fields", &fields_of(&q))],
            local: None,
        },
    )
    .await;
    let rw = Arc::new(rw(&st));
    for l in locals(&st) {
        let (st2, rw2) = (st.clone(), rw.clone());
        let l2 = l.clone();
        let got = tokio::task::spawn_blocking(move || {
            let h = hub(&st2);
            local_dto::with_lib(&st2, l2.id, |lib| {
                let cx = local_dto::Ctx {
                    ids: &st2.ids,
                    alias: &h.alias,
                    server_id: &rw2.server_id,
                    src: l2.id,
                };
                local_dto::resume(lib, n, &cx)
            })
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        c.entries
            .extend(got.into_iter().filter_map(|d| local_entry(&l, d)));
    }
    // Items only the overlay knows are in progress (a server does not list them because the position was never written there).
    let sort = Sort::of(&Query::parse(Some(
        "SortBy=DatePlayed&SortOrder=Descending",
    )));
    let w = Window {
        start: 0,
        limit: Some(n),
    };
    let mut v = page(&st, c, &Order::Sort(sort.clone()), w, tok.as_deref());
    let mut items: Vec<Value> = v["Items"].as_array().cloned().unwrap_or_default();
    for (gid, _) in overlay(&st).resumable(n) {
        if items.iter().any(|i| i["Id"] == gid.to_string()) {
            continue;
        }
        if let Some(d) = detail(&st, gid, tok.as_deref()).await {
            items.push(d);
        }
    }
    // Played, or without a position once the overlay is applied: not resumable.
    items.retain(|i| {
        i["UserData"]["Played"] != true
            && i["UserData"]["PlaybackPositionTicks"].as_i64().unwrap_or(0) > 0
    });
    items.sort_by(|a, b| lists::compare(a, b, &sort));
    items.truncate(n);
    v["TotalRecordCount"] = items.len().into();
    v["Items"] = items.into();
    json_out(&st, &v)
}

async fn next_up(St(st): St<State>, headers: HeaderMap, uri: Uri) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let w = Window::of(&q);
    let c = collect(
        &st,
        &Spec {
            path: "/Shows/NextUp",
            q: &q,
            fetch: Some(w.fetch()),
            forced: &[],
            local: None,
        },
    )
    .await;
    json_out(&st, &page(&st, c, &Order::Interleave, w, tok.as_deref()))
}

async fn suggestions(St(st): St<State>, headers: HeaderMap, uri: Uri) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let w = Window::of(&q);
    let c = collect(
        &st,
        &Spec {
            path: "/Items/Suggestions",
            q: &q,
            fetch: Some(w.fetch()),
            forced: &[],
            local: None,
        },
    )
    .await;
    json_out(&st, &page(&st, c, &Order::Interleave, w, tok.as_deref()))
}

/// `GET /Movies/Recommendations`: every server's categories, items rewritten (not merged).
async fn recommendations(St(st): St<State>, headers: HeaderMap, uri: Uri) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let rwv = Arc::new(rw(&st));
    let mut futs = vec![];
    for s in servers(&st) {
        let Some(mut pairs) = translate(&st.ids, &q, &s.key) else {
            continue;
        };
        pairs.push(("userId".into(), s.user_id.clone()));
        let (st2, pq) = (
            st.clone(),
            format!("/Movies/Recommendations?{}", encode_query(&pairs)),
        );
        futs.push(async move { (s.clone(), get_json(&st2, &s, &pq, true).await) });
    }
    let mut out = vec![];
    for (s, v) in fan_out(futs).await {
        for mut cat in v
            .map(|v| v.as_array().cloned().unwrap_or_default())
            .unwrap_or_default()
        {
            let dtos = cat["Items"].as_array().cloned().unwrap_or_default();
            let es: Vec<Entry> = dtos
                .into_iter()
                .filter_map(|d| server_entry(&st, &rwv, &s, d))
                .collect();
            cat["Items"] = finish(&st, singles(es), tok.as_deref()).into();
            if let Some(c) = cat["CategoryId"].as_str().filter(|c| is_id(c)) {
                cat["CategoryId"] = st.ids.register_single(&s.key, c).to_string().into();
            }
            out.push(cat);
        }
    }
    json_out(&st, &Value::Array(out))
}

/// `GET /Shows/{id}/Seasons` and `/Episodes`: every member series answers; seasons are matched by number and episodes by (season, episode).
async fn seasons_episodes(
    st: State,
    series: &str,
    headers: HeaderMap,
    uri: Uri,
    episodes: bool,
) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let Some((gid, members)) = st.ids.resolve_str(series) else {
        return not_found();
    };
    let w = Window::of(&q);
    let rwv = Arc::new(rw(&st));
    let h = hub(&st);
    for m in &members {
        h.alias.set(Gid::of(&key_single(&m.source, &m.id)), gid);
    }
    let what = if episodes { "Episodes" } else { "Seasons" };
    let mut futs = vec![];
    for s in servers(&st) {
        let Some(m) = members.iter().find(|m| m.source == s.key) else {
            continue;
        };
        let Some(mut pairs) = translate(&st.ids, &q, &s.key) else {
            continue;
        };
        pairs.push(("userId".into(), s.user_id.clone()));
        pairs.push((
            "fields".into(),
            if episodes {
                format!("{},Overview,PrimaryImageAspectRatio", fields_of(&q))
            } else {
                format!("{},ChildCount", fields_of(&q))
            },
        ));
        let (st2, rw2, pq) = (
            st.clone(),
            rwv.clone(),
            format!("/Shows/{}/{what}?{}", enc(&m.id), encode_query(&pairs)),
        );
        futs.push(async move {
            let v = get_json(&st2, &s, &pq, true).await?;
            Some(
                items_of(&v)
                    .0
                    .into_iter()
                    .filter_map(|d| server_entry(&st2, &rw2, &s, d))
                    .collect::<Vec<_>>(),
            )
        });
    }
    let mut entries: Vec<Entry> = fan_out(futs)
        .await
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    for l in locals(&st) {
        let Some(sm) = members.iter().find(|m| {
            m.source == local_dto::SRC
                && local_dto::parse_member(&m.id).is_some_and(|t| local_dto::src_of(&t) == l.id)
        }) else {
            continue;
        };
        let parent = q
            .get("SeasonId")
            .and_then(|g| up_id_for(&st.ids, g, local_dto::SRC))
            .unwrap_or_else(|| sm.id.clone());
        let f = Filter {
            types: vec![if episodes { "episode" } else { "season" }.into()],
            parent: Some(Some(parent)),
            recursive: true,
            ..Default::default()
        };
        let (st2, rw2, l2) = (st.clone(), rwv.clone(), l.clone());
        let got = tokio::task::spawn_blocking(move || {
            let h = hub(&st2);
            local_dto::with_lib(&st2, l2.id, |lib| {
                let cx = local_dto::Ctx {
                    ids: &st2.ids,
                    alias: &h.alias,
                    server_id: &rw2.server_id,
                    src: l2.id,
                };
                local_dto::query(lib, &f, &cx)
            })
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        entries.extend(got.into_iter().filter_map(|d| local_entry(&l, d)));
    }
    let n = |d: &Value, f: &str| d[f].as_i64().unwrap_or(-1);
    let mut buckets: Vec<((i64, i64), Vec<Entry>)> = vec![];
    for e in entries {
        let k = if episodes {
            (n(&e.dto, "ParentIndexNumber"), n(&e.dto, "IndexNumber"))
        } else {
            (n(&e.dto, "IndexNumber"), 0)
        };
        match buckets.iter_mut().find(|(bk, _)| *bk == k) {
            Some((_, v)) if k.0 >= 0 && (!episodes || k.1 >= 0) => v.push(e),
            _ => buckets.push((k, vec![e])),
        }
    }
    buckets.sort_by_key(|(k, _)| *k);
    let total = buckets.len() as u64;
    let groups: Vec<Group> = buckets
        .into_iter()
        .skip(w.start)
        .take(w.size())
        .map(|((a, b), mut copies)| {
            copies.sort_by_key(|c| c.src);
            Group {
                key: (a >= 0)
                    .then(|| format!("{gid}:{}:{a}:{b}", if episodes { "e" } else { "s" })),
                copies,
            }
        })
        .collect();
    json_out(
        &st,
        &list_json(finish(&st, groups, tok.as_deref()), total, w.start),
    )
}

/// Item-scoped reads answered by the item's own (first) server with ids rewritten: similar items, ancestors, extras, trailers, intros, theme media.
async fn item_proxy(
    st: State,
    p: HashMap<String, String>,
    headers: HeaderMap,
    uri: Uri,
    tail: &'static str,
    legacy_user: bool,
) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let Some((_, members)) = st.ids.resolve_str(item_arg(&p)) else {
        return not_found();
    };
    let Some((s, up)) = servers(&st).into_iter().find_map(|s| {
        members
            .iter()
            .find(|m| m.source == s.key)
            .map(|m| (s.clone(), m.id.clone()))
    }) else {
        return json_out(
            &st,
            &json!({"Items": [], "TotalRecordCount": 0, "StartIndex": 0}),
        );
    };
    let Some(mut pairs) = translate(&st.ids, &q, &s.key) else {
        return not_found();
    };
    let path = if legacy_user {
        format!("/Users/{}/Items/{}/{tail}", enc(&s.user_id), enc(&up))
    } else {
        pairs.push(("userId".into(), s.user_id.clone()));
        format!("/Items/{}/{tail}", enc(&up))
    };
    let pq = format!("{path}?{}", encode_query(&pairs));
    let Some(v) = get_json(&st, &s, &pq, true).await else {
        return not_found();
    };
    let rwv = rw(&st);
    let mut conv = |d: &Value| {
        let e = server_entry(&st, &rwv, &s, d.clone())?;
        finish(&st, singles(vec![e]), tok.as_deref())
            .into_iter()
            .next()
    };
    let out = match &*v {
        Value::Array(a) => Value::Array(a.iter().filter_map(&mut conv).collect()),
        o if o["Items"].is_array() => {
            let (items, n) = items_of(o);
            list_json(items.iter().filter_map(&mut conv).collect(), n, 0)
        }
        o => conv(o).unwrap_or(Value::Null),
    };
    json_out(&st, &out)
}

macro_rules! proxy {
    ($tail:literal, $legacy:literal) => {
        |St(st): St<State>, Path(p): Path<HashMap<String, String>>, h: HeaderMap, u: Uri| async move { item_proxy(st, p, h, u, $tail, $legacy).await }
    };
}

/// `GET /Genres`, `/Studios`, `/Persons`, `/Artists`...: names merged across sources (case-insensitive), with a gateway id that maps back to each source's own.
async fn names(st: State, path: &'static str, headers: HeaderMap, uri: Uri) -> Response {
    let (q, tok) = (q_of(&uri), client_token(&headers, &uri));
    let w = Window::of(&q);
    let c = collect(
        &st,
        &Spec {
            path,
            q: &q,
            fetch: Some(w.fetch()),
            forced: &[],
            local: None,
        },
    )
    .await;
    let mut buckets: Vec<(String, Vec<Entry>)> = vec![];
    for e in c.entries {
        let k = format!(
            "name:{path}:{}",
            e.dto["Name"].as_str().unwrap_or("").to_lowercase()
        );
        match buckets.iter_mut().find(|(bk, _)| *bk == k) {
            Some((_, v)) => v.push(e),
            None => buckets.push((k, vec![e])),
        }
    }
    let sort = Sort::of(&q);
    let mut groups: Vec<Group> = buckets
        .into_iter()
        .map(|(k, mut copies)| {
            copies.sort_by_key(|c| c.src);
            Group {
                key: Some(k),
                copies,
            }
        })
        .collect();
    groups.sort_by(|a, b| lists::compare(&a.copies[0].dto, &b.copies[0].dto, &sort));
    let total = groups.len() as u64;
    let groups: Vec<Group> = groups.into_iter().skip(w.start).take(w.size()).collect();
    json_out(
        &st,
        &list_json(finish(&st, groups, tok.as_deref()), total, w.start),
    )
}

/// `GET /Items/Filters`: the union of every server's filter values.
async fn filters(St(st): St<State>, uri: Uri) -> Response {
    let q = q_of(&uri);
    let mut futs = vec![];
    for s in servers(&st) {
        let Some(mut pairs) = translate(&st.ids, &q, &s.key) else {
            continue;
        };
        pairs.push(("userId".into(), s.user_id.clone()));
        let (st2, pq) = (
            st.clone(),
            format!("/Items/Filters?{}", encode_query(&pairs)),
        );
        futs.push(async move { get_json(&st2, &s, &pq, true).await });
    }
    let mut out = serde_json::Map::new();
    for v in fan_out(futs).await.into_iter().flatten() {
        for (k, val) in v.as_object().into_iter().flatten() {
            if let Some(a) = val.as_array() {
                out.entry(k.clone())
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .expect("array")
                    .extend(a.iter().cloned());
            }
        }
    }
    for v in out.values_mut() {
        let a = v.as_array_mut().expect("array");
        a.sort_by_key(|x| x.to_string().to_lowercase());
        a.dedup_by_key(|x| x.to_string().to_lowercase());
    }
    json_out(&st, &Value::Object(out))
}

/// `GET /Items/Counts`: sums.
async fn counts(St(st): St<State>, uri: Uri) -> Response {
    let q = q_of(&uri);
    let mut sum: HashMap<String, i64> = HashMap::new();
    for s in servers(&st) {
        let Some(mut pairs) = translate(&st.ids, &q, &s.key) else {
            continue;
        };
        pairs.push(("userId".into(), s.user_id.clone()));
        if let Some(v) = get_json(
            &st,
            &s,
            &format!("/Items/Counts?{}", encode_query(&pairs)),
            true,
        )
        .await
        {
            for (k, n) in v.as_object().into_iter().flatten() {
                *sum.entry(k.clone()).or_default() += n.as_i64().unwrap_or(0);
            }
        }
    }
    json_out(&st, &json!(sum))
}

/// `GET /Search/Hints`.
async fn search_hints(St(st): St<State>, uri: Uri) -> Response {
    let q = q_of(&uri);
    let limit = q.usize("Limit").unwrap_or(20).min(lists::MAX_PAGE);
    let term = q.get("SearchTerm").unwrap_or("").to_string();
    let rwv = rw(&st);
    let mut futs = vec![];
    for s in servers(&st) {
        let Some(mut pairs) = translate(&st.ids, &q, &s.key) else {
            continue;
        };
        pairs.push(("userId".into(), s.user_id.clone()));
        pairs.push(("limit".into(), limit.to_string()));
        let (st2, pq) = (
            st.clone(),
            format!("/Search/Hints?{}", encode_query(&pairs)),
        );
        futs.push(async move { (s.clone(), get_json(&st2, &s, &pq, true).await) });
    }
    let h = hub(&st);
    let mut hints: Vec<Value> = vec![];
    for (s, v) in fan_out(futs).await {
        for mut d in v
            .map(|v| v["SearchHints"].as_array().cloned().unwrap_or_default())
            .unwrap_or_default()
        {
            rewrite_refs(
                &mut d,
                &JsonCtx {
                    ids: &st.ids,
                    alias: &h.alias,
                    src_key: &s.key,
                    server_id: &rwv.server_id,
                    user_id: &rwv.user_id,
                },
            );
            for k in ["Id", "ItemId"] {
                if let Some(x) = d[k].as_str().filter(|x| is_id(x)) {
                    d[k] = h
                        .alias
                        .get(st.ids.register_single(&s.key, x))
                        .to_string()
                        .into();
                }
            }
            hints.push(d);
        }
    }
    if !term.is_empty() && q.get("ParentId").is_none_or(str::is_empty) {
        for l in locals(&st) {
            let (st2, t, rw2, l2) = (st.clone(), term.clone(), rw(&st), l.clone());
            let got = tokio::task::spawn_blocking(move || {
                let h = hub(&st2);
                local_dto::with_lib(&st2, l2.id, |lib| {
                    let cx = local_dto::Ctx {
                        ids: &st2.ids,
                        alias: &h.alias,
                        server_id: &rw2.server_id,
                        src: l2.id,
                    };
                    local_dto::search_hints(lib, &t, &cx)
                })
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
            for d in got {
                let Some(e) = local_entry(&l, d) else {
                    continue;
                };
                if let Some(v) = finish(&st, singles(vec![e]), None).into_iter().next() {
                    hints.push(json!({"ItemId": v["Id"], "Id": v["Id"], "Name": v["Name"], "Type": v["Type"], "MediaType": v["MediaType"],
                        "ProductionYear": v["ProductionYear"], "RunTimeTicks": v["RunTimeTicks"], "PrimaryImageTag": v["ImageTags"]["Primary"]}));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    hints.retain(|d| {
        seen.insert(format!(
            "{}|{}|{}",
            d["Type"],
            d["Name"].as_str().unwrap_or("").to_lowercase(),
            d["ProductionYear"]
        ))
    });
    hints.truncate(limit);
    let n = hints.len();
    json_out(&st, &json!({"SearchHints": hints, "TotalRecordCount": n}))
}

pub fn routes(r: Routes) -> Routes {
    let list = || get(items_list);
    r.add("/UserViews", Access::Auth, get(views_handler))
        .add("/Users/{userId}/Views", Access::Auth, get(views_handler))
        .add("/Items", Access::Auth, list())
        .add("/Users/{userId}/Items", Access::Auth, list())
        .add("/Items/Latest", Access::Auth, get(latest))
        .add("/Users/{userId}/Items/Latest", Access::Auth, get(latest))
        .add("/UserItems/Resume", Access::Auth, get(resume))
        .add("/Items/Resume", Access::Auth, get(resume))
        .add("/Users/{userId}/Items/Resume", Access::Auth, get(resume))
        .add("/Shows/NextUp", Access::Auth, get(next_up))
        .add("/Items/Suggestions", Access::Auth, get(suggestions))
        .add(
            "/Users/{userId}/Suggestions",
            Access::Auth,
            get(suggestions),
        )
        .add(
            "/Movies/Recommendations",
            Access::Auth,
            get(recommendations),
        )
        .add("/Items/Filters", Access::Auth, get(filters))
        .add("/Items/Counts", Access::Auth, get(counts))
        .add("/Search/Hints", Access::Auth, get(search_hints))
        .add("/Items/{itemId}", Access::Auth, get(item_detail))
        .add(
            "/Users/{userId}/Items/{itemId}",
            Access::Auth,
            get(item_detail),
        )
        .add(
            "/Shows/{itemId}/Seasons",
            Access::Auth,
            get(
                |St(st): St<State>,
                 Path(p): Path<HashMap<String, String>>,
                 h: HeaderMap,
                 u: Uri| async move {
                    seasons_episodes(st.clone(), item_arg(&p), h, u, false).await
                },
            ),
        )
        .add(
            "/Shows/{itemId}/Episodes",
            Access::Auth,
            get(
                |St(st): St<State>,
                 Path(p): Path<HashMap<String, String>>,
                 h: HeaderMap,
                 u: Uri| async move {
                    seasons_episodes(st.clone(), item_arg(&p), h, u, true).await
                },
            ),
        )
        .add(
            "/Items/{itemId}/Similar",
            Access::Auth,
            get(proxy!("Similar", false)),
        )
        .add(
            "/Shows/{itemId}/Similar",
            Access::Auth,
            get(proxy!("Similar", false)),
        )
        .add(
            "/Movies/{itemId}/Similar",
            Access::Auth,
            get(proxy!("Similar", false)),
        )
        .add(
            "/Albums/{itemId}/Similar",
            Access::Auth,
            get(proxy!("Similar", false)),
        )
        .add(
            "/Items/{itemId}/Ancestors",
            Access::Auth,
            get(proxy!("Ancestors", false)),
        )
        .add(
            "/Items/{itemId}/ThemeMedia",
            Access::Auth,
            get(proxy!("ThemeMedia", false)),
        )
        .add(
            "/Users/{userId}/Items/{itemId}/SpecialFeatures",
            Access::Auth,
            get(proxy!("SpecialFeatures", true)),
        )
        .add(
            "/Items/{itemId}/SpecialFeatures",
            Access::Auth,
            get(proxy!("SpecialFeatures", true)),
        )
        .add(
            "/Users/{userId}/Items/{itemId}/LocalTrailers",
            Access::Auth,
            get(proxy!("LocalTrailers", true)),
        )
        .add(
            "/Items/{itemId}/LocalTrailers",
            Access::Auth,
            get(proxy!("LocalTrailers", true)),
        )
        .add(
            "/Users/{userId}/Items/{itemId}/Intros",
            Access::Auth,
            get(proxy!("Intros", true)),
        )
        .add(
            "/Items/{itemId}/Intros",
            Access::Auth,
            get(proxy!("Intros", true)),
        )
        .add(
            "/Genres",
            Access::Auth,
            get(|St(st): St<State>, h: HeaderMap, u: Uri| async move {
                names(st, "/Genres", h, u).await
            }),
        )
        .add(
            "/MusicGenres",
            Access::Auth,
            get(|St(st): St<State>, h: HeaderMap, u: Uri| async move {
                names(st, "/MusicGenres", h, u).await
            }),
        )
        .add(
            "/Studios",
            Access::Auth,
            get(|St(st): St<State>, h: HeaderMap, u: Uri| async move {
                names(st, "/Studios", h, u).await
            }),
        )
        .add(
            "/Persons",
            Access::Auth,
            get(|St(st): St<State>, h: HeaderMap, u: Uri| async move {
                names(st, "/Persons", h, u).await
            }),
        )
        .add(
            "/Artists",
            Access::Auth,
            get(|St(st): St<State>, h: HeaderMap, u: Uri| async move {
                names(st, "/Artists", h, u).await
            }),
        )
        .add(
            "/Artists/AlbumArtists",
            Access::Auth,
            get(|St(st): St<State>, h: HeaderMap, u: Uri| async move {
                names(st, "/Artists/AlbumArtists", h, u).await
            }),
        )
}
