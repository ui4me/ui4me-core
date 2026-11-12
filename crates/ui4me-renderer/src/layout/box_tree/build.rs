//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! Box generation (008 `box-tree.md` §2–§4; FR-001–FR-011, FR-018). The display dispatch consumes
//! the style engine's computed `display` (FR-003): `block`, `list-item`, `flow-root`, `flex` and
//! `grid` generate block-level boxes with their inner display; `inline` generates an inline box,
//! split around block-level descendants (FR-007); `none` suppresses the subtree; `contents`
//! replaces the element by its children (FR-005, Appendix B exceptions behave as `none`).
//!
//! The atomic inline-levels `inline-block`, `inline-flex` and `inline-grid` generate an atomic
//! inline box (`BoxKind::InlineBlock`, FR-010) whose inner display is flow-root, flex or grid;
//! the inline formatting context places it as a Parley inline box (009 `push_atomic`) and the
//! host tree lays its contents out with the matching container algorithm. The HTML rendering
//! rules (`html_rendering.rs`: buttons, replaced elements, controls, `fieldset`, `details`) apply
//! before the CSS dispatch. The table family generates a wrapper box around the table grid box and
//! its captions with anonymous table objects (`table_fixup.rs`, FR-009, FR-076).
//!
//! Never mutates the DOM, never calls host code. Conservative full rebuild per update.

use std::collections::HashMap;
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{FeatureId, ReportKey, ReportLocation, UnsupportedReason};
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::stage::Stage;

use super::anonymous::wrap_inline_runs;
use super::html_rendering::{self, Special};
use super::{
    BoxStyle, DisplayValue, InnerDisplay, RowGroupKind, SourceNode, StyledSource, TableTag,
    contents_behaves_as_none, table_fixup,
};
use crate::layout::coordinator::LayoutReport;
use crate::layout::replaced::ReplacedContent;

/// Index of a box in its `BoxTree` (valid only for that tree; 008 `box-tree.md` §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BoxId(pub u32);

/// Generated boxes never carry DOM identity; `Anonymous` names the containing element for
/// diagnostics only (AD-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoxOwner {
    Element(NodeId),
    Anonymous(NodeId),
    /// A generated `::before`, `::after` or `::marker` box of the element (FR-012, FR-082): no
    /// DOM identity, diagnostics only.
    Pseudo(NodeId, super::PseudoKind),
}

impl BoxOwner {
    /// The element this box belongs to (for anonymous boxes, the containing element).
    pub fn node(self) -> NodeId {
        match self {
            BoxOwner::Element(n) | BoxOwner::Anonymous(n) | BoxOwner::Pseudo(n, _) => n,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BoxKind {
    /// Block-level block container; `inline_root` when its in-flow content is inline-level.
    BlockContainer {
        inline_root: bool,
    },
    InlineBox,
    /// DOM text (unprocessed; white-space processing belongs to the inline behaviour).
    TextRun(String),
    /// Atomic inline whose contents form an independent formatting context (`inline-block`,
    /// `inline-flex`, `inline-grid`, `button`, input buttons; box-tree.md §3 `AtomicInline`):
    /// `LayoutBox::inner` says which. `inline_root` as for a block container.
    InlineBlock {
        inline_root: bool,
    },
    /// Replaced element or form control (FR-070–FR-075): a leaf with a natural geometry; atomic
    /// inline-level unless `block_level`.
    Replaced(ReplacedContent),
}

/// Contract role of a generated box that its `BoxKind` alone does not tell (box-tree.md §3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxTag {
    #[default]
    None,
    /// The anonymous flow-root box inside a `button` that holds its content, centred vertically.
    ButtonContent,
    /// The first eligible `legend` of a `fieldset`.
    RenderedLegend,
    /// A `fieldset`: establishes a block formatting context.
    Fieldset,
    /// A table wrapper, table grid box, row group, row, cell, caption, column or column group
    /// (CSS Tables 3 fix-up; `table_fixup.rs`).
    Table(super::TableTag),
    /// A list item's `::marker` box (FR-082): an atomic inline with a zero-width margin box when
    /// outside.
    Marker(super::MarkerPlacement),
    /// The anonymous flow-thread box of a multicol container (015 FRAG-2a/2b; `crate::layout::fragmentation`):
    /// it holds the container's content, which is laid out as one strip at the column width and sliced
    /// into columns. Generated only for a multicol container (`fragmentation::multicol`), one per row of
    /// content between spanners; every other box tree is unchanged.
    FlowThread,
}

#[derive(Clone, Debug)]
pub struct LayoutBox {
    pub id: BoxId,
    pub owner: BoxOwner,
    pub kind: BoxKind,
    pub tag: BoxTag,
    /// Computed values (anonymous boxes inherit, CSS 2.1 §9.2.1.1). Text runs carry their
    /// parent box's style. Shared (`Arc`) so that a box subtree reused from the previous tree is
    /// copied without cloning styles (015 PERF-1, `incremental.rs`).
    pub style: std::sync::Arc<BoxStyle>,
    pub parent: Option<BoxId>,
    pub children: Vec<BoxId>,
    /// The formatting context a block-level or atomic inline-level box establishes for its
    /// contents (`Flow` for inline boxes, text and replaced boxes, which establish none).
    pub inner: InnerDisplay,
    /// Atomic inline-level classification (`inline-block`, `inline-flex`, `inline-grid`, buttons;
    /// FR-010): the inner display of a `BoxKind::InlineBlock`, `None` for every other box.
    pub atomic_inline: Option<InnerDisplay>,
    /// `display: list-item`: the marker hook for US10 (FR-082).
    pub list_item: bool,
}

/// Field-wise equality; shared styles (`Arc`) compare by pointer first (015 PERF-1).
impl PartialEq for LayoutBox {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
            && self.owner == o.owner
            && self.tag == o.tag
            && self.parent == o.parent
            && self.children == o.children
            && self.inner == o.inner
            && self.atomic_inline == o.atomic_inline
            && self.list_item == o.list_item
            && self.kind == o.kind
            && (std::sync::Arc::ptr_eq(&self.style, &o.style) || *self.style == *o.style)
    }
}

impl LayoutBox {
    /// Block-level: a block container or a block-level replaced box.
    pub fn is_block(&self) -> bool {
        match &self.kind {
            BoxKind::BlockContainer { .. } => true,
            BoxKind::Replaced(r) => r.block_level,
            _ => false,
        }
    }
    /// `position: absolute | fixed` on a block-level box: out of the flow (FR-056–FR-059).
    pub fn is_out_of_flow(&self) -> bool {
        self.is_block() && self.style.position.is_out_of_flow()
    }
    /// An in-flow block-level box with `float` other than `none` (CSS 2.1 §9.5).
    pub fn is_float(&self) -> bool {
        self.is_block()
            && !self.style.position.is_out_of_flow()
            && self.style.float != super::FloatValue::None
    }
    /// A block container (block-level or an atomic inline-block) whose in-flow content is inline.
    pub fn is_inline_root(&self) -> bool {
        matches!(
            self.kind,
            BoxKind::BlockContainer { inline_root: true }
                | BoxKind::InlineBlock { inline_root: true }
        )
    }
    /// An atomic inline-level box: an inline-block (flow-root, flex or grid inside) or an
    /// inline-level replaced / control box.
    pub fn is_atomic_inline(&self) -> bool {
        match &self.kind {
            BoxKind::InlineBlock { .. } => true,
            BoxKind::Replaced(r) => !r.block_level,
            _ => false,
        }
    }
    /// Whether the box lays out block-level children itself (a block container or an
    /// inline-block that is not an inline root); replaced boxes are leaves.
    pub(crate) fn leaf_free(&self) -> bool {
        matches!(
            self.kind,
            BoxKind::BlockContainer { .. } | BoxKind::InlineBlock { .. }
        )
    }
    /// The contract role (`box-tree.md` §3) of this box, told apart by tag where the kind is
    /// shared.
    pub fn role(&self) -> super::BoxRole {
        use super::{AtomicInner, BoxRole};
        match self.tag {
            BoxTag::ButtonContent => return BoxRole::ButtonContent,
            BoxTag::RenderedLegend => return BoxRole::RenderedLegend,
            BoxTag::Marker(p) => return BoxRole::ListMarker(p),
            BoxTag::None
                if self.style.marker_outside && matches!(self.kind, BoxKind::Replaced(_)) =>
            {
                return BoxRole::ListMarker(super::MarkerPlacement::Outside);
            }
            BoxTag::Table(t) => {
                use super::TableTag as T;
                return match t {
                    T::Wrapper if self.atomic_inline.is_some() => {
                        BoxRole::AtomicInline(AtomicInner::Table)
                    }
                    T::Wrapper => BoxRole::TableWrapper,
                    T::Table => BoxRole::Table,
                    T::RowGroup(k) => BoxRole::TableRowGroup(k),
                    T::Row => BoxRole::TableRow,
                    T::Cell { .. } => BoxRole::TableCell,
                    T::Caption => BoxRole::TableCaption,
                    T::ColumnGroup { .. } => BoxRole::TableColumnGroup,
                    T::Column { .. } => BoxRole::TableColumn,
                };
            }
            _ => {}
        }
        match (&self.kind, self.atomic_inline) {
            (BoxKind::InlineBlock { .. }, Some(InnerDisplay::Flex)) => {
                BoxRole::AtomicInline(AtomicInner::Flex)
            }
            (BoxKind::InlineBlock { .. }, Some(InnerDisplay::Grid)) => {
                BoxRole::AtomicInline(AtomicInner::Grid)
            }
            (BoxKind::BlockContainer { .. }, _) => match self.inner {
                InnerDisplay::Flex => BoxRole::FlexContainer,
                InnerDisplay::Grid => BoxRole::GridContainer,
                _ => BoxRole::BlockContainer,
            },
            (kind, _) => kind.role(),
        }
    }
    /// Whether the box establishes an independent formatting context for its contents (FR-022:
    /// flow-root, flex and grid containers, atomic inlines; the other BFC roots join with their
    /// stories).
    pub fn establishes_independent_context(&self) -> bool {
        (self.is_block() && self.inner != InnerDisplay::Flow)
            || matches!(self.kind, BoxKind::InlineBlock { .. })
            || self.effective_containment().contains_layout_or_paint()
    }

    /// The containment that has an effect on this box (CSS Containment 2 section 3): none on a
    /// non-atomic inline box, a text run or an internal table box other than a cell; no size
    /// containment on a table (its wrapper) or a cell. `container` (the query-container kind) is kept
    /// only where size containment applies, because a query container answers size queries only then.
    pub fn effective_containment(&self) -> super::Containment {
        use super::{Containment, TableTag};
        let c = self.style.containment;
        if c == Containment::NONE {
            return c;
        }
        let no_size = Containment {
            size_inline: false,
            size_block: false,
            container: super::ContainerKind::Normal,
            ..c
        };
        match self.tag {
            BoxTag::Table(TableTag::Wrapper | TableTag::Cell { .. }) => no_size,
            BoxTag::Table(TableTag::Caption) => c,
            BoxTag::Table(_) => Containment::NONE,
            _ => match self.kind {
                BoxKind::BlockContainer { .. }
                | BoxKind::InlineBlock { .. }
                | BoxKind::Replaced(_) => c,
                _ => Containment::NONE,
            },
        }
    }
}

/// One box tree per style epoch (`Arc`-shared by the layout output).
#[derive(Clone, Debug)]
pub struct BoxTree {
    pub document: DocumentId,
    pub root: Option<BoxId>,
    pub boxes: Vec<LayoutBox>,
    /// Content order.
    pub node_to_boxes: HashMap<NodeId, Vec<BoxId>>,
    /// Box-stage reports (de-duplicated per key within this build).
    pub reports: Vec<LayoutReport>,
    /// Revision stamp for `BoxHandle` validity (box-tree.md section 4.4); distinct per built tree.
    pub generation: u32,
    /// `overflow` propagated to the viewport (FR-065).
    pub viewport_overflow: ViewportOverflow,
    /// What the next incremental build needs of this build (015 PERF-1); not part of the layout input.
    pub(crate) memo: Option<Arc<super::incremental::BuildMemo>>,
    /// Box ranges `[start, end)` copied from the previous tree with the same box ids (015 PERF-1).
    pub(crate) same_ids: Vec<(u32, u32)>,
}

impl BoxTree {
    /// The DOM revision this tree was built from, when it can seed an incremental build (015 PERF-1):
    /// the caller hands the DOM changes since then (`BoxDamage`).
    pub fn build_revision(&self) -> Option<u64> {
        self.memo.as_ref().map(|m| m.dom_revision)
    }

    /// Whether `other` is the same layout input: layout is a pure function of the boxes (and the
    /// viewport and font environment), so equal trees lay out identically. The per-tree `generation`
    /// stamp and the lookup map (derived from the owners) do not take part.
    pub fn same_layout_input(&self, other: &BoxTree) -> bool {
        self.document == other.document
            && self.root == other.root
            && self.viewport_overflow == other.viewport_overflow
            && self.boxes.len() == other.boxes.len()
            && self.reports == other.reports
            && self.boxes == other.boxes
    }

    /// Debug aid (`UI4ME_PROFILE_DIFF`): a short description of the first difference to `other`.
    pub fn first_difference(&self, other: &BoxTree) -> String {
        if self.boxes.len() != other.boxes.len() {
            return format!("box count {} vs {}", self.boxes.len(), other.boxes.len());
        }
        if self.reports != other.reports {
            return "reports".into();
        }
        for (i, (a, b)) in self.boxes.iter().zip(&other.boxes).enumerate() {
            if a != b {
                let (sa, sb) = (format!("{a:?}"), format!("{b:?}"));
                let p = sa
                    .bytes()
                    .zip(sb.bytes())
                    .position(|(x, y)| x != y)
                    .unwrap_or(0);
                let lo = p.saturating_sub(160);
                let lo = (lo..=p)
                    .find(|&k| sa.is_char_boundary(k) && sb.is_char_boundary(k))
                    .unwrap_or(p);
                let hi_a = ((p + 80).min(sa.len())..=sa.len())
                    .find(|&k| sa.is_char_boundary(k))
                    .unwrap_or(sa.len());
                let hi_b = ((p + 80).min(sb.len())..=sb.len())
                    .find(|&k| sb.is_char_boundary(k))
                    .unwrap_or(sb.len());
                return format!("box {i}: ...{} ||| ...{}", &sa[lo..hi_a], &sb[lo..hi_b]);
            }
        }
        "other".into()
    }
}

/// The `overflow` the viewport takes from the root element or from `body` (CSS Overflow 3 §3.3,
/// FR-065). The element it is taken from has a used `overflow` of `visible`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewportOverflow {
    /// The box the value was propagated from; `None` when both are `visible`.
    pub source: Option<BoxId>,
    /// Used values on the viewport: `visible` is `auto`, `clip` is `hidden`.
    pub x: super::OverflowValue,
    pub y: super::OverflowValue,
}

