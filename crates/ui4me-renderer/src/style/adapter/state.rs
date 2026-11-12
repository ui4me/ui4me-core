//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Element-state composition (T041; 006 `selector-support.md` §3; AD-28). The style engine only
//! **reads** states; DocumentCore stores them (UI-state flags written by 012, control state,
//! attribute-derived state). This file composes Stylo's `ElementState` from those inputs, for the
//! current DOM and — from the journal's prior values — for snapshots, so the cascade and the
//! selector service see exactly the same state.
//!
//! Slice-verified (FR-016): `:focus` (UI flag), `:checked` (control checkedness). Also composed:
//! hover/active/focus-visible/focus-within, `:open`/`:modal` flags, `:indeterminate`,
//! `:enabled`/`:disabled` (the element's own `disabled` attribute or a disabled `fieldset` ancestor,
//! 015 HTML-3), `:link` (`a`/`area` with `href`), `:defined` (built-in HTML elements).

use stylo_dom::ElementState;
use ui4me_document::control_state::ControlFlags;
use ui4me_document::node::{Attribute, ElementData};
use ui4me_document::ui_state::UiFlags;

/// The inputs state depends on (current or prior).
pub(crate) struct StateInputs<'a> {
    pub(crate) element: &'a ElementData,
    pub(crate) attrs: &'a [Attribute],
    pub(crate) ui: UiFlags,
    pub(crate) control: Option<ControlFlags>,
    pub(crate) facts: super::form_state::FormFacts,
}

fn attr<'a>(attrs: &'a [Attribute], local: &str) -> Option<&'a str> {
    attrs.iter().find(|a| a.is(local)).map(|a| a.value.as_str())
}

pub(crate) fn compose(i: &StateInputs<'_>) -> ElementState {
    let mut s = ElementState::empty();
    let ui = i.ui;
    if ui.contains(UiFlags::FOCUS) {
        s |= ElementState::FOCUS;
    }
    if ui.contains(UiFlags::FOCUS_VISIBLE) {
        s |= ElementState::FOCUSRING;
    }
    if ui.contains(UiFlags::FOCUS_WITHIN) {
        s |= ElementState::FOCUS_WITHIN;
    }
    if ui.contains(UiFlags::HOVER) {
        s |= ElementState::HOVER;
    }
    if ui.contains(UiFlags::ACTIVE) {
        s |= ElementState::ACTIVE;
    }
    if ui.contains(UiFlags::MODAL) {
        s |= ElementState::MODAL;
    }
    if ui.contains(UiFlags::TOP_LAYER)
        && !ui.contains(UiFlags::MODAL)
        && attr(i.attrs, "popover").is_some()
        && i.element.is_html()
    {
        s |= ElementState::POPOVER_OPEN;
    }
    let e = i.element;
    let html = e.is_html();
    let local = e.local_name();
    if let Some(c) = i.control {
        if c.checked == Some(true) {
            s |= ElementState::CHECKED;
        }
        if c.indeterminate {
            s |= ElementState::INDETERMINATE;
        }
    }
    // An option is `:checked` by its stored selectedness (the select's selectedness setting algorithm,
    // 003), else by its `selected` attribute when it has no control state.
    if html
        && local == "option"
        && i.control.as_ref().map_or_else(
            || attr(i.attrs, "selected").is_some(),
            |c| c.selected == Some(true),
        )
    {
        s |= ElementState::CHECKED;
    }
    // `:disabled` includes the descendants of a disabled `fieldset` outside its first `legend` (HTML
    // 4.10.18.2; 015 HTML-3); `optgroup` and `option` follow their own attribute only.
    let fieldset = i.facts.fieldset_disabled
        && matches!(
            local,
            "button" | "input" | "select" | "textarea" | "fieldset"
        );
    if html
        && matches!(
            local,
            "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
        )
    {
        if attr(i.attrs, "disabled").is_some() || fieldset {
            s |= ElementState::DISABLED;
        } else {
            s |= ElementState::ENABLED;
        }
    }
    if html && matches!(local, "a" | "area") && attr(i.attrs, "href").is_some() {
        s |= ElementState::UNVISITED;
    }
    if html
        && ((local == "details" || local == "dialog") && attr(i.attrs, "open").is_some()
            || local == "select" && ui.contains(UiFlags::PICKER_OPEN))
    {
        s |= ElementState::OPEN;
    }
    if html {
        super::form_state::compose(&mut s, local, i.attrs, i.control, i.facts);
    }
    if html || !local.contains('-') {
        s |= ElementState::DEFINED;
    }
    s
}
