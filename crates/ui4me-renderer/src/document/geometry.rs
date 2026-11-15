//! The content→window geometry service (F-07 / INTERACT-02, INTERACT-03; audit roadmap 3.2).
//!
//! One mapping from layout's document space (`FragmentTree::document_rect`) to viewport CSS px,
//! built by the same rules the scene uses for painting and hit testing:
//!
//! 1. A *shift* (translation): minus the viewport scroll offset, reset by the nearest
//!    `position: fixed` ancestor-or-self box (to its anchor shift when it is anchored), plus the
//!    shift of every sticky box on the way, minus the offset of every scroll container *around*
//!    the box (a scroll container's own border box does not move with its own offset). This is the
//!    scene adapter's `shift` (`scene::appendix_e::Adapter`), quantized the same way.
//! 2. The CSS 2D *transform* chain the box is painted and hit-tested with, which the scene records
//!    per element while it builds the hit index (`SceneSnapshot::element_transform`).
//!
//! `viewport = transform ∘ translate(shift)`. Every consumer that turns layout coordinates into
//! window coordinates, or a window point into layout coordinates, goes through here: element
//! bounds (`get_rect`, accessibility bounds, the AT click point), text control geometry (caret
//! hit and placement, `<select>` option picking, the IME cursor area), content boxes (range
//! slider), accessibility text-run bounds and scroll-into-view. `scroll_chain` stays the answer
//! to "what scrolls" (wheel, keys, reveal), not to "where is it".

use super::Document;
use crate::layout::box_tree::PositionValue;
use crate::layout::coordinator::overflow::{ScrollOffsets, ScrollTarget};
use crate::layout::fragment::{FragmentId, FragmentKind, FragmentTree};
use crate::scene::appendix_e::SceneSnapshot;
use ui4me_engine_types::geometry::layout_unit::LayoutUnit;
use ui4me_engine_types::geometry::transform::{Affine2D, PointF};
use ui4me_engine_types::ids::NodeId;

/// Snap like the scene adapter does (`LayoutUnit::quantize`).
fn q(v: f64) -> f64 {
    LayoutUnit::quantize(v).value.to_f64()
}

/// The paint/hit-test shift of fragment `f`: the translation from document space to the
/// (pre-transform) viewport space its own box is drawn in.
pub(crate) fn fragment_shift(
    tree: &FragmentTree,
    scroll: &ScrollOffsets,
    f: FragmentId,
) -> (f64, f64) {
    // Root first, like the adapter's traversal.
    let mut path = Vec::new();
    let mut at = Some(f);
    while let Some(a) = at {
        path.push(a);
        at = tree.get(a).parent;
    }
    let (vx, vy) = scroll.offset(ScrollTarget::Viewport);
    let (mut sx, mut sy) = (-q(vx), -q(vy));
    for &a in path.iter().rev() {
        let FragmentKind::Box(b) = &tree.get(a).kind else {
            continue;
        };
        if b.extras.position == PositionValue::Fixed {
            (sx, sy) = (0.0, 0.0);
            if b.extras.anchor_node.is_some() {
                let (dx, dy) =
                    crate::layout::coordinator::sticky::anchor_scroll_shift(tree, scroll, a);
                (sx, sy) = (q(dx), q(dy));
            }
        }
        if b.extras.sticky.is_some()
            && let Ok((dx, dy)) = crate::layout::coordinator::sticky::sticky_shift(tree, scroll, a)
        {
            sx += q(dx);
            sy += q(dy);
        }
        if a != f
            && b.extras.is_scroll_container
            && let Some(n) = b.owner_node()
        {
            let (ox, oy) = scroll.offset(ScrollTarget::Node(n));
            sx -= q(ox);
            sy -= q(oy);
        }
    }
    (sx, sy)
}

