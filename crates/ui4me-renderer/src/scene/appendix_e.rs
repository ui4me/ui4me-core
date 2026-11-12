//! owner: 010-paint-render — created by 001 slice (R-28)
//!
//! CSS 2.1 Appendix E phases, phase ids (010 FR-011, `contracts/display-list.md` §4) and the
//! slice scene build. All phases exist; the slice populates `root-background`, `background`
//! (the root element's own background and borders), `block-background` and `inline-content`
//! (plan "Component slice approach", U4C-SCN).
//!
//! **Input seam.** [`SceneInput`] is the paint-facing view of the committed fragments and
//! styles. [`prepare_scene`] (001 T047) builds it from 008's [`FragmentTree`] and a
//! [`PaintStyleSource`] — the paint, box-model and border groups of 006's `ComputedStyleView`
//! projected into scene types. `ui4me-scene` depends only on `ui4me-core` and `ui4me-layout`,
//! so the `ComputedStyleView` → [`PaintStyle`] mapping is implemented by the caller that holds
//! the style snapshot (the pipeline, T049). [`prepare_scene`] also runs the paint-stage
//! consumption checks (FR-022: for example `border-top-left-radius` is treated as its initial
//! value and reported with stage `scene`) and collects the font blobs the glyph runs reference.
//! The pipeline calls [`PreparedScene::build`] (or [`build_scene`]) at S5. T1 tests construct a
//! [`SceneInput`] directly.

/// The adapter's and builder's maps hash node ids and small keys with `crate::scene::reuse::FastHasher` (015 PERF-2).
type HashMap<K, V> = crate::scene::reuse::FastMap<K, V>;

use crate::layout::box_tree::PositionValue;
use crate::layout::box_tree::{BoxOwner, PseudoKind};
use crate::layout::coordinator::overflow::{ScrollOffsets, ScrollTarget};
use crate::layout::coordinator::sticky::sticky_shift;
use crate::layout::fragment::{ControlInner, FragmentId, FragmentKind, FragmentTree, OverflowMode};
use crate::layout::ifc::interface::{owner_node, owner_pseudo};
use crate::scene::borders::{
    BorderSide, BorderSides, BorderStyle, ComputedBorderStyle, fit_radii, rounded_border_pieces,
    side_fills,
};
use crate::scene::display_list::{
    ClipShape, Color, DisplayItem, DisplayList, DrawOp, Effect, FillRule, FilterEffect, FontRef,
    FontTable, Glyph, GlyphRun, GlyphSynthesis, ImageTable, ItemKind, OwnerKey, Paint, PaintItemId,
    Rect, RoundedRect, Scope, ScopeId, ScopeKind,
};
use crate::scene::hit_index::{HitPart, HitRegion, HitTestIndex};
use crate::scene::inputs::{BoxExtras, EditOverlay, PaintClass, PaintOptions};
use crate::scene::paint::{background, controls, outline, replaced, shadow};
use crate::scene::{clip_tree, stacking};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{FeatureId, ReportKey, ReportLocation, UnsupportedReason};
use ui4me_engine_types::geometry::layout_unit::LayoutUnit;
use ui4me_engine_types::geometry::{LayoutPoint, LayoutRect, LayoutSize};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::revision::RevisionTuple;
use ui4me_engine_types::stage::Stage;

/// Appendix E paint phases (closed list for `ui4me-paint-tiers/1`), in paint order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PaintPhase {
    RootBackground,
    Background,
    NegativeZ,
    BlockBackground,
    Float,
    InlineContent,
    PositionedZeroAuto,
    PositiveZ,
    Outline,
    OverlayScrollbar,
    Chrome,
}

impl PaintPhase {
    /// Every phase, in paint order.
    pub const ALL: &'static [PaintPhase] = &[
        PaintPhase::RootBackground,
        PaintPhase::Background,
        PaintPhase::NegativeZ,
        PaintPhase::BlockBackground,
        PaintPhase::Float,
        PaintPhase::InlineContent,
        PaintPhase::PositionedZeroAuto,
        PaintPhase::PositiveZ,
        PaintPhase::Outline,
        PaintPhase::OverlayScrollbar,
        PaintPhase::Chrome,
    ];

    /// The published phase identifier, for example `"block-background"`.
    pub fn id(self) -> &'static str {
        match self {
            PaintPhase::RootBackground => "root-background",
            PaintPhase::Background => "background",
            PaintPhase::NegativeZ => "negative-z",
            PaintPhase::BlockBackground => "block-background",
            PaintPhase::Float => "float",
            PaintPhase::InlineContent => "inline-content",
            PaintPhase::PositionedZeroAuto => "positioned-zero-auto",
            PaintPhase::PositiveZ => "positive-z",
            PaintPhase::Outline => "outline",
            PaintPhase::OverlayScrollbar => "overlay-scrollbar",
            PaintPhase::Chrome => "chrome",
        }
    }
}

/// One `text-shadow` layer (CSS Text Decoration 3 section 4.1), colour resolved, lengths in CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextShadowLayer {
    pub offset_x: f32,
    pub offset_y: f32,
    /// The blur radius; the Gaussian's standard deviation is half of it (as for `box-shadow`).
    pub blur: f32,
    pub color: Color,
}

/// A text fragment's paint input: one line-height rectangle and its glyph run (009).
#[derive(Clone, Debug, PartialEq)]
pub struct PaintText {
    pub text_node: NodeId,
    /// The text node's parent element (may be an inline element such as `span#n`).
    pub parent_element: NodeId,
    /// Document CSS px; also the hit region (hit-test.md §2.3).
    pub line_rect: LayoutRect,
    pub run: GlyphRun,
    /// The `text-shadow` layers of the text's element, first layer topmost; painted under the run.
    pub shadows: Vec<TextShadowLayer>,
}

/// A text decoration of one line (CSS Text Decoration 3; 008 `DecorationFragment`): the lines of
/// a decorating box over its text. `rect` is the paint extent in document CSS px (see
/// `crate::layout::fragment::DecorationFragment`), `thickness` the thickness of one line.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintDecoration {
    /// The decorating element.
    pub owner: NodeId,
    pub kind: crate::layout::fragment::DecorationKind,
    pub style: crate::layout::fragment::DecorationLineStyle,
    pub color: Color,
    pub rect: Rect,
    pub thickness: f32,
    /// The decorating box's `text-shadow` layers: the shadows of the line are painted under it.
    pub shadows: Vec<TextShadowLayer>,
    /// A line of a vertical writing mode (015 WM-1): the decoration runs along the physical y axis; the
    /// pattern is laid out along the line and turned with it (line-over on the right for `Clockwise`).
    pub turn: crate::scene::display_list::GlyphRotation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaintChild {
    /// Boxed (015 PERF-2): a `PaintBox` is ~850 bytes, and every child (text runs included) was that
    /// size inline; a table's child list was ~7 MiB per scene.
    Box(Box<PaintBox>),
    Text(PaintText),
    /// Painted with the line's text: underlines and overlines before the glyphs, line-throughs after.
    Decoration(PaintDecoration),
    /// An element's box as [`PaintChild::Box`], shared with the scene reuse state ([`crate::scene::reuse`];
    /// 015 PERF-2): an unchanged subtree is taken over from the previous scene, not copied.
    Shared(std::sync::Arc<PaintBox>),
}

/// A block-level box fragment's paint input (slice: no positioning, floats or effects).
#[derive(Clone, Debug, PartialEq)]
pub struct PaintBox {
    pub node: NodeId,
    /// Document CSS px.
    pub border_box: LayoutRect,
    /// `None` for a transparent background.
    pub background: Option<Color>,
    pub border: BorderSides,
    /// Radii, shadows, opacity, overflow, transform, positioning class and background layers.
    pub extras: BoxExtras,
    /// Children in tree order.
    pub children: Vec<PaintChild>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneInput {
    pub generation: u64,
    pub revision: RevisionTuple,
    /// Viewport in CSS px; the canvas covers it.
    pub viewport: LayoutSize,
    /// Canvas background after root/body propagation (010 FR-023).
    pub canvas_background: Color,
    /// The root element's box.
    pub root: PaintBox,
}

/// Immutable scene snapshot: display list plus the hit index built in the same traversal (AD-22).
#[derive(Clone, Debug)]
pub struct SceneSnapshot {
    pub generation: u64,
    pub revision: RevisionTuple,
    pub viewport: LayoutSize,
    pub display_list: DisplayList,
    pub hit_index: HitTestIndex,
    /// Every font blob a `DrawGlyphRun` references, held alive for the snapshot's lifetime
    /// (display-list.md §7 invariant 7). Empty for scenes built by [`build_scene`].
    pub fonts: FontTable,
    /// Every image a `DrawImage` / `DrawImagePattern` item references, held alive for the
    /// snapshot's lifetime (display-list.md §7 invariant 7).
    pub images: ImageTable,
    /// Scene-stage `UNSUPPORTED_FEATURE` reports of this build (from [`prepare_scene`]),
    /// de-duplicated by key, in traversal order. 005 records them with the DOM revision.
    pub reports: Vec<SceneReport>,
}

/// S5: build the display list in Appendix E order and the hit index in one traversal.
pub fn build_scene(input: &SceneInput) -> SceneSnapshot {
    build_scene_with(input, FontTable::new(), Vec::new())
}

/// [`build_scene`] with the font blobs and scene-stage reports of [`prepare_scene`].
pub fn build_scene_with(
    input: &SceneInput,
    fonts: FontTable,
    reports: Vec<SceneReport>,
) -> SceneSnapshot {
    let mut b = Builder::new(input.revision.document);
    let vw = lu(input.viewport.width);
    let vh = lu(input.viewport.height);
    let canvas = Rect {
        x: 0.0,
        y: 0.0,
        width: vw,
        height: vh,
    };
    let canvas_item = Pending {
        owner: input.root.node,
        phase: PaintPhase::RootBackground,
        kind: ItemKind::CANVAS_BACKGROUND,
        ink: canvas,
        scope: None,
        op: DrawOp::FillRect {
            rect: canvas,
            paint: Paint::Solid(input.canvas_background),
        },
    };
    // Appendix E: each stacking context paints its own background and borders, then negative-z
    // contexts, in-flow block backgrounds, floats, inline content, z-index 0/auto positioned
    // descendants and positive-z contexts, recursively (CSS 2.1 Appendix E.2).
    // A root `clip-path` clips the canvas background too: the default canvas colour is painted
    // first, unclipped, then the propagated background inside the root's transform and clip.
    let mut canvas_items = Vec::new();
    match (&input.root.extras.clip_path, input.root.extras.canvas_base) {
        (Some(spec), Some(base)) => {
            canvas_items.push(Pending {
                op: DrawOp::FillRect {
                    rect: canvas,
                    paint: Paint::Solid(base),
                },
                ..canvas_item_clone(&canvas_item)
            });
            let spatial = input
                .root
                .extras
                .transform
                .map(|t| b.scope(None, ScopeKind::Spatial(t)));
            let scope =
                crate::scene::paint::clip_path::clip_scope(&mut b, &input.root, spec, spatial);
            canvas_items.push(Pending {
                scope: Some(scope),
                ..canvas_item
            });
        }
        _ => canvas_items.push(canvas_item),
    }
    if let Some(canvas) = &input.root.extras.canvas_blend {
        // The propagated background belongs to the canvas, outside the source element's
        // opacity, clipping and transforms. The root's canvas clip still applies.
        let parent = canvas_items.last().and_then(|item| item.scope);
        b.paint_box_decor(
            canvas,
            &mut canvas_items,
            PaintPhase::RootBackground,
            parent,
        );
    }
    let root = b.paint_context(&input.root, None, true).flatten();
    let mut viewport_items = Vec::new();
    let mut viewport_regions = Vec::new();
    crate::scene::paint::scrollbar::push(&input.root, &mut viewport_items, None);
    b.scrollbar_hits(&input.root, &mut viewport_regions);
    let items = canvas_items
        .into_iter()
        .chain(root.items)
        .chain(viewport_items)
        .enumerate()
        .map(|(i, p)| DisplayItem {
            id: PaintItemId(i as u32),
            owner: OwnerKey {
                node: p.owner,
                anon: None,
            },
            phase: p.phase,
            kind: p.kind,
            ink: p.ink,
            scope: p.scope,
            op: p.op,
        })
        .collect();
    let regions: Vec<HitRegion> = root
        .regions
        .into_iter()
        .chain(viewport_regions)
        .map(|(r, _)| r)
        .collect();
    let mut all_reports = reports;
    for r in b.reports {
        if !all_reports.iter().any(|x| x.key == r.key) {
            all_reports.push(r);
        }
    }
    SceneSnapshot {
        generation: input.generation,
        revision: input.revision,
        viewport: input.viewport,
        display_list: DisplayList {
            items,
            scopes: b.scopes,
        },
        hit_index: HitTestIndex::from_regions(regions),
        fonts,
        images: b.images,
        reports: all_reports,
    }
}

fn canvas_item_clone(p: &Pending) -> Pending {
    Pending {
        owner: p.owner,
        phase: p.phase,
        kind: p.kind,
        ink: p.ink,
        scope: p.scope,
        op: p.op.clone(),
    }
}

fn lu(v: LayoutUnit) -> f32 {
    v.to_f64() as f32
}

fn rect_of(r: &LayoutRect) -> Rect {
    Rect {
        x: lu(r.origin.x),
        y: lu(r.origin.y),
        width: lu(r.size.width),
        height: lu(r.size.height),
    }
}

fn region(r: &LayoutRect, element: NodeId, part: HitPart, fragment_index: u32) -> HitRegion {
    let (x0, y0) = (r.origin.x.to_f64(), r.origin.y.to_f64());
    HitRegion {
        inverse: ui4me_engine_types::geometry::transform::Affine2D::IDENTITY,
        clips: Default::default(),
        x0,
        y0,
        x1: x0 + r.size.width.to_f64(),
        y1: y0 + r.size.height.to_f64(),
        element,
        part,
        fragment_index,
    }
}

pub(crate) struct Pending {
    pub(crate) owner: NodeId,
    pub(crate) phase: PaintPhase,
    pub(crate) kind: ItemKind,
    pub(crate) ink: Rect,
    pub(crate) scope: Option<ScopeId>,
    pub(crate) op: DrawOp,
}

/// The items of one stacking context by Appendix E step.
#[derive(Default)]
struct Ctx {
    /// Steps 1-2: the context root's own backgrounds, borders and shadows.
    bg: Vec<Pending>,
    /// Step 3, with each context's z-index.
    neg: Vec<(i32, Vec<Pending>)>,
    /// Step 4.
    block: Vec<Pending>,
    /// Step 5.
    float: Vec<Pending>,
    /// Step 7.
    inline: Vec<Pending>,
    /// Step 8: positioned descendants with z-index auto / 0 and z-index 0 contexts, tree order
    /// (each with its box's `tree_order`; they are pushed in fragment order, which puts an
    /// out-of-flow box after the in-flow children of its containing block).
    zero: Vec<(u32, Vec<Pending>)>,
    /// Step 9, with each context's z-index.
    pos: Vec<(i32, Vec<Pending>)>,
    /// Step 10: outlines of the context's boxes, tree order.
    outline: Vec<Pending>,
    /// The top layer (015 HTML-1; CSS Position 4 section 3): its boxes' stacking contexts in top-layer
    /// order, painted after everything else of the context (only the root context has any).
    top: Vec<(u32, Vec<Pending>)>,
    /// The hit regions of the same steps, in the same buckets: hit testing walks the regions in
    /// reverse *paint* order (hit-test.md section 2), not in tree order.
    regions: RegionBuckets,
}

/// `z-index` of an open `select` popup (the control forms a top-most stacking context while open).
pub const POPUP_Z_INDEX: i32 = 2_000_000_000;

/// A hit region and whether it is a text line region.
type Reg = (HitRegion, bool);

/// [`Ctx`]'s buckets for hit regions (outlines have none).
#[derive(Default)]
struct RegionBuckets {
    bg: Vec<Reg>,
    neg: Vec<(i32, Vec<Reg>)>,
    block: Vec<Reg>,
    float: Vec<Reg>,
    inline: Vec<Reg>,
    zero: Vec<(u32, Vec<Reg>)>,
    pos: Vec<(i32, Vec<Reg>)>,
    overlay: Vec<Reg>,
    top: Vec<(u32, Vec<Reg>)>,
}

/// A flattened stacking context: display items and hit regions, both in paint order.
#[derive(Default)]
struct Sub {
    items: Vec<Pending>,
    regions: Vec<Reg>,
}

impl Ctx {
    /// Visit every pending item of the context (all steps, nested contexts included).
    fn for_each_item(&mut self, mut f: impl FnMut(&mut Pending)) {
        let nested = self
            .neg
            .iter_mut()
            .map(|(_, v)| v)
            .chain(self.zero.iter_mut().map(|(_, v)| v))
            .chain(self.pos.iter_mut().map(|(_, v)| v))
            .chain(self.top.iter_mut().map(|(_, v)| v));
        for v in nested {
            v.iter_mut().for_each(&mut f);
        }
        for v in [
            &mut self.bg,
            &mut self.block,
            &mut self.float,
            &mut self.inline,
            &mut self.outline,
        ] {
            v.iter_mut().for_each(&mut f);
        }
    }

