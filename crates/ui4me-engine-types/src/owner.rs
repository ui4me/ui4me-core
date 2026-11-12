//! owner: 001-vertical-slice — created by 001 slice (R-28)
//!
//! Owner-thread proof (core-types.md §6; semantics 004). Zero-sized, `!Send`, `!Sync`.
//! Only the runtime creates it; every owner-thread-confined entry point requires it.
//!
//! ```compile_fail,E0277
//! fn need_send<T: Send>(_: T) {}
//! fn f(cx: ui4me_engine_types::owner::OwnerCx<'_>) { need_send(cx) }
//! ```
//!
//! ```compile_fail,E0277
//! fn need_sync<T: Sync>(_: &T) {}
//! fn f(cx: ui4me_engine_types::owner::OwnerCx<'_>) { need_sync(&cx) }
//! ```

use std::marker::PhantomData;

#[derive(Clone, Copy, Debug)]
pub struct OwnerCx<'a> {
    _not_send_sync: PhantomData<(&'a (), *const ())>,
}

impl<'a> OwnerCx<'a> {
    /// # Safety
    /// The caller (the HostRuntime, 004) must be executing on the runtime's owner thread,
    /// and the proof must not outlive the owner-thread context it was created in (`'a`).
    pub unsafe fn assume_owner_thread() -> Self {
        OwnerCx {
            _not_send_sync: PhantomData,
        }
    }
}

const _: () = assert!(std::mem::size_of::<OwnerCx<'static>>() == 0);
