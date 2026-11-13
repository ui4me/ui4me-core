//! owner: 010-paint-render — created by 001 slice (R-28); moved here by 010 T001 (AD-33)
//!
//! Pre-validated lowering of display items to vello_cpu (AD-17). This is the only file that
//! names vello_cpu/peniko/kurbo types. Validation lives in `crate::raster::validate`.
//! Implemented by 001 T048; scopes, rounded shapes, gradients and box shadows by 010 T021/T022.
//!
//! **Scopes.** Items carry the innermost scope of the display list's scope tree. Before each item
//! the lowering compares the item's scope chain with the currently open scopes, closes the
//! scopes that are no longer on the chain (`pop_clip` / `pop_layer`) and opens the new ones:
//! a spatial scope multiplies the current transform, a clip scope pushes a clip path in the
//! current transform, an effect scope pushes an opacity layer, a mask scope pushes a mask layer
//! whose coverage is composed from the scope's mask layers (below). All scopes are closed at the
//! end.
//!
//! **Masks.** The backend takes one device-sized alpha mask per layer. The mask scope's layers are
//! drawn (their draw operations, in the scope's transform and inside the layer's rounded clip)
//! into a second device-sized context, read back as coverage (alpha, or premultiplied luminance
//! for `mask-mode: luminance`) and combined bottom to top with the layers' `mask-composite`
//! (source-over, source-out, source-in, xor); the result is the layer's `Mask`.
//!
//! **Filters.** A filter scope draws its content into a layer of its own: a second context larger
//! than the surface by the chain's reach on every side (so a blur sees the content just outside the
//! viewport; the scope's transform carries the shift), flushed to pixels when the scope closes, run
//! through `crate::raster::filter::apply` and drawn into the parent layer as an image under the clips and
//! layers the parent has open. Filter scopes nest (a stack of layers).

use std::sync::Arc;

use crate::raster::filter;
use crate::raster::validate::invariant;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

use crate::scene::appendix_e::SceneSnapshot;
use crate::scene::borders::rrect_path;
use crate::scene::display_list::{
    ClipShape, Color, DisplayList, DrawOp, Extend, FillRule, FontTable, Gradient, GradientKind,
    ImageSampling, ImageTable, ItemKind, MaskComposite, MaskEffect, Paint, Path, PathCommand,
    Point, Rect, RoundedRect, ScopeId, ScopeKind,
};
use vello_cpu::color::{AlphaColor, DynamicColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Rect as KRect, Shape};
use vello_cpu::peniko::{self, Blob, ColorStop as PColorStop, Fill, FontData};
use vello_cpu::{
    Glyph, ImageSource, Level, Mask, PixelMetadata, Pixmap, PixmapMut, RasterizerSettings,
    RenderContext, RenderMode, RenderSettings, Resources, TargetInit,
};

/// Deterministic unhinted rendering remains the default. Set before process startup;
/// the choice is cached once and affects both ordinary and synthetic-bold glyphs.
fn glyph_hinting_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("UI4ME_GLYPH_HINTING").is_ok_and(|v| matches!(v.trim(), "1" | "true" | "on"))
    })
}

fn high_precision_requested() -> bool {
    static REQUESTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REQUESTED.get_or_init(|| {
        std::env::var("UI4ME_RASTER_QUALITY").is_ok_and(|v| v.trim().eq_ignore_ascii_case("high"))
    })
}

fn raster_quality_mode() -> RenderMode {
    #[cfg(feature = "diagnostic-raster-quality")]
    if high_precision_requested() {
        return RenderMode::OptimizeQuality;
    }
    RenderMode::OptimizeSpeed
}

fn alpha_color(c: &Color) -> AlphaColor<Srgb> {
    AlphaColor::new([c.r, c.g, c.b, c.a])
}

/// Largest local-space coordinate magnitude handed to the rasteriser. Validation accepts any
/// finite geometry (a `width: 10000000000px` box is valid CSS); the lowering clamps it so the
/// visible part is drawn and the fixed-point maths stays in range (every device coordinate stays
/// below 1e7 for device scales up to 10).
const LOCAL_LIMIT: f32 = 1.0e6;

fn clamp_coord(v: f32) -> f64 {
    v.clamp(-LOCAL_LIMIT, LOCAL_LIMIT) as f64
}

fn bez(path: &Path) -> BezPath {
    let mut bez = BezPath::new();
    let pt = |p: &Point| (clamp_coord(p.x), clamp_coord(p.y));
    for c in &path.commands {
        match c {
            PathCommand::MoveTo(p) => bez.move_to(pt(p)),
            PathCommand::LineTo(p) => bez.line_to(pt(p)),
            PathCommand::QuadTo(a, b) => bez.quad_to(pt(a), pt(b)),
            PathCommand::CubicTo(a, b, c) => bez.curve_to(pt(a), pt(b), pt(c)),
            PathCommand::Close => bez.close_path(),
        }
    }
    bez
}

fn fill_rule(r: FillRule) -> Fill {
    match r {
        FillRule::NonZero => Fill::NonZero,
        FillRule::EvenOdd => Fill::EvenOdd,
    }
}

fn krect(r: &Rect) -> KRect {
    KRect::new(
        clamp_coord(r.x),
        clamp_coord(r.y),
        clamp_coord(r.right()),
        clamp_coord(r.bottom()),
    )
}

/// The paint and the paint transform for a display-list paint (the transform is identity except
/// for elliptical radial gradients, which are circular gradients scaled about their centre).
fn brush(p: &Paint) -> (peniko::Brush, Affine) {
    match p {
        Paint::Solid(c) => (peniko::Brush::Solid(alpha_color(c)), Affine::IDENTITY),
        Paint::Gradient(g) => gradient(g),
    }
}

fn gradient(g: &Gradient) -> (peniko::Brush, Affine) {
    let stops: Vec<PColorStop> = g
        .stops
        .iter()
        .map(|s| PColorStop {
            offset: s.offset,
            color: DynamicColor::from_alpha_color(alpha_color(&s.color)),
        })
        .collect();
    let pt = |p: &Point| (clamp_coord(p.x), clamp_coord(p.y));
    let (grad, paint_tf) = match &g.kind {
        GradientKind::Linear { start, end } => (
            peniko::Gradient::new_linear(pt(start), pt(end)),
            Affine::IDENTITY,
        ),
        GradientKind::Radial {
            center,
            rx,
            ry,
            inner,
        } => {
            let c = pt(center);
            let grad = if *inner > 0.0 {
                peniko::Gradient::new_two_point_radial(c, inner * rx, c, *rx)
            } else {
                peniko::Gradient::new_radial(c, *rx)
            };
            let tf = Affine::translate(c)
                * Affine::scale_non_uniform(1.0, (*ry / *rx) as f64)
                * Affine::translate((-c.0, -c.1));
            (grad, tf)
        }
        GradientKind::Conic {
            center,
            start_angle,
            first,
            last,
        } => {
            // CSS angles run clockwise from "up"; the sweep gradient measures from +x towards +y
            // and only for angles in 0..2pi, so the gradient is turned by the start angle instead
            // of starting the sweep there.
            let a0 = (start_angle - std::f32::consts::FRAC_PI_2) as f64;
            let turn = std::f32::consts::TAU;
            (
                peniko::Gradient::new_sweep(pt(center), first * turn, last * turn),
                Affine::rotate_about(
                    a0,
                    vello_cpu::kurbo::Point::new(center.x as f64, center.y as f64),
                ),
            )
        }
    };
    let extend = match g.extend {
        Extend::Pad => peniko::Extend::Pad,
        Extend::Repeat => peniko::Extend::Repeat,
    };
    let grad = grad.with_extend(extend).with_stops(&stops[..]);
    (peniko::Brush::Gradient(grad), paint_tf)
}

/// What is open on the lowering's scope stack.
#[derive(Clone, Copy)]
enum Open {
    Spatial,
    Clip,
    Effect,
    /// A filter scope: its content goes to a layer of its own (see [`Layers`]).
    Filter,
}

/// The vello_cpu context and resources of one backend instance (single-threaded, baseline
/// SIMD level, u8 pipeline; table key `caps::SLICE_TABLE_KEY`).
pub(crate) struct VelloRaster {
    ctx: Option<RenderContext>,
    /// The device-sized context mask layers are drawn into (created on first use, same size as `ctx`).
    mask_ctx: Option<RenderContext>,
    /// The filter layers (a pool; the ones in use are tracked per render by [`Layers`]).
    layer_ctxs: Vec<RenderContext>,
    resources: Resources,
}

impl std::fmt::Debug for VelloRaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VelloRaster")
            .field("size", &self.ctx.as_ref().map(|c| (c.width(), c.height())))
            .finish()
    }
}

/// The drawing contexts of one render: the surface and the stack of open filter layers (the last open
/// one receives the drawing).
struct Layers<'a> {
    main: &'a mut RenderContext,
    pool: &'a mut Vec<RenderContext>,
    /// Open layers: their margin (device px added on every side of the surface).
    open: Vec<u32>,
    /// The surface `(width, height)`.
    surface: (u16, u16),
}

impl Layers<'_> {
    fn current(&mut self) -> &mut RenderContext {
        match self.open.len() {
            0 => &mut *self.main,
            n => &mut self.pool[n - 1],
        }
    }

    /// The size of the context that receives the drawing.
    fn size(&self) -> (u16, u16) {
        let m: u32 = self.open.iter().sum();
        (
            (u32::from(self.surface.0) + 2 * m) as u16,
            (u32::from(self.surface.1) + 2 * m) as u16,
        )
    }

    /// Conservative estimate of live contexts, including rasterizer scratch.
    fn memory_estimate(&self) -> u64 {
        let surface = u64::from(self.surface.0) * u64::from(self.surface.1);
        let mut margin = 0u64;
        let mut bytes = surface * 32;
        let mut largest = surface;
        for &next in &self.open {
            margin += u64::from(next);
            let pixels =
                (u64::from(self.surface.0) + 2 * margin) * (u64::from(self.surface.1) + 2 * margin);
            largest = largest.max(pixels);
            bytes = bytes.saturating_add(pixels * 16);
        }
        // Closed contexts stay in the pool and may retain rasterizer storage.
        for ctx in self.pool.iter().skip(self.open.len()) {
            let pixels = u64::from(ctx.width()) * u64::from(ctx.height());
            largest = largest.max(pixels);
            bytes = bytes.saturating_add(pixels * 16);
        }
        // The mask context can retain the largest expanded layer dimensions.
        bytes.saturating_add(largest.saturating_sub(surface) * 16)
    }

    /// Open a filter layer with `margin` device px on every side.
    fn push(&mut self, margin: u32, already_live: Option<u64>) -> bool {
        // The contexts and their RGBA buffers remain live while nested. Bound their
        // aggregate size and fail the frame explicitly rather than overflow or exhaust memory.
        let mut m = 0u32;
        let mut pixels = 0u64;
        let mut largest = u64::from(self.surface.0) * u64::from(self.surface.1);
        for next in self.open.iter().copied().chain(std::iter::once(margin)) {
            m = m.saturating_add(next);
            let w = u64::from(self.surface.0) + 2 * u64::from(m);
            let h = u64::from(self.surface.1) + 2 * u64::from(m);
            if w > u64::from(u16::MAX) || h > u64::from(u16::MAX) {
                return false;
            }
            largest = largest.max(w * h);
            pixels = pixels.saturating_add(w * h);
        }
        let surface = u64::from(self.surface.0) * u64::from(self.surface.1);
        // Slots beyond the one being opened remain live even when not currently open.
        for ctx in self.pool.iter().skip(self.open.len() + 1) {
            let retained = u64::from(ctx.width()) * u64::from(ctx.height());
            largest = largest.max(retained);
            pixels = pixels.saturating_add(retained);
        }
        if let Some(live) = already_live
            && (pixels > 32_000_000
                || live
                    .saturating_add(surface.saturating_mul(32))
                    .saturating_add(pixels.saturating_mul(16))
                    .saturating_add(largest.saturating_sub(surface).saturating_mul(16))
                    > 512 * 1024 * 1024)
        {
            return false;
        }
        self.open.push(margin);
        let (w, h) = self.size();
        let idx = self.open.len() - 1;
        if self.pool.len() <= idx {
            self.pool.push(RenderContext::new_with(
                w,
                h,
                RenderSettings {
                    level: Level::baseline(),
                    num_threads: 0,
                },
            ));
        }
        let slot = &mut self.pool[idx];
        if slot.width() == w && slot.height() == h {
            slot.reset();
        } else {
            *slot = RenderContext::new_with(
                w,
                h,
                RenderSettings {
                    level: Level::baseline(),
                    num_threads: 0,
                },
            );
        }
        true
    }
}

