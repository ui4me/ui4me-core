//! owner: 007-resources (T016, T026, T033) — consumer views (`contracts/resource-view.md`).
//!
//! Views are immutable and epoch-tagged; no codec, usvg/resvg, tiny-skia or Fontique type appears
//! here (AD-33). The functions that produce them are methods of `ResourceManager` (lib.rs):
//! `load_text_sync`, `request_css_image`, `sync_font_faces`, `natural_size`, `paint_image`,
//! `font_sources`, `request_face`, `process_completions` (→ `on_completions_applied`) and `pending`.

use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::DocumentId;
use ui4me_engine_types::resource_ids::{ResourceEpoch, ResourceRef};

pub use crate::fonts::{FaceState, FaceView, FontSourcesView};
pub use crate::text::TextResource;

/// Premultiplied RGBA8, sRGB, at most 65,535 px per side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRaster {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub data: Arc<[u8]>,
}

/// For 008: the natural size of a request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageState {
    /// Lay out as "no natural dimensions" (or the previous image's).
    Pending,
    /// Density-corrected CSS px; absent = no natural dimension.
    Ready {
        width_css: Option<f32>,
        height_css: Option<f32>,
        ratio: Option<(f32, f32)>,
    },
    Broken {
        code: Code,
    },
}

/// For 010: pixels to paint.
#[derive(Clone, Debug)]
pub enum PaintImage {
    Raster {
        pixels: Arc<ImageRaster>,
        generation: u64,
    },
    /// Paint nothing (or the previous raster) and report `RESOURCE_PENDING`.
    NotReady,
    Broken {
        code: Code,
    },
}

/// What the host sees per image element (`complete`, natural size, failure code; T033, for 004).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageElementData {
    pub complete: bool,
    pub natural_width: u32,
    pub natural_height: u32,
    pub state: ImageStateTag,
    pub failure: Option<Code>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageStateTag {
    Pending,
    Ready,
    Broken,
}

/// For 005: one drain step's result for one document (`on_completions_applied`). Everything in
/// `changed` belongs to one batch: one resource-epoch increment and one relayout.
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedBatch {
    pub document: DocumentId,
    pub resource_epoch: ResourceEpoch,
    pub changed: Vec<ResourceRef>,
    /// Natural size changed (BOX/LAY dirty reason).
    pub intrinsic_size: Vec<ResourceRef>,
    /// Same natural size, new pixels (SCN dirty reason).
    pub paint_only: Vec<ResourceRef>,
    pub font_epoch_changed: bool,
}
