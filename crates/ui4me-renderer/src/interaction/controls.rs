//! owner: 012-interaction (lane L-CTRL-r10; `contracts/controls.md` sections 1, 3 and 5) — the
//! interaction of the form controls other than text editing:
//!
//! * `select`: a drop-down opens a popup (the `picker-open` flag and the highlight and scroll state
//!   in 003's control state; the popup itself is laid out and painted by 008 and 010 as a layered
//!   panel inside the window, since 011 has no separate popup windows) on a press, Enter, Space or
//!   Alt+Down; arrows, Home, End, PageUp/PageDown and type-ahead move through the options, Enter,
//!   Space or a release over an option commits it, Escape or a press elsewhere closes it. A closed
//!   drop-down changes with the arrows and type-ahead directly. A listbox (display size above 1) picks by press (Ctrl toggles, Shift extends in a `multiple`) and by key. A
//!   commit that changes the selection fires `input` then `change` at the select.
//! * `input type=range`: arrows, PageUp/PageDown, Home and End, and the pointer on the track and
//!   thumb (`input` while dragging, `change` on release).
//! * `input type=number`: ArrowUp and ArrowDown step the value by `step` within `min`/`max`.
//! * `input type=radio`: arrow keys move through the group, checking the next radio with a click.
//!
//! Range details (F0805): `readonly` does not apply to `range` (HTML), so only `disabled` blocks it;
//! under `dir=rtl` (the `dir` attribute chain, not CSS `direction`) ArrowLeft increases and the
//! track is mirrored. A number accepts only the HTML floating-point syntax (`.` decimal separator, no
//! locale grouping); the wheel never changes a number (documented choice, no spin buttons).
//!
//! Not done (reported): a `select` Shift+click range in a single listbox, rubber-band drag
//! selection in a listbox, a popup that flips above a control at the window's bottom edge, the
//! number spin buttons and wheel stepping.

use crate::interaction::dispatch::{DispatchHost, InteractionRuntime, eng, rt, run_dispatch};
use crate::interaction::focus::{FocusSource, focus_update};
use crate::interaction::frame::{EventInit, EventInterface, Modifiers};
use crate::interaction::input_queue::{KeyData, PointerData};
use ui4me_document::control_values::format_float;
use ui4me_document::document::Document;
use ui4me_document::ui_state::UiFlags;
use ui4me_engine_types::control_inner::{POPUP_MAX_ROWS, SelectGeom, SelectRowGeom};
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// Type-ahead buffer reset after this long without a key (ms).
const TYPEAHEAD_MS: f64 = 1000.0;

/// An element drag in progress: a `range` thumb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RangeDrag {
    pub node: NodeId,
    /// The value changed since the press (`change` fires on release).
    pub changed: bool,
}

/// Per-document control interaction state.
#[derive(Debug, Default)]
pub(crate) struct ControlsState {
    /// The select whose popup is open.
    pub picker: Option<NodeId>,
    pub typeahead: String,
    pub typeahead_at: f64,
    pub range_drag: Option<RangeDrag>,
    /// Anchor option index of a `multiple` listbox's Shift extension.
    pub anchor: Option<u32>,
}

fn html_named(d: &Document, n: NodeId, name: &str) -> bool {
    d.arena()
        .get(n)
        .and_then(|x| x.as_element())
        .is_some_and(|e| e.is_html_named(name))
}

fn attr<'a>(d: &'a Document, n: NodeId, name: &str) -> Option<&'a str> {
    d.arena()
        .get(n)
        .and_then(|x| x.as_element())
        .and_then(|e| e.attr(name))
}

fn input_type(d: &Document, n: NodeId) -> Option<String> {
    d.input_type(n).map(|kind| kind.as_str().to_owned())
}

fn is_disabled(d: &Document, n: NodeId) -> bool {
    d.control_facts(n).is_some_and(|facts| facts.disabled)
}

fn ev(ty: &str) -> EventInit {
    EventInit::new(ty).bubbles(true).cancelable(false)
}

/// `input` then `change` at `node`.
fn fire_input_change(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    // A number step commits itself: its blur must not fire a second `change`.
    if let Some(v) = eng(host, doc, |_, d| d.control_value(node))? {
        rt(host, |ix| {
            crate::interaction::editing::set_baseline(ix, doc, node, &v)
        });
    }
    run_dispatch(host, doc, node, ev("input"), true, origin, ts)?;
    if eng(host, doc, |_, d| d.arena().contains(node))? {
        run_dispatch(host, doc, node, ev("change"), true, origin, ts)?;
    }
    Ok(())
}

impl InteractionRuntime {
    /// Set or clear one UI flag on one element (no ancestors).
    pub(crate) fn set_element_flag(
        &mut self,
        d: &mut Document,
        el: NodeId,
        flag: UiFlags,
        on: bool,
    ) {
        let doc = d.id();
        let Some(s) = self.docs.get(&doc) else { return };
        if let Ok(mut w) = d.ui_state_writer(&s.ui_key)
            && let Ok(f) = w.flags(el)
        {
            let g = if on { f | flag } else { f & !flag };
            if g != f {
                let _ = w.set_flags(el, g);
            }
        }
        self.note_doc(doc);
    }

