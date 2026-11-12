//! owner: 008-layout-engine (US6: T077)
//!
//! Overflow and scroll geometry (FR-062–FR-066; research R-09).
//!
//! **Published by layout** (in `fragment::builder`, from Taffy's `ScrollableOverflow`): for every
//! box fragment the `overflow-x`/`-y` modes, `is_scroll_container`, the scrollport (the padding
//! box), the scrollable overflow rectangle (in-flow and out-of-flow border boxes plus the end
//! padding of a scroll container, never ink overflow) and the `clip` rect; for the viewport
//! (`FragmentTree::viewport_scroll`) the same after `overflow` propagation from the root or
//! `body` (FR-065). Scrollbars are overlay and take no space (FR-063): `scrollbar-gutter` and
//! `scrollbar-width` are outside the profile.
//!
//! **This module** is the layout-side API that interaction (012) calls to move a scroll
//! container: [`ScrollOffsets`] holds the offsets, [`ScrollOffsets::scroll_to`] and
//! [`ScrollOffsets::scroll_by`] clamp them against the published scrollable overflow
//! (FR-066) and [`ScrollOffsets::reclamp`] re-clamps after a relayout. It is *not* an input of
//! layout: no layout pass reads it, a change of offset never produces a layout epoch, and the
//! value is owned by whoever holds it (012's `ScrollState` holds one; decision D-LPOSr2-02). The
//! sticky offset is the pure function of 010 applied to this state (`sticky.rs`).
//!
//! Scroll range of a container, in CSS px relative to the initial scroll position (the origin of
//! the scroll direction): `[min(0, so.start - sp.start), max(0, so.end - sp.end)]` per axis for
//! the scrollport `sp` and the scrollable overflow `so`, so a left-to-right container scrolls
//! within `[0, max]` and a right-to-left one within `[-max, 0]` (CSSOM View §6). Overflow before
//! the scroll origin is unreachable and layout already excludes it from `so`.

use std::collections::BTreeMap;

use ui4me_engine_types::geometry::layout_unit::LayoutUnit;
use ui4me_engine_types::geometry::{LayoutPoint, LayoutRect};
use ui4me_engine_types::ids::NodeId;

use crate::layout::fragment::{FragmentId, FragmentKind, FragmentTree};

/// What can scroll: the viewport or an element that is a scroll container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScrollTarget {
    Viewport,
    Node(NodeId),
}

/// The scrollport and scrollable overflow of a target, in one frame (the target's border box for
/// an element, the initial containing block for the viewport).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollGeometry {
    pub scrollport: LayoutRect,
    pub scrollable_overflow: LayoutRect,
    /// Document-space origin of the frame.
    pub frame_origin: LayoutPoint,
}

/// The reachable scroll offsets of a container (CSS px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollRange {
    pub min: (f64, f64),
    pub max: (f64, f64),
}

impl ScrollRange {
    pub fn clamp(&self, x: f64, y: f64) -> (f64, f64) {
        (
            x.clamp(self.min.0, self.max.0),
            y.clamp(self.min.1, self.max.1),
        )
    }
}

/// Why a scroll request was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollError {
    /// The element has no box fragment in this tree, or is not a scroll container.
    NotScrollContainer,
    /// An offset was NaN or infinite (`GEOMETRY_NON_FINITE`).
    NonFinite,
}

/// The outcome of a scroll request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollApplied {
    /// The offset now in force.
    pub offset: (f64, f64),
    /// The request was outside the reachable range and was clamped.
    pub clamped: bool,
    /// The stored offset changed.
    pub changed: bool,
}

/// The fragment of a scroll-container element.
pub fn scroll_container_fragment(tree: &FragmentTree, node: NodeId) -> Option<FragmentId> {
    tree.fragments_of(node)
        .iter()
        .copied()
        .find(|f| match &tree.get(*f).kind {
            FragmentKind::Box(b) => b.extras.is_scroll_container,
            _ => false,
        })
}

/// Scrollport and scrollable overflow of `target`; `None` for an element that is not a scroll
/// container in `tree`.
pub fn scroll_geometry(tree: &FragmentTree, target: ScrollTarget) -> Option<ScrollGeometry> {
    match target {
        ScrollTarget::Viewport => {
            let v = &tree.viewport_scroll;
            Some(ScrollGeometry {
                scrollport: v.scrollport,
                scrollable_overflow: v.scrollable_overflow,
                frame_origin: LayoutPoint::default(),
            })
        }
        ScrollTarget::Node(n) => {
            let f = scroll_container_fragment(tree, n)?;
            let FragmentKind::Box(b) = &tree.get(f).kind else {
                return None;
            };
            Some(ScrollGeometry {
                scrollport: b.extras.scrollport?,
                scrollable_overflow: b.extras.scrollable_overflow?,
                frame_origin: tree.document_rect(f).rect.origin,
            })
        }
    }
}

fn axis_range(
    sp_start: LayoutUnit,
    sp_end: LayoutUnit,
    so_start: LayoutUnit,
    so_end: LayoutUnit,
) -> (f64, f64) {
    let min = (so_start - sp_start).to_f64().min(0.0);
    let max = (so_end - sp_end).to_f64().max(0.0);
    // Sub-pixel overflow is a layout rounding artifact (e.g. 1/64 px from fractional line
    // heights), not scrollable content: browsers round scroll dimensions, so it neither scrolls
    // nor shows a scrollbar.
    let snap = |v: f64| if v.abs() < 1.0 { 0.0 } else { v };
    (snap(min), snap(max))
}

