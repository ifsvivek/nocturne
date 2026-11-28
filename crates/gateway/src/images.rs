//! Images (posters, backdrops, logos, avatars): passed through from the item's own server, or read from the local library's art.
//! With Settings > Gateway > Caching on, small answers are kept in a size-capped LRU under the gateway's own data directory (`imgcache/`, `nocturne_core::imgcache`); nothing else is ever cached to disk, and never video.
use crate::items::{enc, servers};
use crate::lists::{Query, encode_query};
use crate::local_dto;
use crate::server::{Access, Routes};
use crate::state::State;
use axum::body::Bytes;
use axum::extract::State as St;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use nocturne_core::imgcache::DiskCache;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_CACHED: usize = 8 << 20;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct Cache(Mutex<Option<(u32, Arc<DiskCache>)>>);

/// The disk cache at the configured size, or `None` when caching is off.
fn cache(st: &State) -> Option<Arc<DiskCache>> {
    let mb = st.cfg().gw.cache_mb;
    let c = st.ext::<Cache>();
    let mut g = c.0.lock().unwrap();
    if mb == 0 {
        *g = None;
        return None;
    }
    if g.as_ref().is_none_or(|(m, _)| *m != mb) {
        *g = Some((
            mb,
            Arc::new(DiskCache::open(
                st.paths.data.join("imgcache"),
                u64::from(mb) << 20,
            )),
        ));
    }
    g.as_ref().map(|(_, c)| c.clone())
}

/// A cached answer is `content-type`, a newline, the bytes.
fn pack(ctype: &str, body: &[u8]) -> Vec<u8> {
    let mut v = ctype.as_bytes().to_vec();
    v.push(b'\n');
    v.extend_from_slice(body);
    v
}

fn unpack(b: Vec<u8>) -> Option<(String, Vec<u8>)> {
    let i = b.iter().position(|c| *c == b'\n')?;
    Some((
        String::from_utf8(b[..i].to_vec()).ok()?,
        b[i + 1..].to_vec(),
    ))
}

fn image_resp(ctype: &str, body: Vec<u8>, cache_control: &str) -> Response {
    let mut r = Response::new(axum::body::Body::from(Bytes::from(body)));
    let h = r.headers_mut();
    if let Ok(v) = HeaderValue::from_str(ctype) {
        h.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = HeaderValue::from_str(cache_control) {
        h.insert(header::CACHE_CONTROL, v);
    }
    r
}

/// Query pairs for the server: the client's, without its token.
fn pairs_of(q: &Query) -> Vec<(String, String)> {
    q.0.iter()
        .filter(|(k, _)| !matches!(k.to_ascii_lowercase().as_str(), "apikey" | "api_key"))
        .cloned()
        .collect()
}

async fn from_server(
    st: &State,
    headers: &HeaderMap,
    s: &crate::items::Srv,
    path: &str,
    q: &Query,
) -> Option<Response> {
    let pairs = pairs_of(q);
    let pq = if pairs.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", encode_query(&pairs))
    };
    let ckey = format!("{}|{pq}", s.key);
    let disk = cache(st);
    if let Some((ct, body)) = disk.as_ref().and_then(|d| d.get(&ckey)).and_then(unpack) {
        return Some(image_resp(&ct, body, "private, max-age=3600"));
    }
    let mut fwd = vec![("accept", "image/*")];
    if let Some(v) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        fwd.push(("if-none-match", v));
    }
    let resp = tokio::time::timeout(TIMEOUT, s.raw.get(&pq, &fwd))
        .await
        .ok()?
        .ok()?;
    match resp.status() {
        StatusCode::NOT_MODIFIED => Some(StatusCode::NOT_MODIFIED.into_response()),
        StatusCode::UNAUTHORIZED => {
            st.mark_signed_out(s.id);
            None
        }
        s if s.is_success() => {
            let ct = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("image/jpeg")
                .to_string();
            let cc = resp
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("private, max-age=3600")
                .to_string();
            let body = tokio::time::timeout(TIMEOUT, resp.bytes())
                .await
                .ok()?
                .ok()?;
            if let Some(d) = disk
                && body.len() <= MAX_CACHED
            {
                d.put(&ckey, &pack(&ct, &body));
            }
            Some(image_resp(&ct, body.to_vec(), &cc))
        }
        _ => None,
    }
}

