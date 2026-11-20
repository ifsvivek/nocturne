//! Reads of the server's own web client files (`/web/...`, public static files): which stylesheets jellyfin-web loads, and their text. Used by Jellyfin layout mode, which draws jellyfin-web's page structure and needs the stylesheets that go with it. GET only; the files are never stored in the repository, the app caches them per server and web-client build.
use crate::{Client, Error, Result};

/// The stylesheets of one web-client build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebFiles {
    /// The build id (the `?query` of the stylesheet links of `index.html`), a cache key.
    pub build: String,
    /// The stylesheets `index.html` links, in order (paths relative to `/web/`).
    pub linked: Vec<String>,
    /// Every lazily loaded stylesheet of the build as `(chunk name, path)`; unnamed (shared) chunks use their numeric id as the name.
    pub chunks: Vec<(String, String)>,
    /// The text of `index.html`, for [`index_scan`] (plugins add their stylesheets to it).
    pub index: String,
}

/// Files bigger than this are refused (the largest stylesheet of the web client is about 1 MB: an icon font).
const MAX: usize = 4 << 20;

impl Client {
    /// `GET /web/<path>` as text. `Ok(None)` when the server has no web client there (a 404, as with `--nowebclient`).
    pub async fn web_text(&self, path: &str) -> Result<Option<String>> {
        let segs: Vec<&str> = std::iter::once("web")
            .chain(path.split('/').filter(|s| !s.is_empty()))
            .collect();
        let url = self.url(&segs, &[]);
        let p = url.path().to_string();
        match self.send(url, p).await {
            Ok(r) => {
                if r.content_length().is_some_and(|n| n as usize > MAX) {
                    return Err(Error::Config("web file too large".into()));
                }
                let b = r.bytes().await.map_err(crate::client::http_err)?;
                if b.len() > MAX {
                    return Err(Error::Config("web file too large".into()));
                }
                Ok(Some(String::from_utf8_lossy(&b).into_owned()))
            }
            Err(Error::Status { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// `GET /web/<path>` as bytes, at most `max` (a font file). `Ok(None)`: not there, or bigger than `max`.
    pub async fn web_bytes(&self, path: &str, max: usize) -> Result<Option<Vec<u8>>> {
        let segs: Vec<&str> = std::iter::once("web")
            .chain(path.split('/').filter(|s| !s.is_empty()))
            .collect();
        let url = self.url(&segs, &[]);
        let p = url.path().to_string();
        match self.send(url, p).await {
            Ok(r) => {
                if r.content_length().is_some_and(|n| n as usize > max) {
                    return Ok(None);
                }
                let b = r.bytes().await.map_err(crate::client::http_err)?;
                Ok((b.len() <= max).then(|| b.to_vec()))
            }
            Err(Error::Status { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Which stylesheets the server's web client has. `Ok(None)`: no web client.
    pub async fn web_files(&self) -> Result<Option<WebFiles>> {
        let Some(index) = self.web_text("index.html").await? else {
            return Ok(None);
        };
        let linked = index_stylesheets(&index);
        if linked.is_empty() {
            return Ok(None);
        }
        let (build, linked) = own_links(&linked);
        let chunks = match self.web_text("runtime.bundle.js").await? {
            Some(js) => css_chunks(&js),
            None => vec![],
        };
        Ok(Some(WebFiles {
            build,
            linked,
            chunks,
            index,
        }))
    }

    /// The address of the web client's start page, what a browser resolves its relative links against.
    pub fn web_index_url(&self) -> reqwest::Url {
        self.url(&["web", "index.html"], &[])
    }

    /// `GET <url>` as text when it is on this server's own origin (plugins serve their stylesheets from it), at most `max` bytes. `Ok(None)`: another origin, a 404, or too big. Nothing else is fetched through here.
    pub async fn same_origin_text(&self, url: &reqwest::Url, max: usize) -> Result<Option<String>> {
        if url.origin() != self.base.origin() {
            return Ok(None);
        }
        let p = url.path().to_string();
        match self.send(url.clone(), p).await {
            Ok(r) => {
                if r.content_length().is_some_and(|n| n as usize > max) {
                    return Ok(None);
                }
                let b = r.bytes().await.map_err(crate::client::http_err)?;
                Ok((b.len() <= max).then(|| String::from_utf8_lossy(&b).into_owned()))
            }
            Err(Error::Status { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// One stylesheet of a start page, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexSheet {
    /// `<link rel="stylesheet" href>`; `media` is kept only when it narrows the sheet (not `all`/`screen`).
    Link { href: String, media: Option<String> },
    /// `<style>...</style>`.
    Inline { css: String, media: Option<String> },
}

/// What a browser would load from a start page for styling: its stylesheets, and how many scripts it would run (never run here).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexScan {
    pub sheets: Vec<IndexSheet>,
    pub scripts: usize,
}

/// The attributes of a tag's text (lower-case names; quoted, single-quoted, unquoted or bare).
fn attrs(tag: &str) -> Vec<(String, String)> {
    let b = tag.as_bytes();
    let (mut i, mut out) = (0, vec![]);
    while i < b.len() && !b[i].is_ascii_whitespace() {
        i += 1;
    }
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let s = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        let name = tag[s..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let v = i + 1;
                i = tag[v..].find(q as char).map_or(b.len(), |j| v + j);
                val = tag[v..i].to_string();
                i = (i + 1).min(b.len());
            } else {
                let v = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                val = tag[v..i].to_string();
            }
        }
        if !name.is_empty() {
            out.push((name, val));
        }
    }
    out
}

/// The index of the closing tag of the element named `name` whose content starts at `from`, or the end of `html`.
fn close_of(html: &str, lower: &str, from: usize, name: &str) -> usize {
    lower[from..]
        .find(&format!("</{name}"))
        .map_or(html.len(), |j| from + j)
}

/// The stylesheets of a start page as a browser would apply them: `<link>` with a stylesheet `rel` (not `alternate`, not `disabled`, not for `print`) and `<style>` blocks, in document order, plus the number of `<script>`s. Comments are skipped, and so is the inside of scripts (a `<link` in a string is not a tag).
pub fn index_scan(html: &str) -> IndexScan {
    let lower = html.to_ascii_lowercase();
    let mut out = IndexScan::default();
    let mut i = 0;
    while let Some(j) = lower[i..].find('<') {
        let at = i + j;
        if lower[at..].starts_with("<!--") {
            i = lower[at + 4..]
                .find("-->")
                .map_or(html.len(), |k| at + 4 + k + 3);
            continue;
        }
        let name_end = lower[at + 1..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .map_or(html.len(), |k| at + 1 + k);
        let name = &lower[at + 1..name_end];
        // The tag's text ends at the first `>` outside quotes.
        let mut q = None;
        let mut end = html.len();
        for (k, c) in html[name_end..].char_indices() {
            match (q, c) {
                (None, '"' | '\'') => q = Some(c),
                (Some(x), c) if c == x => q = None,
                (None, '>') => {
                    end = name_end + k;
                    break;
                }
                _ => {}
            }
        }
        let tag = &html[at + 1..end];
        let body = (end + 1).min(html.len());
        i = body;
        let get = |a: &[(String, String)], k: &str| {
            a.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
        };
        // Media that narrows the sheet; `print` drops it.
        let media = |a: &[(String, String)]| {
            get(a, "media").map(|m| m.trim().to_string()).filter(|m| {
                !m.is_empty() && !matches!(m.to_ascii_lowercase().as_str(), "all" | "screen")
            })
        };
        let print = |m: &Option<String>| {
            m.as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case("print"))
        };
        match name {
            "link" => {
                let a = attrs(tag);
                let rel = get(&a, "rel").unwrap_or_default().to_ascii_lowercase();
                let rels: Vec<&str> = rel.split_ascii_whitespace().collect();
                let m = media(&a);
                if rels.contains(&"stylesheet")
                    && !rels.contains(&"alternate")
                    && !a.iter().any(|(n, _)| n == "disabled")
                    && !print(&m)
                    && let Some(h) = get(&a, "href").filter(|h| !h.trim().is_empty())
                {
                    out.sheets.push(IndexSheet::Link {
                        href: h.trim().to_string(),
                        media: m,
                    });
                }
            }
            "style" => {
                let a = attrs(tag);
                let e = close_of(html, &lower, body, "style");
                let m = media(&a);
                let ty = get(&a, "type").unwrap_or_default().to_ascii_lowercase();
                if (ty.is_empty() || ty == "text/css")
                    && !a.iter().any(|(n, _)| n == "disabled")
                    && !print(&m)
                    && !html[body..e].trim().is_empty()
                {
                    out.sheets.push(IndexSheet::Inline {
                        css: html[body..e].to_string(),
                        media: m,
                    });
                }
                i = e;
            }
            "script" => {
                out.scripts += 1;
                i = close_of(html, &lower, body, "script");
            }
            _ => {}
        }
    }
    out
}

/// `(build id, paths)` of the links that are the web client's own: the build id is the query most links share, and only relative links with that query count. Links a plugin added to the start page are read by [`index_scan`] instead.
fn own_links(links: &[String]) -> (String, Vec<String>) {
    let mut count: Vec<(&str, usize)> = vec![];
    for q in links.iter().filter_map(|l| l.split_once('?').map(|x| x.1)) {
        match count.iter_mut().find(|c| c.0 == q) {
            Some(c) => c.1 += 1,
            None => count.push((q, 1)),
        }
    }
    let build = count
        .iter()
        .max_by_key(|c| c.1)
        .map(|c| c.0.to_string())
        .unwrap_or_default();
    let own = links
        .iter()
        .filter(|l| {
            !l.starts_with(['/', '.'])
                && !l.contains("://")
                && (build.is_empty() || l.split_once('?').is_some_and(|x| x.1 == build))
        })
        .map(|l| l.split('?').next().unwrap_or("").to_string())
        .collect();
    (build, own)
}

/// The `href` of every `<link rel="stylesheet">` in `html`.
pub fn index_stylesheets(html: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = html;
    while let Some(i) = rest.find("<link") {
        rest = &rest[i + 5..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if tag.contains("rel=\"stylesheet\"")
            && let Some(h) = attr(tag, "href")
        {
            out.push(h.to_string());
        }
        rest = &rest[end..];
    }
    out
}

/// The file of the first `@font-face` for `family` in `css` whose `src` has a `.ttf` (or `.otf`) `url(...)`, relative to `/web/`. The text shaper reads those, not woff.
pub fn font_face_file(css: &str, family: &str) -> Option<String> {
    let mut rest = css;
    while let Some(i) = rest.find("@font-face") {
        rest = &rest[i + 10..];
        let end = rest.find('}').unwrap_or(rest.len());
        let body = &rest[..end];
        let fam = body.split("font-family:").nth(1).map(|f| {
            f.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\''])
        });
        if fam != Some(family) {
            continue;
        }
        for u in body.split("url(").skip(1) {
            let f = u
                .split(')')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\'']);
            let f = f.split(['?', '#']).next().unwrap_or("");
            if f.ends_with(".ttf") || f.ends_with(".otf") {
                return Some(f.to_string());
            }
        }
    }
    None
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')?;
    Some(&tag[i..i + j])
}

/// `(name, path)` of every stylesheet chunk of the webpack runtime (`miniCssF=function(e){return({id:"name",...}[e]||e)+"."+{id:"hash",...}[e]+".css"}`).
pub fn css_chunks(js: &str) -> Vec<(String, String)> {
    let Some(i) = js.find("miniCssF=function") else {
        return vec![];
    };
    let f = &js[i..];
    let Some(names_at) = f.find("return({") else {
        return vec![];
    };
    let f = &f[names_at + 8..];
    let Some(names_end) = f.find("}[e]||e)") else {
        return vec![];
    };
    let (names, rest) = (&f[..names_end], &f[names_end..]);
    let Some(h_at) = rest.find("+\".\"+{") else {
        return vec![];
    };
    let rest = &rest[h_at + 6..];
    let Some(h_end) = rest.find("}[e]") else {
        return vec![];
    };
    let names = pairs(names);
    pairs(&rest[..h_end])
        .into_iter()
        .map(|(id, hash)| {
            let name = names
                .iter()
                .find(|(i, _)| *i == id)
                .map_or(id.as_str(), |(_, n)| n.as_str());
            (name.to_string(), format!("{name}.{hash}.css"))
        })
        .collect()
}

/// `id:"value"` pairs of a JS object literal body.
fn pairs(s: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    for part in s.split(',') {
        if let Some((k, v)) = part.split_once(':') {
            let v = v.trim().trim_matches('"');
            out.push((k.trim().to_string(), v.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_a_start_page_like_a_browser_would() {
        let html = r#"<!doctype html><html><head>
<!-- <link rel="stylesheet" href="commented.css"> -->
<link rel=stylesheet href=/web/a.css>
<LINK REL="Stylesheet" HREF='b.css?v=1' media="screen">
<link rel="alternate stylesheet" href="alt.css">
<link rel="stylesheet" href="print.css" media="print">
<link rel="stylesheet" href="off.css" disabled>
<link rel="stylesheet" href="narrow.css" media="(max-width: 600px)">
<link rel="preload" as="style" href="pre.css">
<link rel="stylesheet" href="https://cdn.example/x.css">
<link rel="stylesheet" href="b.css?v=1">
<style>.a { color: red }</style>
<style media="print">.p{}</style>
<style>  </style>
<script>document.write('<link rel="stylesheet" href="in-script.css">')</script>
<script src="main.js" defer></script>
</head><body><script type="module" src="m.js"></script></body></html>"#;
        let s = index_scan(html);
        let link = |h: &str, m: Option<&str>| IndexSheet::Link {
            href: h.into(),
            media: m.map(Into::into),
        };
        assert_eq!(
            s.sheets,
            [
                link("/web/a.css", None),
                link("b.css?v=1", None),
                link("narrow.css", Some("(max-width: 600px)")),
                link("https://cdn.example/x.css", None),
                link("b.css?v=1", None),
                IndexSheet::Inline {
                    css: ".a { color: red }".into(),
                    media: None
                },
            ]
        );
        assert_eq!(s.scripts, 3);
        assert_eq!(index_scan("<p>nothing</p>"), IndexScan::default());
    }

    #[test]
    fn only_the_builds_own_links_are_the_base_layer() {
        let l = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let (build, own) = own_links(&l(&[
            "1133.aa.css?b1",
            "main.cc.css?b1",
            "plug/x.css",
            "/sleek/y.css?v=2",
            "https://cdn.example/z.css?b1",
            "other.css?v=9",
        ]));
        assert_eq!(build, "b1");
        assert_eq!(own, ["1133.aa.css", "main.cc.css"]);
        // No query anywhere: older web clients, every relative link is the client's.
        assert_eq!(own_links(&l(&["a.css", "/b.css"])).1, ["a.css"]);
    }

    #[test]
    fn reads_the_links_and_the_chunk_map() {
        let html = r#"<link rel="manifest" href="manifest.json"><link href="1133.aa.css?bb" rel="stylesheet"><link href="main.cc.css?bb" rel="stylesheet">"#;
        assert_eq!(
            index_stylesheets(html),
            ["1133.aa.css?bb", "main.cc.css?bb"]
        );
        let js = r#"x.miniCssF=function(e){return({27962:"home",75844:"itemDetails"}[e]||e)+"."+{1451:"af33",27962:"193c",75844:"c860"}[e]+".css"},y"#;
        assert_eq!(
            css_chunks(js),
            [
                ("1451".to_string(), "1451.af33.css".to_string()),
                ("home".to_string(), "home.193c.css".to_string()),
                (
                    "itemDetails".to_string(),
                    "itemDetails.c860.css".to_string()
                ),
            ]
        );
        assert!(css_chunks("nothing").is_empty());
        let face = r#"@font-face{font-family:Material Icons;src:url(a.eot);src:local("x"),url(m.woff2) format("woff2"),url(m.1.ttf) format("truetype")}@font-face{font-family:Other;src:url(o.ttf)}"#;
        assert_eq!(
            font_face_file(face, "Material Icons").as_deref(),
            Some("m.1.ttf")
        );
        assert_eq!(font_face_file(face, "Other").as_deref(), Some("o.ttf"));
        assert_eq!(font_face_file(face, "Nope"), None);
    }
}
