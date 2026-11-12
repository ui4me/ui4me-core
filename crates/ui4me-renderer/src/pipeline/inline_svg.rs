//! F0907 — inline `<svg>` as a replaced element.
//!
//! The DOM subtree of an inline `<svg>` is serialised into a standalone SVG document and handed to
//! the ordinary bounded vector-image path (007: prescan, usvg/resvg, per-size raster cache), exactly
//! as `<img src="data:image/svg+xml,...">` would be. The serialisation is where the CSS meets SVG:
//!
//! * only the static elements of `docs/reference/SVG_PROFILE.md` are written;
//!   `foreignObject`, animation elements, scripts and event attributes never reach the rasteriser;
//! * `href`/`url()` references are limited to same-document fragments (and `data:` images), so an
//!   inline SVG can never trigger a fetch;
//! * the style engine has no SVG presentation properties, so the author rules of the document's
//!   `<style>` elements that set one (`fill`, `stroke`, ... but also `opacity`, `transform`,
//!   `display` on a shape) are matched here with the engine's own selector matcher, ordered by
//!   specificity, and written as the element's `style`; inherited ones also come from HTML
//!   ancestors (`.icon { fill: currentColor }`). `currentColor` is the computed `color` of the
//!   `<svg>`;
//! * element count, depth, attribute and document size are capped.
//!
//! A rejected subtree renders nothing and reports `svg.inline.rejected`.

use crate::style::query::{ParsedSelectorList, SelectorService};
use crate::style::view::ComputedStyleView;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use ui4me_assets::svg::hit::PointerMode;
use ui4me_document::document::Document;
use ui4me_engine_types::ids::{DocumentId, NodeId};

/// Most elements serialised (the rasteriser applies its own, larger limit on top).
pub const MAX_ELEMENTS: usize = 2_000;
/// Deepest element nesting serialised.
pub const MAX_DEPTH: usize = 48;
/// Longest single attribute value (path data, point lists).
pub const MAX_ATTR_BYTES: usize = 64 * 1024;
/// Longest serialised document.
pub const MAX_MARKUP_BYTES: usize = 512 * 1024;
/// Most rules of the document considered.
const MAX_RULES: usize = 4_000;

/// The presentation properties an author rule may set on SVG content, written to the element's
/// `style`. (`inherited` ones also flow from HTML ancestors.)
const PROPS: &[(&str, bool)] = &[
    ("fill", true),
    ("fill-opacity", true),
    ("fill-rule", true),
    ("stroke", true),
    ("stroke-width", true),
    ("stroke-opacity", true),
    ("stroke-linecap", true),
    ("stroke-linejoin", true),
    ("stroke-miterlimit", true),
    ("stroke-dasharray", true),
    ("stroke-dashoffset", true),
    ("clip-rule", true),
    ("color", true),
    ("pointer-events", true),
    ("marker-start", true),
    ("marker-mid", true),
    ("marker-end", true),
    ("shape-rendering", true),
    ("stop-color", false),
    ("stop-opacity", false),
    ("flood-color", false),
    ("flood-opacity", false),
    ("opacity", false),
    ("display", false),
    ("visibility", true),
    ("transform", false),
    ("clip-path", false),
    ("mask", false),
    ("filter", false),
];
/// Properties that belong to the `<svg>` element's CSS box (laid out and composited by HTML), not
/// to its content.
const BOX_PROPS: &[&str] = &[
    "opacity",
    "display",
    "visibility",
    "transform",
    "clip-path",
    "mask",
    "filter",
];

/// Elements written (the static profile). Everything else is dropped with its subtree.
const ELEMENTS: &[&str] = &[
    "svg",
    "g",
    "defs",
    "symbol",
    "use",
    "path",
    "rect",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "text",
    "tspan",
    "textPath",
    "title",
    "desc",
    "linearGradient",
    "radialGradient",
    "stop",
    "pattern",
    "clipPath",
    "mask",
    "marker",
    "filter",
    "feBlend",
    "feColorMatrix",
    "feComponentTransfer",
    "feComposite",
    "feDropShadow",
    "feFlood",
    "feFuncA",
    "feFuncB",
    "feFuncG",
    "feFuncR",
    "feGaussianBlur",
    "feImage",
    "feMerge",
    "feMergeNode",
    "feMorphology",
    "feOffset",
    "feTurbulence",
    "feDiffuseLighting",
    "feSpecularLighting",
    "feDistantLight",
    "fePointLight",
    "feSpotLight",
    "feConvolveMatrix",
    "feDisplacementMap",
    "feTile",
    "image",
    "a",
    "switch",
];
const TEXT_PARENTS: &[&str] = &["text", "tspan", "textPath", "title", "desc"];

