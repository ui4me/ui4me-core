//! Public read-only document interface of Core 0.6: HTML serialization (F0601), author
//! stylesheet attachment with parse diagnostics (F0602) and generation-safe node references for
//! traversal of unidentified elements, text and template contents (F0604).
//!
//! A node reference is the string `node:<index>:<generation>`; it names one DOM node for as long
//! as that node exists in the document (connected, or inside a connected `template`'s contents).
//! A reference to a removed node is a stale-handle error (code 4), never another node.

use super::*;
use ui4me_document::node::NodeKind;

mod explain;

/// Serialized output above this size is refused (limit, code 5).
const MAX_SERIALIZED: usize = 4 * 1024 * 1024;
/// Attached author sheets per document.
const MAX_SHEETS: usize = 64;
/// CSS source per sheet.
const MAX_CSS: usize = 1024 * 1024;
/// Diagnostics returned per parse.
const MAX_SHEET_DIAGNOSTICS: usize = 50;
/// Children listed per call.
pub const MAX_CHILDREN: usize = 4096;
/// Properties per `computed_style` call.
pub const MAX_STYLE_PROPERTIES: usize = 64;
/// Rectangles returned by one `text_rects` call.
const MAX_TEXT_RECTS: usize = 4096;

fn stale(message: impl Into<String>) -> Error {
    Error {
        code: 4,
        message: message.into(),
    }
}

/// The public reference of a node.
pub fn node_ref(n: NodeId) -> String {
    let r = n.raw();
    format!("node:{}:{}", r.index, r.generation.get())
}

fn parse_ref(reference: &str) -> Result<NodeId> {
    let bad = || arg(format!("Invalid node reference: {reference}"));
    let rest = reference.strip_prefix("node:").ok_or_else(bad)?;
    let (index, generation) = rest.split_once(':').ok_or_else(bad)?;
    let index: u32 = index.parse().map_err(|_| bad())?;
    let generation: u32 = generation.parse().map_err(|_| bad())?;
    let generation = NonZeroU32::new(generation).ok_or_else(bad)?;
    Ok(NodeId::from_raw(RawId { index, generation }))
}

fn kind_name(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Document => "document",
        NodeKind::Doctype => "doctype",
        NodeKind::Element => "element",
        NodeKind::Text => "text",
        NodeKind::Comment => "comment",
        NodeKind::DocumentFragment => "fragment",
    }
}

pub(super) fn model_live_ref(d: &Model, reference: &str) -> Result<NodeId> {
    let n = parse_ref(reference)?;
    if !d.arena().contains(n) {
        return Err(stale(format!("Stale node reference: {reference}")));
    }
    if d.is_connected(n).unwrap_or(false) {
        return Ok(n);
    }
    // Inside the contents of a connected template.
    let mut root = n;
    while let Ok(Some(p)) = d.parent(root) {
        root = p;
    }
    let in_template = d.node_kind(root).ok() == Some(NodeKind::DocumentFragment)
        && d.arena()
            .subtree(d.document_node())
            .into_iter()
            .any(|t| d.template_contents(t).ok().flatten() == Some(root));
    if in_template {
        Ok(n)
    } else {
        Err(stale(format!("Stale node reference: {reference}")))
    }
}

impl Document {
    /// Resolve a node reference to a live node of this document.
    pub(super) fn live_ref(&self, reference: &str) -> Result<NodeId> {
        model_live_ref(self.pipeline.document(), reference)
    }

    /// `element` (an HTML id) or `node` (a node reference), or the document when both are absent.
    pub(super) fn target_of(&self, element: Option<&str>, node: Option<&str>) -> Result<NodeId> {
        match (element, node) {
            (Some(_), Some(_)) => Err(arg("Give either element or node, not both")),
            (Some(e), None) => self.node(e),
            (None, Some(r)) => self.live_ref(r),
            (None, None) => Ok(self.pipeline.document().document_node()),
        }
    }

    /// Serialize `element`/`node` (or the document): `outer` includes the node itself (for the
    /// document both are the whole document). Control values are not markup and are not included.
    pub fn get_html(
        &self,
        element: Option<&str>,
        node: Option<&str>,
        outer: bool,
    ) -> Result<String> {
        let n = self.target_of(element, node)?;
        let d = self.pipeline.document();
        let html = if n == d.document_node() {
            d.serialize_document()?
        } else if outer {
            d.outer_html(n)?
        } else {
            d.inner_html(n)?
        };
        if html.len() > MAX_SERIALIZED {
            return Err(Error {
                code: 5,
                message: format!("Serialized HTML exceeds {MAX_SERIALIZED} bytes"),
            });
        }
        Ok(html)
    }

