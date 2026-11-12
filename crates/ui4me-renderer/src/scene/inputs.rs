//! owner: 010-paint-render — created by 010 T001, filled by 010 (T012 subset)
//!
//! Paint-facing box styling that goes beyond the slice's background colour and solid borders:
//! corner radii, box shadows, group opacity, overflow clipping, 2-D transforms, positioning
//! class and background layers. [`BoxExtras`] is carried by `appendix_e::PaintBox`; the adapter
//! (`prepare_scene`) fills it from `PaintStyleSource::paint_extras` (resolved to CSS px against
//! the box), and T1 tests construct it directly. All lengths are CSS px; colours sRGB 0…1.

use crate::scene::display_list::{Color, ImagePixels, ImageSampling, Rect};
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::geometry::transform::Affine2D;
use ui4me_engine_types::ids::NodeId;

/// A length-percentage in linear form: `px + frac * basis` (every computed `calc()` with one
/// percentage basis has this form; `50%` is `frac = 0.5`, `10px` is `px = 10`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LenPct {
    pub px: f32,
    pub frac: f32,
}

impl LenPct {
    pub const ZERO: LenPct = LenPct { px: 0.0, frac: 0.0 };

    pub fn px(v: f32) -> LenPct {
        LenPct { px: v, frac: 0.0 }
    }

    pub fn frac(f: f32) -> LenPct {
        LenPct { px: 0.0, frac: f }
    }

    pub fn resolve(self, basis: f32) -> f32 {
        self.px + self.frac * basis
    }
}

/// A decoded image for painting (007 `PaintImage::Raster` plus the natural size 008 uses).
#[derive(Clone, Debug, PartialEq)]
pub struct ImageData {
    /// Identity of these pixels (the key in the scene's image table): stable while the pixels do
    /// not change, different for new pixels of the same URL.
    pub id: u64,
    pub pixels: ImagePixels,
    /// Natural size in CSS px (density-corrected); `None` per axis when the image has no natural
    /// dimension on it (an SVG without a width or height).
    pub natural: (Option<f32>, Option<f32>),
    /// Natural aspect ratio `width / height`, if any.
    pub ratio: Option<f32>,
    /// A vector image (SVG) can be rasterised again at the size it is painted at (CSS Images 3
    /// section 5: the concrete object size); `pixels` is then only the raster at the natural
    /// size and device scale, used where the placed size is not known or cannot be made.
    pub vector: Option<VectorHandle>,
}

/// Rasterises one vector image at a requested size (007's per-(resource, size) raster cache
/// behind the pipeline's image source). The scene calls it while planning, on the owner thread.
pub trait VectorRaster: Send + Sync + std::fmt::Debug {
    /// The image rasterised for a destination of `css_size` CSS px (the provider applies the
    /// device scale). `None` when it cannot be made (budget, size); the caller then keeps the
    /// image's own pixels.
    fn raster_at(&self, css_size: (f32, f32)) -> Option<Arc<ImageData>>;
}

/// Shared handle to a [`VectorRaster`]; equal when it is the same provider object.
#[derive(Clone, Debug)]
pub struct VectorHandle(pub Arc<dyn VectorRaster>);

impl PartialEq for VectorHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl ImageData {
    /// This image for a destination of `css_size` CSS px: a vector image rasterised at that size
    /// (cached by the provider), any other image (or a size that cannot be made) as it is.
    pub fn for_size(self: &Arc<Self>, css_size: (f32, f32)) -> Arc<ImageData> {
        // Beyond this a raster is not worth its memory: the image's own pixels are stretched.
        const MAX_AREA: f32 = 16.0 * 1024.0 * 1024.0;
        let (w, h) = css_size;
        match &self.vector {
            Some(v)
                if w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite() && w * h <= MAX_AREA =>
            {
                v.0.raster_at(css_size).unwrap_or_else(|| self.clone())
            }
            _ => self.clone(),
        }
    }
}

/// What an [`ImageSource`] knows about a reference.
#[derive(Clone, Debug, PartialEq)]
pub enum ImageLookup {
    Ready(Arc<ImageData>),
    /// Still loading: nothing is painted and the scene reports `RESOURCE_PENDING`.
    Pending,
    /// Failed (denied, undecodable, ...): nothing is painted.
    Broken(Code),
}

