//! Plain-text `contenteditable` editing hosts (0.8 F0801).
//!
//! An element with `contenteditable` (`""`, `true` or `plaintext-only`; `false` turns it off) is an
//! editing host for **plain text**: it is focusable, laid out `white-space: pre-wrap`, and its
//! content is one text node — the first edit flattens any markup inside it to its text, and
//! pasted or typed content is inserted as text, never parsed as HTML. The caret and selection
//! are the document selection (click, drag, Shift+arrows, double click), so copy, highlight and
//! `selectionchange` work as everywhere; a collapsed selection is drawn as a caret while the
//! host has the focus. Editing keys (characters, Enter, Backspace/Delete with Ctrl for words,
//! arrows, Home/End, Up/Down, Ctrl+A/C/X/V/Z/Y) act after an uncanceled `keydown`; each change
//! fires a cancelable `beforeinput` and an `input` (`input_type`, `data`) at the host; IME
//! composition shows the preedit in the text and commits it once. Undo/redo keeps 100 steps.
//! Hosts must be block or inline-block boxes (their text is one paragraph).
//!
//! **Rich hosts (F1003).** `contenteditable` (`""` or `true`) is a *rich* host; only
//! `plaintext-only` keeps the behaviour above. A rich host's content is a bounded HTML subset
//! (see `rich_edit.rs`): paragraphs (`p`/`div`/headings/`li`), inline `b`, `i`, `u`, `s`, `code`,
//! `a href` (kept, never activated while editing) and `br`; other content is preserved. The
//! selection is still the document selection: its points map to **flat byte offsets** (blocks'
//! texts joined by `\n`), so the key handling (arrows, words, Home/End, Up/Down, Backspace,
//! Delete) is shared with plain hosts and only the *edit* differs: the host is read into a
//! `Rich` tree, edited, serialized and written back with `innerHTML` (element identities inside
//! the host are not kept). Enter splits the paragraph, Backspace/Delete across a paragraph
//! boundary merge, Shift+Enter inserts `<br>`, Ctrl+B/I/U toggle formats over a range or set a
//! *pending* format for the next typed text at a caret. Paste inserts the sanitized clipboard
//! HTML reduced to the subset (unknown elements keep their text, only http/https/mailto links),
//! else text; copy/cut write text and HTML. Undo/redo restore serialized snapshots with the
//! selection (100 steps, 16 MiB). `exec_edit` / `get_edit_state` are the programmatic commands.
//! `beforeinput`/`input` carry `formatBold`, `formatItalic`, `formatUnderline`,
//! `formatStrikeThrough`, `formatCode`, `insertLink`, `insertText`, `insertParagraph`,
//! `insertLineBreak`, `deleteContent*`, `insertFromPaste`, `deleteByCut`, `historyUndo/Redo`.
//! Editing never creates `style`, event-handler or script markup. Limitations: a trailing
//! `<br>` at the end of a paragraph does not show an extra empty line; lists have no
//! Enter-on-empty-item exit; formats nest in the order they were applied.

use super::rich_edit::{EditState, Fmt, Rich, safe_href};
mod rich;
use super::*;
use crate::interaction::frame::{EventInit, EventInterface};

const MAX_UNDO: usize = 100;
const MAX_RICH_UNDO_BYTES: usize = 16 * 1024 * 1024;
const MAX_HOST_TEXT: usize = 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct EditableState {
    undo: Vec<(NodeId, String, usize)>,
    redo: Vec<(NodeId, String, usize)>,
    /// The IME composition in the focused host: (host, start, end) bytes of the preedit text.
    comp: Option<(NodeId, usize, usize)>,
    /// x kept across consecutive Up/Down moves.
    goal_x: Option<f64>,
    /// Rich hosts: serialized snapshots with the selection (flat offsets).
    rich_undo: Vec<RichSnap>,
    rich_redo: Vec<RichSnap>,
    /// A format chosen at a caret for the next typed text: (host, caret, formats).
    pending: Option<PendingFormat>,
}

type PendingFormat = (NodeId, usize, Vec<(Fmt, bool)>);

#[derive(Clone, Debug)]
struct RichSnap {
    host: NodeId,
    html: String,
    sel: (usize, usize),
}

fn snap_bytes(v: &[RichSnap]) -> usize {
    v.iter().map(|s| s.html.len()).sum()
}

