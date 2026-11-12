//! Expand one normalized input item into document events and default actions.
//! The outer operation commits after all dispatch steps, so callbacks never see
//! a partially applied input sequence.
//!
//! Mouse presses update hover/active state and dispatch `mousedown`; a primary
//! press also moves focus and starts editing or control interaction. Releases
//! dispatch `mouseup` and activate a matching connected, enabled press target.
//! Pointer events (0.8 F0807): `pointermove`/`pointerdown`/`pointerup`/`pointercancel`
//! precede their compatibility mouse events (a canceled `pointerdown` suppresses `mousedown`);
//! pointer capture retargets the pointer's events to the capturing element until the release,
//! with `gotpointercapture`/`lostpointercapture` (a removed capture target loses it at the
//! Document). Matching releases emit click/auxclick, consecutive primary clicks emit dblclick,
//! and right-click or the keyboard context-menu command emits contextmenu.
//!
//! Keyboard events route to the focused element or document body, then apply
//! Tab navigation, control activation, editing and clipboard defaults. IME
//! composition owns its editing keys until the platform commits the text.

use crate::interaction::dispatch::{DispatchHost, eng, rt, run_dispatch};
use crate::interaction::focus::{FocusSource, click_focus_target, focus_update};
use crate::interaction::frame::{EventInit, EventInterface, Modifiers};
use crate::interaction::input_queue::{InputItem, InputKind, InputOrigin, KeyData, PointerData};
use crate::interaction::{keyboard, pointer};
use ui4me_document::document::Document;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// Per-document pointer state (press target for click matching).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointerState {
    pub press_target: Option<NodeId>,
    pub press_button: Option<i8>,
    last_click: Option<(NodeId, f64, f64, f64, i32)>,
}

/// Per-document pointer capture of the mouse pointer (`pointer_id` 1; 0.8 F0807). Capture
/// requests take effect at the pointer's next event (Pointer Events "process pending pointer
/// capture").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureState {
    pub target: Option<NodeId>,
    /// `Some(new)` = a pending change (`Some(None)` releases).
    pub pending: Option<Option<NodeId>>,
    /// The capture target was removed: `lostpointercapture` fires at the Document.
    pub lost_removed: bool,
}

fn pointer_event(ty: &str, p: &PointerData, m: Modifiers) -> EventInit {
    let cancelable = !matches!(
        ty,
        "pointercancel" | "gotpointercapture" | "lostpointercapture"
    );
    let button = if ty == "pointermove" || !cancelable {
        -1
    } else {
        p.button as i16
    };
    let mut e = EventInit::mouse(
        ty,
        EventInterface::PointerEvent,
        p.x,
        p.y,
        button,
        p.buttons,
    )
    .cancelable(cancelable);
    e.data.pointer_id = Some(p.pointer_id);
    e.data.pointer_type = Some(p.pointer_type.as_str().to_owned());
    e.data.is_primary = Some(p.is_primary);
    e.data.pressure = Some(if p.buttons == 0 { 0.0 } else { p.pressure });
    e.modifiers = m;
    e
}

