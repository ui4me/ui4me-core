//! owner: 007-resources (F0907) — per-shape hit geometry of an inline SVG.
//!
//! The renderer serialises an inline `<svg>` subtree into a standalone SVG document and rasterises
//! it through the ordinary vector path. To report *which shape* a pointer is over, this module
//! parses that same document with usvg and keeps, for every visible path, the flattened outline in
//! the SVG's own coordinate space (viewBox, `transform`s and `use` instances already applied by
//! usvg), its fill rule and stroke half-width. A hit is the topmost shape whose fill area (or
//! stroke band) contains the point. Everything is bounded: segment count and shape count are
//! capped and a document over the cap produces no table (the `<svg>` element itself is hit).

use super::prescan;

/// Most flattened points kept for one document.
const MAX_POINTS: usize = 200_000;
/// Most shapes kept for one document.
const MAX_SHAPES: usize = 5_000;

/// Which painted areas of a shape react to the pointer (the SVG `pointer-events` subset).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerMode {
    /// `none`: the shape is transparent to the pointer.
    None,
    /// `visiblePainted` and friends: the fill area when filled, the stroke band when stroked.
    Painted,
    /// `fill` / `visibleFill`: the fill area, painted or not.
    Fill,
    /// `stroke` / `visibleStroke`: the stroke band, painted or not.
    Stroke,
    /// `all` / `visible`: fill area and stroke band, painted or not.
    All,
}

#[derive(Debug)]
struct Shape {
    id: String,
    /// Flattened subpaths in tree space, each with whether it was closed.
    subpaths: Vec<(Vec<[f32; 2]>, bool)>,
    even_odd: bool,
    fills: bool,
    /// Half the stroke width in tree space, when stroked.
    stroke_half: Option<f32>,
}

/// The hit table of one inline SVG document.
#[derive(Debug)]
pub struct HitShapes {
    size: (f32, f32),
    /// Content is stretched to the box (no intrinsic ratio / `preserveAspectRatio="none"`).
    fill_box: bool,
    shapes: Vec<Shape>,
}

fn flatten(
    path: &usvg::tiny_skia_path::Path,
    ts: usvg::Transform,
    points: &mut usize,
) -> Option<Vec<(Vec<[f32; 2]>, bool)>> {
    use usvg::tiny_skia_path::PathSegment as S;
    let map = |p: usvg::tiny_skia_path::Point| {
        [
            ts.sx * p.x + ts.kx * p.y + ts.tx,
            ts.ky * p.x + ts.sy * p.y + ts.ty,
        ]
    };
    let mut out: Vec<(Vec<[f32; 2]>, bool)> = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();
    let mut last = [0.0f32; 2];
    let flush = |cur: &mut Vec<[f32; 2]>, out: &mut Vec<(Vec<[f32; 2]>, bool)>, closed: bool| {
        if cur.len() > 1 {
            out.push((std::mem::take(cur), closed));
        } else {
            cur.clear();
        }
    };
    const STEPS: usize = 12;
    for seg in path.segments() {
        match seg {
            S::MoveTo(p) => {
                flush(&mut cur, &mut out, false);
                last = map(p);
                cur.push(last);
                *points += 1;
            }
            S::LineTo(p) => {
                last = map(p);
                cur.push(last);
                *points += 1;
            }
            S::QuadTo(c, p) => {
                let (c, p) = (map(c), map(p));
                for i in 1..=STEPS {
                    let t = i as f32 / STEPS as f32;
                    let u = 1.0 - t;
                    cur.push([
                        u * u * last[0] + 2.0 * u * t * c[0] + t * t * p[0],
                        u * u * last[1] + 2.0 * u * t * c[1] + t * t * p[1],
                    ]);
                }
                *points += STEPS;
                last = p;
            }
            S::CubicTo(c1, c2, p) => {
                let (c1, c2, p) = (map(c1), map(c2), map(p));
                for i in 1..=STEPS {
                    let t = i as f32 / STEPS as f32;
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    cur.push([
                        w[0] * last[0] + w[1] * c1[0] + w[2] * c2[0] + w[3] * p[0],
                        w[0] * last[1] + w[1] * c1[1] + w[2] * c2[1] + w[3] * p[1],
                    ]);
                }
                *points += STEPS;
                last = p;
            }
            S::Close => {
                let start = cur.first().copied();
                flush(&mut cur, &mut out, true);
                if let Some(s) = start {
                    // A subpath may continue from its start point after `Z`.
                    last = s;
                    cur.push(s);
                }
            }
        }
        if *points > MAX_POINTS {
            return None;
        }
    }
    flush(&mut cur, &mut out, false);
    Some(out)
}

fn walk(group: &usvg::Group, inherited_id: &str, out: &mut Vec<Shape>, points: &mut usize) -> bool {
    for node in group.children() {
        if out.len() > MAX_SHAPES {
            return false;
        }
        match node {
            usvg::Node::Group(g) => {
                let id = if g.id().is_empty() {
                    inherited_id
                } else {
                    g.id()
                };
                if !walk(g, id, out, points) {
                    return false;
                }
            }
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let id = if p.id().is_empty() {
                    inherited_id
                } else {
                    p.id()
                };
                if id.is_empty() {
                    continue;
                }
                let ts = p.abs_transform();
                let Some(subpaths) = flatten(p.data(), ts, points) else {
                    return false;
                };
                let scale = (ts.sx * ts.sy - ts.kx * ts.ky).abs().sqrt();
                out.push(Shape {
                    id: id.to_owned(),
                    subpaths,
                    even_odd: p
                        .fill()
                        .is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd),
                    fills: p.fill().is_some(),
                    stroke_half: p.stroke().map(|s| s.width().get() * scale / 2.0),
                });
            }
            usvg::Node::Image(_) | usvg::Node::Text(_) => {}
        }
    }
    true
}

