//! Small routes answered by the gateway alone that clients call at start-up: display preferences and user configuration (the gateway's own overlay, never a server's), client log upload, bitrate test, and the empty answers the web client needs for features the gateway does not have.
use crate::server::{Access, Routes};
use crate::state::State;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State as St};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Display preferences per (display preferences id, client), kept in `displayprefs.json` (0600). Clients only read back what they wrote.
#[derive(Default)]
pub struct Prefs {
    map: Mutex<BTreeMap<String, Value>>,
    loaded: std::sync::OnceLock<()>,
}

const MAX_PREFS: usize = 256;
const MAX_PREF_BYTES: usize = 64 * 1024;

fn prefs_path(st: &State) -> std::path::PathBuf {
    st.paths.data.join("displayprefs.json")
}

fn load_prefs(st: &State, p: &Prefs) {
    p.loaded.get_or_init(|| {
        if let Some(m) = std::fs::read(prefs_path(st))
            .ok()
            .and_then(|b| serde_json::from_slice::<BTreeMap<String, Value>>(&b).ok())
        {
            *p.map.lock().unwrap() = m;
        }
    });
}

#[derive(Deserialize)]
struct PrefQuery {
    client: Option<String>,
}

fn pref_key(id: &str, client: &Option<String>) -> String {
    format!(
        "{}|{}",
        id.to_ascii_lowercase(),
        client.as_deref().unwrap_or("").to_ascii_lowercase()
    )
}

fn default_prefs(id: &str, client: &Option<String>) -> Value {
    json!({
        "Id": id, "SortBy": "SortName", "RememberIndexing": false, "PrimaryImageHeight": 250,
        "PrimaryImageWidth": 250, "CustomPrefs": {}, "ScrollDirection": "Horizontal", "ShowBackdrop": true,
        "RememberSorting": false, "SortOrder": "Ascending", "ShowSidebar": false,
        "Client": client.clone().unwrap_or_default(),
    })
}

async fn get_prefs(
    St(st): St<State>,
    Path(id): Path<String>,
    Query(q): Query<PrefQuery>,
) -> Json<Value> {
    let p = st.ext::<Prefs>();
    load_prefs(&st, &p);
    let v = p
        .map
        .lock()
        .unwrap()
        .get(&pref_key(&id, &q.client))
        .cloned();
    Json(v.unwrap_or_else(|| default_prefs(&id, &q.client)))
}

