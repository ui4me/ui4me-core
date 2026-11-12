//! owner: 010-paint-render (lanes L010-r1, L-STY-r2) — pipeline-side projection of a
//! `ComputedStyleView`'s typed `paint`/`overflow` groups (006) onto the scene's `BoxExtras`:
//! radii, box shadows, overflow, transform (2-D), z-index, background layers (gradients and
//! `url()` images through the attached `ImageSource`) and the picture of an `<img>`.
//!
//! The temporary serialisation bridge of round 1 (D-L010r1-03) is gone. Whatever the scene cannot
//! paint faithfully is listed in `BoxExtras::unsupported` (reported `UNSUPPORTED_FEATURE`, stage
//! `scene`) and is not drawn: there is no look-alike.

use crate::scene::display_list::{Color, ImageSampling, MaskComposite};
use crate::scene::inputs::{
    BgBox, BgImage, BgLayerSpec, BgSize, BoxExtras, BoxShadowSpec, GradientSpec, Hint, ImageLookup,
    ImageSource, LenPct, LinearDirection, MaskLayerSpec, ObjectFit, RadialSize, Repeat, Replaced,
    ReplacedImage, StopPosition, StopSpec,
};
use crate::scene::inputs::{BiOutset, BiRepeat, BiSlice, BiWidth, BorderImageSpec};
use crate::style::view::ComputedStyleView;
use crate::style::view::groups as g;
use ui4me_document::document::Document;
use ui4me_engine_types::geometry::transform::{
    Affine2D, IndividualTransforms, RectF, TransformOp, flatten, resolve_transform,
};

fn color(c: &g::Color) -> Color {
    let [r, gr, b, a] = c.to_srgb_f32();
    Color { r, g: gr, b, a }
}

/// A computed length-percentage in linear form (`px + frac * basis`). Every computed `calc()`
/// with one percentage basis is linear, so two probes of the handle recover it exactly.
fn len_pct(v: &g::LengthPercentage) -> LenPct {
    match v {
        g::LengthPercentage::Px(p) => LenPct::px(*p),
        g::LengthPercentage::Percent(f) => LenPct::frac(*f),
        g::LengthPercentage::Calc(h) => {
            let zero = h.resolve(0.0);
            LenPct {
                px: zero,
                frac: h.resolve(1.0) - zero,
            }
        }
    }
}

fn stop_position(v: &g::LengthPercentage) -> StopPosition {
    match v {
        g::LengthPercentage::Px(p) => StopPosition::Length(*p),
        g::LengthPercentage::Percent(f) => StopPosition::Fraction(*f),
        g::LengthPercentage::Calc(_) => {
            let l = len_pct(v);
            StopPosition::Mixed {
                frac: l.frac,
                px: l.px,
            }
        }
    }
}

fn stops(items: &[g::GradientItem]) -> Vec<StopSpec> {
    let mut out: Vec<StopSpec> = Vec::new();
    let mut hint = None;
    for i in items {
        match i {
            g::GradientItem::Stop { color: c, position } => {
                out.push(StopSpec {
                    position: position.as_ref().map(stop_position),
                    color: color(c),
                    hint_before: hint.take(),
                });
            }
            g::GradientItem::Hint(p) => hint = Some(Hint::At(stop_position(p))),
        }
    }
    out
}

fn conic_position(p: g::ConicPosition) -> StopPosition {
    match p {
        g::ConicPosition::Degrees(d) => StopPosition::Fraction(d / 360.0),
        g::ConicPosition::Fraction(f) => StopPosition::Fraction(f),
    }
}

fn conic_stops(items: &[g::ConicItem]) -> Vec<StopSpec> {
    let mut out: Vec<StopSpec> = Vec::new();
    let mut hint = None;
    for i in items {
        match i {
            g::ConicItem::Stop { color: c, position } => out.push(StopSpec {
                position: position.map(conic_position),
                color: color(c),
                hint_before: hint.take(),
            }),
            g::ConicItem::Hint(p) => hint = Some(Hint::At(conic_position(*p))),
        }
    }
    out
}

fn extent(e: g::RadialExtent) -> RadialSize {
    match e {
        g::RadialExtent::ClosestSide => RadialSize::ClosestSide,
        g::RadialExtent::FarthestSide => RadialSize::FarthestSide,
        g::RadialExtent::ClosestCorner => RadialSize::ClosestCorner,
        g::RadialExtent::FarthestCorner => RadialSize::FarthestCorner,
    }
}

