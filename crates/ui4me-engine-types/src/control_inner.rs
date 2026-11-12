//! owner: 012-interaction with 008 (lane L-EDIT-r4) — the published geometry of a text control's
//! inner editing area (`input`, `textarea`). Layout builds it (`ui4me-layout/src/replaced/inner.rs`),
//! the scene paints the caret and the selection from it and 012 edits with it; it lives here so
//! that none of those crates has to depend on another. Offsets are UTF-16 offsets into the
//! control's *value* (invariant 14), never into the displayed text (a password is masked).

use crate::geometry::LayoutRect;

/// One line of a text control's inner editing area, in CSS px relative to the content box
/// (`ControlInner::viewport`) and before any scroll offset.
#[derive(Clone, Debug, PartialEq)]
pub struct InnerLine {
    pub top: f32,
    pub height: f32,
    pub baseline: f32,
    /// The value's UTF-16 range shown on the line (a trailing line break is not part of it).
    pub start: u32,
    pub end: u32,
    /// Caret stops: (UTF-16 offset into the value, x), ascending, from `start` to `end`.
    pub stops: Vec<(u32, f32)>,
    /// The line holds right-to-left text (or is a right-to-left paragraph): the stops are in
    /// logical order with the x of each caret position, which is not ascending (lane L-BIDI-r14).
    pub bidi: bool,
    /// On a bidi line, the extent of each grapheme cluster: (UTF-16 start, UTF-16 end, x0, x1,
    /// right-to-left), in logical order, x0 < x1. Empty on a plain left-to-right line.
    pub clusters: Vec<(u32, u32, f32, f32, bool)>,
}

/// Rows a drop-down's popup shows at once before it scrolls.
pub const POPUP_MAX_ROWS: usize = 20;

/// One row of a `select`'s popup or listbox, in CSS px relative to the control's border box.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectRowGeom {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Index in the select's list of options (`None` for an `optgroup` label row).
    pub option: Option<u32>,
    pub selected: bool,
    pub highlighted: bool,
    pub disabled: bool,
}

/// What a `select` publishes besides its one displayed line (lane L-CTRL-r10): the rows of its
/// open popup (a layered panel below the control, painted by the scene in the same window; 011 has
/// no separate popup windows) or of its listbox.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectGeom {
    /// The rows are those of a dropdown's open popup (else of a listbox).
    pub popup: bool,
    /// The popup panel (x, y, width, height), relative to the border box; zero for a listbox.
    pub panel: [f32; 4],
    pub rows: Vec<SelectRowGeom>,
    /// Rows visible at once (listbox) or in the popup.
    pub visible_rows: u32,
    /// Total rows (all options and group labels, also those scrolled out).
    pub total_rows: u32,
}

/// Geometry of a text control's inner editing area (the box's content box is its scrollport).
#[derive(Clone, Debug, PartialEq)]
pub struct ControlInner {
    /// A `select`'s rows (`None` for text controls).
    pub select: Option<Box<SelectGeom>>,
    pub multiline: bool,
    /// The placeholder is shown (the value is empty); the lines then describe an empty value.
    pub placeholder_shown: bool,
    pub password: bool,
    /// The content box, relative to the control's border box.
    pub viewport: LayoutRect,
    /// Index into `FragmentTree::paragraphs` of the shown text (value or placeholder), if any.
    pub paragraph_index: Option<usize>,
    /// Always at least one line (an empty value has one empty line).
    pub lines: Vec<InnerLine>,
    /// Used `line-height` of the control's font.
    pub line_height: f32,
    /// Extent of the text, including the room of a caret at its end.
    pub content_width: f32,
    pub content_height: f32,
}

impl ControlInner {
    /// Index of the line a caret at value offset `offset` sits on: with `upstream`, the first
    /// line that ends there (end of a soft-wrapped line), else the last line that starts there.
    pub fn line_of(&self, offset: u32, upstream: bool) -> usize {
        let hit = |l: &InnerLine| l.start <= offset && offset <= l.end;
        let found = if upstream {
            self.lines.iter().position(hit)
        } else {
            self.lines.iter().rposition(hit)
        };
        found.unwrap_or_else(|| {
            if offset < self.lines.first().map_or(0, |l| l.start) {
                0
            } else {
                self.lines.len() - 1
            }
        })
    }

    /// The x of the caret stop at `offset` on `line` (the last stop at or before it).
    pub fn x_of(&self, line: usize, offset: u32) -> f32 {
        let l = &self.lines[line.min(self.lines.len() - 1)];
        match l.stops.iter().rev().find(|(o, _)| *o <= offset) {
            Some((_, x)) => *x,
            None => l.stops.first().map_or(0.0, |s| s.1),
        }
    }

    /// The horizontal intervals, left to right, that the value range `a..b` covers on `line`
    /// (merged where they touch): one on plain left-to-right text, several where the range crosses
    /// a change of direction.
    pub fn range_spans(&self, line: usize, a: u32, b: u32) -> Vec<(f32, f32)> {
        let l = &self.lines[line.min(self.lines.len() - 1)];
        if l.clusters.is_empty() {
            let (x0, x1) = (self.x_of(line, a), self.x_of(line, b));
            return vec![(x0.min(x1), x0.max(x1))];
        }
        let mut spans: Vec<(f32, f32)> = l
            .clusters
            .iter()
            .filter(|c| c.0 >= a && c.1 <= b && c.1 > c.0)
            .map(|c| (c.2, c.3))
            .collect();
        spans.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut merged: Vec<(f32, f32)> = Vec::new();
        for s in spans {
            match merged.last_mut() {
                Some(last) if s.0 <= last.1 + 0.01 => last.1 = last.1.max(s.1),
                _ => merged.push(s),
            }
        }
        merged
    }

    /// The value offset nearest to `x` on `line`. On a bidi line the cluster under `x` decides:
    /// the half of it nearer to `x` picks its leading or trailing offset, so the visual left half
    /// of a right-to-left cluster is its end (F0503); elsewhere the nearest caret stop wins.
    pub fn offset_at(&self, line: usize, x: f32) -> u32 {
        let l = &self.lines[line.min(self.lines.len() - 1)];
        let nearest = l.clusters.iter().min_by(|a, b| {
            let d = |c: &&(u32, u32, f32, f32, bool)| (c.2 - x).max(x - c.3).max(0.0);
            d(a).total_cmp(&d(b))
        });
        if let Some(&(s, e, x0, x1, rtl)) = nearest {
            let right_half = x >= (x0 + x1) / 2.0;
            return if right_half == rtl { s } else { e };
        }
        let mut best = (l.start, f32::INFINITY);
        for (o, sx) in &l.stops {
            let d = (sx - x).abs();
            if d < best.1 {
                best = (*o, d);
            }
        }
        best.0
    }

    /// The line whose vertical band contains `y` (clamped to the first and last line).
    pub fn line_at_y(&self, y: f32) -> usize {
        let mut idx = 0;
        for (i, l) in self.lines.iter().enumerate() {
            if y >= l.top {
                idx = i;
            }
        }
        idx
    }
}
