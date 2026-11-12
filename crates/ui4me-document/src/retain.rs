//! owner: 003-document-model
//!
//! Retention and collection (T001/T056; AD-29, FR-022, FR-026): host references per node, the
//! collection checkpoint that frees detached trees nothing refers to, and the `dom.counts`
//! payload. Moved from `document.rs` (R-28: move, never copy).

use crate::document::Document;
use crate::node::NodeData;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// `dom.counts` payload (mutation-journal.md §3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DomCounts {
    pub connected: u64,
    pub detached: u64,
    pub detached_trees: u64,
    pub freed_total: u64,
    pub host_refs_total: u64,
}

impl Document {
    /// Add one host reference (FR-022).
    pub fn retain(&mut self, n: NodeId) -> Result<(), Status> {
        self.check(n)?;
        self.arena.get_mut(n).expect("checked").host_refs =
            self.arena.node(n).host_refs.saturating_add(1);
        Ok(())
    }

    /// Drop one host reference. A stale handle, a `Closing`/`Closed` document or a node with no
    /// references left are no-op successes (idempotent release, R-34).
    pub fn release(&mut self, n: NodeId) -> Result<(), Status> {
        if let Some(node) = self.arena.get_mut(n) {
            node.host_refs = node.host_refs.saturating_sub(1);
            self.arena.note_candidate(n);
        }
        Ok(())
    }

    /// Releases queued by 004 from non-owner threads, applied at the next checkpoint (FR-027).
    pub fn apply_deferred_releases(&mut self, nodes: &[NodeId]) {
        for &n in nodes {
            let _ = self.release(n);
        }
    }

    /// A dispatch-frame strong reference (AD-27; FR-025): keeps the node and its tree alive while a
    /// dispatch path is active. Saturating.
    pub fn frame_retain(&mut self, n: NodeId) -> Result<(), Status> {
        self.check(n)?;
        let c = self.arena.node(n).frame_refs.saturating_add(1);
        self.arena.get_mut(n).expect("checked").frame_refs = c;
        Ok(())
    }

    /// Drop a dispatch-frame reference; idempotent on stale handles and closed documents.
    pub fn frame_release(&mut self, n: NodeId) -> Result<(), Status> {
        if let Some(node) = self.arena.get_mut(n) {
            node.frame_refs = node.frame_refs.saturating_sub(1);
            self.arena.note_candidate(n);
        }
        Ok(())
    }

    /// Collection checkpoint (FR-026): free every detached tree that has no host or dispatch-frame
    /// reference anywhere in it, looking only at the nodes allocated, detached or released since the
    /// last checkpoint. Template contents live as long as their template. Returns the freed ids and
    /// fires `on_nodes_freed` once. Does nothing while a mutating call is in progress.
    pub fn checkpoint(&mut self) -> Vec<NodeId> {
        if self.in_call() {
            return vec![];
        }
        let doc = self.arena.document();
        let mut roots: Vec<NodeId> = vec![];
        for c in self.arena.take_candidates() {
            if !self.arena.contains(c) {
                continue;
            }
            // Climb through template contents to the tree that owns them: the contents live and die
            // with their template, so a candidate inside them nominates the template's tree.
            let mut r = self.arena.root_of(c);
            while let NodeData::DocumentFragment { template: Some(t) } = &self.arena.node(r).data
                && self.arena.contains(*t)
            {
                r = self.arena.root_of(*t);
            }
            if r == doc || roots.contains(&r) {
                continue;
            }
            roots.push(r);
        }
        let mut freed = vec![];
        for r in roots {
            if !self.arena.contains(r) || self.tree_has_refs(r) {
                continue;
            }
            freed.extend(self.arena.free_subtree(r));
        }
        self.freed_total += freed.len() as u64;
        if !freed.is_empty()
            && let Some(mut h) = self.hooks.nodes_freed.take()
        {
            h(self.id, &freed);
            self.hooks.nodes_freed = Some(h);
        }
        freed
    }

    /// Alias kept for the slice's callers.
    pub fn collect(&mut self) -> Vec<NodeId> {
        self.checkpoint()
    }

    fn tree_has_refs(&self, root: NodeId) -> bool {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            let node = self.arena.node(n);
            if node.host_refs > 0 || node.frame_refs > 0 {
                return true;
            }
            stack.extend(node.children.iter().copied());
            if let Some(t) = node.as_element().and_then(|e| e.template_contents) {
                stack.push(t);
            }
        }
        false
    }

    pub fn counts(&self) -> DomCounts {
        let mut c = DomCounts {
            freed_total: self.freed_total,
            ..Default::default()
        };
        let doc = self.arena.document();
        for n in self.arena.iter() {
            c.host_refs_total += u64::from(n.host_refs);
            let root = self.arena.root_of(n.id);
            if root == doc {
                c.connected += 1;
                continue;
            }
            // Template contents count as detached only when their template is detached.
            let owner = match &self.arena.node(root).data {
                NodeData::DocumentFragment { template: Some(t) } => Some(*t),
                _ => None,
            };
            if owner.is_some_and(|t| self.arena.contains(t) && self.arena.is_connected(t)) {
                continue;
            }
            c.detached += 1;
            if n.parent.is_none() && owner.is_none() {
                c.detached_trees += 1;
            }
        }
        c
    }
}
