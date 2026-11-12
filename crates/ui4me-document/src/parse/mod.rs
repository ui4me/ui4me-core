//! owner: 003-document-model — created by 001 slice (R-28), completed by T026
//!
//! Document creation (AD-30; FR-001–FR-007): the real html5ever tree builder over the
//! [`sink::ArenaSink`]. DocumentCore writes the lifecycle `Created -> Parsing -> Live` (R-27); the
//! host never observes `Created` or `Parsing`, and a failed creation leaves no document (FR-002).
//! The initial parse produces no journal entries (the scheduler styles a new document cold).
//!
//! Tree-builder options: scripting **disabled** (`noscript` is ordinary markup; FR-005),
//! `iframe_srcdoc` false, `exact_errors` false (diagnostic wording follows the tokenizer's
//! normal messages; the verbosity belongs to 005's diagnostics level).

pub mod decode;
pub mod fragment;
pub mod sink;

use crate::arena::Arena;
use crate::document::{Document, Lifecycle};
use crate::limits::{DomLimits, LimitName};
use crate::node::NodeData;
use html5ever::driver::ParseOpts;
use html5ever::interface::QuirksMode;
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
use html5ever::{ns, parse_document};
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// The parse options of every ui4me parse (FR-005).
pub(crate) fn parse_opts(quirks: QuirksMode) -> ParseOpts {
    ParseOpts {
        tree_builder: TreeBuilderOpts {
            exact_errors: false,
            scripting_enabled: false,
            iframe_srcdoc: false,
            drop_doctype: false,
            quirks_mode: quirks,
        },
        ..Default::default()
    }
}

/// HTML "maybe clone an option into a selectedcontent", applied to the finished tree for every
/// `option` in tree order. html5ever 0.40.1 runs it only for explicit `</option>` end tags (its
/// FIXME, upstream issue 712), so implicitly closed options are handled here; the last selected
/// option wins, as when the algorithm runs at each pop. Selectedness is the `selected` attribute,
/// else the first option of the `select`.
pub(crate) fn apply_selectedcontent(a: &mut Arena, root: NodeId) {
    let is =
        |a: &Arena, n: NodeId, l: &str| a.node(n).as_element().is_some_and(|e| e.is_html_named(l));
    let selects: Vec<NodeId> = a
        .subtree(root)
        .into_iter()
        .filter(|&n| is(a, n, "select"))
        .collect();
    for select in selects {
        if a.node(select)
            .as_element()
            .is_some_and(|e| e.attr("multiple").is_some())
        {
            continue;
        }
        let tree = a.subtree(select);
        let Some(sc) = tree
            .iter()
            .copied()
            .skip(1)
            .find(|&n| is(a, n, "selectedcontent"))
        else {
            continue;
        };
        let options: Vec<NodeId> = tree
            .iter()
            .copied()
            .filter(|&o| is(a, o, "option") && option_nearest_select(a, o) == Some(select))
            .collect();
        let first = options.first().copied();
        let mut chosen = None;
        for &o in &options {
            if a.node(o)
                .as_element()
                .is_some_and(|e| e.attr("selected").is_some())
                || Some(o) == first
            {
                chosen = Some(o);
            }
        }
        let Some(option) = chosen else { continue };
        for old in a.node(sc).children.clone() {
            a.free_subtree(old);
        }
        for c in a.node(option).children.clone() {
            let cc = a.clone_deep(c);
            a.insert_trusted(sc, cc, None);
        }
    }
}

/// "option element nearest ancestor select" (HTML): `None` across `datalist`, `hr`, `option`, or a
/// second `optgroup`.
fn option_nearest_select(a: &Arena, option: NodeId) -> Option<NodeId> {
    let is = |n: NodeId, l: &str| a.node(n).as_element().is_some_and(|e| e.is_html_named(l));
    let mut seen_optgroup = false;
    let mut cur = a.node(option).parent;
    while let Some(p) = cur {
        if is(p, "datalist") || is(p, "hr") || is(p, "option") {
            return None;
        }
        if is(p, "optgroup") {
            if seen_optgroup {
                return None;
            }
            seen_optgroup = true;
        }
        if is(p, "select") {
            return Some(p);
        }
        cur = a.node(p).parent;
    }
    None
}

