//! owner: 009-text-inline — created by 001 slice (R-28)
//!
//! `ParagraphSource` and shaping (008 `ifc-interface.md` §2, §5.1 `shape`; 009
//! `text-services.md` §3). The source holds already-processed text (white-space processing is
//! done by the inline behaviour, `ui4me-layout/src/ifc/inline/builder.rs`) split into
//! contiguous leaf spans, each with its computed text style and a caller-defined owner tag.
//! Shaping is Parley's (font matching over the pinned collection, bidi, shaping); the result
//! is immutable and shared.

use parley::{
    FontFamily, InlineBox, InlineBoxKind, Layout, LineHeight, PositionedLayoutItem, StyleProperty,
    VerticalAlign, WhiteSpaceCollapse,
};
use std::ops::Range;
use std::sync::Arc;
use ui4me_engine_types::ids::DocumentId;
use ui4me_engine_types::text_metrics::FontSpec;

use super::snap::{AdvancePolicy, SnapTable};
use crate::text::diagnostics::TextDiagnosticKind;
use crate::text::fonts::TextServices;
use crate::text::metrics::font_properties;

/// Caret affinity at a boundary shared by two positions (data-model `TextPosition`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Affinity {
    Upstream,
    #[default]
    Downstream,
}

/// A DOM text position: node, UTF-16 code-unit offset (OD-22) and affinity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextPosition {
    pub node: ui4me_engine_types::ids::NodeId,
    pub offset: u32,
    pub affinity: Affinity,
}

/// A text control's value and its IME preedit range (R-39). The value already contains any
/// preedit text; `preedit_range` (UTF-16 offsets into the value) is only used for the preedit
/// decoration and caret placement, never to insert text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlText {
    pub text: String,
    pub preedit_range: Option<Range<u32>>,
}

/// Kind of one inline item (data-model `ParagraphSource.items`). Atomics, floats and out-of-flow
/// placeholders are identified by their owner tag. Floats and out-of-flow placeholders are zero
/// size boxes at an empty item range (`start..start`) of the processed text; they add no
/// characters (decision D-L009r1-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InlineItemKind {
    Text,
    AtomicInline,
    Float,
    OutOfFlow,
    /// Padding/border/margin of an inline box as an in-flow spacer (G-TEXT-EDGES).
    EdgeSpacer {
        start: bool,
    },
    Tab,
    ForcedBreak,
    BidiControl,
    Generated,
    /// `<wbr>` (U+200B).
    Wbr,
}

/// CSS `vertical-align` of an atomic inline (CSS 2.1 §10.8.1; CSS Inline 3). Upstream's own type
/// never appears in the interface (AD-33).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VerticalAlignSpec {
    #[default]
    Baseline,
    Sub,
    Super,
    TextTop,
    TextBottom,
    Middle,
    /// Line-relative.
    Top,
    /// Line-relative.
    Bottom,
    /// `<length>` already resolved to px (percentages resolve against `line-height` upstream).
    Length(f32),
}

/// One inline item over a byte range of the processed text. Atomic inlines and edge spacers
/// occupy no text: their `range` is empty and sits at the byte offset where they appear.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineItem {
    pub kind: InlineItemKind,
    pub range: Range<usize>,
    pub owner: u64,
    /// Edge spacers: the inline extent in px (padding + border + margin of that edge).
    pub extent: f32,
    /// Atomic inlines: `vertical-align`.
    pub valign: VerticalAlignSpec,
    /// Edge spacers and atomic inlines are neutral objects for the bidi algorithm: the direction
    /// of the embedding they sit in (the nearest enclosing `embed`, `isolate`, `bidi-override`,
    /// `isolate-override` or `plaintext` box, else the paragraph's), which orders them against the
    /// text next to them when it differs on the two sides (UAX #9 N2).
    pub embedding_rtl: bool,
}

impl InlineItem {
    pub fn new(kind: InlineItemKind, range: Range<usize>, owner: u64) -> Self {
        InlineItem {
            kind,
            range,
            owner,
            extent: 0.0,
            valign: VerticalAlignSpec::Baseline,
            embedding_rtl: false,
        }
    }
}

/// `ParagraphKey = (inline root slot, content revision, style epoch, font epoch)`
/// (ifc-interface.md section 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParagraphKey {
    pub root_slot: u64,
    pub content_revision: u64,
    pub style_epoch: u64,
    pub font_epoch: u64,
}

/// Margin-box min/max-content inline contribution of one atomic inline (section 5.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicContribution {
    pub item: u64,
    pub min_content: f32,
    pub max_content: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SynthKind {
    Ellipsis,
    Hyphen,
    String(String),
}

/// Request for `shape_synthetic` (section 5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynthRequest {
    pub kind: SynthKind,
    pub at_cluster: usize,
}

/// An immutable synthesized run with no DOM offsets (ellipsis, hyphen, `text-overflow` string).
#[derive(Clone, Debug, PartialEq)]
pub struct SynthRun {
    pub kind: SynthKind,
    pub text: String,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub anchor_cluster: usize,
    /// The shaped glyphs (pen x from the run start, whole-pixel advances) and their font, for
    /// painting: a synthesized run has no source text to draw from.
    pub glyphs: Vec<SynthGlyph>,
    pub font: Option<SynthFont>,
}

/// One glyph of a synthesized run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SynthGlyph {
    pub id: u32,
    /// Offset of the pen from the start of the run, plus the shaper's own x offset.
    pub x: f32,
    pub y: f32,
    pub advance: f32,
}

/// The font a synthesized run was shaped with (resolved to a `FontRef` against the blobs of the
/// paragraph it is placed in).
#[derive(Clone, Debug, PartialEq)]
pub struct SynthFont {
    /// 007's font blob id.
    pub blob: u64,
    pub index: u32,
    pub coords: Vec<i16>,
    pub embolden: bool,
    pub oblique_angle_deg: Option<f32>,
    pub size_px: f32,
}