/// `/Items/{id}/Images/{type}[/...]` and the by-name forms: the first source that has the image answers.
async fn image(St(st): St<State>, headers: HeaderMap, uri: Uri) -> Response {
    let segs: Vec<&str> = uri.path().split('/').collect();
    let q = Query::parse(uri.query());
    let (Some(first), Some(name)) = (segs.get(1).copied(), segs.get(2).copied()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let rest = segs.get(4..).unwrap_or(&[]).join("/");
    let all = servers(&st);
    if first.eq_ignore_ascii_case("Items") {
        let Some((_, members)) = st.ids.resolve_str(name) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let mut order: Vec<(usize, crate::idmap::Member)> = members
            .into_iter()
            .filter_map(|m| {
                let idx = if m.source == local_dto::SRC {
                    let t = local_dto::parse_member(&m.id)?;
                    let s = local_dto::src_of(&t);
                    st.upstreams().iter().position(|u| u.id == s)?
                } else {
                    all.iter().find(|s| s.key == m.source)?.idx
                };
                Some((idx, m))
            })
            .collect();
        order.sort_by_key(|(i, _)| *i);
        for (_, m) in order {
            if m.source == local_dto::SRC {
                let Some(src) = local_dto::parse_member(&m.id).map(|t| local_dto::src_of(&t))
                else {
                    continue;
                };
                // rest is "Primary", "Primary/0", "Backdrop/0/tag/...": kind first, then an optional index.
                let mut parts = rest.split('/');
                let kind = parts.next().unwrap_or("");
                let index = parts.next().and_then(|i| i.parse().ok()).unwrap_or(0);
                let (st2, id, k) = (st.clone(), m.id.clone(), kind.to_string());
                let p = tokio::task::spawn_blocking(move || {
                    local_dto::with_lib(&st2, src, |lib| local_dto::image_path(lib, &id, &k, index))
                })
                .await
                .ok()
                .flatten()
                .flatten();
                if let Some(p) = p
                    && let Some(p) = crate::stream::under_roots(&st, src, &p)
                {
                    return crate::stream::serve_file(&p, &headers, false).await;
                }
            } else if let Some(s) = all.iter().find(|s| s.key == m.source) {
                let path = format!("/Items/{}/{}", enc(&m.id), prefix(&rest));
                if let Some(r) = from_server(&st, &headers, s, &path, &q).await {
                    return r;
                }
            }
        }
        return StatusCode::NOT_FOUND.into_response();
    }
    if matches!(
        first.to_ascii_lowercase().as_str(),
        "artists" | "genres" | "persons" | "studios" | "musicgenres"
    ) {
        for s in &all {
            let path = format!("/{first}/{name}/{}", prefix(&rest));
            if let Some(r) = from_server(&st, &headers, s, &path, &q).await {
                return r;
            }
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

/// `Images/Primary/0/...`: the route after the id keeps its literal `Images` segment, which `rest` (segments from the type on) lacks.
fn prefix(rest: &str) -> String {
    format!("Images/{rest}")
}

/// Public like on a real server: jellyfin-web and the apps load pictures with plain `<img>` links that carry no token. Item ids are the gateway's own unguessable ids, and only pictures are served.
pub fn routes(r: Routes) -> Routes {
    let img = || get(image);
    r.add_stream("/Items/{itemId}/Images/{imageType}", Access::Public, img())
        .add_stream("/Items/{itemId}/Images/{imageType}/{imageIndex}", Access::Public, img())
        .add_stream("/Items/{itemId}/Images/{imageType}/{imageIndex}/{tag}/{format}/{maxWidth}/{maxHeight}/{percentPlayed}/{unPlayedCount}", Access::Public, img())
        .add_stream("/Artists/{name}/Images/{imageType}/{imageIndex}", Access::Public, img())
        .add_stream("/Genres/{name}/Images/{imageType}/{imageIndex}", Access::Public, img())
        .add_stream("/Persons/{name}/Images/{imageType}/{imageIndex}", Access::Public, img())
        .add_stream("/Studios/{name}/Images/{imageType}/{imageIndex}", Access::Public, img())
        .add_stream("/Artists/{name}/Images/{imageType}", Access::Public, img())
        .add_stream("/Genres/{name}/Images/{imageType}", Access::Public, img())
        .add_stream("/Persons/{name}/Images/{imageType}", Access::Public, img())
        .add_stream("/Studios/{name}/Images/{imageType}", Access::Public, img())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_entries_round_trip() {
        let (ct, b) = unpack(pack("image/png", &[1, 2, 10, 3])).unwrap();
        assert_eq!((ct.as_str(), b), ("image/png", vec![1, 2, 10, 3]));
        assert!(unpack(vec![1, 2, 3]).is_none());
    }

    #[test]
    fn query_loses_the_token() {
        let q = Query::parse(Some("maxWidth=300&ApiKey=SECRET&tag=abc&api_key=S2"));
        let p = encode_query(&pairs_of(&q));
        assert_eq!(p, "maxWidth=300&tag=abc");
    }
}
