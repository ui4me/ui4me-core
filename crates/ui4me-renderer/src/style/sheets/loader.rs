//! owner: 006-style-engine — T027/T028 (L-RES-r3): linked stylesheets and `@import`, loaded
//! synchronously during the style update through a [`SheetLoader`] (AD-P-006-a, R-09).
//!
//! 006 never reads a file itself: the host side (007's `load_text_sync` behind the pipeline)
//! implements [`SheetLoader`], applies the resource policy and returns the bytes. A failure gives
//! an empty sheet and a diagnostic record, never a partial sheet (FR-003/FR-004):
//!
//! * `<link rel=stylesheet href>`: resolved against the document base URL; `RESOURCE_DENIED`,
//!   `URI_MALFORMED`, `FILE_NOT_FOUND`, `SIZE_LIMIT` as 007 reports them.
//! * `@import`: resolved against the importing sheet's URL, under the same policy; honours
//!   `layer()`, `supports()` and media; a chain deeper than 16 is `SIZE_LIMIT`; a cycle is a
//!   `STYLE_PARSE_DIAGNOSTIC` (`import-cycle`).
//!
//! Text is decoded with the CSS Syntax fallback encoding as far as the engine can without an
//! encoding library: a BOM (UTF-8, UTF-16), else UTF-8. An `@charset` naming another encoding is
//! reported (`STYLE_PARSE_DIAGNOSTIC`, `charset`) and the bytes are read as UTF-8.

use super::parse_author_sheet_with;
use crate::style::diagnostics::{Severity, StyleDiagnosticRecord};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use style::context::QuirksMode;
use style::media_queries::MediaList;
use style::servo_arc::Arc as ServoArc;
use style::shared_lock::{Locked, SharedRwLock};
use style::stylesheets::import_rule::{
    ImportLayer, ImportRule, ImportSheet, ImportSupportsCondition,
};
use style::stylesheets::{StylesheetLoader, UrlExtraData};
use style::values::CssUrl;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::SheetOrigin;
use ui4me_engine_types::status::Status;
use url::Url;

/// The longest `@import` chain (007 `IMPORT_MAX_DEPTH`); a deeper import is `SIZE_LIMIT`.
pub const IMPORT_MAX_DEPTH: usize = 16;

/// Sheet ids of imported sheets start here, so their diagnostics are never mistaken for the
/// importing document sheet's (author sheets are numbered from 1 in source order).
pub(crate) const IMPORT_SHEET_ID_BASE: u32 = 1 << 24;

/// The bytes of one loaded sheet and the URL they came from (the base of its relative `url()`s).
#[derive(Clone, Debug)]
pub struct LoadedSheet {
    pub url: Url,
    pub bytes: Arc<[u8]>,
}

/// The host side of sheet loading (007 behind the pipeline). Synchronous; runs on the owner
/// thread inside the style update.
pub trait SheetLoader: Send + Sync + std::fmt::Debug {
    /// Resolve `reference` against `base` and read it under the document's resource policy.
    fn load(&self, base: &Url, reference: &str) -> Result<LoadedSheet, Status>;
}

/// CSS Syntax "determine the fallback encoding", for the encodings the engine supports.
/// Returns the text and, if the sheet declares an encoding the engine cannot decode, its label.
pub fn decode_css(bytes: &[u8]) -> (String, Option<String>) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), None);
    }
    let utf16 = |rest: &[u8], le: bool| {
        let units = rest.as_chunks::<2>().0.iter().map(|c| {
            if le {
                u16::from_le_bytes(*c)
            } else {
                u16::from_be_bytes(*c)
            }
        });
        char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{FFFD}'))
            .collect::<String>()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (utf16(rest, true), None);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (utf16(rest, false), None);
    }
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut declared = None;
    if let Some(rest) = text.strip_prefix("@charset \"")
        && let Some(end) = rest.find("\";")
    {
        let label = rest[..end].trim().to_ascii_lowercase();
        if !matches!(
            label.as_str(),
            "utf-8" | "utf8" | "unicode-1-1-utf-8" | "us-ascii" | "ascii"
        ) {
            declared = Some(label);
        }
    }
    (text, declared)
}

/// A load failure as a diagnostic record (never `about:blank`).
pub(crate) fn failure_record(
    status: &Status,
    category: &'static str,
    sheet_id: u32,
    url: Option<&Url>,
    reference: &str,
) -> StyleDiagnosticRecord {
    StyleDiagnosticRecord {
        code: status.code,
        severity: Severity::Warning,
        feature: None,
        category: Some(category),
        sheet_id,
        origin: SheetOrigin::Author,
        url: url.map(|u| u.as_str().to_owned()),
        node: None,
        line: 0,
        column: 0,
        excerpt: format!(
            "{reference}: {}",
            if status.message.is_empty() {
                status.code.name()
            } else {
                status.message.as_str()
            }
        ),
    }
}

