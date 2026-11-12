//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28); the tick procedure is T087
//! (L-ANIM-r5).
//!
//! Animation clock (005 recompute-plan §7; AD-11). Two modes, chosen at runtime creation:
//!
//! * **Manual** (tests and fixtures): time moves only with [`DocumentPipeline::advance_clock`] and
//!   [`DocumentPipeline::finish_all_animations`]; each call is one tick (virtual time, FR-065).
//! * **Realtime**: the runtime samples monotonic time once per turn
//!   ([`DocumentPipeline::begin_turn`]) and, in turn step 2, runs the tick when the clock is
//!   `Ticking` and 16 ms have passed ([`DocumentPipeline::animation_tick_step`]). The clock is
//!   `Ticking` exactly while 006 reports a pending or running transition or animation
//!   (`needs_animation_ticks`); otherwise it is `Stopped`: no ticks, no deadline, no wake-ups.
//!
//! A tick advances 006's animation states at the clock time (`StyleBridge::tick_animations`),
//! which also queues the transition and animation events for 012 (turn step 4) and marks every
//! element whose value changes. The pipeline then notes `ANIMATION_TICK` for those elements (the
//! document becomes `Dirty`, the next update re-samples their values and a frame follows) and
//! advances the `animation` input epoch. Time is constant within a turn and within an update.

use crate::diagnostics::probes::runtime::{ActiveAnimation, AnimationClockInput, ClockState};
use crate::pipeline::domain::ClockMode;
use crate::pipeline::reasons::{Reason, Subject};
use crate::pipeline::state::DocumentPipeline;
use crate::style::animation::{AnimationEvent, TickOutcome};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// Tick target while ticking (recompute-plan §10).
pub const TICK_TARGET_MS: f64 = 16.0;

/// The one document clock (0.9 F0901): monotonic real time since the document opened, or a
/// manual time that only [`SharedClock::advance`] moves. Smooth scrolling and the style
/// pass's transitions and keyframe animations read the same clock, so a test that advances it
/// moves both. Clones share the state.
#[derive(Clone, Debug)]
pub struct SharedClock(Arc<Mutex<SharedClockState>>);

#[derive(Debug)]
struct SharedClockState {
    origin: Instant,
    manual: Option<f64>,
}

impl Default for SharedClock {
    fn default() -> Self {
        SharedClock(Arc::new(Mutex::new(SharedClockState {
            origin: Instant::now(),
            manual: None,
        })))
    }
}