    fn flatten(mut self) -> Sub {
        // Stable sorts keep tree order among equal z-indices.
        self.neg.sort_by_key(|(z, _)| *z);
        self.pos.sort_by_key(|(z, _)| *z);
        let mut r = self.regions;
        r.neg.sort_by_key(|(z, _)| *z);
        r.pos.sort_by_key(|(z, _)| *z);
        self.zero.sort_by_key(|(order, _)| *order);
        r.zero.sort_by_key(|(order, _)| *order);
        self.top.sort_by_key(|(order, _)| *order);
        r.top.sort_by_key(|(order, _)| *order);
        let mut out = self.bg;
        out.extend(self.neg.into_iter().flat_map(|(_, v)| v));
        out.extend(self.block);
        out.extend(self.float);
        out.extend(self.inline);
        out.extend(self.zero.into_iter().flat_map(|(_, v)| v));
        out.extend(self.pos.into_iter().flat_map(|(_, v)| v));
        out.extend(self.outline);
        out.extend(self.top.into_iter().flat_map(|(_, v)| v));
        let mut regions = r.bg;
        regions.extend(r.neg.into_iter().flat_map(|(_, v)| v));
        regions.extend(r.block);
        regions.extend(r.float);
        regions.extend(r.inline);
        regions.extend(r.zero.into_iter().flat_map(|(_, v)| v));
        regions.extend(r.pos.into_iter().flat_map(|(_, v)| v));
        regions.extend(r.overlay);
        regions.extend(r.top.into_iter().flat_map(|(_, v)| v));
        Sub {
            items: out,
            regions,
        }
    }
}

/// One tree-order traversal producing the display items, the scope tree and the hit regions (AD-22).
pub(crate) struct Builder {
    document: ui4me_engine_types::ids::DocumentId,
    scopes: Vec<Scope>,
    text_ordinal: HashMap<NodeId, u32>,
    reports: Vec<SceneReport>,
    pub(crate) images: ImageTable,

    hit_shapes: std::sync::Arc<Vec<crate::scene::hit_index::HitClip>>,
    hit_transform: ui4me_engine_types::geometry::transform::Affine2D,
    backdrop_root: Option<ScopeId>,
}

impl Builder {
    fn new(document: ui4me_engine_types::ids::DocumentId) -> Builder {
        Builder {
            document,
            scopes: Vec::new(),
            text_ordinal: HashMap::default(),
            reports: Vec::new(),
            images: ImageTable::new(),
            hit_shapes: Default::default(),
            hit_transform: ui4me_engine_types::geometry::transform::Affine2D::IDENTITY,
            backdrop_root: None,
        }
    }

    /// Attach the active clip and spatial chains to a region.
    fn clip_region(&self, mut r: HitRegion) -> Option<HitRegion> {
        r.clips = self.hit_shapes.clone();
        r.inverse = inverse_hit_transform(self.hit_transform);
        Some(r)
    }

    /// The hit region of a box's own area: its border box, cut by the border radius (1.26.0: the
    /// corners outside the curve pass the pointer through, as the painted background does).
    fn box_region(&self, b: &PaintBox, hit: HitRegion) -> Option<HitRegion> {
        let mut r = self.clip_region(hit)?;
        if b.extras.hit_rect.is_none()
            && b.extras.fragment_clip.is_none()
            && b.extras.radii.iter().any(|&(x, y)| x > 0.0 && y > 0.0)
        {
            let rect = rect_of(&b.border_box);
            let rr = RoundedRect {
                rect,
                radii: fit_radii(&rect, b.extras.radii),
            };
            if !rr.is_square() {
                let mut clips = (*r.clips).clone();
                clips.push(crate::scene::hit_index::HitClip {
                    shape: ClipShape::RoundedRect(rr).into(),
                    inverse: inverse_hit_transform(self.hit_transform),
                });
                r.clips = std::sync::Arc::new(clips);
            }
        }
        Some(r)
    }

    /// Overflow clips descendants in the coordinate space of the clipping box.
    fn narrow_hit_clip(
        &mut self,
        b: &PaintBox,
    ) -> std::sync::Arc<Vec<crate::scene::hit_index::HitClip>> {
        let saved = self.hit_shapes.clone();
        if let Some([x0, y0, x1, y1]) = clip_tree::overflow_hit_rect(b) {
            let r = Rect {
                x: x0 as f32,
                y: y0 as f32,
                width: (x1 - x0) as f32,
                height: (y1 - y0) as f32,
            };
            std::sync::Arc::make_mut(&mut self.hit_shapes).push(crate::scene::hit_index::HitClip {
                shape: ClipShape::Rect(r).into(),
                inverse: inverse_hit_transform(self.hit_transform),
            });
        }
        saved
    }

    /// Hold `image`'s pixels in the snapshot for the items that reference it.
    pub(crate) fn register_image(&mut self, image: &crate::scene::inputs::ImageData) {
        self.images.insert(image.id, image.pixels.clone());
    }

    pub(crate) fn scope(&mut self, parent: Option<ScopeId>, kind: ScopeKind) -> ScopeId {
        self.scopes.push(Scope { parent, kind });
        ScopeId((self.scopes.len() - 1) as u32)
    }

    pub(crate) fn report(&mut self, feature: &'static str, node: NodeId) {
        let Some(feature) = FeatureId::new(feature) else {
            return;
        };
        let key = ReportKey {
            feature,
            stage: Stage::Scene,
            document: self.document,
            location: ReportLocation::Element { node },
        };
        if !self.reports.iter().any(|r| r.key == key) {
            self.reports.push(SceneReport {
                key,
                code: Code::UnsupportedFeature,
                reason: UnsupportedReason::NotConsumedByStage,
            });
        }
    }

    /// Paint `b` as the root of its own stacking context (or as an atomic float / positioned
    /// pseudo-context): its own paint first, then its descendants.
    fn paint_context(&mut self, b: &PaintBox, parent: Option<ScopeId>, is_root: bool) -> Ctx {
        let mut ctx = Ctx::default();
        if b.extras.transform.is_some_and(|t| t.determinant() == 0.0) {
            return ctx;
        }
        let saved_shapes = self.hit_shapes.clone();
        let saved_transform = self.hit_transform;
        if let Some(t) = b.extras.transform {
            let combined = self.hit_transform.then_after(&t);
            if !combined.is_finite() || combined.determinant() == 0.0 {
                return ctx;
            }
            self.hit_transform = combined;
        }
        if let Some(spec) = &b.extras.clip_path {
            std::sync::Arc::make_mut(&mut self.hit_shapes).push(crate::scene::hit_index::HitClip {
                shape: crate::scene::paint::clip_path::shape_clip(spec, b).into(),
                inverse: inverse_hit_transform(self.hit_transform),
            });
        }
        let scope = box_scope(self, b, parent);
        let saved_backdrop_root = self.backdrop_root;
        let mut bg = Vec::new();
        if !b.extras.backdrop_filter.is_empty() {
            let rect = rect_of(&b.border_box);
            let clip = RoundedRect {
                rect,
                radii: fit_radii(&rect, b.extras.radii),
            };
            bg.push(Pending {
                owner: b.node,
                phase: PaintPhase::Background,
                kind: ItemKind::BACKGROUND_COLOR,
                ink: rect,
                scope,
                op: DrawOp::BackdropFilter {
                    clip,
                    functions: b.extras.backdrop_filter.clone(),
                    backdrop_root: saved_backdrop_root,
                },
            });
        }
        if b.extras.opacity < 1.0
            || b.extras.blend_mode != crate::scene::display_list::BlendMode::Normal
            || b.extras.clip_path.is_some()
            || !b.extras.mask.is_empty()
            || !b.extras.filter.is_empty()
            || !b.extras.backdrop_filter.is_empty()
        {
            self.backdrop_root = scope;
        }
        self.paint_box_decor(b, &mut bg, PaintPhase::Background, scope);
        ctx.bg = bg;
        if !b.extras.hit_transparent
            && let Some(r) = self.box_region(b, region(&hit_rect(b), b.node, HitPart::Content, 0))
        {
            ctx.regions.bg.push((r, false));
        }
        let content = self.content_scope(b, scope);
        let saved_clip = self.narrow_hit_clip(b);
        self.children(b, &mut ctx, content);
        self.paint_replaced(b, &mut ctx.inline, content);
        self.hit_shapes = saved_clip;
        if !is_root {
            self.scrollbar_hits(b, &mut ctx.regions.overlay);
        }
        self.hit_shapes = saved_shapes;
        self.hit_transform = saved_transform;
        self.backdrop_root = saved_backdrop_root;
        if !is_root {
            crate::scene::paint::scrollbar::push(b, &mut ctx.outline, scope);
        }
        outline::push_outline(self, b, &mut ctx.outline, scope);
        // The ink of a filtered box reaches past its content (Filter Effects 1: the filter region
        // covers the visual overflow plus the blur and the drop shadow): every item of the group
        // grows by the filter's reach.
        if !b.extras.filter.is_empty() {
            let [l, t, r, bt] = FilterEffect {
                functions: b.extras.filter.clone(),
            }
            .reach();
            ctx.for_each_item(|p| {
                p.ink = Rect {
                    x: p.ink.x - l,
                    y: p.ink.y - t,
                    width: p.ink.width + l + r,
                    height: p.ink.height + t + bt,
                };
            });
        }
        ctx
    }

    /// The picture of a replaced element (step 7: foreground content of the box).
    fn paint_replaced(&mut self, b: &PaintBox, out: &mut Vec<Pending>, scope: Option<ScopeId>) {
        if let Some(m) = &b.extras.marker_symbol {
            replaced::push_marker_symbol(b.node, m, out, scope);
        }
        if let Some(c) = &b.extras.control {
            controls::push_foreground(b, c, out, scope);
        }
        if b.extras.replaced.is_none() {
            return;
        }
        let bb = rect_of(&b.border_box);
        let rr = RoundedRect {
            rect: bb,
            radii: fit_radii(&bb, b.extras.radii),
        };
        replaced::push_replaced(self, b, &rr, border_widths(&b.border), out, scope);
    }

    fn children(&mut self, b: &PaintBox, ctx: &mut Ctx, scope: Option<ScopeId>) {
        // A text control's selection highlight goes behind its text, the caret above it.
        if let Some(e) = &b.extras.edit {
            for r in &e.selection {
                ctx.inline.push(Pending {
                    owner: b.node,
                    phase: PaintPhase::InlineContent,
                    kind: ItemKind::SELECTION,
                    ink: *r,
                    scope,
                    op: DrawOp::FillRect {
                        rect: *r,
                        paint: Paint::Solid(e.selection_color),
                    },
                });
            }
        }
        if let Some(c) = &b.extras.control {
            controls::push_behind_text(b, c, &mut ctx.inline, scope);
        }
        for child in &b.children {
            match child {
                PaintChild::Box(c) => self.paint_child_box(c, ctx, scope),
                PaintChild::Shared(c) => self.paint_child_box(c, ctx, scope),
                PaintChild::Text(t) => self.visit_text(t, ctx, scope),
                PaintChild::Decoration(d) => {
                    crate::scene::paint::text::push_decoration(self, d, &mut ctx.inline, scope)
                }
            }
        }
        if let Some(c) = &b.extras.control {
            controls::push_over_text(b, c, &mut ctx.inline, scope);
            // The open popup is a hit target of its own (topmost; 010 FR-067).
            if let Some(p) = c.panel
                && let Some(r) = self.clip_region(HitRegion {
                    inverse: ui4me_engine_types::geometry::transform::Affine2D::IDENTITY,
                    clips: Default::default(),
                    x0: f64::from(p.x),
                    y0: f64::from(p.y),
                    x1: f64::from(p.x + p.width),
                    y1: f64::from(p.y + p.height),
                    element: b.node,
                    part: HitPart::Content,
                    fragment_index: 0,
                })
            {
                ctx.regions.inline.push((r, false));
            }
        }
        if let Some(e) = &b.extras.edit {
            for r in &e.preedit {
                ctx.inline.push(Pending {
                    owner: b.node,
                    phase: PaintPhase::InlineContent,
                    kind: ItemKind::PREEDIT_UNDERLINE,
                    ink: *r,
                    scope,
                    op: DrawOp::FillRect {
                        rect: *r,
                        paint: Paint::Solid(e.caret_color),
                    },
                });
            }
        }
        if let Some(e) = &b.extras.edit
            && let Some(r) = e.caret
        {
            ctx.inline.push(Pending {
                owner: b.node,
                phase: PaintPhase::InlineContent,
                kind: ItemKind::CARET,
                ink: r,
                scope,
                op: DrawOp::FillRect {
                    rect: r,
                    paint: Paint::Solid(e.caret_color),
                },
            });
        }
    }

    fn paint_child_box(&mut self, c: &PaintBox, ctx: &mut Ctx, scope: Option<ScopeId>) {
        let e = &c.extras;
        // A non-invertible transform (`scale(0)`): the box and its subtree are not rendered and
        // not hit-testable (CSS Transforms 1 section 8; 010 FR-083).
        if e.transform.is_some_and(|t| t.determinant() == 0.0) {
            return;
        }
        if let Some(order) = e.top_layer {
            let sub = self.paint_context(c, scope, false).flatten();
            ctx.top.push((order, sub.items));
            ctx.regions.top.push((order, sub.regions));
        } else if stacking::creates_stacking_context(e) {
            let sub = self.paint_context(c, scope, false).flatten();
            match e.z_index.unwrap_or(0) {
                z if z < 0 => {
                    ctx.neg.push((z, sub.items));
                    ctx.regions.neg.push((z, sub.regions));
                }
                0 => {
                    ctx.zero.push((e.tree_order, sub.items));
                    ctx.regions.zero.push((e.tree_order, sub.regions));
                }
                z => {
                    ctx.pos.push((z, sub.items));
                    ctx.regions.pos.push((z, sub.regions));
                }
            }
        } else if e.class != PaintClass::InFlow {
            // A float, or a positioned box with z-index auto: painted atomically as if it
            // formed a stacking context (CSS 2.1 Appendix E steps 5 and 8), "but any positioned
            // descendants and descendants which actually create a new stacking context should be
            // considered part of the parent stacking context, not this new one": its negative,
            // zero/auto-positioned and positive lists move up to `ctx` (where they sort by z-index
            // and tree order with the parent's own), only the rest stays atomic.
            let mut pseudo = self.paint_context(c, scope, false);
            ctx.neg.append(&mut pseudo.neg);
            ctx.regions.neg.append(&mut pseudo.regions.neg);
            ctx.pos.append(&mut pseudo.pos);
            ctx.regions.pos.append(&mut pseudo.regions.pos);
            let hoisted_zero = std::mem::take(&mut pseudo.zero);
            let hoisted_zero_regions = std::mem::take(&mut pseudo.regions.zero);
            let sub = pseudo.flatten();
            ctx.zero.extend(hoisted_zero);
            ctx.regions.zero.extend(hoisted_zero_regions);
            if e.class == PaintClass::Float {
                ctx.float.extend(sub.items);
                ctx.regions.float.extend(sub.regions);
            } else {
                ctx.zero.push((e.tree_order, sub.items));
                ctx.regions.zero.push((e.tree_order, sub.regions));
            }
        } else {
            self.paint_box_decor(c, &mut ctx.block, PaintPhase::BlockBackground, scope);
            if !c.extras.hit_transparent
                && let Some(r) =
                    self.box_region(c, region(&hit_rect(c), c.node, HitPart::Content, 0))
            {
                ctx.regions.block.push((r, false));
            }
            let content = self.content_scope(c, scope);
            let saved_clip = self.narrow_hit_clip(c);
            self.children(c, ctx, content);
            self.paint_replaced(c, &mut ctx.inline, content);
            self.hit_shapes = saved_clip;
            self.scrollbar_hits(c, &mut ctx.regions.overlay);
            crate::scene::paint::scrollbar::push(c, &mut ctx.outline, scope);
            outline::push_outline(self, c, &mut ctx.outline, scope);
        }
    }