/// The pipeline-side provider of decoded images for CSS `url()` references and `<img src>`
/// (007 `request_css_image` / `request_img` + `paint_image`). The scene never loads anything.
pub trait ImageSource: Send + Sync + std::fmt::Debug {
    /// The image of a CSS `url()` of `node`'s `property` (`background-image`), layer `layer`. `url`
    /// is the reference as the style engine kept it (`resolved`: an absolute URL already).
    fn css_image(
        &self,
        node: NodeId,
        property: &'static str,
        layer: u32,
        url: &str,
        resolved: bool,
    ) -> ImageLookup;
    /// The image of an `<img>` element whose `src` attribute is `src`.
    fn img_element(&self, node: NodeId, src: &str) -> ImageLookup;
    /// The picture of an inline `<svg>` element (F0907): `markup` is its serialised standalone
    /// document; `targets` maps the element ids written in it to the DOM elements (hit testing).
    /// The default treats it as the `data:` image it is equivalent to.
    fn inline_svg(
        &self,
        node: NodeId,
        markup: &str,
        targets: std::sync::Arc<crate::pipeline::inline_svg::Targets>,
    ) -> ImageLookup {
        let _ = targets;
        self.img_element(node, &crate::pipeline::inline_svg::data_url(markup))
    }
    /// The image of an `<img>` element with its `src`, `srcset` and `sizes` attributes: the
    /// provider selects the source for the document's device pixel ratio and viewport (HTML
    /// "select an image source") and returns its image at the selected density. `None` when the
    /// element has no usable source. The default ignores `srcset`.
    fn img_element_set(
        &self,
        node: NodeId,
        src: Option<&str>,
        srcset: Option<&str>,
        sizes: Option<&str>,
    ) -> Option<ImageLookup> {
        let _ = (srcset, sizes);
        src.filter(|s| !s.trim().is_empty())
            .map(|s| self.img_element(node, s))
    }
    /// As [`ImageSource::img_element_set`] for an `<img>` whose parent is a `<picture>` (015 HTML-4):
    /// `sources` are the `(srcset, sizes, media, type)` attributes of the `source` children before it,
    /// which take part in the selection first. The default ignores them.
    fn img_element_picture(
        &self,
        node: NodeId,
        sources: &[PictureSourceAttrs<'_>],
        src: Option<&str>,
        srcset: Option<&str>,
        sizes: Option<&str>,
    ) -> Option<ImageLookup> {
        let _ = sources;
        self.img_element_set(node, src, srcset, sizes)
    }
}

/// `(srcset, sizes, media, type)` of a `picture`'s `source` element (015 HTML-4).
pub type PictureSourceAttrs<'a> = (
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

/// A replaced element's picture and how it is placed (CSS Images 3 `object-fit`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReplacedImage {
    pub image: Arc<ImageData>,
    pub fit: ObjectFit,
    /// `object-position`.
    pub position: (LenPct, LenPct),
    pub sampling: ImageSampling,
}

/// The state of a replaced element's picture.
#[derive(Clone, Debug, PartialEq)]
pub enum Replaced {
    Image(ReplacedImage),
    /// Still loading: nothing is painted and the scene reports `RESOURCE_PENDING`.
    Pending,
    /// Failed (`code`): nothing is painted.
    Broken(Code),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

/// One `box-shadow` layer (CSS Backgrounds 3 section 7.2), lengths resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadowSpec {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}

/// Gradient colour stop as written: `position` is `None` when omitted (fix-up distributes it),
/// otherwise a fraction of the gradient line (percentages resolved; absolute lengths divided
/// by the line length by the adapter that knows it, or given via [`StopPosition::Length`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopSpec {
    pub position: Option<StopPosition>,
    pub color: Color,
    /// A colour hint (midpoint) *before* this stop.
    pub hint_before: Option<Hint>,
}

/// A colour hint (CSS Images 3 section 3.5.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hint {
    /// The midpoint as a fraction between the previous stop and this one (`0.5` = linear).
    Segment(f32),
    /// The midpoint at a position along the gradient line (resolved against the stops' fix-up).
    At(StopPosition),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StopPosition {
    Fraction(f32),
    /// CSS px along the gradient line.
    Length(f32),
    /// `frac * line length + px` (a `calc()` mixing a percentage and a length).
    Mixed {
        frac: f32,
        px: f32,
    },
}

/// `radial-gradient` ending shape and size keywords.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadialSize {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    /// Explicit radii in px (`circle` uses `rx == ry`).
    Explicit(f32, f32),
    /// Explicit ellipse radii as length-percentages of the tile width and height.
    ExplicitEllipse(LenPct, LenPct),
}

