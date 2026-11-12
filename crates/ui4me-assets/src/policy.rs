//! owner: 007-resources (T009) — resource policy, OD-19 defaults, hard caps, runtime modes.
//!
//! `contracts/resource-policy.md` §2/§3. A document's policy is the runtime defaults merged with
//! the document's patch, then frozen. `network` is not a field: it is always denied.

use crate::uri::{ResourceKind, Scheme};
use std::path::{Path, PathBuf};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use ui4me_engine_types::text_metrics::GenericFamily;
use url::Url;

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * KIB;
pub const GIB: u64 = 1024 * MIB;

/// Fixed (not configurable) limits: `resource-policy.md` §2 last paragraph; AD-37.
pub const MAX_RASTER_SIDE: u32 = 65_535;
pub const SVG_MAX_NODES: u32 = 1_000_000;
pub const SVG_MAX_NESTING: u32 = 4;
pub const IMPORT_MAX_DEPTH: u32 = 16;

pub fn invalid_argument(msg: impl Into<String>) -> Status {
    Status::new(Code::InvalidArgument, Stage::Resource).with_message(msg)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaxEncodedBytes {
    pub image: u64,
    pub svg: u64,
    pub font: u64,
    pub stylesheet: u64,
}

impl MaxEncodedBytes {
    pub fn for_kind(&self, kind: ResourceKind) -> u64 {
        match kind {
            ResourceKind::Image => self.image,
            ResourceKind::Svg => self.svg,
            ResourceKind::Font => self.font,
            ResourceKind::Stylesheet => self.stylesheet,
        }
    }
}

/// Font policy of a document (`font_policy.*`). In pinned-fonts-only mode `system_fonts` is forced
/// to `false` by the runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontPolicy {
    pub system_fonts: bool,
    pub generic_map: Vec<(GenericFamily, Vec<String>)>,
    pub fallback: Vec<String>,
}

impl Default for FontPolicy {
    fn default() -> Self {
        FontPolicy {
            system_fonts: true,
            generic_map: Vec::new(),
            fallback: Vec::new(),
        }
    }
}

/// A patch over the runtime defaults; every field is optional. `network` is rejected by name by
/// the ABI layer (004); there is deliberately no field for it here.
#[derive(Clone, Debug, Default)]
pub struct ResourcePolicyPatch {
    pub file_roots: Option<Vec<PathBuf>>,
    pub implicit_document_root: Option<bool>,
    pub data_url_max_bytes: Option<u64>,
    pub max_encoded_image: Option<u64>,
    pub max_encoded_svg: Option<u64>,
    pub max_encoded_font: Option<u64>,
    pub max_encoded_stylesheet: Option<u64>,
    pub max_decoded_pixels: Option<u64>,
    pub decode_deadline_ms: Option<u64>,
    pub decoded_memory_budget: Option<u64>,
    pub cache_budget: Option<u64>,
    pub worker_threads: Option<u32>,
    pub system_fonts: Option<bool>,
    pub generic_map: Option<Vec<(GenericFamily, Vec<String>)>>,
    pub fallback: Option<Vec<String>>,
}

/// The effective policy (runtime defaults and a document patch). Roots are canonical.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourcePolicy {
    pub file_roots: Vec<PathBuf>,
    pub implicit_document_root: bool,
    pub data_url_max_bytes: u64,
    pub max_encoded_bytes: MaxEncodedBytes,
    pub max_decoded_pixels: u64,
    pub decode_deadline_ms: u64,
    pub decoded_memory_budget: u64,
    pub cache_budget: u64,
    pub worker_threads: u32,
    pub font_policy: FontPolicy,
    /// Whether `ui4me-res:` resolves against the host-registered bundles (OD-19: APPROVED, owner
    /// standing approval E-029, 2026-11-06; see `bundles.rs`). `false` makes it an unknown scheme
    /// (`RESOURCE_DENIED`), a switch for tests; it is not a host option.
    pub ui4me_res_scheme: bool,
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        ResourcePolicy {
            file_roots: Vec::new(),
            implicit_document_root: true,
            data_url_max_bytes: 4 * MIB,
            max_encoded_bytes: MaxEncodedBytes {
                image: 64 * MIB,
                svg: 16 * MIB,
                font: 32 * MIB,
                stylesheet: 16 * MIB,
            },
            max_decoded_pixels: 64_000_000,
            decode_deadline_ms: 10_000,
            decoded_memory_budget: 512 * MIB,
            cache_budget: 128 * MIB,
            worker_threads: 2,
            font_policy: FontPolicy::default(),
            ui4me_res_scheme: true,
        }
    }
}

