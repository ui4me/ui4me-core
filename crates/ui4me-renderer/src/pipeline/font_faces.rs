//! owner: 005-pipeline-diagnostics — `@font-face` wiring (L-FONTFACE-r12; 006 T029, 007 T063/T064,
//! 009 T057/T060, 005 `FONT_EPOCH`).
//!
//! The flow, per document:
//!
//! 1. **Export.** After each style pass the effective `@font-face` rules are read from the stylist
//!    (`StyleBridge::font_face_rules`) and, when they differ from the last export, handed to 007
//!    (`sync_font_faces`). 007 keeps a face (and its load state) for every rule that is unchanged and
//!    returns the document's [`FaceView`]s; the text stage's face table is replaced in place (no new
//!    collection: nothing new is loaded yet).
//! 2. **Request.** Shaping and the metrics service note the faces the text needs; at the end of the
//!    update the pipeline asks 007 to load them (`request_face`, non-blocking, idempotent).
//! 3. **Complete.** When loads finish, 007 reports one batch per drain step with `font_epoch_changed`;
//!    the runtime hands the document's new face views to [`DocumentPipeline::apply_font_faces`]: the
//!    document's font epoch advances **once for the batch** and the next update rebuilds the font
//!    context over the loaded faces and relays out (`FONT_EPOCH`), exactly like one image batch.
//!
//! Descriptors the engine parses but does not honour (`size-adjust`, the metric overrides,
//! `font-language-override`) are reported
//! `UNSUPPORTED_FEATURE` (`css.at-rule.font-face.<descriptor>`); the rule's line and column are
//! those within its sheet (the rule does not carry its sheet's number, so the sheet field is 0).

use crate::pipeline::state::DocumentPipeline;
use crate::style::font_faces::{FontFaceExport, FontSourceExport, FontStyleExport};
use ui4me_assets::fonts::{FaceSourceSpec, FaceStyle, FaceView, FontFaceRule};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{
    FeatureId, ReportKey, ReportLocation, SheetOrigin, UnsupportedReason,
};
use ui4me_engine_types::resource_ids::FontEpoch;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// Whether an installed font family of that name exists (009's platform collection): the lookup 007
/// uses for `local()` sources in system mode.
pub use crate::text::fonts::system_family_exists;

/// A `descriptor` of an `@font-face` rule the engine does not honour.
fn unsupported_feature(descriptor: &str) -> Option<FeatureId> {
    FeatureId::new(match descriptor {
        "size-adjust" => "css.at-rule.font-face.size-adjust",
        "ascent-override" => "css.at-rule.font-face.ascent-override",
        "descent-override" => "css.at-rule.font-face.descent-override",
        "line-gap-override" => "css.at-rule.font-face.line-gap-override",
        "font-feature-settings" => "css.at-rule.font-face.font-feature-settings",
        "font-variation-settings" => "css.at-rule.font-face.font-variation-settings",
        "font-language-override" => "css.at-rule.font-face.font-language-override",
        _ => return None,
    })
}

/// 006's export as 007's rule (the URLs are already absolute: they were resolved against the declaring
/// sheet, so the base 007 receives is never consulted).
pub(crate) fn to_rule(e: &FontFaceExport) -> FontFaceRule {
    let none = ui4me_assets::Url::parse("about:blank").expect("static URL");
    FontFaceRule {
        rule_id: 0,
        family: e.family.clone(),
        settings: e.settings.clone(),
        weight: e.weight,
        style: match e.style {
            FontStyleExport::Normal => FaceStyle::Normal,
            FontStyleExport::Italic => FaceStyle::Italic,
            FontStyleExport::Oblique { min_deg, max_deg } => {
                FaceStyle::Oblique { min_deg, max_deg }
            }
        },
        width: e.width,
        unicode_range: e.unicode_range.clone(),
        sources: e
            .sources
            .iter()
            .map(|s| match s {
                FontSourceExport::Url {
                    url,
                    reference,
                    format,
                    ..
                } => FaceSourceSpec::Url {
                    reference: url
                        .as_ref()
                        .map_or_else(|| reference.clone(), |u| u.as_str().to_string()),
                    base: none.clone(),
                    format: format.clone(),
                },
                FontSourceExport::Local(n) => FaceSourceSpec::Local(n.clone()),
            })
            .collect(),
        display: e.display.map(str::to_string),
    }
}

