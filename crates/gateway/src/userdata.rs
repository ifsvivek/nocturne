//! Watched, resume and favourite state (plan section 2, decision 8).
//! The gateway keeps an overlay file (`userdata.json`, "stored in NOCTURNE, not on the server") that always records what clients report, so the gateway works with server writes off.
//! Reads merge every copy's own `UserData` with the overlay; writes are forwarded only as WRITES.md allows: `allow_server_writes`, to the item's own server (every copy with `sync_watched`), never to a server the gateway has no login for. Local items write their library's `state.json`.
use crate::auth::{iso, unix_now};
use crate::idmap::{Gid, Member};
use crate::items::{self, Srv};
use crate::local_dto;
use crate::server::{Access, Routes};
use crate::state::State;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State as St};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use nocturne_core::source::SourceId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

const TICKS: f64 = 10_000_000.0;

/// What a gateway client said about one item. `ts` is when (Unix seconds).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ov {
    pub played: Option<bool>,
    pub favorite: Option<bool>,
    pub ticks: Option<i64>,
    pub count: Option<u32>,
    pub last: Option<String>,
    pub ts: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    version: u32,
    items: HashMap<String, Ov>,
}

/// The overlay, loaded from `userdata.json` on first use. One shared instance per process (`state.ext::<Overlay>()`).
#[derive(Default)]
pub struct Overlay {
    map: Mutex<HashMap<String, Ov>>,
    loaded: std::sync::OnceLock<()>,
}

fn path(st: &State) -> std::path::PathBuf {
    st.paths.data.join("userdata.json")
}

pub fn overlay(st: &State) -> std::sync::Arc<Overlay> {
    let o = st.ext::<Overlay>();
    o.loaded.get_or_init(|| {
        if let Some(f) = std::fs::read(path(st))
            .ok()
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
        {
            *o.map.lock().unwrap() = f.items;
        }
    });
    o
}

impl Overlay {
    pub fn get(&self, gid: Gid) -> Option<Ov> {
        self.map.lock().unwrap().get(&gid.to_string()).cloned()
    }

    /// Change one item's entry and write the file (0600, atomic). Entries unread for a long time are not pruned: one small record per watched item.
    pub fn update(&self, st: &State, gid: Gid, f: impl FnOnce(&mut Ov)) -> Ov {
        let (ov, bytes) = {
            let mut m = self.map.lock().unwrap();
            let e = m.entry(gid.to_string()).or_default();
            f(e);
            e.ts = unix_now();
            let ov = e.clone();
            let bytes = serde_json::to_vec(&File {
                version: 1,
                items: m.clone(),
            })
            .unwrap_or_default();
            (ov, bytes)
        };
        let p = path(st);
        if let Err(e) = crate::config::write_private(&p, &bytes) {
            crate::glog!("cannot write the user data overlay: {e}");
        }
        ov
    }

    /// Items with a stop position and not watched, most recent first (the Continue Watching of a gateway client when no server knows the position).
    pub fn resumable(&self, n: usize) -> Vec<(Gid, Ov)> {
        let m = self.map.lock().unwrap();
        let mut v: Vec<(Gid, Ov)> = m
            .iter()
            .filter(|(_, o)| o.ticks.unwrap_or(0) > 0 && o.played != Some(true))
            .filter_map(|(k, o)| Some((Gid::parse(k)?, o.clone())))
            .collect();
        v.sort_by_key(|(_, o)| std::cmp::Reverse(o.ts));
        v.truncate(n);
        v
    }
}

/// One copy's `UserData` and runtime, input of [`merge_user_data`].
pub struct CopyData<'a> {
    pub ud: Option<&'a Value>,
    pub runtime: i64,
}

fn day(s: &str) -> &str {
    s.get(..19).unwrap_or(s)
}

