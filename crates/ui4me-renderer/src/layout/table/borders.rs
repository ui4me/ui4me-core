//! owner: 008-layout-engine (T104, L-TBL-r4) — the collapsed-border model.
//!
//! Clean-room (T098). CSS 2.1 §17.6.2: in the collapsed model the borders of table, column groups,
//! columns, row groups, rows and cells that meet on a grid-line segment collapse to one border. The
//! winner of a segment is chosen by §17.6.2.1:
//!
//! 1. a `hidden` border wins over all others (the segment has no border);
//! 2. `none` has the lowest priority, so any other style shows;
//! 3. the wider border wins;
//! 4. at equal width the style wins in the order `double`, `solid`, `dashed`, `dotted`, `ridge`,
//!    `outset`, `groove`, `inset`;
//! 5. at equal style the origin wins in the order cell, row, row group, column, column group, table;
//! 6. at equal origin the border furthest to the left (top) wins.
//!
//! [`resolve_borders`] gathers the candidates of every segment of the grid, returns the winners,
//! the border widths that layout gives the cells and the table (half of the collapsed width on each
//! side of a line; the outer half of the table's edge belongs to the table) and the merged
//! [`CollapsedBorderSegment`]s that paint publishes. Everything is a pure function of the style
//! values handed in.

#![allow(clippy::needless_range_loop)] // grid indices (row, line, column) are the point of this module

use std::ops::Range;

use ui4me_engine_types::geometry::layout_unit::LayoutUnit;

use super::grid::TableStructure;
use crate::layout::box_tree::BoxId;
use crate::layout::fragment::{BorderLineStyle, CollapsedBorderSegment};

/// Where a border comes from, in increasing priority (rule 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Table,
    ColumnGroup,
    Column,
    RowGroup,
    Row,
    Cell,
}

/// One side of a table-family box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSpec {
    pub style: BorderLineStyle,
    /// Used width in px (0 for `none` and `hidden`).
    pub width: f32,
    /// The colour is opaque (alpha 255); the colour itself is resolved by the scene.
    pub opaque: bool,
    /// Which side of `box_id` (0 top, 1 right, 2 bottom, 3 left).
    pub side: u8,
    pub origin: Origin,
    pub box_id: BoxId,
}

/// Top, right, bottom, left.
pub type SideSpecs = [BorderSpec; 4];
const TOP: usize = 0;
const RIGHT: usize = 1;
const BOTTOM: usize = 2;
const LEFT: usize = 3;

fn style_rank(s: BorderLineStyle) -> u8 {
    use BorderLineStyle::*;
    match s {
        None | Hidden => 0,
        Inset => 1,
        Groove => 2,
        Outset => 3,
        Ridge => 4,
        Dotted => 5,
        Dashed => 6,
        Solid => 7,
        Double => 8,
    }
}

/// The winner among `candidates` (rules 1–6; earlier entries are further left/up). `None` when no
/// border shows: all candidates are `none`, or a `hidden` one wins.
pub fn resolve(candidates: &[BorderSpec]) -> Option<BorderSpec> {
    if candidates
        .iter()
        .any(|c| c.style == BorderLineStyle::Hidden)
    {
        return None;
    }
    let mut best: Option<BorderSpec> = None;
    for c in candidates
        .iter()
        .filter(|c| c.style != BorderLineStyle::None)
    {
        let better = match &best {
            None => true,
            Some(b) => {
                // Strictly better only: ties keep the earlier (left/top) candidate.
                (c.width, style_rank(c.style), c.origin) > (b.width, style_rank(b.style), b.origin)
            }
        };
        if better {
            best = Some(*c);
        }
    }
    best
}

/// Everything the resolution needs from the table's boxes.
pub struct BorderInput<'a> {
    pub structure: &'a TableStructure,
    pub table: SideSpecs,
    /// Row groups with the rows they hold.
    pub row_groups: &'a [(Range<usize>, SideSpecs)],
    pub rows: &'a [SideSpecs],
    /// Column groups with the columns they hold.
    pub col_groups: &'a [(Range<usize>, SideSpecs)],
    /// Per column, the `col` box's borders (`None` for a column without a `col` element).
    pub cols: &'a [Option<SideSpecs>],
    /// Per cell (`structure.cells` order).
    pub cells: &'a [SideSpecs],
}

