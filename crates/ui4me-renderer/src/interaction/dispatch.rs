//! owner: 012-interaction — created by 001 slice (R-28)
//!
//! AD-27 dispatch core (001 T058; 012 `dispatch-state-machine.md` §1–§4): the BUILD…RETURN
//! machine, the failure policy, COMMIT, and the [`InteractionRuntime`] state (frame stack,
//! listener registries, focus, input queue, event log). See `API.md` for the host contract.
//!
//! **Borrow discipline.** Every step touches engine state only inside a short
//! [`DispatchHost::with_runtime`]/[`DispatchHost::with_engine`] closure; listeners are invoked
//! through [`DispatchHost::invoke`] after the closure has returned, so no engine borrow is held
//! across a host callback (plan constraint; AD-43). The frame keeps the cursor across
//! IN_CALLBACK, so the machine resumes from owned data only.

use crate::interaction::activation;
use crate::interaction::focus::FocusShared;
use crate::interaction::frame::{
    Cursor, DISPATCH_DEPTH_LIMIT, DispatchFrame, EventFlags, EventInit, EventInterface, EventState,
    FrameStack, Pass, SavedPreActivation,
};
use crate::interaction::input_queue::{InputItem, NativeInputQueue};
use crate::interaction::listeners::{AddOutcome, ListenerRegistry, ListenerSpec, Released};
use crate::interaction::sequences::PointerState;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use ui4me_document::document::{Document, Lifecycle};
use ui4me_document::ui_state::UiStateKey;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{
    DispatchOrigin, DispatchRecord, EventData, EventPhase, InvocationOutcome, InvocationRecord,
    ListenerCall, Nondeterministic, PreActivation, Ring,
};
use ui4me_engine_types::ids::{
    CallbackToken, DispatchId, DispatchIdSource, DocumentId, ListenerToken, NodeId,
};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// Event-log capacity (005 recompute-plan §10).
pub const EVENT_LOG_CAPACITY: usize = 1024;

/// Listener failure policy (012 §4; the flag is 004's runtime config).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FailurePolicy {
    /// Record `CALLBACK_FAILED`, continue, canceled flag unchanged (default).
    #[default]
    ReportAndContinue,
    /// As above, then behave as if the listener called `preventDefault()` and
    /// `stopImmediatePropagation()` (declared deviation).
    Strict,
}

/// One listener invocation handed to the host (004 `invoke(token, event_handle)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub document: DocumentId,
    /// The event handle: valid for getters and `preventDefault`/`stop*` while this callback
    /// (or a dispatch nested in it) runs; `STALE_HANDLE` afterwards.
    pub event: DispatchId,
    pub listener: ListenerToken,
    pub callback: CallbackToken,
    pub label: Option<String>,
    pub ordinal: u32,
    pub event_type: String,
    pub phase: EventPhase,
    pub current_target: NodeId,
    /// Depth of this dispatch (outermost = 1).
    pub depth: u32,
}

/// A failed host callback (exception, non-zero status).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackError {
    pub message: String,
}

impl CallbackError {
    pub fn new(message: impl Into<String>) -> Self {
        CallbackError {
            message: message.into(),
        }
    }
}

/// RETURN value of one dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub dispatch_id: DispatchId,
    pub canceled: bool,
}

impl DispatchOutcome {
    /// DOM `dispatchEvent` return value.
    pub fn not_canceled(&self) -> bool {
        !self.canceled
    }
}

/// What InteractionRuntime needs from the HostRuntime (004) and the pipeline (005). Implemented
/// by `ui4me-runtime`; see `API.md` §2 for each method's contract.
pub trait DispatchHost {
    /// Run `f` with exclusive access to the interaction runtime. Must always run `f`.
    fn with_runtime(&mut self, f: &mut dyn FnMut(&mut InteractionRuntime));
    /// Run `f` with the interaction runtime and document `doc`, in every lifecycle state
    /// (including `Closing`). An accepted DOM write inside `f` (revision changed) is a pipeline
    /// `INPUT` (`DocumentPipeline::note_input`). `Err` only if the document no longer exists.
    fn with_engine(
        &mut self,
        doc: DocumentId,
        f: &mut dyn FnMut(&mut InteractionRuntime, &mut Document),
    ) -> Result<(), Status>;
    /// Invoke a listener's host callback. Called with no engine borrow held.
    fn invoke(&mut self, invocation: &Invocation) -> Result<(), CallbackError>;
    /// Record an error in 005's error store; returns it with its diagnostic id.
    fn record_error(&mut self, doc: DocumentId, status: Status) -> Status;
    /// Topmost hit element at a viewport point (Flush mode; 010 HitTestIndex).
    fn hit_test(&mut self, doc: DocumentId, x: f64, y: f64) -> Result<Option<NodeId>, Status>;
    /// Hit a scrollbar track for native pointer interaction (not a DOM hit target).
    fn scrollbar_hit(
        &mut self,
        _doc: DocumentId,
        _x: f64,
        _y: f64,
    ) -> Result<Option<crate::interaction::scroll::ScrollbarHit>, Status> {
        Ok(None)
    }
    /// Transform a point to the track's local coordinate while dragging.
    fn scrollbar_local(
        &mut self,
        _doc: DocumentId,
        _node: NodeId,
        _vertical: bool,
        _x: f64,
        _y: f64,
    ) -> Result<Option<f64>, Status> {
        Ok(None)
    }
    /// The runtime clock (manual or virtual clock in deterministic runs).
    fn now_ms(&mut self) -> f64;
    /// COMMIT step 1: run deferred window/document/runtime closes (004).
    fn run_deferred_closes(&mut self);
    /// COMMIT step 2: 003 `outermost_commit(doc)` (completes `Closing → Closed`, collection
    /// checkpoint) for each document dispatched to since the last COMMIT.
    fn outermost_commit(&mut self, doc: DocumentId);
    /// COMMIT: release notifications for removed/`once`/freed registrations (004 un-roots).
    fn release_callbacks(&mut self, released: &[Released]);
    /// COMMIT last step: mark dirty and request one frame (005).
    fn request_frame(&mut self, doc: DocumentId);