/// Parley brush: the index of the source span a glyph belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpanBrush(pub u32);

/// Computed `line-height` (FR-017: `normal`, `<number>`, `<length>`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeightSpec {
    Normal,
    Number(f32),
    Px(f32),
}

/// The text properties of one span (FR-017 text group; colour is a paint property and is not
/// needed for shaping, so it is not here).
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font: FontSpec,
    pub font_size: f32,
    pub line_height: LineHeightSpec,
}

/// `text-decoration-style` (CSS Text Decoration 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecorationLineStyle {
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

/// `text-decoration-thickness`, percentages and calc already resolved against the decorating box's
/// font size by the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecorationThickness {
    /// UA choice: the font's own underline thickness, whole pixels, at least one.
    Auto,
    FromFont,
    Px(f32),
}

/// `text-underline-position` as far as the horizontal text of this engine uses it (CSS Text
/// Decoration 4): the underline sits near the alphabetic baseline (`auto`), at the font's own
/// underline position (`from-font`) or under the font's descent (`under`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnderlinePos {
    #[default]
    Auto,
    FromFont,
    Under,
}

/// One decorating box's text decoration in effect over a span (CSS Text Decoration 3 section 2:
/// the decorations of a box propagate to its in-flow inline descendants; each decorating box draws
/// its own lines with its own style, colour and font metrics).
#[derive(Clone, Debug, PartialEq)]
pub struct DecorationLayer {
    /// The decorating box's owner tag.
    pub owner: u64,
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    pub style: DecorationLineStyle,
    // The colour is paint-only: the scene resolves the decorating box's `text-decoration-color`
    // from its computed style (L-PERF-b3), so a colour change neither reshapes nor relays out.
    pub thickness: DecorationThickness,
    /// `text-underline-offset` in px (a percentage is of 1em of the decorating box); `None` is `auto`.
    pub underline_offset: Option<f32>,
    pub underline_position: UnderlinePos,
    /// The decorating box's font: the line position and the auto thickness come from its first
    /// available font, not from the text it decorates.
    pub font: TextStyle,
}

/// `white-space-collapse` as the text services see it: the text is already processed by the inline
/// behaviour; the value decides how Parley treats spaces at the end of a line (they hang, except
/// under `break-spaces`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SpanCollapse {
    #[default]
    Collapse,
    /// Spaces and tabs are preserved (`pre`, `pre-wrap`, `pre-line` after processing).
    Preserve,
    BreakSpaces,
}

/// `overflow-wrap` (CSS Text 3 section 5.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WrapOverflow {
    #[default]
    Normal,
    BreakWord,
    Anywhere,
}

/// `word-break` (`break-word` is off in the profile).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WordBreakKind {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
}

/// `hyphens` (CSS Text 3 section 6.1): the initial value is `manual` (soft hyphens only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HyphensKind {
    None,
    #[default]
    Manual,
    Auto,
}

/// `line-break`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineBreakKind {
    #[default]
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

/// `tab-size`: a number of space advances (of the span's font, with the span's spacing) or px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabSpec {
    Spaces(f32),
    Px(f32),
}

impl Default for TabSpec {
    fn default() -> Self {
        TabSpec::Spaces(8.0)
    }
}

/// Per-span text properties beyond the shaping style: values that steer painting and line
/// breaking of one span without changing which font shapes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpanProps {
    /// The decorations in effect over the span, outermost decorating box first.
    pub decorations: Arc<[DecorationLayer]>,
    /// The glyph orientation of the span in a vertical line (015 WM-1b; `vertical.rs`).
    pub orientation: super::vertical::GlyphOrientation,
    /// `text-combine-upright: all` in a vertical line (015 WM-1b): the span's text is one upright unit one
    /// em long, set horizontally (compressed to fit).
    pub combine_upright: bool,
    pub collapse: SpanCollapse,
    /// `text-wrap-mode: nowrap`: no soft wrap opportunity inside the span.
    pub nowrap: bool,
    pub overflow_wrap: WrapOverflow,
    pub word_break: WordBreakKind,
    pub line_break: LineBreakKind,
    /// `hyphens`, and `hyphenate-character` (`None` = `auto`) (015 TXT-3).
    pub hyphens: HyphensKind,
    pub hyphenate_character: Option<String>,
    /// `letter-spacing` in px.
    pub letter_spacing: f32,
    /// `word-spacing` in px.
    pub word_spacing: f32,
    pub tab_size: TabSpec,
    /// `vertical-align` of the span relative to the root's baseline, already combined with the
    /// inline boxes around the text by the caller (CSS 2.1 section 10.8.1).
    pub valign: VerticalAlignSpec,
    /// Variation settings, optical sizing and OpenType features (015 TXT-2).
    pub font_settings: crate::text::fonts::features::FontSettings,
}

/// One leaf span: `range` (UTF-8 bytes of `ParagraphSource::text`) and its owner tag (008 maps
/// it to the inline box / element the text belongs to).
#[derive(Clone, Debug, PartialEq)]
pub struct SourceSpan {
    pub range: Range<usize>,
    pub style: TextStyle,
    pub owner: u64,
    pub props: SpanProps,
}

