//! owner: 009-text-inline
//!
//! Text geometry: caret. The stops themselves are produced from the whole-pixel unit table the
//! line breaker uses ([`crate::text::paragraph::FrozenParagraph::caret_stops`]); this module re-exports
//! the types. `position_from_point`, `move_visual` and the bidi cases belong to X-BIDI.
