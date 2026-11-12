//! The desktop-host seam. Native transfers are deliberately split into a
//! request and an owner-thread completion; no platform call runs in editing.
use super::*;
use std::collections::BTreeMap;
use ui4me_document::control_state::{Selection, SelectionDirection};

pub(super) const DECODED_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const CACHE_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const SVG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TRANSFERS: usize = 16;
const MAX_TRANSFER_BYTES: usize = 1024 * 1024;

/// A native clipboard operation. The host returns its outcome with
/// [`Document::complete_clipboard`] and must also retain the document identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardRequest {
    Read { id: u64 },
    Write { id: u64, text: String },
}
impl ClipboardRequest {
    fn id(&self) -> u64 {
        match self {
            Self::Read { id } | Self::Write { id, .. } => *id,
        }
    }
}

/// UTF-8 text around the focused caret, excluding preedit. Cursor and anchor
/// are byte offsets at character boundaries. Password controls expose empty
/// text with both offsets zero. The excerpt is less than 4000 bytes long.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeSurrounding {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
}

#[derive(Clone)]
struct Target {
    node: NodeId,
    focus: Option<NodeId>,
    value: Option<String>,
    selection: Option<Selection>,
    blocked: bool,
}
impl Target {
    fn capture(d: &Document, node: NodeId) -> Self {
        let model = d.pipeline.document();
        let control = model.control_state(node).ok().flatten();
        Self {
            node,
            focus: d.interaction.focused_element(d.id()),
            value: control.and_then(|s| s.value.clone()),
            selection: control.and_then(|s| s.selection),
            blocked: model
                .control_facts(node)
                .is_some_and(|s| s.disabled || s.read_only),
        }
    }
    fn current(&self, d: &Document) -> bool {
        if !d.window_focused || !d.pipeline.document().arena().is_connected(self.node) {
            return false;
        }
        let now = Self::capture(d, self.node);
        now.focus == self.focus
            && now.value == self.value
            && now.selection == self.selection
            && now.blocked == self.blocked
    }
}

enum Pending {
    Paste {
        target: Target,
        modifiers: Modifiers,
    },
    Write {
        cut: Option<Target>,
        modifiers: Modifiers,
    },
}
impl Pending {
    fn edits_document(&self) -> bool {
        matches!(self, Self::Paste { .. } | Self::Write { cut: Some(_), .. })
    }
}

#[derive(Default)]
pub(super) struct DeferredClipboard {
    enabled: bool,
    next_id: u64,
    pending: BTreeMap<u64, Pending>,
    outgoing: VecDeque<ClipboardRequest>,
    pub(super) completion: Option<std::result::Result<Option<String>, String>>,
}
impl DeferredClipboard {
    pub(super) fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub(super) fn request_serial(&self) -> u64 {
        self.next_id
    }
}

pub(super) fn changes_clipboard_context(input: &Input, buttons: u16) -> bool {
    match input {
        Input::Key { pressed: true, .. }
        | Input::PointerButton { pressed: true, .. }
        | Input::Text(_)
        | Input::Focus(false)
        | Input::ImePreedit { .. }
        | Input::ImeCommit(_)
        | Input::ImeDisabled => true,
        Input::PointerMoved { .. } => buttons != 0,
        _ => false,
    }
}

impl Document {
    pub(super) fn refresh_ime_eligibility(&mut self) {
        let model = self.pipeline.document();
        let password = self
            .interaction
            .focused_element(self.id())
            .and_then(|node| {
                let kind = crate::interaction::editing::text_kind(model, node)?;
                let facts = model.control_facts(node)?;
                (!facts.disabled && !facts.read_only && !model.is_inert(node))
                    .then_some(kind.password)
            });
        match password {
            Some(password) => {
                self.ime
                    .get_or_insert(ImeState {
                        password,
                        cursor_area: (0.0, 0.0, 0.0, 0.0),
                    })
                    .password = password;
            }
            None => self.ime = None,
        }
    }

    /// Use the host's asynchronous plain-text clipboard transport. Calling
    /// this does not read or write any clipboard. Headless documents retain
    /// their deterministic in-process clipboard until explicitly enabled.
    pub fn enable_deferred_clipboard(&mut self) {
        self.native_clipboard.enabled = true;
    }

