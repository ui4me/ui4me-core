//! owner: 012-interaction (lane L-CLIP-r13; `contracts/editing.md` section 4)
//!
//! Undo and redo of a text control: a list of steps `{at, removed, inserted, selection before and
//! after}` per control, at most [`LIMIT`] steps (the oldest dropped). Consecutive `insertText`
//! steps that continue each other within [`COALESCE_MS`] merge into one; a caret move (the next
//! step no longer starts where the last one ended), another command kind, a paste, a cut or a
//! composition ends a run. Any new edit clears the redo stack; a host value write clears the whole
//! history. Undo and redo are edits themselves: `beforeinput` (`historyUndo`/`historyRedo`,
//! cancelable) -> apply -> `input`.

use super::{Cx, Edit, edit, snapshot, utf16_len};
use crate::interaction::dispatch::{DispatchHost, rt};
use ui4me_document::control_state::SelectionDirection;
use ui4me_engine_types::status::Status;

/// Steps kept per control.
pub(super) const LIMIT: usize = 100;
/// Typing pauses longer than this start a new step.
pub(super) const COALESCE_MS: f64 = 1000.0;

/// What produced a step (only runs of `Insert` merge).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StepKind {
    Insert,
    Paste,
    Cut,
    Composition,
    Other,
}

/// The effect of one applied edit, UTF-16 offsets into the value before it.
#[derive(Clone, Debug)]
pub(super) struct Applied {
    pub at: u32,
    pub removed: String,
    pub inserted: String,
    pub before: (u32, u32, SelectionDirection),
    pub after: (u32, u32, SelectionDirection),
}

#[derive(Clone, Debug)]
pub(super) struct Step {
    kind: StepKind,
    a: Applied,
    time: f64,
}

#[derive(Debug, Default)]
pub(super) struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl History {
    fn push(&mut self, kind: StepKind, a: Applied, time: f64) {
        self.redo.clear();
        if let Some(last) = self.undo.last_mut()
            && kind == StepKind::Insert
            && last.kind == StepKind::Insert
            && last.a.removed.is_empty()
            && a.removed.is_empty()
            && a.at == last.a.at + utf16_len(&last.a.inserted)
            && time - last.time <= COALESCE_MS
        {
            last.a.inserted.push_str(&a.inserted);
            last.a.after = a.after;
            last.time = time;
            return;
        }
        self.undo.push(Step { kind, a, time });
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }
}

/// Record an applied edit as an undo step.
pub(super) fn record(host: &mut dyn DispatchHost, cx: &Cx, kind: StepKind, a: Applied, time: f64) {
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(cx.doc) {
            s.edit
                .history
                .entry(cx.node)
                .or_default()
                .push(kind, a, time);
        }
    });
}

/// Forget the history of `node` (a host value write, a type change).
pub(super) fn clear(
    ix: &mut crate::interaction::dispatch::InteractionRuntime,
    doc: ui4me_engine_types::ids::DocumentId,
    node: ui4me_engine_types::ids::NodeId,
) {
    if let Some(s) = ix.docs.get_mut(&doc) {
        s.edit.history.remove(&node);
    }
}

/// Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z.
pub(super) fn undo_redo(host: &mut dyn DispatchHost, cx: &Cx, redo: bool) -> Result<(), Status> {
    if snapshot(host, cx)?.is_none_or(|c| c.readonly) {
        return Ok(());
    }
    let step = rt(host, |ix| {
        let h = ix.doc_state(cx.doc).ok()?.edit.history.get_mut(&cx.node)?;
        if redo { h.redo.pop() } else { h.undo.pop() }
    });
    let Some(step) = step else { return Ok(()) };
    let a = &step.a;
    let e = if redo {
        Edit {
            input_type: "historyRedo",
            data: None,
            range: (a.at, a.at + utf16_len(&a.removed)),
            replacement: a.inserted.clone(),
            record: None,
            selection_after: Some(a.after),
        }
    } else {
        Edit {
            input_type: "historyUndo",
            data: None,
            range: (a.at, a.at + utf16_len(&a.inserted)),
            replacement: a.removed.clone(),
            record: None,
            selection_after: Some(a.before),
        }
    };
    let applied = edit(host, cx, e)?;
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(cx.doc) {
            let h = s.edit.history.entry(cx.node).or_default();
            // A canceled or failed undo leaves the step where it was.
            match (applied, redo) {
                (true, false) => h.redo.push(step),
                (true, true) => h.undo.push(step),
                (false, false) => h.undo.push(step),
                (false, true) => h.redo.push(step),
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(at: u32, removed: &str, inserted: &str) -> Applied {
        Applied {
            at,
            removed: removed.into(),
            inserted: inserted.into(),
            before: (at, at + utf16_len(removed), SelectionDirection::None),
            after: (
                at + utf16_len(inserted),
                at + utf16_len(inserted),
                SelectionDirection::None,
            ),
        }
    }

    #[test]
    fn typing_runs_merge_until_a_pause_a_caret_move_or_another_kind() {
        let mut h = History::default();
        h.push(StepKind::Insert, ap(0, "", "a"), 0.0);
        h.push(StepKind::Insert, ap(1, "", "b"), 100.0);
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo[0].a.inserted, "ab");
        h.push(StepKind::Insert, ap(5, "", "c"), 200.0);
        assert_eq!(h.undo.len(), 2, "a caret move starts a new step");
        h.push(StepKind::Insert, ap(6, "", "d"), 2000.0);
        assert_eq!(
            h.undo.len(),
            3,
            "a pause longer than a second starts a new step"
        );
        h.push(StepKind::Paste, ap(7, "", "xyz"), 2100.0);
        h.push(StepKind::Insert, ap(10, "", "e"), 2200.0);
        assert_eq!(
            h.undo.len(),
            5,
            "a paste is a step of its own and ends the run"
        );
    }

    #[test]
    fn a_new_edit_clears_redo_and_the_history_is_limited() {
        let mut h = History::default();
        h.redo.push(Step {
            kind: StepKind::Other,
            a: ap(0, "", "x"),
            time: 0.0,
        });
        h.push(StepKind::Other, ap(0, "", "y"), 1.0);
        assert!(h.redo.is_empty());
        for i in 0..(LIMIT as u32 + 20) {
            h.push(StepKind::Other, ap(i, "", "z"), f64::from(i));
        }
        assert_eq!(h.undo.len(), LIMIT);
    }
}