impl ViewportOverflow {
    /// The used viewport value of a propagated `overflow` keyword.
    fn used(v: super::OverflowValue) -> super::OverflowValue {
        match v {
            super::OverflowValue::Visible => super::OverflowValue::Auto,
            super::OverflowValue::Clip => super::OverflowValue::Hidden,
            other => other,
        }
    }

    /// `auto` for both axes: the default viewport (no propagated value).
    pub const DEFAULT: ViewportOverflow = ViewportOverflow {
        source: None,
        x: super::OverflowValue::Auto,
        y: super::OverflowValue::Auto,
    };
}

impl BoxTree {
    pub fn get(&self, id: BoxId) -> &LayoutBox {
        &self.boxes[id.0 as usize]
    }
    pub fn boxes_of(&self, node: NodeId) -> &[BoxId] {
        self.node_to_boxes
            .get(&node)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn len(&self) -> usize {
        self.boxes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }
    /// Anonymous block boxes, in tree order.
    pub fn anonymous_blocks(&self) -> impl Iterator<Item = &LayoutBox> {
        self.boxes.iter().filter(|b| {
            matches!(b.owner, BoxOwner::Anonymous(_)) && b.is_block() && b.tag == BoxTag::None
        })
    }
}

/// A box under construction (before arena insertion).
pub(crate) enum Proto {
    /// `atomic`: an atomic inline-level box (`inline-block`, `inline-flex`, `inline-grid`,
    /// `button`) whose contents use `inner`; otherwise block-level.
    Block {
        owner: BoxOwner,
        style: BoxStyle,
        children: Vec<Proto>,
        inner: InnerDisplay,
        list_item: bool,
        tag: BoxTag,
        atomic: bool,
    },
    Inline {
        node: NodeId,
        pseudo: Option<super::PseudoKind>,
        style: BoxStyle,
        children: Vec<Proto>,
    },
    Text {
        text: String,
        style: BoxStyle,
        owner: NodeId,
        pseudo: Option<super::PseudoKind>,
    },
    Replaced {
        node: NodeId,
        pseudo: Option<super::PseudoKind>,
        style: BoxStyle,
        content: ReplacedContent,
    },
    /// The box subtree of an element taken over from the previous tree (015 PERF-1, `incremental.rs`):
    /// `root` is its first box there. `style`, `block_level` and `tag` are the root's.
    Reused {
        root: BoxId,
        style: Arc<BoxStyle>,
        block_level: bool,
        tag: BoxTag,
    },
}

impl Proto {
    /// Block-level by its outer display (in flow or not).
    fn outer_block(&self) -> bool {
        match self {
            Proto::Block { atomic, .. } => !atomic,
            Proto::Replaced { content, .. } => content.block_level,
            Proto::Reused { block_level, .. } => *block_level,
            Proto::Inline { .. } | Proto::Text { .. } => false,
        }
    }

    pub(crate) fn style(&self) -> &BoxStyle {
        match self {
            Proto::Block { style, .. }
            | Proto::Inline { style, .. }
            | Proto::Text { style, .. }
            | Proto::Replaced { style, .. } => style,
            Proto::Reused { style, .. } => style,
        }
    }

    /// An in-flow block-level box. An out-of-flow box (`position: absolute|fixed`) is block-level
    /// too, but it takes no part in the flow: it neither splits an inline box (FR-007) nor ends an
    /// inline run (CSS 2.1 §9.2.1.1, CSS Position 3 §3), and it is no flex or grid item (FR-008,
    /// FR-045, FR-053).
    pub(crate) fn is_block_level(&self) -> bool {
        self.outer_block() && !self.style().position.is_out_of_flow() && !self.is_float()
    }

    /// An in-flow float (CSS 2.1 §9.5). Like an out-of-flow box it neither splits an inline box
    /// nor ends an inline run: a float next to inline content is placed by the line that holds it
    /// (the paragraph carries it as a `Float` item). In a block container with no inline content,
    /// and as a flex or grid item, it is a child of the container.
    pub(crate) fn is_float(&self) -> bool {
        self.outer_block()
            && !self.style().position.is_out_of_flow()
            && self.style().float != super::FloatValue::None
    }

    pub(crate) fn is_out_of_flow(&self) -> bool {
        self.outer_block() && self.style().position.is_out_of_flow()
    }

    /// An anonymous block box inheriting from `container` (CSS 2.1 §9.2.1.1).
    pub(crate) fn anonymous_block(
        container: NodeId,
        style: &BoxStyle,
        children: Vec<Proto>,
    ) -> Proto {
        Proto::Block {
            owner: BoxOwner::Anonymous(container),
            style: BoxStyle::inheriting(style, DisplayValue::Block),
            children,
            inner: InnerDisplay::Flow,
            list_item: false,
            tag: BoxTag::None,
            atomic: false,
        }
    }
}

pub(crate) struct Builder<'a, S: StyledSource> {
    pub(crate) src: &'a S,
    pub(crate) reports: Vec<LayoutReport>,
    /// Counter scopes in tree order (FR-085).
    pub(crate) counters: super::counters::CounterScopes,
    /// Quotation nesting (FR-086).
    pub(crate) quotes: super::counters::QuoteState,
    /// The siblings that follow the element being built, innermost list last (the scope of a
    /// counter an element creates reaches its following siblings, CSS Lists 3 section 4.4).
    pub(crate) later_siblings: Vec<(std::rc::Rc<Vec<NodeId>>, usize)>,
    /// Every report call, in order (an element's slice is replayed when its boxes are taken over).
    pub(crate) report_log: Vec<super::incremental::ReportCall>,
    /// Image states read (resource-dependent: such a subtree is never taken over).
    pub(crate) volatile: u64,
    /// The previous tree and what changed since (015 PERF-1); `None` for a full build.
    pub(crate) reuse: Option<super::incremental::ReuseCtx<'a>>,
    /// The elements whose builds the next incremental build may take over.
    pub(crate) candidates: Vec<super::incremental::Candidate>,
    /// A taken-over subtree had written counters (`incremental.rs` module docs).
    pub(crate) skipped_writes: bool,
    /// Whether to record candidates (the source supports incremental builds).
    pub(crate) record: bool,
    /// Elements whose box subtree a parent edited after their build (a marker placed inside, a rendered
    /// legend's tag): not recorded as candidates.
    pub(crate) tainted: Vec<NodeId>,
    /// The styled elements of the top layer, in order (015 HTML-1): boxed under the root, not in place.
    pub(crate) top_layer: Vec<NodeId>,
}

