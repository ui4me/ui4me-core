//! Deterministic-theme paint tokens of the text controls (OD-28; 012 `controls.md` section 5):
//! the light scheme's values, and the canvas default of the dark used colour scheme (display-list.md
//! section 6; the contract defines no other dark token).
use crate::scene::display_list::Color;

/// Canvas default of a dark used colour scheme (010 FR-023): `#121212`, the `Canvas` system colour
/// of `color-scheme: dark` in the reference browser (measured 2026-11-06 with the oracle's
/// Chromium family; the canonical value is recorded by T065's OC capture, which may replace it).
pub const CANVAS_DARK: Color = Color {
    r: 18.0 / 255.0,
    g: 18.0 / 255.0,
    b: 18.0 / 255.0,
    a: 1.0,
};

/// `::placeholder` colour (`darkgray`-like #757575).
pub const PLACEHOLDER: Color = Color {
    r: 117.0 / 255.0,
    g: 117.0 / 255.0,
    b: 117.0 / 255.0,
    a: 1.0,
};
/// Selection highlight of a focused text control (an opaque light blue behind the text).
pub const SELECTION_BACKGROUND: Color = Color {
    r: 179.0 / 255.0,
    g: 212.0 / 255.0,
    b: 253.0 / 255.0,
    a: 1.0,
};
/// Width of the caret in CSS px.
pub const CARET_WIDTH: f32 = 1.0;
/// Thickness of the IME preedit underline in CSS px (solid, text colour; 010 `display-list.md` section 6).
pub const PREEDIT_UNDERLINE_WIDTH: f32 = 1.0;

/// Focus ring of `outline-style: auto` (display-list.md section 6, OD-28): solid, 2 CSS px,
/// `#005FCC`, following the border-box radii at `outline-offset`.
pub const FOCUS_RING: Color = Color {
    r: 0.0,
    g: 95.0 / 255.0,
    b: 204.0 / 255.0,
    a: 1.0,
};
/// Width of the focus ring when `outline-width` is its initial value.
pub const FOCUS_RING_WIDTH: f32 = 2.0;

/// The ring width for a computed `outline-width`. `outline: auto` (the UA sheet) resets the width
/// to its initial value `medium` (3px); that value means "UA default" and gives the token width.
/// Any other computed width is the author's and is used as is.
pub fn focus_ring_width(computed: f32) -> f32 {
    if (computed - 3.0).abs() < 1e-3 {
        FOCUS_RING_WIDTH
    } else {
        computed
    }
}

/// The ring colour: the token when `outline-color` is `currentcolor` (its initial value, which
/// `outline: auto` resets to), else the author's computed colour.
pub fn focus_ring_color(computed: Color, is_current: bool) -> Color {
    if is_current { FOCUS_RING } else { computed }
}

/// Accent of the controls' filled parts (checked mark, range fill and thumb, progress bar,
/// popup highlight): Chromium's default accent `#0075FF` (OD-28); used for `accent-color: auto`.
pub const ACCENT: Color = Color {
    r: 0.0,
    g: 117.0 / 255.0,
    b: 1.0,
    a: 1.0,
};
/// A `meter` bar in its optimum, suboptimal and even-less-good regions.
pub const METER_GOOD: Color = Color {
    r: 16.0 / 255.0,
    g: 166.0 / 255.0,
    b: 16.0 / 255.0,
    a: 1.0,
};
pub const METER_WARN: Color = Color {
    r: 1.0,
    g: 214.0 / 255.0,
    b: 0.0,
    a: 1.0,
};
pub const METER_BAD: Color = Color {
    r: 230.0 / 255.0,
    g: 57.0 / 255.0,
    b: 70.0 / 255.0,
    a: 1.0,
};

/// Is `fg` a light colour (a dark used scheme draws light text)?
pub fn is_light(c: Color) -> bool {
    0.299 * c.r + 0.587 * c.g + 0.114 * c.b > 0.5
}

/// `c` with its alpha multiplied by `a`.
pub fn with_alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}
