//! Library grid math, pure: where a direction moves the selection, and
//! which rows need their covers.

use std::ops::Range;

/// A direction in the grid, as numbered by the Slint side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

impl Move {
    pub fn from_index(i: i32) -> Option<Move> {
        Some(match i {
            0 => Move::Left,
            1 => Move::Right,
            2 => Move::Up,
            3 => Move::Down,
            _ => return None,
        })
    }
}

/// Index selected after `m` from `index`, among `count` items laid out
/// `cols` per row. `None` leaves the grid upward (to the filters). A move
/// against an edge stays put; down from a row above a shorter last row
/// lands on the last item.
pub fn step(index: usize, count: usize, cols: usize, m: Move) -> Option<usize> {
    let cols = cols.max(1);
    if count == 0 {
        return None;
    }
    let index = index.min(count - 1);
    let (row, col) = (index / cols, index % cols);
    let last_row = (count - 1) / cols;
    Some(match m {
        Move::Left if col > 0 => index - 1,
        Move::Right if col + 1 < cols && index + 1 < count => index + 1,
        Move::Up if row == 0 => return None,
        Move::Up => index - cols,
        Move::Down if index + cols < count => index + cols,
        Move::Down if row < last_row => count - 1,
        _ => index,
    })
}

/// Rows whose covers are loaded: the visible ones (`first`, `visible` of
/// them) and one more on each side, so a step shows art at once.
pub fn wanted_rows(first: usize, visible: usize, rows: usize) -> Range<usize> {
    let start = first.saturating_sub(1).min(rows);
    start..(first + visible + 1).min(rows)
}

/// Rows whose covers may stay in memory: one more row on each side, so
/// going back and forth does not decode again.
pub fn kept_rows(wanted: &Range<usize>, rows: usize) -> Range<usize> {
    wanted.start.saturating_sub(1)..(wanted.end + 1).min(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Move::*;

    // 14 items, 6 per row:
    //  0  1  2  3  4  5
    //  6  7  8  9 10 11
    // 12 13
    #[test]
    fn moves_inside_the_grid() {
        assert_eq!(step(7, 14, 6, Left), Some(6));
        assert_eq!(step(7, 14, 6, Right), Some(8));
        assert_eq!(step(7, 14, 6, Up), Some(1));
        assert_eq!(step(7, 14, 6, Down), Some(13));
    }

    #[test]
    fn edges_stop_and_up_leaves() {
        assert_eq!(step(6, 14, 6, Left), Some(6));
        assert_eq!(step(5, 14, 6, Right), Some(5));
        assert_eq!(step(13, 14, 6, Right), Some(13));
        assert_eq!(step(3, 14, 6, Up), None);
        assert_eq!(step(12, 14, 6, Down), Some(12));
    }

    #[test]
    fn down_onto_a_short_last_row() {
        // Nothing under 10: the last item, not nowhere.
        assert_eq!(step(10, 14, 6, Down), Some(13));
        assert_eq!(step(4, 5, 6, Down), Some(4));
    }

    #[test]
    fn degenerate_grids() {
        assert_eq!(step(0, 0, 6, Down), None);
        assert_eq!(step(40, 14, 6, Left), Some(12), "stale index clamps");
        assert_eq!(step(2, 5, 0, Down), Some(3), "0 columns means 1");
        assert_eq!(Move::from_index(3), Some(Down));
        assert_eq!(Move::from_index(9), None);
    }

    #[test]
    fn cover_rows() {
        assert_eq!(wanted_rows(0, 2, 10), 0..3);
        assert_eq!(wanted_rows(4, 2, 10), 3..7);
        assert_eq!(wanted_rows(8, 3, 10), 7..10);
        assert_eq!(wanted_rows(0, 3, 0), 0..0);
        assert_eq!(kept_rows(&(3..7), 10), 2..8);
        assert_eq!(kept_rows(&(0..3), 3), 0..3);
    }
}
