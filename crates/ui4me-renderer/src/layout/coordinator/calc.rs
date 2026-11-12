//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! `calc()` resolution at layout time (AD-12; CSS Values 4 §10). After computed-value time a
//! `<length-percentage>` calc is a tree over px and percentage leaves (font-relative units are
//! already absolute), so layout only needs the percentage basis. Taffy reaches these through
//! `LayoutPartialTree::resolve_calc_value` with a pointer to a boxed `CalcExpr` owned by the
//! host tree for the duration of the pass.

use std::sync::Arc;
use ui4me_engine_types::geometry::layout_unit::LayoutUnit;

/// A calc expression owned by another component (for example 006's `CalcHandle`), reached
/// through a resolver closure: `resolve(basis_px) -> px`.
#[derive(Clone)]
pub struct OpaqueCalc {
    resolve: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
    has_percent: bool,
}

impl OpaqueCalc {
    pub fn new(has_percent: bool, resolve: impl Fn(f32) -> f32 + Send + Sync + 'static) -> Self {
        OpaqueCalc {
            resolve: Arc::new(resolve),
            has_percent,
        }
    }
}

impl std::fmt::Debug for OpaqueCalc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpaqueCalc")
            .field("has_percent", &self.has_percent)
            .finish_non_exhaustive()
    }
}

impl PartialEq for OpaqueCalc {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.resolve, &other.resolve)
    }
}

/// A computed `calc()` expression (FR-017: over px, em, rem and %; em/rem are resolved to px by
/// the style engine, so leaves are px and percentages).
#[derive(Clone, Debug, PartialEq)]
pub enum CalcExpr {
    /// CSS px.
    Px(f32),
    /// Fraction of the basis (0.5 = 50%).
    Percent(f32),
    Sum(Vec<CalcExpr>),
    /// Product with a number.
    Scale(Box<CalcExpr>, f32),
    Min(Vec<CalcExpr>),
    Max(Vec<CalcExpr>),
    Clamp(Box<CalcExpr>, Box<CalcExpr>, Box<CalcExpr>),
    /// A percentage of the basis truncated to 1/64 px, as the reference browser's `LayoutUnit`
    /// resolves a percentage length (host quantisation, X-FLEX variant C; research R-06).
    TruncatedPercent(f32),
    /// Resolved by its owner (006's calc handle, valid for the style epoch; AD-12).
    Opaque(OpaqueCalc),
}

impl CalcExpr {
    /// Evaluate in f64 against `basis` px (CSS Values 4 §10.9 propagation of NaN/∞).
    pub fn eval(&self, basis: f64) -> f64 {
        match self {
            CalcExpr::Px(v) => *v as f64,
            CalcExpr::Percent(p) => *p as f64 * basis,
            CalcExpr::TruncatedPercent(p) => (*p as f64 * basis * 64.0).floor() / 64.0,
            CalcExpr::Sum(xs) => xs.iter().map(|x| x.eval(basis)).sum(),
            CalcExpr::Scale(x, k) => x.eval(basis) * *k as f64,
            CalcExpr::Min(xs) => xs
                .iter()
                .map(|x| x.eval(basis))
                .fold(f64::INFINITY, |a, b| {
                    if b.is_nan() || a.is_nan() {
                        f64::NAN
                    } else {
                        a.min(b)
                    }
                }),
            CalcExpr::Max(xs) => {
                xs.iter()
                    .map(|x| x.eval(basis))
                    .fold(f64::NEG_INFINITY, |a, b| {
                        if b.is_nan() || a.is_nan() {
                            f64::NAN
                        } else {
                            a.max(b)
                        }
                    })
            }
            CalcExpr::Clamp(lo, v, hi) => {
                let (lo, v, hi) = (lo.eval(basis), v.eval(basis), hi.eval(basis));
                if lo.is_nan() || v.is_nan() || hi.is_nan() {
                    f64::NAN
                } else {
                    v.min(hi).max(lo)
                }
            }
            CalcExpr::Opaque(o) => (o.resolve)(basis as f32) as f64,
        }
    }

