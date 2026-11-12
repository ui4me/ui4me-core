//! owner: 003-document-model
//!
//! Pre-insert and replace validation (T046; FR-048–FR-054; DOM "ensure pre-insert validity",
//! `dom.bs:2742-2830`, and the `replace` steps), in DOM step order and mapped through Table B.
//! Validation is read-only: it runs before any mutation, so a failing call changes nothing.
//! Nodes of another document are rejected at the boundary with `WRONG_DOCUMENT` (no implicit
//! adoption; FR-052) before these checks run (see `Precheck`).

use crate::document::Document;
use crate::errors::{hierarchy_request, not_found};
use crate::node::{NodeData, NodeKind};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// One operand of `append`/`prepend`/`before`/`after`/`replaceWith`/`replaceChildren`: a node or a
/// string that becomes a Text node (FR-048).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeOrString {
    Node(NodeId),
    Text(String),
}

impl Document {
    /// "host-including inclusive ancestor": walks up through a template contents fragment to its
    /// template element, so a template cannot be inserted into its own contents.
    pub(crate) fn is_host_including_inclusive_ancestor(&self, node: NodeId, of: NodeId) -> bool {
        let mut cur = Some(of);
        while let Some(n) = cur {
            if n == node {
                return true;
            }
            let x = self.arena.node(n);
            cur = match x.parent {
                Some(p) => Some(p),
                None => match x.data {
                    NodeData::DocumentFragment { template: Some(t) } => Some(t),
                    _ => None,
                },
            };
        }
        false
    }

    fn element_children_of(&self, n: NodeId) -> Vec<NodeId> {
        self.arena
            .node(n)
            .children
            .iter()
            .copied()
            .filter(|&c| self.arena.node(c).is_element())
            .collect()
    }

    fn has_doctype_after(&self, parent: NodeId, child: NodeId) -> bool {
        let kids = &self.arena.node(parent).children;
        match kids.iter().position(|c| *c == child) {
            Some(i) => kids[i + 1..]
                .iter()
                .any(|&c| self.arena.node(c).kind() == NodeKind::Doctype),
            None => false,
        }
    }

    fn has_element_before(&self, parent: NodeId, child: NodeId) -> bool {
        let kids = &self.arena.node(parent).children;
        match kids.iter().position(|c| *c == child) {
            Some(i) => kids[..i].iter().any(|&c| self.arena.node(c).is_element()),
            None => false,
        }
    }

    /// Kind and parent-type checks shared by pre-insert and replace (steps 1, 2, 4, 5).
    fn validate_common(
        &self,
        parent: NodeId,
        node: NodeId,
    ) -> Result<(NodeKind, NodeKind), Status> {
        let pk = self.arena.node(parent).kind();
        // 1. Parent must be Document, DocumentFragment or Element.
        if !matches!(
            pk,
            NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element
        ) {
            return Err(hierarchy_request());
        }
        // 2. node must not be a host-including inclusive ancestor of parent.
        if self.is_host_including_inclusive_ancestor(node, parent) {
            return Err(hierarchy_request());
        }
        let nk = self.arena.node(node).kind();
        // 4. node must be a DocumentFragment, DocumentType, Element, Text or Comment.
        if nk == NodeKind::Document {
            return Err(hierarchy_request());
        }
        // 5. Text under a Document, or a DocumentType under a non-Document.
        if (nk == NodeKind::Text && pk == NodeKind::Document)
            || (nk == NodeKind::Doctype && pk != NodeKind::Document)
        {
            return Err(hierarchy_request());
        }
        Ok((pk, nk))
    }

