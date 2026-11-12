//! owner: 015 WM-1 (lane L-WM; plan §A5)
//!
//! The logical fragment tree to physical coordinates. The fragment builder places every fragment as
//! if the inline axis were horizontal: a fragment's origin is in its parent's logical frame, its size,
//! rects and edges in its own. [`physicalize`] walks the tree once and rewrites every geometry into
//! physical CSS px (relative to the parent's border box, as before), so the geometry queries, the hit
//! index, scrolling and paint read physical values only.

use ui4me_engine_types::geometry::layout_unit::LayoutUnit;
use ui4me_engine_types::geometry::{LayoutEdges, LayoutPoint, LayoutRect, LayoutSize};

use super::{LineOrientation, WritingMode, sides_to_physical};
use crate::layout::box_tree::{BoxTree, Sides};
use crate::layout::coordinator::Viewport;
use crate::layout::fragment::{FragmentId, FragmentKind, FragmentTree, OverflowMode};

/// A logical frame: its writing mode and its logical size (inline extent `w`, block extent `h`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub wm: WritingMode,
    pub w: LayoutUnit,
    pub h: LayoutUnit,
    /// The logical origin of the frame's content box (the paragraph space of an inline root).
    pub content: LayoutPoint,
}

/// The physical rect of `r`, given in the logical frame `f`.
pub(crate) fn map_rect(f: Frame, r: LayoutRect) -> LayoutRect {
    let (x, y, w, h) = (r.origin.x, r.origin.y, r.size.width, r.size.height);
    let (px, py, pw, ph) = match f.wm {
        WritingMode::HorizontalTb => return r,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => (f.h - y - h, x, h, w),
        WritingMode::VerticalLr => (y, x, h, w),
        WritingMode::SidewaysLr => (y, f.w - x - w, h, w),
    };
    LayoutRect {
        origin: LayoutPoint { x: px, y: py },
        size: LayoutSize {
            width: pw,
            height: ph,
        },
    }
}

/// The logical rect in frame `f` of the physical rect `r` (the inverse of [`map_rect`]).
pub fn unmap_rect(
    wm: WritingMode,
    logical_w: LayoutUnit,
    logical_h: LayoutUnit,
    r: LayoutRect,
) -> LayoutRect {
    let (px, py, pw, ph) = (r.origin.x, r.origin.y, r.size.width, r.size.height);
    let (x, y, w, h) = match wm {
        WritingMode::HorizontalTb => return r,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => (py, logical_h - px - pw, ph, pw),
        WritingMode::VerticalLr => (py, px, ph, pw),
        WritingMode::SidewaysLr => (logical_w - py - ph, px, ph, pw),
    };
    LayoutRect {
        origin: LayoutPoint { x, y },
        size: LayoutSize {
            width: w,
            height: h,
        },
    }
}

fn map_edges(wm: WritingMode, e: LayoutEdges) -> LayoutEdges {
    let s = sides_to_physical(
        wm,
        Sides {
            top: e.top,
            right: e.right,
            bottom: e.bottom,
            left: e.left,
        },
    );
    LayoutEdges {
        top: s.top,
        right: s.right,
        bottom: s.bottom,
        left: s.left,
    }
}

/// Whether any box of the tree is in a vertical writing mode (otherwise the logical tree is the
/// physical one and [`physicalize`] is not run).
pub(crate) fn has_vertical(boxes: &BoxTree) -> bool {
    boxes
        .boxes
        .iter()
        .any(|b| b.style.writing_mode.is_vertical())
}

