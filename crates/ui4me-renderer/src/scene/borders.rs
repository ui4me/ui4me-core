//! owner: 010-paint-render — created by 001 slice (R-28)
//!
//! Border side and corner geometry (010 FR-030). Slice subset: solid sides with square
//! corners, each side one `FillPath` trapezoid in its own colour, mitred at the corners.
//! Implemented by 001 T047.

use crate::scene::display_list::{Color, FillRule, Path, PathCommand, Point, Rect, RoundedRect};
use ui4me_engine_types::geometry::layout_unit::LayoutUnit;

/// The border style the scene paints (slice subset).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    None,
    Solid,
    Dotted,
    Dashed,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

/// Computed `border-*-style` as 006 publishes it (`ComputedStyleView::box_model`), the input of
/// the paint-stage consumption check in `appendix_e::prepare_scene`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedBorderStyle {
    None,
    Hidden,
    Solid,
    Dotted,
    Dashed,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl ComputedBorderStyle {
    /// The painted style. Every CSS border style is consumed (010 T070); the `Option` stays so
    /// callers keep their unconsumed-value path for styles a geometry cannot draw.
    pub fn consumed(self) -> Option<BorderStyle> {
        match self {
            ComputedBorderStyle::None | ComputedBorderStyle::Hidden => Some(BorderStyle::None),
            ComputedBorderStyle::Solid => Some(BorderStyle::Solid),
            ComputedBorderStyle::Dotted => Some(BorderStyle::Dotted),
            ComputedBorderStyle::Dashed => Some(BorderStyle::Dashed),
            ComputedBorderStyle::Double => Some(BorderStyle::Double),
            ComputedBorderStyle::Groove => Some(BorderStyle::Groove),
            ComputedBorderStyle::Ridge => Some(BorderStyle::Ridge),
            ComputedBorderStyle::Inset => Some(BorderStyle::Inset),
            ComputedBorderStyle::Outset => Some(BorderStyle::Outset),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSide {
    /// Used (snapped) width.
    pub width: LayoutUnit,
    pub style: BorderStyle,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSides {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// Dash geometry ratios (length, gap) in multiples of the border width, as the reference browser
/// uses them: thin borders (< 3px) get longer dashes and gaps. Gaps are then stretched so a side
/// starts and ends with a full dash (dashes) or a full dot (dots).
fn dash_ratios(width: f32) -> (f32, f32) {
    if width >= 3.0 { (2.0, 1.0) } else { (3.0, 2.0) }
}

/// Segment starts and lengths along a span of `len`: `n` pieces of `piece` with equal gaps,
/// first at 0 and last ending at `len`. A span shorter than one piece gives one shrunken piece.
fn distribute(len: f32, piece: f32, gap: f32) -> Vec<(f32, f32)> {
    if len <= 0.0 || piece <= 0.0 {
        return Vec::new();
    }
    if len <= piece {
        return vec![(0.0, len)];
    }
    let n = (((len + gap) / (piece + gap)).floor() as usize).max(1);
    if n == 1 {
        return vec![((len - piece) / 2.0, piece)];
    }
    let g = (len - n as f32 * piece) / (n as f32 - 1.0);
    (0..n).map(|i| (i as f32 * (piece + g), piece)).collect()
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Path {
    let p = |x: f32, y: f32| Point { x, y };
    Path {
        commands: vec![
            PathCommand::MoveTo(p(x, y)),
            PathCommand::LineTo(p(x + w, y)),
            PathCommand::LineTo(p(x + w, y + h)),
            PathCommand::LineTo(p(x, y + h)),
            PathCommand::Close,
        ],
    }
}

fn circle_path(cx: f32, cy: f32, r: f32) -> Path {
    const K: f32 = 0.552_284_8;
    let p = |x: f32, y: f32| Point { x, y };
    let k = r * K;
    Path {
        commands: vec![
            PathCommand::MoveTo(p(cx + r, cy)),
            PathCommand::CubicTo(p(cx + r, cy + k), p(cx + k, cy + r), p(cx, cy + r)),
            PathCommand::CubicTo(p(cx - k, cy + r), p(cx - r, cy + k), p(cx - r, cy)),
            PathCommand::CubicTo(p(cx - r, cy - k), p(cx - k, cy - r), p(cx, cy - r)),
            PathCommand::CubicTo(p(cx + k, cy - r), p(cx + r, cy - k), p(cx + r, cy)),
            PathCommand::Close,
        ],
    }
}

/// Pieces of a dashed or dotted side. The straight band of a side spans between the corner
/// squares: horizontal sides span the full width, vertical sides run between the horizontal
/// borders, so corner squares are painted once (by the top/bottom sides).
fn dashed_pieces(side: Side, bx: &Rect, sides: &BorderSides, dotted: bool) -> Vec<Path> {
    let w = |s: &BorderSide| s.width.to_f64() as f32;
    let (x0, y0, x1, y1) = (bx.x, bx.y, bx.right(), bx.bottom());
    let (thick, horizontal, start, len, cross) = match side {
        Side::Top => (w(&sides.top), true, x0, x1 - x0, y0),
        Side::Bottom => (w(&sides.bottom), true, x0, x1 - x0, y1 - w(&sides.bottom)),
        Side::Left => (
            w(&sides.left),
            false,
            y0 + w(&sides.top),
            (y1 - y0) - w(&sides.top) - w(&sides.bottom),
            x0,
        ),
        Side::Right => (
            w(&sides.right),
            false,
            y0 + w(&sides.top),
            (y1 - y0) - w(&sides.top) - w(&sides.bottom),
            x1 - w(&sides.right),
        ),
    };
    let (piece, gap) = if dotted {
        (thick, thick)
    } else {
        let (dl, dg) = dash_ratios(thick);
        (dl * thick, dg * thick)
    };
    distribute(len, piece, gap)
        .into_iter()
        .map(|(off, l)| match (dotted, horizontal) {
            (true, true) => circle_path(
                start + off + l / 2.0,
                cross + thick / 2.0,
                l.min(thick) / 2.0,
            ),
            (true, false) => circle_path(
                cross + thick / 2.0,
                start + off + l / 2.0,
                l.min(thick) / 2.0,
            ),
            (false, true) => rect_path(start + off, cross, l, thick),
            (false, false) => rect_path(cross, start + off, thick, l),
        })
        .collect()
}

/// The side quad between the outer box inset by `outer_in` (per side) and inset by `inner_in`.
/// Corners are mitred on the diagonal of the two bounding boxes (as the solid style).
fn band(side: Side, bx: &Rect, outer_in: [f32; 4], inner_in: [f32; 4], square: [bool; 4]) -> Path {
    // Order of insets: top, right, bottom, left. `square[c]` (top-left, top-right, bottom-right,
    // bottom-left) joins the two sides of that corner as one rectangle owned by the top / bottom side.
    let (x0, y0, x1, y1) = (
        bx.x + outer_in[3],
        bx.y + outer_in[0],
        bx.right() - outer_in[1],
        bx.bottom() - outer_in[2],
    );
    let ix0 = (bx.x + inner_in[3]).min(x1).max(x0);
    let iy0 = (bx.y + inner_in[0]).min(y1).max(y0);
    let ix1 = (bx.right() - inner_in[1]).max(ix0).min(x1);
    let iy1 = (bx.bottom() - inner_in[2]).max(iy0).min(y1);
    let p = |x: f32, y: f32| Point { x, y };
    let quad = |a: Point, b: Point, c: Point, d: Point| Path {
        commands: vec![
            PathCommand::MoveTo(a),
            PathCommand::LineTo(b),
            PathCommand::LineTo(c),
            PathCommand::LineTo(d),
            PathCommand::Close,
        ],
    };
    let [tl, tr, br, bl] = square;
    match side {
        Side::Top => quad(
            p(x0, y0),
            p(x1, y0),
            if tr { p(x1, iy0) } else { p(ix1, iy0) },
            if tl { p(x0, iy0) } else { p(ix0, iy0) },
        ),
        Side::Right => quad(
            p(x1, if tr { iy0 } else { y0 }),
            p(x1, if br { iy1 } else { y1 }),
            p(ix1, iy1),
            p(ix1, iy0),
        ),
        Side::Bottom => quad(
            p(x1, y1),
            p(x0, y1),
            if bl { p(x0, iy1) } else { p(ix0, iy1) },
            if br { p(x1, iy1) } else { p(ix1, iy1) },
        ),
        Side::Left => quad(
            p(x0, if bl { iy1 } else { y1 }),
            p(x0, if tl { iy0 } else { y0 }),
            p(ix0, iy0),
            p(ix0, iy1),
        ),
    }
}

/// The fill paths of each painted side (non-zero width, style not `none`), in the order
/// top, right, bottom, left. Solid corners are split on the diagonal from the outer to the inner
/// corner. `double` paints the outer and inner thirds (solid when thinner than 3px); `dashed`
/// and `dotted` paint rectangles and circles along each side (see `dashed_pieces`).
pub fn side_fills(border_box: Rect, sides: &BorderSides) -> Vec<(Side, Path, Color)> {
    let w = |s: &BorderSide| s.width.to_f64() as f32;
    let widths = [
        w(&sides.top),
        w(&sides.right),
        w(&sides.bottom),
        w(&sides.left),
    ];
    let mut out = Vec::with_capacity(4);
    for (side, s) in [
        (Side::Top, &sides.top),
        (Side::Right, &sides.right),
        (Side::Bottom, &sides.bottom),
        (Side::Left, &sides.left),
    ] {
        if s.style == BorderStyle::None || s.width <= LayoutUnit::ZERO {
            continue;
        }
        if matches!(s.style, BorderStyle::Dashed | BorderStyle::Dotted) {
            for path in dashed_pieces(side, &border_box, sides, s.style == BorderStyle::Dotted) {
                out.push((side, path, s.color));
            }
            continue;
        }
        // Layers of the side as bands between insets of `f × widths` (adjacent sides contribute
        // their own thirds / halves, so corners mitre correctly).
        for (f0, f1, color) in side_layers(side, s.style, s.color, widths[side as usize]) {
            let at = |f: f32| [widths[0] * f, widths[1] * f, widths[2] * f, widths[3] * f];
            // Adjacent bands with the same extent and actual colour meet without a
            // diagonal seam. This includes equal shaded halves of groove/ridge borders.
            let matches_layer = |which: Side, s: &BorderSide| {
                s.width > LayoutUnit::ZERO
                    && side_layers(which, s.style, s.color, widths[which as usize])
                        .into_iter()
                        .any(|(a, b, c)| a == f0 && b == f1 && c == color)
            };
            let joined = |a: Side, sa: &BorderSide, b: Side, sb: &BorderSide| {
                matches_layer(a, sa) && matches_layer(b, sb)
            };
            let square = [
                joined(Side::Top, &sides.top, Side::Left, &sides.left),
                joined(Side::Top, &sides.top, Side::Right, &sides.right),
                joined(Side::Bottom, &sides.bottom, Side::Right, &sides.right),
                joined(Side::Bottom, &sides.bottom, Side::Left, &sides.left),
            ];
            out.push((side, band(side, &border_box, at(f0), at(f1), square), color));
        }
    }
    out
}

/// The painted layers of one non-dashed, non-dotted side as `(outer fraction, inner fraction,
/// colour)` of the side's width, outermost first: `solid` is one layer; `double` the outer and
/// inner thirds (one layer below 3px); `inset`/`outset` one layer in the shaded colour;
/// `groove`/`ridge` two halves in the two shades (CSS Backgrounds 3 section 4.2).
pub fn side_layers(
    side: Side,
    style: BorderStyle,
    color: Color,
    width: f32,
) -> Vec<(f32, f32, Color)> {
    match style {
        BorderStyle::None | BorderStyle::Dashed | BorderStyle::Dotted => Vec::new(),
        BorderStyle::Solid => vec![(0.0, 1.0, color)],
        BorderStyle::Double if width < 3.0 => vec![(0.0, 1.0, color)],
        BorderStyle::Double => vec![(0.0, 1.0 / 3.0, color), (2.0 / 3.0, 1.0, color)],
        BorderStyle::Inset | BorderStyle::Outset => {
            vec![(0.0, 1.0, shaded(color, side, style == BorderStyle::Inset))]
        }
        BorderStyle::Groove | BorderStyle::Ridge => {
            // groove = outer half inset-shaded, inner half outset-shaded; ridge the reverse.
            let groove = style == BorderStyle::Groove;
            vec![
                (0.0, 0.5, shaded(color, side, groove)),
                (0.5, 1.0, shaded(color, side, !groove)),
            ]
        }
    }
}

/// The colour of a side of an `inset` (or, with `inset = false`, `outset`) border: the top and
/// left sides of an inset border are darkened and the bottom and right ones lightened (the
/// reverse for outset). Darkening and lightening follow the reference browser's colour
/// functions, including the rule that colours already close to black (or white) are not
/// pushed further.
pub fn shaded(color: Color, side: Side, inset: bool) -> Color {
    let darken = matches!(side, Side::Top | Side::Left) == inset;
    let d2 = |c: &Color, t: f32| -> f32 {
        [c.r, c.g, c.b]
            .iter()
            .map(|v| ((v * 255.0).round() - t).powi(2))
            .sum()
    };
    if darken {
        // Distance from black against the base dark colour 0x202020.
        if d2(&color, 0.0) > 3.0 * 32.0 * 32.0 {
            dark(color)
        } else {
            color
        }
    } else if d2(&color, 255.0) > 3.0 * 20.0 * 20.0 {
        light(color)
    } else {
        color
    }
}

fn dark(c: Color) -> Color {
    let v = c.r.max(c.g).max(c.b);
    if v <= 0.0 {
        return c;
    }
    let m = ((v - 0.33) / v).max(0.0);
    Color {
        r: c.r * m,
        g: c.g * m,
        b: c.b * m,
        a: c.a,
    }
}

fn light(c: Color) -> Color {
    let v = c.r.max(c.g).max(c.b);
    if v <= 0.0 {
        // Lightened black.
        let g = 0x54 as f32 / 255.0;
        return Color {
            r: g,
            g,
            b: g,
            a: c.a,
        };
    }
    let m = (v + 0.33).min(1.0) / v;
    Color {
        r: (c.r * m).min(1.0),
        g: (c.g * m).min(1.0),
        b: (c.b * m).min(1.0),
        a: c.a,
    }
}

// ------------------------------------------------------------------ rounded geometry

const KAPPA: f32 = 0.552_284_8;

/// Per-corner elliptical radii `(rx, ry)` (top-left, top-right, bottom-right, bottom-left) scaled
/// to fit `rect`: when two adjacent radii sum to more than the side they share, all radii are
/// multiplied by the smallest such ratio (CSS Backgrounds 3 section 5.5). Negative or
/// non-finite radii become 0, and a corner with a zero radius on either axis is square.
pub fn fit_radii(rect: &Rect, raw: [(f32, f32); 4]) -> [(f32, f32); 4] {
    let clean = |v: f32| if v.is_finite() && v > 0.0 { v } else { 0.0 };
    let r = raw.map(|(x, y)| (clean(x), clean(y)));
    let mut f = 1.0f32;
    let mut limit = |sum: f32, side: f32| {
        if sum > side && sum > 0.0 {
            f = f.min(side / sum);
        }
    };
    limit(r[0].0 + r[1].0, rect.width);
    limit(r[3].0 + r[2].0, rect.width);
    limit(r[0].1 + r[3].1, rect.height);
    limit(r[1].1 + r[2].1, rect.height);
    r.map(|(x, y)| {
        if x * f <= 0.0 || y * f <= 0.0 {
            (0.0, 0.0)
        } else {
            (x * f, y * f)
        }
    })
}

/// The rounded rect inset on each side; each corner radius shrinks by the adjacent widths and a
/// corner with a zero radius on either axis becomes square.
pub fn inset_rrect(rr: &RoundedRect, top: f32, right: f32, bottom: f32, left: f32) -> RoundedRect {
    let rect = Rect {
        x: rr.rect.x + left,
        y: rr.rect.y + top,
        width: (rr.rect.width - left - right).max(0.0),
        height: (rr.rect.height - top - bottom).max(0.0),
    };
    let corner = |(rx, ry): (f32, f32), dx: f32, dy: f32| {
        let (x, y) = ((rx - dx).max(0.0), (ry - dy).max(0.0));
        if x <= 0.0 || y <= 0.0 {
            (0.0, 0.0)
        } else {
            (x, y)
        }
    };
    RoundedRect {
        rect,
        radii: [
            corner(rr.radii[0], left, top),
            corner(rr.radii[1], right, top),
            corner(rr.radii[2], right, bottom),
            corner(rr.radii[3], left, bottom),
        ],
    }
}

/// Closed path of a rounded rect, clockwise from the top edge.
pub fn rrect_path(rr: &RoundedRect) -> Path {
    let Rect {
        x,
        y,
        width: w,
        height: h,
    } = rr.rect;
    let p = |x: f32, y: f32| Point { x, y };
    let [(tlx, tly), (trx, try_), (brx, bry), (blx, bly)] = rr.radii;
    let mut c = vec![
        PathCommand::MoveTo(p(x + tlx, y)),
        PathCommand::LineTo(p(x + w - trx, y)),
    ];
    if trx > 0.0 {
        c.push(PathCommand::CubicTo(
            p(x + w - trx + trx * KAPPA, y),
            p(x + w, y + try_ - try_ * KAPPA),
            p(x + w, y + try_),
        ));
    }
    c.push(PathCommand::LineTo(p(x + w, y + h - bry)));
    if brx > 0.0 {
        c.push(PathCommand::CubicTo(
            p(x + w, y + h - bry + bry * KAPPA),
            p(x + w - brx + brx * KAPPA, y + h),
            p(x + w - brx, y + h),
        ));
    }
    c.push(PathCommand::LineTo(p(x + blx, y + h)));
    if blx > 0.0 {
        c.push(PathCommand::CubicTo(
            p(x + blx - blx * KAPPA, y + h),
            p(x, y + h - bly + bly * KAPPA),
            p(x, y + h - bly),
        ));
    }
    c.push(PathCommand::LineTo(p(x, y + tly)));
    if tlx > 0.0 {
        c.push(PathCommand::CubicTo(
            p(x, y + tly - tly * KAPPA),
            p(x + tlx - tlx * KAPPA, y),
            p(x + tlx, y),
        ));
    }
    c.push(PathCommand::Close);
    Path { commands: c }
}

/// One filled piece of a rounded border: `path` filled with `fill_rule` in `color`, restricted
/// to `clip` (a side's mitre wedge) when present.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderPiece {
    pub clip: Option<Path>,
    pub path: Path,
    pub fill_rule: FillRule,
    pub color: Color,
}

/// The pieces of a border whose box has non-zero corner radii. Each layer of a side is the ring
/// between two inset rounded rects (even-odd), clipped to the side's mitre wedge; when every
/// side has the same layers (the common uniform case) the rings are unclipped. Dashed and
/// dotted sides are not drawn on rounded boxes: they are returned in the second element so the
/// caller can report them.
pub fn rounded_border_pieces(
    border_box: Rect,
    radii: [(f32, f32); 4],
    sides: &BorderSides,
) -> (Vec<BorderPiece>, Vec<Side>) {
    let w = |s: &BorderSide| s.width.to_f64() as f32;
    let widths = [
        w(&sides.top),
        w(&sides.right),
        w(&sides.bottom),
        w(&sides.left),
    ];
    let outer = RoundedRect {
        rect: border_box,
        radii,
    };
    let at = |f: f32| {
        inset_rrect(
            &outer,
            widths[0] * f,
            widths[1] * f,
            widths[2] * f,
            widths[3] * f,
        )
    };
    let mut skipped = Vec::new();
    type Layers = Vec<(f32, f32, Color)>;
    let mut per_side: Vec<(Side, Layers)> = Vec::new();
    for (side, s) in [
        (Side::Top, &sides.top),
        (Side::Right, &sides.right),
        (Side::Bottom, &sides.bottom),
        (Side::Left, &sides.left),
    ] {
        if s.style == BorderStyle::None || s.width <= LayoutUnit::ZERO {
            continue;
        }
        if matches!(s.style, BorderStyle::Dashed | BorderStyle::Dotted) {
            skipped.push(side);
            continue;
        }
        per_side.push((
            side,
            side_layers(side, s.style, s.color, widths[side as usize]),
        ));
    }
    let ring = |f0: f32, f1: f32| {
        let mut p = rrect_path(&at(f0));
        p.commands.extend(rrect_path(&at(f1)).commands);
        p
    };
    let mut out = Vec::new();
    let uniform = per_side.len() == 4
        && per_side.iter().all(|(_, l)| *l == per_side[0].1)
        && widths.iter().all(|v| *v == widths[0]);
    if uniform {
        for (f0, f1, color) in &per_side[0].1 {
            out.push(BorderPiece {
                clip: None,
                path: ring(*f0, *f1),
                fill_rule: FillRule::EvenOdd,
                color: *color,
            });
        }
        return (out, skipped);
    }
    for (side, layers) in per_side {
        let wedge = band(side, &border_box, [0.0; 4], widths, [false; 4]);
        for (f0, f1, color) in layers {
            out.push(BorderPiece {
                clip: Some(wedge.clone()),
                path: ring(f0, f1),
                fill_rule: FillRule::EvenOdd,
                color,
            });
        }
    }
    (out, skipped)
}
