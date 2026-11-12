//! owner: 012-interaction — 015 HTML-1, HTML-2 (lane L-HTML)
//!
//! `dialog` (HTML 4.11.4) and `details` (HTML 4.11.1) behaviour:
//!
//! * `show()`, `showModal()`, `close(result)`, `requestClose(result)` and `returnValue`. A modal dialog
//!   enters the document's top layer (UI flags `MODAL | TOP_LAYER`, written through the document's
//!   UiStateWriter); while it is there the rest of the document is inert (`Document::is_inert`): not hit,
//!   not focusable, not exposed to assistive technology. `beforetoggle` (cancelable when opening) and
//!   `cancel` fire synchronously; `close` and `toggle` are queued element tasks dispatched in turn step 4
//!   (`ElementTask`), with the toggle task coalesced as HTML's "dialog toggle task tracker" (and the
//!   reference browser) do: a second toggle before the first fired keeps the first's old state and moves
//!   to the end of the queue.
//! * The dialog focusing steps: the dialog itself when it has `autofocus`, else its first focusable
//!   descendant (an `autofocus` one first), else the dialog itself (Chromium 156 focuses the dialog).
//!   Focus returns to the previously focused element on close.
//! * Escape (an uncanceled `keydown`) requests the topmost modal dialog to close: `cancel` (cancelable),
//!   then the close steps.
//! * `details`: activating its summary toggles `open`; every change of `open` (host writes included)
//!   queues a coalesced `toggle` task. The exclusive accordion (`name`) is DocumentCore's attribute
//!   change step (`Document::details_name_group`).

use crate::interaction::dispatch::{DispatchHost, eng, operation, rt, run_dispatch, status};
use crate::interaction::focus::{FocusSource, focus_update, is_focusable};
use crate::interaction::frame::{EventInit, EventInterface};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use ui4me_document::document::Document;
use ui4me_document::ui_state::UiFlags;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// One queued element task of a document (dispatched in turn step 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElementTaskKind {
    /// `toggle` (ToggleEvent) with the states before the first and after the last coalesced change.
    Toggle { old_open: bool, new_open: bool },
    /// `close` of a dialog.
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementTask {
    pub node: NodeId,
    pub kind: ElementTaskKind,
}

/// The queued element tasks of one document, shared with DocumentCore's attribute hook.
pub(crate) type TaskQueue = Rc<RefCell<VecDeque<ElementTask>>>;

/// Per-dialog state (HTML: "previously focused element", `returnValue`).
#[derive(Clone, Debug, Default)]
pub(crate) struct DialogState {
    pub previously_focused: Option<NodeId>,
    pub return_value: String,
}

/// Per-document dialog and details state.
#[derive(Debug, Default)]
pub(crate) struct DialogDocState {
    pub dialogs: HashMap<NodeId, DialogState>,
    /// Showing popovers (F0808): invoker and the element to return focus to.
    pub popovers: HashMap<NodeId, crate::interaction::popover::PopoverState>,
    /// The topmost popover a primary press landed in (`None` inside: outside every popover).
    pub popover_down: Option<Option<NodeId>>,
    pub tasks: TaskQueue,
}

/// Queue a toggle task for `node`, coalescing with a pending one (its old state is kept).
pub(crate) fn queue_toggle(tasks: &TaskQueue, node: NodeId, old_open: bool, new_open: bool) {
    let mut q = tasks.borrow_mut();
    let mut old = old_open;
    if let Some(i) = q
        .iter()
        .position(|t| t.node == node && matches!(t.kind, ElementTaskKind::Toggle { .. }))
        && let Some(ElementTask {
            kind: ElementTaskKind::Toggle { old_open: o, .. },
            ..
        }) = q.remove(i)
    {
        old = o;
    }
    q.push_back(ElementTask {
        node,
        kind: ElementTaskKind::Toggle {
            old_open: old,
            new_open,
        },
    });
}

fn html_element<'a>(
    d: &'a Document,
    n: NodeId,
    name: &str,
) -> Option<&'a ui4me_document::node::ElementData> {
    d.arena()
        .get(n)
        .and_then(|x| x.as_element())
        .filter(|e| e.is_html_named(name))
}

