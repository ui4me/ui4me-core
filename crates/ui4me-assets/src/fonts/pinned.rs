//! owner: 007-resources (T062) — pinned-fonts-only mode (R-05; 001 FR-025; 007 FR-051, FR-052).
//!
//! Configured at runtime creation through 004's runtime configuration and unchangeable
//! afterwards. System enumeration is disabled; only the configured files and, unless its flag is
//! off, the embedded last-resort font are used; the generic map and fallback list are pinned; the
//! mode is reported in diagnostics. The checksum comparison against a fixture's recorded digest
//! belongs to 002 (`environment-mismatch`), not here.

use super::registry::{FontMode, FontRegistry};
use std::path::PathBuf;
use ui4me_engine_types::text_metrics::GenericFamily;

/// Font configuration of a pinned-fonts-only runtime (004 runtime option; IF-33).
#[derive(Clone, Debug)]
pub struct PinnedFontConfig {
    /// Trusted host declarations (no document root applies).
    pub files: Vec<PathBuf>,
    pub generic_map: Vec<(GenericFamily, Vec<String>)>,
    pub fallback: Vec<String>,
    /// The embedded last-resort font as final fallback (default on; IF-33).
    pub last_resort_fallback: bool,
}

impl Default for PinnedFontConfig {
    fn default() -> Self {
        PinnedFontConfig {
            files: Vec::new(),
            generic_map: Vec::new(),
            fallback: Vec::new(),
            last_resort_fallback: true,
        }
    }
}

impl PinnedFontConfig {
    pub fn with_files(files: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        PinnedFontConfig {
            files: files.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    pub fn without_last_resort(mut self) -> Self {
        self.last_resort_fallback = false;
        self
    }
}

impl FontRegistry {
    /// Build the registry of a pinned-fonts-only runtime. The first missing file fails creation
    /// with `FILE_NOT_FOUND` naming it. With no usable face (no files and the last-resort flag
    /// off) creation fails with `ENGINE_MISCONFIGURED` (FR-054).
    pub fn pinned(
        config: &PinnedFontConfig,
    ) -> Result<FontRegistry, ui4me_engine_types::status::Status> {
        let mut reg =
            FontRegistry::with_last_resort(FontMode::PinnedFontsOnly, config.last_resort_fallback);
        for file in &config.files {
            reg.register_file(file)?;
        }
        for (g, fams) in &config.generic_map {
            reg.set_generic(*g, fams.clone());
        }
        if !config.fallback.is_empty() {
            reg.set_fallback(config.fallback.clone());
        }
        reg.validate_effective_set()?;
        Ok(reg)
    }
}
