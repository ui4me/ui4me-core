//! owner: 010-paint-render — created by 010 T001; gradient layers by 010 T068/T069 (subset)
//!
//! `background-image` gradient layers (CSS Backgrounds 3 section 3, CSS Images 3 section 3):
//! positioning area, tile size, repetition, background-clip, and the gradient geometry with
//! colour-stop fix-up (section 3.5.3) and hint expansion. Pure functions; the traversal in
//! `appendix_e` turns a [`LayerPlan`] into display items.

use std::sync::Arc;

use crate::scene::appendix_e::{Builder, PaintBox, PaintPhase, Pending};
use crate::scene::borders::{fit_radii, inset_rrect};
use crate::scene::display_list::{
    ClipShape, Color, ColorStop, DrawOp, Extend, Gradient, GradientKind, ItemKind, Point, Rect,
    RoundedRect, ScopeId, ScopeKind,
};
use crate::scene::inputs::{
    BgBox, BgImage, BgLayerSpec, BgSize, GradientSpec, Hint, ImageData, LenPct, LinearDirection,
    RadialSize, Repeat, StopPosition, StopSpec, padding_box,
};

/// Upper bound on tiles per layer (a tiny `background-size` over a large area); beyond it the
/// layer is not painted and the caller reports it.
pub const MAX_TILES: usize = 4096;

/// Samples inserted per colour hint (the power curve is sampled linearly between them).
const HINT_SAMPLES: usize = 16;

/// What to draw for one layer.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerPlan {
    /// The background-clip area; tiles are cut to it, and rounded corners clip them.
    pub clip: RoundedRect,
    /// Gradient layers: `(rect to fill, gradient in that tile's space)`, in paint order.
    pub tiles: Vec<(Rect, Gradient)>,
    /// Image layers.
    pub image: Option<ImagePlan>,
}

/// How an image layer is drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum ImagePlan {
    /// Individual tiles (`no-repeat`, `space`, or any combination that leaves gaps): `(tile rect
    /// the whole image maps onto, visible part)`.
    Tiles(Vec<(Rect, Rect)>),
    /// A gap-free repetition (`repeat`, `round`, mixed with `no-repeat`): one pattern.
    Pattern {
        tile: Rect,
        repeat_x: bool,
        repeat_y: bool,
        fill: Rect,
    },
}

/// Why a layer could not be planned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerError {
    /// More than [`MAX_TILES`] tiles.
    TooManyTiles,
    /// Degenerate gradient or tile geometry (zero size): the layer paints nothing.
    Empty,
}

