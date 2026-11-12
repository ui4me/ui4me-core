//! owner: 009-text-inline — created by 001 slice (R-28)
//!
//! Font environment (009 `contracts/font-matching.md` §1) and the owner-thread `TextServices`
//! facade (009 `contracts/text-services.md` §1–§2). The Parley `FontContext` is built from 007's
//! `FontSourcesView`: in pinned-fonts-only mode with system fonts **off** and only the registered
//! blobs (R-05, AD-28); in system mode (A18, owner-granted) with the platform collection (fontconfig
//! on Linux, DirectWrite on Windows) behind the registered blobs, which are registered first and so
//! shadow a system family of the same name.

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{FontContext, LayoutContext};
use std::collections::BTreeSet;
use std::sync::Arc;
use ui4me_assets::fonts::generic::keyword_of;
use ui4me_assets::fonts::{FontBlob, FontBlobId, FontSourcesView};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::ErrorRecord;
use ui4me_engine_types::ids::DocumentId;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use ui4me_engine_types::text_metrics::{FamilyName, GenericFamily};

use super::faces::FaceTable;
use crate::text::metrics::MetricsService;
use crate::text::paragraph::source::SpanBrush;

/// Parley blob ids of 007's registered blobs carry this tag. Fontique numbers the blobs it loads
/// itself (system fonts) from a process-wide counter that starts at 0, which would collide with
/// 007's small ids; the tag keeps the two apart (see [`published_blob_id`]).
const REGISTERED_TAG: u64 = 1 << 63;
/// Published identity of a system-font blob: the Parley id under this tag (007 ids stay small).
const SYSTEM_TAG: u64 = 1 << 62;

/// The Parley blob id a registered 007 blob is given.
pub(crate) fn parley_blob_id(id: FontBlobId) -> u64 {
    REGISTERED_TAG | id.0
}

/// The identity published for a font blob (`FontRef.blob.id`, the display-list `FontRef`): a
/// registered blob keeps its 007 id; a system-font blob is `SYSTEM_TAG | its Parley id`, unique in
/// the process.
pub(crate) fn published_blob_id(parley_id: u64) -> u64 {
    if parley_id & REGISTERED_TAG != 0 {
        parley_id & !REGISTERED_TAG
    } else {
        SYSTEM_TAG | parley_id
    }
}

/// The runtime's font collection plus 007's pinned generic and fallback configuration.
#[derive(Clone)]
pub struct FontEnvironment {
    pub(crate) fcx: FontContext,
    epoch: u64,
    /// System fonts are enumerated (system mode and the platform collection loaded).
    system: bool,
    /// Per generic family, the families the platform collection maps it to (empty without system fonts).
    system_generics: Vec<(GenericFamily, Vec<String>)>,
    /// Bytes of the system-font files glyph runs have used, by published id (copied once per file).
    system_blobs: std::collections::HashMap<u64, FontBlob>,
    /// Registered family names, registration order, de-duplicated.
    families: Vec<String>,
    blobs: Vec<FontBlob>,
    generic_map: Vec<(GenericFamily, Vec<String>)>,
    fallback: Vec<String>,
    /// Internal family name of 007's embedded last-resort font (OD-24), appended to every
    /// resolved family list; `None` when 007 supplies none (flag off).
    last_resort_family: Option<String>,
    rejected: Vec<RejectedFace>,
    /// Some registered font has an `opsz` axis (`font-optical-sizing: auto` then sets it).
    opsz: bool,
    /// Families whose faces are all upright and that have a `slnt` axis (value: also an `ital`
    /// axis), by lower-cased collection name: Fontique synthesizes an oblique face with the wrong
    /// sign of `slnt`, so the text styles set it themselves (see `slant_axis_for`).
    slant_families: Vec<(String, bool)>,
    /// The document's `@font-face` faces (007's view; empty without a document).
    faces: Arc<FaceTable>,
    /// Faces the text asked for (`FaceId`s of 007), drained by [`TextServices::take_face_requests`].
    /// Shared with the metrics service's clone of the environment.
    face_requests: Arc<std::sync::Mutex<BTreeSet<u64>>>,
    /// System mode was requested but the platform collection could not be loaded (for example
    /// `libfontconfig` is missing): only registered fonts and the last-resort font are used.
    system_unavailable: bool,
}

