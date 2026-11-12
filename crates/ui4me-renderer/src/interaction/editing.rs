//! owner: 012-interaction (lane L-EDIT-r4; FR-070-FR-082 text editing subset; `contracts/editing.md`)
//!
//! Editing of `input` (text-like types and `number`) and `textarea`: the caret and selection
//! (stored in 003's control state, UTF-16 offsets into the value), the value-changing commands
//! (`beforeinput` -> edit -> `input`), caret and selection commands, pointer placement, drag,
//! double- and triple-click selection, `select` and `change` events, and keeping the caret in view
//! through the control's inner scroll offset. Geometry (which character is where) comes from the
//! committed layout through [`DispatchHost::control_geometry`] ([`ControlInner`]); without it
//! (hosts without a layout) the commands that need positions fall back to the whole value.
//!
//! Clipboard (`copy`/`cut`/`paste`), undo/redo and IME composition are the submodules
//! [`clipboard`], [`history`] and [`ime`] (lane L-CLIP-r13).
//!
//! Grapheme and word editing use Unicode segmentation (UAX #29); visual bidi movement uses
//! the committed text geometry. Selection changes are delivered as asynchronous notifications.

use crate::interaction::dispatch::{DispatchHost, eng, rt, run_dispatch};
use crate::interaction::frame::{EventInit, EventInterface, Modifiers};
use crate::interaction::input_queue::{KeyData, PointerData};
mod clipboard;
mod history;
pub(crate) mod ime;

use std::collections::HashMap;
use ui4me_document::control_state::{Selection, SelectionDirection};
use ui4me_document::document::Document;
use ui4me_engine_types::control_inner::ControlInner;
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;
use unicode_segmentation::UnicodeSegmentation;

/// The inner editing area of a control as the committed layout has it, in viewport space.
#[derive(Clone, Debug, PartialEq)]
pub struct EditGeometry {
    pub inner: ControlInner,
    /// Viewport position of the content box's top-left corner at scroll offset `(0, 0)`.
    pub origin: (f64, f64),
    /// The control's inner scroll offset in force.
    pub scroll: (f64, f64),
}

impl EditGeometry {
    fn viewport_size(&self) -> (f64, f64) {
        (
            self.inner.viewport.size.width.to_f64(),
            self.inner.viewport.size.height.to_f64(),
        )
    }
    /// A viewport point in the (scrolled) text space of the inner area.
    fn to_local(&self, x: f64, y: f64) -> (f32, f32) {
        (
            (x - self.origin.0 + self.scroll.0) as f32,
            (y - self.origin.1 + self.scroll.1) as f32,
        )
    }
    /// The value offset nearest to a viewport point.
    pub fn offset_at_point(&self, x: f64, y: f64) -> u32 {
        let (lx, ly) = self.to_local(x, y);
        let li = if self.inner.multiline {
            self.inner.line_at_y(ly)
        } else {
            0
        };
        self.inner.offset_at(li, lx)
    }
}

// ---------------------------------------------------------------------------------------------
// Per-document editing state

