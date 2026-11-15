//! R3 / F-02 follow-up: one GUI turn may queue thousands of DOM events (a colour-scheme switch with
//! transitions on many controls). The per-turn queue must hold a 10,000-event burst, and any
//! refusal beyond its cap must be counted, never silent.
use super::*;

fn burst(d: &mut Document, moves: usize) {
    for i in 0..moves {
        d.focus(if i % 2 == 0 { "a" } else { "b" }).unwrap();
    }
}

#[test]
fn ten_thousand_events_in_one_turn_are_all_delivered() {
    let mut d =
        Document::parse("<!doctype html><button id=a>A</button><button id=b>B</button>").unwrap();
    d.render(300, 100, 1.0).unwrap();
    d.drain_events();
    // Each focus move queues blur, focusout, focus and focusin.
    burst(&mut d, 2_600);
    let events = d.drain_events();
    assert!(
        events.len() >= 10_000,
        "only {} events delivered",
        events.len()
    );
    assert_eq!(d.dropped_event_count(), 0);
}

#[test]
fn refusals_beyond_the_cap_are_counted() {
    let mut d =
        Document::parse("<!doctype html><button id=a>A</button><button id=b>B</button>").unwrap();
    d.render(300, 100, 1.0).unwrap();
    d.drain_events();
    burst(&mut d, MAX_EVENTS / 2);
    let delivered = d.drain_events().len() as u64;
    assert!(delivered <= MAX_EVENTS as u64);
    assert!(d.dropped_event_count() > 0);
    // Delivered plus refused equals everything produced: 4 per move, except the first move,
    // which has no previously focused element to blur.
    assert_eq!(
        delivered + d.dropped_event_count(),
        4 * (MAX_EVENTS as u64 / 2) - 2
    );
}
