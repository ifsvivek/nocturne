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
pub async fn fetch(client: &jf::Client, scope: &str) -> Option<WebBase> {
    let files = client.web_files().await.ok()??;
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
            return Some(WebBase { css, font });
        }
    }
    let (mut css, names, face) = assemble(client, &files).await?;
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
    Some(WebBase { css, font })
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

    #[test]
    fn keeps_the_base_and_the_wanted_icon_rules_only() {
        let t = ".x{a:b}.material-icons{font-family:Material Icons}\n.material-icons.zoom_in:before{content:\"a\"}\n.material-icons.play_arrow:before{content:\"b\"}\n";
        let (base, names) = icon_rules(t);
        assert_eq!(base, ".material-icons{font-family:Material Icons}\n");
        assert_eq!(names, ".material-icons.play_arrow:before{content:\"b\"}\n");
    }
}
