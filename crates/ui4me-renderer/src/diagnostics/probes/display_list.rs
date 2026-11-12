//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `display_list` probe input (engine summary; 010 `contracts/display-list.md` §4 mapping).
//! Kind labels and phase ids are 010's, carried as strings (this crate does not depend on
//! `ui4me-scene`). T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::{ExportCx, num32};
use serde_json::{Map, Value, json};
use ui4me_engine_types::ids::NodeId;

/// f32 CSS px, space `document`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundsF32 {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayItemSummary {
    pub kind: &'static str,
    pub owner: Option<NodeId>,
    pub phase: &'static str,
    pub stacking_path: Vec<u32>,
    pub stacking_context: u32,
    pub clip_chain: Option<u32>,
    pub bounds: BoundsF32,
    /// Solid paints only: sRGB 0…1.
    pub color: Option<[f32; 4]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackingContextSummary {
    pub id: u32,
    pub owner: Option<NodeId>,
    pub parent: Option<u32>,
    pub z_index: i32,
    pub phase: Option<&'static str>,
    pub isolated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayListProbeInput {
    /// Paint order; `order` = index.
    pub items: Vec<DisplayItemSummary>,
    pub stacking_contexts: Vec<StackingContextSummary>,
}

/// CSSOM-style colour string for a solid paint: `rgb(r, g, b)` or `rgba(r, g, b, a)`.
pub(crate) fn css_color(c: [f32; 4]) -> String {
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = c[3].clamp(0.0, 1.0);
    if a >= 1.0 {
        format!("rgb({}, {}, {})", ch(c[0]), ch(c[1]), ch(c[2]))
    } else {
        // CSSOM serialises alpha with at most 3 decimals after rounding to 1/255.
        let a = ((a * 255.0).round() / 255.0 * 1000.0).round() / 1000.0;
        format!("rgba({}, {}, {}, {})", ch(c[0]), ch(c[1]), ch(c[2]), a)
    }
}

pub(crate) fn to_json(d: &DisplayListProbeInput, cx: &ExportCx) -> Value {
    let items: Vec<Value> = d
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let mut m = Map::new();
            m.insert("order".into(), json!(i));
            m.insert("kind".into(), json!(it.kind));
            if let Some(o) = it.owner {
                m.insert("owner".into(), cx.key(o));
            }
            m.insert("phase".into(), json!(it.phase));
            m.insert("stacking_path".into(), json!(it.stacking_path));
            m.insert("stacking_context".into(), json!(it.stacking_context));
            if let Some(c) = it.clip_chain {
                m.insert("clip_chain".into(), json!(c));
            }
            m.insert(
                "bounds".into(),
                json!({"x": num32(it.bounds.x), "y": num32(it.bounds.y), "width": num32(it.bounds.width.max(0.0)), "height": num32(it.bounds.height.max(0.0))}),
            );
            if let Some(c) = it.color {
                m.insert("paint".into(), json!({"color": css_color(c)}));
            }
            Value::Object(m)
        })
        .collect();
    let contexts: Vec<Value> = d
        .stacking_contexts
        .iter()
        .map(|s| {
            let mut m = Map::new();
            m.insert("id".into(), json!(s.id));
            if let Some(o) = s.owner {
                m.insert("owner".into(), cx.key(o));
            }
            if let Some(p) = s.parent {
                m.insert("parent".into(), json!(p));
            }
            m.insert("z_index".into(), json!(s.z_index));
            if let Some(p) = s.phase {
                m.insert("phase".into(), json!(p));
            }
            m.insert("isolated".into(), json!(s.isolated));
            Value::Object(m)
        })
        .collect();
    // The slice scene has no clips (010 adds clip chains); the empty list is complete.
    json!({"space": "document", "items": items, "stacking_contexts": contexts, "clip_chains": []})
}
