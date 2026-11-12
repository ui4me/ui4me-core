//! owner: 007-resources (T013, T021, T022, T025–T027, T072) — requests, revisions, the state
//! machine, batches and settled tracking. Owner-thread only; no locking here.
//!
//! State machine (data-model): `Resolving -> Pending -> Ready | Broken(code) | Canceled`, and
//! `Discarded` for stale completions. A resolution-time denial is `Resolving -> Broken` at once
//! (terminal; `<img>` queues `error`). A cache hit still passes through Pending and is applied at
//! the next drain point (Clarification Q4).

use crate::cache::{Cache, CachedImage, ResourceKey};
use crate::diagnostics::{
    DocumentCounters, RequesterKind, ResourceDiagnosticRecord, StateName, redact_url,
};
use crate::uri::ResourceKind;
use crate::view::{AppliedBatch, ImageElementData, ImageState, ImageStateTag};
use crate::worker::{CancelToken, Completion, JobOutcome};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::resource_ids::{RequestId, ResourceEpoch, ResourceRef};
use url::Url;

/// Who asked (data-model `requester`). Identity of a requester is stable across revisions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Requester {
    ImgElement(NodeId),
    /// De-duplicated per (node, property, layer, url): the url is part of the identity.
    CssImage {
        node: NodeId,
        property: String,
        layer: u32,
        url: String,
    },
    FontFace(u64),
    Stylesheet(u64),
    SvgNested(RequestId),
}

