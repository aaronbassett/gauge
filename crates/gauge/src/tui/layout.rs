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
            // A fixed height of 0 would make `solve` terminate the layout early
            // (dropping this panel and everything after it), so clamp it to >= 1.
            height: p.height.as_ref().and_then(Height::rows).map(|h| h.max(1)),
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

/// Assign a `Rect` to each cell inside `area` over a 12-column grid.
///
/// Row heights: a row containing any fixed-height cell takes the max fixed height in
/// that row; the remaining vertical space is split evenly among flexible rows (each
/// at least 1 line, with any rounding remainder handed to the earliest flexible rows).
/// Within a row, each cell's width is `span * (area.width / 12)`; the last cell in a
/// row absorbs the rounding remainder up to its grid edge.
///
/// When the area is too short for every row, the straddling row is truncated to the
/// space that remains and any rows beyond it keep their default zero `Rect` (the
/// renderer skips zero-sized rects).
pub fn solve(area: Rect, cells: &[Cell]) -> Vec<Rect> {
    let mut out = vec![Rect::default(); cells.len()];
    if cells.is_empty() || area.width == 0 || area.height == 0 {
        return out;
    }
    let rows = partition_rows(cells);

    // Per-row fixed height (max of fixed cells), or None if the row is fully flexible.
    let row_fixed: Vec<Option<u16>> = rows
        .iter()
        .map(|r| r.iter().filter_map(|&i| cells[i].height).max())
        .collect();
    let fixed_total: u16 = row_fixed.iter().flatten().sum();
    let flex_count = row_fixed.iter().filter(|h| h.is_none()).count() as u16;
    let remaining = area.height.saturating_sub(fixed_total);
    let flex_each = if flex_count > 0 {
        (remaining / flex_count).max(1)
    } else {
        0
    };
    let mut flex_rem = if flex_count > 0 {
        remaining.saturating_sub(flex_each * flex_count)
    } else {
        0
    };

    let col_unit = area.width / 12;
    let mut y = area.y;
    let bottom = area.y.saturating_add(area.height);

    for (ri, row) in rows.iter().enumerate() {
        let mut row_h = match row_fixed[ri] {
            Some(h) => h,
            None => {
                let mut h = flex_each;
                if flex_rem > 0 {
                    h += 1;
                    flex_rem -= 1;
                }
                h
            }
        };
        // Never run past the bottom of the area.
        row_h = row_h.min(bottom.saturating_sub(y));
        if row_h == 0 {
            break;
        }

        let mut x = area.x;
        let mut used_cols = 0u16;
        for (pos, &i) in row.iter().enumerate() {
            let span = cells[i].span.clamp(1, 12);
            let grid_edge = area.x + (used_cols + span).min(12) * col_unit;
            let width = if pos + 1 == row.len() {
                grid_edge.saturating_sub(x).max(1)
            } else {
                (span * col_unit).max(1)
            };
            out[i] = Rect {
                x,
                y,
                width,
                height: row_h,
            };
            x = x.saturating_add(width);
            used_cols += span;
        }

        y = y.saturating_add(row_h);
        if y >= bottom {
            break;
        }
    }
    out
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

    #[test]
    fn solve_places_a_full_row_across_the_width() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let c = cells(&[12]); // one full-width flexible row
        let rects = solve(area, &c);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[0].y, 0);
        assert_eq!(rects[0].width, 120);
        assert_eq!(rects[0].height, 40); // only flexible row → takes all height
    }

    #[test]
    fn solve_splits_a_row_by_span_and_stacks_rows() {
        // width 120 → col_unit 10. Row A: 4x span-3 fixed height 4. Row B: span-12 fill.
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 24,
        };
        let c = vec![
            Cell {
                span: 3,
                height: Some(4),
            },
            Cell {
                span: 3,
                height: Some(4),
            },
            Cell {
                span: 3,
                height: Some(4),
            },
            Cell {
                span: 3,
                height: Some(4),
            },
            Cell {
                span: 12,
                height: None,
            },
        ];
        let rects = solve(area, &c);
        // Row A: y=0, each height 4, widths 30 each, x at 0/30/60/90.
        for (i, x) in [0u16, 30, 60, 90].iter().enumerate() {
            assert_eq!(rects[i].y, 0);
            assert_eq!(rects[i].height, 4);
            assert_eq!(rects[i].x, *x);
            assert_eq!(rects[i].width, 30);
        }
        // Row B: starts at y=4, gets remaining height 24-4=20, full width.
        assert_eq!(rects[4].y, 4);
        assert_eq!(rects[4].height, 20);
        assert_eq!(rects[4].width, 120);
    }

    #[test]
    fn solve_truncates_rows_that_dont_fit() {
        // Two fixed rows want 4+4=8 lines but the area is only 5 tall.
        let area = Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 5,
        };
        let c = vec![
            Cell {
                span: 12,
                height: Some(4),
            },
            Cell {
                span: 12,
                height: Some(4),
            },
        ];
        let rects = solve(area, &c);
        // First row gets its 4 lines; the second is truncated to the remaining 1.
        assert_eq!(rects[0].height, 4);
        assert_eq!(rects[0].y, 0);
        assert_eq!(rects[1].height, 1);
        assert_eq!(rects[1].y, 4);
    }

    #[test]
    fn from_spec_clamps_zero_height_to_one() {
        use crate::tui::config::{Height, PanelSpec};
        let spec = PanelSpec {
            kind: "stat".into(),
            span: 3,
            height: Some(Height::Rows(0)),
            title: None,
            metric: Some("events".into()),
            metrics: vec![],
            group_by: None,
            field: None,
            measure: None,
            limit: None,
            attr: None,
            edges: vec![],
            hidden: false,
            filters: vec![],
        };
        assert_eq!(Cell::from_spec(&spec).height, Some(1));
    }

    #[test]
    fn solve_is_safe_for_zero_area() {
        let rects = solve(Rect::default(), &cells(&[6, 6]));
        assert_eq!(rects.len(), 2);
        assert!(rects.iter().all(|r| r.width == 0 && r.height == 0));
    }
}