pub fn is_dialog(d: &Document, n: NodeId) -> bool {
    html_element(d, n, "dialog").is_some()
}

fn state_name(open: bool) -> &'static str {
    if open { "open" } else { "closed" }
}

/// A `ToggleEvent` init (`beforetoggle` is cancelable only when opening; `toggle` never).
pub fn toggle_event_init(
    event_type: &str,
    old_open: bool,
    new_open: bool,
    cancelable: bool,
) -> EventInit {
    let mut e = EventInit::new(event_type)
        .interface(EventInterface::ToggleEvent)
        .bubbles(false)
        .cancelable(cancelable);
    e.data.old_state = Some(state_name(old_open).into());
    e.data.new_state = Some(state_name(new_open).into());
    e
}

impl crate::interaction::dispatch::InteractionRuntime {
    pub(crate) fn dialog_state(&mut self, doc: DocumentId) -> Option<&mut DialogDocState> {
        self.docs.get_mut(&doc).map(|s| &mut s.dialog)
    }

    /// `dialog.returnValue`.
    pub fn dialog_return_value(&self, doc: DocumentId, dialog: NodeId) -> String {
        self.docs
            .get(&doc)
            .and_then(|s| s.dialog.dialogs.get(&dialog))
            .map(|d| d.return_value.clone())
            .unwrap_or_default()
    }

    /// `dialog.returnValue = v`.
    pub fn set_dialog_return_value(&mut self, doc: DocumentId, dialog: NodeId, v: &str) {
        if let Some(s) = self.dialog_state(doc) {
            s.dialogs.entry(dialog).or_default().return_value = v.to_string();
        }
    }

    /// Take the queued element tasks of `doc` (turn step 4).
    pub fn take_element_tasks(&mut self, doc: DocumentId) -> Vec<ElementTask> {
        self.docs
            .get(&doc)
            .map(|s| s.dialog.tasks.borrow_mut().drain(..).collect())
            .unwrap_or_default()
    }

    /// Whether any document has queued element tasks.
    pub fn has_element_tasks(&self) -> bool {
        self.docs
            .values()
            .any(|s| !s.dialog.tasks.borrow().is_empty())
    }

    fn queue_task(&mut self, doc: DocumentId, task: ElementTask) {
        if let Some(s) = self.dialog_state(doc) {
            s.tasks.borrow_mut().push_back(task);
        }
    }

    pub(crate) fn queue_toggle_task(
        &mut self,
        doc: DocumentId,
        node: NodeId,
        old_open: bool,
        new_open: bool,
    ) {
        if let Some(s) = self.dialog_state(doc) {
            queue_toggle(&s.tasks, node, old_open, new_open);
        }
    }
}

/// DocumentCore's attribute hook part (installed at attach): a change of `details[open]` queues a
/// toggle task; removing `open` from a dialog in the top layer takes it out (HTML "dialog cleanup steps").
pub(crate) fn on_attribute_changed(
    tasks: &TaskQueue,
    w: &mut ui4me_document::ui_state::UiStateWriter<'_>,
    c: &ui4me_document::hooks::AttributeChange,
) {
    crate::interaction::popover::on_attribute_changed(w, c);
    if c.local_name != "open" || c.old.is_some() == c.new.is_some() {
        return;
    }
    let d = w.document();
    if html_element(d, c.element, "details").is_some() {
        queue_toggle(tasks, c.element, c.old.is_some(), c.new.is_some());
    } else if html_element(d, c.element, "dialog").is_some()
        && c.new.is_none()
        && let Ok(f) = w.flags(c.element)
        && f.intersects(UiFlags::MODAL | UiFlags::TOP_LAYER)
    {
        let _ = w.set_flags(c.element, f & !(UiFlags::MODAL | UiFlags::TOP_LAYER));
    }
}

