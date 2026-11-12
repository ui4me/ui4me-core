//! owner: 009-text-inline
//!
//! Text report helpers (T013; FR-059, FR-063, FR-072, FR-092, FR-093, FR-095; AD-34, AD-42; R-20).
//!
//! Text defines no report key of its own: `UNSUPPORTED_FEATURE` entries go into 005's single
//! store (`ui4me_engine_types::diag::UnsupportedReports`) under the key `(feature, text, document,
//! location)`, de-duplicated per document revision there. The other text codes
//! (`RESOURCE_PENDING`, `SIZE_LIMIT`, `DECODE_FAILED`, `GEOMETRY_NON_FINITE`) are stage
//! diagnostics collected in [`TextDiagnostics`], also de-duplicated per document revision.

use std::collections::HashSet;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{
    FeatureId, ReportKey, ReportLocation, UnsupportedReason, UnsupportedReports,
};
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::stage::Stage;

/// 009's feature ids, registered under 005's `css.value` / `css.property` roots.
pub mod feature {
    use super::FeatureId;

    pub const TEXT_JUSTIFY_INTER_CHARACTER: &str = "css.value.text-justify-inter-character";
    pub const BOX_DECORATION_BREAK_CLONE: &str = "css.value.box-decoration-break-clone";
    pub const SMALL_CAPS_SYNTHESIS: &str = "css.value.font-variant-caps.small-caps-synthesis";
    pub const POSITION_SYNTHESIS: &str = "css.value.font-variant-position-synthesis";
    pub const UNICODE_BIDI: &str = "css.property.unicode-bidi";
    /// `hyphens: auto` on an element with no `lang`, with a language that has no shipped patterns, or in a
    /// bidirectional paragraph (015 TXT-3).
    pub const HYPHENS_AUTO_NO_LANGUAGE: &str = "css.value.hyphens-auto-no-language";
    pub const HYPHENS_AUTO_LANGUAGE: &str = "css.value.hyphens-auto-language-without-patterns";
    pub const HYPHENS_AUTO_BIDI: &str = "css.value.hyphens-auto-bidi-paragraph";

    pub fn hyphens_auto_unsupported(
        why: crate::text::paragraph::HyphenUnsupported,
    ) -> &'static str {
        use crate::text::paragraph::HyphenUnsupported as H;
        match why {
            H::NoLanguage => HYPHENS_AUTO_NO_LANGUAGE,
            H::Language => HYPHENS_AUTO_LANGUAGE,
            H::Bidi => HYPHENS_AUTO_BIDI,
        }
    }

    /// Languages with language-sensitive `text-transform` mappings that are not applied.
    pub const TRANSFORM_LANGUAGES: &[(&str, &str)] = &[
        ("tr", "css.value.text-transform-language.tr"),
        ("az", "css.value.text-transform-language.az"),
        ("lt", "css.value.text-transform-language.lt"),
        ("el", "css.value.text-transform-language.el"),
        ("nl", "css.value.text-transform-language.nl"),
        ("ga", "css.value.text-transform-language.ga"),
    ];

    /// `css.value.text-transform-language.<lang>` for a registered language subtag.
    pub fn text_transform_language(lang: &str) -> Option<FeatureId> {
        let primary = lang
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        TRANSFORM_LANGUAGES
            .iter()
            .find(|(l, _)| *l == primary)
            .and_then(|(_, id)| FeatureId::new(id))
    }

    /// Every 009 id; all are valid under 005's roots.
    pub fn all() -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = [
            TEXT_JUSTIFY_INTER_CHARACTER,
            BOX_DECORATION_BREAK_CLONE,
            SMALL_CAPS_SYNTHESIS,
            POSITION_SYNTHESIS,
            UNICODE_BIDI,
            HYPHENS_AUTO_NO_LANGUAGE,
            HYPHENS_AUTO_LANGUAGE,
            HYPHENS_AUTO_BIDI,
        ]
        .iter()
        .filter_map(|id| FeatureId::new(id))
        .collect();
        v.extend(
            TRANSFORM_LANGUAGES
                .iter()
                .filter_map(|(_, id)| FeatureId::new(id)),
        );
        v
    }
}

/// Record `UNSUPPORTED_FEATURE` for `feature` on `node` with stage `text` (R-20 key).
pub fn report_unsupported(
    store: &mut UnsupportedReports,
    document: DocumentId,
    node: NodeId,
    feature: FeatureId,
    reason: UnsupportedReason,
    dom_revision: u64,
) {
    let key = ReportKey {
        feature,
        stage: Stage::Text,
        document,
        location: ReportLocation::Element { node },
    };
    store.record(key, Code::UnsupportedFeature, reason, dom_revision);
}

/// `unicode-bidi` values beyond the gated set are reported with reason `gated-off`.
pub fn report_unicode_bidi_gated_off(
    store: &mut UnsupportedReports,
    document: DocumentId,
    node: NodeId,
    dom_revision: u64,
) {
    if let Some(f) = FeatureId::new(feature::UNICODE_BIDI) {
        report_unsupported(
            store,
            document,
            node,
            f,
            UnsupportedReason::GatedOff,
            dom_revision,
        );
    }
}

