//! owner: 003-document-model — created by 001 slice (R-28)
//!
//! DocumentCore → InteractionRuntime hooks (T037; 003 `dom-internal-interfaces.md` §3; R-14).
//! Hooks are plain registered functions (not traits; AD-33). They run synchronously inside the
//! mutating call, never dispatch events, never run host code, and write only through the
//! [`UiStateWriter`] they are handed (whose writes belong to the call's single revision step).
//! They are never called for a failed call. The removal hook is what 012's focus fix-up
//! consumes: it clears focus flags in the removed subtree before the call returns.

use crate::ui_state::UiStateWriter;
use ui4me_engine_types::ids::{DocumentId, NodeId};

/// One removal step (`on_removed(root, old_parent)`), with the old siblings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Removal {
    pub root: NodeId,
    pub old_parent: NodeId,
    pub old_prev_sibling: Option<NodeId>,
    pub old_next_sibling: Option<NodeId>,
}

/// One attribute change (`on_attribute_changed(element, name, old, new)`, FR-099).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeChange {
    pub element: NodeId,
    pub local_name: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

pub type RemovedHook = Box<dyn FnMut(&mut UiStateWriter<'_>, &Removal)>;
pub type InsertedHook = Box<dyn FnMut(&mut UiStateWriter<'_>, NodeId)>;
pub type AttributeChangedHook = Box<dyn FnMut(&mut UiStateWriter<'_>, &AttributeChange)>;
pub type NodesFreedHook = Box<dyn FnMut(DocumentId, &[NodeId])>;
pub type DocumentClosedHook = Box<dyn FnMut(DocumentId)>;

/// The registered hooks of one document (012 registers them at document creation).
#[derive(Default)]
pub struct Hooks {
    pub(crate) removed: Option<RemovedHook>,
    pub(crate) inserted: Option<InsertedHook>,
    pub(crate) attribute_changed: Option<AttributeChangedHook>,
    pub(crate) nodes_freed: Option<NodesFreedHook>,
    pub(crate) document_closed: Option<DocumentClosedHook>,
    pub(crate) children_changed: Option<ChildrenChangedHook>,
    pub(crate) notifications: Option<NotificationSink>,
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks")
            .field("removed", &self.removed.is_some())
            .field("inserted", &self.inserted.is_some())
            .field("attribute_changed", &self.attribute_changed.is_some())
            .field("nodes_freed", &self.nodes_freed.is_some())
            .field("document_closed", &self.document_closed.is_some())
            .field("children_changed", &self.children_changed.is_some())
            .field("notifications", &self.notifications.is_some())
            .finish()
    }
}

impl Hooks {
    /// `on_removed(root, old_parent)`: inside the removing steps (FR-081; R-14).
    pub fn set_on_removed(&mut self, h: RemovedHook) {
        self.removed = Some(h);
    }
    /// `on_inserted(root)`: after the insertion steps.
    pub fn set_on_inserted(&mut self, h: InsertedHook) {
        self.inserted = Some(h);
    }
    pub fn set_on_attribute_changed(&mut self, h: AttributeChangedHook) {
        self.attribute_changed = Some(h);
    }
    /// `on_nodes_freed(doc, ids)`: at a collection checkpoint (FR-026).
    pub fn set_on_nodes_freed(&mut self, h: NodesFreedHook) {
        self.nodes_freed = Some(h);
    }
    /// `on_document_closed(doc)`: on entering `Closed` (FR-091).
    pub fn set_on_document_closed(&mut self, h: DocumentClosedHook) {
        self.document_closed = Some(h);
    }
}

// ---------------------------------------------------------------- children-changed hook (T016, T087)

pub type ChildrenChangedHook = Box<dyn FnMut(&mut UiStateWriter<'_>, NodeId)>;

impl Hooks {
    /// `on_children_changed(parent)` (FR-099): `select`, `datalist`, an `option`'s parent.
    pub fn set_on_children_changed(&mut self, h: ChildrenChangedHook) {
        self.children_changed = Some(h);
    }

    /// Installed notification sink (007), or none.
    pub fn set_notification_sink(&mut self, s: NotificationSink) {
        self.notifications = Some(s);
    }
}

// ---------------------------------------------------------------- requester notifications (T016, T042; FR-100)

/// The FR-100 requester element kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RequesterKind {
    Img,
    Link,
    Style,
    Script,
    Iframe,
    Object,
    Embed,
    Video,
    Audio,
    Source,
    Track,
    InputImage,
    Svg,
}

/// What happened to a requester.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequesterChange {
    Connected,
    Disconnected,
    /// A request-affecting attribute changed (`src`, `srcset`, `href`, ...).
    AttributeChanged(String),
}

/// Post-commit notifications to ResourceManager (007; dom-internal-interfaces.md section 5). They
/// are delivered synchronously before the mutating call returns, in command order inside a batch,
/// and never after a failed call or a rolled-back batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notification {
    RequestersEnumerated(Vec<(NodeId, RequesterKind)>),
    RequesterChanged {
        element: NodeId,
        kind: RequesterKind,
        change: RequesterChange,
    },
    StylesheetLinkChanged {
        element: NodeId,
        href: Option<String>,
    },
    BaseUrlChanged {
        url: Option<String>,
    },
    DocumentClosed,
}

