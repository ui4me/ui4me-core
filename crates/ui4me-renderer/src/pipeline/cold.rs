//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Cold-recompute oracle (005 recompute-plan §8; constitution VII): a scratch pipeline with
//! fresh style, font, layout and scene state recomputes the live DOM cold, and the result is
//! compared with the live committed set in stage order (computed values → box structure →
//! fragments → display list → hit index). It also rasters both scenes for the FR-058(b)
//! incremental-versus-cold frame comparison. The live state is never modified (FR-031): the
//! only live effect is the precondition `TRIGGER(oracle-prep)` when the document is `Dirty`.
//! Implemented by 001 T050; the comparison moved to oracle/compare.rs by 005 T059 and the scratch
//! pipeline stays here (T058).

use crate::diagnostics::probes::cold_oracle::ColdOracleInput;
use crate::diagnostics::probes::pixels_ref::sha256_hex;
use crate::pipeline::frame::raster_scratch;
use crate::pipeline::state::{DocumentPipeline, PipelineState, Trigger};
use crate::pipeline::update::cold_stages;
use ui4me_engine_types::status::Status;

/// Result of one cold-oracle run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColdReport {
    /// The `cold_oracle` probe input (stages and the first difference).
    pub oracle: ColdOracleInput,
    /// FR-058(b): live and cold frames byte-identical (`None` when frames were not compared).
    pub frames_equal: Option<bool>,
    /// SHA-256 of the live and cold RGBA8 frames, when compared.
    pub frame_hashes: Option<(String, String)>,
}

impl ColdReport {
    /// Every stage equal and, when compared, the frames byte-identical.
    pub fn equal(&self) -> bool {
        self.oracle.equal && self.frames_equal != Some(false)
    }
}

impl DocumentPipeline {
    /// Run the cold oracle; `compare_frames` also rasters both scenes (FR-058(b)).
    pub fn cold_oracle(&mut self, compare_frames: bool) -> Result<ColdReport, Status> {
        self.check_open()?;
        // 1. Precondition: Clean.
        if self.state() != PipelineState::Clean {
            self.trigger(Trigger::OraclePrep)?;
        }
        let live = self.committed_result()?.committed;

        // 2–3. Scratch pipeline, cold S1–S5 over the live DOM (read-only).
        let (cold_bridge, cold_style, cold_layout, cold_scene) = cold_stages(
            &self.doc,
            &self.config,
            &self.fonts,
            self.images.as_ref(),
            self.resources.as_ref(),
            &self.file_input_labels,
            live.scene.generation,
            live.revision,
            &self.scroll,
            (self.caret_visible, self.caret_upstream),
            self.reduced_motion,
            self.dark,
        )?;

        // 4. Compare in stage order; the first difference is reported (oracle/compare.rs).
        // Properties of a running (or held) transition or animation are not compared (Clarification Q1):
        // a cold restyle starts no transition and restarts a keyframe animation. 006 names the elements.
        let mut animating = self.style.animating_nodes(&self.doc);
        animating.extend(cold_bridge.animating_nodes(&self.doc));
        let oracle = crate::pipeline::oracle::compare::compare(
            &self.doc,
            &live,
            &cold_style,
            &cold_layout,
            &cold_scene,
            &animating,
        );
        drop(cold_bridge);

        // FR-058(b): incremental vs cold frame, byte-identical.
        let (frames_equal, frame_hashes) = if compare_frames {
            let scale = self.config.scale;
            let a = raster_scratch(&live.scene, scale)?;
            let b = raster_scratch(&cold_scene, scale)?;
            (
                Some(a == b),
                Some((sha256_hex(&a.data), sha256_hex(&b.data))),
            )
        } else {
            (None, None)
        };
        // 5. The scratch pipeline is dropped here.
        Ok(ColdReport {
            oracle,
            frames_equal,
            frame_hashes,
        })
    }
}