    fn scrollbar_hits(&self, b: &PaintBox, out: &mut Vec<Reg>) {
        for bar in &b.extras.scrollbars {
            let r = bar.track;
            if let Some(hit) = self.clip_region(HitRegion {
                inverse: ui4me_engine_types::geometry::transform::Affine2D::IDENTITY,
                clips: Default::default(),
                x0: f64::from(r.x),
                y0: f64::from(r.y),
                x1: f64::from(r.x + r.width),
                y1: f64::from(r.y + r.height),
                element: b.node,
                part: HitPart::Scrollbar {
                    container: b.node,
                    axis: bar.axis,
                },
                fragment_index: 0,
            }) {
                out.push((hit, false));
            }
        }
    }

    /// The scope for a box's descendants: the box's own scope plus its `overflow` clip.
    fn content_scope(&mut self, b: &PaintBox, scope: Option<ScopeId>) -> Option<ScopeId> {
        let e = &b.extras;
        if !(e.clip_x || e.clip_y) {
            return scope;
        }
        Some(clip_tree::overflow_clip_scope(self, b, scope))
    }

    /// Shadows, background colour, background layers, inset shadows and borders of one box.
    fn paint_box_decor(
        &mut self,
        b: &PaintBox,
        out: &mut Vec<Pending>,
        phase: PaintPhase,
        scope: Option<ScopeId>,
    ) {
        // A piece of a broken box (015 FRAG-2b): the whole box's decoration, cut to the piece.
        let scope = match b.extras.fragment_clip {
            Some(r) => Some(self.scope(scope, ScopeKind::Clip(ClipShape::Rect(r)))),
            None => scope,
        };
        let bb = rect_of(&b.border_box);
        let e = &b.extras;
        let radii = fit_radii(&bb, e.radii);
        let rr = RoundedRect { rect: bb, radii };
        let widths = border_widths(&b.border);
        for s in e.shadows.iter().rev().filter(|s| !s.inset) {
            shadow::push_box_shadow(self, b, &rr, widths, s, out, phase, scope);
        }
        // `background-clip: text`: the layers (and the colour) clipped to the text paint inside one mask
        // scope made of the glyph outlines.
        // Background layers blend only with each other and the background colour, never with
        // the element's backdrop. Keep the isolated background group separate from its borders.
        let bg_scope = if e
            .layers
            .iter()
            .any(|l| l.blend_mode != crate::scene::display_list::BlendMode::Normal)
        {
            Some(self.scope(scope, ScopeKind::Effect(Effect { opacity: 1.0 })))
        } else {
            scope
        };
        let text_scope = e
            .clips_to_text()
            .then(|| crate::scene::paint::text_clip::text_clip_scope(self, b, &rr, bg_scope));
        let clip_scope = |is_text: bool| if is_text { text_scope } else { bg_scope };
        if let Some((color, rect)) = e.canvas_blend_color {
            out.push(Pending {
                owner: b.node,
                phase,
                kind: ItemKind::BACKGROUND_COLOR,
                ink: rect,
                scope: bg_scope,
                op: DrawOp::FillRect {
                    rect,
                    paint: Paint::Solid(color),
                },
            });
        }
        if let (Some(c), Some(clip)) = (b.background.filter(|c| c.a > 0.0), e.color_clip) {
            // The colour is clipped by the final layer's `background-clip`; an inner box has the
            // outer radii reduced by the border (and padding) thickness (Corner shaping).
            let mut crr = background::bg_box(clip, &bb, widths, e.padding, e.radii);
            let opaque_solid_border = widths.iter().all(|width| *width > 0.0)
                && [
                    &b.border.top,
                    &b.border.right,
                    &b.border.bottom,
                    &b.border.left,
                ]
                .iter()
                .all(|side| side.style == BorderStyle::Solid && side.color.a == 1.0);
            if crr.is_square()
                && clip == crate::scene::inputs::BgBox::BorderBox
                && opaque_solid_border
            {
                // Match the snapped straight border edge: otherwise a fractional box leaves
                // a fringe of background outside the border and leaks through its inner edge.
                let snap = |v: f32| (v + 0.5).floor();
                let x = snap(crr.rect.x);
                let y = snap(crr.rect.y);
                crr.rect = Rect {
                    x,
                    y,
                    width: (snap(crr.rect.x + crr.rect.width) - x).max(0.0),
                    height: (snap(crr.rect.y + crr.rect.height) - y).max(0.0),
                };
            }
            let op = if crr.is_square() {
                DrawOp::FillRect {
                    rect: crr.rect,
                    paint: Paint::Solid(c),
                }
            } else {
                DrawOp::FillRoundedRect {
                    rect: crr,
                    paint: Paint::Solid(c),
                }
            };
            out.push(Pending {
                owner: b.node,
                phase,
                kind: ItemKind::BACKGROUND_COLOR,
                ink: crr.rect,
                scope: clip_scope(clip == crate::scene::inputs::BgBox::Text),
                op,
            });
        }
        for layer in e.layers.iter().rev() {
            let base = clip_scope(layer.clip == crate::scene::inputs::BgBox::Text);
            let layer_scope = if layer.blend_mode != crate::scene::display_list::BlendMode::Normal {
                Some(self.scope(base, ScopeKind::Blend(layer.blend_mode)))
            } else {
                base
            };
            background::push_layer(self, b, layer, widths, out, phase, layer_scope);
        }
        for s in e.shadows.iter().rev().filter(|s| s.inset) {
            shadow::push_box_shadow(self, b, &rr, widths, s, out, phase, scope);
        }
        // A ready `border-image` replaces the border-style borders (CSS Backgrounds 3 section 6).
        let border_image_painted = match &e.border_image {
            Some(spec) => crate::scene::paint::border_image::push_border_image(
                self, b, spec, widths, out, phase, scope,
            ),
            None => false,
        };
        // Straight border paths are snapped before painting. Their fieldset gap clips must use
        // the same box, otherwise fractional edges clip away part of an otherwise solid border.
        let snap = |v: f32| (v + 0.5).floor();
        let x0 = snap(bb.x);
        let y0 = snap(bb.y);
        let x1 = snap(bb.x + bb.width);
        let y1 = snap(bb.y + bb.height);
        let snapped = Rect {
            x: x0,
            y: y0,
            width: (x1 - x0).max(0.0),
            height: (y1 - y0).max(0.0),
        };
        let clip_box = if rr.is_square() { snapped } else { bb };
        // A fieldset's legend gap (015 HTML-3): the border is painted three times, under clips that leave
        // the legend's inline range of the block-start border out.
        let border_scopes: Vec<Option<ScopeId>> = match e.fieldset_gap {
            Some([gx0, gx1]) if !border_image_painted => {
                let bt = widths[0];
                let rest = Rect {
                    x: clip_box.x,
                    y: clip_box.y + bt,
                    width: clip_box.width,
                    height: (clip_box.height - bt).max(0.0),
                };
                let left = Rect {
                    x: clip_box.x,
                    y: clip_box.y,
                    width: (gx0 - clip_box.x).max(0.0),
                    height: bt,
                };
                let right = Rect {
                    x: gx1,
                    y: clip_box.y,
                    width: (clip_box.x + clip_box.width - gx1).max(0.0),
                    height: bt,
                };
                [rest, left, right]
                    .into_iter()
                    .map(|r| Some(self.scope(scope, ScopeKind::Clip(ClipShape::Rect(r)))))
                    .collect()
            }
            _ => vec![scope],
        };
        if border_image_painted {
        } else if rr.is_square() {
            // The reference browser snaps straight border box edges in CSS pixels
            // before building style bands and before applying transforms or device scale.
            for &scope in &border_scopes {
                for (_, path, color) in side_fills(snapped, &b.border) {
                    let ink = path_bounds(&path).unwrap_or(bb);
                    out.push(Pending {
                        owner: b.node,
                        phase,
                        kind: ItemKind::BORDER,
                        ink,
                        scope,
                        op: DrawOp::FillPath {
                            path,
                            fill_rule: FillRule::NonZero,
                            paint: Paint::Solid(color),
                        },
                    });
                }
            }
        } else {
            for &scope in &border_scopes {
                let (pieces, skipped) = rounded_border_pieces(bb, radii, &b.border);
                for side in skipped {
                    self.report(BORDER_STYLE_FEATURES[side as usize], b.node);
                }
                for p in pieces {
                    let piece_scope = match p.clip {
                        Some(wedge) => Some(self.scope(
                            scope,
                            ScopeKind::Clip(ClipShape::Path {
                                path: wedge,
                                fill_rule: FillRule::NonZero,
                            }),
                        )),
                        None => scope,
                    };
                    let ink = path_bounds(&p.path).unwrap_or(bb);
                    out.push(Pending {
                        owner: b.node,
                        phase,
                        kind: ItemKind::BORDER,
                        ink,
                        scope: piece_scope,
                        op: DrawOp::FillPath {
                            path: p.path,
                            fill_rule: p.fill_rule,
                            paint: Paint::Solid(p.color),
                        },
                    });
                }
            }
        }
        // Column rules (015 MC-1): after the borders, in the box's decoration (CSS Multicol 1 section 4.2).
        for (r, side) in &e.column_rules {
            let none = BorderSide {
                width: ui4me_engine_types::geometry::layout_unit::LayoutUnit::ZERO,
                style: BorderStyle::None,
                color: side.color,
            };
            let sides = BorderSides {
                top: none,
                right: none,
                bottom: none,
                left: *side,
            };
            for (_, path, color) in side_fills(*r, &sides) {
                let ink = path_bounds(&path).unwrap_or(*r);
                out.push(Pending {
                    owner: b.node,
                    phase,
                    kind: ItemKind::BORDER,
                    ink,
                    scope,
                    op: DrawOp::FillPath {
                        path,
                        fill_rule: FillRule::NonZero,
                        paint: Paint::Solid(color),
                    },
                });
            }
        }
    }

