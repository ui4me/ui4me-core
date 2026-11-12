//! owner: 009-text-inline (015 TXT-3, lane L-HYPH)
//!
//! Hyphenation of a paragraph: where `hyphens: auto` allows a break inside a word, where
//! `hyphens: none` forbids the break after a soft hyphen, and the shaped hyphen glyph.
//!
//! The break points are handed to Parley as line break opportunities (`LineBreakOverrideFn`), so the
//! breaker needs no other change; the snap table (`snap.rs`) then classifies the opportunities that
//! are hyphenation points, adds the hyphen's width when a line ends at one, and the frozen
//! paragraph draws the hyphen after the last glyph of the line (it is not text: no DOM offset).
//!
//! Limits of the reference browser's `hyphenate-limit-chars: auto` (measured, 48 words in en, de, fr, es):
//! a word of at least 5 letters (2 before the hyphen and 3 after it make 5), at least 2 letters before
//! the hyphen and 3 after it. A word with a digit is left alone. The last word of the paragraph is not
//! hyphenated (measured: "I think wonderful" in 124-140 px stays whole, "I think wonderful x" does not), nor is a
//! word an atomic inline follows directly: the reference hyphenates a word only where a break opportunity follows it.
//! A word with soft hyphens is hyphenated as the word without them, besides the author's own points.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use parley::LineBreakContext;

use super::source::{HyphensKind, ParagraphSource, SynthKind, SynthRun, TextStyle};
use crate::text::fonts::TextServices;
use crate::text::hyphenation::{self, Dictionary};

/// `hyphenate-limit-chars: auto` of the reference browser: shortest word, letters before the hyphen,
/// letters after it.
pub const MIN_WORD: usize = 5;
pub const MIN_BEFORE: usize = 2;
pub const MIN_AFTER: usize = 3;

/// U+00AD SOFT HYPHEN.
pub const SOFT_HYPHEN: char = '\u{ad}';

/// The hyphenation facts of one paragraph, byte offsets of the processed text.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HyphenPlan {
    /// Break points inside words (`hyphens: auto`), ascending.
    pub auto: Vec<usize>,
    /// Break points of the words that no break opportunity follows (the last word of the paragraph, a word an
    /// atomic inline follows), with the byte where their word starts: a line takes one only when it starts at
    /// that word, i.e. when the word does not fit a line of its own (measured); the min-content size counts them all.
    pub auto_final: Vec<(usize, usize)>,
    /// Offsets just after a soft hyphen in a `hyphens: none` span: no break opportunity there.
    pub none_soft: Vec<usize>,
    /// Spans with `hyphens: auto` that cannot be hyphenated, once per span, with the reason.
    pub unsupported: Vec<(u32, HyphenUnsupported)>,
}

/// Why `hyphens: auto` does nothing for a span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HyphenUnsupported {
    /// The element has no `lang` (the reference browser does not hyphenate then either).
    NoLanguage,
    /// No patterns for the language.
    Language,
    /// The paragraph is bidirectional: the hyphen is not placed in the bidi path.
    Bidi,
}

fn is_word_char(c: char) -> bool {
    // (a soft hyphen is part of its word: a word that holds one is left to its author, measured)
    c.is_alphanumeric() || c == SOFT_HYPHEN || matches!(c as u32, 0x300..=0x36f)
}

