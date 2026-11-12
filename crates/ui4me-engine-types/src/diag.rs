//! owner: 001-vertical-slice — created by 001 slice (R-28)
//!
//! Plain diagnostics record types (core-types.md §6; semantics 005 `diagnostics-schema-v0.md`).
//! Producers write them; only `ui4me-diagnostics` reads and serialises them. Node references
//! are `NodeId`s here; the export maps them to 005's structural `NodeKey` (schema §4).

use crate::codes_generated::Code;
use crate::ids::{CallbackToken, DispatchId, DocumentId, NodeId};
use crate::stage::Stage;
use crate::status::Status;
use std::collections::{HashMap, VecDeque};

// ------------------------------------------------------------------ ids

/// Roots of 005's single feature-id registry (schema §7; R-37). Adding a root is a 005 change.
pub const FEATURE_ROOTS: &[&str] = &[
    "css.property",
    "css.at-rule",
    "css.selector",
    "css.pseudo",
    "css.value",
    "css.display",
    "css.media",
    "css.unit",
    "css.function",
    "css.syntax",
    "html",
    "img",
    "svg",
    "paint.capability",
    "aria.role",
    "aria.attr",
    "platform",
];

/// Stable dotted id for unsupported-feature reports, under one of [`FEATURE_ROOTS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeatureId(&'static str);

impl FeatureId {
    /// `None` unless the id is `<root>.<suffix>` for a registered root.
    pub fn new(id: &'static str) -> Option<Self> {
        let ok = FEATURE_ROOTS
            .iter()
            .any(|r| id.len() > r.len() + 1 && id.starts_with(r) && id.as_bytes()[r.len()] == b'.');
        ok.then_some(FeatureId(id))
    }
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

/// Static string id of a raster capability (010; AD-17, AD-18).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CapabilityId(pub &'static str);

// ------------------------------------------------------------------ bounded ring

/// Bounded ring buffer: pushing beyond capacity evicts the oldest entry and counts the drop.
#[derive(Clone, Debug)]
pub struct Ring<T> {
    items: VecDeque<T>,
    capacity: usize,
    dropped: u64,
}

impl<T> Ring<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "ring capacity must be positive");
        Ring {
            items: VecDeque::with_capacity(capacity),
            capacity,
            dropped: 0,
        }
    }
    /// A ring of the same bound whose storage grows with use. For the per-document stores (errors, updates, frame
    /// timings): each is bounded in the hundreds of entries (40 KiB and more) while most documents fill none of it, so
    /// reserving the bound up front costs every document load a few tens of KiB of allocator churn (A23).
    pub fn on_demand(capacity: usize) -> Self {
        assert!(capacity > 0, "ring capacity must be positive");
        Ring {
            items: VecDeque::new(),
            capacity,
            dropped: 0,
        }
    }
    pub fn push(&mut self, item: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
            self.dropped += 1;
        }
        self.items.push_back(item);
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ------------------------------------------------------------------ unsupported reports (R-20)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnsupportedReason {
    ProfileDisabled,
    NotConsumedByStage,
    GatedOff,
    CapabilityFallback,
    ResourceUnsupported,
}