/// The `UserData` a gateway client sees for a merged item.
/// Played: any copy played; resume: the copy played most recently, scaled by runtime to the shown copy (`watched::scaled`); favourite: any; count: the most. The overlay wins where it is newer than every copy's last play (favourite: always, a mark has no date on the server).
/// `shown_runtime` is the runtime of the copy the client sees.
pub fn merge_user_data(
    copies: &[CopyData],
    shown_runtime: i64,
    ov: Option<&Ov>,
    item: Gid,
) -> Value {
    let mut out = copies
        .first()
        .and_then(|c| c.ud.cloned())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let (mut played, mut fav, mut count) = (false, false, 0u64);
    let mut newest: Option<(&str, i64, i64)> = None; // last played, position, that copy's runtime
    for c in copies {
        let Some(u) = c.ud else { continue };
        played |= u["Played"].as_bool().unwrap_or(false);
        fav |= u["IsFavorite"].as_bool().unwrap_or(false);
        count = count.max(u["PlayCount"].as_u64().unwrap_or(0));
        let last = u["LastPlayedDate"].as_str().unwrap_or("");
        let pos = u["PlaybackPositionTicks"].as_i64().unwrap_or(0);
        if (pos > 0 || !last.is_empty()) && newest.is_none_or(|n| day(last) > day(n.0)) {
            newest = Some((last, pos, c.runtime));
        }
    }
    let mut ticks = newest.map_or(0, |(_, pos, rt)| {
        nocturne_core::watched::scaled(
            pos as f64 / TICKS,
            rt as f64 / TICKS,
            shown_runtime as f64 / TICKS,
        )
        .map_or(pos, |s| (s * TICKS) as i64)
    });
    let mut last = newest.map(|n| n.0.to_string()).filter(|s| !s.is_empty());
    if let Some(o) = ov {
        fav = o.favorite.unwrap_or(fav);
        if day(o.last.as_deref().unwrap_or("")) >= day(last.as_deref().unwrap_or("")) {
            played = o.played.unwrap_or(played);
            ticks = o.ticks.unwrap_or(ticks);
            count = u64::from(o.count.unwrap_or(count as u32));
            last = o.last.clone().or(last);
        }
    }
    if played {
        ticks = 0;
    }
    let o = out.as_object_mut().expect("object");
    o.insert("Played".into(), played.into());
    o.insert("IsFavorite".into(), fav.into());
    o.insert("PlayCount".into(), count.into());
    o.insert("PlaybackPositionTicks".into(), ticks.into());
    o.insert("ItemId".into(), item.to_string().into());
    o.entry("Key").or_insert_with(|| item.to_string().into());
    if shown_runtime > 0 && ticks > 0 {
        o.insert(
            "PlayedPercentage".into(),
            (ticks as f64 * 100.0 / shown_runtime as f64).into(),
        );
    } else {
        o.remove("PlayedPercentage");
    }
    match last {
        Some(l) => o.insert("LastPlayedDate".into(), l.into()),
        None => o.remove("LastPlayedDate"),
    };
    out
}

// ---------------------------------------------------------------- writes

/// What a mark changes.
#[derive(Clone, Copy)]
pub enum Mark {
    Played(bool),
    Favorite(bool),
}

/// Record `mark` for `item` in the overlay and push it where WRITES.md allows. Returns the overlay entry.
pub async fn apply_mark(st: &State, item: &str, mark: Mark) -> Result<Ov, StatusCode> {
    let (gid, members) = st.ids.resolve_str(item).ok_or(StatusCode::NOT_FOUND)?;
    let ov = overlay(st).update(st, gid, |o| match mark {
        Mark::Played(p) => {
            o.played = Some(p);
            o.ticks = Some(0);
            o.last = Some(iso(unix_now()));
            if p {
                o.count = Some(o.count.unwrap_or(0) + 1);
            }
        }
        Mark::Favorite(f) => o.favorite = Some(f),
    });
    // Local copies follow without the master switch (they write the library's own state file).
    for m in members.iter().filter(|m| m.source == local_dto::SRC) {
        if let (Mark::Played(p), Some((src, _))) = (mark, local_dto::parse_item(&m.id)) {
            let id = m.id.clone();
            let st2 = st.clone();
            let _ = tokio::task::spawn_blocking(move || {
                local_dto::with_lib(&st2, src, |lib| local_dto::record(lib, &id, None, Some(p)))
            })
            .await;
        }
    }
    forward_mark(st, &members, mark).await;
    Ok(ov)
}

/// The servers a mark may be sent to: nothing without `allow_server_writes`; the item's own (first) server; with `sync_watched` every server copy of a played mark.
pub fn mark_targets(st: &State, members: &[Member], mark: Mark) -> Vec<(Srv, String)> {
    let cfg = st.cfg();
    if !cfg.allow_server_writes {
        return vec![];
    }
    let all = items::servers(st);
    let mut out: Vec<(Srv, String)> = members
        .iter()
        .filter_map(|m| {
            all.iter()
                .find(|s| s.key == m.source)
                .map(|s| (s.clone(), m.id.clone()))
        })
        .collect();
    out.sort_by_key(|(s, _)| s.idx);
    let several = matches!(mark, Mark::Played(_)) && cfg.sync_watched;
    out.truncate(if several { usize::MAX } else { 1 });
    out
}

