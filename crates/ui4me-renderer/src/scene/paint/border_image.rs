//! owner: 010-paint-render — lane L-PNT-r8 (WPT-D7)
//!
//! `border-image` (CSS Backgrounds 3 section 6): the border image area (border box grown by
//! `border-image-outset`), the used `border-image-width`s, the nine slices of the image and their
//! placement (corners scaled, edges stretched, repeated, rounded or spaced, the middle part only
//! with `fill`). [`plan`] is a pure function; the traversal in `appendix_e` turns each
//! [`Placement`] into a `DrawImage` (raster images) or a clipped, scaled gradient fill.
//!
//! The image's own coordinate space is its natural size (CSS px) for a raster or vector image;
//! a gradient has no size, so it is as large as the border image area (CSS Images 3 section 5.3,
//! default sizing with the border image area as the default object size).

use crate::scene::appendix_e::{Builder, PaintBox, PaintPhase, Pending};
use crate::scene::display_list::{ClipShape, DrawOp, ItemKind, Rect, ScopeId, ScopeKind};
use crate::scene::inputs::{BgImage, BiOutset, BiRepeat, BiSlice, BiWidth, BorderImageSpec};
use crate::scene::paint::background::{MAX_TILES, paint_of, resolve_gradient};
use ui4me_engine_types::geometry::transform::Affine2D;

/// One piece of the image placed on one region of the border image area.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The slice, in the image's coordinate space.
    pub src: Rect,
    /// Where the whole slice lands (it may extend beyond `visible`: tiles at the ends of a
    /// repeated edge are cut).
    pub dest: Rect,
    /// The part of `dest` that is painted: `dest` clipped to the region.
    pub visible: Rect,
}

/// The result of planning one border image.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The image's coordinate-space size (`W`, `H`).
    pub image: (f32, f32),
    /// The border image area.
    pub area: Rect,
    /// Placements in paint order: corners, edges, then the middle part.
    pub placements: Vec<Placement>,
}

/// Why nothing is painted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// Degenerate geometry (empty area, empty image): nothing to paint, nothing to report.
    Empty,
    /// More than [`MAX_TILES`] tiles.
    TooManyTiles,
}

/// The border image area: the border box grown by the outsets (top, right, bottom, left).
pub fn image_area(border_box: &Rect, widths: [f32; 4], outset: &[BiOutset; 4]) -> Rect {
    let o = |i: usize| {
        match outset[i] {
            BiOutset::Number(n) => n * widths[i],
            BiOutset::Px(p) => p,
        }
        .max(0.0)
    };
    Rect {
        x: border_box.x - o(3),
        y: border_box.y - o(0),
        width: border_box.width + o(1) + o(3),
        height: border_box.height + o(0) + o(2),
    }
}

/// The positions along one axis of the tiles of a region `[start, start + len)`:
/// `(tile start, tile length)`, tiles at the ends may extend beyond the region.
fn axis_tiles(mode: BiRepeat, start: f32, len: f32, tile: f32) -> Vec<(f32, f32)> {
    if !(len > 0.0 && tile > 0.0 && tile.is_finite() && len.is_finite()) {
        return Vec::new();
    }
    match mode {
        BiRepeat::Stretch => vec![(start, len)],
        BiRepeat::Repeat => {
            // A tile is centred on the region; the tiles at both ends are cut.
            let first = start + len / 2.0 - tile / 2.0;
            let before = ((first - start) / tile).ceil().max(0.0) as i64;
            let after = ((start + len - (first + tile)) / tile).ceil().max(0.0) as i64;
            if (before + after + 1) as usize > MAX_TILES {
                return vec![(f32::NAN, f32::NAN)];
            }
            (-before..=after)
                .map(|i| (first + i as f32 * tile, tile))
                .collect()
        }
        BiRepeat::Round => {
            let n = (len / tile).round().max(1.0);
            if n as usize > MAX_TILES {
                return vec![(f32::NAN, f32::NAN)];
            }
            let t = len / n;
            (0..n as usize).map(|i| (start + i as f32 * t, t)).collect()
        }
        BiRepeat::Space => {
            let n = (len / tile).floor();
            if n < 1.0 {
                return Vec::new();
            }
            if n as usize > MAX_TILES {
                return vec![(f32::NAN, f32::NAN)];
            }
            // The extra space is shared by the n + 1 gaps between and around the tiles (a single
            // tile is centred): WPT border-image-repeat-space-10.
            let gap = (len - n * tile) / (n + 1.0);
            (0..n as usize)
                .map(|i| (start + gap + i as f32 * (tile + gap), tile))
                .collect()
        }
    }
}

