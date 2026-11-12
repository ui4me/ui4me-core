//! owner: 009-text-inline
//!
//! DOM UTF-16 offsets <-> processed text map (T015; FR-017, FR-040, FR-083; OD-22; research R-04).
//!
//! The inline behaviour records one [`Segment`] per piece of the processed text, in text order,
//! covering `0..text.len()` with no gap. A segment is
//!
//! * `Copied`: DOM text copied unchanged (equal UTF-16 length, 1:1 mapping inside);
//! * `Collapsed` / `Transformed`: DOM text replaced by shorter, longer or different text (white
//!   space collapsing, case mapping); only the segment ends are exact, interior positions snap to
//!   the nearest end;
//! * `Inserted`: text with no DOM origin (bidi controls, edge spacers, `<wbr>` U+200B, generated
//!   content); positions inside snap to the neighbouring DOM boundary.
//!
//! Both directions are total over every character boundary (OD-22: UTF-16 code units, a
//! surrogate pair is never split) and monotonic per node. No upstream type appears here.

use super::source::{Affinity, TextPosition};
use std::ops::Range;
use ui4me_engine_types::ids::NodeId;

/// A range of UTF-16 code units of one DOM text node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomRange {
    pub node: NodeId,
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Copied,
    Collapsed,
    Transformed,
    Inserted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// UTF-8 byte range of the processed text.
    pub processed: Range<usize>,
    pub dom: Option<DomRange>,
    pub kind: SegmentKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OffsetMapError {
    /// Segments do not cover the text contiguously from 0 to its length.
    NotContiguous,
    /// A segment boundary is not on a character boundary.
    NotCharBoundary,
    /// `Inserted` with a DOM range, or any other kind without one.
    KindMismatch,
    /// A `Copied` segment whose UTF-16 length differs from its DOM range, or an inverted range.
    LengthMismatch,
    /// DOM ranges of one node go backwards.
    NotMonotonic,
}

impl std::fmt::Display for OffsetMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid offset map: {self:?}")
    }
}

impl std::error::Error for OffsetMapError {}

/// The total, monotonic map of one paragraph.
#[derive(Clone, Debug)]
pub struct OffsetMap {
    text: Box<str>,
    segments: Vec<Segment>,
    /// Indices of segments with a DOM range, sorted by `(node, dom.start)`.
    by_node: Vec<usize>,
}

fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

