//! owner: 003-document-model — created by 001 slice (R-28), completed by T025
//!
//! html5ever `TreeSink` on the arena (AD-30; WHATWG HTML §13.2; research R-04). Every method takes
//! `&self` (the tree builder's contract); the arena sits in a `RefCell` owned by the sink for the
//! duration of one parse and is moved back into the Document by `finish`. The sink never runs
//! scripts and never calls host code.
//!
//! * `Handle = NodeId` (generation-checked ids from the arena).
//! * The template contents fragment is created with every `template` element (FR-014).
//! * The MathML `annotation-xml` integration-point flag is recorded at creation.
//! * Foster parenting (`append_based_on_parent_node`, `append_before_sibling`), `reparent_children`
//!   and `add_attrs_if_missing` are implemented against the arena; adjacent text merges.
//! * Declarative shadow roots are refused (`allow_declarative_shadow_roots` -> false, counted so
//!   one `UNSUPPORTED_FEATURE` diagnostic reports them; FR-018).
//! * A fragment parse uses a scratch root (`with_root`) in the same arena, so the real Document is
//!   never touched until the result is moved into a staging fragment (research R-05).

use crate::arena::Arena;
use crate::document::{DocumentMode, ParserDiagnostics};
use crate::node::{Attribute, Doctype, ElementData, NodeData};
use html5ever::QualName;
use html5ever::interface::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::StrTendril;
use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};
use ui4me_engine_types::ids::NodeId;

pub struct ArenaSink {
    arena: RefCell<Arena>,
    root: NodeId,
    diagnostics: RefCell<ParserDiagnostics>,
    mode: Cell<DocumentMode>,
    line: Cell<u64>,
}

/// What a finished parse hands back to the Document.
pub struct ParseOutput {
    pub arena: Arena,
    pub diagnostics: ParserDiagnostics,
    pub mode: DocumentMode,
}

impl ArenaSink {
    /// A sink whose parse root is the arena's Document node.
    pub fn new(arena: Arena) -> ArenaSink {
        let root = arena.document();
        ArenaSink::with_root(arena, root)
    }

    /// A sink whose parse root is `root` (a scratch node for fragment parsing).
    pub fn with_root(arena: Arena, root: NodeId) -> ArenaSink {
        ArenaSink {
            arena: RefCell::new(arena),
            root,
            diagnostics: RefCell::default(),
            mode: Cell::new(DocumentMode::NoQuirks),
            line: Cell::new(1),
        }
    }

    fn new_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        flags: &ElementFlags,
    ) -> NodeId {
        let attrs = attrs
            .into_iter()
            .map(|a| Attribute {
                name: a.name,
                value: a.value.to_string(),
            })
            .collect();
        let mut e = ElementData::new(name, attrs);
        e.source_line = self.line.get().min(u32::MAX as u64) as u32;
        e.parser_inserted = true;
        e.mathml_integration_point = flags.mathml_annotation_xml_integration_point;
        let template = e.is_html_named("template");
        let mut a = self.arena.borrow_mut();
        let id = a.alloc(NodeData::Element(e));
        if template {
            let frag = a.alloc(NodeData::DocumentFragment { template: Some(id) });
            a.get_mut(id)
                .and_then(|n| n.as_element_mut())
                .expect("new")
                .template_contents = Some(frag);
        }
        id
    }

    /// Append, merging adjacent text (tree-builder contract).
    fn append_to(&self, parent: NodeId, child: NodeOrText<NodeId>, before: Option<NodeId>) {
        let mut a = self.arena.borrow_mut();
        match child {
            NodeOrText::AppendText(text) => {
                let kids = &a.node(parent).children;
                let prev = match before {
                    Some(b) => kids
                        .iter()
                        .position(|c| *c == b)
                        .and_then(|i| i.checked_sub(1))
                        .map(|i| kids[i]),
                    None => kids.last().copied(),
                };
                if let Some(p) = prev
                    && let Some(NodeData::Text(t)) = a.get_mut(p).map(|n| &mut n.data)
                {
                    t.push_str(&text);
                    return;
                }
                let t = a.alloc(NodeData::Text(text.to_string()));
                a.insert_trusted(parent, t, before);
            }
            NodeOrText::AppendNode(node) => {
                a.insert_trusted(parent, node, before);
            }
        }
    }
}

