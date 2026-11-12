//! owner: 003-document-model
//!
//! Cloning and import (T057; FR-017, FR-052, FR-058; DOM "clone a node", HTML cloning steps).
//!
//! A clone is detached, has no listeners and no UI-state flags, keeps attributes in order, and
//! copies stored control state (HTML cloning steps for `input`/`textarea`/`select`: value, dirty
//! flags, checkedness). Form owner and the parser-inserted flag are reset (re-evaluated on
//! insertion). Deep clones of a `template` clone its contents (FR-017). All traversals are
//! iterative. `importNode` copies from another open document of the runtime into this arena;
//! implicit adoption does not exist (FR-052) and `adoptNode` always fails (FR-053).

use crate::arena::Arena;
use crate::document::Document;
use crate::errors::{wrong_document, wrong_node_kind};
use crate::node::{NodeData, NodeKind};
use crate::ui_state::UiFlags;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// Cloning copies value, dirty value, checkedness, dirty checkedness (HTML cloning steps), never the
/// selection, composition range or user validity (data-model invariant on `ControlState`).
fn reset_uncloned_control_facts(e: &mut crate::node::ElementData) {
    if let Some(c) = e.control.as_mut() {
        c.composition = None;
        c.selection = None;
        c.user_validity = false;
        c.picker_highlight = None;
        c.picker_scroll = 0;
    }
}

impl Arena {
    /// Clone one node (no children), allocating template contents for a `template`.
    fn clone_one(&mut self, src: NodeId) -> NodeId {
        let data = self.node(src).data.clone();
        match data {
            NodeData::Element(mut e) => {
                let had_template = e.template_contents.take().is_some();
                e.ui = UiFlags::empty();
                e.form_owner = None;
                e.parser_inserted = false;
                reset_uncloned_control_facts(&mut e);
                let id = self.alloc(NodeData::Element(e));
                if had_template {
                    let f = self.alloc(NodeData::DocumentFragment { template: Some(id) });
                    self.get_mut(id)
                        .and_then(|n| n.as_element_mut())
                        .expect("new")
                        .template_contents = Some(f);
                }
                id
            }
            NodeData::DocumentFragment { .. } => {
                self.alloc(NodeData::DocumentFragment { template: None })
            }
            other => self.alloc(other),
        }
    }

    /// Clone `src` and its whole subtree (template contents included) into this arena; the clone's
    /// root is detached.
    pub(crate) fn clone_deep(&mut self, src: NodeId) -> NodeId {
        let root = self.clone_one(src);
        let mut stack = vec![(src, root)];
        while let Some((s, d)) = stack.pop() {
            for c in self.node(s).children.clone() {
                let cc = self.clone_one(c);
                self.insert_trusted(d, cc, None);
                stack.push((c, cc));
            }
            let st = self.node(s).as_element().and_then(|e| e.template_contents);
            let dt = self.node(d).as_element().and_then(|e| e.template_contents);
            if let (Some(st), Some(dt)) = (st, dt) {
                stack.push((st, dt));
            }
        }
        root
    }

    /// Clone `src` without its children (template contents are still created, empty).
    pub(crate) fn clone_shallow(&mut self, src: NodeId) -> NodeId {
        self.clone_one(src)
    }

    /// Number of nodes `clone_deep(src)` would allocate (for the node-limit precheck).
    pub(crate) fn clone_cost(&self, src: NodeId) -> u64 {
        let mut n = 0u64;
        let mut stack = vec![src];
        while let Some(x) = stack.pop() {
            n += 1;
            let node = self.node(x);
            stack.extend(node.children.iter().copied());
            if let Some(t) = node.as_element().and_then(|e| e.template_contents) {
                stack.push(t);
            }
        }
        n
    }
}

impl Document {
    /// `cloneNode(deep)` (A-12). The clone is detached. Documents cannot be cloned in v1.
    pub fn clone_node(&mut self, n: NodeId, deep: bool) -> Result<NodeId, Status> {
        let kind = self.check(n)?.kind();
        if kind == NodeKind::Document {
            return Err(wrong_node_kind());
        }
        let cost = if deep {
            self.arena.clone_cost(n)
        } else {
            1 + u64::from(
                self.arena
                    .node(n)
                    .as_element()
                    .is_some_and(|e| e.template_contents.is_some()),
            )
        };
        self.check_node_budget(cost)?;
        Ok(if deep {
            self.arena.clone_deep(n)
        } else {
            self.arena.clone_shallow(n)
        })
    }

