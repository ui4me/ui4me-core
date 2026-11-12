//! owner: 003-document-model
//!
//! HTML fragment serialization (T030; research R-06; FR-017, FR-060–FR-062): markup5ever's
//! `Serialize` implemented over the arena and driven by the upstream HTML serializer with the
//! scripting flag **disabled** (`noscript` content is ordinary markup, FR-005) and the
//! `ChildrenOnly` / `IncludeNode` scopes. Attributes come out in stored order, escaping follows the
//! HTML fragment serialization algorithm, void elements have no end tag, raw-text parents write text
//! unescaped. A `template` serializes its contents (FR-017). The walk is iterative; output is
//! bounded by the serialization limit (FR-093), failing with `SIZE_LIMIT` before returning anything.

use crate::document::Document;
use crate::errors::{size_limit, wrong_node_kind};
use crate::limits::LimitName;
use crate::node::{NodeData, NodeKind};
use html5ever::QualName;
use html5ever::serialize::{Serialize, SerializeOpts, Serializer, TraversalScope, serialize};
use std::io::{self, Write};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// A serializable view of one arena subtree.
pub struct ArenaView<'a> {
    pub doc: &'a Document,
    pub root: NodeId,
}

enum Op {
    Open(NodeId),
    Close(QualName),
}

impl Serialize for ArenaView<'_> {
    fn serialize<S: Serializer>(&self, s: &mut S, scope: TraversalScope) -> io::Result<()> {
        let a = self.doc.arena();
        let mut ops: Vec<Op> = vec![];
        let push_children = |ops: &mut Vec<Op>, n: NodeId| {
            let node = a.node(n);
            // A template's children are its contents' children (FR-017).
            let kids: &[NodeId] = match node.as_element().and_then(|e| e.template_contents()) {
                Some(t) => a.node(t).children(),
                None => node.children(),
            };
            for &c in kids.iter().rev() {
                ops.push(Op::Open(c));
            }
        };
        match scope {
            TraversalScope::IncludeNode => ops.push(Op::Open(self.root)),
            TraversalScope::ChildrenOnly(_) => push_children(&mut ops, self.root),
        }
        while let Some(op) = ops.pop() {
            match op {
                Op::Close(name) => s.end_elem(name)?,
                Op::Open(n) => match a.node(n).data() {
                    NodeData::Element(e) => {
                        s.start_elem(
                            e.name().clone(),
                            e.attributes()
                                .iter()
                                .map(|at| (&at.name, at.value.as_str())),
                        )?;
                        ops.push(Op::Close(e.name().clone()));
                        push_children(&mut ops, n);
                    }
                    NodeData::Text(t) => s.write_text(t)?,
                    NodeData::Comment(t) => s.write_comment(t)?,
                    NodeData::Doctype(d) => s.write_doctype(&d.name)?,
                    NodeData::Document | NodeData::DocumentFragment { .. } => {
                        push_children(&mut ops, n)
                    }
                },
            }
        }
        Ok(())
    }
}

/// A writer that fails once more than `limit` bytes were written.
struct Bounded {
    buf: Vec<u8>,
    limit: usize,
}

impl Write for Bounded {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if self.buf.len() + b.len() > self.limit {
            return Err(io::Error::other("serialization limit"));
        }
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Document {
    fn serialize_with(&self, root: NodeId, scope: TraversalScope) -> Result<String, Status> {
        let limit = usize::try_from(self.limits.serialization_bytes).unwrap_or(usize::MAX);
        let mut w = Bounded { buf: vec![], limit };
        let opts = SerializeOpts {
            scripting_enabled: false,
            traversal_scope: scope.clone(),
            create_missing_parent: false,
        };
        serialize(&mut w, &ArenaView { doc: self, root }, opts)
            .map_err(|_| size_limit(LimitName::SerializationBytes.name()))?;
        Ok(String::from_utf8(w.buf).expect("the serializer writes UTF-8"))
    }

    /// `innerHTML` getter (A-49): the fragment serialization of the children; a `template`
    /// serializes its contents. Elements and fragments only.
    pub fn inner_html(&self, n: NodeId) -> Result<String, Status> {
        let node = self.check(n)?;
        match node.kind() {
            NodeKind::Element => {
                let name = node.as_element().expect("element").name().clone();
                self.serialize_with(n, TraversalScope::ChildrenOnly(Some(name)))
            }
            NodeKind::DocumentFragment => {
                self.serialize_with(n, TraversalScope::ChildrenOnly(None))
            }
            _ => Err(wrong_node_kind()),
        }
    }

    /// `outerHTML` getter (A-49): the element and its descendants.
    pub fn outer_html(&self, n: NodeId) -> Result<String, Status> {
        if !self.check(n)?.is_element() {
            return Err(wrong_node_kind());
        }
        self.serialize_with(n, TraversalScope::IncludeNode)
    }

    /// Serialize the whole document (A-16): DOCTYPE, then the document element, and so on.
    pub fn serialize_document(&self) -> Result<String, Status> {
        self.check_open()?;
        self.serialize_with(self.arena.document(), TraversalScope::ChildrenOnly(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::codes_generated::Code;

    #[test]
    fn serializes_attributes_in_order_with_escaping_void_elements_and_raw_text() {
        let d = Document::parse_html("<!DOCTYPE html><div id=\"a\" title='x\"y&z' class=c>t &lt; <br><img src=i><style>a>b{}</style><script>if(a<b&&c){}</script><!--c--></div>").unwrap();
        let div = d.element_by_id("a").unwrap();
        let outer = d.outer_html(div).unwrap();
        assert_eq!(
            outer,
            "<div id=\"a\" title=\"x&quot;y&amp;z\" class=\"c\">t &lt; <br><img src=\"i\"><style>a>b{}</style><script>if(a<b&&c){}</script><!--c--></div>"
        );
        assert!(d.inner_html(div).unwrap().starts_with("t &lt; <br>"));
        assert!(
            d.serialize_document()
                .unwrap()
                .starts_with("<!DOCTYPE html><html>")
        );
    }

    #[test]
    fn noscript_is_ordinary_content_and_templates_serialize_their_contents() {
        let d = Document::parse_html(
            "<body><noscript><p>a&b</p></noscript><template id=t><i>x</i></template>",
        )
        .unwrap();
        let body = d.body().unwrap();
        assert_eq!(
            d.inner_html(body).unwrap(),
            "<noscript><p>a&amp;b</p></noscript><template id=\"t\"><i>x</i></template>"
        );
        let t = d.element_by_id("t").unwrap();
        assert_eq!(
            d.inner_html(t).unwrap(),
            "<i>x</i>",
            "innerHTML of a template is its contents"
        );
    }

    #[test]
    fn the_output_limit_fails_with_size_limit_and_returns_nothing() {
        let d = Document::parse_html("<p>0123456789</p>").unwrap();
        let mut d = d.with_limits(crate::limits::DomLimits {
            serialization_bytes: 8,
            ..Default::default()
        });
        let p = d.body().unwrap();
        let e = d.inner_html(p).unwrap_err();
        assert_eq!(e.code, Code::SizeLimit);
        assert!(e.message.contains("dom.serialization_bytes"));
        let _ = &mut d;
    }

    #[test]
    fn wrong_kinds_are_rejected() {
        let d = Document::parse_html("<p>x</p>").unwrap();
        let t = d.children(d.body().unwrap()).unwrap()[0];
        let txt = d.children(t).unwrap()[0];
        assert_eq!(d.outer_html(txt).unwrap_err().code, Code::InvalidArgument);
        assert_eq!(d.inner_html(txt).unwrap_err().code, Code::InvalidArgument);
    }
}
