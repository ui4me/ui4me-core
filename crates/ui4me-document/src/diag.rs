//! owner: 003-document-model
//!
//! Payload builders for 005's `dom.parser_diagnostics`, `dom.stylesheets`, `dom.counts` and
//! `dom.journal` probes (T032, T098; contracts/mutation-journal.md sections 2-3). 005 owns the probe
//! names, the JSON envelope, canonical serialisation and the export policy; this module hands it
//! plain data: entries with node references in the structural form (path over all child nodes from
//! the Document for connected nodes; the node's tree root plus a path from that root for detached
//! trees and template contents, 005 FR-076), and redaction *tags* (never the policy) for the
//! `value` of `type=password` inputs. All builders are read-only and never advance a revision.

use crate::control_state::ControlFlags;
use crate::document::{Document, DomCounts};
use crate::journal::{EntryKind, JournalEntry};
use crate::node::{Attribute, NodeData};
use crate::stylesheets::SheetOrigin;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// Structural node reference (005 `NodeKey`): `path` indexes over *all* child nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeRef {
    /// Reachable from the Document node.
    Connected { path: Vec<u32> },
    /// Not reachable from the Document: the tree root's slot identity and the path from that root.
    Detached { root: NodeId, path: Vec<u32> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParserDiagnosticsPayload {
    pub total: u64,
    /// The first 1,000 `{line, message}` records.
    pub records: Vec<(u64, String)>,
    /// `UNSUPPORTED_FEATURE` reports recorded at creation.
    pub unsupported: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetKind {
    Style,
    Link,
    Host,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetPayload {
    /// Stable id: the element's node reference for element-backed sheets, the attachment id otherwise.
    pub id: SheetId,
    pub kind: SheetKind,
    pub node: Option<NodeRef>,
    pub connected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetId {
    Element(NodeId),
    Attachment(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StylesheetsPayload {
    pub stylesheet_revision: u64,
    pub sheets: Vec<SheetPayload>,
}

/// An attribute as journaled, with a redaction tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrView {
    pub namespace: Option<String>,
    pub prefix: Option<String>,
    pub local_name: String,
    pub value: String,
    /// Under 005's export policy this value is redacted unless the password-values flag is set.
    pub redact: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryDetail {
    ChildList {
        added: Vec<NodeRef>,
        removed: Vec<NodeRef>,
        prev_sibling: Option<NodeRef>,
        next_sibling: Option<NodeRef>,
    },
    Attribute {
        namespace: Option<String>,
        local_name: String,
    },
    PriorAttributes {
        attributes: Vec<AttrView>,
    },
    CharacterData,
    ControlState {
        field: &'static str,
    },
    PriorControlFlags(ControlFlags),
    UiState {
        old: Vec<&'static str>,
        new: Vec<&'static str>,
    },
    Stylesheets {
        stylesheet_revision: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntryView {
    pub seq: u64,
    pub revision: u64,
    pub style_epoch: u64,
    pub kind: &'static str,
    /// The entry's node (the parent for `child_list`); `None` for `stylesheets`.
    pub node: Option<NodeRef>,
    pub detail: EntryDetail,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalPayload {
    pub since_revision: u64,
    pub entries: Vec<JournalEntryView>,
    pub truncated: bool,
    pub oldest_revision: u64,
}

impl Document {
    /// The structural reference of `n` (connected path or detached root + path).
    pub fn node_ref(&self, n: NodeId) -> NodeRef {
        let a = &self.arena;
        let mut path = vec![];
        let mut cur = n;
        while let Some(p) = a.get(cur).and_then(|x| x.parent()) {
            path.push(a.index_in_parent(cur).unwrap_or(0) as u32);
            cur = p;
        }
        path.reverse();
        if cur == a.document() {
            NodeRef::Connected { path }
        } else {
            NodeRef::Detached { root: cur, path }
        }
    }

    /// A reference for a node that may already be freed (journal entries outlive nodes): the root
    /// identity only.
    fn node_ref_or_gone(&self, n: NodeId) -> NodeRef {
        if self.arena.contains(n) {
            self.node_ref(n)
        } else {
            NodeRef::Detached {
                root: n,
                path: vec![],
            }
        }
    }

    /// `dom.parser_diagnostics` (FR-007, FR-064).
    pub fn parser_diagnostics_payload(&self) -> Result<ParserDiagnosticsPayload, Status> {
        self.check_open()?;
        let d = &self.parser_diagnostics;
        Ok(ParserDiagnosticsPayload {
            total: d.total(),
            records: d
                .records()
                .iter()
                .map(|r| (r.line, r.message.clone()))
                .collect(),
            unsupported: d.unsupported_features().to_vec(),
        })
    }

    /// `dom.counts`.
    pub fn counts_payload(&self) -> Result<DomCounts, Status> {
        self.check_open()?;
        Ok(self.counts())
    }

    /// `dom.stylesheets` (FR-087): the sources in list order with their connection state.
    pub fn stylesheets_payload(&self) -> Result<StylesheetsPayload, Status> {
        self.check_open()?;
        let sheets = self
            .stylesheets()
            .into_iter()
            .map(|e| match e.origin {
                SheetOrigin::Style(n) => SheetPayload {
                    id: SheetId::Element(n),
                    kind: SheetKind::Style,
                    node: Some(self.node_ref(n)),
                    connected: self.arena.is_connected(n),
                },
                SheetOrigin::Link(n) => SheetPayload {
                    id: SheetId::Element(n),
                    kind: SheetKind::Link,
                    node: Some(self.node_ref(n)),
                    connected: self.arena.is_connected(n),
                },
                SheetOrigin::Attached(a) => SheetPayload {
                    id: SheetId::Attachment(a.0),
                    kind: SheetKind::Host,
                    node: None,
                    connected: true,
                },
            })
            .collect();
        Ok(StylesheetsPayload {
            stylesheet_revision: self.stylesheet_revision(),
            sheets,
        })
    }

    fn attr_views(&self, node: NodeId, attrs: &[Attribute]) -> Vec<AttrView> {
        let password = self
            .arena
            .get(node)
            .and_then(|n| n.as_element())
            .is_some_and(|e| {
                e.is_html_named("input")
                    && e.attr("type")
                        .is_some_and(|t| t.eq_ignore_ascii_case("password"))
            });
        attrs
            .iter()
            .map(|a| AttrView {
                namespace: Some(a.name.ns.to_string()).filter(|s| !s.is_empty()),
                prefix: a
                    .name
                    .prefix
                    .as_ref()
                    .map(|p| p.to_string())
                    .filter(|s| !s.is_empty()),
                local_name: a.name.local.to_string(),
                value: a.value.clone(),
                redact: password && a.name.ns.is_empty() && &*a.name.local == "value",
            })
            .collect()
    }

    fn entry_view(&self, e: &JournalEntry) -> JournalEntryView {
        let r = |n: NodeId| self.node_ref_or_gone(n);
        let (kind, node, detail) = match &e.kind {
            EntryKind::ChildList {
                parent,
                added,
                removed,
                prev_sibling,
                next_sibling,
            } => (
                "child_list",
                Some(r(*parent)),
                EntryDetail::ChildList {
                    added: added.iter().map(|&n| r(n)).collect(),
                    removed: removed.iter().map(|&n| r(n)).collect(),
                    prev_sibling: prev_sibling.map(r),
                    next_sibling: next_sibling.map(r),
                },
            ),
            EntryKind::Attribute { node, name } => (
                "attribute",
                Some(r(*node)),
                EntryDetail::Attribute {
                    namespace: Some(name.ns.to_string()).filter(|s| !s.is_empty()),
                    local_name: name.local.to_string(),
                },
            ),
            EntryKind::PriorAttributes { node, attributes } => (
                "prior_attributes",
                Some(r(*node)),
                EntryDetail::PriorAttributes {
                    attributes: self.attr_views(*node, attributes),
                },
            ),
            EntryKind::CharacterData { node } => {
                ("character_data", Some(r(*node)), EntryDetail::CharacterData)
            }
            EntryKind::ControlState { node, field } => (
                "control_state",
                Some(r(*node)),
                EntryDetail::ControlState {
                    field: field.name(),
                },
            ),
            EntryKind::PriorControlFlags { node, flags } => (
                "prior_control_flags",
                Some(r(*node)),
                EntryDetail::PriorControlFlags(*flags),
            ),
            EntryKind::UiState { node, old, new } => (
                "ui_state",
                Some(r(*node)),
                EntryDetail::UiState {
                    old: old.names(),
                    new: new.names(),
                },
            ),
            EntryKind::Stylesheets {
                stylesheet_revision,
            } => (
                "stylesheets",
                None,
                EntryDetail::Stylesheets {
                    stylesheet_revision: *stylesheet_revision,
                },
            ),
        };
        JournalEntryView {
            seq: e.seq,
            revision: e.revision,
            style_epoch: e.style_epoch,
            kind,
            node,
            detail,
        }
    }

    /// `dom.journal` (FR-085): entries after `since_revision` still in the window (at least the last
    /// 4,096). A request older than the window returns what is retained with `truncated` and
    /// `oldest_revision`; never an error. Reading changes nothing (FR-086).
    pub fn journal_payload(&self, since_revision: u64) -> Result<JournalPayload, Status> {
        if self.is_closing() {
            return Err(Status::new(
                Code::DocumentClosed,
                ui4me_engine_types::stage::Stage::Host,
            ));
        }
        let w = self.journal.since(since_revision);
        Ok(JournalPayload {
            since_revision,
            truncated: w.truncated,
            oldest_revision: w.oldest_revision,
            entries: w.entries.iter().map(|e| self.entry_view(e)).collect(),
        })
    }

    /// Is `n` a template-contents node (for 005's tree dumps)?
    pub fn is_template_contents(&self, n: NodeId) -> bool {
        self.arena
            .get(n)
            .is_some_and(|x| matches!(x.data(), NodeData::DocumentFragment { template: Some(_) }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_refs_are_paths_for_connected_nodes_and_roots_for_detached_ones() {
        let mut d = Document::parse_html("<div id=a><p id=b></p></div>").unwrap();
        let b = d.element_by_id("b").unwrap();
        let NodeRef::Connected { path } = d.node_ref(b) else {
            panic!()
        };
        assert_eq!(
            path.len(),
            4,
            "document > html > body > div > p (indices over all children)"
        );
        let x = d.create_element("x").unwrap();
        let y = d.create_element("y").unwrap();
        d.append_child(x, y).unwrap();
        assert_eq!(
            d.node_ref(y),
            NodeRef::Detached {
                root: x,
                path: vec![0]
            }
        );
        assert_eq!(
            d.node_ref(x),
            NodeRef::Detached {
                root: x,
                path: vec![]
            }
        );
    }

    #[test]
    fn payloads_for_counts_diagnostics_and_stylesheets() {
        let d = Document::parse_html("<style>a{}</style></x><p>").unwrap();
        let p = d.parser_diagnostics_payload().unwrap();
        assert!(p.total >= 1 && !p.records.is_empty());
        let s = d.stylesheets_payload().unwrap();
        assert_eq!(s.sheets.len(), 1);
        assert!(s.sheets[0].connected && s.sheets[0].kind == SheetKind::Style);
        assert!(d.counts_payload().unwrap().connected > 4);
    }
}
