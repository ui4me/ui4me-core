//! owner: 012-interaction — created by 001 slice (R-28)
//!
//! AD-27 dispatch core, focus, activation, input queue (U4C-INT).
//! Module files are created empty by 001 T020 at the owning feature's integrated path
//! (IF-46, R-52), so later parallel tasks never edit this file.

pub mod activation;
pub mod assistive;
pub mod controls;
pub mod dialog;
pub mod dispatch;
pub mod editing;
pub mod element_events;
pub mod focus;
pub mod frame;
pub mod inject;
pub mod input_queue;
pub mod keyboard;
pub mod listeners;
pub mod pointer;
pub mod popover;
pub mod scroll;
pub mod sequences;
