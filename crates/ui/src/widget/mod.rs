//! Component kit: ports of the GPUI `ui.rs` helpers. Each returns a plain element; state stays in
//! the caller. Colours come from `theme`, shapes from `style`.
pub mod controls;
pub mod layout;
pub mod popover;
pub mod press;
pub mod tex;
pub mod text;

pub use controls::*;
pub use layout::*;
pub use text::{label, mono, tracked, wordmark};
pub mod buffering;
