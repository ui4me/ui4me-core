//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! The Taffy host tree (plan "Component slice approach"; AD-12, AD-13): one Taffy node per
//! block-level box on an owned arena, driven through Taffy's low-level traits
//! (`TraversePartialTree`, `LayoutPartialTree`, `CacheTree`, `LayoutBlockContainer`,
//! `LayoutFlexboxContainer`, `LayoutContainingBlock`). Block and flow-root containers run the
//! block algorithm (CSS 2.1 §10.3.3 widths and margins, §10.6.3 heights, percentages,
//! `box-sizing`, `min-*`/`max-*`, §8.3.1 margin collapsing); flex containers run the flexbox
//! algorithm; each inline root is one custom Taffy leaf driving its IFC container, which lays
//! its atomic inlines out through the tree (`atomic.rs`); replaced and control boxes are leaves
//! sized by `replaced::used_content_size`. The initial containing block is the viewport. Lengths are quantised to 1/64 px before they enter Taffy;
//! `calc()` is resolved by `resolve_calc_value`. Styles come from `style_view`.

use crate::text::fonts::TextServices;
use taffy::prelude::*;
use taffy::{
    AxisStaticEdge, AxisStaticPosition, Baselines, Cache, CacheTree, ContainingBlockClaims,
    ExpandedLengthPercentage, LayoutBlockContainer, LayoutContainingBlock, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, OofCandidate, OofCandidates,
    OofPositioningArea, RequestedAxis, RunMode, SizingMode, TraversePartialTree, TraverseTree,
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_oof_layout, compute_root_layout,
};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{
    ErrorRecord, FeatureId, ReportKey, ReportLocation, UnsupportedReason,
};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use ui4me_engine_types::text_metrics::FontMetricsSource;

use super::atomic::{LeafNode, TreeAtomics};
use super::calc::quantize_input;
use super::host_style::HostStyle;
use super::style_view::{CalcTable, StyleContext, containing_block_claims, taffy_style};
use super::{LayoutReport, Viewport};
use crate::layout::box_tree::{
    BoxId, BoxKind, BoxOwner, BoxTag, BoxTree, InnerDisplay, TableTag, VisibilityValue,
};
use crate::layout::fragment::FragmentTree;
use crate::layout::fragment::builder::build_fragments;
use crate::layout::ifc::container::InlineContainer;
use crate::layout::ifc::interface::AvailableInline;