impl HitShapes {
    /// Parse `markup` (an inline SVG serialised by the renderer). `None` when it cannot be parsed
    /// or exceeds the caps: the caller then only has the `<svg>` element to hit.
    pub fn build(markup: &str) -> Option<HitShapes> {
        let pre = prescan::scan(markup.as_bytes(), 20_000).ok()?;
        let opt = usvg::Options {
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_data: Box::new(|_, _, _| None),
                resolve_string: Box::new(|_, _| None),
            },
            ..usvg::Options::default()
        };
        let tree = usvg::Tree::from_str(markup, &opt).ok()?;
        let mut shapes = Vec::new();
        let mut points = 0;
        if !walk(tree.root(), "", &mut shapes, &mut points) {
            return None;
        }
        let size = tree.size();
        Some(HitShapes {
            size: (size.width(), size.height()),
            fill_box: pre.natural.ratio.is_none() || pre.stretch,
            shapes,
        })
    }

    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    /// The id of the topmost shape under `(x, y)`, CSS px relative to the SVG's `box_w` x `box_h`
    /// content box (drawn like the raster: fitted `xMidYMid meet`, or stretched when it has no
    /// intrinsic ratio). `mode_of` gives the `pointer-events` of a shape id.
    pub fn hit(
        &self,
        x: f32,
        y: f32,
        box_w: f32,
        box_h: f32,
        mode_of: &dyn Fn(&str) -> PointerMode,
    ) -> Option<&str> {
        if !(box_w > 0.0 && box_h > 0.0 && self.size.0 > 0.0 && self.size.1 > 0.0) {
            return None;
        }
        let (sx, sy) = (box_w / self.size.0, box_h / self.size.1);
        let (tx, ty) = if self.fill_box {
            (x / sx, y / sy)
        } else {
            let s = sx.min(sy);
            let ox = (box_w - self.size.0 * s) / 2.0;
            let oy = (box_h - self.size.1 * s) / 2.0;
            ((x - ox) / s, (y - oy) / s)
        };
        let p = [tx, ty];
        for shape in self.shapes.iter().rev() {
            let mode = mode_of(&shape.id);
            let (fill, stroke) = match mode {
                PointerMode::None => continue,
                PointerMode::Painted => (shape.fills, shape.stroke_half.is_some()),
                PointerMode::Fill => (true, false),
                PointerMode::Stroke => (false, true),
                PointerMode::All => (true, true),
            };
            if fill && shape.fill_contains(p) {
                return Some(&shape.id);
            }
            if stroke {
                let half = shape.stroke_half.unwrap_or(0.0).max(0.0);
                if shape.stroke_contains(p, half) {
                    return Some(&shape.id);
                }
            }
        }
        None
    }
}

impl Shape {
    fn fill_contains(&self, p: [f32; 2]) -> bool {
        let mut winding = 0i32;
        let mut crossings = 0u32;
        for (pts, _) in &self.subpaths {
            let n = pts.len();
            for i in 0..n {
                let a = pts[i];
                let b = pts[(i + 1) % n];
                if (a[1] <= p[1]) != (b[1] <= p[1]) {
                    let t = (p[1] - a[1]) / (b[1] - a[1]);
                    if a[0] + t * (b[0] - a[0]) > p[0] {
                        crossings += 1;
                        winding += if b[1] > a[1] { 1 } else { -1 };
                    }
                }
            }
        }
        if self.even_odd {
            crossings % 2 == 1
        } else {
            winding != 0
        }
    }

    fn stroke_contains(&self, p: [f32; 2], half: f32) -> bool {
        let r2 = half * half;
        for (pts, closed) in &self.subpaths {
            let n = pts.len();
            let segs = if *closed { n } else { n - 1 };
            for i in 0..segs {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len2 = dx * dx + dy * dy;
                let t = if len2 > 0.0 {
                    (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (cx, cy) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
                if cx * cx + cy * cy <= r2 {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50">
        <rect id="a" x="0" y="0" width="50" height="50" fill="red"/>
        <circle id="b" cx="75" cy="25" r="20" fill="none" stroke="blue" stroke-width="4"/>
        <rect id="c" x="40" y="10" width="30" height="30" fill="green"/></svg>"##;

    #[test]
    fn topmost_fill_and_stroke_band() {
        let h = HitShapes::build(SVG).unwrap();
        let p = |_: &str| PointerMode::Painted;
        assert_eq!(h.hit(10.0, 10.0, 100.0, 50.0, &p), Some("a"));
        // c is above a where they overlap.
        assert_eq!(h.hit(45.0, 25.0, 100.0, 50.0, &p), Some("c"));
        // b: unfilled ring, only the band reacts (hole at the centre is c's / nothing).
        assert_eq!(h.hit(95.0, 25.0, 100.0, 50.0, &p), Some("b"));
        assert_eq!(h.hit(85.0, 25.0, 100.0, 50.0, &p), None);
        assert_eq!(
            h.hit(85.0, 25.0, 100.0, 50.0, &|_: &str| PointerMode::All),
            Some("b")
        );
        // scaled box
        assert_eq!(h.hit(20.0, 20.0, 200.0, 100.0, &p), Some("a"));
        // pointer-events none lets the one below through
        let none_c = |id: &str| {
            if id == "c" {
                PointerMode::None
            } else {
                PointerMode::Painted
            }
        };
        assert_eq!(h.hit(45.0, 25.0, 100.0, 50.0, &none_c), Some("a"));
    }
}
