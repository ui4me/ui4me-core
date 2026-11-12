//! owner: 010-paint-render — lane L-TXT-r5 (T089 decoration part; FR-050, FR-051)
//!
//! Text decorations (CSS Text Decoration 3) as display items. 008 publishes one
//! `DecorationFragment` per (decorating box, kind, line, contiguous extent) from 009's segments;
//! this module turns each into draw operations: underline and overline before the line's glyphs,
//! line-through after them (CSS Text Decoration 3 section 3.6), each in the decorating box's own
//! style and colour.
//!
//! Geometry of the styles (CSS leaves the patterns to the UA): `solid` is one rectangle of the
//! thickness `t`; `double` two rectangles a thickness apart (`2t` between their tops);
//! `dotted` square or round dots of diameter `t` at a pitch of `2t`; `dashed` dashes of `3t` with
//! gaps of `3t`; `wavy` a ribbon of thickness `t` whose centre line is made of quadratic
//! half-waves of amplitude `t` and a wavelength of about `4t`, stretched to fit the extent in a
//! whole number of half-waves. No trigonometry: the paths are deterministic everywhere. Skip-ink
//! is not implemented (the property is unavailable in the profile).

use crate::layout::fragment::{DecorationKind, DecorationLineStyle};
use crate::scene::appendix_e::{Builder, PaintDecoration, PaintPhase, Pending, TextShadowLayer};
use crate::scene::display_list::{
    DrawOp, FillRule, FilterEffect, FilterFn, ItemKind, Paint, Path, PathCommand, Point, Rect,
    ScopeId, ScopeKind,
};

/// Dots and dashes per decoration are bounded (a pathological width must not make a huge list).
const MAX_PIECES: usize = 2048;

/// A circle of diameter `d` with its top-left at (x, y), as four cubic arcs.
fn dot_path(x: f32, y: f32, d: f32) -> Path {
    const K: f32 = 0.552_284_8;
    let r = d / 2.0;
    let (cx, cy) = (x + r, y + r);
    let p = |px: f32, py: f32| Point { x: px, y: py };
    Path {
        commands: vec![
            PathCommand::MoveTo(p(cx + r, cy)),
            PathCommand::CubicTo(p(cx + r, cy + K * r), p(cx + K * r, cy + r), p(cx, cy + r)),
            PathCommand::CubicTo(p(cx - K * r, cy + r), p(cx - r, cy + K * r), p(cx - r, cy)),
            PathCommand::CubicTo(p(cx - r, cy - K * r), p(cx - K * r, cy - r), p(cx, cy - r)),
            PathCommand::CubicTo(p(cx + K * r, cy - r), p(cx + r, cy - K * r), p(cx + r, cy)),
            PathCommand::Close,
        ],
    }
}

/// The ribbon of a wavy line over `x0..x1` whose top edge starts at `top` (the line's thickness
/// is `t`, the whole extent `3t`): the centre line `top + 1.5t` swings `t` up and down.
fn wavy_path(x0: f32, x1: f32, top: f32, t: f32) -> Path {
    let width = (x1 - x0).max(0.0);
    let half = (2.0 * t).max(1.0);
    let n = ((width / half).round().max(1.0)) as usize;
    let step = width / n as f32;
    let centre = top + 1.5 * t;
    let p = |x: f32, y: f32| Point { x, y };
    let mut cmds = Vec::with_capacity(2 * n + 4);
    // Upper edge, left to right: alternating half-waves, up first.
    cmds.push(PathCommand::MoveTo(p(x0, centre - t / 2.0)));
    for i in 0..n {
        let dir = if i % 2 == 0 { -1.0 } else { 1.0 };
        let (a, b) = (x0 + step * i as f32, x0 + step * (i + 1) as f32);
        cmds.push(PathCommand::QuadTo(
            p((a + b) / 2.0, centre - t / 2.0 + dir * 2.0 * t),
            p(b, centre - t / 2.0),
        ));
    }
    // Lower edge, right to left: the same curve a thickness lower.
    cmds.push(PathCommand::LineTo(p(x0 + width, centre + t / 2.0)));
    for i in (0..n).rev() {
        let dir = if i % 2 == 0 { -1.0 } else { 1.0 };
        let (a, b) = (x0 + step * i as f32, x0 + step * (i + 1) as f32);
        cmds.push(PathCommand::QuadTo(
            p((a + b) / 2.0, centre + t / 2.0 + dir * 2.0 * t),
            p(a, centre + t / 2.0),
        ));
    }
    cmds.push(PathCommand::Close);
    Path { commands: cmds }
}

