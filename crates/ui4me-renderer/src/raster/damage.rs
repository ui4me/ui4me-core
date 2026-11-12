//! **Damage raster.** Consecutive frames of one target mostly repeat the same display items. The
//! damage plan compares the previous frame's scene with the new one and finds the device rect
//! outside which both scenes paint exactly the same pixels; only that rect is rastered again and
//! patched into the previous frame's pixels.
//!
//! Exactness argument (no heuristic damage bits):
//!
//! 1. The items of the two lists are aligned by a sequence diff (common prefix and suffix, then a
//!    bounded Myers diff of the middle). Two items match when their kind, their draw operation and
//!    the *contents* of their whole scope chains (transforms, clip shapes, effects, masks, filters)
//!    are equal, so a matched pair paints the same pixels in the same way, and the diff keeps the
//!    paint order of the matched items.
//! 2. The damage is the union of the device bounds of every unmatched item of either list. For a
//!    pixel outside it, the items touching that pixel are matched items only, in the same order,
//!    under the same non-isolated clips: the pixel is identical.
//! 3. Isolated groups (opacity, mask and filter scopes) composite their content as a group, so a
//!    change of how the group's run of items is split can matter at a pixel no unmatched item
//!    touches: when an unmatched item lies within the run of an isolated scope, the bounds of all of
//!    that scope's items are damaged too. Items under a filter have no bounds (a filter reads its
//!    neighbourhood): an unmatched one makes the frame full.
//! 4. Bounds are conservative: geometry bounds of the operation (Bézier control points, the font's
//!    `head` bounding box for glyphs, the blur reach of shadows), mapped through the scope chain's
//!    transforms, cut to its clips, grown by an anti-aliasing margin and rounded out to device pixels.
//!    An item without known bounds (an unreadable font) makes the frame full.
//! 5. The damage raster draws every item whose bounds intersect the damage rect with the same
//!    context size, scale and transforms as a full raster (vello_cpu's fine stage computes each pixel
//!    from the commands touching it only), so the pixels inside the rect equal the full raster's.
//!
//! A different device size or scale, a font or image id whose bytes changed, or a damage rect
//! covering most of the surface makes the frame a full raster.

use std::collections::HashMap;

use crate::scene::appendix_e::SceneSnapshot;
use crate::scene::display_list::{
    ClipShape, DisplayItem, DisplayList, DrawOp, FontTable, ItemKind, PathCommand, Rect, ScopeId,
    ScopeKind,
};

/// `[x0, y0, x1, y1]` in device px.
pub type DeviceBox = [f64; 4];

/// What the damage raster has to do for a new scene.
#[derive(Clone, Debug, PartialEq)]
pub enum DamagePlan {
    /// Raster everything.
    Full,
    /// The scenes paint the same pixels: reuse the previous frame as is.
    Clean,
    /// Raster the device rect `[x0, y0, x1, y1)` (integers, within the surface) drawing only the
    /// items marked in `draw` (indexed like the new list's items).
    Partial { rect: [u32; 4], draw: Vec<bool> },
}

/// Above this share of the surface a partial raster is not cheaper than a full one.
const MAX_PARTIAL_SHARE: f64 = 0.6;
/// The Myers diff of the middle part gives up (everything in the middle is unmatched) beyond this
/// many edits; the result stays exact, only coarser.
const MAX_EDITS: usize = 512;
/// Device px added on every side of an item's bounds (anti-aliasing, rounding of the mapping).
const AA_MARGIN: f64 = 2.0;

/// Per-font `(units_per_em, x_min, y_min, x_max, y_max)` from the `head` table, cached by `(blob, index)`.
#[derive(Debug, Default)]
pub struct FontBoxes {
    cache: HashMap<(u64, u32), Option<[f32; 5]>>,
}

impl FontBoxes {
    fn get(&mut self, fonts: &FontTable, blob: u64, index: u32) -> Option<[f32; 5]> {
        *self
            .cache
            .entry((blob, index))
            .or_insert_with(|| fonts.get(blob).and_then(|b| head_box(b, index)))
    }

