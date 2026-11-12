//! owner: 005-pipeline-diagnostics — wiring of 007's `ResourceManager` into the update algorithm
//! (lane L-RES-r3; 005 recompute-plan §6 step 3, invalidation-reasons `RESOURCE_EPOCH`).
//!
//! One [`PipelineResources`] per document is the pipeline's whole view of subresources:
//!
//! * the style stage's [`SheetLoader`] (`<link rel=stylesheet>` and `@import`, synchronous reads
//!   through 007's `load_text_sync` under the document's policy, R-09);
//! * the scene stage's [`ImageSource`] (CSS `url()` images and `<img src>`): the first lookup
//!   issues the request (007 de-duplicates per requester), later lookups read its state. Nothing
//!   blocks: a pending image paints nothing until the completion batch is applied;
//! * the document base URL that relative references resolve against.
//!
//! The runtime owns the manager and drains it in turn step 3 (`process_completions`), then hands
//! each [`AppliedBatch`] to [`DocumentPipeline::apply_resource_batch`]: one resource-epoch
//! increment and one `RESOURCE_EPOCH` reason per batch, so the next update restyles nothing it did
//! not need, relays out and repaints once (the existing damage path).
//!
//! Broken and denied images are honest: the request is `Broken { code }` (`RESOURCE_DENIED`,
//! `FILE_NOT_FOUND`, `UNSUPPORTED_MEDIA_TYPE`, ...), 007 keeps the diagnostic record, nothing
//! is painted in its place, and the host reads the state through `ResourceManager`.

use crate::pipeline::state::DocumentPipeline;
use crate::scene::display_list::ImagePixels;
use crate::scene::inputs::{
    ImageData, ImageLookup, ImageSource, PictureSourceAttrs, VectorHandle, VectorRaster,
};
use crate::style::sheets::loader::{LoadedSheet, SheetLoader};
use crate::style::traverse::StyleBridge;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use ui4me_assets::ResourceManager;
use ui4me_assets::Url;
use ui4me_assets::request::Requester;
use ui4me_assets::srcset::{PictureSource, SourceEnv, select_picture_source};
use ui4me_assets::uri::{self, ResourceKind};
use ui4me_assets::view::{AppliedBatch, ImageState, PaintImage};
use ui4me_document::document::Document;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::resource_ids::ResourceRef;
use ui4me_engine_types::status::Status;

/// The runtime-wide manager, shared by every document's [`PipelineResources`]. Every method of the
/// manager runs on the owner thread; the lock is never contended and never held across a call
/// back into the pipeline.
pub type SharedResources = Arc<Mutex<ResourceManager>>;

/// The existing resource caches and the registered source-byte store, counted separately.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceStats {
    pub cache: ui4me_assets::cache::CacheStats,
    pub bundles: ui4me_assets::bundles::BundleStats,
    pub svg_rasters: ui4me_assets::svg::raster_cache::RasterStats,
}

/// What the pipeline of one document uses of 007.
pub struct PipelineResources {
    manager: SharedResources,
    document: DocumentId,
    document_url: Url,
    state: Arc<Mutex<State>>,
}

struct State {
    /// The base URL of the document (`<base href>` applied) at the last update.
    base: Url,
    /// Device scale of the last update (SVG rasters are made at natural size times scale).
    scale: f64,
    /// Viewport (CSS px) of the last update: `srcset` / `sizes` selection.
    viewport: (f32, f32),
    /// The `ImageData` handed to the scene per request, rebuilt when the pixels change.
    images: HashMap<ResourceRef, Cached>,
    /// The request of each `<img>` element that asked for its image.
    img_refs: HashMap<NodeId, ResourceRef>,
    /// The request of each CSS image (`list-style-image`, `content: url()`, backgrounds) by
    /// (element, property key): layout reads the natural size of generated images through it.
    css_refs: HashMap<(NodeId, String), ResourceRef>,
    /// Vector images rasterised at a placed size: (request, raster generation) -> image. The
    /// generation changes with the size and with the entry (007's per-size raster cache).
    sized: HashMap<(ResourceRef, u64), Arc<ImageData>>,
    next_id: u64,
    /// The serialised document and element table of each inline `<svg>` last painted (F0907).
    inline: HashMap<NodeId, InlineEntry>,
    /// The last ready picture of each inline `<svg>`: shown while the next one rasterises.
    inline_last: HashMap<NodeId, Arc<ImageData>>,
}

