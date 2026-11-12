//! owner: 012-interaction (lane L-CLIP-r13; `contracts/editing.md` section 5, AD-39)
//!
//! IME composition in text controls. 011 normalises the platform's input-method events to
//! `ImeEnabled`, `ImePreedit{text, cursor}`, `ImeCommit{text}`, `ImeDeleteSurrounding` and
//! `ImeDisabled` (UTF-16 offsets); this module turns them into UI Events:
//!
//! - the first non-empty preedit fires `compositionstart` (cancelable: canceled asks the platform to
//!   reset and fires `compositionend` with `""`), the selection becomes the composition range;
//! - every preedit update fires `beforeinput` (`insertCompositionText`, not cancelable), replaces the
//!   composition range in the value (the preedit text is part of the value, R-39), stores the range in
//!   003's control state (the scene underlines it), then fires `compositionupdate` and `input` with
//!   `isComposing` true;
//! - `ImeCommit` replaces the composition with the final text (`insertFromComposition`, `maxlength`
//!   applied), clears the range, records one undo step and fires `compositionend`; a commit with no
//!   composition in progress (a Compose-key or dead-key character, a keyboard-less commit) is typed
//!   text: a cancelable `insertText`;
//! - `ImeDisabled`, a blur, the window losing focus and a pointer press commit what is composed as it
//!   is (`compositionend` carries the text, no `input`); a host value write abandons the composition
//!   without events.
//!
//! Key events while composing carry `isComposing` and have no default action. After each change the
//! host is told the caret rectangle, the control's purpose and the text around the caret
//! (`DispatchHost::ime_update`, 011 maps it to the platform's cursor-area and surrounding-text
//! requests).

use super::{
    Chars, Ctl, Cx, Edit, TextKind, edit, fire, history, input_event, number_accepts, reveal_caret,
    snapshot, utf16_len,
};
use crate::interaction::dispatch::{DispatchHost, eng, rt};
use crate::interaction::frame::{EventInit, EventInterface, Modifiers};
use ui4me_document::control_state::{Selection, SelectionDirection};
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::ime::{ImePurposeKind, ImeRequest, surrounding_excerpt};
use ui4me_engine_types::status::Status;

/// A composition in progress.
#[derive(Clone, Debug)]
pub(crate) struct Comp {
    node: NodeId,
    /// The composition range in the value, UTF-16 (`end` follows the preedit text).
    start: u32,
    end: u32,
    /// The text the composition replaced (the selection when it began), for the undo step.
    original: String,
    before: (u32, u32, SelectionDirection),
}

fn composition_event(ty: &str, data: &str, cancelable: bool, m: Modifiers) -> EventInit {
    let mut e = EventInit::new(ty)
        .interface(EventInterface::CompositionEvent)
        .bubbles(true)
        .cancelable(cancelable);
    e.data.data = Some(data.to_owned());
    e.modifiers = m;
    e
}

fn composing_input(
    ty: &str,
    input_type: &str,
    data: &str,
    cancelable: bool,
    m: Modifiers,
) -> EventInit {
    let mut e = input_event(ty, input_type, Some(data), cancelable, m);
    e.data.is_composing = Some(true);
    e
}

/// The focused text control that accepts typing, if any.
fn target(host: &mut dyn DispatchHost, doc: DocumentId) -> Result<Option<NodeId>, Status> {
    let Some(f) = rt(host, |ix| ix.focused_element(doc)) else {
        return Ok(None);
    };
    Ok(eng(host, doc, |_, d| {
        crate::interaction::sequences::is_text_control(d, f)
    })?
    .then_some(f))
}

fn comp_of(host: &mut dyn DispatchHost, doc: DocumentId) -> Option<Comp> {
    rt(host, |ix| {
        ix.doc_state(doc).ok().and_then(|s| s.edit.comp.clone())
    })
}

fn set_comp(host: &mut dyn DispatchHost, doc: DocumentId, c: Option<Comp>) {
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(doc) {
            s.edit.comp = c;
        }
    });
}

/// Whether a composition is in progress in `doc`.
pub(crate) fn is_composing(host: &mut dyn DispatchHost, doc: DocumentId) -> bool {
    comp_of(host, doc).is_some()
}

/// UTF-16 offset to a byte offset of `s`.
fn byte_of(t: &Chars, s: &str, u: u32) -> usize {
    let i = t.index(u);
    s.char_indices().nth(i).map_or(s.len(), |(b, _)| b)
}

fn replace_range(value: &str, start: u32, end: u32, with: &str) -> String {
    let t = Chars::new(value);
    let (si, ei) = (
        t.index(start.min(t.off[t.n()])),
        t.index(end.min(t.off[t.n()])),
    );
    let mut v = String::with_capacity(value.len() + with.len());
    v.extend(t.chars[..si].iter());
    v.push_str(with);
    v.extend(t.chars[ei.max(si)..].iter());
    v
}