/// One inline root's processed text (008 `ifc-interface.md` §2).
#[derive(Clone, Debug, PartialEq)]
pub struct ParagraphSource {
    pub document: DocumentId,
    pub text: String,
    /// The root's style: strut and first available font (CSS 2.1 §10.8.1).
    pub root: TextStyle,
    /// Contiguous, non-overlapping, covering `0..text.len()`, in text order.
    pub spans: Vec<SourceSpan>,
    /// Inline items over the text (empty for plain text; the builder fills them).
    pub items: Vec<InlineItem>,
    /// DOM offset segments (empty means no DOM mapping recorded); see `offset_map`.
    pub segments: Vec<super::offset_map::Segment>,
    /// The inline root's node and the document revision, for per-revision `SIZE_LIMIT` reports.
    pub root_node: Option<ui4me_engine_types::ids::NodeId>,
    pub dom_revision: u64,
    /// The paragraph's base direction (the root's `direction`; `Auto` for `unicode-bidi:
    /// plaintext` and `dir=auto`). A forced line break does not start a new bidi paragraph here
    /// (documented limit, `docs/SUPPORTED_FEATURES.md`).
    pub base_direction: crate::text::bidi::BaseDirection,
    /// With `base_direction: Auto` and several forced-break lines: every line's text is wrapped in
    /// `FSI .. PDI` (`isolate_bidi_paragraphs`) so that each line takes the direction of its own
    /// first strong character, and `text-align: start | end` resolves per line.
    pub isolated_paragraphs: bool,
    /// The direction a bidi paragraph with no strong character takes when there is no previous
    /// paragraph: the containing block's `direction` (UAX #9 P3 in CSS Text 3 `plaintext`).
    pub fallback_rtl: bool,
    /// The direction of the embedding the items pushed next sit in (see
    /// [`InlineItem::embedding_rtl`]); the builder moves it as it enters and leaves boxes.
    pub embedding_rtl: bool,
}

impl ParagraphSource {
    pub fn new(document: DocumentId, root: TextStyle) -> Self {
        ParagraphSource {
            document,
            text: String::new(),
            root,
            spans: Vec::new(),
            items: Vec::new(),
            segments: Vec::new(),
            root_node: None,
            dom_revision: 0,
            base_direction: crate::text::bidi::BaseDirection::Ltr,
            isolated_paragraphs: false,
            fallback_rtl: false,
            embedding_rtl: false,
        }
    }

    /// Insert a UBA control at byte `at` of the text (a char boundary): it joins the span that
    /// holds `at` (the last span at the end), and spans, items and later positions move up.
    fn insert_control_at(&mut self, at: usize, c: char) {
        let n = c.len_utf8();
        self.text.insert(at, c);
        let hit = self
            .spans
            .iter()
            .position(|s| s.range.start <= at && at < s.range.end)
            .or_else(|| self.spans.len().checked_sub(1));
        let owner = hit.map_or(0, |i| self.spans[i].owner);
        for (i, sp) in self.spans.iter_mut().enumerate() {
            if Some(i) == hit {
                sp.range.end += n;
            } else if sp.range.start >= at {
                sp.range.start += n;
                sp.range.end += n;
            }
        }
        for it in self.items.iter_mut() {
            if it.range.start >= at {
                it.range.start += n;
                it.range.end += n;
            }
        }
        let mut item = InlineItem::new(InlineItemKind::BidiControl, at..at + n, owner);
        item.embedding_rtl = self.embedding_rtl;
        self.items.push(item);
    }

