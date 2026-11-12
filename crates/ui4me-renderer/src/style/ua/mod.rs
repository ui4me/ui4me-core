//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! The UA stylesheet subset for the slice (T040; research R-16; OD-26): `html.css`, derived
//! from the WHATWG HTML Rendering section @6d7788b under CC-BY-4.0 (pending owner approval).
//! Provenance (source, SHA, licence, rules taken, modifications): `ua/PROVENANCE.md` and the
//! `UA-SUBSET` entry of `engine/provenance.toml`. Embedded at build time.

use crate::style::diagnostics::StyleDiagnosticRecord;
use std::sync::OnceLock;
use style::shared_lock::SharedRwLock;
use style::stylesheets::DocumentStyleSheet;

/// The UA sheet text (WHATWG Rendering, non-quirks rules).
pub const HTML_CSS: &str = include_str!("html.css");
/// Form-control rules (WHATWG Rendering block + ui4me-owned values of 012 controls.md section 4).
pub const FORMS_CSS: &str = include_str!("forms.css");
/// Quirks-mode rules; in the cascade only while the document is in quirks mode.
pub const QUIRKS_CSS: &str = include_str!("quirks.css");

/// Parse every UA sheet under the profile and return every diagnostic they produce (006 section
/// 8: the UA sheets must produce none).
pub fn check_ua_sheet() -> Vec<StyleDiagnosticRecord> {
    let lock = style::shared_lock::SharedRwLock::new();
    let url = crate::style::sheets::blank_url();
    let mut all = vec![];
    for text in [HTML_CSS, FORMS_CSS, QUIRKS_CSS] {
        let (_sheet, mut recs) = crate::style::sheets::parse_sheet_audited(
            text,
            ui4me_engine_types::diag::SheetOrigin::UserAgent,
            crate::style::sheets::UA_SHEET_ID,
            &lock,
            &url,
            style::context::QuirksMode::NoQuirks,
            true,
        );
        all.append(&mut recs);
    }
    all
}

/// The user-agent sheets, parsed once per process and shared by every document (A23).
///
/// They are immutable and identical for every document (parsed in `NoQuirks` mode; the quirks sheet is only added to a
/// document's cascade while it is in quirks mode), so a document load no longer parses 17 KB of CSS (about 2,700
/// allocations and 130 KB of rules, selectors and declaration blocks per load) and the engine's user-agent cascade-data
/// cache sees the same sheets every time and is not rebuilt for a new document. The sheets live under their own lock;
/// the cascade reads them through the `ua_or_user` guard of [`style::shared_lock::StylesheetGuards`], a document's
/// author sheets through the document's lock.
pub(crate) struct SharedUa {
    pub(crate) lock: SharedRwLock,
    pub(crate) html: DocumentStyleSheet,
    pub(crate) forms: DocumentStyleSheet,
    pub(crate) quirks: DocumentStyleSheet,
}

pub(crate) fn shared() -> &'static SharedUa {
    static UA: OnceLock<SharedUa> = OnceLock::new();
    UA.get_or_init(|| {
        crate::style::profile::ensure_applied();
        let lock = SharedRwLock::new();
        let url = crate::style::sheets::blank_url();
        let parse = |text: &str| {
            let (sheet, recs) = crate::style::sheets::parse_sheet(
                text,
                ui4me_engine_types::diag::SheetOrigin::UserAgent,
                crate::style::sheets::UA_SHEET_ID,
                &lock,
                &url,
                style::context::QuirksMode::NoQuirks,
            );
            debug_assert!(
                recs.is_empty(),
                "the UA sheets produce no diagnostic: {recs:?}"
            );
            sheet
        };
        let (html, forms, quirks) = (parse(HTML_CSS), parse(FORMS_CSS), parse(QUIRKS_CSS));
        SharedUa {
            lock,
            html,
            forms,
            quirks,
        }
    })
}
