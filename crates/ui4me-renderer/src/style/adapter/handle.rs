//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! The style handle (T041; AD-07 as amended by **AD-P-001-y(a)**, approved by the owner
//! 2026-11-05, AUTHORISATIONS.md E-004; X-STYLE-ADAPTER RESULT-2 PASS). Ported from
//! `verification/gates/X-STYLE-ADAPTER/harness/src/handle.rs`; soundness argument in that
//! gate's `evidence/SOUNDNESS.md` §0–§2 and `SOUNDNESS-2.md`, review `REVIEW-2.md`.
//!
//! Logically the handle is the AD-07 triple `{view, sidecar, NodeId}`. Physically it is one
//! word, `StyleNode { id: NodeId }`: Stylo's style-sharing cache transmutes the typeless
//! `StyleSharingCandidate<usize>` cache to `StyleSharingCandidate<E>` and asserts equal size
//! and alignment (`style/sharing/mod.rs:588-608 @37c1ed3`), so `E` must be exactly
//! pointer-sized. `view` (a shared borrow of the Document for the whole pass) and `sidecar`
//! (the address-stable, interior-mutable style state) are bound per **style pass** on the
//! owner thread by [`with_pass`]; the handle resolves them from a thread-local only inside that
//! pass. Handles can be minted only from `&'s Pass` inside the `with_pass` closure, so they
//! cannot outlive the pass. Nested passes panic. Handles are `!Send`/`!Sync`.
//!
//! The identity is `ui4me_engine_types::ids::NodeId`; Stylo's `OpaqueNode`/`OpaqueElement` keys are
//! `ui4me_engine_types::ids::OpaqueStyleKey` (`(generation << 32) | index`; non-zero because the
//! generation is never zero; injective because a slot whose generation is exhausted is retired).

use crate::style::sidecar::SidecarStore;
use std::cell::Cell;
use std::marker::PhantomData;
use ui4me_document::document::Document;
use ui4me_engine_types::ids::NodeId;

#[cfg(not(target_pointer_width = "64"))]
compile_error!(
    "the one-word StyleNode handle and OpaqueStyleKey require a 64-bit target (review F3/F7)"
);

/// The bound `{view, sidecar}` of one pass. Not constructible outside this module.
pub struct Pass<'a> {
    /// The pass borrows `view` and `sidecar` for `'a` (they are held in `CURRENT`); `!Send`.
    _bound: PhantomData<(&'a Document, &'a SidecarStore, *const ())>,
}

/// `repr(C, align(8))` (review F3): lays out exactly like `usize` on 64-bit targets, which
/// Stylo's sharing-cache transmute assumes.
#[derive(Clone, Copy)]
#[repr(C, align(8))]
pub struct StyleNode<'a> {
    pub(crate) id: NodeId,
    /// Ties the handle to the `&'a Pass` it was minted from, and makes it `!Send + !Sync`.
    _pass: PhantomData<(&'a Document, &'a SidecarStore, *const ())>,
}

const _: () = assert!(std::mem::size_of::<StyleNode<'static>>() == std::mem::size_of::<usize>());
const _: () = assert!(std::mem::align_of::<StyleNode<'static>>() == std::mem::align_of::<usize>());
const _: () = assert!(std::mem::size_of::<NodeId>() == std::mem::size_of::<u64>());

type Ctx = (*const Document, *const SidecarStore);

thread_local! {
    /// The `(view, sidecar)` of the style pass active on this thread, if any.
    static CURRENT: Cell<Option<Ctx>> = const { Cell::new(None) };
}

#[inline]
fn ctx() -> Ctx {
    CURRENT
        .with(|c| c.get())
        .expect("StyleNode used outside its style pass or off the owner thread")
}

impl<'a> StyleNode<'a> {
    pub fn id(&self) -> NodeId {
        self.id
    }

    #[inline]
    pub(crate) fn view(&self) -> &'a Document {
        // SAFETY: a `StyleNode<'a>` can only be created by `Pass::handle(&'a self)` (or `with`,
        // from another handle of the same `'a`), i.e. only inside the `with_pass` closure, while
        // `CURRENT` holds pointers derived from that pass's `&Document`. That borrow outlives
        // `'a` (the closure is `for<'s> FnOnce(&'s Pass<'p>)` with `'p: 's`), `CURRENT` is reset
        // only after the closure returns or unwinds, and `with_pass` refuses to nest, so
        // `CURRENT` cannot be repointed while `'a` is live. Handles are `!Send`, so they are only
        // dereferenced on the thread whose `CURRENT` they came from. The Document is only
        // shared-borrowed for the whole pass (all DOM mutation takes `&mut Document`).
        unsafe { &*ctx().0 }
    }

    #[inline]
    pub(crate) fn sidecars(&self) -> &'a SidecarStore {
        // SAFETY: as for `view` (the same pass, the same shared borrow of the store; the store
        // changes only through `&mut SidecarStore`, impossible while the pass borrows it).
        unsafe { &*ctx().1 }
    }

    #[inline]
    pub(crate) fn with(&self, id: NodeId) -> Self {
        StyleNode {
            id,
            _pass: PhantomData,
        }
    }
}

impl<'a> Pass<'a> {
    pub(crate) fn handle<'s>(&'s self, id: NodeId) -> StyleNode<'s> {
        StyleNode {
            id,
            _pass: PhantomData,
        }
    }
}

/// Run `f` with `{view, sidecar}` bound for one pass on this thread. Nested passes panic (AD-07:
/// no nested traversal); the binding is cleared even if `f` unwinds.
pub(crate) fn with_pass<'a, R>(
    view: &'a Document,
    sidecars: &'a SidecarStore,
    f: impl for<'s> FnOnce(&'s Pass<'a>) -> R,
) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CURRENT.with(|c| c.set(None));
        }
    }
    CURRENT.with(|c| {
        assert!(
            c.get().is_none(),
            "nested style pass (AD-07: no nested traversal)"
        );
        c.set(Some((
            view as *const Document,
            sidecars as *const SidecarStore,
        )));
    });
    let _reset = Reset;
    f(&Pass {
        _bound: PhantomData,
    })
}

/// Size facts (evidence and tests).
pub fn handle_size() -> usize {
    std::mem::size_of::<StyleNode<'static>>()
}
pub fn handle_align() -> usize {
    std::mem::align_of::<StyleNode<'static>>()
}