/// `ImePreedit`.
pub(crate) fn preedit(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    text: &str,
    cursor: Option<(u32, u32)>,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    let Some(node) = target(host, doc)? else {
        return Ok(());
    };
    let cx = Cx {
        doc,
        node,
        m,
        origin,
        ts,
    };
    let mut comp = comp_of(host, doc).filter(|c| c.node == node);
    if comp.is_none() {
        if text.is_empty() {
            return Ok(());
        }
        let Some(c) = snapshot(host, &cx)? else {
            return Ok(());
        };
        let t = Chars::new(&c.value);
        let selected: String = t.chars[t.index(c.start)..t.index(c.end)].iter().collect();
        if fire(
            host,
            &cx,
            composition_event("compositionstart", &selected, true, m),
        )? {
            host.ime_reset(doc);
            fire(host, &cx, composition_event("compositionend", "", false, m))?;
            return Ok(());
        }
        // A listener may have moved the selection, the focus or the value.
        if target(host, doc)? != Some(node) {
            return Ok(());
        }
        let Some(c) = snapshot(host, &cx)? else {
            return Ok(());
        };
        let t = Chars::new(&c.value);
        let original: String = t.chars[t.index(c.start)..t.index(c.end)].iter().collect();
        let fresh = Comp {
            node,
            start: c.start,
            end: c.end,
            original,
            before: (c.start, c.end, c.dir),
        };
        set_comp(host, doc, Some(fresh.clone()));
        comp = Some(fresh);
    }
    let Some(mut comp) = comp else { return Ok(()) };
    fire(
        host,
        &cx,
        composing_input("beforeinput", "insertCompositionText", text, false, m),
    )?;
    let new_end = comp.start + utf16_len(text);
    let applied = eng(host, doc, |_, d| {
        let c = Ctl::read(d, node)?;
        let v = replace_range(&c.value, comp.start, comp.end, text);
        let mut w = d.control_state_writer();
        w.set_value(node, &v).ok()?;
        // The preedit cursor, UTF-16 offsets within the preedit text; without one the caret ends it.
        let (a, b) = cursor.map_or((new_end, new_end), |(a, b)| {
            (
                comp.start + a.min(utf16_len(text)),
                comp.start + b.min(utf16_len(text)),
            )
        });
        let dir = if a == b {
            SelectionDirection::None
        } else {
            SelectionDirection::Forward
        };
        w.set_selection(
            node,
            Some(Selection {
                start: a.min(b),
                end: a.max(b),
                direction: dir,
            }),
        )
        .ok()?;
        w.set_composition(node, Some((comp.start, new_end))).ok()?;
        Some(b)
    })?;
    let Some(focus) = applied else { return Ok(()) };
    comp.end = new_end;
    set_comp(host, doc, Some(comp));
    fire(
        host,
        &cx,
        composition_event("compositionupdate", text, false, m),
    )?;
    fire(
        host,
        &cx,
        composing_input("input", "insertCompositionText", text, false, m),
    )?;
    reveal_caret(host, &cx, focus, false)
}

/// `ImeCommit`.
pub(crate) fn commit(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    text: &str,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    let Some(node) = target(host, doc)? else {
        return Ok(());
    };
    let cx = Cx {
        doc,
        node,
        m,
        origin,
        ts,
    };
    let Some(comp) = comp_of(host, doc).filter(|c| c.node == node) else {
        // No composition: the text is typed text.
        let Some(c) = snapshot(host, &cx)? else {
            return Ok(());
        };
        let text: String = if c.kind.number {
            text.chars().filter(|ch| number_accepts(*ch)).collect()
        } else {
            text.to_owned()
        };
        if !text.is_empty() {
            edit(
                host,
                &cx,
                Edit::new(
                    "insertText",
                    Some(text.clone()),
                    (c.start, c.end),
                    text,
                    history::StepKind::Insert,
                ),
            )?;
        }
        return Ok(());
    };
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(());
    };
    // `maxlength` counts UTF-16 units of the value; the commit is cut to what the rest leaves.
    let mut text = text.to_owned();
    if !c.kind.multiline {
        text.retain(|ch| ch != '\n' && ch != '\r');
    }
    if let Some(max) = c.maxlength {
        let room =
            (max as i64 - (i64::from(c.len()) - i64::from(comp.end - comp.start))).max(0) as u32;
        let mut cut = String::new();
        let mut used = 0u32;
        for ch in text.chars() {
            if used + ch.len_utf16() as u32 > room {
                break;
            }
            used += ch.len_utf16() as u32;
            cut.push(ch);
        }
        text = cut;
    }
    fire(
        host,
        &cx,
        composing_input("beforeinput", "insertFromComposition", &text, false, m),
    )?;
    let caret = comp.start + utf16_len(&text);
    let ok = eng(host, doc, |_, d| {
        let c = Ctl::read(d, node)?;
        let v = replace_range(&c.value, comp.start, comp.end, &text);
        let mut w = d.control_state_writer();
        w.set_composition(node, None).ok()?;
        w.set_value(node, &v).ok()?;
        w.set_selection(
            node,
            Some(Selection {
                start: caret,
                end: caret,
                direction: SelectionDirection::None,
            }),
        )
        .ok()?;
        Some(())
    })?;
    if ok.is_none() {
        set_comp(host, doc, None);
        return Ok(());
    }
    let now = ts.unwrap_or_else(|| host.now_ms());
    history::record(
        host,
        &cx,
        history::StepKind::Composition,
        history::Applied {
            at: comp.start,
            removed: comp.original.clone(),
            inserted: text.clone(),
            before: comp.before,
            after: (caret, caret, SelectionDirection::None),
        },
        now,
    );
    set_comp(host, doc, None);
    fire(
        host,
        &cx,
        composition_event("compositionupdate", &text, false, m),
    )?;
    fire(
        host,
        &cx,
        composing_input("input", "insertFromComposition", &text, false, m),
    )?;
    fire(
        host,
        &cx,
        composition_event("compositionend", &text, false, m),
    )?;
    reveal_caret(host, &cx, caret, false)
}

