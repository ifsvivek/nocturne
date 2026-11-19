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

    /// Which stylesheets the server's web client has. `Ok(None)`: no web client.
    pub async fn web_files(&self) -> Result<Option<WebFiles>> {
        let Some(index) = self.web_text("index.html").await? else {
            return Ok(None);
        };
        let linked = index_stylesheets(&index);
        if linked.is_empty() {
            return Ok(None);
        }
        let build = linked
            .iter()
            .find_map(|l| l.split_once('?').map(|(_, q)| q.to_string()))
            .unwrap_or_default();
        let linked: Vec<String> = linked
            .into_iter()
            .map(|l| l.split('?').next().unwrap_or("").to_string())
            .collect();
        let chunks = match self.web_text("runtime.bundle.js").await? {
            Some(js) => css_chunks(&js),
            None => vec![],
        };
        Ok(Some(WebFiles {
            build,
            linked,
            chunks,
        }))
    }
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
    }
}
