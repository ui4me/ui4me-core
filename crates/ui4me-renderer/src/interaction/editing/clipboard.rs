//! owner: 012-interaction (lane L-CLIP-r13; `contracts/editing.md` section 3)
//!
//! Copy, cut and paste. The shortcut (Ctrl+C / Ctrl+Insert, Ctrl+X / Shift+Delete, Ctrl+V /
//! Shift+Insert; Cmd for Meta) fires a trusted, cancelable `ClipboardEvent` at the focused element
//! whose `clipboardData` is the DataTransfer of `ui4me_engine_types::clipboard::ClipboardData`:
//!
//! - `copy` / `cut`: an empty store the listener may fill with `setData` (`text/plain`, `text/html`).
//!   Not canceled: the selected plain text is written to the clipboard (a password control writes
//!   nothing), and `cut` then deletes it (`beforeinput` `deleteByCut`) only after the write was
//!   accepted. Canceled: what the listener put in the store is written (nothing when it is empty) and
//!   the default deletion of `cut` is the listener's.
//! - `paste`: a read-only store holding the clipboard content. Not canceled: its `text/plain` is
//!   inserted at the selection (`beforeinput` `insertFromPaste`, one undo step). Canceled: nothing is
//!   inserted; the listener took over.
//!
//! Native desktop hosts defer reads/writes through `DispatchHost::defer_clipboard_*`; the
//! document's bounded transport validates the target again on completion. Memory/test hosts
//! retain the synchronous `clipboard_read/write` fallback. Failed writes never cut text.
//! Native transfers are plain text only and capped at 1 MiB; the retained internal clipboard
//! model has a separate 16 MiB ceiling for donor regressions.

use super::{Cx, Edit, TextKind, edit, history, number_accepts, snapshot};
use crate::interaction::dispatch::{DispatchHost, rt, run_dispatch};
use crate::interaction::frame::{EventInit, EventInterface, Modifiers};
use ui4me_engine_types::clipboard::ClipboardData;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// The clipboard command a key combination asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Copy,
    Cut,
    Paste,
}

impl Command {
    fn event_type(self) -> &'static str {
        match self {
            Command::Copy => "copy",
            Command::Cut => "cut",
            Command::Paste => "paste",
        }
    }
}

/// The clipboard command of a keydown (`key` is the UI Events key value), if it is one.
pub(crate) fn command_of(key: &str, m: Modifiers) -> Option<Command> {
    let ctrl = m.ctrl || m.meta;
    if m.alt {
        return None;
    }
    if ctrl && !m.shift {
        return match key {
            "Insert" => Some(Command::Copy),
            k if k.eq_ignore_ascii_case("c") => Some(Command::Copy),
            k if k.eq_ignore_ascii_case("x") => Some(Command::Cut),
            k if k.eq_ignore_ascii_case("v") => Some(Command::Paste),
            _ => None,
        };
    }
    if ctrl && m.shift && key.eq_ignore_ascii_case("v") {
        // Paste without formatting: the same plain-text paste.
        return Some(Command::Paste);
    }
    if m.shift && !ctrl {
        return match key {
            "Delete" => Some(Command::Cut),
            "Insert" => Some(Command::Paste),
            _ => None,
        };
    }
    None
}

fn clip_event(command: Command, data: ClipboardData, m: Modifiers) -> EventInit {
    let mut e = EventInit::new(command.event_type())
        .interface(EventInterface::ClipboardEvent)
        .bubbles(true)
        .cancelable(true);
    e.clipboard = Some(data);
    e.modifiers = m;
    e
}

