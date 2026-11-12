//! owner: 015 WM-1 (lane L-WM; plan §A5)
//!
//! Vertical writing modes (CSS Writing Modes 4). Every formatting algorithm runs in one **logical
//! frame**: x goes from line-left to line-right, y from block-start to block-end. A box in a vertical
//! writing mode has its computed values transposed into that frame when its [`BoxStyle`] is mapped
//! ([`to_logical`]): `width`/`height`, the `min-`/`max-` sizes, the four sides of `margin`, `padding`,
//! `border` and the insets, `overflow-x`/`-y`, `border-spacing` and `aspect-ratio`. Values that are
//! already inline/block-relative (`flex-direction`, grid tracks, alignment, line-relative `float` and
//! `text-align`) are unchanged. A box whose writing mode is orthogonal to its parent's has an outer
//! style in the parent's frame ([`relayer`]) and is laid out through a transposed `LayoutInput`
//! (`coordinator/host_tree.rs`).
//!
//! After the fragment tree is built in logical coordinates (each fragment's origin in its parent's
//! frame, its size, rects and edges in its own), [`physicalize`] maps it to physical coordinates. A
//! frame of logical size `W x H` maps a rect `(x, y, w, h)` to
//!
//! | mode | physical rect |
//! |---|---|
//! | `horizontal-tb` | `(x, y, w, h)` |
//! | `vertical-rl`, `sideways-rl` | `(H - y - h, x, h, w)` |
//! | `vertical-lr` | `(y, x, h, w)` |
//! | `sideways-lr` | `(y, W - x - w, h, w)` |
//!
//! The content of a line box uses the line's own frame: line-over is the right side in both
//! `vertical-*` modes (so `vertical-lr` lines use the `vertical-rl` frame inside), the left side in
//! `sideways-lr`.

pub mod physical;

pub use crate::style::view::groups::{TextOrientation, WritingMode};

use crate::layout::box_tree::{BoxStyle, Sides};

/// The orientation of glyphs drawn in a line of a given writing mode (WM-1a: every glyph is drawn
/// sideways; WM-1b resolves `text-orientation`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineOrientation {
    /// Horizontal text.
    #[default]
    Horizontal,
    /// The line runs top to bottom, line-over on the right: glyphs are turned 90 degrees clockwise.
    Clockwise,
    /// The line runs bottom to top, line-over on the left (`sideways-lr`): glyphs are turned 90 degrees
    /// counter-clockwise.
    CounterClockwise,
}

impl LineOrientation {
    pub fn of(wm: WritingMode) -> Self {
        match wm {
            WritingMode::HorizontalTb => LineOrientation::Horizontal,
            WritingMode::SidewaysLr => LineOrientation::CounterClockwise,
            _ => LineOrientation::Clockwise,
        }
    }

    /// The frame the content of a line of this orientation is mapped with.
    pub fn frame(self) -> WritingMode {
        match self {
            LineOrientation::Horizontal => WritingMode::HorizontalTb,
            LineOrientation::Clockwise => WritingMode::VerticalRl,
            LineOrientation::CounterClockwise => WritingMode::SidewaysLr,
        }
    }
}

/// A line box of a vertical writing mode (015 WM-1): how its content maps from the paragraph's logical
/// frame (glyph positions, text and inline-box extents) to the line fragment's physical rect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineFrame {
    pub orientation: LineOrientation,
    /// The line's rect in paragraph space (logical; relative to the inline root's content box).
    pub logical: ui4me_engine_types::geometry::LayoutRect,
}

impl LineFrame {
    fn l(&self) -> (f32, f32, f32, f32) {
        let r = self.logical;
        (
            r.origin.x.to_f64() as f32,
            r.origin.y.to_f64() as f32,
            r.size.width.to_f64() as f32,
            r.size.height.to_f64() as f32,
        )
    }

    /// The physical position, relative to the line fragment's origin, of the paragraph-space point `(x, y)`.
    pub fn point(&self, x: f32, y: f32) -> (f32, f32) {
        let (lx, ly, lw, lh) = self.l();
        let (u, v) = (x - lx, y - ly);
        match self.orientation {
            LineOrientation::Horizontal => (u, v),
            LineOrientation::Clockwise => (lh - v, u),
            LineOrientation::CounterClockwise => (v, lw - u),
        }
    }

    /// The paragraph-space inline extent `(x0, x1)` of the physical span `[a, b)` along the line's inline axis
    /// (physical y for a vertical line), relative to the line fragment's origin.
    pub fn inline_extent(&self, a: f32, b: f32) -> (f32, f32) {
        let (lx, _, lw, _) = self.l();
        match self.orientation {
            LineOrientation::Horizontal | LineOrientation::Clockwise => (lx + a, lx + b),
            LineOrientation::CounterClockwise => (lx + lw - b, lx + lw - a),
        }
    }
}

