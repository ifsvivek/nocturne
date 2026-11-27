//! Media through the gateway (plan section 3): always proxied, never redirected, so a client holds only its gateway token.
//! Direct play and direct stream (`/Videos/{id}/stream`, `/Audio/{id}/stream`, universal, download) pass the body through without buffering and honour `Range`; HLS playlists are buffered (capped) and rewritten, segments stream.
//! Local files are served from the index with Range, read-only, direct play only. No server token, server URL or file path ever reaches a client.
//!
//! ponytail: no per-server cap on concurrent streams (each client holds one upstream connection); add a semaphore per server when a server needs protecting.
use crate::idmap::Gid;
use crate::items::{Srv, client_token, enc, servers};
use crate::lists::{Query, encode_query};
use crate::local_dto;
use crate::rewrite::{UrlCtx, rewrite_playlist};
use crate::server::{Access, Routes};
use crate::state::{State, UpKind};
use axum::body::{Body, Bytes};
use axum::extract::State as St;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use nocturne_core::source::SourceId;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const HEADER_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PLAYLIST: usize = 1 << 20;
const CHUNK: u64 = 64 * 1024;

/// Where an item's bytes come from.
pub enum Dest {
    Server {
        srv: Box<Srv>,
        gid: Gid,
        up_item: String,
        up_msid: Option<String>,
    },
    Local {
        src: SourceId,
        member: String,
    },
}

/// Pick the copy to serve: the media source the client chose (its id says which server), else the first copy in priority order.
pub fn pick(st: &State, item: &str, msid: Option<&str>) -> Option<Dest> {
    let (gid, members) = st.ids.resolve_str(item)?;
    let chosen = msid
        .filter(|m| !m.is_empty())
        .and_then(|m| st.ids.resolve_str(m))
        .filter(|(g, _)| *g != gid);
    let all = servers(st);
    let mut best: Option<(usize, Dest)> = None;
    for m in &members {
        let (idx, dest) = if m.source == local_dto::SRC {
            let Some(local_dto::Target::File(src, _)) = local_dto::parse_member(&m.id) else {
                continue;
            };
            let Some(idx) = st.upstreams().iter().position(|u| u.id == src) else {
                continue;
            };
            (
                idx,
                Dest::Local {
                    src,
                    member: m.id.clone(),
                },
            )
        } else {
            let Some(s) = all.iter().find(|s| s.key == m.source) else {
                continue;
            };
            let up_msid = chosen
                .as_ref()
                .and_then(|(_, mm)| mm.iter().find(|x| x.source == m.source))
                .map(|x| x.id.clone());
            (
                s.idx,
                Dest::Server {
                    srv: Box::new(s.clone()),
                    gid,
                    up_item: m.id.clone(),
                    up_msid,
                },
            )
        };
        let want = chosen
            .as_ref()
            .is_some_and(|(_, mm)| mm.iter().any(|x| x.source == m.source));
        // A chosen media source decides; otherwise the lowest priority index.
        let rank = if want { 0 } else { idx + 1 };
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, dest));
        }
    }
    best.map(|(_, d)| d)
}

/// Headers of the client's request that a server may see (the raw client filters again).
fn fwd(headers: &HeaderMap) -> Vec<(&str, &str)> {
    headers
        .iter()
        .filter_map(|(n, v)| Some((n.as_str(), v.to_str().ok()?)))
        .collect()
}

/// Response headers a client gets from a server's answer.
const BACK: [&str; 7] = [
    "content-type",
    "content-length",
    "content-range",
    "accept-ranges",
    "etag",
    "last-modified",
    "content-disposition",
];

pub fn relay(resp: reqwest::Response, no_store: bool) -> Response {
    let status = resp.status();
    let keep: Vec<(HeaderName, HeaderValue)> = resp
        .headers()
        .iter()
        .filter(|(n, _)| BACK.contains(&n.as_str()))
        .map(|(n, v)| (n.clone(), v.clone()))
        .collect();
    let r: axum::http::Response<reqwest::Body> = resp.into();
    let mut out = Response::new(Body::new(r.into_body()));
    *out.status_mut() = status;
    for (n, v) in keep {
        out.headers_mut().insert(n, v);
    }
    if no_store {
        out.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    out
}

fn bad_gateway() -> Response {
    StatusCode::BAD_GATEWAY.into_response()
}

fn is_playlist(path: &str, ctype: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".m3u8") || ctype.to_ascii_lowercase().contains("mpegurl")
}

