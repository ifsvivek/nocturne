//! Virtualised list math: which rows of a fixed-row-height list intersect the scroll viewport
//! (plus a little overscan), and the offset that brings a row into view. The view builds only
//! those rows between two spacers, so a 5,000-item library costs the same as a 50-item one.
use std::ops::Range;

/// Rows to build for a viewport `vh` tall scrolled by `offset`, `overscan` rows beyond each edge.
pub fn visible_rows(
    offset: f32,
    vh: f32,
    row_h: f32,
    rows: usize,
    overscan: usize,
) -> Range<usize> {
    if rows == 0 || row_h <= 0.0 {
        return 0..0;
    }
    let first = (offset.max(0.0) / row_h).floor() as usize;
    let last = ((offset.max(0.0) + vh.max(0.0)) / row_h).ceil() as usize;
    first.saturating_sub(overscan).min(rows)..(last + overscan).min(rows)
}

/// Items covered by a row range (`cols` per row), clipped to `n`.
pub fn items_of(rows: &Range<usize>, cols: usize, n: usize) -> Range<usize> {
    (rows.start * cols).min(n)..(rows.end * cols).min(n)
}

/// The smallest scroll that makes row `r` fully visible (the offset itself when it already is).
pub fn into_view(offset: f32, vh: f32, row_h: f32, r: usize) -> f32 {
    let top = r as f32 * row_h;
    if top < offset {
        top
    } else if top + row_h > offset + vh {
        (top + row_h - vh).max(0.0)
    } else {
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_rows_in_the_viewport_plus_overscan_are_built() {
        // 1000 rows of 100 px: a 800 px viewport at 5,000 px shows rows 50..58.
        let r = visible_rows(5000.0, 800.0, 100.0, 1000, 2);
        assert_eq!(r, 48..60);
        assert!(r.len() < 20, "never thousands of widgets");
        // Start and end clamp.
        assert_eq!(visible_rows(0.0, 800.0, 100.0, 1000, 2), 0..10);
        assert_eq!(visible_rows(99_900.0, 800.0, 100.0, 1000, 2), 997..1000);
        // A partly scrolled row counts as visible.
        assert_eq!(visible_rows(150.0, 100.0, 100.0, 10, 0), 1..3);
        assert_eq!(visible_rows(0.0, 800.0, 100.0, 0, 2), 0..0);
    }

    #[test]
    fn rows_map_to_items_and_clip_to_the_count() {
        assert_eq!(items_of(&(2..4), 5, 18), 10..18);
        assert_eq!(items_of(&(9..12), 5, 18), 18..18);
    }

    #[test]
    fn keyboard_scroll_moves_only_as_far_as_needed() {
        assert_eq!(into_view(0.0, 500.0, 100.0, 2), 0.0, "already visible");
        assert_eq!(
            into_view(0.0, 500.0, 100.0, 5),
            100.0,
            "just below: bottom edge"
        );
        assert_eq!(into_view(400.0, 500.0, 100.0, 1), 100.0, "above: top edge");
        assert_eq!(into_view(0.0, 500.0, 100.0, 0), 0.0);
    }
}
