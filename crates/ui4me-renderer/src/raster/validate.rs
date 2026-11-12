//! owner: 010-paint-render — created by 010 T001 (moved from the slice's lower.rs, no behaviour change)
//!
//! Backend-neutral pre-validation of display lists (AD-17): names no backend types.

use crate::scene::display_list::{
    ClipShape, Color, DisplayList, DrawOp, FilterFn, GradientKind, Paint, Path, PathCommand, Point,
    Rect, RoundedRect, ScopeKind,
};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
// Finite geometry of any magnitude is valid: a box of `width: 10000000000px` is valid CSS. The
// lowering clamps it (`LOCAL_LIMIT`), so a page never fails to render because of extreme geometry.

/// Largest device surface side. vello_cpu 0.3.0 contexts are u16-sized, but tile alignment overflows
/// u16 near the top of the range: widths above 65,408 and heights above 65,532 panic (X-VELLO-PIN
/// finding; capability-table.md section 4 row 1 delta), so both axes are limited to 65,408.
pub const MAX_SURFACE_SIDE: u32 = 65_408;

pub(crate) fn invariant(message: String) -> Status {
    Status::new(Code::InternalInvariant, Stage::Raster).with_message(message)
}

fn finite_point(p: &Point, _scale: f64) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn finite_rect(r: &Rect, _scale: f64) -> bool {
    [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()) && r.width >= 0.0 && r.height >= 0.0
}

fn valid_color(c: &Color) -> bool {
    [c.r, c.g, c.b, c.a]
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}

fn valid_paint(p: &Paint) -> bool {
    match p {
        Paint::Solid(c) => valid_color(c),
        Paint::Gradient(g) => {
            let geometry = match &g.kind {
                GradientKind::Linear { start, end } => {
                    [start.x, start.y, end.x, end.y]
                        .iter()
                        .all(|v| v.is_finite())
                        && (start.x != end.x || start.y != end.y)
                }
                GradientKind::Radial {
                    center,
                    rx,
                    ry,
                    inner,
                } => {
                    [center.x, center.y, *rx, *ry, *inner]
                        .iter()
                        .all(|v| v.is_finite())
                        && *rx > 0.0
                        && *ry > 0.0
                        && (0.0..1.0).contains(inner)
                }
                GradientKind::Conic {
                    center,
                    start_angle,
                    first,
                    last,
                } => {
                    [center.x, center.y, *start_angle, *first, *last]
                        .iter()
                        .all(|v| v.is_finite())
                        && last > first
                }
            };
            geometry
                && !g.stops.is_empty()
                && g.stops.len() <= MAX_STOPS
                && g.stops
                    .iter()
                    .all(|s| s.offset.is_finite() && valid_color(&s.color))
                && g.stops.windows(2).all(|w| w[0].offset <= w[1].offset)
        }
    }
}

/// Most gradient stops accepted per paint (hint expansion included).
pub(crate) const MAX_STOPS: usize = 1024;

fn valid_rrect(rr: &RoundedRect, scale: f64) -> bool {
    finite_rect(&rr.rect, scale)
        && rr
            .radii
            .iter()
            .all(|(x, y)| x.is_finite() && y.is_finite() && *x >= 0.0 && *y >= 0.0)
}

fn valid_path(path: &Path, scale: f64) -> bool {
    matches!(path.commands.first(), None | Some(PathCommand::MoveTo(_)))
        && path.commands.iter().all(|c| match c {
            PathCommand::MoveTo(p) | PathCommand::LineTo(p) => finite_point(p, scale),
            PathCommand::QuadTo(a, b) => finite_point(a, scale) && finite_point(b, scale),
            PathCommand::CubicTo(a, b, c) => {
                finite_point(a, scale) && finite_point(b, scale) && finite_point(c, scale)
            }
            PathCommand::Close => true,
        })
}

