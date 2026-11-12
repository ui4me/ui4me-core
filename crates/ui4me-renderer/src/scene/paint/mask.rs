//! owner: 010-paint-render — lane L-DARK-r9 (`mask-image` and its longhands; CSS Masking 1 section 7)
//!
//! Mask layers become a [`MaskEffect`] scope: each layer is planned exactly like a background layer
//! (`mask-origin`, `-clip`, `-size`, `-position`, `-repeat` are the background rules, so
//! `background::plan_layer` is the one implementation) and kept as draw operations; the raster
//! backend composes the layers (`mask-composite`) and applies the coverage to the scope's content.

use crate::scene::appendix_e::{Builder, PaintBox, border_widths};
use crate::scene::display_list::{DrawOp, MaskEffect, MaskLayer, Rect, RoundedRect};
use crate::scene::inputs::BgImage;
use crate::scene::paint::background::{ImagePlan, LayerError, bg_box, paint_of, plan_layer};

/// The mask of `bx` (its `extras.mask` is not empty). A layer that cannot be planned (a degenerate
/// tile or origin, too many tiles) paints nothing, i.e. it is transparent black; too many tiles is
/// reported like a background layer.
pub(crate) fn plan_mask(b: &mut Builder, bx: &PaintBox) -> MaskEffect {
    let e = &bx.extras;
    let border_box = Rect {
        x: bx.border_box.origin.x.to_f64() as f32,
        y: bx.border_box.origin.y.to_f64() as f32,
        width: bx.border_box.size.width.to_f64() as f32,
        height: bx.border_box.size.height.to_f64() as f32,
    };
    let widths = border_widths(&bx.border);
    let mut layers = Vec::with_capacity(e.mask.len());
    for spec in &e.mask {
        let empty = |clip: RoundedRect| MaskLayer {
            ops: Vec::new(),
            clip,
            luminance: spec.luminance,
            composite: spec.composite,
        };
        let Some(layer) = &spec.layer else {
            layers.push(empty(RoundedRect::square(border_box)));
            continue;
        };
        let plan = match plan_layer(layer, &border_box, widths, e.padding, e.radii) {
            Ok(p) => p,
            Err(err) => {
                if err == LayerError::TooManyTiles {
                    b.report("css.property.mask-size", bx.node);
                }
                layers.push(empty(bg_box(
                    layer.clip,
                    &border_box,
                    widths,
                    e.padding,
                    e.radii,
                )));
                continue;
            }
        };
        let mut ops = Vec::new();
        for (rect, gradient) in plan.tiles {
            ops.push(DrawOp::FillRect {
                rect,
                paint: paint_of(gradient),
            });
        }
        if let (BgImage::Raster(data), Some(image)) = (&layer.image, plan.image) {
            let data = &crate::scene::paint::background::sized_for(data, &image);
            b.register_image(data);
            match image {
                ImagePlan::Tiles(tiles) => {
                    for (dest, visible) in tiles {
                        ops.push(DrawOp::DrawImage {
                            image: data.id,
                            dest,
                            visible,
                            sampling: layer.sampling,
                        });
                    }
                }
                ImagePlan::Pattern {
                    tile,
                    repeat_x,
                    repeat_y,
                    fill,
                } => {
                    ops.push(DrawOp::DrawImagePattern {
                        image: data.id,
                        tile,
                        repeat_x,
                        repeat_y,
                        fill,
                        sampling: layer.sampling,
                    });
                }
            }
        }
        layers.push(MaskLayer {
            ops,
            clip: plan.clip,
            luminance: spec.luminance,
            composite: spec.composite,
        });
    }
    MaskEffect { layers }
}
