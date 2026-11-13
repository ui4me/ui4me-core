//! owner: 007-resources — created by 001 slice (R-28)
//!
//! Resource and font-source registry (U4C-RES): URL resolution and the resource policy, loading
//! and decoding off the owner thread, the runtime-wide cache, epoch-tagged consumer views, and
//! font sources. `ResourceManager` is the facade; every method runs on the owner thread (004
//! enforces `WRONG_THREAD`), no method ever blocks on another thread, and shutdown never joins.

#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::needless_range_loop
)]

pub mod bundles;
pub mod cache;
pub mod data_url;
pub mod decode;
pub mod deterministic;
pub mod diagnostics;
pub mod fonts;
pub mod fs_guard;
pub mod policy;
pub mod request;
pub mod srcset;
pub mod svg;
pub mod text;
pub mod uri;
pub mod view;
pub mod worker;

use cache::{Cache, CacheStats, PurgeScope};
use decode::RasterDecoder;
use diagnostics::{
    DEFAULT_LOG_BOUND, DiagnosticLog, DocumentCounters, ResourceDiagnosticRecord,
    RuntimeResourceDiagnostics,
};
use fonts::{
    FaceId, FaceSet, FaceSourceSpec, FaceState, FontBlob, FontBlobId, FontConfigReport, FontEpoch,
    FontFaceRule, FontRegistry, FontSourcesView,
};
use policy::{Decision, ResourcePolicy, ResourcePolicyPatch, RuntimeResourceModes};
use request::{Begin, DocTable, ImageEvent, Requester};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::DocumentId;
use ui4me_engine_types::resource_ids::{RequestId, ResourceRef};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use uri::ResourceKind;
pub use url::Url;
use view::{AppliedBatch, ImageElementData, ImageState, PaintImage};
use worker::{CancelToken, CompletionQueue, Job, JobContext, WorkerPool};

/// Answers whether an installed font family of that name exists.
pub type LocalFamilyLookup = Arc<dyn Fn(&str) -> bool + Send + Sync>;

struct DocEntry {
    /// Names of the `ui4me-res:` bundles this document referenced (its own URL, a linked sheet or
    /// an image request), whether or not the reference succeeded: a later change of such a
    /// bundle reaches the document.
    bundle_refs: BTreeSet<String>,
    table: DocTable,
    faces: FaceSet,
    policy: Arc<ResourcePolicy>,
    document_url: Url,
}

/// The facade of U4C-RES for images, stylesheet text and (via `fonts`) font sources.
pub struct ResourceManager {
    t0: Instant,
    modes: RuntimeResourceModes,
    defaults: ResourcePolicy,
    cache: Arc<Mutex<Cache>>,
    bundles: Arc<bundles::BundleRegistry>,
    ctx: Arc<JobContext>,
    /// Device-scale SVG rasters, made on the owner thread (T052).
    svg_rasters: svg::raster_cache::SvgRasterCache,
    completions: Arc<CompletionQueue>,
    pool: Option<WorkerPool>,
    deterministic_jobs: deterministic::JobQueue,
    /// Admission failures coalesce per current request, never disappear into a full queue.
    rejected: BTreeMap<RequestId, worker::Completion>,
    docs: HashMap<DocumentId, DocEntry>,
    /// Per document slot index: the highest generation whose state was released (`release_document`). A released
    /// document answers `DOCUMENT_CLOSED` like a closed one while its memory is gone: after N create/close cycles the
    /// manager holds one number per slot, not one `DocEntry` per document ever made.
    released_upto: Vec<u32>,
    settled: HashMap<DocumentId, Vec<Box<dyn FnOnce() + Send>>>,
    log: DiagnosticLog,
    shut_down: bool,
    deadline_override: Option<Duration>,
    fonts: FontRegistry,
    next_face_id: u64,
    /// Documents whose faces changed state since the last drain step (font epoch batch).
    font_dirty: BTreeSet<DocumentId>,
    /// Whether an installed font family of that name exists (system mode; installed by the runtime
    /// from 009's platform collection). `local()` sources that match no registered font ask it.
    local_family: Option<LocalFamilyLookup>,
}

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

impl ResourceManager {
    /// Create the runtime-wide manager. `defaults` are the runtime resource defaults; the modes are
    /// fixed for the runtime's lifetime. In deterministic resource mode no pool is ever created.
    pub fn new(
        modes: RuntimeResourceModes,
        defaults: ResourcePolicy,
        decoder: Arc<dyn RasterDecoder>,
    ) -> Result<ResourceManager, Status> {
        Self::new_with_system_fonts(modes, defaults, decoder, None)
    }

    /// As `new`, with the result of the platform's system-font enumeration (`None` = assume usable
    /// faces exist when system fonts are enabled). The effective font source set is validated here,
    /// once: no usable face fails with `ENGINE_MISCONFIGURED` (FR-054).
    pub fn new_with_system_fonts(
        modes: RuntimeResourceModes,
        defaults: ResourcePolicy,
        decoder: Arc<dyn RasterDecoder>,
        system_fonts_available: Option<bool>,
    ) -> Result<ResourceManager, Status> {
        let fonts = FontRegistry::for_runtime(
            &modes.fonts,
            defaults.font_policy.system_fonts,
            system_fonts_available,
        )?;
        let cache = Arc::new(Mutex::new(Cache::new(
            defaults.cache_budget,
            defaults.decoded_memory_budget,
        )));
        let svg_rasters = svg::raster_cache::SvgRasterCache::new(defaults.decoded_memory_budget);
        let bundles = Arc::new(bundles::BundleRegistry::new());
        let ctx = Arc::new(JobContext {
            decoder,
            svg: Arc::new(svg::usvg_adapter::UsvgRenderer),
            cache: cache.clone(),
            bundles: bundles.clone(),
        });
        let completions = CompletionQueue::new();
        completions.set_cache(&cache);
        Ok(ResourceManager {
            t0: Instant::now(),
            modes,
            defaults,
            cache,
            bundles,
            ctx,
            svg_rasters,
            completions,
            pool: None,
            deterministic_jobs: deterministic::JobQueue::default(),
            rejected: BTreeMap::new(),
            docs: HashMap::new(),
            released_upto: Vec::new(),
            settled: HashMap::new(),
            log: DiagnosticLog::new(DEFAULT_LOG_BOUND),
            shut_down: false,
            deadline_override: None,
            fonts,
            next_face_id: 1,
            font_dirty: BTreeSet::new(),
            local_family: None,
        })
    }