thread_local! { pub(crate) static STYLE_NS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
thread_local! { pub(crate) static TABLE_NS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
thread_local! { pub(crate) static IFC_NS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }

pub(crate) struct HostNode {
    pub box_id: BoxId,
    pub style: Style,
    pub(crate) cache: Cache,
    pub layout: Layout,
    /// Block-level children; for an inline root, its atomic inlines (laid out and placed by the
    /// IFC container, `coordinator/atomic.rs`).
    pub children: Vec<usize>,
    pub ifc: Option<InlineContainer>,
    /// Atomic inlines of an inline root: (box id, node index).
    pub(crate) atomics: Vec<(BoxId, usize)>,
    /// CSS values kept for atomic inlines, replaced boxes and buttons (margins, sizes, edges).
    pub(crate) css: Option<std::sync::Arc<crate::layout::box_tree::BoxStyle>>,
    /// A replaced or control box (a leaf with a natural geometry).
    pub(crate) leaf: Option<LeafNode>,
    /// Baseline of a replaced box from its border-box top; `None` = bottom edge.
    pub(crate) leaf_baseline: Option<f32>,
    /// A `button` box: its single `ButtonContent` child is centred vertically.
    pub(crate) button: bool,
    /// An atomic inline placed by its inline root: `layout.location` is relative to that root's
    /// content box.
    pub(crate) in_ifc: bool,
    /// Atomic inlines only: the out-of-flow candidates of the last `Final` layout that the atomic
    /// did not claim; its inline root hands them on once the atomic is placed.
    pub(crate) atomic_oof: OofCandidates,
    /// Baselines of the last `PerformLayout` result (research R-12), from the node's border-box top.
    pub baselines: Baselines,
    /// Which out-of-flow descendants this node is the containing block of (FR-056, FR-059, FR-061).
    pub claims: ContainingBlockClaims,
    /// `position: absolute | fixed`: a child of its containing block's fragment, not of its parent.
    pub out_of_flow: bool,
    /// Out-of-flow boxes whose containing block is this node, recorded by Taffy's positioning pass
    /// in document order (the hoisted children of `compute_oof_layout`).
    pub hoisted: Vec<usize>,
    /// Inline roots only: the out-of-flow boxes inside the inline content (host node indexes).
    /// Inline content is not a Taffy tree, so the root hands them to the positioning pass itself.
    pub inline_oof: Vec<InlineOof>,
    /// The grid areas of a grid container, for the grid-area containing block of its
    /// out-of-flow children (FR-053). `None` for every other box.
    pub grid_info: taffy::DetailedLayoutInfo<String>,
    /// A table wrapper or a table grid box (`table/`): the nodes dispatched to the table container.
    pub(crate) table: Option<Box<crate::layout::table::TableHost>>,
    /// A table cell: its position in the grid, published with its fragment (FR-081).
    pub(crate) table_cell: Option<crate::layout::fragment::TableCellInfo>,
    /// `width: min-content | max-content | fit-content[(..)]` (the box's `width` is `auto`): the
    /// inline size is resolved from the content at layout time (`dispatch`).
    pub(crate) width_kw: Option<crate::layout::box_tree::WidthKeyword>,
    /// `min-width: min-content | max-content | fit-content` (`min-width` is `auto`): the inline size is
    /// raised to the keyword's size at layout time (`dispatch`).
    pub(crate) min_width_kw: Option<crate::layout::box_tree::WidthKeyword>,
    /// A block child of a legacy `-webkit-center | -webkit-right` parent: laid out with auto margins so that
    /// it is moved, but its margins stay the specified ones (Blink offsets the box, it does not change its
    /// margins): the specified `(margin-left, margin-right)`, reported by the fragment.
    pub(crate) legacy_align_margins: Option<(
        crate::layout::box_tree::LengthPercentage,
        crate::layout::box_tree::LengthPercentage,
    )>,
    /// An `absolute` box whose containing block is an inline box: the element of that inline box.
    pub(crate) inline_cb_owner: Option<ui4me_engine_types::ids::NodeId>,
    /// A grid item (in-flow or not): its edges are floored to 1/64 px when the fragment is built, as
    /// the reference browser's grid track offsets are (see `fragment/builder.rs`).
    pub(crate) grid_item: bool,
    /// One past the last node of this node's subtree (`add` pushes a subtree contiguously).
    pub(crate) subtree_end: usize,
    /// A subgrid (CSS Grid 2 section 9): what its parent grid recorded for it (Taffy patch P-008-4).
    pub(crate) subgrid: Option<Box<taffy::SubgridInfo<String>>>,
    /// Size containment in effect (inline axis, block axis; CSS Containment 2 section 3.1): the
    /// box's size on a contained axis is computed as if it had no content (`dispatch`).
    pub(crate) size_contain: (bool, bool),
    /// `contain-intrinsic-width` / `-height` (CSS px) of a size-contained box (0.9 F0903).
    pub(crate) intrinsic_size: (Option<f32>, Option<f32>),
    /// Fragmentation facts of the box (015 FRAG-2b, `crate::layout::fragmentation`).
    pub(crate) frag_facts: crate::layout::fragmentation::FragFacts,
    /// The node's role in a fragmentation root (a multicol container, its flow thread).
    pub(crate) frag_role: crate::layout::fragmentation::FragRole,
    /// A flow thread: the geometry it was laid out with and its break tokens (for the fragment builder).
    pub(crate) flow_geom: Option<Box<crate::layout::fragmentation::FlowThreadGeometry>>,
    /// A table cell: its height measure and the top padding of its last final layout (`table/layout.rs`,
    /// 015 PERF-1).
    pub(crate) cell_memo: Option<crate::layout::table::CellMemo>,
    /// The node's last final layout inside a fragmentation root, when it may be taken again (015 MC-2).
    pub(crate) frag_cache: Option<Box<crate::layout::fragmentation::FragCacheEntry>>,
    /// The layout this node was seeded with from the previous pass (`replay.rs`), when it was.
    pub(crate) seed_layout: Option<Layout>,
    /// The node was laid out in this pass (a cache miss). A seeded subtree none of whose nodes was laid
    /// out again has the previous pass's fragments (`fragment/builder.rs`, 015 PERF-1).
    pub(crate) computed: bool,
    /// The writing mode of the node's own logical frame (015 WM-1; `crate::layout::writing_mode`).
    pub(crate) wm: crate::layout::writing_mode::WritingMode,
    /// The writing mode of the frame the node is placed in (its parent's).
    pub(crate) parent_wm: crate::layout::writing_mode::WritingMode,
    /// An orthogonal flow root (CSS Writing Modes 4 section 7): `style` is in the node's own frame, `outer`
    /// in its parent's, and `dispatch_and_position` transposes the layout inputs and output.
    pub(crate) outer: Option<Box<Style>>,
    /// An orthogonal flow root: its scrollable overflow in its own frame from its last final layout.
    pub(crate) ortho_overflow: Option<taffy::ScrollableOverflow>,
    /// A box of a vertical writing mode: how far its first available font's central baseline lies above
    /// the alphabetic one (`crate::layout::writing_mode::central_offset`); for an atomic inline, that of its inline
    /// root's font (the strut it aligns to) is `strut_central`.
    pub(crate) central: f32,
    pub(crate) strut_central: f32,
}

/// What one fragmented layout of a flow thread found (MC-1 balancing).
#[derive(Clone, Debug, Default)]
pub(crate) struct RunSummary {
    pub tallest: f32,
    pub shortage: Option<f32>,
    /// Forced breaks taken.
    pub forced: usize,
    /// The measuring layout: where forced breaks would start a fragmentainer.
    pub forced_marks: Vec<f32>,
    /// A line break violated `orphans`.
    pub orphans_violated: bool,
}

/// An out-of-flow box inside inline content: where the paragraph holds its placeholder.
pub(crate) struct InlineOof {
    pub node: usize,
    /// The box was inline-level before blockification: its static position is a point in a line
    /// (otherwise the start of the line below the content before it). The IFC supplies the
    /// position (`record_static_position`, `FinalExtras::oof_static`).
    pub inline_level: bool,
}

pub(crate) struct HostTree {
    pub nodes: Vec<HostNode>,
    /// Calc expressions referenced by `style`s, keyed by address (stable for the pass).
    pub(crate) calcs: CalcTable,
    /// Widths of the containers being laid out, innermost last (`None` while a container's width is
    /// unknown): the fallback containing-block width of ratio-only replaced boxes.
    pub(crate) container_width: Vec<Option<f32>>,
    /// `absolute` boxes with an inline containing block, as the node that claimed them last saw them (its
    /// index and the candidate in its frame): resolved after the fragments exist.
    pub(crate) inline_cb_pending: Vec<(usize, OofCandidate)>,
    /// The node being measured "as if it had no content" (size containment): its children are hidden
    /// from the formatting algorithms while it is set (`TraversePartialTree`, `dispatch`).
    pub(crate) empty_probe: Option<usize>,
    /// The fragmentation root being laid out (015 FRAG-2b): `None` outside one, and always in a
    /// document without one (every fragmentation hook then returns at once).
    pub(crate) frag: Option<Box<crate::layout::fragmentation::FragContext>>,
    /// The block formatting context of the child about to be laid out holds floats: its final layout inside a
    /// fragmentation root is not kept (015 MC-2, `FragCacheEntry`).
    pub(crate) bfc_floats: bool,
    /// Layouts of fragmentation roots run in this pass and final layouts kept from earlier ones (diagnostics,
    /// `UI4ME_PROFILE`; 015 MC-2).
    pub(crate) frag_layouts: (u32, u32),
    /// Inline containers of the previous pass for inline roots copied with the same ids (015 PERF-1).
    pub(crate) ifc_seeds: std::collections::HashMap<BoxId, InlineContainer>,
    /// The physical viewport (the initial containing block of orthogonal flows, CSS Writing Modes 4 section 7.3).
    pub(crate) viewport: Viewport,
    /// The text services of the layout pass, set while Taffy runs (null otherwise): a paragraph whose first line has
    /// its own style is shaped once its first line is known, which depends on the width it is laid out in
    /// (015 TXT-5, `ifc/container.rs`). Only the layout pass's thread touches it, during `compute_root_layout`.
    pub(crate) text_ptr: *mut TextServices,
    /// The tree is laid out as pages (015 PAGE-1, `crate::layout::fragmentation::paged`): the fragmentation facts of
    /// boxes outside multicol containers follow the page context (page breaks forced, `avoid-page`).
    pub(crate) paged: bool,
}

impl HostTree {
    /// Add the host nodes of box `id` and its subtree (contiguous: `subtree_end`).
    pub(crate) fn add(
        &mut self,
        boxes: &BoxTree,
        id: BoxId,
        parent_inner: Option<InnerDisplay>,
        text: &mut TextServices,
    ) -> usize {
        let idx = self.add_node(boxes, id, parent_inner, text);
        self.nodes[idx].subtree_end = self.nodes.len();
        idx
    }

    fn add_node(
        &mut self,
        boxes: &BoxTree,
        id: BoxId,
        parent_inner: Option<InnerDisplay>,
        text: &mut TextServices,
    ) -> usize {
        use crate::layout::replaced::ReplacedKind;
        use crate::layout::writing_mode::{orthogonal, relayer};
        let b = boxes.get(id);
        // 015 WM-1: the frame of the node. A replaced box has no flow of its own: one whose writing mode is
        // orthogonal to its parent's is laid out in the parent's frame.
        let own_wm = b.style.writing_mode;
        let parent_wm = b.parent.map_or(own_wm, |p| boxes.get(p).style.writing_mode);
        let leaf_box = matches!(b.kind, BoxKind::Replaced(_));
        let relayered;
        let bs: &crate::layout::box_tree::BoxStyle = if leaf_box && orthogonal(own_wm, parent_wm) {
            relayered = relayer(&b.style, own_wm, parent_wm);
            &relayered
        } else {
            &b.style
        };
        let wm = if leaf_box { parent_wm } else { own_wm };
        let ortho = orthogonal(wm, parent_wm);
        // 015 PAGE-1: a box in the page flow (not inside a multicol container's flow thread).
        let page_ctx =
            self.paged && !crate::layout::fragmentation::paged::in_flow_thread(boxes, id);
        let t_st = std::time::Instant::now();
        let mut style = taffy_style(&mut self.calcs, bs, b.inner, StyleContext { parent_inner });
        let outer = ortho.then(|| {
            let mut o = taffy_style(
                &mut self.calcs,
                &relayer(&b.style, own_wm, parent_wm),
                b.inner,
                StyleContext { parent_inner },
            );
            // An orthogonal flow root establishes an independent formatting context; in a block container its
            // automatic size in the parent's inline axis (its own block size) is its content's (not stretched).
            o.contain |= taffy::Contain::PAINT;
            o.grid_template_columns_subgrid = false;
            o.grid_template_rows_subgrid = false;
            if o.size.width.is_auto()
                && matches!(
                    parent_inner,
                    None | Some(InnerDisplay::Flow | InnerDisplay::FlowRoot)
                )
            {
                o.size.width = taffy::Dimension::fit_content();
            }
            Box::new(o)
        });
        if ortho {
            style.contain |= taffy::Contain::PAINT;
            // An orthogonal subgrid (CSS Grid 2 section 9.1: its axes map to the parent's other axes) is laid
            // out as an independent grid and reported (`layout_boxes`; 015 WM-1c).
            style.grid_template_columns_subgrid = false;
            style.grid_template_rows_subgrid = false;
        }
        STYLE_NS.with(|c| c.set(c.get() + t_st.elapsed().as_nanos() as u64));
        // CSS Containment 2: layout and paint containment make the box an independent formatting
        // context (and layout containment suppresses its baselines); Taffy reads both from `contain`.
        let contain = b.effective_containment();
        if contain.layout {
            style.contain |= taffy::Contain::LAYOUT;
        }
        if contain.paint {
            style.contain |= taffy::Contain::PAINT;
        }
        let subgrid_item = parent_inner == Some(InnerDisplay::Grid)
            && !b.is_out_of_flow()
            && b.inner == InnerDisplay::Grid
            && (style.grid_template_columns_subgrid || style.grid_template_rows_subgrid)
            && !style.contain.establishes_independent_formatting_context();
        // A replaced box resolves its own auto width (CSS 2.1 §10.3.4); a control with a natural
        // size is "compressible"; a fieldset establishes a block formatting context (its inner
        // display is flow-root, see `html_rendering.rs`).
        if let BoxKind::Replaced(r) = &b.kind {
            match r.kind {
                ReplacedKind::Control(_) => style.item_is_compressible_replaced = true,
                _ => style.item_is_replaced = true,
            }
        }
        // A table wrapper resolves its own width like a replaced box (CSS 2.1 §17.5.2): Taffy's
        // block algorithm asks it for its size (`item_is_table`).
        if matches!(b.tag, BoxTag::Table(TableTag::Wrapper)) {
            style.item_is_table = true;
            // The wrapper is as wide (and as tall) as the table grid box, whose `width` / `height` are
            // specified on the grid: a flex item wrapper is not stretched on the cross axis when the
            // table's size on that axis is not `auto` (CSS Align 3 §stretch needs an `auto` size).
            if parent_inner == Some(InnerDisplay::Flex)
                && let Some(parent) = b.parent.map(|p| boxes.get(p))
                && let Some(grid) = b
                    .children
                    .iter()
                    .map(|c| boxes.get(*c))
                    .find(|c| matches!(c.tag, BoxTag::Table(TableTag::Table)))
            {
                use crate::layout::box_tree::{FlexDirectionValue as FD, SizeValue};
                let cross_is_width =
                    matches!(parent.style.flex.direction, FD::Column | FD::ColumnReverse);
                let cross = if cross_is_width {
                    &grid.style.width
                } else {
                    &grid.style.height
                };
                let stretches = matches!(style.align_self, None | Some(taffy::AlignItems::STRETCH));
                if stretches
                    && !matches!(cross, SizeValue::Auto)
                    && (!cross_is_width
                        || !matches!(
                            cross,
                            SizeValue::Lp(crate::layout::box_tree::LengthPercentage::Percent(_))
                        ))
                {
                    style.align_self = Some(taffy::AlignItems::FLEX_START);
                }
            }
        }
        // `aspect-ratio` on a box that is not replaced (a replaced box has its own natural ratio) and is
        // not a table part: Taffy's block, flex and grid algorithms apply it (CSS Sizing 4 §5.1).
        if !matches!(b.kind, BoxKind::Replaced(_)) && matches!(b.tag, BoxTag::None) {
            style.aspect_ratio = bs.aspect_ratio;
            // A definite width and an auto height: the height is the width over the ratio. Taffy would
            // also move `max-height` / `min-height` through the ratio onto the (specified) width, which
            // CSS Sizing 4 §5.1 only does for an `auto` width.
            if let (
                Some(r),
                crate::layout::box_tree::SizeValue::Lp(
                    crate::layout::box_tree::LengthPercentage::Px(w),
                ),
                crate::layout::box_tree::SizeValue::Auto,
            ) = (bs.aspect_ratio, &bs.width, &bs.height)
                && r > 0.0
                && r.is_finite()
            {
                style.size.height = taffy::Dimension::length(super::calc::quantize_input(w / r));
                style.aspect_ratio = None;
            }
        }
        // Legacy `text-align: -webkit-center | -webkit-right` on the parent (HTML `align`, `center`): the
        // parent's in-flow block-level children are centred or pushed right by turning their margins
        // into auto margins (Chromium; the width stays specified, auto widths still stretch).
        let mut legacy_align_margins = None;
        if let Some(parent) = b.parent.map(|p| boxes.get(p))
            && parent.style.webkit_align
            && matches!(parent.inner, InnerDisplay::Flow | InnerDisplay::FlowRoot)
            && b.is_block()
            && !b.is_out_of_flow()
            && matches!(b.kind, BoxKind::BlockContainer { .. })
        {
            let auto = LengthPercentageAuto::auto();
            let (l_auto, r_auto) = (style.margin.left.is_auto(), style.margin.right.is_auto());
            let specified = match (&bs.margin.left, &bs.margin.right) {
                (
                    crate::layout::box_tree::MarginValue::Lp(l),
                    crate::layout::box_tree::MarginValue::Lp(r),
                ) => Some((l.clone(), r.clone())),
                _ => None,
            };
            match parent.style.text_align {
                crate::layout::box_tree::TextAlignValue::Center if !l_auto && !r_auto => {
                    style.margin.left = auto;
                    style.margin.right = auto;
                    legacy_align_margins = specified;
                }
                crate::layout::box_tree::TextAlignValue::Right if !l_auto && !r_auto => {
                    style.margin.left = auto;
                    legacy_align_margins = specified;
                }
                _ => {}
            }
        }
        let idx = self.nodes.len();
        let t_ifc = std::time::Instant::now();
        // An inline root copied from the previous tree with its parent (same ids, same subtree) takes the
        // previous pass's container without its results (015 PERF-1): construction reads the subtree and the
        // parent's line clamp only.
        let ifc = b.is_inline_root().then(|| {
            self.ifc_seeds
                .remove(&id)
                .unwrap_or_else(|| InlineContainer::new(boxes, id, text))
        });
        IFC_NS.with(|c| c.set(c.get() + t_ifc.elapsed().as_nanos() as u64));
        let leaf = match &b.kind {
            BoxKind::Replaced(r) => {
                // 015 WM-1: in a vertical frame the natural width is the block size (`aspect_ratio` is stored
                // transposed; the natural size is measured physically, then transposed).
                let vertical = bs.writing_mode.is_vertical();
                let physical_ratio = if vertical {
                    bs.aspect_ratio.map(|r| if r > 0.0 { 1.0 / r } else { r })
                } else {
                    bs.aspect_ratio
                };
                let natural = match &r.kind {
                    ReplacedKind::Image(info) => info.natural(physical_ratio),
                    ReplacedKind::Embedded(e) if e.name == "audio" => {
                        // The audio controls bar: 300 x 54 (Chromium), not the 300 x 150 of other embedded content.
                        crate::layout::replaced::Natural {
                            width: Some(300.0),
                            height: Some(54.0),
                            ..Default::default()
                        }
                    }
                    ReplacedKind::Embedded(e) => crate::layout::replaced::Natural {
                        ratio: e.ratio,
                        ..Default::default()
                    },
                    ReplacedKind::Control(
                        crate::layout::replaced::controls::ControlSpec::TextLine { .. }
                        | crate::layout::replaced::controls::ControlSpec::Number
                        | crate::layout::replaced::controls::ControlSpec::Textarea { .. },
                    ) if bs.field_sizing_content
                        && r.inner.as_ref().is_some_and(|i| i.select.is_none()) =>
                    {
                        let i = r.inner.as_ref().expect("checked");
                        crate::layout::replaced::controls::field_content_natural(
                            i.multiline,
                            i.password,
                            &i.value,
                            &i.placeholder,
                            text,
                            boxes.document,
                            &bs.text,
                        )
                    }
                    ReplacedKind::Control(spec) => spec.natural(text, boxes.document, &bs.text),
                    ReplacedKind::MarkerSymbol { shape, .. } => {
                        // A drawn bullet (FR-083): its box and margins follow the font's ascent.
                        let record =
                            text.metrics_service()
                                .query(&bs.text.font, bs.text.font_size, false);
                        let t = &bs.text;
                        let m = crate::layout::replaced::marker_symbol_metrics(
                            record.ascent,
                            record.descent,
                            record.line_gap,
                            t.line_height,
                            t.font_size,
                        );
                        // Chromium's inside disclosure marker advances 1.059em in pinned
                        // Noto Sans and Serif at 16, 24 and 32px, in both open states.
                        let width = match shape {
                            crate::layout::replaced::MarkerShape::DisclosureOpen
                            | crate::layout::replaced::MarkerShape::DisclosureClosedLtr
                            | crate::layout::replaced::MarkerShape::DisclosureClosedRtl => {
                                t.font_size * 1.059
                            }
                            _ => m.bullet + 2.0,
                        };
                        crate::layout::replaced::Natural {
                            width: Some(width),
                            height: Some(m.height),
                            ..Default::default()
                        }
                    }
                };
                let image_pending = matches!(&r.kind, ReplacedKind::Image(i) if i.is_pending());
                let inner = r.inner.as_ref().map(|src| {
                    let mut inner_style = bs.text.clone();
                    if matches!(
                        &r.kind,
                        ReplacedKind::Control(
                            crate::layout::replaced::controls::ControlSpec::Select {
                                rows: Some(_),
                                ..
                            }
                        ) | ReplacedKind::Control(
                            crate::layout::replaced::controls::ControlSpec::Select {
                                rows: None,
                                native: true,
                                ..
                            }
                        )
                    ) {
                        inner_style.line_height = crate::text::paragraph::LineHeightSpec::Normal;
                    }
                    std::sync::Arc::new(crate::layout::replaced::inner::InnerShaped::new(
                        src,
                        b.owner.node(),
                        boxes.document,
                        &inner_style,
                        control_base_direction(bs),
                        text,
                    ))
                });
                let marker_bullet_y = match &r.kind {
                    ReplacedKind::MarkerSymbol { .. } => {
                        let record =
                            text.metrics_service()
                                .query(&bs.text.font, bs.text.font_size, false);
                        let t = &bs.text;
                        Some(
                            crate::layout::replaced::marker_symbol_metrics(
                                record.ascent,
                                record.descent,
                                record.line_gap,
                                t.line_height,
                                t.font_size,
                            )
                            .bullet_y,
                        )
                    }
                    _ => None,
                };
                // A drawn list bullet is sized along the line already (its box is `bullet + 2` along the line).
                let natural = if vertical && !matches!(r.kind, ReplacedKind::MarkerSymbol { .. }) {
                    crate::layout::replaced::Natural {
                        width: natural.height,
                        height: natural.width,
                        ratio: natural.ratio.map(|r| if r > 0.0 { 1.0 / r } else { r }),
                        ..natural
                    }
                } else {
                    natural
                };
                // Size containment: a replaced element has a natural width (and, for `size`, height) of 0
                // and no natural aspect ratio (CSS Containment 2 section 3.1).
                let natural = if contain.size_inline || contain.size_block {
                    crate::layout::replaced::Natural {
                        width: Some(0.0),
                        height: if contain.size_block {
                            Some(0.0)
                        } else {
                            natural.height
                        },
                        ratio: None,
                        ..natural
                    }
                } else {
                    natural
                };
                let native_check_baseline = bs.native_control
                    && matches!(
                        &r.kind,
                        ReplacedKind::Control(
                            crate::layout::replaced::controls::ControlSpec::Checkbox
                                | crate::layout::replaced::controls::ControlSpec::Radio
                        )
                    );
                Some(LeafNode {
                    natural,
                    image_pending,
                    native_check_baseline,
                    inner,
                    marker_bullet_y,
                })
            }
            _ => None,
        };
        let leaf_is_some = leaf.is_some();
        let button = b.children.len() == 1 && boxes.get(b.children[0]).tag == BoxTag::ButtonContent;
        let keep_css = leaf.is_some()
            || b.is_atomic_inline()
            || b.is_float()
            || button
            || matches!(
                b.tag,
                BoxTag::Table(TableTag::Table | TableTag::Cell { .. })
            );
        self.nodes.push(HostNode {
            box_id: id,
            style,
            cache: Cache::new(),
            layout: Layout::with_order(0),
            children: Vec::new(),
            ifc,
            atomics: Vec::new(),
            css: keep_css.then(|| if ortho { std::sync::Arc::new(relayer(&b.style, own_wm, parent_wm)) } else if std::ptr::eq(bs, &*b.style) { b.style.clone() } else { std::sync::Arc::new(bs.clone()) }),
            leaf,
            leaf_baseline: None,
            button,
            in_ifc: false,
            atomic_oof: OofCandidates::new(),
            baselines: Baselines::NONE,
            // Layout and paint containment make the box the containing block of `absolute` and `fixed`
            // descendants (CSS Containment 2 sections 3.2 and 3.4).
            // An orthogonal flow root holds its out-of-flow descendants (their static positions are in its
            // frame; 015 WM-1, reported limit).
            claims: if contain.contains_layout_or_paint() || ortho { ContainingBlockClaims { absolute: true, fixed: true } } else { containing_block_claims(bs) },
            out_of_flow: b.is_out_of_flow(),
            hoisted: Vec::new(),
            inline_oof: Vec::new(),
            grid_info: taffy::DetailedLayoutInfo::None,
            table: None,
            table_cell: None,
            width_kw: match (&bs.width_kw, &b.kind, b.tag) {
                (Some(crate::layout::box_tree::WidthKeyword::Stretch), ..) => None,
                // An out-of-flow box's keyword is Taffy's (`style_view::oof_keyword`).
                (Some(crate::layout::box_tree::WidthKeyword::MinContent | crate::layout::box_tree::WidthKeyword::MaxContent | crate::layout::box_tree::WidthKeyword::FitContent(None)), ..) if b.is_out_of_flow() => None,
                (Some(k), BoxKind::BlockContainer { .. } | BoxKind::InlineBlock { .. }, BoxTag::None) => Some(k.clone()),
                _ => None,
            },
            min_width_kw: match (&bs.min_width_kw, &b.kind, b.tag) {
                (Some(k), BoxKind::BlockContainer { .. } | BoxKind::InlineBlock { .. }, BoxTag::None | BoxTag::Fieldset) => Some(k.clone()),
                _ => None,
            },
            legacy_align_margins,
            inline_cb_owner: super::oof::inline_containing_block(boxes, id).map(|x| boxes.get(x).owner.node()),
            grid_item: parent_inner == Some(InnerDisplay::Grid) && !b.is_out_of_flow(),
            subtree_end: idx + 1,
            subgrid: None,
            // A replaced box is size-contained through its natural size (below), not by a probe. A subgrid
            // (CSS Grid 2 section 9, Taffy P-008-4: an in-flow grid item with `subgrid` tracks and no layout or
            // paint containment) is not size-contained: its items take part in its parent's sizing, and the
            // reference browser sizes it from them.
            size_contain: if leaf_is_some || subgrid_item {
                (false, false)
            } else {
                (contain.size_inline, contain.size_block)
            },
            intrinsic_size: (
                contain.intrinsic.0.map(|v| v as f32 / 64.0),
                contain.intrinsic.1.map(|v| v as f32 / 64.0),
            ),
            frag_facts: crate::layout::fragmentation::FragFacts {
                // Monolithic content (CSS Break 3 section 4.4; plan §A3): replaced boxes, flex and grid
                // containers, tables, scroll containers, out-of-flow boxes, block-axis size containment and
                // (nested) multicol containers. Atomic inlines are monolithic too (`in_ifc`, set by their
                // inline root). Floats are breakable block containers (015 MC-2, as the reference) unless
                // monolithic for one of these reasons.
                monolithic: leaf_is_some
                    // Flex and grid containers break between their runs (flex lines, the items of a column
                    // container, grid rows) and inside their items (015 STAB-1, Taffy P-008-5); the variants
                    // whose runs do not follow the block axis forwards stay monolithic (reported), as do subgrids.
                    || (b.inner == InnerDisplay::Flex && !crate::layout::fragmentation::flexgrid::flex_breakable(&bs.flex))
                    || subgrid_item
                    // A table breaks between its rows (015 FRAG-2c): its wrapper and grid box are not monolithic,
                    // its cells and captions are (breaks inside cells are deferred).
                    || matches!(b.tag, BoxTag::Table(t) if !matches!(t, TableTag::Wrapper | TableTag::Table))
                    || b.tag == BoxTag::Fieldset
                    // Scroll containers (and `overflow: hidden`) are monolithic in columns; on pages they break like
                    // blocks (015 STAB-1, measured in the reference's print path: their lines move to the next page).
                    || (!page_ctx
                        && (!matches!(bs.overflow_x, crate::layout::box_tree::OverflowValue::Visible | crate::layout::box_tree::OverflowValue::Clip)
                            || !matches!(bs.overflow_y, crate::layout::box_tree::OverflowValue::Visible | crate::layout::box_tree::OverflowValue::Clip)))
                    || b.is_out_of_flow()
                    || contain.size_block
                    || b.tag == BoxTag::FlowThread
                    || crate::layout::fragmentation::multicol(bs).is_some(),
                float: b.is_float(),
                fc_item: matches!(parent_inner, Some(InnerDisplay::Flex | InnerDisplay::Grid)) && !b.is_out_of_flow(),
                split_reserve: bs.frag.split.filter(|s| s.index > 0 && !s.last).map_or(0.0, |s| s.end_pb),
                clears: bs.clear != crate::layout::box_tree::ClearValue::None,
                avoid_inside: if page_ctx {
                    matches!(bs.frag.break_inside, crate::layout::box_tree::BreakWithin::Avoid | crate::layout::box_tree::BreakWithin::AvoidPage)
                } else {
                    matches!(bs.frag.break_inside, crate::layout::box_tree::BreakWithin::Avoid | crate::layout::box_tree::BreakWithin::AvoidColumn)
                },
                // In a column fragmentation context only `column` and `always` force a break: `page`, `left` and
                // `right` (and the legacy `page-break-*: always`) force page breaks, and there are no pages on
                // screen (as in the reference browser). In a page context (015 PAGE-1) the page values force one.
                break_before_forced: crate::layout::fragmentation::breaks::forces_break(bs.frag.break_before, page_ctx),
                break_after_forced: crate::layout::fragmentation::breaks::forces_break(bs.frag.break_after, page_ctx),
                break_before_avoid: crate::layout::fragmentation::breaks::avoids_break(bs.frag.break_before, page_ctx),
                break_after_avoid: crate::layout::fragmentation::breaks::avoids_break(bs.frag.break_after, page_ctx),
                orphans: bs.frag.orphans.clamp(1, u16::MAX as u32) as u16,
                widows: bs.frag.widows.clamp(1, u16::MAX as u32) as u16,
                before_side: if page_ctx { crate::layout::fragmentation::breaks::side_of(bs.frag.break_before) } else { Default::default() },
                after_side: if page_ctx { crate::layout::fragmentation::breaks::side_of(bs.frag.break_after) } else { Default::default() },
                page: if page_ctx { crate::layout::fragmentation::paged::used_page(boxes, id) } else { None },
            },
            frag_role: if b.tag == BoxTag::FlowThread { crate::layout::fragmentation::FragRole::FlowThread } else { crate::layout::fragmentation::FragRole::None },
            flow_geom: None,
            cell_memo: None,
            frag_cache: None,
            seed_layout: None,
            computed: false,
            wm,
            parent_wm,
            outer,
            ortho_overflow: None,
            central: if own_wm.is_vertical() || parent_wm.is_vertical() {
                let r = text.metrics_service().query(&bs.text.font, bs.text.font_size, false);
                crate::layout::writing_mode::central_offset(r.ascent, r.descent)
            } else {
                0.0
            },
            strut_central: 0.0,
        });
        // An image marker (`list-style-image`): the box is the image and a 7 px gap (the reference
        // browser's `kCMarkerPaddingPx`) separates it from the content; an outside marker's margin box is
        // zero wide and its border box ends 7 px before the line start (Blink `ListMarker::InlineMargins*`).
        if let BoxKind::Replaced(r) = &b.kind
            && let ReplacedKind::Image(info) = &r.kind
            && matches!(
                b.owner,
                crate::layout::box_tree::BoxOwner::Pseudo(
                    _,
                    crate::layout::box_tree::PseudoKind::Marker
                )
            )
        {
            const GAP: f32 = 7.0;
            let w = info.natural(bs.aspect_ratio).width.unwrap_or(0.0);
            let (start, end) = if bs.marker_outside {
                (-(w + GAP), GAP)
            } else {
                (0.0, GAP)
            };
            let rtl = bs.direction == crate::layout::box_tree::Direction::Rtl;
            let (left, right) = if rtl { (end, start) } else { (start, end) };
            let node = &mut self.nodes[idx];
            node.style.margin.left =
                LengthPercentageAuto::length(super::calc::quantize_input(left));
            node.style.margin.right =
                LengthPercentageAuto::length(super::calc::quantize_input(right));
            if let Some(css) = node.css.as_mut().map(std::sync::Arc::make_mut) {
                css.margin.left = crate::layout::box_tree::MarginValue::Lp(
                    crate::layout::box_tree::LengthPercentage::Px(left),
                );
                css.margin.right = crate::layout::box_tree::MarginValue::Lp(
                    crate::layout::box_tree::LengthPercentage::Px(right),
                );
            }
        }
        if let BoxKind::Replaced(r) = &b.kind
            && matches!(
                r.kind,
                ReplacedKind::MarkerSymbol {
                    shape: crate::layout::replaced::MarkerShape::Disc
                        | crate::layout::replaced::MarkerShape::Circle
                        | crate::layout::replaced::MarkerShape::Square,
                    ..
                }
            )
        {
            // The margins of a drawn bullet (CSS Lists 3 section 3.1; the reference browser's
            // `ListMarker::InlineMarginsFor{Inside,Outside}`): inside, -1 px before and one em after;
            // outside, the whole margin box is zero wide and ends one `2/3 ascent + 8` px before
            // the line start.
            let record = text
                .metrics_service()
                .query(&bs.text.font, bs.text.font_size, false);
            let t = &bs.text;
            let m = crate::layout::replaced::marker_symbol_metrics(
                record.ascent,
                record.descent,
                record.line_gap,
                t.line_height,
                t.font_size,
            );
            let w = m.bullet + 2.0;
            let (start, end) = if bs.marker_outside {
                let before = m.two_thirds + 8.0;
                (-before, before - w)
            } else if matches!(
                r.kind,
                ReplacedKind::MarkerSymbol {
                    failed_image: true,
                    ..
                }
            ) {
                (0.0, 7.0)
            } else {
                (-1.0, bs.text.font_size)
            };
            let rtl = bs.direction == crate::layout::box_tree::Direction::Rtl;
            let (left, right) = if rtl { (end, start) } else { (start, end) };
            let node = &mut self.nodes[idx];
            node.style.margin.left =
                LengthPercentageAuto::length(super::calc::quantize_input(left));
            node.style.margin.right =
                LengthPercentageAuto::length(super::calc::quantize_input(right));
            if let Some(css) = node.css.as_mut().map(std::sync::Arc::make_mut) {
                css.margin.left = crate::layout::box_tree::MarginValue::Lp(
                    crate::layout::box_tree::LengthPercentage::Px(left),
                );
                css.margin.right = crate::layout::box_tree::MarginValue::Lp(
                    crate::layout::box_tree::LengthPercentage::Px(right),
                );
            }
        }
        if boxes.viewport_overflow.source == Some(id) {
            // FR-065: the element whose `overflow` went to the viewport has a used value of `visible`.
            self.nodes[idx].style.overflow = taffy::Point {
                x: taffy::Overflow::Visible,
                y: taffy::Overflow::Visible,
            };
        }
        match b.tag {
            BoxTag::Table(TableTag::Wrapper) => {
                self.add_wrapper_children(boxes, id, idx, text);
                return idx;
            }
            BoxTag::Table(TableTag::Table) => {
                self.add_grid_children(boxes, id, idx, text);
                return idx;
            }
            _ => {}
        }
        if b.is_inline_root() {
            // The atomic inlines of the root (through its inline boxes) are host nodes of their
            // own, laid out and placed by the IFC container.
            let mut atomics = Vec::new();
            collect_atomics(boxes, id, &mut atomics);
            let mut kids = Vec::new();
            for a in atomics {
                let k = self.add(boxes, a, None, text);
                self.nodes[k].in_ifc = true;
                self.nodes[k].strut_central = self.nodes[idx].central;
                self.nodes[idx].atomics.push((a, k));
                kids.push(k);
            }
            self.nodes[idx].children = kids;
        }
        if b.leaf_free() && !b.is_inline_root() {
            let mut kids: Vec<BoxId> = b
                .children
                .iter()
                .copied()
                .filter(|c| boxes.get(*c).is_block())
                .collect();
            if matches!(b.inner, InnerDisplay::Flex | InnerDisplay::Grid) {
                // CSS `order` (flex and grid items): ordered by order-modified document order;
                // the sort is stable, so ties keep source order (CSS Flexbox 1 §5.4).
                kids.sort_by_key(|k| boxes.get(*k).style.order);
            }
            let kids: Vec<usize> = kids
                .into_iter()
                .map(|c| self.add(boxes, c, Some(b.inner), text))
                .collect();
            // A multicol container (015 MC-1): an independent formatting context whose in-flow children are
            // its rows' flow threads and its spanners.
            let flows: Vec<usize> = kids
                .iter()
                .copied()
                .filter(|k| {
                    self.nodes[*k].frag_role == crate::layout::fragmentation::FragRole::FlowThread
                })
                .collect();
            if let Some(spec) = crate::layout::fragmentation::multicol(bs)
                && !flows.is_empty()
            {
                use crate::layout::fragmentation::GapSpec;
                let gap = match &bs.align.column_gap {
                    crate::layout::box_tree::GapValue::Normal => GapSpec::Px(bs.text.font_size),
                    crate::layout::box_tree::GapValue::Lp(
                        crate::layout::box_tree::LengthPercentage::Px(p),
                    ) => GapSpec::Px(*p),
                    crate::layout::box_tree::GapValue::Lp(
                        crate::layout::box_tree::LengthPercentage::Percent(p),
                    ) => GapSpec::Percent(*p),
                    crate::layout::box_tree::GapValue::Lp(
                        crate::layout::box_tree::LengthPercentage::Calc(c),
                    ) => GapSpec::Calc(c.clone()),
                };
                self.nodes[idx].style.display = Display::FlowRoot;
                let last = flows.len() - 1;
                for (k, f) in flows.iter().enumerate() {
                    let g = self.nodes[*f]
                        .flow_geom
                        .get_or_insert_with(Default::default);
                    g.last_row = k == last && kids.last() == Some(f);
                    g.fill_auto = spec.fill_auto;
                }
                self.nodes[idx].frag_role =
                    crate::layout::fragmentation::FragRole::Multicol { spec, gap, flows };
            }
            self.nodes[idx].children = kids;
        } else if b.is_inline_root() {
            // Out-of-flow boxes inside the inline content are laid out against their containing
            // block like any other (FR-056); their static position is in the inline flow.
            let mut found = Vec::new();
            collect_inline_oof(boxes, &b.children, &mut found);
            let found: Vec<InlineOof> = found
                .into_iter()
                .map(|(c, inline_level)| InlineOof {
                    node: self.add(boxes, c, None, text),
                    inline_level,
                })
                .collect();
            self.nodes[idx].inline_oof = found;
        }
        idx
    }

    /// A block node as large as the viewport with `root` as its only child (see `layout_boxes`).
    fn push_icb_parent(&mut self, root: usize, viewport: Viewport) -> usize {
        let idx = self.nodes.len();
        let style = Style {
            display: Display::Block,
            size: Size {
                width: Dimension::length(quantize_input(viewport.width)),
                height: Dimension::length(quantize_input(viewport.height)),
            },
            ..Style::default()
        };
        let box_id = self.nodes[root].box_id;
        self.nodes.push(HostNode {
            box_id,
            style,
            cache: Cache::new(),
            layout: Layout::with_order(0),
            children: vec![root],
            ifc: None,
            atomics: Vec::new(),
            css: None,
            leaf: None,
            leaf_baseline: None,
            button: false,
            in_ifc: false,
            atomic_oof: OofCandidates::new(),
            baselines: Baselines::NONE,
            claims: ContainingBlockClaims {
                absolute: true,
                fixed: true,
            },
            out_of_flow: false,
            hoisted: Vec::new(),
            inline_oof: Vec::new(),
            grid_info: taffy::DetailedLayoutInfo::None,
            table: None,
            table_cell: None,
            width_kw: None,
            min_width_kw: None,
            legacy_align_margins: None,
            inline_cb_owner: None,
            grid_item: false,
            subtree_end: idx + 1,
            subgrid: None,
            size_contain: (false, false),
            intrinsic_size: (None, None),
            frag_facts: Default::default(),
            frag_role: Default::default(),
            flow_geom: None,
            cell_memo: None,
            frag_cache: None,
            seed_layout: None,
            computed: false,
            wm: self.nodes[root].wm,
            parent_wm: self.nodes[root].wm,
            outer: None,
            ortho_overflow: None,
            central: 0.0,
            strut_central: 0.0,
        });
        idx
    }

    /// The style of a node as its parent's formatting algorithm reads it: an orthogonal flow root's
    /// outer style (015 WM-1).
    fn child_style(&self, n: NodeId) -> HostStyle<'_> {
        let node = &self.nodes[usize::from(n)];
        HostStyle::new(node.outer.as_deref().unwrap_or(&node.style), node.claims)
    }

    fn host_style(&self, n: NodeId) -> HostStyle<'_> {
        let node = &self.nodes[usize::from(n)];
        HostStyle::new(&node.style, node.claims)
    }

    /// Keep the baselines of the last full layout of a node for its box fragment (FR-090/FR-093).
    fn note_baselines(&mut self, n: NodeId, inputs: &LayoutInput, out: &LayoutOutput) {
        if inputs.run_mode == RunMode::PerformLayout {
            self.nodes[usize::from(n)].baselines = out.baselines;
        }
    }

    /// The layout algorithm of a node (the dispatch of `compute_child_layout`). `block_ctx` is the
    /// parent's block formatting context for a block child that does not establish its own.
    fn dispatch(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        let i = usize::from(n);
        // A flex or grid item in a fragmentation root (015 STAB-1): its growth and its first content are noted.
        if inputs.run_mode == RunMode::PerformLayout
            && self.nodes[i].frag_facts.fc_item
            && let Some(f) = self.frag.as_deref()
            && f.root != i
            && f.item_bypass != Some(i)
            && let Some(&y) = f.abs.get(&i)
        {
            return self.dispatch_fc_item(n, inputs, block_ctx, y);
        }
        // Fragmentation (015 FRAG-2b): suspended inside monolithic content; a multicol container sizes
        // its flow thread; a flow thread's final layout is a fragmentation root.
        if let Some(f) = self.frag.as_deref()
            && f.root != i
        {
            let facts = self.nodes[i].frag_facts;
            // A breakable float (015 MC-2) is laid out with the context where the previous pass placed it; first
            // met, it is laid out whole (`FragContext::note_float` asks for the pass that breaks it).
            let float_at = if facts.breakable_float() {
                f.float_at.get(&i).copied()
            } else {
                None
            };
            if let Some(y) = float_at {
                if let Some(f) = self.frag.as_deref_mut() {
                    f.abs.insert(i, y);
                }
            } else if facts.monolithic
                || facts.float
                || self.nodes[i].in_ifc
                || (facts.avoid_inside && !f.avoid_overflow.contains(&i))
            {
                let saved = self.frag.take();
                let out = self.dispatch(n, inputs, block_ctx);
                self.frag = saved;
                return out;
            }
        }
        match self.nodes[i].frag_role {
            crate::layout::fragmentation::FragRole::None => {}
            crate::layout::fragmentation::FragRole::Multicol { .. } => {
                self.prepare_columns(i, &inputs);
                // An intrinsic width (min-/max-content): the content of one column, times the count, plus
                // the gaps (the rows are measured at their content's width: `prepare_columns`).
                let indefinite = inputs.known_dimensions.width.is_none()
                    && !matches!(inputs.available_space.width, AvailableSpace::Definite(_));
                if indefinite && inputs.run_mode != RunMode::PerformLayout {
                    let role = std::mem::take(&mut self.nodes[i].frag_role);
                    let out = self.dispatch(n, inputs, block_ctx);
                    self.nodes[i].frag_role = role;
                    return self.multicol_intrinsic(i, &inputs, out);
                }
            }
            crate::layout::fragmentation::FragRole::FlowThread => {
                let horizontal_probe = inputs.run_mode != RunMode::PerformLayout
                    && inputs.axis == RequestedAxis::Horizontal;
                if !horizontal_probe
                    && inputs.run_mode != RunMode::PerformHiddenLayout
                    && self.frag.as_deref().is_none_or(|f| f.root != i)
                {
                    return self.layout_flow_thread(n, inputs, block_ctx);
                }
            }
        }
        // Size containment (CSS Containment 2 section 3.1): on a contained axis whose size the caller
        // did not fix, the box is sized as if it had no content (its own algorithm, with the children
        // hidden, under the same inputs: explicit sizes, min/max, padding, border, grid tracks and
        // `aspect-ratio` still count), then laid out at that size with its content. Every intrinsic
        // probe of an ancestor (min-content, max-content, flex basis, shrink-to-fit) goes through
        // here, so they all see the empty size.
        let (ci, cb) = self.nodes[i].size_contain;
        if (ci || cb) && self.empty_probe != Some(i) {
            let need_w = ci && inputs.known_dimensions.width.is_none();
            let need_h = cb && inputs.known_dimensions.height.is_none();
            if need_w || need_h {
                let probe = LayoutInput {
                    run_mode: RunMode::ComputeSize,
                    ..inputs
                };
                let saved = self.empty_probe.replace(i);
                let empty = self.dispatch_container_empty(n, probe);
                self.empty_probe = saved;
                let mut inputs = inputs;
                // `contain-intrinsic-size`: sized as if its content were that size.
                let (iw, ih) = self.nodes[i].intrinsic_size;
                if need_w {
                    inputs.known_dimensions.width = Some(empty.width + iw.unwrap_or(0.0));
                }
                if need_h {
                    inputs.known_dimensions.height = Some(empty.height + ih.unwrap_or(0.0));
                }
                return self.dispatch(n, inputs, block_ctx);
            }
        }
        if let Some(kw) = self.nodes[i].width_kw.take() {
            let out = self.dispatch_width_keyword(n, &kw, inputs, block_ctx);
            self.nodes[i].width_kw = Some(kw);
            return out;
        }
        if let Some(kw) = self.nodes[i].min_width_kw.take() {
            let out = self.dispatch_min_width_keyword(n, &kw, inputs, block_ctx);
            self.nodes[i].min_width_kw = Some(kw);
            return out;
        }
        if self.empty_probe == Some(i) {
            // Measured as if empty: the box's own algorithm over no children (no lines, no table grid).
            return self.dispatch_container(n, inputs, None, |_, out| out);
        }
        if let Some(t) = &self.nodes[i].table {
            return match **t {
                crate::layout::table::TableHost::Wrapper(_) => self.layout_table_wrapper(i, inputs),
                crate::layout::table::TableHost::Grid(_) => self.layout_table_grid(i, inputs),
            };
        }
        if self.nodes[i].leaf.is_some() {
            return self.compute_replaced(i, inputs);
        }
        if self.nodes[i].ifc.is_some() {
            return self.layout_inline_root(i, inputs, block_ctx);
        }
        let center = |tree: &mut Self, out: LayoutOutput| {
            if inputs.run_mode == RunMode::PerformLayout && tree.nodes[i].button {
                tree.center_button_content(i, inputs, &out);
            }
            out
        };
        let width_hint = inputs
            .known_dimensions
            .width
            .or(match inputs.available_space.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            })
            .or(self.container_width.last().copied().flatten());
        self.container_width.push(width_hint);
        let out = self.dispatch_container(n, inputs, block_ctx, center);
        self.container_width.pop();
        if inputs.run_mode == RunMode::PerformLayout {
            self.note_block_floats(i);
        }
        out
    }

    /// The breakable floats a block container placed (015 MC-2), in flow-thread coordinates.
    fn note_block_floats(&mut self, i: usize) {
        let Some(f) = self.frag.as_deref_mut() else {
            return;
        };
        let Some(&top) = f.abs.get(&i) else { return };
        for &c in &self.nodes[i].children {
            let cn = &self.nodes[c];
            if cn.frag_facts.breakable_float() && !cn.in_ifc {
                f.note_float(c, top + cn.layout.location.y, cn.layout.size.height);
            }
        }
    }

    /// The intrinsic size of a multicol container measured as one column (`out`): N columns of that
    /// width (at least `column-width`) and the gaps between them.
    fn multicol_intrinsic(
        &mut self,
        i: usize,
        inputs: &LayoutInput,
        out: LayoutOutput,
    ) -> LayoutOutput {
        let crate::layout::fragmentation::FragRole::Multicol { spec, gap, .. } =
            &self.nodes[i].frag_role
        else {
            return out;
        };
        let st = &self.nodes[i].style;
        let basis = inputs.parent_size.width.unwrap_or(0.0);
        let side = |v: &LengthPercentage| resolve_lp(v, basis, &self.calcs);
        let pb_h = side(&st.padding.left)
            + side(&st.padding.right)
            + side(&st.border.left)
            + side(&st.border.right);
        let n = spec.count.unwrap_or(1).clamp(1, 1000) as f32;
        let cw = (out.size.width - pb_h)
            .max(0.0)
            .max(spec.width.unwrap_or(0.0));
        let g = gap.resolve(0.0);
        let w = pb_h + n * cw + (n - 1.0) * g;
        LayoutOutput {
            size: Size {
                width: w,
                height: out.size.height,
            },
            ..out
        }
    }

    /// A multicol container (015 MC-1): the used column count and width from its content width (CSS
    /// Multicol 1 section 3.4), the height its rows may take; every row's flow thread is laid out at the
    /// column width (`fragmentation::multicol`).
    fn prepare_columns(&mut self, i: usize, inputs: &LayoutInput) {
        let crate::layout::fragmentation::FragRole::Multicol { spec, gap, flows } =
            &self.nodes[i].frag_role
        else {
            return;
        };
        let (spec, flows) = (*spec, flows.clone());
        let st = &self.nodes[i].style;
        let basis = inputs.parent_size.width.unwrap_or(0.0);
        let side = |v: &LengthPercentage| resolve_lp(v, basis, &self.calcs);
        let pb_h = side(&st.padding.left)
            + side(&st.padding.right)
            + side(&st.border.left)
            + side(&st.border.right);
        let pb_top = side(&st.padding.top) + side(&st.border.top);
        let pb_v = pb_top + side(&st.padding.bottom) + side(&st.border.bottom);
        let width = inputs
            .known_dimensions
            .width
            .or(match inputs.available_space.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
        let border_box = |h: f32| {
            if st.box_sizing == BoxSizing::ContentBox {
                h + pb_v
            } else {
                h
            }
        };
        let parent_h = inputs.parent_size.height;
        let calcs = &self.calcs;
        let def = |d: Dimension| -> Option<f32> {
            taffy::MaybeResolve::maybe_resolve(d, parent_h, |p, b| calcs.resolve(p, b))
        };
        let def_lpa = |d: LengthPercentageAuto| -> Option<f32> {
            taffy::MaybeResolve::maybe_resolve(d, parent_h, |p, b| calcs.resolve(p, b))
        };
        let height = inputs
            .known_dimensions
            .height
            .or_else(|| def(st.size.height).map(border_box));
        let max_h = def_lpa(st.max_size.height).map(border_box);
        let min_h = def_lpa(st.min_size.height).map(border_box).unwrap_or(0.0);
        let avail = match (height, max_h) {
            (Some(h), _) => Some(h),
            (None, Some(m)) => Some(m.max(min_h)),
            (None, None) => None,
        }
        .map(|h| (h - pb_v).max(0.0));
        let column = width.map(|w| {
            let cw = (w - pb_h).max(0.0);
            let g = gap.resolve(cw);
            let (n, colw) = crate::layout::fragmentation::multicol::resolve(spec, cw, g);
            (n, colw, g)
        });
        let want = match column {
            Some((_, w, _)) => Dimension::length(quantize_input(w)),
            None => Dimension::auto(),
        };
        let (count, column_width, gap) = column.unwrap_or((spec.count.unwrap_or(1), 0.0, 0.0));
        for f in flows {
            let mut changed = self.nodes[f].style.size.width != want;
            if changed {
                self.nodes[f].style.size.width = want;
            }
            let g = self.nodes[f].flow_geom.get_or_insert_with(Default::default);
            if g.avail != avail || g.count != count || g.content_top != pb_top {
                changed = true;
            }
            g.avail = avail;
            g.count = count;
            g.column_width = column_width;
            g.gap = gap;
            g.content_top = pb_top;
            if changed {
                self.nodes[f].cache.clear();
            }
        }
    }

    /// A flow thread (one row of a multicol container, 015 MC-1): a fragmentation root. Its column height
    /// is the container's content height left for the row (`column-fill: auto`, last row, definite
    /// height) or the balanced height; it reports that height as its own (the row's height in the
    /// container) and keeps the strip's height for the fragment builder.
    /// The final layout of a flex or grid item at flow offset `y` inside a fragmentation root (015 STAB-1,
    /// `fragmentation::flexgrid`). When it may cross a boundary, its fragmented auto block size is measured against
    /// its unfragmented one (its growth, used by its container in the next pass); a first content pushed past a
    /// break moves the item's run in the next pass.
    fn dispatch_fc_item(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
        y: f32,
    ) -> LayoutOutput {
        use crate::layout::fragmentation::EPS;
        let i = usize::from(n);
        let facts = self.nodes[i].frag_facts;
        let (unbreakable, crosses) = {
            let f = self.frag.as_deref().expect("inside a fragmentation root");
            let unbreakable = facts.monolithic
                || facts.float
                || (facts.avoid_inside && !f.avoid_overflow.contains(&i));
            let crosses = !f.unbounded()
                && match inputs.known_dimensions.height {
                    Some(h) => y + h > f.next_boundary(y) + EPS,
                    None => true,
                };
            (unbreakable, crosses)
        };
        let saved = self
            .frag
            .as_deref_mut()
            .and_then(|f| f.item_bypass.replace(i));
        // The unfragmented auto block size (measured before the final layouts, which the measure must not follow).
        let whole = (crosses && !unbreakable).then(|| {
            let measure = LayoutInput {
                run_mode: RunMode::ComputeSize,
                axis: RequestedAxis::Vertical,
                known_dimensions: Size {
                    width: inputs.known_dimensions.width,
                    height: None,
                },
                ..inputs
            };
            self.compute_child_layout(n, measure).size.height
        });
        // The fragmented auto block size: a probe layout when the container gave the item its size.
        let mut fragmented = None;
        if crosses && !unbreakable && inputs.known_dimensions.height.is_some() {
            let probe = LayoutInput {
                known_dimensions: Size {
                    width: inputs.known_dimensions.width,
                    height: None,
                },
                ..inputs
            };
            let (tokens, starts) = self
                .frag
                .as_deref()
                .map(|f| (f.tokens.len(), f.starts.len()))
                .unwrap_or_default();
            let auto = self.dispatch(n, probe, None);
            if let Some(f) = self.frag.as_deref_mut() {
                f.tokens.truncate(tokens);
                f.starts.truncate(starts);
            }
            fragmented = Some(auto.size);
        }
        let out = self.dispatch(n, inputs, block_ctx);
        if crosses && !unbreakable && fragmented.is_none() {
            fragmented = Some(out.size);
        }
        // Inside one fragmentainer (at its grown size) nothing breaks in the item: no growth. In the measuring layout of
        // balancing nothing breaks either.
        let mut grow = 0.0;
        if let (Some(sz), Some(whole)) = (fragmented, whole) {
            grow = (sz.height - whole).max(0.0);
        }
        if let Some(f) = self.frag.as_deref_mut() {
            f.item_bypass = saved;
            let first_pushed = f.leading_pushed.remove(&i) && !unbreakable;
            if !f.unbounded() {
                f.end_item(i, grow, first_pushed);
            }
        }
        out
    }

    fn layout_flow_thread(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        mut block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        use crate::layout::fragmentation::EPS;
        use crate::layout::fragmentation::multicol::{Balancer, RowOutcome, ceil_unit};
        let i = usize::from(n);
        let inputs = LayoutInput {
            run_mode: RunMode::PerformLayout,
            ..inputs
        };
        let g = self.nodes[i]
            .flow_geom
            .as_deref()
            .cloned()
            .unwrap_or_default();
        if g.bands.sizes.is_some() {
            // The page flow (015 PAGE-1): pages of the given heights, no balancing.
            let saved = self.frag.take();
            let (out, _) = self.run_fragmented(n, inputs, block_ctx, g.block_size.max(EPS), false);
            self.frag = saved;
            if let Some(g) = self.nodes[i].flow_geom.as_mut() {
                g.flow_height = out.size.height;
                g.balance_iterations = 1;
            }
            return out;
        }
        let cap = g.avail.map(|a| (a - g.row_y.max(0.0)).max(0.0));
        let count = g.count.max(1);
        let saved = self.frag.take();
        let (out, h, iterations) = match cap {
            Some(c) if g.fill_auto && g.last_row => {
                let (out, _) = self.run_fragmented(n, inputs, block_ctx.as_deref_mut(), c, false);
                (out, c, 1)
            }
            _ => {
                // Balancing: measure the row unfragmented (its height and tallest unit), then search.
                let (whole, m) =
                    self.run_fragmented(n, inputs, block_ctx.as_deref_mut(), f32::INFINITY, true);
                let content = self.flow_content_bottom(i, &whole);
                // A piece between two spanners keeps its element's block-end border and padding as room (015 MC-2).
                let reserve = self.nodes[i + 1..self.nodes[i].subtree_end]
                    .iter()
                    .map(|n| n.frag_facts.split_reserve)
                    .fold(0.0, f32::max);
                if content <= EPS {
                    (whole, ceil_unit(reserve), 1)
                } else {
                    // Content between forced breaks that take every column at least needs its tallest segment.
                    let mut tallest = m.tallest.max(reserve);
                    if m.forced_marks.len() + 1 >= count as usize {
                        let mut edges = vec![0.0f32];
                        edges.extend(m.forced_marks.iter().copied());
                        edges.push(content);
                        tallest = edges
                            .windows(2)
                            .map(|w| w[1] - w[0])
                            .fold(tallest, f32::max);
                    }
                    // The first guess spreads the whole row: in the container's last row the last child's bottom
                    // margin is included, a row that ends at a spanner ends at its content (the reference's initial
                    // column heights); fitting is judged on the content's border boxes.
                    let spread = if g.last_row {
                        whole.size.height.max(content)
                    } else {
                        content
                    };
                    let (mut b, mut h) =
                        Balancer::new(count, cap, spread, tallest, whole.size.height);
                    loop {
                        let (out, r) =
                            self.run_fragmented(n, inputs, block_ctx.as_deref_mut(), h, true);
                        let bottom = self.flow_content_bottom(i, &out);
                        let columns = (count as usize).max(r.forced + 1) as f32;
                        let fits = bottom <= columns * h + EPS && !r.orphans_violated;
                        // What the content beyond the last column lacks is a shortage too (015 MC-2, measured: a line
                        // pushed to the last column can leave less overflow than the line itself lacked).
                        let over = bottom - columns * h;
                        let shortage = if over > EPS {
                            Some(r.shortage.map_or(over, |s| s.min(over)))
                        } else {
                            r.shortage
                        };
                        let o = RowOutcome {
                            fits,
                            shortage,
                            spread: (over / columns).max(EPS),
                        };
                        match b.next(h, o) {
                            Some(next) => h = next,
                            None => break (out, h, b.iterations),
                        }
                    }
                }
            }
        };
        self.frag = saved;
        let strip = out.size.height;
        if let Some(g) = self.nodes[i].flow_geom.as_mut() {
            g.block_size = h;
            g.flow_height = strip;
            g.balance_iterations = iterations;
        }
        LayoutOutput {
            size: Size {
                width: out.size.width,
                height: h,
            },
            ..out
        }
    }

    /// One layout of a flow thread at column height `h` (an infinite `h`: unfragmented, only measuring),
    /// laid out again (at most `fragmentation::MAX_PASSES` times) while a box turned out to sit at another
    /// offset than the one its content was laid out at.
    fn run_fragmented(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        mut block_ctx: Option<&mut taffy::BlockContext<'_>>,
        h: f32,
        measure: bool,
    ) -> (LayoutOutput, RunSummary) {
        use crate::layout::fragmentation::{FragContext, MAX_PASSES};
        let i = usize::from(n);
        if !h.is_finite() || h <= crate::layout::fragmentation::EPS {
            // No room to fragment into: one strip (measured when balancing).
            if !measure || h <= crate::layout::fragmentation::EPS {
                let out = self.dispatch_container(n, inputs, block_ctx, |_, o| o);
                return (out, RunSummary::default());
            }
        }
        let block_size = if h.is_finite() {
            h
        } else {
            crate::layout::fragmentation::UNBOUNDED
        };
        let mut carry = crate::layout::fragmentation::Carry {
            measure,
            ..Default::default()
        };
        let mut pass = 0;
        loop {
            pass += 1;
            let mut ctx = FragContext::new(i, block_size, carry);
            if let Some(g) = self.nodes[i].flow_geom.as_deref()
                && g.bands.sizes.is_some()
            {
                // The page flow (015 PAGE-1).
                ctx.bands = g.bands.clone();
                ctx.paged = true;
            }
            self.frag = Some(Box::new(ctx));
            let out = self.dispatch(n, inputs, block_ctx.as_deref_mut());
            let mut ctx = self.frag.take().expect("fragmentation root");
            self.frag_layouts.0 += 1;
            self.frag_layouts.1 += ctx.cache_hits;
            if !ctx.stale || pass >= MAX_PASSES {
                let summary = RunSummary {
                    tallest: ctx.tallest,
                    shortage: ctx.shortage,
                    forced_marks: std::mem::take(&mut ctx.forced_marks),
                    orphans_violated: ctx.orphans_violated,
                    forced: ctx
                        .tokens
                        .iter()
                        .filter(|t| t.kind == crate::layout::fragmentation::BreakKind::Forced)
                        .count(),
                };
                if let Some(g) = self.nodes[i].flow_geom.as_mut() {
                    g.tokens = std::mem::take(&mut ctx.tokens);
                    g.passes = pass;
                    g.blank = std::mem::take(&mut ctx.blank);
                    g.starts = std::mem::take(&mut ctx.starts);
                }
                return (out, summary);
            }
            carry = ctx.carry();
        }
    }

    /// A final layout of node `i` inside a fragmentation root (`cache_get`): its kept layout when it still holds
    /// (`FragCacheEntry`), else `None` (and the root's state is noted for `frag_cache_store`).
    fn frag_cache_get(&mut self, i: usize, input: &LayoutInput) -> Option<LayoutOutput> {
        let entry = self.nodes[i].frag_cache.take();
        let bfc_floats = std::mem::take(&mut self.bfc_floats);
        let n = &self.nodes[i];
        let end = n.subtree_end;
        let eligible = !bfc_floats
            && !n.in_ifc
            && !n.out_of_flow
            && !n.frag_facts.float
            && n.frag_role == crate::layout::fragmentation::FragRole::None
            && n.flow_geom.is_none();
        let f = self
            .frag
            .as_deref_mut()
            .expect("inside a fragmentation root");
        let abs = if eligible && f.root != i {
            f.abs.get(&i).copied()
        } else {
            None
        };
        let at_start = abs.and_then(|a| f.inside_one(a, 0.0));
        let ok = at_start.is_some() && !f.decided_in(i..end);
        if ok
            && let (Some(e), Some(a)) = (entry.as_deref(), abs)
            && e.inputs == *input
            && f.inside_one(a, e.extent) == Some(e.at_start)
        {
            f.tallest = f.tallest.max(e.tallest);
            if let Some(u) = e.first_unit {
                f.first_unit.insert(i, u);
            }
            f.cache_hits += 1;
            let out = e.out.clone();
            self.nodes[i].frag_cache = entry;
            return Some(out);
        }
        let snap = crate::layout::fragmentation::CacheSnap {
            node: i,
            ok,
            abs: abs.unwrap_or(0.0),
            at_start: at_start.unwrap_or(false),
            tokens: f.tokens.len(),
            forced_marks: f.forced_marks.len(),
            tallest: f.tallest,
            violated: f.violated,
            orphans_violated: f.orphans_violated,
        };
        if ok {
            f.tallest = 0.0;
        }
        f.cache_stack.push(snap);
        None
    }

    /// The end of a final layout of node `i` inside a fragmentation root: kept when it stayed inside one
    /// fragmentainer and the root decided nothing in it (`FragCacheEntry`).
    fn frag_cache_store(&mut self, i: usize, input: &LayoutInput, out: LayoutOutput) {
        let Some(snap) = self.frag.as_deref_mut().and_then(|f| f.cache_stack.pop()) else {
            return;
        };
        debug_assert_eq!(snap.node, i);
        if !snap.ok || snap.node != i {
            return;
        }
        let end = self.nodes[i].subtree_end;
        let extent = out
            .size
            .height
            .max(out.scrollable_overflow_rect.rect().bottom)
            .max(self.deep_bottom(i));
        let floats = self.has_bfc_floats(i);
        let f = self
            .frag
            .as_deref_mut()
            .expect("inside a fragmentation root");
        let sub_tallest = f.tallest;
        f.tallest = snap.tallest.max(sub_tallest);
        let clean = f.tokens.len() == snap.tokens
            && f.forced_marks.len() == snap.forced_marks
            && f.violated == snap.violated
            && f.orphans_violated == snap.orphans_violated
            && !floats
            && !f.decided_in(i..end)
            && f.inside_one(snap.abs, extent) == Some(snap.at_start);
        if clean {
            let first_unit = f.first_unit.get(&i).copied();
            let e = crate::layout::fragmentation::FragCacheEntry {
                inputs: *input,
                at_start: snap.at_start,
                extent,
                tallest: sub_tallest,
                first_unit,
                out,
            };
            self.nodes[i].frag_cache = Some(Box::new(e));
        }
    }

    /// Whether node `i`'s subtree holds a float of the block formatting context `i` takes part in.
    fn has_bfc_floats(&self, i: usize) -> bool {
        self.nodes[i].children.iter().any(|&c| {
            let cn = &self.nodes[c];
            !cn.out_of_flow
                && (cn.frag_facts.float
                    || (!cn.in_ifc
                        && !cn.frag_facts.monolithic
                        && cn.style.display == Display::Block
                        && self.has_bfc_floats(c)))
        })
    }

    /// Where the content of flow thread `i` ends (CSS px from its top): the bottom of its last in-flow
    /// box's border box (its trailing margin does not need room in a column), or of its lines, or of its
    /// breakable floats (015 MC-2).
    fn flow_content_bottom(&self, i: usize, out: &LayoutOutput) -> f32 {
        let n = &self.nodes[i];
        if n.ifc.is_some() {
            return out.size.height;
        }
        n.children
            .iter()
            .filter(|c| !self.nodes[**c].out_of_flow)
            .map(|c| {
                let l = &self.nodes[*c].layout;
                l.location.y + l.size.height
            })
            .fold(0.0, f32::max)
            .max(self.deep_bottom(i))
    }

    /// The bottom of the content of node `i` that is broken with the flow (015 MC-2), from its border-box top:
    /// its in-flow descendants' border boxes through boxes that are not monolithic (one that overflows a parent
    /// of a smaller height still needs room in a column, as in the reference's balancing), and its breakable
    /// floats. `-inf` when none.
    pub(crate) fn deep_bottom(&self, i: usize) -> f32 {
        let n = &self.nodes[i];
        let shift = if n.ifc.is_some() {
            n.layout.border.top + n.layout.padding.top
        } else {
            0.0
        };
        let mut b = f32::NEG_INFINITY;
        for &c in &n.children {
            let cn = &self.nodes[c];
            if cn.out_of_flow || (cn.in_ifc && !cn.frag_facts.breakable_float()) {
                continue;
            }
            let top = cn.layout.location.y + shift;
            b = b.max(top + cn.layout.size.height);
            if !cn.frag_facts.monolithic
                && matches!(cn.style.display, Display::Block | Display::FlowRoot)
            {
                b = b.max(top + self.deep_bottom(c));
            }
        }
        b
    }

    /// How far the content of node `i` that continues with the flow reaches below its border-box top (015 MC-2):
    /// its breakable floats, and for a breakable float itself what overflows it.
    pub(crate) fn flow_overflow(&self, i: usize) -> f32 {
        let b = self.float_bottom(i);
        if self.nodes[i].frag_facts.breakable_float() {
            b.max(self.deep_bottom(i))
        } else {
            b
        }
    }

    /// The bottom of the breakable floats (015 MC-2) of node `i`'s block formatting context that are inside
    /// it, from its border-box top (`-inf` when none): floats overflow the blocks they are in, and are broken
    /// across fragmentainers with the flow.
    pub(crate) fn float_bottom(&self, i: usize) -> f32 {
        let n = &self.nodes[i];
        let shift = if n.ifc.is_some() {
            n.layout.border.top + n.layout.padding.top
        } else {
            0.0
        };
        let mut b = f32::NEG_INFINITY;
        for &c in &n.children {
            let cn = &self.nodes[c];
            if cn.out_of_flow {
                continue;
            }
            let top = cn.layout.location.y + shift;
            if cn.frag_facts.breakable_float() {
                // Its content may overflow it (a fixed height): that is broken with the flow too (measured).
                b = b
                    .max(top + cn.layout.size.height)
                    .max(top + self.deep_bottom(c));
            } else if !cn.in_ifc && !cn.frag_facts.monolithic && cn.style.display == Display::Block
            {
                b = b.max(top + self.float_bottom(c));
            }
        }
        b
    }

    /// The border-box size of node `n` with its children hidden (`empty_probe` is `n`): its own
    /// formatting algorithm with no content (an inline root is a block container without lines).
    fn dispatch_container_empty(&mut self, n: NodeId, inputs: LayoutInput) -> Size<f32> {
        self.dispatch(n, inputs, None).size
    }

    fn dispatch_container(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
        center: impl Fn(&mut Self, LayoutOutput) -> LayoutOutput,
    ) -> LayoutOutput {
        let i = usize::from(n);
        match self.nodes[i].style.display {
            Display::Block => {
                let out = compute_block_layout(self, n, inputs, block_ctx);
                center(self, out)
            }
            // A flow-root starts its own block formatting context (FR-022).
            Display::FlowRoot => {
                let out = compute_block_layout(self, n, inputs, None);
                center(self, out)
            }
            Display::Flex => compute_flexbox_layout(self, n, inputs),
            Display::None => compute_hidden_layout(self, n),
            Display::Grid => compute_grid_layout(self, n, inputs),
        }
    }

    /// A box whose `width` is an intrinsic keyword (CSS Sizing 3 §3): its border-box width is its
    /// min-content or max-content width, or the fit-content one (the max-content width, at most the
    /// available space or the `fit-content()` argument, at least the min-content width). The content
    /// sizes come from the box's own algorithm; the box is then laid out at that width.
    fn dispatch_width_keyword(
        &mut self,
        n: NodeId,
        kw: &crate::layout::box_tree::WidthKeyword,
        mut inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        let Some(width) = self.keyword_width(n, kw, &inputs) else {
            return self.dispatch_plain(n, inputs, block_ctx);
        };
        inputs.known_dimensions.width = Some(width);
        if matches!(inputs.available_space.width, AvailableSpace::Definite(_)) {
            inputs.available_space.width = AvailableSpace::Definite(width);
        }
        self.dispatch_plain(n, inputs, block_ctx)
    }

    /// A box whose `min-width` is an intrinsic keyword (CSS Sizing 3 section 3, CSS 2.1 section 10.4): the
    /// used inline size is at least the keyword's size. A box whose width is not fixed by the algorithm
    /// yet (an intrinsic probe) is as wide as its content already, never below its min-content size.
    fn dispatch_min_width_keyword(
        &mut self,
        n: NodeId,
        kw: &crate::layout::box_tree::WidthKeyword,
        mut inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        if let Some(w) = inputs.known_dimensions.width
            && let Some(min) = self.keyword_width(n, kw, &inputs)
            && min > w
        {
            inputs.known_dimensions.width = Some(min);
            if matches!(inputs.available_space.width, AvailableSpace::Definite(_)) {
                inputs.available_space.width = AvailableSpace::Definite(min);
            }
        }
        self.dispatch_plain(n, inputs, block_ctx)
    }

    /// The border-box inline size an intrinsic keyword resolves to for `n` (`None` for `stretch`).
    fn keyword_width(
        &mut self,
        n: NodeId,
        kw: &crate::layout::box_tree::WidthKeyword,
        inputs: &LayoutInput,
    ) -> Option<f32> {
        use crate::layout::box_tree::{LengthPercentage as Lp, WidthKeyword as K};
        let probe = |tree: &mut Self, space: AvailableSpace| -> f32 {
            let input = LayoutInput {
                run_mode: RunMode::ComputeSize,
                sizing_mode: SizingMode::ContentSize,
                axis: RequestedAxis::Horizontal,
                known_dimensions: Size::NONE,
                known_dimensions_are_definite: Size {
                    width: true,
                    height: true,
                },
                parent_size: inputs.parent_size,
                available_space: Size {
                    width: space,
                    height: AvailableSpace::MaxContent,
                },
                vertical_margins_are_collapsible: Line::FALSE,
            };
            tree.dispatch(n, input, None).size.width
        };
        let avail = inputs
            .known_dimensions
            .width
            .or(match inputs.available_space.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
        let width = match kw {
            K::MinContent => probe(self, AvailableSpace::MinContent),
            K::MaxContent => probe(self, AvailableSpace::MaxContent),
            K::FitContent(arg) => {
                let (min, max) = (
                    probe(self, AvailableSpace::MinContent),
                    probe(self, AvailableSpace::MaxContent),
                );
                let cap = match (arg, avail) {
                    (Some(Lp::Px(p)), _) => Some(*p),
                    (Some(Lp::Percent(p)), Some(a)) => Some(p * a),
                    (Some(Lp::Calc(e)), Some(a)) => {
                        Some(crate::layout::coordinator::calc::resolve_calc_value(e, a))
                    }
                    (Some(_), None) => None,
                    (None, a) => a,
                };
                // The argument is a size of the box-sizing box: a content box adds its padding and border.
                let extra = {
                    let st = &self.nodes[usize::from(n)].style;
                    if st.box_sizing == BoxSizing::ContentBox && arg.is_some() {
                        let b = avail.unwrap_or(0.0);
                        resolve_lp(&st.padding.left, b, &self.calcs)
                            + resolve_lp(&st.padding.right, b, &self.calcs)
                            + resolve_lp(&st.border.left, b, &self.calcs)
                            + resolve_lp(&st.border.right, b, &self.calcs)
                    } else {
                        0.0
                    }
                };
                cap.map_or(max, |c| max.min(c + extra).max(min))
            }
            K::Stretch => return None,
        };
        Some(crate::layout::coordinator::calc::quantize_input(width))
    }

    /// `dispatch` for a node whose `width_kw` is taken out (it never recurses into the keyword).
    fn dispatch_plain(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        self.dispatch(n, inputs, block_ctx)
    }

    /// `dispatch`, then the out-of-flow positioning pass for the boxes this node is the containing
    /// block of (research R-07). It runs inside the cache closure and only for full passes, so a
    /// cache hit returns an output whose candidate list is already processed and measure passes
    /// never write the layouts of hoisted boxes.
    fn dispatch_and_position(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        if self.nodes[usize::from(n)].outer.is_some() {
            return self.dispatch_orthogonal(n, inputs);
        }
        self.dispatch_and_position_own(n, inputs, block_ctx)
    }

    /// An orthogonal flow root (CSS Writing Modes 4 section 7; 015 WM-1): the parent's inputs in the node's
    /// own frame (axes swapped), its own layout, and the output back in the parent's frame. An automatic
    /// inline size is fit-content against the parent's definite block size, else against the initial
    /// containing block's extent in that axis (section 7.3), less the node's margins on that axis.
    fn dispatch_orthogonal(&mut self, n: NodeId, inputs: LayoutInput) -> LayoutOutput {
        let i = usize::from(n);
        fn t<T>(s: Size<T>) -> Size<T> {
            Size {
                width: s.height,
                height: s.width,
            }
        }
        let mut inner = LayoutInput {
            known_dimensions: t(inputs.known_dimensions),
            known_dimensions_are_definite: t(inputs.known_dimensions_are_definite),
            parent_size: t(inputs.parent_size),
            available_space: t(inputs.available_space),
            axis: match inputs.axis {
                RequestedAxis::Horizontal => RequestedAxis::Vertical,
                RequestedAxis::Vertical => RequestedAxis::Horizontal,
                RequestedAxis::Both => RequestedAxis::Both,
            },
            vertical_margins_are_collapsible: Line::FALSE,
            ..inputs
        };
        // A min-content probe of the parent in this axis gets the box's own min-content inline size, and so does a
        // max-content probe of a grid's track sizing (WPT `grid-template-rows-fit-content-001`); otherwise the
        // inline size is fit-content (section 7.3: a block parent's max-content available height is no probe).
        let intrinsic = match inner.available_space.width {
            AvailableSpace::MinContent => true,
            AvailableSpace::MaxContent => self.nodes[i].grid_item,
            AvailableSpace::Definite(_) => false,
        };
        if inner.known_dimensions.width.is_none()
            && self.nodes[i].style.size.width.is_auto()
            && !intrinsic
        {
            let (margin_start, margin_end) = {
                let o = self.nodes[i].outer.as_deref().expect("orthogonal");
                let basis = inputs.parent_size.width.unwrap_or(0.0);
                let m = |v: LengthPercentageAuto| {
                    v.resolve_to_option(basis, |p, b| self.calcs.resolve(p, b))
                        .unwrap_or(0.0)
                };
                (m(o.margin.top), m(o.margin.bottom))
            };
            let icb = if self.nodes[i].wm.is_vertical() {
                self.viewport.height
            } else {
                self.viewport.width
            };
            let avail = inputs.parent_size.height.unwrap_or(icb) - margin_start - margin_end;
            let probe = |w: AvailableSpace| LayoutInput {
                run_mode: RunMode::ComputeSize,
                axis: RequestedAxis::Horizontal,
                known_dimensions: Size::NONE,
                available_space: Size {
                    width: w,
                    height: AvailableSpace::MaxContent,
                },
                ..inner
            };
            let min = self
                .dispatch_and_position_own(n, probe(AvailableSpace::MinContent), None)
                .size
                .width;
            let max = self
                .dispatch_and_position_own(n, probe(AvailableSpace::MaxContent), None)
                .size
                .width;
            inner.known_dimensions.width = Some(max.min(avail.max(min)).max(0.0));
        }
        let out = self.dispatch_and_position_own(n, inner, None);
        if inputs.run_mode == RunMode::PerformLayout {
            self.nodes[i].ortho_overflow = Some(out.scrollable_overflow_rect);
        }
        LayoutOutput {
            size: t(out.size),
            scrollable_overflow_rect: taffy::ScrollableOverflow::ZERO,
            baselines: Baselines::NONE,
            top_margin: taffy::CollapsibleMarginSet::ZERO,
            bottom_margin: taffy::CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: false,
            oof_candidates: OofCandidates::new(),
            oof_positioning_area: None,
        }
    }

    fn dispatch_and_position_own(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        let mut out = self.dispatch(n, inputs, block_ctx);
        if inputs.run_mode == RunMode::PerformLayout {
            for c in out.oof_candidates.iter() {
                if self.nodes[usize::from(c.node)].inline_cb_owner.is_some() {
                    // the last node to see the candidate is the one that claims it
                    self.inline_cb_pending.retain(|(_, p)| p.node != c.node);
                    self.inline_cb_pending.push((usize::from(n), *c));
                }
            }
            compute_oof_layout(self, n, &mut out);
        }
        out
    }

    /// Inline root: one custom leaf per IFC (AD-13). The container is taken out of the node while
    /// it runs so that it can lay out its atomic inlines through the tree (`atomic.rs`). It takes
    /// part in the block formatting context of its parent (`block_ctx`): the floats beside it
    /// shorten its lines and the floats in its text are placed there (`floats.rs`); a root that
    /// is a block formatting context of its own (no `block_ctx`) keeps the floats in its text and
    /// grows to hold them (CSS 2.1 section 10.6.7).
    fn layout_inline_root(
        &mut self,
        i: usize,
        inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        use super::floats::BfcSpace;
        use crate::layout::ifc::container::FloatSpace;
        // The out-of-flow boxes of the inline content: (node, position).
        let oof: Vec<(usize, Position)> = self.nodes[i]
            .inline_oof
            .iter()
            .map(|o| (o.node, self.nodes[o.node].style.position))
            .collect();
        let mut ifc = self.nodes[i].ifc.take().expect("inline root");
        let style = self.nodes[i].style.clone();
        let calcs = self.calcs.clone();
        let perform = inputs.run_mode == RunMode::PerformLayout;
        let mut first_baseline: Option<f32> = None;
        // Padding and border of the root, for the float space (content box inside the border box).
        let pb = {
            let basis = inputs.parent_size.width.unwrap_or(0.0);
            let side = |v: &LengthPercentage| resolve_lp(v, basis, &calcs);
            (
                side(&style.padding.left) + side(&style.border.left),
                side(&style.padding.right) + side(&style.border.right),
                side(&style.padding.top) + side(&style.border.top),
            )
        };
        let has_floats = ifc.has_floats();
        let contain_floats = block_ctx.is_none();
        // Inside a fragmentation root (015 FRAG-2b): the flow offset of the content-box top and the
        // fragmentainer size, for the line struts (`fragmentation::lines`).
        let frag_lines = if perform {
            self.frag
                .as_deref()
                .and_then(|f| f.abs.get(&i).map(|a| (a + pb.2, f.block_size)))
        } else {
            None
        };
        let frag_bands = self
            .frag
            .as_deref()
            .map(|f| f.bands.clone())
            .unwrap_or_default();
        if let (Some((base, _)), Some(f)) = (frag_lines, self.frag.as_deref_mut()) {
            // The floats of its lines are placed in its content box (015 MC-2: breakable floats).
            f.ifc_base.insert(i, base);
        }
        let mut pushed_lines: Vec<f32> = Vec::new();
        let mut first_line_pushed = false;
        let mut line_measure: (f32, Option<f32>) = (0.0, None);
        let mut first_lack = 0.0f32;
        let mut float_lines: Vec<f32> = Vec::new();
        let forced_lines: Vec<f32> = match (&frag_lines, self.frag.as_deref()) {
            (Some((base, _)), Some(f)) => f
                .line_breaks
                .get(&i)
                .filter(|(b, _)| (b - base).abs() <= crate::layout::fragmentation::EPS)
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut shared = block_ctx.map(|c| BfcSpace {
            ctx: c,
            x0: pb.0,
            y0: pb.2,
        });
        let mut out = {
            let mut atomics = TreeAtomics {
                tree: &mut *self,
                parent: i,
            };
            compute_leaf_layout(
                inputs,
                &style,
                |p, b| calcs.resolve(p, b),
                |known, avail| {
                    // A width the algorithm fixed, or the available width when none was: a block-level
                    // box with `width: auto` fills the space its parent gives it (CSS 2.1 §10.3.3).
                    // Without a fixed width but with a definite available one the box is sized by its
                    // content (shrink-to-fit: absolutely positioned boxes, CSS 2.1 §10.3.7, and
                    // intrinsic probes of floats and flex items): the fit-content width, never beyond
                    // the space offered unless the longest unbreakable run needs it.
                    let fit = |ifc: &mut InlineContainer, atomics: &mut TreeAtomics<'_>, w: f32| {
                        let i = ifc.intrinsic(atomics);
                        i.max_content.min(i.min_content.max(w))
                    };
                    // `compute_leaf_layout` hands the closure no known size in a full pass, and the
                    // fixed width as the definite available one instead: the algorithm's own input
                    // says whether the width was fixed.
                    let width = match (known.width, avail.width) {
                        (Some(w), _) => Some(w),
                        (None, AvailableSpace::Definite(w))
                            if inputs.known_dimensions.width.is_some() =>
                        {
                            Some(w)
                        }
                        (None, AvailableSpace::Definite(w)) => Some(fit(&mut ifc, &mut atomics, w)),
                        _ => None,
                    };
                    // Padding wider than the box leaves no room, never a negative one (WPT
                    // list-marker-position-crash: `padding: 0 2147483652%`).
                    let width = width.map(|w| w.max(0.0));
                    // The float space: the block formatting context the root belongs to, a fresh one
                    // when the root is its own BFC root (or has no known width).
                    let mut local_bfc = taffy::BlockFormattingContext::new();
                    let mut local_ctx = local_bfc.root_block_context();
                    let mut local = BfcSpace {
                        ctx: &mut local_ctx,
                        x0: pb.0,
                        y0: pb.2,
                    };
                    let use_shared = shared.is_some() && width.is_some();
                    if let Some(w) = width {
                        if use_shared {
                            if let Some(s) = shared.as_mut() {
                                s.ctx
                                    .apply_content_box_inset(taffy::FloatInsets::new(pb.0, pb.1));
                            }
                        } else if has_floats {
                            local.ctx.set_width(w + pb.0 + pb.1);
                            local
                                .ctx
                                .apply_content_box_inset(taffy::FloatInsets::new(pb.0, pb.1));
                        }
                    } else if has_floats {
                        local.ctx.set_width(pb.0 + pb.1);
                    }
                    let space: Option<&mut dyn FloatSpace> = if use_shared {
                        shared.as_mut().map(|s| s as &mut dyn FloatSpace)
                    } else if has_floats {
                        Some(&mut local as &mut dyn FloatSpace)
                    } else {
                        None
                    };
                    match width {
                        Some(w) if perform && frag_lines.is_some() => {
                            let (base, block_size) = frag_lines.unwrap_or_default();
                            let mut fs = crate::layout::fragmentation::lines::FragmentingSpace::new(
                                space, base, block_size, w,
                            );
                            // Pages of different heights (015 PAGE-1).
                            fs.bands = frag_bands.clone();
                            fs.forced = forced_lines.clone();
                            let mut h = ifc.perform(w, &mut atomics, Some(&mut fs));
                            if contain_floats {
                                h = h.max(fs.floats_bottom());
                            }
                            pushed_lines = std::mem::take(&mut fs.pushed);
                            first_line_pushed = fs.pushed_at_start;
                            line_measure = (fs.tallest, fs.shortage);
                            float_lines = std::mem::take(&mut fs.float_lines);
                            first_lack = fs.first_lack;
                            first_baseline =
                                ifc.final_outcome.as_ref().and_then(|f| f.first_baseline);
                            Size {
                                width: w,
                                height: known.height.unwrap_or(h),
                            }
                        }
                        Some(w) if perform => {
                            let mut space = space;
                            let mut h = ifc.perform(w, &mut atomics, space.as_deref_mut());
                            if contain_floats && let Some(sp) = space.as_deref() {
                                h = h.max(sp.floats_bottom());
                            }
                            first_baseline =
                                ifc.final_outcome.as_ref().and_then(|f| f.first_baseline);
                            Size {
                                width: w,
                                height: known.height.unwrap_or(h),
                            }
                        }
                        Some(w) => {
                            let mut space = space;
                            let m = ifc.measure(
                                AvailableInline::Definite(w),
                                &mut atomics,
                                space.as_deref_mut(),
                            );
                            let mut h = m.block_size;
                            if contain_floats && let Some(sp) = space.as_deref() {
                                h = h.max(sp.floats_bottom());
                            }
                            first_baseline = m.first_baseline;
                            Size {
                                width: w,
                                height: known.height.unwrap_or(h),
                            }
                        }
                        None => {
                            let intrinsic = ifc.intrinsic(&mut atomics);
                            let w = match avail.width {
                                AvailableSpace::MinContent => intrinsic.min_content,
                                _ => intrinsic.max_content,
                            };
                            // The width is known now: the floats go to a fresh context of that width.
                            let mut bfc2 = taffy::BlockFormattingContext::new();
                            let mut local_ctx2 = bfc2.root_block_context();
                            let mut local2 = BfcSpace {
                                ctx: &mut local_ctx2,
                                x0: pb.0,
                                y0: pb.2,
                            };
                            if has_floats {
                                local2.ctx.set_width(w + pb.0 + pb.1);
                                local2
                                    .ctx
                                    .apply_content_box_inset(taffy::FloatInsets::new(pb.0, pb.1));
                            }
                            let m = ifc.measure(
                                AvailableInline::Definite(w),
                                &mut atomics,
                                if has_floats {
                                    Some(&mut local2 as &mut dyn FloatSpace)
                                } else {
                                    None
                                },
                            );
                            let mut h = m.block_size;
                            first_baseline = m.first_baseline;
                            if perform {
                                let mut bfc3 = taffy::BlockFormattingContext::new();
                                let mut local_ctx3 = bfc3.root_block_context();
                                let mut local3 = BfcSpace {
                                    ctx: &mut local_ctx3,
                                    x0: pb.0,
                                    y0: pb.2,
                                };
                                if has_floats {
                                    local3.ctx.set_width(w + pb.0 + pb.1);
                                    local3.ctx.apply_content_box_inset(taffy::FloatInsets::new(
                                        pb.0, pb.1,
                                    ));
                                }
                                let hp = ifc.perform(
                                    w,
                                    &mut atomics,
                                    if has_floats {
                                        Some(&mut local3 as &mut dyn FloatSpace)
                                    } else {
                                        None
                                    },
                                );
                                if has_floats {
                                    h = h.max(hp).max(local3.floats_bottom());
                                }
                                first_baseline =
                                    ifc.final_outcome.as_ref().and_then(|f| f.first_baseline);
                            } else if has_floats {
                                h = h.max(local2.floats_bottom());
                            }
                            Size {
                                width: w,
                                height: known.height.unwrap_or(h),
                            }
                        }
                    }
                },
            )
        };
        self.nodes[i].ifc = Some(ifc);
        // The floats of the lines (content-box top and bottom of their margin boxes), for widows (015 MC-2).
        let line_floats: Vec<(f32, f32)> = if frag_lines.is_some() {
            self.nodes[i]
                .children
                .iter()
                .map(|&c| &self.nodes[c])
                .filter(|c| c.frag_facts.float && c.in_ifc)
                .map(|c| {
                    (
                        c.layout.location.y - c.layout.margin.top,
                        c.layout.location.y + c.layout.size.height + c.layout.margin.bottom,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        if let Some(f) = self.frag.as_deref_mut() {
            if first_line_pushed {
                f.leading_pushed.insert(i);
                if f.measure {
                    f.lead_lack.insert(i, first_lack);
                }
            }
            if let Some((base, _)) = frag_lines {
                for y in float_lines {
                    f.force_line(i, base, y);
                }
            }
            if let Some((base, _)) = frag_lines
                && let Some(fin) = self.nodes[i]
                    .ifc
                    .as_ref()
                    .and_then(|c| c.final_outcome.as_ref())
            {
                let lines: Vec<(f32, f32)> = fin.lines.iter().map(|l| (l.y, l.height)).collect();
                if f.measure {
                    // Balancing: a line is an unbreakable unit (the line breaker asks the space for room with a
                    // provisional height, so the final lines are the measure).
                    f.tallest = lines.iter().map(|l| l.1).fold(f.tallest, f32::max);
                    if let Some(l) = lines.first() {
                        f.first_unit.insert(i, pb.2 + l.0 + l.1);
                    }
                }
                let facts = self.nodes[i].frag_facts;
                f.orphans_widows(
                    i,
                    base,
                    &lines,
                    &line_floats,
                    facts.orphans as usize,
                    facts.widows as usize,
                );
            }
            if f.measure {
                f.tallest = f.tallest.max(line_measure.0);
                if let Some(s) = line_measure
                    .1
                    .filter(|s| *s > crate::layout::fragmentation::EPS)
                {
                    f.shortage = Some(f.shortage.map_or(s, |x| x.min(s)));
                }
            }
            for offset in pushed_lines {
                use crate::layout::fragmentation::{BreakAppeal, BreakKind, BreakToken};
                f.tokens.push(BreakToken {
                    node: i,
                    offset,
                    kind: BreakKind::PushedLine,
                    appeal: BreakAppeal::Perfect,
                });
            }
        }
        let calcs = &calcs;
        // FR-042: the first baseline of a block container is its first line box's, measured from
        // the top of its border box (research R-12). Padding and border above the content add.
        // Layout containment: the box has no baseline (CSS Containment 2 section 3.2).
        if let Some(b) = first_baseline.filter(|_| !style.contain.contains(taffy::Contain::LAYOUT))
        {
            let basis = inputs.parent_size.width.unwrap_or(0.0);
            let above = resolve_lp(&style.padding.top, basis, calcs)
                + resolve_lp(&style.border.top, basis, calcs);
            out.baselines = Baselines::from_first(Some(b + above));
        }
        if perform {
            let basis = inputs.parent_size.width.unwrap_or(0.0);
            let side = |v: &LengthPercentage, calcs: &CalcTable| resolve_lp(v, basis, calcs);
            let st = &style;
            let padding = taffy::Rect {
                top: side(&st.padding.top, calcs),
                right: side(&st.padding.right, calcs),
                bottom: side(&st.padding.bottom, calcs),
                left: side(&st.padding.left, calcs),
            };
            let border = taffy::Rect {
                top: side(&st.border.top, calcs),
                right: side(&st.border.right, calcs),
                bottom: side(&st.border.bottom, calcs),
                left: side(&st.border.left, calcs),
            };
            // FR-064: an unbreakable run or an over-wide line is scrollable overflow of the box.
            // The leaf reports the available width as the content width; the lines say what the
            // paragraph really occupies (padding-box frame, CSS Overflow 3 section 2.2).
            if let Some(f) = self.nodes[i]
                .ifc
                .as_ref()
                .and_then(|c| c.final_outcome.as_ref())
            {
                let mut rect = out.scrollable_overflow_rect.rect();
                // (A line box's width is the room the line was offered, not what it holds: the
                // text pieces say how far the content really reaches.)
                for (li, l) in f.lines.iter().enumerate() {
                    rect.bottom = rect.bottom.max(padding.top + l.y + l.height);
                    for piece in f.paragraph.text_pieces(li) {
                        rect.left = rect.left.min(padding.left + piece.x);
                        rect.right = rect.right.max(padding.left + piece.x + piece.width);
                    }
                    // A line `text-overflow` cut still reaches the whole text (L-GEO-r9).
                    if let Some(end) = f.paragraph.uncut_content_end(li) {
                        rect.right = rect.right.max(padding.left + end);
                    }
                }
                out.scrollable_overflow_rect = taffy::ScrollableOverflow::new(
                    rect,
                    out.scrollable_overflow_rect.is_reversed(),
                );
            }
            // The floats of the text are scrollable overflow of the root too (they overflow it when
            // they hang below its last line).
            if has_floats {
                let mut rect = out.scrollable_overflow_rect.rect();
                let floats: Vec<usize> = self.nodes[i]
                    .atomics
                    .iter()
                    .filter(|a| {
                        self.nodes[a.1]
                            .css
                            .as_ref()
                            .is_some_and(|c| c.float != crate::layout::box_tree::FloatValue::None)
                    })
                    .map(|a| a.1)
                    .collect();
                for k in floats {
                    let n = &self.nodes[k];
                    rect.left = rect.left.min(padding.left + n.layout.location.x);
                    rect.top = rect.top.min(padding.top + n.layout.location.y);
                    rect.right = rect
                        .right
                        .max(padding.left + n.layout.location.x + n.layout.size.width);
                    rect.bottom = rect
                        .bottom
                        .max(padding.top + n.layout.location.y + n.layout.size.height);
                }
                out.scrollable_overflow_rect = taffy::ScrollableOverflow::new(
                    rect,
                    out.scrollable_overflow_rect.is_reversed(),
                );
            }
            // Out-of-flow boxes inside an atomic inline that the atomic does not claim bubble up
            // like those of a block child (Taffy translates a child's candidates by its location;
            // an atomic's location is relative to this root's content box).
            let mut bubbled = OofCandidates::new();
            let atoms: Vec<usize> = self.nodes[i].atomics.iter().map(|a| a.1).collect();
            for k in atoms {
                let mut c = self.nodes[k].atomic_oof.take();
                if !c.is_empty() {
                    let loc = self.nodes[k].layout.location;
                    c.translate(taffy::Point {
                        x: loc.x + border.left + padding.left,
                        y: loc.y + border.top + padding.top,
                    });
                    bubbled.append(&mut c);
                }
            }
            if !oof.is_empty() || !bubbled.is_empty() {
                // Inline content is not a Taffy tree: hand the out-of-flow boxes to the
                // positioning pass (run by the caller) as this node's candidates. Their static
                // position is the IFC's (the placeholder's place in its line, beside any floats;
                // a block-level box sits at the start edge below the content before it).
                let rtl = st.direction == taffy::Direction::Rtl;
                let x = if rtl {
                    out.size.width - border.right - padding.right
                } else {
                    border.left + padding.left
                };
                let edge = if rtl {
                    AxisStaticEdge::End
                } else {
                    AxisStaticEdge::Start
                };
                let y = border.top + padding.top;
                let statics: Vec<(u64, f32, f32)> = self.nodes[i]
                    .ifc
                    .as_ref()
                    .map(|c| c.oof_static.clone())
                    .unwrap_or_default();
                let mut candidates = OofCandidates::new();
                for (order, (node, position)) in oof.iter().enumerate() {
                    let item = self.nodes[*node].box_id.0 as u64;
                    let at = statics.iter().find(|(it, ..)| *it == item);
                    let (sx, sy) = match at {
                        Some(&(_, ax, ay)) => (if ax.is_nan() || rtl { x } else { x + ax }, y + ay),
                        None => (x, y),
                    };
                    candidates.push(OofCandidate {
                        node: NodeId::from(*node),
                        order: order as u32,
                        position: *position,
                        static_position: taffy::Point {
                            x: AxisStaticPosition::from_edge(sx, edge),
                            y: AxisStaticPosition::from_edge(sy, AxisStaticEdge::Start),
                        },
                    });
                }
                candidates.append(&mut bubbled);
                out.oof_candidates = candidates;
                out.oof_positioning_area = Some(OofPositioningArea {
                    size: taffy::Size {
                        width: (out.size.width - border.left - border.right).max(0.0),
                        height: (out.size.height - border.top - border.bottom).max(0.0),
                    },
                    offset: taffy::Point {
                        x: border.left,
                        y: border.top,
                    },
                });
            }
        }
        out
    }
}

/// The out-of-flow boxes inside inline content, in tree order, with whether each was inline-level
/// before blockification.
fn collect_inline_oof(boxes: &BoxTree, children: &[BoxId], out: &mut Vec<(BoxId, bool)>) {
    for &c in children {
        let b = boxes.get(c);
        match &b.kind {
            BoxKind::InlineBox => collect_inline_oof(boxes, &b.children, out),
            _ if b.is_out_of_flow() => out.push((c, b.style.original_inline)),
            _ => {}
        }
    }
}

/// The in-flow atomic inlines and floats of the inline root `id`, found through its inline boxes,
/// in tree order.
fn collect_atomics(boxes: &BoxTree, id: BoxId, out: &mut Vec<BoxId>) {
    for &c in &boxes.get(id).children {
        let cb = boxes.get(c);
        match cb.kind {
            BoxKind::InlineBox => collect_atomics(boxes, c, out),
            _ if (cb.is_atomic_inline() || cb.is_float()) && !cb.is_out_of_flow() => out.push(c),
            _ => {}
        }
    }
}

pub(crate) fn resolve_lp(v: &LengthPercentage, basis: f32, calcs: &CalcTable) -> f32 {
    match v.expand() {
        ExpandedLengthPercentage::Length(l) => l,
        ExpandedLengthPercentage::Percent(p) => p * basis,
        ExpandedLengthPercentage::Calc(ptr) => calcs.resolve(ptr, basis),
    }
}

pub(crate) struct ChildIter<'a>(std::slice::Iter<'a, usize>);
impl Iterator for ChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        self.0.next().map(|i| NodeId::from(*i))
    }
}

impl TraversePartialTree for HostTree {
    type ChildIter<'a> = ChildIter<'a>;
    fn child_ids(&self, n: NodeId) -> ChildIter<'_> {
        if self.empty_probe == Some(usize::from(n)) {
            return ChildIter([].iter());
        }
        ChildIter(self.nodes[usize::from(n)].children.iter())
    }
    fn child_count(&self, n: NodeId) -> usize {
        if self.empty_probe == Some(usize::from(n)) {
            return 0;
        }
        self.nodes[usize::from(n)].children.len()
    }
    fn get_child_id(&self, n: NodeId, i: usize) -> NodeId {
        NodeId::from(self.nodes[usize::from(n)].children[i])
    }
}
impl TraverseTree for HostTree {}

impl LayoutPartialTree for HostTree {
    type CustomIdent = String;
    type CoreContainerStyle<'a>
        = HostStyle<'a>
    where
        Self: 'a;

    fn get_core_container_style(&self, n: NodeId) -> HostStyle<'_> {
        self.host_style(n)
    }
    fn resolve_calc_value(&self, val: *const (), basis: f32) -> f32 {
        self.calcs.resolve(val, basis)
    }
    fn set_unrounded_layout(&mut self, n: NodeId, layout: &Layout) {
        self.nodes[usize::from(n)].layout = *layout;
    }
    fn compute_child_layout(&mut self, n: NodeId, inputs: LayoutInput) -> LayoutOutput {
        let out = compute_cached_layout(self, n, inputs, |tree, n, inputs| {
            tree.nodes[usize::from(n)].computed = true;
            tree.dispatch_and_position(n, inputs, None)
        });
        self.note_baselines(n, &inputs, &out);
        out
    }

    /// Flex and grid fragmentation (Taffy P-008-5, 015 STAB-1): a breakable container placed in the root being
    /// laid out (its offset is known).
    fn frag_wants(&mut self, container: NodeId) -> bool {
        let c = usize::from(container);
        self.frag
            .as_deref()
            .is_some_and(|f| f.root != c && f.abs.contains_key(&c))
    }

    fn frag_item_grow(&mut self, _container: NodeId, item: NodeId) -> f32 {
        self.frag
            .as_deref()
            .and_then(|f| f.grow.get(&usize::from(item)).copied())
            .unwrap_or(0.0)
    }

    fn frag_place_runs(
        &mut self,
        container: NodeId,
        runs: &mut [taffy::FragRun],
        items: &[taffy::FragRunItem],
    ) {
        let c = usize::from(container);
        let grid = self.nodes[c].style.display == Display::Grid;
        let items: Vec<crate::layout::fragmentation::flexgrid::RunItem> = items
            .iter()
            .map(|it| {
                let n = usize::from(it.node);
                crate::layout::fragmentation::flexgrid::RunItem {
                    node: n,
                    first: it.first as usize,
                    last: it.last as usize,
                    offset: it.offset,
                    size: it.size,
                    facts: self.nodes[n].frag_facts,
                }
            })
            .collect();
        if let Some(f) = self.frag.as_deref_mut() {
            f.place_runs(c, grid, runs, &items);
        }
    }

    fn frag_begin_item(&mut self, container: NodeId, item: NodeId, y: f32) {
        if let Some(f) = self.frag.as_deref_mut()
            && let Some(&p) = f.abs.get(&usize::from(container))
        {
            f.abs.insert(usize::from(item), p + y);
        }
    }
}

