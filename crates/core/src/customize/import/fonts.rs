//! `@font-face` support for imported themes, the pure part: reading the descriptors, choosing the
//! one file to load per face, decoding WOFF and WOFF2 to TTF/OTF and checking that the result is a
//! font. No I/O; `import.rs` fetches. Fonts are only ever parsed by `wuff` and `ttf-parser` (safe
//! Rust) inside `catch_unwind`, never executed or handed to anything else unchecked.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

/// One font file as downloaded.
pub const MAX_FILE: usize = 2 * 1024 * 1024;
/// All font files of one theme.
pub const MAX_TOTAL: usize = 8 * 1024 * 1024;
pub const MAX_FILES: usize = 24;
/// A decoded font (a WOFF2 can be several times its download size).
pub const MAX_DECODED: usize = 8 * 1024 * 1024;
/// Descriptors read per theme; the rest are ignored with one note.
pub(super) const MAX_FACES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// A font ready to register: TTF/OTF bytes (WOFF and WOFF2 are already decoded) and the
/// `@font-face` descriptors it was declared with.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedFont {
    /// `font-family` as the stylesheet names it, unquoted.
    pub family: String,
    /// `font-weight`; a variable font's range is `weight..=weight_max` (equal for a single weight).
    pub weight: u16,
    pub weight_max: u16,
    pub style: FontStyle,
    /// The family name inside the font file, which is what a text engine looks fonts up by. It can
    /// differ from `family`, so register the font under this name (and map `family` to it).
    pub internal_family: Option<String>,
    pub url: String,
    pub bytes: Arc<[u8]>,
}

#[derive(Clone, Debug)]
pub(super) struct Src {
    /// Absolute address or a `data:` URI.
    pub url: String,
    pub format: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct Face {
    pub family: String,
    pub weight: u16,
    pub weight_max: u16,
    pub style: FontStyle,
    pub srcs: Vec<Src>,
    pub unicode_range: Option<String>,
    pub anchor: (u32, u32),
}

impl Face {
    fn key(&self) -> (String, u16, u16, FontStyle) {
        (
            self.family.to_lowercase(),
            self.weight,
            self.weight_max,
            self.style,
        )
    }
}

/// Split at top-level `sep` (outside quotes and parentheses).
pub(super) fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (vec![], 0i32, None::<char>, 0usize);
    let mut esc = false;
    for (i, c) in s.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match (c, quote) {
            ('\\', _) => esc = true,
            (q, Some(open)) if q == open => quote = None,
            (_, Some(_)) => {}
            ('"' | '\'', None) => quote = Some(c),
            ('(', None) => depth += 1,
            (')', None) => depth = (depth - 1).max(0),
            (c, None) if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(r) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return r.trim();
        }
    }
    s
}

/// The text inside `name( ... )` at the start of `s` (case-insensitive), and what follows it.
fn func<'a>(s: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let s = s.trim_start();
    let head = s.get(..name.len() + 1)?;
    if !head[..name.len()].eq_ignore_ascii_case(name) || !head.ends_with('(') {
        return None;
    }
    let body = &s[name.len() + 1..];
    let (mut quote, mut esc) = (None::<char>, false);
    for (i, c) in body.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match (c, quote) {
            ('\\', _) => esc = true,
            (q, Some(open)) if q == open => quote = None,
            (_, Some(_)) => {}
            ('"' | '\'', None) => quote = Some(c),
            (')', None) => return Some((&body[..i], &body[i + 1..])),
            _ => {}
        }
    }
    None
}

fn weight(v: &str) -> Option<(u16, u16)> {
    let nums: Vec<u16> = v
        .split_whitespace()
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "normal" | "lighter" => Some(400),
            "bold" | "bolder" => Some(700),
            n => n
                .parse::<f32>()
                .ok()
                .map(|f| f.round().clamp(1.0, 1000.0) as u16),
        })
        .collect::<Option<_>>()?;
    match nums[..] {
        [a] => Some((a, a)),
        [a, b] => Some((a.min(b), a.max(b))),
        _ => None,
    }
}

