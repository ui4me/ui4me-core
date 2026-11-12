//! owner: 012-interaction (lane L-INT-r3: US3 essentials T047 subset, FR-031, FR-039, FR-042)
//!
//! Pointer state that CSS can see: `:hover` on the element under the pointer and its ancestors,
//! `:active` on the element a primary press (or Space on a button) activated and its ancestors,
//! both written to 003's element-state table through the document's `UiStateWriter` (the only
//! writer, AD-28/R-38). Pointer moves also dispatch the legacy boundary events
//! (`mouseout`/`mouseleave`/`mouseover`/`mouseenter`) and `mousemove` at the hit element.
//!
//! Not done (012 T047/T048 stay open): `pointer*` events and pointer capture, the label's
//! control, the stationary re-hit after a scene-changing COMMIT (only after a wheel scroll),
//! pen and touch (only the primary mouse pointer drives hover).

use crate::interaction::dispatch::{DispatchHost, eng, rt, run_dispatch};
use crate::interaction::frame::{EventInit, EventInterface, Modifiers};
use ui4me_document::document::Document;
use ui4me_document::ui_state::UiFlags;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::input::{PointerData, PointerType};
use ui4me_engine_types::status::Status;

/// Hover/active/modality state of one document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HoverState {
    /// Deepest element under the pointer.
    pub hover: Option<NodeId>,
    /// Element of the primary press (or Space press) that set `:active`.
    pub active: Option<NodeId>,
    /// Last pointer position (viewport CSS px), for the re-hit after a scroll.
    pub last_pointer: Option<(f64, f64)>,
    /// The last user interaction was a key press (FR-048).
    pub keyboard_modality: bool,
}

fn is_element(d: &Document, n: NodeId) -> bool {
    d.arena().get(n).is_some_and(|x| x.is_element())
}

/// `n` and its element ancestors, innermost first (the flat tree is the DOM tree: no shadow
/// trees in v1).
pub(crate) fn element_chain(d: &Document, n: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut a = Some(n);
    while let Some(x) = a {
        if is_element(d, x) {
            out.push(x);
        }
        a = d.arena().get(x).and_then(|y| y.parent());
    }
    out
}

impl crate::interaction::dispatch::InteractionRuntime {
    /// Move `flag` from the chain of `old` to the chain of `new`: only elements that differ are
    /// written (no write, no revision step, when nothing changes).
    pub(crate) fn write_chain_flag(
        &mut self,
        d: &mut Document,
        flag: UiFlags,
        old: Option<NodeId>,
        new: Option<NodeId>,
    ) {
        let doc = d.id();
        let old_chain = old
            .filter(|o| d.arena().contains(*o))
            .map(|o| element_chain(d, o))
            .unwrap_or_default();
        let new_chain = new
            .filter(|n| d.arena().contains(*n))
            .map(|n| element_chain(d, n))
            .unwrap_or_default();
        let Some(s) = self.docs.get(&doc) else { return };
        if let Ok(mut w) = d.ui_state_writer(&s.ui_key) {
            for n in old_chain.iter().filter(|n| !new_chain.contains(n)) {
                if let Ok(f) = w.flags(*n) {
                    let _ = w.set_flags(*n, f & !flag);
                }
            }
            for n in new_chain.iter().filter(|n| !old_chain.contains(n)) {
                if let Ok(f) = w.flags(*n)
                    && w.document().arena().is_connected(*n)
                {
                    let _ = w.set_flags(*n, f | flag);
                }
            }
        }
        self.note_doc(doc);
    }

    /// Set the hovered element (flags only; events are the caller's).
    pub(crate) fn set_hover(&mut self, d: &mut Document, new: Option<NodeId>) {
        let doc = d.id();
        let Some(old) = self.docs.get(&doc).map(|s| s.hover.hover) else {
            return;
        };
        if old == new {
            return;
        }
        self.write_chain_flag(d, UiFlags::HOVER, old, new);
        if let Some(s) = self.docs.get_mut(&doc) {
            s.hover.hover = new;
        }
    }

    /// Set the active element (`:active`).
    pub(crate) fn set_active(&mut self, d: &mut Document, new: Option<NodeId>) {
        let doc = d.id();
        let Some(old) = self.docs.get(&doc).map(|s| s.hover.active) else {
            return;
        };
        if old == new {
            return;
        }
        self.write_chain_flag(d, UiFlags::ACTIVE, old, new);
        if let Some(s) = self.docs.get_mut(&doc) {
            s.hover.active = new;
        }
    }

