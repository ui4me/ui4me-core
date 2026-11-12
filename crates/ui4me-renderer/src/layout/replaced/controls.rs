//! owner: 008-layout-engine (L-INL-r2) — control sizing (FR-075; T087; 012 `contracts/controls.md`
//! §4, R-19).
//!
//! Natural content sizes and baselines of the form controls that are replaced-like boxes: text
//! lines (`size` × the average character advance), `textarea` (`cols` × avg by `rows` × the used
//! line height), `checkbox`/`radio` (13×13), `range` (129×16), `progress` (10em×1em), `meter`
//! (5em×1em) and `select` (widest option label, plus 20 px of arrow area for a dropdown). The
//! average advance is the font's `xAvgCharWidth` scaled to the font size, else the advance of
//! "0" (009's metrics service); the line height is the used `line-height` of the control's own
//! computed font (`normal` is the font's ascent + descent + line gap), obtained by breaking one
//! probe line with 009's text services, so the whole-pixel metrics policy applies. Padding and
//! border come from the cascaded style (006's UA sheet); CSS `width` / `height` replace the natural
//! content size. Buttons (`button`, `input type=button|submit|reset|file|image`) are not here:
//! they are `ButtonContent` boxes with real content (`box_tree/html_rendering.rs`).

use crate::text::fonts::TextServices;
use crate::text::paragraph::{BreakEvent, LineParams, ParagraphSource, TextStyle, scratch};
use ui4me_engine_types::ids::DocumentId;
use ui4me_engine_types::text_metrics::FontMetricsSource;

use super::Natural;

/// The kinds of control boxes with a natural size (012 controls.md §4 rows).
#[derive(Clone, Debug, PartialEq)]
pub enum ControlSpec {
    /// text, search, tel, url, email, password: `size` characters wide (default 20).
    TextLine {
        size: u32,
    },
    /// `number`: 20 characters, `size` ignored.
    Number,
    Textarea {
        cols: u32,
        rows: u32,
    },
    Checkbox,
    Radio,
    Range,
    /// `input type=color`: a 50 x 27 px swatch box (border and padding included).
    Color,
    /// `rows = None`: dropdown; `Some(n)`: listbox with `n` visible rows. `native`: the dropdown keeps
    /// its native look (no author border or background), so its inner block has 1px of block padding.
    Select {
        options: Vec<String>,
        rows: Option<u32>,
        native: bool,
    },
    Progress,
    Meter,
}

/// The vertical scrollbar a `textarea` reserves in its natural width.
pub const TEXTAREA_SCROLLBAR_WIDTH: f32 = 15.0;

/// Width of the dropdown arrow area (controls.md §4).
pub const SELECT_ARROW_WIDTH: f32 = 20.0;

/// The height and baseline (from the line top) of one line of `style` text, as 009 breaks it.
pub fn line_box(text: &mut TextServices, doc: DocumentId, style: &TextStyle) -> (f32, f32) {
    let mut src = ParagraphSource::new(doc, style.clone());
    src.push("0", style, 0);
    let shaped = text.shape(&src);
    let mut sc = scratch(&shaped);
    let mut br = sc.breaker();
    br.set_line(LineParams {
        start_x: 0.0,
        max_advance: f32::INFINITY,
        y: 0.0,
        max_height: f32::INFINITY,
    });
    match br.next() {
        Some(BreakEvent::LineDone { metrics, .. }) => {
            (metrics.height, metrics.baseline - metrics.top)
        }
        _ => {
            let m = text
                .metrics_service()
                .query(&style.font, style.font_size, false);
            (m.normal_line_height(), m.ascent)
        }
    }
}

/// Max-content width of `label` set in `style`.
pub fn text_width(text: &mut TextServices, doc: DocumentId, style: &TextStyle, label: &str) -> f32 {
    let mut src = ParagraphSource::new(doc, style.clone());
    src.push(label, style, 0);
    if src.is_empty() {
        return 0.0;
    }
    text.shape(&src).content_widths().max
}

/// `field-sizing: content` (CSS UI 4; 0.9 F0903): a text control's natural content box fits
/// its value (else its placeholder) plus the caret, one line for an `input`, every line for a
/// `textarea` (at least one); the `size`/`cols`/`rows` attributes no longer apply.
pub fn field_content_natural(
    multiline: bool,
    password: bool,
    value: &str,
    placeholder: &str,
    text: &mut TextServices,
    doc: DocumentId,
    style: &TextStyle,
) -> Natural {
    let shown = if value.is_empty() { placeholder } else { value };
    let masked;
    let shown = if password && !value.is_empty() {
        masked = "\u{2022}".repeat(value.chars().count());
        masked.as_str()
    } else {
        shown
    };
    let (lh, base) = line_box(text, doc, style);
    let lines: Vec<&str> = if multiline {
        shown.split('\n').collect()
    } else {
        vec![shown]
    };
    let widest = lines
        .iter()
        .map(|l| text_width(text, doc, style, l))
        .fold(0.0f32, f32::max);
    // The caret's 1 px stays inside the box.
    let width = (widest + 1.0).ceil();
    if multiline {
        Natural {
            width: Some(width),
            height: Some(lines.len().max(1) as f32 * lh),
            baseline: None,
            first_line_at_top: true,
            ..Natural::default()
        }
    } else {
        Natural {
            width: Some(width),
            height: Some(lh),
            baseline: Some(base),
            inner_line: Some(lh),
            ..Natural::default()
        }
    }
}

