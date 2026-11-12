//! owner: 008-layout-engine — created by 001 slice (R-28); atomic inlines by L-INL-r2
//!
//! The IFC container (008 `ifc-interface.md` §1, §3): one per inline root. It builds and shapes
//! the paragraph once per pass, answers the coordinator's measure requests through the inline
//! behaviour, and keeps the `FinalOutcome` of the last `PerformLayout` (last write wins, §8.2).
//! It implements `InlineLayoutHost`: line slots come from the block formatting context of the
//! root ([`FloatSpace`], the host tree's view of Taffy's `BlockContext`; without floats every slot
//! is the full available width with unbounded valid height) and floats met in the text are laid
//! out and placed through it. Atomic inlines and floats are laid out recursively through
//! [`AtomicHost`], which the host tree implements over its own nodes: the container never touches
//! Taffy.

use crate::style::view::groups::TextOverflowSide;
use crate::text::fonts::TextServices;
use crate::text::paragraph::{ShapedParagraph, SynthKind, SynthRequest, SynthRun};
use std::sync::Arc;
use ui4me_engine_types::diag::FeatureId;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;

use crate::layout::box_tree::{BoxId, BoxTree, Direction, OverflowValue};
use crate::layout::coordinator::calc::quantize_input;
use crate::layout::ifc::inline::{align, builder, driver};
use crate::layout::ifc::interface::{
    AtomicMode, AtomicResult, AvailableInline, FinalOutcome, FloatRect, FloatSize,
    InlineLayoutHost, IntrinsicOutcome, LineSlot, MeasureOutcome, RootConstraints,
};

/// The margin box of a float laid out by the host tree and the properties placement needs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FloatBox {
    /// A float broken across fragmentainers (015 MC-2): never moved past a boundary as a whole.
    pub breakable: bool,
    pub width: f32,
    pub height: f32,
    /// `float: left` (otherwise `right`).
    pub left: bool,
    pub clear: FloatClear,
}

/// `clear` of a float (physical sides).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FloatClear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

/// What the container needs of the tree around it for atomic inlines and floats: recursive layout
/// of an atomic (by the item tag, the atomic's box id) and the placement of its margin box once the
/// lines are final.
pub(crate) trait AtomicHost {
    /// The text services of the layout pass, for a paragraph that is shaped during layout (a first line with its
    /// own style, 015 TXT-5). `None` when the host has none (the paragraph is then laid out without that style).
    fn text_services(&mut self) -> Option<&mut TextServices> {
        None
    }
    fn layout_atomic(&mut self, item: u64, mode: AtomicMode) -> AtomicResult;
    /// The atomic's margin box top-left in the root's content-box space.
    fn place_atomic(&mut self, item: u64, x: f32, y: f32);
    /// Lay a float of the paragraph out (shrink-to-fit in `available`, the root's content width;
    /// `final_pass` clears the cached results first) and return its margin box.
    fn layout_float(&mut self, _item: u64, _available: f32, _final_pass: bool) -> FloatBox {
        FloatBox::default()
    }
    /// The float's margin box top-left in the root's content-box space.
    fn place_float_node(&mut self, _item: u64, _x: f32, _y: f32) {}
    /// The (min-content, max-content) margin-box widths of a float.
    fn float_contribution(&mut self, _item: u64) -> (f32, f32) {
        (0.0, 0.0)
    }
    /// Whether the out-of-flow box `item` was inline-level before blockification.
    fn oof_inline_level(&self, _item: u64) -> bool {
        true
    }
}

/// The float placement of the block formatting context a root takes part in
/// (`coordinator/floats.rs` implements it over Taffy's `BlockContext`). Coordinates are the root's
/// content-box space.
pub(crate) trait FloatSpace {
    /// The room for a line at `y` that is `height` tall (CSS 2.1 section 9.5: a line box is
    /// shortened by every float its vertical extent meets).
    fn slot(&mut self, y: f32, height: f32) -> LineSlot;
    /// Place a float with its margin-box top not above `min_y`; returns the margin box origin.
    fn place(&mut self, item: u64, b: FloatBox, min_y: f32) -> (f32, f32);
    /// The nearest y below `y` where the room beside floats changes.
    fn next_edge(&mut self, y: f32) -> Option<f32>;
    /// The bottom of the lowest float placed so far through this space, `-inf` when none.
    fn floats_bottom(&self) -> f32;
    /// The y below the floats on the given sides (015 STAB-1, line clearance after `<br clear>`), `None` when none.
    fn clearance(&mut self, _left: bool, _right: bool) -> Option<f32> {
        None
    }
}

/// Host for a root's lines: the line slots and floats of its [`FloatSpace`] (none: no floats).
pub(crate) struct FlowHost<'a, 'b> {
    pub constraints: RootConstraints,
    pub reports: &'a mut Vec<(FeatureId, Stage, NodeId)>,
    pub atomics: &'a mut dyn AtomicHost,
    pub space: Option<&'a mut (dyn FloatSpace + 'b)>,
    /// The last layout of the pass is under way (floats are laid out afresh).
    pub final_pass: bool,
    floats: Vec<(u64, FloatBox)>,
    /// `text-indent` in px and the resolved `text-align-last` of the root.
    indent: f32,
    align_last: Option<crate::text::paragraph::TextAlignment>,
    /// `text-wrap-style: balance` and the legacy line clamp of the root (015 TXT-4).
    pub balance: bool,
    pub clamp: Option<usize>,
    /// The `br` boxes whose `clear` is not `none` (015 STAB-1): owner tag, (left, right).
    pub br_clears: Vec<(u64, (bool, bool))>,
}