    /// Whether any leaf is a percentage.
    pub fn has_percent(&self) -> bool {
        match self {
            CalcExpr::Px(_) => false,
            CalcExpr::Percent(_) | CalcExpr::TruncatedPercent(_) => true,
            CalcExpr::Sum(xs) | CalcExpr::Min(xs) | CalcExpr::Max(xs) => {
                xs.iter().any(CalcExpr::has_percent)
            }
            CalcExpr::Scale(x, _) => x.has_percent(),
            CalcExpr::Clamp(a, b, c) => a.has_percent() || b.has_percent() || c.has_percent(),
            CalcExpr::Opaque(o) => o.has_percent,
        }
    }
}

/// `resolve_calc_value` (AD-12): evaluate and cross the calc boundary into layout units
/// (NaN → 0, ±∞ → bound, finite values quantised to 1/64; 008 `fragment-tree.md` §1).
pub fn resolve_calc_value(expr: &CalcExpr, basis: f32) -> f32 {
    let v = expr.eval(basis as f64);
    if v.is_finite() {
        floor_to_unit(v) as f32
    } else {
        LayoutUnit::from_calc(v).to_f64() as f32
    }
}

/// Quantise a host length to 1/64 px before it enters a layout algorithm (008 §1 "inputs").
///
/// The reference browser converts a computed length to a `LayoutUnit` by flooring (measured:
/// `height: 9.4px` is 9.390625, `9.99px` is 9.984375, `calc(100% / 7)` of 300px is 42.84375), so
/// inputs floor to the unit; the epsilon keeps float noise from dropping a whole unit (L-INL,
/// D-LINL-04). Output edges still round (008 `fragment-tree.md` §1).
pub fn quantize_input(px: f32) -> f32 {
    floor_to_unit(px as f64) as f32
}

/// `floor(px * 64 + epsilon) / 64`, with non-finite values and the range handled as the rounding
/// quantiser does.
fn floor_to_unit(px: f64) -> f64 {
    if !px.is_finite() || px.abs() > (i32::MAX as f64) / 64.0 {
        return LayoutUnit::quantize(px).value.to_f64();
    }
    ((px * 64.0 + 1e-3).floor()) / 64.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_px_and_percent() {
        // calc(50% - 2em + 1rem) with em = 16px, rem = 16px → 50% - 16px.
        let e = CalcExpr::Sum(vec![
            CalcExpr::Percent(0.5),
            CalcExpr::Px(-32.0),
            CalcExpr::Px(16.0),
        ]);
        assert_eq!(resolve_calc_value(&e, 320.0), 144.0);
        assert!(e.has_percent());
    }

    #[test]
    fn boundary_rules() {
        assert_eq!(
            resolve_calc_value(
                &CalcExpr::Scale(Box::new(CalcExpr::Px(f32::INFINITY)), 0.0),
                0.0
            ),
            0.0
        );
        let inf = resolve_calc_value(&CalcExpr::Px(f32::INFINITY), 0.0);
        assert_eq!(inf as f64, LayoutUnit::MAX.to_f64() as f32 as f64);
        assert_eq!(
            resolve_calc_value(&CalcExpr::Px(1.0 / 3.0), 0.0),
            21.0 / 64.0
        );
    }

    #[test]
    fn opaque_resolvers_are_called_with_the_basis() {
        let e = CalcExpr::Opaque(OpaqueCalc::new(true, |b| b * 0.25 + 1.0));
        assert_eq!(resolve_calc_value(&e, 100.0), 26.0);
        assert!(e.has_percent());
    }

    #[test]
    fn min_max_clamp() {
        let e = CalcExpr::Clamp(
            Box::new(CalcExpr::Px(10.0)),
            Box::new(CalcExpr::Percent(1.0)),
            Box::new(CalcExpr::Px(100.0)),
        );
        assert_eq!(resolve_calc_value(&e, 50.0), 50.0);
        assert_eq!(resolve_calc_value(&e, 500.0), 100.0);
        assert_eq!(
            resolve_calc_value(
                &CalcExpr::Min(vec![CalcExpr::Px(3.0), CalcExpr::Percent(0.1)]),
                20.0
            ),
            2.0
        );
        assert_eq!(
            resolve_calc_value(
                &CalcExpr::Max(vec![CalcExpr::Px(3.0), CalcExpr::Percent(0.1)]),
                20.0
            ),
            3.0
        );
    }
}
