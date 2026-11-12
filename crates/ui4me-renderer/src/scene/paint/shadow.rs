//! owner: 010-paint-render — created by 010 T001; box-shadow subset of T073 (FR-040, AD-20)
//!
//! `box-shadow` as semantic `DrawBoxShadow` items (one per layer; the caller walks the list in
//! reverse so the first layer paints on top). Outer shadows are clipped to the outside of the
//! border box and inset shadows to the inside of the padding box by the raster backend.

use crate::scene::appendix_e::{Builder, Pending};
use crate::scene::appendix_e::{PaintBox, PaintPhase};
use crate::scene::borders::inset_rrect;
use crate::scene::display_list::{
    ClipShape, DrawOp, ItemKind, Rect, RoundedRect, ScopeId, ScopeKind,
};
use crate::scene::inputs::BoxShadowSpec;

/// Append the item for one shadow layer of `bx`. A fully transparent layer or one whose shape
/// is empty paints nothing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_box_shadow(
    b: &mut Builder,
    bx: &PaintBox,
    rr: &RoundedRect,
    widths: [f32; 4],
    s: &BoxShadowSpec,
    out: &mut Vec<Pending>,
    phase: PaintPhase,
    scope: Option<ScopeId>,
) {
    if s.color.a <= 0.0
        || !(s.offset_x.is_finite()
            && s.offset_y.is_finite()
            && s.blur.is_finite()
            && s.spread.is_finite())
    {
        return;
    }
    let blur = s.blur.max(0.0);
    let padding = inset_rrect(rr, widths[0], widths[1], widths[2], widths[3]);
    let ink = if s.inset {
        padding.rect
    } else {
        let grow = s.spread.max(0.0) + blur * 1.5;
        Rect {
            x: rr.rect.x + s.offset_x - grow,
            y: rr.rect.y + s.offset_y - grow,
            width: rr.rect.width + 2.0 * grow,
            height: rr.rect.height + 2.0 * grow,
        }
    };
    let [open_start, open_end] = bx.extras.slice_open;
    if open_start || open_end {
        // `box-decoration-break: slice` (CSS Break 3 section 5.3): the shadow is that of the whole box laid
        // out as one strip, cut at the break. The shape continues across an open side (square corners
        // there) and the piece paints only what lies inside its own edge on that side.
        const CONTINUE: f32 = 20_000.0;
        let extend = |r: &RoundedRect| {
            let mut r = *r;
            if open_start {
                r.rect.x -= CONTINUE;
                r.rect.width += CONTINUE;
                r.radii[0] = (0.0, 0.0);
                r.radii[3] = (0.0, 0.0);
            }
            if open_end {
                r.rect.width += CONTINUE;
                r.radii[1] = (0.0, 0.0);
                r.radii[2] = (0.0, 0.0);
            }
            r
        };
        let reach = s.offset_x.abs().max(s.offset_y.abs()) + s.spread.abs() + blur * 3.0 + 1.0;
        let x0 = if open_start {
            rr.rect.x
        } else {
            rr.rect.x - reach
        };
        let x1 = if open_end {
            rr.rect.right()
        } else {
            rr.rect.right() + reach
        };
        let clip = Rect {
            x: x0,
            y: rr.rect.y - reach,
            width: (x1 - x0).max(0.0),
            height: rr.rect.height + 2.0 * reach,
        };
        let piece_scope = Some(b.scope(scope, ScopeKind::Clip(ClipShape::Rect(clip))));
        out.push(Pending {
            owner: bx.node,
            phase,
            kind: ItemKind::BOX_SHADOW,
            ink: Rect {
                x: ink.x.max(clip.x),
                y: ink.y.max(clip.y),
                width: (ink.right().min(clip.right()) - ink.x.max(clip.x)).max(0.0),
                height: (ink.bottom().min(clip.bottom()) - ink.y.max(clip.y)).max(0.0),
            },
            scope: piece_scope,
            op: DrawOp::DrawBoxShadow {
                border_rrect: extend(rr),
                padding_rrect: extend(&padding),
                offset: (s.offset_x, s.offset_y),
                blur_radius: blur,
                spread: s.spread,
                color: s.color,
                inset: s.inset,
            },
        });
        return;
    }
    out.push(Pending {
        owner: bx.node,
        phase,
        kind: ItemKind::BOX_SHADOW,
        ink,
        scope,
        op: DrawOp::DrawBoxShadow {
            border_rrect: *rr,
            padding_rrect: padding,
            offset: (s.offset_x, s.offset_y),
            blur_radius: blur,
            spread: s.spread,
            color: s.color,
            inset: s.inset,
        },
    });
}
