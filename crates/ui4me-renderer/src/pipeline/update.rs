//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Update algorithm (005 recompute-plan §4; v1: incremental style, conservative full rebuild of
//! box tree, layout and scene) and the `INPUT` side: accepted DOM mutations note an input
//! (`RevisionInputs::dom` +1 per accepted call) and make the state `Dirty`. DOM reads see
//! mutations immediately; derived reads go through read.rs.
//! Implemented by 001 T049.

use crate::layout::box_tree::StyleBridgeSource;
use crate::layout::coordinator::overflow::ScrollOffsets;
use crate::layout::coordinator::{LayoutOutput, Viewport};
use crate::layout::fragment::FragmentTree;
use crate::pipeline::lifecycle::fresh_stages;
use crate::pipeline::publish::CommittedSet;
use crate::pipeline::state::{
    DocumentPipeline, Effect, Event, PipelineState, Trigger, UpdateMode, UpdateOutcome,
    UpdateRecord,
};
use crate::scene::appendix_e::{
    PaintStyle, PaintStyleSource, SceneFrame, SceneSnapshot, prepare_scene_scrolled,
};
use crate::scene::borders::ComputedBorderStyle;
use crate::scene::display_list::Color;
use crate::scene::inputs::{BoxExtras, PaintOptions};
use crate::style::traverse::StyleBridge;
use crate::style::view::StyleSnapshot;
use crate::style::view::groups::{BorderStyle, LengthPercentage, Visibility};
use crate::text::fonts::TextServices;
use std::sync::Arc;
use ui4me_document::document::Document;
use ui4me_document::journal::EpochSegment;
use ui4me_document::node::NodeKind;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{
    FeatureId, ReportKey, ReportLocation, UnsupportedReason, UnsupportedReports,
};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::revision::{RevisionInputs, RevisionTuple};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

impl DocumentPipeline {
    /// `getElementById`.
    pub fn element_by_id(&self, id: &str) -> Option<NodeId> {
        self.doc.element_by_id(id)
    }

    pub fn text_content(&self, node: NodeId) -> Result<String, Status> {
        self.check_open()?;
        Ok(self.doc.text_content(node)?.unwrap_or_default())
    }

    pub fn has_class(&self, node: NodeId, class: &str) -> Result<bool, Status> {
        self.check_open()?;
        self.doc.has_class(node, class)
    }

    pub fn set_text_content(&mut self, node: NodeId, text: &str) -> Result<(), Status> {
        self.mutate(|d| d.set_text_content(node, text))
    }

    pub fn set_attribute(&mut self, node: NodeId, name: &str, value: &str) -> Result<(), Status> {
        self.mutate(|d| d.set_attribute(node, name, value))
    }

    /// `classList.toggle`; returns whether the class is now present.
    pub fn toggle_class(&mut self, node: NodeId, class: &str) -> Result<bool, Status> {
        self.mutate(|d| d.toggle_class(node, class))
    }

    /// Create a detached HTML element.
    pub fn create_element(&mut self, local_name: &str) -> Result<NodeId, Status> {
        self.mutate(|d| d.create_element(local_name))
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), Status> {
        self.mutate(|d| d.append_child(parent, child)).map(|_| ())
    }

    /// Any DocumentCore operation: `DOCUMENT_CLOSED` once `Closing` (R-34); an accepted
    /// mutation (the DOM revision advanced) is an `INPUT` (recompute-plan §2).
    pub fn mutate<R>(
        &mut self,
        op: impl FnOnce(&mut Document) -> Result<R, Status>,
    ) -> Result<R, Status> {
        self.check_open()?;
        let before = self.doc.revision();
        let out = op(&mut self.doc);
        if self.doc.revision() != before {
            if self.scroll.tracked_nodes().next().is_some() {
                let removed: std::collections::HashSet<NodeId> = self
                    .doc
                    .journal()
                    .current_epoch_since(before)
                    .flat_map(|e| match &e.kind {
                        ui4me_document::journal::EntryKind::ChildList { removed, .. } => {
                            removed.as_slice()
                        }
                        _ => &[],
                    })
                    .copied()
                    .collect();
                // Descendants remain under a removed root unless moved again;
                // every subsequent move is itself journaled as another removal.
                // Walking the removed subtrees avoids scanning unrelated scrollers.
                let mut freed_root = false;
                for root in removed {
                    if self.doc.arena().contains(root) {
                        for node in self.doc.arena().subtree(root) {
                            self.scroll.forget_node(node);
                        }
                    } else {
                        freed_root = true;
                    }
                }
                // Collection can free an entire removed subtree before this
                // mutation returns; reclaim its now-stale tracked IDs as well.
                if freed_root {
                    let stale: Vec<_> = self
                        .scroll
                        .tracked_nodes()
                        .filter(|&n| !self.doc.is_connected(n).unwrap_or(false))
                        .collect();
                    for node in stale {
                        self.scroll.forget_node(node);
                    }
                }
            }
            self.note_journal_since(before);
            self.note_input();
            self.earliest_mutation
                .get_or_insert_with(std::time::Instant::now);
        }
        out
    }

    /// Record the reasons of the journal entries a mutation call committed after DOM revision
    /// `since` (invalidation-reasons §1, §3): the reason follows the entry kind.
    fn note_journal_since(&mut self, since: u64) {
        use crate::pipeline::reasons::{Reason, Subject};
        use ui4me_document::journal::EntryKind;
        let window = self.doc.journal().since(since);
        for e in window.entries {
            match e.kind {
                EntryKind::Attribute { node, name } => match &*name.local {
                    "class" => self.dirty.note(Reason::DomAttrClass, Subject::Node(node)),
                    "id" => self.dirty.note(Reason::DomAttrId, Subject::Node(node)),
                    "style" => self.dirty.note(Reason::DomAttrStyle, Subject::Node(node)),
                    other => self.dirty.note(
                        Reason::DomAttrOther,
                        Subject::Attribute {
                            node,
                            name: other.to_string(),
                        },
                    ),
                },
                EntryKind::ChildList {
                    parent,
                    added,
                    removed,
                    prev_sibling,
                    next_sibling,
                } => {
                    let mut nodes = vec![parent];
                    nodes.extend(added);
                    nodes.extend(removed);
                    nodes.extend(prev_sibling);
                    nodes.extend(next_sibling);
                    self.dirty.note(Reason::DomChildList, Subject::Nodes(nodes));
                }
                EntryKind::CharacterData { node } => {
                    self.dirty.note(Reason::DomText, Subject::Node(node))
                }
                // A selection change moves the caret and the highlight only (012 `editing.md`).
                EntryKind::ControlState {
                    node,
                    field: ui4me_document::control_state::ControlField::Selection,
                } => self
                    .dirty
                    .note(Reason::CaretSelectionPaint, Subject::Node(node)),
                EntryKind::ControlState { node, .. } => {
                    self.dirty.note(Reason::ControlState, Subject::Node(node))
                }
                EntryKind::UiState { node, .. } => {
                    self.dirty.note(Reason::ElementState, Subject::Node(node))
                }
                EntryKind::Stylesheets {
                    stylesheet_revision,
                } => self.dirty.note(
                    Reason::Stylesheet,
                    Subject::Sheet(stylesheet_revision as u32),
                ),
                EntryKind::PriorAttributes { .. } | EntryKind::PriorControlFlags { .. } => {}
            }
        }
    }

    /// `INPUT(reason)`: `Clean → Dirty`; `FailedUpdate` stays and sets `retry_allowed`.
    pub fn note_input(&mut self) {
        // No host code runs during an update (recompute-plan §2): an `INPUT` while `Updating`
        // is impossible and ignored here.
        let _ = self.machine.on(Event::Input);
    }

    /// Number of updates run (`Updating` entries), for coalescing checks; the `invalidation`
    /// probe records each one.
    pub fn updates_run(&self) -> u64 {
        self.updates_run
    }

    /// Updates that ended in `FAIL` (`document` probe `failure_count`).
    pub fn failure_count(&self) -> u64 {
        self.machine.failure_count()
    }

    /// Test hook (until 005's `fault-injection` feature exists): the next update fails at
    /// `stage` with `code`, once.
    #[doc(hidden)]
    pub fn fail_next_update_for_testing(&mut self, stage: Stage, code: Code) {
        self.fault = Some((stage, code));
    }

