//! owner: 010-paint-render — created by 010 T001; overflow clipping subset of T042 (FR-070)
//!
//! `overflow` clipping: a box whose `overflow-x` / `overflow-y` is not `visible` clips its
//! descendants (not itself) to its padding box, with the inner border radius on rounded boxes
//! (CSS Backgrounds 3 section 5.3). A clip on one axis only leaves the other axis unbounded.

use crate::scene::appendix_e::{Builder, PaintBox, border_widths};
use crate::scene::borders::{fit_radii, inset_rrect};
use crate::scene::display_list::{ClipShape, Rect, RoundedRect, ScopeId, ScopeKind};
use crate::scene::inputs::UNBOUNDED;
use ui4me_engine_types::geometry::LayoutRect;

fn css_rect(r: &LayoutRect) -> Rect {
    let f = |v: ui4me_engine_types::geometry::layout_unit::LayoutUnit| v.to_f64() as f32;
    Rect {
        x: f(r.origin.x),
        y: f(r.origin.y),
        width: f(r.size.width),
        height: f(r.size.height),
    }
}

/// The clip scope of `bx`'s content under `parent`.
pub(crate) fn overflow_clip_scope(
    b: &mut Builder,
    bx: &PaintBox,
    parent: Option<ScopeId>,
) -> ScopeId {
    let e = &bx.extras;
    let border_box = css_rect(&bx.border_box);
    let w = border_widths(&bx.border);
    let outer = RoundedRect {
        rect: border_box,
        radii: fit_radii(&border_box, e.radii),
    };
    let mut padding = inset_rrect(&outer, w[0], w[1], w[2], w[3]);
    if e.clip_content_box {
        // The content box: also inside the padding (a text control's inner editing area).
        padding = inset_rrect(
            &padding,
            e.padding[0],
            e.padding[1],
            e.padding[2],
            e.padding[3],
        );
    }
    if !e.clip_x {
        padding.rect.x = -UNBOUNDED;
        padding.rect.width = 2.0 * UNBOUNDED;
        padding.radii = [(0.0, 0.0); 4];
    }
    if !e.clip_y {
        padding.rect.y = -UNBOUNDED;
        padding.rect.height = 2.0 * UNBOUNDED;
        padding.radii = [(0.0, 0.0); 4];
    }
    let shape = if padding.is_square() {
        ClipShape::Rect(padding.rect)
    } else {
        ClipShape::RoundedRect(padding)
    };
    b.scope(parent, ScopeKind::Clip(shape))
}

/// The padding or content box of `bx` as `[x0, y0, x1, y1]` (CSS px) when it clips its
/// content, with the unclipped axis unbounded; `None` for `overflow: visible` on both axes.
/// Hit testing retains this clip in the box's spatial coordinate system. Rounded inner corners
/// are not applied to overflow hit testing.
pub(crate) fn overflow_hit_rect(bx: &PaintBox) -> Option<[f64; 4]> {
    let e = &bx.extras;
    if !(e.clip_x || e.clip_y) {
        return None;
    }
    let b = css_rect(&bx.border_box);
    let w = border_widths(&bx.border);
    let (mut x0, mut x1) = (f64::from(b.x + w[3]), f64::from(b.x + b.width - w[1]));
    let (mut y0, mut y1) = (f64::from(b.y + w[0]), f64::from(b.y + b.height - w[2]));
    if e.clip_content_box {
        x0 += f64::from(e.padding[3]);
        x1 -= f64::from(e.padding[1]);
        y0 += f64::from(e.padding[0]);
        y1 -= f64::from(e.padding[2]);
    }
    if !e.clip_x {
        (x0, x1) = (f64::from(-UNBOUNDED), f64::from(UNBOUNDED));
    }
    if !e.clip_y {
        (y0, y1) = (f64::from(-UNBOUNDED), f64::from(UNBOUNDED));
    }
    Some([x0, y0, x1.max(x0), y1.max(y0)])
}
