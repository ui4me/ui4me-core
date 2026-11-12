//! owner: 008-layout-engine (T102, L-TBL-r4) — width distribution of the automatic and fixed
//! table layouts.
//!
//! Clean-room (T098). Input is the width the columns share (the table's content width minus
//! border-spacing, borders and padding); output is one width per column.
//!
//! **Automatic layout** (CSS 2.1 §17.5.2.2 as refined by CSS Tables 3): every column starts at its
//! minimum. Percentage columns then reach their share of the target (never less than their
//! minimum; shares above 100 % in total are scaled down). The remaining space grows the other
//! columns from their minimum towards their maximum in proportion to `max − min`. What is left
//! beyond every maximum goes to the `auto` columns in proportion to their maximum (to the fixed
//! columns when there are none, then to the percentage columns). If the target is below the sum of
//! the minimums the columns keep their minimums (the table is then wider than the target).
//!
//! **Fixed layout** (§17.5.2.1): column widths come from the `col` elements, else from the cells
//! of the first row (a spanning cell shares its width equally); the `auto` columns split what the
//! fixed ones leave equally; extra space beyond that grows all columns in proportion to their widths.

use super::measures::{Column, SpecWidth};

/// Distribute `target` over the columns (module docs). The result sums to `target` unless the
/// minimums need more.
pub fn distribute_auto(cols: &[Column], target: f32) -> Vec<f32> {
    let n = cols.len();
    let mut w: Vec<f32> = cols.iter().map(|c| c.min).collect();
    if n == 0 {
        return w;
    }
    let mut avail = target - w.iter().sum::<f32>();
    if avail <= 0.0 {
        return w;
    }
    // Percentage columns.
    let percent_total: f32 = cols
        .iter()
        .filter_map(|c| {
            if let SpecWidth::Percent(p) = c.spec {
                Some(p)
            } else {
                None
            }
        })
        .sum();
    if percent_total > 0.0 {
        let scale = if percent_total > 100.0 {
            100.0 / percent_total
        } else {
            1.0
        };
        for (i, c) in cols.iter().enumerate() {
            if let SpecWidth::Percent(p) = c.spec {
                let want = (target * p * scale / 100.0).max(c.min);
                let grow = (want - w[i]).min(avail).max(0.0);
                w[i] += grow;
                avail -= grow;
            }
        }
    }
    // The other columns grow from min to max in proportion to max - min.
    let others: Vec<usize> = (0..n)
        .filter(|&i| !matches!(cols[i].spec, SpecWidth::Percent(_)))
        .collect();
    let room: f32 = others.iter().map(|&i| cols[i].max - cols[i].min).sum();
    if avail > 0.0 && room > 0.0 {
        let share = (avail / room).min(1.0);
        for &i in &others {
            w[i] += (cols[i].max - cols[i].min) * share;
        }
        avail -= room * share;
    }
    // Beyond every maximum: auto columns, else fixed columns, else percentage columns.
    if avail > 0.0 {
        let pick = |f: &dyn Fn(&SpecWidth) -> bool| {
            (0..n).filter(|&i| f(&cols[i].spec)).collect::<Vec<_>>()
        };
        let mut targets = pick(&|s| *s == SpecWidth::Auto);
        if targets.is_empty() {
            targets = pick(&|s| matches!(s, SpecWidth::Fixed(_)));
        }
        if targets.is_empty() {
            targets = (0..n).collect();
        }
        let total: f32 = targets.iter().map(|&i| cols[i].max).sum();
        for &i in &targets {
            let share = if total > 0.0 {
                cols[i].max / total
            } else {
                1.0 / targets.len() as f32
            };
            w[i] += avail * share;
        }
    }
    w
}