/// A gradient image as specified.
#[derive(Clone, Debug, PartialEq)]
pub enum GradientSpec {
    /// Angle in degrees, CSS convention (0 = to top, 90 = to right). `to <corner>` forms are
    /// resolved by the adapter to the magic-corner angle at the layer's tile size, so they take
    /// [`LinearDirection::ToCorner`].
    Linear {
        direction: LinearDirection,
        stops: Vec<StopSpec>,
        repeating: bool,
    },
    /// Centre as length-percentages of the tile (`50%, 50%` = centre).
    Radial {
        circle: bool,
        size: RadialSize,
        center: (LenPct, LenPct),
        stops: Vec<StopSpec>,
        repeating: bool,
    },
    Conic {
        from_deg: f32,
        center: (LenPct, LenPct),
        stops: Vec<StopSpec>,
        repeating: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinearDirection {
    Angle(f32),
    /// `to top right` etc.: horizontal sign (`+1` right, `-1` left) and vertical sign (`-1`
    /// top, `+1` bottom), per CSS Images 3 "magic corners".
    ToCorner {
        x: i8,
        y: i8,
    },
}

/// `background-repeat` per axis (CSS Backgrounds 3 section 3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // The variants mirror the CSS keywords.
pub enum Repeat {
    Repeat,
    NoRepeat,
    /// Whole tiles only, the rest of the area is spread between them.
    Space,
    /// Whole tiles only, the tiles are rescaled to fill the area.
    Round,
}

/// `background-origin` / `background-clip` box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BgBox {
    BorderBox,
    PaddingBox,
    ContentBox,
    /// `background-clip: text` (CSS Backgrounds 3 section 3.7, `-webkit-background-clip: text`): the layer
    /// paints only inside the glyph outlines of the box's text (`BoxExtras::text_clip`), within the border box.
    /// Only valid as a clip.
    Text,
}

/// The image of a `background-image` layer.
#[derive(Clone, Debug, PartialEq)]
pub enum BgImage {
    Gradient(GradientSpec),
    /// A decoded raster or SVG image (`url()`).
    Raster(Arc<ImageData>),
}

/// `background-size` (CSS Backgrounds 3 section 3.9); `None` per axis is `auto`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BgSize {
    Cover,
    Contain,
    Explicit(Option<LenPct>, Option<LenPct>),
}

impl BgSize {
    /// `auto auto`.
    pub const AUTO: BgSize = BgSize::Explicit(None, None);
}

/// One `background-image` layer, lengths unresolved: they resolve against the positioning area
/// in `background::plan_layer`.
#[derive(Clone, Debug, PartialEq)]
pub struct BgLayerSpec {
    pub image: BgImage,
    /// Mixing with lower background layers inside the isolated background group.
    pub blend_mode: crate::scene::display_list::BlendMode,
    pub origin: BgBox,
    pub clip: BgBox,
    pub size: BgSize,
    /// `background-position`: the offset of the tile is `px + frac * (area - tile)` per axis.
    pub position: (LenPct, LenPct),
    pub repeat: (Repeat, Repeat),
    pub sampling: ImageSampling,
}

/// One `mask-image` layer (CSS Masking 1 section 7): the image, placed like a background layer
/// (`mask-origin`, `-clip`, `-size`, `-position`, `-repeat` map to the background fields; the
/// `attachment` of a background does not exist here), with its mode and composite.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskLayerSpec {
    /// `None`: a mask reference that is not an image (a failed `url()`), which is a transparent
    /// black layer (CSS Masking 1 section 7.1): everything it covers is masked out.
    pub layer: Option<BgLayerSpec>,
    /// `mask-mode: luminance` (an image or gradient `match-source` is `alpha`).
    pub luminance: bool,
    /// `mask-composite` against the layers below.
    pub composite: crate::scene::display_list::MaskComposite,
}

/// One `border-image-slice` offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BiSlice {
    /// Image pixels (coordinates of the image's natural size for vector images).
    Number(f32),
    /// A fraction of the image's width (left, right) or height (top, bottom).
    Fraction(f32),
}

/// One `border-image-width` side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BiWidth {
    /// A multiple of the matching border width.
    Number(f32),
    /// Against the border image area's width (left, right) or height (top, bottom).
    Len(LenPct),
    /// The intrinsic size of the matching slice.
    Auto,
}

