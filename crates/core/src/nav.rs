//! Grid selection math. Pure, so it is unit-tested without a window.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Columns that fit in `width` px given card width and gap (at least 1).
pub fn columns(width: f32, card_w: f32, gap: f32) -> usize {
    (((width + gap) / (card_w + gap)).floor() as usize).max(1)
}

/// Columns for cards of roughly `target` px that fill `avail` evenly (at least 1).
pub fn fit_cols(avail: f32, target: f32) -> usize {
    ((avail / target.max(1.0)).round() as usize).max(1)
}

/// Result of a 2D move inside a grid that sits in a taller page.
#[derive(Debug, PartialEq, Eq)]
pub enum Move {
    Inside(usize),
    /// Fell off the top or bottom edge: the page should move to the neighbouring row.
    Leave(Dir),
}

pub fn grid_move(sel: usize, cols: usize, len: usize, d: Dir) -> Move {
    let n = step(sel, cols, len, d);
    if n == sel.min(len.saturating_sub(1)) && matches!(d, Dir::Up | Dir::Down) {
        Move::Leave(d)
    } else {
        Move::Inside(n)
    }
}

/// Next selected index in a row-major grid of `len` items with `cols` columns.
/// Left/Right never wrap rows; Down from a short last row lands on the last item.
pub fn step(sel: usize, cols: usize, len: usize, d: Dir) -> usize {
    if len == 0 {
        return 0;
    }
    let cols = cols.max(1);
    let sel = sel.min(len - 1);
    match d {
        Dir::Left if !sel.is_multiple_of(cols) => sel - 1,
        Dir::Right if sel % cols + 1 < cols && sel + 1 < len => sel + 1,
        Dir::Up if sel >= cols => sel - cols,
        Dir::Down if sel + cols < len => sel + cols,
        Dir::Down if sel / cols < (len - 1) / cols => len - 1,
        _ => sel,
    }
}

/// Browser-style history over a page stack (index 0 is Home and never leaves). `back` parks the
/// top page so `forward` can restore it exactly as it was; a fresh navigation forgets them.
pub fn back<T>(stack: &mut Vec<T>, fwd: &mut Vec<T>) -> bool {
    if stack.len() > 1 {
        fwd.extend(stack.pop());
        return true;
    }
    false
}

pub fn forward<T>(stack: &mut Vec<T>, fwd: &mut Vec<T>) -> bool {
    match fwd.pop() {
        Some(p) => {
            stack.push(p);
            true
        }
        None => false,
    }
}

/// Which of `len` breadcrumbs to show when at most `max` fit: `(start, elided)` where `start`
/// is the first shown index and `elided` says a "..." stands in for the hidden ones
/// (the first crumb, Home, always stays).
pub fn crumb_window(len: usize, max: usize) -> (usize, bool) {
    let max = max.max(2);
    if len <= max {
        (0, false)
    } else {
        (len - (max - 1), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Dir::*;

    #[test]
    fn columns_fit() {
        assert_eq!(columns(1000.0, 180.0, 20.0), 5);
        assert_eq!(columns(50.0, 180.0, 20.0), 1);
    }

    #[test]
    fn moves_inside_grid() {
        // 3 cols, 7 items: rows [0 1 2] [3 4 5] [6]
        assert_eq!(step(0, 3, 7, Right), 1);
        assert_eq!(step(2, 3, 7, Right), 2, "no wrap");
        assert_eq!(step(3, 3, 7, Left), 3, "no wrap");
        assert_eq!(step(1, 3, 7, Down), 4);
        assert_eq!(step(4, 3, 7, Up), 1);
        assert_eq!(step(1, 3, 7, Up), 1);
    }

    #[test]
    fn short_last_row() {
        assert_eq!(step(5, 3, 7, Down), 6, "lands on last item");
        assert_eq!(step(6, 3, 7, Down), 6);
        assert_eq!(step(6, 3, 7, Right), 6);
        assert_eq!(step(6, 3, 7, Up), 3);
    }

    #[test]
    fn card_columns_fill_evenly() {
        assert_eq!(fit_cols(1800.0, 340.0), 5);
        assert_eq!(fit_cols(1184.0, 340.0), 3);
        assert_eq!(fit_cols(300.0, 340.0), 1);
        assert_eq!(fit_cols(0.0, 340.0), 1);
    }

    #[test]
    fn grid_moves_or_leaves() {
        // 3 cols, 7 items.
        assert_eq!(grid_move(4, 3, 7, Down), Move::Inside(6));
        assert_eq!(grid_move(6, 3, 7, Down), Move::Leave(Down));
        assert_eq!(grid_move(1, 3, 7, Up), Move::Leave(Up));
        assert_eq!(grid_move(4, 3, 7, Up), Move::Inside(1));
        assert_eq!(grid_move(0, 3, 7, Right), Move::Inside(1));
        assert_eq!(
            grid_move(2, 3, 7, Right),
            Move::Inside(2),
            "edges do not wrap"
        );
    }

    #[test]
    fn degenerate() {
        assert_eq!(step(0, 3, 0, Down), 0);
        assert_eq!(step(9, 3, 2, Left), 0, "stale index is clamped");
        assert_eq!(step(0, 0, 3, Down), 1, "zero cols acts as one column");
    }

    #[test]
    fn history_back_forward() {
        let (mut s, mut f) = (vec!["home", "a", "b"], vec![]);
        assert!(back(&mut s, &mut f) && s == ["home", "a"] && f == ["b"]);
        assert!(back(&mut s, &mut f) && f == ["b", "a"]);
        assert!(!back(&mut s, &mut f), "home stays");
        assert!(forward(&mut s, &mut f) && s == ["home", "a"]);
        assert!(forward(&mut s, &mut f) && s == ["home", "a", "b"]);
        assert!(!forward(&mut s, &mut f));
    }

    #[test]
    fn crumbs_elide_the_middle() {
        assert_eq!(crumb_window(3, 4), (0, false));
        assert_eq!(crumb_window(4, 4), (0, false));
        assert_eq!(crumb_window(7, 4), (4, true), "Home, ..., last three");
        assert_eq!(crumb_window(2, 0), (0, false));
    }
}
