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
/// Total size cap of the assembled sheet, and the biggest file taken (the icon font's class list is left out: no icon font in the engine yet).
const TOTAL: usize = 3 << 20;
const FILE: usize = 100 << 10;

fn cache_file(scope: &str, build: &str) -> Option<PathBuf> {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in scope.bytes().chain(build.bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    Some(
        nocturne_session::paths::cache_dir()?
            .join("jfweb")
            .join(format!("{h:016x}.css")),
    )
}

/// The base stylesheet of `client`'s web client, `None` when the server runs without one (`--nowebclient`) or it cannot be read.
pub async fn fetch(client: &jf::Client, scope: &str) -> Option<String> {
    let files = client.web_files().await.ok()??;
    let cache = cache_file(scope, &files.build);
    if let Some(c) = &cache
        && let Ok(s) = std::fs::read_to_string(c)
        && !s.is_empty()
    {
        return Some(s);
    }
    let css = assemble(client, &files).await?;
    if let Some(c) = &cache {
        if let Some(d) = c.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        // Atomic: a torn file would be taken for the whole sheet.
        let tmp = c.with_extension("tmp");
        if std::fs::write(&tmp, &css).is_ok() {
            let _ = std::fs::rename(&tmp, c);
        }
    }
    Some(css)
}

async fn assemble(client: &jf::Client, files: &jf::WebFiles) -> Option<String> {
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
    for (i, (_, filtered)) in want.iter().enumerate() {
        let Some(t) = &got[i] else { continue };
        if t.len() > FILE && i != theme {
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
    (!out.is_empty()).then_some(out)
}
