//! owner: 012-interaction — created by 001 slice (R-28)
//!
//! Focus (001 T059, T084): focusable areas, the focusing steps and focus update (`blur` and
//! `focusout` on the old element, then `focus` and `focusin` on the new one; trusted,
//! non-cancelable; see `focus_update`), focus moving on press, the synchronous fix-up on
//! removal (R-14, no events) and the COMMIT validity fix-up. Focus is written to 003's element-state table
//! only by InteractionRuntime through its `UiStateWriter` (AD-28).

use crate::interaction::dispatch::{
    DispatchHost, InteractionRuntime, eng, is_closing, operation, rt, run_dispatch, status,
};
use crate::interaction::frame::{EventInit, EventInterface};
use std::cell::RefCell;
use std::rc::Rc;
use ui4me_document::document::Document;
use ui4me_document::ui_state::UiFlags;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// What moved focus (FR-048: decides whether the new focus matches `:focus-visible`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusSource {
    /// Sequential navigation (Tab, Shift+Tab).
    Keyboard,
    /// A pointer press.
    Pointer,
    /// `focus()` / `blur()` from the host.
    Script,
}

/// Elements that accept keyboard text input (HTML: text-like `input` types including `number`,
/// and `textarea`): they match `:focus-visible` whatever moved focus (FR-048).
pub fn accepts_text_input(d: &Document, n: NodeId) -> bool {
    d.control_facts(n).is_some_and(|facts| facts.accepts_text())
}

/// Focus state shared with 003's removal hook.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusShared {
    /// The focused element; `None` = the viewport.
    pub focused: Option<NodeId>,
}

fn focus_flags() -> UiFlags {
    UiFlags::FOCUS | UiFlags::FOCUS_VISIBLE
}

fn is_element(d: &Document, n: NodeId) -> bool {
    d.arena().get(n).is_some_and(|x| x.is_element())
}

/// 003's removal hook (focus fix-up, R-14: synchronous, no events) and nodes-freed hook.
pub(crate) fn install_hooks(
    d: &mut Document,
    focus: Rc<RefCell<FocusShared>>,
    freed: Rc<RefCell<Vec<NodeId>>>,
) {
    d.hooks_mut().set_on_removed(Box::new(move |w, removal| {
        // A removed dialog leaves the top layer (HTML "dialog removing steps"; 015 HTML-1).
        for n in w.flagged_in_subtree(removal.root, UiFlags::MODAL | UiFlags::TOP_LAYER) {
            if let Ok(f) = w.flags(n) {
                let _ = w.set_flags(n, f & !(UiFlags::MODAL | UiFlags::TOP_LAYER));
            }
        }
        let Some(el) = focus.borrow().focused else {
            return;
        };
        if !w.document().arena().is_inclusive_ancestor(removal.root, el) {
            return;
        }
        for n in w.flagged_in_subtree(removal.root, focus_flags() | UiFlags::FOCUS_WITHIN) {
            if let Ok(f) = w.flags(n) {
                let _ = w.set_flags(n, f & !(focus_flags() | UiFlags::FOCUS_WITHIN));
            }
        }
        let mut a = Some(removal.old_parent);
        while let Some(n) = a {
            if is_element(w.document(), n)
                && let Ok(f) = w.flags(n)
            {
                let _ = w.set_flags(n, f & !UiFlags::FOCUS_WITHIN);
            }
            a = w.document().arena().get(n).and_then(|x| x.parent());
        }
        focus.borrow_mut().focused = None;
    }));
    d.hooks_mut().set_on_nodes_freed(Box::new(move |_doc, ids| {
        freed.borrow_mut().extend_from_slice(ids)
    }));
}

