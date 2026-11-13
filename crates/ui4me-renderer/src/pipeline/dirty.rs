//! owner: 005-pipeline-diagnostics — T015. Coalesced invalidation records and the per-stage dirty
//! sets (`contracts/invalidation-reasons.md`; recompute-plan §4 step 2).
//!
//! The dirty sets are always a **superset** of the true change set (FR-020): with the v1 policy
//! each recorded reason dirties its widened stage set. Coalescing is a union that never drops a
//! reason or a subject (FR-008). Scoped mechanisms later narrow the stage sets only behind
//! recorded cold-oracle evidence (T086, T093, T095).

pub use crate::pipeline::reasons::Widening;
use crate::pipeline::reasons::{Reason, StageSet, Subject};
use crate::pipeline::state::DocumentPipeline;
use ui4me_engine_types::status::Status;

/// One coalesced `(reason, subject)` entry with its occurrence count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirtyRecord {
    pub reason: Reason,
    pub subject: Subject,
    pub count: u64,
}

/// The coalesced inputs since the last publication.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirtySets {
    records: Vec<DirtyRecord>,
    widening: Widening,
}

impl DirtySets {
    /// Record one `INPUT(reason)`; repeats of one `(reason, subject)` merge into one entry.
    pub fn note(&mut self, reason: Reason, subject: Subject) {
        match self
            .records
            .iter_mut()
            .find(|r| r.reason == reason && r.subject == subject)
        {
            Some(r) => r.count += 1,
            None => self.records.push(DirtyRecord {
                reason,
                subject,
                count: 1,
            }),
        }
    }

    /// Document facts that widen `ENVIRONMENT_SCALE` (a resolution media query exists).
    pub fn set_widening(&mut self, widening: Widening) {
        self.widening = widening;
    }

    pub fn records(&self) -> &[DirtyRecord] {
        &self.records
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The distinct reasons, in first-seen order.
    pub fn reasons(&self) -> Vec<Reason> {
        let mut out = Vec::new();
        for r in &self.records {
            if !out.contains(&r.reason) {
                out.push(r.reason);
            }
        }
        out
    }

    pub fn has(&self, reason: Reason, subject: &Subject) -> bool {
        self.records
            .iter()
            .any(|r| r.reason == reason && &r.subject == subject)
    }

    /// Total occurrences of `reason`.
    pub fn count(&self, reason: Reason) -> u64 {
        self.records
            .iter()
            .filter(|r| r.reason == reason)
            .map(|r| r.count)
            .sum()
    }

    /// The stages the minimum column dirties.
    pub fn minimum_stages(&self) -> StageSet {
        self.records
            .iter()
            .fold(StageSet::EMPTY, |a, r| a.union(r.reason.minimum_stages()))
    }

    /// The per-stage dirty set under the v1 policy (always a superset of the minimum).
    pub fn stages(&self) -> StageSet {
        self.records.iter().fold(StageSet::EMPTY, |a, r| {
            a.union(r.reason.widened_stages(self.widening))
        })
    }

    /// Whether a conservative step was used where a scoped one exists in principle
    /// (`widened` in the update record).
    pub fn widened(&self) -> bool {
        self.records.iter().any(|r| r.reason.is_widened_in_v1())
    }

    pub fn clear(&mut self) {
        self.records.clear();
    }

    /// Take the records (publication starts a new epoch).
    pub fn take(&mut self) -> Vec<DirtyRecord> {
        std::mem::take(&mut self.records)
    }
}

impl DocumentPipeline {
    /// `note_input(reason, subject)`: records the input and applies `INPUT` to the state machine
    /// (`Clean → Dirty`; `FailedUpdate` stays and sets `retry_allowed`).
    pub fn note_input_reason(&mut self, reason: Reason, subject: Subject) {
        self.dirty.note(reason, subject);
        self.note_input();
    }

    /// A media-environment change other than size and scale (colour scheme, reduced motion,
    /// pointer capabilities): `ENVIRONMENT_MEDIA`. The environment epoch advances (HostRuntime
    /// is its single writer) and the next update re-evaluates media against a fresh stylist.
    pub fn note_environment_media(&mut self) -> Result<(), Status> {
        self.check_open()?;
        self.environment_epoch += 1;
        self.env_pending = true;
        self.note_input_reason(Reason::EnvironmentMedia, Subject::Document);
        Ok(())
    }

    /// Set the `prefers-reduced-motion` preference (`true` = reduce) of the document's media
    /// environment (006 P-006-1). A change is `ENVIRONMENT_MEDIA`; returns whether it changed.
    pub fn set_reduced_motion(&mut self, reduce: bool) -> Result<bool, Status> {
        self.check_open()?;
        if self.reduced_motion == reduce {
            return Ok(false);
        }
        self.reduced_motion = reduce;
        self.note_environment_media()?;
        Ok(true)
    }

    /// Set the `prefers-color-scheme` preference (`true` = dark) of the document's media
    /// environment (006): `prefers-color-scheme`, `light-dark()`, the `Canvas`/`CanvasText`
    /// system colours of a `normal` or `light dark` scheme, and the canvas default follow it. A
    /// change is one `ENVIRONMENT_MEDIA` step and a full restyle; returns whether it changed.
    pub fn set_dark(&mut self, dark: bool) -> Result<bool, Status> {
        self.check_open()?;
        if self.dark == dark {
            return Ok(false);
        }
        self.dark = dark;
        self.note_environment_media()?;
        Ok(true)
    }

    /// Whether `prefers-color-scheme: dark` is in force for this document.
    pub fn dark(&self) -> bool {
        self.dark
    }

