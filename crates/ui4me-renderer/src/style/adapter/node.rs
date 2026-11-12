//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! `NodeInfo`/`TNode` on the arena (T041). Every navigation is a generation-checked arena read
//! through the pass-bound view (defence in depth: a stale id panics, it never dangles).

use super::document::NoShadowRoot;
use super::handle::StyleNode;
use crate::style::sidecar::StyleSidecar;
use std::fmt;
use std::hash::{Hash, Hasher};
use style::dom::{NodeInfo, OpaqueNode, TNode};
use ui4me_document::node::{ElementData, Node, NodeData};
use ui4me_engine_types::ids::OpaqueStyleKey;

impl<'a> StyleNode<'a> {
    pub(crate) fn node(&self) -> &'a Node {
        self.view().arena().node(self.id)
    }
    pub(crate) fn elem(&self) -> Option<&'a ElementData> {
        self.node().as_element()
    }
    pub(crate) fn is_elem(&self) -> bool {
        self.node().is_element()
    }
    /// The sidecar (traversal invariant: synced before the pass).
    pub(crate) fn sc(&self) -> &'a StyleSidecar {
        self.sidecars().of(self.id)
    }
    /// The sidecar if one exists (selector-service passes may see unsynced nodes).
    pub(crate) fn sc_opt(&self) -> Option<&'a StyleSidecar> {
        self.sidecars().get(self.id)
    }
    pub(crate) fn sibling(&self, delta: isize) -> Option<Self> {
        let parent = self.node().parent()?;
        let sibs = self.view().arena().node(parent).children();
        let pos = sibs.iter().position(|c| *c == self.id)? as isize + delta;
        if pos < 0 {
            return None;
        }
        sibs.get(pos as usize).map(|c| self.with(*c))
    }
    /// `(generation << 32) | index` as Stylo's opaque key.
    pub(crate) fn key(&self) -> usize {
        OpaqueStyleKey::of(self.id).get().get() as usize
    }
}

impl PartialEq for StyleNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for StyleNode<'_> {}
impl Hash for StyleNode<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}
impl fmt::Debug for StyleNode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.node().data() {
            NodeData::Element(e) => write!(f, "<{} {:?}>", e.local_name(), self.id),
            NodeData::Document => write!(f, "#document {:?}", self.id),
            NodeData::Text(_) => write!(f, "#text {:?}", self.id),
            _ => write!(f, "#node {:?}", self.id),
        }
    }
}

impl NodeInfo for StyleNode<'_> {
    fn is_element(&self) -> bool {
        self.is_elem()
    }
    fn is_text_node(&self) -> bool {
        matches!(self.node().data(), NodeData::Text(_))
    }
}

impl<'a> TNode for StyleNode<'a> {
    type ConcreteElement = StyleNode<'a>;
    type ConcreteDocument = StyleNode<'a>;
    type ConcreteShadowRoot = NoShadowRoot<'a>;

    fn parent_node(&self) -> Option<Self> {
        self.node().parent().map(|p| self.with(p))
    }
    fn first_child(&self) -> Option<Self> {
        self.node().children().first().map(|c| self.with(*c))
    }
    fn last_child(&self) -> Option<Self> {
        self.node().children().last().map(|c| self.with(*c))
    }
    fn prev_sibling(&self) -> Option<Self> {
        self.sibling(-1)
    }
    fn next_sibling(&self) -> Option<Self> {
        self.sibling(1)
    }
    fn owner_doc(&self) -> Self::ConcreteDocument {
        self.with(self.view().document_node())
    }
    fn is_in_document(&self) -> bool {
        self.view().arena().is_connected(self.id)
    }
    fn traversal_parent(&self) -> Option<Self::ConcreteElement> {
        self.parent_node().filter(|p| p.is_elem())
    }
    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(self.key())
    }
    fn debug_id(self) -> usize {
        self.key()
    }
    fn as_element(&self) -> Option<Self::ConcreteElement> {
        self.is_elem().then_some(*self)
    }
    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        matches!(self.node().data(), NodeData::Document).then_some(*self)
    }
    fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
        None
    }
}