/// The upstream request of a media route: the request path with the item (and a media source in a subtitle or attachment path) replaced by the server's own ids, the query without the client's token.
fn upstream_pq(
    st: &State,
    uri: &Uri,
    up_item: &str,
    up_msid: Option<&str>,
    srv: &Srv,
) -> Option<String> {
    let mut segs: Vec<String> = uri.path().split('/').map(String::from).collect();
    // segs[0] is "" (leading slash): the item is segs[2].
    *segs.get_mut(2)? = enc(up_item);
    let lower: Vec<String> = segs.iter().map(|s| s.to_ascii_lowercase()).collect();
    if segs.len() > 4 && matches!(lower[4].as_str(), "subtitles" | "attachments") {
        let m = &segs[3];
        segs[3] = match crate::items::up_id_for(&st.ids, m, &srv.key) {
            Some(u) => enc(&u),
            None => up_msid.map_or_else(|| enc(up_item), enc),
        };
    }
    let q = Query::parse(uri.query());
    let mut pairs = vec![];
    for (k, v) in q.0 {
        match k.to_ascii_lowercase().as_str() {
            "apikey" | "api_key" => {}
            "userid" => pairs.push((k, srv.user_id.clone())),
            "mediasourceid" => {
                if let Some(u) = up_msid
                    .map(String::from)
                    .or_else(|| crate::items::up_id_for(&st.ids, &v, &srv.key))
                {
                    pairs.push((k, u));
                }
            }
            _ => pairs.push((k, v)),
        }
    }
    let path = segs.join("/");
    Some(if pairs.is_empty() {
        path
    } else {
        format!("{path}?{}", encode_query(&pairs))
    })
}

/// Media requests served since start (`state.ext::<Stats>()`): how many, how many with a `Range`, and the body bytes announced (`Content-Length`). For tests and diagnosis; no per-client data.
#[derive(Default)]
pub struct Stats {
    pub requests: std::sync::atomic::AtomicU64,
    pub ranged: std::sync::atomic::AtomicU64,
    pub bytes: std::sync::atomic::AtomicU64,
}

async fn media(St(st): St<State>, headers: HeaderMap, uri: Uri) -> Response {
    use std::sync::atomic::Ordering::Relaxed;
    let stats = st.ext::<Stats>();
    stats.requests.fetch_add(1, Relaxed);
    if headers.contains_key(header::RANGE) {
        stats.ranged.fetch_add(1, Relaxed);
    }
    let r = media_inner(st, headers, uri).await;
    if let Some(n) = r
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        stats.bytes.fetch_add(n, Relaxed);
    }
    r
}

/// Every media route: `/Videos/{id}/...`, `/Audio/{id}/...`, `/Items/{id}/Download`, `/MediaSegments/{id}`, trickplay, subtitles, attachments.
async fn media_inner(st: State, headers: HeaderMap, uri: Uri) -> Response {
    let segs: Vec<&str> = uri.path().split('/').collect();
    let Some(item) = segs.get(2).copied() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let q = Query::parse(uri.query());
    let path_msid = (segs.len() > 4
        && matches!(
            segs[4].to_ascii_lowercase().as_str(),
            "subtitles" | "attachments"
        ))
    .then(|| segs[3]);
    let Some(dest) = pick(&st, item, path_msid.or(q.get("MediaSourceId"))) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match dest {
        Dest::Local { src, member } => {
            // Direct play only: the stream routes, not HLS, subtitles or trickplay.
            let last = segs.last().copied().unwrap_or("").to_ascii_lowercase();
            let streamish = last == "stream"
                || last.starts_with("stream.")
                || last == "universal"
                || last == "download";
            if !streamish {
                return StatusCode::NOT_FOUND.into_response();
            }
            serve_local(&st, src, &member, &headers).await
        }
        Dest::Server {
            srv,
            gid,
            up_item,
            up_msid,
        } => {
            let Some(pq) = upstream_pq(&st, &uri, &up_item, up_msid.as_deref(), &srv) else {
                return StatusCode::NOT_FOUND.into_response();
            };
            let sent = tokio::time::timeout(HEADER_TIMEOUT, srv.raw.get(&pq, &fwd(&headers))).await;
            let Ok(Ok(resp)) = sent else {
                return bad_gateway();
            };
            if resp.status() == StatusCode::UNAUTHORIZED {
                st.mark_signed_out(srv.id);
                return bad_gateway();
            }
            if resp.status().is_redirection() {
                return bad_gateway();
            }
            let ctype = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            if resp.status().is_success() && is_playlist(uri.path(), &ctype) {
                return playlist(
                    &st,
                    resp,
                    &srv,
                    gid,
                    &up_item,
                    client_token(&headers, &uri).as_deref(),
                )
                .await;
            }
            relay(resp, true)
        }
    }
}

