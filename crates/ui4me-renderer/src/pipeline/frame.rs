//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Frame handoff (005 recompute-plan §6 step 5): the committed scene is rastered with
//! full-surface damage (until X-DAMAGE), and every attempt's RenderReceipt goes to the receipt
//! store (the last 32 per target, recompute-plan §10). The slice rasters headless only; the
//! window target path is added with 011 (T055+). Implemented by 001 T050.

use crate::diagnostics::probes::render_receipts::{
    BudgetEcho, DegradedEcho, ReceiptInput, ReceiptStatusEcho, ReceiptTarget, ReceiptTimings,
};
use crate::pipeline::state::{DocumentPipeline, Trigger};
use crate::raster::headless::{PixelBuffer, RasterBackend};
use crate::raster::receipt::{BudgetKind, RenderMode, RenderReceipt};
pub use crate::raster::receipt::{ReceiptStatus, RenderTarget};
use crate::scene::appendix_e::SceneSnapshot;
use std::sync::Arc;
use ui4me_engine_types::revision::RevisionTuple;
use ui4me_engine_types::status::Status;

/// Receipts kept per target (recompute-plan §10).
pub const RECEIPT_RING: usize = 32;

/// Per-document frame state: the raster backend (created on the first frame) and the receipts.
#[derive(Debug)]
pub struct FrameState {
    backend: Option<RasterBackend>,
    receipts: Vec<RenderReceipt>,
    scale: f64,
    /// A presentation failure (`SURFACE_LOST`) happened: the next frame is a full repaint
    /// (FR-040). Frames are always full until X-DAMAGE passes; the flag keeps the rule once
    /// damage-limited rendering exists (T097).
    pub(crate) force_full_next: bool,
    /// Frames the damage raster produced without rastering every pixel.
    partial_frames: u64,
}

impl FrameState {
    pub(crate) fn new(scale: f64) -> FrameState {
        FrameState {
            backend: None,
            receipts: Vec::new(),
            scale,
            force_full_next: false,
            partial_frames: 0,
        }
    }

    pub(crate) fn set_scale(&mut self, scale: f64) {
        self.scale = scale;
    }

    fn push(&mut self, receipt: RenderReceipt) {
        self.receipts.push(receipt.clone());
        let target = receipt.target;
        let n = self.receipts.iter().filter(|r| r.target == target).count();
        if n > RECEIPT_RING
            && let Some(i) = self.receipts.iter().position(|r| r.target == target)
        {
            self.receipts.remove(i);
        }
    }
}

/// What the platform did with a frame (011 presentation.md §2; 010 render-receipt rule 3),
/// written back into the stored receipt by [`DocumentPipeline::record_presentation`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentUpdate {
    /// `window:<ordinal>`.
    pub target: RenderTarget,
    /// `Presented`, or `Failed(SURFACE_LOST)`.
    pub status: ReceiptStatus,
    pub present_ms: f64,
    pub presented_at_ms: f64,
    pub first_frame: bool,
}

/// One rastered frame.
#[derive(Clone, Debug)]
pub struct FrameOutput {
    pub pixels: Arc<PixelBuffer>,
    pub receipt: RenderReceipt,
    /// The committed revision the frame shows.
    pub revision: RevisionTuple,
}

/// Release the raster's spare frame buffers (the runtime calls this when it is destroyed).
pub fn trim_frame_pool() {
    crate::raster::headless::pool::trim();
}

impl FrameOutput {
    /// The frame has been presented (or discarded): give its pixel storage back to the raster's bounded frame-buffer
    /// pool so the next frame reuses it. Does nothing but drop when something else (a diagnostics export) still
    /// holds the pixels.
    pub fn recycle(self) {
        match Arc::try_unwrap(self.pixels) {
            Ok(pixels) => pixels.recycle(),
            Err(_) => crate::raster::headless::pool::note_shared(),
        }
    }
}

impl DocumentPipeline {
    /// Render the committed set headless (an explicit request: Flush first, then raster with
    /// full damage). Headless documents get no frames from the turn (FR-049); this is how the
    /// runtime obtains pixels for `pixels_ref` and the self-consistency checks.
    pub fn render_headless(&mut self) -> Result<FrameOutput, Status> {
        self.check_open()?;
        self.trigger(Trigger::Explicit)?;
        self.raster_committed()
    }

    /// Capture without scrollbar chrome when explicitly requested, preserving the committed scene.
    pub fn capture_headless(&mut self, hide_scrollbars: bool) -> Result<FrameOutput, Status> {
        self.capture_headless_with_options(hide_scrollbars, false)
    }