impl Requester {
    pub fn tag(&self) -> RequesterKind {
        match self {
            Requester::ImgElement(_) => RequesterKind::ImgElement,
            Requester::CssImage { .. } => RequesterKind::CssImage,
            Requester::FontFace(_) => RequesterKind::FontFace,
            Requester::Stylesheet(_) => RequesterKind::Stylesheet,
            Requester::SvgNested(_) => RequesterKind::SvgNested,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestState {
    Resolving,
    Pending,
    Ready,
    Broken(Code),
    Canceled,
    Discarded,
}

impl RequestState {
    fn name(self) -> StateName {
        match self {
            RequestState::Resolving => StateName::Resolving,
            RequestState::Pending => StateName::Pending,
            RequestState::Ready => StateName::Ready,
            RequestState::Broken(_) => StateName::Broken,
            RequestState::Canceled => StateName::Canceled,
            RequestState::Discarded => StateName::Discarded,
        }
    }
}

/// Requesters that load nothing in v1 (T022): reported once per element or rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExcludedRequester {
    ScriptSrc,
    Iframe,
    Object,
    Embed,
    Video,
    Audio,
    Source,
    Track,
    InputTypeImage,
    LinkRelOther,
    CursorUrl,
    SrcsetWidthDescriptors,
    PictureSourceSelection,
}

impl ExcludedRequester {
    /// The R-20 report key under the `html` root.
    pub fn report_key(self) -> &'static str {
        match self {
            ExcludedRequester::ScriptSrc => "html.script-src",
            ExcludedRequester::Iframe => "html.iframe",
            ExcludedRequester::Object => "html.object",
            ExcludedRequester::Embed => "html.embed",
            ExcludedRequester::Video => "html.video",
            ExcludedRequester::Audio => "html.audio",
            ExcludedRequester::Source => "html.source",
            ExcludedRequester::Track => "html.track",
            ExcludedRequester::InputTypeImage => "html.input-image",
            ExcludedRequester::LinkRelOther => "html.link-rel",
            ExcludedRequester::CursorUrl => "html.cursor-url",
            ExcludedRequester::SrcsetWidthDescriptors => "html.srcset-width",
            ExcludedRequester::PictureSourceSelection => "html.picture",
        }
    }
}

/// One `srcset` candidate with a density descriptor (`1x` when none is written).
#[derive(Clone, Debug, PartialEq)]
pub struct SrcsetCandidate {
    pub url: String,
    pub density: f32,
}

/// Result of parsing a `srcset` attribute.
#[derive(Clone, Debug, PartialEq)]
pub enum SrcsetParse {
    /// Density candidates only.
    Densities(Vec<SrcsetCandidate>),
    /// A candidate uses a width descriptor (`w`): needs `sizes`, which is not supported (T022).
    WidthDescriptors,
}

/// Parse `srcset` (HTML "parse a srcset attribute", simplified to the descriptors v1 supports).
pub fn parse_srcset(srcset: &str) -> SrcsetParse {
    let mut out = Vec::new();
    let b = srcset.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut url = &srcset[start..i];
        let trailing_commas = url.len() - url.trim_end_matches(',').len();
        let mut descriptors = String::new();
        if trailing_commas > 0 {
            url = url.trim_end_matches(',');
        } else {
            let dstart = i;
            let mut depth = 0;
            while i < b.len() && !(b[i] == b',' && depth == 0) {
                match b[i] {
                    b'(' => depth += 1,
                    b')' if depth > 0 => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            descriptors = srcset[dstart..i].to_string();
        }
        if url.is_empty() {
            continue;
        }
        let mut density = 1.0f32;
        for d in descriptors.split_whitespace() {
            if let Some(n) = d.strip_suffix('w') {
                if n.parse::<f32>().is_ok() {
                    return SrcsetParse::WidthDescriptors;
                }
            } else if let Some(n) = d.strip_suffix('x') {
                match n.parse::<f32>() {
                    Ok(v) if v > 0.0 && v.is_finite() => density = v,
                    _ => {
                        density = 0.0; // invalid descriptor: the candidate is dropped
                    }
                }
            } else {
                density = 0.0;
            }
        }
        if density > 0.0 {
            out.push(SrcsetCandidate {
                url: url.to_string(),
                density,
            });
        }
    }
    SrcsetParse::Densities(out)
}

/// Select the candidate for `dpr`: the smallest density that is at least the DPR, else the
/// largest available. A `src` attribute counts as a `1x` candidate (HTML), unless a `1x`
/// candidate is already listed.
pub fn select_candidate(
    mut candidates: Vec<SrcsetCandidate>,
    src: Option<&str>,
    dpr: f32,
) -> Option<SrcsetCandidate> {
    if let Some(s) = src.filter(|s| !s.trim().is_empty()) {
        if !candidates.iter().any(|c| c.density == 1.0) {
            candidates.push(SrcsetCandidate {
                url: s.to_string(),
                density: 1.0,
            });
        }
    }
    candidates.sort_by(|a, b| a.density.total_cmp(&b.density));
    candidates
        .iter()
        .find(|c| c.density >= dpr)
        .or(candidates.last())
        .cloned()
}

/// Event queued for an `<img>` element, dispatched by 012 as idle work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageEventKind {
    Load,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageEvent {
    pub node: NodeId,
    pub kind: ImageEventKind,
    pub request: RequestId,
    pub revision: u32,
}

#[derive(Debug)]
pub struct Request {
    pub id: RequestId,
    pub requester: Requester,
    pub kind: ResourceKind,
    pub revision: u32,
    pub url: Option<Url>,
    pub density: f32,
    /// State of the latest revision.
    pub state: RequestState,
    /// The image currently displayed (the last Ready one) and its density.
    pub shown: Option<(Arc<CachedImage>, f32)>,
    pub broken: Option<Code>,
    pub cancel: Option<CancelToken>,
    pub deadline: Option<Instant>,
    pub held_key: Option<ResourceKey>,
    transitions: Vec<(StateName, u64)>,
    flags: Vec<&'static str>,
    outcome: Code,
}

impl Request {
    fn transition(&mut self, state: RequestState, t0: Instant) {
        self.state = state;
        self.transitions
            .push((state.name(), t0.elapsed().as_nanos() as u64));
    }

    fn natural_css(&self) -> Option<(f32, f32)> {
        self.shown
            .as_ref()
            .map(|(img, d)| (img.natural_px.0 as f32 / d, img.natural_px.1 as f32 / d))
    }

    pub fn image_state(&self) -> ImageState {
        match (&self.shown, self.broken) {
            (Some((img, density)), _) if img.vector.is_some() => {
                // SVG: nothing is invented; an absent dimension stays absent (FR-037).
                let n = img.vector.as_ref().expect("vector").natural;
                ImageState::Ready {
                    width_css: n.width.map(|w| w / density),
                    height_css: n.height.map(|h| h / density),
                    ratio: n.ratio,
                }
            }
            (Some((img, density)), _) => {
                let (w, h) = img.natural_px;
                ImageState::Ready {
                    width_css: Some(w as f32 / density),
                    height_css: Some(h as f32 / density),
                    ratio: (w > 0 && h > 0).then_some((w as f32, h as f32)),
                }
            }
            (None, Some(code)) => ImageState::Broken { code },
            (None, None) => ImageState::Pending,
        }
    }
}

/// A change to observable state recorded for the next batch.
#[derive(Clone, Copy, Debug)]
struct Change {
    reference: ResourceRef,
    intrinsic: bool,
}

#[derive(Debug)]
pub struct DocTable {
    pub document: DocumentId,
    pub live: bool,
    next_serial: u64,
    requests: BTreeMap<u64, Request>,
    by_requester: HashMap<Requester, u64>,
    pub epoch: ResourceEpoch,
    events: Vec<ImageEvent>,
    unreported: Vec<Change>,
    pub counters: DocumentCounters,
    t0: Instant,
}

/// What `begin` decided.
#[derive(Debug)]
pub enum Begin {
    /// Same requester, same URL and density, not broken: nothing to do.
    Unchanged(ResourceRef),
    /// A job must be scheduled for this reference.
    Schedule(ResourceRef),
    /// Terminal at resolution time (denial, malformed URL); the event, if any, is queued.
    Resolved(ResourceRef),
}

impl DocTable {
    pub fn new(document: DocumentId, t0: Instant) -> Self {
        DocTable {
            document,
            live: true,
            next_serial: 1,
            requests: BTreeMap::new(),
            by_requester: HashMap::new(),
            epoch: ResourceEpoch(0),
            events: Vec::new(),
            unreported: Vec::new(),
            counters: DocumentCounters::default(),
            t0,
        }
    }

    pub fn get(&self, r: &ResourceRef) -> Option<&Request> {
        if r.document != self.document {
            return None;
        }
        self.requests.get(&r.request.serial)
    }

    pub fn request_by_id(&self, id: RequestId) -> Option<&Request> {
        self.requests.get(&id.serial)
    }

    pub fn requests(&self) -> impl Iterator<Item = &Request> {
        self.requests.values()
    }

    pub fn can_begin(&self, requester: &Requester) -> bool {
        self.by_requester.contains_key(requester) || self.requests.len() < 4096
    }

    pub fn retain_requesters(
        &mut self,
        mut keep: impl FnMut(&Requester) -> bool,
        cache: &mut Cache,
    ) {
        let removed: std::collections::HashSet<u64> = self
            .requests
            .iter()
            .filter_map(|(serial, request)| (!keep(&request.requester)).then_some(*serial))
            .collect();
        for serial in &removed {
            if let Some(mut request) = self.requests.remove(serial) {
                if let Some(cancel) = request.cancel.take() {
                    cancel.cancel();
                }
                if let Some(key) = request.held_key.take() {
                    cache.release_ref(self.document, &key);
                }
            }
        }
        self.by_requester
            .retain(|_, serial| !removed.contains(serial));
        self.events
            .retain(|event| !removed.contains(&event.request.serial));
        self.unreported
            .retain(|change| !removed.contains(&change.reference.request.serial));
    }

    pub fn pending(&self) -> u32 {
        self.requests
            .values()
            .filter(|r| r.state == RequestState::Pending)
            .count() as u32
    }

    fn reference(&self, r: &Request) -> ResourceRef {
        ResourceRef {
            document: self.document,
            request: r.id,
            revision: r.revision,
        }
    }

    /// Register a request for `requester`, bumping its revision when the URL or density changed.
    /// `resolved` is the outcome of synchronous URL resolution and policy: `Err(code)` makes the
    /// request Broken at once. On `Schedule` the caller must submit a job with the returned cancel
    /// token (already stored) and deadline.
    pub fn begin(
        &mut self,
        requester: Requester,
        kind: ResourceKind,
        resolved: Result<Url, Code>,
        density: f32,
        cancel: CancelToken,
        deadline: Instant,
        cache: &mut Cache,
    ) -> Begin {
        self.begin_inner(
            requester, kind, resolved, density, cancel, deadline, cache, false,
        )
    }

    /// As `begin`, but a request for the same URL and density is started again (a new revision)
    /// instead of being `Unchanged`: the content behind the URL changed (a replaced bundle).
    pub fn begin_again(
        &mut self,
        requester: Requester,
        kind: ResourceKind,
        resolved: Result<Url, Code>,
        density: f32,
        cancel: CancelToken,
        deadline: Instant,
        cache: &mut Cache,
    ) -> Begin {
        self.begin_inner(
            requester, kind, resolved, density, cancel, deadline, cache, true,
        )
    }

    /// The image requests whose URL starts with `prefix`, for re-requesting after a bundle
    /// changed (fonts are not included).
    pub fn image_requests_under(&self, prefix: &str) -> Vec<(Requester, ResourceKind, Url, f32)> {
        self.requests
            .values()
            .filter(|r| r.kind != ResourceKind::Font)
            .filter_map(|r| {
                r.url
                    .as_ref()
                    .filter(|u| u.as_str().starts_with(prefix))
                    .map(|u| (r.requester.clone(), r.kind, u.clone(), r.density))
            })
            .collect()
    }

    fn begin_inner(
        &mut self,
        requester: Requester,
        kind: ResourceKind,
        resolved: Result<Url, Code>,
        density: f32,
        cancel: CancelToken,
        deadline: Instant,
        cache: &mut Cache,
        again: bool,
    ) -> Begin {
        let t0 = self.t0;
        let existing = self.by_requester.get(&requester).copied();
        if let (Some(serial), Ok(url)) = (existing, &resolved) {
            let r = &self.requests[&serial];
            if !again
                && r.url.as_ref() == Some(url)
                && r.density == density
                && !matches!(r.state, RequestState::Canceled)
            {
                // A Broken request for the same URL is terminal: the renderer asks again on every
                // frame, and restarting it each time re-ran the failing load forever (a missing,
                // denied or oversized file stayed "pending" and burned a worker per frame).
                // A changed URL or density, or `begin_again`, still starts a new load.
                return Begin::Unchanged(self.reference(r));
            }
        }
        let serial = match existing {
            Some(s) => s,
            None => {
                let s = self.next_serial;
                self.next_serial += 1;
                let id = RequestId {
                    document: self.document,
                    serial: s,
                };
                self.requests.insert(
                    s,
                    Request {
                        id,
                        requester: requester.clone(),
                        kind,
                        revision: 0,
                        url: None,
                        density,
                        state: RequestState::Resolving,
                        shown: None,
                        broken: None,
                        cancel: None,
                        deadline: None,
                        held_key: None,
                        transitions: Vec::new(),
                        flags: Vec::new(),
                        outcome: Code::Ok,
                    },
                );
                self.by_requester.insert(requester.clone(), s);
                s
            }
        };
        // Supersede an in-flight revision.
        let counters = &mut self.counters;
        let req = self.requests.get_mut(&serial).expect("request exists");
        if req.state == RequestState::Pending {
            if let Some(c) = req.cancel.take() {
                c.cancel();
            }
            counters.canceled += 1;
        }
        req.revision += 1;
        req.density = density;
        req.kind = kind;
        req.url = resolved.as_ref().ok().cloned();
        req.transitions.clear();
        req.transition(RequestState::Resolving, t0);
        let reference = ResourceRef {
            document: self.document,
            request: req.id,
            revision: req.revision,
        };
        match resolved {
            Err(code) => {
                // Broken at resolution time: terminal; the `error` event is queued now.
                let had_image = req.shown.is_some();
                if let Some(key) = req.held_key.take() {
                    cache.release_ref(self.document, &key);
                }
                req.shown = None;
                req.broken = Some(code);
                req.outcome = code;
                req.transition(RequestState::Broken(code), t0);
                if code == Code::ResourceDenied || code == Code::UriMalformed {
                    self.counters.denied += 1;
                }
                if let Requester::ImgElement(node) = req.requester {
                    self.events.push(ImageEvent {
                        node,
                        kind: ImageEventKind::Error,
                        request: req.id,
                        revision: req.revision,
                    });
                }
                self.unreported.push(Change {
                    reference,
                    intrinsic: had_image || true,
                });
                Begin::Resolved(reference)
            }
            Ok(_) => {
                req.cancel = Some(cancel);
                req.deadline = Some(deadline);
                req.transition(RequestState::Pending, t0);
                Begin::Schedule(reference)
            }
        }
    }

    /// Cancel every request of the document (`Closing`): tokens signalled, cache references
    /// released, late completions become `Discarded`.
    pub fn close(&mut self, cache: &mut Cache) {
        self.live = false;
        let t0 = self.t0;
        for req in self.requests.values_mut() {
            if let Some(c) = req.cancel.take() {
                c.cancel();
            }
            if req.state == RequestState::Pending || req.state == RequestState::Resolving {
                req.transition(RequestState::Canceled, t0);
                self.counters.canceled += 1;
            }
            if let Some(k) = req.held_key.take() {
                cache.release_ref(self.document, &k);
            }
        }
        self.events.clear();
        self.unreported.clear();
    }

    /// Cancel everything at runtime shutdown (tokens only; no cache work).
    pub fn cancel_all_tokens(&mut self) {
        for req in self.requests.values_mut() {
            if let Some(c) = &req.cancel {
                c.cancel();
            }
        }
    }

    /// Apply one completion. Returns `None` when it was stale (counted) or the document is closed.
    fn apply(
        &mut self,
        c: Completion,
        cache: &mut Cache,
        fonts: &mut Vec<FontEvent>,
    ) -> Option<(Change, Option<ResourceRecordInput>)> {
        let t0 = self.t0;
        let stale = !self.live
            || match self.requests.get(&c.request.serial) {
                None => true,
                Some(r) => r.revision != c.revision || r.state != RequestState::Pending,
            };
        if stale {
            self.counters.stale_discarded += 1;
            if let JobOutcome::Loaded { image, .. } = &c.outcome {
                cache.unpin(&image.key);
            }
            return None;
        }
        let document = self.document;
        let req = self
            .requests
            .get_mut(&c.request.serial)
            .expect("checked above");
        req.cancel = None;
        if let Requester::FontFace(face) = req.requester {
            let reference = ResourceRef {
                document,
                request: req.id,
                revision: req.revision,
            };
            let result = match c.outcome {
                JobOutcome::Font(bytes) if c.finished <= c.deadline => Ok(bytes),
                JobOutcome::Font(_) => Err(Code::BudgetExceeded),
                JobOutcome::Failed(s) => Err(s.code),
                JobOutcome::Loaded { .. } => Err(Code::InternalInvariant),
            };
            match &result {
                Ok(_) => {
                    req.outcome = Code::Ok;
                    req.transition(RequestState::Ready, t0);
                }
                Err(code) => {
                    req.outcome = *code;
                    req.broken = Some(*code);
                    req.transition(RequestState::Broken(*code), t0);
                }
            }
            fonts.push(FontEvent {
                reference,
                face,
                result,
            });
            return None;
        }
        let prev_natural = req.natural_css();
        let was_unresolved = req.shown.is_none();
        let late = c.finished > c.deadline;
        let reference = ResourceRef {
            document,
            request: req.id,
            revision: req.revision,
        };
        let outcome = match c.outcome {
            JobOutcome::Font(_) => unreachable!("font completions are handled above"),
            JobOutcome::Loaded { image, .. } if late => {
                cache.unpin(&image.key);
                JobOutcome::Failed(
                    ui4me_engine_types::status::Status::new(
                        Code::BudgetExceeded,
                        ui4me_engine_types::stage::Stage::Resource,
                    )
                    .with_message("result arrived after the decode deadline"),
                )
            }
            o => o,
        };
        match outcome {
            JobOutcome::Font(_) => unreachable!("font completions are handled above"),
            JobOutcome::Failed(status) => {
                if let Some(k) = req.held_key.take() {
                    cache.release_ref(document, &k);
                }
                req.shown = None;
                req.broken = Some(status.code);
                req.outcome = status.code;
                req.transition(RequestState::Broken(status.code), t0);
                if matches!(status.code, Code::ResourceDenied | Code::UriMalformed) {
                    self.counters.denied += 1;
                }
                if let Requester::ImgElement(node) = req.requester {
                    self.events.push(ImageEvent {
                        node,
                        kind: ImageEventKind::Error,
                        request: req.id,
                        revision: req.revision,
                    });
                }
                Some((
                    Change {
                        reference,
                        intrinsic: true,
                    },
                    None,
                ))
            }
            JobOutcome::Loaded { image, cache_hit } => {
                if let Some(k) = req.held_key.take() {
                    cache.release_ref(document, &k);
                }
                cache.add_ref(document, &image.key);
                cache.unpin(&image.key);
                req.held_key = Some(image.key.clone());
                req.broken = None;
                req.outcome = Code::Ok;
                req.flags.clear();
                if image.animated_first_frame {
                    req.flags.push("animated_first_frame");
                }
                if image.downscaled {
                    req.flags.push("downscaled");
                }
                if image.icc_ignored {
                    req.flags.push("icc_ignored");
                }
                if cache_hit {
                    req.flags.push("cache_hit");
                }
                let animated = image.animated_first_frame;
                let reports = image.reports.clone();
                req.shown = Some((image.clone(), req.density));
                req.transition(RequestState::Ready, t0);
                let new_natural = req.natural_css();
                if let Requester::ImgElement(node) = req.requester {
                    self.events.push(ImageEvent {
                        node,
                        kind: ImageEventKind::Load,
                        request: req.id,
                        revision: req.revision,
                    });
                }
                let intrinsic = was_unresolved || prev_natural != new_natural;
                Some((
                    Change {
                        reference,
                        intrinsic,
                    },
                    (animated || !reports.is_empty())
                        .then_some(ResourceRecordInput { animated, reports }),
                ))
            }
        }
    }

    /// Apply a drain step's completions for this document as one batch. `None` = nothing changed.
    pub fn apply_batch(
        &mut self,
        completions: Vec<Completion>,
        cache: &mut Cache,
        log: &mut Vec<(RequestId, &'static str)>,
        fonts: &mut Vec<FontEvent>,
    ) -> Option<AppliedBatch> {
        let mut changes: Vec<Change> = std::mem::take(&mut self.unreported);
        for c in completions {
            if let Some((ch, extra)) = self.apply(c, cache, fonts) {
                if let Some(extra) = extra {
                    if extra.animated {
                        log.push((ch.reference.request, "img.animation"));
                    }
                    for key in extra.reports {
                        log.push((ch.reference.request, key));
                    }
                }
                changes.push(ch);
            }
        }
        if changes.is_empty() {
            return None;
        }
        self.epoch = self.epoch.next();
        let mut batch = AppliedBatch {
            document: self.document,
            resource_epoch: self.epoch,
            changed: Vec::new(),
            intrinsic_size: Vec::new(),
            paint_only: Vec::new(),
            font_epoch_changed: false,
        };
        for ch in changes {
            batch.changed.push(ch.reference);
            if ch.intrinsic {
                batch.intrinsic_size.push(ch.reference)
            } else {
                batch.paint_only.push(ch.reference)
            }
        }
        self.refresh_counters();
        Some(batch)
    }

    fn refresh_counters(&mut self) {
        let (mut p, mut r, mut b) = (0, 0, 0);
        for q in self.requests.values() {
            match q.state {
                RequestState::Pending => p += 1,
                RequestState::Ready => r += 1,
                RequestState::Broken(_) => b += 1,
                _ => {}
            }
        }
        self.counters.pending = p;
        self.counters.ready = r;
        self.counters.broken = b;
    }

    pub fn counters(&mut self) -> DocumentCounters {
        self.refresh_counters();
        self.counters
    }

    pub fn take_events(&mut self) -> Vec<ImageEvent> {
        std::mem::take(&mut self.events)
    }

    /// Host data for an `<img>` element (T033).
    pub fn element_data(&self, r: &ResourceRef) -> Option<ImageElementData> {
        let req = self.get(r)?;
        let (state, failure) = match req.image_state() {
            ImageState::Pending => (ImageStateTag::Pending, None),
            ImageState::Ready { .. } => (ImageStateTag::Ready, None),
            ImageState::Broken { code } => (ImageStateTag::Broken, Some(code)),
        };
        let (natural_width, natural_height) = req
            .shown
            .as_ref()
            .map(|(i, d)| {
                (
                    (i.natural_px.0 as f32 / d).round() as u32,
                    (i.natural_px.1 as f32 / d).round() as u32,
                )
            })
            .unwrap_or((0, 0));
        Some(ImageElementData {
            complete: state != ImageStateTag::Pending && req.state != RequestState::Pending,
            natural_width,
            natural_height,
            state,
            failure,
        })
    }

    /// The diagnostic record of one request.
    pub fn record_of(&self, id: RequestId) -> Option<ResourceDiagnosticRecord> {
        self.records(false).into_iter().find(|r| r.request == id)
    }

    /// Diagnostic record of the current state of every request (terminal ones only when asked).
    pub fn records(&self, only_terminal: bool) -> Vec<ResourceDiagnosticRecord> {
        self.requests
            .values()
            .filter(|r| {
                !only_terminal
                    || !matches!(r.state, RequestState::Pending | RequestState::Resolving)
            })
            .map(|r| ResourceDiagnosticRecord {
                request: r.id,
                revision: r.revision,
                kind: r.kind,
                requester: r.requester.tag(),
                url: r
                    .url
                    .as_ref()
                    .map(redact_url)
                    .unwrap_or_else(|| "<unresolved>".into()),
                transitions: r.transitions.clone(),
                outcome: r.outcome,
                encoded_bytes: r.shown.as_ref().map(|(i, _)| i.encoded_bytes).unwrap_or(0),
                decoded_bytes: r
                    .shown
                    .as_ref()
                    .map(|(i, _)| i.decoded_bytes())
                    .unwrap_or(0),
                decode_ms: r.shown.as_ref().map(|(i, _)| i.decode_ms).unwrap_or(0),
                natural: r.shown.as_ref().map(|(i, _)| i.natural_px),
                flags: r.flags.clone(),
                warnings: {
                    let mut w: Vec<Code> = if r.flags.contains(&"downscaled") {
                        vec![Code::SizeLimit]
                    } else {
                        Vec::new()
                    };
                    if let Some((i, _)) = r.shown.as_ref() {
                        w.extend(i.warnings.iter().copied());
                    }
                    w
                },
            })
            .collect()
    }
}

/// A font completion for the manager to fold into the face state (`fonts/faces.rs`).
#[derive(Debug)]
pub struct FontEvent {
    pub reference: ResourceRef,
    pub face: u64,
    pub result: Result<Arc<[u8]>, Code>,
}

/// Side information of an applied completion (kept small on purpose).
#[derive(Debug)]
pub struct ResourceRecordInput {
    pub animated: bool,
    /// Further `UNSUPPORTED_FEATURE` keys of this resource (`svg.text`), reported once each.
    pub reports: Vec<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::ids::{RawId, Slots};

    fn ids() -> (DocumentId, NodeId) {
        let mut s = Slots::new();
        let a = s.alloc();
        (
            DocumentId::from_raw(a),
            NodeId::from_raw(RawId {
                index: 0,
                generation: a.generation,
            }),
        )
    }

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn revisions_bump_and_dedup() {
        let (doc, node) = ids();
        let mut t = DocTable::new(doc, Instant::now());
        let mut cache = Cache::new(1 << 20, 1 << 24);
        let dl = Instant::now() + std::time::Duration::from_secs(5);
        let r = Requester::ImgElement(node);
        let Begin::Schedule(r1) = t.begin(
            r.clone(),
            ResourceKind::Image,
            Ok(url("file:///a.png")),
            1.0,
            CancelToken::new(),
            dl,
            &mut cache,
        ) else {
            panic!()
        };
        assert_eq!(r1.revision, 1);
        assert!(
            matches!(t.begin(r.clone(), ResourceKind::Image, Ok(url("file:///a.png")), 1.0, CancelToken::new(), dl, &mut cache), Begin::Unchanged(x) if x == r1)
        );
        let Begin::Schedule(r2) = t.begin(
            r.clone(),
            ResourceKind::Image,
            Ok(url("file:///b.png")),
            1.0,
            CancelToken::new(),
            dl,
            &mut cache,
        ) else {
            panic!()
        };
        assert_eq!(
            (r2.revision, r2.request),
            (2, r1.request),
            "same requester keeps its request id"
        );
        assert_eq!(t.pending(), 1);
        // a density change selects another candidate: a new revision
        let Begin::Schedule(r3) = t.begin(
            r.clone(),
            ResourceKind::Image,
            Ok(url("file:///b.png")),
            2.0,
            CancelToken::new(),
            dl,
            &mut cache,
        ) else {
            panic!()
        };
        assert_eq!(r3.revision, 3);
        assert_eq!(t.counters.canceled, 2);
    }

    #[test]
    fn denial_is_broken_at_resolution_and_queues_error() {
        let (doc, node) = ids();
        let mut t = DocTable::new(doc, Instant::now());
        let mut cache = Cache::new(1 << 20, 1 << 24);
        let dl = Instant::now();
        let b = t.begin(
            Requester::ImgElement(node),
            ResourceKind::Image,
            Err(Code::ResourceDenied),
            1.0,
            CancelToken::new(),
            dl,
            &mut cache,
        );
        let Begin::Resolved(r) = b else { panic!() };
        assert_eq!(
            t.get(&r).unwrap().state,
            RequestState::Broken(Code::ResourceDenied)
        );
        assert_eq!(t.pending(), 0);
        let ev = t.take_events();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, ImageEventKind::Error);
        let batch = t
            .apply_batch(vec![], &mut cache, &mut Vec::new(), &mut Vec::new())
            .expect("sync denial is reported in the next batch");
        assert_eq!(batch.changed, vec![r]);
        assert_eq!(batch.resource_epoch, ResourceEpoch(1));
        assert!(
            t.apply_batch(vec![], &mut cache, &mut Vec::new(), &mut Vec::new())
                .is_none()
        );
    }

    #[test]
    fn css_requests_make_no_events() {
        let (doc, node) = ids();
        let mut t = DocTable::new(doc, Instant::now());
        let mut cache = Cache::new(1 << 20, 1 << 24);
        let q = Requester::CssImage {
            node,
            property: "background-image".into(),
            layer: 0,
            url: "x".into(),
        };
        t.begin(
            q,
            ResourceKind::Image,
            Err(Code::ResourceDenied),
            1.0,
            CancelToken::new(),
            Instant::now(),
            &mut cache,
        );
        assert!(t.take_events().is_empty());
    }
}
