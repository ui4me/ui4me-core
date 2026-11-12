//! owner: 009-text-inline (R-36; IF-46) — created by 001 slice (R-28)
//!
//! `text-align` (CSS Text 3 §6.1; FR-017: left, right, center, start, end, justify). `start`/`end`
//! resolve through the root's `direction`. Values outside the consumed set are treated as the
//! initial value `start` and reported (stage `layout`, FR-022).
//!
//! `text-align-last` (§6.2) aligns the last line and the lines that end in a forced break: `auto`
//! is the `text-align` value, except that `justify` becomes `start`.

use crate::text::paragraph::TextAlignment;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;

use crate::layout::box_tree::{Direction, TextAlignValue};
use crate::layout::ifc::interface::InlineLayoutHost;

pub fn resolve(
    value: TextAlignValue,
    direction: Direction,
    root: NodeId,
    host: &mut dyn InlineLayoutHost,
) -> TextAlignment {
    resolve_for(value, direction, false, root, host)
}

/// [`resolve`]; with `per_line`, `start` and `end` stay relative (each line of the paragraph has
/// its own direction, `ParagraphSource::isolated_paragraphs`) and freezing resolves them.
pub fn resolve_for(
    value: TextAlignValue,
    direction: Direction,
    per_line: bool,
    root: NodeId,
    host: &mut dyn InlineLayoutHost,
) -> TextAlignment {
    let start = match (direction, per_line) {
        (_, true) => TextAlignment::Start,
        (Direction::Ltr, _) => TextAlignment::Left,
        (Direction::Rtl, _) => TextAlignment::Right,
    };
    let end = match (direction, per_line) {
        (_, true) => TextAlignment::End,
        (Direction::Ltr, _) => TextAlignment::Right,
        (Direction::Rtl, _) => TextAlignment::Left,
    };
    match value {
        TextAlignValue::Left => TextAlignment::Left,
        TextAlignValue::Right => TextAlignment::Right,
        TextAlignValue::Center => TextAlignment::Center,
        TextAlignValue::Start => start,
        TextAlignValue::End => end,
        TextAlignValue::Justify => TextAlignment::Justify,
        TextAlignValue::Unsupported(f) => {
            host.report(f, Stage::Layout, root);
            start
        }
    }
}

/// The alignment of the last line and of the lines before a forced break.
pub fn resolve_last(
    last: Option<TextAlignValue>,
    main: TextAlignment,
    direction: Direction,
    per_line: bool,
    root: NodeId,
    host: &mut dyn InlineLayoutHost,
) -> TextAlignment {
    match last {
        Some(v) => resolve_for(v, direction, per_line, root, host),
        None if main == TextAlignment::Justify => match (direction, per_line) {
            (_, true) => TextAlignment::Start,
            (Direction::Ltr, _) => TextAlignment::Left,
            (Direction::Rtl, _) => TextAlignment::Right,
        },
        None => main,
    }
}
