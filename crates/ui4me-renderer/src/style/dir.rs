//! owner: 006-style-engine (015 WM-2, lane L-WM)
//!
//! The directionality of an element (HTML "the directionality of an element"), which `:dir(ltr | rtl)`
//! matches (Selectors 4 section 7.1; parsed by the carried Stylo patch P-006-8, register UP-12). It is
//! independent of the CSS `direction` property:
//!
//! * `dir=ltr` / `dir=rtl`: that direction;
//! * `dir=auto` (and a `bdi` element without a valid `dir`): the direction of the first character of
//!   bidirectional class L, R or AL in the element's descendant text, skipping `bdi`, `script`, `style`
//!   and `textarea` elements and elements with a valid `dir` of their own (a `textarea` and a text `input`
//!   look at their value); with none, `ltr` (measured in the reference browser under a `dir=rtl` parent);
//! * otherwise the parent's directionality; the root element without `dir` is `ltr`.
//!
//! Invalidation (`traverse.rs`): a document whose sheets use `:dir(` restyles the subtree of an element
//! whose `dir` attribute changed, and of the nearest `dir=auto` ancestor of changed text or of an inserted
//! or removed node.

use ui4me_document::document::Document;
use ui4me_engine_types::ids::NodeId;

/// A valid `dir` attribute value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirAttr {
    Ltr,
    Rtl,
    Auto,
}

pub(crate) fn dir_attr(doc: &Document, n: NodeId) -> Option<DirAttr> {
    let v = doc.get_attribute(n, "dir").ok().flatten()?;
    match v.trim().to_ascii_lowercase().as_str() {
        "ltr" => Some(DirAttr::Ltr),
        "rtl" => Some(DirAttr::Rtl),
        "auto" => Some(DirAttr::Auto),
        _ => None,
    }
}

fn is_element(doc: &Document, n: NodeId) -> bool {
    doc.local_name(n).is_ok()
}

/// Whether the element's directionality is right-to-left.
pub(crate) fn is_rtl(doc: &Document, n: NodeId) -> bool {
    let mut cur = Some(n);
    while let Some(e) = cur {
        if !is_element(doc, e) {
            return false;
        }
        let attr = dir_attr(doc, e)
            .or_else(|| (doc.local_name(e).ok() == Some("bdi")).then_some(DirAttr::Auto));
        match attr {
            Some(DirAttr::Ltr) => return false,
            Some(DirAttr::Rtl) => return true,
            // With no strong character the element is left-to-right (HTML; as the reference browser, whatever the
            // parent's directionality).
            Some(DirAttr::Auto) => return auto_direction(doc, e).unwrap_or(false),
            None => {}
        }
        cur = doc.parent(e).ok().flatten().filter(|p| is_element(doc, *p));
    }
    false
}

/// The direction of the first strong character of an auto-directed element's text (`None`: none).
fn auto_direction(doc: &Document, e: NodeId) -> Option<bool> {
    let name = doc.local_name(e).unwrap_or("");
    if name == "input" {
        return first_strong(doc.get_attribute(e, "value").ok().flatten().unwrap_or(""));
    }
    if name == "textarea" {
        return doc
            .text_content(e)
            .ok()
            .flatten()
            .and_then(|t| first_strong(&t));
    }
    first_strong_in(doc, e)
}

fn first_strong_in(doc: &Document, e: NodeId) -> Option<bool> {
    for &c in doc.children(e).ok()? {
        if doc.node_kind(c).ok() == Some(ui4me_document::node::NodeKind::Text) {
            if let Some(d) = doc.text_data(c).ok().and_then(first_strong) {
                return Some(d);
            }
            continue;
        }
        if !is_element(doc, c) {
            continue;
        }
        let name = doc.local_name(c).unwrap_or("");
        if matches!(name, "bdi" | "script" | "style" | "textarea") || dir_attr(doc, c).is_some() {
            continue;
        }
        if let Some(d) = first_strong_in(doc, c) {
            return Some(d);
        }
    }
    None
}

/// The elements whose subtree must be restyled because the directionality of some element in it may have
/// changed in `segment` (a document whose sheets use `:dir(`): an element whose `dir` attribute changed, and the
/// nearest auto-directed ancestor of changed text, changed children, a changed `dir` attribute or a changed
/// control value.
pub(crate) fn invalidation_roots(
    doc: &Document,
    segment: &ui4me_document::journal::EpochSegment,
) -> Vec<NodeId> {
    use ui4me_document::journal::EntryKind;
    let arena = doc.arena();
    let live = |n: NodeId| arena.contains(n) && arena.is_connected(n);
    let mut out: Vec<NodeId> = Vec::new();
    let push = |n: NodeId, out: &mut Vec<NodeId>| {
        if live(n) && !out.contains(&n) {
            out.push(n);
        }
    };
    for e in segment.entries() {
        let start = match &e.kind {
            EntryKind::Attribute { node, name } if &*name.local == "dir" => {
                push(*node, &mut out);
                doc.parent(*node).ok().flatten()
            }
            EntryKind::CharacterData { node } => doc.parent(*node).ok().flatten(),
            EntryKind::ChildList { parent, .. } => Some(*parent),
            EntryKind::ControlState { node, .. } => Some(*node),
            _ => None,
        };
        if let Some(a) = start
            .filter(|s| live(*s))
            .and_then(|s| nearest_auto(doc, s))
        {
            push(a, &mut out);
        }
    }
    out
}

/// The nearest inclusive ancestor of `n` whose directionality comes from its text (`dir=auto`, a `bdi` without
/// a valid `dir`), stopping at an element whose `dir` fixes its direction.
fn nearest_auto(doc: &Document, n: NodeId) -> Option<NodeId> {
    let mut cur = Some(n);
    while let Some(e) = cur.filter(|e| is_element(doc, *e)) {
        match dir_attr(doc, e) {
            Some(DirAttr::Auto) => return Some(e),
            Some(_) => return None,
            None if doc.local_name(e).ok() == Some("bdi") => return Some(e),
            None => {}
        }
        cur = doc.parent(e).ok().flatten();
    }
    None
}

/// The first character of bidirectional class R or AL (`Some(true)`) or L (`Some(false)`).
fn first_strong(text: &str) -> Option<bool> {
    text.chars().find_map(|c| {
        if is_strong_rtl(c) {
            Some(true)
        } else if c == '\u{200E}' || (c.is_alphabetic() && !is_combining(c)) {
            Some(false)
        } else {
            None
        }
    })
}

fn is_strong_rtl(c: char) -> bool {
    c == '\u{200F}'
        || (matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
            && (c.is_alphabetic()
                || matches!(
                    c as u32,
                    0x05BE | 0x05C0 | 0x05C3 | 0x05C6 | 0x061B..=0x061F
                ))
            && !is_combining(c)
            && !matches!(c as u32, 0x0660..=0x0669 | 0x06F0..=0x06F9))
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7 | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8 | 0x06EA..=0x06ED)
}

#[cfg(test)]
mod tests {
    use super::first_strong;

    #[test]
    fn first_strong_character() {
        assert_eq!(first_strong("123 abc"), Some(false));
        assert_eq!(first_strong("  \u{5E9}\u{5DC}"), Some(true));
        assert_eq!(first_strong("\u{627}\u{644}"), Some(true));
        assert_eq!(first_strong("12 ,."), None);
    }
}
