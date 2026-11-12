//! owner: 001-vertical-slice — created by 001 slice (R-28)
//!
//! Identities (core-types.md §1). Semantics: node identity 003, ABI tokens 004 (R-37).
//! No ABI struct and no handle encoding live here (004 owns both).

use crate::codes_generated::Code;
use std::num::{NonZeroU32, NonZeroU64};

/// `{ index, generation }`; generation starts at 1 and is bumped when the slot is freed (AD-29).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RawId {
    pub index: u32,
    pub generation: NonZeroU32,
}

/// Generation-checked slot allocator shared by every id kind. A slot whose generation would
/// overflow is retired permanently (never reused), so there is no ABA problem.
#[derive(Debug, Default)]
pub struct Slots {
    /// Current generation per slot and whether it is live.
    slots: Vec<(NonZeroU32, bool)>,
    free: Vec<u32>,
    retired: usize,
}

impl Slots {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate a live id, reusing a freed (non-retired) slot first.
    pub fn alloc(&mut self) -> RawId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.1 = true;
            return RawId {
                index,
                generation: slot.0,
            };
        }
        let index = u32::try_from(self.slots.len()).expect("slot index space exhausted");
        self.slots.push((NonZeroU32::MIN, true));
        RawId {
            index,
            generation: NonZeroU32::MIN,
        }
    }

    /// Free a live id: bump its slot's generation, or retire the slot permanently if the
    /// generation would overflow. A stale id is `STALE_HANDLE`.
    pub fn free(&mut self, id: RawId) -> Result<(), Code> {
        if !self.is_live(id) {
            return Err(Code::StaleHandle);
        }
        let slot = &mut self.slots[id.index as usize];
        slot.1 = false;
        match slot.0.checked_add(1) {
            Some(g) => {
                slot.0 = g;
                self.free.push(id.index);
            }
            None => self.retired += 1,
        }
        Ok(())
    }

    pub fn is_live(&self, id: RawId) -> bool {
        self.slots
            .get(id.index as usize)
            .is_some_and(|&(g, live)| live && g == id.generation)
    }

    pub fn retired_count(&self) -> usize {
        self.retired
    }

    /// Test hook: set a live slot's generation (to exercise overflow retirement).
    #[doc(hidden)]
    pub fn force_generation_for_testing(&mut self, index: u32, generation: NonZeroU32) {
        self.slots[index as usize].0 = generation;
    }
}

macro_rules! raw_id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(RawId);
        impl $name {
            pub const fn from_raw(raw: RawId) -> Self {
                $name(raw)
            }
            pub const fn raw(self) -> RawId {
                self.0
            }
        }
    };
}
raw_id_type!(/// Process-unique runtime (at most one live, AD-03).
    RuntimeId);
raw_id_type!(/// Runtime-scoped window.
    WindowId);
raw_id_type!(/// Runtime-scoped document.
    DocumentId);
raw_id_type!(/// Document-scoped node; never names a generated or anonymous box (AD-01).
    NodeId);

/// A node across a component or ABI boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeKey {
    pub document: DocumentId,
    pub node: NodeId,
}

impl NodeKey {
    /// The node if `doc` is its document, else `WRONG_DOCUMENT`.
    pub fn node_in(&self, doc: DocumentId) -> Result<NodeId, Code> {
        if self.document == doc {
            Ok(self.node)
        } else {
            Err(Code::WrongDocument)
        }
    }
}

/// `(generation << 32) | index`: non-zero and generation-encoding (AD-07). x86_64 only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpaqueStyleKey(NonZeroU64);

impl OpaqueStyleKey {
    pub fn of(node: NodeId) -> Self {
        let r = node.raw();
        // generation >= 1, so the value is never zero
        let v = ((r.generation.get() as u64) << 32) | r.index as u64;
        OpaqueStyleKey(NonZeroU64::new(v).expect("generation is non-zero"))
    }
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// Monotonic per runtime from 1; never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DispatchId(NonZeroU64);

impl DispatchId {
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Debug, Default)]
pub struct DispatchIdSource {
    last: u64,
}

impl DispatchIdSource {
    pub fn new() -> Self {
        Self::default()
    }
    /// The next id: 1, 2, 3, … (u64 exhaustion is unreachable in practice and panics).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> DispatchId {
        self.last = self
            .last
            .checked_add(1)
            .expect("DispatchId space exhausted");
        DispatchId(NonZeroU64::new(self.last).expect("starts at 1"))
    }
}

macro_rules! token_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(NonZeroU64);
        impl $name {
            pub const fn new(v: u64) -> Option<Self> {
                match NonZeroU64::new(v) {
                    Some(n) => Some($name(n)),
                    None => None,
                }
            }
            pub const fn get(self) -> NonZeroU64 {
                self.0
            }
        }
    };
}
token_type!(/// Listener registration; writer InteractionRuntime (AD-28).
    ListenerToken);
token_type!(/// Host callback rooting token; writer HostRuntime callback table (AD-28, AD-43).
    CallbackToken);
