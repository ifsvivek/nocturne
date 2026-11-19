//! Saved themes: `themes.toml` next to `settings.toml`, versioned, atomic write, no secrets.
//! Only the CSS text is stored; tokens are re-read from it on load (parsing is milliseconds), so a
//! better engine later improves old themes. Selection is global with an optional per-source
//! override keyed by the source id of `sources.toml`; see `docs/dev/CUSTOMIZATION_PLAN.md`.
use super::import::{LoadedFont, Resolved, has_remote, parse_resolved};
use super::{Error, Parsed, Theme, parse};
use crate::source::SourceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;

/// One saved theme.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub id: u32,
    pub name: String,
    pub css: String,
    /// Where it was imported from, kept for a manual "Refresh"; never fetched on its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    /// Next id to hand out; only grows.
    pub next_id: u32,
    #[serde(rename = "theme")]
    pub themes: Vec<Saved>,
    /// Theme used by the shell and by sources without an override.
    pub global: Option<u32>,
    /// Source id (as text, TOML keys are strings) to theme id.
    pub server_theme: BTreeMap<String, u32>,
    /// Server branding CSS per source (only for sources in Server UI mode). Session state: never
    /// serialized, never turned into a saved theme.
    #[serde(skip)]
    pub branding: BTreeMap<String, String>,
    /// The `remote_css` setting as the app last told us (session state). Off: resolved imports
    /// are ignored and the plain parser warnings explain what was skipped.
    #[serde(skip, default = "yes")]
    remote: bool,
    /// Expanded copies (imports inlined, fonts loaded) of stylesheets, keyed by a hash of the
    /// original text, so edited text or changed branding is simply not found and asks for a new
    /// fetch. Session state, newest last, only a few kept.
    #[serde(skip)]
    resolved: Vec<(u64, Resolved)>,
}

fn yes() -> bool {
    true
}

const KEPT_RESOLVED: usize = 8;

fn text_hash(css: &str) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    css.hash(&mut h);
    h.finish()
}

/// Which layer decides the look for a source; compared to skip redundant `theme::set` calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Active {
    Override(u32),
    /// Hash of the branding CSS, so changed CSS re-applies.
    Branding(u64),
    Global(u32),
    Default,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: VERSION,
            next_id: 1,
            themes: vec![],
            global: None,
            server_theme: BTreeMap::new(),
            branding: BTreeMap::new(),
            remote: true,
            resolved: vec![],
        }
    }
}

impl Store {
    pub fn default_path() -> Option<PathBuf> {
        nocturne_session::Servers::default_path()
            .ok()
            .map(|p| p.with_file_name("themes.toml"))
    }

    pub fn load() -> Self {
        Self::default_path().map_or_else(Self::default, |p| Self::load_from(&p))
    }

    /// Missing or corrupt file gives an empty store (themes are optional decoration).
    pub fn load_from(path: &Path) -> Self {
        let mut s: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        s.version = VERSION;
        let top = s.themes.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        s.next_id = s.next_id.max(top);
        s.prune();
        s
    }

    pub fn save(&self) {
        if let Some(p) = Self::default_path() {
            let _ = self.save_to(&p); // calm: a failed save only loses the theme choice
        }
    }