    /// The computed `cursor` value of an element (a CSS value string: keywords, possibly after
    /// `url()` references). Default: `auto`.
    fn computed_cursor(&mut self, doc: DocumentId, node: NodeId) -> Result<String, Status> {
        let _ = (doc, node);
        Ok("auto".into())
    }
    /// Apply a cursor decision (a CSS keyword) to the document's windows (011). Called only when
    /// the decision changes. Default: nothing.
    fn set_cursor(&mut self, doc: DocumentId, keyword: &str) {
        let _ = (doc, keyword);
    }

    /// Whether layout gives this element a visible focus area. Pure DOM test
    /// hosts may accept all nodes; the application renderer checks its committed
    /// style and fragments, including hidden ancestors and visibility.
    fn focus_target_visible(&mut self, _doc: DocumentId, _node: NodeId) -> Result<bool, Status> {
        Ok(true)
    }

    /// The scroll containers a scroll starting at `from` may move, innermost first, ending with
    /// the viewport (`target: None`) when the document has a layout. Default: none.
    fn scroll_chain(
        &mut self,
        doc: DocumentId,
        from: NodeId,
    ) -> Result<Vec<crate::interaction::scroll::ScrollTargetInfo>, Status> {
        let _ = (doc, from);
        Ok(Vec::new())
    }
    /// Scroll state of one target (`None` = the viewport); `Ok(None)` for an element that is not
    /// a scroll container. Default: nothing scrolls.
    fn scroll_info(
        &mut self,
        doc: DocumentId,
        target: Option<NodeId>,
    ) -> Result<Option<crate::interaction::scroll::ScrollTargetInfo>, Status> {
        let _ = (doc, target);
        Ok(None)
    }
    /// The inner editing area of a text control from the committed layout (flushes first), in
    /// viewport space; `None` when the node has none or the host has no layout. Default: none.
    fn control_geometry(
        &mut self,
        doc: DocumentId,
        node: NodeId,
    ) -> Result<Option<crate::interaction::editing::EditGeometry>, Status> {
        let _ = (doc, node);
        Ok(None)
    }
    /// The caret of `node`: whether it is in the visible half of its blink and whether it belongs
    /// at the end of the previous line (affinity). A repaint of the scene only. Default: nothing.
    fn set_caret_state(&mut self, doc: DocumentId, node: NodeId, visible: bool, upstream: bool) {
        let _ = (doc, node, visible, upstream);
    }
    /// Start (`Some`) or stop (`None`) the caret blink of the focused text control. Default: nothing.
    fn caret_blink(&mut self, doc: DocumentId, node: Option<NodeId>) {
        let _ = (doc, node);
    }
    /// The clipboard content (`text/plain`, `text/html`), for `paste`. Default: `UNSUPPORTED_FEATURE`
    /// (a host without a clipboard; 012 `editing.md` section 3 records `UNSUPPORTED_DEFAULT_ACTION`).
    fn clipboard_read(&mut self) -> Result<ui4me_engine_types::clipboard::ClipboardData, Status> {
        Err(status(Code::UnsupportedFeature).with_message("this host has no clipboard"))
    }
    /// Replace the clipboard content, for `copy` and `cut`. Default: `UNSUPPORTED_FEATURE`.
    fn clipboard_write(
        &mut self,
        data: &ui4me_engine_types::clipboard::ClipboardData,
    ) -> Result<(), Status> {
        let _ = data;
        Err(status(Code::UnsupportedFeature).with_message("this host has no clipboard"))
    }
    /// Queue a native paste without waiting on the owner thread. A true result
    /// means that completion (including failure) is owned by this host.
    fn defer_clipboard_paste(&mut self, doc: DocumentId, node: NodeId) -> bool {
        let _ = (doc, node);
        false
    }
    /// Queue a native copy/cut. A deferred cut must not delete the selection
    /// until the write succeeds and the original editing context is current.
    fn defer_clipboard_write(
        &mut self,
        doc: DocumentId,
        node: Option<NodeId>,
        data: &ui4me_engine_types::clipboard::ClipboardData,
        cut: bool,
    ) -> bool {
        let _ = (doc, node, data, cut);
        false
    }
    /// The focused text control of `doc` wants IME input (`Some`: purpose, caret rectangle and the text
    /// around it), or no text control has focus (`None`: IME input is not wanted). Called when focus,
    /// the caret or the value changes. Default: nothing.
    fn ime_update(
        &mut self,
        doc: DocumentId,
        request: Option<ui4me_engine_types::ime::ImeRequest>,
    ) {
        let _ = (doc, request);
    }
    /// A form's controls were reset. Default: nothing.
    fn form_reset(&mut self, doc: DocumentId, form: NodeId) {
        let _ = (doc, form);
    }
    /// A file input's activation behaviour ran: the host shows its file picker. Default: nothing.
    fn file_input_activated(&mut self, doc: DocumentId, input: NodeId) {
        let _ = (doc, input);
    }
    /// A hyperlink's activation behaviour ran (an uncanceled click). Default: nothing.
    fn link_activated(&mut self, doc: DocumentId, link: NodeId) {
        let _ = (doc, link);
    }
    /// Ask the platform to drop the current IME preedit (the composition was canceled by the
    /// engine). Default: nothing.
    fn ime_reset(&mut self, doc: DocumentId) {
        let _ = doc;
    }
    /// The content box of an element from the committed layout (flushes first), `(x, y, width,
    /// height)` in viewport space; `None` when it has no box or the host has no layout. Default: none.
    fn element_content_rect(
        &mut self,
        doc: DocumentId,
        node: NodeId,
    ) -> Result<Option<(f64, f64, f64, f64)>, Status> {
        let _ = (doc, node);
        Ok(None)
    }
    /// Set the scroll offset of a target, clamped to the published scrollable overflow and
    /// snapped to 1/64 px (layout's `ScrollOffsets`), and note the scroll input to the pipeline
    /// (a paint-only update, no relayout). A non-scroll-container changes nothing. Default:
    /// nothing scrolls.
    fn scroll_set(
        &mut self,
        doc: DocumentId,
        target: Option<NodeId>,
        x: f64,
        y: f64,
    ) -> Result<crate::interaction::scroll::ScrollApplied, Status> {
        let _ = (doc, target, x, y);
        Ok(crate::interaction::scroll::ScrollApplied {
            offset: (0.0, 0.0),
            changed: false,
            clamped: false,
        })
    }
    /// A user scroll (wheel, keys, track paging) of `target` by `(dx, dy)`: cancels a smooth
    /// scroll of the target and applies snapping. Default: a plain `scroll_set` of the sum.
    fn scroll_user_by(
        &mut self,
        doc: DocumentId,
        target: Option<NodeId>,
        dx: f64,
        dy: f64,
    ) -> Result<crate::interaction::scroll::ScrollApplied, Status> {
        let (x, y) = self
            .scroll_info(doc, target)?
            .map_or((0.0, 0.0), |i| i.offset);
        self.scroll_set(doc, target, x + dx, y + dy)
    }
    /// A user drag of the scrollbar takes over `target`: cancels its smooth scroll. Default: none.
    fn scroll_cancel(&mut self, doc: DocumentId, target: Option<NodeId>) -> Result<(), Status> {
        let _ = (doc, target);
        Ok(())
    }
    /// The user released the scrollbar thumb: settle on a snap position. `true` when the offset
    /// changed at once. Default: nothing.
    fn scroll_user_settle(
        &mut self,
        doc: DocumentId,
        target: Option<NodeId>,
    ) -> Result<bool, Status> {
        let _ = (doc, target);
        Ok(false)
    }
}