#[derive(Clone, Copy, Debug, PartialEq)]
enum Granularity {
    Char,
    Word { start: u32, end: u32 },
    Paragraph { start: u32, end: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    node: NodeId,
    anchor: u32,
    granularity: Granularity,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ClickTracker {
    node: NodeId,
    x: f64,
    y: f64,
    time: f64,
    count: u8,
}

/// Editing state of one document (owned by the interaction runtime).
#[derive(Debug, Default)]
pub(crate) struct EditState {
    drag: Option<Drag>,
    click: Option<ClickTracker>,
    /// x kept across consecutive vertical caret moves (textarea).
    goal_x: Option<f32>,
    /// The caret sits at the end of a soft-wrapped line (upstream affinity) at this offset of this
    /// control: Home, Up and Down then act on that line, not on the one that starts there.
    upstream_at: Option<(NodeId, u32)>,
    /// The `change` baseline of each text control: its value when focused or last committed.
    baseline: HashMap<NodeId, String>,
    /// Undo and redo stacks per text control (`history.rs`).
    history: HashMap<NodeId, history::History>,
    /// The IME composition in progress (`ime.rs`).
    comp: Option<ime::Comp>,
}

/// Double and triple clicks: within this many ms and px of the previous press.
const MULTI_CLICK_MS: f64 = 500.0;
const MULTI_CLICK_PX: f64 = 4.0;

// ---------------------------------------------------------------------------------------------
// Control facts

/// What kind of text control an element is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextKind {
    pub multiline: bool,
    pub password: bool,
    pub number: bool,
}

/// `Some` for an `input` of a text-like type (Text, Search, Telephone, URL, Email, Password, Number;
/// a missing or invalid `type` is Text) or a `textarea`, whether or not it is editable.
pub fn text_kind(d: &Document, n: NodeId) -> Option<TextKind> {
    use ui4me_document::control_semantics::{ControlKind, InputType};
    let facts = d.control_facts(n)?;
    if !d.arena().is_connected(n) {
        return None;
    }
    match facts.kind {
        ControlKind::Textarea => Some(TextKind {
            multiline: true,
            password: false,
            number: false,
        }),
        ControlKind::Input(kind) if kind.accepts_text() => Some(TextKind {
            multiline: false,
            password: kind == InputType::Password,
            number: kind == InputType::Number,
        }),
        _ => None,
    }
}

/// A value snapshot with its selection.
#[derive(Clone, Debug)]
struct Ctl {
    value: String,
    kind: TextKind,
    /// Selection in UTF-16 offsets, `start <= end`; the caret when equal. No stored selection
    /// means the caret is at the end of the value.
    start: u32,
    end: u32,
    dir: SelectionDirection,
    readonly: bool,
    maxlength: Option<usize>,
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

impl Ctl {
    fn read(d: &Document, n: NodeId) -> Option<Ctl> {
        let kind = text_kind(d, n)?;
        let e = d.arena().get(n)?.as_element()?;
        let cs = d.control_state(n).ok().flatten()?;
        let value = d.control_value(n).unwrap_or_default();
        let len = utf16_len(&value);
        let (start, end, dir) = match cs.selection {
            Some(s) => (
                s.start.min(len),
                s.end.min(len).max(s.start.min(len)),
                s.direction,
            ),
            None => (len, len, SelectionDirection::None),
        };
        let maxlength = e
            .attr("maxlength")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|v| *v >= 0)
            .map(|v| v as usize);
        let facts = d.control_facts(n)?;
        Some(Ctl {
            value,
            kind,
            start,
            end,
            dir,
            readonly: facts.read_only || facts.disabled,
            maxlength,
        })
    }
    fn len(&self) -> u32 {
        utf16_len(&self.value)
    }
    /// (anchor, focus) of the selection.
    fn anchor_focus(&self) -> (u32, u32) {
        match self.dir {
            SelectionDirection::Backward => (self.end, self.start),
            _ => (self.start, self.end),
        }
    }
}

/// The text as characters with their UTF-16 offsets.
struct Chars {
    chars: Vec<char>,
    /// UTF-16 offset of the start of `chars[i]`; one more entry for the end.
    off: Vec<u32>,
    clusters: Vec<usize>,
    words: Vec<(usize, usize)>,
}

impl Chars {
    fn new(s: &str) -> Chars {
        let chars: Vec<char> = s.chars().collect();
        let mut off = Vec::with_capacity(chars.len() + 1);
        let mut u = 0u32;
        for c in &chars {
            off.push(u);
            u += c.len_utf16() as u32;
        }
        off.push(u);
        let mut scalar = 0;
        let mut clusters = vec![0];
        for grapheme in s.graphemes(true) {
            scalar += grapheme.chars().count();
            clusters.push(scalar);
        }
        let mut scalar = 0;
        let mut words: Vec<(usize, usize)> = Vec::new();
        for segment in s.split_word_bounds() {
            let start = scalar;
            scalar += segment.chars().count();
            // Adjacent white-space segments form one selection unit.
            if segment.chars().all(char::is_whitespace)
                && words
                    .last()
                    .is_some_and(|&(a, b)| chars[a..b].iter().all(|c| c.is_whitespace()))
            {
                words.last_mut().expect("checked above").1 = scalar;
            } else {
                words.push((start, scalar));
            }
        }
        Chars {
            chars,
            off,
            clusters,
            words,
        }
    }
    /// Index of the character starting at (or containing) UTF-16 offset `u`.
    fn index(&self, u: u32) -> usize {
        match self.off.binary_search(&u) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }
    fn n(&self) -> usize {
        self.chars.len()
    }
    /// Extended grapheme boundaries from UAX #29, in scalar indices. The same boundary
    /// table drives both directions, so flags, ZWJ sequences and Indic conjuncts cannot be
    /// split by backward movement or Backspace.
    fn cluster_end(&self, i: usize) -> usize {
        let at = self.clusters.partition_point(|&b| b <= i);
        self.clusters.get(at).copied().unwrap_or(self.n())
    }
    fn cluster_start(&self, i: usize) -> usize {
        let at = self.clusters.partition_point(|&b| b < i);
        self.clusters
            .get(at.saturating_sub(1))
            .copied()
            .unwrap_or(0)
    }
    fn segment_at(&self, i: usize) -> Option<(usize, usize)> {
        self.words.iter().copied().find(|&(s, e)| s <= i && i < e)
    }
}

fn word_left(t: &Chars, mut i: usize) -> usize {
    i = i.min(t.n());
    while i > 0 && t.chars[i - 1].is_whitespace() {
        i -= 1;
    }
    t.segment_at(i.saturating_sub(1)).map_or(0, |(s, _)| s)
}

/// Windows moves to the start of the next word; other platforms to the end of
/// the current/next word. Both use Unicode word boundaries, including apostrophes.
fn word_right(t: &Chars, mut i: usize) -> usize {
    i = i.min(t.n());
    if cfg!(windows) {
        i = t.segment_at(i).map_or(t.n(), |(_, e)| e);
        while i < t.n() && t.chars[i].is_whitespace() {
            i += 1;
        }
    } else {
        while i < t.n() && t.chars[i].is_whitespace() {
            i += 1;
        }
        i = t.segment_at(i).map_or(t.n(), |(_, e)| e);
    }
    i
}

/// The Unicode word or punctuation segment at the caret; a caret just after a
/// word selects that preceding word, matching desktop text controls.
fn word_around(t: &Chars, i: usize) -> (usize, usize) {
    if t.n() == 0 {
        return (0, 0);
    }
    let i = i.min(t.n());
    let at = if i < t.n() && !t.chars[i].is_whitespace() {
        i
    } else if i > 0 && !t.chars[i - 1].is_whitespace() {
        i - 1
    } else {
        i.min(t.n() - 1)
    };
    t.segment_at(at).unwrap_or((at, at + 1))
}

/// The hard line (paragraph) around char index `i`.
fn paragraph_around(t: &Chars, i: usize) -> (usize, usize) {
    let mut s = i.min(t.n());
    while s > 0 && t.chars[s - 1] != '\n' {
        s -= 1;
    }
    let mut e = i.min(t.n());
    while e < t.n() && t.chars[e] != '\n' {
        e += 1;
    }
    (s, e)
}

// ---------------------------------------------------------------------------------------------
// Events

fn input_event(
    ty: &str,
    input_type: &str,
    data: Option<&str>,
    cancelable: bool,
    m: Modifiers,
) -> EventInit {
    let mut e = EventInit::new(ty)
        .interface(EventInterface::InputEvent)
        .bubbles(true)
        .cancelable(cancelable);
    e.data.data = data.map(str::to_owned);
    e.data.input_type = Some(input_type.to_owned());
    e.modifiers = m;
    e
}

fn key_event(ty: &str, k: &KeyData, m: Modifiers) -> EventInit {
    let mut e = EventInit::new(ty)
        .interface(EventInterface::KeyboardEvent)
        .bubbles(true)
        .cancelable(true);
    e.data.key = Some(k.key.clone());
    e.data.code = Some(k.code.clone());
    e.modifiers = m;
    e
}

/// Where a command came from, for dispatch.
#[derive(Clone, Copy)]
struct Cx {
    doc: DocumentId,
    node: NodeId,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
}

fn snapshot(host: &mut dyn DispatchHost, cx: &Cx) -> Result<Option<Ctl>, Status> {
    eng(host, cx.doc, |_, d| Ctl::read(d, cx.node))
}

/// Resume a native clipboard request after its host completion. The document
/// validates its captured focus, value and selection before calling this.
pub(crate) fn complete_native_paste(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    m: Modifiers,
) -> Result<(), Status> {
    let cx = Cx {
        doc,
        node,
        m,
        origin: DispatchOrigin::Native,
        ts: None,
    };
    if snapshot(host, &cx)?.is_some() {
        clipboard::paste(host, &cx)
    } else {
        clipboard::document_command(
            host,
            doc,
            node,
            clipboard::Command::Paste,
            m,
            DispatchOrigin::Native,
            None,
        )
    }
}

/// A successful asynchronous cut writes first, then removes the still-current
/// selection through the normal beforeinput/input and undo machinery.
pub(crate) fn complete_native_cut(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    m: Modifiers,
) -> Result<(), Status> {
    let cx = Cx {
        doc,
        node,
        m,
        origin: DispatchOrigin::Native,
        ts: None,
    };
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(());
    };
    if !c.kind.password && c.start != c.end {
        edit(
            host,
            &cx,
            Edit::new(
                "deleteByCut",
                None,
                (c.start, c.end),
                String::new(),
                history::StepKind::Cut,
            ),
        )?;
    }
    Ok(())
}

fn fire(host: &mut dyn DispatchHost, cx: &Cx, init: EventInit) -> Result<bool, Status> {
    run_dispatch(host, cx.doc, cx.node, init, true, cx.origin, cx.ts).map(|o| o.canceled)
}