/// The focus delegate of a dialog (HTML "focus delegate", dialog case): its first focusable
/// descendant in tree order, a descendant with `autofocus` first.
fn focus_delegate(d: &Document, dialog: NodeId) -> Option<NodeId> {
    let all: Vec<NodeId> = d
        .arena()
        .subtree(dialog)
        .into_iter()
        .skip(1)
        .filter(|&n| is_focusable(d, n))
        .collect();
    let autofocus = all.iter().copied().find(|&n| {
        d.arena()
            .get(n)
            .and_then(|x| x.as_element())
            .is_some_and(|e| e.attr("autofocus").is_some())
    });
    autofocus.or_else(|| all.first().copied())
}

/// The dialog focusing steps.
fn focusing_steps(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    dialog: NodeId,
) -> Result<(), Status> {
    let control = eng(host, doc, |_, d| {
        if html_element(d, dialog, "dialog").is_some_and(|e| e.attr("autofocus").is_some()) {
            Some(dialog)
        } else {
            focus_delegate(d, dialog).or(Some(dialog))
        }
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

/// The state checks of `show()` / `showModal()`: `Ok(true)` to go on, `Ok(false)` to return (already
/// open the same way), `INVALID_STATE` otherwise.
fn check_show(d: &Document, dialog: NodeId, modal: bool) -> Result<bool, Status> {
    if !d.arena().contains(dialog) {
        return Err(status(Code::StaleHandle));
    }
    if crate::interaction::dispatch::is_closing(d) {
        return Err(status(Code::DocumentClosed));
    }
    let e = html_element(d, dialog, "dialog")
        .ok_or_else(|| status(Code::InvalidArgument).with_message("not a dialog element"))?;
    let is_modal = e.ui_flags().contains(UiFlags::MODAL);
    if e.attr("open").is_some() {
        return if is_modal == modal {
            Ok(false)
        } else {
            Err(status(Code::InvalidState).with_message("the dialog is already open"))
        };
    }
    if modal && !d.arena().is_connected(dialog) {
        return Err(status(Code::InvalidState).with_message("showModal() on a disconnected dialog"));
    }
    Ok(true)
}

/// `dialog.show()` (non-modal) or `dialog.showModal()`.
pub fn show(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    dialog: NodeId,
    modal: bool,
) -> Result<(), Status> {
    operation(host, |h| {
        if !eng(h, doc, |_, d| check_show(d, dialog, modal))?? {
            return Ok(());
        }
        // `beforetoggle` (cancelable); the state is checked again after it.
        if fire(
            h,
            doc,
            dialog,
            toggle_event_init("beforetoggle", false, true, true),
        )? {
            return Ok(());
        }
        if !eng(h, doc, |_, d| {
            d.arena().contains(dialog)
                && html_element(d, dialog, "dialog").is_some_and(|e| e.attr("open").is_none())
                && (!modal || d.arena().is_connected(dialog))
        })? {
            return Ok(());
        }
        eng(h, doc, |ix, d| {
            d.set_attribute(dialog, "open", "")?;
            if modal {
                ix.set_element_flag(d, dialog, UiFlags::MODAL | UiFlags::TOP_LAYER, true);
            }
            let focused = ix.focused_element(doc);
            if let Some(s) = ix.dialog_state(doc) {
                s.dialogs.entry(dialog).or_default().previously_focused = focused;
            }
            ix.queue_toggle_task(doc, dialog, false, true);
            Ok::<(), Status>(())
        })??;
        focusing_steps(h, doc, dialog)
    })
}

/// `dialog.close(result)`: the close steps (nothing when the dialog is not open).
pub fn close(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    dialog: NodeId,
    result: Option<&str>,
) -> Result<(), Status> {
    operation(host, |h| close_steps(h, doc, dialog, result))
}

fn is_open_dialog(d: &Document, dialog: NodeId) -> Result<bool, Status> {
    if !d.arena().contains(dialog) {
        return Err(status(Code::StaleHandle));
    }
    if crate::interaction::dispatch::is_closing(d) {
        return Err(status(Code::DocumentClosed));
    }
    let e = html_element(d, dialog, "dialog")
        .ok_or_else(|| status(Code::InvalidArgument).with_message("not a dialog element"))?;
    Ok(e.attr("open").is_some())
}

fn close_steps(
    h: &mut dyn DispatchHost,
    doc: DocumentId,
    dialog: NodeId,
    result: Option<&str>,
) -> Result<(), Status> {
    if !eng(h, doc, |_, d| is_open_dialog(d, dialog))?? {
        return Ok(());
    }
    // `beforetoggle` (not cancelable), then the state is checked again.
    fire(
        h,
        doc,
        dialog,
        toggle_event_init("beforetoggle", true, false, false),
    )?;
    if !eng(h, doc, |_, d| is_open_dialog(d, dialog).unwrap_or(false))? {
        return Ok(());
    }
    let restore = eng(h, doc, |ix, d| {
        let was_modal = d.ui_flags(dialog).is_ok_and(|f| f.contains(UiFlags::MODAL));
        let _ = d.remove_attribute(dialog, "open");
        // The attribute hook took it out of the top layer; clear anything left.
        ix.set_element_flag(d, dialog, UiFlags::MODAL | UiFlags::TOP_LAYER, false);
        let focused = ix.focused_element(doc);
        let focus_inside = focused.is_some_and(|f| d.arena().is_inclusive_ancestor(dialog, f));
        let st = ix
            .dialog_state(doc)
            .map(|s| s.dialogs.entry(dialog).or_default());
        let mut restore = None;
        if let Some(st) = st {
            if let Some(r) = result {
                st.return_value = r.to_string();
            }
            let prev = st.previously_focused.take();
            if was_modal || focus_inside || focused.is_none() {
                restore = prev;
            }
        }
        ix.queue_task(
            doc,
            ElementTask {
                node: dialog,
                kind: ElementTaskKind::Close,
            },
        );
        ix.queue_toggle_task(doc, dialog, true, false);
        restore
    })?;
    if let Some(p) = restore
        && eng(h, doc, |_, d| d.arena().contains(p) && is_focusable(d, p))?
    {
        focus_update(h, doc, Some(p), DispatchOrigin::Focus, FocusSource::Script)?;
    }
    Ok(())
}

/// `dialog.requestClose(result)` and the close request of Escape: `cancel` (cancelable, not
/// bubbling), then, unless canceled, the close steps.
pub fn request_close(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    dialog: NodeId,
    result: Option<&str>,
) -> Result<(), Status> {
    operation(host, |h| {
        if !eng(h, doc, |_, d| is_open_dialog(d, dialog))?? {
            return Ok(());
        }
        if fire(h, doc, dialog, EventInit::new("cancel").cancelable(true))? {
            return Ok(());
        }
        close_steps(h, doc, dialog, result)
    })
}

/// The `details` a summary is the summary for (HTML "summary for its parent details"): `summary`'s
/// parent is a `details` and `summary` is its first `summary` child.
pub fn summary_details(d: &Document, summary: NodeId) -> Option<NodeId> {
    html_element(d, summary, "summary")?;
    let parent = d.arena().get(summary)?.parent()?;
    html_element(d, parent, "details")?;
    let first = d
        .arena()
        .get(parent)?
        .children()
        .iter()
        .copied()
        .find(|&c| html_element(d, c, "summary").is_some())?;
    (first == summary).then_some(parent)
}

/// The summary's activation behaviour: toggle the parent `details`' `open` attribute.
pub(crate) fn toggle_details(d: &mut Document, summary: NodeId) {
    let Some(details) = summary_details(d, summary) else {
        return;
    };
    let open = html_element(d, details, "details").is_some_and(|e| e.attr("open").is_some());
    let _ = if open {
        d.remove_attribute(details, "open")
    } else {
        d.set_attribute(details, "open", "")
    };
}

/// Dispatch the queued element tasks of `doc` (turn step 4; one outermost operation each).
pub fn dispatch_element_tasks(host: &mut dyn DispatchHost, doc: DocumentId) {
    let tasks = rt(host, |ix| ix.take_element_tasks(doc));
    for t in tasks {
        let init = match t.kind {
            ElementTaskKind::Toggle { old_open, new_open } => {
                toggle_event_init("toggle", old_open, new_open, false)
            }
            ElementTaskKind::Close => EventInit::new("close"),
        };
        let _ = crate::interaction::element_events::dispatch_element_task(host, doc, t.node, init);
    }
}