/// Read the declarations of one `@font-face` block (the text between the braces). `resolve` turns
/// a `url()` argument into an absolute address (or `None` when it cannot be).
pub(super) fn parse_face(
    body: &str,
    resolve: &dyn Fn(&str) -> Option<String>,
    anchor: (u32, u32),
) -> Result<Face, String> {
    let (mut family, mut w, mut style, mut srcs, mut range) =
        (None, (400, 400), FontStyle::Normal, vec![], None);
    for d in split_top(body, ';') {
        let Some((name, value)) = d.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "font-family" => {
                // The first name only; a font-face names exactly one family.
                family =
                    Some(unquote(split_top(value, ',')[0]).to_string()).filter(|f| !f.is_empty())
            }
            "font-weight" => w = weight(value).unwrap_or((400, 400)),
            "font-style" => {
                let t = value
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                style = match t.as_str() {
                    "italic" => FontStyle::Italic,
                    "oblique" => FontStyle::Oblique,
                    _ => FontStyle::Normal,
                }
            }
            "unicode-range" => range = Some(value.to_string()),
            "src" => {
                for entry in split_top(value, ',') {
                    let Some((arg, rest)) = func(entry, "url") else {
                        continue; // local(...) and anything else: never read from the machine
                    };
                    let Some(url) = resolve(unquote(arg)) else {
                        continue;
                    };
                    let format = func(rest, "format").map(|(f, _)| unquote(f).to_ascii_lowercase());
                    srcs.push(Src { url, format });
                }
            }
            _ => {}
        }
    }
    let family = family.ok_or("the @font-face has no font-family")?;
    if srcs.is_empty() {
        return Err(format!("font `{family}`: no usable url() in src"));
    }
    Ok(Face {
        family,
        weight: w.0,
        weight_max: w.1,
        style,
        srcs,
        unicode_range: range,
        anchor,
    })
}

/// Try order of a source: TTF/OTF first (no decoding), then WOFF2, then WOFF; `None` for formats
/// that are never loaded (EOT, SVG fonts). No hint: judged by the extension, else tried last and
/// identified by its bytes.
fn rank(s: &Src) -> Option<u8> {
    if s.url.starts_with("data:") && s.format.is_none() {
        return Some(3);
    }
    if let Some(f) = &s.format {
        return match f.as_str() {
            "truetype"
            | "opentype"
            | "ttf"
            | "otf"
            | "truetype-variations"
            | "opentype-variations"
            | "collection" => Some(0),
            "woff2" | "woff2-variations" => Some(1),
            "woff" | "woff-variations" => Some(2),
            _ => None,
        };
    }
    let path = s
        .url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    Some(match path.rsplit('.').next() {
        Some("ttf" | "otf") => 0,
        Some("woff2") => 1,
        Some("woff") => 2,
        Some("eot" | "svg") => return None,
        _ => 3,
    })
}

/// The sources of a face in the order to try them.
pub(super) fn ordered(face: &Face) -> Vec<&Src> {
    let mut v: Vec<(u8, &Src)> = face
        .srcs
        .iter()
        .filter_map(|s| rank(s).map(|r| (r, s)))
        .collect();
    v.sort_by_key(|(r, _)| *r);
    v.into_iter().map(|(_, s)| s).collect()
}

/// Does a `unicode-range` list cover `lo..=hi` entirely in one range?
fn covers(range: &str, lo_need: u32, hi_need: u32) -> bool {
    range.split(',').any(|r| {
        let r = r.trim().to_ascii_uppercase();
        let Some(r) = r.strip_prefix("U+") else {
            return false;
        };
        let (lo, hi) = match r.split_once('-') {
            Some((a, b)) => (u32::from_str_radix(a, 16), u32::from_str_radix(b, 16)),
            None => (u32::from_str_radix(r, 16), u32::from_str_radix(r, 16)),
        };
        matches!((lo, hi), (Ok(lo), Ok(hi)) if lo <= lo_need && hi >= hi_need)
    })
}

/// Does a `unicode-range` list cover basic Latin letters?
fn latin(range: &str) -> bool {
    covers(range, 0x41, 0x7A)
}

/// Does it cover the Latin Extended-A letters (Google's `latin-ext`: U+0100-02BA and more)?
fn latin_ext(range: &str) -> bool {
    covers(range, 0x100, 0x17F)
}