    pub fn deterministic_resources(&self) -> bool {
        self.modes.deterministic_resources
    }

    /// Install the wake-up the host runtime posts when a worker completion arrives (AD-03).
    pub fn set_wake(&self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.completions.set_wake(wake);
    }

    /// `resource_defaults_set`: affects documents created afterwards (and the runtime budgets).
    pub fn resource_defaults_set(&mut self, patch: &ResourcePolicyPatch) -> Result<(), Status> {
        let merged = self.defaults.merged(patch)?;
        let mut cache = self.cache.lock().expect("cache lock");
        cache.cache_budget = merged.cache_budget;
        cache.memory_budget = merged.decoded_memory_budget;
        drop(cache);
        self.defaults = merged;
        Ok(())
    }

    pub fn defaults(&self) -> &ResourcePolicy {
        &self.defaults
    }

    /// Set the independent SVG raster allowance before any document or raster
    /// exists. It is a separate charge from the decoded image cache; callers
    /// must include both when describing aggregate asset limits.
    pub fn set_svg_raster_budget_before_use(&mut self, bytes: u64) -> Result<(), Status> {
        if bytes == 0
            || !self.docs.is_empty()
            || !self.released_upto.is_empty()
            || self.svg_rasters.stats().rasterisations != 0
        {
            return Err(Status::new(Code::InvalidState, Stage::Resource)
                .with_message("SVG budget must be positive and set before resource use"));
        }
        self.svg_rasters = svg::raster_cache::SvgRasterCache::new(bytes);
        Ok(())
    }

    /// Create the per-document state: runtime defaults merged with `patch`, plus the implicit root
    /// when the document was loaded from `document_file`; then frozen.
    pub fn create_document(
        &mut self,
        document: DocumentId,
        document_url: Url,
        document_file: Option<&Path>,
        patch: &ResourcePolicyPatch,
    ) -> Result<(), Status> {
        if self.shut_down {
            return Err(err(
                Code::InvalidState,
                "the runtime resource manager was shut down",
            ));
        }
        if self.docs.contains_key(&document) || self.is_released(document) {
            return Err(err(
                Code::InvalidState,
                "document resource identity already exists or was released",
            ));
        }
        if self.docs.len() >= 32 {
            return Err(err(
                Code::BudgetExceeded,
                "at most 32 resource document scopes may be retained",
            ));
        }
        let mut policy = self
            .defaults
            .merged(patch)?
            .with_document_file(document_file)?;
        if self.modes.fonts.pinned.is_some() {
            policy.font_policy.system_fonts = false;
        }
        if self.pool.is_none() && !self.modes.deterministic_resources {
            // fixed at the first document
            self.pool = Some(WorkerPool::start(
                policy.worker_threads as usize,
                self.ctx.clone(),
                self.completions.clone(),
            ));
        }
        let mut bundle_refs = BTreeSet::new();
        if let Some(b) = bundles::bundle_of(&document_url) {
            bundle_refs.insert(b.to_string());
        }
        self.docs.insert(
            document,
            DocEntry {
                bundle_refs,
                table: DocTable::new(document, self.t0),
                faces: FaceSet::default(),
                policy: Arc::new(policy),
                document_url,
            },
        );
        Ok(())
    }

    fn doc(&self, document: DocumentId) -> Result<&DocEntry, Status> {
        match self.docs.get(&document) {
            Some(d) if d.table.live => Ok(d),
            Some(_) => Err(err(Code::DocumentClosed, "document is closed")),
            None if self.is_released(document) => {
                Err(err(Code::DocumentClosed, "document is closed"))
            }
            None => Err(err(Code::StaleHandle, "unknown document")),
        }
    }

    /// Documents with an entry (live, or closed and not yet released). Leak diagnostics.
    #[doc(hidden)]
    pub fn retained_documents(&self) -> usize {
        self.docs.len()
    }

    /// Retire interests of removed DOM generations. Public HTML IDs are not
    /// identities: a replacement node must request its own resources.
    pub fn retain_nodes(
        &mut self,
        document: DocumentId,
        live: &std::collections::HashSet<ui4me_engine_types::ids::NodeId>,
    ) {
        let Some(entry) = self.docs.get_mut(&document) else {
            return;
        };
        let faces: std::collections::HashSet<u64> =
            entry.faces.views().iter().map(|face| face.id.0).collect();
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        entry.table.retain_requesters(
            |requester| match requester {
                Requester::ImgElement(node) | Requester::CssImage { node, .. } => {
                    live.contains(node)
                }
                Requester::FontFace(face) => faces.contains(face),
                _ => true,
            },
            &mut cache,
        );
        self.rejected.retain(|request, _| {
            request.document != document || entry.table.request_by_id(*request).is_some()
        });
        drop(cache);
        if let Some(pool) = &self.pool {
            pool.purge_cancelled();
        }
    }

    /// `true` for a document whose state was dropped by [`ResourceManager::release_document`].
    fn is_released(&self, document: DocumentId) -> bool {
        let r = document.raw();
        self.released_upto
            .get(r.index as usize)
            .is_some_and(|&g| r.generation.get() <= g)
    }