/// The width of a column for the fixed layout.
pub fn distribute_fixed(specs: &[SpecWidth], target: f32) -> Vec<f32> {
    let n = specs.len();
    let mut w = vec![0.0f32; n];
    let mut auto = Vec::new();
    let mut used = 0.0f32;
    for (i, s) in specs.iter().enumerate() {
        match *s {
            SpecWidth::Fixed(x) => {
                w[i] = x.max(0.0);
                used += w[i];
            }
            SpecWidth::Percent(p) => {
                w[i] = (target * p / 100.0).max(0.0);
                used += w[i];
            }
            SpecWidth::Auto => auto.push(i),
        }
    }
    if !auto.is_empty() {
        let each = ((target - used) / auto.len() as f32).max(0.0);
        for i in auto {
            w[i] = each;
        }
    } else if used < target && used > 0.0 {
        let k = target / used;
        for x in &mut w {
            *x *= k;
        }
    } else if used < target && n > 0 {
        for x in &mut w {
            *x = target / n as f32;
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(min: f32, max: f32, spec: SpecWidth) -> Column {
        Column { min, max, spec }
    }
    fn sum(v: &[f32]) -> f32 {
        v.iter().sum()
    }

    #[test]
    fn below_the_minimums_the_columns_keep_them() {
        let w = distribute_auto(
            &[
                col(30.0, 90.0, SpecWidth::Auto),
                col(20.0, 20.0, SpecWidth::Auto),
            ],
            40.0,
        );
        assert_eq!(w, vec![30.0, 20.0]);
    }

    #[test]
    fn between_min_and_max_interpolates() {
        // mins 10+10, maxes 110+30: target 80 -> 60 spare over 120 room = 0.5.
        let w = distribute_auto(
            &[
                col(10.0, 110.0, SpecWidth::Auto),
                col(10.0, 30.0, SpecWidth::Auto),
            ],
            80.0,
        );
        assert!(
            (w[0] - 60.0).abs() < 1e-3 && (w[1] - 20.0).abs() < 1e-3,
            "{w:?}"
        );
        assert!((sum(&w) - 80.0).abs() < 1e-3);
    }

    #[test]
    fn beyond_the_maximums_auto_columns_grow_by_max() {
        let w = distribute_auto(
            &[
                col(10.0, 30.0, SpecWidth::Auto),
                col(10.0, 10.0, SpecWidth::Fixed(10.0)),
            ],
            100.0,
        );
        // All maxima reached (30, 10) = 40; 60 extra goes to the auto column.
        assert!(
            (w[0] - 90.0).abs() < 1e-3 && (w[1] - 10.0).abs() < 1e-3,
            "{w:?}"
        );
    }

    #[test]
    fn percent_columns_get_their_share() {
        let w = distribute_auto(
            &[
                col(10.0, 20.0, SpecWidth::Percent(25.0)),
                col(10.0, 200.0, SpecWidth::Auto),
            ],
            200.0,
        );
        assert!(
            (w[0] - 50.0).abs() < 1e-3 && (w[1] - 150.0).abs() < 1e-3,
            "{w:?}"
        );
    }

    #[test]
    fn overallocated_percentages_are_scaled() {
        let w = distribute_auto(
            &[
                col(0.0, 0.0, SpecWidth::Percent(80.0)),
                col(0.0, 0.0, SpecWidth::Percent(80.0)),
            ],
            100.0,
        );
        assert!(
            (w[0] - 50.0).abs() < 1e-3 && (w[1] - 50.0).abs() < 1e-3,
            "{w:?}"
        );
    }

    #[test]
    fn fixed_layout() {
        let w = distribute_fixed(
            &[SpecWidth::Fixed(100.0), SpecWidth::Auto, SpecWidth::Auto],
            300.0,
        );
        assert_eq!(w, vec![100.0, 100.0, 100.0]);
        let w = distribute_fixed(&[SpecWidth::Fixed(100.0), SpecWidth::Fixed(100.0)], 400.0);
        assert_eq!(w, vec![200.0, 200.0]);
        let w = distribute_fixed(&[SpecWidth::Percent(25.0), SpecWidth::Auto], 200.0);
        assert_eq!(w, vec![50.0, 150.0]);
    }
}