impl LayoutContainingBlock for HostTree {
    type OofItemStyle<'a>
        = HostStyle<'a>
    where
        Self: 'a;
    fn get_oof_item_style(&self, n: NodeId) -> HostStyle<'_> {
        self.child_style(n)
    }
    fn get_detailed_layout_info(&self, n: NodeId) -> &taffy::DetailedLayoutInfo<String> {
        &self.nodes[usize::from(n)].grid_info
    }
    fn clear_hoisted_children(&mut self, n: NodeId) {
        self.nodes[usize::from(n)].hoisted.clear();
    }
    fn add_hoisted_children(&mut self, n: NodeId, hoisted: &[NodeId]) {
        self.nodes[usize::from(n)]
            .hoisted
            .extend(hoisted.iter().map(|h| usize::from(*h)));
    }
}

impl CacheTree for HostTree {
    fn cache_get(&mut self, n: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        // Inside a fragmentation root a final layout depends on the block offset, which the cache key
        // does not hold (015 FRAG-2b): the final-layout entries are bypassed there, but a layout that stayed
        // inside one fragmentainer is kept for the next pass (015 MC-2, `FragCacheEntry`).
        let i = usize::from(n);
        if input.run_mode == RunMode::PerformLayout {
            if self.frag.is_some() {
                return self.frag_cache_get(i, input);
            }
            self.nodes[i].frag_cache = None;
        }
        self.nodes[i].cache.get(input)
    }
    fn cache_store(&mut self, n: NodeId, input: &LayoutInput, out: LayoutOutput) {
        if self.frag.is_some() && input.run_mode == RunMode::PerformLayout {
            self.frag_cache_store(usize::from(n), input, out);
            return;
        }
        let node = &mut self.nodes[usize::from(n)];
        // A seeded node (015 PERF-1 replay) that runs a measuring layout lays its descendants out again for
        // that measure, so its final-layout entry from the previous pass no longer matches their layouts: a
        // later hit would leave them at the measure's geometry (015 PERF-2: an inline-block in a table cell
        // re-measured at a new grid width kept its max-content layout). The entry is replaced by one no
        // request matches (same cache mode, so the measures stay).
        if input.run_mode == RunMode::ComputeSize && node.seed_layout.is_some() {
            let unmatchable = LayoutInput {
                run_mode: RunMode::PerformLayout,
                known_dimensions: taffy::Size {
                    width: Some(f32::NAN),
                    height: Some(f32::NAN),
                },
                ..*input
            };
            node.cache.store(&unmatchable, LayoutOutput::HIDDEN);
        }
        node.cache.store(input, out)
    }
    fn cache_clear(&mut self, n: NodeId) {
        self.nodes[usize::from(n)].cache.clear();
    }
}