/// Most inline SVG documents kept for hit testing.
const MAX_INLINE: usize = 256;

struct InlineEntry {
    markup: Arc<str>,
    targets: Arc<crate::pipeline::inline_svg::Targets>,
    /// Built on the first hit test; `Some(None)` = it has no usable table.
    shapes: Option<Option<Arc<ui4me_assets::svg::hit::HitShapes>>>,
}

/// Most sized rasters whose `ImageData` is kept; beyond it the map starts over (007 owns the
/// pixels and its own budget, this only keeps ids stable between updates).
const MAX_SIZED: usize = 128;

/// One vector image's re-rasterisation at placed sizes (the scene asks while planning).
///
/// Both references are weak: the raster is stored in the `State` it points at (`State::images`), and a
/// strong reference would make a cycle that kept the state, every cached picture and the whole resource
/// manager of a closed document alive for the life of the process (F1006).
struct SizedRaster {
    manager: std::sync::Weak<Mutex<ResourceManager>>,
    state: std::sync::Weak<Mutex<State>>,
    request: ResourceRef,
}

impl std::fmt::Debug for SizedRaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizedRaster")
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

impl VectorRaster for SizedRaster {
    fn raster_at(&self, css_size: (f32, f32)) -> Option<Arc<ImageData>> {
        let (manager, state) = (self.manager.upgrade()?, self.state.upgrade()?);
        let scale = lock(&state).scale;
        let dev = |v: f32| ((f64::from(v) * scale).ceil().max(1.0)) as u32;
        let (natural, ratio) = {
            let mgr = lock(&manager);
            match mgr.natural_size(&self.request) {
                ImageState::Ready {
                    width_css,
                    height_css,
                    ratio,
                } => ((width_css, height_css), ratio.map(|(w, h)| w / h)),
                _ => return None,
            }
        };
        let (pixels, generation) =
            match lock(&manager).paint_image(&self.request, (dev(css_size.0), dev(css_size.1))) {
                PaintImage::Raster { pixels, generation } => (pixels, generation),
                PaintImage::NotReady | PaintImage::Broken { .. } => return None,
            };
        let mut s = lock(&state);
        if let Some(d) = s.sized.get(&(self.request, generation)) {
            return Some(d.clone());
        }
        if s.sized.len() >= MAX_SIZED {
            s.sized.clear();
        }
        s.next_id += 1;
        let id = s.next_id;
        let image = Arc::new(ImageData {
            id,
            pixels: raster_pixels(&pixels),
            natural,
            ratio,
            vector: None,
        });
        s.sized.insert((self.request, generation), image.clone());
        Some(image)
    }
}

/// 007's pixels as the scene's (tight rows).
fn raster_pixels(pixels: &ui4me_assets::view::ImageRaster) -> ImagePixels {
    let row = pixels.width as usize * 4;
    let data: Arc<[u8]> = if pixels.stride as usize == row {
        pixels.data.clone()
    } else {
        pixels
            .data
            .chunks(pixels.stride as usize)
            .take(pixels.height as usize)
            .flat_map(|r| r[..row].iter().copied())
            .collect::<Vec<u8>>()
            .into()
    };
    ImagePixels {
        width: pixels.width,
        height: pixels.height,
        data,
    }
}

struct Cached {
    generation: u64,
    size: (u32, u32),
    data: Arc<ImageData>,
}