/// Write the selection (anchor, focus) and fire `select` when a user action made it non-empty.
fn write_selection(
    host: &mut dyn DispatchHost,
    cx: &Cx,
    anchor: u32,
    focus: u32,
    announce: bool,
) -> Result<bool, Status> {
    let (start, end) = (anchor.min(focus), anchor.max(focus));
    let dir = match anchor.cmp(&focus) {
        std::cmp::Ordering::Less => SelectionDirection::Forward,
        std::cmp::Ordering::Greater => SelectionDirection::Backward,
        std::cmp::Ordering::Equal => SelectionDirection::None,
    };
    let changed = eng(host, cx.doc, |_, d| {
        let Some(old) = Ctl::read(d, cx.node) else {
            return false;
        };
        // A clean `textarea` shows its children's text; the stored value (which the selection is
        // validated against) catches up when the selection is first written.
        if d.control_state(cx.node)
            .ok()
            .flatten()
            .and_then(|c| c.value.as_deref())
            != Some(old.value.as_str())
        {
            let _ = d.control_state_writer().set_value(cx.node, &old.value);
        }
        let stored = d
            .control_state(cx.node)
            .ok()
            .flatten()
            .and_then(|c| c.selection);
        if stored.is_some_and(|s| s.start == start && s.end == end && s.direction == dir) {
            return false;
        }
        let ok = d
            .control_state_writer()
            .set_selection(
                cx.node,
                Some(Selection {
                    start,
                    end,
                    direction: dir,
                }),
            )
            .is_ok();
        ok && (old.start, old.end, old.dir) != (start, end, dir)
    })?;
    if changed && announce && start != end {
        fire(host, cx, EventInit::new("select").bubbles(true))?;
    }
    if changed {
        fire(host, cx, EventInit::new("selectionchange").bubbles(true))?;
    }
    Ok(changed)
}

// ---------------------------------------------------------------------------------------------
// Caret visibility and scrolling

/// Show the caret (restarting the blink) and scroll the inner area so that it is visible.
fn reveal_caret(
    host: &mut dyn DispatchHost,
    cx: &Cx,
    offset: u32,
    upstream: bool,
) -> Result<(), Status> {
    // Programmatic editing can address an unfocused control. Its value/selection changes must
    // not move the visible caret or replace the currently focused control's native IME state.
    if rt(host, |ix| ix.focused_element(cx.doc)) != Some(cx.node) {
        return Ok(());
    }
    host.set_caret_state(cx.doc, cx.node, true, upstream);
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(cx.doc) {
            s.edit.upstream_at = upstream.then_some((cx.node, offset));
        }
    });
    let Some(g) = host.control_geometry(cx.doc, cx.node)? else {
        ime::report(host, cx);
        return Ok(());
    };
    let li = g.inner.line_of(offset, upstream);
    let x = f64::from(g.inner.x_of(li, offset));
    let l = &g.inner.lines[li];
    let (vw, vh) = g.viewport_size();
    let (mut sx, mut sy) = g.scroll;
    if x < sx {
        sx = x;
    } else if x + 1.0 > sx + vw {
        sx = x + 1.0 - vw;
    }
    if g.inner.multiline {
        let (top, bottom) = (f64::from(l.top), f64::from(l.top + l.height));
        if top < sy {
            sy = top;
        } else if bottom > sy + vh {
            sy = bottom - vh;
        }
    }
    if (sx, sy) != g.scroll {
        host.scroll_set(cx.doc, Some(cx.node), sx, sy)?;
    }
    ime::report(host, cx);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Value-changing commands

/// One edit: replace `range` (UTF-16) by `replacement`, caret after it.
struct Edit {
    input_type: &'static str,
    data: Option<String>,
    range: (u32, u32),
    replacement: String,
    /// How the step is recorded for undo (`None` = not at all: undo and redo themselves).
    record: Option<history::StepKind>,
    /// The selection after the edit when it is not the caret after the replacement.
    selection_after: Option<(u32, u32, SelectionDirection)>,
}

impl Edit {
    fn new(
        input_type: &'static str,
        data: Option<String>,
        range: (u32, u32),
        replacement: String,
        kind: history::StepKind,
    ) -> Edit {
        Edit {
            input_type,
            data,
            range,
            replacement,
            record: Some(kind),
            selection_after: None,
        }
    }
}

/// `beforeinput` -> apply -> `input` (editing.md section 1). Returns whether the value changed.
fn edit(host: &mut dyn DispatchHost, cx: &Cx, e: Edit) -> Result<bool, Status> {
    let Some(c) = snapshot(host, cx)? else {
        return Ok(false);
    };
    if c.readonly {
        return Ok(false);
    }
    let mut replacement = e.replacement.clone();
    if !c.kind.multiline {
        replacement.retain(|ch| ch != '\n' && ch != '\r');
    }
    let (s, t) = (e.range.0.min(c.len()), e.range.1.min(c.len()));
    // `maxlength` counts UTF-16 code units of the value; an insertion is cut to what remains.
    if let Some(max) = c.maxlength
        && !replacement.is_empty()
        && e.record.is_some()
    {
        let room = (max as i64 - (i64::from(c.len()) - i64::from(t - s))).max(0) as u32;
        if room == 0 {
            return Ok(false);
        }
        if utf16_len(&replacement) > room {
            let mut cut = String::new();
            let mut used = 0u32;
            for ch in replacement.chars() {
                if used + ch.len_utf16() as u32 > room {
                    break;
                }
                used += ch.len_utf16() as u32;
                cut.push(ch);
            }
            replacement = cut;
        }
    }
    if s == t && replacement.is_empty() {
        return Ok(false);
    }
    let data = e.data.clone().map(|d| {
        if replacement.is_empty() || e.input_type != "insertText" {
            d
        } else {
            replacement.clone()
        }
    });
    if fire(
        host,
        cx,
        input_event("beforeinput", e.input_type, data.as_deref(), true, cx.m),
    )? {
        return Ok(false);
    }
    let sel_after = e.selection_after;
    let changed = eng(host, cx.doc, |_, d| {
        let c = Ctl::read(d, cx.node)?;
        if c.readonly {
            return None;
        }
        let chars = Chars::new(&c.value);
        let (s, t) = (s.min(c.len()), t.min(c.len()));
        let (si, ti) = (chars.index(s), chars.index(t));
        let mut v = String::with_capacity(c.value.len() + replacement.len());
        v.extend(chars.chars[..si].iter());
        v.push_str(&replacement);
        v.extend(chars.chars[ti..].iter());
        let removed: String = chars.chars[si..ti].iter().collect();
        let at = chars.off[si];
        let caret = at + utf16_len(&replacement);
        let after = sel_after.map_or(
            Selection {
                start: caret,
                end: caret,
                direction: SelectionDirection::None,
            },
            |(a, b, dir)| Selection {
                start: a,
                end: b,
                direction: dir,
            },
        );
        let mut w = d.control_state_writer();
        w.set_value(cx.node, &v).ok()?;
        w.set_selection(cx.node, Some(after)).ok()?;
        Some((
            c.value != v,
            after.end,
            history::Applied {
                at,
                removed,
                inserted: replacement.clone(),
                before: (c.start, c.end, c.dir),
                after: (after.start, after.end, after.direction),
            },
        ))
    })?;
    let Some((value_changed, caret, applied)) = changed else {
        return Ok(false);
    };
    let selection_changed = applied.before != applied.after;
    if value_changed {
        if let Some(kind) = e.record {
            let now = cx.ts.unwrap_or_else(|| host.now_ms());
            history::record(host, cx, kind, applied, now);
        }
        fire(
            host,
            cx,
            input_event("input", e.input_type, data.as_deref(), false, cx.m),
        )?;
    }
    if selection_changed {
        fire(host, cx, EventInit::new("selectionchange").bubbles(true))?;
    }
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(cx.doc) {
            s.edit.goal_x = None;
        }
    });
    reveal_caret(host, cx, caret, false)?;
    Ok(value_changed)
}

