//! Jellyfin layout mode (`docs/dev/V052_PLAN.md` item 10): when the source in view uses a server's UI, Home, the header, the library grid and the detail page are built with jellyfin-web's own page structure (the same element kinds, class names, ids, data attributes and nesting for the parts NOCTURNE shows), as engine element trees. The base stylesheet is the server's own jellyfin-web CSS fetched at run time (`nocturne_core::webcss`), then the server's branding CSS, then the user's theme, in the web client's order, in one sheet layer; the Jellyfin selector translator is off because the classes are native. `jf_ua.css` is the little a browser would add under those sheets (default display and margins) plus the pieces jellyfin-web draws with MUI's run-time styles, written here by their stable `Mui*` class names. Interactive leaves stay slots; focus and keys come from the scene as before.
pub(crate) mod card;
mod header;
mod home;
mod library;

use super::App;
pub(crate) use home::second_line;
use std::sync::Arc;

/// What a browser adds under the web client's own sheets, and the MUI pieces.
pub(crate) const UA: &str = include_str!("jf_ua.css");

impl App {
    /// The base stylesheet of the server whose UI is in use for what is on screen, when the layout mode applies (the server has its web client).
    pub(super) fn jf_base(&self) -> Option<Arc<String>> {
        let ui = self.theme_ui()?;
        let web = self.server_ui.get(&ui)?.web_css.clone()?;
        // Debug builds: `NOCTURNE_JF_BASE=<file>` stands in for the fetched sheet (bisecting what an engine gap does to a page).
        if cfg!(debug_assertions)
            && let Ok(p) = std::env::var("NOCTURNE_JF_BASE")
        {
            static OVERRIDE: std::sync::OnceLock<Arc<String>> = std::sync::OnceLock::new();
            return Some(
                OVERRIDE
                    .get_or_init(|| Arc::new(std::fs::read_to_string(p).unwrap_or_default()))
                    .clone(),
            );
        }
        Some(web)
    }

    /// The file name of the web client's own logo (`icon-transparent.<hash>.png`), as its stylesheet names it.
    pub(super) fn jf_logo(&self) -> Option<String> {
        let css = self.jf_base()?;
        let i = css.find("icon-transparent.")?;
        let end = i + css[i..].find(".png")? + 4;
        let name = &css[i..end];
        name.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            .then(|| name.to_string())
    }

    /// The server's branding CSS and the user's own theme as one text, in the web client's order (imports inlined once fetched). The compat translator never sees it.
    pub(super) fn jf_user_text(&self) -> String {
        let (src, ui) = (self.theme_source(), self.theme_ui());
        let resolved = |css: &str| -> String {
            self.themes
                .resolved_for(css)
                .map_or(css, |r| r.css.as_str())
                .to_string()
        };
        let mut out = String::new();
        // The stock header logo, under any theme's own rule for it.
        if let Some(l) = self.jf_logo() {
            out += &format!(
                ".MuiToolbar-root .MuiButton-startIcon img.jf-brand {{ content: url(\"{l}\"); height: 24px; width: auto; }}\n"
            );
        }
        if let Some(b) = ui.and_then(|u| self.themes.branding_of(u)) {
            out += &resolved(b);
            out.push('\n');
        }
        // The user's own choice layers over the branding (it replaces it in NOCTURNE's own layout).
        let own = src
            .and_then(|s| self.themes.override_of(s))
            .or(self.themes.global);
        if let Some(t) = own.and_then(|i| self.themes.get(i)) {
            out += &resolved(&t.css);
        }
        out
    }
}

/// Mark the root as jellyfin-web's `html` (`layout-desktop`, `data-theme`) with `body > #reactRoot` under it; returns `#reactRoot`.
pub(super) fn jf_root(tree: &mut super::header_tree::Tree, screen_class: &str) -> usize {
    let d = &mut tree.doc;
    d.nodes[0].tag = "html".into();
    d.nodes[0].classes.push(
        if crate::couch::on() {
            "layout-tv"
        } else {
            "layout-desktop"
        }
        .into(),
    );
    d.attr(0, "data-theme", "dark");
    d.attr(0, "dir", "ltr");
    let body = d.add(0, &format!("body.{screen_class}"));
    d.attr(body, "dir", "ltr");
    // `#reactRoot`: a column as high as the window, the page area below the header's spacer growing to fill it.
    let root = d.add(body, "div.jf-react");
    d.set_id(root, "reactRoot");
    // Tabbed pages always have the page-sized `.backgroundContainer` (themes tint it: Jellyfish's translucent teal over black); the item page adds its own with a backdrop.
    if screen_class.starts_with("libraryDocument") {
        let bc = d.add(root, "div.backdropContainer");
        d.add(bc, "div.backgroundContainer");
    }
    root
}
