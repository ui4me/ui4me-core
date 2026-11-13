//! owner: 012-interaction — accessibility action entry points (lane L-A11Y-b1, beta B3; additive)
//!
//! What an assistive technology asks of an element (013 `contracts/actions.md` §1), executed as the
//! equivalent user input: every dispatch is trusted and carries origin `accessibility`; each entry
//! is one outermost operation (COMMIT, then queued input drains), so it is never run inside an
//! in-progress dispatch by its caller (the runtime calls it in turn step 1, on the owner thread).
//!
//! - [`focus`]: the focusing steps as for sequential navigation (focus-visible).
//! - [`activate`] (Default/Click): an `option` is picked in its select (`input`, `change`);
//!   otherwise focus the element when it is a focusable area, then a trusted
//!   `click` (PointerEvent) at the given point with activation behaviour (checkbox toggle, submit,
//!   label forwarding, ...), so the application's listeners run as for a real click.
//! - [`set_text_value`]: focus the text control, then replace its whole value through the editing
//!   path (`beforeinput` / `input` with inputType `insertReplacementText`), then `change`.
//! - [`set_number`] / [`step`]: a range or number input takes a value / steps by one, then
//!   `input` and `change`.

use crate::interaction::activation::is_disabled_form_control;
use crate::interaction::dispatch::{
    DispatchHost, eng, is_closing, operation, rt, run_dispatch, status,
};
use crate::interaction::focus::{FocusSource, focus_update, is_focusable};
use crate::interaction::frame::{EventInit, EventInterface};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

const ORIGIN: DispatchOrigin = DispatchOrigin::Accessibility;

fn check(host: &mut dyn DispatchHost, doc: DocumentId, el: NodeId) -> Result<(), Status> {
    eng(host, doc, |_, d| {
        if !d.arena().contains(el) {
            return Err(
                status(Code::StaleHandle).with_message("the accessible node no longer exists")
            );
        }
        if is_closing(d) {
            return Err(status(Code::DocumentClosed));
        }
        Ok(())
    })?
}

fn unsupported(m: &str) -> Status {
    status(Code::InvalidState).with_message(format!("unsupported accessible action: {m}"))
}

/// Focus: the focusing steps (focus-visible, as Tab). `Err` when `el` is not a focusable area.
pub fn focus(host: &mut dyn DispatchHost, doc: DocumentId, el: NodeId) -> Result<(), Status> {
    operation(host, |h| {
        check(h, doc, el)?;
        if !eng(h, doc, |_, d| is_focusable(d, el))? {
            return Err(status(Code::FocusMoveRejected).with_message("not a focusable area"));
        }
        focus_update(h, doc, Some(el), ORIGIN, FocusSource::Keyboard)
    })
}

/// Blur: the unfocusing steps when `el` is focused.
pub fn blur(host: &mut dyn DispatchHost, doc: DocumentId, el: NodeId) -> Result<(), Status> {
    operation(host, |h| {
        if rt(h, |ix| ix.focused_element(doc)) != Some(el) {
            return Ok(());
        }
        focus_update(h, doc, None, ORIGIN, FocusSource::Keyboard)
    })
}

/// Default action: focus when focusable, then a trusted `click` at `point` (viewport CSS px; the
/// node's centre). Nothing for a disabled form control.
pub fn activate(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    el: NodeId,
    point: Option<(f64, f64)>,
) -> Result<(), Status> {
    operation(host, |h| {
        check(h, doc, el)?;
        let (disabled, focusable) = eng(h, doc, |_, d| {
            (is_disabled_form_control(d, el), is_focusable(d, el))
        })?;
        if disabled {
            return Err(unsupported("the control is disabled"));
        }
        // An option of a dropdown has no box to click: pick it like the popup would.
        if crate::interaction::controls::assistive_pick_option(h, doc, el, ORIGIN)?.is_some() {
            return Ok(());
        }
        if focusable && rt(h, |ix| ix.focused_element(doc)) != Some(el) {
            focus_update(h, doc, Some(el), ORIGIN, FocusSource::Keyboard)?;
        }
        if !eng(h, doc, |_, d| d.arena().contains(el))? {
            return Ok(());
        }
        if !rt(h, |ix| ix.click_in_progress.insert((doc, el))) {
            return Ok(());
        }
        let (x, y) = point.unwrap_or((0.0, 0.0));
        let init = EventInit::mouse("click", EventInterface::PointerEvent, x, y, 0, 0);
        let r = run_dispatch(h, doc, el, init, true, ORIGIN, None);
        rt(h, |ix| ix.click_in_progress.remove(&(doc, el)));
        r.map(|_| ())
    })
}

