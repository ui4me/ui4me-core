//! owner: 007-resources (T059, T060) — font-source registry: registered blobs, font mode, the
//! embedded last-resort font and the runtime font epoch (007 FR-050–FR-054, FR-068).
//!
//! Host-supplied paths are trusted declarations (Clarification Q2): normalised, they must name an
//! existing regular file and are size-checked, but no document root applies.

use super::faces::FaceView;
use super::generic::{self, Platform};
use super::last_resort::{self, LastResortInfo};
use super::validate;
use crate::fs_guard;
use crate::policy::FontConfiguration;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use ui4me_engine_types::text_metrics::GenericFamily;

/// Runtime-wide font epoch (007 FR-053); bumped by every change to the source set.
/// Defined in `ui4me-core` (T003) and re-exported here.
pub use ui4me_engine_types::resource_ids::FontEpoch;

/// Identity of a registered font blob; stable for the runtime and never reused. 009 uses it as
/// the identity of the shared font bytes (`FontRef.blob`, R-55). Id 0 is the embedded last-resort font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontBlobId(pub u64);

/// Font mode (AD-28; 009 `font-matching.md` §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontMode {
    /// Only configured font files; system enumeration disabled (R-05).
    PinnedFontsOnly,
    /// System fonts per policy plus host-registered fonts.
    System,
}

/// Default size limit for host-registered font files (OD-19 font limit).
pub const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;

/// Shared, immutable font file bytes (`resource-view.md` §5 `FontBlobView`).
#[derive(Clone, Debug)]
pub struct FontBlob {
    pub id: FontBlobId,
    /// File name (no directory) for diagnostics; `<bytes>` names for in-memory blobs.
    pub name: String,
    pub bytes: Arc<[u8]>,
    pub family_override: Option<String>,
}

/// Immutable snapshot of the font sources for 009 (`resource-view.md` §5).
#[derive(Clone, Debug)]
pub struct FontSourcesView {
    pub font_epoch: FontEpoch,
    /// Whether system fonts may be enumerated; always `false` in pinned-fonts-only mode.
    pub system_fonts: bool,
    pub pinned_fonts_only: bool,
    /// Host-registered blobs in registration order (never includes the last-resort font).
    pub registered: Vec<FontBlob>,
    /// The embedded last-resort font, the final fallback after `fallback`; `None` when the flag is
    /// off or the checksum check failed.
    pub last_resort: Option<FontBlob>,
    /// Explicit generic mapping (family names). Pinned mode: as configured (a generic absent here
    /// maps to the registered families in registration order). System mode: platform defaults with
    /// the runtime overrides applied.
    pub generic_map: Vec<(GenericFamily, Vec<String>)>,
    /// Fallback family list; pinned mode: as configured (empty = the registered families in order).
    pub fallback: Vec<String>,
    /// `@font-face` faces of one document (empty unless built per document by the manager).
    pub faces: Vec<FaceView>,
}

/// `font_config_get` (`contracts/resource-policy.md` §1).
#[derive(Clone, Debug)]
pub struct FontConfigReport {
    pub system_fonts: bool,
    pub pinned_fonts_only: bool,
    pub registered: Vec<(FontBlobId, String, Option<String>, usize)>,
    pub last_resort: LastResortInfo,
    pub generic_map: Vec<(GenericFamily, Vec<String>)>,
    pub fallback: Vec<String>,
    pub font_epoch: FontEpoch,
    pub deterministic_resources: bool,
}

#[derive(Debug)]
pub struct FontRegistry {
    mode: FontMode,
    epoch: FontEpoch,
    next_id: u64,
    blobs: Vec<FontBlob>,
    generic_map: Vec<(GenericFamily, Vec<String>)>,
    fallback: Vec<String>,
    system_fonts: bool,
    /// Whether the platform has usable system faces; `None` = assume so when system fonts are on.
    system_fonts_available: Option<bool>,
    last_resort: Option<FontBlob>,
    last_resort_info: LastResortInfo,
}

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

