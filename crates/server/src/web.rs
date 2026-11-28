//! The web dashboard: the files of `crates/server/web/` embedded at build time (`build.rs`) and served at `/`. Unknown paths outside `/api` get `index.html` (client-side routing); unknown `/api` paths are a JSON 404.
use crate::api::ApiErr;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

include!(concat!(env!("OUT_DIR"), "/web_files.rs"));

/// Strict: the dashboard loads only its own files and talks only to this server.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// Shown until the dashboard files exist.
const PLACEHOLDER: &str = "<!doctype html><meta charset=utf-8><title>NOCTURNE server</title><p>The NOCTURNE server is running. The dashboard is not part of this build; use the API at <code>/api/v1</code>.";

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn file(path: &str) -> Option<(&'static str, &'static [u8])> {
    FILES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(p, b)| (mime(p), *b))
}

pub async fn serve(req: Request) -> Response {
    let path = req.uri().path();
    if path.starts_with("/api/") || path == "/api" {
        return ApiErr::not_found().into_response();
    }
    let rel = path.trim_start_matches('/');
    let hit = file(rel).or_else(|| file("index.html"));
    match hit {
        Some((m, b)) => (
            [
                (header::CONTENT_TYPE, m),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::CACHE_CONTROL, "no-cache"),
                (header::CONTENT_SECURITY_POLICY, CSP),
                (header::X_FRAME_OPTIONS, "DENY"),
                (header::REFERRER_POLICY, "no-referrer"),
            ],
            b,
        )
            .into_response(),
        None => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CONTENT_SECURITY_POLICY, CSP),
            ],
            PLACEHOLDER,
        )
            .into_response(),
    }
}
