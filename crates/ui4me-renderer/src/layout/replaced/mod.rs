//! owner: 008-layout-engine (L-INL-r2) — replaced elements and form-control boxes (FR-067–FR-075;
//! `contracts/box-tree.md` §6; 012 `contracts/controls.md` §4).
//!
//! A replaced box is a leaf with a natural geometry ([`Natural`]) and no formatting context of its
//! own. [`used_content_size`] is the CSS 2.1 §10.3.2 / §10.3.4 / §10.6.2 used-size algorithm with
//! the §10.4 constraint-violation table; the host tree applies it to image, embedded-content and
//! control boxes alike (controls differ only in their natural size and baseline, `controls.rs`).

pub mod controls;
pub mod embedded;
pub mod images;
pub mod inner;

use crate::text::paragraph::LineHeightSpec;

/// The kind of content a replaced box holds.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplacedKind {
    Image(images::ImageInfo),
    Embedded(embedded::Embedded),
    Control(controls::ControlSpec),
    /// The drawn bullet of a `disc`, `circle` or `square` list marker (FR-083): the reference
    /// browser paints these as shapes, not glyphs (`marker_symbol`).
    MarkerSymbol {
        shape: MarkerShape,
        failed_image: bool,
    },
}

/// The shape of a bullet marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerShape {
    Disc,
    Circle,
    Square,
    DisclosureOpen,
    DisclosureClosedLtr,
    DisclosureClosedRtl,
}