impl Document {
    /// `create(htmlText)` (A-01): parse to completion and return a `Live` document with the
    /// default limits and `about:blank` as its URL.
    pub fn parse(id: DocumentId, html: &str) -> Result<Document, Status> {
        Document::create(id, html, "about:blank", DomLimits::default())
    }

    /// `create(htmlText, url, limits)`: the runtime's entry (004 supplies the limits).
    pub fn create(
        id: DocumentId,
        html: &str,
        url: &str,
        limits: DomLimits,
    ) -> Result<Document, Status> {
        let limits = DomLimits::configured(limits);
        limits.check(LimitName::InputBytes, html.len() as u64)?;
        let mut doc = Document::new_created(id);
        doc.limits = limits;
        doc.url = url.to_owned();
        doc.set_lifecycle(Lifecycle::Parsing);
        let arena = std::mem::replace(&mut doc.arena, Arena::new());
        let out = parse_document(
            sink::ArenaSink::new(arena),
            parse_opts(QuirksMode::NoQuirks),
        )
        .one(html);
        doc.arena = out.arena;
        let droot = doc.arena.document();
        apply_selectedcontent(&mut doc.arena, droot);
        doc.finish_parse_forms();
        doc.arena.take_candidates();
        doc.parser_diagnostics = out.diagnostics;
        doc.mode = out.mode;
        // A creation that blows a limit leaves no document (FR-002): `doc` is simply dropped.
        doc.limits
            .check(LimitName::Nodes, doc.arena.live_count() as u64)?;
        doc.record_unsupported_constructs();
        doc.set_lifecycle(Lifecycle::Live);
        Ok(doc)
    }

    /// `createEmpty` (A-01; FR-003): `<!DOCTYPE html><html><head></head><body></body></html>`,
    /// no-quirks, `Live` on return.
    pub fn create_empty(id: DocumentId, url: &str, limits: DomLimits) -> Result<Document, Status> {
        Document::create(
            id,
            "<!DOCTYPE html><html><head></head><body></body></html>",
            url,
            limits,
        )
    }

    /// Create from bytes: UTF-8 decoding per FR-004 (BOM removal, U+FFFD with a parser
    /// diagnostic per invalid sequence, UTF-16 BOM or foreign `<meta charset>` reported as
    /// `UNSUPPORTED_FEATURE`), then as [`Document::create`].
    pub fn create_from_bytes(
        id: DocumentId,
        bytes: &[u8],
        url: &str,
        limits: DomLimits,
    ) -> Result<Document, Status> {
        let limits = DomLimits::configured(limits);
        limits.check(LimitName::InputBytes, bytes.len() as u64)?;
        let dec = decode::decode_utf8(bytes);
        let mut doc = Document::create(id, &dec.text, url, limits)?;
        doc.apply_decode_report(dec);
        Ok(doc)
    }

    /// FR-006 / FR-018: one `UNSUPPORTED_FEATURE` diagnostic per document reporting the number of
    /// `script` elements, event-handler content attributes (`on*`) and declarative shadow-root
    /// templates that were inserted but are inert / not attached.
    fn record_unsupported_constructs(&mut self) {
        let (mut scripts, mut handlers, mut shadows) = (0u64, 0u64, 0u64);
        for n in self.arena.iter() {
            let NodeData::Element(e) = &n.data else {
                continue;
            };
            if e.is_html_named("script") {
                scripts += 1;
            }
            handlers += e
                .attrs
                .iter()
                .filter(|a| {
                    a.name.ns == ns!() && a.name.local.len() > 2 && a.name.local.starts_with("on")
                })
                .count() as u64;
            if e.is_html_named("template")
                && e.attr("shadowrootmode").is_some_and(|v| {
                    v.eq_ignore_ascii_case("open") || v.eq_ignore_ascii_case("closed")
                })
            {
                shadows += 1;
            }
        }
        if scripts + handlers + shadows > 0 {
            self.parser_diagnostics.push_unsupported(format!(
                "scripting is not supported: {scripts} script element(s) inserted but not executed, {handlers} event-handler attribute(s) stored inertly, {shadows} declarative shadow root(s) not attached"
            ));
        }
    }
}