    pub(crate) fn controls_state(&mut self, doc: DocumentId) -> Option<&mut ControlsState> {
        self.docs.get_mut(&doc).map(|s| &mut s.controls)
    }

    /// The select whose popup is open in `doc`.
    pub fn open_picker(&self, doc: DocumentId) -> Option<NodeId> {
        self.docs.get(&doc).and_then(|s| s.controls.picker)
    }
}

// ----------------------------------------------------------------------------- select model

fn is_dropdown(d: &Document, n: NodeId) -> bool {
    html_named(d, n, "select") && d.select_is_dropdown(n)
}

fn is_listbox(d: &Document, n: NodeId) -> bool {
    html_named(d, n, "select") && !d.select_is_dropdown(n)
}

/// Visible-row capacity of the popup or listbox of `select`.
fn capacity(d: &Document, select: NodeId) -> usize {
    if d.select_is_dropdown(select) {
        POPUP_MAX_ROWS
    } else {
        d.select_display_size(select).max(1) as usize
    }
}

fn ensure_visible(row: usize, scroll: u32, total: usize, cap: usize) -> u32 {
    let mut s = scroll as usize;
    if row < s {
        s = row;
    } else if row >= s + cap {
        s = row + 1 - cap;
    }
    s.min(total.saturating_sub(cap)) as u32
}

/// Option indexes of the options a user can pick (shown and enabled), in order.
fn pickable(d: &Document, select: NodeId) -> Vec<u32> {
    d.select_rows(select)
        .into_iter()
        .filter_map(|r| r.option.zip(r.option_index))
        .filter(|(o, _)| !d.option_disabled(*o))
        .map(|(_, i)| i)
        .collect()
}

fn row_of_option(d: &Document, select: NodeId, option_index: u32) -> usize {
    d.select_rows(select)
        .iter()
        .position(|r| r.option_index == Some(option_index))
        .unwrap_or(0)
}

/// Write the highlight (active option) and keep it visible.
fn set_highlight(d: &mut Document, select: NodeId, option_index: Option<u32>) {
    let total = d.select_rows(select).len();
    let cap = capacity(d, select);
    let cur = d
        .control_state(select)
        .ok()
        .flatten()
        .map_or(0, |c| c.picker_scroll);
    let scroll = option_index.map_or(cur, |i| {
        ensure_visible(row_of_option(d, select, i), cur, total, cap)
    });
    let _ = d
        .control_state_writer()
        .set_picker(select, option_index, scroll);
}

/// Keep a listbox option visible using the same pixel scroll offset as wheel,
/// programmatic scrolling, painting and hit testing. A non-scrollable listbox
/// (`overflow: visible` or `clip`) has no scroll target.
fn reveal_listbox_option(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    select: NodeId,
    option: u32,
) -> Result<(), Status> {
    let Some(g) = host.control_geometry(doc, select)? else {
        return Ok(());
    };
    let Some(rows) = g.inner.select.as_ref().filter(|s| !s.popup) else {
        return Ok(());
    };
    let Some(row) = rows.rows.iter().find(|r| r.option == Some(option)) else {
        return Ok(());
    };
    let Some(info) = host.scroll_info(doc, Some(select))? else {
        return Ok(());
    };
    let top = f64::from(row.y) - g.inner.viewport.origin.y.to_f64();
    let bottom = top + f64::from(row.height);
    let dy = if top < info.offset.1 {
        top - info.offset.1
    } else if bottom > info.offset.1 + info.scrollport.1 {
        bottom - info.offset.1 - info.scrollport.1
    } else {
        0.0
    };
    if dy != 0.0 {
        crate::interaction::scroll::scroll_container_by(host, doc, &info, 0.0, dy, true)?;
    }
    Ok(())
}

fn highlight_of(d: &Document, select: NodeId) -> Option<u32> {
    d.control_state(select)
        .ok()
        .flatten()
        .and_then(|c| c.picker_highlight)
}

fn open_picker_state(ix: &mut InteractionRuntime, d: &mut Document, select: NodeId) {
    let doc = d.id();
    if !is_dropdown(d, select) || is_disabled(d, select) || !d.arena().is_connected(select) {
        return;
    }
    if let Some(prev) = ix.open_picker(doc).filter(|p| *p != select) {
        close_picker_state(ix, d, prev);
    }
    let sel = d.select_selected_index(select);
    let pick = pickable(d, select);
    let hl = if sel >= 0 {
        Some(sel as u32)
    } else {
        pick.first().copied()
    };
    let _ = d.control_state_writer().set_picker(select, hl, 0);
    set_highlight(d, select, hl);
    ix.set_element_flag(d, select, UiFlags::PICKER_OPEN, true);
    if let Some(c) = ix.controls_state(doc) {
        c.picker = Some(select);
    }
}

fn close_picker_state(ix: &mut InteractionRuntime, d: &mut Document, select: NodeId) {
    let doc = d.id();
    if d.arena().contains(select) {
        let _ = d.control_state_writer().set_picker(select, None, 0);
        ix.set_element_flag(d, select, UiFlags::PICKER_OPEN, false);
    }
    if let Some(c) = ix.controls_state(doc)
        && c.picker == Some(select)
    {
        c.picker = None;
    }
}