    /// The code of a reference to a document with no entry: closed (released) or never known.
    fn missing_document_code(&self, document: DocumentId) -> Code {
        if self.is_released(document) {
            Code::DocumentClosed
        } else {
            Code::StaleHandle
        }
    }

    /// Drop everything held for a closed document (its requests, policy, faces and table) and remember only that it
    /// was closed. The runtime calls this when the document reaches `Closed`; completions that arrive later find
    /// no entry and are discarded with their cache pin released (see `process_completions`).
    pub fn release_document(&mut self, document: DocumentId) {
        self.close_document(document);
        self.font_dirty.remove(&document);
        if self.docs.remove(&document).is_some() {
            let r = document.raw();
            let idx = r.index as usize;
            if self.released_upto.len() <= idx {
                self.released_upto.resize(idx + 1, 0);
            }
            self.released_upto[idx] = self.released_upto[idx].max(r.generation.get());
        }
    }

    /// `resource_policy_get`: the effective policy.
    pub fn resource_policy_get(&self, document: DocumentId) -> Result<Arc<ResourcePolicy>, Status> {
        self.doc(document).map(|d| d.policy.clone())
    }

    pub fn document_url(&self, document: DocumentId) -> Result<Url, Status> {
        self.doc(document).map(|d| d.document_url.clone())
    }

    /// Document close (`Closing`): cancel its requests; late completions are `Discarded`.
    pub fn close_document(&mut self, document: DocumentId) {
        if let Some(d) = self.docs.get_mut(&document) {
            let mut cache = self.cache.lock().expect("cache lock");
            d.table.close(&mut cache);
        }
        self.settled.remove(&document);
        self.rejected
            .retain(|request, _| request.document != document);
        if let Some(pool) = &self.pool {
            pool.purge_cancelled();
        }
    }

    fn deadline_for(&self, policy: &ResourcePolicy) -> Instant {
        Instant::now()
            + self
                .deadline_override
                .unwrap_or(Duration::from_millis(policy.decode_deadline_ms))
    }

    #[doc(hidden)]
    pub fn set_deadline_override_for_testing(&mut self, d: Option<Duration>) {
        self.deadline_override = d;
    }

    /// Request an image for `requester` (`<img src>`, or a CSS image via `request_css_image`).
    /// `reference` is resolved against `base`; `density` is the selected `srcset` density (1.0 for
    /// `src`). Resolution and policy are synchronous; a denial makes the request Broken at once.
    pub fn request_image(
        &mut self,
        document: DocumentId,
        requester: Requester,
        base: &Url,
        reference: &str,
        density: f32,
    ) -> Result<ResourceRef, Status> {
        if self.shut_down {
            return Err(err(
                Code::InvalidState,
                "the runtime resource manager was shut down",
            ));
        }
        let policy = self.doc(document)?.policy.clone();
        if !self.doc(document)?.table.can_begin(&requester) {
            return Err(err(
                Code::BudgetExceeded,
                "document resource-request limit reached (4096)",
            ));
        }
        let resolved: Result<Url, Code> = match uri::resolve(base, reference) {
            Err(s) => Err(s.code),
            Ok(url) => match policy::check(&url, &policy) {
                Decision::Deny(code) => Err(code),
                Decision::Permit(_) => Ok(url),
            },
        };
        let deadline = self.deadline_for(&policy);
        let cancel = CancelToken::new();
        let entry = self.docs.get_mut(&document).expect("checked");
        if let Some(b) = resolved.as_ref().ok().and_then(bundles::bundle_of) {
            entry.bundle_refs.insert(b.to_string());
        }
        let mut cache = self.cache.lock().expect("cache lock");
        let begun = entry.table.begin(
            requester,
            ResourceKind::Image,
            resolved.clone(),
            density,
            cancel.clone(),
            deadline,
            &mut cache,
        );
        drop(cache);
        match begun {
            Begin::Unchanged(r) | Begin::Resolved(r) => Ok(r),
            Begin::Schedule(r) => {
                let job = Job {
                    request: r.request,
                    revision: r.revision,
                    url: resolved.expect("scheduled requests resolved"),
                    kind: ResourceKind::Image,
                    policy,
                    cancel,
                    deadline,
                };
                self.submit(job);
                Ok(r)
            }
        }
    }

    /// `request_css_image`: de-duplicated per (node, property, layer, url); no events, diagnostics only.
    pub fn request_css_image(
        &mut self,
        document: DocumentId,
        node: ui4me_engine_types::ids::NodeId,
        property: &str,
        layer: u32,
        base: &Url,
        url: &str,
    ) -> Result<ResourceRef, Status> {
        let requester = Requester::CssImage {
            node,
            property: property.to_string(),
            layer,
            url: url.to_string(),
        };
        // Replacing one CSS layer retires its old URL interest. Keeping the URL
        // in requester identity must not retain every historical style value.
        self.doc(document)?;
        if let Some(entry) = self.docs.get_mut(&document) {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            entry.table.retain_requesters(|old| !matches!(old,
                Requester::CssImage { node: old_node, property: old_property, layer: old_layer, url: old_url }
                    if *old_node == node && old_property == property && *old_layer == layer && old_url != url), &mut cache);
            self.rejected.retain(|request, _| {
                request.document != document || entry.table.request_by_id(*request).is_some()
            });
        }
        if let Some(pool) = &self.pool {
            pool.purge_cancelled();
        }
        self.request_image(document, requester, base, url, 1.0)
    }

