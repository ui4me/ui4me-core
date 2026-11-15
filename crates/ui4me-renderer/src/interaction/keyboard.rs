//! owner: 012-interaction (lane L-INT-r3: US4/US7 essentials; FR-042, FR-045, FR-087)
//!
//! Key default actions of an uncanceled `keydown`/`keyup`: sequential focus navigation (Tab,
//! Shift+Tab), button-like activation (Enter on keydown; Space on keyup, `:active` between
//! the two; a link follows Enter only, a checkbox or radio Space only, an element with only an
//! ARIA `role=button` neither, as in browsers) and keyboard scrolling of the nearest scroll container (arrows 40 px, PageUp /
//! PageDown / Space by 87.5 % of the scrollport, Home / End to the edges), unless the focused
//! element consumes the key.
//!
//! Radio-group roving and arrow keys inside controls live in `controls`. Not done: the
//! sequential navigation starting point after a click, access keys.

use crate::interaction::activation;
use crate::interaction::dispatch::{DispatchHost, eng, run_dispatch};
use crate::interaction::focus::{FocusSource, focus_update, is_focusable};
use crate::interaction::frame::{EventInit, EventInterface, Modifiers};
use crate::interaction::input_queue::KeyData;
use crate::interaction::scroll::{ARROW_STEP_PX, PAGE_FRACTION, pick, scroll_container_user};
use ui4me_document::document::Document;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// The key a Ctrl/Meta shortcut matches (F-17b / INTERACT-06): the layout's `key`, unless the
/// layout produces no Latin letter for it (Cyrillic, Greek, Hebrew, Arabic, ...), when the letter
/// of the physical key (`code` `KeyA`..`KeyZ`, the US position) stands in for it, as Chromium, GTK
/// and Qt do. A Latin layout keeps its own letters (Dvorak `key` wins over `code`); named keys and
/// keys without Ctrl/Meta are untouched. So are keys with Alt: AltGr arrives as Ctrl+Alt on Windows,
/// and AltGr+A typing "ą" is not Ctrl+A (no engine shortcut takes Alt; renderer review #4).
pub(crate) fn shortcut_key<'a>(
    key: &'a str,
    code: &str,
    m: Modifiers,
) -> std::borrow::Cow<'a, str> {
    use std::borrow::Cow;
    if !(m.ctrl || m.meta) || m.alt {
        return Cow::Borrowed(key);
    }
    let mut chars = key.chars();
    let single_non_latin = matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_ascii());
    if !single_non_latin {
        return Cow::Borrowed(key);
    }
    match code.strip_prefix("Key").map(str::as_bytes) {
        Some([c]) if c.is_ascii_uppercase() => {
            Cow::Owned((c.to_ascii_lowercase() as char).to_string())
        }
        _ => Cow::Borrowed(key),
    }
}

fn tabindex(d: &Document, n: NodeId) -> Option<i64> {
    d.arena()
        .get(n)
        .and_then(|x| x.as_element())
        .and_then(|e| e.attr("tabindex"))
        .and_then(|t| t.trim().parse::<i64>().ok())
}

/// The tabindex-ordered navigation scope (FR-045): positive tabindex ascending (ties in tree
/// order), then tabindex 0 and default-focusable elements in tree order; negative tabindex
/// is not reachable by Tab.
pub fn sequential_order(d: &Document) -> Vec<NodeId> {
    let mut all = Vec::new();
    collect(d, d.document_node(), &mut all);
    let mut positive: Vec<(i64, usize, NodeId)> = Vec::new();
    let mut zero: Vec<NodeId> = Vec::new();
    for (i, n) in all.into_iter().enumerate() {
        if !is_focusable(d, n) || skipped_radio(d, n) {
            continue;
        }
        // A dialog is focused by the dialog focusing steps only, never by Tab (its tabIndex is -1).
        let dialog = tabindex(d, n).is_none() && crate::interaction::dialog::is_dialog(d, n);
        match tabindex(d, n) {
            _ if dialog => {}
            Some(t) if t < 0 => {}
            Some(t) if t > 0 => positive.push((t, i, n)),
            _ => zero.push(n),
        }
    }
    positive.sort();
    positive
        .into_iter()
        .map(|(_, _, n)| n)
        .chain(zero)
        .collect()
}

