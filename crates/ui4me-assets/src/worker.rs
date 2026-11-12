//! owner: 007-resources (T023, T068, T069, T071, T073) — jobs, the bounded worker pool and the
//! pipeline a job runs (`run_job`), shared by the pool and deterministic resource mode.
//!
//! Workers never touch DOM or engine state: a job is plain data and its `Completion` is posted to
//! the owner thread's queue, applied only at a drain point. Shutdown is non-blocking: stop intake,
//! signal every token, drop the queue and return; workers exit at their next cancellation check
//! and cannot call a retired document. A process-owned fixed pool is reused by
//! later sessions; closing an application never creates replacement threads.

use crate::cache::{Cache, CachedImage, ContentKey, ResourceKey};
use crate::decode::{
    self, DecodedImage, RasterDecoder, SvgOptions, SvgRenderer, exif, scale, sniff,
};
use crate::fs_guard;
use crate::policy::{MAX_RASTER_SIDE, ResourcePolicy};
use crate::uri::{ResourceKind, Scheme};
use crate::view::ImageRaster;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::resource_ids::RequestId;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use url::Url;

/// Cooperative cancellation (document close, supersession, shutdown).
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// One unit of background work. Plain data: nothing here can reach engine state.
#[derive(Clone, Debug)]
pub struct Job {
    pub request: RequestId,
    pub revision: u32,
    pub url: Url,
    pub kind: ResourceKind,
    pub policy: Arc<ResourcePolicy>,
    pub cancel: CancelToken,
    pub deadline: Instant,
}

/// Why a job ended without an image.
#[derive(Debug)]
pub enum JobOutcome {
    Loaded {
        image: Arc<CachedImage>,
        cache_hit: bool,
    },
    /// A validated font file (TTF/OTF/TTC bytes) for an `@font-face` source.
    Font(Arc<[u8]>),
    Failed(Status),
}

/// A job's result, posted to the owner thread.
#[derive(Debug)]
pub struct Completion {
    pub request: RequestId,
    pub revision: u32,
    pub outcome: JobOutcome,
    pub finished: Instant,
    pub deadline: Instant,
}

/// Shared state a job needs; handed to worker threads.
pub struct JobContext {
    pub decoder: Arc<dyn RasterDecoder>,
    pub svg: Arc<dyn SvgRenderer>,
    pub cache: Arc<Mutex<Cache>>,
    /// The runtime's `ui4me-res:` bundles (OD-19, approved).
    pub bundles: Arc<crate::bundles::BundleRegistry>,
}

fn status(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

fn check_live(job: &Job) -> Result<(), Status> {
    if job.cancel.is_canceled() {
        return Err(status(Code::InvalidState, "canceled"));
    }
    if Instant::now() > job.deadline {
        return Err(status(Code::BudgetExceeded, "decode deadline exceeded"));
    }
    Ok(())
}

/// Releases a decoded-memory reservation on every exit path, including a decoder panic.
struct Reservation<'a> {
    cache: &'a Mutex<Cache>,
    bytes: u64,
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if let Ok(mut c) = self.cache.lock() {
            c.release_reservation(self.bytes);
        }
    }
}

fn is_svg_hint(url: &Url) -> bool {
    url.path().to_ascii_lowercase().ends_with(".svg")
}

