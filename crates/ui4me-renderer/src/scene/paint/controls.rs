//! owner: 010-paint-render — lane L-CTRL-r10 (012 `contracts/controls.md` section 5)
//!
//! The themed parts of form controls, drawn with one deterministic ui4me theme (OD-28): the
//! `select` drop-down arrow, its open popup panel with the highlight, the listbox rows, the
//! `range` track and thumb, the `progress` and `meter` bars, and the checked marks of `checkbox`
//! and `radio` (only when `appearance` is not `none`, so Bootstrap-style author drawings are not
//! doubled). Tints derive from the control's text colour (`fg`) so one set of tokens serves the
//! light and the dark scheme; the accent comes from the inherited CSS value or the default theme.
//!
//! Order within the control's own stacking position: the popup panel and the row highlights go
//! *behind* the control's text lines ([`push_behind_text`]); the arrow, bars, thumbs and marks, the
//! dimming of disabled rows go over them ([`push_foreground`]).

use crate::scene::appendix_e::{PaintBox, PaintPhase, Pending};
use crate::scene::display_list::{
    DrawOp, FillRule, ItemKind, Paint, Path, PathCommand as C, Point, Rect, RoundedRect, ScopeId,
};
use crate::scene::inputs::{ControlKind, ControlPaint, MeterRegion};
use crate::scene::theme;

fn item(bx: &PaintBox, ink: Rect, scope: Option<ScopeId>, op: DrawOp) -> Pending {
    Pending {
        owner: bx.node,
        phase: PaintPhase::InlineContent,
        kind: ItemKind::CONTROL,
        ink,
        scope,
        op,
    }
}

fn fill(
    bx: &PaintBox,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
    rect: Rect,
    color: crate::scene::display_list::Color,
) {
    if rect.width > 0.0 && rect.height > 0.0 && color.a > 0.0 {
        out.push(item(
            bx,
            rect,
            scope,
            DrawOp::FillRect {
                rect,
                paint: Paint::Solid(color),
            },
        ));
    }
}

fn round(
    bx: &PaintBox,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
    rect: Rect,
    radius: f32,
    color: crate::scene::display_list::Color,
) {
    if rect.width > 0.0 && rect.height > 0.0 && color.a > 0.0 {
        let r = radius.min(rect.width / 2.0).min(rect.height / 2.0);
        out.push(item(
            bx,
            rect,
            scope,
            DrawOp::FillRoundedRect {
                rect: RoundedRect {
                    rect,
                    radii: [(r, r); 4],
                },
                paint: Paint::Solid(color),
            },
        ));
    }
}

fn poly(
    bx: &PaintBox,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
    pts: &[(f32, f32)],
    color: crate::scene::display_list::Color,
) {
    let mut commands = Vec::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        let p = Point { x: *x, y: *y };
        commands.push(if i == 0 { C::MoveTo(p) } else { C::LineTo(p) });
    }
    commands.push(C::Close);
    let path = Path { commands };
    let ink = crate::scene::appendix_e::path_bounds(&path).unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    });
    out.push(item(
        bx,
        ink,
        scope,
        DrawOp::FillPath {
            path,
            fill_rule: FillRule::NonZero,
            paint: Paint::Solid(color),
        },
    ));
}

/// A thick polyline (a check mark, an arrow) as a stroked outline polygon, `w` px wide.
fn stroke(
    bx: &PaintBox,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
    pts: &[(f32, f32)],
    w: f32,
    color: crate::scene::display_list::Color,
) {
    for pair in pts.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (nx, ny) = (-dy / len * w / 2.0, dx / len * w / 2.0);
        poly(
            bx,
            out,
            scope,
            &[
                (x0 + nx, y0 + ny),
                (x1 + nx, y1 + ny),
                (x1 - nx, y1 - ny),
                (x0 - nx, y0 - ny),
            ],
            color,
        );
    }
}

fn circle(
    bx: &PaintBox,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
    cx: f32,
    cy: f32,
    r: f32,
    color: crate::scene::display_list::Color,
) {
    round(
        bx,
        out,
        scope,
        Rect {
            x: cx - r,
            y: cy - r,
            width: 2.0 * r,
            height: 2.0 * r,
        },
        r,
        color,
    );
}

