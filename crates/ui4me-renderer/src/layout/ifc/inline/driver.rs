//! owner: 009-text-inline (R-36; IF-46) — created by 001 slice (R-28)
//!
//! The line loop (`InlineDriver`; 008 `ifc-interface.md` §5.2 `run`, §6 rules 1, 2, 4, 6):
//! lines are produced top-down; each asks the host for a line slot and breaks one line in it.
//! A line taller than the slot's valid height is retried after reverting the breaker (bounded;
//! without floats the slot height is unbounded, so no retry happens in the slice).
//!
//! Atomic inlines: before the first line, the driver lays every atomic out through
//! `InlineLayoutHost::layout_atomic` (`Intrinsic` for `intrinsic`, `Measure` / `Final` for `run`)
//! and hands the margin-box sizes to the scratch layout; the final outcome lists where each
//! atomic landed so that 008 can place it (CSS 2.1 §10.8.1 baseline alignment is the line
//! builder's: the box's baseline sits on the line's baseline, `vertical-align` applies).

use crate::text::paragraph::{
    AtomicBox, AtomicContribution, BreakEvent, FreezeExtras, FreezeParams, FrozenParagraph,
    InlineItemKind, LineEnd, LineParams, ScratchLayout, ShapedParagraph, SynthRun, TextAlignment,
    scratch,
};
use std::sync::Arc;

use crate::layout::box_tree::Direction;
use crate::layout::ifc::inline::line_box::line_boxes;
use crate::layout::ifc::interface::{
    AtomicMode, AtomicPlacement, AtomicResult, AvailableInline, EdgeSpacerPlacement, FinalExtras,
    FinalOutcome, InlineLayoutHost, IntrinsicKind, IntrinsicOutcome, MeasureOutcome,
};

/// Bound on per-line retries (§6.2: float edges in range + 1; one without floats).
const MAX_RETRIES: usize = 8;

/// The atomic inlines of the paragraph, in text order.
fn atomic_items(shaped: &ShapedParagraph) -> Vec<u64> {
    shaped
        .source()
        .items
        .iter()
        .filter(|i| i.kind == InlineItemKind::AtomicInline)
        .map(|i| i.owner)
        .collect()
}

/// `intrinsic(&shaped, host)` (§5.2): the atomics' margin-box contributions through
/// `layout_atomic(item, Intrinsic(..))`, combined with the text by `content_widths`. No float
/// placement happens.
pub fn intrinsic(shaped: &ShapedParagraph, host: &mut dyn InlineLayoutHost) -> IntrinsicOutcome {
    let items = atomic_items(shaped);
    let contributions: Vec<AtomicContribution> = items
        .iter()
        .map(
            |&item| match host.layout_atomic(item, AtomicMode::Intrinsic(IntrinsicKind::Max)) {
                AtomicResult::Contribution(c) => c.tagged(item),
                AtomicResult::Metrics(m) => AtomicContribution {
                    item,
                    min_content: m.margin_width,
                    max_content: m.margin_width,
                },
            },
        )
        .collect();
    let w = shaped.content_widths_with(&contributions);
    // The first line's indent is part of the max-content size (CSS Text 3 section 9).
    IntrinsicOutcome {
        min_content: w.min,
        max_content: w.max + host.text_indent().max(0.0),
    }
}

enum Done {
    Measure(MeasureOutcome),
    Final(FinalOutcome),
    /// The end of the first line (byte, non-text units before it), `None` for a paragraph without a line.
    First(Option<(usize, usize)>),
}

/// How the first line of the paragraph is treated (015 TXT-5, `::first-line`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstLineMode {
    /// Free breaking.
    None,
    /// Break the first line only and report where it ends (the paragraph shaped with the first-line style).
    Probe,
    /// The first line ends at this boundary (what the probe found), whatever the room.
    Forced((usize, usize)),
}

