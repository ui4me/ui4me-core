//! owner: 003-document-model
//!
//! The structure-frozen view `DomView<'a>` (T015; dom-internal-interfaces section 2.1; AD-07, AD-28).
//! It is a shared borrow of the Document, lent for one synchronous style pass or query: no `&mut` is
//! ever exposed, and the borrow checker makes mutation impossible while the view is alive.
//!
//! ```compile_fail,E0502
//! let mut doc = ui4me_document::document::Document::parse_html("<p>x</p>").unwrap();
//! let view = doc.view();
//! let p = doc.body().unwrap();
//! doc.set_attribute(p, "a", "b").unwrap(); // mutation while a view is borrowed
//! drop(view);
//! ```

use crate::control_state::ControlState;
use crate::document::{Document, DocumentMode};
use crate::node::{Attribute, ElementData, Node, NodeKind};
use crate::stylesheets::StylesheetEntry;
use crate::ui_state::UiFlags;
use html5ever::QualName;
use std::num::NonZeroU64;
use ui4me_engine_types::ids::NodeId;

/// Read-only structural view of one document.
#[derive(Clone, Copy)]
pub struct DomView<'a> {
    doc: &'a Document,
}

impl Document {
    /// Lend the structure-frozen view.
    pub fn view(&self) -> DomView<'_> {
        DomView { doc: self }
    }
}

impl<'a> DomView<'a> {
    pub fn document(&self) -> &'a Document {
        self.doc
    }

    pub fn root(&self) -> NodeId {
        self.doc.arena().document()
    }

    fn node(&self, n: NodeId) -> &'a Node {
        self.doc.arena().node(n)
    }

    pub fn kind(&self, n: NodeId) -> NodeKind {
        self.node(n).kind()
    }

    // ---- navigation (template contents are never children)

    pub fn parent(&self, n: NodeId) -> Option<NodeId> {
        self.node(n).parent()
    }
    pub fn parent_element(&self, n: NodeId) -> Option<NodeId> {
        self.parent(n).filter(|&p| self.node(p).is_element())
    }
    pub fn children(&self, n: NodeId) -> &'a [NodeId] {
        self.node(n).children()
    }
    pub fn first_child(&self, n: NodeId) -> Option<NodeId> {
        self.children(n).first().copied()
    }
    pub fn last_child(&self, n: NodeId) -> Option<NodeId> {
        self.children(n).last().copied()
    }
    fn sibling(&self, n: NodeId, delta: isize) -> Option<NodeId> {
        let p = self.parent(n)?;
        let kids = self.children(p);
        let i = kids.iter().position(|c| *c == n)? as isize + delta;
        usize::try_from(i).ok().and_then(|i| kids.get(i)).copied()
    }
    pub fn prev_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.sibling(n, -1)
    }
    pub fn next_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.sibling(n, 1)
    }
    pub fn prev_element_sibling(&self, n: NodeId) -> Option<NodeId> {
        let mut c = self.prev_sibling(n);
        while let Some(x) = c {
            if self.node(x).is_element() {
                return Some(x);
            }
            c = self.prev_sibling(x);
        }
        None
    }
    pub fn next_element_sibling(&self, n: NodeId) -> Option<NodeId> {
        let mut c = self.next_sibling(n);
        while let Some(x) = c {
            if self.node(x).is_element() {
                return Some(x);
            }
            c = self.next_sibling(x);
        }
        None
    }
    pub fn is_connected(&self, n: NodeId) -> bool {
        self.doc.arena().is_connected(n)
    }
    /// The contents fragment of a `template` element (reached only through the template).
    pub fn template_contents(&self, n: NodeId) -> Option<NodeId> {
        self.node(n)
            .as_element()
            .and_then(ElementData::template_contents)
    }

    // ---- element data

    pub fn element(&self, n: NodeId) -> Option<&'a ElementData> {
        self.node(n).as_element()
    }
    /// The qualified-name atoms shared with the parser and the style system (research R-07).
    pub fn qual_name(&self, n: NodeId) -> Option<&'a QualName> {
        self.element(n).map(ElementData::name)
    }
    pub fn attributes(&self, n: NodeId) -> &'a [Attribute] {
        self.element(n).map_or(&[], ElementData::attributes)
    }
    pub fn id_attr(&self, n: NodeId) -> Option<&'a str> {
        self.element(n).and_then(|e| e.attr("id"))
    }
    pub fn class_attr(&self, n: NodeId) -> Option<&'a str> {
        self.element(n).and_then(|e| e.attr("class"))
    }
    pub fn classes(&self, n: NodeId) -> impl Iterator<Item = &'a str> {
        self.element(n).into_iter().flat_map(ElementData::classes)
    }
    /// HTML element in an HTML document (every document here is an HTML document).
    pub fn is_html_element_in_html_document(&self, n: NodeId) -> bool {
        self.element(n).is_some_and(ElementData::is_html)
    }
    pub fn document_mode(&self) -> DocumentMode {
        self.doc.mode()
    }

    // ---- state inputs

    pub fn ui_flags(&self, n: NodeId) -> UiFlags {
        self.element(n)
            .map_or(UiFlags::empty(), ElementData::ui_flags)
    }
    /// Control facts used by pseudo-classes (checkedness, indeterminate, selectedness, validity
    /// flags, user validity, range state, value emptiness) via [`ControlState::flags`].
    pub fn control_state(&self, n: NodeId) -> Option<&'a ControlState> {
        self.element(n).and_then(ElementData::control_state)
    }

    /// Generation-encoding key (research R-02).
    pub fn opaque_key(&self, n: NodeId) -> NonZeroU64 {
        crate::ids::to_opaque(n)
    }

    // ---- stylesheets

    pub fn stylesheets(&self) -> Vec<StylesheetEntry> {
        self.doc.stylesheets()
    }
    pub fn stylesheet_revision(&self) -> u64 {
        self.doc.stylesheet_revision()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_reads_every_listed_item() {
        let d = Document::parse_html("<!DOCTYPE html><style>a{}</style><div id=a class='x y' hidden><p>1</p>t<template><b>c</b></template><input type=checkbox checked></div>").unwrap();
        let v = d.view();
        let a = d.element_by_id("a").unwrap();
        assert_eq!(v.id_attr(a), Some("a"));
        assert_eq!(v.classes(a).collect::<Vec<_>>(), ["x", "y"]);
        assert_eq!(v.attributes(a).len(), 3);
        assert!(v.is_html_element_in_html_document(a));
        assert_eq!(v.document_mode(), DocumentMode::NoQuirks);
        let kids = v.children(a);
        assert_eq!(kids.len(), 4);
        assert_eq!(v.next_element_sibling(kids[0]), Some(kids[2]));
        assert_eq!(v.prev_element_sibling(kids[2]), Some(kids[0]));
        assert_eq!(v.prev_sibling(kids[0]), None);
        let tmpl = kids[2];
        assert!(
            v.children(tmpl).is_empty(),
            "template contents are not children"
        );
        let contents = v.template_contents(tmpl).unwrap();
        assert!(!v.is_connected(v.first_child(contents).unwrap()));
        let input = kids[3];
        assert_eq!(v.control_state(input).unwrap().flags().checked, Some(true));
        assert_eq!(v.ui_flags(input), UiFlags::empty());
        assert_eq!(
            v.opaque_key(a).get() & 0xffff_ffff,
            u64::from(a.raw().index)
        );
        assert_eq!(v.stylesheets().len(), 1);
        assert_eq!(v.parent_element(kids[0]), Some(a));
        assert_eq!(v.kind(v.root()), NodeKind::Document);
    }
}
