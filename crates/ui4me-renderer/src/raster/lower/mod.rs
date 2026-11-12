//! owner: 010-paint-render — created by 010 T001
//!
//! Lowering entry points. Backend types live only in `vello_cpu.rs` (AD-33).

mod vello_cpu;

pub use crate::raster::validate::{MAX_SURFACE_SIDE, prevalidate};
pub(crate) use vello_cpu::VelloRaster;
