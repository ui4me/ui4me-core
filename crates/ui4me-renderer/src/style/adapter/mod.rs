//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! The Stylo adapter on DocumentCore's arena (T041; AD-07 as amended by AD-P-001-y(a)).
//!
//! **Sealed surface** (X-STYLE-ADAPTER review F1): outside code can neither mint a `StyleNode`
//! nor reach `ElementData`. These must not compile:
//!
//! ```compile_fail,E0616
//! // the sidecar store is private to StyleBridge
//! fn f(b: &crate::style::traverse::StyleBridge) { let _ = &b.sidecars; }
//! ```
//!
//! ```compile_fail,E0603
//! // style passes (and thus handle minting) are crate-internal
//! use crate::style::adapter::handle::with_pass;
//! ```
//!
//! ```compile_fail,E0624
//! // ElementData accessors are crate-internal
//! fn f(s: &crate::style::sidecar::StyleSidecar) { let _ = s.mutate_data(); }
//! ```
//!
//! ```compile_fail,E0624
//! // store lookups are crate-internal
//! fn f(s: &crate::style::sidecar::SidecarStore, id: ui4me_engine_types::ids::NodeId) { let _ = s.get(id); }
//! ```
//!
//! ```compile_fail,E0624
//! // handles cannot be re-targeted from outside
//! fn f(h: crate::style::adapter::handle::StyleNode<'_>, id: ui4me_engine_types::ids::NodeId) { let _ = h.with(id); }
//! ```
//!
//! **Unsafe inventory** (kept minimal, each with a `SAFETY` comment): two `unsafe {}` blocks in
//! handle.rs (the pass-scoped `view`/`sidecars` derefs), two in traverse.rs (`ensure_data` and
//! `unset_dirty_descendants` in `process_preorder`), and the five trait-mandated `unsafe fn`
//! impls in element.rs, whose bodies are safe. No `unsafe impl`, `UnsafeCell` or `transmute`.

pub mod document;
pub mod element;
pub(crate) mod form_state;
pub mod handle;
pub mod node;
pub mod state;
