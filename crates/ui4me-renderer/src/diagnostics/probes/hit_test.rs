//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `hit_test` probe input: answers from the HitTestIndex for the requested points (AD-22).
//! T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::{ExportCx, point_json};
use serde_json::{Value, json};
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Debug, PartialEq)]
pub struct HitQueryAnswer {
    pub point: (f64, f64),
    /// Topmost first: `(node, local point)`.
    pub path: Vec<(NodeId, (f64, f64))>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HitTestProbeInput {
    pub scene: u64,
    pub queries: Vec<HitQueryAnswer>,
}

pub(crate) fn to_json(h: &HitTestProbeInput, cx: &ExportCx) -> Value {
    let queries: Vec<Value> = h
        .queries
        .iter()
        .map(|q| {
            let path: Vec<Value> = q
                .path
                .iter()
                .map(|(n, (x, y))| json!({"node": cx.key(*n), "local": point_json(*x, *y)}))
                .collect();
            json!({"point": point_json(q.point.0, q.point.1), "path": path})
        })
        .collect();
    json!({"scene": h.scene, "queries": queries})
}