/// The draw operations of one decoration, each with its ink rectangle.
pub(crate) fn decoration_ops(d: &PaintDecoration) -> Vec<(DrawOp, Rect)> {
    use crate::scene::display_list::GlyphRotation as G;
    if d.turn != G::None {
        // A vertical line (015 WM-1): the pattern in the line's own frame (along x, over side at the top),
        // then every point turned into the physical rect.
        let p = d.rect;
        let logical = PaintDecoration {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: p.height,
                height: p.width,
            },
            turn: G::None,
            ..d.clone()
        };
        let cw = d.turn == G::Clockwise;
        let pt = |q: Point| {
            if cw {
                Point {
                    x: p.x + p.width - q.y,
                    y: p.y + q.x,
                }
            } else {
                Point {
                    x: p.x + q.y,
                    y: p.y + p.height - q.x,
                }
            }
        };
        let rect = |r: Rect| {
            let (a, b) = (
                pt(Point { x: r.x, y: r.y }),
                pt(Point {
                    x: r.x + r.width,
                    y: r.y + r.height,
                }),
            );
            Rect {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                width: (a.x - b.x).abs(),
                height: (a.y - b.y).abs(),
            }
        };
        return decoration_ops(&logical)
            .into_iter()
            .map(|(op, ink)| {
                let op = match op {
                    DrawOp::FillRect { rect: r, paint } => DrawOp::FillRect {
                        rect: rect(r),
                        paint,
                    },
                    DrawOp::FillPath {
                        path,
                        fill_rule,
                        paint,
                    } => {
                        let commands = path
                            .commands
                            .into_iter()
                            .map(|c| match c {
                                PathCommand::MoveTo(a) => PathCommand::MoveTo(pt(a)),
                                PathCommand::LineTo(a) => PathCommand::LineTo(pt(a)),
                                PathCommand::QuadTo(a, b) => PathCommand::QuadTo(pt(a), pt(b)),
                                PathCommand::CubicTo(a, b, e) => {
                                    PathCommand::CubicTo(pt(a), pt(b), pt(e))
                                }
                                PathCommand::Close => PathCommand::Close,
                            })
                            .collect();
                        DrawOp::FillPath {
                            path: Path { commands },
                            fill_rule,
                            paint,
                        }
                    }
                    other => other,
                };
                (op, rect(ink))
            })
            .collect();
    }
    let r = d.rect;
    let t = d.thickness;
    let paint = Paint::Solid(d.color);
    if !(t > 0.0 && r.width > 0.0 && t.is_finite() && r.width.is_finite()) || d.color.a <= 0.0 {
        return Vec::new();
    }
    let fill = |rect: Rect| {
        (
            DrawOp::FillRect {
                rect,
                paint: paint.clone(),
            },
            rect,
        )
    };
    let path = |path: Path| {
        let ink = crate::scene::appendix_e::path_bounds(&path).unwrap_or(r);
        (
            DrawOp::FillPath {
                path,
                fill_rule: FillRule::NonZero,
                paint: paint.clone(),
            },
            ink,
        )
    };
    let (x0, x1, top) = (r.x, r.x + r.width, r.y);
    match d.style {
        DecorationLineStyle::Solid => vec![fill(Rect {
            x: x0,
            y: top,
            width: r.width,
            height: t,
        })],
        DecorationLineStyle::Double => vec![
            fill(Rect {
                x: x0,
                y: top,
                width: r.width,
                height: t,
            }),
            fill(Rect {
                x: x0,
                y: top + t + 1.0,
                width: r.width,
                height: t,
            }),
        ],
        DecorationLineStyle::Dotted => {
            let mut out = Vec::new();
            let mut x = x0;
            while x + t <= x1 + 1e-3 && out.len() < MAX_PIECES {
                // Whole-pixel dots are squares (a 1 px circle is not a dot).
                out.push(if t < 2.0 {
                    fill(Rect {
                        x,
                        y: top,
                        width: t,
                        height: t,
                    })
                } else {
                    path(dot_path(x, top, t))
                });
                x += 2.0 * t;
            }
            out
        }
        DecorationLineStyle::Dashed => {
            let (dash, gap) = (3.0 * t, 3.0 * t);
            let mut out = Vec::new();
            let mut x = x0;
            while x < x1 - 1e-3 && out.len() < MAX_PIECES {
                out.push(fill(Rect {
                    x,
                    y: top,
                    width: dash.min(x1 - x),
                    height: t,
                }));
                x += dash + gap;
            }
            out
        }
        DecorationLineStyle::Wavy => vec![path(wavy_path(x0, x1, top, t))],
    }
}