/// `freeze` with the `text-overflow` end run when there is one, and the legacy line clamp (the lines
/// shown and the ellipsis that ends the last). The extras name no line, piece or decoration, so
/// `freeze_with` cannot refuse them.
fn freeze(
    sc: ScratchLayout,
    alignment: TextAlignment,
    align_last: Option<TextAlignment>,
    end_ellipsis: Option<&SynthRun>,
    clamp: Option<(usize, &SynthRun)>,
) -> Arc<FrozenParagraph> {
    let extras = FreezeExtras {
        align_last,
        end_ellipsis: end_ellipsis.cloned(),
        clamp: clamp.map(|(n, e)| (n, e.clone())),
        ..FreezeExtras::default()
    };
    match sc.freeze_with(FreezeParams { alignment }, extras) {
        Ok(p) => p,
        Err(_) => unreachable!("freeze extras without per-line data are always valid"),
    }
}

/// The slot of the line at `y`; the first line of the paragraph starts `indent` in from the start
/// edge (`text-indent`, CSS Text 3 section 9) and has that much less room. In a right-to-left
/// paragraph the start edge is the right one: the line keeps its left edge and loses the room.
fn slot_for(
    host: &mut dyn InlineLayoutHost,
    y: f32,
    height: f32,
    first: bool,
    indent: f32,
) -> crate::layout::ifc::interface::LineSlot {
    let mut s = host.line_slot(y, height, 0.0);
    if first && indent != 0.0 {
        if host.constraints().direction == Direction::Ltr {
            s.inline_start += indent;
        }
        s.inline_size = (s.inline_size - indent).max(0.0);
    }
    s
}

