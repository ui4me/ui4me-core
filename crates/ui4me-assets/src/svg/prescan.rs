//! owner: 007-resources (T048) — the owned SVG pre-scan (plan, SVG pipeline; FR-037, FR-039, FR-041).
//!
//! usvg invents a 100x100 size when the root has neither `width`/`height` nor `viewBox`, and it
//! silently drops `<text>` when its `text` feature is off. Both would violate FR-037/FR-039 and
//! constitution III, so the pre-scan (roxmltree, the parser usvg itself uses) decides the natural
//! size and ratio, detects `<text>`, counts elements for the early exit and rejects gzip.
//! DTDs are not accepted (entity expansion is an attack surface and no supported SVG needs one).
//!
//! F-03: usvg and resvg recurse on the native stack once per nesting level, and a reference
//! (`use`, `clip-path`, `mask`, `filter`, `fill`/`stroke` patterns, markers, `feImage`, gradient
//! `href`) continues that recursion inside the referenced element. The pre-scan computes the
//! **effective depth** — the longest such path — without recursing itself, and refuses documents
//! deeper than [`SVG_MAX_DEPTH`], documents whose references expand to more than
//! [`SVG_MAX_EXPANDED`] converted elements (usvg converts a referenced mask, clip path or pattern
//! once per use, so a chain where each link uses the previous one twice is exponential in time),
//! reference cycles (usvg breaks only one- and two-step cycles) and
//! style-sheet selectors with more than [`SVG_MAX_SELECTOR_COMPOUNDS`] compound selectors (matched
//! recursively by simplecss), all as `DECODE_FAILED` before usvg sees the bytes.

use crate::decode::SvgNatural;
pub use crate::policy::{SVG_MAX_DEPTH, SVG_MAX_EXPANDED, SVG_MAX_SELECTOR_COMPOUNDS};
use std::collections::HashMap;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

pub const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// What the pre-scan learned. `elements` counts every element of the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Prescan {
    pub natural: SvgNatural,
    pub has_text: bool,
    pub elements: u32,
    /// The root says `preserveAspectRatio="none"`: the content is stretched to any box it is drawn into.
    pub stretch: bool,
    /// The root's `viewBox` has a zero width or height: rendering is disabled (SVG 2 section 8.2), the
    /// image draws nothing.
    pub disabled: bool,
    /// Effective depth: the most nesting levels usvg/resvg recurse through, counting the content
    /// that references pull in (at most [`SVG_MAX_DEPTH`]). Nested `data:` SVG images add theirs.
    pub depth: u32,
}

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

/// An absolute CSS length in px; relative lengths (`%`, `em`, `ex`) and garbage are `None`.
pub fn absolute_length(s: &str) -> Option<f32> {
    let s = s.trim();
    // The number ends at the first character that cannot belong to it; `e`/`E` are exponent
    // markers only when a digit or sign follows ("em"/"ex" are units).
    let b = s.as_bytes();
    let cut = s
        .char_indices()
        .find(|&(i, c)| match c {
            '0'..='9' | '.' | '-' | '+' => false,
            'e' | 'E' => !b
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_digit() || *n == b'-' || *n == b'+'),
            _ => true,
        })
        .map_or(s.len(), |(i, _)| i);
    let (num, unit) = s.split_at(cut);
    let v: f32 = num.parse().ok()?;
    if !v.is_finite() {
        return None;
    }
    let k = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        _ => return None,
    };
    Some(v * k)
}

fn view_box_ratio(s: &str) -> Option<(f32, f32)> {
    let v: Vec<f32> = s
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<f32>().ok())
        .collect::<Option<_>>()?;
    match v.as_slice() {
        [_, _, w, h] if *w > 0.0 && *h > 0.0 && w.is_finite() && h.is_finite() => Some((*w, *h)),
        _ => None,
    }
}

