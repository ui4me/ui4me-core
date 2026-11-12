//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Selector service for DOM queries (T043; 006 `contracts/dom-services.md` §2; R-12): the
//! minimal slice service that 006 extends in its Phase 2 (never a parallel file, R-28).
//!
//! * **One profile, one parser, one matcher.** Selectors are parsed with the engine's
//!   `SelectorParser` under the process profile and matched with the same `selectors::Element`
//!   implementation and element state as the cascade (adapter/element.rs), which reads the
//!   current DOM — so a selector matches an element in a query iff it would in a style rule.
//! * **Scope** is DocumentCore's: [`Document::scope_for_query`] validates the root and
//!   [`Document::scope_elements`] gives the candidates in tree order (static results).
//! * **Errors**: syntax → `INVALID_ARGUMENT`/`SYNTAX`; a profile-excluded construct (e.g.
//!   `:has()` while G-STYLE-HAS is not PASS) → `INVALID_ARGUMENT`/`PROFILE_EXCLUDED` with the
//!   feature id and one `UNSUPPORTED_FEATURE` diagnostic record.
//!
//! The service never mutates the DOM, never starts a traversal and never touches ElementData:
//! it runs in its own pass with an empty sidecar store.

use crate::style::adapter::handle::{StyleNode, with_pass};
use crate::style::diagnostics::{Severity, StyleDiagnosticRecord};
use crate::style::sidecar::SidecarStore;
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags,
    QuirksMode as SelQuirks, SelectorCaches,
};
use selectors::matching::matches_selector_list;
use selectors::parser::SelectorList;
use style::selector_parser::{SelectorImpl, SelectorParser};
use style::shared_lock::SharedRwLock;
use style::stylesheets::UrlExtraData;
use ui4me_document::document::{Document, DocumentMode};
use ui4me_engine_types::codes_generated::{Code, SubReason};
use ui4me_engine_types::diag::{FeatureId, SheetOrigin};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// `SelectorError` (dom-services.md §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorError {
    pub code: Code,
    pub sub_reason: SubReason,
    pub feature: Option<FeatureId>,
    /// Byte offset of the error in the selector text, when known.
    pub position: Option<usize>,
    /// The `UNSUPPORTED_FEATURE` record for a profile-excluded construct.
    pub diagnostic: Option<Box<StyleDiagnosticRecord>>,
}

impl SelectorError {
    /// The host-facing status (004 maps it).
    pub fn to_status(&self) -> Status {
        let mut s = Status::new(self.code, Stage::Cascade).with_sub_reason(self.sub_reason);
        if let Some(f) = self.feature {
            s = s.with_message(f.as_str());
        }
        s
    }
}

/// A parsed selector list (cache key: text + profile digest).
#[derive(Clone, Debug)]
pub struct ParsedSelectorList {
    list: SelectorList<SelectorImpl>,
    text: String,
    digest: &'static str,
}

impl ParsedSelectorList {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn profile_digest(&self) -> &str {
        self.digest
    }
}

/// Selector-parse cache counters (by `(text, profile digest)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
}

type SelectorCache = (
    std::collections::HashMap<(String, String), ParsedSelectorList>,
    CacheStats,
);

pub struct SelectorService {
    url: UrlExtraData,
    cache: std::sync::Mutex<SelectorCache>,
    /// Matching never reads ElementData; this store stays empty.
    empty: SidecarStore,
}

impl std::fmt::Debug for SelectorService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SelectorService")
    }
}

impl Default for SelectorService {
    fn default() -> Self {
        SelectorService::new()
    }
}

fn status(code: Code) -> Status {
    Status::new(code, Stage::Cascade)
}

impl SelectorService {
    pub fn new() -> SelectorService {
        crate::style::profile::ensure_applied();
        SelectorService {
            url: crate::style::sheets::blank_url(),
            cache: Default::default(),
            empty: SidecarStore::new(SharedRwLock::new()),
        }
    }

