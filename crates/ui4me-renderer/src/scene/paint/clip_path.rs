//! owner: 010-paint-render — lane L-PAINT2-r12 (WPT-D16, FR-074)
//!
//! `clip-path` basic shapes (CSS Masking 1 section 5.1, CSS Shapes 1): `inset()` (with `round`,
//! and `rect()` / `xywh()` which compute to it), `circle()`, `ellipse()`, `polygon()` (fill rule)
//! and `path()`, on a reference box (`margin-box`, `border-box`, `padding-box`, `content-box`), and
//! a geometry box alone (the edge of that box). The shape becomes a [`ClipShape`] scope around the
//! box's own painting and its descendants; the raster backend clips with it (vello_cpu path
//! clipping). `url(#id)` and `shape()` are not here: the style projection reports them.
//!

use crate::scene::appendix_e::{Builder, PaintBox, border_widths};
use crate::scene::borders::{fit_radii, inset_rrect};
use crate::scene::display_list::{
    ClipShape, FillRule, Path, PathCommand as C, Point, Rect, RoundedRect, ScopeId, ScopeKind,
};
use crate::scene::inputs::{ClipPathSpec, ClipRefBox, LenPct, ShapeRadiusSpec, ShapeSpec};

/// The reference box rectangle, with the corner radii of the box where it has them (the border,
/// padding and content boxes follow the border radius; the margin box is square).
pub fn reference_box(bx: &PaintBox, which: ClipRefBox) -> RoundedRect {
    let e = &bx.extras;
    let bb = Rect {
        x: bx.border_box.origin.x.to_f64() as f32,
        y: bx.border_box.origin.y.to_f64() as f32,
        width: bx.border_box.size.width.to_f64() as f32,
        height: bx.border_box.size.height.to_f64() as f32,
    };
    let outer = RoundedRect {
        rect: bb,
        radii: fit_radii(&bb, e.radii),
    };
    let w = border_widths(&bx.border);
    let p = e.padding;
    match which {
        ClipRefBox::Border => outer,
        ClipRefBox::Padding => inset_rrect(&outer, w[0], w[1], w[2], w[3]),
        ClipRefBox::Content => {
            inset_rrect(&outer, w[0] + p[0], w[1] + p[1], w[2] + p[2], w[3] + p[3])
        }
        ClipRefBox::Margin => {
            let m = e.margin;
            RoundedRect::square(Rect {
                x: bb.x - m[3],
                y: bb.y - m[0],
                width: (bb.width + m[1] + m[3]).max(0.0),
                height: (bb.height + m[0] + m[2]).max(0.0),
            })
        }
    }
}

fn ellipse_path(cx: f32, cy: f32, rx: f32, ry: f32) -> Path {
    const K: f32 = 4.0 / 3.0 * (std::f32::consts::SQRT_2 - 1.0);
    let p = |x: f32, y: f32| Point { x, y };
    Path {
        commands: vec![
            C::MoveTo(p(cx + rx, cy)),
            C::CubicTo(
                p(cx + rx, cy + K * ry),
                p(cx + K * rx, cy + ry),
                p(cx, cy + ry),
            ),
            C::CubicTo(
                p(cx - K * rx, cy + ry),
                p(cx - rx, cy + K * ry),
                p(cx - rx, cy),
            ),
            C::CubicTo(
                p(cx - rx, cy - K * ry),
                p(cx - K * rx, cy - ry),
                p(cx, cy - ry),
            ),
            C::CubicTo(
                p(cx + K * rx, cy - ry),
                p(cx + rx, cy - K * ry),
                p(cx + rx, cy),
            ),
            C::Close,
        ],
    }
}

fn at(v: (LenPct, LenPct), r: &Rect) -> (f32, f32) {
    (r.x + v.0.resolve(r.width), r.y + v.1.resolve(r.height))
}

/// A `circle()` radius (CSS Shapes 1: a percentage is of `sqrt((w^2 + h^2) / 2)`; the keywords
/// measure from the centre to the sides or corners of the reference box).
fn circle_radius(radius: &ShapeRadiusSpec, c: (f32, f32), r: &Rect) -> f32 {
    let (dl, dr, dt, db) = (
        (c.0 - r.x).abs(),
        (r.right() - c.0).abs(),
        (c.1 - r.y).abs(),
        (r.bottom() - c.1).abs(),
    );
    let corners = [dl.hypot(dt), dr.hypot(dt), dl.hypot(db), dr.hypot(db)];
    match radius {
        ShapeRadiusSpec::Length(l) => {
            l.resolve(((r.width * r.width + r.height * r.height) / 2.0).sqrt())
        }
        ShapeRadiusSpec::ClosestSide => dl.min(dr).min(dt).min(db),
        ShapeRadiusSpec::FarthestSide => dl.max(dr).max(dt).max(db),
        ShapeRadiusSpec::ClosestCorner => corners.into_iter().fold(f32::INFINITY, f32::min),
        ShapeRadiusSpec::FarthestCorner => corners.into_iter().fold(0.0, f32::max),
    }
}

