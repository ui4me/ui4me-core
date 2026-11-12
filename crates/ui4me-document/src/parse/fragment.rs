//! owner: 003-document-model
//!
//! HTML fragment parsing (T070; research R-05; FR-008, FR-017, FR-047, FR-092, FR-093).
//!
//! `parse_fragment_for_element` runs the HTML fragment parsing algorithm with a context element of
//! this document, into a **staging** DocumentFragment in the same arena: the real tree is not
//! touched, so a failure (node limit, allocation failure) frees the staging nodes and leaves the
//! document, journal and revision exactly as they were. The nearest `form` ancestor of the context
//! is the parser's form pointer (parser association), and the result inherits the document's mode.

use crate::arena::Arena;
use crate::document::{Document, DocumentMode};
use crate::errors::{no_modification_allowed, size_limit, wrong_node_kind};
use crate::limits::LimitName;
use crate::node::NodeData;
use crate::parse::{parse_opts, sink::ArenaSink};
use html5ever::driver::parse_fragment_for_element;
use html5ever::interface::QuirksMode;
use html5ever::tendril::TendrilSink;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

impl Document {
    /// Detach the children of an internal staging fragment without journaling or hooks (they were
    /// never visible) and free the fragment; returns the detached nodes in order.
    fn take_staging_children(&mut self, frag: NodeId) -> Vec<NodeId> {
        let kids = self.arena.node(frag).children.clone();
        for &k in &kids {
            self.arena.detach(k);
        }
        self.arena.free_subtree(frag);
        kids
    }

    /// The fragment-parsing context for markup inserted next to / into `parent`: an element is its
    /// own context; for anything else (a DocumentFragment parent) or an `html` element in an HTML
    /// document, a new `body` element is used (HTML "fragment parsing" context rule).
    fn context_for(&mut self, parent: NodeId) -> Result<(NodeId, bool), Status> {
        let node = self.arena.node(parent);
        let is_html_root = node.as_element().is_some_and(|e| e.is_html_named("html"));
        if node.is_element() && !is_html_root {
            return Ok((parent, false));
        }
        self.check_node_budget(1)?;
        let body = self.alloc_element(
            html5ever::QualName::new(
                None,
                html5ever::ns!(html),
                html5ever::LocalName::from("body"),
            ),
            vec![],
        );
        Ok((body, true))
    }

    /// `innerHTML` setter (A-49): parse with the element as context and replace its children (a
    /// `template`'s contents) in ONE atomic step. Any failure leaves the target unchanged.
    pub fn set_inner_html(&mut self, el: NodeId, html: &str) -> Result<(), Status> {
        self.set_inner_html_checked(el, html, |_, _| Ok(()))
    }

    /// Parse into staging, validate the proposed tree, then commit exactly once.
    /// Validation failure frees all staged nodes without a revision or removal hook.
    pub fn set_inner_html_checked(
        &mut self,
        el: NodeId,
        html: &str,
        validate: impl FnOnce(&Document, NodeId) -> Result<(), Status>,
    ) -> Result<(), Status> {
        if !self.check(el)?.is_element() {
            return Err(wrong_node_kind());
        }
        let frag = self.parse_fragment_for_element(el, html)?;
        if let Err(error) = validate(self, frag) {
            self.arena.free_subtree(frag);
            return Err(error);
        }
        let kids = self.take_staging_children(frag);
        let container = self
            .arena
            .node(el)
            .as_element()
            .and_then(|e| e.template_contents())
            .unwrap_or(el);
        let old = self.arena.node(container).children.clone();
        if kids.is_empty() && old.is_empty() {
            return Ok(());
        }
        self.begin_call();
        let rev = self.pending_revision();
        if kids.is_empty() {
            self.remove_nodes_step(rev, container, &old);
        } else {
            self.remove_nodes_ex(rev, container, &old, false);
            self.insert_step_ex(rev, container, kids, None, old);
        }
        self.end_call();
        Ok(())
    }

    /// `outerHTML` setter (A-49): replace the element with the parsed fragment. No parent is no
    /// effect; a Document parent is `NO_MODIFICATION`.
    pub fn set_outer_html(&mut self, el: NodeId, html: &str) -> Result<(), Status> {
        if !self.check(el)?.is_element() {
            return Err(wrong_node_kind());
        }
        let Some(parent) = self.arena.node(el).parent else {
            return Ok(());
        };
        if self.arena.node(parent).kind() == crate::node::NodeKind::Document {
            return Err(no_modification_allowed());
        }
        let (ctx, temp) = self.context_for(parent)?;
        let parsed = self.parse_fragment_for_element(ctx, html);
        if temp {
            self.arena.free_subtree(ctx);
        }
        let kids = self.take_staging_children(parsed?);
        if kids.is_empty() {
            return self.remove_child(parent, el).map(|_| ());
        }
        self.apply_replace(parent, kids, el);
        Ok(())
    }