// Conservatively account heap payloads copied into a replay prefix. Arc-backed paint,
// image and font bytes are shared; B-tree nodes and vector allocation slack are covered
// by the multiplier and per-entry allowances. The hit index is not cloned.
fn replay_clone_estimate(
    list: &DisplayList,
    end: u32,
    font_count: usize,
    image_count: usize,
) -> u64 {
    use crate::scene::display_list::{
        ClipShape, DisplayItem, FilterFn, Glyph, MaskLayer, PathCommand, Scope,
    };
    use std::mem::size_of;
    fn op_bytes(op: &DrawOp) -> u64 {
        match op {
            DrawOp::BackdropFilter { functions, .. } => {
                (functions.len() * size_of::<FilterFn>()) as u64
            }
            DrawOp::FillPath { path, .. } => {
                (path.commands.len() * size_of::<PathCommand>()) as u64
            }
            DrawOp::DrawGlyphRun(run) => {
                (run.glyphs.len() * size_of::<Glyph>() + run.coords.len() * size_of::<i16>()) as u64
            }
            _ => 0,
        }
    }
    let mut bytes = ((font_count + image_count) as u64).saturating_mul(128);
    for item in list.items.iter().take_while(|i| i.id.0 < end) {
        bytes = bytes
            .saturating_add(size_of::<DisplayItem>() as u64)
            .saturating_add(op_bytes(&item.op));
    }
    for scope in &list.scopes {
        bytes = bytes.saturating_add(size_of::<Scope>() as u64);
        match &scope.kind {
            ScopeKind::Clip(ClipShape::Path { path, .. }) => {
                bytes =
                    bytes.saturating_add((path.commands.len() * size_of::<PathCommand>()) as u64)
            }
            ScopeKind::Filter(effect) => {
                bytes =
                    bytes.saturating_add((effect.functions.len() * size_of::<FilterFn>()) as u64)
            }
            ScopeKind::Mask(mask) => {
                for layer in &mask.layers {
                    bytes = bytes.saturating_add(size_of::<MaskLayer>() as u64);
                    for op in &layer.ops {
                        bytes = bytes
                            .saturating_add(size_of::<DrawOp>() as u64)
                            .saturating_add(op_bytes(op));
                    }
                }
            }
            _ => {}
        }
    }
    bytes.saturating_mul(2)
}

#[derive(Clone)]
struct BackdropCapture {
    pixmap: Arc<Pixmap>,
    size: (u32, u32),
}

#[derive(Default)]
struct BackdropBudget {
    bytes: u64,
    replays: u32,
    depth: u32,
    bounded: bool,
    live_estimate: u64,
    failure: Option<Status>,
}

/// Per-render scope tracking.
struct Scopes {
    /// Open scopes, outermost first, with the transform in force after opening each one.
    stack: Vec<(ScopeId, Open, Affine)>,
    base: Affine,
}

impl Scopes {
    fn transform(&self) -> Affine {
        self.stack.last().map(|s| s.2).unwrap_or(self.base)
    }
}

impl VelloRaster {
    pub(crate) fn new() -> VelloRaster {
        VelloRaster {
            ctx: None,
            mask_ctx: None,
            layer_ctxs: Vec::new(),
            resources: Resources::new(),
        }
    }

