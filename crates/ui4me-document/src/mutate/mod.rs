//! owner: 003-document-model — created by 001 slice (R-28), driver completed by T018/T049/T050
//!
//! The mutation pipeline and the public structural API (Table A rows A-10, A-11, A-25, A-27, A-29,
//! A-32, A-33, A-50 structural part).
//!
//! Every call is immediate and atomic (research R-09): precheck (handles, lifecycle) -> read-only
//! validation in DOM step order -> apply links -> insertion / removing steps -> 012 hook calls ->
//! journal entries -> exactly one revision step -> post-commit notifications. A failed call changes
//! nothing and journals nothing; no host code and no event dispatch run inside a mutation (hooks
//! write only through the two writers). `M*` calls whose stored value does not change are no
//! mutation. Mutations of detached nodes are mutations. Creation (`C`) steps no revision.

pub mod insert;
pub mod remove;
pub mod replace;
pub mod validate;

pub use validate::NodeOrString;

use crate::document::Document;
use crate::errors::{
    hierarchy_request, invalid_character, not_found, syntax_error, wrong_node_kind,
};
use crate::hooks::Removal;
use crate::journal::EntryKind;
use crate::node::{Attribute, ElementData, NodeData, NodeKind};
use crate::ui_state::UiStateWriter;
use html5ever::{LocalName, QualName, ns};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// XML `Name` validation for `createElement` / `setAttribute` (DOM `dom.bs:191-290`).
pub(crate) fn is_valid_name(s: &str) -> bool {
    crate::names::is_name(s)
}

impl Document {
    // ------------------------------------------------------------ creation (C)

    /// `createElement(localName)` (A-10): an HTML element, name lowercased, detached.
    pub fn create_element(&mut self, local_name: &str) -> Result<NodeId, Status> {
        self.check_open()?;
        if !is_valid_name(local_name) {
            return Err(invalid_character());
        }
        self.check_node_budget(1 + u64::from(local_name.eq_ignore_ascii_case("template")))?;
        let name = QualName::new(
            None,
            ns!(html),
            LocalName::from(local_name.to_ascii_lowercase()),
        );
        Ok(self.alloc_element(name, vec![]))
    }

    /// `createElementNS(namespace, qualifiedName)` (A-10): DOM "validate and extract", detached.
    pub fn create_element_ns(
        &mut self,
        namespace: Option<&str>,
        qualified_name: &str,
    ) -> Result<NodeId, Status> {
        self.check_open()?;
        let x = crate::names::validate_and_extract(namespace, qualified_name)?;
        self.check_node_budget(
            1 + u64::from(
                x.local == "template"
                    && x.namespace.as_deref() == Some("http://www.w3.org/1999/xhtml"),
            ),
        )?;
        Ok(self.alloc_element(x.to_qual_name(), vec![]))
    }

    pub(crate) fn alloc_element(&mut self, name: QualName, attrs: Vec<Attribute>) -> NodeId {
        let e = ElementData::new(name, attrs);
        let is_template = e.is_html_named("template");
        let id = self.arena.alloc(NodeData::Element(e));
        if is_template {
            let frag = self
                .arena
                .alloc(NodeData::DocumentFragment { template: Some(id) });
            self.arena
                .get_mut(id)
                .and_then(|n| n.as_element_mut())
                .expect("new")
                .template_contents = Some(frag);
        }
        id
    }

    /// `createTextNode(data)` (A-11).
    pub fn create_text_node(&mut self, data: &str) -> Result<NodeId, Status> {
        self.check_open()?;
        self.check_string(data)?;
        self.check_node_budget(1)?;
        Ok(self.arena.alloc(NodeData::Text(data.to_owned())))
    }

    pub fn create_comment(&mut self, data: &str) -> Result<NodeId, Status> {
        self.check_open()?;
        self.check_string(data)?;
        self.check_node_budget(1)?;
        Ok(self.arena.alloc(NodeData::Comment(data.to_owned())))
    }

    pub fn create_document_fragment(&mut self) -> Result<NodeId, Status> {
        self.check_open()?;
        self.check_node_budget(1)?;
        Ok(self
            .arena
            .alloc(NodeData::DocumentFragment { template: None }))
    }

    // ------------------------------------------------------------ shared pipeline helpers

    /// Is `n` inside template contents (its tree root is a template's contents fragment)? Such
    /// nodes are inert: no hook calls, no requester notifications, never connected (FR-015).
    pub(crate) fn in_template_contents(&self, n: NodeId) -> bool {
        matches!(
            self.arena.node(self.arena.root_of(n)).data,
            NodeData::DocumentFragment { template: Some(_) }
        )
    }

