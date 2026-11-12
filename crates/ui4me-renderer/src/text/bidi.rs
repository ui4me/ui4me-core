//! owner: 009-text-inline
//!
//! UBA control insertion, base direction and first-strong detection (T095, T096; FR-053..FR-057;
//! research R-09; CSS Writing Modes 4 section 2.4; UAX #9).
//!
//! The inline behaviour (`ui4me-layout/src/ifc/inline/builder.rs`) puts the Unicode explicit
//! formatting characters of an inline box around its content in the processed text
//! ([`controls_for`]) and records each as an `InlineItemKind::BidiControl` item over its bytes.
//! They are non-DOM text: never rendered (zero advance), never a caret stop, never selectable,
//! and they are not soft wrap opportunities. Parley's UAX #9 resolver (embeddings, isolates,
//! overrides, mirrored brackets, L1-L2 reordering per line) does the rest.
//!
//! | `unicode-bidi` | opening | closing |
//! |---|---|---|
//! | `normal` | none | none |
//! | `embed` | LRE / RLE | PDF |
//! | `isolate` | LRI / RLI | PDI |
//! | `bidi-override` | LRO / RLO | PDF |
//! | `isolate-override` | LRI / RLI, then LRO / RLO | PDF, then PDI |
//! | `plaintext` | FSI | PDI |

pub const LRE: char = '\u{202A}';
pub const RLE: char = '\u{202B}';
pub const PDF: char = '\u{202C}';
pub const LRO: char = '\u{202D}';
pub const RLO: char = '\u{202E}';
pub const LRI: char = '\u{2066}';
pub const RLI: char = '\u{2067}';
pub const FSI: char = '\u{2068}';
pub const PDI: char = '\u{2069}';

/// `unicode-bidi` as the text layer sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BidiMode {
    #[default]
    Normal,
    Embed,
    Isolate,
    Override,
    IsolateOverride,
    Plaintext,
}

/// The paragraph's base direction (UAX #9 P2-P3 for `Auto`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BaseDirection {
    #[default]
    Ltr,
    Rtl,
    /// First strong character of the paragraph (`unicode-bidi: plaintext`, `dir=auto`).
    Auto,
}

/// Whether `c` is one of the nine explicit formatting characters ([`controls_for`]).
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Opening and closing control sequences of an inline box with `mode` whose own direction is
/// `rtl` (CSS Writing Modes 4 section 2.4.2). A box that opens nothing returns two empty lists.
pub fn controls_for(mode: BidiMode, rtl: bool) -> (Vec<char>, Vec<char>) {
    let (embed, isolate, over) = if rtl {
        (RLE, RLI, RLO)
    } else {
        (LRE, LRI, LRO)
    };
    match mode {
        BidiMode::Normal => (vec![], vec![]),
        BidiMode::Embed => (vec![embed], vec![PDF]),
        BidiMode::Isolate => (vec![isolate], vec![PDI]),
        BidiMode::Override => (vec![over], vec![PDF]),
        BidiMode::IsolateOverride => (vec![isolate, over], vec![PDF, PDI]),
        BidiMode::Plaintext => (vec![FSI], vec![PDI]),
    }
}

/// Characters of the strong right-to-left classes (R and AL): Hebrew, Arabic, Syriac, Thaana,
/// NKo, Samaritan, Mandaic and the presentation forms, plus U+200F RLM and the Adlam, Hatran and
/// other right-to-left supplementary blocks.
pub fn is_strong_rtl(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x05FF | 0x0600..=0x07BF | 0x07C0..=0x085F | 0x0860..=0x08FF
        | 0x200F | 0xFB1D..=0xFB4F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF
        | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
        && !matches!(c as u32, 0x0600..=0x0605 | 0x0660..=0x0669 | 0x066B..=0x066C | 0x06DD | 0x06F0..=0x06F9 | 0x0890..=0x0891 | 0x08E2)
        && !is_combining_mark(c)
}

fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8 | 0x06EA..=0x06ED)
}

/// Whether `c` is a strong left-to-right character: a letter that is not right-to-left (digits,
/// punctuation and marks are weak or neutral), or U+200E LRM.
pub fn is_strong_ltr(c: char) -> bool {
    c == '\u{200E}' || (c.is_alphabetic() && !is_strong_rtl(c) && !is_combining_mark(c))
}

/// UAX #9 P2-P3: the direction of the first strong character of `text` (`Some(true)` for
/// right-to-left), skipping the content of isolates (between an isolate initiator and its
/// matching PDI) and embeddings' controls; `None` when there is none.
pub fn first_strong(text: &str) -> Option<bool> {
    let mut isolate_depth = 0usize;
    for c in text.chars() {
        match c {
            LRI | RLI | FSI => isolate_depth += 1,
            PDI => isolate_depth = isolate_depth.saturating_sub(1),
            _ if isolate_depth > 0 => {}
            _ if is_strong_rtl(c) => return Some(true),
            _ if is_strong_ltr(c) => return Some(false),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_table() {
        assert_eq!(controls_for(BidiMode::Embed, true), (vec![RLE], vec![PDF]));
        assert_eq!(controls_for(BidiMode::Embed, false), (vec![LRE], vec![PDF]));
        assert_eq!(
            controls_for(BidiMode::Isolate, true),
            (vec![RLI], vec![PDI])
        );
        assert_eq!(
            controls_for(BidiMode::Override, false),
            (vec![LRO], vec![PDF])
        );
        assert_eq!(
            controls_for(BidiMode::IsolateOverride, true),
            (vec![RLI, RLO], vec![PDF, PDI])
        );
        assert_eq!(
            controls_for(BidiMode::Plaintext, true),
            (vec![FSI], vec![PDI])
        );
        assert_eq!(controls_for(BidiMode::Normal, true), (vec![], vec![]));
    }

    #[test]
    fn first_strong_skips_neutrals_digits_and_isolates() {
        assert_eq!(first_strong("123 (abc"), Some(false));
        assert_eq!(first_strong("123 \u{5E9}\u{5DC}"), Some(true));
        assert_eq!(first_strong("  ,.-"), None);
        assert_eq!(first_strong("\u{2067}\u{5E9}\u{2069}abc"), Some(false));
        assert_eq!(first_strong("\u{627}\u{644}"), Some(true));
        // Arabic-Indic digits are numbers, not strong.
        assert_eq!(first_strong("\u{661}\u{662} abc"), Some(false));
    }

    #[test]
    fn control_characters_are_recognised() {
        for c in [LRE, RLE, PDF, LRO, RLO, LRI, RLI, FSI, PDI] {
            assert!(is_bidi_control(c));
        }
        assert!(!is_bidi_control('a') && !is_bidi_control('\u{200F}'));
    }
}