/// The pipeline: guard/open, cache lookup, read, sniff, probe, limits, decode, orient, downscale,
/// publish. Every limit is enforced before the allocation it protects (`resource-policy.md` §4).
pub fn run_pipeline(job: &Job, ctx: &JobContext) -> Result<(Arc<CachedImage>, bool), Status> {
    check_live(job)?;
    let kind = if job.kind == ResourceKind::Image && is_svg_hint(&job.url) {
        ResourceKind::Svg
    } else {
        job.kind
    };
    let max_encoded = job.policy.max_encoded_bytes.for_kind(kind);
    match Scheme::of(&job.url) {
        Scheme::File => {
            let path = crate::uri::file_url_to_path_checked(&job.url)
                .map_err(|c| status(c, "file URL denied"))?;
            let vf = fs_guard::open_verified(&path, &job.policy.file_roots, max_encoded)?;
            let key = ResourceKey {
                url: crate::uri::without_fragment(&job.url).to_string(),
                content: ContentKey::File(vf.identity),
            };
            let lock = {
                let mut c = ctx.cache.lock().expect("cache lock");
                if let Some(hit) = c.lookup_pinned(&key) {
                    return Ok((hit, true));
                }
                c.key_lock(&key)
            };
            // One decode per key: later workers wait here (worker to worker, never the owner
            // thread) and then find the published entry.
            let held = lock.lock().unwrap_or_else(|p| p.into_inner());
            let result = (|| {
                if let Some(hit) = ctx.cache.lock().expect("cache lock").lookup_pinned(&key) {
                    return Ok((hit, true));
                }
                check_live(job)?;
                let bytes = fs_guard::read_limited(&vf.file, max_encoded)?;
                decode_and_publish(job, ctx, kind, &bytes, key.clone())
            })();
            drop(held);
            drop(lock);
            ctx.cache.lock().expect("cache lock").drop_key_lock(&key);
            result
        }
        Scheme::Data => {
            let payload =
                crate::data_url::process(job.url.as_str(), job.policy.data_url_max_bytes)?;
            check_live(job)?;
            let kind = if payload.is_svg() {
                ResourceKind::Svg
            } else {
                kind
            };
            let digest: [u8; 32] = Sha256::digest(&payload.bytes).into();
            let key = ResourceKey {
                url: "data:".to_string(),
                content: ContentKey::Data(digest),
            };
            if let Some(hit) = ctx.cache.lock().expect("cache lock").lookup_pinned(&key) {
                return Ok((hit, true));
            }
            decode_and_publish(job, ctx, kind, &payload.bytes, key)
        }
        Scheme::UiRes => {
            let hit = ctx.bundles.lookup(&job.url)?;
            let kind = if hit.is_svg() {
                ResourceKind::Svg
            } else {
                job.kind
            };
            let max = job.policy.max_encoded_bytes.for_kind(kind);
            if hit.data.len() as u64 > max {
                return Err(status(
                    Code::SizeLimit,
                    format!(
                        "bundle entry is {} bytes, over the {max} byte limit",
                        hit.data.len()
                    ),
                ));
            }
            // One decode per (URL, bundle generation): replacing the bundle changes the key.
            let key = ResourceKey {
                url: crate::uri::without_fragment(&job.url).to_string(),
                content: ContentKey::Bundle(hit.generation),
            };
            if let Some(cached) = ctx.cache.lock().expect("cache lock").lookup_pinned(&key) {
                return Ok((cached, true));
            }
            check_live(job)?;
            decode_and_publish(job, ctx, kind, &hit.data, key)
        }
        Scheme::Other => Err(status(Code::ResourceDenied, "scheme not permitted")),
    }
}