/// Per-document interaction state.
pub(crate) struct DocState {
    pub ui_key: UiStateKey,
    pub listeners: ListenerRegistry,
    /// Shared with 003's removal hook (synchronous focus fix-up, R-14).
    pub focus: Rc<RefCell<FocusShared>>,
    /// Filled by 003's `on_nodes_freed` hook.
    pub freed: Rc<RefCell<Vec<NodeId>>>,
    pub pointer: PointerState,
    /// Pointer capture (0.8 F0807).
    pub capture: crate::interaction::sequences::CaptureState,
    pub scrollbar_drag: Option<crate::interaction::scroll::ScrollbarDrag>,
    pub last_seq: Option<u64>,
    /// Hover, `:active` and modality state (FR-031, FR-042, FR-048).
    pub hover: crate::interaction::pointer::HoverState,
    /// The cursor keyword last applied to the document's windows.
    pub cursor: Option<String>,
    /// Scroll events waiting for the end of the operation (one per target).
    pub pending_scroll: Vec<crate::interaction::scroll::PendingScroll>,
    /// Text editing state (caret goal, drag, click count, `change` baselines).
    pub edit: crate::interaction::editing::EditState,
    /// Select popup, type-ahead and range drag state (lane L-CTRL-r10).
    pub controls: crate::interaction::controls::ControlsState,
    /// `dialog` and `details` state and the queued element tasks (015 HTML-1, HTML-2).
    pub dialog: crate::interaction::dialog::DialogDocState,
}

/// InteractionRuntime (U4C-INT): one per HostRuntime; owns the frame stack, nesting depth,
/// COMMIT timing and deferral queues (R-35).
pub struct InteractionRuntime {
    pub(crate) docs: HashMap<DocumentId, DocState>,
    pub(crate) stack: FrameStack,
    pub(crate) ids: DispatchIdSource,
    pub(crate) next_listener: u64,
    pub(crate) policy: FailurePolicy,
    pub(crate) log: Ring<DispatchRecord>,
    /// Completed records of the current outermost operation, pushed in start order at COMMIT.
    pub(crate) pending_records: Vec<DispatchRecord>,
    pub(crate) pending_releases: Vec<Released>,
    /// Outermost operations in progress (host call or input item); > 0 ⇒ input is queued.
    pub(crate) span: u32,
    pub(crate) draining: bool,
    pub(crate) commit_docs: Vec<DocumentId>,
    pub(crate) commits: u64,
    pub(crate) queue: NativeInputQueue,
    pub(crate) click_in_progress: HashSet<(DocumentId, NodeId)>,
    /// Forms whose `submit` event is being fired (HTML "firing submission events").
    pub(crate) submitting: HashSet<(DocumentId, NodeId)>,
    /// The `clipboardData` of the clipboard event that dispatched last, taken by the default action.
    pub(crate) last_clipboard: Option<ui4me_engine_types::clipboard::ClipboardData>,
}

impl std::fmt::Debug for InteractionRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractionRuntime")
            .field("documents", &self.docs.len())
            .field("depth", &self.stack.depth())
            .field("commits", &self.commits)
            .finish_non_exhaustive()
    }
}

impl Default for InteractionRuntime {
    fn default() -> Self {
        InteractionRuntime::new()
    }
}

pub(crate) fn status(code: Code) -> Status {
    Status::new(code, Stage::Event)
}

fn stale() -> Status {
    status(Code::StaleHandle).with_message("event handle is not valid outside its callback")
}

/// Run `f` on the interaction runtime through the host.
pub(crate) fn rt<R>(
    host: &mut dyn DispatchHost,
    f: impl FnOnce(&mut InteractionRuntime) -> R,
) -> R {
    let mut f = Some(f);
    let mut out = None;
    host.with_runtime(&mut |ix| {
        if let Some(f) = f.take() {
            out = Some(f(ix));
        }
    });
    out.expect("DispatchHost::with_runtime must run its closure")
}

/// Run `f` on the interaction runtime and a document through the host.
pub(crate) fn eng<R>(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    f: impl FnOnce(&mut InteractionRuntime, &mut Document) -> R,
) -> Result<R, Status> {
    let mut f = Some(f);
    let mut out = None;
    host.with_engine(doc, &mut |ix, d| {
        if let Some(f) = f.take() {
            out = Some(f(ix, d));
        }
    })?;
    out.ok_or_else(|| {
        status(Code::InternalInvariant)
            .with_message("DispatchHost::with_engine did not run its closure")
    })
}

pub(crate) fn is_closing(d: &Document) -> bool {
    matches!(d.lifecycle(), Lifecycle::Closing | Lifecycle::Closed)
}

impl InteractionRuntime {
    pub fn new() -> Self {
        InteractionRuntime::with_event_log_capacity(EVENT_LOG_CAPACITY)
    }