/// SetValue on a text control: focus it when not focused, then replace the whole value as one
/// edit (`insertReplacementText`), then `change`. Returns whether the value changed.
pub fn set_text_value(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    el: NodeId,
    value: &str,
) -> Result<bool, Status> {
    operation(host, |h| {
        check(h, doc, el)?;
        let ok = eng(h, doc, |_, d| {
            crate::interaction::editing::text_kind(d, el).is_some()
                && !is_disabled_form_control(d, el)
        })?;
        if !ok {
            return Err(unsupported("not an editable text control"));
        }
        if rt(h, |ix| ix.focused_element(doc)) != Some(el)
            && eng(h, doc, |_, d| is_focusable(d, el))?
        {
            focus_update(h, doc, Some(el), ORIGIN, FocusSource::Keyboard)?;
        }
        crate::interaction::editing::replace_all(h, doc, el, value, ORIGIN)
    })
}

/// The text control `el` accepts an assistive edit: focus it first when it is not focused.
fn text_target(host: &mut dyn DispatchHost, doc: DocumentId, el: NodeId) -> Result<(), Status> {
    check(host, doc, el)?;
    let ok = eng(host, doc, |_, d| {
        crate::interaction::editing::text_kind(d, el).is_some() && !is_disabled_form_control(d, el)
    })?;
    if !ok {
        return Err(unsupported("not an editable text control"));
    }
    if rt(host, |ix| ix.focused_element(doc)) != Some(el)
        && eng(host, doc, |_, d| is_focusable(d, el))?
    {
        focus_update(host, doc, Some(el), ORIGIN, FocusSource::Keyboard)?;
    }
    Ok(())
}

/// SetTextSelection on a text control (0.5 F0504): focus it when not focused, then place the
/// caret / selection at UTF-16 offsets `anchor`..`focus` (clamped), ending any composition.
pub fn set_selection(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    el: NodeId,
    anchor: u32,
    focus: u32,
) -> Result<(), Status> {
    operation(host, |h| {
        text_target(h, doc, el)?;
        crate::interaction::editing::assistive_select(h, doc, el, anchor, focus, ORIGIN)
    })
}

/// ReplaceSelectedText on a text control (0.5 F0504): replace the selection (insert at the caret)
/// as one edit (`insertReplacementText`). Returns whether the value changed.
pub fn replace_selection(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    el: NodeId,
    text: &str,
) -> Result<bool, Status> {
    operation(host, |h| {
        text_target(h, doc, el)?;
        crate::interaction::editing::assistive_replace(h, doc, el, text, ORIGIN)
    })
}

/// SetValue (numeric) on a range or number input: sanitised, then `input` and `change`.
pub fn set_number(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    el: NodeId,
    value: f64,
) -> Result<bool, Status> {
    operation(host, |h| {
        check(h, doc, el)?;
        crate::interaction::controls::assistive_set_number(h, doc, el, Some(value), 0, ORIGIN)
    })
}

/// Increment (`dir > 0`) or Decrement (`dir < 0`) a range or number input by one step.
pub fn step(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    el: NodeId,
    dir: i32,
) -> Result<bool, Status> {
    operation(host, |h| {
        check(h, doc, el)?;
        crate::interaction::controls::assistive_set_number(h, doc, el, None, dir.signum(), ORIGIN)
    })
}
