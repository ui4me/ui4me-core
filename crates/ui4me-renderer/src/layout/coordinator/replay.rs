//! owner: 008-layout-engine — formatting-context replay (L-PERF-b3, beta B2)
//!
//! Layout is a pure function of the box tree, the viewport and the font environment. Taffy's per-node
//! `Cache` and the node results (`layout`, baselines, inline final outcomes) of a subtree depend only
//! on the subtree and on the `LayoutInput` its parent hands it, as long as nothing outside reaches in:
//! no block formatting context is shared with the parent (flex and grid items, atomic inlines and
//! table cells establish their own and are laid out through `compute_child_layout`), no out-of-flow
//! box escapes it, and no replaced box sizes itself from the width of an enclosing container
//! (`blocks_replay`). The caller passes the previous pass only while the viewport and the font and
//! resource epochs are unchanged.
//!
//! A root R of the last pass whose box subtree is equal (same box ids, owners, kinds and styles) gets
//! its old results back on its new host nodes before the layout runs. When its parent asks for an
//! input R has answered before, Taffy answers from R's cache and never descends; when it asks for
//! another, R lays out as usual (with its descendants' seeded caches). Either way the result is the
//! one a fresh layout computes. The fragments are built from the host tree as always.
//!
//! A table cell is laid out by its table twice per pass (a measure without a height, then the final
//! layout in its row's height) and gets its top padding changed in between (the vertical-align
//! offset, `table/layout.rs`). Its own results are therefore not carried (only its descendants'):
//! the cell answers its intrinsic-width measures and lays out again, its content from the seeds.

use super::host_tree::{HostNode, HostTree};
use crate::layout::box_tree::{BoxId, BoxKind, BoxTag, BoxTree, InnerDisplay, TableTag};
use crate::layout::ifc::container::InlineContainer;
use taffy::{Baselines, Cache, Layout, OofCandidates};

/// What a pass keeps of its host tree for the next pass's replay.
pub struct HostSnapshot {
    nodes: Vec<SnapNode>,
    /// Host node of each box id (`u32::MAX`: none).
    box_to_host: Vec<u32>,
    /// How many roots this pass seeded from the previous one.
    pub(crate) seeded: usize,
}

impl HostSnapshot {
    /// Host nodes of the pass.
    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

impl std::fmt::Debug for HostSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostSnapshot")
            .field("nodes", &self.nodes.len())
            .field("seeded", &self.seeded)
            .finish()
    }
}

/// The results of one host node, and its inline container (replay compares its inputs).
struct SnapNode {
    box_id: BoxId,
    subtree_end: usize,
    cache: Cache,
    layout: Layout,
    baselines: Baselines,
    atomic_oof: OofCandidates,
    grid_info: taffy::DetailedLayoutInfo<String>,
    leaf_baseline: Option<f32>,
    ifc: Option<InlineContainer>,
    cell_memo: Option<crate::layout::table::CellMemo>,
    /// The used border widths (a collapsed-border cell's are resolved from its neighbours, not its box).
    border: [(usize, u64); 4],
}

/// A used border width as plain data (`LengthPercentage` holds a calc pointer, which is not `Send`).
fn border_key(r: &taffy::Rect<taffy::LengthPercentage>) -> [(usize, u64); 4] {
    let k = |l: taffy::LengthPercentage| {
        let raw = l.into_raw();
        if raw.is_plain_calc() {
            (raw.tag(), raw.calc_value() as usize as u64)
        } else {
            (raw.tag(), u64::from(raw.value().to_bits()))
        }
    };
    [k(r.top), k(r.right), k(r.bottom), k(r.left)]
}

fn host_index(nodes: impl Iterator<Item = BoxId>, box_count: usize) -> Vec<u32> {
    let mut map = vec![u32::MAX; box_count];
    for (i, b) in nodes.enumerate() {
        // The first node of a box: the stand-in parent of a floated root (`push_icb_parent`) comes last.
        if let Some(slot) = map.get_mut(b.0 as usize)
            && *slot == u32::MAX
        {
            *slot = i as u32;
        }
    }
    map
}

impl HostSnapshot {
    pub(crate) fn of(tree: HostTree, box_count: usize, seeded: usize) -> HostSnapshot {
        let box_to_host = host_index(tree.nodes.iter().map(|n| n.box_id), box_count);
        let nodes = tree
            .nodes
            .into_iter()
            .map(|n| SnapNode {
                box_id: n.box_id,
                subtree_end: n.subtree_end,
                cache: n.cache,
                layout: n.layout,
                baselines: n.baselines,
                atomic_oof: n.atomic_oof,
                grid_info: n.grid_info,
                leaf_baseline: n.leaf_baseline,
                ifc: n.ifc,
                cell_memo: n.cell_memo,
                border: border_key(&n.style.border),
            })
            .collect();
        HostSnapshot {
            nodes,
            box_to_host,
            seeded,
        }
    }
}