    /// DOM "ensure pre-insert validity" of inserting `node` into `parent` before `child`.
    pub(crate) fn validate_pre_insert(
        &self,
        parent: NodeId,
        node: NodeId,
        child: Option<NodeId>,
    ) -> Result<(), Status> {
        let (pk, nk) = self.validate_common(parent, node)?;
        // 3. child's parent must be parent.
        if let Some(c) = child
            && self.arena.node(c).parent != Some(parent)
        {
            return Err(not_found());
        }
        if pk != NodeKind::Document {
            return Ok(());
        }
        // 6. Document parent: at most one element child, doctype before the element.
        let child_is_doctype =
            child.is_some_and(|c| self.arena.node(c).kind() == NodeKind::Doctype);
        let doctype_follows_child = child.is_some_and(|c| self.has_doctype_after(parent, c));
        match nk {
            NodeKind::DocumentFragment => {
                let elems = self.element_children_of(node).len();
                let has_text = self
                    .arena
                    .node(node)
                    .children
                    .iter()
                    .any(|&c| self.arena.node(c).kind() == NodeKind::Text);
                if elems > 1 || has_text {
                    return Err(hierarchy_request());
                }
                if elems == 1
                    && (!self.element_children_of(parent).is_empty()
                        || child_is_doctype
                        || doctype_follows_child)
                {
                    return Err(hierarchy_request());
                }
            }
            NodeKind::Element => {
                if !self.element_children_of(parent).is_empty()
                    || child_is_doctype
                    || doctype_follows_child
                {
                    return Err(hierarchy_request());
                }
            }
            NodeKind::Doctype => {
                let has_doctype = self
                    .arena
                    .node(parent)
                    .children
                    .iter()
                    .any(|&c| self.arena.node(c).kind() == NodeKind::Doctype);
                let elem_before = child.is_some_and(|c| self.has_element_before(parent, c));
                let no_child_but_element =
                    child.is_none() && !self.element_children_of(parent).is_empty();
                if has_doctype || elem_before || no_child_but_element {
                    return Err(hierarchy_request());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// DOM `replace` validity of replacing `child` of `parent` with `node` (`replaceChild`).
    pub(crate) fn validate_replace(
        &self,
        parent: NodeId,
        node: NodeId,
        child: NodeId,
    ) -> Result<(), Status> {
        let (pk, nk) = self.validate_common(parent, node)?;
        // 3. child's parent must be parent.
        if self.arena.node(child).parent != Some(parent) {
            return Err(not_found());
        }
        if pk != NodeKind::Document {
            return Ok(());
        }
        let other_element_child = |this: &Self| {
            this.element_children_of(parent)
                .into_iter()
                .any(|e| e != child)
        };
        match nk {
            NodeKind::DocumentFragment => {
                let elems = self.element_children_of(node).len();
                let has_text = self
                    .arena
                    .node(node)
                    .children
                    .iter()
                    .any(|&c| self.arena.node(c).kind() == NodeKind::Text);
                if elems > 1 || has_text {
                    return Err(hierarchy_request());
                }
                if elems == 1
                    && (other_element_child(self) || self.has_doctype_after(parent, child))
                {
                    return Err(hierarchy_request());
                }
            }
            NodeKind::Element => {
                if other_element_child(self) || self.has_doctype_after(parent, child) {
                    return Err(hierarchy_request());
                }
            }
            NodeKind::Doctype => {
                let other_doctype = self
                    .arena
                    .node(parent)
                    .children
                    .iter()
                    .any(|&c| c != child && self.arena.node(c).kind() == NodeKind::Doctype);
                if other_doctype || self.has_element_before(parent, child) {
                    return Err(hierarchy_request());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Validate inserting the aggregate of `items` (already resolved to node ids, strings turned
    /// into fresh detached Text nodes) into `parent` before `child`, as DOM's "convert nodes into a
    /// node" followed by pre-insert would: a single item is validated as itself, several items as
    /// the fragment they would form (no doctype, no text under a Document, at most one element).
    pub(crate) fn validate_insert_aggregate(
        &self,
        parent: NodeId,
        items: &[NodeId],
        child: Option<NodeId>,
    ) -> Result<(), Status> {
        if let [one] = items {
            return self.validate_pre_insert(parent, *one, child);
        }
        // Several operands: each must individually be insertable, then the fragment rules.
        let mut flat = vec![];
        for &i in items {
            if self.arena.node(i).kind() == NodeKind::DocumentFragment {
                flat.extend(self.arena.node(i).children.iter().copied());
            } else {
                flat.push(i);
            }
        }
        for &n in &flat {
            self.validate_common(parent, n)?;
            if self.arena.node(n).kind() == NodeKind::Doctype {
                return Err(hierarchy_request());
            }
        }
        if let Some(c) = child
            && self.arena.node(c).parent != Some(parent)
        {
            return Err(not_found());
        }
        if self.arena.node(parent).kind() == NodeKind::Document {
            let elems = flat
                .iter()
                .filter(|&&n| self.arena.node(n).is_element())
                .count();
            let child_is_doctype =
                child.is_some_and(|c| self.arena.node(c).kind() == NodeKind::Doctype);
            let doctype_follows = child.is_some_and(|c| self.has_doctype_after(parent, c));
            if elems > 1
                || (elems == 1
                    && (!self.element_children_of(parent).is_empty()
                        || child_is_doctype
                        || doctype_follows))
            {
                return Err(hierarchy_request());
            }
        }
        Ok(())
    }
}