    fn visit_text(&mut self, t: &PaintText, ctx: &mut Ctx, scope: Option<ScopeId>) {
        let ordinal = self.text_ordinal.entry(t.parent_element).or_insert(0);
        let fragment_index = *ordinal;
        *ordinal += 1;
        if let Some(r) = self.clip_region(region(
            &t.line_rect,
            t.parent_element,
            HitPart::Text {
                text_node: t.text_node,
            },
            fragment_index,
        )) {
            ctx.regions.inline.push((r, true));
        }
        if t.run.glyphs.is_empty() {
            return;
        }
        // `text-shadow`: the run again, offset, in the shadow colour and blurred, last layer first (the first is on top).
        for sh in t.shadows.iter().rev() {
            if sh.color.a <= 0.0 {
                continue;
            }
            let mut run = t.run.clone();
            for g in run.glyphs.iter_mut() {
                g.x += sh.offset_x;
                g.y += sh.offset_y;
            }
            if let Some((x, y)) = &mut run.origin {
                *x += sh.offset_x;
                *y += sh.offset_y;
            }
            run.paint = Paint::Solid(sh.color);
            let base = rect_of(&t.line_rect);
            let ink = crate::scene::paint::text::shadow_ink(base, sh);
            let sc = crate::scene::paint::text::shadow_scope(self, scope, sh);
            ctx.inline.push(Pending {
                owner: t.parent_element,
                phase: PaintPhase::InlineContent,
                kind: ItemKind::TEXT_SHADOW,
                ink,
                scope: sc,
                op: DrawOp::DrawGlyphRun(run),
            });
        }
        // Ink: the line rectangle (glyph outlines are not measured by the scene in the slice).
        ctx.inline.push(Pending {
            owner: t.parent_element,
            phase: PaintPhase::InlineContent,
            kind: ItemKind::TEXT,
            ink: rect_of(&t.line_rect),
            scope,
            op: DrawOp::DrawGlyphRun(t.run.clone()),
        });
    }
}

/// The glyph runs of `children` and of their in-flow and floated descendants, for `background-clip: text`
/// (positioned and transformed descendants paint in other spaces and are not part of the clip).
fn collect_text_runs(children: &[PaintChild], out: &mut Vec<GlyphRun>) {
    for c in children {
        match c {
            PaintChild::Text(t) => {
                if !t.run.glyphs.is_empty() {
                    out.push(t.run.clone());
                }
            }
            PaintChild::Box(b) => {
                if b.extras.transform.is_none() && b.extras.class != PaintClass::Positioned {
                    collect_text_runs(&b.children, out);
                }
            }
            PaintChild::Shared(b) => {
                if b.extras.transform.is_none() && b.extras.class != PaintClass::Positioned {
                    collect_text_runs(&b.children, out);
                }
            }
            PaintChild::Decoration(_) => {}
        }
    }
}

/// `visibility: hidden | collapse`: the box keeps its geometry (and its descendants theirs, which may be
/// `visibility: visible`) but paints no background, border, shadow, image, outline or control of its own
/// and is not a hit target (CSS Display 3 / CSS 2.1 section 11.2). Scopes (opacity, filter, clip, mask, transform) stay:
/// they apply to the visible descendants.
fn hide_own_painting(
    background: &mut Option<Color>,
    border: &mut BorderSides,
    extras: &mut BoxExtras,
) {
    *background = None;
    extras.column_rules.clear();
    for s in [
        &mut border.top,
        &mut border.right,
        &mut border.bottom,
        &mut border.left,
    ] {
        s.style = BorderStyle::None;
    }
    extras.layers.clear();
    extras.shadows.clear();
    extras.border_image = None;
    extras.outline = None;
    extras.replaced = None;
    extras.control = None;
    extras.edit = None;
    extras.marker_symbol = None;
    extras.hit_transparent = true;
}

/// The spatial (transform) and effect (opacity) scopes of a box that forms a context.
fn box_scope(b: &mut Builder, bx: &PaintBox, parent: Option<ScopeId>) -> Option<ScopeId> {
    let e = &bx.extras;
    let mut s = parent;
    if let Some(t) = e.transform {
        s = Some(b.scope(s, ScopeKind::Spatial(t)));
    }
    if e.blend_mode != crate::scene::display_list::BlendMode::Normal {
        s = Some(b.scope(s, ScopeKind::Blend(e.blend_mode)));
    }
    if e.opacity < 1.0 || e.isolation || !e.backdrop_filter.is_empty() {
        s = Some(b.scope(
            s,
            ScopeKind::Effect(Effect {
                opacity: e.opacity.clamp(0.0, 1.0),
            }),
        ));
    }
    // The mask applies to the box's own painting (after filters and clips, before opacity: opacity is the outer group).
    if !e.mask.is_empty() {
        let effect = crate::scene::paint::mask::plan_mask(b, bx);
        s = Some(b.scope(s, ScopeKind::Mask(effect)));
    }
    // The clip path is the innermost: it cuts the box's painting and its descendants before the
    // mask and the opacity group are applied (Filter Effects 1 order: filter, clip, mask, opacity; the filter scope below is innermost).
    if let Some(spec) = &e.clip_path {
        s = Some(crate::scene::paint::clip_path::clip_scope(b, bx, spec, s));
    }
    // The filter is the innermost group: it applies to the box's own painting and its descendants
    // first, then the clip path, the mask and the opacity group cut and fade its result.
    if !e.filter.is_empty() {
        s = Some(b.scope(
            s,
            ScopeKind::Filter(FilterEffect {
                functions: e.filter.clone(),
            }),
        ));
    }
    s
}

/// The hit region of a box: its piece when it is broken across fragmentainers (015 FRAG-2b).
fn inverse_hit_transform(
    t: ui4me_engine_types::geometry::transform::Affine2D,
) -> ui4me_engine_types::geometry::transform::Affine2D {
    use ui4me_engine_types::geometry::transform::Affine2D;
    let det = t.determinant();
    if det == 0.0 || !det.is_finite() {
        return Affine2D::IDENTITY;
    }
    Affine2D {
        a: t.d / det,
        b: -t.b / det,
        c: -t.c / det,
        d: t.a / det,
        e: (t.c * t.f - t.d * t.e) / det,
        f: (t.b * t.e - t.a * t.f) / det,
    }
}

fn hit_rect(b: &PaintBox) -> LayoutRect {
    match b.extras.hit_rect.or(b.extras.fragment_clip) {
        Some(r) => {
            let q = |v: f32| LayoutUnit::quantize(f64::from(v)).value;
            LayoutRect {
                origin: LayoutPoint {
                    x: q(r.x),
                    y: q(r.y),
                },
                size: ui4me_engine_types::geometry::LayoutSize {
                    width: q(r.width),
                    height: q(r.height),
                },
            }
        }
        None => b.border_box,
    }
}

pub(crate) fn border_widths(sides: &BorderSides) -> [f32; 4] {
    let w = |s: &BorderSide| s.width.to_f64() as f32;
    [
        w(&sides.top),
        w(&sides.right),
        w(&sides.bottom),
        w(&sides.left),
    ]
}

pub(crate) fn path_bounds(path: &crate::scene::display_list::Path) -> Option<Rect> {
    use crate::scene::display_list::PathCommand as C;
    Rect::bounding(path.commands.iter().flat_map(|c| match c {
        // Control points bound a curve, so the ink box is a safe superset.
        C::MoveTo(p) | C::LineTo(p) => vec![*p],
        C::QuadTo(a, b) => vec![*a, *b],
        C::CubicTo(a, b, c) => vec![*a, *b, *c],
        C::Close => Vec::new(),
    }))
}

// ------------------------------------------------------------------ adapter (008 + 006 → SceneInput)

/// Default canvas colour of the light used colour scheme (display-list.md §6 "Canvas default").
pub const CANVAS_DEFAULT_LIGHT: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

/// The paint-facing projection of one element's `ComputedStyleView` (006 groups `paint`,
/// `inline_text`, `box_model`, `display_flow`), in scene types. Colours are sRGB 0…1,
/// non-premultiplied, already clipped (006 `Color::to_srgb_f32`).
#[derive(Clone, Debug, PartialEq)]
pub struct PaintStyle {
    /// Inherited accent colour, or the theme default when `auto`.
    pub accent_color: Option<Color>,
    /// `color` (text colour; `currentColor` already resolved by 006).
    pub color: Color,
    /// `background-color` (alpha 0 = transparent).
    pub background_color: Color,
    /// `opacity`.
    pub opacity: f32,
    /// `visibility: visible`.
    pub visible: bool,
    /// `border-{top,right,bottom,left}-style`. Widths come from 008's fragments (used, snapped).
    pub border_style: [ComputedBorderStyle; 4],
    /// `border-{top,right,bottom,left}-color`.
    pub border_color: [Color; 4],
    /// `text-decoration-color` (`currentColor` resolved): the colour of the lines this box
    /// decorates (L-PERF-b3: resolved here, not carried by the fragments).
    pub decoration_color: Color,
    /// `text-shadow` layers, first layer topmost (inherited, so the element's own text and the
    /// lines it decorates use its list).
    pub text_shadow: Vec<TextShadowLayer>,
    /// Whether `border-{top-left,top-right,bottom-right,bottom-left}-radius` is not its
    /// initial value `0` (either axis).
    pub border_radius_non_initial: [bool; 4],
}

/// What [`prepare_scene`] reads of the styled document besides the fragments.
pub trait PaintStyleSource {
    /// The element's paint style; `None` for an unstyled node.
    fn paint_style(&self, element: NodeId) -> Option<PaintStyle>;

    /// Radii, shadows, overflow, transform, positioning and background layers of the element,
    /// lengths resolved against its border box `(x, y, width, height)` in document px. `opacity` and the
    /// padding are filled by the adapter. The default is "none of them".
    fn paint_extras(&self, element: NodeId, border_box: (f32, f32, f32, f32)) -> BoxExtras {
        let _ = (element, border_box);
        BoxExtras::default()
    }

    /// The paint style of a generated pseudo-element (`::before`, `::after`, `::marker`) of the
    /// element; `None` = unstyled (its text paints in the default colour). The default is "none".
    fn paint_style_pseudo(&self, element: NodeId, kind: PseudoKind) -> Option<PaintStyle> {
        let _ = (element, kind);
        None
    }

    /// `column-rule-style` and `column-rule-color` (`currentColor` resolved) of a multicol container
    /// (015 MC-1); `None` = no rule. The default is "no rule".
    fn column_rule(&self, element: NodeId) -> Option<(ComputedBorderStyle, Color)> {
        let _ = element;
        None
    }

    /// [`paint_extras`](Self::paint_extras) of a generated pseudo-element's box.
    fn paint_extras_pseudo(
        &self,
        element: NodeId,
        kind: PseudoKind,
        border_box: (f32, f32, f32, f32),
    ) -> BoxExtras {
        let _ = (element, kind, border_box);
        BoxExtras::default()
    }

    /// The HTML `body` element when it is a child of the root `html` element (CSS Backgrounds 3
    /// §2.11.2 background propagation); `None` otherwise.
    fn canvas_body(&self) -> Option<NodeId> {
        None
    }

    /// The editing state of a text control: selection, focus and caret blink phase. The default
    /// is "nothing selected, not focused".
    fn edit_state(&self, element: NodeId) -> Option<EditState> {
        let _ = element;
        None
    }

    /// The facts of a form control that the scene draws (lane L-CTRL-r10): the kind and state of a
    /// `select`, `range`, `progress`, `meter`, checkbox, radio or colour input. The default is "not a
    /// control the scene draws".
    /// Used colors and thickness for this element's overlay scrollbars.
    fn scrollbar_style(&self, _element: NodeId) -> Option<crate::scene::inputs::ScrollbarStyle> {
        None
    }

    fn control_facts(&self, element: NodeId) -> Option<crate::scene::inputs::ControlFacts> {
        let _ = element;
        None
    }

    /// The element's DOM text children in tree order, each with its number of non-white-space
    /// characters (used to attribute text fragments to text nodes for hit testing). When empty,
    /// a text hit names the element itself as the text node.
    fn text_nodes(&self, element: NodeId) -> Vec<(NodeId, usize)> {
        let _ = element;
        Vec::new()
    }

    /// Scene reuse (015 PERF-2, [`crate::scene::reuse`]): whether every answer of this source for `node`
    /// (paint styles and extras of the element and its pseudo-elements, column rule, control facts,
    /// edit state, DOM text nodes) equals its answer when the previous scene was prepared with the
    /// same [`crate::scene::reuse::SceneMemo`]. A call also records the current answers for the next scene's
    /// question. The default ("not known") disables reuse.
    fn unchanged(&self, node: NodeId) -> bool {
        let _ = node;
        false
    }
}

/// The editing state of one text control, as the scene needs it (offsets are UTF-16 offsets into
/// the control's value, `start <= end`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditState {
    pub start: u32,
    pub end: u32,
    /// The caret belongs after the end of the previous line (End on a soft-wrapped line).
    pub upstream: bool,
    pub focused: bool,
    /// Blink phase: the caret is in its visible half.
    pub caret_visible: bool,
    /// The IME preedit text, UTF-16 offsets into the value (underlined; lane L-CLIP-r13).
    pub composition: Option<(u32, u32)>,
}

/// Identity of the scene being prepared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneFrame {
    pub generation: u64,
    pub revision: RevisionTuple,
    /// Canvas colour under the propagated background (colour-scheme default).
    pub default_canvas: Color,
}

/// One scene-stage `UNSUPPORTED_FEATURE` report under 005's key (R-20), de-duplicated within
/// one build (same shape as `crate::layout::coordinator::LayoutReport`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneReport {
    pub key: ReportKey,
    pub code: Code,
    pub reason: UnsupportedReason,
}

/// Output of [`prepare_scene`]: the scene input, the font blobs its glyph runs reference, and
/// the scene-stage reports.
#[derive(Clone, Debug)]
pub struct PreparedScene {
    pub input: SceneInput,
    pub fonts: FontTable,
    pub reports: Vec<SceneReport>,
}

impl PreparedScene {
    /// S5: [`build_scene_with`] over this input, fonts and reports.
    pub fn build(&self) -> SceneSnapshot {
        build_scene_with(&self.input, self.fonts.clone(), self.reports.clone())
    }
}

const BORDER_STYLE_FEATURES: [&str; 4] = [
    "css.property.border-top-style",
    "css.property.border-right-style",
    "css.property.border-bottom-style",
    "css.property.border-left-style",
];
const RADIUS_FEATURES: [&str; 4] = [
    "css.property.border-top-left-radius",
    "css.property.border-top-right-radius",
    "css.property.border-bottom-right-radius",
    "css.property.border-bottom-left-radius",
];

fn transparent() -> Color {
    Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    }
}

/// `src` over `dst` (non-premultiplied sRGB).
fn over(src: Color, dst: Color) -> Color {
    let a = src.a + dst.a * (1.0 - src.a);
    if a <= 0.0 {
        return transparent();
    }
    let mix = |s: f32, d: f32| ((s * src.a + d * dst.a * (1.0 - src.a)) / a).clamp(0.0, 1.0);
    Color {
        r: mix(src.r, dst.r),
        g: mix(src.g, dst.g),
        b: mix(src.b, dst.b),
        a,
    }
}

/// An element's text nodes (id, non-white-space characters), the cursor, and the characters
/// consumed of the current node.
type TextCursor = (Vec<(NodeId, usize)>, usize, usize);
/// A glyph run of one line: owner element, first pen x (paragraph space), document-space run.
type LineRun = (NodeId, Option<PseudoKind>, f32, GlyphRun);

struct Adapter<'a, S: PaintStyleSource> {
    tree: &'a FragmentTree,
    styles: &'a S,
    document: ui4me_engine_types::ids::DocumentId,
    /// The element whose background went to the canvas (painted nowhere else).
    propagated: Option<NodeId>,
    paint_radii: bool,
    fonts: FontTable,
    reports: Vec<SceneReport>,
    checked: crate::scene::reuse::FastSet<NodeId>,
    style_cache: HashMap<(NodeId, Option<PseudoKind>), Option<PaintStyle>>,
    /// Per element: its text nodes, the cursor and the characters consumed of the current one.
    text_cursor: HashMap<NodeId, TextCursor>,
    /// Glyph runs per (paragraph, line), positioned in document space, with their owner.
    runs: HashMap<(usize, usize), std::rc::Rc<Vec<LineRun>>>,
    /// Table tracks and collapsed-border segments name their boxes, not elements: the element of a
    /// box, built on first use from the fragment tree's maps.
    box_node: Option<HashMap<crate::layout::box_tree::BoxId, NodeId>>,
    /// Per inline box: its first and last line piece (built on first use).
    piece_span: Option<HashMap<crate::layout::box_tree::BoxId, (FragmentId, FragmentId)>>,
    /// Scroll offsets in force (012 through the pipeline); empty = nothing scrolled.
    scroll: &'a ScrollOffsets,
    /// The colour of the text of a control that shows its placeholder (set while its children are adapted).
    placeholder: Option<Color>,
    /// Translation from document space to viewport space for the subtree being adapted: minus
    /// the offsets of the scroll containers around it, plus sticky shifts; a `fixed` box resets it.
    shift: LayoutPoint,
    /// Scene reuse (015 PERF-2; `crate::scene::reuse`): `None` builds everything and records nothing.
    reuse: Option<crate::scene::reuse::Reuse<'a>>,
}

