//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! The slice subset of 008 `contracts/ifc-interface.md` types (§3–§7). Changes here need both
//! 008 and 009 owners. No upstream type appears (AD-33). All lengths are CSS px f32,
//! pre-quantised by the container, in the inline root's content-box space (x right, y down).
//!
//! Joint 008/009 review (R-36): T015 additions (float/atomic host operations, `FinalExtras`)
//! are implemented by 008; 009 review: ACCEPTED 2026-11-05 (lane L009), see
//! planning-handoff/integration-log/ifc-interface-review.md.

use crate::text::paragraph::FrozenParagraph;
use std::sync::Arc;
use ui4me_engine_types::diag::FeatureId;
use ui4me_engine_types::ids::{NodeId, RawId};
use ui4me_engine_types::stage::Stage;

use crate::layout::box_tree::Direction;

/// Available inline size of the root (§4 `constraints`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AvailableInline {
    Definite(f32),
    MinContent,
    MaxContent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootConstraints {
    pub available: AvailableInline,
    pub direction: Direction,
}

/// Float-free band (§4 `line_slot`). Without floats: full width, `valid_height = +∞`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSlot {
    pub inline_start: f32,
    pub inline_size: f32,
    pub valid_height: f32,
}

/// What LAY provides to the inline behaviour (§4; slice subset: no floats, atomics or
/// out-of-flow boxes in inline content).
pub trait InlineLayoutHost {
    fn constraints(&self) -> RootConstraints;

    /// `text-wrap-style: balance` (015 TXT-4): the lines of a short paragraph are broken into the narrowest
    /// width that keeps their number, and aligned within the full width. Default: no.
    fn balance_lines(&self) -> bool {
        false
    }

    /// A legacy line clamp (015 TXT-4): the number of lines the block shows. Default: none.
    fn line_clamp(&self) -> Option<usize> {
        None
    }
    fn line_slot(&mut self, y: f32, height: f32, min_inline: f32) -> LineSlot;
    fn record_static_position(&mut self, item: u64, x: f32, y: f32);
    /// `text-indent` of the root in px (CSS Text 3 section 9): the first line starts this far in
    /// from the start edge (a negative value hangs out) and has that much less room.
    fn text_indent(&self) -> f32 {
        0.0
    }
    /// `text-align-last` of the root, resolved (CSS Text 3 section 6.2); `None`: the paragraph's own
    /// alignment.
    fn align_last(&self) -> Option<crate::text::paragraph::TextAlignment> {
        None
    }

    // ---- T015 additions (ifc-interface.md section 4): defaulted so the slice container and
    // ---- 009's existing callers stay source-compatible. The 008 container overrides them as
    // ---- floats, atomics and out-of-flow content land (US1/US5). Additive; needs 009 review.