fn rect_isect(a: &Rect, b: &Rect) -> Option<Rect> {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// The box a `background-origin`/`-clip` value names, with radii.
pub fn bg_box(
    which: BgBox,
    border_box: &Rect,
    widths: [f32; 4],
    padding: [f32; 4],
    radii: [(f32, f32); 4],
) -> RoundedRect {
    let outer = RoundedRect {
        rect: *border_box,
        radii: fit_radii(border_box, radii),
    };
    match which {
        // `text` clips to the glyphs inside the border box: the mask scope cuts, the area is the border box.
        BgBox::BorderBox | BgBox::Text => outer,
        BgBox::PaddingBox => inset_rrect(&outer, widths[0], widths[1], widths[2], widths[3]),
        BgBox::ContentBox => inset_rrect(
            &outer,
            widths[0] + padding[0],
            widths[1] + padding[1],
            widths[2] + padding[2],
            widths[3] + padding[3],
        ),
    }
}

/// What the layer's image knows about its own size (a gradient knows nothing).
#[derive(Clone, Copy, Debug, Default)]
struct Intrinsic {
    w: Option<f32>,
    h: Option<f32>,
    ratio: Option<f32>,
}

fn intrinsic_of(image: &BgImage) -> Intrinsic {
    match image {
        BgImage::Gradient(_) => Intrinsic::default(),
        BgImage::Raster(d) => {
            let (w, h) = (
                d.natural.0.filter(|v| *v > 0.0),
                d.natural.1.filter(|v| *v > 0.0),
            );
            let ratio = d
                .ratio
                .filter(|r| r.is_finite() && *r > 0.0)
                .or_else(|| w.zip(h).map(|(w, h)| w / h));
            Intrinsic { w, h, ratio }
        }
    }
}

/// The concrete tile size (CSS Backgrounds 3 section 3.9 with the default sizing algorithm of
/// CSS Images 3 section 5.3: a gradient or an image without natural size is as large as the
/// positioning area). `None` for a degenerate size.
fn tile_size(size: &BgSize, area: &Rect, i: Intrinsic) -> Option<(f32, f32)> {
    let contain = |ratio: f32| {
        if area.width / area.height > ratio {
            (area.height * ratio, area.height)
        } else {
            (area.width, area.width / ratio)
        }
    };
    let cover = |ratio: f32| {
        if area.width / area.height > ratio {
            (area.width, area.width / ratio)
        } else {
            (area.height * ratio, area.height)
        }
    };
    let auto = || -> (f32, f32) {
        match (i.w, i.h, i.ratio) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some(r)) => (w, w / r),
            (None, Some(h), Some(r)) => (h * r, h),
            (None, None, Some(r)) => contain(r),
            (Some(w), None, None) => (w, area.height),
            (None, Some(h), None) => (area.width, h),
            (None, None, None) => (area.width, area.height),
        }
    };
    let (w, h) = match *size {
        // Without a ratio `contain` and `cover` have nothing to preserve: the image is as large
        // as the positioning area (CSS Images 3 section 5.3.6, the default sizing algorithm with
        // the area as specified size; an SVG with only a width or a height).
        BgSize::Cover => match i.ratio {
            Some(r) => cover(r),
            None => (area.width, area.height),
        },
        BgSize::Contain => match i.ratio {
            Some(r) => contain(r),
            None => (area.width, area.height),
        },
        BgSize::Explicit(w, h) => {
            let (rw, rh) = (
                w.map(|l| l.resolve(area.width)),
                h.map(|l| l.resolve(area.height)),
            );
            match (rw, rh) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (w, i.ratio.map(|r| w / r).or(i.h).unwrap_or(area.height)),
                (None, Some(h)) => (i.ratio.map(|r| h * r).or(i.w).unwrap_or(area.width), h),
                (None, None) => auto(),
            }
        }
    };
    (w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()).then_some((w, h))
}

/// One axis of the tile grid.
#[derive(Clone, Copy, Debug)]
struct Axis {
    /// Tile length along the axis (rescaled for `round`).
    len: f32,
    /// Start of one tile.
    start: f32,
    /// Distance between tile starts (`len` plus the `space` gap).
    period: f32,
    repeats: bool,
}

impl Axis {
    fn gapless(&self) -> bool {
        (self.period - self.len).abs() < 1e-4
    }

    /// Indices of the tiles intersecting `[lo, hi)`.
    fn indices(&self, lo: f32, hi: f32) -> (i64, i64) {
        if !self.repeats {
            return (0, 0);
        }
        (
            ((lo - self.start) / self.period).floor() as i64,
            ((hi - self.start) / self.period).ceil() as i64 - 1,
        )
    }
}

fn axis(repeat: Repeat, area_start: f32, area_len: f32, tile: f32, pos: LenPct) -> Axis {
    match repeat {
        Repeat::Repeat => Axis {
            len: tile,
            start: area_start + pos.resolve(area_len - tile),
            period: tile,
            repeats: true,
        },
        Repeat::NoRepeat => Axis {
            len: tile,
            start: area_start + pos.resolve(area_len - tile),
            period: tile,
            repeats: false,
        },
        Repeat::Round => {
            let n = (area_len / tile).round().max(1.0);
            let len = area_len / n;
            Axis {
                len,
                start: area_start + pos.resolve(area_len - len),
                period: len,
                repeats: true,
            }
        }
        Repeat::Space => {
            let n = (area_len / tile).floor();
            if n >= 2.0 {
                // The tiles are spread over the area; `background-position` does not apply.
                Axis {
                    len: tile,
                    start: area_start,
                    period: tile + (area_len - n * tile) / (n - 1.0),
                    repeats: true,
                }
            } else {
                Axis {
                    len: tile,
                    start: area_start + pos.resolve(area_len - tile),
                    period: tile,
                    repeats: false,
                }
            }
        }
    }
}

