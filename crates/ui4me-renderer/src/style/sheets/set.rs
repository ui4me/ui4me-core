//! owner: 006-style-engine — created by 001 slice (R-28); linked sheets and `@import` by
//! L-RES-r3 (T027/T028).
//!
//! `DocumentStylesheetSet` (T040; 006 plan §6): the parsed sheets of one document in cascade
//! order — the UA subset, then the document's `<style>` and `<link rel=stylesheet>` sheets in
//! tree order. (Host-added sheets, A-15, come after these in 006's integrated plan; the slice has
//! none.) The set is brought in line with DocumentCore's source list whenever
//! `stylesheet_revision` changes.
//!
//! A linked sheet's bytes come from the [`SheetLoader`] (synchronously, R-09) and are cached per
//! `(link element, href, base URL)`, so a re-sync caused by an unrelated `<style>` edit does not
//! read the file again. Without a loader a `<link>` is reported `UNSUPPORTED_FEATURE`
//! (`html.link-stylesheet`), never dropped silently.

use super::loader::{
    IMPORT_SHEET_ID_BASE, ImportLoader, ImportShared, LoadedSheet, SheetLoader, decode_css,
    failure_record,
};
use super::{parse_author_sheet_with, parse_media_attribute};
use crate::style::diagnostics::{Severity, StyleDiagnosticRecord};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use style::context::QuirksMode;
use style::shared_lock::SharedRwLock;
use style::stylesheets::{DocumentStyleSheet, UrlExtraData};
use style::stylist::Stylist;
use ui4me_document::document::{Document, StyleSource};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{FeatureId, SheetOrigin};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;
use url::Url;

pub(crate) struct AuthorSheet {
    pub(crate) source: StyleSource,
    pub(crate) sheet: DocumentStyleSheet,
    /// The sheet's own text (the `<style>` text, or the decoded linked file): imports excluded.
    pub(crate) text: String,
    /// Results of `any_text` scans of `text`, by predicate (the text never changes; 015 PERF-1: the `lh`
    /// scan of a large sheet ran on every restyle).
    pub(crate) scans: std::sync::Mutex<Vec<(usize, bool)>>,
}

pub(crate) struct DocumentStylesheetSet {
    pub(crate) author: Vec<AuthorSheet>,
    seen_revision: Option<u64>,
    /// The quirks-mode UA sheet; in the stylist only while the document is in quirks mode.
    ua_quirks: DocumentStyleSheet,
    quirks_active: bool,
    loader: Option<Arc<dyn SheetLoader>>,
    base: Url,
    /// Loaded `<link>` bytes (or the load failure) per `(element, href, base)`.
    link_cache: HashMap<(NodeId, String, Url), Result<LoadedSheet, Status>>,
    next_import_id: u32,
    /// The loader or base changed: re-parse even if the source list is unchanged.
    force: bool,
}

/// Outcome of a sync.
pub(crate) struct SheetSync {
    pub(crate) changed: bool,
    pub(crate) records: Vec<StyleDiagnosticRecord>,
}

impl DocumentStylesheetSet {
    /// Register the UA subset (parsed once per process, [`crate::style::ua::shared`]) with the document's stylist.
    pub(crate) fn new(
        stylist: &mut Stylist,
        _lock: &SharedRwLock,
        _url: &UrlExtraData,
        base: Url,
    ) -> (DocumentStylesheetSet, Vec<StyleDiagnosticRecord>) {
        let ua = crate::style::ua::shared();
        for sheet in [&ua.html, &ua.forms] {
            stylist.append_stylesheet(sheet.clone(), &ua.lock.read());
        }
        (
            DocumentStylesheetSet {
                author: vec![],
                seen_revision: None,
                ua_quirks: ua.quirks.clone(),
                quirks_active: false,
                loader: None,
                base,
                link_cache: HashMap::new(),
                next_import_id: IMPORT_SHEET_ID_BASE,
                force: false,
            },
            vec![],
        )
    }