/// The plan for `source`, whose text is `text` (the same bytes: tabs are replaced by spaces).
pub(crate) fn plan(source: &ParagraphSource, text: &str) -> HyphenPlan {
    let mut out = HyphenPlan::default();
    if !source
        .spans
        .iter()
        .any(|s| s.props.hyphens != HyphensKind::Manual)
    {
        return out;
    }
    if source.is_bidi() {
        for (i, s) in source.spans.iter().enumerate() {
            if s.props.hyphens == HyphensKind::Auto {
                out.unsupported.push((i as u32, HyphenUnsupported::Bidi));
            }
        }
        return out;
    }
    let span_at = |byte: usize| {
        source
            .spans
            .iter()
            .position(|s| s.range.start <= byte && byte < s.range.end)
    };
    for s in &source.spans {
        if s.props.hyphens == HyphensKind::None
            && let Some(t) = text.get(s.range.clone())
        {
            for (i, c) in t.char_indices() {
                if c == SOFT_HYPHEN {
                    out.none_soft.push(s.range.start + i + c.len_utf8());
                }
            }
        }
    }
    let mut reported: Vec<u32> = Vec::new();
    let mut word_start: Option<usize> = None;
    // The last token of the text (after the last collapsible white space) has no break opportunity after it.
    let final_token = text
        .char_indices()
        .rev()
        .find(|&(_, c)| c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}'))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let atomic_at = |byte: usize| {
        source.items.iter().any(|it| {
            it.kind == super::source::InlineItemKind::AtomicInline && it.range.start == byte
        })
    };
    let mut flush = |ws: usize, we: usize, out: &mut HyphenPlan| {
        let Some(si) = span_at(ws) else { return };
        let span = &source.spans[si];
        // `word-break: break-all` and `line-break: anywhere` break between any two letters: no hyphen is used
        if span.props.hyphens != HyphensKind::Auto
            || span.props.word_break == super::source::WordBreakKind::BreakAll
            || span.props.line_break == super::source::LineBreakKind::Anywhere
        {
            return;
        }
        let raw = &text[ws..we];
        // soft hyphens are no letters of the word: the patterns see the word without them (measured: the reference
        // hyphenates "extra<shy>ordinary" at the points of "extraordinary" besides the soft hyphen)
        let clean: std::borrow::Cow<'_, str> = if raw.contains(SOFT_HYPHEN) {
            raw.replace(SOFT_HYPHEN, "").into()
        } else {
            raw.into()
        };
        let word: &str = &clean;
        if word.chars().count() < MIN_WORD || word.chars().any(char::is_numeric) {
            return;
        }
        // (a word that starts a text item - a span of its own, text after an inline box - is hyphenated like any other:
        // measured, "ture <span>internationalization</span>" breaks the span's word in mid-line)
        let no_opportunity_after = (ws >= final_token || atomic_at(we)) && ws > span.range.start;
        match hyphenation::dictionary(&span.style.font.lang) {
            Dictionary::Supported(h) => {
                for p in h.break_points(word, MIN_BEFORE, MIN_AFTER) {
                    // back to the byte of `raw`: past the soft hyphens that came before the point's letter, and no
                    // point where a soft hyphen already is (the break after it is a manual one)
                    let at = if clean.len() == raw.len() {
                        ws + p
                    } else {
                        let mut kept = 0usize;
                        let mut found = None;
                        for (i, c) in raw.char_indices() {
                            if c == SOFT_HYPHEN {
                                continue;
                            }
                            if kept == p {
                                found = if raw[..i].ends_with(SOFT_HYPHEN) {
                                    None
                                } else {
                                    Some(ws + i)
                                };
                                break;
                            }
                            kept += c.len_utf8();
                        }
                        match found {
                            Some(b) => b,
                            None => continue,
                        }
                    };
                    // the break is allowed where the text it falls in is `hyphens: auto`
                    if span_at(at)
                        .is_some_and(|k| source.spans[k].props.hyphens == HyphensKind::Auto)
                    {
                        if no_opportunity_after {
                            out.auto_final.push((at, ws));
                        } else {
                            out.auto.push(at);
                        }
                    }
                }
            }
            other => {
                if !reported.contains(&(si as u32)) {
                    reported.push(si as u32);
                    out.unsupported.push((
                        si as u32,
                        if matches!(other, Dictionary::NoLanguage) {
                            HyphenUnsupported::NoLanguage
                        } else {
                            HyphenUnsupported::Language
                        },
                    ));
                }
            }
        }
    };
    for (i, c) in text.char_indices() {
        if is_word_char(c) {
            word_start.get_or_insert(i);
        } else if let Some(ws) = word_start.take() {
            flush(ws, i, &mut out);
        }
    }
    if let Some(ws) = word_start {
        flush(ws, text.len(), &mut out);
    }
    out.auto.sort_unstable();
    out.auto.dedup();
    out.auto_final.sort_unstable();
    out.auto_final.dedup();
    out
}

/// Parley's line break override for a plan: `Some(true)` before the char at each auto point,
/// `Some(false)` after a soft hyphen of a `hyphens: none` span. Parley asks once per pair of
/// adjacent characters in text order, except for pairs that touch a mandatory break; the position is
/// tracked here, and the characters Parley passes are checked against the text (a mismatch answers
/// `None`: the default opportunity).
pub(crate) struct BreakOverride {
    chars: Vec<char>,
    force: HashMap<usize, bool>,
    next: AtomicUsize,
}

fn is_mandatory(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

impl BreakOverride {
    pub fn new(text: &str, plan: &HyphenPlan) -> BreakOverride {
        let chars: Vec<char> = text.chars().collect();
        let mut index_of: HashMap<usize, usize> = HashMap::new();
        for (i, (b, _)) in text.char_indices().enumerate() {
            index_of.insert(b, i);
        }
        let mut force = HashMap::new();
        for &b in &plan.none_soft {
            if let Some(&i) = index_of.get(&b) {
                force.insert(i, false);
            }
        }
        for &b in plan
            .auto
            .iter()
            .chain(plan.auto_final.iter().map(|(b, _)| b))
        {
            if let Some(&i) = index_of.get(&b) {
                force.insert(i, true);
            }
        }
        BreakOverride {
            chars,
            force,
            next: AtomicUsize::new(1),
        }
    }

    pub fn call(&self, cx: LineBreakContext) -> Option<bool> {
        let mut m = self.next.load(Ordering::Relaxed);
        while m < self.chars.len()
            && (is_mandatory(self.chars[m]) || is_mandatory(self.chars[m - 1]))
        {
            m += 1;
        }
        self.next.store(m + 1, Ordering::Relaxed);
        if m >= self.chars.len() || self.chars[m] != cx.after || self.chars[m - 1] != cx.before {
            return None;
        }
        self.force.get(&m).copied()
    }
}

impl TextServices {
    /// The hyphen of a span: `hyphenate-character` when given, else U+2010 and, when the font has no
    /// glyph for it, U+002D (CSS Text 4 section 6.2), shaped with the span's style.
    pub(crate) fn shape_hyphen(
        &mut self,
        style: &TextStyle,
        custom: Option<&str>,
        at: usize,
    ) -> SynthRun {
        let candidates: Vec<String> = match custom {
            // an empty string draws no hyphen and takes no room
            Some(c) => vec![c.to_string()],
            None => vec!["\u{2010}".into(), "-".into()],
        };
        self.shape_synth_candidates(SynthKind::Hyphen, style, candidates, at)
    }
}