    /// Test hook: the container-facts source of the feedback loop (inert by default).
    #[doc(hidden)]
    pub fn set_container_facts_for_testing(
        &mut self,
        facts: Box<dyn crate::pipeline::feedback::ContainerFacts>,
    ) {
        self.facts = Some(facts);
    }

    /// `TRIGGER(kind)` (recompute-plan §2): runs at most one update. `Ok` = the committed set
    /// is current (or was already); `Err` = `FAIL` (O4), with the error recorded.
    pub fn trigger(&mut self, kind: Trigger) -> Result<(), Status> {
        self.check_open()?;
        if self.state() == PipelineState::Clean
            && kind == Trigger::OraclePrep
            && self.committed.is_none()
        {
            // Nothing published yet: the oracle needs a committed set to compare.
            self.machine.on(Event::Input).ok();
        }
        match self.machine.on(Event::Trigger(kind)) {
            Ok(Effect::StartUpdate { cold }) => self.run_update(kind, cold),
            // `Clean` is a no-op; a frame never busy-retries a failing document (FR-038).
            Ok(_) => Ok(()),
            Err(_) => Err(Status::new(Code::InternalInvariant, Stage::Host)
                .with_message("nested pipeline update")),
        }
    }

    /// The update algorithm (recompute-plan §4).
    fn run_update(&mut self, trigger: Trigger, cold: bool) -> Result<(), Status> {
        // 1. Capture inputs.
        let r0 = self.current_inputs();
        let mode = if cold {
            UpdateMode::Cold
        } else {
            UpdateMode::Incremental
        };
        let dom_from = self.committed.as_ref().map_or(0, |c| c.revision.inputs.dom);
        self.updates_run += 1;
        let update_seq = self.updates_run;

        // 2. S0: the journal segment since the last style epoch (005 is its single consumer).
        let segment = self.doc.take_epoch();
        if cold && trigger == Trigger::OraclePrep
            || mode == UpdateMode::Cold && self.dirty.is_empty()
        {
            self.dirty.note(
                crate::pipeline::reasons::Reason::ColdRebuild,
                crate::pipeline::reasons::Subject::Document,
            );
        }
        let reasons = self.dirty.take();
        let widened = reasons.is_empty() || reasons.iter().any(|r| r.reason.is_widened_in_v1());
        let mut stages_run = Vec::new();
        // A scroll alone dirties only the scene stage (invalidation-reasons: `SCROLL` = P): S5
        // re-runs over the committed style and fragments (no restyle, no relayout, FR-066).
        let paint_only = !cold
            && !self.env_pending
            && self.committed.is_some()
            && !reasons.is_empty()
            && reasons.iter().all(|r| {
                matches!(
                    r.reason,
                    crate::pipeline::reasons::Reason::Scroll
                        | crate::pipeline::reasons::Reason::CaretSelectionPaint
                )
            });
        // An animation tick (006 re-samples the animated values) may be paint-only too: the style
        // pass runs, and when every restyled element is damaged at most by a repaint or a stacking
        // context change the box tree and the layout are reused and only S5 re-runs.
        let anim_paint = !paint_only
            && !cold
            && !self.env_pending
            && !self.no_animation_paint_path
            && self.committed.is_some()
            && reasons
                .iter()
                .any(|r| r.reason == crate::pipeline::reasons::Reason::AnimationTick)
            && reasons.iter().all(|r| {
                matches!(
                    r.reason,
                    crate::pipeline::reasons::Reason::AnimationTick
                        | crate::pipeline::reasons::Reason::Scroll
                        | crate::pipeline::reasons::Reason::CaretSelectionPaint
                )
            });
        let draft = if paint_only {
            self.compute_paint_draft(r0, &mut stages_run)
        } else {
            self.compute_draft(mode, &segment, r0, &mut stages_run, anim_paint)
        };

        // P can fail too (injection point `publish`).
        #[cfg(feature = "fault-injection")]
        let draft = match draft {
            Ok(d) => self
                .faults
                .hit(crate::pipeline::fault::FaultPoint::Publish)
                .map(|()| d),
            e => e,
        };

        // 8. Stale check (defensive: no host code ran).
        let outcome = match draft {
            Ok(_) if self.current_inputs() != r0 => UpdateOutcome::Stale,
            Ok(ref d) if d.loop_end.non_convergence() => UpdateOutcome::NonConvergence,
            Ok(_) => UpdateOutcome::Ok,
            Err(ref s) => UpdateOutcome::Fail(s.code, s.stage),
        };
        let container_iterations = draft.as_ref().map_or(0, |d| d.loop_end.iterations());
        // 10. A failure is recorded (stage, diagnostic id) before the update record refers to it.
        let failure = match &draft {
            Err(status) if outcome != UpdateOutcome::Stale => {
                let revision = self.draft_revision(r0);
                Some(
                    self.stores
                        .record_error(self.doc.id(), status.clone(), revision),
                )
            }
            _ => None,
        };
        self.stores.updates.push(UpdateRecord {
            update_seq,
            trigger,
            mode,
            dom_from,
            dom_to: r0.dom,
            stages_run,
            widened,
            outcome: outcome.clone(),
            reasons,
            diagnostic_id: failure.as_ref().map(|s| s.diagnostic_id),
            loop_iterations: (0, container_iterations),
        });
        match (outcome, draft) {
            (UpdateOutcome::Ok | UpdateOutcome::NonConvergence, Ok(draft)) => {
                // `@font-face` faces the text needed in this update start loading (non-blocking).
                self.request_text_faces();
                self.publish(r0, draft);
                self.finish_font_rebuild();
                Ok(())
            }
            (UpdateOutcome::Stale, _) => {
                let _ = self.machine.on(Event::Stale);
                Ok(())
            }
            (_, Err(_)) => {
                // 10. Discard the drafts; FAIL(code, stage).
                let _ = self.machine.on(Event::Fail);
                Err(failure.expect("a failed update recorded its error"))
            }
            (UpdateOutcome::Fail(..), Ok(_)) => unreachable!("outcome follows the draft"),
        }
    }

    /// The revision a draft computed from `inputs` would publish.
    fn draft_revision(&self, inputs: RevisionInputs) -> RevisionTuple {
        RevisionTuple {
            document: self.doc.id(),
            inputs,
            style: self.epochs.style,
            layout: self.epochs.layout,
            scene: self.epochs.scene,
        }
    }

    fn injected(&mut self, stage: Stage) -> Result<(), Status> {
        #[cfg(feature = "fault-injection")]
        if let Some(point) = crate::pipeline::fault::FaultPoint::from_stage(stage) {
            self.faults.hit(point)?;
        }
        match self.fault {
            Some((s, code)) if s == stage => {
                self.fault = None;
                Err(Status::new(code, stage)
                    .with_message(format!("injected {} failure (test hook)", stage.name())))
            }
            _ => Ok(()),
        }
    }