fn range(name: &str, v: u64, lo: u64, hi: u64) -> Result<u64, Status> {
    if v < lo || v > hi {
        Err(invalid_argument(format!(
            "{name} = {v} is outside {lo}..={hi}"
        )))
    } else {
        Ok(v)
    }
}

/// Canonicalise a configured root: it must exist and be a directory (`INVALID_ARGUMENT` otherwise).
pub fn canonical_root(p: &Path) -> Result<PathBuf, Status> {
    let c = std::fs::canonicalize(p)
        .map_err(|e| invalid_argument(format!("file root {}: {e}", p.display())))?;
    if !c.is_dir() {
        return Err(invalid_argument(format!(
            "file root {} is not a directory",
            p.display()
        )));
    }
    Ok(strip_verbatim(c))
}

/// `std::fs::canonicalize` returns `\\?\` paths on Windows; the guard compares plain components.
pub(crate) fn strip_verbatim(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest.to_string());
        }
    }
    p
}

impl ResourcePolicy {
    /// Merge a patch over `self` (the runtime defaults), validating ranges and canonicalising roots.
    /// The result is the frozen document policy. Nothing is changed on error.
    pub fn merged(&self, patch: &ResourcePolicyPatch) -> Result<ResourcePolicy, Status> {
        let mut p = self.clone();
        if let Some(roots) = &patch.file_roots {
            p.file_roots = roots
                .iter()
                .map(|r| canonical_root(r))
                .collect::<Result<_, _>>()?;
        }
        if let Some(v) = patch.implicit_document_root {
            p.implicit_document_root = v;
        }
        if let Some(v) = patch.data_url_max_bytes {
            p.data_url_max_bytes = range("data_url_max_bytes", v, 0, 64 * MIB)?;
        }
        if let Some(v) = patch.max_encoded_image {
            p.max_encoded_bytes.image = range("max_encoded_bytes.image", v, KIB, GIB)?;
        }
        if let Some(v) = patch.max_encoded_svg {
            p.max_encoded_bytes.svg = range("max_encoded_bytes.svg", v, KIB, GIB)?;
        }
        if let Some(v) = patch.max_encoded_font {
            p.max_encoded_bytes.font = range("max_encoded_bytes.font", v, KIB, GIB)?;
        }
        if let Some(v) = patch.max_encoded_stylesheet {
            p.max_encoded_bytes.stylesheet = range("max_encoded_bytes.stylesheet", v, KIB, GIB)?;
        }
        if let Some(v) = patch.max_decoded_pixels {
            p.max_decoded_pixels = range("max_decoded_pixels", v, 1, 256_000_000)?;
        }
        if let Some(v) = patch.decode_deadline_ms {
            p.decode_deadline_ms = range("decode_deadline_ms", v, 1_000, 60_000)?;
        }
        if let Some(v) = patch.decoded_memory_budget {
            p.decoded_memory_budget = range("decoded_memory_budget", v, 16 * MIB, 4 * GIB)?;
        }
        if let Some(v) = patch.worker_threads {
            p.worker_threads = range("worker_threads", v as u64, 1, 8)? as u32;
        }
        if let Some(v) = patch.cache_budget {
            p.cache_budget = v;
        }
        if p.cache_budget > p.decoded_memory_budget {
            return Err(invalid_argument(format!(
                "cache_budget {} exceeds decoded_memory_budget {}",
                p.cache_budget, p.decoded_memory_budget
            )));
        }
        if let Some(v) = patch.system_fonts {
            p.font_policy.system_fonts = v;
        }
        if let Some(v) = &patch.generic_map {
            p.font_policy.generic_map = v.clone();
        }
        if let Some(v) = &patch.fallback {
            p.font_policy.fallback = v.clone();
        }
        Ok(p)
    }

