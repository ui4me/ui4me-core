//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! Inline formatting context (AD-13; 008 `contracts/ifc-interface.md`): one IFC container per
//! inline root (008, `container.rs`, the only Taffy-facing part), the inline behaviour (009,
//! `inline/**`, no Taffy types) and the text services (`ui4me-text`).

pub mod container;
pub mod inline;
pub mod interface;