impl<'a, 'b> FlowHost<'a, 'b> {
    pub fn new(
        constraints: RootConstraints,
        reports: &'a mut Vec<(FeatureId, Stage, NodeId)>,
        atomics: &'a mut dyn AtomicHost,
    ) -> Self {
        FlowHost {
            constraints,
            reports,
            atomics,
            space: None,
            final_pass: false,
            floats: Vec::new(),
            indent: 0.0,
            align_last: None,
            balance: false,
            clamp: None,
            br_clears: Vec::new(),
        }
    }

    /// The margin box and side of a float of the paragraph (laid out on first use).
    pub(crate) fn float_info(&mut self, item: u64) -> FloatBox {
        self.float_box(item)
    }

    fn float_box(&mut self, item: u64) -> FloatBox {
        if let Some((_, b)) = self.floats.iter().find(|(i, _)| *i == item) {
            return *b;
        }
        let available = match self.constraints.available {
            AvailableInline::Definite(w) => w,
            _ => 0.0,
        };
        let b = self.atomics.layout_float(item, available, self.final_pass);
        self.floats.push((item, b));
        b
    }
}

impl InlineLayoutHost for FlowHost<'_, '_> {
    fn constraints(&self) -> RootConstraints {
        self.constraints
    }
    fn balance_lines(&self) -> bool {
        self.balance
    }
    fn line_clamp(&self) -> Option<usize> {
        self.clamp
    }
    fn line_slot(&mut self, y: f32, height: f32, _min_inline: f32) -> LineSlot {
        let w = match self.constraints.available {
            AvailableInline::Definite(w) => w,
            AvailableInline::MaxContent => f32::INFINITY,
            AvailableInline::MinContent => 0.0,
        };
        match (self.space.as_deref_mut(), self.constraints.available) {
            (Some(space), AvailableInline::Definite(_)) => space.slot(y, height),
            _ => LineSlot {
                inline_start: 0.0,
                inline_size: w,
                valid_height: f32::INFINITY,
            },
        }
    }
    fn record_static_position(&mut self, _item: u64, _x: f32, _y: f32) {}
    fn text_indent(&self) -> f32 {
        self.indent
    }
    fn align_last(&self) -> Option<crate::text::paragraph::TextAlignment> {
        self.align_last
    }
    fn layout_atomic(&mut self, item: u64, mode: AtomicMode) -> AtomicResult {
        self.atomics.layout_atomic(item, mode)
    }
    fn measure_float(&mut self, item: u64) -> FloatSize {
        let b = self.float_box(item);
        FloatSize {
            width: b.width,
            height: b.height,
        }
    }
    fn place_float(&mut self, item: u64, y_line_top: f32) -> FloatRect {
        let b = self.float_box(item);
        let Some(space) = self.space.as_deref_mut() else {
            return FloatRect {
                x: 0.0,
                y: y_line_top,
                width: b.width,
                height: b.height,
            };
        };
        let (x, y) = space.place(item, b, y_line_top);
        self.atomics.place_float_node(item, x, y);
        FloatRect {
            x,
            y,
            width: b.width,
            height: b.height,
        }
    }
    fn next_float_edge(&mut self, y: f32) -> Option<f32> {
        self.space.as_deref_mut().and_then(|s| s.next_edge(y))
    }
    fn break_clear(&self, owner: u64) -> (bool, bool) {
        self.br_clears
            .iter()
            .find(|(o, _)| *o == owner)
            .map_or((false, false), |(_, c)| *c)
    }
    fn clear_floats(&mut self, y: f32, left: bool, right: bool) -> f32 {
        self.space
            .as_deref_mut()
            .and_then(|s| s.clearance(left, right))
            .map_or(y, |c| c.max(y))
    }
    fn oof_is_inline_level(&self, item: u64) -> bool {
        self.atomics.oof_inline_level(item)
    }
    fn report(&mut self, feature: FeatureId, stage: Stage, node: NodeId) {
        if !self.reports.contains(&(feature, stage, node)) {
            self.reports.push((feature, stage, node));
        }
    }
}

/// The host the first line of a first-line paragraph is probed in (015 TXT-5): it gives the room the real host gives, but the
/// floats the line meets are only remembered here (their space is shared, placing them is irreversible); the line
/// slots take them away from the real room.
struct ProbeHost<'h, 'a, 'b> {
    inner: &'h mut FlowHost<'a, 'b>,
    /// (top, bottom, width, on the left) of the floats the probe placed.
    floats: Vec<(f32, f32, f32, bool)>,
}

