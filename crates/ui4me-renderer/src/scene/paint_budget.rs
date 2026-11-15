//! F-15 (INPUTSEC-04) and F-14 (CSS filters): paint budgets degrade the offending item instead of
//! failing the frame.
//!
//! The raster pre-validation (`raster::validate`) bounds what one display list may ask of the
//! backend: gradient stops, filter functions per `filter`, mask layers and scope nesting. Values
//! past those bounds come from author CSS (a gradient with 5,000 stops, 65 filter functions, 129
//! nested `overflow: hidden` boxes), and a rejected list failed the whole frame; live, the host
//! then dropped the document. [`apply`] runs on every built scene and brings it within the bounds:
//!
//! * a gradient keeps [`MAX_STOPS`] of its stops (the first, the last and evenly spaced ones);
//! * a `filter` / `backdrop-filter` keeps its first [`MAX_FILTER_FUNCTIONS`] functions;
//! * a mask keeps its first [`MAX_MASK_LAYERS`] layers and [`MAX_MASK_OPS`] operations per layer;
//! * an item nested deeper than [`MAX_SCOPE_DEPTH`] scopes is drawn in its ancestor scope at that
//!   depth (the scopes below are not applied to it);
//! * CSS filter work is estimated (blur and drop shadow cost per pixel of the filtered layer, whose
//!   size grows with the blur extent of the filters inside it) and, over [`CSS_FILTER_WORK_BUDGET`],
//!   the blur and drop-shadow functions of the most expensive filters are removed (64 x
//!   `blur(50px)` took 6 s, 64 nested blurred boxes 25 s).
//!
//! Every degradation is reported once per element as `BUDGET_EXCEEDED` (stage `scene`, feature
//! `paint.capability.*`). The pass is a function of the built list alone, so a scene built
//! incrementally and one built cold degrade identically.

use crate::raster::validate::{
    MAX_FILTER_FUNCTIONS, MAX_MASK_LAYERS, MAX_MASK_OPS, MAX_SCOPE_DEPTH, MAX_STOPS,
};
use crate::scene::appendix_e::SceneReport;
use crate::scene::display_list::{
    DisplayList, DrawOp, Effect, FilterFn, Gradient, Paint, ScopeId, ScopeKind,
};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{FeatureId, ReportKey, ReportLocation, UnsupportedReason};
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::stage::Stage;

/// Estimated CSS filter work of one scene (ns of a release build at device scale 1).
pub const CSS_FILTER_WORK_BUDGET: f64 = 100_000_000.0;
/// Per filtered pixel: a blur, a drop shadow, any other function (measured with
/// `content_stalls.rs` on the reference machine: one `blur(50px)` of a 400x400 box ~7 ms, one
/// `drop-shadow(0 0 500px)` of a 1200x1200 box ~210 ms, 64 x `blur(50px)` 6.2 s).
const BLUR_NS: f64 = 15.0;
const SHADOW_NS: f64 = 20.0;
const OTHER_NS: f64 = 1.0;
/// How far (CSS px per side) a filtered layer grows past its content and the viewport: large blurs
/// are drawn downsampled, so their cost stops growing with the radius.
const MAX_GROWTH: f32 = 600.0;

const STOPS: &str = "paint.capability.gradient-stops";
const FUNCTIONS: &str = "paint.capability.filter-functions";
const MASK: &str = "paint.capability.mask-layers";
const DEPTH: &str = "paint.capability.scope-depth";
const FILTER_WORK: &str = "paint.capability.filter-budget";

struct Reports<'a> {
    document: DocumentId,
    out: &'a mut Vec<SceneReport>,
}

impl Reports<'_> {
    fn push(&mut self, feature: &'static str, node: NodeId) {
        let Some(feature) = FeatureId::new(feature) else {
            return;
        };
        let key = ReportKey {
            feature,
            stage: Stage::Scene,
            document: self.document,
            location: ReportLocation::Element { node },
        };
        if !self.out.iter().any(|r| r.key == key) {
            self.out.push(SceneReport {
                key,
                code: Code::BudgetExceeded,
                reason: UnsupportedReason::CapabilityFallback,
            });
        }
    }
}