/// Internal family name under which the last-resort bytes are registered, so that they never
/// merge with a registered family called "Noto Sans".
const LAST_RESORT_FAMILY: &str = "ui4me-last-resort";

/// A registered blob that Fontique could not decode (reported as `DECODE_FAILED`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedFace {
    pub name: String,
    pub code: Code,
}

impl std::fmt::Debug for FontEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontEnvironment")
            .field("epoch", &self.epoch)
            .field("families", &self.families)
            .field(
                "blobs",
                &self
                    .blobs
                    .iter()
                    .map(|b| (&b.id, &b.name))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl FontEnvironment {
    /// Build the collection from 007's view. With `system_fonts` the platform collection is loaded
    /// too (a platform without a usable one degrades to the registered fonts, reported by
    /// [`FontEnvironment::system_unavailable`]); the registered blobs are registered first. A view
    /// without a usable face is `ENGINE_MISCONFIGURED` (OD-24, 007 FR-054).
    pub fn new(sources: &FontSourcesView) -> Result<Self, Status> {
        let (mut collection, system_generics, system_unavailable) = if sources.system_fonts {
            // Loading the platform collection can fail hard when its library is missing (a dlopen'd
            // fontconfig): contain it and degrade to the registered fonts.
            let loaded = std::panic::catch_unwind(|| {
                let mut c = Collection::new(CollectionOptions {
                    shared: false,
                    system_fonts: true,
                });
                let generics = system_generics(&mut c);
                (c, generics)
            });
            match loaded {
                Ok((c, g)) => (c, g, false),
                Err(_) => (
                    Collection::new(CollectionOptions {
                        shared: false,
                        system_fonts: false,
                    }),
                    Vec::new(),
                    true,
                ),
            }
        } else {
            (
                Collection::new(CollectionOptions {
                    shared: false,
                    system_fonts: false,
                }),
                Vec::new(),
                false,
            )
        };
        let mut families = Vec::new();
        let mut rejected = Vec::new();
        for blob in &sources.registered {
            // The 007 blob id is the Parley blob id, so glyph runs map back to 007's bytes.
            let data = Blob::from_raw_parts(Arc::new(blob.bytes.clone()), parley_blob_id(blob.id));
            let over =
                blob.family_override
                    .as_deref()
                    .map(|name| parley::fontique::FontInfoOverride {
                        family_name: Some(name),
                        width: None,
                        style: None,
                        weight: None,
                        axes: None,
                    });
            let added = collection.register_fonts(data, over);
            if added.is_empty() {
                rejected.push(RejectedFace {
                    name: blob.name.clone(),
                    code: Code::DecodeFailed,
                });
            }
            // `register_fonts` returns a collection's families in an unspecified order; order them by
            // their lowest face index in the file so face 0 comes first on every run (a `.ttc`'s
            // reported family was otherwise nondeterministic: JP/TC/SC/KR across runs, L-T2W-r7).
            let mut in_file: Vec<(u32, String)> = added
                .iter()
                .filter_map(|(fid, fonts)| {
                    let name = collection.family_name(*fid)?.to_string();
                    Some((
                        fonts.iter().map(|f| f.index()).min().unwrap_or(u32::MAX),
                        name,
                    ))
                })
                .collect();
            in_file.sort();
            for (_, name) in in_file {
                if !families.contains(&name) {
                    families.push(name);
                }
            }
        }
        // 007's embedded last-resort font: final fallback in both modes while its flag is on.
        let mut blobs = sources.registered.clone();
        let mut last_resort_family = None;
        if let Some(lr) = &sources.last_resort {
            let data = Blob::from_raw_parts(Arc::new(lr.bytes.clone()), parley_blob_id(lr.id));
            let over = parley::fontique::FontInfoOverride {
                family_name: Some(LAST_RESORT_FAMILY),
                width: None,
                style: None,
                weight: None,
                axes: None,
            };
            if collection.register_fonts(data, Some(over)).is_empty() {
                rejected.push(RejectedFace {
                    name: lr.name.clone(),
                    code: Code::DecodeFailed,
                });
            } else {
                last_resort_family = Some(LAST_RESORT_FAMILY.to_string());
                blobs.push(lr.clone());
            }
        }
        // The document's `@font-face` faces: loaded ones are registered under their group's internal family
        // name with the rule's descriptors as the face attributes; later-declared faces first so that a
        // later rule wins a tie. A `local()` alias registers nothing (its family is already there).
        let faces = FaceTable::build(&sources.faces);
        for (face, group, blob) in faces.loaded() {
            let data = Blob::from_raw_parts(Arc::new(blob.bytes.clone()), parley_blob_id(blob.id));
            let (weight, style, width) = face_attributes(face, &blob.bytes);
            let over = parley::fontique::FontInfoOverride {
                family_name: Some(group.name.as_str()),
                width,
                style,
                weight,
                axes: None,
            };
            if collection.register_fonts(data, Some(over)).is_empty() {
                rejected.push(RejectedFace {
                    name: blob.name.clone(),
                    code: Code::DecodeFailed,
                });
            } else if !blobs.iter().any(|b| b.id == blob.id) {
                blobs.push(blob.clone());
            }
        }
        if families.is_empty() && last_resort_family.is_none() {
            return Err(Status::new(Code::EngineMisconfigured, Stage::Text)
                .with_message("no usable font face in the registered font sources"));
        }
        let mut slant_families = Vec::new();
        let mut names = families.clone();
        names.extend(faces.loaded().into_iter().map(|(_, g, _)| g.name.clone()));
        names.sort();
        names.dedup();
        for n in names {
            if let Some(fam) = collection.family_by_name(&n) {
                let fonts = fam.fonts();
                let upright = fonts
                    .iter()
                    .all(|f| f.style() == parley::fontique::FontStyle::Normal);
                let slnt = fonts.iter().any(|f| f.has_slant_axis());
                let ital = fonts.iter().any(|f| f.has_italic_axis());
                if upright && slnt {
                    slant_families.push((n.to_ascii_lowercase(), ital));
                }
            }
        }
        let opsz = blobs.iter().any(|b| {
            super::features::font_axes(&b.bytes)
                .iter()
                .any(|a| a == b"opsz")
        });
        Ok(FontEnvironment {
            fcx: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            epoch: sources.font_epoch.0,
            system: sources.system_fonts && !system_unavailable,
            system_generics,
            system_blobs: Default::default(),
            families,
            blobs,
            generic_map: sources.generic_map.clone(),
            fallback: sources.fallback.clone(),
            last_resort_family,
            rejected,
            opsz,
            slant_families,
            faces: Arc::new(faces),
            face_requests: Default::default(),
            system_unavailable,
        })
    }

    /// Whether the platform font collection is part of this environment.
    pub fn system_fonts(&self) -> bool {
        self.system
    }

    /// System mode was requested but the platform collection could not be loaded.
    pub fn system_unavailable(&self) -> bool {
        self.system_unavailable
    }

    /// The font blobs a layout's glyph runs use, as 007-shaped [`FontBlob`]s: the registered blobs
    /// (all of them) plus every system-font file a run uses (bytes copied once per file and cached).
    pub(crate) fn blobs_for_layout<B: parley::Brush>(
        &mut self,
        layout: &parley::Layout<B>,
    ) -> Vec<FontBlob> {
        let mut out = self.blobs.clone();
        if !self.system {
            return out;
        }
        for line in layout.lines() {
            for item in line.items() {
                let parley::PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let data = &gr.run().font().data;
                if data.id() & REGISTERED_TAG != 0 {
                    continue;
                }
                let id = published_blob_id(data.id());
                if out.iter().any(|b| b.id.0 == id) {
                    continue;
                }
                let blob = self
                    .system_blobs
                    .entry(id)
                    .or_insert_with(|| FontBlob {
                        id: FontBlobId(id),
                        name: "<system font>".to_string(),
                        bytes: Arc::from(data.as_ref()),
                        family_override: None,
                    })
                    .clone();
                out.push(blob);
            }
        }
        out
    }

    pub fn font_epoch(&self) -> u64 {
        self.epoch
    }

    /// The `slnt` axis value `style` asks of the first usable family of `names` when it is an upright
    /// variable face with a `slnt` axis (CSS Fonts 4 section 2.3: `oblique <angle>` sets `slnt` to the
    /// negated angle, `italic` without an `ital` axis to -14): `None` when Fontique's own
    /// synthesis is right or no variable face applies.
    pub fn slant_axis_for(
        &self,
        names: &[String],
        style: ui4me_engine_types::text_metrics::FontStyleSpec,
    ) -> Option<f32> {
        use ui4me_engine_types::text_metrics::FontStyleSpec as S;
        if self.slant_families.is_empty() {
            return None;
        }
        let (_, ital) = names.iter().find_map(|n| {
            self.slant_families
                .iter()
                .find(|(f, _)| f.eq_ignore_ascii_case(n))
        })?;
        match style {
            S::Normal => None,
            S::Oblique(a) => Some(-a),
            S::Italic if !*ital => Some(-14.0),
            S::Italic => None,
        }
    }

    /// Whether `font-optical-sizing: auto` has a font to act on (see the field).
    pub fn opsz_available(&self) -> bool {
        self.opsz
    }

    /// Internal family name of the last-resort font, if 007 supplied it.
    pub fn last_resort_family(&self) -> Option<String> {
        self.last_resort_family.clone()
    }

    /// Faces Fontique refused to decode (excluded from the collection).
    pub fn rejected(&self) -> &[RejectedFace] {
        &self.rejected
    }

    /// Every family name in the underlying collection (registered files and the last-resort
    /// font; never system fonts in pinned mode).
    pub fn collection_families(&mut self) -> Vec<String> {
        self.fcx
            .collection
            .family_names()
            .map(str::to_string)
            .collect()
    }

    /// Every family the collection can match (only registered families; no system fonts).
    pub fn family_names(&self) -> &[String] {
        &self.families
    }

    /// 007's registered blobs (shared bytes).
    pub fn blobs(&self) -> &[FontBlob] {
        &self.blobs
    }

    pub fn blob(&self, id: u64) -> Option<&FontBlob> {
        self.blobs.iter().find(|b| b.id.0 == id)
    }

    /// Concrete family names for a computed `font-family` list (009 `font-matching.md` §2
    /// step 1 and step 4): generics through 007's pinned map (default: the registered families
    /// in order), named families as given, then the pinned fallback list (default: registered
    /// families). Duplicates are removed, first occurrence wins. Unknown names stay in the list
    /// and are skipped by matching (not an error).
    pub fn resolve_families(&self, families: &[FamilyName]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut seen = BTreeSet::new();
        // A name the document declares with `@font-face` stands for the collection families of its
        // groups (later-declared first); while none is usable it stands for nothing (the declared
        // family shadows an installed one of the same name).
        let faces = &self.faces;
        let mut push = |name: &str, out: &mut Vec<String>| match faces.names_for(name) {
            Some(groups) => {
                for g in groups {
                    if seen.insert(g.name.to_ascii_lowercase()) {
                        out.push(g.name.clone());
                    }
                }
            }
            None => {
                if seen.insert(name.to_ascii_lowercase()) {
                    out.push(name.to_string());
                }
            }
        };
        for f in families {
            match f {
                FamilyName::Named(n) => push(n, &mut out),
                FamilyName::Generic(g) => {
                    match self.generic_map.iter().find(|(k, _)| k == g) {
                        Some((_, names)) => names.iter().for_each(|n| push(n, &mut out)),
                        None => self.families.iter().for_each(|n| push(n, &mut out)),
                    }
                    // System mode: 007's names come first (a registered or well-known family wins), then the
                    // platform's own mapping of the generic (fontconfig / DirectWrite defaults).
                    if let Some((_, names)) = self.system_generics.iter().find(|(k, _)| k == g) {
                        names.iter().for_each(|n| push(n, &mut out));
                    }
                }
            }
        }
        if self.fallback.is_empty() {
            self.families.iter().for_each(|n| push(n, &mut out));
        } else {
            self.fallback.iter().for_each(|n| push(n, &mut out));
        }
        // The last-resort font is always last (OD-24).
        if let Some(lr) = &self.last_resort_family {
            push(lr, &mut out);
        }
        out
    }

    /// The document's `@font-face` face table.
    pub fn face_table(&self) -> &FaceTable {
        &self.faces
    }

    /// Note the declared faces `spec` needs (lazy loading, CSS Fonts 4 section 4.8): the faces that win
    /// the descriptor matching for the family list, plus (when `text` is given) the `unicode-range`
    /// faces whose range holds a character of it. 007 starts the loads when the pipeline hands the
    /// ids over ([`TextServices::take_face_requests`]).
    pub fn note_face_use(
        &self,
        spec: &ui4me_engine_types::text_metrics::FontSpec,
        text: Option<&str>,
    ) {
        if self.faces.is_empty() {
            return;
        }
        let needed = self.faces.needed_faces(spec, text);
        if needed.is_empty() {
            return;
        }
        if let Ok(mut set) = self.face_requests.lock() {
            set.extend(needed.into_iter().map(|f| f.0));
        }
    }

    /// Pieces of `text` (byte ranges relative to `text`) whose characters need a different family
    /// list than `resolved` because a `unicode-range` group is in it; each piece carries its list.
    /// Empty when no group of `resolved` has a range (the common case: nothing to split).
    pub fn split_for_unicode_range(
        &self,
        resolved: &[String],
        text: &str,
    ) -> Vec<(std::ops::Range<usize>, Vec<String>)> {
        if !self.faces.has_ranged_groups(resolved) {
            return Vec::new();
        }
        let mut out: Vec<(std::ops::Range<usize>, Vec<String>)> = Vec::new();
        for (i, c) in text.char_indices() {
            let list = self.faces.effective_list(resolved, c);
            let end = i + c.len_utf8();
            match out.last_mut() {
                Some((r, l)) if *l == list => r.end = end,
                _ => out.push((i..end, list)),
            }
        }
        // keep only the pieces that differ from the full list
        out.retain(|(_, l)| l.as_slice() != resolved);
        out
    }
}

type FaceAttributes = (
    Option<parley::fontique::FontWeight>,
    Option<parley::fontique::FontStyle>,
    Option<parley::fontique::FontWidth>,
);

/// The Fontique attributes a face's descriptors give: a single value is set; a range is left to the
/// font's own variation axis when it has one for it (Fontique then follows the requested value), and
/// otherwise stands for its lower bound.
fn face_attributes(face: &super::faces::FaceRecord, bytes: &[u8]) -> FaceAttributes {
    let axes = font_axes(bytes);
    let has = |tag: &[u8; 4]| axes.iter().any(|a| a == tag);
    let weight = if face.weight.0 == face.weight.1 || !has(b"wght") {
        Some(parley::fontique::FontWeight::new(
            face.weight.0.min(face.weight.1),
        ))
    } else {
        None
    };
    let width = if face.width.0 == face.width.1 || !has(b"wdth") {
        Some(parley::fontique::FontWidth::from_percentage(
            face.width.0.min(face.width.1),
        ))
    } else {
        None
    };
    let style = match face.style {
        ui4me_assets::fonts::FaceStyle::Normal => Some(parley::fontique::FontStyle::Normal),
        ui4me_assets::fonts::FaceStyle::Italic => Some(parley::fontique::FontStyle::Italic),
        ui4me_assets::fonts::FaceStyle::Oblique { min_deg, max_deg } => {
            if min_deg == max_deg || !has(b"slnt") {
                Some(parley::fontique::FontStyle::Oblique(Some(
                    min_deg.min(max_deg),
                )))
            } else {
                None
            }
        }
    };
    (weight, style, width)
}

/// The variation axis tags of a font file (`fvar`), first face of a collection.
fn font_axes(data: &[u8]) -> Vec<[u8; 4]> {
    super::features::font_axes(data)
}

/// Whether the platform font collection has an installed family called `name` (case-insensitive;
/// Fontique matches the family name, not the full or PostScript name of a face). Backs `local()` in
/// `@font-face` (system mode only: 007 asks it when no registered font matches). The platform
/// collection is enumerated once per process, on the first question; a platform without one answers no.
pub fn system_family_exists(name: &str) -> bool {
    use std::sync::{Mutex, OnceLock};
    static SYSTEM: OnceLock<Mutex<Option<Collection>>> = OnceLock::new();
    let cell = SYSTEM.get_or_init(|| {
        Mutex::new(
            std::panic::catch_unwind(|| {
                Collection::new(CollectionOptions {
                    shared: false,
                    system_fonts: true,
                })
            })
            .ok(),
        )
    });
    let Ok(mut guard) = cell.lock() else {
        return false;
    };
    guard.as_mut().is_some_and(|c| c.family_id(name).is_some())
}

/// The platform collection's mapping of each CSS generic family to family names (fontconfig's
/// default `serif` / `sans-serif` / `monospace`, DirectWrite's equivalents), loaded eagerly so
/// `resolve_families` stays a read-only query.
fn system_generics(collection: &mut Collection) -> Vec<(GenericFamily, Vec<String>)> {
    GenericFamily::ALL
        .iter()
        .map(|g| {
            let names = match parley::fontique::GenericFamily::parse(keyword_of(*g)) {
                Some(fq) => {
                    let ids: Vec<_> = collection.generic_families(fq).collect();
                    ids.into_iter()
                        .filter_map(|id| collection.family_name(id).map(str::to_string))
                        .collect()
                }
                None => Vec::new(),
            };
            (*g, names)
        })
        .collect()
}

/// The recoverable diagnostic for a system-font request the platform could not serve.
fn system_unavailable_record() -> ErrorRecord {
    ErrorRecord {
        document: None,
        status: Status::new(Code::ResourcePending, Stage::Text)
            .with_message("system fonts requested but the platform font collection could not be loaded (is fontconfig installed?); using the registered and bundled fonts only"),
    }
}

/// Owner-thread text facade (009 `text-services.md` §1). Created by the runtime from 007's
/// initial font sources.
pub struct TextServices {
    pub(crate) env: FontEnvironment,
    pub(crate) lcx: LayoutContext<SpanBrush>,
    /// Documents that already received their missing-glyph report (FR-025: once per document).
    pub(crate) missing_glyph_reported: BTreeSet<(u32, u32)>,
    pub(crate) diagnostics: Vec<ErrorRecord>,
    /// Text stage diagnostics (size limits, ...), de-duplicated per document revision.
    pub text_diagnostics: crate::text::diagnostics::TextDiagnostics,
    /// The shared metrics service handed out by `metrics_service()`; rebuilt in place by
    /// `apply_font_epoch` so holders keep a valid handle.
    pub(crate) metrics: std::sync::OnceLock<Arc<MetricsService>>,
    pub(crate) shaped_cache: crate::text::paragraph::shaped::ShapedCache,
    /// Content-addressed shaping reuse across layout passes (`TextServices::shape`).
    pub(crate) content_cache: crate::text::paragraph::shaped::ContentCache,
    /// Count of diagnostics `shape_uncached` raised (a result produced while it moved is not cached).
    pub(crate) shape_diag_events: u64,
    /// Subpixel glyph positioning: Windows at every scale, Linux above device scale 1.
    /// Set by layout from the viewport scale (`paragraph/snap.rs`).
    pub(crate) subpixel: bool,
    pub(crate) epochs: crate::text::paragraph::shaped::LayoutEpochs,
    /// Metrics of decorating fonts, by (font epoch, the font style's debug form): probing shapes a
    /// character, so it is done once per font and epoch (`paragraph/decoration.rs`).
    pub(crate) deco_font_cache: std::collections::HashMap<
        crate::text::paragraph::decoration::DecorationFontKey,
        crate::text::paragraph::decoration::DecoFont,
    >,
    /// The thread that created the facade: the only thread the `try_*` operations accept
    /// (text-services.md section 1).
    owner: std::thread::ThreadId,
}

impl std::fmt::Debug for TextServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextServices")
            .field("env", &self.env)
            .field("diagnostics", &self.diagnostics)
            .finish()
    }
}

