//! owner: 003-document-model
//!
//! Tree primitives and public navigation (T013, T029; FR-025, FR-029, FR-094; DOM §4.2, §4.4).
//!
//! Every traversal here is iterative (never recursive), so a 1,000,000-deep chain is safe.
//! Connectedness (`IS_CONNECTED`, data-model §2) is *computed* from the root rather than cached
//! per node: that is observably identical (invariant 5) and cannot go stale, which is the property
//! the cache exists to guarantee. Template contents are reached only through their template,
//! never as children.

use crate::arena::Arena;
use crate::document::{Document, err};
use crate::node::{NodeData, NodeKind};
use std::cmp::Ordering;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::{DocumentId, NodeId, NodeKey};
use ui4me_engine_types::status::Status;

/// `compareDocumentPosition` bits (DOM §4.4).
pub mod position {
    pub const DISCONNECTED: u16 = 1;
    pub const PRECEDING: u16 = 2;
    pub const FOLLOWING: u16 = 4;
    pub const CONTAINS: u16 = 8;
    pub const CONTAINED_BY: u16 = 16;
    pub const IMPLEMENTATION_SPECIFIC: u16 = 32;
}

impl Arena {
    /// Inclusive ancestors of `n`, nearest first (iterative).
    pub fn inclusive_ancestors(&self, n: NodeId) -> Vec<NodeId> {
        let mut out = vec![];
        let mut cur = Some(n);
        while let Some(x) = cur {
            out.push(x);
            cur = self.get(x).and_then(|y| y.parent);
        }
        out
    }

    /// Inclusive descendants of `root` in reverse tree order (last descendant first).
    pub fn subtree_reverse(&self, root: NodeId) -> Vec<NodeId> {
        let mut v = self.subtree(root);
        v.reverse();
        v
    }

    /// Tree order of two nodes of the same tree: `Less` when `a` precedes `b`; `None` when they
    /// are in different trees. A node precedes its descendants.
    pub fn tree_order(&self, a: NodeId, b: NodeId) -> Option<Ordering> {
        if a == b {
            return Some(Ordering::Equal);
        }
        let mut pa = self.inclusive_ancestors(a);
        let mut pb = self.inclusive_ancestors(b);
        pa.reverse();
        pb.reverse();
        if pa.first() != pb.first() {
            return None;
        }
        let mut i = 0;
        while i < pa.len() && i < pb.len() && pa[i] == pb[i] {
            i += 1;
        }
        // i >= 1 (same root). If one path is a prefix of the other, the shorter is the ancestor.
        if i == pa.len() {
            return Some(Ordering::Less);
        }
        if i == pb.len() {
            return Some(Ordering::Greater);
        }
        let parent = pa[i - 1];
        let kids = &self.node(parent).children;
        let ia = kids
            .iter()
            .position(|c| *c == pa[i])
            .expect("tree invariant");
        let ib = kids
            .iter()
            .position(|c| *c == pb[i])
            .expect("tree invariant");
        Some(ia.cmp(&ib))
    }

    /// The index of `n` among its parent's children.
    pub fn index_in_parent(&self, n: NodeId) -> Option<usize> {
        let p = self.get(n)?.parent?;
        self.node(p).children.iter().position(|c| *c == n)
    }
}

impl Document {
    pub fn parent_node(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        self.parent(n)
    }

    /// `parentElement`: the parent when it is an element.
    pub fn parent_element(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        Ok(self
            .check(n)?
            .parent
            .filter(|&p| self.arena.node(p).is_element()))
    }

    pub fn first_child(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        Ok(self.check(n)?.children.first().copied())
    }

    pub fn last_child(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        Ok(self.check(n)?.children.last().copied())
    }

    fn sibling(
        &self,
        n: NodeId,
        delta: isize,
        elements_only: bool,
    ) -> Result<Option<NodeId>, Status> {
        let node = self.check(n)?;
        let Some(p) = node.parent else {
            return Ok(None);
        };
        let kids = &self.arena.node(p).children;
        let mut i = kids.iter().position(|c| *c == n).expect("tree invariant") as isize + delta;
        while i >= 0 && (i as usize) < kids.len() {
            let c = kids[i as usize];
            if !elements_only || self.arena.node(c).is_element() {
                return Ok(Some(c));
            }
            i += delta;
        }
        Ok(None)
    }

