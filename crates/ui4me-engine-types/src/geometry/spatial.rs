//! owner: 010-paint-render — created by 010 T009 inside 001's crate (R-18, R-28; co-sign D-075)
//!
//! Scroll, sticky, fixed and chain composition (spatial-tree.md §2–§3). Pure functions over
//! values; the capability fallbacks are a value input (`TransformFallback`), never read from a
//! table here (FR-018, Clarification Q1).

use super::transform::{
    Affine2D, GeoResult, RectF, Transform3D, compose, flatten, is_backface_visible,
};
use crate::codes_generated::Code;

/// `(-x, -y)`: the content translation of a scroll container at `offset`.
pub fn scroll_translate(offset: (f64, f64)) -> GeoResult<Affine2D> {
    if !(offset.0.is_finite() && offset.1.is_finite()) {
        return Err(Code::GeometryNonFinite);
    }
    Ok(Affine2D::translate(-offset.0, -offset.1))
}

/// Fixed-position boxes cancel the root scroll: `(+x, +y)`.
pub fn fixed_anchor(viewport_scroll: (f64, f64)) -> GeoResult<Affine2D> {
    if !(viewport_scroll.0.is_finite() && viewport_scroll.1.is_finite()) {
        return Err(Code::GeometryNonFinite);
    }
    Ok(Affine2D::translate(viewport_scroll.0, viewport_scroll.1))
}

/// Resolved sticky insets in px; `None` is `auto` (no constraint).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StickyInsets {
    pub top: Option<f64>,
    pub right: Option<f64>,
    pub bottom: Option<f64>,
    pub left: Option<f64>,
}

/// The value fields of 008's `StickyConstraints`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickyValues {
    pub containing_block_rect: RectF,
    pub insets: StickyInsets,
    pub normal_flow_rect: RectF,
}

/// One axis: `(start, end)` insets, the element's `(n0, n1)` edges, the view's `(v0, v1)` edges
/// and the containing block's `(c0, c1)` edges.
fn axis(insets: (Option<f64>, Option<f64>), n: (f64, f64), v: (f64, f64), c: (f64, f64)) -> f64 {
    // The end inset is applied first, so the start inset wins when both cannot hold.
    let mut d = 0.0f64;
    if let Some(h) = insets.1 {
        d = d.min((v.1 - h) - n.1);
    }
    if let Some(l) = insets.0 {
        d = d.max((v.0 + l) - n.0);
    }
    // Never leave the containing block (upper bound first, so the start edge wins on overflow).
    d = d.min(c.1 - n.1);
    d.max(c.0 - n.0)
}

/// CSS Position 3 sticky offset of `normal_flow_rect` for the sticky view rectangle `scroll_view`
/// (the scrollport in Document space after its own scroll offset).
pub fn sticky_offset(c: &StickyValues, scroll_view: &RectF) -> GeoResult<(f64, f64)> {
    let ins = &c.insets;
    let all_ok = c.containing_block_rect.is_finite()
        && c.normal_flow_rect.is_finite()
        && scroll_view.is_finite()
        && [ins.top, ins.right, ins.bottom, ins.left]
            .iter()
            .all(|v| v.is_none_or(f64::is_finite));
    if !all_ok {
        return Err(Code::GeometryNonFinite);
    }
    let n = &c.normal_flow_rect;
    let cb = &c.containing_block_rect;
    let v = scroll_view;
    let dx = axis(
        (ins.left, ins.right),
        (n.x, n.right()),
        (v.x, v.right()),
        (cb.x, cb.right()),
    );
    let dy = axis(
        (ins.top, ins.bottom),
        (n.y, n.bottom()),
        (v.y, v.bottom()),
        (cb.y, cb.bottom()),
    );
    Ok((dx, dy))
}

/// Transform capability fallbacks in force (a value supplied by the runtime from the active
/// capability table; `effect-tiers.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransformFallback {
    pub perspective: PerspectiveMode,
    pub preserve_3d: Preserve3dMode,
}

impl TransformFallback {
    pub const EXACT: TransformFallback = TransformFallback {
        perspective: PerspectiveMode::Exact,
        preserve_3d: Preserve3dMode::Exact,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerspectiveMode {
    Exact,
    /// Use `flatten(m).affine` and never report a projective result.
    AffinePart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preserve3dMode {
    Exact,
    /// Compose as `transform-style: flat`: each transform step is flattened first.
    Flat,
}

/// One link of the root-to-leaf chain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpatialStep {
    /// The fragment's origin offset in its parent.
    Translate((f64, f64)),
    /// A scroll container's offset (applies `(-x, -y)`).
    Scroll((f64, f64)),
    /// A sticky shift, already computed by `sticky_offset`.
    Sticky((f64, f64)),
    /// A resolved transform matrix (origin included).
    Transform(Transform3D),
    /// A parent's perspective matrix.
    Perspective(Transform3D),
    /// A fixed-position box: cancels the root scroll offset.
    Fixed((f64, f64)),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accumulated {
    pub affine: Affine2D,
    pub projective: bool,
    /// The accumulated matrix shows the back face (hidden by `backface-visibility: hidden`).
    pub backface_visible: bool,
}

/// Compose `steps` (root first). Each step is applied to the points of the next step.
pub fn compose_chain(steps: &[SpatialStep], fallback: TransformFallback) -> GeoResult<Accumulated> {
    let mut acc = Transform3D::IDENTITY;
    for step in steps {
        let m = match *step {
            SpatialStep::Translate((x, y)) | SpatialStep::Sticky((x, y)) => {
                if !(x.is_finite() && y.is_finite()) {
                    return Err(Code::GeometryNonFinite);
                }
                Affine2D::translate(x, y).to_3d()
            }
            SpatialStep::Scroll(o) => scroll_translate(o)?.to_3d(),
            SpatialStep::Fixed(o) => fixed_anchor(o)?.to_3d(),
            SpatialStep::Transform(m) => match fallback.preserve_3d {
                Preserve3dMode::Exact => m,
                Preserve3dMode::Flat => flatten(&m)?.affine.to_3d(),
            },
            SpatialStep::Perspective(m) => m,
        };
        acc = compose(&acc, &m)?;
    }
    let f = flatten(&acc)?;
    let projective = f.projective && fallback.perspective == PerspectiveMode::Exact;
    Ok(Accumulated {
        affine: f.affine,
        projective,
        backface_visible: is_backface_visible(&acc),
    })
}
