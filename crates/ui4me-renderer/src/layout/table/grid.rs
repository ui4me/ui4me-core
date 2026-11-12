//! owner: 008-layout-engine (T101, L-TBL-r4) — slot assignment by the HTML table model.
//!
//! Clean-room implementation (T098: no Servo code is ported; see `table/mod.rs`). The input is the
//! table's row groups in **visual order** (the first `thead` first, bodies in source order, the
//! first `tfoot` last; the caller orders them), each holding rows of cells with their `colspan` and
//! `rowspan`. The output is the grid of the WHATWG HTML Standard §4.9.12.1 ("forming a table"):
//!
//! * a cell starts at the first free column of its row; a cell never starts on, or runs over, a slot
//!   another cell already holds (the standard calls an overlap a table model error; here the later
//!   cell moves right past the conflict, so that **no slot ever holds two cells**);
//! * `colspan` is 1–1000 and `rowspan` 0–65534 (see [`parse_colspan`], [`parse_rowspan`]);
//! * a row span never leaves its row group: it is clamped to the group's last row, and `rowspan=0`
//!   spans to the end of the row group;
//! * rows without cells still count; the column count is the widest row (including spans).
//!
//! Everything here is a pure function of the spans: no style, no DOM.

use std::ops::Range;

/// Maximum `colspan` (HTML §4.9.12 rule: values above are clamped).
pub const MAX_COLSPAN: u32 = 1000;
/// Maximum `rowspan`.
pub const MAX_ROWSPAN: u32 = 65534;

/// Parse a `colspan` attribute (HTML §4.9.12: "rules for parsing non-negative integers"; an error,
/// 0 or a missing attribute gives 1; values above 1000 are clamped).
pub fn parse_colspan(value: Option<&str>) -> u32 {
    match parse_nonneg(value) {
        Some(v) if v >= 1 => v.min(MAX_COLSPAN),
        _ => 1,
    }
}

/// Parse a `rowspan` attribute (an error or a missing attribute gives 1; 0 is kept and means "to the
/// end of the row group"; values above 65534 are clamped).
pub fn parse_rowspan(value: Option<&str>) -> u32 {
    match parse_nonneg(value) {
        Some(v) => v.min(MAX_ROWSPAN),
        None => 1,
    }
}

/// HTML "rules for parsing non-negative integers": ASCII white space is skipped, an optional `+`
/// is allowed, digits are read up to the first non-digit; a leading `-` (other than `-0`) or no
/// digit is an error. Overflow saturates (the caller clamps).
fn parse_nonneg(value: Option<&str>) -> Option<u32> {
    let s = value?.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0C' | '\r'));
    let (neg, rest) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let v = digits
        .parse::<u64>()
        .map(|v| v.min(u64::from(u32::MAX)))
        .unwrap_or(u64::from(u32::MAX)) as u32;
    if neg && v != 0 {
        return None;
    }
    Some(v)
}

/// One cell as the structure sees it: its (already clamped) spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellInput {
    pub col_span: u32,
    /// 0 = to the end of the row group.
    pub row_span: u32,
}

/// A row: its cells in source order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowInput {
    pub cells: Vec<CellInput>,
}

/// A row group in visual order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupInput {
    pub rows: Vec<RowInput>,
}

/// A cell placed in the grid. `id` is the cell's index in iteration order (groups, rows, cells),
/// which is how the caller maps it back to its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedCell {
    pub id: usize,
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
}

/// The slot grid of a table (data-model §3 `TableStructure`, without the box references).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableStructure {
    pub column_count: usize,
    pub row_count: usize,
    /// Cells in iteration order; `cells[i].id == i`.
    pub cells: Vec<PlacedCell>,
    /// `slots[row][col]` = index into `cells`; each row is `column_count` wide.
    pub slots: Vec<Vec<Option<usize>>>,
    /// The rows of each input group.
    pub groups: Vec<Range<usize>>,
}

impl TableStructure {
    /// The cell covering slot (`row`, `col`).
    pub fn cell_at(&self, row: usize, col: usize) -> Option<&PlacedCell> {
        self.slots
            .get(row)?
            .get(col)
            .copied()
            .flatten()
            .map(|i| &self.cells[i])
    }

    /// Widen the grid to at least `columns` columns (columns declared by `col` elements that no cell
    /// reaches still exist).
    pub fn ensure_columns(&mut self, columns: usize) {
        if columns > self.column_count {
            self.column_count = columns;
            for r in &mut self.slots {
                r.resize(columns, None);
            }
        }
    }
}