    /// Steps 3–7: S1 style, S2–S4 box tree, layout and fragments, S5 scene.
    fn compute_draft(
        &mut self,
        mode: UpdateMode,
        segment: &EpochSegment,
        r0: RevisionInputs,
        stages_run: &mut Vec<Stage>,
        anim_paint: bool,
    ) -> Result<Draft, Status> {
        let rebuild = mode == UpdateMode::Cold || self.env_pending;
        if rebuild {
            // Cold (or a changed environment): a fresh stylist and font context over the same sources; nothing reused.
            let (style, text) = fresh_stages(
                self.doc.id(),
                &self.config,
                &self.fonts,
                self.reduced_motion,
                self.dark,
            )?;
            // The running transitions and animations (and their queued events) survive the
            // rebuild: they are keyed by element, not by style data (006 T087).
            let mut old = std::mem::replace(&mut self.style, style);
            self.style.adopt_animations_from(&mut old);
            self.text = text;
        }
        // Document animation time for this update: constant within it (FR-065).
        self.clock.ensure_sampled();
        self.style.set_animation_time_ms(self.clock.time_ms());
        // The loader and base URL of linked sheets and `@import` (007), once per update: a new
        // base URL or loader re-reads the sheets in the style pass below.
        crate::pipeline::resources::configure_style(
            &mut self.style,
            self.resources.as_ref(),
            &self.doc,
            self.config.scale,
            (
                self.config.viewport_width as f32,
                self.config.viewport_height as f32,
            ),
        );
        let dom_revision = r0.dom;
        let cold_segment = EpochSegment::empty();
        #[cfg(feature = "fault-injection")]
        let omitted = self.faults.filter_segment(segment);
        #[cfg(feature = "fault-injection")]
        let segment = omitted.as_ref().unwrap_or(segment);
        let seg = if rebuild { &cold_segment } else { segment };

        let mut ms = ui4me_engine_types::diag::StageMs::default();
        let t_style = std::time::Instant::now();
        // S1
        self.injected(Stage::Cascade)?;
        stages_run.push(Stage::Cascade);
        let report = self.style.restyle(&self.doc, seg)?;
        let t_a = t_style.elapsed();
        self.sync_font_faces(dom_revision);
        let mut style = self.style.snapshot();
        let t_b = t_style.elapsed();
        record_all(
            &mut self.stores.unsupported,
            self.style.unsupported_reports(),
            dom_revision,
        );
        // Transitions and animations are consumed (006 T087): the pass starts and cancels them,
        // and the clock ticks while 006 reports a pending or running one (AD-11). Only the
        // properties of scroll-driven and view timelines are still not consumed (FR-022).
        report_unconsumed_animation_properties(
            &self.doc,
            &style,
            &mut self.stores.unsupported,
            dom_revision,
        );
        self.clock.set_active(self.style.animations_need_ticks());
        let mut style_changed = report.restyled_elements > 0
            || report.sheets_changed
            || self.committed.is_none()
            || rebuild;

        ms.style = Some(t_style.elapsed().as_secs_f64() * 1000.0);
        if std::env::var_os("UI4ME_PROFILE").is_some() {
            eprintln!(
                "[prof] restyle {:.2} snapshot {:.2} rest {:.2}",
                t_a.as_secs_f64() * 1e3,
                (t_b - t_a).as_secs_f64() * 1e3,
                (t_style.elapsed() - t_b).as_secs_f64() * 1e3
            );
        }
        // The animation paint-only path: every damage bit this pass recorded is a repaint or a
        // stacking-context change (Stylo `ServoRestyleDamage` <= 0b0011) and the sheets did not
        // change. Such a tick used to skip the box tree too, but a repaint can still move a box input
        // (a background going from transparent to a colour flips `has_background`; L-PERF-b2 found it
        // with the cold oracle): the box tree is rebuilt and compared, and the committed layout is
        // reused when it is equal (exact by construction, below). Only the scene re-runs then.
        let anim_paint = anim_paint
            && !rebuild
            && !report.sheets_changed
            && !report.color_changed
            && report.damage_bits & !0b0011 == 0;
        let t_layout = std::time::Instant::now();
        // S2–S4 (conservative full rebuild)
        self.injected(Stage::Box)?;
        stages_run.extend([Stage::Box, Stage::Text, Stage::Layout]);
        // The previous layout is reusable when the new box tree equals its box tree (a style change
        // that moved no layout input: colours, shadows, backgrounds, hover states) and the font,
        // resource and environment epochs did not move.
        let reuse_base = if rebuild || self.no_layout_reuse || self.layout_font_gen != self.font_gen
        {
            None
        } else {
            self.committed.clone().filter(|b| {
                let i = &b.revision.inputs;
                i.environment == r0.environment && i.font == r0.font && i.resource == r0.resource
            })
        };
        // The box tree need not even be rebuilt when nothing box generation reads moved (L-PERF-b3):
        // since the committed pass the DOM saw only hover / focus / active state changes (the journal
        // accounts for every DOM revision step in between) and every element's mapped `BoxStyle` is
        // the same. Box generation reads the DOM (tree, attributes, text, control state, the
        // `PICKER_OPEN` flag of a `select`) and those styles only, so it would build an equal tree.
        let boxes_unchanged = reuse_base.as_ref().is_some_and(|b| {
            !report.sheets_changed && box_inputs_untouched(seg, b.revision.inputs.dom, r0.dom)
        }) && self.box_styles.unchanged_mapping(&self.doc, &style);
        let outcome = if boxes_unchanged {
            if std::env::var_os("UI4ME_PROFILE").is_some() {
                eprintln!("[prof] unchanged: box inputs untouched");
            }
            crate::layout::coordinator::PassOutcome::Unchanged
        } else {
            layout_stage(
                &self.doc,
                &style,
                &self.config,
                &mut self.text,
                self.resources.as_ref(),
                reuse_base.as_ref().map(|b| &*b.layout),
                &mut self.box_styles,
                !self.no_layout_reuse,
            )?
        };
        self.injected(Stage::Text)?;
        self.injected(Stage::Layout)?;
        // The layout of this update: a fresh one, or the committed one reused (same boxes, same fragments).
        let (mut layout, mut fresh) = match (outcome, reuse_base.as_ref()) {
            (crate::layout::coordinator::PassOutcome::Fresh(layout), _) => (Arc::new(layout), true),
            (crate::layout::coordinator::PassOutcome::Unchanged, Some(base)) => {
                (base.layout.clone(), false)
            }
            (crate::layout::coordinator::PassOutcome::Unchanged, None) => {
                unreachable!("reuse needs a base")
            }
        };

        // 6. The container-query loop (015 CQ-3, plan A1 item 4; AD-10), before the scene: the sizes of
        // the query containers this layout produced go to the style stage; when one changed since the
        // style pass that produced this layout, the subtrees of the changed containers are restyled and
        // box tree and layout re-run, at most `BOUND` times. The host never sees an unresolved query:
        // the first layout of a container (no size yet, its queries false) is followed by the second
        // round in the same update. A test source (`set_container_facts_for_testing`) replaces it.
        let loop_end = match self.facts.as_mut() {
            Some(f) => crate::pipeline::feedback::run(&mut **f, crate::pipeline::feedback::BOUND),
            None => {
                let (end, restyled) =
                    self.container_loop(&mut style, &mut layout, &mut fresh, dom_revision)?;
                style_changed |= restyled;
                end
            }
        };

        let mut diagnostics = Vec::new();
        let revision = RevisionTuple {
            document: self.doc.id(),
            inputs: r0,
            style: self.epochs.style + u64::from(style_changed),
            layout: self.epochs.layout + u64::from(fresh),
            scene: self.epochs.scene + 1,
        };
        let layout_changed = fresh;
        if fresh {
            let l = Arc::get_mut(&mut layout).expect("a fresh layout is owned by this update");
            for r in &l.reports {
                self.stores
                    .unsupported
                    .record(r.key, r.code, r.reason, dom_revision);
            }
            l.fragments.stamp(revision.layout, Some(revision));
            self.layout_font_gen = self.font_gen;
            for d in &l.diagnostics {
                // Stage diagnostics (RESOURCE_PENDING missing glyph, GEOMETRY_NON_FINITE) are O3.
                let s = self
                    .stores
                    .record_error(self.doc.id(), d.status.clone(), revision);
                diagnostics.push(s);
            }
        } else if let Some(base) = reuse_base.as_ref() {
            // Same boxes, same fragments: the layout epoch stays; the reports are seen again.
            for r in &base.layout.reports {
                self.stores
                    .unsupported
                    .record(r.key, r.code, r.reason, dom_revision);
            }
            diagnostics = base.diagnostics.clone();
            self.layout_reuses += 1;
            if anim_paint {
                self.animation_paint_updates += 1;
            }
        }
        if loop_end.non_convergence() {
            let s = Status::new(Code::LayoutNonConvergence, Stage::Layout).with_message(format!(
                "style/layout feedback did not converge after {} iterations",
                loop_end.iterations()
            ));
            let s = self.stores.record_error(self.doc.id(), s, revision);
            diagnostics.push(s);
        }

        ms.layout = Some(t_layout.elapsed().as_secs_f64() * 1000.0);
        let t_scene = std::time::Instant::now();
        // S5
        self.injected(Stage::Scene)?;
        stages_run.push(Stage::Scene);
        let generation = self.generation + 1;
        // Offsets are re-clamped against the new scrollable overflow (008 `reclamp`).
        let mut scroll = self.scroll.clone();
        scroll.reclamp_connected(&layout.fragments, |n| {
            self.doc.is_connected(n).unwrap_or(false)
        });
        // The no-reuse reference (tests) projects every paint style afresh.
        let fresh = Default::default();
        let paint_styles = if self.no_layout_reuse {
            &fresh
        } else {
            &self.paint_styles
        };
        let reuse = (self.scene_reuse && !self.no_layout_reuse).then_some(&layout);
        let scene = scene_stage(
            &self.doc,
            &style,
            &layout.fragments,
            reuse,
            generation,
            revision,
            self.images.as_ref(),
            &scroll,
            (self.caret_visible, self.caret_upstream),
            paint_styles,
        );
        for r in &scene.reports {
            self.stores
                .unsupported
                .record(r.key, r.code, r.reason, dom_revision);
        }
        ms.scene = Some(t_scene.elapsed().as_secs_f64() * 1000.0);
        self.last_stage_ms = ms;
        if std::env::var_os("UI4ME_PROFILE").is_some() {
            eprintln!(
                "[prof] style {:.2} layout {:.2} scene {:.2}",
                ms.style.unwrap_or(0.0),
                ms.layout.unwrap_or(0.0),
                ms.scene.unwrap_or(0.0)
            );
        }
        Ok(Draft {
            loop_end,
            revision,
            style,
            layout,
            scene: Arc::new(scene),
            diagnostics,
            style_changed,
            layout_changed,
            scroll,
        })
    }