async fn forward_mark(st: &State, members: &[Member], mark: Mark) {
    let mut any = false;
    for (srv, up) in mark_targets(st, members, mark) {
        let (seg, on) = match mark {
            Mark::Played(p) => ("UserPlayedItems", p),
            Mark::Favorite(f) => ("UserFavoriteItems", f),
        };
        let pq = format!(
            "/{seg}/{}?userId={}",
            items::enc(&up),
            items::enc(&srv.user_id)
        );
        let r = if on {
            srv.write.post(&pq, None).await
        } else {
            srv.write.delete(&pq).await
        };
        any = true;
        match r {
            Ok(resp) if resp.status().is_success() => {}
            Ok(resp) => crate::glog!("{}: mark not accepted ({})", srv.label, resp.status()),
            Err(e) => crate::glog!("{}: mark failed ({e})", srv.label),
        }
    }
    if any {
        items::hub(st).cache.clear();
    }
}

fn dto_of(ov: &Ov, gid: Gid) -> Value {
    json!({
        "PlaybackPositionTicks": ov.ticks.unwrap_or(0), "PlayCount": ov.count.unwrap_or(0),
        "IsFavorite": ov.favorite.unwrap_or(false), "Played": ov.played.unwrap_or(false),
        "LastPlayedDate": ov.last, "Key": gid.to_string(), "ItemId": gid.to_string(),
    })
}

fn item_param(p: &HashMap<String, String>) -> &str {
    p.get("itemId").map_or("", String::as_str)
}