    /// `<img src srcset>`: select the candidate for `dpr` (density descriptors only) and request it.
    /// A scale change that selects another candidate is a new revision; the current image stays
    /// displayed until the new one is Ready or Broken. `srcset` with width descriptors loads nothing
    /// and is reported (`html.srcset-width`); `src` is then used if present. Returns `None` when
    /// nothing is requestable.
    pub fn request_img(
        &mut self,
        document: DocumentId,
        node: ui4me_engine_types::ids::NodeId,
        base: &Url,
        src: Option<&str>,
        srcset: Option<&str>,
        dpr: f32,
    ) -> Result<Option<ResourceRef>, Status> {
        let mut candidates = Vec::new();
        if let Some(s) = srcset.filter(|s| !s.trim().is_empty()) {
            match request::parse_srcset(s) {
                request::SrcsetParse::Densities(c) => candidates = c,
                request::SrcsetParse::WidthDescriptors => self.report_excluded_requester(
                    document,
                    node,
                    request::ExcludedRequester::SrcsetWidthDescriptors,
                ),
            }
        }
        match request::select_candidate(candidates, src, dpr) {
            Some(c) => self
                .request_image(
                    document,
                    Requester::ImgElement(node),
                    base,
                    &c.url,
                    c.density,
                )
                .map(Some),
            None => Ok(None),
        }
    }

    /// Report a root inline `<svg>` once (T053, OD-09): `UNSUPPORTED_FEATURE` `html.inline-svg`.
    pub fn report_inline_svg(
        &mut self,
        document: DocumentId,
        node: ui4me_engine_types::ids::NodeId,
    ) {
        self.log.report_unsupported(
            svg::INLINE_SVG_KEY,
            diagnostics::ReportSubject::Element(node),
        );
        let _ = document;
    }

    fn submit(&mut self, job: Job) {
        let accepted = if self.modes.deterministic_resources {
            self.deterministic_jobs.push(job.clone())
        } else {
            self.pool
                .as_ref()
                .is_some_and(|pool| pool.submit(job.clone()))
        };
        if !accepted {
            self.rejected.insert(
                job.request,
                worker::failed_completion(&job, "asset work admission is full or retired"),
            );
        }
    }

    /// `process_resource_completions` and the event-loop idle drain point. Non-blocking. In
    /// deterministic resource mode it first runs every pending job synchronously on the owner
    /// thread. Everything applied for one document forms one batch (one epoch increment, one
    /// relayout). Never call from inside a callback or a Flush read.
    pub fn process_completions(&mut self) -> Vec<AppliedBatch> {
        if self.shut_down {
            return Vec::new();
        }
        if self.modes.deterministic_resources {
            self.deterministic_jobs
                .run_all(&self.ctx, &self.completions);
        }
        let mut per_doc: BTreeMap<DocumentId, Vec<worker::Completion>> = BTreeMap::new();
        for c in self.completions.take_all() {
            per_doc.entry(c.request.document).or_default().push(c);
        }
        for (_, c) in std::mem::take(&mut self.rejected) {
            per_doc.entry(c.request.document).or_default().push(c);
        }
        let mut batches: Vec<AppliedBatch> = Vec::new();
        let mut ids: Vec<DocumentId> = self.docs.keys().copied().collect();
        ids.sort();
        let mut unsupported = Vec::new();
        let mut font_events = Vec::new();
        for id in &ids {
            let cs = per_doc.remove(id).unwrap_or_default();
            let entry = self.docs.get_mut(id).expect("listed");
            let mut cache = self.cache.lock().expect("cache lock");
            let mut notes = Vec::new();
            let mut fonts = Vec::new();
            if let Some(batch) = entry
                .table
                .apply_batch(cs, &mut cache, &mut notes, &mut fonts)
            {
                for r in &batch.changed {
                    if let Some(rec) = entry.table.record_of(r.request) {
                        self.log.push(rec);
                    }
                }
                batches.push(batch);
            }
            unsupported.extend(notes);
            font_events.extend(fonts);
        }
        for (req, key) in unsupported {
            self.log
                .report_unsupported(key, diagnostics::ReportSubject::Request(req));
        }
        // font completions: fold into the faces; a failed source starts the next one
        for ev in font_events {
            self.apply_font_event(ev);
        }
        self.flush_font_epoch(&mut batches);
        // stale completions of documents we no longer know
        for (_, cs) in per_doc {
            let mut cache = self.cache.lock().expect("cache lock");
            for c in cs {
                if let worker::JobOutcome::Loaded { image, .. } = &c.outcome {
                    cache.unpin(&image.key);
                }
            }
        }
        // settled notifications
        let ready: Vec<DocumentId> = self
            .settled
            .keys()
            .copied()
            .filter(|d| self.docs.get(d).is_some_and(|e| e.table.pending() == 0))
            .collect();
        for d in ready {
            for cb in self.settled.remove(&d).unwrap_or_default() {
                cb();
            }
        }
        batches
    }

    /// One font-epoch increment per drain step that changed any face (FR-049, FR-053); every
    /// document with a changed face is named in its own batch (`font_epoch_changed`), so 009
    /// invalidates only those documents (IF-04, 009 FR-097).
    fn flush_font_epoch(&mut self, batches: &mut Vec<AppliedBatch>) {
        if self.font_dirty.is_empty() {
            return;
        }
        self.fonts.bump_for_faces();
        for d in std::mem::take(&mut self.font_dirty) {
            let Some(entry) = self.docs.get(&d) else {
                continue;
            };
            match batches.iter_mut().find(|b| b.document == d) {
                Some(b) => b.font_epoch_changed = true,
                None => batches.push(AppliedBatch {
                    document: d,
                    resource_epoch: entry.table.epoch,
                    changed: Vec::new(),
                    intrinsic_size: Vec::new(),
                    paint_only: Vec::new(),
                    font_epoch_changed: true,
                }),
            }
        }
    }

