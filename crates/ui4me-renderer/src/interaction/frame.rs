//! owner: 012-interaction — created by 001 slice (R-28)
//!
//! Dispatch frames and the frame stack (001 T058; 012 `dispatch-state-machine.md` §2). The stack
//! is the single source of nesting state (R-35): depth counts the outermost dispatch as 1 and a
//! 33rd nested dispatch is refused with `DISPATCH_RECURSION_LIMIT` before any frame is pushed.
//! Frames hold no reference into DocumentCore: nodes are `NodeId`s kept alive by retention
//! counts taken at BUILD and released when the frame pops (AD-29).

use crate::interaction::listeners::ListenerEntry;
use std::rc::Rc;
use ui4me_engine_types::diag::{
    DispatchOrigin, DispatchRecord, EventData, EventPhase, InvocationRecord, ListenerCall,
};
use ui4me_engine_types::ids::{DispatchId, DocumentId, NodeId};

/// Maximum nesting depth; the outermost dispatch is depth 1 (012 FR-012, R-35).
pub const DISPATCH_DEPTH_LIMIT: u32 = 32;

/// Event interface (012 `event-types.md` §1; the slice subset).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventInterface {
    Event,
    CustomEvent,
    UiEvent,
    FocusEvent,
    MouseEvent,
    PointerEvent,
    WheelEvent,
    KeyboardEvent,
    InputEvent,
    CompositionEvent,
    ClipboardEvent,
    SubmitEvent,
    TransitionEvent,
    AnimationEvent,
    /// `beforetoggle` / `toggle` of `details` and `dialog` (HTML ToggleEvent: `oldState`, `newState`).
    ToggleEvent,
}

impl EventInterface {
    pub const fn name(self) -> &'static str {
        match self {
            EventInterface::Event => "Event",
            EventInterface::CustomEvent => "CustomEvent",
            EventInterface::UiEvent => "UIEvent",
            EventInterface::FocusEvent => "FocusEvent",
            EventInterface::MouseEvent => "MouseEvent",
            EventInterface::PointerEvent => "PointerEvent",
            EventInterface::WheelEvent => "WheelEvent",
            EventInterface::KeyboardEvent => "KeyboardEvent",
            EventInterface::InputEvent => "InputEvent",
            EventInterface::CompositionEvent => "CompositionEvent",
            EventInterface::ClipboardEvent => "ClipboardEvent",
            EventInterface::SubmitEvent => "SubmitEvent",
            EventInterface::TransitionEvent => "TransitionEvent",
            EventInterface::AnimationEvent => "AnimationEvent",
            EventInterface::ToggleEvent => "ToggleEvent",
        }
    }
    pub const fn is_mouse(self) -> bool {
        matches!(
            self,
            EventInterface::MouseEvent | EventInterface::PointerEvent
        )
    }
}

/// Modifier state carried by UI events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// What an event is created with (synthetic `dispatch_event` init or a trusted producer).
#[derive(Clone, Debug, PartialEq)]
pub struct EventInit {
    pub event_type: String,
    pub interface: EventInterface,
    pub bubbles: bool,
    pub cancelable: bool,
    /// Observable payload (key/code, data, button, client coordinates).
    pub data: EventData,
    /// MouseEvent `buttons`.
    pub buttons: u16,
    /// UIEvent `detail` (click count for mouse events).
    pub detail: i32,
    pub modifiers: Modifiers,
    /// FocusEvent/MouseEvent `relatedTarget`.
    pub related_target: Option<NodeId>,
    /// CustomEvent `detail` (host string).
    pub custom_detail: Option<String>,
    /// `ClipboardEvent.clipboardData` (a DataTransfer): the clipboard content for `paste`, an empty
    /// store a `copy`/`cut` listener fills with `setData`. Never part of the event-log trace.
    pub clipboard: Option<ui4me_engine_types::clipboard::ClipboardData>,
}

