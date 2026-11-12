//! owner: 007-resources (T003) — shared resource identities and epochs (resource-view.md §1).
//!
//! `ResourceEpoch` (per document) and `FontEpoch` (per runtime) are the `resource` and `font`
//! inputs of 005's revision tuple (R-37). Semantics and allocation belong to 007's ResourceManager.

use crate::ids::DocumentId;

/// Per-document request identity: `serial` is unique within the document and never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId {
    pub document: DocumentId,
    pub serial: u64,
}

/// A per-requester handle: `revision` increments on `src`/`srcset`/density change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceRef {
    pub document: DocumentId,
    pub request: RequestId,
    pub revision: u32,
}

/// Per-document resource epoch; +1 per applied completion batch that changes an observable value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceEpoch(pub u64);

/// Per-runtime font epoch; +1 per font-source-set change batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontEpoch(pub u64);

impl ResourceEpoch {
    pub const fn next(self) -> Self {
        ResourceEpoch(self.0 + 1)
    }
}

impl FontEpoch {
    pub const fn next(self) -> Self {
        FontEpoch(self.0 + 1)
    }
}