    /// Forget the cached boxes (font blobs may be re-registered under new ids only; kept bounded anyway).
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

/// The `head` table's units per em and font bounding box of face `index` (sfnt or TrueType collection).
fn head_box(b: &[u8], index: u32) -> Option<[f32; 5]> {
    let base = if b.get(0..4)? == b"ttcf" {
        let n = be32(b, 8)?;
        if index >= n {
            return None;
        }
        be32(b, 12 + 4 * index as usize)? as usize
    } else {
        0
    };
    let tables = be16(b, base + 4)? as usize;
    for t in 0..tables {
        let rec = base + 12 + 16 * t;
        if b.get(rec..rec + 4)? == b"head" {
            let off = be32(b, rec + 8)? as usize;
            let upem = f32::from(be16(b, off + 18)?);
            let i = |at: usize| be16(b, off + at).map(|v| f32::from(v as i16));
            if upem <= 0.0 {
                return None;
            }
            return Some([upem, i(36)?, i(38)?, i(40)?, i(42)?]);
        }
    }
    None
}

/// The device-space geometry of one scope: the transform from its space to device px, the device
/// bounds of its clips, and whether it (or an ancestor) is an isolated group or a filter.
#[derive(Clone, Copy, Debug)]
struct ScopeGeo {
    m: [f64; 6],
    clip: Option<DeviceBox>,
    filtered: bool,
}

fn mul(p: &[f64; 6], l: &[f64; 6]) -> [f64; 6] {
    // kurbo order `[a, b, c, d, e, f]`: (x, y) -> (a x + c y + e, b x + d y + f); `p * l` applies `l` first.
    [
        p[0] * l[0] + p[2] * l[1],
        p[1] * l[0] + p[3] * l[1],
        p[0] * l[2] + p[2] * l[3],
        p[1] * l[2] + p[3] * l[3],
        p[0] * l[4] + p[2] * l[5] + p[4],
        p[1] * l[4] + p[3] * l[5] + p[5],
    ]
}

fn map_box(m: &[f64; 6], b: DeviceBox) -> DeviceBox {
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3])] {
        let (dx, dy) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        out[0] = out[0].min(dx);
        out[1] = out[1].min(dy);
        out[2] = out[2].max(dx);
        out[3] = out[3].max(dy);
    }
    out
}

fn intersect(a: DeviceBox, b: DeviceBox) -> DeviceBox {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

fn grow(b: DeviceBox) -> DeviceBox {
    [
        b[0] - AA_MARGIN,
        b[1] - AA_MARGIN,
        b[2] + AA_MARGIN,
        b[3] + AA_MARGIN,
    ]
}

fn is_empty(b: &DeviceBox) -> bool {
    !(b[0] < b[2] && b[1] < b[3])
}

/// Per item of `scene` (by position), whether it may paint a pixel of the `surface`: its conservative
/// device bounds (cut to its clips, see `item_boxes`) meet the surface, or it has none (L-PERF-b3).
/// An item that paints no pixel of the surface can be left out of a full raster with the same
/// argument as the damage raster's: every other pixel is computed from the commands covering it.
pub(crate) fn surface_items(
    scene: &SceneSnapshot,
    scale: f32,
    surface: (u32, u32),
    boxes: &mut FontBoxes,
) -> Vec<bool> {
    item_boxes(scene, scale, surface, boxes)
        .iter()
        .map(|b| b.is_none_or(|b| !is_empty(&b)))
        .collect()
}

fn union(a: Option<DeviceBox>, b: DeviceBox) -> Option<DeviceBox> {
    if is_empty(&b) {
        return a;
    }
    Some(match a {
        None => b,
        Some(a) => [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ],
    })
}

fn rect_box(r: &Rect) -> DeviceBox {
    [
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.x + r.width),
        f64::from(r.y + r.height),
    ]
}

/// The lowering clamps translations to its coordinate limit; so do the bounds.
fn clamp_coord(v: f64) -> f64 {
    v.clamp(-1.0e6, 1.0e6)
}