    /// `unicode-bidi: plaintext` / `dir=auto` on a root with several forced-break lines: each line
    /// is a bidi paragraph of its own (UAX #9 P1), whose base direction is the direction of its
    /// first strong character (P2, P3). Parley resolves one paragraph per layout, so the lines are
    /// separated by wrapping each in `FSI .. PDI` and the base direction of the layout is left to
    /// right; [`ParagraphSource::isolated_paragraphs`] tells freezing to align and place each line
    /// by its own direction.
    pub fn isolate_bidi_paragraphs(&mut self) {
        if self.isolated_paragraphs || !self.text.contains('\n') {
            return;
        }
        let mut points: Vec<(usize, char)> = Vec::new();
        let mut start = 0usize;
        for (i, c) in self.text.char_indices() {
            if c == '\n' {
                points.push((start, crate::text::bidi::FSI));
                points.push((i, crate::text::bidi::PDI));
                start = i + 1;
            }
        }
        if start < self.text.len() {
            points.push((start, crate::text::bidi::FSI));
            points.push((self.text.len(), crate::text::bidi::PDI));
        }
        // From the end, so that earlier positions stay valid; at one position the PDI of a line
        // goes in before the FSI of the next.
        points.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then((b.1 == crate::text::bidi::FSI).cmp(&(a.1 == crate::text::bidi::FSI)))
        });
        for (at, c) in points {
            self.insert_control_at(at, c);
        }
        self.items.sort_by_key(|i| i.range.start);
        self.isolated_paragraphs = true;
    }

    /// A UBA control character (`bidi::controls_for`) at the current end of the text, styled like
    /// the span it joins. Non-DOM text: its bytes are listed by [`ParagraphSource::control_ranges`].
    pub fn push_bidi_control(&mut self, c: char, style: &TextStyle, owner: u64, props: &SpanProps) {
        debug_assert!(crate::text::bidi::is_bidi_control(c));
        let start = self.text.len();
        let mut buf = [0u8; 4];
        self.push_with(c.encode_utf8(&mut buf), style, owner, props);
        let mut item = InlineItem::new(InlineItemKind::BidiControl, start..self.text.len(), owner);
        item.embedding_rtl = self.embedding_rtl;
        self.items.push(item);
    }

    /// The byte ranges of the inserted UBA controls (they have no DOM text).
    pub fn control_ranges(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        self.items
            .iter()
            .filter(|i| i.kind == InlineItemKind::BidiControl)
            .map(|i| i.range.clone())
    }

    /// Whether the paragraph needs the bidirectional placement path: a right-to-left base
    /// direction, a UBA control, or right-to-left text.
    pub fn is_bidi(&self) -> bool {
        self.base_direction != crate::text::bidi::BaseDirection::Ltr
            || self
                .items
                .iter()
                .any(|i| i.kind == InlineItemKind::BidiControl)
            || self.text.chars().any(crate::text::bidi::is_strong_rtl)
    }

    /// Append processed text with its style and owner; adjacent pushes with an equal owner and
    /// style merge into one span.
    pub fn push(&mut self, text: &str, style: &TextStyle, owner: u64) {
        self.push_with(text, style, owner, &SpanProps::default());
    }

    /// [`ParagraphSource::push`] with the span's [`SpanProps`]; adjacent pushes merge only when
    /// owner, style and props are equal.
    pub fn push_with(&mut self, text: &str, style: &TextStyle, owner: u64, props: &SpanProps) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if let Some(last) = self.spans.last_mut()
            && last.owner == owner
            && last.style == *style
            && last.props == *props
            && last.range.end == start
        {
            last.range.end = end;
            return;
        }
        self.spans.push(SourceSpan {
            range: start..end,
            style: style.clone(),
            owner,
            props: props.clone(),
        });
    }

    /// An atomic inline (`inline-block`, replaced element, control) at the current end of the
    /// text; `owner` is the caller's tag (008 uses the box id) and keys `layout_atomic`.
    pub fn push_atomic(&mut self, owner: u64, valign: VerticalAlignSpec) {
        let at = self.text.len();
        let mut item = InlineItem::new(InlineItemKind::AtomicInline, at..at, owner);
        item.valign = valign;
        item.embedding_rtl = self.embedding_rtl;
        self.items.push(item);
    }

    /// A float (CSS 2.1 §9.5) met in the inline content: a zero-size placeholder at the current
    /// end of the text; the breaker yields `FloatEncountered` when the line reaches it. `owner`
    /// is the caller's tag (008 uses the box id).
    pub fn push_float(&mut self, owner: u64) {
        let at = self.text.len();
        self.items
            .push(InlineItem::new(InlineItemKind::Float, at..at, owner));
    }

    /// An out-of-flow box (`position: absolute | fixed`) met in the inline content: a zero-size
    /// placeholder whose position on its line is its static position (`OutOfFlowEncountered`).
    pub fn push_out_of_flow(&mut self, owner: u64) {
        let at = self.text.len();
        self.items
            .push(InlineItem::new(InlineItemKind::OutOfFlow, at..at, owner));
    }

    /// Whether the paragraph has content that makes a line box (text, an atomic inline or an
    /// inline box edge); floats and out-of-flow placeholders alone do not (CSS 2.1 §9.4.2).
    pub fn has_line_content(&self) -> bool {
        !self.text.is_empty()
            || self
                .items
                .iter()
                .any(|i| !matches!(i.kind, InlineItemKind::Float | InlineItemKind::OutOfFlow))
    }

    /// The start or end edge (padding + border + margin, px) of an inline box as an in-flow
    /// spacer (G-TEXT-EDGES).
    pub fn push_edge(&mut self, start: bool, extent: f32, owner: u64) {
        let at = self.text.len();
        let mut item = InlineItem::new(InlineItemKind::EdgeSpacer { start }, at..at, owner);
        item.extent = extent;
        item.embedding_rtl = self.embedding_rtl;
        self.items.push(item);
    }

    /// Whether `other` lays out like this paragraph: the same text, items and spans (ranges, shaping style and
    /// every span property that steers shaping or breaking), ignoring owner tags and the decorations (paint only).
    /// What a first-line variant that changes colours only looks like (015 TXT-5).
    pub fn same_layout(&self, other: &ParagraphSource) -> bool {
        self.text == other.text
            && self.root == other.root
            && self.items.len() == other.items.len()
            && self.items.iter().zip(&other.items).all(|(a, b)| {
                a.kind == b.kind
                    && a.range == b.range
                    && a.extent == b.extent
                    && a.valign == b.valign
            })
            && self.spans.len() == other.spans.len()
            && self.spans.iter().zip(&other.spans).all(|(a, b)| {
                a.range == b.range
                    && a.style == b.style
                    && SpanProps {
                        decorations: Default::default(),
                        ..a.props.clone()
                    } == SpanProps {
                        decorations: Default::default(),
                        ..b.props.clone()
                    }
            })
    }

    /// The paragraph whose text up to byte `cut` is `first` (a first-line variant) and the rest is `rest`: the first
    /// formatted line of a block with `::first-line` (015 TXT-5). Both were built from the same box subtree, so
    /// they differ in the spans' styles and owner tags (and in the case or form of the text, `text-transform`) only.
    /// `None` when the two do not line up (different text lengths, items or a cut that is not a character boundary):
    /// the caller then lays the paragraph out without the first-line style.
    pub fn mix_first_line(
        first: &ParagraphSource,
        rest: &ParagraphSource,
        cut: usize,
    ) -> Option<ParagraphSource> {
        if first.text.len() != rest.text.len()
            || !first.text.is_char_boundary(cut)
            || !rest.text.is_char_boundary(cut)
            || first.items.len() != rest.items.len()
            || first
                .items
                .iter()
                .zip(&rest.items)
                .any(|(a, b)| a.kind != b.kind || a.range != b.range)
        {
            return None;
        }
        let mut out = rest.clone();
        // The strut of every line is the root's (Parley has one per paragraph), but the first line's is the first-line
        // style's. When the two differ the root becomes a strut with no height: each line is then sized by its own
        // spans, which have the first-line style on the first line and the normal one after it, as the struts do
        // (they only differ from the spans on a line with no text of its own style).
        if first.root != rest.root {
            out.root.line_height = LineHeightSpec::Px(0.0);
        }
        out.text.clear();
        out.text.push_str(&first.text[..cut]);
        out.text.push_str(&rest.text[cut..]);
        out.spans.clear();
        for sp in &first.spans {
            if sp.range.start < cut {
                let mut sp = sp.clone();
                sp.range.end = sp.range.end.min(cut);
                out.spans.push(sp);
            }
        }
        for sp in &rest.spans {
            if sp.range.end > cut {
                let mut sp = sp.clone();
                sp.range.start = sp.range.start.max(cut);
                out.spans.push(sp);
            }
        }
        // Items at or after the cut belong to the later lines (their kinds and ranges are the same in both).
        for (i, it) in out.items.iter_mut().enumerate() {
            if it.range.start < cut {
                *it = first.items[i].clone();
            }
        }
        Some(out)
    }

    /// Whether the paragraph produces no line box content at all (no text, atomics or edges).
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.items.is_empty()
    }
}