    /// Parse a selector list under the profile. Cached by `(text, profile digest)`; an entry is
    /// never reused across digests (dom-services.md §2). Errors are not cached.
    pub fn parse(&self, text: &str) -> Result<ParsedSelectorList, SelectorError> {
        let key = (text.to_owned(), digest().to_owned());
        {
            let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(hit) = c.0.get(&key).cloned() {
                c.1.hits += 1;
                return Ok(hit);
            }
        }
        let list = self.parse_uncached(text)?;
        let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        c.1.misses += 1;
        c.0.insert(key, list.clone());
        c.1.entries = c.0.len();
        Ok(list)
    }

    /// `css_validate_selector`: parse only, no document needed.
    pub fn validate(&self, text: &str) -> Result<(), SelectorError> {
        self.parse(text).map(|_| ())
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).1
    }

    /// Whether a list for `(text, digest)` is in the cache.
    pub fn cache_would_reuse(&self, text: &str, profile_digest: &str) -> bool {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .0
            .contains_key(&(text.to_owned(), profile_digest.to_owned()))
    }

    fn parse_uncached(&self, text: &str) -> Result<ParsedSelectorList, SelectorError> {
        crate::style::profile::note_parse();
        let excluded = crate::style::profile::table::selector_feature(text);
        match SelectorParser::parse_author_origin_no_namespace(text, &self.url) {
            Ok(list) if excluded.is_none() => Ok(ParsedSelectorList {
                list,
                text: text.to_owned(),
                digest: digest(),
            }),
            result => {
                // The engine's selector ParseError carries no location at this revision, so the
                // position is the start of the first top-level comma-separated selector that does
                // not parse on its own (selector granularity; 0 for a single selector).
                let position: Option<usize> =
                    result.err().map(|_| first_bad_selector(text, &self.url));
                Err(match excluded {
                    Some(f) => SelectorError {
                        code: Code::InvalidArgument,
                        sub_reason: SubReason::ProfileExcluded,
                        feature: Some(f),
                        position,
                        diagnostic: Some(Box::new(StyleDiagnosticRecord {
                            code: Code::UnsupportedFeature,
                            severity: Severity::Warning,
                            feature: Some(f),
                            category: None,
                            sheet_id: crate::style::sheets::INLINE_SHEET_ID,
                            origin: SheetOrigin::Author,
                            url: None,
                            node: None,
                            line: 1,
                            column: position.map_or(1, |p| p as u32 + 1),
                            excerpt: text.chars().take(120).collect(),
                        })),
                    },
                    None => SelectorError {
                        code: Code::InvalidArgument,
                        sub_reason: SubReason::Syntax,
                        feature: None,
                        position,
                        diagnostic: None,
                    },
                })
            }
        }
    }

    fn run<R>(
        &self,
        doc: &Document,
        f: impl for<'s> FnOnce(
            &mut dyn FnMut(StyleNode<'s>, Option<NodeId>) -> bool,
            &dyn Fn(NodeId) -> StyleNode<'s>,
        ) -> R,
        list: &ParsedSelectorList,
    ) -> R {
        let quirks = match doc.mode() {
            DocumentMode::Quirks => SelQuirks::Quirks,
            DocumentMode::LimitedQuirks => SelQuirks::LimitedQuirks,
            DocumentMode::NoQuirks => SelQuirks::NoQuirks,
        };
        // `:has()` fast-rejects through a bloom filter of the id, classes and attributes of the anchor's
        // siblings or descendants, which the adapter reads from the sidecars: a `:has()` query needs them
        // (id/class atoms only; no style data). Every other query reads the attributes directly.
        let synced;
        let store: &SidecarStore = if list.text.contains(":has(") {
            let mut st = SidecarStore::new(SharedRwLock::new());
            let lock = st.lock.clone();
            st.sync(doc.arena(), |_, _| {
                style::servo_arc::Arc::new(
                    lock.wrap(style::properties::PropertyDeclarationBlock::new()),
                )
            });
            synced = st;
            &synced
        } else {
            &self.empty
        };
        with_pass(doc, store, |pass| {
            let mut caches = SelectorCaches::default();
            let mut matcher = |el: StyleNode<'_>, scope: Option<NodeId>| {
                let mut cx = MatchingContext::new(
                    MatchingMode::Normal,
                    None,
                    &mut caches,
                    quirks,
                    NeedsSelectorFlags::No,
                    MatchingForInvalidation::No,
                );
                cx.scope_element = scope.map(|s| selectors::Element::opaque(&el.with(s)));
                matches_selector_list(&list.list, el, &mut cx)
            };
            let mint = |id: NodeId| pass.handle(id);
            f(&mut matcher, &mint)
        })
    }

    /// `element.matches(list)`; `scope_root` is `:scope` (default: the element itself).
    pub fn matches(
        &self,
        doc: &Document,
        list: &ParsedSelectorList,
        element: NodeId,
        scope_root: Option<NodeId>,
    ) -> Result<bool, Status> {
        if doc.node_kind(element)? != ui4me_document::node::NodeKind::Element {
            return Err(status(Code::InvalidArgument).with_sub_reason(SubReason::WrongNodeKind));
        }
        let scope = scope_root.or(Some(element));
        Ok(self.run(doc, |m, mint| m(mint(element), scope), list))
    }

    /// `querySelectorAll`: static, tree order, descendants of `root`.
    pub fn query_all(
        &self,
        doc: &Document,
        list: &ParsedSelectorList,
        root: NodeId,
    ) -> Result<Vec<NodeId>, Status> {
        let scope = doc.scope_for_query(root)?;
        let candidates = doc.scope_elements(&scope);
        let scope_el = doc.arena().node(root).is_element().then_some(root);
        Ok(self.run(
            doc,
            |m, mint| {
                candidates
                    .into_iter()
                    .filter(|&c| m(mint(c), scope_el))
                    .collect()
            },
            list,
        ))
    }

    /// `querySelector`: the first match in tree order.
    pub fn query_first(
        &self,
        doc: &Document,
        list: &ParsedSelectorList,
        root: NodeId,
    ) -> Result<Option<NodeId>, Status> {
        let scope = doc.scope_for_query(root)?;
        let candidates = doc.scope_elements(&scope);
        let scope_el = doc.arena().node(root).is_element().then_some(root);
        Ok(self.run(
            doc,
            |m, mint| candidates.into_iter().find(|&c| m(mint(c), scope_el)),
            list,
        ))
    }

    /// `closest`: the nearest inclusive ancestor element that matches.
    pub fn closest(
        &self,
        doc: &Document,
        list: &ParsedSelectorList,
        element: NodeId,
    ) -> Result<Option<NodeId>, Status> {
        if doc.node_kind(element)? != ui4me_document::node::NodeKind::Element {
            return Err(status(Code::InvalidArgument).with_sub_reason(SubReason::WrongNodeKind));
        }
        let ancestors = doc.inclusive_ancestor_elements(element)?;
        Ok(self.run(
            doc,
            |m, mint| ancestors.into_iter().find(|&a| m(mint(a), Some(element))),
            list,
        ))
    }
}