impl PartialEq for SharedClock {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl SharedClock {
    fn state(&self) -> std::sync::MutexGuard<'_, SharedClockState> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Milliseconds on the document clock.
    pub fn now(&self) -> f64 {
        let s = self.state();
        s.manual
            .unwrap_or_else(|| s.origin.elapsed().as_secs_f64() * 1000.0)
    }
    pub fn is_manual(&self) -> bool {
        self.state().manual.is_some()
    }
    /// Freeze the clock at its present time; only [`Self::advance`] moves it afterwards.
    pub fn use_manual(&self) {
        let now = self.now();
        self.state().manual = Some(now);
    }
    /// Move a manual clock by `ms` (a real-time clock is unaffected).
    pub fn advance(&self, ms: f64) {
        if let Some(t) = self.state().manual.as_mut() {
            *t += ms.max(0.0);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationClock {
    mode: ClockMode,
    time_ms: f64,
    /// The `animation` input epoch: +1 per tick that inputs an animating element. Never
    /// advances while inert (no element animates).
    epoch: u64,
    /// Ticks so far: realtime ticks and manual `advance`/`finish_all` calls (each acts as one).
    ticks: u64,
    /// The time of the last realtime tick (the tick deadline is this + 16 ms).
    last_tick_ms: Option<f64>,
    /// 006 reports a pending or running transition or animation.
    active: bool,
    /// Realtime: the monotonic origin (time 0) and whether this turn already sampled it.
    origin: Instant,
    turn_open: bool,
    /// The document clock the pipeline follows (a `Document`); `None` for a bare pipeline.
    shared: Option<SharedClock>,
}

impl AnimationClock {
    /// Manual mode (tests and fixtures; 004 runtime-creation option).
    pub fn manual() -> AnimationClock {
        AnimationClock::with_mode(ClockMode::Manual)
    }

    pub fn realtime() -> AnimationClock {
        AnimationClock::with_mode(ClockMode::Realtime)
    }

    fn with_mode(mode: ClockMode) -> AnimationClock {
        AnimationClock {
            mode,
            time_ms: 0.0,
            epoch: 0,
            ticks: 0,
            last_tick_ms: None,
            active: false,
            origin: Instant::now(),
            turn_open: false,
            shared: None,
        }
    }

    pub fn new(mode: ClockMode) -> AnimationClock {
        AnimationClock::with_mode(mode)
    }

    pub fn mode(&self) -> ClockMode {
        self.mode
    }

    pub fn time_ms(&self) -> f64 {
        self.time_ms
    }

    /// `Manual` in manual mode; in realtime mode `Ticking` while an animation is active, else
    /// `Stopped`.
    pub fn state(&self) -> ClockState {
        if let Some(c) = &self.shared {
            return match (c.is_manual(), self.active) {
                (true, _) => ClockState::Manual,
                (false, true) => ClockState::Ticking,
                (false, false) => ClockState::Stopped,
            };
        }
        match self.mode {
            ClockMode::Manual => ClockState::Manual,
            ClockMode::Realtime if self.active => ClockState::Ticking,
            ClockMode::Realtime => ClockState::Stopped,
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Whether the event loop must schedule a tick: realtime and some animation is active.
    pub fn needs_tick(&self) -> bool {
        (self.mode == ClockMode::Realtime || self.shared.is_some()) && self.active
    }

    /// Follow the document clock `shared` (time is sampled from it before each update).
    pub fn follow(&mut self, shared: SharedClock) {
        self.time_ms = shared.now();
        self.shared = Some(shared);
    }

    /// 006 reports whether any transition or animation needs ticks.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        if !active {
            self.last_tick_ms = None;
        }
    }

    /// The next tick deadline in milliseconds from `now_ms`, or `None` when the clock is not
    /// `Ticking` (no wake-ups at all). Only a ticking realtime clock has a deadline.
    pub fn next_tick_deadline_ms(&self, now_ms: f64) -> Option<f64> {
        if !self.needs_tick() {
            return None;
        }
        let last = self.last_tick_ms.unwrap_or(now_ms - TICK_TARGET_MS);
        Some((last + TICK_TARGET_MS - now_ms).max(0.0))
    }

    /// Ticks so far (`animation_clock` probe).
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Monotonic milliseconds since the clock was created.
    pub fn monotonic_ms(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
    }

    /// Realtime: sample monotonic time **once** for the turn (it is constant for the rest of the
    /// turn). Manual: nothing.
    pub(crate) fn begin_turn(&mut self) {
        if self.mode == ClockMode::Realtime {
            self.time_ms = self.monotonic_ms();
            self.turn_open = true;
        }
    }

    pub(crate) fn end_turn(&mut self) {
        self.turn_open = false;
    }

    /// An update outside a turn (an API flush) samples time itself, unless the clock is mid-turn.
    pub(crate) fn ensure_sampled(&mut self) {
        if let Some(c) = &self.shared {
            self.time_ms = c.now();
            return;
        }
        if self.mode == ClockMode::Realtime && !self.turn_open && !self.active {
            self.time_ms = self.monotonic_ms();
        }
    }

    /// Whether a realtime tick is due at the current (turn-sampled) time.
    fn tick_due(&self) -> bool {
        self.needs_tick()
            && self
                .last_tick_ms
                .is_none_or(|l| self.time_ms - l >= TICK_TARGET_MS)
    }

    fn note_tick(&mut self) {
        self.last_tick_ms = Some(self.time_ms);
        self.ticks += 1;
    }

    /// `advance(ms)` (testing group, manual mode only; R-24). Acts as one tick: time moves.
    pub fn advance(&mut self, ms: f64) -> Result<(), Status> {
        if self.mode != ClockMode::Manual {
            return Err(Status::new(Code::InvalidState, Stage::Host)
                .with_message("advance() needs the manual clock"));
        }
        if !ms.is_finite() || ms < 0.0 {
            return Err(Status::new(Code::InvalidArgument, Stage::Host)
                .with_message(format!("advance({ms}) needs a finite, non-negative step")));
        }
        self.time_ms += ms;
        self.ticks += 1;
        Ok(())
    }

    /// `finish_all()`: moves time to `end_ms` (the latest end of all active sets) if that is later;
    /// acts as one tick.
    pub fn finish_all(&mut self, end_ms: Option<f64>) -> Result<(), Status> {
        if self.mode != ClockMode::Manual {
            return Err(Status::new(Code::InvalidState, Stage::Host)
                .with_message("finish_all() needs the manual clock"));
        }
        if let Some(end) = end_ms.filter(|e| e.is_finite() && *e > self.time_ms) {
            self.time_ms = end;
        }
        self.ticks += 1;
        Ok(())
    }
}

impl DocumentPipeline {
    /// Follow the document clock (0.9 F0901): updates sample it, and [`Self::tick_to_clock`]
    /// runs the animation tick.
    pub fn follow_clock(&mut self, shared: SharedClock) {
        self.clock.follow(shared);
    }

    /// The document clock's tick (0.9 F0901): when a transition or animation is in flight, move
    /// the animation time to the clock and tick once. A real-time clock ticks at most every
    /// 16 ms unless `force`; a manual clock ticks when its time moved. Returns whether a tick ran.
    pub fn tick_to_clock(&mut self, force: bool) -> bool {
        let Some(shared) = self.clock.shared.clone() else {
            return false;
        };
        if self.is_closing() || !self.clock.active {
            return false;
        }
        let now = shared.now();
        let due = match self.clock.last_tick_ms {
            None => true,
            Some(last) if shared.is_manual() => now != last,
            Some(last) => force || now - last >= TICK_TARGET_MS - 1.0,
        };
        if !due {
            return false;
        }
        self.clock.time_ms = now;
        self.clock.note_tick();
        self.run_tick();
        true
    }

    /// Milliseconds until the next animation tick is due, or `None` while nothing animates (no
    /// wake-ups). A manual clock always answers one frame.
    pub fn next_animation_wake_ms(&self) -> Option<f64> {
        let shared = self.clock.shared.as_ref()?;
        if !self.clock.active {
            return None;
        }
        if shared.is_manual() {
            return Some(TICK_TARGET_MS);
        }
        self.clock.next_tick_deadline_ms(shared.now())
    }

    pub fn clock(&self) -> &AnimationClock {
        &self.clock
    }

    #[doc(hidden)]
    pub fn clock_mut_for_test(&mut self) -> &mut AnimationClock {
        &mut self.clock
    }

    /// Select the clock mode (runtime-creation option, immutable afterwards; 004).
    pub fn set_clock_mode(&mut self, mode: ClockMode) {
        self.clock = AnimationClock::new(mode);
    }

    /// The `animation_clock` probe input: mode, time, state and the transitions and animations
    /// in force.
    pub fn animation_clock_probe_input(&self) -> AnimationClockInput {
        let active = self
            .style
            .active_animations(&self.doc)
            .into_iter()
            .map(|a| ActiveAnimation {
                node: a.node,
                property: a.property,
                is_animation: a.is_animation,
                start_ms: a.start_ms,
                end_ms: a.end_ms,
            })
            .collect();
        AnimationClockInput {
            mode: self.clock.mode,
            time_ms: self.clock.time_ms,
            state: self.clock.state(),
            active,
        }
    }

    /// One tick at the clock's time: 006 advances the animation states and queues the events;
    /// every element whose value changes is an `ANIMATION_TICK` input.
    fn run_tick(&mut self) -> TickOutcome {
        self.style.set_animation_time_ms(self.clock.time_ms);
        let out = self.style.tick_animations(&self.doc);
        if !out.changed.is_empty() {
            self.clock.epoch += 1;
            for &n in &out.changed {
                self.dirty.note(Reason::AnimationTick, Subject::Node(n));
            }
            self.note_input();
        }
        self.clock.set_active(out.active);
        out
    }

    pub fn advance_clock(&mut self, ms: f64) -> Result<(), Status> {
        self.check_open()?;
        self.clock.advance(ms)?;
        self.run_tick();
        Ok(())
    }

    /// `finish_all()` (ABI testing group, manual mode, owner thread): moves the clock to the
    /// latest end of every finite transition and animation (an infinite animation has none and
    /// keeps running) and ticks once, so every one of them reaches its end state.
    pub fn finish_all_animations(&mut self) -> Result<(), Status> {
        self.check_open()?;
        let end = self.style.latest_animation_end_ms();
        self.clock.finish_all(end)?;
        self.run_tick();
        Ok(())
    }

    /// Test hook: cancel every animation with an infinite iteration count (it has no end state, so
    /// `finish_all_animations` leaves it running) and re-style its element; the reference page's
    /// `end-state` policy does the same (`Animation.cancel()`). Manual clock only.
    pub fn cancel_infinite_animations(&mut self) -> Result<(), Status> {
        self.check_open()?;
        let nodes = self.style.cancel_infinite_animations(&self.doc);
        if !nodes.is_empty() {
            self.clock.epoch += 1;
            for &n in &nodes {
                self.dirty.note(Reason::AnimationTick, Subject::Node(n));
            }
            self.note_input();
            let _ = self.run_tick();
        }
        Ok(())
    }

    /// Turn start (recompute-plan §6): a realtime clock samples monotonic time once; it is
    /// constant for the whole turn.
    pub fn begin_turn(&mut self) {
        self.clock.begin_turn();
    }

    pub fn end_turn(&mut self) {
        self.clock.end_turn();
    }

    /// Turn step 2, the animation tick if due (realtime only; a manual clock moves only through
    /// `advance`). Returns whether a tick ran.
    pub fn animation_tick_step(&mut self) -> bool {
        if self.is_closing() || !self.clock.tick_due() {
            return false;
        }
        self.clock.note_tick();
        self.run_tick();
        true
    }

    /// The transition and animation events queued since the last call, for 012 (turn step 4).
    /// Events of disconnected elements are dropped.
    pub fn take_animation_events(&mut self) -> Vec<AnimationEvent> {
        self.style.take_animation_events(&self.doc)
    }

    /// Updates that took the animation paint-only path (S5 over the committed layout).
    pub fn animation_paint_updates(&self) -> u64 {
        self.animation_paint_updates
    }

    /// Test hook: every animation tick takes the full rebuild (the differential tests compare it
    /// with the paint-only path).
    #[doc(hidden)]
    pub fn disable_animation_paint_path_for_testing(&mut self) {
        self.no_animation_paint_path = true;
    }

    /// Updates that reused the committed layout because the new box tree was equal to it.
    pub fn layout_reuses(&self) -> u64 {
        self.layout_reuses
    }

    /// Scene reuse on or off (015 PERF-2; off by default in 0.4, see `PERF-4`). Switching it off forgets what
    /// the last scene kept, so nothing is retained between frames.
    pub fn set_scene_reuse(&mut self, on: bool) {
        self.scene_reuse = on;
        if !on {
            *self.paint_styles.borrow_mut() = Default::default();
        }
    }

    pub fn scene_reuse(&self) -> bool {
        self.scene_reuse
    }

    /// What the last scene took over from the one before (015 PERF-2: element-box subtrees reused
    /// and built).
    pub fn scene_reuse_stats(&self) -> crate::scene::reuse::ReuseStats {
        self.paint_styles.borrow().memo_stats()
    }

    /// Test hook: every update lays out afresh, even when the box tree did not change.
    #[doc(hidden)]
    pub fn disable_layout_reuse_for_testing(&mut self) {
        self.no_layout_reuse = true;
    }

    /// Test hook: every frame is a full raster instead of a damage raster.
    #[doc(hidden)]
    pub fn disable_damage_raster_for_testing(&mut self) {
        self.no_damage_raster = true;
    }

    /// Whether transition or animation events are queued (the loop must not sleep).
    pub fn has_animation_events(&self) -> bool {
        self.style.pending_animation_events() > 0
    }

    /// An animating element was removed or freed: `ANIMATION_CANCEL` is bookkeeping only (no
    /// stage is dirtied), so it is recorded without making the document `Dirty`.
    pub fn note_animation_cancel(&mut self, element: ui4me_engine_types::ids::NodeId) {
        self.dirty.note(
            crate::pipeline::reasons::Reason::AnimationCancel,
            crate::pipeline::reasons::Subject::Node(element),
        );
    }
}
