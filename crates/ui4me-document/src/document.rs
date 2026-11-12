//! owner: 003-document-model — created by 001 slice (R-28)
//!
//! DocumentCore (T035): one [`Document`] per document — arena, lifecycle, DOM revision,
//! journal, hooks, element-state and control-state tables, retention and collection.
//!
//! * **Lifecycle** (R-27, R-34) is written only here: `Created → Parsing → Live` for a parsed
//!   document, then `Closing`/`Closed`. From `Closing`, DOM and derived calls return
//!   `DOCUMENT_CLOSED`, while releases and the lifecycle status stay OK (idempotent).
//! * **Error precedence** (FR-021): `STALE_HANDLE` (arguments in order) → `WRONG_DOCUMENT` →
//!   `DOCUMENT_CLOSED` → DOM validation. `WRONG_THREAD` is the HostRuntime's (owner-thread
//!   context, 004); DocumentCore itself holds no thread-local state.
//! * **Revision**: exactly one DOM revision step per accepted mutating call (FR-082–083); reads
//!   never step it. Creation (`C`) steps nothing.
//! * **Retention** (AD-29, FR-022): host references per node; a detached tree with no
//!   reference anywhere in it is freed at the next collection checkpoint ([`Document::collect`]),
//!   after which its ids are `STALE_HANDLE`. A detached node stays valid while referenced.
//!
//! Errors carry stage `host` (they are host-call validation failures; parser records are
//! stage `parse`).

use crate::arena::Arena;
use crate::control_state::{ControlState, ControlStateWriter};
use crate::hooks::Hooks;
use crate::journal::{EntryKind, EpochSegment, Journal};
use crate::node::{Attribute, Node, NodeData, NodeKind};
use crate::ui_state::{UiFlags, UiStateKey, UiStateWriter};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::{DocumentId, NodeId, NodeKey, RawId};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// A DOM-API error status (stage `host`).
pub(crate) fn err(code: Code) -> Status {
    Status::new(code, Stage::Host)
}

/// Document lifecycle (R-27; 005 `document` probe vocabulary).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lifecycle {
    Created,
    Parsing,
    Live,
    Closing,
    Closed,
}

/// How an operation enters a document (the entry classifier of T012, R-34): DOM calls (reads and
/// mutations) and derived calls fail from `Closing`; document-status operations (lifecycle query,
/// `close()`) and releases succeed in every state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpClass {
    Dom,
    Derived,
    DocumentStatus,
    Release,
}

impl OpClass {
    pub const ALL: [OpClass; 4] = [
        OpClass::Dom,
        OpClass::Derived,
        OpClass::DocumentStatus,
        OpClass::Release,
    ];
}

impl Lifecycle {
    /// The AD-30 graph: `Created -> Parsing -> Live -> Closing -> Closed`, plus `Live -> Closed`
    /// (close outside a dispatch). Nothing else.
    pub const fn can_enter(self, next: Lifecycle) -> bool {
        matches!(
            (self, next),
            (Lifecycle::Created, Lifecycle::Parsing)
                | (Lifecycle::Parsing, Lifecycle::Live)
                | (Lifecycle::Live, Lifecycle::Closing)
                | (Lifecycle::Live, Lifecycle::Closed)
                | (Lifecycle::Closing, Lifecycle::Closed)
        )
    }

    /// Does an operation of `class` pass the entry check in this state?
    pub const fn admits(self, class: OpClass) -> bool {
        match class {
            OpClass::DocumentStatus | OpClass::Release => true,
            OpClass::Dom | OpClass::Derived => {
                !matches!(self, Lifecycle::Closing | Lifecycle::Closed)
            }
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Lifecycle::Created => "created",
            Lifecycle::Parsing => "parsing",
            Lifecycle::Live => "live",
            Lifecycle::Closing => "closing",
            Lifecycle::Closed => "closed",
        }
    }
}

/// Document mode (A-04), set by the parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentMode {
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

/// `dom.parser_diagnostics` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParserDiagnostic {
    pub line: u64,
    pub message: String,
}