/// Natural dimensions from the root attributes; nothing is invented.
pub fn natural_of(width: Option<&str>, height: Option<&str>, view_box: Option<&str>) -> SvgNatural {
    let w = width.and_then(absolute_length).filter(|v| *v > 0.0);
    let h = height.and_then(absolute_length).filter(|v| *v > 0.0);
    let vb = view_box.and_then(view_box_ratio);
    match (w, h) {
        (Some(w), Some(h)) => SvgNatural {
            width: Some(w),
            height: Some(h),
            ratio: Some((w, h)),
        },
        (Some(w), None) => SvgNatural {
            width: Some(w),
            height: vb.map(|(a, b)| w * b / a),
            ratio: vb,
        },
        (None, Some(h)) => SvgNatural {
            width: vb.map(|(a, b)| h * a / b),
            height: Some(h),
            ratio: vb,
        },
        (None, None) => SvgNatural {
            width: None,
            height: None,
            ratio: vb,
        },
    }
}

/// Scan one document. `max_nodes` bounds the element count (`SIZE_LIMIT`).
pub fn scan(bytes: &[u8], max_nodes: u32) -> Result<Prescan, Status> {
    if crate::decode::sniff::is_gzip(bytes) {
        return Err(err(
            Code::UnsupportedMediaType,
            "gzip-compressed content (SVGZ) is unsupported",
        ));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| err(Code::DecodeFailed, "SVG is not UTF-8"))?;
    let opt = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: max_nodes.saturating_mul(2).max(16),
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, opt).map_err(|e| match e {
        roxmltree::Error::NodesLimitReached => err(
            Code::SizeLimit,
            format!("SVG exceeds the {max_nodes}-element limit"),
        ),
        roxmltree::Error::DtdDetected => err(Code::DecodeFailed, "SVG with a DTD is not supported"),
        other => err(
            Code::DecodeFailed,
            format!("SVG is not well-formed: {other}"),
        ),
    })?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" || root.tag_name().namespace() != Some(SVG_NS) {
        return Err(err(
            Code::DecodeFailed,
            "the root element is not an SVG <svg>",
        ));
    }
    let mut elements = 0u32;
    let mut has_text = false;
    for n in doc.descendants().filter(|n| n.is_element()) {
        elements += 1;
        if elements > max_nodes {
            return Err(err(
                Code::SizeLimit,
                format!("SVG exceeds the {max_nodes}-element limit"),
            ));
        }
        if n.tag_name().name() == "text" && n.tag_name().namespace() == Some(SVG_NS) {
            has_text = true;
        }
    }
    let depth = effective_depth(&doc)?;
    let natural = natural_of(
        root.attribute("width"),
        root.attribute("height"),
        root.attribute("viewBox"),
    );
    let stretch = root
        .attribute("preserveAspectRatio")
        .is_some_and(|v| v.split_whitespace().find(|t| *t != "defer") == Some("none"));
    let disabled = root.attribute("viewBox").is_some_and(|s| {
        let v: Option<Vec<f32>> = s.split(|c: char| c.is_ascii_whitespace() || c == ',').filter(|p| !p.is_empty()).map(|p| p.parse::<f32>().ok()).collect();
        matches!(v.as_deref(), Some([_, _, w, h]) if *w >= 0.0 && *h >= 0.0 && (*w == 0.0 || *h == 0.0))
    });
    Ok(Prescan {
        natural,
        has_text,
        elements,
        stretch,
        disabled,
        depth,
    })
}

fn too_deep() -> Status {
    err(
        Code::DecodeFailed,
        format!("SVG nests deeper than {SVG_MAX_DEPTH} levels (counting referenced content)"),
    )
}

const NONE: u32 = u32::MAX;

/// How a reference continues the recursion.
#[derive(Clone, Copy, PartialEq)]
enum Via {
    /// Converted at the referencing element (`clip-path`, `mask`, `filter`, gradient `href`).
    Ref,
    /// Instantiated content (`use`, `feImage`): it also inherits the referencing element's paint.
    Use,
}

/// Every `url(#id)` in a property value.
fn url_ids(value: &str) -> impl Iterator<Item = &str> {
    value.match_indices("url(").filter_map(move |(i, _)| {
        let rest = value[i + 4..].trim_start();
        let rest = rest.strip_prefix(['"', '\'']).unwrap_or(rest);
        let rest = rest.strip_prefix('#')?;
        let end = rest.find([')', '"', '\'', ' ']).unwrap_or(rest.len());
        Some(rest[..end].trim()).filter(|id| !id.is_empty())
    })
}

