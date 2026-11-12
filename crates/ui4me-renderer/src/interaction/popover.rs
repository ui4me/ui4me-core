//! owner: 012-interaction — F0808 (Core 0.8)
//!
//! The HTML `popover` attribute subset (HTML 6.12):
//!
//! * `popover=auto` (also the empty value) and `popover=manual` (every other value). An open popover
//!   is in the document's top layer (`UiFlags::TOP_LAYER` without `MODAL`; that is the "popover open"
//!   state and what `:popover-open` matches), painted above the page and ordered with modal dialogs by
//!   top-layer insertion order.
//! * `show`, `hide` and `toggle` (public commands and the `popovertarget` invokers). `beforetoggle`
//!   (cancelable when opening) fires synchronously, `toggle` is the queued element task shared with
//!   `<details>` and `<dialog>`; both carry `old_state` / `new_state`.
//! * Auto popovers are light dismissed: opening one hides every open auto popover that is not an
//!   ancestor of it (DOM ancestry, or the ancestry of its invoker); a press and release outside the
//!   open auto popovers hides them all; Escape hides the topmost close watcher (an open auto popover
//!   or a modal dialog, whichever entered the top layer last); hiding one hides those above it.
//! * Focus: showing a popover focuses its `autofocus` descendant (or itself with `autofocus`); hiding
//!   one returns focus to the previously focused element when focus was inside it.
//!
//! Not implemented: `popover=hint`, `popovertarget` on elements other than buttons, `:popover-open`
//! in shadow trees, the `source` argument of `toggle()`.

use crate::interaction::dialog::toggle_event_init;
use crate::interaction::dispatch::{DispatchHost, eng, operation, run_dispatch, status};
use crate::interaction::focus::{FocusSource, focus_update, is_focusable};
use crate::interaction::frame::EventInit;
use ui4me_document::document::Document;
use ui4me_document::ui_state::UiFlags;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// The popover state of an element with a `popover` attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverKind {
    Auto,
    Manual,
}

impl PopoverKind {
    pub fn name(self) -> &'static str {
        match self {
            PopoverKind::Auto => "auto",
            PopoverKind::Manual => "manual",
        }
    }
}

/// Per-popover runtime state.
#[derive(Clone, Debug, Default)]
pub(crate) struct PopoverState {
    /// The invoker that showed it (`popovertarget`), for ancestry and focus return.
    pub invoker: Option<NodeId>,
    pub previously_focused: Option<NodeId>,
}

/// The popover state of `n`: `None` without a `popover` attribute (or not an HTML element).
pub fn popover_kind(d: &Document, n: NodeId) -> Option<PopoverKind> {
    let e = d.arena().get(n)?.as_element()?;
    if !e.is_html() {
        return None;
    }
    let v = e.attr("popover")?;
    Some(if v.is_empty() || v.eq_ignore_ascii_case("auto") {
        PopoverKind::Auto
    } else {
        PopoverKind::Manual
    })
}

/// Is `n` a showing popover (HTML "popover visibility state: showing")?
pub fn is_open(d: &Document, n: NodeId) -> bool {
    popover_kind(d, n).is_some()
        && d.arena().is_connected(n)
        && d.ui_flags(n)
            .is_ok_and(|f| f.contains(UiFlags::TOP_LAYER) && !f.contains(UiFlags::MODAL))
}

/// The open auto popovers, bottom first (top-layer order).
fn auto_stack(d: &Document) -> Vec<NodeId> {
    d.top_layer()
        .iter()
        .copied()
        .filter(|&n| is_open(d, n) && popover_kind(d, n) == Some(PopoverKind::Auto))
        .collect()
}

/// The invoker target of a `popovertarget` button: `(popover, action)`; `None` when `n` is not an
/// enabled button invoker with a valid target.
pub fn invoker_target(d: &Document, n: NodeId) -> Option<(NodeId, &'static str)> {
    let e = d.arena().get(n)?.as_element()?;
    if !e.is_html() {
        return None;
    }
    let button = match e.local_name() {
        "button" => true,
        "input" => matches!(
            e.attr("type")
                .map(|t| t.trim().to_ascii_lowercase())
                .as_deref(),
            Some("button" | "submit" | "reset" | "image")
        ),
        _ => false,
    };
    if !button || crate::interaction::activation::is_disabled_form_control(d, n) {
        return None;
    }
    let target = d.element_by_id(e.attr("popovertarget")?)?;
    popover_kind(d, target)?;
    let action = match e
        .attr("popovertargetaction")
        .map(|a| a.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("show") => "show",
        Some("hide") => "hide",
        _ => "toggle",
    };
    Some((target, action))
}

