//! owner: 008-layout-engine (015 TXT-5, lane L-PSEUDO) - the `::first-letter` box.
//!
//! CSS Pseudo 4 section 3.2: the first typographic letter unit of the first formatted line of a block container
//! (`ui4me-text` `first_letter`: leading spaces and punctuation, one grapheme cluster, the punctuation after it)
//! is wrapped in a box of the pseudo-element's style. It is built the way the reference browser builds it: the
//! text node's run is split in two in the box tree, so the letter is an inline box of its own (an inline box
//! around a text run, owner `Pseudo(element, FirstLetter)`) inside the box that holds the text, or, with a
//! `float`, a floated block-level box (a drop cap) that the paragraph places like any other float. The DOM text
//! is unchanged: both halves are DOM text, no generated content.
//!
//! Where the letter is looked for (measured in the pinned Chromium, `lane-LPSEUDO-evidence.md`): the first text
//! that is not collapsible white space, descending through inline boxes (their `::before` content included) and
//! through the first in-flow block children (`<div>` `<p>` ... and list items, after the marker), never into
//! floats, out-of-flow boxes, flex, grid or table containers. An atomic inline (inline-block, image, control) or
//! a forced break reached first ends the search: that block has no first letter.

use std::sync::Arc;
use ui4me_engine_types::ids::NodeId;

use super::anonymous::is_collapsible_whitespace;
use super::build::{BoxOwner, BoxTag, Builder, Proto, propagate_decorations};
use super::{BoxStyle, DisplayValue, FloatValue, InnerDisplay, PseudoKind, StyledSource};

/// What the search found.
enum Loc {
    /// The first letter is the first `len` bytes of the text at the path.
    Found(usize),
    /// Nothing before an atomic inline, a forced break or an unsuitable text: no first letter.
    Blocked,
    /// Nothing in this subtree (only white space, floats, empty boxes): keep looking.
    None,
}

/// A text that carries line content (not collapsible white space).
fn has_text(p: &Proto) -> bool {
    match p {
        Proto::Text { text, style, .. } => {
            !(is_collapsible_whitespace(text)
                && !(style.white_space.preserves_newlines() && text.contains('\n')))
        }
        Proto::Inline { children, .. } => children.iter().any(has_text),
        Proto::Block {
            children,
            atomic: false,
            style,
            ..
        } if style.float == FloatValue::None && !style.position.is_out_of_flow() => {
            children.iter().any(has_text)
        }
        _ => false,
    }
}

fn locate(
    kids: &mut [Proto],
    path: &mut Vec<usize>,
    outer_more: bool,
    builder_expand: &dyn Fn(&Proto) -> Option<Proto>,
    tainted: &mut Vec<NodeId>,
) -> Loc {
    for i in 0..kids.len() {
        if let Some(p) = builder_expand(&kids[i]) {
            kids[i] = p;
        }
        let more = outer_more || kids[i + 1..].iter().any(has_text);
        path.push(i);
        let r = match &mut kids[i] {
            Proto::Text {
                text,
                style,
                pseudo,
                ..
            } => {
                if pseudo.is_some_and(|k| k == PseudoKind::Marker) {
                    Loc::None
                } else if is_collapsible_whitespace(text) {
                    if style.white_space.preserves_newlines() && text.contains('\n') {
                        Loc::Blocked
                    } else {
                        Loc::None
                    }
                } else {
                    match crate::text::first_letter::first_letter_len(
                        text,
                        more,
                        style.text.font.lang.to_ascii_lowercase().split('-').next() == Some("nl"),
                    ) {
                        // A preserved line break before the letter makes the first line empty: no first letter.
                        Some(n)
                            if style.white_space.preserves_newlines()
                                && text[..n].contains('\n') =>
                        {
                            Loc::Blocked
                        }
                        Some(n) => Loc::Found(n),
                        None => Loc::Blocked,
                    }
                }
            }
            Proto::Inline {
                pseudo, children, ..
            } => {
                if *pseudo == Some(PseudoKind::Marker) {
                    Loc::None
                } else {
                    locate(children, path, more, builder_expand, tainted)
                }
            }
            Proto::Block {
                tag: BoxTag::Marker(_),
                ..
            } => Loc::None,
            Proto::Replaced {
                pseudo: Some(PseudoKind::Marker),
                ..
            } => Loc::None,
            Proto::Block { atomic: true, .. } | Proto::Replaced { .. } => Loc::Blocked,
            Proto::Block { style, .. }
                if style.float != FloatValue::None || style.position.is_out_of_flow() =>
            {
                Loc::None
            }
            Proto::Block {
                children,
                inner: InnerDisplay::Flow | InnerDisplay::FlowRoot,
                tag: BoxTag::None,
                owner,
                ..
            } => {
                if let BoxOwner::Element(n) = owner {
                    // Its boxes are edited after its own build: the next incremental build must not take it over.
                    tainted.push(*n);
                }
                locate(children, path, more, builder_expand, tainted)
            }
            Proto::Block { .. } => Loc::Blocked,
            Proto::Reused { .. } => Loc::Blocked,
        };
        match r {
            Loc::None => {
                path.pop();
            }
            other => return other,
        }
    }
    Loc::None
}

