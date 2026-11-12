//! owner: 003-document-model — shared test helpers (html5lib-style tree dumper).
#![allow(dead_code)]

pub mod invariants;

use html5ever::{Namespace, ns};
use ui4me_document::document::Document;
use ui4me_document::node::{NodeData, NodeKind};
use ui4me_engine_types::ids::NodeId;

/// html5lib tree dump of the children of `root` (a Document or DocumentFragment), one `| ` line each.
pub fn dump_children(doc: &Document, root: NodeId) -> String {
    let mut out = String::new();
    for &c in doc.arena().node(root).children() {
        dump(doc, c, 0, &mut out);
    }
    out
}

fn ns_prefix(n: &Namespace) -> &'static str {
    if *n == ns!(svg) {
        "svg "
    } else if *n == ns!(mathml) {
        "math "
    } else {
        ""
    }
}

pub fn dump(doc: &Document, id: NodeId, depth: usize, out: &mut String) {
    // Iterative to survive deeply nested corpus cases.
    let mut stack: Vec<(NodeId, usize, bool)> = vec![(id, depth, false)];
    while let Some((id, depth, template_content)) = stack.pop() {
        let node = doc.arena().node(id);
        let indent = " ".repeat(depth * 2 + 1);
        let pad = format!("|{indent}");
        match node.data() {
            NodeData::Doctype(d) => {
                if d.public_id.is_empty() && d.system_id.is_empty() {
                    out.push_str(&format!("| <!DOCTYPE {}>\n", d.name));
                } else {
                    out.push_str(&format!(
                        "| <!DOCTYPE {} \"{}\" \"{}\">\n",
                        d.name, d.public_id, d.system_id
                    ));
                }
                let _ = template_content;
            }
            NodeData::Text(t) => out.push_str(&format!("{pad}\"{t}\"\n")),
            NodeData::Comment(t) => out.push_str(&format!("{pad}<!-- {t} -->\n")),
            NodeData::Element(e) => {
                out.push_str(&format!(
                    "{pad}<{}{}>\n",
                    ns_prefix(&e.name().ns),
                    e.local_name()
                ));
                let mut attrs: Vec<(String, &str)> = e
                    .attributes()
                    .iter()
                    .map(|a| {
                        let p = match &a.name.prefix {
                            Some(p) if !p.is_empty() => format!("{} {}", p, a.name.local),
                            _ => {
                                // namespaced attributes without a stored prefix use the namespace's conventional prefix
                                if a.name.ns == ns!(xlink) {
                                    format!("xlink {}", a.name.local)
                                } else if a.name.ns == ns!(xml) {
                                    format!("xml {}", a.name.local)
                                } else if a.name.ns == ns!(xmlns) {
                                    format!("xmlns {}", a.name.local)
                                } else {
                                    a.name.local.to_string()
                                }
                            }
                        };
                        (p, a.value.as_str())
                    })
                    .collect();
                attrs.sort();
                for (n, v) in attrs {
                    out.push_str(&format!("{pad}  {n}=\"{v}\"\n"));
                }
                // push children (and template contents) in reverse so they pop in order
                let kids: Vec<NodeId> = node.children().to_vec();
                for &c in kids.iter().rev() {
                    stack.push((c, depth + 1, false));
                }
                if let Some(t) = e.template_contents() {
                    let tkids: Vec<NodeId> = doc.arena().node(t).children().to_vec();
                    for &c in tkids.iter().rev() {
                        stack.push((c, depth + 2, true));
                    }
                    // the `content` line must come before the contents; emit it now by splicing
                    // it ahead of the already pushed content children.
                    out.push_str(&format!("{pad}  content\n"));
                    // children of a template element itself are not in the tree; nothing else to do.
                }
            }
            NodeData::Document | NodeData::DocumentFragment { .. } => {}
        }
        let _ = NodeKind::Element;
    }
}
