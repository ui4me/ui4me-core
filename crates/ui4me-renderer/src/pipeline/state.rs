//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Pipeline state machine (005 recompute-plan §2): `Clean`/`Dirty`/`Updating`/`FailedUpdate`
//! with `retry_allowed` and `cold_next`.
//!
//! **Slice test surface.** `DocumentPipeline` is the per-document pipeline (005 plan: `lib.rs`;
//! it lives here because the crate root is fixed by 001 T020). Tests drive it through a narrow
//! surface: [`DocumentPipeline::load_html`] (lifecycle.rs), DOM mutations and reads
//! (update.rs), [`DocumentPipeline::flush`] / [`DocumentPipeline::read`] (read.rs) and the
//! committed set (publish.rs). Which task implements what:
//! - T049: state.rs, lifecycle.rs, update.rs, publish.rs, read.rs (states, publication, read modes);
//! - T050: turn.rs, frame.rs, clock.rs, cold.rs (idle turn, frames and receipts, inert clock, cold run);
//! - T052/T053: the probes and the runtime's export assembly read the committed set.

use crate::diagnostics::probes::errors::ErrorEntry;
use crate::pipeline::clock::AnimationClock;
use crate::pipeline::frame::FrameState;
use crate::pipeline::publish::CommittedSet;
use crate::style::traverse::StyleBridge;
use crate::text::fonts::TextServices;
use std::path::PathBuf;
use std::sync::Arc;
use ui4me_assets::fonts::FontSourcesView;
use ui4me_document::document::Document;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{ErrorRecord, Ring, UnsupportedReports};
use ui4me_engine_types::revision::RevisionTuple;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PipelineState {
    Clean,
    Dirty,
    Updating,
    FailedUpdate,
}

impl PipelineState {
    pub const fn name(self) -> &'static str {
        match self {
            PipelineState::Clean => "Clean",
            PipelineState::Dirty => "Dirty",
            PipelineState::Updating => "Updating",
            PipelineState::FailedUpdate => "FailedUpdate",
        }
    }
}

/// `TRIGGER(kind)` (recompute-plan §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    FlushRead,
    Probe,
    Frame,
    Explicit,
    OraclePrep,
}

impl Trigger {
    pub const fn name(self) -> &'static str {
        match self {
            Trigger::FlushRead => "flush-read",
            Trigger::Probe => "probe",
            Trigger::Frame => "frame",
            Trigger::Explicit => "explicit",
            Trigger::OraclePrep => "oracle-prep",
        }
    }
}

/// `INPUT`, `TRIGGER(kind)`, `OK`, `STALE`, `FAIL(code, stage)`, `BOUND(iter)` (recompute-plan §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    Input,
    Trigger(Trigger),
    Ok,
    Stale,
    Fail,
    Bound,
}

/// What the caller does after an accepted event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Nothing to do (results come from the committed set, or a frame declined to retry).
    Nothing,
    /// An input was recorded.
    Noted,
    /// Run an update now, cold or incremental.
    StartUpdate { cold: bool },
    /// Publish the committed set (P).
    Publish,
    /// Publish the last iteration and attach the `LAYOUT_NON_CONVERGENCE` diagnostic.
    PublishNonConverged,
    /// Discard the draft; the inputs changed during the update.
    DiscardDraft,
    /// Discard the draft and store an error record (stage, diagnostic id).
    DiscardDraftAndRecordError,
}

/// An event that the transition table marks impossible for the state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IllegalEvent {
    pub state: PipelineState,
    pub event: Event,
}

/// The pipeline state machine with its flags (`cold_next`, `retry_allowed`, `failure_count`,
/// `non_convergence`). Pure: the update procedure in update.rs drives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateMachine {
    state: PipelineState,
    retry_allowed: bool,
    cold_next: bool,
    failure_count: u64,
    non_convergence: bool,
}