    pub fn previous_sibling(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        self.sibling(n, -1, false)
    }
    pub fn next_sibling(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        self.sibling(n, 1, false)
    }
    pub fn previous_element_sibling(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        self.sibling(n, -1, true)
    }
    pub fn next_element_sibling(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        self.sibling(n, 1, true)
    }

    pub fn first_element_child(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        Ok(self
            .check(n)?
            .children
            .iter()
            .copied()
            .find(|&c| self.arena.node(c).is_element()))
    }
    pub fn last_element_child(&self, n: NodeId) -> Result<Option<NodeId>, Status> {
        Ok(self
            .check(n)?
            .children
            .iter()
            .rev()
            .copied()
            .find(|&c| self.arena.node(c).is_element()))
    }

    /// Element children, in order (static list; A-22).
    pub fn element_children(&self, n: NodeId) -> Result<Vec<NodeId>, Status> {
        Ok(self
            .check(n)?
            .children
            .iter()
            .copied()
            .filter(|&c| self.arena.node(c).is_element())
            .collect())
    }

    pub fn child_element_count(&self, n: NodeId) -> Result<usize, Status> {
        Ok(self
            .check(n)?
            .children
            .iter()
            .filter(|&&c| self.arena.node(c).is_element())
            .count())
    }

    pub fn has_child_nodes(&self, n: NodeId) -> Result<bool, Status> {
        Ok(!self.check(n)?.children.is_empty())
    }

    /// `getRootNode()`: the root of the node's tree. Inside template contents the contents
    /// fragment is the root (FR-016).
    pub fn root_node(&self, n: NodeId) -> Result<NodeId, Status> {
        self.check(n)?;
        Ok(self.arena.root_of(n))
    }

    /// `ownerDocument` is the document id for every node but the Document itself (A-23).
    pub fn owner_document(&self, n: NodeId) -> Result<Option<DocumentId>, Status> {
        let node = self.check(n)?;
        Ok(if matches!(node.data, NodeData::Document) {
            None
        } else {
            Some(self.id)
        })
    }

    /// `contains(other)`: inclusive descendant test (a node contains itself).
    pub fn contains(&self, node: NodeId, other: Option<NodeId>) -> Result<bool, Status> {
        self.check(node)?;
        match other {
            None => Ok(false),
            Some(o) => {
                self.check(o)?;
                Ok(self.arena.is_inclusive_ancestor(node, o))
            }
        }
    }

    /// `isSameNode`.
    pub fn is_same_node(&self, a: NodeId, b: Option<NodeId>) -> Result<bool, Status> {
        self.check(a)?;
        if let Some(b) = b {
            self.check(b)?;
        }
        Ok(b == Some(a))
    }

    /// `compareDocumentPosition(other)` of two nodes of this document (DOM §4.4).
    pub fn compare_document_position(&self, node: NodeId, other: NodeId) -> Result<u16, Status> {
        self.check(node)?;
        self.check(other)?;
        use position::*;
        if node == other {
            return Ok(0);
        }
        match self.arena.tree_order(node, other) {
            None => {
                // Different trees: DISCONNECTED | IMPLEMENTATION_SPECIFIC with a consistent
                // PRECEDING/FOLLOWING choice (by slot order, stable for a pair).
                let pre = if node.raw().index < other.raw().index {
                    FOLLOWING
                } else {
                    PRECEDING
                };
                Ok(DISCONNECTED | IMPLEMENTATION_SPECIFIC | pre)
            }
            Some(_) if self.arena.is_inclusive_ancestor(other, node) => Ok(CONTAINS | PRECEDING),
            Some(_) if self.arena.is_inclusive_ancestor(node, other) => {
                Ok(CONTAINED_BY | FOLLOWING)
            }
            // `other` precedes `node` in tree order => PRECEDING; else FOLLOWING.
            Some(Ordering::Greater) => Ok(PRECEDING),
            Some(_) => Ok(FOLLOWING),
        }
    }

