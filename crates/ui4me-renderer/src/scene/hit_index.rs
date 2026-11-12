//! owner: 010-paint-render — created by 001 slice (R-28)
//!
//! HitTestIndex (010 `contracts/hit-test.md`, AD-22), built in the same traversal as the display
//! list. Slice regions: border boxes and text line rectangles. The read mode, revision `current`
//! flag and outcome rows (005 recompute-plan §11) are applied by the pipeline around this query.
//! Implemented by 001 T047.

use crate::scene::appendix_e::SceneSnapshot;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::revision::RevisionTuple;

/// Viewport point in CSS px (equal to document space while unscrolled).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitOptions {
    /// `false` = topmost only.
    pub all: bool,
    pub include_scrollbars: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitPart {
    Content,
    Scrollbar { container: NodeId, axis: ScrollAxis },
    Text { text_node: NodeId },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitRecord {
    /// Originating element (for text: the parent element of the text node).
    pub element: NodeId,
    pub part: HitPart,
    /// Point relative to the fragment's border-box origin.
    pub local: (f64, f64),
    pub fragment_index: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HitResult {
    pub scene_generation: u64,
    pub revision: RevisionTuple,
    /// Topmost first.
    pub hits: Vec<HitRecord>,
}

/// One hit region: a border box or a text line rectangle, in document CSS px (half-open).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HitRegion {
    pub(crate) clips: std::sync::Arc<Vec<HitClip>>,
    pub(crate) inverse: ui4me_engine_types::geometry::transform::Affine2D,
    pub(crate) x0: f64,
    pub(crate) y0: f64,
    pub(crate) x1: f64,
    pub(crate) y1: f64,
    pub(crate) element: NodeId,
    pub(crate) part: HitPart,
    pub(crate) fragment_index: u32,
}

impl HitRegion {
    fn local_point(&self, x: f64, y: f64) -> ui4me_engine_types::geometry::transform::PointF {
        self.inverse
            .map_point(ui4me_engine_types::geometry::transform::PointF { x, y })
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        let p = self.local_point(x, y);
        p.x >= self.x0
            && p.x < self.x1
            && p.y >= self.y0
            && p.y < self.y1
            && self.clips.iter().all(|c| {
                let q = c
                    .inverse
                    .map_point(ui4me_engine_types::geometry::transform::PointF { x, y });
                shape_contains(&c.shape, q.x, q.y)
            })
    }
}

/// Hit regions in paint order (opaque; built by the scene in the display-list traversal).
#[derive(Clone, Debug, Default)]
pub struct HitTestIndex {
    regions: Vec<HitRegion>,
}

impl HitTestIndex {
    pub(crate) fn from_regions(regions: Vec<HitRegion>) -> HitTestIndex {
        HitTestIndex { regions }
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }
}

/// Geometry for a scrollbar pointer interaction, in the track's local coordinate system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarHit {
    pub container: NodeId,
    pub axis: ScrollAxis,
    pub local: f64,
    pub track_length: f64,
    pub thumb_start: f64,
    pub thumb_length: f64,
}

impl SceneSnapshot {
    /// Transform a viewport point into a scrollbar track's coordinate, even outside the track.
    pub fn scrollbar_local(
        &self,
        node: NodeId,
        axis: ScrollAxis,
        point: ViewportPoint,
    ) -> Option<f64> {
        let r = self.hit_index.regions.iter().rev().find(|r| matches!(r.part, HitPart::Scrollbar { container, axis: a } if container == node && a == axis))?;
        let p = r.local_point(point.x, point.y);
        Some(match axis {
            ScrollAxis::Horizontal => p.x - r.x0,
            ScrollAxis::Vertical => p.y - r.y0,
        })
    }

