//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `render_receipts` probe input: 010's RenderReceipt (`contracts/render-receipt.md` §1) as
//! plain data (this crate does not depend on `ui4me-raster`; the summary
//! `ui4me_engine_types::diag::ReceiptRecord` lacks fields the schema requires).
//! T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::{ExportCx, num};
use serde_json::{Map, Value, json};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::CapabilityId;
use ui4me_engine_types::geometry::DeviceRect;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::revision::RevisionTuple;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptTarget {
    Window(u32),
    Headless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptStatusEcho {
    Presented,
    RenderedHeadless,
    Failed(Code),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradedEcho {
    pub owner: Option<NodeId>,
    pub capability: CapabilityId,
    pub fallback: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetEcho {
    pub owner: Option<NodeId>,
    /// `layer-depth` or `effect-surface`.
    pub kind: &'static str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReceiptTimings {
    pub raster_ms: f64,
    pub present_ms: f64,
    pub presented_at_ms: f64,
    pub first_frame: bool,
    pub input_to_present_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReceiptInput {
    pub frame_seq: u64,
    pub target: ReceiptTarget,
    pub scene_generation: u64,
    pub scene_revision: RevisionTuple,
    pub device_width: u32,
    pub device_height: u32,
    pub scale: f64,
    /// `true` = mode `full`.
    pub full: bool,
    pub damage: Vec<DeviceRect>,
    pub degraded: Vec<DegradedEcho>,
    pub budget: Vec<BudgetEcho>,
    pub status: ReceiptStatusEcho,
    pub capability_table: String,
    pub nondeterministic: ReceiptTimings,
}

/// Receipts retained per target in the export (recompute-plan §10).
pub const RECEIPTS_PER_TARGET: usize = 32;

fn target_name(t: ReceiptTarget) -> String {
    match t {
        ReceiptTarget::Window(n) => format!("window:{n}"),
        ReceiptTarget::Headless => "headless".to_string(),
    }
}

pub(crate) fn to_json(list: &[ReceiptInput], cx: &ExportCx) -> Value {
    // The last 32 per target, in the input (sequence) order.
    let mut keep = vec![false; list.len()];
    let mut seen: Vec<(ReceiptTarget, usize)> = Vec::new();
    for (i, r) in list.iter().enumerate().rev() {
        let n = match seen.iter_mut().find(|(t, _)| *t == r.target) {
            Some((_, n)) => n,
            None => {
                seen.push((r.target, 0));
                &mut seen.last_mut().expect("just pushed").1
            }
        };
        if *n < RECEIPTS_PER_TARGET {
            keep[i] = true;
            *n += 1;
        }
    }
    let out: Vec<Value> = list
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(r, _)| {
            let status = match r.status {
                ReceiptStatusEcho::Presented => "presented".to_string(),
                ReceiptStatusEcho::RenderedHeadless => "rendered-headless".to_string(),
                ReceiptStatusEcho::Failed(c) => format!("failed:{}", c.name()),
            };
            let degraded: Vec<Value> = r
                .degraded
                .iter()
                .map(|d| {
                    let mut m = Map::new();
                    if let Some(o) = d.owner {
                        m.insert("owner".into(), cx.key(o));
                    }
                    m.insert("capability".into(), json!(d.capability.0));
                    m.insert("fallback".into(), json!(d.fallback));
                    Value::Object(m)
                })
                .collect();
            let budget: Vec<Value> = r
                .budget
                .iter()
                .map(|b| {
                    let mut m = Map::new();
                    if let Some(o) = b.owner {
                        m.insert("owner".into(), cx.key(o));
                    }
                    m.insert("kind".into(), json!(b.kind));
                    m.insert("code".into(), json!("BUDGET_EXCEEDED"));
                    Value::Object(m)
                })
                .collect();
            let damage: Vec<Value> = r.damage.iter().map(|d| json!({"x": d.x, "y": d.y, "w": d.w, "h": d.h})).collect();
            let t = &r.nondeterministic;
            let mut nd = Map::new();
            nd.insert("raster_ms".into(), num(t.raster_ms));
            nd.insert("present_ms".into(), num(t.present_ms));
            nd.insert("presented_at_ms".into(), num(t.presented_at_ms));
            nd.insert("first_frame".into(), json!(t.first_frame));
            if let Some(v) = t.input_to_present_ms {
                nd.insert("input_to_present_ms".into(), num(v));
            }
            json!({
                "frame_seq": r.frame_seq, "target": target_name(r.target),
                // The scene revision is the tuple without the `current` flag (all members integers).
                "scene": {"generation": r.scene_generation, "revision": cx.revision_json(&r.scene_revision)},
                "device": {"width": r.device_width, "height": r.device_height, "scale": num(r.scale)},
                "mode": if r.full { "full" } else { "damage" }, "damage": damage, "degraded": degraded, "budget": budget,
                "status": status, "capability_table": r.capability_table, "nondeterministic": nd
            })
        })
        .collect();
    Value::Array(out)
}