impl UnsupportedReason {
    pub const fn name(self) -> &'static str {
        match self {
            UnsupportedReason::ProfileDisabled => "profile-disabled",
            UnsupportedReason::NotConsumedByStage => "not-consumed-by-stage",
            UnsupportedReason::GatedOff => "gated-off",
            UnsupportedReason::CapabilityFallback => "capability-fallback",
            UnsupportedReason::ResourceUnsupported => "resource-unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SheetOrigin {
    UserAgent,
    Author,
    Inline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReportLocation {
    Stylesheet {
        sheet: u32,
        origin: SheetOrigin,
        line: u32,
        column: u32,
    },
    Element {
        node: NodeId,
    },
}

/// The single report key `(feature, stage, document, location)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReportKey {
    pub feature: FeatureId,
    pub stage: Stage,
    pub document: DocumentId,
    pub location: ReportLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedReport {
    pub key: ReportKey,
    /// `UNSUPPORTED_FEATURE`, or `UNSUPPORTED_PAINT_FEATURE` (010).
    pub code: Code,
    pub reason: UnsupportedReason,
    /// Number of DOM revisions in which the key was emitted.
    pub revisions: u64,
    pub first_seen_dom: u64,
    pub last_seen_dom: u64,
}

/// De-duplicating store: one entry per key (O(1) lookup); repeats within one DOM revision count
/// once. Holds [`UnsupportedReports::CAPACITY`] distinct entries, then counts overflow (FR-068).
#[derive(Clone, Debug, Default)]
pub struct UnsupportedReports {
    entries: Vec<UnsupportedReport>,
    index: HashMap<ReportKey, usize>,
    overflow: u64,
}

impl UnsupportedReports {
    /// Distinct entries kept before `overflow_count` grows (FR-068; recompute-plan §10).
    pub const CAPACITY: usize = 4096;

    pub fn record(
        &mut self,
        key: ReportKey,
        code: Code,
        reason: UnsupportedReason,
        dom_revision: u64,
    ) {
        match self.index.get(&key) {
            Some(&i) => {
                let e = &mut self.entries[i];
                if dom_revision != e.last_seen_dom {
                    e.revisions += 1;
                    e.last_seen_dom = dom_revision;
                }
            }
            None if self.entries.len() >= Self::CAPACITY => self.overflow += 1,
            None => {
                self.index.insert(key, self.entries.len());
                self.entries.push(UnsupportedReport {
                    key,
                    code,
                    reason,
                    revisions: 1,
                    first_seen_dom: dom_revision,
                    last_seen_dom: dom_revision,
                });
            }
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &UnsupportedReport> {
        self.entries.iter()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Distinct keys that arrived after the store was full (`unsupported.overflow_count`).
    pub fn overflow_count(&self) -> u64 {
        self.overflow
    }
}

// ------------------------------------------------------------------ event trace (schema §6)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DispatchOrigin {
    Native,
    Injected,
    Click,
    Focus,
    DispatchEvent,
    Activation,
    Accessibility,
    ElementTask,
}

impl DispatchOrigin {
    pub const fn name(self) -> &'static str {
        match self {
            DispatchOrigin::Native => "native",
            DispatchOrigin::Injected => "injected",
            DispatchOrigin::Click => "click()",
            DispatchOrigin::Focus => "focus()",
            DispatchOrigin::DispatchEvent => "dispatch_event",
            DispatchOrigin::Activation => "activation",
            DispatchOrigin::Accessibility => "accessibility",
            DispatchOrigin::ElementTask => "element-task",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventPhase {
    Capturing,
    AtTarget,
    Bubbling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenerRecord {
    pub ordinal: u32,
    /// Host-supplied listener label (R-25); 002 maps it to `listener_id`.
    pub label: Option<String>,
    pub capture: bool,
    pub once: bool,
    pub passive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvocationOutcome {
    Returned,
    /// The callback failed; reported and dispatch continued (AD-27).
    Failed,
}

/// A call a listener made on the event, in call order (005 schema §6 `invocations[].calls`).
/// Added by 001 T052 (additive).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListenerCall {
    PreventDefault,
    /// `preventDefault()` inside a passive listener (ignored).
    PreventDefaultIgnoredPassive,
    /// `preventDefault()` on a non-cancelable event (ignored).
    PreventDefaultIgnoredNoncancelable,
    StopPropagation,
    StopImmediatePropagation,
}

impl ListenerCall {
    pub const fn name(self) -> &'static str {
        match self {
            ListenerCall::PreventDefault => "preventDefault",
            ListenerCall::PreventDefaultIgnoredPassive => "preventDefault-ignored-passive",
            ListenerCall::PreventDefaultIgnoredNoncancelable => {
                "preventDefault-ignored-noncancelable"
            }
            ListenerCall::StopPropagation => "stopPropagation",
            ListenerCall::StopImmediatePropagation => "stopImmediatePropagation",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvocationRecord {
    pub seq: u32,
    pub phase: EventPhase,
    pub current_target: NodeId,
    pub listener: ListenerRecord,
    pub outcome: InvocationOutcome,
    pub diagnostic_id: Option<u64>,
    pub default_prevented_after: bool,
    pub propagation_stopped_after: bool,
    pub immediate_stopped_after: bool,
    /// Calls the listener made on the event, in order (001 T052, additive).
    pub calls: Vec<ListenerCall>,
    /// Dispatches started from inside this invocation.
    pub nested: Vec<DispatchId>,
}

/// Legacy-pre-activation behaviour (AD-27). `saved` is the saved checkedness, exported as
/// `pre_activation.saved.checked` (005 schema §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreActivation {
    pub performed: bool,
    pub saved: bool,
}

/// Observable event payload (key/code, data, button, client coordinates).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventData {
    pub key: Option<String>,
    pub code: Option<String>,
    pub data: Option<String>,
    pub button: Option<i16>,
    pub client_x: Option<f64>,
    pub client_y: Option<f64>,
    /// `wheel` events: `deltaX`/`deltaY` in CSS px (`deltaMode` 0). Not exported when `None`.
    pub delta_x: Option<f64>,
    pub delta_y: Option<f64>,
    /// Keyboard events: `KeyboardEvent.repeat` (auto-repeat). `None` = not set by the producer (reads as false).
    pub repeat: Option<bool>,
    /// `beforeinput`/`input`: the `inputType` (Input Events level 2), when the producer sets it.
    /// Not exported when `None`.
    pub input_type: Option<String>,
    /// `InputEvent.isComposing` / `KeyboardEvent.isComposing`; set (to true) only while an IME
    /// composition is active, so traces of events outside a composition are unchanged.
    pub is_composing: Option<bool>,
    /// `TransitionEvent.propertyName`; `AnimationEvent.animationName`; `elapsedTime` in seconds
    /// and `pseudoElement` (`""` or `"::before"`/`"::after"`) for both. Not exported when `None`.
    pub property_name: Option<String>,
    pub animation_name: Option<String>,
    pub elapsed_time: Option<f64>,
    pub pseudo_element: Option<String>,
    /// `ToggleEvent.oldState` / `newState` (`"open"` or `"closed"`; 015 HTML-1, HTML-2). Not exported when `None`.
    pub old_state: Option<String>,
    pub new_state: Option<String>,
    /// `SubmitEvent.submitter`: the `id` of the submit button that caused the submission, when it has one.
    pub submitter: Option<String>,
    /// `PointerEvent` `pointerId`, `pointerType` (`"mouse"`, `"pen"`, `"touch"`), `isPrimary`
    /// and `pressure` (0.8 F0807). Not exported when `None`.
    pub pointer_id: Option<u32>,
    pub pointer_type: Option<String>,
    pub is_primary: Option<bool>,
    pub pressure: Option<f32>,
}

/// Members the export marks `nondeterministic` (excluded from byte-identity).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nondeterministic {
    pub time_ms: Option<f64>,
    pub host_language: Option<String>,
    pub callback_token: Option<CallbackToken>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DispatchRecord {
    pub document: DocumentId,
    pub dispatch_id: DispatchId,
    pub parent_dispatch_id: Option<DispatchId>,
    /// Outermost dispatch = 1 (012's frame stack; limit 32, R-35).
    pub depth: u32,
    pub event_type: String,
    pub bubbles: bool,
    pub cancelable: bool,
    pub is_trusted: bool,
    pub origin: DispatchOrigin,
    pub target: NodeId,
    /// Ends at the Document (R-29).
    pub path: Vec<NodeId>,
    pub activation_target: Option<NodeId>,
    pub pre_activation: Option<PreActivation>,
    pub invocations: Vec<InvocationRecord>,
    pub canceled: bool,
    pub event_data: Option<EventData>,
    /// `OK` or a refused-dispatch code such as `DISPATCH_RECURSION_LIMIT`.
    pub status: Code,
    pub nondeterministic: Nondeterministic,
}

// ------------------------------------------------------------------ errors, receipts, invalidation

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorRecord {
    pub document: Option<DocumentId>,
    pub status: Status,
}

/// Summary of a 010 render receipt (`frame_seq`, `scene.revision.dom`, `status`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReceiptRecord {
    pub document: DocumentId,
    pub frame_seq: u64,
    pub scene_dom_revision: u64,
    pub status: Code,
    pub input_to_present_ms: Option<f64>,
}

/// One invalidation/recompute decision (005 `invalidation-reasons.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidationRecord {
    pub document: DocumentId,
    /// 005 reason code (string id from `invalidation-reasons.md`).
    pub reason: &'static str,
    pub dom_revision: u64,
    pub stages: Vec<Stage>,
}

// ------------------------------------------------------------------ bounded stores (005 T012)

/// Dispatch records kept in the event log (FR-064).
pub const EVENT_LOG_CAPACITY: usize = 1024;
/// Records kept in the error ring (recompute-plan §10).
pub const ERROR_RING_CAPACITY: usize = 256;
/// Render receipts kept per target (FR-046).
pub const RECEIPTS_PER_TARGET: usize = 32;
/// Updates kept in the invalidation history (recompute-plan §10).
pub const INVALIDATION_HISTORY: usize = 16;
/// Resource request records kept per document (FR-075; IF-05).
pub const REQUEST_LOG_CAPACITY: usize = 1024;

/// A receipt ring key (`window:<ordinal>` or `headless`; 005 renumbers window ordinals).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReceiptTargetKey {
    Window(u32),
    Headless,
}

/// One bounded ring per target (receipts: [`RECEIPTS_PER_TARGET`] each).
#[derive(Clone, Debug)]
pub struct TargetRings<T> {
    capacity: usize,
    rings: HashMap<ReceiptTargetKey, Ring<T>>,
}

impl<T> TargetRings<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        TargetRings {
            capacity,
            rings: HashMap::new(),
        }
    }
    pub fn push(&mut self, target: ReceiptTargetKey, item: T) {
        let capacity = self.capacity;
        self.rings
            .entry(target)
            .or_insert_with(|| Ring::with_capacity(capacity))
            .push(item);
    }
    pub fn iter(&self, target: ReceiptTargetKey) -> impl Iterator<Item = &T> {
        self.rings.get(&target).into_iter().flat_map(|r| r.iter())
    }
    pub fn dropped(&self, target: ReceiptTargetKey) -> u64 {
        self.rings.get(&target).map_or(0, Ring::dropped)
    }
    /// Targets with at least one record, sorted.
    pub fn targets(&self) -> Vec<ReceiptTargetKey> {
        let mut t: Vec<_> = self.rings.keys().copied().collect();
        t.sort();
        t
    }
}

/// Identifies one request in a [`RequestLog`] (from 0, in push order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId(pub u64);

/// One resource request record; the content is 007's, the bound is 005's (FR-075).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestRecord {
    pub id: RequestId,
    pub label: String,
    /// `None` while pending.
    pub outcome: Option<String>,
}

/// Request log: a ring that evicts the oldest *completed* request first and never a pending
/// one (so it may exceed its bound while requests are pending), with a `dropped_count`.
#[derive(Clone, Debug)]
pub struct RequestLog {
    capacity: usize,
    records: VecDeque<RequestRecord>,
    next: u64,
    dropped: u64,
}

impl RequestLog {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "log capacity must be positive");
        RequestLog {
            capacity,
            records: VecDeque::new(),
            next: 0,
            dropped: 0,
        }
    }
    /// Append a pending request; first evicts the oldest completed one if the log is full.
    pub fn push_pending(&mut self, label: impl Into<String>) -> RequestId {
        if self.records.len() >= self.capacity
            && let Some(i) = self.records.iter().position(|r| r.outcome.is_some())
        {
            self.records.remove(i);
            self.dropped += 1;
        }
        let id = RequestId(self.next);
        self.next += 1;
        self.records.push_back(RequestRecord {
            id,
            label: label.into(),
            outcome: None,
        });
        id
    }
    /// Mark a request completed (no-op if it was evicted or is unknown).
    pub fn complete(&mut self, id: RequestId, outcome: impl Into<String>) {
        if let Some(r) = self.records.iter_mut().find(|r| r.id == id) {
            r.outcome = Some(outcome.into());
        }
    }
    pub fn get(&self, id: RequestId) -> Option<&RequestRecord> {
        self.records.iter().find(|r| r.id == id)
    }
    pub fn iter(&self) -> impl Iterator<Item = &RequestRecord> {
        self.records.iter()
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }
}