/// The first 1,000 parser records plus the total (FR-007, FR-064).
#[derive(Clone, Debug, Default)]
pub struct ParserDiagnostics {
    pub(crate) total: u64,
    pub(crate) records: Vec<ParserDiagnostic>,
    /// `UNSUPPORTED_FEATURE` reports (scripts, handler attributes, shadow roots, encodings).
    pub(crate) unsupported: Vec<String>,
}

impl ParserDiagnostics {
    pub const CAP: usize = 1000;
    /// `UNSUPPORTED_FEATURE` messages recorded at creation (FR-004, FR-006, FR-018).
    pub fn unsupported_features(&self) -> &[String] {
        &self.unsupported
    }
    pub(crate) fn push_unsupported(&mut self, message: String) {
        self.unsupported.push(message);
    }
    pub fn total(&self) -> u64 {
        self.total
    }
    pub fn records(&self) -> &[ParserDiagnostic] {
        &self.records
    }
    pub(crate) fn push(&mut self, line: u64, message: String) {
        self.total += 1;
        if self.records.len() < Self::CAP {
            self.records.push(ParserDiagnostic { line, message });
        }
    }
}

pub use crate::retain::DomCounts;

/// One connected `<style>` element's sheet text, or one applicable `<link rel=stylesheet>`
/// element (tree order; FR-015).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleSource {
    pub node: NodeId,
    /// The `<style>` text; empty for a `<link>` (its bytes come from 007, loaded by 006).
    pub text: String,
    /// 1-based document line of the `<style>`/`<link>` start tag (0 when not parser-created); a
    /// stylesheet line `n` is document line `line + n - 1`.
    pub line: u32,
    /// `Some(href attribute)` for a `<link>` source (`Some("")` when the attribute is absent or
    /// empty), `None` for a `<style>`.
    pub href: Option<String>,
    /// The `media` attribute, if any.
    pub media: Option<String>,
}

static NEXT_STANDALONE_DOCUMENT: AtomicU32 = AtomicU32::new(1 << 31);

