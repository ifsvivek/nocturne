//! `PlaybackInfo` and playback reporting from clients.
//! PlaybackInfo is answered per copy (each server decides direct play or transcode for its own file), the media sources of every copy are unioned and labelled by origin, and `TranscodingUrl` is rewritten so the client's own token replaces the server's (finding V5). Local files answer direct play only.
//! Reports (`/Sessions/Playing[/Progress|/Stopped|/Ping]`) are always recorded (in-memory now-playing table for the session routes, the overlay for position and watched, the local `state.json` for local items) and forwarded to the item's own server only as WRITES.md allows: `allow_server_writes` and `sync_progress`.
use crate::auth::{AuthCtx, iso, unix_now};
use crate::idmap::Gid;
use crate::items::{self, Rw, client_token, enc};
use crate::lists::{Query, encode_query};
use crate::local_dto;
use crate::rewrite::{UrlCtx, rewrite_source};
use crate::server::{Access, Routes};
use crate::state::State;
use crate::userdata::overlay;
use axum::Extension;
use axum::body::Bytes;
use axum::extract::{Path, State as St};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

const TICKS: i64 = 10_000_000;
/// Jellyfin marks an item played when playback stops at or past this share of its length.
const PLAYED_AT: f64 = 0.9;

/// What a signed-in gateway client is playing right now, by token id. The session routes read it to show `NowPlayingItem`.
#[derive(Clone, Debug)]
pub struct NowPlaying {
    pub item: Gid,
    pub ticks: i64,
    pub paused: bool,
    pub play_session_id: Option<String>,
    /// Gateway media source id.
    pub media_source_id: Option<String>,
    pub updated: u64,
}

#[derive(Default)]
pub struct PlayTable(Mutex<HashMap<String, NowPlaying>>);

impl PlayTable {
    pub fn get(&self, token_id: &str) -> Option<NowPlaying> {
        self.0.lock().unwrap().get(token_id).cloned()
    }
    pub fn all(&self) -> Vec<(String, NowPlaying)> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    fn set(&self, token_id: &str, n: Option<NowPlaying>) {
        let mut m = self.0.lock().unwrap();
        match n {
            Some(n) => m.insert(token_id.to_string(), n),
            None => m.remove(token_id),
        };
    }
}

// ---------------------------------------------------------------- PlaybackInfo

async fn playback_info(
    St(st): St<State>,
    Path(p): Path<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let (q, tok) = (Query::parse(uri.query()), client_token(&headers, &uri));
    let item = p.get("itemId").map_or("", String::as_str);
    let Some((gid, members)) = st.ids.resolve_str(item) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let body_json = parse_body(&body);
    let want = q
        .get("MediaSourceId")
        .or(body_json["MediaSourceId"].as_str())
        .filter(|m| !m.is_empty())
        .and_then(|m| st.ids.resolve_str(m))
        .filter(|(g, _)| *g != gid)
        .map(|(_, m)| m);
    let multi = members.len() > 1;
    let all = items::servers(&st);
    let mut futs = vec![];
    for m in members.iter().filter(|m| m.source != local_dto::SRC) {
        if want
            .as_ref()
            .is_some_and(|w| !w.iter().any(|x| x.source == m.source))
        {
            continue;
        }
        let Some(s) = all.iter().find(|s| s.key == m.source).cloned() else {
            continue;
        };
        let Some(mut pairs) = items::translate(&st.ids, &q, &s.key) else {
            continue;
        };
        pairs.retain(|(k, _)| !k.eq_ignore_ascii_case("MediaSourceId"));
        if let Some(u) = want
            .as_ref()
            .and_then(|w| w.iter().find(|x| x.source == m.source))
        {
            pairs.push(("MediaSourceId".into(), u.id.clone()));
        }
        pairs.push(("userId".into(), s.user_id.clone()));
        let pq = format!(
            "/Items/{}/PlaybackInfo?{}",
            enc(&m.id),
            encode_query(&pairs)
        );
        let up_msid = want
            .as_ref()
            .and_then(|w| w.iter().find(|x| x.source == m.source))
            .map(|x| x.id.clone());
        let (st2, up, post_body, get_it) = (
            st.clone(),
            m.id.clone(),
            upstream_body(&body_json, &body, &s.user_id, up_msid.as_deref()),
            method == Method::GET,
        );
        futs.push(async move {
            let r = if get_it {
                s.raw.get(&pq, &[]).await.ok()?
            } else {
                s.write.post(&pq, Some(post_body)).await.ok()?
            };
            if r.status() == StatusCode::UNAUTHORIZED {
                st2.mark_signed_out(s.id);
                return None;
            }
            if !r.status().is_success() {
                return None;
            }
            let v: Value = serde_json::from_slice(&r.bytes().await.ok()?).ok()?;
            Some((s, up, v))
        });
    }
    let mut sources: Vec<Value> = vec![];
    let mut play_session: Option<Value> = None;
    for (s, up, v) in items::fan_out(futs).await.into_iter().flatten() {
        play_session = play_session.or(v.get("PlaySessionId").cloned());
        let c = UrlCtx {
            ids: &st.ids,
            src_key: &s.key,
            item: gid,
            up_item: &up,
            authority: &s.authority,
            token: tok.as_deref(),
        };
        for mut src in v["MediaSources"].as_array().cloned().unwrap_or_default() {
            rewrite_source(&mut src, &c, multi.then_some(s.label.as_str()));
            sources.push(src);
        }
    }
    // Local copies: one direct-play source each.
    if want.is_none() || members.iter().any(|m| m.source == local_dto::SRC) {
        let rwv = items::rw(&st);
        for m in members.iter().filter(|m| m.source == local_dto::SRC) {
            if want
                .as_ref()
                .is_some_and(|w| !w.iter().any(|x| x.source == m.source))
            {
                continue;
            }
            sources.extend(local_sources(&st, &rwv, gid, &m.id, tok.as_deref(), multi).await);
        }
    }
    if sources.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    items::json_out(
        &st,
        &json!({"MediaSources": sources, "PlaySessionId": play_session.unwrap_or_else(|| crate::auth::random_hex::<8>().into())}),
    )
}