/// The characters a number input accepts from the keyboard.
fn number_accepts(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.' | 'e' | 'E')
}

fn insert_text(
    host: &mut dyn DispatchHost,
    cx: &Cx,
    k: &KeyData,
    text: &str,
) -> Result<(), Status> {
    let Some(c) = snapshot(host, cx)? else {
        return Ok(());
    };
    if c.readonly {
        return Ok(());
    }
    let text: String = if c.kind.number {
        text.chars().filter(|ch| number_accepts(*ch)).collect()
    } else {
        text.to_owned()
    };
    if text.is_empty() {
        return Ok(());
    }
    if fire(host, cx, key_event("keypress", k, cx.m))? {
        return Ok(());
    }
    edit(
        host,
        cx,
        Edit::new(
            "insertText",
            Some(text.clone()),
            (c.start, c.end),
            text,
            history::StepKind::Insert,
        ),
    )?;
    Ok(())
}

fn insert_line_break(host: &mut dyn DispatchHost, cx: &Cx, k: &KeyData) -> Result<(), Status> {
    let Some(c) = snapshot(host, cx)? else {
        return Ok(());
    };
    if c.readonly {
        return Ok(());
    }
    if fire(host, cx, key_event("keypress", k, cx.m))? {
        return Ok(());
    }
    edit(
        host,
        cx,
        Edit::new(
            "insertLineBreak",
            None,
            (c.start, c.end),
            "\n".into(),
            history::StepKind::Other,
        ),
    )?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Backward,
    Forward,
}

fn delete(host: &mut dyn DispatchHost, cx: &Cx, dir: Dir, word: bool) -> Result<(), Status> {
    let Some(c) = snapshot(host, cx)? else {
        return Ok(());
    };
    if c.readonly {
        return Ok(());
    }
    let (start, end, input_type) = if c.start != c.end {
        (
            c.start,
            c.end,
            match (dir, word) {
                (Dir::Backward, false) => "deleteContentBackward",
                (Dir::Forward, false) => "deleteContentForward",
                (Dir::Backward, true) => "deleteWordBackward",
                (Dir::Forward, true) => "deleteWordForward",
            },
        )
    } else {
        let t = Chars::new(&c.value);
        let i = t.index(c.start);
        match dir {
            Dir::Backward => {
                let j = if word {
                    word_left(&t, i)
                } else {
                    t.cluster_start(i)
                };
                (
                    t.off[j],
                    c.start,
                    if word {
                        "deleteWordBackward"
                    } else {
                        "deleteContentBackward"
                    },
                )
            }
            Dir::Forward => {
                let j = if word {
                    word_right(&t, i)
                } else {
                    t.cluster_end(i)
                };
                (
                    c.start,
                    t.off[j.min(t.n())],
                    if word {
                        "deleteWordForward"
                    } else {
                        "deleteContentForward"
                    },
                )
            }
        }
    };
    if start == end {
        return Ok(());
    }
    edit(
        host,
        cx,
        Edit::new(
            input_type,
            None,
            (start, end),
            String::new(),
            history::StepKind::Other,
        ),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Caret and selection commands

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Char(Dir),
    /// An arrow key: one grapheme visually left (`Backward`) or right (`Forward`); on a line with
    /// right-to-left text that is not the previous or next offset in the value.
    Visual(Dir),
    Word(Dir),
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    Up,
    Down,
    PageUp,
    PageDown,
}

/// The offset one grapheme visually left (`Backward`) or right of the caret on a line that holds
/// right-to-left text (or is right-to-left): the caret stop with the nearest smaller or larger x;
/// past the visual end of the line, the far end of the line before or after it. `None` when the
/// line is plain left-to-right text (the logical neighbour is the visual one). A selection that is
/// not extended collapses to its visually left or right edge.
fn visual_step(
    inner: &ControlInner,
    focus: u32,
    start: u32,
    end: u32,
    collapsed_or_extending: bool,
    d: Dir,
) -> Option<u32> {
    let li = inner.line_of(focus, false);
    let line = &inner.lines[li];
    if !line.bidi {
        return None;
    }
    let left = d == Dir::Backward;
    if !collapsed_or_extending {
        let (xs, xe) = (
            inner.x_of(inner.line_of(start, false), start),
            inner.x_of(inner.line_of(end, false), end),
        );
        return Some(if (xs <= xe) == left { start } else { end });
    }
    let x = inner.x_of(li, focus);
    let eps = 0.01;
    let best =
        |l: &ui4me_engine_types::control_inner::InnerLine, left_side: bool| -> Option<(u32, f32)> {
            l.stops
                .iter()
                .filter(|(_, sx)| {
                    if left_side {
                        *sx < x - eps
                    } else {
                        *sx > x + eps
                    }
                })
                .copied()
                .reduce(|a, b| {
                    let better = if left_side { b.1 > a.1 } else { b.1 < a.1 };
                    if better { b } else { a }
                })
        };
    if let Some((o, _)) = best(line, left) {
        return Some(o);
    }
    // At the visual end of the line: the far end of the neighbouring line, if there is one.
    let far = |l: &ui4me_engine_types::control_inner::InnerLine, rightmost: bool| -> Option<u32> {
        l.stops
            .iter()
            .copied()
            .reduce(|a, b| if (b.1 > a.1) == rightmost { b } else { a })
            .map(|(o, _)| o)
    };
    if left && li > 0 {
        return far(&inner.lines[li - 1], true);
    }
    if !left && li + 1 < inner.lines.len() {
        return far(&inner.lines[li + 1], false);
    }
    Some(focus)
}

fn goal_x(host: &mut dyn DispatchHost, cx: &Cx) -> Option<f32> {
    rt(host, |ix| {
        ix.doc_state(cx.doc).ok().and_then(|s| s.edit.goal_x)
    })
}

fn set_goal_x(host: &mut dyn DispatchHost, cx: &Cx, x: Option<f32>) {
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(cx.doc) {
            s.edit.goal_x = x;
        }
    });
}

