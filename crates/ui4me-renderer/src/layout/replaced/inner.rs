//! owner: 008-layout-engine (lane L-EDIT-r4) with 009 text services — the inner editing area of
//! text controls (`input` text-like types and `number`, `textarea`; 012 `contracts/controls.md`
//! sections 4-5; WHATWG HTML section 15.5).
//!
//! A text control stays a replaced leaf with its natural size (`controls.rs`). What it shows is a
//! paragraph of the control's own font laid out in its content box: the value (a `password` value
//! masked with one U+2022 per character), else the placeholder, as processed text that keeps its
//! white space (`white-space: pre` for a single line, `pre-wrap` for a `textarea`; the paragraph
//! is built here, so the `normal`-only inline behaviour is not involved). A single line is never
//! broken and is centred vertically in a taller box; a `textarea` breaks at the content width and
//! starts at the top. Lines are emitted as `Line`/`Text` fragments of the control's own fragment
//! (the scene paints them like any text; the owner is the control element) and the caret geometry
//! of every line is published in [`ControlInner`] (`BoxExtras::control_inner`), in UTF-16 offsets
//! of the *value* (invariant 14), so that 012 and the scene never see the displayed text.
//!
//! The control is a scroll container whose scrollport is its content box (`fragment/builder.rs`):
//! 012 moves the inner scroll offset through the ordinary scroll state to keep the caret visible,
//! and the scene shifts the text by it and clips to the content box.
//!
//! `overflow-wrap: break-word` applies in a `textarea` (UA sheet; L-TXT-r5). Not done, reported:
//! right-to-left and bidi text (the stops are logical, see
//! `InnerLine::bidi`), `text-align` other than `start`, `maxlength` display.

use crate::text::bidi::BaseDirection;
use crate::text::fonts::TextServices;
use crate::text::paragraph::{
    BreakEvent, FreezeParams, LineParams, ParagraphSource, ShapedParagraph, SpanCollapse,
    SpanProps, TextAlignment, TextStyle, WrapOverflow, scratch,
};
use std::sync::Arc;
use ui4me_engine_types::ids::{DocumentId, NodeId};

use super::controls::line_box;
use crate::layout::box_tree::{SelectFacts, SelectRowFact};
use crate::layout::fragment::{ControlInner, InnerLine, SelectGeom, SelectRowGeom};
use crate::layout::ifc::inline::line_box::line_boxes;
use crate::layout::ifc::interface::{FinalExtras, FinalOutcome, owner_tag};

/// The mask character of a `password` value.
pub const PASSWORD_MASK: char = '\u{2022}';

/// What a text control shows, from the DOM at box-tree time.
#[derive(Clone, Debug, PartialEq)]
pub struct InnerSource {
    pub multiline: bool,
    pub password: bool,
    /// The control's value (003's control state), not sanitised.
    pub value: String,
    /// The `placeholder` attribute (empty = none).
    pub placeholder: String,
    /// A `select`: its rows and popup state. The displayed text is the selected option's label
    /// (a drop-down) or the visible rows (a listbox); an open drop-down adds its popup rows
    /// below the control (lane L-CTRL-r10).
    pub select: Option<SelectFacts>,
}

pub use ui4me_engine_types::control_inner::POPUP_MAX_ROWS;
/// Horizontal padding of a popup row, and the indent of an option inside an `optgroup`.
const POPUP_PAD_X: f32 = 6.0;
const GROUP_INDENT: f32 = 12.0;

impl SelectFacts {
    fn dropdown(&self) -> bool {
        self.display_size == 1
    }

    /// The label a drop-down shows.
    fn label(&self) -> &str {
        self.rows
            .iter()
            .find(|r| r.selected && !r.group)
            .map_or("", |r| r.label.as_str())
    }

    /// The rows shown: an open drop-down's popup window or a listbox's visible rows, with the
    /// index of the first one.
    fn window(&self) -> (usize, &[SelectRowFact]) {
        if !self.dropdown() {
            // Multi-row listboxes are real scroll containers: retain all rows so
            // extent, scene translation and hit testing share one pixel offset.
            return (0, &self.rows);
        }
        if !self.open {
            return (0, &[]);
        }
        let n = self.rows.len();
        let first = (self.scroll as usize).min(n.saturating_sub(1));
        (first, &self.rows[first..(first + POPUP_MAX_ROWS).min(n)])
    }
}

