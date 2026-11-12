//! owner: 010-paint-render — created by 001 slice (R-28)
//!
//! Renderer-neutral display list (010 `contracts/display-list.md`). Slice subset: the three
//! draw ops the slice paints (`FillRect`, `FillPath`, `DrawGlyphRun`) with the §4 item metadata.
//! Scope ops (§2), the other draw ops (§3) and stacking/clip metadata are added by 010.
//! T032 types; completed by 001 T047 (font table, rect helpers).

use crate::scene::appendix_e::PaintPhase;
use std::collections::BTreeMap;
use std::sync::Arc;
use ui4me_engine_types::geometry::transform::Affine2D;
use ui4me_engine_types::ids::NodeId;

/// sRGB colour, channels in 0…1, already clipped per channel (display-list.md §1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// f32 CSS px point in the item's spatial-node space (document space in the slice).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// f32 CSS px rect (display-list.md §1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// The smallest rect containing `points` (`None` for no points).
    pub fn bounding(points: impl IntoIterator<Item = Point>) -> Option<Rect> {
        let mut it = points.into_iter();
        let p = it.next()?;
        let (mut l, mut t, mut r, mut b) = (p.x, p.y, p.x, p.y);
        for p in it {
            l = l.min(p.x);
            t = t.min(p.y);
            r = r.max(p.x);
            b = b.max(p.y);
        }
        Some(Rect {
            x: l,
            y: t,
            width: r - l,
            height: b - t,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadTo(Point, Point),
    CubicTo(Point, Point, Point),
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub commands: Vec<PathCommand>,
}

/// One gradient colour stop (display-list.md §3): `offset` is in gradient-line units (0 at the
/// start, 1 at the end; repeating gradients are normalised by the scene so the stops span 0…1
/// of one period). Hints are already expanded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    pub offset: f32,
    pub color: Color,
}

/// How a gradient continues beyond its line / angle range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extend {
    Pad,
    Repeat,
}

/// Gradient geometry in the item's space (CSS px). Interpolation is premultiplied sRGB, the
/// default of the legacy gradient syntax; other interpolation spaces are reported by the scene
/// (`UNSUPPORTED_FEATURE`) and never reach the display list.
#[derive(Clone, Debug, PartialEq)]
pub enum GradientKind {
    Linear {
        start: Point,
        end: Point,
    },
    /// Elliptical radial gradient: the end (t = 1) ellipse has radii `(rx, ry)`; t = 0 is the
    /// concentric ellipse scaled by `inner` (0 for a non-repeating gradient).
    Radial {
        center: Point,
        rx: f32,
        ry: f32,
        inner: f32,
    },
    /// CSS conic gradient: `start_angle` in radians clockwise from "up"; the stops span the
    /// turn fractions `first..last` (0..1 unless repeating).
    Conic {
        center: Point,
        start_angle: f32,
        first: f32,
        last: f32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<ColorStop>,
    pub extend: Extend,
}

/// `paint` (display-list.md §3): a solid colour or a gradient.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Color),
    Gradient(Arc<Gradient>),
}

impl Paint {
    /// The colour of a solid paint, or of a gradient's first stop (summaries, diagnostics).
    pub fn representative_color(&self) -> Color {
        match self {
            Paint::Solid(c) => *c,
            Paint::Gradient(g) => g.stops.first().map(|s| s.color).unwrap_or(Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }),
        }
    }
}

/// A rectangle with per-corner elliptical radii `(rx, ry)` in the order top-left, top-right,
/// bottom-right, bottom-left. Radii are already scaled to fit (CSS Backgrounds 3 §5.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedRect {
    pub rect: Rect,
    pub radii: [(f32, f32); 4],
}

impl RoundedRect {
    pub fn square(rect: Rect) -> RoundedRect {
        RoundedRect {
            rect,
            radii: [(0.0, 0.0); 4],
        }
    }

    pub fn is_square(&self) -> bool {
        self.radii.iter().all(|r| r.0 <= 0.0 || r.1 <= 0.0)
    }
}

/// A clip shape in the space of the scope's enclosing spatial node.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipShape {
    Rect(Rect),
    RoundedRect(RoundedRect),
    Path { path: Path, fill_rule: FillRule },
}

