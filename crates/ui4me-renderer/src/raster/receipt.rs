//! owner: 010-paint-render — created by 001 slice (R-28)
//!
//! RenderReceipt (010 `contracts/render-receipt.md` §1; field names identical to 005's export,
//! IF-17). One receipt per render attempt, including failed ones.
//! T032 types; assembly implemented by 001 T048 (`headless::RasterBackend::render_headless`).

use crate::raster::caps::FallbackId;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::CapabilityId;
use ui4me_engine_types::geometry::DeviceRect;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::revision::RevisionTuple;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderTarget {
    /// `window:<ordinal>` (005 renumbers ordinals in the export).
    Window(u32),
    Headless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneRef {
    pub generation: u64,
    pub revision: RevisionTuple,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceInfo {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    Full,
    Damage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradedEntry {
    pub owner: Option<NodeId>,
    pub capability: CapabilityId,
    pub fallback: FallbackId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetKind {
    LayerDepth,
    EffectSurface,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetEntry {
    pub owner: Option<NodeId>,
    pub kind: BudgetKind,
    /// Always `BUDGET_EXCEEDED`.
    pub code: Code,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptStatus {
    Presented,
    RenderedHeadless,
    /// The frame could not be produced (for example `OUT_OF_MEMORY`, `SIZE_LIMIT`,
    /// `BUDGET_EXCEEDED`, `UNSUPPORTED_FEATURE`, `INTERNAL_INVARIANT` or `SURFACE_LOST`).
    Failed(Code),
}

/// Excluded from comparisons (export member `nondeterministic`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReceiptNondeterministic {
    pub raster_ms: f64,
    pub present_ms: f64,
    pub presented_at_ms: f64,
    pub first_frame: bool,
    pub input_to_present_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderReceipt {
    /// Per target, from 1.
    pub frame_seq: u64,
    pub target: RenderTarget,
    pub scene: SceneRef,
    pub device: DeviceInfo,
    pub mode: RenderMode,
    /// Rects actually rasterised (`Full` ⇒ one rect = target).
    pub damage: Vec<DeviceRect>,
    /// Sorted by (paint order, capability).
    pub degraded: Vec<DegradedEntry>,
    pub budget: Vec<BudgetEntry>,
    pub status: ReceiptStatus,
    /// Table-key digest (`CapabilityTable::digest`).
    pub capability_table: String,
    pub nondeterministic: ReceiptNondeterministic,
}