/// Close a popup when a select changes from a menulist to a listbox, or its
/// multiple-selection mode changes. UI-state writes participate in DOM batch
/// rollback; the interaction cache is reconciled lazily at the next input.
pub(crate) fn on_attribute_changed(
    w: &mut ui4me_document::ui_state::UiStateWriter<'_>,
    c: &ui4me_document::hooks::AttributeChange,
) {
    if !matches!(c.local_name.as_str(), "size" | "multiple") {
        return;
    }
    let d = w.document();
    if !html_named(d, c.element, "select")
        || (c.local_name == "size" && d.select_is_dropdown(c.element))
    {
        return;
    }
    if let Ok(flags) = w.flags(c.element)
        && flags.contains(UiFlags::PICKER_OPEN)
    {
        let _ = w.set_flags(c.element, flags & !UiFlags::PICKER_OPEN);
    }
}

/// Attribute mutations can clear the transactional UI flag while this cache
/// still names the old picker. Clear the cache before processing input; rolled
/// back batches restore the flag and consequently keep the picker intact.
fn sync_picker(host: &mut dyn DispatchHost, doc: DocumentId) -> Result<(), Status> {
    if rt(host, |ix| ix.open_picker(doc)).is_none() {
        return Ok(());
    }
    eng(host, doc, |ix, d| {
        if let Some(n) = ix.open_picker(doc)
            && !d
                .ui_flags(n)
                .is_ok_and(|f| f.contains(UiFlags::PICKER_OPEN))
            && let Some(s) = ix.controls_state(doc)
        {
            s.picker = None;
        }
    })
}

/// The popup closes when its select loses focus (no commit).
pub(crate) fn on_blur(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
) -> Result<(), Status> {
    eng(host, doc, |ix, d| {
        if ix.open_picker(doc) == Some(node) {
            close_picker_state(ix, d, node);
        }
    })
}

/// Select `option_index` as a user pick. Returns whether the selection changed.
fn pick(d: &mut Document, select: NodeId, option_index: u32, toggle: bool) -> bool {
    let Some(o) = d.select_options(select).get(option_index as usize).copied() else {
        return false;
    };
    if d.option_disabled(o) {
        return false;
    }
    d.select_user_pick(o, toggle).unwrap_or(false)
}