/// A radio of a named group Tab passes over (HTML: the group has one tab stop): the checked radio is
/// it, else the group's first focusable radio.
fn skipped_radio(d: &Document, n: NodeId) -> bool {
    if !activation::is_radio(d, n) {
        return false;
    }
    let group = d.radio_group(n);
    if group.len() <= 1 {
        return false;
    }
    let checked = |r: &NodeId| {
        d.control_state(*r)
            .ok()
            .flatten()
            .and_then(|c| c.checkedness)
            == Some(true)
    };
    match group.iter().find(|r| checked(r) && is_focusable(d, **r)) {
        Some(c) => *c != n,
        None => group
            .iter()
            .find(|r| is_focusable(d, **r))
            .is_some_and(|first| *first != n),
    }
}

fn collect(d: &Document, n: NodeId, out: &mut Vec<NodeId>) {
    if d.arena().get(n).is_some_and(|x| x.is_element()) {
        out.push(n);
    }
    if let Ok(children) = d.children(n) {
        for c in children {
            collect(d, *c, out);
        }
    }
}

/// The element Tab moves to from `current` (`None` = nothing focused), wrapping at both ends.
pub fn next_in_order(order: &[NodeId], current: Option<NodeId>, backwards: bool) -> Option<NodeId> {
    if order.is_empty() {
        return None;
    }
    let pos = current.and_then(|c| order.iter().position(|n| *n == c));
    let i = match (pos, backwards) {
        (Some(p), false) => (p + 1) % order.len(),
        (Some(p), true) => (p + order.len() - 1) % order.len(),
        (None, false) => 0,
        (None, true) => order.len() - 1,
    };
    Some(order[i])
}

/// Elements that keep arrows, Home/End and Space for themselves (FR-087).
fn consumes_scroll_keys(d: &Document, n: NodeId, space: bool) -> bool {
    let Some(e) = d
        .arena()
        .get(n)
        .and_then(|x| x.as_element())
        .filter(|e| e.is_html())
    else {
        return false;
    };
    let ty = e.attr("type").map(|t| t.trim().to_ascii_lowercase());
    match e.local_name() {
        "textarea" | "select" => true,
        "button" => space,
        "input" => match ty.as_deref() {
            Some("checkbox" | "button" | "submit" | "reset" | "image") => space,
            Some("radio" | "range") => true,
            _ => true,
        },
        _ => e
            .attr("contenteditable")
            .is_some_and(|v| !v.eq_ignore_ascii_case("false")),
    }
}

/// Button-like elements Enter/Space activate with a click (HTML activation behaviours; a `details`
/// summary too, 015 HTML-2).
fn is_button_like(d: &Document, n: NodeId) -> bool {
    activation::has_activation_behaviour(d, n)
        && d.arena()
            .get(n)
            .and_then(|x| x.as_element())
            .is_some_and(|e| matches!(e.local_name(), "button" | "input" | "summary"))
}

/// A hyperlink (`a` / `area` with `href`): Enter follows it with a click on keydown, as in
/// browsers; Space does not activate it (it scrolls).
fn is_link(d: &Document, n: NodeId) -> bool {
    d.arena()
        .get(n)
        .and_then(|x| x.as_element())
        .is_some_and(|e| {
            e.is_html() && matches!(e.local_name(), "a" | "area") && e.attr("href").is_some()
        })
}

fn is_checkbox_or_radio(d: &Document, n: NodeId) -> bool {
    d.arena()
        .get(n)
        .and_then(|x| x.as_element())
        .is_some_and(|e| {
            e.local_name() == "input"
                && matches!(
                    e.attr("type")
                        .map(|t| t.trim().to_ascii_lowercase())
                        .as_deref(),
                    Some("checkbox" | "radio")
                )
        })
}

fn click_init(m: Modifiers) -> EventInit {
    let mut e = EventInit::new("click")
        .interface(EventInterface::PointerEvent)
        .bubbles(true)
        .cancelable(true);
    e.detail = 1;
    e.modifiers = m;
    e
}

