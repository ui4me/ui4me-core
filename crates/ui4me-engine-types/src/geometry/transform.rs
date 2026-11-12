//! owner: 010-paint-render — created by 010 T008 inside 001's crate (R-18, R-28; co-sign D-075)
//!
//! Pure transform maths (spatial-tree.md §1). f64 throughout; no allocation-visible state; no
//! panics. Non-finite input returns `Err(Code::GeometryNonFinite)` (`invert` returns `None`).
//!
//! Conventions: `Transform3D` stores the 16 values in CSS `matrix3d()` order (column-major,
//! element `(row, col)` at index `col * 4 + row`); points are column vectors, so
//! `compose(parent, child) = parent * child` applies `child` first. `Affine2D { a, b, c, d, e, f }`
//! is CSS `matrix(a, b, c, d, e, f)`: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.

use crate::codes_generated::Code;

pub type GeoResult<T> = Result<T, Code>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointF {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl RectF {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> RectF {
        RectF {
            x,
            y,
            width,
            height,
        }
    }
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
    pub fn is_finite(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
    }
}

/// Four corners in order: top-left, top-right, bottom-right, bottom-left (of the source rect).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad(pub [PointF; 4]);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2D {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine2D {
    pub const IDENTITY: Affine2D = Affine2D {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translate(x: f64, y: f64) -> Affine2D {
        Affine2D {
            e: x,
            f: y,
            ..Affine2D::IDENTITY
        }
    }

    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }

    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// `self * other`: applies `other` first.
    pub fn then_after(&self, other: &Affine2D) -> Affine2D {
        Affine2D {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    pub fn map_point(&self, p: PointF) -> PointF {
        PointF {
            x: self.a * p.x + self.c * p.y + self.e,
            y: self.b * p.x + self.d * p.y + self.f,
        }
    }

    pub fn to_3d(&self) -> Transform3D {
        let mut m = Transform3D::IDENTITY;
        m.0[0] = self.a;
        m.0[1] = self.b;
        m.0[4] = self.c;
        m.0[5] = self.d;
        m.0[12] = self.e;
        m.0[13] = self.f;
        m
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform3D(pub [f64; 16]);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flattened {
    pub affine: Affine2D,
    /// True when perspective terms (the bottom row other than `(0, 0, 0, 1)`) are non-zero.
    pub projective: bool,
}

impl Transform3D {
    pub const IDENTITY: Transform3D = Transform3D([
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);

    pub fn at(&self, row: usize, col: usize) -> f64 {
        self.0[col * 4 + row]
    }

    fn set(&mut self, row: usize, col: usize, v: f64) {
        self.0[col * 4 + row] = v;
    }

    pub fn is_finite(&self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }

    pub fn translation(x: f64, y: f64, z: f64) -> Transform3D {
        let mut m = Transform3D::IDENTITY;
        m.set(0, 3, x);
        m.set(1, 3, y);
        m.set(2, 3, z);
        m
    }

    pub fn scaling(x: f64, y: f64, z: f64) -> Transform3D {
        let mut m = Transform3D::IDENTITY;
        m.set(0, 0, x);
        m.set(1, 1, y);
        m.set(2, 2, z);
        m
    }

    /// rotate3d(x, y, z, angle): `angle` in radians. A zero-length axis gives the identity.
    pub fn rotation(x: f64, y: f64, z: f64, angle: f64) -> Transform3D {
        let len = (x * x + y * y + z * z).sqrt();
        if len == 0.0 || !len.is_finite() {
            return Transform3D::IDENTITY;
        }
        let (x, y, z) = (x / len, y / len, z / len);
        let (s, c) = angle.sin_cos();
        let t = 1.0 - c;
        let mut m = Transform3D::IDENTITY;
        m.set(0, 0, c + x * x * t);
        m.set(0, 1, x * y * t - z * s);
        m.set(0, 2, x * z * t + y * s);
        m.set(1, 0, y * x * t + z * s);
        m.set(1, 1, c + y * y * t);
        m.set(1, 2, y * z * t - x * s);
        m.set(2, 0, z * x * t - y * s);
        m.set(2, 1, z * y * t + x * s);
        m.set(2, 2, c + z * z * t);
        m
    }

    pub fn skew(ax: f64, ay: f64) -> Transform3D {
        let mut m = Transform3D::IDENTITY;
        m.set(0, 1, ax.tan());
        m.set(1, 0, ay.tan());
        m
    }

    /// perspective(d): `m34 = -1/d`. The caller guarantees `d > 0` and finite.
    pub fn perspective(d: f64) -> Transform3D {
        let mut m = Transform3D::IDENTITY;
        m.set(3, 2, -1.0 / d);
        m
    }

    /// `self * rhs` without the finiteness check.
    fn mul(&self, rhs: &Transform3D) -> Transform3D {
        let mut out = [0.0; 16];
        for r in 0..4 {
            for c in 0..4 {
                let mut s = 0.0;
                for k in 0..4 {
                    s += self.at(r, k) * rhs.at(k, c);
                }
                out[c * 4 + r] = s;
            }
        }
        Transform3D(out)
    }

    /// General 4x4 inverse (Gauss-Jordan, partial pivoting). `None` when singular or non-finite.
    pub fn invert(&self) -> Option<Transform3D> {
        if !self.is_finite() {
            return None;
        }
        let mut a = [[0.0f64; 8]; 4];
        for (r, row) in a.iter_mut().enumerate() {
            for (c, value) in row[..4].iter_mut().enumerate() {
                *value = self.at(r, c);
            }
            row[4 + r] = 1.0;
        }
        for col in 0..4 {
            let piv = (col..4).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
            if a[piv][col].abs() < 1e-12 {
                return None;
            }
            a.swap(col, piv);
            let p = a[col][col];
            for v in a[col].iter_mut() {
                *v /= p;
            }
            let pivot_row = a[col];
            for (r, row) in a.iter_mut().enumerate() {
                if r != col {
                    let f = row[col];
                    if f != 0.0 {
                        for (value, pivot) in row.iter_mut().zip(pivot_row) {
                            *value -= f * pivot;
                        }
                    }
                }
            }
        }
        let mut out = [0.0; 16];
        for r in 0..4 {
            for c in 0..4 {
                out[c * 4 + r] = a[r][4 + c];
            }
        }
        let m = Transform3D(out);
        m.is_finite().then_some(m)
    }
}

/// One entry of a computed `transform` list, lengths already resolved to px by the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransformOp {
    Matrix(Affine2D),
    Matrix3d([f64; 16]),
    Translate {
        x: f64,
        y: f64,
        z: f64,
    },
    Scale {
        x: f64,
        y: f64,
        z: f64,
    },
    /// Axis and angle (radians).
    Rotate {
        x: f64,
        y: f64,
        z: f64,
        angle: f64,
    },
    /// Radians.
    Skew {
        ax: f64,
        ay: f64,
    },
    /// `d` in px; `d <= 0` is treated as no perspective (the caller clamps per CSS).
    Perspective(f64),
}

/// The individual transform properties `translate`, `rotate` and `scale`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IndividualTransforms {
    pub translate: Option<[f64; 3]>,
    /// Axis (x, y, z) and angle in radians.
    pub rotate: Option<([f64; 3], f64)>,
    pub scale: Option<[f64; 3]>,
}

fn op_matrix(op: &TransformOp) -> Transform3D {
    match *op {
        TransformOp::Matrix(a) => a.to_3d(),
        TransformOp::Matrix3d(v) => Transform3D(v),
        TransformOp::Translate { x, y, z } => Transform3D::translation(x, y, z),
        TransformOp::Scale { x, y, z } => Transform3D::scaling(x, y, z),
        TransformOp::Rotate { x, y, z, angle } => Transform3D::rotation(x, y, z, angle),
        TransformOp::Skew { ax, ay } => Transform3D::skew(ax, ay),
        TransformOp::Perspective(d) => {
            if d > 0.0 {
                Transform3D::perspective(d)
            } else {
                Transform3D::IDENTITY
            }
        }
    }
}

/// CSS Transforms 2 "current transformation matrix":
/// `translate(origin) · translate · rotate · scale · transform-list · translate(-origin)`.
/// `origin` is relative to `reference_box`'s top-left corner (percentages already resolved), and
/// the result maps points in the coordinate space that contains `reference_box`.
pub fn resolve_transform(
    list: &[TransformOp],
    individual: &IndividualTransforms,
    origin: [f64; 3],
    reference_box: RectF,
) -> GeoResult<Transform3D> {
    if !reference_box.is_finite()
        || !origin.iter().all(|v| v.is_finite())
        || !individual_finite(individual)
    {
        return Err(Code::GeometryNonFinite);
    }
    let ox = reference_box.x + origin[0];
    let oy = reference_box.y + origin[1];
    let oz = origin[2];
    let mut m = Transform3D::translation(ox, oy, oz);
    if let Some(t) = individual.translate {
        m = m.mul(&Transform3D::translation(t[0], t[1], t[2]));
    }
    if let Some((axis, angle)) = individual.rotate {
        m = m.mul(&Transform3D::rotation(axis[0], axis[1], axis[2], angle));
    }
    if let Some(s) = individual.scale {
        m = m.mul(&Transform3D::scaling(s[0], s[1], s[2]));
    }
    for op in list {
        if !op_finite(op) {
            return Err(Code::GeometryNonFinite);
        }
        m = m.mul(&op_matrix(op));
    }
    m = m.mul(&Transform3D::translation(-ox, -oy, -oz));
    finite(m)
}

fn individual_finite(i: &IndividualTransforms) -> bool {
    i.translate.is_none_or(|t| t.iter().all(|v| v.is_finite()))
        && i.rotate
            .is_none_or(|(a, g)| a.iter().all(|v| v.is_finite()) && g.is_finite())
        && i.scale.is_none_or(|t| t.iter().all(|v| v.is_finite()))
}

fn op_finite(op: &TransformOp) -> bool {
    match *op {
        TransformOp::Matrix(a) => a.is_finite(),
        TransformOp::Matrix3d(v) => v.iter().all(|x| x.is_finite()),
        TransformOp::Translate { x, y, z } | TransformOp::Scale { x, y, z } => {
            [x, y, z].iter().all(|v| v.is_finite())
        }
        TransformOp::Rotate { x, y, z, angle } => [x, y, z, angle].iter().all(|v| v.is_finite()),
        TransformOp::Skew { ax, ay } => {
            ax.is_finite() && ay.is_finite() && ax.cos() != 0.0 && ay.cos() != 0.0
        }
        TransformOp::Perspective(d) => d.is_finite(),
    }
}

fn finite(m: Transform3D) -> GeoResult<Transform3D> {
    if m.is_finite() {
        Ok(m)
    } else {
        Err(Code::GeometryNonFinite)
    }
}

/// CSS Transforms 2 "perspective" applied by a parent: `translate(o) · perspective(d) · translate(-o)`
/// with `o = reference_box.origin + perspective_origin`. `Ok(None)` when `perspective` is `none`
/// (`None` or not strictly positive).
pub fn perspective_matrix(
    perspective: Option<f64>,
    perspective_origin: [f64; 2],
    reference_box: RectF,
) -> GeoResult<Option<Transform3D>> {
    let Some(d) = perspective else {
        return Ok(None);
    };
    if !d.is_finite()
        || !reference_box.is_finite()
        || !perspective_origin.iter().all(|v| v.is_finite())
    {
        return Err(Code::GeometryNonFinite);
    }
    if d <= 0.0 {
        return Ok(None);
    }
    let ox = reference_box.x + perspective_origin[0];
    let oy = reference_box.y + perspective_origin[1];
    let m = Transform3D::translation(ox, oy, 0.0)
        .mul(&Transform3D::perspective(d))
        .mul(&Transform3D::translation(-ox, -oy, 0.0));
    finite(m).map(Some)
}

/// 4x4 composition `parent * child` (the child's transform applies first).
pub fn compose(parent: &Transform3D, child: &Transform3D) -> GeoResult<Transform3D> {
    if !parent.is_finite() || !child.is_finite() {
        return Err(Code::GeometryNonFinite);
    }
    finite(parent.mul(child))
}

/// Orthographic projection onto the parent plane (drops `z` and `w`).
pub fn flatten(m: &Transform3D) -> GeoResult<Flattened> {
    if !m.is_finite() {
        return Err(Code::GeometryNonFinite);
    }
    let affine = Affine2D {
        a: m.at(0, 0),
        b: m.at(1, 0),
        c: m.at(0, 1),
        d: m.at(1, 1),
        e: m.at(0, 3),
        f: m.at(1, 3),
    };
    let projective =
        m.at(3, 0) != 0.0 || m.at(3, 1) != 0.0 || m.at(3, 2) != 0.0 || m.at(3, 3) != 1.0;
    Ok(Flattened { affine, projective })
}

/// True when the transformed element shows its back face to the viewer (the case in which
/// `backface-visibility: hidden` hides it): the inverse's `m33` is negative. Singular matrices
/// are not rendered and report `false`.
pub fn is_backface_visible(m: &Transform3D) -> bool {
    m.invert().is_some_and(|i| i.at(2, 2) < 0.0)
}

/// Inverse of a 2D affine. `None` for non-invertible or non-finite input (FR-083).
pub fn invert(a: &Affine2D) -> Option<Affine2D> {
    if !a.is_finite() {
        return None;
    }
    let det = a.determinant();
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let inv = 1.0 / det;
    let r = Affine2D {
        a: a.d * inv,
        b: -a.b * inv,
        c: -a.c * inv,
        d: a.a * inv,
        e: (a.c * a.f - a.d * a.e) * inv,
        f: (a.b * a.e - a.a * a.f) * inv,
    };
    r.is_finite().then_some(r)
}

/// The image of `rect` under `a`.
pub fn map_rect(a: &Affine2D, rect: &RectF) -> GeoResult<Quad> {
    if !a.is_finite() || !rect.is_finite() {
        return Err(Code::GeometryNonFinite);
    }
    let pts = [
        PointF {
            x: rect.x,
            y: rect.y,
        },
        PointF {
            x: rect.right(),
            y: rect.y,
        },
        PointF {
            x: rect.right(),
            y: rect.bottom(),
        },
        PointF {
            x: rect.x,
            y: rect.bottom(),
        },
    ];
    let q = Quad(pts.map(|p| a.map_point(p)));
    if q.0.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
        Ok(q)
    } else {
        Err(Code::GeometryNonFinite)
    }
}

/// Axis-aligned bounds of a quad.
pub fn bounds(q: &Quad) -> RectF {
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in &q.0 {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    RectF {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}