impl<S: PaintStyleSource> Adapter<'_, S> {
    /// The fragment's border box in viewport space (document space shifted by the scroll state).
    fn drect(&self, id: FragmentId) -> LayoutRect {
        let mut r = self.tree.document_rect(id).rect;
        r.origin = LayoutPoint {
            x: r.origin.x + self.shift.x,
            y: r.origin.y + self.shift.y,
        };
        r
    }

    fn style(&mut self, node: NodeId) -> Option<PaintStyle> {
        self.style_p(node, None)
    }

    // ---- scene reuse (015 PERF-2, `crate::scene::reuse`)

    /// The element-box subtree being recorded, if any.
    fn frame(&mut self) -> Option<&mut crate::scene::reuse::Frame> {
        self.reuse.as_mut().and_then(|r| r.stack.last_mut())
    }

    /// The subtree being recorded reads what the style source answers for `node`.
    fn read(&mut self, node: NodeId) {
        if let Some(f) = self.frame()
            && f.reads.last() != Some(&node)
        {
            f.reads.push(node);
        }
    }

    /// The scroll offset of a scroll container or control (recorded).
    fn scroll_offset(&mut self, node: NodeId) -> (f64, f64) {
        let o = self.scroll.offset(ScrollTarget::Node(node));
        if let Some(f) = self.frame() {
            f.scroll.push((node, o));
        }
        o
    }

    /// [`Self::paint_box`] of an element box, taken over from the previous scene when nothing it was
    /// projected from changed, else built (and recorded for the next scene).
    fn paint_box_shared(&mut self, id: FragmentId) -> Option<std::sync::Arc<PaintBox>> {
        use std::sync::Arc;
        let tree = self.tree;
        let FragmentKind::Box(bf) = &tree.get(id).kind else {
            return None;
        };
        if matches!(bf.owner, BoxOwner::Anonymous(_)) {
            return None;
        }
        // Entries are keyed by box: an element can generate several boxes (a table's wrapper and grid box).
        let key = bf.box_id;
        if self.reuse.is_none() {
            return self.paint_box(id).map(Arc::new);
        }
        if let Some(p) = self.try_reuse(id, key, bf.box_id) {
            return Some(p);
        }
        // Only a box with one fragment is an entry (a broken box's pieces are painted as one box each).
        let candidate = bf.extras.piece.is_none()
            && self
                .reuse
                .as_ref()
                .and_then(|r| r.new_of_box.get(key.0 as usize))
                == Some(&id.0);
        if candidate {
            let f = crate::scene::reuse::Frame::new(
                key,
                bf.box_id,
                id,
                self.shift,
                self.placeholder,
                self.propagated,
            );
            self.reuse.as_mut().expect("reuse").stack.push(f);
        }
        let out = self.paint_box(id).map(Arc::new);
        if candidate {
            let f = self
                .reuse
                .as_mut()
                .expect("reuse")
                .stack
                .pop()
                .expect("the frame pushed above");
            self.finish(f, out.as_ref());
        }
        out
    }

    /// Close a recorded subtree: an entry when every whole-tree fact it read is contained in it,
    /// else its reads go to the enclosing subtree.
    fn finish(&mut self, f: crate::scene::reuse::Frame, paint: Option<&std::sync::Arc<PaintBox>>) {
        let tree = self.tree;
        let contained = !f.volatile
            && f.contain
                .iter()
                .all(|c| crate::scene::reuse::descends(tree, *c, f.root))
            && f.text_owners.iter().all(|o| {
                tree.text_fragments_of(*o)
                    .iter()
                    .all(|t| crate::scene::reuse::descends(tree, t.id, f.root))
            });
        match paint {
            Some(paint) if contained => {
                // Record the nodes' current answers for the next scene's comparison.
                for n in &f.reads {
                    self.styles.unchanged(*n);
                }
                let key = f.key;
                let r = self.reuse.as_mut().expect("reuse");
                r.next
                    .insert(key, std::sync::Arc::new(f.into_entry(paint.clone())));
                r.stats.built += 1;
                if let Some(parent) = r.stack.last_mut() {
                    parent.events.push(crate::scene::reuse::Event::Nested(key));
                }
            }
            _ => {
                if let Some(parent) = self.frame() {
                    parent.absorb(f);
                }
            }
        }
    }

    fn try_reuse(
        &mut self,
        id: FragmentId,
        key: crate::scene::reuse::Key,
        box_id: crate::layout::box_tree::BoxId,
    ) -> Option<std::sync::Arc<PaintBox>> {
        let r = self.reuse.as_mut()?;
        // The old entry leaves the memo here whatever happens: when it is not reused, its subtree is freed
        // before the replacement is built, so the new blocks take the old ones' memory (holding both
        // generations until the end of the scene fragmented glibc's heap; 015 PERF-2).
        let e = r.prev.take(&key)?;
        r.clean.as_ref()?;
        if r.new_of_box.get(key.0 as usize) != Some(&id.0) {
            return None;
        }
        if e.box_id != box_id
            || e.shift != self.shift
            || e.placeholder != self.placeholder
            || e.propagated != self.propagated
        {
            return None;
        }
        if !r.clean.as_mut()?.box_eq(id) {
            return None;
        }
        if !self.inputs_ok_entry(&e) {
            return None;
        }
        let paint = e.paint.clone();
        self.adopt(key, e);
        let r = self.reuse.as_mut()?;
        r.stats.reused += 1;
        if let Some(parent) = r.stack.last_mut() {
            parent.events.push(crate::scene::reuse::Event::Nested(key));
        }
        Some(paint)
    }

    /// Whether every node, scroll offset and track box the remembered entry `key` (and the entries inside
    /// it) read is unchanged; memoised for the scene.
    fn inputs_ok(&mut self, key: &crate::scene::reuse::Key) -> bool {
        let r = self.reuse.as_mut().expect("reuse");
        if let Some(v) = r.inputs_ok.get(key) {
            return *v;
        }
        let Some(e) = r.prev.get(key).cloned() else {
            return false;
        };
        let ok = self.inputs_ok_entry(&e);
        self.reuse
            .as_mut()
            .expect("reuse")
            .inputs_ok
            .insert(*key, ok);
        ok
    }

    fn inputs_ok_entry(&mut self, e: &crate::scene::reuse::Entry) -> bool {
        let ok = e.reads.iter().all(|n| self.styles.unchanged(*n))
            && e.scroll
                .iter()
                .all(|(n, o)| self.scroll.offset(ScrollTarget::Node(*n)) == *o)
            && e.boxes.iter().all(|(b, n)| self.node_of_box_raw(*b) == *n);
        ok && e.events.iter().all(|ev| match ev {
            crate::scene::reuse::Event::Nested(k) => self.inputs_ok(k),
            _ => true,
        })
    }

    /// Take entry `e` (and the entries inside it) over into the next scene's memo and replay its side
    /// effects (reports, consumption checks, font blobs) in build order.
    fn adopt(
        &mut self,
        key: crate::scene::reuse::Key,
        e: std::sync::Arc<crate::scene::reuse::Entry>,
    ) {
        self.reuse
            .as_mut()
            .expect("reuse")
            .next
            .insert(key, e.clone());
        for ev in e.events.iter() {
            match ev {
                crate::scene::reuse::Event::Nested(k) => {
                    if let Some(n) = self.reuse.as_mut().expect("reuse").prev.take(k) {
                        self.adopt(*k, n);
                    }
                }
                crate::scene::reuse::Event::Report(feature, node) => {
                    self.report_raw(feature, *node)
                }
                crate::scene::reuse::Event::Checked(node) => {
                    self.checked.insert(*node);
                }
                crate::scene::reuse::Event::Font(id, bytes) => {
                    self.fonts.insert(*id, bytes.clone())
                }
            }
        }
    }

    /// The paint style of an element or of one of its generated pseudo-elements (FR-086).
    fn style_p(&mut self, node: NodeId, pseudo: Option<PseudoKind>) -> Option<PaintStyle> {
        self.read(node);
        if let Some(s) = self.style_cache.get(&(node, pseudo)) {
            return s.clone();
        }
        let s = match pseudo {
            None => self.styles.paint_style(node),
            Some(k) => self.styles.paint_style_pseudo(node, k),
        };
        self.style_cache.insert((node, pseudo), s.clone());
        s
    }

    fn report(&mut self, feature: &'static str, node: NodeId) {
        if let Some(f) = self.frame() {
            f.events
                .push(crate::scene::reuse::Event::Report(feature, node));
        }
        self.report_raw(feature, node);
    }

    fn report_raw(&mut self, feature: &'static str, node: NodeId) {
        let Some(feature) = FeatureId::new(feature) else {
            return;
        };
        let key = ReportKey {
            feature,
            stage: Stage::Scene,
            document: self.document,
            location: ReportLocation::Element { node },
        };
        if !self.reports.iter().any(|r| r.key == key) {
            self.reports.push(SceneReport {
                key,
                code: Code::UnsupportedFeature,
                reason: UnsupportedReason::NotConsumedByStage,
            });
        }
    }

    /// Paint-stage consumption checks (FR-022): each unconsumed non-initial value is treated
    /// as its initial value and reported once per (element, property). `boxed` = the element
    /// has a block box (inline elements' borders are reported by layout, E-D10).
    fn check(&mut self, node: NodeId, style: &PaintStyle, boxed: bool) {
        if !self.checked.insert(node) {
            return;
        }
        if let Some(f) = self.frame() {
            f.events.push(crate::scene::reuse::Event::Checked(node));
        }
        if !self.paint_radii {
            for (i, non_initial) in style.border_radius_non_initial.iter().enumerate() {
                if *non_initial {
                    self.report(RADIUS_FEATURES[i], node);
                }
            }
        }
        if boxed {
            for (i, s) in style.border_style.iter().enumerate() {
                if s.consumed().is_none() {
                    self.report(BORDER_STYLE_FEATURES[i], node);
                }
            }
        }
        // An inline element's background and borders are painted from its line pieces
        // (`inline_piece_box`). `visibility: hidden | collapse` is applied where the boxes, text
        // and decorations are made (`hide_own_painting`, `text`, `children_of`).
    }

    /// Whether the element (or pseudo-element) paints: `visibility: visible` (an unstyled node does).
    fn is_visible(&mut self, node: NodeId, pseudo: Option<PseudoKind>) -> bool {
        self.style_p(node, pseudo).is_none_or(|s| s.visible)
    }

    fn paint_box(&mut self, id: FragmentId) -> Option<PaintBox> {
        let f = self.tree.get(id);
        let FragmentKind::Box(bf) = &f.kind else {
            return None;
        };
        // A generated box (`::before`, `::after`, `::marker`) paints with its pseudo-element's style
        // and hits as its originating element.
        let (node, pseudo) = match bf.owner {
            BoxOwner::Element(n) => (n, None),
            BoxOwner::Pseudo(n, k) => (n, Some(k)),
            BoxOwner::Anonymous(_) => return None,
        };
        // The anonymous image of a generated `content: url()` paints the picture only.
        let generated_image = bf.extras.generated_image;
        self.read(node);
        let style = if generated_image {
            None
        } else {
            self.style_p(node, pseudo)
        };
        if let Some(s) = &style {
            self.check(node, s, true);
        }
        let background = match &style {
            Some(s)
                if (pseudo.is_some() || Some(node) != self.propagated)
                    && s.background_color.a > 0.0 =>
            {
                Some(s.background_color)
            }
            _ => None,
        };
        let w = [
            bf.border_widths.top,
            bf.border_widths.right,
            bf.border_widths.bottom,
            bf.border_widths.left,
        ];
        let side = |i: usize| match &style {
            Some(s) => BorderSide {
                width: w[i],
                style: s.border_style[i].consumed().unwrap_or(BorderStyle::None),
                color: s.border_color[i],
            },
            None => BorderSide {
                width: w[i],
                style: BorderStyle::None,
                color: transparent(),
            },
        };
        let mut border = BorderSides {
            top: side(0),
            right: side(1),
            bottom: side(2),
            left: side(3),
        };
        // A fieldset with a rendered legend (015 HTML-3): layout saw no block-start border; paint draws it.
        if let Some(bt) = bf.extras.fieldset_border_top
            && let Some(s) = &style
        {
            border.top = BorderSide {
                width: LayoutUnit::quantize(f64::from(bt)).value,
                style: s.border_style[0].consumed().unwrap_or(BorderStyle::None),
                color: s.border_color[0],
            };
        }
        let mut background = background;
        // Tables (FR-080, FR-081): in the collapsed model the borders are painted from the winning
        // segments (a table with segments, and its cells, draw none of their own); `empty-cells:
        // hide` removes the background and borders of an empty cell.
        let in_collapsed_table = bf.extras.collapsed_borders.is_some()
            || (bf.extras.table_cell.is_some()
                && f.parent.is_some_and(|p| matches!(&self.tree.get(p).kind, FragmentKind::Box(pb) if pb.extras.collapsed_borders.is_some())));
        let hide_cell = bf.extras.table_cell.is_some_and(|c| c.empty_cells_hidden);
        if in_collapsed_table || hide_cell {
            for s in [
                &mut border.top,
                &mut border.right,
                &mut border.bottom,
                &mut border.left,
            ] {
                s.style = BorderStyle::None;
            }
        }
        if hide_cell {
            background = None;
        }
        // Scrolling (FR-066: paint and hit testing only, layout is untouched): a `fixed` box
        // ignores the scroll of its ancestors, a sticky box is shifted by its constraint, and
        // the content of a scroll container moves by minus its offset.
        let saved_shift = self.shift;
        if bf.extras.position == PositionValue::Fixed {
            self.shift = LayoutPoint::default();
            if bf.extras.anchor_node.is_some() {
                // An anchored box follows the scroll of the containers around its anchor.
                if let Some(f) = self.frame() {
                    f.volatile = true;
                }
                let (dx, dy) = crate::layout::coordinator::sticky::anchor_scroll_shift(
                    self.tree,
                    self.scroll,
                    id,
                );
                let q = |v: f64| LayoutUnit::quantize(v).value;
                self.shift = LayoutPoint { x: q(dx), y: q(dy) };
            }
        }
        if bf.extras.sticky.is_some()
            && let Some(f) = self.frame()
        {
            // A sticky shift reads the offsets of every scroll container around the box.
            f.volatile = true;
        }
        if bf.extras.sticky.is_some()
            && let Ok((dx, dy)) = sticky_shift(self.tree, self.scroll, id)
        {
            let q = |v: f64| LayoutUnit::quantize(v).value;
            self.shift = LayoutPoint {
                x: self.shift.x + q(dx),
                y: self.shift.y + q(dy),
            };
        }
        let border_box = self.drect(id);
        if bf.extras.is_scroll_container {
            let (ox, oy) = self.scroll_offset(node);
            let q = |v: f64| LayoutUnit::quantize(v).value;
            self.shift = LayoutPoint {
                x: self.shift.x - q(ox),
                y: self.shift.y - q(oy),
            };
        }
        let mut children = Vec::new();
        let saved_placeholder = self.placeholder;
        self.placeholder = bf
            .extras
            .control_inner
            .as_ref()
            .is_some_and(|c| c.placeholder_shown)
            .then_some(crate::scene::theme::PLACEHOLDER);
        self.children_of(id, &mut children);
        self.placeholder = saved_placeholder;
        let own_scroll = if bf.extras.control_inner.is_some() {
            self.scroll_offset(node)
        } else {
            (0.0, 0.0)
        };
        self.shift = saved_shift;
        // A piece of a broken box (015 FRAG-2b) styles its decoration as the whole box.
        let bb = match (bf.extras.piece, bf.extras.piece_whole) {
            // A vertical writing mode (015 WM-1c): the whole box is given physically.
            (Some(_), Some(w)) => (
                lu(border_box.origin.x + w.origin.x),
                lu(border_box.origin.y + w.origin.y),
                lu(w.size.width),
                lu(w.size.height),
            ),
            (Some(p), None) => (
                lu(border_box.origin.x),
                lu(border_box.origin.y - p.block_offset),
                lu(border_box.size.width),
                lu(p.whole_block_size),
            ),
            (None, _) => (
                lu(border_box.origin.x),
                lu(border_box.origin.y),
                lu(border_box.size.width),
                lu(border_box.size.height),
            ),
        };
        let mut extras = match pseudo {
            None => self.styles.paint_extras(node, bb),
            Some(k) => self.styles.paint_extras_pseudo(node, k, bb),
        };
        if matches!(pseudo, Some(PseudoKind::Before | PseudoKind::After)) {
            // The picture of `content: url()` belongs to its anonymous image box, not to the
            // pseudo-element's own box; the image box has no styling of its own.
            if generated_image {
                extras = BoxExtras {
                    replaced: extras.replaced,
                    unsupported: extras.unsupported,
                    ..BoxExtras::default()
                };
            } else {
                extras.replaced = None;
            }
        }
        if !self.paint_radii {
            extras.radii = [(0.0, 0.0); 4];
        }
        if let Some(s) = &style {
            extras.opacity = s.opacity;
        }
        if hide_cell {
            extras.layers.clear();
            extras.shadows.clear();
        }
        if let Some(facts) = self.styles.control_facts(node) {
            let fg = style.as_ref().map(|s| s.color).unwrap_or(Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            let own_bg = style
                .as_ref()
                .map(|s| s.background_color)
                .filter(|c| c.a >= 0.99);
            let bg = own_bg.unwrap_or(if crate::scene::theme::is_light(fg) {
                Color {
                    r: 0.17,
                    g: 0.17,
                    b: 0.17,
                    a: 1.0,
                }
            } else {
                Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }
            });
            let bb = rect_of(&border_box);
            let cr = bf.content_rect;
            let content = Rect {
                x: bb.x + lu(cr.origin.x),
                y: bb.y + lu(cr.origin.y),
                width: lu(cr.size.width),
                height: lu(cr.size.height),
            };
            let mut paint = crate::scene::inputs::ControlPaint {
                facts,
                fg,
                bg,
                accent: style
                    .as_ref()
                    .and_then(|s| s.accent_color)
                    .unwrap_or(crate::scene::theme::ACCENT),
                border_box: bb,
                content,
                panel: None,
                rows: Vec::new(),
                listbox: false,
            };
            if let Some(sel) = bf
                .extras
                .control_inner
                .as_ref()
                .and_then(|ci| ci.select.as_ref())
            {
                let at = |x: f32, y: f32, w: f32, h: f32| Rect {
                    x: bb.x + x,
                    y: bb.y + y,
                    width: w,
                    height: h,
                };
                paint.listbox = !sel.popup;
                if sel.popup {
                    paint.panel = Some(at(sel.panel[0], sel.panel[1], sel.panel[2], sel.panel[3]));
                    // An open popup floats above its siblings: its own top-most stacking context.
                    extras.class = PaintClass::Positioned;
                    extras.z_index = Some(POPUP_Z_INDEX);
                    extras.clip_x = false;
                    extras.clip_y = false;
                }
                let row_scroll = if sel.popup || !bf.extras.is_scroll_container {
                    (0.0, 0.0)
                } else {
                    (own_scroll.0 as f32, own_scroll.1 as f32)
                };
                paint.rows = sel
                    .rows
                    .iter()
                    .map(|r| crate::scene::inputs::SelectRowPaint {
                        rect: at(r.x - row_scroll.0, r.y - row_scroll.1, r.width, r.height),
                        selected: r.selected,
                        highlighted: r.highlighted,
                        disabled: r.disabled,
                    })
                    .collect();
            }
            extras.control = Some(Box::new(paint));
        }
        if let Some(ci) = &bf.extras.control_inner {
            extras.clip_content_box = true;
            let text_color = style.as_ref().map(|s| s.color).unwrap_or(Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            if ci.select.is_none()
                && !matches!(self.styles.control_facts(node), Some(f) if f.kind == crate::scene::inputs::ControlKind::Select)
            {
                extras.edit = Some(Box::new(
                    self.edit_overlay(node, border_box, ci, own_scroll, text_color),
                ));
            }
        }
        for feature in std::mem::take(&mut extras.unsupported) {
            self.report(feature, node);
        }
        // Layout-published facts (008 fragment extras): overflow modes and out-of-flow placement.
        let layout = &bf.extras;
        // The top layer (015 HTML-1; CSS Position 4 section 3): each box is a stacking context of its own,
        // painted after every other content of the root context, in top-layer order (a `::backdrop` just
        // below its element).
        if let Some(order) = layout.top_layer {
            extras.class = PaintClass::Positioned;
            extras.top_layer = Some(order);
            extras.forms_stacking_context = true;
        }
        extras.clip_x |= layout.overflow_x != OverflowMode::Visible;
        extras.clip_y |= layout.overflow_y != OverflowMode::Visible;
        if bf.extras.is_float && layout.position == PositionValue::Static {
            extras.class = PaintClass::Float;
        }
        // `position` other than `static` (relative, sticky included) paints at Appendix E step 8
        // like an out-of-flow box (a sticky header overlays the content scrolling behind it).
        if (layout.containing_block.is_some() || layout.position != PositionValue::Static)
            && extras.class == PaintClass::InFlow
        {
            extras.class = PaintClass::Positioned;
        }
        if bf.extras.is_scroll_container
            && pseudo.is_none()
            && let (Some(sp), Some(range), Some(style)) = (
                bf.extras.scrollport,
                crate::layout::coordinator::overflow::scroll_range(
                    self.tree,
                    ScrollTarget::Node(node),
                ),
                self.styles.scrollbar_style(node),
            )
            && style.width > 0.0
        {
            let port = Rect {
                x: lu(border_box.origin.x + sp.origin.x),
                y: lu(border_box.origin.y + sp.origin.y),
                width: lu(sp.size.width),
                height: lu(sp.size.height),
            };
            let offset = self.scroll.offset(ScrollTarget::Node(node));
            extras.scrollbars = crate::scene::paint::scrollbar::layout(
                port,
                range,
                offset,
                style,
                layout.overflow_x,
                layout.overflow_y,
            );
        }
        extras.padding = [
            lu(bf.padding.top),
            lu(bf.padding.right),
            lu(bf.padding.bottom),
            lu(bf.padding.left),
        ];
        extras.margin = [
            lu(bf.margins.top),
            lu(bf.margins.right),
            lu(bf.margins.bottom),
            lu(bf.margins.left),
        ];
        extras.tree_order = bf.box_id.0;
        // 008's resolved `object-fit` rectangle is relative to the border box.
        if let Some(m) = layout.marker_symbol {
            let c = style.as_ref().map(|s| s.color).unwrap_or(Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            let r = m.bullet;
            extras.marker_symbol = Some(crate::scene::inputs::MarkerSymbolPaint {
                shape: m.shape,
                rect: Rect {
                    x: lu(border_box.origin.x) + lu(r.origin.x),
                    y: lu(border_box.origin.y) + lu(r.origin.y),
                    width: lu(r.size.width),
                    height: lu(r.size.height),
                },
                color: c,
            });
        }
        if !layout.column_rules.is_empty()
            && pseudo.is_none()
            && let Some((rs, color)) = self.styles.column_rule(node)
            && let Some(st) = rs.consumed()
        {
            for r in &layout.column_rules {
                let rect = Rect {
                    x: lu(border_box.origin.x) + lu(r.origin.x),
                    y: lu(border_box.origin.y) + lu(r.origin.y),
                    width: lu(r.size.width),
                    height: lu(r.size.height),
                };
                extras.column_rules.push((
                    rect,
                    BorderSide {
                        width: r.size.width,
                        style: st,
                        color,
                    },
                ));
            }
        }
        if let Some(r) = layout.replaced_content_rect {
            extras.replaced_content_rect = Some(Rect {
                x: lu(border_box.origin.x) + lu(r.origin.x),
                y: lu(border_box.origin.y) + lu(r.origin.y),
                width: lu(r.size.width),
                height: lu(r.size.height),
            });
        }
        if style.as_ref().is_some_and(|s| !s.visible) {
            hide_own_painting(&mut background, &mut border, &mut extras);
        }
        if extras.clips_to_text() {
            collect_text_runs(&children, &mut extras.text_clip);
        }
        // A fieldset with a rendered legend (015 HTML-3; HTML 15.3.13): the border box is decorated from the
        // block-start border centred on the legend (when the legend is the taller), and the legend's gap is
        // cut out of that border; the whole border box stays the hit region.
        let mut border_box = border_box;
        if let Some(bt) = layout.fieldset_border_top
            && let Some(l) = self.fieldset_legend_rect(id)
        {
            let top = border_box.origin.y.to_f64();
            let (ly, lh) = (l.origin.y.to_f64(), l.size.height.to_f64());
            let offset = (ly + lh / 2.0 - f64::from(bt) / 2.0 - top)
                .max(0.0)
                .min(border_box.size.height.to_f64());
            extras.hit_rect = Some(rect_of(&border_box));
            let off = LayoutUnit::quantize(offset).value;
            border_box = LayoutRect {
                origin: LayoutPoint {
                    x: border_box.origin.x,
                    y: border_box.origin.y + off,
                },
                size: ui4me_engine_types::geometry::LayoutSize {
                    width: border_box.size.width,
                    height: border_box.size.height - off,
                },
            };
            extras.fieldset_gap = Some([lu(l.origin.x), lu(l.origin.x + l.size.width)]);
        }
        // A piece of a box broken across fragmentainers (015 FRAG-2b): the box paints as the whole box
        // laid out unbroken at the piece's place, cut to the piece (`box-decoration-break: slice`). The
        // edges of the piece at a break carry no border (layout drops them).
        if let Some(p) = layout.piece {
            extras.fragment_clip = Some(rect_of(&border_box));
            border_box = match layout.piece_whole {
                Some(w) => LayoutRect {
                    origin: LayoutPoint {
                        x: border_box.origin.x + w.origin.x,
                        y: border_box.origin.y + w.origin.y,
                    },
                    size: w.size,
                },
                None => LayoutRect {
                    origin: LayoutPoint {
                        x: border_box.origin.x,
                        y: border_box.origin.y - p.block_offset,
                    },
                    size: ui4me_engine_types::geometry::LayoutSize {
                        width: border_box.size.width,
                        height: p.whole_block_size,
                    },
                },
            };
        }
        Some(PaintBox {
            node,
            border_box,
            background,
            border,
            extras,
            children,
        })
    }

    /// The selection rectangles and the caret of a text control in viewport space (the control's
    /// own scroll offset `scroll` applied). The selection and the caret show in a focused control only.
    fn edit_overlay(
        &self,
        node: NodeId,
        border_box: LayoutRect,
        ci: &ControlInner,
        scroll: (f64, f64),
        text_color: Color,
    ) -> EditOverlay {
        let (ox, oy) = (
            lu(border_box.origin.x) + lu(ci.viewport.origin.x) - scroll.0 as f32,
            lu(border_box.origin.y) + lu(ci.viewport.origin.y) - scroll.1 as f32,
        );
        let mut overlay = EditOverlay {
            selection: Vec::new(),
            selection_color: crate::scene::theme::SELECTION_BACKGROUND,
            caret: None,
            caret_color: text_color,
            placeholder_color: ci
                .placeholder_shown
                .then_some(crate::scene::theme::PLACEHOLDER),
            preedit: Vec::new(),
        };
        let Some(st) = self.styles.edit_state(node).filter(|s| s.focused) else {
            return overlay;
        };
        if st.start != st.end {
            for (li, l) in ci.lines.iter().enumerate() {
                let (a, b) = (st.start.max(l.start), st.end.min(l.end));
                if a < b {
                    for (x0, x1) in ci.range_spans(li, a, b) {
                        overlay.selection.push(Rect {
                            x: ox + x0,
                            y: oy + l.top,
                            width: x1 - x0,
                            height: l.height,
                        });
                    }
                }
            }
        } else if st.caret_visible {
            let li = ci.line_of(st.start, st.upstream);
            let l = &ci.lines[li];
            overlay.caret = Some(Rect {
                x: ox + ci.x_of(li, st.start),
                y: oy + l.top,
                width: crate::scene::theme::CARET_WIDTH,
                height: l.height,
            });
        }
        // The preedit: a solid underline along the bottom of its text on every line it covers.
        if let Some((cs, ce)) = st.composition.filter(|(a, b)| a < b) {
            for (li, l) in ci.lines.iter().enumerate() {
                let (a, b) = (cs.max(l.start), ce.min(l.end));
                if a < b {
                    for (x0, x1) in ci.range_spans(li, a, b) {
                        overlay.preedit.push(Rect {
                            x: ox + x0,
                            y: oy + l.top + l.height - crate::scene::theme::PREEDIT_UNDERLINE_WIDTH,
                            width: x1 - x0,
                            height: crate::scene::theme::PREEDIT_UNDERLINE_WIDTH,
                        });
                    }
                }
            }
        }
        overlay
    }

    /// The border box (viewport space) of the rendered legend of fieldset fragment `id` (015 HTML-3): the
    /// first box of its first child, the anonymous legend area.
    fn fieldset_legend_rect(&self, id: FragmentId) -> Option<LayoutRect> {
        let area = *self.tree.get(id).children.first()?;
        let legend = self.tree.get(area).children.iter().copied().find(|&c| matches!(&self.tree.get(c).kind, FragmentKind::Box(b) if matches!(b.owner, BoxOwner::Element(_))))?;
        Some(self.drect(legend))
    }

    /// Children of box fragment `id` in tree order; anonymous block boxes are flattened into
    /// their parent (they paint nothing and hit-test as their containing element).
    fn children_of(&mut self, id: FragmentId, out: &mut Vec<PaintChild>) {
        let f = self.tree.get(id);
        let FragmentKind::Box(bf) = &f.kind else {
            return;
        };
        let doc = self.drect(id);
        let content = LayoutPoint {
            x: doc.origin.x + bf.content_rect.origin.x,
            y: doc.origin.y + bf.content_rect.origin.y,
        };
        // Whether a positioned cell is among the children (asked by every collapsed-border segment; once).
        let mut above_cells_memo: Option<bool> = None;
        for &c in &f.children {
            match &self.tree.get(c).kind {
                FragmentKind::Box(cb) => match cb.owner {
                    BoxOwner::Element(_) | BoxOwner::Pseudo(..) => {
                        out.extend(self.paint_box_shared(c).map(PaintChild::Shared))
                    }
                    // An anonymous clipping box of no height (the content slot of a closed `details`) paints
                    // nothing and is not hit (015 HTML-2).
                    BoxOwner::Anonymous(_)
                        if cb.extras.overflow_y != OverflowMode::Visible
                            && self.tree.get(c).size.height == LayoutUnit::ZERO => {}
                    BoxOwner::Anonymous(_) => self.children_of(c, out),
                },
                FragmentKind::Line(_) => {
                    // Underlines and overlines go under the glyphs, line-throughs over them.
                    let line = self.drect(c);
                    let tree = self.tree;
                    let decos: Vec<PaintDecoration> = tree
                        .decorations_of(c)
                        .iter()
                        .map(|d| PaintDecoration {
                            owner: d.owner,
                            kind: d.kind,
                            style: d.style,
                            // The decorating box's `text-decoration-color` (L-PERF-b3).
                            color: self.style_p(d.owner, d.owner_pseudo).map_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, |s| s.decoration_color),
                            rect: Rect { x: lu(line.origin.x + d.origin.x), y: lu(line.origin.y + d.origin.y), width: lu(d.size.width), height: lu(d.size.height) },
                            thickness: lu(d.thickness),
                            shadows: self.style_p(d.owner, d.owner_pseudo).map(|s| s.text_shadow).unwrap_or_default(),
                            turn: match &tree.get(c).kind {
                                FragmentKind::Line(lf) => match lf.vertical.map(|v| v.orientation) {
                                    Some(crate::layout::writing_mode::LineOrientation::Clockwise) => crate::scene::display_list::GlyphRotation::Clockwise,
                                    Some(crate::layout::writing_mode::LineOrientation::CounterClockwise) => crate::scene::display_list::GlyphRotation::CounterClockwise,
                                    _ => crate::scene::display_list::GlyphRotation::None,
                                },
                                _ => crate::scene::display_list::GlyphRotation::None,
                            },
                        })
                        .collect();
                    // Lines of a hidden decorating element are not drawn.
                    let decos: Vec<PaintDecoration> = decos
                        .into_iter()
                        .filter(|d| self.is_visible(d.owner, None))
                        .collect();
                    // Inline boxes' backgrounds and borders (one piece per box and line, outer first)
                    // go under the text and its decorations.
                    let pieces: Vec<FragmentId> = self
                        .tree
                        .get(c)
                        .children
                        .iter()
                        .copied()
                        .filter(|t| {
                            matches!(self.tree.get(*t).kind, FragmentKind::InlineBoxPiece(_))
                        })
                        .collect();
                    let piece_start = out.len();
                    for t in pieces {
                        out.extend(
                            self.inline_piece_box(t)
                                .map(|b| PaintChild::Box(Box::new(b))),
                        );
                    }
                    out.extend(
                        decos
                            .iter()
                            .filter(|d| !crate::scene::paint::text::after_text(d.kind))
                            .cloned()
                            .map(PaintChild::Decoration),
                    );
                    let text_start = out.len();
                    for &t in &self.tree.get(c).children {
                        self.text(t, content, out);
                    }
                    // `background-clip: text` on an inline box: the glyphs of this line that lie inside the piece
                    // (its own text and that of the inline boxes within it).
                    let (pieces_part, text_part) = out.split_at_mut(text_start);
                    for child in &mut pieces_part[piece_start..] {
                        let PaintChild::Box(pb) = child else { continue };
                        if !pb.extras.clips_to_text() {
                            continue;
                        }
                        let (left, right) = (
                            lu(pb.border_box.origin.x),
                            lu(pb.border_box.origin.x + pb.border_box.size.width),
                        );
                        for t in text_part.iter() {
                            let PaintChild::Text(t) = t else { continue };
                            let glyphs: Vec<Glyph> = t
                                .run
                                .glyphs
                                .iter()
                                .filter(|g| g.x >= left - 0.01 && g.x < right - 0.01)
                                .copied()
                                .collect();
                            if !glyphs.is_empty() {
                                pb.extras.text_clip.push(GlyphRun {
                                    glyphs,
                                    ..t.run.clone()
                                });
                            }
                        }
                    }
                    out.extend(
                        decos
                            .into_iter()
                            .filter(|d| crate::scene::paint::text::after_text(d.kind))
                            .map(PaintChild::Decoration),
                    );
                }
                FragmentKind::Text(_) => self.text(c, content, out),
                FragmentKind::TableTrack(t) => out.extend(
                    self.track_box(
                        c,
                        t.box_id,
                        t.kind == crate::layout::fragment::TableTrackKind::RowOverhang,
                    )
                    .map(|b| PaintChild::Box(Box::new(b))),
                ),
                FragmentKind::CollapsedBorderSegment(s) => {
                    // A positioned cell paints at Appendix E step 8, over the table's own borders; the
                    // collapsed borders then go there too, after every cell (WPT
                    // collapsed-border-positioned-tr-td).
                    let tree = self.tree;
                    let above_cells = *above_cells_memo.get_or_insert_with(|| {
                        f.children.iter().any(|k| matches!(&tree.get(*k).kind, FragmentKind::Box(kb) if kb.extras.table_cell.is_some() && kb.extras.position != PositionValue::Static))
                    });
                    let mut seg = self.segment_box(c, bf.owner.node(), s);
                    if above_cells && let PaintChild::Box(pb) = &mut seg {
                        pb.extras.class = PaintClass::Positioned;
                        pb.extras.tree_order = u32::MAX;
                    }
                    out.push(seg);
                }
                // A fragmentainer (015 FRAG-2b) paints nothing itself: its content (the flow thread's piece)
                // paints in the multicol container's flow.
                FragmentKind::Fragmentainer(_) => {
                    for &k in &self.tree.get(c).children {
                        match &self.tree.get(k).kind {
                            FragmentKind::Box(kb) if matches!(kb.owner, BoxOwner::Anonymous(_)) => {
                                self.children_of(k, out)
                            }
                            FragmentKind::Box(_) => {
                                out.extend(self.paint_box_shared(k).map(PaintChild::Shared))
                            }
                            _ => {}
                        }
                    }
                }
                // 008 T012 variants: produced by later layout tasks, painted by 010 when they land.
                FragmentKind::InlineBoxPiece(_) | FragmentKind::Marker(_) => {}
            }
        }
    }

    /// An inline box's piece on one line (CSS 2.1 section 8.1, 10.8): its background and borders
    /// around the content area, with the padding and border of the edges the piece has (a box
    /// that continues past the line paints no border on that side: `box-decoration-break:
    /// slice`).
    fn inline_piece_box(&mut self, id: FragmentId) -> Option<PaintBox> {
        let f = self.tree.get(id);
        let FragmentKind::InlineBoxPiece(p) = &f.kind else {
            return None;
        };
        if !p.paints {
            // A plain box: its piece exists for the element's geometry only.
            return None;
        }
        let (node, pseudo) = match p.owner {
            BoxOwner::Element(n) => (n, None),
            BoxOwner::Pseudo(n, k) => (n, Some(k)),
            BoxOwner::Anonymous(_) => return None,
        };
        let (pad, bw, start, end, box_id) = (
            p.padding,
            p.border_widths,
            p.has_start_edge,
            p.has_end_edge,
            p.box_id,
        );
        let style = self.style_p(node, pseudo)?;
        self.check(node, &style, true);
        let zero = LayoutUnit::ZERO;
        let content = self.drect(id);
        // Which sides carry the piece's padding and border (the line's start and end sides are cut where the
        // box continues; in a vertical line those are top and bottom, 015 WM-1).
        let on = self.tree.piece_sides(id);
        let _ = (start, end);
        let l = if on[3] { pad.left + bw.left } else { zero };
        let r = if on[1] { pad.right + bw.right } else { zero };
        let t = if on[0] { pad.top + bw.top } else { zero };
        let b = if on[2] { pad.bottom + bw.bottom } else { zero };
        let border_box = LayoutRect {
            origin: LayoutPoint {
                x: content.origin.x - l,
                y: content.origin.y - t,
            },
            size: ui4me_engine_types::geometry::LayoutSize {
                width: content.size.width + l + r,
                height: content.size.height + t + b,
            },
        };
        let width = [
            if on[0] { bw.top } else { zero },
            if on[1] { bw.right } else { zero },
            if on[2] { bw.bottom } else { zero },
            if on[3] { bw.left } else { zero },
        ];
        let side = |i: usize| BorderSide {
            width: width[i],
            style: style.border_style[i]
                .consumed()
                .unwrap_or(BorderStyle::None),
            color: style.border_color[i],
        };
        let border = BorderSides {
            top: side(0),
            right: side(1),
            bottom: side(2),
            left: side(3),
        };
        let background = (style.background_color.a > 0.0).then_some(style.background_color);
        let bb = (
            lu(border_box.origin.x),
            lu(border_box.origin.y),
            lu(border_box.size.width),
            lu(border_box.size.height),
        );
        let mut extras = match pseudo {
            None => self.styles.paint_extras(node, bb),
            Some(k) => self.styles.paint_extras_pseudo(node, k, bb),
        };
        if !self.paint_radii {
            extras.radii = [(0.0, 0.0); 4];
        }
        if pseudo.is_some() {
            // The picture of `content: url()` belongs to the anonymous image box inside the
            // pseudo-element's inline box, not to the inline box's own background area.
            extras.replaced = None;
        }
        for feature in std::mem::take(&mut extras.unsupported) {
            self.report(feature, node);
        }
        extras.hit_transparent = true;
        extras.tree_order = box_id.0;
        extras.slice_open = self.slice_open(box_id, id);
        let (mut background, mut border) = (background, border);
        if !style.visible {
            hide_own_painting(&mut background, &mut border, &mut extras);
        }
        Some(PaintBox {
            node,
            border_box,
            background,
            border,
            extras,
            children: Vec::new(),
        })
    }

    /// Whether the inline box's piece `id` continues on another line: `[before, after]`, from the first and last
    /// piece of the box in line order (`box-decoration-break: slice`).
    fn slice_open(&mut self, box_id: crate::layout::box_tree::BoxId, id: FragmentId) -> [bool; 2] {
        let tree = self.tree;
        let span = self.piece_span.get_or_insert_with(|| {
            let mut m: HashMap<crate::layout::box_tree::BoxId, (FragmentId, FragmentId)> =
                HashMap::default();
            for f in tree.preorder() {
                if let FragmentKind::InlineBoxPiece(p) = &tree.get(f).kind {
                    m.entry(p.box_id).and_modify(|e| e.1 = f).or_insert((f, f));
                }
            }
            m
        });
        let r = span.get(&box_id).copied();
        if let Some((first, last)) = r
            && let Some(f) = self.frame()
        {
            f.contain.push(first);
            f.contain.push(last);
        }
        r.map_or([false; 2], |(first, last)| [first != id, last != id])
    }

    /// The element a box belongs to (generated boxes have none).
    fn node_of_box(&mut self, b: crate::layout::box_tree::BoxId) -> Option<NodeId> {
        let n = self.node_of_box_raw(b);
        if let Some(f) = self.frame() {
            f.boxes.push((b, n));
        }
        if let Some(n) = n {
            self.read(n);
        }
        n
    }

    fn node_of_box_raw(&mut self, b: crate::layout::box_tree::BoxId) -> Option<NodeId> {
        let tree = self.tree;
        let map = self.box_node.get_or_insert_with(|| {
            let mut m = HashMap::default();
            for (n, boxes) in &tree.maps.node_to_boxes {
                for b in boxes {
                    m.insert(*b, *n);
                }
            }
            m
        });
        map.get(&b).copied()
    }

    /// A table column, column group, row or row group (`TableTrack`, FR-081) as a box that paints
    /// the element's background (colour and images) over the track's rectangle, before the cells.
    /// `None` when the track has no element or paints nothing.
    fn track_box(
        &mut self,
        id: FragmentId,
        b: Option<crate::layout::box_tree::BoxId>,
        colour_only: bool,
    ) -> Option<PaintBox> {
        let node = self.node_of_box(b?)?;
        let style = self.style(node)?;
        self.check(node, &style, true);
        let border_box = self.drect(id);
        let mut extras = self.styles.paint_extras(
            node,
            (
                lu(border_box.origin.x),
                lu(border_box.origin.y),
                lu(border_box.size.width),
                lu(border_box.size.height),
            ),
        );
        if !self.paint_radii {
            extras.radii = [(0.0, 0.0); 4];
        }
        extras.opacity = style.opacity;
        for feature in std::mem::take(&mut extras.unsupported) {
            self.report(feature, node);
        }
        if colour_only {
            extras.layers.clear();
            extras.shadows.clear();
        }
        let background = (style.background_color.a > 0.0).then_some(style.background_color);
        if !style.visible
            || (background.is_none() && extras.layers.is_empty() && extras.shadows.is_empty())
        {
            return None;
        }
        extras.hit_transparent = true;
        let none = BorderSide {
            width: LayoutUnit::ZERO,
            style: BorderStyle::None,
            color: transparent(),
        };
        Some(PaintBox {
            node,
            border_box,
            background,
            border: BorderSides {
                top: none,
                right: none,
                bottom: none,
                left: none,
            },
            extras,
            children: Vec::new(),
        })
    }

    /// A collapsed-border segment (`CollapsedBorderSegment`, FR-080) as a box with one border side
    /// of the winning style, colour and width over the segment's rectangle.
    fn segment_box(
        &mut self,
        id: FragmentId,
        table: NodeId,
        s: &crate::layout::fragment::CollapsedBorderSegment,
    ) -> PaintChild {
        use crate::layout::fragment::BorderLineStyle as L;
        let style = match s.style {
            L::None | L::Hidden => BorderStyle::None,
            L::Solid => BorderStyle::Solid,
            L::Dotted => BorderStyle::Dotted,
            L::Dashed => BorderStyle::Dashed,
            L::Double => BorderStyle::Double,
            L::Groove => BorderStyle::Groove,
            L::Ridge => BorderStyle::Ridge,
            L::Inset => BorderStyle::Inset,
            L::Outset => BorderStyle::Outset,
        };
        // The winner's computed `border-<side>-color`, at the 8-bit sRGB precision the table
        // resolution used to carry; an anonymous table box has the initial colour (black).
        use crate::layout::box_tree::OwnerKey;
        let computed = match s.origin_owner {
            Some(OwnerKey::Element(n)) => self.style_p(n, None),
            Some(OwnerKey::Pseudo(n, k)) => self.style_p(n, Some(k)),
            _ => None,
        };
        let c = computed.map_or([0, 0, 0, 255], |st| {
            let c = st.border_color[usize::from(s.origin_side.min(3))];
            [c.r, c.g, c.b, c.a].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        });
        let color = Color {
            r: f32::from(c[0]) / 255.0,
            g: f32::from(c[1]) / 255.0,
            b: f32::from(c[2]) / 255.0,
            a: f32::from(c[3]) / 255.0,
        };
        let none = BorderSide {
            width: LayoutUnit::ZERO,
            style: BorderStyle::None,
            color: transparent(),
        };
        let line = BorderSide {
            width: s.width,
            style,
            color,
        };
        let border = if s.horizontal {
            BorderSides {
                top: line,
                right: none,
                bottom: none,
                left: none,
            }
        } else {
            BorderSides {
                top: none,
                right: none,
                bottom: none,
                left: line,
            }
        };
        let extras = BoxExtras {
            hit_transparent: true,
            ..BoxExtras::default()
        };
        PaintChild::Box(Box::new(PaintBox {
            node: table,
            border_box: self.drect(id),
            background: None,
            border,
            extras,
            children: Vec::new(),
        }))
    }

    /// The glyph runs of a line in document space, computed once per line and shared by its text
    /// fragments (L-PERF-b3: they were copied for every fragment of the line).
    fn line_runs(
        &mut self,
        paragraph: usize,
        line: usize,
        content: LayoutPoint,
        vertical: Option<(crate::layout::writing_mode::LineFrame, LayoutPoint)>,
    ) -> std::rc::Rc<Vec<LineRun>> {
        if let Some(r) = self.runs.get(&(paragraph, line)) {
            return r.clone();
        }
        let tree = self.tree;
        let out = match tree.paragraphs.get(paragraph) {
            Some(p) => p.with_glyph_runs(line, |runs| {
                let mut placed = self.place_runs(runs, content);
                // A line of a vertical writing mode (015 WM-1): the glyph origins are mapped from paragraph space
                // through the line's frame into document space, and every glyph is drawn turned.
                if let Some((frame, origin)) = vertical {
                    let (ox, oy) = (lu(origin.x), lu(origin.y));
                    let rotation = match frame.orientation {
                        crate::layout::writing_mode::LineOrientation::CounterClockwise => {
                            crate::scene::display_list::GlyphRotation::CounterClockwise
                        }
                        _ => crate::scene::display_list::GlyphRotation::Clockwise,
                    };
                    let (cx, cy) = (lu(content.x), lu(content.y));
                    // Upright glyphs (015 WM-1b) are drawn as they are: a run is split where the orientation changes.
                    let mut split: Vec<LineRun> = Vec::with_capacity(placed.len());
                    for ((owner, pseudo, first_x, run), view) in placed
                        .into_iter()
                        .zip(runs.iter().filter(|r| owner_node(r.owner).is_some()))
                    {
                        let mut cur: Option<((bool, u32), Vec<Glyph>)> = None;
                        let flush = |cur: Option<((bool, u32), Vec<Glyph>)>,
                                     split: &mut Vec<LineRun>| {
                            if let Some(((u, sx), gs)) = cur {
                                let mut r = GlyphRun {
                                    origin: None,
                                    glyphs: gs,
                                    ..run.clone()
                                };
                                r.synthesis.rotation = if u {
                                    crate::scene::display_list::GlyphRotation::None
                                } else {
                                    rotation
                                };
                                let sx = f32::from_bits(sx);
                                r.synthesis.scale_x = (sx != 1.0).then_some(sx);
                                split.push((owner, pseudo, first_x, r));
                            }
                        };
                        for (g, v) in run.glyphs.iter().zip(view.glyphs.iter()) {
                            let (px, py) = frame.point(g.x - cx, g.y - cy);
                            let glyph = Glyph {
                                id: g.id,
                                x: ox + px,
                                y: oy + py,
                            };
                            let key = (v.upright, v.scale_x.to_bits());
                            match &mut cur {
                                Some((k, gs)) if *k == key => gs.push(glyph),
                                _ => {
                                    flush(cur.take(), &mut split);
                                    cur = Some((key, vec![glyph]));
                                }
                            }
                        }
                        flush(cur.take(), &mut split);
                    }
                    placed = split;
                }
                placed
            }),
            None => Vec::new(),
        };
        let out = std::rc::Rc::new(out);
        self.runs.insert((paragraph, line), out.clone());
        out
    }

    fn place_runs(
        &mut self,
        runs: &[crate::layout::fragment::GlyphRunView],
        content: LayoutPoint,
    ) -> Vec<LineRun> {
        let (cx, cy) = (lu(content.x), lu(content.y));
        let mut out = Vec::with_capacity(runs.len());
        for run in runs {
            let Some(owner) = owner_node(run.owner) else {
                continue;
            };
            let pseudo = owner_pseudo(run.owner);
            self.fonts
                .insert(run.font.blob.id.0, run.font.blob.bytes.clone());
            if let Some(f) = self.frame()
                && !f.fonts.contains(&run.font.blob.id.0)
            {
                f.fonts.push(run.font.blob.id.0);
                f.events.push(crate::scene::reuse::Event::Font(
                    run.font.blob.id.0,
                    run.font.blob.bytes.clone(),
                ));
            }
            // Synthetic bold and italic are drawn by the rasteriser (`GlyphSynthesis`): consumed.
            let color = self
                .placeholder
                .or_else(|| self.style_p(owner, pseudo).map(|s| s.color))
                .unwrap_or(Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                });
            let first_x = run.glyphs.first().map(|g| g.x).unwrap_or(0.0);
            out.push((
                owner,
                pseudo,
                first_x,
                GlyphRun {
                    origin: Some((cx + run.origin.0, cy + run.origin.1)),
                    font: FontRef {
                        blob: run.font.blob.id.0,
                        index: run.font.index,
                    },
                    size_px: run.size_px,
                    glyphs: run
                        .glyphs
                        .iter()
                        .map(|g| Glyph {
                            id: g.id,
                            x: cx + g.x,
                            y: cy + g.y,
                        })
                        .collect(),
                    paint: Paint::Solid(color),
                    synthesis: GlyphSynthesis {
                        embolden: run.font.embolden,
                        skew_degrees: run.font.oblique_angle_deg.unwrap_or(0.0),
                        ..Default::default()
                    },
                    coords: if run.font.coords.iter().any(|c| *c != 0) {
                        run.font.coords.clone()
                    } else {
                        Vec::new()
                    },
                },
            ));
        }
        out
    }

    fn text_node_for(&mut self, element: NodeId, chars: usize) -> NodeId {
        self.read(element);
        if let Some(f) = self.frame()
            && f.text_owners.last() != Some(&element)
        {
            f.text_owners.push(element);
        }
        let styles = self.styles;
        let (nodes, idx, used) = self
            .text_cursor
            .entry(element)
            .or_insert_with(|| (styles.text_nodes(element), 0, 0));
        while *idx < nodes.len() && *used >= nodes[*idx].1 && (*idx + 1 < nodes.len()) {
            *used -= nodes[*idx].1;
            *idx += 1;
        }
        let node = nodes.get(*idx).map(|n| n.0).unwrap_or(element);
        *used += chars;
        node
    }

    fn text(&mut self, id: FragmentId, content: LayoutPoint, out: &mut Vec<PaintChild>) {
        let f = self.tree.get(id);
        let FragmentKind::Text(tf) = &f.kind else {
            return;
        };
        let (owner, paragraph, line, range, pseudo) = (
            tf.owner,
            tf.paragraph_index,
            tf.line_index,
            tf.text_range.clone(),
            tf.pseudo,
        );
        let rect = self.drect(id);
        if let Some(s) = self.style_p(owner, pseudo) {
            let boxed = self.tree.box_fragment(owner).is_some();
            self.check(owner, &s, boxed);
        }
        let chars = self
            .tree
            .paragraphs
            .get(paragraph)
            .and_then(|p| p.text().get(range))
            .map(|t| t.chars().filter(|c| !c.is_whitespace()).count())
            .unwrap_or(0);
        // Generated text has no DOM text node: a hit names the originating element (FR-086).
        let text_node = if pseudo.is_some_and(|k| k.is_generated()) {
            owner
        } else {
            self.text_node_for(owner, chars)
        };
        // `visibility: hidden | collapse`: the text keeps its place in the line (layout) but paints nothing and is not a hit target.
        if !self.is_visible(owner, pseudo) {
            return;
        }
        // Paragraph-space x extent of this fragment (glyph positions are paragraph space). In a vertical line
        // (015 WM-1) the extent is the fragment's span along the line, mapped back through the line's frame.
        let vertical = f.parent.and_then(|l| match &self.tree.get(l).kind {
            FragmentKind::Line(lf) => lf.vertical.map(|v| (v, self.drect(l).origin)),
            _ => None,
        });
        let (x0, x1) = match vertical {
            Some((frame, origin)) => {
                let a = lu(rect.origin.y) - lu(origin.y);
                frame.inline_extent(a, a + lu(rect.size.height))
            }
            None => {
                let x0 = lu(rect.origin.x) - lu(content.x);
                (x0, x0 + lu(rect.size.width))
            }
        };
        let runs = self.line_runs(paragraph, line, content, vertical);
        let mine: Vec<&LineRun> = runs
            .iter()
            .filter(|r| r.0 == owner && r.1 == pseudo)
            .collect();
        let inside: Vec<&LineRun> = mine
            .iter()
            .copied()
            .filter(|r| r.2 >= x0 - 0.5 && r.2 < x1 - 0.5)
            .collect();
        // A fragment with no run starting inside it (white space only) still gets a hit region.
        let chosen: Vec<&LineRun> = if inside.is_empty()
            && mine.len() == 1
            && self.text_fragments_on_line(owner, pseudo, paragraph, line) == 1
        {
            mine
        } else {
            inside
        };
        if chosen.is_empty() {
            out.push(PaintChild::Text(PaintText {
                text_node,
                parent_element: owner,
                line_rect: rect,
                run: GlyphRun {
                    origin: None,
                    font: FontRef { blob: 0, index: 0 },
                    size_px: 0.0,
                    glyphs: Vec::new(),
                    paint: Paint::Solid(transparent()),
                    synthesis: GlyphSynthesis::default(),
                    coords: Vec::new(),
                },
                shadows: Vec::new(),
            }));
        }
        let shadows = self
            .style_p(owner, pseudo)
            .map(|s| s.text_shadow)
            .unwrap_or_default();
        for r in chosen {
            out.push(PaintChild::Text(PaintText {
                text_node,
                parent_element: owner,
                line_rect: rect,
                run: r.3.clone(),
                shadows: shadows.clone(),
            }));
        }
    }

    fn text_fragments_on_line(
        &mut self,
        owner: NodeId,
        pseudo: Option<PseudoKind>,
        paragraph: usize,
        line: usize,
    ) -> usize {
        if let Some(f) = self.frame()
            && f.text_owners.last() != Some(&owner)
        {
            f.text_owners.push(owner);
        }
        self.tree
            .text_fragments_of(owner)
            .iter()
            .filter(|f| matches!(&f.kind, FragmentKind::Text(t) if t.paragraph_index == paragraph && t.line_index == line && t.pseudo == pseudo))
            .count()
    }
}

