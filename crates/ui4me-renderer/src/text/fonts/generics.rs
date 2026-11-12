//! owner: 009-text-inline (T019; FR-075; font-matching.md §1)
//!
//! Generic family handling: base sizes per generic. The name-to-family mapping itself is 007's
//! pinned `generic_map` applied by `FontEnvironment::resolve_families`.

use ui4me_engine_types::text_metrics::GenericFamily;

/// Base `font-size: medium` per generic (FR-075): 16 px; `monospace` and `ui-monospace` 13 px.
pub fn base_size(generic: Option<GenericFamily>) -> f32 {
    match generic {
        Some(GenericFamily::Monospace) | Some(GenericFamily::UiMonospace) => 13.0,
        _ => 16.0,
    }
}