/// Sniff, probe, enforce the budgets, decode, orient, downscale and publish.
fn decode_and_publish(
    job: &Job,
    ctx: &JobContext,
    kind: ResourceKind,
    bytes: &[u8],
    key: ResourceKey,
) -> Result<(Arc<CachedImage>, bool), Status> {
    check_live(job)?;
    let started = Instant::now();
    match sniff::detect(bytes, kind == ResourceKind::Svg)? {
        sniff::Detected::Svg => {
            // The parsed tree is charged to the decoded-memory budget by a size estimate
            // (about 16 bytes per encoded byte, floor 64 KiB) before it is built.
            let estimate = (bytes.len() as u64).saturating_mul(16).max(64 * 1024);
            if !ctx.cache.lock().expect("cache lock").try_reserve(estimate) {
                return Err(status(
                    Code::BudgetExceeded,
                    format!("decoded-memory budget cannot take {estimate} bytes"),
                ));
            }
            let _reservation = Reservation {
                cache: &ctx.cache,
                bytes: estimate,
            };
            let opts = SvgOptions {
                max_data_bytes: job.policy.data_url_max_bytes,
                max_embedded_pixels: job.policy.max_decoded_pixels,
                ..SvgOptions::default()
            };
            let parsed = ctx.svg.parse(bytes, &opts)?;
            check_live(job)?;
            let (nw, nh) = (
                parsed.natural.width.map_or(0, |w| w.round() as u32),
                parsed.natural.height.map_or(0, |h| h.round() as u32),
            );
            let placeholder = Arc::new(ImageRaster {
                width: 1,
                height: 1,
                stride: 4,
                data: Arc::from(vec![0u8; 4]),
            });
            let mut cache = ctx.cache.lock().expect("cache lock");
            let image = Arc::new(CachedImage {
                key,
                raster: placeholder,
                natural_px: (nw, nh),
                downscaled: false,
                animated_first_frame: false,
                icc_ignored: false,
                encoded_bytes: bytes.len() as u64,
                decode_ms: started.elapsed().as_millis() as u64,
                generation: cache.next_generation(),
                vector: Some(Arc::new(crate::svg::VectorImage {
                    tree: parsed.tree,
                    natural: parsed.natural,
                    renderer: ctx.svg.clone(),
                    est_bytes: estimate,
                })),
                reports: parsed.reports,
                warnings: parsed.warnings,
            });
            cache.insert_with_pin(image.clone(), true);
            Ok((image, false))
        }
        sniff::Detected::Raster(format) => {
            let header = ctx.decoder.probe(format, bytes)?;
            let pixels = header.width as u64 * header.height as u64;
            if pixels > job.policy.max_decoded_pixels {
                return Err(status(
                    Code::SizeLimit,
                    format!(
                        "{}x{} = {pixels} px exceeds the {} px budget",
                        header.width, header.height, job.policy.max_decoded_pixels
                    ),
                ));
            }
            let estimate = decode::decoded_bytes(header.width, header.height);
            if !ctx.cache.lock().expect("cache lock").try_reserve(estimate) {
                return Err(status(
                    Code::BudgetExceeded,
                    format!("decoded-memory budget cannot take {estimate} bytes"),
                ));
            }
            let _reservation = Reservation {
                cache: &ctx.cache,
                bytes: estimate,
            };
            check_live(job)?;
            let decoded: DecodedImage = ctx.decoder.decode(bytes, &header, &job.cancel)?;
            check_live(job)?;
            let oriented = exif::apply_orientation(decoded, header.orientation);
            let (natural_w, natural_h) = (oriented.width, oriented.height);
            let (raster, downscaled) = scale::downscale_if_needed(oriented, MAX_RASTER_SIDE);
            let stride = raster.width * 4;
            let mut cache = ctx.cache.lock().expect("cache lock");
            let image = Arc::new(CachedImage {
                key,
                raster: Arc::new(ImageRaster {
                    width: raster.width,
                    height: raster.height,
                    stride,
                    data: Arc::from(raster.data),
                }),
                natural_px: (natural_w, natural_h),
                downscaled,
                animated_first_frame: header.animated,
                icc_ignored: header.has_icc,
                encoded_bytes: bytes.len() as u64,
                decode_ms: started.elapsed().as_millis() as u64,
                generation: cache.next_generation(),
                vector: None,
                reports: Vec::new(),
                warnings: Vec::new(),
            });
            cache.insert_with_pin(image.clone(), true);
            Ok((image, false))
        }
    }
}

/// The validated bytes of an `@font-face` file: sfnt / TTC as is, WOFF 1.0 and 2.0 converted to their sfnt.
fn face_bytes(raw: Arc<[u8]>) -> Result<Arc<[u8]>, Status> {
    if raw.starts_with(b"wOFF") || raw.starts_with(b"wOF2") {
        return Ok(Arc::from(crate::fonts::validate::prepare_face_font(
            raw.to_vec(),
        )?));
    }
    crate::fonts::validate::validate_font(&raw)?;
    Ok(raw)
}

