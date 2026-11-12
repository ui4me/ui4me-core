//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Style parse reporting (T043; 006 `css-feature-profile.md` §2; 001 FR-023; AD-42; R-20).
//! The engine's `ParseErrorReporter` feeds a per-parse [`Collector`]; each error is classified:
//!
//! * a construct whose feature the profile excludes (Off, Gated-not-passed, Unavailable) →
//!   `UNSUPPORTED_FEATURE` (warning) with the feature id, and one entry in the R-20 store
//!   (`ui4me_engine_types::diag::UnsupportedReports`, report key `(feature, cascade, document,
//!   stylesheet location)`);
//! * other invalid syntax → `STYLE_PARSE_DIAGNOSTIC` (warning) with a category
//!   (`unknown-property`, `invalid-value`, `invalid-selector`, `unknown-at-rule`, …).
//!
//! Locations are 1-based lines and columns within the sheet text. Per sheet, the first 1,000
//! records are kept, then only counts per feature/category (spec FR-061).

use cssparser::SourceLocation;
use std::collections::BTreeMap;
use std::sync::Mutex;
use style::error_reporting::{ContextualParseError, ParseErrorReporter};
use style::stylesheets::UrlExtraData;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{FeatureId, SheetOrigin};
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
}

/// `{code, severity, feature, sheet_id, origin, url, line, column, excerpt}` (§2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleDiagnosticRecord {
    pub code: Code,
    pub severity: Severity,
    /// Set for `UNSUPPORTED_FEATURE`.
    pub feature: Option<FeatureId>,
    /// Set for `STYLE_PARSE_DIAGNOSTIC`.
    pub category: Option<&'static str>,
    pub sheet_id: u32,
    pub origin: SheetOrigin,
    /// The sheet URL (never `about:blank`); redacted on export (AD-35), see [`Self::redacted`].
    pub url: Option<String>,
    /// The element for `style=""` declarations.
    pub node: Option<NodeId>,
    pub line: u32,
    pub column: u32,
    pub excerpt: String,
}

impl StyleDiagnosticRecord {
    /// The export form (AD-35): the URL and the excerpt pass through the redaction policy `f`
    /// (005's `RedactionPolicy::text`), so a stored record never leaks a path or a secret.
    pub fn redacted(&self, f: impl Fn(&str) -> String) -> StyleDiagnosticRecord {
        StyleDiagnosticRecord {
            url: self.url.as_deref().map(&f),
            excerpt: f(&self.excerpt),
            ..self.clone()
        }
    }

    /// Feature id or category (the per-sheet overflow key).
    pub fn key(&self) -> &'static str {
        self.feature
            .map(|f| f.as_str())
            .or(self.category)
            .unwrap_or("other")
    }
}

