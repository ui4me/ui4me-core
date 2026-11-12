//! owner: 001-vertical-slice — created by 001 slice (R-28)
//!
//! Shared identities, geometry, status, diagnostics record types (no upstream types).
//! Module files are created empty by 001 T020 at the owning feature's integrated path
//! (IF-46, R-52), so later parallel tasks never edit this file.

pub mod clipboard;
pub mod codes_generated;
pub mod control_inner;
pub mod css_profile;
pub mod diag;
pub mod geometry;
pub mod ids;
pub mod ime;
pub mod input;
pub mod owner;
pub mod resource_ids;
pub mod revision;
pub mod stage;
pub mod status;
pub mod text_metrics;