impl LayoutBlockContainer for HostTree {
    type BlockContainerStyle<'a>
        = HostStyle<'a>
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = HostStyle<'a>
    where
        Self: 'a;
    fn get_block_container_style(&self, n: NodeId) -> HostStyle<'_> {
        self.host_style(n)
    }
    fn get_block_child_style(&self, n: NodeId) -> HostStyle<'_> {
        self.child_style(n)
    }
    /// A block child lays out in its parent's block formatting context unless it establishes
    /// its own (CSS 2.1 §9.4.1): this is what lets an empty block's margins collapse through
    /// (§8.3.1) instead of every box acting as a BFC root.
    fn compute_block_child_layout(
        &mut self,
        n: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut taffy::BlockContext<'_>>,
    ) -> LayoutOutput {
        // A child in its parent's block formatting context sees the floats there (015 MC-2, `FragCacheEntry`).
        self.bfc_floats = block_ctx.as_deref().is_some_and(|c| c.has_floats());
        let out = compute_cached_layout(self, n, inputs, |tree, n, inputs| {
            tree.nodes[usize::from(n)].computed = true;
            tree.dispatch_and_position(n, inputs, block_ctx)
        });
        self.note_baselines(n, &inputs, &out);
        out
    }

    /// Block fragmentation (patch P-008-3, 015 FRAG-2b): the offset `child` is laid out at.
    fn begin_block_child(&mut self, parent: NodeId, child: NodeId, estimated_y: f32) {
        if let Some(f) = self.frag.as_deref_mut() {
            f.begin_child(usize::from(parent), usize::from(child), estimated_y);
        }
        // A row of a multicol container (015 MC-1): where it starts, for the height left to it.
        let c = usize::from(child);
        if matches!(
            self.nodes[c].frag_role,
            crate::layout::fragmentation::FragRole::FlowThread
        ) && let Some(g) = self.nodes[c].flow_geom.as_deref_mut()
        {
            let y = (estimated_y - g.content_top).max(0.0);
            if (g.row_y - y).abs() > crate::layout::fragmentation::EPS {
                g.row_y = y;
                self.nodes[c].cache.clear();
            }
        }
    }

    /// Block fragmentation (patch P-008-3): the strut of `child` once it is placed.
    fn end_block_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        placement: taffy::BlockChildPlacement,
    ) -> f32 {
        let Some(f) = self.frag.as_deref_mut() else {
            return 0.0;
        };
        let c = usize::from(child);
        let mut facts = self.nodes[c].frag_facts;
        facts.monolithic |= self.nodes[c].in_ifc;
        let mut placement: crate::layout::fragmentation::breaks::Placement = placement.into();
        {
            let pst = &self.nodes[usize::from(parent)].style;
            let side = |v: &LengthPercentage| resolve_lp(v, 0.0, &self.calcs);
            placement.parent_lead = side(&pst.border.top) + side(&pst.padding.top);
        }
        if f.measure && !facts.monolithic {
            // Balancing (015 MC-2): a box's block-start and block-end border and padding are not broken (measured:
            // the empty piece of a box split before a spanner keeps its 8 px of border and padding in one column).
            let st = &self.nodes[c].style;
            let side = |v: &LengthPercentage| resolve_lp(v, 0.0, &self.calcs);
            let top = side(&st.border.top) + side(&st.padding.top);
            let bottom = side(&st.border.bottom) + side(&st.padding.bottom);
            f.tallest = f.tallest.max(top).max(bottom);
            placement.trailing = bottom;
        }
        f.end_child(usize::from(parent), c, placement, facts)
    }
}