fn digest() -> &'static str {
    crate::style::profile::active().digest()
}

/// Start offset of the first top-level comma-separated selector that fails to parse alone.
fn first_bad_selector(text: &str, url: &UrlExtraData) -> usize {
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut esc = false;
    let mut segs: Vec<(usize, usize)> = vec![];
    for (i, c) in text.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match (quote, c) {
            (_, '\\') => esc = true,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, ',') if depth == 0 => {
                segs.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
    }
    segs.push((start, text.len()));
    for (a, b) in segs {
        if SelectorParser::parse_author_origin_no_namespace(&text[a..b], url).is_err() {
            let lead = text[a..b].len() - text[a..b].trim_start().len();
            return a + lead;
        }
    }
    0
}

// ------------------------------------------------------------------ inline-style service (T018)

use crate::style::diagnostics::Collector;
use style::context::QuirksMode as StyleQuirks;
use style::properties::declaration_block::{
    PropertyDeclarationBlock, SourcePropertyDeclarationUpdate,
};
use style::properties::{Importance, PropertyId, SourcePropertyDeclaration};
use style::stylesheets::{CssRuleType, Origin};
use style_traits::ParsingMode;

/// Internal outcome of a write (dom-services.md section 3; XR-028). 004's composite calls map
/// all three to OK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineOutcome {
    Applied,
    /// Invalid value or unknown property: the block is unchanged.
    Ignored,
    /// Profile-excluded property: never stored.
    Dropped,
}

