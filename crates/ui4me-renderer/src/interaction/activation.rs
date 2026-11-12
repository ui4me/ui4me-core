//! owner: 012-interaction — created by 001 slice (R-28)
//!
//! Activation (012 `dispatch-state-machine.md` §1 BUILD/PRE_ACTIVATION/POST; DOM dispatch
//! `dom.bs:1377-1772` @3c53f93; HTML `source:51441-51474` @6d7788b).
//!
//! 001 T058 part: the choice of the activation target (MouseEvent/PointerEvent `click` only:
//! the target if it has activation behaviour, else the first bubbling ancestor that has),
//! checkbox legacy-pre-activation (save `{checked, indeterminate}`, toggle, clear
//! indeterminate — visible to listeners) and legacy-canceled-activation (restore).
//! 001 T083: the activation behaviours of uncanceled activation (checkbox `input`/`change`,
//! submit-button form submission); see [`activation_behaviour`].

use crate::interaction::dispatch::{DispatchHost, eng, rt, run_dispatch};
use crate::interaction::frame::{EventInit, EventInterface, SavedPreActivation};
use ui4me_document::document::Document;
use ui4me_document::node::ElementData;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DispatchId, DocumentId, NodeId};
use ui4me_engine_types::status::Status;

fn element(d: &Document, n: NodeId) -> Option<&ElementData> {
    d.arena()
        .get(n)
        .and_then(|x| x.as_element())
        .filter(|e| e.is_html())
}

fn input_type(e: &ElementData) -> String {
    e.input_type().unwrap_or_default().as_str().to_owned()
}

/// Does `n` have activation behaviour (HTML; slice set)?
pub fn has_activation_behaviour(d: &Document, n: NodeId) -> bool {
    let Some(e) = element(d, n) else { return false };
    match e.local_name() {
        "button" | "label" | "summary" => true,
        "a" | "area" => e.attr("href").is_some(),
        "input" => matches!(
            input_type(e).as_str(),
            "checkbox" | "radio" | "submit" | "reset" | "button" | "image" | "file"
        ),
        _ => false,
    }
}

pub(crate) fn is_radio(d: &Document, n: NodeId) -> bool {
    element(d, n).is_some_and(|e| e.local_name() == "input" && input_type(e) == "radio")
}

pub(crate) fn is_checkbox(d: &Document, n: NodeId) -> bool {
    element(d, n).is_some_and(|e| e.local_name() == "input" && input_type(e) == "checkbox")
}

/// A disabled form control (HTML 4.10.18.2 "actually disabled"): a `button`, `input`, `select` or
/// `textarea` with a `disabled` attribute, or a descendant of a disabled `fieldset` that is not inside
/// that fieldset's first `legend` child (015 HTML-3; `Document::fieldset_disabled`).
pub fn is_disabled_form_control(d: &Document, n: NodeId) -> bool {
    d.control_facts(n).is_some_and(|facts| facts.disabled)
}

/// A mouse hit can belong to a label/span inside a disabled button. Suppress
/// activation for that entire control, including descendants. A disabled
/// fieldset itself is not an activation control (its first legend is exempt).
pub(crate) fn disabled_click_target(d: &Document, mut node: NodeId) -> bool {
    use ui4me_document::control_semantics::ControlKind;
    loop {
        if d.control_facts(node).is_some_and(|facts| {
            facts.disabled
                && matches!(
                    facts.kind,
                    ControlKind::Input(_)
                        | ControlKind::Button
                        | ControlKind::Select
                        | ControlKind::Textarea
                )
        }) {
            return true;
        }
        let Some(parent) = d.parent(node).ok().flatten() else {
            return false;
        };
        node = parent;
    }
}