/// The physical sides of a value as the sides of the logical frame of `wm` (top = block-start,
/// left = line-left).
pub fn sides_to_logical<T>(wm: WritingMode, s: Sides<T>) -> Sides<T> {
    let Sides {
        top,
        right,
        bottom,
        left,
    } = s;
    match wm {
        WritingMode::HorizontalTb => Sides {
            top,
            right,
            bottom,
            left,
        },
        WritingMode::VerticalRl | WritingMode::SidewaysRl => Sides {
            top: right,
            right: bottom,
            bottom: left,
            left: top,
        },
        WritingMode::VerticalLr => Sides {
            top: left,
            right: bottom,
            bottom: right,
            left: top,
        },
        WritingMode::SidewaysLr => Sides {
            top: left,
            right: top,
            bottom: right,
            left: bottom,
        },
    }
}

/// The inverse of [`sides_to_logical`].
pub fn sides_to_physical<T>(wm: WritingMode, s: Sides<T>) -> Sides<T> {
    let Sides {
        top,
        right,
        bottom,
        left,
    } = s;
    match wm {
        WritingMode::HorizontalTb => Sides {
            top,
            right,
            bottom,
            left,
        },
        WritingMode::VerticalRl | WritingMode::SidewaysRl => Sides {
            top: left,
            right: top,
            bottom: right,
            left: bottom,
        },
        WritingMode::VerticalLr => Sides {
            top: left,
            right: bottom,
            bottom: right,
            left: top,
        },
        WritingMode::SidewaysLr => Sides {
            top: right,
            right: bottom,
            bottom: left,
            left: top,
        },
    }
}

/// How far above the alphabetic baseline the central baseline of a font lies, as the reference browser
/// computes it: ascent and descent are rounded to whole pixels and the central ascent is `H - floor(H / 2)` of
/// their sum `H` (measured: H = 33 gives 17, H = 55 gives 28, pinned Chromium 1248).
pub fn central_offset(ascent: f32, descent: f32) -> f32 {
    let (a, d) = (ascent.round(), descent.round());
    let h = (a + d) as i32;
    a - (h - h / 2) as f32
}

/// The dominant baseline of a vertical line is `central` (CSS Inline 3 section 4.2): the spans of a vertical
/// paragraph whose font differs from the root's are shifted so that their central baselines line up with the
/// root's (the line breaker aligns alphabetic baselines). A span already shifted by a length keeps its shift
/// on top; the other `vertical-align` keywords are left as they are.
pub(crate) fn central_spans(
    s: &mut crate::text::paragraph::ParagraphSource,
    text: &mut crate::text::fonts::TextServices,
) {
    use crate::text::paragraph::VerticalAlignSpec as V;
    use ui4me_engine_types::text_metrics::FontMetricsSource;
    let mut memo: Vec<(crate::text::paragraph::TextStyle, f32)> = Vec::new();
    let mut delta = |st: &crate::text::paragraph::TextStyle| {
        if let Some((_, d)) = memo
            .iter()
            .find(|(k, _)| k.font == st.font && k.font_size == st.font_size)
        {
            return *d;
        }
        let r = text.metrics_service().query(&st.font, st.font_size, false);
        let d = central_offset(r.ascent, r.descent);
        memo.push((st.clone(), d));
        d
    };
    let root = delta(&s.root);
    for span in &mut s.spans {
        let d = delta(&span.style);
        if (d - root).abs() < 1e-4 {
            continue;
        }
        span.props.valign = match span.props.valign {
            V::Baseline => V::Length(root - d),
            V::Length(l) => V::Length(l + root - d),
            v => v,
        };
    }
}

/// The glyph orientation of the text of a box (CSS Writing Modes 4 section 5.1): `sideways-*` writing modes
/// set every glyph sideways, `vertical-*` follow `text-orientation`.
pub fn glyph_orientation(style: &BoxStyle) -> crate::text::paragraph::GlyphOrientation {
    use crate::text::paragraph::GlyphOrientation as O;
    match style.writing_mode {
        WritingMode::HorizontalTb => O::Horizontal,
        WritingMode::SidewaysRl | WritingMode::SidewaysLr => O::Sideways,
        WritingMode::VerticalRl | WritingMode::VerticalLr => match style.text_orientation {
            TextOrientation::Mixed => O::Mixed,
            TextOrientation::Upright => O::Upright,
            TextOrientation::Sideways => O::Sideways,
        },
    }
}

/// Two writing modes are orthogonal when one is horizontal and the other vertical.
pub fn orthogonal(a: WritingMode, b: WritingMode) -> bool {
    a.is_vertical() != b.is_vertical()
}

fn swap_sides<T>(wm: WritingMode, s: &mut Sides<T>)
where
    T: Clone,
{
    let v = Sides {
        top: s.top.clone(),
        right: s.right.clone(),
        bottom: s.bottom.clone(),
        left: s.left.clone(),
    };
    *s = sides_to_logical(wm, v);
}

