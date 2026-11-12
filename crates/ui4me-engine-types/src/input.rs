//! owner: 011-platform-windowing (normalised input types) — created by 001 slice (R-28)
//!
//! Normalised input events (011 `contracts/input-normalization.md` §1–§2; U4C-WIN → U4C-INT).
//! Produced by 011's translation layer (`ui4me-platform::input`) and by 012's window-free
//! injection entry point; consumed by 012's NativeInputQueue (whose `InputItem` is this
//! envelope with `target = Document`). No upstream type appears here.
//!
//! Slice subset (001 T063). The variant shapes 012's slice code already matches on are kept
//! exactly (`Wheel`, `Resize`, `ScaleChange`, `Drop`); the contract's extra fields for those
//! kinds (`delta_mode`/`phase`, `viewport_css`/`device_px`, `files`) are added when 012 widens
//! its patterns (recorded deviation, lane P P-C3). Pointer and key fields follow §2 in full.

use crate::ids::{DocumentId, WindowId};

/// Envelope `target` (§1). `Document` is used by window-free injection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputTarget {
    Window(WindowId),
    Document(DocumentId),
}

/// Envelope `origin` (§1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputOrigin {
    Native,
    Injected,
}

/// Envelope `modifiers` (§1): the state after the event for key events, current otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct InputModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerType {
    Mouse,
    Pen,
    Touch,
}

impl PointerType {
    /// The Pointer Events `pointerType` string.
    pub fn as_str(self) -> &'static str {
        match self {
            PointerType::Mouse => "mouse",
            PointerType::Pen => "pen",
            PointerType::Touch => "touch",
        }
    }
}

/// `button` → `buttons` bit (§3: left/middle/right/back/forward = 0/1/2/3/4 → 1/4/2/8/16; pen
/// eraser 5 → 32). Unknown buttons have no bit.
pub fn button_bit(button: i8) -> u16 {
    match button {
        0 => 1,
        1 => 4,
        2 => 2,
        3 => 8,
        4 => 16,
        5 => 32,
        _ => 0,
    }
}

/// Pointer fields (§2). Positions are CSS px relative to the viewport (device px ÷ scale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerData {
    pub pointer_id: u32,
    pub pointer_type: PointerType,
    pub is_primary: bool,
    pub x: f64,
    pub y: f64,
    /// −1 for move/enter/leave/cancel.
    pub button: i8,
    pub buttons: u16,
    /// 0…1; without device pressure 0.5 while any button is down, else 0.
    pub pressure: f32,
    pub tilt_x: Option<f32>,
    pub tilt_y: Option<f32>,
    pub twist: Option<f32>,
    /// Contact geometry, CSS px (1 × 1 for mouse and pen).
    pub width: f32,
    pub height: f32,
}

impl PointerData {
    /// The primary mouse pointer (`pointer_id` 1) at `(x, y)` with `button` pressed (0 =
    /// primary); `buttons` holds that button's bit.
    pub fn mouse(x: f64, y: f64, button: i8) -> Self {
        PointerData {
            pointer_id: 1,
            pointer_type: PointerType::Mouse,
            is_primary: true,
            x,
            y,
            button,
            buttons: button_bit(button),
            pressure: 0.5,
            tilt_x: None,
            tilt_y: None,
            twist: None,
            width: 1.0,
            height: 1.0,
        }
    }
}

/// Key fields (§2, §5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyData {
    /// UI Events `key` value (`a`, `Enter`, `Dead`, `Process`, `Unidentified`, …).
    pub key: String,
    /// UI Events `code` value (`KeyA`, `ShiftLeft`, …; unknown → `Unidentified`).
    pub code: String,
    /// 0 standard, 1 left, 2 right, 3 numpad.
    pub location: u8,
    pub repeat: bool,
    pub is_composing: bool,
    /// KeyDown only: the characters the key inserts.
    pub text: Option<String>,
}

impl KeyData {
    pub fn new(key: impl Into<String>, code: impl Into<String>) -> Self {
        KeyData {
            key: key.into(),
            code: code.into(),
            location: 0,
            repeat: false,
            is_composing: false,
            text: None,
        }
    }
}

/// Normalised input kinds (§2).
#[derive(Clone, Debug, PartialEq)]
pub enum InputKind {
    PointerMove(PointerData),
    PointerDown(PointerData),
    PointerUp(PointerData),
    PointerEnter(PointerData),
    PointerLeave(PointerData),
    PointerCancel(PointerData),
    /// Deltas in CSS px, UI Events sign (positive = scroll towards the end); line deltas are
    /// converted by the platform (lane P P-D7) until `delta_mode` lands.
    Wheel {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    },
    KeyDown(KeyData),
    KeyUp(KeyData),
    ModifiersSync,
    ImeEnabled,
    ImeDisabled,
    /// `cursor` in UTF-16 code units of `text`; `None` = hide the cursor; empty `text` = clear.
    ImePreedit {
        text: String,
        cursor: Option<(u32, u32)>,
    },
    ImeCommit {
        text: String,
    },
    /// UTF-16 code units (Wayland only).
    ImeDeleteSurrounding {
        before_u16: u32,
        after_u16: u32,
    },
    WindowFocus {
        focused: bool,
    },
    /// Window-only (rejected for injection).
    CloseRequest,
    /// Window-only (rejected for injection). New viewport, CSS px.
    Resize {
        width: f64,
        height: f64,
    },
    /// Window-only (rejected for injection).
    ScaleChange {
        scale: f64,
    },
    /// Window-only (rejected for injection).
    Drop {
        x: f64,
        y: f64,
    },
}

