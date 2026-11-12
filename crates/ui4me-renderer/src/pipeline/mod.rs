//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Pipeline state machine, revisions, publication (U4C-INV).
//! Module files are created empty by 001 T020 at the owning feature's integrated path
//! (IF-46, R-52), so later parallel tasks never edit this file.

pub mod clock;
pub mod cold;
pub mod dirty;
pub mod domain;
#[cfg(feature = "fault-injection")]
pub mod fault;
pub mod feedback;
pub mod font_faces;
pub mod frame;
pub mod inline_svg;
pub mod lifecycle;
pub mod oracle;
pub mod paged;
mod paint_extras;
pub mod publish;
pub mod read;
pub mod reasons;
pub mod resources;
pub mod scroll;
pub mod state;
pub mod turn;
pub mod update;
pub mod views;