/// The geometry of a drawn bullet marker, in whole pixels as the reference browser computes it
/// (`ListMarker::WidthOfSymbol`, `RelativeSymbolMarkerRect`) from the primary font's rounded
/// ascent: the bullet is `(2/3 * ascent + 1) / 2` px square, one pixel in from the left of a box
/// that is 2 px wider, `3 * (ascent - 2/3 * ascent) / 2` px below the top of an `ascent`-tall
/// square that rests on the baseline.
///
/// The layout box rests on the baseline and is as tall as the marker's inline box is above it: the
/// font's rounded ascent plus half the leading of the marker's `line-height`, as for text of that
/// font. A marker in the font and line height of its item thereby never makes the line taller; a
/// larger `::marker` font does, as it does for the reference browser's marker.
pub fn marker_symbol_metrics(
    ascent: f32,
    descent: f32,
    line_gap: f32,
    line_height: LineHeightSpec,
    font_size: f32,
) -> MarkerSymbolMetrics {
    let (asc, desc) = (
        ascent.round().max(0.0) as i32,
        descent.round().max(0.0) as i32,
    );
    let two_thirds = asc * 2 / 3;
    let bullet = (two_thirds + 1) / 2;
    let top = 3 * (asc - two_thirds) / 2;
    // The inline box above the baseline is computed as for text of this font: whole-pixel ascent
    // and descent, the half-leading floored (as the reference browser does), so that it equals the
    // item's own strut when the fonts agree.
    let content = (asc + desc) as f32;
    let used = match line_height {
        LineHeightSpec::Normal => content + line_gap,
        LineHeightSpec::Number(n) => n * font_size,
        LineHeightSpec::Px(p) => p,
    };
    let above = asc as f32 + ((used - content) / 2.0).floor();
    let height = above.max(0.0);
    MarkerSymbolMetrics {
        bullet: bullet as f32,
        two_thirds: two_thirds as f32,
        height,
        // From the box top down to the bullet's top: the bullet is `asc - top` above the baseline.
        bullet_y: height - (asc - top) as f32,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerSymbolMetrics {
    /// The bullet's edge length; the box is `bullet + 2` wide.
    pub bullet: f32,
    /// `floor(2/3 * ascent)`, which places an outside marker.
    pub two_thirds: f32,
    /// The height of the layout box: above the baseline, none below.
    pub height: f32,
    /// The bullet's offset below the top of the box (negative: it starts above it).
    pub bullet_y: f32,
}

/// A replaced or control box (box-tree.md §3 `Replaced`, `Control`): atomic inline-level unless
/// the element is block-level (`display: block` image).
#[derive(Clone, Debug, PartialEq)]
pub struct ReplacedContent {
    pub block_level: bool,
    pub kind: ReplacedKind,
    /// What a text control shows in its inner editing area (`inner.rs`); `None` for every other box.
    pub inner: Option<inner::InnerSource>,
}

/// Natural geometry of a replaced box in content-box CSS px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Natural {
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// Width / height of the intrinsic (or attribute, or `aspect-ratio`) ratio.
    pub ratio: Option<f32>,
    /// Baseline from the content-box top; `None` = the bottom margin edge (CSS 2.1 §10.8.1: an
    /// atomic inline without line boxes).
    pub baseline: Option<f32>,
    /// A control whose inner text line (this tall) is centred vertically when the used content
    /// height exceeds it (text controls, selects).
    pub inner_line: Option<f32>,
    /// A listbox / textarea-like control whose first line sits at the content top.
    pub first_line_at_top: bool,
}

/// Default object size of replaced content without natural dimensions (CSS 2.1 §10.3.2).
pub const DEFAULT_WIDTH: f32 = 300.0;
pub const DEFAULT_HEIGHT: f32 = 150.0;

/// What the used-size algorithm needs, all content-box CSS px (`None` = `auto`).
#[derive(Clone, Copy, Debug)]
pub struct SizeRequest {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub natural: Natural,
    /// The containing block's width, for content with an intrinsic ratio only (§10.3.2 rule 4).
    pub available_width: Option<f32>,
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.min(hi).max(lo)
}

/// Used content-box size of a replaced box.
pub fn used_content_size(r: &SizeRequest) -> (f32, f32) {
    let n = r.natural;
    let ratio = n.ratio.filter(|x| x.is_finite() && *x > 0.0);
    // Tentative size without min/max (CSS 2.1 §10.3.2 / §10.6.2).
    let (w, h) = match (r.width, r.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (
            w,
            ratio.map(|q| w / q).or(n.height).unwrap_or(DEFAULT_HEIGHT),
        ),
        (None, Some(h)) => (ratio.map(|q| h * q).or(n.width).unwrap_or(DEFAULT_WIDTH), h),
        (None, None) => match (n.width, n.height, ratio) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some(q)) => (w, w / q),
            (None, Some(h), Some(q)) => (h * q, h),
            (Some(w), None, None) => (w, DEFAULT_HEIGHT),
            (None, Some(h), None) => (DEFAULT_WIDTH, h),
            (None, None, Some(q)) => {
                let w = r
                    .available_width
                    .filter(|a| a.is_finite())
                    .unwrap_or(DEFAULT_WIDTH);
                (w, w / q)
            }
            (None, None, None) => (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        },
    };
    // Constraint violations (§10.4). The table applies to content with an intrinsic ratio unless
    // both sizes are specified (then each is clamped on its own, as browsers do).
    let one_derived = r.width.is_none() || r.height.is_none();
    if let (true, Some(_), true) = (one_derived, ratio, w > 0.0 && h > 0.0) {
        let (min_w, max_w, min_h, max_h) = (r.min_width, r.max_width, r.min_height, r.max_height);
        let (over_w, under_w, over_h, under_h) = (w > max_w, w < min_w, h > max_h, h < min_h);
        let rule = match (over_w, under_w, over_h, under_h) {
            (false, false, false, false) => (w, h),
            (true, _, true, _) => {
                if max_w / w <= max_h / h {
                    (max_w, min_h.max(max_w * h / w))
                } else {
                    (min_w.max(max_h * w / h), max_h)
                }
            }
            (_, true, _, true) => {
                if min_w / w <= min_h / h {
                    (max_w.min(min_h * w / h), min_h)
                } else {
                    (min_w, max_h.min(min_w * h / w))
                }
            }
            (_, true, true, _) => (min_w, max_h),
            (true, _, _, true) => (max_w, min_h),
            (true, _, _, _) => (max_w, min_h.max(max_w * h / w)),
            (_, true, _, _) => (min_w, max_h.min(min_w * h / w)),
            (_, _, true, _) => (min_w.max(max_h * w / h), max_h),
            (_, _, _, true) => (max_w.min(min_h * w / h), min_h),
        };
        return (rule.0.max(0.0), rule.1.max(0.0));
    }
    (
        clamp(w, r.min_width, r.max_width).max(0.0),
        clamp(h, r.min_height, r.max_height).max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(natural: Natural) -> SizeRequest {
        SizeRequest {
            width: None,
            height: None,
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
            natural,
            available_width: None,
        }
    }

    fn img(w: f32, h: f32) -> Natural {
        Natural {
            width: Some(w),
            height: Some(h),
            ratio: Some(w / h),
            ..Natural::default()
        }
    }

    #[test]
    fn natural_size_is_the_default_used_size() {
        assert_eq!(used_content_size(&req(img(400.0, 300.0))), (400.0, 300.0));
    }

    #[test]
    fn one_specified_dimension_scales_the_other_by_the_ratio() {
        let mut r = req(img(400.0, 300.0));
        r.width = Some(200.0);
        assert_eq!(used_content_size(&r), (200.0, 150.0));
        r.width = None;
        r.height = Some(60.0);
        assert_eq!(used_content_size(&r), (80.0, 60.0));
    }

    #[test]
    fn no_natural_size_is_300_by_150() {
        assert_eq!(used_content_size(&req(Natural::default())), (300.0, 150.0));
    }

    #[test]
    fn ratio_only_takes_the_containing_block_width() {
        let mut r = req(Natural {
            ratio: Some(2.0),
            ..Natural::default()
        });
        r.available_width = Some(200.0);
        assert_eq!(used_content_size(&r), (200.0, 100.0));
    }

    #[test]
    fn max_width_scales_the_height_with_the_ratio() {
        let mut r = req(img(400.0, 300.0));
        r.max_width = 200.0;
        assert_eq!(used_content_size(&r), (200.0, 150.0));
    }

    #[test]
    fn min_height_scales_the_width_with_the_ratio() {
        let mut r = req(img(40.0, 30.0));
        r.min_height = 60.0;
        assert_eq!(used_content_size(&r), (80.0, 60.0));
    }

    #[test]
    fn specified_sizes_are_clamped_independently() {
        let mut r = req(img(400.0, 300.0));
        r.width = Some(500.0);
        r.height = Some(10.0);
        r.max_width = 100.0;
        r.min_height = 20.0;
        assert_eq!(used_content_size(&r), (100.0, 20.0));
    }

    #[test]
    fn a_specified_width_over_max_width_scales_the_derived_height() {
        let mut r = req(img(400.0, 300.0));
        r.width = Some(400.0);
        r.max_width = 200.0;
        assert_eq!(used_content_size(&r), (200.0, 150.0));
    }

    #[test]
    fn both_over_the_maxima_pick_the_tighter_scale() {
        let mut r = req(img(400.0, 300.0));
        r.max_width = 200.0;
        r.max_height = 100.0;
        // 200/400 = 0.5 > 100/300 = 0.333: the height limit binds.
        let (w, h) = used_content_size(&r);
        assert!((w - 133.33333).abs() < 1e-3 && h == 100.0, "{w} {h}");
    }
}