/// The topmost open auto popover among `node`'s ancestors (itself included when `inclusive`) and the
/// popovers that `node` or an ancestor invoked.
fn topmost_popover_ancestor(d: &Document, node: NodeId, inclusive: bool) -> Option<NodeId> {
    let stack = auto_stack(d);
    let mut best: Option<usize> = None;
    let mut consider = |p: NodeId| {
        if let Some(i) = stack.iter().position(|&s| s == p) {
            best = Some(best.map_or(i, |b| b.max(i)));
        }
    };
    let mut a = if inclusive {
        Some(node)
    } else {
        d.arena().get(node).and_then(|x| x.parent())
    };
    while let Some(x) = a {
        consider(x);
        // A `popovertarget` button counts as part of the popover it controls.
        if let Some((target, _)) = invoker_target(d, x) {
            consider(target);
        }
        a = d.arena().get(x).and_then(|n| n.parent());
    }
    best.map(|i| stack[i])
}

/// The first `popovertarget` invoker (tree order) of `popover`: its implicit anchor (F0808).
pub fn invoker_of(d: &Document, popover: NodeId) -> Option<NodeId> {
    popover_kind(d, popover)?;
    d.arena()
        .subtree(d.document_node())
        .into_iter()
        .find(|&n| invoker_target(d, n).is_some_and(|(t, _)| t == popover))
}

fn check_target(d: &Document, popover: NodeId) -> Result<PopoverKind, Status> {
    if !d.arena().contains(popover) {
        return Err(status(Code::StaleHandle));
    }
    if crate::interaction::dispatch::is_closing(d) {
        return Err(status(Code::DocumentClosed));
    }
    popover_kind(d, popover).ok_or_else(|| {
        status(Code::InvalidArgument).with_message("not an element with a popover attribute")
    })
}

fn fire(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    init: EventInit,
) -> Result<bool, Status> {
    run_dispatch(
        host,
        doc,
        target,
        init,
        true,
        DispatchOrigin::Activation,
        None,
    )
    .map(|o| o.canceled)
}

/// The popover focusing steps: the popover itself with `autofocus`, else its first focusable
/// `autofocus` descendant.
fn focusing_steps(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    popover: NodeId,
) -> Result<(), Status> {
    let control = eng(host, doc, |_, d| {
        let autofocus = |n: NodeId| {
            d.arena()
                .get(n)
                .and_then(|x| x.as_element())
                .is_some_and(|e| e.attr("autofocus").is_some())
        };
        if autofocus(popover) {
            return Some(popover);
        }
        d.arena()
            .subtree(popover)
            .into_iter()
            .skip(1)
            .find(|&n| autofocus(n) && is_focusable(d, n))
    })?;
    if let Some(c) = control
        && eng(host, doc, |_, d| is_focusable(d, c))?
    {
        focus_update(
            host,
            doc,
            Some(c),
            DispatchOrigin::Focus,
            FocusSource::Script,
        )?;
    }
    Ok(())
}

/// Show `popover` (HTML "show popover"). `Ok(true)` when it is showing afterwards because of this
/// call, `Ok(false)` when it was already open or `beforetoggle` was canceled.
pub fn show(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    popover: NodeId,
    invoker: Option<NodeId>,
) -> Result<bool, Status> {
    operation(host, |h| show_inner(h, doc, popover, invoker))
}

fn can_show(d: &Document, popover: NodeId) -> Result<bool, Status> {
    check_target(d, popover)?;
    if is_open(d, popover) {
        return Ok(false);
    }
    if !d.arena().is_connected(popover) {
        return Err(
            status(Code::InvalidState).with_message("a popover must be connected to be shown")
        );
    }
    if d.ui_flags(popover)
        .is_ok_and(|f| f.contains(UiFlags::MODAL))
    {
        return Err(status(Code::InvalidState).with_message("a modal dialog cannot be a popover"));
    }
    Ok(true)
}