/// Plan one layer for a box: its border box, border widths, padding and radii.
pub fn plan_layer(
    layer: &BgLayerSpec,
    border_box: &Rect,
    widths: [f32; 4],
    padding: [f32; 4],
    radii: [(f32, f32); 4],
) -> Result<LayerPlan, LayerError> {
    plan_layer_in(layer, border_box, widths, padding, radii, None)
}

fn plan_layer_in(
    layer: &BgLayerSpec,
    border_box: &Rect,
    widths: [f32; 4],
    padding: [f32; 4],
    radii: [(f32, f32); 4],
    canvas: Option<Rect>,
) -> Result<LayerPlan, LayerError> {
    let clip = canvas
        .map(RoundedRect::square)
        .unwrap_or_else(|| bg_box(layer.clip, border_box, widths, padding, radii));
    let area = bg_box(layer.origin, border_box, widths, padding, radii).rect;
    if clip.rect.width <= 0.0 || clip.rect.height <= 0.0 || area.width <= 0.0 || area.height <= 0.0
    {
        return Err(LayerError::Empty);
    }
    let intrinsic = intrinsic_of(&layer.image);
    let (mut tw, mut th) = tile_size(&layer.size, &area, intrinsic).ok_or(LayerError::Empty)?;
    // `round` on one axis with `auto` on the other keeps the aspect ratio (section 3.9).
    let auto_h = matches!(layer.size, BgSize::Explicit(_, None));
    let auto_w = matches!(layer.size, BgSize::Explicit(None, _));
    let (rw, rh) = (tw, th);
    if layer.repeat.0 == Repeat::Round {
        tw = area.width / (area.width / rw).round().max(1.0);
        if auto_h && layer.repeat.1 != Repeat::Round {
            th = rh * tw / rw;
        }
    }
    if layer.repeat.1 == Repeat::Round {
        th = area.height / (area.height / rh).round().max(1.0);
        if auto_w && layer.repeat.0 != Repeat::Round {
            tw = rw * th / rh;
        }
    }
    let ax = axis(layer.repeat.0, area.x, area.width, tw, layer.position.0);
    let ay = axis(layer.repeat.1, area.y, area.height, th, layer.position.1);
    let (ix0, ix1) = ax.indices(clip.rect.x, clip.rect.right());
    let (iy0, iy1) = ay.indices(clip.rect.y, clip.rect.bottom());
    let tile_rect = |ix: i64, iy: i64| Rect {
        x: ax.start + ix as f32 * ax.period,
        y: ay.start + iy as f32 * ay.period,
        width: ax.len,
        height: ay.len,
    };
    let count = ((ix1 - ix0 + 1).max(0) as u128) * ((iy1 - iy0 + 1).max(0) as u128);
    match &layer.image {
        BgImage::Gradient(spec) => {
            if count > MAX_TILES as u128 {
                return Err(LayerError::TooManyTiles);
            }
            let mut tiles = Vec::new();
            for iy in iy0..=iy1 {
                for ix in ix0..=ix1 {
                    let tile = tile_rect(ix, iy);
                    let Some(fill) = rect_isect(&tile, &clip.rect) else {
                        continue;
                    };
                    if let Some(g) = resolve_gradient(spec, &tile) {
                        tiles.push((fill, g));
                    }
                }
            }
            if tiles.is_empty() {
                return Err(LayerError::Empty);
            }
            Ok(LayerPlan {
                clip,
                tiles,
                image: None,
            })
        }
        BgImage::Raster(_) if ax.gapless() && ay.gapless() => {
            // One pattern; along a non-repeating axis the fill is limited to the single tile.
            let tile = tile_rect(0, 0);
            let span = |repeats: bool, t0: f32, t1: f32, c0: f32, c1: f32| {
                if repeats {
                    (c0, c1)
                } else {
                    (t0.max(c0), t1.min(c1))
                }
            };
            let (x0, x1) = span(
                ax.repeats,
                tile.x,
                tile.right(),
                clip.rect.x,
                clip.rect.right(),
            );
            let (y0, y1) = span(
                ay.repeats,
                tile.y,
                tile.bottom(),
                clip.rect.y,
                clip.rect.bottom(),
            );
            if !(x1 > x0 && y1 > y0) {
                return Err(LayerError::Empty);
            }
            let fill = Rect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            };
            Ok(LayerPlan {
                clip,
                tiles: Vec::new(),
                image: Some(ImagePlan::Pattern {
                    tile,
                    repeat_x: ax.repeats,
                    repeat_y: ay.repeats,
                    fill,
                }),
            })
        }
        BgImage::Raster(_) => {
            if count > MAX_TILES as u128 {
                return Err(LayerError::TooManyTiles);
            }
            let mut tiles = Vec::new();
            for iy in iy0..=iy1 {
                for ix in ix0..=ix1 {
                    let tile = tile_rect(ix, iy);
                    if let Some(vis) = rect_isect(&tile, &clip.rect) {
                        tiles.push((tile, vis));
                    }
                }
            }
            if tiles.is_empty() {
                return Err(LayerError::Empty);
            }
            Ok(LayerPlan {
                clip,
                tiles: Vec::new(),
                image: Some(ImagePlan::Tiles(tiles)),
            })
        }
    }
}