/// The reachable offsets of `target` in `tree`.
pub fn scroll_range(tree: &FragmentTree, target: ScrollTarget) -> Option<ScrollRange> {
    let g = scroll_geometry(tree, target)?;
    let (sp, so) = (g.scrollport, g.scrollable_overflow);
    let (min_x, max_x) = axis_range(
        sp.origin.x,
        sp.origin.x + sp.size.width,
        so.origin.x,
        so.origin.x + so.size.width,
    );
    let (min_y, max_y) = axis_range(
        sp.origin.y,
        sp.origin.y + sp.size.height,
        so.origin.y,
        so.origin.y + so.size.height,
    );
    Some(ScrollRange {
        min: (min_x, min_y),
        max: (max_x, max_y),
    })
}

/// Scroll offsets by target. Pure data: layout never reads it (FR-066).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollOffsets {
    offsets: BTreeMap<ScrollTarget, (f64, f64)>,
    /// Non-zero offsets of connected elements temporarily without a scroll box.
    dormant: BTreeMap<NodeId, (f64, f64)>,
}

impl ScrollOffsets {
    pub fn new() -> Self {
        ScrollOffsets::default()
    }

    /// The stored offset (`(0, 0)` when the target was never scrolled).
    pub fn offset(&self, target: ScrollTarget) -> (f64, f64) {
        self.offsets.get(&target).copied().unwrap_or((0.0, 0.0))
    }

    /// Targets with a non-zero offset, in a stable order.
    pub fn scrolled(&self) -> impl Iterator<Item = (ScrollTarget, (f64, f64))> + '_ {
        self.offsets
            .iter()
            .filter(|(_, o)| **o != (0.0, 0.0))
            .map(|(t, o)| (*t, *o))
    }

    /// Connected nodes with an active or dormant non-zero offset.
    pub fn tracked_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.offsets
            .keys()
            .filter_map(|t| match t {
                ScrollTarget::Node(n) => Some(*n),
                ScrollTarget::Viewport => None,
            })
            .chain(self.dormant.keys().copied())
    }

    /// Forget all scroll state for a removed node.
    pub fn forget_node(&mut self, node: NodeId) {
        self.offsets.remove(&ScrollTarget::Node(node));
        self.dormant.remove(&node);
    }

    /// Set the offset of `target` (CSSOM View `scrollTo`), clamped to the reachable range of
    /// `tree`'s published scrollable overflow and rounded to 1/64 px. A `hidden` container can be
    /// scrolled programmatically (CSS Overflow 3 §3.1); a non-scroll-container is refused.
    pub fn scroll_to(
        &mut self,
        tree: &FragmentTree,
        target: ScrollTarget,
        x: f64,
        y: f64,
    ) -> Result<ScrollApplied, ScrollError> {
        if !(x.is_finite() && y.is_finite()) {
            return Err(ScrollError::NonFinite);
        }
        let range = scroll_range(tree, target).ok_or(ScrollError::NotScrollContainer)?;
        let (cx, cy) = range.clamp(x, y);
        let snap = |v: f64| LayoutUnit::quantize(v).value.to_f64();
        let offset = (snap(cx), snap(cy));
        let previous = self.offset(target);
        // A successful write supersedes any dormant value, including zero.
        if let ScrollTarget::Node(n) = target {
            self.dormant.remove(&n);
        }
        if offset == (0.0, 0.0) {
            self.offsets.remove(&target);
        } else {
            self.offsets.insert(target, offset);
        }
        Ok(ScrollApplied {
            offset,
            clamped: (cx, cy) != (x, y),
            changed: offset != previous,
        })
    }

    /// Move the offset of `target` by `(dx, dy)` (CSSOM View `scrollBy`).
    pub fn scroll_by(
        &mut self,
        tree: &FragmentTree,
        target: ScrollTarget,
        dx: f64,
        dy: f64,
    ) -> Result<ScrollApplied, ScrollError> {
        let (x, y) = self.offset(target);
        self.scroll_to(tree, target, x + dx, y + dy)
    }

    /// After a new layout epoch: clamp offsets and forget missing containers.
    pub fn reclamp(&mut self, tree: &FragmentTree) -> Vec<(ScrollTarget, (f64, f64))> {
        self.reclamp_connected(tree, |_| false)
    }

    /// Preserve a connected element's offset while it has no scroll container,
    /// and restore it if a scroll container returns. The caller must forget
    /// removed nodes synchronously, including descendants of removed subtrees.
    pub fn reclamp_connected(
        &mut self,
        tree: &FragmentTree,
        connected: impl Fn(NodeId) -> bool,
    ) -> Vec<(ScrollTarget, (f64, f64))> {
        let mut changed = Vec::new();
        let targets: Vec<ScrollTarget> = self.offsets.keys().copied().collect();
        for t in targets {
            let (x, y) = self.offset(t);
            match scroll_range(tree, t) {
                None => {
                    self.offsets.remove(&t);
                    if let ScrollTarget::Node(n) = t
                        && connected(n)
                    {
                        self.dormant.insert(n, (x, y));
                    }
                    changed.push((t, (0.0, 0.0)));
                }
                Some(r) => {
                    let c = r.clamp(x, y);
                    if c != (x, y) {
                        if c == (0.0, 0.0) {
                            self.offsets.remove(&t);
                        } else {
                            self.offsets.insert(t, c);
                        }
                        changed.push((t, c));
                    }
                }
            }
        }
        for n in self.dormant.keys().copied().collect::<Vec<_>>() {
            let t = ScrollTarget::Node(n);
            if !connected(n) {
                self.dormant.remove(&n);
                continue;
            }
            if let Some(r) = scroll_range(tree, t) {
                let c = r.clamp(self.dormant[&n].0, self.dormant[&n].1);
                self.dormant.remove(&n);
                if c != (0.0, 0.0) {
                    self.offsets.insert(t, c);
                    changed.push((t, c));
                }
            }
        }
        changed
    }
}