pub struct Document {
    pub(crate) id: DocumentId,
    pub(crate) arena: Arena,
    lifecycle: Lifecycle,
    history: Vec<Lifecycle>,
    pub(crate) mode: DocumentMode,
    revision: u64,
    /// Depth of the accepted mutating call in progress (0 = none).
    call_depth: u32,
    pub(crate) journal: Journal,
    pub(crate) hooks: Hooks,
    ui_key_taken: bool,
    pub(crate) parser_diagnostics: ParserDiagnostics,
    pub(crate) stylesheet_revision: u64,
    pub(crate) freed_total: u64,
    pub(crate) limits: crate::limits::DomLimits,
    pub(crate) url: String,
    pub(crate) attachments: crate::stylesheets::Attachments,
    pub(crate) notif_queue: Vec<crate::hooks::Notification>,
    pub(crate) batch_active: bool,
    /// DOM revision of the call that last advanced the stylesheet revision (one step per call).
    pub(crate) sheet_noted_rev: Option<u64>,
    /// `(stylesheet_revision, queued notifications)` at batch start, restored on rollback.
    pub(crate) batch_saved: Option<(u64, usize)>,
    /// The top layer (015 HTML-1), in insertion order: the elements whose UI flags hold `TOP_LAYER`.
    pub(crate) top_layer: Vec<NodeId>,
    #[cfg(feature = "test-sinks")]
    pub(crate) fail_alloc_countdown: std::cell::Cell<u32>,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("id", &self.id)
            .field("lifecycle", &self.lifecycle)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl Document {
    fn with_lifecycle(id: DocumentId, lifecycle: Lifecycle) -> Document {
        Document {
            id,
            arena: Arena::new(),
            lifecycle,
            history: vec![lifecycle],
            mode: DocumentMode::NoQuirks,
            revision: 0,
            call_depth: 0,
            journal: Journal::new(),
            hooks: Hooks::default(),
            ui_key_taken: false,
            parser_diagnostics: ParserDiagnostics::default(),
            stylesheet_revision: 0,
            freed_total: 0,
            limits: crate::limits::DomLimits::default(),
            url: "about:blank".to_owned(),
            attachments: Default::default(),
            notif_queue: vec![],
            batch_active: false,
            sheet_noted_rev: None,
            batch_saved: None,
            top_layer: Vec::new(),
            #[cfg(feature = "test-sinks")]
            fail_alloc_countdown: std::cell::Cell::new(0),
        }
    }

    /// `createEmpty` (A-01; FR-003): a `Live` document with no children.
    pub fn new_empty(id: DocumentId) -> Document {
        Document::with_lifecycle(id, Lifecycle::Live)
    }

    pub(crate) fn new_created(id: DocumentId) -> Document {
        Document::with_lifecycle(id, Lifecycle::Created)
    }

    pub(crate) fn set_lifecycle(&mut self, l: Lifecycle) {
        debug_assert!(
            self.lifecycle == l
                || self.history.len() == 1
                    && self.lifecycle == Lifecycle::Created
                    && l == Lifecycle::Parsing
                || self.lifecycle.can_enter(l),
            "lifecycle {:?} -> {:?} is not an AD-30 edge",
            self.lifecycle,
            l
        );
        self.lifecycle = l;
        self.history.push(l);
    }

    /// Parse a full document with an id minted from a process-wide counter (tests, headless
    /// tools). Runtime documents use [`Document::parse`] with the runtime-scoped id (004).
    pub fn parse_html(html: &str) -> Result<Document, Status> {
        let index = NEXT_STANDALONE_DOCUMENT.fetch_add(1, Ordering::Relaxed);
        Document::parse(
            DocumentId::from_raw(RawId {
                index,
                generation: NonZeroU32::MIN,
            }),
            html,
        )
    }

    pub fn id(&self) -> DocumentId {
        self.id
    }

    /// The arena (structure-frozen view while borrowed; AD-07 `DomView`).
    pub fn arena(&self) -> &Arena {
        &self.arena
    }

    pub fn mode(&self) -> DocumentMode {
        self.mode
    }

    // ------------------------------------------------------------ lifecycle

    /// Status operation: succeeds in every state (A-14).
    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// Every lifecycle state entered, in order.
    pub fn lifecycle_history(&self) -> &[Lifecycle] {
        &self.history
    }

    pub(crate) fn is_closing(&self) -> bool {
        matches!(self.lifecycle, Lifecycle::Closing | Lifecycle::Closed)
    }

    /// `close()` (FR-090): `Live → Closed` when no dispatch is active, else `Live → Closing`
    /// until 012 reports the outermost COMMIT. Idempotent.
    pub fn request_close(&mut self, dispatch_active: bool) {
        match self.lifecycle {
            Lifecycle::Closing | Lifecycle::Closed => {}
            _ if dispatch_active => self.set_lifecycle(Lifecycle::Closing),
            _ => self.enter_closed(),
        }
    }

    /// 012 reports the outermost COMMIT: complete a pending `Closing → Closed`.
    pub fn outermost_commit(&mut self) {
        if self.lifecycle == Lifecycle::Closing {
            self.enter_closed();
        } else {
            self.collect();
        }
    }

    /// FR-091: free the tree of a `Closed` document (ids go stale; every handle of the document is
    /// still reported as `DOCUMENT_CLOSED`). No effect on a document that is not `Closed`.
    pub fn free_memory(&mut self) {
        if self.lifecycle == Lifecycle::Closed {
            self.arena.free_all();
        }
    }

    fn enter_closed(&mut self) {
        self.set_lifecycle(Lifecycle::Closed);
        // FR-091: listeners are released exactly once (012's hook), pending resource completions for
        // the document are discarded (007's notification), queued notifications are dropped, the
        // tree's memory is freed (ids go stale, so releases stay idempotent no-ops).
        if let Some(mut h) = self.hooks.document_closed.take() {
            h(self.id);
            self.hooks.document_closed = Some(h);
        }
        self.notif_queue.clear();
        if let Some(mut sink) = self.hooks.notifications.take() {
            sink(&crate::hooks::Notification::DocumentClosed);
            self.hooks.notifications = Some(sink);
        }
        self.attachments = Default::default();
    }

    // ------------------------------------------------------------ revision and journal

    /// `domRevision` (A-14): `DOCUMENT_CLOSED` once `Closing`.
    pub fn dom_revision(&self) -> Result<u64, Status> {
        if self.is_closing() {
            return Err(err(Code::DocumentClosed));
        }
        Ok(self.revision)
    }

    /// The DOM revision in every lifecycle state (005's revision input `dom`).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn stylesheet_revision(&self) -> u64 {
        self.stylesheet_revision
    }

