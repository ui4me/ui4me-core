//! owner: 009-text-inline (R-36; IF-46) — created by 001 slice (R-28)
//!
//! Inline behaviour (AD-13; 008 `ifc-interface.md` §5.2): box tree → paragraph source
//! (`builder`), the line loop driving `ui4me-text` (`driver`), line boxes (`line_box`) and
//! `text-align` (`align`). No Taffy type is used here.

pub mod align;
pub mod builder;
pub mod control;
pub mod decorations;
pub mod driver;
pub mod edges;
pub mod intrinsic;
pub mod line_box;
pub mod overflow;
pub mod tabs;