/// Move the caret (or extend the selection, keeping its anchor).
fn motion(host: &mut dyn DispatchHost, cx: &Cx, mo: Motion, extend: bool) -> Result<(), Status> {
    let Some(c) = snapshot(host, cx)? else {
        return Ok(());
    };
    let (anchor, focus) = c.anchor_focus();
    let t = Chars::new(&c.value);
    let len = c.len();
    let collapsed = c.start == c.end;
    let mut upstream = false;
    let held_upstream = rt(host, |ix| {
        ix.doc_state(cx.doc)
            .ok()
            .is_some_and(|s| s.edit.upstream_at == Some((cx.node, focus)))
    });
    let mut keep_goal = false;
    let g = match mo {
        Motion::Visual(_)
        | Motion::LineStart
        | Motion::LineEnd
        | Motion::Up
        | Motion::Down
        | Motion::PageUp
        | Motion::PageDown => host.control_geometry(cx.doc, cx.node)?,
        _ => None,
    };
    let new_focus = match mo {
        // A collapsed move by one: a non-empty selection collapses to its edge instead.
        Motion::Char(Dir::Backward) if !extend && !collapsed => c.start,
        Motion::Char(Dir::Forward) if !extend && !collapsed => c.end,
        Motion::Visual(d) => match g
            .as_ref()
            .and_then(|g| visual_step(&g.inner, focus, c.start, c.end, collapsed || extend, d))
        {
            Some(v) => v,
            None if !extend && !collapsed => {
                if d == Dir::Backward {
                    c.start
                } else {
                    c.end
                }
            }
            None if d == Dir::Backward => t.off[t.cluster_start(t.index(focus))],
            None => t.off[t.cluster_end(t.index(focus)).min(t.n())],
        },
        Motion::Char(Dir::Backward) => t.off[t.cluster_start(t.index(focus))],
        Motion::Char(Dir::Forward) => t.off[t.cluster_end(t.index(focus)).min(t.n())],
        Motion::Word(Dir::Backward) => t.off[word_left(&t, t.index(focus))],
        Motion::Word(Dir::Forward) => t.off[word_right(&t, t.index(focus)).min(t.n())],
        Motion::DocStart => 0,
        Motion::DocEnd => len,
        Motion::LineStart | Motion::LineEnd => match &g {
            Some(g) if g.inner.multiline => {
                let li = g.inner.line_of(focus, held_upstream);
                let l = &g.inner.lines[li];
                if mo == Motion::LineStart {
                    l.start
                } else {
                    // The end of a soft-wrapped line is also the start of the next one.
                    upstream = l.end == g.inner.lines.get(li + 1).map_or(u32::MAX, |n| n.start);
                    l.end
                }
            }
            _ => {
                if mo == Motion::LineStart {
                    0
                } else {
                    len
                }
            }
        },
        Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => match &g {
            Some(g) if g.inner.multiline => {
                let li = g.inner.line_of(focus, held_upstream);
                let x = goal_x(host, cx).unwrap_or_else(|| g.inner.x_of(li, focus));
                keep_goal = true;
                set_goal_x(host, cx, Some(x));
                let page = ((g.viewport_size().1 / f64::from(g.inner.line_height.max(1.0))).floor()
                    as i64
                    - 1)
                .max(1);
                let step = if matches!(mo, Motion::Up | Motion::Down) {
                    1
                } else {
                    page
                };
                let target = if matches!(mo, Motion::Up | Motion::PageUp) {
                    li as i64 - step
                } else {
                    li as i64 + step
                };
                if target < 0 {
                    0
                } else if target >= g.inner.lines.len() as i64 {
                    len
                } else {
                    g.inner.offset_at(target as usize, x)
                }
            }
            // A single line: up and down go to its ends.
            _ => {
                if matches!(mo, Motion::Up | Motion::PageUp) {
                    0
                } else {
                    len
                }
            }
        },
    };
    if !keep_goal {
        set_goal_x(host, cx, None);
    }
    let (a, f) = if extend {
        (anchor, new_focus)
    } else {
        (new_focus, new_focus)
    };
    write_selection(host, cx, a, f, true)?;
    reveal_caret(host, cx, new_focus, upstream)
}

fn select_all(host: &mut dyn DispatchHost, cx: &Cx) -> Result<(), Status> {
    let Some(c) = snapshot(host, cx)? else {
        return Ok(());
    };
    write_selection(host, cx, 0, c.len(), true)?;
    reveal_caret(host, cx, c.len(), false)
}

// ---------------------------------------------------------------------------------------------
// Keyboard entry point

/// Implicit submission (HTML): the form's default button is clicked; a form without one but with a
/// single text field is submitted. `change` has already been fired.
fn implicit_submit(host: &mut dyn DispatchHost, cx: &Cx) -> Result<(), Status> {
    let target = eng(host, cx.doc, |_, d| {
        let form = crate::interaction::activation::form_owner(d, cx.node)?;
        let mut buttons = Vec::new();
        let mut fields = 0;
        fn walk(d: &Document, n: NodeId, out: &mut Vec<NodeId>, fields: &mut u32) {
            if crate::interaction::activation::is_submit_button(d, n) {
                out.push(n);
            } else if text_kind(d, n).is_some_and(|k| !k.multiline) {
                *fields += 1;
            }
            if let Ok(children) = d.children(n) {
                for c in children {
                    walk(d, *c, out, fields);
                }
            }
        }
        let mut f = 0u32;
        walk(d, form, &mut buttons, &mut f);
        fields += f;
        let button = buttons
            .into_iter()
            .find(|b| !crate::interaction::activation::is_disabled_form_control(d, *b));
        match (button, fields) {
            (Some(b), _) => Some((b, true)),
            (None, n) if n <= 1 => Some((form, false)),
            _ => None,
        }
    })?;
    let Some((t, is_button)) = target else {
        return Ok(());
    };
    if is_button {
        let mut click = EventInit::new("click")
            .interface(EventInterface::PointerEvent)
            .bubbles(true)
            .cancelable(true);
        click.detail = 1;
        click.modifiers = cx.m;
        run_dispatch(host, cx.doc, t, click, true, cx.origin, cx.ts)?;
    } else {
        crate::interaction::activation::submit_form(host, cx.doc, t, None);
    }
    Ok(())
}

/// A control step (number arrows, assistive Increment) already fired its own `change`: move the
/// baseline to `value` so a later blur does not fire a second one. Leaves composition and history alone.
pub(crate) fn set_baseline(
    ix: &mut crate::interaction::dispatch::InteractionRuntime,
    doc: DocumentId,
    node: NodeId,
    value: &str,
) {
    if let Some(s) = ix.docs.get_mut(&doc)
        && let Some(b) = s.edit.baseline.get_mut(&node)
    {
        *b = value.to_owned();
    }
}

