//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Diagnostics schema v0, canonical JSON, probes (U4C-DIA).
//! Module files are created empty by 001 T020 at the owning feature's integrated path
//! (IF-46, R-52), so later parallel tasks never edit this file.

pub mod canonical_json;
pub mod envelope;
pub mod keys;
pub mod probes;
pub mod redact;