impl ControlSpec {
    /// Natural content-box geometry of the control (controls.md §4).
    pub fn natural(&self, text: &mut TextServices, doc: DocumentId, style: &TextStyle) -> Natural {
        let em = style.font_size;
        let record = text
            .metrics_service()
            .query(&style.font, style.font_size, false);
        let avg = || record.avg_char_advance;
        // Chromium's `LayoutTextControlSingleLine::PreferredContentLogicalWidth` (calibrated against the
        // Debian 13 baseline, ERRATA E008-8): `size` average advances plus, for a font with a valid
        // `head` box, the widest glyph box (rounded) less one average advance, whole pixels up.
        let text_input_width = |size: f32| {
            let w = size * avg();
            (if record.max_char_width > 0.0 {
                w + record.max_char_width.round() - avg()
            } else {
                w
            })
            .ceil()
        };
        match self {
            ControlSpec::TextLine { size } => {
                let w = text_input_width(*size as f32);
                let (lh, base) = line_box(text, doc, style);
                Natural {
                    width: Some(w),
                    height: Some(lh),
                    baseline: Some(base),
                    inner_line: Some(lh),
                    ..Natural::default()
                }
            }
            ControlSpec::Number => ControlSpec::TextLine { size: 20 }.natural(text, doc, style),
            ControlSpec::Textarea { cols, rows } => {
                // The columns, whole pixels up, plus the vertical scrollbar (`overflow: auto`, 15 px).
                let w = (*cols as f32 * avg() + TEXTAREA_SCROLLBAR_WIDTH).ceil();
                let (lh, _) = line_box(text, doc, style);
                Natural {
                    width: Some(w),
                    height: Some(*rows as f32 * lh),
                    // Textareas align their bottom margin edge, not the first text line.
                    baseline: None,
                    first_line_at_top: true,
                    ..Natural::default()
                }
            }
            ControlSpec::Checkbox | ControlSpec::Radio => Natural {
                width: Some(13.0),
                height: Some(13.0),
                ..Natural::default()
            },
            // UA padding 1px 2px and border 1px: 50 x 27 outside.
            ControlSpec::Color => Natural {
                width: Some(44.0),
                height: Some(23.0),
                ..Natural::default()
            },
            ControlSpec::Range => Natural {
                width: Some(129.0),
                height: Some(16.0),
                ..Natural::default()
            },
            ControlSpec::Progress => Natural {
                width: Some(10.0 * em),
                height: Some(em),
                ..Natural::default()
            },
            ControlSpec::Meter => Natural {
                width: Some(5.0 * em),
                height: Some(em),
                ..Natural::default()
            },
            ControlSpec::Select {
                options,
                rows,
                native,
            } => {
                let widest = options
                    .iter()
                    .map(|o| text_width(text, doc, style, o.trim()))
                    .fold(0.0f32, f32::max);
                // Native dropdowns and listbox option rows use normal line-height.
                // Dropdowns with appearance:none retain the computed line-height.
                let mut line_style = style.clone();
                if *native || rows.is_some() {
                    line_style.line_height = crate::text::paragraph::LineHeightSpec::Normal;
                }
                let (lh, base) = line_box(text, doc, &line_style);
                let pad = if *native { 2.0 } else { 0.0 };
                match rows {
                    None => Natural {
                        width: Some(widest + if *native { SELECT_ARROW_WIDTH } else { 0.0 }),
                        // The menulist's inner block has 1px of padding above and below its line while the
                        // control is native (Chromium's `PopupInternalPaddingTop`); appearance:none
                        // removes it even when the author supplies border or background styling.
                        height: Some(lh + pad),
                        baseline: Some(base),
                        inner_line: Some(lh),
                        ..Natural::default()
                    },
                    // Listboxes align their bottom margin edge, not the first option's baseline.
                    // A row is the option's line plus 1px; options have 2px of padding on each side.
                    Some(n) => Natural {
                        width: Some(widest + 4.0),
                        height: Some(*n as f32 * (lh + 1.0)),
                        baseline: None,
                        first_line_at_top: true,
                        ..Natural::default()
                    },
                }
            }
        }
    }
}