/// Elements that are never written and are reported when present.
fn unsupported_element(name: &str) -> Option<&'static str> {
    match name {
        "foreignObject" => Some("svg.foreignobject"),
        "animate" | "animateMotion" | "animateTransform" | "set" | "discard" | "mpath" => {
            Some("svg.animation")
        }
        "script" => Some("svg.script"),
        _ => None,
    }
}

/// Element id written in the markup -> the DOM element and its `pointer-events`.
pub type Targets = HashMap<String, (NodeId, PointerMode)>;

/// One serialised inline SVG.
#[derive(Debug)]
pub struct Serialized {
    pub markup: String,
    pub targets: Targets,
    pub unsupported: Vec<&'static str>,
}

struct Rule {
    selector: ParsedSelectorList,
    specificity: (u32, u32, u32),
    order: usize,
    decls: Vec<(String, String)>,
}

#[derive(Default)]
struct Rules {
    list: Vec<Rule>,
}

thread_local! {
    static SERVICE: SelectorService = SelectorService::new();
    static RULES: RefCell<Option<(DocumentId, u64, Rc<Rules>)>> = const { RefCell::new(None) };
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `(ids, classes/attributes/pseudo-classes, types)` of a simple selector text.
fn specificity(sel: &str) -> (u32, u32, u32) {
    let (mut a, mut b, mut c) = (0, 0, 0);
    let bytes = sel.as_bytes();
    let mut i = 0;
    let mut in_attr = false;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                if !in_attr {
                    b += 1;
                }
                in_attr = true;
            }
            b']' => in_attr = false,
            _ if in_attr => {}
            b'#' => a += 1,
            b'.' => b += 1,
            b':' => {
                if bytes.get(i + 1) == Some(&b':') {
                    c += 1;
                    i += 1;
                } else {
                    b += 1;
                }
            }
            ch if ch.is_ascii_alphabetic() || ch == b'_' => {
                let prev = if i == 0 { b' ' } else { bytes[i - 1] };
                let start = !(prev.is_ascii_alphanumeric()
                    || matches!(prev, b'-' | b'_' | b'#' | b'.' | b':' | b'\\'));
                if start {
                    c += 1;
                }
                while i + 1 < bytes.len()
                    && (bytes[i + 1].is_ascii_alphanumeric() || matches!(bytes[i + 1], b'-' | b'_'))
                {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (a, b, c)
}

/// Split at top-level commas (not inside parentheses, brackets or quotes).
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0);
    for (i, ch) in s.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, c) if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn parse_decls(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for d in split_top(text, ';') {
        let Some((name, value)) = d.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        if !PROPS.iter().any(|(p, _)| *p == name) {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_suffix("!important")
            .map_or(value, |v| v.trim_end());
        if !value.is_empty() {
            out.push((name, value.to_owned()));
        }
    }
    out
}

fn parse_sheet(text: &str, svc: &SelectorService, out: &mut Vec<Rule>) {
    let text = strip_comments(text);
    let mut rest = text.as_str();
    while out.len() < MAX_RULES {
        let Some(open) = rest.find('{') else { break };
        let prelude = rest[..open].trim();
        // The matching close brace (at-rule blocks nest).
        let mut depth = 0;
        let mut close = None;
        for (i, ch) in rest[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { break };
        let body = &rest[open + 1..close];
        rest = &rest[close + 1..];
        if prelude.starts_with('@') {
            // `@media`, `@supports`, `@font-face`, ...: not applied to SVG content.
            continue;
        }
        let decls = parse_decls(body);
        if decls.is_empty() {
            continue;
        }
        for sel in split_top(prelude, ',') {
            let sel = sel.trim();
            if sel.is_empty() {
                continue;
            }
            if let Ok(selector) = svc.parse(sel) {
                let order = out.len();
                out.push(Rule {
                    selector,
                    specificity: specificity(sel),
                    order,
                    decls: decls.clone(),
                });
            }
        }
    }
}

fn collect_style_text(doc: &Document, root: NodeId, out: &mut String) {
    let mut stack = vec![root];
    let mut visited = 0usize;
    let mut order = Vec::new();
    while let Some(n) = stack.pop() {
        visited += 1;
        if visited > 200_000 {
            break;
        }
        if doc.local_name(n).is_ok_and(|t| t == "style") {
            order.push(n);
            continue;
        }
        if let Ok(children) = doc.children(n) {
            for &c in children.iter().rev() {
                stack.push(c);
            }
        }
    }
    for n in order {
        if let Ok(Some(t)) = doc.text_content(n) {
            out.push_str(&t);
            out.push('\n');
        }
    }
}

fn rules_of(doc: &Document, svc: &SelectorService) -> Rc<Rules> {
    let key = doc.dom_revision().ok().map(|r| (doc.id(), r));
    if let Some((id, rev)) = key
        && let Some(hit) = RULES.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|(i, r, _)| *i == id && *r == rev)
                .map(|(_, _, rules)| rules.clone())
        })
    {
        return hit;
    }
    let mut text = String::new();
    collect_style_text(doc, doc.document_node(), &mut text);
    let mut rules = Rules::default();
    parse_sheet(&text, svc, &mut rules.list);
    let rules = Rc::new(rules);
    if let Some((id, rev)) = key {
        RULES.with(|c| *c.borrow_mut() = Some((id, rev, rules.clone())));
    }
    rules
}