    fn apply_font_event(&mut self, ev: request::FontEvent) {
        let document = ev.reference.document;
        let face_id = FaceId(ev.face);
        let Some(entry) = self.docs.get_mut(&document) else {
            return;
        };
        let url_name = entry
            .table
            .request_by_id(ev.reference.request)
            .and_then(|r| r.url.as_ref())
            .and_then(|u| {
                u.path_segments()
                    .and_then(|mut s| s.next_back().map(str::to_string))
            })
            .unwrap_or_else(|| "<font>".to_string());
        let Some(face) = entry.faces.get_mut(face_id) else {
            return;
        };
        let FaceState::Loading { source_index } = face.state.clone() else {
            return;
        };
        match ev.result {
            Ok(bytes) => {
                let id = self.fonts.alloc_blob_id();
                face.state = FaceState::Loaded(FontBlob {
                    id,
                    name: url_name,
                    bytes,
                    family_override: Some(face.rule.family.clone()),
                });
                self.font_dirty.insert(document);
            }
            Err(code) => {
                face.failures.push(code);
                self.advance_face(document, face_id, source_index + 1);
            }
        }
    }

    /// Try `sources[from..]` in order until one starts loading, resolves, or all have failed.
    fn advance_face(&mut self, document: DocumentId, face_id: FaceId, from: usize) {
        let Some(entry) = self.docs.get_mut(&document) else {
            return;
        };
        let policy = entry.policy.clone();
        let can_begin = entry.table.can_begin(&Requester::FontFace(face_id.0));
        let Some(face) = entry.faces.get_mut(face_id) else {
            return;
        };
        if !can_begin {
            face.failures.push(Code::BudgetExceeded);
            face.state = FaceState::Failed(face.failures.clone());
            self.font_dirty.insert(document);
            return;
        }
        let mut idx = from;
        while idx < face.rule.sources.len() {
            match face.rule.sources[idx].clone() {
                FaceSourceSpec::Url {
                    format: Some(h), ..
                } if fonts::validate::is_unsupported_format_hint(&h) => {
                    face.failures.push(Code::UnsupportedMediaType); // skipped without a read (FR-046)
                }
                FaceSourceSpec::Url {
                    reference, base, ..
                } => match uri::resolve(&base, &reference) {
                    Err(s) => face.failures.push(s.code),
                    Ok(url) => match policy::check(&url, &policy) {
                        Decision::Deny(code) => face.failures.push(code),
                        Decision::Permit(uri::Scheme::File)
                        | Decision::Permit(uri::Scheme::Data)
                        | Decision::Permit(uri::Scheme::UiRes) => {
                            if let Some(b) = bundles::bundle_of(&url) {
                                entry.bundle_refs.insert(b.to_string());
                            }
                            face.state = FaceState::Loading { source_index: idx };
                            let deadline = Instant::now()
                                + self
                                    .deadline_override
                                    .unwrap_or(Duration::from_millis(policy.decode_deadline_ms));
                            let cancel = CancelToken::new();
                            let mut cache = self.cache.lock().expect("cache lock");
                            let begun = entry.table.begin(
                                Requester::FontFace(face_id.0),
                                ResourceKind::Font,
                                Ok(url.clone()),
                                1.0,
                                cancel.clone(),
                                deadline,
                                &mut cache,
                            );
                            drop(cache);
                            if let Begin::Schedule(r) = begun {
                                let job = Job {
                                    request: r.request,
                                    revision: r.revision,
                                    url,
                                    kind: ResourceKind::Font,
                                    policy: policy.clone(),
                                    cancel,
                                    deadline,
                                };
                                self.submit(job);
                            }
                            return;
                        }
                        Decision::Permit(_) => face.failures.push(Code::ResourceDenied),
                    },
                },
                FaceSourceSpec::Local(name) => {
                    // local() against host-registered fonts; 009 resolves it against the system
                    // fonts it enumerates (cross-feature item).
                    let hit = self.fonts.blobs().iter().find_map(|b| {
                        b.family_override
                            .as_deref()
                            .filter(|f| f.eq_ignore_ascii_case(&name))
                            .map(str::to_string)
                    });
                    match hit {
                        // a host-registered family: the declared family becomes an alias of it (its blob is
                        // already in the collection; registering it a second time would duplicate its id)
                        Some(family) => {
                            face.state = FaceState::LocalFamily(family);
                            self.font_dirty.insert(document);
                            return;
                        }
                        None if self.local_family.as_ref().is_some_and(|f| f(&name)) => {
                            face.state = FaceState::LocalFamily(name);
                            self.font_dirty.insert(document);
                            return;
                        }
                        None => face.failures.push(Code::FileNotFound),
                    }
                }
            }
            idx += 1;
        }
        face.state = FaceState::Failed(face.failures.clone());
        self.font_dirty.insert(document);
    }

    /// Install the lookup that answers whether an installed (system) font family exists, for
    /// `local()` sources (system mode only; the lookup is 009's, 007 holds no font library type).
    pub fn set_local_family_lookup(&mut self, lookup: Option<LocalFamilyLookup>) {
        self.local_family = lookup;
    }

    /// `sync_font_faces`: register the document's effective `@font-face` rules at each style update.
    pub fn sync_font_faces(
        &mut self,
        document: DocumentId,
        rules: &[FontFaceRule],
    ) -> Result<Vec<FaceId>, Status> {
        self.doc(document)?;
        let entry = self.docs.get_mut(&document).expect("checked");
        let ids = entry.faces.sync(rules, &mut self.next_face_id);
        let live: std::collections::HashSet<u64> = ids.iter().map(|id| id.0).collect();
        {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            entry.table.retain_requesters(
                |requester| !matches!(requester, Requester::FontFace(id) if !live.contains(id)),
                &mut cache,
            );
        }
        self.rejected.retain(|request, _| {
            request.document != document || entry.table.request_by_id(*request).is_some()
        });
        for r in rules.iter().filter(|r| r.display.is_some()) {
            let _ = r; // reported on the face view as `font.display` (info)
        }
        Ok(ids)
    }