/// DOM dispatch steps 5 and 9.6: the activation target of a MouseEvent/PointerEvent `click`.
pub(crate) fn choose_target(d: &Document, path: &[NodeId], init: &EventInit) -> Option<NodeId> {
    if init.event_type != "click" || !init.interface.is_mouse() {
        return None;
    }
    let (&target, ancestors) = path.split_first()?;
    if has_activation_behaviour(d, target) {
        return Some(target);
    }
    if !init.bubbles {
        return None;
    }
    ancestors
        .iter()
        .copied()
        .find(|&a| has_activation_behaviour(d, a))
}

/// Legacy-pre-activation behaviour (checkbox): save, toggle, clear indeterminate.
pub(crate) fn legacy_pre_activation(d: &mut Document, act: NodeId) -> Option<SavedPreActivation> {
    if is_radio(d, act) {
        let was_checked = d
            .control_state(act)
            .ok()
            .flatten()?
            .checkedness
            .unwrap_or(false);
        let previous = d.radio_group(act).into_iter().find(|r| {
            *r != act
                && d.control_state(*r)
                    .ok()
                    .flatten()
                    .and_then(|c| c.checkedness)
                    == Some(true)
        });
        d.radio_check(act).ok()?;
        return Some(SavedPreActivation::Radio {
            was_checked,
            previous,
        });
    }
    if !is_checkbox(d, act) {
        return None;
    }
    let c = d.control_state(act).ok().flatten()?;
    let (checked, indeterminate) = (c.checkedness.unwrap_or(false), c.indeterminate);
    let mut w = d.control_state_writer();
    w.set_checkedness(act, !checked).ok()?;
    let _ = w.set_indeterminate(act, false);
    Some(SavedPreActivation::Checkbox {
        checked,
        indeterminate,
    })
}

/// Legacy-canceled-activation behaviour: restore the saved checkbox state.
pub(crate) fn legacy_canceled_activation(d: &mut Document, act: NodeId, saved: SavedPreActivation) {
    match saved {
        SavedPreActivation::Checkbox {
            checked,
            indeterminate,
        } => {
            let mut w = d.control_state_writer();
            let _ = w.set_checkedness(act, checked);
            let _ = w.set_indeterminate(act, indeterminate);
        }
        SavedPreActivation::Radio {
            was_checked,
            previous,
        } => {
            let mut w = d.control_state_writer();
            let _ = w.set_checkedness(act, was_checked);
            if let Some(p) = previous {
                let _ = w.set_checkedness(p, true);
            }
        }
    }
}

/// Is `n` connected (a stale node is not)?
fn connected(d: &Document, n: NodeId) -> bool {
    d.arena().contains(n) && d.arena().is_connected(n)
}

/// A submit button (HTML: `button` whose type is submit — the missing and invalid value
/// default —, `input type=submit|image`).
pub fn is_submit_button(d: &Document, n: NodeId) -> bool {
    let Some(e) = element(d, n) else { return false };
    match e.local_name() {
        "button" => !matches!(
            e.attr("type")
                .map(|t| t.trim().to_ascii_lowercase())
                .as_deref(),
            Some("button" | "reset")
        ),
        "input" => matches!(input_type(e).as_str(), "submit" | "image"),
        _ => false,
    }
}

/// The form owner of a form-associated element (HTML "reset the form owner",
/// slice subset): the element named by a `form` content attribute when present (an HTML `form`
/// in the same tree, else none), otherwise the nearest `form` ancestor.
pub fn form_owner(d: &Document, n: NodeId) -> Option<NodeId> {
    let e = element(d, n)?;
    if let Some(id) = e.attr("form") {
        let f = d.element_by_id(id)?;
        return element(d, f)
            .filter(|x| x.local_name() == "form")
            .map(|_| f);
    }
    let mut a = d.arena().get(n).and_then(|x| x.parent());
    while let Some(p) = a {
        if element(d, p).is_some_and(|x| x.local_name() == "form") {
            return Some(p);
        }
        a = d.arena().get(p).and_then(|x| x.parent());
    }
    None
}