// ------------------------------------------------------------------ performance store (005 T039)

/// Frames kept for the `performance` probe (FR-077).
pub const PERFORMANCE_FRAMES: usize = 256;

/// Per-stage durations of one frame, milliseconds; `None` = the stage did not run for it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StageMs {
    pub style: Option<f64>,
    pub layout: Option<f64>,
    pub scene: Option<f64>,
    pub raster: Option<f64>,
    pub present: Option<f64>,
}

/// One frame's timings (IF-32, IF-34).
#[derive(Clone, Debug, PartialEq)]
pub struct FrameTiming {
    pub frame_seq: u64,
    /// `window:<ordinal>` or `headless`.
    pub target: Option<String>,
    pub stage_ms: StageMs,
    /// From the earliest input the frame reflects (stamped at COMMIT) to 010's present timestamp.
    pub input_to_present_ms: Option<f64>,
    /// From the earliest mutation the frame reflects to the present timestamp.
    pub mutation_to_present_ms: Option<f64>,
}

/// One component's memory counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryComponent {
    pub name: String,
    pub current_bytes: u64,
    pub peak_bytes: u64,
}

/// Everything the `performance` probe exports; all of it is nondeterministic (FR-071).
#[derive(Clone, Debug)]
pub struct PerformanceStore {
    pub frames: Ring<FrameTiming>,
    components: Vec<MemoryComponent>,
    pub event_loop_wakes: u64,
    pub idle_turns: u64,
}

