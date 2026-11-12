//! owner: 011-platform-windowing with 012-interaction (lane L-CLIP-r13)
//!
//! What 012 asks 011 to tell the platform's input method (IME) about the focused text control
//! (AD-39): that composition input is wanted, what the control is for, where its caret is, and
//! the text around it. No upstream type appears here; 011 maps it to winit's `ImeRequest`.

/// What the focused control is for (winit `ImePurpose`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImePurposeKind {
    #[default]
    Normal,
    Password,
    Number,
    Phone,
    Url,
    Email,
}

/// The IME state of the window's focused text control.
#[derive(Clone, Debug, PartialEq)]
pub struct ImeRequest {
    pub purpose: ImePurposeKind,
    /// The caret rectangle `(x, y, width, height)` in viewport CSS px (the candidate box is placed
    /// next to it, not over it).
    pub cursor_area: (f64, f64, f64, f64),
    /// At most [`MAX_SURROUNDING_BYTES`] bytes of the value around the caret, without the preedit:
    /// `(text, cursor_byte, anchor_byte)`. `None` for a password control.
    pub surrounding: Option<(String, usize, usize)>,
}

/// The platform's limit for surrounding text (winit `ImeSurroundingText::MAX_TEXT_BYTES`).
pub const MAX_SURROUNDING_BYTES: usize = 4000;

/// Cut `text` to fewer than [`MAX_SURROUNDING_BYTES`] bytes around byte offsets `cursor` and `anchor`
/// (code-point boundaries), returning the excerpt and the offsets within it.
pub fn surrounding_excerpt(text: &str, cursor: usize, anchor: usize) -> (String, usize, usize) {
    let limit = MAX_SURROUNDING_BYTES - 1;
    if text.len() <= limit {
        return (text.to_owned(), cursor, anchor);
    }
    let (lo, hi) = (cursor.min(anchor), cursor.max(anchor));
    // Centre the window on the selection (or start it at the selection when it is wider than the limit).
    let span = hi - lo;
    let mut start = if span >= limit {
        lo
    } else {
        lo.saturating_sub((limit - span) / 2)
    };
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + limit).min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    let clamp = |o: usize| o.clamp(start, end) - start;
    (text[start..end].to_owned(), clamp(cursor), clamp(anchor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_kept_whole() {
        assert_eq!(
            surrounding_excerpt("foobar", 3, 3),
            ("foobar".to_owned(), 3, 3)
        );
    }

    #[test]
    fn long_text_is_cut_around_the_caret_on_char_boundaries() {
        let text = "\u{e9}".repeat(5000); // 2 bytes each
        let (cut, c, a) = surrounding_excerpt(&text, 5000, 5000);
        assert!(cut.len() < MAX_SURROUNDING_BYTES);
        assert!(cut.is_char_boundary(c) && c == a);
        assert!(c > 0 && c < cut.len());
    }
}