/// The client's PlaybackInfo body for one server: its user and media source ids instead of the gateway's.
fn upstream_body(v: &Value, raw: &[u8], user_id: &str, msid: Option<&str>) -> Vec<u8> {
    let Some(o) = v.as_object() else {
        return raw.to_vec();
    };
    let mut o = o.clone();
    if o.contains_key("UserId") {
        o.insert("UserId".into(), user_id.into());
    }
    match msid {
        Some(m) => o.insert("MediaSourceId".into(), m.into()),
        None => o.remove("MediaSourceId"),
    };
    serde_json::to_vec(&o).unwrap_or_else(|_| raw.to_vec())
}

async fn local_sources(
    st: &State,
    rwv: &Rw,
    gid: Gid,
    member: &str,
    tok: Option<&str>,
    multi: bool,
) -> Vec<Value> {
    let Some(src) = local_dto::parse_member(member).map(|t| local_dto::src_of(&t)) else {
        return vec![];
    };
    let (st2, id, server_id) = (st.clone(), member.to_string(), rwv.server_id.clone());
    let dto = tokio::task::spawn_blocking(move || {
        let h = items::hub(&st2);
        local_dto::with_lib(&st2, src, |lib| {
            let cx = local_dto::Ctx {
                ids: &st2.ids,
                alias: &h.alias,
                server_id: &server_id,
                src,
            };
            local_dto::member_dto(lib, &id, &cx, true)
        })
    })
    .await
    .ok()
    .flatten()
    .flatten();
    let label = st
        .upstreams()
        .iter()
        .find(|u| u.id == src)
        .map(|u| u.label.clone())
        .unwrap_or_default();
    let c = UrlCtx {
        ids: &st.ids,
        src_key: local_dto::SRC,
        item: gid,
        up_item: member,
        authority: "",
        token: tok,
    };
    dto.and_then(|d| d["MediaSources"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .map(|mut s| {
            rewrite_source(&mut s, &c, multi.then_some(label.as_str()));
            s
        })
        .collect()
}

// ---------------------------------------------------------------- reporting

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Start,
    Progress,
    Stopped,
    Ping,
}

impl Kind {
    fn path(self) -> &'static str {
        match self {
            Kind::Start => "/Sessions/Playing",
            Kind::Progress => "/Sessions/Playing/Progress",
            Kind::Stopped => "/Sessions/Playing/Stopped",
            Kind::Ping => "/Sessions/Playing/Ping",
        }
    }
}