/// A typed gradient as the scene's spec; `Err` when it uses something the scene does not paint
/// (an interpolation space other than the legacy default).
fn gradient(gr: &g::Gradient) -> Result<GradientSpec, &'static str> {
    let default_only = |i: &g::GradientInterpolation| {
        if i.is_default {
            Ok(())
        } else {
            Err("css.property.background-image")
        }
    };
    Ok(match gr {
        g::Gradient::Linear {
            direction,
            items,
            repeating,
            interpolation,
        } => {
            default_only(interpolation)?;
            let direction = match *direction {
                g::LinearDirection::Angle(a) => LinearDirection::Angle(a),
                g::LinearDirection::Corner { x, y } => LinearDirection::ToCorner { x, y },
            };
            GradientSpec::Linear {
                direction,
                stops: stops(items),
                repeating: *repeating,
            }
        }
        g::Gradient::Radial {
            shape,
            position,
            items,
            repeating,
            interpolation,
        } => {
            default_only(interpolation)?;
            let (circle, size) = match shape {
                g::RadialShape::Circle(r) => (true, RadialSize::Explicit(*r, *r)),
                g::RadialShape::CircleExtent(e) => (true, extent(*e)),
                g::RadialShape::Ellipse(x, y) => {
                    (false, RadialSize::ExplicitEllipse(len_pct(x), len_pct(y)))
                }
                g::RadialShape::EllipseExtent(e) => (false, extent(*e)),
            };
            GradientSpec::Radial {
                circle,
                size,
                center: (len_pct(&position.0), len_pct(&position.1)),
                stops: stops(items),
                repeating: *repeating,
            }
        }
        g::Gradient::Conic {
            from_deg,
            position,
            items,
            repeating,
            interpolation,
        } => {
            default_only(interpolation)?;
            GradientSpec::Conic {
                from_deg: *from_deg,
                center: (len_pct(&position.0), len_pct(&position.1)),
                stops: conic_stops(items),
                repeating: *repeating,
            }
        }
    })
}

fn bg_box(b: g::BackgroundBox) -> Option<BgBox> {
    Some(match b {
        g::BackgroundBox::BorderBox => BgBox::BorderBox,
        g::BackgroundBox::PaddingBox => BgBox::PaddingBox,
        g::BackgroundBox::ContentBox => BgBox::ContentBox,
        g::BackgroundBox::Text | g::BackgroundBox::BorderArea | g::BackgroundBox::Other => {
            return None;
        }
    })
}

/// `background-clip` of a layer: the boxes of `bg_box` and `text` (the glyph outlines of the box's text).
fn bg_clip(b: g::BackgroundBox) -> Option<BgBox> {
    match b {
        g::BackgroundBox::Text => Some(BgBox::Text),
        other => bg_box(other),
    }
}

fn repeat(r: g::BackgroundRepeat) -> Repeat {
    match r {
        g::BackgroundRepeat::Repeat => Repeat::Repeat,
        g::BackgroundRepeat::NoRepeat => Repeat::NoRepeat,
        g::BackgroundRepeat::Space => Repeat::Space,
        g::BackgroundRepeat::Round => Repeat::Round,
    }
}

fn sampling(p: &g::Paint) -> ImageSampling {
    if p.image_rendering_pixelated {
        ImageSampling::Nearest
    } else {
        ImageSampling::Smooth
    }
}

/// The scene layers of `paint.background_layers`, topmost first. A layer the scene cannot paint
/// faithfully is not drawn and its property is added to `unsupported`.
fn layers(
    node: ui4me_engine_types::ids::NodeId,
    paint: &g::Paint,
    images: Option<&dyn ImageSource>,
    unsupported: &mut Vec<&'static str>,
) -> Vec<BgLayerSpec> {
    let mut out = Vec::new();
    let mut fail = |feature: &'static str| {
        if !unsupported.contains(&feature) {
            unsupported.push(feature);
        }
    };
    for (index, l) in paint.background_layers.iter().enumerate() {
        let image = match &l.image {
            g::Image::None => continue,
            g::Image::Unsupported(_) => {
                fail("css.property.background-image");
                continue;
            }
            g::Image::Gradient(gr) => match gradient(gr) {
                Ok(spec) => BgImage::Gradient(spec),
                Err(f) => {
                    fail(f);
                    continue;
                }
            },
            g::Image::Url(u) => match images {
                // No image provider is attached (the resource manager is not wired to this
                // pipeline): report instead of dropping the layer silently.
                None => {
                    fail("css.property.background-image");
                    continue;
                }
                Some(src) => {
                    match src.css_image(node, "background-image", index as u32, &u.text, u.resolved)
                    {
                        ImageLookup::Ready(data) => BgImage::Raster(data),
                        // Pending and broken images paint nothing (007 records the state).
                        ImageLookup::Pending | ImageLookup::Broken(_) => continue,
                    }
                }
            },
        };
        if l.attachment != g::BackgroundAttachment::Scroll {
            fail("css.property.background-attachment");
            continue;
        }
        let Some(blend_mode) = crate::scene::display_list::BlendMode::from_css(l.blend_mode) else {
            fail("css.property.background-blend-mode");
            continue;
        };
        let (Some(origin), Some(clip)) = (bg_box(l.origin), bg_clip(l.clip)) else {
            fail("css.property.background-clip");
            continue;
        };
        let axis = |a: &g::BackgroundSizeAxis| match a {
            g::BackgroundSizeAxis::Auto => None,
            g::BackgroundSizeAxis::LengthPercentage(v) => Some(len_pct(v)),
        };
        let size = match &l.size {
            g::BackgroundSize::Cover => BgSize::Cover,
            g::BackgroundSize::Contain => BgSize::Contain,
            g::BackgroundSize::Explicit { width, height } => {
                BgSize::Explicit(axis(width), axis(height))
            }
        };
        out.push(BgLayerSpec {
            image,
            blend_mode,
            origin,
            clip,
            size,
            position: (len_pct(&l.position.0), len_pct(&l.position.1)),
            repeat: (repeat(l.repeat.0), repeat(l.repeat.1)),
            sampling: sampling(paint),
        });
    }
    out
}