/// A host write replaced the value of `node`: when it has a `change` baseline, restart it from `value`.
pub(crate) fn reset_baseline(
    ix: &mut crate::interaction::dispatch::InteractionRuntime,
    doc: DocumentId,
    node: NodeId,
    value: &str,
) -> bool {
    if let Some(s) = ix.docs.get_mut(&doc)
        && let Some(b) = s.edit.baseline.get_mut(&node)
    {
        *b = value.to_owned();
    }
    // A host value write abandons a composition and ends the undo history (editing.md sections 4, 5).
    let abandoned = ime::abandon(ix, doc, node);
    history::clear(ix, doc, node);
    abandoned
}

/// Fire `change` when the value differs from the baseline, and make the value the new baseline.
fn commit_change(host: &mut dyn DispatchHost, cx: &Cx) -> Result<(), Status> {
    let Some(c) = snapshot(host, cx)? else {
        return Ok(());
    };
    let differs = rt(host, |ix| match ix.doc_state(cx.doc) {
        Ok(s) => match s.edit.baseline.get(&cx.node) {
            Some(b) if *b != c.value => {
                s.edit.baseline.insert(cx.node, c.value.clone());
                true
            }
            _ => false,
        },
        Err(_) => false,
    });
    if differs {
        fire(host, cx, EventInit::new("change").bubbles(true))?;
    }
    Ok(())
}

/// An assistive technology's SetValue (013 actions.md §1; `crate::interaction::assistive`): replace the whole value as one
/// edit (`beforeinput` cancelable, `input`, inputType `insertReplacementText`), then `change` when the value
/// differs from the baseline. Returns whether the value changed.
pub(crate) fn replace_all(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    value: &str,
    origin: DispatchOrigin,
) -> Result<bool, Status> {
    let cx = Cx {
        doc,
        node,
        m: Modifiers::default(),
        origin,
        ts: None,
    };
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(false);
    };
    if c.readonly {
        return Ok(false);
    }
    // A control that could not take focus has no `change` baseline yet: the value before the edit is it.
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(doc) {
            s.edit
                .baseline
                .entry(node)
                .or_insert_with(|| c.value.clone());
        }
    });
    let changed = edit(
        host,
        &cx,
        Edit::new(
            "insertReplacementText",
            Some(value.to_owned()),
            (0, c.len()),
            value.to_owned(),
            history::StepKind::Other,
        ),
    )?;
    commit_change(host, &cx)?;
    Ok(changed)
}

/// An assistive SetTextSelection (0.5 F0504): any composition ends, then the selection is placed
/// at `anchor`..`focus` (UTF-16, clamped) and the caret revealed.
pub(crate) fn assistive_select(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    anchor: u32,
    focus: u32,
    origin: DispatchOrigin,
) -> Result<(), Status> {
    let cx = Cx {
        doc,
        node,
        m: Modifiers::default(),
        origin,
        ts: None,
    };
    ime::finish(host, doc, true, Modifiers::default(), origin, None)?;
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(());
    };
    let chars = Chars::new(&c.value);
    let (a, f) = (
        chars.off[chars.index(anchor.min(c.len()))],
        chars.off[chars.index(focus.min(c.len()))],
    );
    write_selection(host, &cx, a, f, true)?;
    reveal_caret(host, &cx, f, false)
}

/// An application selection write shares IME termination, caret reveal and notifications with
/// native selection. It does not move keyboard focus or destroy the undo history.
pub(crate) fn set_application_selection(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    start: u32,
    end: u32,
    direction: SelectionDirection,
) -> Result<(), Status> {
    let origin = DispatchOrigin::ElementTask;
    let cx = Cx {
        doc,
        node,
        m: Modifiers::default(),
        origin,
        ts: None,
    };
    finish_application_composition(host, doc, node)?;
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(());
    };
    let chars = Chars::new(&c.value);
    let end = chars.off[chars.index(end.min(c.len()))];
    let start = chars.off[chars.index(start.min(end))];
    let (anchor, focus) = if direction == SelectionDirection::Backward {
        (end, start)
    } else {
        (start, end)
    };
    write_selection(host, &cx, anchor, focus, true)?;
    // A direction of `none` is distinct from `forward` for the public selection API.
    if direction == SelectionDirection::None && start != end {
        eng(host, doc, |_, d| {
            d.control_state_writer().set_selection(
                node,
                Some(Selection {
                    start,
                    end,
                    direction,
                }),
            )
        })??;
    }
    reveal_caret(host, &cx, focus, false)
}

/// Application replacement as a single edit, with the final selection included in the undo
/// record. No temporary selection is exposed and cancellation preserves the original state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn replace_application_range(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    text: &str,
    start: u32,
    end: u32,
    mode: &str,
) -> Result<bool, Status> {
    let origin = DispatchOrigin::ElementTask;
    let cx = Cx {
        doc,
        node,
        m: Modifiers::default(),
        origin,
        ts: None,
    };
    finish_application_composition(host, doc, node)?;
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(false);
    };
    if c.readonly {
        return Ok(false);
    }
    let mut replacement = text.to_owned();
    if !c.kind.multiline {
        replacement.retain(|ch| ch != '\r' && ch != '\n');
    }
    if let Some(max) = c.maxlength {
        let room = (max as i64 - (i64::from(c.len()) - i64::from(end - start))).max(0) as u32;
        if !replacement.is_empty() && room == 0 {
            return Ok(false);
        }
        let chars = Chars::new(&replacement);
        let count = chars.index(room.min(chars.off[chars.n()]));
        replacement = chars.chars[..count].iter().collect();
    }
    let inserted = utf16_len(&replacement);
    let selection = match mode {
        "select" => (start, start + inserted, SelectionDirection::Forward),
        "start" => (start, start, SelectionDirection::None),
        "end" => (start + inserted, start + inserted, SelectionDirection::None),
        _ => {
            let delta = i64::from(inserted) - i64::from(end - start);
            let a = if c.start > end {
                (i64::from(c.start) + delta) as u32
            } else if c.start > start {
                start
            } else {
                c.start
            };
            let b = if c.end > end {
                (i64::from(c.end) + delta) as u32
            } else if c.end > start {
                start + inserted
            } else {
                c.end
            };
            (a, b, c.dir)
        }
    };
    edit(
        host,
        &cx,
        Edit {
            input_type: "insertReplacementText",
            data: Some(replacement.clone()),
            range: (start, end),
            replacement,
            record: Some(history::StepKind::Other),
            selection_after: Some(selection),
        },
    )
}

pub(crate) fn finish_application_composition(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
) -> Result<(), Status> {
    if rt(host, |ix| ix.focused_element(doc)) == Some(node) {
        ime::finish(
            host,
            doc,
            true,
            Modifiers::default(),
            DispatchOrigin::ElementTask,
            None,
        )?;
    }
    Ok(())
}