async fn playlist(
    st: &State,
    mut resp: reqwest::Response,
    srv: &Srv,
    gid: Gid,
    up_item: &str,
    token: Option<&str>,
) -> Response {
    let mut buf: Vec<u8> = vec![];
    while let Ok(Some(c)) = tokio::time::timeout(HEADER_TIMEOUT, resp.chunk())
        .await
        .unwrap_or(Ok(None))
    {
        buf.extend_from_slice(&c);
        if buf.len() > MAX_PLAYLIST {
            return bad_gateway();
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let c = UrlCtx {
        ids: &st.ids,
        src_key: &srv.key,
        item: gid,
        up_item,
        authority: &srv.authority,
        token,
    };
    let out = rewrite_playlist(&text, &c);
    (
        [
            (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        out,
    )
        .into_response()
}

// ---------------------------------------------------------------- local files

#[derive(Debug, PartialEq, Eq)]
pub enum Span {
    Full,
    /// Inclusive byte range.
    Part(u64, u64),
    Unsatisfiable,
}

/// `Range: bytes=...` against a file of `len` bytes. One range only (several are served whole, as RFC 9110 allows); an end past the file is clamped; a malformed header is ignored.
pub fn parse_range(h: Option<&str>, len: u64) -> Span {
    let Some(spec) = h.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return Span::Full;
    };
    if spec.contains(',') || len == 0 {
        return Span::Full;
    }
    let Some((a, b)) = spec.trim().split_once('-') else {
        return Span::Full;
    };
    match (a.trim().parse::<u64>(), b.trim().parse::<u64>()) {
        (Ok(a), _) if a >= len => Span::Unsatisfiable,
        (Ok(a), Ok(b)) if b >= a => Span::Part(a, b.min(len - 1)),
        (Ok(a), Err(_)) if b.trim().is_empty() => Span::Part(a, len - 1),
        (Err(_), Ok(n)) if a.trim().is_empty() => {
            if n == 0 {
                Span::Unsatisfiable
            } else {
                Span::Part(len.saturating_sub(n), len - 1)
            }
        }
        _ => Span::Full,
    }
}

pub fn content_type(p: &Path) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "ts" | "m2ts" => "video/mp2t",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" | "aac" => "audio/mp4",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// The index's path for a local file, checked to still be under one of the source's folders.
async fn local_path(st: &State, src: SourceId, member: &str) -> Option<PathBuf> {
    let (st2, m) = (st.clone(), member.to_string());
    let p = tokio::task::spawn_blocking(move || {
        local_dto::with_lib(&st2, src, |lib| local_dto::file_path(lib, &m))
    })
    .await
    .ok()??;
    under_roots(st, src, &p?)
}

/// `p`, canonical, if it lies under a configured folder of local source `src`.
pub fn under_roots(st: &State, src: SourceId, p: &Path) -> Option<PathBuf> {
    let canon = std::fs::canonicalize(p).ok()?;
    let ups = st.upstreams();
    let UpKind::Local { roots } = &ups.iter().find(|u| u.id == src)?.kind else {
        return None;
    };
    roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|r| canon.starts_with(r))
        .then_some(canon)
}

async fn serve_local(st: &State, src: SourceId, member: &str, headers: &HeaderMap) -> Response {
    let Some(path) = local_path(st, src, member).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    serve_file(&path, headers, true).await
}

/// A file with Range support, opened read-only.
pub async fn serve_file(path: &Path, headers: &HeaderMap, no_store: bool) -> Response {
    let Ok(mut f) = tokio::fs::File::open(path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(meta) = f.metadata().await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !meta.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let len = meta.len();
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    // We send no validator, so a conditional range cannot be honoured: serve the whole file.
    let span = if headers.contains_key(header::IF_RANGE) {
        Span::Full
    } else {
        parse_range(range, len)
    };
    let (status, start, n) = match span {
        Span::Full => (StatusCode::OK, 0, len),
        Span::Part(a, b) => (StatusCode::PARTIAL_CONTENT, a, b - a + 1),
        Span::Unsatisfiable => {
            let mut r = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            r.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{len}")).expect("ascii"),
            );
            return r;
        }
    };
    if start > 0 && f.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let stream = futures_util::stream::unfold((f, n), |(mut f, rem)| async move {
        if rem == 0 {
            return None;
        }
        let mut buf = vec![0u8; rem.min(CHUNK) as usize];
        match f.read(&mut buf).await {
            Ok(0) | Err(_) => None,
            Ok(k) => {
                buf.truncate(k);
                Some((
                    Ok::<_, std::io::Error>(Bytes::from(buf)),
                    (f, rem - k as u64),
                ))
            }
        }
    });
    let mut r = Response::new(Body::from_stream(stream));
    *r.status_mut() = status;
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(path)),
    );
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(n));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if status == StatusCode::PARTIAL_CONTENT {
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{}/{len}", start + n - 1))
                .expect("ascii"),
        );
    }
    if no_store {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    r
}