/// The result of the resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedBorders {
    /// `v[row][line]` (line 0..=columns): the winner of the vertical segment of `row`.
    pub v: Vec<Vec<Option<BorderSpec>>>,
    /// `h[line][col]` (line 0..=rows): the winner of the horizontal segment of `col`.
    pub h: Vec<Vec<Option<BorderSpec>>>,
    /// Border widths the cells take (top, right, bottom, left), per cell: half of the widest
    /// winner along each edge.
    pub cell_borders: Vec<[f32; 4]>,
    /// Border widths of the table box: half of the widest outer winner.
    pub table_border: [f32; 4],
    /// Winners merged along their lines.
    pub segments: Vec<CollapsedBorderSegment>,
}

fn w(b: &Option<BorderSpec>) -> f32 {
    b.map_or(0.0, |b| b.width)
}

/// Resolve every segment of the grid (module docs).
pub fn resolve_borders(input: &BorderInput<'_>) -> ResolvedBorders {
    let t = input.structure;
    let (rows, cols) = (t.row_count, t.column_count);
    let pick = |v: &mut Vec<BorderSpec>, s: BorderSpec| v.push(s);
    // Group lookup: rows -> the group they belong to, columns likewise.
    let row_group_of = |r: usize| {
        input
            .row_groups
            .iter()
            .find(|(range, _)| range.contains(&r))
    };
    let col_group_of = |c: usize| {
        input
            .col_groups
            .iter()
            .find(|(range, _)| range.contains(&c))
    };

    // Vertical segments: line x, row y.
    let mut v = vec![vec![None; cols + 1]; rows];
    for y in 0..rows {
        for x in 0..=cols {
            let mut cand: Vec<BorderSpec> = Vec::new();
            // Left side of the line first (further left wins ties).
            if x == 0 {
                pick(&mut cand, input.table[LEFT]);
            }
            if x > 0
                && let Some(cg) = col_group_of(x - 1).filter(|g| g.0.end == x)
            {
                pick(&mut cand, cg.1[RIGHT]);
            }
            if let Some(cg) = col_group_of(x).filter(|g| g.0.start == x) {
                pick(&mut cand, cg.1[LEFT]);
            }
            if x > 0
                && let Some(Some(c)) = input.cols.get(x - 1)
            {
                pick(&mut cand, c[RIGHT]);
            }
            if let Some(Some(c)) = input.cols.get(x) {
                pick(&mut cand, c[LEFT]);
            }
            if let Some(g) = row_group_of(y) {
                if x == 0 {
                    pick(&mut cand, g.1[LEFT]);
                }
                if x == cols {
                    pick(&mut cand, g.1[RIGHT]);
                }
            }
            if x == 0 {
                pick(&mut cand, input.rows[y][LEFT]);
            }
            if x == cols {
                pick(&mut cand, input.rows[y][RIGHT]);
            }
            let left = (x > 0).then(|| t.cell_at(y, x - 1)).flatten();
            let right = (x < cols).then(|| t.cell_at(y, x)).flatten();
            if let (Some(a), Some(b)) = (left, right)
                && a.id == b.id
            {
                continue; // inside a spanning cell: no border
            }
            if let Some(a) = left.filter(|a| a.col + a.col_span == x) {
                pick(&mut cand, input.cells[a.id][RIGHT]);
            }
            if let Some(b) = right.filter(|b| b.col == x) {
                pick(&mut cand, input.cells[b.id][LEFT]);
            }
            if x == cols {
                pick(&mut cand, input.table[RIGHT]);
            }
            // Later-listed sources of the same origin are further right: keep a stable order by
            // origin priority is irrelevant; `resolve` ranks by origin itself.
            v[y][x] = resolve(&cand);
        }
    }

    // Horizontal segments: line y, column x.
    let mut h = vec![vec![None; cols]; rows + 1];
    for y in 0..=rows {
        for x in 0..cols {
            let mut cand: Vec<BorderSpec> = Vec::new();
            if y == 0 {
                pick(&mut cand, input.table[TOP]);
                if let Some(cg) = col_group_of(x) {
                    pick(&mut cand, cg.1[TOP]);
                }
                if let Some(Some(c)) = input.cols.get(x) {
                    pick(&mut cand, c[TOP]);
                }
            }
            if y > 0 {
                if let Some(g) = row_group_of(y - 1).filter(|g| g.0.end == y) {
                    pick(&mut cand, g.1[BOTTOM]);
                }
                pick(&mut cand, input.rows[y - 1][BOTTOM]);
            }
            if y < rows {
                if let Some(g) = row_group_of(y).filter(|g| g.0.start == y) {
                    pick(&mut cand, g.1[TOP]);
                }
                pick(&mut cand, input.rows[y][TOP]);
            }
            let above = (y > 0).then(|| t.cell_at(y - 1, x)).flatten();
            let below = (y < rows).then(|| t.cell_at(y, x)).flatten();
            if let (Some(a), Some(b)) = (above, below)
                && a.id == b.id
            {
                continue;
            }
            if let Some(a) = above.filter(|a| a.row + a.row_span == y) {
                pick(&mut cand, input.cells[a.id][BOTTOM]);
            }
            if let Some(b) = below.filter(|b| b.row == y) {
                pick(&mut cand, input.cells[b.id][TOP]);
            }
            if y == rows {
                pick(&mut cand, input.table[BOTTOM]);
                if let Some(cg) = col_group_of(x) {
                    pick(&mut cand, cg.1[BOTTOM]);
                }
                if let Some(Some(c)) = input.cols.get(x) {
                    pick(&mut cand, c[BOTTOM]);
                }
            }
            h[y][x] = resolve(&cand);
        }
    }

    // Widths: half of the widest winner along each edge.
    let max_of = |it: &mut dyn Iterator<Item = f32>| it.fold(0.0f32, f32::max);
    let cell_borders: Vec<[f32; 4]> = t
        .cells
        .iter()
        .map(|c| {
            let top = max_of(&mut (c.col..c.col + c.col_span).map(|x| w(&h[c.row][x])));
            let bottom =
                max_of(&mut (c.col..c.col + c.col_span).map(|x| w(&h[c.row + c.row_span][x])));
            let left = max_of(&mut (c.row..c.row + c.row_span).map(|y| w(&v[y][c.col])));
            let right =
                max_of(&mut (c.row..c.row + c.row_span).map(|y| w(&v[y][c.col + c.col_span])));
            [top / 2.0, right / 2.0, bottom / 2.0, left / 2.0]
        })
        .collect();
    let table_border = if rows == 0 || cols == 0 {
        [0.0; 4]
    } else {
        [
            max_of(&mut (0..cols).map(|x| w(&h[0][x]))) / 2.0,
            max_of(&mut (0..rows).map(|y| w(&v[y][cols]))) / 2.0,
            max_of(&mut (0..cols).map(|x| w(&h[rows][x]))) / 2.0,
            max_of(&mut (0..rows).map(|y| w(&v[y][0]))) / 2.0,
        ]
    };

    ResolvedBorders {
        segments: merge_segments(&v, &h),
        v,
        h,
        cell_borders,
        table_border,
    }
}