/// Classify one engine parse error (the profile table decides exclusion).
pub(crate) fn classify(
    error: &ContextualParseError<'_>,
) -> (
    Code,
    Severity,
    Option<FeatureId>,
    Option<&'static str>,
    String,
) {
    use crate::style::profile::table::{at_rule_feature, property_feature, selector_feature};
    use cssparser::ParseErrorKind;
    use style_traits::StyleParseErrorKind as K;
    let unsupported = |f: FeatureId, text: &str| {
        (
            Code::UnsupportedFeature,
            Severity::Warning,
            Some(f),
            None,
            text.to_owned(),
        )
    };
    let parse = |cat: &'static str, text: &str| {
        (
            Code::StyleParseDiagnostic,
            Severity::Warning,
            None,
            Some(cat),
            text.to_owned(),
        )
    };
    match error {
        ContextualParseError::UnsupportedPropertyDeclaration(text, err, _) => {
            let name = text.split(':').next().unwrap_or("").trim();
            if let Some(f) = property_feature(name) {
                return unsupported(f, text);
            }
            if let Some((_, value)) = text.split_once(':')
                && let Some(f) = crate::style::profile::table::value_feature(name, value)
            {
                return unsupported(f, text);
            }
            if name.eq_ignore_ascii_case("display")
                && let Some(f) = text
                    .split(':')
                    .nth(1)
                    .and_then(|v| display_feature(v.trim()))
            {
                return unsupported(f, text);
            }
            match err.kind {
                // P-006-2: one `info` record per unknown vendor-prefixed declaration (excerpt = the lower-cased name).
                ParseErrorKind::Custom(K::UnknownVendorProperty) => (
                    Code::StyleParseDiagnostic,
                    Severity::Info,
                    None,
                    Some("vendor-prefixed"),
                    name.to_ascii_lowercase(),
                ),
                ParseErrorKind::Custom(K::UnknownProperty) if name.starts_with('-') => (
                    Code::StyleParseDiagnostic,
                    Severity::Info,
                    None,
                    Some("vendor-prefixed"),
                    (*text).to_owned(),
                ),
                ParseErrorKind::Custom(K::UnknownProperty) => parse("unknown-property", text),
                _ => parse("invalid-value", text),
            }
        }
        ContextualParseError::InvalidRule(text, _) => {
            // An at-rule the servo-mode parser (or a profile-off pref) rejects arrives here as an
            // invalid rule, not as `UnsupportedRule`; classify it by its at-keyword first.
            let at = text
                .trim_start()
                .strip_prefix('@')
                .and_then(|t| {
                    t.split(|c: char| c.is_whitespace() || c == '{' || c == ';' || c == '(')
                        .next()
                })
                .and_then(at_rule_feature);
            match at
                .or_else(|| selector_feature(text))
                .or_else(|| crate::style::profile::filter::sheet_selector_feature(text))
            {
                Some(f) => unsupported(f, text),
                None => parse("invalid-selector", text),
            }
        }
        ContextualParseError::UnsupportedRule(text, _) => {
            let name = text
                .trim_start()
                .trim_start_matches('@')
                .split(|c: char| c.is_whitespace() || c == '{' || c == ';' || c == '(')
                .next()
                .unwrap_or("");
            match at_rule_feature(name) {
                Some(f) => unsupported(f, text),
                None => parse("unknown-at-rule", text),
            }
        }
        ContextualParseError::InvalidMediaRule(text, _) => parse("invalid-media-query", text),
        ContextualParseError::UnsupportedValue(text, _) => parse("invalid-value", text),
        ContextualParseError::InvalidKeyframeRule(text, _) => parse("invalid-keyframe", text),
        ContextualParseError::UnsupportedFontFaceDescriptor(text, _) => {
            parse("invalid-descriptor", text)
        }
        other => parse("other", &other.to_string()),
    }
}

/// `display` values the engine does not parse in servo mode or the profile turns off (§3, §5).
fn display_feature(value: &str) -> Option<FeatureId> {
    let v = value
        .trim_end_matches(';')
        .trim()
        .trim_end_matches("!important")
        .trim()
        .to_ascii_lowercase();
    let id = match v.as_str() {
        "ruby" => "css.display.ruby",
        "ruby-base" => "css.display.ruby-base",
        "ruby-text" => "css.display.ruby-text",
        "ruby-base-container" => "css.display.ruby-base-container",
        "ruby-text-container" => "css.display.ruby-text-container",
        "grid-lanes" => "css.display.grid-lanes",
        "inline-grid-lanes" => "css.display.inline-grid-lanes",
        _ => return None,
    };
    FeatureId::new(id)
}

/// The engine-facing reporter for one parse (one sheet or one `style=""` value).
pub(crate) struct Collector {
    pub(crate) sheet_id: u32,
    pub(crate) origin: SheetOrigin,
    pub(crate) node: Option<NodeId>,
    pub(crate) records: Mutex<Vec<StyleDiagnosticRecord>>,
}

