//! owner: 009-text-inline (R-36; IF-46) — created by 001 slice (R-28)
//!
//! `build_source` (008 `ifc-interface.md` §5.2): collects the inline root's text runs in tree
//! order into one `ParagraphSource`, applying the white-space processing of each run's own
//! `white-space-collapse` (CSS Text 3 §4.1.1, CSS Text 4 §4.1): `collapse` turns segment breaks and
//! tabs into spaces, removes a collapsible space following another one (also across inline element
//! boundaries) and the spaces at the start and end of the paragraph (§4.1.2; spaces at soft wraps
//! hang in the breaker); `preserve` and `break-spaces` keep spaces, tabs and segment breaks (a
//! preserved segment break is a forced line break, a preserved tab goes to the next tab stop);
//! `preserve-breaks` collapses spaces and tabs but keeps segment breaks and removes the
//! collapsible spaces around them. `text-wrap-mode`, `overflow-wrap`, `word-break`, `line-break`,
//! `letter-spacing`, `word-spacing`, `tab-size` and the text decorations in effect travel with the
//! span (`SpanProps`). Inline elements that change only inherited text properties become spans
//! with their own owner (FR-019).
//!
//! Atomic inlines (inline-blocks, replaced elements, controls) are inline items at their text
//! position, keyed by their box id; a collapsible space next to an atomic is kept (it separates
//! the atomic from the text). The start and end edges of an inline box (padding + border + margin,
//! CSS 2.1 §8, §10.3.1) are in-flow edge spacers (G-TEXT-EDGES). Percentage padding or margins
//! cannot be resolved while the source is built and are reported (FR-022) and treated as zero.
//!
//! Bidirectional text (CSS Writing Modes 4 section 2.4; UAX #9; lane L-BIDI-r14): the paragraph's
//! base direction is the root's `direction` (`Auto`, the first strong character, for
//! `unicode-bidi: plaintext` and `dir=auto`); an inline box with `unicode-bidi` other than `normal`
//! (the UA sheet gives `bdi`, `bdo` and `[dir]` theirs) is wrapped in the Unicode explicit
//! formatting characters of `crate::text::bidi::controls_for` -- opening controls, start edge,
//! content, end edge, closing controls -- as non-DOM text; a root with `bidi-override` is wrapped
//! the same way. The edge widths of a box map the physical sides to its start and end by the
//! direction of the embedding it sits in.

use crate::style::view::groups as g;
use crate::text::paragraph::{
    HyphensKind, LineBreakKind, ParagraphSource, SpanCollapse, SpanProps, TabSpec,
    VerticalAlignSpec, WordBreakKind, WrapOverflow,
};
use ui4me_engine_types::diag::FeatureId;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;

use crate::layout::box_tree::{
    BoxId, BoxKind, BoxStyle, BoxTree, CollapseMode, Direction, LengthPercentage, MarginValue,
    UnicodeBidi,
};
use crate::layout::ifc::interface::{InlineLayoutHost, owner_tag};
use crate::text::bidi::{self, BaseDirection, BidiMode};

fn feature(id: &'static str) -> FeatureId {
    FeatureId::new(id).expect("css.property root is registered")
}

/// An edge in px, or the feature that could not be resolved (percentages and calc with percent).
fn lp_px(v: &LengthPercentage, id: &'static str, unresolved: &mut Vec<&'static str>) -> f32 {
    match v {
        LengthPercentage::Px(p) => *p,
        LengthPercentage::Percent(p) if *p == 0.0 => 0.0,
        LengthPercentage::Calc(_) if v.is_zero() => 0.0,
        _ => {
            unresolved.push(id);
            0.0
        }
    }
}

fn margin_px(v: &MarginValue, id: &'static str, unresolved: &mut Vec<&'static str>) -> f32 {
    match v {
        MarginValue::Auto => 0.0,
        MarginValue::Lp(lp) => lp_px(lp, id, unresolved),
    }
}

