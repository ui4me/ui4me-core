//! owner: 003-document-model
//!
//! Replacement (T048; FR-048, FR-050): DOM "replace a child with node within a parent" for an
//! already validated operation, in one call. The reference child is `child`'s next sibling that is
//! not itself being inserted.

use crate::document::Document;
use crate::node::NodeKind;
use ui4me_engine_types::ids::NodeId;

impl Document {
    /// Replace `child` of `parent` with `items` (fragments are flattened) in ONE call.
    pub(crate) fn apply_replace(&mut self, parent: NodeId, items: Vec<NodeId>, child: NodeId) {
        let mut flat = vec![];
        for &n in &items {
            if self.arena.node(n).kind() == NodeKind::DocumentFragment {
                flat.extend(self.arena.node(n).children.iter().copied());
            } else {
                flat.push(n);
            }
        }
        // Replacing a node with itself changes nothing.
        if flat == [child] {
            return;
        }
        let kids = self.arena.node(parent).children.clone();
        let pos = kids
            .iter()
            .position(|c| *c == child)
            .expect("validated child");
        let reference = kids[pos + 1..].iter().copied().find(|c| !flat.contains(c));
        self.begin_call();
        let rev = self.pending_revision();
        for &n in &items {
            if self.arena.node(n).kind() == NodeKind::DocumentFragment {
                let k = self.arena.node(n).children.clone();
                self.remove_nodes_step(rev, n, &k);
            } else if n != child
                && let Some(p) = self.arena.node(n).parent
            {
                self.remove_nodes_step(rev, p, &[n]);
            }
        }
        if self.arena.node(child).parent == Some(parent) {
            self.remove_nodes_step(rev, parent, &[child]);
        }
        self.insert_step(rev, parent, flat, reference);
        self.end_call();
    }
}
