//! owner: 005-pipeline-diagnostics — T049. Snapshot views (recompute-plan §9): a view pins one
//! committed set; `release` is idempotent in every lifecycle state; a read through a released
//! view is `SNAPSHOT_RETIRED` (O6); from `Closing` on, reads are `DOCUMENT_CLOSED` while release
//! still succeeds (R-34). A set is freed once no view and no consumer holds it (the pipeline,
//! raster in use by a frame, exports in progress, views).

use crate::pipeline::publish::CommittedSet;
use crate::pipeline::read::ReadResult;
use crate::pipeline::state::DocumentPipeline;
use std::collections::HashMap;
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// A snapshot-view handle (per document; ids are never reused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewId(pub u64);

/// Only live views occupy storage. Since IDs are monotonic, an absent issued ID
/// is retired; retaining one tombstone per released view is unnecessary.
#[derive(Debug, Default)]
pub struct Views {
    next: u64,
    pinned: HashMap<ViewId, Arc<CommittedSet>>,
}

impl Views {
    /// Views currently pinning a set.
    pub fn live(&self) -> usize {
        self.pinned.len()
    }

    #[cfg(test)]
    pub(crate) fn retained_entries(&self) -> usize {
        self.pinned.len()
    }
}

impl DocumentPipeline {
    /// Pin the current committed set in a new view. O1 for a `Closing`/`Closed` document;
    /// O5 (`SNAPSHOT_NOT_READY`) before the first publication.
    pub fn pin_view(&mut self) -> Result<ViewId, Status> {
        self.check_open()?;
        let set = self.committed.clone().ok_or_else(|| {
            Status::new(Code::SnapshotNotReady, Stage::Host)
                .with_message("no committed set has been published yet")
        })?;
        if self.views.pinned.len() >= 1024 {
            return Err(Status::new(Code::SizeLimit, Stage::Host)
                .with_message("too many live snapshot views"));
        }
        let id = ViewId(self.views.next);
        self.views.next = self.views.next.checked_add(1).ok_or_else(|| {
            Status::new(Code::SizeLimit, Stage::Host).with_message("snapshot ID space exhausted")
        })?;
        self.views.pinned.insert(id, set);
        Ok(id)
    }

    /// A read through a view: the pinned set with its own revision tuple, `current` evaluated now.
    /// O1 when the document is `Closing`/`Closed`, O6 after release, `STALE_HANDLE` for an id this
    /// document never issued.
    pub fn read_view(&self, view: ViewId) -> Result<ReadResult, Status> {
        self.check_open()?;
        match self.views.pinned.get(&view) {
            None if view.0 >= self.views.next => {
                Err(Status::new(Code::StaleHandle, Stage::Host)
                    .with_message("unknown snapshot view"))
            }
            None => Err(Status::new(Code::SnapshotRetired, Stage::Host)
                .with_message("the snapshot view was released")),
            Some(set) => {
                let committed = set.clone();
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
        }
    }

    /// `release(view)`: idempotent, and OK in every lifecycle state (R-34), including for an id
    /// that was already released. An id this document never issued is `STALE_HANDLE`.
    pub fn release_view(&mut self, view: ViewId) -> Result<(), Status> {
        if view.0 >= self.views.next {
            return Err(
                Status::new(Code::StaleHandle, Stage::Host).with_message("unknown snapshot view")
            );
        }
        self.views.pinned.remove(&view);
        Ok(())
    }

    pub fn views(&self) -> &Views {
        &self.views
    }
}