fn run_inner(
    shaped: &Arc<ShapedParagraph>,
    host: &mut dyn InlineLayoutHost,
    alignment: Option<TextAlignment>,
    end_ellipsis: Option<&SynthRun>,
    clamp_ellipsis: Option<&SynthRun>,
    first_mode: FirstLineMode,
) -> Done {
    // Section 6 rule 6: a paragraph with no text and no item makes no line box (Parley would
    // lay out one empty line, which has strut height).
    let empty = shaped.source().text.is_empty() && shaped.source().items.is_empty();
    let width = match host.constraints().available {
        AvailableInline::Definite(w) => w.max(0.0),
        AvailableInline::MaxContent => f32::INFINITY,
        AvailableInline::MinContent => intrinsic(shaped, host).min_content,
    };
    let indent = host.text_indent();
    let align_last = host.align_last();
    // The room of a line of the paragraph: the first one is `indent` narrower.
    let line_room = |first: bool| {
        if !width.is_finite() {
            f32::INFINITY
        } else if first {
            (width - indent).max(0.0)
        } else {
            width
        }
    };
    let mut sc = scratch(shaped);
    let items = atomic_items(shaped);
    if !items.is_empty() {
        let mode = if alignment.is_some() {
            AtomicMode::Final { available: width }
        } else {
            AtomicMode::Measure { available: width }
        };
        let boxes: Vec<AtomicBox> = items
            .iter()
            .map(|&item| match host.layout_atomic(item, mode) {
                AtomicResult::Metrics(m) => AtomicBox {
                    item,
                    width: m.margin_width,
                    height: m.margin_height,
                    baseline: m.baseline,
                },
                AtomicResult::Contribution(c) => AtomicBox {
                    item,
                    width: c.max_content,
                    height: 0.0,
                    baseline: None,
                },
            })
            .collect();
        sc.set_atomics(&boxes);
    }
    // `text-wrap-style: balance`: each line is broken into its planned width and aligned within the full
    // width. A paragraph with floats or out-of-flow boxes is left alone, as the reference leaves it.
    // (A line clamp wins: the reference leaves the lines shown as they break, WPT text-wrap-balance-line-clamp-001.)
    let plan: Option<Vec<f32>> = if host.balance_lines()
        && host.line_clamp().is_none()
        && width.is_finite()
        && !shaped
            .source()
            .items
            .iter()
            .any(|i| matches!(i.kind, InlineItemKind::Float | InlineItemKind::OutOfFlow))
    {
        sc.balance_plan(line_room(true), line_room(false))
    } else {
        None
    };
    let balanced = plan.as_ref();
    let break_room = |slot_w: f32, line: usize| {
        balanced
            .and_then(|p| p.get(line))
            .map_or(slot_w, |w| slot_w.min(*w))
    };
    // A legacy line clamp: the block ends with the line `clamp_lines`; the lines after it stay laid out.
    let clamp_lines = host.line_clamp();
    let (mut clamp_y, mut clamp_baseline): (Option<f32>, Option<f32>) = (None, None);
    let (mut y, mut lines, mut first, mut last) = (0.0f32, 0usize, None, None);
    // A paragraph that ends on a forced break; atomic inlines and inline-box edges after the break
    // are content of the line it starts (`<br><span style=display:inline-block>`, WPT-D49).
    let text_end = shaped.source().text.len();
    let content_after_break = shaped.source().items.iter().any(|i| {
        matches!(
            i.kind,
            InlineItemKind::AtomicInline | InlineItemKind::EdgeSpacer { .. }
        ) && i.range.start >= text_end
    });
    let forced_end = shaped.source().text.ends_with('\n') && !content_after_break;
    let forced_breaks = shaped.source().text.matches('\n').count();
    // The owners of the forced breaks in text order (015 STAB-1: `<br clear>` clears the line after its break).
    let break_owners: Vec<u64> = {
        let src = shaped.source();
        src.text
            .match_indices('\n')
            .map(|(i, _)| {
                src.spans
                    .iter()
                    .find(|sp| sp.range.contains(&i))
                    .map_or(0, |sp| sp.owner)
            })
            .collect()
    };
    let mut forced_done = 0usize;
    // Floats that did not fit beside the content of their line: placed at the top of the next
    // line, after the current line is committed (ifc-interface.md section 6 rule 3).
    let mut queued: Vec<u64> = Vec::new();
    // Floats already placed (a reverted line meets them again; placement is irreversible).
    let mut placed: Vec<u64> = Vec::new();
    // Out-of-flow placeholders met on the current line: (item, inline position). Their static
    // position is known when the line is (a block-level one sits below the content before it).
    let mut pending_oof: Vec<(u64, f32)> = Vec::new();
    let mut statics: Vec<(u64, f32, f32)> = Vec::new();
    // Out-of-flow boxes after the forced break that ends the paragraph.
    let mut after_forced_end: Vec<u64> = Vec::new();
    let resolve_statics = |host: &mut dyn InlineLayoutHost,
                           pending: &mut Vec<(u64, f32)>,
                           statics: &mut Vec<(u64, f32, f32)>,
                           top: f32,
                           height: f32,
                           start: f32| {
        for (item, adv) in pending.drain(..) {
            if host.oof_is_inline_level(item) {
                host.record_static_position(item, adv, top);
                statics.push((item, adv, top));
            } else {
                // A block-level box would end the line it follows content on: its static position
                // is the start of the next line; at the start of a line it is that line's top.
                let y = if adv - start > 0.5 { top + height } else { top };
                host.record_static_position(item, start, y);
                statics.push((item, f32::NAN, y));
            }
        }
    };
    let mut probe_end: Option<(usize, usize)> = None;
    {
        let mut br = sc.breaker();
        if let FirstLineMode::Forced(p) = first_mode {
            br.set_first_line_end(Some(p));
        }
        'lines: loop {
            if probe_end.is_some() {
                break 'lines;
            }
            for item in queued.drain(..) {
                let _ = host.measure_float(item);
                host.place_float(item, y);
                placed.push(item);
            }
            // A forced break at the very end of the paragraph ends its line; it does not start another
            // (CSS Text 3 §4.1.3). Only a paragraph that is nothing but breaks gets here, the
            // inline builder drops the final break of any other.
            if forced_end && lines >= forced_breaks {
                // What follows the last break is the start of a line that is never made: the static
                // position of an out-of-flow box there is that line's start.
                let slot = slot_for(host, y, 0.0, false, 0.0);
                after_forced_end.extend(
                    shaped
                        .source()
                        .items
                        .iter()
                        .filter(|i| {
                            i.kind == InlineItemKind::OutOfFlow
                                && !statics.iter().any(|s| s.0 == i.owner)
                        })
                        .map(|i| i.owner),
                );
                for item in after_forced_end.drain(..) {
                    host.record_static_position(item, slot.inline_start, y);
                    statics.push((item, slot.inline_start, y));
                }
                break 'lines;
            }
            let cp = br.checkpoint();
            let mut provisional = 0.0f32;
            let mut retries = 0;
            // After MAX_RETRIES the line is laid out with no bound on its height (the fallback below used to be overridden by
            // the slot's own height at the top of the loop, so a line taller than its band retried forever).
            let mut unbounded = false;
            let mut shifts = 0;
            loop {
                let first_line = lines == 0;
                let slot = slot_for(host, y, provisional, first_line, indent);
                let slot_w = if width.is_finite() {
                    slot.inline_size.min(line_room(first_line))
                } else {
                    f32::INFINITY
                };
                br.set_align_width(balanced.map(|_| slot_w));
                br.set_line(LineParams {
                    start_x: slot.inline_start,
                    max_advance: break_room(slot_w, lines),
                    y,
                    max_height: if unbounded {
                        f32::INFINITY
                    } else {
                        slot.valid_height
                    },
                });
                match br.next() {
                    None => {
                        resolve_statics(
                            host,
                            &mut pending_oof,
                            &mut statics,
                            y,
                            0.0,
                            slot.inline_start,
                        );
                        break 'lines;
                    }
                    Some(BreakEvent::LineDone { metrics, reason }) => {
                        // A line whose content does not fit the room beside floats moves down to the
                        // next float edge, until it fits or no float is left (CSS 2.1 section 9.5).
                        let narrowed = width.is_finite() && slot_w < line_room(first_line) - 1e-3;
                        if narrowed
                            && metrics.advance - metrics.hanging_advance > slot_w + 1e-3
                            && shifts < 256
                            && let Some(next) = host.next_float_edge(y).filter(|n| *n > y + 1e-4)
                        {
                            shifts += 1;
                            br.revert(cp.clone());
                            pending_oof.clear();
                            y = next;
                            provisional = 0.0;
                            continue;
                        }
                        // A float the breaker met while looking for this line's end, but that lies after
                        // the end, belongs to a later line: that line meets it again (CSS 2.1 section 9.5
                        // rule 6 only holds a float below the lines that precede it in the text).
                        queued.retain(|f| br.item_unit(*f).is_none_or(|at| at <= metrics.end_unit));
                        // The same for an out-of-flow box met while looking for the end of the line:
                        // one after a forced break is the first thing of the next line (its static
                        // position is there, CSS 2.1 section 10.3.7).
                        let forced = reason == LineEnd::Forced;
                        let (here, later): (Vec<_>, Vec<_>) = std::mem::take(&mut pending_oof)
                            .into_iter()
                            .partition(|(o, _)| {
                                br.item_unit(*o).is_none_or(|at| {
                                    if forced {
                                        at < metrics.end_unit
                                    } else {
                                        at <= metrics.end_unit
                                    }
                                })
                            });
                        pending_oof = here;
                        // Kept for a paragraph that ends on this break: no later line meets them.
                        after_forced_end = later.into_iter().map(|(o, _)| o).collect();
                        resolve_statics(
                            host,
                            &mut pending_oof,
                            &mut statics,
                            metrics.top,
                            metrics.height,
                            slot.inline_start,
                        );
                        first.get_or_insert(metrics.baseline);
                        last = Some(metrics.baseline);
                        y = metrics.top + metrics.height;
                        if forced {
                            if let Some(&owner) = break_owners.get(forced_done) {
                                let (l, r) = host.break_clear(owner);
                                if l || r {
                                    y = host.clear_floats(y, l, r);
                                }
                            }
                            forced_done += 1;
                        }
                        lines += 1;
                        if first_mode == FirstLineMode::Probe {
                            probe_end = Some(br.unit_position(metrics.end_unit));
                        }
                        if clamp_lines == Some(lines) {
                            clamp_y = Some(y);
                            clamp_baseline = Some(metrics.baseline);
                        }
                        break;
                    }
                    Some(BreakEvent::MaxHeightExceeded { line_height, .. }) => {
                        retries += 1;
                        provisional = line_height;
                        br.revert(cp.clone());
                        pending_oof.clear();
                        if retries >= MAX_RETRIES {
                            // LAYOUT_NON_CONVERGENCE is reported by the 008 host when floats
                            // exist; keep the last attempt with an unbounded slot height.
                            unbounded = true;
                            br.set_line(LineParams {
                                start_x: slot.inline_start,
                                max_advance: break_room(slot_w, lines),
                                y,
                                max_height: f32::INFINITY,
                            });
                        }
                    }
                    Some(BreakEvent::OutOfFlowEncountered { item, advance }) => {
                        pending_oof.push((item, advance));
                    }
                    Some(BreakEvent::FloatEncountered { item, advance }) => {
                        // Section 6 rule 3: place it now when its margin box fits beside the
                        // content already on the line (or the line has none), else queue it for
                        // the next line.
                        if queued.contains(&item) {
                            continue;
                        }
                        let size = host.measure_float(item);
                        let on_line = advance - slot.inline_start;
                        if placed.contains(&item)
                            || on_line <= 0.5
                            || on_line + size.width <= slot_w + 1e-3
                        {
                            if !placed.contains(&item) {
                                host.place_float(item, y);
                                placed.push(item);
                                if on_line > 0.5 {
                                    // The content before the float is on the line already: the line is
                                    // broken afresh in the room the float left (the float sits at the
                                    // line's top and the text flows around it, CSS 2.1 section 9.5).
                                    br.revert(cp.clone());
                                    pending_oof.clear();
                                    continue;
                                }
                            }
                            // The line continues in the band the float left (same line, same
                            // checkpoint: the retry bound still counts per line).
                            let slot = slot_for(host, y, provisional, first_line, indent);
                            let slot_w = if width.is_finite() {
                                slot.inline_size.min(line_room(first_line))
                            } else {
                                f32::INFINITY
                            };
                            br.set_line(LineParams {
                                start_x: slot.inline_start,
                                max_advance: break_room(slot_w, lines),
                                y,
                                max_height: if unbounded {
                                    f32::INFINITY
                                } else {
                                    slot.valid_height
                                },
                            });
                        } else {
                            queued.push(item);
                        }
                    }
                }
            }
        }
        br.finish();
    }
    // Floats queued by the last line sit below it.
    for item in queued.drain(..) {
        let _ = host.measure_float(item);
        host.place_float(item, y);
    }
    if first_mode == FirstLineMode::Probe {
        return Done::First(probe_end);
    }
    if empty {
        return match alignment {
            None => Done::Measure(MeasureOutcome {
                block_size: 0.0,
                first_baseline: None,
                last_baseline: None,
                line_count: 0,
            }),
            Some(a) => {
                let paragraph = freeze(sc, a, align_last, end_ellipsis, None);
                Done::Final(FinalOutcome {
                    paragraph,
                    block_size: 0.0,
                    first_baseline: None,
                    last_baseline: None,
                    lines: Vec::new(),
                    extras: FinalExtras::default(),
                })
            }
        };
    }
    // The clamp cuts the block after its `n`th line when there are more.
    let clamped = clamp_lines.filter(|n| lines > *n);
    match alignment {
        None => match (clamped, clamp_y) {
            (Some(_), Some(cy)) => Done::Measure(MeasureOutcome {
                block_size: cy,
                first_baseline: first,
                last_baseline: clamp_baseline.or(last),
                line_count: lines,
            }),
            _ => Done::Measure(MeasureOutcome {
                block_size: y,
                first_baseline: first,
                last_baseline: last,
                line_count: lines,
            }),
        },
        Some(a) => {
            let paragraph = freeze(sc, a, align_last, end_ellipsis, clamped.zip(clamp_ellipsis));
            let lb = line_boxes(&paragraph);
            let mut extras = FinalExtras {
                oof_static: statics,
                ..FinalExtras::default()
            };
            for li in 0..paragraph.line_count() {
                for a in paragraph.atomic_placements(li) {
                    extras.atomics.push(AtomicPlacement {
                        item: a.item,
                        line_index: li,
                        margin_box_origin: (a.x, a.y),
                    });
                }
                for e in paragraph.edge_placements(li) {
                    extras.edges.push(EdgeSpacerPlacement {
                        owner: e.owner,
                        start: e.start,
                        line_index: li,
                        x: e.x,
                        width: e.width,
                    });
                }
            }
            Done::Final(FinalOutcome {
                // The bottom of the last line (lines moved down beside floats leave a gap above them); of the last
                // line shown under a line clamp.
                block_size: clamped
                    .and_then(|n| paragraph.lines().get(n - 1))
                    .or(paragraph.lines().last())
                    .map_or(0.0, |l| l.top + l.height),
                first_baseline: paragraph.first_baseline(),
                last_baseline: clamped
                    .and_then(|n| paragraph.lines().get(n - 1))
                    .map_or_else(|| paragraph.last_baseline(), |l| Some(l.baseline)),
                lines: lb,
                extras,
                paragraph,
            })
        }
    }
}