/// Most layers of one mask accepted.
pub(crate) const MAX_MASK_LAYERS: usize = 64;
/// Most draw operations of one mask layer accepted (the scene caps tiles per layer).
pub(crate) const MAX_MASK_OPS: usize = 8192;

/// Most functions of one `filter` accepted.
pub(crate) const MAX_FILTER_FUNCTIONS: usize = 64;

fn valid_filter_fn(f: &FilterFn) -> bool {
    match *f {
        FilterFn::Blur(v)
        | FilterFn::Brightness(v)
        | FilterFn::Contrast(v)
        | FilterFn::Grayscale(v)
        | FilterFn::Invert(v)
        | FilterFn::Opacity(v)
        | FilterFn::Saturate(v)
        | FilterFn::Sepia(v) => v.is_finite() && v >= 0.0,
        FilterFn::HueRotate(v) => v.is_finite(),
        FilterFn::DropShadow {
            dx,
            dy,
            blur,
            color,
        } => {
            [dx, dy].iter().all(|v| v.is_finite())
                && blur.is_finite()
                && blur >= 0.0
                && valid_color(&color)
        }
    }
}

/// Deepest scope nesting accepted (clip, effect and spatial scopes together): the backend's layer
/// stack is bounded.
pub(crate) const MAX_SCOPE_DEPTH: usize = 128;

fn validate_scopes(list: &DisplayList, scale: f64) -> Result<(), Status> {
    for (i, scope) in list.scopes.iter().enumerate() {
        if let Some(p) = scope.parent.filter(|p| p.0 as usize >= i) {
            return Err(invariant(format!(
                "scope {i}: parent {} does not precede it",
                p.0
            )));
        }
        let ok = match &scope.kind {
            ScopeKind::Spatial(a) => a.is_finite() && a.determinant() != 0.0,
            ScopeKind::Clip(ClipShape::Rect(r)) => finite_rect(r, scale),
            ScopeKind::Clip(ClipShape::RoundedRect(rr)) => valid_rrect(rr, scale),
            ScopeKind::Clip(ClipShape::Path { path, .. }) => valid_path(path, scale),
            ScopeKind::Blend(_) => true,
            ScopeKind::Effect(e) => e.opacity.is_finite() && (0.0..=1.0).contains(&e.opacity),
            ScopeKind::Filter(f) => {
                !f.functions.is_empty()
                    && f.functions.len() <= MAX_FILTER_FUNCTIONS
                    && f.functions.iter().all(valid_filter_fn)
            }
            ScopeKind::Mask(m) => {
                !m.layers.is_empty()
                    && m.layers.len() <= MAX_MASK_LAYERS
                    && m.layers
                        .iter()
                        .all(|l| valid_rrect(&l.clip, scale) && l.ops.len() <= MAX_MASK_OPS)
                    && m.layers
                        .iter()
                        .flat_map(|l| &l.ops)
                        .all(|op| validate_op(0, op, scale).is_ok())
            }
        };
        if !ok {
            return Err(invariant(format!("scope {i}: invalid scope payload")));
        }
    }
    for item in &list.items {
        if let Some(s) = item.scope {
            if s.0 as usize >= list.scopes.len() {
                return Err(invariant(format!(
                    "item {}: scope {} does not exist",
                    item.id.0, s.0
                )));
            }
            if list.scope_chain(Some(s)).len() > MAX_SCOPE_DEPTH {
                return Err(
                    Status::new(Code::BudgetExceeded, Stage::Raster).with_message(format!(
                        "item {}: scope nesting deeper than {MAX_SCOPE_DEPTH}",
                        item.id.0
                    )),
                );
            }
        }
    }
    Ok(())
}

