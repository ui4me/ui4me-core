//! owner: 003-document-model — created by 001 slice (R-28)
//!
//! Generation-checked node arena (T035; AD-29). Identity is `ui4me_engine_types::ids::NodeId`
//! (`{index, generation}`) allocated from `ui4me_engine_types::ids::Slots`: freeing a node bumps its
//! slot's generation, and a slot whose generation would overflow is retired for good, so a
//! stale id never names a newer node (no ABA; X-STYLE-ADAPTER review F7).
//!
//! **Tree invariant** (X-STYLE-ADAPTER review F2): every node has at most one parent, appears
//! in exactly one child list (its parent's), and no node is its own ancestor. All structural
//! writes are `pub(crate)` and go through [`Arena::insert`], which rejects self-insertion and
//! cycles and detaches the child from its old parent first. The style adapter's `SAFETY`
//! argument (each element is visited once per traversal) relies on this.

use crate::node::{Node, NodeData};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::{NodeId, OpaqueStyleKey, Slots};

/// Structural errors (mapped to canonical codes by the callers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaError {
    Stale(NodeId),
    /// Insertion would make a node its own ancestor (DOM HierarchyRequestError).
    Cycle,
}

impl ArenaError {
    pub fn code(self) -> Code {
        match self {
            ArenaError::Stale(_) => Code::StaleHandle,
            ArenaError::Cycle => Code::InvalidHierarchy,
        }
    }
}

#[derive(Debug)]
pub struct Arena {
    slots: Slots,
    /// Indexed by slot index; `None` for a freed (or retired) slot.
    nodes: Vec<Option<Node>>,
    document: NodeId,
    live: usize,
    undo: Option<UndoLog>,
    /// Nodes that were allocated or detached since the last collection checkpoint: the only places a
    /// tree can have become an unreferenced detached tree (research R-03).
    candidates: Vec<NodeId>,
}

impl Arena {
    /// A new arena holding only the Document node.
    pub(crate) fn new() -> Arena {
        let mut a = Arena {
            slots: Slots::new(),
            nodes: vec![],
            live: 0,
            undo: None,
            candidates: vec![],
            document: NodeId::from_raw(ui4me_engine_types::ids::RawId {
                index: 0,
                generation: std::num::NonZeroU32::MIN,
            }),
        };
        a.document = a.alloc(NodeData::Document);
        a
    }

    /// The Document node (private field; review R2-3).
    pub fn document(&self) -> NodeId {
        self.document
    }

    pub(crate) fn alloc(&mut self, data: NodeData) -> NodeId {
        let raw = self.slots.alloc();
        let id = NodeId::from_raw(raw);
        let i = raw.index as usize;
        if self.nodes.len() <= i {
            self.nodes.resize_with(i + 1, || None);
        }
        debug_assert!(self.nodes[i].is_none());
        self.live += 1;
        self.candidates.push(id);
        if let Some(u) = self.undo.as_mut() {
            u.created.insert(raw.index);
            u.created_ids.push(id);
        }
        self.nodes[i] = Some(Node {
            id,
            parent: None,
            children: vec![],
            data,
            host_refs: 0,
            frame_refs: 0,
        });
        id
    }