    /// Raster a pre-validated `list` into `out` (premultiplied RGBA8, `(width, height)`, both in
    /// 1…=65,535; checked by the caller). Items that cannot be drawn (a glyph run whose font is
    /// not in `fonts`) are omitted and recorded in `diagnostics` (`INTERNAL_INVARIANT`, stage
    /// `raster`; capability-table.md §4); combinations the backend cannot draw faithfully are
    /// omitted and recorded as `UNSUPPORTED_FEATURE`.
    ///
    /// Only the items whose `draw` entry is true (all when `None`) are drawn, into the
    /// first `rows` device rows of the surface: `out` is `width * rows * 4` bytes. The scene, the
    /// transforms and the context are those of the full surface, so every pixel of those rows is
    /// computed exactly as a full render computes it, except where a skipped item would have
    /// painted (the damage raster skips only items that paint no pixel of its damage rect).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_rows(
        &mut self,
        scene: &SceneSnapshot,
        scale: f32,
        size: (u32, u32),
        draw: Option<&[bool]>,
        rows: u32,
        out: &mut [u8],
        diagnostics: &mut Vec<Status>,
    ) -> Result<(), Status> {
        if high_precision_requested() && !cfg!(feature = "diagnostic-raster-quality") {
            return Err(Status::new(Code::UnsupportedFeature, Stage::Raster)
                .with_message("UI4ME_RASTER_QUALITY=high requires the diagnostic-raster-quality build feature"));
        }
        let mut backdrops = std::collections::HashMap::new();
        let mut budget = BackdropBudget {
            bounded: scene
                .display_list
                .items
                .iter()
                .any(|i| matches!(i.op, DrawOp::BackdropFilter { .. })),
            ..Default::default()
        };
        if budget.bounded {
            // Do not retain contexts sized for a previous, unrelated frame.
            self.mask_ctx = None;
            self.layer_ctxs.clear();
        }
        self.render_rows_inner(
            scene,
            scale,
            size,
            (0.0, 0.0),
            draw,
            rows,
            out,
            diagnostics,
            &mut backdrops,
            &mut budget,
        );
        match budget.failure {
            Some(status) => Err(status),
            None => Ok(()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_rows_inner(
        &mut self,
        scene: &SceneSnapshot,
        scale: f32,
        (width, height): (u32, u32),
        origin: (f64, f64),
        draw: Option<&[bool]>,
        rows: u32,
        out: &mut [u8],
        diagnostics: &mut Vec<Status>,
        backdrops: &mut std::collections::HashMap<(u32, u32, u32, u64, u64), BackdropCapture>,
        budget: &mut BackdropBudget,
    ) {
        let (list, fonts, images) = (&scene.display_list, &scene.fonts, &scene.images);
        let (w, h) = (width as u16, height as u16);
        let ctx = match &mut self.ctx {
            Some(c) => {
                // Vello CPU supports resizing the existing context. Rebuilding
                // it on every mouse-move/window-size event discards reusable
                // allocations and makes the compositor's resize feel sticky.
                if c.width() != w || c.height() != h {
                    c.reset_and_resize(w, h);
                } else {
                    c.reset();
                }
                c
            }
            slot => slot.insert(RenderContext::new_with(
                w,
                h,
                RenderSettings {
                    level: Level::baseline(),
                    num_threads: 0,
                },
            )),
        };
        let resources = &mut self.resources;
        let mask_ctx = &mut self.mask_ctx;
        let mut layers = Layers {
            main: ctx,
            pool: &mut self.layer_ctxs,
            open: Vec::new(),
            surface: (w, h),
        };
        let mut scopes = Scopes {
            stack: Vec::new(),
            base: Affine::translate((-origin.0, -origin.1)) * Affine::scale(scale as f64),
        };
        // One pixmap per referenced image per render (premultiplied RGBA8, shared by its items).
        let mut pixmaps: std::collections::HashMap<u64, Option<Arc<Pixmap>>> =
            std::collections::HashMap::new();
        for item in &list.items {
            if let Some(d) = draw
                && !d.get(item.id.0 as usize).copied().unwrap_or(true)
            {
                continue;
            }
            {
                let mut masker = Masker {
                    ctx2: &mut *mask_ctx,
                    resources: &mut *resources,
                    fonts,
                    images,
                    pixmaps: &mut pixmaps,
                    diagnostics: &mut *diagnostics,
                    size: layers.size(),
                    item: item.id.0,
                    scale,
                    origin,
                };
                if let Err(code) = sync_scopes(
                    &mut layers,
                    list,
                    &mut scopes,
                    item.scope,
                    &mut masker,
                    budget
                        .bounded
                        .then(|| budget.live_estimate.saturating_add(budget.bytes)),
                ) {
                    budget.failure.get_or_insert_with(|| {
                        Status::new(code, Stage::Raster)
                            .with_message("filter surface budget exceeded or allocation failed")
                    });
                    break;
                }
            }
            let tf = scopes.transform();
            if let DrawOp::BackdropFilter {
                clip,
                functions,
                backdrop_root,
            } = &item.op
            {
                // Capture in the nearest backdrop root's surface. Expanded ancestor filter
                // layers retain their offscreen margins, including multiple nested filters.
                let current_margin: u32 = layers.open.iter().sum();
                let mut filter_index = 0usize;
                let mut source_margin = 0u32;
                if let Some(root) = backdrop_root {
                    for (id, kind, _) in &scopes.stack {
                        if matches!(kind, Open::Filter) {
                            source_margin += layers.open[filter_index];
                            filter_index += 1;
                        }
                        if id == root {
                            break;
                        }
                    }
                }
                let offset = current_margin - source_margin;
                let sw = u32::from(layers.surface.0) + 2 * source_margin;
                let sh = u32::from(layers.surface.1) + 2 * source_margin;
                let source_origin = (
                    origin.0 - f64::from(source_margin),
                    origin.1 - f64::from(source_margin),
                );
                let source_tf = Affine::translate((-f64::from(offset), -f64::from(offset))) * tf;
                let bounds = source_tf.transform_rect_bbox(krect(&clip.rect));
                if sw > u16::MAX as u32
                    || sh > u16::MAX as u32
                    || u64::from(sw) * u64::from(sh) > 16_000_000
                {
                    budget.failure.get_or_insert_with(|| {
                        Status::new(Code::BudgetExceeded, Stage::Raster)
                            .with_message("backdrop capture exceeds its surface limit")
                    });
                    continue;
                }
                let key = (
                    item.id.0,
                    sw,
                    sh,
                    source_origin.0.to_bits(),
                    source_origin.1.to_bits(),
                );
                let capture = if let Some(capture) = backdrops.get(&key) {
                    capture.clone()
                } else {
                    const FRAME_BUDGET: u64 = 512 * 1024 * 1024;
                    let clone_bytes =
                        replay_clone_estimate(list, item.id.0, fonts.len(), images.len());
                    let current = layers.memory_estimate().saturating_add(clone_bytes);
                    let bytes = u64::from(sw) * u64::from(sh) * 4;
                    if budget
                        .live_estimate
                        .saturating_add(current)
                        .saturating_add(budget.bytes)
                        .saturating_add(bytes)
                        > FRAME_BUDGET
                    {
                        budget.failure.get_or_insert_with(|| {
                            Status::new(Code::BudgetExceeded, Stage::Raster)
                                .with_message("backdrop frame memory estimate exceeds budget")
                        });
                        continue;
                    }
                    if backdrops.len() >= 128
                        || budget.bytes + bytes > 128 * 1024 * 1024
                        || budget.replays >= 128
                        || budget.depth >= 4
                    {
                        budget.failure.get_or_insert_with(|| {
                            Status::new(Code::BudgetExceeded, Stage::Raster)
                                .with_message("backdrop filter replay or cache limit exceeded")
                        });
                        continue;
                    }
                    budget.bytes += bytes;
                    budget.replays += 1;
                    // Replays need paint data only; avoid cloning the hit index and items
                    // after this marker, which can dwarf the actual prefix.
                    let mut prefix = SceneSnapshot {
                        generation: scene.generation,
                        revision: scene.revision,
                        viewport: scene.viewport,
                        display_list: DisplayList {
                            items: list
                                .items
                                .iter()
                                .take_while(|candidate| candidate.id.0 < item.id.0)
                                .cloned()
                                .collect(),
                            scopes: list.scopes.clone(),
                        },
                        hit_index: Default::default(),
                        fonts: fonts.clone(),
                        images: images.clone(),
                        reports: Vec::new(),
                    };
                    if let Some(root) = backdrop_root {
                        let belongs = |mut scope: Option<ScopeId>| {
                            while let Some(id) = scope {
                                if id == *root {
                                    return true;
                                }
                                scope = list.scopes[id.0 as usize].parent;
                            }
                            false
                        };
                        prefix.display_list.items.retain(|i| belongs(i.scope));
                        let mut ancestor = Some(*root);
                        while let Some(id) = ancestor {
                            let scope = &mut prefix.display_list.scopes[id.0 as usize];
                            ancestor = scope.parent;
                            if !matches!(scope.kind, ScopeKind::Spatial(_)) {
                                scope.kind = ScopeKind::Spatial(
                                    ui4me_engine_types::geometry::transform::Affine2D::IDENTITY,
                                );
                            }
                        }
                    }
                    let Some(mut pixels) = zeroed(sw as usize * sh as usize * 4) else {
                        budget.failure.get_or_insert_with(|| {
                            Status::new(Code::OutOfMemory, Stage::Raster)
                                .with_message("backdrop capture allocation failed")
                        });
                        continue;
                    };
                    let mut replay = VelloRaster::new();
                    budget.depth += 1;
                    budget.live_estimate += current;
                    replay.render_rows_inner(
                        &prefix,
                        scale,
                        (sw, sh),
                        source_origin,
                        None,
                        sh,
                        &mut pixels,
                        diagnostics,
                        backdrops,
                        budget,
                    );
                    budget.live_estimate -= current;
                    budget.depth -= 1;
                    // Release replay contexts before allocating the filter work buffers.
                    drop(replay);
                    drop(prefix);
                    if budget.failure.is_some() {
                        continue;
                    }
                    let visible = KRect::new(
                        bounds.x0.max(0.0).min(f64::from(sw)),
                        bounds.y0.max(0.0).min(f64::from(sh)),
                        bounds.x1.max(0.0).min(f64::from(sw)),
                        bounds.y1.max(0.0).min(f64::from(sh)),
                    );
                    let remaining = FRAME_BUDGET.saturating_sub(
                        budget
                            .live_estimate
                            .saturating_add(current)
                            .saturating_add(budget.bytes),
                    );
                    let max_work = remaining; // Bytes available for work and filter scratch.
                    if let Err(code) = filter_backdrop(
                        &mut pixels,
                        sw as usize,
                        sh as usize,
                        visible,
                        &clip.rect,
                        functions,
                        source_tf,
                        max_work,
                    ) {
                        budget.failure.get_or_insert_with(|| {
                            Status::new(code, Stage::Raster)
                                .with_message("backdrop filter work failed")
                        });
                        continue;
                    }
                    let pixmap = Arc::new(Pixmap::from_parts(
                        pixels,
                        sw as u16,
                        sh as u16,
                        PixelMetadata::new(peniko::ImageAlphaType::AlphaPremultiplied, true),
                    ));
                    let capture = BackdropCapture {
                        pixmap,
                        size: (sw, sh),
                    };
                    backdrops.insert(key, capture.clone());
                    capture
                };
                let ctx = layers.current();
                ctx.set_transform(tf);
                ctx.push_layer(
                    Some(&bez(&rrect_path(clip))),
                    Some(peniko::Compose::Copy.into()),
                    None,
                    None,
                    None,
                );
                ctx.set_transform(Affine::translate((f64::from(offset), f64::from(offset))));
                ctx.reset_paint_transform();
                ctx.set_paint(peniko::Brush::Image(vello_cpu::Image {
                    image: ImageSource::Pixmap(capture.pixmap),
                    sampler: peniko::ImageSampler {
                        x_extend: peniko::Extend::Pad,
                        y_extend: peniko::Extend::Pad,
                        quality: peniko::ImageQuality::Low,
                        alpha: 1.0,
                    },
                }));
                ctx.fill_rect(&KRect::new(
                    0.0,
                    0.0,
                    f64::from(capture.size.0),
                    f64::from(capture.size.1),
                ));
                ctx.pop_layer();
                continue;
            }
            let size = layers.size();
            let mut env = DrawEnv {
                resources: &mut *resources,
                fonts,
                images,
                pixmaps: &mut pixmaps,
                diagnostics: &mut *diagnostics,
                size: (u32::from(size.0), u32::from(size.1)),
                scale,
                origin,
            };
            draw_op(
                layers.current(),
                &mut env,
                tf,
                item.id.0,
                Some(item.kind),
                &item.op,
            );
        }
        if budget.failure.is_some() {
            return;
        }
        {
            let mut masker = Masker {
                ctx2: &mut *mask_ctx,
                resources: &mut *resources,
                fonts,
                images,
                pixmaps: &mut pixmaps,
                diagnostics: &mut *diagnostics,
                size: layers.size(),
                item: 0,
                scale,
                origin,
            };
            if let Err(code) = sync_scopes(
                &mut layers,
                list,
                &mut scopes,
                None,
                &mut masker,
                budget
                    .bounded
                    .then(|| budget.live_estimate.saturating_add(budget.bytes)),
            ) {
                budget.failure.get_or_insert_with(|| {
                    Status::new(code, Stage::Raster)
                        .with_message("filter surface budget exceeded or allocation failed")
                });
            }
        }
        if budget.failure.is_some() {
            return;
        }
        let ctx = layers.main;
        ctx.flush();
        let Some(target) = PixmapMut::new(w, rows.min(height) as u16, out) else {
            diagnostics.push(invariant("target buffer size mismatch".into()));
            return;
        };
        ctx.render_with(
            target,
            resources,
            RasterizerSettings {
                render_mode: raster_quality_mode(),
                target_init: TargetInit::Clear(AlphaColor::TRANSPARENT),
                pixel_format: vello_cpu::PixelFormat::Rgba8,
                offset: (0, 0),
            },
        );
    }
}

/// What drawing one operation needs besides the context.
struct DrawEnv<'a> {
    resources: &'a mut Resources,
    fonts: &'a FontTable,
    images: &'a ImageTable,
    pixmaps: &'a mut std::collections::HashMap<u64, Option<Arc<Pixmap>>>,
    diagnostics: &'a mut Vec<Status>,
    /// The device surface `(width, height)`; the canvas fills all of it.
    size: (u32, u32),
    scale: f32,
    origin: (f64, f64),
}

/// Align the baseline of a 1x translated run without changing glyph spacing.
/// Other transforms and device scales retain their existing subpixel behavior.
fn glyph_origin_adjustment(
    first: Option<(f32, f32)>,
    scene: Affine,
    scale: f32,
    origin: (f64, f64),
) -> (f32, f32) {
    let Some(first) = first else {
        return (0.0, 0.0);
    };
    let [a, b, c, d, _, ty] = scene.as_coeffs();
    if scale != 1.0
        || (a - 1.0).abs() > 1e-9
        || b.abs() > 1e-9
        || c.abs() > 1e-9
        || (d - 1.0).abs() > 1e-9
    {
        return (0.0, 0.0);
    }
    // Backdrop replays draw to a translated surface; align to the original device grid.
    let y = f64::from(first.1) + ty + origin.1;
    (0.0, ((y + 0.5).floor() - y) as f32)
}

/// Draw one operation of item `id` (its `kind` when it is a display item; mask-layer operations
/// have none) under `scene`, the transform of the item's scope chain.
fn draw_op(
    ctx: &mut RenderContext,
    env: &mut DrawEnv<'_>,
    scene: Affine,
    id: u32,
    kind: Option<ItemKind>,
    op: &DrawOp,
) {
    let DrawEnv {
        resources,
        fonts,
        images,
        pixmaps,
        diagnostics,
        size: (width, height),
        scale,
        origin,
    } = env;
    match op {
        // Backdrops are handled by the render loop, which has access to the preceding items.
        DrawOp::BackdropFilter { .. } => {}
        // The canvas covers the whole device surface (also the fractional last row and
        // column of `ceil(viewport × scale)`).
        DrawOp::FillRect {
            paint: Paint::Solid(c),
            ..
        } if kind == Some(ItemKind::CANVAS_BACKGROUND) => {
            ctx.set_transform(Affine::IDENTITY);
            ctx.set_paint(alpha_color(c));
            ctx.fill_rect(&KRect::new(0.0, 0.0, *width as f64, *height as f64));
        }
        DrawOp::FillRect { rect, paint } => {
            set_paint(ctx, scene, paint);
            ctx.fill_rect(&krect(rect));
        }
        DrawOp::FillRoundedRect { rect, paint } => {
            set_paint(ctx, scene, paint);
            ctx.set_fill_rule(Fill::NonZero);
            ctx.fill_path(&bez(&rrect_path(rect)));
        }
        DrawOp::FillPath {
            path,
            fill_rule: rule,
            paint,
        } => {
            set_paint(ctx, scene, paint);
            ctx.set_fill_rule(fill_rule(*rule));
            ctx.fill_path(&bez(path));
        }
        // `font-size: 0` (or a size beyond the rasteriser's range): nothing to draw.
        DrawOp::DrawGlyphRun(run) if run.size_px <= 0.0 || run.size_px > LOCAL_LIMIT => {}
        DrawOp::DrawGlyphRun(run) => {
            let Some(bytes) = fonts.get(run.font.blob) else {
                diagnostics.push(invariant(format!(
                    "item {id}: font blob {} is not held by the scene snapshot; glyph run omitted",
                    run.font.blob
                )));
                return;
            };
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(bytes.clone());
            let font = FontData::new(Blob::from_raw_parts(data, run.font.blob), run.font.index);
            set_paint(ctx, scene, &run.paint);
            ctx.set_fill_rule(Fill::NonZero);
            // Grayscale anti-aliasing; hinting is an explicit process opt-in. No subpixel AA.
            // At 1x, pure translations place the run baseline on the device pixel grid.
            // Keep offsets within the shaped run unchanged, including combining marks.
            let (dx, dy) = glyph_origin_adjustment(run.origin, scene, *scale, *origin);
            let glyphs = run
                .glyphs
                .iter()
                .filter(|g| g.x.abs() <= LOCAL_LIMIT && g.y.abs() <= LOCAL_LIMIT)
                .map(|g| Glyph {
                    id: g.id,
                    x: g.x + dx,
                    y: g.y + dy,
                });
            // Synthetic italic shears the outlines about the baseline; synthetic bold
            // strokes them with the standard fake-bold width as well as filling (below).
            let skew = synthetic_skew(run.synthesis.skew_degrees);
            // A sideways glyph in vertical text (015 WM-1) is turned about its origin, after its synthetic shear.
            // A combined (tate-chu-yoko) run is compressed horizontally about each glyph origin.
            let skew = match run.synthesis.scale_x.filter(|s| (*s - 1.0).abs() > 1e-4) {
                Some(s) => Some(
                    skew.map_or(Affine::scale_non_uniform(f64::from(s), 1.0), |k| {
                        Affine::scale_non_uniform(f64::from(s), 1.0) * k
                    }),
                ),
                None => skew,
            };
            let skew = match run.synthesis.rotation {
                crate::scene::display_list::GlyphRotation::None => skew,
                r => {
                    let turn = Affine::rotate(
                        if r == crate::scene::display_list::GlyphRotation::Clockwise {
                            std::f64::consts::FRAC_PI_2
                        } else {
                            -std::f64::consts::FRAC_PI_2
                        },
                    );
                    Some(skew.map_or(turn, |k| turn * k))
                }
            };
            // A variable face at a non-default instance is drawn at its normalised coordinates.
            let mut builder = ctx
                .glyph_run(resources, &font)
                .font_size(run.size_px)
                .hint(glyph_hinting_enabled())
                .normalized_coords(&run.coords);
            if let Some(skew) = skew {
                builder = builder.glyph_transform(skew);
            }
            if let Err(e) = builder.fill_glyphs(glyphs.clone()) {
                diagnostics.push(invariant(format!(
                    "item {id}: glyph run partially rendered ({e:?})"
                )));
            }
            if run.synthesis.embolden {
                ctx.set_stroke(vello_cpu::kurbo::Stroke::new(f64::from(fake_bold_extra(
                    run.size_px,
                ))));
                let mut builder = ctx
                    .glyph_run(resources, &font)
                    .font_size(run.size_px)
                    .hint(glyph_hinting_enabled())
                    .normalized_coords(&run.coords);
                if let Some(skew) = skew {
                    builder = builder.glyph_transform(skew);
                }
                if let Err(e) = builder.stroke_glyphs(glyphs) {
                    diagnostics.push(invariant(format!(
                        "item {id}: synthetic bold partially rendered ({e:?})"
                    )));
                }
            }
        }
        DrawOp::DrawImage {
            image,
            dest,
            visible,
            sampling,
        } => {
            let Some(pixmap) = image_pixmap(images, pixmaps, *image, id, diagnostics) else {
                return;
            };
            ctx.set_transform(scene);
            set_image_paint(ctx, pixmap, *sampling, (false, false), dest);
            ctx.fill_rect(&krect(visible));
        }
        DrawOp::DrawImagePattern {
            image,
            tile,
            repeat_x,
            repeat_y,
            fill,
            sampling,
        } => {
            let Some(pixmap) = image_pixmap(images, pixmaps, *image, id, diagnostics) else {
                return;
            };
            ctx.set_transform(scene);
            set_image_paint(ctx, pixmap, *sampling, (*repeat_x, *repeat_y), tile);
            ctx.fill_rect(&krect(fill));
        }
        DrawOp::DrawBoxShadow {
            border_rrect,
            padding_rrect,
            offset,
            blur_radius,
            spread,
            color,
            inset,
        } => {
            ctx.set_transform(scene);
            ctx.reset_paint_transform();
            let sh = Shadow {
                border: border_rrect,
                padding: padding_rrect,
                offset: *offset,
                blur: *blur_radius,
                spread: *spread,
                color: *color,
            };
            if *inset {
                inset_shadow(ctx, &sh, id, diagnostics);
            } else {
                outer_shadow(ctx, &sh, id, diagnostics);
            }
        }
    }
}

/// The second context and the shared state mask layers are drawn with.
struct Masker<'a> {
    ctx2: &'a mut Option<RenderContext>,
    resources: &'a mut Resources,
    fonts: &'a FontTable,
    images: &'a ImageTable,
    pixmaps: &'a mut std::collections::HashMap<u64, Option<Arc<Pixmap>>>,
    diagnostics: &'a mut Vec<Status>,
    /// The device size of the main context (the mask must match it).
    size: (u16, u16),
    /// The item the mask opens before (diagnostics only).
    item: u32,
    scale: f32,
    origin: (f64, f64),
}