    /// The container-query loop of one update (see `compute_draft` step 6 and `feedback.rs`): returns
    /// how it ended and whether it restyled anything. `layout` and `fresh` are replaced by each round's
    /// layout; a round's layout pass may reuse the previous round's (equal box tree) but never the
    /// committed one, whose styles are older.
    fn container_loop(
        &mut self,
        style: &mut Arc<StyleSnapshot>,
        layout: &mut Arc<LayoutOutput>,
        fresh: &mut bool,
        dom_revision: u64,
    ) -> Result<(crate::pipeline::feedback::LoopEnd, bool), Status> {
        use crate::pipeline::feedback::LoopEnd;
        let mut restyled = false;
        if !self.style.has_query_containers() {
            return Ok((LoopEnd::Converged { iterations: 0 }, false));
        }
        let mut iterations = 0;
        loop {
            let measured =
                layout
                    .container_facts
                    .iter()
                    .map(|f| crate::style::container::MeasuredContainer {
                        node: f.node,
                        width: f.width,
                        height: f.height,
                    });
            let update = self.style.update_container_sizes(&self.doc, measured);
            if std::env::var_os("UI4ME_CQ_DEBUG").is_some() {
                eprintln!(
                    "[cq] round {iterations}: facts {:?} changed {:?}",
                    layout.container_facts, update.changed
                );
            }
            if update.is_empty() {
                return Ok((LoopEnd::Converged { iterations }, restyled));
            }
            if iterations == crate::pipeline::feedback::BOUND {
                // The sizes still move: publish this round (its queries used the previous sizes).
                return Ok((LoopEnd::Bound { iterations }, restyled));
            }
            iterations += 1;
            let report = self.style.restyle(&self.doc, &EpochSegment::empty())?;
            record_all(
                &mut self.stores.unsupported,
                self.style.unsupported_reports(),
                dom_revision,
            );
            restyled |= report.restyled_elements > 0;
            *style = self.style.snapshot();
            match layout_stage(
                &self.doc,
                style,
                &self.config,
                &mut self.text,
                self.resources.as_ref(),
                Some(&**layout),
                &mut self.box_styles,
                !self.no_layout_reuse,
            )? {
                crate::layout::coordinator::PassOutcome::Fresh(l) => {
                    *layout = Arc::new(l);
                    *fresh = true;
                }
                // The restyle moved no box input: this round's layout is the previous round's.
                crate::layout::coordinator::PassOutcome::Unchanged => {}
            }
        }
    }

    /// The paint-only update of a scroll: S5 over the committed style and fragments with the
    /// current offsets. The layout epoch does not advance and the fragment tree is the same
    /// `Arc` (008 FR-066, tested in `tests/scroll_paint_only.rs`).
    fn compute_paint_draft(
        &mut self,
        r0: RevisionInputs,
        stages_run: &mut Vec<Stage>,
    ) -> Result<Draft, Status> {
        let Some(base) = self.committed.clone() else {
            return Err(Status::new(Code::InternalInvariant, Stage::Scene)
                .with_message("paint-only update without a committed set"));
        };
        let t_scene = std::time::Instant::now();
        self.injected(Stage::Scene)?;
        stages_run.push(Stage::Scene);
        let revision = RevisionTuple {
            document: self.doc.id(),
            inputs: r0,
            style: self.epochs.style,
            layout: self.epochs.layout,
            scene: self.epochs.scene + 1,
        };
        let generation = self.generation + 1;
        let fresh = Default::default();
        let paint_styles = if self.no_layout_reuse {
            &fresh
        } else {
            &self.paint_styles
        };
        let reuse = (self.scene_reuse && !self.no_layout_reuse).then_some(&base.layout);
        let scene = scene_stage(
            &self.doc,
            &base.style,
            &base.layout.fragments,
            reuse,
            generation,
            revision,
            self.images.as_ref(),
            &self.scroll,
            (self.caret_visible, self.caret_upstream),
            paint_styles,
        );
        for r in &scene.reports {
            self.stores
                .unsupported
                .record(r.key, r.code, r.reason, r0.dom);
        }
        self.last_stage_ms = ui4me_engine_types::diag::StageMs {
            scene: Some(t_scene.elapsed().as_secs_f64() * 1000.0),
            ..Default::default()
        };
        // No style or layout ran: nothing for the container loop to compare.
        let loop_end = match self.facts.as_mut() {
            Some(f) => crate::pipeline::feedback::run(&mut **f, crate::pipeline::feedback::BOUND),
            None => crate::pipeline::feedback::LoopEnd::Converged { iterations: 0 },
        };
        Ok(Draft {
            loop_end,
            revision,
            style: base.style.clone(),
            layout: base.layout.clone(),
            scene: Arc::new(scene),
            diagnostics: base.diagnostics.clone(),
            style_changed: false,
            layout_changed: false,
            scroll: self.scroll.clone(),
        })
    }

    /// Step 9: publish one committed set atomically.
    fn publish(&mut self, _r0: RevisionInputs, draft: Draft) {
        let draft_non_convergence = draft.loop_end.non_convergence();
        if draft.style_changed {
            self.epochs.style += 1;
        }
        if draft.layout_changed {
            self.epochs.layout += 1;
        }
        self.epochs.scene += 1;
        self.generation += 1;
        debug_assert_eq!(
            (
                draft.revision.style,
                draft.revision.layout,
                draft.revision.scene
            ),
            (self.epochs.style, self.epochs.layout, self.epochs.scene)
        );
        self.committed = Some(Arc::new(CommittedSet {
            revision: draft.revision,
            scene: draft.scene,
            style: draft.style,
            layout: draft.layout,
            diagnostics: draft.diagnostics,
        }));
        self.env_pending = false;
        let _ = self.machine.on(if draft_non_convergence {
            Event::Bound
        } else {
            Event::Ok
        });
        // A relayout moved the scrollable overflow: the offsets were re-clamped for this scene;
        // note it as a scroll input so the revision's scroll epoch follows (a paint-only update).
        if draft.scroll != self.scroll {
            self.scroll = draft.scroll;
            self.scroll_epoch += 1;
            self.note_input_reason(
                crate::pipeline::reasons::Reason::Scroll,
                crate::pipeline::reasons::Subject::Document,
            );
        }
    }
}

/// Drafts of one update (discarded on failure).
struct Draft {
    loop_end: crate::pipeline::feedback::LoopEnd,
    revision: RevisionTuple,
    style: Arc<StyleSnapshot>,
    layout: Arc<LayoutOutput>,
    scene: Arc<SceneSnapshot>,
    diagnostics: Vec<Status>,
    style_changed: bool,
    /// A new fragment tree was computed (false for a paint-only update).
    layout_changed: bool,
    /// The scroll offsets this draft's scene was drawn with.
    scroll: ScrollOffsets,
}

fn record_all(store: &mut UnsupportedReports, from: &UnsupportedReports, dom_revision: u64) {
    for r in from.iter() {
        // The producer's store already de-duplicates; re-recording with the current DOM
        // revision would inflate `revisions`, so only the producer's own last revision counts.
        store.record(r.key, r.code, r.reason, r.last_seen_dom.min(dom_revision));
    }
}