    /// Generation-checked lookup.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        if !self.slots.is_live(id.raw()) {
            return None;
        }
        self.nodes.get(id.raw().index as usize)?.as_ref()
    }

    pub(crate) fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        if !self.slots.is_live(id.raw()) {
            return None;
        }
        let i = id.raw().index;
        if let Some(u) = self.undo.as_mut()
            && !u.created.contains(&i)
            && !u.before.contains_key(&i)
            && let Some(n) = self.nodes.get(i as usize).and_then(|x| x.as_ref())
        {
            u.before.insert(i, n.clone());
        }
        self.nodes.get_mut(i as usize)?.as_mut()
    }

    // ------------------------------------------------------------ batch undo (T078)

    /// Start recording before-images of every node written through [`Arena::get_mut`] and the
    /// slots allocated from now on, so [`Arena::rollback_undo`] restores the arena exactly.
    pub(crate) fn begin_undo(&mut self) {
        debug_assert!(self.undo.is_none(), "undo scopes do not nest");
        self.undo = Some(UndoLog::default());
    }

    /// Keep the changes; drop the log.
    pub(crate) fn commit_undo(&mut self) {
        self.undo = None;
    }

    /// Restore every touched node and free every node created since [`Arena::begin_undo`].
    /// Created nodes were never observable outside the batch, so their ids simply go stale.
    pub(crate) fn rollback_undo(&mut self) {
        let Some(log) = self.undo.take() else { return };
        for (i, node) in log.before {
            self.nodes[i as usize] = Some(node);
        }
        for &id in log.created_ids.iter().rev() {
            if self.slots.is_live(id.raw()) {
                self.nodes[id.raw().index as usize] = None;
                self.live -= 1;
                self.slots.free(id.raw()).expect("created node is live");
            }
        }
    }

    /// Whether a batch undo scope is open.
    pub fn is_recording_undo(&self) -> bool {
        self.undo.is_some()
    }

    /// Panicking accessor for ids the caller knows are live (traversal invariant).
    pub fn node(&self, id: NodeId) -> &Node {
        self.get(id)
            .unwrap_or_else(|| panic!("stale NodeId {id:?}"))
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.get(id).is_some()
    }

    /// Take the checkpoint candidates (allocated or detached nodes since the last call).
    pub(crate) fn take_candidates(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.candidates)
    }

    /// Note that `n` may have become collectable (its last reference was dropped).
    pub(crate) fn note_candidate(&mut self, n: NodeId) {
        self.candidates.push(n);
    }

    /// Number of live nodes (template contents included).
    pub fn live_count(&self) -> usize {
        self.live
    }

    /// Number of slots ever allocated (the style sidecar store sizes itself to this).
    pub fn slot_count(&self) -> usize {
        self.nodes.len()
    }

    /// The live id occupying slot `index`, if any.
    pub fn id_at(&self, index: usize) -> Option<NodeId> {
        self.nodes.get(index)?.as_ref().map(|n| n.id)
    }

    /// Live nodes in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().flatten()
    }

    /// `(generation << 32) | index`: non-zero, generation-encoding (AD-07; research R-02).
    pub fn opaque_key(&self, id: NodeId) -> std::num::NonZeroU64 {
        OpaqueStyleKey::of(id).get()
    }

    pub fn is_inclusive_ancestor(&self, ancestor: NodeId, node: NodeId) -> bool {
        let mut cur = Some(node);
        while let Some(n) = cur {
            if n == ancestor {
                return true;
            }
            cur = self.get(n).and_then(|x| x.parent);
        }
        false
    }

    /// The root of `node`'s tree (itself when it has no parent).
    pub fn root_of(&self, node: NodeId) -> NodeId {
        let mut n = node;
        while let Some(p) = self.get(n).and_then(|x| x.parent) {
            n = p;
        }
        n
    }

    /// Connected = the tree root is the Document (template contents are never connected).
    pub fn is_connected(&self, node: NodeId) -> bool {
        self.root_of(node) == self.document
    }

    /// Inclusive descendants of `root` in tree order (pre-order).
    pub fn subtree(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = vec![];
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            let Some(node) = self.get(n) else { continue };
            out.push(n);
            stack.extend(node.children.iter().rev().copied());
        }
        out
    }

    /// Remove `child` from its parent's child list. Returns `(old_parent, index)`.
    pub(crate) fn detach(&mut self, child: NodeId) -> Option<(NodeId, usize)> {
        let parent = self.get_mut(child)?.parent.take()?;
        // The detached subtree is a candidate, and so is what remains of the old tree (it may have
        // just lost the last node that referred to it).
        self.candidates.push(child);
        self.candidates.push(parent);
        let p = self.get_mut(parent).expect("a live node's parent is live");
        let i = p
            .children
            .iter()
            .position(|c| *c == child)
            .expect("child is in its parent's list (tree invariant)");
        p.children.remove(i);
        Some((parent, i))
    }

    /// Link without the O(depth) cycle check: for the parser, whose freshly created or
    /// detached nodes cannot be ancestors of their new parent. Panics on stale ids.
    pub(crate) fn insert_trusted(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        self.link(parent, child, before);
    }

    fn link(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        self.detach(child);
        let p = self.get_mut(parent).expect("checked");
        let at = match before {
            Some(b) => p
                .children
                .iter()
                .position(|c| *c == b)
                .unwrap_or(p.children.len()),
            None => p.children.len(),
        };
        p.children.insert(at, child);
        self.get_mut(child).expect("checked").parent = Some(parent);
    }

    /// Detach `root` and free it and its subtree (including template contents). Every freed
    /// slot's generation is bumped (or the slot retired), so outstanding ids become stale.
    pub(crate) fn free_subtree(&mut self, root: NodeId) -> Vec<NodeId> {
        self.detach(root);
        let mut freed = vec![];
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            let Some(node) = self.get_mut(n) else {
                continue;
            };
            stack.append(&mut node.children);
            if let NodeData::Element(e) = &node.data
                && let Some(t) = e.template_contents
            {
                stack.push(t);
            }
            self.live -= 1;
            self.nodes[n.raw().index as usize] = None;
            self.slots.free(n.raw()).expect("live node frees once");
            freed.push(n);
        }
        freed
    }

    /// Free every live node (document close, FR-091): ids go stale; the arena stays usable only
    /// for identity checks.
    pub(crate) fn free_all(&mut self) {
        let ids: Vec<NodeId> = self.nodes.iter().flatten().map(|n| n.id).collect();
        for id in ids {
            self.nodes[id.raw().index as usize] = None;
            let _ = self.slots.free(id.raw());
        }
        self.live = 0;
        self.candidates.clear();
        self.undo = None;
    }

    /// Slots permanently retired because their generation was exhausted.
    pub fn retired_slots(&self) -> usize {
        self.slots.retired_count()
    }
}