/// State shared by every parse of one sheet tree (the document sheet and its imports).
pub(crate) struct ImportShared<'a> {
    pub(crate) loader: &'a dyn SheetLoader,
    pub(crate) quirks: QuirksMode,
    pub(crate) next_id: Cell<u32>,
    pub(crate) records: RefCell<Vec<StyleDiagnosticRecord>>,
}

/// The `@import` loader of one sheet: knows the sheet's URL and its ancestors.
pub(crate) struct ImportLoader<'a> {
    pub(crate) shared: &'a ImportShared<'a>,
    /// Id of the sheet being parsed (its diagnostics are attributed to it).
    pub(crate) sheet_id: u32,
    /// URL of the sheet being parsed.
    pub(crate) base: Url,
    /// Ancestors, document sheet first, ending with `base`.
    pub(crate) chain: Vec<Url>,
}

impl ImportLoader<'_> {
    fn rule(
        &self,
        url: CssUrl,
        location: cssparser::SourceLocation,
        lock: &SharedRwLock,
        stylesheet: ImportSheet,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> ServoArc<Locked<ImportRule>> {
        ServoArc::new(lock.wrap(ImportRule {
            url,
            stylesheet,
            supports,
            layer,
            source_location: location,
        }))
    }

    fn note(
        &self,
        status: &Status,
        category: &'static str,
        reference: &str,
        url: Option<&Url>,
        location: cssparser::SourceLocation,
    ) {
        let mut r = failure_record(status, category, self.sheet_id, url, reference);
        r.line = location.line;
        r.column = location.column;
        self.shared.records.borrow_mut().push(r);
    }
}

impl StylesheetLoader for ImportLoader<'_> {
    fn request_stylesheet(
        &self,
        url: CssUrl,
        location: cssparser::SourceLocation,
        lock: &SharedRwLock,
        media: ServoArc<Locked<MediaList>>,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> ServoArc<Locked<ImportRule>> {
        let reference = url.as_str().to_owned();
        let refuse = |me: &Self, url: CssUrl, supports, layer| {
            me.rule(
                url,
                location,
                lock,
                ImportSheet::new_refused(),
                supports,
                layer,
            )
        };
        // `supports()` false: never fetched (CSS Cascade 5 section 2.2).
        if supports.as_ref().is_some_and(|s| !s.enabled) {
            return refuse(self, url, supports, layer);
        }
        // CssUrl::url() is the URL resolved against this sheet's URL; absent when it could not be.
        let resolved: Option<Url> = url.url().map(|u| (**u).clone());
        let Some(target) = resolved else {
            let s = Status::new(
                Code::UriMalformed,
                ui4me_engine_types::stage::Stage::Cascade,
            )
            .with_message("the @import URL could not be resolved against the sheet URL");
            self.note(&s, "import-load", &reference, Some(&self.base), location);
            return refuse(self, url, supports, layer);
        };
        if self.chain.iter().any(|u| u == &target) {
            let s = Status::new(
                Code::StyleParseDiagnostic,
                ui4me_engine_types::stage::Stage::Cascade,
            )
            .with_message("@import cycle");
            self.note(&s, "import-cycle", &reference, Some(&target), location);
            return refuse(self, url, supports, layer);
        }
        if self.chain.len() > IMPORT_MAX_DEPTH {
            let s = Status::new(Code::SizeLimit, ui4me_engine_types::stage::Stage::Cascade)
                .with_message(format!("@import chain deeper than {IMPORT_MAX_DEPTH}"));
            self.note(&s, "import-load", &reference, Some(&target), location);
            return refuse(self, url, supports, layer);
        }
        let loaded = match self.shared.loader.load(&self.base, target.as_str()) {
            Ok(l) => l,
            Err(s) => {
                self.note(&s, "import-load", &reference, Some(&target), location);
                return refuse(self, url, supports, layer);
            }
        };
        let id = self.shared.next_id.get();
        self.shared.next_id.set(id + 1);
        let (text, declared) = decode_css(&loaded.bytes);
        if let Some(label) = declared {
            let s = Status::new(
                Code::StyleParseDiagnostic,
                ui4me_engine_types::stage::Stage::Cascade,
            )
            .with_message(format!("@charset {label} is not decodable; read as UTF-8"));
            self.shared.records.borrow_mut().push(failure_record(
                &s,
                "charset",
                id,
                Some(&loaded.url),
                &reference,
            ));
        }
        let mut chain = self.chain.clone();
        chain.push(loaded.url.clone());
        let child = ImportLoader {
            shared: self.shared,
            sheet_id: id,
            base: loaded.url.clone(),
            chain,
        };
        let (sheet, records) = parse_author_sheet_with(
            &text,
            id,
            lock,
            &UrlExtraData::from(loaded.url.clone()),
            self.shared.quirks,
            media,
            Some(&child),
            Some(&loaded.url),
        );
        self.shared.records.borrow_mut().extend(records);
        self.rule(
            url,
            location,
            lock,
            ImportSheet::new(sheet.0),
            supports,
            layer,
        )
    }
}