impl Masker<'_> {
    /// The coverage (0 to 1 per device pixel, row-major) of one mask layer under `transform`.
    fn layer_coverage(
        &mut self,
        layer: &crate::scene::display_list::MaskLayer,
        transform: Affine,
    ) -> Vec<f32> {
        let (w, h) = self.size;
        let n = w as usize * h as usize;
        let ctx2 = match &mut *self.ctx2 {
            Some(c) if c.width() == w && c.height() == h => {
                c.reset();
                c
            }
            slot => slot.insert(RenderContext::new_with(
                w,
                h,
                RenderSettings {
                    level: Level::baseline(),
                    num_threads: 0,
                },
            )),
        };
        // The layer is drawn inside the mask painting area (rounded by the box radii).
        ctx2.set_transform(transform);
        ctx2.reset_paint_transform();
        ctx2.set_fill_rule(Fill::NonZero);
        if layer.clip.is_square() {
            ctx2.push_clip_rect(&krect(&layer.clip.rect));
        } else {
            ctx2.push_clip_path(&bez(&rrect_path(&layer.clip)));
        }
        let mut env = DrawEnv {
            resources: &mut *self.resources,
            fonts: self.fonts,
            images: self.images,
            pixmaps: &mut *self.pixmaps,
            diagnostics: &mut *self.diagnostics,
            size: (w as u32, h as u32),
            scale: self.scale,
            origin: self.origin,
        };
        for op in &layer.ops {
            draw_op(ctx2, &mut env, transform, self.item, None, op);
        }
        ctx2.pop_clip();
        ctx2.flush();
        let mut buf = vec![0u8; n * 4];
        if let Some(target) = PixmapMut::new(w, h, &mut buf) {
            ctx2.render_with(
                target,
                self.resources,
                RasterizerSettings {
                    render_mode: raster_quality_mode(),
                    target_init: TargetInit::Clear(AlphaColor::TRANSPARENT),
                    pixel_format: vello_cpu::PixelFormat::Rgba8,
                    offset: (0, 0),
                },
            );
        }
        let (rgba, _) = buf.as_chunks::<4>();
        rgba.iter()
            .map(|p| {
                if layer.luminance {
                    // Premultiplied sRGB luminance (CSS Masking 1 section 7.10.1; the backend's own rule).
                    (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2]))
                        / 255.0
                } else {
                    f32::from(p[3]) / 255.0
                }
            })
            .collect()
    }

    /// The device-sized mask of `m` under `transform`: the layers composed bottom to top.
    fn build(&mut self, m: &MaskEffect, transform: Affine) -> Mask {
        let (w, h) = self.size;
        let mut acc: Option<Vec<f32>> = None;
        for layer in m.layers.iter().rev() {
            let cov = self.layer_coverage(layer, transform);
            acc = Some(match acc {
                None => cov,
                Some(below) => cov
                    .iter()
                    .zip(&below)
                    .map(|(&s, &d)| match layer.composite {
                        MaskComposite::Add => s + d * (1.0 - s),
                        MaskComposite::Subtract => s * (1.0 - d),
                        MaskComposite::Intersect => s * d,
                        MaskComposite::Exclude => s * (1.0 - d) + d * (1.0 - s),
                    })
                    .collect(),
            });
        }
        let data = acc
            .unwrap_or_default()
            .iter()
            .map(|c| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
            .collect();
        Mask::from_parts(data, w, h)
    }
}

/// The pixmap of image `id`, built once per render; an image the snapshot does not hold, or one
/// larger than the backend's 65,535 px limit, is omitted and recorded.
fn image_pixmap(
    table: &ImageTable,
    cache: &mut std::collections::HashMap<u64, Option<Arc<Pixmap>>>,
    id: u64,
    item: u32,
    diagnostics: &mut Vec<Status>,
) -> Option<Arc<Pixmap>> {
    cache
        .entry(id)
        .or_insert_with(|| {
            let Some(px) = table.get(id) else {
                diagnostics.push(invariant(format!(
                    "item {item}: image {id} is not held by the scene snapshot; item omitted"
                )));
                return None;
            };
            let (Ok(w), Ok(h)) = (u16::try_from(px.width), u16::try_from(px.height)) else {
                unsupported(
                    diagnostics,
                    item,
                    "image larger than 65,535 px on a side is not drawn",
                );
                return None;
            };
            if w == 0 || h == 0 || px.data.len() != w as usize * h as usize * 4 {
                diagnostics.push(invariant(format!(
                    "item {item}: image {id} has inconsistent dimensions; item omitted"
                )));
                return None;
            }
            Some(Arc::new(Pixmap::from_parts(
                px.data.to_vec(),
                w,
                h,
                PixelMetadata::new(peniko::ImageAlphaType::AlphaPremultiplied, true),
            )))
        })
        .clone()
}