/// Keep `MAX_STOPS` stops: the first, the last and evenly spaced ones (offsets stay ordered).
fn thin_stops(g: &mut Gradient) -> bool {
    let n = g.stops.len();
    if n <= MAX_STOPS {
        return false;
    }
    let keep = MAX_STOPS;
    g.stops = (0..keep)
        .map(|i| g.stops[(i * (n - 1) + (keep - 1) / 2) / (keep - 1)])
        .collect();
    true
}

fn thin_paint(p: &mut Paint) -> bool {
    match p {
        Paint::Gradient(g) if g.stops.len() > MAX_STOPS => thin_stops(std::sync::Arc::make_mut(g)),
        Paint::Gradient(_) => false,
        Paint::Solid(_) => false,
    }
}

/// Bring one draw operation within the bounds; whether anything changed and what.
fn op_budget(op: &mut DrawOp) -> Option<&'static str> {
    match op {
        DrawOp::FillRect { paint, .. }
        | DrawOp::FillRoundedRect { paint, .. }
        | DrawOp::FillPath { paint, .. } => thin_paint(paint).then_some(STOPS),
        DrawOp::DrawGlyphRun(run) => thin_paint(&mut run.paint).then_some(STOPS),
        DrawOp::BackdropFilter { functions, .. } if functions.len() > MAX_FILTER_FUNCTIONS => {
            functions.truncate(MAX_FILTER_FUNCTIONS);
            Some(FUNCTIONS)
        }
        _ => None,
    }
}

/// Blur extent of one function (CSS px), and its cost per filtered pixel.
fn function_cost(f: &FilterFn) -> (f32, f64) {
    match *f {
        FilterFn::Blur(s) if s > 0.0 => (3.0 * s, BLUR_NS),
        FilterFn::DropShadow { dx, dy, blur, .. } => {
            (3.0 * blur.max(0.0) + dx.abs().max(dy.abs()), SHADOW_NS)
        }
        _ => (0.0, OTHER_NS),
    }
}

fn expensive(f: &FilterFn) -> bool {
    matches!(f, FilterFn::Blur(s) if *s > 0.0) || matches!(f, FilterFn::DropShadow { .. })
}

/// Apply the paint budgets to `list` (module docs); degradations are reported in `reports`.
pub fn apply(
    list: &mut DisplayList,
    viewport: (f32, f32),
    document: DocumentId,
    reports: &mut Vec<SceneReport>,
) {
    let mut reports = Reports {
        document,
        out: reports,
    };
    // Depth of every scope (scopes precede their children) and the first item under it.
    let n = list.scopes.len();
    let mut depth = vec![0usize; n];
    for i in 0..n {
        depth[i] = list.scopes[i]
            .parent
            .filter(|p| (p.0 as usize) < i)
            .map_or(1, |p| depth[p.0 as usize] + 1);
    }
    let mut owner: Vec<Option<NodeId>> = vec![None; n];
    for item in &mut list.items {
        let node = item.owner.node;
        if let Some(what) = op_budget(&mut item.op) {
            reports.push(what, node);
        }
        if let Some(s) = item.scope {
            let s = s.0 as usize;
            if s < n && depth[s] > MAX_SCOPE_DEPTH {
                // Re-home the item at its ancestor scope of the deepest allowed depth.
                let mut cur = Some(ScopeId(s as u32));
                while let Some(c) = cur
                    && depth[c.0 as usize] > MAX_SCOPE_DEPTH
                {
                    cur = list.scopes[c.0 as usize].parent;
                }
                item.scope = cur;
                if let DrawOp::BackdropFilter { backdrop_root, .. } = &mut item.op
                    && let Some(root) = *backdrop_root
                    && depth
                        .get(root.0 as usize)
                        .is_none_or(|d| *d > MAX_SCOPE_DEPTH)
                {
                    *backdrop_root = None;
                }
                reports.push(DEPTH, node);
            }
            // Owner of the scopes on its chain (for reports), innermost first.
            let mut cur = item.scope;
            while let Some(c) = cur {
                let o = &mut owner[c.0 as usize];
                if o.is_some() {
                    break;
                }
                *o = Some(node);
                cur = list.scopes[c.0 as usize].parent;
            }
        }
    }
    for (i, scope) in list.scopes.iter_mut().enumerate() {
        let node = owner[i];
        match &mut scope.kind {
            ScopeKind::Filter(f) if f.functions.len() > MAX_FILTER_FUNCTIONS => {
                f.functions.truncate(MAX_FILTER_FUNCTIONS);
                if let Some(node) = node {
                    reports.push(FUNCTIONS, node);
                }
            }
            ScopeKind::Mask(m) => {
                let mut changed = m.layers.len() > MAX_MASK_LAYERS;
                m.layers.truncate(MAX_MASK_LAYERS);
                for layer in &mut m.layers {
                    changed |= layer.ops.len() > MAX_MASK_OPS;
                    layer.ops.truncate(MAX_MASK_OPS);
                    for op in &mut layer.ops {
                        changed |= op_budget(op).is_some();
                    }
                }
                if changed && let Some(node) = node {
                    reports.push(MASK, node);
                }
            }
            _ => {}
        }
    }
    filter_work(list, viewport, &owner, &mut reports);
}