/// The image of a planned layer for its tile size: a vector image rasterised at that size, any
/// other image unchanged. Tiles of one layer share one size (`round` and `space` included).
pub(crate) fn sized_for(data: &Arc<ImageData>, plan: &ImagePlan) -> Arc<ImageData> {
    let tile = match plan {
        ImagePlan::Tiles(t) => match t.first() {
            Some((dest, _)) => *dest,
            None => return data.clone(),
        },
        ImagePlan::Pattern { tile, .. } => *tile,
    };
    data.for_size((tile.width, tile.height))
}

/// The padding box helper re-exported for the traversal.
pub fn padding_of(border_box: &Rect, widths: [f32; 4]) -> Rect {
    padding_box(border_box, widths)
}

/// Append the items of one layer of `bx` (tiles in paint order, inside a clip scope when the
/// background-clip area has rounded corners). A layer with too many tiles is reported and not
/// painted.
pub(crate) fn push_layer(
    b: &mut Builder,
    bx: &PaintBox,
    layer: &BgLayerSpec,
    widths: [f32; 4],
    out: &mut Vec<Pending>,
    phase: PaintPhase,
    scope: Option<ScopeId>,
) {
    let border_box = Rect {
        x: bx.border_box.origin.x.to_f64() as f32,
        y: bx.border_box.origin.y.to_f64() as f32,
        width: bx.border_box.size.width.to_f64() as f32,
        height: bx.border_box.size.height.to_f64() as f32,
    };
    let e = &bx.extras;
    let plan = match plan_layer_in(
        layer,
        &border_box,
        widths,
        e.padding,
        e.radii,
        e.canvas_blend_color.map(|(_, r)| r),
    ) {
        Ok(p) => p,
        Err(LayerError::TooManyTiles) => {
            b.report("css.property.background-size", bx.node);
            return;
        }
        Err(LayerError::Empty) => return,
    };
    let scope = if plan.clip.is_square() {
        scope
    } else {
        Some(b.scope(scope, ScopeKind::Clip(ClipShape::RoundedRect(plan.clip))))
    };
    for (rect, gradient) in plan.tiles {
        out.push(Pending {
            owner: bx.node,
            phase,
            kind: ItemKind::BACKGROUND_IMAGE,
            ink: rect,
            scope,
            op: DrawOp::FillRect {
                rect,
                paint: paint_of(gradient),
            },
        });
    }
    let (BgImage::Raster(data), Some(image)) = (&layer.image, plan.image) else {
        return;
    };
    // A vector image is rasterised at the tile size (device scale applied by the provider).
    let data = &sized_for(data, &image);
    b.register_image(data);
    match image {
        ImagePlan::Tiles(tiles) => {
            for (dest, visible) in tiles {
                out.push(Pending {
                    owner: bx.node,
                    phase,
                    kind: ItemKind::BACKGROUND_IMAGE,
                    ink: visible,
                    scope,
                    op: DrawOp::DrawImage {
                        image: data.id,
                        dest,
                        visible,
                        sampling: layer.sampling,
                    },
                });
            }
        }
        ImagePlan::Pattern {
            tile,
            repeat_x,
            repeat_y,
            fill,
        } => {
            out.push(Pending {
                owner: bx.node,
                phase,
                kind: ItemKind::BACKGROUND_IMAGE,
                ink: fill,
                scope,
                op: DrawOp::DrawImagePattern {
                    image: data.id,
                    tile,
                    repeat_x,
                    repeat_y,
                    fill,
                    sampling: layer.sampling,
                },
            });
        }
    }
}

