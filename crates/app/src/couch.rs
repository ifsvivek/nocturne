//! Couch / TV mode, resolved from Settings + gamepad + display (see `app/couch.rs`) and read by the
//! few widgets that look different at ten feet. One process-wide flag, like `motion`. The mode
//! itself scales the whole UI (Iced's application scale factor), so type, targets and spacing grow
//! together from the theme tokens.
use crate::theme as t;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);

pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Application scale factor for the current mode.
pub fn factor() -> f32 {
    if on() { t::COUCH_SCALE } else { 1.0 }
}

/// Width of the focus ring on a card or control.
pub fn ring() -> f32 {
    if on() { 3.0 } else { 1.0 }
}