    /// Atomic: temp file in the same directory, then rename.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().expect("path has a parent"))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(
            &tmp,
            toml::to_string_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&tmp, path)
    }

    pub fn get(&self, id: u32) -> Option<&Saved> {
        self.themes.iter().find(|t| t.id == id)
    }

    pub fn add(&mut self, name: &str, css: &str, url: Option<String>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.themes.push(Saved {
            id,
            name: clean_name(name),
            css: css.into(),
            url,
        });
        id
    }

    /// Replace a theme's name, CSS and import URL; false when it is gone.
    pub fn update(&mut self, id: u32, name: &str, css: &str, url: Option<String>) -> bool {
        let Some(t) = self.themes.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        t.name = clean_name(name);
        t.css = css.into();
        t.url = url;
        true
    }

    /// Delete a theme and every assignment that pointed at it.
    pub fn delete(&mut self, id: u32) {
        self.themes.retain(|t| t.id != id);
        self.prune();
    }

    pub fn set_global(&mut self, id: Option<u32>) {
        self.global = id.filter(|i| self.get(*i).is_some());
    }

    /// `None` removes the override of `source`.
    pub fn set_override(&mut self, source: SourceId, id: Option<u32>) {
        match id.filter(|i| self.get(*i).is_some()) {
            Some(i) => self.server_theme.insert(source.0.to_string(), i),
            None => self.server_theme.remove(&source.0.to_string()),
        };
    }

    pub fn override_of(&self, source: SourceId) -> Option<u32> {
        self.server_theme.get(&source.0.to_string()).copied()
    }

    /// `None` removes the branding layer of `source` (Server UI off, or the server has none).
    pub fn set_branding(&mut self, source: SourceId, css: Option<String>) {
        match css.filter(|c| !c.trim().is_empty()) {
            Some(c) => self.branding.insert(source.0.to_string(), c),
            None => self.branding.remove(&source.0.to_string()),
        };
    }

    pub fn branding_of(&self, source: SourceId) -> Option<&str> {
        self.branding.get(&source.0.to_string()).map(String::as_str)
    }

    /// Which layer applies, in order: the source's own assignment (an explicit choice), the
    /// branding CSS of the UI source (Server UI mode), the global theme. The shell (settings,
    /// header) and local folders follow the global theme.
    pub fn active(&self, source: Option<SourceId>) -> Active {
        self.active_with(source, source)
    }

    /// As [`Store::active`] when the UI of another source is used: the override belongs to
    /// `source` (what is browsed), the branding layer to `ui` (whose UI is shown); see
    /// `server_ui::pick`. The user's own per-source theme always wins over the branding.
    pub fn active_with(&self, source: Option<SourceId>, ui: Option<SourceId>) -> Active {
        if let Some(i) = source.and_then(|s| self.override_of(s)) {
            return Active::Override(i);
        }
        if let Some(c) = ui.and_then(|u| self.branding_of(u)) {
            return Active::Branding(text_hash(c));
        }
        self.global.map_or(Active::Default, Active::Global)
    }

    /// The stylesheet text of the layer that applies (before imports are expanded).
    pub fn active_css(&self, source: Option<SourceId>, ui: Option<SourceId>) -> Option<&str> {
        match self.active_with(source, ui) {
            Active::Override(i) | Active::Global(i) => self.get(i).map(|t| t.css.as_str()),
            Active::Branding(_) => ui.and_then(|u| self.branding_of(u)),
            Active::Default => None,
        }
    }

    /// `active` as one comparable number (the type itself is not exported). It also changes when
    /// the imports of the active text arrive, so the new look is applied.
    pub fn active_key(&self, source: Option<SourceId>) -> u64 {
        self.active_key_with(source, source)
    }

    pub fn active_key_with(&self, source: Option<SourceId>, ui: Option<SourceId>) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.active_with(source, ui).hash(&mut h);
        self.active_css(source, ui)
            .and_then(|c| self.resolved_for(c))
            .is_some()
            .hash(&mut h);
        h.finish()
    }

    /// The tokens that apply (the built-in look when nothing is assigned).
    pub fn resolve(&self, source: Option<SourceId>) -> Theme {
        self.resolve_all(source).0
    }

    /// Tokens and layout of the theme that applies (both built-in when nothing is assigned).
    pub fn resolve_all(&self, source: Option<SourceId>) -> (Theme, super::Layout) {
        self.resolve_all_with(source, source)
    }

    pub fn resolve_all_with(
        &self,
        source: Option<SourceId>,
        ui: Option<SourceId>,
    ) -> (Theme, super::Layout) {
        self.active_css(source, ui)
            .and_then(|c| self.parse_text(c).ok())
            .map(|p| (p.theme, p.layout))
            .unwrap_or_default()
    }

    // ---- remote imports and fonts ----

    /// The `remote_css` setting changed. Turning it off forgets everything fetched this session.
    pub fn set_remote(&mut self, on: bool) {
        self.remote = on;
        if !on {
            self.resolved.clear();
        }
    }

    pub fn remote(&self) -> bool {
        self.remote
    }

    /// Store the result of `import::resolve(original, ..)`.
    pub fn set_resolved(&mut self, original: &str, r: Resolved) {
        let k = text_hash(original);
        self.resolved.retain(|(h, _)| *h != k);
        self.resolved.push((k, r));
        if self.resolved.len() > KEPT_RESOLVED {
            self.resolved.remove(0);
        }
    }

    /// Forget fetched copies (a manual Refresh, before resolving again).
    pub fn clear_resolved(&mut self) {
        self.resolved.clear();
    }

    pub fn resolved_for(&self, css: &str) -> Option<&Resolved> {
        if !self.remote {
            return None;
        }
        let k = text_hash(css);
        self.resolved.iter().find(|(h, _)| *h == k).map(|(_, r)| r)
    }

    /// Does `css` have imports or fonts that still have to be fetched? True once per text; the
    /// app then runs `import::resolve` through the bridge and hands the result to `set_resolved`.
    /// Never true with the setting off, and nothing polls: a theme is only fetched when it is
    /// applied, refreshed by hand, or when its text changed.
    pub fn needs_resolve(&self, css: &str) -> bool {
        self.remote && has_remote(css) && self.resolved_for(css).is_none()
    }

    /// Parse a stylesheet with its fetched imports when there are any.
    pub fn parse_text(&self, css: &str) -> Result<Parsed, Error> {
        match self.resolved_for(css) {
            Some(r) => parse_resolved(r),
            None => parse(css),
        }
    }

    /// Fonts fetched for `css`, to register with the UI.
    pub fn fonts_for(&self, css: &str) -> &[LoadedFont] {
        self.resolved_for(css).map_or(&[], |r| &r.fonts)
    }

    fn prune(&mut self) {
        let have: Vec<u32> = self.themes.iter().map(|t| t.id).collect();
        if self.global.is_some_and(|g| !have.contains(&g)) {
            self.global = None;
        }
        self.server_theme.retain(|_, t| have.contains(t));
    }
}