// ------------------------------------------------------------------ gradients

/// Resolve a gradient for a tile (CSS Images 3 sections 3.1 to 3.5). `None` when the geometry
/// degenerates (zero-size tile or ray).
pub fn resolve_gradient(spec: &GradientSpec, tile: &Rect) -> Option<Gradient> {
    if !(tile.width > 0.0 && tile.height > 0.0) {
        return None;
    }
    match spec {
        GradientSpec::Linear {
            direction,
            stops,
            repeating,
        } => {
            let (w, h) = (tile.width, tile.height);
            let (dx, dy) = match *direction {
                LinearDirection::Angle(deg) => {
                    let a = deg.to_radians();
                    (a.sin(), -a.cos())
                }
                LinearDirection::ToCorner { x, y } => {
                    let (vx, vy) = (x as f32 * h, y as f32 * w);
                    let n = (vx * vx + vy * vy).sqrt();
                    (vx / n, vy / n)
                }
            };
            let len = (w * dx).abs() + (h * dy).abs();
            if !(len > 0.0 && len.is_finite()) {
                return None;
            }
            let (cx, cy) = (tile.x + w / 2.0, tile.y + h / 2.0);
            let start = Point {
                x: cx - dx * len / 2.0,
                y: cy - dy * len / 2.0,
            };
            let (stops, first, last) = fix_stops(stops, len, *repeating)?;
            // Repeating gradients: the line spans one period [first, last].
            let (s, e) = if *repeating {
                (
                    Point {
                        x: start.x + dx * len * first,
                        y: start.y + dy * len * first,
                    },
                    Point {
                        x: start.x + dx * len * last,
                        y: start.y + dy * len * last,
                    },
                )
            } else {
                (
                    start,
                    Point {
                        x: start.x + dx * len,
                        y: start.y + dy * len,
                    },
                )
            };
            Some(Gradient {
                kind: GradientKind::Linear { start: s, end: e },
                stops,
                extend: if *repeating {
                    Extend::Repeat
                } else {
                    Extend::Pad
                },
            })
        }
        GradientSpec::Radial {
            circle,
            size,
            center,
            stops,
            repeating,
        } => {
            let (cx, cy) = (
                tile.x + center.0.resolve(tile.width),
                tile.y + center.1.resolve(tile.height),
            );
            // Distances from the centre to the four sides.
            let (l, r, t, b) = (
                cx - tile.x,
                tile.right() - cx,
                cy - tile.y,
                tile.bottom() - cy,
            );
            let (rx, ry) = match *size {
                RadialSize::Explicit(x, y) => (x, y),
                RadialSize::ExplicitEllipse(x, y) => {
                    (x.resolve(tile.width), y.resolve(tile.height))
                }
                RadialSize::ClosestSide | RadialSize::FarthestSide => {
                    let far = matches!(size, RadialSize::FarthestSide);
                    let pick = |a: f32, b: f32| {
                        if far {
                            a.abs().max(b.abs())
                        } else {
                            a.abs().min(b.abs())
                        }
                    };
                    if *circle {
                        let m = if far {
                            pick(l, r).max(pick(t, b))
                        } else {
                            pick(l, r).min(pick(t, b))
                        };
                        (m, m)
                    } else {
                        (pick(l, r), pick(t, b))
                    }
                }
                RadialSize::ClosestCorner | RadialSize::FarthestCorner => {
                    let far = matches!(size, RadialSize::FarthestCorner);
                    let pick = |a: f32, b: f32| {
                        if far {
                            a.abs().max(b.abs())
                        } else {
                            a.abs().min(b.abs())
                        }
                    };
                    let (sx, sy) = (pick(l, r), pick(t, b));
                    if *circle {
                        let d = (sx * sx + sy * sy).sqrt();
                        (d, d)
                    } else if sx > 0.0 && sy > 0.0 {
                        // Same aspect ratio as the matching side-sized ellipse, through the corner.
                        (sx * std::f32::consts::SQRT_2, sy * std::f32::consts::SQRT_2)
                    } else {
                        // A centre on a side: the corner distance degenerates to a thin ellipse.
                        (
                            sx.max(1e-3) * std::f32::consts::SQRT_2,
                            sy.max(1e-3) * std::f32::consts::SQRT_2,
                        )
                    }
                }
            };
            if !(rx > 0.0 && ry > 0.0 && rx.is_finite() && ry.is_finite()) {
                return None;
            }
            let (stops, first, last) = fix_stops(stops, rx, *repeating)?;
            let (rx0, ry0, extend) = if *repeating {
                (rx * last, ry * last, Extend::Repeat)
            } else {
                (rx, ry, Extend::Pad)
            };
            let inner = if *repeating { first / last } else { 0.0 };
            Some(Gradient {
                kind: GradientKind::Radial {
                    center: Point { x: cx, y: cy },
                    rx: rx0,
                    ry: ry0,
                    inner,
                },
                stops,
                extend,
            })
        }
        GradientSpec::Conic {
            from_deg,
            center,
            stops,
            repeating,
        } => {
            let (cx, cy) = (
                tile.x + center.0.resolve(tile.width),
                tile.y + center.1.resolve(tile.height),
            );
            // Stops are fractions of a turn; percentages resolve against 360 degrees, lengths are not allowed.
            let (stops, first, last) = fix_stops(stops, 1.0, *repeating)?;
            let start = from_deg.to_radians();
            let kind = GradientKind::Conic {
                center: Point { x: cx, y: cy },
                start_angle: start,
                first,
                last,
            };
            Some(Gradient {
                kind,
                stops,
                extend: if *repeating {
                    Extend::Repeat
                } else {
                    Extend::Pad
                },
            })
        }
    }
}