/// FR-022: `transition-*` and `animation-*` are consumed (006 T087; the clock ticks and the
/// style pass interpolates). Scroll-driven and view timelines are not: a non-initial
/// `animation-timeline` or `animation-range-*` is reported (stage `cascade`) and the animation
/// runs on the document timeline. Returns whether any element has one.
fn report_unconsumed_animation_properties(
    doc: &Document,
    style: &StyleSnapshot,
    store: &mut UnsupportedReports,
    dom_revision: u64,
) -> bool {
    let mut found = false;
    for node in style.nodes() {
        let Some(view) = style.view(node) else {
            continue;
        };
        // Only these longhands matter: asking for them by name costs three comparisons per element, where listing every
        // non-initial longhand serialised ~450 values twice per element on every update (A23: ~2,300 allocations per cycle).
        for prop in [
            "animation-timeline",
            "animation-range-start",
            "animation-range-end",
        ] {
            if view.is_initial(prop) {
                continue;
            }
            let Some(feature) = FeatureId::new(leak_feature(prop)) else {
                continue;
            };
            let key = ReportKey {
                feature,
                stage: Stage::Cascade,
                document: doc.id(),
                location: ReportLocation::Element { node },
            };
            store.record(
                key,
                Code::UnsupportedFeature,
                UnsupportedReason::NotConsumedByStage,
                dom_revision,
            );
            found = true;
        }
    }
    found
}

/// `css.property.<longhand>` as a `'static` id (the longhand set is finite, so each id is
/// interned once).
fn leak_feature(longhand: &'static str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static IDS: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();
    let mut m = IDS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    m.entry(longhand)
        .or_insert_with(|| Box::leak(format!("css.property.{longhand}").into_boxed_str()))
}

/// Whether the journal `seg` holds nothing box generation reads and accounts for every DOM revision
/// step from `committed` (the DOM revision the committed box tree stands for) to `now`: its entries
/// are UI-state changes (hover, active, focus, ...) that neither open or close a `select` picker nor
/// change the modal flag, and their revisions are exactly the steps in between (a step without an
/// entry, or an entry from before, makes the answer no).
fn box_inputs_untouched(seg: &EpochSegment, committed: u64, now: u64) -> bool {
    use ui4me_document::journal::EntryKind;
    use ui4me_document::ui_state::UiFlags;
    if std::env::var_os("UI4ME_PROFILE_DIFF").is_some() {
        let kinds: Vec<String> = seg
            .entries()
            .iter()
            .map(|e| format!("{}@{}", e.kind.name(), e.revision))
            .collect();
        eprintln!("[diff] journal committed {committed} now {now}: {kinds:?}");
    }
    if now < committed {
        return false;
    }
    let mut steps: Vec<u64> = Vec::new();
    for e in seg.entries() {
        match &e.kind {
            EntryKind::UiState { old, new, .. }
                if [UiFlags::PICKER_OPEN, UiFlags::MODAL, UiFlags::TOP_LAYER]
                    .iter()
                    .all(|f| old.contains(*f) == new.contains(*f)) => {}
            _ => return false,
        }
        if e.revision <= committed || e.revision > now {
            return false;
        }
        if steps.last() != Some(&e.revision) {
            steps.push(e.revision);
        }
    }
    steps.sort_unstable();
    steps.dedup();
    steps.len() as u64 == now - committed
}

/// The DOM changes since DOM revision `built` (the revision a box tree was built from) as the
/// incremental box build reads them (015 PERF-1): the nodes of child-list, text, attribute and control
/// changes and of `select` picker / modal flag changes (hover, focus and active changes reach box
/// generation through styles only). `None` (build in full) when the journal window no longer holds
/// every entry since then or a revision step has no entry.
pub(crate) fn box_damage(
    doc: &Document,
    built: u64,
) -> Option<crate::layout::box_tree::incremental::BoxDamage> {
    use ui4me_document::journal::EntryKind;
    use ui4me_document::ui_state::UiFlags;
    let now = doc.revision();
    if now < built {
        return None;
    }
    if now == built {
        return Some(Default::default());
    }
    let w = doc.journal().since(built);
    if w.truncated {
        return None;
    }
    let mut nodes = Vec::new();
    let mut steps: Vec<u64> = Vec::with_capacity(w.entries.len());
    for e in &w.entries {
        if e.revision > now {
            return None;
        }
        steps.push(e.revision);
        match &e.kind {
            EntryKind::ChildList {
                parent,
                added,
                removed,
                ..
            } => {
                nodes.push(*parent);
                nodes.extend(added);
                nodes.extend(removed);
            }
            EntryKind::Attribute { node, .. }
            | EntryKind::PriorAttributes { node, .. }
            | EntryKind::CharacterData { node }
            | EntryKind::ControlState { node, .. }
            | EntryKind::PriorControlFlags { node, .. } => nodes.push(*node),
            EntryKind::UiState { node, old, new } => {
                if [UiFlags::PICKER_OPEN, UiFlags::MODAL, UiFlags::TOP_LAYER]
                    .iter()
                    .any(|f| old.contains(*f) != new.contains(*f))
                {
                    nodes.push(*node);
                }
            }
            EntryKind::Stylesheets { .. } => {}
        }
    }
    steps.sort_unstable();
    steps.dedup();
    (steps.len() as u64 == now - built)
        .then_some(crate::layout::box_tree::incremental::BoxDamage { nodes })
}

/// The nodes the DOM journal names since DOM revision `since`, with all their ancestors (015 PERF-2: an
/// element whose subtree saw a DOM change is not reused by the scene). `None` when the journal window no
/// longer holds every entry since then, a revision step has no entry or the style sheets changed.
fn dom_dirty(doc: &Document, since: u64) -> Option<crate::scene::reuse::FastSet<NodeId>> {
    use ui4me_document::journal::EntryKind;
    let now = doc.revision();
    let mut out = crate::scene::reuse::FastSet::default();
    if now < since {
        return None;
    }
    if now == since {
        return Some(out);
    }
    let w = doc.journal().since(since);
    if w.truncated {
        return None;
    }
    let mut nodes = Vec::new();
    let mut steps: Vec<u64> = Vec::with_capacity(w.entries.len());
    for e in &w.entries {
        if e.revision > now {
            return None;
        }
        steps.push(e.revision);
        match &e.kind {
            EntryKind::ChildList {
                parent,
                added,
                removed,
                ..
            } => {
                nodes.push(*parent);
                nodes.extend(added);
                nodes.extend(removed);
            }
            EntryKind::Attribute { node, .. }
            | EntryKind::PriorAttributes { node, .. }
            | EntryKind::CharacterData { node }
            | EntryKind::ControlState { node, .. }
            | EntryKind::PriorControlFlags { node, .. }
            | EntryKind::UiState { node, .. } => nodes.push(*node),
            EntryKind::Stylesheets { .. } => return None,
        }
    }
    steps.sort_unstable();
    steps.dedup();
    if steps.len() as u64 != now - since {
        return None;
    }
    for mut n in nodes {
        while out.insert(n) {
            match doc.parent(n) {
                Ok(Some(p)) => n = p,
                _ => break,
            }
        }
    }
    Some(out)
}

/// S2–S4 for one document. With `incremental`, the box tree takes unchanged subtrees over from `prev`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn layout_stage(
    doc: &Document,
    style: &StyleSnapshot,
    config: &crate::pipeline::state::PipelineConfig,
    text: &mut TextServices,
    resources: Option<&Arc<crate::pipeline::resources::PipelineResources>>,
    prev: Option<&LayoutOutput>,
    cache: &mut crate::layout::box_tree::BoxStyleCache,
    incremental: bool,
) -> Result<crate::layout::coordinator::PassOutcome, Status> {
    let viewport = Viewport {
        width: config.viewport_width as f32,
        height: config.viewport_height as f32,
        scale: config.scale as f32,
    };
    // 007's natural-size view of `<img>` elements (FR-070; D-LRESr3-12). An image first requested by
    // the scene stage is pending here; its completion batch dirties layout, so the next pass sizes it.
    let natural = |n: NodeId| resources.and_then(|r| r.image_element_state(n));
    let css_natural =
        |n: NodeId, property: &str| resources.and_then(|r| r.css_image_state(n, property));
    let src = StyleBridgeSource::new_cached(doc, style, cache)
        .with_images(&natural)
        .with_css_images(&css_natural);
    // 008 T016 entry: validates the viewport and isolates internal failures (a failure keeps the previous epoch, O4).
    let damage = if incremental {
        prev.and_then(|p| p.boxes.build_revision())
            .and_then(|built| box_damage(doc, built))
    } else {
        None
    };
    crate::layout::coordinator::try_layout_document_reusing(
        &src,
        Some(viewport),
        text,
        prev,
        damage.as_ref(),
    )
}

/// Paint options of the scene stage: border radii are painted (010). The 001 slice scenario's S1-OOS
/// radius expectation was superseded once radii were implemented (D-080).
pub const SCENE_PAINT_OPTIONS: PaintOptions = PaintOptions::FULL;