/// The radii of an `ellipse()`: a keyword radius is measured per axis (sides), and a corner keyword
/// is the distance from the centre to the closest (farthest) corner on either axis (what the
/// reference browser does; the WPT reference draws `farthest-corner closest-corner` with radii 247.487 x 125).
fn ellipse_radii(
    rx: &ShapeRadiusSpec,
    ry: &ShapeRadiusSpec,
    c: (f32, f32),
    r: &Rect,
) -> (f32, f32) {
    let (dl, dr, dt, db) = (
        (c.0 - r.x).abs(),
        (r.right() - c.0).abs(),
        (c.1 - r.y).abs(),
        (r.bottom() - c.1).abs(),
    );
    let axis = |s: &ShapeRadiusSpec, basis: f32, near: f32, far: f32| match s {
        ShapeRadiusSpec::Length(l) => l.resolve(basis),
        ShapeRadiusSpec::ClosestSide => near,
        ShapeRadiusSpec::FarthestSide => far,
        ShapeRadiusSpec::ClosestCorner => dl.min(dr).hypot(dt.min(db)),
        ShapeRadiusSpec::FarthestCorner => dl.max(dr).hypot(dt.max(db)),
    };
    (
        axis(rx, r.width, dl.min(dr), dl.max(dr)),
        axis(ry, r.height, dt.min(db), dt.max(db)),
    )
}

/// The clip of a shape on its reference box, `None` when nothing can be clipped to (a degenerate
/// shape clips everything away: [`ClipShape::Rect`] of zero size).
pub fn shape_clip(spec: &ClipPathSpec, bx: &PaintBox) -> ClipShape {
    let rr = reference_box(bx, spec.reference);
    let r = rr.rect;
    let nothing = ClipShape::Rect(Rect {
        x: r.x,
        y: r.y,
        width: 0.0,
        height: 0.0,
    });
    match &spec.shape {
        ShapeSpec::Box => {
            if rr.is_square() {
                ClipShape::Rect(rr.rect)
            } else {
                ClipShape::RoundedRect(rr)
            }
        }
        ShapeSpec::Inset { insets, radii } => {
            let rect = Rect {
                x: r.x + insets[3].resolve(r.width),
                y: r.y + insets[0].resolve(r.height),
                width: r.width - insets[3].resolve(r.width) - insets[1].resolve(r.width),
                height: r.height - insets[0].resolve(r.height) - insets[2].resolve(r.height),
            };
            if !(rect.width > 0.0 && rect.height > 0.0) {
                return nothing;
            }
            let raw = radii.map(|(x, y)| (x.resolve(r.width), y.resolve(r.height)));
            let rrect = RoundedRect {
                rect,
                radii: fit_radii(&rect, raw),
            };
            if rrect.is_square() {
                ClipShape::Rect(rect)
            } else {
                ClipShape::RoundedRect(rrect)
            }
        }
        ShapeSpec::Circle { radius, center } => {
            let c = at(*center, &r);
            let rad = circle_radius(radius, c, &r);
            if !(rad > 0.0 && rad.is_finite()) {
                return nothing;
            }
            ClipShape::Path {
                path: ellipse_path(c.0, c.1, rad, rad),
                fill_rule: FillRule::NonZero,
            }
        }
        ShapeSpec::Ellipse { rx, ry, center } => {
            let c = at(*center, &r);
            let (rx, ry) = ellipse_radii(rx, ry, c, &r);
            if !(rx > 0.0 && ry > 0.0 && rx.is_finite() && ry.is_finite()) {
                return nothing;
            }
            ClipShape::Path {
                path: ellipse_path(c.0, c.1, rx, ry),
                fill_rule: FillRule::NonZero,
            }
        }
        ShapeSpec::Polygon { evenodd, points } => {
            let mut commands = Vec::with_capacity(points.len() + 1);
            for (i, p) in points.iter().enumerate() {
                let (x, y) = at(*p, &r);
                let pt = Point { x, y };
                commands.push(if i == 0 { C::MoveTo(pt) } else { C::LineTo(pt) });
            }
            if commands.is_empty() {
                return nothing;
            }
            commands.push(C::Close);
            ClipShape::Path {
                path: Path { commands },
                fill_rule: if *evenodd {
                    FillRule::EvenOdd
                } else {
                    FillRule::NonZero
                },
            }
        }
        ShapeSpec::Path { evenodd, path } => {
            // `path()` coordinates are px from the reference box's top-left corner.
            let m = |p: &Point| Point {
                x: p.x + r.x,
                y: p.y + r.y,
            };
            let commands: Vec<C> = path
                .commands
                .iter()
                .map(|c| match c {
                    C::MoveTo(p) => C::MoveTo(m(p)),
                    C::LineTo(p) => C::LineTo(m(p)),
                    C::QuadTo(a, b) => C::QuadTo(m(a), m(b)),
                    C::CubicTo(a, b, c) => C::CubicTo(m(a), m(b), m(c)),
                    C::Close => C::Close,
                })
                .collect();
            if commands.is_empty() {
                return nothing;
            }
            ClipShape::Path {
                path: Path { commands },
                fill_rule: if *evenodd {
                    FillRule::EvenOdd
                } else {
                    FillRule::NonZero
                },
            }
        }
    }
}

/// Open the `clip-path` scope of `bx` under `parent`.
pub(crate) fn clip_scope(
    b: &mut Builder,
    bx: &PaintBox,
    spec: &ClipPathSpec,
    parent: Option<ScopeId>,
) -> ScopeId {
    let shape = shape_clip(spec, bx);
    b.scope(parent, ScopeKind::Clip(shape))
}
