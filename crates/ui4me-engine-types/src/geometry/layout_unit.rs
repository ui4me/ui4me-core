//! owner: 001-vertical-slice (file); rules 008-layout-engine (R-37) — created by 001 slice (R-28)
//!
//! `LayoutUnit(i32)` at 1/64 CSS px. Rules: 008 `contracts/fragment-tree.md` §1 (XR-031),
//! implemented unchanged.

use crate::codes_generated::Code;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayoutUnit(i32);

/// A quantised value plus the stage diagnostic to record, if any (`GEOMETRY_NON_FINITE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quantized {
    pub value: LayoutUnit,
    pub diagnostic: Option<Code>,
}

impl LayoutUnit {
    pub const ZERO: LayoutUnit = LayoutUnit(0);
    pub const MAX: LayoutUnit = LayoutUnit(i32::MAX);
    pub const MIN: LayoutUnit = LayoutUnit(-i32::MAX);

    pub const fn from_raw(raw: i32) -> Self {
        LayoutUnit(raw)
    }
    pub const fn raw(self) -> i32 {
        self.0
    }
    /// Exact: `raw / 64.0` (every i32 is exactly representable in f64).
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / 64.0
    }

    /// Round to the nearest 1/64, ties toward +∞ (008 §1): `floor(px * 64 + 0.5)`.
    fn round_units(px: f64) -> f64 {
        (px * 64.0 + 0.5).floor()
    }

    /// Host-input conversion: NaN/±∞ rejected with `GEOMETRY_NON_FINITE` (AD-15);
    /// finite values beyond the range saturate to the bound.
    pub fn from_css_px(px: f64) -> Result<Self, Code> {
        if !px.is_finite() {
            return Err(Code::GeometryNonFinite);
        }
        Ok(Self::quantize(px).value)
    }
    pub fn from_css_px_f32(px: f32) -> Result<Self, Code> {
        Self::from_css_px(px as f64)
    }

    /// Layout-internal quantisation (008 §1): beyond ±(2³¹−1)/64 clamps to the bound with a
    /// `GEOMETRY_NON_FINITE` diagnostic; a non-finite value becomes 0 plus the diagnostic.
    pub fn quantize(px: f64) -> Quantized {
        if !px.is_finite() {
            return Quantized {
                value: Self::ZERO,
                diagnostic: Some(Code::GeometryNonFinite),
            };
        }
        let u = Self::round_units(px);
        if u > i32::MAX as f64 {
            Quantized {
                value: Self::MAX,
                diagnostic: Some(Code::GeometryNonFinite),
            }
        } else if u < -(i32::MAX as f64) {
            Quantized {
                value: Self::MIN,
                diagnostic: Some(Code::GeometryNonFinite),
            }
        } else {
            Quantized {
                value: LayoutUnit(u as i32),
                diagnostic: None,
            }
        }
    }

    /// Calc boundary (CSS Values 4): NaN → 0, ±∞ → bound; finite values quantise and clamp.
    pub fn from_calc(px: f64) -> Self {
        if px.is_nan() {
            Self::ZERO
        } else if px == f64::INFINITY {
            Self::MAX
        } else if px == f64::NEG_INFINITY {
            Self::MIN
        } else {
            Self::quantize(px).value
        }
    }

    fn clamp_raw(v: i64) -> Self {
        LayoutUnit(v.clamp(-(i32::MAX as i64), i32::MAX as i64) as i32)
    }
}

impl std::ops::Add for LayoutUnit {
    type Output = LayoutUnit;
    /// Saturating at the symmetric bound.
    fn add(self, rhs: Self) -> Self {
        Self::clamp_raw(self.0 as i64 + rhs.0 as i64)
    }
}
impl std::ops::Sub for LayoutUnit {
    type Output = LayoutUnit;
    /// Saturating at the symmetric bound.
    fn sub(self, rhs: Self) -> Self {
        Self::clamp_raw(self.0 as i64 - rhs.0 as i64)
    }
}

// ---------------------------------------------------------------------------------------------
// 008 T010 (R-37, fragment-tree.md section 1; D-075 additive): range statement constants,
// edge quantisation helpers and the saturation hook. Existing API unchanged.
// ---------------------------------------------------------------------------------------------

/// Exact-interop range with f32 layout computation: |v| < 2^18 CSS px.
pub const EXACT_RANGE_PX: f64 = 262_144.0;
/// Saturation bound in CSS px: (2^31 - 1) / 64.
pub const MAX_PX: f64 = (i32::MAX as f64) / 64.0;

/// Stage-diagnostic hook for saturation (`GEOMETRY_NON_FINITE`, stage `layout`): implemented by
/// the layout coordinator, which attaches the diagnostic to the epoch (de-duplicated there).
pub trait SaturationHook {
    fn saturated(&mut self, code: Code);
}

impl SaturationHook for Vec<Code> {
    fn saturated(&mut self, code: Code) {
        self.push(code);
    }
}

impl LayoutUnit {
    /// Quantise, reporting any saturation to `hook`.
    pub fn quantize_with(px: f64, hook: &mut impl SaturationHook) -> LayoutUnit {
        let q = Self::quantize(px);
        if let Some(code) = q.diagnostic {
            hook.saturated(code);
        }
        q.value
    }

    /// Whether `px` lies in the exact-interop range (published values outside it stay finite but
    /// computation resolution is coarser).
    pub fn in_exact_range(px: f64) -> bool {
        px.abs() < EXACT_RANGE_PX
    }

    /// Edge quantisation (fragment-tree.md section 1): `start` and `start + size` are rounded as
    /// edges (nearest 1/64, ties toward +infinity) and the size is the difference of the quantised
    /// edges, so adjacent boxes share an identical edge. Returns `(origin, size)`.
    pub fn quantize_span(
        start: f64,
        size: f64,
        hook: &mut impl SaturationHook,
    ) -> (LayoutUnit, LayoutUnit) {
        let a = Self::quantize_with(start, hook);
        let b = Self::quantize_with(start + size, hook);
        (a, b - a)
    }
}