/// Fire a clipboard event at `target`; returns whether it was canceled and the store as the listeners left it.
#[allow(clippy::too_many_arguments)]
fn dispatch(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    command: Command,
    data: ClipboardData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(bool, ClipboardData), Status> {
    rt(host, |ix| ix.last_clipboard = None);
    let out = run_dispatch(
        host,
        doc,
        target,
        clip_event(command, data, m),
        true,
        origin,
        ts,
    )?;
    let store = rt(host, |ix| ix.last_clipboard.take()).unwrap_or_default();
    Ok((out.canceled, store))
}

fn note_unavailable(host: &mut dyn DispatchHost, doc: DocumentId, what: &str, e: &Status) {
    let st = Status::new(
        if e.code == Code::SizeLimit {
            Code::SizeLimit
        } else {
            Code::UnsupportedDefaultAction
        },
        Stage::Event,
    )
    .with_message(format!("{what}: {}", e.message));
    host.record_error(doc, st);
}

/// Write `data` to the host clipboard; a failure is recorded and returned as `false`.
fn write(host: &mut dyn DispatchHost, doc: DocumentId, data: &ClipboardData) -> bool {
    if host.defer_clipboard_write(doc, None, data, false) {
        return true;
    }
    match host.clipboard_write(data) {
        Ok(()) => true,
        Err(e) => {
            note_unavailable(host, doc, "clipboard write", &e);
            false
        }
    }
}

/// Copy or cut at a text control (`cx.node`).
pub(super) fn copy_or_cut(
    host: &mut dyn DispatchHost,
    cx: &Cx,
    command: Command,
) -> Result<(), Status> {
    let Some(c) = snapshot(host, cx)? else {
        return Ok(());
    };
    let (selected, secret) = (selected_text(&c), c.kind.password);
    let (canceled, store) = dispatch(
        host,
        cx.doc,
        cx.node,
        command,
        ClipboardData::default(),
        cx.m,
        cx.origin,
        cx.ts,
    )?;
    if canceled {
        if !store.is_empty() {
            write(host, cx.doc, &store);
        }
        return Ok(());
    }
    // Password controls never put their value on the clipboard (events fire; no write, no deletion).
    if secret || selected.is_empty() {
        return Ok(());
    }
    let data = ClipboardData::text(selected);
    if host.defer_clipboard_write(cx.doc, Some(cx.node), &data, command == Command::Cut) {
        return Ok(());
    }
    if !write(host, cx.doc, &data) {
        return Ok(());
    }
    if command == Command::Cut {
        // The selection may have been changed by a listener: cut what is selected now.
        let Some(c) = snapshot(host, cx)? else {
            return Ok(());
        };
        if c.start != c.end {
            edit(
                host,
                cx,
                Edit::new(
                    "deleteByCut",
                    None,
                    (c.start, c.end),
                    String::new(),
                    history::StepKind::Cut,
                ),
            )?;
        }
    }
    Ok(())
}

/// Paste into a text control (`cx.node`).
pub(super) fn paste(host: &mut dyn DispatchHost, cx: &Cx) -> Result<(), Status> {
    if host.defer_clipboard_paste(cx.doc, cx.node) {
        return Ok(());
    }
    let content = match host
        .clipboard_read()
        .and_then(|d| d.check_size().map(|()| d))
    {
        Ok(d) => d,
        Err(e) => {
            note_unavailable(host, cx.doc, "clipboard read", &e);
            ClipboardData::default()
        }
    };
    let (canceled, _) = dispatch(
        host,
        cx.doc,
        cx.node,
        Command::Paste,
        content.clone(),
        cx.m,
        cx.origin,
        cx.ts,
    )?;
    if canceled {
        return Ok(());
    }
    let Some(text) = content.plain.filter(|t| !t.is_empty()) else {
        return Ok(());
    };
    let Some(c) = snapshot(host, cx)? else {
        return Ok(());
    };
    if c.readonly {
        return Ok(());
    }
    let text = match c.kind {
        TextKind { number: true, .. } => text.chars().filter(|ch| number_accepts(*ch)).collect(),
        _ => text.replace("\r\n", "\n").replace('\r', "\n"),
    };
    if text.is_empty() {
        return Ok(());
    }
    edit(
        host,
        cx,
        Edit::new(
            "insertFromPaste",
            Some(text.clone()),
            (c.start, c.end),
            text,
            history::StepKind::Paste,
        ),
    )?;
    Ok(())
}

/// The selected plain text of a control.
fn selected_text(c: &super::Ctl) -> String {
    let t = super::Chars::new(&c.value);
    t.chars[t.index(c.start)..t.index(c.end)].iter().collect()
}

/// A clipboard shortcut where no text control has focus: the event fires at `target` (the focused
/// element or the body) so application code can supply the data of `copy`/`cut` (`setData` plus
/// `preventDefault`) and handle `paste`. The engine has no selection over static document text, so
/// there is no default action.
pub(crate) fn document_command(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    command: Command,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    if command == Command::Paste && host.defer_clipboard_paste(doc, target) {
        return Ok(());
    }
    let content = if command == Command::Paste {
        match host
            .clipboard_read()
            .and_then(|d| d.check_size().map(|()| d))
        {
            Ok(d) => d,
            Err(e) => {
                note_unavailable(host, doc, "clipboard read", &e);
                ClipboardData::default()
            }
        }
    } else {
        ClipboardData::default()
    };
    let (canceled, store) = dispatch(host, doc, target, command, content, m, origin, ts)?;
    if canceled && command != Command::Paste && !store.is_empty() {
        write(host, doc, &store);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(ctrl: bool, shift: bool) -> Modifiers {
        Modifiers {
            ctrl,
            shift,
            alt: false,
            meta: false,
        }
    }

    #[test]
    fn shortcuts_map_to_commands() {
        assert_eq!(command_of("c", m(true, false)), Some(Command::Copy));
        assert_eq!(command_of("C", m(true, false)), Some(Command::Copy));
        assert_eq!(command_of("x", m(true, false)), Some(Command::Cut));
        assert_eq!(command_of("v", m(true, false)), Some(Command::Paste));
        assert_eq!(command_of("v", m(true, true)), Some(Command::Paste));
        assert_eq!(command_of("Insert", m(true, false)), Some(Command::Copy));
        assert_eq!(command_of("Insert", m(false, true)), Some(Command::Paste));
        assert_eq!(command_of("Delete", m(false, true)), Some(Command::Cut));
        assert_eq!(
            command_of("c", m(true, true)),
            None,
            "Ctrl+Shift+C is not copy"
        );
        assert_eq!(command_of("c", m(false, false)), None);
        let meta = Modifiers {
            meta: true,
            ..Modifiers::default()
        };
        assert_eq!(command_of("v", meta), Some(Command::Paste));
        let alt = Modifiers {
            ctrl: true,
            alt: true,
            ..Modifiers::default()
        };
        assert_eq!(command_of("c", alt), None);
    }
}