/// The text a control paints and the map from its byte offsets back to the value's UTF-16 offsets.
struct Display {
    text: String,
    /// `(byte end in text, UTF-16 end in value)` per displayed character, ascending.
    map: Vec<(usize, u32)>,
    placeholder: bool,
}

impl Display {
    /// UTF-16 offset in the value of display byte offset `byte` (a character boundary).
    fn to_u16(&self, byte: usize) -> u32 {
        if byte == 0 {
            return 0;
        }
        match self.map.binary_search_by(|(b, _)| b.cmp(&byte)) {
            Ok(i) => self.map[i].1,
            Err(i) => self.map.get(i).or(self.map.last()).map_or(0, |m| m.1),
        }
    }
}

impl InnerSource {
    fn display(&self) -> Display {
        if let Some(sel) = &self.select {
            let mut parts: Vec<&str> = Vec::new();
            if sel.dropdown() {
                parts.push(sel.label());
            }
            parts.extend(sel.window().1.iter().map(|r| r.label.as_str()));
            let text = parts.join("\n");
            return Display {
                text,
                map: Vec::new(),
                placeholder: false,
            };
        }
        if self.value.is_empty() && !self.placeholder.is_empty() {
            let mut map = Vec::new();
            let mut text = String::new();
            let mut u = 0u32;
            for c in self.placeholder.chars() {
                // A placeholder keeps its line breaks in a textarea only; a single line shows spaces.
                let c = if !self.multiline && matches!(c, '\n' | '\r') {
                    ' '
                } else {
                    c
                };
                text.push(c);
                u += c.len_utf16() as u32;
                map.push((text.len(), u));
            }
            return Display {
                text,
                map,
                placeholder: true,
            };
        }
        let mut text = String::with_capacity(self.value.len());
        let mut map = Vec::with_capacity(self.value.len());
        let mut u = 0u32;
        for c in self.value.chars() {
            u += c.len_utf16() as u32;
            let shown = if self.password {
                PASSWORD_MASK
            } else if !self.multiline && matches!(c, '\n' | '\r') {
                ' '
            } else {
                c
            };
            text.push(shown);
            map.push((text.len(), u));
        }
        Display {
            text,
            map,
            placeholder: false,
        }
    }
}

/// A control's text shaped once per layout pass (the host tree builds it; breaking happens when
/// the used content-box size is known).
pub(crate) struct InnerShaped {
    pub(crate) src: InnerSource,
    display: Display,
    shaped: Option<Arc<ShapedParagraph>>,
    /// Used `line-height` and baseline of one line of the control's font.
    line_height: f32,
    /// The base direction the text is shaped with.
    base: BaseDirection,
}

impl std::fmt::Debug for InnerShaped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InnerShaped")
            .field("text", &self.display.text)
            .finish()
    }
}

/// The laid-out inner area: the lines to turn into fragments and the published geometry.
pub(crate) struct InnerLayout {
    pub outcome: Option<FinalOutcome>,
    /// `paragraph_index` is filled in by the fragment builder.
    pub control: ControlInner,
}

impl InnerShaped {
    pub(crate) fn new(
        src: &InnerSource,
        node: NodeId,
        doc: DocumentId,
        style: &TextStyle,
        base: BaseDirection,
        text: &mut TextServices,
    ) -> InnerShaped {
        let display = src.display();
        let shaped = (!display.text.is_empty()).then(|| {
            let mut ps = ParagraphSource::new(doc, style.clone());
            ps.root_node = Some(node);
            ps.base_direction = base;
            // `white-space: pre` (single line) / `pre-wrap` (textarea) and the UA sheet's
            // `overflow-wrap: break-word` of a textarea: a long word breaks inside, tabs go to stops.
            let props = SpanProps {
                collapse: SpanCollapse::Preserve,
                nowrap: !src.multiline,
                overflow_wrap: if src.multiline {
                    WrapOverflow::BreakWord
                } else {
                    WrapOverflow::Normal
                },
                ..SpanProps::default()
            };
            ps.push_with(&display.text, style, owner_tag(node), &props);
            if base == BaseDirection::Auto {
                ps.isolate_bidi_paragraphs();
            }
            text.shape(&ps)
        });
        let (line_height, _) = line_box(text, doc, style);
        InnerShaped {
            src: src.clone(),
            display,
            shaped,
            line_height,
            base,
        }
    }