/// Image paint: image pixel `(0,0)` is the origin of `tile`, the whole image spans `tile`;
/// `repeat` makes the image repeat along an axis (otherwise its edge pixels extend, which only
/// matters outside the filled area).
fn set_image_paint(
    ctx: &mut RenderContext,
    pixmap: Arc<Pixmap>,
    sampling: ImageSampling,
    repeat: (bool, bool),
    tile: &Rect,
) {
    let ext = |r: bool| {
        if r {
            peniko::Extend::Repeat
        } else {
            peniko::Extend::Pad
        }
    };
    let (pw, ph) = (pixmap.width() as f64, pixmap.height() as f64);
    let brush = vello_cpu::Image {
        image: ImageSource::Pixmap(pixmap),
        sampler: peniko::ImageSampler {
            x_extend: ext(repeat.0),
            y_extend: ext(repeat.1),
            quality: match sampling {
                ImageSampling::Smooth => peniko::ImageQuality::Medium,
                ImageSampling::Nearest => peniko::ImageQuality::Low,
            },
            alpha: 1.0,
        },
    };
    ctx.set_paint_transform(
        Affine::translate((tile.x as f64, tile.y as f64))
            * Affine::scale_non_uniform(tile.width as f64 / pw, tile.height as f64 / ph),
    );
    ctx.set_paint(peniko::Brush::Image(brush));
}

fn set_paint(ctx: &mut RenderContext, transform: Affine, paint: &Paint) {
    ctx.set_transform(transform);
    let (b, tf) = brush(paint);
    match b {
        peniko::Brush::Solid(c) => {
            ctx.reset_paint_transform();
            ctx.set_paint(c);
        }
        peniko::Brush::Gradient(g) => {
            ctx.set_paint_transform(tf);
            ctx.set_paint(g);
        }
        // Not produced by `brush`.
        _ => {}
    }
}

/// The linear part of an affine map.
fn linear(a: Affine) -> filter::Linear {
    let c = a.as_coeffs();
    [c[0], c[1], c[2], c[3]]
}

fn zeroed(len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.try_reserve_exact(len).ok()?;
    out.resize(len, 0);
    Some(out)
}