impl LayoutFlexboxContainer for HostTree {
    type FlexboxContainerStyle<'a>
        = HostStyle<'a>
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = HostStyle<'a>
    where
        Self: 'a;
    fn get_flexbox_container_style(&self, n: NodeId) -> HostStyle<'_> {
        self.host_style(n)
    }
    fn get_flexbox_child_style(&self, n: NodeId) -> HostStyle<'_> {
        self.child_style(n)
    }
}

impl LayoutGridContainer for HostTree {
    type GridContainerStyle<'a>
        = HostStyle<'a>
    where
        Self: 'a;
    type GridItemStyle<'a>
        = HostStyle<'a>
    where
        Self: 'a;
    fn get_grid_container_style(&self, n: NodeId) -> HostStyle<'_> {
        self.host_style(n)
    }
    fn get_grid_child_style(&self, n: NodeId) -> HostStyle<'_> {
        self.child_style(n)
    }
    fn set_detailed_grid_info(&mut self, n: NodeId, info: taffy::DetailedGridInfo<String>) {
        self.nodes[usize::from(n)].grid_info = taffy::DetailedLayoutInfo::Grid(Box::new(info));
    }
    /// A subgrid's tracks, gaps and line names as its parent grid recorded them (Taffy patch P-008-4). The
    /// subgrid's layout depends on them, which its cache key does not hold: a change clears its cache.
    fn set_subgrid_info(&mut self, n: NodeId, info: Option<taffy::SubgridInfo<String>>) {
        let node = &mut self.nodes[usize::from(n)];
        if node.subgrid.as_deref() != info.as_ref() {
            node.subgrid = info.map(Box::new);
            node.cache.clear();
        }
    }
    fn subgrid_info(&self, n: NodeId) -> Option<taffy::SubgridInfo<String>> {
        self.nodes[usize::from(n)].subgrid.as_deref().cloned()
    }
}