    /// Break and freeze at the used content-box size `(width, height)`.
    pub(crate) fn layout(&self, width: f32, height: f32, outer: [f32; 4]) -> InnerLayout {
        if let Some(sel) = &self.src.select {
            return self.layout_select(sel, width, height, outer);
        }
        let multiline = self.src.multiline;
        let lh = self.line_height;
        // A single line is centred vertically (the baseline of the control assumes the same).
        let y0 = if multiline || height <= lh {
            0.0
        } else {
            (height - lh) / 2.0
        };
        let mut lines: Vec<InnerLine> = Vec::new();
        let mut outcome = None;
        let value_len = self.src.value.encode_utf16().count() as u32;
        let (mut content_w, mut content_h) = (0.0f32, y0 + lh);
        if self.display.placeholder || self.shaped.is_none() {
            // An empty value: one empty line for the caret.
            // The caret of an empty value is at the start edge: the right one in right-to-left text.
            let x = if self.base == BaseDirection::Rtl {
                width.max(0.0)
            } else {
                0.0
            };
            lines.push(InnerLine {
                top: y0,
                height: lh,
                baseline: y0 + lh,
                start: 0,
                end: 0,
                stops: vec![(0, x)],
                bidi: self.base == BaseDirection::Rtl,
                clusters: Vec::new(),
            });
        }
        if let Some(shaped) = &self.shaped {
            let mut sc = scratch(shaped);
            let mut y = y0;
            {
                let mut br = sc.breaker();
                // A right-to-left line is aligned to the right edge of the content box.
                let rtl_start = shaped.source().base_direction != BaseDirection::Ltr;
                let max_advance = if multiline || rtl_start {
                    width.max(0.0)
                } else {
                    f32::INFINITY
                };
                loop {
                    br.set_line(LineParams {
                        start_x: 0.0,
                        max_advance,
                        y,
                        max_height: f32::INFINITY,
                    });
                    match br.next() {
                        Some(BreakEvent::LineDone { metrics, .. }) => {
                            y = metrics.top + metrics.height
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
                br.finish();
            }
            let paragraph = sc.freeze(FreezeParams {
                alignment: TextAlignment::Start,
            });
            let boxes = line_boxes(&paragraph);
            content_h = content_h.max(paragraph.height() + y0);
            if !self.display.placeholder {
                lines.clear();
                for (li, fl) in paragraph.lines().iter().enumerate() {
                    let cs = paragraph.caret_stops(li);
                    let mut stops: Vec<(u32, f32)> = Vec::with_capacity(cs.stops.len());
                    let text = &self.display.text;
                    let ends_with_break = text
                        .get(fl.text_range.clone())
                        .is_some_and(|t| t.ends_with('\n') || t.ends_with('\r'));
                    let n = cs.stops.len();
                    for (k, s) in cs.stops.iter().enumerate() {
                        // The stop behind a line break is the start of the next line.
                        if ends_with_break && k + 1 == n && n > 1 {
                            continue;
                        }
                        let u = self.display.to_u16(s.byte);
                        if stops.last().is_none_or(|l| l.0 != u) {
                            stops.push((u, s.x));
                        }
                    }
                    let start = stops.first().map_or(0, |s| s.0);
                    let end = stops.last().map_or(start, |s| s.0);
                    content_w = content_w.max(stops.iter().map(|s| s.1).fold(0.0, f32::max));
                    let clusters = if cs.bidi {
                        paragraph
                            .cluster_extents(li)
                            .into_iter()
                            .filter(|e| {
                                e.end > e.start
                                    && !text
                                        .get(e.start..e.end)
                                        .is_some_and(|t| t.ends_with(['\n', '\r']))
                            })
                            .map(|e| {
                                (
                                    self.display.to_u16(e.start),
                                    self.display.to_u16(e.end),
                                    e.x0,
                                    e.x1,
                                    e.rtl,
                                )
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    lines.push(InnerLine {
                        top: fl.top,
                        height: fl.height,
                        baseline: fl.baseline,
                        start,
                        end,
                        stops,
                        bidi: cs.bidi,
                        clusters,
                    });
                }
                // Text that ends with a line break has an empty last line the caret can sit on.
                if multiline
                    && self.display.text.ends_with(['\n', '\r'])
                    && lines.last().is_some_and(|l| l.end < value_len)
                {
                    let top = lines.last().map_or(y0, |l| l.top + l.height);
                    lines.push(InnerLine {
                        top,
                        height: lh,
                        baseline: top + lh,
                        start: value_len,
                        end: value_len,
                        stops: vec![(value_len, 0.0)],
                        bidi: false,
                        clusters: Vec::new(),
                    });
                    content_h = content_h.max(top + lh);
                }
            } else {
                // A placeholder never scrolls.
                content_w = 0.0;
                content_h = y0 + lh;
            }
            let first_baseline = paragraph.first_baseline();
            let last_baseline = paragraph.last_baseline();
            outcome = Some(FinalOutcome {
                block_size: paragraph.height(),
                first_baseline,
                last_baseline,
                lines: boxes,
                extras: FinalExtras::default(),
                paragraph,
            });
        }
        let control = ControlInner {
            select: None,
            multiline,
            placeholder_shown: self.display.placeholder,
            password: self.src.password,
            viewport: Default::default(),
            paragraph_index: None,
            lines,
            line_height: lh,
            // Room for a caret at the end of the longest line.
            content_width: content_w + 1.0,
            content_height: content_h,
        };
        InnerLayout { outcome, control }
    }
}

impl InnerShaped {
    /// A `select` (lane L-CTRL-r10): every text line is one row, placed by hand. A drop-down shows
    /// its label centred in the content box and, when open, its popup rows in a panel below the
    /// border box (`row_h` = line + 2 px, 1 px panel border); a listbox shows its visible rows from
    /// the top of the content box (`row_h` = line + 1 px, the natural row of controls.md section 4).
    /// `outer` = border + padding (top, right, bottom, left) turns content-box coordinates into
    /// border-box ones.
    fn layout_select(
        &self,
        sel: &SelectFacts,
        width: f32,
        height: f32,
        outer: [f32; 4],
    ) -> InnerLayout {
        let lh = self.line_height;
        let dropdown = sel.dropdown();
        let (first, window) = sel.window();
        let y0 = if dropdown && height > lh {
            (height - lh) / 2.0
        } else {
            0.0
        };
        let border_w = outer[3] + width + outer[1];
        let border_h = outer[0] + height + outer[2];
        let row_h = if dropdown { lh + 2.0 } else { lh + 1.0 };
        // Content-box position of each text line (x, y) and its row geometry (border-box space).
        let mut places: Vec<(f32, f32)> = Vec::new();
        if dropdown {
            places.push((0.0, y0));
        }
        let panel_top_content = height + outer[2];
        for (k, r) in window.iter().enumerate() {
            let indent = if r.indent { GROUP_INDENT } else { 0.0 };
            if dropdown {
                places.push((
                    -outer[3] + 1.0 + POPUP_PAD_X + indent,
                    panel_top_content + 1.0 + k as f32 * row_h + 1.0,
                ));
            } else {
                places.push((2.0 + indent, k as f32 * row_h));
            }
        }
        let mut outcome = None;
        let mut line_widths: Vec<f32> = Vec::new();
        let mut lines: Vec<InnerLine> = Vec::new();
        let (mut content_w, content_h) = (
            0.0f32,
            if dropdown {
                y0 + lh
            } else {
                window.len() as f32 * row_h
            },
        );
        if let Some(shaped) = &self.shaped {
            let mut sc = scratch(shaped);
            let mut idx = 0usize;
            {
                let mut br = sc.breaker();
                loop {
                    let (x, y) = places.get(idx).copied().unwrap_or((0.0, 0.0));
                    br.set_line(LineParams {
                        start_x: x,
                        max_advance: f32::INFINITY,
                        y,
                        max_height: f32::INFINITY,
                    });
                    match br.next() {
                        Some(BreakEvent::LineDone { .. }) => idx += 1,
                        Some(_) => {}
                        None => break,
                    }
                }
                br.finish();
            }
            let paragraph = sc.freeze(FreezeParams {
                alignment: TextAlignment::Left,
            });
            let boxes = line_boxes(&paragraph);
            line_widths = boxes.iter().map(|b| b.width).collect();
            if let Some(fl) = paragraph.lines().first() {
                let w = boxes.first().map_or(0.0, |b| b.width);
                content_w = if dropdown {
                    w
                } else {
                    window
                        .iter()
                        .enumerate()
                        .map(|(k, row)| {
                            line_widths.get(k).copied().unwrap_or(0.0)
                                + if row.indent { GROUP_INDENT } else { 0.0 }
                        })
                        .fold(0.0f32, f32::max)
                        + 3.0
                };
                lines.push(InnerLine {
                    top: fl.top,
                    height: fl.height,
                    baseline: fl.baseline,
                    start: 0,
                    end: 0,
                    stops: vec![(0, 0.0)],
                    bidi: false,
                    clusters: Vec::new(),
                });
            }
            outcome = Some(FinalOutcome {
                block_size: paragraph.height(),
                first_baseline: paragraph.first_baseline(),
                last_baseline: paragraph.last_baseline(),
                lines: boxes,
                extras: FinalExtras::default(),
                paragraph,
            });
        }
        if lines.is_empty() {
            lines.push(InnerLine {
                top: y0,
                height: lh,
                baseline: y0 + lh,
                start: 0,
                end: 0,
                stops: vec![(0, 0.0)],
                bidi: false,
                clusters: Vec::new(),
            });
        }
        // Row geometry in border-box space.
        let hl = sel.highlight;
        let text_w = |k: usize| {
            line_widths
                .get(k + usize::from(dropdown))
                .copied()
                .unwrap_or(0.0)
        };
        let mut geom = None;
        if dropdown && sel.open && !window.is_empty() {
            let widest = (0..window.len())
                .map(|k| text_w(k) + if window[k].indent { GROUP_INDENT } else { 0.0 })
                .fold(0.0f32, f32::max);
            let panel_w = border_w.max(widest + 2.0 * POPUP_PAD_X + 2.0);
            let panel_h = window.len() as f32 * row_h + 2.0;
            let rows = window
                .iter()
                .enumerate()
                .map(|(k, r)| SelectRowGeom {
                    x: 1.0,
                    y: border_h + 1.0 + k as f32 * row_h,
                    width: panel_w - 2.0,
                    height: row_h,
                    option: r.option_index,
                    selected: r.selected,
                    highlighted: r.option_index.is_some() && r.option_index == hl,
                    disabled: r.disabled,
                })
                .collect();
            geom = Some(Box::new(SelectGeom {
                popup: true,
                panel: [0.0, border_h, panel_w, panel_h],
                rows,
                visible_rows: window.len() as u32,
                total_rows: sel.rows.len() as u32,
            }));
        } else if !dropdown {
            let rows = window
                .iter()
                .enumerate()
                .map(|(k, r)| SelectRowGeom {
                    x: outer[3],
                    y: outer[0] + k as f32 * row_h,
                    width: width.max(0.0),
                    height: row_h,
                    option: r.option_index,
                    selected: r.selected,
                    highlighted: r.option_index.is_some() && r.option_index == hl,
                    disabled: r.disabled,
                })
                .collect();
            geom = Some(Box::new(SelectGeom {
                popup: false,
                panel: [0.0; 4],
                rows,
                visible_rows: sel.display_size.max(1),
                total_rows: sel.rows.len() as u32,
            }));
        }
        let _ = first;
        let control = ControlInner {
            select: geom,
            multiline: false,
            placeholder_shown: false,
            password: false,
            viewport: Default::default(),
            paragraph_index: None,
            lines,
            line_height: lh,
            content_width: content_w + 1.0,
            content_height: content_h,
        };
        InnerLayout { outcome, control }
    }
}