/// Is `n` a focusable area (012 FR-044, slice subset: `a`/`area` with `href`; `button`,
/// `input` (not hidden), `select`, `textarea` without `disabled` (its own attribute or a disabled
/// `fieldset` ancestor, HTML 4.10.18.2); any element with a valid `tabindex`; the summary of a
/// `details`; an open `dialog`, which the dialog focusing steps focus when it holds nothing
/// focusable, as the reference browser does). An inert node (the `inert` attribute, or outside the
/// modal dialog while one is open; 015 HTML-1) is not focusable. `display: none` is 012's later work.
pub fn is_focusable(d: &Document, n: NodeId) -> bool {
    let Some(e) = d.arena().get(n).and_then(|x| x.as_element()) else {
        return false;
    };
    if !e.is_html() || !d.arena().is_connected(n) || d.is_inert(n) {
        return false;
    }
    if crate::interaction::activation::is_disabled_form_control(d, n) {
        return false;
    }
    if e.attr("tabindex")
        .is_some_and(|t| t.trim().parse::<i64>().is_ok())
    {
        return true;
    }
    let disabled = || crate::interaction::activation::is_disabled_form_control(d, n);
    match e.local_name() {
        "a" | "area" => e.attr("href").is_some(),
        "button" | "select" | "textarea" => !disabled(),
        "input" => {
            !disabled()
                && !e
                    .attr("type")
                    .is_some_and(|t| t.trim().eq_ignore_ascii_case("hidden"))
        }
        "summary" => crate::interaction::dialog::summary_details(d, n).is_some(),
        "dialog" => e.attr("open").is_some(),
        // A plain-text editing host (0.8 F0801).
        _ => crate::document::editable::is_editing_host(d, n),
    }
}

/// Nearest focusable inclusive ancestor (the focus target of a press), `None` = viewport.
pub(crate) fn click_focus_target(d: &Document, target: NodeId) -> Option<NodeId> {
    let mut n = Some(target);
    while let Some(x) = n {
        if is_focusable(d, x) {
            return Some(x);
        }
        n = d.arena().get(x).and_then(|y| y.parent());
    }
    None
}

impl InteractionRuntime {
    /// Write the focused element and its UI flags (`focus` on the element, `focus-within` on
    /// its inclusive ancestors) through the document's UiStateWriter.
    pub(crate) fn set_focus(&mut self, d: &mut Document, new: Option<NodeId>, visible: bool) {
        let doc = d.id();
        let Some(s) = self.docs.get(&doc) else { return };
        let old = s.focus.borrow().focused;
        if let Ok(mut w) = d.ui_state_writer(&s.ui_key) {
            let chain =
                |w: &mut ui4me_document::ui_state::UiStateWriter<'_>, from: NodeId, set: bool| {
                    let mut a = Some(from);
                    while let Some(n) = a {
                        if is_element(w.document(), n)
                            && let Ok(f) = w.flags(n)
                        {
                            let mut g = if set {
                                f | UiFlags::FOCUS_WITHIN
                            } else {
                                f & !UiFlags::FOCUS_WITHIN
                            };
                            if n == from {
                                g = if set {
                                    g | UiFlags::FOCUS
                                        | if visible {
                                            UiFlags::FOCUS_VISIBLE
                                        } else {
                                            UiFlags::empty()
                                        }
                                } else {
                                    g & !focus_flags()
                                };
                            }
                            let _ = w.set_flags(n, g);
                        }
                        a = w.document().arena().get(n).and_then(|x| x.parent());
                    }
                };
            if let Some(o) = old.filter(|o| w.document().arena().contains(*o)) {
                chain(&mut w, o, false);
            }
            if let Some(n) = new {
                chain(&mut w, n, true);
            }
        }
        s.focus.borrow_mut().focused = new;
        self.note_doc(doc);
    }
}

/// COMMIT focus validity fix-up: a focused element that is no longer a focusable area moves
/// focus to the viewport, with no events.
pub(crate) fn validity_fixup(ix: &mut InteractionRuntime, d: &mut Document) {
    let Some(f) = ix.focused_element(d.id()) else {
        return;
    };
    if !is_focusable(d, f) {
        ix.set_focus(d, None, false);
    }
}

/// HTML `focus()`: the focusing steps. `FOCUS_MOVE_REJECTED` if `element` is not a focusable
/// area (no events). Already focused → no events.
pub fn focus(host: &mut dyn DispatchHost, doc: DocumentId, element: NodeId) -> Result<(), Status> {
    operation(host, |h| {
        let focusable = eng(h, doc, |_, d| {
            if !d.arena().contains(element) {
                return Err(status(Code::StaleHandle));
            }
            if is_closing(d) {
                return Err(status(Code::DocumentClosed));
            }
            Ok(is_focusable(d, element))
        })??;
        if !focusable || !h.focus_target_visible(doc, element)? {
            return Err(status(Code::FocusMoveRejected).with_message("not a focusable area"));
        }
        focus_update(
            h,
            doc,
            Some(element),
            DispatchOrigin::Focus,
            FocusSource::Script,
        )
    })
}