/// CSS compositing blend function, independent of the raster backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub fn from_css(s: &str) -> Option<Self> {
        Some(match s {
            "normal" => Self::Normal,
            "multiply" => Self::Multiply,
            "screen" => Self::Screen,
            "overlay" => Self::Overlay,
            "darken" => Self::Darken,
            "lighten" => Self::Lighten,
            "color-dodge" => Self::ColorDodge,
            "color-burn" => Self::ColorBurn,
            "hard-light" => Self::HardLight,
            "soft-light" => Self::SoftLight,
            "difference" => Self::Difference,
            "exclusion" => Self::Exclusion,
            "hue" => Self::Hue,
            "saturation" => Self::Saturation,
            "color" => Self::Color,
            "luminosity" => Self::Luminosity,
            _ => return None,
        })
    }
}

/// Isolated group effect (display-list.md §2 `PushEffect`). Opacity only so far; blend modes
/// and filters are 010 T082.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effect {
    pub opacity: f32,
}

/// `mask-composite` (CSS Masking 1 section 7.8): how a mask layer combines with the layers below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskComposite {
    /// source-over
    Add,
    /// source-out
    Subtract,
    /// source-in
    Intersect,
    /// xor
    Exclude,
}

/// One mask layer of a [`MaskEffect`]: the layer's image, tiled and placed, as draw operations in
/// the scope's space (gradient tiles and image tiles, exactly as a background layer is planned),
/// inside the mask painting area `clip`. Outside the drawn tiles and `clip` the layer is
/// transparent black.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskLayer {
    pub ops: Vec<DrawOp>,
    /// The `mask-clip` area (rounded by the box radii).
    pub clip: RoundedRect,
    /// `mask-mode`: the layer's coverage is its luminance (premultiplied) instead of its alpha.
    pub luminance: bool,
    /// `mask-composite` against the layers below this one (ignored for the lowest layer).
    pub composite: MaskComposite,
}

/// `PushEffect` with a mask (display-list.md section 2, CSS Masking 1): the scope's content is an
/// isolated group multiplied by the coverage of the composed mask layers.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskEffect {
    /// Topmost layer first (the `mask-image` order). Never empty.
    pub layers: Vec<MaskLayer>,
}

/// One `filter` function (Filter Effects 1 section 5), lengths in CSS px of the scope's space,
/// amounts as computed (negative amounts are not valid CSS). `url(#id)` filters never reach the display
/// list (the profile reports them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterFn {
    /// `blur()`: Gaussian blur, the argument is the standard deviation.
    Blur(f32),
    /// `brightness()`: a linear slope on each colour channel.
    Brightness(f32),
    Contrast(f32),
    /// `grayscale()`: the amount is clamped to 0..=1 by the raster.
    Grayscale(f32),
    /// `hue-rotate()`, degrees.
    HueRotate(f32),
    Invert(f32),
    /// `opacity()`: multiplies the alpha channel (clamped to 0..=1).
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
    /// `drop-shadow()`: offset, standard deviation of the blur, colour (already resolved).
    DropShadow {
        dx: f32,
        dy: f32,
        blur: f32,
        color: Color,
    },
}

impl FilterFn {
    /// How far (CSS px) this function can paint beyond its input on each side: `(left, top, right, bottom)`.
    /// A blur reaches three standard deviations; a drop shadow its offset plus the blur.
    pub fn reach(&self) -> [f32; 4] {
        match *self {
            FilterFn::Blur(s) => {
                let r = 3.0 * s.max(0.0);
                [r, r, r, r]
            }
            FilterFn::DropShadow { dx, dy, blur, .. } => {
                let r = 3.0 * blur.max(0.0);
                [
                    (r - dx).max(0.0),
                    (r - dy).max(0.0),
                    (r + dx).max(0.0),
                    (r + dy).max(0.0),
                ]
            }
            _ => [0.0; 4],
        }
    }
}

/// `PushEffect` with a `filter` (display-list.md section 2, Filter Effects 1): the scope's content is
/// an isolated group drawn into its own layer, the functions are applied to the layer in order, and the
/// result is composited into the parent. The filter region is the visual overflow of the content
/// extended by [`FilterEffect::reach`] (the CSS functions have no fixed region).
#[derive(Clone, Debug, PartialEq)]
pub struct FilterEffect {
    /// In `filter` order, first applied first. Never empty.
    pub functions: Vec<FilterFn>,
}

