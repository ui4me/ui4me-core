//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `unsupported` probe input: the R-20 store (`ui4me_engine_types::diag::UnsupportedReports`,
//! de-duplicated per DOM revision), exported sorted by report key.
//! T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::{ExportCx, stage_name};
use serde_json::{Map, Value, json};
use ui4me_engine_types::diag::{
    ReportLocation, SheetOrigin, UnsupportedReport, UnsupportedReports,
};

#[derive(Clone, Debug, Default)]
pub struct UnsupportedProbeInput {
    pub reports: UnsupportedReports,
    pub overflow_count: u64,
}

fn origin_name(o: SheetOrigin) -> &'static str {
    // The schema's `user` origin has no producer in v1 (no user style sheets); core has no
    // variant for it (adding one would break exhaustive matches in ui4me-style).
    match o {
        SheetOrigin::UserAgent => "ua",
        SheetOrigin::Author => "author",
        SheetOrigin::Inline => "inline",
    }
}

/// Sort key: (feature, stage, document, location) — stylesheet locations before element ones.
fn sort_key(r: &UnsupportedReport, cx: &ExportCx) -> (String, u8, u32, u8, Vec<u32>) {
    let k = &r.key;
    let loc = match k.location {
        ReportLocation::Stylesheet {
            sheet,
            line,
            column,
            ..
        } => (0, vec![sheet, line, column]),
        ReportLocation::Element { node } => (
            1,
            cx.keys
                .get(node)
                .map(|x| x.path.clone())
                .unwrap_or_else(|| vec![u32::MAX, node.raw().index]),
        ),
    };
    (
        k.feature.as_str().to_string(),
        k.stage.code(),
        cx.ordinal_of(k.document),
        loc.0,
        loc.1,
    )
}

pub(crate) fn to_json(u: &UnsupportedProbeInput, cx: &ExportCx) -> Value {
    let mut reports: Vec<&UnsupportedReport> = u
        .reports
        .iter()
        .filter(|r| cx.includes(r.key.document))
        .collect();
    reports.sort_by_cached_key(|r| sort_key(r, cx));
    let out: Vec<Value> = reports
        .into_iter()
        .map(|r| {
            let k = &r.key;
            let location = match k.location {
                ReportLocation::Stylesheet { sheet, origin, line, column } => {
                    let mut m = Map::new();
                    m.insert("kind".into(), json!("stylesheet"));
                    m.insert("sheet".into(), json!(sheet));
                    m.insert("origin".into(), json!(origin_name(origin)));
                    if line >= 1 {
                        m.insert("line".into(), json!(line));
                    }
                    if column >= 1 {
                        m.insert("column".into(), json!(column));
                    }
                    Value::Object(m)
                }
                ReportLocation::Element { node } => json!({"kind": "element", "node": cx.key(node)}),
            };
            json!({
                "feature": k.feature.as_str(), "code": r.code.name(), "stage": stage_name(k.stage),
                "document": cx.ordinal_of(k.document), "reason": r.reason.name(), "location": location,
                "revisions": r.revisions, "first_seen": {"dom": r.first_seen_dom}, "last_seen": {"dom": r.last_seen_dom}
            })
        })
        .collect();
    json!({"overflow_count": u.overflow_count + u.reports.overflow_count(), "reports": out})
}
