//! The stage-2 theme engine (see `docs/dev/THEME_ENGINE_PLAN.md`): an element tree per screen, a CSS cascade, Taffy layout,
//! transitions, focus order and spatial navigation, bounded and safe. UI-free like `nocturne-core`; the Iced widget that paints a
//! [`Scene`] lives in the app crate (`widget/styled.rs`).
pub mod anim;
pub mod calc;
pub mod cascade;
pub mod compat;
pub mod compat_map;
pub mod css_edit;
pub mod demo;
pub mod doc;
pub mod engine;
pub mod fonts;
pub mod inspect;
pub mod layout;
pub mod nav;
pub mod paint;
pub mod props;
pub mod select;
pub mod sheet;
pub mod text;
pub mod virt;
pub mod warn;

pub use taffy;

pub use anim::Eff;
pub use cascade::{Budget, Failure, Interaction, flags};
pub use doc::{Doc, Gen, Id, ImageRef};
pub use engine::{Engine, SAFE_CHORD, SAFE_ENV, safe_env_enabled};
pub use fonts::{FontChoice, Fonts};
pub use layout::{Scene, Update};
pub use nav::Dir;
pub use sheet::Sheet;
pub use text::{Block, MeasureCache, TextStyle, approx};
pub use virt::{Builders, Grid, VirtBuilder};
pub use warn::{Kind, Report};

/// The example themes of the spike (a fourth, `dock`, moves the header to the bottom edge).
pub const THEMES: &[(&str, &str)] = &[
    ("streaming", include_str!("../themes/streaming.css")),
    ("videosite", include_str!("../themes/videosite.css")),
    ("rail", include_str!("../themes/rail.css")),
    ("dock", include_str!("../themes/dock.css")),
];

/// Does this stylesheet use the vocabulary only the element trees emit (classes of the detail page, the player's bar, the section titles...)? Stage-1 themes (Marquee, Feed, Classic and the like) keep running through the tree builders' translator; a sheet like this one is layered over the default sheet as it is. Interim rule until the Theme Studio marks a theme as an engine theme.
pub fn is_engine_sheet(css: &str) -> bool {
    // The Theme Studio marks its themes.
    if css.contains("nocturne:engine") {
        return true;
    }
    // The answer for a Jellyfin sheet needs a translation: remember the last sheet asked about (the app asks on every update).
    thread_local!(static LAST: std::cell::Cell<Option<(u64, bool)>> = const { std::cell::Cell::new(None) });
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    css.hash(&mut h);
    let k = h.finish();
    if let Some((p, v)) = LAST.get()
        && p == k
    {
        return v;
    }
    let v = engine_sheet(css);
    LAST.set(Some((k, v)));
    v
}

fn engine_sheet(css: &str) -> bool {
    // `.detail-title`, `.detail-text`, `.detail-backdrop` and `.section-latest` are stage-1 names and do not count.
    const MARKS: &[&str] = &[
        ".detail-hero",
        ".detail-actions",
        ".detail-summary",
        ".detail-meta",
        ".detail-genres",
        ".detail-chips",
        ".detail-more",
        ".detail-top",
        ".detail-body",
        ".detail-col",
        ".detail-art",
        ".detail-logo",
        ".detail-tags",
        ".detail-facts",
        ".detail-about",
        ".detail-info",
        ".section-title",
        ".section-head",
        ".section-box",
        ".section-strip",
        ".section-text",
        ".poster",
        ".episode",
        ".entry",
        ".player-",
        ".header-bar",
        ".button-primary",
        ".round",
        ".tab",
    ];
    MARKS.iter().any(|m| css.contains(m)) || compat::translate(css).1.applied
}

/// Built-in starting points for the Themes page: engine stylesheets written on the vocabulary the tree builders emit.
pub const STARTERS: &[(&str, &str)] = &[
    ("Spotlight", include_str!("../themes/spotlight.css")),
    ("Glass", include_str!("../themes/glass.css")),
];

#[cfg(test)]
mod tests;
