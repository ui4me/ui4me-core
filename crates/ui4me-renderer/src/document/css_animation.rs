//! CSS transitions and keyframe animations of a document (0.9 F0901, F0902).
//!
//! The style pass (Stylo) starts and cancels transitions and `@keyframes` animations; the
//! document clock ([`SharedClock`](crate::pipeline::clock::SharedClock), the same one smooth
//! scrolling reads) advances them: [`Document::poll_resources`] and
//! [`Document::advance_clock`] call [`Document::tick_animations`], which moves the animation
//! states, queues the events and marks the animated elements for the next update. The host
//! asks for a wake-up only while something runs (`next_wake_in`, one frame of ~16 ms, so at
//! most about 60 Hz even for an `infinite` animation); an idle document has no deadline. An
//! element that is removed or becomes `display: none` loses its animations at the next style
//! pass. A closed window drops the document and with it every animation.
//!
//! Events (`transitionrun`, `transitionstart`, `transitionend`, `transitioncancel`,
//! `animationstart`, `animationiteration`, `animationend`, `animationcancel`) are dispatched
//! as element tasks when the host drains events, in the order the engine generated them, and
//! reach the host with `property_name` or `animation_name`, `elapsed_time` and
//! `pseudo_element`. No event is delivered for an element that was removed.
//!
//! Reduced motion: the app environment (`app_reduced_motion`) and the OS signal drive the
//! `prefers-reduced-motion` media query. Core does not suppress author animations by itself;
//! a style sheet opts out with `@media (prefers-reduced-motion: reduce) { ... animation: none;
//! transition: none }`.
//!
//! Not supported (reported, left to the profile): `animation-timeline` and `animation-range*`
//! (scroll-driven animations), view transitions.

use super::*;
use crate::interaction::element_events::{animation_event_init, dispatch_element_task};

/// Events handled per drain; the rest of a burst is dropped (a page cannot grow the queue).
const MAX_ANIMATION_EVENTS: usize = 512;

impl Document {
    /// Run the CSS animation tick on the document clock. Returns whether a tick ran (the
    /// animated elements are then restyled by the next update and a frame follows).
    pub(super) fn tick_animations(&mut self, force: bool) -> bool {
        let ticked = self.pipeline.tick_to_clock(force);
        if ticked {
            self.repaint = true;
        }
        ticked
    }

    /// Dispatch the queued transition and animation events (element tasks, trusted).
    pub(super) fn dispatch_animation_events(&mut self) {
        let events = self.pipeline.take_animation_events();
        if events.is_empty() {
            return;
        }
        let id = self.id();
        for e in events.into_iter().take(MAX_ANIMATION_EVENTS) {
            let init = animation_event_init(
                e.kind.event_type(),
                e.kind.is_transition(),
                &e.name,
                e.elapsed_time,
                e.pseudo_element,
            );
            let _ = dispatch_element_task(self, id, e.node, init);
        }
    }

    /// Is any transition or animation in flight (a frame is wanted)?
    pub fn is_css_animating(&self) -> bool {
        self.pipeline.next_animation_wake_ms().is_some()
    }
}