    pub(crate) fn run_removed_hook(&mut self, removal: Removal) {
        if let Some(mut h) = self.hooks.removed.take() {
            h(
                &mut UiStateWriter {
                    doc: self,
                    in_call: true,
                },
                &removal,
            );
            self.hooks.removed = Some(h);
        }
    }

    pub(crate) fn run_inserted_hook(&mut self, n: NodeId) {
        if let Some(mut h) = self.hooks.inserted.take() {
            h(
                &mut UiStateWriter {
                    doc: self,
                    in_call: true,
                },
                n,
            );
            self.hooks.inserted = Some(h);
        }
    }

    pub(crate) fn run_children_changed_hook(&mut self, parent: NodeId) {
        if let Some(mut h) = self.hooks.children_changed.take() {
            h(
                &mut UiStateWriter {
                    doc: self,
                    in_call: true,
                },
                parent,
            );
            self.hooks.children_changed = Some(h);
        }
    }

    pub(crate) fn siblings_at(
        &self,
        parent: NodeId,
        index: usize,
    ) -> (Option<NodeId>, Option<NodeId>) {
        let kids = &self.arena.node(parent).children;
        (
            index.checked_sub(1).and_then(|i| kids.get(i)).copied(),
            kids.get(index).copied(),
        )
    }

    // ------------------------------------------------------------ text (M)

