//! owner: 009-text-inline (R-36; IF-46) — created by 001 slice (R-28)
//!
//! Line boxes of a frozen paragraph (CSS 2.1 §9.4.2, §10.8): one per line, spanning the
//! line's available width, with the height that `line-height` gives (strut plus inline boxes)
//! and the baseline position.

use crate::text::paragraph::FrozenParagraph;

use crate::layout::ifc::interface::LineBox;

pub fn line_boxes(p: &FrozenParagraph) -> Vec<LineBox> {
    p.lines()
        .iter()
        .map(|l| LineBox {
            x: l.inline_start,
            y: l.top,
            width: l.inline_size,
            height: l.height,
            baseline: l.baseline,
            cluster_range: l.text_range.clone(),
        })
        .collect()
}
