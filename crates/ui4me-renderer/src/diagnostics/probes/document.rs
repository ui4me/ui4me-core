//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `document` probe input (diagnostics-schema-v0.md §5).
//! T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::ExportCx;
use serde_json::{Value, json};

/// Lifecycle written by DocumentCore (R-27).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEcho {
    Created,
    Parsing,
    Live,
    Closing,
    Closed,
}

/// Pipeline state (005 recompute-plan §2), mapped from `crate::pipeline::state::PipelineState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineStateEcho {
    Clean,
    Dirty,
    Updating,
    FailedUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentProbeInput {
    pub lifecycle: LifecycleEcho,
    pub pipeline: PipelineStateEcho,
    pub failure_count: u64,
    pub non_convergence: bool,
    pub detached_subtrees: u64,
    pub node_count: u64,
    pub pending_resources: u64,
}

impl LifecycleEcho {
    pub const fn name(self) -> &'static str {
        match self {
            LifecycleEcho::Created => "Created",
            LifecycleEcho::Parsing => "Parsing",
            LifecycleEcho::Live => "Live",
            LifecycleEcho::Closing => "Closing",
            LifecycleEcho::Closed => "Closed",
        }
    }
}

impl PipelineStateEcho {
    pub const fn name(self) -> &'static str {
        match self {
            PipelineStateEcho::Clean => "Clean",
            PipelineStateEcho::Dirty => "Dirty",
            PipelineStateEcho::Updating => "Updating",
            PipelineStateEcho::FailedUpdate => "FailedUpdate",
        }
    }
}

pub(crate) fn to_json(d: &DocumentProbeInput, cx: &ExportCx) -> Value {
    json!({
        "document": cx.ordinal, "lifecycle": d.lifecycle.name(), "pipeline": d.pipeline.name(),
        "failure_count": d.failure_count, "non_convergence": d.non_convergence,
        "detached_subtrees": d.detached_subtrees, "node_count": d.node_count, "pending_resources": d.pending_resources
    })
}