    /// Margin-box size of a float met in inline content (shrink-to-fit, same run mode).
    fn measure_float(&mut self, _item: u64) -> FloatSize {
        FloatSize {
            width: 0.0,
            height: 0.0,
        }
    }
    /// Place the float now (CSS 2.1 9.5.1) with its top not above `y_line_top`; returns its margin
    /// box in root space. Irreversible within the pass.
    fn place_float(&mut self, _item: u64, y_line_top: f32) -> FloatRect {
        FloatRect {
            x: 0.0,
            y: y_line_top,
            width: 0.0,
            height: 0.0,
        }
    }
    /// Recursive layout of an atomic inline (the item tag is the atomic's box id): an `Intrinsic`
    /// call returns the margin-box `Contribution` (both min- and max-content, whichever kind was
    /// asked), `Measure` and `Final` return `Metrics`. `Final` is the last layout of the atomic in
    /// the pass; its placement follows in `FinalOutcome::extras`.
    fn layout_atomic(&mut self, _item: u64, _mode: AtomicMode) -> AtomicResult {
        AtomicResult::Metrics(AtomicMetrics {
            margin_width: 0.0,
            margin_height: 0.0,
            baseline: None,
        })
    }
    /// The nearest y below `y` (root content-box space) at which the room beside the floats
    /// changes: where the bottom of a float ends. `None` when no float reaches below `y`. A line
    /// whose first piece does not fit beside floats moves down to it (CSS 2.1 §9.5, "if a
    /// shortened line box is too small to contain any content, then the line box is shifted
    /// downward").
    fn next_float_edge(&mut self, _y: f32) -> Option<f32> {
        None
    }
    /// Whether the out-of-flow box `item` was inline-level before its blockification (its static
    /// position is a point in a line) or block-level (the start of the line below the content
    /// before it, CSS 2.1 §10.3.7).
    fn oof_is_inline_level(&self, _item: u64) -> bool {
        true
    }
    /// `clear` of the `<br>` whose forced break is owned by `owner` (015 STAB-1, HTML `<br clear>` and CSS `clear`
    /// on a `br`): (left, right). The line after that break starts below the floats on those sides.
    fn break_clear(&self, _owner: u64) -> (bool, bool) {
        (false, false)
    }
    /// The lowest y at or below `y` that is clear of the floats on the given sides (CSS 2.1 section 9.5.2).
    fn clear_floats(&mut self, y: f32, _left: bool, _right: bool) -> f32 {
        y
    }
    /// `UNSUPPORTED_FEATURE` under 005's report key (stage `text` or `layout`).
    fn report(&mut self, feature: FeatureId, stage: Stage, node: NodeId);
}

/// Run mode of the inline behaviour (§5.2 `run`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineMode {
    Measure,
    Final,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntrinsicOutcome {
    pub min_content: f32,
    pub max_content: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasureOutcome {
    pub block_size: f32,
    pub first_baseline: Option<f32>,
    pub last_baseline: Option<f32>,
    pub line_count: usize,
}

/// One line box of the final break (§7).
#[derive(Clone, Debug, PartialEq)]
pub struct LineBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Baseline y in root content-box space.
    pub baseline: f32,
    /// UTF-8 byte range of the paragraph's processed text.
    pub cluster_range: std::ops::Range<usize>,
}

/// `FinalOutcome` (§7), slice subset.
#[derive(Clone, Debug)]
pub struct FinalOutcome {
    pub paragraph: Arc<FrozenParagraph>,
    pub block_size: f32,
    pub first_baseline: Option<f32>,
    pub last_baseline: Option<f32>,
    pub lines: Vec<LineBox>,
    /// Placements beyond the slice subset (section 7): atomic inlines and inline-box edges.
    pub extras: FinalExtras,
}

/// Owner tags carried by paragraph spans: a packed `NodeId` (index, generation).
pub fn owner_tag(node: NodeId) -> u64 {
    let r = node.raw();
    ((r.generation.get() as u64) << 32) | r.index as u64
}

pub fn owner_node(tag: u64) -> Option<NodeId> {
    let generation = std::num::NonZeroU32::new(((tag & !PSEUDO_MASK) >> 32) as u32)?;
    Some(NodeId::from_raw(RawId {
        index: tag as u32,
        generation,
    }))
}

/// Bits 60-62 of an owner tag name the generated box or the first-line variant of the element the tag's node
/// identifies: 1 `::before`, 2 `::after`, 3 `::marker`, 4 `::first-letter`, 5 the element on the first line,
/// 6 `::before` and 7 `::after` on the first line (0: the element itself). `owner_node` ignores them, so every consumer
/// that maps a tag to an element sees the originating element.
const PSEUDO_SHIFT: u32 = 60;
const PSEUDO_MASK: u64 = 0b111 << PSEUDO_SHIFT;

/// The owner tag of the text and inline boxes of a pseudo-element of `node` (FR-086).
pub fn owner_tag_pseudo(node: NodeId, kind: crate::layout::box_tree::PseudoKind) -> u64 {
    use crate::layout::box_tree::PseudoKind as K;
    let bits: u64 = match kind {
        K::Before => 1,
        K::After => 2,
        K::Marker => 3,
        K::FirstLetter => 4,
        K::FirstLine => 5,
        K::FirstLineBefore => 6,
        K::FirstLineAfter => 7,
        // A backdrop holds no text: its tag is never read back (015 HTML-1).
        K::Backdrop => 0,
    };
    owner_tag(node) | (bits << PSEUDO_SHIFT)
}