/// The ink of a shadow layer of something that covers `base`: moved by the offset, grown by three
/// standard deviations of the blur.
pub(crate) fn shadow_ink(base: Rect, sh: &TextShadowLayer) -> Rect {
    let reach = 3.0 * (sh.blur.max(0.0) / 2.0);
    Rect {
        x: base.x + sh.offset_x - reach,
        y: base.y + sh.offset_y - reach,
        width: base.width + 2.0 * reach,
        height: base.height + 2.0 * reach,
    }
}

/// The scope a shadow layer paints in: a blur filter scope when the layer is blurred (sigma = blur / 2).
pub(crate) fn shadow_scope(
    b: &mut Builder,
    scope: Option<ScopeId>,
    sh: &TextShadowLayer,
) -> Option<ScopeId> {
    if sh.blur > 0.0 {
        Some(b.scope(
            scope,
            ScopeKind::Filter(FilterEffect {
                functions: vec![FilterFn::Blur(sh.blur / 2.0)],
            }),
        ))
    } else {
        scope
    }
}

/// Append the items of `d`: the decoration's own phase is the inline content of its context. The
/// decorating box's `text-shadow` layers go under the line (CSS Text Decoration 3 section 4.1: the
/// shadows apply to the decoration lines as to the glyphs).
pub(crate) fn push_decoration(
    b: &mut Builder,
    d: &PaintDecoration,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
) {
    for sh in d.shadows.iter().rev() {
        if sh.color.a <= 0.0 {
            continue;
        }
        let moved = PaintDecoration {
            rect: Rect {
                x: d.rect.x + sh.offset_x,
                y: d.rect.y + sh.offset_y,
                ..d.rect
            },
            color: sh.color,
            ..d.clone()
        };
        let sc = shadow_scope(b, scope, sh);
        for (op, ink) in decoration_ops(&moved) {
            let reach = 3.0 * (sh.blur.max(0.0) / 2.0);
            let ink = Rect {
                x: ink.x - reach,
                y: ink.y - reach,
                width: ink.width + 2.0 * reach,
                height: ink.height + 2.0 * reach,
            };
            out.push(Pending {
                owner: d.owner,
                phase: PaintPhase::InlineContent,
                kind: ItemKind::TEXT_SHADOW,
                ink,
                scope: sc,
                op,
            });
        }
    }
    for (op, ink) in decoration_ops(d) {
        out.push(Pending {
            owner: d.owner,
            phase: PaintPhase::InlineContent,
            kind: ItemKind::TEXT_DECORATION,
            ink,
            scope,
            op,
        });
    }
}

/// Whether a decoration is drawn after the line's glyphs (line-through) or before them.
pub(crate) fn after_text(kind: DecorationKind) -> bool {
    kind == DecorationKind::LineThrough
}
