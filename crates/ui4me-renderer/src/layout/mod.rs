//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! Box tree, layout coordinator, fragments (U4C-BOX/LAY/FRG).
//! Module files are created empty by 001 T020 at the owning feature's integrated path
//! (IF-46, R-52), so later parallel tasks never edit this file.

pub mod box_tree;
pub mod coordinator;
pub mod fragment;
pub mod fragmentation;
pub mod ifc;
pub mod query;
pub mod replaced;
pub mod table;
pub mod writing_mode;
