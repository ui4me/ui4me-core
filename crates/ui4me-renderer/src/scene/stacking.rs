//! owner: 010-paint-render — created by 010 T001; classification subset of T039 (FR-010)
//!
//! Stacking-context classification over the paint-facing box styling. The triggers that the
//! scene input carries today are `opacity` below 1, a transform, a `mask-image`, a `filter`, a `clip-path`, and a positioned box with a
//! `z-index` other than `auto`; further triggers (blend modes, `isolation`, `will-change`,
//! flex/grid items with `z-index`, containment) join when 006 publishes them.

use crate::scene::inputs::{BoxExtras, PaintClass};

/// Whether a box with these extras forms a stacking context of its own (and is painted
/// atomically at Appendix E step 3, 8 or 9 of its parent).
pub fn creates_stacking_context(e: &BoxExtras) -> bool {
    e.opacity < 1.0
        || e.blend_mode != crate::scene::display_list::BlendMode::Normal
        || e.isolation
        || e.transform.is_some()
        || !e.mask.is_empty()
        || !e.filter.is_empty()
        || !e.backdrop_filter.is_empty()
        || e.forms_stacking_context
        || e.clip_path.is_some()
        || (e.class == PaintClass::Positioned && e.z_index.is_some())
}