    /// A removed or disconnected hover/active/press target must not stay referenced (no stale
    /// `:hover` on its former ancestors, no press that can later click): clear those states. The
    /// next pointer move re-hits.
    pub(crate) fn drop_dead_pointer_targets(&mut self, d: &mut Document) {
        let doc = d.id();
        let Some(s) = self.docs.get(&doc) else { return };
        let dead = |n: Option<NodeId>| n.is_some_and(|n| !d.arena().is_connected(n));
        let (hover, active, press) = (
            dead(s.hover.hover),
            dead(s.hover.active),
            dead(s.pointer.press_target),
        );
        let lost = dead(s.capture.target);
        let pending_dead = s.capture.pending.is_some_and(dead);
        if let Some(s) = self.docs.get_mut(&doc) {
            if lost {
                s.capture.target = None;
                s.capture.lost_removed = true;
            }
            if pending_dead {
                s.capture.pending = None;
            }
        }
        if !(hover || active || press) {
            return;
        }
        let live: Vec<NodeId> = d
            .arena()
            .iter()
            .map(|n| n.id())
            .filter(|&n| d.arena().is_connected(n) && is_element(d, n))
            .collect();
        let mask = if hover {
            UiFlags::HOVER
        } else {
            UiFlags::empty()
        } | if active {
            UiFlags::ACTIVE
        } else {
            UiFlags::empty()
        };
        if let Some(s) = self.docs.get(&doc)
            && let Ok(mut w) = d.ui_state_writer(&s.ui_key)
        {
            for n in live {
                if let Ok(f) = w.flags(n)
                    && f.intersects(mask)
                {
                    let _ = w.set_flags(n, f & !mask);
                }
            }
        }
        if let Some(s) = self.docs.get_mut(&doc) {
            if hover {
                s.hover.hover = None;
            }
            if active {
                s.hover.active = None;
            }
            if press {
                s.pointer = Default::default();
            }
        }
        self.note_doc(doc);
    }

    pub(crate) fn note_modality(&mut self, doc: DocumentId, keyboard: bool) {
        if let Some(s) = self.docs.get_mut(&doc) {
            s.hover.keyboard_modality = keyboard;
        }
    }
}

fn boundary(
    ty: &str,
    p: &PointerData,
    related: Option<NodeId>,
    bubbles: bool,
    cancelable: bool,
    m: Modifiers,
) -> EventInit {
    let mut e = EventInit::mouse(ty, EventInterface::MouseEvent, p.x, p.y, -1, p.buttons);
    e.bubbles = bubbles;
    e.cancelable = cancelable;
    e.related_target = related;
    e.modifiers = m;
    e
}

/// The hit element, else the document element, else the Document.
pub(crate) fn hit_element(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    x: f64,
    y: f64,
) -> Result<NodeId, Status> {
    if let Some(n) = host.hit_test(doc, x, y)? {
        return Ok(n);
    }
    eng(host, doc, |_, d| {
        d.document_element().unwrap_or_else(|| d.document_node())
    })
}

/// Update hover to `new` and dispatch the boundary events in contract order (FR-039): `out` at
/// the old element, `leave` at the old-only ancestors (innermost first), then `over` at the new
/// element and `enter` at the new-only ancestors (outermost first). The flags change before
/// the first event, as in browsers. Returns whether hover changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_hover(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    new: Option<NodeId>,
    p: &PointerData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
    events: bool,
) -> Result<bool, Status> {
    let old = rt(host, |ix| ix.hovered_element(doc));
    if old == new {
        return Ok(false);
    }
    let (old_alive, old_only, new_only) = eng(host, doc, |ix, d| {
        let old_alive = old.filter(|o| d.arena().contains(*o));
        let oc = old_alive.map(|o| element_chain(d, o)).unwrap_or_default();
        let nc = new.map(|n| element_chain(d, n)).unwrap_or_default();
        let old_only: Vec<NodeId> = oc.iter().copied().filter(|n| !nc.contains(n)).collect();
        let new_only: Vec<NodeId> = nc.iter().copied().filter(|n| !oc.contains(n)).collect();
        ix.set_hover(d, new);
        (old_alive, old_only, new_only)
    })?;
    if !events {
        return Ok(true);
    }
    if let Some(o) = old_alive {
        run_dispatch(
            host,
            doc,
            o,
            boundary("mouseout", p, new, true, true, m),
            true,
            origin,
            ts,
        )?;
        for n in &old_only {
            if eng(host, doc, |_, d| d.arena().contains(*n))? {
                run_dispatch(
                    host,
                    doc,
                    *n,
                    boundary("mouseleave", p, new, false, false, m),
                    true,
                    origin,
                    ts,
                )?;
            }
        }
    }
    if let Some(n) = new {
        if eng(host, doc, |_, d| d.arena().contains(n))? {
            run_dispatch(
                host,
                doc,
                n,
                boundary("mouseover", p, old_alive, true, true, m),
                true,
                origin,
                ts,
            )?;
        }
        for n in new_only.iter().rev() {
            if eng(host, doc, |_, d| d.arena().contains(*n))? {
                run_dispatch(
                    host,
                    doc,
                    *n,
                    boundary("mouseenter", p, old_alive, false, false, m),
                    true,
                    origin,
                    ts,
                )?;
            }
        }
    }
    Ok(true)
}