    /// Parse diagnostics of `css` as an author sheet (bounded), without applying it.
    fn sheet_diagnostics(&self, css: &str) -> Vec<Value> {
        let lock = style::shared_lock::SharedRwLock::new();
        let (_sheet, records) = crate::style::sheets::parse_sheet(
            css,
            ui4me_engine_types::diag::SheetOrigin::Author,
            0,
            &lock,
            &crate::style::sheets::blank_url(),
            style::context::QuirksMode::NoQuirks,
        );
        records
            .iter()
            .take(MAX_SHEET_DIAGNOSTICS)
            .map(|r| {
                json!({
                    "code": r.code.name(),
                    "category": r.category,
                    "feature": r.feature.map(|f| f.as_str()),
                    "line": r.line,
                    "column": r.column,
                    "excerpt": r.excerpt,
                })
            })
            .collect()
    }

    fn check_css(css: &str) -> Result<()> {
        if css.len() > MAX_CSS {
            return Err(Error {
                code: 5,
                message: format!("CSS exceeds {MAX_CSS} bytes"),
            });
        }
        if css.contains('\0') {
            return Err(arg("NUL is unsupported"));
        }
        Ok(())
    }

    /// Attach an author stylesheet after the document's own sheets; returns its id and the parse
    /// diagnostics (invalid rules are dropped, the rest applies).
    pub fn add_stylesheet(&mut self, css: &str) -> Result<Value> {
        Self::check_css(css)?;
        let attached = self
            .pipeline
            .document()
            .stylesheets()
            .iter()
            .filter(|s| {
                matches!(
                    s.origin,
                    ui4me_document::stylesheets::SheetOrigin::Attached(_)
                )
            })
            .count();
        if attached >= MAX_SHEETS {
            return Err(Error {
                code: 5,
                message: format!("At most {MAX_SHEETS} attached stylesheets"),
            });
        }
        let diagnostics = self.sheet_diagnostics(css);
        let id = self.pipeline.mutate(|d| d.attach_stylesheet(css))?;
        self.after_mutation(false);
        Ok(json!({"sheet": id.0, "diagnostics": diagnostics}))
    }

    /// Replace the text of an attached sheet (unknown id: stale, code 4).
    pub fn replace_stylesheet(&mut self, sheet: u64, css: &str) -> Result<Value> {
        Self::check_css(css)?;
        let diagnostics = self.sheet_diagnostics(css);
        self.pipeline.mutate(|d| {
            d.replace_stylesheet(ui4me_document::stylesheets::AttachmentId(sheet), css)
        })?;
        self.after_mutation(false);
        Ok(json!({"sheet": sheet, "diagnostics": diagnostics}))
    }

    /// Detach an attached sheet (unknown id: stale, code 4).
    pub fn remove_stylesheet(&mut self, sheet: u64) -> Result<()> {
        self.pipeline
            .mutate(|d| d.detach_stylesheet(ui4me_document::stylesheets::AttachmentId(sheet)))?;
        self.after_mutation(false);
        Ok(())
    }

    /// The document's stylesheet sources in cascade order: `style`/`link` elements in tree order,
    /// then attached sheets.
    pub fn list_stylesheets(&self) -> Vec<Value> {
        use ui4me_document::stylesheets::SheetOrigin as O;
        let d = self.pipeline.document();
        d.stylesheets()
            .into_iter()
            .map(|s| {
                let (kind, element, sheet) = match s.origin {
                    O::Style(n) => ("style", Some(n), None),
                    O::Link(n) => ("link", Some(n), None),
                    O::Attached(id) => ("attached", None, Some(id.0)),
                };
                json!({
                    "kind": kind,
                    "sheet": sheet,
                    "node": element.map(node_ref),
                    "element": element.and_then(|n| d.get_attribute(n, "id").ok().flatten().map(str::to_owned)),
                    "href": s.href,
                    "media": s.media,
                    "length": s.text.as_ref().map(|t| t.len()),
                })
            })
            .collect()
    }

    /// The reference of `element` (or of the document).
    pub fn node_of(&self, element: Option<&str>) -> Result<String> {
        Ok(node_ref(self.target_of(element, None)?))
    }

    /// One node: type, name, id, attributes, text, parent, siblings and child count.
    pub fn get_node(&self, reference: &str) -> Result<Value> {
        let n = self.live_ref(reference)?;
        let d = self.pipeline.document();
        let kind = d.node_kind(n)?;
        let mut v = json!({
            "node": node_ref(n),
            "type": kind_name(kind),
            "parent": d.parent(n)?.map(node_ref),
            "child_count": d.children(n)?.len(),
        });
        if let Some(p) = d.parent(n)? {
            let kids = d.children(p)?;
            let i = kids.iter().position(|&k| k == n);
            v["previous"] = json!(i.and_then(|i| i.checked_sub(1)).map(|j| node_ref(kids[j])));
            v["next"] = json!(i.and_then(|i| kids.get(i + 1)).map(|&k| node_ref(k)));
        }
        match kind {
            NodeKind::Element => {
                v["name"] = json!(d.local_name(n)?);
                v["id"] = json!(d.get_attribute(n, "id").ok().flatten());
                let attrs: serde_json::Map<String, Value> = d
                    .attributes(n)?
                    .iter()
                    .map(|a| (a.name.local.to_string(), json!(a.value.to_string())))
                    .collect();
                v["attributes"] = Value::Object(attrs);
                if let Some(c) = d.template_contents(n)? {
                    v["content"] = json!(node_ref(c));
                }
            }
            NodeKind::Text | NodeKind::Comment => {
                v["text"] = json!(d.text_data(n)?);
            }
            NodeKind::Doctype => {
                v["name"] = json!(d.doctype().map(|t| t.name.to_string()));
            }
            _ => {}
        }
        Ok(v)
    }