    /// Find a scrollbar and its thumb for pointer interaction.
    pub fn scrollbar_hit(&self, point: ViewportPoint) -> Option<ScrollbarHit> {
        let hit = self
            .hit_test(
                point,
                HitOptions {
                    all: false,
                    include_scrollbars: true,
                },
            )
            .hits
            .into_iter()
            .next()?;
        let HitPart::Scrollbar { container, axis } = hit.part else {
            return None;
        };
        let (tk, bk) = match axis {
            ScrollAxis::Horizontal => ("scrollbar-h-track", "scrollbar-h-thumb"),
            ScrollAxis::Vertical => ("scrollbar-v-track", "scrollbar-v-thumb"),
        };
        let rect = |kind| {
            self.display_list.items.iter().find_map(|i| {
                if i.owner.node != container || i.kind.0 != kind {
                    return None;
                }
                match &i.op {
                    crate::scene::display_list::DrawOp::FillRect { rect, .. } => Some(*rect),
                    crate::scene::display_list::DrawOp::FillRoundedRect { rect, .. } => {
                        Some(rect.rect)
                    }
                    _ => None,
                }
            })
        };
        let (track, thumb) = (rect(tk)?, rect(bk)?);
        let (local, track_length, thumb_start, thumb_length) = match axis {
            ScrollAxis::Horizontal => (hit.local.0, track.width, thumb.x - track.x, thumb.width),
            ScrollAxis::Vertical => (hit.local.1, track.height, thumb.y - track.y, thumb.height),
        };
        Some(ScrollbarHit {
            container,
            axis,
            local,
            track_length: f64::from(track_length),
            thumb_start: f64::from(thumb_start),
            thumb_length: f64::from(thumb_length),
        })
    }