impl FilterEffect {
    /// The union of the functions' reaches along the chain: `(left, top, right, bottom)` in CSS px.
    pub fn reach(&self) -> [f32; 4] {
        // Each function extends the region left by the previous ones.
        self.functions.iter().fold([0.0; 4], |acc, f| {
            let r = f.reach();
            [acc[0] + r[0], acc[1] + r[1], acc[2] + r[2], acc[3] + r[3]]
        })
    }
}

/// Index of a scope in [`DisplayList::scopes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub enum ScopeKind {
    /// `PushSpatial`: a 2-D transform from the scope's local space to its parent's space.
    Spatial(Affine2D),
    /// `PushClip`: non-isolated clip.
    Clip(ClipShape),
    /// `PushEffect`: isolated group.
    Effect(Effect),
    /// An isolated group composited with the parent using a CSS blend function.
    Blend(BlendMode),
    /// `PushEffect` with a `mask`: isolated group masked by the composed mask layers.
    Mask(MaskEffect),
    /// `PushEffect` with a `filter`: isolated group whose layer the filter functions are applied to.
    Filter(FilterEffect),
}

/// One node of the scope tree. A scope is open for exactly the items whose
/// [`DisplayItem::scope`] chain contains it, so push/pop are balanced by construction
/// (display-list.md §7 invariant 1); the lowering derives them from consecutive items' chains.
#[derive(Clone, Debug, PartialEq)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    pub kind: ScopeKind,
}

/// The slice form of 009's `FontRef` (`text-services.md` §5, R-55), copied from 009's
/// `GlyphRunView` without re-resolving anything: `blob` is 007's `FontBlobId.0` (the identity
/// 009 publishes as `FontRef.blob.id.0`, lane E decision E-D4) and `index` the face index.
///
/// 001 T047 keeps this scene-side shape instead of naming 009's type: `ui4me-scene` does not
/// depend on `ui4me-text`, and the T032 tests construct it literally. The bytes behind `blob`
/// are held alive by the snapshot's [`FontTable`] (display-list.md §7 invariant 7). Variation
/// coordinates and synthesis are not carried in the slice (one static face); a run that needs
/// them is reported by the adapter (`appendix_e::prepare_scene`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontRef {
    /// Identity of the font blob held alive by the snapshot.
    pub blob: u64,
    pub index: u32,
}

/// The font blobs referenced by a scene's glyph runs, keyed by [`FontRef::blob`]. Shared
/// (`Arc`) with 007's registry: no bytes are copied (R-55).
#[derive(Clone, Default, PartialEq)]
pub struct FontTable {
    blobs: BTreeMap<u64, Arc<[u8]>>,
}

impl FontTable {
    pub fn new() -> FontTable {
        FontTable::default()
    }

    /// Register the bytes of blob `blob` (the first registration of an id wins).
    pub fn insert(&mut self, blob: u64, bytes: Arc<[u8]>) {
        self.blobs.entry(blob).or_insert(bytes);
    }

    pub fn get(&self, blob: u64) -> Option<&Arc<[u8]>> {
        self.blobs.get(&blob)
    }

    pub fn contains(&self, blob: u64) -> bool {
        self.blobs.contains_key(&blob)
    }

    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// `(blob id, bytes)` in id order.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &Arc<[u8]>)> {
        self.blobs.iter().map(|(k, v)| (*k, v))
    }
}

impl std::fmt::Debug for FontTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.blobs.iter().map(|(k, v)| (k, v.len())))
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

/// Synthetic styling of a glyph run whose family has no matching bold or italic face (CSS Fonts 4
/// `font-synthesis`, initial value `weight style`; L-STYFIX-r7, D-LSTYFIXr7-06). 009 decides it per
/// run (the font's `Synthesis`), the rasteriser applies it: `embolden` expands the outlines by
/// the standard fake-bold amount, `skew_degrees` (0 = none) shears them horizontally.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlyphSynthesis {
    pub embolden: bool,
    pub skew_degrees: f32,
    /// Vertical writing modes (015 WM-1): every glyph of the run is turned about its origin (the pen
    /// position) by a quarter turn; the positions are physical already.
    pub rotation: GlyphRotation,
    /// Horizontal compression of every glyph about its origin (`text-combine-upright` wider than one em,
    /// 015 WM-1b); `None` = 1.
    pub scale_x: Option<f32>,
}