    /// Take pending transport work once. Transfers are bounded per document;
    /// the host must complete successful and failed requests alike.
    pub fn take_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        self.native_clipboard.outgoing.drain(..).collect()
    }

    fn clipboard_failure(&mut self, code: Code, message: impl Into<String>) {
        let doc = self.id();
        self.record_error(
            doc,
            Status::new(code, Stage::Platform).with_message(message),
        );
    }

    fn clipboard_id(&mut self) -> Option<u64> {
        if self.native_clipboard.pending.len() >= MAX_TRANSFERS {
            self.clipboard_failure(Code::SizeLimit, "Too many pending clipboard transfers");
            return None;
        }
        let Some(id) = self.native_clipboard.next_id.checked_add(1) else {
            self.clipboard_failure(Code::SizeLimit, "Clipboard request identities exhausted");
            return None;
        };
        self.native_clipboard.next_id = id;
        Some(id)
    }

    pub(super) fn invalidate_clipboard_context(&mut self) {
        // A caret can move away and back while an OS read is in flight. Merely
        // comparing the final value/selection would miss that change of intent.
        self.native_clipboard
            .pending
            .retain(|_, p| !p.edits_document());
        self.native_clipboard
            .outgoing
            .retain(|r| self.native_clipboard.pending.contains_key(&r.id()));
    }

    pub(super) fn defer_native_paste(&mut self, doc: DocumentId, node: NodeId) -> bool {
        if !self.native_clipboard.enabled {
            return false;
        }
        if doc != self.id() {
            self.clipboard_failure(
                Code::WrongDocument,
                "Clipboard target belongs to another document",
            );
            return true;
        }
        let Some(id) = self.clipboard_id() else {
            return true;
        };
        let target = Target::capture(self, node);
        self.native_clipboard.pending.insert(
            id,
            Pending::Paste {
                target,
                modifiers: self.modifiers,
            },
        );
        self.native_clipboard
            .outgoing
            .push_back(ClipboardRequest::Read { id });
        true
    }

    pub(super) fn defer_native_write(
        &mut self,
        doc: DocumentId,
        node: Option<NodeId>,
        data: &ClipboardData,
        cut: bool,
    ) -> bool {
        if !self.native_clipboard.enabled {
            return false;
        }
        if doc != self.id() {
            self.clipboard_failure(
                Code::WrongDocument,
                "Clipboard target belongs to another document",
            );
            return true;
        }
        let text = data.plain.as_deref().unwrap_or("");
        if text.len() > MAX_TRANSFER_BYTES || text.contains('\0') {
            self.clipboard_failure(
                Code::SizeLimit,
                "Native clipboard text exceeds 1 MiB or contains NUL",
            );
            return true;
        }
        let Some(id) = self.clipboard_id() else {
            return true;
        };
        let cut = if cut {
            node.map(|n| Target::capture(self, n))
        } else {
            None
        };
        self.native_clipboard.pending.insert(
            id,
            Pending::Write {
                cut,
                modifiers: self.modifiers,
            },
        );
        self.native_clipboard
            .outgoing
            .push_back(ClipboardRequest::Write {
                id,
                text: text.to_owned(),
            });
        true
    }

    /// Complete one native transfer on the owner thread. For a write, use
    /// `Ok(None)` only after the OS accepted it. Failed writes never cut text.
    /// Stale/cancelled results are harmless. The returned events are drained
    /// exactly once, as with `handle_input`.
    pub fn complete_clipboard(
        &mut self,
        id: u64,
        result: std::result::Result<Option<String>, String>,
    ) -> Result<Vec<Value>> {
        let Some(pending) = self.native_clipboard.pending.remove(&id) else {
            return Ok(Vec::new());
        };
        self.native_clipboard.outgoing.retain(|r| r.id() != id);
        let text = match result {
            Ok(text) => text,
            Err(message) => {
                self.clipboard_failure(
                    Code::ResourceDenied,
                    format!("Native clipboard transfer failed: {message}"),
                );
                return Ok(self.drain_events());
            }
        };
        let doc = self.id();
        match pending {
            Pending::Paste { target, modifiers } if target.current(self) => {
                if text
                    .as_ref()
                    .is_some_and(|s| s.len() > MAX_TRANSFER_BYTES || s.contains('\0'))
                {
                    self.clipboard_failure(
                        Code::SizeLimit,
                        "Native clipboard text exceeds 1 MiB or contains NUL",
                    );
                    return Ok(self.drain_events());
                }
                self.native_clipboard.completion = Some(Ok(text));
                self.native_clipboard.enabled = false;
                let result = dispatch::operation(self, |h| {
                    crate::interaction::editing::complete_native_paste(
                        h,
                        doc,
                        target.node,
                        modifiers,
                    )
                });
                self.native_clipboard.enabled = true;
                self.native_clipboard.completion = None;
                result?;
            }
            Pending::Write {
                cut: Some(target),
                modifiers,
            } if target.current(self) => {
                dispatch::operation(self, |h| {
                    crate::interaction::editing::complete_native_cut(h, doc, target.node, modifiers)
                })?;
            }
            _ => {}
        }
        Ok(self.drain_events())
    }

    /// The last surrounding-text state for the focused native text control.
    pub fn ime_surrounding(&self) -> Option<ImeSurrounding> {
        self.ime_request()?;
        let node = self.interaction.focused_element(self.id())?;
        let model = self.pipeline.document();
        let kind = crate::interaction::editing::text_kind(model, node)?;
        let facts = model.control_facts(node)?;
        if facts.disabled || facts.read_only {
            return None;
        }
        if kind.password {
            return Some(ImeSurrounding {
                text: String::new(),
                cursor: 0,
                anchor: 0,
            });
        }
        // Read the canonical control state here as well as during IME reports:
        // an application value write may occur between platform IME events.
        let state = model.control_state(node).ok().flatten()?;
        let mut text = model.control_value(node).unwrap_or_default();
        let (cursor, anchor) = if let Some((start, end)) = state.composition {
            let start = byte_offset(&text, start);
            let end = byte_offset(&text, end);
            text.replace_range(start..end, "");
            (start, start)
        } else if let Some(s) = state.selection {
            let (start, end) = (byte_offset(&text, s.start), byte_offset(&text, s.end));
            if s.direction == SelectionDirection::Backward {
                (start, end)
            } else {
                (end, start)
            }
        } else {
            (text.len(), text.len())
        };
        let (text, cursor, anchor) =
            ui4me_engine_types::ime::surrounding_excerpt(&text, cursor, anchor);
        Some(ImeSurrounding {
            text,
            cursor,
            anchor,
        })
    }

    /// Delete UTF-8 byte counts before/after the current selection (or preedit
    /// insertion point). Counts must fit the last bounded surrounding-text
    /// report and may not split a code point. Password text is never disclosed
    /// or deletable through a surrounding-text request.
    pub fn ime_delete_surrounding(&mut self, before: usize, after: usize) -> Result<Vec<Value>> {
        let Some(state) = self.ime_surrounding() else {
            return Ok(Vec::new());
        };
        let lo = state.cursor.min(state.anchor);
        let hi = state.cursor.max(state.anchor);
        let start = lo
            .checked_sub(before)
            .ok_or_else(|| arg("IME deletion exceeds surrounding text"))?;
        let end = hi
            .checked_add(after)
            .filter(|&e| e <= state.text.len())
            .ok_or_else(|| arg("IME deletion exceeds surrounding text"))?;
        if !state.text.is_char_boundary(start) || !state.text.is_char_boundary(end) {
            return Err(arg("IME deletion splits a UTF-8 code point"));
        }
        if self.ime_request().is_some_and(|s| s.password) {
            return if before == 0 && after == 0 {
                Ok(Vec::new())
            } else {
                Err(arg("Password controls expose no surrounding text"))
            };
        }
        let before = state.text[start..lo].encode_utf16().count() as u32;
        let after = state.text[hi..end].encode_utf16().count() as u32;
        self.invalidate_clipboard_context();
        let doc = self.id();
        let modifiers = self.modifiers;
        dispatch::operation(self, |h| {
            crate::interaction::editing::ime::delete_surrounding(
                h,
                doc,
                before,
                after,
                modifiers,
                DispatchOrigin::Native,
                None,
            )
        })?;
        Ok(self.drain_events())
    }
}

fn byte_offset(text: &str, utf16: u32) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= utf16 {
            return byte;
        }
        units += ch.len_utf16() as u32;
    }
    text.len()
}

#[cfg(test)]
#[path = "hosted_tests.rs"]
mod tests;