impl<S: StyledSource> Builder<'_, S> {
    pub(crate) fn report(&mut self, feature: FeatureId, stage: Stage, node: NodeId) {
        if self.record {
            self.report_log.push((feature, stage, node));
        }
        let key = ReportKey {
            feature,
            stage,
            document: self.src.document(),
            location: ReportLocation::Element { node },
        };
        if !self.reports.iter().any(|r| r.key == key) {
            self.reports.push(LayoutReport {
                key,
                code: Code::UnsupportedFeature,
                reason: UnsupportedReason::NotConsumedByStage,
            });
        }
    }

    /// The effective display of an element (FR-018): a value outside the supported set boxes as
    /// `inline` (its initial value) with one report.
    pub(crate) fn display(&mut self, node: NodeId, style: &BoxStyle) -> DisplayValue {
        match style.display {
            DisplayValue::Unsupported(f) => {
                self.report(f, Stage::Box, node);
                DisplayValue::Inline
            }
            d => d,
        }
    }

    /// Boxes for the children of `parent` (an element generating `parent_style`). The result may
    /// hold block-level boxes next to inline-level ones; the caller wraps or splits (FR-006,
    /// FR-007, FR-008).
    pub(crate) fn children(&mut self, parent: NodeId, parent_style: &BoxStyle) -> Vec<Proto> {
        let mut out = Vec::new();
        // `content-visibility: hidden` (0.9 F0903): no contents (their boxes, pseudo-elements,
        // paint, hit testing and accessibility are skipped).
        if parent_style.containment.skips_contents {
            return out;
        }
        self.pseudo_element(parent, super::PseudoKind::Before, parent_style, &mut out);
        let kids = std::rc::Rc::new(html_rendering::rendered_children(self.src, parent));
        // A closed `details`: everything but its first `summary` goes to the hidden slot.
        let closed = html_rendering::closed_details_summary(self.src, parent);
        // A `details` without a `summary` child shows the browser's default summary ("Details"): not
        // generated, reported (015 HTML-2).
        if self.src.html(parent).is_some_and(|i| i.name == "details")
            && !kids
                .iter()
                .any(|c| self.src.html(*c).is_some_and(|i| i.name == "summary"))
        {
            self.report(
                html_rendering::feature_id("html.details-default-summary"),
                Stage::Box,
                parent,
            );
        }
        let mut hidden = Vec::new();
        for (i, child) in kids.iter().copied().enumerate() {
            // An element in the top layer is boxed under the root (`append_top_layer`).
            if !self.top_layer.is_empty() && self.top_layer.contains(&child) {
                continue;
            }
            let mut slot_out = Vec::new();
            let in_slot = closed.is_some_and(|summary| summary != Some(i));
            self.later_siblings.push((kids.clone(), i + 1));
            let before = self.later_siblings.len();
            match self.src.node(child) {
                SourceNode::Text(t) => {
                    let text = Proto::Text {
                        text: t.to_string(),
                        style: parent_style.clone(),
                        owner: parent,
                        pseudo: None,
                    };
                    if in_slot {
                        hidden.push(text)
                    } else {
                        out.push(text)
                    }
                }
                SourceNode::Element(style) => {
                    if let Some(p) = self.try_reuse(child, parent_style, style) {
                        if in_slot {
                            hidden.push(p)
                        } else {
                            out.push(p)
                        }
                        debug_assert_eq!(self.later_siblings.len(), before);
                        self.later_siblings.pop();
                        continue;
                    }
                    let mut style = style.clone();
                    style.decorations = propagate_decorations(parent_style, &style);
                    complete_first_line(parent_style, &mut style);
                    if in_slot {
                        self.element(child, style, &mut slot_out);
                        hidden.append(&mut slot_out);
                    } else {
                        self.element(child, style, &mut out);
                    }
                }
                SourceNode::Ignored => {}
            }
            debug_assert_eq!(self.later_siblings.len(), before);
            self.later_siblings.pop();
        }
        if !hidden.is_empty() {
            out.push(html_rendering::closed_details_slot(
                parent,
                parent_style,
                hidden,
            ));
        }
        self.pseudo_element(parent, super::PseudoKind::After, parent_style, &mut out);
        // CSS 2.1 §17.2.1 rule 3: table-internal boxes outside a table get an anonymous table.
        if !matches!(
            parent_style.display,
            DisplayValue::Contents
                | DisplayValue::Table
                | DisplayValue::InlineTable
                | DisplayValue::TableRowGroup
                | DisplayValue::TableHeaderGroup
                | DisplayValue::TableFooterGroup
                | DisplayValue::TableRow
                | DisplayValue::TableColumnGroup
                | DisplayValue::TableColumn
        ) {
            out = table_fixup::wrap_misparented(parent, parent_style, out);
        }
        out
    }

    /// `colspan` / `rowspan` of a `td` or `th` (HTML §4.9.12; other elements span 1 × 1).
    fn cell_spans(&self, node: NodeId) -> (u32, u32) {
        if matches!(self.src.tag_name(node), Some("td" | "th")) {
            (
                crate::layout::table::grid::parse_colspan(
                    self.src.attr(node, "colspan").as_deref(),
                ),
                crate::layout::table::grid::parse_rowspan(
                    self.src.attr(node, "rowspan").as_deref(),
                ),
            )
        } else {
            (1, 1)
        }
    }

    /// `span` of a `col` or `colgroup`.
    fn column_span(&self, node: NodeId, tag: &str) -> u32 {
        if self.src.tag_name(node) == Some(tag) {
            crate::layout::table::grid::parse_colspan(self.src.attr(node, "span").as_deref())
        } else {
            1
        }
    }

    /// A table-internal box of `tag` holding `kids`.
    fn table_part(
        &self,
        node: NodeId,
        style: BoxStyle,
        inner: InnerDisplay,
        tag: TableTag,
        kids: Vec<Proto>,
    ) -> Proto {
        Proto::Block {
            owner: BoxOwner::Element(node),
            style,
            children: kids,
            inner,
            list_item: false,
            tag: BoxTag::Table(tag),
            atomic: false,
        }
    }

    /// One element: its counter operations apply (in tree order) when it generates a box, and its
    /// children see the counters it created (FR-085). An element with `display: none` has no
    /// effect on counters; neither has one with `display: contents`.
    fn element(&mut self, node: NodeId, style: BoxStyle, out: &mut Vec<Proto>) {
        let (log0, reads0, writes0, volatile0, n0) = (
            self.report_log.len(),
            self.reads(),
            self.writes(),
            self.volatile,
            out.len(),
        );
        let decorations = self.record.then(|| style.decorations.clone());
        let display = self.display(node, &style);
        let counts = !matches!(display, DisplayValue::None | DisplayValue::Contents);
        if counts {
            self.enter_counters(node, &style);
        }
        self.element_dispatch(node, style, out);
        if counts {
            self.leave_counters();
        }
        // One box subtree of the element's own (incremental.rs): recorded for the next build.
        if let Some(decorations) = decorations
            && out.len() == n0 + 1
            && self.reads() == reads0
            && self.volatile == volatile0
        {
            let wrapper = match &out[n0] {
                Proto::Block {
                    owner: BoxOwner::Element(n),
                    ..
                } if *n == node => Some(false),
                Proto::Block {
                    owner: BoxOwner::Anonymous(n),
                    tag: BoxTag::Table(TableTag::Wrapper),
                    ..
                } if *n == node => Some(true),
                Proto::Replaced {
                    node: n,
                    pseudo: None,
                    ..
                } if *n == node => Some(false),
                _ => None,
            };
            if let Some(wrapper) = wrapper {
                let reports = self.report_log[log0..].to_vec();
                self.candidates.push(super::incremental::Candidate {
                    node,
                    wrapper,
                    decorations,
                    reports,
                    writes: self.writes() != writes0,
                });
            }
        }
    }

    /// The top layer (CSS Position 4 section 3; 015 HTML-1): each element of it, after its `::backdrop`,
    /// becomes a child of the root box, in top-layer order, so its containing block is the initial one
    /// (the viewport for `fixed`) and no ancestor clips or transforms it. Each box carries its paint
    /// order (`BoxStyle::top_layer`): the scene paints them above everything else. An element whose
    /// `position` is not `absolute` or `fixed` is laid out as `absolute`.
    pub(crate) fn append_top_layer(&mut self, root: &mut Proto) {
        if self.top_layer.is_empty() {
            return;
        }
        let Proto::Block {
            children,
            style: root_style,
            ..
        } = root
        else {
            return;
        };
        let root_style = root_style.clone();
        for (i, n) in self.top_layer.clone().into_iter().enumerate() {
            let SourceNode::Element(es) = self.src.node(n) else {
                continue;
            };
            let mut es = es.clone();
            if es.display == DisplayValue::None {
                continue;
            }
            let order = i as u32;
            if let Some(bs) = self
                .src
                .pseudo(n, super::PseudoKind::Backdrop)
                .filter(|b| b.display != DisplayValue::None)
            {
                let mut bs = bs.clone();
                bs.top_layer = Some(2 * order);
                if !bs.position.is_out_of_flow() {
                    bs.position = super::PositionValue::Fixed;
                }
                bs.display = DisplayValue::Block;
                children.push(Proto::Block {
                    owner: BoxOwner::Pseudo(n, super::PseudoKind::Backdrop),
                    style: bs,
                    children: Vec::new(),
                    inner: InnerDisplay::FlowRoot,
                    list_item: false,
                    tag: BoxTag::None,
                    atomic: false,
                });
            }
            es.top_layer = Some(2 * order + 1);
            if !es.position.is_out_of_flow() {
                es.position = super::PositionValue::Absolute;
            }
            es.decorations = propagate_decorations(&root_style, &es);
            let mut out = Vec::new();
            self.element(n, es, &mut out);
            children.extend(out);
        }
    }

    pub(crate) fn reads(&self) -> u64 {
        self.counters.reads + self.quotes.reads
    }

    pub(crate) fn writes(&self) -> u64 {
        self.counters.writes
    }

    /// The box subtree of element `node` from the previous tree, when nothing it was built from changed
    /// (`incremental.rs`). Its counter frame is opened and closed as a full build does, and its reports
    /// are made again in place.
    fn try_reuse(
        &mut self,
        node: NodeId,
        parent_style: &BoxStyle,
        style: &BoxStyle,
    ) -> Option<Proto> {
        let ctx = self.reuse.as_ref()?;
        if ctx.unclean.contains(&node) {
            return None;
        }
        let m = ctx.memo.roots.get(&node)?;
        let decorations = propagate_decorations(parent_style, style);
        if !(Arc::ptr_eq(&decorations, &m.decorations) || *decorations == *m.decorations) {
            return None;
        }
        let root = ctx.old.get(m.root);
        let proto = Proto::Reused {
            root: m.root,
            style: root.style.clone(),
            block_level: root.is_block(),
            tag: root.tag,
        };
        let (reports, writes) = (m.reports.clone(), m.writes);
        self.enter_counters(node, style);
        self.leave_counters();
        for (f, st, n) in reports {
            self.report(f, st, n);
        }
        if writes {
            self.skipped_writes = true;
            self.counters.writes += 1;
        }
        Some(proto)
    }

    /// Replace taken-over subtrees among `kids` by their protos (before a parent edits a child's subtree).
    /// Expand the taken-over subtrees on the path `pseudo::place_marker` descends to the first line (the
    /// same skips: out-of-flow boxes, floats, collapsible white space; plain block containers entered).
    pub(crate) fn expand_first_line_path(&self, kids: &mut [Proto]) {
        let Some(ctx) = self.reuse.as_ref() else {
            return;
        };
        for k in kids.iter_mut() {
            if k.is_out_of_flow()
                || k.style().float != super::FloatValue::None
                || super::anonymous::only_collapsible(k)
            {
                continue;
            }
            if let Proto::Reused { root, .. } = k {
                *k = super::incremental::expand(ctx.old, *root);
            }
            if let Proto::Block {
                tag: BoxTag::None,
                inner: InnerDisplay::Flow | InnerDisplay::FlowRoot,
                atomic: false,
                children,
                ..
            } = k
            {
                self.expand_first_line_path(children);
            }
            break;
        }
    }

    pub(crate) fn expand_reused(&self, kids: &mut [Proto]) {
        let Some(ctx) = self.reuse.as_ref() else {
            return;
        };
        for k in kids.iter_mut() {
            if let Proto::Reused { root, .. } = k {
                *k = super::incremental::expand(ctx.old, *root);
            }
        }
    }

    /// The display dispatch of one element (FR-002, FR-003), after the HTML rendering rules.
    fn element_dispatch(&mut self, node: NodeId, style: BoxStyle, out: &mut Vec<Proto>) {
        let display = self.display(node, &style);
        if !matches!(display, DisplayValue::None | DisplayValue::Contents)
            && let Some(info) = self.src.html(node)
        {
            match html_rendering::special(self, node, &info, &style, display) {
                Special::Ordinary => {}
                Special::Skip => return,
                Special::Generated(p) => {
                    out.push(*p);
                    return;
                }
            }
        }
        match display {
            DisplayValue::None => {}
            DisplayValue::Contents => {
                if self
                    .src
                    .tag_name(node)
                    .is_some_and(contents_behaves_as_none)
                {
                    return;
                }
                // The element's own box is suppressed; its children (and its text, with the
                // element's inherited style) take its place in the parent's box list.
                let kids = self.children(node, &style);
                out.extend(kids);
            }
            DisplayValue::Table | DisplayValue::InlineTable => {
                let kids = self.children(node, &style);
                let kids = table_fixup::fix_table_children(node, &style, kids);
                out.push(table_fixup::assemble(
                    BoxOwner::Anonymous(node),
                    BoxOwner::Element(node),
                    style,
                    kids,
                    display == DisplayValue::InlineTable,
                ));
            }
            DisplayValue::TableRowGroup
            | DisplayValue::TableHeaderGroup
            | DisplayValue::TableFooterGroup => {
                let kind = match display {
                    DisplayValue::TableHeaderGroup => RowGroupKind::Header,
                    DisplayValue::TableFooterGroup => RowGroupKind::Footer,
                    _ => RowGroupKind::Body,
                };
                let kids = self.children(node, &style);
                let kids = table_fixup::fix_group_children(node, &style, kids);
                out.push(self.table_part(
                    node,
                    style,
                    InnerDisplay::Flow,
                    TableTag::RowGroup(kind),
                    kids,
                ));
            }
            DisplayValue::TableRow => {
                let kids = self.children(node, &style);
                let kids = table_fixup::fix_row_children(node, &style, kids);
                out.push(self.table_part(node, style, InnerDisplay::Flow, TableTag::Row, kids));
            }
            DisplayValue::TableCell => {
                let (col_span, row_span) = self.cell_spans(node);
                let tag = BoxTag::Table(TableTag::Cell { col_span, row_span });
                out.push(self.block_boxed(
                    BoxOwner::Element(node),
                    node,
                    style,
                    InnerDisplay::FlowRoot,
                    false,
                    tag,
                    false,
                ));
            }
            DisplayValue::TableCaption => {
                let tag = BoxTag::Table(TableTag::Caption);
                out.push(self.block_boxed(
                    BoxOwner::Element(node),
                    node,
                    style,
                    InnerDisplay::FlowRoot,
                    false,
                    tag,
                    false,
                ));
            }
            DisplayValue::TableColumn => {
                // CSS 2.1 §17.2.1 rule 1.1: the children of a column generate nothing.
                let span = self.column_span(node, "col");
                out.push(self.table_part(
                    node,
                    style,
                    InnerDisplay::Flow,
                    TableTag::Column { span },
                    Vec::new(),
                ));
            }
            DisplayValue::TableColumnGroup => {
                let kids = self.children(node, &style);
                let cols = table_fixup::fix_column_group_children(kids);
                // `span` counts only when the group has no `col` child (HTML §4.9.12).
                let span = if cols.is_empty() {
                    self.column_span(node, "colgroup")
                } else {
                    1
                };
                out.push(self.table_part(
                    node,
                    style,
                    InnerDisplay::Flow,
                    TableTag::ColumnGroup { span },
                    cols,
                ));
            }
            d if d.is_block_level() => {
                let inner = d.inner().unwrap_or(InnerDisplay::Flow);
                out.push(self.block(
                    BoxOwner::Element(node),
                    node,
                    style,
                    inner,
                    d == DisplayValue::ListItem,
                ));
            }
            d if d.is_atomic_inline() => {
                // `inline-block`, `inline-flex`, `inline-grid`: one atomic inline-level box whose
                // contents are a flow-root, flex or grid formatting context (FR-010).
                let inner = d.inner().unwrap_or(InnerDisplay::FlowRoot);
                out.push(self.block_boxed(
                    BoxOwner::Element(node),
                    node,
                    style,
                    inner,
                    false,
                    BoxTag::None,
                    true,
                ));
            }
            _ => {
                // `inline` and the unsupported values (boxed as `inline`).
                let kids = self.children(node, &style);
                out.extend(split_inline(node, style, kids));
            }
        }
    }

    fn block(
        &mut self,
        owner: BoxOwner,
        node: NodeId,
        style: BoxStyle,
        inner: InnerDisplay,
        list_item: bool,
    ) -> Proto {
        self.block_boxed(owner, node, style, inner, list_item, BoxTag::None, false)
    }

    /// A container box for `node`: block-level, or atomic inline-level when `atomic`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn block_boxed(
        &mut self,
        owner: BoxOwner,
        node: NodeId,
        style: BoxStyle,
        inner: InnerDisplay,
        list_item: bool,
        tag: BoxTag,
        atomic: bool,
    ) -> Proto {
        // The marker's number is read before the children are built: a nested list creates its own
        // `list-item` counter, which must not be seen by the item's marker (CSS Lists 3 section 4.4).
        let marker = if list_item {
            self.marker(node, &style)
        } else {
            None
        };
        let mut kids = self.children(node, &style);
        // 015 TXT-5: the `::first-letter` of a block container (flow content: not flex, grid or table).
        if let Some(fl) = style.first_letter.clone()
            && matches!(inner, InnerDisplay::Flow | InnerDisplay::FlowRoot)
            && matches!(
                tag,
                BoxTag::None | BoxTag::Table(TableTag::Cell { .. } | TableTag::Caption)
            )
        {
            self.apply_first_letter(&style, &fl, &mut kids);
        }
        if let Some(marker) = marker {
            // The marker may join the first line of a descendant: no child subtree stays taken over, nor any
            // subtree on the way down to that line (found by the multicol random harness, lane L-MC2).
            self.expand_reused(&mut kids);
            self.expand_first_line_path(&mut kids);
            super::pseudo::place_marker(&mut kids, marker, &mut self.tainted);
        }
        // Text and inline-level children of a flex or grid container become anonymous items,
        // even when no block-level sibling exists (FR-008).
        let always = matches!(inner, InnerDisplay::Flex | InnerDisplay::Grid);
        let mut children = wrap_inline_runs(node, &style, kids, always);
        // A multicol container (015 MC-1): its content moves into anonymous flow-thread boxes, one per row;
        // an in-flow block-level child with `column-span: all` (a spanner) ends a row and stays a child of
        // the container, laid out at its full width.
        if !atomic
            && tag == BoxTag::None
            && matches!(inner, InnerDisplay::Flow | InnerDisplay::FlowRoot)
            && crate::layout::fragmentation::multicol(&style).is_some()
        {
            let flow = |children: Vec<Proto>| Proto::Block {
                owner: BoxOwner::Anonymous(node),
                style: BoxStyle::inheriting(&style, DisplayValue::FlowRoot),
                children,
                inner: InnerDisplay::FlowRoot,
                list_item: false,
                tag: BoxTag::FlowThread,
                atomic: false,
            };
            // Spanners nested inside blocks of the container's block formatting context (015 MC-2) are lifted
            // out; the blocks around them are split into pieces, one per row.
            children = self.lift_spanners(children, style.writing_mode);
            let spans = children
                .iter()
                .any(|c| c.is_block_level() && c.style().frag.column_span_all);
            if !spans {
                children = vec![flow(children)];
            } else {
                let mut rows = Vec::new();
                let mut run: Vec<Proto> = Vec::new();
                for c in children {
                    if c.is_block_level() && c.style().frag.column_span_all {
                        if !run.is_empty() {
                            rows.push(flow(std::mem::take(&mut run)));
                        }
                        rows.push(c);
                    } else {
                        run.push(c);
                    }
                }
                if !run.is_empty() {
                    rows.push(flow(run));
                }
                children = rows;
            }
        }
        Proto::Block {
            owner,
            style,
            children,
            inner,
            list_item,
            tag,
            atomic,
        }
    }
}

impl<S: StyledSource> Builder<'_, S> {
    /// The children of a multicol container with every **nested spanner** lifted out (015 MC-2; CSS Multicol 1
    /// section 6): a `column-span: all` box inside in-flow blocks that take part in the container's block
    /// formatting context spans the container like a child would. Each such ancestor is split into pieces
    /// around it: the content before goes to a piece in the row above, the content after to a piece in the row
    /// below (each piece is a box of the element, `FragStyle::split`, whose margin, border and padding on a
    /// split side are dropped: `box-decoration-break: slice`). The rows are formed afterwards from the
    /// top-level spanners.
    pub(crate) fn lift_spanners(
        &mut self,
        children: Vec<Proto>,
        wm: crate::layout::writing_mode::WritingMode,
    ) -> Vec<Proto> {
        if !children.iter().any(|c| self.holds_spanner(c, wm)) {
            return children;
        }
        let mut out = Vec::with_capacity(children.len() + 2);
        for c in children {
            if self.holds_spanner(&c, wm) {
                out.extend(self.split_around_spanners(c, wm));
            } else {
                out.push(c);
            }
        }
        out
    }

    /// `p` is a block the container's flow passes through (`passes_spanners`) and a spanner is inside it.
    fn holds_spanner(&self, p: &Proto, wm: crate::layout::writing_mode::WritingMode) -> bool {
        match p {
            Proto::Block {
                style,
                children,
                inner,
                tag,
                atomic,
                ..
            } => {
                p.is_block_level()
                    && passes_spanners(style, *inner, *tag, *atomic, wm)
                    && children
                        .iter()
                        .any(|c| is_spanner(c) || self.holds_spanner(c, wm))
            }
            Proto::Reused { root, .. } => self
                .reuse
                .as_ref()
                .is_some_and(|ctx| old_holds_spanner(ctx.old, *root, wm)),
            _ => false,
        }
    }

    /// Split `p` (see `holds_spanner`) around the spanners inside it: its pieces and the spanners, in order.
    /// Measured in the reference: the first and the last piece always exist (an empty one keeps the box's
    /// border and padding on its outer side); a piece between two spanners only when content is between them.
    fn split_around_spanners(
        &mut self,
        p: Proto,
        wm: crate::layout::writing_mode::WritingMode,
    ) -> Vec<Proto> {
        let p = match p {
            Proto::Reused { root, .. } if self.reuse.is_some() => {
                super::incremental::expand(self.reuse.as_ref().expect("reuse").old, root)
            }
            p => p,
        };
        let Proto::Block {
            owner,
            style,
            children,
            inner,
            list_item,
            tag,
            atomic,
        } = p
        else {
            return vec![p];
        };
        if let BoxOwner::Element(n) = owner {
            // Its boxes are no longer one subtree of its own build (`incremental.rs`).
            self.tainted.push(n);
        }
        // The element's content between spanners, and the spanners.
        let mut segments: Vec<Vec<Proto>> = vec![Vec::new()];
        let mut spanners: Vec<Proto> = Vec::new();
        let place = |part: Proto, segments: &mut Vec<Vec<Proto>>, spanners: &mut Vec<Proto>| {
            if is_spanner(&part) {
                spanners.push(part);
                segments.push(Vec::new());
            } else {
                segments.last_mut().expect("a segment").push(part);
            }
        };
        for c in children {
            if !is_spanner(&c) && self.holds_spanner(&c, wm) {
                for part in self.split_around_spanners(c, wm) {
                    place(part, &mut segments, &mut spanners);
                }
            } else {
                place(c, &mut segments, &mut spanners);
            }
        }
        let n = segments.len();
        let kept: Vec<bool> = segments
            .iter()
            .enumerate()
            .map(|(k, s)| k == 0 || k + 1 == n || !s.is_empty())
            .collect();
        let total = kept.iter().filter(|k| **k).count();
        let mut out = Vec::with_capacity(n * 2);
        let mut spanners = spanners.into_iter();
        let mut index = 0;
        for (k, seg) in segments.into_iter().enumerate() {
            if kept[k] {
                let piece = split_piece_style(&style, index, total);
                let inner = if index > 0 {
                    InnerDisplay::FlowRoot
                } else {
                    inner
                };
                out.push(Proto::Block {
                    owner,
                    style: piece,
                    children: seg,
                    inner,
                    list_item: list_item && index == 0,
                    tag,
                    atomic,
                });
                index += 1;
            }
            if let Some(sp) = spanners.next() {
                out.push(sp);
            }
        }
        out
    }
}

