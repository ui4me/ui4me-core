//! owner: 009-text-inline
//!
//! Style interning and size limits, checked before any Parley call (T017; FR-092, FR-096;
//! constitution V; research R-04).
//!
//! * At most [`MAX_STYLES`] distinct span styles per paragraph (Parley asserts on more than
//!   `u16::MAX` styles including the root, a panic path that must be unreachable). Spans whose
//!   style would exceed the limit take the root's style, the nearest ancestor every span shares
//!   in the flat source, and keep their range.
//! * At most [`MAX_TEXT_BYTES`] UTF-8 bytes per paragraph; longer text is laid out up to the
//!   limit (cut on a character boundary); spans, items and segments are clipped accordingly.

use super::offset_map::Segment;
use super::source::{InlineItem, ParagraphSource, SourceSpan};

/// Style-bearing spans per paragraph (Parley's limit is `u16::MAX` resolved styles).
pub const MAX_STYLES: usize = 65_000;
/// Maximum paragraph length in UTF-8 bytes (FR-092).
pub const MAX_TEXT_BYTES: usize = 16_777_215;

/// Result of [`apply_limits`].
#[derive(Debug)]
pub struct Limited {
    pub source: ParagraphSource,
    pub styles_clamped: bool,
    pub text_truncated: bool,
}

/// Apply both limits. The returned source equals the input when neither is exceeded.
pub fn apply_limits(source: &ParagraphSource) -> Limited {
    apply_limits_with(source, MAX_STYLES, MAX_TEXT_BYTES)
}

/// Same with explicit limits (tests use small byte limits).
pub fn apply_limits_with(source: &ParagraphSource, max_styles: usize, max_bytes: usize) -> Limited {
    let mut out = source.clone();
    let mut text_truncated = false;
    if out.text.len() > max_bytes {
        let mut cut = max_bytes;
        while !out.text.is_char_boundary(cut) {
            cut -= 1;
        }
        out.text.truncate(cut);
        out.spans = out
            .spans
            .iter()
            .filter(|s| s.range.start < cut)
            .map(|s| SourceSpan {
                range: s.range.start..s.range.end.min(cut),
                style: s.style.clone(),
                owner: s.owner,
                props: s.props.clone(),
            })
            .collect();
        out.items = out
            .items
            .iter()
            .filter(|i| i.range.start < cut)
            .map(|i| InlineItem {
                range: i.range.start..i.range.end.min(cut),
                ..i.clone()
            })
            .collect();
        out.segments = out
            .segments
            .iter()
            .filter(|s| s.processed.start < cut)
            .map(|s| Segment {
                processed: s.processed.start..s.processed.end.min(cut),
                dom: s.dom,
                kind: s.kind,
            })
            .collect();
        text_truncated = true;
    }
    // Parley resolves one style per differing span (the brush carries the span index), so the
    // span count is what must stay under u16::MAX. Spans beyond the limit are merged into one
    // trailing span with the root style (the nearest ancestor) and the first excess span's owner.
    let mut styles_clamped = false;
    if out.spans.len() > max_styles {
        let tail: Vec<SourceSpan> = out.spans.split_off(max_styles);
        let owner = tail[0].owner;
        out.spans.push(SourceSpan {
            range: tail[0].range.start..tail[tail.len() - 1].range.end,
            style: out.root.clone(),
            owner,
            props: super::source::SpanProps::default(),
        });
        styles_clamped = true;
    }
    Limited {
        source: out,
        styles_clamped,
        text_truncated,
    }
}