impl InlineLayoutHost for ProbeHost<'_, '_, '_> {
    fn constraints(&self) -> RootConstraints {
        self.inner.constraints()
    }
    fn line_slot(&mut self, y: f32, height: f32, min_inline: f32) -> LineSlot {
        let mut s = self.inner.line_slot(y, height, min_inline);
        for &(top, bottom, w, left) in &self.floats {
            if top < y + height.max(0.01) && bottom > y {
                if left {
                    s.inline_start += w;
                }
                s.inline_size = (s.inline_size - w).max(0.0);
            }
        }
        s
    }
    fn record_static_position(&mut self, _item: u64, _x: f32, _y: f32) {}
    fn text_indent(&self) -> f32 {
        self.inner.text_indent()
    }
    fn align_last(&self) -> Option<crate::text::paragraph::TextAlignment> {
        self.inner.align_last()
    }
    fn layout_atomic(&mut self, item: u64, mode: AtomicMode) -> AtomicResult {
        self.inner.layout_atomic(item, mode)
    }
    fn measure_float(&mut self, item: u64) -> FloatSize {
        self.inner.measure_float(item)
    }
    fn place_float(&mut self, item: u64, y_line_top: f32) -> FloatRect {
        let b = self.inner.float_info(item);
        let slot = self.inner.line_slot(y_line_top, 0.0, 0.0);
        let x = if b.left {
            slot.inline_start
        } else {
            slot.inline_start + slot.inline_size - b.width
        };
        self.floats
            .push((y_line_top, y_line_top + b.height, b.width, b.left));
        FloatRect {
            x,
            y: y_line_top,
            width: b.width,
            height: b.height,
        }
    }
    fn next_float_edge(&mut self, y: f32) -> Option<f32> {
        let own = self
            .floats
            .iter()
            .map(|f| f.1)
            .filter(|&b| b > y + 1e-4)
            .fold(None, |m: Option<f32>, b| Some(m.map_or(b, |m| m.min(b))));
        match (self.inner.next_float_edge(y), own) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    fn oof_is_inline_level(&self, item: u64) -> bool {
        self.inner.oof_is_inline_level(item)
    }
    fn break_clear(&self, owner: u64) -> (bool, bool) {
        self.inner.break_clear(owner)
    }
    fn clear_floats(&mut self, y: f32, left: bool, right: bool) -> f32 {
        let own = self
            .floats
            .iter()
            .filter(|f| (f.3 && left) || (!f.3 && right))
            .map(|f| f.1)
            .fold(y, f32::max);
        self.inner.clear_floats(own, left, right)
    }
    fn report(&mut self, _feature: FeatureId, _stage: Stage, _node: NodeId) {}
}

/// The first line of a paragraph whose block has `::first-line` (015 TXT-5): the paragraph is built twice from the same boxes,
/// once with every box in its first-line style, once as usual. A layout breaks the first in the room it is given,
/// reads where the first line ends, and lays the paragraph out as the first-line text up to there followed by the
/// normal text, with the first line forced to end there (as the reference browser lays out the first line with the
/// first-line style and the rest with the normal one).
/// Where a first line ends: the byte offset and the number of edge spacers and atomic inlines before it.
type FirstEnd = (usize, usize);

#[derive(Clone)]
struct FirstLineState {
    normal: crate::text::paragraph::ParagraphSource,
    first: crate::text::paragraph::ParagraphSource,
    /// `first` shaped, when it lays out differently from `normal` (otherwise `normal`'s shaping is probed).
    first_shaped: Option<Arc<ShapedParagraph>>,
    /// Where the first line ends, by the room it was probed in.
    ends: Vec<(MemoKey, Option<FirstEnd>)>,
    /// The paragraph with the first line's text in the first-line style, by where the line ends.
    mixes: Vec<(FirstEnd, Option<Arc<ShapedParagraph>>)>,
}

/// An `AtomicHost` for a paragraph without atomic inlines (and for build-time reporting).
pub(crate) struct NoAtomics;

impl AtomicHost for NoAtomics {
    fn layout_atomic(&mut self, _item: u64, _mode: AtomicMode) -> AtomicResult {
        AtomicResult::Metrics(crate::layout::ifc::interface::AtomicMetrics {
            margin_width: 0.0,
            margin_height: 0.0,
            baseline: None,
        })
    }
    fn place_atomic(&mut self, _item: u64, _x: f32, _y: f32) {}
}

/// The ascent and descent (whole pixels, as a text run's) of the font of each inline box under the
/// inline root `root`, keyed by `BoxId.0`.
fn inline_box_metrics(tree: &BoxTree, root: BoxId, text: &TextServices) -> Vec<(u32, f32, f32)> {
    use ui4me_engine_types::text_metrics::FontMetricsSource;
    let mut out = Vec::new();
    let mut stack: Vec<BoxId> = tree.get(root).children.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let b = tree.get(id);
        if b.kind == crate::layout::box_tree::BoxKind::InlineBox {
            let r = text
                .metrics_service()
                .query(&b.style.text.font, b.style.text.font_size, false);
            out.push((id.0, r.ascent.round(), r.descent.round()));
            stack.extend(b.children.iter().rev().copied());
        }
    }
    out
}

/// The `br` boxes under the inline root `root` whose `clear` is not `none` (015 STAB-1): an inline box holding only the
/// forced break of its own element (`box_tree/html_rendering.rs`), keyed by the owner tag of that break's text.
fn br_clears(tree: &BoxTree, root: BoxId) -> Vec<(u64, (bool, bool))> {
    use crate::layout::box_tree::ClearValue;
    let mut out = Vec::new();
    let mut stack: Vec<BoxId> = tree.get(root).children.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let b = tree.get(id);
        if b.kind != crate::layout::box_tree::BoxKind::InlineBox {
            continue;
        }
        if b.style.clear != ClearValue::None
            && let [only] = b.children.as_slice()
            && let c = tree.get(*only)
            && matches!(&c.kind, crate::layout::box_tree::BoxKind::TextRun(t) if t == "\n")
            && let crate::layout::box_tree::BoxOwner::Element(node) = b.owner
            && matches!(c.owner, crate::layout::box_tree::BoxOwner::Anonymous(n) | crate::layout::box_tree::BoxOwner::Element(n) if n == node)
        {
            let sides = match b.style.clear {
                ClearValue::Left => (true, false),
                ClearValue::Right => (false, true),
                _ => (true, true),
            };
            out.push((crate::layout::ifc::interface::owner_tag(node), sides));
        }
        stack.extend(b.children.iter().rev().copied());
    }
    out
}

