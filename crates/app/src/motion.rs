//! The reduce-motion preference, resolved from Settings + the desktop (see `platform`) and read by
//! every animation. One process-wide flag: it is an OS-level preference, not screen state.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static REDUCED: AtomicBool = AtomicBool::new(false);

pub fn set_reduced(on: bool) {
    REDUCED.store(on, Ordering::Relaxed);
}

pub fn reduced() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

/// `d`, or zero when motion is reduced (instant state change).
pub fn dur(d: Duration) -> Duration {
    if reduced() { Duration::ZERO } else { d }
}

/// Cubic ease-out: fast start, soft landing. `t` is clamped to 0..=1.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_is_monotonic_and_bounded() {
        assert_eq!(
            (ease_out(-1.0), ease_out(0.0), ease_out(1.0), ease_out(9.0)),
            (0.0, 0.0, 1.0, 1.0)
        );
        assert!(ease_out(0.3) > 0.3 && ease_out(0.3) < ease_out(0.6));
    }
}
