//! owner: 009-text-inline — lane L-TXT-r5 (T074 geometry; FR-044..FR-047)
//!
//! Text decoration geometry (CSS Text Decoration 3). A span's [`DecorationLayer`]s name the
//! decorating boxes whose lines are drawn under its text; the lines belong to the decorating box,
//! not to the text: position and auto thickness come from the decorating box's own first
//! available font, style and colour are its own, and a line runs unbroken over every piece of
//! text the box decorates on a line (nested boxes with their own decorations add their own lines).
//! Atomic inlines and inline-box edges interrupt a segment (the decoration is not drawn over them).
//!
//! Geometry follows the reference browser's rules (Blink `decoration_info.cc`), whole pixels:
//!
//! * thickness (measured against the reference, Chromium 156 on Linux, Noto Sans at 16, 24, 32, 48
//!   and 64 px; lane L-TXT, 015 TXT-6): `auto` is `font-size / 10` and `from-font` the font's own
//!   underline thickness (`post`), both as floats whose floor, at least one pixel, is painted; a
//!   length is rounded, at least one pixel (zero draws nothing);
//! * underline `auto`: top edge `max(1, ceil(t / 2))` below the baseline, `t` the float thickness
//!   (the rounded one for a length); with `text-underline-offset: <length>` the top edge is
//!   `round(offset)` below the baseline (the gap is replaced, not added);
//! * underline `from-font`: `round(font underline position + offset)` below the baseline;
//! * underline `under`: the content area's bottom (`descent`, whole pixels) less `floor(size * 0.08)`
//!   (fitted to the five sizes; other fonts are not measured), plus `round(offset)`;
//! * overline: the line sits above the decorating font's content area, its bottom edge at
//!   `baseline - ascent`;
//! * line-through: centred `ascent / 3` above the baseline.
//!
//! `skip-ink` is not implemented (`text-decoration-skip-ink` is unavailable in the profile and
//! reported by the cascade).

use super::frozen::{DecorationKind, DecorationSegment, FrozenParagraph};
use super::source::{
    DecorationLayer, DecorationThickness, ParagraphSource, SpanProps, TextStyle, UnderlinePos,
    text_properties,
};
use crate::text::fonts::TextServices;
use crate::text::metrics::FontQueryKey;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DecorationFontKey {
    epoch: u64,
    font: FontQueryKey,
    line_height: (u8, u32),
}
impl DecorationFontKey {
    fn new(epoch: u64, style: &TextStyle) -> Self {
        let line_height = match style.line_height {
            super::source::LineHeightSpec::Normal => (0, 0),
            super::source::LineHeightSpec::Number(value) => (1, value.to_bits()),
            super::source::LineHeightSpec::Px(value) => (2, value.to_bits()),
        };
        Self {
            epoch,
            font: FontQueryKey::new(&style.font, style.font_size),
            line_height,
        }
    }
}

/// The metrics of one decorating font (the first available font of a decorating box).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecoFont {
    pub style: TextStyle,
    /// Whole-pixel ascent of the font's content area.
    pub ascent: f32,
    /// The font's underline thickness, when it has a usable one (float px).
    pub underline_size: Option<f32>,
    /// The font's underline position: the distance of the line's top edge below the baseline (float px).
    pub underline_position: Option<f32>,
    /// Whole-pixel descent of the font's content area.
    pub descent: f32,
}

impl TextServices {
    /// The metrics of every distinct decorating font of `source`'s spans.
    pub(crate) fn probe_decoration_fonts(&mut self, source: &ParagraphSource) -> Vec<DecoFont> {
        let mut out: Vec<DecoFont> = Vec::new();
        let layers: Vec<DecorationLayer> = source
            .spans
            .iter()
            .flat_map(|s| s.props.decorations.iter().cloned())
            .collect();
        for layer in layers {
            if out.iter().any(|f| f.style == layer.font) {
                continue;
            }
            let key = DecorationFontKey::new(self.env.font_epoch(), &layer.font);
            if let Some(f) = self.deco_font_cache.get(&key) {
                out.push(f.clone());
                continue;
            }
            let props = text_properties(self, &layer.font, &SpanProps::default(), true);
            let mut b = self.lcx.ranged_builder(&mut self.env.fcx, "x", 1.0, true);
            for p in props {
                b.push_default(p);
            }
            let mut layout = b.build("x");
            let (ascent, descent) = layout
                .root_span_metrics()
                .map(|m| (m.ascent.round(), m.descent.round()))
                .unwrap_or((layer.font.font_size * 0.9, layer.font.font_size * 0.2));
            layout.break_all_lines(None);
            let metrics = layout
                .lines()
                .next()
                .and_then(|l| l.runs().next().map(|r| *r.font_metrics()));
            let underline_size = metrics
                .as_ref()
                .map(|m| m.underline_size)
                .filter(|s| s.is_finite() && *s > 0.0);
            // Parley's `underline_offset` is the distance of the line above the baseline (negative below).
            let underline_position = metrics
                .as_ref()
                .map(|m| -m.underline_offset)
                .filter(|p| p.is_finite() && *p > 0.0);
            let f = DecoFont {
                style: layer.font.clone(),
                ascent,
                underline_size,
                underline_position,
                descent,
            };
            if self.deco_font_cache.len() == 1024 {
                self.deco_font_cache.clear();
            }
            self.deco_font_cache.insert(key, f.clone());
            out.push(f);
        }
        out
    }
}

