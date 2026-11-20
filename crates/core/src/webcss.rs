//! The server's own jellyfin-web stylesheets, the base layer of Jellyfin layout mode (`docs/dev/V052_PLAN.md` item 10): read from the server at run time (as a browser does), cached per server and web-client build under `$XDG_CACHE_HOME/nocturne/jfweb/`, never bundled. Order is the web client's: the stylesheets `index.html` links, the shared and page stylesheets, then the web theme (`themes/dark/theme.css`); the server's branding CSS and the user's theme go on top, in the app.
use nocturne_jellyfin as jf;
use std::path::PathBuf;
use tokio::task::JoinSet;

/// Page stylesheets the Home, library and detail pages load.
const PAGES: &[&str] = &[
    "home",
    "home-js",
    "home-tsx",
    "hometab",
    "itemDetails",
    "library",
    "list",
    "movies",
    "movies-movies",
    "shows",
    "shows-tvshows",
    "boxsets",
    "favorites",
    "mixed",
];
/// A shared (numerically named) stylesheet is kept when it styles one of these (the others are players, dialogs and the dashboard).
const KEYS: &[&str] = &[
    ".homePage",
    ".itemDetailPage",
    ".emby-tab-button",
    ".lazy-hidden",
    ".libraryPage",
    ".detailPage",
    ".cardBox",
];
/// Total size cap of the assembled sheet, and the biggest file taken (the icon font's class list is left out; only its base `.material-icons` rule is kept).
const TOTAL: usize = 3 << 20;
const FILE: usize = 100 << 10;
/// The icon font of the web client: the family its stylesheet names, and the biggest font file taken (the ttf is about 350 KB).
pub const ICON_FAMILY: &str = "Material Icons";
const FONT_MAX: usize = 2 << 20;

/// What the server's web client gives the layout mode: its stylesheet and its icon font.
#[derive(Clone, Debug, Default)]
pub struct WebBase {
    pub css: String,
    /// `(family, ttf bytes)`, fetched from the server at run time, cached beside the sheet, never bundled.
    pub font: Option<(String, Vec<u8>)>,
    /// The stylesheets the server's start page adds (plugins), read as a browser would.
    pub plugins: Option<Plugins>,
}

/// The stylesheet limits for plugin sheets: how many, how big each (same origin), and how big together.
const PLUGIN_SHEETS: usize = 60;
const PLUGIN_FILE: usize = 1 << 20;
const PLUGIN_TOTAL: usize = 4 << 20;

/// What the server's start page (`/web/index.html`) adds to the web client's own stylesheets, as one text in document order. Scripts are only counted: they are never run.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Plugins {
    /// The sheets in order; cross-origin https ones as `@import` lines (the import machinery of `customize::import` fetches those under its rules and the `remote_css` setting).
    pub css: String,
    pub loaded: usize,
    pub scripts: usize,
    /// One line per sheet left out, with the reason.
    pub skipped: Vec<String>,
}

/// One start-page stylesheet after the rules about where it may come from.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// Same origin: fetched from the server.
    Fetch(reqwest::Url, Option<String>),
    Inline(String, Option<String>, reqwest::Url),
    /// Another origin over https: left to the import machinery.
    Remote(reqwest::Url, Option<String>),
}

/// Which sheets of `scan` to use, in order. `own` are the paths (`/web/...`) of the stylesheets the base layer already has; duplicates (same address and media) count once; at most [`PLUGIN_SHEETS`].
fn plan(
    index: &reqwest::Url,
    scan: &jf::IndexScan,
    own: &[String],
    skipped: &mut Vec<String>,
) -> Vec<Plan> {
    let mut out: Vec<Plan> = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut over = 0;
    for s in &scan.sheets {
        let p = match s {
            jf::IndexSheet::Inline { css, media } => {
                Plan::Inline(css.clone(), media.clone(), index.clone())
            }
            jf::IndexSheet::Link { href, media } => {
                let Ok(mut u) = index.join(href) else {
                    skipped.push(format!("{href}: not a valid address"));
                    continue;
                };
                u.set_fragment(None);
                if own.iter().any(|o| o == u.path()) {
                    continue;
                }
                if !seen.insert((u.to_string(), media.clone())) {
                    continue;
                }
                if u.origin() == index.origin() {
                    Plan::Fetch(u, media.clone())
                } else if u.scheme() == "https" {
                    Plan::Remote(u, media.clone())
                } else {
                    skipped.push(format!("{u}: another server, and not https"));
                    continue;
                }
            }
        };
        if out.len() >= PLUGIN_SHEETS {
            over += 1;
        } else {
            out.push(p);
        }
    }
    if over > 0 {
        skipped.push(format!(
            "{over} more stylesheets: only the first {PLUGIN_SHEETS} are read"
        ));
    }
    out
}

