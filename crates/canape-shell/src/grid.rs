//! Library shelf math, pure: which covers need decoding. A shelf is a
//! grid of one item per row, so a "row" here is one game.

use std::ops::Range;

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