pub(crate) fn push_report(reports: &mut Vec<LayoutReport>, key: ReportKey) {
    if !reports.iter().any(|r| r.key == key) {
        reports.push(LayoutReport {
            key,
            code: Code::UnsupportedFeature,
            reason: UnsupportedReason::NotConsumedByStage,
        });
    }
}

/// FR-022 consumption check for layout-stage properties (treated as initial: never mapped), and the layout-stage
/// reports of box properties (shared by the screen and the paged layout).
fn consumption_reports(boxes: &BoxTree, reports: &mut Vec<LayoutReport>) {
    for b in &boxes.boxes {
        if let BoxOwner::Element(node) = b.owner {
            for f in &b.style.unconsumed_layout {
                let location = ReportLocation::Element { node };
                push_report(
                    reports,
                    ReportKey {
                        feature: *f,
                        stage: Stage::Layout,
                        document: boxes.document,
                        location,
                    },
                );
            }
            // FR-047: `visibility: collapse` on a flex item behaves as `hidden` and is reported.
            let in_flex = b.is_block()
                && b.parent.is_some_and(|p| {
                    boxes.get(p).is_block() && boxes.get(p).inner == InnerDisplay::Flex
                });
            // FR-079 (T103): `visibility: collapse` on a column or column group acts as `hidden` (the
            // space is kept) and is reported. On a row or row group it is implemented (L-LAYFIX-r8,
            // CSS 2.1 §17.5.5: the row takes no space), so it is not reported.
            let in_table = matches!(
                b.tag,
                BoxTag::Table(TableTag::Column { .. } | TableTag::ColumnGroup { .. })
            );
            // 015 WM-1c: an orthogonal subgrid is laid out as a grid of its own (`add_node`), reported.
            if (b.style.grid.column_subgrid || b.style.grid.row_subgrid)
                && b.parent.is_some_and(|p| {
                    crate::layout::writing_mode::orthogonal(
                        boxes.get(p).style.writing_mode,
                        b.style.writing_mode,
                    )
                })
                && let Some(feature) = FeatureId::new("css.value.subgrid-orthogonal")
            {
                push_report(
                    reports,
                    ReportKey {
                        feature,
                        stage: Stage::Layout,
                        document: boxes.document,
                        location: ReportLocation::Element { node },
                    },
                );
            }
            if (in_flex || in_table) && b.style.visibility == VisibilityValue::Collapse {
                let feature = FeatureId::new("css.value.visibility-collapse")
                    .expect("css.value root is registered");
                let location = ReportLocation::Element { node };
                push_report(
                    reports,
                    ReportKey {
                        feature,
                        stage: Stage::Layout,
                        document: boxes.document,
                        location,
                    },
                );
            }
        }
    }
}

