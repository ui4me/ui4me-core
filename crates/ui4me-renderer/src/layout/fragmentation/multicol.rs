//! owner: 015-core-completion (MC-1, lane L-MC) — multicolumn containers (CSS Multicol 1) on the
//! fragmentation engine (plan §A3 "Multicol").
//!
//! * **Which boxes:** a block container (`display: block | flow-root | list-item`) whose `column-count`
//!   or `column-width` is not `auto` ([`multicol`]). Its content moves into anonymous flow-thread boxes
//!   (`BoxTag::FlowThread`), one per *row*: a `column-span: all` child (a **spanner**) ends a row, is
//!   laid out at the container's full width between the rows, and the next content starts a new row.
//! * **Columns:** the used count and width follow Multicol 1 section 3.4 ([`resolve`]); `column-gap:
//!   normal` is 1em. Every row is laid out at the column width as one strip and cut into columns of
//!   one height (the row's *column height*); columns past the count overflow in the inline direction.
//! * **Column height:** with `column-fill: auto` and a definite height (the last row only), the
//!   container's content height left for the row; otherwise the row is **balanced** ([`Balancer`]):
//!   start at max(content height / N, tallest unbreakable unit), lay out, grow by the smallest shortage
//!   a pushed unit reports, at most [`BALANCE_GROWTH_STEPS`] times, then a binary search between the last
//!   height that did not fit and one that did guarantees termination. A definite (or `max-`) height caps
//!   the column height.
//! * **Nested multicol** containers are monolithic in an outer fragmentation context (first version).
//! * **Column rules** are painted between two columns that both hold content (the fragment builder
//!   records them on the container's fragment, the scene paints them in the container's decoration).

use crate::layout::box_tree::{BoxStyle, DisplayValue};

/// Growth steps of the balancing loop before the binary search (plan §A3: ≤ 6).
pub const BALANCE_GROWTH_STEPS: u32 = 6;

/// The binary search stops when the bracket is narrower than this (CSS px), or after this many steps.
pub const BALANCE_SEARCH_PRECISION: f32 = 1.0 / 64.0;
pub const BALANCE_SEARCH_STEPS: u32 = 16;

/// Column heights are whole layout units (1/64 px), rounded up.
pub fn ceil_unit(h: f32) -> f32 {
    if !h.is_finite() {
        return h;
    }
    let v = (h * 64.0).ceil() / 64.0;
    // Float noise a hair above a unit does not add a unit.
    if v - h > 1.0 / 64.0 - 1e-3 {
        ((h * 64.0).round()) / 64.0
    } else {
        v
    }
}

/// The column properties of a multicol container.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MulticolSpec {
    /// `column-count` (`None` = `auto`).
    pub count: Option<u32>,
    /// `column-width` in px (`None` = `auto`).
    pub width: Option<f32>,
    /// `column-fill: auto`.
    pub fill_auto: bool,
}

/// Whether `style` (a block container's) makes a multicol container laid out by this engine.
pub fn multicol(style: &BoxStyle) -> Option<MulticolSpec> {
    let f = &style.frag;
    if !f.is_multicol() || f.column_count == Some(0) {
        return None;
    }
    if !matches!(
        style.display,
        DisplayValue::Block | DisplayValue::FlowRoot | DisplayValue::ListItem
    ) {
        return None;
    }
    Some(MulticolSpec {
        count: f.column_count,
        width: f.column_width.filter(|w| w.is_finite()),
        fill_auto: f.column_fill_auto,
    })
}

/// Upper bound of the used column count (a guard against `column-width: 0` style inputs).
const MAX_COLUMNS: u32 = 1000;

