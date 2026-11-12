//! owner: 009-text-inline — created by 001 slice (R-28)
//!
//! Scratch layouts and the revertible line breaker (008 `ifc-interface.md` §5.1, §6; 009
//! `text-services.md` §3). A scratch layout is a private clone of a shaped paragraph used for
//! one measurement or the final break; the breaker wraps Parley's `BreakLines`.

use parley::{BreakLines, BreakReason, BreakerState, Layout, YieldData};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

use super::snap::{SnapTable, UnitKind};
use super::source::{InlineItemKind, ShapedParagraph, SpanBrush};

/// Per-line geometry for `Breaker::set_line` (root content-box space, CSS px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineParams {
    pub start_x: f32,
    pub max_advance: f32,
    pub y: f32,
    /// `LineSlot.valid_height`; `f32::INFINITY` without floats.
    pub max_height: f32,
}

/// Why a line ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnd {
    /// End of the paragraph.
    End,
    /// Soft wrap at a UAX #14 opportunity.
    SoftWrap,
    /// Forced break.
    Forced,
    /// Emergency break (no opportunity fitted).
    Emergency,
}

fn line_end(r: BreakReason) -> LineEnd {
    match r {
        BreakReason::None => LineEnd::End,
        BreakReason::Regular => LineEnd::SoftWrap,
        BreakReason::Explicit => LineEnd::Forced,
        BreakReason::Emergency => LineEnd::Emergency,
    }
}

/// Metrics of a committed line (before alignment).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineDoneMetrics {
    pub top: f32,
    pub height: f32,
    pub baseline: f32,
    pub advance: f32,
    /// Trailing white space that hangs (excluded from alignment).
    pub hanging_advance: f32,
    /// Unit index where the line ends: a float or out-of-flow box at a later unit belongs to a
    /// following line.
    pub end_unit: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BreakEvent {
    LineDone {
        metrics: LineDoneMetrics,
        reason: LineEnd,
    },
    /// The line being built is taller than `LineParams::max_height` (rule §6.2).
    MaxHeightExceeded { advance: f32, line_height: f32 },
    /// A float met in the inline content at inline position `advance` of the current line
    /// (rule §6.3; `item` is the owner tag of the `Float` item). The driver measures it and
    /// either places it now or queues it for the next line.
    FloatEncountered { item: u64, advance: f32 },
    /// An out-of-flow placeholder at inline position `advance` on the current line (rule §6.4);
    /// `item` is the owner tag of the `OutOfFlow` item.
    OutOfFlowEncountered { item: u64, advance: f32 },
}

/// A saved breaker state (`checkpoint` / `revert`).
#[derive(Clone)]
pub struct BreakerCheckpoint {
    state: BreakerState,
    cursor: usize,
    lines: usize,
    /// The breaker that took it: a checkpoint is only valid for its own breaker.
    breaker: u64,
}

impl std::fmt::Debug for BreakerCheckpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BreakerCheckpoint")
    }
}

/// The margin box of one atomic inline for one break (`layout_atomic` result): size in px and
/// the baseline from the margin-box top (`None` = bottom margin edge, CSS 2.1 §10.8.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicBox {
    pub item: u64,
    pub width: f32,
    pub height: f32,
    pub baseline: Option<f32>,
}

/// Height handed to Parley for an atomic inline whose margin box is zero tall. Parley treats a
/// line holding only zero-extent inline boxes as an invisible line box of height 0, but an
/// atomic inline makes its line box non-empty (CSS 2.1 section 10.8: the strut applies; Chromium
/// agrees), so the box is given a sub-LayoutUnit height that makes the line content-bearing
/// without changing the line (the strut dominates). Placements report 0 again (`atomic_height`).
pub(crate) const ZERO_ATOMIC_HEIGHT: f32 = 1.0 / 4096.0;

/// The height an atomic inline placement reports: undoes the [`ZERO_ATOMIC_HEIGHT`] stand-in.
pub(crate) fn atomic_height(h: f32) -> f32 {
    if h <= ZERO_ATOMIC_HEIGHT * 1.5 {
        0.0
    } else {
        h
    }
}