/// The inline-start and inline-end edge widths of an inline box laid out in an embedding of
/// direction `rtl` (the start edge is the right one in a right-to-left embedding), and the
/// longhands that cannot be resolved here.
fn edge_widths(s: &BoxStyle, rtl: bool) -> (f32, f32, Vec<&'static str>) {
    let mut un = Vec::new();
    let left = lp_px(&s.padding.left, "css.property.padding-left", &mut un)
        + s.border.left
        + margin_px(&s.margin.left, "css.property.margin-left", &mut un);
    let right = lp_px(&s.padding.right, "css.property.padding-right", &mut un)
        + s.border.right
        + margin_px(&s.margin.right, "css.property.margin-right", &mut un);
    let (start, end) = if rtl { (right, left) } else { (left, right) };
    // Vertical edges do not take part in line layout; they only have to be well formed.
    lp_px(&s.padding.top, "css.property.padding-top", &mut un);
    lp_px(&s.padding.bottom, "css.property.padding-bottom", &mut un);
    margin_px(&s.margin.top, "css.property.margin-top", &mut un);
    margin_px(&s.margin.bottom, "css.property.margin-bottom", &mut un);
    (start, end, un)
}

/// The bidi mode of an inline box and whether it takes its direction from its first strong
/// character (`dir=auto` isolates, `unicode-bidi: plaintext`).
fn bidi_mode(style: &BoxStyle) -> (BidiMode, bool) {
    match style.unicode_bidi {
        UnicodeBidi::Normal => (BidiMode::Normal, false),
        UnicodeBidi::Embed => (BidiMode::Embed, false),
        UnicodeBidi::Isolate => {
            if style.dir_auto {
                (BidiMode::Plaintext, true)
            } else {
                (BidiMode::Isolate, false)
            }
        }
        UnicodeBidi::BidiOverride => (BidiMode::Override, false),
        UnicodeBidi::IsolateOverride => (BidiMode::IsolateOverride, false),
        UnicodeBidi::Plaintext => (BidiMode::Plaintext, true),
    }
}