/// A host node whose layout may read something outside its subtree, or whose state replay does not
/// carry: tables (their grid data), out-of-flow boxes and boxes with an inline containing block,
/// replaced boxes (a ratio-only one reads the width of the enclosing container; a text control keeps
/// its shaped value outside the results) and intrinsic-keyword widths.
fn blocks_replay(n: &HostNode) -> bool {
    n.table.is_some() || n.out_of_flow || !n.inline_oof.is_empty() || n.inline_cb_owner.is_some() || n.leaf.is_some() || n.width_kw.is_some() || n.min_width_kw.is_some()
        // A fragmentation root keeps its geometry and break tokens outside the results (015 FRAG-2b).
        || n.frag_role != crate::layout::fragmentation::FragRole::None
        // A float is placed in the block formatting context it is part of: a block that takes part in
        // its parent's context answers from its cache without placing its floats there again, so a root
        // laid out again over seeded descendants would lose them (found by the randomized harness,
        // 015 PERF-1).
        || n.style.float != taffy::Float::None
        // An orthogonal flow root (015 WM-1) keeps its own-frame overflow outside the results.
        || n.outer.is_some()
}

/// A final-layout request no layout makes (NaN known sizes), in the cache mode of the table's requests
/// to a cell (content-box sizing, no collapsible margins): storing it evicts the cell's final-layout
/// entry and keeps its measures. A cache in another mode is cleared by the store instead (still exact).
pub(crate) const UNMATCHABLE_FINAL: taffy::LayoutInput = taffy::LayoutInput {
    run_mode: taffy::RunMode::PerformLayout,
    sizing_mode: taffy::SizingMode::ContentSize,
    axis: taffy::RequestedAxis::Both,
    known_dimensions: taffy::Size {
        width: Some(f32::NAN),
        height: Some(f32::NAN),
    },
    known_dimensions_are_definite: taffy::Size {
        width: true,
        height: true,
    },
    parent_size: taffy::Size {
        width: None,
        height: None,
    },
    available_space: taffy::Size {
        width: taffy::AvailableSpace::MaxContent,
        height: taffy::AvailableSpace::MaxContent,
    },
    vertical_margins_are_collapsible: taffy::Line::FALSE,
};

fn is_cell(boxes: &BoxTree, b: BoxId) -> bool {
    matches!(boxes.get(b).tag, BoxTag::Table(TableTag::Cell { .. }))
}

/// Box `b` is laid out by its parent through `compute_child_layout`, without a shared block
/// formatting context: an in-flow flex or grid item, an atomic inline (not a float) placed by its
/// inline root, or a table cell (module docs).
///
/// With `plain_blocks` (no float and no multi-column flow thread in either tree: nothing then reaches
/// from a sibling into a block child, and every other input of its layout is in Taffy's cache key) an
/// in-flow block of a block container is one too (F1004): a long log or chat of plain rows re-lays out
/// only the rows that changed instead of every row on every update.
fn replay_root(boxes: &BoxTree, b: BoxId, plain_blocks: bool) -> bool {
    if is_cell(boxes, b) {
        return true;
    }
    let lb = boxes.get(b);
    if lb.is_out_of_flow() || lb.is_float() || matches!(lb.tag, BoxTag::Table(_)) {
        return false;
    }
    if matches!(lb.kind, BoxKind::InlineBlock { .. }) {
        return true;
    }
    lb.is_block()
        && lb.parent.is_some_and(|p| {
            let p = boxes.get(p);
            matches!(p.inner, InnerDisplay::Flex | InnerDisplay::Grid)
                || (plain_blocks
                    && matches!(p.kind, BoxKind::BlockContainer { .. })
                    && matches!(p.inner, InnerDisplay::Flow | InnerDisplay::FlowRoot)
                    && matches!(lb.kind, BoxKind::BlockContainer { .. }))
        })
}

/// Neither tree has a float or a multi-column flow thread (see [`replay_root`]).
fn plain_blocks(a: &BoxTree, b: &BoxTree) -> bool {
    let plain = |t: &BoxTree| {
        t.boxes
            .iter()
            .all(|x| !x.is_float() && x.tag != BoxTag::FlowThread)
    };
    plain(a) && plain(b)
}

/// Equal box subtrees: same ids, owners, kinds, tags, styles and children (the root's parent aside).
fn same_box_subtree(a: &BoxTree, b: &BoxTree, id: BoxId, root: bool) -> bool {
    let (x, y) = (a.get(id), b.get(id));
    x.owner == y.owner
        && x.tag == y.tag
        && x.inner == y.inner
        && x.atomic_inline == y.atomic_inline
        && x.list_item == y.list_item
        && x.children == y.children
        && (root || x.parent == y.parent)
        && x.kind == y.kind
        && (std::sync::Arc::ptr_eq(&x.style, &y.style) || *x.style == *y.style)
        && x.children.iter().all(|c| same_box_subtree(a, b, *c, false))
}

