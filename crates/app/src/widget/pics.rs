//! The pictures stylesheets refer to with `url()` (background images): a process-wide store the engine's widget reads while it draws, filled by `nocturne_core::customize::import::fetch_picture` through the app (`app/pics.rs`). The scenes only name addresses (`Computed::pic_url`); an address is resolved against the server whose UI is shown (`/web/` of its client for relative ones), fetched once, decoded off the UI thread, and kept while the same server's UI stays in use. Caps: 96 addresses and 96 MB of pixels per server.
use iced::advanced::svg;
use iced::widget::image::Handle;
use nocturne_style::Scene;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Debug)]
pub enum Data {
    Raster(Handle),
    Vector(svg::Handle),
}

#[derive(Clone, Debug)]
pub struct Pic {
    pub data: Data,
    /// Intrinsic size of a raster picture (a vector one is measured by the renderer).
    pub w: u32,
    pub h: u32,
    bytes: usize,
}

#[derive(Default)]
struct Store {
    /// `(origin, web base)` of the server whose pictures these are.
    ctx: Option<(String, String)>,
    ready: HashMap<String, Option<Arc<Pic>>>,
    asked: HashSet<String>,
    want: Vec<String>,
    bytes: usize,
}

const MAX_URLS: usize = 96;
const MAX_BYTES: usize = 96 << 20;

fn store() -> std::sync::MutexGuard<'static, Store> {
    static S: OnceLock<Mutex<Store>> = OnceLock::new();
    S.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Which server the relative addresses belong to (`None`: only absolute and `data:` ones). Another server drops what was loaded.
pub fn set_context(server: Option<&str>) {
    let ctx = server.map(|s| {
        let o = s.trim_end_matches('/').to_string();
        (o.clone(), format!("{o}/web/"))
    });
    let mut st = store();
    if st.ctx != ctx {
        *st = Store {
            ctx,
            ..Store::default()
        };
    }
}

/// The picture for an address as written in the stylesheet, when it has been loaded.
pub fn get(raw: &str) -> Option<Arc<Pic>> {
    store().ready.get(raw)?.clone()
}

/// Note the addresses the scene refers to (new ones are fetched by the app at its next update).
pub fn want_scene(s: &Scene) {
    let mut st = store();
    for n in 0..s.doc.len() {
        let Some(u) = &s.style(n).pic_url else {
            continue;
        };
        if !st.asked.contains(&**u) && st.asked.len() < MAX_URLS {
            st.asked.insert(u.to_string());
            st.want.push(u.to_string());
        }
    }
}

/// `(raw, absolute, origin)` of what to fetch now.
pub fn take_wanted() -> Vec<(String, String, Option<String>)> {
    let mut st = store();
    let ctx = st.ctx.clone();
    std::mem::take(&mut st.want)
        .into_iter()
        .filter_map(|raw| {
            let abs = if raw.starts_with("data:") {
                raw.clone()
            } else {
                match &ctx {
                    Some((_, base)) => nocturne_core::customize::import::join_url(base, &raw)?,
                    None if raw.contains("://") => raw.clone(),
                    None => return None,
                }
            };
            Some((raw, abs, ctx.as_ref().map(|c| c.0.clone())))
        })
        .collect()
}

/// Keep the decoded picture (or the failure, so that it is not asked again).
pub fn put(raw: String, pic: Option<Arc<Pic>>) {
    if let Some(p) = &pic {
        // Layout sizes an `img` whose picture is its `content` from this.
        nocturne_style::paint::set_pic_dims(&raw, p.w, p.h);
    }
    let mut st = store();
    let add = pic.as_ref().map_or(0, |p| p.bytes);
    if st.bytes + add > MAX_BYTES {
        st.ready.insert(raw, None);
        return;
    }
    st.bytes += add;
    st.ready.insert(raw, pic);
}

/// Decode fetched bytes (off the UI thread): rasters are limited to 2048 px on the long side.
pub fn decode(bytes: &[u8], svg_data: bool) -> Result<Pic, String> {
    if svg_data {
        let (w, h) = svg_size(bytes);
        return Ok(Pic {
            data: Data::Vector(svg::Handle::from_memory(bytes.to_vec())),
            w,
            h,
            bytes: bytes.len() * 4,
        });
    }
    let mut img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    if img.width().max(img.height()) > 2048 {
        img = img.thumbnail(2048, 2048);
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(Pic {
        data: Data::Raster(Handle::from_rgba(w, h, rgba.into_raw())),
        w,
        h,
        bytes: (w * h * 4) as usize,
    })
}

/// The size of an SVG picture from its root tag (`width`/`height`, else `viewBox`); 0 x 0 when it says neither.
fn svg_size(b: &[u8]) -> (u32, u32) {
    let head = String::from_utf8_lossy(&b[..b.len().min(2048)]).to_string();
    let Some(i) = head.to_ascii_lowercase().find("<svg") else {
        return (0, 0);
    };
    let tag = &head[i..head[i..].find('>').map_or(head.len(), |e| i + e)];
    let attr = |name: &str| -> Option<String> {
        let at = tag.to_ascii_lowercase().find(&format!(" {name}="))? + name.len() + 2;
        let q = tag[at..]
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '\''))?;
        let rest = &tag[at + 1..];
        Some(rest[..rest.find(q)?].to_string())
    };
    let num = |s: &str| s.trim().trim_end_matches("px").parse::<f32>().ok();
    if let (Some(w), Some(h)) = (
        attr("width").and_then(|s| num(&s)),
        attr("height").and_then(|s| num(&s)),
    ) {
        return (w.max(1.0) as u32, h.max(1.0) as u32);
    }
    if let Some(vb) = attr("viewbox") {
        let n: Vec<f32> = vb
            .split([' ', ','])
            .filter_map(|t| t.parse().ok())
            .collect();
        if n.len() == 4 {
            return (n[2].max(1.0) as u32, n[3].max(1.0) as u32);
        }
    }
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_sizes_come_from_the_root_tag() {
        assert_eq!(
            svg_size(b"<svg xmlns='x' viewBox='0 0 120 30'><g/></svg>"),
            (120, 30)
        );
        assert_eq!(
            svg_size(
                b"<?xml version='1.0'?><svg width=\"64px\" height=\"16\" viewBox=\"0 0 1 1\">"
            ),
            (64, 16)
        );
        assert_eq!(svg_size(b"<svg>"), (0, 0));
    }
}