/// Colour-stop fix-up (CSS Images 3 section 3.5.3) for a gradient line of length `len` px:
/// positions as fractions of `len`; missing first / last default to 0 / 1; positions never
/// decrease; runs of missing positions are evenly spaced; hints are expanded into sampled
/// stops. For repeating gradients the stops are normalised to the period and `(first, last)`
/// (fractions of `len`) are returned; otherwise `(0, 1)`.
fn fix_stops(specs: &[StopSpec], len: f32, repeating: bool) -> Option<(Vec<ColorStop>, f32, f32)> {
    if specs.len() < 2 || len.is_nan() || len <= 0.0 {
        return None;
    }
    let n = specs.len();
    let mut pos: Vec<Option<f32>> = specs
        .iter()
        .map(|s| match s.position {
            None => None,
            Some(StopPosition::Fraction(f)) => Some(f),
            Some(StopPosition::Length(px)) => Some(px / len),
            Some(StopPosition::Mixed { frac, px }) => Some(frac + px / len),
        })
        .collect();
    if pos[0].is_none() {
        pos[0] = Some(0.0);
    }
    if pos[n - 1].is_none() {
        pos[n - 1] = Some(1.0);
    }
    // Monotonic: a position smaller than any before it is raised to the maximum so far.
    let mut max = f32::MIN;
    for p in pos.iter_mut().flatten() {
        if *p < max {
            *p = max;
        }
        max = *p;
    }
    // Evenly space runs of missing positions between their neighbours.
    let mut i = 0;
    while i < n {
        if pos[i].is_some() {
            i += 1;
            continue;
        }
        let a = i - 1;
        let mut b = i;
        while pos[b].is_none() {
            b += 1;
        }
        let (pa, pb) = (pos[a]?, pos[b]?);
        #[allow(clippy::needless_range_loop)]
        for k in i..b {
            pos[k] = Some(pa + (pb - pa) * (k - a) as f32 / (b - a) as f32);
        }
        i = b;
    }
    let pos: Vec<f32> = pos.into_iter().map(|p| p.unwrap_or(0.0)).collect();
    let (first, last) = (pos[0], pos[n - 1]);
    let mut out: Vec<ColorStop> = Vec::with_capacity(n);
    for i in 0..n {
        if let (Some(hint), true) = (specs[i].hint_before, i > 0) {
            let (pa, pb) = (pos[i - 1], pos[i]);
            // The hint as a fraction of the way from the previous stop to this one.
            let h = match hint {
                Hint::Segment(h) => Some(h),
                Hint::At(p) => {
                    let at = match p {
                        StopPosition::Fraction(f) => f,
                        StopPosition::Length(px) => px / len,
                        StopPosition::Mixed { frac, px } => frac + px / len,
                    };
                    (pb > pa).then(|| (at - pa) / (pb - pa))
                }
            };
            if let Some(h) = h.filter(|h| h.is_finite()) {
                if pb > pa && h > 0.0 && h < 1.0 {
                    // Colour at fraction t between the stops is mix(a, b, t^(ln 0.5 / ln h)).
                    let exp = (0.5f32).ln() / h.ln();
                    for k in 1..HINT_SAMPLES {
                        let t = k as f32 / HINT_SAMPLES as f32;
                        let m = t.powf(exp);
                        out.push(ColorStop {
                            offset: pa + (pb - pa) * t,
                            color: mix(&specs[i - 1].color, &specs[i].color, m),
                        });
                    }
                } else if pb > pa && h <= 0.0 {
                    // A hint at (or before) the previous stop: the colour switches there (CSS
                    // Images 3 section 3.5.3 clamps the hint into the segment).
                    out.push(ColorStop {
                        offset: pa,
                        color: specs[i].color,
                    });
                } else if pb > pa && h >= 1.0 {
                    out.push(ColorStop {
                        offset: pb,
                        color: specs[i - 1].color,
                    });
                }
            }
        }
        out.push(ColorStop {
            offset: pos[i],
            color: specs[i].color,
        });
    }
    if repeating {
        if last.is_nan() || last <= first {
            return None;
        }
        let span = last - first;
        for s in &mut out {
            s.offset = (s.offset - first) / span;
        }
        Some((out, first, last))
    } else {
        Some((out, 0.0, 1.0))
    }
}

/// Linear mix in premultiplied sRGB (the interpolation the lowering uses).
fn mix(a: &Color, b: &Color, t: f32) -> Color {
    let pre = |c: &Color| [c.r * c.a, c.g * c.a, c.b * c.a, c.a];
    let (pa, pb) = (pre(a), pre(b));
    let m: Vec<f32> = pa.iter().zip(&pb).map(|(x, y)| x + (y - x) * t).collect();
    let al = m[3];
    if al <= 0.0 {
        return Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
    }
    Color {
        r: m[0] / al,
        g: m[1] / al,
        b: m[2] / al,
        a: al,
    }
}

/// A shared gradient paint.
pub fn paint_of(g: Gradient) -> crate::scene::display_list::Paint {
    crate::scene::display_list::Paint::Gradient(Arc::new(g))
}