    /// Capture with optional scrollbar and caret suppression, without changing the committed scene.
    pub fn capture_headless_with_options(
        &mut self,
        hide_scrollbars: bool,
        hide_caret: bool,
    ) -> Result<FrameOutput, Status> {
        if !hide_scrollbars && !hide_caret {
            return self.render_headless();
        }
        self.check_open()?;
        self.trigger(Trigger::Explicit)?;
        let committed = self.committed_result()?.committed;
        let mut scene = (*committed.scene).clone();
        scene.display_list.items.retain(|item| {
            (!hide_scrollbars
                || item.phase != crate::scene::appendix_e::PaintPhase::OverlayScrollbar)
                && (!hide_caret || item.kind != crate::scene::display_list::ItemKind::CARET)
        });
        for (index, item) in scene.display_list.items.iter_mut().enumerate() {
            item.id = crate::scene::display_list::PaintItemId(index as u32);
        }
        // Do not contaminate the incremental renderer's normal frame cache.
        let (pixels, receipt) = RasterBackend::new()
            .render_headless(&scene, self.frames.scale as f32)
            .map_err(|e| e.status.clone())?;
        Ok(FrameOutput {
            pixels: Arc::new(pixels),
            receipt,
            revision: scene.revision,
        })
    }

    /// Raster the current committed scene (no update) and store the receipt.
    pub(crate) fn raster_committed(&mut self) -> Result<FrameOutput, Status> {
        let committed = self.committed_result()?.committed;
        let scale = self.frames.scale as f32;
        let backend = self.frames.backend.get_or_insert_with(RasterBackend::new);
        // The damage raster (crate::raster::damage) re-rasters only what differs from the previous
        // frame; the pixels equal a full raster's. After a presentation failure the next frame is a
        // full raster. The receipt keeps full-surface damage (the frame is complete).
        // `UI4ME_FULL_RASTER=1` turns the damage raster off (A/B measurements).
        // With it off (and in the no-reuse reference pipeline of the tests) every frame is a plain full
        // raster of every item, the reference the damage raster's frames (including its culled full
        // frames) are compared with.
        let plain = self.no_damage_raster || std::env::var_os("UI4ME_FULL_RASTER").is_some();
        if std::mem::take(&mut self.frames.force_full_next) || plain {
            backend.reset_damage();
        }
        let t_raster = std::time::Instant::now();
        let rendered = if plain {
            backend.render_headless(&committed.scene, scale)
        } else {
            backend
                .render_incremental(&committed.scene, scale)
                .map(|(p, r, outcome)| {
                    if outcome != crate::raster::headless::DamageOutcome::Full {
                        self.frames.partial_frames += 1;
                    }
                    (p, r)
                })
        };
        let raster_ms = t_raster.elapsed().as_secs_f64() * 1000.0;
        if std::env::var_os("UI4ME_PROFILE").is_some() {
            eprintln!(
                "[prof] raster {raster_ms:.2} (partial frames {})",
                self.frames.partial_frames
            );
        }
        match rendered {
            Ok((pixels, receipt)) => {
                let mut stage_ms = self.last_stage_ms;
                stage_ms.raster = Some(raster_ms);
                self.perf.push_frame(ui4me_engine_types::diag::FrameTiming {
                    frame_seq: receipt.frame_seq,
                    target: Some(match receipt.target {
                        RenderTarget::Window(n) => format!("window:{n}"),
                        RenderTarget::Headless => "headless".to_string(),
                    }),
                    stage_ms,
                    input_to_present_ms: receipt.nondeterministic.input_to_present_ms,
                    // from the earliest mutation this frame reflects (stamped at mutation time)
                    mutation_to_present_ms: self
                        .earliest_mutation
                        .take()
                        .map(|t| t.elapsed().as_secs_f64() * 1000.0),
                });
                // the stage durations belong to the update that produced the set; a later frame of
                // the same set repeats only the raster time
                self.last_stage_ms = ui4me_engine_types::diag::StageMs::default();
                self.frames.push(receipt.clone());
                Ok(FrameOutput {
                    pixels: Arc::new(pixels),
                    receipt,
                    revision: committed.revision,
                })
            }
            Err(failure) => {
                self.frames.push(failure.receipt.clone());
                Err(self.stores.record_error(
                    self.doc.id(),
                    failure.status.clone(),
                    committed.revision,
                ))
            }
        }
    }