    /// `importNode(node, deep)` (A-12): copy `node` of `source` (another open document of the
    /// runtime) into this document, detached. A closed or closing source is `DOCUMENT_CLOSED`; a
    /// stale source handle is `STALE_HANDLE`. Nothing implicit moves between documents.
    pub fn import_node(
        &mut self,
        source: &Document,
        node: NodeId,
        deep: bool,
    ) -> Result<NodeId, Status> {
        self.check_open()?;
        let skind = source.check(node)?.kind();
        if skind == NodeKind::Document {
            return Err(wrong_node_kind());
        }
        let cost = if deep {
            source.arena.clone_cost(node)
        } else {
            1
        };
        self.check_node_budget(cost)?;
        // Copy by walking the source and building into this arena.
        let root = self.import_one(source, node, true);
        let mut stack = vec![(node, root)];
        while let Some((s, d)) = stack.pop() {
            if deep {
                for &c in &source.arena.node(s).children {
                    let cc = self.import_one(source, c, true);
                    self.arena.insert_trusted(d, cc, None);
                    stack.push((c, cc));
                }
                let st = source
                    .arena
                    .node(s)
                    .as_element()
                    .and_then(|e| e.template_contents);
                let dt = self
                    .arena
                    .node(d)
                    .as_element()
                    .and_then(|e| e.template_contents);
                if let (Some(st), Some(dt)) = (st, dt) {
                    stack.push((st, dt));
                }
            }
        }
        Ok(root)
    }

    fn import_one(&mut self, source: &Document, src: NodeId, _fresh: bool) -> NodeId {
        let data = source.arena.node(src).data.clone();
        match data {
            NodeData::Element(mut e) => {
                let had_template = e.template_contents.take().is_some();
                e.ui = UiFlags::empty();
                e.form_owner = None;
                e.parser_inserted = false;
                reset_uncloned_control_facts(&mut e);
                let id = self.arena.alloc(NodeData::Element(e));
                if had_template {
                    let f = self
                        .arena
                        .alloc(NodeData::DocumentFragment { template: Some(id) });
                    self.arena
                        .get_mut(id)
                        .and_then(|n| n.as_element_mut())
                        .expect("new")
                        .template_contents = Some(f);
                }
                id
            }
            NodeData::DocumentFragment { .. } => self
                .arena
                .alloc(NodeData::DocumentFragment { template: None }),
            other => self.arena.alloc(other),
        }
    }

    /// `adoptNode`: never supported (FR-053); always `WRONG_DOCUMENT`.
    pub fn adopt_node(&mut self, _source: &Document, _node: NodeId) -> Result<NodeId, Status> {
        Err(wrong_document())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_clone_copies_structure_template_contents_and_control_state_but_not_ui_state() {
        let mut d = Document::parse_html("<div id=a class='x y'><template><p>t</p></template><input type=checkbox checked><b>bold</b></div>").unwrap();
        let a = d.element_by_id("a").unwrap();
        let before_live = d.arena.live_count();
        let c = d.clone_node(a, true).unwrap();
        assert!(d.parent(c).unwrap().is_none(), "clone is detached");
        assert!(d.is_equal_node(a, Some(c)).unwrap());
        assert_ne!(a, c);
        let tmpl = d.children(c).unwrap()[0];
        let contents = d.template_contents(tmpl).unwrap().unwrap();
        assert_eq!(
            d.children(contents).unwrap().len(),
            1,
            "the template's contents were cloned"
        );
        let orig_contents = d
            .template_contents(d.children(a).unwrap()[0])
            .unwrap()
            .unwrap();
        assert_ne!(contents, orig_contents);
        let input = d.children(c).unwrap()[1];
        assert_eq!(
            d.control_state(input).unwrap().unwrap().checkedness,
            Some(true)
        );
        assert!(d.arena.live_count() > before_live);
        assert_eq!(
            d.attributes(c).unwrap().len(),
            2,
            "attributes kept in order"
        );
    }

    #[test]
    fn shallow_clone_has_no_children_and_import_copies_across_documents() {
        let mut d = Document::parse_html("<div id=a><p>x</p></div>").unwrap();
        let a = d.element_by_id("a").unwrap();
        let s = d.clone_node(a, false).unwrap();
        assert!(d.children(s).unwrap().is_empty());
        let mut other = Document::parse_html("<section><i>q</i></section>").unwrap();
        let sec = other.body().unwrap();
        let sec = other.children(sec).unwrap()[0];
        let imp = d.import_node(&other, sec, true).unwrap();
        assert_eq!(d.local_name(imp).unwrap(), "section");
        assert_eq!(d.text_content(imp).unwrap().as_deref(), Some("q"));
        assert_eq!(d.owner_document(imp).unwrap(), Some(d.id()));
        other.request_close(false);
        assert_eq!(
            d.import_node(&other, sec, true).unwrap_err().code,
            ui4me_engine_types::codes_generated::Code::DocumentClosed
        );
        assert_eq!(
            d.adopt_node(&other, sec).unwrap_err().code,
            ui4me_engine_types::codes_generated::Code::WrongDocument
        );
        let _ = &mut other;
    }
}