pub fn routes(r: Routes) -> Routes {
    let m = || get(media);
    r.add_stream("/Videos/{itemId}/stream", Access::Auth, m())
        .add_stream("/Videos/{itemId}/stream.{container}", Access::Auth, m())
        .add_stream("/Audio/{itemId}/stream", Access::Auth, m())
        .add_stream("/Audio/{itemId}/stream.{container}", Access::Auth, m())
        .add_stream("/Audio/{itemId}/universal", Access::Auth, m())
        .add_stream("/Videos/{itemId}/master.m3u8", Access::Auth, m())
        .add_stream("/Videos/{itemId}/main.m3u8", Access::Auth, m())
        .add_stream("/Videos/{itemId}/live.m3u8", Access::Auth, m())
        .add_stream("/Audio/{itemId}/master.m3u8", Access::Auth, m())
        .add_stream("/Audio/{itemId}/main.m3u8", Access::Auth, m())
        .add_stream("/Videos/{itemId}/hls1/{playlistId}/{segment}", Access::Auth, m())
        .add_stream("/Videos/{itemId}/hls/{playlistId}/{segment}", Access::Auth, m())
        .add_stream("/Audio/{itemId}/hls1/{playlistId}/{segment}", Access::Auth, m())
        .add_stream("/Videos/{itemId}/{mediaSourceId}/Subtitles/{index}/Stream.{format}", Access::Auth, m())
        .add_stream("/Videos/{itemId}/{mediaSourceId}/Subtitles/{index}/{startPositionTicks}/Stream.{format}", Access::Auth, m())
        .add_stream("/Videos/{itemId}/{mediaSourceId}/Subtitles/{index}/subtitles.m3u8", Access::Auth, m())
        .add_stream("/Videos/{itemId}/{mediaSourceId}/Attachments/{index}", Access::Auth, m())
        .add_stream("/Videos/{itemId}/Trickplay/{width}/{file}", Access::Auth, m())
        .add_stream("/MediaSegments/{itemId}", Access::Auth, m())
        .add_stream("/Items/{itemId}/Download", Access::Auth, m())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let r = |h: &str| parse_range(Some(h), 1000);
        assert_eq!(r("bytes=0-99"), Span::Part(0, 99));
        assert_eq!(r("bytes=900-"), Span::Part(900, 999));
        assert_eq!(r("bytes=-100"), Span::Part(900, 999));
        assert_eq!(
            r("bytes=990-5000"),
            Span::Part(990, 999),
            "end clamped to the file"
        );
        assert_eq!(r("bytes=5000-"), Span::Unsatisfiable);
        assert_eq!(r("bytes=-0"), Span::Unsatisfiable);
        assert_eq!(r("bytes=-5000"), Span::Part(0, 999));
        assert_eq!(r("bytes=0-1,5-9"), Span::Full, "several ranges: whole file");
        assert_eq!(r("bytes=9-5"), Span::Full, "inverted: ignored");
        assert_eq!(r("items=0-5"), Span::Full);
        assert_eq!(r("bytes=abc"), Span::Full);
        assert_eq!(parse_range(None, 10), Span::Full);
        assert_eq!(parse_range(Some("bytes=0-1"), 0), Span::Full);
    }

    #[test]
    fn playlist_detection_and_types() {
        assert!(is_playlist("/Videos/x/main.M3U8", ""));
        assert!(is_playlist("/x", "application/vnd.apple.mpegurl"));
        assert!(!is_playlist("/Videos/x/stream", "video/mp4"));
        assert_eq!(content_type(Path::new("a/B.MKV")), "video/x-matroska");
        assert_eq!(content_type(Path::new("a/b")), "application/octet-stream");
    }

    #[tokio::test]
    async fn serves_a_file_with_range_and_refuses_a_directory() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.mp4");
        std::fs::write(&p, (0u8..=255).cycle().take(1000).collect::<Vec<u8>>()).unwrap();
        let mut h = HeaderMap::new();
        h.insert(header::RANGE, "bytes=10-19".parse().unwrap());
        let r = serve_file(&p, &h, true).await;
        assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(r.headers()[header::CONTENT_RANGE], "bytes 10-19/1000");
        assert_eq!(r.headers()[header::CONTENT_LENGTH], "10");
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert_eq!(&b[..], &(10u8..20).collect::<Vec<u8>>()[..]);
        let full = serve_file(&p, &HeaderMap::new(), true).await;
        assert_eq!(full.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(full.into_body(), 1 << 20)
                .await
                .unwrap()
                .len(),
            1000
        );
        h.insert(header::RANGE, "bytes=5000-".parse().unwrap());
        assert_eq!(
            serve_file(&p, &h, true).await.status(),
            StatusCode::RANGE_NOT_SATISFIABLE
        );
        assert_eq!(
            serve_file(d.path(), &HeaderMap::new(), true).await.status(),
            StatusCode::NOT_FOUND
        );
    }
}