/// Build the [`SceneInput`] of one committed layout: 008's fragments (origins relative to the
/// parent border box, read through `document_rect`; 009's glyph runs per line, positioned in
/// document space, `FontRef` copied unchanged) and the paint styles. Applies root/body
/// background propagation to the canvas (CSS Backgrounds 3 §2.11.2, 010 FR-023) and the
/// paint-stage consumption checks (stage `scene`).
pub fn prepare_scene(
    fragments: &FragmentTree,
    styles: &impl PaintStyleSource,
    frame: &SceneFrame,
) -> PreparedScene {
    prepare_scene_with(fragments, styles, frame, PaintOptions::FULL)
}

/// [`prepare_scene`] with explicit [`PaintOptions`].
pub fn prepare_scene_with(
    fragments: &FragmentTree,
    styles: &impl PaintStyleSource,
    frame: &SceneFrame,
    options: PaintOptions,
) -> PreparedScene {
    prepare_scene_scrolled(fragments, styles, frame, options, &ScrollOffsets::new())
}

/// [`prepare_scene_with`] with the scroll offsets in force: the content of every scrolled
/// container (and of the viewport) is placed at its scrolled position, so the display list and
/// the hit regions are in viewport space. Nothing of layout is read except the published
/// fragments; a scroll never produces a layout epoch (008 FR-066, E008-3).
pub fn prepare_scene_scrolled(
    fragments: &FragmentTree,
    styles: &impl PaintStyleSource,
    frame: &SceneFrame,
    options: PaintOptions,
    scroll: &ScrollOffsets,
) -> PreparedScene {
    prepare_inner(fragments, styles, frame, options, scroll, None).0
}