/// Parts behind the control's text: a drop-down's open popup panel and the row highlights of the
/// popup and of a listbox.
pub(crate) fn push_behind_text(
    bx: &PaintBox,
    c: &ControlPaint,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
) {
    if let Some(p) = c.panel {
        // Border, then the panel colour.
        fill(bx, out, scope, p, theme::with_alpha(c.fg, 0.45));
        fill(
            bx,
            out,
            scope,
            Rect {
                x: p.x + 1.0,
                y: p.y + 1.0,
                width: p.width - 2.0,
                height: p.height - 2.0,
            },
            c.bg,
        );
    }
    for r in &c.rows {
        if r.highlighted {
            fill(
                bx,
                out,
                scope,
                r.rect,
                theme::with_alpha(theme::ACCENT, 0.35),
            );
        } else if r.selected && !c.facts.disabled {
            // The selected option of an open popup / of a listbox: the accent while focused, grey else.
            let tint = if c.facts.focused || c.panel.is_some() {
                theme::with_alpha(theme::ACCENT, 0.2)
            } else {
                theme::with_alpha(c.fg, 0.15)
            };
            fill(bx, out, scope, r.rect, tint);
        }
    }
}

/// Parts over the control's text: dimming of disabled rows.
pub(crate) fn push_over_text(
    bx: &PaintBox,
    c: &ControlPaint,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
) {
    for r in c.rows.iter().filter(|r| r.disabled) {
        fill(bx, out, scope, r.rect, theme::with_alpha(c.bg, 0.55));
    }
}

