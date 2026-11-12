//! owner: 003-document-model
//!
//! Form-owner algorithm (T084; research R-15; FR-072, FR-076; HTML "reset the form owner").
//!
//! Every form-associated element (`button`, `fieldset`, `input`, `object`, `output`, `select`,
//! `textarea`, `img`) has a form owner: the `form` element named by its `form` attribute when it is
//! listed (not `img`), connected and the first element with that id is a `form`; otherwise its
//! nearest `form` ancestor; otherwise none. The owner is re-evaluated on insertion and removal of the
//! element or an ancestor (unless the parser inserted the element, whose parser-time association
//! stands until it is removed), on a change of the element's `form` attribute, and on changes to the
//! id or presence of `form` elements. Owner changes are not selector-relevant and are not journaled;
//! they are written through the arena so a batch rollback restores them.

use crate::document::Document;
use crate::errors::wrong_node_kind;
use crate::node::{ElementData, NodeKind};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// Does the element have a form owner (listed elements and `img`)?
pub fn is_form_associated(e: &ElementData) -> bool {
    e.is_html()
        && matches!(
            e.local_name(),
            "button" | "fieldset" | "input" | "object" | "output" | "select" | "textarea" | "img"
        )
}

/// Listed elements: those in `form.elements` (all form-associated except `img`).
pub fn is_listed(e: &ElementData) -> bool {
    is_form_associated(e) && e.local_name() != "img"
}

impl Document {
    /// `form` (A-53): the form owner of a form-associated element, `None` otherwise.
    pub fn form_owner(&self, el: NodeId) -> Result<Option<NodeId>, Status> {
        let e = self.check(el)?.as_element().ok_or_else(wrong_node_kind)?;
        Ok(e.form_owner)
    }

    /// `elements` of a `form` (A-53): listed elements owned by it, in tree order (static list).
    pub fn form_elements(&self, form: NodeId) -> Result<Vec<NodeId>, Status> {
        let node = self.check(form)?;
        if !node.as_element().is_some_and(|e| e.is_html_named("form")) {
            return Err(wrong_node_kind());
        }
        let root = self.arena.root_of(form);
        Ok(self
            .arena
            .subtree(root)
            .into_iter()
            .filter(|&n| {
                self.arena
                    .node(n)
                    .as_element()
                    .is_some_and(|e| is_listed(e) && e.form_owner == Some(form))
            })
            .collect())
    }

    fn nearest_form_ancestor(&self, el: NodeId) -> Option<NodeId> {
        let mut cur = self.arena.node(el).parent;
        while let Some(p) = cur {
            if self
                .arena
                .node(p)
                .as_element()
                .is_some_and(|e| e.is_html_named("form"))
            {
                return Some(p);
            }
            cur = self.arena.node(p).parent;
        }
        None
    }

    /// The first element in `el`'s tree, in tree order, with the given id.
    fn first_element_with_id_in_tree(&self, el: NodeId, id: &str) -> Option<NodeId> {
        let root = self.arena.root_of(el);
        self.arena
            .subtree(root)
            .into_iter()
            .find(|&n| self.arena.node(n).as_element().and_then(|e| e.attr("id")) == Some(id))
    }

    /// HTML "reset the form owner" for one form-associated element.
    pub(crate) fn reset_form_owner(&mut self, el: NodeId) {
        let Some(e) = self.arena.get(el).and_then(|n| n.as_element()) else {
            return;
        };
        if !is_form_associated(e) {
            return;
        }
        let listed = is_listed(e);
        let form_attr = e.attr("form").map(str::to_owned);
        let current = e.form_owner;
        let nearest = self.nearest_form_ancestor(el);
        // Step 2: keep the owner when it is still the nearest form ancestor and no `form` attribute
        // (on a listed element) overrides it.
        let keep = current.is_some() && (!listed || form_attr.is_none()) && current == nearest;
        let new_owner = if keep {
            current
        } else if listed
            && let Some(v) = form_attr.as_deref()
            && self.arena.is_connected(el)
        {
            match self.first_element_with_id_in_tree(el, v) {
                Some(f)
                    if self
                        .arena
                        .node(f)
                        .as_element()
                        .is_some_and(|x| x.is_html_named("form")) =>
                {
                    Some(f)
                }
                _ => None,
            }
        } else {
            nearest
        };
        if let Some(n) = self.arena.get_mut(el)
            && let Some(ed) = n.as_element_mut()
        {
            ed.parser_inserted = false;
            if ed.form_owner != new_owner {
                ed.form_owner = new_owner;
            }
        }
    }

    /// Re-evaluate every listed element that carries a `form` attribute (a `form` element was
    /// inserted, removed or changed its id).
    pub(crate) fn reset_form_attr_owners(&mut self) {
        let ids: Vec<NodeId> = self
            .arena
            .iter()
            .filter(|n| {
                n.as_element()
                    .is_some_and(|e| is_listed(e) && e.attr("form").is_some())
            })
            .map(|n| n.id)
            .collect();
        for id in ids {
            self.reset_form_owner(id);
        }
    }

    /// After `roots` (detached nodes) were inserted: reset non-parser-inserted form-associated
    /// descendants; if a form element or an element with an id arrived, re-resolve `form=` owners.
    pub(crate) fn form_owner_after_insert(&mut self, roots: &[NodeId]) {
        let mut touch_forms = false;
        for &r in roots {
            for n in self.arena.subtree(r) {
                let Some(e) = self.arena.node(n).as_element() else {
                    continue;
                };
                if e.is_html_named("form") {
                    touch_forms = true;
                }
                if is_form_associated(e) && !e.parser_inserted {
                    self.reset_form_owner(n);
                }
            }
        }
        if touch_forms {
            self.reset_form_attr_owners();
        }
    }

    /// After `root`'s subtree was removed from its parent: owners that depended on the old ancestor
    /// chain are reset (a form-owned element whose form is no longer an ancestor loses the owner).
    pub(crate) fn form_owner_after_remove(&mut self, root: NodeId) {
        let mut touch_forms = false;
        for n in self.arena.subtree(root) {
            let Some(e) = self.arena.node(n).as_element() else {
                continue;
            };
            if e.is_html_named("form") {
                touch_forms = true;
            }
            if is_form_associated(e) && e.form_owner.is_some() {
                self.reset_form_owner(n);
            }
        }
        if touch_forms {
            self.reset_form_attr_owners();
        }
    }

    /// Attribute `name` of `el` changed: `form` on a listed element, or `id` on a `form` element.
    pub(crate) fn form_owner_attr_changed(&mut self, el: NodeId, name: &str) {
        let Some(e) = self.arena.get(el).and_then(|n| n.as_element()) else {
            return;
        };
        if name == "form" && is_listed(e) {
            self.reset_form_owner(el);
        } else if name == "id" && e.is_html_named("form") {
            self.reset_form_attr_owners();
        }
    }

    /// End of a full-document parse: listed elements with a `form` attribute resolve their owner now.
    pub(crate) fn finish_parse_forms(&mut self) {
        self.reset_form_attr_owners();
        self.finish_parse_selects();
    }

    /// Is `n` a node kind that can have a form owner (used by batch and tests)?
    pub fn is_form_associated_node(&self, n: NodeId) -> bool {
        self.arena.get(n).is_some_and(|x| {
            x.kind() == NodeKind::Element && x.as_element().is_some_and(is_form_associated)
        })
    }
}