/// The element whose transform a fragment is painted with: its own element box's owner, or the
/// nearest element box around it (text, anonymous and pseudo boxes).
fn transform_owner(tree: &FragmentTree, f: FragmentId) -> Option<NodeId> {
    let mut at = Some(f);
    while let Some(a) = at {
        if let FragmentKind::Box(b) = &tree.get(a).kind
            && let Some(n) = b.owner_node()
        {
            return Some(n);
        }
        at = tree.get(a).parent;
    }
    None
}

/// The document-space → viewport mapping of fragment `f`.
pub(crate) fn fragment_to_viewport(
    tree: &FragmentTree,
    scroll: &ScrollOffsets,
    scene: &SceneSnapshot,
    f: FragmentId,
) -> Affine2D {
    let (sx, sy) = fragment_shift(tree, scroll, f);
    let shift = Affine2D::translate(sx, sy);
    match transform_owner(tree, f).and_then(|n| scene.element_transform(n)) {
        Some(t) => t.then_after(&shift),
        None => shift,
    }
}

/// The axis-aligned bounding box `(x0, y0, x1, y1)` of a rectangle mapped by `t`.
pub(crate) fn map_bounds(t: &Affine2D, x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64, f64, f64) {
    let pts = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| t.map_point(PointF { x, y }));
    let (mut a, mut b, mut c, mut d) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in pts {
        a = a.min(p.x);
        b = b.min(p.y);
        c = c.max(p.x);
        d = d.max(p.y);
    }
    (a, b, c, d)
}

/// The inverse of `t`, or the identity for a singular one (whose content is neither painted nor hit).
pub(crate) fn inverse(t: &Affine2D) -> Affine2D {
    ui4me_engine_types::geometry::transform::invert(t).unwrap_or(Affine2D::IDENTITY)
}

impl Document {
    /// The principal box fragment of `n` (its first box; a text node has none).
    pub(crate) fn principal_box(&self, n: NodeId) -> Option<FragmentId> {
        let c = self.pipeline.committed.as_deref()?;
        c.layout.fragments.box_fragment(n).map(|(f, _)| f.id)
    }

    /// The document-space → viewport mapping of `n`'s principal box (scroll, sticky, fixed and
    /// transforms, as painted and hit-tested). `None` when `n` has no box.
    pub(crate) fn node_to_viewport(&self, n: NodeId) -> Option<Affine2D> {
        let c = self.pipeline.committed.as_deref()?;
        let f = self.principal_box(n)?;
        Some(fragment_to_viewport(
            &c.layout.fragments,
            self.pipeline.scroll_offsets(),
            &c.scene,
            f,
        ))
    }

    /// The mapping from a box's local space (its border-box top-left at `(0, 0)` plus `local`,
    /// for example a text control's content-box origin) to the viewport.
    pub(crate) fn box_local_to_viewport(&self, n: NodeId, local: (f64, f64)) -> Option<Affine2D> {
        let c = self.pipeline.committed.as_deref()?;
        let f = self.principal_box(n)?;
        let origin = c.layout.fragments.document_rect(f).rect.origin;
        let m = self.node_to_viewport(n)?;
        Some(m.then_after(&Affine2D::translate(
            origin.x.to_f64() + local.0,
            origin.y.to_f64() + local.1,
        )))
    }

    /// An element's content box: the mapping from its own space (its top-left at `(0, 0)`) to the
    /// viewport, and its size. Flushes first; `None` when it has no box.
    pub(crate) fn content_box_space(
        &mut self,
        _id: ui4me_engine_types::ids::DocumentId,
        n: NodeId,
    ) -> Result<Option<(Affine2D, f64, f64)>, ui4me_engine_types::status::Status> {
        let read = self.pipeline.flush()?;
        let Some((_, bf)) = read.committed.layout.fragments.box_fragment(n) else {
            return Ok(None);
        };
        let c = bf.content_rect;
        let local = (c.origin.x.to_f64(), c.origin.y.to_f64());
        let (w, h) = (c.size.width.to_f64(), c.size.height.to_f64());
        Ok(self.box_local_to_viewport(n, local).map(|m| (m, w, h)))
    }
}