impl StateMachine {
    /// A parsed document starts `Dirty` (its DOM is an input the empty committed set lacks).
    pub fn new() -> StateMachine {
        StateMachine::in_state(PipelineState::Dirty)
    }

    /// A machine in `state` with no flags set (tests; `FailedUpdate` via events otherwise).
    pub fn in_state(state: PipelineState) -> StateMachine {
        StateMachine {
            state,
            retry_allowed: state == PipelineState::Dirty,
            cold_next: false,
            failure_count: 0,
            non_convergence: false,
        }
    }

    pub fn state(&self) -> PipelineState {
        self.state
    }
    pub fn retry_allowed(&self) -> bool {
        self.retry_allowed
    }
    pub fn cold_next(&self) -> bool {
        self.cold_next
    }
    pub fn failure_count(&self) -> u64 {
        self.failure_count
    }
    pub fn non_convergence(&self) -> bool {
        self.non_convergence
    }

    #[doc(hidden)]
    pub fn set_cold_next_for_testing(&mut self, v: bool) {
        self.cold_next = v;
    }

    /// Apply `event` per the transition table; an impossible event changes nothing.
    pub fn on(&mut self, event: Event) -> Result<Effect, IllegalEvent> {
        use PipelineState::*;
        let illegal = IllegalEvent {
            state: self.state,
            event,
        };
        match (self.state, event) {
            (Clean, Event::Input) => {
                self.state = Dirty;
                Ok(Effect::Noted)
            }
            (Dirty, Event::Input) => Ok(Effect::Noted),
            (FailedUpdate, Event::Input) => {
                self.retry_allowed = true;
                Ok(Effect::Noted)
            }
            (Updating, Event::Input) => Err(illegal),

            (Clean, Event::Trigger(_)) => Ok(Effect::Nothing),
            (Dirty, Event::Trigger(kind)) => Ok(self.start(kind == Trigger::OraclePrep)),
            (FailedUpdate, Event::Trigger(kind)) => {
                if self.retry_allowed
                    || matches!(
                        kind,
                        Trigger::FlushRead | Trigger::Probe | Trigger::Explicit
                    )
                {
                    // The retry is cold (`cold_next` is set by the failure).
                    Ok(self.start(true))
                } else {
                    Ok(Effect::Nothing)
                }
            }
            (Updating, Event::Trigger(_)) => Err(illegal),

            (Updating, Event::Ok) => {
                self.state = Clean;
                self.cold_next = false;
                Ok(Effect::Publish)
            }
            (Updating, Event::Stale) => {
                self.state = Dirty;
                Ok(Effect::DiscardDraft)
            }
            (Updating, Event::Fail) => {
                self.state = FailedUpdate;
                self.cold_next = true;
                self.failure_count += 1;
                Ok(Effect::DiscardDraftAndRecordError)
            }
            (Updating, Event::Bound) => {
                self.state = Clean;
                self.cold_next = false;
                self.non_convergence = true;
                Ok(Effect::PublishNonConverged)
            }
            (_, Event::Ok | Event::Stale | Event::Fail | Event::Bound) => Err(illegal),
        }
    }

    fn start(&mut self, cold_requested: bool) -> Effect {
        self.state = PipelineState::Updating;
        self.retry_allowed = false;
        self.non_convergence = false;
        Effect::StartUpdate {
            cold: cold_requested || self.cold_next,
        }
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        StateMachine::new()
    }
}

/// Per-document environment and font configuration (004 names for the font options, R-37).
#[derive(Clone, Debug, PartialEq)]
pub struct PipelineConfig {
    /// CSS px.
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub scale: f64,
    /// Pinned font files (pinned-fonts-only mode, 007).
    pub font_files: Vec<PathBuf>,
}

/// Capacity of the error ring (recompute-plan §10).
pub const ERROR_RING: usize = 256;
/// Updates retained by the invalidation record ring (recompute-plan §10).
pub const INVALIDATION_RING: usize = 16;

