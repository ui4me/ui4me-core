//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `errors` probe input (ring of 256). The schema requires a revision per record, which
//! `ui4me_engine_types::diag::ErrorRecord` does not carry, so the store pairs them.
//! T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::{ExportCx, stage_name};
use serde_json::{Map, Value, json};
use ui4me_engine_types::diag::ErrorRecord;
use ui4me_engine_types::revision::RevisionTuple;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorEntry {
    pub record: ErrorRecord,
    pub revision: RevisionTuple,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErrorsProbeInput {
    pub dropped_count: u64,
    pub records: Vec<ErrorEntry>,
}

pub(crate) fn to_json(e: &ErrorsProbeInput, cx: &ExportCx) -> Value {
    let records: Vec<Value> = e
        .records
        .iter()
        .filter(|r| r.record.document.is_none_or(|d| cx.includes(d)))
        .map(|r| {
            let s = &r.record.status;
            let mut m = Map::new();
            m.insert("diagnostic_id".into(), json!(s.diagnostic_id));
            m.insert("code".into(), json!(s.code.name()));
            m.insert("stage".into(), json!(stage_name(s.stage)));
            if let Some(sr) = s.sub_reason {
                m.insert("sub_reason".into(), json!(sr.name()));
            }
            m.insert("message".into(), json!(cx.redaction.text(&s.message)));
            if let Some(d) = s.dispatch_id {
                m.insert("dispatch_id".into(), json!(d.get().get()));
            }
            m.insert(
                "revision".into(),
                Value::Object(cx.revision_json(&r.revision)),
            );
            Value::Object(m)
        })
        .collect();
    json!({"dropped_count": e.dropped_count, "records": records})
}
