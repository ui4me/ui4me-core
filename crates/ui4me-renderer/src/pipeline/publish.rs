//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Atomic publication (005 recompute-plan §4 step 9): the committed set swapped in as one
//! reference-counted value, output epochs incremented for changed outputs.
//! Implemented by 001 T049.
//!
//! Every stage consumes only the immutable published output of the earlier stage of the same
//! update: S2–S4 read the `Arc<StyleSnapshot>` of S1, S5 reads S3/S4's `LayoutOutput` and the
//! same snapshot. Nothing in a published set is mutated afterwards.

use crate::layout::coordinator::LayoutOutput;
use crate::scene::appendix_e::SceneSnapshot;
use crate::style::view::StyleSnapshot;
use std::sync::Arc;
use ui4me_engine_types::revision::RevisionTuple;
use ui4me_engine_types::status::Status;

/// Immutable committed set `{style, boxes, fragments, scene+hit}` with one revision tuple.
#[derive(Debug)]
pub struct CommittedSet {
    pub revision: RevisionTuple,
    pub scene: Arc<SceneSnapshot>,
    /// S1 output (006).
    pub style: Arc<StyleSnapshot>,
    /// S2–S4 output (008): box tree, fragment tree, layout reports and diagnostics.
    pub layout: Arc<LayoutOutput>,
    /// O3 diagnostics attached to the update that produced this set (never error statuses).
    pub diagnostics: Vec<Status>,
}