/// S5 for one document.
#[allow(clippy::too_many_arguments)]
/// With `reuse` (the layout `fragments` belong to), the element-box subtrees whose inputs did not change
/// since the previous scene are taken over (015 PERF-2, `crate::scene::reuse`); the scene is the same.
pub(crate) fn scene_stage(
    doc: &Document,
    style: &StyleSnapshot,
    fragments: &FragmentTree,
    reuse: Option<&Arc<LayoutOutput>>,
    generation: u64,
    revision: RevisionTuple,
    images: Option<&Arc<dyn crate::scene::inputs::ImageSource>>,
    scroll: &ScrollOffsets,
    caret: (bool, bool),
    cache: &std::cell::RefCell<PaintStyleCache>,
) -> SceneSnapshot {
    cache.borrow_mut().begin();
    let mut memo = match reuse {
        Some(_) => {
            let mut c = cache.borrow_mut();
            c.begin_reuse(doc);
            Some(std::mem::take(&mut c.memo))
        }
        None => None,
    };
    let src = PaintStyles {
        doc,
        style,
        images: images.map(|i| i.as_ref()),
        caret,
        cache,
        scrollbars: true,
    };
    // The canvas default is the `Canvas` colour of the root element's used colour scheme (010 FR-023):
    // white for light, the fixed ui4me dark token for dark.
    let dark = doc
        .document_element()
        .and_then(|root| style.used_color_scheme_dark(root))
        .unwrap_or_else(|| style.prefers_dark());
    let default_canvas = if dark {
        crate::scene::theme::CANVAS_DARK
    } else {
        crate::scene::appendix_e::CANVAS_DEFAULT_LIGHT
    };
    let frame = SceneFrame {
        generation,
        revision,
        default_canvas,
    };
    let t0 = std::time::Instant::now();
    let prepared = match (reuse, memo.as_mut()) {
        (Some(layout), Some(memo)) => crate::scene::appendix_e::prepare_scene_reusing(
            layout,
            &src,
            &frame,
            SCENE_PAINT_OPTIONS,
            scroll,
            memo,
        ),
        _ => prepare_scene_scrolled(fragments, &src, &frame, SCENE_PAINT_OPTIONS, scroll),
    };
    let t1 = t0.elapsed();
    let scene = prepared.build();
    if std::env::var_os("UI4ME_PROFILE").is_some() {
        let st = memo.as_ref().map(|m| m.stats()).unwrap_or_default();
        eprintln!(
            "[prof]   scene prepare {:.2} build {:.2} items {} (subtrees reused {} built {})",
            t1.as_secs_f64() * 1e3,
            (t0.elapsed() - t1).as_secs_f64() * 1e3,
            scene.display_list.items.len(),
            st.reused,
            st.built
        );
    }
    if let Some(memo) = memo {
        cache.borrow_mut().memo = memo;
    }
    scene
}

/// 006's computed views projected for the scene adapter (010 `PaintStyleSource`).
pub(crate) struct PaintStyles<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) style: &'a StyleSnapshot,
    pub(crate) images: Option<&'a dyn crate::scene::inputs::ImageSource>,
    /// (blink phase visible, upstream affinity)
    pub(crate) caret: (bool, bool),
    pub(crate) cache: &'a std::cell::RefCell<PaintStyleCache>,
    /// Native scrollbar chrome is absent from paginated output.
    pub(crate) scrollbars: bool,
}

/// The paint styles of the previous scene, by element (and pseudo-element) with the computed values
/// they were projected from (L-PERF-b3). A `PaintStyle` is a pure function of the computed values, so
/// an element whose `StyleIdentity` is unchanged keeps its projection (a hover or a scroll repaints
/// thousands of elements whose values did not move). Only the entries the last scene used are kept.
#[derive(Default)]
pub(crate) struct PaintStyleCache {
    prev: crate::scene::reuse::FastMap<
        (NodeId, u8),
        (crate::style::view::StyleIdentity, Option<PaintStyle>),
    >,
    cur: crate::scene::reuse::FastMap<
        (NodeId, u8),
        (crate::style::view::StyleIdentity, Option<PaintStyle>),
    >,
    /// The paint extras of elements by computed values and border-box size (015 PERF-1); only extras without
    /// a transform (which reads the box's position) and that read no image (no image layer, mask, border image or replaced picture) are kept, since image states
    /// change without a style change. Same keep rule as the paint styles.
    extras_prev: crate::scene::reuse::FastMap<
        NodeId,
        (crate::style::view::StyleIdentity, [u32; 2], BoxExtras),
    >,
    extras_cur: crate::scene::reuse::FastMap<
        NodeId,
        (crate::style::view::StyleIdentity, [u32; 2], BoxExtras),
    >,
    /// Scene reuse (015 PERF-2): the subtrees of the last scene (`crate::scene::reuse`).
    memo: crate::scene::reuse::SceneMemo,
    /// What `PaintStyleSource::unchanged` compares with: per node read by the last scene, its computed
    /// values and whether it reads an image; and this scene's answers.
    nodes_prev: crate::scene::reuse::FastMap<NodeId, (NodeFacts, bool)>,
    nodes_cur: crate::scene::reuse::FastMap<NodeId, (NodeFacts, bool)>,
    /// The nodes the DOM journal names since the last scene, with their ancestors; `None` = unknown (all).
    dirty: Option<crate::scene::reuse::FastSet<NodeId>>,
    /// The DOM revision of the last scene.
    scene_dom: Option<u64>,
}

/// One node's inputs of `PaintStyleSource::unchanged`.
struct NodeFacts {
    identity: Option<crate::style::view::StyleIdentity>,
    /// The element or one of its pseudo-elements reads an image (whose state changes without a style
    /// change): never unchanged.
    images: bool,
}

impl PaintStyleCache {
    /// A scene begins: what the previous one used is the reuse base.
    pub(crate) fn memo_stats(&self) -> crate::scene::reuse::ReuseStats {
        self.memo.stats()
    }

    /// A scene with reuse begins: the DOM changes since the last one (015 PERF-2).
    fn begin_reuse(&mut self, doc: &Document) {
        let now = doc.revision();
        self.dirty = self.scene_dom.and_then(|since| dom_dirty(doc, since));
        self.scene_dom = Some(now);
        // The two tables swap and keep their storage (no table is allocated per scene: long-lived blocks allocated
        // anew every update kept glibc's heap growing, 015 PERF-2).
        std::mem::swap(&mut self.nodes_prev, &mut self.nodes_cur);
        self.nodes_cur.clear();
        if self.dirty.is_none() {
            self.memo.clear();
        }
    }

    fn begin(&mut self) {
        // Swapped and cleared, so the tables keep their storage from scene to scene (015 PERF-2).
        std::mem::swap(&mut self.prev, &mut self.cur);
        self.cur.clear();
        std::mem::swap(&mut self.extras_prev, &mut self.extras_cur);
        self.extras_cur.clear();
    }
}

impl PaintStyles<'_> {
    /// The element whose style `kind` of `element` is: `::first-letter` text may sit in an inline descendant of the
    /// block that has the pseudo-element (015 TXT-5), whose style it is.
    fn pseudo_owner(&self, element: NodeId, kind: crate::layout::box_tree::PseudoKind) -> NodeId {
        if kind != crate::layout::box_tree::PseudoKind::FirstLetter {
            return element;
        }
        let mut cur = Some(element);
        while let Some(n) = cur {
            if self
                .style
                .view_pseudo(n, Some(crate::style::view::PseudoKind::FirstLetter))
                .is_some()
            {
                return n;
            }
            cur = self.doc.parent(n).ok().flatten();
        }
        element
    }

    fn cached_paint_style(
        &self,
        element: NodeId,
        pseudo: u8,
        compute: impl FnOnce() -> Option<PaintStyle>,
    ) -> Option<PaintStyle> {
        let Some(id) = self.style.identity(element) else {
            return compute();
        };
        let key = (element, pseudo);
        {
            let mut c = self.cache.borrow_mut();
            if let Some((i, s)) = c.cur.get(&key)
                && *i == id
            {
                return s.clone();
            }
            if let Some((i, s)) = c.prev.remove(&key)
                && i == id
            {
                c.cur.insert(key, (i, s.clone()));
                return s;
            }
        }
        let s = compute();
        self.cache.borrow_mut().cur.insert(key, (id, s.clone()));
        s
    }
}