/// HTML `blur()`: the unfocusing steps (focus moves to the viewport) if `element` is focused.
pub fn blur(host: &mut dyn DispatchHost, doc: DocumentId, element: NodeId) -> Result<(), Status> {
    operation(host, |h| {
        if rt(h, |ix| ix.focused_element(doc)) != Some(element) {
            return Ok(());
        }
        focus_update(h, doc, None, DispatchOrigin::Focus, FocusSource::Script)
    })
}

/// `blur`/`focus` (non-bubbling) and `focusout`/`focusin` (bubbling); none is cancelable.
fn focus_event(ty: &str, related: Option<NodeId>) -> EventInit {
    let bubbles = matches!(ty, "focusin" | "focusout");
    EventInit::new(ty)
        .interface(EventInterface::FocusEvent)
        .bubbles(bubbles)
        .related_target(related)
}

/// Focus update steps (HTML `source:87343-87467` @6d7788b; `new = None` = viewport): `blur` at
/// the old element (after its flags are cleared, so the focused element is already the viewport
/// while `blur` listeners run), `focusout` at the old element, then the focused element is
/// updated, then `focus` and `focusin` at the new one. All four are trusted FocusEvents and not
/// cancelable; `blur`/`focus` do not bubble, `focusout`/`focusin` do; inside a callback they
/// nest one level deeper. `focusout`/`focusin` follow Chromium's positions (directly after
/// `blur`/`focus`), which are provisional and not compared (X-SPECS; slice-scenario §3 GE-4).
/// Removal of the focused element never comes here: 003's removal hook clears focus
/// synchronously with no events (R-14).
pub(crate) fn focus_update(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    new: Option<NodeId>,
    origin: DispatchOrigin,
    source: FocusSource,
) -> Result<(), Status> {
    let new = match new {
        Some(node) if !host.focus_target_visible(doc, node)? => None,
        other => other,
    };
    let old = rt(host, |ix| ix.focused_element(doc));
    if old == new {
        return Ok(());
    }
    // FR-048, decided before the old element's flags are cleared.
    let visible = match new {
        None => false,
        Some(n) => eng(host, doc, |ix, d| match source {
            FocusSource::Keyboard => true,
            FocusSource::Pointer => accepts_text_input(d, n),
            FocusSource::Script => {
                accepts_text_input(d, n)
                    || old.is_some_and(|o| {
                        d.ui_flags(o)
                            .is_ok_and(|f| f.contains(UiFlags::FOCUS_VISIBLE))
                    })
                    || ix.last_interaction_keyboard(doc)
            }
        })?,
    };
    if let Some(o) = old {
        // `change` precedes `blur` for an edited text control (editing.md section 6).
        let _ = crate::interaction::editing::on_blur(host, doc, o, origin);
        let _ = crate::interaction::controls::on_blur(host, doc, o);
        let alive = eng(host, doc, |ix, d| {
            ix.set_focus(d, None, false);
            d.arena().contains(o)
        })?;
        if alive {
            let _ = run_dispatch(host, doc, o, focus_event("blur", new), true, origin, None);
            if eng(host, doc, |_, d| d.arena().contains(o))? {
                let _ = run_dispatch(
                    host,
                    doc,
                    o,
                    focus_event("focusout", new),
                    true,
                    origin,
                    None,
                );
            }
        }
    }
    if let Some(n) = new {
        let ok = eng(host, doc, |ix, d| {
            // A blur listener may have moved focus elsewhere; the later request wins.
            let ok = is_focusable(d, n) && ix.focused_element(doc).is_none();
            if ok {
                ix.set_focus(d, Some(n), visible);
            }
            ok
        })?;
        if ok {
            let _ = run_dispatch(host, doc, n, focus_event("focus", old), true, origin, None);
            if eng(host, doc, |_, d| d.arena().contains(n))? {
                let _ = run_dispatch(
                    host,
                    doc,
                    n,
                    focus_event("focusin", old),
                    true,
                    origin,
                    None,
                );
            }
            let _ = crate::interaction::editing::on_focus(
                host,
                doc,
                n,
                source == FocusSource::Keyboard,
                origin,
            );
        }
    }
    Ok(())
}
