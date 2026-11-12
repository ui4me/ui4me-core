//! owner: 003-document-model
//!
//! Identity helpers (T010; research R-02). `NodeId`/`DocumentId` live in ui4me-core (001-owned);
//! this module adds the opaque 64-bit form `generation << 32 | index` used as the style key and
//! by hosts, and its inverse.

use std::num::{NonZeroU32, NonZeroU64};
use ui4me_engine_types::ids::{NodeId, RawId};

/// `generation << 32 | index`: non-zero because generations start at 1.
pub fn to_opaque(id: NodeId) -> NonZeroU64 {
    let r = id.raw();
    NonZeroU64::new((u64::from(r.generation.get()) << 32) | u64::from(r.index))
        .expect("generation is non-zero")
}

/// The inverse of [`to_opaque`]; `None` when the generation half is zero.
pub fn from_opaque(v: NonZeroU64) -> Option<NodeId> {
    let generation = NonZeroU32::new((v.get() >> 32) as u32)?;
    Some(NodeId::from_raw(RawId {
        index: v.get() as u32,
        generation,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_form_round_trips_and_encodes_generation_high() {
        let id = NodeId::from_raw(RawId {
            index: 7,
            generation: NonZeroU32::new(3).unwrap(),
        });
        let o = to_opaque(id);
        assert_eq!(o.get(), (3u64 << 32) | 7);
        assert_eq!(from_opaque(o), Some(id));
        assert_eq!(from_opaque(NonZeroU64::new(5).unwrap()), None);
    }
}