/// The first strong direction of the text below `id`, not looking into isolates (UAX #9 P2).
fn subtree_first_strong(tree: &BoxTree, id: BoxId) -> Option<bool> {
    fn walk(tree: &BoxTree, id: BoxId, depth_guard: usize) -> Option<bool> {
        if depth_guard > 256 {
            return None;
        }
        for &c in &tree.get(id).children {
            let b = tree.get(c);
            match &b.kind {
                BoxKind::TextRun(t) => {
                    if let Some(d) = bidi::first_strong(t) {
                        return Some(d);
                    }
                }
                BoxKind::InlineBox => {
                    let isolating = matches!(
                        b.style.unicode_bidi,
                        UnicodeBidi::Isolate
                            | UnicodeBidi::IsolateOverride
                            | UnicodeBidi::Plaintext
                    );
                    if !isolating && let Some(d) = walk(tree, c, depth_guard + 1) {
                        return Some(d);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(tree, id, 0)
}

/// The base direction a root's paragraph is shaped with, and the direction `start` and `end`
/// resolve to for it (`direction`, or the first strong character for `plaintext` and `dir=auto`).
pub fn root_direction(tree: &BoxTree, root: BoxId) -> (BaseDirection, Direction) {
    let r = tree.get(root);
    let auto = r.style.unicode_bidi == UnicodeBidi::Plaintext || r.style.dir_auto;
    if auto {
        let rtl = subtree_first_strong(tree, root).unwrap_or(false);
        (
            BaseDirection::Auto,
            if rtl { Direction::Rtl } else { Direction::Ltr },
        )
    } else if r.style.direction == Direction::Rtl {
        (BaseDirection::Rtl, Direction::Rtl)
    } else {
        (BaseDirection::Ltr, Direction::Ltr)
    }
}

/// Remove the collapsible space that ends the paragraph's content (CSS Text 3 section 4.1.2) when
/// only UBA controls follow it; the ranges after it move up by one byte.
fn remove_trailing_space(source: &mut ParagraphSource) {
    let Some((idx, _)) = source
        .text
        .char_indices()
        .rev()
        .find(|(_, c)| !bidi::is_bidi_control(*c))
    else {
        return;
    };
    if source.text.as_bytes()[idx] != b' ' {
        return;
    }
    source.text.remove(idx);
    for sp in source.spans.iter_mut() {
        if sp.range.start > idx {
            sp.range.start -= 1;
        }
        if sp.range.end > idx {
            sp.range.end -= 1;
        }
    }
    source.spans.retain(|sp| !sp.range.is_empty());
    for i in source.items.iter_mut() {
        if i.range.start > idx {
            i.range.start -= 1;
            i.range.end -= 1;
        }
    }
}

/// The `SpanProps` of text styled by `style`.
fn span_props(style: &BoxStyle) -> SpanProps {
    let ws = style.white_space;
    let f = &style.flow;
    SpanProps {
        decorations: style.decorations.clone(),
        collapse: match ws.collapse {
            CollapseMode::Collapse => SpanCollapse::Collapse,
            CollapseMode::Preserve | CollapseMode::PreserveBreaks => SpanCollapse::Preserve,
            CollapseMode::BreakSpaces => SpanCollapse::BreakSpaces,
        },
        nowrap: !ws.wrap,
        overflow_wrap: match f.overflow_wrap {
            g::OverflowWrap::Normal => WrapOverflow::Normal,
            g::OverflowWrap::BreakWord => WrapOverflow::BreakWord,
            g::OverflowWrap::Anywhere => WrapOverflow::Anywhere,
        },
        word_break: match f.word_break {
            g::WordBreak::Normal => WordBreakKind::Normal,
            g::WordBreak::BreakAll => WordBreakKind::BreakAll,
            g::WordBreak::KeepAll => WordBreakKind::KeepAll,
        },
        line_break: match f.line_break {
            g::LineBreak::Auto => LineBreakKind::Auto,
            g::LineBreak::Loose => LineBreakKind::Loose,
            g::LineBreak::Normal => LineBreakKind::Normal,
            g::LineBreak::Strict => LineBreakKind::Strict,
            g::LineBreak::Anywhere => LineBreakKind::Anywhere,
        },
        hyphens: match f.hyphens {
            g::Hyphens::None => HyphensKind::None,
            g::Hyphens::Manual => HyphensKind::Manual,
            g::Hyphens::Auto => HyphensKind::Auto,
        },
        hyphenate_character: f.hyphenate_character.clone(),
        letter_spacing: f.letter_spacing,
        word_spacing: f.word_spacing,
        tab_size: match f.tab_size {
            g::TabSize::Number(n) => TabSpec::Spaces(n),
            g::TabSize::Length(px) => TabSpec::Px(px),
        },
        valign: VerticalAlignSpec::Baseline,
        font_settings: vertical_font_settings(style, &f.font_settings),
        orientation: crate::layout::writing_mode::glyph_orientation(style),
        combine_upright: style.text_combine_upright && style.writing_mode.is_vertical(),
    }
}

/// Upright glyphs of a vertical line are shaped with their vertical alternates (`vert`; 015 WM-1b).
fn vertical_font_settings(
    style: &BoxStyle,
    fs: &crate::text::fonts::features::FontSettings,
) -> crate::text::fonts::features::FontSettings {
    use crate::text::paragraph::GlyphOrientation as O;
    if !matches!(
        crate::layout::writing_mode::glyph_orientation(style),
        O::Mixed | O::Upright
    ) {
        return fs.clone();
    }
    let mut out = fs.clone();
    let mut features: Vec<_> = vec![(*b"vert", 1u32)];
    features.extend(fs.features.iter().copied());
    out.features = features.into();
    out
}

/// Remove the last character of the processed text (a trailing collapsible space or a trailing
/// preserved newline); spans and items positioned after the new end move with it.
fn pop_last(source: &mut ParagraphSource) {
    source.text.pop();
    let end = source.text.len();
    if let Some(last) = source.spans.last_mut() {
        last.range.end = end;
        if last.range.is_empty() {
            source.spans.pop();
        }
    }
    for i in source.items.iter_mut() {
        if i.range.start > end {
            i.range = end..end;
        }
    }
}

struct Collector<'a> {
    tree: &'a BoxTree,
    source: ParagraphSource,
    /// Whether the last character emitted is a collapsible space.
    after_space: bool,
    /// Whether any text or atomic inline has been emitted (edges do not count).
    has_content: bool,
    /// Edge longhands to report if the edges end up unused: (feature, element).
    edge_reports: Vec<(&'static str, NodeId)>,
    /// A tab was preserved in a wrapping run: reported once (`css.property.tab-size`).
    tab_in_wrap: Option<NodeId>,
    /// The text style of the inline root: its strut, the parent of the outermost inline boxes.
    root_text: crate::text::paragraph::TextStyle,
    /// The inline boxes around the box being walked, outermost first (`vertical-align`).
    valign: Vec<ValignEntry>,
    /// Build the paragraph with the first-line style on every box that has one (015 TXT-5, `FirstLineSource`).
    first_line: bool,
}

/// One inline box around the text being collected, for `vertical-align` (CSS 2.1 section 10.8.1).
///
/// The line breaker aligns every text span against the inline root (Parley's span table is flat
/// here), so what is exact is: a chain of parent-relative shifts (`sub`, `super`, lengths,
/// percentages), which add up; one of `text-top`, `text-bottom` and `middle` on a span whose parent
/// has the root's metrics; and `top` or `bottom` on a span whose inline descendants all have its own
/// metrics (then the whole aligned subtree is one box height). Anything else is not consumed and
/// reported.
struct ValignEntry {
    /// The shift the text directly inside the box gets, relative to the root's baseline.
    eff: VerticalAlignSpec,
    text: crate::text::paragraph::TextStyle,
}

impl ValignEntry {
    fn baseline(text: &crate::text::paragraph::TextStyle) -> Self {
        ValignEntry {
            eff: VerticalAlignSpec::Baseline,
            text: text.clone(),
        }
    }
}

/// The effective `vertical-align` of an inline box with `v` inside `parent`, or `None` when the
/// combination is not consumed.
fn combine_valign(
    v: VerticalAlignSpec,
    text: &crate::text::paragraph::TextStyle,
    parent: &ValignEntry,
    root: &crate::text::paragraph::TextStyle,
) -> Option<VerticalAlignSpec> {
    use VerticalAlignSpec as V;
    let pe = parent.eff;
    let parent_size = parent.text.font_size;
    // The text inside a `top` / `bottom` box is part of that box's aligned subtree.
    if matches!(pe, V::Top | V::Bottom) {
        let same_box = text.font == parent.text.font
            && text.font_size == parent.text.font_size
            && text.line_height == parent.text.line_height;
        return (same_box && (matches!(v, V::Baseline) || v == pe)).then_some(pe);
    }
    let shift = |d: f32| match pe {
        V::Baseline => Some(V::Length(d)),
        V::Length(s) => Some(V::Length(s + d)),
        _ => None,
    };
    match v {
        V::Baseline => Some(pe),
        V::Length(l) => shift(l),
        V::Sub => shift(-parent_size / 5.0),
        V::Super => shift(parent_size / 3.0),
        V::Top | V::Bottom => Some(v),
        V::TextTop | V::TextBottom | V::Middle => {
            let same_metrics = parent.text.font == root.font
                && parent.text.font_size == root.font_size
                && parent.text.line_height == root.line_height;
            (pe == V::Baseline && same_metrics).then_some(v)
        }
    }
}

impl Collector<'_> {
    /// The style a box is laid out with: its first-line variant in a first-line build.
    fn style_of<'t>(&self, b: &'t crate::layout::box_tree::LayoutBox) -> &'t BoxStyle {
        if self.first_line {
            b.style.first_line.as_deref().unwrap_or(&b.style)
        } else {
            &b.style
        }
    }

    /// The owner tag of a box's spans: the pseudo-element's for generated content, the first-line variant's in a
    /// first-line build (the scene and the geometry queries read the variant's style from it).
    fn tag_of(&self, b: &crate::layout::box_tree::LayoutBox, owner: NodeId) -> u64 {
        use crate::layout::box_tree::{BoxOwner as O, PseudoKind as K};
        if self.first_line && b.style.first_line.is_some() {
            match b.owner {
                O::Pseudo(n, K::Before) => {
                    return crate::layout::ifc::interface::owner_tag_pseudo(n, K::FirstLineBefore);
                }
                O::Pseudo(n, K::After) => {
                    return crate::layout::ifc::interface::owner_tag_pseudo(n, K::FirstLineAfter);
                }
                O::Pseudo(..) => {}
                O::Element(n) | O::Anonymous(n) => {
                    return crate::layout::ifc::interface::owner_tag_pseudo(
                        if matches!(b.owner, O::Element(_)) {
                            n
                        } else {
                            owner
                        },
                        K::FirstLine,
                    );
                }
            }
        }
        box_tag(b, owner)
    }

    /// Whether every inline box and text under the `top` / `bottom` box `id` has the box's own
    /// metrics and baseline alignment, and no atomic inline is inside: then the box's aligned
    /// subtree is as high as one box of those metrics and each text can be aligned alone.
    fn top_subtree_is_one_box(&self, id: BoxId) -> bool {
        let own = &self.tree.get(id).style;
        fn walk(tree: &BoxTree, id: BoxId, own: &BoxStyle) -> bool {
            tree.get(id).children.iter().all(|&c| {
                let b = tree.get(c);
                let same = b.style.text.font == own.text.font
                    && b.style.text.font_size == own.text.font_size
                    && b.style.text.line_height == own.text.line_height;
                match &b.kind {
                    BoxKind::TextRun(_) => same,
                    BoxKind::InlineBox => {
                        same && (b.style.vertical_align == VerticalAlignSpec::Baseline
                            || b.style.vertical_align == own.vertical_align)
                            && walk(tree, c, own)
                    }
                    BoxKind::InlineBlock { .. } | BoxKind::Replaced(_) => false,
                    _ => true,
                }
            })
        }
        walk(self.tree, id, own)
    }

    fn text(
        &mut self,
        text: &str,
        style: &BoxStyle,
        owner: NodeId,
        tag: u64,
        host: &mut dyn InlineLayoutHost,
    ) {
        for f in &style.unconsumed_text {
            host.report(*f, Stage::Layout, owner);
        }
        let ws = style.white_space;
        let collapse_spaces = !ws.preserves_spaces();
        let keep_breaks = ws.preserves_newlines();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            // A CRLF pair is one segment break.
            let c = if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    continue;
                }
                '\n'
            } else {
                c
            };
            match c {
                '\n' if keep_breaks => {
                    if collapse_spaces && self.after_space {
                        // A collapsible space before a preserved segment break is removed.
                        if out.ends_with(' ') {
                            out.pop();
                        } else if out.is_empty() && self.source.text.ends_with(' ') {
                            pop_last(&mut self.source);
                        }
                    }
                    out.push('\n');
                    // Spaces after the break are at a line start: removed when collapsible.
                    self.after_space = collapse_spaces;
                    self.has_content = true;
                }
                '\n' | '\t' | '\u{000C}' | ' ' if collapse_spaces || c == '\n' => {
                    // Collapse with the previous space; drop at paragraph start.
                    if self.after_space || (!self.has_content && out.is_empty()) {
                        continue;
                    }
                    self.after_space = true;
                    out.push(' ');
                }
                '\t' => {
                    if ws.wrap && self.tab_in_wrap.is_none() {
                        self.tab_in_wrap = Some(owner);
                    }
                    self.after_space = false;
                    out.push('\t');
                }
                '\u{000C}' => {
                    self.after_space = false;
                    out.push(' ');
                }
                c => {
                    self.after_space = false;
                    out.push(c);
                }
            }
        }
        if !out.is_empty() {
            self.has_content = true;
        }
        let tf = style.flow.text_transform;
        if tf != g::TextTransformValue::default() && !out.is_empty() {
            // `text-transform` on the white-space-processed text (the DOM text is unchanged);
            // `capitalize` needs to know whether the run starts a word.
            let starts_word = !self
                .source
                .text
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let spec = crate::text::transform::TextTransform {
                case: match tf.case {
                    g::TextTransformCase::None => crate::text::transform::CaseTransform::None,
                    g::TextTransformCase::Capitalize => {
                        crate::text::transform::CaseTransform::Capitalize
                    }
                    g::TextTransformCase::Uppercase => {
                        crate::text::transform::CaseTransform::Uppercase
                    }
                    g::TextTransformCase::Lowercase => {
                        crate::text::transform::CaseTransform::Lowercase
                    }
                },
                full_width: tf.full_width,
                full_size_kana: tf.full_size_kana,
            };
            let t =
                crate::text::transform::transform(&out, spec, &style.text.font.lang, starts_word);
            out = t.text;
            if let Some(lang) = t.unapplied_language
                && let Some(f) = crate::text::diagnostics::feature::text_transform_language(lang)
            {
                // Greek accent removal and Dutch `IJ` are not performed (FR-039).
                host.report(f, Stage::Text, owner);
            }
            if t.unmapped_halfwidth {
                host.report(
                    feature("css.value.text-transform-full-width-halfwidth"),
                    Stage::Text,
                    owner,
                );
            }
        }
        let mut props = span_props(style);
        props.valign = self
            .valign
            .last()
            .map_or(VerticalAlignSpec::Baseline, |e| e.eff);
        self.source.push_with(&out, &style.text, tag, &props);
    }

    fn walk(&mut self, id: BoxId, owner: NodeId, host: &mut dyn InlineLayoutHost) {
        let b = self.tree.get(id);
        match &b.kind {
            BoxKind::TextRun(t) => {
                let t = t.clone();
                let tag = self.tag_of(b, owner);
                let sty = self.style_of(b);
                self.text(&t, sty, owner, tag, host);
            }
            BoxKind::InlineBox => {
                let node = b.owner.node();
                let tag = self.tag_of(b, node);
                let sty = self.style_of(b);
                let parent = self.valign.last().map_or_else(
                    || ValignEntry::baseline(&self.root_text),
                    |e| ValignEntry {
                        eff: e.eff,
                        text: e.text.clone(),
                    },
                );
                let mut combined =
                    combine_valign(sty.vertical_align, &sty.text, &parent, &self.root_text);
                if matches!(
                    sty.vertical_align,
                    VerticalAlignSpec::Top | VerticalAlignSpec::Bottom
                ) && !self.top_subtree_is_one_box(id)
                {
                    // The whole aligned subtree would have to be one box (see `ValignEntry`).
                    combined = None;
                }
                let eff = match combined {
                    Some(e) => e,
                    None => {
                        // Not consumed (see `ValignEntry`): the text stays where the parent puts it.
                        host.report(feature("css.property.baseline-shift"), Stage::Layout, node);
                        parent.eff
                    }
                };
                self.valign.push(ValignEntry {
                    eff,
                    text: sty.text.clone(),
                });
                // Bidi: the box's own embedding, if it opens one (CSS Writing Modes 4 section 2.4).
                let outer_rtl = self.source.embedding_rtl;
                let (mode, auto) = bidi_mode(&b.style);
                let inner_rtl = match mode {
                    BidiMode::Normal => outer_rtl,
                    _ if auto => subtree_first_strong(self.tree, id).unwrap_or(false),
                    _ => b.style.direction == Direction::Rtl,
                };
                let (open, close) = bidi::controls_for(mode, inner_rtl);
                let mut props = span_props(sty);
                props.valign = self
                    .valign
                    .last()
                    .map_or(VerticalAlignSpec::Baseline, |e| e.eff);
                self.source.embedding_rtl = inner_rtl;
                for c in open {
                    self.source.push_bidi_control(c, &sty.text, tag, &props);
                }
                let (mut start, mut end, unresolved) = edge_widths(&b.style, inner_rtl);
                for id in unresolved {
                    self.edge_reports.push((id, node));
                }
                // A piece of an inline box split around a block has the edge on one side only.
                let (has_start, has_end) = (!b.style.split_start, !b.style.split_end);
                if !has_start {
                    start = 0.0;
                }
                if !has_end {
                    end = 0.0;
                }
                if (start != 0.0 || end != 0.0) && has_start {
                    self.source.push_edge(true, start, tag);
                }
                for &c in &b.children {
                    self.walk(c, node, host);
                }
                if (start != 0.0 || end != 0.0) && has_end {
                    self.source.push_edge(false, end, tag);
                }
                for c in close {
                    self.source.push_bidi_control(c, &sty.text, tag, &props);
                }
                self.source.embedding_rtl = outer_rtl;
                self.valign.pop();
            }
            _ if b.is_out_of_flow() => {
                // Positioned against its containing block by the positioning pass
                // (`coordinator/oof.rs`); it takes no room in the line. Its place in the text is
                // its static position (CSS 2.1 section 10.3.7).
                self.source.push_out_of_flow(id.0 as u64);
            }
            _ if b.is_float() => {
                // A float (CSS 2.1 section 9.5) is placed by the line that holds this point.
                self.source.push_float(id.0 as u64);
            }
            BoxKind::InlineBlock { .. } | BoxKind::Replaced(_) if b.style.marker_outside => {
                // An outside marker hangs before the line start: it takes no room and does not
                // end the collapsing of the white space that leads the item's text (FR-082).
                self.source.push_atomic(id.0 as u64, b.style.vertical_align);
            }
            BoxKind::InlineBlock { .. } | BoxKind::Replaced(_) => {
                if self
                    .valign
                    .last()
                    .is_some_and(|e| e.eff != VerticalAlignSpec::Baseline)
                {
                    // The span's shift would carry the atomic inline with it: not consumed.
                    host.report(feature("css.property.baseline-shift"), Stage::Layout, owner);
                }
                // An atomic inline: a collapsible space after it is kept (CSS Text 3 §4.1.1).
                self.source.push_atomic(id.0 as u64, b.style.vertical_align);
                self.after_space = false;
                self.has_content = true;
            }
            BoxKind::BlockContainer { .. } => {
                // Not reachable: an inline root holds no in-flow block-level box that is not a float.
            }
        }
    }
}