impl TextServices {
    pub fn new(sources: &FontSourcesView) -> Result<Self, Status> {
        let env = FontEnvironment::new(sources)?;
        let mut diagnostics: Vec<ErrorRecord> = env
            .rejected()
            .iter()
            .map(|r| ErrorRecord {
                document: None,
                status: Status::new(Code::DecodeFailed, Stage::Text).with_message(format!(
                    "font face `{}` could not be decoded and was excluded",
                    r.name
                )),
            })
            .collect();
        if env.system_unavailable() {
            diagnostics.push(system_unavailable_record());
        }
        Ok(TextServices {
            env,
            lcx: LayoutContext::new(),
            missing_glyph_reported: BTreeSet::new(),
            diagnostics,
            text_diagnostics: Default::default(),
            metrics: std::sync::OnceLock::new(),
            shaped_cache: Default::default(),
            content_cache: Default::default(),
            shape_diag_events: 0,
            subpixel: cfg!(target_os = "windows"),
            epochs: Default::default(),
            deco_font_cache: Default::default(),
            owner: std::thread::current().id(),
        })
    }

    /// Select glyph positioning for the platform and device scale. A change drops shaped-paragraph
    /// caches, whose entries carry the old advances.
    pub fn set_device_scale(&mut self, scale: f32) {
        let subpixel = cfg!(target_os = "windows") || scale > 1.0;
        if subpixel != self.subpixel {
            self.subpixel = subpixel;
            self.shaped_cache = Default::default();
            self.content_cache.clear();
        }
        // Called once per layout pass: age the content cache.
        self.content_cache.next_pass(self.env.font_epoch());
    }

