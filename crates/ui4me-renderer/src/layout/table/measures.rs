//! owner: 008-layout-engine (T102, L-TBL-r4) — column measures of the automatic table layout.
//!
//! Clean-room (T098). Each column gets a minimum and a maximum width and a specified width
//! (`auto`, fixed or percentage) from the cells that start in it and from the `col` elements
//! (CSS 2.1 §17.5.2.2 leaves the details to the user agent; the choices below follow the
//! behaviour the oracle browser shows for ordinary tables):
//!
//! * the minimum (maximum) of a column is the largest minimum (maximum) of the cells that start
//!   there and span only that column; a cell with a fixed `width` has a maximum of
//!   `max(minimum, width)`;
//! * of a column's specified widths a percentage beats a fixed width, which beats `auto`; two of the
//!   same kind take the larger;
//! * a cell spanning several columns adds what the columns cannot already hold: its extra minimum
//!   and maximum is shared among the spanned columns, preferring the `auto` ones, in proportion to
//!   their maximum widths (equally when those are all zero). Narrower spans are processed first.

/// A specified width of a column or a cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecWidth {
    Auto,
    /// Border-box width in px.
    Fixed(f32),
    /// A percentage of the table, 0–100.
    Percent(f32),
}

impl SpecWidth {
    /// The stronger of two specifications (module docs).
    pub fn merge(self, other: SpecWidth) -> SpecWidth {
        use SpecWidth::*;
        match (self, other) {
            (Percent(a), Percent(b)) => Percent(a.max(b)),
            (Percent(a), _) | (_, Percent(a)) => Percent(a),
            (Fixed(a), Fixed(b)) => Fixed(a.max(b)),
            (Fixed(a), Auto) | (Auto, Fixed(a)) => Fixed(a),
            (Auto, Auto) => Auto,
        }
    }
}

/// What one cell asks of its columns (border-box widths, padding and border included).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellConstraint {
    pub col: usize,
    pub span: usize,
    pub min: f32,
    pub max: f32,
    pub spec: SpecWidth,
}

/// A column's constraints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub min: f32,
    pub max: f32,
    pub spec: SpecWidth,
}

/// Combine the cells' constraints into column constraints. `col_specs` are the widths of the `col`
/// elements (shorter than `n` = the rest are `auto`).
pub fn column_constraints(
    n: usize,
    col_specs: &[SpecWidth],
    cells: &[CellConstraint],
) -> Vec<Column> {
    let mut cols: Vec<Column> = (0..n)
        .map(|i| {
            let spec = col_specs.get(i).copied().unwrap_or(SpecWidth::Auto);
            // A fixed width on a `col` is a floor for the column (CSS 2.1 §17.5.2.2: "the
            // column's width is the larger of the col element's width and the cell minimums").
            let floor = if let SpecWidth::Fixed(w) = spec {
                w
            } else {
                0.0
            };
            Column {
                min: 0.0,
                max: floor,
                spec,
            }
        })
        .collect();
    for c in cells.iter().filter(|c| c.span == 1 && c.col < n) {
        let col = &mut cols[c.col];
        col.min = col.min.max(c.min);
        col.max = col.max.max(c.max);
        col.spec = col.spec.merge(c.spec);
    }
    let mut spanning: Vec<&CellConstraint> =
        cells.iter().filter(|c| c.span > 1 && c.col < n).collect();
    spanning.sort_by_key(|c| c.span); // stable: ties keep source order
    for c in spanning {
        let end = (c.col + c.span).min(n);
        let range = c.col..end;
        // A percentage on a spanning cell is shared by the columns that have none.
        if let SpecWidth::Percent(p) = c.spec
            && !cols[range.clone()]
                .iter()
                .any(|k| matches!(k.spec, SpecWidth::Percent(_)))
        {
            let share = p / range.len() as f32;
            for k in &mut cols[range.clone()] {
                k.spec = SpecWidth::Percent(share);
            }
        }
        let sum_min: f32 = cols[range.clone()].iter().map(|k| k.min).sum();
        if c.min > sum_min {
            share_extra(&mut cols[range.clone()], c.min - sum_min, |k| &mut k.min);
        }
        let sum_max: f32 = cols[range.clone()].iter().map(|k| k.max).sum();
        if c.max > sum_max {
            share_extra(&mut cols[range.clone()], c.max - sum_max, |k| &mut k.max);
        }
    }
    for k in &mut cols {
        k.max = k.max.max(k.min);
    }
    cols
}