    pub fn with_event_log_capacity(capacity: usize) -> Self {
        InteractionRuntime {
            docs: HashMap::new(),
            stack: FrameStack::default(),
            ids: DispatchIdSource::new(),
            next_listener: 0,
            policy: FailurePolicy::default(),
            log: Ring::with_capacity(capacity),
            pending_records: vec![],
            pending_releases: vec![],
            span: 0,
            draining: false,
            commit_docs: vec![],
            commits: 0,
            queue: NativeInputQueue::default(),
            click_in_progress: HashSet::new(),
            submitting: HashSet::new(),
            last_clipboard: None,
        }
    }

    pub fn set_failure_policy(&mut self, p: FailurePolicy) {
        self.policy = p;
    }

    pub fn failure_policy(&self) -> FailurePolicy {
        self.policy
    }

    // ------------------------------------------------------------------ documents

    /// Register a document: takes its UI-state writer key and installs 003's removal and
    /// nodes-freed hooks. Call once at document creation (before any listener is added).
    pub fn attach_document(&mut self, d: &mut Document) -> Result<(), Status> {
        let id = d.id();
        if self.docs.contains_key(&id) {
            return Err(status(Code::InvalidState).with_message("document already attached"));
        }
        let ui_key = d.take_ui_state_key().ok_or_else(|| {
            status(Code::InvalidState).with_message("the document's UI-state key was already taken")
        })?;
        let focus = Rc::new(RefCell::new(FocusShared::default()));
        let freed = Rc::new(RefCell::new(Vec::new()));
        crate::interaction::focus::install_hooks(d, focus.clone(), freed.clone());
        let dialog = crate::interaction::dialog::DialogDocState::default();
        let tasks = dialog.tasks.clone();
        d.hooks_mut()
            .set_on_attribute_changed(Box::new(move |w, c| {
                crate::interaction::dialog::on_attribute_changed(&tasks, w, c);
                crate::interaction::controls::on_attribute_changed(w, c);
            }));
        self.docs.insert(
            id,
            DocState {
                ui_key,
                listeners: ListenerRegistry::default(),
                focus,
                freed,
                pointer: PointerState::default(),
                capture: Default::default(),
                scrollbar_drag: None,
                last_seq: None,
                hover: Default::default(),
                cursor: None,
                pending_scroll: Vec::new(),
                edit: Default::default(),
                controls: Default::default(),
                dialog,
            },
        );
        Ok(())
    }

    pub fn is_attached(&self, doc: DocumentId) -> bool {
        self.docs.contains_key(&doc)
    }

    /// Forget a document (destroyed): drops its listeners (returned for release) and queued
    /// input. Must not be called while one of its dispatches is on the stack.
    pub fn detach_document(&mut self, doc: DocumentId) -> Vec<Released> {
        self.queue.remove_document(doc);
        self.click_in_progress.retain(|(d, _)| *d != doc);
        self.submitting.retain(|(d, _)| *d != doc);
        self.docs
            .remove(&doc)
            .map(|mut s| s.listeners.drain_all())
            .unwrap_or_default()
    }

    pub(crate) fn doc_state(&mut self, doc: DocumentId) -> Result<&mut DocState, Status> {
        let s = self.docs.get_mut(&doc).ok_or_else(|| {
            status(Code::InvalidState)
                .with_message("document not attached to the interaction runtime")
        })?;
        // Listeners of nodes freed at a collection checkpoint are dropped (003 on_nodes_freed).
        let freed: Vec<NodeId> = s.freed.borrow_mut().drain(..).collect();
        if !freed.is_empty() {
            let rel = s.listeners.drop_nodes(&freed);
            self.pending_releases.extend(rel);
        }
        Ok(self.docs.get_mut(&doc).expect("checked"))
    }

    // ------------------------------------------------------------------ listeners

    /// `addEventListener`. A duplicate (same target, type, capture, identity key) returns the
    /// existing token with `duplicate = true`.
    pub fn add_listener(
        &mut self,
        d: &Document,
        target: NodeId,
        spec: ListenerSpec,
    ) -> Result<AddOutcome, Status> {
        if !d.arena().contains(target) {
            return Err(status(Code::StaleHandle));
        }
        if is_closing(d) {
            return Err(status(Code::DocumentClosed));
        }
        let doc = d.id();
        self.doc_state(doc)?;
        let s = self.docs.get_mut(&doc).expect("checked");
        if let Some(e) = s.listeners.find_matching(
            target,
            &spec.event_type,
            spec.options.capture,
            spec.identity_key,
        ) {
            return Ok(AddOutcome {
                token: e.token,
                duplicate: true,
            });
        }
        self.next_listener += 1;
        let token = ListenerToken::new(self.next_listener).expect("starts at 1");
        Ok(s.listeners.add(token, target, spec))
    }

    /// `removeEventListener` by token; idempotent. Outside any operation the registration is
    /// returned for immediate release; inside one, release is deferred to COMMIT (`None`).
    pub fn remove_listener(&mut self, token: ListenerToken) -> Option<Released> {
        let doc = self
            .docs
            .iter()
            .find(|(_, s)| s.listeners.contains(token))
            .map(|(d, _)| *d)?;
        let e = self.docs.get_mut(&doc)?.listeners.remove(token)?;
        let r = Released {
            listener: e.token,
            callback: e.callback,
        };
        if self.in_operation() {
            self.pending_releases.push(r);
            None
        } else {
            Some(r)
        }
    }

    /// `removeEventListener(type, callback, {capture})` by identity key.
    pub fn remove_matching(
        &mut self,
        doc: DocumentId,
        target: NodeId,
        event_type: &str,
        capture: bool,
        identity_key: u64,
    ) -> Option<Released> {
        let token = self
            .docs
            .get(&doc)?
            .listeners
            .find_matching(target, event_type, capture, identity_key)?
            .token;
        self.remove_listener(token)
    }

    /// Live registrations on `target`.
    pub fn listener_count(&self, doc: DocumentId, target: NodeId) -> usize {
        self.docs
            .get(&doc)
            .map_or(0, |s| s.listeners.count_on(target))
    }

    // ------------------------------------------------------------------ event handle API

    fn frame_in_callback(&self, ev: DispatchId) -> Result<&DispatchFrame, Status> {
        self.stack
            .find(ev)
            .filter(|f| f.in_callback)
            .ok_or_else(stale)
    }

    fn frame_in_callback_mut(&mut self, ev: DispatchId) -> Result<&mut DispatchFrame, Status> {
        self.stack
            .find_mut(ev)
            .filter(|f| f.in_callback)
            .ok_or_else(stale)
    }