/// Add inline border boxes and vertical text bounds to enclosing scroll containers.
/// These fragments are constructed after host overflow calculation; clipping boxes stop propagation.
pub(crate) fn inline_scroll_overflow(tree: &mut FragmentTree) {
    let ids: Vec<_> = tree
        .fragments
        .iter()
        .filter_map(|f| match &f.kind {
            FragmentKind::InlineBoxPiece(_)
                if f.parent
                    .is_some_and(|p| matches!(&tree.get(p).kind, FragmentKind::Line(_))) =>
            {
                Some(f.id)
            }
            FragmentKind::Text(_)
                if f.parent.is_some_and(
                    |p| matches!(&tree.get(p).kind, FragmentKind::Line(l) if l.vertical.is_some()),
                ) =>
            {
                Some(f.id)
            }
            _ => None,
        })
        .collect();
    for id in ids {
        let z = LayoutUnit::ZERO;
        let (l, r, t, b) = match &tree.get(id).kind {
            FragmentKind::InlineBoxPiece(p) => {
                let on = tree.piece_sides(id);
                (
                    if on[3] {
                        p.padding.left + p.border_widths.left
                    } else {
                        z
                    },
                    if on[1] {
                        p.padding.right + p.border_widths.right
                    } else {
                        z
                    },
                    if on[0] {
                        p.padding.top + p.border_widths.top
                    } else {
                        z
                    },
                    if on[2] {
                        p.padding.bottom + p.border_widths.bottom
                    } else {
                        z
                    },
                )
            }
            FragmentKind::Text(_) => (z, z, z, z),
            _ => continue,
        };
        let rect = tree.document_rect(id).rect;
        let mut parent = tree.get(id).parent;
        while let Some(pid) = parent {
            let f = tree.get(pid);
            let next = f.parent;
            let origin = tree.document_rect(pid).rect.origin;
            if let FragmentKind::Box(bf) = &f.kind {
                let clips = bf.extras.overflow_x != OverflowMode::Visible
                    || bf.extras.overflow_y != OverflowMode::Visible;
                if bf.extras.is_scroll_container {
                    let port = bf.extras.scrollport.unwrap_or(bf.padding_rect);
                    let (mut left, mut top) =
                        (rect.origin.x - l - origin.x, rect.origin.y - t - origin.y);
                    let (mut right, mut bottom) = (
                        rect.origin.x + rect.size.width + r - origin.x,
                        rect.origin.y + rect.size.height + b - origin.y,
                    );
                    if bf.extras.scroll_from_end.0 {
                        right = right.min(port.origin.x + port.size.width);
                    } else {
                        left = left.max(port.origin.x);
                    }
                    if bf.extras.scroll_from_end.1 {
                        bottom = bottom.min(port.origin.y + port.size.height);
                    } else {
                        top = top.max(port.origin.y);
                    }
                    if right > left && bottom > top {
                        let old = bf.extras.scrollable_overflow.unwrap_or(port);
                        left = left.min(old.origin.x);
                        top = top.min(old.origin.y);
                        right = right.max(old.origin.x + old.size.width);
                        bottom = bottom.max(old.origin.y + old.size.height);
                        if let FragmentKind::Box(bf) = &mut tree.fragments[pid.0 as usize].kind {
                            bf.extras.scrollable_overflow = Some(LayoutRect {
                                origin: LayoutPoint { x: left, y: top },
                                size: LayoutSize::from_edges(left, right, top, bottom),
                            });
                        }
                    }
                }
                if clips {
                    break;
                }
            }
            parent = next;
        }
    }
}

struct Walk<'a> {
    boxes: &'a BoxTree,
    tree: &'a mut FragmentTree,
    /// Decoration fragments by line, mapped with their line.
    decos_by_line: std::collections::HashMap<FragmentId, Vec<usize>>,
}

/// Map the logical fragment tree of a pass to physical coordinates (see the module comment).
pub(crate) fn physicalize(tree: &mut FragmentTree, boxes: &BoxTree, viewport: Viewport) {
    let Some(root) = tree.root else { return };
    let root_wm = match &tree.get(root).kind {
        FragmentKind::Box(b) => boxes.get(b.box_id).style.writing_mode,
        _ => WritingMode::HorizontalTb,
    };
    let q = |v: f32| LayoutUnit::quantize(v as f64).value;
    let (vw, vh) = (q(viewport.width), q(viewport.height));
    // The initial containing block has the root's writing mode (CSS Writing Modes 4 section 3.2).
    let zero = LayoutPoint::default();
    let icb = if root_wm.is_vertical() {
        Frame {
            wm: root_wm,
            w: vh,
            h: vw,
            content: zero,
        }
    } else {
        Frame {
            wm: root_wm,
            w: vw,
            h: vh,
            content: zero,
        }
    };
    let mut decos_by_line: std::collections::HashMap<FragmentId, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, d) in tree.decorations.iter().enumerate() {
        decos_by_line.entry(d.line).or_default().push(i);
    }
    let mut w = Walk {
        boxes,
        tree,
        decos_by_line,
    };
    w.node(root, icb);
    tree.recompute_document_origins();
    remap_sticky(tree, boxes);
}