/// A `column-span: all` box (in flow, block-level).
fn is_spanner(p: &Proto) -> bool {
    p.is_block_level() && p.style().frag.column_span_all
}

/// A block of a multicol container's flow that a nested spanner is lifted out of (015 MC-2): a block
/// container whose content stays in the container's block formatting context (not a flow-root, float,
/// scroll container, contained box, nested multicol, table, fieldset or other formatting context root,
/// not in another writing mode, not a spanner itself). Measured: a spanner in a float or a flow-root
/// does not span.
fn passes_spanners(
    style: &BoxStyle,
    inner: InnerDisplay,
    tag: BoxTag,
    atomic: bool,
    wm: crate::layout::writing_mode::WritingMode,
) -> bool {
    use super::OverflowValue as O;
    !atomic
        && tag == BoxTag::None
        && inner == InnerDisplay::Flow
        && matches!(style.display, DisplayValue::Block | DisplayValue::ListItem)
        && matches!(style.overflow_x, O::Visible | O::Clip)
        && matches!(style.overflow_y, O::Visible | O::Clip)
        && !style.containment.contains_layout_or_paint()
        && !style.frag.is_multicol()
        && !style.frag.column_span_all
        && style.writing_mode == wm
        && style.line_clamp.is_none()
}

/// `holds_spanner` for a box subtree of the previous tree (taken over unchanged: 015 PERF-1).
fn old_holds_spanner(
    old: &BoxTree,
    id: BoxId,
    wm: crate::layout::writing_mode::WritingMode,
) -> bool {
    let b = old.get(id);
    let in_flow = |k: &LayoutBox| {
        k.is_block()
            && !k.style.position.is_out_of_flow()
            && k.style.float == super::FloatValue::None
    };
    if !matches!(b.kind, BoxKind::BlockContainer { .. })
        || !in_flow(b)
        || !passes_spanners(&b.style, b.inner, b.tag, false, wm)
    {
        return false;
    }
    b.children.iter().any(|c| {
        let k = old.get(*c);
        (in_flow(k) && k.style.frag.column_span_all) || old_holds_spanner(old, *c, wm)
    })
}

/// The style of piece `k` of `n` of an element split around nested spanners: margin, border and padding
/// dropped on the split sides (`box-decoration-break: slice`), forced breaks only on the outer sides. A
/// definite block size is not distributed over the pieces (they are auto; reported limit, lane L-MC2).
fn split_piece_style(style: &BoxStyle, k: usize, n: usize) -> BoxStyle {
    let mut s = style.clone();
    let zero = super::LengthPercentage::ZERO;
    if k > 0 {
        // A piece after a split starts at a break, not at an edge of the box: the margins of its first content
        // stay inside it (measured), as in a box that establishes a formatting context.
        s.display = DisplayValue::FlowRoot;
        s.margin.top = super::MarginValue::Lp(zero.clone());
        s.padding.top = zero.clone();
        s.border.top = 0.0;
        s.frag.break_before = super::BreakBetween::Auto;
    }
    if k + 1 < n {
        s.margin.bottom = super::MarginValue::Lp(zero.clone());
        s.padding.bottom = zero;
        s.border.bottom = 0.0;
        s.frag.break_after = super::BreakBetween::Auto;
    }
    s.height = super::SizeValue::Auto;
    s.min_height = super::SizeValue::Auto;
    s.max_height = super::MaxSizeValue::None;
    let end_pb = style.border.bottom
        + if let super::LengthPercentage::Px(p) = style.padding.bottom {
            p
        } else {
            0.0
        };
    s.frag.split = Some(super::SplitPiece {
        index: k as u16,
        last: k + 1 == n,
        end_pb,
    });
    s
}

/// An inline element whose children include block-level boxes is split around them (FR-007,
/// CSS 2.1 §9.2.1.1): the inline pieces before, between and after the blocks stay inline boxes of
/// the same element, in source order; the blocks move out to the inline's parent.
/// The text decorations in effect over an element's text (CSS Text Decoration 3 section 2): the
/// parent's, then the element's own. They do not propagate into floats, out-of-flow boxes or atomic
/// inline-level boxes (inline blocks, inline tables, inline flex and grid containers), whose own
/// decoration starts afresh, and an element that generates no box (`display: contents`) has none
/// of its own.
pub(crate) fn propagate_decorations(
    parent: &BoxStyle,
    own: &BoxStyle,
) -> std::sync::Arc<[crate::text::paragraph::DecorationLayer]> {
    let blocked = own.position.is_out_of_flow()
        || own.display.is_atomic_inline()
        || own.display == DisplayValue::InlineTable;
    if !blocked && own.own_decoration.is_none() {
        return parent.decorations.clone();
    }
    let mut v: Vec<_> = if blocked {
        Vec::new()
    } else {
        parent.decorations.to_vec()
    };
    if own.display != DisplayValue::Contents {
        v.extend(own.own_decoration.iter().cloned());
    }
    v.into()
}

/// The decorations of a first-line variant: those of the style it sits in (`base`), then its own.
pub(crate) fn first_line_decorations(
    base: &Arc<[crate::text::paragraph::DecorationLayer]>,
    fl: &BoxStyle,
) -> Arc<[crate::text::paragraph::DecorationLayer]> {
    let blocked = fl.position.is_out_of_flow()
        || fl.display.is_atomic_inline()
        || fl.display == DisplayValue::InlineTable;
    if !blocked && fl.own_decoration.is_none() {
        return base.clone();
    }
    let mut v: Vec<_> = if blocked { Vec::new() } else { base.to_vec() };
    if fl.display != DisplayValue::Contents {
        v.extend(fl.own_decoration.iter().cloned());
    }
    v.into()
}

/// Complete the first-line variant of `style` (015 TXT-5): its decorations come from the variant of its parent when
/// that has one (the box is inside the first line of an ancestor block), else from the block's own decorations (a
/// block with `::first-line`: the pseudo-element sits in the block).
pub(crate) fn complete_first_line(parent: &BoxStyle, style: &mut BoxStyle) {
    let base = match &parent.first_line {
        Some(pfl) => pfl.decorations.clone(),
        None => style.decorations.clone(),
    };
    let normal_white_space = style.white_space;
    let normal_flow = style.flow.clone();
    if let Some(fl) = style.first_line.as_mut() {
        let dec = first_line_decorations(&base, fl);
        let f = Arc::make_mut(fl);
        f.decorations = dec;
        // What `::first-line` may not change (CSS Pseudo 4 section 3.1: white space, wrapping, tabs, hyphenation, line
        // breaking) is the box's own: the variant only differs in the font, colour, spacing, transform and decoration.
        f.white_space = normal_white_space;
        f.flow.tab_size = normal_flow.tab_size;
        f.flow.overflow_wrap = normal_flow.overflow_wrap;
        f.flow.word_break = normal_flow.word_break;
        f.flow.line_break = normal_flow.line_break;
        f.flow.hyphens = normal_flow.hyphens;
        f.flow.hyphenate_character = normal_flow.hyphenate_character.clone();
    }
}

pub(crate) fn split_inline(node: NodeId, style: BoxStyle, kids: Vec<Proto>) -> Vec<Proto> {
    if !kids.iter().any(Proto::is_block_level) {
        return vec![Proto::Inline {
            node,
            pseudo: None,
            style,
            children: kids,
        }];
    }
    let mut out = Vec::new();
    let mut run: Vec<Proto> = Vec::new();
    // The pieces after the first have no start edge and the ones before the last have no end edge.
    let mut first = true;
    let flush = |run: &mut Vec<Proto>, out: &mut Vec<Proto>, first: &mut bool| {
        if !run.is_empty() {
            let mut piece = style.clone();
            piece.split_start = !*first;
            piece.split_end = true;
            *first = false;
            out.push(Proto::Inline {
                node,
                pseudo: None,
                style: piece,
                children: std::mem::take(run),
            });
        }
    };
    for k in kids {
        if k.is_block_level() {
            flush(&mut run, &mut out, &mut first);
            // Whatever follows a block is not the first piece, even when nothing inline came before.
            first = false;
            out.push(k);
        } else {
            run.push(k);
        }
    }
    flush(&mut run, &mut out, &mut first);
    // The last piece (the last item pushed, when it is inline) keeps its end edge.
    if let Some(Proto::Inline { style, .. }) = out.last_mut() {
        style.split_end = false;
    }
    out
}

/// The previous tree a build takes subtrees over from, and the memo of the tree being built.
struct InsertCx<'a> {
    old: Option<(&'a BoxTree, &'a super::incremental::BuildMemo)>,
    roots: HashMap<NodeId, super::incremental::RootMemo>,
}

/// Copy the box subtree of `root` in `cx.old` into `tree` under `parent` (pre-order, ids shifted).
fn insert_reused(
    tree: &mut BoxTree,
    cx: &mut InsertCx<'_>,
    root: BoxId,
    parent: Option<BoxId>,
) -> BoxId {
    let (old, memo) = cx.old.expect("a taken-over subtree has a previous tree");
    let start = root.0 as usize;
    let end = memo.subtree_end[start] as usize;
    let base = tree.boxes.len() as u32;
    if base == root.0 {
        tree.same_ids.push((base, base + (end - start) as u32));
    }
    let shift = |b: BoxId| BoxId(b.0 - root.0 + base);
    tree.boxes.reserve(end - start);
    for b in &old.boxes[start..end] {
        let id = shift(b.id);
        let parent = if b.id == root {
            parent
        } else {
            b.parent.map(shift)
        };
        tree.boxes.push(LayoutBox {
            id,
            owner: b.owner,
            kind: b.kind.clone(),
            tag: b.tag,
            style: b.style.clone(),
            parent,
            children: b.children.iter().map(|c| shift(*c)).collect(),
            inner: b.inner,
            atomic_inline: b.atomic_inline,
            list_item: b.list_item,
        });
        match b.owner {
            BoxOwner::Element(n) => {
                tree.node_to_boxes.entry(n).or_default().push(id);
                if let Some(m) = memo.roots.get(&n)
                    && m.root == b.id
                {
                    cx.roots.insert(
                        n,
                        super::incremental::RootMemo {
                            root: id,
                            ..m.clone()
                        },
                    );
                }
            }
            BoxOwner::Anonymous(n) if b.tag == BoxTag::Table(TableTag::Wrapper) => {
                if let Some(m) = memo.roots.get(&n)
                    && m.root == b.id
                {
                    cx.roots.insert(
                        n,
                        super::incremental::RootMemo {
                            root: id,
                            ..m.clone()
                        },
                    );
                }
            }
            _ => {}
        }
    }
    BoxId(base)
}

fn insert(tree: &mut BoxTree, cx: &mut InsertCx<'_>, proto: Proto, parent: Option<BoxId>) -> BoxId {
    let id = BoxId(tree.boxes.len() as u32);
    let (owner, kind, style, children, inner, atomic_inline, list_item, tag) = match proto {
        Proto::Reused { root, .. } => return insert_reused(tree, cx, root, parent),
        Proto::Block {
            owner,
            style,
            children,
            inner,
            list_item,
            tag,
            atomic,
        } => {
            // An inline root has inline-level content and no in-flow block-level child. Flex and
            // grid containers never are one; a container of out-of-flow boxes only is not one.
            let inline_root = matches!(inner, InnerDisplay::Flow | InnerDisplay::FlowRoot)
                && children.iter().any(|c| {
                    (!c.is_out_of_flow() && !c.is_float())
                        || (c.style().original_inline
                            && c.is_out_of_flow()
                            && children.iter().any(Proto::is_float))
                })
                && children.iter().all(|c| !c.is_block_level());
            let kind = if atomic {
                BoxKind::InlineBlock { inline_root }
            } else {
                BoxKind::BlockContainer { inline_root }
            };
            (
                owner,
                kind,
                style,
                children,
                inner,
                atomic.then_some(inner),
                list_item,
                tag,
            )
        }
        Proto::Inline {
            node,
            pseudo,
            style,
            children,
        } => {
            let owner = pseudo.map_or(BoxOwner::Element(node), |k| BoxOwner::Pseudo(node, k));
            (
                owner,
                BoxKind::InlineBox,
                style,
                children,
                InnerDisplay::Flow,
                None,
                false,
                BoxTag::None,
            )
        }
        Proto::Text {
            text,
            style,
            owner,
            pseudo,
        } => {
            let owner = pseudo.map_or(BoxOwner::Anonymous(owner), |k| BoxOwner::Pseudo(owner, k));
            (
                owner,
                BoxKind::TextRun(text),
                style,
                Vec::new(),
                InnerDisplay::Flow,
                None,
                false,
                BoxTag::None,
            )
        }
        Proto::Replaced {
            node,
            pseudo,
            style,
            content,
        } => {
            let owner = pseudo.map_or(BoxOwner::Element(node), |k| BoxOwner::Pseudo(node, k));
            (
                owner,
                BoxKind::Replaced(content),
                style,
                Vec::new(),
                InnerDisplay::Flow,
                None,
                false,
                BoxTag::None,
            )
        }
    };
    // `has_background` is read for inline boxes only (their line pieces); on every other box it would
    // make a paint-only change (a hover background on a button or a row) a box-tree difference and
    // cost a relayout (L-PERF-b3).
    let mut style = style;
    if !matches!(kind, BoxKind::InlineBox) {
        style.has_background = false;
    }
    tree.boxes.push(LayoutBox {
        id,
        owner,
        kind,
        tag,
        style: std::sync::Arc::new(style),
        parent,
        children: Vec::new(),
        inner,
        atomic_inline,
        list_item,
    });
    if let BoxOwner::Element(n) = owner {
        tree.node_to_boxes.entry(n).or_default().push(id);
    }
    let kids: Vec<BoxId> = children
        .into_iter()
        .map(|c| insert(tree, cx, c, Some(id)))
        .collect();
    tree.boxes[id.0 as usize].children = kids;
    id
}