/// Filter an axis-aligned backdrop with mirrored border-box edges (Filter Effects 2).
/// Returns a coded error when a bounded capture cannot be produced.
// Backdrop operations carry the surface, geometry and finite work budget explicitly.
#[allow(clippy::too_many_arguments)]
fn filter_backdrop(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    bounds: vello_cpu::kurbo::Rect,
    clip: &Rect,
    functions: &[crate::scene::display_list::FilterFn],
    tf: Affine,
    max_work: u64,
) -> Result<(), Code> {
    use crate::scene::display_list::FilterFn;
    let spatial = functions
        .iter()
        .any(|f| matches!(f, FilterFn::Blur(_) | FilterFn::DropShadow { .. }));
    if !spatial {
        let (x0, y0, x1, y1) = (
            bounds.x0.floor() as usize,
            bounds.y0.floor() as usize,
            bounds.x1.ceil() as usize,
            bounds.y1.ceil() as usize,
        );
        if x1 <= x0 || y1 <= y0 {
            return Ok(());
        }
        let (cw, ch) = (x1 - x0, y1 - y0);
        if (cw as u64) * (ch as u64) > 8_500_000 || (cw as u64) * (ch as u64) > max_work / 4 {
            return Err(Code::BudgetExceeded);
        }
        let Some(mut work) = zeroed(cw * ch * 4) else {
            return Err(Code::OutOfMemory);
        };
        for y in 0..ch {
            let src = ((y + y0) * width + x0) * 4;
            work[y * cw * 4..(y + 1) * cw * 4].copy_from_slice(&pixels[src..src + cw * 4]);
        }
        if !filter::apply_colors(&mut work, functions) {
            return Err(Code::OutOfMemory);
        }
        for y in 0..ch {
            let dst = ((y + y0) * width + x0) * 4;
            pixels[dst..dst + cw * 4].copy_from_slice(&work[y * cw * 4..(y + 1) * cw * 4]);
        }
        return Ok(());
    }
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return Ok(());
    }
    let c = tf.as_coeffs();
    if bounds.x0 < 0.0 || bounds.y0 < 0.0 || bounds.x1 > width as f64 || bounds.y1 > height as f64 {
        return Err(Code::UnsupportedFeature);
    }
    let rotated = c[1].abs() > 1e-9 || c[2].abs() > 1e-9;
    if rotated {
        let dot = c[0] * c[2] + c[1] * c[3];
        if dot.abs() > 1e-6 {
            return filter_skewed_backdrop(
                pixels, width, height, bounds, clip, functions, tf, max_work,
            );
        }
        return filter_transformed_backdrop(
            pixels, width, height, bounds, clip, functions, tf, max_work,
        );
    }
    let (x0, y0, x1, y1) = (
        bounds.x0.floor() as usize,
        bounds.y0.floor() as usize,
        bounds.x1.ceil() as usize,
        bounds.y1.ceil() as usize,
    );
    if x1 <= x0 || y1 <= y0 {
        return Ok(());
    }
    let reach = filter::device_reach(functions, linear(tf))
        .into_iter()
        .max()
        .unwrap_or(0);
    if reach > i64::from(filter::MAX_MARGIN) {
        return Err(Code::BudgetExceeded);
    }
    let pad = reach as usize;
    let (cw, ch) = (x1 - x0, y1 - y0);
    let pw = cw + 2 * pad;
    let output_bytes = (cw as u64) * (ch as u64) * 4;
    // Each strip reads its entire halo from the original capture; keep output separate
    // until every strip is finished so later strips never sample filtered pixels.
    let work_pixels = max_work.saturating_sub(output_bytes) / 72;
    let max_rows = (work_pixels / pw as u64).min(8_500_000 / pw as u64) as usize;
    if max_rows <= 2 * pad {
        return Err(Code::BudgetExceeded);
    }
    let rows = (max_rows - 2 * pad).min(ch);
    let reflect = |v: isize, n: usize| -> usize {
        let k = v.rem_euclid((2 * n) as isize) as usize;
        if k < n { k } else { 2 * n - 1 - k }
    };
    let Some(mut output) = zeroed(cw * ch * 4) else {
        return Err(Code::OutOfMemory);
    };
    let mut top = 0;
    while top < ch {
        let count = rows.min(ch - top);
        let ph = count + 2 * pad;
        let Some(mut work) = zeroed(pw * ph * 4) else {
            return Err(Code::OutOfMemory);
        };
        for y in 0..ph {
            let sy = y0 + reflect((top + y) as isize - pad as isize, ch);
            for x in 0..pw {
                let sx = x0 + reflect(x as isize - pad as isize, cw);
                let src = (sy * width + sx) * 4;
                let dst = (y * pw + x) * 4;
                work[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
        }
        if !filter::apply(&mut work, pw, ph, functions, linear(tf)) {
            return Err(Code::OutOfMemory);
        }
        for y in 0..count {
            let src = ((y + pad) * pw + pad) * 4;
            let dst = (top + y) * cw * 4;
            output[dst..dst + cw * 4].copy_from_slice(&work[src..src + cw * 4]);
        }
        top += count;
    }
    for y in 0..ch {
        let dst = ((y + y0) * width + x0) * 4;
        pixels[dst..dst + cw * 4].copy_from_slice(&output[y * cw * 4..(y + 1) * cw * 4]);
    }
    Ok(())
}

/// Stylo's skewed-filter reference uses a device-axis blur. Mirror samples through the
/// element's local edges, then filter in device coordinates.
#[allow(clippy::too_many_arguments)]
fn filter_skewed_backdrop(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    bounds: KRect,
    clip: &Rect,
    functions: &[crate::scene::display_list::FilterFn],
    tf: Affine,
    max_work: u64,
) -> Result<(), Code> {
    let reach = filter::device_reach(functions, linear(tf))
        .into_iter()
        .max()
        .unwrap_or(0);
    if reach > i64::from(filter::MAX_MARGIN) {
        return Err(Code::BudgetExceeded);
    }
    let (x0, y0, x1, y1) = (
        bounds.x0.floor() as usize,
        bounds.y0.floor() as usize,
        bounds.x1.ceil() as usize,
        bounds.y1.ceil() as usize,
    );
    if x1 <= x0 || y1 <= y0 {
        return Ok(());
    }
    let (cw, ch, pad) = (x1 - x0, y1 - y0, reach as usize);
    let (pw, ph) = (cw + 2 * pad, ch + 2 * pad);
    let mirror = |v: f64, lo: f64, hi: f64| {
        let n = hi - lo;
        let t = (v - lo).rem_euclid(2.0 * n);
        lo + if t < n { t } else { 2.0 * n - t }
    };
    let inverse = tf.inverse();
    let (left, top, right, bottom) = (
        f64::from(clip.x),
        f64::from(clip.y),
        f64::from(clip.right()),
        f64::from(clip.bottom()),
    );
    if (pw as u64) * (ph as u64) <= 8_500_000 && (pw as u64) * (ph as u64) <= max_work / 72 {
        let Some(mut work) = zeroed(pw * ph * 4) else {
            return Err(Code::OutOfMemory);
        };
        for y in 0..ph {
            for x in 0..pw {
                let p = vello_cpu::kurbo::Point::new(
                    x0 as f64 + x as f64 - pad as f64 + 0.5,
                    y0 as f64 + y as f64 - pad as f64 + 0.5,
                );
                let q = inverse * p;
                let q = tf
                    * vello_cpu::kurbo::Point::new(
                        mirror(q.x, left, right),
                        mirror(q.y, top, bottom),
                    );
                let (xx, yy) = (
                    (q.x - 0.5).clamp(0.0, (width - 1) as f64),
                    (q.y - 0.5).clamp(0.0, (height - 1) as f64),
                );
                let (ax, ay) = (xx.floor() as usize, yy.floor() as usize);
                let (bx, by) = ((ax + 1).min(width - 1), (ay + 1).min(height - 1));
                let (fx, fy) = (xx - ax as f64, yy - ay as f64);
                let dst = (y * pw + x) * 4;
                for chn in 0..4 {
                    let a = f64::from(pixels[(ay * width + ax) * 4 + chn]);
                    let b = f64::from(pixels[(ay * width + bx) * 4 + chn]);
                    let c = f64::from(pixels[(by * width + ax) * 4 + chn]);
                    let d = f64::from(pixels[(by * width + bx) * 4 + chn]);
                    work[dst + chn] = ((a * (1.0 - fx) + b * fx) * (1.0 - fy)
                        + (c * (1.0 - fx) + d * fx) * fy)
                        .round() as u8;
                }
            }
        }
        if !filter::apply(&mut work, pw, ph, functions, linear(tf)) {
            return Err(Code::OutOfMemory);
        }
        for y in 0..ch {
            let src = ((y + pad) * pw + pad) * 4;
            let dst = ((y + y0) * width + x0) * 4;
            pixels[dst..dst + cw * 4].copy_from_slice(&work[src..src + cw * 4]);
        }
        return Ok(());
    }
    let output_bytes = (cw as u64) * (ch as u64) * 4;
    let work_pixels = max_work.saturating_sub(output_bytes) / 72;
    let max_rows = (work_pixels / pw as u64).min(8_500_000 / pw as u64) as usize;
    if max_rows <= 2 * pad {
        return Err(Code::BudgetExceeded);
    }
    let rows = (max_rows - 2 * pad).min(ch);
    let Some(mut output) = zeroed(cw * ch * 4) else {
        return Err(Code::OutOfMemory);
    };
    let mut row_start = 0;
    while row_start < ch {
        let count = rows.min(ch - row_start);
        let ph = count + 2 * pad;
        let Some(mut work) = zeroed(pw * ph * 4) else {
            return Err(Code::OutOfMemory);
        };
        for y in 0..ph {
            for x in 0..pw {
                let p = vello_cpu::kurbo::Point::new(
                    x0 as f64 + x as f64 - pad as f64 + 0.5,
                    y0 as f64 + (row_start + y) as f64 - pad as f64 + 0.5,
                );
                let q = inverse * p;
                let q = tf
                    * vello_cpu::kurbo::Point::new(
                        mirror(q.x, left, right),
                        mirror(q.y, top, bottom),
                    );
                let (xx, yy) = (
                    (q.x - 0.5).clamp(0.0, (width - 1) as f64),
                    (q.y - 0.5).clamp(0.0, (height - 1) as f64),
                );
                let (ax, ay) = (xx.floor() as usize, yy.floor() as usize);
                let (bx, by) = ((ax + 1).min(width - 1), (ay + 1).min(height - 1));
                let (fx, fy) = (xx - ax as f64, yy - ay as f64);
                let dst = (y * pw + x) * 4;
                for chn in 0..4 {
                    let a = f64::from(pixels[(ay * width + ax) * 4 + chn]);
                    let b = f64::from(pixels[(ay * width + bx) * 4 + chn]);
                    let c = f64::from(pixels[(by * width + ax) * 4 + chn]);
                    let d = f64::from(pixels[(by * width + bx) * 4 + chn]);
                    work[dst + chn] = ((a * (1.0 - fx) + b * fx) * (1.0 - fy)
                        + (c * (1.0 - fx) + d * fx) * fy)
                        .round() as u8;
                }
            }
        }
        if !filter::apply(&mut work, pw, ph, functions, linear(tf)) {
            return Err(Code::OutOfMemory);
        }
        for y in 0..count {
            let src = ((y + pad) * pw + pad) * 4;
            let dst = (row_start + y) * cw * 4;
            output[dst..dst + cw * 4].copy_from_slice(&work[src..src + cw * 4]);
        }
        row_start += count;
    }
    for y in 0..ch {
        let dst = ((y + y0) * width + x0) * 4;
        pixels[dst..dst + cw * 4].copy_from_slice(&output[y * cw * 4..(y + 1) * cw * 4]);
    }
    Ok(())
}

/// Sample a transformed backdrop on a local-axis grid, mirror its edges, filter there,
/// then map it back to device pixels. This keeps rotated anisotropic kernels oriented with the box.
#[allow(clippy::too_many_arguments)]
fn filter_transformed_backdrop(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    bounds: KRect,
    clip: &Rect,
    functions: &[crate::scene::display_list::FilterFn],
    tf: Affine,
    max_work: u64,
) -> Result<(), Code> {
    let c = tf.as_coeffs();
    let (sx, sy) = (c[0].hypot(c[1]), c[2].hypot(c[3]));
    if !(sx.is_finite() && sy.is_finite() && sx > 0.0 && sy > 0.0) {
        return Err(Code::UnsupportedFeature);
    }
    let local = [sx, 0.0, 0.0, sy];
    let reach = filter::device_reach(functions, local)
        .into_iter()
        .max()
        .unwrap_or(0);
    if reach > i64::from(filter::MAX_MARGIN) {
        return Err(Code::BudgetExceeded);
    }
    let (wf, hf) = (
        (f64::from(clip.width) * sx).ceil(),
        (f64::from(clip.height) * sy).ceil(),
    );
    if !wf.is_finite() || !hf.is_finite() || wf > 8_500_000.0 || hf > 8_500_000.0 {
        return Err(Code::BudgetExceeded);
    }
    let (cw, ch) = (wf as usize, hf as usize);
    if cw == 0 || ch == 0 {
        return Ok(());
    }
    let pad = reach as usize;
    // Keep one filtered pixel outside each edge for bilinear sampling.
    let edge = usize::from(pad > 0);
    let (ow, oh) = (cw + 2 * edge, ch + 2 * edge);
    let pw = ow + 2 * pad;
    let reflect = |v: isize, n: usize| -> usize {
        let k = v.rem_euclid((2 * n) as isize) as usize;
        if k < n { k } else { 2 * n - 1 - k }
    };
    let sample = |data: &[u8], w: usize, h: usize, x: f64, y: f64, channel: usize| -> u8 {
        let x = x.clamp(0.0, (w - 1) as f64);
        let y = y.clamp(0.0, (h - 1) as f64);
        let (ax, ay) = (x.floor() as usize, y.floor() as usize);
        let (bx, by) = ((ax + 1).min(w - 1), (ay + 1).min(h - 1));
        let (fx, fy) = (x - ax as f64, y - ay as f64);
        let a = f64::from(data[(ay * w + ax) * 4 + channel]);
        let b = f64::from(data[(ay * w + bx) * 4 + channel]);
        let c = f64::from(data[(by * w + ax) * 4 + channel]);
        let d = f64::from(data[(by * w + bx) * 4 + channel]);
        ((a * (1.0 - fx) + b * fx) * (1.0 - fy) + (c * (1.0 - fx) + d * fx) * fy).round() as u8
    };
    // Preserve the single-surface path whenever it fits; tile only when needed.
    let (single_w, single_h) = (cw + 2 * pad, ch + 2 * pad);
    if (single_w as u64) * (single_h as u64) <= 8_500_000
        && (single_w as u64) * (single_h as u64) <= max_work / 72
    {
        let (pw, ph) = (single_w, single_h);
        let Some(mut work) = zeroed(pw * ph * 4) else {
            return Err(Code::OutOfMemory);
        };
        for y in 0..ph {
            let ly = f64::from(clip.y)
                + (reflect(y as isize - pad as isize, ch) as f64 + 0.5) * f64::from(clip.height)
                    / ch as f64;
            for x in 0..pw {
                let lx = f64::from(clip.x)
                    + (reflect(x as isize - pad as isize, cw) as f64 + 0.5) * f64::from(clip.width)
                        / cw as f64;
                let q = tf * vello_cpu::kurbo::Point::new(lx, ly);
                let dst = (y * pw + x) * 4;
                for channel in 0..4 {
                    work[dst + channel] =
                        sample(pixels, width, height, q.x - 0.5, q.y - 0.5, channel);
                }
            }
        }
        if !filter::apply(&mut work, pw, ph, functions, local) {
            return Err(Code::OutOfMemory);
        }
        let inverse = tf.inverse();
        let (x0, y0, x1, y1) = (
            bounds.x0.floor() as usize,
            bounds.y0.floor() as usize,
            bounds.x1.ceil() as usize,
            bounds.y1.ceil() as usize,
        );
        for y in y0..y1 {
            for x in x0..x1 {
                let q = inverse * vello_cpu::kurbo::Point::new(x as f64 + 0.5, y as f64 + 0.5);
                let lx = (q.x - f64::from(clip.x)) * cw as f64 / f64::from(clip.width) - 0.5
                    + pad as f64;
                let ly = (q.y - f64::from(clip.y)) * ch as f64 / f64::from(clip.height) - 0.5
                    + pad as f64;
                let dst = (y * width + x) * 4;
                for channel in 0..4 {
                    pixels[dst + channel] = sample(&work, pw, ph, lx, ly, channel);
                }
            }
        }
        return Ok(());
    }
    let output_bytes = (ow as u64) * (oh as u64) * 4;
    let work_pixels = max_work.saturating_sub(output_bytes) / 72;
    let max_rows = (work_pixels / pw as u64).min(8_500_000 / pw as u64) as usize;
    if max_rows <= 2 * pad {
        return Err(Code::BudgetExceeded);
    }
    let rows = (max_rows - 2 * pad).min(oh);
    let Some(mut output) = zeroed(ow * oh * 4) else {
        return Err(Code::OutOfMemory);
    };
    let mut top = 0;
    while top < oh {
        let count = rows.min(oh - top);
        let ph = count + 2 * pad;
        let Some(mut work) = zeroed(pw * ph * 4) else {
            return Err(Code::OutOfMemory);
        };
        for y in 0..ph {
            let ly = f64::from(clip.y)
                + (reflect((top + y) as isize - pad as isize - edge as isize, ch) as f64 + 0.5)
                    * f64::from(clip.height)
                    / ch as f64;
            for x in 0..pw {
                let lx = f64::from(clip.x)
                    + (reflect(x as isize - pad as isize - edge as isize, cw) as f64 + 0.5)
                        * f64::from(clip.width)
                        / cw as f64;
                let q = tf * vello_cpu::kurbo::Point::new(lx, ly);
                let dst = (y * pw + x) * 4;
                for channel in 0..4 {
                    work[dst + channel] =
                        sample(pixels, width, height, q.x - 0.5, q.y - 0.5, channel);
                }
            }
        }
        if !filter::apply(&mut work, pw, ph, functions, local) {
            return Err(Code::OutOfMemory);
        }
        for y in 0..count {
            let src = ((y + pad) * pw + pad) * 4;
            let dst = (top + y) * ow * 4;
            output[dst..dst + ow * 4].copy_from_slice(&work[src..src + ow * 4]);
        }
        top += count;
    }
    let inverse = tf.inverse();
    let (x0, y0, x1, y1) = (
        bounds.x0.floor() as usize,
        bounds.y0.floor() as usize,
        bounds.x1.ceil() as usize,
        bounds.y1.ceil() as usize,
    );
    for y in y0..y1 {
        for x in x0..x1 {
            let q = inverse * vello_cpu::kurbo::Point::new(x as f64 + 0.5, y as f64 + 0.5);
            let lx =
                (q.x - f64::from(clip.x)) * cw as f64 / f64::from(clip.width) - 0.5 + edge as f64;
            let ly =
                (q.y - f64::from(clip.y)) * ch as f64 / f64::from(clip.height) - 0.5 + edge as f64;
            let dst = (y * width + x) * 4;
            for channel in 0..4 {
                pixels[dst + channel] = sample(&output, ow, oh, lx, ly, channel);
            }
        }
    }
    Ok(())
}

/// Close the innermost filter layer: run the scope's functions over its pixels and draw the result into the parent layer.
fn close_filter(
    layers: &mut Layers<'_>,
    effect: &crate::scene::display_list::FilterEffect,
    tf: Affine,
    resources: &mut Resources,
    already_live: Option<u64>,
) -> Result<(), Code> {
    let (w, h) = layers.size();
    // Filter output, two RGBA f32 regions, and worst-case line scratch must fit
    // alongside every open parent layer and replay.
    let scratch = u64::from(w) * u64::from(h) * 72;
    let remaining = already_live.map(|live| {
        (512u64 * 1024 * 1024).saturating_sub(live.saturating_add(layers.memory_estimate()))
    });
    let input_bytes = u64::from(w) * u64::from(h) * 4;
    if remaining.is_some_and(|available| available < input_bytes) {
        return Err(Code::BudgetExceeded);
    }
    let margin = f64::from(layers.open.last().copied().unwrap_or(0));
    let Some(mut buf) = zeroed(w as usize * h as usize * 4) else {
        return Err(Code::OutOfMemory);
    };
    {
        let layer = layers.current();
        layer.flush();
        if let Some(target) = PixmapMut::new(w, h, &mut buf) {
            layer.render_with(
                target,
                resources,
                RasterizerSettings {
                    render_mode: raster_quality_mode(),
                    target_init: TargetInit::Clear(AlphaColor::TRANSPARENT),
                    pixel_format: vello_cpu::PixelFormat::Rgba8,
                    offset: (0, 0),
                },
            );
        }
    }
    if let Some(available) = remaining
        && scratch > available
    {
        filter::apply_tiled(
            &mut buf,
            w as usize,
            h as usize,
            &effect.functions,
            linear(tf),
            available - input_bytes,
        )
        .map_err(|e| match e {
            filter::TiledFailure::Budget => Code::BudgetExceeded,
            filter::TiledFailure::Memory => Code::OutOfMemory,
        })?;
    } else if !filter::apply(
        &mut buf,
        w as usize,
        h as usize,
        &effect.functions,
        linear(tf),
    ) {
        return Err(Code::OutOfMemory);
    }
    layers.open.pop();
    let pixmap = Arc::new(Pixmap::from_parts(
        buf,
        w,
        h,
        PixelMetadata::new(peniko::ImageAlphaType::AlphaPremultiplied, true),
    ));
    let parent = layers.current();
    parent.set_transform(Affine::translate((-margin, -margin)));
    parent.reset_paint_transform();
    parent.set_paint(peniko::Brush::Image(vello_cpu::Image {
        image: ImageSource::Pixmap(pixmap),
        sampler: peniko::ImageSampler {
            x_extend: peniko::Extend::Pad,
            y_extend: peniko::Extend::Pad,
            quality: peniko::ImageQuality::Low,
            alpha: 1.0,
        },
    }));
    parent.fill_rect(&KRect::new(0.0, 0.0, f64::from(w), f64::from(h)));
    Ok(())
}

/// Close and open scopes so that exactly the chain of `target` is open.
fn sync_scopes(
    layers: &mut Layers<'_>,
    list: &DisplayList,
    scopes: &mut Scopes,
    target: Option<ScopeId>,
    masker: &mut Masker<'_>,
    already_live: Option<u64>,
) -> Result<(), Code> {
    let chain = list.scope_chain(target);
    let common = scopes
        .stack
        .iter()
        .zip(&chain)
        .take_while(|(open, id)| open.0 == **id)
        .count();
    while scopes.stack.len() > common {
        match scopes.stack.pop() {
            Some((_, Open::Clip, _)) => layers.current().pop_clip(),
            Some((_, Open::Effect, _)) => layers.current().pop_layer(),
            Some((id, Open::Filter, tf)) => {
                if let ScopeKind::Filter(effect) = &list.scopes[id.0 as usize].kind {
                    close_filter(layers, effect, tf, masker.resources, already_live)?;
                }
            }
            Some((_, Open::Spatial, _)) | None => {}
        }
    }
    for id in &chain[common..] {
        let cur = scopes.transform();
        if let ScopeKind::Filter(effect) = &list.scopes[id.0 as usize].kind {
            let margin = filter::margin(&effect.functions, linear(cur));
            if !layers.push(margin, already_live) {
                return Err(Code::BudgetExceeded);
            }
            // The layer's origin is `margin` device px up and left of the surface's: everything drawn in it shifts.
            scopes.stack.push((
                *id,
                Open::Filter,
                Affine::translate((f64::from(margin), f64::from(margin))) * cur,
            ));
            continue;
        }
        match &list.scopes[id.0 as usize].kind {
            ScopeKind::Spatial(a) => {
                let m = cur
                    * Affine::new([
                        a.a,
                        a.b,
                        a.c,
                        a.d,
                        clamp_coord(a.e as f32),
                        clamp_coord(a.f as f32),
                    ]);
                scopes.stack.push((*id, Open::Spatial, m));
            }
            ScopeKind::Clip(shape) => {
                let ctx = layers.current();
                ctx.set_transform(cur);
                ctx.reset_paint_transform();
                match shape {
                    ClipShape::Rect(r) => ctx.push_clip_rect(&krect(r)),
                    ClipShape::RoundedRect(rr) => {
                        ctx.set_fill_rule(Fill::NonZero);
                        ctx.push_clip_path(&bez(&rrect_path(rr)));
                    }
                    ClipShape::Path {
                        path,
                        fill_rule: rule,
                    } => {
                        ctx.set_fill_rule(fill_rule(*rule));
                        ctx.push_clip_path(&bez(path));
                    }
                }
                scopes.stack.push((*id, Open::Clip, cur));
            }
            ScopeKind::Blend(mode) => {
                use crate::scene::display_list::BlendMode as B;
                let mix = match mode {
                    B::Normal => peniko::Mix::Normal,
                    B::Multiply => peniko::Mix::Multiply,
                    B::Screen => peniko::Mix::Screen,
                    B::Overlay => peniko::Mix::Overlay,
                    B::Darken => peniko::Mix::Darken,
                    B::Lighten => peniko::Mix::Lighten,
                    B::ColorDodge => peniko::Mix::ColorDodge,
                    B::ColorBurn => peniko::Mix::ColorBurn,
                    B::HardLight => peniko::Mix::HardLight,
                    B::SoftLight => peniko::Mix::SoftLight,
                    B::Difference => peniko::Mix::Difference,
                    B::Exclusion => peniko::Mix::Exclusion,
                    B::Hue => peniko::Mix::Hue,
                    B::Saturation => peniko::Mix::Saturation,
                    B::Color => peniko::Mix::Color,
                    B::Luminosity => peniko::Mix::Luminosity,
                };
                let ctx = layers.current();
                ctx.set_transform(cur);
                ctx.push_layer(None, Some(mix.into()), None, None, None);
                scopes.stack.push((*id, Open::Effect, cur));
            }
            ScopeKind::Effect(e) => {
                let ctx = layers.current();
                ctx.set_transform(cur);
                ctx.push_layer(None, None, Some(e.opacity), None, None);
                scopes.stack.push((*id, Open::Effect, cur));
            }
            ScopeKind::Mask(m) => {
                let mask = masker.build(m, cur);
                let ctx = layers.current();
                ctx.set_transform(cur);
                ctx.push_mask_layer(mask);
                scopes.stack.push((*id, Open::Effect, cur));
            }
            ScopeKind::Filter(_) => {}
        }
    }
    Ok(())
}

struct Shadow<'a> {
    border: &'a RoundedRect,
    padding: &'a RoundedRect,
    offset: (f32, f32),
    blur: f32,
    spread: f32,
    color: Color,
}