/// Results of the line breaker for one shaped paragraph, kept on the paragraph (`layout_memo`) and so
/// reused across passes while its content is unchanged. Only for paragraphs that no outside state
/// reaches into: no atomic inlines, floats or out-of-flow boxes, no `text-overflow` end run, and a
/// call without a shared float space (flex, grid and table items, flow roots). Entries are made only
/// by calls that raised no report.
#[derive(Default)]
struct IfcMemo {
    intrinsic: Vec<(MemoKey, IntrinsicOutcome)>,
    measure: Vec<(MemoKey, MeasureOutcome)>,
    finals: Vec<(MemoKey, FinalOutcome)>,
}

/// Everything besides the paragraph that a result depends on.
#[derive(Clone, Copy, PartialEq)]
struct MemoKey {
    rtl: bool,
    /// 0 definite, 1 max-content, 2 min-content.
    kind: u8,
    width: u32,
    indent: u32,
    align: Option<crate::text::paragraph::TextAlignment>,
    align_last: Option<crate::text::paragraph::TextAlignment>,
}

const MEMO_CAP: usize = 8;

/// Per inline root state for one layout pass.
#[derive(Clone)]
pub struct InlineContainer {
    pub root: BoxId,
    owner: NodeId,
    direction: Direction,
    shaped: Option<Arc<ShapedParagraph>>,
    alignment: crate::text::paragraph::TextAlignment,
    /// `text-align-last`, resolved.
    align_last: crate::text::paragraph::TextAlignment,
    /// `text-indent` (percentages of the content width).
    indent: crate::layout::box_tree::LengthPercentage,
    /// `text-overflow` at the line-end edge: the synthesized run that replaces the clipped end of
    /// a line that overflows (shaped once with the root's font).
    end_ellipsis: Option<SynthRun>,
    /// `text-wrap-style: balance` on the root (015 TXT-4).
    balance: bool,
    /// The legacy line clamp of the root: the lines shown and the ellipsis that ends the last (015 TXT-4).
    clamp: Option<(usize, SynthRun)>,
    /// The `br` boxes of the paragraph whose `clear` is not `none` (015 STAB-1): owner tag, (left, right).
    br_clears: Vec<(u64, (bool, bool))>,
    /// Items of a paragraph with no line content, in order (no line box exists).
    contentless: Vec<(u64, crate::text::paragraph::InlineItemKind)>,
    /// Static positions of the out-of-flow boxes of the last `PerformLayout`: (item, x, y) in the
    /// root's content-box space; `x` is NaN for a block-level box (the start edge of the root).
    pub(crate) oof_static: Vec<(u64, f32, f32)>,
    /// Floats of the paragraph (item tags).
    float_items: Vec<u64>,
    /// The font ascent and descent of each inline box of the paragraph (`FinalExtras::box_metrics`).
    box_metrics: Vec<(u32, f32, f32)>,
    pub(crate) final_outcome: Option<FinalOutcome>,
    pub(crate) reports: Vec<(FeatureId, Stage, NodeId)>,
    /// Results may be memoised on the shaped paragraph (see [`IfcMemo`]).
    memo_ok: bool,
    /// How many of `reports` construction made (the rest come from layouts).
    construct_reports: usize,
    /// The first-line style of the block (015 TXT-5).
    first_line: Option<FirstLineState>,
}

impl std::fmt::Debug for InlineContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineContainer")
            .field("root", &self.root)
            .field("shaped", &self.shaped)
            .finish()
    }
}