fn color(c: &crate::style::view::groups::Color) -> Color {
    let [r, g, b, a] = c.to_srgb_f32();
    Color { r, g, b, a }
}

fn border_style(s: BorderStyle) -> ComputedBorderStyle {
    match s {
        BorderStyle::None => ComputedBorderStyle::None,
        BorderStyle::Hidden => ComputedBorderStyle::Hidden,
        BorderStyle::Solid => ComputedBorderStyle::Solid,
        BorderStyle::Dotted => ComputedBorderStyle::Dotted,
        BorderStyle::Dashed => ComputedBorderStyle::Dashed,
        BorderStyle::Double => ComputedBorderStyle::Double,
        BorderStyle::Groove => ComputedBorderStyle::Groove,
        BorderStyle::Ridge => ComputedBorderStyle::Ridge,
        BorderStyle::Inset => ComputedBorderStyle::Inset,
        BorderStyle::Outset => ComputedBorderStyle::Outset,
    }
}

fn lp_is_zero(v: &LengthPercentage) -> bool {
    match v {
        LengthPercentage::Px(p) | LengthPercentage::Percent(p) => *p == 0.0,
        LengthPercentage::Calc(_) => false,
    }
}

fn style_pseudo(
    kind: crate::layout::box_tree::PseudoKind,
) -> Option<crate::style::view::PseudoKind> {
    use crate::layout::box_tree::PseudoKind as K;
    match kind {
        K::Before => Some(crate::style::view::PseudoKind::Before),
        K::After => Some(crate::style::view::PseudoKind::After),
        K::Marker => Some(crate::style::view::PseudoKind::Marker),
        K::FirstLetter => Some(crate::style::view::PseudoKind::FirstLetter),
        K::FirstLine => Some(crate::style::view::PseudoKind::FirstLine),
        K::FirstLineBefore => Some(crate::style::view::PseudoKind::FirstLineBefore),
        K::FirstLineAfter => Some(crate::style::view::PseudoKind::FirstLineAfter),
        K::Backdrop => Some(crate::style::view::PseudoKind::Backdrop),
    }
}

impl PaintStyleSource for PaintStyles<'_> {
    fn paint_style(&self, element: NodeId) -> Option<PaintStyle> {
        self.cached_paint_style(element, 0, || {
            let v = self.style.view(element)?;
            self.paint_style_of(&v)
        })
    }

    fn scrollbar_style(&self, element: NodeId) -> Option<crate::scene::inputs::ScrollbarStyle> {
        if !self.scrollbars {
            return None;
        }
        let view = self.style.view(element)?;
        let paint = view.paint();
        let (thumb, track) = paint
            .scrollbar_colors
            .as_ref()
            .map(|(t, r)| (color(t), color(r)))
            .unwrap_or((
                Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.45,
                },
                Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                },
            ));
        Some(crate::scene::inputs::ScrollbarStyle {
            width: paint.scrollbar_width,
            left: view.inline_text().direction == crate::style::view::groups::Direction::Rtl,
            thumb,
            track,
        })
    }

    fn column_rule(&self, element: NodeId) -> Option<(ComputedBorderStyle, Color)> {
        let v = self.style.view(element)?;
        let f = v.fragmentation();
        Some((
            border_style(f.column_rule_style),
            color(&f.column_rule_color),
        ))
    }

    fn paint_style_pseudo(
        &self,
        element: NodeId,
        kind: crate::layout::box_tree::PseudoKind,
    ) -> Option<PaintStyle> {
        use crate::layout::box_tree::PseudoKind as K;
        let tag = match kind {
            K::Before => 1,
            K::After => 2,
            K::Marker => 3,
            K::FirstLetter => 4,
            K::FirstLine => 5,
            K::FirstLineBefore => 6,
            K::FirstLineAfter => 7,
            K::Backdrop => 8,
        };
        let owner = self.pseudo_owner(element, kind);
        self.cached_paint_style(owner, tag, || {
            let v = self.style.view_pseudo(owner, Some(style_pseudo(kind)?))?;
            self.paint_style_of(&v)
        })
    }

    fn paint_extras_pseudo(
        &self,
        element: NodeId,
        kind: crate::layout::box_tree::PseudoKind,
        border_box: (f32, f32, f32, f32),
    ) -> BoxExtras {
        match style_pseudo(kind).and_then(|k| {
            self.style
                .view_pseudo(self.pseudo_owner(element, kind), Some(k))
        }) {
            Some(v) => {
                let mut e = crate::pipeline::paint_extras::extras_of(
                    self.doc,
                    element,
                    &v,
                    border_box,
                    self.images,
                );
                crate::pipeline::paint_extras::pseudo_image(
                    element,
                    kind == crate::layout::box_tree::PseudoKind::Marker,
                    kind == crate::layout::box_tree::PseudoKind::Before,
                    &v,
                    self.images,
                    &mut e,
                );
                e
            }
            None => BoxExtras::default(),
        }
    }

    fn paint_extras(&self, element: NodeId, border_box: (f32, f32, f32, f32)) -> BoxExtras {
        let Some(v) = self.style.view(element) else {
            return BoxExtras::default();
        };
        let Some(id) = self.style.identity(element) else {
            return crate::pipeline::paint_extras::extras_of(
                self.doc,
                element,
                &v,
                border_box,
                self.images,
            );
        };
        // Only a transform reads the border box's position (its origin); the extras of a box without one are
        // kept by size, so a scroll or a move keeps them.
        let bb = [border_box.2.to_bits(), border_box.3.to_bits()];
        {
            let mut c = self.cache.borrow_mut();
            if let Some((i, b, e)) = c.extras_cur.get(&element)
                && *i == id
                && *b == bb
            {
                return e.clone();
            }
            if let Some((i, b, e)) = c.extras_prev.remove(&element)
                && i == id
                && b == bb
            {
                c.extras_cur.insert(element, (i, b, e.clone()));
                return e;
            }
        }
        let e = crate::pipeline::paint_extras::extras_of(
            self.doc,
            element,
            &v,
            border_box,
            self.images,
        );
        if e.transform.is_none()
            && !crate::pipeline::paint_extras::reads_images(&v)
            && e.replaced.is_none()
            && self.doc.local_name(element).is_ok_and(|n| n != "img")
        {
            self.cache
                .borrow_mut()
                .extras_cur
                .insert(element, (id, bb, e.clone()));
        }
        e
    }

    fn edit_state(&self, element: NodeId) -> Option<crate::scene::appendix_e::EditState> {
        self.edit_state_of(element)
    }

    fn control_facts(&self, element: NodeId) -> Option<crate::scene::inputs::ControlFacts> {
        self.control_facts_of(element)
    }

    fn canvas_body(&self) -> Option<NodeId> {
        self.canvas_body_of()
    }

    fn text_nodes(&self, element: NodeId) -> Vec<(NodeId, usize)> {
        self.text_nodes_of(element)
    }

    fn unchanged(&self, node: NodeId) -> bool {
        use ui4me_document::ui_state::UiFlags;
        let mut c = self.cache.borrow_mut();
        if let Some((_, answer)) = c.nodes_cur.get(&node) {
            return *answer;
        }
        let identity = self.style.identity(node);
        let prev = c.nodes_prev.get(&node).map(|(f, _)| f);
        let same = prev.is_some_and(|p| p.identity == identity);
        let images = match prev {
            Some(p) if same => p.images,
            _ => crate::pipeline::paint_extras::node_reads_images(self.doc, self.style, node),
        };
        // The focused control's edit overlay reads the caret blink phase and affinity (not journaled).
        let focused = self
            .doc
            .ui_flags(node)
            .is_ok_and(|f| f.contains(UiFlags::FOCUS));
        let answer =
            same && !images && !focused && c.dirty.as_ref().is_some_and(|d| !d.contains(&node));
        c.nodes_cur
            .insert(node, (NodeFacts { identity, images }, answer));
        answer
    }
}

