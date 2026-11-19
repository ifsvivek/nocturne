//! Remote parts of a theme: `@import` of other stylesheets and `@font-face` font files. A theme
//! (or a server's branding CSS) is expanded here before it is parsed: every `@import` is replaced
//! by the fetched text (CSS order kept, `@media` lists kept as `@media` wrappers), every font is
//! downloaded, decoded and handed back as [`LoadedFont`]. Nothing is executed and nothing in a
//! fetched sheet is followed except further `@import` and `@font-face` `url()`s under the same
//! limits; `url()` in a declaration is still never fetched (the parser warns).
//!
//! Rules, all enforced here and tested against a loopback server: https only; no credentials,
//! cookies or `Referer`; a fixed generic User-Agent; 10 s per download and 30 s per theme; at
//! most 3 redirects, each https again; hosts that are this computer or a private network are
//! refused; stylesheets text/css or text/plain, 256 KB each, 1 MB together, depth 3, 16 imports,
//! cycles refused by normalised address; fonts 2 MB each, 8 MB together, 12 files, `font/*`,
//! `application/font-*` or `application/octet-stream`, checked by their first bytes and parsed
//! before use. Failures become [`Note`]s (shown as warnings), never errors. Downloads are cached
//! on disk by address and only replaced by a manual refresh. Run [`resolve`] through
//! `bridge::spawn`.
mod fonts;

pub use fonts::{FontStyle, LoadedFont};

use super::{Error, MAX_BYTES, Parsed, Warning, WarningKind, Warnings, css, translate};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// All imported stylesheets of one theme.
pub const MAX_TOTAL: usize = 1024 * 1024;
pub const MAX_DEPTH: usize = 3;
pub const MAX_IMPORTS: usize = 16;
const MAX_URL: usize = 2048;
const AGENT: &str = "NOCTURNE";
/// Cache bounds: oldest files go first.
const CACHE_FILES: usize = 256;
const CACHE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Options {
    /// The `remote_css` setting. Off: nothing is fetched and the text is returned untouched.
    pub remote: bool,
    /// Manual refresh: download again instead of using the cache (the cached copy is the fallback
    /// when that fails).
    pub refresh: bool,
    /// Tests only: allow `http://127.0.0.1` and loopback redirects. Never set by the app.
    pub allow_loopback: bool,
    /// Per-user cache directory; `None` disables caching.
    pub cache_dir: Option<PathBuf>,
    pub timeout: Duration,
    /// Time for all downloads of one theme.
    pub budget: Duration,
}

impl Options {
    pub fn new(remote: bool, refresh: bool) -> Self {
        Self {
            remote,
            refresh,
            allow_loopback: false,
            cache_dir: default_cache_dir(),
            timeout: Duration::from_secs(10),
            budget: Duration::from_secs(30),
        }
    }

    /// When `url` was last downloaded (Unix seconds), for "fetched on ..." in the Themes page.
    pub fn fetched_at(&self, url: &str) -> Option<u64> {
        let key = norm(&Url::parse(url).ok()?);
        cache_read(self.cache_dir.as_deref()?, &key).map(|c| c.fetched)
    }
}

/// `css` in the per-user cache directory.
pub fn default_cache_dir() -> Option<PathBuf> {
    Some(nocturne_session::paths::cache_dir()?.join("css"))
}

/// One thing that was not fetched or not used, with the address and why. Shown as a warning on
/// the line of the `@import` or `@font-face` that caused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub line: u32,
    pub col: u32,
    pub url: String,
    pub reason: String,
    /// Needs no action (shown dimmed).
    pub info: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    /// The text with imports inlined and `@font-face` blocks removed (their fonts are in
    /// `fonts`). Comments are blanked and line numbers of the original text are kept.
    pub css: String,
    pub notes: Vec<Note>,
    pub fonts: Vec<LoadedFont>,
    /// Files fetched from the network / read from the cache.
    pub downloaded: usize,
    pub from_cache: usize,
}

/// Does the text have anything this module would fetch? (Cheap; lets callers skip the async step.)
pub fn has_remote(css: &str) -> bool {
    let l = css.to_ascii_lowercase();
    l.contains("@import") || l.contains("@font-face")
}