/// An assistive ReplaceSelectedText (0.5 F0504): the selection (or nothing, at the caret) is
/// replaced by `text` as one edit (`insertReplacementText`). Returns whether the value changed.
pub(crate) fn assistive_replace(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    text: &str,
    origin: DispatchOrigin,
) -> Result<bool, Status> {
    let cx = Cx {
        doc,
        node,
        m: Modifiers::default(),
        origin,
        ts: None,
    };
    ime::finish(host, doc, true, Modifiers::default(), origin, None)?;
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(false);
    };
    if c.readonly {
        return Ok(false);
    }
    let mut text = text.to_owned();
    if !c.kind.multiline {
        text.retain(|ch| ch != '\n' && ch != '\r');
    }
    edit(
        host,
        &cx,
        Edit::new(
            "insertReplacementText",
            Some(text.clone()),
            (c.start, c.end),
            text,
            history::StepKind::Other,
        ),
    )
}

/// The default action of an uncanceled `keydown` at a text control. Returns `true` when the key
/// was consumed (no further default action).
pub(crate) fn keydown(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    k: &KeyData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<bool, Status> {
    let cx = Cx {
        doc,
        node,
        m,
        origin,
        ts,
    };
    if k.is_composing {
        return Ok(false);
    }
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(false);
    };
    let ctrl = m.ctrl || m.meta;
    // Characters the key produces (011 section 2: the item's `text`), unless a shortcut.
    if let Some(text) = k
        .text
        .as_deref()
        .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
        .filter(|_| !(m.ctrl || m.alt || m.meta))
    {
        insert_text(host, &cx, k, text)?;
        return Ok(true);
    }
    let key = k.key.as_str();
    if ctrl && !m.alt && key.eq_ignore_ascii_case("a") {
        select_all(host, &cx)?;
        return Ok(true);
    }
    if let Some(command) = clipboard::command_of(key, m) {
        match command {
            clipboard::Command::Paste => clipboard::paste(host, &cx)?,
            c => clipboard::copy_or_cut(host, &cx, c)?,
        }
        return Ok(true);
    }
    if ctrl && !m.alt && key.eq_ignore_ascii_case("z") {
        history::undo_redo(host, &cx, m.shift)?;
        return Ok(true);
    }
    if ctrl && !m.alt && !m.shift && key.eq_ignore_ascii_case("y") {
        history::undo_redo(host, &cx, true)?;
        return Ok(true);
    }
    if m.alt && !ctrl {
        return Ok(false);
    }
    let extend = m.shift;
    match key {
        "ArrowLeft" => motion(
            host,
            &cx,
            if ctrl {
                Motion::Word(Dir::Backward)
            } else {
                Motion::Visual(Dir::Backward)
            },
            extend,
        )?,
        "ArrowRight" => motion(
            host,
            &cx,
            if ctrl {
                Motion::Word(Dir::Forward)
            } else {
                Motion::Visual(Dir::Forward)
            },
            extend,
        )?,
        "ArrowUp" if !ctrl => motion(host, &cx, Motion::Up, extend)?,
        "ArrowDown" if !ctrl => motion(host, &cx, Motion::Down, extend)?,
        "Home" => motion(
            host,
            &cx,
            if ctrl {
                Motion::DocStart
            } else {
                Motion::LineStart
            },
            extend,
        )?,
        "End" => motion(
            host,
            &cx,
            if ctrl {
                Motion::DocEnd
            } else {
                Motion::LineEnd
            },
            extend,
        )?,
        "PageUp" if c.kind.multiline => motion(host, &cx, Motion::PageUp, extend)?,
        "PageDown" if c.kind.multiline => motion(host, &cx, Motion::PageDown, extend)?,
        "Backspace" => delete(host, &cx, Dir::Backward, ctrl)?,
        "Delete" if !m.shift => delete(host, &cx, Dir::Forward, ctrl)?,
        "Enter" if c.kind.multiline => insert_line_break(host, &cx, k)?,
        "Enter" => {
            commit_change(host, &cx)?;
            implicit_submit(host, &cx)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// The clipboard command of a keydown, if any (sequences.rs, for focus outside a text control).
pub(crate) fn clipboard_command(key: &str, m: Modifiers) -> Option<clipboard::Command> {
    clipboard::command_of(key, m)
}

/// A clipboard shortcut with no text control focused: `copy`/`cut`/`paste` reach application code.
pub(crate) fn document_clipboard(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: NodeId,
    c: clipboard::Command,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    clipboard::document_command(host, doc, target, c, m, origin, ts)
}

/// `ImeEnabled`: tell the platform where the caret is.
pub(crate) fn ime_enabled(host: &mut dyn DispatchHost, doc: DocumentId) {
    if let Some(node) = rt(host, |ix| ix.focused_element(doc)) {
        let cx = Cx {
            doc,
            node,
            m: Modifiers::default(),
            origin: DispatchOrigin::Injected,
            ts: None,
        };
        ime::report(host, &cx);
    }
}

// ---------------------------------------------------------------------------------------------
// Focus

/// A text control received focus: remember the `change` baseline; a keyboard focus (Tab) selects
/// the whole value of a single-line input (browsers do), a caret is shown.
pub(crate) fn on_focus(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    keyboard: bool,
    origin: DispatchOrigin,
) -> Result<(), Status> {
    let cx = Cx {
        doc,
        node,
        m: Modifiers::default(),
        origin,
        ts: None,
    };
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(());
    };
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(doc) {
            s.edit.baseline.insert(node, c.value.clone());
        }
    });
    if keyboard && !c.kind.multiline && !c.value.is_empty() {
        write_selection(host, &cx, 0, c.len(), false)?;
    }
    host.set_caret_state(doc, node, true, false);
    host.caret_blink(doc, Some(node));
    ime::report(host, &cx);
    Ok(())
}