/// What an uncanceled activation does, decided inside one engine borrow.
enum Behaviour {
    None,
    /// Checkbox: fire `input`, then `change` at the element.
    InputChange,
    /// Submit button: submit the form owner (fire `submit` at the form).
    Submit {
        form: NodeId,
        submitter: NodeId,
    },
    /// Reset button: fire `reset` at the form and, unless canceled, reset its controls.
    Reset {
        form: NodeId,
    },
    /// A label: focus and click the labeled control.
    Label {
        control: NodeId,
    },
    /// A file input: the host opens the native picker (0.6 F0606).
    FilePicker,
    /// A hyperlink (`a`/`area` with `href`): reported to the host, which applies its link
    /// policy (0.6 F0608). Core never navigates the document or fetches the target.
    Link,
    /// A `details` summary: toggle the `details` (015 HTML-2).
    Summary,
}

/// Labelable elements (HTML 4.10.2): the ones a label can forward to.
fn labelable(d: &Document, n: NodeId) -> bool {
    element(d, n).is_some_and(|e| match e.local_name() {
        "button" | "meter" | "output" | "progress" | "select" | "textarea" => true,
        "input" => input_type(e) != "hidden",
        _ => false,
    })
}

/// `label.control`: the element its `for` names when labelable, else its first labelable descendant.
pub fn label_control(d: &Document, label: NodeId) -> Option<NodeId> {
    if let Some(id) = element(d, label).and_then(|e| e.attr("for")) {
        return d.element_by_id(id).filter(|c| labelable(d, *c));
    }
    d.arena()
        .subtree(label)
        .into_iter()
        .skip(1)
        .find(|c| labelable(d, *c))
}

/// Interactive content a click on which is its own: it does not forward to the label's control.
fn interactive(d: &Document, n: NodeId) -> bool {
    element(d, n).is_some_and(|e| match e.local_name() {
        "a" => e.attr("href").is_some(),
        "button" | "input" | "select" | "textarea" | "label" | "summary" => true,
        _ => false,
    })
}

fn is_reset_button(d: &Document, n: NodeId) -> bool {
    element(d, n).is_some_and(|e| match e.local_name() {
        "button" => e
            .attr("type")
            .is_some_and(|t| t.trim().eq_ignore_ascii_case("reset")),
        "input" => input_type(e) == "reset",
        _ => false,
    })
}