/// The container list and the style of the box that holds the text at `path` (all but its last index).
fn descend<'a>(
    kids: &'a mut Vec<Proto>,
    path: &[usize],
    parent: &'a BoxStyle,
) -> (&'a mut Vec<Proto>, BoxStyle) {
    let mut list = kids;
    let mut style = parent.clone();
    for &i in &path[..path.len() - 1] {
        let p = &mut list[i];
        let (next, st) = match p {
            Proto::Inline {
                children, style, ..
            }
            | Proto::Block {
                children, style, ..
            } => (children, style.clone()),
            _ => unreachable!("the path goes through containers only"),
        };
        style = st;
        list = next;
    }
    (list, style)
}

impl<S: StyledSource> Builder<'_, S> {
    /// Wrap the first letter of the block container `node` (style `style`, children `kids`) in its
    /// `::first-letter` box.
    pub(crate) fn apply_first_letter(
        &mut self,
        style: &BoxStyle,
        fl: &Arc<BoxStyle>,
        kids: &mut Vec<Proto>,
    ) {
        let mut path = Vec::new();
        let mut tainted = Vec::new();
        let expand = |p: &Proto| -> Option<Proto> {
            if let Proto::Reused { root, .. } = p {
                self.reuse
                    .as_ref()
                    .map(|c| super::incremental::expand(c.old, *root))
            } else {
                None
            }
        };
        let found = locate(kids, &mut path, false, &expand, &mut tainted);
        self.tainted.extend(tainted);
        let Loc::Found(len) = found else { return };
        let (list, parent) = descend(kids, &path, style);
        let at = *path.last().expect("a path to the text");
        let Proto::Text {
            text,
            style: tstyle,
            owner,
            pseudo,
        } = list.remove(at)
        else {
            unreachable!("the path ends at a text")
        };
        let (letter, rest) = text.split_at(len);
        let mut f = (**fl).clone();
        let floated = f.float != FloatValue::None;
        // The letter inherits the text's style where the pseudo-element does not set the property: the cascade
        // already did that from the block (or its first-line style); the writing mode and bidi context are the box's.
        f.writing_mode = tstyle.writing_mode;
        f.text_orientation = tstyle.text_orientation;
        f.direction = tstyle.direction;
        let (display, decorations) = if floated {
            (
                DisplayValue::Block,
                f.own_decoration.iter().cloned().collect::<Vec<_>>().into(),
            )
        } else {
            (DisplayValue::Inline, propagate_decorations(&parent, &f))
        };
        f.display = display;
        f.decorations = decorations;
        // A letter in the first line of a block with `::first-line` sits on that line.
        f.first_line = None;
        let letter_text = Proto::Text {
            text: letter.to_string(),
            style: f.clone(),
            owner,
            pseudo: Some(PseudoKind::FirstLetter),
        };
        let boxed = if floated {
            f.position = super::PositionValue::Static;
            Proto::Block {
                owner: BoxOwner::Pseudo(owner, PseudoKind::FirstLetter),
                style: f,
                children: vec![letter_text],
                inner: InnerDisplay::FlowRoot,
                list_item: false,
                tag: BoxTag::None,
                atomic: false,
            }
        } else {
            Proto::Inline {
                node: owner,
                pseudo: Some(PseudoKind::FirstLetter),
                style: f,
                children: vec![letter_text],
            }
        };
        list.insert(at, boxed);
        if !rest.is_empty() {
            list.insert(
                at + 1,
                Proto::Text {
                    text: rest.to_string(),
                    style: tstyle,
                    owner,
                    pseudo,
                },
            );
        }
    }
}