/// The update mode (recompute-plan §4 step 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateMode {
    Incremental,
    Cold,
}

/// How an update ended (recompute-plan §2 events).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The feedback-loop bound was reached: the last iteration is published (`BOUND`, FR-041).
    NonConvergence,
    Ok,
    Stale,
    Fail(Code, Stage),
}

/// One `invalidation` probe record (recompute-plan §4 step 9; schema §5). The slice keeps the
/// ring; the probe itself is 005's later work (`UNSUPPORTED_PROBE` in the slice export).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateRecord {
    pub update_seq: u64,
    pub trigger: Trigger,
    pub mode: UpdateMode,
    /// DOM revisions folded into this update (`from` exclusive, `to` inclusive).
    pub dom_from: u64,
    pub dom_to: u64,
    pub stages_run: Vec<Stage>,
    /// v1 always widens (conservative full rebuild of box, layout and scene).
    pub widened: bool,
    pub outcome: UpdateOutcome,
    /// The coalesced `(reason, subject)` inputs folded into this update (S0).
    pub reasons: Vec<crate::pipeline::dirty::DirtyRecord>,
    /// The error record's diagnostic id when the update failed.
    pub diagnostic_id: Option<u64>,
    /// Feedback-loop iterations `(relational, container)`; `(0, 0)` while both are inert.
    pub loop_iterations: (u32, u32),
}

/// 005's diagnostics stores for one document (R-20 unsupported store, error ring, update ring).
#[derive(Clone, Debug)]
pub struct DiagnosticStores {
    pub unsupported: UnsupportedReports,
    pub errors: Ring<ErrorEntry>,
    pub updates: Ring<UpdateRecord>,
    pub(crate) next_diagnostic_id: u64,
}

impl Default for DiagnosticStores {
    fn default() -> Self {
        DiagnosticStores {
            unsupported: UnsupportedReports::default(),
            errors: Ring::on_demand(ERROR_RING),
            updates: Ring::on_demand(INVALIDATION_RING),
            next_diagnostic_id: 1,
        }
    }
}

impl DiagnosticStores {
    /// Record an error with a fresh diagnostic id (≥ 1) and the revision it belongs to; the
    /// returned status carries the id.
    pub fn record_error(
        &mut self,
        document: ui4me_engine_types::ids::DocumentId,
        mut status: Status,
        revision: RevisionTuple,
    ) -> Status {
        if status.stage == Stage::None {
            // Every record carries one stage (FR-013); unattributed failures belong to the host boundary.
            status.stage = Stage::Host;
        }
        if status.diagnostic_id == 0 {
            status.diagnostic_id = self.next_diagnostic_id;
            self.next_diagnostic_id += 1;
        }
        self.errors.push(ErrorEntry {
            record: ErrorRecord {
                document: Some(document),
                status: status.clone(),
            },
            revision,
        });
        status
    }
}

/// Output epochs, assigned at publication (recompute-plan §4 step 9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OutputEpochs {
    pub style: u64,
    pub layout: u64,
    pub scene: u64,
}

