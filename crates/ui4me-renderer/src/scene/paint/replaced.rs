//! owner: 010-paint-render — lane L-STY-r2 (replaced-element image painting; FR-060–FR-063 subset)
//!
//! The picture of a replaced element (`<img>`): `DrawImage` over the content box, placed by
//! `object-fit` / `object-position` (CSS Images 3 section 5.5) unless layout published the
//! resolved rectangle (`BoxFragment.extras.replaced_content_rect`, 008), and clipped to the
//! content box (with the inner border radii) whenever it would overflow it.
//!
//! **Interface with 008.** `BoxExtras.replaced_content_rect` (document space) is the resolved
//! content rectangle of the replaced content. While layout does not publish it the placement is
//! computed here from the same inputs (content box, natural size, `object-fit`,
//! `object-position`); the two must agree, and the scene prefers layout's value when present.

use crate::scene::appendix_e::{Builder, PaintBox, PaintPhase, Pending};
use crate::scene::borders::inset_rrect;
use crate::scene::display_list::{
    ClipShape, DrawOp, ItemKind, Rect, RoundedRect, ScopeId, ScopeKind,
};
use crate::scene::inputs::{ObjectFit, Replaced, ReplacedImage};

/// The destination rectangle of the picture inside the content box `content`.
pub fn object_fit_rect(content: &Rect, img: &ReplacedImage) -> Option<Rect> {
    let (cw, ch) = (content.width, content.height);
    if !(cw > 0.0 && ch > 0.0) {
        return None;
    }
    let d = &img.image;
    let ratio = d.ratio.filter(|r| r.is_finite() && *r > 0.0).or_else(|| {
        d.natural
            .0
            .zip(d.natural.1)
            .filter(|(w, h)| *w > 0.0 && *h > 0.0)
            .map(|(w, h)| w / h)
    });
    // The natural size; a missing dimension comes from the ratio, an image with no natural size
    // at all is as large as the content box (CSS Images 3 section 5.3, default object size).
    let contain = |r: f32| {
        if cw / ch > r {
            (ch * r, ch)
        } else {
            (cw, cw / r)
        }
    };
    let natural = match (d.natural.0, d.natural.1, ratio) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(r)) => (w, w / r),
        (None, Some(h), Some(r)) => (h * r, h),
        (None, None, Some(r)) => contain(r),
        (Some(w), None, None) => (w, ch),
        (None, Some(h), None) => (cw, h),
        (None, None, None) => (cw, ch),
    };
    let (w, h) = match img.fit {
        ObjectFit::Fill => (cw, ch),
        ObjectFit::None => natural,
        ObjectFit::Contain | ObjectFit::Cover | ObjectFit::ScaleDown => {
            let r = ratio.unwrap_or(natural.0 / natural.1);
            let fit = |cover: bool| {
                let wide = cw / ch > r;
                if wide != cover {
                    (ch * r, ch)
                } else {
                    (cw, cw / r)
                }
            };
            match img.fit {
                ObjectFit::Contain => fit(false),
                ObjectFit::Cover => fit(true),
                _ => {
                    // scale-down: the smaller of `none` and `contain`.
                    let c = fit(false);
                    if natural.0 <= c.0 && natural.1 <= c.1 {
                        natural
                    } else {
                        c
                    }
                }
            }
        }
    };
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return None;
    }
    Some(Rect {
        x: content.x + img.position.0.resolve(cw - w),
        y: content.y + img.position.1.resolve(ch - h),
        width: w,
        height: h,
    })
}

fn contains(outer: &Rect, inner: &Rect) -> bool {
    inner.x >= outer.x - 1e-3
        && inner.y >= outer.y - 1e-3
        && inner.right() <= outer.right() + 1e-3
        && inner.bottom() <= outer.bottom() + 1e-3
}