impl Default for PerformanceStore {
    fn default() -> Self {
        PerformanceStore {
            frames: Ring::on_demand(PERFORMANCE_FRAMES),
            components: Vec::new(),
            event_loop_wakes: 0,
            idle_turns: 0,
        }
    }
}

impl PerformanceStore {
    pub fn push_frame(&mut self, frame: FrameTiming) {
        self.frames.push(frame);
    }

    /// A component reports its current allocation; the peak is the maximum reported so far.
    /// Only components that report are listed.
    pub fn push_memory(&mut self, name: &str, current_bytes: u64) {
        match self.components.iter_mut().find(|c| c.name == name) {
            Some(c) => {
                c.current_bytes = current_bytes;
                c.peak_bytes = c.peak_bytes.max(current_bytes);
            }
            None => self.components.push(MemoryComponent {
                name: name.to_string(),
                current_bytes,
                peak_bytes: current_bytes,
            }),
        }
    }

    pub fn components(&self) -> &[MemoryComponent] {
        &self.components
    }
    pub fn total_current_bytes(&self) -> u64 {
        self.components.iter().map(|c| c.current_bytes).sum()
    }
    pub fn total_peak_bytes(&self) -> u64 {
        self.components.iter().map(|c| c.peak_bytes).sum()
    }
}