fn escape(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
}

/// A value that may only reference the same document: every `url(` must point at a fragment.
fn local_refs_only(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("url(") {
        let after = rest[i + 4..].trim_start();
        let after = after.trim_start_matches(['"', '\'']);
        if !after.starts_with('#') {
            return false;
        }
        rest = &rest[i + 4..];
    }
    !lower.contains("@import") && !lower.contains("expression(")
}

fn css_transform_to_svg(v: &str) -> String {
    // CSS lengths and angles carry units the SVG attribute grammar does not take.
    let mut out = String::new();
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            out.push_str(&v[i..j]);
            let unit_end = v[j..]
                .find(|c: char| !c.is_ascii_alphabetic())
                .map_or(v.len(), |k| j + k);
            if matches!(&v[j..unit_end], "px" | "deg") {
                i = unit_end;
            } else {
                i = j;
            }
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

fn pointer_mode(v: &str, inherited: PointerMode) -> PointerMode {
    match v.trim().to_ascii_lowercase().as_str() {
        "none" => PointerMode::None,
        "all" | "visible" => PointerMode::All,
        "fill" | "visiblefill" => PointerMode::Fill,
        "stroke" | "visiblestroke" => PointerMode::Stroke,
        "painted" | "visiblepainted" | "auto" => PointerMode::Painted,
        _ => inherited,
    }
}

struct Writer<'a> {
    doc: &'a Document,
    svc: &'a SelectorService,
    rules: &'a Rules,
    view: &'a ComputedStyleView<'a>,
    out: String,
    targets: HashMap<String, (NodeId, PointerMode)>,
    unsupported: Vec<&'static str>,
    elements: usize,
    next_id: usize,
    root: NodeId,
}

