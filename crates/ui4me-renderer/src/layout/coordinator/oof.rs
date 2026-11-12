//! owner: 008-layout-engine (US6: T075)
//!
//! Out-of-flow layout (FR-025, FR-055–FR-061; research R-07). The geometry is Taffy's: its block,
//! flex and grid algorithms record every `absolute`/`fixed` child as an out-of-flow candidate with
//! the static position computed by that algorithm; the positioning pass
//! (`taffy::compute_oof_layout`, run by the host tree inside the cache closure) lays a candidate
//! out in the nearest box that claims it. What this crate owns, and what lives in the other files:
//!
//! * **Containing-block claims** (`style_view::containing_block_claims`, `host_style::HostStyle`):
//!   a positioned box (sticky included, FR-061) claims `absolute` descendants; a box with a
//!   `transform`, `perspective`, `filter` or matching `will-change` claims `absolute` and `fixed`
//!   ones (FR-056, FR-059). The root claims whatever is left: the initial containing block.
//! * **Box generation**: an out-of-flow box neither splits an inline box nor ends an inline run and
//!   is no flex or grid item (`box_tree::anonymous`, `box_tree::build`).
//! * **Fragments**: an out-of-flow box is a child of its containing block's fragment, after the
//!   in-flow children, with `containing_block` and `dom_parent` set (`fragment::builder`).
//! * **Static positions inside inline content**: an out-of-flow box inside inline content is an
//!   `OutOfFlow` item of the paragraph; the IFC supplies its static position (`record_static_position`,
//!   `FinalExtras::oof_static`): the point in its line, beside any floats, for a box that was
//!   inline-level before blockification (`BoxStyle::original_inline`), the start edge below the content
//!   before it for a block-level one (`host_tree::layout_inline_root`, `floats.rs`). A paragraph with
//!   no line content (only floats and such boxes) makes no line box.
//!
//! An *inline* containing block (FR-057, T076, CSS 2.1 section 10.1 item 4) is not a Taffy node and does
//! not claim: Taffy hoists the box to the next claiming ancestor, and once the fragments exist
//! `host_tree::resolve_inline_containing_blocks` lays it out again against the bounding box of the padding
//! boxes of the inline box's first and last fragments (`taffy::compute_oof_layout_for_area`).

use ui4me_engine_types::diag::{FeatureId, ReportKey, ReportLocation};
use ui4me_engine_types::stage::Stage;

use super::LayoutReport;
use super::host_tree::push_report;
use crate::layout::box_tree::{BoxId, BoxKind, BoxOwner, BoxTree};

/// `css.value.position-inline-containing-block`: the containing block is an inline box (T076).
pub const INLINE_CONTAINING_BLOCK: &str = "css.value.position-inline-containing-block";

fn feature(id: &'static str) -> FeatureId {
    FeatureId::new(id).expect("css.value root is registered")
}

/// The inline box that is the containing block of the `absolute` out-of-flow box `id` (CSS 2.1 section 10.1
/// item 4): its nearest ancestor that would claim it, when that ancestor is an inline box. Such boxes are not
/// Taffy nodes, so Taffy hoists the out-of-flow box to the next claiming ancestor; `host_tree::
/// resolve_inline_containing_blocks` then positions it against the inline box's fragments.
pub(crate) fn inline_containing_block(boxes: &BoxTree, id: BoxId) -> Option<BoxId> {
    let b = boxes.get(id);
    if !b.is_out_of_flow() || b.style.position != crate::layout::box_tree::PositionValue::Absolute {
        return None;
    }
    let mut cur = b.parent;
    while let Some(p) = cur {
        let a = boxes.get(p);
        if a.style.position.is_positioned()
            || a.style.claims_fixed_descendants
            || a.effective_containment().contains_layout_or_paint()
        {
            return matches!(a.kind, BoxKind::InlineBox).then_some(p);
        }
        cur = a.parent;
    }
    None
}

/// FR-022-style honesty for what is not computed: an inline containing block none of whose fragments exists
/// (the inline box generated no line content), so the box falls through to the next claiming ancestor.
pub(crate) fn report_unresolved_inline_containing_block(
    boxes: &BoxTree,
    node: BoxId,
    reports: &mut Vec<LayoutReport>,
) {
    report(
        boxes,
        boxes.get(node).owner,
        INLINE_CONTAINING_BLOCK,
        reports,
    );
}

/// The anchor element of the `anchor-name` `name` for the anchored box `me`: the last box in tree order that
/// carries the name (F0808).
pub(crate) fn anchor_by_name(
    boxes: &BoxTree,
    name: &str,
    me: BoxId,
) -> Option<ui4me_engine_types::ids::NodeId> {
    boxes
        .boxes
        .iter()
        .enumerate()
        .filter(|(i, b)| {
            *i != me.0 as usize
                && b.style
                    .anchor
                    .as_ref()
                    .is_some_and(|a| a.props.names.iter().any(|n| n == name))
        })
        .filter_map(|(_, b)| match b.owner {
            BoxOwner::Element(n) => Some(n),
            _ => None,
        })
        .next_back()
}

/// The anchor of one `anchor()` inset of box `id`: the function's own name, else `position-anchor`, else the
/// implicit anchor of a popover.
pub(crate) fn anchor_of_inset(
    boxes: &BoxTree,
    id: BoxId,
    ins: &crate::style::view::groups::AnchorInset,
) -> Option<ui4me_engine_types::ids::NodeId> {
    let a = boxes.get(id).style.anchor.as_ref()?;
    match ins.name.as_ref().or(a.props.position_anchor.as_ref()) {
        Some(name) => anchor_by_name(boxes, name, id),
        None => a.implicit,
    }
}

/// The first resolved anchor of box `id` (what a fixed box follows when its scroll container scrolls).
pub(crate) fn first_anchor(boxes: &BoxTree, id: BoxId) -> Option<ui4me_engine_types::ids::NodeId> {
    let a = boxes.get(id).style.anchor.as_ref()?;
    a.props
        .insets
        .iter()
        .flatten()
        .find_map(|ins| anchor_of_inset(boxes, id, ins))
}

fn report(boxes: &BoxTree, owner: BoxOwner, id: &'static str, reports: &mut Vec<LayoutReport>) {
    if let BoxOwner::Element(node) = owner {
        let key = ReportKey {
            feature: feature(id),
            stage: Stage::Layout,
            document: boxes.document,
            location: ReportLocation::Element { node },
        };
        push_report(reports, key);
    }
}
