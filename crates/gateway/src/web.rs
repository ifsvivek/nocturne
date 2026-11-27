//! The Jellyfin web client, passed through from a server that has one. The official phone apps are a shell around it: they load `/web/` from the address they are given, so without it they never get past loading. Static files only (no token goes out, nothing is rewritten); every API call the page then makes comes back to the gateway itself.
use axum::extract::State as St;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use std::time::Duration;

use crate::items::servers;
use crate::server::{Access, Routes};
use crate::state::State;

const TIMEOUT: Duration = Duration::from_secs(30);
/// The largest file of jellyfin-web is a few MB; anything far above that is not the web client.
const MAX: usize = 32 << 20;

async fn file(St(st): St<State>, uri: Uri) -> Response {
    let path = uri.path();
    // `/web` and `/web/` are the client's start page, as on a real server.
    let pq = if path.eq_ignore_ascii_case("/web") || path.eq_ignore_ascii_case("/web/") {
        "/web/index.html".to_string()
    } else {
        match uri.query() {
            Some(q) => format!("{path}?{q}"),
            None => path.to_string(),
        }
    };
    for s in servers(&st) {
        let Ok(Ok(resp)) = tokio::time::timeout(TIMEOUT, s.raw.get(&pq, &[])).await else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let keep = |h: header::HeaderName| {
            resp.headers()
                .get(&h)
                .and_then(|v| v.to_str().ok())
                .map(|v| (h, v.to_string()))
        };
        let hdrs: Vec<_> = [header::CONTENT_TYPE, header::CACHE_CONTROL, header::ETAG]
            .into_iter()
            .filter_map(keep)
            .collect();
        let Ok(Ok(body)) = tokio::time::timeout(TIMEOUT, resp.bytes()).await else {
            continue;
        };
        if body.len() > MAX {
            continue;
        }
        let mut r = body.into_response();
        for (n, v) in hdrs {
            if let Ok(v) = v.parse() {
                r.headers_mut().insert(n, v);
            }
        }
        return r;
    }
    StatusCode::NOT_FOUND.into_response()
}

pub fn routes(r: Routes) -> Routes {
    // A real server sends `/` to its web client; browsers and the phone apps expect that.
    r.add("/", Access::Public, get(|| async { Redirect::to("/web/") }))
        .add_stream("/web", Access::Public, get(file))
        .add_stream("/web/", Access::Public, get(file))
        .add_stream("/web/{*path}", Access::Public, get(file))
}
