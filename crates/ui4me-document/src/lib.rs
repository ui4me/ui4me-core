//! owner: 003-document-model — created by 001 slice (R-28)
//!
//! DocumentCore: arena DOM, parser sink, mutation journal (U4C-DOC).
//! Module files are created empty by 001 T020 at the owning feature's integrated path
//! (IF-46, R-52), so later parallel tasks never edit this file.

#![forbid(unsafe_code)]

pub mod arena;
pub mod attrs;
pub mod batch;
pub mod chardata;
pub mod clone;
pub mod constraint;
pub mod control_semantics;
pub mod control_state;
pub mod control_values;
pub mod diag;
pub mod document;
pub mod errors;
pub mod forms;
pub mod hooks;
pub mod ids;
pub mod journal;
pub mod limits;
pub mod mutate;
pub mod names;
pub mod node;
pub mod parse;
pub mod query;
pub mod retain;
pub mod select;
pub mod serialize;
pub mod stylesheets;
pub mod tokens;
pub mod tree;
pub mod ui_state;
pub mod view;
