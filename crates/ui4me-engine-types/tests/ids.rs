// owner: 001-vertical-slice — T023 (core-types.md §1; AD-01, AD-07, AD-29)
use std::num::NonZeroU32;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::*;

#[test]
fn generation_starts_at_one_and_bumps_on_free() {
    let mut s = Slots::new();
    let a = s.alloc();
    assert_eq!(a.generation.get(), 1);
    assert!(s.is_live(a));
    s.free(a).unwrap();
    assert!(!s.is_live(a), "freed id is stale");
    let b = s.alloc();
    assert_eq!(b.index, a.index, "slot reused");
    assert_eq!(b.generation.get(), 2, "generation bumped on free");
    assert_ne!(a, b);
    assert_eq!(
        s.free(a),
        Err(Code::StaleHandle),
        "freeing a stale id is STALE_HANDLE"
    );
}

#[test]
fn overflowing_slot_is_retired_permanently_no_aba() {
    let mut s = Slots::new();
    let a = s.alloc();
    s.force_generation_for_testing(a.index, NonZeroU32::MAX);
    let top = RawId {
        index: a.index,
        generation: NonZeroU32::MAX,
    };
    assert!(s.is_live(top));
    s.free(top).unwrap();
    assert_eq!(s.retired_count(), 1);
    for _ in 0..4 {
        let n = s.alloc();
        assert_ne!(n.index, a.index, "a retired slot is never reused");
    }
    assert!(!s.is_live(top));
    assert!(
        !s.is_live(RawId {
            index: a.index,
            generation: NonZeroU32::MIN
        }),
        "no wrap to generation 1"
    );
}

#[test]
fn typed_ids_wrap_raw_ids() {
    let mut s = Slots::new();
    let r = s.alloc();
    let n = NodeId::from_raw(r);
    assert_eq!(n.raw(), r);
    assert_eq!(DocumentId::from_raw(r).raw(), r);
    assert_eq!(WindowId::from_raw(r).raw(), r);
    assert_eq!(RuntimeId::from_raw(r).raw(), r);
}

#[test]
fn node_key_mismatch_is_wrong_document() {
    let mut docs = Slots::new();
    let mut nodes = Slots::new();
    let d1 = DocumentId::from_raw(docs.alloc());
    let d2 = DocumentId::from_raw(docs.alloc());
    let n = NodeId::from_raw(nodes.alloc());
    let key = NodeKey {
        document: d1,
        node: n,
    };
    assert_eq!(key.node_in(d1), Ok(n));
    assert_eq!(key.node_in(d2), Err(Code::WrongDocument));
}

#[test]
fn opaque_style_key_is_nonzero_and_generation_encoding() {
    let raw0 = RawId {
        index: 0,
        generation: NonZeroU32::MIN,
    };
    let k0 = OpaqueStyleKey::of(NodeId::from_raw(raw0));
    assert_eq!(k0.get().get(), 1u64 << 32, "(generation << 32) | index");
    let reused = OpaqueStyleKey::of(NodeId::from_raw(RawId {
        index: 0,
        generation: NonZeroU32::new(2).unwrap(),
    }));
    assert_ne!(k0, reused, "reused slot gets a different key");
    let k = OpaqueStyleKey::of(NodeId::from_raw(RawId {
        index: 7,
        generation: NonZeroU32::new(3).unwrap(),
    }));
    assert_eq!(k.get().get(), (3u64 << 32) | 7);
}

#[test]
fn dispatch_ids_are_monotonic_from_one_and_never_reused() {
    let mut src = DispatchIdSource::new();
    let a = src.next();
    let b = src.next();
    let c = src.next();
    assert_eq!(a.get().get(), 1);
    assert!(a < b && b < c);
    assert_eq!(c.get().get(), 3);
}

#[test]
fn tokens_are_nonzero() {
    assert!(ListenerToken::new(0).is_none());
    assert_eq!(ListenerToken::new(5).unwrap().get().get(), 5);
    assert!(CallbackToken::new(0).is_none());
    assert_eq!(CallbackToken::new(9).unwrap().get().get(), 9);
}