/// Which faces of one (family, weight, style) are worth downloading: an unrestricted one wins over
/// all; else the Latin and Latin Extended `unicode-range` subsets (Google Fonts serves one
/// `@font-face` per script); the other scripts are skipped. Returns the kept faces, Latin first (so
/// a download limit never costs a family its basic letters), and for each family the number of
/// distinct skipped subsets.
pub(super) fn select(faces: Vec<Face>) -> (Vec<Face>, Vec<(String, usize)>) {
    let mut keys: Vec<(String, u16, u16, FontStyle)> = vec![];
    let mut groups: Vec<Vec<Face>> = vec![];
    for f in faces {
        match keys.iter().position(|k| *k == f.key()) {
            Some(i) => groups[i].push(f),
            None => {
                keys.push(f.key());
                groups.push(vec![f]);
            }
        }
    }
    let (mut first, mut ext) = (vec![], vec![]);
    let mut skipped: Vec<(String, Vec<String>)> = vec![];
    for g in groups {
        if let Some(i) = g.iter().position(|f| f.unicode_range.is_none()) {
            first.extend(g.into_iter().nth(i));
            continue;
        }
        let wanted = |f: &Face| {
            f.unicode_range
                .as_deref()
                .is_some_and(|r| latin(r) || latin_ext(r))
        };
        // Nothing Latin at all (a single-script font): keep the first so the family still loads.
        let keep_first = !g.iter().any(wanted);
        for (n, f) in g.into_iter().enumerate() {
            if wanted(&f) || (keep_first && n == 0) {
                if f.unicode_range.as_deref().is_some_and(latin) || keep_first {
                    first.push(f);
                } else {
                    ext.push(f);
                }
            } else {
                let r = f.unicode_range.clone().unwrap_or_default();
                match skipped.iter_mut().find(|(fam, _)| *fam == f.family) {
                    Some((_, rs)) if !rs.contains(&r) => rs.push(r),
                    Some(_) => {}
                    None => skipped.push((f.family.clone(), vec![r])),
                }
            }
        }
    }
    first.extend(ext);
    (
        first,
        skipped.into_iter().map(|(f, r)| (f, r.len())).collect(),
    )
}

/// `data:[mime][;base64],payload` decoded; only base64 payloads (what fonts use).
pub(super) fn data_uri(uri: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let rest = uri.strip_prefix("data:").ok_or("not a data: address")?;
    let (meta, payload) = rest.split_once(',').ok_or("malformed data: address")?;
    if !meta.to_ascii_lowercase().ends_with(";base64") {
        return Err("only base64 data: addresses are supported".into());
    }
    if payload.len() > MAX_FILE * 4 / 3 + 8 {
        return Err(format!("the font is larger than {} MB", MAX_FILE >> 20));
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|_| "malformed base64 data".to_string())
}

/// Check the signature and decode to TTF/OTF/TTC. WOFF and WOFF2 go through `wuff`; the declared
/// decoded size is checked first so a small file cannot expand without bound.
pub(super) fn to_sfnt(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let sig = bytes.get(..4).ok_or("the file is empty")?;
    let declared = || {
        bytes
            .get(16..20)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| "truncated font header".to_string())
    };
    let decoded = |r: std::thread::Result<Result<Vec<u8>, wuff::WuffErr>>| match r {
        Ok(Ok(v)) if v.len() <= MAX_DECODED => Ok(v),
        Ok(Ok(_)) => Err("the decoded font is too large".to_string()),
        Ok(Err(_)) | Err(_) => Err("the web font could not be decoded".to_string()),
    };
    match sig {
        [0, 1, 0, 0] | b"true" | b"OTTO" | b"ttcf" => Ok(bytes.to_vec()),
        b"wOFF" => {
            if declared()? > MAX_DECODED {
                return Err("the decoded font is too large".into());
            }
            decoded(catch_unwind(AssertUnwindSafe(|| {
                wuff::decompress_woff1(bytes)
            })))
        }
        b"wOF2" => {
            if declared()? > MAX_DECODED {
                return Err("the decoded font is too large".into());
            }
            decoded(catch_unwind(AssertUnwindSafe(|| {
                wuff::decompress_woff2(bytes)
            })))
        }
        _ => Err("not a TTF, OTF, WOFF or WOFF2 font (unknown file signature)".into()),
    }
}

/// Parse the font once (safe Rust, panics caught) and return its family name from the name table.
pub(super) fn validate(font: &[u8]) -> Result<Option<String>, String> {
    catch_unwind(AssertUnwindSafe(|| {
        let face = ttf_parser::Face::parse(font, 0)
            .map_err(|_| "the file is not a valid font".to_string())?;
        let name = |id| {
            face.names()
                .into_iter()
                .filter(|n| n.name_id == id && n.is_unicode())
                .find_map(|n| n.to_string())
                .filter(|s| !s.trim().is_empty())
        };
        Ok(name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| name(ttf_parser::name_id::FAMILY)))
    }))
    .unwrap_or_else(|_| Err("the file is not a valid font".into()))
}