/// The owner tag of a box's spans: the pseudo-element's when the box is generated content.
fn box_tag(b: &crate::layout::box_tree::LayoutBox, owner: NodeId) -> u64 {
    match b.owner {
        crate::layout::box_tree::BoxOwner::Pseudo(n, k) => {
            crate::layout::ifc::interface::owner_tag_pseudo(n, k)
        }
        _ => owner_tag(owner),
    }
}

/// Build the paragraph of an inline root; `None` when it has no text, atomic inline or inline
/// edge after white-space processing (no line boxes, CSS 2.1 §9.4.2).
pub fn build_source(
    tree: &BoxTree,
    root: BoxId,
    host: &mut dyn InlineLayoutHost,
) -> Option<ParagraphSource> {
    build_source_in(tree, root, host, false)
}

/// [`build_source`]; with `first_line` every box that has a first-line variant is built with it (the paragraph as if
/// all of it were on the first line of a block with `::first-line`, 015 TXT-5). The root's own strut stays the
/// block's.
pub fn build_source_in(
    tree: &BoxTree,
    root: BoxId,
    host: &mut dyn InlineLayoutHost,
    first_line: bool,
) -> Option<ParagraphSource> {
    let r = tree.get(root);
    let owner = r.owner.node();
    let mut c = Collector {
        tree,
        source: ParagraphSource::new(
            tree.document,
            if first_line {
                r.style
                    .first_line
                    .as_deref()
                    .unwrap_or(&r.style)
                    .text
                    .clone()
            } else {
                r.style.text.clone()
            },
        ),
        after_space: false,
        has_content: false,
        edge_reports: Vec::new(),
        tab_in_wrap: None,
        root_text: r.style.text.clone(),
        valign: Vec::new(),
        first_line,
    };
    let (base, resolved) = root_direction(tree, root);
    c.source.base_direction = base;
    c.source.embedding_rtl = resolved == Direction::Rtl;
    // A block container with `bidi-override` overrides the direction of all its inline content.
    let root_override = matches!(
        r.style.unicode_bidi,
        UnicodeBidi::BidiOverride | UnicodeBidi::IsolateOverride
    );
    if root_override {
        let rtl = c.source.embedding_rtl;
        let props = span_props(&r.style);
        c.source.push_bidi_control(
            if rtl { bidi::RLO } else { bidi::LRO },
            &r.style.text,
            owner_tag(owner),
            &props,
        );
    }
    for &child in &r.children {
        c.walk(child, owner, host);
    }
    if root_override {
        let props = span_props(&r.style);
        c.source
            .push_bidi_control(bidi::PDF, &r.style.text, owner_tag(owner), &props);
    }
    let mut source = c.source;
    // Remove the trailing collapsible space (end of the last line, CSS Text 3 §4.1.2) unless an
    // atomic inline follows it.
    let atomic_after = source.items.iter().any(|i| {
        i.kind == crate::text::paragraph::InlineItemKind::AtomicInline
            && i.range.start >= source.text.len()
    });
    if c.after_space && !atomic_after {
        if source.text.ends_with(' ') {
            pop_last(&mut source);
        } else if source.text.ends_with(bidi::is_bidi_control) {
            remove_trailing_space(&mut source);
        }
    }
    // A preserved segment break at the very end of the block ends its last line; it does not make
    // an empty line after it (CSS Text 3 §4.1.3, as the reference browser lays out `pre`): the
    // line driver stops after the last forced break (`driver.rs`).

    if base == BaseDirection::Auto && source.text.contains('\n') {
        // Each forced-break line is a bidi paragraph of its own (UAX #9 P1).
        source.embedding_rtl = false;
        source.fallback_rtl = r.style.direction == Direction::Rtl;
        source.isolate_bidi_paragraphs();
    }
    for (id, node) in c.edge_reports {
        host.report(feature(id), Stage::Layout, node);
    }
    if let Some(node) = c.tab_in_wrap {
        // The tab stops of a line that wraps are measured from the start of its forced line, not
        // from the soft-wrap start (009 snap table); reported, never silently approximated.
        host.report(feature("css.property.tab-size"), Stage::Layout, node);
    }
    (!source.is_empty()).then_some(source)
}