impl InlineContainer {
    /// Build the paragraph source (009) and shape it (009 text services).
    pub fn new(tree: &BoxTree, root: BoxId, text: &mut TextServices) -> Self {
        let b = tree.get(root);
        let mut reports = Vec::new();
        // `start` and `end` resolve against the paragraph's direction: the root's `direction`, or
        // its first strong character for `unicode-bidi: plaintext` and `dir=auto`.
        let direction = builder::root_direction(tree, root).1;
        let constraints = RootConstraints {
            available: AvailableInline::MaxContent,
            direction,
        };
        let mut none = NoAtomics;
        let mut host = FlowHost::new(constraints, &mut reports, &mut none);
        let mut source = builder::build_source(tree, root, &mut host);
        if b.style.writing_mode.is_vertical()
            && let Some(s) = source.as_mut()
        {
            crate::layout::writing_mode::central_spans(s, text);
        }
        let per_line = source.as_ref().is_some_and(|s| s.isolated_paragraphs);
        let alignment = align::resolve_for(
            b.style.text_align,
            direction,
            per_line,
            b.owner.node(),
            &mut host,
        );
        let align_last = align::resolve_last(
            b.style.text_align_last,
            alignment,
            direction,
            per_line,
            b.owner.node(),
            &mut host,
        );
        // A paragraph with no line content (only floats and out-of-flow boxes, CSS 2.1 section 9.4.2)
        // makes no line box: it is not shaped, its items are placed in order.
        let mut contentless = Vec::new();
        let source_copy = if b.style.first_line.is_some() {
            source.clone()
        } else {
            None
        };
        let shaped = match source {
            Some(s) if s.has_line_content() => Some(text.shape(&s)),
            Some(s) => {
                contentless = s.items.iter().map(|i| (i.owner, i.kind)).collect();
                None
            }
            None => None,
        };
        // `::first-line` (015 TXT-5): the same boxes in their first-line style.
        let mut first_line: Option<FirstLineState> = None;
        if b.style.first_line.is_some()
            && let (Some(src), Some(sh)) = (source_copy.as_ref(), shaped.as_ref())
        {
            let mut sink = Vec::new();
            let mut none = NoAtomics;
            let mut fhost = FlowHost::new(constraints, &mut sink, &mut none);
            let mut fsrc = builder::build_source_in(tree, root, &mut fhost, true);
            if b.style.writing_mode.is_vertical()
                && let Some(s) = fsrc.as_mut()
            {
                crate::layout::writing_mode::central_spans(s, text);
            }
            if let Some(fsrc) = fsrc {
                if fsrc.text.len() == src.text.len() && fsrc.items.len() == src.items.len() {
                    let first_shaped = if fsrc.same_layout(src) {
                        None
                    } else {
                        Some(text.shape(&fsrc))
                    };
                    first_line = Some(FirstLineState {
                        normal: src.clone(),
                        first: fsrc,
                        first_shaped,
                        ends: Vec::new(),
                        mixes: Vec::new(),
                    });
                } else if let Some(f) = FeatureId::new("css.pseudo.first-line") {
                    // A `text-transform` that changes the length of the text (`ß` to `SS`) in the first line.
                    reports.push((f, Stage::Layout, b.owner.node()));
                }
                let _ = sh;
            }
        }
        // Small capitals and sub- and superscripts the font cannot make: the reference browser
        // synthesizes them, this engine does not (reported, never approximated).
        if let Some(sh) = &shaped {
            for (span, tag) in sh.missing_synthesized_features() {
                let id = if matches!(tag, b"subs" | b"sups") {
                    crate::text::diagnostics::feature::POSITION_SYNTHESIS
                } else {
                    crate::text::diagnostics::feature::SMALL_CAPS_SYNTHESIS
                };
                if let (Some(f), Some(node)) = (
                    FeatureId::new(id),
                    sh.source()
                        .spans
                        .get(*span as usize)
                        .and_then(|s| crate::layout::ifc::interface::owner_node(s.owner)),
                ) && !reports.iter().any(|r| r.0 == f && r.2 == node)
                {
                    reports.push((f, Stage::Text, node));
                }
            }
        }
        // `hyphens: auto` that cannot hyphenate (no `lang`, no patterns for the language, a bidirectional
        // paragraph): reported, never approximated with another language.
        if let Some(sh) = &shaped {
            for (span, why) in sh.hyphenation_unsupported() {
                let id = crate::text::diagnostics::feature::hyphens_auto_unsupported(*why);
                if let (Some(f), Some(node)) = (
                    FeatureId::new(id),
                    sh.source()
                        .spans
                        .get(*span as usize)
                        .and_then(|s| crate::layout::ifc::interface::owner_node(s.owner)),
                ) && !reports.iter().any(|r| r.0 == f && r.2 == node)
                {
                    reports.push((f, Stage::Text, node));
                }
            }
        }
        // `text-overflow` (CSS UI 4 section 5.1): applies to a block container that clips its
        // overflow, on the line-end edge; the line-start side and right-to-left paragraphs are
        // not implemented and reported (the clip still applies).
        let mut end_ellipsis = None;
        if b.style.overflow_x != OverflowValue::Visible
            && let Some(sh) = &shaped
        {
            let rtl = direction == Direction::Rtl;
            let (start, end) = b.style.flow.text_overflow.start_end(rtl);
            let feature = FeatureId::new("css.property.text-overflow")
                .expect("css.property root is registered");
            if *start != TextOverflowSide::Clip || (rtl && *end != TextOverflowSide::Clip) {
                reports.push((feature, Stage::Layout, b.owner.node()));
            } else {
                let kind = match end {
                    TextOverflowSide::Clip => None,
                    TextOverflowSide::Ellipsis => Some(SynthKind::Ellipsis),
                    TextOverflowSide::String(s) => Some(SynthKind::String(s.clone())),
                };
                end_ellipsis = kind.map(|kind| {
                    text.shape_synthetic(
                        sh,
                        &SynthRequest {
                            kind,
                            at_cluster: 0,
                        },
                    )
                });
            }
        }
        let float_items: Vec<u64> = match (&shaped, &contentless) {
            (Some(sh), _) => sh
                .source()
                .items
                .iter()
                .filter(|i| i.kind == crate::text::paragraph::InlineItemKind::Float)
                .map(|i| i.owner)
                .collect(),
            (None, c) => c
                .iter()
                .filter(|(_, k)| *k == crate::text::paragraph::InlineItemKind::Float)
                .map(|(o, _)| *o)
                .collect(),
        };
        let box_metrics = inline_box_metrics(tree, root, text);
        let br_clears = br_clears(tree, root);
        // `text-wrap-style: balance` applies to a paragraph that wraps; the legacy line clamp to the block
        // container whose lines are these (015 TXT-4).
        let balance = b.style.flow.balance && b.style.white_space.wrap && shaped.is_some();
        let clamp = match (b.style.line_clamp, &shaped) {
            (Some(n), Some(sh)) => Some((
                n.max(1) as usize,
                text.shape_synthetic(
                    sh,
                    &SynthRequest {
                        kind: SynthKind::Ellipsis,
                        at_cluster: 0,
                    },
                ),
            )),
            _ => None,
        };
        // The clamp counts the lines of the clamp box's own inline content; a block child of it keeps
        // its lines whole (reported, never approximated).
        if b.style.line_clamp.is_none()
            && let Some(pb) = b.parent.map(|p| tree.get(p))
            && pb.style.line_clamp.is_some()
            && let Some(f) = FeatureId::new("css.property.-webkit-line-clamp")
        {
            reports.push((f, Stage::Layout, pb.owner.node()));
        }
        let balance = balance && first_line.is_none();
        let memo_ok = end_ellipsis.is_none()
            && first_line.is_none()
            && !balance
            && clamp.is_none()
            && float_items.is_empty()
            && contentless.is_empty()
            && shaped.as_ref().is_some_and(|s| {
                !s.source().items.iter().any(|i| {
                    matches!(
                        i.kind,
                        crate::text::paragraph::InlineItemKind::AtomicInline
                            | crate::text::paragraph::InlineItemKind::Float
                            | crate::text::paragraph::InlineItemKind::OutOfFlow
                    )
                })
            });
        InlineContainer {
            construct_reports: reports.len(),
            first_line,
            memo_ok,
            contentless,
            box_metrics,
            oof_static: Vec::new(),
            float_items,
            end_ellipsis,
            balance,
            clamp,
            br_clears,
            root,
            owner: b.owner.node(),
            direction,
            shaped,
            alignment,
            align_last,
            indent: b.style.text_indent.clone(),
            final_outcome: None,
            reports,
        }
    }