impl Walk<'_> {
    /// The writing mode of the frame fragment `id` lays its children (and its own rects) out in.
    fn own_wm(&self, id: FragmentId, parent: WritingMode) -> WritingMode {
        match &self.tree.get(id).kind {
            FragmentKind::Box(b) => self.boxes.get(b.box_id).style.writing_mode,
            FragmentKind::Line(_) => LineOrientation::of(parent).frame(),
            _ => parent,
        }
    }

    fn node(&mut self, id: FragmentId, parent: Frame) {
        let own = self.own_wm(id, parent.wm);
        let f = self.tree.get(id);
        let (lw, lh) = (f.size.width, f.size.height);
        let (pw, ph) = super::physical_size(own, lw, lh);
        // The fragment's size in its parent's frame (its origin is there).
        let (sw, sh) = if parent.wm.is_vertical() {
            (ph, pw)
        } else {
            (pw, ph)
        };
        let placed = map_rect(
            parent,
            LayoutRect {
                origin: f.origin,
                size: LayoutSize {
                    width: sw,
                    height: sh,
                },
            },
        );
        // The paragraph space of a box's lines starts at its content box; in a piece of a broken box the lines
        // were moved up by the piece's offset (015 FRAG-2b).
        let content = match &f.kind {
            FragmentKind::Box(b) => {
                let o = b.content_rect.origin;
                LayoutPoint {
                    x: o.x,
                    y: o.y - b.extras.piece.map_or(LayoutUnit::ZERO, |p| p.block_offset),
                }
            }
            _ => LayoutPoint::default(),
        };
        let line_logical = LayoutRect {
            origin: LayoutPoint {
                x: f.origin.x - parent.content.x,
                y: f.origin.y - parent.content.y,
            },
            size: f.size,
        };
        let frame = Frame {
            wm: own,
            w: lw,
            h: lh,
            content,
        };
        if own.is_vertical() || parent.wm.is_vertical() {
            let f = &mut self.tree.fragments[id.0 as usize];
            f.origin = placed.origin;
            f.size = LayoutSize {
                width: pw,
                height: ph,
            };
            if own.is_vertical() {
                match &mut f.kind {
                    FragmentKind::Box(b) => {
                        b.padding_rect = map_rect(frame, b.padding_rect);
                        b.content_rect = map_rect(frame, b.content_rect);
                        b.margins = map_edges(own, b.margins);
                        b.border_widths = map_edges(own, b.border_widths);
                        b.padding = map_edges(own, b.padding);
                        let x = &mut b.extras;
                        for r in [
                            &mut x.scrollport,
                            &mut x.scrollable_overflow,
                            &mut x.clip_rect,
                            &mut x.replaced_content_rect,
                        ]
                        .into_iter()
                        .flatten()
                        {
                            *r = map_rect(frame, *r);
                        }
                        std::mem::swap(&mut x.overflow_x, &mut x.overflow_y);
                        let rtl = self.boxes.get(b.box_id).style.direction
                            == crate::layout::box_tree::Direction::Rtl;
                        x.scroll_from_end = (
                            matches!(own, WritingMode::VerticalRl | WritingMode::SidewaysRl),
                            rtl != (own == WritingMode::SidewaysLr),
                        );
                        for r in &mut x.column_rules {
                            *r = map_rect(frame, *r);
                        }
                        if let Some(m) = &mut x.marker_symbol {
                            m.bullet = map_rect(frame, m.bullet);
                        }
                        if let Some(t) = &mut x.table_box {
                            t.rect = map_rect(frame, t.rect);
                            t.margins = map_edges(own, t.margins);
                        }
                        if let Some(p) = x.piece {
                            let whole = LayoutRect {
                                origin: LayoutPoint {
                                    x: LayoutUnit::ZERO,
                                    y: LayoutUnit::ZERO - p.block_offset,
                                },
                                size: LayoutSize {
                                    width: lw,
                                    height: p.whole_block_size,
                                },
                            };
                            x.piece_whole = Some(map_rect(frame, whole));
                        }
                        if let Some(segs) = &mut x.collapsed_borders {
                            for s in segs {
                                s.horizontal = !s.horizontal;
                            }
                        }
                    }
                    FragmentKind::InlineBoxPiece(p) => {
                        p.padding = map_edges(own, p.padding);
                        p.border_widths = map_edges(own, p.border_widths);
                        p.margins = map_edges(own, p.margins);
                        for r in &mut p.client_rects {
                            *r = map_rect(frame, *r);
                        }
                    }
                    FragmentKind::TableTrack(t) => {
                        t.rect = map_rect(parent, t.rect);
                    }
                    FragmentKind::Line(l) => {
                        l.vertical = Some(super::LineFrame {
                            orientation: LineOrientation::of(parent.wm),
                            logical: line_logical,
                        });
                    }
                    _ => {}
                }
            }
        }
        if matches!(self.tree.get(id).kind, FragmentKind::Line(_))
            && own.is_vertical()
            && let Some(ds) = self.decos_by_line.remove(&id)
        {
            for i in ds {
                let d = &mut self.tree.decorations[i];
                let r = map_rect(
                    frame,
                    LayoutRect {
                        origin: d.origin,
                        size: d.size,
                    },
                );
                d.origin = r.origin;
                d.size = r.size;
            }
        }
        let children = self.tree.get(id).children.clone();
        for c in children {
            self.node(c, frame);
        }
    }
}

