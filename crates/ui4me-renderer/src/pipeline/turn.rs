//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! One owner-thread turn (005 recompute-plan §6, authoritative for the order; R-26). 004 owns
//! the event loop and calls [`run_turn`] from its idle hook with its own [`TurnHooks`]; this
//! module fixes the order and the frame-hook contents ([`DocumentPipeline::frame_hook`]).
//! Implemented by 001 T050.

use crate::pipeline::frame::FrameOutput;
use crate::pipeline::state::{DocumentPipeline, Trigger};
use ui4me_engine_types::status::Status;

/// The steps of one turn, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TurnStep {
    /// 1. Posted commands and native input, FIFO; each outermost dispatch ends in COMMIT.
    DrainInput,
    /// 2. Due host timers, then the animation tick if due.
    Timers,
    /// 3. Resource and font completions (at most one epoch increment each per batch).
    Completions,
    /// 4. Queued element events, FIFO.
    QueuedElementEvents,
    /// 5. Frames for visible, non-closing windows that need one (at most one per window).
    Frames,
    /// 6. Compute the next wake-up.
    Sleep,
}

impl TurnStep {
    /// R-26 order.
    pub const ORDER: [TurnStep; 6] = [
        TurnStep::DrainInput,
        TurnStep::Timers,
        TurnStep::Completions,
        TurnStep::QueuedElementEvents,
        TurnStep::Frames,
        TurnStep::Sleep,
    ];
}

/// When the event loop wakes next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wake {
    /// `ControlFlow::Wait`: no timer and no tick (idle CPU is zero).
    Indefinitely,
    /// Milliseconds from now.
    After(f64),
}

/// The event loop's side of one turn (004 implements it).
pub trait TurnHooks {
    fn drain_input(&mut self);
    fn fire_timers(&mut self);
    fn apply_completions(&mut self);
    fn dispatch_queued_element_events(&mut self);
    /// Step 5; headless documents skip it (FR-049).
    fn frames(&mut self);
    /// The next timer deadline, if any (the clock never ticks in the slice).
    fn next_timer_ms(&mut self) -> Option<f64>;
}

/// Run one turn in the R-26 order; returns when to wake next.
pub fn run_turn(hooks: &mut impl TurnHooks) -> Wake {
    for step in TurnStep::ORDER {
        match step {
            TurnStep::DrainInput => hooks.drain_input(),
            TurnStep::Timers => hooks.fire_timers(),
            TurnStep::Completions => hooks.apply_completions(),
            TurnStep::QueuedElementEvents => hooks.dispatch_queued_element_events(),
            TurnStep::Frames => hooks.frames(),
            TurnStep::Sleep => {}
        }
    }
    match hooks.next_timer_ms() {
        Some(ms) => Wake::After(ms.max(0.0)),
        None => Wake::Indefinitely,
    }
}

impl DocumentPipeline {
    /// The frame hook for one visible window (step 5): `None` when the document is closing or
    /// no frame is produced; otherwise `TRIGGER(frame)` and, if the result is `Clean`, raster
    /// the committed scene with full-surface damage and store the receipt.
    pub fn frame_hook(&mut self) -> Option<Result<FrameOutput, Status>> {
        if self.is_closing() {
            return None;
        }
        if let Err(e) = self.trigger(Trigger::Frame) {
            return Some(Err(e));
        }
        if self.state() != crate::pipeline::state::PipelineState::Clean {
            // FailedUpdate without retry_allowed: no frame (no busy retry, FR-038).
            return None;
        }
        Some(self.raster_committed())
    }
}

// ---- step 5: frame production (T075) ---------------------------------------------------------------

/// What the frame step reads about one window (011's window state, R-33).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowFacts {
    pub visible: bool,
    /// The window was exposed since its last frame.
    pub exposed: bool,
    /// The window was resized or rescaled since its last frame.
    pub resized_or_rescaled: bool,
}

/// One window with a document, as the frame step sees it. A document without a window is
/// headless (R-17) and is not a frame target.
pub struct FrameTarget<'a> {
    /// The window ordinal.
    pub window: u32,
    pub pipeline: &'a mut DocumentPipeline,
    pub facts: WindowFacts,
}

/// The outcome of the frame step for one window.
#[derive(Debug)]
pub struct FrameResult {
    pub window: u32,
    /// `None`: no frame was needed or produced.
    pub frame: Option<Result<FrameOutput, Status>>,
}

impl DocumentPipeline {
    /// Whether a visible window showing this document needs a frame: the document is not
    /// `Closing`/`Closed` and is `Dirty` (or retrying after input in `FailedUpdate`), ticking,
    /// exposed, or resized/rescaled. A hidden window needs none.
    pub fn frame_needed(&self, facts: WindowFacts) -> bool {
        if self.is_closing() || !facts.visible {
            return false;
        }
        let state = self.state();
        let dirty = match state {
            crate::pipeline::state::PipelineState::Dirty => true,
            crate::pipeline::state::PipelineState::FailedUpdate => self.machine().retry_allowed(),
            _ => false,
        };
        dirty || self.clock().needs_tick() || facts.exposed || facts.resized_or_rescaled
    }
}

/// Step 5 of the idle turn: for each target window that needs a frame, `TRIGGER(frame)` and, if
/// the result is `Clean`, raster the **committed** set with full-surface damage and push the
/// receipt. At most one frame per window per turn (a window appearing twice in `targets` is
/// served once). Presentation (011) follows in the caller, which writes its status back through
/// [`DocumentPipeline::record_presentation`].
pub fn run_frames(targets: &mut [FrameTarget<'_>]) -> Vec<FrameResult> {
    let mut served: Vec<u32> = Vec::new();
    let mut out = Vec::with_capacity(targets.len());
    for t in targets.iter_mut() {
        if served.contains(&t.window) || !t.pipeline.frame_needed(t.facts) {
            out.push(FrameResult {
                window: t.window,
                frame: None,
            });
            continue;
        }
        served.push(t.window);
        out.push(FrameResult {
            window: t.window,
            frame: t.pipeline.frame_hook(),
        });
    }
    out
}

// ---- counters (T076) --------------------------------------------------------------------------------

/// The event-loop counters of the `performance` probe (FR-077): every owner-thread turn counts,
/// including turns that do no work; `event_loop_wakes` counts OS wake-ups and stays 0 after
/// creation (the loop waits indefinitely when idle).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TurnCounters {
    pub idle_turns: u64,
    pub event_loop_wakes: u64,
}

impl TurnCounters {
    /// Run one turn and count it.
    pub fn turn(&mut self, hooks: &mut impl TurnHooks) -> Wake {
        self.idle_turns += 1;
        run_turn(hooks)
    }

    /// Called by the event loop for every OS wake-up (004's loop counter).
    pub fn wake(&mut self) {
        self.event_loop_wakes += 1;
    }
}
