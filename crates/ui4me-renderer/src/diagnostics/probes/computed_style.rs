//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `computed_style.resolved_style` probe input (neutral): CSSOM resolved values for the
//! requested property list. T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::ExportCx;
use serde_json::{Map, Value, json};
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedStyleInput {
    pub node: NodeId,
    /// `(property name, CSSOM-serialised value)`.
    pub properties: Vec<(String, String)>,
}

pub(crate) fn to_json(list: &[ResolvedStyleInput], cx: &ExportCx) -> Value {
    let entries: Vec<Value> = list
        .iter()
        .map(|e| {
            let props: Map<String, Value> = e
                .properties
                .iter()
                .map(|(k, v)| (k.clone(), json!(cx.redaction.scrub(v))))
                .collect();
            json!({"node": cx.key(e.node), "properties": props})
        })
        .collect();
    json!({"resolved_style": entries})
}