    /// Whether `prefers-reduced-motion: reduce` is in force for this document.
    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    /// The font sources or `@font-face` load state changed (ResourceManager, one call per
    /// completion batch): `FONT_EPOCH`. Font-metric dependants are restyled and text reshaped.
    pub fn note_font_epoch_changed(&mut self, families: Vec<String>) -> Result<(), Status> {
        self.check_open()?;
        // A fresh font context over the same sources: loaded faces change the
        // text collection, which an in-place environment update keeps.
        self.env_pending = true;
        self.font_context_pending = true;
        self.note_input_reason(Reason::FontEpoch, Subject::FontFamilies(families));
        Ok(())
    }

    /// An image or SVG decode finished or failed (ResourceManager, one call per completion
    /// batch): `RESOURCE_EPOCH`; the resource epoch advances once for the batch.
    pub fn apply_resource_completions(&mut self, resource_ids: &[u64]) -> Result<(), Status> {
        self.check_open()?;
        if resource_ids.is_empty() {
            return Ok(());
        }
        self.resource_epoch += 1;
        for id in resource_ids {
            self.dirty
                .note(Reason::ResourceEpoch, Subject::Resource(*id));
        }
        self.note_input();
        Ok(())
    }

    /// A decoded image replacement with unchanged natural dimensions needs a
    /// new scene and raster, but must not invalidate the CSS or layout stages.
    pub fn apply_resource_pixels(&mut self, ids: &[u64]) -> Result<(), Status> {
        self.check_open()?;
        if ids.is_empty() {
            return Ok(());
        }
        self.resource_epoch += 1;
        for &id in ids {
            self.dirty.note(
                crate::pipeline::reasons::Reason::ResourcePixels,
                crate::pipeline::reasons::Subject::Resource(id),
            );
        }
        self.note_input();
        Ok(())
    }

    /// A ScrollState offset changed (InteractionRuntime): `SCROLL`; the scroll epoch advances.
    pub fn note_scroll(
        &mut self,
        container: ui4me_engine_types::ids::NodeId,
    ) -> Result<(), Status> {
        self.check_open()?;
        self.scroll_epoch += 1;
        self.note_input_reason(Reason::Scroll, Subject::Node(container));
        Ok(())
    }

    /// Caret blink or selection highlight geometry changed: `CARET_SELECTION_PAINT` (scene only).
    pub fn note_caret_selection_paint(
        &mut self,
        node: ui4me_engine_types::ids::NodeId,
    ) -> Result<(), Status> {
        self.check_open()?;
        self.note_input_reason(Reason::CaretSelectionPaint, Subject::Node(node));
        Ok(())
    }

    /// The caret blink phase (012's timer): a change repaints the scene only (`CARET_SELECTION_PAINT`).
    pub fn set_caret_state(
        &mut self,
        visible: bool,
        upstream: bool,
        node: ui4me_engine_types::ids::NodeId,
    ) -> Result<(), Status> {
        self.check_open()?;
        if self.caret_visible != visible || self.caret_upstream != upstream {
            self.caret_visible = visible;
            self.caret_upstream = upstream;
            self.note_caret_selection_paint(node)?;
        }
        Ok(())
    }

    pub fn caret_visible(&self) -> bool {
        self.caret_visible
    }

    /// The coalesced inputs since the last publication.
    pub fn dirty(&self) -> &DirtySets {
        &self.dirty
    }
}

// ---- the `invalidation` probe input (T029) ---------------------------------------------------------

use crate::diagnostics::probes::invalidation::{
    InvalidationOutcome, InvalidationProbeInput, ReasonEntry, UpdateEntry,
};
use crate::pipeline::reasons::Subject as Subj;
use crate::pipeline::state::{UpdateMode, UpdateOutcome};
use ui4me_engine_types::stage::Stage as CoreStage;

impl DocumentPipeline {
    /// The last updates for the `invalidation` probe, oldest first.
    pub fn invalidation_probe_input(&self) -> InvalidationProbeInput {
        let updates = self
            .stores
            .updates
            .iter()
            .map(|u| {
                let reasons = u
                    .reasons
                    .iter()
                    .map(|r| ReasonEntry {
                        code: r.reason.code(),
                        subject: match &r.subject {
                            Subj::Node(n) => Some(*n),
                            Subj::Attribute { node, .. } => Some(*node),
                            Subj::Nodes(v) => v.first().copied(),
                            _ => None,
                        },
                        count: r.count,
                    })
                    .collect();
                let mut stages_run: Vec<char> = Vec::new();
                for s in &u.stages_run {
                    let c = match s {
                        CoreStage::Cascade => 'S',
                        CoreStage::Box => 'B',
                        CoreStage::Text | CoreStage::Layout => 'L',
                        CoreStage::Scene => 'P',
                        _ => continue,
                    };
                    if !stages_run.contains(&c) {
                        stages_run.push(c);
                    }
                }
                UpdateEntry {
                    update_seq: u.update_seq,
                    trigger: u.trigger.name(),
                    mode: match u.mode {
                        UpdateMode::Incremental => "incremental",
                        UpdateMode::Cold => "cold",
                    },
                    reasons,
                    stages_run,
                    widened: u.widened,
                    loop_relational: u.loop_iterations.0,
                    loop_container: u.loop_iterations.1,
                    outcome: match u.outcome {
                        UpdateOutcome::Ok => InvalidationOutcome::Published,
                        UpdateOutcome::Stale => InvalidationOutcome::StaleDiscarded,
                        UpdateOutcome::NonConvergence => InvalidationOutcome::NonConvergence,
                        UpdateOutcome::Fail(..) => InvalidationOutcome::Failed,
                    },
                    error: match (u.outcome.clone(), u.diagnostic_id) {
                        (UpdateOutcome::Fail(code, stage), Some(id)) => Some((code, stage, id)),
                        _ => None,
                    },
                }
            })
            .collect();
        InvalidationProbeInput { updates }
    }
}