    /// Read-only cross-document `compareDocumentPosition` (no `WRONG_DOCUMENT`; spec Q2).
    pub fn compare_document_position_keys(&self, a: NodeKey, b: NodeKey) -> Result<u16, Status> {
        use position::*;
        if a.document == self.id && b.document == self.id {
            return self.compare_document_position(a.node, b.node);
        }
        // Stale own-document handles still outrank the DOM result.
        for k in [a, b] {
            if k.document == self.id && !self.arena.contains(k.node) {
                return Err(err(Code::StaleHandle));
            }
        }
        self.check_open()?;
        let pre = if a.document.raw() < b.document.raw() {
            FOLLOWING
        } else {
            PRECEDING
        };
        Ok(DISCONNECTED | IMPLEMENTATION_SPECIFIC | pre)
    }

    /// `isEqualNode` of two nodes of this document (iterative; DOM §4.4).
    pub fn is_equal_node(&self, a: NodeId, b: Option<NodeId>) -> Result<bool, Status> {
        self.check(a)?;
        let Some(b) = b else { return Ok(false) };
        self.check(b)?;
        let mut stack = vec![(a, b)];
        while let Some((x, y)) = stack.pop() {
            let (nx, ny) = (self.arena.node(x), self.arena.node(y));
            if nx.kind() != ny.kind() {
                return Ok(false);
            }
            let same_shallow = match (&nx.data, &ny.data) {
                (NodeData::Doctype(p), NodeData::Doctype(q)) => p == q,
                (NodeData::Text(p), NodeData::Text(q))
                | (NodeData::Comment(p), NodeData::Comment(q)) => p == q,
                (NodeData::Element(p), NodeData::Element(q)) => {
                    p.name.ns == q.name.ns
                        && p.name.prefix == q.name.prefix
                        && p.name.local == q.name.local
                        && p.attrs.len() == q.attrs.len()
                        && p.attrs.iter().all(|at| {
                            q.attrs.iter().any(|bt| {
                                bt.name.ns == at.name.ns
                                    && bt.name.local == at.name.local
                                    && bt.value == at.value
                            })
                        })
                }
                _ => true,
            };
            if !same_shallow || nx.children.len() != ny.children.len() {
                return Ok(false);
            }
            stack.extend(nx.children.iter().copied().zip(ny.children.iter().copied()));
        }
        Ok(true)
    }

    /// Whether `node` is an inclusive ancestor of `descendant` (both live).
    pub fn is_inclusive_ancestor(&self, node: NodeId, descendant: NodeId) -> Result<bool, Status> {
        self.contains(node, Some(descendant))
    }

