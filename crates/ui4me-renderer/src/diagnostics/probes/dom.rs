//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `dom` probe input (neutral; diagnostics-schema-v0.md §5): the node tree from the Document.
//! T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::ExportCx;
use serde_json::{Map, Value, json};
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomNodeKind {
    Document,
    Doctype,
    Element,
    Text,
    Comment,
    /// A `template` element's contents fragment (never a child of the template).
    Fragment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomNodeInput {
    pub node: NodeId,
    pub kind: DomNodeKind,
    pub name: Option<String>,
    pub namespace: Option<String>,
    /// Source order.
    pub attributes: Vec<(String, String)>,
    pub data: Option<String>,
    /// All child nodes, in tree order.
    pub children: Vec<DomNodeInput>,
    /// The contents fragment of a `template` element (schema §5 `template_content`).
    pub template_content: Option<Box<DomNodeInput>>,
    /// A doctype's public and system identifiers (003; HTML "DocumentType"); `None` when empty.
    pub public_id: Option<String>,
    pub system_id: Option<String>,
}

const HTML_NS: &str = "http://www.w3.org/1999/xhtml";

fn is_password_control(n: &DomNodeInput) -> bool {
    n.kind == DomNodeKind::Element
        && n.name
            .as_deref()
            .is_some_and(|x| x.eq_ignore_ascii_case("input"))
        && n.namespace.as_deref().is_none_or(|ns| ns == HTML_NS)
        && n.attributes
            .iter()
            .any(|(k, v)| k == "type" && v.trim().eq_ignore_ascii_case("password"))
}

/// Every password control in the tree (redaction targets, schema §8).
pub(crate) fn password_controls(root: &DomNodeInput) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if is_password_control(n) {
            out.push(n.node);
        }
        stack.extend(n.children.iter());
    }
    out
}

impl DomNodeKind {
    pub const fn name(self) -> &'static str {
        match self {
            DomNodeKind::Document => "document",
            DomNodeKind::Doctype => "doctype",
            DomNodeKind::Element => "element",
            DomNodeKind::Text => "text",
            DomNodeKind::Comment => "comment",
            DomNodeKind::Fragment => "fragment",
        }
    }
}

fn node_json(n: &DomNodeInput, cx: &ExportCx) -> Value {
    let mut m = Map::new();
    m.insert("key".into(), cx.key(n.node));
    m.insert("type".into(), json!(n.kind.name()));
    if let Some(name) = &n.name {
        m.insert("name".into(), json!(name));
    }
    if let Some(ns) = &n.namespace {
        m.insert("namespace".into(), json!(ns));
    }
    if n.kind == DomNodeKind::Element {
        let redact_value = is_password_control(n) && !cx.redaction.password_values_included();
        let attrs: Vec<Value> = n
            .attributes
            .iter()
            .map(|(k, v)| {
                if redact_value && k == "value" {
                    json!([k, {"redacted": true}])
                } else {
                    json!([k, cx.redaction.scrub(v)])
                }
            })
            .collect();
        m.insert("attributes".into(), Value::Array(attrs));
    }
    if let Some(id) = &n.public_id {
        m.insert("public_id".into(), json!(id));
    }
    if let Some(id) = &n.system_id {
        m.insert("system_id".into(), json!(id));
    }
    if let Some(d) = &n.data {
        m.insert("data".into(), json!(cx.redaction.scrub(d)));
    }
    if let Some(t) = &n.template_content {
        m.insert("template_content".into(), node_json(t, cx));
    }
    if matches!(
        n.kind,
        DomNodeKind::Document | DomNodeKind::Element | DomNodeKind::Fragment
    ) {
        m.insert(
            "children".into(),
            Value::Array(n.children.iter().map(|c| node_json(c, cx)).collect()),
        );
    }
    Value::Object(m)
}

pub(crate) fn to_json(root: &DomNodeInput, cx: &ExportCx) -> Value {
    json!({"root": node_json(root, cx)})
}
