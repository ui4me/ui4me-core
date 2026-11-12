//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Sheet sources (T040; FR-015): the UA subset (origin user-agent), author `<style>` elements in
//! tree order (origin author) and `style=""` declaration blocks (inline). DocumentCore owns the
//! sources and their order (AD-28); this module parses them, always after the profile is
//! applied ([`crate::style::profile::note_parse`]), with the reporting collector of diagnostics.rs.

pub mod loader;
pub mod set;

use loader::ImportLoader;
use style::shared_lock::Locked;

use crate::style::diagnostics::{Collector, StyleDiagnosticRecord};
use style::context::QuirksMode;
use style::media_queries::MediaList;
use style::properties::PropertyDeclarationBlock;
use style::servo_arc::Arc;
use style::shared_lock::SharedRwLock;
use style::stylesheets::{
    AllowImportRules, CssRuleType, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData,
};
use ui4me_engine_types::diag::SheetOrigin;
use ui4me_engine_types::ids::NodeId;

/// Sheet id of the UA sheet; author sheets are numbered from 1 in source order.
pub const UA_SHEET_ID: u32 = 0;
/// Sheet id used for `style=""` records (the location is the element).
pub const INLINE_SHEET_ID: u32 = u32::MAX;

pub(crate) fn blank_url() -> UrlExtraData {
    UrlExtraData::from(url::Url::parse("about:blank").expect("static URL"))
}

/// Parse one stylesheet. Returns the sheet and its diagnostic records.
///
/// The user-agent sheets are embedded and known to produce no profile diagnostic (`ua::check_ua_sheet` runs the audited
/// parse over them in the tests), so the post-parse audit (serialising every selector and declaration to look for
/// excluded features) is skipped for them; author sheets are always audited.
pub(crate) fn parse_sheet(
    text: &str,
    origin: SheetOrigin,
    sheet_id: u32,
    lock: &SharedRwLock,
    url: &UrlExtraData,
    quirks: QuirksMode,
) -> (DocumentStyleSheet, Vec<StyleDiagnosticRecord>) {
    parse_sheet_audited(
        text,
        origin,
        sheet_id,
        lock,
        url,
        quirks,
        origin != SheetOrigin::UserAgent,
    )
}

/// [`parse_sheet`] with the profile audit (media-feature prescan and rule census) chosen by the caller.
pub(crate) fn parse_sheet_audited(
    text: &str,
    origin: SheetOrigin,
    sheet_id: u32,
    lock: &SharedRwLock,
    url: &UrlExtraData,
    quirks: QuirksMode,
    audit: bool,
) -> (DocumentStyleSheet, Vec<StyleDiagnosticRecord>) {
    crate::style::profile::note_parse();
    let collector = Collector::new(sheet_id, origin, None);
    let engine_origin = match origin {
        SheetOrigin::UserAgent => Origin::UserAgent,
        SheetOrigin::Author | SheetOrigin::Inline => Origin::Author,
    };
    let sheet = Stylesheet::from_str(
        text,
        url.clone(),
        engine_origin,
        Arc::new(lock.wrap(MediaList::empty())),
        lock.clone(),
        None,
        Some(&collector),
        quirks,
        AllowImportRules::No,
    );
    let mut records = collector.take();
    if audit {
        let w = crate::style::profile::filter::Where {
            sheet_id,
            origin,
            node: None,
        };
        let found = crate::style::profile::filter::prescan(text, w, &records);
        records.extend(found);
        records.extend(crate::style::profile::filter::census(&sheet, lock, w));
    }
    (DocumentStyleSheet(Arc::new(sheet)), records)
}

/// Parse a `style=""` value into a declaration block (profile-filtered by the engine's
/// enabled-property check; excluded declarations are reported against the element).
pub(crate) fn parse_style_attribute(
    text: &str,
    node: impl Into<Option<NodeId>>,
    url: &UrlExtraData,
    quirks: QuirksMode,
) -> (PropertyDeclarationBlock, Vec<StyleDiagnosticRecord>) {
    crate::style::profile::note_parse();
    let node_opt: Option<NodeId> = node.into();
    let collector = Collector::new(INLINE_SHEET_ID, SheetOrigin::Inline, node_opt);
    let block = style::properties::parse_style_attribute(
        text,
        url,
        Some(&collector),
        quirks,
        CssRuleType::Style,
    );
    let mut records = collector.take();
    let lower = text.to_ascii_lowercase();
    if lower.contains("background-clip") || lower.contains("column") || lower.contains("break-") {
        let w = crate::style::profile::filter::Where {
            sheet_id: INLINE_SHEET_ID,
            origin: SheetOrigin::Inline,
            node: node_opt,
        };
        let found = crate::style::profile::filter::prescan_declarations(text, w, &records);
        records.extend(found);
    }
    (block, records)
}

/// Parse one author sheet whose `@import`s are followed through `loader` (when present). `media`
/// is the sheet's media list (`<link media>` / `@import ... <media>`); `sheet_url` is attached to
/// the records. Returns the sheet and its diagnostic records.
#[allow(clippy::too_many_arguments)]
pub(crate) fn parse_author_sheet_with(
    text: &str,
    sheet_id: u32,
    lock: &SharedRwLock,
    url: &UrlExtraData,
    quirks: QuirksMode,
    media: Arc<Locked<MediaList>>,
    loader: Option<&ImportLoader<'_>>,
    sheet_url: Option<&url::Url>,
) -> (DocumentStyleSheet, Vec<StyleDiagnosticRecord>) {
    crate::style::profile::note_parse();
    let collector = Collector::new(sheet_id, SheetOrigin::Author, None);
    let sheet = Stylesheet::from_str(
        text,
        url.clone(),
        Origin::Author,
        media,
        lock.clone(),
        loader.map(|l| l as &dyn style::stylesheets::StylesheetLoader),
        Some(&collector),
        quirks,
        if loader.is_some() {
            AllowImportRules::Yes
        } else {
            AllowImportRules::No
        },
    );
    let mut records = collector.take();
    let w = crate::style::profile::filter::Where {
        sheet_id,
        origin: SheetOrigin::Author,
        node: None,
    };
    let found = crate::style::profile::filter::prescan(text, w, &records);
    records.extend(found);
    records.extend(crate::style::profile::filter::census(&sheet, lock, w));
    if let Some(u) = sheet_url {
        for r in &mut records {
            if r.url.is_none() {
                r.url = Some(u.as_str().to_owned());
            }
        }
    }
    (DocumentStyleSheet(Arc::new(sheet)), records)
}

/// A `media` attribute / `@import` media text as a media list (an empty or absent text matches
/// everything; an invalid query list matches nothing, Media Queries 4 error handling).
pub(crate) fn parse_media_attribute(
    lock: &SharedRwLock,
    text: Option<&str>,
    url: &UrlExtraData,
    quirks: QuirksMode,
) -> Arc<Locked<MediaList>> {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return Arc::new(lock.wrap(MediaList::empty()));
    };
    let mut parser = cssparser::Parser::new(text);
    let mut context = style::parser::ParserContext::new(
        Origin::Author,
        url,
        Some(CssRuleType::Media),
        style_traits::ParsingMode::DEFAULT,
        quirks,
        std::borrow::Cow::Owned(style::stylesheets::Namespaces::default()),
        None,
        None,
        style::custom_properties::AttrTaint::default(),
    );
    Arc::new(lock.wrap(MediaList::parse(&mut context, &mut parser)))
}