pub type NotificationSink = Box<dyn FnMut(&Notification)>;

/// Request-affecting attributes (FR-100).
pub const REQUEST_ATTRIBUTES: &[&str] = &[
    "src", "srcset", "href", "rel", "type", "media", "sizes", "data", "poster",
];

/// The requester kind of an element (FR-100): `img`, `link`, `style`, `script[src]`, `iframe`,
/// `object`, `embed`, `video`, `audio`, `source`, `track`, `input type=image`, inline `svg`.
pub fn requester_kind(e: &crate::node::ElementData) -> Option<RequesterKind> {
    use html5ever::ns;
    if e.name().ns == *"http://www.w3.org/2000/svg" && e.local_name() == "svg" {
        return Some(RequesterKind::Svg);
    }
    if e.name().ns != ns!(html) {
        return None;
    }
    Some(match e.local_name() {
        "img" => RequesterKind::Img,
        "link" => RequesterKind::Link,
        "style" => RequesterKind::Style,
        "script" if e.attr("src").is_some() => RequesterKind::Script,
        "iframe" => RequesterKind::Iframe,
        "object" => RequesterKind::Object,
        "embed" => RequesterKind::Embed,
        "video" => RequesterKind::Video,
        "audio" => RequesterKind::Audio,
        "source" => RequesterKind::Source,
        "track" => RequesterKind::Track,
        "input"
            if e.attr("type")
                .is_some_and(|t| t.eq_ignore_ascii_case("image")) =>
        {
            RequesterKind::InputImage
        }
        _ => return None,
    })
}

/// FR-099: does a change of attribute `name` on this element call `on_attribute_changed`?
/// Elements `input`, `textarea`, `select`, `option`, `form`, `fieldset`, `label`, `details`,
/// `dialog` (any attribute); `tabindex`, `disabled`, `inert`, `hidden`, `popover` on any element.
pub fn attribute_hook_applies(e: &crate::node::ElementData, name: &str) -> bool {
    if matches!(
        name,
        "tabindex" | "disabled" | "inert" | "hidden" | "popover"
    ) {
        return true;
    }
    e.is_html()
        && matches!(
            e.local_name(),
            "input"
                | "textarea"
                | "select"
                | "option"
                | "form"
                | "fieldset"
                | "label"
                | "details"
                | "dialog"
        )
}

/// Does `parent` take the `on_children_changed` hook (FR-099): `select`, `datalist`, or an
/// `option`'s parent (an `optgroup` holding options).
pub fn children_hook_applies(parent: &crate::node::ElementData) -> bool {
    parent.is_html() && matches!(parent.local_name(), "select" | "datalist" | "optgroup")
}