fn wrap(css: &str, media: &Option<String>) -> String {
    match media {
        Some(m) => format!("@media {m} {{\n{css}\n}}\n"),
        None => format!("{css}\n"),
    }
}

/// The text of the plan in order. `got[i]` is what was fetched for `plan[i]` (`None` for a failed fetch, which is reported); the total stays under [`PLUGIN_TOTAL`].
fn build(plan: &[Plan], got: Vec<Option<String>>, skipped: &mut Vec<String>) -> (String, usize) {
    let (mut out, mut n) = (String::new(), 0);
    for (p, t) in plan.iter().zip(got) {
        let piece = match (p, t) {
            (Plan::Fetch(u, m), Some(t)) => {
                // Relative `url()` of a sheet are relative to that sheet, which is lost once it is inlined.
                let mut abs = String::new();
                crate::customize::import::push_abs(
                    &mut abs,
                    &t.chars().collect::<Vec<_>>(),
                    Some(u),
                );
                wrap(&abs, m)
            }
            (Plan::Fetch(u, _), None) => {
                skipped.push(format!("{u}: not readable (missing, too big or an error)"));
                continue;
            }
            (Plan::Inline(css, m, base), _) => {
                let mut abs = String::new();
                crate::customize::import::push_abs(
                    &mut abs,
                    &css.chars().collect::<Vec<_>>(),
                    Some(base),
                );
                wrap(&abs, m)
            }
            (Plan::Remote(u, m), _) => {
                let cond = m.as_deref().map(|m| format!(" {m}")).unwrap_or_default();
                format!(
                    "@import url(\"{}\"){cond};\n",
                    u.as_str().replace('"', "%22")
                )
            }
        };
        if out.len() + piece.len() > PLUGIN_TOTAL {
            skipped.push(format!(
                "the rest: the stylesheets together are over {} MB",
                PLUGIN_TOTAL >> 20
            ));
            break;
        }
        out.push_str(&piece);
        n += 1;
    }
    (out, n)
}

/// `css` with each `@import` of a stylesheet on the same server replaced by that file's text (as a browser would load it), `depth` levels deep and at most eight per file. Imports of other servers stay for the import machinery.
fn inline_imports<'a>(
    c: &'a jf::Client,
    css: String,
    base: &'a reqwest::Url,
    depth: u8,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send + 'a>> {
    use crate::customize::import::{Item, push_abs, scan};
    Box::pin(async move {
        if depth == 0 {
            return css;
        }
        let chars: Vec<char> = css.chars().collect();
        let (mut out, mut at, mut n) = (String::new(), 0, 0);
        for it in scan(&chars) {
            let Item::Import {
                start,
                end,
                url,
                cond,
            } = it
            else {
                continue;
            };
            let Ok(u) = base.join(&url) else { continue };
            if u.origin() != base.origin() || n >= 8 {
                continue;
            }
            let Some(t) = c.same_origin_text(&u, PLUGIN_FILE).await.ok().flatten() else {
                continue;
            };
            n += 1;
            let t = inline_imports(c, t, &u, depth - 1).await;
            out.extend(&chars[at..start]);
            let mut abs = String::new();
            push_abs(&mut abs, &t.chars().collect::<Vec<_>>(), Some(&u));
            out.push_str(&wrap(&abs, &Some(cond).filter(|c| !c.is_empty())));
            at = end;
        }
        out.extend(&chars[at..]);
        out
    })
}