/// Run the default action of an uncanceled `keydown` at `target` (the focused element or the
/// body). `focused` says whether `target` is the focused element.
#[allow(clippy::too_many_arguments)]
pub(crate) fn keydown_default(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    focused: bool,
    k: &KeyData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    if m.ctrl || m.alt || m.meta || k.is_composing {
        return Ok(());
    }
    match k.key.as_str() {
        "Tab" => {
            let (candidates, current) = eng(host, doc, |ix, d| {
                (sequential_order(d), ix.focused_element(doc))
            })?;
            let mut order = Vec::with_capacity(candidates.len());
            for node in candidates {
                if host.focus_target_visible(doc, node)? {
                    order.push(node);
                }
            }
            if let Some(next) = next_in_order(&order, current, m.shift) {
                focus_update(host, doc, Some(next), origin, FocusSource::Keyboard)?;
            }
        }
        // A close request for the topmost modal dialog (HTML close watchers; 015 HTML-1).
        "Escape" => {
            crate::interaction::popover::escape(host, doc)?;
        }
        "Enter" if focused => {
            if eng(host, doc, |_, d| {
                (is_button_like(d, target)
                    && !is_checkbox_or_radio(d, target)
                    && !activation::is_disabled_form_control(d, target))
                    || is_link(d, target)
            })? {
                run_dispatch(host, doc, target, click_init(m), true, origin, ts)?;
            }
        }
        " " | "Spacebar"
            if focused
                && eng(host, doc, |_, d| {
                    is_button_like(d, target) && !activation::is_disabled_form_control(d, target)
                })? =>
        {
            // Space presses the button: `:active` until the matching keyup (FR-042).
            eng(host, doc, |ix, d| ix.set_active(d, Some(target)))?;
        }
        key => scroll_key(host, doc, target, focused, key, m)?,
    }
    Ok(())
}

/// Keyboard scrolling (FR-087), when the focused element does not consume the key.
fn scroll_key(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    focused: bool,
    key: &str,
    m: Modifiers,
) -> Result<(), Status> {
    let space = matches!(key, " " | "Spacebar");
    if !matches!(
        key,
        "ArrowDown"
            | "ArrowUp"
            | "ArrowLeft"
            | "ArrowRight"
            | "PageDown"
            | "PageUp"
            | "Home"
            | "End"
    ) && !space
    {
        return Ok(());
    }
    if focused && eng(host, doc, |_, d| consumes_scroll_keys(d, target, space))? {
        return Ok(());
    }
    enum Amount {
        Step,
        Page,
        Edge,
    }
    let (dir_x, dir_y, amount) = match key {
        "ArrowDown" => (0.0, 1.0, Amount::Step),
        "ArrowUp" => (0.0, -1.0, Amount::Step),
        "ArrowRight" => (1.0, 0.0, Amount::Step),
        "ArrowLeft" => (-1.0, 0.0, Amount::Step),
        "PageDown" => (0.0, 1.0, Amount::Page),
        "PageUp" => (0.0, -1.0, Amount::Page),
        "Home" => (0.0, -1.0, Amount::Edge),
        "End" => (0.0, 1.0, Amount::Edge),
        _ => (0.0, if m.shift { -1.0 } else { 1.0 }, Amount::Page), // Space / Shift+Space
    };
    let chain = host.scroll_chain(doc, target)?;
    let Some(info) = pick(&chain, dir_x, dir_y) else {
        return Ok(());
    };
    let (dx, dy) = match amount {
        Amount::Step => (dir_x * ARROW_STEP_PX, dir_y * ARROW_STEP_PX),
        Amount::Page => (0.0, dir_y * (info.scrollport.1 * PAGE_FRACTION).max(1.0)),
        Amount::Edge => (
            0.0,
            (if dir_y > 0.0 { info.max.1 } else { info.min.1 }) - info.offset.1,
        ),
    };
    scroll_container_user(host, doc, &info, dx, dy, true)?;
    Ok(())
}

/// Run the default action of a `keyup` (Space releases a pressed button: click).
#[allow(clippy::too_many_arguments)]
pub(crate) fn keyup_default(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    focused: bool,
    k: &KeyData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    if !matches!(k.key.as_str(), " " | "Spacebar") {
        return Ok(());
    }
    let pressed = eng(host, doc, |ix, d| {
        let p = ix.active_press_element(doc);
        if p.is_some() {
            ix.set_active(d, None);
        }
        p
    })?;
    if focused
        && pressed == Some(target)
        && eng(host, doc, |_, d| {
            d.arena().contains(target)
                && is_button_like(d, target)
                && !activation::is_disabled_form_control(d, target)
        })?
    {
        run_dispatch(host, doc, target, click_init(m), true, origin, ts)?;
    }
    Ok(())
}
