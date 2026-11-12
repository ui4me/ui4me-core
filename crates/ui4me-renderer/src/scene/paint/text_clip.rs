//! owner: 010-paint-render — lane L-PAINT3-r14 (`background-clip: text`, `-webkit-background-clip: text`)
//!
//! CSS Backgrounds 3 section 3.7 / CSS Backgrounds 4: with `background-clip: text` the background is painted
//! only where the text of the element and of its in-flow and floated descendants is. The scene makes
//! that a mask scope ([`ScopeKind::Mask`]) around the clipped layers: one mask layer, inside the border box,
//! whose operations are the element's glyph runs painted opaque. The raster backend already composes mask
//! scopes from draw operations, so the clip follows the glyph outlines exactly (anti-aliased, with
//! synthetic bold and oblique), with no backend change.
//!
//! Not part of the clip (recorded limits): text decorations and `text-shadow`, text of positioned or
//! transformed descendants, and the text of a nested element that is itself an isolated group.

use crate::scene::appendix_e::{Builder, PaintBox};
use crate::scene::display_list::{
    Color, DrawOp, MaskComposite, MaskEffect, MaskLayer, Paint, Rect, RoundedRect, ScopeId,
    ScopeKind,
};

/// The mask scope that clips the text-clipped layers of `bx` (parent scope `scope`). A box without text
/// gets an empty mask layer: nothing of its clipped layers is painted.
pub(crate) fn text_clip_scope(
    b: &mut Builder,
    bx: &PaintBox,
    rr: &RoundedRect,
    scope: Option<ScopeId>,
) -> ScopeId {
    let black = Paint::Solid(Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    });
    let ops = bx
        .extras
        .text_clip
        .iter()
        .filter(|r| !r.glyphs.is_empty())
        .map(|r| {
            let mut run = r.clone();
            run.paint = black.clone();
            DrawOp::DrawGlyphRun(run)
        })
        .collect();
    let clip = RoundedRect {
        rect: Rect {
            x: rr.rect.x,
            y: rr.rect.y,
            width: rr.rect.width,
            height: rr.rect.height,
        },
        radii: rr.radii,
    };
    b.scope(
        scope,
        ScopeKind::Mask(MaskEffect {
            layers: vec![MaskLayer {
                ops,
                clip,
                luminance: false,
                composite: MaskComposite::Add,
            }],
        }),
    )
}