/// Read the stylesheets of the server's start page, cached per server and start page text (the page changes when a plugin is added), under `jfweb/`. [`clear_plugin_cache`] forgets them.
async fn plugins(client: &jf::Client, scope: &str, files: &jf::WebFiles) -> Option<Plugins> {
    let cache = cache_file(scope, &files.index, "plug");
    if let Some(c) = &cache
        && let Ok(b) = std::fs::read(c)
        && let Ok(p) = serde_json::from_slice::<Plugins>(&b)
    {
        return Some(p);
    }
    let scan = jf::index_scan(&files.index);
    let index = client.web_index_url();
    let own: Vec<String> = files
        .linked
        .iter()
        .chain(files.chunks.iter().map(|(_, p)| p))
        .filter_map(|p| index.join(p).ok().map(|u| u.path().to_string()))
        .collect();
    let mut skipped = vec![];
    let plan = plan(&index, &scan, &own, &mut skipped);
    let mut got: Vec<Option<String>> = vec![None; plan.len()];
    let mut set = JoinSet::new();
    let mut todo: Vec<usize> = (0..plan.len())
        .filter(|i| matches!(plan[*i], Plan::Fetch(..)))
        .collect();
    todo.reverse();
    loop {
        while set.len() < 8
            && let Some(i) = todo.pop()
        {
            let Plan::Fetch(u, _) = &plan[i] else {
                continue;
            };
            let (c, u) = (client.clone(), u.clone());
            set.spawn(async move {
                let t = c.same_origin_text(&u, PLUGIN_FILE).await.ok().flatten();
                match t {
                    Some(t) => (i, Some(inline_imports(&c, t, &u, 2).await)),
                    None => (i, None),
                }
            });
        }
        match set.join_next().await {
            Some(Ok((i, t))) => got[i] = t,
            Some(Err(_)) => {}
            None => break,
        }
    }
    let (css, loaded) = build(&plan, got, &mut skipped);
    let p = Plugins {
        css,
        loaded,
        scripts: scan.scripts,
        skipped,
    };
    if let (Some(c), Ok(b)) = (&cache, serde_json::to_vec(&p)) {
        write_atomic(c, &b);
    }
    Some(p)
}

/// The base sheet, icon font and plugin stylesheets of `client`'s web client; `None` when the server runs without one (`--nowebclient`) or it cannot be read.
pub async fn fetch(client: &jf::Client, scope: &str) -> Option<WebBase> {
    let files = client.web_files().await.ok()??;
    let mut b = base(client, scope, &files).await?;
    b.plugins = plugins(client, scope, &files).await;
    Some(b)
}