/// `InlineStyleWriteResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineWriteResult {
    pub outcome: InlineOutcome,
    /// `STYLE_PARSE_DIAGNOSTIC` (Ignored) or `UNSUPPORTED_FEATURE` (Dropped).
    pub diagnostic: Option<StyleDiagnosticRecord>,
    /// Present only when the block changed: the canonical text 003 commits as `style=""`.
    pub new_attribute_text: Option<String>,
    pub feature: Option<FeatureId>,
}

/// An ordered, profile-filtered declaration block parsed from a `style` attribute.
#[derive(Debug)]
pub struct DeclarationBlockView {
    block: PropertyDeclarationBlock,
    url: UrlExtraData,
}

/// The inline-style declaration service. It never touches the DOM: 003 commits
/// `new_attribute_text` as one mutation (AD-04).
pub struct InlineStyleService {
    quirks: StyleQuirks,
    element: Option<NodeId>,
}

impl Default for InlineStyleService {
    fn default() -> Self {
        InlineStyleService::new()
    }
}

impl InlineStyleService {
    pub fn new() -> InlineStyleService {
        crate::style::profile::ensure_applied();
        InlineStyleService {
            quirks: StyleQuirks::NoQuirks,
            element: None,
        }
    }

    /// Parse and serialise under the document's quirks mode.
    pub fn with_quirks(mut self, mode: DocumentMode) -> Self {
        self.quirks = match mode {
            DocumentMode::Quirks => StyleQuirks::Quirks,
            DocumentMode::LimitedQuirks => StyleQuirks::LimitedQuirks,
            DocumentMode::NoQuirks => StyleQuirks::NoQuirks,
        };
        self
    }

    /// Name the element that diagnostics refer to.
    pub fn for_element(mut self, element: NodeId) -> Self {
        self.element = Some(element);
        self
    }

    fn url_for(base: Option<&url::Url>) -> UrlExtraData {
        base.map_or_else(crate::style::sheets::blank_url, |u| {
            UrlExtraData::from(u.clone())
        })
    }

    /// `parse`: the ordered block of an attribute text.
    pub fn parse(&self, attr_text: &str, base_url: Option<&url::Url>) -> DeclarationBlockView {
        self.parse_with_diagnostics(attr_text, base_url).0
    }

    /// `parse` with the records of one parse of this attribute revision (spec FR-005).
    pub fn parse_with_diagnostics(
        &self,
        attr_text: &str,
        base_url: Option<&url::Url>,
    ) -> (DeclarationBlockView, Vec<StyleDiagnosticRecord>) {
        let url = Self::url_for(base_url);
        let (block, recs) =
            crate::style::sheets::parse_style_attribute(attr_text, self.element, &url, self.quirks);
        (DeclarationBlockView { block, url }, recs)
    }

    fn id_of(name: &str) -> Option<PropertyId> {
        PropertyId::parse_enabled_for_all_content(name).ok()
    }

    /// `get`: `("", false)` when absent.
    pub fn get(&self, view: &DeclarationBlockView, name: &str) -> (String, bool) {
        let Some(id) = Self::id_of(name) else {
            return (String::new(), false);
        };
        let mut s = String::new();
        let _ = view.block.property_value_to_css(&id, &mut s);
        if s.is_empty() {
            return (s, false);
        }
        (s, view.block.property_priority(&id).important())
    }

    /// `text`: CSSOM serialisation.
    pub fn text(&self, view: &DeclarationBlockView) -> String {
        Self::serialise(&view.block)
    }

    fn serialise(block: &PropertyDeclarationBlock) -> String {
        let mut s = String::new();
        let _ = block.to_css(&mut s);
        s
    }

    /// `list`: declared longhands (and custom properties) in order.
    pub fn list(&self, view: &DeclarationBlockView) -> Vec<String> {
        view.block
            .declarations()
            .iter()
            .map(|d| d.id().name().into_owned())
            .collect()
    }

    fn result(&self, before: &str, view: &DeclarationBlockView) -> InlineWriteResult {
        let after = Self::serialise(&view.block);
        InlineWriteResult {
            outcome: InlineOutcome::Applied,
            diagnostic: None,
            new_attribute_text: (after != before).then_some(after),
            feature: None,
        }
    }