/// A selection change: `input` then `change`.
fn commit(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    select: NodeId,
    option_index: u32,
    toggle: bool,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<bool, Status> {
    let changed = eng(host, doc, |_, d| pick(d, select, option_index, toggle))?;
    if changed {
        fire_input_change(host, doc, select, origin, ts)?;
    }
    Ok(changed)
}

fn printable(k: &KeyData) -> Option<char> {
    let t = k.text.as_deref().unwrap_or(&k.key);
    let mut it = t.chars();
    match (it.next(), it.next()) {
        (Some(c), None) if !c.is_control() => Some(c),
        _ => None,
    }
}

/// Type-ahead: the option whose label starts with the typed prefix, searching from the option after
/// the current one (a repeated single character cycles).
fn typeahead_target(
    d: &Document,
    select: NodeId,
    buffer: &str,
    current: Option<u32>,
) -> Option<u32> {
    let cands: Vec<(u32, String)> = d
        .select_rows(select)
        .into_iter()
        .filter_map(|r| r.option.zip(r.option_index))
        .filter(|(o, _)| !d.option_disabled(*o))
        .map(|(o, i)| (i, d.option_label(o).trim_start().to_lowercase()))
        .collect();
    let needle = buffer.to_lowercase();
    let cycle = needle.chars().count() > 1 && needle.chars().all(|c| needle.starts_with(c));
    let needle = if cycle {
        needle.chars().take(1).collect::<String>()
    } else {
        needle
    };
    let start = current
        .and_then(|c| cands.iter().position(|(i, _)| *i == c))
        .map_or(0, |p| {
            if needle.chars().count() == 1 {
                p + 1
            } else {
                p
            }
        });
    (0..cands.len())
        .map(|k| (start + k) % cands.len())
        .map(|k| &cands[k])
        .find(|(_, l)| l.starts_with(&needle))
        .map(|(i, _)| *i)
}

enum Move {
    Next,
    Prev,
    First,
    Last,
    PageDown,
    PageUp,
}

fn moved(d: &Document, select: NodeId, from: Option<u32>, mv: Move) -> Option<u32> {
    let cands = pickable(d, select);
    if cands.is_empty() {
        return None;
    }
    let pos = from.and_then(|f| cands.iter().position(|c| *c == f));
    let page = capacity(d, select).saturating_sub(1).max(1);
    let i = match (mv, pos) {
        (Move::First, _) => 0,
        (Move::Last, _) => cands.len() - 1,
        (Move::Next, Some(p)) => (p + 1).min(cands.len() - 1),
        (Move::Next, None) => 0,
        (Move::Prev, Some(p)) => p.saturating_sub(1),
        (Move::Prev, None) => cands.len() - 1,
        (Move::PageDown, Some(p)) => (p + page).min(cands.len() - 1),
        (Move::PageDown, None) => 0,
        (Move::PageUp, Some(p)) => p.saturating_sub(page),
        (Move::PageUp, None) => cands.len() - 1,
    };
    cands.get(i).copied()
}

fn nav_key(key: &str) -> Option<Move> {
    Some(match key {
        "ArrowDown" | "ArrowRight" => Move::Next,
        "ArrowUp" | "ArrowLeft" => Move::Prev,
        "Home" => Move::First,
        "End" => Move::Last,
        "PageDown" => Move::PageDown,
        "PageUp" => Move::PageUp,
        _ => return None,
    })
}

/// The key default actions of a focused `select` (and the `range`, `number` and `radio` inputs).
/// Returns whether the key was consumed.
pub(crate) fn keydown(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    k: &KeyData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<bool, Status> {
    if k.is_composing || m.ctrl || m.meta {
        return Ok(false);
    }
    let (is_select, ty) = eng(host, doc, |_, d| {
        (
            html_named(d, target, "select") && !is_disabled(d, target),
            input_type(d, target),
        )
    })?;
    if is_select {
        return select_keydown(host, doc, target, k, m, origin, ts);
    }
    match ty.as_deref() {
        Some("range") => range_keydown(host, doc, target, k, origin, ts),
        Some("number") => number_keydown(host, doc, target, k, origin, ts),
        Some("radio") => radio_keydown(host, doc, target, k, m, origin, ts),
        _ => Ok(false),
    }
}

fn select_keydown(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    select: NodeId,
    k: &KeyData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<bool, Status> {
    sync_picker(host, doc)?;
    let now = host.now_ms();
    let (dropdown, open, multiple) = eng(host, doc, |ix, d| {
        (
            is_dropdown(d, select),
            ix.open_picker(doc) == Some(select),
            d.select_is_multiple(select),
        )
    })?;
    let key = k.key.as_str();
    let typed = if m.alt { None } else { printable(k) };
    // Type-ahead buffer.
    if let Some(c) = typed.filter(|c| {
        !(*c == ' '
            && eng(host, doc, |ix, _| {
                ix.controls_state(doc)
                    .is_none_or(|s| s.typeahead.is_empty() || now - s.typeahead_at > TYPEAHEAD_MS)
            })
            .unwrap_or(true))
    }) {
        let buffer = eng(host, doc, |ix, _| {
            let s = ix.controls_state(doc)?;
            if now - s.typeahead_at > TYPEAHEAD_MS {
                s.typeahead.clear();
            }
            s.typeahead.push(c);
            s.typeahead_at = now;
            Some(s.typeahead.clone())
        })?;
        let Some(buffer) = buffer else {
            return Ok(false);
        };
        let current = eng(host, doc, |_, d| {
            if dropdown && open || !dropdown {
                highlight_of(d, select).or(Some(d.select_selected_index(select))
                    .filter(|i| *i >= 0)
                    .map(|i| i as u32))
            } else {
                Some(d.select_selected_index(select))
                    .filter(|i| *i >= 0)
                    .map(|i| i as u32)
            }
        })?;
        let Some(t) = eng(host, doc, |_, d| {
            typeahead_target(d, select, &buffer, current)
        })?
        else {
            return Ok(true);
        };
        if dropdown && open {
            eng(host, doc, |_, d| set_highlight(d, select, Some(t)))?;
        } else {
            eng(host, doc, |_, d| set_highlight(d, select, Some(t)))?;
            if !dropdown {
                reveal_listbox_option(host, doc, select, t)?;
            }
            commit(host, doc, select, t, false, origin, ts)?;
        }
        return Ok(true);
    }
    if dropdown {
        if open {
            return match key {
                "Escape" => {
                    eng(host, doc, |ix, d| close_picker_state(ix, d, select))?;
                    Ok(true)
                }
                "Enter" | " " | "Tab" => {
                    let hl = eng(host, doc, |_, d| highlight_of(d, select))?;
                    eng(host, doc, |ix, d| close_picker_state(ix, d, select))?;
                    if let Some(h) = hl {
                        commit(host, doc, select, h, multiple, origin, ts)?;
                    }
                    Ok(key != "Tab")
                }
                "ArrowDown" | "ArrowUp" if m.alt => {
                    let hl = eng(host, doc, |_, d| highlight_of(d, select))?;
                    eng(host, doc, |ix, d| close_picker_state(ix, d, select))?;
                    if let Some(h) = hl {
                        commit(host, doc, select, h, multiple, origin, ts)?;
                    }
                    Ok(true)
                }
                _ => match nav_key(key) {
                    Some(mv) => {
                        eng(host, doc, |_, d| {
                            let to = moved(d, select, highlight_of(d, select), mv);
                            if to.is_some() {
                                set_highlight(d, select, to);
                            }
                        })?;
                        Ok(true)
                    }
                    None => Ok(false),
                },
            };
        }
        // Closed drop-down.
        if matches!(key, "Enter" | " ")
            || (m.alt && matches!(key, "ArrowDown" | "ArrowUp"))
            || key == "F4"
        {
            eng(host, doc, |ix, d| open_picker_state(ix, d, select))?;
            return Ok(true);
        }
        if let Some(mv) = nav_key(key) {
            let from = eng(host, doc, |_, d| {
                Some(d.select_selected_index(select))
                    .filter(|i| *i >= 0)
                    .map(|i| i as u32)
            })?;
            if let Some(to) = eng(host, doc, |_, d| moved(d, select, from, mv))? {
                commit(host, doc, select, to, false, origin, ts)?;
            }
            return Ok(true);
        }
        return Ok(false);
    }
    // Listbox.
    if key == " " || (key == "Enter" && false) {
        if multiple && let Some(h) = eng(host, doc, |_, d| highlight_of(d, select))? {
            commit(host, doc, select, h, true, origin, ts)?;
        }
        return Ok(true);
    }
    if let Some(mv) = nav_key(key) {
        let to = eng(host, doc, |_, d| {
            let from = highlight_of(d, select).or(Some(d.select_selected_index(select))
                .filter(|i| *i >= 0)
                .map(|i| i as u32));
            moved(d, select, from, mv)
        })?;
        let Some(to) = to else { return Ok(true) };
        eng(host, doc, |ix, d| {
            set_highlight(d, select, Some(to));
            if m.shift && multiple {
                let anchor = ix.controls_state(doc).and_then(|s| s.anchor).unwrap_or(to);
                if let Some(s) = ix.controls_state(doc) {
                    s.anchor = Some(anchor);
                }
            } else if let Some(s) = ix.controls_state(doc) {
                s.anchor = Some(to);
            }
        })?;
        reveal_listbox_option(host, doc, select, to)?;
        if m.shift && multiple {
            let changed = eng(host, doc, |ix, d| {
                let anchor = ix.controls_state(doc).and_then(|s| s.anchor).unwrap_or(to);
                select_range(d, select, anchor, to)
            })?;
            if changed {
                fire_input_change(host, doc, select, origin, ts)?;
            }
        } else {
            commit(host, doc, select, to, false, origin, ts)?;
        }
        return Ok(true);
    }
    Ok(false)
}

/// Select exactly the pickable options between two option indexes; whether anything changed.
fn select_range(d: &mut Document, select: NodeId, a: u32, b: u32) -> bool {
    let (lo, hi) = (a.min(b), a.max(b));
    let options = d.select_options(select);
    let before: Vec<bool> = options.iter().map(|o| d.option_selected(*o)).collect();
    for (i, o) in options.iter().enumerate() {
        let on = (lo..=hi).contains(&(i as u32)) && !d.option_disabled(*o);
        let _ = d.control_state_writer().set_selectedness(*o, on);
    }
    let after: Vec<bool> = options.iter().map(|o| d.option_selected(*o)).collect();
    before != after
}

// ----------------------------------------------------------------------------- select pointer

type SelectGeometry = ((f64, f64), SelectGeom);

/// The geometry of an open popup or a listbox: the select's border-box origin in the viewport and
/// its published rows.
fn select_geometry(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    select: NodeId,
) -> Result<Option<SelectGeometry>, Status> {
    let Some(g) = host.control_geometry(doc, select)? else {
        return Ok(None);
    };
    let Some(sel) = g.inner.select.clone() else {
        return Ok(None);
    };
    let scroll = if sel.popup || host.scroll_info(doc, Some(select))?.is_none() {
        (0.0, 0.0)
    } else {
        g.scroll
    };
    let origin = (
        g.origin.0 - g.inner.viewport.origin.x.to_f64() - scroll.0,
        g.origin.1 - g.inner.viewport.origin.y.to_f64() - scroll.1,
    );
    Ok(Some((origin, *sel)))
}

fn row_at(origin: (f64, f64), g: &SelectGeom, x: f64, y: f64) -> Option<SelectRowGeom> {
    let (lx, ly) = ((x - origin.0) as f32, (y - origin.1) as f32);
    g.rows
        .iter()
        .find(|r| lx >= r.x && lx < r.x + r.width && ly >= r.y && ly < r.y + r.height)
        .cloned()
}

fn in_panel(origin: (f64, f64), g: &SelectGeom, x: f64, y: f64) -> bool {
    let (lx, ly) = ((x - origin.0) as f32, (y - origin.1) as f32);
    g.popup
        && lx >= g.panel[0]
        && lx < g.panel[0] + g.panel[2]
        && ly >= g.panel[1]
        && ly < g.panel[1] + g.panel[3]
}

/// A primary press. Closes an open popup the press is outside of; opens, toggles or picks in a
/// select; starts a range drag. Returns nothing: the press goes on (focus, `click`) as usual.
pub(crate) fn pointer_down(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    p: &PointerData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    sync_picker(host, doc)?;
    let open = rt(host, |ix| ix.open_picker(doc));
    if let Some(sel) = open
        && sel != target
    {
        eng(host, doc, |ix, d| close_picker_state(ix, d, sel))?;
    }
    let (is_select, dropdown, ty, disabled) = eng(host, doc, |_, d| {
        (
            html_named(d, target, "select"),
            is_dropdown(d, target),
            input_type(d, target),
            is_disabled(d, target),
        )
    })?;
    if disabled {
        return Ok(());
    }
    if is_select && dropdown {
        if open == Some(target) {
            let geo = select_geometry(host, doc, target)?;
            match geo {
                Some((o, g)) if in_panel(o, &g, p.x, p.y) => {
                    if let Some(r) =
                        row_at(o, &g, p.x, p.y).filter(|r| r.option.is_some() && !r.disabled)
                    {
                        eng(host, doc, |_, d| set_highlight(d, target, r.option))?;
                    }
                }
                _ => eng(host, doc, |ix, d| close_picker_state(ix, d, target))?,
            }
        } else {
            eng(host, doc, |ix, d| open_picker_state(ix, d, target))?;
        }
    } else if is_select {
        // Listbox: a press on a row picks it.
        if let Some((o, g)) = select_geometry(host, doc, target)?
            && let Some(r) = row_at(o, &g, p.x, p.y).filter(|r| r.option.is_some() && !r.disabled)
        {
            let idx = r.option.unwrap_or(0);
            let multiple = eng(host, doc, |_, d| d.select_is_multiple(target))?;
            let changed = eng(host, doc, |ix, d| {
                set_highlight(d, target, Some(idx));
                if multiple && m.shift {
                    let anchor = ix.controls_state(doc).and_then(|s| s.anchor).unwrap_or(idx);
                    select_range(d, target, anchor, idx)
                } else {
                    if let Some(s) = ix.controls_state(doc) {
                        s.anchor = Some(idx);
                    }
                    pick(d, target, idx, multiple && (m.ctrl || m.meta))
                }
            })?;
            reveal_listbox_option(host, doc, target, idx)?;
            if changed {
                fire_input_change(host, doc, target, origin, ts)?;
            }
        }
    } else if ty.as_deref() == Some("range") {
        let changed = set_range_from_point(host, doc, target, p.x)?;
        eng(host, doc, |ix, _| {
            if let Some(s) = ix.controls_state(doc) {
                s.range_drag = Some(RangeDrag {
                    node: target,
                    changed,
                });
            }
        })?;
        if changed {
            run_dispatch(host, doc, target, ev("input"), true, origin, ts)?;
        }
    }
    Ok(())
}

/// Pointer move: the open popup follows the pointer; a range drag moves its thumb.
pub(crate) fn pointer_move(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    sync_picker(host, doc)?;
    if let Some(sel) = rt(host, |ix| ix.open_picker(doc))
        && let Some((o, g)) = select_geometry(host, doc, sel)?
        && let Some(r) = row_at(o, &g, p.x, p.y).filter(|r| r.option.is_some() && !r.disabled)
    {
        eng(host, doc, |_, d| {
            if highlight_of(d, sel) != r.option {
                set_highlight(d, sel, r.option);
            }
        })?;
    }
    if let Some(drag) = rt(host, |ix| {
        ix.docs.get(&doc).and_then(|s| s.controls.range_drag)
    }) && p.buttons & 1 != 0
        && set_range_from_point(host, doc, drag.node, p.x)?
    {
        eng(host, doc, |ix, _| {
            if let Some(s) = ix.controls_state(doc)
                && let Some(d) = s.range_drag.as_mut()
            {
                d.changed = true;
            }
        })?;
        run_dispatch(host, doc, drag.node, ev("input"), true, origin, ts)?;
    }
    Ok(())
}

/// Pointer release: an open popup commits the option under the pointer; a range drag ends with `change`.
pub(crate) fn pointer_up(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    sync_picker(host, doc)?;
    if let Some(sel) = rt(host, |ix| ix.open_picker(doc))
        && let Some((o, g)) = select_geometry(host, doc, sel)?
        && let Some(r) = row_at(o, &g, p.x, p.y).filter(|r| r.option.is_some() && !r.disabled)
    {
        let multiple = eng(host, doc, |_, d| d.select_is_multiple(sel))?;
        eng(host, doc, |ix, d| close_picker_state(ix, d, sel))?;
        commit(host, doc, sel, r.option.unwrap_or(0), multiple, origin, ts)?;
    }
    let drag = eng(host, doc, |ix, _| {
        ix.controls_state(doc).and_then(|s| s.range_drag.take())
    })?;
    if let Some(d) = drag
        && d.changed
        && eng(host, doc, |_, dd| dd.arena().contains(d.node))?
    {
        run_dispatch(host, doc, d.node, ev("change"), true, origin, ts)?;
    }
    Ok(())
}

/// Wheel over an open popup scrolls its rows; returns whether it was consumed.
pub(crate) fn wheel(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    x: f64,
    y: f64,
    dy: f64,
) -> Result<bool, Status> {
    sync_picker(host, doc)?;
    let Some(sel) = rt(host, |ix| ix.open_picker(doc)) else {
        return Ok(false);
    };
    let Some((o, g)) = select_geometry(host, doc, sel)? else {
        return Ok(false);
    };
    if !in_panel(o, &g, x, y) {
        return Ok(false);
    }
    eng(host, doc, |_, d| {
        let total = d.select_rows(sel).len();
        let cap = capacity(d, sel);
        let hl = highlight_of(d, sel);
        let cur = d
            .control_state(sel)
            .ok()
            .flatten()
            .map_or(0, |c| c.picker_scroll) as i64;
        let next = (cur
            + if dy > 0.0 {
                3
            } else if dy < 0.0 {
                -3
            } else {
                0
            })
        .clamp(0, total.saturating_sub(cap) as i64) as u32;
        let _ = d.control_state_writer().set_picker(sel, hl, next);
    })?;
    Ok(true)
}

// ----------------------------------------------------------------------------- range and number

/// Write the value of a range or number input; whether it changed.
fn write_number(d: &mut Document, node: NodeId, v: f64) -> bool {
    let text = format_float(v);
    let range = input_type(d, node).as_deref() == Some("range");
    let before = if range {
        d.range_value(node).map(format_float)
    } else {
        d.control_value(node)
    };
    if before.as_deref() == Some(text.as_str()) {
        return false;
    }
    let ok = d.control_state_writer().set_value(node, &text).is_ok();
    if ok && !range {
        let len = text.encode_utf16().count() as u32;
        let _ = d.control_state_writer().set_selection(
            node,
            Some(ui4me_document::control_state::Selection {
                start: len,
                end: len,
                direction: ui4me_document::control_state::SelectionDirection::None,
            }),
        );
    }
    ok
}

/// An assistive technology's SetValue (`value`) or Increment/Decrement (`dir`) on a range or number
/// input (`crate::interaction::assistive`): sanitised as the keyboard steps are, then `input` and `change`.
/// An assistive Click on an `option` (what AT-SPI/UIA selection requests send): the same user
/// pick as a press in the popup or listbox, then `input` and `change`. A `multiple` select
/// toggles that option; a single select selects it alone. Disabled options/selects are refused.
/// Returns `None` when `option` is not an option of a select (the caller falls back to a click).
pub(crate) fn assistive_pick_option(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    option: NodeId,
    origin: DispatchOrigin,
) -> Result<Option<bool>, Status> {
    let Some((select, index, multiple)) = eng(host, doc, |_, d| {
        if !html_named(d, option, "option") {
            return None;
        }
        let select = d.option_select(option)?;
        Some((select, d.option_index(option), d.select_is_multiple(select)))
    })?
    else {
        return Ok(None);
    };
    let refused = eng(host, doc, |_, d| {
        is_disabled(d, select) || d.option_disabled(option)
    })?;
    if refused {
        return Err(crate::interaction::dispatch::status(
            ui4me_engine_types::codes_generated::Code::InvalidState,
        )
        .with_message("the option or its select is disabled"));
    }
    commit(host, doc, select, index, multiple, origin, None).map(Some)
}

pub(crate) fn assistive_set_number(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    value: Option<f64>,
    dir: i32,
    origin: DispatchOrigin,
) -> Result<bool, Status> {
    let changed = eng(host, doc, |_, d| {
        let ty = input_type(d, node);
        if !matches!(ty.as_deref(), Some("range" | "number"))
            || is_disabled(d, node)
            || (ty.as_deref() == Some("number") && attr(d, node, "readonly").is_some())
        {
            return Err(crate::interaction::dispatch::status(
                ui4me_engine_types::codes_generated::Code::InvalidState,
            )
            .with_message("not an enabled range or number input"));
        }
        let range = ty.as_deref() == Some("range");
        let v = match value {
            Some(v) if range => d.range_sanitize(node, v),
            Some(v) => Some(v),
            None if range => {
                let m = d.input_step_model(node);
                let step = m.as_ref().and_then(|m| m.step).unwrap_or(1.0);
                d.range_value(node)
                    .and_then(|cur| d.range_sanitize(node, cur + step * f64::from(dir)))
            }
            None => d.input_step_value(node, dir),
        };
        Ok(v.is_some_and(|v| write_number(d, node, v)))
    })??;
    if changed {
        fire_input_change(host, doc, node, origin, None)?;
    }
    Ok(changed)
}

fn set_range_from_point(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    x: f64,
) -> Result<bool, Status> {
    let Some(r) = host.element_content_rect(doc, node)? else {
        return Ok(false);
    };
    eng(host, doc, |_, d| {
        let Some(model) = d.input_step_model(node) else {
            return false;
        };
        let (min, max) = (model.min.unwrap_or(0.0), model.max.unwrap_or(100.0));
        let thumb = r.3.clamp(8.0, 16.0);
        let span = (r.2 - thumb).max(1.0);
        let mut t = ((x - r.0 - thumb / 2.0) / span).clamp(0.0, 1.0);
        if crate::style::dir::is_rtl(d, node) {
            t = 1.0 - t;
        }
        let Some(v) = d.range_sanitize(node, min + t * (max - min)) else {
            return false;
        };
        write_number(d, node, v)
    })
}

fn range_keydown(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    k: &KeyData,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<bool, Status> {
    let key = k.key.as_str();
    if !matches!(
        key,
        "ArrowRight"
            | "ArrowUp"
            | "ArrowLeft"
            | "ArrowDown"
            | "PageUp"
            | "PageDown"
            | "Home"
            | "End"
    ) {
        return Ok(false);
    }
    let changed = eng(host, doc, |_, d| {
        // `readonly` does not apply to `range` (HTML): only `disabled` blocks the keyboard.
        if is_disabled(d, node) {
            return false;
        }
        let rtl = crate::style::dir::is_rtl(d, node);
        let Some(m) = d.input_step_model(node) else {
            return false;
        };
        let (min, max) = (m.min.unwrap_or(0.0), m.max.unwrap_or(100.0));
        let step = m.step.unwrap_or(1.0);
        let cur = d.range_value(node).unwrap_or(min);
        let page = ((max - min) / 10.0).max(step);
        let raw = match key {
            "ArrowUp" => cur + step,
            "ArrowDown" => cur - step,
            "ArrowRight" if !rtl => cur + step,
            "ArrowLeft" if rtl => cur + step,
            "ArrowRight" | "ArrowLeft" => cur - step,
            "PageUp" => cur + page,
            "PageDown" => cur - page,
            "Home" => min,
            _ => max,
        };
        let Some(v) = d.range_sanitize(node, raw) else {
            return false;
        };
        write_number(d, node, v)
    })?;
    if changed {
        fire_input_change(host, doc, node, origin, ts)?;
    }
    Ok(true)
}

fn number_keydown(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    k: &KeyData,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<bool, Status> {
    let n = match k.key.as_str() {
        "ArrowUp" => 1,
        "ArrowDown" => -1,
        _ => return Ok(false),
    };
    let changed = eng(host, doc, |_, d| {
        if is_disabled(d, node) || attr(d, node, "readonly").is_some() {
            return false;
        }
        match d.input_step_value(node, n) {
            Some(v) => write_number(d, node, v),
            None => false,
        }
    })?;
    if changed {
        fire_input_change(host, doc, node, origin, ts)?;
    }
    Ok(true)
}

// ----------------------------------------------------------------------------- radio

fn radio_keydown(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    k: &KeyData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<bool, Status> {
    if m.alt || m.shift {
        return Ok(false);
    }
    let dir = match k.key.as_str() {
        "ArrowDown" | "ArrowRight" => 1i32,
        "ArrowUp" | "ArrowLeft" => -1,
        _ => return Ok(false),
    };
    let next = eng(host, doc, |_, d| {
        let group: Vec<NodeId> = d
            .radio_group(node)
            .into_iter()
            .filter(|r| !is_disabled(d, *r))
            .collect();
        let pos = group.iter().position(|r| *r == node)?;
        let n = group.len() as i32;
        group
            .get(((pos as i32 + dir).rem_euclid(n)) as usize)
            .copied()
            .filter(|r| *r != node)
    })?;
    if let Some(next) = next {
        focus_update(host, doc, Some(next), origin, FocusSource::Keyboard)?;
        let mut click = EventInit::new("click")
            .interface(EventInterface::PointerEvent)
            .bubbles(true)
            .cancelable(true);
        click.detail = 1;
        run_dispatch(host, doc, next, click, true, origin, ts)?;
    }
    Ok(true)
}

// ----------------------------------------------------------------------------- host writes

/// The sanitised value a host `value` write stores for the control (HTML value sanitization
/// algorithm of the input type; a `textarea` normalises line breaks to LF).
fn sanitize_value(d: &Document, el: NodeId, value: &str) -> String {
    if html_named(d, el, "textarea") {
        return value.replace("\r\n", "\n").replace('\r', "\n");
    }
    let strip = |s: &str| {
        s.chars()
            .filter(|c| !matches!(c, '\n' | '\r'))
            .collect::<String>()
    };
    let trim = |s: String| {
        s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'))
            .to_string()
    };
    match input_type(d, el).as_deref() {
        Some("url" | "email") => trim(strip(value)),
        Some("number") => ui4me_document::control_values::parse_float(value)
            .map_or(String::new(), |_| value.trim().to_string()),
        Some("range") => {
            // Stored as given; the sanitised value is what `range_value` reports.
            let raw = ui4me_document::control_values::parse_float(value);
            raw.and_then(|v| d.range_sanitize(el, v))
                .map_or_else(String::new, format_float)
        }
        Some("color") => {
            let v = value.trim().to_ascii_lowercase();
            if v.len() == 7 && v.starts_with('#') && v[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
                v
            } else {
                "#000000".to_string()
            }
        }
        Some(
            "checkbox" | "radio" | "hidden" | "submit" | "reset" | "button" | "image" | "file",
        ) => value.to_string(),
        _ => strip(value),
    }
}

/// The host `value` setter (HTML IDL, `controls.md` section 3): `select.value` picks the option, a text
/// control stores the sanitised value, dirty, with the caret at the end and no composition. No events.
pub fn host_set_value(d: &mut Document, el: NodeId, value: &str) -> Result<(), Status> {
    if html_named(d, el, "select") {
        return d.select_set_value(el, value);
    }
    let v = sanitize_value(d, el, value);
    d.control_state_writer().set_value(el, &v)?;
    if crate::interaction::editing::text_kind(d, el).is_some() {
        let len = v.encode_utf16().count() as u32;
        let _ = d.control_state_writer().set_composition(el, None);
        let _ = d.control_state_writer().set_selection(
            el,
            Some(ui4me_document::control_state::Selection {
                start: len,
                end: len,
                direction: ui4me_document::control_state::SelectionDirection::None,
            }),
        );
    }
    Ok(())
}

/// The host `checked` setter: a radio that becomes checked unchecks the others of its group.
pub fn host_set_checked(d: &mut Document, el: NodeId, checked: bool) -> Result<(), Status> {
    if checked && input_type(d, el).as_deref() == Some("radio") {
        return d.radio_check(el);
    }
    d.control_state_writer().set_checkedness(el, checked)
}

impl InteractionRuntime {
    /// A host write replaced the value of a text control: the `change` baseline restarts from it.
    /// Returns whether a composition was abandoned (the platform preedit must be reset).
    pub fn host_value_written(&mut self, doc: DocumentId, node: NodeId, value: &str) -> bool {
        crate::interaction::editing::reset_baseline(self, doc, node, value)
    }
}