    /// `textContent` setter (A-25): replace all children of an element or fragment with one Text
    /// node (none for ""); set the data of Text/Comment; no effect on Document/Doctype.
    pub fn set_text_content(&mut self, n: NodeId, text: &str) -> Result<(), Status> {
        let kind = self.check(n)?.kind();
        match kind {
            NodeKind::Document | NodeKind::Doctype => Ok(()),
            NodeKind::Text | NodeKind::Comment => self.set_data(n, text),
            NodeKind::Element | NodeKind::DocumentFragment => {
                self.check_string(text)?;
                if text.is_empty() && self.arena.node(n).children.is_empty() {
                    return Ok(());
                }
                if !text.is_empty() {
                    self.check_node_budget(1)?;
                }
                self.begin_call();
                let rev = self.pending_revision();
                let old = self.arena.node(n).children.clone();
                if text.is_empty() {
                    self.remove_nodes_step(rev, n, &old);
                } else {
                    // DOM "replace all": removal and insertion share one `child_list` entry.
                    self.remove_nodes_ex(rev, n, &old, false);
                    let t = self.arena.alloc(NodeData::Text(text.to_owned()));
                    self.insert_step_ex(rev, n, vec![t], None, old);
                }
                self.end_call();
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------ structure (M)

    /// Resolve operands to node ids; strings become fresh detached Text nodes (returned in
    /// `created` so a failing call can free them). Checks the node and string limits first.
    fn resolve_items(
        &mut self,
        items: &[NodeOrString],
    ) -> Result<(Vec<NodeId>, Vec<NodeId>), Status> {
        let mut texts = 0u64;
        for it in items {
            match it {
                NodeOrString::Node(n) => {
                    self.check(*n)?;
                }
                NodeOrString::Text(s) => {
                    self.check_string(s)?;
                    texts += 1;
                }
            }
        }
        self.check_node_budget(texts)?;
        let mut ids = vec![];
        let mut created = vec![];
        for it in items {
            match it {
                NodeOrString::Node(n) => ids.push(*n),
                NodeOrString::Text(s) => {
                    let t = self.arena.alloc(NodeData::Text(s.clone()));
                    ids.push(t);
                    created.push(t);
                }
            }
        }
        Ok((ids, created))
    }

    fn free_created(&mut self, created: Vec<NodeId>) {
        for c in created {
            self.arena.free_subtree(c);
        }
    }

    /// Apply an already validated insertion of `items` into `parent` before `before` in ONE call
    /// (one revision step). Fragments are emptied into the insertion; nodes with a parent are
    /// removed from it first (a move happens inside the same call, FR-054).
    pub(crate) fn apply_insert(
        &mut self,
        parent: NodeId,
        items: Vec<NodeId>,
        before: Option<NodeId>,
    ) {
        let mut flat = vec![];
        for &n in &items {
            if self.arena.node(n).kind() == NodeKind::DocumentFragment {
                flat.extend(self.arena.node(n).children.iter().copied());
            } else {
                flat.push(n);
            }
        }
        if flat.is_empty() {
            return;
        }
        self.begin_call();
        let rev = self.pending_revision();
        for &n in &items {
            if self.arena.node(n).kind() == NodeKind::DocumentFragment {
                let kids = self.arena.node(n).children.clone();
                self.remove_nodes_step(rev, n, &kids);
            } else if self.arena.node(n).parent.is_some() {
                let p = self.arena.node(n).parent.expect("checked");
                self.remove_nodes_step(rev, p, &[n]);
            }
        }
        self.insert_step(rev, parent, flat, before);
        self.end_call();
    }

    /// `insertBefore(node, child?)` (A-27). Returns the inserted node.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        node: NodeId,
        before: Option<NodeId>,
    ) -> Result<NodeId, Status> {
        self.check(parent)?;
        self.check(node)?;
        if let Some(b) = before {
            self.check(b)?;
        }
        self.validate_pre_insert(parent, node, before)?;
        // "If referenceChild is node, set referenceChild to node's next sibling."
        let before = if before == Some(node) {
            self.next_sibling(node)?
        } else {
            before
        };
        self.check_node_budget(0)?;
        self.apply_insert(parent, vec![node], before);
        Ok(node)
    }

    /// `appendChild(node)` (A-27).
    pub fn append_child(&mut self, parent: NodeId, node: NodeId) -> Result<NodeId, Status> {
        self.insert_before(parent, node, None)
    }

    /// `removeChild(child)` (A-27): `INVALID_HIERARCHY`/`NOT_A_CHILD` if not a child.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<NodeId, Status> {
        self.check(parent)?;
        self.check(child)?;
        if !matches!(
            self.arena.node(parent).kind(),
            NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element
        ) {
            return Err(hierarchy_request());
        }
        if self.arena.node(child).parent != Some(parent) {
            return Err(not_found());
        }
        self.begin_call();
        let rev = self.pending_revision();
        self.remove_nodes_step(rev, parent, &[child]);
        self.end_call();
        Ok(child)
    }

    /// `replaceChild(node, child)` (A-27). Returns the replaced child.
    pub fn replace_child(
        &mut self,
        parent: NodeId,
        node: NodeId,
        child: NodeId,
    ) -> Result<NodeId, Status> {
        self.check(parent)?;
        self.check(node)?;
        self.check(child)?;
        self.validate_replace(parent, node, child)?;
        self.check_node_budget(0)?;
        self.apply_replace(parent, vec![node], child);
        Ok(child)
    }

    /// `ChildNode.remove()` (A-33): no parent -> no effect.
    pub fn remove(&mut self, node: NodeId) -> Result<(), Status> {
        match self.check(node)?.parent {
            Some(p) => self.remove_child(p, node).map(|_| ()),
            None => Ok(()),
        }
    }

    /// `append(...nodes)` (A-32).
    pub fn append(&mut self, parent: NodeId, items: &[NodeOrString]) -> Result<(), Status> {
        self.check(parent)?;
        let (ids, created) = self.resolve_items(items)?;
        if let Err(e) = self.validate_insert_aggregate(parent, &ids, None) {
            self.free_created(created);
            return Err(e);
        }
        self.apply_insert(parent, ids, None);
        Ok(())
    }

    /// `prepend(...nodes)` (A-32).
    pub fn prepend(&mut self, parent: NodeId, items: &[NodeOrString]) -> Result<(), Status> {
        self.check(parent)?;
        let (ids, created) = self.resolve_items(items)?;
        // Reference: the first child that is not itself being inserted (viable).
        let first = self
            .arena
            .node(parent)
            .children
            .iter()
            .copied()
            .find(|c| !ids.contains(c));
        if let Err(e) = self.validate_insert_aggregate(parent, &ids, first) {
            self.free_created(created);
            return Err(e);
        }
        self.apply_insert(parent, ids, first);
        Ok(())
    }

    /// `replaceChildren(...nodes)` (A-32): validated as a pre-insert into the (emptied) parent.
    pub fn replace_children(
        &mut self,
        parent: NodeId,
        items: &[NodeOrString],
    ) -> Result<(), Status> {
        self.check(parent)?;
        let (ids, created) = self.resolve_items(items)?;
        // Validate against the parent as it will be after removal: a node that is a current child
        // is moved, so the Document element-count rule must ignore the children being replaced.
        if let Err(e) = self.validate_insert_aggregate_for_replace_all(parent, &ids) {
            self.free_created(created);
            return Err(e);
        }
        let mut flat = vec![];
        for &n in &ids {
            if self.arena.node(n).kind() == NodeKind::DocumentFragment {
                flat.extend(self.arena.node(n).children.iter().copied());
            } else {
                flat.push(n);
            }
        }
        let old = self.arena.node(parent).children.clone();
        if flat.is_empty() && old.is_empty() {
            return Ok(());
        }
        self.begin_call();
        let rev = self.pending_revision();
        // Fragments are emptied, then the parent's own children are removed, then the nodes are
        // moved in from wherever they were.
        for &n in &ids {
            if self.arena.node(n).kind() == NodeKind::DocumentFragment {
                let kids = self.arena.node(n).children.clone();
                self.remove_nodes_step(rev, n, &kids);
            }
        }
        let current = self.arena.node(parent).children.clone();
        self.remove_nodes_step(rev, parent, &current);
        for &n in &flat {
            if let Some(p) = self.arena.node(n).parent {
                self.remove_nodes_step(rev, p, &[n]);
            }
        }
        self.insert_step(rev, parent, flat, None);
        self.end_call();
        Ok(())
    }

    /// Validate `items` as the replacement of all of `parent`'s children.
    fn validate_insert_aggregate_for_replace_all(
        &self,
        parent: NodeId,
        items: &[NodeId],
    ) -> Result<(), Status> {
        // DOM: "ensure pre-insert validity of node into this before null" — the parent still holds
        // its children at that point.
        self.validate_insert_aggregate(parent, items, None)
    }

    /// `before(...nodes)` (A-33).
    pub fn before(&mut self, node: NodeId, items: &[NodeOrString]) -> Result<(), Status> {
        let Some(parent) = self.check(node)?.parent else {
            return Ok(());
        };
        let (ids, created) = self.resolve_items(items)?;
        // Viable previous sibling: nearest preceding sibling not in `ids`.
        let kids = &self.arena.node(parent).children;
        let pos = kids
            .iter()
            .position(|c| *c == node)
            .expect("tree invariant");
        let viable_prev = kids[..pos].iter().rev().copied().find(|c| !ids.contains(c));
        let reference = match viable_prev {
            Some(p) => self.next_sibling(p)?,
            None => kids.first().copied(),
        };
        if let Err(e) = self.validate_insert_aggregate(parent, &ids, reference) {
            self.free_created(created);
            return Err(e);
        }
        self.apply_insert(parent, ids, reference);
        Ok(())
    }

    /// `after(...nodes)` (A-33).
    pub fn after(&mut self, node: NodeId, items: &[NodeOrString]) -> Result<(), Status> {
        let Some(parent) = self.check(node)?.parent else {
            return Ok(());
        };
        let (ids, created) = self.resolve_items(items)?;
        let kids = &self.arena.node(parent).children;
        let pos = kids
            .iter()
            .position(|c| *c == node)
            .expect("tree invariant");
        let viable_next = kids[pos + 1..].iter().copied().find(|c| !ids.contains(c));
        if let Err(e) = self.validate_insert_aggregate(parent, &ids, viable_next) {
            self.free_created(created);
            return Err(e);
        }
        self.apply_insert(parent, ids, viable_next);
        Ok(())
    }

    /// `replaceWith(...nodes)` (A-33).
    pub fn replace_with(&mut self, node: NodeId, items: &[NodeOrString]) -> Result<(), Status> {
        let Some(parent) = self.check(node)?.parent else {
            return Ok(());
        };
        let (ids, created) = self.resolve_items(items)?;
        let kids = &self.arena.node(parent).children;
        let pos = kids
            .iter()
            .position(|c| *c == node)
            .expect("tree invariant");
        let viable_next = kids[pos + 1..].iter().copied().find(|c| !ids.contains(c));
        if ids.contains(&node) {
            // `node` is among the operands: replace-with-itself is a move of the other operands
            // around it; validate as an insertion before the viable next sibling.
            if let Err(e) = self.validate_insert_aggregate(parent, &ids, viable_next) {
                self.free_created(created);
                return Err(e);
            }
            self.apply_insert(parent, ids, viable_next);
            return Ok(());
        }
        let check = if let [one] = ids.as_slice() {
            self.validate_replace(parent, *one, node)
        } else {
            self.validate_insert_aggregate(parent, &ids, Some(node))
        };
        if let Err(e) = check {
            self.free_created(created);
            return Err(e);
        }
        self.apply_replace(parent, ids, node);
        Ok(())
    }

    /// `insertAdjacentElement(position, element)` (A-50). Returns the node, or `None` when
    /// `beforebegin`/`afterend` has no parent. An unknown position is `SYNTAX`.
    pub fn insert_adjacent_element(
        &mut self,
        el: NodeId,
        position: &str,
        node: NodeId,
    ) -> Result<Option<NodeId>, Status> {
        self.check(node)?;
        if !self.arena.node(node).is_element() {
            return Err(wrong_node_kind());
        }
        match self.adjacent_target(el, position)? {
            None => Ok(None),
            Some((parent, reference)) => {
                self.validate_pre_insert(parent, node, reference)?;
                let reference = if reference == Some(node) {
                    self.next_sibling(node)?
                } else {
                    reference
                };
                self.check_node_budget(0)?;
                self.apply_insert(parent, vec![node], reference);
                Ok(Some(node))
            }
        }
    }

    /// `insertAdjacentText(position, data)` (A-50).
    pub fn insert_adjacent_text(
        &mut self,
        el: NodeId,
        position: &str,
        data: &str,
    ) -> Result<(), Status> {
        let target = self.adjacent_target(el, position)?;
        self.check_string(data)?;
        if let Some((parent, reference)) = target {
            self.check_node_budget(1)?;
            let t = self.arena.alloc(NodeData::Text(data.to_owned()));
            if let Err(e) = self.validate_pre_insert(parent, t, reference) {
                self.arena.free_subtree(t);
                return Err(e);
            }
            self.apply_insert(parent, vec![t], reference);
        }
        Ok(())
    }

    /// The `(parent, reference child)` of an adjacent position of element `el`.
    pub(crate) fn adjacent_target(
        &self,
        el: NodeId,
        position: &str,
    ) -> Result<Option<(NodeId, Option<NodeId>)>, Status> {
        if !self.check(el)?.is_element() {
            return Err(wrong_node_kind());
        }
        let pos = position.to_ascii_lowercase();
        match pos.as_str() {
            "beforebegin" => Ok(self.arena.node(el).parent.map(|p| (p, Some(el)))),
            "afterbegin" => Ok(Some((el, self.arena.node(el).children.first().copied()))),
            "beforeend" => Ok(Some((el, None))),
            "afterend" => match self.arena.node(el).parent {
                None => Ok(None),
                Some(p) => {
                    let next = self.next_sibling(el)?;
                    Ok(Some((p, next)))
                }
            },
            _ => Err(syntax_error()),
        }
    }

    // ------------------------------------------------------------ normalize

    /// `normalize()` (A-29, FR-047): in the subtree of `n` (template contents excluded), remove
    /// empty Text nodes and merge runs of adjacent Text nodes into the first, in one call.
    pub fn normalize(&mut self, n: NodeId) -> Result<(), Status> {
        self.check(n)?;
        let nodes = self.arena.subtree(n);
        // Plan first: nothing to do means no mutation and no revision step.
        let mut needs = false;
        for &p in &nodes {
            let kids = &self.arena.node(p).children;
            for (i, &c) in kids.iter().enumerate() {
                if let NodeData::Text(t) = &self.arena.node(c).data
                    && (t.is_empty()
                        || kids
                            .get(i + 1)
                            .is_some_and(|&x| matches!(self.arena.node(x).data, NodeData::Text(_))))
                {
                    needs = true;
                }
            }
        }
        if !needs {
            return Ok(());
        }
        self.begin_call();
        let rev = self.pending_revision();
        for p in nodes {
            let mut i = 0;
            while i < self.arena.node(p).children.len() {
                let c = self.arena.node(p).children[i];
                if !matches!(self.arena.node(c).data, NodeData::Text(_)) {
                    i += 1;
                    continue;
                }
                // Merge following Text siblings into c.
                loop {
                    let next = self.arena.node(p).children.get(i + 1).copied();
                    let Some(nx) = next else { break };
                    let NodeData::Text(add) = &self.arena.node(nx).data else {
                        break;
                    };
                    let add = add.clone();
                    if let Some(NodeData::Text(t)) = self.arena.get_mut(c).map(|x| &mut x.data) {
                        t.push_str(&add);
                    }
                    self.journal.push(rev, EntryKind::CharacterData { node: c });
                    self.remove_nodes_step(rev, p, &[nx]);
                }
                if matches!(&self.arena.node(c).data, NodeData::Text(t) if t.is_empty()) {
                    self.remove_nodes_step(rev, p, &[c]);
                } else {
                    i += 1;
                }
            }
        }
        self.end_call();
        Ok(())
    }
}
