//! owner: 003-document-model — created by 001 slice (R-28)
//!
//! DocumentCore's part of DOM queries (T037; A-07, A-09, A-51): `getElementById`, and the
//! scope, order and errors of `querySelector(All)`/`matches`/`closest`. **Selector parsing and
//! matching are 006's service** (`ui4me-style/src/query.rs`, R-12): there is no second matcher
//! here. [`Document::scope_for_query`] validates the root and lends the structure-frozen view
//! for one service call (`dom-internal-interfaces.md` §1); [`Document::scope_elements`] lists
//! the candidates (the root's descendant elements in tree order) the service matches.

use crate::document::{Document, err};
use crate::node::NodeKind;
use ui4me_engine_types::codes_generated::{Code, SubReason};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// A validated query root (Document, DocumentFragment or Element).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryScope {
    pub root: NodeId,
}

impl Document {
    /// `getElementById` (A-07): the first connected element in tree order whose `id` equals
    /// `id`; the empty string never matches.
    pub fn element_by_id(&self, id: &str) -> Option<NodeId> {
        if id.is_empty() || self.check_open().is_err() {
            return None;
        }
        self.arena
            .subtree(self.arena.document())
            .into_iter()
            .find(|&n| self.arena.node(n).as_element().and_then(|e| e.attr("id")) == Some(id))
    }

    /// Validate a query root (handles, lifecycle, root kind; FR-021 precedence).
    pub fn scope_for_query(&self, root: NodeId) -> Result<QueryScope, Status> {
        match self.check(root)?.kind() {
            NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element => {
                Ok(QueryScope { root })
            }
            _ => Err(err(Code::InvalidArgument).with_sub_reason(SubReason::WrongNodeKind)),
        }
    }

    /// Candidate elements of a scope: the root's descendant elements in tree order (template
    /// contents are not descendants). A static list.
    pub fn scope_elements(&self, scope: &QueryScope) -> Vec<NodeId> {
        self.arena
            .subtree(scope.root)
            .into_iter()
            .skip(1)
            .filter(|&n| self.arena.node(n).is_element())
            .collect()
    }

    /// Inclusive ancestor elements of `element`, nearest first (`closest`).
    pub fn inclusive_ancestor_elements(&self, element: NodeId) -> Result<Vec<NodeId>, Status> {
        self.check(element)?;
        let mut out = vec![];
        let mut cur = Some(element);
        while let Some(n) = cur {
            let node = self.arena.node(n);
            if node.is_element() {
                out.push(n);
            }
            cur = node.parent;
        }
        Ok(out)
    }

    // ------------------------------------------------------------ non-selector queries (T061; A-07, A-08)

    /// `getElementById` on a DocumentFragment (or any tree root): the first element in tree order
    /// whose `id` equals `id`; the empty string never matches. Template contents are not descendants.
    pub fn fragment_element_by_id(
        &self,
        fragment: NodeId,
        id: &str,
    ) -> Result<Option<NodeId>, Status> {
        self.check(fragment)?;
        if id.is_empty() {
            return Ok(None);
        }
        Ok(self
            .arena
            .subtree(fragment)
            .into_iter()
            .skip(1)
            .find(|&n| self.arena.node(n).as_element().and_then(|e| e.attr("id")) == Some(id)))
    }

    fn descendant_elements(&self, root: NodeId) -> Result<Vec<NodeId>, Status> {
        match self.check(root)?.kind() {
            NodeKind::Document | NodeKind::Element | NodeKind::DocumentFragment => {}
            _ => return Err(err(Code::InvalidArgument).with_sub_reason(SubReason::WrongNodeKind)),
        }
        Ok(self
            .arena
            .subtree(root)
            .into_iter()
            .skip(1)
            .filter(|&n| self.arena.node(n).is_element())
            .collect())
    }

    /// `getElementsByTagName(name)` (A-08): static list in tree order; `*` matches all; for HTML
    /// elements the comparison is against the lowercased name, for others exact (DOM section 4.2.6).
    pub fn get_elements_by_tag_name(
        &self,
        root: NodeId,
        name: &str,
    ) -> Result<Vec<NodeId>, Status> {
        let all = self.descendant_elements(root)?;
        if name == "*" {
            return Ok(all);
        }
        let lower = name.to_ascii_lowercase();
        Ok(all
            .into_iter()
            .filter(|&n| {
                let e = self.arena.node(n).as_element().expect("element");
                let q = crate::attrs::qualified_name(e.name());
                if e.is_html() { q == lower } else { q == name }
            })
            .collect())
    }

    /// `getElementsByTagNameNS(namespace, localName)` (A-08): `*` wildcards on either side.
    pub fn get_elements_by_tag_name_ns(
        &self,
        root: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Result<Vec<NodeId>, Status> {
        let all = self.descendant_elements(root)?;
        Ok(all
            .into_iter()
            .filter(|&n| {
                let e = self.arena.node(n).as_element().expect("element");
                let ns_ok = namespace == Some("*")
                    || &*e.name().ns == namespace.filter(|s| !s.is_empty()).unwrap_or("");
                ns_ok && (local == "*" || e.local_name() == local)
            })
            .collect())
    }

    /// `getElementsByClassName(classes)` (A-08): elements having every listed class; ASCII
    /// case-insensitive in quirks mode; no classes gives an empty list.
    pub fn get_elements_by_class_name(
        &self,
        root: NodeId,
        classes: &str,
    ) -> Result<Vec<NodeId>, Status> {
        let all = self.descendant_elements(root)?;
        let want: Vec<&str> = classes
            .split([' ', '\t', '\n', '\x0c', '\r'])
            .filter(|s| !s.is_empty())
            .collect();
        if want.is_empty() {
            return Ok(vec![]);
        }
        let quirks = self.mode == crate::document::DocumentMode::Quirks;
        Ok(all
            .into_iter()
            .filter(|&n| {
                let e = self.arena.node(n).as_element().expect("element");
                want.iter().all(|w| {
                    e.classes().any(|c| {
                        if quirks {
                            c.eq_ignore_ascii_case(w)
                        } else {
                            c == *w
                        }
                    })
                })
            })
            .collect())
    }

    /// Document `getElementsByName(name)` (A-08): HTML elements with `name=name`, tree order.
    pub fn get_elements_by_name(&self, name: &str) -> Result<Vec<NodeId>, Status> {
        self.check_open()?;
        Ok(self
            .arena
            .subtree(self.arena.document())
            .into_iter()
            .filter(|&n| {
                self.arena
                    .node(n)
                    .as_element()
                    .is_some_and(|e| e.is_html() && e.attr("name") == Some(name))
            })
            .collect())
    }

    /// `children` (A-30): element children, a static list.
    pub fn children_elements(&self, n: NodeId) -> Result<Vec<NodeId>, Status> {
        self.element_children(n)
    }

    /// Convert query results to a static list for the ABI (FR-030): validates every id and keeps the
    /// given tree order.
    pub fn into_static_list(&self, nodes: &[NodeId]) -> Result<Vec<NodeId>, Status> {
        for &n in nodes {
            self.check(n)?;
        }
        Ok(nodes.to_vec())
    }
}