/// The per-document pipeline: DOM inputs, the state machine and the committed set.
pub struct DocumentPipeline {
    pub(crate) doc: Document,
    pub(crate) config: PipelineConfig,
    /// 007's font sources (shared with the cold scratch pipeline).
    pub(crate) fonts: FontSourcesView,
    /// The `@font-face` rules last exported to 007 (`font_faces.rs`).
    pub(crate) exported_faces: Vec<crate::style::font_faces::FontFaceExport>,
    /// A face became usable or left during an update: the next update rebuilds the font context.
    pub(crate) font_rebuild_pending: bool,
    pub(crate) text: TextServices,
    pub(crate) style: StyleBridge,
    pub(crate) machine: StateMachine,
    /// Container facts between layout epochs (inert in v1; test doubles exercise the bound).
    /// A test source for the container loop (`set_container_facts_for_testing`); `None` runs the real loop.
    pub(crate) facts: Option<Box<dyn crate::pipeline::feedback::ContainerFacts>>,
    /// Coalesced inputs since the last publication (S0).
    pub(crate) dirty: crate::pipeline::dirty::DirtySets,
    /// Snapshot views (recompute-plan §9).
    pub(crate) views: crate::pipeline::views::Views,
    /// The `performance` probe store (T039; every member nondeterministic).
    pub(crate) perf: ui4me_engine_types::diag::PerformanceStore,
    /// Stage durations of the last update, attached to the next frame it produces.
    pub(crate) last_stage_ms: ui4me_engine_types::diag::StageMs,
    /// The earliest mutation not yet shown by a frame (stamped at mutation time, T076).
    pub(crate) earliest_mutation: Option<std::time::Instant>,
    /// Test hook: never take the paint-only path of an animation tick (the differential tests
    /// compare it with the full rebuild).
    pub(crate) no_animation_paint_path: bool,
    /// Test hook: never reuse the committed layout when the box tree is unchanged (the differential tests compare it with reuse).
    pub(crate) no_layout_reuse: bool,
    /// Scene reuse (015 PERF-2, `crate::scene::reuse`): element-box paint subtrees kept from one scene to the next.
    /// On by default since F1004 (heap plateau qualified: `long_document` tests, F1006 soak); off with `UI4ME_SCENE_REUSE=0`. Formerly off (PERF-4). On with
    /// `set_scene_reuse(true)` or `UI4ME_SCENE_REUSE=1` (tests, bench).
    pub(crate) scene_reuse: bool,
    /// Test hook: every frame is a full raster (the damage raster is compared with it).
    pub(crate) no_damage_raster: bool,
    /// Moves whenever the declared-face table or the faces' states change: layout depends on them (what the text
    /// asks to load, which face it uses) without any style value changing. A layout is reused only while it holds.
    pub(crate) font_gen: u64,
    /// `font_gen` when the committed layout was computed.
    pub(crate) layout_font_gen: u64,
    /// Box styles mapped from the style snapshot, reused for elements whose computed values did not change.
    pub(crate) box_styles: crate::layout::box_tree::BoxStyleCache,
    /// Derived presentation only; selected paths remain owned by the public Document.
    pub(crate) file_input_labels:
        std::collections::HashMap<ui4me_engine_types::ids::NodeId, String>,
    /// The derived label cache is outside the DOM journal. Until a successful
    /// commit, its changes invalidate box/layout and scene reuse independently.
    pub(crate) file_input_labels_dirty: bool,
    /// Paint styles of the last scene, by element and computed values (L-PERF-b3).
    pub(crate) paint_styles: std::cell::RefCell<crate::pipeline::update::PaintStyleCache>,
    /// Updates that reused the committed layout because the box tree did not change.
    pub(crate) layout_reuses: u64,
    /// Updates that took the animation paint-only path (S5 only, no box tree or layout).
    pub(crate) animation_paint_updates: u64,
    /// Environment epoch (viewport, scale, media); 1 from load.
    pub(crate) environment_epoch: u64,
    /// Resource epoch: +1 per completion batch applied in turn step 3 (007; recompute-plan §6).
    pub(crate) resource_epoch: u64,
    /// Scroll epoch: +1 per ScrollState change (012).
    pub(crate) scroll_epoch: u64,
    /// The scroll offsets in force (an input of S5; written only by InteractionRuntime, see
    /// `scroll.rs`).
    pub(crate) scroll: crate::layout::coordinator::overflow::ScrollOffsets,
    /// Caret blink phase (012 owns the timer): painted by S5 for a focused text control.
    pub(crate) caret_visible: bool,
    /// The caret belongs after the end of the previous line (affinity of End on a wrapped line).
    pub(crate) caret_upstream: bool,
    /// The environment changed since the last publication: the next update restyles against a
    /// fresh stylist (media/viewport units) and rebuilds layout and scene (005 P-C1 / AB).
    pub(crate) env_pending: bool,
    /// Font-source/face changes require a fresh text collection; viewport, DPI,
    /// colour scheme and reduced motion do not.
    pub(crate) font_context_pending: bool,
    /// The document's `prefers-reduced-motion: reduce` preference (006 P-006-1); set through
    /// [`DocumentPipeline::set_reduced_motion`], default no-preference.
    pub(crate) reduced_motion: bool,
    /// The document's `prefers-color-scheme: dark` preference (006), set through
    /// [`DocumentPipeline::set_dark`]; default light.
    pub(crate) dark: bool,
    pub(crate) committed: Option<Arc<CommittedSet>>,
    pub(crate) epochs: OutputEpochs,
    /// Scene generation counter (one per publication).
    pub(crate) generation: u64,
    pub(crate) updates_run: u64,
    pub(crate) fault: Option<(Stage, Code)>,
    /// Test-only injection points (T022); compiled out of release builds.
    #[cfg(feature = "fault-injection")]
    pub(crate) faults: crate::pipeline::fault::FaultPlan,
    pub(crate) stores: DiagnosticStores,
    pub(crate) frames: FrameState,
    pub(crate) clock: AnimationClock,
    /// Decoded images for CSS `url()` and `<img>` (007; set by the host or a dev tool). `None`
    /// until the resource manager is wired to the pipeline: image references are then reported
    /// `UNSUPPORTED_FEATURE` by the scene stage, never dropped silently.
    pub(crate) images: Option<Arc<dyn crate::scene::inputs::ImageSource>>,
    /// The document's view of 007 (linked sheets, `@import`, images); `None` for a bare pipeline
    /// that has no resource manager (a `<link>` and `url()` image are then reported unsupported).
    pub(crate) resources: Option<Arc<crate::pipeline::resources::PipelineResources>>,
}