/// Whether a stop at `ticks` of `runtime` counts as watched.
pub fn finished(ticks: i64, runtime: i64) -> bool {
    runtime > 0 && ticks as f64 >= runtime as f64 * PLAYED_AT
}

async fn report(st: State, ctx: AuthCtx, kind: Kind, body: Bytes) -> StatusCode {
    let mut v = parse_body(&body);
    if !v.is_object() {
        return StatusCode::BAD_REQUEST;
    }
    let item = v["ItemId"]
        .as_str()
        .or(v["Item"]["Id"].as_str())
        .unwrap_or("")
        .to_string();
    let Some((gid, members)) = st.ids.resolve_str(&item) else {
        return if kind == Kind::Ping {
            StatusCode::NO_CONTENT
        } else {
            StatusCode::NOT_FOUND
        };
    };
    let ticks = v["PositionTicks"].as_i64().unwrap_or(0).max(0);
    let table = st.ext::<PlayTable>();
    if kind == Kind::Stopped {
        table.set(&ctx.token_id, None);
    } else {
        table.set(
            &ctx.token_id,
            Some(NowPlaying {
                item: gid,
                ticks,
                paused: v["IsPaused"].as_bool().unwrap_or(false),
                play_session_id: v["PlaySessionId"].as_str().map(String::from),
                media_source_id: v["MediaSourceId"].as_str().map(String::from),
                updated: unix_now(),
            }),
        );
    }
    if kind == Kind::Ping || kind == Kind::Start {
        forward(&st, kind, gid, &members, &mut v).await;
        return StatusCode::NO_CONTENT;
    }
    let played = if kind == Kind::Stopped {
        let runtime = match items::hub(&st).runtime(gid) {
            Some(r) => r,
            None => items::detail(&st, gid, None)
                .await
                .map_or(0, |d| d["RunTimeTicks"].as_i64().unwrap_or(0)),
        };
        finished(ticks, runtime)
    } else {
        false
    };
    overlay(&st).update(&st, gid, |o| {
        o.ticks = Some(if played { 0 } else { ticks });
        o.last = Some(iso(unix_now()));
        if played {
            o.played = Some(true);
            o.count = Some(o.count.unwrap_or(0) + 1);
        }
    });
    for m in members.iter().filter(|m| m.source == local_dto::SRC) {
        if let Some(src) = local_dto::parse_member(&m.id).map(|t| local_dto::src_of(&t)) {
            let (st2, id) = (st.clone(), m.id.clone());
            let secs = (ticks / TICKS) as u64;
            let _ = tokio::task::spawn_blocking(move || {
                local_dto::with_lib(&st2, src, |lib| {
                    local_dto::record(
                        lib,
                        &id,
                        Some(if played { 0 } else { secs }),
                        played.then_some(true),
                    )
                })
            })
            .await;
        }
    }
    forward(&st, kind, gid, &members, &mut v).await;
    StatusCode::NO_CONTENT
}

/// Send the report to the item's own server when both switches are on (WRITES.md: `allow_server_writes` and `sync_progress`). Never anywhere else; never a failure to the client.
async fn forward(
    st: &State,
    kind: Kind,
    gid: Gid,
    members: &[crate::idmap::Member],
    v: &mut Value,
) {
    if !st.cfg().forward_progress() {
        return;
    }
    let msid = v["MediaSourceId"]
        .as_str()
        .and_then(|m| st.ids.resolve_str(m))
        .filter(|(g, _)| *g != gid)
        .map(|(_, m)| m);
    let all = items::servers(st);
    let mut targets: Vec<(&items::Srv, &crate::idmap::Member)> = members
        .iter()
        .filter_map(|m| all.iter().find(|s| s.key == m.source).map(|s| (s, m)))
        .collect();
    targets.sort_by_key(|(s, _)| s.idx);
    let chosen = msid.as_ref().and_then(|w| {
        targets
            .iter()
            .find(|(s, _)| w.iter().any(|x| x.source == s.key))
            .copied()
    });
    let Some((srv, m)) = chosen.or(targets.first().copied()) else {
        return;
    };
    if let Some(o) = v.as_object_mut() {
        o.remove("Item");
        o.insert("ItemId".into(), m.id.clone().into());
        match msid.and_then(|w| w.into_iter().find(|x| x.source == srv.key)) {
            Some(u) => o.insert("MediaSourceId".into(), u.id.into()),
            None => o.remove("MediaSourceId"),
        };
    }
    let Ok(bytes) = serde_json::to_vec(v) else {
        return;
    };
    match srv.write.post(kind.path(), Some(bytes)).await {
        Ok(r) if r.status().is_success() => {
            if kind == Kind::Stopped {
                items::hub(st).cache.clear();
            }
        }
        Ok(r) => crate::glog!(
            "{}: playback report not accepted ({})",
            srv.label,
            r.status()
        ),
        Err(e) => crate::glog!("{}: playback report failed ({e})", srv.label),
    }
}