/// One `border-image-outset` side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BiOutset {
    /// A multiple of the matching border width.
    Number(f32),
    Px(f32),
}

/// `border-image-repeat` per axis (CSS Backgrounds 3 section 6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BiRepeat {
    Stretch,
    Repeat,
    Round,
    Space,
}

/// `border-image` of a box whose source is ready (CSS Backgrounds 3 section 6). Sides are in the
/// order top, right, bottom, left. A source that is pending, broken or unpainted is left out by
/// the pipeline and the `border-style` borders paint instead (section 6.1, invalid image).
#[derive(Clone, Debug, PartialEq)]
pub struct BorderImageSpec {
    pub source: BgImage,
    pub slice: [BiSlice; 4],
    pub fill: bool,
    pub width: [BiWidth; 4],
    pub outset: [BiOutset; 4],
    pub repeat: (BiRepeat, BiRepeat),
    pub sampling: ImageSampling,
}

/// `outline` of a box (CSS UI 4 section 3): `style` is never `None` (the adapter leaves the field
/// empty for no outline).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineSpec {
    pub width: f32,
    pub style: crate::scene::borders::BorderStyle,
    pub color: Color,
    pub offset: f32,
    /// `outline-style: auto`: the UA-chosen focus ring (CSS UI 4 section 3.1; display-list.md
    /// section 6). `style` is `Solid`, `width` and `color` are the ring's (already resolved by
    /// the adapter with [`crate::scene::theme::focus_ring_width`] / [`crate::scene::theme::focus_ring_color`]); the
    /// items carry the `focus-ring` kind.
    pub auto: bool,
}

/// Positioning class of a box for stacking (006 `display_flow.position`/`float`,
/// `position_offsets.z_index`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PaintClass {
    #[default]
    InFlow,
    Float,
    /// `position` other than `static`.
    Positioned,
}