/// End the composition of `doc` keeping the composed text as it is (`ImeDisabled`, blur, the window
/// losing focus, a pointer press): `compositionend` carries the text; no `input` event.
///
/// `reset` asks the platform to drop its own preedit too: for a pointer press or an element blur
/// inside a focused window (Tab, a host `focus`), where the platform still holds it and would
/// otherwise commit it again into the next control (0.5 F0501); not after `ImeDisabled` or the
/// window losing focus, where the platform has already committed or cleared it (011 Clarification Q3).
pub(crate) fn finish(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    reset: bool,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    let Some(comp) = comp_of(host, doc) else {
        return Ok(());
    };
    set_comp(host, doc, None);
    let cx = Cx {
        doc,
        node: comp.node,
        m,
        origin,
        ts,
    };
    let kept = eng(host, doc, |_, d| {
        let c = Ctl::read(d, comp.node)?;
        let _ = d.control_state_writer().set_composition(comp.node, None);
        let t = Chars::new(&c.value);
        let (s, e) = (t.index(comp.start), t.index(comp.end.min(c.len())));
        Some((
            t.chars[s..e.max(s)].iter().collect::<String>(),
            c.start.max(c.end),
        ))
    })?;
    let Some((text, _)) = kept else { return Ok(()) };
    let now = ts.unwrap_or_else(|| host.now_ms());
    if !text.is_empty() || !comp.original.is_empty() {
        let caret = comp.start + utf16_len(&text);
        history::record(
            host,
            &cx,
            history::StepKind::Composition,
            history::Applied {
                at: comp.start,
                removed: comp.original.clone(),
                inserted: text.clone(),
                before: comp.before,
                after: (caret, caret, SelectionDirection::None),
            },
            now,
        );
    }
    if reset {
        host.ime_reset(doc);
    }
    fire(
        host,
        &cx,
        composition_event("compositionend", &text, false, m),
    )?;
    Ok(())
}