fn isect(a: &Rect, b: &Rect) -> Option<Rect> {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// Plan the painting of `spec` for a box with border box `border_box` and used border widths
/// `widths` (top, right, bottom, left).
pub fn plan(
    spec: &BorderImageSpec,
    border_box: &Rect,
    widths: [f32; 4],
) -> Result<Plan, PlanError> {
    let area = image_area(border_box, widths, &spec.outset);
    if !(area.width > 0.0 && area.height > 0.0 && area.width.is_finite() && area.height.is_finite())
    {
        return Err(PlanError::Empty);
    }
    // The image's coordinate space.
    let (iw, ih, intrinsic) = match &spec.source {
        BgImage::Gradient(_) => (area.width, area.height, false),
        // A vector image without a natural size takes the border image area as its default object
        // size (CSS Images 3 section 5.3), keeping its ratio when it has one.
        BgImage::Raster(d) if d.vector.is_some() => {
            let (nw, nh) = (
                d.natural.0.filter(|v| *v > 0.0),
                d.natural.1.filter(|v| *v > 0.0),
            );
            let ratio = d
                .ratio
                .filter(|r| r.is_finite() && *r > 0.0)
                .or_else(|| nw.zip(nh).map(|(w, h)| w / h));
            let (w, h) = match (nw, nh, ratio) {
                (Some(w), Some(h), _) => (w, h),
                (Some(w), None, Some(r)) => (w, w / r),
                (None, Some(h), Some(r)) => (h * r, h),
                (None, None, Some(r)) => {
                    if area.width / area.height > r {
                        (area.height * r, area.height)
                    } else {
                        (area.width, area.width / r)
                    }
                }
                (Some(w), None, None) => (w, area.height),
                (None, Some(h), None) => (area.width, h),
                (None, None, None) => (area.width, area.height),
            };
            (w, h, true)
        }
        BgImage::Raster(d) => {
            let w = d
                .natural
                .0
                .filter(|v| *v > 0.0)
                .unwrap_or(d.pixels.width as f32);
            let h = d
                .natural
                .1
                .filter(|v| *v > 0.0)
                .unwrap_or(d.pixels.height as f32);
            (w, h, true)
        }
    };
    if !(iw > 0.0 && ih > 0.0 && iw.is_finite() && ih.is_finite()) {
        return Err(PlanError::Empty);
    }
    // Slices (section 6.1): numbers are image pixels, percentages fractions of the image size;
    // values beyond the image size mean the whole size.
    let dim = [ih, iw, ih, iw];
    let slice: [f32; 4] = std::array::from_fn(|i| {
        let v = match spec.slice[i] {
            BiSlice::Number(n) => n,
            BiSlice::Fraction(f) => f * dim[i],
        };
        v.clamp(0.0, dim[i])
    });
    // Used widths (section 6.2): numbers multiply the border width, `auto` is the slice's own size
    // when the image has one.
    let basis = [area.height, area.width, area.height, area.width];
    let mut w: [f32; 4] = std::array::from_fn(|i| match spec.width[i] {
        BiWidth::Number(n) => n * widths[i],
        BiWidth::Len(l) => l.resolve(basis[i]),
        BiWidth::Auto if intrinsic => slice[i],
        BiWidth::Auto => widths[i],
    });
    for v in &mut w {
        *v = v.max(0.0);
    }
    // Too wide: all four are reduced by the same factor (section 6.2).
    let (hsum, vsum) = (w[1] + w[3], w[0] + w[2]);
    let mut f = 1.0f32;
    if hsum > area.width {
        f = f.min(area.width / hsum);
    }
    if vsum > area.height {
        f = f.min(area.height / vsum);
    }
    if f < 1.0 {
        for v in &mut w {
            *v *= f;
        }
    }
    let (st, sr, sb, sl) = (slice[0], slice[1], slice[2], slice[3]);
    let (wt, wr, wb, wl) = (w[0], w[1], w[2], w[3]);
    // Target grid and source grid.
    let xs = [area.x, area.x + wl, area.right() - wr, area.right()];
    let ys = [area.y, area.y + wt, area.bottom() - wb, area.bottom()];
    let mid_sw = (iw - sl - sr).max(0.0);
    let mid_sh = (ih - st - sb).max(0.0);
    let (mid_w, mid_h) = ((xs[2] - xs[1]).max(0.0), (ys[2] - ys[1]).max(0.0));
    let mut out: Vec<Placement> = Vec::new();
    let mut count = 0usize;
    let push = |out: &mut Vec<Placement>, src: Rect, dest: Rect, region: Rect| {
        if src.width > 0.0
            && src.height > 0.0
            && dest.width > 0.0
            && dest.height > 0.0
            && let Some(visible) = isect(&dest, &region)
        {
            out.push(Placement { src, dest, visible });
        }
    };
    let r = |x: f32, y: f32, w: f32, h: f32| Rect {
        x,
        y,
        width: w.max(0.0),
        height: h.max(0.0),
    };
    // Corners: scaled to the widths of their sides.
    let corners = [
        (r(0.0, 0.0, sl, st), r(xs[0], ys[0], wl, wt)),
        (r(iw - sr, 0.0, sr, st), r(xs[2], ys[0], wr, wt)),
        (r(0.0, ih - sb, sl, sb), r(xs[0], ys[2], wl, wb)),
        (r(iw - sr, ih - sb, sr, sb), r(xs[2], ys[2], wr, wb)),
    ];
    for (src, dest) in corners {
        push(&mut out, src, dest, dest);
    }
    // Horizontal edges (top, bottom): scaled to the edge's width, tiled along x.
    let mut edges_x =
        |out: &mut Vec<Placement>, src: Rect, region: Rect, scale: f32| -> Result<(), PlanError> {
            if src.height <= 0.0 || region.width <= 0.0 || region.height <= 0.0 || src.width <= 0.0
            {
                return Ok(());
            }
            let tile_w = src.width * scale;
            let tiles = axis_tiles(spec.repeat.0, region.x, region.width, tile_w);
            if tiles.first().is_some_and(|t| t.0.is_nan()) {
                return Err(PlanError::TooManyTiles);
            }
            for (tx, tl) in tiles {
                count += 1;
                push(out, src, r(tx, region.y, tl, region.height), region);
            }
            Ok(())
        };
    let scale_t = if st > 0.0 { wt / st } else { 0.0 };
    let scale_b = if sb > 0.0 { wb / sb } else { 0.0 };
    edges_x(
        &mut out,
        r(sl, 0.0, mid_sw, st),
        r(xs[1], ys[0], mid_w, wt),
        scale_t,
    )?;
    edges_x(
        &mut out,
        r(sl, ih - sb, mid_sw, sb),
        r(xs[1], ys[2], mid_w, wb),
        scale_b,
    )?;
    // Vertical edges (left, right): tiled along y.
    let edges_y =
        |out: &mut Vec<Placement>, src: Rect, region: Rect, scale: f32| -> Result<(), PlanError> {
            if src.width <= 0.0 || src.height <= 0.0 || region.width <= 0.0 || region.height <= 0.0
            {
                return Ok(());
            }
            let tile_h = src.height * scale;
            let tiles = axis_tiles(spec.repeat.1, region.y, region.height, tile_h);
            if tiles.first().is_some_and(|t| t.0.is_nan()) {
                return Err(PlanError::TooManyTiles);
            }
            for (ty, tl) in tiles {
                push(out, src, r(region.x, ty, region.width, tl), region);
            }
            Ok(())
        };
    let scale_l = if sl > 0.0 { wl / sl } else { 0.0 };
    let scale_r = if sr > 0.0 { wr / sr } else { 0.0 };
    edges_y(
        &mut out,
        r(0.0, st, sl, mid_sh),
        r(xs[0], ys[1], wl, mid_h),
        scale_l,
    )?;
    edges_y(
        &mut out,
        r(iw - sr, st, sr, mid_sh),
        r(xs[2], ys[1], wr, mid_h),
        scale_r,
    )?;
    // The middle part (only with `fill`): the same scale factors as the edges along a tiled axis.
    if spec.fill && mid_sw > 0.0 && mid_sh > 0.0 && mid_w > 0.0 && mid_h > 0.0 {
        let src = r(sl, st, mid_sw, mid_sh);
        let region = r(xs[1], ys[1], mid_w, mid_h);
        let kx = if scale_t > 0.0 { scale_t } else { scale_b };
        let ky = if scale_l > 0.0 { scale_l } else { scale_r };
        let tile_w = if spec.repeat.0 == BiRepeat::Stretch || kx <= 0.0 {
            mid_w
        } else {
            mid_sw * kx
        };
        let tile_h = if spec.repeat.1 == BiRepeat::Stretch || ky <= 0.0 {
            mid_h
        } else {
            mid_sh * ky
        };
        let tx = axis_tiles(spec.repeat.0, region.x, region.width, tile_w);
        let ty = axis_tiles(spec.repeat.1, region.y, region.height, tile_h);
        if tx.first().is_some_and(|t| t.0.is_nan())
            || ty.first().is_some_and(|t| t.0.is_nan())
            || tx.len() * ty.len() > MAX_TILES
        {
            return Err(PlanError::TooManyTiles);
        }
        for &(y, h) in &ty {
            for &(x, wd) in &tx {
                push(&mut out, src, r(x, y, wd, h), region);
            }
        }
    }
    if out.len() > MAX_TILES || count > MAX_TILES {
        return Err(PlanError::TooManyTiles);
    }
    if out.is_empty() {
        return Err(PlanError::Empty);
    }
    Ok(Plan {
        image: (iw, ih),
        area,
        placements: out,
    })
}

/// Append the items of `bx`'s border image to `out`. Returns `false` when nothing was painted
/// because the geometry is empty or too large (the caller then paints the `border-style`
/// borders, which is also what happens for an invalid image); a too-large plan is reported.
pub(crate) fn push_border_image(
    b: &mut Builder,
    bx: &PaintBox,
    spec: &BorderImageSpec,
    widths: [f32; 4],
    out: &mut Vec<Pending>,
    phase: PaintPhase,
    scope: Option<ScopeId>,
) -> bool {
    let bb = Rect {
        x: bx.border_box.origin.x.to_f64() as f32,
        y: bx.border_box.origin.y.to_f64() as f32,
        width: bx.border_box.size.width.to_f64() as f32,
        height: bx.border_box.size.height.to_f64() as f32,
    };
    let plan = match plan(spec, &bb, widths) {
        Ok(p) => p,
        Err(PlanError::Empty) => return false,
        Err(PlanError::TooManyTiles) => {
            b.report("css.property.border-image-repeat", bx.node);
            return true;
        }
    };
    let (iw, ih) = plan.image;
    match &spec.source {
        BgImage::Raster(data) => {
            b.register_image(data);
            // Each slice is drawn from its own cropped pixmap, so the sampler clamps at the slice's
            // edges and the neighbouring slice's colour never bleeds in (WPT-D48; the reference
            // browser samples within the slice).
            let mut crops: Vec<((u64, [u32; 4]), u64)> = Vec::new();
            for p in &plan.placements {
                let (kx, ky) = (p.dest.width / p.src.width, p.dest.height / p.src.height);
                // A vector image is rasterised at the scale its whole image lands at: one uniform
                // scale (the larger of the two, so the stretching of a slice only shrinks), capped
                // so that the raster stays reasonable; the stretching itself is the DrawImage's.
                let s = kx
                    .max(ky)
                    .min((16.0 * 1024.0 * 1024.0 / (iw * ih)).sqrt() * 0.99);
                let data = data.for_size((iw * s, ih * s));
                let (pw, ph) = (data.pixels.width, data.pixels.height);
                let (fx, fy) = (pw as f32 / iw, ph as f32 / ih);
                // Only pixels wholly inside the slice: a rasterised vector image has anti-aliased
                // pixels on a fractional slice edge that mix in the neighbour's colour.
                let inner = |a: f32, b: f32, max: u32| {
                    let (lo, hi) = (
                        (a - 1e-3).ceil().max(0.0) as u32,
                        (b + 1e-3).floor().max(0.0) as u32,
                    );
                    if hi > lo {
                        (lo.min(max - 1), hi.min(max))
                    } else {
                        let mid = (((a + b) / 2.0).floor().max(0.0) as u32).min(max - 1);
                        (mid, mid + 1)
                    }
                };
                let (x0, x1) = inner(p.src.x * fx, (p.src.x + p.src.width) * fx, pw.max(1));
                let (y0, y1) = inner(p.src.y * fy, (p.src.y + p.src.height) * fy, ph.max(1));
                let key = (data.id, [x0, y0, x1, y1]);
                let id = match crops.iter().find(|(k, _)| *k == key) {
                    Some((_, id)) => *id,
                    None => {
                        let id = crop_image(b, &data, [x0, y0, x1, y1]);
                        crops.push((key, id));
                        id
                    }
                };
                out.push(Pending {
                    owner: bx.node,
                    phase,
                    kind: ItemKind::BORDER,
                    ink: p.visible,
                    scope,
                    op: DrawOp::DrawImage {
                        image: id,
                        dest: p.dest,
                        visible: p.visible,
                        sampling: spec.sampling,
                    },
                });
            }
        }
        BgImage::Gradient(g) => {
            let Some(gradient) = resolve_gradient(
                g,
                &Rect {
                    x: 0.0,
                    y: 0.0,
                    width: iw,
                    height: ih,
                },
            ) else {
                return false;
            };
            let paint = paint_of(gradient);
            for p in &plan.placements {
                let (kx, ky) = (p.dest.width / p.src.width, p.dest.height / p.src.height);
                // The gradient lives in the image's space; a scaled scope places it, a clip scope
                // cuts it to the visible part of the tile.
                let clip = b.scope(scope, ScopeKind::Clip(ClipShape::Rect(p.visible)));
                let t = Affine2D {
                    a: kx as f64,
                    b: 0.0,
                    c: 0.0,
                    d: ky as f64,
                    e: (p.dest.x - p.src.x * kx) as f64,
                    f: (p.dest.y - p.src.y * ky) as f64,
                };
                let spatial = b.scope(Some(clip), ScopeKind::Spatial(t));
                out.push(Pending {
                    owner: bx.node,
                    phase,
                    kind: ItemKind::BORDER,
                    ink: p.visible,
                    scope: Some(spatial),
                    op: DrawOp::FillRect {
                        rect: p.src,
                        paint: paint.clone(),
                    },
                });
            }
        }
    }
    true
}

/// Register the pixels `[x0, y0, x1, y1)` of `image` as an image of their own and return its id
/// (derived from the source id and the rectangle, so equal crops share one entry).
fn crop_image(
    b: &mut Builder,
    image: &crate::scene::inputs::ImageData,
    [x0, y0, x1, y1]: [u32; 4],
) -> u64 {
    let (w, h, stride) = (
        (x1 - x0) as usize,
        (y1 - y0) as usize,
        image.pixels.width as usize * 4,
    );
    let mut data = Vec::with_capacity(w * h * 4);
    for y in y0..y1 {
        let row = y as usize * stride + x0 as usize * 4;
        data.extend_from_slice(&image.pixels.data[row..row + w * 4]);
    }
    let mut id = 0xcbf2_9ce4_8422_2325_u64 ^ image.id;
    for v in [x0, y0, x1, y1] {
        id = (id ^ u64::from(v)).wrapping_mul(0x0100_0000_01b3);
    }
    b.images.insert(
        id,
        crate::scene::display_list::ImagePixels {
            width: w as u32,
            height: h as u32,
            data: data.into(),
        },
    );
    id
}