/// A quarter turn of every glyph of a run about its origin (sideways text in a vertical writing mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GlyphRotation {
    #[default]
    None,
    /// 90 degrees clockwise (`vertical-*`, `sideways-rl`).
    Clockwise,
    /// 90 degrees counter-clockwise (`sideways-lr`).
    CounterClockwise,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRun {
    /// Pre-shaping run pen and baseline in scene space, if available.
    pub origin: Option<(f32, f32)>,
    pub font: FontRef,
    pub size_px: f32,
    pub glyphs: Vec<Glyph>,
    pub paint: Paint,
    pub synthesis: GlyphSynthesis,
    /// Normalised variation coordinates (F2Dot14 bits, one per `fvar` axis in axis order) of a
    /// variable face at the run's instance; empty for a static face or the default instance
    /// (015 TXT-2).
    pub coords: Vec<i16>,
}

/// Decoded image pixels: premultiplied RGBA8, sRGB, row-major without padding (007's
/// `ImageRaster`), shared with the resource cache (`Arc`; no bytes are copied).
#[derive(Clone, PartialEq, Eq)]
pub struct ImagePixels {
    pub width: u32,
    pub height: u32,
    pub data: Arc<[u8]>,
}

impl std::fmt::Debug for ImagePixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ImagePixels({}x{}, {} bytes)",
            self.width,
            self.height,
            self.data.len()
        )
    }
}

/// The images referenced by a scene's `DrawImage` items, keyed by the item's `image` id. Held
/// alive by the snapshot like [`FontTable`] (display-list.md §7 invariant 7).
#[derive(Clone, Default, PartialEq)]
pub struct ImageTable {
    images: BTreeMap<u64, ImagePixels>,
}

impl ImageTable {
    pub fn new() -> ImageTable {
        ImageTable::default()
    }

    /// Register the pixels of image `id` (the first registration of an id wins).
    pub fn insert(&mut self, id: u64, pixels: ImagePixels) {
        self.images.entry(id).or_insert(pixels);
    }

    pub fn get(&self, id: u64) -> Option<&ImagePixels> {
        self.images.get(&id)
    }

    pub fn contains(&self, id: u64) -> bool {
        self.images.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u64, &ImagePixels)> {
        self.images.iter().map(|(k, v)| (*k, v))
    }
}

impl std::fmt::Debug for ImageTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.images.iter()).finish()
    }
}

/// `image-rendering` as the raster consumes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageSampling {
    /// `auto` / `smooth` / `high-quality`: bilinear.
    #[default]
    Smooth,
    /// `pixelated` / `crisp-edges`: nearest neighbour.
    Nearest,
}

/// Draw operations (display-list.md §3), slice subset.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawOp {
    /// Filter the previously painted backdrop inside the rounded border box.
    BackdropFilter {
        clip: RoundedRect,
        functions: Vec<FilterFn>,
        backdrop_root: Option<ScopeId>,
    },
    FillRect {
        rect: Rect,
        paint: Paint,
    },
    FillRoundedRect {
        rect: RoundedRect,
        paint: Paint,
    },
    FillPath {
        path: Path,
        fill_rule: FillRule,
        paint: Paint,
    },
    DrawGlyphRun(GlyphRun),
    /// AD-20 `box-shadow` layer: the shadow of `border_rrect` moved by `offset`, grown by
    /// `spread`, blurred with sigma = `blur_radius / 2` (CSS px). An outer shadow is clipped to
    /// the outside of the border box; an `inset` one to the inside of the padding box
    /// (`padding_rrect`).
    DrawBoxShadow {
        border_rrect: RoundedRect,
        padding_rrect: RoundedRect,
        offset: (f32, f32),
        blur_radius: f32,
        spread: f32,
        color: Color,
        inset: bool,
    },
    /// Image `image` (a key of the snapshot's [`ImageTable`]) stretched over `dest`; only the part
    /// inside `visible` (which lies within `dest`) is painted. The whole image maps onto `dest`.
    DrawImage {
        image: u64,
        dest: Rect,
        visible: Rect,
        sampling: ImageSampling,
    },
    /// Image `image` tiled from `tile` (the position and size of one tile): repeated along the
    /// axes with `repeat_*`, drawn once along the others, painted inside `fill`.
    DrawImagePattern {
        image: u64,
        tile: Rect,
        repeat_x: bool,
        repeat_y: bool,
        fill: Rect,
        sampling: ImageSampling,
    },
}

