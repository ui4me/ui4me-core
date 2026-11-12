//! owner: 005-pipeline-diagnostics — T039 (FR-077, FR-078; IF-32, IF-34). The `performance`
//! probe: per-frame stage timings and latencies (a 256-frame ring), per-component memory
//! counters, and the event-loop wake and idle-turn counters. **Every member sits inside the
//! probe's `nondeterministic` section** (FR-071), so it never enters determinism comparison.
//! External RSS/PSS is 014's and is not exported.

use crate::diagnostics::envelope::{ExportCx, num};
use serde_json::{Map, Value, json};
use ui4me_engine_types::diag::{FrameTiming, PerformanceStore};

fn frame_json(f: &FrameTiming) -> Value {
    let mut stage = Map::new();
    for (k, v) in [
        ("style", f.stage_ms.style),
        ("layout", f.stage_ms.layout),
        ("scene", f.stage_ms.scene),
        ("raster", f.stage_ms.raster),
        ("present", f.stage_ms.present),
    ] {
        if let Some(v) = v {
            stage.insert(k.into(), num(v));
        }
    }
    let mut m = Map::new();
    m.insert("frame_seq".into(), json!(f.frame_seq));
    if let Some(t) = &f.target {
        m.insert("target".into(), json!(t));
    }
    m.insert("stage_ms".into(), Value::Object(stage));
    if let Some(v) = f.input_to_present_ms {
        m.insert("input_to_present_ms".into(), num(v));
    }
    if let Some(v) = f.mutation_to_present_ms {
        m.insert("mutation_to_present_ms".into(), num(v));
    }
    Value::Object(m)
}

pub(crate) fn to_json(p: &PerformanceStore, _cx: &ExportCx) -> Value {
    let frames: Vec<Value> = p.frames.iter().map(frame_json).collect();
    let components: Vec<Value> = p.components().iter().map(|c| json!({"name": c.name, "current_bytes": c.current_bytes, "peak_bytes": c.peak_bytes})).collect();
    json!({"nondeterministic": {
        "frames": frames,
        "memory": {"components": components, "total_current_bytes": p.total_current_bytes(), "total_peak_bytes": p.total_peak_bytes()},
        "counters": {"event_loop_wakes": p.event_loop_wakes, "idle_turns": p.idle_turns},
    }})
}