/// Forget the cached plugin stylesheets (Themes > Refresh imports); the next load of a server's UI reads them again.
pub fn clear_plugin_cache() {
    let Some(d) = nocturne_session::paths::cache_dir().map(|d| d.join("jfweb")) else {
        return;
    };
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        if e.path().extension().is_some_and(|x| x == "plug") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn cache_file(scope: &str, build: &str, ext: &str) -> Option<PathBuf> {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in scope.bytes().chain(build.bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    Some(
        nocturne_session::paths::cache_dir()?
            .join("jfweb")
            .join(format!("{h:016x}.{ext}")),
    )
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // Atomic: a torn file would be taken for the whole sheet.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// The base stylesheet and icon font of `client`'s web client, `None` when the server runs without one (`--nowebclient`) or it cannot be read.
async fn base(client: &jf::Client, scope: &str, files: &jf::WebFiles) -> Option<WebBase> {
    let css_cache = cache_file(scope, &files.build, "css");
    let font_cache = cache_file(scope, &files.build, "ttf");
    if let Some(c) = &css_cache
        && let Ok(css) = std::fs::read_to_string(c)
        && !css.is_empty()
    {
        let font = font_cache
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .map(|b| (ICON_FAMILY.to_string(), b));
        // Name rules without their font would draw boxes: ask the server again.
        if font.is_some() || !css.contains(".material-icons.play_arrow:before") {
            return Some(WebBase {
                css,
                font,
                plugins: None,
            });
        }
    }
    let (mut css, names, face) = assemble(client, files).await?;
    let font = match &face {
        Some(path) => client
            .web_bytes(path, FONT_MAX)
            .await
            .ok()
            .flatten()
            .map(|b| (ICON_FAMILY.to_string(), b)),
        None => None,
    };
    if font.is_some() {
        css.push_str(&names);
    }
    // A sheet whose font failed to arrive is not cached: the next start asks again.
    if face.is_none() || font.is_some() {
        if let Some(c) = &css_cache {
            write_atomic(c, css.as_bytes());
        }
        if let (Some(f), Some((_, b))) = (&font_cache, &font) {
            write_atomic(f, b);
        }
    }
    Some(WebBase {
        css,
        font,
        plugins: None,
    })
}

/// Icon names whose `.material-icons.NAME:before` rule is kept from the font's class list (the list has ~2,000; the layout mode draws these).
#[rustfmt::skip]
const ICONS: &[&str] = &[
    "play_arrow",
    "replay",
    "get_app",
    "theaters",
    "explore",
    "shuffle",
    "delete",
    "stop",
    "check",
    "favorite",
    "favorite_border",
    "call_split",
    "more_vert",
    "more_horiz",
    "star",
    "keyboard_arrow_down",
    "keyboard_arrow_up",
    "keyboard_arrow_right",
    "keyboard_arrow_left",
    "chevron_right",
    "chevron_left",
    "arrow_back",
    "arrow_forward",
    "home",
    "search",
    "cast",
    "cast_connected",
    "menu",
    "person",
    "add",
    "close",
    "done",
    "folder",
    "movie",
    "tv",
    "live_tv",
    "library_music",
    "photo_library",
    "video_library",
    "video_settings",
    "settings",
    "dashboard",
    "info",
    "sort",
    "filter_list",
    "view_list",
    "view_module",
    "play_circle_filled",
    "play_circle_outline",
    "playlist_add",
    "playlist_play",
    "edit",
    "refresh",
    "volume_up",
    "volume_off",
    "fullscreen",
    "closed_caption",
    "audiotrack",
    "subtitles",
    "visibility",
    "lock",
    "expand_more",
    "expand_less",
    "check_circle",
    "radio_button_checked",
    "radio_button_unchecked",
    "file_download",
    "fiber_manual_record",
    "videocam",
    "photo_album",
    "photo",
    "open_in_new",
    "link",
    "queue_music",
    "skip_next",
    "skip_previous",
    "fast_forward",
    "fast_rewind",
    "pause",
    "list",
    "new_releases",
    "account_circle",
    "collections_bookmark",
    "book",
    "music_note",
    "navigate_before",
    "navigate_next",
    "mic",
    "language",
    "public",
];

/// `(base rule, name rules)` of a stylesheet too big to take whole: the `.material-icons{...}` base rule, and the `:before` rules of [`ICONS`] (the class list maps icon names to code points). The name rules are only useful once the font itself is there, else they would draw boxes.
fn icon_rules(t: &str) -> (String, String) {
    let mut base = String::new();
    let mut out = String::new();
    if let Some(i) = t.find(".material-icons{")
        && let Some(j) = t[i..].find('}')
    {
        base.push_str(&t[i..i + j + 1]);
        base.push('\n');
    }
    for n in ICONS {
        let key = format!(".material-icons.{n}:before{{");
        if let Some(i) = t.find(&key)
            && let Some(j) = t[i..].find('}')
        {
            out.push_str(&t[i..i + j + 1]);
            out.push('\n');
        }
    }
    (base, out)
}

/// The sheet, the icon name rules, and the path of the icon font file when a stylesheet declares one.
async fn assemble(
    client: &jf::Client,
    files: &jf::WebFiles,
) -> Option<(String, String, Option<String>)> {
    let numeric = |n: &str| n.bytes().all(|b| b.is_ascii_digit());
    // (order key, path): linked first, then shared and page chunks in the runtime's order, the theme last.
    let mut want: Vec<(String, bool)> = files.linked.iter().map(|p| (p.clone(), false)).collect();
    for (name, path) in &files.chunks {
        if PAGES.contains(&name.as_str()) {
            want.push((path.clone(), false));
        } else if numeric(name) {
            want.push((path.clone(), true));
        }
    }
    want.push(("themes/dark/theme.css".into(), false));
    let theme = want.len() - 1;
    let mut got: Vec<Option<String>> = vec![None; want.len()];
    let mut set = JoinSet::new();
    let mut next = 0;
    let mut done = 0;
    while done < want.len() {
        while next < want.len() && set.len() < 8 {
            let (path, _) = want[next].clone();
            let c = client.clone();
            let i = next;
            set.spawn(async move { (i, c.web_text(&path).await.ok().flatten()) });
            next += 1;
        }
        let (i, t) = set.join_next().await?.ok()?;
        got[i] = t;
        done += 1;
    }
    let mut out = String::new();
    let mut names = String::new();
    let face = got
        .iter()
        .flatten()
        .find_map(|t| jf::font_face_file(t, ICON_FAMILY));
    for (i, (_, filtered)) in want.iter().enumerate() {
        let Some(t) = &got[i] else { continue };
        if t.len() > FILE && i != theme {
            let (b, n) = icon_rules(t);
            out.push_str(&b);
            names.push_str(&n);
            continue;
        }
        if *filtered && !KEYS.iter().any(|k| t.contains(k)) {
            continue;
        }
        if out.len() + t.len() > TOTAL {
            break;
        }
        out.push_str(t);
        out.push('\n');
    }
    (!out.is_empty()).then_some((out, names, face))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"<link rel="stylesheet" href="main.abc.css?x">
<link rel="stylesheet" href="/sleek/a.css">
<link rel="stylesheet" href="https://fonts.example/f.css" media="(min-width: 800px)">
<link rel="stylesheet" href="http://other.example/z.css">
<link rel="stylesheet" href="/sleek/a.css#again">
<style>.x{background:url(img/p.png)}</style>
<link rel="stylesheet" href="b.css" media="(max-width: 600px)">
<script src="s.js"></script>"#;

    #[test]
    fn plans_by_origin_skips_the_base_and_duplicates_in_document_order() {
        let index = reqwest::Url::parse("http://jf.lan:8096/web/index.html").unwrap();
        let scan = jf::index_scan(INDEX);
        let mut skipped = vec![];
        let own = vec!["/web/main.abc.css".to_string()];
        let plan = plan(&index, &scan, &own, &mut skipped);
        assert_eq!(plan.len(), 4);
        assert!(matches!(&plan[0], Plan::Fetch(u, None) if u.path() == "/sleek/a.css"));
        assert!(
            matches!(&plan[1], Plan::Remote(u, Some(m)) if u.host_str() == Some("fonts.example") && m == "(min-width: 800px)")
        );
        assert!(matches!(&plan[2], Plan::Inline(..)));
        assert!(matches!(&plan[3], Plan::Fetch(u, Some(_)) if u.path() == "/web/b.css"));
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("other.example"));
        assert_eq!(scan.scripts, 1);
        // Order, media wrapping, imports for remote ones, relative url() made absolute.
        let got = vec![
            Some(".a{color:red}".into()),
            None,
            None,
            Some(".b{x:y}".into()),
        ];
        let (css, n) = build(&plan, got, &mut skipped);
        assert_eq!(n, 4);
        let at = |s: &str| css.find(s).unwrap();
        assert!(at(".a{") < at("@import url(\"https://fonts.example/f.css\") (min-width: 800px);"));
        assert!(at("@import") < at("http://jf.lan:8096/web/img/p.png"));
        assert!(at("p.png") < at("@media (max-width: 600px) {"));
    }

    #[test]
    fn caps_the_number_of_sheets_and_reports_unreadable_ones() {
        let index = reqwest::Url::parse("http://jf.lan/web/index.html").unwrap();
        let html: String = (0..70)
            .map(|i| format!("<link rel=stylesheet href=/p/{i}.css>"))
            .collect();
        let mut skipped = vec![];
        let p = plan(&index, &jf::index_scan(&html), &[], &mut skipped);
        assert_eq!(p.len(), PLUGIN_SHEETS);
        assert!(skipped[0].starts_with("10 more"));
        let (css, n) = build(&p[..2], vec![None, Some("a{}".into())], &mut skipped);
        assert_eq!((css.as_str(), n), ("a{}\n", 1));
        assert!(skipped[1].contains("/p/0.css"));
    }

    #[test]
    fn keeps_the_base_and_the_wanted_icon_rules_only() {
        let t = ".x{a:b}.material-icons{font-family:Material Icons}\n.material-icons.zoom_in:before{content:\"a\"}\n.material-icons.play_arrow:before{content:\"b\"}\n";
        let (base, names) = icon_rules(t);
        assert_eq!(base, ".material-icons{font-family:Material Icons}\n");
        assert_eq!(names, ".material-icons.play_arrow:before{content:\"b\"}\n");
    }
}
