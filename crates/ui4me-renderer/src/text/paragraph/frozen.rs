//! owner: 009-text-inline — created by 001 slice (R-28)
//!
//! Freezing and frozen paragraphs (008 `ifc-interface.md` §5.1 `freeze`, §8; 009
//! `text-services.md` §5–§6). `freeze` applies alignment and makes the layout immutable; every
//! geometry and glyph-run query reads only a `FrozenParagraph`. All values are CSS px, f32, in
//! the inline root's content-box space; 008 quantises published edges.

use parley::{Alignment, AlignmentOptions, Layout, PositionedLayoutItem};
use std::ops::Range;
use std::sync::Arc;
use ui4me_assets::fonts::FontBlob;

use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

use super::decoration::DecoFont;
use super::scratch::{LineEnd, ScratchLayout};
use super::snap::{AdvancePolicy, FaceKey, SnapTable, UnitKind};
use super::source::{DecorationLineStyle, MissingGlyph, ParagraphSource, SpanBrush, SynthRun};

/// Resolved `text-align` (physical; 008 maps `start`/`end` through `direction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlignment {
    Left,
    Right,
    Center,
    /// `text-align: start` / `end` where each line has its own direction (`unicode-bidi:
    /// plaintext` and `dir=auto` roots with several forced-break lines): resolved per line.
    Start,
    End,
    /// `justify` (CSS Text 3 section 7): the word spaces of a line stretch so that the line fills its
    /// available width (`inter-word`); lines the alignment of the last line applies to are aligned
    /// by `text-align-last` instead.
    Justify,
}

/// `FreezeParams` (slice subset: alignment only; no ellipsis, hyphens, pieces or decorations).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreezeParams {
    pub alignment: TextAlignment,
}

/// What the inline behaviour computed for one line and `freeze` embeds (ifc-interface.md 5.1
/// `FreezeParams.lines`): freeze computes none of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineExtras {
    pub ellipsis: Option<SynthRun>,
    pub hyphen: Option<SynthRun>,
    /// Byte range of the processed text hidden on this line by `text-overflow`.
    pub hidden_range: Option<Range<usize>>,
    /// The line lies past a line clamp: it keeps its geometry (the reference keeps laying it out) but is not painted.
    pub clamped_out: bool,
}

/// One inline box fragment piece: one per line and visual run (G-TEXT-EDGES; section 7
/// `inline_box_pieces`). `x`/`width` span the content area on the line, in paragraph space.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineBoxPiece {
    pub owner: u64,
    pub line_index: usize,
    pub x: f32,
    pub width: f32,
    pub has_start_edge: bool,
    pub has_end_edge: bool,
    pub bidi_level: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationKind {
    Underline,
    Overline,
    LineThrough,
}

/// One text-decoration segment on one line (section 7 `decorations`; FR-044): one per
/// (decorating box, kind, line, contiguous extent). The decorating box is `owner`; the geometry
/// comes from the decorating box's first available font (CSS Text Decoration 3 section 2.2) and is
/// in whole pixels; the style is the decorating box's (its colour is resolved at paint time).
#[derive(Clone, Debug, PartialEq)]
pub struct DecorationSegment {
    pub owner: u64,
    pub line_index: usize,
    pub kind: DecorationKind,
    pub style: DecorationLineStyle,
    /// Baseline y (paragraph space) of the text the segment is under.
    pub baseline: f32,
    /// Top edge of the line relative to `baseline` (down is positive).
    pub offset: f32,
    pub thickness: f32,
    /// Inline extent in paragraph space (start, end).
    pub inline_range: (f32, f32),
}

/// The part of `FreezeParams` beyond alignment (ifc-interface.md 5.1): values computed by the
/// inline behaviour (`overflow.rs`, `edges.rs`, `decorations.rs`) that `freeze` only embeds.
/// Kept separate from `FreezeParams` so the slice's struct literal stays source-compatible
/// (decision D-L009r1-03).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FreezeExtras {
    /// `text-align-last` (the alignment of the last line and of lines ending in a forced
    /// break); `None` = same as `FreezeParams::alignment`.
    pub align_last: Option<TextAlignment>,
    /// Per line, in line order (shorter than the line count = no extras for the rest).
    pub lines: Vec<LineExtras>,
    pub pieces: Vec<InlineBoxPiece>,
    pub decorations: Vec<DecorationSegment>,
    /// `text-overflow` at the line-end edge: the synthesized run (an ellipsis or a string) to put
    /// where a line that overflows its available inline size is cut. `freeze` hides whole
    /// clusters of such a line until the run fits (CSS UI 4 section 5.1) and records the hidden
    /// range and the run in the line's [`LineExtras`]; a line whose hidden part holds an atomic
    /// inline is left alone and listed by [`FrozenParagraph::overflow_blocked_lines`].
    pub end_ellipsis: Option<SynthRun>,
    /// A legacy line clamp (`-webkit-line-clamp`, 015 TXT-4): the number of lines shown and the
    /// ellipsis that ends the last of them. The lines after it stay laid out but are not painted;
    /// the last one is cut after the clusters that leave room for the ellipsis and shows it, also when
    /// nothing needs cutting.
    pub clamp: Option<(usize, SynthRun)>,
}

/// One line of a frozen paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenLine {
    /// Line box top and height (`line-height`, CSS 2.1 §10.8).
    pub top: f32,
    pub height: f32,
    /// Baseline y (same space as `top`).
    pub baseline: f32,
    /// Line box inline start and size (the available width the line was broken into).
    pub inline_start: f32,
    pub inline_size: f32,
    /// Alignment offset of the content from `inline_start`.
    pub offset: f32,
    /// Content advance excluding hanging trailing white space.
    pub advance: f32,
    /// Justification: the extra advance of each word space of the line (0 when the line is not
    /// justified) and the byte from which trailing spaces hang and take none.
    pub justify: f32,
    pub justify_end: usize,
    pub text_range: Range<usize>,
    pub end: LineEnd,
    /// The line's units (text clusters, atomic inlines, edge spacers) in the paragraph's unit
    /// sequence.
    pub units: Range<usize>,
    /// The direction of the line's bidi paragraph is right-to-left.
    pub rtl: bool,
}

/// An atomic inline placed on a line: the margin box in the paragraph's space (x right, y down),
/// positioned with the whole-pixel advances like the text around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicPlacement {
    pub item: u64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The start or end edge spacer of an inline box on a line (G-TEXT-EDGES): the spacer occupies
/// `x..x + width` of the line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgePlacement {
    pub owner: u64,
    pub start: bool,
    pub x: f32,
    pub width: f32,
}

/// A contiguous piece of one span's text on one line (one per owner run; 008 makes a text
/// fragment from each).
#[derive(Clone, Debug, PartialEq)]
pub struct TextPiece {
    pub span: u32,
    pub owner: u64,
    pub x: f32,
    pub width: f32,
    pub text_range: Range<usize>,
    /// 1 for a right-to-left run, else 0 (the parity of the resolved bidi level).
    pub bidi_level: u8,
    /// Baseline y (paragraph space) and the font's whole-pixel ascent and descent: the text's
    /// content area is `baseline − ascent .. baseline + descent` (CSS Inline 3 §5; CSS 2.1
    /// §10.6.1; whole pixels per the text metrics policy, `snap.rs`).
    pub baseline: f32,
    pub ascent: f32,
    pub descent: f32,
    /// The hyphen at the end of a hyphenated line (015 TXT-3): no text (an empty `text_range` at the end of
    /// the line's text), a fragment of its own, listed apart by `getClientRects()` as in the reference browser.
    pub hyphen: bool,
}

/// One caret position of a line: a UTF-8 byte offset of the paragraph text and the x of the
/// insertion point there (paragraph space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretStop {
    pub byte: usize,
    pub x: f32,
}

/// The caret stops of one line (see [`FrozenParagraph::caret_stops`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaretStops {
    pub stops: Vec<CaretStop>,
    /// The line has a right-to-left run: the stops are in logical, not visual, order.
    pub bidi: bool,
}

/// The horizontal extent of one grapheme cluster on a line, in paragraph space, from the glyph
/// positions of the line's visual order (bidi lines; `FrozenParagraph::cluster_extents`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterExtent {
    pub start: usize,
    pub end: usize,
    pub x0: f32,
    pub x1: f32,
    pub rtl: bool,
}

impl ClusterExtent {
    /// The edge a caret before the cluster sits at (the right edge of a right-to-left cluster).
    pub fn lead(&self) -> f32 {
        if self.rtl { self.x1 } else { self.x0 }
    }

    /// The edge a caret after the cluster sits at.
    pub fn trail(&self) -> f32 {
        if self.rtl { self.x0 } else { self.x1 }
    }
}

