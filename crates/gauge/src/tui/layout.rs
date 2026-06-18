//! The dashboard layout solver: flow panels left-to-right across a 12-column grid,
//! wrapping to a new row on overflow, then assign each panel a `Rect`.

use ratatui::layout::Rect;

use crate::tui::config::{Height, PanelSpec};

/// A panel's grid footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Grid columns, clamped to 1..=12.
    pub span: u16,
    /// Fixed terminal rows, or `None` to share leftover vertical space.
    pub height: Option<u16>,
}

impl Cell {
    pub fn from_spec(p: &PanelSpec) -> Cell {
        Cell {
            span: p.span.clamp(1, 12),
            height: p.height.as_ref().and_then(Height::rows),
        }
    }
}

/// Group cell indices into rows; a new row begins when the next cell would push the
/// running column total past 12.
pub fn partition_rows(cells: &[Cell]) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut used = 0u16;
    for (i, c) in cells.iter().enumerate() {
        let span = c.span.clamp(1, 12);
        if used + span > 12 && !cur.is_empty() {
            rows.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(i);
        used += span;
    }
    if !cur.is_empty() {
        rows.push(cur);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(spans: &[u16]) -> Vec<Cell> {
        spans
            .iter()
            .map(|&span| Cell { span, height: None })
            .collect()
    }

    #[test]
    fn partitions_the_default_layout_into_four_rows() {
        // timeseries(12), 4x stat(3), top_n(6)+numeric_stats(6), 3x breakdown(4)
        let c = cells(&[12, 3, 3, 3, 3, 6, 6, 4, 4, 4]);
        let rows = partition_rows(&c);
        assert_eq!(
            rows,
            vec![vec![0], vec![1, 2, 3, 4], vec![5, 6], vec![7, 8, 9],]
        );
    }

    #[test]
    fn a_single_overfull_cell_still_gets_its_own_row() {
        let c = cells(&[12, 12]);
        assert_eq!(partition_rows(&c), vec![vec![0], vec![1]]);
    }
}
