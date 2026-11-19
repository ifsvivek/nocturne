//! Spike for the stage-2 theme engine (see `docs/dev/THEME_ENGINE_PLAN.md`): an element tree per screen, a CSS cascade, Taffy layout.
//! UI-free like `nocturne-core`; the Iced widget that paints a [`Scene`] lives in the app crate's debug preview.
pub mod cascade;
pub mod demo;
pub mod doc;
pub mod layout;
pub mod props;
pub mod select;

pub use taffy;

pub use cascade::{Sheet, flags};
pub use doc::{Doc, Id};
pub use layout::{Scene, TextStyle, approx};

/// The three example themes of the spike (a fourth, `dock`, moves the header to the bottom edge).
pub const THEMES: &[(&str, &str)] = &[
    ("streaming", include_str!("../themes/streaming.css")),
    ("videosite", include_str!("../themes/videosite.css")),
    ("rail", include_str!("../themes/rail.css")),
    ("dock", include_str!("../themes/dock.css")),
];

#[cfg(test)]
mod tests;