fn is_primary_mouse(p: &PointerData) -> bool {
    p.pointer_type == PointerType::Mouse && p.is_primary
}

/// `PointerMove` of the primary mouse: hover, boundary events, `mousemove` at the hit element.
pub(crate) fn on_move(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    captured: Option<NodeId>,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    if !is_primary_mouse(p) {
        return Ok(());
    }
    eng(host, doc, |ix, _| {
        if let Some(s) = ix.docs.get_mut(&doc) {
            s.hover.last_pointer = Some((p.x, p.y));
        }
    })?;
    let target = hit_element(host, doc, p.x, p.y)?;
    update_hover(host, doc, Some(target), p, m, origin, ts, true)?;
    // The listeners above may have removed the target.
    let target = if eng(host, doc, |_, d| d.arena().contains(target))? {
        target
    } else {
        rt(host, |ix| ix.hovered_element(doc)).unwrap_or(target)
    };
    // A captured pointer's compatibility events go to the capture target too.
    let target = captured.unwrap_or(target);
    let mut mv = EventInit::mouse(
        "mousemove",
        EventInterface::MouseEvent,
        p.x,
        p.y,
        -1,
        p.buttons,
    );
    mv.modifiers = m;
    run_dispatch(host, doc, target, mv, true, origin, ts)?;
    Ok(())
}

/// `PointerLeave`/`PointerCancel` of the primary mouse: hover clears with `out`/`leave` events.
pub(crate) fn on_leave(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    if !is_primary_mouse(p) {
        return Ok(());
    }
    eng(host, doc, |ix, _| {
        if let Some(s) = ix.docs.get_mut(&doc) {
            s.hover.last_pointer = None;
        }
    })?;
    update_hover(host, doc, None, p, m, origin, ts, true)?;
    Ok(())
}

/// A press or release at `(x, y)` without a preceding move: hover follows silently (flags only,
/// no boundary events), so `:hover` styles apply under an injected click.
pub(crate) fn sync_hover_silently(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    target: NodeId,
    m: Modifiers,
    origin: DispatchOrigin,
) -> Result<(), Status> {
    if !is_primary_mouse(p) {
        return Ok(());
    }
    eng(host, doc, |ix, _| {
        if let Some(s) = ix.docs.get_mut(&doc) {
            s.hover.last_pointer = Some((p.x, p.y));
        }
    })?;
    update_hover(host, doc, Some(target), p, m, origin, None, false)?;
    Ok(())
}

/// The keyword a computed `cursor` value resolves to: the last entry of the list (the fallback
/// keyword after any `url()` references, which are not loaded - FR-043 `[V1-LIMIT]`).
pub fn cursor_keyword(computed: &str) -> &str {
    computed
        .rsplit(',')
        .next()
        .map(str::trim)
        .filter(|k| !k.is_empty() && !k.starts_with("url("))
        .unwrap_or("auto")
}

/// Decide the window cursor (FR-043) from the computed `cursor` of the pressed element (while a
/// button is held) or the hovered one: `auto` resolves to `text` over a text control and
/// `default` elsewhere. The platform is told only when the decision changes.
pub(crate) fn update_cursor(host: &mut dyn DispatchHost, doc: DocumentId) -> Result<(), Status> {
    let node = rt(host, |ix| {
        ix.docs
            .get(&doc)
            .and_then(|s| s.pointer.press_target.or(s.hover.hover))
    });
    let keyword = match node {
        None => "default".to_owned(),
        Some(n) if !eng(host, doc, |_, d| d.arena().contains(n))? => "default".to_owned(),
        Some(n) => {
            let css = host.computed_cursor(doc, n)?;
            match cursor_keyword(&css) {
                "auto" => {
                    if eng(host, doc, |_, d| {
                        crate::interaction::focus::accepts_text_input(d, n)
                    })? {
                        "text".to_owned()
                    } else {
                        "default".to_owned()
                    }
                }
                k => k.to_owned(),
            }
        }
    };
    let changed = rt(host, |ix| match ix.docs.get_mut(&doc) {
        Some(s) if s.cursor.as_deref() != Some(keyword.as_str()) => {
            s.cursor = Some(keyword.clone());
            true
        }
        _ => false,
    });
    if changed {
        host.set_cursor(doc, &keyword);
    }
    Ok(())
}