/// The pseudo-element an owner tag names, if any.
pub fn owner_pseudo(tag: u64) -> Option<crate::layout::box_tree::PseudoKind> {
    use crate::layout::box_tree::PseudoKind as K;
    match (tag & PSEUDO_MASK) >> PSEUDO_SHIFT {
        1 => Some(K::Before),
        2 => Some(K::After),
        3 => Some(K::Marker),
        4 => Some(K::FirstLetter),
        5 => Some(K::FirstLine),
        6 => Some(K::FirstLineBefore),
        7 => Some(K::FirstLineAfter),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// T015 types (ifc-interface.md sections 4, 6, 7). The TEXT service types (`AtomicContribution`,
// `ParagraphKey`, ...) are declared by 009 in `ui4me-text` and re-exported by T019; until then
// `AtomicContributionPx` carries the same two numbers (no competing declaration of 009's name).
// ---------------------------------------------------------------------------------------------

/// Margin-box size of a float, CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatSize {
    pub width: f32,
    pub height: f32,
}

/// Margin box of a placed float in the inline root's content-box space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrinsicKind {
    Min,
    Max,
}

/// Run mode of `layout_atomic`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AtomicMode {
    Intrinsic(IntrinsicKind),
    Measure { available: f32 },
    Final { available: f32 },
}

/// Margin-box metrics of an atomic inline after recursive layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicMetrics {
    pub margin_width: f32,
    pub margin_height: f32,
    /// Baseline from the margin-box top (inner formatting context's rule); `None` = bottom edge.
    pub baseline: Option<f32>,
}

/// Margin-box min/max-content contributions of one atomic inline as the host returns them; the
/// driver (009) attaches the item tag to form 009's `AtomicContribution` (review 2026-11-05,
/// point 3: both are kept, no competing declaration).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicContributionPx {
    pub min_content: f32,
    pub max_content: f32,
}

impl AtomicContributionPx {
    /// The text-service form for `item` (`content_widths` input).
    pub fn tagged(self, item: u64) -> AtomicContribution {
        AtomicContribution {
            item,
            min_content: self.min_content,
            max_content: self.max_content,
        }
    }
}

// T019 (IF-44): TEXT service types declared by 009 in `ui4me-text`, re-exported unchanged.
pub use crate::text::paragraph::AtomicContribution;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AtomicResult {
    Metrics(AtomicMetrics),
    Contribution(AtomicContributionPx),
}

/// Placement facts of a `FinalOutcome` beyond the slice subset (section 7), keyed by item tag.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FinalExtras {
    pub atomics: Vec<AtomicPlacement>,
    pub edges: Vec<EdgeSpacerPlacement>,
    pub floats_placed: Vec<(u64, FloatRect)>,
    pub oof_static: Vec<(u64, f32, f32)>,
    pub outside_marker_anchor: Option<MarkerAnchor>,
    /// Per inline box of the paragraph (`BoxId.0`): the ascent and descent of its own font, whole
    /// pixels like a text run's. The content area of a box that holds no text of its own on a line
    /// (only an atomic inline, or only edges) is as high as these (CSS 2.1 section 10.6.1).
    pub box_metrics: Vec<(u32, f32, f32)>,
}

/// The start or end edge spacer of an inline box on a line, in the root's content-box space
/// (`owner` is the inline box's owner tag, `owner_tag(node)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeSpacerPlacement {
    pub owner: u64,
    pub start: bool,
    pub line_index: usize,
    pub x: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicPlacement {
    pub item: u64,
    pub line_index: usize,
    pub margin_box_origin: (f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerAnchor {
    pub first_line_baseline: f32,
    pub line_start: f32,
}

/// Retries per line for the section 6.2 float re-slot loop: float edges in range plus one.
pub fn max_line_retries(float_edges_in_range: usize) -> usize {
    float_edges_in_range + 1
}
