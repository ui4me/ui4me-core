//! owner: 010-paint-render — lane L-STY-r2 (FR-032 outline subset)
//!
//! `outline` (CSS UI 4 section 3): a ring outside the border edge, `outline-offset` away from it,
//! drawn after the stacking context's content (Appendix E step 10) and not clipped by the box's
//! own `overflow`. It follows the box's `border-radius` (each non-zero radius grows by the offset
//! and the width). The ring reuses the border painter: an outline is the border of the box grown
//! by `offset + width` with the outline's style and colour on every side, so every border style
//! (solid, dotted, dashed, double, groove, ridge, inset, outset) has the same geometry. The UA
//! `outline-style: auto` is the ui4me focus ring (CSS UI 4 lets the UA choose the appearance; the
//! theme token of display-list.md section 6): the adapter resolves a solid ring, and the items
//! carry the `focus-ring` kind.

use crate::scene::appendix_e::{Builder, PaintBox, PaintPhase, Pending};
use crate::scene::borders::{
    BorderSide, BorderSides, BorderStyle, fit_radii, rounded_border_pieces, side_fills,
};
use crate::scene::display_list::{
    ClipShape, DrawOp, FillRule, ItemKind, Paint, Rect, ScopeId, ScopeKind,
};
use ui4me_engine_types::geometry::layout_unit::LayoutUnit;

/// Append the outline items of `bx` (nothing for a zero width, a `none` style or a transparent colour).
pub(crate) fn push_outline(
    b: &mut Builder,
    bx: &PaintBox,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
) {
    let Some(o) = &bx.extras.outline else { return };
    let kind = if o.auto {
        ItemKind::FOCUS_RING
    } else {
        ItemKind::OUTLINE
    };
    if o.style == BorderStyle::None
        || !(o.width > 0.0 && o.width.is_finite() && o.offset.is_finite())
        || o.color.a <= 0.0
    {
        return;
    }
    // A piece of a box broken across fragmentainers (015 FRAG-2c) has an outline of its own around the
    // piece (the reference browser outlines every fragment), not the whole box's cut to the piece.
    let bb = bx.extras.fragment_clip.unwrap_or(Rect {
        x: bx.border_box.origin.x.to_f64() as f32,
        y: bx.border_box.origin.y.to_f64() as f32,
        width: bx.border_box.size.width.to_f64() as f32,
        height: bx.border_box.size.height.to_f64() as f32,
    });
    let grow = o.offset + o.width;
    let outer = Rect {
        x: bb.x - grow,
        y: bb.y - grow,
        width: bb.width + 2.0 * grow,
        height: bb.height + 2.0 * grow,
    };
    if !(outer.width > 0.0 && outer.height > 0.0) {
        return;
    }
    let radii = fit_radii(&bb, bx.extras.radii).map(|(rx, ry)| {
        if rx > 0.0 && ry > 0.0 {
            ((rx + grow).max(0.0), (ry + grow).max(0.0))
        } else {
            (0.0, 0.0)
        }
    });
    let width = LayoutUnit::from_css_px_f32(o.width).unwrap_or(LayoutUnit::ZERO);
    let side = BorderSide {
        width,
        style: o.style,
        color: o.color,
    };
    let sides = BorderSides {
        top: side,
        right: side,
        bottom: side,
        left: side,
    };
    let square = radii.iter().all(|r| r.0 <= 0.0 || r.1 <= 0.0);
    if square {
        for (_, path, color) in side_fills(outer, &sides) {
            let ink = crate::scene::appendix_e::path_bounds(&path).unwrap_or(outer);
            out.push(Pending {
                owner: bx.node,
                phase: PaintPhase::Outline,
                kind,
                ink,
                scope,
                op: DrawOp::FillPath {
                    path,
                    fill_rule: FillRule::NonZero,
                    paint: Paint::Solid(color),
                },
            });
        }
        return;
    }
    let (pieces, skipped) = rounded_border_pieces(outer, radii, &sides);
    if !skipped.is_empty() {
        b.report("css.property.outline-style", bx.node);
    }
    for p in pieces {
        let piece_scope = match p.clip {
            Some(wedge) => Some(b.scope(
                scope,
                ScopeKind::Clip(ClipShape::Path {
                    path: wedge,
                    fill_rule: FillRule::NonZero,
                }),
            )),
            None => scope,
        };
        let ink = crate::scene::appendix_e::path_bounds(&p.path).unwrap_or(outer);
        out.push(Pending {
            owner: bx.node,
            phase: PaintPhase::Outline,
            kind,
            ink,
            scope: piece_scope,
            op: DrawOp::FillPath {
                path: p.path,
                fill_rule: p.fill_rule,
                paint: Paint::Solid(p.color),
            },
        });
    }
}
