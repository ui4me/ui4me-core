//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Export `NodeKey` (diagnostics-schema-v0.md §4): `path` of child indices over all child
//! nodes from the Document node, `probe` (= `data-probe`), informative `label`.
//! Slice by 001 T051; pseudo/anonymous extensions, structural keys and the detached subtree
//! form by 005 T018/T019.
//!
//! A node absent from the DOM input (removed from the tree, or never inserted) has no connected
//! key; it is exported in the detached form with `root` = the node's own slot identity and an
//! empty `path` (the slice has no detached-subtree registry, so the node is its own root).

use crate::diagnostics::probes::dom::{DomNodeInput, DomNodeKind};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportNodeKey {
    pub path: Vec<u32>,
    pub probe: Option<String>,
    pub label: Option<String>,
    /// Pseudo-element extension (`::before`, …).
    pub pseudo: Option<String>,
    /// Anonymous-box owner extension (`anon: k`).
    pub anon: Option<u32>,
    /// The detached form (FR-076): the arena `(index, generation)` of the detached subtree's
    /// root; `path` is then relative to that root.
    pub detached_root: Option<(u32, u32)>,
}

/// Why a key index cannot be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// Two detached subtrees claim one `(index, generation)` root: a freed root must never be
    /// reused with the same identity (AD-29).
    DuplicateDetachedRoot,
}

/// The structural key of the box, fragment or display item `ordinal` of `owner`:
/// `<owner path joined with />[::pseudo][~anonK]#<ordinal>` (the Document is `/`). Identical
/// between incremental and cold runs because it uses only structure, never generation ids.
pub fn structural_key(owner: &ExportNodeKey, ordinal: u32) -> String {
    let mut s = String::from("/");
    s.push_str(
        &owner
            .path
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("/"),
    );
    if let Some(p) = &owner.pseudo {
        s.push_str(p);
    }
    if let Some(k) = owner.anon {
        s.push_str(&format!("~anon{k}"));
    }
    s.push('#');
    s.push_str(&ordinal.to_string());
    s
}

impl ExportNodeKey {
    /// The schema `node_key` object (connected form; see [`NodeKeyIndex::key_json`] for
    /// detached keys, which also need the document ordinal).
    pub fn to_json(&self) -> Value {
        self.to_json_in(0)
    }

    fn to_json_in(&self, document_ordinal: u32) -> Value {
        let mut m = Map::new();
        if let Some((index, generation)) = self.detached_root {
            m.insert("detached".into(), json!(true));
            m.insert("document".into(), json!(document_ordinal));
            m.insert(
                "root".into(),
                json!({"index": index, "generation": generation}),
            );
        }
        if let Some(p) = &self.pseudo {
            m.insert("pseudo".into(), json!(p));
        }
        if let Some(k) = self.anon {
            m.insert("anon".into(), json!(k));
        }
        m.insert("path".into(), json!(self.path));
        if let Some(p) = &self.probe {
            m.insert("probe".into(), json!(p));
        }
        if let Some(l) = &self.label {
            m.insert("label".into(), json!(l));
        }
        Value::Object(m)
    }
}

/// `NodeId` → export key, built once per export from the DOM input.
#[derive(Clone, Debug, Default)]
pub struct NodeKeyIndex {
    keys: HashMap<NodeId, ExportNodeKey>,
    /// Document ordinal used by the detached form.
    document_ordinal: u32,
    /// `data-probe` of nodes that left the tree (their detached key still names the probe, 002 D4).
    detached_probes: HashMap<NodeId, String>,
    /// Keys of nodes in registered detached subtrees (relative to their subtree root).
    detached: HashMap<NodeId, ExportNodeKey>,
}

impl NodeKeyIndex {
    pub fn build(document: &DomNodeInput) -> NodeKeyIndex {
        let mut keys = HashMap::new();
        let mut path = Vec::new();
        visit(document, &mut path, None, &mut keys);
        NodeKeyIndex {
            keys,
            document_ordinal: 1,
            detached_probes: HashMap::new(),
            detached: HashMap::new(),
        }
    }

    /// The document ordinal the detached form carries (default 1).
    pub fn with_document_ordinal(mut self, ordinal: u32) -> NodeKeyIndex {
        self.document_ordinal = ordinal.max(1);
        self
    }

    /// The `data-probe` values of detached nodes, carried into their detached key.
    pub fn with_detached_probes(mut self, probes: HashMap<NodeId, String>) -> NodeKeyIndex {
        self.detached_probes = probes;
        self
    }