/// Mutable copy of a shaped paragraph for one break.
pub struct ScratchLayout {
    pub(crate) layout: Layout<SpanBrush>,
    pub(crate) shaped: Arc<ShapedParagraph>,
    /// The unit table with this scratch layout's atomic widths.
    pub(crate) snap: Arc<SnapTable>,
    /// Per committed line: the line's real geometry (Parley only sees the max advance that
    /// makes it end the line where the whole-pixel policy decided).
    pub(crate) lines: Vec<CommittedLine>,
}

/// What the breaker knew about a committed line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CommittedLine {
    /// Available inline size (`LineParams::max_advance`; infinite for max-content).
    pub available: f32,
    /// Line box top (unrounded).
    pub top: f32,
    pub height: f32,
    /// The units (text clusters, atomics, edges) the line holds.
    pub units: std::ops::Range<usize>,
}

impl std::fmt::Debug for ScratchLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScratchLayout")
            .field("shaped", &self.shaped)
            .finish()
    }
}

/// `scratch(&ShapedParagraph)`.
pub fn scratch(shaped: &Arc<ShapedParagraph>) -> ScratchLayout {
    ScratchLayout {
        layout: shaped.layout.clone(),
        shaped: shaped.clone(),
        snap: shaped.snap.clone(),
        lines: Vec::new(),
    }
}

static NEXT_BREAKER: AtomicU64 = AtomicU64::new(1);

impl ScratchLayout {
    /// `text-wrap-style: balance` (015 TXT-4): the width of each line of the balanced breaking of this
    /// paragraph into `room` px (the first line `first_room`), or `None` when it is not balanced (see
    /// `SnapTable::balance_plan`). Call it after `set_atomics`: atomic inlines count with their sizes.
    pub fn balance_plan(&self, first_room: f32, room: f32) -> Option<Vec<f32>> {
        self.snap.balance_plan(first_room, room)
    }

    /// Give every atomic inline its margin-box size for this break (ifc-interface.md §5.2: the
    /// inline behaviour obtains them from `layout_atomic`). Atomics not listed keep size 0.
    pub fn set_atomics(&mut self, boxes: &[AtomicBox]) {
        for b in self.layout.inline_boxes_mut() {
            if let Some(a) = boxes.iter().find(|a| a.item == b.id) {
                b.width = a.width;
                b.height = if a.height == 0.0 {
                    ZERO_ATOMIC_HEIGHT
                } else {
                    a.height
                };
                b.baseline = a.baseline;
            }
        }
        self.snap = Arc::new(self.shaped.snap.with_atomic_widths(|item| {
            boxes
                .iter()
                .find(|a| a.item == item)
                .map(|a| a.width)
                .unwrap_or(0.0)
        }));
    }

    pub fn breaker(&mut self) -> Breaker<'_> {
        self.lines.clear();
        // Floats and out-of-flow placeholders are Parley `CustomOutOfFlow` boxes identified by
        // their owner tag; the breaker maps a yielded box back to its kind and to its position in
        // the unit table: after the text that starts before its byte position and after the
        // atomics and edge spacers that precede it in source order.
        let items = &self.shaped.source.items;
        let mut boxes = HashMap::new();
        let mut units_before = 0usize;
        for i in items {
            match i.kind {
                InlineItemKind::AtomicInline | InlineItemKind::EdgeSpacer { .. } => {
                    units_before += 1
                }
                InlineItemKind::Float | InlineItemKind::OutOfFlow => {
                    let text = self
                        .snap
                        .clusters
                        .iter()
                        .filter(|u| u.kind == UnitKind::Text && u.start < i.range.start)
                        .count();
                    boxes.insert(i.owner, (i.kind, text + units_before));
                }
                _ => {}
            }
        }
        Breaker {
            inner: self.layout.break_lines(),
            snap: self.snap.clone(),
            lines: &mut self.lines,
            cursor: 0,
            pending: None,
            boxes,
            start_x: 0.0,
            id: NEXT_BREAKER.fetch_add(1, Ordering::Relaxed),
            align_width: None,
            first_end: None,
            misuse: None,
        }
    }

    pub fn shaped(&self) -> &Arc<ShapedParagraph> {
        &self.shaped
    }
}