/// The used column count and width (CSS Multicol 1 section 3.4) for a content width `u` and gap `gap`.
pub fn resolve(spec: MulticolSpec, u: f32, gap: f32) -> (u32, f32) {
    let u = u.max(0.0);
    let by_width = |w: f32| -> u32 {
        let step = w.max(0.0) + gap;
        if step <= 0.0 {
            return MAX_COLUMNS;
        }
        (((u + gap) / step).floor() as u32).clamp(1, MAX_COLUMNS)
    };
    let n = match (spec.count, spec.width) {
        (Some(c), None) => c.clamp(1, MAX_COLUMNS),
        (None, Some(w)) => by_width(w),
        (Some(c), Some(w)) => c.clamp(1, MAX_COLUMNS).min(by_width(w)),
        (None, None) => 1,
    };
    let w = match (spec.count, spec.width) {
        (Some(_), None) => ((u - (n as f32 - 1.0) * gap) / n as f32).max(0.0),
        _ => (((u + gap) / n as f32) - gap).max(0.0),
    };
    (n, w)
}

/// The balancing loop of one row (see the module docs): the caller lays the row out at the heights
/// [`Balancer::next`] proposes and reports each outcome; the loop ends with the height to keep.
#[derive(Clone, Debug)]
pub struct Balancer {
    /// Columns available.
    pub count: u32,
    /// The cap on the column height (definite or `max-` height), if any.
    pub cap: Option<f32>,
    /// The height of the row unfragmented (one column holding everything always fits).
    pub whole: f32,
    /// The largest height that did not fit, the smallest that did.
    lo: f32,
    hi: Option<f32>,
    growth: u32,
    search: u32,
    settled: bool,
    /// Layouts proposed so far (for tests and diagnostics).
    pub iterations: u32,
}

/// What one layout of a row at a column height found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowOutcome {
    /// Every content of the row ends within `count` columns (forced breaks may add columns).
    pub fits: bool,
    /// The smallest amount a pushed unit needed beyond its column end (the minimum shortage).
    pub shortage: Option<f32>,
    /// How much the content overflows the columns, spread over them (fallback growth when no unit
    /// reported a shortage, e.g. a split box of a definite height).
    pub spread: f32,
}

impl Balancer {
    /// The first height: max(content height / N, tallest unit), capped.
    pub fn new(
        count: u32,
        cap: Option<f32>,
        content: f32,
        tallest: f32,
        whole: f32,
    ) -> (Self, f32) {
        let count = count.max(1);
        let mut h = (content / count as f32).max(tallest).max(super::EPS);
        if let Some(c) = cap {
            h = h.min(c);
        }
        let b = Balancer {
            count,
            cap,
            whole: whole.max(content),
            lo: 0.0,
            hi: None,
            growth: 0,
            search: 0,
            settled: false,
            iterations: 1,
        };
        (b, ceil_unit(h))
    }

    /// The outcome of the layout at `h`; returns the next height to try, or `None` when `h` is final.
    pub fn next(&mut self, h: f32, o: RowOutcome) -> Option<f32> {
        let p = self.next_raw(h, o)?;
        let q = ceil_unit(p);
        if (q - h).abs() < 1e-4 {
            return None;
        }
        Some(self.cap.map_or(q, |c| q.min(c)))
    }