    /// Write a presentation outcome into the stored receipt of the headless raster attempt
    /// `frame_seq` (the one the frame carried): target, status and the presentation timings
    /// (P-C2; 010 render-receipt rule 3). `frame_seq` stays the raster attempt counter. Returns
    /// whether the receipt was still in the store.
    pub fn record_presentation(&mut self, frame_seq: u64, update: &PresentUpdate) -> bool {
        let Some(r) = self
            .frames
            .receipts
            .iter_mut()
            .rev()
            .find(|r| r.frame_seq == frame_seq && r.target == RenderTarget::Headless)
        else {
            return false;
        };
        r.target = update.target;
        r.status = update.status;
        if matches!(update.status, ReceiptStatus::Failed(_)) {
            // Render-receipt rule 4: nothing new is on screen, so damage and degraded are empty.
            r.damage.clear();
            r.degraded.clear();
            self.frames.force_full_next = true;
        }
        r.nondeterministic.present_ms = update.present_ms;
        r.nondeterministic.presented_at_ms = update.presented_at_ms;
        r.nondeterministic.first_frame = update.first_frame;
        let target = update.target;
        if self
            .frames
            .receipts
            .iter()
            .filter(|r| r.target == target)
            .count()
            > RECEIPT_RING
            && let Some(i) = self.frames.receipts.iter().position(|r| r.target == target)
        {
            self.frames.receipts.remove(i);
        }
        true
    }

    /// The `performance` probe store (all members nondeterministic).
    pub fn performance(&self) -> &ui4me_engine_types::diag::PerformanceStore {
        &self.perf
    }

    /// The event loop's counters (004's loop counter and the idle-turn count) for the probe.
    pub fn set_loop_counters(&mut self, event_loop_wakes: u64, idle_turns: u64) {
        self.perf.event_loop_wakes = event_loop_wakes;
        self.perf.idle_turns = idle_turns;
    }

    /// A component reports its memory (`push_memory`, FR-077).
    pub fn push_memory(&mut self, component: &str, current_bytes: u64) {
        self.perf.push_memory(component, current_bytes);
    }

    /// Frames the damage raster produced by re-rastering only part of the surface (or nothing).
    pub fn partial_frames(&self) -> u64 {
        self.frames.partial_frames
    }

    /// The receipt store: the last 32 receipts per target, in render order.
    pub fn receipts(&self) -> &[RenderReceipt] {
        &self.frames.receipts
    }
}

/// Raster `scene` with a scratch backend (cold-oracle frame comparison; touches no store).
pub(crate) fn raster_scratch(scene: &SceneSnapshot, scale: f64) -> Result<PixelBuffer, Status> {
    RasterBackend::new()
        .render_headless(scene, scale as f32)
        .map(|(p, _)| p)
        .map_err(|f| f.status.clone())
}

/// A 010 receipt as the diagnostics probe input (plain data; IF-17 field names).
pub fn receipt_input(r: &RenderReceipt) -> ReceiptInput {
    ReceiptInput {
        frame_seq: r.frame_seq,
        target: match r.target {
            RenderTarget::Window(n) => ReceiptTarget::Window(n),
            RenderTarget::Headless => ReceiptTarget::Headless,
        },
        scene_generation: r.scene.generation,
        scene_revision: r.scene.revision,
        device_width: r.device.width,
        device_height: r.device.height,
        scale: f64::from(r.device.scale),
        full: r.mode == RenderMode::Full,
        damage: r.damage.clone(),
        degraded: r
            .degraded
            .iter()
            .map(|d| DegradedEcho {
                owner: d.owner,
                capability: d.capability,
                fallback: d.fallback.0,
            })
            .collect(),
        budget: r
            .budget
            .iter()
            .map(|b| BudgetEcho {
                owner: b.owner,
                kind: match b.kind {
                    BudgetKind::LayerDepth => "layer-depth",
                    BudgetKind::EffectSurface => "effect-surface",
                },
            })
            .collect(),
        status: match r.status {
            ReceiptStatus::Presented => ReceiptStatusEcho::Presented,
            ReceiptStatus::RenderedHeadless => ReceiptStatusEcho::RenderedHeadless,
            ReceiptStatus::Failed(c) => ReceiptStatusEcho::Failed(c),
        },
        capability_table: r.capability_table.clone(),
        nondeterministic: ReceiptTimings {
            raster_ms: r.nondeterministic.raster_ms,
            present_ms: r.nondeterministic.present_ms,
            presented_at_ms: r.nondeterministic.presented_at_ms,
            first_frame: r.nondeterministic.first_frame,
            input_to_present_ms: r.nondeterministic.input_to_present_ms,
        },
    }
}