impl Writer<'_> {
    fn substitute_var(&self, value: &str) -> Option<String> {
        let mut out = String::new();
        let mut rest = value;
        while let Some(i) = rest.find("var(") {
            out.push_str(&rest[..i]);
            let inner_start = i + 4;
            let mut depth = 1;
            let mut end = None;
            for (k, ch) in rest[inner_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(inner_start + k);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let end = end?;
            let inner = &rest[inner_start..end];
            let (name, fallback) = match inner.split_once(',') {
                Some((n, f)) => (n.trim(), Some(f.trim())),
                None => (inner.trim(), None),
            };
            if !name.starts_with("--") {
                return None;
            }
            let v = self
                .view
                .custom(name)
                .filter(|v| !v.trim().is_empty())
                .or_else(|| fallback.map(str::to_owned))?;
            out.push_str(v.trim());
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        Some(out)
    }

    /// The `style` declarations contributed by matching author rules, lowest priority first.
    fn rule_decls(&self, n: NodeId, root_box: bool, only_inherited: bool) -> Vec<(String, String)> {
        let mut hits: Vec<&Rule> = self
            .rules
            .list
            .iter()
            .filter(|r| {
                self.svc
                    .matches(self.doc, &r.selector, n, None)
                    .unwrap_or(false)
            })
            .collect();
        hits.sort_by_key(|r| (r.specificity, r.order));
        let mut out = Vec::new();
        for r in hits {
            for (name, value) in &r.decls {
                let inherited = PROPS.iter().any(|(p, i)| p == name && *i);
                // `color` is the computed one (written on the root): rules only matter below it.
                if name == "color" && (only_inherited || n == self.root) {
                    continue;
                }
                if only_inherited && !inherited {
                    continue;
                }
                if root_box && BOX_PROPS.contains(&name.as_str()) {
                    continue;
                }
                let Some(value) = self.substitute_var(value) else {
                    continue;
                };
                if !local_refs_only(&value) {
                    continue;
                }
                let value = if name == "transform" {
                    css_transform_to_svg(&value)
                } else {
                    value
                };
                out.push((name.clone(), value));
            }
        }
        out
    }

    fn element(
        &mut self,
        n: NodeId,
        depth: usize,
        inherited_pe: PointerMode,
        ancestor_style: &[(String, String)],
    ) -> Result<(), &'static str> {
        let doc = self.doc;
        let name = doc
            .local_name(n)
            .map_err(|_| "svg.inline.rejected")?
            .to_owned();
        if let Some(feature) = unsupported_element(&name) {
            if !self.unsupported.contains(&feature) {
                self.unsupported.push(feature);
            }
            return Ok(());
        }
        if !ELEMENTS.contains(&name.as_str()) {
            return Ok(());
        }
        if name == "text" && !self.unsupported.contains(&"svg.text") {
            self.unsupported.push("svg.text");
        }
        self.elements += 1;
        if self.elements > MAX_ELEMENTS || depth > MAX_DEPTH {
            return Err("svg.inline.rejected");
        }
        let is_root = n == self.root;
        let attrs = doc.attributes(n).map_err(|_| "svg.inline.rejected")?;
        // A standalone SVG decoder has no containing HTML viewport against
        // which to resolve a root percentage. Keep the HTML attribute intact
        // for layout, but give the image decoder the viewBox's intrinsic size.
        // This is particularly common for icons with width/height="100%".
        let intrinsic = if is_root {
            attrs
                .iter()
                .find(|a| &*a.name.local == "viewBox")
                .and_then(|a| viewbox_size(a.value.as_str()))
        } else {
            None
        };
        let mut id = String::new();
        let mut own_style = String::new();
        let mut pe = inherited_pe;
        let mut written = String::new();
        for a in attrs {
            let local: &str = &a.name.local;
            let value = a.value.as_str();
            if value.len() > MAX_ATTR_BYTES {
                return Err("svg.inline.rejected");
            }
            let ns: &str = &a.name.ns;
            let qname = match ns {
                "" => local.to_owned(),
                n if n.ends_with("/xlink") => format!("xlink:{local}"),
                n if n.ends_with("/XML/1998/namespace") => format!("xml:{local}"),
                _ => continue,
            };
            match qname.as_str() {
                "id" => id = value.to_owned(),
                "style" => own_style = value.to_owned(),
                "class" | "tabindex" | "xmlns" => {}
                q if q.starts_with("on") || q.starts_with("data-") || q.starts_with("aria-") => {}
                "href" | "xlink:href" => {
                    let v = value.trim();
                    let ok = v.starts_with('#')
                        || (name == "image" && v.to_ascii_lowercase().starts_with("data:"));
                    if !ok {
                        if !self.unsupported.contains(&"svg.external-reference") {
                            self.unsupported.push("svg.external-reference");
                        }
                        continue;
                    }
                    written.push_str(&format!(" {qname}=\""));
                    escape(v, &mut written);
                    written.push('"');
                }
                q => {
                    if !local_refs_only(value) {
                        continue;
                    }
                    if q == "pointer-events" {
                        pe = pointer_mode(value, pe);
                    }
                    if is_root && matches!(q, "style" | "transform") {
                        continue;
                    }
                    written.push_str(&format!(" {qname}=\""));
                    if let Some((w, h)) = intrinsic
                        && matches!(q, "width" | "height")
                        && value
                            .trim()
                            .strip_suffix('%')
                            .and_then(|v| v.trim().parse::<f64>().ok())
                            .is_some_and(|v| v.is_finite() && v > 0.0)
                    {
                        written.push_str(&(if q == "width" { w } else { h }).to_string());
                    } else {
                        escape(value, &mut written);
                    }
                    written.push('"');
                }
            }
        }
        // Cascade: ancestors' inherited rules, this element's rules, then its own `style`.
        let mut decls: Vec<(String, String)> = ancestor_style.to_vec();
        decls.extend(self.rule_decls(n, is_root, false));
        let mut style = String::new();
        for (k, v) in &decls {
            style.push_str(k);
            style.push(':');
            style.push_str(v);
            style.push(';');
            if k == "pointer-events" {
                pe = pointer_mode(v, pe);
            }
        }
        if !own_style.is_empty() && local_refs_only(&own_style) {
            let own = parse_decls(&own_style);
            for (k, v) in own {
                if is_root && BOX_PROPS.contains(&k.as_str()) {
                    continue;
                }
                if k == "pointer-events" {
                    pe = pointer_mode(&v, pe);
                }
                let v = if k == "transform" {
                    css_transform_to_svg(&v)
                } else {
                    v
                };
                style.push_str(&format!("{k}:{v};"));
            }
        }
        let shape = !matches!(
            name.as_str(),
            "svg"
                | "defs"
                | "title"
                | "desc"
                | "stop"
                | "linearGradient"
                | "radialGradient"
                | "pattern"
                | "clipPath"
                | "mask"
                | "marker"
                | "filter"
        ) && !name.starts_with("fe");
        if id.is_empty() && (shape || is_root) {
            self.next_id += 1;
            id = format!("__ui4me_{}", self.next_id);
        }
        if !id.is_empty() {
            self.targets.insert(id.clone(), (n, pe));
        }
        self.out.push('<');
        self.out.push_str(&name);
        if is_root {
            self.out.push_str(
                " xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"",
            );
        }
        if !id.is_empty() {
            self.out.push_str(" id=\"");
            escape(&id, &mut self.out);
            self.out.push('"');
        }
        self.out.push_str(&written);
        if !style.is_empty() || is_root {
            let mut full = String::new();
            if is_root && let Some(c) = self.view.serialize("color") {
                full.push_str(&format!("color:{c};"));
            }
            full.push_str(&style);
            self.out.push_str(" style=\"");
            escape(&full, &mut self.out);
            self.out.push('"');
        }
        if self.out.len() > MAX_MARKUP_BYTES {
            return Err("svg.inline.rejected");
        }
        let children = doc.children(n).map_err(|_| "svg.inline.rejected")?.to_vec();
        if children.is_empty() {
            self.out.push_str("/>");
            return Ok(());
        }
        self.out.push('>');
        for c in children {
            match doc.node_kind(c) {
                Ok(ui4me_document::node::NodeKind::Element) => {
                    if doc
                        .namespace_uri(c)
                        .ok()
                        .flatten()
                        .is_some_and(|ns| ns.ends_with("/2000/svg"))
                    {
                        self.element(c, depth + 1, pe, &[])?;
                    }
                }
                Ok(ui4me_document::node::NodeKind::Text) => {
                    if TEXT_PARENTS.contains(&name.as_str())
                        && let Ok(Some(t)) = doc.text_content(c)
                    {
                        escape(&t, &mut self.out);
                    }
                }
                _ => {}
            }
        }
        self.out.push_str("</");
        self.out.push_str(&name);
        self.out.push('>');
        if self.out.len() > MAX_MARKUP_BYTES {
            return Err("svg.inline.rejected");
        }
        Ok(())
    }
}