/// Apply a pending capture change (and a removal loss) before the pointer's next event.
fn process_pending_capture(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<Option<NodeId>, Status> {
    let (old, pending, lost) = eng(host, doc, |ix, d| {
        let s = ix.doc_state(doc).ok()?;
        let c = std::mem::take(&mut s.capture);
        let live = |n: Option<NodeId>| n.filter(|&n| d.arena().is_connected(n));
        Some((live(c.target), c.pending.map(live), c.lost_removed))
    })?
    .unwrap_or((None, None, false));
    if lost {
        let root = eng(host, doc, |_, d| d.document_node())?;
        run_dispatch(
            host,
            doc,
            root,
            pointer_event("lostpointercapture", p, m),
            true,
            origin,
            ts,
        )?;
    }
    let new = pending.unwrap_or(old);
    if new != old {
        if let Some(o) = old {
            run_dispatch(
                host,
                doc,
                o,
                pointer_event("lostpointercapture", p, m),
                true,
                origin,
                ts,
            )?;
        }
        if let Some(n) = new {
            run_dispatch(
                host,
                doc,
                n,
                pointer_event("gotpointercapture", p, m),
                true,
                origin,
                ts,
            )?;
        }
    }
    eng(host, doc, |ix, _| {
        // A change a listener above requested stays pending for the next event.
        if let Ok(s) = ix.doc_state(doc) {
            s.capture.target = new;
        }
    })?;
    Ok(new)
}

/// The pointer's capture ends at its release (implicit release).
fn implicit_release(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    let old = eng(host, doc, |ix, d| {
        let s = ix.doc_state(doc).ok()?;
        let c = std::mem::take(&mut s.capture);
        c.target.filter(|&n| d.arena().is_connected(n))
    })?;
    if let Some(o) = old {
        run_dispatch(
            host,
            doc,
            o,
            pointer_event("lostpointercapture", p, m),
            true,
            origin,
            ts,
        )?;
    }
    Ok(())
}

fn origin_of(item: &InputItem) -> DispatchOrigin {
    match item.origin {
        InputOrigin::Native => DispatchOrigin::Native,
        InputOrigin::Injected => DispatchOrigin::Injected,
    }
}

/// The hit element, else the document element, else the Document.
fn hit(host: &mut dyn DispatchHost, doc: DocumentId, x: f64, y: f64) -> Result<NodeId, Status> {
    if let Some(n) = host.hit_test(doc, x, y)? {
        return Ok(n);
    }
    eng(host, doc, |_, d| {
        d.document_element().unwrap_or_else(|| d.document_node())
    })
}

fn mouse(
    ty: &str,
    interface: EventInterface,
    p: &PointerData,
    buttons: u16,
    m: Modifiers,
) -> EventInit {
    let mut e = EventInit::mouse(ty, interface, p.x, p.y, p.button as i16, buttons);
    e.detail = 1;
    e.modifiers = m;
    e
}

fn key(ty: &str, k: &KeyData, m: Modifiers) -> EventInit {
    let mut e = EventInit::new(ty)
        .interface(EventInterface::KeyboardEvent)
        .bubbles(true)
        .cancelable(true);
    e.data.key = Some(k.key.clone());
    e.data.code = Some(k.code.clone());
    e.data.repeat = Some(k.repeat);
    if k.is_composing {
        e.data.is_composing = Some(true);
    }
    e.modifiers = m;
    e
}

/// The focused element, else the body (else the document element, else the Document).
fn key_target(host: &mut dyn DispatchHost, doc: DocumentId) -> Result<NodeId, Status> {
    match rt(host, |ix| ix.focused_element(doc)) {
        Some(f) => Ok(f),
        None => eng(host, doc, |_, d| {
            d.body()
                .or(d.document_element())
                .unwrap_or_else(|| d.document_node())
        }),
    }
}

/// A text control that accepts typed text (HTML: text-like `input` types, `number` and
/// `textarea`; not disabled or read-only; connected).
pub fn is_text_control(d: &Document, n: NodeId) -> bool {
    d.arena().is_connected(n)
        && d.control_facts(n)
            .is_some_and(|facts| facts.accepts_text() && !facts.disabled && !facts.read_only)
}

/// Run one item's sequence (inside the item's outermost operation), then the `scroll` events it
/// caused (FR-088: once per target).
pub(crate) fn run_item(host: &mut dyn DispatchHost, item: &InputItem) -> Result<(), Status> {
    let r = run_item_inner(host, item);
    if matches!(
        item.kind,
        InputKind::PointerMove(_)
            | InputKind::PointerDown(_)
            | InputKind::PointerUp(_)
            | InputKind::PointerLeave(_)
            | InputKind::PointerCancel(_)
            | InputKind::Wheel { .. }
    ) {
        // A failed style read leaves the cursor as it is.
        let _ = pointer::update_cursor(host, item.document);
    }
    crate::interaction::scroll::flush_scroll_events(
        host,
        item.document,
        origin_of(item),
        item.timestamp_ms,
    );
    r
}

fn run_item_inner(host: &mut dyn DispatchHost, item: &InputItem) -> Result<(), Status> {
    let doc = item.document;
    let origin = origin_of(item);
    let ts = item.timestamp_ms;
    match &item.kind {
        InputKind::PointerMove(p) => {
            if crate::interaction::scroll::scrollbar_move(host, doc, p, false, origin, ts)? {
                return Ok(());
            }
            let captured = process_pending_capture(host, doc, p, item.modifiers, origin, ts)?;
            let target = match captured {
                Some(c) => c,
                None => hit(host, doc, p.x, p.y)?,
            };
            run_dispatch(
                host,
                doc,
                target,
                pointer_event("pointermove", p, item.modifiers),
                true,
                origin,
                ts,
            )?;
            pointer::on_move(host, doc, p, captured, item.modifiers, origin, ts)?;
            crate::interaction::editing::on_pointer_move(host, doc, p, item.modifiers, origin, ts)?;
            crate::interaction::controls::pointer_move(host, doc, p, origin, ts)?;
        }
        InputKind::PointerLeave(p) | InputKind::PointerCancel(p) => {
            if matches!(item.kind, InputKind::PointerCancel(_))
                && crate::interaction::scroll::scrollbar_move(host, doc, p, true, origin, ts)?
            {
                return Ok(());
            }
            if matches!(item.kind, InputKind::PointerCancel(_)) {
                eng(host, doc, |ix, d| ix.set_active(d, None))?;
                let captured = process_pending_capture(host, doc, p, item.modifiers, origin, ts)?;
                let target = match captured {
                    Some(c) => c,
                    None => hit(host, doc, p.x, p.y)?,
                };
                run_dispatch(
                    host,
                    doc,
                    target,
                    pointer_event("pointercancel", p, item.modifiers),
                    true,
                    origin,
                    ts,
                )?;
                implicit_release(host, doc, p, item.modifiers, origin, ts)?;
            }
            pointer::on_leave(host, doc, p, item.modifiers, origin, ts)?;
        }
        InputKind::Wheel {
            x,
            y,
            delta_x,
            delta_y,
        } => {
            if !crate::interaction::controls::wheel(host, doc, *x, *y, *delta_y)? {
                crate::interaction::scroll::on_wheel(
                    host,
                    doc,
                    *x,
                    *y,
                    *delta_x,
                    *delta_y,
                    item.modifiers,
                    origin,
                    ts,
                )?
            }
        }
        InputKind::PointerDown(p) => {
            if crate::interaction::scroll::scrollbar_down(host, doc, p)? {
                return Ok(());
            }
            let target = hit(host, doc, p.x, p.y)?;
            pointer::sync_hover_silently(host, doc, p, target, item.modifiers, origin)?;
            let primary = p.button == 0 && p.is_primary;
            let disabled = eng(host, doc, |_, d| {
                crate::interaction::activation::disabled_click_target(d, target)
            })?;
            eng(host, doc, |ix, d| {
                ix.note_modality(doc, false);
                if primary && !disabled {
                    ix.set_active(d, Some(target));
                }
            })?;
            let pdown = run_dispatch(
                host,
                doc,
                target,
                pointer_event("pointerdown", p, item.modifiers),
                true,
                origin,
                ts,
            )?;
            // A canceled `pointerdown` suppresses the compatibility `mousedown` (Pointer Events §11).
            let out = if pdown.canceled {
                pdown
            } else {
                run_dispatch(
                    host,
                    doc,
                    target,
                    mouse(
                        "mousedown",
                        EventInterface::MouseEvent,
                        p,
                        p.buttons,
                        item.modifiers,
                    ),
                    true,
                    origin,
                    ts,
                )?
            };
            if p.button == 0 {
                crate::interaction::popover::pointer_down(host, doc, target);
            }
            eng(host, doc, |ix, _| {
                if let Ok(s) = ix.doc_state(doc) {
                    s.pointer.press_target = Some(target);
                    s.pointer.press_button = Some(p.button);
                }
            })?;
            if !out.canceled && p.button == 0 && !disabled {
                let to = eng(host, doc, |_, d| click_focus_target(d, target))?;
                focus_update(host, doc, to, origin, FocusSource::Pointer)?;
                if primary
                    && eng(host, doc, |_, d| {
                        crate::interaction::editing::text_kind(d, target).is_some()
                    })?
                {
                    crate::interaction::editing::on_pointer_down(
                        host,
                        doc,
                        target,
                        p,
                        item.modifiers,
                        origin,
                        ts,
                    )?;
                }
                if primary {
                    crate::interaction::controls::pointer_down(
                        host,
                        doc,
                        target,
                        p,
                        item.modifiers,
                        origin,
                        ts,
                    )?;
                }
            }
        }
        InputKind::PointerUp(p) => {
            if crate::interaction::scroll::scrollbar_move(host, doc, p, true, origin, ts)? {
                return Ok(());
            }
            let hit_target = hit(host, doc, p.x, p.y)?;
            pointer::sync_hover_silently(host, doc, p, hit_target, item.modifiers, origin)?;
            let captured = process_pending_capture(host, doc, p, item.modifiers, origin, ts)?;
            let target = captured.unwrap_or(hit_target);
            run_dispatch(
                host,
                doc,
                target,
                pointer_event("pointerup", p, item.modifiers),
                true,
                origin,
                ts,
            )?;
            if p.button == 0 {
                crate::interaction::editing::on_pointer_up(host, doc);
                crate::interaction::controls::pointer_up(host, doc, p, origin, ts)?;
            }
            run_dispatch(
                host,
                doc,
                target,
                mouse(
                    "mouseup",
                    EventInterface::MouseEvent,
                    p,
                    p.buttons,
                    item.modifiers,
                ),
                true,
                origin,
                ts,
            )?;
            if p.buttons == 0 {
                implicit_release(host, doc, p, item.modifiers, origin, ts)?;
            }
            if p.button == 0 {
                crate::interaction::popover::pointer_up(host, doc, target);
            }
            let (press, connected) = eng(host, doc, |ix, d| {
                if p.button == 0 {
                    ix.set_active(d, None);
                }
                let press = ix
                    .doc_state(doc)
                    .map(|s| (s.pointer.press_target.take(), s.pointer.press_button.take()))
                    .unwrap_or_default();
                (press, d.arena().is_connected(target))
            })?;
            let same = press.0 == Some(target) && press.1 == Some(p.button);
            let disabled = eng(host, doc, |_, d| {
                crate::interaction::activation::disabled_click_target(d, target)
            })?;
            if p.button == 0 && same && connected && !disabled {
                let now = ts.unwrap_or_else(|| host.now_ms());
                let count = rt(host, |ix| {
                    let Ok(state) = ix.doc_state(doc) else {
                        return 1;
                    };
                    let count = match state.pointer.last_click {
                        Some((old, x, y, time, count))
                            if old == target
                                && now >= time
                                && now - time <= 500.0
                                && (p.x - x).abs() <= 4.0
                                && (p.y - y).abs() <= 4.0 =>
                        {
                            count.saturating_add(1)
                        }
                        _ => 1,
                    };
                    state.pointer.last_click = Some((target, p.x, p.y, now, count));
                    count
                });
                let mut click = mouse(
                    "click",
                    EventInterface::PointerEvent,
                    p,
                    p.buttons,
                    item.modifiers,
                );
                click.detail = count;
                run_dispatch(host, doc, target, click, true, origin, ts)?;
                if count == 2 {
                    let mut double = mouse(
                        "dblclick",
                        EventInterface::MouseEvent,
                        p,
                        p.buttons,
                        item.modifiers,
                    );
                    double.detail = 2;
                    run_dispatch(host, doc, target, double, true, origin, ts)?;
                }
            } else if p.button != 0 && same && connected && !disabled {
                run_dispatch(
                    host,
                    doc,
                    target,
                    mouse(
                        "auxclick",
                        EventInterface::PointerEvent,
                        p,
                        p.buttons,
                        item.modifiers,
                    ),
                    true,
                    origin,
                    ts,
                )?;
            }
            if p.button == 2 && same && connected && !disabled {
                let mut menu = mouse(
                    "contextmenu",
                    EventInterface::PointerEvent,
                    p,
                    p.buttons,
                    item.modifiers,
                );
                menu.detail = 0;
                run_dispatch(host, doc, target, menu, true, origin, ts)?;
            }
        }
        InputKind::KeyDown(k) => {
            eng(host, doc, |ix, _| ix.note_modality(doc, true))?;
            let target = key_target(host, doc)?;
            let focused = rt(host, |ix| ix.focused_element(doc)).is_some();
            let down = run_dispatch(
                host,
                doc,
                target,
                key("keydown", k, item.modifiers),
                true,
                origin,
                ts,
            )?;
            if down.canceled {
                // A canceled keydown: no keypress, beforeinput, input or default action.
                return Ok(());
            }
            if !k.is_composing
                && (k.key == "ContextMenu"
                    || (k.key == "F10"
                        && item.modifiers.shift
                        && !item.modifiers.ctrl
                        && !item.modifiers.alt
                        && !item.modifiers.meta))
            {
                let mut menu = EventInit::new("contextmenu")
                    .interface(EventInterface::PointerEvent)
                    .bubbles(true)
                    .cancelable(true);
                menu.modifiers = item.modifiers;
                menu.data.button = Some(2);
                run_dispatch(host, doc, target, menu, true, origin, ts)?;
                return Ok(());
            }
            // Select, range, number and radio keys (012 `controls.md`), then text editing.
            if focused
                && crate::interaction::controls::keydown(
                    host,
                    doc,
                    target,
                    k,
                    item.modifiers,
                    origin,
                    ts,
                )?
            {
                return Ok(());
            }
            // A text control edits (012 `editing.md`); every other key default is keyboard.rs's.
            if focused
                && eng(host, doc, |_, d| {
                    crate::interaction::editing::text_kind(d, target).is_some()
                })?
                && crate::interaction::editing::keydown(
                    host,
                    doc,
                    target,
                    k,
                    item.modifiers,
                    origin,
                    ts,
                )?
            {
                return Ok(());
            }
            // Clipboard shortcuts where no text control has the focus: the event reaches application code.
            if !k.is_composing
                && let Some(c) =
                    crate::interaction::editing::clipboard_command(&k.key, item.modifiers)
            {
                crate::interaction::editing::document_clipboard(
                    host,
                    doc,
                    target,
                    c,
                    item.modifiers,
                    origin,
                    ts,
                )?;
                return Ok(());
            }
            keyboard::keydown_default(host, doc, target, focused, k, item.modifiers, origin, ts)?;
        }
        InputKind::KeyUp(k) => {
            let target = key_target(host, doc)?;
            let focused = rt(host, |ix| ix.focused_element(doc)).is_some();
            let up = run_dispatch(
                host,
                doc,
                target,
                key("keyup", k, item.modifiers),
                true,
                origin,
                ts,
            )?;
            if !up.canceled {
                keyboard::keyup_default(host, doc, target, focused, k, item.modifiers, origin, ts)?;
            }
        }
        InputKind::ImeEnabled => {
            crate::interaction::editing::ime_enabled(host, doc);
        }
        InputKind::ImeDisabled => {
            crate::interaction::editing::ime::finish(host, doc, false, item.modifiers, origin, ts)?
        }
        InputKind::ImePreedit { text, cursor } => crate::interaction::editing::ime::preedit(
            host,
            doc,
            text,
            *cursor,
            item.modifiers,
            origin,
            ts,
        )?,
        InputKind::ImeCommit { text } => {
            crate::interaction::editing::ime::commit(host, doc, text, item.modifiers, origin, ts)?
        }
        InputKind::ImeDeleteSurrounding {
            before_u16,
            after_u16,
        } => crate::interaction::editing::ime::delete_surrounding(
            host,
            doc,
            *before_u16,
            *after_u16,
            item.modifiers,
            origin,
            ts,
        )?,
        // The window lost the focus: a composition in progress ends with what was composed (the
        // platform has already committed or cleared its preedit, 011 Clarification Q3).
        InputKind::WindowFocus { focused: false } => {
            // A press or drag in progress ends: the matching release will not arrive here.
            eng(host, doc, |ix, d| {
                ix.set_active(d, None);
                if let Ok(s) = ix.doc_state(doc) {
                    s.pointer = PointerState::default();
                }
            })?;
            crate::interaction::editing::on_pointer_up(host, doc);
            crate::interaction::editing::ime::finish(host, doc, false, item.modifiers, origin, ts)?
        }
        // Window focus gained and modifier sync have no default action.
        _ => {}
    }
    Ok(())
}