    /// The scoped event record (getters).
    pub fn event(&self, ev: DispatchId) -> Result<&EventState, Status> {
        Ok(&self.frame_in_callback(ev)?.event)
    }
    pub fn event_type(&self, ev: DispatchId) -> Result<&str, Status> {
        Ok(&self.event(ev)?.init.event_type)
    }
    pub fn event_target(&self, ev: DispatchId) -> Result<NodeId, Status> {
        Ok(self.event(ev)?.target)
    }
    pub fn event_current_target(&self, ev: DispatchId) -> Result<Option<NodeId>, Status> {
        Ok(self.event(ev)?.current_target)
    }
    /// `None` = `NONE`.
    pub fn event_phase(&self, ev: DispatchId) -> Result<Option<EventPhase>, Status> {
        Ok(self.event(ev)?.phase)
    }
    pub fn event_bubbles(&self, ev: DispatchId) -> Result<bool, Status> {
        Ok(self.event(ev)?.init.bubbles)
    }
    pub fn event_cancelable(&self, ev: DispatchId) -> Result<bool, Status> {
        Ok(self.event(ev)?.init.cancelable)
    }
    pub fn event_default_prevented(&self, ev: DispatchId) -> Result<bool, Status> {
        Ok(self.event(ev)?.flags.canceled)
    }
    pub fn event_is_trusted(&self, ev: DispatchId) -> Result<bool, Status> {
        Ok(self.event(ev)?.is_trusted)
    }
    pub fn event_time_stamp(&self, ev: DispatchId) -> Result<f64, Status> {
        Ok(self.event(ev)?.time_stamp_ms)
    }
    pub fn event_data(&self, ev: DispatchId) -> Result<&EventData, Status> {
        Ok(&self.event(ev)?.init.data)
    }
    /// `ClipboardEvent.clipboardData.getData(format)`: `None` when the event has no clipboard data or
    /// holds no such format.
    pub fn event_clipboard_get(
        &self,
        ev: DispatchId,
        mime: &str,
    ) -> Result<Option<String>, Status> {
        Ok(self
            .event(ev)?
            .init
            .clipboard
            .as_ref()
            .and_then(|c| c.get(mime))
            .map(str::to_owned))
    }
    /// Whether the event is a clipboard event (it has `clipboardData`).
    pub fn event_has_clipboard(&self, ev: DispatchId) -> Result<bool, Status> {
        Ok(self.event(ev)?.init.clipboard.is_some())
    }
    /// `clipboardData.setData(format, data)`: allowed on `copy` and `cut` only (on `paste` the store is
    /// read-only: `INVALID_STATE`); an unsupported format is `INVALID_ARGUMENT`.
    pub fn event_clipboard_set(
        &mut self,
        ev: DispatchId,
        mime: &str,
        data: &str,
    ) -> Result<(), Status> {
        let f = self.frame_in_callback_mut(ev)?;
        let writable = matches!(f.event.init.event_type.as_str(), "copy" | "cut");
        let Some(store) = f.event.init.clipboard.as_mut() else {
            return Err(status(Code::InvalidState).with_message("the event has no clipboardData"));
        };
        if !writable {
            return Err(status(Code::InvalidState)
                .with_message("clipboardData is read-only outside copy and cut"));
        }
        if !store.set(mime, data) {
            return Err(status(Code::InvalidArgument)
                .with_message("unsupported clipboard format (text/plain and text/html only)"));
        }
        Ok(())
    }
    /// `composedPath()` while dispatching: target → … → root.
    pub fn event_path(&self, ev: DispatchId) -> Result<Vec<NodeId>, Status> {
        Ok(self.frame_in_callback(ev)?.path.clone())
    }
    /// Depth of the event's dispatch (outermost = 1).
    pub fn event_depth(&self, ev: DispatchId) -> Result<u32, Status> {
        Ok(self.frame_in_callback(ev)?.depth)
    }

    /// `preventDefault()`: ignored (and recorded) for non-cancelable events and in passive
    /// listeners.
    pub fn prevent_default(&mut self, ev: DispatchId) -> Result<(), Status> {
        let f = self.frame_in_callback_mut(ev)?;
        let call = if !f.event.init.cancelable {
            ListenerCall::PreventDefaultIgnoredNoncancelable
        } else if f.event.flags.in_passive {
            ListenerCall::PreventDefaultIgnoredPassive
        } else {
            f.event.flags.canceled = true;
            ListenerCall::PreventDefault
        };
        f.note_call(call);
        Ok(())
    }

    pub fn stop_propagation(&mut self, ev: DispatchId) -> Result<(), Status> {
        let f = self.frame_in_callback_mut(ev)?;
        f.event.flags.stop_propagation = true;
        f.note_call(ListenerCall::StopPropagation);
        Ok(())
    }

    pub fn stop_immediate_propagation(&mut self, ev: DispatchId) -> Result<(), Status> {
        let f = self.frame_in_callback_mut(ev)?;
        f.event.flags.stop_propagation = true;
        f.event.flags.stop_immediate = true;
        f.note_call(ListenerCall::StopImmediatePropagation);
        Ok(())
    }

    // ------------------------------------------------------------------ state views