/// The only font input painting needs (009 `text-services.md` §5; R-55).
#[derive(Clone, Debug)]
pub struct FontRef {
    /// 007's shared bytes; `blob.id.0` is the font identity.
    pub blob: FontBlob,
    /// Face index in the file.
    pub index: u32,
    /// Normalised variation coordinates (F2Dot14); empty for static faces.
    pub coords: Vec<i16>,
    pub embolden: bool,
    pub oblique_angle_deg: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphView {
    pub id: u32,
    /// Pen position in the paragraph's space (x right, y = baseline, down).
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    /// An upright glyph of a vertical line (015 WM-1b): `x` and `y` are its drawing origin in paragraph
    /// space (the scene maps it without turning the glyph); otherwise the glyph is drawn sideways.
    pub upright: bool,
    /// Horizontal compression of the glyph (`text-combine-upright` wider than one em); 1 otherwise.
    pub scale_x: f32,
}

#[derive(Clone, Debug)]
pub struct GlyphRunView {
    /// Run pen and baseline in paragraph space, before per-glyph shaping offsets.
    pub origin: (f32, f32),
    pub font: FontRef,
    pub size_px: f32,
    pub span: u32,
    pub owner: u64,
    pub glyphs: Vec<GlyphView>,
    /// 0 = LTR, 1 = RTL (the slice has no bidi; X-BIDI).
    pub bidi_level: u8,
}

/// Immutable, line-broken, aligned paragraph (`Arc`).
pub struct FrozenParagraph {
    layout: Layout<SpanBrush>,
    source: Arc<ParagraphSource>,
    lines: Vec<FrozenLine>,
    missing: Vec<MissingGlyph>,
    blobs: Vec<FontBlob>,
    policy: Arc<AdvancePolicy>,
    extras: FreezeExtras,
    snap: Arc<SnapTable>,
    deco_fonts: Arc<Vec<DecoFont>>,
    overflow_blocked: Vec<usize>,
    /// The paragraph has right-to-left text, a right-to-left base direction or UBA controls: its
    /// lines are placed in bidi visual order (`placed_items_bidi`).
    bidi: bool,
    /// Per line, `text_pieces` and `glyph_runs` once computed (L-PERF-b3): the paragraph is immutable
    /// and both are pure functions of it, read again by every layout pass that reuses the paragraph
    /// (scrollable overflow, fragments) and by every scene build.
    pieces_memo: Box<[std::sync::OnceLock<Vec<TextPiece>>]>,
    runs_memo: Box<[std::sync::OnceLock<Vec<GlyphRunView>>]>,
}

enum Cut {
    None,
    Blocked,
    At(Range<usize>),
}

/// Where `text-overflow` cuts an overflowing line: the longest prefix of units whose whole-pixel
/// width leaves room for `ell`, never before a zero-advance cluster (the rest of a ligature or a
/// combining mark belongs to the glyph before it). Whole clusters only.
fn overflow_cut(snap: &SnapTable, fl: &FrozenLine, ell: &SynthRun) -> Cut {
    if !(fl.inline_size.is_finite() && fl.advance > fl.inline_size + 1e-3) {
        return Cut::None;
    }
    let units = &snap.clusters[fl.units.clone()];
    let budget = fl.inline_size - ell.width;
    let (mut used, mut keep) = (0.0f32, 0usize);
    for (k, u) in units.iter().enumerate() {
        if used + u.snapped > budget + 1e-3 {
            break;
        }
        used += u.snapped;
        keep = k + 1;
    }
    while keep > 0
        && keep < units.len()
        && units[keep].kind == UnitKind::Text
        && units[keep].snapped == 0.0
    {
        keep -= 1;
    }
    let hidden = &units[keep..];
    if hidden.iter().any(|u| matches!(u.kind, UnitKind::Atomic(_))) {
        return Cut::Blocked;
    }
    let start = hidden
        .iter()
        .find(|u| u.kind == UnitKind::Text)
        .map(|u| u.start);
    let end = hidden
        .iter()
        .rev()
        .find(|u| u.kind == UnitKind::Text)
        .map(|u| u.end);
    match (start, end) {
        (Some(s), Some(e)) => Cut::At(s..e),
        _ => Cut::None,
    }
}

/// Where the last line of a line clamp is cut for `ell`: the clusters that fit before it stay; with
/// room to spare the ellipsis simply follows the last non-hanging text.
fn clamp_cut(snap: &SnapTable, fl: &FrozenLine, ell: &SynthRun) -> Cut {
    let units = &snap.clusters[fl.units.clone()];
    let budget = if fl.inline_size.is_finite() {
        fl.inline_size - ell.width
    } else {
        f32::INFINITY
    };
    let mut visible = units.len();
    // trailing white space hangs: the ellipsis follows the last glyph
    while visible > 0 && units[visible - 1].kind == UnitKind::Text && units[visible - 1].hangs {
        visible -= 1;
    }
    let (mut used, mut keep) = (0.0f32, 0usize);
    for (k, u) in units[..visible].iter().enumerate() {
        if used + u.snapped > budget + 1e-3 {
            break;
        }
        used += u.snapped;
        keep = k + 1;
    }
    if keep < visible {
        while keep > 0 && units[keep].kind == UnitKind::Text && units[keep].snapped == 0.0 {
            keep -= 1;
        }
    }
    let shown = &units[..keep];
    let hidden = &units[keep..visible];
    if hidden.iter().any(|u| matches!(u.kind, UnitKind::Atomic(_))) {
        return Cut::Blocked;
    }
    let end_of_shown = shown
        .iter()
        .rev()
        .find(|u| u.kind == UnitKind::Text)
        .map(|u| u.end)
        .or(Some(fl.text_range.start));
    let end = hidden
        .iter()
        .rev()
        .find(|u| u.kind == UnitKind::Text)
        .map(|u| u.end);
    match (
        hidden
            .iter()
            .find(|u| u.kind == UnitKind::Text)
            .map(|u| u.start),
        end,
    ) {
        (Some(s), Some(e)) => Cut::At(s..e),
        // nothing cut: the ellipsis follows the end of the shown text
        _ => end_of_shown.map_or(Cut::None, |e| Cut::At(e..e)),
    }
}

/// Whether the bidi paragraph holding byte `at` has a right-to-left base direction: the source's
/// when it is fixed; the first strong character of the forced-break line (UAX #9 P2-P3, the
/// inserted controls ignored) for an `Auto` source whose lines are separate paragraphs, else the
/// layout's own resolution.
fn paragraph_is_rtl(source: &ParagraphSource, layout_rtl: bool, at: usize) -> bool {
    use crate::text::bidi::BaseDirection;
    match source.base_direction {
        BaseDirection::Ltr => false,
        BaseDirection::Rtl => true,
        BaseDirection::Auto if !source.isolated_paragraphs => layout_rtl,
        BaseDirection::Auto => {
            // A paragraph with no strong character takes the direction of the one before it, the
            // first one that of the containing block (CSS Text 3 `unicode-bidi: plaintext`).
            let text = &source.text;
            let at = at.min(text.len());
            let mut dir = source.fallback_rtl;
            let mut start = 0usize;
            loop {
                let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
                let line: String = text[start..end]
                    .chars()
                    .filter(|c| !crate::text::bidi::is_bidi_control(*c))
                    .collect();
                if let Some(d) = crate::text::bidi::first_strong(&line) {
                    dir = d;
                }
                if at <= end || end >= text.len() {
                    return dir;
                }
                start = end + 1;
            }
        }
    }
}

/// The cluster a glyph of a run belongs to and, for a tab, its advance.
#[derive(Clone, Copy)]
struct GlyphCluster {
    byte: usize,
    tab: Option<f32>,
}

/// One glyph run of a line positioned with the whole-pixel advances (`snap.rs`).
struct PlacedRun {
    span: u32,
    owner: u64,
    x: f32,
    width: f32,
    text_range: Range<usize>,
    baseline: f32,
    ascent: f32,
    descent: f32,
    glyphs: Vec<GlyphView>,
    font: Option<FontRef>,
    size_px: f32,
    bidi_level: u8,
    /// The hyphen at the end of a hyphenated line (015 TXT-3): a fragment of its own, as in the reference
    /// browser, where `getClientRects()` lists it after the text of the line.
    hyphen: bool,
}

impl std::fmt::Debug for FrozenParagraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrozenParagraph")
            .field("text", &self.source.text)
            .field("lines", &self.lines)
            .finish()
    }
}

impl ScratchLayout {
    /// Apply alignment and make the layout immutable. The breaker must have finished (the
    /// type system enforces it: the breaker borrows the scratch layout).
    pub fn freeze(self, params: FreezeParams) -> Arc<FrozenParagraph> {
        self.freeze_unchecked(params, FreezeExtras::default())
    }

    /// `freeze` with `text-overflow` at the line-end edge: lines that overflow their available
    /// inline size lose whole clusters at the end until `end` fits, and show it there.
    pub fn freeze_overflow(
        self,
        params: FreezeParams,
        end: Option<SynthRun>,
    ) -> Arc<FrozenParagraph> {
        self.freeze_unchecked(
            params,
            FreezeExtras {
                end_ellipsis: end,
                ..FreezeExtras::default()
            },
        )
    }

    /// `freeze_overflow` with the `text-align-last` alignment of the last line and of the lines
    /// that end in a forced break (`None`: the paragraph's alignment).
    pub fn freeze_aligned(
        self,
        params: FreezeParams,
        align_last: Option<TextAlignment>,
        end: Option<SynthRun>,
    ) -> Arc<FrozenParagraph> {
        self.freeze_unchecked(
            params,
            FreezeExtras {
                align_last,
                end_ellipsis: end,
                ..FreezeExtras::default()
            },
        )
    }

    /// `freeze` with the synthesized runs, hidden ranges, pieces and decorations computed by the
    /// inline behaviour embedded unchanged. Extras that name a line or a byte range outside the
    /// paragraph are interface misuse: `INTERNAL_INVARIANT`, nothing frozen.
    pub fn freeze_with(
        self,
        params: FreezeParams,
        extras: FreezeExtras,
    ) -> Result<Arc<FrozenParagraph>, Status> {
        let line_count = self.layout.len();
        let text_len = self.shaped.source.text.len();
        let bad = |what: &str| {
            Err(Status::new(Code::InternalInvariant, Stage::Text)
                .with_message(format!("freeze: {what}")))
        };
        if extras.lines.len() > line_count {
            return bad("more per-line extras than lines");
        }
        if extras.lines.iter().any(|l| {
            l.hidden_range
                .as_ref()
                .is_some_and(|r| r.start > r.end || r.end > text_len)
        }) {
            return bad("hidden range outside the paragraph text");
        }
        if extras.pieces.iter().any(|p| p.line_index >= line_count)
            || extras
                .decorations
                .iter()
                .any(|d| d.line_index >= line_count)
        {
            return bad("piece or decoration names a line outside the paragraph");
        }
        Ok(self.freeze_unchecked(params, extras))
    }