/// Before-images of nodes touched, and slots created, while a batch is open.
#[derive(Debug, Default)]
pub(crate) struct UndoLog {
    before: std::collections::HashMap<u32, Node>,
    created: std::collections::HashSet<u32>,
    created_ids: Vec<NodeId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;
    use ui4me_engine_types::ids::RawId;

    #[test]
    fn a_million_allocate_free_cycles_never_reuse_an_index_generation_pair() {
        let mut a = Arena::new();
        let mut last: Option<NodeId> = None;
        let mut max_index = 0u32;
        for _ in 0..1_000_000 {
            let n = a.alloc(NodeData::Text(String::new()));
            max_index = max_index.max(n.raw().index);
            if let Some(prev) = last {
                assert_ne!(prev, n, "an id is never handed out twice");
                assert!(a.get(prev).is_none(), "the freed id stays stale");
            }
            a.free_subtree(n);
            last = Some(n);
        }
        assert!(
            max_index < 4,
            "freed slots are reused, so the arena does not grow"
        );
        assert_eq!(a.live_count(), 1, "only the Document node remains");
    }

    #[test]
    fn a_slot_whose_generation_would_wrap_is_retired_forever() {
        let mut a = Arena::new();
        let n = a.alloc(NodeData::Text("x".into()));
        let idx = n.raw().index;
        a.slots.force_generation_for_testing(idx, NonZeroU32::MAX);
        let at_max = NodeId::from_raw(RawId {
            index: idx,
            generation: NonZeroU32::MAX,
        });
        assert!(a.get(at_max).is_some());
        a.free_subtree(at_max);
        assert_eq!(a.retired_slots(), 1);
        let next = a.alloc(NodeData::Text("y".into()));
        assert_ne!(next.raw().index, idx, "a retired slot is never reused");
        assert!(a.get(at_max).is_none() && a.get(next).is_some());
    }
}