impl HostTree {
    /// The page flow (015 PAGE-1): a block node `width` wide above `root` (the root element's node), whose layout is
    /// the fragmentation root of the pages (`bands`: the page area heights, a leading virtual band first in a later
    /// width run); `None` lays it out unfragmented (the preliminary layout of a width run).
    fn push_page_flow(
        &mut self,
        root: usize,
        width: f32,
        height: f32,
        bands: Option<crate::layout::fragmentation::Bands>,
    ) -> usize {
        let p = self.push_icb_parent(
            root,
            Viewport {
                width,
                height,
                scale: self.viewport.scale,
            },
        );
        self.nodes[p].style.size.height = Dimension::auto();
        if let Some(bands) = bands {
            let first = bands.size_of(0, height);
            self.nodes[p].frag_role = crate::layout::fragmentation::FragRole::FlowThread;
            self.nodes[p].flow_geom =
                Some(Box::new(crate::layout::fragmentation::FlowThreadGeometry {
                    block_size: first,
                    column_width: width,
                    count: 1,
                    fill_auto: true,
                    last_row: true,
                    bands,
                    ..Default::default()
                }));
        }
        p
    }

    /// The flow offset of node `target`'s border-box top below node `from` (in-flow and hoisted children).
    fn offset_below(&self, from: usize, target: usize) -> Option<f32> {
        if from == target {
            return Some(0.0);
        }
        let n = &self.nodes[from];
        let shift = if n.ifc.is_some() {
            n.layout.border.top + n.layout.padding.top
        } else {
            0.0
        };
        for &c in n.children.iter() {
            if c <= target && target < self.nodes[c].subtree_end.max(c + 1) {
                return self.offset_below(c, target).map(|y| {
                    y + self.nodes[c].layout.location.y
                        + if self.nodes[c].in_ifc { shift } else { 0.0 }
                });
            }
        }
        None
    }
}

/// One layout of the page flow of `boxes` at `width` (015 PAGE-1).
// The coordinator keeps layout inputs and the diagnostic output sink explicit.
#[allow(clippy::too_many_arguments)]
fn paged_tree(
    boxes: &BoxTree,
    root: BoxId,
    width: f32,
    height: f32,
    bands: Option<crate::layout::fragmentation::Bands>,
    viewport: Viewport,
    text: &mut TextServices,
    reports: &mut Vec<LayoutReport>,
) -> (HostTree, usize, usize) {
    let mut tree = HostTree {
        nodes: Vec::new(),
        calcs: CalcTable::default(),
        container_width: Vec::new(),
        inline_cb_pending: Vec::new(),
        empty_probe: None,
        frag: None,
        bfc_floats: false,
        frag_layouts: (0, 0),
        ifc_seeds: Default::default(),
        viewport,
        text_ptr: std::ptr::null_mut(),
        paged: true,
    };
    let root_idx = tree.add(boxes, root, None, text);
    // The root's percentage heights resolve against the page area (015 STAB-1, measured in the reference's print path:
    // `html { height: 100% }` is as tall as the first page area), not against the page flow, which is auto.
    let area = bands.as_ref().map_or(height, |b| b.size_of(0, height));
    let per = |d: Dimension| {
        if d.tag() == taffy::CompactLength::PERCENT_TAG {
            Dimension::length(d.value() * area)
        } else {
            d
        }
    };
    let per_limit = |d: LengthPercentageAuto| {
        let raw = d.into_raw();
        if raw.tag() == taffy::CompactLength::PERCENT_TAG {
            LengthPercentageAuto::length(raw.value() * area)
        } else {
            d
        }
    };
    let st = &mut tree.nodes[root_idx].style;
    st.size.height = per(st.size.height);
    st.min_size.height = per_limit(st.min_size.height);
    st.max_size.height = per_limit(st.max_size.height);
    let flow = tree.push_page_flow(root_idx, width, height, bands);
    let avail = Size {
        width: AvailableSpace::Definite(quantize_input(width)),
        height: AvailableSpace::Definite(quantize_input(height)),
    };
    tree.text_ptr = &mut *text as *mut TextServices;
    compute_root_layout(&mut tree, NodeId::from(flow), avail);
    tree.text_ptr = std::ptr::null_mut();
    resolve_inline_containing_blocks(boxes, &mut tree, root_idx, viewport, reports);
    (tree, root_idx, flow)
}

/// Paged layout (015 PAGE-1, `crate::layout::fragmentation::paged`): the pages of `boxes` with the page boxes `geom`
/// gives, and how many layouts of the flow it took.
pub(crate) fn layout_boxes_paged(
    boxes: &BoxTree,
    geom: &mut dyn crate::layout::fragmentation::paged::PageGeometry,
    scale: f32,
    text: &mut TextServices,
    reports: &mut Vec<LayoutReport>,
    diagnostics: &mut Vec<ErrorRecord>,
) -> (Vec<crate::layout::fragmentation::paged::PagedPage>, u32) {
    use crate::layout::fragmentation::paged::{
        MAX_PAGE_PASSES, PageBox, PageInfo, PagedPage, name_of, page_names, used_page,
    };
    use crate::layout::fragmentation::{Bands, EPS};
    text.set_device_scale(scale);
    consumption_reports(boxes, reports);
    let viewport_of = |pb: &PageBox| Viewport {
        width: pb.width,
        height: pb.height,
        scale,
    };
    let Some(root) = boxes.root else {
        let info = PageInfo::assumed(0);
        let pb = geom.page_box(&info);
        let tree = FragmentTree::empty(viewport_of(&pb));
        return (
            vec![PagedPage {
                info,
                page_box: pb,
                fragments: tree,
                flow_top: 0.0,
                flow_bottom: 0.0,
                run: 0,
            }],
            0,
        );
    };
    if boxes.get(root).style.writing_mode.is_vertical()
        && let Some(feature) = FeatureId::new("css.at-rule.page-vertical")
    {
        // Pages in a vertical writing mode are laid out horizontally (015 PAGE-1, reported limit).
        push_report(
            reports,
            ReportKey {
                feature,
                stage: Stage::Layout,
                document: boxes.document,
                location: ReportLocation::Element {
                    node: boxes.get(root).owner.node(),
                },
            },
        );
    }
    let root_name = used_page(boxes, root);
    let named = |info: PageInfo, n: Option<u32>| PageInfo {
        name: n.and_then(name_of),
        ..info
    };
    let mut assumed: Vec<PageInfo> = (0..2)
        .map(|i| named(PageInfo::assumed(i), root_name))
        .collect();
    let mut layouts = 0u32;
    let mut result = Vec::new();
    for pass in 0..MAX_PAGE_PASSES {
        let assumed_boxes: Vec<PageBox> = assumed.iter().map(|i| geom.page_box(i)).collect();
        let mut pages: Vec<PagedPage> = Vec::new();
        let mut piece_counts = std::collections::HashMap::new();
        let mut run_start = 0usize;
        // The resume point of a later width run: the host node that starts its first page, and whether a forced
        // break put it there (its margin is kept).
        let mut resume: Option<(usize, bool)> = None;
        let mut run = 0u32;
        // The page box of a later run's first page (its actual selectors are known from the run before).
        let mut run_box: Option<PageBox> = None;
        loop {
            let first_box =
                run_box.unwrap_or(assumed_boxes[run_start.min(assumed_boxes.len() - 1)]);
            let width = first_box.content_width();
            let mut heights: Vec<f32> = assumed_boxes[run_start.min(assumed_boxes.len() - 1)..]
                .iter()
                .map(PageBox::content_height)
                .collect();
            if heights.is_empty() {
                heights.push(first_box.content_height());
            }
            heights[0] = first_box.content_height();
            let vp = viewport_of(&first_box);
            // A later run: everything before the resume point falls into one virtual leading band.
            let lead = if let Some((r, forced)) = resume {
                let (pre, _, flow) = paged_tree(
                    boxes,
                    root,
                    width,
                    first_box.content_height(),
                    None,
                    vp,
                    text,
                    &mut Vec::new(),
                );
                layouts += 1;
                let y = pre.offset_below(flow, r).unwrap_or(0.0);
                let m = if forced {
                    pre.nodes[r].layout.margin.top.max(0.0)
                } else {
                    0.0
                };
                Some((y - m).max(EPS))
            } else {
                None
            };
            let lead_n = usize::from(lead.is_some());
            let sizes: Vec<f32> = lead.into_iter().chain(heights.iter().copied()).collect();
            let bands = Bands {
                sizes: Some(std::sync::Arc::new(sizes)),
                first_page: run_start as i32 - lead_n as i32,
                lead: lead_n == 1,
            };
            let (tree, _root_idx, flow) = paged_tree(
                boxes,
                root,
                width,
                first_box.content_height(),
                Some(bands),
                vp,
                text,
                reports,
            );
            layouts += 1;
            let g = tree.nodes[flow]
                .flow_geom
                .as_deref()
                .cloned()
                .unwrap_or_default();
            let all = crate::layout::fragment::builder::page_bands(boxes, &tree, flow);
            let band_rects: Vec<(f32, f32)> = all
                .iter()
                .map(|b| (b.block_offset, b.block_offset + b.block_size))
                .collect();
            let names = page_names(&g.starts, &band_rects, root_name);
            let mut end = all.len();
            let mut next_resume = None;
            // Where the content width changes, the run ends at the first such page that starts at a block-level box.
            let mut next_box = None;
            for k in lead_n + 1..all.len() {
                let pi = run_start + k - lead_n;
                let info = named(
                    PageInfo {
                        index: pi as u32,
                        first: pi == 0,
                        blank: g.blank.contains(&(k as u32)),
                        left: pi % 2 == 1,
                        name: None,
                    },
                    names[k],
                );
                let pb = geom.page_box(&info);
                if (pb.content_width() - width).abs() <= EPS {
                    continue;
                }
                let top = all[k].block_offset;
                // The outermost box whose border box (or, after a forced break, margin box) starts the page.
                let forced = g.tokens.iter().any(|t| {
                    t.kind == crate::layout::fragmentation::BreakKind::Forced
                        && (t.offset - top).abs() <= EPS
                });
                let starter = g.starts.iter().rev().find(|(y, _, n)| {
                    let m = if forced {
                        tree.nodes[*n].layout.margin.top.max(0.0)
                    } else {
                        0.0
                    };
                    (y - m - top).abs() <= EPS
                        && tree.nodes[*n].frag_role == crate::layout::fragmentation::FragRole::None
                });
                match starter {
                    Some(&(_, _, n)) => {
                        end = k;
                        next_resume = Some((n, forced));
                        next_box = Some(pb);
                        break;
                    }
                    None => {
                        // A paragraph broken across the change: this page keeps the run's width (reported).
                        if let Some(feature) = FeatureId::new("css.at-rule.page-width-change") {
                            push_report(
                                reports,
                                ReportKey {
                                    feature,
                                    stage: Stage::Layout,
                                    document: boxes.document,
                                    location: ReportLocation::Element {
                                        node: boxes.get(root).owner.node(),
                                    },
                                },
                            );
                        }
                    }
                }
            }
            if std::env::var_os("UI4ME_PAGED_DEBUG").is_some() {
                eprintln!(
                    "[paged] pass {pass} run {run} start {run_start} width {width} lead {lead:?} resume {resume:?} bands {:?} end {end} next {next_resume:?} starts {:?}",
                    all.iter()
                        .map(|b| (b.block_offset, b.block_size))
                        .collect::<Vec<_>>(),
                    g.starts
                );
            }
            for k in lead_n..end {
                let pi = run_start + k - lead_n;
                let info = named(
                    PageInfo {
                        index: pi as u32,
                        first: pi == 0,
                        blank: g.blank.contains(&(k as u32)),
                        left: pi % 2 == 1,
                        name: None,
                    },
                    names[k],
                );
                let pb = geom.page_box(&info);
                // A page whose width differs but has no resume point is laid out at the run's width.
                let pb = PageBox {
                    margin: if (pb.content_width() - width).abs() <= EPS {
                        pb.margin
                    } else {
                        [
                            pb.margin[0],
                            pb.width - pb.margin[3] - width,
                            pb.margin[2],
                            pb.margin[3],
                        ]
                    },
                    ..pb
                };
                let last = k + 1 == all.len();
                let area = Viewport {
                    width: pb.content_width(),
                    height: pb.content_height(),
                    scale,
                };
                let (fragments, non_finite) = crate::layout::fragment::builder::build_page(
                    boxes,
                    &tree,
                    flow,
                    all[k],
                    last,
                    &pb,
                    area,
                    &mut piece_counts,
                );
                if non_finite {
                    diagnostics.push(ErrorRecord {
                        document: Some(boxes.document),
                        status: Status::new(Code::GeometryNonFinite, Stage::Layout)
                            .with_message("geometry clamped to the LayoutUnit range"),
                    });
                }
                pages.push(PagedPage {
                    info,
                    page_box: pb,
                    fragments,
                    flow_top: all[k].block_offset,
                    flow_bottom: all[k].block_offset + all[k].block_size,
                    run,
                });
            }
            match next_resume {
                Some(r) if end > lead_n && run < 64 => {
                    run_start += end - lead_n;
                    resume = Some(r);
                    run_box = next_box;
                    run += 1;
                }
                Some(_) => break,
                None => break,
            }
        }
        // The geometry the pages were laid out with must be what their actual selectors give.
        let consistent = pages.iter().all(|p| {
            let assumed_h = assumed_boxes[(p.info.index as usize).min(assumed_boxes.len() - 1)]
                .content_height();
            (geom.page_box(&p.info).content_height() - assumed_h).abs() <= EPS
        }) && assumed.len() >= pages.len();
        let mut next: Vec<PageInfo> = pages.iter().map(|p| p.info.clone()).collect();
        let n = next.len() as u32;
        let last_name = next
            .last()
            .and_then(|l| l.name.as_deref())
            .map(crate::layout::fragmentation::paged::intern);
        next.extend((n..n + 2).map(|i| named(PageInfo::assumed(i), last_name)));
        result = pages;
        if consistent || pass + 1 == MAX_PAGE_PASSES {
            break;
        }
        assumed = next;
    }
    (result, layouts)
}

