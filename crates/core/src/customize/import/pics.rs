//! Pictures a stylesheet refers to with `url()` (logos, tiles, textures, icons), fetched when the engine paints them. Same rules as the rest of this module for anything that is not the configured server's own: https only, public hosts only, no credentials or `Referer`, a fixed User-Agent, 10 s, 3 redirects (each checked again), 4 MB, an image content type AND the first bytes of a PNG, JPEG, WebP or SVG, a disk cache by address, and only with `remote_css` on. The configured server's own files (`origin`, any scheme and host, for the `/web/` assets of its client) are the exception to the https and public-host rules: they are what the user pointed the app at. `data:` addresses are decoded in place. Nothing is executed; the app decodes the bytes.
use super::{AGENT, Url, cache_read, cache_write, check_url, default_cache_dir, describe, norm};
use std::time::Duration;

pub const MAX_FILE: usize = 4 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub bytes: Vec<u8>,
    pub svg: bool,
}

/// PNG, JPEG, WebP (raster) or SVG by the first bytes; `None` for anything else (GIF and HTML included).
pub fn sniff(b: &[u8]) -> Option<bool> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") || b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(false);
    }
    if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return Some(false);
    }
    let head = String::from_utf8_lossy(&b[..b.len().min(1024)]).to_ascii_lowercase();
    let t = head.trim_start_matches('\u{feff}').trim_start();
    (t.starts_with("<svg")
        || ((t.starts_with("<?xml") || t.starts_with("<!--")) && t.contains("<svg")))
    .then_some(true)
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// `data:image/...;base64,...` or percent-encoded text (the usual inline SVG).
pub fn data_uri(uri: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let rest = uri.strip_prefix("data:").ok_or("not a data: address")?;
    let (meta, payload) = rest.split_once(',').ok_or("malformed data: address")?;
    if payload.len() > MAX_FILE * 4 / 3 + 8 {
        return Err("larger than 4 MB".into());
    }
    if meta.to_ascii_lowercase().ends_with(";base64") {
        base64::engine::general_purpose::STANDARD
            .decode(percent_decode(payload.trim()))
            .map_err(|_| "malformed base64 data".to_string())
    } else {
        Ok(percent_decode(payload))
    }
}

/// `raw` resolved against `base` (both as text).
pub fn join_url(base: &str, raw: &str) -> Option<String> {
    Some(Url::parse(base).ok()?.join(raw).ok()?.to_string())
}

fn same_origin(u: &Url, origin: Option<&str>) -> bool {
    origin.and_then(|o| Url::parse(o).ok()).is_some_and(|o| {
        o.scheme() == u.scheme()
            && o.host_str() == u.host_str()
            && o.port_or_known_default() == u.port_or_known_default()
    })
}

/// The picture at `url` (already absolute, or a `data:` address). `origin`: the configured server's address.
pub async fn fetch(url: &str, origin: Option<&str>, remote: bool) -> Result<Picture, String> {
    let done = |bytes: Vec<u8>| -> Result<Picture, String> {
        let svg = sniff(&bytes).ok_or("not a PNG, JPEG, WebP or SVG picture")?;
        Ok(Picture { bytes, svg })
    };
    if url.starts_with("data:") {
        return done(data_uri(url)?);
    }
    let u = Url::parse(url).map_err(|_| "not an absolute address".to_string())?;
    let own = same_origin(&u, origin);
    if !own {
        if !remote {
            return Err("pictures of other sites are fetched only with remote styles on".into());
        }
        check_url(&u, false)?;
    } else if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() {
        return Err("only http(s) addresses are fetched".into());
    }
    let key = norm(&u);
    let dir = default_cache_dir().map(|d| d.with_file_name("cssimg"));
    if let Some(c) = dir.as_deref().and_then(|d| cache_read(d, &key))
        && c.bytes.len() <= MAX_FILE
    {
        return done(c.bytes);
    }
    let policy = {
        let host = u.host_str().map(str::to_string);
        reqwest::redirect::Policy::custom(move |a| {
            let ok = a.previous().len() <= 3
                && if own {
                    a.url().host_str().map(str::to_string) == host
                } else {
                    check_url(a.url(), false).is_ok()
                };
            if ok { a.follow() } else { a.stop() }
        })
    };
    let client = reqwest::Client::builder()
        .user_agent(AGENT)
        .referer(false)
        .redirect(policy)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "no http client".to_string())?;
    let get = async {
        let mut r = client
            .get(u.clone())
            .header(reqwest::header::ACCEPT, "image/*")
            .send()
            .await
            .map_err(|e| describe(&e).to_string())?;
        if !r.status().is_success() {
            return Err(format!("the server answered {}", r.status().as_u16()));
        }
        let ct = r
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !(ct.starts_with("image/") || ct.starts_with("application/octet-stream")) {
            return Err("not an image content type".into());
        }
        if r.content_length().is_some_and(|n| n as usize > MAX_FILE) {
            return Err("larger than 4 MB".into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = r.chunk().await.map_err(|e| describe(&e).to_string())? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_FILE {
                return Err("larger than 4 MB".into());
            }
        }
        Ok(body)
    };
    let body = tokio::time::timeout(Duration::from_secs(10), get)
        .await
        .unwrap_or_else(|_| Err("timed out".into()))?;
    let pic = done(body)?;
    if let Some(d) = &dir {
        cache_write(d, &key, &key, &pic.bytes);
    }
    Ok(pic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_and_decodes() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nxxxx"), Some(false));
        assert_eq!(sniff(b"  <?xml version='1.0'?><svg/>"), Some(true));
        assert_eq!(sniff(b"<html><svg>"), None);
        assert_eq!(sniff(b"GIF89a"), None);
        let d = data_uri("data:image/svg+xml,%3Csvg viewBox='0 0 1 1'/%3E").unwrap();
        assert_eq!(sniff(&d), Some(true));
        assert!(data_uri("data:image/png;base64,iVBORw0KGgo=").is_ok());
    }

    #[test]
    fn relative_pictures_become_absolute() {
        let base = Url::parse("https://cdn.example/gh/x/Themes/Solaris/Solaris.css").unwrap();
        let mut out = String::new();
        let css: Vec<char> = "a{background:url(\"../../assets/m.svg\") no-repeat} b{fill:url(#g);c:url(data:image/png;base64,AA==);d:url(https://h/p.png)}".chars().collect();
        super::super::push_abs(&mut out, &css, Some(&base));
        assert!(out.contains("url(\"https://cdn.example/gh/x/assets/m.svg\") no-repeat"));
        assert!(out.contains("url(#g)") && out.contains("url(data:image/png;base64,AA==)"));
        assert!(out.contains("url(https://h/p.png)"));
    }

    #[test]
    fn other_sites_need_the_setting_and_https() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = |u: &str, remote: bool| {
            rt.block_on(fetch(u, Some("http://192.168.1.5:8096"), remote))
                .unwrap_err()
        };
        assert!(err("https://example.com/a.png", false).contains("remote styles"));
        assert!(err("http://example.com/a.png", true).contains("https"));
        assert!(err("https://127.0.0.1/a.png", true).contains("private"));
    }
}
