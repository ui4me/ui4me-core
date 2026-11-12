//! owner: 012-interaction — created by 001 slice (R-28)
//!
//! NativeInputQueue (001 T059; 012 `dispatch-state-machine.md` §7): native, injected and
//! accessibility input items, FIFO. An item is processed only while the frame stack is empty and
//! no outermost operation is running; each item's sequence and its COMMIT complete before the
//! next item starts (005 §6, R-26). Items for a `Closing`/`Closed` document are dropped and
//! counted (invariant 8).
//!
//! The item types are 011's normalised input (`ui4me-core/src/input.rs`,
//! `input-normalization.md` §1–§2), re-exported here (001 T063).

use crate::interaction::dispatch::{DispatchHost, eng, is_closing, operation, rt};
use std::collections::VecDeque;
use ui4me_engine_types::ids::DocumentId;

pub use crate::interaction::frame::Modifiers;

/// 011's normalised input types (`ui4me-core/src/input.rs`, 011 `input-normalization.md`
/// §1–§2); this queue's item is their envelope with `target = Document`.
pub use ui4me_engine_types::input::{InputKind, InputOrigin, KeyData, PointerData};

/// One input item: 011's envelope with `target = Document`.
#[derive(Clone, Debug, PartialEq)]
pub struct InputItem {
    pub document: DocumentId,
    /// Drives `timeStamp`; `None` = assigned from the host clock.
    pub timestamp_ms: Option<f64>,
    /// Strictly increasing per document when supplied.
    pub seq: Option<u64>,
    pub origin: InputOrigin,
    pub modifiers: Modifiers,
    pub kind: InputKind,
}

impl InputItem {
    pub fn new(document: DocumentId, origin: InputOrigin, kind: InputKind) -> Self {
        InputItem {
            document,
            timestamp_ms: None,
            seq: None,
            origin,
            modifiers: Modifiers::default(),
            kind,
        }
    }
    /// An injected item.
    pub fn injected(document: DocumentId, kind: InputKind) -> Self {
        InputItem::new(document, InputOrigin::Injected, kind)
    }
    pub fn at(mut self, timestamp_ms: f64) -> Self {
        self.timestamp_ms = Some(timestamp_ms);
        self
    }
    pub fn seq(mut self, seq: u64) -> Self {
        self.seq = Some(seq);
        self
    }
    /// Injected primary-button press at `(x, y)`.
    pub fn pointer_down(document: DocumentId, x: f64, y: f64) -> Self {
        InputItem::injected(
            document,
            InputKind::PointerDown(PointerData::mouse(x, y, 0)),
        )
    }
    /// Injected primary-button release at `(x, y)`.
    pub fn pointer_up(document: DocumentId, x: f64, y: f64) -> Self {
        let mut p = PointerData::mouse(x, y, 0);
        p.buttons = 0;
        InputItem::injected(document, InputKind::PointerUp(p))
    }
}

/// FIFO of pending input items (runtime-wide; items carry their document).
#[derive(Debug, Default)]
pub struct NativeInputQueue {
    items: VecDeque<InputItem>,
    dropped_closing: u64,
}

impl NativeInputQueue {
    pub fn push(&mut self, item: InputItem) {
        self.items.push_back(item);
    }
    pub(crate) fn pop(&mut self) -> Option<InputItem> {
        self.items.pop_front()
    }
    pub(crate) fn note_dropped(&mut self) {
        self.dropped_closing += 1;
    }
    /// Remove the items of one document (detach/destroy); returns how many were removed.
    pub(crate) fn remove_document(&mut self, doc: DocumentId) -> usize {
        let before = self.items.len();
        self.items.retain(|i| i.document != doc);
        before - self.items.len()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// Items dropped because their document was `Closing`/`Closed`.
    pub fn dropped_closing(&self) -> u64 {
        self.dropped_closing
    }
}

fn process_item(host: &mut dyn DispatchHost, item: InputItem) {
    let doc = item.document;
    let live = eng(host, doc, |ix, d| !is_closing(d) && ix.is_attached(doc)).unwrap_or(false);
    if !live {
        // No input item is dispatched for a document after it entered `Closing` (invariant 8).
        rt(host, |ix| ix.queue.note_dropped());
        return;
    }
    operation(host, |h| {
        let _ = crate::interaction::sequences::run_item(h, &item);
    });
}

fn next_item(host: &mut dyn DispatchHost) -> Option<InputItem> {
    rt(host, |ix| {
        if ix.in_operation() {
            None
        } else {
            ix.queue.pop()
        }
    })
}

/// Turn step 1 (005 §6, R-26): process queued items FIFO until the queue is empty, each with
/// its sequence and COMMIT, only while nothing is running. Returns the number processed.
pub fn drain_input(host: &mut dyn DispatchHost) -> usize {
    let start = rt(host, |ix| !std::mem::replace(&mut ix.draining, true));
    if !start {
        return 0;
    }
    let mut n = 0;
    while let Some(item) = next_item(host) {
        process_item(host, item);
        n += 1;
    }
    rt(host, |ix| ix.draining = false);
    n
}

/// Process at most one queued item (only with an empty stack). `true` if one was processed.
pub fn process_next_input(host: &mut dyn DispatchHost) -> bool {
    let start = rt(host, |ix| !std::mem::replace(&mut ix.draining, true));
    if !start {
        return false;
    }
    let item = next_item(host);
    let some = item.is_some();
    if let Some(item) = item {
        process_item(host, item);
    }
    rt(host, |ix| ix.draining = false);
    some
}