    /// `request_face` from 009 at the first layout that selects the face: non-blocking and
    /// idempotent; completion bumps the font epoch once per batch.
    pub fn request_face(&mut self, document: DocumentId, face: FaceId) -> Result<(), Status> {
        if self.shut_down {
            return Err(err(
                Code::InvalidState,
                "the runtime resource manager was shut down",
            ));
        }
        self.doc(document)?;
        let entry = self.docs.get_mut(&document).expect("checked");
        let Some(f) = entry.faces.get_mut(face) else {
            return Err(err(Code::StaleHandle, "unknown or removed face"));
        };
        if f.requested {
            return Ok(());
        }
        f.requested = true;
        self.advance_face(document, face, 0);
        Ok(())
    }

    /// `font_sources(document)`: the runtime sources plus this document's faces.
    pub fn font_sources(&self, document: DocumentId) -> Result<FontSourcesView, Status> {
        let d = self.doc(document)?;
        let mut v = self.fonts.font_sources();
        v.faces = d.faces.views();
        Ok(v)
    }

    /// The document's faces with their current state (a cheap part of [`ResourceManager::font_sources`]).
    pub fn document_faces(&self, document: DocumentId) -> Result<Vec<fonts::FaceView>, Status> {
        Ok(self.doc(document)?.faces.views())
    }

    /// The runtime view without faces (for callers that have no document).
    pub fn runtime_font_sources(&self) -> FontSourcesView {
        self.fonts.font_sources()
    }

    pub fn font_epoch(&self) -> FontEpoch {
        self.fonts.epoch()
    }

    /// `font_register(path)`: a trusted host declaration (no document root applies).
    pub fn font_register_file(
        &mut self,
        path: &Path,
        family_override: Option<String>,
    ) -> Result<FontBlobId, Status> {
        self.fonts.register_file_with_family(path, family_override)
    }

    /// `font_register(bytes)`.
    pub fn font_register_bytes(
        &mut self,
        name: &str,
        bytes: Arc<[u8]>,
        family_override: Option<String>,
    ) -> Result<FontBlobId, Status> {
        self.fonts
            .register_bytes_with_family(name, bytes, family_override)
    }

    /// `font_unregister`: idempotent; `INVALID_STATE` if it would leave no usable face.
    pub fn font_unregister(&mut self, id: FontBlobId) -> Result<(), Status> {
        self.fonts.unregister(id)
    }

    /// `font_config_get`.
    pub fn font_config_get(&self) -> FontConfigReport {
        self.fonts.config_report(self.modes.deterministic_resources)
    }

    /// Completions posted by workers (or by the deterministic runner) and not yet applied.
    pub fn queued_completions(&self) -> usize {
        self.completions.len() + self.rejected.len()
    }

    /// Whether a drain step has work: deterministic-mode jobs not yet run, or completions posted
    /// and not yet applied (the event loop does not sleep while this is true).
    pub fn has_queued_work(&self) -> bool {
        !self.deterministic_jobs.is_empty()
            || !self.completions.is_empty()
            || !self.rejected.is_empty()
    }

    /// Report a requester that loads nothing (T022): once per element, `UNSUPPORTED_FEATURE` under
    /// 005's `html` root. Discovered through 003's `requesters_enumerated` / `requester_changed`.
    pub fn report_excluded_requester(
        &mut self,
        document: DocumentId,
        element: ui4me_engine_types::ids::NodeId,
        kind: request::ExcludedRequester,
    ) {
        self.log.report_unsupported(
            kind.report_key(),
            diagnostics::ReportSubject::Element(element),
        );
        let _ = document;
    }

    /// Report an unsupported CSS rule feature (e.g. `cursor: url()`), once per rule.
    pub fn report_excluded_rule(&mut self, rule: u64, kind: request::ExcludedRequester) {
        self.log
            .report_unsupported(kind.report_key(), diagnostics::ReportSubject::Rule(rule));
    }

    /// `resources_pending(document)`.
    pub fn pending(&self, document: DocumentId) -> Result<u32, Status> {
        Ok(self.doc(document)?.table.pending())
    }

    /// `on_resources_settled(document, callback)`: fires at the first drain point where nothing is
    /// pending (immediately-settled documents fire at the next drain point too).
    pub fn on_resources_settled(
        &mut self,
        document: DocumentId,
        callback: Box<dyn FnOnce() + Send>,
    ) -> Result<(), Status> {
        self.doc(document)?;
        self.settled.entry(document).or_default().push(callback);
        Ok(())
    }

    /// `natural_size(ref)` for 008.
    pub fn natural_size(&self, r: &ResourceRef) -> ImageState {
        match self.docs.get(&r.document) {
            None => ImageState::Broken {
                code: self.missing_document_code(r.document),
            },
            Some(d) if !d.table.live => ImageState::Broken {
                code: Code::DocumentClosed,
            },
            Some(d) => d
                .table
                .get(r)
                .map(|q| q.image_state())
                .unwrap_or(ImageState::Broken {
                    code: Code::StaleHandle,
                }),
        }
    }

    /// Whether the image shown for `r` is a vector image (SVG), i.e. whether `paint_image` honours
    /// its size argument. False while pending, broken or unknown.
    pub fn is_vector(&self, r: &ResourceRef) -> bool {
        let Some(d) = self.docs.get(&r.document) else {
            return false;
        };
        d.table.get(r).is_some_and(|q| {
            q.shown
                .as_ref()
                .is_some_and(|(img, _)| img.vector.is_some())
        })
    }

    /// Counters of the device-scale SVG raster cache (rasterisations, hits, clamped requests).
    pub fn svg_raster_stats(&self) -> svg::raster_cache::RasterStats {
        self.svg_rasters.stats()
    }