    /// Start an accepted mutating call (after validation): entries get `pending_revision`.
    pub(crate) fn begin_call(&mut self) {
        self.call_depth += 1;
    }
    pub(crate) fn pending_revision(&self) -> u64 {
        self.revision + 1
    }
    /// Commit the call: exactly one revision step for the outermost call.
    pub(crate) fn end_call(&mut self) {
        self.call_depth -= 1;
        if self.call_depth == 0 {
            self.revision += 1;
            self.flush_notifications();
        }
    }

    /// Leave a call without a revision step (batch rollback).
    pub(crate) fn abort_call(&mut self) {
        self.call_depth -= 1;
    }

    /// Queue a post-commit notification (007); delivered when the outermost call commits.
    pub(crate) fn queue_notification(&mut self, n: crate::hooks::Notification) {
        self.notif_queue.push(n);
    }

    fn flush_notifications(&mut self) {
        if self.notif_queue.is_empty() {
            return;
        }
        let queued = std::mem::take(&mut self.notif_queue);
        if let Some(mut sink) = self.hooks.notifications.take() {
            for n in &queued {
                sink(n);
            }
            self.hooks.notifications = Some(sink);
        }
    }

    /// Whether a mutating call is in progress.
    pub(crate) fn in_call(&self) -> bool {
        self.call_depth > 0
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// The scheduler's `take_epoch()` (005 is the single consumer).
    pub fn take_epoch(&mut self) -> EpochSegment {
        self.journal.take_epoch()
    }

    pub fn hooks_mut(&mut self) -> &mut Hooks {
        &mut self.hooks
    }

    pub fn parser_diagnostics(&self) -> &ParserDiagnostics {
        &self.parser_diagnostics
    }

    // ------------------------------------------------------------ handle checks

    /// A node of this document that is live, in a document that is not closing.
    pub(crate) fn check(&self, n: NodeId) -> Result<&Node, Status> {
        // A `Closed` document keeps no nodes (FR-091): every handle of it is `DOCUMENT_CLOSED`
        // rather than stale. While `Closing`, nodes still exist and staleness outranks closure.
        if self.lifecycle == Lifecycle::Closed {
            return Err(err(Code::DocumentClosed));
        }
        let node = self.arena.get(n).ok_or_else(|| err(Code::StaleHandle))?;
        if self.is_closing() {
            return Err(err(Code::DocumentClosed));
        }
        Ok(node)
    }

    pub(crate) fn check_writable(&self, n: NodeId) -> Result<(), Status> {
        self.check(n).map(|_| ())
    }

    /// The entry check for an operation class (T012): `DOCUMENT_CLOSED` when the state refuses it.
    pub fn entry_check(&self, class: OpClass) -> Result<(), Status> {
        if self.lifecycle.admits(class) {
            Ok(())
        } else {
            Err(err(Code::DocumentClosed))
        }
    }

    pub(crate) fn check_open(&self) -> Result<(), Status> {
        if self.is_closing() {
            Err(err(Code::DocumentClosed))
        } else {
            Ok(())
        }
    }

    /// The handle form used across component boundaries.
    pub fn key(&self, n: NodeId) -> NodeKey {
        NodeKey {
            document: self.id,
            node: n,
        }
    }

    /// Resolve keys with the fixed precedence: `STALE_HANDLE` (own-document keys, in argument
    /// order) → `WRONG_DOCUMENT` → `DOCUMENT_CLOSED`.
    pub fn resolve_all(&self, keys: &[NodeKey]) -> Result<Vec<NodeId>, Status> {
        for k in keys {
            if k.document == self.id
                && self.lifecycle != Lifecycle::Closed
                && !self.arena.contains(k.node)
            {
                return Err(err(Code::StaleHandle));
            }
        }
        let ids = keys
            .iter()
            .map(|k| k.node_in(self.id).map_err(err))
            .collect::<Result<Vec<_>, _>>()?;
        self.check_open()?;
        Ok(ids)
    }

    pub fn resolve(&self, key: NodeKey) -> Result<NodeId, Status> {
        self.resolve_all(&[key]).map(|v| v[0])
    }

    /// `appendChild` with boundary handles (WRONG_DOCUMENT for a foreign node).
    pub fn append_child_key(&mut self, parent: NodeKey, child: NodeKey) -> Result<NodeId, Status> {
        let ids = self.resolve_all(&[parent, child])?;
        self.append_child(ids[0], ids[1])
    }

    // ------------------------------------------------------------ reads (R)

    pub fn document_node(&self) -> NodeId {
        self.arena.document()
    }

    /// `documentElement` (A-02).
    pub fn document_element(&self) -> Option<NodeId> {
        self.arena
            .node(self.arena.document())
            .children
            .iter()
            .copied()
            .find(|&c| self.arena.node(c).is_element())
    }

    fn html_child(&self, parent: Option<NodeId>, local: &str) -> Option<NodeId> {
        let p = parent?;
        self.arena.node(p).children.iter().copied().find(|&c| {
            self.arena
                .node(c)
                .as_element()
                .is_some_and(|e| e.is_html_named(local))
        })
    }

    pub fn head(&self) -> Option<NodeId> {
        let html = self.document_element().filter(|&h| {
            self.arena
                .node(h)
                .as_element()
                .is_some_and(|e| e.is_html_named("html"))
        });
        self.html_child(html, "head")
    }

    pub fn body(&self) -> Option<NodeId> {
        let html = self.document_element().filter(|&h| {
            self.arena
                .node(h)
                .as_element()
                .is_some_and(|e| e.is_html_named("html"))
        });
        self.html_child(html, "body")
    }

    pub fn node_kind(&self, n: NodeId) -> Result<NodeKind, Status> {
        Ok(self.check(n)?.kind())
    }

    pub fn parent(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        Ok(self.check(n)?.parent)
    }

    /// Child nodes (static, A-22). Template contents are never children.
    pub fn children(&self, n: NodeId) -> Result<&[NodeId], Status> {
        Ok(&self.check(n)?.children)
    }

    pub fn is_connected(&self, n: NodeId) -> Result<bool, Status> {
        self.check(n)?;
        Ok(self.arena.is_connected(n))
    }

    fn element(&self, n: NodeId) -> Result<&crate::node::ElementData, Status> {
        self.check(n)?.as_element().ok_or_else(|| {
            err(Code::InvalidArgument)
                .with_sub_reason(ui4me_engine_types::codes_generated::SubReason::WrongNodeKind)
        })
    }

    /// `localName` of an element.
    pub fn local_name(&self, n: NodeId) -> Result<&str, Status> {
        Ok(self.element(n)?.local_name())
    }

    pub fn qual_name(&self, n: NodeId) -> Result<&html5ever::QualName, Status> {
        Ok(self.element(n)?.name())
    }

    /// `attributes()` in stored order (A-44).
    pub fn attributes(&self, n: NodeId) -> Result<&[Attribute], Status> {
        Ok(self.element(n)?.attributes())
    }

    pub fn has_class(&self, n: NodeId, class: &str) -> Result<bool, Status> {
        Ok(self.element(n)?.classes().any(|c| c == class))
    }

    /// Text/Comment data.
    pub fn text_data(&self, n: NodeId) -> Result<&str, Status> {
        self.check(n)?.character_data().ok_or_else(|| {
            err(Code::InvalidArgument)
                .with_sub_reason(ui4me_engine_types::codes_generated::SubReason::WrongNodeKind)
        })
    }

    /// `textContent` (A-25): `None` for Document and DocumentType.
    pub fn text_content(&self, n: NodeId) -> Result<Option<String>, Status> {
        let node = self.check(n)?;
        Ok(match &node.data {
            NodeData::Document | NodeData::Doctype(_) => None,
            NodeData::Text(t) | NodeData::Comment(t) => Some(t.clone()),
            NodeData::Element(_) | NodeData::DocumentFragment { .. } => {
                Some(self.descendant_text(n))
            }
        })
    }

    pub(crate) fn descendant_text(&self, n: NodeId) -> String {
        let mut s = String::new();
        for d in self.arena.subtree(n) {
            if let NodeData::Text(t) = &self.arena.node(d).data {
                s.push_str(t);
            }
        }
        s
    }

    pub fn template_contents(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        Ok(self.element(n)?.template_contents())
    }

    pub fn doctype(&self) -> Option<&crate::node::Doctype> {
        self.arena
            .node(self.arena.document())
            .children
            .iter()
            .find_map(|&c| match &self.arena.node(c).data {
                NodeData::Doctype(d) => Some(d),
                _ => None,
            })
    }

    // ------------------------------------------------------------ state tables

    /// Is `n` inside a disabled `fieldset` (HTML 4.10.18.2 "actually disabled", 015 HTML-3)? True when an
    /// ancestor `fieldset` has `disabled` and `n` is not inside that fieldset's first `legend` child.
    pub fn fieldset_disabled(&self, n: NodeId) -> bool {
        if !self.arena.contains(n) {
            return false;
        }
        let mut child = n;
        let mut a = self.arena.node(n).parent();
        while let Some(p) = a {
            let node = self.arena.node(p);
            if node
                .as_element()
                .is_some_and(|e| e.is_html_named("fieldset") && e.attr("disabled").is_some())
            {
                let first_legend = node.children().iter().copied().find(|&c| {
                    self.arena
                        .node(c)
                        .as_element()
                        .is_some_and(|e| e.is_html_named("legend"))
                });
                if first_legend != Some(child) {
                    return true;
                }
            }
            child = p;
            a = node.parent();
        }
        false
    }

    /// The top layer in paint order (first = bottom-most; 015 HTML-1, CSS Position 4 section 3).
    pub fn top_layer(&self) -> &[NodeId] {
        &self.top_layer
    }

    /// The topmost modal `dialog` of the top layer (HTML "blocked by a modal dialog"), if any.
    pub fn topmost_modal(&self) -> Option<NodeId> {
        self.top_layer.iter().rev().copied().find(|n| {
            self.arena.is_connected(*n)
                && self
                    .arena
                    .get(*n)
                    .and_then(|x| x.as_element())
                    .is_some_and(|e| e.ui_flags().contains(UiFlags::MODAL))
        })
    }

    /// Is `n` inert (HTML 6.3 "inert subtrees")? A node is inert when it or an ancestor has the `inert`
    /// attribute (an HTML element), or when the document is blocked by a modal dialog and `n` is not
    /// the dialog or inside it. Inert nodes are not hit, take no focus and are not exposed to
    /// assistive technology (015 HTML-1).
    pub fn is_inert(&self, n: NodeId) -> bool {
        if !self.arena.contains(n) {
            return false;
        }
        if let Some(m) = self.topmost_modal()
            && !self.arena.is_inclusive_ancestor(m, n)
        {
            return true;
        }
        let mut a = Some(n);
        while let Some(x) = a {
            let node = self.arena.node(x);
            if node
                .as_element()
                .is_some_and(|e| e.is_html() && e.attr("inert").is_some())
            {
                return true;
            }
            a = node.parent();
        }
        false
    }

    pub fn ui_flags(&self, n: NodeId) -> Result<UiFlags, Status> {
        Ok(self.element(n)?.ui_flags())
    }

    /// Stored control record (A-55): `None` for a non-control.
    pub fn control_state(&self, n: NodeId) -> Result<Option<&ControlState>, Status> {
        Ok(self.element(n)?.control_state())
    }

    /// Take the document's single UI-state writer key (InteractionRuntime, at creation).
    pub fn take_ui_state_key(&mut self) -> Option<UiStateKey> {
        if self.ui_key_taken {
            return None;
        }
        self.ui_key_taken = true;
        Some(UiStateKey { document: self.id })
    }

    pub fn ui_state_writer(&mut self, key: &UiStateKey) -> Result<UiStateWriter<'_>, Status> {
        if key.document != self.id {
            return Err(err(Code::WrongDocument));
        }
        self.check_open()?;
        Ok(UiStateWriter {
            doc: self,
            in_call: false,
        })
    }