/// Assign every cell to its slots (module docs).
pub fn assign_slots(groups: &[GroupInput]) -> TableStructure {
    let mut t = TableStructure::default();
    // Rows are materialised lazily: `slots[r]` may be shorter than `width` while it is being built.
    let mut width = 0usize;
    for g in groups {
        let first = t.slots.len();
        let last = first + g.rows.len(); // exclusive
        t.slots.resize_with(last, Vec::new);
        for (ri, row) in g.rows.iter().enumerate() {
            let y = first + ri;
            let mut x = 0usize;
            for c in &row.cells {
                let col_span = c.col_span.clamp(1, MAX_COLSPAN) as usize;
                let want = if c.row_span == 0 {
                    last - y
                } else {
                    (c.row_span.min(MAX_ROWSPAN) as usize).min(last - y)
                };
                let row_span = want.max(1);
                // First x at or after the cursor where the whole span is free in every spanned row.
                while !free(&t.slots, y, row_span, x, col_span) {
                    x += 1;
                }
                let id = t.cells.len();
                t.cells.push(PlacedCell {
                    id,
                    row: y,
                    col: x,
                    row_span,
                    col_span,
                });
                for r in y..y + row_span {
                    let s = &mut t.slots[r];
                    if s.len() < x + col_span {
                        s.resize(x + col_span, None);
                    }
                    for slot in &mut s[x..x + col_span] {
                        *slot = Some(id);
                    }
                }
                width = width.max(x + col_span);
                x += col_span;
            }
        }
        t.groups.push(first..last);
    }
    t.row_count = t.slots.len();
    t.column_count = width;
    for r in &mut t.slots {
        r.resize(width, None);
    }
    t
}

