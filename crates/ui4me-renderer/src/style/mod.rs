//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! StyleBridge: Stylo embedding (U4C-CSS; AD-07 as amended by AD-P-001-y(a)).
//! Module files are created empty by 001 T020 at the owning feature's integrated path
//! (IF-46, R-52), so later parallel tasks never edit this file.

pub mod adapter;
pub mod animation;
pub mod computed;
pub mod container;
pub mod device;
pub mod diagnostics;
pub(crate) mod dir;
pub(crate) mod first_pseudo;
pub mod font_faces;
pub mod font_metrics;
pub mod has;
pub mod hints;
pub mod page;
pub mod profile;
pub mod query;
pub mod sheets;
pub mod sidecar;
pub mod snapshot;
pub mod traverse;
pub mod ua;
pub mod view;