/// The scene layers of `paint.mask_layers`, topmost first (CSS Masking 1 section 7). A `none` layer is
/// skipped (all `none` = no mask); a layer the scene cannot paint faithfully (a gradient with a
/// non-default interpolation, `url(#fragment)` to an SVG `<mask>` element, an untyped image function) is
/// not applied and its property is added to `unsupported`; a pending image is not applied yet (007
/// repaints when it is ready); a failed image is a transparent black layer (section 7.1).
fn masks(
    node: ui4me_engine_types::ids::NodeId,
    paint: &g::Paint,
    images: Option<&dyn ImageSource>,
    unsupported: &mut Vec<&'static str>,
) -> Vec<MaskLayerSpec> {
    let mut out = Vec::new();
    let mut fail = |feature: &'static str| {
        if !unsupported.contains(&feature) {
            unsupported.push(feature);
        }
    };
    for (index, l) in paint.mask_layers.iter().enumerate() {
        let image = match &l.image {
            g::Image::None => continue,
            g::Image::Unsupported(_) => {
                fail("css.property.mask-image");
                continue;
            }
            g::Image::Gradient(gr) => match gradient(gr) {
                Ok(spec) => Some(BgImage::Gradient(spec)),
                Err(_) => {
                    fail("css.property.mask-image");
                    continue;
                }
            },
            g::Image::Url(u) if u.text.trim_start().starts_with('#') => {
                // A reference to an element of the document (an SVG `<mask>`): not supported.
                fail("css.property.mask-image");
                continue;
            }
            g::Image::Url(u) => match images {
                None => {
                    fail("css.property.mask-image");
                    continue;
                }
                Some(src) => {
                    match src.css_image(node, "mask-image", index as u32, &u.text, u.resolved) {
                        ImageLookup::Ready(data) => Some(BgImage::Raster(data)),
                        ImageLookup::Pending => continue,
                        ImageLookup::Broken(_) => None,
                    }
                }
            },
        };
        let (Some(origin), Some(clip)) = (bg_box(l.origin), bg_box(l.clip)) else {
            fail("css.property.mask-clip");
            continue;
        };
        let axis = |a: &g::BackgroundSizeAxis| match a {
            g::BackgroundSizeAxis::Auto => None,
            g::BackgroundSizeAxis::LengthPercentage(v) => Some(len_pct(v)),
        };
        let size = match &l.size {
            g::BackgroundSize::Cover => BgSize::Cover,
            g::BackgroundSize::Contain => BgSize::Contain,
            g::BackgroundSize::Explicit { width, height } => {
                BgSize::Explicit(axis(width), axis(height))
            }
        };
        let layer = image.map(|image| BgLayerSpec {
            image,
            blend_mode: Default::default(),
            origin,
            clip,
            size,
            position: (len_pct(&l.position.0), len_pct(&l.position.1)),
            repeat: (repeat(l.repeat.0), repeat(l.repeat.1)),
            sampling: sampling(paint),
        });
        out.push(MaskLayerSpec {
            layer,
            luminance: l.mode == g::MaskMode::Luminance,
            composite: match l.composite {
                g::MaskComposite::Add => MaskComposite::Add,
                g::MaskComposite::Subtract => MaskComposite::Subtract,
                g::MaskComposite::Intersect => MaskComposite::Intersect,
                g::MaskComposite::Exclude => MaskComposite::Exclude,
            },
        });
    }
    out
}

