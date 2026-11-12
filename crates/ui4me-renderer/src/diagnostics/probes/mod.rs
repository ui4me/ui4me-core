//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)

pub mod accessibility;
pub mod boxes;
pub mod cold_oracle;
pub mod computed_style;
pub mod display_list;
pub mod document;
pub mod dom;
pub mod dom_counts;
pub mod dom_journal;
pub mod dom_parser_diagnostics;
pub mod dom_stylesheets;
pub mod element_geometry;
pub mod errors;
pub mod event_log;
pub mod fragments;
pub mod hit_test;
pub mod interaction;
pub mod invalidation;
pub mod performance;
pub mod pixels_ref;
pub mod render_receipts;
pub mod resources;
pub mod runtime;
pub mod scroll;
pub mod text_geometry;
pub mod unsupported;

use crate::diagnostics::envelope::{ExportCx, ExtraProbeInputs, ProbeInputs};
use serde_json::Value;

/// Every FR-056 probe name, in the table's order (`animation_clock` and `cold_oracle` included;
/// `pixels_ref` is an input-gated extra).
pub const FR056_PROBES: &[&str] = &[
    "runtime",
    "document",
    "dom",
    "dom.journal",
    "dom.counts",
    "dom.parser_diagnostics",
    "dom.stylesheets",
    "computed_style",
    "element_geometry",
    "text_geometry",
    "scroll",
    "boxes",
    "fragments",
    "display_list",
    "hit_test",
    "event_log",
    "accessibility",
    "interaction",
    "render_receipts",
    "invalidation",
    "animation_clock",
    "unsupported",
    "errors",
    "resources",
    "cold_oracle",
    "performance",
];

/// The `UNSUPPORTED_PROBE` reason of a probe that has no producer yet; always names the probe.
fn not_yet_produced(name: &str) -> Option<String> {
    Some(match name {
        "text_geometry" => text_geometry::unsupported_reason(),
        "boxes" => boxes::unsupported_reason(),
        "accessibility" => accessibility::unsupported_reason(),
        "interaction" => interaction::unsupported_reason(),
        "resources" => resources::unsupported_reason(),
        "dom.journal" => dom_journal::unsupported_reason(),
        "dom.parser_diagnostics" => dom_parser_diagnostics::unsupported_reason(),
        "dom.stylesheets" => dom_stylesheets::unsupported_reason(),
        _ => return None,
    })
}

/// Serialise probe `name`, or `Err(reason)` for `UNSUPPORTED_PROBE`.
pub(crate) fn produce(
    name: &str,
    inputs: &ProbeInputs,
    extra: &ExtraProbeInputs,
    cx: &ExportCx,
) -> Result<Value, String> {
    let missing = || format!("probe `{name}` has no input in this export");
    match name {
        "runtime" => inputs.runtime.as_ref().map(runtime::to_json).ok_or_else(missing),
        "document" => inputs.document.as_ref().map(|d| document::to_json(d, cx)).ok_or_else(missing),
        "dom" => Ok(dom::to_json(&inputs.dom, cx)),
        "dom.counts" => extra.dom_counts.as_ref().map(dom_counts::to_json).ok_or_else(missing),
        "computed_style" => inputs.computed_style.as_ref().map(|c| computed_style::to_json(c, cx)).ok_or_else(missing),
        "element_geometry" => inputs.element_geometry.as_ref().map(|g| element_geometry::to_json(g, cx)).ok_or_else(missing),
        "scroll" => extra
            .scroll
            .as_ref()
            .map(|s| element_geometry::scroll_json(s, cx))
            .ok_or_else(|| "probe `scroll`: scroll offsets and overflow are 012/008 work; this export has no scroll input".to_string()),
        "fragments" => inputs.fragments.as_ref().map(|f| fragments::to_json(f, cx)).ok_or_else(missing),
        "display_list" => inputs.display_list.as_ref().map(|d| display_list::to_json(d, cx)).ok_or_else(missing),
        "hit_test" => inputs.hit_test.as_ref().map(|h| hit_test::to_json(h, cx)).ok_or_else(missing),
        "event_log" => inputs.event_log.as_ref().map(|e| event_log::to_json(e, cx)).ok_or_else(missing),
        "invalidation" => extra.invalidation.as_ref().map(|i| invalidation::to_json(i, cx)).ok_or_else(missing),
        "performance" => extra.performance.as_ref().map(|p| performance::to_json(p, cx)).ok_or_else(missing),
        "animation_clock" => extra.animation_clock.as_ref().map(|a| runtime::animation_clock_json(a, cx)).ok_or_else(missing),
        "unsupported" => inputs.unsupported.as_ref().map(|u| unsupported::to_json(u, cx)).ok_or_else(missing),
        "errors" => inputs.errors.as_ref().map(|e| errors::to_json(e, cx)).ok_or_else(missing),
        "render_receipts" => inputs.render_receipts.as_ref().map(|r| render_receipts::to_json(r, cx)).ok_or_else(missing),
        "pixels_ref" => extra
            .pixels_ref
            .as_ref()
            .map(|p| pixels_ref::to_json(p, cx))
            .ok_or_else(|| "probe `pixels_ref`: no headless render was requested for this export".to_string()),
        "cold_oracle" => extra
            .cold_oracle
            .as_ref()
            .map(|c| cold_oracle::to_json(c, cx))
            .ok_or_else(|| "probe `cold_oracle`: the cold oracle runs on demand only (testing group, R-24)".to_string()),
        other => match not_yet_produced(other) {
            Some(reason) => Err(reason),
            None => Err(format!("unknown probe `{other}`")),
        },
    }
}