/// Font pipeline: guard/open under the document roots, size limit (32 MiB), read, validate the
/// sfnt/TTC header and table-directory bounds (a WOFF 1.0 or 2.0 file is first converted to its sfnt). `data:` fonts go through the data-url processor (T041).
pub fn run_font_pipeline(job: &Job, ctx: &JobContext) -> Result<Arc<[u8]>, Status> {
    check_live(job)?;
    match Scheme::of(&job.url) {
        Scheme::File => {
            let path = crate::uri::file_url_to_path_checked(&job.url)
                .map_err(|c| status(c, "file URL denied"))?;
            let max = job.policy.max_encoded_bytes.for_kind(ResourceKind::Font);
            let vf = fs_guard::open_verified(&path, &job.policy.file_roots, max)?;
            check_live(job)?;
            let bytes = fs_guard::read_limited(&vf.file, max)?;
            check_live(job)?;
            face_bytes(Arc::from(bytes))
        }
        Scheme::Data => {
            let payload =
                crate::data_url::process(job.url.as_str(), job.policy.data_url_max_bytes)?;
            check_live(job)?;
            face_bytes(Arc::from(payload.bytes))
        }
        Scheme::UiRes => {
            let hit = ctx.bundles.lookup(&job.url)?;
            let max = job.policy.max_encoded_bytes.for_kind(ResourceKind::Font);
            if hit.data.len() as u64 > max {
                return Err(status(
                    Code::SizeLimit,
                    format!(
                        "bundle entry is {} bytes, over the {max} byte limit",
                        hit.data.len()
                    ),
                ));
            }
            face_bytes(hit.data)
        }
        Scheme::Other => Err(status(Code::ResourceDenied, "scheme not permitted")),
    }
}

/// Run one job to a completion; a decoder panic is contained and mapped to `DECODE_FAILED`.
pub fn run_job(job: &Job, ctx: &JobContext) -> Completion {
    if job.kind == ResourceKind::Font {
        let outcome = match catch_unwind(AssertUnwindSafe(|| run_font_pipeline(job, ctx))) {
            Ok(Ok(bytes)) => JobOutcome::Font(bytes),
            Ok(Err(s)) => JobOutcome::Failed(s),
            Err(_) => JobOutcome::Failed(status(Code::DecodeFailed, "font validation panicked")),
        };
        return Completion {
            request: job.request,
            revision: job.revision,
            outcome,
            finished: Instant::now(),
            deadline: job.deadline,
        };
    }
    let outcome = match catch_unwind(AssertUnwindSafe(|| run_pipeline(job, ctx))) {
        Ok(Ok((image, cache_hit))) => JobOutcome::Loaded { image, cache_hit },
        Ok(Err(s)) => JobOutcome::Failed(s),
        Err(_) => JobOutcome::Failed(status(Code::DecodeFailed, "decoder panicked")),
    };
    Completion {
        request: job.request,
        revision: job.revision,
        outcome,
        finished: Instant::now(),
        deadline: job.deadline,
    }
}

/// Admission spans queued, executing and completed-but-undelivered work.
pub const MAX_ADMITTED_JOBS: usize = 128;
pub const MAX_QUEUED_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_COMPLETION_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_IN_FLIGHT_ENCODED_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Default)]
struct Usage {
    jobs: usize,
    source: u64,
    results: u64,
    encoded: u64,
}

struct Admission {
    supervisor: Arc<Supervisor>,
    source: u64,
    results: u64,
}
impl Admission {
    fn charge_result(&mut self, bytes: u64) -> bool {
        let mut usage = self
            .supervisor
            .usage
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if usage.results.saturating_add(bytes) > MAX_COMPLETION_BYTES {
            return false;
        }
        usage.results += bytes;
        self.results = bytes;
        true
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        let mut usage = self
            .supervisor
            .usage
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        usage.jobs -= 1;
        usage.source -= self.source;
        usage.results -= self.results;
    }
}