    fn record(
        &self,
        code: Code,
        feature: Option<FeatureId>,
        category: Option<&'static str>,
        excerpt: String,
    ) -> StyleDiagnosticRecord {
        StyleDiagnosticRecord {
            code,
            severity: Severity::Warning,
            feature,
            category,
            sheet_id: crate::style::sheets::INLINE_SHEET_ID,
            origin: SheetOrigin::Inline,
            url: None,
            node: self.element,
            line: 1,
            column: 1,
            excerpt: excerpt.chars().take(120).collect(),
        }
    }

    /// `set`: CSSOM `setProperty`. An empty value removes the property.
    pub fn set(
        &self,
        view: &mut DeclarationBlockView,
        name: &str,
        value: &str,
        important: bool,
    ) -> InlineWriteResult {
        if value.trim().is_empty() {
            return self.remove(view, name);
        }
        let before = Self::serialise(&view.block);
        let excerpt = format!("{name}: {value}");
        if let Some(f) = crate::style::profile::table::property_feature(name) {
            let d = self.record(Code::UnsupportedFeature, Some(f), None, excerpt);
            return InlineWriteResult {
                outcome: InlineOutcome::Dropped,
                diagnostic: Some(d),
                new_attribute_text: None,
                feature: Some(f),
            };
        }
        let Some(id) = Self::id_of(name) else {
            let d = self.record(
                Code::StyleParseDiagnostic,
                None,
                Some("unknown-property"),
                excerpt,
            );
            return InlineWriteResult {
                outcome: InlineOutcome::Ignored,
                diagnostic: Some(d),
                new_attribute_text: None,
                feature: None,
            };
        };
        crate::style::profile::note_parse();
        let collector = Collector::new(
            crate::style::sheets::INLINE_SHEET_ID,
            SheetOrigin::Inline,
            self.element,
        );
        let mut src = SourcePropertyDeclaration::default();
        let parsed = style::properties::parse_one_declaration_into(
            &mut src,
            id,
            value,
            Origin::Author,
            &view.url,
            Some(&collector),
            ParsingMode::DEFAULT,
            self.quirks,
            CssRuleType::Style,
        );
        let recs = collector.take();
        if parsed.is_err() {
            if let Some(r) = recs.iter().find(|r| r.code == Code::UnsupportedFeature) {
                return InlineWriteResult {
                    outcome: InlineOutcome::Dropped,
                    feature: r.feature,
                    diagnostic: Some(r.clone()),
                    new_attribute_text: None,
                };
            }
            let d = recs.into_iter().next().unwrap_or_else(|| {
                self.record(
                    Code::StyleParseDiagnostic,
                    None,
                    Some("invalid-value"),
                    excerpt,
                )
            });
            return InlineWriteResult {
                outcome: InlineOutcome::Ignored,
                diagnostic: Some(d),
                new_attribute_text: None,
                feature: None,
            };
        }
        let importance = if important {
            Importance::Important
        } else {
            Importance::Normal
        };
        let mut updates = SourcePropertyDeclarationUpdate::default();
        if view
            .block
            .prepare_for_update(&src, importance, &mut updates)
        {
            view.block.update(src.drain(), importance, &mut updates);
        }
        self.result(&before, view)
    }

    /// `remove`: CSSOM `removeProperty`; a shorthand removes its longhands.
    pub fn remove(&self, view: &mut DeclarationBlockView, name: &str) -> InlineWriteResult {
        let before = Self::serialise(&view.block);
        if let Some(id) = Self::id_of(name)
            && let Some(first) = view.block.first_declaration_to_remove(&id)
        {
            view.block.remove_property(&id, first);
        }
        self.result(&before, view)
    }

    /// `set_text`: the `cssText` setter; re-parses the whole block.
    pub fn set_text(&self, view: &mut DeclarationBlockView, attr_text: &str) -> InlineWriteResult {
        let before = Self::serialise(&view.block);
        let (block, _recs) = crate::style::sheets::parse_style_attribute(
            attr_text,
            self.element,
            &view.url,
            self.quirks,
        );
        view.block = block;
        self.result(&before, view)
    }
}