fn clean_name(n: &str) -> String {
    let n: String = n.chars().filter(|c| !c.is_control()).take(48).collect();
    match n.trim() {
        "" => "Untitled theme".into(),
        t => t.into(),
    }
}

/// Name for a stylesheet that was just imported from a file or URL. An import never edits the selected saved theme (the
/// editor detaches from it and saving adds a new one), so a selected theme's name is not reused; a name typed for a new
/// theme is kept.
pub fn import_name(selected: bool, typed: &str, file: &str) -> String {
    if selected || typed.trim().is_empty() {
        file.to_string()
    } else {
        typed.to_string()
    }
}

const MAX_URL: usize = 2048;

/// Only the URL the user typed is contacted: https (plain http only for loopback), a host, no
/// embedded credentials.
pub fn validate_url(raw: &str) -> Result<String, String> {
    let u = raw.trim();
    if u.is_empty() || u.len() > MAX_URL || u.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Enter one web address".into());
    }
    let (https, rest) = if let Some(r) = u.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = u.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("The address must start with https://".into());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err("The address needs a host and no user name or password".into());
    }
    if !https {
        let host = match authority.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => h,
            _ => authority,
        };
        if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
            return Err("Only https:// addresses are allowed".into());
        }
    }
    Ok(u.into())
}