async fn user_data_of(st: &State, item: &str, headers: &HeaderMap, uri: &Uri) -> Response {
    let Some((gid, _)) = st.ids.resolve_str(item) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match items::detail(st, gid, items::client_token(headers, uri).as_deref()).await {
        Some(d) => Json(d["UserData"].clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn mark_resp(st: &State, p: &HashMap<String, String>, mark: Mark) -> Response {
    let item = item_param(p);
    match apply_mark(st, item, mark).await {
        Ok(ov) => {
            let g = st.ids.resolve_str(item).map(|x| x.0).unwrap_or(Gid::of(""));
            Json(dto_of(&ov, g)).into_response()
        }
        Err(c) => c.into_response(),
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Update {
    played: Option<bool>,
    is_favorite: Option<bool>,
    playback_position_ticks: Option<i64>,
    play_count: Option<u32>,
}

async fn set_user_data(
    St(st): St<State>,
    Path(p): Path<HashMap<String, String>>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let u: Update = serde_json::from_value(crate::playback::parse_body(&body)).unwrap_or_default();
    let item = item_param(&p);
    if let Some(pl) = u.played {
        let _ = apply_mark(&st, item, Mark::Played(pl)).await;
    }
    if let Some(f) = u.is_favorite {
        let _ = apply_mark(&st, item, Mark::Favorite(f)).await;
    }
    if u.playback_position_ticks.is_some() || u.play_count.is_some() {
        // A position set by hand lands in the overlay only: it is not a playback report, nothing is forwarded.
        if let Some((gid, _)) = st.ids.resolve_str(item) {
            overlay(&st).update(&st, gid, |o| {
                o.ticks = u.playback_position_ticks.or(o.ticks);
                o.count = u.play_count.or(o.count);
                o.last = Some(iso(unix_now()));
            });
        }
    }
    user_data_of(&st, item, &headers, &uri).await
}

pub fn routes(r: Routes) -> Routes {
    let played = |m: Mark| {
        move |St(st): St<State>, Path(p): Path<HashMap<String, String>>| async move {
            mark_resp(&st, &p, m).await
        }
    };
    let _ = SourceId(0);
    let played_on = played(Mark::Played(true));
    let played_off = played(Mark::Played(false));
    let fav_on = played(Mark::Favorite(true));
    let fav_off = played(Mark::Favorite(false));
    let (p2, p3, f2, f3) = (played_on, played_off, fav_on, fav_off);
    r.add("/UserPlayedItems/{itemId}", Access::Auth, post(played_on).delete(played_off))
        .add("/Users/{userId}/PlayedItems/{itemId}", Access::Auth, post(p2).delete(p3))
        .add("/UserFavoriteItems/{itemId}", Access::Auth, post(fav_on).delete(fav_off))
        .add("/Users/{userId}/FavoriteItems/{itemId}", Access::Auth, post(f2).delete(f3))
        .add(
            "/UserItems/{itemId}/UserData",
            Access::Auth,
            get(|St(st): St<State>, Path(p): Path<HashMap<String, String>>, h: HeaderMap, u: Uri| async move {
                user_data_of(&st, item_param(&p), &h, &u).await
            })
            .post(set_user_data),
        )
        .add(
            "/Users/{userId}/Items/{itemId}/UserData",
            Access::Auth,
            get(|St(st): St<State>, Path(p): Path<HashMap<String, String>>, h: HeaderMap, u: Uri| async move {
                user_data_of(&st, item_param(&p), &h, &u).await
            })
            .post(set_user_data),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ud(played: bool, pos: i64, last: &str, fav: bool, count: u64) -> Value {
        json!({"Played": played, "PlaybackPositionTicks": pos, "LastPlayedDate": last, "IsFavorite": fav, "PlayCount": count, "Key": "k"})
    }
    const G: fn() -> Gid = || Gid::of("x");

    #[test]
    fn any_played_any_favourite_max_count() {
        let (a, b) = (
            ud(false, 0, "", false, 1),
            ud(true, 0, "2025-01-01T00:00:00.0000000Z", true, 3),
        );
        let m = merge_user_data(
            &[
                CopyData {
                    ud: Some(&a),
                    runtime: 100 * 10_000_000,
                },
                CopyData {
                    ud: Some(&b),
                    runtime: 100 * 10_000_000,
                },
            ],
            100 * 10_000_000,
            None,
            G(),
        );
        assert_eq!(
            (
                m["Played"].clone(),
                m["IsFavorite"].clone(),
                m["PlayCount"].clone()
            ),
            (json!(true), json!(true), json!(3))
        );
        assert_eq!(
            m["PlaybackPositionTicks"], 0,
            "a watched item has no resume point"
        );
    }

    #[test]
    fn resume_from_newest_copy_scaled_by_runtime() {
        let a = ud(
            false,
            50 * 10_000_000,
            "2025-01-01T10:00:00.0000000Z",
            false,
            0,
        );
        let b = ud(
            false,
            30 * 10_000_000,
            "2025-02-01T10:00:00.0000000Z",
            false,
            0,
        );
        // The newer copy (b, 100 s long) stopped at 30 s; shown copy is 200 s long: 60 s.
        let m = merge_user_data(
            &[
                CopyData {
                    ud: Some(&a),
                    runtime: 100 * 10_000_000,
                },
                CopyData {
                    ud: Some(&b),
                    runtime: 100 * 10_000_000,
                },
            ],
            200 * 10_000_000,
            None,
            G(),
        );
        assert_eq!(m["PlaybackPositionTicks"], 60 * 10_000_000);
        assert_eq!(m["LastPlayedDate"], "2025-02-01T10:00:00.0000000Z");
        assert_eq!(m["PlayedPercentage"].as_f64().unwrap().round(), 30.0);
    }

    #[test]
    fn overlay_wins_only_when_newer_and_favourite_always() {
        let a = ud(true, 0, "2025-03-01T00:00:00.0000000Z", false, 1);
        let older = Ov {
            played: Some(false),
            ticks: Some(5),
            favorite: Some(true),
            last: Some("2025-01-01T00:00:00.0000000Z".into()),
            ..Default::default()
        };
        let m = merge_user_data(
            &[CopyData {
                ud: Some(&a),
                runtime: 0,
            }],
            0,
            Some(&older),
            G(),
        );
        assert_eq!(
            (m["Played"].clone(), m["IsFavorite"].clone()),
            (json!(true), json!(true))
        );
        let newer = Ov {
            played: Some(false),
            ticks: Some(7),
            last: Some("2025-04-01T00:00:00.0000000Z".into()),
            ..Default::default()
        };
        let m = merge_user_data(
            &[CopyData {
                ud: Some(&a),
                runtime: 0,
            }],
            0,
            Some(&newer),
            G(),
        );
        assert_eq!(
            (m["Played"].clone(), m["PlaybackPositionTicks"].clone()),
            (json!(false), json!(7))
        );
        // No server knows the item: the overlay alone.
        let m = merge_user_data(&[], 0, Some(&newer), G());
        assert_eq!(m["PlaybackPositionTicks"], 7);
    }

    #[test]
    fn overlay_file_round_trips_and_lists_resumable() {
        let o = Overlay::default();
        let g = Gid::of("a");
        {
            let mut m = o.map.lock().unwrap();
            m.insert(
                g.to_string(),
                Ov {
                    ticks: Some(9),
                    ts: 5,
                    ..Default::default()
                },
            );
            m.insert(
                Gid::of("b").to_string(),
                Ov {
                    ticks: Some(9),
                    played: Some(true),
                    ts: 9,
                    ..Default::default()
                },
            );
        }
        assert_eq!(o.get(g).unwrap().ticks, Some(9));
        let r = o.resumable(10);
        assert_eq!(r.len(), 1, "played items do not resume");
        assert_eq!(r[0].0, g);
        let f: File = serde_json::from_slice(
            &serde_json::to_vec(&File {
                version: 1,
                items: HashMap::from([("x".into(), Ov::default())]),
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(f.items.len(), 1);
    }
}
