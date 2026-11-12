//! owner: 009-text-inline — created by 001 slice (R-28)

pub mod environment;

pub use environment::{FontEnvironment, TextServices, system_family_exists};

pub mod faces;
pub mod features;
pub mod generics;
pub mod matching;