impl DocumentPipeline {
    /// Step 1: export the effective `@font-face` rules to 007 when they changed since the last export
    /// and replace the text stage's face table. Called after each style pass.
    pub(crate) fn sync_font_faces(&mut self, dom_revision: u64) {
        let rules = self.style.font_face_rules();
        if rules == self.exported_faces {
            return;
        }
        for r in &rules {
            for d in &r.unsupported {
                let Some(feature) = unsupported_feature(d) else {
                    continue;
                };
                let key = ReportKey {
                    feature,
                    stage: Stage::Cascade,
                    document: self.doc.id(),
                    location: ReportLocation::Stylesheet {
                        sheet: 0,
                        origin: SheetOrigin::Author,
                        line: r.line,
                        column: r.column,
                    },
                };
                self.stores.unsupported.record(
                    key,
                    Code::UnsupportedFeature,
                    UnsupportedReason::NotConsumedByStage,
                    dom_revision,
                );
            }
        }
        self.exported_faces = rules;
        let Some(res) = self.resources.clone() else {
            // Without a resource view there is nothing to load a face with: say so, never drop the rule silently.
            if let Some(feature) = FeatureId::new("css.at-rule.font-face") {
                for r in &self.exported_faces {
                    let key = ReportKey {
                        feature,
                        stage: Stage::Cascade,
                        document: self.doc.id(),
                        location: ReportLocation::Stylesheet {
                            sheet: 0,
                            origin: SheetOrigin::Author,
                            line: r.line,
                            column: r.column,
                        },
                    };
                    self.stores.unsupported.record(
                        key,
                        Code::UnsupportedFeature,
                        UnsupportedReason::ResourceUnsupported,
                        dom_revision,
                    );
                }
            }
            return;
        };
        let mapped: Vec<FontFaceRule> = self.exported_faces.iter().map(to_rule).collect();
        let Ok(views) = res.sync_font_faces(&mapped) else {
            return;
        };
        // Faces that are already usable but not in the current font context (kept across a rebuild, or
        // loaded between two updates) need a rebuild; everything else is a table swap.
        let usable = |v: &[FaceView]| {
            v.iter()
                .filter(|f| {
                    matches!(
                        f.state,
                        ui4me_assets::fonts::FaceState::Loaded(_)
                            | ui4me_assets::fonts::FaceState::LocalFamily(_)
                    )
                })
                .map(|f| f.id)
                .collect::<Vec<_>>()
        };
        let needs_rebuild = usable(&views) != usable(&self.fonts.faces);
        self.fonts.faces = views;
        self.font_gen += 1;
        self.text.set_declared_faces(&self.fonts.faces);
        // The font context of this update is already built: the rebuild (and its font-epoch step, an input
        // of the update in flight) follows as the next update (`finish_font_rebuild`).
        self.font_rebuild_pending |= needs_rebuild;
    }

    /// After a publication: a face that became usable (or left) during the update needs a new font
    /// context. Advances the font epoch once and notes `FONT_EPOCH` so that the next update rebuilds.
    pub(crate) fn finish_font_rebuild(&mut self) {
        if std::mem::take(&mut self.font_rebuild_pending) {
            self.fonts.font_epoch = FontEpoch(self.fonts.font_epoch.0 + 1);
            let families = self.fonts.faces.iter().map(|f| f.family.clone()).collect();
            let _ = self.note_font_epoch_changed(families);
        }
    }

    /// Step 2: ask 007 to load the faces the text asked for during this update.
    pub(crate) fn request_text_faces(&mut self) {
        let ids = self.text.take_face_requests();
        if ids.is_empty() {
            return;
        }
        if let Some(res) = &self.resources {
            res.request_faces(&ids);
        }
    }

    /// Step 3: a completion batch of 007 changed the state of this document's faces (`AppliedBatch::
    /// font_epoch_changed`). `faces` is the document's face views after the batch. The document's font
    /// epoch advances once, `FONT_EPOCH` is recorded for the families that changed, and the next update
    /// rebuilds the font context over the loaded faces and relays out. Returns whether the document
    /// became dirty (a batch that changed nothing this document renders is a no-op).
    pub fn apply_font_faces(&mut self, faces: Vec<FaceView>) -> Result<bool, Status> {
        self.check_open()?;
        let state_of =
            |v: &[FaceView]| v.iter().map(|f| (f.id, f.state.name())).collect::<Vec<_>>();
        if state_of(&faces) == state_of(&self.fonts.faces) {
            return Ok(false);
        }
        let mut changed: Vec<String> = faces
            .iter()
            .filter(|f| {
                self.fonts
                    .faces
                    .iter()
                    .find(|o| o.id == f.id)
                    .is_none_or(|o| o.state.name() != f.state.name())
            })
            .map(|f| f.family.clone())
            .collect();
        changed.sort();
        changed.dedup();
        self.fonts.faces = faces;
        self.font_gen += 1;
        self.fonts.font_epoch = FontEpoch(self.fonts.font_epoch.0 + 1);
        self.note_font_epoch_changed(changed)?;
        Ok(true)
    }

    /// The document's `@font-face` faces as 009 sees them (for tests and diagnostics).
    pub fn font_faces(&self) -> &[FaceView] {
        &self.fonts.faces
    }

    /// The font epoch of this document's font context.
    pub fn font_epoch(&self) -> u64 {
        self.text.font_epoch()
    }
}

impl crate::pipeline::resources::PipelineResources {
    /// Register the document's effective `@font-face` rules with 007 and return the document's faces.
    pub fn sync_font_faces(&self, rules: &[FontFaceRule]) -> Result<Vec<FaceView>, Status> {
        let mut m = self.manager_lock();
        m.sync_font_faces(self.document(), rules)?;
        m.document_faces(self.document())
    }

    /// Ask 007 to load faces (non-blocking, idempotent); a face that left the cascade is ignored.
    pub fn request_faces(&self, ids: &[ui4me_assets::fonts::FaceId]) {
        let mut m = self.manager_lock();
        for id in ids {
            let _ = m.request_face(self.document(), *id);
        }
    }

    /// The document's faces after 007's last drain step.
    pub fn document_faces(&self) -> Result<Vec<FaceView>, Status> {
        self.manager_lock().document_faces(self.document())
    }
}
