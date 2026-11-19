//! The stage-2 theme engine (see `docs/dev/THEME_ENGINE_PLAN.md`): an element tree per screen, a CSS cascade, Taffy layout,
//! transitions, focus order and spatial navigation, bounded and safe. UI-free like `nocturne-core`; the Iced widget that paints a
//! [`Scene`] lives in the app crate (`widget/styled.rs`).
pub mod anim;
pub mod calc;
pub mod cascade;
pub mod demo;
pub mod doc;
pub mod engine;
pub mod fonts;
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

#[cfg(test)]
mod tests;