    /// Add the implicit root of a document loaded from `document_file` (its directory subtree,
    /// never a parent; Clarification Q1). A string document passes `None` and gets none.
    pub fn with_document_file(
        &self,
        document_file: Option<&Path>,
    ) -> Result<ResourcePolicy, Status> {
        let mut p = self.clone();
        if let (true, Some(file)) = (p.implicit_document_root, document_file) {
            let canon = std::fs::canonicalize(file)
                .map_err(|e| invalid_argument(format!("document file {}: {e}", file.display())))?;
            let canon = strip_verbatim(canon);
            if let Some(dir) = canon.parent() {
                if !p.file_roots.iter().any(|r| r == dir) {
                    p.file_roots.push(dir.to_path_buf());
                }
            }
        }
        Ok(p)
    }

    /// Roots with the directory names redacted for diagnostics export (AD-35).
    pub fn roots_redacted(&self) -> Vec<String> {
        (0..self.file_roots.len())
            .map(|i| format!("<root-{i}>"))
            .collect()
    }
}

/// Font configuration given at runtime creation (IF-33; `contracts/resource-policy.md` §1 last row).
#[derive(Clone, Debug)]
pub struct FontConfiguration {
    /// Pinned-fonts-only mode: the configured files (trusted host declarations) and mappings.
    pub pinned: Option<crate::fonts::PinnedFontConfig>,
    /// Host-registered font files of the system mode (trusted declarations, registered **before** any
    /// system font is consulted, so a registered family always wins over a system family of the same name;
    /// A18). Ignored in pinned mode, where `pinned.files` is the source set.
    pub files: Vec<PathBuf>,
    /// Generic-family overrides for system mode (`font_policy.generic_map`).
    pub generic_map: Vec<(GenericFamily, Vec<String>)>,
    /// Fallback family list override for system mode (empty = the platform default list).
    pub fallback: Vec<String>,
    /// Embedded last-resort font (OD-24) as final fallback; default on.
    pub last_resort_fallback: bool,
}

impl Default for FontConfiguration {
    fn default() -> Self {
        FontConfiguration {
            pinned: None,
            files: Vec::new(),
            generic_map: Vec::new(),
            fallback: Vec::new(),
            last_resort_fallback: true,
        }
    }
}

/// Runtime resource modes, fixed at runtime creation (data-model `RuntimeResourceModes`).
#[derive(Clone, Debug, Default)]
pub struct RuntimeResourceModes {
    pub deterministic_resources: bool,
    pub fonts: FontConfiguration,
}

/// Outcome of the synchronous policy check (`policy::check`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Permit(Scheme),
    Deny(Code),
}

