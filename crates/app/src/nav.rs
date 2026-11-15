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
    fn degenerate() {
        assert_eq!(step(0, 3, 0, Down), 0);
        assert_eq!(step(9, 3, 2, Left), 0, "stale index is clamped");
        assert_eq!(step(0, 0, 3, Down), 1, "zero cols acts as one column");
    }
}