/// The bullet of a `disc`, `circle` or `square` marker, filled or ringed in the marker colour (the
/// reference browser draws these shapes; a ring is the `circle` bullet with a 1 px stroke).
pub(crate) fn push_marker_symbol(
    node: ui4me_engine_types::ids::NodeId,
    m: &crate::scene::inputs::MarkerSymbolPaint,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
) {
    use crate::layout::replaced::MarkerShape;
    use crate::scene::display_list::{FillRule, Paint, Path, PathCommand as C, Point};
    let r = m.rect;
    if !(r.width > 0.0 && r.height > 0.0) {
        return;
    }
    let paint = Paint::Solid(m.color);
    let op = match m.shape {
        MarkerShape::DisclosureOpen
        | MarkerShape::DisclosureClosedLtr
        | MarkerShape::DisclosureClosedRtl => {
            let p = |x, y| Point { x, y };
            let (a, b, c) = match m.shape {
                MarkerShape::DisclosureOpen => (
                    p(r.x, r.y),
                    p(r.x + r.width, r.y),
                    p(r.x + r.width / 2.0, r.y + r.height),
                ),
                MarkerShape::DisclosureClosedLtr => (
                    p(r.x, r.y),
                    p(r.x, r.y + r.height),
                    p(r.x + r.width, r.y + r.height / 2.0),
                ),
                _ => (
                    p(r.x + r.width, r.y),
                    p(r.x + r.width, r.y + r.height),
                    p(r.x, r.y + r.height / 2.0),
                ),
            };
            DrawOp::FillPath {
                path: Path {
                    commands: vec![C::MoveTo(a), C::LineTo(b), C::LineTo(c), C::Close],
                },
                fill_rule: FillRule::NonZero,
                paint,
            }
        }
        MarkerShape::Square => DrawOp::FillRect { rect: r, paint },
        MarkerShape::Disc => DrawOp::FillRoundedRect {
            rect: RoundedRect {
                rect: r,
                radii: [(r.width / 2.0, r.height / 2.0); 4],
            },
            paint,
        },
        MarkerShape::Circle => {
            // Two ellipses, even-odd: the outer one is the bullet's box, the inner one 1 px in.
            let ellipse = |x: f32, y: f32, w: f32, h: f32, cmds: &mut Vec<C>| {
                const K: f32 = 0.552_284_8;
                let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
                let p = |x: f32, y: f32| Point { x, y };
                cmds.push(C::MoveTo(p(cx + rx, cy)));
                cmds.push(C::CubicTo(
                    p(cx + rx, cy + K * ry),
                    p(cx + K * rx, cy + ry),
                    p(cx, cy + ry),
                ));
                cmds.push(C::CubicTo(
                    p(cx - K * rx, cy + ry),
                    p(cx - rx, cy + K * ry),
                    p(cx - rx, cy),
                ));
                cmds.push(C::CubicTo(
                    p(cx - rx, cy - K * ry),
                    p(cx - K * rx, cy - ry),
                    p(cx, cy - ry),
                ));
                cmds.push(C::CubicTo(
                    p(cx + K * rx, cy - ry),
                    p(cx + rx, cy - K * ry),
                    p(cx + rx, cy),
                ));
                cmds.push(C::Close);
            };
            let mut commands = Vec::new();
            ellipse(r.x, r.y, r.width, r.height, &mut commands);
            if r.width > 2.0 && r.height > 2.0 {
                ellipse(
                    r.x + 1.0,
                    r.y + 1.0,
                    r.width - 2.0,
                    r.height - 2.0,
                    &mut commands,
                );
            }
            DrawOp::FillPath {
                path: Path { commands },
                fill_rule: FillRule::EvenOdd,
                paint,
            }
        }
    };
    out.push(Pending {
        owner: node,
        phase: PaintPhase::InlineContent,
        kind: ItemKind::MARKER,
        ink: r,
        scope,
        op,
    });
}

/// Append the item of `bx`'s picture (nothing while it is pending or broken, or when the box has
/// no content area).
pub(crate) fn push_replaced(
    b: &mut Builder,
    bx: &PaintBox,
    rr: &RoundedRect,
    widths: [f32; 4],
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
) {
    let e = &bx.extras;
    let Some(Replaced::Image(img)) = &e.replaced else {
        return;
    };
    let p = e.padding;
    let content = inset_rrect(
        rr,
        widths[0] + p[0],
        widths[1] + p[1],
        widths[2] + p[2],
        widths[3] + p[3],
    );
    let dest = match e.replaced_content_rect {
        Some(r) => r,
        None => match object_fit_rect(&content.rect, img) {
            Some(r) => r,
            None => return,
        },
    };
    if !(dest.width > 0.0 && dest.height > 0.0) {
        return;
    }
    let scope = if !content.is_square() {
        Some(b.scope(scope, ScopeKind::Clip(ClipShape::RoundedRect(content))))
    } else if !contains(&content.rect, &dest) {
        Some(b.scope(scope, ScopeKind::Clip(ClipShape::Rect(content.rect))))
    } else {
        scope
    };
    let visible = match (content.is_square(), contains(&content.rect, &dest)) {
        (true, false) => {
            let (x0, y0) = (dest.x.max(content.rect.x), dest.y.max(content.rect.y));
            let (x1, y1) = (
                dest.right().min(content.rect.right()),
                dest.bottom().min(content.rect.bottom()),
            );
            if !(x1 > x0 && y1 > y0) {
                return;
            }
            Rect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            }
        }
        _ => dest,
    };
    // SVGs must be rasterised at the concrete object size and device scale,
    // just as CSS background/border images are. The natural-size bitmap is
    // only a bounded fallback; stretching it makes small external icons blur.
    let image = img.image.for_size((dest.width, dest.height));
    b.register_image(&image);
    out.push(Pending {
        owner: bx.node,
        phase: PaintPhase::InlineContent,
        kind: ItemKind::IMAGE,
        ink: visible,
        scope,
        op: DrawOp::DrawImage {
            image: image.id,
            dest,
            visible,
            sampling: img.sampling,
        },
    });
}