    /// `paint_image(ref, device_size_px)` for 010. For raster images `device_size_px` is ignored;
    /// an SVG is rasterised synchronously (owner-thread work) at that size the first time it is
    /// asked for (T052).
    pub fn paint_image(&self, r: &ResourceRef, device_size_px: (u32, u32)) -> PaintImage {
        match self.docs.get(&r.document) {
            None => PaintImage::Broken {
                code: self.missing_document_code(r.document),
            },
            Some(d) if !d.table.live => PaintImage::Broken {
                code: Code::DocumentClosed,
            },
            Some(d) => match d.table.get(r) {
                None => PaintImage::Broken {
                    code: Code::StaleHandle,
                },
                Some(q) => match (&q.shown, q.broken) {
                    (Some((img, _)), _) if img.vector.is_some() => {
                        match self.svg_rasters.raster(img, device_size_px) {
                            Ok(p) => PaintImage::Raster {
                                pixels: p.raster,
                                generation: p.generation,
                            },
                            Err(Code::InvalidArgument) => PaintImage::NotReady,
                            Err(code) => PaintImage::Broken { code },
                        }
                    }
                    (Some((img, _)), _) => PaintImage::Raster {
                        pixels: img.raster.clone(),
                        generation: img.generation,
                    },
                    (None, Some(code)) => PaintImage::Broken { code },
                    (None, None) => PaintImage::NotReady,
                },
            },
        }
    }

    /// Host data per image element (`complete`, natural size, failure code).
    pub fn image_element_data(&self, r: &ResourceRef) -> Option<ImageElementData> {
        self.docs.get(&r.document)?.table.element_data(r)
    }

    /// Queued `load`/`error` events of `<img>` elements, for 012 to dispatch as idle work.
    pub fn take_image_events(&mut self, document: DocumentId) -> Vec<ImageEvent> {
        self.docs
            .get_mut(&document)
            .map(|d| d.table.take_events())
            .unwrap_or_default()
    }

    pub fn resource_epoch(
        &self,
        document: DocumentId,
    ) -> Result<ui4me_engine_types::resource_ids::ResourceEpoch, Status> {
        Ok(self.doc(document)?.table.epoch)
    }

    /// Synchronous stylesheet text for 006 (`load_text_sync`).
    pub fn load_text_sync(
        &mut self,
        document: DocumentId,
        base: &Url,
        reference: &str,
        kind: ResourceKind,
    ) -> Result<text::TextResource, Status> {
        let d = self.doc(document)?;
        let policy = d.policy.clone();
        // Remember a bundle the sheet comes from even if the read fails (the bundle may be
        // registered later): its registration then reaches this document.
        if let Ok(url) = uri::resolve(base, reference) {
            if let Some(b) = bundles::bundle_of(&url) {
                let b = b.to_string();
                self.docs
                    .get_mut(&document)
                    .expect("checked")
                    .bundle_refs
                    .insert(b);
            }
        }
        text::load_text_sync(&self.bundles, &policy, base, reference, kind)
    }

    /// The runtime's `ui4me-res:` bundle registry (OD-19, approved).
    pub fn bundles(&self) -> &Arc<bundles::BundleRegistry> {
        &self.bundles
    }

    /// `bundle_register`: register `entries` as bundle `name`, replacing a bundle of that name.
    /// Every live document that referenced the bundle is reported in the result (the host
    /// runtime advances its resource epoch and re-reads its linked sheets); this manager already
    /// dropped the bundle's cached decodes and started its images again (new revisions, the old
    /// image stays shown until the new one is Ready or Broken). `INVALID_ARGUMENT` for a bad name
    /// or path, `SIZE_LIMIT` for a bundle over its limits; nothing changes on error.
    pub fn bundle_register(
        &mut self,
        name: &str,
        entries: Vec<bundles::BundleEntryInput>,
    ) -> Result<bundles::BundleChange, Status> {
        if self.shut_down {
            return Err(err(
                Code::InvalidState,
                "the runtime resource manager was shut down",
            ));
        }
        let existed = self.bundles.contains(name);
        let impact = self.bundles.impact(name, Some(&entries));
        self.bundles.register(name, entries)?;
        Ok(self.bundle_changed(name, existed, impact))
    }

    /// `bundle_register` from a directory tree read now (an immutable snapshot, see `bundles::read_dir`).
    pub fn bundle_register_dir(
        &mut self,
        name: &str,
        dir: &Path,
    ) -> Result<bundles::BundleChange, Status> {
        if !bundles::valid_name(name) {
            return Err(err(
                Code::InvalidArgument,
                format!("bundle name {name:?} must match [a-z0-9][a-z0-9._-]{{0,62}}"),
            ));
        }
        let entries = bundles::read_dir(dir)?;
        self.bundle_register(name, entries)
    }

    /// `bundle_unregister`: idempotent. Documents that referenced the bundle are reported as for
    /// a replacement (their images and sheets from it become `FILE_NOT_FOUND`).
    pub fn bundle_unregister(&mut self, name: &str) -> bundles::BundleChange {
        let impact = self.bundles.impact(name, None);
        let existed = self.bundles.unregister(name);
        if existed {
            self.bundle_changed(name, true, impact)
        } else {
            bundles::BundleChange::default()
        }
    }