fn show_inner(
    h: &mut dyn DispatchHost,
    doc: DocumentId,
    popover: NodeId,
    invoker: Option<NodeId>,
) -> Result<bool, Status> {
    if !eng(h, doc, |_, d| can_show(d, popover))?? {
        return Ok(false);
    }
    if fire(
        h,
        doc,
        popover,
        toggle_event_init("beforetoggle", false, true, true),
    )? {
        return Ok(false);
    }
    // The state is checked again after the event.
    if !eng(h, doc, |_, d| can_show(d, popover).unwrap_or(false))? {
        return Ok(false);
    }
    let kind = eng(h, doc, |_, d| popover_kind(d, popover))?.unwrap_or(PopoverKind::Manual);
    if kind == PopoverKind::Auto {
        // Hide every open auto popover that is not an ancestor of this one (or of its invoker).
        let ancestor = eng(h, doc, |_, d| {
            let own = topmost_popover_ancestor(d, popover, false);
            let via_invoker = invoker.and_then(|i| topmost_popover_ancestor(d, i, true));
            let stack = auto_stack(d);
            [own, via_invoker]
                .into_iter()
                .flatten()
                .max_by_key(|p| stack.iter().position(|s| s == p))
        })?;
        hide_until(h, doc, ancestor)?;
        if !eng(h, doc, |_, d| can_show(d, popover).unwrap_or(false))? {
            return Ok(false);
        }
    }
    eng(h, doc, |ix, d| {
        ix.set_element_flag(d, popover, UiFlags::TOP_LAYER, true);
        let focused = ix.focused_element(doc);
        if let Some(s) = ix.dialog_state(doc) {
            let st = s.popovers.entry(popover).or_default();
            st.invoker = invoker;
            st.previously_focused = focused;
        }
        ix.queue_toggle_task(doc, popover, false, true);
    })?;
    focusing_steps(h, doc, popover)?;
    Ok(true)
}

/// Hide the open auto popovers above `ancestor` (all of them when `None`), topmost first.
fn hide_until(
    h: &mut dyn DispatchHost,
    doc: DocumentId,
    ancestor: Option<NodeId>,
) -> Result<(), Status> {
    // Bounded: every pass hides one popover or stops.
    for _ in 0..1024 {
        let top = eng(h, doc, |_, d| {
            let stack = auto_stack(d);
            match (stack.last().copied(), ancestor) {
                (Some(t), Some(a)) if t == a => None,
                (t, _) => t,
            }
        })?;
        match top {
            Some(p) => hide_one(h, doc, p)?,
            None => break,
        }
    }
    Ok(())
}

/// Hide exactly `popover` (no stack handling): `beforetoggle`, close, queue `toggle`, return focus.
fn hide_one(h: &mut dyn DispatchHost, doc: DocumentId, popover: NodeId) -> Result<(), Status> {
    if !eng(h, doc, |_, d| is_open(d, popover))? {
        return Ok(());
    }
    fire(
        h,
        doc,
        popover,
        toggle_event_init("beforetoggle", true, false, false),
    )?;
    let restore = eng(h, doc, |ix, d| {
        if !is_open(d, popover) {
            return None;
        }
        ix.set_element_flag(d, popover, UiFlags::TOP_LAYER, false);
        let focused = ix.focused_element(doc);
        let inside = focused.is_some_and(|f| d.arena().is_inclusive_ancestor(popover, f));
        let st = ix
            .dialog_state(doc)
            .and_then(|s| s.popovers.remove(&popover));
        ix.queue_toggle_task(doc, popover, true, false);
        if inside {
            st.and_then(|s| s.previously_focused.or(s.invoker))
        } else {
            None
        }
    })?;
    if let Some(p) = restore
        && eng(h, doc, |_, d| d.arena().contains(p) && is_focusable(d, p))?
    {
        focus_update(h, doc, Some(p), DispatchOrigin::Focus, FocusSource::Script)?;
    }
    Ok(())
}

/// Hide `popover` and, for an auto popover, every popover above it (HTML "hide popover").
pub fn hide(host: &mut dyn DispatchHost, doc: DocumentId, popover: NodeId) -> Result<(), Status> {
    operation(host, |h| {
        let kind = eng(h, doc, |_, d| check_target(d, popover))??;
        if !eng(h, doc, |_, d| is_open(d, popover))? {
            return Ok(());
        }
        if kind == PopoverKind::Auto {
            // Everything above it first, then the popover itself.
            let above = eng(h, doc, |_, d| {
                let stack = auto_stack(d);
                stack
                    .iter()
                    .position(|&s| s == popover)
                    .map(|i| stack[i + 1..].iter().rev().copied().collect::<Vec<_>>())
                    .unwrap_or_default()
            })?;
            for p in above {
                hide_one(h, doc, p)?;
            }
        }
        hide_one(h, doc, popover)
    })
}

