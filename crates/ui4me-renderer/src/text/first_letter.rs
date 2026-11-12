//! owner: 009-text-inline (015 TXT-5, lane L-PSEUDO)
//!
//! The extent of the `::first-letter` of a text (CSS Pseudo 4 section 3.2), as the reference browser forms it
//! (measured in the pinned Chromium 1248, not read from Blink; `lane-LPSEUDO-evidence.md`):
//!
//! * the unit is the leading spaces and punctuation (any Unicode `P*`, so `"`, `(`, `-`, `_`, `...`, `#`), then
//!   one *grapheme cluster* (the first character that is neither: a letter, a digit, a symbol such as `$` or `+`,
//!   an emoji sequence, a regional-indicator pair), then the punctuation that follows it *immediately* (`a)`,
//!   `1,`); spaces between a letter and the punctuation after it do not belong to it;
//! * a text with no such character (only spaces and punctuation) has a first letter only when it ends in
//!   punctuation and the line's content goes on in a later text (`"<b>Quoted</b>`): the punctuation alone;
//! * the spaces and newlines in front count (they collapse in layout; the DOM text offsets are unchanged).

use icu_properties::props::{
    ExtendedPictographic, GeneralCategory, GeneralCategoryGroup, GraphemeClusterBreak,
};
use icu_properties::{CodePointMapData, CodePointSetData};

fn category(c: char) -> GeneralCategory {
    CodePointMapData::<GeneralCategory>::new().get(c)
}

fn is_punctuation(c: char) -> bool {
    GeneralCategoryGroup::Punctuation.contains(category(c))
}

/// Typographic space: the document white space and the space separators (a vertical tab is a control character,
/// not a space: it becomes the first letter itself, WPT first-letter-c0).
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
        || category(c) == GeneralCategory::SpaceSeparator
}

/// The word-separator characters of CSS Text 3 section 8.4: a space that is not one of them can bridge a letter and
/// the punctuation after it.
fn is_word_separator(c: char) -> bool {
    matches!(
        c,
        ' ' | '\u{a0}' | '\u{1361}' | '\u{10100}' | '\u{10101}' | '\u{1039f}' | '\u{1091f}'
    )
}

/// Open punctuation and dashes end the first letter when they follow it (`T(rail`, `T-rail`).
fn is_open_or_dash(c: char) -> bool {
    matches!(
        category(c),
        GeneralCategory::OpenPunctuation | GeneralCategory::DashPunctuation
    )
}

fn gcb(c: char) -> GraphemeClusterBreak {
    CodePointMapData::<GraphemeClusterBreak>::new().get(c)
}

fn pictographic(c: char) -> bool {
    CodePointSetData::new::<ExtendedPictographic>().contains(c)
}

/// The byte length of the grapheme cluster that starts `s` (UAX #29, without the rarely needed Indic conjunct
/// rule): prepends, Hangul syllable sequences, extenders and spacing marks, zero-width-joiner emoji sequences and
/// regional-indicator pairs.
fn cluster_len(s: &str) -> usize {
    use GraphemeClusterBreak as G;
    let mut it = s.char_indices().peekable();
    let Some((_, first)) = it.next() else {
        return 0;
    };
    let mut end = first.len_utf8();
    let mut prev = gcb(first);
    let mut prev_pict = pictographic(first);
    let mut ri_count = usize::from(prev == G::RegionalIndicator);
    // Prepend characters bind to what follows.
    while prev == G::Prepend {
        match it.next() {
            Some((i, c)) => {
                end = i + c.len_utf8();
                prev = gcb(c);
                prev_pict = pictographic(c);
                ri_count = usize::from(prev == G::RegionalIndicator);
            }
            None => return end,
        }
    }
    // After a ZWJ that follows an extended pictographic (plus its extenders), another pictographic joins.
    let mut pict_run = prev_pict;
    while let Some(&(i, c)) = it.peek() {
        let next = gcb(c);
        let join = match (prev, next) {
            (G::Control | G::CR | G::LF, _) | (_, G::Control | G::CR | G::LF) => false,
            (G::L, G::L | G::V | G::LV | G::LVT) => true,
            (G::LV | G::V, G::V | G::T) => true,
            (G::LVT | G::T, G::T) => true,
            (_, G::Extend | G::ZWJ | G::SpacingMark) => true,
            (G::ZWJ, _) if pict_run && pictographic(c) => true,
            (G::RegionalIndicator, G::RegionalIndicator) if ri_count % 2 == 1 => true,
            _ => false,
        };
        if !join {
            break;
        }
        if next == G::RegionalIndicator {
            ri_count += 1;
        }
        if next != G::Extend && next != G::ZWJ {
            pict_run = pictographic(c);
        }
        prev = next;
        end = i + c.len_utf8();
        it.next();
    }
    end
}