fn scope_geometry(list: &DisplayList, scale: f32) -> Vec<ScopeGeo> {
    let root = ScopeGeo {
        m: [f64::from(scale), 0.0, 0.0, f64::from(scale), 0.0, 0.0],
        clip: None,
        filtered: false,
    };
    let mut out: Vec<ScopeGeo> = Vec::with_capacity(list.scopes.len());
    for s in &list.scopes {
        let parent = s
            .parent
            .and_then(|p| out.get(p.0 as usize).copied())
            .unwrap_or(root);
        let mut g = parent;
        match &s.kind {
            ScopeKind::Spatial(a) => {
                let local = [
                    a.a,
                    a.b,
                    a.c,
                    a.d,
                    clamp_coord(f64::from(a.e as f32)),
                    clamp_coord(f64::from(a.f as f32)),
                ];
                g.m = mul(&parent.m, &local);
            }
            ScopeKind::Clip(shape) => {
                let local = match shape {
                    ClipShape::Rect(r) => Some(rect_box(r)),
                    ClipShape::RoundedRect(rr) => Some(rect_box(&rr.rect)),
                    ClipShape::Path { path, .. } => path_box(&path.commands),
                };
                if let Some(local) = local {
                    // Clip edges are antialiased. Geometries that merely touch the clip
                    // can still contribute pixels on either side of the shared edge.
                    let d = grow(map_box(&parent.m, local));
                    g.clip = Some(match parent.clip {
                        Some(c) => intersect(c, d),
                        None => d,
                    });
                }
            }
            ScopeKind::Effect(_) | ScopeKind::Mask(_) => {}
            ScopeKind::Filter(_) | ScopeKind::Blend(_) => g.filtered = true,
        }
        out.push(g);
    }
    out
}

fn path_box(commands: &[PathCommand]) -> Option<DeviceBox> {
    let mut b: Option<DeviceBox> = None;
    let mut add = |p: &crate::scene::display_list::Point| {
        let (x, y) = (f64::from(p.x), f64::from(p.y));
        b = Some(match b {
            None => [x, y, x, y],
            Some(o) => [o[0].min(x), o[1].min(y), o[2].max(x), o[3].max(y)],
        });
    };
    for c in commands {
        match c {
            PathCommand::MoveTo(p) | PathCommand::LineTo(p) => add(p),
            PathCommand::QuadTo(a, p) => {
                add(a);
                add(p);
            }
            PathCommand::CubicTo(a, c2, p) => {
                add(a);
                add(c2);
                add(p);
            }
            PathCommand::Close => {}
        }
    }
    b
}

/// The bounds of an operation in its scope's space; `None` when unknown.
fn op_box(op: &DrawOp, fonts: &FontTable, boxes: &mut FontBoxes) -> Option<DeviceBox> {
    Some(match op {
        // Backdrop pixels depend on earlier painting, including pixels outside the clipped region.
        DrawOp::BackdropFilter { .. } => return None,
        DrawOp::FillRect { rect, .. } => rect_box(rect),
        DrawOp::FillRoundedRect { rect, .. } => rect_box(&rect.rect),
        DrawOp::FillPath { path, .. } => path_box(&path.commands).unwrap_or([0.0; 4]),
        DrawOp::DrawGlyphRun(run) => {
            if run.glyphs.is_empty() || run.size_px <= 0.0 || run.size_px > 1.0e6 {
                return Some([0.0; 4]);
            }
            let [upem, x0, y0, x1, y1] = boxes.get(fonts, run.font.blob, run.font.index)?;
            let s = f64::from(run.size_px) / f64::from(upem);
            let (x0, y0, x1, y1) = (
                f64::from(x0) * s,
                f64::from(y0) * s,
                f64::from(x1) * s,
                f64::from(y1) * s,
            );
            let size = f64::from(run.size_px);
            // Synthetic oblique shears by tan(angle) * height; synthetic bold strokes the outline.
            let shear = f64::from(run.synthesis.skew_degrees)
                .to_radians()
                .tan()
                .abs()
                * (y1.abs().max(y0.abs()));
            let grow = if run.synthesis.embolden {
                size / 12.0
            } else {
                0.0
            } + shear
                + 0.1 * size;
            let mut b = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            if run.synthesis.rotation != crate::scene::display_list::GlyphRotation::None {
                // A turned glyph (015 WM-1): its outline lies within the font box's largest extent around the origin.
                let r = x0.abs().max(x1.abs()).max(y0.abs()).max(y1.abs());
                for g in &run.glyphs {
                    let (gx, gy) = (f64::from(g.x), f64::from(g.y));
                    b = [
                        b[0].min(gx - r),
                        b[1].min(gy - r),
                        b[2].max(gx + r),
                        b[3].max(gy + r),
                    ];
                }
                return Some([b[0] - grow, b[1] - grow, b[2] + grow, b[3] + grow]);
            }
            for g in &run.glyphs {
                let (gx, gy) = (f64::from(g.x), f64::from(g.y));
                b[0] = b[0].min(gx + x0);
                b[2] = b[2].max(gx + x1);
                // glyph space is y-up: the outline spans baseline - y_max ..= baseline - y_min
                b[1] = b[1].min(gy - y1);
                b[3] = b[3].max(gy - y0);
            }
            [b[0] - grow, b[1] - grow, b[2] + grow, b[3] + grow]
        }
        DrawOp::DrawBoxShadow {
            border_rrect,
            offset,
            blur_radius,
            spread,
            inset,
            ..
        } => {
            let b = rect_box(&border_rrect.rect);
            if *inset {
                b
            } else {
                // The shape moved by the offset, grown by the spread, blurred (sigma = blur / 2; the
                // kernels reach about 3 sigma): 2 blur radii are a generous reach.
                let r = f64::from(spread.max(0.0)) + 2.0 * f64::from(blur_radius.max(0.0)) + 1.0;
                let (dx, dy) = (f64::from(offset.0), f64::from(offset.1));
                [b[0] + dx - r, b[1] + dy - r, b[2] + dx + r, b[3] + dy + r]
            }
        }
        DrawOp::DrawImage { visible, .. } => rect_box(visible),
        DrawOp::DrawImagePattern { fill, .. } => rect_box(fill),
    })
}