    /// The children of a node, `offset..offset+limit` (limit ≤ [`MAX_CHILDREN`]), and the total.
    pub fn node_children(&self, reference: &str, offset: usize, limit: usize) -> Result<Value> {
        if limit == 0 || limit > MAX_CHILDREN {
            return Err(arg(format!("limit must be 1..{MAX_CHILDREN}")));
        }
        let n = self.live_ref(reference)?;
        let d = self.pipeline.document();
        let kids = d.children(n)?;
        let page: Vec<Value> = kids
            .iter()
            .skip(offset)
            .take(limit)
            .map(|&k| {
                let kind = d.node_kind(k).map(kind_name).unwrap_or("other");
                json!({
                    "node": node_ref(k),
                    "type": kind,
                    "name": d.local_name(k).ok(),
                    "id": d.get_attribute(k, "id").ok().flatten(),
                })
            })
            .collect();
        Ok(json!({"children": page, "total": kids.len()}))
    }

    /// The rendered text of `element`/`node` (the text a selection of it copies: its text
    /// blocks joined with `\n`) and the rectangles of its UTF-16 range `start..end` (default: all),
    /// viewport CSS px. An inline element resolves to its containing text block.
    pub fn text_rects(
        &mut self,
        element: Option<&str>,
        node: Option<&str>,
        start: Option<u32>,
        end: Option<u32>,
    ) -> Result<Value> {
        let n = self.target_of(element, node)?;
        let range = match (start, end) {
            (None, None) => None,
            (s, e) => Some((s.unwrap_or(0), e.unwrap_or(u32::MAX))),
        };
        let range = match range {
            Some((s, u32::MAX)) => {
                let (text, _) = self.text_range_rects(n, Some((0, 0)), 1)?;
                Some((s, text.encode_utf16().count() as u32))
            }
            r => r,
        };
        let (text, rects) = self.text_range_rects(n, range, MAX_TEXT_RECTS)?;
        Ok(json!({
            "text": text,
            "length": text.encode_utf16().count(),
            "rects": rects.iter().map(|&(x, y, w, h, v)| json!({"x": x, "y": y, "width": w, "height": h, "visible": v})).collect::<Vec<_>>(),
        }))
    }

    /// The text under a viewport point: the nearest identified element containing it and the
    /// UTF-16 offset in that element's rendered text a click would place a selection at.
    pub fn text_at_point(&mut self, x: f64, y: f64) -> Result<Value> {
        let Some((root, offset, owner)) = self.text_position_at(x, y)? else {
            return Ok(Value::Null);
        };
        let d = self.pipeline.document();
        Ok(json!({
            "element": owner.and_then(|o| d.get_attribute(o, "id").ok().flatten().map(str::to_owned)),
            "offset": offset,
            "block": node_ref(root),
        }))
    }

    /// Computed values of up to [`MAX_STYLE_PROPERTIES`] properties of an element (0.6 F0609),
    /// schema 1: per property `{"status": "supported", "value", "inherited"}`, or `unsupported`
    /// (profile-excluded, with the feature id) or `invalid` (unknown name). Custom properties
    /// (`--x`) are allowed. An element without a box resolves its values on demand.
    pub fn computed_style(
        &mut self,
        element: Option<&str>,
        node: Option<&str>,
        properties: &[String],
    ) -> Result<Value> {
        if properties.is_empty() || properties.len() > MAX_STYLE_PROPERTIES {
            return Err(arg(format!(
                "properties must list 1..{MAX_STYLE_PROPERTIES} names"
            )));
        }
        let n = self.target_of(element, node)?;
        if self.pipeline.document().node_kind(n)? != NodeKind::Element {
            return Err(arg("computed style needs an element"));
        }
        self.pipeline.flush()?;
        let mut out = serde_json::Map::new();
        for name in properties {
            let entry = match self.pipeline.style_bridge().computed_value(
                self.pipeline.document(),
                n,
                name,
            ) {
                Ok(value) => {
                    let inherited =
                        style::properties::PropertyId::parse_enabled_for_all_content(name)
                            .ok()
                            .and_then(|id| id.longhand_id())
                            .map(|l| l.inherited());
                    json!({"status": "supported", "value": value, "inherited": inherited.unwrap_or(name.starts_with("--"))})
                }
                Err(e)
                    if e.code == ui4me_engine_types::codes_generated::Code::UnsupportedFeature =>
                {
                    json!({"status": "unsupported", "feature": e.message})
                }
                Err(e) => {
                    json!({"status": "invalid", "reason": if e.message.is_empty() { e.code.name().to_owned() } else { e.message.clone() }})
                }
            };
            out.insert(name.clone(), entry);
        }
        Ok(json!({"schema": 1, "properties": out}))
    }
}
