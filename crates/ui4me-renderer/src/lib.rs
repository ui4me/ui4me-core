//! Native HTML/CSS document engine. Backend stages are private implementation details.
// Private backend helpers also support internal regression/diagnostic tools. Keeping
// them private intentionally makes them unreachable to application callers.
pub mod a11y;
#[allow(dead_code)]
pub(crate) mod diagnostics;
mod document;
#[allow(dead_code)]
pub(crate) mod interaction;
#[allow(dead_code)]
pub(crate) mod layout;
#[allow(dead_code)]
pub(crate) mod pipeline;
#[allow(dead_code)]
pub(crate) mod raster;
#[allow(dead_code)]
pub(crate) mod scene;
#[allow(dead_code)]
pub(crate) mod style;
#[allow(dead_code)]
pub(crate) mod text;
/// The hosted CSS feature profile (feature id, state name). Page rules can be
/// parsed as inert screen CSS, but no page-output capability is provided.
pub fn css_profile() -> (&'static str, Vec<(&'static str, &'static str)>) {
    (
        "ui4me-desktop-1.0",
        style::profile::table::FEATURES
            .iter()
            .map(|(id, s)| {
                (
                    *id,
                    if matches!(*id, "css.at-rule.page" | "css.property.page") {
                        "off"
                    } else {
                        s.name()
                    },
                )
            })
            .collect(),
    )
}
// Retained page-output regressions are not an application API.
#[cfg(test)]
pub(crate) use document::paged_facade::PageSize;
pub use document::{
    Clipboard, ClipboardRequest, DefaultActionRule, Document, DomOp, Error, ImeState,
    ImeSurrounding, Input, MAX_PIXELS, MAX_TEXT, NodeOp, Rect, Result, ScrollState,
};
/// Generation-checked DOM node identity (opaque to hosts).
pub use ui4me_engine_types::ids::NodeId;

#[cfg(test)]
mod fuzz_gate;
#[cfg(test)]
mod fuzz_resources;
#[cfg(test)]
mod regression;