fn free(slots: &[Vec<Option<usize>>], y: usize, rows: usize, x: usize, cols: usize) -> bool {
    (y..y + rows).all(|r| (x..x + cols).all(|c| slots[r].get(c).copied().flatten().is_none()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn cell(c: u32, r: u32) -> CellInput {
        CellInput {
            col_span: c,
            row_span: r,
        }
    }
    fn group(rows: Vec<Vec<CellInput>>) -> GroupInput {
        GroupInput {
            rows: rows.into_iter().map(|cells| RowInput { cells }).collect(),
        }
    }

    #[test]
    fn parse_spans() {
        assert_eq!(parse_colspan(None), 1);
        assert_eq!(parse_colspan(Some("0")), 1);
        assert_eq!(parse_colspan(Some("3")), 3);
        assert_eq!(parse_colspan(Some(" 4abc")), 4);
        assert_eq!(parse_colspan(Some("-2")), 1);
        assert_eq!(parse_colspan(Some("5000")), MAX_COLSPAN);
        assert_eq!(parse_colspan(Some("99999999999999999999")), MAX_COLSPAN);
        assert_eq!(parse_rowspan(None), 1);
        assert_eq!(parse_rowspan(Some("0")), 0);
        assert_eq!(parse_rowspan(Some("-0")), 0);
        assert_eq!(parse_rowspan(Some("-1")), 1);
        assert_eq!(parse_rowspan(Some("x")), 1);
        assert_eq!(parse_rowspan(Some("70000")), MAX_ROWSPAN);
    }

    #[test]
    fn plain_grid() {
        let t = assign_slots(&[group(vec![
            vec![cell(1, 1), cell(1, 1)],
            vec![cell(1, 1), cell(1, 1)],
        ])]);
        assert_eq!((t.row_count, t.column_count), (2, 2));
        assert_eq!(t.cell_at(1, 1).unwrap().id, 3);
    }

    #[test]
    fn rowspan_pushes_the_next_row_right() {
        // |A A|  A spans 2 rows at col 0; row 1's first cell lands at col 1.
        let t = assign_slots(&[group(vec![vec![cell(1, 2), cell(1, 1)], vec![cell(1, 1)]])]);
        assert_eq!(t.cells[2].col, 1);
        assert_eq!(t.cells[2].row, 1);
        assert_eq!(t.column_count, 2);
    }

    #[test]
    fn rowspan_zero_spans_to_the_end_of_the_group_only() {
        let t = assign_slots(&[
            group(vec![vec![cell(1, 0)], vec![], vec![]]),
            group(vec![vec![cell(1, 1)]]),
        ]);
        assert_eq!(t.cells[0].row_span, 3);
        // The second group starts at row 3 and its cell is at column 0.
        assert_eq!((t.cells[1].row, t.cells[1].col), (3, 0));
    }

    #[test]
    fn rowspan_is_clamped_to_the_group() {
        let t = assign_slots(&[
            group(vec![vec![cell(1, 9)], vec![]]),
            group(vec![vec![cell(1, 1)]]),
        ]);
        assert_eq!(t.cells[0].row_span, 2);
        assert_eq!(t.cells[1].col, 0);
    }

    #[test]
    fn colspan_cell_moves_past_an_occupied_slot() {
        // Row 0: A (rowspan 2) at col 1 after an empty col 0? A is first, so it is at col 0.
        // Row 1: B has colspan 2: col 0 is taken by A, so B starts at col 1.
        let t = assign_slots(&[group(vec![vec![cell(1, 2)], vec![cell(2, 1)]])]);
        assert_eq!((t.cells[1].col, t.cells[1].col_span), (1, 2));
        assert_eq!(t.column_count, 3);
    }

    #[test]
    fn empty_rows_count() {
        let t = assign_slots(&[group(vec![vec![], vec![cell(2, 1)], vec![]])]);
        assert_eq!((t.row_count, t.column_count), (3, 2));
        assert_eq!(t.groups, vec![0..3]);
    }

    fn arb_cell() -> impl Strategy<Value = CellInput> {
        (1u32..6, prop_oneof![Just(0u32), 1u32..6]).prop_map(|(c, r)| CellInput {
            col_span: c,
            row_span: r,
        })
    }
    fn arb_groups() -> impl Strategy<Value = Vec<GroupInput>> {
        prop::collection::vec(
            prop::collection::vec(prop::collection::vec(arb_cell(), 0..5), 0..5).prop_map(|rows| {
                GroupInput {
                    rows: rows.into_iter().map(|cells| RowInput { cells }).collect(),
                }
            }),
            0..4,
        )
    }

    proptest! {
        // T099: no slot holds two cells; every cell covers exactly its rectangle; spans stay in the
        // group; rowspan=0 reaches the group's last row; the grid is rectangular.
        #[test]
        fn slots_are_consistent(groups in arb_groups()) {
            let t = assign_slots(&groups);
            prop_assert_eq!(t.slots.len(), t.row_count);
            for row in &t.slots {
                prop_assert_eq!(row.len(), t.column_count);
            }
            let mut covered = vec![vec![0usize; t.column_count]; t.row_count];
            for c in &t.cells {
                prop_assert!(c.row_span >= 1 && c.col_span >= 1);
                prop_assert!(c.row + c.row_span <= t.row_count && c.col + c.col_span <= t.column_count);
                for (r, row) in covered.iter_mut().enumerate().skip(c.row).take(c.row_span) {
                    for (col, count) in row.iter_mut().enumerate().skip(c.col).take(c.col_span) {
                        *count += 1;
                        prop_assert_eq!(t.slots[r][col], Some(c.id));
                    }
                }
            }
            for row in &covered {
                for n in row {
                    prop_assert!(*n <= 1, "a slot holds two cells");
                }
            }
            // Every cell with a non-empty slot belongs to exactly the slots it covers.
            let filled = t.slots.iter().flatten().filter(|s| s.is_some()).count();
            let area: usize = t.cells.iter().map(|c| c.row_span * c.col_span).sum();
            prop_assert_eq!(filled, area);
        }

        #[test]
        fn spans_respect_clamps_and_groups(groups in arb_groups()) {
            let t = assign_slots(&groups);
            let mut id = 0usize;
            for (gi, g) in groups.iter().enumerate() {
                let range = t.groups[gi].clone();
                prop_assert_eq!(range.len(), g.rows.len());
                for (ri, row) in g.rows.iter().enumerate() {
                    for c in &row.cells {
                        let p = t.cells[id];
                        id += 1;
                        prop_assert_eq!(p.row, range.start + ri);
                        prop_assert!(p.row + p.row_span <= range.end, "span left its group");
                        prop_assert!(p.col_span as u32 == c.col_span.clamp(1, MAX_COLSPAN));
                        if c.row_span == 0 {
                            prop_assert_eq!(p.row + p.row_span, range.end);
                        } else {
                            prop_assert!(p.row_span as u32 <= c.row_span);
                        }
                    }
                }
            }
        }
    }
}