/// `run(&shaped, host, Mode::Measure)`: the scratch layout is dropped.
pub fn measure(shaped: &Arc<ShapedParagraph>, host: &mut dyn InlineLayoutHost) -> MeasureOutcome {
    measure_with(shaped, host, FirstLineMode::None)
}

/// [`measure`] with the first line decided by `first` (015 TXT-5).
pub fn measure_with(
    shaped: &Arc<ShapedParagraph>,
    host: &mut dyn InlineLayoutHost,
    first: FirstLineMode,
) -> MeasureOutcome {
    match run_inner(shaped, host, None, None, None, first) {
        Done::Measure(m) => m,
        Done::Final(_) | Done::First(_) => unreachable!("measure never freezes"),
    }
}

/// Where the first line of `shaped` ends in `host`'s room (byte offset and the number of edge spacers and atomic
/// inlines before it), `None` for a paragraph with no line (015 TXT-5). Floats the line meets are placed in `host`.
pub fn first_line_end(
    shaped: &Arc<ShapedParagraph>,
    host: &mut dyn InlineLayoutHost,
) -> Option<(usize, usize)> {
    match run_inner(shaped, host, None, None, None, FirstLineMode::Probe) {
        Done::First(p) => p,
        _ => None,
    }
}

/// `run(&shaped, host, Mode::Final)`: breaks, aligns and freezes.
pub fn final_layout(
    shaped: &Arc<ShapedParagraph>,
    host: &mut dyn InlineLayoutHost,
    alignment: TextAlignment,
    end_ellipsis: Option<&SynthRun>,
    clamp_ellipsis: Option<&SynthRun>,
) -> FinalOutcome {
    final_layout_with(
        shaped,
        host,
        alignment,
        end_ellipsis,
        clamp_ellipsis,
        FirstLineMode::None,
    )
}

/// [`final_layout`] with the first line decided by `first` (015 TXT-5).
pub fn final_layout_with(
    shaped: &Arc<ShapedParagraph>,
    host: &mut dyn InlineLayoutHost,
    alignment: TextAlignment,
    end_ellipsis: Option<&SynthRun>,
    clamp_ellipsis: Option<&SynthRun>,
    first: FirstLineMode,
) -> FinalOutcome {
    match run_inner(
        shaped,
        host,
        Some(alignment),
        end_ellipsis,
        clamp_ellipsis,
        first,
    ) {
        Done::Final(f) => f,
        Done::Measure(_) | Done::First(_) => unreachable!("final always freezes"),
    }
}
