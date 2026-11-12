//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `runtime` probe input (diagnostics-schema-v0.md §5). The capability report mirrors 010's
//! table as plain data (this crate does not depend on `ui4me-raster`).
//! T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::{ClockMode, ExportCx, SCHEMA_VERSION, num};
use serde_json::{Map, Value, json};
use ui4me_engine_types::diag::CapabilityId;
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssFeatureProfileEcho {
    pub name: String,
    pub enabled: Vec<String>,
    pub gated: Vec<String>,
    pub disabled: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityTier {
    Mandatory,
    Optional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityAvailability {
    Native,
    Owned,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterCapabilityEcho {
    pub id: CapabilityId,
    pub tier: CapabilityTier,
    pub availability: CapabilityAvailability,
    pub fallback: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProbeInput {
    pub ui4me_version: String,
    pub abi_version: String,
    pub css_feature_profile: CssFeatureProfileEcho,
    pub raster_capabilities: Vec<RasterCapabilityEcho>,
    pub raster_capability_table: Option<String>,
    pub clock_mode: ClockMode,
    pub documents: u32,
}

impl CapabilityTier {
    pub const fn name(self) -> &'static str {
        match self {
            CapabilityTier::Mandatory => "mandatory",
            CapabilityTier::Optional => "optional",
        }
    }
}

impl CapabilityAvailability {
    pub const fn name(self) -> &'static str {
        match self {
            CapabilityAvailability::Native => "native",
            CapabilityAvailability::Owned => "owned",
            CapabilityAvailability::Unavailable => "unavailable",
        }
    }
}

pub(crate) fn to_json(r: &RuntimeProbeInput) -> Value {
    let caps: Vec<Value> = r
        .raster_capabilities
        .iter()
        .map(|c| {
            let mut m = Map::new();
            m.insert("id".into(), json!(c.id.0));
            m.insert("tier".into(), json!(c.tier.name()));
            m.insert("availability".into(), json!(c.availability.name()));
            if let Some(f) = c.fallback {
                m.insert("fallback".into(), json!(f));
            }
            Value::Object(m)
        })
        .collect();
    let p = &r.css_feature_profile;
    let mut m = Map::new();
    m.insert("ui4me_version".into(), json!(r.ui4me_version));
    m.insert("abi_version".into(), json!(r.abi_version));
    m.insert("schema_version".into(), json!(SCHEMA_VERSION));
    m.insert(
        "css_feature_profile".into(),
        json!({"name": p.name, "enabled": p.enabled, "gated": p.gated, "disabled": p.disabled}),
    );
    m.insert("raster_capabilities".into(), Value::Array(caps));
    if let Some(t) = &r.raster_capability_table {
        m.insert("raster_capability_table".into(), json!(t));
    }
    m.insert("clock_mode".into(), json!(r.clock_mode.name()));
    m.insert("documents".into(), json!(r.documents));
    Value::Object(m)
}

// ------------------------------------------------------------------ animation_clock (T050/T052)

/// Animation-clock state (recompute-plan §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockState {
    Stopped,
    Ticking,
    Manual,
}

/// One running transition or animation.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveAnimation {
    pub node: NodeId,
    pub property: String,
    /// `true` = `animation`, `false` = `transition`.
    pub is_animation: bool,
    pub start_ms: f64,
    pub end_ms: f64,
}

/// `animation_clock` probe input. In the slice the clock is inert: `active` is always empty and
/// non-initial `transition-*`/`animation-*` values are reported as unsupported (FR-022).
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationClockInput {
    pub mode: ClockMode,
    pub time_ms: f64,
    pub state: ClockState,
    pub active: Vec<ActiveAnimation>,
}

pub(crate) fn animation_clock_json(a: &AnimationClockInput, cx: &ExportCx) -> Value {
    let state = match a.state {
        ClockState::Stopped => "stopped",
        ClockState::Ticking => "ticking",
        ClockState::Manual => "manual",
    };
    let active: Vec<Value> = a
        .active
        .iter()
        .map(|x| {
            json!({"node": cx.key(x.node), "property": x.property, "kind": if x.is_animation { "animation" } else { "transition" },
                   "start_ms": num(x.start_ms), "end_ms": num(x.end_ms)})
        })
        .collect();
    // Realtime: wall time is nondeterministic (ERRATA E-005-R1); manual: deterministic.
    if a.mode.name() == "realtime" {
        json!({"mode": a.mode.name(), "nondeterministic": {"time_ms": num(a.time_ms)}, "state": state, "active": active})
    } else {
        json!({"mode": a.mode.name(), "time_ms": num(a.time_ms), "state": state, "active": active})
    }
}