/// The `border-image` of an element as the scene's spec. `None` when there is none, or when the
/// source is pending, broken or not one the scene paints (CSS Backgrounds 3 section 6.1: an
/// invalid image is as if `none`, so the `border-style` borders paint); a source the scene cannot
/// paint is added to `unsupported`.
fn border_image(
    node: ui4me_engine_types::ids::NodeId,
    paint: &g::Paint,
    images: Option<&dyn ImageSource>,
    unsupported: &mut Vec<&'static str>,
) -> Option<BorderImageSpec> {
    let bi = paint.border_image.as_ref()?;
    let mut fail = |feature: &'static str| {
        if !unsupported.contains(&feature) {
            unsupported.push(feature);
        }
    };
    let source = match &bi.source {
        g::Image::None => return None,
        g::Image::Unsupported(_) => {
            fail("css.property.border-image-source");
            return None;
        }
        g::Image::Gradient(gr) => match gradient(gr) {
            Ok(spec) => BgImage::Gradient(spec),
            Err(_) => {
                fail("css.property.border-image-source");
                return None;
            }
        },
        g::Image::Url(u) => match images {
            None => {
                fail("css.property.border-image-source");
                return None;
            }
            Some(src) => match src.css_image(node, "border-image-source", 0, &u.text, u.resolved) {
                ImageLookup::Ready(data) => BgImage::Raster(data),
                ImageLookup::Pending | ImageLookup::Broken(_) => return None,
            },
        },
    };
    let slice = bi.slice.map(|s| match s {
        g::BorderImageSlice::Number(n) => BiSlice::Number(n),
        g::BorderImageSlice::Fraction(f) => BiSlice::Fraction(f),
    });
    let width = bi.width.clone().map(|w| match w {
        g::BorderImageWidth::Number(n) => BiWidth::Number(n),
        g::BorderImageWidth::LengthPercentage(l) => BiWidth::Len(len_pct(&l)),
        g::BorderImageWidth::Auto => BiWidth::Auto,
    });
    let outset = bi.outset.map(|o| match o {
        g::BorderImageOutset::Number(n) => BiOutset::Number(n),
        g::BorderImageOutset::Px(p) => BiOutset::Px(p),
    });
    let rep = |r: g::BorderImageRepeat| match r {
        g::BorderImageRepeat::Stretch => BiRepeat::Stretch,
        g::BorderImageRepeat::Repeat => BiRepeat::Repeat,
        g::BorderImageRepeat::Round => BiRepeat::Round,
        g::BorderImageRepeat::Space => BiRepeat::Space,
    };
    Some(BorderImageSpec {
        source,
        slice,
        fill: bi.fill,
        width,
        outset,
        repeat: (rep(bi.repeat.0), rep(bi.repeat.1)),
        sampling: sampling(paint),
    })
}

/// The 2-D matrix of the element's transform properties (CSS Transforms 2 current transformation
/// matrix) in the parent's space; `Ok(None)` for no transform. Anything that is not planar (3-D
/// rotation, a `z` translation, perspective) is `Err`: the scene paints 2-D
/// transforms only.
fn transform(t: &g::Transform, bx: (f32, f32, f32, f32)) -> Result<Option<Affine2D>, &'static str> {
    const FEATURE: &str = "css.property.transform";
    if t.is_none() {
        return Ok(None);
    }
    let (x, y, w, h) = (bx.0 as f64, bx.1 as f64, bx.2 as f64, bx.3 as f64);
    let lp = |v: &g::LengthPercentage, basis: f64| v.resolve(basis as f32) as f64;
    let mut list = Vec::with_capacity(t.list.len());
    for f in &t.list {
        list.push(match f {
            g::TransformFunction::Matrix(m) => TransformOp::Matrix(Affine2D {
                a: m[0] as f64,
                b: m[1] as f64,
                c: m[2] as f64,
                d: m[3] as f64,
                e: m[4] as f64,
                f: m[5] as f64,
            }),
            g::TransformFunction::Matrix3d(m) => TransformOp::Matrix3d(m.map(f64::from)),
            g::TransformFunction::Translate { x: tx, y: ty, z } => TransformOp::Translate {
                x: lp(tx, w),
                y: lp(ty, h),
                z: *z as f64,
            },
            g::TransformFunction::Scale { x, y, z } => TransformOp::Scale {
                x: *x as f64,
                y: *y as f64,
                z: *z as f64,
            },
            g::TransformFunction::Rotate { x, y, z, deg } => TransformOp::Rotate {
                x: *x as f64,
                y: *y as f64,
                z: *z as f64,
                angle: (*deg as f64).to_radians(),
            },
            g::TransformFunction::Skew { x_deg, y_deg } => TransformOp::Skew {
                ax: (*x_deg as f64).to_radians(),
                ay: (*y_deg as f64).to_radians(),
            },
            g::TransformFunction::Perspective(d) => {
                TransformOp::Perspective(d.map_or(0.0, f64::from))
            }
            g::TransformFunction::Intermediate => return Err(FEATURE),
        });
    }
    let individual = IndividualTransforms {
        translate: t
            .individual
            .translate
            .as_ref()
            .map(|(tx, ty, z)| [lp(tx, w), lp(ty, h), *z as f64]),
        rotate: t.individual.rotate.map(|(axis, deg)| {
            (
                [axis[0] as f64, axis[1] as f64, axis[2] as f64],
                (deg as f64).to_radians(),
            )
        }),
        scale: t.individual.scale.map(|s| s.map(f64::from)),
    };
    let origin = [lp(&t.origin.x, w), lp(&t.origin.y, h), t.origin.z as f64];
    let m = resolve_transform(&list, &individual, origin, RectF::new(x, y, w, h))
        .map_err(|_| FEATURE)?;
    let eq = |r: usize, c: usize, v: f64| (m.at(r, c) - v).abs() < 1e-9;
    let planar = eq(0, 2, 0.0)
        && eq(1, 2, 0.0)
        && eq(2, 0, 0.0)
        && eq(2, 1, 0.0)
        && eq(2, 2, 1.0)
        && eq(2, 3, 0.0)
        && eq(3, 0, 0.0)
        && eq(3, 1, 0.0)
        && eq(3, 2, 0.0)
        && eq(3, 3, 1.0);
    if !planar {
        return Err(FEATURE);
    }
    let flat = flatten(&m).map_err(|_| FEATURE)?;
    // A singular matrix (`scale(0)`) is valid CSS: the element and its subtree are not rendered
    // and not hit-testable (CSS Transforms 1 section 8); the scope paints nothing.
    if flat.projective || !flat.affine.is_finite() {
        return Err(FEATURE);
    }
    Ok(Some(flat.affine))
}

