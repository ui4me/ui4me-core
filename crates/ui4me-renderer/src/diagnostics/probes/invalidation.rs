//! owner: 005-pipeline-diagnostics — T029. The `invalidation` probe (schema §5; recompute-plan §4
//! step 9): the last 16 updates with trigger, mode, coalesced reasons and subjects, stages run,
//! `widened`, feedback-loop iteration counts and the outcome. Plain data in, JSON out; the
//! pipeline assembles [`InvalidationProbeInput`] from its update ring.

use crate::diagnostics::envelope::ExportCx;
use serde_json::{Map, Value, json};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;

/// Updates retained (recompute-plan §10).
pub const UPDATES_RETAINED: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidationOutcome {
    Published,
    Failed,
    StaleDiscarded,
    NonConvergence,
}

impl InvalidationOutcome {
    pub const fn name(self) -> &'static str {
        match self {
            InvalidationOutcome::Published => "published",
            InvalidationOutcome::Failed => "failed",
            InvalidationOutcome::StaleDiscarded => "stale-discarded",
            InvalidationOutcome::NonConvergence => "non-convergence",
        }
    }
}

/// One coalesced `(reason, subject)` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReasonEntry {
    /// The stable reason code (`DOM_ATTR_CLASS`, …).
    pub code: &'static str,
    /// The node subject, when the reason has one.
    pub subject: Option<NodeId>,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateEntry {
    pub update_seq: u64,
    /// `flush-read`, `probe`, `frame`, `explicit` or `oracle-prep`.
    pub trigger: &'static str,
    /// `incremental` or `cold`.
    pub mode: &'static str,
    pub reasons: Vec<ReasonEntry>,
    /// Stages run, as the stage letters `S`, `B`, `L`, `P`.
    pub stages_run: Vec<char>,
    pub widened: bool,
    pub loop_relational: u32,
    pub loop_container: u32,
    pub outcome: InvalidationOutcome,
    /// `(code, stage, diagnostic_id)` of a failed update.
    pub error: Option<(Code, Stage, u64)>,
}

/// The last updates, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InvalidationProbeInput {
    pub updates: Vec<UpdateEntry>,
}

pub(crate) fn to_json(input: &InvalidationProbeInput, cx: &ExportCx) -> Value {
    let skip = input.updates.len().saturating_sub(UPDATES_RETAINED);
    Value::Array(
        input
            .updates
            .iter()
            .skip(skip)
            .map(|u| {
                let reasons: Vec<Value> = u
                    .reasons
                    .iter()
                    .map(|r| {
                        let mut m = Map::new();
                        m.insert("code".into(), json!(r.code));
                        if let Some(n) = r.subject {
                            m.insert("subject".into(), cx.key(n));
                        }
                        m.insert("count".into(), json!(r.count));
                        Value::Object(m)
                    })
                    .collect();
                let mut m = Map::new();
                m.insert("update_seq".into(), json!(u.update_seq));
                m.insert("trigger".into(), json!(u.trigger));
                m.insert("mode".into(), json!(u.mode));
                m.insert("reasons".into(), Value::Array(reasons));
                m.insert("stages_run".into(), json!(u.stages_run.iter().map(char::to_string).collect::<Vec<_>>()));
                m.insert("widened".into(), json!(u.widened));
                m.insert("loop_iterations".into(), json!({"relational": u.loop_relational, "container": u.loop_container}));
                m.insert("outcome".into(), json!(u.outcome.name()));
                if let Some((code, stage, id)) = u.error {
                    m.insert("error".into(), json!({"code": code.name(), "stage": crate::diagnostics::envelope::stage_name(stage), "diagnostic_id": id}));
                }
                Value::Object(m)
            })
            .collect(),
    )
}