impl Collector {
    pub(crate) fn new(sheet_id: u32, origin: SheetOrigin, node: Option<NodeId>) -> Collector {
        Collector {
            sheet_id,
            origin,
            node,
            records: Mutex::new(vec![]),
        }
    }
    pub(crate) fn take(self) -> Vec<StyleDiagnosticRecord> {
        self.records.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

impl ParseErrorReporter for Collector {
    fn report_error(
        &self,
        url: &UrlExtraData,
        location: SourceLocation,
        error: ContextualParseError,
    ) {
        let (code, severity, feature, category, mut excerpt) = classify(&error);
        if excerpt.len() > 120 {
            let mut cut = 120;
            while !excerpt.is_char_boundary(cut) {
                cut -= 1;
            }
            excerpt.truncate(cut);
        }
        let rec = StyleDiagnosticRecord {
            code,
            severity,
            feature,
            category,
            sheet_id: self.sheet_id,
            origin: self.origin,
            url: Some(url.as_str())
                .filter(|u| *u != "about:blank")
                .map(str::to_owned),
            node: self.node,
            line: location.line + 1,
            column: location.column,
            excerpt,
        };
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(rec);
    }
}

/// Per-document style diagnostics: kept records (≤ 1,000 per sheet) plus overflow counts.
#[derive(Clone, Debug, Default)]
pub struct StyleDiagnostics {
    records: Vec<StyleDiagnosticRecord>,
    total: u64,
    kept_per_sheet: BTreeMap<u32, usize>,
    overflow: BTreeMap<(u32, &'static str), u64>,
    /// Unknown vendor-prefixed declarations per `(sheet, property name)` (R-08 fallback, T047).
    vendor_counts: BTreeMap<(u32, String), u64>,
}

impl StyleDiagnostics {
    pub const CAP_PER_SHEET: usize = 1000;

    pub(crate) fn push(&mut self, r: StyleDiagnosticRecord) {
        self.total += 1;
        if r.category == Some("vendor-prefixed") {
            *self
                .vendor_counts
                .entry((r.sheet_id, r.excerpt.clone()))
                .or_default() += 1;
        }
        let kept = self.kept_per_sheet.entry(r.sheet_id).or_default();
        if *kept < Self::CAP_PER_SHEET {
            *kept += 1;
            self.records.push(r);
        } else {
            *self.overflow.entry((r.sheet_id, r.key())).or_default() += 1;
        }
    }

    /// Drop everything recorded for the given sheets (they are being re-parsed).
    pub(crate) fn clear_sheets(&mut self, pred: impl Fn(u32) -> bool) {
        self.vendor_counts.retain(|(s, _), _| !pred(*s));
        let before = self.records.len();
        self.records.retain(|r| !pred(r.sheet_id));
        let removed_kept = (before - self.records.len()) as u64;
        let removed_overflow: u64 = self
            .overflow
            .iter()
            .filter(|((s, _), _)| pred(*s))
            .map(|(_, n)| *n)
            .sum();
        self.overflow.retain(|(s, _), _| !pred(*s));
        self.kept_per_sheet.retain(|s, _| !pred(*s));
        self.total -= removed_kept + removed_overflow;
    }

    /// Drop the inline (`style=""`) records of one element (its attribute is re-parsed).
    pub(crate) fn clear_inline(&mut self, node: NodeId) {
        let before = self.records.len();
        self.records.retain(|r| {
            !(r.sheet_id == crate::style::sheets::INLINE_SHEET_ID && r.node == Some(node))
        });
        let removed = before - self.records.len();
        self.total -= removed as u64;
        if let Some(k) = self
            .kept_per_sheet
            .get_mut(&crate::style::sheets::INLINE_SHEET_ID)
        {
            *k -= removed;
        }
    }

    pub fn records(&self) -> &[StyleDiagnosticRecord] {
        &self.records
    }
    /// `(sheet_id, property name) -> count` of unknown vendor-prefixed declarations, which the
    /// engine skips silently (R-08 fallback: every one is counted, none is skipped uncounted).
    pub fn vendor_prefixed_counts(&self) -> &BTreeMap<(u32, String), u64> {
        &self.vendor_counts
    }

    /// Every record ever classified for the current sheets, kept or not.
    pub fn total(&self) -> u64 {
        self.total
    }
    /// Records not kept because a sheet passed the cap.
    pub fn overflow_count(&self) -> u64 {
        self.overflow.values().sum()
    }
    /// `(sheet_id, feature-or-category) → count` beyond the cap.
    pub fn overflow(&self) -> &BTreeMap<(u32, &'static str), u64> {
        &self.overflow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(sheet: u32, key: &'static str) -> StyleDiagnosticRecord {
        StyleDiagnosticRecord {
            code: Code::StyleParseDiagnostic,
            severity: Severity::Warning,
            feature: None,
            category: Some(key),
            sheet_id: sheet,
            origin: SheetOrigin::Author,
            url: Some("file:///home/u/secret/site.css".into()),
            node: None,
            line: 1,
            column: 1,
            excerpt: "color: /home/u/secret".into(),
        }
    }

    #[test]
    fn cap_is_1000_per_sheet_with_overflow_counted_per_id() {
        let mut d = StyleDiagnostics::default();
        for _ in 0..1000 {
            d.push(rec(1, "unknown-property"));
        }
        for _ in 0..30 {
            d.push(rec(1, "invalid-value"));
        }
        for _ in 0..5 {
            d.push(rec(1, "unknown-property"));
        }
        d.push(rec(2, "unknown-property"));
        assert_eq!(
            d.records().iter().filter(|r| r.sheet_id == 1).count(),
            1000,
            "first 1,000 kept"
        );
        assert_eq!(
            d.records().iter().filter(|r| r.sheet_id == 2).count(),
            1,
            "the cap is per sheet"
        );
        assert_eq!(d.overflow().get(&(1, "invalid-value")), Some(&30));
        assert_eq!(d.overflow().get(&(1, "unknown-property")), Some(&5));
        assert_eq!(d.overflow_count(), 35);
        assert_eq!(d.total(), 1036);
    }

    #[test]
    fn export_form_is_redacted() {
        let r = rec(1, "x");
        let out = r.redacted(|t| t.replace("/home/u/secret", "<redacted>"));
        assert_eq!(out.url.as_deref(), Some("file://<redacted>/site.css"));
        assert_eq!(out.excerpt, "color: <redacted>");
        assert_eq!(
            r.url.as_deref(),
            Some("file:///home/u/secret/site.css"),
            "the stored record is untouched"
        );
    }

    #[test]
    fn collector_records_the_sheet_url_but_not_about_blank() {
        let lock = style::shared_lock::SharedRwLock::new();
        let url = UrlExtraData::from(url::Url::parse("file:///a/b.css").unwrap());
        let (_s, recs) = crate::style::sheets::parse_sheet(
            "p { colr: red }",
            SheetOrigin::Author,
            1,
            &lock,
            &url,
            style::context::QuirksMode::NoQuirks,
        );
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].url.as_deref(), Some("file:///a/b.css"));
        let (_s, recs) = crate::style::sheets::parse_sheet(
            "p { colr: red }",
            SheetOrigin::Author,
            1,
            &lock,
            &crate::style::sheets::blank_url(),
            style::context::QuirksMode::NoQuirks,
        );
        assert_eq!(recs[0].url, None);
    }

    #[test]
    fn report_keys_follow_r20() {
        // (feature, stage, document, location): repeats within one DOM revision count once.
        use ui4me_engine_types::diag::{
            ReportKey, ReportLocation, UnsupportedReason, UnsupportedReports,
        };
        use ui4me_engine_types::stage::Stage;
        let f = FeatureId::new("css.property.backdrop-filter").unwrap();
        let key = ReportKey {
            feature: f,
            stage: Stage::Cascade,
            document: ui4me_document::document::Document::parse_html("<p>x")
                .unwrap()
                .id(),
            location: ReportLocation::Stylesheet {
                sheet: 1,
                origin: SheetOrigin::Author,
                line: 3,
                column: 5,
            },
        };
        let mut store = UnsupportedReports::default();
        store.record(
            key,
            Code::UnsupportedFeature,
            UnsupportedReason::ProfileDisabled,
            7,
        );
        store.record(
            key,
            Code::UnsupportedFeature,
            UnsupportedReason::ProfileDisabled,
            7,
        );
        store.record(
            key,
            Code::UnsupportedFeature,
            UnsupportedReason::ProfileDisabled,
            8,
        );
        assert_eq!(store.len(), 1);
        assert_eq!(store.iter().next().unwrap().revisions, 2);
    }
}
