//! F-14 (INPUTSEC-03): a bound on the SVG filter work of one rasterisation.
//!
//! resvg runs filter primitives synchronously, and an SVG image is rasterised on the owner (GUI)
//! thread at paint time (`raster_cache`). The cost of a primitive is its filter region in device
//! pixels times a per-pixel cost that some primitives multiply by an author-controlled factor
//! (`feTurbulence numOctaves`, the `feMorphology` radius, the `feConvolveMatrix` order), and a
//! chain may hold any number of primitives: a 300x300 `feTurbulence numOctaves="10000"` took 47 s.
//!
//! [`filter_work`] estimates the work of drawing a tree at a device transform, in nanoseconds of a
//! release build on the reference machine (the weights below, measured with
//! `tests/svg_filter_cost.rs`, rounded up). It walks every filtered group with the transform resvg
//! uses for it, including the content that clip paths, masks, patterns, `feImage` and nested SVG
//! images pull in, and bounds each filter region the way resvg does (the group's layer box, at most
//! five times the canvas per side). The raster cache compares it with
//! [`crate::policy::SVG_FILTER_WORK_BUDGET`] and draws the image without its filters when it is over
//! (`usvg_adapter`).

use usvg::Transform;
use usvg::filter::Kind;

/// Per-pixel cost of every primitive (allocation, colour-space conversion, region crop), ns.
const BASE_NS: u64 = 12;
/// `feGaussianBlur` with a standard deviation under 2 device px (resvg's IIR blur), per pixel.
const IIR_BLUR_NS: u64 = 220;
/// `feGaussianBlur` otherwise (three box blurs per axis, independent of the radius).
const BOX_BLUR_NS: u64 = 45;
/// `feTurbulence`, per pixel and octave.
const TURBULENCE_NS: u64 = 45;
/// `feMorphology`, per pixel and kernel pixel.
const MORPHOLOGY_NS: f64 = 0.3;
/// `feConvolveMatrix`, per pixel and kernel cell.
const CONVOLVE_NS: f64 = 4.0;
/// `feDiffuseLighting` / `feSpecularLighting`, per pixel.
const LIGHTING_NS: u64 = 60;
/// `feDisplacementMap`, per pixel.
const DISPLACEMENT_NS: u64 = 25;
/// Every other primitive (blend, colour matrix, component transfer, composite, flood, merge input,
/// offset, tile, image), per pixel.
const SIMPLE_NS: u64 = 20;

/// What one primitive costs per region pixel at device scale `(sx, sy)`, the region being
/// `w` x `h` device pixels.
fn per_pixel(kind: &Kind, (sx, sy): (f32, f32), (w, h): (f64, f64)) -> f64 {
    let blur = |dx: f32, dy: f32| {
        let (dx, dy) = (dx * sx, dy * sy);
        if dx <= 0.0 && dy <= 0.0 {
            0
        } else if dx < 2.0 && dy < 2.0 {
            IIR_BLUR_NS
        } else {
            BOX_BLUR_NS
        }
    };
    let ns = match kind {
        Kind::GaussianBlur(b) => blur(b.std_dev_x().get(), b.std_dev_y().get()) as f64,
        Kind::DropShadow(d) => {
            (blur(d.std_dev_x().get(), d.std_dev_y().get()) + 3 * SIMPLE_NS) as f64
        }
        Kind::Turbulence(t) => TURBULENCE_NS as f64 * f64::from(t.num_octaves()),
        Kind::Morphology(m) => {
            // resvg clamps the radius to the region.
            let rx = (f64::from(m.radius_x().get() * sx)).round().clamp(0.0, w);
            let ry = (f64::from(m.radius_y().get() * sy)).round().clamp(0.0, h);
            MORPHOLOGY_NS * (2.0 * rx + 1.0) * (2.0 * ry + 1.0)
        }
        Kind::ConvolveMatrix(c) => {
            CONVOLVE_NS * f64::from(c.matrix().rows()) * f64::from(c.matrix().columns())
        }
        Kind::DiffuseLighting(_) | Kind::SpecularLighting(_) => LIGHTING_NS as f64,
        Kind::DisplacementMap(_) => DISPLACEMENT_NS as f64,
        Kind::Merge(m) => (SIMPLE_NS as f64) * m.inputs().len().max(1) as f64,
        Kind::Blend(_)
        | Kind::ColorMatrix(_)
        | Kind::ComponentTransfer(_)
        | Kind::Composite(_)
        | Kind::Flood(_)
        | Kind::Image(_)
        | Kind::Offset(_)
        | Kind::Tile(_) => SIMPLE_NS as f64,
    };
    BASE_NS as f64 + ns
}