/// The 2-D matrix of the element's `transform` / `translate` / `rotate` / `scale` in document space
/// (about its own border box, `border_box` = x, y, width, height in document px), for the geometry
/// queries (`getBoundingClientRect`, `getClientRects`, the `element_geometry` probe). `None` = no
/// transform, or one the scene does not paint (3-D, perspective, singular): L-GEO-r9.
pub fn element_transform(
    view: &ComputedStyleView<'_>,
    border_box: (f32, f32, f32, f32),
) -> Option<Affine2D> {
    transform(&view.paint().transform, border_box)
        .ok()
        .flatten()
}

fn lookup_to_replaced(
    lookup: ImageLookup,
    view_paint: &g::Paint,
    fit: ObjectFit,
    position: (LenPct, LenPct),
) -> Replaced {
    match lookup {
        ImageLookup::Ready(image) => Replaced::Image(ReplacedImage {
            image,
            fit,
            position,
            sampling: sampling(view_paint),
        }),
        ImageLookup::Pending => Replaced::Pending,
        ImageLookup::Broken(code) => Replaced::Broken(code),
    }
}

fn object_fit(p: &g::Paint) -> ObjectFit {
    match p.object_fit {
        "contain" => ObjectFit::Contain,
        "cover" => ObjectFit::Cover,
        "none" => ObjectFit::None,
        "scale-down" => ObjectFit::ScaleDown,
        _ => ObjectFit::Fill,
    }
}

/// The scene's `clip-path` input from the view's; a value the view could not type is reported.
fn clip_path(
    view: &g::ClipPath,
    unsupported: &mut Vec<&'static str>,
) -> Option<Box<crate::scene::inputs::ClipPathSpec>> {
    use crate::scene::display_list::{Path, PathCommand as C, Point};
    use crate::scene::inputs::{ClipPathSpec, ClipRefBox, ShapeRadiusSpec, ShapeSpec};
    let reference = |b: g::ClipBox| match b {
        g::ClipBox::Margin => ClipRefBox::Margin,
        g::ClipBox::Border => ClipRefBox::Border,
        g::ClipBox::Padding => ClipRefBox::Padding,
        g::ClipBox::Content => ClipRefBox::Content,
    };
    let radius = |r: &g::ShapeRadius| match r {
        g::ShapeRadius::Length(l) => ShapeRadiusSpec::Length(len_pct(l)),
        g::ShapeRadius::ClosestSide => ShapeRadiusSpec::ClosestSide,
        g::ShapeRadius::FarthestSide => ShapeRadiusSpec::FarthestSide,
        g::ShapeRadius::ClosestCorner => ShapeRadiusSpec::ClosestCorner,
        g::ShapeRadius::FarthestCorner => ShapeRadiusSpec::FarthestCorner,
    };
    let xy = |p: &(g::LengthPercentage, g::LengthPercentage)| (len_pct(&p.0), len_pct(&p.1));
    match view {
        g::ClipPath::None | g::ClipPath::Url => None,
        g::ClipPath::Unsupported(feature) => {
            unsupported.push(feature);
            None
        }
        g::ClipPath::Box(b) => Some(Box::new(ClipPathSpec {
            shape: ShapeSpec::Box,
            reference: reference(*b),
        })),
        g::ClipPath::Shape {
            shape,
            reference: b,
        } => {
            let shape = match &**shape {
                g::BasicShape::Inset { insets, radii } => ShapeSpec::Inset {
                    insets: [0, 1, 2, 3].map(|i| len_pct(&insets[i])),
                    radii: [0, 1, 2, 3].map(|i| xy(&radii[i])),
                },
                g::BasicShape::Circle { radius: r, center } => ShapeSpec::Circle {
                    radius: radius(r),
                    center: xy(center),
                },
                g::BasicShape::Ellipse { rx, ry, center } => ShapeSpec::Ellipse {
                    rx: radius(rx),
                    ry: radius(ry),
                    center: xy(center),
                },
                g::BasicShape::Polygon { evenodd, points } => ShapeSpec::Polygon {
                    evenodd: *evenodd,
                    points: points.iter().map(xy).collect(),
                },
                g::BasicShape::Path { evenodd, segments } => {
                    let p = |x: f32, y: f32| Point { x, y };
                    let commands = segments
                        .iter()
                        .map(|s| match *s {
                            g::PathSegment::Move(x, y) => C::MoveTo(p(x, y)),
                            g::PathSegment::Line(x, y) => C::LineTo(p(x, y)),
                            g::PathSegment::Cubic(a, b, c) => {
                                C::CubicTo(p(a.0, a.1), p(b.0, b.1), p(c.0, c.1))
                            }
                            g::PathSegment::Close => C::Close,
                        })
                        .collect();
                    ShapeSpec::Path {
                        evenodd: *evenodd,
                        path: Path { commands },
                    }
                }
            };
            Some(Box::new(ClipPathSpec {
                shape,
                reference: reference(*b),
            }))
        }
    }
}