/// A character that no available font could render (`.notdef` drawn).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissingGlyph {
    pub byte: usize,
    pub ch: char,
}

/// Immutable shaping result (`Arc`), shared across scratch layouts and epochs.
pub struct ShapedParagraph {
    pub(crate) layout: Layout<SpanBrush>,
    pub(crate) source: Arc<ParagraphSource>,
    pub(crate) missing: Vec<MissingGlyph>,
    pub(crate) blobs: Vec<ui4me_assets::fonts::FontBlob>,
    /// Features a span asks for that the font it was shaped with lacks and the reference browser
    /// synthesizes (small capitals, sub- and superscript): `(span index, tag)`; reported, not applied.
    pub(crate) synthesis_missing: Vec<(u32, crate::text::fonts::features::FontTag)>,
    /// The whole-pixel text metrics policy (`snap.rs`): face tables and per-cluster advances.
    pub(crate) policy: Arc<AdvancePolicy>,
    pub(crate) snap: Arc<SnapTable>,
    /// The shaped hyphen of each span that can end a line with one (015 TXT-3), by span index.
    pub(crate) hyphen_runs: Arc<Vec<Option<SynthRun>>>,
    /// Spans with `hyphens: auto` that cannot be hyphenated, with the reason (reported by the caller).
    pub(crate) hyphen_unsupported: Vec<(u32, super::hyphen::HyphenUnsupported)>,
    pub(crate) font_epoch: u64,
    /// The first-available-font metrics of every decorating box's font (see `decoration.rs`).
    pub(crate) deco_fonts: Arc<Vec<super::decoration::DecoFont>>,
    /// A slot the layout engine keeps its results for this paragraph in (L-PERF-b1): the shaped
    /// paragraph is reused across passes while its content is unchanged, and so are results that
    /// depend on nothing else. Opaque to this crate.
    layout_memo: std::sync::Mutex<Option<Box<dyn std::any::Any + Send>>>,
}

impl std::fmt::Debug for ShapedParagraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapedParagraph")
            .field("text", &self.source.text)
            .field("missing", &self.missing)
            .finish()
    }
}

/// Intrinsic inline sizes (008 `ifc-interface.md` §5.1 `content_widths`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentWidths {
    pub min: f32,
    pub max: f32,
}

impl ShapedParagraph {
    /// Shaped without a missing glyph and within the size limits: shaping it again records no diagnostic
    /// (what the content cache keeps; the layout's incremental host build may skip such a shape, 015 PERF-1).
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty()
            && self.source.text.len() < super::limits::MAX_TEXT_BYTES
            && self.source.spans.len() < super::limits::MAX_STYLES
    }

    /// The layout engine's memo slot of this paragraph (see the field).
    pub fn layout_memo(&self) -> &std::sync::Mutex<Option<Box<dyn std::any::Any + Send>>> {
        &self.layout_memo
    }

    pub fn source(&self) -> &ParagraphSource {
        &self.source
    }

    /// Font epoch this paragraph was shaped under.
    pub fn font_epoch(&self) -> u64 {
        self.font_epoch
    }

    /// Features a span asks for that its font lacks and the reference browser would synthesize
    /// (`(span index, tag)`, see `fonts::features::SYNTHESIZED_TAGS`).
    pub fn missing_synthesized_features(&self) -> &[(u32, crate::text::fonts::features::FontTag)] {
        &self.synthesis_missing
    }

    /// Spans (by index) whose `hyphens: auto` does nothing, and why (no `lang`, no patterns for the
    /// language, a bidirectional paragraph): `UNSUPPORTED_FEATURE`, never a silent fallback.
    pub fn hyphenation_unsupported(&self) -> &[(u32, super::hyphen::HyphenUnsupported)] {
        &self.hyphen_unsupported
    }

    /// Characters drawn with `.notdef`, in text order.
    pub fn missing_glyphs(&self) -> &[MissingGlyph] {
        &self.missing
    }

    /// Min-content and max-content inline sizes of the text alone, with the whole-pixel glyph
    /// advances of the text metrics policy (`snap.rs`). Atomic inlines count as zero.
    pub fn content_widths(&self) -> ContentWidths {
        self.content_widths_with(&[])
    }

    /// `content_widths(&ShapedParagraph, atomics)` (ifc-interface.md §5.1): the text combined
    /// with the margin-box contributions of its atomic inlines. An atomic inline is an
    /// unbreakable segment with soft wrap opportunities on both sides; `min` uses each atomic's
    /// min-content contribution and `max` its max-content contribution.
    pub fn content_widths_with(&self, atomics: &[AtomicContribution]) -> ContentWidths {
        let pick = |f: fn(&AtomicContribution) -> f32| {
            move |item: u64| {
                atomics
                    .iter()
                    .find(|a| a.item == item)
                    .map(f)
                    .unwrap_or(0.0)
            }
        };
        let min = self
            .snap
            .with_atomic_widths(pick(|a| a.min_content))
            .content_widths()
            .0;
        let max = self
            .snap
            .with_atomic_widths(pick(|a| a.max_content))
            .content_widths()
            .1;
        ContentWidths { min, max }
    }
}

fn item_kind_name(k: InlineItemKind) -> &'static str {
    match k {
        InlineItemKind::Text => "text",
        InlineItemKind::AtomicInline => "atomic-inline",
        InlineItemKind::Float => "float",
        InlineItemKind::OutOfFlow => "out-of-flow",
        InlineItemKind::EdgeSpacer { .. } => "edge-spacer",
        InlineItemKind::Tab => "tab",
        InlineItemKind::ForcedBreak => "forced-break",
        InlineItemKind::BidiControl => "bidi-control",
        InlineItemKind::Generated => "generated",
        InlineItemKind::Wbr => "wbr",
    }
}