/// All bounded diagnostic stores of one runtime or document (plain data, no trait). Every
/// `push_*` is O(1); the request log's eviction scan is bounded by its capacity.
#[derive(Clone, Debug)]
pub struct DiagStores {
    pub events: Ring<DispatchRecord>,
    pub unsupported: UnsupportedReports,
    pub errors: Ring<ErrorRecord>,
    pub receipts: TargetRings<ReceiptRecord>,
    pub invalidations: Ring<InvalidationRecord>,
    pub requests: RequestLog,
    pub performance: PerformanceStore,
}

impl Default for DiagStores {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagStores {
    pub fn new() -> Self {
        DiagStores {
            events: Ring::with_capacity(EVENT_LOG_CAPACITY),
            unsupported: UnsupportedReports::default(),
            errors: Ring::with_capacity(ERROR_RING_CAPACITY),
            receipts: TargetRings::with_capacity(RECEIPTS_PER_TARGET),
            invalidations: Ring::with_capacity(INVALIDATION_HISTORY),
            requests: RequestLog::with_capacity(REQUEST_LOG_CAPACITY),
            performance: PerformanceStore::default(),
        }
    }
    pub fn push_event(&mut self, record: DispatchRecord) {
        self.events.push(record);
    }
    pub fn push_unsupported(
        &mut self,
        key: ReportKey,
        code: Code,
        reason: UnsupportedReason,
        dom_revision: u64,
    ) {
        self.unsupported.record(key, code, reason, dom_revision);
    }
    pub fn push_error(&mut self, record: ErrorRecord) {
        self.errors.push(record);
    }
    pub fn push_receipt(&mut self, target: ReceiptTargetKey, record: ReceiptRecord) {
        self.receipts.push(target, record);
    }
    pub fn push_invalidation(&mut self, record: InvalidationRecord) {
        self.invalidations.push(record);
    }
    /// Append a pending request record; complete it through [`RequestLog::complete`].
    pub fn push_request(&mut self, label: impl Into<String>) -> RequestId {
        self.requests.push_pending(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Slots;

    #[test]
    fn feature_ids_need_a_registered_root() {
        assert!(FeatureId::new("css.display.flex").is_some());
        assert!(FeatureId::new("paint.capability.filter-color-matrix").is_some());
        assert!(
            FeatureId::new("css.display").is_none(),
            "root alone is not an id"
        );
        assert!(FeatureId::new("css.displayx.flex").is_none());
        assert!(FeatureId::new("bogus.thing").is_none());
    }

    #[test]
    fn ring_is_bounded_and_counts_drops() {
        let mut r = Ring::with_capacity(2);
        for i in 0..5 {
            r.push(i);
        }
        assert_eq!(r.iter().copied().collect::<Vec<_>>(), vec![3, 4]);
        assert_eq!(r.dropped(), 3);
    }

    #[test]
    fn reports_deduplicate_per_dom_revision() {
        let doc = DocumentId::from_raw(Slots::new().alloc());
        let key = ReportKey {
            feature: FeatureId::new("css.display.flex").unwrap(),
            stage: Stage::Box,
            document: doc,
            location: ReportLocation::Stylesheet {
                sheet: 0,
                origin: SheetOrigin::Author,
                line: 1,
                column: 1,
            },
        };
        let mut r = UnsupportedReports::default();
        r.record(
            key,
            Code::UnsupportedFeature,
            UnsupportedReason::NotConsumedByStage,
            1,
        );
        r.record(
            key,
            Code::UnsupportedFeature,
            UnsupportedReason::NotConsumedByStage,
            1,
        );
        r.record(
            key,
            Code::UnsupportedFeature,
            UnsupportedReason::NotConsumedByStage,
            3,
        );
        assert_eq!(r.len(), 1);
        let e = r.iter().next().unwrap();
        assert_eq!((e.revisions, e.first_seen_dom, e.last_seen_dom), (2, 1, 3));
    }
}