    /// `WRONG_THREAD` unless called on the thread that created the facade (text-services.md
    /// section 1: `TextServices` is owner-thread only). The `try_*` operations check it; the
    /// infallible operations are for callers that already run on the owner thread (layout).
    pub fn check_owner_thread(&self) -> Result<(), Status> {
        if std::thread::current().id() == self.owner {
            Ok(())
        } else {
            Err(Status::new(Code::WrongThread, Stage::Text)
                .with_message("TextServices is owner-thread only"))
        }
    }

    /// `shape` with the owner-thread check.
    pub fn try_shape(
        &mut self,
        source: &crate::text::paragraph::source::ParagraphSource,
    ) -> Result<Arc<crate::text::paragraph::source::ShapedParagraph>, Status> {
        self.check_owner_thread()?;
        Ok(self.shape(source))
    }

    /// `shape_keyed` with the owner-thread check.
    pub fn try_shape_keyed(
        &mut self,
        key: crate::text::paragraph::source::ParagraphKey,
        source: &crate::text::paragraph::source::ParagraphSource,
    ) -> Result<Arc<crate::text::paragraph::source::ShapedParagraph>, Status> {
        self.check_owner_thread()?;
        Ok(self.shape_keyed(key, source))
    }

    /// Replace the document's declared-face table without building a new collection. Valid when no
    /// face became usable (loaded or aliased) since the collection was built: the declared names then
    /// still shadow installed families and the faces to load are known. Drops the shaped-paragraph
    /// cache and the metrics memo, whose entries were made under the old table.
    pub fn set_declared_faces(&mut self, faces: &[ui4me_assets::fonts::FaceView]) {
        self.env.faces = Arc::new(FaceTable::build(faces));
        self.shaped_cache = Default::default();
        self.content_cache.clear();
        self.deco_font_cache.clear();
        if let Some(m) = self.metrics.get() {
            m.apply_environment(self.env.clone());
        }
    }

