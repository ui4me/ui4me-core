//! owner: 008-layout-engine (US6: T078)
//!
//! Sticky positioning (FR-060, FR-061; research R-08). A sticky box is laid out at its normal-flow
//! position and publishes `StickyConstraints` on its fragment (the nearest scroll container or the
//! viewport, the containing block's content box, the resolved insets and its normal-flow rect;
//! built in `fragment::builder`). Layout does **no** sticky arithmetic: the offset is the pure
//! `sticky_offset` of 010 (`ui4me-core::geometry::spatial`), the same function the scene and the
//! composed geometry query use. [`sticky_shift`] only feeds it with the published constraints and
//! the scroll offsets of a [`ScrollOffsets`], so a caller that holds both (012, the geometry
//! query) gets the shift without re-deriving the sticky view rectangle.

use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::geometry::LayoutRect;
use ui4me_engine_types::geometry::spatial::{
    StickyInsets as CoreInsets, StickyValues, sticky_offset,
};
use ui4me_engine_types::geometry::transform::RectF;

use super::overflow::{ScrollOffsets, ScrollTarget};
use crate::layout::fragment::{
    FragmentId, FragmentKind, FragmentTree, StickyConstraints, StickyScrollContainer,
};

fn rect_f(r: LayoutRect) -> RectF {
    RectF::new(
        r.origin.x.to_f64(),
        r.origin.y.to_f64(),
        r.size.width.to_f64(),
        r.size.height.to_f64(),
    )
}

/// The sticky constraints of a fragment, if it is a sticky box.
pub fn constraints(tree: &FragmentTree, fragment: FragmentId) -> Option<&StickyConstraints> {
    match &tree.get(fragment).kind {
        FragmentKind::Box(b) => b.extras.sticky.as_ref(),
        _ => None,
    }
}

/// The sticky view rectangle (CSS Position 3): the scrollport of the sticky box's scroll
/// container in document space, moved by that container's scroll offset.
pub fn sticky_view(
    tree: &FragmentTree,
    offsets: &ScrollOffsets,
    c: &StickyConstraints,
) -> Option<RectF> {
    let (target, scrollport_doc) = match c.scroll_container {
        StickyScrollContainer::Viewport => {
            (ScrollTarget::Viewport, tree.viewport_scroll.scrollport)
        }
        StickyScrollContainer::Fragment(f) => {
            let FragmentKind::Box(b) = &tree.get(f).kind else {
                return None;
            };
            let sp = b.extras.scrollport?;
            let origin = tree.document_rect(f).rect.origin;
            let node = b.owner_node()?;
            (
                ScrollTarget::Node(node),
                LayoutRect {
                    origin: ui4me_engine_types::geometry::LayoutPoint {
                        x: origin.x + sp.origin.x,
                        y: origin.y + sp.origin.y,
                    },
                    size: sp.size,
                },
            )
        }
    };
    let (ox, oy) = offsets.offset(target);
    let v = rect_f(scrollport_doc);
    Some(RectF::new(v.x + ox, v.y + oy, v.width, v.height))
}

/// Anchor scroll compensation (F0808): a `fixed` box positioned with `anchor()` follows its anchor when the
/// containers around the anchor scroll. Layout placed it with every offset at zero, so it is drawn and
/// hit-tested shifted by minus the viewport and scroll-container offsets that move the anchor.
pub fn anchor_scroll_shift(
    tree: &FragmentTree,
    offsets: &ScrollOffsets,
    fragment: FragmentId,
) -> (f64, f64) {
    let FragmentKind::Box(b) = &tree.get(fragment).kind else {
        return (0.0, 0.0);
    };
    let Some(anchor) = b.extras.anchor_node else {
        return (0.0, 0.0);
    };
    let Some(&af) = tree.fragments_of(anchor).first() else {
        return (0.0, 0.0);
    };
    let (mut sx, mut sy) = offsets.offset(ScrollTarget::Viewport);
    let mut at = tree.get(af).parent;
    while let Some(a) = at {
        if let FragmentKind::Box(ab) = &tree.get(a).kind
            && ab.extras.is_scroll_container
            && let Some(node) = ab.owner_node()
        {
            let (ox, oy) = offsets.offset(ScrollTarget::Node(node));
            sx += ox;
            sy += oy;
        }
        at = tree.get(a).parent;
    }
    (-sx, -sy)
}

/// The shift (dx, dy) a sticky fragment is drawn and hit-tested at, for the scroll offsets in
/// `offsets`. `(0, 0)` for a fragment that is not sticky.
pub fn sticky_shift(
    tree: &FragmentTree,
    offsets: &ScrollOffsets,
    fragment: FragmentId,
) -> Result<(f64, f64), Code> {
    let Some(c) = constraints(tree, fragment) else {
        return Ok((0.0, 0.0));
    };
    let view = sticky_view(tree, offsets, c).ok_or(Code::InternalInvariant)?;
    let ins = &c.insets;
    let px =
        |v: Option<ui4me_engine_types::geometry::layout_unit::LayoutUnit>| v.map(|u| u.to_f64());
    let values = StickyValues {
        containing_block_rect: rect_f(c.containing_block_rect),
        insets: CoreInsets {
            top: px(ins.top),
            right: px(ins.right),
            bottom: px(ins.bottom),
            left: px(ins.left),
        },
        normal_flow_rect: rect_f(c.normal_flow_rect),
    };
    sticky_offset(&values, &view)
}
