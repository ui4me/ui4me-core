//! UI4ME's native desktop host and hosted HTML/CSS document boundary. Native validation is tracked separately
//! from protocol and lifecycle verification; see docs/STATUS.md.
pub mod accessibility;
pub mod backend;
pub mod config;
pub mod host;
pub mod icons;
pub mod lifecycle;
pub mod logging;
pub mod platform;
pub mod presentation;
pub mod protocol;
pub mod snapshot;
pub mod transport;