    /// `insertAdjacentHTML(position, markup)` (A-50): `SYNTAX` for an unknown position,
    /// `NO_MODIFICATION` when `beforebegin`/`afterend` has a Document parent, no effect without a parent.
    pub fn insert_adjacent_html(
        &mut self,
        el: NodeId,
        position: &str,
        html: &str,
    ) -> Result<(), Status> {
        self.insert_adjacent_html_checked(el, position, html, |_, _| Ok(()))
    }

    /// [`Self::insert_adjacent_html`] with a host validation of the detached staging fragment
    /// before anything is inserted; a rejection frees the fragment and leaves the tree unchanged.
    pub fn insert_adjacent_html_checked(
        &mut self,
        el: NodeId,
        position: &str,
        html: &str,
        validate: impl FnOnce(&Document, NodeId) -> Result<(), Status>,
    ) -> Result<(), Status> {
        let Some((parent, reference)) = self.adjacent_target(el, position)? else {
            return Ok(());
        };
        let pos = position.to_ascii_lowercase();
        let outside = matches!(pos.as_str(), "beforebegin" | "afterend");
        if outside && self.arena.node(parent).kind() == crate::node::NodeKind::Document {
            return Err(no_modification_allowed());
        }
        let ctx_src = if outside { parent } else { el };
        let (ctx, temp) = self.context_for(ctx_src)?;
        let parsed = self.parse_fragment_for_element(ctx, html);
        if temp {
            self.arena.free_subtree(ctx);
        }
        let frag = parsed?;
        if let Err(error) = validate(self, frag) {
            self.arena.free_subtree(frag);
            return Err(error);
        }
        let kids = self.take_staging_children(frag);
        if kids.is_empty() {
            return Ok(());
        }
        self.apply_insert(parent, kids, reference);
        Ok(())
    }

    /// Parse `html` as a fragment with `context` as the context element; returns a detached
    /// staging DocumentFragment holding the result (A-49/A-50 building block).
    pub fn parse_fragment_for_element(
        &mut self,
        context: NodeId,
        html: &str,
    ) -> Result<NodeId, Status> {
        if !self.check(context)?.is_element() {
            return Err(wrong_node_kind());
        }
        self.limits
            .check(LimitName::InputBytes, html.len() as u64)?;
        self.check_node_budget(0)?;
        let quirks = match self.mode {
            DocumentMode::NoQuirks => QuirksMode::NoQuirks,
            DocumentMode::LimitedQuirks => QuirksMode::LimitedQuirks,
            DocumentMode::Quirks => QuirksMode::Quirks,
        };
        // The nearest form ancestor (inclusive) is the form element pointer.
        let form = self
            .arena
            .inclusive_ancestors(context)
            .into_iter()
            .find(|&a| {
                self.arena
                    .node(a)
                    .as_element()
                    .is_some_and(|e| e.is_html_named("form"))
            });
        let nodes_before = self.arena.live_count();
        let mut arena = std::mem::replace(&mut self.arena, Arena::new());
        let scratch = arena.alloc(NodeData::DocumentFragment { template: None });
        let sink = ArenaSink::with_root(arena, scratch);
        let out =
            parse_fragment_for_element(sink, parse_opts(quirks), context, false, form).one(html);
        self.arena = out.arena;
        // The tree builder appended its synthetic `html` root under the scratch root; the result is
        // that root's children.
        let root_el = self
            .arena
            .node(scratch)
            .children
            .iter()
            .copied()
            .find(|&c| self.arena.node(c).is_element());
        let frag = self
            .arena
            .alloc(NodeData::DocumentFragment { template: None });
        if let Some(r) = root_el {
            let kids = self.arena.node(r).children.clone();
            for k in kids {
                self.arena.insert_trusted(frag, k, None);
            }
        }
        self.arena.free_subtree(scratch);
        super::apply_selectedcontent(&mut self.arena, frag);
        let added = self.arena.live_count().saturating_sub(nodes_before) as u64;
        if self.arena.live_count() as u64 > self.limits.nodes {
            self.arena.free_subtree(frag);
            return Err(size_limit(LimitName::Nodes.name()));
        }
        let _ = added;
        Ok(frag)
    }
}
