//! owner: 009-text-inline — created by 001 slice (R-28)
//!
//! Text shaping, line breaking, font metrics (U4C-TEXT).
//! Module files are created empty by 001 T020 at the owning feature's integrated path
//! (IF-46, R-52), so later parallel tasks never edit this file.

pub mod bidi;
pub mod breaking;
pub mod diagnostics;
pub mod first_letter;
pub mod fonts;
pub mod geometry;
pub mod hyphenation;
pub mod metrics;
pub mod paragraph;
pub mod transform;