fn parley_valign(v: VerticalAlignSpec) -> parley::VerticalAlign {
    use parley::VerticalAlign as V;
    match v {
        VerticalAlignSpec::Baseline => V::BASELINE,
        VerticalAlignSpec::Sub => V::SUB,
        VerticalAlignSpec::Super => V::SUPER,
        VerticalAlignSpec::TextTop => V::TEXT_TOP,
        VerticalAlignSpec::TextBottom => V::TEXT_BOTTOM,
        VerticalAlignSpec::Middle => V::MIDDLE,
        VerticalAlignSpec::Top => V::TOP,
        VerticalAlignSpec::Bottom => V::BOTTOM,
        VerticalAlignSpec::Length(l) => V::length(l),
    }
}

fn line_height(lh: LineHeightSpec) -> LineHeight {
    match lh {
        // `normal`: the font's ascent + descent + line gap (IF-01).
        LineHeightSpec::Normal => LineHeight::MetricsRelative(1.0),
        LineHeightSpec::Number(n) => LineHeight::FontSizeRelative(n),
        LineHeightSpec::Px(px) => LineHeight::Absolute(px),
    }
}

pub(crate) fn text_properties<'a>(
    services: &TextServices,
    style: &TextStyle,
    span: &SpanProps,
    is_root: bool,
) -> Vec<StyleProperty<'a, SpanBrush>> {
    let mut props = font_properties(&services.env, &style.font, style.font_size);
    props.push(StyleProperty::LineHeight(line_height(style.line_height)));
    // Text is already processed by the inline behaviour; Parley only decides how spaces at the end
    // of a line hang and where it may break.
    props.push(StyleProperty::WhiteSpaceCollapse(match span.collapse {
        SpanCollapse::Collapse => WhiteSpaceCollapse::Collapse,
        SpanCollapse::Preserve => WhiteSpaceCollapse::Preserve,
        SpanCollapse::BreakSpaces => WhiteSpaceCollapse::BreakSpaces,
    }));
    if span.nowrap {
        props.push(StyleProperty::TextWrapMode(parley::TextWrapMode::NoWrap));
    }
    if span.overflow_wrap != WrapOverflow::Normal {
        props.push(StyleProperty::OverflowWrap(match span.overflow_wrap {
            WrapOverflow::Normal => parley::OverflowWrap::Normal,
            WrapOverflow::BreakWord => parley::OverflowWrap::BreakWord,
            WrapOverflow::Anywhere => parley::OverflowWrap::Anywhere,
        }));
    }
    if span.word_break != WordBreakKind::Normal {
        props.push(StyleProperty::WordBreak(match span.word_break {
            WordBreakKind::Normal => parley::WordBreak::Normal,
            WordBreakKind::BreakAll => parley::WordBreak::BreakAll,
            WordBreakKind::KeepAll => parley::WordBreak::KeepAll,
        }));
    }
    // CSS `line-break: auto` is the UA's choice: the strict rules (Parley's default and the
    // reference browser's for Latin text).
    props.push(StyleProperty::LineBreak(match span.line_break {
        LineBreakKind::Auto | LineBreakKind::Strict => parley::LineBreak::Strict,
        LineBreakKind::Loose => parley::LineBreak::Loose,
        LineBreakKind::Normal => parley::LineBreak::Normal,
        LineBreakKind::Anywhere => parley::LineBreak::Anywhere,
    }));
    if span.valign != VerticalAlignSpec::Baseline {
        props.push(StyleProperty::VerticalAlign(parley_valign(span.valign)));
    }
    let slnt = if is_root {
        None
    } else {
        services.env.slant_axis_for(
            &services.env.resolve_families(&style.font.families),
            style.font.style,
        )
    };
    props.extend(crate::text::fonts::features::style_properties(
        &span.font_settings,
        style.font_size,
        !is_root && services.env.opsz_available(),
        slnt,
    ));
    if span.letter_spacing != 0.0 {
        props.push(StyleProperty::LetterSpacing(span.letter_spacing));
    }
    if span.word_spacing != 0.0 {
        props.push(StyleProperty::WordSpacing(span.word_spacing));
    }
    props
}

impl TextServices {
    /// Font matching, bidi analysis and shaping (008 `ifc-interface.md` §5.1). Characters no
    /// font covers are drawn as `.notdef`; the first one yields one `RESOURCE_PENDING`
    /// (missing glyph) diagnostic per document (FR-025).
    pub fn shape(&mut self, source: &ParagraphSource) -> Arc<ShapedParagraph> {
        // Content-addressed reuse across layout passes (L-PERF-b1): shaping is a pure function of the
        // source (modulo `dom_revision`, which only stamps diagnostics) and the font environment, so an
        // unchanged paragraph is not shaped again. Only clean results are kept (no missing glyph, no
        // diagnostic), so a hit never swallows a report.
        let hash = super::shaped::content_hash(source);
        if let Some(hit) = self.content_cache.get(hash, source, self.env.font_epoch()) {
            return hit;
        }
        let events = self.shape_diag_events;
        let shaped = self.shape_uncached(source);
        if shaped.missing.is_empty() && events == self.shape_diag_events {
            self.content_cache
                .insert(hash, source, shaped.clone(), self.env.font_epoch());
        }
        shaped
    }