/// Device bounds of every item of a scene (`None` = unknown or unbounded), rounded out to whole
/// pixels with the anti-aliasing margin. Empty boxes paint nothing.
pub(crate) fn item_boxes(
    scene: &SceneSnapshot,
    scale: f32,
    surface: (u32, u32),
    boxes: &mut FontBoxes,
) -> Vec<Option<DeviceBox>> {
    let list = &scene.display_list;
    let geo = scope_geometry(list, scale);
    let full = [0.0, 0.0, f64::from(surface.0), f64::from(surface.1)];
    list.items
        .iter()
        .map(|item| {
            if item.kind == ItemKind::CANVAS_BACKGROUND
                && item.scope.is_none()
                && matches!(item.op, DrawOp::FillRect { .. })
            {
                return Some(full);
            }
            let g = item
                .scope
                .and_then(|s| geo.get(s.0 as usize).copied())
                .unwrap_or(ScopeGeo {
                    m: [f64::from(scale), 0.0, 0.0, f64::from(scale), 0.0, 0.0],
                    clip: None,
                    filtered: false,
                });
            if g.filtered {
                return None;
            }
            let local = op_box(&item.op, &scene.fonts, boxes)?;
            if is_empty(&local) {
                return Some([0.0; 4]);
            }
            let mut d = grow(map_box(&g.m, local));
            if let Some(c) = g.clip {
                d = intersect(d, c);
            }
            if !d.iter().all(|v| v.is_finite()) {
                return None;
            }
            if is_empty(&d) {
                return Some([0.0; 4]);
            }
            let d = [d[0].floor(), d[1].floor(), d[2].ceil(), d[3].ceil()];
            Some(intersect(d, full))
        })
        .collect()
}

/// Whether two scope chains have the same contents (memoised per scope pair).
struct ChainEq<'a> {
    a: &'a DisplayList,
    b: &'a DisplayList,
    memo: HashMap<(u32, u32), bool>,
}

impl ChainEq<'_> {
    fn eq(&mut self, x: Option<ScopeId>, y: Option<ScopeId>) -> bool {
        match (x, y) {
            (None, None) => true,
            (Some(x), Some(y)) => {
                if let Some(v) = self.memo.get(&(x.0, y.0)) {
                    return *v;
                }
                let (sa, sb) = (&self.a.scopes[x.0 as usize], &self.b.scopes[y.0 as usize]);
                let v = sa.kind == sb.kind && self.eq(sa.parent, sb.parent);
                self.memo.insert((x.0, y.0), v);
                v
            }
            _ => false,
        }
    }

    fn items(&mut self, i: &DisplayItem, j: &DisplayItem) -> bool {
        i.kind == j.kind && i.op == j.op && self.eq(i.scope, j.scope)
    }
}