/// `ImeDeleteSurrounding`: delete around the caret (`deleteContent`); the counts are UTF-16 units.
pub(crate) fn delete_surrounding(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    before: u32,
    after: u32,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    let Some(node) = target(host, doc)? else {
        return Ok(());
    };
    let cx = Cx {
        doc,
        node,
        m,
        origin,
        ts,
    };
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(());
    };
    if c.readonly || c.kind.password {
        return Ok(());
    }
    if let Some(mut comp) = comp_of(host, doc).filter(|k| k.node == node) {
        // The platform's surrounding text excludes preedit. In a native IME
        // transaction the old preedit is removed before surrounding deletion,
        // then the following commit/preedit replaces this now-empty range.
        let s = comp.start.saturating_sub(before);
        let e = comp.end.saturating_add(after).min(c.len());
        let mut event = input_event("beforeinput", "deleteContent", None, true, m);
        event.data.is_composing = Some(true);
        if fire(host, &cx, event)? {
            return Ok(());
        }
        if target(host, doc)? != Some(node)
            || snapshot(host, &cx)?.is_none_or(|now| now.value != c.value || now.readonly)
            || comp_of(host, doc).is_none_or(|now| now.start != comp.start || now.end != comp.end)
        {
            return Ok(());
        }
        let t = Chars::new(&c.value);
        let left: String = t.chars[t.index(s)..t.index(comp.start)].iter().collect();
        let right: String = t.chars[t.index(comp.end)..t.index(e)].iter().collect();
        // Fold the surrounding deletion into the same undo transaction as the
        // composition, including any text selected before composition began.
        comp.original = format!("{left}{}{right}", comp.original);
        let value = replace_range(&c.value, s, e, "");
        eng(host, doc, |_, d| {
            let mut w = d.control_state_writer();
            w.set_composition(node, None)?;
            w.set_value(node, &value)?;
            w.set_selection(
                node,
                Some(Selection {
                    start: s,
                    end: s,
                    direction: SelectionDirection::None,
                }),
            )?;
            w.set_composition(node, Some((s, s)))
        })??;
        comp.start = s;
        comp.end = s;
        set_comp(host, doc, Some(comp));
        fire(
            host,
            &cx,
            composition_event("compositionupdate", "", false, m),
        )?;
        let mut event = input_event("input", "deleteContent", None, false, m);
        event.data.is_composing = Some(true);
        fire(host, &cx, event)?;
        return reveal_caret(host, &cx, s, false);
    }
    let (s, e) = (
        c.start.saturating_sub(before),
        c.end.saturating_add(after).min(c.len()),
    );
    if s < e {
        edit(
            host,
            &cx,
            Edit::new(
                "deleteContent",
                None,
                (s, e),
                String::new(),
                history::StepKind::Other,
            ),
        )?;
    }
    Ok(())
}

/// A host value write abandoned the composition of `node`: forget it, no events. Returns whether a
/// composition was in progress (the host must then reset the platform's preedit too).
pub(crate) fn abandon(
    ix: &mut crate::interaction::dispatch::InteractionRuntime,
    doc: DocumentId,
    node: NodeId,
) -> bool {
    if let Some(s) = ix.docs.get_mut(&doc)
        && s.edit.comp.as_ref().is_some_and(|c| c.node == node)
    {
        s.edit.comp = None;
        return true;
    }
    false
}

/// Tell the host where the caret of `cx.node` is and what surrounds it.
pub(super) fn report(host: &mut dyn DispatchHost, cx: &Cx) {
    let Ok(Some(c)) = snapshot(host, cx) else {
        return;
    };
    if c.readonly {
        host.ime_update(cx.doc, None);
        return;
    }
    let comp = comp_of(host, cx.doc).filter(|k| k.node == cx.node);
    let purpose = eng(host, cx.doc, |_, d| purpose_of(d, cx.node, c.kind)).unwrap_or_default();
    let (anchor, focus) = c.anchor_focus();
    let surrounding = (purpose != ImePurposeKind::Password).then(|| {
        let (text, cur, anc) = match &comp {
            Some(k) => {
                let text = replace_range(&c.value, k.start, k.end, "");
                let t = Chars::new(&text);
                let b = byte_of(&t, &text, k.start);
                (text, b, b)
            }
            None => {
                let t = Chars::new(&c.value);
                (
                    c.value.clone(),
                    byte_of(&t, &c.value, focus),
                    byte_of(&t, &c.value, anchor),
                )
            }
        };
        surrounding_excerpt(&text, cur, anc)
    });
    let area = match host.control_geometry(cx.doc, cx.node) {
        Ok(Some(g)) => {
            let li = g.inner.line_of(focus, false);
            let l = &g.inner.lines[li];
            (
                g.origin.0 + f64::from(g.inner.x_of(li, focus)) - g.scroll.0,
                g.origin.1 + f64::from(l.top) - g.scroll.1,
                1.0,
                f64::from(l.height),
            )
        }
        _ => (0.0, 0.0, 0.0, 0.0),
    };
    host.ime_update(
        cx.doc,
        Some(ImeRequest {
            purpose,
            cursor_area: area,
            surrounding,
        }),
    );
}

fn purpose_of(
    d: &ui4me_document::document::Document,
    node: NodeId,
    kind: TextKind,
) -> ImePurposeKind {
    if kind.password {
        return ImePurposeKind::Password;
    }
    if kind.number {
        return ImePurposeKind::Number;
    }
    let ty = d
        .arena()
        .get(node)
        .and_then(|n| n.as_element())
        .and_then(|e| e.attr("type"))
        .map(|t| t.trim().to_ascii_lowercase());
    match ty.as_deref() {
        Some("email") => ImePurposeKind::Email,
        Some("url") => ImePurposeKind::Url,
        Some("tel") => ImePurposeKind::Phone,
        _ => ImePurposeKind::Normal,
    }
}

/// A text control lost focus or the window did: no IME input is wanted.
pub(crate) fn clear(host: &mut dyn DispatchHost, doc: DocumentId) {
    host.ime_update(doc, None);
}