fn next_generation() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Build the box tree of a styled document. The root element is always block-level (CSS
/// Display 3 §2.7 blockification of the root); its box is the root of the tree.
pub fn build_box_tree(src: &impl StyledSource) -> BoxTree {
    build_box_tree_reusing(src, None)
}

/// [`build_box_tree`] taking unchanged element subtrees over from `prev` (the previous tree and the DOM
/// changes since it was built; `incremental.rs`). The result equals a full build.
pub fn build_box_tree_reusing(
    src: &impl StyledSource,
    prev: Option<(&BoxTree, &super::incremental::BoxDamage)>,
) -> BoxTree {
    let ctx = prev.and_then(|(old, damage)| super::incremental::reuse_ctx(src, old, damage));
    let incremental = ctx.is_some();
    let (tree, read_skipped) = build_with(src, ctx);
    if incremental && read_skipped {
        // A counter was read and a taken-over subtree had written counters: the writes it skipped may
        // be visible, so build in full.
        return build_with(src, None).0;
    }
    tree
}

fn build_with<S: StyledSource>(
    src: &S,
    reuse: Option<super::incremental::ReuseCtx<'_>>,
) -> (BoxTree, bool) {
    let mut tree = BoxTree {
        document: src.document(),
        root: None,
        boxes: Vec::new(),
        node_to_boxes: HashMap::new(),
        reports: Vec::new(),
        generation: next_generation(),
        viewport_overflow: ViewportOverflow::DEFAULT,
        memo: None,
        same_ids: Vec::new(),
    };
    let maps = src.style_maps();
    let Some(root) = src.root_element() else {
        return (tree, false);
    };
    let SourceNode::Element(style) = src.node(root) else {
        return (tree, false);
    };
    let mut style = style.clone();
    style.decorations = style
        .own_decoration
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .into();
    // The principal writing mode (CSS Writing Modes 4 section 8): an `html` root with a `body` child takes
    // that child's `writing-mode` as its used value (015 WM-1; as the reference browser).
    if src.tag_name(root) == Some("html")
        && let Some(body) = src
            .children(root)
            .into_iter()
            .find(|c| src.tag_name(*c) == Some("body"))
        && let SourceNode::Element(bs) = src.node(body)
        && bs.writing_mode != style.writing_mode
    {
        let mut s =
            crate::layout::writing_mode::relayer(&style, style.writing_mode, bs.writing_mode);
        s.writing_mode = bs.writing_mode;
        style = s;
    }
    let old = reuse.as_ref().map(|c| (c.old, c.memo));
    if let Some((o, _)) = old {
        tree.boxes.reserve(o.boxes.len() + 16);
        tree.node_to_boxes.reserve(o.node_to_boxes.len() + 16);
    }
    let mut b = Builder {
        src,
        reports: Vec::new(),
        counters: Default::default(),
        quotes: Default::default(),
        later_siblings: Vec::new(),
        report_log: Vec::new(),
        volatile: 0,
        reuse,
        candidates: Vec::new(),
        skipped_writes: false,
        record: maps.is_some(),
        tainted: Vec::new(),
        top_layer: src
            .top_layer()
            .into_iter()
            .filter(|&n| matches!(src.node(n), SourceNode::Element(_)))
            .collect(),
    };
    let proto = match b.display(root, &style) {
        DisplayValue::None => None,
        DisplayValue::Table | DisplayValue::InlineTable => {
            let mut out = Vec::new();
            b.element(root, style, &mut out);
            out.into_iter().next()
        }
        d => {
            let inner = if d.is_block_level() {
                d.inner().unwrap_or(InnerDisplay::Flow)
            } else {
                InnerDisplay::Flow
            };
            b.enter_counters(root, &style);
            let mut p = b.block(
                BoxOwner::Element(root),
                root,
                style,
                inner,
                d == DisplayValue::ListItem,
            );
            b.append_top_layer(&mut p);
            b.leave_counters();
            Some(p)
        }
    };
    let read_skipped = b.skipped_writes && b.reads() > 0;
    if std::env::var_os("UI4ME_PROFILE").is_some()
        && let Some(r) = b.reuse.as_ref()
    {
        eprintln!(
            "[prof]   incremental box build: unclean {} candidates {} read_skipped {}",
            r.unclean.len(),
            b.candidates.len(),
            read_skipped
        );
    }
    let candidates = std::mem::take(&mut b.candidates);
    let tainted: std::collections::HashSet<NodeId> = b.tainted.iter().copied().collect();
    tree.reports = b.reports;
    let mut cx = InsertCx {
        old,
        roots: HashMap::new(),
    };
    if let Some(p) = proto {
        tree.root = Some(insert(&mut tree, &mut cx, p, None));
    }
    tree.viewport_overflow = propagate_viewport_overflow(src, &tree);
    if let (Some(styles), Some(dom_revision)) = (maps, src.dom_revision()) {
        let subtree_end = super::incremental::subtree_ends(&tree);
        let mut roots = cx.roots;
        for c in candidates {
            if tainted.contains(&c.node) {
                continue;
            }
            let Some(&first) = tree.boxes_of(c.node).first() else {
                continue;
            };
            let root = if c.wrapper {
                match tree.get(first).parent {
                    Some(p)
                        if tree.get(p).owner == BoxOwner::Anonymous(c.node)
                            && tree.get(p).tag == BoxTag::Table(TableTag::Wrapper) =>
                    {
                        p
                    }
                    _ => continue,
                }
            } else {
                first
            };
            let end = subtree_end[root.0 as usize];
            if matches!(
                tree.get(root).kind,
                BoxKind::InlineBox | BoxKind::TextRun(_)
            ) || !tree
                .boxes_of(c.node)
                .iter()
                .all(|b| b.0 >= root.0 && b.0 < end)
            {
                continue;
            }
            roots.insert(
                c.node,
                super::incremental::RootMemo {
                    root,
                    decorations: c.decorations,
                    reports: c.reports,
                    writes: c.writes,
                },
            );
        }
        tree.memo = Some(Arc::new(super::incremental::BuildMemo {
            styles,
            roots,
            subtree_end,
            dom_revision,
        }));
    }
    (tree, read_skipped)
}

/// The content language of an element: its own or the nearest ancestor's `lang` attribute
/// (HTML section 3.2.6.2); empty when none is set.
fn lang_of(doc: &ui4me_document::document::Document, node: NodeId) -> String {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let Ok(Some(l)) = doc.get_attribute(n, "lang")
            && !l.trim().is_empty()
        {
            return l.trim().to_string();
        }
        cur = doc.parent(n).ok().flatten();
    }
    String::new()
}

/// FR-065: the root element's `overflow` goes to the viewport; when it is `visible` in both axes
/// and the root is an `html` element with a `body` child, the first such child's goes instead.
fn propagate_viewport_overflow(src: &impl StyledSource, tree: &BoxTree) -> ViewportOverflow {
    let Some(root) = tree.root else {
        return ViewportOverflow::DEFAULT;
    };
    let take = |id: BoxId| {
        let st = &tree.get(id).style;
        (st.overflow_x != super::OverflowValue::Visible
            || st.overflow_y != super::OverflowValue::Visible)
            .then_some(ViewportOverflow {
                source: Some(id),
                x: ViewportOverflow::used(st.overflow_x),
                y: ViewportOverflow::used(st.overflow_y),
            })
    };
    if let Some(v) = take(root) {
        return v;
    }
    let root_node = tree.get(root).owner.node();
    // Containment on the root or the body stops the body's propagation (CSS Containment 2; as the
    // reference browser, any containment type).
    let contained = |id: BoxId| tree.get(id).style.containment != super::Containment::NONE;
    if src.tag_name(root_node) == Some("html") && !contained(root) {
        let body = src
            .children(root_node)
            .into_iter()
            .find(|c| src.tag_name(*c) == Some("body"));
        if let Some(b) = body.and_then(|b| tree.boxes_of(b).first().copied())
            && !contained(b)
            && let Some(v) = take(b)
        {
            return v;
        }
    }
    ViewportOverflow::DEFAULT
}

// ------------------------------------------------------------------ StyleBridge adapter (lane D view)

/// `StyledSource` over the real DOM (003) and a published style snapshot (006): every styled
/// element's `ComputedStyleView` is mapped once into a `BoxStyle` (FR-017 consumed set; values
/// outside it are recorded for FR-021/FR-022 reporting).
pub struct StyleBridgeSource<'a> {
    doc: &'a ui4me_document::document::Document,
    /// The mapped style of every styled element; an element whose computed values (and `lang`, `dir`)
    /// did not change keeps the same `Arc` from pass to pass (the incremental box build compares them).
    styles: Arc<HashMap<NodeId, Arc<super::BoxStyle>>>,
    /// `::before`, `::after` and `::marker` styles of the elements that have them.
    pseudos: Arc<HashMap<(NodeId, super::PseudoKind), Arc<super::BoxStyle>>>,
    images: Option<&'a dyn Fn(NodeId) -> Option<ui4me_assets::view::ImageState>>,
    css_images: Option<CssImageStates<'a>>,
}

/// 007's natural-size view of CSS images by (element, property key).
type CssImageStates<'a> = &'a dyn Fn(NodeId, &str) -> Option<ui4me_assets::view::ImageState>;

impl std::fmt::Debug for StyleBridgeSource<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleBridgeSource")
            .field("styled", &self.styles.len())
            .finish()
    }
}

/// Interned `&'static str` feature ids (`FeatureId` holds a static; the keyword set is finite).
pub(crate) fn static_feature(id: String) -> FeatureId {
    use std::sync::{Mutex, OnceLock};
    static TABLE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut t = TABLE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let s: &'static str = match t.get(&id) {
        Some(s) => s,
        None => {
            let leaked: &'static str = Box::leak(id.clone().into_boxed_str());
            t.insert(id, leaked);
            leaked
        }
    };
    FeatureId::new(s)
        .unwrap_or_else(|| FeatureId::new("css.property.unknown").expect("registered root"))
}

pub(super) mod map {
    use super::static_feature;
    use crate::layout::box_tree as bt;
    use crate::layout::box_tree::projection_reports;
    use crate::layout::box_tree::typed_layout;
    use crate::layout::coordinator::calc::{CalcExpr, OpaqueCalc};
    use crate::style::view::ComputedStyleView;
    use crate::style::view::groups as g;
    use crate::text::paragraph::{LineHeightSpec, TextStyle, VerticalAlignSpec};
    use std::sync::Arc;
    use ui4me_engine_types::diag::FeatureId;
    use ui4me_engine_types::text_metrics::{FamilyName, FontSpec, FontStyleSpec, GenericFamily};

    fn prop(name: &str) -> FeatureId {
        static_feature(format!("css.property.{name}"))
    }

    pub(crate) fn lp(v: &g::LengthPercentage) -> bt::LengthPercentage {
        match v {
            g::LengthPercentage::Px(p) => bt::LengthPercentage::Px(*p),
            g::LengthPercentage::Percent(p) => bt::LengthPercentage::Percent(*p),
            g::LengthPercentage::Calc(h) => {
                let h = h.clone();
                let has = h.has_percentage();
                bt::LengthPercentage::Calc(Arc::new(CalcExpr::Opaque(OpaqueCalc::new(
                    has,
                    move |b| h.resolve(b),
                ))))
            }
        }
    }

    fn size(v: &g::Size, name: &str, out: &mut Vec<FeatureId>) -> bt::SizeValue {
        match v {
            g::Size::Auto => bt::SizeValue::Auto,
            g::Size::LengthPercentage(x) => bt::SizeValue::Lp(lp(x)),
            _ => {
                out.push(prop(name));
                bt::SizeValue::Auto
            }
        }
    }

    fn width_keyword(v: &g::Size) -> Option<bt::WidthKeyword> {
        match v {
            g::Size::MinContent => Some(bt::WidthKeyword::MinContent),
            g::Size::MaxContent => Some(bt::WidthKeyword::MaxContent),
            // `fit-content(<length-percentage>)` is not a `width` value in the reference browser (grid tracks
            // only): it stays unconsumed and reported, the width is `auto`.
            g::Size::FitContent(None) => Some(bt::WidthKeyword::FitContent(None)),
            g::Size::Stretch => Some(bt::WidthKeyword::Stretch),
            _ => None,
        }
    }

    /// `min-width` with an intrinsic keyword consumed (`min_width_kw`); `stretch` is not (it stays reported).
    fn min_width_keyword(v: &g::Size) -> Option<bt::WidthKeyword> {
        width_keyword(v).filter(|k| *k != bt::WidthKeyword::Stretch)
    }

    /// `width` with the intrinsic keywords consumed (`width_kw`): they are `auto` here.
    fn size_or_kw(v: &g::Size, name: &str, out: &mut Vec<FeatureId>) -> bt::SizeValue {
        if width_keyword(v).is_some() {
            bt::SizeValue::Auto
        } else {
            size(v, name, out)
        }
    }

    fn max(v: &g::Size, name: &str, out: &mut Vec<FeatureId>) -> bt::MaxSizeValue {
        match v {
            g::Size::None => bt::MaxSizeValue::None,
            g::Size::LengthPercentage(x) => bt::MaxSizeValue::Lp(lp(x)),
            _ => {
                out.push(prop(name));
                bt::MaxSizeValue::None
            }
        }
    }

    fn inset(v: &g::Inset) -> bt::InsetValue {
        match v {
            g::Inset::Auto => bt::InsetValue::Auto,
            g::Inset::LengthPercentage(x) => bt::InsetValue::Lp(lp(x)),
        }
    }

    fn margin(v: &g::Margin) -> bt::MarginValue {
        match v {
            g::Margin::Auto => bt::MarginValue::Auto,
            g::Margin::LengthPercentage(x) => bt::MarginValue::Lp(lp(x)),
        }
    }

    fn generic(gf: &g::GenericFamily) -> GenericFamily {
        match gf {
            g::GenericFamily::Serif => GenericFamily::Serif,
            g::GenericFamily::SansSerif => GenericFamily::SansSerif,
            g::GenericFamily::Monospace => GenericFamily::Monospace,
            g::GenericFamily::Cursive => GenericFamily::Cursive,
            g::GenericFamily::Fantasy => GenericFamily::Fantasy,
            g::GenericFamily::SystemUi => GenericFamily::SystemUi,
            g::GenericFamily::Math => GenericFamily::Math,
            g::GenericFamily::Emoji => GenericFamily::Emoji,
        }
    }