/// [`prepare_scene_scrolled`] over `layout`'s fragments, taking over from the previous scene recorded
/// in `memo` the subtrees whose inputs did not change (015 PERF-2, [`crate::scene::reuse`]). The result
/// equals [`prepare_scene_scrolled`]'s; `memo` then describes this scene.
pub fn prepare_scene_reusing(
    layout: &std::sync::Arc<crate::layout::coordinator::LayoutOutput>,
    styles: &impl PaintStyleSource,
    frame: &SceneFrame,
    options: PaintOptions,
    scroll: &ScrollOffsets,
    memo: &mut crate::scene::reuse::SceneMemo,
) -> PreparedScene {
    let old = memo
        .built_from
        .upgrade()
        .filter(|_| memo.paint_radii == Some(options.paint_radii));
    let prev = std::mem::take(&mut memo.entries);
    let (clean, prev) = match &old {
        Some(old) => (
            Some(crate::scene::reuse::Clean::new(
                &layout.fragments,
                &old.fragments,
                std::sync::Arc::ptr_eq(layout, old),
            )),
            prev,
        ),
        None => (None, Default::default()),
    };
    let boxes = layout.boxes.boxes.len();
    let reuse = crate::scene::reuse::Reuse {
        clean,
        next: std::mem::take(&mut memo.spare).reset(boxes),
        prev,
        inputs_ok: std::mem::take(&mut memo.spare_ok).reset(boxes),
        new_of_box: crate::scene::reuse::single_fragment_of_box(&layout.fragments),
        stack: Vec::new(),
        stats: Default::default(),
    };
    let (prepared, reuse) = prepare_inner(
        &layout.fragments,
        styles,
        frame,
        options,
        scroll,
        Some(reuse),
    );
    let reuse = reuse.expect("reuse state");
    memo.entries = reuse.next;
    memo.spare = reuse.prev.reset(0);
    memo.spare_ok = reuse.inputs_ok.reset(0);
    memo.stats = reuse.stats;
    memo.built_from = std::sync::Arc::downgrade(layout);
    memo.paint_radii = Some(options.paint_radii);
    prepared
}