/// Revertible line breaker over a scratch layout. Never touches a frozen paragraph.
///
/// Line-fit decisions use the whole-pixel advances of the text metrics policy (`snap.rs`):
/// `set_line` chooses the line's end and gives Parley the max advance that ends it there.
pub struct Breaker<'a> {
    inner: BreakLines<'a, SpanBrush>,
    snap: Arc<SnapTable>,
    lines: &'a mut Vec<CommittedLine>,
    /// Unit index where the next line starts.
    cursor: usize,
    /// The current line's (available width, chosen end).
    pending: Option<(f32, Option<usize>)>,
    /// Owner tag -> (kind, unit index) of the float / out-of-flow boxes.
    boxes: HashMap<u64, (InlineItemKind, usize)>,
    /// `LineParams::start_x` of the current line.
    start_x: f32,
    /// The inline size alignment and justification of the following lines refer to, when it is not
    /// the one they are broken into (`text-wrap: balance`, 015 TXT-4).
    align_width: Option<f32>,
    /// The unit the first line must end before (a first line decided elsewhere, 015 TXT-5).
    first_end: Option<usize>,
    id: u64,
    /// First interface misuse (`INTERNAL_INVARIANT`); the breaker keeps working best-effort.
    misuse: Option<Status>,
}

impl std::fmt::Debug for Breaker<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Breaker")
    }
}