/// The CSS filter work budget (module docs).
fn filter_work(
    list: &mut DisplayList,
    (vw, vh): (f32, f32),
    owner: &[Option<NodeId>],
    reports: &mut Reports<'_>,
) {
    let n = list.scopes.len();
    if !list
        .scopes
        .iter()
        .any(|s| matches!(s.kind, ScopeKind::Filter(_)))
    {
        return;
    }
    // The content box of every filter scope: the union of the ink of the items drawn in it.
    let mut bbox: Vec<Option<(f32, f32, f32, f32)>> = vec![None; n];
    for item in &list.items {
        let r = item.ink;
        let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            continue;
        }
        let mut cur = item.scope;
        while let Some(c) = cur {
            let i = c.0 as usize;
            if matches!(list.scopes[i].kind, ScopeKind::Filter(_)) {
                let b = bbox[i].get_or_insert((x0, y0, x1, y1));
                *b = (b.0.min(x0), b.1.min(y0), b.2.max(x1), b.3.max(y1));
            }
            cur = list.scopes[i].parent;
        }
    }
    // Blur extent a filtered layer grows by: its own functions' and the largest of the filters
    // inside it (an inner filter's output is the outer one's input). Children follow parents.
    let mut extent = vec![0f32; n];
    let mut cost_px = vec![0f64; n];
    for (i, scope) in list.scopes.iter().enumerate() {
        if let ScopeKind::Filter(f) = &scope.kind {
            for func in &f.functions {
                let (e, c) = function_cost(func);
                extent[i] += e;
                cost_px[i] += c;
            }
        }
    }
    let mut inner = vec![0f32; n];
    for i in (0..n).rev() {
        let total = extent[i] + inner[i];
        if let Some(p) = list.scopes[i].parent.filter(|p| (p.0 as usize) < i) {
            let p = p.0 as usize;
            inner[p] = inner[p].max(total);
        }
    }
    // A filtered layer: the content box grown by the blur extent of its filters and of those
    // inside it, at most the viewport grown the same way, the growth capped (`MAX_GROWTH`).
    let layer = |i: usize| {
        let grow = 2.0 * f64::from((extent[i] + inner[i]).min(MAX_GROWTH));
        let (w, h) = bbox[i].map_or((0.0, 0.0), |b| {
            (f64::from(b.2 - b.0).max(0.0), f64::from(b.3 - b.1).max(0.0))
        });
        let w = (w.min(f64::from(vw)) + grow).max(1.0);
        let h = (h.min(f64::from(vh)) + grow).max(1.0);
        w * h
    };
    let mut costs: Vec<(f64, usize)> = (0..n)
        .filter(|&i| cost_px[i] > 0.0)
        .map(|i| (layer(i) * cost_px[i], i))
        .collect();
    let mut total: f64 = costs.iter().map(|c| c.0).sum();
    if total <= CSS_FILTER_WORK_BUDGET {
        return;
    }
    // Remove the expensive functions of the costliest filters first.
    costs.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    for (cost, i) in costs {
        if total <= CSS_FILTER_WORK_BUDGET {
            break;
        }
        let ScopeKind::Filter(f) = &mut list.scopes[i].kind else {
            continue;
        };
        if !f.functions.iter().any(expensive) {
            continue;
        }
        f.functions.retain(|func| !expensive(func));
        let left: f64 = f.functions.len() as f64 * OTHER_NS * layer(i);
        total -= cost - left;
        if f.functions.is_empty() {
            // An isolated group with nothing to apply (a filter list is never empty).
            list.scopes[i].kind = ScopeKind::Effect(Effect { opacity: 1.0 });
        }
        if let Some(node) = owner[i] {
            reports.push(FILTER_WORK, node);
        }
    }
}