/// A text control is about to lose focus: `change` if the value was edited.
pub(crate) fn on_blur(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    origin: DispatchOrigin,
) -> Result<(), Status> {
    let cx = Cx {
        doc,
        node,
        m: Modifiers::default(),
        origin,
        ts: None,
    };
    host.caret_blink(doc, None);
    // The composition ends (`compositionend`) before `change` and `blur`. The window still has the
    // focus (window deactivation already ended it), so the platform is told to drop its preedit.
    ime::finish(host, doc, true, Modifiers::default(), origin, None)?;
    ime::clear(host, doc);
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(doc) {
            s.edit.drag = None;
        }
    });
    if eng(host, doc, |_, d| text_kind(d, node).is_some())? {
        commit_change(host, &cx)?;
        rt(host, |ix| {
            if let Ok(s) = ix.doc_state(doc) {
                s.edit.baseline.remove(&node);
            }
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Pointer

fn word_range(c: &Ctl, offset: u32) -> (u32, u32) {
    let t = Chars::new(&c.value);
    let (s, e) = word_around(&t, t.index(offset));
    (t.off[s], t.off[e])
}

fn paragraph_range(c: &Ctl, offset: u32) -> (u32, u32) {
    let t = Chars::new(&c.value);
    let (s, e) = paragraph_around(&t, t.index(offset));
    (t.off[s], t.off[e])
}

/// A primary press on a text control (after `mousedown`, focus done): place the caret, extend with
/// Shift, select a word (double click) or the line (triple click), and start a drag.
pub(crate) fn on_pointer_down(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    node: NodeId,
    p: &PointerData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    let cx = Cx {
        doc,
        node,
        m,
        origin,
        ts,
    };
    ime::finish(host, doc, true, m, origin, ts)?;
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(());
    };
    if eng(host, doc, |_, d| {
        crate::interaction::activation::is_disabled_form_control(d, node)
    })? {
        return Ok(());
    }
    let Some(g) = host.control_geometry(doc, node)? else {
        return Ok(());
    };
    let at = g.offset_at_point(p.x, p.y);
    let now = ts.unwrap_or_else(|| host.now_ms());
    let count = rt(host, |ix| {
        let Ok(s) = ix.doc_state(doc) else { return 1 };
        let n = match s.edit.click {
            Some(t)
                if t.node == node
                    && now - t.time <= MULTI_CLICK_MS
                    && (p.x - t.x).abs() <= MULTI_CLICK_PX
                    && (p.y - t.y).abs() <= MULTI_CLICK_PX =>
            {
                t.count % 3 + 1
            }
            _ => 1,
        };
        s.edit.click = Some(ClickTracker {
            node,
            x: p.x,
            y: p.y,
            time: now,
            count: n,
        });
        n
    });
    let (anchor, focus, granularity) = match count {
        2 => {
            let (s, e) = word_range(&c, at);
            (s, e, Granularity::Word { start: s, end: e })
        }
        3 => {
            let (s, e) = if c.kind.multiline {
                paragraph_range(&c, at)
            } else {
                (0, c.len())
            };
            (s, e, Granularity::Paragraph { start: s, end: e })
        }
        _ if m.shift => (c.anchor_focus().0, at, Granularity::Char),
        _ => (at, at, Granularity::Char),
    };
    let drag_anchor = if m.shift && count == 1 {
        c.anchor_focus().0
    } else {
        anchor
    };
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(doc) {
            s.edit.drag = Some(Drag {
                node,
                anchor: drag_anchor,
                granularity,
            });
            s.edit.goal_x = None;
        }
    });
    write_selection(host, &cx, anchor, focus, true)?;
    reveal_caret(host, &cx, focus, false)
}

/// A pointer move with the primary button held: extend the selection of the dragged control.
pub(crate) fn on_pointer_move(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    if p.buttons & 1 == 0 {
        return Ok(());
    }
    let Some(drag) = rt(host, |ix| ix.doc_state(doc).ok().and_then(|s| s.edit.drag)) else {
        return Ok(());
    };
    let cx = Cx {
        doc,
        node: drag.node,
        m,
        origin,
        ts,
    };
    let Some(c) = snapshot(host, &cx)? else {
        return Ok(());
    };
    let Some(g) = host.control_geometry(doc, drag.node)? else {
        return Ok(());
    };
    let at = g.offset_at_point(p.x, p.y);
    let (anchor, focus) = match drag.granularity {
        Granularity::Char => (drag.anchor, at),
        Granularity::Word { start, end } => {
            let (s, e) = word_range(&c, at);
            if at < start {
                (end, s)
            } else {
                (start, e.max(end))
            }
        }
        Granularity::Paragraph { start, end } => {
            let (s, e) = if c.kind.multiline {
                paragraph_range(&c, at)
            } else {
                (0, c.len())
            };
            if at < start {
                (end, s)
            } else {
                (start, e.max(end))
            }
        }
    };
    write_selection(host, &cx, anchor, focus, true)?;
    reveal_caret(host, &cx, focus, false)
}

/// The primary button was released: the drag ends.
pub(crate) fn on_pointer_up(host: &mut dyn DispatchHost, doc: DocumentId) {
    rt(host, |ix| {
        if let Ok(s) = ix.doc_state(doc) {
            s.edit.drag = None;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_keep_combining_marks_and_crlf_together() {
        let t = Chars::new("a\u{0301}b\r\nc");
        assert_eq!(t.cluster_end(0), 2, "a + combining acute");
        assert_eq!(t.cluster_start(2), 0);
        assert_eq!(t.cluster_end(3), 5, "CRLF is one cluster");
        assert_eq!(t.cluster_start(5), 3);
        let flag = Chars::new("\u{1F1E9}\u{1F1EA}x");
        assert_eq!(flag.cluster_end(0), 2, "a regional-indicator pair");
        assert_eq!(flag.cluster_start(2), 0, "backward must not split the flag");
    }

    #[test]
    fn unicode_graphemes_are_symmetric_for_flags_zwj_indic_and_hangul() {
        for text in ["🇨🇿", "👩🏽‍💻", "👨‍👩‍👧‍👦", "क्\u{200d}ष", "각", "a\u{0301}"]
        {
            let chars = Chars::new(text);
            assert_eq!(chars.cluster_end(0), chars.n(), "forward {text:?}");
            assert_eq!(chars.cluster_start(chars.n()), 0, "backward {text:?}");
        }
        let chars = Chars::new("🇨🇿🇩🇪");
        assert_eq!(chars.cluster_start(4), 2);
        assert_eq!(chars.cluster_start(2), 0);
    }

    #[test]
    fn word_selection_uses_unicode_boundaries_for_apostrophes_and_combining_marks() {
        let text = Chars::new("can't e\u{0301}cole");
        assert_eq!(word_around(&text, 2), (0, 5));
        assert_eq!(word_around(&text, 8), (6, 12));
        assert_eq!(word_left(&text, 12), 6);
    }

    #[test]
    fn words_follow_character_classes() {
        let t = Chars::new("foo  bar, baz");
        assert_eq!(word_left(&t, 13), 10);
        assert_eq!(word_left(&t, 10), 8, "the comma is a word of its own");
        assert_eq!(word_left(&t, 8), 5);
        assert_eq!(word_left(&t, 5), 0);
        if !cfg!(windows) {
            assert_eq!(word_right(&t, 0), 3);
            assert_eq!(word_right(&t, 3), 8);
            assert_eq!(word_right(&t, 8), 9);
        }
        assert_eq!(word_around(&t, 1), (0, 3));
        assert_eq!(
            word_around(&t, 3),
            (0, 3),
            "at the end of a word the word before the caret"
        );
        assert_eq!(word_around(&t, 4), (3, 5), "white space between words");
    }

    #[test]
    fn utf16_offsets_count_surrogate_pairs() {
        let t = Chars::new("a\u{1F600}b");
        assert_eq!(t.off, vec![0, 1, 3, 4]);
        assert_eq!(t.index(3), 2);
        assert_eq!(t.index(2), 1, "inside a pair snaps to its start");
    }
}