/// Detail of a text stage diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextDiagnosticKind {
    /// A character had no glyph in any font; names the Unicode script (ISO 15924 tag).
    MissingGlyph { script: String },
    /// A font face is still loading.
    FacePending { family: String },
    /// A size limit was hit and the paragraph was truncated or clamped.
    SizeLimit { limit: &'static str },
    /// A font blob failed to decode.
    DecodeFailed { family: String },
    /// A geometry value was NaN or infinite and was clamped.
    GeometryNonFinite { what: &'static str },
    /// An inline item kind the text services do not shape yet; the item is ignored and reported
    /// (never approximated).
    UnsupportedItem { item: &'static str },
}

impl TextDiagnosticKind {
    /// The canonical code (never an ad-hoc string).
    pub fn code(&self) -> Code {
        match self {
            TextDiagnosticKind::MissingGlyph { .. } | TextDiagnosticKind::FacePending { .. } => {
                Code::ResourcePending
            }
            TextDiagnosticKind::SizeLimit { .. } => Code::SizeLimit,
            TextDiagnosticKind::DecodeFailed { .. } => Code::DecodeFailed,
            TextDiagnosticKind::GeometryNonFinite { .. } => Code::GeometryNonFinite,
            TextDiagnosticKind::UnsupportedItem { .. } => Code::UnsupportedFeature,
        }
    }
}

/// One text stage diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextDiagnostic {
    pub document: DocumentId,
    pub node: Option<NodeId>,
    pub kind: TextDiagnosticKind,
    pub first_dom_revision: u64,
}

impl TextDiagnostic {
    pub fn code(&self) -> Code {
        self.kind.code()
    }
    pub fn stage(&self) -> Stage {
        Stage::Text
    }
}

/// Collector with de-duplication: one entry per `(document, node, kind)` per DOM revision.
#[derive(Clone, Debug, Default)]
pub struct TextDiagnostics {
    entries: Vec<TextDiagnostic>,
    seen: HashSet<(DocumentId, Option<NodeId>, TextDiagnosticKind, u64)>,
}

impl TextDiagnostics {
    /// `true` when the diagnostic was new for this revision.
    pub fn record(
        &mut self,
        document: DocumentId,
        node: Option<NodeId>,
        kind: TextDiagnosticKind,
        dom_revision: u64,
    ) -> bool {
        if !self
            .seen
            .insert((document, node, kind.clone(), dom_revision))
        {
            return false;
        }
        self.entries.push(TextDiagnostic {
            document,
            node,
            kind,
            first_dom_revision: dom_revision,
        });
        true
    }
    pub fn iter(&self) -> impl Iterator<Item = &TextDiagnostic> {
        self.entries.iter()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::ids::Slots;

    fn doc() -> DocumentId {
        DocumentId::from_raw(Slots::new().alloc())
    }
    fn node() -> NodeId {
        NodeId::from_raw(Slots::new().alloc())
    }

    #[test]
    fn feature_ids_are_registered() {
        assert_eq!(feature::all().len(), 8 + feature::TRANSFORM_LANGUAGES.len());
        assert_eq!(
            feature::text_transform_language("tr-TR").unwrap().as_str(),
            "css.value.text-transform-language.tr"
        );
        assert!(feature::text_transform_language("en").is_none());
    }

    #[test]
    fn unsupported_uses_the_single_r20_key_and_dedups_per_revision() {
        let mut s = UnsupportedReports::default();
        let f = FeatureId::new(feature::TEXT_JUSTIFY_INTER_CHARACTER).unwrap();
        let (d, n) = (doc(), node());
        report_unsupported(&mut s, d, n, f, UnsupportedReason::NotConsumedByStage, 3);
        report_unsupported(&mut s, d, n, f, UnsupportedReason::NotConsumedByStage, 3);
        report_unsupported(&mut s, d, n, f, UnsupportedReason::NotConsumedByStage, 4);
        assert_eq!(s.len(), 1);
        let e = s.iter().next().unwrap();
        assert_eq!(e.key.stage, Stage::Text);
        assert_eq!(e.code, Code::UnsupportedFeature);
        assert_eq!((e.revisions, e.first_seen_dom, e.last_seen_dom), (2, 3, 4));
    }

    #[test]
    fn unicode_bidi_is_gated_off() {
        let mut s = UnsupportedReports::default();
        report_unicode_bidi_gated_off(&mut s, doc(), node(), 1);
        let e = s.iter().next().unwrap();
        assert_eq!(e.reason, UnsupportedReason::GatedOff);
        assert_eq!(e.key.feature.as_str(), "css.property.unicode-bidi");
    }

    #[test]
    fn stage_diagnostics_use_canonical_codes_and_dedup() {
        let mut d = TextDiagnostics::default();
        let dd = doc();
        let k = TextDiagnosticKind::MissingGlyph {
            script: "Deva".into(),
        };
        assert!(d.record(dd, None, k.clone(), 1));
        assert!(!d.record(dd, None, k, 1));
        assert!(d.record(
            dd,
            None,
            TextDiagnosticKind::SizeLimit { limit: "styles" },
            1
        ));
        assert!(d.record(
            dd,
            None,
            TextDiagnosticKind::DecodeFailed { family: "x".into() },
            1
        ));
        assert!(d.record(
            dd,
            None,
            TextDiagnosticKind::GeometryNonFinite { what: "caret" },
            1
        ));
        let codes: Vec<Code> = d.iter().map(|x| x.code()).collect();
        assert_eq!(
            codes,
            [
                Code::ResourcePending,
                Code::SizeLimit,
                Code::DecodeFailed,
                Code::GeometryNonFinite
            ]
        );
        assert!(d.iter().all(|x| x.stage() == Stage::Text));
    }
}