/// The declarations of a `style` attribute (`name: value; ...`).
fn style_declarations(style: &str) -> impl Iterator<Item = (&str, &str)> {
    style.split(';').filter_map(|d| {
        let (name, value) = d.split_once(':')?;
        Some((name.trim(), value.trim()))
    })
}

struct XmlNode<'a, 'input>(roxmltree::Node<'a, 'input>);

/// The same matching usvg applies to the document's style sheets (its `XmlNode`).
impl simplecss::Element for XmlNode<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(XmlNode)
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(XmlNode)
    }
    fn has_local_name(&self, local_name: &str) -> bool {
        self.0.tag_name().name() == local_name
    }
    fn attribute_matches(&self, local_name: &str, operator: simplecss::AttributeOperator) -> bool {
        self.0
            .attribute(local_name)
            .is_some_and(|value| operator.matches(value))
    }
    fn pseudo_class_matches(&self, class: simplecss::PseudoClass) -> bool {
        matches!(class, simplecss::PseudoClass::FirstChild) && self.prev_sibling_element().is_none()
    }
}

/// Compound selectors of `selector` (combinators separate them in its canonical form).
fn compounds(selector: &simplecss::Selector<'_>) -> usize {
    selector
        .to_string()
        .split_ascii_whitespace()
        .filter(|t| !matches!(*t, ">" | "+" | "~"))
        .count()
}