/// Everything beyond the slice that the scene paints for a box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxExtras {
    /// Border radii `(rx, ry)` in px: top-left, top-right, bottom-right, bottom-left, before
    /// the CSS Backgrounds 3 section 5.5 scaling.
    pub radii: [(f32, f32); 4],
    /// `box-shadow`, first layer topmost.
    pub shadows: Vec<BoxShadowSpec>,
    /// `opacity` (1 = opaque).
    pub opacity: f32,
    /// Blend this element's isolated group with its backdrop.
    pub blend_mode: crate::scene::display_list::BlendMode,
    /// Isolate descendants from the backdrop.
    pub isolation: bool,
    /// The CSS transform as a matrix in the parent's space (transform-origin already applied).
    pub transform: Option<Affine2D>,
    /// `overflow-x`/`overflow-y` other than `visible` (clips the padding box).
    pub clip_x: bool,
    pub clip_y: bool,
    pub class: PaintClass,
    /// `z-index` (`None` = auto).
    pub z_index: Option<i32>,
    /// `background-image` layers, first layer topmost.
    pub layers: Vec<BgLayerSpec>,
    /// `mask-image` layers, first layer topmost; empty = no mask. A mask forms a stacking context
    /// and an isolated group (CSS Masking 1 section 7).
    pub mask: Vec<MaskLayerSpec>,
    /// The `background-clip` of the final layer, which is the clip of `background-color`
    /// (CSS Backgrounds 3 section 3.7); `None` when that clip is one the scene does not paint
    /// (`text`, `border-area`: reported, and the colour is not painted).
    pub color_clip: Option<BgBox>,
    /// `filter` functions in order; empty = none. A filter forms a stacking context (Filter Effects 1
    /// section 5; 010 FR-010) and an isolated group the functions are applied to.
    pub filter: Vec<crate::scene::display_list::FilterFn>,
    /// Filters applied to pixels behind this element before its own paint.
    pub backdrop_filter: Vec<crate::scene::display_list::FilterFn>,
    /// A property that forms a stacking context without painting anything itself (`perspective`: CSS
    /// Transforms 2 section 13; its 3-D effect on transformed descendants is reported by their `transform`).
    pub forms_stacking_context: bool,
    /// The glyph runs `background-clip: text` clips to: the text of the box and of its in-flow and floated
    /// descendants (for an inline box's piece, the text of that line inside it), in the space of the
    /// box's own scope. Filled by the adapter only when a background layer or the colour clips to text.
    pub text_clip: Vec<crate::scene::display_list::GlyphRun>,
    /// An inline box's piece on one line: `[start, end]` is true on a side where the box continues on
    /// another line (`box-decoration-break: slice`). The box's shadows are those of the whole box
    /// sliced at the line breaks: the shape continues across an open side and is cut at the piece's edge.
    pub slice_open: [bool; 2],
    /// A piece of a box broken across fragmentainers (015 FRAG-2b): the piece's border box in document
    /// space. The box's own painting (`PaintBox::border_box` is then the whole, unbroken box) is clipped
    /// to it (`box-decoration-break: slice`), and it is the piece's hit region; the descendants are not
    /// clipped.
    pub fragment_clip: Option<Rect>,
    /// A `fieldset` with a rendered legend (015 HTML-3): the inline range `[x0, x1)` (viewport px) of the
    /// legend's border box, cut out of the block-start border, and the hit region (the whole border box,
    /// while `border_box` is the decorated box, which starts at the border centred on the legend).
    pub fieldset_gap: Option<[f32; 2]>,
    /// The box of an element in the top layer or of its `::backdrop` (015 HTML-1): its order there. It is a
    /// stacking context painted after everything else of the root context, above every `z-index`.
    pub top_layer: Option<u32>,
    pub hit_rect: Option<Rect>,
    /// A multicol container's column rules (015 MC-1): each rule's rectangle in document space and its
    /// line (painted as a left border of that rectangle, after the box's borders).
    pub column_rules: Vec<(Rect, crate::scene::borders::BorderSide)>,
    /// `border-image` (replaces the border-style borders when present).
    pub border_image: Option<BorderImageSpec>,
    /// `outline`, painted last in the box's stacking context.
    pub outline: Option<OutlineSpec>,
    /// The picture of a replaced element (`<img>`), placed by `object-fit`/`object-position`
    /// unless layout published `replaced_content_rect`.
    pub replaced: Option<Replaced>,
    /// Layout's resolved content rectangle of the replaced content (`object-fit` /
    /// `object-position` applied), in document space; filled by the adapter from
    /// `BoxFragment.extras.replaced_content_rect` (008). `None` = place it from `replaced`.
    pub replaced_content_rect: Option<Rect>,
    /// Features (`css.property.*` ids) the style projection could not turn into typed inputs;
    /// the adapter reports each as `UNSUPPORTED_FEATURE` (stage `scene`) and then drops it.
    pub unsupported: Vec<&'static str>,
    /// Used padding (top, right, bottom, left) from the fragment, for `background-origin` /
    /// `-clip: content-box`.
    pub padding: [f32; 4],
    /// The box paints but is not a hit target (a collapsed-border segment of a table, which belongs
    /// to the cells on both sides of its line, FR-080).
    pub hit_transparent: bool,
    /// Clip the content to the content box instead of the padding box (the inner editing area of
    /// a text control scrolls inside its padding).
    pub clip_content_box: bool,
    /// The selection, caret and placeholder of a text control (filled by the adapter).
    pub edit: Option<Box<EditOverlay>>,
    /// The themed parts of a form control (`appearance: auto`; lane L-CTRL-r10): select arrow, popup
    /// and listbox rows, range track and thumb, progress and meter bars, radio and checkbox marks.
    /// Filled by the adapter from layout's `ControlInner.select` geometry and the style source's
    /// [`ControlFacts`].
    pub control: Option<Box<ControlPaint>>,
    /// Overlay scrollbar tracks and thumbs for this box.
    pub scrollbars: Vec<ScrollbarPaint>,
    /// A drawn list bullet (`disc`, `circle`, `square` marker) and its colour, filled by the adapter
    /// from `BoxFragment.extras.marker_symbol` (008) and the `::marker` colour.
    pub marker_symbol: Option<MarkerSymbolPaint>,
    /// The box's index in tree (document) order, from the fragment's box id (preorder in the box
    /// tree). Appendix E step 8 paints the positioned boxes of a stacking context in tree order
    /// whichever fragment they are children of: an out-of-flow box is a child of its containing
    /// block's fragment, after that block's in-flow children, but it paints at its own place in
    /// the tree. 0 for boxes that are never positioned.
    pub tree_order: u32,
    /// `clip-path` (basic shape or geometry box); `None` = none. A clip path forms a stacking
    /// context (CSS Masking 1 section 5.1; 010 FR-010).
    pub clip_path: Option<Box<ClipPathSpec>>,
    /// Used margins (top, right, bottom, left) from the fragment, for the `margin-box` reference
    /// box of `clip-path`.
    pub margin: [f32; 4],
    /// Root element only, and only with a `clip-path`: the canvas background (propagated from the
    /// root or `body`) is clipped with the root's clip path (WPT `clip-path-document-element`),
    /// so the unpropagated default canvas colour is painted unclipped underneath it.
    pub canvas_base: Option<Color>,
    /// Propagated canvas colour, retained with the background layers for blending.
    pub canvas_blend_color: Option<(Color, Rect)>,
    /// Canvas-propagated blended background, painted before all element content.
    pub canvas_blend: Option<Box<crate::scene::appendix_e::PaintBox>>,
}