impl Breaker<'_> {
    fn note_misuse(&mut self, what: &str) {
        if self.misuse.is_none() {
            self.misuse = Some(
                Status::new(Code::InternalInvariant, Stage::Text)
                    .with_message(format!("breaker misuse: {what}")),
            );
        }
    }

    /// The first interface misuse seen (`INTERNAL_INVARIANT`, text-services.md section 3), if
    /// any. Misuse never panics; the offending call is ignored or answered best-effort.
    pub fn take_misuse(&mut self) -> Option<Status> {
        self.misuse.take()
    }

    /// The following lines are broken into their `max_advance`, but aligned (and justified) within
    /// `width`: balanced text keeps its container's alignment box (`None` restores the default).
    pub fn set_align_width(&mut self, width: Option<f32>) {
        self.align_width = width;
    }

    /// The boundary after the end of the line `end_unit` of another break of the same box subtree: (byte, non-text
    /// units before it), see `unit_position`.
    pub fn unit_position(&self, unit: usize) -> (usize, usize) {
        self.snap.unit_position(unit)
    }

    /// The first line must end at the boundary (byte offset, non-text units before it) another paragraph of the
    /// same box subtree ended its first line at (015 TXT-5, `::first-line`). `false` when this paragraph has no
    /// such boundary (the line breaks freely).
    pub fn set_first_line_end(&mut self, position: Option<(usize, usize)>) -> bool {
        self.first_end = position
            .and_then(|(byte, nontext)| self.snap.unit_at(byte, nontext))
            .filter(|&u| u > 0);
        position.is_none() || self.first_end.is_some()
    }

    pub fn set_line(&mut self, p: LineParams) {
        self.start_x = p.start_x;
        let available = if p.max_advance.is_finite() {
            p.max_advance.max(0.0)
        } else {
            f32::INFINITY
        };
        let forced = self.first_end.filter(|_| self.cursor == 0);
        let (adv, end) = if let Some(end) = forced {
            let (end, max) = self.snap.choose_to(self.cursor, end, available);
            (max, Some(end))
        } else if available.is_finite() {
            match self.snap.choose(self.cursor, available) {
                Some((end, max)) => (max, Some(end)),
                None => (available, None),
            }
        } else {
            (f32::MAX, None)
        };
        self.pending = Some((
            self.align_width
                .filter(|_| available.is_finite())
                .unwrap_or(available),
            end,
        ));
        let st = self.inner.state_mut();
        // Parley asserts the line advance does not exceed the layout advance.
        st.set_layout_max_advance(adv);
        st.set_line_max_advance(adv);
        st.set_line_x(p.start_x);
        st.set_line_y(p.y as f64);
        st.set_line_max_height(if p.max_height.is_finite() {
            p.max_height
        } else {
            f32::MAX
        });
    }

    pub fn checkpoint(&self) -> BreakerCheckpoint {
        BreakerCheckpoint {
            state: self.inner.state().clone(),
            cursor: self.cursor,
            lines: self.lines.len(),
            breaker: self.id,
        }
    }

    pub fn revert(&mut self, cp: BreakerCheckpoint) {
        if cp.breaker != self.id {
            // A checkpoint of another breaker would corrupt the line state.
            self.note_misuse("checkpoint belongs to another breaker");
            return;
        }
        if cp.lines > self.lines.len() {
            self.note_misuse("checkpoint is ahead of the breaker");
            return;
        }
        self.inner.revert_to(cp.state);
        self.cursor = cp.cursor;
        self.lines.truncate(cp.lines);
    }

    /// Break the next line; `None` when all text is placed.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<BreakEvent> {
        if self.pending.is_none() {
            // `set_line` precedes every line (section 6 rule 1); a float or out-of-flow event
            // continues the same line and keeps its parameters.
            self.note_misuse("next() without set_line()");
        }
        match self.inner.break_next()? {
            YieldData::LineBreak(d) => {
                let (available, end) = self.pending.take().unwrap_or((f32::INFINITY, None));
                let start = self.cursor;
                let hint =
                    end.unwrap_or_else(|| self.snap.opportunities.last().copied().unwrap_or(0));
                self.cursor = self.snap.resync(start, d.metrics.advance, hint);
                let units = start..self.cursor;
                let w = self.snap.widths(start..self.cursor);
                // A line that ends at a soft hyphen or a hyphenation point also holds the hyphen.
                let hyphen = if d.reason == BreakReason::Regular {
                    self.snap.hyphen_at(self.cursor).map_or(0.0, |h| h.width)
                } else {
                    0.0
                };
                // Parley's quantized line top is rounded; the line box keeps the unrounded top
                // it was given, with the baseline at its (whole-pixel) offset from the top.
                let top = d.line_y_start as f32;
                let height = (d.line_y_end - d.line_y_start) as f32;
                let baseline = top + (d.metrics.baseline - d.metrics.block_min_coord);
                self.lines.push(CommittedLine {
                    available,
                    top,
                    height,
                    units,
                });
                Some(BreakEvent::LineDone {
                    metrics: LineDoneMetrics {
                        top,
                        height,
                        baseline,
                        advance: w.snapped_full + hyphen,
                        hanging_advance: w.snapped_full - w.snapped,
                        end_unit: self.cursor,
                    },
                    reason: line_end(d.reason),
                })
            }
            YieldData::MaxHeightExceeded(d) => Some(BreakEvent::MaxHeightExceeded {
                advance: d.advance,
                line_height: d.line_height,
            }),
            YieldData::InlineBoxBreak(d) => {
                // Zero-size placeholder: append it so iteration makes progress.
                self.inner
                    .state_mut()
                    .append_inline_box_to_line(d.advance, 0.0, 0.0);
                // Inline position on the current line with the whole-pixel advances of the text
                // metrics policy (not Parley's fractional x); `at` is a unit index.
                let (kind, at) = self
                    .boxes
                    .get(&d.inline_box_id)
                    .copied()
                    .unwrap_or((InlineItemKind::OutOfFlow, self.cursor));
                let advance = self.start_x
                    + self
                        .snap
                        .widths(self.cursor..at.max(self.cursor))
                        .snapped_full;
                let item = d.inline_box_id;
                Some(match kind {
                    InlineItemKind::Float => BreakEvent::FloatEncountered { item, advance },
                    _ => BreakEvent::OutOfFlowEncountered { item, advance },
                })
            }
        }
    }

    /// The unit index at which the float or out-of-flow box `item` sits in the text.
    pub fn item_unit(&self, item: u64) -> Option<usize> {
        self.boxes.get(&item).map(|(_, at)| *at)
    }

    pub fn finish(self) {
        self.inner.finish();
    }
}