    /// The `@font-face` faces the text has asked for since the last call (007 `FaceId`s; ask 007 to
    /// load each with `request_face`, which is idempotent).
    pub fn take_face_requests(&self) -> Vec<ui4me_assets::fonts::FaceId> {
        match self.env.face_requests.lock() {
            Ok(mut set) => std::mem::take(&mut *set)
                .into_iter()
                .map(ui4me_assets::fonts::FaceId)
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn environment(&self) -> &FontEnvironment {
        &self.env
    }

    /// Current runtime-wide font epoch (`font` input of 005's revision tuple).
    pub fn font_epoch(&self) -> u64 {
        self.env.font_epoch()
    }

    /// The thread-safe metrics service over a clone of this collection (AD-08). One shared
    /// instance per `TextServices`; `apply_font_epoch` updates it in place.
    pub fn metrics_service(&self) -> Arc<MetricsService> {
        self.metrics
            .get_or_init(|| Arc::new(MetricsService::new(self.env.clone())))
            .clone()
    }

    /// Generic base size (FR-075): 16 px, monospace 13 px.
    pub fn generic_base_size(&self, generic: Option<GenericFamily>) -> f32 {
        super::generics::base_size(generic)
    }

    /// Apply a new font epoch from 007 (`text-services.md` §2): rebuild the collection from the
    /// view (registered files, last-resort bytes, generic and fallback maps), clear the metrics
    /// memo and make every shaped paragraph of an older epoch unusable (`is_current`). A view
    /// that cannot produce a usable collection is `ENGINE_MISCONFIGURED` and leaves the current
    /// environment untouched. The affected-document computation is T122.
    pub fn apply_font_epoch(&mut self, sources: &FontSourcesView) -> Result<u64, Status> {
        let env = FontEnvironment::new(sources)?;
        for r in env.rejected() {
            self.diagnostics.push(ErrorRecord {
                document: None,
                status: Status::new(Code::DecodeFailed, Stage::Text).with_message(format!(
                    "font face `{}` could not be decoded and was excluded",
                    r.name
                )),
            });
        }
        if env.system_unavailable() {
            self.diagnostics.push(system_unavailable_record());
        }
        if let Some(m) = self.metrics.get() {
            m.apply_environment(env.clone());
        }
        self.env = env;
        self.shaped_cache.retain_font_epoch(self.env.font_epoch());
        self.content_cache.clear();
        self.deco_font_cache.clear();
        Ok(self.env.font_epoch())
    }

    /// Whether a shaped paragraph was produced under the current font epoch; older ones must be
    /// reshaped, never reused (FR-084, FR-090).
    pub fn is_current(&self, shaped: &crate::text::paragraph::source::ShapedParagraph) -> bool {
        shaped.font_epoch() == self.env.font_epoch()
    }

    /// Diagnostics produced since the last call (stage `text`), in production order.
    pub fn take_diagnostics(&mut self) -> Vec<ErrorRecord> {
        std::mem::take(&mut self.diagnostics)
    }

    pub(crate) fn note_missing_glyph(&mut self, document: DocumentId, ch: char) {
        let raw = document.raw();
        if self
            .missing_glyph_reported
            .insert((raw.index, raw.generation.get()))
        {
            let status = Status::new(Code::ResourcePending, Stage::Text).with_message(format!(
                "missing glyph: U+{:04X} has no glyph in the available fonts; .notdef drawn",
                ch as u32
            ));
            self.diagnostics.push(ErrorRecord {
                document: Some(document),
                status,
            });
        }
    }
}