/// The reference box of a `clip-path` shape (`fill-box`, `stroke-box` and `view-box` are
/// `Border` for an HTML element).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipRefBox {
    Margin,
    Border,
    Padding,
    Content,
}

/// A `<shape-radius>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeRadiusSpec {
    Length(LenPct),
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

/// A `clip-path` shape, lengths in linear form against the reference box (x basis: its width, y
/// basis: its height).
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeSpec {
    /// The edge of the reference box itself (`clip-path: <geometry-box>`).
    Box,
    /// `inset()`: top, right, bottom, left and the corner radii (top-left, top-right,
    /// bottom-right, bottom-left).
    Inset {
        insets: [LenPct; 4],
        radii: [(LenPct, LenPct); 4],
    },
    Circle {
        radius: ShapeRadiusSpec,
        center: (LenPct, LenPct),
    },
    Ellipse {
        rx: ShapeRadiusSpec,
        ry: ShapeRadiusSpec,
        center: (LenPct, LenPct),
    },
    Polygon {
        evenodd: bool,
        points: Vec<(LenPct, LenPct)>,
    },
    /// `path()`: px from the reference box's top-left corner.
    Path {
        evenodd: bool,
        path: crate::scene::display_list::Path,
    },
}

/// `clip-path` of a box.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPathSpec {
    pub shape: ShapeSpec,
    pub reference: ClipRefBox,
}

/// The bullet of a drawn list marker, in document space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerSymbolPaint {
    pub shape: crate::layout::replaced::MarkerShape,
    pub rect: Rect,
    pub color: Color,
}

/// What a text control paints besides its text (010 with 012 `controls.md` section 5), in
/// viewport CSS px.
#[derive(Clone, Debug, PartialEq)]
pub struct EditOverlay {
    /// One rectangle per selected line, painted behind the text.
    pub selection: Vec<Rect>,
    pub selection_color: Color,
    /// The caret (a collapsed selection in a focused control, in the visible blink phase).
    pub caret: Option<Rect>,
    pub caret_color: Color,
    /// The IME preedit underline, one rectangle per line it covers (lane L-CLIP-r13).
    pub preedit: Vec<Rect>,
    /// The text is the placeholder: painted in this colour instead of the control's `color`.
    pub placeholder_color: Option<Color>,
}

impl Default for BoxExtras {
    fn default() -> BoxExtras {
        BoxExtras {
            padding: [0.0; 4],
            margin: [0.0; 4],
            clip_path: None,
            canvas_base: None,
            canvas_blend_color: None,
            canvas_blend: None,
            hit_transparent: false,
            unsupported: Vec::new(),
            radii: [(0.0, 0.0); 4],
            shadows: Vec::new(),
            opacity: 1.0,
            blend_mode: Default::default(),
            isolation: false,
            transform: None,
            clip_x: false,
            clip_y: false,
            class: PaintClass::InFlow,
            z_index: None,
            layers: Vec::new(),
            mask: Vec::new(),
            filter: Vec::new(),
            backdrop_filter: Vec::new(),
            forms_stacking_context: false,
            slice_open: [false; 2],
            fragment_clip: None,
            fieldset_gap: None,
            top_layer: None,
            hit_rect: None,
            column_rules: Vec::new(),
            text_clip: Vec::new(),
            color_clip: Some(BgBox::BorderBox),
            border_image: None,
            outline: None,
            replaced: None,
            replaced_content_rect: None,
            clip_content_box: false,
            edit: None,
            control: None,
            scrollbars: Vec::new(),
            marker_symbol: None,
            tree_order: 0,
        }
    }
}