fn viewbox_size(value: &str) -> Option<(f64, f64)> {
    let values: Vec<_> = value
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|v| !v.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<_, _>>()
        .ok()?;
    if values.len() == 4
        && values.iter().all(|v| v.is_finite())
        && values[2] > 0.0
        && values[3] > 0.0
    {
        Some((values[2], values[3]))
    } else {
        None
    }
}

/// Serialise the inline `<svg>` element `svg` (an element in the SVG namespace). `Err` carries the
/// feature id to report; the element then renders nothing.
pub fn serialize(
    doc: &Document,
    svg: NodeId,
    view: &ComputedStyleView<'_>,
) -> Result<Serialized, &'static str> {
    if !doc
        .namespace_uri(svg)
        .ok()
        .flatten()
        .is_some_and(|ns| ns.ends_with("/2000/svg"))
    {
        return Err("svg.inline.rejected");
    }
    // A nested `<svg>` is part of its outermost ancestor's picture.
    if let Ok(Some(parent)) = doc.parent(svg)
        && doc.local_name(parent).is_ok_and(|t| t == "svg")
        && doc
            .namespace_uri(parent)
            .ok()
            .flatten()
            .is_some_and(|ns| ns.ends_with("/2000/svg"))
    {
        return Err("svg.inline.nested");
    }
    SERVICE.with(|svc| {
        let rules = rules_of(doc, svc);
        let mut w = Writer {
            doc,
            svc,
            rules: &rules,
            view,
            out: String::new(),
            targets: HashMap::new(),
            unsupported: Vec::new(),
            elements: 0,
            next_id: 0,
            root: svg,
        };
        // Inherited properties of the HTML ancestors (outermost first).
        let mut inherited: Vec<(String, String)> = Vec::new();
        if !rules.list.is_empty() {
            let mut chain = Vec::new();
            let mut cur = doc.parent(svg).ok().flatten();
            while let Some(p) = cur {
                if doc.node_kind(p).ok() == Some(ui4me_document::node::NodeKind::Element) {
                    chain.push(p);
                }
                cur = doc.parent(p).ok().flatten();
            }
            for p in chain.into_iter().rev() {
                inherited.extend(w.rule_decls(p, false, true));
            }
        }
        w.element(svg, 0, PointerMode::Painted, &inherited)?;
        Ok(Serialized {
            markup: w.out,
            targets: w.targets,
            unsupported: w.unsupported,
        })
    })
}