    /// Register detached subtrees (roots not connected to the document, FR-076): their nodes
    /// get the detached form relative to the subtree root. Two subtrees with one root identity
    /// are rejected.
    pub fn with_detached_subtrees(
        mut self,
        roots: Vec<DomNodeInput>,
    ) -> Result<NodeKeyIndex, KeyError> {
        let mut seen = std::collections::HashSet::new();
        for root in &roots {
            let raw = root.node.raw();
            if !seen.insert((raw.index, raw.generation.get())) {
                return Err(KeyError::DuplicateDetachedRoot);
            }
            let mut keys = HashMap::new();
            visit(root, &mut Vec::new(), None, &mut keys);
            for (node, mut k) in keys {
                k.detached_root = Some((raw.index, raw.generation.get()));
                self.detached.insert(node, k);
            }
        }
        Ok(self)
    }

    /// The connected key of `node` (`None` for detached nodes).
    pub fn get(&self, node: NodeId) -> Option<&ExportNodeKey> {
        self.keys.get(&node)
    }

    /// A pseudo-element key of `owner` (`None` if `owner` is not a connected node).
    pub fn pseudo_key(&self, owner: NodeId, pseudo: &str) -> Option<ExportNodeKey> {
        self.get(owner).map(|k| ExportNodeKey {
            pseudo: Some(pseudo.to_string()),
            ..k.clone()
        })
    }

    pub fn pseudo_key_json(&self, owner: NodeId, pseudo: &str) -> Option<Value> {
        self.pseudo_key(owner, pseudo).map(|k| k.to_json())
    }

    /// An anonymous-box owner key: `k` distinguishes the anonymous boxes of one owner.
    pub fn anon_key(&self, owner: NodeId, k: u32) -> Option<ExportNodeKey> {
        self.get(owner).map(|key| ExportNodeKey {
            anon: Some(k),
            ..key.clone()
        })
    }

    pub fn anon_key_json(&self, owner: NodeId, k: u32) -> Option<Value> {
        self.anon_key(owner, k).map(|k| k.to_json())
    }

    /// The schema `node_key` for `node`: connected form when the node is in the DOM input,
    /// otherwise the detached form.
    pub fn key_json(&self, node: NodeId) -> Value {
        match self.get(node) {
            Some(k) => k.to_json(),
            None if self.detached.contains_key(&node) => {
                let mut k = self.detached[&node].clone();
                if k.probe.is_none() {
                    k.probe = self.detached_probes.get(&node).cloned();
                }
                k.to_json_in(self.document_ordinal)
            }
            None => {
                let raw = node.raw();
                let mut m = json!({"detached": true, "document": self.document_ordinal,
                       "root": {"index": raw.index, "generation": raw.generation.get()}, "path": []});
                if let Some(p) = self.detached_probes.get(&node) {
                    m["probe"] = json!(p);
                }
                m
            }
        }
    }
}

fn label_part(n: &DomNodeInput) -> Option<String> {
    match n.kind {
        DomNodeKind::Element => {
            let name = n.name.clone().unwrap_or_default();
            match n.attributes.iter().find(|(k, _)| k == "id") {
                Some((_, id)) if !id.is_empty() => Some(format!("{name}#{id}")),
                _ => Some(name),
            }
        }
        DomNodeKind::Text => Some("#text".into()),
        DomNodeKind::Comment => Some("#comment".into()),
        DomNodeKind::Doctype => Some("#doctype".into()),
        DomNodeKind::Document | DomNodeKind::Fragment => None,
    }
}

fn visit(
    n: &DomNodeInput,
    path: &mut Vec<u32>,
    parent_label: Option<&str>,
    keys: &mut HashMap<NodeId, ExportNodeKey>,
) {
    let label = label_part(n).map(|part| match parent_label {
        Some(p) => format!("{p}/{part}"),
        None => part,
    });
    let probe = (n.kind == DomNodeKind::Element)
        .then(|| {
            n.attributes
                .iter()
                .find(|(k, _)| k == "data-probe")
                .map(|(_, v)| v.clone())
        })
        .flatten();
    keys.insert(
        n.node,
        ExportNodeKey {
            path: path.clone(),
            probe,
            label: label.clone(),
            pseudo: None,
            anon: None,
            detached_root: None,
        },
    );
    for (i, c) in n.children.iter().enumerate() {
        path.push(i as u32);
        visit(c, path, label.as_deref(), keys);
        path.pop();
    }
    if let Some(t) = &n.template_content {
        // The contents fragment is keyed after the template's children.
        path.push(n.children.len() as u32);
        visit(t, path, label.as_deref(), keys);
        path.pop();
    }
}