impl DrawOp {
    /// A representative colour of the op's paint (diagnostic summaries).
    pub fn representative_color(&self) -> Color {
        match self {
            DrawOp::FillRect { paint, .. }
            | DrawOp::FillRoundedRect { paint, .. }
            | DrawOp::FillPath { paint, .. } => paint.representative_color(),
            DrawOp::DrawGlyphRun(r) => r.paint.representative_color(),
            DrawOp::DrawBoxShadow { color, .. } => *color,
            DrawOp::BackdropFilter { .. }
            | DrawOp::DrawImage { .. }
            | DrawOp::DrawImagePattern { .. } => Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        }
    }
}

/// Open list of kind labels (display-list.md §4); exported unchanged by 005.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemKind(pub &'static str);

impl ItemKind {
    pub const CANVAS_BACKGROUND: ItemKind = ItemKind("canvas-background");
    pub const BACKGROUND_COLOR: ItemKind = ItemKind("background-color");
    pub const BACKGROUND_IMAGE: ItemKind = ItemKind("background-image");
    pub const BORDER: ItemKind = ItemKind("border");
    /// Replaced content: an `<img>`'s picture.
    pub const IMAGE: ItemKind = ItemKind("image");
    /// The drawn bullet of a `disc`, `circle` or `square` list marker.
    pub const MARKER: ItemKind = ItemKind("marker");
    pub const BOX_SHADOW: ItemKind = ItemKind("box-shadow");
    pub const OUTLINE: ItemKind = ItemKind("outline");
    /// The ui4me focus ring of `outline-style: auto` (display-list.md section 6).
    pub const FOCUS_RING: ItemKind = ItemKind("focus-ring");
    pub const TEXT: ItemKind = ItemKind("text");
    /// One layer of a `text-shadow` under a text run or a decoration line (CSS Text Decoration 3).
    pub const TEXT_SHADOW: ItemKind = ItemKind("text-shadow");
    /// An underline, overline or line-through of a decorating box (CSS Text Decoration 3).
    pub const TEXT_DECORATION: ItemKind = ItemKind("text-decoration");
    /// The selection highlight of a text control (behind its text).
    pub const SELECTION: ItemKind = ItemKind("selection");
    /// The caret of a focused text control (above its text).
    pub const CARET: ItemKind = ItemKind("caret");
    /// The underline of an IME preedit in a text control (010 `display-list.md`).
    pub const PREEDIT_UNDERLINE: ItemKind = ItemKind("preedit-underline");
    /// A themed part of a form control (select arrow and popup, range, progress, meter, marks).
    pub const CONTROL: ItemKind = ItemKind("control");

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Index of an item in paint order within one scene generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaintItemId(pub u32);

/// Owner of an item: the originating node (005 maps it to a `NodeKey`); `anon` names an
/// anonymous box under that node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OwnerKey {
    pub node: NodeId,
    pub anon: Option<u32>,
}

/// One display item with its §4 metadata. `ink` is in document CSS px.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayItem {
    pub id: PaintItemId,
    pub owner: OwnerKey,
    pub phase: PaintPhase,
    pub kind: ItemKind,
    pub ink: Rect,
    /// The innermost open scope of this item (`None` = root, document space).
    pub scope: Option<ScopeId>,
    pub op: DrawOp,
}

/// Items in FR-011 paint order (invariant 2); `items[i].id == PaintItemId(i)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayList {
    pub items: Vec<DisplayItem>,
    /// The scope tree (spatial, clip and effect scopes), parents before children.
    pub scopes: Vec<Scope>,
}

impl DisplayList {
    /// The scope chain of `scope`, outermost first.
    pub fn scope_chain(&self, scope: Option<ScopeId>) -> Vec<ScopeId> {
        let mut chain = Vec::new();
        let mut cur = scope;
        while let Some(id) = cur {
            chain.push(id);
            cur = self.scopes[id.0 as usize].parent;
        }
        chain.reverse();
        chain
    }
}