impl std::fmt::Debug for PipelineResources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineResources")
            .field("document", &self.document)
            .field("document_url", &self.document_url.as_str())
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl PipelineResources {
    /// `document` must already be registered with the manager (`create_document`).
    pub fn new(
        manager: SharedResources,
        document: DocumentId,
        document_url: Url,
    ) -> PipelineResources {
        PipelineResources {
            manager,
            document,
            document_url: document_url.clone(),
            state: Arc::new(Mutex::new(State {
                base: document_url,
                scale: 1.0,
                viewport: (800.0, 600.0),
                images: HashMap::new(),
                img_refs: HashMap::new(),
                css_refs: HashMap::new(),
                sized: HashMap::new(),
                next_id: 0,
                inline: HashMap::new(),
                inline_last: HashMap::new(),
            })),
        }
    }

    pub fn document(&self) -> DocumentId {
        self.document
    }

    /// Atomically replace an in-memory bundle. The caller applies `apply_bundle_change` to
    /// every affected document after this method returns (the manager lock is released).
    pub fn register_bundle(
        &self,
        name: &str,
        entries: Vec<ui4me_assets::bundles::BundleEntryInput>,
    ) -> Result<ui4me_assets::bundles::BundleChange, Status> {
        self.manager().bundle_register(name, entries)
    }

    /// Remove source bytes and restart affected resource requests as missing. Idempotent.
    pub fn unregister_bundle(
        &self,
        name: &str,
    ) -> Result<ui4me_assets::bundles::BundleChange, Status> {
        if !ui4me_assets::bundles::valid_name(name) {
            return Err(Status::new(
                Code::InvalidArgument,
                ui4me_engine_types::stage::Stage::Resource,
            )
            .with_message("invalid resource bundle name"));
        }
        Ok(self.manager().bundle_unregister(name))
    }

    pub fn resource_stats(&self) -> ResourceStats {
        let manager = self.manager();
        ResourceStats {
            cache: manager.cache_stats(),
            bundles: manager.bundles().stats(),
            svg_rasters: manager.svg_raster_stats(),
        }
    }

    /// Purge unused decoded cache entries only; registered bundles and live images remain.
    /// `url` is an optional exact absolute resource URL, not a filesystem path or prefix.
    pub fn purge_unused_resources(&self, url: Option<&str>) -> Result<usize, Status> {
        if let Some(url) = url {
            Url::parse(url).map_err(|_| {
                Status::new(
                    Code::InvalidArgument,
                    ui4me_engine_types::stage::Stage::Resource,
                )
                .with_message("purge URL must be an absolute resource URL")
            })?;
        }
        Ok(self.manager().purge_unused_resources(url))
    }

    /// The URL the document was loaded from (`about:blank` for a string document).
    pub fn document_url(&self) -> &Url {
        &self.document_url
    }

    /// The base URL of the last update.
    pub fn base_url(&self) -> Url {
        lock(&self.state).base.clone()
    }

    /// Start of an update: the base URL (`<base href>`) and device scale the lookups use.
    pub(crate) fn begin_update(&self, doc: &Document, scale: f64, viewport: (f32, f32)) -> Url {
        let from_doc = doc.base_uri();
        let href = (from_doc != doc.url()).then_some(from_doc.as_str());
        let base = uri::document_base_url(&self.document_url, href);
        let mut s = lock(&self.state);
        s.base = base.clone();
        s.scale = scale;
        s.viewport = viewport;
        base
    }

    fn manager(&self) -> MutexGuard<'_, ResourceManager> {
        lock(&self.manager)
    }

    /// The manager, for the `@font-face` wiring (`font_faces.rs`).
    pub(crate) fn manager_lock(&self) -> MutexGuard<'_, ResourceManager> {
        lock(&self.manager)
    }

    /// The environment of image source selection: the viewport and device scale of the last update.
    fn source_env(&self) -> SourceEnv {
        let s = lock(&self.state);
        SourceEnv {
            viewport: s.viewport,
            dpr: s.scale as f32,
        }
    }

    /// `img.currentSrc` (015 HTML-4): the URL of the source HTML "select an image source" picks for the
    /// element now (a parent `picture`'s `source` elements first, then `srcset` / `sizes` / `src`, for
    /// the viewport and device scale of the last update), resolved against the document base URL; empty
    /// when it has none. A URL the policy denies is still its current source (its request is broken).
    pub fn img_current_src(&self, doc: &Document, node: NodeId) -> String {
        let env = self.source_env();
        let attr = |name: &str| doc.get_attribute(node, name).ok().flatten();
        let sources = picture_sources(doc, node);
        let list: Vec<PictureSource<'_>> = sources
            .iter()
            .map(|&(srcset, sizes, media, mime)| PictureSource {
                srcset,
                sizes,
                media,
                mime,
            })
            .collect();
        let Some(picked) =
            select_picture_source(&list, attr("src"), attr("srcset"), attr("sizes"), &env)
        else {
            return String::new();
        };
        let base = lock(&self.state).base.clone();
        base.join(picked.url.trim())
            .map(|u| u.to_string())
            .unwrap_or(picked.url)
    }

    /// The request of an `<img>` element (after its first lookup), for `image_element_data`.
    pub fn image_element_ref(&self, node: NodeId) -> Option<ResourceRef> {
        lock(&self.state).img_refs.get(&node).copied()
    }

    /// 007's natural-size view of an `<img>` element already requested by the scene stage
    /// (`None` before its first request: layout then treats it as pending).
    pub fn image_element_state(&self, node: NodeId) -> Option<ImageState> {
        let r = self.image_element_ref(node)?;
        Some(self.manager().natural_size(&r))
    }

    /// 007's natural-size view of a CSS image already requested by the scene stage under
    /// (`node`, `property`) (`None` before its first request: layout then treats it as pending).
    pub fn css_image_state(&self, node: NodeId, property: &str) -> Option<ImageState> {
        let r = lock(&self.state)
            .css_refs
            .get(&(node, property.to_string()))
            .copied()?;
        Some(self.manager().natural_size(&r))
    }

    fn lookup(&self, requester: Requester, reference: &str) -> ImageLookup {
        self.lookup_at(requester, reference, 1.0)
    }

    /// `lookup` for a `srcset` selection: `density` is the density of the selected candidate (the
    /// natural size is the pixel size divided by it).
    fn lookup_at(&self, requester: Requester, reference: &str, density: f32) -> ImageLookup {
        let base = lock(&self.state).base.clone();
        let img_node = if let Requester::ImgElement(n) = &requester {
            Some(*n)
        } else {
            None
        };
        let css_key = if let Requester::CssImage { node, property, .. } = &requester {
            Some((*node, property.clone()))
        } else {
            None
        };
        let mut mgr = self.manager();
        let r = match mgr.request_image(self.document, requester, &base, reference, density) {
            Ok(r) => r,
            Err(s) => return ImageLookup::Broken(s.code),
        };
        if let Some(n) = img_node {
            lock(&self.state).img_refs.insert(n, r);
        }
        if let Some(k) = css_key {
            lock(&self.state).css_refs.insert(k, r);
        }
        let (natural, ratio) = match mgr.natural_size(&r) {
            ImageState::Pending => return ImageLookup::Pending,
            ImageState::Broken { code } => return ImageLookup::Broken(code),
            ImageState::Ready {
                width_css,
                height_css,
                ratio,
            } => ((width_css, height_css), ratio.map(|(w, h)| w / h)),
        };
        // A vector image is rasterised at its natural size times the device scale (the scene does
        // not tell the provider the placed size); one with only a ratio gets a 300 px wide raster
        // of that ratio. Rasters ignore the size.
        let scale = lock(&self.state).scale;
        let dev = |v: f32| ((f64::from(v) * scale).ceil().max(1.0)) as u32;
        let (w, h) = match (natural.0, natural.1, ratio) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some(r)) => (w, w / r),
            (None, Some(h), Some(r)) => (h * r, h),
            (None, None, Some(r)) => (300.0, 300.0 / r),
            (w, h, None) => (w.unwrap_or(300.0), h.unwrap_or(150.0)),
        };
        let mgr_is_vector = mgr.is_vector(&r);
        let (pixels, generation) = match mgr.paint_image(&r, (dev(w), dev(h))) {
            PaintImage::NotReady => return ImageLookup::Pending,
            PaintImage::Broken { code } => return ImageLookup::Broken(code),
            PaintImage::Raster { pixels, generation } => (pixels, generation),
        };
        drop(mgr);
        let mut s = lock(&self.state);
        let size = (pixels.width, pixels.height);
        if let Some(c) = s.images.get(&r)
            && c.generation == generation
            && c.size == size
            && c.data.natural == natural
        {
            return ImageLookup::Ready(c.data.clone());
        }
        s.next_id += 1;
        let id = s.next_id;
        let vector = mgr_is_vector.then(|| {
            VectorHandle(Arc::new(SizedRaster {
                manager: Arc::downgrade(&self.manager),
                state: Arc::downgrade(&self.state),
                request: r,
            }))
        });
        let image = Arc::new(ImageData {
            id,
            pixels: raster_pixels(&pixels),
            natural,
            ratio,
            vector,
        });
        s.images.insert(
            r,
            Cached {
                generation,
                size,
                data: image.clone(),
            },
        );
        ImageLookup::Ready(image)
    }
}