/// The picture of an image-only generated box: a `::marker` with `list-style-image: url()` (the
/// request key is the list item's `list-style-image`) or a `::before` / `::after` whose `content` is
/// a single `url()`. Layout made the box (`box_tree/pseudo.rs`); other images in `content` are reported
/// there, not painted.
pub(crate) fn pseudo_image(
    node: ui4me_engine_types::ids::NodeId,
    marker: bool,
    before: bool,
    view: &ComputedStyleView<'_>,
    images: Option<&dyn ImageSource>,
    e: &mut BoxExtras,
) {
    use crate::style::view::generated::{Content, ContentItem};
    let generated = view.generated();
    let (property, url) = if marker {
        match &generated.list.image {
            g::Image::Url(u) => ("list-style-image", u.clone()),
            _ => return,
        }
    } else {
        match &generated.content {
            Content::Items(items) => match items.as_slice() {
                [ContentItem::Image(g::Image::Url(u))] => (
                    if before {
                        "content::before"
                    } else {
                        "content::after"
                    },
                    u.clone(),
                ),
                _ => return,
            },
            _ => return,
        }
    };
    let Some(src) = images else {
        e.unsupported.push(if marker {
            "css.property.list-style-image"
        } else {
            "css.value.content-image"
        });
        return;
    };
    let paint = view.paint();
    let centre = (LenPct::frac(0.5), LenPct::frac(0.5));
    e.replaced = Some(lookup_to_replaced(
        src.css_image(node, property, 0, &url.text, url.resolved),
        &paint,
        ObjectFit::Fill,
        centre,
    ));
}

/// Largest blur standard deviation (CSS px) the display list carries.
const MAX_BLUR_SIGMA: f32 = 128.0;

/// One computed `filter` function as the display list carries it (the colour of `drop-shadow()` is
/// already resolved by the style view).
fn filter_fn(f: &g::FilterFunction) -> crate::scene::display_list::FilterFn {
    use crate::scene::display_list::FilterFn as F;
    match *f {
        // 1.26.0: a blur beyond this deviation is flat on any surface the engine rasterises, and an unbounded one
        // would only make the filter work (and a backdrop capture) exceed its budget.
        g::FilterFunction::Blur(v) => F::Blur(if v.is_finite() {
            v.clamp(0.0, MAX_BLUR_SIGMA)
        } else {
            0.0
        }),
        g::FilterFunction::Brightness(v) => F::Brightness(v),
        g::FilterFunction::Contrast(v) => F::Contrast(v),
        g::FilterFunction::Grayscale(v) => F::Grayscale(v),
        g::FilterFunction::HueRotate(v) => F::HueRotate(v),
        g::FilterFunction::Invert(v) => F::Invert(v),
        g::FilterFunction::Opacity(v) => F::Opacity(v),
        g::FilterFunction::Saturate(v) => F::Saturate(v),
        g::FilterFunction::Sepia(v) => F::Sepia(v),
        g::FilterFunction::DropShadow {
            offset_x,
            offset_y,
            blur,
            color: c,
        } => F::DropShadow {
            dx: offset_x,
            dy: offset_y,
            blur,
            color: color(&c),
        },
    }
}