    pub fn owner(&self) -> NodeId {
        self.owner
    }

    /// The same inputs as `other` (formatting-context replay, `coordinator/replay.rs`): the same
    /// shaped paragraph (the content cache hands out one `Arc` per content) and the same resolved
    /// root values. The results (`final_outcome`, `oof_static`, layout-time reports) are not compared.
    pub(crate) fn same_input(&self, other: &InlineContainer) -> bool {
        let shaped = match (&self.shaped, &other.shaped) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        shaped
            && self.root == other.root
            && self.owner == other.owner
            && self.direction == other.direction
            && self.alignment == other.alignment
            && self.align_last == other.align_last
            && self.indent == other.indent
            && self.end_ellipsis == other.end_ellipsis
            && self.balance == other.balance
            && self.clamp == other.clamp
            && self.contentless == other.contentless
            && self.float_items == other.float_items
            && self.box_metrics == other.box_metrics
            && self.memo_ok == other.memo_ok
            && match (&self.first_line, &other.first_line) {
                (None, None) => true,
                (Some(a), Some(b)) => a.normal == b.normal && a.first == b.first,
                _ => false,
            }
    }

    /// Take over the results of `other`, a container with the same inputs ([`Self::same_input`]).
    /// This container as `InlineContainer::new` builds it from the same box subtree (015 PERF-1): its
    /// layout results dropped. `None` when building it again could record a text diagnostic.
    pub(crate) fn fresh_copy(&self) -> Option<InlineContainer> {
        if self.shaped.as_ref().is_some_and(|s| !s.is_clean()) {
            return None;
        }
        let mut c = self.clone();
        c.final_outcome = None;
        c.oof_static.clear();
        c.reports.truncate(self.construct_reports);
        Some(c)
    }

    pub(crate) fn adopt_results(&mut self, other: &InlineContainer) {
        self.final_outcome = other.final_outcome.clone();
        self.oof_static = other.oof_static.clone();
        self.reports = other.reports.clone();
    }

    fn memo_key(
        &self,
        available: AvailableInline,
        indent: f32,
        align: Option<crate::text::paragraph::TextAlignment>,
    ) -> MemoKey {
        let (kind, width) = match available {
            AvailableInline::Definite(w) => (0, quantize_input(w).to_bits()),
            AvailableInline::MaxContent => (1, 0),
            AvailableInline::MinContent => (2, 0),
        };
        MemoKey {
            rtl: self.direction == Direction::Rtl,
            kind,
            width,
            indent: indent.to_bits(),
            align: Some(self.alignment),
            align_last: align,
        }
    }

    /// Run `f` on the paragraph's memo (`None` when this paragraph is not memoisable).
    fn with_memo<R>(&self, f: impl FnOnce(&mut IfcMemo) -> R) -> Option<R> {
        if !self.memo_ok {
            return None;
        }
        let mut guard = self.shaped.as_ref()?.layout_memo().lock().ok()?;
        let slot = guard.get_or_insert_with(|| Box::new(IfcMemo::default()));
        slot.downcast_mut::<IfcMemo>().map(f)
    }

    /// Whether the paragraph holds floats (they need a [`FloatSpace`]).
    pub fn has_floats(&self) -> bool {
        !self.float_items.is_empty()
    }

