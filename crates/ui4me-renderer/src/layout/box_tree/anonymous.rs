//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! Anonymous block boxes (CSS 2.1 §9.2.1.1; FR-006, FR-008): in a block container with both
//! block-level and inline-level children, each maximal run of inline-level children is wrapped
//! in an anonymous block box inheriting from the container. In a flex or grid container every
//! such run becomes an anonymous item, with or without block-level siblings. A run that contains
//! only collapsible white space (`white-space: normal`, CSS Text 3 §4.1.1) is removed: it would
//! produce no line boxes. Block-in-inline splitting (FR-007) lives with the display dispatch in
//! `build.rs` (`split_inline`), which hands the blocks to this wrapping step.

use ui4me_engine_types::ids::NodeId;

use super::BoxStyle;
use super::build::Proto;

/// Collapsible white space under `white-space: normal`: space, tab, segment breaks (LF, CR,
/// FF per CSS Text 3 document white space).
pub fn is_collapsible_whitespace(text: &str) -> bool {
    text.chars()
        .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000C}'))
}

/// Whether an inline-level proto box contributes nothing but collapsible white space.
pub(crate) fn only_collapsible(p: &Proto) -> bool {
    match p {
        // Non-normal `white-space` is reported by the inline behaviour and treated as `normal`.
        // A preserved segment break (`<br>`, `white-space: pre*`) makes a line of its own.
        Proto::Text { text, style, .. } => {
            is_collapsible_whitespace(text)
                && !(style.white_space.preserves_newlines() && text.contains('\n'))
        }
        // An inline element is content of its own (it may carry inherited properties only, but
        // an empty `<span></span>` also yields no line box; CSS 2.1 §9.4.2).
        Proto::Inline { children, .. } => children.iter().all(only_collapsible),
        // An atomic inline (inline-block, replaced, control) is content even when empty.
        Proto::Block { .. } | Proto::Replaced { .. } | Proto::Reused { .. } => false,
    }
}

/// Whether `items` (a run of inline-level children, with floats and out-of-flow boxes) make an
/// inline formatting context of their own: they hold line content, or a float next to an
/// out-of-flow box whose static position is in a line (CSS 2.1 §9.4.2, §10.3.7: that position is
/// where the box would have been as an inline-level box, beside the float).
fn needs_line_context(items: &[Proto]) -> bool {
    items
        .iter()
        .any(|p| !p.is_out_of_flow() && !p.is_float() && !only_collapsible(p))
        || (items.iter().any(Proto::is_float)
            && items
                .iter()
                .any(|p| p.is_out_of_flow() && p.style().original_inline))
}

/// Wrap the maximal inline-level runs of `children` in anonymous blocks. With `always` (flex and
/// grid containers) wrapping happens even when no child is block-level and floats are items of the
/// container. Elsewhere a float joins the inline run around it (it is placed by the line holding it,
/// CSS 2.1 §9.5); a run that holds only floats, out-of-flow boxes and collapsible white space
/// generates no line box, and its floats and out-of-flow boxes become children of the container.
pub(crate) fn wrap_inline_runs(
    container: NodeId,
    style: &BoxStyle,
    children: Vec<Proto>,
    always: bool,
) -> Vec<Proto> {
    let floats_join = !always;
    let blocky = |c: &Proto| c.is_block_level() || (always && c.is_float());
    let has_float = floats_join && children.iter().any(Proto::is_float);
    if !always && !has_float && !children.iter().any(Proto::is_block_level) {
        return children;
    }
    if !always && !children.iter().any(Proto::is_block_level) {
        // Floats among inline content, no block-level box: the container is the inline root, unless
        // there is no line content (then the floats and out-of-flow boxes are its children).
        if needs_line_context(&children) {
            return children;
        }
        return children
            .into_iter()
            .filter(|p| p.is_out_of_flow() || p.is_float())
            .collect();
    }
    let mut out = Vec::new();
    let mut run: Vec<Proto> = Vec::new();
    let flush = |run: &mut Vec<Proto>, out: &mut Vec<Proto>| {
        if run.is_empty() {
            return;
        }
        let items = std::mem::take(run);
        // A run of collapsible white space, floats and out-of-flow boxes only generates no line
        // box: the floats and out-of-flow boxes become children of the container itself, where
        // block layout records their static position in the flow (CSS 2.1 §9.4.2, CSS Position 3 §3).
        if !needs_line_context(&items) {
            out.extend(
                items
                    .into_iter()
                    .filter(|p| p.is_out_of_flow() || p.is_float()),
            );
            return;
        }
        // The text after a block-level sibling continues the container's text: it is not its first
        // line, so `text-indent` does not apply to it (CSS Text 3 section 9; WPT block-in-inline-first-line-002).
        let after_block = out.iter().any(|p: &Proto| p.is_block_level());
        let mut block = Proto::anonymous_block(container, style, items);
        if let Proto::Block { style: s, .. } = &mut block {
            if after_block {
                s.text_indent = super::LengthPercentage::ZERO;
            } else {
                // The first anonymous block holds the container's first formatted line (015 TXT-5).
                s.first_line = style.first_line.clone();
            }
        }
        out.push(block);
    };
    for c in children {
        if blocky(&c) {
            flush(&mut run, &mut out);
            out.push(c);
        } else if always && c.is_out_of_flow() {
            // Not a flex or grid item (FR-045, FR-053): a child of the container, positioned by
            // its alignment; it does not break the contiguous text run.
            out.push(c);
        } else {
            run.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}
