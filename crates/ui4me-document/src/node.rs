//! owner: 003-document-model — created by 001 slice (R-28)
//!
//! Node records stored in the arena (T035). Read access is public (`&Node`); every field is
//! written only by DocumentCore (`pub(crate)`), so the tree invariant the style adapter relies
//! on (each node has at most one parent and appears in exactly one child list; no cycles;
//! X-STYLE-ADAPTER review F2) cannot be broken from outside this crate.
//!
//! No style-engine type lives here (AD-07 purity): names are html5ever `QualName`s (the
//! qualified-name atoms shared by the parser and the style system, research R-07) and values
//! are plain strings.

use crate::control_state::ControlState;
use crate::ui_state::UiFlags;
use html5ever::{QualName, ns};
use ui4me_engine_types::ids::NodeId;

/// DOM node type (the slice subset; DOM `nodeType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Document,
    Doctype,
    Element,
    Text,
    Comment,
    DocumentFragment,
}

/// One attribute in stored (source) order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub name: QualName,
    pub value: String,
}

impl Attribute {
    /// Local-name match in the null namespace (HTML attributes).
    pub fn is(&self, local: &str) -> bool {
        self.name.ns == ns!() && &*self.name.local == local
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doctype {
    pub name: String,
    pub public_id: String,
    pub system_id: String,
}

/// Element-specific data.
#[derive(Clone, Debug)]
pub struct ElementData {
    pub(crate) name: QualName,
    pub(crate) attrs: Vec<Attribute>,
    /// Element UI-state flags (focus, hover, …), written only through `UiStateWriter` (R-38).
    pub(crate) ui: UiFlags,
    /// Stored control state (value, checkedness) for form controls; `None` otherwise.
    pub(crate) control: Option<ControlState>,
    /// The contents fragment of a `template` element (never a child, A-21).
    pub(crate) template_contents: Option<NodeId>,
    /// 1-based source line of the start tag for parser-created elements; 0 for script-created ones.
    pub(crate) source_line: u32,
    /// Resolved form owner of a form-associated element (forms.rs; FR-072).
    pub(crate) form_owner: Option<NodeId>,
    /// Created by the parser (not by the API): the HTML "parser inserted" flag for form association.
    pub(crate) parser_inserted: bool,
    /// MathML `annotation-xml` HTML integration point (recorded at creation by the parser).
    pub(crate) mathml_integration_point: bool,
}

impl ElementData {
    pub(crate) fn new(name: QualName, attrs: Vec<Attribute>) -> ElementData {
        let mut e = ElementData {
            name,
            attrs,
            ui: UiFlags::empty(),
            control: None,
            template_contents: None,
            source_line: 0,
            form_owner: None,
            parser_inserted: false,
            mathml_integration_point: false,
        };
        e.control = crate::control_state::initial_for(&e);
        e
    }
    pub fn form_owner(&self) -> Option<NodeId> {
        self.form_owner
    }
    pub fn is_parser_inserted(&self) -> bool {
        self.parser_inserted
    }
    pub fn is_mathml_integration_point(&self) -> bool {
        self.mathml_integration_point
    }
    pub fn name(&self) -> &QualName {
        &self.name
    }
    pub fn local_name(&self) -> &str {
        &self.name.local
    }
    pub fn is_html(&self) -> bool {
        self.name.ns == ns!(html)
    }
    /// Is this the HTML element `local`?
    pub fn is_html_named(&self, local: &str) -> bool {
        self.is_html() && &*self.name.local == local
    }
    pub fn attributes(&self) -> &[Attribute] {
        &self.attrs
    }
    /// Attribute value by local name in the null namespace.
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.is(local))
            .map(|a| a.value.as_str())
    }
    pub fn ui_flags(&self) -> UiFlags {
        self.ui
    }
    pub fn control_state(&self) -> Option<&ControlState> {
        self.control.as_ref()
    }
    pub fn template_contents(&self) -> Option<NodeId> {
        self.template_contents
    }
    /// Whitespace-separated class tokens (DOMTokenList over `class`).
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.attr("class").unwrap_or("").split_ascii_whitespace()
    }
}

#[derive(Clone, Debug)]
pub enum NodeData {
    Document,
    Doctype(Doctype),
    Element(ElementData),
    Text(String),
    Comment(String),
    /// `template` lists the owning template element for template contents.
    DocumentFragment {
        template: Option<NodeId>,
    },
}

/// One node of the arena.
#[derive(Clone, Debug)]
pub struct Node {
    pub(crate) id: NodeId,
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) data: NodeData,
    /// Host references (FR-022, AD-29); a detached tree with none is freed at a checkpoint.
    pub(crate) host_refs: u32,
    /// Dispatch-frame references from InteractionRuntime (AD-27, FR-025); held while a dispatch path is active.
    pub(crate) frame_refs: u32,
}

impl Node {
    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }
    pub fn data(&self) -> &NodeData {
        &self.data
    }
    pub fn kind(&self) -> NodeKind {
        match self.data {
            NodeData::Document => NodeKind::Document,
            NodeData::Doctype(_) => NodeKind::Doctype,
            NodeData::Element(_) => NodeKind::Element,
            NodeData::Text(_) => NodeKind::Text,
            NodeData::Comment(_) => NodeKind::Comment,
            NodeData::DocumentFragment { .. } => NodeKind::DocumentFragment,
        }
    }
    pub fn as_element(&self) -> Option<&ElementData> {
        match &self.data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }
    pub(crate) fn as_element_mut(&mut self) -> Option<&mut ElementData> {
        match &mut self.data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }
    /// Text or comment data.
    pub fn character_data(&self) -> Option<&str> {
        match &self.data {
            NodeData::Text(t) | NodeData::Comment(t) => Some(t),
            _ => None,
        }
    }
    pub fn is_element(&self) -> bool {
        matches!(self.data, NodeData::Element(_))
    }
    pub fn frame_refs(&self) -> u32 {
        self.frame_refs
    }
    pub fn host_refs(&self) -> u32 {
        self.host_refs
    }
}