    pub(super) fn display(d: &g::Display) -> bt::DisplayValue {
        use g::{DisplayInside as I, DisplayOutside as O};
        match (d.outside, d.inside, d.list_item) {
            (O::None, I::None, _) => bt::DisplayValue::None,
            (O::Contents, I::Contents, _) => bt::DisplayValue::Contents,
            (O::Block, I::Flow, false) => bt::DisplayValue::Block,
            (O::Block, I::Flow, true) => bt::DisplayValue::ListItem,
            (O::Inline, I::Flow, false) => bt::DisplayValue::Inline,
            (O::Block, I::FlowRoot, false) => bt::DisplayValue::FlowRoot,
            (O::Inline, I::FlowRoot, false) => bt::DisplayValue::InlineBlock,
            (O::Block, I::Flex, false) => bt::DisplayValue::Flex,
            (O::Inline, I::Flex, false) => bt::DisplayValue::InlineFlex,
            (O::Block, I::Grid, false) => bt::DisplayValue::Grid,
            (O::Inline, I::Grid, false) => bt::DisplayValue::InlineGrid,
            // The table family (T100), by the keyword the style engine serialises.
            (_, _, false) => match d.keyword() {
                // The legacy flexible box (`-webkit-box`) other than the clamped one (which Stylo turns into
                // `flow-root`): laid out as a block container, reported (the children of a horizontal box would
                // sit in a row).
                "-webkit-box" => bt::DisplayValue::FlowRoot,
                "-webkit-inline-box" => bt::DisplayValue::InlineBlock,
                "table" => bt::DisplayValue::Table,
                "inline-table" => bt::DisplayValue::InlineTable,
                "table-row-group" => bt::DisplayValue::TableRowGroup,
                "table-header-group" => bt::DisplayValue::TableHeaderGroup,
                "table-footer-group" => bt::DisplayValue::TableFooterGroup,
                "table-row" => bt::DisplayValue::TableRow,
                "table-cell" => bt::DisplayValue::TableCell,
                "table-column" => bt::DisplayValue::TableColumn,
                "table-column-group" => bt::DisplayValue::TableColumnGroup,
                "table-caption" => bt::DisplayValue::TableCaption,
                _ => bt::DisplayValue::Unsupported(static_feature(d.feature_id())),
            },
            // `inline list-item`, ruby, -webkit-box and the rest: FR-018.
            _ => bt::DisplayValue::Unsupported(static_feature(d.feature_id())),
        }
    }

    pub(super) fn box_style(v: &ComputedStyleView<'_>) -> bt::BoxStyle {
        let (df, sz, bm, po, font, it) = (
            v.display_flow(),
            v.sizing(),
            v.box_model(),
            v.position_offsets(),
            v.font(),
            v.inline_text(),
        );
        let mut un = Vec::new();
        let rtl = it.direction == g::Direction::Rtl;
        if matches!(df.display.keyword(), "-webkit-box" | "-webkit-inline-box") {
            un.push(static_feature(df.display.feature_id()));
        }
        let float = match df.float {
            g::Float::None => bt::FloatValue::None,
            g::Float::Left => bt::FloatValue::Left,
            g::Float::Right => bt::FloatValue::Right,
            g::Float::InlineStart => {
                if rtl {
                    bt::FloatValue::Right
                } else {
                    bt::FloatValue::Left
                }
            }
            g::Float::InlineEnd => {
                if rtl {
                    bt::FloatValue::Left
                } else {
                    bt::FloatValue::Right
                }
            }
        };
        let clear = match (df.clear_keyword.as_str(), rtl) {
            ("left", _) | ("inline-start", false) | ("inline-end", true) => bt::ClearValue::Left,
            ("right", _) | ("inline-end", false) | ("inline-start", true) => bt::ClearValue::Right,
            ("both", _) => bt::ClearValue::Both,
            _ => bt::ClearValue::None,
        };
        let style = match (font.italic, font.oblique_deg) {
            (true, _) => FontStyleSpec::Italic,
            (false, Some(a)) => FontStyleSpec::Oblique(a),
            (false, None) => FontStyleSpec::Normal,
        };
        let families = font
            .family
            .iter()
            .map(|f| match f {
                g::FontFamily::Named(n) => FamilyName::Named(n.clone()),
                g::FontFamily::Generic(gf) => FamilyName::Generic(generic(gf)),
            })
            .collect();
        let text = TextStyle {
            font: FontSpec {
                families,
                weight: font.weight,
                style,
                stretch: font.width_percent,
                lang: String::new(),
            },
            font_size: font.size_px,
            line_height: match font.line_height {
                g::LineHeight::Normal => LineHeightSpec::Normal,
                g::LineHeight::Number(n) => LineHeightSpec::Number(n),
                g::LineHeight::Px(p) => LineHeightSpec::Px(p),
            },
        };
        let used_border = |w: f32, s: g::BorderStyle| {
            if matches!(s, g::BorderStyle::None | g::BorderStyle::Hidden) {
                0.0
            } else {
                w
            }
        };
        let align_value = |a: g::TextAlign, name: &str| match a {
            g::TextAlign::Start => bt::TextAlignValue::Start,
            g::TextAlign::End => bt::TextAlignValue::End,
            g::TextAlign::Left | g::TextAlign::WebkitLeft => bt::TextAlignValue::Left,
            g::TextAlign::Right | g::TextAlign::WebkitRight => bt::TextAlignValue::Right,
            g::TextAlign::Center | g::TextAlign::WebkitCenter => bt::TextAlignValue::Center,
            g::TextAlign::Justify => bt::TextAlignValue::Justify,
            _ => bt::TextAlignValue::Unsupported(prop(name)),
        };
        let text_align = align_value(it.text_align, "text-align");
        let text_align_last = it
            .text_align_last
            .map(|a| align_value(a, "text-align-last"));
        let text_indent = lp(&it.text_indent);
        let white_space = bt::WhiteSpaceValue {
            collapse: match it.white_space_collapse {
                g::WhiteSpaceCollapse::Collapse => bt::CollapseMode::Collapse,
                g::WhiteSpaceCollapse::Preserve => bt::CollapseMode::Preserve,
                g::WhiteSpaceCollapse::PreserveBreaks => bt::CollapseMode::PreserveBreaks,
                g::WhiteSpaceCollapse::BreakSpaces => bt::CollapseMode::BreakSpaces,
                // Off in the profile (never computed); kept honest if it ever is.
                g::WhiteSpaceCollapse::PreserveSpaces => {
                    un.push(prop("white-space-collapse"));
                    bt::CollapseMode::Preserve
                }
            },
            wrap: it.text_wrap,
        };
        let word_spacing = match &it.word_spacing {
            g::LengthPercentage::Px(p) => *p,
            _ => {
                // A percentage is relative to the advance of the affected character: not consumed.
                un.push(prop("word-spacing"));
                0.0
            }
        };
        let lf = v.line_flow();
        // The legacy `-webkit-line-clamp` clamps only on a vertical `-webkit-box`, as in the reference (elsewhere
        // it does nothing there either); the standard `line-clamp` is not implemented by either, here reported.
        if lf.standard_clamp {
            un.push(prop("line-clamp"));
        }
        let flow = bt::TextFlow {
            tab_size: it.tab_size,
            text_transform: it.text_transform,
            overflow_wrap: it.overflow_wrap,
            word_break: it.word_break,
            line_break: it.line_break,
            hyphens: it.hyphens,
            hyphenate_character: it.hyphenate_character.clone(),
            letter_spacing: it.letter_spacing_px,
            word_spacing,
            text_overflow: it.text_overflow.clone(),
            balance: v.line_flow().balance,
            font_settings: {
                let ot = v.font_open_type();
                crate::text::fonts::features::FontSettings {
                    variations: ot.variations.into(),
                    optical_sizing_none: ot.optical_sizing_none,
                    features: ot.features.into(),
                    no_synthesis_weight: ot.no_synthesis_weight,
                    no_synthesis_style: ot.no_synthesis_style,
                    no_synthesis_small_caps: ot.no_synthesis_small_caps,
                    no_synthesis_position: ot.no_synthesis_position,
                }
            },
        };
        // Text longhands no stage consumes yet: reported where the element's own text is laid out
        // (never silently dropped). `text-transform`, `tab-size`, `letter-spacing`, ... are applied.
        let unconsumed_text: Vec<FeatureId> = [
            "text-justify",
            "font-variant-alternates",
            "font-language-override",
            "font-size-adjust",
            "text-rendering",
        ]
        .into_iter()
        .filter(|name| !v.is_initial(name))
        .map(prop)
        .collect();
        let td = v.text_decoration();
        if td.blink {
            // `blink` is a permitted no-op (CSS Text Decoration 3), but never a silent one.
            un.push(prop("text-decoration-line"));
        }
        let own_decoration = td
            .has_line()
            .then(|| crate::text::paragraph::DecorationLayer {
                owner: match v.pseudo() {
                    Some(crate::style::view::PseudoKind::Before) => {
                        crate::layout::ifc::interface::owner_tag_pseudo(
                            v.node(),
                            bt::PseudoKind::Before,
                        )
                    }
                    Some(crate::style::view::PseudoKind::After) => {
                        crate::layout::ifc::interface::owner_tag_pseudo(
                            v.node(),
                            bt::PseudoKind::After,
                        )
                    }
                    Some(crate::style::view::PseudoKind::Marker) => {
                        crate::layout::ifc::interface::owner_tag_pseudo(
                            v.node(),
                            bt::PseudoKind::Marker,
                        )
                    }
                    Some(crate::style::view::PseudoKind::FirstLetter) => {
                        crate::layout::ifc::interface::owner_tag_pseudo(
                            v.node(),
                            bt::PseudoKind::FirstLetter,
                        )
                    }
                    Some(crate::style::view::PseudoKind::FirstLine) => {
                        crate::layout::ifc::interface::owner_tag_pseudo(
                            v.node(),
                            bt::PseudoKind::FirstLine,
                        )
                    }
                    Some(crate::style::view::PseudoKind::FirstLineBefore) => {
                        crate::layout::ifc::interface::owner_tag_pseudo(
                            v.node(),
                            bt::PseudoKind::FirstLineBefore,
                        )
                    }
                    Some(crate::style::view::PseudoKind::FirstLineAfter) => {
                        crate::layout::ifc::interface::owner_tag_pseudo(
                            v.node(),
                            bt::PseudoKind::FirstLineAfter,
                        )
                    }
                    Some(crate::style::view::PseudoKind::Backdrop) => {
                        crate::layout::ifc::interface::owner_tag_pseudo(
                            v.node(),
                            bt::PseudoKind::Backdrop,
                        )
                    }
                    _ => crate::layout::ifc::interface::owner_tag(v.node()),
                },
                underline: td.underline,
                overline: td.overline,
                line_through: td.line_through,
                style: match td.style {
                    g::TextDecorationStyle::Solid => {
                        crate::text::paragraph::DecorationLineStyle::Solid
                    }
                    g::TextDecorationStyle::Double => {
                        crate::text::paragraph::DecorationLineStyle::Double
                    }
                    g::TextDecorationStyle::Dotted => {
                        crate::text::paragraph::DecorationLineStyle::Dotted
                    }
                    g::TextDecorationStyle::Dashed => {
                        crate::text::paragraph::DecorationLineStyle::Dashed
                    }
                    g::TextDecorationStyle::Wavy => {
                        crate::text::paragraph::DecorationLineStyle::Wavy
                    }
                },
                thickness: match &td.thickness {
                    g::DecorationThickness::Auto => {
                        crate::text::paragraph::DecorationThickness::Auto
                    }
                    g::DecorationThickness::FromFont => {
                        crate::text::paragraph::DecorationThickness::FromFont
                    }
                    // A percentage is of 1em of the decorating box.
                    g::DecorationThickness::LengthPercentage(lp) => {
                        crate::text::paragraph::DecorationThickness::Px(lp.resolve(font.size_px))
                    }
                },
                underline_offset: td
                    .underline_offset
                    .as_ref()
                    .map(|o| o.resolve(font.size_px)),
                underline_position: if td.underline_position.from_font {
                    crate::text::paragraph::UnderlinePos::FromFont
                } else if td.underline_position.under {
                    crate::text::paragraph::UnderlinePos::Under
                } else {
                    crate::text::paragraph::UnderlinePos::Auto
                },
                font: text.clone(),
            });
        if td.underline_position.left || td.underline_position.right {
            // `left` and `right` place the underline in vertical text only: horizontal text ignores them (CSS Text Decoration 4).
        }
        // Layout properties come from typed computed-style groups.
        let intern = |s: &str| static_feature(s.to_string());
        let (flex, align, order, grid, (overflow_x, overflow_y)) = {
            let mut u = projection_reports::Unconsumed {
                out: &mut un,
                intern: &intern,
            };
            let (f, a, gr) = (v.flex(), v.alignment(), v.grid());
            (
                typed_layout::flex_style(&f, &mut u),
                typed_layout::align_style(&a, &mut u),
                typed_layout::order(&f),
                typed_layout::grid_style(&gr, &mut u),
                typed_layout::overflow(v.overflow()),
            )
        };
        let mut style = bt::BoxStyle {
            display: display(&df.display),
            box_sizing: if df.box_sizing_border_box {
                bt::BoxSizing::BorderBox
            } else {
                bt::BoxSizing::ContentBox
            },
            width_kw: width_keyword(&sz.width),
            min_width_kw: min_width_keyword(&sz.min_width),
            height_kw: width_keyword(&sz.height).filter(|k| *k != bt::WidthKeyword::Stretch),
            float,
            clear,
            width: size_or_kw(&sz.width, "width", &mut un),
            height: size_or_kw(&sz.height, "height", &mut un),
            min_width: if min_width_keyword(&sz.min_width).is_some() {
                bt::SizeValue::Auto
            } else {
                size(&sz.min_width, "min-width", &mut un)
            },
            min_height: size(&sz.min_height, "min-height", &mut un),
            max_width: max(&sz.max_width, "max-width", &mut un),
            max_height: max(&sz.max_height, "max-height", &mut un),
            margin: bt::Sides {
                top: margin(&bm.margin.top),
                right: margin(&bm.margin.right),
                bottom: margin(&bm.margin.bottom),
                left: margin(&bm.margin.left),
            },
            padding: bt::Sides {
                top: lp(&bm.padding.top),
                right: lp(&bm.padding.right),
                bottom: lp(&bm.padding.bottom),
                left: lp(&bm.padding.left),
            },
            border: bt::Sides {
                top: used_border(bm.border_width.top, bm.border_style.top),
                right: used_border(bm.border_width.right, bm.border_style.right),
                bottom: used_border(bm.border_width.bottom, bm.border_style.bottom),
                left: used_border(bm.border_width.left, bm.border_style.left),
            },
            text,
            text_align,
            text_align_last,
            text_indent,
            webkit_align: matches!(
                it.text_align,
                g::TextAlign::WebkitLeft | g::TextAlign::WebkitRight | g::TextAlign::WebkitCenter
            ),
            direction: match it.direction {
                g::Direction::Ltr => bt::Direction::Ltr,
                g::Direction::Rtl => bt::Direction::Rtl,
            },
            unicode_bidi: it.unicode_bidi,
            dir_auto: false,
            white_space,
            flow,
            line_clamp: lf.legacy_clamp,
            unconsumed_text,
            first_line: None,
            first_letter: None,
            decorations: own_decoration.iter().cloned().collect::<Vec<_>>().into(),
            own_decoration,
            vertical_align: valign(v, &mut un),
            aspect_ratio: aspect_ratio(v),
            unconsumed_layout: un,
            visibility: match df.visibility {
                g::Visibility::Visible => bt::VisibilityValue::Visible,
                g::Visibility::Hidden => bt::VisibilityValue::Hidden,
                g::Visibility::Collapse => bt::VisibilityValue::Collapse,
            },
            order,
            flex,
            align,
            grid,
            position: match df.position {
                g::Position::Static => bt::PositionValue::Static,
                g::Position::Relative => bt::PositionValue::Relative,
                g::Position::Absolute => bt::PositionValue::Absolute,
                g::Position::Fixed => bt::PositionValue::Fixed,
                g::Position::Sticky => bt::PositionValue::Sticky,
            },
            inset: bt::Sides {
                top: inset(&po.inset.top),
                right: inset(&po.inset.right),
                bottom: inset(&po.inset.bottom),
                left: inset(&po.inset.left),
            },
            z_index: po.z_index,
            top_layer: None,
            anchor: (!po.anchor.is_empty()).then(|| {
                Box::new(bt::AnchorStyle {
                    props: po.anchor.clone(),
                    implicit: None,
                })
            }),
            fieldset_border_top: None,
            legend_report_margin_top: None,
            overflow_x,
            overflow_y,
            claims_fixed_descendants: v.claims_fixed_descendants(),
            containment: containment_style(&v.containment()),
            table: table_style(v, &bm),
            native_control: false,
            field_sizing_content: v.serialize("field-sizing").is_some_and(|s| s == "content"),
            original_inline: matches!(df.original_display.outside, g::DisplayOutside::Inline),
            generated: Arc::new(v.generated()),
            marker_outside: false,
            has_background: v.serialize("background-color").is_some_and(|c| c != "rgba(0, 0, 0, 0)" && c != "transparent")
                || v.serialize("background-image").is_some_and(|i| i != "none")
                // An inline box with a `box-shadow` has paint of its own per line piece (L-PAINT3-r14).
                || v.serialize("box-shadow").is_some_and(|i| i != "none"),
            split_start: false,
            split_end: false,
            frag: frag_style(&v.fragmentation()),
            writing_mode: it.writing_mode,
            text_orientation: it.text_orientation,
            text_combine_upright: it.text_combine_upright,
        };
        // 015 WM-1: a vertical box keeps its physical values in its logical frame.
        crate::layout::writing_mode::to_logical(&mut style);
        style
    }