fn unsupported(diagnostics: &mut Vec<Status>, id: u32, what: &str) {
    diagnostics.push(
        Status::new(Code::UnsupportedFeature, Stage::Raster)
            .with_message(format!("item {id}: {what}")),
    );
}

/// The radius of a uniform circular-cornered shape (`Some(0)` for a square one).
fn uniform_radius(rr: &RoundedRect) -> Option<f32> {
    let r = rr.radii[0];
    let same = rr
        .radii
        .iter()
        .all(|c| (c.0 - r.0).abs() < 1e-4 && (c.1 - r.1).abs() < 1e-4);
    (same && (r.0 - r.1).abs() < 1e-4).then_some(r.0)
}

/// The shadow shape: the border rrect moved by `offset` and grown by `spread` (a corner's radius
/// grows with the spread unless it is square; CSS Backgrounds 3 section 7.2).
fn spread_shape(border: &RoundedRect, offset: (f32, f32), spread: f32) -> RoundedRect {
    let r = &border.rect;
    let rect = Rect {
        x: r.x + offset.0 - spread,
        y: r.y + offset.1 - spread,
        width: (r.width + 2.0 * spread).max(0.0),
        height: (r.height + 2.0 * spread).max(0.0),
    };
    let grow = |(rx, ry): (f32, f32)| {
        if rx <= 0.0 || ry <= 0.0 {
            (0.0, 0.0)
        } else {
            ((rx + spread).max(0.0), (ry + spread).max(0.0))
        }
    };
    RoundedRect {
        rect,
        radii: border.radii.map(grow),
    }
}

fn big_rect() -> KRect {
    KRect::new(-1.0e6, -1.0e6, 1.0e6, 1.0e6)
}

fn outer_shadow(ctx: &mut RenderContext, sh: &Shadow<'_>, id: u32, diagnostics: &mut Vec<Status>) {
    let shape = spread_shape(sh.border, sh.offset, sh.spread);
    if shape.rect.width <= 0.0 || shape.rect.height <= 0.0 {
        return;
    }
    // Clip to the outside of the border box: everything minus the border rrect.
    let mut outside = BezPath::new();
    outside.extend(big_rect().path_elements(0.1));
    outside.extend(bez(&rrect_path(sh.border)).elements().iter().copied());
    let sigma = sh.blur / 2.0;
    if sigma > 0.0 && uniform_radius(&shape).is_none() {
        // Elliptical or per-corner radii: a Gaussian blur layer over the shape. The outside-the-
        // border-box clip is the layer's own clip path: a clip *stack* entry around a filter
        // layer is evaluated in the layer's shifted source space in vello_cpu 0.3.0 and clips
        // everything away (X-VELLO-PIN finding).
        ctx.set_fill_rule(Fill::EvenOdd);
        ctx.push_layer(Some(&outside), None, None, None, Some(peniko_filter(sigma)));
        ctx.set_fill_rule(Fill::NonZero);
        ctx.set_paint(alpha_color(&sh.color));
        ctx.fill_path(&bez(&rrect_path(&shape)));
        ctx.pop_layer();
        return;
    }
    ctx.set_fill_rule(Fill::EvenOdd);
    ctx.push_clip_path(&outside);
    ctx.set_paint(alpha_color(&sh.color));
    ctx.set_fill_rule(Fill::NonZero);
    match uniform_radius(&shape) {
        _ if sigma <= 0.0 => ctx.fill_path(&bez(&rrect_path(&shape))),
        Some(radius) => ctx.fill_blurred_rounded_rect(&krect(&shape.rect), radius, sigma, false),
        None => {}
    }
    ctx.pop_clip();
    let _ = (id, &diagnostics);
}