    fn bundle_changed(
        &mut self,
        name: &str,
        existed: bool,
        impact: bundles::BundleImpact,
    ) -> bundles::BundleChange {
        if impact.is_empty() {
            // Byte-identical re-registration: every consumer already holds
            // exactly these bytes, so keep cached decodes and notify nobody.
            return bundles::BundleChange {
                affected: Vec::new(),
                existed,
                impact,
            };
        }
        let prefix = bundles::url_prefix(name);
        self.cache
            .lock()
            .expect("cache lock")
            .invalidate_url_prefix(&prefix);
        let mut ids: Vec<DocumentId> = self
            .docs
            .iter()
            .filter(|(_, e)| e.table.live && e.bundle_refs.contains(name))
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        // A source-only font/stylesheet change must not redownload images.
        let mut font_changed = false;
        for id in &ids {
            if impact.fonts || impact.unknown {
                let entry = self.docs.get_mut(id).expect("live document was listed");
                let invalidated = entry.faces.invalidate_sources_under(&prefix);
                if !invalidated.is_empty() {
                    let invalidated: std::collections::HashSet<u64> =
                        invalidated.into_iter().map(|f| f.0).collect();
                    let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
                    entry.table.retain_requesters(
                        |r| !matches!(r, Requester::FontFace(fid) if invalidated.contains(fid)),
                        &mut cache,
                    );
                    self.font_dirty.insert(*id);
                    font_changed = true;
                }
            }
            if !(impact.images || impact.unknown) {
                continue;
            }
            let (policy, again) = {
                let e = &self.docs[id];
                (e.policy.clone(), e.table.image_requests_under(&prefix))
            };
            for (requester, kind, url, density) in again {
                let deadline = self.deadline_for(&policy);
                let cancel = CancelToken::new();
                let entry = self.docs.get_mut(id).expect("listed");
                let mut cache = self.cache.lock().expect("cache lock");
                let begun = entry.table.begin_again(
                    requester,
                    kind,
                    Ok(url.clone()),
                    density,
                    cancel.clone(),
                    deadline,
                    &mut cache,
                );
                drop(cache);
                if let Begin::Schedule(r) = begun {
                    self.submit(Job {
                        request: r.request,
                        revision: r.revision,
                        url,
                        kind,
                        policy: policy.clone(),
                        cancel,
                        deadline,
                    });
                }
            }
        }
        if font_changed {
            self.fonts.bump_for_faces();
        }
        bundles::BundleChange {
            affected: ids,
            existed,
            impact,
        }
    }

    /// The bytes of a `ui4me-res:` document (`load_url`): policy-checked, within the stylesheet
    /// text limit (16 MiB by default). `URI_MALFORMED`, `RESOURCE_DENIED`, `FILE_NOT_FOUND`, `SIZE_LIMIT`.
    pub fn load_bundle_document(&self, url: &Url) -> Result<text::TextResource, Status> {
        text::load_text_url(&self.bundles, &self.defaults, url, ResourceKind::Stylesheet)
    }

    /// `resources_purge`: drops unreferenced cache entries; referenced entries are invalidated so
    /// their next request decodes again. Returns the number of entries dropped.
    pub fn resources_purge(&mut self, scope: PurgeScope, url: Option<&str>) -> usize {
        let mut cache = self.cache.lock().expect("cache lock");
        let n = cache.purge_unreferenced(url);
        n + cache.invalidate(&scope, url)
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.lock().expect("cache lock").stats()
    }

    /// Release only unused decoded resources. Live image pixels and registered source bytes
    /// remain available; purging is a memory-management operation, never a silent reload.
    pub fn purge_unused_resources(&mut self, url: Option<&str>) -> usize {
        let decoded = self
            .cache
            .lock()
            .expect("cache lock")
            .purge_unreferenced(url);
        decoded
            + if url.is_none() {
                self.svg_rasters.purge_unused()
            } else {
                0
            }
    }

    /// Diagnostic records retained so far (bounded).
    pub fn diagnostic_records(&self) -> Vec<ResourceDiagnosticRecord> {
        self.log.records().cloned().collect()
    }

    pub fn unsupported_reports(&self) -> Vec<diagnostics::UnsupportedReport> {
        self.log.unsupported().to_vec()
    }

    pub fn document_counters(&mut self, document: DocumentId) -> Option<DocumentCounters> {
        self.docs.get_mut(&document).map(|d| d.table.counters())
    }

    pub fn document_records(&self, document: DocumentId) -> Vec<ResourceDiagnosticRecord> {
        self.docs
            .get(&document)
            .map(|d| d.table.records(false))
            .unwrap_or_default()
    }

    pub fn runtime_diagnostics(&self) -> RuntimeResourceDiagnostics {
        RuntimeResourceDiagnostics {
            deterministic_resources: self.modes.deterministic_resources,
            worker_threads: self
                .pool
                .as_ref()
                .map(|p| p.configured_threads())
                .unwrap_or(0),
            cache: self.cache_stats(),
        }
    }

    /// Process asset workers (zero in deterministic mode). They remain for reuse.
    pub fn alive_workers(&self) -> usize {
        self.pool.as_ref().map(|p| p.alive_workers()).unwrap_or(0)
    }

    /// Handle to observe this manager's active jobs retiring after shutdown.
    pub fn alive_workers_handle(&self) -> Option<Arc<std::sync::atomic::AtomicUsize>> {
        self.pool.as_ref().map(|p| p.alive_handle())
    }

    pub fn request_id_known(&self, id: RequestId) -> bool {
        self.docs
            .get(&id.document)
            .is_some_and(|d| d.table.request_by_id(id).is_some())
    }

    /// Non-blocking shutdown (FR-059): stop intake, signal every cancel token, drop the completion
    /// queue and return. Jobs retire at cancellation checks; process workers remain.
    pub fn shutdown(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            for d in self.docs.values_mut() {
                d.table.close(&mut cache);
            }
        }
        self.deterministic_jobs.clear();
        self.rejected.clear();
        if let Some(p) = self.pool.as_mut() {
            p.stop_intake();
        }
        self.completions.close();
        self.settled.clear();
    }

    pub fn is_shut_down(&self) -> bool {
        self.shut_down
    }

    /// Canonical roots of a document's policy (tests, diagnostics).
    pub fn file_roots(&self, document: DocumentId) -> Result<Vec<PathBuf>, Status> {
        Ok(self.doc(document)?.policy.file_roots.clone())
    }
}

impl Drop for ResourceManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