    /// `shape` without the content cache.
    pub fn shape_uncached(&mut self, source: &ParagraphSource) -> Arc<ShapedParagraph> {
        // Limits are checked before any Parley call (FR-092, FR-096): Parley asserts on more than
        // u16::MAX styles.
        let limited = super::limits::apply_limits(source);
        if limited.styles_clamped {
            self.shape_diag_events += 1;
            self.text_diagnostics.record(
                source.document,
                source.root_node,
                TextDiagnosticKind::SizeLimit {
                    limit: "inline-styles",
                },
                source.dom_revision,
            );
        }
        if limited.text_truncated {
            self.shape_diag_events += 1;
            self.text_diagnostics.record(
                source.document,
                source.root_node,
                TextDiagnosticKind::SizeLimit {
                    limit: "paragraph-bytes",
                },
                source.dom_revision,
            );
        }
        let source = &limited.source;
        let mut unsupported_items: Vec<InlineItemKind> = Vec::new();
        let root_props = text_properties(self, &source.root, &SpanProps::default(), true);
        let span_props: Vec<_> = source
            .spans
            .iter()
            .map(|s| text_properties(self, &s.style, &s.props, false))
            .collect();
        // `quantize`: whole-pixel ascent and descent, floored half-leading above the baseline and a
        // whole-pixel `line-height: normal` (the text metrics policy, `snap.rs`).
        // A tab is shaped as a space (the same byte length, so every offset stays valid); its used
        // width is the snap table's (`snap.rs`, tab stops).
        let shaped_text: std::borrow::Cow<'_, str> = if source.text.contains('\t') {
            source.text.replace('\t', " ").into()
        } else {
            std::borrow::Cow::Borrowed(&source.text)
        };
        // `@font-face` faces: note the faces the text needs (lazy loading; the font epoch moves when 007 has
        // them) and split each span at the `unicode-range` boundaries of the groups in its family list.
        type Pieces = Vec<(Range<usize>, Vec<parley::FontFamilyName<'static>>)>;
        let mut range_pieces_root: Pieces = Vec::new();
        let mut range_pieces_spans: Vec<Pieces> = vec![Vec::new(); source.spans.len()];
        if !self.env.face_table().is_empty() {
            let mut budget = 1024usize;
            let mut pieces_of = |env: &crate::text::fonts::FontEnvironment,
                                 spec: &FontSpec,
                                 base: usize,
                                 text: &str|
             -> Pieces {
                env.note_face_use(spec, Some(text));
                let resolved = env.resolve_families(&spec.families);
                let mut out = Vec::new();
                for (r, list) in env.split_for_unicode_range(&resolved, text) {
                    if budget == 0 {
                        break;
                    }
                    budget -= 1;
                    let names = list
                        .into_iter()
                        .map(|n| parley::FontFamilyName::Named(std::borrow::Cow::Owned(n)))
                        .collect();
                    out.push((base + r.start..base + r.end, names));
                }
                out
            };
            range_pieces_root = pieces_of(&self.env, &source.root.font, 0, &shaped_text);
            for (i, span) in source.spans.iter().enumerate() {
                if let Some(text) = shaped_text.get(span.range.clone()) {
                    range_pieces_spans[i] =
                        pieces_of(&self.env, &span.style.font, span.range.start, text);
                }
            }
        }
        // Hyphenation (015 TXT-3): the break points inside words and the soft hyphens that must not
        // break go to Parley as line break opportunities.
        let hplan = super::hyphen::plan(source, &shaped_text);
        let hyphen_override: Option<Box<parley::LineBreakOverrideFn>> =
            if hplan.auto.is_empty() && hplan.auto_final.is_empty() && hplan.none_soft.is_empty() {
                None
            } else {
                let ov = super::hyphen::BreakOverride::new(&shaped_text, &hplan);
                Some(Box::new(move |cx| ov.call(cx)))
            };
        let mut b = self
            .lcx
            .ranged_builder(&mut self.env.fcx, &shaped_text, 1.0, true);
        b.set_line_break_override(hyphen_override.as_deref());
        b.set_base_direction(match source.base_direction {
            crate::text::bidi::BaseDirection::Ltr => parley::BaseDirection::Ltr,
            crate::text::bidi::BaseDirection::Rtl => parley::BaseDirection::Rtl,
            crate::text::bidi::BaseDirection::Auto if source.isolated_paragraphs => {
                parley::BaseDirection::Ltr
            }
            crate::text::bidi::BaseDirection::Auto => parley::BaseDirection::Auto,
        });
        for p in root_props {
            b.push_default(p);
        }
        b.push_default(StyleProperty::Brush(SpanBrush(u32::MAX)));
        // `@font-face` faces: the family list of each stretch of text outside a `unicode-range` group's
        // range lacks that group (see `fonts/faces.rs`). Nothing here runs for a document without them.
        if !range_pieces_root.is_empty() {
            for (r, names) in &range_pieces_root {
                b.push(
                    StyleProperty::FontFamily(FontFamily::List(std::borrow::Cow::Owned(
                        names.clone(),
                    ))),
                    r.clone(),
                );
            }
        }
        for (i, (span, props)) in source.spans.iter().zip(span_props).enumerate() {
            for p in props {
                b.push(p, span.range.clone());
            }
            b.push(
                StyleProperty::Brush(SpanBrush(i as u32)),
                span.range.clone(),
            );
            for (r, names) in &range_pieces_spans[i] {
                b.push(
                    StyleProperty::FontFamily(FontFamily::List(std::borrow::Cow::Owned(
                        names.clone(),
                    ))),
                    r.clone(),
                );
            }
        }
        // Atomic inlines are in-flow inline boxes of unknown size here; the line loop sets their
        // margin-box size on each scratch layout (`ScratchLayout::set_atomics`). Floats and
        // out-of-flow placeholders are zero-size `CustomOutOfFlow` boxes: the breaker yields
        // control to the driver when it reaches one. Both are positioned by byte index (an empty
        // item range); the id is the item's owner tag. Edge spacers are units of the snap table
        // only (Parley does not see them). Other item kinds are not shaped yet and are reported,
        // never approximated.
        for item in &source.items {
            match item.kind {
                InlineItemKind::AtomicInline => b.push_inline_box(InlineBox {
                    id: item.owner,
                    kind: InlineBoxKind::InFlow,
                    index: item.range.start.min(source.text.len()),
                    width: 0.0,
                    height: 0.0,
                    baseline: None,
                    vertical_align: parley_valign(item.valign),
                }),
                InlineItemKind::Float | InlineItemKind::OutOfFlow => b.push_inline_box(InlineBox {
                    id: item.owner,
                    kind: InlineBoxKind::CustomOutOfFlow,
                    index: item.range.start.min(source.text.len()),
                    width: 0.0,
                    height: 0.0,
                    baseline: None,
                    vertical_align: VerticalAlign::BASELINE,
                }),
                // Plain text items carry no box; edge spacers live in the snap table; UBA controls
                // are characters of the text itself.
                InlineItemKind::Text
                | InlineItemKind::EdgeSpacer { .. }
                | InlineItemKind::BidiControl => {}
                other => unsupported_items.push(other),
            }
        }
        let layout = b.build(&shaped_text);
        for kind in unsupported_items {
            self.shape_diag_events += 1;
            self.text_diagnostics.record(
                source.document,
                source.root_node,
                TextDiagnosticKind::UnsupportedItem {
                    item: item_kind_name(kind),
                },
                source.dom_revision,
            );
        }

        // Detect `.notdef` glyphs on an unbounded break of a throwaway copy.
        let mut probe = layout.clone();
        probe.break_all_lines(None);
        let mut missing = Vec::new();
        let mut synthesis_missing: Vec<(u32, crate::text::fonts::features::FontTag)> = Vec::new();
        let mut gsub_tags: std::collections::HashMap<
            (u64, u32),
            Vec<crate::text::fonts::features::FontTag>,
        > = Default::default();
        for line in probe.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let span = gr.style().brush.0;
                if let Some(sp) = source.spans.get(span as usize) {
                    for (tag, v) in sp.props.font_settings.features.iter() {
                        if *v == 0
                            || !crate::text::fonts::features::SYNTHESIZED_TAGS.contains(tag)
                            || synthesis_missing.contains(&(span, *tag))
                        {
                            continue;
                        }
                        let fs = &sp.props.font_settings;
                        if (matches!(tag, b"subs" | b"sups") && fs.no_synthesis_position)
                            || (!matches!(tag, b"subs" | b"sups") && fs.no_synthesis_small_caps)
                        {
                            continue;
                        }
                        let font = gr.run().font();
                        let tags = gsub_tags
                            .entry((font.data.id(), font.index))
                            .or_insert_with(|| {
                                crate::text::fonts::features::gsub_features(
                                    font.data.as_ref(),
                                    font.index,
                                )
                            });
                        if !tags.contains(tag) {
                            synthesis_missing.push((span, *tag));
                        }
                    }
                }
                for cluster in gr.run().clusters() {
                    let r = cluster.text_range();
                    let text = &source.text[r.clone()];
                    let visible = text.chars().any(|c| {
                        !c.is_whitespace()
                            && !c.is_control()
                            && !crate::text::bidi::is_bidi_control(c)
                    });
                    if visible && cluster.glyphs().any(|g| g.id == 0) {
                        missing.push(MissingGlyph {
                            byte: r.start,
                            ch: text.chars().next().unwrap_or('\u{FFFD}'),
                        });
                    }
                }
            }
        }
        if let Some(first) = missing.first() {
            self.note_missing_glyph(source.document, first.ch);
        }
        // the fonts the broken copy used (glyph runs exist only after breaking): registered blobs plus system files
        let blobs = self.env.blobs_for_layout(&probe);
        let mut policy = AdvancePolicy::for_layout(&probe, &blobs, self.subpixel);
        // Vertical lines (015 WM-1b): upright clusters advance by the vertical metrics of their faces.
        if source.spans.iter().any(|s| {
            matches!(
                s.props.orientation,
                super::vertical::GlyphOrientation::Mixed
                    | super::vertical::GlyphOrientation::Upright
            )
        }) {
            policy.load_vertical(&probe, &blobs);
        }
        // The hyphen of every span that holds a soft hyphen or an automatic break point.
        let mut hyphen_runs: Vec<Option<SynthRun>> = vec![None; source.spans.len()];
        if !source.is_bidi() {
            for (i, span) in source.spans.iter().enumerate() {
                if span.props.hyphens == HyphensKind::None {
                    continue;
                }
                let has_soft = source
                    .text
                    .get(span.range.clone())
                    .is_some_and(|t| t.contains(super::hyphen::SOFT_HYPHEN));
                let has_auto = hplan
                    .auto
                    .get(hplan.auto.partition_point(|&b| b < span.range.start))
                    .is_some_and(|&b| b < span.range.end)
                    || hplan
                        .auto_final
                        .get(hplan.auto_final.partition_point(|p| p.0 < span.range.start))
                        .is_some_and(|p| p.0 < span.range.end);
                if has_soft || has_auto {
                    hyphen_runs[i] = Some(self.shape_hyphen(
                        &span.style,
                        span.props.hyphenate_character.as_deref(),
                        span.range.start,
                    ));
                }
            }
        }
        let hyphens = super::snap::HyphenInput {
            auto: &hplan.auto,
            auto_final: &hplan.auto_final,
            runs: &hyphen_runs,
        };
        let snap = SnapTable::build(
            &layout,
            &policy,
            &source.text,
            &source.items,
            &source.spans,
            &hyphens,
        );
        let deco_fonts = Arc::new(self.probe_decoration_fonts(source));
        Arc::new(ShapedParagraph {
            deco_fonts,
            layout,
            source: Arc::new(source.clone()),
            missing,
            blobs,
            synthesis_missing,
            policy: Arc::new(policy),
            snap: Arc::new(snap),
            hyphen_runs: Arc::new(hyphen_runs),
            hyphen_unsupported: hplan.unsupported,
            font_epoch: self.env.font_epoch(),
            layout_memo: Default::default(),
        })
    }
}