/// The effective depth of `doc` (see the module documentation). Iterative: it runs on the same
/// worker stack it protects.
fn effective_depth(doc: &roxmltree::Document<'_>) -> Result<u32, Status> {
    // Plain element nesting first: cheap, and it bounds everything below (simplecss walks
    // ancestors).
    let mut index = vec![NONE; doc.descendants().count()];
    let mut elements: Vec<roxmltree::Node<'_, '_>> = Vec::new();
    let mut level: Vec<u32> = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        let parent_level = node
            .parent_element()
            .map_or(0, |p| level[index[p.id().get_usize()] as usize]);
        if parent_level + 1 > SVG_MAX_DEPTH {
            return Err(too_deep());
        }
        index[node.id().get_usize()] = elements.len() as u32;
        elements.push(node);
        level.push(parent_level + 1);
    }
    let of = |n: roxmltree::Node<'_, '_>| index[n.id().get_usize()];

    let mut ids: HashMap<&str, Vec<u32>> = HashMap::new();
    for (i, node) in elements.iter().enumerate() {
        if let Some(id) = node.attribute("id") {
            ids.entry(id).or_default().push(i as u32);
        }
    }

    // Author style sheets, as usvg reads them; only rules that reference something matter here.
    let mut sheet = simplecss::StyleSheet::new();
    for node in elements.iter().filter(|n| n.tag_name().name() == "style") {
        if matches!(node.attribute("type"), None | Some("text/css"))
            && let Some(text) = node.text()
        {
            sheet.parse_more(text);
        }
    }
    if sheet
        .rules
        .iter()
        .any(|r| compounds(&r.selector) > SVG_MAX_SELECTOR_COMPOUNDS)
    {
        return Err(err(
            Code::DecodeFailed,
            format!(
                "SVG style sheet has a selector of more than {SVG_MAX_SELECTOR_COMPOUNDS} compound selectors"
            ),
        ));
    }
    let rules: Vec<&simplecss::Rule<'_>> = sheet
        .rules
        .iter()
        .filter(|r| r.declarations.iter().any(|d| d.value.contains("url(")))
        .collect();

    // References and inherited paint per element.
    let mut refs: Vec<(u32, Via)> = Vec::new();
    let mut ref_start = Vec::with_capacity(elements.len() + 1);
    // Inherited paint state: an index into `states` (nearest declaration per INHERITED slot).
    let mut states: Vec<[u32; 5]> = vec![[NONE; 5]];
    let mut state_of: Vec<u32> = Vec::with_capacity(elements.len());
    for node in &elements {
        ref_start.push(refs.len() as u32);
        let tag = node.tag_name().name();
        let parent_state = node
            .parent_element()
            .map_or(0, |p| state_of[of(p) as usize]);
        let mut state = states[parent_state as usize];
        let mut changed = false;
        let mut declare = |name: &str, value: &str, refs: &mut Vec<(u32, Via)>| {
            let slots: &[usize] = match name {
                "fill" => &[0],
                "stroke" => &[1],
                "marker-start" => &[2],
                "marker-mid" => &[3],
                "marker-end" => &[4],
                "marker" => &[2, 3, 4],
                _ => &[],
            };
            let first = url_ids(value).next();
            if !slots.is_empty() {
                // The nearest declaration wins; anything but a resolvable url() stops inheritance.
                let target = first
                    .and_then(|id| ids.get(id))
                    .and_then(|t| t.first().copied())
                    .unwrap_or(NONE);
                for &k in slots {
                    state[k] = target;
                }
                changed = true;
                // Duplicate ids: every candidate counts.
                for id in url_ids(value) {
                    for &t in ids.get(id).into_iter().flatten().skip(1) {
                        refs.push((t, Via::Ref));
                    }
                }
                return;
            }
            for id in url_ids(value) {
                for &t in ids.get(id).into_iter().flatten() {
                    refs.push((t, Via::Ref));
                }
            }
        };
        for attr in node.attributes() {
            let (name, value) = (attr.name(), attr.value());
            if name == "href" {
                if let Some(id) = value.trim().strip_prefix('#') {
                    let via = if matches!(tag, "use" | "feImage") {
                        Via::Use
                    } else {
                        Via::Ref
                    };
                    if matches!(
                        tag,
                        "use"
                            | "feImage"
                            | "pattern"
                            | "linearGradient"
                            | "radialGradient"
                            | "filter"
                            | "textPath"
                    ) {
                        for &t in ids.get(id.trim()).into_iter().flatten() {
                            refs.push((t, via));
                        }
                    }
                }
            } else if name == "style" {
                for (n, v) in style_declarations(value) {
                    declare(n, v, &mut refs);
                }
            } else {
                declare(name, value, &mut refs);
            }
        }
        for rule in &rules {
            if rule.selector.matches(&XmlNode(*node)) {
                for d in &rule.declarations {
                    declare(d.name, d.value, &mut refs);
                }
            }
        }
        if changed {
            states.push(state);
            state_of.push(states.len() as u32 - 1);
        } else {
            state_of.push(parent_state);
        }
    }
    ref_start.push(refs.len() as u32);

    // Longest path over children, references and inherited paint; a back edge is a cycle.
    // `depth[e]` is the effective depth of e's subtree including e.
    let paint_server = |e: usize| {
        matches!(
            elements[e].tag_name().name(),
            "linearGradient" | "radialGradient"
        )
    };
    let successors = |e: usize, out: &mut Vec<(u32, Option<Via>)>| {
        out.clear();
        // Gradient stops are read as data, never converted as nodes.
        if !paint_server(e) {
            out.extend(
                elements[e]
                    .children()
                    .filter(|c| c.is_element())
                    .map(|c| (of(c), None)),
            );
            for &t in &states[state_of[e] as usize] {
                if t != NONE {
                    out.push((t, Some(Via::Ref)));
                }
            }
        }
        for &(t, via) in &refs[ref_start[e] as usize..ref_start[e + 1] as usize] {
            out.push((t, Some(via)));
        }
    };
    let cycle = || {
        err(
            Code::DecodeFailed,
            "SVG references form a cycle (use, clip-path, mask, filter, pattern or marker)",
        )
    };
    const WHITE: u8 = 0;
    const GREY: u8 = 1;
    const BLACK: u8 = 2;
    let mut color = vec![WHITE; elements.len()];
    let mut depth = vec![0u32; elements.len()];
    // Elements converted for e's subtree with every reference instantiated where it is used.
    let mut expanded = vec![0u64; elements.len()];
    let mut next = Vec::new();
    let mut stack: Vec<(u32, bool)> = vec![(0, false)];
    while let Some((e, exit)) = stack.pop() {
        let e = e as usize;
        if !exit {
            match color[e] {
                BLACK => continue,
                GREY => return Err(cycle()),
                _ => {}
            }
            color[e] = GREY;
            stack.push((e as u32, true));
            successors(e, &mut next);
            for &(s, _) in &next {
                match color[s as usize] {
                    WHITE => stack.push((s, false)),
                    GREY => return Err(cycle()),
                    _ => {}
                }
            }
            continue;
        }
        successors(e, &mut next);
        // Paint inherited at a `use` also applies inside the instantiated content.
        let inherited = states[state_of[e] as usize]
            .iter()
            .filter(|&&t| t != NONE)
            .map(|&t| depth[t as usize])
            .max()
            .unwrap_or(0);
        let mut below = 0u32;
        let mut size = 1u64;
        for &(s, via) in &next {
            let d = depth[s as usize];
            below = below.max(match via {
                Some(Via::Use) => d.saturating_add(inherited),
                _ => d,
            });
            size = size.saturating_add(expanded[s as usize]);
        }
        if size > SVG_MAX_EXPANDED {
            return Err(err(
                Code::DecodeFailed,
                format!("SVG references expand to more than {SVG_MAX_EXPANDED} elements"),
            ));
        }
        expanded[e] = size;
        let d = below.saturating_add(1);
        if d > SVG_MAX_DEPTH {
            // Every element is inside the root, so the root is at least this deep.
            return Err(too_deep());
        }
        depth[e] = d;
        color[e] = BLACK;
    }
    Ok(depth[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        assert_eq!(absolute_length("40"), Some(40.0));
        assert_eq!(absolute_length("40px"), Some(40.0));
        assert_eq!(absolute_length("1in"), Some(96.0));
        assert_eq!(absolute_length("12pt"), Some(16.0));
        assert_eq!(absolute_length("50%"), None);
        assert_eq!(absolute_length("2em"), None);
        assert_eq!(absolute_length("1e1"), Some(10.0));
        assert_eq!(absolute_length("abc"), None);
    }

    #[test]
    fn natural_sizes_invent_nothing() {
        assert_eq!(
            natural_of(None, None, None),
            SvgNatural {
                width: None,
                height: None,
                ratio: None
            }
        );
        assert_eq!(
            natural_of(None, None, Some("0 0 20 10")).ratio,
            Some((20.0, 10.0))
        );
        let n = natural_of(Some("40"), Some("20"), Some("0 0 1 1"));
        assert_eq!(
            (n.width, n.height, n.ratio),
            (Some(40.0), Some(20.0), Some((40.0, 20.0)))
        );
        let n = natural_of(Some("50%"), Some("20"), Some("0 0 10 5"));
        assert_eq!(
            (n.width, n.height),
            (Some(40.0), Some(20.0)),
            "the missing side follows the ratio"
        );
        assert_eq!(
            natural_of(Some("0"), Some("-3"), None),
            SvgNatural {
                width: None,
                height: None,
                ratio: None
            }
        );
    }

    #[test]
    fn root_preserve_aspect_ratio_none_and_a_zero_view_box() {
        let svg = |attrs: &str| format!("<svg xmlns='http://www.w3.org/2000/svg' {attrs}/>");
        let scan = |a: &str| scan(svg(a).as_bytes(), 100).unwrap();
        assert!(scan("viewBox='0 0 4 4' preserveAspectRatio='none'").stretch);
        assert!(scan("viewBox='0 0 4 4' preserveAspectRatio='defer none slice'").stretch);
        assert!(!scan("viewBox='0 0 4 4' preserveAspectRatio='xMidYMid meet'").stretch);
        assert!(!scan("viewBox='0 0 4 4'").stretch);
        // A zero width or height disables rendering (SVG 2 section 8.2); a negative one is an error, ignored.
        assert!(scan("viewBox='0 0 0 8'").disabled);
        assert!(scan("viewBox='0 0 8 0'").disabled);
        assert!(!scan("viewBox='0 0 8 8'").disabled);
        assert!(!scan("viewBox='0 0 -1 8'").disabled);
        assert!(!scan("width='8'").disabled);
    }
}