/// `text/css` or `text/plain` (raw GitHub serves plain text), parameters ignored.
pub fn content_type_ok(ct: &str) -> bool {
    let m = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    m == "text/css" || m == "text/plain"
}

/// Fetch a stylesheet once, on the user's action (run through `bridge::spawn`): no cookies or
/// credentials, at most 3 redirects and only to https (or loopback http), 10 s, 256 KB read while
/// streaming. Nothing in the text is followed.
pub async fn fetch(url: &str) -> Result<String, String> {
    use std::time::Duration;
    let url = validate_url(url)?;
    let policy = reqwest::redirect::Policy::custom(|a| {
        if a.previous().len() >= 3 || validate_url(a.url().as_str()).is_err() {
            a.stop()
        } else {
            a.follow()
        }
    });
    let mut r = reqwest::Client::builder()
        .user_agent(concat!("NOCTURNE/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .redirect(policy)
        .build()
        .map_err(|e| e.to_string())?
        .get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| "Could not download that address".to_string())?;
    let ct = r
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type_ok(ct) {
        return Err("That address is not a stylesheet (expected text/css or text/plain)".into());
    }
    let mut body = Vec::new();
    while let Some(c) = r
        .chunk()
        .await
        .map_err(|_| "The download was interrupted".to_string())?
    {
        body.extend_from_slice(&c);
        if body.len() > super::MAX_BYTES {
            return Err(format!(
                "The stylesheet is larger than {} KB",
                super::MAX_BYTES / 1024
            ));
        }
    }
    String::from_utf8(body).map_err(|_| "The stylesheet is not UTF-8 text".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: &str = ":root { --nocturne-accent: #ff0000; }";
    const BLUE: &str = ":root { --nocturne-accent: #0000ff; }";

    fn accent(t: &Theme) -> Option<[u8; 3]> {
        t.accent.map(|c| [c.r, c.g, c.b])
    }

    #[test]
    fn import_adds_a_theme_named_after_the_file_and_keeps_the_selected_one() {
        let mut s = Store::default();
        let marquee = s.add("marquee", RED, None);
        // editor state while "marquee" is selected: an import detaches (sel = None) and renames
        assert_eq!(import_name(true, "marquee", "feed"), "feed");
        let feed = s.add(&import_name(true, "marquee", "feed"), BLUE, None);
        assert_ne!(feed, marquee);
        assert_eq!(s.get(marquee).unwrap().css, RED);
        assert_eq!(s.get(feed).unwrap().name, "feed");
        // a new theme the user already named keeps that name; a blank one takes the file's
        assert_eq!(import_name(false, "Mine", "feed"), "Mine");
        assert_eq!(import_name(false, "  ", "feed"), "feed");
    }

    #[test]
    fn resolve_prefers_override_then_global_then_default() {
        let mut s = Store::default();
        let (red, blue) = (s.add("Red", RED, None), s.add("Blue", BLUE, None));
        assert_eq!(s.resolve(None), Theme::default());
        s.set_global(Some(red));
        s.set_override(SourceId(7), Some(blue));
        assert_eq!(accent(&s.resolve(None)), Some([255, 0, 0]));
        assert_eq!(accent(&s.resolve(Some(SourceId(3)))), Some([255, 0, 0]));
        assert_eq!(accent(&s.resolve(Some(SourceId(7)))), Some([0, 0, 255]));
        s.set_override(SourceId(7), None);
        assert_eq!(accent(&s.resolve(Some(SourceId(7)))), Some([255, 0, 0]));
    }

    #[test]
    fn branding_sits_between_override_and_global_and_is_never_saved() {
        let mut s = Store::default();
        let (red, blue) = (s.add("Red", RED, None), s.add("Blue", BLUE, None));
        let src = SourceId(5);
        s.set_global(Some(red));
        s.set_branding(src, Some(":root { --nocturne-accent: #00ff00; }".into()));
        assert_eq!(accent(&s.resolve(Some(src))), Some([0, 255, 0]));
        assert_eq!(accent(&s.resolve(Some(SourceId(6)))), Some([255, 0, 0]));
        assert_eq!(accent(&s.resolve(None)), Some([255, 0, 0]));
        s.set_override(src, Some(blue));
        assert_eq!(accent(&s.resolve(Some(src))), Some([0, 0, 255]));
        s.set_override(src, None);
        let a = s.active(Some(src));
        s.set_branding(src, Some(":root { --nocturne-accent: #00fe00; }".into()));
        assert_ne!(s.active(Some(src)), a, "changed CSS must re-apply");
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("themes.toml");
        s.save_to(&p).unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("00fe00"));
        assert!(Store::load_from(&p).branding.is_empty());
        s.set_branding(src, Some("  ".into()));
        assert_eq!(s.active(Some(src)), Active::Global(red));
    }

    #[test]
    fn delete_clears_assignments_and_unknown_ids_are_refused() {
        let mut s = Store::default();
        let a = s.add("A", RED, None);
        s.set_global(Some(a));
        s.set_override(SourceId(1), Some(a));
        s.set_override(SourceId(2), Some(99));
        assert!(s.override_of(SourceId(2)).is_none());
        s.delete(a);
        assert!(s.global.is_none() && s.server_theme.is_empty() && s.themes.is_empty());
        assert_eq!(s.add("B", BLUE, None), a + 1, "ids are never reused");
    }

    #[test]
    fn round_trip_is_atomic_and_tolerates_junk() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nocturne").join("themes.toml");
        let mut s = Store::default();
        let a = s.add("  Mine\n", RED, Some("https://example.com/a.css".into()));
        s.set_global(Some(a));
        s.set_override(SourceId(4), Some(a));
        s.save_to(&p).unwrap();
        assert!(!p.with_extension("toml.tmp").exists());
        assert_eq!(Store::load_from(&p), s);
        assert_eq!(s.get(a).unwrap().name, "Mine");
        std::fs::write(&p, "not toml {{{").unwrap();
        assert_eq!(Store::load_from(&p), Store::default());
        assert_eq!(
            Store::load_from(&d.path().join("none.toml")),
            Store::default()
        );
    }

    #[test]
    fn urls_are_https_only_without_credentials() {
        for ok in [
            "https://raw.githubusercontent.com/a/b/main/t.css",
            "http://127.0.0.1:8000/t.css",
            "http://localhost/t.css",
        ] {
            assert!(validate_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "http://example.com/t.css",
            "ftp://example.com/t.css",
            "https://user:pw@example.com/t.css",
            "https://",
            "https://exa mple.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "http://localhost.evil.com/t.css",
        ] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
    }

    /// Loopback server answering every request with `head` + `body`.
    async fn serve(head: &'static str, body: Vec<u8>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut c, _)) = l.accept().await {
                let mut b = [0u8; 1024];
                let _ = c.read(&mut b).await;
                let n = body.len();
                let h = format!(
                    "HTTP/1.1 200 OK\r\n{head}Content-Length: {n}\r\nConnection: close\r\n\r\n"
                );
                let _ = c.write_all(h.as_bytes()).await;
                let _ = c.write_all(&body).await;
            }
        });
        format!("http://127.0.0.1:{port}/t.css")
    }

    #[tokio::test]
    async fn fetch_checks_type_and_size() {
        let ok = serve("Content-Type: text/plain\r\n", RED.into()).await;
        assert_eq!(fetch(&ok).await.unwrap(), RED);
        let html = serve("Content-Type: text/html\r\n", RED.into()).await;
        assert!(fetch(&html).await.is_err());
        let big = vec![b'a'; super::super::MAX_BYTES + 1];
        let big = serve("Content-Type: text/css\r\n", big).await;
        assert!(fetch(&big).await.unwrap_err().contains("larger"));
        assert!(fetch("http://example.com/x.css").await.is_err());
    }

    #[test]
    fn another_sources_branding_sits_below_the_own_theme_and_above_global() {
        let (a, b, c) = (SourceId(1), SourceId(2), SourceId(3));
        let mut s = Store::default();
        let g = s.add("g", ":root{--nocturne-accent:#010101}", None);
        let own = s.add("own", ":root{--nocturne-accent:#020202}", None);
        s.set_global(Some(g));
        s.set_branding(a, Some(":root{--nocturne-accent:#0a0a0a}".into()));
        // browsing b (or All, or local) with a's UI: a's branding replaces global
        let accent = |s: &Store, src, ui| s.resolve_all_with(src, ui).0.accent;
        assert_ne!(accent(&s, Some(b), Some(a)), accent(&s, Some(b), None));
        assert_eq!(accent(&s, None, Some(a)), accent(&s, Some(a), Some(a)));
        assert_eq!(accent(&s, Some(b), None), accent(&s, Some(c), None));
        // the user's own theme for b wins over a's branding
        s.set_override(b, Some(own));
        assert_eq!(accent(&s, Some(b), Some(a)), accent(&s, Some(b), None));
        // the old single-source API is the same thing with ui == source
        assert_eq!(s.active(Some(a)), s.active_with(Some(a), Some(a)));
        assert_eq!(s.active_key(Some(a)), s.active_key_with(Some(a), Some(a)));
        // a source without branding of its own does not borrow one
        assert_eq!(s.active(Some(c)), Active::Global(g));
    }

    #[test]
    fn fetched_imports_are_used_per_text_and_respect_the_setting() {
        use super::super::import::Resolved;
        let mut s = Store::default();
        let a = SourceId(1);
        let text = "@import url(\"https://example.com/a.css\");";
        s.set_branding(a, Some(text.into()));
        assert!(s.needs_resolve(text) && !s.needs_resolve(":root{}"));
        let k0 = s.active_key(Some(a));
        assert!(s.resolve(Some(a)).accent.is_none());
        let r = Resolved {
            css: ":root{--nocturne-accent:#123456}".into(),
            ..Default::default()
        };
        s.set_resolved(text, r);
        assert!(!s.needs_resolve(text), "fetched once, never polled");
        assert!(s.resolve(Some(a)).accent.is_some());
        assert_ne!(k0, s.active_key(Some(a)), "arrival re-applies the theme");
        // changed branding text is a new text: it asks for a fetch again
        let edited = format!("{text} ");
        assert!(s.needs_resolve(&edited));
        assert!(s.parse_text(&edited).unwrap().theme.accent.is_none());
        // setting off: ignored and forgotten, nothing is wanted
        s.set_remote(false);
        assert!(s.resolved_for(text).is_none() && !s.needs_resolve(text));
        assert!(s.resolve(Some(a)).accent.is_none());
        assert!(
            s.parse_text(text)
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.message.contains("`@import` is not fetched"))
        );
        s.set_remote(true);
        assert!(s.needs_resolve(text), "turning it back on fetches again");
        // only a few copies are kept
        for i in 0..20 {
            s.set_resolved(&format!("@import 'https://e.example/{i}';"), Resolved::default());
        }
        assert!(s.resolved.len() <= KEPT_RESOLVED);
        // never written to themes.toml
        assert!(!toml::to_string(&s).unwrap().contains("resolved"));
    }

    #[test]
    fn count_counts_set_tokens() {
        let t = parse(":root{--nocturne-accent:red;--nocturne-radius-card:4px}")
            .unwrap()
            .theme;
        assert_eq!(t.count(), 2);
        assert_eq!(Theme::default().count(), 0);
    }

    #[test]
    fn content_types() {
        assert!(content_type_ok("text/css; charset=utf-8"));
        assert!(content_type_ok("TEXT/PLAIN"));
        assert!(!content_type_ok("text/html"));
        assert!(!content_type_ok(""));
    }
}
