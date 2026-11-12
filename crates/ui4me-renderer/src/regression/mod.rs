//! Focused donor regressions retained behind the document engine seam.
mod dom_query;
#[cfg(feature = "fault-injection")]
mod fault;
mod incremental_vs_cold;
mod inline_style;
mod long_document;
mod scroll_retention;
mod views;
