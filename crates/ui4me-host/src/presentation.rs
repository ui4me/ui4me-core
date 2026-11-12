//! A deliberately small CPU test surface. This is the integration probe, not
//! the future HTML/CSS engine. Editing, focus and accessibility stay in Rust.
use accesskit::{
    Action, Node, NodeId, Rect, Role, TextDirection, TextPosition, TextSelection, TreeId, TreeInfo,
    TreeUpdate,
};
use fontdue::{Font, FontSettings};
use std::ops::Range;
use std::sync::OnceLock;
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const ROOT: NodeId = NodeId(1);
pub const EDITOR: NodeId = NodeId(2);
pub const TEXT_RUN: NodeId = NodeId(20);
pub const BUTTONS: [&str; 4] = ["Copy", "Paste", "Open file", "Quit"];

fn font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::from_bytes(
            include_bytes!("../assets/NotoSans-Regular.ttf") as &[u8],
            FontSettings::default(),
        )
        .expect("bundled font is valid")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeAction {
    Copy,
    Paste,
    OpenFile,
    Quit,
}

#[derive(Debug, Clone)]
pub struct Probe {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
    pub preedit: String,
    pub preedit_cursor: Option<(usize, usize)>,
    /// 0 is the editor, 1..=4 are the buttons.
    pub focus_index: usize,
    pub focused: bool,
    pub status: String,
    pub failure: Option<String>,
    scroll_x: f32,
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            anchor: 0,
            preedit: String::new(),
            preedit_cursor: None,
            focus_index: 0,
            focused: false,
            status: "Ready. This surface tests the native host.".into(),
            failure: None,
            scroll_x: 0.0,
        }
    }
}