    /// 006's fragmentation group (CSS Multicol 1, CSS Break 3; 015 FRAG-2a) as the box's values.
    fn frag_style(f: &crate::style::view::layout_groups::Fragmentation) -> bt::FragStyle {
        bt::FragStyle {
            column_count: f.column_count,
            column_width: f.column_width,
            column_fill_auto: f.column_fill == crate::style::view::layout_groups::ColumnFill::Auto,
            column_span_all: f.column_span_all,
            column_rule_width: f.column_rule_width,
            break_before: f.break_before,
            break_after: f.break_after,
            break_inside: f.break_inside,
            orphans: f.orphans,
            widows: f.widows,
            decoration_clone: f.decoration_clone,
            split: None,
            page: f
                .page
                .as_deref()
                .map(crate::layout::fragmentation::paged::intern),
        }
    }

    /// 006's containment group (CSS Containment 2, CSS Conditional 5) as the box's containment.
    fn containment_style(c: &crate::style::view::layout_groups::Containment) -> bt::Containment {
        use crate::style::view::layout_groups::ContainerType as T;
        bt::Containment {
            size_inline: c.size_inline,
            size_block: c.size_block,
            layout: c.layout,
            paint: c.paint,
            style: c.style,
            container: match c.container_type {
                T::Normal => bt::ContainerKind::Normal,
                T::Size => bt::ContainerKind::Size,
                T::InlineSize => bt::ContainerKind::InlineSize,
            },
            skips_contents: c.skips_contents,
            intrinsic: c.intrinsic,
        }
    }

    fn border_line(s: g::BorderStyle) -> crate::layout::fragment::BorderLineStyle {
        use crate::layout::fragment::BorderLineStyle as L;
        match s {
            g::BorderStyle::None => L::None,
            g::BorderStyle::Hidden => L::Hidden,
            g::BorderStyle::Solid => L::Solid,
            g::BorderStyle::Dotted => L::Dotted,
            g::BorderStyle::Dashed => L::Dashed,
            g::BorderStyle::Double => L::Double,
            g::BorderStyle::Groove => L::Groove,
            g::BorderStyle::Ridge => L::Ridge,
            g::BorderStyle::Inset => L::Inset,
            g::BorderStyle::Outset => L::Outset,
        }
    }

    /// The table properties (FR-076–FR-081) from the serialised longhands and the border groups.
    fn table_style(v: &ComputedStyleView<'_>, bm: &g::BoxModel) -> bt::TableStyle {
        let get = |name: &str| v.serialize(name).unwrap_or_default();
        let px = |s: &str| {
            s.trim()
                .strip_suffix("px")
                .and_then(|n| n.trim().parse::<f32>().ok())
                .unwrap_or(0.0)
        };
        let spacing = get("border-spacing");
        let mut parts = spacing.split_whitespace();
        let h = parts.next().map(px).unwrap_or(0.0);
        let vv = parts.next().map(px).unwrap_or(h);
        bt::TableStyle {
            border_collapse: if get("border-collapse").trim() == "collapse" {
                bt::BorderCollapseValue::Collapse
            } else {
                bt::BorderCollapseValue::Separate
            },
            border_spacing: (h.max(0.0), vv.max(0.0)),
            caption_side: match get("caption-side").trim() {
                "bottom" | "bottom-outside" | "block-end" => bt::CaptionSideValue::Bottom,
                _ => bt::CaptionSideValue::Top,
            },
            empty_cells_hide: get("empty-cells").trim() == "hide",
            table_layout: if get("table-layout").trim() == "fixed" {
                bt::TableLayoutValue::Fixed
            } else {
                bt::TableLayoutValue::Auto
            },
            border_style: bt::Sides {
                top: border_line(bm.border_style.top),
                right: border_line(bm.border_style.right),
                bottom: border_line(bm.border_style.bottom),
                left: border_line(bm.border_style.left),
            },
            border_opaque: bt::Sides {
                top: bm.border_color.top.to_srgb_u8()[3] == 255,
                right: bm.border_color.right.to_srgb_u8()[3] == 255,
                bottom: bm.border_color.bottom.to_srgb_u8()[3] == 255,
                left: bm.border_color.left.to_srgb_u8()[3] == 255,
            },
        }
    }

    /// `vertical-align` (CSS 2.1 §10.8.1), a shorthand of `alignment-baseline` and `baseline-shift`
    /// in the engine's style system. Percentages resolve against `line-height` and keywords outside
    /// CSS 2.1 are not consumed: they are treated as `baseline` and reported.
    fn valign(v: &ComputedStyleView<'_>, un: &mut Vec<FeatureId>) -> VerticalAlignSpec {
        let ab = v.serialize("alignment-baseline").unwrap_or_default();
        let bs = v.serialize("baseline-shift").unwrap_or_default();
        let (ab, bs) = (ab.trim(), bs.trim());
        match bs {
            "top" => return VerticalAlignSpec::Top,
            "bottom" => return VerticalAlignSpec::Bottom,
            "sub" => return VerticalAlignSpec::Sub,
            "super" => return VerticalAlignSpec::Super,
            "" | "0px" | "0" => {}
            other => match other
                .strip_suffix("px")
                .and_then(|n| n.trim().parse::<f32>().ok())
            {
                Some(px) => return VerticalAlignSpec::Length(px),
                None => {
                    // A percentage is of the element's own `line-height` (CSS 2.1 section 10.8.1);
                    // `line-height: normal` needs the font's metrics, which are not known here.
                    let pct = other
                        .strip_suffix('%')
                        .and_then(|n| n.trim().parse::<f32>().ok());
                    let lh = v.serialize("line-height").and_then(|l| {
                        let l = l.trim();
                        match l.strip_suffix("px") {
                            Some(n) => n.trim().parse::<f32>().ok(),
                            None => {
                                let size = v
                                    .serialize("font-size")?
                                    .trim()
                                    .strip_suffix("px")?
                                    .trim()
                                    .parse::<f32>()
                                    .ok()?;
                                l.parse::<f32>().ok().map(|n| n * size)
                            }
                        }
                    });
                    match (pct, lh) {
                        (Some(p), Some(lh)) => return VerticalAlignSpec::Length(p / 100.0 * lh),
                        _ => {
                            un.push(prop("baseline-shift"));
                            return VerticalAlignSpec::Baseline;
                        }
                    }
                }
            },
        }
        match ab {
            "baseline" | "" => VerticalAlignSpec::Baseline,
            "middle" => VerticalAlignSpec::Middle,
            "text-top" => VerticalAlignSpec::TextTop,
            "text-bottom" => VerticalAlignSpec::TextBottom,
            _ => {
                un.push(prop("alignment-baseline"));
                VerticalAlignSpec::Baseline
            }
        }
    }

    /// Whether a select retains native dropdown metrics. Chromium uses its native
    /// inner line with `appearance: auto` even when borders or backgrounds are
    /// authored; `appearance: none` uses the computed line height instead.
    pub(super) fn native_control(v: &ComputedStyleView<'_>) -> bool {
        v.serialize("appearance")
            .is_some_and(|a| a == "auto" || a == "menulist")
    }

    /// `aspect-ratio` as width / height: `auto`, `<number>`, `<w> / <h>` and `auto <w> / <h>`.
    fn aspect_ratio(v: &ComputedStyleView<'_>) -> Option<f32> {
        let s = v.serialize("aspect-ratio")?;
        let s = s.trim().trim_start_matches("auto").trim();
        let mut parts = s.split('/');
        let w: f32 = parts.next()?.trim().parse().ok()?;
        let h: f32 = match parts.next() {
            Some(h) => h.trim().parse().ok()?,
            None => 1.0,
        };
        (w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()).then_some(w / h)
    }
}

/// The `BoxStyle`s mapped from a style snapshot, kept across passes (L-PERF-b1): the mapping is a pure
/// function of an element's computed values, so an element whose values did not change keeps its
/// mapped style. Only the parts that come from the DOM (`lang`, `dir`) are recomputed each pass.
#[derive(Default)]
pub struct BoxStyleCache {
    /// (environment epoch, font epoch, prefers-dark) of the snapshot the entries were mapped from.
    key: (u64, u64, bool),
    map: HashMap<NodeId, Box<CachedBoxStyle>>,
}

struct CachedBoxStyle {
    id: crate::style::view::StyleIdentity,
    style: super::BoxStyle,
    pseudos: Vec<(super::PseudoKind, super::BoxStyle)>,
    /// The display before blockification, mapped (read for the children of a flex or grid container
    /// boxed as `inline`, S-1 in `StyleBridgeSource::new_cached`).
    original_display: super::DisplayValue,
    /// The styles handed out last time, with the `lang` and `dir=auto` they were completed with: the same
    /// `Arc`s are handed out again while those are unchanged (015 PERF-1).
    issued: Option<Issued>,
}

struct Issued {
    lang: String,
    dir_auto: bool,
    style: Arc<super::BoxStyle>,
    pseudos: Vec<(super::PseudoKind, Arc<super::BoxStyle>)>,
}

impl CachedBoxStyle {
    /// Map the computed values of `node` (`None` when the snapshot has no view of it).
    fn map(
        doc: &ui4me_document::document::Document,
        snapshot: &crate::style::view::StyleSnapshot,
        node: NodeId,
        id: crate::style::view::StyleIdentity,
    ) -> Option<Self> {
        let v = snapshot.view(node)?;
        let mut style = map::box_style(&v);
        if doc.local_name(node).is_ok_and(|n| n == "select") {
            style.native_control = map::native_control(&v);
        } else if doc.local_name(node).is_ok_and(|n| n == "input") {
            // Native checkboxes and radios use a different baseline from
            // appearance:none controls; the individual type is checked later.
            style.native_control = v.serialize("appearance").is_some_and(|a| a != "none");
        }
        // 015 TXT-5: the first-line variant and `::first-letter` ride on the element's style.
        let mapped = |k| {
            snapshot
                .view_pseudo(node, Some(k))
                .map(|pv| Arc::new(map::box_style(&pv)))
        };
        style.first_line = mapped(crate::style::view::PseudoKind::FirstLine);
        style.first_letter = mapped(crate::style::view::PseudoKind::FirstLetter);
        let mut ps = Vec::new();
        for (sk, k) in [
            (
                crate::style::view::PseudoKind::Before,
                super::PseudoKind::Before,
            ),
            (
                crate::style::view::PseudoKind::After,
                super::PseudoKind::After,
            ),
            (
                crate::style::view::PseudoKind::Marker,
                super::PseudoKind::Marker,
            ),
            (
                crate::style::view::PseudoKind::FirstLineBefore,
                super::PseudoKind::FirstLineBefore,
            ),
            (
                crate::style::view::PseudoKind::FirstLineAfter,
                super::PseudoKind::FirstLineAfter,
            ),
            (
                crate::style::view::PseudoKind::Backdrop,
                super::PseudoKind::Backdrop,
            ),
        ] {
            if let Some(pv) = snapshot.view_pseudo(node, Some(sk)) {
                ps.push((k, map::box_style(&pv)));
            }
        }
        let original_display = map::display(&v.display_flow().original_display);
        Some(CachedBoxStyle {
            id,
            style,
            pseudos: ps,
            original_display,
            issued: None,
        })
    }

    /// This mapping for another element `node` with the same computed values (`id`): the only part of
    /// a mapped style that names its element is the owner of its own text decoration.
    fn for_node(&self, node: NodeId, id: crate::style::view::StyleIdentity) -> CachedBoxStyle {
        use crate::layout::ifc::interface::{owner_tag, owner_tag_pseudo};
        let fix = |s: &mut super::BoxStyle, tag: u64| {
            if let Some(d) = s.own_decoration.as_mut() {
                d.owner = tag;
                s.decorations = std::iter::once(d.clone()).collect::<Vec<_>>().into();
            }
        };
        let mut style = self.style.clone();
        fix(&mut style, owner_tag(node));
        let pseudos = self
            .pseudos
            .iter()
            .map(|(k, p)| {
                let mut p = p.clone();
                fix(&mut p, owner_tag_pseudo(node, *k));
                (*k, p)
            })
            .collect();
        CachedBoxStyle {
            id,
            style,
            pseudos,
            original_display: self.original_display,
            issued: None,
        }
    }