/// Toggle `popover`; returns whether it is open afterwards.
pub fn toggle(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    popover: NodeId,
    invoker: Option<NodeId>,
) -> Result<bool, Status> {
    operation(host, |h| {
        eng(h, doc, |_, d| check_target(d, popover))??;
        if eng(h, doc, |_, d| is_open(d, popover))? {
            hide(h, doc, popover)?;
        } else {
            show_inner(h, doc, popover, invoker)?;
        }
        eng(h, doc, |_, d| is_open(d, popover))
    })
}

/// The `popovertarget` activation behaviour of a click on `act` (run before the button's own).
pub(crate) fn invoker_activation(host: &mut dyn DispatchHost, doc: DocumentId, act: NodeId) {
    let Ok(Some((target, action))) = eng(host, doc, |_, d| {
        d.arena()
            .is_connected(act)
            .then(|| invoker_target(d, act))
            .flatten()
    }) else {
        return;
    };
    let open = eng(host, doc, |_, d| is_open(d, target)).unwrap_or(false);
    let _ = match (open, action) {
        (true, "toggle" | "hide") => hide(host, doc, target),
        (false, "toggle" | "show") => show(host, doc, target, Some(act)).map(|_| ()),
        _ => Ok(()),
    };
}

/// Escape (an uncanceled `keydown`): the topmost close watcher of the top layer, an open auto
/// popover or a modal dialog, gets a close request. Returns whether one did.
pub(crate) fn escape(host: &mut dyn DispatchHost, doc: DocumentId) -> Result<bool, Status> {
    enum Watcher {
        Dialog(NodeId),
        Popover(NodeId),
    }
    let w = eng(host, doc, |_, d| {
        d.top_layer().iter().rev().copied().find_map(|n| {
            if !d.arena().is_connected(n) {
                None
            } else if d.ui_flags(n).is_ok_and(|f| f.contains(UiFlags::MODAL)) {
                Some(Watcher::Dialog(n))
            } else if is_open(d, n) && popover_kind(d, n) == Some(PopoverKind::Auto) {
                Some(Watcher::Popover(n))
            } else {
                None
            }
        })
    })?;
    match w {
        Some(Watcher::Dialog(m)) => {
            crate::interaction::dialog::request_close(host, doc, m, None)?;
            Ok(true)
        }
        Some(Watcher::Popover(p)) => {
            hide(host, doc, p)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Pointer press: remember the topmost popover the press landed in (HTML "light dismiss open
/// popovers", pointerdown half).
pub(crate) fn pointer_down(host: &mut dyn DispatchHost, doc: DocumentId, target: NodeId) {
    let _ = eng(host, doc, |ix, d| {
        let a = topmost_popover_ancestor(d, target, true);
        if let Some(s) = ix.dialog_state(doc) {
            s.popover_down = Some(a);
        }
    });
}

/// Pointer release: when it lands in the same popover as the press, hide the open auto popovers
/// above it (all of them when it landed in none).
pub(crate) fn pointer_up(host: &mut dyn DispatchHost, doc: DocumentId, target: NodeId) {
    let Ok(down) = eng(host, doc, |ix, _| {
        ix.dialog_state(doc).and_then(|s| s.popover_down.take())
    }) else {
        return;
    };
    let Some(down) = down else { return };
    let Ok(up) = eng(host, doc, |_, d| {
        d.arena()
            .contains(target)
            .then(|| topmost_popover_ancestor(d, target, true))
    }) else {
        return;
    };
    if up == Some(down) {
        let _ = operation(host, |h| hide_until(h, doc, down));
    }
}

/// DocumentCore's attribute hook part: removing `popover` from a showing popover hides it (silently).
pub(crate) fn on_attribute_changed(
    w: &mut ui4me_document::ui_state::UiStateWriter<'_>,
    c: &ui4me_document::hooks::AttributeChange,
) {
    if c.local_name != "popover" || c.new.is_some() {
        return;
    }
    if let Ok(f) = w.flags(c.element)
        && f.contains(UiFlags::TOP_LAYER)
        && !f.contains(UiFlags::MODAL)
    {
        let _ = w.set_flags(c.element, f & !UiFlags::TOP_LAYER);
    }
}
