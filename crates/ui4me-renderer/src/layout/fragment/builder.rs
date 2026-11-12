//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! Fragment construction from Taffy's unrounded layouts and the IFC final outcomes (008
//! `ifc-interface.md` §7): one box fragment per block-level box, one line fragment per line,
//! one text fragment per (line, contiguous run of one owner). Edges are quantised in parent
//! space (008 `fragment-tree.md` §1).
//!
//! Out-of-flow boxes are children of their containing block's fragment (`fragment-tree.md` §2),
//! after its in-flow children, in document order; each carries `containing_block` and the
//! fragment of its DOM parent (`dom_parent`) for paint-order construction. Scroll containers
//! publish their scrollport and scrollable overflow (FR-062–FR-064); sticky boxes publish their
//! `StickyConstraints` (FR-060). Layout computes no scroll or sticky offset (R-18, FR-066).

use ui4me_engine_types::geometry::layout_unit::LayoutUnit;
use ui4me_engine_types::geometry::{LayoutPoint, LayoutRect, LayoutSize};

use std::collections::HashMap;

use super::{
    BoxExtras, BoxFragment, DecorationFragment, Fragment, FragmentId, FragmentKind, FragmentTree,
    InlineBoxPiece, LineFragment, OverflowMode, StickyConstraints, StickyInsets,
    StickyScrollContainer, TableTrack, TableTrackKind, TextFragment, ViewportScroll,
};
use crate::layout::box_tree::{
    BoxId, BoxKind, BoxOwner, BoxTree, InsetValue, LengthPercentage, OverflowValue, PositionValue,
};
use crate::layout::coordinator::Viewport;
use crate::layout::coordinator::host_tree::HostTree;
use crate::layout::coordinator::quantize::QuantizeSink;
use crate::layout::ifc::interface::owner_node;

struct Build<'a> {
    boxes: &'a BoxTree,
    host: &'a HostTree,
    out: FragmentTree,
    q: QuantizeSink,
    /// Out-of-flow fragments and the block-level box that is their DOM parent (resolved to a
    /// fragment once every fragment exists).
    dom_parents: Vec<(FragmentId, BoxId)>,
    /// The content-box width of the parent of the in-flow box being built (the basis of percentage margins).
    parent_content_w: f32,
    /// The next `node` call builds a piece of its box (015 FRAG-2b, `fragmentation::slice`).
    piece: Option<crate::layout::fragmentation::slice::PieceReq>,
    /// Pieces built so far per box (the next piece's index).
    piece_counts: HashMap<BoxId, u32>,
    /// Host nodes after an unforced break (pushed whole, or with a margin cut at the break): their top
    /// margin is truncated to zero (CSS Break 3 section 5.2).
    truncated: std::collections::HashSet<usize>,
    /// Building the pages of a paged layout (015 PAGE-1): every `thead` / `tfoot` repeats.
    paged: bool,
    /// Building a repeated copy (a table header or footer in a later piece, 015 FRAG-2c): its fragments are
    /// painted and hit but are not the element's boxes (geometry queries see the original only).
    cloning: bool,
    /// The previous pass's fragments, for seeded subtrees that were not laid out again (015 PERF-1).
    prev: Option<&'a FragmentTree>,
    /// `HostTree::float_bottom` per host node (015 MC-2).
    float_bottoms: HashMap<usize, f32>,
}

/// The used tracks of a grid container from Taffy's detailed grid info. A track's size is the
/// difference of its end and start; like the grid item edges it is floored to the layout unit, as
/// the reference browser does (the epsilon keeps float noise from dropping a whole unit).
fn grid_tracks_of(
    info: &taffy::DetailedLayoutInfo<String>,
) -> Option<crate::layout::fragment::GridTracks> {
    let taffy::DetailedLayoutInfo::Grid(g) = info else {
        return None;
    };
    let floor = |v: f32| LayoutUnit::from_raw(((v as f64) * 64.0 + 1e-3).floor() as i32);
    let axis = |t: &taffy::DetailedGridTracksInfo<String>| {
        let tracks: Vec<crate::layout::fragment::GridTrack> = t
            .positions
            .iter()
            .map(|p| crate::layout::fragment::GridTrack {
                position: floor(p.start),
                size: floor(p.end - p.start),
            })
            .collect();
        let names: Vec<Vec<String>> = (0..=tracks.len())
            .map(|i| t.names_for_line(i).to_vec())
            .collect();
        (tracks, names)
    };
    let ((columns, column_line_names), (rows, row_line_names)) = (axis(&g.columns), axis(&g.rows));
    Some(crate::layout::fragment::GridTracks {
        columns,
        rows,
        column_line_names,
        row_line_names,
        column_subgrid: g.columns.subgrid,
        row_subgrid: g.rows.subgrid,
    })
}

fn mode(v: OverflowValue) -> OverflowMode {
    match v {
        OverflowValue::Visible => OverflowMode::Visible,
        OverflowValue::Hidden => OverflowMode::Hidden,
        OverflowValue::Clip => OverflowMode::Clip,
        OverflowValue::Scroll => OverflowMode::Scroll,
        OverflowValue::Auto => OverflowMode::Auto,
    }
}

fn is_scroll_mode(m: OverflowMode) -> bool {
    matches!(
        m,
        OverflowMode::Hidden | OverflowMode::Scroll | OverflowMode::Auto
    )
}

/// A `<length-percentage>` inset in px against `basis`.
fn resolve_lp(v: &LengthPercentage, basis: f32) -> f32 {
    match v {
        LengthPercentage::Px(p) => *p,
        LengthPercentage::Percent(p) => p * basis,
        LengthPercentage::Calc(e) => e.eval(basis as f64) as f32,
    }
}

fn union(a: LayoutRect, b: LayoutRect) -> LayoutRect {
    let l = a.origin.x.min(b.origin.x);
    let t = a.origin.y.min(b.origin.y);
    let r = (a.origin.x + a.size.width).max(b.origin.x + b.size.width);
    let bt = (a.origin.y + a.size.height).max(b.origin.y + b.size.height);
    LayoutRect {
        origin: LayoutPoint { x: l, y: t },
        size: LayoutSize::from_edges(l, r, t, bt),
    }
}

fn translate(r: LayoutRect, by: LayoutPoint) -> LayoutRect {
    LayoutRect {
        origin: LayoutPoint {
            x: r.origin.x + by.x,
            y: r.origin.y + by.y,
        },
        size: r.size,
    }
}

/// The sum of the relative offsets (CSS 2.1 §9.4.3) of the `position: relative` inline boxes from
/// the text owner `owner` up to the inline root `root`. `left` wins over `right` in a left-to-right
/// box and `top` over `bottom`; percentages of `left`/`right` resolve against `basis` (the root's
/// content width, the width of the containing block); a percentage `top`/`bottom` needs a definite
/// containing-block height an inline box cannot see here and is treated as `auto`.
fn inline_relative_offset(
    boxes: &BoxTree,
    root: BoxId,
    owner: ui4me_engine_types::ids::NodeId,
    basis: f32,
) -> (f32, f32) {
    let Some(start) = boxes.boxes_of(owner).iter().copied().find(|b| {
        let mut cur = boxes.get(*b).parent;
        while let Some(p) = cur {
            if p == root {
                return true;
            }
            cur = boxes.get(p).parent;
        }
        false
    }) else {
        return (0.0, 0.0);
    };
    relative_chain_offset(boxes, start, basis)
}

/// The relative offset of box `b` itself (zero unless `position: relative`; rules as in
/// `inline_relative_offset`).
fn own_relative_offset(boxes: &BoxTree, b: BoxId, basis: f32) -> (f32, f32) {
    let lb = boxes.get(b);
    if lb.style.position != PositionValue::Relative {
        return (0.0, 0.0);
    }
    let px = |v: &InsetValue, basis: Option<f32>| match (v, basis) {
        (InsetValue::Auto, _) => None,
        (InsetValue::Lp(LengthPercentage::Percent(_)), None) => None,
        (InsetValue::Lp(lp), basis) => Some(resolve_lp(lp, basis.unwrap_or(0.0))),
    };
    let i = &lb.style.inset;
    let (left, right) = (px(&i.left, Some(basis)), px(&i.right, Some(basis)));
    let dx = if lb.style.direction == crate::layout::box_tree::Direction::Rtl {
        right.map(|r| -r).or(left).unwrap_or(0.0)
    } else {
        left.or(right.map(|r| -r)).unwrap_or(0.0)
    };
    let dy = px(&i.top, None)
        .or(px(&i.bottom, None).map(|b| -b))
        .unwrap_or(0.0);
    (dx, dy)
}

/// An atomic inline placed by its inline root moves with its own `position: relative` and with
/// that of the inline boxes around it (CSS 2.1 §9.4.3): the IFC places it, so no container
/// algorithm applies the offset.
fn atomic_relative_offset(boxes: &BoxTree, atomic: BoxId, basis: f32) -> (f32, f32) {
    let (mut dx, mut dy) = own_relative_offset(boxes, atomic, basis);
    if let Some(p) = boxes.get(atomic).parent {
        let (x, y) = relative_chain_offset(boxes, p, basis);
        dx += x;
        dy += y;
    }
    (dx, dy)
}

/// The summed relative offsets of `start` and the inline boxes enclosing it, up to the first box
/// that is not an inline box.
fn relative_chain_offset(boxes: &BoxTree, start: BoxId, basis: f32) -> (f32, f32) {
    let (mut dx, mut dy) = (0.0f32, 0.0f32);
    let mut cur = Some(start);
    while let Some(b) = cur {
        let lb = boxes.get(b);
        if !matches!(lb.kind, crate::layout::box_tree::BoxKind::InlineBox) {
            break;
        }
        let (x, y) = own_relative_offset(boxes, b, basis);
        dx += x;
        dy += y;
        cur = lb.parent;
    }
    (dx, dy)
}

/// Inline fragments use the line's frame, whose over side is right in both
/// vertical writing modes. The paragraph frame for vertical-lr instead has
/// block-start on the left, so its block-axis offset must be reversed.
fn line_relative_offset(boxes: &BoxTree, root: BoxId, (dx, dy): (f32, f32)) -> (f32, f32) {
    if boxes.get(root).style.writing_mode == crate::layout::writing_mode::WritingMode::VerticalLr {
        (dx, -dy)
    } else {
        (dx, dy)
    }
}