impl TreeSink for ArenaSink {
    type Handle = NodeId;
    type Output = ParseOutput;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> ParseOutput {
        ParseOutput {
            arena: self.arena.into_inner(),
            diagnostics: self.diagnostics.into_inner(),
            mode: self.mode.get(),
        }
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        self.diagnostics
            .borrow_mut()
            .push(self.line.get(), msg.into_owned());
    }

    fn set_current_line(&self, line_number: u64) {
        self.line.set(line_number);
    }

    fn get_document(&self) -> NodeId {
        self.root
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.arena.borrow(), |a| {
            &a.node(*target)
                .as_element()
                .expect("elem_name on an element")
                .name
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        flags: ElementFlags,
    ) -> NodeId {
        self.new_element(name, attrs, &flags)
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.arena
            .borrow_mut()
            .alloc(NodeData::Comment(text.to_string()))
    }

    /// The HTML parser never creates processing instructions; kept as a comment if it does.
    fn create_pi(&self, target: StrTendril, data: StrTendril) -> NodeId {
        self.arena
            .borrow_mut()
            .alloc(NodeData::Comment(format!("?{target} {data}")))
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        self.append_to(*parent, child, None);
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.arena.borrow().node(*element).parent.is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let doc = self.get_document();
        let mut a = self.arena.borrow_mut();
        let d = a.alloc(NodeData::Doctype(Doctype {
            name: name.to_string(),
            public_id: public_id.to_string(),
            system_id: system_id.to_string(),
        }));
        a.insert_trusted(doc, d, None);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        self.arena
            .borrow()
            .node(*target)
            .as_element()
            .and_then(|e| e.template_contents)
            .expect("template element")
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.mode.set(match mode {
            QuirksMode::Quirks => DocumentMode::Quirks,
            QuirksMode::LimitedQuirks => DocumentMode::LimitedQuirks,
            QuirksMode::NoQuirks => DocumentMode::NoQuirks,
        });
    }

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let parent = self
            .arena
            .borrow()
            .node(*sibling)
            .parent
            .expect("sibling has a parent");
        self.append_to(parent, new_node, Some(*sibling));
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<html5ever::Attribute>) {
        let mut a = self.arena.borrow_mut();
        let e = a
            .get_mut(*target)
            .and_then(|n| n.as_element_mut())
            .expect("element");
        for at in attrs {
            if !e.attrs.iter().any(|x| x.name == at.name) {
                e.attrs.push(Attribute {
                    name: at.name,
                    value: at.value.to_string(),
                });
            }
        }
    }

    /// Parser form association: the tree builder passes the form element it tracks as the
    /// form-element pointer (forms.rs finishes the algorithm after the parse; FR-072).
    fn associate_with_form(
        &self,
        target: &NodeId,
        form: &NodeId,
        _nodes: (&NodeId, Option<&NodeId>),
    ) {
        let mut a = self.arena.borrow_mut();
        if let Some(e) = a.get_mut(*target).and_then(|n| n.as_element_mut()) {
            e.form_owner = Some(*form);
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.arena.borrow_mut().detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut a = self.arena.borrow_mut();
        let kids = a.node(*node).children.clone();
        for k in kids {
            a.insert_trusted(*new_parent, k, None);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeId) -> bool {
        self.arena
            .borrow()
            .node(*handle)
            .as_element()
            .is_some_and(|e| e.mathml_integration_point)
    }

    /// FR-018: declarative shadow roots are never attached; the template stays ordinary.
    fn allow_declarative_shadow_roots(&self, _intended_parent: &NodeId) -> bool {
        false
    }
}