/// The used thickness of a layer: the painted whole-pixel thickness and the thickness the underline gap
/// is computed from (a float for `auto` and `from-font`, the rounded value for a length); `None` when
/// nothing is drawn.
pub(crate) fn used_thickness(
    layer: &DecorationLayer,
    font: Option<&DecoFont>,
) -> Option<(f32, f32)> {
    let float = |t: f32| (t.floor().max(1.0), t);
    Some(match layer.thickness {
        DecorationThickness::Auto => float(layer.font.font_size / 10.0),
        DecorationThickness::FromFont => float(
            font.and_then(|f| f.underline_size)
                .unwrap_or(layer.font.font_size / 10.0),
        ),
        DecorationThickness::Px(p) => {
            if p <= 0.0 || !p.is_finite() {
                return None;
            }
            let t = p.round().max(1.0);
            (t, t)
        }
    })
}

/// Top edge of the line of `kind` relative to the baseline (down is positive), whole pixels.
pub(crate) fn used_offset(
    kind: DecorationKind,
    layer: &DecorationLayer,
    painted: f32,
    gap_basis: f32,
    font: Option<&DecoFont>,
    ascent: f32,
) -> f32 {
    match kind {
        DecorationKind::Underline => {
            let auto_gap = (gap_basis / 2.0).ceil().max(1.0);
            let offset = layer.underline_offset.unwrap_or(0.0);
            match layer.underline_position {
                UnderlinePos::Auto => match layer.underline_offset {
                    None => auto_gap,
                    Some(o) => o.round(),
                },
                UnderlinePos::FromFont => match font.and_then(|f| f.underline_position) {
                    Some(p) => (p + offset).round(),
                    None => auto_gap + offset.round(),
                },
                UnderlinePos::Under => {
                    let descent = font.map_or(layer.font.font_size * 0.2, |f| f.descent);
                    descent - (layer.font.font_size * 0.08).floor().max(1.0) + offset.round()
                }
            }
        }
        DecorationKind::Overline => -ascent - painted,
        DecorationKind::LineThrough => -(ascent / 3.0 + painted / 2.0).round(),
    }
}

/// The decoration segments of the source's decorating boxes, for every line of `p`.
pub(crate) fn line_segments(p: &FrozenParagraph) -> Vec<DecorationSegment> {
    let mut out: Vec<DecorationSegment> = Vec::new();
    let source = p.source();
    if source.spans.iter().all(|s| s.props.decorations.is_empty()) {
        return out;
    }
    for line in 0..p.line_count() {
        if p.line_extras(line).clamped_out {
            continue;
        }
        let first = out.len();
        for piece in p.text_pieces(line) {
            if piece.width <= 0.0 {
                continue;
            }
            let Some(span) = source.spans.get(piece.span as usize) else {
                continue;
            };
            for layer in span.props.decorations.iter() {
                let font = p.deco_fonts().iter().find(|f| f.style == layer.font);
                let Some((thickness, gap_basis)) = used_thickness(layer, font) else {
                    continue;
                };
                let ascent = font.map(|f| f.ascent).unwrap_or(piece.ascent);
                for (on, kind) in [
                    (layer.underline, DecorationKind::Underline),
                    (layer.overline, DecorationKind::Overline),
                    (layer.line_through, DecorationKind::LineThrough),
                ] {
                    if !on {
                        continue;
                    }
                    let (x0, x1) = (piece.x, piece.x + piece.width);
                    // Extend the previous segment of this decorating box and kind when the text is
                    // contiguous (the same baseline, no edge spacer or atomic between).
                    if let Some(prev) = out[first..]
                        .iter_mut()
                        .rev()
                        .find(|d| d.owner == layer.owner && d.kind == kind)
                        && (prev.inline_range.1 - x0).abs() < 0.01
                        && (prev.baseline - piece.baseline).abs() < 0.01
                        && prev.style == layer.style
                    {
                        prev.inline_range.1 = x1;
                        continue;
                    }
                    out.push(DecorationSegment {
                        owner: layer.owner,
                        line_index: line,
                        kind,
                        style: layer.style,
                        baseline: piece.baseline,
                        offset: used_offset(kind, layer, thickness, gap_basis, font, ascent),
                        thickness,
                        inline_range: (x0, x1),
                    });
                }
            }
        }
    }
    out
}