    /// `nodeType` numeric value (A-20).
    pub fn node_type(&self, n: NodeId) -> Result<u16, Status> {
        Ok(match self.check(n)?.kind() {
            NodeKind::Element => 1,
            NodeKind::Text => 3,
            NodeKind::Comment => 8,
            NodeKind::Document => 9,
            NodeKind::Doctype => 10,
            NodeKind::DocumentFragment => 11,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::position::*;
    use super::*;

    fn doc() -> Document {
        Document::parse_html("<div id=a><p id=b>t<i id=c></i></p><span id=d></span></div>").unwrap()
    }

    #[test]
    fn navigation_reads() {
        let d = doc();
        let a = d.element_by_id("a").unwrap();
        let (b, c, s) = (
            d.element_by_id("b").unwrap(),
            d.element_by_id("c").unwrap(),
            d.element_by_id("d").unwrap(),
        );
        assert_eq!(d.first_child(a).unwrap(), Some(b));
        assert_eq!(d.last_child(a).unwrap(), Some(s));
        assert_eq!(d.next_sibling(b).unwrap(), Some(s));
        assert_eq!(d.previous_element_sibling(s).unwrap(), Some(b));
        assert_eq!(d.parent_element(c).unwrap(), Some(b));
        assert_eq!(
            d.parent_element(d.document_element().unwrap()).unwrap(),
            None
        );
        assert_eq!(d.child_element_count(b).unwrap(), 1);
        assert_eq!(d.element_children(a).unwrap(), vec![b, s]);
        assert!(d.has_child_nodes(b).unwrap());
        assert!(!d.has_child_nodes(s).unwrap());
        assert_eq!(d.owner_document(a).unwrap(), Some(d.id()));
        assert_eq!(d.owner_document(d.document_node()).unwrap(), None);
        assert_eq!(d.root_node(c).unwrap(), d.document_node());
        assert_eq!(d.node_type(a).unwrap(), 1);
    }

    #[test]
    fn contains_and_compare_position() {
        let d = doc();
        let (a, b, c, s) = (
            d.element_by_id("a").unwrap(),
            d.element_by_id("b").unwrap(),
            d.element_by_id("c").unwrap(),
            d.element_by_id("d").unwrap(),
        );
        assert!(d.contains(a, Some(c)).unwrap());
        assert!(d.contains(a, Some(a)).unwrap());
        assert!(!d.contains(c, Some(a)).unwrap());
        assert!(!d.contains(a, None).unwrap());
        assert_eq!(d.compare_document_position(a, a).unwrap(), 0);
        assert_eq!(d.compare_document_position(b, s).unwrap(), FOLLOWING);
        assert_eq!(d.compare_document_position(s, b).unwrap(), PRECEDING);
        assert_eq!(
            d.compare_document_position(a, c).unwrap(),
            CONTAINED_BY | FOLLOWING
        );
        assert_eq!(
            d.compare_document_position(c, a).unwrap(),
            CONTAINS | PRECEDING
        );
    }

    #[test]
    fn detached_trees_are_disconnected_and_cross_document_comparison_is_read_only() {
        let mut d = doc();
        let x = d.create_element("x").unwrap();
        let a = d.element_by_id("a").unwrap();
        let p = d.compare_document_position(a, x).unwrap();
        assert_eq!(p & DISCONNECTED, DISCONNECTED);
        assert_eq!(p & IMPLEMENTATION_SPECIFIC, IMPLEMENTATION_SPECIFIC);
        assert_eq!(
            d.compare_document_position(x, a).unwrap() & (PRECEDING | FOLLOWING),
            if p & PRECEDING != 0 {
                FOLLOWING
            } else {
                PRECEDING
            }
        );
        let other = Document::parse_html("<p>q</p>").unwrap();
        let k1 = d.key(a);
        let k2 = other.key(other.document_element().unwrap());
        let r = d.compare_document_position_keys(k1, k2).unwrap();
        assert_eq!(r & DISCONNECTED, DISCONNECTED);
        assert!(!d.is_connected(x).unwrap());
    }

    #[test]
    fn is_equal_node_compares_structure_and_attributes_unordered() {
        let d = Document::parse_html(
            "<div><p a=1 b=2>x</p><p b=2 a=1>x</p><p a=1>x</p><p a=1 b=2>y</p></div>",
        )
        .unwrap();
        let ps: Vec<_> = d
            .element_children(d.element_children(d.body().unwrap()).unwrap()[0])
            .unwrap();
        assert!(d.is_equal_node(ps[0], Some(ps[1])).unwrap());
        assert!(!d.is_equal_node(ps[0], Some(ps[2])).unwrap());
        assert!(!d.is_equal_node(ps[0], Some(ps[3])).unwrap());
        assert!(!d.is_equal_node(ps[0], None).unwrap());
        assert!(d.is_same_node(ps[0], Some(ps[0])).unwrap());
    }

    #[test]
    fn a_million_deep_chain_is_traversed_without_stack_growth() {
        let mut d = Document::new_empty(ui4me_engine_types::ids::DocumentId::from_raw(
            ui4me_engine_types::ids::RawId {
                index: 5,
                generation: std::num::NonZeroU32::MIN,
            },
        ));
        let mut cur = d.create_element("div").unwrap();
        let top = cur;
        d.append_child(d.document_node(), top).unwrap();
        for _ in 0..1_000_000 {
            let n = d.create_element("div").unwrap();
            // Raw link (the public insert's cycle check is O(depth); building is not under test).
            d.arena.get_mut(n).unwrap().parent = Some(cur);
            d.arena.get_mut(cur).unwrap().children.push(n);
            cur = n;
        }
        assert_eq!(d.arena.subtree(top).len(), 1_000_001);
        assert_eq!(d.arena.inclusive_ancestors(cur).len(), 1_000_002);
        assert!(d.arena.is_connected(cur));
        assert_eq!(d.arena.subtree_reverse(top).first(), Some(&cur));
        assert_eq!(
            d.compare_document_position(top, cur).unwrap(),
            CONTAINED_BY | FOLLOWING
        );
    }
}
