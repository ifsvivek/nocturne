//! Component kit: ports of the GPUI `ui.rs` helpers. Each returns a plain element; state stays in
//! the caller. Colours come from `theme`, shapes from `style`.
pub mod cell;
pub mod controls;
pub mod layout;
pub mod overlay;
#[cfg(feature = "theme-engine")]
pub mod pics;
pub mod popover;
pub mod press;
pub mod source;
#[cfg(feature = "theme-engine")]
pub mod styled;
#[cfg(feature = "theme-engine")]
pub mod styled_preview;
#[cfg(all(test, feature = "theme-engine"))]
mod styled_tests;
pub mod tex;
pub mod text;

pub use controls::*;
pub use layout::*;
pub use source::{dot, source_mark, status_color};
pub use text::{label, mono, tracked, wordmark};
pub mod buffering;
pub mod hole;
pub mod probe;
pub mod track;
pub mod zoom;