macro_rules! reporter {
    ($name:ident, $kind:expr) => {
        async fn $name(
            St(st): St<State>,
            Extension(ctx): Extension<AuthCtx>,
            body: Bytes,
        ) -> StatusCode {
            report(st, ctx, $kind, body).await
        }
    };
}
reporter!(report_start, Kind::Start);
reporter!(report_progress, Kind::Progress);
reporter!(report_stopped, Kind::Stopped);
reporter!(report_ping, Kind::Ping);

pub fn routes(r: Routes) -> Routes {
    r.add(
        "/Items/{itemId}/PlaybackInfo",
        Access::Auth,
        get(playback_info).post(playback_info),
    )
    .add("/Sessions/Playing", Access::Auth, post(report_start))
    .add(
        "/Sessions/Playing/Progress",
        Access::Auth,
        post(report_progress),
    )
    .add(
        "/Sessions/Playing/Stopped",
        Access::Auth,
        post(report_stopped),
    )
    .add("/Sessions/Playing/Ping", Access::Auth, post(report_ping))
}

const BODY_KEYS: [&str; 17] = [
    "ItemId",
    "MediaSourceId",
    "PositionTicks",
    "IsPaused",
    "PlaySessionId",
    "UserId",
    "Item",
    "Played",
    "IsFavorite",
    "PlaybackPositionTicks",
    "PlayCount",
    "EnableDirectPlay",
    "EnableDirectStream",
    "EnableTranscoding",
    "AutoOpenLiveStream",
    "DeviceProfile",
    "SessionId",
];

/// The one place request bodies are parsed: JSON whose top-level keys known to us are put into their canonical case (clients send `userId`, `itemId`; ASP.NET binds case-insensitively).
/// Not an object or not JSON: `Value::Null`. To be replaced by the shared case-insensitive extractor.
pub fn parse_body(raw: &[u8]) -> Value {
    let Ok(Value::Object(m)) = serde_json::from_slice::<Value>(raw) else {
        return Value::Null;
    };
    Value::Object(
        m.into_iter()
            .map(|(k, v)| {
                let k = BODY_KEYS
                    .iter()
                    .find(|c| c.eq_ignore_ascii_case(&k))
                    .map_or(k, |c| (*c).to_string());
                (k, v)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn body_keys_are_case_insensitive() {
        let v = super::parse_body(br#"{"itemid":"a","POSITIONTICKS":5,"Other":1}"#);
        assert_eq!(
            (
                v["ItemId"].clone(),
                v["PositionTicks"].clone(),
                v["Other"].clone()
            ),
            (
                serde_json::json!("a"),
                serde_json::json!(5),
                serde_json::json!(1)
            )
        );
        assert!(super::parse_body(b"[1]").is_null() && super::parse_body(b"x").is_null());
    }

    use super::*;

    #[test]
    fn watched_at_ninety_percent() {
        assert!(finished(90 * TICKS, 100 * TICKS));
        assert!(finished(100 * TICKS, 100 * TICKS));
        assert!(!finished(89 * TICKS, 100 * TICKS));
        assert!(!finished(5, 0), "unknown runtime never counts");
    }

    #[test]
    fn now_playing_table() {
        let t = PlayTable::default();
        let n = NowPlaying {
            item: Gid::of("x"),
            ticks: 5,
            paused: false,
            play_session_id: None,
            media_source_id: None,
            updated: 1,
        };
        t.set("tok", Some(n));
        assert_eq!(t.get("tok").unwrap().ticks, 5);
        assert_eq!(t.all().len(), 1);
        t.set("tok", None);
        assert!(t.get("tok").is_none());
    }
}