impl PipelineResources {
    /// Whether the inline `<svg>` `node` has `pointer-events: none` (it is transparent to hits).
    pub fn inline_svg_transparent(&self, node: NodeId) -> bool {
        use ui4me_assets::svg::hit::PointerMode;
        lock(&self.state).inline.get(&node).is_some_and(|e| {
            e.targets
                .values()
                .any(|(n, mode)| *n == node && *mode == PointerMode::None)
        })
    }

    /// The element of the inline `<svg>` `node` under the point `(x, y)` (CSS px relative to its
    /// `w` x `h` content box), by the shape geometry of the picture last painted. `None` when no
    /// shape reacts there (the `<svg>` itself is then the target).
    pub fn inline_svg_hit(&self, node: NodeId, x: f32, y: f32, w: f32, h: f32) -> Option<NodeId> {
        use ui4me_assets::svg::hit::{HitShapes, PointerMode};
        let (markup, targets, cached) = {
            let s = lock(&self.state);
            let e = s.inline.get(&node)?;
            (e.markup.clone(), e.targets.clone(), e.shapes.clone())
        };
        let shapes: Option<Arc<HitShapes>> = match cached {
            Some(c) => c,
            None => {
                let built = HitShapes::build(&markup).map(Arc::new);
                if let Some(e) = lock(&self.state).inline.get_mut(&node)
                    && Arc::ptr_eq(&e.markup, &markup)
                {
                    e.shapes = Some(built.clone());
                }
                built
            }
        };
        let shapes = shapes?;
        let mode_of = |id: &str| targets.get(id).map_or(PointerMode::None, |t| t.1);
        let id = shapes.hit(x, y, w, h, &mode_of)?;
        targets.get(id).map(|t| t.0)
    }
}