    fn freeze_unchecked(
        mut self,
        params: FreezeParams,
        extras: FreezeExtras,
    ) -> Arc<FrozenParagraph> {
        // Parley's own alignment works on fractional advances and on the max advances the
        // breaker steered it with; the line offset is computed here from whole-pixel advances
        // (`snap.rs`) and the line's real available width. Parley keeps a start-aligned layout.
        self.layout.align(
            Alignment::Left,
            AlignmentOptions {
                align_when_overflowing: false,
                last_line_alignment: None,
            },
        );
        let snap = self.snap.clone();
        let lines: Vec<FrozenLine> = self
            .layout
            .lines()
            .enumerate()
            .map(|(i, line)| {
                let m = line.metrics();
                let committed = self.lines.get(i);
                let text_range = line.text_range();
                let units = committed.map(|c| c.units.clone()).unwrap_or_else(|| {
                    snap.first_text_unit_at(text_range.start)
                        ..snap.first_text_unit_at(text_range.end)
                });
                let w = snap.widths(units.clone());
                // a line that soft-wraps at a soft hyphen or a hyphenation point ends with the hyphen
                let hyphen = if line.break_reason() == parley::BreakReason::Regular {
                    snap.hyphen_at(units.end)
                } else {
                    None
                };
                let advance = w.snapped + hyphen.map_or(0.0, |h| h.width);
                let available = committed
                    .map(|c| c.available)
                    .filter(|a| a.is_finite())
                    .unwrap_or(advance);
                let free = available - advance;
                let last_like = matches!(
                    line.break_reason(),
                    parley::BreakReason::None | parley::BreakReason::Explicit
                );
                let alignment = if last_like {
                    extras.align_last.unwrap_or(params.alignment)
                } else {
                    params.alignment
                };
                let line_rtl =
                    paragraph_is_rtl(&self.shaped.source, self.layout.is_rtl(), text_range.start);
                let alignment = match alignment {
                    TextAlignment::Start => {
                        if line_rtl {
                            TextAlignment::Right
                        } else {
                            TextAlignment::Left
                        }
                    }
                    TextAlignment::End => {
                        if line_rtl {
                            TextAlignment::Left
                        } else {
                            TextAlignment::Right
                        }
                    }
                    other => other,
                };
                // `justify`: the free room is shared by the word spaces that do not hang.
                let (mut justify, mut justify_end) = (0.0f32, usize::MAX);
                if alignment == TextAlignment::Justify && free > 0.0 {
                    let text = &self.shaped.source.text;
                    let cs = &snap.clusters
                        [units.start.min(snap.clusters.len())..units.end.min(snap.clusters.len())];
                    if let Some(h) = cs.iter().rev().take_while(|u| u.hangs).last() {
                        justify_end = h.start;
                    }
                    let spaces = cs
                        .iter()
                        .filter(|u| {
                            u.kind == UnitKind::Text
                                && u.start < justify_end
                                && matches!(text.get(u.start..u.end), Some(" ") | Some("\u{a0}"))
                        })
                        .count();
                    if spaces > 0 {
                        justify = free / spaces as f32;
                    }
                }
                let offset = if free <= 0.0 {
                    // An overflowing line is anchored at its inline-start edge,
                    // regardless of text-align. In RTL that edge is the right
                    // edge, so the excess must extend into negative x.
                    if line_rtl { free } else { 0.0 }
                } else {
                    match alignment {
                        TextAlignment::Left
                        | TextAlignment::Justify
                        | TextAlignment::Start
                        | TextAlignment::End => 0.0,
                        TextAlignment::Right => free,
                        TextAlignment::Center => free / 2.0,
                    }
                };
                let top = committed.map(|c| c.top).unwrap_or(m.block_min_coord);
                FrozenLine {
                    top,
                    height: committed.map(|c| c.height).unwrap_or(m.line_height),
                    // Parley's quantized top is rounded; the baseline keeps its whole-pixel
                    // offset from the unrounded line top.
                    baseline: top + (m.baseline - m.block_min_coord),
                    inline_start: m.inline_min_coord,
                    inline_size: available,
                    offset,
                    // A justified line fills its width.
                    advance: if justify > 0.0 {
                        advance + free
                    } else {
                        advance
                    },
                    justify,
                    justify_end,
                    text_range,
                    units,
                    rtl: line_rtl,
                    end: match line.break_reason() {
                        parley::BreakReason::None => LineEnd::End,
                        parley::BreakReason::Regular => LineEnd::SoftWrap,
                        parley::BreakReason::Explicit => LineEnd::Forced,
                        parley::BreakReason::Emergency => LineEnd::Emergency,
                    },
                }
            })
            .collect();
        let mut extras = extras;
        for (i, fl) in lines.iter().enumerate() {
            if fl.end == LineEnd::SoftWrap
                && let Some(h) = snap.hyphen_at(fl.units.end)
                && let Some(Some(run)) = self.shaped.hyphen_runs.get(h.span as usize)
            {
                if extras.lines.len() <= i {
                    extras.lines.resize(i + 1, LineExtras::default());
                }
                extras.lines[i].hyphen = Some(run.clone());
            }
        }
        let mut overflow_blocked = Vec::new();
        if let Some(ell) = extras.end_ellipsis.clone() {
            extras.lines.resize(lines.len(), LineExtras::default());
            for (i, fl) in lines.iter().enumerate() {
                match overflow_cut(&snap, fl, &ell) {
                    Cut::None => {}
                    Cut::Blocked => overflow_blocked.push(i),
                    Cut::At(range) => {
                        extras.lines[i].ellipsis = Some(ell.clone());
                        extras.lines[i].hidden_range = Some(range);
                    }
                }
            }
        }
        if let Some((n, ell)) = extras.clamp.clone()
            && n >= 1
            && lines.len() > n
        {
            extras.lines.resize(lines.len(), LineExtras::default());
            for l in extras.lines.iter_mut().skip(n) {
                l.clamped_out = true;
            }
            if let Cut::At(range) = clamp_cut(&snap, &lines[n - 1], &ell) {
                extras.lines[n - 1].ellipsis = Some(ell);
                extras.lines[n - 1].hidden_range = Some(range);
            }
        }
        let mut fp = FrozenParagraph {
            source: self.shaped.source.clone(),
            missing: self.shaped.missing.clone(),
            blobs: self.shaped.blobs.clone(),
            policy: self.shaped.policy.clone(),
            snap,
            layout: self.layout,
            lines,
            extras,
            deco_fonts: self.shaped.deco_fonts.clone(),
            overflow_blocked,
            bidi: self.shaped.source.is_bidi(),
            pieces_memo: Box::default(),
            runs_memo: Box::default(),
        };
        // The decorations of the source's decorating boxes (host-supplied segments, such as a
        // preedit underline, stay in front).
        let computed = super::decoration::line_segments(&fp);
        fp.extras.decorations.extend(computed);
        let n = fp.lines.len();
        fp.pieces_memo = (0..n).map(|_| std::sync::OnceLock::new()).collect();
        fp.runs_memo = (0..n).map(|_| std::sync::OnceLock::new()).collect();
        Arc::new(fp)
    }
}

impl FrozenParagraph {
    pub fn source(&self) -> &ParagraphSource {
        &self.source
    }

    pub fn text(&self) -> &str {
        &self.source.text
    }

    pub fn lines(&self) -> &[FrozenLine] {
        &self.lines
    }

    /// The synthesized runs and hidden range embedded for a line (default when none).
    pub fn line_extras(&self, line_index: usize) -> LineExtras {
        self.extras
            .lines
            .get(line_index)
            .cloned()
            .unwrap_or_default()
    }

    /// Inline-box pieces embedded at freeze, in the order given.
    pub fn pieces(&self) -> &[InlineBoxPiece] {
        &self.extras.pieces
    }

    /// Decoration segments: the host-supplied ones embedded at freeze in the order given, then
    /// those computed from the source's decorating boxes (`DecorationLayer`s), in line order.
    pub fn decorations(&self) -> &[DecorationSegment] {
        &self.extras.decorations
    }

    /// The segments of one line (host-supplied and computed).
    pub fn line_decorations(&self, line_index: usize) -> impl Iterator<Item = &DecorationSegment> {
        self.extras
            .decorations
            .iter()
            .filter(move |d| d.line_index == line_index)
    }