    /// Reverse paint order over the hit regions (hit-test.md §2). Topmost only, or every
    /// element under the point once (its topmost record), topmost first. The slice has no
    /// scrollbar tracks participate only when `include_scrollbars` is set.
    pub fn hit_test(&self, point: ViewportPoint, opts: HitOptions) -> HitResult {
        let mut hits: Vec<HitRecord> = Vec::new();
        if point.x.is_finite() && point.y.is_finite() {
            for r in self.hit_index.regions.iter().rev() {
                if (!opts.include_scrollbars && matches!(r.part, HitPart::Scrollbar { .. }))
                    || !r.contains(point.x, point.y)
                    || hits.iter().any(|h| h.element == r.element)
                {
                    continue;
                }
                let p = r.local_point(point.x, point.y);
                hits.push(HitRecord {
                    element: r.element,
                    part: r.part,
                    local: (p.x - r.x0, p.y - r.y0),
                    fragment_index: r.fragment_index,
                });
                if !opts.all {
                    break;
                }
            }
        }
        HitResult {
            scene_generation: self.generation,
            revision: self.revision,
            hits,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HitClip {
    pub(crate) shape: HitShape,
    pub(crate) inverse: ui4me_engine_types::geometry::transform::Affine2D,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HitShape {
    Rect(crate::scene::display_list::Rect),
    Rounded(crate::scene::display_list::RoundedRect),
    Path(kurbo::BezPath, crate::scene::display_list::FillRule),
}

impl From<crate::scene::display_list::ClipShape> for HitShape {
    fn from(shape: crate::scene::display_list::ClipShape) -> Self {
        use crate::scene::display_list::{ClipShape, PathCommand};
        match shape {
            ClipShape::Rect(r) => Self::Rect(r),
            ClipShape::RoundedRect(r) => Self::Rounded(r),
            ClipShape::Path { path, fill_rule } => {
                let mut p = kurbo::BezPath::new();
                let pt = |p: &crate::scene::display_list::Point| (p.x as f64, p.y as f64);
                // Filling closes each open subpath, including one preceding another move.
                let mut open = false;
                for c in &path.commands {
                    match c {
                        PathCommand::MoveTo(a) => {
                            if open {
                                p.close_path();
                            }
                            p.move_to(pt(a));
                            open = true;
                        }
                        PathCommand::LineTo(a) => p.line_to(pt(a)),
                        PathCommand::QuadTo(a, b) => p.quad_to(pt(a), pt(b)),
                        PathCommand::CubicTo(a, b, c) => p.curve_to(pt(a), pt(b), pt(c)),
                        PathCommand::Close => {
                            p.close_path();
                            open = false;
                        }
                    }
                }
                if open {
                    p.close_path();
                }
                Self::Path(p, fill_rule)
            }
        }
    }
}

fn shape_contains(shape: &HitShape, x: f64, y: f64) -> bool {
    use crate::scene::display_list::FillRule;
    use kurbo::Shape;
    let in_rect = |r: &crate::scene::display_list::Rect| {
        x >= r.x as f64 && y >= r.y as f64 && x < r.right() as f64 && y < r.bottom() as f64
    };
    match shape {
        HitShape::Rect(r) => in_rect(r),
        HitShape::Rounded(rr) => {
            let r = &rr.rect;
            if !in_rect(r) {
                return false;
            }
            let corners = [
                (r.x, r.y, 1.0, 1.0),
                (r.right(), r.y, -1.0, 1.0),
                (r.right(), r.bottom(), -1.0, -1.0),
                (r.x, r.bottom(), 1.0, -1.0),
            ];
            for (i, (cx, cy, sx, sy)) in corners.into_iter().enumerate() {
                let (rx, ry) = rr.radii[i];
                if rx > 0.0 && ry > 0.0 {
                    let dx = (x - (cx + sx * rx) as f64) * sx as f64;
                    let dy = (y - (cy + sy * ry) as f64) * sy as f64;
                    if dx < 0.0
                        && dy < 0.0
                        && (dx / rx as f64).powi(2) + (dy / ry as f64).powi(2) > 1.0
                    {
                        return false;
                    }
                }
            }
            true
        }
        HitShape::Path(p, fill_rule) => {
            let winding = p.winding(kurbo::Point::new(x, y));
            match fill_rule {
                FillRule::NonZero => winding != 0,
                FillRule::EvenOdd => winding.abs() % 2 == 1,
            }
        }
    }
}

#[cfg(test)]
mod clip_tests {
    use super::shape_contains;
    use crate::scene::display_list::{
        ClipShape, FillRule, Path, PathCommand as C, Point, Rect, RoundedRect,
    };

    #[test]
    fn rounded_and_evenodd_shapes_exclude_their_holes() {
        let rounded = ClipShape::RoundedRect(RoundedRect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            radii: [(20.0, 20.0); 4],
        });
        assert!(!shape_contains(&rounded.clone().into(), 1.0, 1.0));
        assert!(shape_contains(&rounded.clone().into(), 20.0, 20.0));
        let p = |x, y| Point { x, y };
        let commands = vec![
            C::MoveTo(p(0.0, 0.0)),
            C::LineTo(p(100.0, 0.0)),
            C::LineTo(p(100.0, 100.0)),
            C::LineTo(p(0.0, 100.0)),
            C::Close,
            C::MoveTo(p(25.0, 25.0)),
            C::LineTo(p(75.0, 25.0)),
            C::LineTo(p(75.0, 75.0)),
            C::LineTo(p(25.0, 75.0)),
            C::Close,
        ];
        let shape = ClipShape::Path {
            path: Path { commands },
            fill_rule: FillRule::EvenOdd,
        };
        assert!(shape_contains(&shape.clone().into(), 10.0, 10.0));
        assert!(!shape_contains(&shape.clone().into(), 50.0, 50.0));
        let triangle = ClipShape::Path {
            path: Path {
                commands: vec![
                    C::MoveTo(p(0.0, 0.0)),
                    C::LineTo(p(100.0, 0.0)),
                    C::LineTo(p(0.0, 100.0)),
                ],
            },
            fill_rule: FillRule::NonZero,
        };
        assert!(shape_contains(&triangle.into(), 10.0, 10.0));
    }
}