fn prepare_inner<'a, S: PaintStyleSource>(
    fragments: &'a FragmentTree,
    styles: &'a S,
    frame: &SceneFrame,
    options: PaintOptions,
    scroll: &'a ScrollOffsets,
    reuse: Option<crate::scene::reuse::Reuse<'a>>,
) -> (PreparedScene, Option<crate::scene::reuse::Reuse<'a>>) {
    let viewport = LayoutSize {
        width: LayoutUnit::from_css_px_f32(fragments.viewport.width).unwrap_or(LayoutUnit::ZERO),
        height: LayoutUnit::from_css_px_f32(fragments.viewport.height).unwrap_or(LayoutUnit::ZERO),
    };
    let mut a = Adapter {
        tree: fragments,
        styles,
        document: frame.revision.document,
        propagated: None,
        paint_radii: options.paint_radii,
        fonts: FontTable::new(),
        reports: Vec::new(),
        checked: Default::default(),
        style_cache: HashMap::default(),
        text_cursor: HashMap::default(),
        runs: HashMap::default(),
        box_node: None,
        piece_span: None,
        scroll,
        placeholder: None,
        shift: {
            let (vx, vy) = scroll.offset(ScrollTarget::Viewport);
            let q = |v: f64| LayoutUnit::quantize(v).value;
            LayoutPoint {
                x: LayoutUnit::ZERO - q(vx),
                y: LayoutUnit::ZERO - q(vy),
            }
        },
        reuse,
    };
    let root_node = fragments.root.and_then(|r| match &fragments.get(r).kind {
        FragmentKind::Box(b) => Some(b.owner.node()),
        _ => None,
    });
    // Background propagation: the root's background if not transparent, else body's.
    let mut canvas = frame.default_canvas;
    if let Some(root) = root_node {
        let root_bg = a
            .style(root)
            .map(|s| s.background_color)
            .unwrap_or_else(transparent);
        // A non-transparent root image also suppresses body-to-canvas propagation even when
        // the root colour is transparent. Inspect typed blend layers for this compositing path.
        let root_blended_image = a
            .styles
            .paint_extras(
                root,
                (
                    0.0,
                    0.0,
                    viewport.width.to_f64() as f32,
                    viewport.height.to_f64() as f32,
                ),
            )
            .layers
            .iter()
            .any(|l| l.blend_mode != crate::scene::display_list::BlendMode::Normal);
        if root_bg.a > 0.0 || root_blended_image {
            a.propagated = Some(root);
            canvas = over(root_bg, frame.default_canvas);
        } else if let Some(body) = styles.canvas_body() {
            let body_bg = a
                .style(body)
                .map(|s| s.background_color)
                .unwrap_or_else(transparent);
            if body_bg.a > 0.0 {
                a.propagated = Some(body);
                canvas = over(body_bg, frame.default_canvas);
            }
        }
    }
    let empty_root = |node: NodeId| PaintBox {
        node,
        border_box: LayoutRect::default(),
        background: None,
        border: BorderSides {
            top: BorderSide {
                width: LayoutUnit::ZERO,
                style: BorderStyle::None,
                color: transparent(),
            },
            right: BorderSide {
                width: LayoutUnit::ZERO,
                style: BorderStyle::None,
                color: transparent(),
            },
            bottom: BorderSide {
                width: LayoutUnit::ZERO,
                style: BorderStyle::None,
                color: transparent(),
            },
            left: BorderSide {
                width: LayoutUnit::ZERO,
                style: BorderStyle::None,
                color: transparent(),
            },
        },
        extras: BoxExtras::default(),
        children: Vec::new(),
    };
    let mut root = match fragments
        .root
        .and_then(|r| a.paint_box_shared(r))
        .map(std::sync::Arc::unwrap_or_clone)
    {
        Some(mut b) => {
            if b.extras.clip_path.is_some() {
                b.extras.canvas_base = Some(frame.default_canvas);
            }
            b
        }
        // No root box (empty document or `display: none` root): canvas only. The owner of the
        // canvas item is then the root element if known, else a placeholder id of the document.
        None => {
            empty_root(root_node.unwrap_or_else(|| NodeId::from_raw(frame.revision.document.raw())))
        }
    };
    // Viewport scrollbars are in viewport coordinates, outside root transforms and clipping.
    if let (Some(node), Some(style), Some(range)) = (
        root_node,
        styles.scrollbar_style(root.node),
        crate::layout::coordinator::overflow::scroll_range(fragments, ScrollTarget::Viewport),
    ) {
        let _ = node;
        let port = Rect {
            x: 0.0,
            y: 0.0,
            width: viewport.width.to_f64() as f32,
            height: viewport.height.to_f64() as f32,
        };
        let v = &fragments.viewport_scroll;
        root.extras.scrollbars = crate::scene::paint::scrollbar::layout(
            port,
            range,
            scroll.offset(ScrollTarget::Viewport),
            style,
            v.overflow_x,
            v.overflow_y,
        );
    }
    // A propagated colour and image layers must composite together before the default canvas.
    // Retain the colour with the source box's background group rather than precompositing it.
    fn take_canvas(b: &mut PaintBox, node: NodeId, color: Color, canvas: Rect) -> Option<PaintBox> {
        if b.node == node
            && b.extras
                .layers
                .iter()
                .any(|l| l.blend_mode != crate::scene::display_list::BlendMode::Normal)
        {
            let mut extras = BoxExtras::default();
            extras.layers = std::mem::take(&mut b.extras.layers);
            extras.padding = b.extras.padding;
            extras.radii = b.extras.radii;
            extras.canvas_blend_color = Some((color, canvas));
            let mut source = PaintBox {
                node: b.node,
                border_box: b.border_box,
                background: None,
                border: b.border,
                extras,
                children: Vec::new(),
            };
            for side in [
                &mut source.border.top,
                &mut source.border.right,
                &mut source.border.bottom,
                &mut source.border.left,
            ] {
                side.style = BorderStyle::None;
            }
            return Some(source);
        }
        b.children.iter_mut().find_map(|child| match child {
            PaintChild::Box(c) => take_canvas(c, node, color, canvas),
            PaintChild::Shared(c) => take_canvas(std::sync::Arc::make_mut(c), node, color, canvas),
            _ => None,
        })
    }
    if let Some(node) = a.propagated {
        let color = a
            .style(node)
            .map(|s| s.background_color)
            .unwrap_or_else(transparent);
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: viewport.width.to_f64() as f32,
            height: viewport.height.to_f64() as f32,
        };
        if let Some(bg) = take_canvas(&mut root, node, color, rect) {
            root.extras.canvas_blend = Some(Box::new(bg));
            canvas = frame.default_canvas;
        }
    }
    let prepared = PreparedScene {
        input: SceneInput {
            generation: frame.generation,
            revision: frame.revision,
            viewport,
            canvas_background: canvas,
            root,
        },
        fonts: a.fonts,
        reports: a.reports,
    };
    (prepared, a.reuse)
}
