//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Flush and Committed reads (005 recompute-plan §11, the single outcome vocabulary, R-37).
//! `Ok` = O2/O3 (diagnostics attached for O3); `Err` = O1, O4 (`FailedUpdate`; committed set
//! untouched) or O5 (`SNAPSHOT_NOT_READY`). O6 belongs to snapshot views.
//! Implemented by 001 T049.
//!
//! O1 here covers the lifecycle (`DOCUMENT_CLOSED`); thread, handle and document checks are
//! the runtime's (004), evaluated before it calls into the pipeline.

use crate::pipeline::publish::CommittedSet;
use crate::pipeline::state::{DocumentPipeline, Trigger};
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::revision::{RevisionInputs, RevisionTuple};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

pub use crate::pipeline::domain::ReadMode;

#[derive(Clone, Debug)]
pub struct ReadResult {
    pub committed: Arc<CommittedSet>,
    /// Equal to `committed.revision`.
    pub revision: RevisionTuple,
    /// The revision's inputs equal the document's current inputs.
    pub current: bool,
    /// O3 attached diagnostics (never surfaced as an error status).
    pub diagnostics: Vec<Status>,
}

impl DocumentPipeline {
    /// One derived read. `Flush` = `TRIGGER(flush-read)` then the committed result; `Committed`
    /// never recomputes.
    pub fn read(&mut self, mode: ReadMode) -> Result<ReadResult, Status> {
        self.read_with(mode, Trigger::FlushRead)
    }

    /// [`DocumentPipeline::read`] with the trigger kind of a Flush (`probe` for exports).
    pub fn read_with(&mut self, mode: ReadMode, trigger: Trigger) -> Result<ReadResult, Status> {
        // O1
        self.check_open()?;
        if mode == ReadMode::Flush {
            // O4 propagates with stage and diagnostic id; the committed set is untouched.
            self.trigger(trigger)?;
        }
        self.committed_result()
    }

    /// The committed set as a read result, or O5.
    pub fn committed_result(&self) -> Result<ReadResult, Status> {
        let committed = self.committed.clone().ok_or_else(|| {
            Status::new(Code::SnapshotNotReady, Stage::Host)
                .with_message("no committed set has been published yet")
        })?;
        let revision = committed.revision;
        let current = revision.is_current(self.doc.id(), &self.current_inputs());
        let diagnostics = committed.diagnostics.clone();
        Ok(ReadResult {
            committed,
            revision,
            current,
            diagnostics,
        })
    }

    /// `read(ReadMode::Flush)`.
    pub fn flush(&mut self) -> Result<ReadResult, Status> {
        self.read(ReadMode::Flush)
    }

    /// The document's current input epochs.
    pub fn current_inputs(&self) -> RevisionInputs {
        RevisionInputs {
            dom: self.doc.revision(),
            environment: self.environment_epoch,
            resource: self.resource_epoch,
            font: self.fonts.font_epoch.0,
            scroll: self.scroll_epoch,
            animation: self.clock.epoch(),
        }
    }
}

// ---- recompute-plan §11: the single read-outcome vocabulary (T021; R-37) -------------------------

/// The row of the read-mode outcome table a derived read ends in. 004 maps rows to ABI
/// statuses and binding exceptions; 006 and 008 reference this table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadOutcome {
    /// O1: precondition error (`STALE_HANDLE`, `WRONG_DOCUMENT`, `WRONG_THREAD`, `DOCUMENT_CLOSED`).
    O1PreconditionError,
    /// O2: success.
    O2Success,
    /// O3: success with attached diagnostics (never surfaced as an error status).
    O3SuccessWithDiagnostics,
    /// O4: update failure (`FailedUpdate`; committed set untouched).
    O4UpdateFailure,
    /// O5: not yet published (`SNAPSHOT_NOT_READY`).
    O5NotYetPublished,
    /// O6: view released (`SNAPSHOT_RETIRED`).
    O6ViewReleased,
}

impl ReadOutcome {
    /// The row label (`O1`…`O6`).
    pub const fn row(self) -> &'static str {
        match self {
            ReadOutcome::O1PreconditionError => "O1",
            ReadOutcome::O2Success => "O2",
            ReadOutcome::O3SuccessWithDiagnostics => "O3",
            ReadOutcome::O4UpdateFailure => "O4",
            ReadOutcome::O5NotYetPublished => "O5",
            ReadOutcome::O6ViewReleased => "O6",
        }
    }

    /// The row an error `code` belongs to; `None` for a code that is no read outcome.
    pub fn of_error(code: Code) -> Option<ReadOutcome> {
        match code {
            Code::StaleHandle | Code::WrongDocument | Code::WrongThread | Code::DocumentClosed => {
                Some(ReadOutcome::O1PreconditionError)
            }
            Code::SnapshotNotReady => Some(ReadOutcome::O5NotYetPublished),
            Code::SnapshotRetired => Some(ReadOutcome::O6ViewReleased),
            // `ENGINE_MISCONFIGURED` (a missing mandatory adapter) is an O4 failure.
            Code::OutOfMemory
            | Code::InternalInvariant
            | Code::BudgetExceeded
            | Code::EngineMisconfigured => Some(ReadOutcome::O4UpdateFailure),
            _ => None,
        }
    }

    /// Classify one derived-read result.
    pub fn of(result: &Result<ReadResult, Status>) -> Option<ReadOutcome> {
        match result {
            Ok(r) if r.diagnostics.is_empty() => Some(ReadOutcome::O2Success),
            Ok(_) => Some(ReadOutcome::O3SuccessWithDiagnostics),
            Err(s) => ReadOutcome::of_error(s.code),
        }
    }
}

/// The facts the O1 checks read, in the order they are evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preconditions {
    pub on_owner_thread: bool,
    pub handle_valid: bool,
    pub document_matches: bool,
    /// The document is `Closing` or `Closed`.
    pub document_closing: bool,
}

/// O1 in the fixed order: thread, handle validity, document match, lifecycle. The first failing
/// check decides; nothing is evaluated after it, and no update runs.
pub fn check_preconditions(p: Preconditions) -> Result<(), Status> {
    if !p.on_owner_thread {
        return Err(Status::new(Code::WrongThread, Stage::Host)
            .with_message("derived reads run on the owner thread"));
    }
    if !p.handle_valid {
        return Err(Status::new(Code::StaleHandle, Stage::Host).with_message("the handle is stale"));
    }
    if !p.document_matches {
        return Err(Status::new(Code::WrongDocument, Stage::Host)
            .with_message("the handle belongs to another document"));
    }
    if p.document_closing {
        return Err(Status::new(Code::DocumentClosed, Stage::Host)
            .with_message("the document is closing or closed"));
    }
    Ok(())
}