/// Normalise a host-supplied path (absolute, lexically clean): no root check applies.
pub fn normalise_host_path(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl FontRegistry {
    /// An empty registry in `mode` with the embedded last-resort font enabled (the default).
    pub fn new(mode: FontMode) -> Self {
        Self::with_last_resort(mode, true)
    }

    pub fn with_last_resort(mode: FontMode, last_resort_fallback: bool) -> Self {
        let (blob, info) = last_resort::load(last_resort_fallback);
        let system = mode == FontMode::System;
        FontRegistry {
            mode,
            epoch: FontEpoch(0),
            next_id: 1,
            blobs: Vec::new(),
            generic_map: if system {
                generic::default_generic_map(Platform::current())
            } else {
                Vec::new()
            },
            fallback: if system {
                generic::default_fallback(Platform::current())
            } else {
                Vec::new()
            },
            system_fonts: system,
            system_fonts_available: None,
            last_resort: blob,
            last_resort_info: info,
        }
    }

    /// Build the runtime's registry from the runtime configuration (IF-33) and validate the
    /// effective font source set **once**: with no usable face the runtime cannot be created
    /// (`ENGINE_MISCONFIGURED`, FR-054). Document creation never fails for fonts.
    pub fn for_runtime(
        cfg: &FontConfiguration,
        system_fonts_policy: bool,
        system_fonts_available: Option<bool>,
    ) -> Result<FontRegistry, Status> {
        let mut reg = match &cfg.pinned {
            Some(p) => {
                let mut p = p.clone();
                p.last_resort_fallback = cfg.last_resort_fallback;
                FontRegistry::pinned(&p)?
            }
            None => {
                let mut r =
                    FontRegistry::with_last_resort(FontMode::System, cfg.last_resort_fallback);
                r.system_fonts = system_fonts_policy;
                r.system_fonts_available = system_fonts_available;
                r.generic_map =
                    generic::effective_generic_map(Platform::current(), &cfg.generic_map);
                if !cfg.fallback.is_empty() {
                    r.fallback = cfg.fallback.clone();
                }
                // Host-registered files first: the text layer registers them ahead of the system collection, so a
                // registered family shadows a system family of the same name (A18).
                for file in &cfg.files {
                    r.register_file(file)?;
                }
                r.validate_effective_set()?;
                r
            }
        };
        reg.epoch = FontEpoch(reg.epoch.0.max(1));
        Ok(reg)
    }

    /// FR-054: the effective set must contain a usable face.
    pub fn validate_effective_set(&self) -> Result<(), Status> {
        if self.has_usable_face() {
            Ok(())
        } else {
            Err(err(
                Code::EngineMisconfigured,
                "the effective font source set has no usable face (system fonts disabled or unavailable, no registered font, last-resort fallback off)",
            ))
        }
    }

    fn has_usable_face(&self) -> bool {
        (self.system_fonts && self.system_fonts_available.unwrap_or(true))
            || !self.blobs.is_empty()
            || self.last_resort.is_some()
    }

    pub fn mode(&self) -> FontMode {
        self.mode
    }

    pub fn epoch(&self) -> FontEpoch {
        self.epoch
    }

    fn bump(&mut self) {
        self.epoch = FontEpoch(self.epoch.0 + 1);
    }

    /// Bump the epoch once for a batch of per-document `@font-face` transitions (FR-053).
    pub fn bump_for_faces(&mut self) -> FontEpoch {
        self.bump();
        self.epoch
    }

    /// A fresh blob id for a face loaded from an `@font-face` source (runtime-unique, never reused).
    pub fn alloc_blob_id(&mut self) -> FontBlobId {
        let id = FontBlobId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn last_resort_info(&self) -> &LastResortInfo {
        &self.last_resort_info
    }

    /// Register a font file (synchronous owner-thread read at runtime creation, 007 FR-052). The
    /// path is a trusted host declaration: normalised, an existing regular file, size-checked.
    /// `FILE_NOT_FOUND`, `RESOURCE_DENIED` (not regular / unreadable), `SIZE_LIMIT`,
    /// `DECODE_FAILED` (invalid data), `UNSUPPORTED_MEDIA_TYPE` (WOFF, WOFF2, EOT, SVG). Nothing is
    /// registered on failure.
    pub fn register_file(&mut self, path: &Path) -> Result<FontBlobId, Status> {
        self.register_file_with_family(path, None)
    }

    pub fn register_file_with_family(
        &mut self,
        path: &Path,
        family_override: Option<String>,
    ) -> Result<FontBlobId, Status> {
        let norm = normalise_host_path(path);
        let shown = norm.display().to_string();
        let named = |s: Status| Status {
            message: format!("font file {shown}: {}", s.message),
            ..s
        };
        let vf = fs_guard::open_trusted(&norm, MAX_FONT_BYTES).map_err(named)?;
        let bytes = fs_guard::read_limited(&vf.file, MAX_FONT_BYTES).map_err(named)?;
        let name = norm
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(shown);
        self.register_bytes_with_family(name, Arc::from(bytes), family_override)
    }

    /// Register in-memory font bytes under a diagnostic name.
    pub fn register_bytes(
        &mut self,
        name: impl Into<String>,
        bytes: Arc<[u8]>,
    ) -> Result<FontBlobId, Status> {
        self.register_bytes_with_family(name, bytes, None)
    }

    pub fn register_bytes_with_family(
        &mut self,
        name: impl Into<String>,
        bytes: Arc<[u8]>,
        family_override: Option<String>,
    ) -> Result<FontBlobId, Status> {
        let name = name.into();
        if bytes.len() as u64 > MAX_FONT_BYTES {
            return Err(err(
                Code::SizeLimit,
                format!(
                    "font {name}: {} bytes exceeds the {MAX_FONT_BYTES} byte limit",
                    bytes.len()
                ),
            ));
        }
        validate::validate_font(&bytes).map_err(|s| Status {
            message: format!("font {name}: {}", s.message),
            ..s
        })?;
        let id = FontBlobId(self.next_id);
        self.next_id += 1;
        self.blobs.push(FontBlob {
            id,
            name,
            bytes,
            family_override,
        });
        self.bump();
        Ok(id)
    }

    /// Unregister a host font; idempotent for an unknown id. If it would leave the runtime with no
    /// usable face the call fails with `INVALID_STATE` and changes nothing (FR-054). The embedded
    /// last-resort font has no id and cannot be unregistered.
    pub fn unregister(&mut self, id: FontBlobId) -> Result<(), Status> {
        let Some(pos) = self.blobs.iter().position(|b| b.id == id) else {
            return Ok(());
        };
        let usable_without = (self.system_fonts && self.system_fonts_available.unwrap_or(true))
            || self.blobs.len() > 1
            || self.last_resort.is_some();
        if !usable_without {
            return Err(err(
                Code::InvalidState,
                "unregistering this font would leave the runtime with no usable face",
            ));
        }
        self.blobs.remove(pos);
        self.bump();
        Ok(())
    }

    /// Set the pinned mapping of one generic family (007 FR-050–FR-051).
    pub fn set_generic(&mut self, generic: GenericFamily, families: Vec<String>) {
        self.generic_map.retain(|(g, _)| *g != generic);
        self.generic_map.push((generic, families));
        self.generic_map.sort_by_key(|(g, _)| *g);
        self.bump();
    }

    /// Set the pinned fallback list.
    pub fn set_fallback(&mut self, families: Vec<String>) {
        self.fallback = families;
        self.bump();
    }

    pub fn blobs(&self) -> &[FontBlob] {
        &self.blobs
    }

    pub fn blob(&self, id: FontBlobId) -> Option<&FontBlob> {
        self.blobs.iter().find(|b| b.id == id)
    }

    /// The view for 009 (`resource-view.md` §5 `font_sources`). No system font is ever listed.
    pub fn font_sources(&self) -> FontSourcesView {
        FontSourcesView {
            font_epoch: self.epoch,
            system_fonts: self.system_fonts && self.mode != FontMode::PinnedFontsOnly,
            pinned_fonts_only: self.mode == FontMode::PinnedFontsOnly,
            registered: self.blobs.clone(),
            last_resort: self.last_resort.clone(),
            generic_map: self.generic_map.clone(),
            fallback: self.fallback.clone(),
            faces: Vec::new(),
        }
    }

    /// `font_config_get`.
    pub fn config_report(&self, deterministic_resources: bool) -> FontConfigReport {
        FontConfigReport {
            system_fonts: self.system_fonts && self.mode != FontMode::PinnedFontsOnly,
            pinned_fonts_only: self.mode == FontMode::PinnedFontsOnly,
            registered: self
                .blobs
                .iter()
                .map(|b| {
                    (
                        b.id,
                        b.name.clone(),
                        b.family_override.clone(),
                        b.bytes.len(),
                    )
                })
                .collect(),
            last_resort: self.last_resort_info.clone(),
            generic_map: self.generic_map.clone(),
            fallback: self.fallback.clone(),
            font_epoch: self.epoch,
            deterministic_resources,
        }
    }
}