    /// Install (or remove) the loader of linked sheets and `@import`s. The next sync re-parses.
    pub(crate) fn set_loader(&mut self, loader: Option<Arc<dyn SheetLoader>>) {
        let same = match (&self.loader, &loader) {
            (None, None) => true,
            (Some(a), Some(b)) => std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b)),
            _ => false,
        };
        if same {
            return;
        }
        self.loader = loader;
        self.link_cache.clear();
        self.seen_revision = None;
        self.force = true;
    }

    /// Drop the cached loads of linked sheets and `@import`s so the next sync reads them again
    /// (the content behind a URL changed, e.g. a replaced `ui4me-res:` bundle).
    pub(crate) fn reload_links(&mut self) {
        self.link_cache.clear();
        self.seen_revision = None;
        self.force = true;
    }

    /// The document base URL (relative `<link href>` and `@import` resolve against it). A change
    /// drops the cached loads and re-parses on the next sync.
    pub(crate) fn set_base(&mut self, base: Url) {
        if base != self.base {
            self.base = base;
            self.link_cache.clear();
            self.seen_revision = None;
            self.force = true;
        }
    }

    /// Re-parse the author sheets if the document's source list changed.
    pub(crate) fn sync(
        &mut self,
        doc: &Document,
        stylist: &mut Stylist,
        lock: &SharedRwLock,
        url: &UrlExtraData,
        quirks: QuirksMode,
    ) -> SheetSync {
        // The quirks UA sheet follows the document mode (limited-quirks does not use it).
        let want_quirks = quirks == QuirksMode::Quirks;
        let mut mode_changed = false;
        if want_quirks != self.quirks_active {
            let guard = crate::style::ua::shared().lock.read();
            if want_quirks {
                stylist.append_stylesheet(self.ua_quirks.clone(), &guard);
            } else {
                stylist.remove_stylesheet(self.ua_quirks.clone(), &guard);
            }
            self.quirks_active = want_quirks;
            mode_changed = true;
        }
        if mode_changed {
            // Author sheets are parsed with the document's quirks mode: re-parse them too.
            self.seen_revision = None;
        }
        if self.seen_revision == Some(doc.stylesheet_revision()) {
            return SheetSync {
                changed: false,
                records: vec![],
            };
        }
        self.seen_revision = Some(doc.stylesheet_revision());
        let sources = doc.style_sources();
        if sources.len() == self.author.len()
            && sources
                .iter()
                .zip(&self.author)
                .all(|(s, a)| *s == a.source)
            && !self.author.is_empty()
            && !mode_changed
            && !self.force
        {
            return SheetSync {
                changed: false,
                records: vec![],
            };
        }
        self.force = false;
        let guard = lock.read();
        for a in self.author.drain(..) {
            stylist.remove_stylesheet(a.sheet, &guard);
        }
        drop(guard);
        let live_links: Vec<(NodeId, String)> = sources
            .iter()
            .filter_map(|s| s.href.clone().map(|h| (s.node, h)))
            .collect();
        self.link_cache
            .retain(|(n, h, _), _| live_links.iter().any(|(ln, lh)| ln == n && lh == h));
        let mut records = vec![];
        for (i, s) in sources.into_iter().enumerate() {
            let id = i as u32 + 1;
            let media = parse_media_attribute(lock, s.media.as_deref(), url, quirks);
            let (sheet, text, mut recs) = match &s.href {
                None => self.parse_style_element(&s, id, lock, url, quirks, media),
                Some(href) => self.parse_link(&s, href, id, lock, quirks, media),
            };
            // Report document lines: the sheet's first line is the start tag's line (a `<style>`
            // sheet only; a linked sheet's lines are its own).
            if s.line > 1 && s.href.is_none() {
                for r in recs.iter_mut().filter(|r| r.sheet_id == id) {
                    r.line += s.line - 1;
                }
            }
            records.extend(recs);
            stylist.append_stylesheet(sheet.clone(), &lock.read());
            self.author.push(AuthorSheet {
                source: s,
                sheet,
                text,
                scans: Default::default(),
            });
        }
        SheetSync {
            changed: true,
            records,
        }
    }

    fn parse_style_element(
        &mut self,
        s: &StyleSource,
        id: u32,
        lock: &SharedRwLock,
        url: &UrlExtraData,
        quirks: QuirksMode,
        media: style::servo_arc::Arc<style::shared_lock::Locked<style::media_queries::MediaList>>,
    ) -> (DocumentStyleSheet, String, Vec<StyleDiagnosticRecord>) {
        let base = self.base.clone();
        let (sheet, mut recs) =
            self.parse_with_imports(&s.text, id, lock, url, quirks, media, &base);
        (sheet, s.text.clone(), std::mem::take(&mut recs))
    }

    fn parse_link(
        &mut self,
        s: &StyleSource,
        href: &str,
        id: u32,
        lock: &SharedRwLock,
        quirks: QuirksMode,
        media: style::servo_arc::Arc<style::shared_lock::Locked<style::media_queries::MediaList>>,
    ) -> (DocumentStyleSheet, String, Vec<StyleDiagnosticRecord>) {
        let empty = |me: &mut Self, recs: Vec<StyleDiagnosticRecord>| {
            let (sheet, mut more) = parse_author_sheet_with(
                "",
                id,
                lock,
                &UrlExtraData::from(me.base.clone()),
                quirks,
                media.clone(),
                None,
                None,
            );
            let mut all = recs;
            all.append(&mut more);
            (sheet, String::new(), all)
        };
        if href.trim().is_empty() {
            // A link without an href fetches nothing (HTML: "link type stylesheet", processing).
            return empty(self, vec![]);
        }
        let Some(loader) = self.loader.clone() else {
            let rec = StyleDiagnosticRecord {
                code: Code::UnsupportedFeature,
                severity: Severity::Warning,
                feature: FeatureId::new("html.link-stylesheet"),
                category: None,
                sheet_id: id,
                origin: SheetOrigin::Author,
                url: None,
                node: None,
                line: s.line,
                column: 1,
                excerpt: href.to_owned(),
            };
            return empty(self, vec![rec]);
        };
        let key = (s.node, href.to_owned(), self.base.clone());
        let loaded = self
            .link_cache
            .entry(key)
            .or_insert_with(|| loader.load(&self.base, href))
            .clone();
        match loaded {
            Err(status) => {
                let mut rec = failure_record(&status, "link-load", id, None, href);
                rec.line = s.line;
                rec.column = 1;
                empty(self, vec![rec])
            }
            Ok(sheet_bytes) => {
                let (text, declared) = decode_css(&sheet_bytes.bytes);
                let mut pre = vec![];
                if let Some(label) = declared {
                    let st = Status::new(
                        Code::StyleParseDiagnostic,
                        ui4me_engine_types::stage::Stage::Cascade,
                    )
                    .with_message(format!("@charset {label} is not decodable; read as UTF-8"));
                    pre.push(failure_record(
                        &st,
                        "charset",
                        id,
                        Some(&sheet_bytes.url),
                        href,
                    ));
                }
                let sheet_url = UrlExtraData::from(sheet_bytes.url.clone());
                let (sheet, mut recs) = self.parse_with_imports(
                    &text,
                    id,
                    lock,
                    &sheet_url,
                    quirks,
                    media,
                    &sheet_bytes.url,
                );
                pre.append(&mut recs);
                (sheet, text, pre)
            }
        }
    }

    /// Parse `text` (a sheet at `sheet_url`), following its `@import`s through the loader.
    #[allow(clippy::too_many_arguments)]
    fn parse_with_imports(
        &mut self,
        text: &str,
        id: u32,
        lock: &SharedRwLock,
        url: &UrlExtraData,
        quirks: QuirksMode,
        media: style::servo_arc::Arc<style::shared_lock::Locked<style::media_queries::MediaList>>,
        sheet_url: &Url,
    ) -> (DocumentStyleSheet, Vec<StyleDiagnosticRecord>) {
        let Some(loader) = self.loader.clone() else {
            return parse_author_sheet_with(
                text,
                id,
                lock,
                url,
                quirks,
                media,
                None,
                Some(sheet_url),
            );
        };
        let shared = ImportShared {
            loader: &*loader,
            quirks,
            next_id: Cell::new(self.next_import_id),
            records: RefCell::new(vec![]),
        };
        let root = ImportLoader {
            shared: &shared,
            sheet_id: id,
            base: sheet_url.clone(),
            chain: vec![sheet_url.clone()],
        };
        let (sheet, mut recs) = parse_author_sheet_with(
            text,
            id,
            lock,
            url,
            quirks,
            media,
            Some(&root),
            Some(sheet_url),
        );
        self.next_import_id = shared.next_id.get();
        recs.extend(shared.records.take());
        (sheet, recs)
    }

    /// Whether any author sheet's own text satisfies `pred` (`lh` unit detection).
    pub(crate) fn any_text(&self, pred: fn(&str) -> bool) -> bool {
        let key = pred as usize;
        self.author.iter().any(|a| {
            let Ok(mut scans) = a.scans.lock() else {
                return pred(&a.text);
            };
            if let Some((_, r)) = scans.iter().find(|(k, _)| *k == key) {
                return *r;
            }
            let r = pred(&a.text);
            scans.push((key, r));
            r
        })
    }
}
