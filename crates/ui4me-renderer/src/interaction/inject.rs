//! owner: 012-interaction — created by 001 slice (R-28)
//!
//! Window-free input injection (R-05; 012 `input-injection.md` §1–§2). The item takes the
//! same path as native input (queue → sequence → COMMIT); every dispatch it causes carries
//! origin `injected` and `isTrusted = true`. The owner-thread check (`WRONG_THREAD`) is the
//! runtime's (004), made before it calls in.

use crate::interaction::dispatch::{DispatchHost, eng, is_closing, rt, status};
use crate::interaction::input_queue::{InputItem, InputKind, InputOrigin, drain_input};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::status::Status;

fn invalid(msg: impl Into<String>) -> Status {
    status(Code::InvalidArgument).with_message(msg)
}

/// Item validation (§1): nothing is dispatched for a rejected item.
pub(crate) fn validate(item: &InputItem) -> Result<(), Status> {
    if item.kind.is_window_only() {
        return Err(invalid(format!(
            "{} is a window-only input kind",
            item.kind.name()
        )));
    }
    if item.timestamp_ms.is_some_and(|t| !t.is_finite()) {
        return Err(invalid("non-finite timestamp"));
    }
    let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
    match &item.kind {
        InputKind::PointerMove(p)
        | InputKind::PointerDown(p)
        | InputKind::PointerUp(p)
        | InputKind::PointerEnter(p)
        | InputKind::PointerLeave(p)
        | InputKind::PointerCancel(p) => {
            if !finite(&[p.x, p.y]) || !p.pressure.is_finite() {
                return Err(invalid("non-finite pointer coordinate"));
            }
        }
        InputKind::Wheel {
            x,
            y,
            delta_x,
            delta_y,
        } => {
            if !finite(&[*x, *y, *delta_x, *delta_y]) {
                return Err(invalid("non-finite wheel value"));
            }
        }
        InputKind::KeyDown(k) | InputKind::KeyUp(k) => {
            if k.key.is_empty() {
                return Err(invalid("empty key"));
            }
            if k.location > 3 {
                return Err(invalid("key location outside 0..3"));
            }
        }
        InputKind::ImePreedit {
            text,
            cursor: Some((start, end)),
        } => {
            let len = text.encode_utf16().count() as u32;
            if start > end || *end > len {
                return Err(invalid("IME cursor outside the preedit text"));
            }
        }
        _ => {}
    }
    Ok(())
}

/// `inject(document, item)`. Outside any operation the item (after any earlier queued input,
/// FIFO) runs to its COMMIT before this returns; inside a callback it is queued and `Ok`
/// returns at once (FR-013).
///
/// The item's `origin` is set to `injected` whatever the caller passed: injection is the only
/// producer of injected items, and native items come from 011 through `queue_input` (§1).
pub fn inject(host: &mut dyn DispatchHost, mut item: InputItem) -> Result<(), Status> {
    validate(&item)?;
    item.origin = InputOrigin::Injected;
    let doc = item.document;
    let seq = item.seq;
    eng(host, doc, |ix, d| {
        if is_closing(d) {
            return Err(status(Code::DocumentClosed));
        }
        let s = ix.doc_state(doc)?;
        if let Some(n) = seq {
            if s.last_seq.is_some_and(|last| n <= last) {
                return Err(invalid("non-increasing seq"));
            }
            s.last_seq = Some(n);
        }
        Ok(())
    })??;
    let inside = rt(host, |ix| {
        let inside = ix.in_operation();
        ix.queue_input(item);
        inside
    });
    if !inside {
        drain_input(host);
    }
    Ok(())
}