/// The previous containers of the inline roots `boxes` copied from `old_boxes` with the same ids together
/// with their parent (`BoxTree::same_ids`), results dropped (`InlineContainer::fresh_copy`).
pub(crate) fn ifc_seeds(
    boxes: &BoxTree,
    old_boxes: &BoxTree,
    old: &HostSnapshot,
) -> std::collections::HashMap<BoxId, InlineContainer> {
    let mut out = std::collections::HashMap::new();
    if boxes.same_ids.is_empty() || old_boxes.document != boxes.document {
        return out;
    }
    // Sized once (015 PERF-2): an inline container is large and the map rehashed several times a pass.
    out.reserve(old.nodes.iter().filter(|n| n.ifc.is_some()).count());
    for &(start, end) in &boxes.same_ids {
        for i in start + 1..end {
            let id = BoxId(i);
            if !boxes.get(id).is_inline_root() {
                continue;
            }
            let Some(&oh) = old.box_to_host.get(i as usize) else {
                continue;
            };
            if oh == u32::MAX {
                continue;
            }
            if let Some(c) = old.nodes[oh as usize]
                .ifc
                .as_ref()
                .filter(|c| c.root == id)
                .and_then(InlineContainer::fresh_copy)
            {
                out.insert(id, c);
            }
        }
    }
    out
}

/// Seed every replayable root of `tree` (outermost first) from `old`; returns how many were seeded.
pub(crate) fn replay(
    tree: &mut HostTree,
    boxes: &BoxTree,
    old_boxes: &BoxTree,
    old: &HostSnapshot,
) -> usize {
    let Some(root) = boxes.root else { return 0 };
    if old_boxes.document != boxes.document {
        return 0;
    }
    let new_map = host_index(tree.nodes.iter().map(|n| n.box_id), boxes.boxes.len());
    let mut seeded = 0;
    let plain = plain_blocks(boxes, old_boxes);
    let mut stack = vec![root];
    while let Some(b) = stack.pop() {
        let h = new_map[b.0 as usize];
        if h != u32::MAX
            && b != root
            && replay_root(boxes, b, plain)
            && seed(tree, boxes, old_boxes, old, b, h as usize, plain)
        {
            seeded += 1;
            continue;
        }
        stack.extend(boxes.get(b).children.iter().copied());
    }
    seeded
}

fn seed(
    tree: &mut HostTree,
    boxes: &BoxTree,
    old_boxes: &BoxTree,
    old: &HostSnapshot,
    b: BoxId,
    h: usize,
    plain: bool,
) -> bool {
    let i = b.0 as usize;
    let Some(&oh) = old.box_to_host.get(i) else {
        return false;
    };
    if oh == u32::MAX || i >= old_boxes.boxes.len() {
        return false;
    }
    let oh = oh as usize;
    // The root's own host node is also built from its parent's inner display, table role and legacy
    // `-webkit-` alignment: they must be the same, and the root must have been one in the old pass.
    let (Some(np), Some(op)) = (
        boxes.get(b).parent.map(|p| boxes.get(p)),
        old_boxes.get(b).parent.map(|p| old_boxes.get(p)),
    ) else {
        return false;
    };
    // The parent's writing mode decides whether the root is an orthogonal flow (015 WM-1).
    if np.inner != op.inner
        || np.tag != op.tag
        || np.kind != op.kind
        || np.style.webkit_align
        || op.style.webkit_align
        || np.style.writing_mode != op.style.writing_mode
        || !replay_root(old_boxes, b, plain)
    {
        return false;
    }
    if boxes.viewport_overflow.source == Some(b) || old_boxes.viewport_overflow.source == Some(b) {
        return false;
    }
    let (end, oend) = (tree.nodes[h].subtree_end, old.nodes[oh].subtree_end);
    if end - h != oend - oh || !same_box_subtree(boxes, old_boxes, b, true) {
        return false;
    }
    for k in 0..end - h {
        let (n, o) = (&tree.nodes[h + k], &old.nodes[oh + k]);
        if n.box_id != o.box_id || blocks_replay(n) || border_key(&n.style.border) != o.border {
            return false;
        }
        match (&n.ifc, &o.ifc) {
            (None, None) => {}
            (Some(a), Some(c)) if a.same_input(c) => {}
            _ => return false,
        }
    }
    // A cell takes its whole cache with its memo (015 PERF-1): the table uses the memo's height measure
    // instead of a measuring layout and keeps the final-layout entry only while the top padding it gives
    // the cell is the one the entry was computed with (`table/layout.rs`). Its descendants take everything.
    if is_cell(boxes, b) {
        tree.nodes[h].cell_memo = old.nodes[oh].cell_memo;
    }
    for k in 0..end - h {
        let (n, o) = (&mut tree.nodes[h + k], &old.nodes[oh + k]);
        n.cache = o.cache.clone();
        n.layout = o.layout;
        n.seed_layout = Some(o.layout);
        n.baselines = o.baselines;
        n.atomic_oof = o.atomic_oof.clone();
        n.grid_info = o.grid_info.clone();
        n.leaf_baseline = o.leaf_baseline;
        if let (Some(a), Some(c)) = (&mut n.ifc, &o.ifc) {
            a.adopt_results(c);
        }
    }
    true
}