    fn host<'a, 'b>(
        direction: Direction,
        reports: &'a mut Vec<(FeatureId, Stage, NodeId)>,
        available: AvailableInline,
        atomics: &'a mut dyn AtomicHost,
        space: Option<&'a mut (dyn FloatSpace + 'b)>,
        final_pass: bool,
        text: (f32, Option<crate::text::paragraph::TextAlignment>),
    ) -> FlowHost<'a, 'b> {
        let available = match available {
            AvailableInline::Definite(w) => AvailableInline::Definite(quantize_input(w)),
            a => a,
        };
        let mut h = FlowHost::new(
            RootConstraints {
                available,
                direction,
            },
            reports,
            atomics,
        );
        h.space = space;
        h.final_pass = final_pass;
        h.indent = text.0;
        h.align_last = text.1;
        h
    }

    /// `text-indent` in px for a content width `width` (a percentage of it; of nothing when the
    /// width is not definite).
    fn indent_px(&self, width: f32) -> f32 {
        use crate::layout::box_tree::LengthPercentage as L;
        let basis = if width.is_finite() { width } else { 0.0 };
        match &self.indent {
            L::Px(p) => *p,
            L::Percent(p) => p * basis,
            L::Calc(c) => crate::layout::coordinator::calc::resolve_calc_value(c, basis),
        }
    }

    /// Place the items of a paragraph with no line content in order: floats at the top, the
    /// out-of-flow boxes at the start of the (empty) line, which floats may have shifted.
    fn run_contentless(
        items: &[(u64, crate::text::paragraph::InlineItemKind)],
        host: &mut FlowHost<'_, '_>,
    ) -> Vec<(u64, f32, f32)> {
        let mut statics = Vec::new();
        for &(item, kind) in items {
            match kind {
                crate::text::paragraph::InlineItemKind::Float => {
                    let _ = host.measure_float(item);
                    host.place_float(item, 0.0);
                }
                _ => {
                    let x = if host.oof_is_inline_level(item) {
                        host.line_slot(0.0, 0.0, 0.0).inline_start
                    } else {
                        f32::NAN
                    };
                    statics.push((item, x, 0.0));
                }
            }
        }
        statics
    }

    /// The paragraph to lay out in `available` and where its first line must end, when the block has a first line of
    /// its own (015 TXT-5; see [`FirstLineState`]). `None`: lay the normal paragraph out.
    fn first_line_layout(
        &mut self,
        available: AvailableInline,
        indent: f32,
        atomics: &mut dyn AtomicHost,
        space: Option<&mut (dyn FloatSpace + '_)>,
    ) -> Option<(Arc<ShapedParagraph>, driver::FirstLineMode)> {
        let mut fl = self.first_line.take()?;
        let key = self.memo_key(available, indent, None);
        let end = match fl.ends.iter().find(|(k, _)| *k == key) {
            Some((_, e)) => *e,
            None => {
                let e = fl
                    .first_shaped
                    .clone()
                    .or_else(|| self.shaped.clone())
                    .and_then(|probe_for| {
                        let mut sink = Vec::new();
                        let mut host = Self::host(
                            self.direction,
                            &mut sink,
                            available,
                            &mut *atomics,
                            space,
                            false,
                            (indent, None),
                        );
                        let mut probe = ProbeHost {
                            inner: &mut host,
                            floats: Vec::new(),
                        };
                        driver::first_line_end(&probe_for, &mut probe)
                    });
                fl.ends.push((key, e));
                e
            }
        };
        let out = end.and_then(|end| {
            let sh = match fl.mixes.iter().find(|(e, _)| *e == end) {
                Some((_, sh)) => sh.clone(),
                None => {
                    let mixed = crate::text::paragraph::ParagraphSource::mix_first_line(
                        &fl.first, &fl.normal, end.0,
                    );
                    let sh = match (mixed, atomics.text_services()) {
                        (Some(m), Some(t)) => Some(t.shape(&m)),
                        _ => None,
                    };
                    fl.mixes.push((end, sh.clone()));
                    sh
                }
            };
            sh.map(|sh| (sh, driver::FirstLineMode::Forced(end)))
        });
        self.first_line = Some(fl);
        out
    }

    pub(crate) fn intrinsic(&mut self, atomics: &mut dyn AtomicHost) -> IntrinsicOutcome {
        // Floats add to the widths: their margin boxes are as wide as the widest of them at least
        // (min-content), and sit on the line beside the text at most (max-content).
        let (mut fmin, mut fsum) = (0.0f32, 0.0f32);
        for &item in &self.float_items {
            let (mn, mx) = atomics.float_contribution(item);
            fmin = fmin.max(mn);
            fsum += mx;
        }
        let indent = self.indent_px(f32::INFINITY);
        let text = match self.shaped.clone() {
            Some(s) => {
                let key = self.memo_key(AvailableInline::MaxContent, indent, None);
                let known = self
                    .with_memo(|m| m.intrinsic.iter().find(|(k, _)| *k == key).map(|(_, v)| *v))
                    .flatten();
                let reports_before = self.reports.len();
                let i = match known {
                    Some(v) => v,
                    None => {
                        let mut host = Self::host(
                            self.direction,
                            &mut self.reports,
                            AvailableInline::MaxContent,
                            atomics,
                            None,
                            false,
                            (indent, None),
                        );
                        let v = driver::intrinsic(&s, &mut host);
                        if self.reports.len() == reports_before {
                            self.with_memo(|m| {
                                if m.intrinsic.len() < MEMO_CAP {
                                    m.intrinsic.push((key, v))
                                }
                            });
                        }
                        v
                    }
                };
                // Content contributions are LayoutUnits rounded up (Blink `LayoutUnit::FromFloatCeil`), so a
                // box sized by its max-content width always holds its text: fractional advances (subpixel
                // positioning above device scale 1) otherwise lose up to half a unit to rounding and wrap.
                let up = |w: f32| (w * 64.0 - 1e-3).ceil() / 64.0;
                // The first line in its own style may be wider (015 TXT-5): the contribution is the wider of the two.
                let i = match self
                    .first_line
                    .as_ref()
                    .and_then(|f| f.first_shaped.clone())
                {
                    Some(fs) => {
                        let mut sink = Vec::new();
                        let mut host = Self::host(
                            self.direction,
                            &mut sink,
                            AvailableInline::MaxContent,
                            atomics,
                            None,
                            false,
                            (indent, None),
                        );
                        let j = driver::intrinsic(&fs, &mut host);
                        IntrinsicOutcome {
                            min_content: i.min_content.max(j.min_content),
                            max_content: i.max_content.max(j.max_content),
                        }
                    }
                    None => i,
                };
                IntrinsicOutcome {
                    min_content: up(i.min_content),
                    max_content: up(i.max_content),
                }
            }
            None => IntrinsicOutcome {
                min_content: 0.0,
                max_content: 0.0,
            },
        };
        IntrinsicOutcome {
            min_content: text.min_content.max(fmin),
            max_content: text.max_content + fsum,
        }
    }

    /// `ComputeSize`: break a scratch copy and drop it. Floats are placed in `space` (the block
    /// formatting context of the root), once.
    pub(crate) fn measure(
        &mut self,
        available: AvailableInline,
        atomics: &mut dyn AtomicHost,
        space: Option<&mut (dyn FloatSpace + '_)>,
    ) -> MeasureOutcome {
        let none = MeasureOutcome {
            block_size: 0.0,
            first_baseline: None,
            last_baseline: None,
            line_count: 0,
        };
        let Some(shaped) = self.shaped.clone() else {
            if !self.contentless.is_empty() {
                let mut host = Self::host(
                    self.direction,
                    &mut self.reports,
                    available,
                    atomics,
                    space,
                    false,
                    (0.0, None),
                );
                let _ = Self::run_contentless(&self.contentless, &mut host);
            }
            return none;
        };
        let width = match available {
            AvailableInline::Definite(w) => w,
            _ => f32::INFINITY,
        };
        let text = (self.indent_px(width), Some(self.align_last));
        let key = self.memo_key(available, text.0, text.1);
        let memoisable = space.is_none();
        if memoisable
            && let Some(v) = self
                .with_memo(|m| m.measure.iter().find(|(k, _)| *k == key).map(|(_, v)| *v))
                .flatten()
        {
            return v;
        }
        let reports_before = self.reports.len();
        let mut space = space;
        let first = self.first_line_layout(available, text.0, &mut *atomics, space.as_deref_mut());
        let (shaped, mode) = first.unwrap_or((shaped, driver::FirstLineMode::None));
        let mut host = Self::host(
            self.direction,
            &mut self.reports,
            available,
            atomics,
            space,
            false,
            text,
        );
        host.balance = self.balance;
        host.clamp = self.clamp.as_ref().map(|c| c.0);
        host.br_clears = self.br_clears.clone();
        let v = driver::measure_with(&shaped, &mut host, mode);
        if memoisable && self.reports.len() == reports_before {
            self.with_memo(|m| {
                if m.measure.len() < MEMO_CAP {
                    m.measure.push((key, v))
                }
            });
        }
        v
    }

    /// `PerformLayout`: break, align and freeze; replaces the previous outcome (§8.2). The
    /// atomics and floats are placed in the content-box space of the root.
    pub(crate) fn perform(
        &mut self,
        width: f32,
        atomics: &mut dyn AtomicHost,
        space: Option<&mut (dyn FloatSpace + '_)>,
    ) -> f32 {
        let Some(shaped) = self.shaped.clone() else {
            self.final_outcome = None;
            self.oof_static.clear();
            if !self.contentless.is_empty() {
                let mut host = Self::host(
                    self.direction,
                    &mut self.reports,
                    AvailableInline::Definite(width),
                    atomics,
                    space,
                    true,
                    (0.0, None),
                );
                self.oof_static = Self::run_contentless(&self.contentless, &mut host);
            }
            return 0.0;
        };
        let alignment = self.alignment;
        let text = (self.indent_px(width), Some(self.align_last));
        let key = self.memo_key(AvailableInline::Definite(width), text.0, text.1);
        let memoisable = space.is_none();
        let known = if memoisable {
            self.with_memo(|m| {
                m.finals
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.clone())
            })
            .flatten()
        } else {
            None
        };
        let mut out = match known {
            Some(v) => v,
            None => {
                let reports_before = self.reports.len();
                let mut space = space;
                let first = self.first_line_layout(
                    AvailableInline::Definite(width),
                    text.0,
                    &mut *atomics,
                    space.as_deref_mut(),
                );
                let (shaped, mode) =
                    first.unwrap_or_else(|| (shaped.clone(), driver::FirstLineMode::None));
                let mut host = Self::host(
                    self.direction,
                    &mut self.reports,
                    AvailableInline::Definite(width),
                    atomics,
                    space,
                    true,
                    text,
                );
                host.balance = self.balance;
                host.clamp = self.clamp.as_ref().map(|c| c.0);
                host.br_clears = self.br_clears.clone();
                let v = driver::final_layout_with(
                    &shaped,
                    &mut host,
                    alignment,
                    self.end_ellipsis.as_ref(),
                    self.clamp.as_ref().map(|c| &c.1),
                    mode,
                );
                if memoisable && self.reports.len() == reports_before {
                    self.with_memo(|m| {
                        if m.finals.len() < MEMO_CAP {
                            m.finals.push((key, v.clone()))
                        }
                    });
                }
                v
            }
        };
        if !out.paragraph.overflow_blocked_lines().is_empty() {
            // Hiding an atomic inline whole is not implemented: the line stays clipped.
            let feature = FeatureId::new("css.property.text-overflow")
                .expect("css.property root is registered");
            if !self
                .reports
                .iter()
                .any(|r| r.0 == feature && r.2 == self.owner)
            {
                self.reports.push((feature, Stage::Layout, self.owner));
            }
        }
        for a in &out.extras.atomics {
            atomics.place_atomic(a.item, a.margin_box_origin.0, a.margin_box_origin.1);
        }
        out.extras.box_metrics = self.box_metrics.clone();
        self.oof_static = out.extras.oof_static.clone();
        let h = out.block_size;
        self.final_outcome = Some(out);
        h
    }

    /// Baseline of the last line of the latest `PerformLayout`, from the content-box top.
    pub(crate) fn last_baseline(&self) -> Option<f32> {
        self.final_outcome.as_ref().and_then(|f| f.last_baseline)
    }
}