impl OffsetMap {
    pub fn new(text: &str, segments: Vec<Segment>) -> Result<Self, OffsetMapError> {
        let mut at = 0usize;
        for s in &segments {
            if s.processed.start != at
                || s.processed.end < s.processed.start
                || s.processed.end > text.len()
            {
                return Err(OffsetMapError::NotContiguous);
            }
            if !text.is_char_boundary(s.processed.start) || !text.is_char_boundary(s.processed.end)
            {
                return Err(OffsetMapError::NotCharBoundary);
            }
            match (s.kind, &s.dom) {
                (SegmentKind::Inserted, None) => {}
                (SegmentKind::Inserted, Some(_)) | (_, None) => {
                    return Err(OffsetMapError::KindMismatch);
                }
                (kind, Some(d)) => {
                    if d.end < d.start {
                        return Err(OffsetMapError::LengthMismatch);
                    }
                    if kind == SegmentKind::Copied
                        && utf16_len(&text[s.processed.clone()]) != d.end - d.start
                    {
                        return Err(OffsetMapError::LengthMismatch);
                    }
                }
            }
            at = s.processed.end;
        }
        if at != text.len() {
            return Err(OffsetMapError::NotContiguous);
        }
        let mut by_node: Vec<usize> = (0..segments.len())
            .filter(|&i| segments[i].dom.is_some())
            .collect();
        by_node.sort_by_key(|&i| {
            let d = segments[i].dom.as_ref().unwrap();
            (d.node, d.start, i)
        });
        for w in by_node.windows(2) {
            let (a, b) = (
                segments[w[0]].dom.as_ref().unwrap(),
                segments[w[1]].dom.as_ref().unwrap(),
            );
            if a.node == b.node && b.start < a.end {
                return Err(OffsetMapError::NotMonotonic);
            }
        }
        // Processed order must agree with DOM order for each node.
        let mut last: Vec<(NodeId, u32)> = Vec::new();
        for s in &segments {
            if let Some(d) = &s.dom {
                match last.iter_mut().find(|(n, _)| *n == d.node) {
                    Some((_, e)) => {
                        if d.start < *e {
                            return Err(OffsetMapError::NotMonotonic);
                        }
                        *e = d.end;
                    }
                    None => last.push((d.node, d.end)),
                }
            }
        }
        Ok(OffsetMap {
            text: text.into(),
            segments,
            by_node,
        })
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The segment index holding `byte`: at a shared boundary `Downstream` takes the segment
    /// that starts there and `Upstream` the one that ends there.
    fn segment_at(&self, byte: usize, aff: Affinity) -> Option<usize> {
        if self.segments.is_empty() {
            return None;
        }
        // First segment whose end is > byte (Downstream) or >= byte (Upstream).
        let i = match aff {
            Affinity::Downstream => self.segments.partition_point(|s| s.processed.end <= byte),
            Affinity::Upstream => self.segments.partition_point(|s| s.processed.end < byte),
        };
        // Skip empty segments that would shadow the boundary choice; clamp to the last segment.
        Some(i.min(self.segments.len() - 1))
    }

    fn floor_char_boundary(&self, byte: usize) -> usize {
        let mut b = byte.min(self.text.len());
        while !self.text.is_char_boundary(b) {
            b -= 1;
        }
        b
    }

    /// Processed byte offset -> DOM position. `None` only when the paragraph has no DOM text.
    pub fn processed_to_dom(&self, byte: usize, aff: Affinity) -> Option<TextPosition> {
        let byte = self.floor_char_boundary(byte);
        let i = self.segment_at(byte, aff)?;
        let seg = &self.segments[i];
        let pos = |d: &DomRange, offset: u32| TextPosition {
            node: d.node,
            offset,
            affinity: aff,
        };
        match (&seg.dom, seg.kind) {
            (Some(d), SegmentKind::Copied) => {
                let inside = byte.clamp(seg.processed.start, seg.processed.end);
                Some(pos(
                    d,
                    d.start + utf16_len(&self.text[seg.processed.start..inside]),
                ))
            }
            (Some(d), _) => {
                let (s, e) = (seg.processed.start, seg.processed.end);
                let byte = byte.clamp(s, e);
                let to_start = byte - s;
                let to_end = e - byte;
                let at_start = match to_start.cmp(&to_end) {
                    std::cmp::Ordering::Less => true,
                    std::cmp::Ordering::Greater => false,
                    std::cmp::Ordering::Equal => aff == Affinity::Upstream,
                };
                Some(pos(d, if at_start { d.start } else { d.end }))
            }
            (None, _) => {
                let prev = self.segments[..i].iter().rev().find_map(|s| s.dom.as_ref());
                let next = self.segments[i + 1..].iter().find_map(|s| s.dom.as_ref());
                match (aff, prev, next) {
                    (Affinity::Upstream, Some(p), _) => Some(pos(p, p.end)),
                    (Affinity::Upstream, None, Some(n)) => Some(pos(n, n.start)),
                    (Affinity::Downstream, _, Some(n)) => Some(pos(n, n.start)),
                    (Affinity::Downstream, Some(p), None) => Some(pos(p, p.end)),
                    (_, None, None) => None,
                }
            }
        }
    }

    /// DOM position (node, UTF-16 offset) -> processed byte offset on a character boundary.
    /// An offset inside a surrogate pair snaps down to the pair start. `None` for an unknown
    /// node or an offset past the node's last mapped unit.
    pub fn dom_to_processed(&self, node: NodeId, offset: u32, aff: Affinity) -> Option<usize> {
        let lo = self
            .by_node
            .partition_point(|&i| self.segments[i].dom.as_ref().unwrap().node < node);
        let hi = self
            .by_node
            .partition_point(|&i| self.segments[i].dom.as_ref().unwrap().node <= node);
        let cands = &self.by_node[lo..hi];
        let dom = |i: usize| self.segments[i].dom.as_ref().unwrap();
        let chosen = match aff {
            Affinity::Downstream => cands
                .iter()
                .copied()
                .find(|&i| dom(i).start <= offset && offset < dom(i).end)
                .or_else(|| cands.iter().copied().rev().find(|&i| dom(i).end == offset)),
            Affinity::Upstream => cands
                .iter()
                .copied()
                .rev()
                .find(|&i| dom(i).start < offset && offset <= dom(i).end)
                .or_else(|| cands.iter().copied().find(|&i| dom(i).start == offset)),
        }?;
        let seg = &self.segments[chosen];
        let d = seg.dom.as_ref().unwrap();
        Some(match seg.kind {
            SegmentKind::Copied => {
                let want = offset - d.start;
                let mut units = 0u32;
                let mut byte = seg.processed.start;
                for c in self.text[seg.processed.clone()].chars() {
                    let w = c.len_utf16() as u32;
                    if units + w > want {
                        break; // `want` falls inside this character (pair): snap down
                    }
                    units += w;
                    byte += c.len_utf8();
                }
                byte
            }
            _ => {
                let to_start = offset - d.start;
                let to_end = d.end - offset;
                let at_start = match to_start.cmp(&to_end) {
                    std::cmp::Ordering::Less => true,
                    std::cmp::Ordering::Greater => false,
                    std::cmp::Ordering::Equal => aff == Affinity::Upstream,
                };
                if at_start {
                    seg.processed.start
                } else {
                    seg.processed.end
                }
            }
        })
    }

    /// Snap a processed byte offset to the nearest boundary in `boundaries` (sorted grapheme
    /// cluster starts plus the text length, from the shaping analysis); ties go to the earlier
    /// one for `Upstream` and the later one for `Downstream`.
    pub fn snap_to_boundary(boundaries: &[usize], byte: usize, aff: Affinity) -> usize {
        if boundaries.is_empty() {
            return byte;
        }
        let i = boundaries.partition_point(|&b| b < byte);
        match (
            i.checked_sub(1).map(|k| boundaries[k]),
            boundaries.get(i).copied(),
        ) {
            (Some(a), Some(b)) if a != byte && b != byte => {
                let (da, db) = (byte - a, b - byte);
                if da < db || (da == db && aff == Affinity::Upstream) {
                    a
                } else {
                    b
                }
            }
            (_, Some(b)) => b,
            (Some(a), None) => a,
            (None, None) => byte,
        }
    }
}