/// Whether layout and paint containment have an effect on the element's principal box (CSS
/// Containment 2 sections 3.2 and 3.4): not on a non-atomic inline box (an `inline` element that is
/// not replaced) and not on an internal table box other than a cell.
fn containment_applies(
    doc: &Document,
    node: ui4me_engine_types::ids::NodeId,
    view: &ComputedStyleView<'_>,
) -> bool {
    use crate::style::view::groups::{DisplayInside as I, DisplayOutside as O};
    let d = view.display_flow().display;
    match (d.outside, d.inside) {
        (_, I::None | I::Contents) => false,
        (_, I::TableInternal) => d.keyword() == "table-cell",
        (O::Inline, I::Flow) if view.pseudo().is_none() => {
            // Replaced elements are atomic inline-level boxes.
            doc.local_name(node).is_ok_and(|n| {
                matches!(
                    n,
                    "img"
                        | "video"
                        | "canvas"
                        | "iframe"
                        | "embed"
                        | "object"
                        | "input"
                        | "select"
                        | "textarea"
                        | "audio"
                )
            })
        }
        (O::Inline, I::Flow) => false,
        _ => true,
    }
}

/// Whether the extras of a box with these values read an image (background, mask, border image): their
/// state changes without a style change, so such extras are not kept across scenes (015 PERF-1).
/// Whether the paint of `node` (the element and its generated and first-letter/first-line pseudo-elements) depends on an
/// image's state, which changes without a style change (015 PERF-2: such a node is never "unchanged"):
/// an `<img>`, image layers, masks and border images, `content: url()` and `list-style-image`.
pub(crate) fn node_reads_images(
    doc: &Document,
    style: &crate::style::view::StyleSnapshot,
    node: ui4me_engine_types::ids::NodeId,
) -> bool {
    use crate::style::view::PseudoKind as P;
    use crate::style::view::generated::{Content, ContentItem};
    // An inline `<svg>` paints its whole subtree (and the rules matching it), which change without a
    // change of the element's own style (F0907).
    if doc.local_name(node).is_ok_and(|n| n == "img" || n == "svg") {
        return true;
    }
    let Some(v) = style.view(node) else {
        return false;
    };
    if reads_images(&v) {
        return true;
    }
    [P::Before, P::After, P::Marker, P::FirstLetter, P::FirstLine, P::FirstLineBefore, P::FirstLineAfter].into_iter().any(|k| {
        style.view_pseudo(node, Some(k)).is_some_and(|pv| {
            let generated = pv.generated();
            reads_images(&pv)
                || !matches!(generated.list.image, g::Image::None)
                || matches!(&generated.content, Content::Items(items) if items.iter().any(|i| matches!(i, ContentItem::Image(_))))
        })
    })
}

pub(crate) fn reads_images(view: &ComputedStyleView<'_>) -> bool {
    let paint = view.paint();
    paint
        .background_layers
        .iter()
        .any(|l| !matches!(l.image, g::Image::None))
        || paint
            .mask_layers
            .iter()
            .any(|l| !matches!(l.image, g::Image::None))
        || paint
            .border_image
            .as_ref()
            .is_some_and(|b| !matches!(b.source, g::Image::None))
}

