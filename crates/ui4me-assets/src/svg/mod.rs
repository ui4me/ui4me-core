//! owner: 007-resources (T045–T053) — static SVG images (AD-24).
//!
//! * `prescan` — owned roxmltree pre-scan: natural size and ratio (nothing invented), `<text>`
//!   detection, element limit, gzip refusal;
//! * `usvg_adapter` — the usvg/resvg `SvgRenderer` with the policy `ImageHrefResolver`;
//! * `raster_cache` — device-scale rasterisation on the owner thread, cached per (entry, size).
//!
//! A parsed SVG is published in the runtime cache as a `VectorImage` (the parsed tree plus its
//! natural size); it has no raster of its own, only a one-pixel placeholder.

use crate::decode::{SvgNatural, SvgRenderer, SvgTree};
use std::sync::Arc;

/// R-20 report key of a root inline `<svg>` (FR-043, OD-09): parsed into the DOM by 003 but not
/// rendered; layout treats it as a replaced element without natural dimensions (008).
pub const INLINE_SVG_KEY: &str = "html.inline-svg";
/// R-20 report key of `<text>` inside an SVG image (FR-039): text is not rendered.
pub const TEXT_KEY: &str = "svg.text";

pub mod hit;
pub mod prescan;
pub mod raster_cache;
pub mod usvg_adapter;

/// The parsed, immutable SVG of one cache entry.
pub struct VectorImage {
    pub tree: SvgTree,
    pub natural: SvgNatural,
    pub renderer: Arc<dyn SvgRenderer>,
    /// Estimate of the parsed tree's memory, counted against the decoded-memory budget.
    pub est_bytes: u64,
}

impl std::fmt::Debug for VectorImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorImage")
            .field("natural", &self.natural)
            .field("est_bytes", &self.est_bytes)
            .finish()
    }
}