/// Transpose the physical values of a mapped style into the logical frame of its own writing mode
/// (no change for `horizontal-tb`). Called once, where the computed values are mapped.
pub fn to_logical(style: &mut BoxStyle) {
    let wm = style.writing_mode;
    if !wm.is_vertical() {
        return;
    }
    transpose(style, wm, true);
}

/// The values of `style` (in the logical frame of `from`) in the logical frame of `to`: the outer
/// style of a box in an orthogonal flow, as its parent's formatting algorithm reads it.
pub fn relayer(style: &BoxStyle, from: WritingMode, to: WritingMode) -> BoxStyle {
    let mut s = style.clone();
    if from == to {
        return s;
    }
    // Back to physical, then into `to`.
    s.margin = sides_to_logical(to, sides_to_physical(from, s.margin));
    s.padding = sides_to_logical(to, sides_to_physical(from, s.padding));
    s.border = sides_to_logical(to, sides_to_physical(from, s.border));
    s.inset = sides_to_logical(to, sides_to_physical(from, s.inset));
    if orthogonal(from, to) {
        swap_axes(&mut s);
    }
    s
}

fn swap_axes(s: &mut BoxStyle) {
    std::mem::swap(&mut s.width, &mut s.height);
    std::mem::swap(&mut s.min_width, &mut s.min_height);
    std::mem::swap(&mut s.max_width, &mut s.max_height);
    std::mem::swap(&mut s.overflow_x, &mut s.overflow_y);
    // `border-spacing` is logical already (inline-axis spacing first, as the reference browser applies it).
    s.aspect_ratio = s.aspect_ratio.map(|r| if r > 0.0 { 1.0 / r } else { r });
    // The intrinsic width keywords name the physical width: in a vertical frame that is the block
    // axis, where they are not supported (reported by the mapping).
    s.width_kw = None;
    s.min_width_kw = None;
}

fn transpose(s: &mut BoxStyle, wm: WritingMode, _own: bool) {
    swap_sides(wm, &mut s.margin);
    swap_sides(wm, &mut s.padding);
    swap_sides(wm, &mut s.border);
    swap_sides(wm, &mut s.inset);
    swap_axes(s);
}

/// The layout of an orthogonal flow root (given in its parent's frame by the parent's algorithm) in the
/// node's own frame: size and the sides of margin, border and padding.
pub(crate) fn layout_to_own(l: &mut taffy::Layout, parent: WritingMode, own: WritingMode) {
    let r = |v: taffy::Rect<f32>| {
        let p = sides_to_physical(
            parent,
            Sides {
                top: v.top,
                right: v.right,
                bottom: v.bottom,
                left: v.left,
            },
        );
        let o = sides_to_logical(own, p);
        taffy::Rect {
            top: o.top,
            right: o.right,
            bottom: o.bottom,
            left: o.left,
        }
    };
    l.margin = r(l.margin);
    l.border = r(l.border);
    l.padding = r(l.padding);
    if orthogonal(parent, own) {
        l.size = taffy::Size {
            width: l.size.height,
            height: l.size.width,
        };
    }
}

/// The size of a box given in the logical frame of `wm`, physically (`(w, h)` swapped in a vertical
/// mode).
pub fn physical_size<T>(wm: WritingMode, w: T, h: T) -> (T, T) {
    if wm.is_vertical() { (h, w) } else { (w, h) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides_round_trip_in_every_mode() {
        for wm in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            let s = Sides {
                top: 1,
                right: 2,
                bottom: 3,
                left: 4,
            };
            let back = sides_to_physical(wm, sides_to_logical(wm, s));
            assert_eq!(
                (back.top, back.right, back.bottom, back.left),
                (1, 2, 3, 4),
                "{wm:?}"
            );
        }
    }

    #[test]
    fn vertical_rl_block_start_is_the_right_side_and_line_left_the_top() {
        let l = sides_to_logical(
            WritingMode::VerticalRl,
            Sides {
                top: "t",
                right: "r",
                bottom: "b",
                left: "l",
            },
        );
        assert_eq!((l.top, l.left, l.bottom, l.right), ("r", "t", "l", "b"));
        let l = sides_to_logical(
            WritingMode::VerticalLr,
            Sides {
                top: "t",
                right: "r",
                bottom: "b",
                left: "l",
            },
        );
        assert_eq!((l.top, l.left, l.bottom, l.right), ("l", "t", "r", "b"));
        let l = sides_to_logical(
            WritingMode::SidewaysLr,
            Sides {
                top: "t",
                right: "r",
                bottom: "b",
                left: "l",
            },
        );
        assert_eq!((l.top, l.left, l.bottom, l.right), ("l", "b", "r", "t"));
    }
}