    /// Current nesting depth (0 = idle).
    pub fn depth(&self) -> u32 {
        self.stack.depth()
    }
    pub fn is_dispatching(&self) -> bool {
        !self.stack.is_empty()
    }
    /// An outermost operation (host call or input item) is running: input is queued, and
    /// posted work must wait (004).
    pub fn in_operation(&self) -> bool {
        self.span > 0 || !self.stack.is_empty()
    }
    /// COMMITs run so far (invariant 6).
    pub fn commit_count(&self) -> u64 {
        self.commits
    }
    /// 005's event-log store (`event_log` probe input).
    pub fn event_log(&self) -> &Ring<DispatchRecord> {
        &self.log
    }
    /// The NativeInputQueue.
    pub fn input_queue(&self) -> &NativeInputQueue {
        &self.queue
    }
    /// Append a native item (011 → 012). Processed by `input_queue::drain_input`.
    pub fn queue_input(&mut self, item: InputItem) {
        self.queue.push(item);
    }
    /// The focused element (`None` = the viewport).
    /// Request (`Some`) or release (`None`) pointer capture of the mouse pointer for `element`
    /// (0.8 F0807): it applies at the pointer's next event; a request needs a button pressed
    /// (Pointer Events: an inactive pointer cannot be captured). Returns whether it was accepted.
    pub fn request_pointer_capture(
        &mut self,
        doc: DocumentId,
        element: NodeId,
        capture: bool,
    ) -> bool {
        let Some(s) = self.docs.get_mut(&doc) else {
            return false;
        };
        if capture {
            if s.pointer.press_target.is_none() {
                return false;
            }
            s.capture.pending = Some(Some(element));
            true
        } else {
            let current = s.capture.pending.unwrap_or(s.capture.target);
            if current == Some(element) {
                s.capture.pending = Some(None);
            }
            true
        }
    }
    /// The element that has (or will get at the next event) the mouse pointer's capture.
    pub fn pointer_capture(&self, doc: DocumentId) -> Option<NodeId> {
        self.docs
            .get(&doc)
            .and_then(|s| s.capture.pending.unwrap_or(s.capture.target))
    }
    pub fn focused_element(&self, doc: DocumentId) -> Option<NodeId> {
        self.docs.get(&doc).and_then(|s| s.focus.borrow().focused)
    }
    /// FR-048: the last user interaction with the document was a key press (not a pointer press).
    pub fn last_interaction_keyboard(&self, doc: DocumentId) -> bool {
        self.docs
            .get(&doc)
            .is_some_and(|s| s.hover.keyboard_modality)
    }
    /// The deepest element under the pointer (`:hover`), `None` = nothing hovered.
    pub fn hovered_element(&self, doc: DocumentId) -> Option<NodeId> {
        self.docs.get(&doc).and_then(|s| s.hover.hover)
    }
    /// The element a pressed primary button (or Space) activated (`:active`).
    pub fn active_press_element(&self, doc: DocumentId) -> Option<NodeId> {
        self.docs.get(&doc).and_then(|s| s.hover.active)
    }
    /// `document.activeElement`: the focused element, else the body.
    pub fn active_element(&self, d: &Document) -> Option<NodeId> {
        self.focused_element(d.id()).or_else(|| d.body())
    }
}

// ---------------------------------------------------------------------- the machine

/// What POST needs after the path walk.
struct PostInfo {
    canceled: bool,
    activation_target: Option<NodeId>,
    saved: Option<SavedPreActivation>,
}

impl InteractionRuntime {
    pub(crate) fn note_doc(&mut self, doc: DocumentId) {
        if !self.commit_docs.contains(&doc) {
            self.commit_docs.push(doc);
        }
    }

    /// BUILD (§1): validation, depth check, path with strong references, activation target and
    /// legacy pre-activation, frame push.
    #[allow(clippy::too_many_arguments)]
    fn build(
        &mut self,
        d: &mut Document,
        target: NodeId,
        init: EventInit,
        trusted: bool,
        origin: DispatchOrigin,
        time_stamp_ms: f64,
    ) -> Result<DispatchId, Status> {
        let doc = d.id();
        self.doc_state(doc)?;
        if !d.arena().contains(target) {
            return Err(status(Code::StaleHandle)
                .with_message("dispatch target is not a live node of the document"));
        }
        if is_closing(d) {
            return Err(status(Code::DocumentClosed));
        }
        let parent = self.stack.top().map(|f| f.dispatch_id);
        let depth = self.stack.depth() + 1;
        let id = self.ids.next();
        if let Some(f) = self.stack.top_mut() {
            match f.current.as_mut() {
                Some(c) => c.nested.push(id),
                // Fired between invocations (activation behaviour after the listener loop): the
                // dispatch belongs to the invocation that ran last, so exports and mappers that
                // walk `nested` reach it in order.
                None => {
                    if let Some(last) = f.record.invocations.last_mut() {
                        last.nested.push(id);
                    }
                }
            }
        }
        let event_data = (init.data != EventData::default()).then(|| init.data.clone());
        let mut record = DispatchRecord {
            document: doc,
            dispatch_id: id,
            parent_dispatch_id: parent,
            depth,
            event_type: init.event_type.clone(),
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            is_trusted: trusted,
            origin,
            target,
            path: vec![],
            activation_target: None,
            pre_activation: None,
            invocations: vec![],
            canceled: false,
            event_data,
            status: Code::Ok,
            nondeterministic: Nondeterministic {
                time_ms: Some(time_stamp_ms),
                ..Nondeterministic::default()
            },
        };
        self.note_doc(doc);
        if depth > DISPATCH_DEPTH_LIMIT {
            // Refused: no frame pushed; a status record in the event log (012 FR-012, R-35).
            record.status = Code::DispatchRecursionLimit;
            self.pending_records.push(record);
            return Err(status(Code::DispatchRecursionLimit)
                .with_dispatch_id(id)
                .with_message(format!(
                    "nested dispatch refused at depth {depth} (limit {DISPATCH_DEPTH_LIMIT})"
                )));
        }
        // The path, computed once: target → parents → root (the Document when connected, R-29).
        let mut path = vec![target];
        let mut n = target;
        while let Some(p) = d.arena().get(n).and_then(|x| x.parent()) {
            path.push(p);
            n = p;
        }
        let mut retained = Vec::with_capacity(path.len());
        for &p in &path {
            if d.retain(p).is_ok() {
                retained.push(p);
            }
        }
        let activation_target = activation::choose_target(d, &path, &init);
        let saved = activation_target.and_then(|a| activation::legacy_pre_activation(d, a));
        record.path = path.clone();
        record.activation_target = activation_target;
        record.pre_activation = saved.map(|s| match s {
            SavedPreActivation::Checkbox { checked, .. } => PreActivation {
                performed: true,
                saved: checked,
            },
            SavedPreActivation::Radio { was_checked, .. } => PreActivation {
                performed: true,
                saved: was_checked,
            },
        });
        let event = EventState {
            init,
            is_trusted: trusted,
            time_stamp_ms,
            target,
            current_target: None,
            phase: None,
            flags: EventFlags {
                dispatch: true,
                ..EventFlags::default()
            },
        };
        self.stack.frames.push(DispatchFrame {
            dispatch_id: id,
            parent,
            depth,
            document: doc,
            origin,
            event,
            path,
            retained,
            activation_target,
            saved,
            cursor: Cursor {
                pass: Pass::Capture,
                item: 0,
                snapshot: None,
                entry: 0,
            },
            record,
            current: None,
            in_callback: false,
        });
        Ok(id)
    }