fn editable_value(v: Option<&str>) -> Option<bool> {
    v.map(|v| !v.trim().eq_ignore_ascii_case("false"))
}

/// Whether `n` is editable: its own `contenteditable` decides, else its parent's (HTML inheritance).
fn editable(d: &Model, n: NodeId) -> bool {
    for a in d.arena().inclusive_ancestors(n) {
        if let Some(v) = d
            .arena()
            .get(a)
            .and_then(|x| x.as_element())
            .and_then(|e| editable_value(e.attr("contenteditable")))
        {
            return v;
        }
    }
    false
}

/// The editing host of `n`: the topmost editable inclusive ancestor whose parent is not editable.
pub(crate) fn editing_host(d: &Model, n: NodeId) -> Option<NodeId> {
    if !editable(d, n) {
        return None;
    }
    let mut host = n;
    for a in d.arena().inclusive_ancestors(n).into_iter().skip(1) {
        if d.arena().get(a).and_then(|x| x.as_element()).is_none() || !editable(d, a) {
            break;
        }
        host = a;
    }
    // A text node's host is its element.
    d.arena()
        .get(host)
        .and_then(|x| x.as_element())
        .map(|_| host)
}

/// Whether `n` itself is an editing host (focusable, `pre-wrap`).
pub(crate) fn is_editing_host(d: &Model, n: NodeId) -> bool {
    editing_host(d, n) == Some(n)
}

fn grapheme_left(t: &str, at: usize) -> usize {
    let mut b = at.min(t.len());
    let mut prev = None;
    for (i, c) in t[..b].char_indices().rev() {
        let mark = matches!(c, '\u{300}'..='\u{36F}' | '\u{200D}' | '\u{FE0E}' | '\u{FE0F}' | '\u{1F3FB}'..='\u{1F3FF}')
            || unicode_mark(c);
        if mark || prev == Some('\u{200D}') {
            b = i;
            prev = Some(c);
            continue;
        }
        if c == '\r' && t[i + 1..].starts_with('\n') {
            b = i;
            break;
        }
        b = i;
        break;
    }
    let _ = prev;
    b
}

fn unicode_mark(c: char) -> bool {
    // Combining marks of common scripts (Arabic, Hebrew, Devanagari vowel signs).
    matches!(c, '\u{591}'..='\u{5C7}' | '\u{610}'..='\u{61A}' | '\u{64B}'..='\u{65F}' | '\u{670}' | '\u{900}'..='\u{903}' | '\u{93A}'..='\u{94F}' | '\u{951}'..='\u{957}' | '\u{962}' | '\u{963}')
}

fn grapheme_right(t: &str, at: usize) -> usize {
    let mut b = at.min(t.len());
    let mut chars = t[b..].char_indices();
    let Some((_, first)) = chars.next() else {
        return t.len();
    };
    b += first.len_utf8();
    let mut joined = first == '\u{200D}';
    for (_, c) in t[b..].char_indices() {
        let mark = matches!(c, '\u{300}'..='\u{36F}' | '\u{200D}' | '\u{FE0E}' | '\u{FE0F}' | '\u{1F3FB}'..='\u{1F3FF}')
            || unicode_mark(c);
        if mark || joined {
            joined = c == '\u{200D}';
            b += c.len_utf8();
            continue;
        }
        break;
    }
    b
}

fn word_left(t: &str, at: usize) -> usize {
    let s = &t[..at.min(t.len())];
    let trimmed = s.trim_end_matches(|c: char| !c.is_alphanumeric());
    trimmed
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_alphanumeric())
        .map_or(0, |(i, c)| i + c.len_utf8())
}

fn word_right(t: &str, at: usize) -> usize {
    let start = at.min(t.len());
    let rest = &t[start..];
    let skip = rest.len()
        - rest
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .len();
    let after = &rest[skip..];
    let word = after
        .find(|c: char| !c.is_alphanumeric())
        .unwrap_or(after.len());
    start + skip + word
}