/// The synchronous, lexical part of the decision table (`resource-policy.md` §3): scheme
/// dispatch, lexical `file:` rejection and `data:` enablement. The rows that need the file system
/// (roots, regular file, size) are enforced by the guard when the file is opened.
pub fn check(url: &Url, policy: &ResourcePolicy) -> Decision {
    match Scheme::of(url) {
        Scheme::File => match crate::uri::file_url_to_path_checked(url) {
            Ok(_) => Decision::Permit(Scheme::File),
            Err(code) => Decision::Deny(code),
        },
        Scheme::Data if policy.data_url_max_bytes == 0 => Decision::Deny(Code::ResourceDenied),
        Scheme::Data => Decision::Permit(Scheme::Data),
        Scheme::UiRes if policy.ui4me_res_scheme => Decision::Permit(Scheme::UiRes),
        Scheme::UiRes | Scheme::Other => Decision::Deny(Code::ResourceDenied),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_od19() {
        let p = ResourcePolicy::default();
        assert_eq!(p.data_url_max_bytes, 4 * MIB);
        assert_eq!(
            p.max_encoded_bytes,
            MaxEncodedBytes {
                image: 64 * MIB,
                svg: 16 * MIB,
                font: 32 * MIB,
                stylesheet: 16 * MIB
            }
        );
        assert_eq!(p.max_decoded_pixels, 64_000_000);
        assert_eq!(p.decode_deadline_ms, 10_000);
        assert_eq!(p.decoded_memory_budget, 512 * MIB);
        assert_eq!(p.cache_budget, 128 * MIB);
        assert_eq!(p.worker_threads, 2);
        assert!(p.implicit_document_root && p.file_roots.is_empty() && p.font_policy.system_fonts);
        assert!(p.ui4me_res_scheme, "OD-19: ui4me-res: approved (E-029)");
    }

    #[test]
    fn out_of_range_values_are_invalid_argument() {
        let base = ResourcePolicy::default();
        let bad = [
            ResourcePolicyPatch {
                data_url_max_bytes: Some(65 * MIB),
                ..Default::default()
            },
            ResourcePolicyPatch {
                max_encoded_image: Some(10),
                ..Default::default()
            },
            ResourcePolicyPatch {
                max_encoded_font: Some(2 * GIB),
                ..Default::default()
            },
            ResourcePolicyPatch {
                max_decoded_pixels: Some(0),
                ..Default::default()
            },
            ResourcePolicyPatch {
                max_decoded_pixels: Some(257_000_000),
                ..Default::default()
            },
            ResourcePolicyPatch {
                decode_deadline_ms: Some(999),
                ..Default::default()
            },
            ResourcePolicyPatch {
                decode_deadline_ms: Some(60_001),
                ..Default::default()
            },
            ResourcePolicyPatch {
                decoded_memory_budget: Some(MIB),
                ..Default::default()
            },
            ResourcePolicyPatch {
                worker_threads: Some(0),
                ..Default::default()
            },
            ResourcePolicyPatch {
                worker_threads: Some(9),
                ..Default::default()
            },
            ResourcePolicyPatch {
                cache_budget: Some(600 * MIB),
                ..Default::default()
            },
            ResourcePolicyPatch {
                file_roots: Some(vec![PathBuf::from("/definitely/not/here")]),
                ..Default::default()
            },
        ];
        for patch in bad {
            let e = base.merged(&patch).expect_err(&format!("{patch:?}"));
            assert_eq!(e.code, Code::InvalidArgument);
        }
    }

    #[test]
    fn valid_patch_merges_and_zero_disables_data() {
        let base = ResourcePolicy::default();
        let p = base
            .merged(&ResourcePolicyPatch {
                data_url_max_bytes: Some(0),
                worker_threads: Some(4),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(p.data_url_max_bytes, 0);
        assert_eq!(p.worker_threads, 4);
        assert_eq!(
            base.data_url_max_bytes,
            4 * MIB,
            "runtime defaults are not modified"
        );
        let u = Url::parse("data:text/plain,hi").unwrap();
        assert_eq!(check(&u, &p), Decision::Deny(Code::ResourceDenied));
        assert_eq!(check(&u, &base), Decision::Permit(Scheme::Data));
    }

    #[test]
    fn unknown_and_network_schemes_are_denied() {
        let p = ResourcePolicy::default();
        for s in [
            "http://cdn/a.png",
            "https://x/a.png",
            "ftp://x/a",
            "javascript:alert(1)",
            "blob:x",
            "ws://x/",
        ] {
            assert_eq!(
                check(&Url::parse(s).unwrap(), &p),
                Decision::Deny(Code::ResourceDenied),
                "{s}"
            );
        }
        // OD-19 approved (E-029): ui4me-res: is permitted; the switch off restores the unknown-scheme denial.
        assert_eq!(
            check(&Url::parse("ui4me-res://app/a.png").unwrap(), &p),
            Decision::Permit(Scheme::UiRes)
        );
        let off = ResourcePolicy {
            ui4me_res_scheme: false,
            ..p
        };
        assert_eq!(
            check(&Url::parse("ui4me-res://app/a.png").unwrap(), &off),
            Decision::Deny(Code::ResourceDenied)
        );
    }
}