/// Whether `tree` has a filter anywhere, including referenced and nested content.
pub fn has_filters(tree: &usvg::Tree) -> bool {
    let mut found = false;
    walk(tree.root(), Transform::identity(), &mut |g, _| {
        found |= !g.filters().is_empty();
    });
    found
}

/// Estimated filter work (ns) of drawing `tree` with `transform` into a `width` x `height` canvas.
pub fn filter_work(tree: &usvg::Tree, transform: Transform, width: u32, height: u32) -> u64 {
    // resvg fits a filter region into the canvas grown by two canvases on every side.
    let (max_w, max_h) = (f64::from(width) * 5.0, f64::from(height) * 5.0);
    let mut total = 0f64;
    walk(tree.root(), transform, &mut |g, base| {
        if g.filters().is_empty() {
            return;
        }
        let ts = base.pre_concat(g.abs_transform());
        let (w, h) = match g.layer_bounding_box().transform(ts) {
            Some(r) => (
                f64::from(r.width()).ceil().clamp(1.0, max_w),
                f64::from(r.height()).ceil().clamp(1.0, max_h),
            ),
            None => (max_w.max(1.0), max_h.max(1.0)),
        };
        let scale = ts.get_scale();
        for f in g.filters() {
            for p in f.primitives() {
                total += w * h * per_pixel(p.kind(), scale, (w, h));
            }
        }
    });
    if total.is_finite() {
        total.min(u64::MAX as f64) as u64
    } else {
        u64::MAX
    }
}

/// Visit every group drawn for `root`, with the transform from its tree's root space to the
/// device (`base`, to be combined with the group's `abs_transform`). Referenced content (clip
/// paths, masks, patterns, `feImage`, nested SVG images) is visited once per reference, as resvg
/// draws it; its groups' absolute transforms start at the referencing node. Iterative: it runs on
/// the GUI thread, and the pre-scan bounds the effective depth and the expanded element count.
fn walk(root: &usvg::Group, base: Transform, visit: &mut dyn FnMut(&usvg::Group, Transform)) {
    let mut stack: Vec<(&usvg::Group, Transform)> = vec![(root, base)];
    while let Some((g, base)) = stack.pop() {
        visit(g, base);
        let here = base.pre_concat(g.abs_transform());
        let mut clip = g.clip_path();
        while let Some(c) = clip {
            stack.push((c.root(), here));
            clip = c.clip_path();
        }
        let mut mask = g.mask();
        while let Some(m) = mask {
            stack.push((m.root(), here));
            mask = m.mask();
        }
        for f in g.filters() {
            for p in f.primitives() {
                if let Kind::Image(image) = p.kind() {
                    stack.push((image.root(), here));
                }
            }
        }
        for node in g.children() {
            match node {
                usvg::Node::Group(child) => stack.push((child, base)),
                usvg::Node::Path(path) => {
                    let at = base.pre_concat(path.abs_transform());
                    for paint in [
                        path.fill().map(|f| f.paint()),
                        path.stroke().map(|s| s.paint()),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if let usvg::Paint::Pattern(p) = paint {
                            stack.push((p.root(), at.pre_concat(p.transform())));
                        }
                    }
                }
                usvg::Node::Image(image) => {
                    if let usvg::ImageKind::SVG(sub) = image.kind() {
                        // A nested document is drawn into the image's view box.
                        let size = sub.size();
                        let fit = Transform::from_scale(
                            image.size().width() / size.width(),
                            image.size().height() / size.height(),
                        );
                        stack.push((
                            sub.root(),
                            base.pre_concat(image.abs_transform()).pre_concat(fit),
                        ));
                    }
                }
                usvg::Node::Text(_) => {}
            }
        }
    }
}