impl Probe {
    pub fn set_text(&mut self, text: &str) -> Result<(), &'static str> {
        if text.len() > MAX_TEXT_BYTES {
            return Err("probe text exceeds 64 KiB");
        }
        // The probe intentionally exposes a single-line input.
        self.text = text
            .chars()
            .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
            .collect();
        self.cursor = self.text.len();
        self.anchor = self.cursor;
        self.preedit.clear();
        Ok(())
    }

    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }
    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    pub fn insert(&mut self, text: &str) -> bool {
        let value: String = text.chars().filter(|c| !c.is_control()).collect();
        let selection = self.selection();
        if self.text.len() - selection.len() + value.len() > MAX_TEXT_BYTES {
            return false;
        }
        self.text.replace_range(selection.clone(), &value);
        self.cursor = selection.start + value.len();
        self.anchor = self.cursor;
        self.preedit.clear();
        true
    }

    pub fn move_cursor(&mut self, direction: i32, extend: bool) {
        let selection = self.selection();
        self.cursor = if !extend && !selection.is_empty() {
            if direction < 0 {
                selection.start
            } else {
                selection.end
            }
        } else if direction < 0 {
            self.text[..self.cursor]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(i, _)| i)
        } else {
            self.text[self.cursor..]
                .graphemes(true)
                .next()
                .map_or(self.text.len(), |g| self.cursor + g.len())
        };
        if !extend {
            self.anchor = self.cursor;
        }
    }

    pub fn boundary(&mut self, end: bool, extend: bool) {
        self.cursor = if end { self.text.len() } else { 0 };
        if !extend {
            self.anchor = self.cursor;
        }
    }

    pub fn delete(&mut self, backward: bool) {
        if self.selection().is_empty() {
            let original = self.cursor;
            self.move_cursor(if backward { -1 } else { 1 }, false);
            self.anchor = original;
        }
        self.insert("");
    }

    /// Wayland gives byte counts around the selection. Never split UTF-8 even
    /// when an IME sends a malformed/outdated request.
    pub fn delete_surrounding(&mut self, before: usize, after: usize) -> bool {
        let selected = self.selection();
        let Some(start) = selected.start.checked_sub(before) else {
            return false;
        };
        let Some(end) = selected.end.checked_add(after) else {
            return false;
        };
        if end > self.text.len()
            || !self.text.is_char_boundary(start)
            || !self.text.is_char_boundary(end)
        {
            return false;
        }
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.anchor = start;
        true
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }
    pub fn select_bytes(&mut self, anchor: usize, cursor: usize) -> bool {
        if self.text.is_char_boundary(anchor) && self.text.is_char_boundary(cursor) {
            self.anchor = anchor;
            self.cursor = cursor;
            true
        } else {
            false
        }
    }

    pub fn select_characters(&mut self, anchor: usize, cursor: usize) -> bool {
        let boundaries: Vec<_> = self
            .text
            .char_indices()
            .map(|(i, _)| i)
            .chain([self.text.len()])
            .collect();
        match (boundaries.get(anchor), boundaries.get(cursor)) {
            (Some(&a), Some(&c)) => self.select_bytes(a, c),
            _ => false,
        }
    }

    pub fn tab(&mut self, backwards: bool) {
        self.focus_index = (self.focus_index + if backwards { 4 } else { 1 }) % 5;
        self.preedit.clear();
    }

    pub fn activate(&self) -> Option<ProbeAction> {
        match self.focus_index {
            1 => Some(ProbeAction::Copy),
            2 => Some(ProbeAction::Paste),
            3 => Some(ProbeAction::OpenFile),
            4 => Some(ProbeAction::Quit),
            _ => None,
        }
    }

    pub fn click(
        &mut self,
        x: f64,
        y: f64,
        width: u32,
        scale: f64,
        extend: bool,
    ) -> Option<ProbeAction> {
        let logical_x = (x / scale) as f32;
        let logical_y = (y / scale) as f32;
        if (100.0..158.0).contains(&logical_y) {
            self.focus_index = 0;
            let mut pen = 34.0 - self.scroll_x / scale as f32;
            let mut index = self.text.len();
            for (offset, ch) in self.text.char_indices() {
                let advance = font().metrics(ch, 19.0).advance_width;
                if logical_x < pen + advance / 2.0 {
                    index = offset;
                    break;
                }
                pen += advance;
            }
            self.cursor = index;
            if !extend {
                self.anchor = self.cursor;
            }
            return None;
        }
        let _ = width;
        if (182.0..222.0).contains(&logical_y) && logical_x >= 24.0 {
            let index = ((logical_x - 24.0) / 120.0) as usize;
            if index < 4 && (logical_x - 24.0) % 120.0 <= 108.0 {
                self.focus_index = index + 1;
                return self.activate();
            }
        }
        None
    }

    /// A bounded surrounding-text excerpt. The winit beta currently rejects
    /// exactly MAX_TEXT_BYTES, so deliberately leave one byte of headroom.
    pub fn surrounding(&self) -> (String, usize, usize) {
        let max = winit::window::ImeSurroundingText::MAX_TEXT_BYTES - 1;
        let mut start = self.cursor.saturating_sub(max / 2);
        while !self.text.is_char_boundary(start) {
            start += 1;
        }
        let mut end = (start + max).min(self.text.len());
        while !self.text.is_char_boundary(end) {
            end -= 1;
        }
        if self.cursor > end {
            end = self.cursor;
            start = end.saturating_sub(max);
            while !self.text.is_char_boundary(start) {
                start += 1;
            }
        }
        let anchor = self.anchor.clamp(start, end);
        (
            self.text[start..end].into(),
            self.cursor - start,
            anchor - start,
        )
    }

    fn advance(text: &str, px: f32) -> f32 {
        text.chars()
            .map(|c| font().metrics(c, px).advance_width)
            .sum()
    }

    fn caret_advance(&self, scale: f32) -> f32 {
        let start = if self.preedit.is_empty() {
            self.cursor
        } else {
            self.selection().start
        };
        let mut preedit_cursor = self
            .preedit_cursor
            .map_or(0, |x| x.0)
            .min(self.preedit.len());
        while !self.preedit.is_char_boundary(preedit_cursor) {
            preedit_cursor -= 1;
        }
        Self::advance(&self.text[..start], 19.0 * scale)
            + Self::advance(&self.preedit[..preedit_cursor], 19.0 * scale)
    }

    pub fn caret_rect(&self, scale: f64) -> (f64, f64, f64, f64) {
        let s = scale as f32;
        let x = 34.0 * s + self.caret_advance(s) - self.scroll_x;
        (x as f64, 115.0 * scale, 2.0 * scale, 26.0 * scale)
    }

    pub fn render(&mut self, pixels: &mut [u32], width: u32, height: u32, scale: f64, title: &str) {
        let s = scale.clamp(0.5, 8.0) as f32;
        let caret = self.caret_advance(s);
        let available = (width as f32 - 80.0 * s).max(1.0);
        if caret - self.scroll_x > available {
            self.scroll_x = caret - available;
        }
        if caret - self.scroll_x < 0.0 {
            self.scroll_x = caret;
        }
        // Never scroll further than needed to show the end of the value (including any
        // composition): after the text shrinks, a stale offset would hide its first glyphs.
        let content = Self::advance(&self.text, 19.0 * s) + Self::advance(&self.preedit, 19.0 * s);
        self.scroll_x = self.scroll_x.min((content - available).max(0.0));
        let mut canvas = Canvas {
            pixels,
            width: width as i32,
            height: height as i32,
            scale: s,
        };
        canvas.pixels.fill(0x101723);
        canvas.text(24.0, 39.0, title, 23.0, 0xf1f5f9, None);
        canvas.text(
            24.0,
            72.0,
            "Native host / editable input / OS integration probe",
            13.0,
            0x9eafc5,
            None,
        );
        canvas.rect(
            24.0,
            100.0,
            width as f32 / s - 48.0,
            58.0,
            if self.focused && self.focus_index == 0 {
                0x61c6ff
            } else {
                0x38485e
            },
        );
        canvas.rect(26.0, 102.0, width as f32 / s - 52.0, 54.0, 0x182334);
        let clip = Some((30.0 * s, (width as f32 - 30.0 * s).max(30.0 * s)));
        let selection = self.selection();
        let before = Self::advance(&self.text[..selection.start], 19.0 * s) - self.scroll_x;
        let selected_width = Self::advance(&self.text[selection.clone()], 19.0 * s);
        if !selection.is_empty() && self.preedit.is_empty() {
            let left = (34.0 * s + before).max(30.0 * s);
            let right = (34.0 * s + before + selected_width).min(width as f32 - 30.0 * s);
            canvas.rect(
                left / s,
                113.0,
                ((right - left) / s).max(0.0),
                29.0,
                0x285d91,
            );
        }
        if self.preedit.is_empty() {
            canvas.text(
                34.0 - self.scroll_x / s,
                137.0,
                &self.text,
                19.0,
                0xf1f5f9,
                clip,
            );
        } else {
            canvas.text(
                34.0 - self.scroll_x / s,
                137.0,
                &self.text[..selection.start],
                19.0,
                0xf1f5f9,
                clip,
            );
            let x = 34.0 + before / s;
            let preedit_width = Self::advance(&self.preedit, 19.0 * s) / s;
            canvas.text(x, 137.0, &self.preedit, 19.0, 0x7dd3fc, clip);
            canvas.text(
                x + preedit_width,
                137.0,
                &self.text[selection.end..],
                19.0,
                0xf1f5f9,
                clip,
            );
            canvas.rect(
                x.max(30.0),
                142.0,
                ((x + preedit_width).min(width as f32 / s - 30.0) - x.max(30.0)).max(0.0),
                1.0,
                0x7dd3fc,
            );
        }
        if self.focused
            && self.focus_index == 0
            && (self.preedit.is_empty() || self.preedit_cursor.is_some())
        {
            let (x, y, w, h) = self.caret_rect(scale);
            canvas.rect(
                x as f32 / s,
                y as f32 / s,
                w as f32 / s,
                h as f32 / s,
                0xffffff,
            );
        }
        for (i, label) in BUTTONS.iter().enumerate() {
            let x = 24.0 + i as f32 * 120.0;
            canvas.rect(
                x,
                182.0,
                108.0,
                40.0,
                if self.focus_index == i + 1 && self.focused {
                    0x61c6ff
                } else {
                    0x38485e
                },
            );
            canvas.rect(x + 2.0, 184.0, 104.0, 36.0, 0x223247);
            canvas.text(x + 12.0, 208.0, label, 15.0, 0xe2e8f0, None);
        }
        canvas.text(
            24.0,
            258.0,
            "Tab / Shift+Tab: focus   Ctrl+A/C/V   Ctrl+Q: quit",
            12.0,
            0x9eafc5,
            None,
        );
        canvas.text(24.0, 286.0, &self.status, 13.0, 0xb8c9de, None);
        if let Some(error) = &self.failure {
            canvas.rect(20.0, 310.0, width as f32 / s - 40.0, 105.0, 0x4d202c);
            canvas.text(
                34.0,
                336.0,
                "Application backend stopped",
                18.0,
                0xffc4cf,
                None,
            );
            canvas.text(34.0, 361.0, error, 12.0, 0xffdce2, None);
            canvas.text(
                34.0,
                389.0,
                "This host is still responsive. Quit closes it and cleans up workers.",
                12.0,
                0xffdce2,
                None,
            );
        }
    }

    pub fn tree(&self, title: &str, width: u32, height: u32, scale: f64) -> TreeUpdate {
        let mut root = Node::new(Role::Window);
        root.set_label(title.to_string());
        root.set_bounds(Rect::new(0.0, 0.0, width.into(), height.into()));
        root.set_children([
            EDITOR,
            NodeId(3),
            NodeId(4),
            NodeId(5),
            NodeId(6),
            NodeId(7),
        ]);
        let mut editor = Node::new(Role::TextInput);
        editor.set_label("Editable input");
        editor.set_value(self.text.clone());
        editor.set_bounds(Rect::new(
            24.0 * scale,
            100.0 * scale,
            (f64::from(width) - 24.0 * scale).max(24.0 * scale),
            158.0 * scale,
        ));
        editor.set_children([TEXT_RUN]);
        for action in [Action::Focus, Action::SetValue, Action::SetTextSelection] {
            editor.add_action(action);
        }
        editor.set_text_selection(TextSelection {
            anchor: TextPosition {
                node: TEXT_RUN,
                character_index: self.text[..self.anchor].chars().count(),
            },
            focus: TextPosition {
                node: TEXT_RUN,
                character_index: self.text[..self.cursor].chars().count(),
            },
        });
        let mut run = Node::new(Role::TextRun);
        run.set_value(self.text.clone());
        run.set_text_direction(TextDirection::LeftToRight);
        run.set_character_lengths(
            self.text
                .chars()
                .map(|c| c.len_utf8() as u8)
                .collect::<Vec<_>>(),
        );
        let widths: Vec<f32> = self
            .text
            .chars()
            .map(|c| font().metrics(c, 19.0 * scale as f32).advance_width)
            .collect();
        let mut pen = 0.0;
        let positions: Vec<f32> = widths
            .iter()
            .map(|&w| {
                let x = pen;
                pen += w;
                x
            })
            .collect();
        run.set_character_positions(positions);
        run.set_character_widths(widths);
        run.set_bounds(Rect::new(
            34.0 * scale - f64::from(self.scroll_x),
            113.0 * scale,
            34.0 * scale - f64::from(self.scroll_x) + f64::from(pen),
            143.0 * scale,
        ));
        let mut nodes = vec![(ROOT, root), (EDITOR, editor), (TEXT_RUN, run)];
        for (i, label) in BUTTONS.iter().enumerate() {
            let mut node = Node::new(Role::Button);
            node.set_label(*label);
            node.add_action(Action::Focus);
            node.add_action(Action::Click);
            let x = (24.0 + i as f64 * 120.0) * scale;
            node.set_bounds(Rect::new(
                x,
                182.0 * scale,
                x + 108.0 * scale,
                222.0 * scale,
            ));
            nodes.push((NodeId(3 + i as u64), node));
        }
        let mut status = Node::new(Role::Label);
        status.set_value(self.failure.as_ref().map_or(self.status.clone(), |e| {
            format!("Application backend stopped. {e}")
        }));
        status.set_live(accesskit::Live::Polite);
        nodes.push((NodeId(7), status));
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: NodeId(2 + self.focus_index as u64),
        }
    }
}