/// Parse a resolved text: as `customize::parse`, with the size limit of the expanded text and the
/// notes added as warnings.
pub fn parse_resolved(r: &Resolved) -> Result<Parsed, Error> {
    if r.css.len() > MAX_BYTES + MAX_TOTAL {
        return Err(Error::TooLarge { bytes: r.css.len() });
    }
    let mut chars: Vec<char> = r.css.chars().collect();
    let mut w = Warnings::new(&chars);
    css::blank_comments(&mut chars, &mut w);
    let rules = css::parse(&chars, &mut w);
    let (theme, layout) = translate(&rules, &mut w);
    for n in &r.notes {
        w.list.push(Warning {
            line: n.line,
            col: n.col,
            kind: if n.info {
                WarningKind::Approximated
            } else {
                WarningKind::RemoteResource
            },
            message: if n.url.is_empty() {
                n.reason.clone()
            } else {
                format!("{}: {}", css::clip(&n.url), n.reason)
            },
        });
    }
    w.list.sort_by_key(|x| (x.line, x.col));
    Ok(Parsed {
        theme,
        layout,
        warnings: w.list,
        dropped_warnings: w.dropped,
    })
}

// ---- addresses -------------------------------------------------------------------------------

fn norm(u: &Url) -> String {
    let mut u = u.clone();
    u.set_fragment(None);
    u.to_string()
}

fn private_host(u: &Url) -> bool {
    use std::net::{Ipv4Addr, Ipv6Addr};
    let v4 = |a: Ipv4Addr| {
        let o = a.octets();
        a.is_loopback()
            || a.is_private()
            || a.is_link_local()
            || a.is_unspecified()
            || a.is_broadcast()
            || o[0] == 0
            || (o[0] == 100 && (64..=127).contains(&o[1]))
    };
    let v6 = |a: Ipv6Addr| {
        let s = a.segments()[0];
        a.is_loopback()
            || a.is_unspecified()
            || s & 0xfe00 == 0xfc00
            || s & 0xffc0 == 0xfe80
            || a.to_ipv4_mapped().is_some_and(v4)
    };
    match u.host() {
        Some(h) => match h.to_string().parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(a)) => v4(a),
            Ok(std::net::IpAddr::V6(a)) => v6(a),
            Err(_) => {
                let d = u
                    .host_str()
                    .unwrap_or("")
                    .trim_end_matches('.')
                    .to_ascii_lowercase();
                d == "localhost" || d.ends_with(".localhost")
            }
        },
        None => true,
    }
}

fn loopback_host(u: &Url) -> bool {
    match u.host_str().map(|h| h.trim_matches(['[', ']'])) {
        Some(h) => {
            h.eq_ignore_ascii_case("localhost")
                || h.parse::<std::net::IpAddr>().is_ok_and(|a| a.is_loopback())
        }
        None => false,
    }
}

/// May this address be fetched at all?
fn check_url(u: &Url, allow_loopback: bool) -> Result<(), String> {
    if u.as_str().len() > MAX_URL {
        return Err("the address is too long".into());
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("addresses with a user name or password are not fetched".into());
    }
    if u.host().is_none() {
        return Err("the address has no host".into());
    }
    match u.scheme() {
        "https" => {}
        "http" if allow_loopback && loopback_host(u) => {}
        _ => return Err("only https:// addresses are fetched".into()),
    }
    if !allow_loopback && private_host(u) {
        return Err("addresses on this computer or a private network are not fetched".into());
    }
    Ok(())
}

fn absolute(raw: &str, base: Option<&Url>) -> Result<Url, String> {
    if let Ok(u) = Url::parse(raw) {
        return Ok(u);
    }
    base.ok_or_else(|| {
        "not a full address, and there is no stylesheet address to resolve it against".to_string()
    })?
    .join(raw)
    .map_err(|_| "not a valid address".to_string())
}

// ---- the disk cache --------------------------------------------------------------------------

struct Cached {
    fetched: u64,
    final_url: String,
    bytes: Vec<u8>,
}