impl InputKind {
    pub fn name(&self) -> &'static str {
        match self {
            InputKind::PointerMove(_) => "PointerMove",
            InputKind::PointerDown(_) => "PointerDown",
            InputKind::PointerUp(_) => "PointerUp",
            InputKind::PointerEnter(_) => "PointerEnter",
            InputKind::PointerLeave(_) => "PointerLeave",
            InputKind::PointerCancel(_) => "PointerCancel",
            InputKind::Wheel { .. } => "Wheel",
            InputKind::KeyDown(_) => "KeyDown",
            InputKind::KeyUp(_) => "KeyUp",
            InputKind::ModifiersSync => "ModifiersSync",
            InputKind::ImeEnabled => "ImeEnabled",
            InputKind::ImeDisabled => "ImeDisabled",
            InputKind::ImePreedit { .. } => "ImePreedit",
            InputKind::ImeCommit { .. } => "ImeCommit",
            InputKind::ImeDeleteSurrounding { .. } => "ImeDeleteSurrounding",
            InputKind::WindowFocus { .. } => "WindowFocus",
            InputKind::CloseRequest => "CloseRequest",
            InputKind::Resize { .. } => "Resize",
            InputKind::ScaleChange { .. } => "ScaleChange",
            InputKind::Drop { .. } => "Drop",
        }
    }

    /// `CloseRequest`/`Resize`/`ScaleChange`/`Drop` exist only for windows (§2.1).
    pub fn is_window_only(&self) -> bool {
        matches!(
            self,
            InputKind::CloseRequest
                | InputKind::Resize { .. }
                | InputKind::ScaleChange { .. }
                | InputKind::Drop { .. }
        )
    }

    /// §2.1 routing: `true` for the kinds that go to 012's NativeInputQueue (pointer, wheel,
    /// key, modifiers, IME, window focus); the window-only kinds go to 004's host callbacks.
    pub fn routes_to_queue(&self) -> bool {
        !self.is_window_only()
    }
}

/// One normalised event: the §1 envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct InputEvent {
    pub target: InputTarget,
    /// Monotonic runtime clock, ms; `None` = assigned by the consumer (host clock).
    pub timestamp_ms: Option<f64>,
    /// Strictly increasing per target.
    pub seq: u64,
    pub origin: InputOrigin,
    pub modifiers: InputModifiers,
    pub kind: InputKind,
}

/// UTF-8 byte offset → UTF-16 code-unit offset in `s` (§6 IME conversion). An offset inside a
/// character or past the end is clamped to the preceding character boundary.
pub fn utf8_to_utf16_offset(s: &str, byte: usize) -> u32 {
    let mut b = byte.min(s.len());
    while !s.is_char_boundary(b) {
        b -= 1;
    }
    s[..b].encode_utf16().count() as u32
}

/// Bytes before `cursor_byte` (UTF-8) counted as UTF-16 units; the text deleted by
/// `DeleteSurrounding{before_bytes}` ends at the cursor, so it is measured backwards from it.
pub fn utf8_span_before_to_utf16(s: &str, cursor_byte: usize, before_bytes: usize) -> u32 {
    let end = cursor_byte.min(s.len());
    let start = end.saturating_sub(before_bytes);
    utf8_to_utf16_offset(s, end) - utf8_to_utf16_offset(s, start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_bits_follow_section_3() {
        assert_eq!(
            [0, 1, 2, 3, 4, 5, 9].map(button_bit),
            [1, 4, 2, 8, 16, 32, 0]
        );
        let p = PointerData::mouse(1.0, 2.0, 2);
        assert_eq!(
            (p.pointer_id, p.buttons, p.width, p.height),
            (1, 2, 1.0, 1.0)
        );
    }

    #[test]
    fn window_only_kinds_do_not_route_to_the_queue() {
        assert!(!InputKind::CloseRequest.routes_to_queue());
        assert!(
            !InputKind::Resize {
                width: 1.0,
                height: 1.0
            }
            .routes_to_queue()
        );
        assert!(InputKind::WindowFocus { focused: true }.routes_to_queue());
        assert!(InputKind::ImeCommit { text: "x".into() }.routes_to_queue());
    }

    #[test]
    fn utf8_to_utf16_offsets() {
        let s = "a\u{e9}\u{1F600}b\u{301}";
        // a(1) é(2) 😀(4) b(1) combining acute(2) bytes; UTF-16: 1, 1, 2, 1, 1.
        assert_eq!(utf8_to_utf16_offset(s, 0), 0);
        assert_eq!(utf8_to_utf16_offset(s, 1), 1);
        assert_eq!(utf8_to_utf16_offset(s, 3), 2);
        assert_eq!(utf8_to_utf16_offset(s, 7), 4);
        assert_eq!(
            utf8_to_utf16_offset(s, 5),
            2,
            "inside the astral character: clamped back"
        );
        assert_eq!(utf8_to_utf16_offset(s, 999), 6);
        assert_eq!(utf8_span_before_to_utf16(s, 7, 4), 2);
    }

    proptest::proptest! {
        #[test]
        fn utf16_offset_matches_prefix_encoding(s in "\\PC{0,24}", cut in 0usize..96) {
            let mut b = cut.min(s.len());
            while !s.is_char_boundary(b) { b -= 1; }
            proptest::prop_assert_eq!(utf8_to_utf16_offset(&s, cut) as usize, s[..b].encode_utf16().count());
            proptest::prop_assert_eq!(utf8_to_utf16_offset(&s, s.len()) as usize, s.encode_utf16().count());
        }
    }
}