/// Lay out a box tree in the viewport and build its fragment tree.
pub(crate) fn layout_boxes(
    boxes: &BoxTree,
    viewport: Viewport,
    text: &mut TextServices,
    reports: &mut Vec<LayoutReport>,
    diagnostics: &mut Vec<ErrorRecord>,
    trace: &mut Vec<super::EpochState>,
    prev: Option<(
        &BoxTree,
        &super::replay::HostSnapshot,
        Option<&FragmentTree>,
    )>,
) -> (FragmentTree, Option<super::replay::HostSnapshot>) {
    // Text advances follow the device scale (subpixel positioning above 1; L-GEO-r9).
    text.set_device_scale(viewport.scale);
    consumption_reports(boxes, reports);
    trace.push(super::EpochState::Layout);
    let Some(root) = boxes.root else {
        trace.push(super::EpochState::FragmentBuild);
        return (FragmentTree::empty(viewport), None);
    };
    let ifc_seeds = prev
        .map(|(old_boxes, old, _)| super::replay::ifc_seeds(boxes, old_boxes, old))
        .unwrap_or_default();
    // One allocation of the node table, sized from the previous pass, a power of two (015 PERF-2: see
    // `build_fragments`).
    let nodes = Vec::with_capacity(
        prev.map_or(0, |(_, old, _)| (old.node_count() + 16).next_power_of_two()),
    );
    let mut tree = HostTree {
        nodes,
        calcs: CalcTable::default(),
        container_width: Vec::new(),
        inline_cb_pending: Vec::new(),
        empty_probe: None,
        frag: None,
        bfc_floats: false,
        frag_layouts: (0, 0),
        ifc_seeds,
        viewport,
        text_ptr: std::ptr::null_mut(),
        paged: false,
    };
    let tp0 = std::time::Instant::now();
    let root_idx = tree.add(boxes, root, None, text);
    // Formatting-context replay (015 PERF-1) is off when this pass or the previous one has a box in a vertical
    // writing mode (015 WM-1): the randomized harness found seeded results that depend on the frame they were
    // computed in; the cold path stays exact (D-LWM-09 g).
    let replayed = prev
        .filter(|(old_boxes, _, _)| {
            !crate::layout::writing_mode::physical::has_vertical(boxes)
                && !crate::layout::writing_mode::physical::has_vertical(old_boxes)
        })
        .map_or(0, |(old_boxes, old, _)| {
            super::replay::replay(&mut tree, boxes, old_boxes, old)
        });
    let tp_add = tp0.elapsed();
    // 015 WM-1: the initial containing block has the root's writing mode; a vertical root lays out in its
    // logical frame, where the viewport's height is the inline size.
    let root_vertical = boxes.get(root).style.writing_mode.is_vertical();
    let viewport_l = if root_vertical {
        Viewport {
            width: viewport.height,
            height: viewport.width,
            ..viewport
        }
    } else {
        viewport
    };
    let icb = Size {
        width: AvailableSpace::Definite(quantize_input(viewport_l.width)),
        height: AvailableSpace::Definite(quantize_input(viewport_l.height)),
    };
    // A floated root element (`:root { float: right }`) has no parent block to place it: the initial
    // containing block is its parent (CSS 2.1 section 9.5, the float is shrink-to-fit and sits at
    // the edge of the viewport), so a block node of the viewport's size stands in as that parent.
    let layout_root = if tree.nodes[root_idx].style.float != taffy::Float::None {
        tree.push_icb_parent(root_idx, viewport_l)
    } else {
        root_idx
    };
    tree.text_ptr = &mut *text as *mut TextServices;
    compute_root_layout(&mut tree, NodeId::from(layout_root), icb);
    tree.text_ptr = std::ptr::null_mut();
    let tp_compute = tp0.elapsed();
    if layout_root != root_idx {
        // The boxes the stand-in positioned against the initial containing block belong to the root.
        // Their positions are the viewport's; the root's fragment is where it floated.
        let hoisted = std::mem::take(&mut tree.nodes[layout_root].hoisted);
        let at = tree.nodes[root_idx].layout.location;
        for &h in &hoisted {
            let loc = &mut tree.nodes[h].layout.location;
            loc.x -= at.x;
            loc.y -= at.y;
        }
        tree.nodes[root_idx].hoisted.extend(hoisted);
    }
    for node in &tree.nodes {
        if let Some(ifc) = &node.ifc {
            for (feature, stage, n) in &ifc.reports {
                let location = ReportLocation::Element { node: *n };
                push_report(
                    reports,
                    ReportKey {
                        feature: *feature,
                        stage: *stage,
                        document: boxes.document,
                        location,
                    },
                );
            }
        }
    }
    for node in &tree.nodes {
        // FR-072: a pending image is laid out as 0×0 (or by its attribute ratio); one
        // RESOURCE_PENDING diagnostic per image.
        if node.leaf.as_ref().is_some_and(|l| l.image_pending) {
            diagnostics.push(ErrorRecord {
                document: Some(boxes.document),
                status: Status::new(Code::ResourcePending, Stage::Layout)
                    .with_message("image resource is pending; laid out without a natural size"),
            });
        }
    }
    resolve_inline_containing_blocks(boxes, &mut tree, root_idx, viewport, reports);
    resolve_anchor_positions(boxes, &mut tree, root_idx, viewport);
    trace.push(super::EpochState::FragmentBuild);
    let tp_pre = tp0.elapsed();
    // Seeded subtrees that were not laid out again take the previous fragments (015 PERF-1).
    // A tree with a vertical box is built in logical coordinates and mapped (015 WM-1): previous fragments are
    // physical, so none is copied.
    let vertical = crate::layout::writing_mode::physical::has_vertical(boxes);
    let old_fragments = if replayed > 0 && !vertical {
        prev.and_then(|p| p.2)
    } else {
        None
    };
    let (mut fragments, non_finite) =
        build_fragments(boxes, &tree, root_idx, viewport, old_fragments);
    if vertical {
        crate::layout::writing_mode::physical::physicalize(&mut fragments, boxes, viewport);
    }
    crate::layout::writing_mode::physical::inline_scroll_overflow(&mut fragments);
    fragments.viewport_scroll =
        crate::layout::fragment::builder::viewport_scroll_of(&fragments, boxes);
    if std::env::var_os("UI4ME_PROFILE").is_some() {
        eprintln!(
            "[prof]   ifc_new {:.2} taffy_style {:.2} table_grid {:.2}",
            IFC_NS.with(|c| c.replace(0)) as f64 / 1e6,
            STYLE_NS.with(|c| c.replace(0)) as f64 / 1e6,
            TABLE_NS.with(|c| c.replace(0)) as f64 / 1e6
        );
        eprintln!(
            "[prof]   tree.add {:.2} compute {:.2} post {:.2} build_fragments {:.2} (nodes {}, replayed {}, fragmented layouts {} kept {})",
            tp_add.as_secs_f64() * 1e3,
            (tp_compute - tp_add).as_secs_f64() * 1e3,
            (tp_pre - tp_compute).as_secs_f64() * 1e3,
            (tp0.elapsed() - tp_pre).as_secs_f64() * 1e3,
            tree.nodes.len(),
            replayed,
            tree.frag_layouts.0,
            tree.frag_layouts.1
        );
    }
    if non_finite {
        diagnostics.push(ErrorRecord {
            document: Some(boxes.document),
            status: Status::new(Code::GeometryNonFinite, Stage::Layout)
                .with_message("geometry clamped to the LayoutUnit range"),
        });
    }
    (
        fragments,
        Some(super::replay::HostSnapshot::of(
            tree,
            boxes.boxes.len(),
            replayed,
        )),
    )
}

/// CSS 2.1 section 10.1 item 4: the containing block of an `absolute` box whose nearest positioned ancestor is
/// an inline box is the bounding box of the padding boxes of that inline box's first and last fragments (the top
/// left of the first and the bottom right of the last in `ltr`; the mirror in `rtl`). The box was laid out
/// against the next claiming ancestor; with the fragments of a first build (their inline-box pieces give the
/// padding boxes) it is laid out again against that rectangle, still a child of the ancestor that hoisted it.
fn resolve_inline_containing_blocks(
    boxes: &BoxTree,
    tree: &mut HostTree,
    root_idx: usize,
    viewport: Viewport,
    reports: &mut Vec<LayoutReport>,
) {
    use crate::layout::fragment::FragmentKind;
    let pending = std::mem::take(&mut tree.inline_cb_pending);
    // Only boxes that are still hoisted by the node that claimed them in the last layout.
    let pending: Vec<(usize, OofCandidate)> = pending
        .into_iter()
        .filter(|(h, c)| tree.nodes[*h].hoisted.contains(&usize::from(c.node)))
        .collect();
    if pending.is_empty() {
        return;
    }
    let (prelim, _) = build_fragments(boxes, tree, root_idx, viewport, None);
    for (claimer, cand) in pending {
        let node = usize::from(cand.node);
        let Some(owner) = tree.nodes[node].inline_cb_owner else {
            continue;
        };
        let pieces: Vec<_> = prelim
            .fragments_of(owner)
            .iter()
            .filter_map(|id| match &prelim.get(*id).kind {
                FragmentKind::InlineBoxPiece(p) if matches!(p.owner, BoxOwner::Element(_)) => {
                    Some((prelim.document_rect(*id).rect, p))
                }
                _ => None,
            })
            .collect();
        let Some(claimer_frag) = prelim
            .maps
            .box_to_fragments
            .get(&tree.nodes[claimer].box_id)
            .and_then(|v| v.first())
            .copied()
        else {
            continue;
        };
        let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
            super::oof::report_unresolved_inline_containing_block(
                boxes,
                tree.nodes[node].box_id,
                reports,
            );
            continue;
        };
        let rtl = tree.nodes[claimer].style.direction == taffy::Direction::Rtl;
        // The padding box of a piece: its content area plus the padding on the edges it has.
        let padding_box = |(r, p): &(
            ui4me_engine_types::geometry::LayoutRect,
            &crate::layout::fragment::InlineBoxPiece,
        )| {
            let left = r.origin.x.to_f64() as f32
                - if p.has_start_edge {
                    p.padding.left.to_f64() as f32
                } else {
                    0.0
                };
            let right = (r.origin.x + r.size.width).to_f64() as f32
                + if p.has_end_edge {
                    p.padding.right.to_f64() as f32
                } else {
                    0.0
                };
            let top = r.origin.y.to_f64() as f32 - p.padding.top.to_f64() as f32;
            let bottom =
                (r.origin.y + r.size.height).to_f64() as f32 + p.padding.bottom.to_f64() as f32;
            (left, top, right, bottom)
        };
        let (f, l) = (padding_box(first), padding_box(last));
        let (left, right) = if rtl { (l.0, f.2) } else { (f.0, l.2) };
        let (top, bottom) = (f.1, l.3);
        let origin = prelim.document_rect(claimer_frag).rect.origin;
        let area = OofPositioningArea {
            size: taffy::Size {
                width: (right - left).max(0.0),
                height: (bottom - top).max(0.0),
            },
            offset: taffy::Point {
                x: left - origin.x.to_f64() as f32,
                y: top - origin.y.to_f64() as f32,
            },
        };
        let direction = tree.nodes[claimer].style.direction;
        let claims = super::style_view::containing_block_claims(&inline_cb_style(
            boxes,
            tree.nodes[node].box_id,
        ));
        taffy::compute_oof_layout_for_area(
            tree,
            NodeId::from(claimer),
            &[cand],
            area,
            direction,
            claims,
        );
    }
}

/// CSS Anchor Positioning subset (F0808): an out-of-flow box whose `top`/`right`/`bottom`/`left` is an
/// `anchor()` function is moved so that the edge the inset names meets the named side of its anchor's border box.
/// The anchor is the element with the `anchor-name` the function (or `position-anchor`) names, else the
/// popover's implicit anchor (its invoker). The box is laid out normally first (the anchor insets are `auto`),
/// then, with the fragments of that pass, shifted: the shift is recomputed from the current fragments on every
/// layout, so a resize or any change of the anchor's or the box's geometry repositions it. A box whose axis has
/// two anchored insets follows the start edge (`left`, `top`) and is not stretched; an unknown anchor or a side
/// that does not belong to the inset's axis leaves the box where the normal layout put it.
fn resolve_anchor_positions(
    boxes: &BoxTree,
    tree: &mut HostTree,
    root_idx: usize,
    viewport: Viewport,
) {
    use crate::style::view::groups::AnchorSide;
    let anchored: Vec<(usize, BoxId)> = tree
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            let b = boxes.get(n.box_id);
            let a = b.style.anchor.as_ref()?;
            (b.is_out_of_flow()
                && matches!(b.owner, BoxOwner::Element(_))
                && a.props.insets.iter().any(Option::is_some))
            .then_some((i, n.box_id))
        })
        .collect();
    if anchored.is_empty() {
        return;
    }
    let (prelim, _) = build_fragments(boxes, tree, root_idx, viewport, None);
    let rect_of = |node: ui4me_engine_types::ids::NodeId| {
        let f = prelim.fragments_of(node).first().copied()?;
        let r = prelim.document_rect(f).rect;
        Some((
            r.origin.x.to_f64(),
            r.origin.y.to_f64(),
            r.size.width.to_f64(),
            r.size.height.to_f64(),
        ))
    };
    for (idx, id) in anchored {
        let b = boxes.get(id);
        let BoxOwner::Element(node) = b.owner else {
            continue;
        };
        let Some(a) = b.style.anchor.as_ref() else {
            continue;
        };
        let Some((ox, oy, ow, oh)) = rect_of(node) else {
            continue;
        };
        let (mut dx, mut dy): (Option<f64>, Option<f64>) = (None, None);
        // Start edges first: `left` before `right`, `top` before `bottom`.
        for i in [3usize, 1, 0, 2] {
            let Some(ins) = &a.props.insets[i] else {
                continue;
            };
            let horizontal = i == 1 || i == 3;
            if (horizontal && dx.is_some()) || (!horizontal && dy.is_some()) {
                continue;
            }
            let anchor_node = super::oof::anchor_of_inset(boxes, id, ins);
            let Some((ax, ay, aw, ah)) = anchor_node.and_then(rect_of) else {
                continue;
            };
            // Where along the property's axis (0 = start, 1 = end) the named side lies; `None` when the
            // keyword belongs to the other axis.
            let own_end = i == 1 || i == 2;
            let frac = match ins.side {
                AnchorSide::Top if !horizontal => 0.0,
                AnchorSide::Bottom if !horizontal => 1.0,
                AnchorSide::Left if horizontal => 0.0,
                AnchorSide::Right if horizontal => 1.0,
                AnchorSide::Top | AnchorSide::Bottom | AnchorSide::Left | AnchorSide::Right => {
                    continue;
                }
                AnchorSide::Center => 0.5,
                AnchorSide::Start | AnchorSide::SelfStart => 0.0,
                AnchorSide::End | AnchorSide::SelfEnd => 1.0,
                AnchorSide::Inside => f64::from(u8::from(own_end)),
                AnchorSide::Outside => f64::from(u8::from(!own_end)),
                AnchorSide::Percent(p) => f64::from(p),
            };
            let (a_start, a_len, o_start, o_len) = if horizontal {
                (ax, aw, ox, ow)
            } else {
                (ay, ah, oy, oh)
            };
            let target = a_start + frac * a_len;
            let own_edge = if own_end { o_start + o_len } else { o_start };
            if horizontal {
                dx = Some(target - own_edge);
            } else {
                dy = Some(target - own_edge);
            }
        }
        let loc = &mut tree.nodes[idx].layout.location;
        loc.x += dx.unwrap_or(0.0) as f32;
        loc.y += dy.unwrap_or(0.0) as f32;
    }
}

/// The style of the inline box that is the containing block of the `absolute` box `id`.
fn inline_cb_style(boxes: &BoxTree, id: BoxId) -> crate::layout::box_tree::BoxStyle {
    super::oof::inline_containing_block(boxes, id)
        .map(|x| (*boxes.get(x).style).clone())
        .unwrap_or_else(|| (*boxes.get(id).style).clone())
}

/// The base direction a text control's text is shaped with: its `direction`, or the first strong
/// character of each line for `unicode-bidi: plaintext` and `dir=auto` (HTML: `textarea`, `pre`
/// and the search-like inputs get `plaintext` from the UA sheet).
fn control_base_direction(
    style: &crate::layout::box_tree::BoxStyle,
) -> crate::text::bidi::BaseDirection {
    use crate::text::bidi::BaseDirection;
    if style.dir_auto || style.unicode_bidi == crate::layout::box_tree::UnicodeBidi::Plaintext {
        BaseDirection::Auto
    } else if style.direction == crate::layout::box_tree::Direction::Rtl {
        BaseDirection::Rtl
    } else {
        BaseDirection::Ltr
    }
}