fn cache_file(dir: &Path, key: &str) -> PathBuf {
    let h = Sha256::digest(key.as_bytes());
    dir.join(format!(
        "{}.bin",
        h.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

/// File layout: fetch time (Unix seconds), requested address, final address (one per line), body.
fn cache_read(dir: &Path, key: &str) -> Option<Cached> {
    let raw = std::fs::read(cache_file(dir, key)).ok()?;
    let mut parts = raw.splitn(4, |b| *b == b'\n');
    let fetched = std::str::from_utf8(parts.next()?).ok()?.parse().ok()?;
    if std::str::from_utf8(parts.next()?).ok()? != key {
        return None;
    }
    let final_url = std::str::from_utf8(parts.next()?).ok()?.to_string();
    Some(Cached {
        fetched,
        final_url,
        bytes: parts.next()?.to_vec(),
    })
}

fn cache_write(dir: &Path, key: &str, final_url: &str, bytes: &[u8]) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut out = format!("{now}\n{key}\n{final_url}\n").into_bytes();
    out.extend_from_slice(bytes);
    let path = cache_file(dir, key);
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, out).is_ok() && std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    prune(dir);
}

fn prune(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = rd
        .filter_map(|e| {
            let e = e.ok()?;
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let mut count = files.len();
    for (_, len, p) in files {
        if count <= CACHE_FILES && total <= CACHE_BYTES {
            break;
        }
        if std::fs::remove_file(p).is_ok() {
            count -= 1;
            total -= len;
        }
    }
}

// ---- scanning --------------------------------------------------------------------------------

enum Item {
    Import {
        start: usize,
        end: usize,
        url: String,
        cond: String,
    },
    Face {
        start: usize,
        end: usize,
        body: (usize, usize),
    },
}

fn skip_str(c: &[char], i: usize) -> usize {
    let q = c[i];
    let mut j = i + 1;
    while j < c.len() {
        match c[j] {
            '\\' => j += 2,
            ch if ch == q => return j + 1,
            '\n' => return j,
            _ => j += 1,
        }
    }
    c.len()
}

fn block_end(c: &[char], open: usize) -> usize {
    let (mut depth, mut i) = (0i32, open);
    while i < c.len() {
        match c[i] {
            '"' | '\'' => {
                i = skip_str(c, i);
                continue;
            }
            '\\' => {
                i += 2;
                continue;
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth <= 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    c.len()
}

fn ws(c: &[char], mut i: usize) -> usize {
    while i < c.len() && c[i].is_whitespace() {
        i += 1;
    }
    i
}

fn starts_ci(c: &[char], i: usize, s: &str) -> bool {
    s.chars()
        .enumerate()
        .all(|(k, ch)| c.get(i + k).is_some_and(|x| x.eq_ignore_ascii_case(&ch)))
}

/// A quoted string at `i` without escapes or line breaks: (text, index after the quote).
fn quoted(c: &[char], i: usize) -> Option<(String, usize)> {
    let q = *c.get(i).filter(|q| matches!(q, '"' | '\''))?;
    let n = c[i + 1..]
        .iter()
        .position(|ch| *ch == q || *ch == '\\' || *ch == '\n')?;
    (c[i + 1 + n] == q).then(|| (c[i + 1..i + 1 + n].iter().collect(), i + n + 2))
}

/// `@import <url> <conditions>;` at `at`: (index after the statement, address, conditions).
fn parse_import(c: &[char], at: usize) -> Option<(usize, String, String)> {
    let mut j = ws(c, at + "@import".len());
    let url = if starts_ci(c, j, "url(") {
        j = ws(c, j + 4);
        let u = if let Some((s, k)) = quoted(c, j) {
            j = ws(c, k);
            s
        } else {
            let n = c[j..].iter().position(|ch| *ch == ')')?;
            let s: String = c[j..j + n].iter().collect::<String>().trim().to_string();
            if s.contains(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | '\\')) {
                return None;
            }
            j += n;
            s
        };
        if c.get(j) != Some(&')') {
            return None;
        }
        j += 1;
        u
    } else {
        let (s, k) = quoted(c, j)?;
        j = k;
        s
    };
    let mut k = j;
    let mut depth = 0i32;
    while k < c.len() {
        match c[k] {
            '"' | '\'' => {
                k = skip_str(c, k);
                continue;
            }
            '(' => depth += 1,
            ')' => depth -= 1,
            ';' if depth <= 0 => break,
            '{' | '}' => return None,
            _ => {}
        }
        k += 1;
    }
    let cond: String = c[j..k.min(c.len())].iter().collect();
    Some(((k + 1).min(c.len()), url, cond.trim().to_string()))
}

fn scan(c: &[char]) -> Vec<Item> {
    let (mut out, mut i, mut depth) = (vec![], 0usize, 0i32);
    while i < c.len() {
        match c[i] {
            '"' | '\'' => {
                i = skip_str(c, i);
                continue;
            }
            '\\' => i += 1,
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            '@' if depth == 0 && starts_ci(c, i + 1, "import") => {
                if let Some((end, url, cond)) = parse_import(c, i) {
                    out.push(Item::Import {
                        start: i,
                        end,
                        url,
                        cond,
                    });
                    i = end;
                    continue;
                }
            }
            '@' if starts_ci(c, i + 1, "font-face") => {
                let open = ws(c, i + "@font-face".len());
                if c.get(open) == Some(&'{') {
                    let end = block_end(c, open);
                    out.push(Item::Face {
                        start: i,
                        end: (end + 1).min(c.len()),
                        body: (open + 1, end.min(c.len())),
                    });
                    i = end + 1;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn position(c: &[char], idx: usize) -> (u32, u32) {
    let before = &c[..idx.min(c.len())];
    let line = before.iter().filter(|ch| **ch == '\n').count();
    let col = before.iter().rev().take_while(|ch| **ch != '\n').count();
    (line as u32 + 1, col as u32 + 1)
}

/// Braces must balance (outside strings) so a fetched sheet cannot close or swallow the rules
/// around it.
fn balanced(c: &[char]) -> bool {
    let (mut d, mut i) = (0i64, 0usize);
    while i < c.len() {
        match c[i] {
            '"' | '\'' => {
                i = skip_str(c, i);
                continue;
            }
            '\\' => i += 1,
            '{' => d += 1,
            '}' => {
                d -= 1;
                if d < 0 {
                    return false;
                }
            }
            _ => {}
        }
        i += 1;
    }
    d == 0
}

/// A media query list that cannot break out of the `@media` wrapper.
fn cond_ok(s: &str) -> Result<(), &'static str> {
    let l = s.to_ascii_lowercase();
    if l.starts_with("layer") || l.starts_with("supports") {
        return Err("`layer` and `supports()` after an @import are not supported");
    }
    if s.len() > 500 || s.contains(['"', '\'', '\\', '{', '}', ';']) {
        return Err("the media condition is not supported");
    }
    let mut d = 0i32;
    for ch in s.chars() {
        d += match ch {
            '(' | '[' => 1,
            ')' | ']' => -1,
            _ => 0,
        };
        if d < 0 {
            return Err("the media condition is not supported");
        }
    }
    if d != 0 {
        return Err("the media condition is not supported");
    }
    Ok(())
}

// ---- fetching and expanding ------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Css,
    Font,
}

type Raw = Result<(Arc<Vec<u8>>, Url), String>;

struct Net {
    client: reqwest::Client,
    opts: Options,
    deadline: Instant,
    css_total: usize,
    font_total: usize,
    imports: usize,
    font_files: usize,
    stack: Vec<String>,
    memo: HashMap<String, Raw>,
    notes: Vec<Note>,
    faces: Vec<fonts::Face>,
    faces_capped: bool,
    /// Where the notes of the current import or font point.
    cur: (u32, u32),
    downloaded: usize,
    from_cache: usize,
}

fn describe(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else if e.is_redirect() {
        "too many redirects"
    } else {
        "the download failed"
    }
}

impl Net {
    fn note(&mut self, at: (u32, u32), url: &str, reason: impl Into<String>, info: bool) {
        self.notes.push(Note {
            line: at.0,
            col: at.1,
            url: url.to_string(),
            reason: reason.into(),
            info,
        });
    }

    async fn net_get(&self, url: &Url, kind: Kind) -> Raw {
        let left = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or("the time allowed for downloads ran out")?;
        let max = if kind == Kind::Css {
            MAX_BYTES
        } else {
            fonts::MAX_FILE
        };
        let get = async {
            let mut r = self
                .client
                .get(url.clone())
                .header(
                    reqwest::header::ACCEPT,
                    if kind == Kind::Css {
                        "text/css,text/plain"
                    } else {
                        "font/*,*/*"
                    },
                )
                .send()
                .await
                .map_err(|e| describe(&e).to_string())?;
            let status = r.status();
            if status.is_redirection() {
                return Err("redirect refused (only https, at most 3)".to_string());
            }
            if !status.is_success() {
                return Err(format!("the server answered {}", status.as_u16()));
            }
            let ct = r
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let ok = match kind {
                Kind::Css => super::content_type_ok(&ct),
                Kind::Font => fonts::content_type_ok(&ct),
            };
            if !ok {
                return Err(match kind {
                    Kind::Css => "not a stylesheet (expected text/css or text/plain)".to_string(),
                    Kind::Font => format!("not a font (content type `{}`)", css::clip(&ct)),
                });
            }
            let too_big = || format!("larger than {} KB", max / 1024);
            if r.content_length().is_some_and(|n| n as usize > max) {
                return Err(too_big());
            }
            let mut body = Vec::new();
            while let Some(chunk) = r.chunk().await.map_err(|e| describe(&e).to_string())? {
                body.extend_from_slice(&chunk);
                if body.len() > max {
                    return Err(too_big());
                }
            }
            Ok((Arc::new(body), r.url().clone()))
        };
        tokio::time::timeout(self.opts.timeout.min(left), get)
            .await
            .unwrap_or_else(|_| Err("timed out".into()))
    }

    /// Memo, then disk cache, then network (then the stale cache copy when a refresh fails).
    async fn fetch(&mut self, url: &Url, kind: Kind) -> Raw {
        let key = norm(url);
        let mkey = format!("{}{key}", if kind == Kind::Css { 'c' } else { 'f' });
        if let Some(r) = self.memo.get(&mkey) {
            return r.clone();
        }
        let max = if kind == Kind::Css {
            MAX_BYTES
        } else {
            fonts::MAX_FILE
        };
        let cached = self
            .opts
            .cache_dir
            .as_deref()
            .and_then(|d| cache_read(d, &key))
            .filter(|c| c.bytes.len() <= max);
        let from_cache = |c: Cached| -> Raw {
            let u = Url::parse(&c.final_url).unwrap_or_else(|_| url.clone());
            Ok((Arc::new(c.bytes), u))
        };
        let result = match cached {
            Some(c) if !self.opts.refresh => {
                self.from_cache += 1;
                from_cache(c)
            }
            c => match self.net_get(url, kind).await {
                Ok((bytes, fin)) => {
                    self.downloaded += 1;
                    if let Some(d) = &self.opts.cache_dir {
                        cache_write(d, &key, &norm(&fin), &bytes);
                    }
                    Ok((bytes, fin))
                }
                Err(e) => match c {
                    Some(c) => {
                        self.from_cache += 1;
                        let at = self.cur;
                        self.note(
                            at,
                            &key,
                            format!("could not refresh ({e}); the saved copy is used"),
                            false,
                        );
                        from_cache(c)
                    }
                    None => Err(e),
                },
            },
        };
        self.memo.insert(mkey, result.clone());
        result
    }

    /// One `@import` at nesting `level`: the text to put in its place (no line breaks).
    async fn import(
        &mut self,
        raw: &str,
        cond: &str,
        base: Option<&Url>,
        level: usize,
        at: (u32, u32),
    ) -> String {
        self.cur = at;
        let fail = |n: &mut Self, why: String| {
            n.note(at, raw, why, false);
            String::new()
        };
        if let Err(why) = cond_ok(cond) {
            return fail(self, why.into());
        }
        let url = match absolute(raw, base).and_then(|u| {
            check_url(&u, self.opts.allow_loopback)?;
            Ok(u)
        }) {
            Ok(u) => u,
            Err(why) => return fail(self, why),
        };
        let key = norm(&url);
        if self.stack.contains(&key) {
            return fail(
                self,
                "an import cycle: this stylesheet imports itself".into(),
            );
        }
        if level + 1 > MAX_DEPTH {
            return fail(
                self,
                format!("imports nested deeper than {MAX_DEPTH} levels"),
            );
        }
        if self.imports >= MAX_IMPORTS {
            return fail(
                self,
                format!("more than {MAX_IMPORTS} imports in one theme"),
            );
        }
        self.imports += 1;
        let (bytes, final_url) = match self.fetch(&url, Kind::Css).await {
            Ok(r) => r,
            Err(why) => return fail(self, why),
        };
        if self.css_total + bytes.len() > MAX_TOTAL {
            return fail(
                self,
                format!(
                    "the {} MB limit for imported styles is reached",
                    MAX_TOTAL >> 20
                ),
            );
        }
        self.css_total += bytes.len();
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return fail(self, "the stylesheet is not UTF-8 text".into());
        };
        let mut chars: Vec<char> = text.chars().collect();
        let mut w = Warnings::new(&chars);
        css::blank_comments(&mut chars, &mut w);
        if !balanced(&chars) {
            return fail(
                self,
                "unbalanced braces; the stylesheet was not used".into(),
            );
        }
        let prepared: String = chars.into_iter().collect();
        self.stack.push(key);
        let inner = self
            .expand(&prepared, Some(&final_url), level + 1, Some(at))
            .await;
        self.stack.pop();
        let inner: String = inner
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();
        if cond.is_empty() {
            inner
        } else {
            format!("@media {cond} {{ {inner} }}")
        }
    }

    /// `text` at nesting `level` with imports inlined and font faces collected and removed.
    /// `anchor` is the position of the top-level import that led here (warnings point at it).
    fn expand<'a>(
        &'a mut self,
        text: &'a str,
        base: Option<&'a Url>,
        level: usize,
        anchor: Option<(u32, u32)>,
    ) -> Pin<Box<dyn std::future::Future<Output = String> + Send + 'a>> {
        Box::pin(async move {
            let mut c: Vec<char> = text.chars().collect();
            let mut w = Warnings::new(&c);
            css::blank_comments(&mut c, &mut w);
            let items = scan(&c);
            let (mut out, mut last) = (String::new(), 0usize);
            for item in items {
                let (start, end) = match &item {
                    Item::Import { start, end, .. } | Item::Face { start, end, .. } => {
                        (*start, *end)
                    }
                };
                out.extend(&c[last..start]);
                let at = anchor.unwrap_or_else(|| position(&c, start));
                match item {
                    Item::Import { url, cond, .. } => {
                        let r = self.import(&url, &cond, base, level, at).await;
                        out.push_str(&r);
                    }
                    Item::Face { body: (a, b), .. } => {
                        if self.faces.len() >= fonts::MAX_FACES {
                            if !self.faces_capped {
                                self.faces_capped = true;
                                self.note(
                                    at,
                                    "",
                                    format!(
                                        "more than {} @font-face rules; the rest are ignored",
                                        fonts::MAX_FACES
                                    ),
                                    true,
                                );
                            }
                        } else {
                            let body: String = c[a..b].iter().collect();
                            let resolve = |raw: &str| {
                                if raw.starts_with("data:") {
                                    return Some(raw.to_string());
                                }
                                absolute(raw, base).ok().map(|u| norm(&u))
                            };
                            match fonts::parse_face(&body, &resolve, at) {
                                Ok(f) => self.faces.push(f),
                                Err(why) => self.note(at, "", why, false),
                            }
                        }
                    }
                }
                // Keep the line count of the original so warnings still point at the right line.
                out.extend(c[start..end].iter().filter(|ch| **ch == '\n'));
                last = end;
            }
            out.extend(&c[last..]);
            out
        })
    }

    async fn font_file(&mut self, src: &fonts::Src) -> Result<(Vec<u8>, Option<String>), String> {
        if self.font_files >= fonts::MAX_FILES {
            return Err(format!(
                "more than {} font files in one theme",
                fonts::MAX_FILES
            ));
        }
        self.font_files += 1;
        let raw: Arc<Vec<u8>> = if src.url.starts_with("data:") {
            Arc::new(fonts::data_uri(&src.url)?)
        } else {
            let url = Url::parse(&src.url).map_err(|_| "not a valid address".to_string())?;
            check_url(&url, self.opts.allow_loopback)?;
            self.fetch(&url, Kind::Font).await?.0
        };
        if raw.len() > fonts::MAX_FILE {
            return Err(format!("larger than {} MB", fonts::MAX_FILE >> 20));
        }
        if self.font_total + raw.len() > fonts::MAX_TOTAL {
            return Err(format!(
                "the {} MB limit for fonts in one theme is reached",
                fonts::MAX_TOTAL >> 20
            ));
        }
        self.font_total += raw.len();
        let sfnt = fonts::to_sfnt(&raw)?;
        let family = fonts::validate(&sfnt)?;
        Ok((sfnt, family))
    }

    async fn load_fonts(&mut self) -> Vec<LoadedFont> {
        let faces = std::mem::take(&mut self.faces);
        let (faces, dropped) = fonts::select(faces);
        if dropped > 0 {
            self.note((1, 1), "", format!("{dropped} other unicode-range part(s) of the same fonts were skipped; the Latin one is used"), true);
        }
        let mut out = vec![];
        for face in faces {
            let mut why = "no loadable source (only TTF, OTF, WOFF and WOFF2 are used)".to_string();
            let mut shown = face.srcs.first().map(|s| s.url.clone()).unwrap_or_default();
            let mut loaded = None;
            self.cur = face.anchor;
            for src in fonts::ordered(&face) {
                match self.font_file(src).await {
                    Ok(f) => {
                        loaded = Some((src.url.clone(), f));
                        break;
                    }
                    Err(e) => {
                        why = e;
                        shown = src.url.clone();
                    }
                }
            }
            match loaded {
                Some((url, (bytes, internal_family))) => out.push(LoadedFont {
                    family: face.family,
                    weight: face.weight,
                    weight_max: face.weight_max,
                    style: face.style,
                    internal_family,
                    url,
                    bytes: bytes.into(),
                }),
                None => {
                    let shown = if shown.starts_with("data:") {
                        "data:".into()
                    } else {
                        shown
                    };
                    self.note(
                        face.anchor,
                        &shown,
                        format!("font `{}`: {why}", face.family),
                        false,
                    )
                }
            }
        }
        out
    }
}

/// Expand the imports of `css` and load its fonts. `base` is the address the text came from (for
/// relative imports); `None` for typed or pasted text. With `opts.remote` off, or nothing to
/// fetch, the text comes back unchanged and the parser's own warnings explain what was ignored.
pub async fn resolve(css: &str, base: Option<&str>, opts: &Options) -> Resolved {
    let unchanged = || Resolved {
        css: css.to_string(),
        ..Default::default()
    };
    if !opts.remote || !has_remote(css) {
        return unchanged();
    }
    let allow = opts.allow_loopback;
    let policy = reqwest::redirect::Policy::custom(move |a| {
        if a.previous().len() > 3 || check_url(a.url(), allow).is_err() {
            a.stop()
        } else {
            a.follow()
        }
    });
    let Ok(client) = reqwest::Client::builder()
        .user_agent(AGENT)
        .referer(false)
        .redirect(policy)
        .connect_timeout(opts.timeout)
        .build()
    else {
        return unchanged();
    };
    let mut net = Net {
        client,
        opts: opts.clone(),
        deadline: Instant::now() + opts.budget,
        css_total: 0,
        font_total: 0,
        imports: 0,
        font_files: 0,
        stack: vec![],
        memo: HashMap::new(),
        notes: vec![],
        faces: vec![],
        faces_capped: false,
        cur: (1, 1),
        downloaded: 0,
        from_cache: 0,
    };
    let base = base.and_then(|b| Url::parse(b).ok());
    let out = net.expand(css, base.as_ref(), 0, None).await;
    let fonts = net.load_fonts().await;
    let mut notes = net.notes;
    notes.sort_by_key(|n| (n.line, n.col));
    Resolved {
        css: out,
        notes,
        fonts,
        downloaded: net.downloaded,
        from_cache: net.from_cache,
    }
}

// ---- built-in themes -------------------------------------------------------------------------

/// A theme that ships with NOCTURNE. Third-party themes are never bundled: users paste any
/// Jellyfin CSS (or an `@import` line, or a link) themselves and [`resolve`] fetches the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinTheme {
    pub id: &'static str,
    pub name: &'static str,
    pub css: &'static str,
    /// Shown under the name when the theme is someone else's work.
    pub credit: &'static str,
    pub homepage: &'static str,
    /// The text only imports other files, so it does nothing without `remote_css`.
    pub remote: bool,
}

/// The built-in themes, in the order of the picker. NOCTURNE is the default look (no CSS).
pub const BUILTIN: &[BuiltinTheme] = &[
    BuiltinTheme {
        id: "nocturne",
        name: "NOCTURNE",
        css: "",
        credit: "",
        homepage: "",
        remote: false,
    },
    // Placeholder: the stylesheet arrives with the Glass preset.
    BuiltinTheme {
        id: "glass",
        name: "Glass",
        css: "",
        credit: "",
        homepage: "",
        remote: false,
    },
];

pub fn builtin(id: &str) -> Option<&'static BuiltinTheme> {
    BUILTIN.iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests;