fn decide(
    d: &Document,
    act: NodeId,
    saved: Option<SavedPreActivation>,
    target: NodeId,
) -> Behaviour {
    // A node freed or disconnected by a listener has no activation behaviour left to run
    // (checkbox: "if the element is not connected, return"; a disconnected button's form
    // cannot navigate, so form submission returns).
    if !connected(d, act) {
        return Behaviour::None;
    }
    if is_checkbox(d, act) {
        return if is_disabled_form_control(d, act) {
            Behaviour::None
        } else {
            Behaviour::InputChange
        };
    }
    if is_radio(d, act) {
        // `input` and `change` only when the click checked a radio that was not checked.
        return match saved {
            Some(SavedPreActivation::Radio {
                was_checked: false, ..
            }) if !is_disabled_form_control(d, act) => Behaviour::InputChange,
            _ => Behaviour::None,
        };
    }
    if element(d, act).is_some_and(|e| e.local_name() == "label") {
        let Some(control) = label_control(d, act) else {
            return Behaviour::None;
        };
        // A click on the control itself, or on interactive content inside the label, is its own.
        let mut a = Some(target);
        while let Some(n) = a.filter(|n| *n != act) {
            if n == control || interactive(d, n) {
                return Behaviour::None;
            }
            a = d.arena().get(n).and_then(|x| x.parent());
        }
        return if is_disabled_form_control(d, control) {
            Behaviour::None
        } else {
            Behaviour::Label { control }
        };
    }
    if crate::interaction::dialog::summary_details(d, act).is_some() {
        return Behaviour::Summary;
    }
    if is_reset_button(d, act)
        && !is_disabled_form_control(d, act)
        && let Some(form) = form_owner(d, act).filter(|&f| connected(d, f))
    {
        return Behaviour::Reset { form };
    }
    if is_submit_button(d, act)
        && !is_disabled_form_control(d, act)
        && let Some(form) = form_owner(d, act).filter(|&f| connected(d, f))
    {
        return Behaviour::Submit {
            form,
            submitter: act,
        };
    }
    if element(d, act).is_some_and(|e| e.local_name() == "input" && input_type(e) == "file") {
        return if is_disabled_form_control(d, act) {
            Behaviour::None
        } else {
            Behaviour::FilePicker
        };
    }
    if element(d, act)
        .is_some_and(|e| matches!(e.local_name(), "a" | "area") && e.attr("href").is_some())
    {
        return Behaviour::Link;
    }
    Behaviour::None
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

/// Activation behaviour of an uncanceled activation event (POST; 012 §1), run while the
/// activation event's frame is still on the stack, so its dispatches nest one level deeper:
/// - checkbox (HTML `source:51441-51474`): if still connected, `input` then `change` (plain
///   `Event`s, bubbling, not cancelable, trusted). A checkbox removed by its own click listener
///   stays toggled and fires neither.
/// - submit button (HTML `source:54086-54097`): only when connected and with a connected form
///   owner, the form-submission algorithm's `submit` event (SubmitEvent, bubbling, cancelable,
///   trusted) at the form; no constraint validation, no navigation in v1 (ui4me navigates
///   nowhere; the host reacts to `submit`). Re-entrant submission of the same form while its
///   `submit` is being fired is ignored (HTML "firing submission events" flag).
///
/// Other activation behaviours (radio groups, reset, labels, links, summary) are 012's later
/// work. Errors of the nested dispatches (depth limit, closing document) end the behaviour.
pub(crate) fn activation_behaviour(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    act: NodeId,
    dispatch: DispatchId,
) {
    let frame = rt(host, |ix| {
        ix.stack
            .frames
            .iter()
            .find(|f| f.dispatch_id == dispatch)
            .map(|f| (f.saved, f.event.target))
    });
    let (saved, target) = frame.unwrap_or((None, act));
    // `popovertarget` on a button runs before the button's own behaviour (HTML 4.10.6).
    crate::interaction::popover::invoker_activation(host, doc, act);
    let Ok(b) = eng(host, doc, |_, d| decide(d, act, saved, target)) else {
        return;
    };
    match b {
        Behaviour::None => {}
        Behaviour::InputChange => {
            let ev = |ty: &str| EventInit::new(ty).bubbles(true).cancelable(false);
            if fire(host, doc, act, ev("input")).is_err() {
                return;
            }
            let _ = fire(host, doc, act, ev("change"));
        }
        Behaviour::Label { control } => {
            let _ = crate::interaction::focus::focus_update(
                host,
                doc,
                Some(control),
                DispatchOrigin::Activation,
                crate::interaction::focus::FocusSource::Pointer,
            );
            let mut click = EventInit::new("click")
                .interface(EventInterface::PointerEvent)
                .bubbles(true)
                .cancelable(true);
            click.detail = 1;
            let _ = fire(host, doc, control, click);
        }
        Behaviour::Link => host.link_activated(doc, act),
        Behaviour::FilePicker => host.file_input_activated(doc, act),
        Behaviour::Summary => {
            let _ = eng(host, doc, |_, d| {
                crate::interaction::dialog::toggle_details(d, act)
            });
        }
        Behaviour::Reset { form } => {
            let init = EventInit::new("reset").bubbles(true).cancelable(true);
            if let Ok(false) = fire(host, doc, form, init) {
                let _ = eng(host, doc, |_, d| d.form_reset_controls(form));
                host.form_reset(doc, form);
            }
        }
        Behaviour::Submit { form, submitter } => {
            submit_form(host, doc, form, Some(submitter));
        }
    }
}

/// The controls of `scope` (a form: its listed elements in tree order; any other node: itself)
/// that fail constraint validation right now.
fn invalid_controls(d: &Document, scope: NodeId) -> Vec<NodeId> {
    let nodes = if element(d, scope).is_some_and(|e| e.local_name() == "form") {
        d.form_elements(scope).unwrap_or_default()
    } else {
        vec![scope]
    };
    nodes
        .into_iter()
        .filter(|&n| d.constraint_validity(n).is_some_and(|v| !v.valid()))
        .collect()
}

/// Fire a non-bubbling, cancelable `invalid` at each invalid control of `scope` (HTML "statically
/// validate the constraints"). A control removed (or moved out of the form) by an earlier handler
/// is skipped. Returns `(fired, unhandled)`: the controls that received `invalid`, and those whose
/// event was not canceled.
pub(crate) fn fire_invalid_events(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    scope: NodeId,
) -> (Vec<NodeId>, Vec<NodeId>) {
    let Ok(list) = eng(host, doc, |_, d| invalid_controls(d, scope)) else {
        return (Vec::new(), Vec::new());
    };
    let is_form = eng(host, doc, |_, d| {
        element(d, scope).is_some_and(|e| e.local_name() == "form")
    })
    .unwrap_or(false);
    let (mut fired, mut unhandled) = (Vec::new(), Vec::new());
    for c in list {
        let live = eng(host, doc, |_, d| {
            connected(d, c) && (!is_form || form_owner(d, c) == Some(scope))
        })
        .unwrap_or(false);
        if !live {
            continue;
        }
        let init = EventInit::new("invalid").bubbles(false).cancelable(true);
        match fire(host, doc, c, init) {
            Ok(canceled) => {
                fired.push(c);
                if !canceled {
                    unhandled.push(c);
                }
            }
            Err(_) => break,
        }
    }
    (fired, unhandled)
}

/// HTML form submission (interactive): unless `novalidate` / `formnovalidate`, statically
/// validate the form; invalid fires `invalid` at each failing control, focuses the first
/// unhandled one and submits nothing. Otherwise fires the cancelable `submit` (with `submitter`)
/// at the form. No navigation (the host reacts to `submit`). Re-entrant submission of a form
/// whose submission is in progress is ignored.
pub(crate) fn submit_form(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    form: NodeId,
    submitter: Option<NodeId>,
) {
    if !rt(host, |ix| ix.submitting.insert((doc, form))) {
        return;
    }
    submit_form_guarded(host, doc, form, submitter);
    rt(host, |ix| ix.submitting.remove(&(doc, form)));
}

fn submit_form_guarded(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    form: NodeId,
    submitter: Option<NodeId>,
) {
    let Ok((no_validate, submitter_id)) = eng(host, doc, |_, d| {
        let skip = element(d, form).is_some_and(|e| e.attr("novalidate").is_some())
            || submitter
                .and_then(|s| element(d, s))
                .is_some_and(|e| e.attr("formnovalidate").is_some());
        let id = submitter
            .and_then(|s| element(d, s))
            .and_then(|e| e.attr("id"))
            .filter(|i| !i.is_empty())
            .map(str::to_owned);
        (skip, id)
    }) else {
        return;
    };
    if !no_validate {
        let (fired, unhandled) = fire_invalid_events(host, doc, form);
        if !fired.is_empty() {
            let first = unhandled
                .into_iter()
                .find(|&c| eng(host, doc, |_, d| connected(d, c)).unwrap_or(false));
            if let Some(c) = first {
                let _ = crate::interaction::focus::focus_update(
                    host,
                    doc,
                    Some(c),
                    DispatchOrigin::Activation,
                    crate::interaction::focus::FocusSource::Script,
                );
            }
            return;
        }
    }
    if !eng(host, doc, |_, d| connected(d, form)).unwrap_or(false) {
        return;
    }
    let mut init = EventInit::new("submit")
        .interface(EventInterface::SubmitEvent)
        .bubbles(true)
        .cancelable(true);
    init.data.submitter = submitter_id;
    let _ = fire(host, doc, form, init);
}
