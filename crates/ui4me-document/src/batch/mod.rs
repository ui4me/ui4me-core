//! owner: 003-document-model
//!
//! Atomic batch command buffer (T077-T079; A-62; FR-071-FR-075). `decode` parses and validates the
//! whole buffer first; `exec` applies it through the single-call internals under the arena undo log
//! and the journal staging area (`undo` documents the mechanism). The ABI entry point
//! (`ui4me_batch_apply`) is 004's; it calls [`crate::document::Document::batch_apply`].

pub mod decode;
pub mod exec;
pub mod undo;