/// The control's own drawing: arrow, range, bars and marks.
pub(crate) fn push_foreground(
    bx: &PaintBox,
    c: &ControlPaint,
    out: &mut Vec<Pending>,
    scope: Option<ScopeId>,
) {
    let dim = if c.facts.disabled { 0.5 } else { 1.0 };
    // An author-styled control does not take the CSS accent; range/progress retain
    // the engine's existing fallback drawing when appearance is none.
    let control_accent = if c.facts.native {
        c.accent
    } else {
        theme::ACCENT
    };
    let accent = theme::with_alpha(control_accent, dim);
    let ink = theme::with_alpha(c.fg, 0.7 * dim);
    let track = theme::with_alpha(c.fg, 0.25 * dim);
    let r = c.content;
    match &c.facts.kind {
        ControlKind::Select => {
            // The drop-down arrow: a chevron in the right padding area of the content box, only when the
            // control draws its own parts (`appearance: auto`).
            if !c.listbox && c.facts.native {
                let b = c.border_box;
                let (cx, cy) = (b.x + b.width - 10.0, b.y + b.height / 2.0);
                stroke(
                    bx,
                    out,
                    scope,
                    &[(cx - 4.0, cy - 2.0), (cx, cy + 2.0), (cx + 4.0, cy - 2.0)],
                    1.6,
                    ink,
                );
            }
        }
        ControlKind::Checkbox {
            checked,
            indeterminate,
        } if c.facts.native => {
            let b = Rect {
                x: r.x,
                y: r.y,
                width: r.width.min(r.height),
                height: r.width.min(r.height),
            };
            if *checked || *indeterminate {
                round(bx, out, scope, b, 2.0, accent);
                let white = if theme::is_light(c.accent) {
                    crate::scene::display_list::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: dim,
                    }
                } else {
                    crate::scene::display_list::Color {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: dim,
                    }
                };
                if *indeterminate {
                    fill(
                        bx,
                        out,
                        scope,
                        Rect {
                            x: b.x + b.width * 0.25,
                            y: b.y + b.height / 2.0 - 1.0,
                            width: b.width * 0.5,
                            height: 2.0,
                        },
                        white,
                    );
                } else {
                    stroke(
                        bx,
                        out,
                        scope,
                        &[
                            (b.x + b.width * 0.24, b.y + b.height * 0.52),
                            (b.x + b.width * 0.43, b.y + b.height * 0.72),
                            (b.x + b.width * 0.78, b.y + b.height * 0.30),
                        ],
                        1.8,
                        white,
                    );
                }
            } else {
                round(bx, out, scope, b, 2.0, theme::with_alpha(c.fg, 0.55 * dim));
                round(
                    bx,
                    out,
                    scope,
                    Rect {
                        x: b.x + 1.0,
                        y: b.y + 1.0,
                        width: b.width - 2.0,
                        height: b.height - 2.0,
                    },
                    1.5,
                    c.bg,
                );
            }
        }
        ControlKind::Radio { checked } if c.facts.native => {
            let d = r.width.min(r.height);
            let (cx, cy) = (r.x + d / 2.0, r.y + d / 2.0);
            if *checked {
                circle(bx, out, scope, cx, cy, d / 2.0, accent);
                circle(bx, out, scope, cx, cy, d / 2.0 - 1.5, c.bg);
                circle(bx, out, scope, cx, cy, d / 2.0 - 3.5, accent);
            } else {
                circle(
                    bx,
                    out,
                    scope,
                    cx,
                    cy,
                    d / 2.0,
                    theme::with_alpha(c.fg, 0.55 * dim),
                );
                circle(bx, out, scope, cx, cy, d / 2.0 - 1.0, c.bg);
            }
        }
        ControlKind::Checkbox { .. } | ControlKind::Radio { .. } => {}
        ControlKind::Range { ratio } => {
            // A 4 px track, the filled part in the accent, a 16 px thumb (controls.md section 4: 129 x 16).
            let thumb = r.height.clamp(8.0, 16.0);
            let (x0, x1) = (r.x + thumb / 2.0, r.x + r.width - thumb / 2.0);
            let cy = r.y + r.height / 2.0;
            let tx = x0 + (x1 - x0).max(0.0) * ratio.clamp(0.0, 1.0);
            let tr = Rect {
                x: r.x,
                y: cy - 2.0,
                width: r.width,
                height: 4.0,
            };
            round(bx, out, scope, tr, 2.0, track);
            round(
                bx,
                out,
                scope,
                Rect {
                    x: r.x,
                    y: cy - 2.0,
                    width: (tx - r.x).max(0.0),
                    height: 4.0,
                },
                2.0,
                accent,
            );
            circle(bx, out, scope, tx, cy, thumb / 2.0, accent);
            if c.facts.focused {
                circle(
                    bx,
                    out,
                    scope,
                    tx,
                    cy,
                    thumb / 2.0 + 2.0,
                    theme::with_alpha(control_accent, 0.3),
                );
                circle(bx, out, scope, tx, cy, thumb / 2.0, accent);
            }
        }
        ControlKind::Progress { ratio } => {
            round(
                bx,
                out,
                scope,
                r,
                r.height / 2.0,
                theme::with_alpha(c.fg, 0.18),
            );
            match ratio {
                Some(v) => round(
                    bx,
                    out,
                    scope,
                    Rect {
                        width: r.width * v.clamp(0.0, 1.0),
                        ..r
                    },
                    r.height / 2.0,
                    accent,
                ),
                None => {
                    // Indeterminate: a stripe pattern of 6 px accent stripes every 12 px.
                    let mut x = r.x;
                    while x < r.x + r.width {
                        fill(
                            bx,
                            out,
                            scope,
                            Rect {
                                x,
                                y: r.y,
                                width: 6.0f32.min(r.x + r.width - x),
                                height: r.height,
                            },
                            theme::with_alpha(control_accent, 0.55 * dim),
                        );
                        x += 12.0;
                    }
                }
            }
        }
        ControlKind::Meter { ratio, region } => {
            round(
                bx,
                out,
                scope,
                r,
                r.height / 2.0,
                theme::with_alpha(c.fg, 0.18),
            );
            let color = match region {
                MeterRegion::Optimum => theme::METER_GOOD,
                MeterRegion::Suboptimal => theme::METER_WARN,
                MeterRegion::EvenLessGood => theme::METER_BAD,
            };
            round(
                bx,
                out,
                scope,
                Rect {
                    width: r.width * ratio.clamp(0.0, 1.0),
                    ..r
                },
                r.height / 2.0,
                color,
            );
        }
        ControlKind::Color { rgb } => {
            let sw = Rect {
                x: r.x + 2.0,
                y: r.y + 2.0,
                width: (r.width - 4.0).max(0.0),
                height: (r.height - 4.0).max(0.0),
            };
            fill(
                bx,
                out,
                scope,
                Rect {
                    x: sw.x - 1.0,
                    y: sw.y - 1.0,
                    width: sw.width + 2.0,
                    height: sw.height + 2.0,
                },
                theme::with_alpha(c.fg, 0.5),
            );
            fill(
                bx,
                out,
                scope,
                sw,
                crate::scene::display_list::Color {
                    r: rgb[0],
                    g: rgb[1],
                    b: rgb[2],
                    a: 1.0,
                },
            );
        }
    }
}