pub(crate) fn extras_of(
    doc: &Document,
    node: ui4me_engine_types::ids::NodeId,
    view: &ComputedStyleView<'_>,
    border_box: (f32, f32, f32, f32),
    images: Option<&dyn ImageSource>,
) -> BoxExtras {
    let (_, _, w, h) = border_box;
    let mut e = BoxExtras::default();
    let r = &view.box_model().border_radius;
    e.radii = [&r.top_left, &r.top_right, &r.bottom_right, &r.bottom_left]
        .map(|(x, y)| (x.resolve(w), y.resolve(h)));
    e.z_index = view.position_offsets().z_index;
    let paint = view.paint();

    e.shadows = paint
        .box_shadow
        .iter()
        .map(|s| BoxShadowSpec {
            offset_x: s.offset_x,
            offset_y: s.offset_y,
            blur: s.blur,
            spread: s.spread,
            color: color(&s.color),
            inset: s.inset,
        })
        .collect();
    let overflow = view.overflow();
    e.clip_x = overflow.x.clips();
    e.clip_y = overflow.y.clips();
    match transform(&paint.transform, border_box) {
        Ok(t) => e.transform = t,
        Err(feature) => e.unsupported.push(feature),
    }
    // 3-D context properties. `perspective` only changes how descendants with a 3-D transform render
    // (those transforms are reported as unsupported by their own `transform`); on its own it forms a
    // stacking context and a containing block (layout) and paints nothing, so nothing is dropped here.
    e.forms_stacking_context = paint.transform.perspective.is_some();
    // CSS Containment 2: layout and paint containment form a stacking context; paint containment clips
    // the contents to the overflow clip edge (the padding box, like `overflow: clip`). Neither has an
    // effect on a non-atomic inline box or an internal table box other than a cell (015 CQ-2).
    let contain = view.containment();
    if (contain.layout || contain.paint) && containment_applies(doc, node, view) {
        e.forms_stacking_context = true;
        if contain.paint {
            e.clip_x = true;
            e.clip_y = true;
        }
    }
    if paint.transform.preserve_3d {
        e.unsupported.push("css.property.transform-style");
    }
    if paint.transform.backface_hidden {
        e.unsupported.push("css.property.backface-visibility");
    }
    e.filter = paint.filter.iter().map(filter_fn).collect();
    e.backdrop_filter = paint.backdrop_filter.iter().map(filter_fn).collect();
    match crate::scene::display_list::BlendMode::from_css(paint.mix_blend_mode) {
        Some(mode) => e.blend_mode = mode,
        None => e.unsupported.push("css.property.mix-blend-mode"),
    }
    e.isolation = paint.isolation_isolate;
    e.clip_path = clip_path(&paint.clip_path, &mut e.unsupported);
    e.layers = layers(node, &paint, images, &mut e.unsupported);
    e.mask = masks(node, &paint, images, &mut e.unsupported);
    e.border_image = border_image(node, &paint, images, &mut e.unsupported);
    // `background-color` is clipped by the final layer's `background-clip` (Backgrounds 3 section 3.7).
    if let Some(last) = paint.background_layers.last() {
        e.color_clip = bg_clip(last.clip);
        if e.color_clip.is_none()
            && paint.background_color.to_srgb_f32()[3] > 0.0
            && !e.unsupported.contains(&"css.property.background-clip")
        {
            e.unsupported.push("css.property.background-clip");
        }
    }
    match paint.outline.style {
        g::OutlineStyle::Style(g::BorderStyle::None)
        | g::OutlineStyle::Style(g::BorderStyle::Hidden) => {}
        // CSS UI 4: the UA chooses the appearance of `auto`: the ui4me focus ring token (display-list.md section 6).
        g::OutlineStyle::Auto => {
            e.outline = Some(crate::scene::inputs::OutlineSpec {
                width: crate::scene::theme::focus_ring_width(paint.outline.width),
                style: crate::scene::borders::BorderStyle::Solid,
                color: crate::scene::theme::focus_ring_color(
                    color(&paint.outline.color),
                    paint.outline.color_is_current,
                ),
                offset: paint.outline.offset,
                auto: true,
            });
        }
        g::OutlineStyle::Style(style) => {
            use crate::scene::borders::BorderStyle as S;
            let style = match style {
                g::BorderStyle::Solid => S::Solid,
                g::BorderStyle::Dotted => S::Dotted,
                g::BorderStyle::Dashed => S::Dashed,
                g::BorderStyle::Double => S::Double,
                g::BorderStyle::Groove => S::Groove,
                g::BorderStyle::Ridge => S::Ridge,
                g::BorderStyle::Inset => S::Inset,
                g::BorderStyle::Outset => S::Outset,
                g::BorderStyle::None | g::BorderStyle::Hidden => S::None,
            };
            e.outline = Some(crate::scene::inputs::OutlineSpec {
                width: paint.outline.width,
                style,
                color: color(&paint.outline.color),
                offset: paint.outline.offset,
                auto: false,
            });
        }
    }

    // The picture of an inline <svg> (F0907): its subtree serialised and rasterised as a vector image.
    if doc.local_name(node).is_ok_and(|n| n == "svg")
        && doc
            .namespace_uri(node)
            .ok()
            .flatten()
            .is_some_and(|ns| ns.ends_with("/2000/svg"))
    {
        match crate::pipeline::inline_svg::serialize(doc, node, view) {
            Ok(s) => {
                e.unsupported.extend(s.unsupported.iter().copied());
                e.replaced = match images {
                    Some(provider) => {
                        let lookup =
                            provider.inline_svg(node, &s.markup, std::sync::Arc::new(s.targets));
                        Some(lookup_to_replaced(
                            lookup,
                            &paint,
                            ObjectFit::Fill,
                            (LenPct::frac(0.5), LenPct::frac(0.5)),
                        ))
                    }
                    None => None,
                };
                if matches!(e.replaced, Some(Replaced::Broken(_))) {
                    e.unsupported.push("svg.inline.rejected");
                }
            }
            Err("svg.inline.nested") => {}
            Err(feature) => e.unsupported.push(feature),
        }
    }

    // The picture of an <img> (HTML replaced element).
    if doc.local_name(node).is_ok_and(|n| n == "img") {
        let attr = |name: &str| doc.get_attribute(node, name).ok().flatten();
        let (src, srcset, sizes) = (attr("src"), attr("srcset"), attr("sizes"));
        // 015 HTML-4: the `source` elements of a parent `picture` that precede the image.
        let sources = crate::pipeline::resources::picture_sources(doc, node);
        if src.is_some() || srcset.is_some() || !sources.is_empty() {
            let position = (
                len_pct(&paint.object_position.0),
                len_pct(&paint.object_position.1),
            );
            e.replaced = match images {
                Some(provider) => provider
                    .img_element_picture(node, &sources, src, srcset, sizes)
                    .map(|l| lookup_to_replaced(l, &paint, object_fit(&paint), position)),
                None => {
                    e.unsupported.push("html.img");
                    None
                }
            };
        }
    }
    e
}
