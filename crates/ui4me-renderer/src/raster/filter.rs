//! owner: 010-paint-render — lane L-PAINT3-r14 (CSS `filter`, Filter Effects 1 section 5)
//!
//! Pixel passes for the `filter` functions, applied to the premultiplied RGBA8 pixels of a filter
//! scope's isolated layer (the lowering draws the scope's content into its own layer, runs
//! [`apply`] and composites the result into the parent). vello_cpu 0.3.0 has a Gaussian blur and a
//! drop shadow only, experimental and for one primitive at a time, and no colour matrix: all ten
//! functions are implemented here instead, in the same code path, so a chain behaves the same
//! whichever functions it holds.
//!
//! * **Colour functions** (`brightness`, `contrast`, `grayscale`, `hue-rotate`, `invert`, `opacity`,
//!   `saturate`, `sepia`) act on un-premultiplied sRGB (the default `color-interpolation-filters` of
//!   the CSS functions is sRGB) with the matrices and transfer functions of Filter Effects 1
//!   section 5; the working precision is f32 and the result is rounded once at the end of the chain.
//! * **`blur(σ)`** is a Gaussian with transparent edges (edge mode `none`). For σ below 2 device px the
//!   kernel is exact (radius 3σ); from 2 px on it is the three-box-blur approximation of the SVG
//!   `feGaussianBlur` definition (Filter Effects 1 section 9.5), which is also what the reference
//!   browsers do. σ is measured in device px along the device axes: the linear part of the scope's
//!   transform scales it per axis (a rotation or skew keeps the axis-aligned device blur; recorded
//!   limit).
//! * **`drop-shadow()`** is the input's alpha, offset (the offset vector goes through the linear part
//!   of the transform and rounds to whole device px: recorded limit), blurred and tinted, under the input.
//!
//! The region worked on is the bounding box of the non-transparent pixels grown by the chain's reach;
//! pixels outside it are untouched (and transparent), so the cost follows the painted area, not the
//! surface.

use crate::scene::display_list::FilterFn;

/// The linear part `[a, b, c, d]` of a device transform (`x' = a x + c y`, `y' = b x + d y`).
pub(crate) type Linear = [f64; 4];

/// Largest margin (device px) a filter layer is extended by on each side.
pub(crate) const MAX_MARGIN: u32 = 1024;

fn sigma_dev(s: f32, lin: Linear) -> (f32, f32) {
    let sx = (lin[0] * lin[0] + lin[1] * lin[1]).sqrt() as f32;
    let sy = (lin[2] * lin[2] + lin[3] * lin[3]).sqrt() as f32;
    (s.max(0.0) * sx, s.max(0.0) * sy)
}

fn offset_dev(dx: f32, dy: f32, lin: Linear) -> (i32, i32) {
    let x = lin[0] * f64::from(dx) + lin[2] * f64::from(dy);
    let y = lin[1] * f64::from(dx) + lin[3] * f64::from(dy);
    (
        x.round().clamp(-1.0e6, 1.0e6) as i32,
        y.round().clamp(-1.0e6, 1.0e6) as i32,
    )
}

/// Pixels a blur of standard deviation `s` (device px) reaches on one side.
fn blur_reach(s: f32) -> i64 {
    if s <= 0.0 {
        0
    } else {
        (3.0 * f64::from(s)).ceil() as i64 + 2
    }
}

/// How far the chain paints beyond its input, in device px: `[left, top, right, bottom]`.
pub(crate) fn device_reach(functions: &[FilterFn], lin: Linear) -> [i64; 4] {
    let mut acc = [0i64; 4];
    for f in functions {
        let r = match *f {
            FilterFn::Blur(s) => {
                let (sx, sy) = sigma_dev(s, lin);
                [
                    blur_reach(sx),
                    blur_reach(sy),
                    blur_reach(sx),
                    blur_reach(sy),
                ]
            }
            FilterFn::DropShadow { dx, dy, blur, .. } => {
                let (sx, sy) = sigma_dev(blur, lin);
                let (ox, oy) = offset_dev(dx, dy, lin);
                let (bx, by) = (blur_reach(sx), blur_reach(sy));
                [
                    (bx - i64::from(ox)).max(0),
                    (by - i64::from(oy)).max(0),
                    (bx + i64::from(ox)).max(0),
                    (by + i64::from(oy)).max(0),
                ]
            }
            _ => [0; 4],
        };
        for i in 0..4 {
            acc[i] += r[i];
        }
    }
    acc
}