async fn set_prefs(
    St(st): St<State>,
    Path(id): Path<String>,
    Query(q): Query<PrefQuery>,
    body: Bytes,
) -> StatusCode {
    if body.len() > MAX_PREF_BYTES {
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    let Ok(mut v) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    if !v.is_object() {
        return StatusCode::BAD_REQUEST;
    }
    v["Id"] = json!(id);
    let p = st.ext::<Prefs>();
    load_prefs(&st, &p);
    let bytes = {
        let mut m = p.map.lock().unwrap();
        let key = pref_key(&id, &q.client);
        if m.len() >= MAX_PREFS && !m.contains_key(&key) {
            return StatusCode::INSUFFICIENT_STORAGE;
        }
        m.insert(key, v);
        serde_json::to_vec(&*m).unwrap_or_default()
    };
    let path = prefs_path(&st);
    let _ = tokio::task::spawn_blocking(move || crate::config::write_private(&path, &bytes)).await;
    StatusCode::NO_CONTENT
}

/// The gateway user's configuration (the web client's display, home, playback and subtitle settings), kept in `userconfig.json` (0600). Never sent to a server.
#[derive(Default)]
pub struct UserConfig(Mutex<Option<Value>>);

fn user_config_path(st: &State) -> std::path::PathBuf {
    st.paths.data.join("userconfig.json")
}

/// What the client last saved, if anything.
pub fn user_config(st: &State) -> Option<Value> {
    let c = st.ext::<UserConfig>();
    let mut g = c.0.lock().unwrap();
    if g.is_none() {
        *g = std::fs::read(user_config_path(st))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(Value::is_object);
    }
    g.clone()
}

/// `POST /Users/Configuration` (and the older `/Users/{userId}/Configuration`): the whole configuration object, as the client sends it.
async fn set_user_config(St(st): St<State>, body: Bytes) -> StatusCode {
    if body.len() > MAX_PREF_BYTES {
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    let Some(v) = serde_json::from_slice::<Value>(&body)
        .ok()
        .filter(Value::is_object)
    else {
        return StatusCode::BAD_REQUEST;
    };
    let bytes = serde_json::to_vec(&v).unwrap_or_default();
    *st.ext::<UserConfig>().0.lock().unwrap() = Some(v);
    let path = user_config_path(&st);
    let _ = tokio::task::spawn_blocking(move || crate::config::write_private(&path, &bytes)).await;
    StatusCode::NO_CONTENT
}

/// Language, country and rating lists for the settings pages: the same on every server, so the first one that answers is passed through (no ids, no paths) and kept for the life of the process.
#[derive(Default)]
pub struct Localization(Mutex<BTreeMap<&'static str, Value>>);

async fn localization(st: State, what: &'static str) -> Response {
    if let Some(v) = st.ext::<Localization>().0.lock().unwrap().get(what) {
        return Json(v.clone()).into_response();
    }
    for s in crate::items::servers(&st) {
        let pq = format!("/Localization/{what}");
        if let Some(v) = crate::items::get_json(&st, &s, &pq, true).await
            && v.is_array()
        {
            st.ext::<Localization>()
                .0
                .lock()
                .unwrap()
                .insert(what, (*v).clone());
            return crate::items::json_out(&st, &v);
        }
    }
    Json(json!([])).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SizeQuery {
    size: Option<usize>,
}

const MAX_TEST_BYTES: usize = 5 << 20;

/// `GET /Playback/BitrateTest?Size=N`: N bytes (capped at 5 MiB) that do not compress, so clients measure the link.
async fn bitrate_test(Query(q): Query<SizeQuery>) -> Response {
    let n = q.size.unwrap_or(500_000).min(MAX_TEST_BYTES);
    let chunk = crate::auth::random_bytes::<4096>();
    let body: Vec<u8> = chunk.iter().copied().cycle().take(n).collect();
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

/// Accepted and dropped: client crash logs are not kept.
async fn client_log(_: Bytes) -> Json<Value> {
    Json(json!({"FileName": "client.log"}))
}

pub fn routes(r: Routes) -> Routes {
    r.add(
        "/DisplayPreferences/{displayPreferencesId}",
        Access::Auth,
        get(get_prefs).post(set_prefs),
    )
    .add("/Playback/BitrateTest", Access::Auth, get(bitrate_test))
    .add("/ClientLog/Document", Access::Auth, post(client_log))
    .add("/Users/Configuration", Access::Auth, post(set_user_config))
    .add(
        "/Users/{userId}/Configuration",
        Access::Auth,
        post(set_user_config),
    )
    .add(
        "/Localization/Cultures",
        Access::Auth,
        get(|St(st): St<State>| localization(st, "Cultures")),
    )
    .add(
        "/Localization/Countries",
        Access::Auth,
        get(|St(st): St<State>| localization(st, "Countries")),
    )
    .add(
        "/Localization/ParentalRatings",
        Access::Auth,
        get(|St(st): St<State>| localization(st, "ParentalRatings")),
    )
    // The web client's network check: never "local", so it measures the link and picks a bitrate.
    .add(
        "/System/Endpoint",
        Access::Auth,
        get(|| async { Json(json!({"IsLocal": false, "IsInNetwork": false})) }),
    )
    // SyncPlay stays in NOCTURNE (plan section 1): no groups.
    .add(
        "/SyncPlay/List",
        Access::Auth,
        get(|| async { Json(json!([])) }),
    )
    // Library grouping on the home settings page: the merged views are never grouped.
    .add(
        "/UserViews/GroupingOptions",
        Access::Auth,
        get(|| async { Json(json!([])) }),
    )
    .add(
        "/Users/{userId}/GroupingOptions",
        Access::Auth,
        get(|| async { Json(json!([])) }),
    )
}
