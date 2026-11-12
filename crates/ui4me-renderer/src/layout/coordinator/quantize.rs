//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! Output quantisation (008 `fragment-tree.md` §1; AD-15): edges are rounded to the nearest
//! 1/64 px, ties toward +∞, in the parent's coordinate space; sizes are differences of quantised
//! edges. Out-of-range or non-finite values become bounds/0 plus a `GEOMETRY_NON_FINITE` stage
//! diagnostic (never an error).

use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::geometry::layout_unit::LayoutUnit;
use ui4me_engine_types::geometry::{LayoutEdges, LayoutPoint, LayoutRect, LayoutSize};

/// Collects whether any quantisation needed the non-finite/overflow rule.
#[derive(Debug, Default, Clone, Copy)]
pub struct QuantizeSink {
    pub non_finite: bool,
}

impl QuantizeSink {
    pub fn edge(&mut self, px: f32) -> LayoutUnit {
        let q = LayoutUnit::quantize(px as f64);
        if q.diagnostic == Some(Code::GeometryNonFinite) {
            self.non_finite = true;
        }
        q.value
    }

    /// A rect from unrounded (x, y, w, h) in parent space: quantise the four edges.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) -> LayoutRect {
        let (l, t) = (self.edge(x), self.edge(y));
        let (r, b) = (self.edge(x + w), self.edge(y + h));
        LayoutRect {
            origin: LayoutPoint { x: l, y: t },
            size: LayoutSize::from_edges(l, r, t, b),
        }
    }

    pub fn edges(&mut self, top: f32, right: f32, bottom: f32, left: f32) -> LayoutEdges {
        LayoutEdges {
            top: self.edge(top),
            right: self.edge(right),
            bottom: self.edge(bottom),
            left: self.edge(left),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_then_sizes() {
        let mut q = QuantizeSink::default();
        // x = 0.3/64 rounds to 0, x + w = 10.0 + 0.3/64 rounds to 640 units.
        let r = q.rect(0.3 / 64.0, 0.0, 10.0, 1.0);
        assert_eq!((r.origin.x.raw(), r.size.width.raw()), (0, 640));
        // Ties toward +inf: 0.5/64 → 1 unit.
        assert_eq!(q.edge(0.5 / 64.0).raw(), 1);
        assert_eq!(q.edge(-0.5 / 64.0).raw(), 0);
        assert!(!q.non_finite);
        assert_eq!(q.edge(f32::NAN), LayoutUnit::ZERO);
        assert!(q.non_finite);
    }
}