impl Build<'_> {
    fn push(
        &mut self,
        parent: Option<FragmentId>,
        origin: LayoutPoint,
        size: LayoutSize,
        kind: FragmentKind,
    ) -> FragmentId {
        let id = FragmentId(self.out.fragments.len() as u32);
        let doc = match parent {
            Some(p) => {
                let po = self.out.document_origins[p.0 as usize];
                LayoutPoint {
                    x: po.x + origin.x,
                    y: po.y + origin.y,
                }
            }
            None => origin,
        };
        let owner = match &kind {
            FragmentKind::Box(b) => match b.owner {
                BoxOwner::Element(n) => Some(n),
                BoxOwner::Anonymous(_) | BoxOwner::Pseudo(..) => None,
            },
            FragmentKind::Text(t) => Some(t.owner),
            FragmentKind::TableTrack(t) => t.box_id.and_then(|b| match self.boxes.get(b).owner {
                BoxOwner::Element(n) => Some(n),
                BoxOwner::Anonymous(_) | BoxOwner::Pseudo(..) => None,
            }),
            FragmentKind::Line(_)
            | FragmentKind::CollapsedBorderSegment(_)
            | FragmentKind::Fragmentainer(_) => None,
            FragmentKind::InlineBoxPiece(p) => Some(p.owner.node()),
            FragmentKind::Marker(m) => Some(m.owner.node()),
        };
        if let FragmentKind::Box(b) = &kind
            && !self.cloning
        {
            self.out
                .maps
                .box_to_fragments
                .entry(b.box_id)
                .or_default()
                .push(id);
        }
        self.out.fragments.push(Fragment {
            id,
            parent,
            origin,
            size,
            kind,
            children: Vec::new(),
        });
        self.out.document_origins.push(doc);
        if let Some(p) = parent {
            self.out.fragments[p.0 as usize].children.push(id);
        }
        if let Some(n) = owner
            && !self.cloning
        {
            self.out.node_to_fragments.entry(n).or_default().push(id);
        }
        if let (Some(n), true, FragmentKind::Box(_)) =
            (owner, self.cloning, &self.out.fragments[id.0 as usize].kind)
        {
            self.out.maps.repeated.entry(n).or_default().push(id);
        }
        id
    }

    /// The previous fragments of host node `idx`'s descendants, when they still hold (015 PERF-1): the node
    /// and its whole host subtree were seeded from the previous pass (`replay.rs`, same box subtree), none
    /// of them was laid out again and the node's own layout is the seeded one but for its location. Its own
    /// box fragment is built as usual; the fragments below it (lines, text, inline box pieces, decorations,
    /// descendant boxes; all in parent-relative space) are copied. Returns the old fragment range below the
    /// node and its old paragraph index.
    fn splice_source(
        &self,
        idx: usize,
        containing_block: Option<FragmentId>,
    ) -> Option<(usize, usize, Option<usize>)> {
        let old = self.prev?;
        let host = self.host;
        let n = &host.nodes[idx];
        if self.piece.is_some()
            || self.cloning
            || containing_block.is_some()
            || host.frag.is_some()
            || n.out_of_flow
            || !n.hoisted.is_empty()
        {
            return None;
        }
        if n.frag_role != crate::layout::fragmentation::FragRole::None
            || n.flow_geom.is_some()
            || n.table.is_some()
            || n.leaf.is_some()
        {
            return None;
        }
        let seeded = n.seed_layout?;
        let l = n.layout;
        if n.computed
            || l.size != seeded.size
            || l.border != seeded.border
            || l.padding != seeded.padding
            || l.margin != seeded.margin
            || l.scrollable_overflow_rect != seeded.scrollable_overflow_rect
        {
            return None;
        }
        if host.nodes[idx + 1..n.subtree_end]
            .iter()
            .any(|m| m.seed_layout.is_none() || m.computed || !m.hoisted.is_empty())
        {
            return None;
        }
        let of = match old.maps.box_to_fragments.get(&n.box_id).map(Vec::as_slice) {
            Some([f]) => f.0 as usize,
            _ => return None,
        };
        let FragmentKind::Box(ob) = &old.fragments[of].kind else {
            return None;
        };
        if ob.extras.piece.is_some() || ob.extras.sticky.is_some() {
            return None;
        }
        // The old subtree is contiguous (built by one `node` call): its end is one past its last descendant.
        let mut end = of + 1;
        let mut stack: Vec<FragmentId> = old.fragments[of].children.clone();
        let mut count = 0usize;
        while let Some(c) = stack.pop() {
            count += 1;
            end = end.max(c.0 as usize + 1);
            let f = &old.fragments[c.0 as usize];
            if let FragmentKind::Box(b) = &f.kind
                && (b.extras.sticky.is_some()
                    || b.extras.containing_block.is_some()
                    || b.extras.dom_parent.is_some()
                    || b.extras.piece.is_some())
            {
                return None;
            }
            if matches!(
                f.kind,
                FragmentKind::Fragmentainer(_)
                    | FragmentKind::TableTrack(_)
                    | FragmentKind::CollapsedBorderSegment(_)
            ) {
                return None;
            }
            stack.extend(f.children.iter().copied());
        }
        if end - of - 1 != count {
            return None;
        }
        Some((of + 1, end, ob.paragraph_index))
    }

    /// Copy the old fragments `from..to` (the descendants of an old box fragment whose paragraph was `old_pi`)
    /// under the new fragment `id` whose paragraph is `new_pi` (see `splice_source`).
    fn splice_descendants(
        &mut self,
        id: FragmentId,
        from: usize,
        to: usize,
        old_pi: Option<usize>,
        new_pi: Option<usize>,
    ) {
        let old = self.prev.expect("a splice has a previous tree");
        let old_root = from - 1;
        let base = self.out.fragments.len();
        let map_id = |f: FragmentId| {
            if f.0 as usize == old_root {
                id
            } else {
                FragmentId((f.0 as usize - from + base) as u32)
            }
        };
        let mut paragraphs: HashMap<usize, usize> = HashMap::new();
        if let (Some(o), Some(n)) = (old_pi, new_pi) {
            paragraphs.insert(o, n);
        }
        for k in from..to {
            let f = &old.fragments[k];
            let mut kind = f.kind.clone();
            let mut remap = |pi: &mut usize| {
                let next = self.out.paragraphs.len();
                let n = *paragraphs.entry(*pi).or_insert(next);
                if n == next {
                    self.out.paragraphs.push(old.paragraphs[*pi].clone());
                }
                *pi = n;
            };
            match &mut kind {
                FragmentKind::Box(b) => {
                    if let Some(pi) = b.paragraph_index.as_mut() {
                        remap(pi);
                    }
                    if let Some(ci) = b.extras.control_inner.as_mut()
                        && let Some(pi) = ci.paragraph_index.as_mut()
                    {
                        remap(pi);
                    }
                }
                FragmentKind::Line(line) => remap(&mut line.paragraph_index),
                FragmentKind::Text(t) => remap(&mut t.paragraph_index),
                FragmentKind::Marker(m) => {
                    if let Some(pi) = m.paragraph_index.as_mut() {
                        remap(pi);
                    }
                }
                _ => {}
            }
            let parent = f.parent.map(map_id);
            let nid = self.push(parent, f.origin, f.size, kind);
            debug_assert_eq!(nid, map_id(f.id));
        }
        // The decorations of the copied lines, in line order like the rest.
        let lo = old
            .decorations
            .partition_point(|d| (d.line.0 as usize) < from);
        let hi = old
            .decorations
            .partition_point(|d| (d.line.0 as usize) < to);
        for d in &old.decorations[lo..hi] {
            let mut d = d.clone();
            d.line = map_id(d.line);
            self.out.decorations.push(d);
        }
    }

    /// The table tracks or collapsed border segments pushed to `table` from fragment `from` on, for a piece
    /// `pc` of the table (015 FRAG-2c): moved into the piece's space and cut to it (the ones outside it
    /// become empty).
    fn window_table_parts(
        &mut self,
        table: FragmentId,
        from: usize,
        pc: crate::layout::fragmentation::slice::PieceReq,
    ) {
        let top = self.q.edge(pc.top);
        let bottom = if pc.last_band {
            None
        } else {
            Some(self.q.edge(pc.bottom) - top)
        };
        let base = self.out.document_origins[table.0 as usize];
        for k in from..self.out.fragments.len() {
            let f = &mut self.out.fragments[k];
            if f.parent != Some(table)
                || !matches!(
                    f.kind,
                    FragmentKind::TableTrack(_) | FragmentKind::CollapsedBorderSegment(_)
                )
            {
                continue;
            }
            let mut y0 = f.origin.y - top;
            let mut y1 = y0 + f.size.height;
            y0 = y0.max(LayoutUnit::ZERO);
            if let Some(b) = bottom {
                // A part below the piece is empty at its end, not past it (015 MC-2: no scrollable overflow there).
                y0 = y0.min(b);
                y1 = y1.min(b);
            }
            let h = if y1 > y0 { y1 - y0 } else { LayoutUnit::ZERO };
            f.origin.y = y0;
            f.size.height = h;
            if let FragmentKind::TableTrack(t) = &mut f.kind {
                t.rect = LayoutRect {
                    origin: f.origin,
                    size: f.size,
                };
            }
            let o = f.origin;
            self.out.document_origins[k] = LayoutPoint {
                x: base.x + o.x,
                y: base.y + o.y,
            };
        }
    }

    /// The row group tracks of a piece of a broken table (015 STAB-1, D-REL04-1, measured): the rows of the group in the
    /// piece, not the piece's whole window (the repeated header and footer are not part of the body group there).
    fn fit_row_groups(
        &mut self,
        table: FragmentId,
        from: usize,
        d: &crate::layout::table::GridData,
    ) {
        let group_of = |b: BoxId| {
            d.row_groups
                .iter()
                .find(|(_, r)| {
                    r.clone()
                        .any(|k| d.rows.get(k).is_some_and(|row| row.box_id == b))
                })
                .map(|(g, _)| *g)
        };
        let mut extent: HashMap<BoxId, (LayoutUnit, LayoutUnit)> = HashMap::new();
        for k in from..self.out.fragments.len() {
            let f = &self.out.fragments[k];
            if f.parent != Some(table) || f.size.height <= LayoutUnit::ZERO {
                continue;
            }
            if let FragmentKind::TableTrack(t) = &f.kind
                && t.kind == TableTrackKind::Row
                && let Some(g) = t.box_id.and_then(group_of)
            {
                let (y0, y1) = (f.origin.y, f.origin.y + f.size.height);
                let e = extent.entry(g).or_insert((y0, y1));
                *e = (e.0.min(y0), e.1.max(y1));
            }
        }
        let base = self.out.document_origins[table.0 as usize];
        for k in from..self.out.fragments.len() {
            let f = &mut self.out.fragments[k];
            if f.parent != Some(table) {
                continue;
            }
            let FragmentKind::TableTrack(t) = &mut f.kind else {
                continue;
            };
            if t.kind != TableTrackKind::RowGroup {
                continue;
            }
            let Some(b) = t.box_id else { continue };
            let (y0, y1) = extent
                .get(&b)
                .copied()
                .unwrap_or((f.origin.y + f.size.height, f.origin.y + f.size.height));
            f.origin.y = y0;
            f.size.height = y1 - y0;
            t.rect = LayoutRect {
                origin: f.origin,
                size: f.size,
            };
            self.out.document_origins[k] = LayoutPoint {
                x: base.x + f.origin.x,
                y: base.y + f.origin.y,
            };
        }
    }

    fn box_fragment(&self, id: FragmentId) -> Option<&BoxFragment> {
        match &self.out.fragments[id.0 as usize].kind {
            FragmentKind::Box(b) => Some(b),
            _ => None,
        }
    }

    /// Document-space border box of an existing fragment.
    fn doc_rect(&self, id: FragmentId) -> LayoutRect {
        LayoutRect {
            origin: self.out.document_origins[id.0 as usize],
            size: self.out.fragments[id.0 as usize].size,
        }
    }

    /// `StickyConstraints` of the sticky box `id` (FR-060): its nearest scroll container (or the
    /// viewport), the containing block's content box, its resolved insets and its normal-flow
    /// rect (it is laid out there). Percentage insets resolve against the scrollport.
    fn sticky(&mut self, id: FragmentId, box_id: BoxId, parent: FragmentId) -> StickyConstraints {
        let style = &self.boxes.get(box_id).style;
        let mut container = None;
        let mut cursor = Some(parent);
        while let Some(c) = cursor {
            if self
                .box_fragment(c)
                .is_some_and(|b| b.extras.is_scroll_container)
            {
                container = Some(c);
                break;
            }
            cursor = self.out.fragments[c.0 as usize].parent;
        }
        let scrollport = match container {
            Some(c) => self
                .box_fragment(c)
                .and_then(|b| b.extras.scrollport)
                .map(|r| LayoutRect {
                    origin: r.origin,
                    size: r.size,
                }),
            None => Some(self.out.viewport_scroll.scrollport),
        }
        .unwrap_or_default();
        let (pw, ph) = (
            scrollport.size.width.to_f64() as f32,
            scrollport.size.height.to_f64() as f32,
        );
        let mut inset = |v: &InsetValue, basis: f32| match v {
            InsetValue::Auto => None,
            InsetValue::Lp(lp) => Some(self.q.edge(resolve_lp(lp, basis))),
        };
        let insets = StickyInsets {
            top: inset(&style.inset.top, ph),
            right: inset(&style.inset.right, pw),
            bottom: inset(&style.inset.bottom, ph),
            left: inset(&style.inset.left, pw),
        };
        let mut cb = self
            .box_fragment(parent)
            .map(|b| b.content_rect)
            .unwrap_or_default();
        // A sticky box directly in its scroll container is constrained by the container's content
        // as it extends over the scrollable overflow (as browsers do): the content box of a
        // scroll container is only its viewport onto that content.
        if container == Some(parent)
            && let Some(so) = self
                .box_fragment(parent)
                .and_then(|b| b.extras.scrollable_overflow)
        {
            let right = (cb.origin.x + cb.size.width).max(so.origin.x + so.size.width);
            let bottom = (cb.origin.y + cb.size.height).max(so.origin.y + so.size.height);
            cb = LayoutRect {
                origin: cb.origin,
                size: LayoutSize::from_edges(cb.origin.x, right, cb.origin.y, bottom),
            };
        }
        let containing_block_rect = translate(cb, self.out.document_origins[parent.0 as usize]);
        StickyConstraints {
            scroll_container: container.map_or(
                StickyScrollContainer::Viewport,
                StickyScrollContainer::Fragment,
            ),
            containing_block_rect,
            insets,
            normal_flow_rect: self.doc_rect(id),
        }
    }

    /// The inline box (`BoxKind::InlineBox`) of an element, if it generated one.
    fn inline_box_of(&self, node: ui4me_engine_types::ids::NodeId) -> Option<BoxId> {
        self.boxes
            .boxes_of(node)
            .iter()
            .copied()
            .find(|b| self.boxes.get(*b).kind == BoxKind::InlineBox)
    }

    /// The inline boxes enclosing `id` (itself first when it is one), innermost first.
    fn inline_chain(&self, id: BoxId) -> Vec<BoxId> {
        let mut out = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let b = self.boxes.get(c);
            if b.kind == BoxKind::InlineBox {
                out.push(c);
            }
            cur = b.parent;
        }
        out
    }

    /// The inline boxes under the inline root `root`, by owner tag (the element's, or its
    /// pseudo-element's for generated boxes): where a text run or edge spacer says which box it
    /// belongs to.
    fn inline_boxes_by_tag(&self, root: BoxId) -> HashMap<u64, BoxId> {
        use crate::layout::ifc::interface::{owner_tag, owner_tag_pseudo};
        let mut out = HashMap::new();
        let mut stack: Vec<BoxId> = self
            .boxes
            .get(root)
            .children
            .iter()
            .rev()
            .copied()
            .collect();
        while let Some(id) = stack.pop() {
            let b = self.boxes.get(id);
            if b.kind != BoxKind::InlineBox {
                continue;
            }
            let tag = match b.owner {
                BoxOwner::Pseudo(n, k) => owner_tag_pseudo(n, k),
                BoxOwner::Element(n) | BoxOwner::Anonymous(n) => owner_tag(n),
            };
            out.entry(tag).or_insert(id);
            // The spans of the box on the first line of a block with `::first-line` carry the variant's tag (015 TXT-5).
            if b.style.first_line.is_some() {
                use crate::layout::box_tree::PseudoKind as K;
                let fl = match b.owner {
                    BoxOwner::Pseudo(n, K::Before) => Some(owner_tag_pseudo(n, K::FirstLineBefore)),
                    BoxOwner::Pseudo(n, K::After) => Some(owner_tag_pseudo(n, K::FirstLineAfter)),
                    BoxOwner::Pseudo(..) => None,
                    BoxOwner::Element(n) | BoxOwner::Anonymous(n) => {
                        Some(owner_tag_pseudo(n, K::FirstLine))
                    }
                };
                if let Some(t) = fl {
                    out.entry(t).or_insert(id);
                }
            }
            stack.extend(b.children.iter().rev().copied());
        }
        out
    }

    /// `InlineBoxPiece` fragments (G-TEXT-EDGES): one per inline box on a line, sized as its
    /// content area there (the span from after its start edge, or the first content of the line,
    /// to before its end edge, or the last content: the content of the boxes inside it included,
    /// atomic inlines too), as high as the ascent and descent of its own text, or of its own font
    /// when it has no text of its own there (CSS 2.1 section 10.6.1). Outer boxes come first (paint
    /// order). They are children of the line fragment, like the text fragments. A box with no
    /// background, padding or border has a piece for its geometry only (`paints: false`).
    #[allow(clippy::too_many_arguments)]
    fn inline_box_pieces(
        &mut self,
        line_id: FragmentId,
        li: usize,
        line: &crate::layout::ifc::interface::LineBox,
        text_pieces: &[crate::text::paragraph::TextPiece],
        fin: &crate::layout::ifc::interface::FinalOutcome,
        rel_basis: f32,
        by_tag: &HashMap<u64, BoxId>,
        root: BoxId,
    ) {
        #[derive(Default)]
        struct Ext {
            content: Option<(f32, f32)>,
            /// The start and end edge spacers of the box on the line: (x, width).
            start_edge: Option<(f32, f32)>,
            end_edge: Option<(f32, f32)>,
            ascent: f32,
            descent: f32,
            baseline: Option<f32>,
        }
        let mut ext: HashMap<BoxId, Ext> = HashMap::new();
        let widen = |ext: &mut HashMap<BoxId, Ext>, chain: &[BoxId], x0: f32, x1: f32| {
            for b in chain {
                let e = ext.entry(*b).or_default();
                e.content = Some(match e.content {
                    Some((a, z)) => (a.min(x0), z.max(x1)),
                    None => (x0, x1),
                });
            }
        };
        let box_of_tag = |this: &Self, tag: u64| -> Option<BoxId> {
            by_tag
                .get(&tag)
                .copied()
                .or_else(|| owner_node(tag).and_then(|n| this.inline_box_of(n)))
        };
        // The content that belongs to a box directly (its own text, the atomic inlines it holds),
        // as x intervals: the pieces of the box's `client_rects`.
        let mut direct: HashMap<BoxId, Vec<(f32, f32, u8)>> = HashMap::new();
        for piece in text_pieces {
            let Some(bid) = box_of_tag(self, piece.owner) else {
                continue;
            };
            // The text of a `::first-letter` box is content of the inline box around it (its rectangles are one
            // fragment with the rest of the box's text, as the reference browser reports them, 015 TXT-5).
            let direct_bid = match self.boxes.get(bid).owner {
                BoxOwner::Pseudo(_, crate::layout::box_tree::PseudoKind::FirstLetter) => self
                    .boxes
                    .get(bid)
                    .parent
                    .filter(|p| self.boxes.get(*p).kind == BoxKind::InlineBox)
                    .unwrap_or(bid),
                _ => bid,
            };
            direct.entry(direct_bid).or_default().push((
                piece.x,
                piece.x + piece.width,
                if piece.hyphen { 2 } else { piece.bidi_level },
            ));
            let chain = self.inline_chain(bid);
            widen(&mut ext, &chain, piece.x, piece.x + piece.width);
            // The box's own text sets the content area's height.
            let e = ext.entry(bid).or_default();
            e.ascent = e.ascent.max(piece.ascent);
            e.descent = e.descent.max(piece.descent);
            e.baseline.get_or_insert(piece.baseline);
        }
        for a in fin.paragraph.atomic_placements(li) {
            let chain = self.inline_chain(BoxId(a.item as u32));
            widen(&mut ext, &chain, a.x, a.x + a.width);
            if let Some(parent) = self
                .boxes
                .get(BoxId(a.item as u32))
                .parent
                .filter(|p| self.boxes.get(*p).kind == BoxKind::InlineBox)
            {
                direct
                    .entry(parent)
                    .or_default()
                    .push((a.x, a.x + a.width, 0));
            }
        }
        for e in fin.extras.edges.iter().filter(|e| e.line_index == li) {
            let Some(bid) = box_of_tag(self, e.owner) else {
                continue;
            };
            // The spacer is content of the enclosing boxes ...
            let parent_chain = self
                .boxes
                .get(bid)
                .parent
                .map(|p| self.inline_chain(p))
                .unwrap_or_default();
            widen(&mut ext, &parent_chain, e.x, e.x + e.width);
            // ... and bounds its own box's content area.
            let x = ext.entry(bid).or_default();
            if e.start {
                x.start_edge = Some((e.x, e.width));
            } else {
                x.end_edge = Some((e.x, e.width));
            }
        }
        // `::first-line` with a background (015 TXT-5): a box around the content of the first line, outside every
        // inline box (the reference browser paints it as the first inline box of the line, as high as the block's
        // font's content area).
        if li == 0
            && let Some(fl) = self.boxes.get(root).style.first_line.as_ref()
            && fl.has_background
        {
            let mut span: Option<(f32, f32)> = None;
            let mut add =
                |a: f32, z: f32| span = Some(span.map_or((a, z), |(x, y)| (x.min(a), y.max(z))));
            for p in text_pieces {
                add(p.x, p.x + p.width);
            }
            for a in fin.paragraph.atomic_placements(li) {
                add(a.x, a.x + a.width);
            }
            for e in fin.extras.edges.iter().filter(|e| e.line_index == li) {
                add(e.x, e.x + e.width);
            }
            if let (Some((x0, x1)), Some(first)) = (span, text_pieces.first()) {
                let r = self.q.rect(
                    x0 - line.x,
                    first.baseline - first.ascent - line.y,
                    x1 - x0,
                    first.ascent + first.descent,
                );
                let zero = self.q.edges(0.0, 0.0, 0.0, 0.0);
                let owner = BoxOwner::Pseudo(
                    self.boxes.get(root).owner.node(),
                    crate::layout::box_tree::PseudoKind::FirstLine,
                );
                self.push(
                    Some(line_id),
                    r.origin,
                    r.size,
                    FragmentKind::InlineBoxPiece(InlineBoxPiece {
                        box_id: root,
                        owner,
                        has_start_edge: false,
                        has_end_edge: false,
                        padding: zero,
                        border_widths: zero,
                        margins: zero,
                        paints: true,
                        client_rects: Vec::new(),
                        bidi_level: 0,
                    }),
                );
            }
        }
        let mut boxes: Vec<BoxId> = ext.keys().copied().collect();
        // Outer boxes first: fewer enclosing inline boxes, then tree order.
        boxes.sort_by_key(|b| (self.inline_chain(*b).len(), b.0));
        struct Geo {
            /// Content area in line space: x0, x1, top, bottom.
            area: (f32, f32, f32, f32),
            /// Padding + border: left, right, top, bottom (the edges the piece has).
            edges: (f32, f32, f32, f32),
            /// Margins: left, right, top, bottom.
            margins: (f32, f32, f32, f32),
            paints: bool,
        }
        let basis = fin
            .paragraph
            .lines()
            .get(li)
            .map(|l| l.inline_size)
            .unwrap_or(0.0);
        let pad = |lp: &crate::layout::box_tree::LengthPercentage| match lp {
            crate::layout::box_tree::LengthPercentage::Px(p) => *p,
            crate::layout::box_tree::LengthPercentage::Percent(p) => p * basis,
            crate::layout::box_tree::LengthPercentage::Calc(c) => {
                crate::layout::coordinator::calc::resolve_calc_value(c, basis)
            }
        };
        let mar = |m: &crate::layout::box_tree::MarginValue| match m {
            crate::layout::box_tree::MarginValue::Auto => 0.0,
            crate::layout::box_tree::MarginValue::Lp(lp) => pad(lp),
        };
        let mut geo: HashMap<BoxId, Geo> = HashMap::new();
        for &bid in &boxes {
            let Some(e) = ext.get(&bid) else { continue };
            let (cmin, cmax) = e.content.unwrap_or((line.x, line.x));
            // The edges sit on the left or right of the content by the direction the box was laid
            // out in (a right-to-left box has its start edge on the right).
            let start_left = match (e.start_edge, e.end_edge) {
                (Some(s), Some(en)) => s.0 <= en.0,
                (Some(s), None) | (None, Some(s)) if e.content.is_some() => {
                    let left_of_content = s.0 + s.1 / 2.0 < (cmin + cmax) / 2.0;
                    left_of_content == e.start_edge.is_some()
                }
                _ => true,
            };
            let (left_edge, right_edge) = if start_left {
                (e.start_edge, e.end_edge)
            } else {
                (e.end_edge, e.start_edge)
            };
            let x0 = left_edge.map_or(cmin, |(x, w)| x + w);
            let x1 = right_edge.map_or(cmax, |(x, _)| x).max(x0);
            // Height: the box's own text; else its own font on the line's baseline; else the
            // first text of the line; else the line box.
            let own = fin.extras.box_metrics.iter().find(|m| m.0 == bid.0);
            let (asc, desc, base) = if e.baseline.is_some() {
                (e.ascent, e.descent, e.baseline.unwrap_or(line.baseline))
            } else if let Some(&(_, a, d)) = own {
                (a, d, line.baseline)
            } else if let Some(t) = text_pieces.first() {
                (t.ascent, t.descent, t.baseline)
            } else {
                (
                    line.baseline - line.y,
                    line.y + line.height - line.baseline,
                    line.baseline,
                )
            };
            // `position: relative` on the box or the inline boxes around it moves the piece with
            // its text (FR-055; the offsets are those of `inline_relative_offset`).
            let (rx, ry) = line_relative_offset(
                self.boxes,
                root,
                relative_chain_offset(self.boxes, bid, rel_basis),
            );
            let s = &self.boxes.get(bid).style;
            let paints = s.has_background
                || !s.padding.top.is_zero()
                || !s.padding.right.is_zero()
                || !s.padding.bottom.is_zero()
                || !s.padding.left.is_zero()
                || s.border.top != 0.0
                || s.border.right != 0.0
                || s.border.bottom != 0.0
                || s.border.left != 0.0;
            let l = if left_edge.is_some() {
                pad(&s.padding.left) + s.border.left
            } else {
                0.0
            };
            let r = if right_edge.is_some() {
                pad(&s.padding.right) + s.border.right
            } else {
                0.0
            };
            geo.insert(
                bid,
                Geo {
                    area: (x0 + rx, x1 + rx, base - asc + ry, base + desc + ry),
                    edges: (
                        l,
                        r,
                        pad(&s.padding.top) + s.border.top,
                        pad(&s.padding.bottom) + s.border.bottom,
                    ),
                    margins: (
                        mar(&s.margin.left),
                        mar(&s.margin.right),
                        mar(&s.margin.top),
                        mar(&s.margin.bottom),
                    ),
                    paints,
                },
            );
        }
        // What `getClientRects()` reports for each box on the line, in line space (x0, top, x1,
        // bottom): one border box for a box with a background, padding or border; else the runs of
        // its own content (text, atomic inlines) and the rectangles of the inline boxes inside it,
        // in order, the runs between two inline boxes merged (the reference browser's culled
        // inline boxes). Boxes inside are done before the box.
        type Rc = (f32, f32, f32, f32);
        let mut rects: HashMap<BoxId, Vec<Rc>> = HashMap::new();
        let mut deepest = boxes.clone();
        deepest.reverse();
        for &bid in &deepest {
            let Some(g) = geo.get(&bid) else { continue };
            let (x0, x1, top, bottom) = g.area;
            if g.paints {
                rects.insert(
                    bid,
                    vec![(
                        x0 - g.edges.0,
                        top - g.edges.2,
                        x1 + g.edges.1,
                        bottom + g.edges.3,
                    )],
                );
                continue;
            }
            // (x0, x1, rects of the inline box inside) in text order; the level of a run keeps the
            // runs of different bidi levels apart (one rectangle per fragment of the line).
            let mut items: Vec<(f32, f32, Option<Vec<Rc>>, u8)> = Vec::new();
            let (rx, _) = relative_chain_offset(self.boxes, bid, rel_basis);
            for &(a, z, lv) in direct.get(&bid).map(Vec::as_slice).unwrap_or(&[]) {
                items.push((a + rx, z + rx, None, lv));
            }
            for c in &boxes {
                if self.boxes.get(*c).parent == Some(bid)
                    && !matches!(
                        self.boxes.get(*c).owner,
                        BoxOwner::Pseudo(_, crate::layout::box_tree::PseudoKind::FirstLetter)
                    )
                    && let Some(list) = rects.get(c)
                    && let Some(first) = list.first()
                {
                    items.push((first.0, first.2, Some(list.clone()), 0));
                }
            }
            items.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut out: Vec<Rc> = Vec::new();
            let mut run: Option<(f32, f32, u8)> = None;
            for (a, z, child, lv) in items {
                match child {
                    // Runs of one bidi level merge; a run of another level is another fragment.
                    None => match run {
                        Some((ra, rz, rl)) if rl == lv => run = Some((ra.min(a), rz.max(z), rl)),
                        Some((ra, rz, _)) => {
                            out.push((ra, top, rz, bottom));
                            run = Some((a, z, lv));
                        }
                        None => run = Some((a, z, lv)),
                    },
                    Some(list) => {
                        if let Some((ra, rz, _)) = run.take() {
                            out.push((ra, top, rz, bottom));
                        }
                        out.extend(list);
                    }
                }
            }
            if let Some((ra, rz, _)) = run {
                out.push((ra, top, rz, bottom));
            }
            if out.is_empty() {
                out.push((x0, top, x1, bottom));
            }
            rects.insert(bid, out);
        }
        for bid in boxes {
            let (Some(e), Some(g)) = (ext.get(&bid), geo.get(&bid)) else {
                continue;
            };
            let lb = self.boxes.get(bid);
            let (x0, x1, top, bottom) = g.area;
            let r = self
                .q
                .rect(x0 - line.x, top - line.y, x1 - x0, bottom - top);
            let client_rects: Vec<LayoutRect> = rects
                .get(&bid)
                .map(|l| {
                    l.iter()
                        .map(|&(a, t, z, b)| {
                            let c = self.q.rect(a - line.x, t - line.y, z - a, b - t);
                            LayoutRect {
                                origin: LayoutPoint {
                                    x: c.origin.x - r.origin.x,
                                    y: c.origin.y - r.origin.y,
                                },
                                size: c.size,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let s = &lb.style;
            let padding = self.q.edges(
                pad(&s.padding.top),
                pad(&s.padding.right),
                pad(&s.padding.bottom),
                pad(&s.padding.left),
            );
            let border_widths =
                self.q
                    .edges(s.border.top, s.border.right, s.border.bottom, s.border.left);
            let margins = self
                .q
                .edges(g.margins.2, g.margins.1, g.margins.3, g.margins.0);
            let owner = lb.owner;
            self.push(
                Some(line_id),
                r.origin,
                r.size,
                FragmentKind::InlineBoxPiece(InlineBoxPiece {
                    box_id: bid,
                    owner,
                    has_start_edge: e.start_edge.is_some(),
                    has_end_edge: e.end_edge.is_some(),
                    padding,
                    border_widths,
                    margins,
                    paints: g.paints,
                    client_rects,
                    bidi_level: 0,
                }),
            );
        }
    }

    /// The decoration fragments of line `li` (009 segments, FR-044): geometry in the line fragment's
    /// space, moved with the decorating box's own `position: relative` offset (an inline box that
    /// is relatively positioned moves its text and its lines together).
    fn decoration_fragments(
        &mut self,
        line_id: FragmentId,
        li: usize,
        line: &crate::layout::ifc::interface::LineBox,
        paragraph: &crate::text::paragraph::FrozenParagraph,
        root: BoxId,
        rel_basis: f32,
    ) {
        use crate::text::paragraph::DecorationLineStyle as S;
        let segments: Vec<_> = paragraph.line_decorations(li).cloned().collect();
        for seg in segments {
            let Some(owner) = owner_node(seg.owner) else {
                continue;
            };
            let (dx, dy) = line_relative_offset(
                self.boxes,
                root,
                inline_relative_offset(self.boxes, root, owner, rel_basis),
            );
            let t = seg.thickness;
            let extent = match seg.style {
                S::Solid | S::Dotted | S::Dashed => t,
                S::Double | S::Wavy => 3.0 * t,
            };
            // Extra lines of an overline extend away from the text (upward), the others downward.
            let top = seg.baseline + seg.offset - line.y + dy
                - if seg.kind == crate::text::paragraph::DecorationKind::Overline {
                    extent - t
                } else {
                    0.0
                };
            let r = self.q.rect(
                seg.inline_range.0 - line.x + dx,
                top,
                seg.inline_range.1 - seg.inline_range.0,
                extent,
            );
            let thickness = self.q.edge(t);
            self.out.decorations.push(DecorationFragment {
                line: line_id,
                origin: r.origin,
                size: r.size,
                owner,
                kind: seg.kind,
                style: seg.style,
                owner_pseudo: crate::layout::ifc::interface::owner_pseudo(seg.owner),
                thickness,
            });
        }
    }

    /// Build the fragment of host node `idx`. `containing_block` is set for an out-of-flow box
    /// hoisted to its containing block's fragment (the `parent`). `shift`: the content-box offset of
    /// an inline root for its atomic inlines, whose layout locations are relative to that content
    /// box.
    fn node(
        &mut self,
        idx: usize,
        parent: Option<FragmentId>,
        containing_block: Option<FragmentId>,
        shift: (f32, f32),
    ) {
        let (host, boxes) = (self.host, self.boxes);
        let splice = self.splice_source(idx, containing_block);
        let n = &host.nodes[idx];
        let mut l = n.layout;
        // An orthogonal flow root (015 WM-1): its fragment is built in its own frame (its origin stays in
        // its parent's).
        if n.outer.is_some() {
            crate::layout::writing_mode::layout_to_own(&mut l, n.parent_wm, n.wm);
            if let Some(o) = n.ortho_overflow {
                l.scrollable_overflow_rect = o;
            }
        }
        // A flow thread reports its column height to its container: the strip it is cut from is taller.
        if let Some(g) = n
            .flow_geom
            .as_deref()
            .filter(|g| g.block_size > crate::layout::fragmentation::EPS)
        {
            l.size.height = g.flow_height;
        }
        // A piece of a box broken across fragmentainers (015 FRAG-2b): its part `[top, bottom)` of the
        // border box; the edges at a break are sliced off (`box-decoration-break: slice`).
        let piece = self.piece.take();
        // A piece below the box's own end (its floats continue there, 015 MC-2) is empty, at the band's top.
        let own = |v: f32| v.min(l.size.height).max(0.0);
        let rect = if let Some(pc) = piece {
            self.q.rect(
                l.location.x + shift.0,
                l.location.y + shift.1 + pc.top,
                l.size.width,
                own(pc.bottom) - own(pc.top),
            )
        } else if n.grid_item {
            // Grid track offsets are floored to the layout unit in the reference browser (a track edge
            // is the floor of the unrounded cumulative track sizes: three `1fr` tracks of 800 px end at
            // 266.65625, 533.328125 and 799.984375), not rounded to nearest like the other layouts. The
            // epsilon keeps an edge that float noise left a hair below a unit from dropping a whole unit.
            let floor = |v: f32| ((v * 64.0 + 1e-3).floor()) / 64.0;
            let (x0, y0) = (l.location.x + shift.0, l.location.y + shift.1);
            let (fx, fy) = (floor(x0), floor(y0));
            self.q.rect(
                fx,
                fy,
                floor(x0 + l.size.width) - fx,
                floor(y0 + l.size.height) - fy,
            )
        } else {
            self.q.rect(
                l.location.x + shift.0,
                l.location.y + shift.1,
                l.size.width,
                l.size.height,
            )
        };
        let border = self
            .q
            .edges(l.border.top, l.border.right, l.border.bottom, l.border.left);
        let padding = self.q.edges(
            l.padding.top,
            l.padding.right,
            l.padding.bottom,
            l.padding.left,
        );
        let mut margins =
            self.q
                .edges(l.margin.top, l.margin.right, l.margin.bottom, l.margin.left);
        // A child of a `-webkit-center | -webkit-right` parent was moved by auto margins; the box keeps its
        // specified margins (the offset is not a margin in the reference browser).
        if let Some((ml, mr)) = &n.legacy_align_margins {
            let basis = self.parent_content_w;
            margins = self.q.edges(
                l.margin.top,
                resolve_lp(mr, basis),
                l.margin.bottom,
                resolve_lp(ml, basis),
            );
        }
        // The legend's ignored top margin and auto inline margins still need CSSOM box
        // geometry; flex positioning uses the layout margins above, not these reported values.
        let legend_style = &boxes.get(n.box_id).style;
        if let Some(original_top) = &legend_style.legend_report_margin_top {
            use crate::layout::box_tree::MarginValue;
            margins.top = self.q.edge(match original_top {
                MarginValue::Auto => 0.0,
                MarginValue::Lp(v) => resolve_lp(v, self.parent_content_w),
            });
            if matches!(legend_style.margin.left, MarginValue::Auto) {
                margins.left = LayoutUnit::ZERO;
            }
            if matches!(legend_style.margin.right, MarginValue::Auto) {
                margins.right = LayoutUnit::ZERO;
            }
        }
        if self.truncated.contains(&idx) {
            margins.top = LayoutUnit::ZERO;
        }
        let mut table_box = None;
        // The table element's margins live on its (anonymous) wrapper; the grid box that carries the
        // element's fragment reports them, so its margin box is the element's CSS margin box.
        if matches!(
            n.table.as_deref(),
            Some(crate::layout::table::TableHost::Grid(_))
        ) && let Some(p) = parent
            && let FragmentKind::Box(pb) = &self.out.fragments[p.0 as usize].kind
            && matches!(
                boxes.get(pb.box_id).tag,
                crate::layout::box_tree::BoxTag::Table(crate::layout::box_tree::TableTag::Wrapper)
            )
        {
            margins = pb.margins;
            table_box = Some(crate::layout::fragment::TableBoxInfo {
                rect: LayoutRect {
                    origin: LayoutPoint {
                        x: LayoutUnit::ZERO - rect.origin.x,
                        y: LayoutUnit::ZERO - rect.origin.y,
                    },
                    size: self.out.fragments[p.0 as usize].size,
                },
                margins: pb.margins,
            });
        }
        let mut border = border;
        let mut padding = padding;
        let piece_info = piece.map(|pc| {
            let index = {
                let e = self.piece_counts.entry(n.box_id).or_insert(0);
                *e += 1;
                *e - 1
            };
            let last = pc.bottom
                >= l.size.height.max(self.host.flow_overflow(idx))
                    - crate::layout::fragmentation::EPS;
            if index > 0 {
                border.top = LayoutUnit::ZERO;
                padding.top = LayoutUnit::ZERO;
                margins.top = LayoutUnit::ZERO;
            }
            if !last {
                border.bottom = LayoutUnit::ZERO;
                padding.bottom = LayoutUnit::ZERO;
                margins.bottom = LayoutUnit::ZERO;
            }
            super::FragmentPiece {
                index,
                last,
                block_offset: self.q.edge(pc.top),
                whole_block_size: self.q.edge(l.size.height),
                repeated: false,
                split: false,
            }
        });
        // A fieldset with a rendered legend (015 HTML-3): its real block-start border and padding, for the box
        // model it reports (the reference browser's padding and content boxes start below the border, the
        // legend's excess height inside the content box).
        if let Some((bt, pt)) = boxes.get(n.box_id).style.fieldset_border_top {
            border.top = self.q.edge(bt);
            padding.top = self.q.edge(pt);
        }
        let size = rect.size;
        let padding_rect = LayoutRect {
            origin: LayoutPoint {
                x: border.left,
                y: border.top,
            },
            size: LayoutSize::from_edges(
                border.left,
                size.width - border.right,
                border.top,
                size.height - border.bottom,
            ),
        };
        let (cl, ct) = (border.left + padding.left, border.top + padding.top);
        let content_rect = LayoutRect {
            origin: LayoutPoint { x: cl, y: ct },
            size: LayoutSize::from_edges(
                cl,
                size.width - border.right - padding.right,
                ct,
                size.height - border.bottom - padding.bottom,
            ),
        };
        // A text control's inner editing area (`replaced/inner.rs`) is laid out at the used
        // content-box size and emitted like the lines of an inline root.
        let inner_layout = n.leaf.as_ref().and_then(|lf| lf.inner.as_ref()).map(|i| {
            i.layout(
                l.size.width - l.border.left - l.border.right - l.padding.left - l.padding.right,
                l.size.height - l.border.top - l.border.bottom - l.padding.top - l.padding.bottom,
                [
                    l.border.top + l.padding.top,
                    l.border.right + l.padding.right,
                    l.border.bottom + l.padding.bottom,
                    l.border.left + l.padding.left,
                ],
            )
        });
        let fin_ref: Option<&crate::layout::ifc::interface::FinalOutcome> = n
            .ifc
            .as_ref()
            .and_then(|c| c.final_outcome.as_ref())
            .or_else(|| inner_layout.as_ref().and_then(|il| il.outcome.as_ref()));
        let paragraph_index = fin_ref.map(|f| {
            self.out.paragraphs.push(f.paragraph.clone());
            self.out.paragraphs.len() - 1
        });
        let lbox = boxes.get(n.box_id);
        let owner = lbox.owner;
        let (first_baseline, last_baseline) = (
            n.baselines.first.map(|b| self.q.edge(b)),
            n.baselines.last.map(|b| self.q.edge(b)),
        );

        // FR-062–FR-065: overflow modes (the element the viewport took its value from is
        // `visible`), scrollport = padding box, scrollable overflow (padding-box frame in Taffy;
        // published in the fragment's border-box space like the other rects).
        let propagated = boxes.viewport_overflow.source == Some(n.box_id);
        let (ox, oy) = if propagated {
            (OverflowMode::Visible, OverflowMode::Visible)
        } else {
            (mode(lbox.style.overflow_x), mode(lbox.style.overflow_y))
        };
        // A text control scrolls its inner editing area: input never by the user's wheel, a
        // textarea like `overflow: auto`; the scrollport is the content box.
        let (ox, oy) = match &inner_layout {
            Some(il) if il.control.multiline => (
                if ox == OverflowMode::Visible {
                    OverflowMode::Auto
                } else {
                    ox
                },
                if oy == OverflowMode::Visible {
                    OverflowMode::Auto
                } else {
                    oy
                },
            ),
            // An open drop-down's popup lies outside the border box: nothing clips it.
            Some(il) if il.control.select.as_ref().is_some_and(|s| s.popup) => {
                (OverflowMode::Visible, OverflowMode::Visible)
            }
            Some(il)
                if il
                    .control
                    .select
                    .as_ref()
                    .is_some_and(|s| !s.popup && s.visible_rows > 1) =>
            {
                (ox, oy)
            }
            Some(_) => (OverflowMode::Hidden, OverflowMode::Hidden),
            None => (ox, oy),
        };
        let is_scroll_container = is_scroll_mode(ox) || is_scroll_mode(oy);
        let so = l.scrollable_overflow_rect.rect();
        let mut so_rect = {
            let (ol, ot) = (l.border.left, l.border.top);
            let (a, b) = (self.q.edge(so.left + ol), self.q.edge(so.top + ot));
            let (c, d) = (self.q.edge(so.right + ol), self.q.edge(so.bottom + ot));
            union(
                LayoutRect {
                    origin: LayoutPoint { x: a, y: b },
                    size: LayoutSize::from_edges(a, c, b, d),
                },
                padding_rect,
            )
        };
        let mut control_inner = None;
        if let Some(il) = &inner_layout {
            let mut ci = il.control.clone();
            ci.viewport = content_rect;
            ci.paragraph_index = paragraph_index;
            let text_extent = LayoutRect {
                origin: content_rect.origin,
                size: LayoutSize {
                    width: self.q.edge(ci.content_width),
                    height: self.q.edge(ci.content_height),
                },
            };
            // The scrollable overflow of the control is its content box united with the text.
            so_rect = union(content_rect, text_extent);
            control_inner = Some(Box::new(ci));
        }
        let clip_rect =
            (ox == OverflowMode::Clip || oy == OverflowMode::Clip).then_some(padding_rect);
        let position = lbox.style.position;
        let z_index = lbox.style.z_index;
        let box_id = n.box_id;
        let out_of_flow = n.out_of_flow;
        let dom_parent_box = if out_of_flow {
            let mut p = lbox.parent;
            while let Some(b) = p {
                // The nearest ancestor with a box fragment of its own (block-level or atomic).
                if boxes.get(b).is_block() || boxes.get(b).is_atomic_inline() {
                    break;
                }
                p = boxes.get(b).parent;
            }
            p
        } else {
            None
        };

        let grid = match n.table.as_deref() {
            Some(crate::layout::table::TableHost::Grid(d)) => Some(d),
            _ => None,
        };
        let collapsed = grid
            .and_then(|d| d.borders.as_ref())
            .map(|b| b.segments.clone())
            .filter(|s| !s.is_empty());
        let marker_symbol = match &lbox.kind {
            crate::layout::box_tree::BoxKind::Replaced(r) => match r.kind {
                crate::layout::replaced::ReplacedKind::MarkerSymbol { shape, .. } => {
                    // The box is `bullet + 2` wide; the bullet sits one pixel in, at the top.
                    let w = size.width.to_f64() as f32;
                    let bullet = (w - 2.0).max(0.0);
                    let bullet_y = n
                        .leaf
                        .as_ref()
                        .and_then(|l| l.marker_bullet_y)
                        .unwrap_or(0.0);
                    use crate::layout::replaced::MarkerShape;
                    let (x, y, width, height) = match shape {
                        MarkerShape::DisclosureOpen
                        | MarkerShape::DisclosureClosedLtr
                        | MarkerShape::DisclosureClosedRtl => {
                            // Triangle bounds measured against Chromium's inside marker;
                            // its advance is held separately by the replaced box width.
                            let fs = lbox.style.text.font_size;
                            let open = shape == MarkerShape::DisclosureOpen;
                            let width = fs * if open { 0.6875 } else { 0.5625 };
                            let height = fs * if open { 0.5625 } else { 0.6875 };
                            let x = if lbox.style.direction
                                == crate::layout::box_tree::Direction::Rtl
                            {
                                w - width
                            } else {
                                0.0
                            };
                            let y = size.height.to_f64() as f32
                                - fs * if open { 0.625 } else { 0.6875 };
                            (x, y, width, height)
                        }
                        _ => (1.0, bullet_y, bullet, bullet),
                    };
                    Some(super::MarkerSymbolBox {
                        shape,
                        bullet: LayoutRect {
                            origin: LayoutPoint {
                                x: self.q.edge(x),
                                y: self.q.edge(y),
                            },
                            size: LayoutSize {
                                width: self.q.edge(width),
                                height: self.q.edge(height),
                            },
                        },
                    })
                }
                _ => None,
            },
            _ => None,
        };
        let id = self.push(
            parent,
            rect.origin,
            size,
            FragmentKind::Box(BoxFragment {
                box_id,
                owner,
                padding_rect,
                content_rect,
                margins,
                border_widths: border,
                padding,
                paragraph_index,
                extras: BoxExtras {
                    // First and last baselines from the border-box top (research R-12).
                    first_baseline,
                    last_baseline,
                    is_scroll_container,
                    is_float: lbox.is_float() && !lbox.parent.is_some_and(|p| matches!(boxes.get(p).inner, crate::layout::box_tree::InnerDisplay::Flex | crate::layout::box_tree::InnerDisplay::Grid)),
                    // Horizontal RTL scrolls from its right edge; vertical modes are remapped
                    // by writing_mode::physical after this logical fragment is built.
                    scroll_from_end: (lbox.style.writing_mode == crate::layout::writing_mode::WritingMode::HorizontalTb && lbox.style.direction == crate::layout::box_tree::Direction::Rtl, false),
                    scrollport: is_scroll_container.then_some(if control_inner.is_some() { content_rect } else { padding_rect }),
                    scrollable_overflow: Some(so_rect),
                    overflow_x: ox,
                    overflow_y: oy,
                    clip_rect,
                    containing_block,
                    position,
                    z_index,
                    top_layer: lbox.style.top_layer,
                    anchor_node: if position == PositionValue::Fixed {
                        crate::layout::coordinator::oof::first_anchor(boxes, box_id)
                    } else {
                        None
                    },
                    fieldset_border_top: lbox.style.fieldset_border_top.map(|t| t.0),
                    table_cell: n.table_cell,
                    grid_tracks: grid_tracks_of(&n.grid_info),
                    collapsed_borders: collapsed,
                    control_inner,
                    table_box,
                    marker_symbol,
                    generated_image: matches!(&lbox.kind, crate::layout::box_tree::BoxKind::Replaced(r) if matches!(r.kind, crate::layout::replaced::ReplacedKind::Image(_)))
                        && matches!(owner, crate::layout::box_tree::BoxOwner::Pseudo(_, crate::layout::box_tree::PseudoKind::Before | crate::layout::box_tree::PseudoKind::After)),
                    ..Default::default()
                },
            }),
        );
        if let Some(d) = grid {
            let from = self.out.fragments.len();
            self.table_tracks(id, d);
            if let Some(pc) = piece {
                self.window_table_parts(id, from, pc);
                self.fit_row_groups(id, from, d);
            }
        }
        if let Some(b) = dom_parent_box {
            self.dom_parents.push((id, b));
        }
        if let Some((from, to, pi)) = splice {
            self.splice_descendants(id, from, to, pi, paragraph_index);
            return;
        }
        if position == PositionValue::Sticky
            && let Some(p) = parent
        {
            let sticky = self.sticky(id, box_id, p);
            if let FragmentKind::Box(b) = &mut self.out.fragments[id.0 as usize].kind {
                b.extras.sticky = Some(sticky);
            }
        }
        if let (Some(pi), Some(fin)) = (paragraph_index, fin_ref) {
            // Content-box offset in unrounded px, so line edges round in the box's space.
            let (ox, oy) = (l.border.left + l.padding.left, l.border.top + l.padding.top);
            let paragraph = fin.paragraph.clone();
            let by_tag = self.inline_boxes_by_tag(box_id);
            let piece_top = piece.map_or(0.0, |pc| pc.top);
            for (li, line) in fin.lines.iter().enumerate() {
                // In a piece, the lines whose top is in it (015 FRAG-2b).
                if let Some(pc) = piece
                    && !crate::layout::fragmentation::slice::line_in_piece(pc, oy + line.y)
                {
                    continue;
                }
                // A line `text-overflow` cut still reaches the whole text: the fragment keeps that extent so
                // the box's scrollable overflow (`scrollWidth`) does (the cut text fragment is shorter).
                let width = paragraph
                    .uncut_content_end(li)
                    .map_or(line.width, |end| line.width.max(end - line.x));
                let lr = self
                    .q
                    .rect(ox + line.x, oy + line.y - piece_top, width, line.height);
                let baseline = self.q.edge(line.baseline - line.y);
                let line_id = self.push(
                    Some(id),
                    lr.origin,
                    lr.size,
                    FragmentKind::Line(LineFragment {
                        paragraph_index: pi,
                        line_index: li,
                        baseline,
                        vertical: None,
                    }),
                );
                let text_pieces = paragraph.text_pieces(li);
                let rel_basis = l.size.width
                    - l.border.left
                    - l.border.right
                    - l.padding.left
                    - l.padding.right;
                self.inline_box_pieces(
                    line_id,
                    li,
                    line,
                    &text_pieces,
                    fin,
                    rel_basis,
                    &by_tag,
                    box_id,
                );
                for piece in text_pieces {
                    let Some(owner) = owner_node(piece.owner) else {
                        continue;
                    };
                    let pseudo = crate::layout::ifc::interface::owner_pseudo(piece.owner);
                    // A text fragment is the text's content area: the font's ascent and
                    // descent around the baseline (CSS Inline 3 §5, CSS 2.1 §10.6.1; whole
                    // pixels per 009's text metrics policy), not the line box's height. This
                    // is what `getClientRects()` reports for an inline box and what text
                    // contributes to scrollable overflow.
                    let top = piece.baseline - piece.ascent - line.y;
                    // `position: relative` on the inline boxes around the text moves the text
                    // after layout (FR-055); the line box and everything else stay where they are.
                    let (dx, dy) = line_relative_offset(
                        boxes,
                        n.box_id,
                        inline_relative_offset(
                            boxes,
                            n.box_id,
                            owner,
                            l.size.width
                                - l.border.left
                                - l.border.right
                                - l.padding.left
                                - l.padding.right,
                        ),
                    );
                    let tr = self.q.rect(
                        piece.x - line.x + dx,
                        top + dy,
                        piece.width,
                        piece.ascent + piece.descent,
                    );
                    let text_baseline = self.q.edge(piece.baseline - line.y) - tr.origin.y;
                    self.push(
                        Some(line_id),
                        tr.origin,
                        tr.size,
                        FragmentKind::Text(TextFragment {
                            owner,
                            pseudo,
                            paragraph_index: pi,
                            line_index: li,
                            text_range: piece.text_range.clone(),
                            baseline: text_baseline,
                        }),
                    );
                }
                self.decoration_fragments(
                    line_id,
                    li,
                    line,
                    &paragraph,
                    n.box_id,
                    l.size.width
                        - l.border.left
                        - l.border.right
                        - l.padding.left
                        - l.padding.right,
                );
            }
        }
        let shift = if n.ifc.is_some() {
            (l.border.left + l.padding.left, l.border.top + l.padding.top)
        } else {
            (0.0, 0.0)
        };
        let content_w =
            l.size.width - l.border.left - l.border.right - l.padding.left - l.padding.right;
        for &c in &n.children {
            if !host.nodes[c].out_of_flow {
                let s = if host.nodes[c].in_ifc {
                    let (dx, dy) = atomic_relative_offset(boxes, host.nodes[c].box_id, content_w);
                    (shift.0 + dx, shift.1 + dy)
                } else {
                    shift
                };
                self.parent_content_w = content_w;
                // A flow thread with a fragmentation outcome is sliced into fragmentainers (015 FRAG-2b).
                if host.nodes[c]
                    .flow_geom
                    .as_ref()
                    .is_some_and(|g| g.block_size > crate::layout::fragmentation::EPS)
                {
                    self.slice_flow_thread(c, id, idx);
                    continue;
                }
                match piece {
                    None => self.node(c, Some(id), None, s),
                    Some(pc) => {
                        use crate::layout::fragmentation::slice::{ChildSlice, child_slice};
                        let cn = &host.nodes[c];
                        // Breakable floats (015 MC-2) are split like blocks, also in inline content.
                        let splittable = (!cn.in_ifc || cn.frag_facts.breakable_float())
                            && !cn.frag_facts.monolithic
                            && matches!(boxes.get(cn.box_id).kind, BoxKind::BlockContainer { .. });
                        // On pages, monolithic content that does not fit is sliced across them (015 STAB-1, CSS Break 3 section
                        // 4.3, as the reference prints it): a piece per page, the whole box painted cut to each.
                        let sliced =
                            host.paged && cn.frag_facts.monolithic && !cn.in_ifc && !cn.out_of_flow;
                        let splittable = splittable || sliced;
                        let s = (s.0, s.1 - pc.top);
                        // A box whose floats continue below it has pieces down to their end (015 MC-2).
                        let floats = if splittable && !sliced {
                            self.float_bottom(c)
                        } else {
                            f32::NEG_INFINITY
                        };
                        let overflow = if host.paged && splittable && !sliced {
                            host.deep_bottom(c)
                        } else {
                            f32::NEG_INFINITY
                        };
                        let extent = cn.layout.size.height.max(floats).max(overflow);
                        match child_slice(
                            pc,
                            cn.layout.location.y + s.1 + pc.top,
                            extent,
                            splittable,
                        ) {
                            ChildSlice::Skip => {}
                            ChildSlice::Whole => self.node(c, Some(id), None, s),
                            ChildSlice::Piece(mut cp) => {
                                // A box is only broken where content follows the break: with nothing starting
                                // after the band's end it ends here (its overflow paints past the band), and a
                                // band holding no start of its content holds no piece of it.
                                let auto_height = cn.style.size.height.is_auto()
                                    && !cn.frag_facts.fc_item
                                    && !sliced;
                                let eps = crate::layout::fragmentation::EPS;
                                if auto_height
                                    && cp.top > 0.0
                                    && floats <= cp.top + eps
                                    && !self.content_from(c, cp.top)
                                {
                                    continue;
                                }
                                if auto_height
                                    && floats <= cp.bottom + eps
                                    && !self.content_from(c, cp.bottom)
                                {
                                    cp.bottom = extent.max(cp.bottom);
                                    cp.last_band = true;
                                }
                                if cp.top <= 0.0
                                    && cp.bottom >= extent - crate::layout::fragmentation::EPS
                                {
                                    self.node(c, Some(id), None, s);
                                } else {
                                    self.piece = Some(cp);
                                    self.node(c, Some(id), None, s);
                                }
                            }
                        }
                    }
                }
            }
        }
        if let (Some(d), Some(pc), Some(pi)) = (grid, piece, piece_info) {
            self.repeat_table_rows(idx, id, d, pc, pi);
        }
        if let Some(d) = grid {
            let from = self.out.fragments.len();
            self.table_segments(id, d);
            if let Some(pc) = piece {
                let originals: Vec<(LayoutPoint, LayoutSize, FragmentKind)> = self.out.fragments
                    [from..]
                    .iter()
                    .filter(|f| matches!(f.kind, FragmentKind::CollapsedBorderSegment(_)))
                    .map(|f| (f.origin, f.size, f.kind.clone()))
                    .collect();
                self.window_table_parts(id, from, pc);
                if let Some(pi) = piece_info {
                    self.repeat_table_segments(id, from, d, pc, pi, &originals);
                }
            }
        }
        // The out-of-flow boxes this box is the containing block of, in document order. A broken box (015
        // FRAG-2c): each is placed in flow-thread coordinates and belongs to the piece that holds its top
        // (split out-of-flow boxes are deferred).
        match (piece, piece_info) {
            (Some(pc), Some(pi)) => {
                for &h in &n.hoisted {
                    let top = host.nodes[h].layout.location.y;
                    let eps = crate::layout::fragmentation::EPS;
                    if (pi.index == 0 || top >= pc.top - eps)
                        && (top < pc.bottom - eps || pc.last_band)
                    {
                        self.node(h, Some(id), Some(id), (0.0, -pc.top));
                    }
                }
            }
            _ => {
                for &h in &n.hoisted {
                    self.node(h, Some(id), Some(id), (0.0, 0.0));
                }
            }
        }
        if let FragmentKind::Box(bf) = &mut self.out.fragments[id.0 as usize].kind
            && let Some(pi) = piece_info
        {
            bf.extras.piece = Some(pi);
            bf.extras.scrollable_overflow = Some(bf.padding_rect);
        }
        if let crate::layout::fragmentation::FragRole::Multicol { .. } = &n.frag_role {
            // The columns, not the flow-thread strip, are the container's content.
            let pr = match &self.out.fragments[id.0 as usize].kind {
                FragmentKind::Box(bf) => bf.padding_rect,
                _ => LayoutRect::default(),
            };
            let mut so = pr;
            let cols: Vec<FragmentId> = self.out.fragments[id.0 as usize]
                .children
                .iter()
                .copied()
                .filter(|c| {
                    matches!(
                        self.out.fragments[c.0 as usize].kind,
                        FragmentKind::Fragmentainer(_)
                    )
                })
                .collect();
            for c in cols {
                let f = &self.out.fragments[c.0 as usize];
                so = union(
                    so,
                    LayoutRect {
                        origin: f.origin,
                        size: f.size,
                    },
                );
            }
            if let FragmentKind::Box(bf) = &mut self.out.fragments[id.0 as usize].kind {
                bf.extras.scrollable_overflow = Some(so);
            }
        }
        // An inline root's out-of-flow boxes whose containing block is further up are hoisted
        // by that block, never built here.
    }

    /// `TableTrack` fragments of a table grid box (FR-081): column groups, columns, row groups, then
    /// rows, in the paint order of CSS 2.1 §17.5.1, as children of the table fragment before its cells.
    /// The repeated header (in every piece after the first) and footer (in every piece but the last) of a
    /// table grid broken across fragmentainers (015 FRAG-2c): copies of the header and footer rows' cells,
    /// the header at the piece's top (where the row struts left room), the footer after the piece's last row.
    /// The collapsed borders of the repeated header and footer copies of a broken table (015 PAGE-1): the
    /// segments of the header rows again at the piece's top, those of the footer rows after its last row, and the
    /// vertical lines of a piece that is not the last ending at its content (its last row or the footer copy).
    fn repeat_table_segments(
        &mut self,
        id: FragmentId,
        from: usize,
        d: &crate::layout::table::GridData,
        pc: crate::layout::fragmentation::slice::PieceReq,
        pi: super::FragmentPiece,
        originals: &[(LayoutPoint, LayoutSize, FragmentKind)],
    ) {
        let Some(r) = d.last.as_ref() else { return };
        if r.row_y.is_empty() {
            return;
        }
        let eps = crate::layout::fragmentation::EPS;
        let (header_rows, footer_rows) = if self.paged {
            (&d.page_header_rows, &d.page_footer_rows)
        } else {
            (&d.header_rows, &d.footer_rows)
        };
        let rows_in_piece: Vec<usize> = (0..r.row_y.len())
            .filter(|&k| {
                r.row_y[k] >= pc.top - eps && (r.row_y[k] < pc.bottom - eps || pc.last_band)
            })
            .collect();
        let row_end = |k: usize| r.row_y[k] + r.row_h[k];
        let band = |rows: &std::ops::Range<usize>| (r.row_y[rows.start], row_end(rows.end - 1));
        let mut copies: Vec<(f32, f32, f32)> = Vec::new();
        if pi.index > 0
            && r.repeat_header > 0.0
            && !header_rows.is_empty()
            && header_rows.end <= r.row_y.len()
        {
            let (y0, y1) = band(header_rows);
            copies.push((y0, y1, 0.0));
        }
        let footer_copied = !pi.last
            && r.repeat_footer > 0.0
            && !footer_rows.is_empty()
            && footer_rows.end <= r.row_y.len()
            && !rows_in_piece.iter().any(|k| footer_rows.contains(k));
        let end = rows_in_piece
            .iter()
            .map(|&k| row_end(k))
            .fold(0.0f32, f32::max);
        let mut content_end = end - pc.top;
        if footer_copied {
            let (f0, f1) = band(footer_rows);
            let shift = end + d.spacing.1 - pc.top - f0;
            copies.push((f0, f1, shift));
            content_end = f1 + shift;
        }
        for &(y0, y1, shift) in &copies {
            for (o, sz, kind) in originals {
                let FragmentKind::CollapsedBorderSegment(seg) = kind else {
                    continue;
                };
                let w = seg.width.to_f64() as f32;
                let (a, b) = (o.y.to_f64() as f32, (o.y + sz.height).to_f64() as f32);
                let (c0, c1) = (a.max(y0 - w), b.min(y1 + w));
                if c1 <= c0 + eps || (seg.horizontal && (a < y0 - w || b > y1 + w)) {
                    continue;
                }
                let rc = self.q.rect(
                    o.x.to_f64() as f32,
                    c0 + shift,
                    sz.width.to_f64() as f32,
                    c1 - c0,
                );
                self.push(Some(id), rc.origin, rc.size, kind.clone());
            }
        }
        if !pc.last_band && !rows_in_piece.is_empty() {
            let limit = self.q.edge(content_end);
            let base = self.out.document_origins[id.0 as usize];
            for k in from..self.out.fragments.len() {
                let f = &mut self.out.fragments[k];
                let FragmentKind::CollapsedBorderSegment(seg) = &f.kind else {
                    continue;
                };
                if f.parent != Some(id) || seg.horizontal {
                    continue;
                }
                let w = self.q.edge(seg.width.to_f64() as f32 / 2.0);
                if f.origin.y + f.size.height > limit + w {
                    f.size.height = if limit + w > f.origin.y {
                        limit + w - f.origin.y
                    } else {
                        LayoutUnit::ZERO
                    };
                }
                let o = f.origin;
                self.out.document_origins[k] = LayoutPoint {
                    x: base.x + o.x,
                    y: base.y + o.y,
                };
            }
        }
    }

    fn repeat_table_rows(
        &mut self,
        grid: usize,
        id: FragmentId,
        d: &crate::layout::table::GridData,
        pc: crate::layout::fragmentation::slice::PieceReq,
        pi: super::FragmentPiece,
    ) {
        let Some(r) = d.last.as_ref() else { return };
        let host = self.host;
        let rows_in_piece: Vec<usize> = (0..r.row_y.len())
            .filter(|&k| {
                r.row_y[k] >= pc.top - crate::layout::fragmentation::EPS
                    && (r.row_y[k] < pc.bottom - crate::layout::fragmentation::EPS || pc.last_band)
            })
            .collect();
        let mut copies: Vec<(usize, f32)> = Vec::new();
        let (header_rows, footer_rows) = if self.paged {
            (&d.page_header_rows, &d.page_footer_rows)
        } else {
            (&d.header_rows, &d.footer_rows)
        };
        if pi.index > 0 && r.repeat_header > 0.0 && !header_rows.is_empty() {
            for (k, &cell) in d.cells.iter().enumerate() {
                if header_rows.contains(&d.structure.cells[k].row) {
                    copies.push((cell, 0.0));
                }
            }
        }
        if !pi.last
            && r.repeat_footer > 0.0
            && !footer_rows.is_empty()
            && !rows_in_piece.iter().any(|k| footer_rows.contains(k))
        {
            let end = rows_in_piece
                .iter()
                .map(|&k| r.row_y[k] + r.row_h[k])
                .fold(0.0f32, f32::max);
            let f0 = r.row_y[footer_rows.start];
            let sp = d.spacing.1;
            for (k, &cell) in d.cells.iter().enumerate() {
                if footer_rows.contains(&d.structure.cells[k].row) {
                    copies.push((cell, end + sp - pc.top - f0));
                }
            }
        }
        if copies.is_empty() {
            return;
        }
        let _ = (grid, host);
        // The rows and row groups of the copies (015 STAB-1, D-REL04-1): their rectangles in this piece, for the
        // geometry of the first occurrence.
        let (left, right) = match (r.col_x.first(), r.col_x.last(), r.col_w.last()) {
            (Some(&a), Some(&b), Some(&w)) => (a, b + w),
            _ => (0.0, 0.0),
        };
        let mut rows: Vec<(usize, f32)> = Vec::new();
        for &(cell, dy) in &copies {
            if let Some(k) = d.cells.iter().position(|c| *c == cell)
                && !rows.iter().any(|(row, _)| *row == d.structure.cells[k].row)
            {
                rows.push((d.structure.cells[k].row, dy));
            }
        }
        let base = self.out.document_origins[id.0 as usize];
        let mut spans: Vec<(BoxId, f32, f32)> = Vec::new();
        for &(k, dy) in &rows {
            if let (Some(row), Some(&y), Some(&h)) = (d.rows.get(k), r.row_y.get(k), r.row_h.get(k))
            {
                spans.push((row.box_id, y + dy, y + dy + h));
            }
        }
        for (b, range) in &d.row_groups {
            if !range.is_empty() && range.clone().all(|k| rows.iter().any(|(row, _)| *row == k)) {
                let dy = rows
                    .iter()
                    .find(|(row, _)| *row == range.start)
                    .map_or(0.0, |x| x.1);
                spans.push((
                    *b,
                    r.row_y[range.start] + dy,
                    r.row_y[range.end - 1] + r.row_h[range.end - 1] + dy,
                ));
            }
        }
        for (b, y0, y1) in spans {
            let rc = self.q.rect(left, y0, right - left, y1 - y0);
            let rect = LayoutRect {
                origin: LayoutPoint {
                    x: base.x + rc.origin.x,
                    y: base.y + rc.origin.y,
                },
                size: rc.size,
            };
            self.out
                .maps
                .repeated_tracks
                .entry(self.boxes.get(b).owner.node())
                .or_default()
                .push((id, rect));
        }
        let saved = std::mem::replace(&mut self.cloning, true);
        for (cell, dy) in copies {
            self.node(cell, Some(id), None, (0.0, dy));
        }
        self.cloning = saved;
    }

    /// The fragmentainers of flow thread `flow` (a child of the multicol container fragment `container`)
    /// and the pieces of its content in each (015 FRAG-2b, `fragmentation::slice`).
    fn slice_flow_thread(&mut self, flow: usize, container: FragmentId, container_idx: usize) {
        let host = self.host;
        let n = &host.nodes[flow];
        let mut l = n.layout;
        let Some(g) = n.flow_geom.as_deref() else {
            return;
        };
        l.size.height = g.flow_height;
        let rtl = n.style.direction == taffy::Direction::Rtl;
        // Floats (and what overflows them) continue in further fragmentainers (015 MC-2).
        let floats = self.float_bottom(flow);
        let mut bands = g.fragmentainers(l.size.height.max(floats));
        // No fragmentainer after the last one in which content starts (overflow stays in that one).
        while bands.len() > 1
            && bands.last().is_some_and(|b| {
                floats <= b.block_offset + crate::layout::fragmentation::EPS
                    && !self.content_from(flow, b.block_offset)
            })
        {
            bands.pop();
        }
        for t in &g.tokens {
            if matches!(
                t.kind,
                crate::layout::fragmentation::BreakKind::PushedBox
                    | crate::layout::fragmentation::BreakKind::TruncatedMargin
            ) {
                self.truncated.insert(t.node);
            }
        }
        let count = bands.len();
        let step = g.column_width + g.gap;
        let column_x = |k: f32| {
            if rtl {
                l.location.x + l.size.width - g.column_width - k * step
            } else {
                l.location.x + k * step
            }
        };
        for band in bands {
            let k = band.index as f32;
            let r = self
                .q
                .rect(column_x(k), l.location.y, g.column_width, g.block_size);
            let offset_lu = self.q.edge(band.block_offset);
            let fc = self.push(
                Some(container),
                r.origin,
                r.size,
                FragmentKind::Fragmentainer(super::FragmentainerFragment {
                    index: band.index,
                    block_offset: offset_lu,
                    row_layouts: g.balance_iterations,
                }),
            );
            let last_band = band.index as usize + 1 == count;
            let top = band.block_offset;
            let bottom = if last_band {
                l.size.height.max(top)
            } else {
                top + band.block_size
            };
            self.piece = Some(crate::layout::fragmentation::slice::PieceReq {
                top,
                bottom,
                last_band,
            });
            self.node(flow, Some(fc), None, (-l.location.x, -l.location.y - top));
        }
        // Column rules (CSS Multicol 1 section 4.2): in the middle of each gap between two columns that hold
        // content, as tall as the columns.
        let rule_w = self
            .boxes
            .get(host.nodes[container_idx].box_id)
            .style
            .frag
            .column_rule_width;
        if rule_w > 0.0 && count > 1 {
            let rules: Vec<LayoutRect> = (0..count - 1)
                .map(|k| {
                    let (a, b) = (column_x(k as f32), column_x(k as f32 + 1.0));
                    let mid = if rtl {
                        b + g.column_width + g.gap / 2.0
                    } else {
                        a + g.column_width + g.gap / 2.0
                    };
                    self.q
                        .rect(mid - rule_w / 2.0, l.location.y, rule_w, g.block_size)
                })
                .collect();
            if let FragmentKind::Box(bf) = &mut self.out.fragments[container.0 as usize].kind {
                bf.extras.column_rules.extend(rules);
            }
        }
    }

    /// How far host node `idx`'s breakable floats reach below its border-box top (015 MC-2; `-inf`: none).
    fn float_bottom(&mut self, idx: usize) -> f32 {
        if let Some(b) = self.float_bottoms.get(&idx) {
            return *b;
        }
        let b = self.host.flow_overflow(idx);
        self.float_bottoms.insert(idx, b);
        b
    }

    /// Whether some content of host node `idx` (a line, or an in-flow child box) starts at or below `y`
    /// (its own border-box coordinates), looking into children that may be split (015 FRAG-2b).
    fn content_from(&self, idx: usize, y: f32) -> bool {
        use crate::layout::fragmentation::EPS;
        let n = &self.host.nodes[idx];
        let l = n.layout;
        let (ox, oy) = (l.border.left + l.padding.left, l.border.top + l.padding.top);
        let _ = ox;
        if let Some(f) = n.ifc.as_ref().and_then(|c| c.final_outcome.as_ref())
            && f.lines.iter().any(|line| oy + line.y >= y - EPS)
        {
            return true;
        }
        let shift = if n.ifc.is_some() { oy } else { 0.0 };
        n.children.iter().any(|&c| {
            let cn = &self.host.nodes[c];
            if cn.out_of_flow || cn.in_ifc {
                return false;
            }
            let top = cn.layout.location.y + shift;
            if top >= y - EPS {
                return true;
            }
            let splittable = !cn.frag_facts.monolithic
                && matches!(
                    self.boxes.get(cn.box_id).kind,
                    BoxKind::BlockContainer { .. }
                );
            // Monolithic content sliced across pages (015 STAB-1) is content down to its end.
            let sliced = self.host.paged && cn.frag_facts.monolithic && !cn.out_of_flow;
            // A box with a definite height is content down to its end (it takes up the space it is given).
            (splittable || sliced)
                && ((top + cn.layout.size.height > y + EPS
                    && (sliced
                        || !cn.style.size.height.is_auto()
                        || cn.frag_facts.fc_item
                        || self.content_from(c, y - top)))
                    || (self.paged && splittable && self.content_from(c, y - top)))
        })
    }

    fn table_tracks(&mut self, table: FragmentId, d: &crate::layout::table::GridData) {
        let Some(r) = d.last.as_ref() else { return };
        if r.row_y.is_empty() || r.col_x.is_empty() {
            return;
        }
        let (rows_top, rows_bottom) = (
            r.row_y[0],
            r.row_y[r.row_y.len() - 1] + r.row_h[r.row_h.len() - 1],
        );
        let (left, right) = (
            r.col_x[0],
            r.col_x[r.col_x.len() - 1] + r.col_w[r.col_w.len() - 1],
        );
        let col_span = |a: usize, b: usize| (r.col_x[a], r.col_x[b - 1] + r.col_w[b - 1]);
        let row_span = |a: usize, b: usize| (r.row_y[a], r.row_y[b - 1] + r.row_h[b - 1]);
        let push = |this: &mut Self,
                    kind: TableTrackKind,
                    box_id: BoxId,
                    x0: f32,
                    x1: f32,
                    y0: f32,
                    y1: f32| {
            let rc = this.q.rect(x0, y0, x1 - x0, y1 - y0);
            this.push(
                Some(table),
                rc.origin,
                rc.size,
                FragmentKind::TableTrack(TableTrack {
                    kind,
                    box_id: Some(box_id),
                    rect: rc,
                }),
            );
        };
        for (b, range) in &d.col_groups {
            if !range.is_empty() {
                let (x0, x1) = col_span(range.start, range.end);
                push(
                    self,
                    TableTrackKind::ColumnGroup,
                    *b,
                    x0,
                    x1,
                    rows_top,
                    rows_bottom,
                );
            }
        }
        for (c, info) in d.columns.iter().enumerate() {
            if let Some(b) = info.box_id {
                let (x0, x1) = col_span(c, c + 1);
                push(
                    self,
                    TableTrackKind::Column,
                    b,
                    x0,
                    x1,
                    rows_top,
                    rows_bottom,
                );
            }
        }
        for (b, range) in &d.row_groups {
            if !range.is_empty() {
                let (y0, y1) = row_span(range.start, range.end);
                push(self, TableTrackKind::RowGroup, *b, left, right, y0, y1);
            }
        }
        for (i, row) in d.rows.iter().enumerate() {
            let (y0, y1) = row_span(i, i + 1);
            push(self, TableTrackKind::Row, row.box_id, left, right, y0, y1);
            for pc in d
                .structure
                .cells
                .iter()
                .filter(|c| c.row == i && c.row_span > 1)
            {
                let (_, below) = row_span(i, i + 1);
                let (_, bottom) = row_span(pc.row, pc.row + pc.row_span);
                if bottom > below {
                    let (cx0, cx1) = col_span(pc.col, pc.col + pc.col_span);
                    push(
                        self,
                        TableTrackKind::RowOverhang,
                        row.box_id,
                        cx0,
                        cx1,
                        below,
                        bottom,
                    );
                }
            }
        }
    }

    /// `CollapsedBorderSegment` fragments (FR-080): the winners along the grid lines, each placed as the
    /// rectangle the border occupies (centred on its line, `width` thick), after the cells so that they
    /// paint over them.
    fn table_segments(&mut self, table: FragmentId, d: &crate::layout::table::GridData) {
        let (Some(r), Some(b)) = (d.last.as_ref(), d.borders.as_ref()) else {
            return;
        };
        if r.col_x.is_empty() || r.row_y.is_empty() {
            return;
        }
        let line_x = |i: usize| {
            if i < r.col_x.len() {
                r.col_x[i]
            } else {
                r.col_x[i - 1] + r.col_w[i - 1]
            }
        };
        let line_y = |i: usize| {
            if i < r.row_y.len() {
                r.row_y[i]
            } else {
                r.row_y[i - 1] + r.row_h[i - 1]
            }
        };
        for s in &b.segments {
            let w = s.width.to_f64() as f32;
            let (a, z) = (s.extent.0 as usize, s.extent.1 as usize);
            // At the table's outer corners the wider of the two borders that meet owns the corner
            // square: it reaches half the other one's width beyond the grid line, so the corner is
            // filled once (CSS 2.1 §17.6.2: borders are centred on the grid lines; the outer half
            // of the table edge belongs to the table). Interior lines never reach beyond their ends.
            let (cols, rows) = (r.col_x.len(), r.row_y.len());
            let wv_at = |row: usize, line: usize| b.v[row][line].map_or(0.0, |s| s.width);
            let wh_at = |line: usize, col: usize| b.h[line][col].map_or(0.0, |s| s.width);
            let rc = if s.horizontal {
                let (mut x0, mut x1) = (line_x(a), line_x(z));
                let line = s.grid_line as usize;
                // The widest vertical border meeting this line at grid line `x`.
                let wv_here = |x: usize| {
                    let above = if line > 0 { wv_at(line - 1, x) } else { 0.0 };
                    let below = if line < rows { wv_at(line, x) } else { 0.0 };
                    above.max(below)
                };
                if a == 0 && w > wv_here(0) {
                    x0 -= wv_here(0) / 2.0;
                }
                if z == cols && w > wv_here(cols) {
                    x1 += wv_here(cols) / 2.0;
                }
                let y = line_y(line);
                self.q.rect(x0, y - w / 2.0, x1 - x0, w)
            } else {
                let (mut y0, mut y1) = (line_y(a), line_y(z));
                let line = s.grid_line as usize;
                // The widest horizontal border meeting this line at grid line `y`.
                let wh_here = |y: usize| {
                    let left = if line > 0 { wh_at(y, line - 1) } else { 0.0 };
                    let right = if line < cols { wh_at(y, line) } else { 0.0 };
                    left.max(right)
                };
                let outer = line == 0 || line == cols;
                // A vertical segment that owns its junction reaches half the horizontal border's
                // width past the grid line: at the table's outer corners, and across a row that has
                // no segment of its own on this line (an empty row), so the border stays continuous.
                // Where the next row has a segment of its own, an opaque solid pair overlaps instead of
                // abutting, so a fractional grid line leaves no anti-aliased seam; other pairs abut.
                let opaque_self = s.style == super::BorderLineStyle::Solid && s.opaque;
                let joins = |n: &Option<crate::layout::table::borders::BorderSpec>| {
                    n.as_ref().is_none_or(|n| {
                        opaque_self && n.style == super::BorderLineStyle::Solid && n.opaque
                    })
                };
                let top_open = if a == 0 {
                    outer
                } else {
                    joins(&b.v[a - 1][line])
                };
                let bottom_open = if z == rows {
                    outer
                } else {
                    joins(&b.v[z][line])
                };
                if top_open && w >= wh_here(a) {
                    y0 -= wh_here(a) / 2.0;
                }
                if bottom_open && w >= wh_here(z) {
                    y1 += wh_here(z) / 2.0;
                }
                let x = line_x(line);
                self.q.rect(x - w / 2.0, y0, w, y1 - y0)
            };
            self.push(
                Some(table),
                rc.origin,
                rc.size,
                FragmentKind::CollapsedBorderSegment(
                    crate::layout::fragment::CollapsedBorderSegment {
                        origin_owner: Some(self.boxes.get(s.origin_box).owner.key()),
                        ..*s
                    },
                ),
            );
        }
    }

    /// The viewport's scroll geometry (FR-064, FR-065): the initial containing block united with
    /// the root's scrollable overflow and with the out-of-flow boxes hoisted to the root other than
    /// `fixed` ones (which are anchored to the viewport and extend nothing).
    fn viewport_scroll(&mut self, root: FragmentId) -> ViewportScroll {
        viewport_scroll_at(&self.out, self.boxes, root)
    }
}

/// The viewport's scroll geometry of a built tree (FR-062–FR-065), after its root fragment.
pub(crate) fn viewport_scroll_of(tree: &FragmentTree, boxes: &BoxTree) -> ViewportScroll {
    match tree.root {
        Some(r) => viewport_scroll_at(tree, boxes, r),
        None => ViewportScroll::of_viewport(tree.viewport),
    }
}

fn viewport_scroll_at(tree: &FragmentTree, boxes: &BoxTree, root: FragmentId) -> ViewportScroll {
    let bf = |id: FragmentId| match &tree.get(id).kind {
        FragmentKind::Box(b) => Some(b),
        _ => None,
    };
    let doc_rect = |id: FragmentId| tree.document_rect(id).rect;
    let base = ViewportScroll::of_viewport(tree.viewport);
    let v = boxes.viewport_overflow;
    let mut so = base.scrollport;
    let root_origin = doc_rect(root).origin;
    if let Some(b) = bf(root) {
        let r = b.extras.scrollable_overflow.unwrap_or_default();
        so = union(so, translate(r, root_origin));
        so = union(so, doc_rect(root));
    }
    let hoisted: Vec<FragmentId> = tree
        .get(root)
        .children
        .iter()
        .copied()
        .filter(|c| bf(*c).is_some_and(|b| b.extras.containing_block.is_some()))
        .collect();
    for h in hoisted {
        let Some(b) = bf(h) else { continue };
        if b.extras.position == PositionValue::Fixed {
            continue;
        }
        let mut r = doc_rect(h);
        if !b.extras.is_scroll_container
            && let Some(o) = b.extras.scrollable_overflow
        {
            r = union(r, translate(o, doc_rect(h).origin));
        }
        so = union(so, r);
    }
    // Overflow before the scroll origin (above or to the left of the ICB origin, for a left-to-right
    // horizontal root) is unreachable (CSS Overflow 3 §2.2). The scroll origin is the block-start and
    // inline-start corner: in `vertical-rl` the block-start side is the right one (015 WM-1).
    let root_style = boxes.root.map(|r| &boxes.get(r).style);
    let rtl = root_style.is_some_and(|s| s.direction == crate::layout::box_tree::Direction::Rtl);
    let wm = root_style.map_or(
        crate::layout::writing_mode::WritingMode::HorizontalTb,
        |s| s.writing_mode,
    );
    use crate::layout::writing_mode::WritingMode as W;
    // (left start, top start, right start, bottom start): which physical edges are start edges.
    let (sl, st, sr, sb) = match wm {
        W::HorizontalTb => (!rtl, true, rtl, false),
        W::VerticalRl | W::SidewaysRl => (false, !rtl, true, rtl),
        W::VerticalLr => (true, !rtl, false, rtl),
        W::SidewaysLr => (true, rtl, false, !rtl),
    };
    let vp = base.scrollport;
    let mut left = so.origin.x;
    let mut top = so.origin.y;
    let mut right = so.origin.x + so.size.width;
    let mut bottom = so.origin.y + so.size.height;
    if sl {
        left = left.max(vp.origin.x);
    }
    if st {
        top = top.max(vp.origin.y);
    }
    if sr {
        right = right.min(vp.origin.x + vp.size.width);
    }
    if sb {
        bottom = bottom.min(vp.origin.y + vp.size.height);
    }
    // The horizontal-tb rtl case keeps its previous behaviour: overflow to the right was never cut.
    if wm == W::HorizontalTb && rtl {
        right = so.origin.x + so.size.width;
    }
    ViewportScroll {
        source: v.source,
        overflow_x: mode(v.x),
        overflow_y: mode(v.y),
        scrollport: base.scrollport,
        scrollable_overflow: LayoutRect {
            origin: LayoutPoint { x: left, y: top },
            size: LayoutSize::from_edges(left, right, top, bottom),
        },
    }
}

/// Returns the tree and whether any value needed the non-finite/overflow rule.
pub(crate) fn build_fragments(
    boxes: &BoxTree,
    host: &HostTree,
    root: usize,
    viewport: Viewport,
    prev: Option<&FragmentTree>,
) -> (FragmentTree, bool) {
    let mut b = Build {
        boxes,
        host,
        out: FragmentTree::empty(viewport),
        q: QuantizeSink::default(),
        dom_parents: Vec::new(),
        parent_content_w: 0.0,
        piece: None,
        piece_counts: HashMap::new(),
        truncated: Default::default(),
        paged: false,
        cloning: false,
        prev,
        float_bottoms: HashMap::new(),
    };
    b.out.viewport_scroll.source = boxes.viewport_overflow.source;
    // Size the big tables once from the previous tree (015 PERF-2): growing them by doubling every pass
    // allocated and freed ~20 MiB of blocks per update on the medium page, which glibc's heap kept. The
    // capacity is a power of two, so consecutive passes ask for the same block sizes (a hole freed by
    // one pass fits the next; sizes drifting by a few elements made glibc's heap grow without bound).
    if let Some(p) = prev {
        b.out
            .fragments
            .reserve((p.fragments.len() + 16).next_power_of_two());
        b.out
            .decorations
            .reserve((p.decorations.len() + 16).next_power_of_two());
        b.out
            .paragraphs
            .reserve((p.paragraphs.len() + 16).next_power_of_two());
        b.out
            .node_to_fragments
            .reserve(p.node_to_fragments.len() + 16);
        b.out
            .maps
            .box_to_fragments
            .reserve(p.maps.box_to_fragments.len() + 16);
    }
    b.node(root, None, None, (0.0, 0.0));
    b.out.maps.node_to_boxes = boxes.node_to_boxes.clone();
    b.out.root = Some(FragmentId(0));
    for (frag, parent_box) in std::mem::take(&mut b.dom_parents) {
        let target = b
            .out
            .maps
            .box_to_fragments
            .get(&parent_box)
            .and_then(|v| v.first())
            .copied();
        if let FragmentKind::Box(bf) = &mut b.out.fragments[frag.0 as usize].kind {
            bf.extras.dom_parent = target;
        }
    }
    b.out.viewport_scroll = b.viewport_scroll(FragmentId(0));
    stitch_split_pieces(&mut b.out, boxes);
    (b.out, b.q.non_finite)
}

/// The boxes of an element split around nested spanners (015 MC-2, `FragStyle::split`) are pieces of one
/// box: their fragments (whole, or column pieces) are numbered in order across them, each placed at the sum
/// of the earlier pieces' heights in a whole box as tall as all of them (what the reference's box model
/// and `getClientRects()` report, and what the scene paints a piece as, cut to the piece).
fn stitch_split_pieces(out: &mut FragmentTree, boxes: &BoxTree) {
    let mut groups: Vec<(BoxOwner, Vec<usize>)> = Vec::new();
    let mut at: HashMap<BoxOwner, usize> = HashMap::new();
    for (i, f) in out.fragments.iter().enumerate() {
        let FragmentKind::Box(b) = &f.kind else {
            continue;
        };
        if b.extras.piece.is_some_and(|p| p.repeated)
            || boxes.get(b.box_id).style.frag.split.is_none()
        {
            continue;
        }
        let k = *at.entry(b.owner).or_insert_with(|| {
            groups.push((b.owner, Vec::new()));
            groups.len() - 1
        });
        groups[k].1.push(i);
    }
    for (_, ids) in groups {
        let total = ids
            .iter()
            .fold(LayoutUnit::ZERO, |a, i| a + out.fragments[*i].size.height);
        let mut offset = LayoutUnit::ZERO;
        let n = ids.len();
        for (k, i) in ids.into_iter().enumerate() {
            let h = out.fragments[i].size.height;
            if let FragmentKind::Box(bf) = &mut out.fragments[i].kind {
                bf.extras.piece = Some(super::FragmentPiece {
                    index: k as u32,
                    last: k + 1 == n,
                    block_offset: offset,
                    whole_block_size: total,
                    repeated: false,
                    split: true,
                });
                bf.extras.scrollable_overflow = Some(bf.padding_rect);
            }
            offset = offset + h;
        }
    }
}

/// One page of a paged layout (015 PAGE-1, `crate::layout::fragmentation::paged`): the root element's piece in band `band`
/// of the page flow `flow` (a host node whose only child is the root element's), in page-area coordinates; the
/// tree's viewport is the page area (the reference paints the root background over the page area only). `piece_counts` carries the pieces built on earlier pages (the
/// next piece's index per box) from page to page. The out-of-flow boxes the page flow holds (whose containing block
/// is the initial containing block) go to the page that holds their top; `fixed` ones repeat on every page, placed
/// against the page area.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_page(
    boxes: &BoxTree,
    host: &HostTree,
    flow: usize,
    band: crate::layout::fragmentation::Fragmentainer,
    last_band: bool,
    page: &crate::layout::fragmentation::paged::PageBox,
    viewport: Viewport,
    piece_counts: &mut HashMap<BoxId, u32>,
) -> (FragmentTree, bool) {
    use crate::layout::fragmentation::slice::PieceReq;
    let mut b = Build {
        boxes,
        host,
        out: FragmentTree::empty(viewport),
        q: QuantizeSink::default(),
        dom_parents: Vec::new(),
        parent_content_w: 0.0,
        piece: None,
        piece_counts: std::mem::take(piece_counts),
        truncated: Default::default(),
        paged: true,
        cloning: false,
        prev: None,
        float_bottoms: HashMap::new(),
    };
    b.out.viewport_scroll.source = boxes.viewport_overflow.source;
    let n = &host.nodes[flow];
    if let Some(g) = n.flow_geom.as_deref() {
        for t in &g.tokens {
            if matches!(
                t.kind,
                crate::layout::fragmentation::BreakKind::PushedBox
                    | crate::layout::fragmentation::BreakKind::TruncatedMargin
            ) {
                b.truncated.insert(t.node);
            }
        }
    }
    let flow_h = n.flow_geom.as_deref().map_or(n.layout.size.height, |g| {
        g.flow_height.max(n.layout.size.height)
    });
    let Some(&root) = n.children.first() else {
        *piece_counts = b.piece_counts;
        return (b.out, false);
    };
    let rl = host.nodes[root].layout;
    let top = band.block_offset;
    let bottom = if last_band {
        flow_h.max(top + band.block_size)
    } else {
        top + band.block_size
    };
    let rel_top = (top - rl.location.y).max(0.0);
    let rel_bottom = bottom - rl.location.y;
    let extent = rl
        .size
        .height
        .max(b.float_bottom(root))
        .max(host.deep_bottom(root));
    let whole =
        rel_top <= 0.0 && (last_band || rel_bottom >= extent - crate::layout::fragmentation::EPS);
    if !whole {
        b.piece = Some(PieceReq {
            top: rel_top,
            bottom: if last_band {
                extent.max(rel_top)
            } else {
                rel_bottom.max(rel_top)
            },
            last_band,
        });
    }
    b.node(root, None, None, (0.0, -top));
    b.out.root = Some(FragmentId(0));
    // The out-of-flow boxes of the initial containing block (the page flow claims them).
    let root_y_in_area = if whole {
        rl.location.y - top
    } else {
        (rl.location.y - top).max(0.0)
    };
    let area_h = page.content_height();
    let eps = crate::layout::fragmentation::EPS;
    for &h in &n.hoisted {
        let hn = &host.nodes[h];
        let lb = boxes.get(hn.box_id);
        let fixed = lb.style.position == PositionValue::Fixed;
        let hy = hn.layout.location.y;
        let shift_x = -rl.location.x;
        if fixed {
            // Repeated on every page, against the page area (`bottom`-anchored ones against its bottom).
            let bottom_anchored = matches!(lb.style.inset.top, InsetValue::Auto)
                && !matches!(lb.style.inset.bottom, InsetValue::Auto);
            let y = if bottom_anchored {
                hy - n.layout.size.height + area_h
            } else {
                hy
            };
            b.node(
                h,
                Some(FragmentId(0)),
                Some(FragmentId(0)),
                (shift_x, y - hy - root_y_in_area),
            );
        } else if hy >= top - eps && (hy < bottom - eps || last_band) {
            b.node(
                h,
                Some(FragmentId(0)),
                Some(FragmentId(0)),
                (shift_x, -top - root_y_in_area),
            );
        }
    }
    b.out.maps.node_to_boxes = boxes.node_to_boxes.clone();
    for (frag, parent_box) in std::mem::take(&mut b.dom_parents) {
        let target = b
            .out
            .maps
            .box_to_fragments
            .get(&parent_box)
            .and_then(|v| v.first())
            .copied();
        if let FragmentKind::Box(bf) = &mut b.out.fragments[frag.0 as usize].kind {
            bf.extras.dom_parent = target;
        }
    }
    b.out.viewport_scroll = b.viewport_scroll(FragmentId(0));
    stitch_split_pieces(&mut b.out, boxes);
    *piece_counts = std::mem::take(&mut b.piece_counts);
    (b.out, b.q.non_finite)
}

/// The pages of a laid-out page flow (015 PAGE-1): its fragmentainers down to the last one in which content
/// starts (overflow stays in that one; floats continue into further ones), as `slice_flow_thread` cuts columns.
pub(crate) fn page_bands(
    boxes: &BoxTree,
    host: &HostTree,
    flow: usize,
) -> Vec<crate::layout::fragmentation::Fragmentainer> {
    let mut b = Build {
        boxes,
        host,
        out: FragmentTree::empty(host.viewport),
        q: QuantizeSink::default(),
        dom_parents: Vec::new(),
        parent_content_w: 0.0,
        piece: None,
        piece_counts: HashMap::new(),
        truncated: Default::default(),
        paged: true,
        cloning: false,
        prev: None,
        float_bottoms: HashMap::new(),
    };
    let n = &host.nodes[flow];
    let Some(g) = n.flow_geom.as_deref() else {
        return Vec::new();
    };
    let floats = b.float_bottom(flow);
    // Fixed-height ancestors may have in-flow descendants continuing beyond their border boxes.
    let mut bands = g.fragmentainers(
        g.flow_height
            .max(n.layout.size.height)
            .max(floats)
            .max(host.deep_bottom(flow)),
    );
    while bands.len() > 1
        && bands.last().is_some_and(|l| {
            floats <= l.block_offset + crate::layout::fragmentation::EPS
                && !b.content_from(flow, l.block_offset)
        })
    {
        bands.pop();
    }
    bands
}