impl std::fmt::Debug for DocumentPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentPipeline")
            .field("document", &self.doc)
            .field("state", &self.machine.state())
            .field("revision", &self.committed.as_ref().map(|c| c.revision))
            .field("updates_run", &self.updates_run)
            .finish_non_exhaustive()
    }
}

impl DocumentPipeline {
    /// Test hook: attach a decoded-image provider for the scene stage (fakes in the paint tests).
    /// A pipeline with a resource manager gets its provider from
    /// [`DocumentPipeline::attach_resources`] instead. Marks the scene stale only through the next
    /// update; callers set it before the first one.
    #[doc(hidden)]
    pub fn set_image_source(&mut self, source: Option<Arc<dyn crate::scene::inputs::ImageSource>>) {
        self.images = source;
    }

    pub fn state(&self) -> PipelineState {
        self.machine.state()
    }

    /// The state machine with its flags (`retry_allowed`, `cold_next`, `non_convergence`).
    pub fn machine(&self) -> &StateMachine {
        &self.machine
    }

    /// The configuration the pipeline was created with.
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// The document (read-only; mutations go through the pipeline so that `INPUT` is noted).
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// 005's diagnostics stores of this document.
    pub fn stores(&self) -> &DiagnosticStores {
        &self.stores
    }

    /// The text services (font environment) of the live pipeline.
    pub fn text_services(&self) -> &TextServices {
        &self.text
    }

    /// The font sources the pipeline was built from.
    pub fn font_sources(&self) -> &FontSourcesView {
        &self.fonts
    }

    /// The live style bridge (computed-value host reads, 006 `style.computed_value`).
    pub fn style_bridge(&self) -> &StyleBridge {
        &self.style
    }

    /// The current committed set, if one was published.
    pub fn committed(&self) -> Option<&Arc<CommittedSet>> {
        self.committed.as_ref()
    }

    /// Whether the last update failed and the next one runs cold.
    pub fn cold_next(&self) -> bool {
        self.machine.cold_next()
    }
}