impl EventInit {
    /// A plain `Event` that neither bubbles nor is cancelable (DOM `new Event(type)`).
    pub fn new(event_type: impl Into<String>) -> Self {
        EventInit {
            event_type: event_type.into(),
            interface: EventInterface::Event,
            bubbles: false,
            cancelable: false,
            data: EventData::default(),
            buttons: 0,
            detail: 0,
            modifiers: Modifiers::default(),
            related_target: None,
            custom_detail: None,
            clipboard: None,
        }
    }
    pub fn interface(mut self, i: EventInterface) -> Self {
        self.interface = i;
        self
    }
    pub fn bubbles(mut self, on: bool) -> Self {
        self.bubbles = on;
        self
    }
    pub fn cancelable(mut self, on: bool) -> Self {
        self.cancelable = on;
        self
    }
    pub fn related_target(mut self, n: Option<NodeId>) -> Self {
        self.related_target = n;
        self
    }
    /// A bubbling, cancelable mouse-family event at a viewport point.
    pub fn mouse(
        event_type: impl Into<String>,
        interface: EventInterface,
        x: f64,
        y: f64,
        button: i16,
        buttons: u16,
    ) -> Self {
        let mut e = EventInit::new(event_type)
            .interface(interface)
            .bubbles(true)
            .cancelable(true);
        e.data.button = Some(button);
        e.data.client_x = Some(x);
        e.data.client_y = Some(y);
        e.buttons = buttons;
        e
    }
}

/// Event flags (DOM §2.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventFlags {
    pub canceled: bool,
    pub stop_propagation: bool,
    pub stop_immediate: bool,
    pub in_passive: bool,
    pub dispatch: bool,
}

/// The event record owned by a frame (§5).
#[derive(Clone, Debug, PartialEq)]
pub struct EventState {
    pub init: EventInit,
    pub is_trusted: bool,
    pub time_stamp_ms: f64,
    pub target: NodeId,
    pub current_target: Option<NodeId>,
    pub phase: Option<EventPhase>,
    pub flags: EventFlags,
}

/// Which pass of the path walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pass {
    Capture,
    Bubble,
}

/// Saved legacy-pre-activation state (§2 `saved_pre_activation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SavedPreActivation {
    Checkbox {
        checked: bool,
        indeterminate: bool,
    },
    /// A radio: its own checkedness and the other radio of its group that was checked.
    Radio {
        was_checked: bool,
        previous: Option<NodeId>,
    },
}

/// Persisted cursor `{phase, item_index, entry_index}` with the current snapshot.
#[derive(Debug)]
pub(crate) struct Cursor {
    pub pass: Pass,
    /// Index into the pass order (capture: root → target; bubble: target → root).
    pub item: usize,
    pub snapshot: Option<Vec<Rc<ListenerEntry>>>,
    pub entry: usize,
}

/// One dispatch on the stack (§2).
#[derive(Debug)]
pub(crate) struct DispatchFrame {
    pub dispatch_id: DispatchId,
    #[allow(dead_code)] // §2 field; the trace carries it as `parent_dispatch_id`
    pub parent: Option<DispatchId>,
    pub depth: u32,
    pub document: DocumentId,
    #[allow(dead_code)] // §2 field; the trace carries it as `origin`
    pub origin: DispatchOrigin,
    pub event: EventState,
    /// target → … → root; never pruned or changed after BUILD (invariant 1).
    pub path: Vec<NodeId>,
    /// Path nodes whose retention count this frame holds (released once at pop, invariant 5).
    pub retained: Vec<NodeId>,
    pub activation_target: Option<NodeId>,
    pub saved: Option<SavedPreActivation>,
    pub cursor: Cursor,
    /// The trace being built; pushed to the event log when the frame pops.
    pub record: DispatchRecord,
    /// The invocation in progress while IN_CALLBACK.
    pub current: Option<InvocationRecord>,
    pub in_callback: bool,
}

impl DispatchFrame {
    pub(crate) fn note_call(&mut self, call: ListenerCall) {
        if let Some(c) = self.current.as_mut() {
            c.calls.push(call);
        }
    }
}

/// The runtime-wide frame stack.
#[derive(Debug, Default)]
pub(crate) struct FrameStack {
    pub frames: Vec<DispatchFrame>,
}

impl FrameStack {
    pub fn depth(&self) -> u32 {
        self.frames.len() as u32
    }
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub fn top(&self) -> Option<&DispatchFrame> {
        self.frames.last()
    }
    pub fn top_mut(&mut self) -> Option<&mut DispatchFrame> {
        self.frames.last_mut()
    }
    pub fn find(&self, id: DispatchId) -> Option<&DispatchFrame> {
        self.frames.iter().find(|f| f.dispatch_id == id)
    }
    pub fn find_mut(&mut self, id: DispatchId) -> Option<&mut DispatchFrame> {
        self.frames.iter_mut().find(|f| f.dispatch_id == id)
    }
}
