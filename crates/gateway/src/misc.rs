//! Small routes answered by the gateway alone that clients call at start-up: display preferences, client log upload, bitrate test.
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
}