impl ImageSource for PipelineResources {
    fn inline_svg(
        &self,
        node: NodeId,
        markup: &str,
        targets: Arc<crate::pipeline::inline_svg::Targets>,
    ) -> ImageLookup {
        {
            let mut s = lock(&self.state);
            if !s.inline.get(&node).is_some_and(|e| *e.markup == *markup) {
                if s.inline.len() >= MAX_INLINE {
                    s.inline.clear();
                }
                s.inline.insert(
                    node,
                    InlineEntry {
                        markup: Arc::from(markup),
                        targets,
                        shapes: None,
                    },
                );
            }
        }
        let lookup = self.img_element(node, &crate::pipeline::inline_svg::data_url(markup));
        let mut s = lock(&self.state);
        match lookup {
            ImageLookup::Ready(image) => {
                if s.inline_last.len() >= MAX_INLINE {
                    s.inline_last.clear();
                }
                s.inline_last.insert(node, image.clone());
                ImageLookup::Ready(image)
            }
            ImageLookup::Pending => match s.inline_last.get(&node) {
                Some(last) => ImageLookup::Ready(last.clone()),
                None => ImageLookup::Pending,
            },
            broken => {
                s.inline_last.remove(&node);
                broken
            }
        }
    }

    fn css_image(
        &self,
        node: NodeId,
        property: &'static str,
        layer: u32,
        url: &str,
        _resolved: bool,
    ) -> ImageLookup {
        self.lookup(
            Requester::CssImage {
                node,
                property: property.to_string(),
                layer,
                url: url.to_string(),
            },
            url,
        )
    }

    fn img_element(&self, node: NodeId, src: &str) -> ImageLookup {
        self.lookup(Requester::ImgElement(node), src)
    }

    fn img_element_set(
        &self,
        node: NodeId,
        src: Option<&str>,
        srcset: Option<&str>,
        sizes: Option<&str>,
    ) -> Option<ImageLookup> {
        self.img_element_picture(node, &[], src, srcset, sizes)
    }