impl Document {
    /// The editing host holding the focus, if any.
    fn focused_host(&self) -> Option<NodeId> {
        let f = self.interaction.focused_element(self.id())?;
        editing_host(self.pipeline.document(), f)
    }
    /// Whether `host` is a rich host (anything but `contenteditable=plaintext-only`).
    pub(super) fn is_rich_host(&self, host: NodeId) -> bool {
        self.pipeline
            .document()
            .arena()
            .get(host)
            .and_then(|x| x.as_element())
            .and_then(|e| e.attr("contenteditable"))
            .is_some_and(|v| {
                let v = v.trim();
                v.is_empty() || v.eq_ignore_ascii_case("true")
            })
    }
    pub(super) fn rich_load(&self, host: NodeId) -> Rich {
        Rich::read(self.pipeline.document(), host)
    }
    fn host_value(&self, host: NodeId) -> String {
        if self.is_rich_host(host) {
            return self.rich_load(host).text();
        }
        self.pipeline.text_content(host).unwrap_or_default()
    }
    /// The selection in `host` (anchor, focus), defaulting to a caret at the end.
    fn host_range(&self, host: NodeId) -> (usize, usize) {
        let len = self.host_value(host).len();
        self.host_selection(host)
            .map_or((len, len), |(a, f)| (a.min(len), f.min(len)))
    }
    fn fire_input(
        &mut self,
        host: NodeId,
        ty: &str,
        input_type: &str,
        data: Option<&str>,
        cancelable: bool,
        composing: bool,
    ) -> bool {
        let mut e = EventInit::new(ty)
            .interface(EventInterface::InputEvent)
            .bubbles(true)
            .cancelable(cancelable);
        e.data.input_type = Some(input_type.to_owned());
        e.data.data = data.map(str::to_owned);
        e.data.is_composing = Some(composing);
        let id = self.id();
        crate::interaction::element_events::dispatch_element_task(self, id, host, e)
            .map(|o| o.canceled)
            .unwrap_or(true)
    }
    /// Replace bytes `start..end` of the host's text with `text` (a user edit): `beforeinput`
    /// (cancelable unless composing), the change, the caret after it, `input`.
    fn host_edit(
        &mut self,
        host: NodeId,
        start: usize,
        end: usize,
        text: &str,
        input_type: &str,
        record: bool,
    ) -> Result<bool> {
        if self.is_rich_host(host) {
            let text = text.to_owned();
            return self.rich_edit(
                host,
                start,
                end,
                input_type,
                Some(&text),
                record,
                |r, s, e, p| {
                    r.delete_range(s, e);
                    let caret = if text.is_empty() {
                        s
                    } else {
                        r.insert_text(s, &text, p)
                    };
                    Some((caret, caret))
                },
            );
        }
        let value = self.host_value(host);
        let (start, end) = (start.min(value.len()), end.min(value.len()));
        let (start, end) = (start.min(end), start.max(end));
        if value.len() - (end - start) + text.len() > MAX_HOST_TEXT {
            return Ok(false);
        }
        let composing = self.editable.comp.is_some();
        if self.fire_input(
            host,
            "beforeinput",
            input_type,
            Some(text),
            !composing,
            composing,
        ) && !composing
        {
            return Ok(false);
        }
        let d = self.pipeline.document();
        if !d.arena().contains(host) || !d.is_connected(host).unwrap_or(false) {
            return Ok(false);
        }
        let mut next = String::with_capacity(value.len() + text.len());
        next.push_str(&value[..start]);
        next.push_str(text);
        next.push_str(&value[end..]);
        if record {
            let caret = self.host_range(host).1;
            self.editable.undo.push((host, value.clone(), caret));
            if self.editable.undo.len() > MAX_UNDO {
                self.editable.undo.remove(0);
            }
            self.editable.redo.clear();
        }
        self.pipeline.set_text_content(host, &next)?;
        self.after_mutation(true);
        let caret = start + text.len();
        self.set_host_selection(host, caret, caret);
        self.fire_input(host, "input", input_type, Some(text), false, composing);
        Ok(true)
    }
    /// Editing behaviour of an input that reached a focused editing host (after its dispatch).
    pub(super) fn editable_input(&mut self, input: &Input, events: &[Value]) -> Result<bool> {
        let handled = self.editable_input_inner(input, events)?;
        if handled && let Some(host) = self.focused_host() {
            self.reveal_caret(host)?;
        }
        Ok(handled)
    }
    fn editable_input_inner(&mut self, input: &Input, events: &[Value]) -> Result<bool> {
        let Some(host) = self.focused_host() else {
            self.editable.comp = None;
            return Ok(false);
        };
        let prevented = |t: &str| {
            events
                .iter()
                .any(|e| e["type"] == t && e["default_prevented"] == true)
        };
        match input {
            Input::Text(t) if self.editable.comp.is_none() => {
                let t: String = t.chars().filter(|c| !c.is_control()).collect();
                if !t.is_empty() {
                    let (a, f) = self.host_range(host);
                    self.host_edit(host, a, f, &t, "insertText", true)?;
                }
                Ok(true)
            }
            Input::ImePreedit { text, .. } => {
                let (a, f) = self.host_range(host);
                let (s, e) = match self.editable.comp {
                    Some((h, s, e)) if h == host => (s, e),
                    _ => (a.min(f), a.max(f)),
                };
                self.editable.comp = Some((host, s, e));
                if self.host_edit(host, s, e, text, "insertCompositionText", false)? {
                    self.editable.comp = Some((host, s, s + text.len()));
                    if text.is_empty() {
                        self.editable.comp = None;
                    }
                }
                Ok(true)
            }
            Input::ImeCommit(text) => {
                let (s, e) = match self.editable.comp.take() {
                    Some((h, s, e)) if h == host => (s, e),
                    _ => {
                        let (a, f) = self.host_range(host);
                        (a.min(f), a.max(f))
                    }
                };
                self.host_edit(host, s, e, text, "insertFromComposition", true)?;
                Ok(true)
            }
            Input::Key {
                key,
                pressed: true,
                ctrl,
                shift,
                alt: false,
                meta: false,
                ..
            } if !prevented("keydown") && self.editable.comp.is_none() => {
                self.editable_key(host, key, *ctrl, *shift)
            }
            _ => Ok(false),
        }
    }
    fn editable_key(&mut self, host: NodeId, key: &str, ctrl: bool, shift: bool) -> Result<bool> {
        let value = self.host_value(host);
        let (a, f) = self.host_range(host);
        let (lo, hi) = (a.min(f), a.max(f));
        let vertical = matches!(key, "ArrowUp" | "ArrowDown");
        if !vertical {
            self.editable.goal_x = None;
        }
        let place = |d: &mut Self, to: usize| {
            let anchor = if shift { a } else { to };
            d.set_host_selection(host, anchor, to);
        };
        match (key, ctrl) {
            ("Enter", false) if shift && self.is_rich_host(host) => {
                self.rich_line_break(host, lo, hi)?;
            }
            ("Enter", false) => {
                self.host_edit(host, lo, hi, "\n", "insertParagraph", true)?;
            }
            ("b" | "B" | "i" | "I" | "u" | "U", true) if self.is_rich_host(host) => {
                let f = match key {
                    "b" | "B" => Fmt::Bold,
                    "i" | "I" => Fmt::Italic,
                    _ => Fmt::Underline,
                };
                self.rich_format(host, f)?;
            }
            ("Backspace", _) => {
                let start = if lo != hi {
                    lo
                } else if ctrl {
                    word_left(&value, lo)
                } else {
                    grapheme_left(&value, lo)
                };
                if start != hi {
                    self.host_edit(
                        host,
                        start,
                        hi,
                        "",
                        if ctrl {
                            "deleteWordBackward"
                        } else {
                            "deleteContentBackward"
                        },
                        true,
                    )?;
                }
            }
            ("Delete", _) => {
                let end = if lo != hi {
                    hi
                } else if ctrl {
                    word_right(&value, hi)
                } else {
                    grapheme_right(&value, hi)
                };
                if end != lo {
                    self.host_edit(
                        host,
                        lo,
                        end,
                        "",
                        if ctrl {
                            "deleteWordForward"
                        } else {
                            "deleteContentForward"
                        },
                        true,
                    )?;
                }
            }
            ("ArrowLeft", _) => {
                let to = if !shift && lo != hi {
                    lo
                } else if ctrl {
                    word_left(&value, f)
                } else {
                    grapheme_left(&value, f)
                };
                place(self, to);
            }
            ("ArrowRight", _) => {
                let to = if !shift && lo != hi {
                    hi
                } else if ctrl {
                    word_right(&value, f)
                } else {
                    grapheme_right(&value, f)
                };
                place(self, to);
            }
            ("Home", true) => place(self, 0),
            ("End", true) => place(self, value.len()),
            ("Home" | "End" | "ArrowUp" | "ArrowDown", false) => {
                let lines = self.host_lines(host)?;
                let li = lines.iter().rposition(|l| l.0.start <= f).unwrap_or(0);
                let to = match key {
                    "Home" => lines.get(li).map_or(0, |l| l.0.start),
                    "End" => lines.get(li).map_or(value.len(), |l| {
                        let mut e = l.0.end.min(value.len());
                        if value[..e].ends_with('\n') && e > l.0.start {
                            e -= 1;
                        }
                        e
                    }),
                    _ => {
                        let x = self.editable.goal_x.or_else(|| {
                            lines.get(li).and_then(|l| {
                                l.3.iter().min_by_key(|s| s.0.abs_diff(f)).map(|s| s.1)
                            })
                        });
                        self.editable.goal_x = x;
                        let target = if key == "ArrowUp" {
                            li.checked_sub(1)
                        } else {
                            Some(li + 1).filter(|&n| n < lines.len())
                        };
                        match (target, x) {
                            (Some(t), Some(x)) => lines[t]
                                .3
                                .iter()
                                .min_by(|p, q| (p.1 - x).abs().total_cmp(&(q.1 - x).abs()))
                                .map_or(lines[t].0.start, |s| s.0),
                            (None, _) if key == "ArrowUp" => 0,
                            (None, _) => value.len(),
                            (Some(t), None) => lines[t].0.start,
                        }
                    }
                };
                place(self, to);
            }
            ("a" | "A", true) => self.set_host_selection(host, 0, value.len()),
            ("c" | "C", true) | ("x" | "X", true) => {
                if lo != hi {
                    let lo_t = lo.min(value.len());
                    let hi_t = hi.min(value.len());
                    let written = if self.is_rich_host(host) {
                        let html = self.rich_load(host).slice_html(lo, hi);
                        self.clipboard.write_html(&html, &value[lo_t..hi_t])
                    } else {
                        self.clipboard.write_text(&value[lo_t..hi_t])
                    };
                    written.map_err(|e| Error {
                        code: 6,
                        message: format!("Clipboard write failed: {e}"),
                    })?;
                    if matches!(key, "x" | "X") {
                        self.host_edit(host, lo, hi, "", "deleteByCut", true)?;
                    }
                }
            }
            ("v" | "V", true) => {
                if self.is_rich_host(host)
                    && let Some(html) = self.clipboard_html(false).ok().flatten()
                    && let Ok(paras) = Rich::paste_paragraphs(&html)
                    && paras.iter().any(|p| !p.is_empty())
                {
                    let plain = paras
                        .iter()
                        .map(|p| Rich::runs_text(p))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.rich_edit(
                        host,
                        lo,
                        hi,
                        "insertFromPaste",
                        Some(&plain),
                        true,
                        |r, s, e, _| {
                            r.delete_range(s, e);
                            let caret = r.insert_paragraphs(s, &paras);
                            Some((caret, caret))
                        },
                    )?;
                    return Ok(true);
                }
                // Plain-text hosts never parse clipboard HTML; rich hosts fall back to text.
                if let Ok(Some(t)) = self.clipboard.read_text() {
                    let t = t.replace("\r\n", "\n").replace('\r', "\n");
                    let t: String = t
                        .chars()
                        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                        .collect();
                    self.host_edit(host, lo, hi, &t, "insertFromPaste", true)?;
                }
            }
            ("z" | "Z", true) if !shift => {
                self.host_undo(host, true)?;
            }
            ("y" | "Y", true) | ("z" | "Z", true) => {
                self.host_undo(host, false)?;
            }
            ("Tab", _) => return Ok(false),
            _ => return Ok(false),
        }
        Ok(true)
    }
    fn host_undo(&mut self, host: NodeId, undo: bool) -> Result<bool> {
        if self.is_rich_host(host) {
            return self.rich_undo(host, undo);
        }
        let current = self.host_value(host);
        let now = self.host_range(host).1;
        let (from, to) = if undo {
            (&mut self.editable.undo, &mut self.editable.redo)
        } else {
            (&mut self.editable.redo, &mut self.editable.undo)
        };
        let Some(pos) = from.iter().rposition(|e| e.0 == host) else {
            return Ok(false);
        };
        let (_, text, caret) = from.remove(pos);
        to.push((host, current, now));
        let input_type = if undo { "historyUndo" } else { "historyRedo" };
        if self.fire_input(host, "beforeinput", input_type, None, true, false) {
            return Ok(false);
        }
        self.pipeline.set_text_content(host, &text)?;
        self.after_mutation(true);
        let caret = caret.min(text.len());
        self.set_host_selection(host, caret, caret);
        self.fire_input(host, "input", input_type, None, false, false);
        Ok(true)
    }
    /// A click in a focused editing host places the caret there (a collapsed selection).
    pub(super) fn editable_pointer(&mut self, input: &Input) -> Result<()> {
        let Input::PointerButton {
            pressed: false,
            button: 0,
            ..
        } = input
        else {
            return Ok(());
        };
        let Some(host) = self.focused_host() else {
            return Ok(());
        };
        if self.host_selection(host).is_none() {
            let len = self.host_value(host).len();
            self.set_host_selection(host, len, len);
        }
        Ok(())
    }
    /// The caret at byte `f` of `host`: viewport x, top and height.
    fn caret_rect(&mut self, host: NodeId, f: usize) -> Result<Option<(f64, f64, f64)>> {
        let lines = self.host_lines(host)?;
        let caret = lines.iter().rev().find(|l| l.0.start <= f).and_then(|l| {
            l.3.iter()
                .find(|s| s.0 == f)
                .or_else(|| l.3.last())
                .map(|s| (s.1, l.1, l.2))
        });
        Ok(caret.or_else(|| {
            self.viewport_rect(host)
                .map(|(x, y, _, h)| (x + 2.0, y + 2.0, (h - 4.0).clamp(10.0, 24.0)))
        }))
    }
    /// Scroll the host and its scroll containers, innermost first, so the caret (the selection
    /// focus) is visible (nearest alignment, instantly).
    fn reveal_caret(&mut self, host: NodeId) -> Result<()> {
        let f = self.host_range(host).1;
        self.pipeline.flush()?;
        let chain: Vec<ScrollTarget> = self
            .pipeline
            .scroll_chain(host)
            .into_iter()
            .map(|e| e.target)
            .collect();
        for target in chain {
            self.pipeline.flush()?;
            let Some((x, y, h)) = self.caret_rect(host, f)? else {
                return Ok(());
            };
            let (Some(port), Some(e)) = (
                self.scrollport_rect(target),
                self.pipeline.scroll_entry(target),
            ) else {
                continue;
            };
            let dx = align("nearest", x, 2.0, port.0, port.2);
            let dy = align("nearest", y, h, port.1, port.3);
            if dx != 0.0 || dy != 0.0 {
                let t = match target {
                    ScrollTarget::Viewport => None,
                    ScrollTarget::Node(c) => Some(c),
                };
                let to = (
                    (e.offset.0 + dx).clamp(e.range.min.0, e.range.max.0.max(e.range.min.0)),
                    (e.offset.1 + dy).clamp(e.range.min.1, e.range.max.1.max(e.range.min.1)),
                );
                self.begin_scroll(t, to, false)?;
            }
        }
        Ok(())
    }
    /// Draw the caret of a focused editing host with a collapsed selection.
    pub(super) fn paint_editing_caret(
        &mut self,
        pixels: &mut [u32],
        width: u32,
        height: u32,
        scale: f64,
    ) -> Result<()> {
        let Some(host) = self.focused_host() else {
            return Ok(());
        };
        let (a, f) = self.host_range(host);
        if a != f {
            return Ok(());
        }
        let Some((x, y, h)) = self.caret_rect(host, f)? else {
            return Ok(());
        };
        let x0 = ((x * scale).round().max(0.0) as u32).min(width);
        let x1 = (x0 + (1.5 * scale).ceil().max(1.0) as u32).min(width);
        let y0 = ((y * scale).round().max(0.0) as u32).min(height);
        let y1 = (((y + h) * scale).round().max(0.0) as u32).min(height);
        for row in y0..y1 {
            for px in &mut pixels[(row * width + x0) as usize..(row * width + x1) as usize] {
                *px = 0x00202020;
            }
        }
        Ok(())
    }
}