    pub fn control_state_writer(&mut self) -> ControlStateWriter<'_> {
        ControlStateWriter { doc: self }
    }

    // ------------------------------------------------------------ stylesheet sources

    /// Connected `<style>` elements and applicable `<link rel=stylesheet>` elements in tree order
    /// (DocumentCore owns stylesheet sources and order, AD-28). A `<link>` whose `rel` also
    /// contains `alternate`, or that has the `disabled` attribute, is not applied and is not
    /// listed (HTML "link type stylesheet": alternate style sheets are not enabled by default).
    pub fn style_sources(&self) -> Vec<StyleSource> {
        self.arena
            .subtree(self.arena.document())
            .into_iter()
            .filter_map(|n| {
                let node = self.arena.node(n);
                let e = node.as_element()?;
                let media = e.attr("media").map(str::to_owned);
                if e.is_html_named("style") {
                    return Some(StyleSource {
                        node: n,
                        line: e.source_line,
                        text: node
                            .children
                            .iter()
                            .filter_map(|&c| match &self.arena.node(c).data {
                                NodeData::Text(t) => Some(t.as_str()),
                                _ => None,
                            })
                            .collect(),
                        href: None,
                        media,
                    });
                }
                if e.is_html_named("link") && crate::stylesheets::is_stylesheet_source(e) {
                    let alternate = e.attr("rel").is_some_and(|r| {
                        r.split_ascii_whitespace()
                            .any(|t| t.eq_ignore_ascii_case("alternate"))
                    });
                    if alternate || e.attr("disabled").is_some() {
                        return None;
                    }
                    return Some(StyleSource {
                        node: n,
                        line: e.source_line,
                        text: String::new(),
                        href: Some(e.attr("href").unwrap_or("").to_owned()),
                        media,
                    });
                }
                None
            })
            // Attached author sheets (A-15) follow the document's own sources, in attachment order.
            .chain(self.attachments.sheets.iter().map(|(_, css)| StyleSource {
                node: self.arena.document(),
                line: 0,
                text: css.clone(),
                href: None,
                media: None,
            }))
            .collect()
    }

    /// Does a change at `n` (or its subtree) affect the stylesheet source list?
    pub(crate) fn touches_style_sources(&self, n: NodeId) -> bool {
        let is_style = |id: NodeId| {
            self.arena
                .get(id)
                .and_then(|x| x.as_element())
                .is_some_and(|e| e.is_html_named("style"))
        };
        let mut a = Some(n);
        while let Some(x) = a {
            if is_style(x) {
                return true;
            }
            a = self.arena.get(x).and_then(|y| y.parent);
        }
        self.arena.subtree(n).into_iter().any(is_style)
    }

    // ------------------------------------------------------------ document-level reads (T028)

    /// `compatMode` (A-04, FR-008): `"BackCompat"` in quirks mode, else `"CSS1Compat"`.
    pub fn compat_mode(&self) -> &'static str {
        if self.mode == DocumentMode::Quirks {
            "BackCompat"
        } else {
            "CSS1Compat"
        }
    }

    /// `URL` (A-06): the creation URL (default `about:blank`).
    pub fn url(&self) -> &str {
        &self.url
    }

    /// `baseURI` (A-06, FR-010): the `href` text of the first connected `base` element that has one
    /// (the frozen base URL), else the document URL. Resolving the `href` is 007's.
    pub fn base_uri(&self) -> String {
        self.frozen_base_href().unwrap_or_else(|| self.url.clone())
    }

    /// The document `title` (A-05): the child text content of the first `title` element in tree
    /// order, with ASCII whitespace stripped and collapsed; "" when there is none.
    pub fn title(&self) -> String {
        let t = self
            .arena
            .subtree(self.arena.document())
            .into_iter()
            .find(|&n| {
                self.arena
                    .node(n)
                    .as_element()
                    .is_some_and(|e| e.is_html_named("title"))
            });
        let Some(t) = t else { return String::new() };
        let raw: String = self
            .arena
            .node(t)
            .children
            .iter()
            .filter_map(|&c| {
                self.arena
                    .node(c)
                    .character_data()
                    .filter(|_| matches!(self.arena.node(c).data, NodeData::Text(_)))
            })
            .collect();
        raw.split([' ', '\t', '\n', '\x0c', '\r'])
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `title` setter (A-05): replace the text of the `title` element, creating one in `head` when
    /// the document has a `head` and no `title`; with neither, no effect.
    pub fn set_title(&mut self, text: &str) -> Result<(), Status> {
        self.check_open()?;
        let existing = self
            .arena
            .subtree(self.arena.document())
            .into_iter()
            .find(|&n| {
                self.arena
                    .node(n)
                    .as_element()
                    .is_some_and(|e| e.is_html_named("title"))
            });
        match (existing, self.head()) {
            (Some(t), _) => self.set_text_content(t, text),
            (None, Some(head)) => {
                let t = self.create_element("title")?;
                self.append_child(head, t)?;
                self.set_text_content(t, text)
            }
            (None, None) => Ok(()),
        }
    }

    pub(crate) fn note_stylesheets_changed(&mut self, rev: u64) {
        // One stylesheet-revision step and one `stylesheets` journal entry per call or batch.
        if self.sheet_noted_rev == Some(rev) {
            return;
        }
        self.sheet_noted_rev = Some(rev);
        self.stylesheet_revision += 1;
        let r = self.stylesheet_revision;
        self.journal.push(
            rev,
            EntryKind::Stylesheets {
                stylesheet_revision: r,
            },
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    const ALL: [Lifecycle; 5] = [
        Lifecycle::Created,
        Lifecycle::Parsing,
        Lifecycle::Live,
        Lifecycle::Closing,
        Lifecycle::Closed,
    ];

    #[test]
    fn transitions_follow_only_the_ad30_graph() {
        let mut edges = 0;
        for a in ALL {
            for b in ALL {
                if a.can_enter(b) {
                    edges += 1;
                }
            }
        }
        assert_eq!(edges, 5);
        assert!(
            !Lifecycle::Closed.can_enter(Lifecycle::Live)
                && !Lifecycle::Closing.can_enter(Lifecycle::Live)
                && !Lifecycle::Live.can_enter(Lifecycle::Parsing)
        );
    }

    #[test]
    fn the_entry_classifier_for_every_operation_class_in_every_state() {
        for st in ALL {
            for c in OpClass::ALL {
                let expect = match c {
                    OpClass::DocumentStatus | OpClass::Release => true,
                    OpClass::Dom | OpClass::Derived => matches!(
                        st,
                        Lifecycle::Created | Lifecycle::Parsing | Lifecycle::Live
                    ),
                };
                assert_eq!(st.admits(c), expect, "{st:?} {c:?}");
            }
        }
        let mut d = Document::parse_html("<p>x</p>").unwrap();
        assert!(d.entry_check(OpClass::Dom).is_ok());
        d.request_close(true);
        assert_eq!(
            d.entry_check(OpClass::Dom).unwrap_err().code,
            Code::DocumentClosed
        );
        assert_eq!(
            d.entry_check(OpClass::Derived).unwrap_err().code,
            Code::DocumentClosed
        );
        assert!(
            d.entry_check(OpClass::DocumentStatus).is_ok()
                && d.entry_check(OpClass::Release).is_ok()
        );
    }
}