    fn img_element_picture(
        &self,
        node: NodeId,
        sources: &[PictureSourceAttrs<'_>],
        src: Option<&str>,
        srcset: Option<&str>,
        sizes: Option<&str>,
    ) -> Option<ImageLookup> {
        let env = self.source_env();
        let list: Vec<PictureSource<'_>> = sources
            .iter()
            .map(|&(srcset, sizes, media, mime)| PictureSource {
                srcset,
                sizes,
                media,
                mime,
            })
            .collect();
        let picked = select_picture_source(&list, src, srcset, sizes, &env)?;
        Some(self.lookup_at(Requester::ImgElement(node), &picked.url, picked.density))
    }
}

impl SheetLoader for PipelineResources {
    fn load(&self, base: &Url, reference: &str) -> Result<LoadedSheet, Status> {
        let t = self.manager().load_text_sync(
            self.document,
            base,
            reference,
            ResourceKind::Stylesheet,
        )?;
        Ok(LoadedSheet {
            url: t.final_url,
            bytes: t.bytes,
        })
    }
}

/// Point a style bridge at the document's resources: base URL (`<base href>` applied) and the
/// sheet loader. A no-op without resources and when nothing changed.
pub(crate) fn configure_style(
    style: &mut StyleBridge,
    resources: Option<&Arc<PipelineResources>>,
    doc: &Document,
    scale: f64,
    viewport: (f32, f32),
) {
    if let Some(r) = resources {
        let base = r.begin_update(doc, scale, viewport);
        style.set_base_url(base);
        style.set_sheet_loader(Some(r.clone() as Arc<dyn SheetLoader>));
    }
}

impl DocumentPipeline {
    /// Attach the document's resource view: linked stylesheets and `@import` load through it in
    /// the style stage, images through it in the scene stage. Marks the document `Dirty` (the
    /// next update re-reads its sheets). Replaces any image source set by [`set_image_source`].
    ///
    /// [`set_image_source`]: DocumentPipeline::set_image_source
    pub fn attach_resources(&mut self, resources: Arc<PipelineResources>) -> Result<(), Status> {
        self.check_open()?;
        self.images = Some(resources.clone());
        self.resources = Some(resources);
        configure_style(
            &mut self.style,
            self.resources.as_ref(),
            &self.doc,
            self.config.scale,
            (
                self.config.viewport_width as f32,
                self.config.viewport_height as f32,
            ),
        );
        // The loader and base URL are part of what the cascade reads: the next update re-reads
        // every sheet.
        self.note_input_reason(
            crate::pipeline::reasons::Reason::ColdRebuild,
            crate::pipeline::reasons::Subject::Document,
        );
        Ok(())
    }

    /// A `ui4me-res:` bundle this document referenced was registered again, replaced or removed
    /// (007 `BundleChange::affected`): linked sheets and `@import`s are read again at the next
    /// update, the resource epoch advances once and the next update restyles, relays out and
    /// repaints (`RESOURCE_EPOCH`, `COLD_REBUILD`; the request serial 0 stands for the bundle). Images from the bundle restart in 007 and arrive as ordinary batches.
    pub fn apply_bundle_change(&mut self) -> Result<(), Status> {
        self.check_open()?;
        self.style.reload_linked_sheets();
        self.resource_epoch += 1;
        // The cascade reads the sheets: restyle too (as `attach_resources`), not only L and P.
        self.dirty.note(
            crate::pipeline::reasons::Reason::ResourceEpoch,
            crate::pipeline::reasons::Subject::Resource(0),
        );
        self.note_input_reason(
            crate::pipeline::reasons::Reason::ColdRebuild,
            crate::pipeline::reasons::Subject::Document,
        );
        Ok(())
    }

    /// The document's resource view, if attached.
    pub fn resources(&self) -> Option<&Arc<PipelineResources>> {
        self.resources.as_ref()
    }

    /// Apply one completion batch of 007 (`AppliedBatch`, turn step 3): one resource-epoch
    /// increment and one `RESOURCE_EPOCH` reason naming the changed requests; the next update
    /// runs layout and paint once for the whole batch. A font-epoch batch is
    /// [`DocumentPipeline::note_font_epoch_changed`]'s. Returns whether the document became dirty.
    pub fn apply_resource_batch(&mut self, batch: &AppliedBatch) -> Result<bool, Status> {
        self.check_open()?;
        if batch.document != self.doc.id() {
            return Err(Status::new(
                Code::WrongDocument,
                ui4me_engine_types::stage::Stage::Resource,
            ));
        }
        let ids: Vec<u64> = batch.changed.iter().map(|r| r.request.serial).collect();
        self.apply_resource_completions(&ids)?;
        Ok(!ids.is_empty())
    }
}

/// The `(srcset, sizes, media, type)` of the `source` children of `img`'s parent `picture` that precede
/// it, in order (015 HTML-4); empty when the parent is not a `picture`.
pub fn picture_sources(doc: &Document, img: NodeId) -> Vec<PictureSourceAttrs<'_>> {
    let Some(parent) = doc
        .parent(img)
        .ok()
        .flatten()
        .filter(|&p| doc.local_name(p).is_ok_and(|n| n == "picture"))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &c in doc.children(parent).unwrap_or(&[]) {
        if c == img {
            break;
        }
        if doc.local_name(c).is_ok_and(|n| n == "source") {
            let a = |name: &str| doc.get_attribute(c, name).ok().flatten();
            out.push((a("srcset"), a("sizes"), a("media"), a("type")));
        }
    }
    out
}