/// Sticky constraints are document-space rects and physical insets: rebuilt from the mapped tree for
/// the sticky boxes of a vertical frame.
fn remap_sticky(tree: &mut FragmentTree, boxes: &BoxTree) {
    let ids: Vec<FragmentId> = tree
        .fragments
        .iter()
        .filter(|f| matches!(&f.kind, FragmentKind::Box(b) if b.extras.sticky.is_some() && boxes.get(b.box_id).style.writing_mode.is_vertical()))
        .map(|f| f.id)
        .collect();
    for id in ids {
        let normal = tree.document_rect(id).rect;
        let parent_content = tree.get(id).parent.and_then(|p| match &tree.get(p).kind {
            FragmentKind::Box(pb) => {
                let o = tree.document_rect(p).rect.origin;
                Some(LayoutRect {
                    origin: LayoutPoint {
                        x: o.x + pb.content_rect.origin.x,
                        y: o.y + pb.content_rect.origin.y,
                    },
                    size: pb.content_rect.size,
                })
            }
            _ => None,
        });
        let FragmentKind::Box(b) = &mut tree.fragments[id.0 as usize].kind else {
            continue;
        };
        let wm = boxes.get(b.box_id).style.writing_mode;
        if let Some(s) = &mut b.extras.sticky {
            let i = s.insets;
            let p = sides_to_physical(
                wm,
                Sides {
                    top: i.top,
                    right: i.right,
                    bottom: i.bottom,
                    left: i.left,
                },
            );
            s.insets = crate::layout::fragment::StickyInsets {
                top: p.top,
                right: p.right,
                bottom: p.bottom,
                left: p.left,
            };
            s.normal_flow_rect = normal;
            if let Some(cb) = parent_content {
                s.containing_block_rect = cb;
            }
        }
    }
}

/// `true` when `m` makes the box a scroll container.
#[allow(dead_code)]
fn scrolls(m: OverflowMode) -> bool {
    matches!(
        m,
        OverflowMode::Scroll | OverflowMode::Auto | OverflowMode::Hidden
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lu(v: f32) -> LayoutUnit {
        LayoutUnit::quantize(v as f64).value
    }
    fn rect(x: f32, y: f32, w: f32, h: f32) -> LayoutRect {
        LayoutRect {
            origin: LayoutPoint { x: lu(x), y: lu(y) },
            size: LayoutSize {
                width: lu(w),
                height: lu(h),
            },
        }
    }

    #[test]
    fn map_and_unmap_are_inverse() {
        for wm in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            let f = Frame {
                wm,
                w: lu(300.0),
                h: lu(200.0),
                content: LayoutPoint::default(),
            };
            let r = rect(10.0, 20.0, 30.0, 40.0);
            assert_eq!(unmap_rect(wm, f.w, f.h, map_rect(f, r)), r, "{wm:?}");
        }
    }

    #[test]
    fn vertical_rl_puts_the_first_block_on_the_right() {
        // A 300 x 200 logical frame (inline 300 tall, block 200 wide): the first 50 px of the block axis.
        let f = Frame {
            wm: WritingMode::VerticalRl,
            w: lu(300.0),
            h: lu(200.0),
            content: LayoutPoint::default(),
        };
        assert_eq!(
            map_rect(f, rect(0.0, 0.0, 300.0, 50.0)),
            rect(150.0, 0.0, 50.0, 300.0)
        );
        let f = Frame {
            wm: WritingMode::VerticalLr,
            ..f
        };
        assert_eq!(
            map_rect(f, rect(0.0, 0.0, 300.0, 50.0)),
            rect(0.0, 0.0, 50.0, 300.0)
        );
        let f = Frame {
            wm: WritingMode::SidewaysLr,
            ..f
        };
        assert_eq!(
            map_rect(f, rect(0.0, 0.0, 100.0, 50.0)),
            rect(0.0, 200.0, 50.0, 100.0)
        );
    }
}