    /// The same box inputs: equal styles, except for `has_background` where no box can read it.
    fn same_mapping(&self, other: &CachedBoxStyle) -> bool {
        // `build_box_tree` keeps `has_background` on inline boxes only; an element whose display (and
        // display before blockification) cannot make one does not pass it to any box.
        let same = |a: &super::BoxStyle,
                    b: &super::BoxStyle,
                    original: Option<&super::DisplayValue>| {
            use super::DisplayValue as D;
            let no_inline_box = |d: &D| !matches!(d, D::Inline | D::Contents | D::Unsupported(_));
            if a.has_background != b.has_background
                && no_inline_box(&a.display)
                && no_inline_box(&b.display)
                && original.is_none_or(no_inline_box)
            {
                let mut b = b.clone();
                b.has_background = a.has_background;
                *a == b
            } else {
                a == b
            }
        };
        self.original_display == other.original_display
            && same(&self.style, &other.style, Some(&self.original_display))
            && self.pseudos.len() == other.pseudos.len()
            && self
                .pseudos
                .iter()
                .zip(&other.pseudos)
                .all(|((ka, a), (kb, b))| ka == kb && same(a, b, None))
    }
}

/// Test aid (L-PERF-b3): the elements of `snapshot` whose mapped styles, as `StyleBridgeSource`
/// builds them (sharing one mapping between elements with the same computed values), differ from
/// mapping each element on its own.
#[doc(hidden)]
pub fn shared_mapping_differences(
    doc: &ui4me_document::document::Document,
    snapshot: &crate::style::view::StyleSnapshot,
) -> (usize, Vec<NodeId>) {
    let mut cache = BoxStyleCache::default();
    let _ = StyleBridgeSource::new_cached(doc, snapshot, &mut cache);
    let mut out = Vec::new();
    for (node, e) in &cache.map {
        let direct = CachedBoxStyle::map(doc, snapshot, *node, e.id.clone())
            .expect("a mapped element has a view");
        if direct.style != e.style
            || direct.pseudos != e.pseudos
            || direct.original_display != e.original_display
        {
            out.push(*node);
        }
    }
    (cache.map.len(), out)
}

impl BoxStyleCache {
    /// Whether every element of `snapshot` maps to the `BoxStyle`s (and pseudo-element styles) of the
    /// pass this cache was last used for (L-PERF-b3). Elements whose computed values changed are mapped
    /// again and compared; the cache is brought up to date either way. Together with an unchanged DOM
    /// (no journal entry that box generation reads) this means the box tree of that pass is still the
    /// box tree, since box generation reads the DOM and these mapped styles only.
    pub fn unchanged_mapping(
        &mut self,
        doc: &ui4me_document::document::Document,
        snapshot: &crate::style::view::StyleSnapshot,
    ) -> bool {
        let inputs = snapshot.revision_inputs();
        if self.key != (inputs.environment, inputs.font, snapshot.prefers_dark()) {
            return false;
        }
        let mut seen = 0usize;
        let mut same = true;
        for node in snapshot.nodes() {
            let Some(id) = snapshot.identity(node) else {
                continue;
            };
            seen += 1;
            let Some(e) = self.map.get_mut(&node) else {
                return false;
            };
            if e.id == id {
                continue;
            }
            let Some(fresh) = CachedBoxStyle::map(doc, snapshot, node, id) else {
                return false;
            };
            same &= e.same_mapping(&fresh);
            **e = fresh;
        }
        same && seen == self.map.len()
    }
}

/// `lang` and `dir=auto` of every element, from the DOM in one top-down walk (an element's `lang` is
/// its own or the nearest ancestor's).
fn lang_and_dir(
    doc: &ui4me_document::document::Document,
) -> HashMap<NodeId, (std::rc::Rc<str>, bool)> {
    let mut out = HashMap::new();
    let Some(root) = doc.document_element() else {
        return out;
    };
    let mut stack: Vec<(NodeId, std::rc::Rc<str>)> = vec![(root, std::rc::Rc::from(""))];
    while let Some((n, inherited)) = stack.pop() {
        let own = doc
            .get_attribute(n, "lang")
            .ok()
            .flatten()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(std::rc::Rc::from);
        let lang = own.unwrap_or(inherited);
        let dir_auto = doc
            .get_attribute(n, "dir")
            .ok()
            .flatten()
            .is_some_and(|d| d.eq_ignore_ascii_case("auto"));
        if let Ok(kids) = doc.children(n) {
            for &c in kids.iter().rev() {
                stack.push((c, lang.clone()));
            }
        }
        out.insert(n, (lang, dir_auto));
    }
    out
}

impl<'a> StyleBridgeSource<'a> {
    pub fn new(
        doc: &'a ui4me_document::document::Document,
        snapshot: &crate::style::view::StyleSnapshot,
    ) -> Self {
        Self::new_cached(doc, snapshot, &mut BoxStyleCache::default())
    }

    /// [`StyleBridgeSource::new`] reusing the styles `cache` mapped from the same values.
    pub fn new_cached(
        doc: &'a ui4me_document::document::Document,
        snapshot: &crate::style::view::StyleSnapshot,
        cache: &mut BoxStyleCache,
    ) -> Self {
        let t0 = std::time::Instant::now();
        let mut styles = HashMap::with_capacity(cache.map.len() + 16);
        let mut pseudos = HashMap::new();
        let inputs = snapshot.revision_inputs();
        let key = (inputs.environment, inputs.font, snapshot.prefers_dark());
        if cache.key != key {
            cache.map.clear();
            cache.key = key;
        }
        let mut old = std::mem::take(&mut cache.map);
        cache.map.reserve(old.len() + 16);
        let dom = lang_and_dir(doc);
        let t1 = t0.elapsed();
        // Elements that share their computed values (Stylo's style sharing: the cells of a table, the
        // items of a list) share the mapping too, with their own decoration owner (L-PERF-b3).
        let mut shared: HashMap<(crate::style::view::StyleIdentity, u8), CachedBoxStyle> =
            HashMap::new();
        let mut owned_lang;
        for node in snapshot.nodes() {
            let Some(id) = snapshot.identity(node) else {
                continue;
            };
            let entry = match old.remove(&node) {
                Some(e) if e.id == id => e,
                _ => {
                    let kind = match doc.local_name(node) {
                        Ok("select") => 1,
                        Ok("input") => 2,
                        _ => 0,
                    };
                    let key = (id.clone(), kind);
                    if let Some(e) = shared.get(&key) {
                        Box::new(e.for_node(node, id))
                    } else {
                        let Some(e) = CachedBoxStyle::map(doc, snapshot, node, id) else {
                            continue;
                        };
                        shared.insert(key, e.for_node(node, e.id.clone()));
                        Box::new(e)
                    }
                }
            };
            let mut entry = entry;
            let (lang, dir_auto) = match dom.get(&node) {
                Some((l, d)) => (&**l, *d),
                None => {
                    owned_lang = lang_of(doc, node);
                    (
                        owned_lang.as_str(),
                        doc.get_attribute(node, "dir")
                            .ok()
                            .flatten()
                            .is_some_and(|d| d.eq_ignore_ascii_case("auto")),
                    )
                }
            };
            if !entry
                .issued
                .as_ref()
                .is_some_and(|i| i.lang == lang && i.dir_auto == dir_auto)
            {
                let mut style = entry.style.clone();
                style.text.font.lang = lang.to_string();
                style.dir_auto = dir_auto;
                let ps = entry
                    .pseudos
                    .iter()
                    .map(|(k, p)| {
                        let mut p = p.clone();
                        p.text.font.lang = lang.to_string();
                        (*k, Arc::new(p))
                    })
                    .collect();
                entry.issued = Some(Issued {
                    lang: lang.to_string(),
                    dir_auto,
                    style: Arc::new(style),
                    pseudos: ps,
                });
            }
            let issued = entry.issued.as_ref().expect("just issued");
            for (k, p) in &issued.pseudos {
                pseudos.insert((node, *k), p.clone());
            }
            styles.insert(node, issued.style.clone());
            cache.map.insert(node, entry);
        }
        // S-1: Stylo blockifies the children of flex/grid containers at computed-value time.
        // When such a container is boxed as `inline` (a value outside the supported set takes
        // the FR-018 fallback), its children get their original (pre-blockification) display
        // back, read from the view (`original_display`). `inline-flex` and `inline-grid` are
        // laid out as flex and grid containers, so their children stay blockified.
        let blockified: Vec<NodeId> = styles
            .iter()
            .filter(|(_, s)| match s.display {
                super::DisplayValue::Unsupported(f) => {
                    matches!(
                        f.as_str(),
                        "css.display.flex"
                            | "css.display.grid"
                            | "css.display.inline-flex"
                            | "css.display.inline-grid"
                    )
                }
                _ => false,
            })
            .flat_map(|(n, _)| doc.children(*n).map(<[NodeId]>::to_vec).unwrap_or_default())
            .collect();
        for c in blockified {
            if let (Some(s), Some(v)) = (styles.get_mut(&c), snapshot.view(c)) {
                Arc::make_mut(s).display = map::display(&v.display_flow().original_display);
            }
        }
        if std::env::var_os("UI4ME_PROFILE").is_some() {
            eprintln!(
                "[prof]   style bridge: lang_dir {:.2} total {:.2}",
                t1.as_secs_f64() * 1e3,
                t0.elapsed().as_secs_f64() * 1e3
            );
        }
        // The implicit anchor of an anchor-positioned popover is the button that invokes it (F0808).
        for (n, s) in styles.iter_mut() {
            if s.anchor
                .as_ref()
                .is_some_and(|a| a.props.insets.iter().any(Option::is_some))
                && let Some(inv) = crate::interaction::popover::invoker_of(doc, *n)
                && let Some(a) = Arc::make_mut(s).anchor.as_mut()
            {
                a.implicit = Some(inv);
            }
        }
        StyleBridgeSource {
            doc,
            styles: Arc::new(styles),
            pseudos: Arc::new(pseudos),
            images: None,
            css_images: None,
        }
    }

    /// Supply 007's natural-size views of `img` elements (FR-070). Without it every image is laid
    /// out as pending (natural size 0×0, FR-072).
    /// 007's natural-size view of CSS images (`list-style-image`, `content: url()`), by node and property.
    pub fn with_css_images(mut self, images: CssImageStates<'a>) -> Self {
        self.css_images = Some(images);
        self
    }

    pub fn with_images(
        mut self,
        images: &'a dyn Fn(NodeId) -> Option<ui4me_assets::view::ImageState>,
    ) -> Self {
        self.images = Some(images);
        self
    }
}

impl StyleBridgeSource<'_> {
    /// A `select`'s rows and popup state from the document (lane L-CTRL-r10): options that
    /// generate no box (`display: none`, e.g. `hidden`) are left out; `optgroup` labels are rows.
    fn select_facts(&self, select: NodeId) -> super::SelectFacts {
        use ui4me_document::ui_state::UiFlags;
        let mut facts = super::SelectFacts {
            multiple: self.doc.select_is_multiple(select),
            display_size: self.doc.select_display_size(select),
            open: self
                .doc
                .ui_flags(select)
                .is_ok_and(|f| f.contains(UiFlags::PICKER_OPEN)),
            ..Default::default()
        };
        if let Ok(Some(c)) = self.doc.control_state(select) {
            facts.highlight = c.picker_highlight;
            facts.scroll = c.picker_scroll;
        }
        for r in self.doc.select_rows(select) {
            match (r.option, r.group) {
                (Some(o), _) => facts.rows.push(super::SelectRowFact {
                    label: self.doc.option_label(o),
                    group: false,
                    indent: r.indent,
                    selected: self.doc.option_selected(o),
                    disabled: self.doc.option_disabled(o),
                    option_index: r.option_index,
                }),
                (None, Some(g)) => {
                    let label = self
                        .doc
                        .get_attribute(g, "label")
                        .ok()
                        .flatten()
                        .unwrap_or("")
                        .to_string();
                    facts.rows.push(super::SelectRowFact {
                        label,
                        group: true,
                        disabled: true,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        facts
    }
}

impl StyledSource for StyleBridgeSource<'_> {
    fn document(&self) -> DocumentId {
        self.doc.id()
    }
    fn top_layer(&self) -> Vec<NodeId> {
        self.doc
            .top_layer()
            .iter()
            .copied()
            .filter(|&n| self.doc.arena().is_connected(n))
            .collect()
    }
    fn root_element(&self) -> Option<NodeId> {
        self.doc.document_element()
    }
    fn children(&self, node: NodeId) -> Vec<NodeId> {
        self.doc
            .children(node)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default()
    }
    fn tag_name(&self, node: NodeId) -> Option<&str> {
        self.doc.local_name(node).ok()
    }
    fn node(&self, node: NodeId) -> SourceNode<'_> {
        use ui4me_document::node::NodeKind;
        match self.doc.node_kind(node) {
            Ok(NodeKind::Element) => match self.styles.get(&node) {
                Some(s) => SourceNode::Element(s),
                None => SourceNode::Ignored,
            },
            Ok(NodeKind::Text) => match self.doc.text_data(node) {
                Ok(t) => SourceNode::Text(t),
                Err(_) => SourceNode::Ignored,
            },
            _ => SourceNode::Ignored,
        }
    }
    fn attr(&self, node: NodeId, name: &str) -> Option<String> {
        let attrs = self.doc.attributes(node).ok()?;
        attrs
            .iter()
            .find(|a| a.name.local.to_string().eq_ignore_ascii_case(name))
            .map(|a| a.value.to_string())
    }
    fn html(&self, node: NodeId) -> Option<super::HtmlInfo> {
        if !matches!(
            self.doc.node_kind(node),
            Ok(ui4me_document::node::NodeKind::Element)
        ) {
            return None;
        }
        let name = self.doc.local_name(node).ok()?;
        if !html_rendering::is_html_special(name) {
            return None;
        }
        let attrs = self
            .doc
            .attributes(node)
            .map(|a| {
                a.iter()
                    .map(|a| {
                        (
                            a.name.local.to_string().to_ascii_lowercase(),
                            a.value.to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let control_value = self.doc.control_value(node);
        let select = (name == "select").then(|| self.select_facts(node));
        Some(super::HtmlInfo {
            name: name.to_ascii_lowercase(),
            attrs,
            control_value,
            select,
        })
    }
    fn image_state(&self, node: NodeId) -> Option<ui4me_assets::view::ImageState> {
        self.images.and_then(|f| f(node))
    }
    fn css_image_state(
        &self,
        node: NodeId,
        property: &str,
    ) -> Option<ui4me_assets::view::ImageState> {
        self.css_images.and_then(|f| f(node, property))
    }
    fn pseudo(&self, node: NodeId, kind: super::PseudoKind) -> Option<&super::BoxStyle> {
        self.pseudos.get(&(node, kind)).map(|p| &**p)
    }
    fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.doc.parent(node).ok().flatten()
    }
    fn style_maps(&self) -> Option<super::incremental::StyleMaps> {
        Some(super::incremental::StyleMaps {
            elements: self.styles.clone(),
            pseudos: self.pseudos.clone(),
        })
    }
    fn dom_revision(&self) -> Option<u64> {
        Some(self.doc.revision())
    }
}
