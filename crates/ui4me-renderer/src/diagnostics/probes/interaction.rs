//! owner: 005-pipeline-diagnostics — T001 placeholder (FR-056). Probe `interaction` (content owner 012).
//! Until the producing component supplies its input, the probe answers `UNSUPPORTED_PROBE`
//! naming itself (never empty data); the implementing task replaces this body.

/// The probe's schema name.
pub const NAME: &str = "interaction";
/// The component that supplies the content.
pub const OWNER: &str = "012";

/// The `UNSUPPORTED_PROBE` reason while no producer exists.
pub fn unsupported_reason() -> String {
    format!("probe `{NAME}` is not produced yet (content owner {OWNER})")
}