/// The margin (device px, the same on every side) a layer needs so that the chain's output is not cut.
pub(crate) fn margin(functions: &[FilterFn], lin: Linear) -> u32 {
    let r = device_reach(functions, lin);
    r.iter()
        .copied()
        .max()
        .unwrap_or(0)
        .clamp(0, i64::from(MAX_MARGIN)) as u32
}

/// A colour function as a 5x4 affine map on un-premultiplied RGBA (rows R, G, B, A; last column the offset).
#[derive(Clone, Copy)]
struct ColorOp {
    m: [[f32; 5]; 4],
}

const IDENTITY: [[f32; 5]; 4] = [
    [1.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0, 0.0],
];

/// The matrix of a colour function (Filter Effects 1 section 5), `None` for the others.
fn color_op(f: &FilterFn) -> Option<ColorOp> {
    let mut m = IDENTITY;
    let rgb = |m: &mut [[f32; 5]; 4], k: [[f32; 3]; 3]| {
        for (row, k) in m.iter_mut().zip(k) {
            row[..3].copy_from_slice(&k);
        }
    };
    match *f {
        FilterFn::Grayscale(a) => {
            let s = 1.0 - a.clamp(0.0, 1.0);
            rgb(
                &mut m,
                [
                    [
                        0.2126 + 0.7874 * s,
                        0.7152 - 0.7152 * s,
                        0.0722 - 0.0722 * s,
                    ],
                    [
                        0.2126 - 0.2126 * s,
                        0.7152 + 0.2848 * s,
                        0.0722 - 0.0722 * s,
                    ],
                    [
                        0.2126 - 0.2126 * s,
                        0.7152 - 0.7152 * s,
                        0.0722 + 0.9278 * s,
                    ],
                ],
            );
        }
        FilterFn::Sepia(a) => {
            let s = 1.0 - a.clamp(0.0, 1.0);
            rgb(
                &mut m,
                [
                    [0.393 + 0.607 * s, 0.769 - 0.769 * s, 0.189 - 0.189 * s],
                    [0.349 - 0.349 * s, 0.686 + 0.314 * s, 0.168 - 0.168 * s],
                    [0.272 - 0.272 * s, 0.534 - 0.534 * s, 0.131 + 0.869 * s],
                ],
            );
        }
        FilterFn::Saturate(s) => {
            let s = s.max(0.0);
            rgb(
                &mut m,
                [
                    [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
                    [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
                    [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
                ],
            );
        }
        FilterFn::HueRotate(deg) => {
            let (sin, cos) = deg.to_radians().sin_cos();
            rgb(
                &mut m,
                [
                    [
                        0.213 + cos * 0.787 - sin * 0.213,
                        0.715 - cos * 0.715 - sin * 0.715,
                        0.072 - cos * 0.072 + sin * 0.928,
                    ],
                    [
                        0.213 - cos * 0.213 + sin * 0.143,
                        0.715 + cos * 0.285 + sin * 0.140,
                        0.072 - cos * 0.072 - sin * 0.283,
                    ],
                    [
                        0.213 - cos * 0.213 - sin * 0.787,
                        0.715 - cos * 0.715 + sin * 0.715,
                        0.072 + cos * 0.928 + sin * 0.072,
                    ],
                ],
            );
        }
        FilterFn::Brightness(b) => {
            let b = b.max(0.0);
            for (i, row) in m.iter_mut().enumerate().take(3) {
                row[i] = b;
            }
        }
        FilterFn::Contrast(c) => {
            let c = c.max(0.0);
            for (i, row) in m.iter_mut().enumerate().take(3) {
                row[i] = c;
                row[4] = 0.5 - 0.5 * c;
            }
        }
        FilterFn::Invert(a) => {
            let a = a.clamp(0.0, 1.0);
            for (i, row) in m.iter_mut().enumerate().take(3) {
                row[i] = 1.0 - 2.0 * a;
                row[4] = a;
            }
        }
        FilterFn::Opacity(a) => m[3][3] = a.clamp(0.0, 1.0),
        FilterFn::Blur(_) | FilterFn::DropShadow { .. } => return None,
    }
    Some(ColorOp { m })
}

/// The pixels of a region: premultiplied RGBA, f32, row-major.
struct Region {
    w: usize,
    h: usize,
    px: Vec<f32>,
}

fn color_pixel(p: &mut [f32], op: &ColorOp) {
    let m = &op.m;
    let a = p[3];
    if a <= 0.0 {
        return;
    }
    let (r, g, b) = (p[0] / a, p[1] / a, p[2] / a);
    let out = |row: &[f32; 5]| {
        (row[0] * r + row[1] * g + row[2] * b + row[3] * a + row[4]).clamp(0.0, 1.0)
    };
    let na = out(&m[3]);
    p[0] = out(&m[0]) * na;
    p[1] = out(&m[1]) * na;
    p[2] = out(&m[2]) * na;
    p[3] = na;
}

/// Apply a chain of color-only filters without allocating an image-sized f32 region.
pub(crate) fn apply_colors(buf: &mut [u8], functions: &[FilterFn]) -> bool {
    let mut ops = Vec::new();
    if ops.try_reserve_exact(functions.len()).is_err() {
        return false;
    }
    ops.extend(functions.iter().filter_map(color_op));
    for pixel in buf.as_chunks_mut::<4>().0 {
        let mut p = [0.0f32; 4];
        for (d, s) in p.iter_mut().zip(pixel.iter()) {
            *d = f32::from(*s) / 255.0;
        }
        for op in &ops {
            color_pixel(&mut p, op);
        }
        for (d, s) in pixel.iter_mut().zip(p) {
            *d = (s.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        let alpha = pixel[3];
        for c in &mut pixel[..3] {
            *c = (*c).min(alpha);
        }
    }
    true
}

impl Region {
    fn color(&mut self, op: &ColorOp) {
        for p in self.px.as_chunks_mut::<4>().0 {
            color_pixel(p, op);
        }
    }

    /// Blur every channel of `px` (`w` x `h`, 4 channels) by `(sx, sy)` device px.
    fn blur_pixels(px: &mut [f32], w: usize, h: usize, sx: f32, sy: f32) -> bool {
        if sx > 0.0 {
            let Some(mut scratch) = zeroed_f32(w * 4) else {
                return false;
            };
            for row in px.chunks_exact_mut(w * 4) {
                blur_line(row, &mut scratch, sx);
            }
        }
        if sy > 0.0 {
            let Some(mut col) = zeroed_f32(h * 4) else {
                return false;
            };
            let Some(mut scratch) = zeroed_f32(h * 4) else {
                return false;
            };
            for x in 0..w {
                for y in 0..h {
                    col[y * 4..y * 4 + 4]
                        .copy_from_slice(&px[(y * w + x) * 4..(y * w + x) * 4 + 4]);
                }
                blur_line(&mut col, &mut scratch, sy);
                for y in 0..h {
                    px[(y * w + x) * 4..(y * w + x) * 4 + 4]
                        .copy_from_slice(&col[y * 4..y * 4 + 4]);
                }
            }
        }
        true
    }

    fn blur(&mut self, sx: f32, sy: f32) -> bool {
        Region::blur_pixels(&mut self.px, self.w, self.h, sx, sy)
    }

    fn drop_shadow(&mut self, (ox, oy): (i32, i32), (sx, sy): (f32, f32), color: [f32; 4]) -> bool {
        let (w, h) = (self.w as i64, self.h as i64);
        // The shadow: the input's alpha shifted, tinted with the colour (premultiplied).
        let Some(mut shadow) = zeroed_f32(self.px.len()) else {
            return false;
        };
        for y in 0..h {
            let ty = y + i64::from(oy);
            if !(0..h).contains(&ty) {
                continue;
            }
            for x in 0..w {
                let tx = x + i64::from(ox);
                if !(0..w).contains(&tx) {
                    continue;
                }
                let a = self.px[((y * w + x) * 4 + 3) as usize] * color[3];
                let t = ((ty * w + tx) * 4) as usize;
                shadow[t] = color[0] * a;
                shadow[t + 1] = color[1] * a;
                shadow[t + 2] = color[2] * a;
                shadow[t + 3] = a;
            }
        }
        if !Region::blur_pixels(&mut shadow, self.w, self.h, sx, sy) {
            return false;
        }
        // The input over the shadow.
        for (p, s) in self
            .px
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(shadow.as_chunks::<4>().0)
        {
            let k = 1.0 - p[3];
            for c in 0..4 {
                p[c] += s[c] * k;
            }
        }
        true
    }
}

/// Blur one line of `n` premultiplied pixels in place (`line` has `n * 4` floats); `scratch` is a buffer
/// of the same length.
fn blur_line(line: &mut [f32], scratch: &mut [f32], sigma: f32) {
    let n = line.len() / 4;
    if sigma < 2.0 {
        let radius = (3.0 * sigma).ceil().max(1.0) as i64;
        let kernel: Vec<f32> = {
            let k: Vec<f32> = (-radius..=radius)
                .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
                .collect();
            let sum: f32 = k.iter().sum();
            k.iter().map(|v| v / sum).collect()
        };
        for i in 0..n as i64 {
            let mut acc = [0.0f32; 4];
            for (j, k) in kernel.iter().enumerate() {
                let s = i + j as i64 - radius;
                if (0..n as i64).contains(&s) {
                    for c in 0..4 {
                        acc[c] += line[s as usize * 4 + c] * k;
                    }
                }
            }
            scratch[i as usize * 4..i as usize * 4 + 4].copy_from_slice(&acc);
        }
        line.copy_from_slice(scratch);
        return;
    }
    // Three box blurs (SVG feGaussianBlur): d = floor(sigma * 3 * sqrt(2 pi) / 4 + 0.5).
    let d = (f64::from(sigma) * 3.0 * (2.0 * std::f64::consts::PI).sqrt() / 4.0 + 0.5)
        .floor()
        .max(1.0) as usize;
    let passes: [(usize, usize); 3] = if d % 2 == 1 {
        [(d, (d - 1) / 2); 3]
    } else {
        [(d, d / 2), (d, d / 2 - 1), (d + 1, d / 2)]
    };
    for (size, left) in passes {
        box_pass(line, scratch, n, size, left);
        line.copy_from_slice(scratch);
    }
}

/// One box blur pass: `out[i]` is the mean of `size` pixels from `i - left` (transparent outside).
fn box_pass(src: &[f32], out: &mut [f32], n: usize, size: usize, left: usize) {
    let inv = 1.0 / size as f32;
    let mut acc = [0.0f32; 4];
    // The window of output 0 is [-left, size - 1 - left].
    for s in 0..(size - left).min(n) {
        for c in 0..4 {
            acc[c] += src[s * 4 + c];
        }
    }
    for i in 0..n {
        for c in 0..4 {
            out[i * 4 + c] = acc[c] * inv;
        }
        // Slide to output i + 1: add pixel i + 1 + (size - 1 - left), drop pixel i - left.
        let add = i + size - left;
        if add < n {
            for c in 0..4 {
                acc[c] += src[add * 4 + c];
            }
        }
        if i >= left {
            let drop = i - left;
            for c in 0..4 {
                acc[c] -= src[drop * 4 + c];
            }
        }
    }
}

/// Apply `functions` to the premultiplied RGBA8 layer `buf` (`w` x `h`) in place. `lin` is the linear
/// part of the transform from the scope's space to the layer's device pixels.
fn zeroed_f32(len: usize) -> Option<Vec<f32>> {
    let mut values = Vec::new();
    values.try_reserve_exact(len).ok()?;
    values.resize(len, 0.0);
    Some(values)
}

#[derive(Clone, Copy)]
pub(crate) enum TiledFailure {
    Budget,
    Memory,
}

/// Apply a filter chain in row strips with the chain's full source halo.
/// `available` excludes the already allocated input buffer.
pub(crate) fn apply_tiled(
    buf: &mut [u8],
    w: usize,
    h: usize,
    functions: &[FilterFn],
    lin: Linear,
    available: u64,
) -> Result<(), TiledFailure> {
    let output_bytes = (w as u64).saturating_mul(h as u64).saturating_mul(4);
    let pad = device_reach(functions, lin)
        .into_iter()
        .max()
        .unwrap_or(0)
        .max(0) as usize;
    if pad > MAX_MARGIN as usize {
        return Err(TiledFailure::Budget);
    }
    let work_pixels = available.saturating_sub(output_bytes) / 72;
    let max_rows = (work_pixels / w as u64).min(8_500_000 / w as u64) as usize;
    if max_rows <= 2 * pad {
        return Err(TiledFailure::Budget);
    }
    let rows = (max_rows - 2 * pad).min(h);
    let mut output = Vec::new();
    output
        .try_reserve_exact(buf.len())
        .map_err(|_| TiledFailure::Memory)?;
    output.resize(buf.len(), 0);
    let mut top = 0;
    while top < h {
        let count = rows.min(h - top);
        let ph = count + 2 * pad;
        let mut work = Vec::new();
        work.try_reserve_exact(w * ph * 4)
            .map_err(|_| TiledFailure::Memory)?;
        work.resize(w * ph * 4, 0);
        for y in 0..ph {
            let sy = top as i64 + y as i64 - pad as i64;
            if (0..h as i64).contains(&sy) {
                let src = sy as usize * w * 4;
                work[y * w * 4..(y + 1) * w * 4].copy_from_slice(&buf[src..src + w * 4]);
            }
        }
        if !apply(&mut work, w, ph, functions, lin) {
            return Err(TiledFailure::Memory);
        }
        for y in 0..count {
            let src = (y + pad) * w * 4;
            let dst = (top + y) * w * 4;
            output[dst..dst + w * 4].copy_from_slice(&work[src..src + w * 4]);
        }
        top += count;
    }
    buf.copy_from_slice(&output);
    Ok(())
}

pub(crate) fn apply(
    buf: &mut [u8],
    w: usize,
    h: usize,
    functions: &[FilterFn],
    lin: Linear,
) -> bool {
    debug_assert_eq!(buf.len(), w * h * 4);
    // The bounding box of the painted pixels.
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h {
        let row = &buf[y * w * 4..(y + 1) * w * 4];
        if let Some(first) = row.as_chunks::<4>().0.iter().position(|p| p[3] != 0) {
            let last = row
                .as_chunks::<4>()
                .0
                .iter()
                .rposition(|p| p[3] != 0)
                .unwrap_or(first);
            x0 = x0.min(first);
            x1 = x1.max(last + 1);
            y0 = y0.min(y);
            y1 = y1.max(y + 1);
        }
    }
    if x0 >= x1 {
        return true;
    }
    let reach = device_reach(functions, lin);
    let grow = |v: usize, by: i64, lo: bool, limit: usize| -> usize {
        if lo {
            (v as i64 - by).max(0) as usize
        } else {
            (v as i64 + by).min(limit as i64) as usize
        }
    };
    let (rx0, ry0, rx1, ry1) = (
        grow(x0, reach[0], true, w),
        grow(y0, reach[1], true, h),
        grow(x1, reach[2], false, w),
        grow(y1, reach[3], false, h),
    );
    let region_w = rx1 - rx0;
    let region_h = ry1 - ry0;
    let Some(px) = zeroed_f32(region_w * region_h * 4) else {
        return false;
    };
    let mut region = Region {
        w: region_w,
        h: region_h,
        px,
    };
    for y in 0..region_h {
        let src = &buf[((ry0 + y) * w + rx0) * 4..((ry0 + y) * w + rx0 + region_w) * 4];
        for (d, s) in region.px[y * region_w * 4..(y + 1) * region_w * 4]
            .iter_mut()
            .zip(src)
        {
            *d = f32::from(*s) / 255.0;
        }
    }
    for f in functions {
        if let Some(op) = color_op(f) {
            region.color(&op);
            continue;
        }
        match *f {
            FilterFn::Blur(s) => {
                let (sx, sy) = sigma_dev(s, lin);
                if !region.blur(sx, sy) {
                    return false;
                }
            }
            FilterFn::DropShadow {
                dx,
                dy,
                blur,
                color,
            } if !region.drop_shadow(
                offset_dev(dx, dy, lin),
                sigma_dev(blur, lin),
                [color.r, color.g, color.b, color.a],
            ) =>
            {
                return false;
            }
            _ => {}
        }
    }
    for y in 0..region_h {
        let dst = &mut buf[((ry0 + y) * w + rx0) * 4..((ry0 + y) * w + rx0 + region_w) * 4];
        for (d, s) in dst
            .iter_mut()
            .zip(&region.px[y * region_w * 4..(y + 1) * region_w * 4])
        {
            *d = (s.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        // Premultiplied output: a colour channel never exceeds alpha (rounding).
        for p in dst.as_chunks_mut::<4>().0 {
            let a = p[3];
            for c in &mut p[..3] {
                *c = (*c).min(a);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::display_list::Color;

    fn px(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend(f(x, y));
            }
        }
        v
    }

    const ID: Linear = [1.0, 0.0, 0.0, 1.0];

    #[test]
    fn tiled_filter_chain_matches_full_region_across_seams() {
        let (w, h) = (40usize, 60usize);
        let mut source = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let alpha = if (15..45).contains(&y) { 128 } else { 0 };
                source.extend_from_slice(&[
                    ((x * 3) as u8).min(alpha),
                    ((y * 2) as u8).min(alpha),
                    ((x + y) as u8).min(alpha),
                    alpha,
                ]);
            }
        }
        let functions = [
            FilterFn::Blur(2.0),
            FilterFn::DropShadow {
                dx: 1.0,
                dy: -1.0,
                blur: 1.0,
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.7,
                },
            },
        ];
        let mut full = source.clone();
        let mut tiled = source;
        assert!(apply(&mut full, w, h, &functions, ID));
        assert!(apply_tiled(&mut tiled, w, h, &functions, ID, 200_000).is_ok());
        let max = tiled
            .iter()
            .zip(&full)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(max <= 1, "maximum channel difference {max}");
    }

    #[test]
    fn streaming_color_chain_matches_full_region() {
        let functions = [
            FilterFn::Invert(0.4),
            FilterFn::Opacity(0.7),
            FilterFn::Contrast(1.2),
            FilterFn::HueRotate(70.0),
        ];
        let mut a = vec![
            0, 0, 0, 0, 20, 40, 60, 128, 80, 160, 240, 255, 15, 15, 15, 32,
        ];
        let mut b = a.clone();
        assert!(apply(&mut a, 4, 1, &functions, ID));
        assert!(apply_colors(&mut b, &functions));
        assert_eq!(a, b);
    }

    #[test]
    fn grayscale_full_gives_luminance_and_keeps_alpha() {
        let mut b = px(4, 4, |_, _| [255, 0, 0, 255]);
        apply(&mut b, 4, 4, &[FilterFn::Grayscale(1.0)], ID);
        // 0.2126 * 255 = 54.2
        assert_eq!(&b[..4], &[54, 54, 54, 255]);
    }

    #[test]
    fn invert_and_brightness_and_contrast() {
        let mut b = px(2, 2, |_, _| [51, 102, 153, 255]);
        apply(&mut b, 2, 2, &[FilterFn::Invert(1.0)], ID);
        assert_eq!(&b[..4], &[204, 153, 102, 255]);
        let mut b = px(2, 2, |_, _| [51, 102, 153, 255]);
        apply(&mut b, 2, 2, &[FilterFn::Brightness(2.0)], ID);
        assert_eq!(&b[..4], &[102, 204, 255, 255]);
        let mut b = px(2, 2, |_, _| [51, 102, 153, 255]);
        apply(&mut b, 2, 2, &[FilterFn::Contrast(0.0)], ID);
        // contrast(0) is mid grey 0.5.
        assert_eq!(&b[..4], &[128, 128, 128, 255]);
    }

    #[test]
    fn opacity_scales_premultiplied_pixels() {
        let mut b = px(2, 2, |_, _| [100, 100, 100, 200]);
        apply(&mut b, 2, 2, &[FilterFn::Opacity(0.5)], ID);
        assert_eq!(&b[..4], &[50, 50, 50, 100]);
    }

    #[test]
    fn transparent_pixels_stay_transparent() {
        let mut b = vec![0u8; 4 * 4 * 4];
        apply(
            &mut b,
            4,
            4,
            &[FilterFn::Invert(1.0), FilterFn::Brightness(3.0)],
            ID,
        );
        assert!(b.iter().all(|v| *v == 0));
    }

    #[test]
    fn hue_rotate_zero_and_360_are_identity_and_120_cycles_primaries() {
        let src = px(2, 2, |_, _| [200, 100, 50, 255]);
        let mut b = src.clone();
        apply(&mut b, 2, 2, &[FilterFn::HueRotate(360.0)], ID);
        for (a, c) in b.iter().zip(&src) {
            assert!((i32::from(*a) - i32::from(*c)).abs() <= 1);
        }
        let mut b = px(2, 2, |_, _| [255, 0, 0, 255]);
        apply(&mut b, 2, 2, &[FilterFn::HueRotate(120.0)], ID);
        // The matrix rotates red towards green (0.213 - 0.213cos - 0.787 sin ...).
        assert!(b[1] > b[0] && b[1] > b[2], "{:?}", &b[..4]);
    }

    #[test]
    fn blur_conserves_energy_and_spreads_symmetrically() {
        // A 1-pixel dot in the middle of 41x41, blurred by sigma 3 (box path) and 1 (exact path).
        for sigma in [1.0f32, 3.0] {
            let mut b = px(41, 41, |x, y| {
                if (x, y) == (20, 20) {
                    [255, 255, 255, 255]
                } else {
                    [0; 4]
                }
            });
            apply(&mut b, 41, 41, &[FilterFn::Blur(sigma)], ID);
            let total: u32 = b.as_chunks::<4>().0.iter().map(|p| u32::from(p[3])).sum();
            assert!(
                (total as f32 - 255.0).abs() < 255.0 * 0.12,
                "sigma {sigma}: total alpha {total}"
            );
            let at = |x: usize, y: usize| b[(y * 41 + x) * 4 + 3];
            assert!(at(20, 20) < 255 && at(20, 20) > 0);
            assert_eq!(at(19, 20), at(21, 20), "sigma {sigma} symmetric");
            assert_eq!(at(20, 19), at(20, 21), "sigma {sigma} symmetric");
        }
    }

    #[test]
    fn blur_peak_matches_the_gaussian() {
        // A 2-D Gaussian of sigma 2 has peak 1 / (2 pi sigma^2) = 0.0398 of the energy: 10.1 of 255.
        let mut b = px(41, 41, |x, y| {
            if (x, y) == (20, 20) {
                [255, 255, 255, 255]
            } else {
                [0; 4]
            }
        });
        apply(&mut b, 41, 41, &[FilterFn::Blur(2.0)], ID);
        let peak = b[(20 * 41 + 20) * 4 + 3];
        assert!((i32::from(peak) - 10).abs() <= 2, "peak {peak}");
    }

    #[test]
    fn drop_shadow_goes_under_the_input_at_the_offset() {
        let mut b = px(20, 20, |x, y| {
            if (8..12).contains(&x) && (8..12).contains(&y) {
                [255, 0, 0, 255]
            } else {
                [0; 4]
            }
        });
        let black = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        apply(
            &mut b,
            20,
            20,
            &[FilterFn::DropShadow {
                dx: 4.0,
                dy: 3.0,
                blur: 0.0,
                color: black,
            }],
            ID,
        );
        let at = |x: usize, y: usize| {
            <[u8; 4]>::try_from(&b[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4]).unwrap()
        };
        assert_eq!(at(9, 9), [255, 0, 0, 255]);
        assert_eq!(at(14, 12), [0, 0, 0, 255]); // shadow: x 12..16, y 11..15
        assert_eq!(at(12, 11), [0, 0, 0, 255]);
        assert_eq!(at(16, 12), [0; 4]);
        assert_eq!(at(12, 15), [0; 4]);
    }

    #[test]
    fn reach_adds_up_along_a_chain() {
        let f = [
            FilterFn::Blur(2.0),
            FilterFn::DropShadow {
                dx: 5.0,
                dy: 0.0,
                blur: 1.0,
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            },
        ];
        let r = device_reach(&f, ID);
        assert_eq!(r[0], blur_reach(2.0) + blur_reach(1.0) - 5);
        assert_eq!(r[2], blur_reach(2.0) + blur_reach(1.0) + 5);
        assert_eq!(margin(&[FilterFn::Grayscale(1.0)], ID), 0);
    }

    #[test]
    fn device_scale_scales_sigma_and_offset() {
        let two = [2.0, 0.0, 0.0, 2.0];
        assert_eq!(sigma_dev(3.0, two), (6.0, 6.0));
        assert_eq!(offset_dev(1.5, -2.0, two), (3, -4));
    }
}