fn inset_shadow(ctx: &mut RenderContext, sh: &Shadow<'_>, id: u32, diagnostics: &mut Vec<Status>) {
    // The shadow is inside the padding box: clip to it; the "hole" is the padding box moved by
    // the offset and shrunk by the spread, and the shadow colour fills everything outside it,
    // fading across the blurred edge.
    let hole = {
        let r = &sh.padding.rect;
        let rect = Rect {
            x: r.x + sh.offset.0 + sh.spread,
            y: r.y + sh.offset.1 + sh.spread,
            width: (r.width - 2.0 * sh.spread).max(0.0),
            height: (r.height - 2.0 * sh.spread).max(0.0),
        };
        let shrink = |(rx, ry): (f32, f32)| {
            if rx <= 0.0 || ry <= 0.0 {
                (0.0, 0.0)
            } else {
                ((rx - sh.spread).max(0.0), (ry - sh.spread).max(0.0))
            }
        };
        RoundedRect {
            rect,
            radii: sh.padding.radii.map(shrink),
        }
    };
    let sigma = sh.blur / 2.0;
    if sigma > 0.0
        && hole.rect.width > 0.0
        && hole.rect.height > 0.0
        && uniform_radius(&hole).is_none()
    {
        // Blur the complement of the exact elliptical/per-corner hole, with a
        // finite outer edge beyond the Gaussian reach. Put the padding clip on
        // the filtered layer itself (the same shifted-space rule as outer shadows).
        let pad = &sh.padding.rect;
        let margin = sigma * 4.0 + 2.0;
        let exterior = KRect::new(
            f64::from(pad.x.min(hole.rect.x) - margin),
            f64::from(pad.y.min(hole.rect.y) - margin),
            f64::from(pad.right().max(hole.rect.right()) + margin),
            f64::from(pad.bottom().max(hole.rect.bottom()) + margin),
        );
        let mut ring = BezPath::new();
        ring.extend(exterior.path_elements(0.1));
        ring.extend(bez(&rrect_path(&hole)).elements().iter().copied());
        ctx.set_fill_rule(Fill::NonZero);
        ctx.push_layer(
            Some(&bez(&rrect_path(sh.padding))),
            None,
            None,
            None,
            Some(peniko_filter(sigma)),
        );
        ctx.set_fill_rule(Fill::EvenOdd);
        ctx.set_paint(alpha_color(&sh.color));
        ctx.fill_path(&ring);
        ctx.pop_layer();
        return;
    }
    ctx.set_fill_rule(Fill::NonZero);
    ctx.push_clip_path(&bez(&rrect_path(sh.padding)));
    ctx.set_paint(alpha_color(&sh.color));
    if hole.rect.width <= 0.0 || hole.rect.height <= 0.0 {
        ctx.fill_rect(&krect(&sh.padding.rect));
    } else if sigma <= 0.0 {
        // Hard edge: the padding box minus the hole.
        let mut ring = BezPath::new();
        ring.extend(krect(&sh.padding.rect).path_elements(0.1));
        ring.extend(bez(&rrect_path(&hole)).elements().iter().copied());
        ctx.set_fill_rule(Fill::EvenOdd);
        ctx.fill_path(&ring);
    } else if let Some(radius) = uniform_radius(&hole) {
        let grow = 2.5 * sigma + 1.0;
        let inflated = Rect {
            x: hole.rect.x - grow,
            y: hole.rect.y - grow,
            width: hole.rect.width + 2.0 * grow,
            height: hole.rect.height + 2.0 * grow,
        };
        // Opaque colour beyond the blur's reach, the inverted blur inside it.
        let mut ring = BezPath::new();
        ring.extend(krect(&sh.padding.rect).path_elements(0.1));
        ring.extend(krect(&inflated).path_elements(0.1));
        ctx.set_fill_rule(Fill::EvenOdd);
        ctx.fill_path(&ring);
        ctx.fill_blurred_rounded_rect(&krect(&hole.rect), radius, sigma, true);
    }
    ctx.pop_clip();
    let _ = (id, diagnostics);
}

fn peniko_filter(sigma: f32) -> vello_cpu::filter_effects::Filter {
    vello_cpu::filter_effects::Filter::from_function(
        vello_cpu::filter_effects::FilterFunction::Blur { radius: sigma },
    )
}

/// The horizontal shear of a synthetic italic or oblique face (`degrees` from the font's
/// synthesis; 0 = none): glyph space is y-up, so the top of the glyph moves right.
fn synthetic_skew(degrees: f32) -> Option<Affine> {
    (degrees != 0.0).then(|| Affine::skew(-f64::from(degrees).to_radians().tan(), 0.0))
}

/// The outline growth of a synthetic bold face, in px (total stroke width): the fake-bold amount
/// of the reference browser's rasteriser, 1/24 of the font size up to 9 px falling linearly to
/// 1/32 from 36 px on.
fn fake_bold_extra(size_px: f32) -> f32 {
    let t = ((size_px - 9.0) / (36.0 - 9.0)).clamp(0.0, 1.0);
    size_px * (1.0 / 24.0 + t * (1.0 / 32.0 - 1.0 / 24.0))
}

#[cfg(test)]
mod replay_budget_tests {
    #[test]
    fn glyph_origins_snap_only_on_one_x_translation() {
        use super::{Affine, glyph_origin_adjustment};
        let translation = Affine::translate((0.125, 0.25));
        assert_eq!(
            glyph_origin_adjustment(Some((10.125, 20.125)), translation, 1.0, (0.0, 0.0)),
            (0.0, -0.375)
        );
        assert_eq!(
            glyph_origin_adjustment(Some((10.125, 20.125)), translation, 2.0, (0.0, 0.0)),
            (0.0, 0.0)
        );
        assert_eq!(
            glyph_origin_adjustment(Some((10.125, 20.125)), Affine::scale(2.0), 1.0, (0.0, 0.0)),
            (0.0, 0.0)
        );
        assert_eq!(
            glyph_origin_adjustment(None, translation, 1.0, (0.0, 0.0)),
            (0.0, 0.0)
        );
        // A baseline exactly half a pixel above zero rounds toward positive infinity.
        assert_eq!(
            glyph_origin_adjustment(Some((0.0, -0.5)), Affine::IDENTITY, 1.0, (0.0, 0.0)),
            (0.0, 0.5)
        );
        // A backdrop replay has an offset local surface but uses the original device grid.
        assert_eq!(
            glyph_origin_adjustment(
                Some((10.125, 20.125)),
                Affine::translate((-0.375, -0.25)),
                1.0,
                (0.5, 0.5)
            ),
            (0.0, -0.375)
        );
    }

    use super::*;
    use crate::scene::display_list::{FilterEffect, FilterFn, Scope};

    #[test]
    fn tiled_backdrop_blur_matches_single_surface_across_seams() {
        let (w, h) = (40usize, 60usize);
        let mut full = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let alpha = if (15..45).contains(&y) { 128 } else { 0 };
                full.extend_from_slice(&[
                    ((x * 3) as u8).min(alpha),
                    ((y * 2) as u8).min(alpha),
                    ((x + y) as u8).min(alpha),
                    alpha,
                ]);
            }
        }
        let bounds = KRect::new(0.0, 0.0, w as f64, h as f64);
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        };
        let cases = [
            (vec![FilterFn::Blur(0.5)], 100_000),
            (vec![FilterFn::Blur(4.0)], 300_000),
            (
                vec![
                    FilterFn::Blur(2.0),
                    FilterFn::DropShadow {
                        dx: 1.0,
                        dy: -1.0,
                        blur: 1.0,
                        color: crate::scene::display_list::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.7,
                        },
                    },
                ],
                300_000,
            ),
        ];
        for (functions, budget) in cases {
            let mut reference = full.clone();
            let mut tiled = full.clone();
            filter_backdrop(
                &mut reference,
                w,
                h,
                bounds,
                &clip,
                &functions,
                Affine::IDENTITY,
                10_000_000,
            )
            .unwrap();
            filter_backdrop(
                &mut tiled,
                w,
                h,
                bounds,
                &clip,
                &functions,
                Affine::IDENTITY,
                budget,
            )
            .unwrap();
            let max_difference = tiled
                .iter()
                .zip(&reference)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                max_difference <= 1,
                "{functions:?}: maximum channel difference {max_difference}"
            );
        }
    }

    #[test]
    fn tiled_rotated_backdrop_matches_single_surface() {
        let (w, h) = (60usize, 60usize);
        let mut source = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                source.extend_from_slice(&[(x * 4) as u8, (y * 4) as u8, 128, 255]);
            }
        }
        let mut full = source.clone();
        let mut tiled = source;
        let bounds = KRect::new(0.0, 0.0, w as f64, h as f64);
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 30.0,
            height: 40.0,
        };
        let tf = Affine::translate((15.0, 5.0)) * Affine::rotate(0.2);
        let functions = [FilterFn::Blur(2.0)];
        filter_transformed_backdrop(&mut full, w, h, bounds, &clip, &functions, tf, 10_000_000)
            .unwrap();
        filter_transformed_backdrop(&mut tiled, w, h, bounds, &clip, &functions, tf, 100_000)
            .unwrap();
        let mut max_difference = 0;
        for y in 0..h {
            for x in 0..w {
                let q = tf.inverse() * vello_cpu::kurbo::Point::new(x as f64 + 0.5, y as f64 + 0.5);
                if q.x >= -1.0 && q.x < 31.0 && q.y >= -1.0 && q.y < 41.0 {
                    for c in 0..4 {
                        max_difference = max_difference
                            .max(tiled[(y * w + x) * 4 + c].abs_diff(full[(y * w + x) * 4 + c]));
                    }
                }
            }
        }
        assert!(
            max_difference <= 1,
            "maximum channel difference {max_difference}"
        );
    }

    #[test]
    fn tiled_skewed_backdrop_matches_single_surface() {
        let (w, h) = (60usize, 60usize);
        let mut source = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                source.extend_from_slice(&[(x * 4) as u8, (y * 4) as u8, 128, 255]);
            }
        }
        let mut full = source.clone();
        let mut tiled = source;
        let bounds = KRect::new(0.0, 0.0, w as f64, h as f64);
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 50.0,
        };
        let tf = Affine::new([1.0, 0.0, 0.3, 1.0, 0.0, 0.0]);
        let functions = [FilterFn::Blur(2.0)];
        filter_skewed_backdrop(&mut full, w, h, bounds, &clip, &functions, tf, 10_000_000).unwrap();
        filter_skewed_backdrop(&mut tiled, w, h, bounds, &clip, &functions, tf, 200_000).unwrap();
        let max_difference = tiled
            .iter()
            .zip(&full)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            max_difference <= 1,
            "maximum channel difference {max_difference}"
        );
    }

    #[test]
    fn layer_reservation_accounts_for_parent_replays() {
        let mut ctx = RenderContext::new(1, 1);
        let mut pool = Vec::new();
        let mut layers = Layers {
            main: &mut ctx,
            pool: &mut pool,
            open: Vec::new(),
            surface: (100, 100),
        };
        assert!(!layers.push(0, Some(512 * 1024 * 1024)));
        assert!(layers.open.is_empty());
        assert!(layers.push(0, Some(0)));
        layers.open.clear();
        assert_eq!(
            layers.memory_estimate(),
            100 * 100 * (32 + 16),
            "closed pool context remains accounted"
        );
    }

    #[test]
    fn clone_estimate_includes_scope_and_filter_payloads() {
        let empty = DisplayList::default();
        let base = replay_clone_estimate(&empty, 0, 1, 1);
        let list = DisplayList {
            items: vec![],
            scopes: vec![Scope {
                parent: None,
                kind: ScopeKind::Filter(FilterEffect {
                    functions: vec![FilterFn::Blur(1.0); 100],
                }),
            }],
        };
        let estimate = replay_clone_estimate(&list, 0, 1, 1);
        assert!(
            estimate
                >= base
                    + 2 * (std::mem::size_of::<Scope>() + 100 * std::mem::size_of::<FilterFn>())
                        as u64
        );
        assert!(replay_clone_estimate(&empty, 0, 2, 1) > base);
    }
}