    fn next_raw(&mut self, h: f32, o: RowOutcome) -> Option<f32> {
        let eps = super::EPS;
        if self.settled {
            return None;
        }
        if o.fits {
            self.hi = Some(self.hi.map_or(h, |x: f32| x.min(h)));
            if self.search == 0 {
                // The first height that fits while growing by the minimum shortage is the balanced one.
                return None;
            }
        } else {
            self.lo = self.lo.max(h);
            if self.cap.is_some_and(|c| h >= c - eps) {
                // No taller column is allowed: the rest overflows into extra columns.
                return None;
            }
            if self.hi.is_none() && self.growth < BALANCE_GROWTH_STEPS {
                self.growth += 1;
                let step = o.shortage.filter(|s| *s > eps).unwrap_or(o.spread).max(eps);
                let p = self.cap.map_or(h + step, |c| (h + step).min(c));
                self.iterations += 1;
                return Some(p);
            }
        }
        // Binary search between the largest height that failed and the smallest that fit (or the cap, or
        // the whole row in one column, which always fits).
        let hi = self
            .hi
            .unwrap_or_else(|| self.cap.unwrap_or(self.whole).max(self.lo));
        if hi - self.lo <= BALANCE_SEARCH_PRECISION || self.search >= BALANCE_SEARCH_STEPS {
            self.settled = true;
            if (hi - h).abs() <= eps {
                return None;
            }
            self.iterations += 1;
            return Some(hi);
        }
        self.search += 1;
        self.iterations += 1;
        Some(((self.lo + hi) / 2.0).max(self.lo + eps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(count: Option<u32>, width: Option<f32>) -> MulticolSpec {
        MulticolSpec {
            count,
            width,
            fill_auto: false,
        }
    }

    #[test]
    fn column_count_and_width_resolve_per_section_3_4() {
        assert_eq!(resolve(spec(Some(3), None), 340.0, 20.0), (3, 100.0));
        // column-width: as many as fit, then stretched to fill.
        let (n, w) = resolve(spec(None, Some(110.0)), 340.0, 20.0);
        assert_eq!(n, 2);
        assert!((w - 160.0).abs() < 1e-3);
        // Both: the count is a maximum.
        assert_eq!(resolve(spec(Some(2), Some(50.0)), 340.0, 20.0).0, 2);
        assert_eq!(resolve(spec(Some(5), Some(100.0)), 340.0, 20.0).0, 3);
        // Narrower than one column: one column of the content width.
        assert_eq!(resolve(spec(None, Some(500.0)), 340.0, 20.0), (1, 340.0));
    }

    /// A row of `units` unbreakable units laid into columns of height h (each column takes units while they fit).
    fn outcome(units: &[f32], n: u32, h: f32) -> RowOutcome {
        let mut cols = 1;
        let mut used = 0.0;
        let mut shortage: Option<f32> = None;
        for &u in units {
            if used + u > h + 1e-4 && used > 0.0 {
                shortage = Some(shortage.map_or(used + u - h, |s: f32| s.min(used + u - h)));
                cols += 1;
                used = 0.0;
            }
            used += u;
        }
        RowOutcome {
            fits: cols <= n,
            shortage,
            spread: 0.0,
        }
    }

    fn balance(units: &[f32], n: u32, cap: Option<f32>) -> (f32, u32) {
        let total: f32 = units.iter().sum();
        let tallest = units.iter().cloned().fold(0.0, f32::max);
        let (mut b, mut h) = Balancer::new(n, cap, total, tallest, total);
        loop {
            match b.next(h, outcome(units, n, h)) {
                Some(x) => h = x,
                None => return (h, b.iterations),
            }
        }
    }

    #[test]
    fn equal_units_balance_at_the_first_height() {
        let (h, it) = balance(&[20.0; 6], 3, None);
        assert_eq!(h, 40.0);
        assert_eq!(it, 1);
    }

    #[test]
    fn unequal_units_grow_by_the_minimum_shortage() {
        // 7 lines of 20 into 3 columns: start at 46.67, a line is pushed short by 13.33: 60 fits.
        let (h, it) = balance(&[20.0; 7], 3, None);
        assert!((h - 60.0).abs() < 1e-3, "{h}");
        assert_eq!(it, 2);
    }

    #[test]
    fn a_tall_unit_sets_the_floor() {
        let (h, it) = balance(&[10.0, 90.0, 10.0], 3, None);
        assert_eq!(h, 90.0);
        assert_eq!(it, 1);
    }

    #[test]
    fn the_cap_ends_balancing() {
        let (h, _) = balance(&[20.0; 12], 2, Some(50.0));
        assert_eq!(h, 50.0);
    }

    #[test]
    fn growth_is_bounded_then_searched() {
        // Shortages of a hair each time: the growth steps run out and the binary search settles.
        let units: Vec<f32> = (0..40).map(|i| 10.0 + (i % 7) as f32 * 0.37).collect();
        let (h, it) = balance(&units, 4, None);
        assert!(outcome(&units, 4, h).fits);
        assert!(
            it <= 1 + BALANCE_GROWTH_STEPS + BALANCE_SEARCH_STEPS + 1,
            "{it}"
        );
    }
}