/// The unmatched items of `old` and `new` (indexes, ascending) of a diff that keeps the order of the
/// matched items.
fn diff(old: &DisplayList, new: &DisplayList) -> (Vec<usize>, Vec<usize>) {
    let (a, b) = (&old.items, &new.items);
    let mut ce = ChainEq {
        a: old,
        b: new,
        memo: HashMap::new(),
    };
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && ce.items(&a[pre], &b[pre]) {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre
        && suf < b.len() - pre
        && ce.items(&a[a.len() - 1 - suf], &b[b.len() - 1 - suf])
    {
        suf += 1;
    }
    let (a0, a1, b0, b1) = (pre, a.len() - suf, pre, b.len() - suf);
    let (n, m) = (a1 - a0, b1 - b0);
    if n == 0 || m == 0 {
        return ((a0..a1).collect(), (b0..b1).collect());
    }
    match myers(n, m, MAX_EDITS, |i, j| ce.items(&a[a0 + i], &b[b0 + j])) {
        Some((ua, ub)) => (
            ua.into_iter().map(|i| a0 + i).collect(),
            ub.into_iter().map(|j| b0 + j).collect(),
        ),
        None => ((a0..a1).collect(), (b0..b1).collect()),
    }
}

/// Myers' O(ND) diff of sequences of lengths `n` and `m` under `eq`: the unmatched indexes of both,
/// or `None` beyond `max_d` edits.
fn myers(
    n: usize,
    m: usize,
    max_d: usize,
    mut eq: impl FnMut(usize, usize) -> bool,
) -> Option<(Vec<usize>, Vec<usize>)> {
    let max = (n + m).min(max_d);
    let off = max as isize + 1;
    let width = 2 * max + 3;
    let mut v = vec![0isize; width];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let (n_i, m_i) = (n as isize, m as isize);
    let mut found = None;
    'outer: for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let mut x =
                if k == -d || (k != d && v[(off + k - 1) as usize] < v[(off + k + 1) as usize]) {
                    v[(off + k + 1) as usize]
                } else {
                    v[(off + k - 1) as usize] + 1
                };
            let mut y = x - k;
            while x < n_i && y < m_i && eq(x as usize, y as usize) {
                x += 1;
                y += 1;
            }
            v[(off + k) as usize] = x;
            if x >= n_i && y >= m_i {
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    let d_end = found?;
    // Backtrack the edit path.
    let (mut ua, mut ub) = (Vec::new(), Vec::new());
    let (mut x, mut y) = (n_i, m_i);
    for d in (1..=d_end).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && v[(off + k - 1) as usize] < v[(off + k + 1) as usize])
        {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v[(off + prev_k) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
        }
        if prev_k == k + 1 {
            // insertion of new[prev_y]
            ub.push(prev_y as usize);
        } else {
            ua.push(prev_x as usize);
        }
        x = prev_x;
        y = prev_y;
    }
    ua.reverse();
    ub.reverse();
    Some((ua, ub))
}

/// For every isolated-group scope (opacity, mask, filter) of `list` whose run of items contains an
/// unmatched index, the bounds of all the scope's items (`Err` when one of them has none).
fn isolated_damage(
    list: &DisplayList,
    unmatched: &[usize],
    bounds: &[Option<DeviceBox>],
    mut acc: Option<DeviceBox>,
) -> Result<Option<DeviceBox>, ()> {
    let isolating: Vec<bool> = list
        .scopes
        .iter()
        .map(|s| {
            matches!(
                s.kind,
                ScopeKind::Effect(_)
                    | ScopeKind::Mask(_)
                    | ScopeKind::Filter(_)
                    | ScopeKind::Blend(_)
            )
        })
        .collect();
    if !isolating.iter().any(|v| *v) || unmatched.is_empty() {
        return Ok(acc);
    }
    // span (first, last item index) and bounds per isolating scope
    let mut spans: HashMap<u32, (usize, usize, Option<DeviceBox>, bool)> = HashMap::new();
    for (i, item) in list.items.iter().enumerate() {
        let mut cur = item.scope;
        while let Some(s) = cur {
            if isolating[s.0 as usize] {
                let e = spans.entry(s.0).or_insert((i, i, None, false));
                e.1 = i;
                match bounds[i] {
                    Some(b) => e.2 = union(e.2, b),
                    None => e.3 = true,
                }
            }
            cur = list.scopes[s.0 as usize].parent;
        }
    }
    for (first, last, b, unbounded) in spans.values() {
        let k = unmatched.partition_point(|u| u < first);
        if k < unmatched.len() && unmatched[k] <= *last {
            if *unbounded {
                return Err(());
            }
            if let Some(b) = b {
                acc = union(acc, *b);
            }
        }
    }
    Ok(acc)
}

/// The damage plan from the scene of the previous frame to `new`, both rastered at `scale` into a
/// `surface` of the same size.
pub fn plan(
    old: &SceneSnapshot,
    new: &SceneSnapshot,
    scale: f32,
    surface: (u32, u32),
    boxes: &mut FontBoxes,
) -> DamagePlan {
    if old.viewport != new.viewport {
        return DamagePlan::Full;
    }
    // An id must name the same bytes in both scenes (ids are content identities; checked anyway).
    for (id, px) in new.images.iter() {
        if let Some(o) = old.images.get(id)
            && !(std::sync::Arc::ptr_eq(&o.data, &px.data) || o == px)
        {
            return DamagePlan::Full;
        }
    }
    for (id, bytes) in new.fonts.iter() {
        if let Some(o) = old.fonts.get(id)
            && !(std::sync::Arc::ptr_eq(o, bytes) || o == bytes)
        {
            return DamagePlan::Full;
        }
    }
    let (ua, ub) = diff(&old.display_list, &new.display_list);
    if ua.is_empty() && ub.is_empty() {
        return DamagePlan::Clean;
    }
    if old
        .display_list
        .items
        .iter()
        .chain(&new.display_list.items)
        .any(|i| matches!(i.op, DrawOp::BackdropFilter { .. }))
    {
        return DamagePlan::Full;
    }
    let old_bounds = item_boxes(old, scale, surface, boxes);
    let new_bounds = item_boxes(new, scale, surface, boxes);
    let mut damage: Option<DeviceBox> = None;
    for (list, bounds) in [(&ua, &old_bounds), (&ub, &new_bounds)] {
        for &i in list.iter() {
            match bounds[i] {
                Some(b) => damage = union(damage, b),
                None => return DamagePlan::Full,
            }
        }
    }
    damage = match isolated_damage(&old.display_list, &ua, &old_bounds, damage)
        .and_then(|d| isolated_damage(&new.display_list, &ub, &new_bounds, d))
    {
        Ok(d) => d,
        Err(()) => return DamagePlan::Full,
    };
    let Some(d) = damage else {
        return DamagePlan::Clean;
    };
    let full = [0.0, 0.0, f64::from(surface.0), f64::from(surface.1)];
    let d = intersect(d, full);
    if is_empty(&d) {
        return DamagePlan::Clean;
    }
    let area = (d[2] - d[0]) * (d[3] - d[1]);
    if area > MAX_PARTIAL_SHARE * full[2] * full[3] {
        return DamagePlan::Full;
    }
    let draw = new_bounds
        .iter()
        .map(|b| b.is_none_or(|b| !is_empty(&intersect(b, d))))
        .collect();
    DamagePlan::Partial {
        rect: [d[0] as u32, d[1] as u32, d[2] as u32, d[3] as u32],
        draw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn myers_finds_minimal_unmatched_sets() {
        let a = [1, 2, 3, 4, 5, 6];
        let b = [1, 3, 4, 9, 5, 6, 7];
        let (ua, ub) = myers(a.len(), b.len(), 100, |i, j| a[i] == b[j]).unwrap();
        assert_eq!(ua, vec![1]);
        assert_eq!(ub, vec![3, 6]);
        assert!(myers(a.len(), b.len(), 1, |i, j| a[i] == b[j]).is_none());
        let (ua, ub) = myers(3, 3, 10, |i, j| [1, 2, 3][i] == [4, 5, 6][j]).unwrap();
        assert_eq!((ua, ub), (vec![0, 1, 2], vec![0, 1, 2]));
    }

    #[test]
    fn transforms_compose_like_kurbo() {
        let scale = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
        let tr = [1.0, 0.0, 0.0, 1.0, 10.0, 5.0];
        let m = mul(&scale, &tr);
        assert_eq!(map_box(&m, [0.0, 0.0, 1.0, 1.0]), [20.0, 10.0, 22.0, 12.0]);
    }
}