struct Canvas<'a> {
    pixels: &'a mut [u32],
    width: i32,
    height: i32,
    scale: f32,
}
impl Canvas<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        let left = (x * self.scale).round() as i32;
        let top = (y * self.scale).round() as i32;
        let right = ((x + w) * self.scale).round() as i32;
        let bottom = ((y + h) * self.scale).round() as i32;
        for row in top.max(0)..bottom.min(self.height) {
            for col in left.max(0)..right.min(self.width) {
                self.pixels[(row * self.width + col) as usize] = color;
            }
        }
    }
    fn text(
        &mut self,
        x: f32,
        baseline: f32,
        text: &str,
        size: f32,
        color: u32,
        clip: Option<(f32, f32)>,
    ) {
        let mut pen = x * self.scale;
        for ch in text.chars() {
            let metrics = font().metrics(ch, size * self.scale);
            if pen > self.width as f32 {
                break;
            }
            if pen + metrics.advance_width >= 0.0 {
                let (_, bitmap) = font().rasterize(ch, size * self.scale);
                let left = pen.round() as i32 + metrics.xmin;
                let top =
                    (baseline * self.scale).round() as i32 - metrics.height as i32 - metrics.ymin;
                for row in 0..metrics.height {
                    let y = top + row as i32;
                    if y < 0 || y >= self.height {
                        continue;
                    }
                    for col in 0..metrics.width {
                        let x = left + col as i32;
                        if x < 0
                            || x >= self.width
                            || clip.is_some_and(|(l, r)| (x as f32) < l || (x as f32) >= r)
                        {
                            continue;
                        }
                        let alpha = u32::from(bitmap[row * metrics.width + col]);
                        if alpha == 0 {
                            continue;
                        }
                        let index = (y * self.width + x) as usize;
                        let old = self.pixels[index];
                        let mut blended = 0;
                        for shift in [0, 8, 16] {
                            let channel = (((color >> shift) & 255) * alpha
                                + ((old >> shift) & 255) * (255 - alpha))
                                / 255;
                            blended |= channel << shift;
                        }
                        self.pixels[index] = blended;
                    }
                }
            }
            pen += metrics.advance_width;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grapheme_editing_preserves_combining_and_emoji_sequences() {
        let mut p = Probe::default();
        p.set_text("a e\u{301}👨‍👩‍👧‍👦").unwrap();
        p.delete(true);
        assert_eq!(p.text, "a e\u{301}");
        p.delete(true);
        assert_eq!(p.text, "a ");
        p.select_all();
        p.insert("東京");
        p.move_cursor(-1, false);
        p.delete(false);
        assert_eq!(p.text, "東");
    }
    #[test]
    fn ime_excerpt_stays_bounded_and_on_utf8_boundaries() {
        let mut p = Probe::default();
        p.set_text(&"東京🙂".repeat(1000)).unwrap();
        p.select_all();
        let (text, cursor, anchor) = p.surrounding();
        assert!(text.len() < 4000);
        assert!(text.is_char_boundary(cursor) && text.is_char_boundary(anchor));
        assert_eq!(cursor, text.len());
        assert!(!p.delete_surrounding(1, 1));
    }
    #[test]
    fn oversized_insert_does_not_partially_modify_selection() {
        let mut p = Probe::default();
        p.set_text("keep").unwrap();
        p.select_all();
        assert!(!p.insert(&"x".repeat(MAX_TEXT_BYTES + 1)));
        assert_eq!(p.text, "keep");
    }
    #[test]
    fn accessibility_selection_matches_unicode_byte_offsets() {
        let mut p = Probe::default();
        p.set_text("aé🙂").unwrap();
        assert!(p.select_characters(1, 3));
        assert_eq!(p.selected_text(), "é🙂");
        let tree = p.tree("Test", 640, 480, 1.5);
        let editor = &tree.nodes.iter().find(|(id, _)| *id == EDITOR).unwrap().1;
        assert_eq!(editor.text_selection().unwrap().focus.character_index, 3);
        assert!(!p.select_bytes(2, 3));
    }
    #[test]
    fn composing_over_a_selection_positions_caret_without_committing_text() {
        let mut p = Probe::default();
        p.set_text("aOLDz").unwrap();
        p.select_bytes(1, 4);
        p.preedit = "é".into();
        p.preedit_cursor = Some((2, 2));
        let mut expected = Probe::default();
        expected.set_text("aéz").unwrap();
        expected.select_bytes(3, 3);
        assert_eq!(p.caret_rect(1.0), expected.caret_rect(1.0));
        let mut pixels = vec![0; 900 * 560];
        p.render(&mut pixels, 900, 560, 1.0, "IME test");
        assert_eq!(p.text, "aOLDz");
        assert_eq!(p.selection(), 1..4);
        p.insert("é");
        assert_eq!(p.text, "aéz");
    }
    /// Replacing a long, horizontally scrolled value with text that fits must scroll back:
    /// the caret stays visible either way, but a stale offset hides the first glyphs.
    #[test]
    fn shrinking_text_returns_the_editor_to_its_left_edge() {
        let mut p = Probe::default();
        let mut pixels = vec![0; 900 * 560];
        p.set_text(&"long text ".repeat(40)).unwrap();
        p.render(&mut pixels, 900, 560, 1.0, "scroll test");
        assert!(p.scroll_x > 0.0, "a long value scrolls to its caret");
        p.select_all();
        p.insert("IME short");
        p.render(&mut pixels, 900, 560, 1.0, "scroll test");
        assert_eq!(p.scroll_x, 0.0, "a value that fits is shown from its start");
        // A long value scrolled to its caret never scrolls past the end of the text.
        p.set_text(&"wide ".repeat(60)).unwrap();
        p.render(&mut pixels, 900, 560, 1.0, "scroll test");
        let end = p.scroll_x;
        p.insert("x");
        p.render(&mut pixels, 900, 560, 1.0, "scroll test");
        assert!(p.scroll_x >= end);
        p.delete(true);
        p.delete(true);
        p.render(&mut pixels, 900, 560, 1.0, "scroll test");
        let total = Probe::advance(&p.text, 19.0);
        assert!(p.scroll_x <= (total - (900.0 - 80.0)).max(0.0) + 0.01);
    }
}