struct QueuedCompletion {
    completion: Completion,
    _admission: Option<Admission>,
}
#[derive(Default)]
struct DeliveryState {
    closed: bool,
    queue: VecDeque<QueuedCompletion>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// A closable owner mailbox. Checking closure and publishing are one operation.
pub struct CompletionQueue {
    state: Mutex<DeliveryState>,
    cache: Mutex<Option<Weak<Mutex<Cache>>>>,
}
impl CompletionQueue {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(DeliveryState::default()),
            cache: Mutex::new(None),
        })
    }
    pub fn set_cache(&self, cache: &Arc<Mutex<Cache>>) {
        *self.cache.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::downgrade(cache));
    }
    pub fn set_wake(&self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !state.closed {
            state.wake = wake;
        }
    }
    fn discard(&self, completion: &Completion) {
        if let JobOutcome::Loaded { image, .. } = &completion.outcome {
            if let Some(cache) = self
                .cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .and_then(Weak::upgrade)
            {
                cache
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .unpin(&image.key);
            }
        }
    }
    fn publish(&self, completion: Completion, admission: Option<Admission>) -> bool {
        let wake = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if state.closed || state.queue.len() >= MAX_ADMITTED_JOBS {
                drop(state);
                self.discard(&completion);
                return false;
            }
            state.queue.push_back(QueuedCompletion {
                completion,
                _admission: admission,
            });
            state.wake.clone()
        };
        // Wakes are host proxies, never binding callbacks, and run outside locks.
        if let Some(wake) = wake {
            wake();
        }
        true
    }
    /// Deterministic mode has the same bounded queue and no asynchronous producer.
    pub fn post(&self, completion: Completion) -> bool {
        self.publish(completion, None)
    }
    pub fn has_capacity(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        !state.closed && state.queue.len() < MAX_ADMITTED_JOBS
    }
    pub fn take_all(&self) -> Vec<Completion> {
        let queued: Vec<_> = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .queue
            .drain(..)
            .collect();
        queued.into_iter().map(|q| q.completion).collect()
    }
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .queue
            .len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn close(&self) {
        let queued = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.closed = true;
            state.wake = None;
            std::mem::take(&mut state.queue)
        };
        for queued in queued {
            self.discard(&queued.completion);
        }
    }
}

struct PoolState {
    accepting: AtomicBool,
    active: Arc<AtomicUsize>,
}
struct Envelope {
    job: Job,
    context: Arc<JobContext>,
    destination: Weak<CompletionQueue>,
    owner: Arc<PoolState>,
    admission: Admission,
}
struct Supervisor {
    jobs: Mutex<VecDeque<Envelope>>,
    ready: Condvar,
    usage: Mutex<Usage>,
    alive: Arc<AtomicUsize>,
    threads: usize,
    handles: Mutex<Vec<JoinHandle<()>>>,
}
static SUPERVISOR: OnceLock<Arc<Supervisor>> = OnceLock::new();

impl Supervisor {
    fn get() -> Arc<Self> {
        SUPERVISOR
            .get_or_init(|| {
                let supervisor = Arc::new(Self {
                    jobs: Mutex::new(VecDeque::new()),
                    ready: Condvar::new(),
                    usage: Mutex::new(Usage::default()),
                    alive: Arc::new(AtomicUsize::new(0)),
                    threads: 2,
                    handles: Mutex::new(Vec::new()),
                });
                for n in 0..supervisor.threads {
                    let service = supervisor.clone();
                    supervisor.alive.fetch_add(1, Ordering::AcqRel);
                    match std::thread::Builder::new()
                        .name(format!("ui4me-assets-{n}"))
                        .spawn(move || service.worker())
                    {
                        Ok(handle) => supervisor
                            .handles
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .push(handle),
                        Err(_) => {
                            supervisor.alive.fetch_sub(1, Ordering::AcqRel);
                        }
                    }
                }
                supervisor
            })
            .clone()
    }
    fn reserve(self: &Arc<Self>, source: u64) -> Option<Admission> {
        let mut usage = self.usage.lock().unwrap_or_else(|p| p.into_inner());
        if self.alive.load(Ordering::Acquire) == 0
            || usage.jobs >= MAX_ADMITTED_JOBS
            || usage.source.saturating_add(source) > MAX_QUEUED_SOURCE_BYTES
        {
            return None;
        }
        usage.jobs += 1;
        usage.source += source;
        Some(Admission {
            supervisor: self.clone(),
            source,
            results: 0,
        })
    }
    fn worker(self: Arc<Self>) {
        struct Active(Arc<AtomicUsize>);
        impl Drop for Active {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let _alive = Active(self.alive.clone());
        loop {
            let mut envelope = {
                let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
                while jobs.is_empty() {
                    jobs = self.ready.wait(jobs).unwrap_or_else(|p| p.into_inner());
                }
                jobs.pop_front().expect("nonempty queue")
            };
            if envelope.job.cancel.is_canceled()
                || !envelope.owner.accepting.load(Ordering::Acquire)
            {
                continue;
            }
            envelope.owner.active.fetch_add(1, Ordering::AcqRel);
            let _active = Active(envelope.owner.active.clone());
            let encoded = envelope
                .job
                .policy
                .max_encoded_bytes
                .for_kind(envelope.job.kind);
            let reserved = {
                let mut usage = self.usage.lock().unwrap_or_else(|p| p.into_inner());
                if usage.encoded.saturating_add(encoded) > MAX_IN_FLIGHT_ENCODED_BYTES {
                    false
                } else {
                    usage.encoded += encoded;
                    true
                }
            };
            let mut completion = if reserved {
                let completion = run_job(&envelope.job, &envelope.context);
                self.usage.lock().unwrap_or_else(|p| p.into_inner()).encoded -= encoded;
                completion
            } else {
                failed_completion(&envelope.job, "process encoded-input budget is busy")
            };
            let bytes = match &completion.outcome {
                JobOutcome::Loaded { image, .. } => image.decoded_bytes(),
                JobOutcome::Font(bytes) => bytes.len() as u64,
                JobOutcome::Failed(_) => 0,
            };
            if !envelope.admission.charge_result(bytes) {
                if let JobOutcome::Loaded { image, .. } = &completion.outcome {
                    envelope
                        .context
                        .cache
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .unpin(&image.key);
                }
                completion.outcome = JobOutcome::Failed(status(
                    Code::BudgetExceeded,
                    "process completion-byte budget is full",
                ));
            }
            if let Some(destination) = envelope.destination.upgrade() {
                let _ = destination.publish(completion, Some(envelope.admission));
            } else if let JobOutcome::Loaded { image, .. } = &completion.outcome {
                envelope
                    .context
                    .cache
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .unpin(&image.key);
            }
        }
    }
    fn purge_cancelled(&self) {
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|e| e.owner.accepting.load(Ordering::Acquire) && !e.job.cancel.is_canceled());
    }
}