/// Validate one draw operation (finite geometry, valid paint); `id` names the item in the message.
fn validate_op(id: u32, op: &DrawOp, s: f64) -> Result<(), Status> {
    match op {
        DrawOp::BackdropFilter {
            clip, functions, ..
        } => {
            if !valid_rrect(clip, s)
                || functions.is_empty()
                || functions.len() > MAX_FILTER_FUNCTIONS
                || !functions.iter().all(valid_filter_fn)
            {
                return Err(invariant(format!("item {id}: invalid backdrop filter")));
            }
        }
        DrawOp::FillRect { rect, paint } => {
            if !finite_rect(rect, s) || !valid_paint(paint) {
                return Err(invariant(format!("item {id}: invalid FillRect")));
            }
        }
        DrawOp::FillRoundedRect { rect, paint } => {
            if !valid_rrect(rect, s) || !valid_paint(paint) {
                return Err(invariant(format!("item {id}: invalid FillRoundedRect")));
            }
        }
        DrawOp::FillPath { path, paint, .. } => {
            if !valid_paint(paint) {
                return Err(invariant(format!("item {id}: invalid FillPath paint")));
            }
            // A path starts with MoveTo; every point is finite.
            if !valid_path(path, s) {
                return Err(invariant(format!(
                    "item {id}: path does not start with MoveTo or a point is not finite or out of range"
                )));
            }
        }
        DrawOp::DrawImage { dest, visible, .. } => {
            if !finite_rect(dest, s)
                || !finite_rect(visible, s)
                || dest.width <= 0.0
                || dest.height <= 0.0
            {
                return Err(invariant(format!("item {id}: invalid DrawImage")));
            }
        }
        DrawOp::DrawImagePattern { tile, fill, .. } => {
            if !finite_rect(tile, s)
                || !finite_rect(fill, s)
                || tile.width <= 0.0
                || tile.height <= 0.0
            {
                return Err(invariant(format!("item {id}: invalid DrawImagePattern")));
            }
        }
        DrawOp::DrawBoxShadow {
            border_rrect,
            padding_rrect,
            offset,
            blur_radius,
            spread,
            color,
            ..
        } => {
            let ok = valid_rrect(border_rrect, s)
                && valid_rrect(padding_rrect, s)
                && [offset.0, offset.1, *blur_radius, *spread]
                    .iter()
                    .all(|v| v.is_finite())
                && *blur_radius >= 0.0
                && valid_color(color);
            if !ok {
                return Err(invariant(format!("item {id}: invalid DrawBoxShadow")));
            }
        }
        DrawOp::DrawGlyphRun(run) => {
            // `font-size: 0` is valid CSS: the run paints nothing (the lowering skips it).
            if !(run.size_px.is_finite() && run.size_px >= 0.0) || !valid_paint(&run.paint) {
                return Err(invariant(format!(
                    "item {id}: invalid glyph run size or paint"
                )));
            }
            if !run
                .glyphs
                .iter()
                .all(|g| finite_point(&Point { x: g.x, y: g.y }, s))
            {
                return Err(invariant(format!(
                    "item {id}: glyph position not finite or out of range"
                )));
            }
        }
    }
    Ok(())
}

/// Validate every item (finite geometry, supported op, device size within limits) at `scale`.
/// An item that fails is an `INTERNAL_INVARIANT` (scene bug) or `SIZE_LIMIT` with stage `raster`.
pub fn prevalidate(list: &DisplayList, scale: f32) -> Result<(), Status> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(
            Status::new(Code::SizeLimit, Stage::Raster).with_message(format!(
                "device scale {scale} is not a positive finite number"
            )),
        );
    }
    let s = scale as f64;
    validate_scopes(list, s)?;
    for item in &list.items {
        let id = item.id.0;
        if !finite_rect(&item.ink, s) {
            return Err(invariant(format!(
                "item {id}: ink bounds not finite or out of range"
            )));
        }
        validate_op(id, &item.op, s)?;
        if let DrawOp::BackdropFilter {
            backdrop_root: Some(root),
            ..
        } = &item.op
            && !list.scope_chain(item.scope).contains(root)
        {
            return Err(invariant(format!(
                "item {id}: backdrop root is not an ancestor scope"
            )));
        }
    }
    Ok(())
}