    /// CAPTURE ▸ TARGET ▸ BUBBLE: advance the cursor of the top frame to the next listener to
    /// invoke (taking per-item, per-phase snapshots), or `None` when the walk is over.
    fn next_step(&mut self, id: DispatchId) -> Option<Invocation> {
        let InteractionRuntime {
            stack,
            docs,
            pending_releases,
            ..
        } = self;
        let frame = stack.top_mut().filter(|f| f.dispatch_id == id)?;
        let registry = &mut docs.get_mut(&frame.document)?.listeners;
        loop {
            let n_items = frame.path.len();
            if frame.cursor.item >= n_items {
                match frame.cursor.pass {
                    Pass::Capture => {
                        frame.cursor = Cursor {
                            pass: Pass::Bubble,
                            item: 0,
                            snapshot: None,
                            entry: 0,
                        };
                        continue;
                    }
                    Pass::Bubble => return None,
                }
            }
            let pass = frame.cursor.pass;
            let idx = match pass {
                Pass::Capture => n_items - 1 - frame.cursor.item,
                Pass::Bubble => frame.cursor.item,
            };
            let node = frame.path[idx];
            let at_target = idx == 0;
            if frame.cursor.snapshot.is_none() {
                let skip_bubble = pass == Pass::Bubble && !at_target && !frame.event.init.bubbles;
                if frame.event.flags.stop_propagation || skip_bubble {
                    frame.cursor.item += 1;
                    continue;
                }
                frame.event.current_target = Some(node);
                frame.event.phase = Some(if at_target {
                    EventPhase::AtTarget
                } else if pass == Pass::Capture {
                    EventPhase::Capturing
                } else {
                    EventPhase::Bubbling
                });
                frame.cursor.snapshot = Some(registry.snapshot(node));
                frame.cursor.entry = 0;
            }
            let snapshot = frame.cursor.snapshot.as_ref().expect("taken above");
            while frame.cursor.entry < snapshot.len() {
                let e = snapshot[frame.cursor.entry].clone();
                frame.cursor.entry += 1;
                if e.is_removed()
                    || e.event_type != frame.event.init.event_type
                    || e.options.capture != (pass == Pass::Capture)
                {
                    continue;
                }
                if e.options.once {
                    // Removed before the call; released at COMMIT.
                    registry.remove(e.token);
                    pending_releases.push(Released {
                        listener: e.token,
                        callback: e.callback,
                    });
                }
                frame.event.flags.in_passive = e.options.passive;
                frame.in_callback = true;
                let phase = frame.event.phase.expect("set with the snapshot");
                frame.current = Some(InvocationRecord {
                    seq: frame.record.invocations.len() as u32,
                    phase,
                    current_target: node,
                    listener: e.record(),
                    outcome: InvocationOutcome::Returned,
                    diagnostic_id: None,
                    default_prevented_after: false,
                    propagation_stopped_after: false,
                    immediate_stopped_after: false,
                    calls: vec![],
                    nested: vec![],
                });
                return Some(Invocation {
                    document: frame.document,
                    event: frame.dispatch_id,
                    listener: e.token,
                    callback: e.callback,
                    label: e.label.clone(),
                    ordinal: e.ordinal,
                    event_type: e.event_type.clone(),
                    phase,
                    current_target: node,
                    depth: frame.depth,
                });
            }
            frame.cursor.snapshot = None;
            frame.cursor.item += 1;
        }
    }

    /// Back from IN_CALLBACK: apply the failure policy (§4), clear in-passive, record the
    /// invocation, honour stop-immediate.
    fn finish_invocation(&mut self, id: DispatchId, failure: Option<u64>) {
        let policy = self.policy;
        let Some(frame) = self.stack.top_mut().filter(|f| f.dispatch_id == id) else {
            return;
        };
        frame.in_callback = false;
        let Some(mut inv) = frame.current.take() else {
            return;
        };
        if let Some(diag) = failure {
            inv.outcome = InvocationOutcome::Failed;
            inv.diagnostic_id = Some(diag);
            if policy == FailurePolicy::Strict {
                let f = &mut frame.event.flags;
                if frame.event.init.cancelable && !f.in_passive {
                    f.canceled = true;
                }
                f.stop_propagation = true;
                f.stop_immediate = true;
            }
        }
        let flags = &mut frame.event.flags;
        flags.in_passive = false;
        inv.default_prevented_after = flags.canceled;
        inv.propagation_stopped_after = flags.stop_propagation;
        inv.immediate_stopped_after = flags.stop_immediate;
        frame.record.invocations.push(inv);
        if flags.stop_immediate {
            frame.cursor.entry = usize::MAX;
        }
    }

    /// POST, first part: reset the event's dispatch state.
    fn post(&mut self, id: DispatchId) -> Option<PostInfo> {
        let frame = self.stack.top_mut().filter(|f| f.dispatch_id == id)?;
        let e = &mut frame.event;
        e.phase = None;
        e.current_target = None;
        e.flags.dispatch = false;
        e.flags.stop_propagation = false;
        e.flags.stop_immediate = false;
        Some(PostInfo {
            canceled: e.flags.canceled,
            activation_target: frame.activation_target,
            saved: frame.saved,
        })
    }

    /// Pop the frame; its record goes to the pending list (event log at COMMIT). Returns the
    /// retained nodes to release.
    fn pop(&mut self, id: DispatchId) -> Vec<NodeId> {
        let Some(pos) = self.stack.frames.iter().rposition(|f| f.dispatch_id == id) else {
            return vec![];
        };
        debug_assert_eq!(pos + 1, self.stack.frames.len(), "frames pop in LIFO order");
        let mut frame = self.stack.frames.remove(pos);
        frame.record.canceled = frame.event.flags.canceled;
        self.pending_records.push(frame.record);
        frame.retained
    }

    /// COMMIT bookkeeping: records to the log in start order; returns the pending releases.
    fn finish_commit(&mut self) -> Vec<Released> {
        let mut recs = std::mem::take(&mut self.pending_records);
        recs.sort_by_key(|r| r.dispatch_id);
        for r in recs {
            self.log.push(r);
        }
        self.commits += 1;
        std::mem::take(&mut self.pending_releases)
    }
}