    pub(super) fn deco_fonts(&self) -> &[DecoFont] {
        &self.deco_fonts
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Sum of line heights.
    pub fn height(&self) -> f32 {
        self.lines.iter().map(|l| l.height).sum()
    }

    pub fn first_baseline(&self) -> Option<f32> {
        self.lines.first().map(|l| l.baseline)
    }

    pub fn last_baseline(&self) -> Option<f32> {
        self.lines.last().map(|l| l.baseline)
    }

    /// Characters drawn with `.notdef`.
    pub fn missing_glyphs(&self) -> &[MissingGlyph] {
        &self.missing
    }

    /// Lines that overflow their inline size where `text-overflow` would cut, but whose hidden
    /// part holds an atomic inline (hidden whole, CSS UI 4 section 5.1, which this layer does not
    /// do): they are left uncut and the caller reports the property.
    pub fn overflow_blocked_lines(&self) -> &[usize] {
        &self.overflow_blocked
    }

    /// The inline-axis end of the line's content as laid out *before* `text-overflow` cut it (the
    /// line start plus its alignment offset and content advance, paragraph space), for a line the
    /// cut shortened; `None` for an uncut line. A cut line's scrollable overflow still reaches the
    /// full text (CSS Overflow 3 section 2.2; `scrollWidth` of a `text-overflow: ellipsis` box).
    pub fn uncut_content_end(&self, line_index: usize) -> Option<f32> {
        let cut = self
            .extras
            .lines
            .get(line_index)
            .is_some_and(|e| e.hidden_range.is_some());
        let fl = self.lines.get(line_index)?;
        cut.then_some(fl.inline_start + fl.offset + fl.advance)
    }

    fn owner_of(&self, span: SpanBrush) -> (u32, u64) {
        match self.source.spans.get(span.0 as usize) {
            Some(s) => (span.0, s.owner),
            // Root-styled text outside every span (cannot happen for a covering span list).
            None => (span.0, u64::MAX),
        }
    }

    /// The line's glyph runs in visual order, placed with whole-pixel advances from the
    /// line's aligned start (`snap.rs`).
    fn placed_runs(&self, line_index: usize) -> Vec<PlacedRun> {
        self.placed_items(line_index).0
    }

    /// The line's atomic inlines, placed like the text.
    pub fn atomic_placements(&self, line_index: usize) -> Vec<AtomicPlacement> {
        // Only AtomicInline source items are passed to Parley as in-flow inline boxes.
        // Float and out-of-flow placeholders are CustomOutOfFlow and do not appear here.
        if !self
            .source
            .items
            .iter()
            .any(|item| item.kind == super::source::InlineItemKind::AtomicInline)
        {
            return Vec::new();
        }
        self.placed_items(line_index).1
    }

    /// The edge spacers of the line's inline boxes, placed like the text. Edges are placed for
    /// left-to-right runs only (the inline behaviour does not generate them in bidi paragraphs).
    pub fn edge_placements(&self, line_index: usize) -> Vec<EdgePlacement> {
        self.placed_items(line_index).2
    }

    /// Text runs, atomics and edge spacers of a line in visual order; the pen moves over all three.
    fn placed_items(
        &self,
        line_index: usize,
    ) -> (Vec<PlacedRun>, Vec<AtomicPlacement>, Vec<EdgePlacement>) {
        if self.bidi
            && self
                .extras
                .lines
                .get(line_index)
                .is_none_or(|e| e.hidden_range.is_none())
        {
            let (runs, atomics, edges, _) = self.placed_items_bidi(line_index);
            return (runs, atomics, edges);
        }
        let mut out = Vec::new();
        let mut atomics = Vec::new();
        let mut edges = Vec::new();
        let (Some(line), Some(fl)) = (self.layout.get(line_index), self.lines.get(line_index))
        else {
            return (out, atomics, edges);
        };
        let shift = fl.baseline - line.metrics().baseline;
        let mut pen = fl.inline_start + fl.offset;
        let mut next_unit = fl.units.start;
        // `text-overflow`: the part of the line from `hidden.start` is not placed; the ellipsis
        // takes its place after the last visible glyph.
        let line_extras = self.extras.lines.get(line_index);
        let hidden = line_extras.and_then(|e| e.hidden_range.clone());
        let cut_unit = hidden
            .as_ref()
            .map(|h| self.snap.first_text_unit_at(h.start))
            .unwrap_or(usize::MAX);
        // Edge spacers of units `next_unit..upto` are laid out before whatever comes next.
        let flush_edges =
            |upto: usize, next_unit: &mut usize, pen: &mut f32, edges: &mut Vec<EdgePlacement>| {
                while *next_unit < upto.min(fl.units.end).min(cut_unit) {
                    let u = &self.snap.clusters[*next_unit];
                    if let UnitKind::Edge { start } = u.kind {
                        let owner = self.edge_owner(*next_unit);
                        edges.push(EdgePlacement {
                            owner,
                            start,
                            x: *pen,
                            width: u.snapped,
                        });
                        *pen += u.snapped;
                    }
                    *next_unit += 1;
                }
            };
        for item in line.items() {
            let gr = match item {
                PositionedLayoutItem::GlyphRun(gr) => gr,
                PositionedLayoutItem::InlineBox(b) => {
                    if b.kind != parley::InlineBoxKind::InFlow {
                        continue;
                    }
                    if let Some(k) = self.snap.atomic_unit(b.id) {
                        flush_edges(k, &mut next_unit, &mut pen, &mut edges);
                        next_unit = next_unit.max(k + 1);
                    }
                    atomics.push(AtomicPlacement {
                        item: b.id,
                        x: pen,
                        y: b.y + shift,
                        width: b.width,
                        height: super::scratch::atomic_height(b.height),
                    });
                    pen += b.width;
                    continue;
                }
            };
            let run = gr.run();
            let font = run.font();
            let face = FaceKey::of(run);
            let (span, owner) = self.owner_of(gr.style().brush);
            // A glyph run is one style's part of a shaping run: its text is the run's range
            // intersected with the span's range.
            let run_range = run.text_range();
            let full_range = match self.source.spans.get(span as usize) {
                Some(s) => run_range.start.max(s.range.start)..run_range.end.min(s.range.end),
                None => run_range,
            };
            if hidden.as_ref().is_some_and(|h| full_range.start >= h.start) {
                continue;
            }
            let text_range = match &hidden {
                Some(h) => full_range.start..full_range.end.min(h.start),
                None => full_range.clone(),
            };
            flush_edges(
                self.snap.first_text_unit_at(text_range.start),
                &mut next_unit,
                &mut pen,
                &mut edges,
            );
            next_unit = next_unit.max(self.snap.text_units_end(text_range.end));
            let baseline = gr.baseline() + shift;
            let x = pen;
            let orientation = self
                .source
                .spans
                .get(span as usize)
                .map(|s| s.props.orientation)
                .unwrap_or_default();
            let vertical = !matches!(
                orientation,
                super::vertical::GlyphOrientation::Horizontal
                    | super::vertical::GlyphOrientation::Sideways
            );
            let info = self.glyph_clusters(
                *run,
                &full_range,
                gr.glyphs().count(),
                hidden.is_some() || fl.justify > 0.0 || vertical,
            );
            let fm = run.font_metrics();
            let mut glyphs: Vec<GlyphView> = Vec::new();
            let mut last_justified: Option<usize> = None;
            // `text-combine-upright: all` (015 WM-1b): the run is one em along the line; its glyphs are set
            // horizontally, centred on the run's central baseline and in the em, compressed when wider.
            let combine = self.source.spans.get(span as usize).is_some_and(|s| {
                s.props.combine_upright
                    && s.props.orientation != super::vertical::GlyphOrientation::Horizontal
            });
            if combine && hidden.is_none() {
                let em = run.font_size();
                let advances: Vec<f32> = gr
                    .glyphs()
                    .map(|g| self.policy.snap(face, g.id, g.advance))
                    .collect();
                let width: f32 = advances.iter().sum();
                let s = if width > em && width > 0.0 {
                    em / width
                } else {
                    1.0
                };
                let central = baseline - super::vertical::central_offset(fm.ascent, fm.descent);
                let along = pen + em / 2.0 + (fm.ascent - fm.descent) / 2.0;
                let mut off = 0.0;
                for (g, a) in gr.glyphs().zip(advances) {
                    glyphs.push(GlyphView {
                        id: g.id,
                        x: along,
                        y: central + width * s / 2.0 - off * s,
                        advance: 0.0,
                        upright: true,
                        scale_x: s,
                    });
                    off += a;
                }
                pen += em;
            }
            for (i, g) in gr.glyphs().enumerate() {
                if combine && hidden.is_none() {
                    break;
                }
                let gc = info.get(i);
                if let (Some(h), Some(gc)) = (&hidden, gc)
                    && gc.byte >= h.start
                {
                    continue;
                }
                let mut advance = gc
                    .and_then(|c| c.tab)
                    .unwrap_or_else(|| self.policy.snap(face, g.id, g.advance));
                // The word spaces of a justified line grow by the line's share (one per space).
                if fl.justify > 0.0
                    && let Some(gc) = gc
                    && gc.byte < fl.justify_end
                    && last_justified != Some(gc.byte)
                    && matches!(
                        self.source
                            .text
                            .get(gc.byte..)
                            .and_then(|t| t.chars().next()),
                        Some(' ') | Some('\u{a0}')
                    )
                {
                    advance += fl.justify;
                    last_justified = Some(gc.byte);
                }
                // An upright glyph (015 WM-1b): its vertical advance along the line, its vertical origin at the
                // pen and its horizontal centre on the run's central baseline.
                if vertical
                    && let Some(gc) = gc
                    && self
                        .source
                        .text
                        .get(gc.byte..)
                        .and_then(|t| t.chars().next())
                        .is_some_and(|ch| orientation.upright(ch))
                {
                    let advance = gc.tab.unwrap_or_else(|| {
                        self.policy
                            .upright_advance(face, g.id, fm.ascent, fm.descent)
                    });
                    let origin = self.policy.upright_origin(face, g.id, fm.ascent);
                    let central = baseline - super::vertical::central_offset(fm.ascent, fm.descent);
                    glyphs.push(GlyphView {
                        id: g.id,
                        x: pen + origin,
                        y: central + g.advance / 2.0,
                        advance,
                        upright: true,
                        scale_x: 1.0,
                    });
                    pen += advance;
                    continue;
                }
                glyphs.push(GlyphView {
                    id: g.id,
                    x: pen + g.x,
                    y: baseline + g.y,
                    advance,
                    upright: false,
                    scale_x: 1.0,
                });
                pen += advance;
            }
            let metrics = run.font_metrics();
            let synthesis = run.synthesis();
            // `font-synthesis-weight: none` and `-style: none` (CSS Fonts 4 section 2.5): no faux bold, no faux oblique
            let (no_synth_weight, no_synth_style) =
                self.source
                    .spans
                    .get(span as usize)
                    .map_or((false, false), |s| {
                        (
                            s.props.font_settings.no_synthesis_weight,
                            s.props.font_settings.no_synthesis_style,
                        )
                    });
            out.push(PlacedRun {
                span,
                owner,
                x,
                width: pen - x,
                text_range,
                baseline,
                ascent: metrics.ascent.round(),
                descent: metrics.descent.round(),
                glyphs,
                font: self
                    .blobs
                    .iter()
                    .find(|b| {
                        b.id.0 == crate::text::fonts::environment::published_blob_id(font.data.id())
                    })
                    .map(|blob| FontRef {
                        blob: blob.clone(),
                        index: font.index,
                        coords: run
                            .normalized_coords()
                            .iter()
                            .map(|c| c.to_bits())
                            .collect(),
                        embolden: synthesis.embolden() && !no_synth_weight,
                        oblique_angle_deg: synthesis.skew().filter(|_| !no_synth_style),
                    }),
                size_px: run.font_size(),
                bidi_level: u8::from(run.is_rtl()),
                hyphen: false,
            });
        }
        flush_edges(fl.units.end, &mut next_unit, &mut pen, &mut edges);
        if let (Some(h), Some(ell)) = (&hidden, line_extras.and_then(|e| e.ellipsis.as_ref())) {
            // The ellipsis follows the last visible run (it takes that run's span and owner, so the
            // text fragment that ends there carries it) or, with everything hidden, the first span.
            let (span, owner, baseline, ascent, descent) = match out.last() {
                Some(r) => (r.span, r.owner, r.baseline, r.ascent, r.descent),
                None => (
                    0,
                    self.source
                        .spans
                        .first()
                        .map(|s| s.owner)
                        .unwrap_or(u64::MAX),
                    fl.baseline,
                    ell.ascent,
                    ell.descent,
                ),
            };
            let font = ell.font.as_ref().and_then(|f| {
                self.blobs
                    .iter()
                    .find(|b| b.id.0 == f.blob)
                    .map(|blob| FontRef {
                        blob: blob.clone(),
                        index: f.index,
                        coords: f.coords.clone(),
                        embolden: f.embolden,
                        oblique_angle_deg: f.oblique_angle_deg,
                    })
            });
            out.push(PlacedRun {
                span,
                owner,
                x: pen,
                width: ell.width,
                text_range: h.start..h.start,
                baseline,
                ascent,
                descent,
                glyphs: ell
                    .glyphs
                    .iter()
                    .map(|g| GlyphView {
                        id: g.id,
                        x: pen + g.x,
                        y: baseline + g.y,
                        advance: g.advance,
                        upright: false,
                        scale_x: 1.0,
                    })
                    .collect(),
                font,
                size_px: ell.font.as_ref().map(|f| f.size_px).unwrap_or(0.0),
                bidi_level: 0,
                hyphen: false,
            });
        }
        // The hyphen of a line that wraps at a soft hyphen or a hyphenation point: drawn after the last
        // glyph of the line, in the span's font; it is no text (an empty range at the end of the line).
        if hidden.is_none()
            && let Some(hy) = line_extras.and_then(|e| e.hyphen.as_ref())
        {
            let (span, owner, baseline, ascent, descent) =
                match out.iter().rev().find(|r| !r.text_range.is_empty()) {
                    Some(r) => (r.span, r.owner, r.baseline, r.ascent, r.descent),
                    None => (
                        0,
                        self.source
                            .spans
                            .first()
                            .map(|s| s.owner)
                            .unwrap_or(u64::MAX),
                        fl.baseline,
                        hy.ascent,
                        hy.descent,
                    ),
                };
            let font = hy.font.as_ref().and_then(|f| {
                self.blobs
                    .iter()
                    .find(|b| b.id.0 == f.blob)
                    .map(|blob| FontRef {
                        blob: blob.clone(),
                        index: f.index,
                        coords: f.coords.clone(),
                        embolden: f.embolden,
                        oblique_angle_deg: f.oblique_angle_deg,
                    })
            });
            let at = fl.text_range.end;
            out.push(PlacedRun {
                span,
                owner,
                x: pen,
                width: hy.width,
                text_range: at..at,
                baseline,
                ascent,
                descent,
                glyphs: hy
                    .glyphs
                    .iter()
                    .map(|g| GlyphView {
                        id: g.id,
                        x: pen + g.x,
                        y: baseline + g.y,
                        advance: g.advance,
                        upright: false,
                        scale_x: 1.0,
                    })
                    .collect(),
                font,
                size_px: hy.font.as_ref().map(|f| f.size_px).unwrap_or(0.0),
                bidi_level: 0,
                hyphen: true,
            });
        }
        (out, atomics, edges)
    }

    /// Per glyph of a glyph run, the cluster it belongs to and, for a tab, its advance (the stop
    /// distance from the snap table; a tab is shaped as one space glyph). Computed only when the
    /// run holds a tab or `need` asks for it (`text-overflow` hides glyphs by cluster); empty when
    /// not needed or when the run does not give one entry per glyph.
    fn glyph_clusters(
        &self,
        run: parley::Run<'_, SpanBrush>,
        run_range: &Range<usize>,
        glyph_count: usize,
        need: bool,
    ) -> Vec<GlyphCluster> {
        let text = &self.source.text;
        if !need
            && !text
                .get(run_range.clone())
                .is_some_and(|t| t.contains('\t'))
        {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(glyph_count);
        // The glyphs of a run are in visual order: so are the clusters whose glyph counts pair with them.
        for c in run.visual_clusters().filter(|c| {
            c.text_range().start >= run_range.start && c.text_range().start < run_range.end
        }) {
            let r = c.text_range();
            let tab = if text.get(r.clone()) == Some("\t") {
                self.snap
                    .clusters
                    .iter()
                    .find(|u| u.tab && u.start == r.start)
                    .map(|u| u.snapped)
            } else {
                None
            };
            for _ in 0..c.glyphs().count() {
                out.push(GlyphCluster { byte: r.start, tab });
            }
        }
        if out.len() == glyph_count {
            out
        } else {
            Vec::new()
        }
    }

    /// The owner tag of the edge spacer at unit `unit`: the `i`-th edge unit is the `i`-th
    /// `EdgeSpacer` item of the source.
    fn edge_owner(&self, unit: usize) -> u64 {
        let nth = self.snap.clusters[..unit]
            .iter()
            .filter(|u| matches!(u.kind, UnitKind::Edge { .. }))
            .count();
        self.source
            .items
            .iter()
            .filter(|i| matches!(i.kind, super::source::InlineItemKind::EdgeSpacer { .. }))
            .nth(nth)
            .map(|i| i.owner)
            .unwrap_or(u64::MAX)
    }

    /// Text pieces of a line in visual order, adjacent glyph runs of one span merged.
    pub fn text_pieces(&self, line_index: usize) -> Vec<TextPiece> {
        match self.pieces_memo.get(line_index) {
            Some(m) => m
                .get_or_init(|| self.compute_text_pieces(line_index))
                .clone(),
            None => self.compute_text_pieces(line_index),
        }
    }

    fn compute_text_pieces(&self, line_index: usize) -> Vec<TextPiece> {
        let mut out: Vec<TextPiece> = Vec::new();
        for r in self.placed_runs(line_index) {
            // UBA controls are not DOM text: a run of nothing else (and the line break after them)
            // makes no piece, and the controls at the ends of a piece are cut off its range.
            let mut r = r;
            if self.bidi && !r.text_range.is_empty() {
                let t = self.source.text.get(r.text_range.clone()).unwrap_or("");
                if t.chars().any(crate::text::bidi::is_bidi_control) {
                    if t.chars()
                        .all(|c| crate::text::bidi::is_bidi_control(c) || c == '\n')
                    {
                        continue;
                    }
                    let lead = t.len()
                        - t.trim_start_matches(crate::text::bidi::is_bidi_control)
                            .len();
                    let trail =
                        t.len() - t.trim_end_matches(crate::text::bidi::is_bidi_control).len();
                    r.text_range = r.text_range.start + lead..r.text_range.end - trail;
                }
            }
            if !r.hyphen
                && let Some(last) = out.last_mut()
                && last.span == r.span
                && last.bidi_level == r.bidi_level
                && (last.x + last.width - r.x).abs() < 1e-3
            {
                last.width += r.width;
                last.text_range.start = last.text_range.start.min(r.text_range.start);
                last.text_range.end = last.text_range.end.max(r.text_range.end);
                // The content area covers every font of the piece.
                let (top, bottom) = (
                    (last.baseline - last.ascent).min(r.baseline - r.ascent),
                    (last.baseline + last.descent).max(r.baseline + r.descent),
                );
                last.ascent = last.baseline - top;
                last.descent = bottom - last.baseline;
                continue;
            }
            out.push(TextPiece {
                span: r.span,
                owner: r.owner,
                x: r.x,
                width: r.width,
                text_range: r.text_range,
                bidi_level: r.bidi_level,
                baseline: r.baseline,
                ascent: r.ascent,
                descent: r.descent,
                hyphen: r.hyphen,
            });
        }
        // Trailing collapsible white space hangs: it is not part of any text fragment (CSS Text 3
        // §4.1.3; Chromium removes it from the item), so no piece extends past the content
        // advance. Painting uses `glyph_runs`, which keep the (blank) hanging glyphs.
        if let Some(fl) = self.lines.get(line_index) {
            let end = fl.inline_start + fl.offset + fl.advance;
            let start = fl.inline_start + fl.offset;
            let rtl = self.bidi && self.lines.get(line_index).is_some_and(|l| l.rtl);
            for p in &mut out {
                if p.x + p.width > end + 1e-3 {
                    p.width = (end - p.x).max(0.0);
                }
                // A right-to-left paragraph's hanging white space is left of the aligned content.
                if rtl && p.x < start - 1e-3 {
                    p.width = (p.x + p.width - start).max(0.0);
                    p.x = start;
                }
            }
        }
        out
    }

    /// Caret stops of a line in logical order: the UTF-8 byte offset of every grapheme-cluster
    /// boundary the line holds and the pen x (paragraph space) there, from the same whole-pixel
    /// advances the glyph runs use. Left-to-right text only: a right-to-left run would need the
    /// visual order of `move_visual` (X-BIDI), so `bidi` is reported and the stops stay logical.
    /// The first stop is the start of the line's first unit; for an empty line the single stop is
    /// the line start.
    pub fn caret_stops(&self, line_index: usize) -> CaretStops {
        let Some(fl) = self.lines.get(line_index) else {
            return CaretStops::default();
        };
        if self.bidi
            && self
                .extras
                .lines
                .get(line_index)
                .is_none_or(|e| e.hidden_range.is_none())
        {
            return self.caret_stops_bidi(line_index);
        }
        let mut pen = fl.inline_start + fl.offset;
        let mut stops = Vec::new();
        let units = &self.snap.clusters[fl.units.clone().start.min(self.snap.clusters.len())
            ..fl.units.end.min(self.snap.clusters.len())];
        let first = units
            .iter()
            .find(|u| u.kind == super::snap::UnitKind::Text)
            .map(|u| u.start)
            .unwrap_or(fl.text_range.start);
        stops.push(CaretStop {
            byte: first,
            x: pen,
        });
        for u in units
            .iter()
            .filter(|u| u.kind == super::snap::UnitKind::Text)
        {
            pen += u.snapped;
            stops.push(CaretStop {
                byte: u.end,
                x: pen,
            });
        }
        let bidi = self.layout.get(line_index).is_some_and(|l| {
            l.items()
                .any(|i| matches!(i, PositionedLayoutItem::GlyphRun(gr) if gr.run().is_rtl()))
        });
        CaretStops { stops, bidi }
    }

    /// The caret stops of a line of a bidi paragraph, in logical order, with the x of the caret
    /// at each: between two clusters of one direction the trailing edge of the first; between
    /// clusters of different directions the leading edge of the second (the caret of the
    /// character it would type before, downstream). UBA controls and the zero-width clusters
    /// inside them are no stops.
    fn caret_stops_bidi(&self, line_index: usize) -> CaretStops {
        let Some(fl) = self.lines.get(line_index) else {
            return CaretStops::default();
        };
        let ext: Vec<ClusterExtent> = self
            .cluster_extents(line_index)
            .into_iter()
            .filter(|e| self.control_at(e.start).is_none())
            .collect();
        let mut stops = Vec::with_capacity(ext.len() + 1);
        let Some(first) = ext.first() else {
            return CaretStops {
                stops: vec![CaretStop {
                    byte: fl.text_range.start,
                    x: fl.inline_start + fl.offset,
                }],
                bidi: fl.rtl,
            };
        };
        stops.push(CaretStop {
            byte: first.start,
            x: first.lead(),
        });
        for (i, e) in ext.iter().enumerate() {
            let x = match ext.get(i + 1) {
                Some(n) if n.rtl != e.rtl => n.lead(),
                _ => e.trail(),
            };
            stops.push(CaretStop { byte: e.end, x });
        }
        let bidi = fl.rtl || ext.iter().any(|e| e.rtl);
        CaretStops { stops, bidi }
    }

    /// Glyph runs of a line for painting (009 `text-services.md` §5).
    pub fn glyph_runs(&self, line_index: usize) -> Vec<GlyphRunView> {
        match self.runs_memo.get(line_index) {
            Some(m) => m
                .get_or_init(|| self.compute_glyph_runs(line_index))
                .clone(),
            None => self.compute_glyph_runs(line_index),
        }
    }

    /// [`Self::glyph_runs`] handed to `f` by reference (no copy of the glyphs).
    pub fn with_glyph_runs<R>(&self, line_index: usize, f: impl FnOnce(&[GlyphRunView]) -> R) -> R {
        match self.runs_memo.get(line_index) {
            Some(m) => f(m.get_or_init(|| self.compute_glyph_runs(line_index))),
            None => f(&self.compute_glyph_runs(line_index)),
        }
    }

    fn compute_glyph_runs(&self, line_index: usize) -> Vec<GlyphRunView> {
        // A line past a line clamp keeps its geometry but paints nothing.
        if self
            .extras
            .lines
            .get(line_index)
            .is_some_and(|e| e.clamped_out)
        {
            return Vec::new();
        }
        self.placed_runs(line_index)
            .into_iter()
            .filter_map(|r| {
                Some(GlyphRunView {
                    origin: (r.x, r.baseline),
                    font: r.font?,
                    size_px: r.size_px,
                    span: r.span,
                    owner: r.owner,
                    glyphs: r.glyphs,
                    bidi_level: r.bidi_level,
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Bidirectional placement (T095, T096; FR-053..FR-057).
//
// Parley reorders the glyph runs of a line (UAX #9 L1-L2) but knows nothing of the inline-box edge
// spacers and treats an atomic inline as a copy of the run before it. A line of a bidi paragraph
// is therefore placed here: the runs in Parley's visual order, with the edge spacers and atomic
// inlines (neutral objects, UAX #9 N1-N2) put where their neighbours say. The neighbours' direction
// is the resolved direction of the run holding the nearest non-control character on each side
// within the object's embedding; when the two sides differ, or one is missing, the embedding's
// direction decides. The object then sits against the side that shares its direction: after the
// previous run for a left-to-right object, left of it for a right-to-left one, and so on.

/// One glyph run of a bidi line, in Parley's visual order.
struct BidiRun<'a> {
    gr: parley::GlyphRun<'a, SpanBrush>,
    rtl: bool,
    span: u32,
    owner: u64,
    /// The run's text range (clipped to its span).
    range: Range<usize>,
    /// Per glyph, the cluster it belongs to; empty when the run does not give one entry per glyph
    /// (then no neutral object is placed inside the run).
    clusters: Vec<GlyphCluster>,
}

/// An edge spacer or atomic inline of a bidi line.
#[derive(Clone, Copy)]
struct Neutral {
    unit: usize,
    at: usize,
    rtl: bool,
    kind: NeutralKind,
}

#[derive(Clone, Copy)]
enum NeutralKind {
    Edge {
        start: bool,
        width: f32,
    },
    Atomic {
        item: u64,
        width: f32,
        height: f32,
        y: f32,
    },
}

/// Where a run of a line (or a part of one) sits in the visual sequence.
enum Slot {
    Run {
        run: usize,
        bytes: Option<Range<usize>>,
    },
    Neutral(usize),
}

/// Where a neutral object sits against the runs of its line. `Inside` splits a run; `Pre` and
/// `Post` put the object against the left or right end of a run; `Left` and `Right` against the
/// line's visual edges.
#[derive(Clone, Copy)]
enum Anchor {
    Inside(usize),
    Pre(usize),
    Post(usize),
    Left,
    Right,
}

impl FrozenParagraph {
    /// Whether the paragraph's lines are placed in bidi visual order.
    pub fn is_bidi(&self) -> bool {
        self.bidi
    }

    /// The paragraph's base direction is right-to-left.
    pub fn is_rtl(&self) -> bool {
        self.layout.is_rtl()
    }

    fn control_at(&self, byte: usize) -> Option<char> {
        self.source
            .text
            .get(byte..)
            .and_then(|t| t.chars().next())
            .filter(|c| crate::text::bidi::is_bidi_control(*c))
    }

    /// The resolved direction of the nearest non-control character before `at` (`rtl`: true) in
    /// the object's own embedding, or `None` when the line, or the embedding, starts there.
    fn neighbour_before(&self, runs: &[BidiRun<'_>], at: usize, line_start: usize) -> Option<bool> {
        let text = &self.source.text;
        let mut depth = 0usize;
        let mut pos = at;
        while pos > line_start {
            let c = text[..pos].chars().next_back()?;
            pos -= c.len_utf8();
            match c {
                crate::text::bidi::PDI | crate::text::bidi::PDF => depth += 1,
                crate::text::bidi::LRI
                | crate::text::bidi::RLI
                | crate::text::bidi::FSI
                | crate::text::bidi::LRE
                | crate::text::bidi::RLE
                | crate::text::bidi::LRO
                | crate::text::bidi::RLO => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                }
                _ if depth > 0 => {}
                _ => return runs.iter().find(|r| r.range.contains(&pos)).map(|r| r.rtl),
            }
        }
        None
    }

    /// The same after `at`.
    fn neighbour_after(&self, runs: &[BidiRun<'_>], at: usize, line_end: usize) -> Option<bool> {
        let text = &self.source.text;
        let mut depth = 0usize;
        let mut pos = at;
        while pos < line_end {
            let c = text.get(pos..)?.chars().next()?;
            let here = pos;
            pos += c.len_utf8();
            match c {
                crate::text::bidi::LRI
                | crate::text::bidi::RLI
                | crate::text::bidi::FSI
                | crate::text::bidi::LRE
                | crate::text::bidi::RLE
                | crate::text::bidi::LRO
                | crate::text::bidi::RLO => depth += 1,
                crate::text::bidi::PDI | crate::text::bidi::PDF => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                }
                '\n' => return None,
                _ if depth > 0 => {}
                _ => return runs.iter().find(|r| r.range.contains(&here)).map(|r| r.rtl),
            }
        }
        None
    }

    /// The line's runs, edge spacers and atomic inlines in visual order with their positions.
    /// The fourth value is the horizontal extent of every cluster of the line, in logical order.
    fn placed_items_bidi(
        &self,
        line_index: usize,
    ) -> (
        Vec<PlacedRun>,
        Vec<AtomicPlacement>,
        Vec<EdgePlacement>,
        Vec<ClusterExtent>,
    ) {
        let (mut out, mut atomics, mut edges, mut extents) = (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::<ClusterExtent>::new(),
        );
        let (Some(line), Some(fl)) = (self.layout.get(line_index), self.lines.get(line_index))
        else {
            return (out, atomics, edges, extents);
        };
        let shift = fl.baseline - line.metrics().baseline;
        // The hanging white space of a right-to-left paragraph sits left of the aligned content.
        let w = self.snap.widths(fl.units.clone());
        let mut pen = fl.inline_start + fl.offset;
        if fl.rtl && fl.justify == 0.0 {
            pen -= w.snapped_full - w.snapped;
        }
        let units: &[super::snap::ClusterAdvance] =
            &self.snap.clusters[fl.units.start.min(self.snap.clusters.len())
                ..fl.units.end.min(self.snap.clusters.len())];
        // Parley's runs, in its visual order, and the sizes of the line's atomic inlines.
        let mut runs: Vec<BidiRun<'_>> = Vec::new();
        let mut boxes: Vec<(u64, f32, f32, f32)> = Vec::new();
        for item in line.items() {
            match item {
                PositionedLayoutItem::GlyphRun(gr) => {
                    let (span, owner) = self.owner_of(gr.style().brush);
                    let run_range = gr.run().text_range();
                    let range = match self.source.spans.get(span as usize) {
                        Some(s) => {
                            run_range.start.max(s.range.start)..run_range.end.min(s.range.end)
                        }
                        None => run_range,
                    };
                    let clusters =
                        self.glyph_clusters(*gr.run(), &range, gr.glyphs().count(), true);
                    runs.push(BidiRun {
                        rtl: gr.run().is_rtl(),
                        span,
                        owner,
                        range,
                        clusters,
                        gr,
                    });
                }
                PositionedLayoutItem::InlineBox(b) if b.kind == parley::InlineBoxKind::InFlow => {
                    boxes.push((b.id, b.width, b.height, b.y + shift))
                }
                PositionedLayoutItem::InlineBox(_) => {}
            }
        }
        // The inline boxes with an edge on this line: their edges and the units of their content.
        // An edge of a box with content sits at the visual end of that content (below); an edge of
        // a box without content, and an atomic inline, is a neutral object placed by its
        // neighbours.
        struct EdgeBox {
            start: Option<usize>,
            end: Option<usize>,
            /// The content units, inclusive (the line's first or last unit when the box goes on).
            first: usize,
            last: usize,
            rtl: bool,
            has_content: bool,
        }
        let line_units = fl.units.start..fl.units.end.min(self.snap.clusters.len());
        let edge_item_rtl = |unit: usize| -> bool {
            let nth = self.snap.clusters[..unit]
                .iter()
                .filter(|x| matches!(x.kind, UnitKind::Edge { .. }))
                .count();
            self.source
                .items
                .iter()
                .filter(|i| matches!(i.kind, super::source::InlineItemKind::EdgeSpacer { .. }))
                .nth(nth)
                .is_some_and(|i| i.embedding_rtl)
        };
        let mut edge_boxes: Vec<EdgeBox> = Vec::new();
        {
            let mut open: Vec<(usize, u64, usize)> = Vec::new(); // (unit, owner, box index)
            for unit in line_units.clone() {
                let UnitKind::Edge { start } = self.snap.clusters[unit].kind else {
                    continue;
                };
                let owner = self.edge_owner(unit);
                if start {
                    edge_boxes.push(EdgeBox {
                        start: Some(unit),
                        end: None,
                        first: unit + 1,
                        last: line_units.end.saturating_sub(1),
                        rtl: edge_item_rtl(unit),
                        has_content: false,
                    });
                    open.push((unit, owner, edge_boxes.len() - 1));
                } else if let Some(pos) = open.iter().rposition(|o| o.1 == owner) {
                    let (_, _, bi) = open.remove(pos);
                    edge_boxes[bi].end = Some(unit);
                    edge_boxes[bi].last = unit.saturating_sub(1);
                } else {
                    edge_boxes.push(EdgeBox {
                        start: None,
                        end: Some(unit),
                        first: line_units.start,
                        last: unit.saturating_sub(1),
                        rtl: edge_item_rtl(unit),
                        has_content: false,
                    });
                }
            }
            for b in &mut edge_boxes {
                b.has_content = (b.first..=b.last.max(b.first))
                    .filter(|j| line_units.contains(j) && *j >= b.first && *j <= b.last)
                    .any(|j| {
                        let u = &self.snap.clusters[j];
                        match u.kind {
                            UnitKind::Atomic(_) => true,
                            UnitKind::Text => self.control_at(u.start).is_none(),
                            UnitKind::Edge { .. } => false,
                        }
                    });
            }
        }
        let anchored_edge = |unit: usize| -> bool {
            !edge_boxes
                .iter()
                .any(|b| b.has_content && (b.start == Some(unit) || b.end == Some(unit)))
        };
        // The neutral objects of the line, in logical order.
        let mut neutrals: Vec<Neutral> = Vec::new();
        for (k, u) in units.iter().enumerate() {
            let unit = fl.units.start + k;
            match u.kind {
                UnitKind::Edge { start } if anchored_edge(unit) => {
                    neutrals.push(Neutral {
                        unit,
                        at: u.start,
                        rtl: edge_item_rtl(unit),
                        kind: NeutralKind::Edge {
                            start,
                            width: u.snapped,
                        },
                    });
                }
                UnitKind::Edge { .. } => {}
                UnitKind::Atomic(id) => {
                    let (width, height, y) = boxes
                        .iter()
                        .find(|b| b.0 == id)
                        .map(|b| (b.1, b.2, b.3))
                        .unwrap_or((u.snapped, 0.0, fl.top));
                    let rtl = self
                        .source
                        .items
                        .iter()
                        .find(|i| {
                            i.kind == super::source::InlineItemKind::AtomicInline && i.owner == id
                        })
                        .is_some_and(|i| i.embedding_rtl);
                    neutrals.push(Neutral {
                        unit,
                        at: u.start,
                        rtl,
                        kind: NeutralKind::Atomic {
                            item: id,
                            width,
                            height: super::scratch::atomic_height(height),
                            y,
                        },
                    });
                }
                UnitKind::Text => {}
            }
        }
        // An empty line (a paragraph of only edges or atomics) can report an out-of-range text range.
        let tlen = self.source.text.len();
        let line_end = fl.text_range.end.min(tlen);
        let line_start = fl.text_range.start.min(line_end);
        let run_at = |byte: usize| runs.iter().position(|r| r.range.contains(&byte));
        let mut anchors: Vec<Anchor> = Vec::with_capacity(neutrals.len());
        let mut dirs: Vec<bool> = Vec::with_capacity(neutrals.len());
        for n in &neutrals {
            let nat = n.at.min(tlen);
            let p = self.neighbour_before(&runs, nat, line_start);
            let q = self.neighbour_after(&runs, nat, line_end);
            let d = match (p, q) {
                (Some(a), Some(b)) if a == b => a,
                _ => n.rtl,
            };
            dirs.push(d);
            let prev_run = if nat > line_start {
                self.source.text[..nat]
                    .char_indices()
                    .rev()
                    .find(|(_, c)| !crate::text::bidi::is_bidi_control(*c))
                    .and_then(|(i, _)| run_at(i))
            } else {
                None
            };
            let next_run = if nat < line_end {
                self.source.text[nat..line_end]
                    .char_indices()
                    .find(|(_, c)| !crate::text::bidi::is_bidi_control(*c) && *c != '\n')
                    .and_then(|(i, _)| run_at(nat + i))
            } else {
                None
            };
            let a = match (prev_run, next_run, p, q) {
                (Some(pr), Some(nr), _, _)
                    if pr == nr
                        && !runs[pr].clusters.is_empty()
                        && runs[pr].range.start < nat
                        && nat < runs[pr].range.end =>
                {
                    Anchor::Inside(pr)
                }
                (Some(pr), _, Some(pp), _) if pp == d => {
                    if d {
                        Anchor::Pre(pr)
                    } else {
                        Anchor::Post(pr)
                    }
                }
                (_, Some(nr), _, Some(qq)) if qq == d => {
                    if d {
                        Anchor::Post(nr)
                    } else {
                        Anchor::Pre(nr)
                    }
                }
                _ => {
                    // The object is alone at the logical start or end of the line (or of its
                    // embedding) in direction `d`: at the matching visual edge.
                    let at_start = prev_run.is_none() || p.is_none();
                    match (at_start, d) {
                        (true, false) | (false, true) => Anchor::Left,
                        _ => Anchor::Right,
                    }
                }
            };
            anchors.push(a);
        }
        // UAX #9 L1: the whitespace (and isolate controls) at the end of the line goes at the
        // end side of the paragraph, whatever level the resolver gave it (Parley resolves a
        // paragraph at a time and does not apply L1 per line). `trail` is the byte where that
        // sequence starts, when it holds a real white space character.
        let trail: Option<usize> = {
            let text = &self.source.text;
            let end = if text[line_start..line_end].ends_with('\n') {
                line_end - 1
            } else {
                line_end
            };
            let mut at = end;
            let mut has_space = false;
            for (i, c) in text[line_start..end].char_indices().rev() {
                if c.is_whitespace() && c != '\n' {
                    has_space = true;
                } else if !crate::text::bidi::is_bidi_control(c) {
                    break;
                }
                at = line_start + i;
            }
            (has_space && at < end).then_some(at)
        };
        // The visual sequence.
        let mut seq: Vec<Slot> = Vec::new();
        let order = |idx: &mut Vec<usize>, d: bool| {
            idx.sort_by_key(|&i| neutrals[i].unit);
            if d {
                idx.reverse();
            }
        };
        let group = |pick: &dyn Fn(Anchor) -> bool| -> (Vec<usize>, bool) {
            let idx: Vec<usize> = (0..neutrals.len()).filter(|&i| pick(anchors[i])).collect();
            let d = idx.first().is_some_and(|&i| dirs[i]);
            (idx, d)
        };
        let (mut left, d) = group(&|a| matches!(a, Anchor::Left));
        order(&mut left, d);
        seq.extend(left.into_iter().map(Slot::Neutral));
        for (ri, r) in runs.iter().enumerate() {
            let (mut pre, d) = group(&|a| matches!(a, Anchor::Pre(x) if x == ri));
            order(&mut pre, d);
            seq.extend(pre.into_iter().map(Slot::Neutral));
            // The run, split where a neutral object sits inside it.
            let mut cuts: Vec<usize> = (0..neutrals.len())
                .filter(|&i| matches!(anchors[i], Anchor::Inside(x) if x == ri))
                .map(|i| neutrals[i].at)
                .collect();
            if let Some(t) = trail
                && r.range.start < t
                && t < r.range.end
                && !r.clusters.is_empty()
            {
                cuts.push(t);
            }
            cuts.sort_unstable();
            cuts.dedup();
            let mut bounds = vec![r.range.start];
            bounds.extend(cuts.iter().copied());
            bounds.push(r.range.end);
            let parts: Vec<Range<usize>> = bounds.windows(2).map(|b| b[0]..b[1]).collect();
            let ordered: Vec<usize> = if r.rtl {
                (0..parts.len()).rev().collect()
            } else {
                (0..parts.len()).collect()
            };
            for (k, &pi) in ordered.iter().enumerate() {
                if k > 0 {
                    // The cut between the previous part (in visual order) and this one.
                    let at = if r.rtl {
                        parts[pi].end
                    } else {
                        parts[pi].start
                    };
                    let (mut mid, d) = group(&|a| matches!(a, Anchor::Inside(x) if x == ri));
                    mid.retain(|&i| neutrals[i].at == at);
                    order(&mut mid, d);
                    seq.extend(mid.into_iter().map(Slot::Neutral));
                }
                let bytes = (parts.len() > 1).then(|| parts[pi].clone());
                seq.push(Slot::Run { run: ri, bytes });
            }
            let (mut post, d) = group(&|a| matches!(a, Anchor::Post(x) if x == ri));
            order(&mut post, d);
            seq.extend(post.into_iter().map(Slot::Neutral));
        }
        let (mut right, d) = group(&|a| matches!(a, Anchor::Right));
        order(&mut right, d);
        seq.extend(right.into_iter().map(Slot::Neutral));
        // L1: move the trailing whitespace parts to the end side of the paragraph.
        if let Some(t) = trail {
            let part_start = |slot: &Slot| -> Option<usize> {
                match slot {
                    Slot::Run { run, bytes } => {
                        Some(bytes.as_ref().map_or(runs[*run].range.start, |b| b.start))
                    }
                    Slot::Neutral(_) => None,
                }
            };
            let (mut tail, mut rest): (Vec<Slot>, Vec<Slot>) = (Vec::new(), Vec::new());
            for slot in seq {
                if part_start(&slot).is_some_and(|b| b >= t) {
                    tail.push(slot)
                } else {
                    rest.push(slot)
                }
            }
            // Logical order of the moved parts, then the end side's reading direction.
            tail.sort_by_key(|sl| part_start(sl).unwrap_or(0));
            if fl.rtl {
                tail.reverse();
                tail.extend(rest);
                seq = tail;
            } else {
                rest.extend(tail);
                seq = rest;
            }
        }
        // The edges of boxes with content: at the visual ends of the content, innermost box first
        // (so that an outer edge lies outside an inner one). The start edge is on the left in a
        // left-to-right embedding, on the right in a right-to-left one.
        let slot_units = |slot: &Slot, neutrals: &[Neutral]| -> Option<(usize, usize)> {
            match slot {
                Slot::Neutral(i) => Some((neutrals[*i].unit, neutrals[*i].unit)),
                Slot::Run { run, bytes } => {
                    let range = bytes.clone().unwrap_or_else(|| runs[*run].range.clone());
                    let mut lo_hi: Option<(usize, usize)> = None;
                    for j in line_units.clone() {
                        let u = &self.snap.clusters[j];
                        if u.kind == UnitKind::Text
                            && range.contains(&u.start)
                            && self.control_at(u.start).is_none()
                        {
                            lo_hi = Some(lo_hi.map_or((j, j), |(a, b)| (a.min(j), b.max(j))));
                        }
                    }
                    lo_hi
                }
            }
        };
        let mut order_boxes: Vec<usize> = (0..edge_boxes.len())
            .filter(|&i| edge_boxes[i].has_content)
            .collect();
        order_boxes.sort_by_key(|&i| edge_boxes[i].last.saturating_sub(edge_boxes[i].first));
        for bi in order_boxes {
            let (first, last, rtl) = (
                edge_boxes[bi].first,
                edge_boxes[bi].last,
                edge_boxes[bi].rtl,
            );
            let (mut lo, mut hi): (Option<usize>, Option<usize>) = (None, None);
            for (si, slot) in seq.iter().enumerate() {
                if let Some((a, b)) = slot_units(slot, &neutrals)
                    && a <= last
                    && b >= first
                {
                    lo.get_or_insert(si);
                    hi = Some(si);
                }
            }
            let (Some(lo), Some(hi)) = (lo, hi) else {
                continue;
            };
            let make = |unit: usize, start: bool, neutrals: &mut Vec<Neutral>| {
                neutrals.push(Neutral {
                    unit,
                    at: self.snap.clusters[unit].start,
                    rtl,
                    kind: NeutralKind::Edge {
                        start,
                        width: self.snap.clusters[unit].snapped,
                    },
                });
                Slot::Neutral(neutrals.len() - 1)
            };
            // (unit, is start, goes after the content)
            let mut edges_to_add: Vec<(usize, bool, bool)> = Vec::new();
            if let Some(u) = edge_boxes[bi].start {
                edges_to_add.push((u, true, rtl));
            }
            if let Some(u) = edge_boxes[bi].end {
                edges_to_add.push((u, false, !rtl));
            }
            // The edge after the content first: it does not move the index of the one before.
            edges_to_add.sort_by_key(|e| !e.2);
            for (unit, start, after) in edges_to_add {
                let slot = make(unit, start, &mut neutrals);
                if after {
                    seq.insert(hi + 1, slot);
                } else {
                    seq.insert(lo, slot);
                }
            }
        }
        // Place.
        let mut bytes_x: Vec<(usize, f32, f32, bool)> = Vec::new();
        for slot in seq {
            match slot {
                Slot::Neutral(i) => {
                    let n = neutrals[i];
                    match n.kind {
                        NeutralKind::Edge { start, width } => {
                            let owner = self.edge_owner(n.unit);
                            edges.push(EdgePlacement {
                                owner,
                                start,
                                x: pen,
                                width,
                            });
                            pen += width;
                        }
                        NeutralKind::Atomic {
                            item,
                            width,
                            height,
                            y,
                        } => {
                            atomics.push(AtomicPlacement {
                                item,
                                x: pen,
                                y,
                                width,
                                height,
                            });
                            pen += width;
                        }
                    }
                }
                Slot::Run { run: ri, bytes } => {
                    let r = &runs[ri];
                    let run = r.gr.run();
                    let font = run.font();
                    let face = FaceKey::of(run);
                    let range = bytes.clone().unwrap_or_else(|| r.range.clone());
                    let baseline = r.gr.baseline() + shift;
                    let x = pen;
                    let mut glyphs: Vec<GlyphView> = Vec::new();
                    let mut last_justified: Option<usize> = None;
                    for (i, g) in r.gr.glyphs().enumerate() {
                        let gc = r.clusters.get(i);
                        if let (Some(b), Some(gc)) = (&bytes, gc)
                            && !b.contains(&gc.byte)
                        {
                            continue;
                        }
                        let mut advance = gc
                            .and_then(|c| c.tab)
                            .unwrap_or_else(|| self.policy.snap(face, g.id, g.advance));
                        if fl.justify > 0.0
                            && let Some(gc) = gc
                            && gc.byte < fl.justify_end
                            && last_justified != Some(gc.byte)
                            && matches!(
                                self.source
                                    .text
                                    .get(gc.byte..)
                                    .and_then(|t| t.chars().next()),
                                Some(' ') | Some('\u{a0}')
                            )
                        {
                            advance += fl.justify;
                            last_justified = Some(gc.byte);
                        }
                        if let Some(gc) = gc {
                            bytes_x.push((gc.byte, pen, pen + advance, r.rtl));
                        }
                        glyphs.push(GlyphView {
                            id: g.id,
                            x: pen + g.x,
                            y: baseline + g.y,
                            advance,
                            upright: false,
                            scale_x: 1.0,
                        });
                        pen += advance;
                    }
                    let metrics = run.font_metrics();
                    let synthesis = run.synthesis();
                    let (no_synth_weight, no_synth_style) = self
                        .source
                        .spans
                        .get(r.span as usize)
                        .map_or((false, false), |s| {
                            (
                                s.props.font_settings.no_synthesis_weight,
                                s.props.font_settings.no_synthesis_style,
                            )
                        });
                    out.push(PlacedRun {
                        span: r.span,
                        owner: r.owner,
                        x,
                        width: pen - x,
                        text_range: range,
                        baseline,
                        ascent: metrics.ascent.round(),
                        descent: metrics.descent.round(),
                        glyphs,
                        font: self
                            .blobs
                            .iter()
                            .find(|b| {
                                b.id.0
                                    == crate::text::fonts::environment::published_blob_id(
                                        font.data.id(),
                                    )
                            })
                            .map(|blob| FontRef {
                                blob: blob.clone(),
                                index: font.index,
                                coords: run
                                    .normalized_coords()
                                    .iter()
                                    .map(|c| c.to_bits())
                                    .collect(),
                                embolden: synthesis.embolden() && !no_synth_weight,
                                oblique_angle_deg: synthesis.skew().filter(|_| !no_synth_style),
                            }),
                        size_px: run.font_size(),
                        bidi_level: u8::from(r.rtl),
                        hyphen: false,
                    });
                }
            }
        }
        // Cluster extents in logical order: one per text unit; a unit without glyphs of its own
        // (the rest of a ligature, a zero-width control) is an empty extent at the edge of the
        // cluster before it.
        let mut by_byte: std::collections::BTreeMap<usize, (f32, f32, bool)> =
            std::collections::BTreeMap::new();
        for (b, x0, x1, rtl) in bytes_x {
            let e = by_byte.entry(b).or_insert((x0, x1, rtl));
            e.0 = e.0.min(x0);
            e.1 = e.1.max(x1);
        }
        let mut prev_trail: Option<f32> = None;
        for u in units.iter().filter(|u| u.kind == UnitKind::Text) {
            let e = match by_byte.get(&u.start) {
                Some(&(x0, x1, rtl)) => ClusterExtent {
                    start: u.start,
                    end: u.end,
                    x0,
                    x1,
                    rtl,
                },
                None => {
                    let at = prev_trail.unwrap_or(fl.inline_start + fl.offset);
                    ClusterExtent {
                        start: u.start,
                        end: u.end,
                        x0: at,
                        x1: at,
                        rtl: false,
                    }
                }
            };
            prev_trail = Some(e.trail());
            extents.push(e);
        }
        (out, atomics, edges, extents)
    }

    /// The horizontal extent of every grapheme cluster of a line in logical order, from the
    /// positions the glyphs are painted at. UBA controls are included (zero width).
    pub fn cluster_extents(&self, line_index: usize) -> Vec<ClusterExtent> {
        if self.bidi
            && self
                .extras
                .lines
                .get(line_index)
                .is_none_or(|e| e.hidden_range.is_none())
        {
            return self.placed_items_bidi(line_index).3;
        }
        let Some(fl) = self.lines.get(line_index) else {
            return Vec::new();
        };
        let mut pen = fl.inline_start + fl.offset;
        let mut out = Vec::new();
        for u in &self.snap.clusters[fl.units.start.min(self.snap.clusters.len())
            ..fl.units.end.min(self.snap.clusters.len())]
        {
            if u.kind == UnitKind::Text {
                out.push(ClusterExtent {
                    start: u.start,
                    end: u.end,
                    x0: pen,
                    x1: pen + u.snapped,
                    rtl: false,
                });
            }
            pen += u.snapped;
        }
        out
    }

    /// The horizontal intervals (paragraph space, left to right, merged where they touch) that
    /// the clusters of `bytes` cover on a line: one for a left-to-right range, several where the
    /// range crosses direction changes. Controls and clusters outside the range are not covered.
    pub fn selection_spans(&self, line_index: usize, bytes: Range<usize>) -> Vec<(f32, f32)> {
        let mut spans: Vec<(f32, f32)> = self
            .cluster_extents(line_index)
            .into_iter()
            .filter(|e| {
                e.start >= bytes.start
                    && e.end <= bytes.end
                    && e.end > e.start
                    && self.control_at(e.start).is_none()
            })
            .map(|e| (e.x0, e.x1))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f32, f32)> = Vec::new();
        for s in spans {
            match merged.last_mut() {
                Some(last) if s.0 <= last.1 + 0.01 => last.1 = last.1.max(s.1),
                _ => merged.push(s),
            }
        }
        merged
    }
}