pub fn failed_completion(job: &Job, message: &str) -> Completion {
    Completion {
        request: job.request,
        revision: job.revision,
        outcome: JobOutcome::Failed(status(Code::BudgetExceeded, message)),
        finished: Instant::now(),
        deadline: job.deadline,
    }
}

/// A session interest in the one fixed process service; it owns no threads.
pub struct WorkerPool {
    supervisor: Arc<Supervisor>,
    context: Arc<JobContext>,
    destination: Weak<CompletionQueue>,
    state: Arc<PoolState>,
}
impl WorkerPool {
    pub fn start(
        _threads: usize,
        context: Arc<JobContext>,
        completions: Arc<CompletionQueue>,
    ) -> Self {
        completions.set_cache(&context.cache);
        Self {
            supervisor: Supervisor::get(),
            context,
            destination: Arc::downgrade(&completions),
            state: Arc::new(PoolState {
                accepting: AtomicBool::new(true),
                active: Arc::new(AtomicUsize::new(0)),
            }),
        }
    }
    pub fn submit(&self, job: Job) -> bool {
        self.supervisor.purge_cancelled();
        let Some(admission) = self.supervisor.reserve(job.url.as_str().len() as u64 + 256) else {
            return false;
        };
        let mut jobs = self
            .supervisor
            .jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !self.state.accepting.load(Ordering::Acquire) {
            return false;
        }
        jobs.push_back(Envelope {
            job,
            context: self.context.clone(),
            destination: self.destination.clone(),
            owner: self.state.clone(),
            admission,
        });
        drop(jobs);
        self.supervisor.ready.notify_one();
        true
    }
    pub fn purge_cancelled(&self) {
        self.supervisor.purge_cancelled();
    }
    pub fn stop_intake(&mut self) {
        self.state.accepting.store(false, Ordering::Release);
        self.supervisor.purge_cancelled();
    }
    pub fn configured_threads(&self) -> usize {
        self.supervisor.threads
    }
    pub fn alive_workers(&self) -> usize {
        self.supervisor.alive.load(Ordering::Acquire)
    }
    /// Active jobs for this interest; global worker threads intentionally remain.
    pub fn alive_handle(&self) -> Arc<AtomicUsize> {
        self.state.active.clone()
    }
}
impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.stop_intake();
    }
}

/// Convenience for tests and diagnostics: a deadline `ms` from now.
pub fn deadline_in(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}