/// The byte length of the first letter of `text` (see the module documentation), or `None` when the text has
/// none. `more_text_follows`: the line's content goes on in a later text node. `dutch_ij`: the text is Dutch, where
/// `IJ` / `ij` is one letter (CSS Pseudo 4 section 3.2; WPT first-letter-digraph).
pub fn first_letter_len(text: &str, more_text_follows: bool, dutch_ij: bool) -> Option<usize> {
    let mut at = 0;
    let mut last_was_space = true;
    while let Some(c) = text[at..].chars().next() {
        let cluster = cluster_len(&text[at..]);
        if is_space(c) {
            last_was_space = true;
            at += cluster;
        } else if is_punctuation(c) {
            last_was_space = false;
            at += cluster;
        } else {
            let mut end = at + cluster;
            if dutch_ij
                && ((c == 'I' && text[end..].starts_with('J'))
                    || (c == 'i' && text[end..].starts_with('j')))
            {
                end += 1;
            }
            // The punctuation that follows the letter (not an open one or a dash), over any space that is not a word
            // separator between them; a space after the last of them is not part of it.
            loop {
                let mut j = end;
                while let Some(sp) = text[j..].chars().next().filter(|sp| {
                    is_space(*sp) && !is_word_separator(*sp) && !matches!(sp, '\n' | '\r' | '\u{c}')
                }) {
                    j += sp.len_utf8();
                }
                match text[j..].chars().next() {
                    Some(p) if is_punctuation(p) && !is_open_or_dash(p) => {
                        end = j + cluster_len(&text[j..])
                    }
                    _ => break,
                }
            }
            return Some(end);
        }
    }
    // Only spaces and punctuation.
    (!last_was_space && !text.is_empty() && more_text_follows).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use super::first_letter_len as f2;

    fn f(s: &str, more: bool, ij: bool) -> Option<usize> {
        f2(s, more, ij)
    }

    fn head(s: &str) -> &str {
        &s[..f(s, false, false).unwrap_or(0)]
    }

    #[test]
    fn measured_cases() {
        assert_eq!(head("Hello world"), "H");
        assert_eq!(head("\"Hello\" world"), "\"H");
        assert_eq!(head("  \"(Hello world"), "  \"(H");
        assert_eq!(head("\"  Hello"), "\"  H");
        assert_eq!(head("-Hello"), "-H");
        assert_eq!(head("3.14 pi"), "3.");
        assert_eq!(head("Hello\" world"), "H");
        assert_eq!(head("\"\" x"), "\"\" x");
        assert_eq!(head("\u{c9}a"), "\u{c9}");
        assert_eq!(head("\u{2026}Hello"), "\u{2026}H");
        assert_eq!(head("\u{bf}Qu\u{e9}?"), "\u{bf}Q");
        assert_eq!(head("$5 now"), "$");
        assert_eq!(head("$$5"), "$");
        assert_eq!(head("+5"), "+");
        assert_eq!(head("#hash"), "#h");
        assert_eq!(head("(a) b"), "(a)");
        assert_eq!(head("a. b"), "a.");
        assert_eq!(head("a  .  b"), "a");
        assert_eq!(head("a)) b"), "a))");
        assert_eq!(head("1,000"), "1,");
        assert_eq!(head("\"-\"A"), "\"-\"A");
        assert_eq!(head("_under"), "_u");
        assert_eq!(head("*star*"), "*s");
        assert_eq!(head("\u{a0}Hello"), "\u{a0}H");
        assert_eq!(head("e\u{301}a"), "e\u{301}");
        assert_eq!(head("\u{1f600} smile"), "\u{1f600}");
        assert_eq!(
            head("\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} family"),
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}"
        );
        assert_eq!(head("\u{1f1e9}\u{1f1ea} flag"), "\u{1f1e9}\u{1f1ea}");
        assert_eq!(head("\u{d55c}\u{ad6d}\u{c5b4}"), "\u{d55c}");
        assert_eq!(head("a\u{200b}b"), "a");
    }

    #[test]
    fn wpt_rules() {
        // css-pseudo-4 section 3.2 (WPT first-letter-trailing-punctuation, -004, -c0, -digraph)
        assert_eq!(head("(T)est"), "(T)");
        assert_eq!(head("T(rail"), "T");
        assert_eq!(head("T\u{2013}rail"), "T");
        assert_eq!(head("T\u{2003}.est"), "T\u{2003}.");
        assert_eq!(head("T.\u{2003}est"), "T.");
        assert_eq!(head("\u{ab}\u{a0}T\u{a0}\u{bb}est"), "\u{ab}\u{a0}T");
        assert_eq!(
            head("\u{104e}\u{300}T\u{300}\u{104e}\u{300}est"),
            "\u{104e}\u{300}T\u{300}\u{104e}\u{300}"
        );
        assert_eq!(head("\u{b}Test"), "\u{b}");
        assert_eq!(head("\u{19}Test"), "\u{19}");
        assert_eq!(f("IJsselmeer", false, true), Some(2));
        assert_eq!(f("ijsselmeer", false, true), Some(2));
        assert_eq!(f("IJsselmeer", false, false), Some(1));
        assert_eq!(f("Ijotalen", false, true), Some(1));
    }

    #[test]
    fn punctuation_only() {
        assert_eq!(f("\"", false, false), None);
        assert_eq!(f("\"", true, false), Some(1));
        assert_eq!(f("\"  ", true, false), None);
        assert_eq!(f("   ", true, false), None);
        assert_eq!(f("", true, false), None);
    }
}