impl PaintStyles<'_> {
    fn paint_style_of(&self, v: &crate::style::view::ComputedStyleView<'_>) -> Option<PaintStyle> {
        let paint = v.paint();
        let bm = v.box_model();
        let radius =
            |c: &(LengthPercentage, LengthPercentage)| !(lp_is_zero(&c.0) && lp_is_zero(&c.1));
        Some(PaintStyle {
            accent_color: paint.accent_color.as_ref().map(color),
            color: color(&paint.color),
            background_color: color(&paint.background_color),
            opacity: paint.opacity,
            visible: v.display_flow().visibility == Visibility::Visible,
            border_style: [
                bm.border_style.top,
                bm.border_style.right,
                bm.border_style.bottom,
                bm.border_style.left,
            ]
            .map(border_style),
            border_color: [
                &bm.border_color.top,
                &bm.border_color.right,
                &bm.border_color.bottom,
                &bm.border_color.left,
            ]
            .map(color),
            decoration_color: color(&v.text_decoration().color),
            text_shadow: paint
                .text_shadow
                .iter()
                .map(|s| crate::scene::appendix_e::TextShadowLayer {
                    offset_x: s.offset_x,
                    offset_y: s.offset_y,
                    blur: s.blur,
                    color: color(&s.color),
                })
                .collect(),
            border_radius_non_initial: [
                radius(&bm.border_radius.top_left),
                radius(&bm.border_radius.top_right),
                radius(&bm.border_radius.bottom_right),
                radius(&bm.border_radius.bottom_left),
            ],
        })
    }

    fn edit_state_of(&self, element: NodeId) -> Option<crate::scene::appendix_e::EditState> {
        use ui4me_document::ui_state::UiFlags;
        let c = self.doc.control_state(element).ok().flatten()?;
        let value_len = self
            .doc
            .control_value(element)
            .map_or(0, |v| v.encode_utf16().count() as u32);
        // No stored selection: the caret is at the end of the value.
        let (start, end) = c.selection.map_or((value_len, value_len), |s| {
            (
                s.start.min(s.end).min(value_len),
                s.start.max(s.end).min(value_len),
            )
        });
        let focused = self
            .doc
            .ui_flags(element)
            .is_ok_and(|f| f.contains(UiFlags::FOCUS));
        let composition = c.composition.filter(|(a, b)| a <= b && *b <= value_len);
        Some(crate::scene::appendix_e::EditState {
            start,
            end,
            upstream: self.caret.1,
            focused,
            caret_visible: self.caret.0,
            composition,
        })
    }

    /// What the scene draws of a form control (lane L-CTRL-r10), from 003's control state and the
    /// computed `appearance`: `none` leaves checkbox, radio and the select arrow to the author's CSS;
    /// a `range` is always drawn (ui4me has no `::slider-*` pseudo-elements to style one with).
    fn control_facts_of(&self, element: NodeId) -> Option<crate::scene::inputs::ControlFacts> {
        use crate::scene::inputs::{ControlFacts, ControlKind, MeterRegion};
        use ui4me_document::control_semantics::{ControlKind as ModelControl, InputType};
        use ui4me_document::control_values::MeterRegionKind;
        use ui4me_document::ui_state::UiFlags;
        let doc = self.doc;
        let facts = doc.control_facts(element)?;
        let kind = match facts.kind {
            ModelControl::Select => ControlKind::Select,
            ModelControl::Progress => ControlKind::Progress {
                ratio: doc.progress_ratio(element),
            },
            ModelControl::Meter => {
                let (ratio, region) = doc.meter_state(element)?;
                let region = match region {
                    MeterRegionKind::Optimum => MeterRegion::Optimum,
                    MeterRegionKind::Suboptimal => MeterRegion::Suboptimal,
                    MeterRegionKind::EvenLessGood => MeterRegion::EvenLessGood,
                };
                ControlKind::Meter { ratio, region }
            }
            ModelControl::Input(input_type) => match input_type {
                InputType::Range => ControlKind::Range {
                    ratio: doc.range_ratio(element)?,
                },
                InputType::Checkbox => ControlKind::Checkbox {
                    checked: facts.checked.unwrap_or(false),
                    indeterminate: facts.indeterminate,
                },
                InputType::Radio => ControlKind::Radio {
                    checked: facts.checked.unwrap_or(false),
                },
                InputType::Color => {
                    let v = doc.control_value(element).unwrap_or_default();
                    let hex = v
                        .strip_prefix('#')
                        .filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                        .unwrap_or("000000");
                    let c = |i: usize| {
                        f32::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)) / 255.0
                    };
                    ControlKind::Color {
                        rgb: [c(0), c(2), c(4)],
                    }
                }
                _ => return None,
            },
            _ => return None,
        };
        let flags = doc.ui_flags(element).ok()?;
        let native = self
            .style
            .view(element)
            .is_none_or(|v| v.native_appearance());
        Some(ControlFacts {
            kind,
            disabled: facts.disabled,
            focused: flags.contains(UiFlags::FOCUS),
            native,
        })
    }

    fn canvas_body_of(&self) -> Option<NodeId> {
        let html = self.doc.document_element()?;
        let body = self.doc.body()?;
        // Any containment on the root or the body stops the propagation of the body's background
        // (CSS Containment 2; the reference browser includes style containment and the containment
        // `container-type` implies, measured 2026-11-07; WPT background-color-body-propagation-008).
        let contained = |n: NodeId| self.style.view(n).is_some_and(|v| v.containment().any());
        if contained(html) || contained(body) {
            return None;
        }
        (self.doc.parent(body).ok().flatten() == Some(html)).then_some(body)
    }

    fn text_nodes_of(&self, element: NodeId) -> Vec<(NodeId, usize)> {
        let Ok(children) = self.doc.children(element) else {
            return Vec::new();
        };
        children
            .iter()
            .filter(|c| self.doc.node_kind(**c) == Ok(NodeKind::Text))
            .map(|c| {
                (
                    *c,
                    self.doc
                        .text_data(*c)
                        .map(|t| t.chars().filter(|ch| !ch.is_whitespace()).count())
                        .unwrap_or(0),
                )
            })
            .collect()
    }
}

/// Cold S1–S5 over `doc` with fresh stages (the scratch pipeline of recompute-plan §8).
#[allow(clippy::too_many_arguments)]
pub(crate) fn cold_stages(
    doc: &Document,
    config: &crate::pipeline::state::PipelineConfig,
    fonts: &ui4me_assets::fonts::FontSourcesView,
    images: Option<&Arc<dyn crate::scene::inputs::ImageSource>>,
    resources: Option<&Arc<crate::pipeline::resources::PipelineResources>>,
    generation: u64,
    revision: RevisionTuple,
    scroll: &ScrollOffsets,
    caret: (bool, bool),
    reduced_motion: bool,
    dark: bool,
) -> Result<(StyleBridge, Arc<StyleSnapshot>, LayoutOutput, SceneSnapshot), Status> {
    let (mut style, mut text) = fresh_stages(doc.id(), config, fonts, reduced_motion, dark)?;
    crate::pipeline::resources::configure_style(
        &mut style,
        resources,
        doc,
        config.scale,
        (config.viewport_width as f32, config.viewport_height as f32),
    );
    style.restyle(doc, &EpochSegment::empty())?;
    let mut snap = style.snapshot();
    let crate::layout::coordinator::PassOutcome::Fresh(mut layout) = layout_stage(
        doc,
        &snap,
        config,
        &mut text,
        resources,
        None,
        &mut Default::default(),
        false,
    )?
    else {
        unreachable!("no previous layout to reuse")
    };
    // The container-query loop, cold: every round restyles and lays out from scratch (015 CQ-3).
    if style.has_query_containers() {
        for round in 0..=crate::pipeline::feedback::BOUND {
            let measured =
                layout
                    .container_facts
                    .iter()
                    .map(|f| crate::style::container::MeasuredContainer {
                        node: f.node,
                        width: f.width,
                        height: f.height,
                    });
            if style.update_container_sizes(doc, measured).is_empty()
                || round == crate::pipeline::feedback::BOUND
            {
                break;
            }
            style.restyle(doc, &EpochSegment::empty())?;
            snap = style.snapshot();
            let crate::layout::coordinator::PassOutcome::Fresh(l) = layout_stage(
                doc,
                &snap,
                config,
                &mut text,
                resources,
                None,
                &mut Default::default(),
                false,
            )?
            else {
                unreachable!("no previous layout to reuse")
            };
            layout = l;
        }
    }
    layout.fragments.stamp(revision.layout, Some(revision));
    let mut scroll = scroll.clone();
    scroll.reclamp_connected(&layout.fragments, |n| doc.is_connected(n).unwrap_or(false));
    let scene = scene_stage(
        doc,
        &snap,
        &layout.fragments,
        None,
        generation,
        revision,
        images,
        &scroll,
        caret,
        &Default::default(),
    );
    Ok((style, snap, layout, scene))
}