/// Content types a font may be served with.
pub fn content_type_ok(ct: &str) -> bool {
    let m = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    m.starts_with("font/")
        || m.starts_with("application/font-")
        || m.starts_with("application/x-font-")
        || m == "application/octet-stream"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(u: &str) -> Option<String> {
        Some(u.to_string())
    }

    #[test]
    fn reads_descriptors_and_orders_sources() {
        let f = parse_face(
            "font-family: 'Open Sans'; font-style: italic; font-weight: 300 700;\
             src: local('Open Sans'), url(a.eot) format('embedded-opentype'), url(\"b.woff\") format(\"woff\"), url(c.woff2) format('woff2'), url(d.ttf)",
            &abs,
            (1, 1),
        )
        .unwrap();
        assert_eq!(
            (f.family.as_str(), f.weight, f.weight_max, f.style),
            ("Open Sans", 300, 700, FontStyle::Italic)
        );
        let o: Vec<&str> = ordered(&f).iter().map(|s| s.url.as_str()).collect();
        assert_eq!(o, ["d.ttf", "c.woff2", "b.woff"]); // eot dropped, ttf first
        assert!(parse_face("src: url(x.ttf)", &abs, (1, 1)).is_err());
        assert!(parse_face("font-family: A; src: local(A)", &abs, (1, 1)).is_err());
    }

    #[test]
    fn weights() {
        assert_eq!(weight("bold"), Some((700, 700)));
        assert_eq!(weight("900 100"), Some((100, 900)));
        assert_eq!(weight("nonsense"), None);
    }

    #[test]
    fn unicode_range_subsets_keep_latin() {
        let mk = |r: Option<&str>, u: &str| Face {
            family: "Inter".into(),
            weight: 400,
            weight_max: 400,
            style: FontStyle::Normal,
            srcs: vec![Src {
                url: u.into(),
                format: None,
            }],
            unicode_range: r.map(Into::into),
            anchor: (1, 1),
        };
        let (k, skipped) = select(vec![
            mk(Some("U+0400-045F, U+0490-0491"), "cyr"),
            mk(Some("U+0100-02BA, U+02BD-02C5"), "ext"),
            mk(Some("U+0000-00FF, U+0131"), "latin"),
            mk(Some("U+0370-03FF"), "greek"),
        ]);
        let urls: Vec<_> = k.iter().map(|f| f.srcs[0].url.as_str()).collect();
        assert_eq!(urls, ["latin", "ext"], "Latin first");
        assert_eq!(skipped, [("Inter".to_string(), 2)]);
        let (k, _) = select(vec![mk(Some("U+0400-045F"), "cyr"), mk(None, "all")]);
        assert_eq!(k[0].srcs[0].url, "all");
    }

    #[test]
    fn signatures_and_data_uris() {
        assert!(to_sfnt(b"").is_err());
        assert!(to_sfnt(b"<html>").is_err());
        assert!(to_sfnt(b"%PDF-1.7 ....").unwrap_err().contains("signature"));
        assert_eq!(to_sfnt(&[0, 1, 0, 0, 9]).unwrap(), [0, 1, 0, 0, 9]);
        // a WOFF header claiming a huge decoded size is refused before decoding
        let mut w = b"wOFF".to_vec();
        w.resize(44, 0);
        w[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(to_sfnt(&w).unwrap_err().contains("too large"));
        assert_eq!(
            data_uri("data:font/ttf;base64,AAEAAA==").unwrap(),
            [0, 1, 0, 0]
        );
        assert!(data_uri("data:font/ttf,abc").is_err());
        assert!(validate(&[0, 1, 0, 0, 0, 0]).is_err()); // a signature alone is not a font
    }

    #[test]
    fn content_types() {
        for ok in [
            "font/woff2",
            "font/ttf",
            "application/font-woff",
            "application/x-font-ttf",
            "application/octet-stream",
            "Font/OTF; x=y",
        ] {
            assert!(content_type_ok(ok), "{ok}");
        }
        for bad in ["text/html", "image/png", "", "application/json"] {
            assert!(!content_type_ok(bad), "{bad}");
        }
    }
}