fn segment(
    line: usize,
    extent: (usize, usize),
    horizontal: bool,
    b: &BorderSpec,
) -> CollapsedBorderSegment {
    CollapsedBorderSegment {
        grid_line: line as u32,
        extent: (extent.0 as u32, extent.1 as u32),
        horizontal,
        width: LayoutUnit::quantize(f64::from(b.width)).value,
        style: b.style,
        opaque: b.opaque,
        origin_side: b.side,
        origin_owner: None,
        origin_box: b.box_id,
    }
}

fn same(a: &BorderSpec, b: &BorderSpec) -> bool {
    a.style == b.style
        && a.width == b.width
        && a.opaque == b.opaque
        && a.side == b.side
        && a.box_id == b.box_id
}

/// Merge neighbouring segments of one line that have the same winner. Vertical lines first (by
/// line, then position), then horizontal ones.
fn merge_segments(
    v: &[Vec<Option<BorderSpec>>],
    h: &[Vec<Option<BorderSpec>>],
) -> Vec<CollapsedBorderSegment> {
    let mut out = Vec::new();
    let rows = v.len();
    let lines_v = v.first().map_or(0, Vec::len);
    for x in 0..lines_v {
        let mut y = 0;
        while y < rows {
            let Some(b) = v[y][x] else {
                y += 1;
                continue;
            };
            let start = y;
            while y < rows && v[y][x].is_some_and(|n| same(&n, &b)) {
                y += 1;
            }
            out.push(segment(x, (start, y), false, &b));
        }
    }
    let lines_h = h.len();
    for (y, line) in h.iter().enumerate().take(lines_h) {
        let cols = line.len();
        let mut x = 0;
        while x < cols {
            let Some(b) = line[x] else {
                x += 1;
                continue;
            };
            let start = x;
            while x < cols && line[x].is_some_and(|n| same(&n, &b)) {
                x += 1;
            }
            out.push(segment(y, (start, x), true, &b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::table::grid::{CellInput, GroupInput, RowInput, assign_slots};

    fn spec(style: BorderLineStyle, width: f32, origin: Origin, id: u32) -> BorderSpec {
        BorderSpec {
            style,
            width: if matches!(style, BorderLineStyle::None | BorderLineStyle::Hidden) {
                0.0
            } else {
                width
            },
            opaque: true,
            side: 0,
            origin,
            box_id: BoxId(id),
        }
    }
    fn none(origin: Origin, id: u32) -> SideSpecs {
        [spec(BorderLineStyle::None, 0.0, origin, id); 4]
    }
    fn solid(w: f32, origin: Origin, id: u32) -> SideSpecs {
        [spec(BorderLineStyle::Solid, w, origin, id); 4]
    }

    #[test]
    fn wider_wins_then_style_then_origin_then_position() {
        let a = spec(BorderLineStyle::Solid, 2.0, Origin::Cell, 1);
        let b = spec(BorderLineStyle::Solid, 5.0, Origin::Table, 2);
        assert_eq!(resolve(&[a, b]).unwrap().box_id, BoxId(2));
        let c = spec(BorderLineStyle::Double, 5.0, Origin::Table, 3);
        assert_eq!(resolve(&[b, c]).unwrap().box_id, BoxId(3));
        let d = spec(BorderLineStyle::Dashed, 5.0, Origin::Cell, 4);
        let e = spec(BorderLineStyle::Dashed, 5.0, Origin::Row, 5);
        assert_eq!(resolve(&[e, d]).unwrap().box_id, BoxId(4));
        // Same width, style and origin: the first (left/top) wins.
        let f = spec(BorderLineStyle::Solid, 3.0, Origin::Cell, 6);
        let g = spec(BorderLineStyle::Solid, 3.0, Origin::Cell, 7);
        assert_eq!(resolve(&[f, g]).unwrap().box_id, BoxId(6));
    }

    #[test]
    fn hidden_wins_and_none_loses() {
        let a = spec(BorderLineStyle::Solid, 9.0, Origin::Cell, 1);
        let h = spec(BorderLineStyle::Hidden, 0.0, Origin::Table, 2);
        assert_eq!(resolve(&[a, h]), None);
        let n = spec(BorderLineStyle::None, 0.0, Origin::Cell, 3);
        assert_eq!(resolve(&[n, a]).unwrap().box_id, BoxId(1));
        assert_eq!(resolve(&[n]), None);
    }

    fn two_by_two<'a>(
        st: &'a TableStructure,
        table: SideSpecs,
        rows: &'a [SideSpecs],
        cells: &'a [SideSpecs],
        no_cols: &'a [Option<SideSpecs>],
    ) -> BorderInput<'a> {
        BorderInput {
            structure: st,
            table,
            row_groups: &[],
            rows,
            col_groups: &[],
            cols: no_cols,
            cells,
        }
    }

    #[test]
    fn acceptance_2_conflict_between_neighbouring_cells() {
        // 1x2: left cell has a 4px dashed right border, right cell a 2px solid left border: the
        // wider (dashed 4px) wins; both cells take half of it.
        let st = assign_slots(&[GroupInput {
            rows: vec![RowInput {
                cells: vec![
                    CellInput {
                        col_span: 1,
                        row_span: 1
                    };
                    2
                ],
            }],
        }]);
        let mut left = none(Origin::Cell, 10);
        left[RIGHT] = spec(BorderLineStyle::Dashed, 4.0, Origin::Cell, 10);
        let mut right = none(Origin::Cell, 11);
        right[LEFT] = spec(BorderLineStyle::Solid, 2.0, Origin::Cell, 11);
        let rows = [none(Origin::Row, 5)];
        let cols = [None, None];
        let cells = [left, right];
        let r = resolve_borders(&two_by_two(
            &st,
            none(Origin::Table, 1),
            &rows,
            &cells,
            &cols,
        ));
        let mid = r.v[0][1].unwrap();
        assert_eq!(
            (mid.style, mid.width, mid.box_id),
            (BorderLineStyle::Dashed, 4.0, BoxId(10))
        );
        assert_eq!(r.cell_borders[0], [0.0, 2.0, 0.0, 0.0]);
        assert_eq!(r.cell_borders[1], [0.0, 0.0, 0.0, 2.0]);
        // The outer edges have no border; one segment is published.
        assert_eq!(r.segments.len(), 1);
        assert_eq!(
            (
                r.segments[0].grid_line,
                r.segments[0].extent,
                r.segments[0].horizontal
            ),
            (1, (0, 1), false)
        );
    }

    #[test]
    fn uniform_borders_merge_into_long_segments_and_the_table_takes_half() {
        let st = assign_slots(&[GroupInput {
            rows: vec![
                RowInput {
                    cells: vec![
                        CellInput {
                            col_span: 1,
                            row_span: 1
                        };
                        2
                    ]
                };
                2
            ],
        }]);
        let cells = vec![solid(2.0, Origin::Cell, 20); 4];
        let rows = [none(Origin::Row, 5), none(Origin::Row, 6)];
        let cols = [None, None];
        let r = resolve_borders(&two_by_two(
            &st,
            none(Origin::Table, 1),
            &rows,
            &cells,
            &cols,
        ));
        assert_eq!(r.table_border, [1.0; 4]);
        for b in &r.cell_borders {
            assert_eq!(*b, [1.0; 4]);
        }
        // Three vertical lines and three horizontal lines, each one segment of extent 2.
        assert_eq!(r.segments.len(), 6);
        assert!(r.segments.iter().all(|s| s.extent == (0, 2)));
    }

    #[test]
    fn spanning_cell_has_no_inner_border() {
        let st = assign_slots(&[GroupInput {
            rows: vec![
                RowInput {
                    cells: vec![CellInput {
                        col_span: 2,
                        row_span: 1,
                    }],
                },
                RowInput {
                    cells: vec![
                        CellInput {
                            col_span: 1,
                            row_span: 1
                        };
                        2
                    ],
                },
            ],
        }]);
        let cells = vec![solid(2.0, Origin::Cell, 30); 3];
        let rows = [none(Origin::Row, 5), none(Origin::Row, 6)];
        let cols = [None, None];
        let r = resolve_borders(&two_by_two(
            &st,
            none(Origin::Table, 1),
            &rows,
            &cells,
            &cols,
        ));
        assert!(r.v[0][1].is_none(), "inside the spanning cell");
        assert!(r.v[1][1].is_some());
    }

    #[test]
    fn table_border_wins_on_the_outer_edge_when_wider() {
        let st = assign_slots(&[GroupInput {
            rows: vec![RowInput {
                cells: vec![CellInput {
                    col_span: 1,
                    row_span: 1,
                }],
            }],
        }]);
        let cells = [solid(1.0, Origin::Cell, 40)];
        let rows = [none(Origin::Row, 5)];
        let cols = [None];
        let r = resolve_borders(&two_by_two(
            &st,
            solid(6.0, Origin::Table, 1),
            &rows,
            &cells,
            &cols,
        ));
        assert_eq!(r.table_border, [3.0; 4]);
        assert_eq!(r.cell_borders[0], [3.0; 4]);
        assert_eq!(r.h[0][0].unwrap().box_id, BoxId(1));
    }
}
