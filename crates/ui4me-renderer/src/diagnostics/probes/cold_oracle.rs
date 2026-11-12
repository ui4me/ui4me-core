//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `cold_oracle` probe input (schema §5; recompute-plan §8): the live committed set compared with
//! a cold scratch pipeline, in stage order. On demand only. Serialised by 001 T052.

use crate::diagnostics::envelope::ExportCx;
use serde_json::{Map, Value, json};
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColdStage {
    Cascade,
    Box,
    Layout,
    Scene,
    HitIndex,
}

impl ColdStage {
    /// Comparison order (recompute-plan §8 step 4).
    pub const ALL: &'static [ColdStage] = &[
        ColdStage::Cascade,
        ColdStage::Box,
        ColdStage::Layout,
        ColdStage::Scene,
        ColdStage::HitIndex,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ColdStage::Cascade => "cascade",
            ColdStage::Box => "box",
            ColdStage::Layout => "layout",
            ColdStage::Scene => "scene",
            ColdStage::HitIndex => "hit-index",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColdDifference {
    pub stage: ColdStage,
    pub node: Option<NodeId>,
    pub field: String,
    pub live: String,
    pub cold: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColdOracleInput {
    pub equal: bool,
    pub stages: Vec<(ColdStage, bool)>,
    pub first_difference: Option<ColdDifference>,
    /// `(node, property)` excluded because a transition or animation is running.
    pub skipped: Vec<(NodeId, String)>,
    pub seed: Option<u64>,
    pub step: Option<u64>,
}

pub(crate) fn to_json(c: &ColdOracleInput, cx: &ExportCx) -> Value {
    let mut m = Map::new();
    m.insert("equal".into(), json!(c.equal));
    m.insert(
        "stages".into(),
        Value::Array(
            c.stages
                .iter()
                .map(|(s, e)| json!({"stage": s.name(), "equal": e}))
                .collect(),
        ),
    );
    if let Some(d) = &c.first_difference {
        let mut f = Map::new();
        f.insert("stage".into(), json!(d.stage.name()));
        if let Some(n) = d.node {
            f.insert("node".into(), cx.key(n));
        }
        f.insert("field".into(), json!(d.field));
        f.insert("live".into(), json!(cx.redaction.text(&d.live)));
        f.insert("cold".into(), json!(cx.redaction.text(&d.cold)));
        m.insert("first_difference".into(), Value::Object(f));
    }
    m.insert(
        "skipped".into(),
        Value::Array(
            c.skipped
                .iter()
                .map(|(n, p)| json!({"node": cx.key(*n), "property": p, "reason": "animating"}))
                .collect(),
        ),
    );
    if let Some(s) = c.seed {
        m.insert("seed".into(), json!(s));
    }
    if let Some(s) = c.step {
        m.insert("step".into(), json!(s));
    }
    m.insert("nondeterministic".into(), json!({}));
    Value::Object(m)
}