/// Add `extra` to the chosen field of the columns: to the `auto` columns when there are any, else
/// to all of them; in proportion to the maximum widths, equally when those are all zero.
fn share_extra(cols: &mut [Column], extra: f32, field: impl Fn(&mut Column) -> &mut f32) {
    let auto: Vec<usize> = (0..cols.len())
        .filter(|&i| cols[i].spec == SpecWidth::Auto)
        .collect();
    let targets: Vec<usize> = if auto.is_empty() {
        (0..cols.len()).collect()
    } else {
        auto
    };
    let total: f32 = targets.iter().map(|&i| cols[i].max).sum();
    for &i in &targets {
        let w = if total > 0.0 {
            cols[i].max / total
        } else {
            1.0 / targets.len() as f32
        };
        *field(&mut cols[i]) += extra * w;
    }
}

/// Minimum and maximum content width of the columns (without spacing, borders and padding): the
/// sum of the minimums, and the smallest width that gives every column its maximum while honouring
/// the percentage columns (a percentage column must hold its maximum at its share of the whole;
/// the other columns must fit in what the percentages leave).
pub fn totals(cols: &[Column]) -> (f32, f32) {
    let min: f32 = cols.iter().map(|c| c.min).sum();
    let mut max: f32 = cols.iter().map(|c| c.max).sum();
    let percent_sum: f32 = cols
        .iter()
        .filter_map(|c| {
            if let SpecWidth::Percent(p) = c.spec {
                Some(p)
            } else {
                None
            }
        })
        .sum();
    if percent_sum > 0.0 {
        for c in cols {
            if let SpecWidth::Percent(p) = c.spec
                && p > 0.0
            {
                max = max.max(c.max * 100.0 / p);
            }
        }
        let rest: f32 = cols
            .iter()
            .filter(|c| !matches!(c.spec, SpecWidth::Percent(_)))
            .map(|c| c.max)
            .sum();
        if percent_sum < 100.0 {
            max = max.max(rest * 100.0 / (100.0 - percent_sum));
        }
    }
    (min, max.max(min))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(col: usize, span: usize, min: f32, max: f32) -> CellConstraint {
        CellConstraint {
            col,
            span,
            min,
            max,
            spec: SpecWidth::Auto,
        }
    }

    #[test]
    fn single_cells_take_the_largest() {
        let cols = column_constraints(
            2,
            &[],
            &[
                cell(0, 1, 10.0, 40.0),
                cell(0, 1, 20.0, 30.0),
                cell(1, 1, 5.0, 5.0),
            ],
        );
        assert_eq!((cols[0].min, cols[0].max), (20.0, 40.0));
        assert_eq!((cols[1].min, cols[1].max), (5.0, 5.0));
    }

    #[test]
    fn spec_merge_prefers_percent_then_fixed() {
        use SpecWidth::*;
        assert_eq!(Auto.merge(Fixed(3.0)), Fixed(3.0));
        assert_eq!(Fixed(3.0).merge(Percent(10.0)), Percent(10.0));
        assert_eq!(Percent(10.0).merge(Percent(25.0)), Percent(25.0));
        assert_eq!(Fixed(3.0).merge(Fixed(9.0)), Fixed(9.0));
    }

    #[test]
    fn spanning_cell_adds_only_what_is_missing() {
        // Columns hold 30 and 10 (max 30+10 = 40); a span-2 cell wants max 100: +60 shared by the
        // maximum widths (30:10).
        let cols = column_constraints(
            2,
            &[],
            &[
                cell(0, 1, 10.0, 30.0),
                cell(1, 1, 5.0, 10.0),
                cell(0, 2, 20.0, 100.0),
            ],
        );
        assert!((cols[0].max - 75.0).abs() < 1e-3, "{cols:?}");
        assert!((cols[1].max - 25.0).abs() < 1e-3, "{cols:?}");
        assert_eq!(cols[0].min + cols[1].min, 20.0);
    }

    #[test]
    fn spanning_percent_is_shared() {
        let mut c = cell(0, 2, 0.0, 0.0);
        c.spec = SpecWidth::Percent(50.0);
        let cols = column_constraints(2, &[], &[c]);
        assert_eq!(cols[0].spec, SpecWidth::Percent(25.0));
        assert_eq!(cols[1].spec, SpecWidth::Percent(25.0));
    }

    #[test]
    fn totals_account_for_percentages() {
        let cols = [
            Column {
                min: 10.0,
                max: 20.0,
                spec: SpecWidth::Percent(50.0),
            },
            Column {
                min: 10.0,
                max: 30.0,
                spec: SpecWidth::Auto,
            },
        ];
        // The auto column needs 30 in the other 50%: the table is at least 60 wide.
        let (min, max) = totals(&cols);
        assert_eq!(min, 20.0);
        assert!((max - 60.0).abs() < 1e-3, "{max}");
    }
}
