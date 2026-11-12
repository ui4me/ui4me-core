//! owner: 012-interaction — L-ANIM-r5 (FR-034; `contracts/interfaces.md` element-event queue;
//! `contracts/event-types.md` `TransitionEvent` and `AnimationEvent`).
//!
//! Queued element events: events that other features generate during an update (006 transition
//! and animation events here; later link and image `load`/`error`) are queued by their producer
//! and dispatched on the owner thread in turn step 4, in queue order, never during a style,
//! layout or paint update and never inside another dispatch. Each is one outermost operation
//! (trusted, origin `element-task`) and so ends in its own COMMIT (dispatch-state-machine §1).
//! None is dispatched for a `Closing` or `Closed` document: the build step refuses it.

use crate::interaction::dispatch::{DispatchHost, DispatchOutcome, operation, run_dispatch};
use crate::interaction::frame::{EventInit, EventInterface};
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// The init of a CSS transition event (`transition` = true: `propertyName` is `name`) or CSS
/// animation event (`animationName`). All eight bubble and none is cancelable; `elapsed_time` is in
/// seconds and `pseudo_element` is `""`, `"::before"` or `"::after"`.
pub fn animation_event_init(
    event_type: &str,
    transition: bool,
    name: &str,
    elapsed_time: f64,
    pseudo_element: &str,
) -> EventInit {
    let mut e = EventInit::new(event_type)
        .interface(if transition {
            EventInterface::TransitionEvent
        } else {
            EventInterface::AnimationEvent
        })
        .bubbles(true)
        .cancelable(false);
    if transition {
        e.data.property_name = Some(name.to_string());
    } else {
        e.data.animation_name = Some(name.to_string());
    }
    e.data.elapsed_time = Some(elapsed_time);
    e.data.pseudo_element = Some(pseudo_element.to_string());
    e
}

/// Dispatch one queued element event (turn step 4): trusted, origin `element-task`, one COMMIT.
/// An event for an element that is gone, or for a closing document, is refused with the error
/// the dispatch core returns (the caller drops it).
pub fn dispatch_element_task(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    init: EventInit,
) -> Result<DispatchOutcome, Status> {
    operation(host, |h| {
        run_dispatch(
            h,
            doc,
            target,
            init,
            true,
            DispatchOrigin::ElementTask,
            None,
        )
    })
}