/// `data:image/svg+xml,` URL of a serialised document (percent-encoded).
pub fn data_url(markup: &str) -> String {
    let mut s = String::with_capacity(markup.len() + 32);
    s.push_str("data:image/svg+xml;charset=utf-8,");
    for &b in markup.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b' '
            | b'/'
            | b':'
            | b'='
            | b'('
            | b')'
            | b','
            | b';'
            | b'\''
            | b'*'
            | b'!'
            | b'@' => s.push(b as char),
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specificity_counts() {
        assert_eq!(specificity("#a .b c"), (1, 1, 1));
        assert_eq!(specificity("svg.icon path"), (0, 1, 2));
        assert_eq!(specificity("a[href]:hover"), (0, 2, 1));
        assert_eq!(specificity("circle"), (0, 0, 1));
    }

    #[test]
    fn declarations_are_filtered_and_split() {
        let d = parse_decls("fill: red; margin: 1px; stroke:url(a;b) ; opacity:.5 !important");
        let names: Vec<_> = d.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["fill", "stroke", "opacity"]);
        assert_eq!(d[1].1, "url(a;b)");
        assert_eq!(d[2].1, ".5");
    }

    #[test]
    fn only_fragment_references_are_local() {
        assert!(local_refs_only("url(#g)"));
        assert!(local_refs_only("url('#g') red"));
        assert!(!local_refs_only("url(http://x/y.svg#g)"));
        assert!(!local_refs_only("url(file.png)"));
    }

    #[test]
    fn css_transform_loses_units() {
        assert_eq!(
            css_transform_to_svg("translate(10px, 5px) rotate(45deg)"),
            "translate(10, 5) rotate(45)"
        );
    }

    #[test]
    fn data_url_round_trips() {
        let m = "<svg a=\"#\">\u{e9}</svg>";
        let url = data_url(m);
        let p = ui4me_assets::data_url::process(&url, 1024).unwrap();
        assert_eq!(p.bytes, m.as_bytes());
        assert!(p.is_svg());
    }
}