impl BoxExtras {
    /// Whether a background layer or the background colour is clipped to the text (`background-clip: text`).
    pub fn clips_to_text(&self) -> bool {
        self.color_clip == Some(BgBox::Text) || self.layers.iter().any(|l| l.clip == BgBox::Text)
    }

    /// Whether any extra changes how the box paints.
    pub fn is_default(&self) -> bool {
        *self
            == BoxExtras {
                tree_order: self.tree_order,
                ..BoxExtras::default()
            }
    }
}

/// Options of `prepare_scene_with`. `paint_radii = false` keeps the 001 slice scenario's
/// out-of-subset expectation (S1-OOS): a non-initial `border-*-radius` is reported
/// `UNSUPPORTED_FEATURE` (stage `scene`) and painted as `0`. Every other capability of the
/// scene is always on. The pipeline picks the option (`lane-L010r1-decisions.md`, D-L010r1-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintOptions {
    pub paint_radii: bool,
}

impl PaintOptions {
    /// The 001 slice behaviour for radii (reported, not painted).
    pub const SLICE: PaintOptions = PaintOptions { paint_radii: false };
    /// Radii painted.
    pub const FULL: PaintOptions = PaintOptions { paint_radii: true };
}

/// An unbounded clip extent along an axis that `overflow` does not clip.
pub const UNBOUNDED: f32 = 1.0e6;

/// The padding box of a border box with the given border widths (top, right, bottom, left).
pub fn padding_box(border_box: &Rect, widths: [f32; 4]) -> Rect {
    Rect {
        x: border_box.x + widths[3],
        y: border_box.y + widths[0],
        width: (border_box.width - widths[3] - widths[1]).max(0.0),
        height: (border_box.height - widths[0] - widths[2]).max(0.0),
    }
}

/// The facts a style source provides about a form control for painting (012 `controls.md`
/// section 5; the pipeline reads them from 003's control state and the element's computed style).
#[derive(Clone, Debug, PartialEq)]
pub struct ControlFacts {
    pub kind: ControlKind,
    pub disabled: bool,
    /// The control is focused (a listbox shows its selection in the accent colour).
    pub focused: bool,
    /// `appearance` is not `none`: the control draws its own parts (a `range` always does, since
    /// ui4me has no `::slider-*` pseudo-elements to style it with).
    pub native: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlKind {
    /// A `select` (drop-down or listbox); the rows come from layout's geometry.
    Select,
    Checkbox {
        checked: bool,
        indeterminate: bool,
    },
    Radio {
        checked: bool,
    },
    /// `ratio` is the position of the value within [min, max], 0..1.
    Range {
        ratio: f32,
    },
    /// `None` = indeterminate.
    Progress {
        ratio: Option<f32>,
    },
    Meter {
        ratio: f32,
        region: MeterRegion,
    },
    /// `input type=color`: the swatch.
    Color {
        rgb: [f32; 3],
    },
}

/// The used style of overlay scrollbars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarStyle {
    pub width: f32,
    pub left: bool,
    pub thumb: Color,
    pub track: Color,
}

/// One visible overlay track and its thumb in viewport space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarPaint {
    pub axis: crate::scene::hit_index::ScrollAxis,
    pub track: Rect,
    pub thumb: Rect,
    pub colors: ScrollbarStyle,
}

/// The region of a `meter` value (HTML 4.10.14): decides the bar colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterRegion {
    Optimum,
    Suboptimal,
    EvenLessGood,
}

/// One row of a `select` popup or listbox in viewport space.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectRowPaint {
    pub rect: Rect,
    pub selected: bool,
    pub highlighted: bool,
    pub disabled: bool,
}

/// The painted parts of a form control (see [`BoxExtras::control`]); rectangles in viewport CSS px.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlPaint {
    pub facts: ControlFacts,
    /// Text colour of the control, which the parts derive their tints from.
    pub fg: Color,
    /// Resolved inherited accent colour (the default theme for `auto`).
    pub accent: Color,
    /// Opaque background of the control (its own, else the scheme's field colour).
    pub bg: Color,
    /// The control's border box.
    pub border_box: Rect,
    /// The control's content box.
    pub content: Rect,
    /// A drop-down's open popup panel.
    pub panel: Option<Rect>,
    /// Popup rows or listbox rows.
    pub rows: Vec<SelectRowPaint>,
    /// A listbox (else a drop-down).
    pub listbox: bool,
}