fn listener_name(inv: &Invocation) -> String {
    inv.label
        .clone()
        .unwrap_or_else(|| format!("#{}", inv.ordinal))
}

/// BUILD…RETURN for one dispatch (no COMMIT; the caller's outermost operation commits).
pub(crate) fn run_dispatch(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    init: EventInit,
    trusted: bool,
    origin: DispatchOrigin,
    time_stamp_ms: Option<f64>,
) -> Result<DispatchOutcome, Status> {
    let ts = match time_stamp_ms {
        Some(t) => t,
        None => host.now_ms(),
    };
    let id = eng(host, doc, |ix, d| {
        ix.build(d, target, init, trusted, origin, ts)
    })??;
    // CAPTURE ▸ TARGET ▸ BUBBLE, one IN_CALLBACK per listener with no engine borrow held.
    while let Some(inv) = rt(host, |ix| ix.next_step(id)) {
        let failure = match host.invoke(&inv) {
            Ok(()) => None,
            Err(e) => {
                let st = status(Code::CallbackFailed)
                    .with_dispatch_id(id)
                    .with_message(format!(
                        "listener {} failed: {}",
                        listener_name(&inv),
                        e.message
                    ));
                Some(host.record_error(doc, st).diagnostic_id)
            }
        };
        rt(host, |ix| ix.finish_invocation(id, failure));
    }
    // POST
    let info = rt(host, |ix| ix.post(id));
    let canceled = info.as_ref().is_some_and(|i| i.canceled);
    if let Some(info) = info
        && let Some(act) = info.activation_target
    {
        if info.canceled {
            if let Some(saved) = info.saved {
                let _ = eng(host, doc, |_, d| {
                    activation::legacy_canceled_activation(d, act, saved)
                });
            }
        } else {
            activation::activation_behaviour(host, doc, act, id);
        }
    }
    rt(host, |ix| {
        if let Some(c) = ix
            .stack
            .find_mut(id)
            .and_then(|f| f.event.init.clipboard.take())
        {
            ix.last_clipboard = Some(c);
        }
    });
    let retained = rt(host, |ix| ix.pop(id));
    if !retained.is_empty() {
        let _ = eng(host, doc, |_, d| {
            for n in &retained {
                let _ = d.release(*n);
            }
        });
    }
    Ok(DispatchOutcome {
        dispatch_id: id,
        canceled,
    })
}

/// COMMIT (§1): deferred closes, 003 outermost commit and collection checkpoint, focus validity
/// fix-up, release notifications, event-log records, one frame request per document.
fn commit(host: &mut dyn DispatchHost) {
    host.run_deferred_closes();
    let docs = rt(host, |ix| std::mem::take(&mut ix.commit_docs));
    for &d in &docs {
        host.outermost_commit(d);
    }
    for &doc in &docs {
        let _ = eng(host, doc, |ix, d| {
            if ix.doc_state(doc).is_err() {
                return;
            }
            if d.lifecycle() == Lifecycle::Closed {
                // Document closed: every registration is released (004 T039).
                let rel = ix
                    .docs
                    .get_mut(&doc)
                    .map(|s| s.listeners.drain_all())
                    .unwrap_or_default();
                ix.pending_releases.extend(rel);
                ix.queue.remove_document(doc);
                return;
            }
            crate::interaction::focus::validity_fixup(ix, d);
        });
    }
    let released = rt(host, |ix| ix.finish_commit());
    if !released.is_empty() {
        host.release_callbacks(&released);
    }
    for &d in &docs {
        host.request_frame(d);
    }
}

/// Run `op` as one outermost operation when nothing is running (COMMIT afterwards, then the
/// input queue drains), or as a nested operation otherwise.
pub(crate) fn operation<R>(
    host: &mut dyn DispatchHost,
    op: impl FnOnce(&mut dyn DispatchHost) -> R,
) -> R {
    let outer = rt(host, |ix| {
        let o = !ix.in_operation();
        ix.span += 1;
        o
    });
    let r = op(host);
    rt(host, |ix| ix.span -= 1);
    if outer {
        commit(host);
        crate::interaction::input_queue::drain_input(host);
    }
    r
}

/// `dispatchEvent(event)` from the host: a synthetic (untrusted) event, origin
/// `dispatch_event`. Inside a callback it is a nested dispatch (depth ≤ 32).
pub fn dispatch_event(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    init: EventInit,
) -> Result<DispatchOutcome, Status> {
    if init.event_type.len() > 1024 {
        return Err(status(Code::SizeLimit).with_message("event type longer than 1024 bytes"));
    }
    let coords = [init.data.client_x, init.data.client_y];
    if coords.iter().flatten().any(|v| !v.is_finite()) {
        return Err(status(Code::InvalidArgument).with_message("non-finite event coordinate"));
    }
    operation(host, |h| {
        run_dispatch(
            h,
            doc,
            target,
            init,
            false,
            DispatchOrigin::DispatchEvent,
            None,
        )
    })
}

/// HTML `element.click()`: a synthetic, untrusted `click` (PointerEvent, bubbles, cancelable),
/// origin `click()`; nothing for a disabled form control or while a click() on the element
/// is in progress.
pub fn click(host: &mut dyn DispatchHost, doc: DocumentId, element: NodeId) -> Result<(), Status> {
    operation(host, |h| {
        let go = eng(h, doc, |ix, d| {
            if !d.arena().contains(element) {
                return Err(status(Code::StaleHandle));
            }
            if is_closing(d) {
                return Err(status(Code::DocumentClosed));
            }
            if activation::is_disabled_form_control(d, element)
                || !ix.click_in_progress.insert((doc, element))
            {
                return Ok(false);
            }
            Ok(true)
        })??;
        if !go {
            return Ok(());
        }
        let init = EventInit::new("click")
            .interface(EventInterface::PointerEvent)
            .bubbles(true)
            .cancelable(true);
        let r = run_dispatch(h, doc, element, init, false, DispatchOrigin::Click, None);
        rt(h, |ix| ix.click_in_progress.remove(&(doc, element)));
        r.map(|_| ())
    })
}
