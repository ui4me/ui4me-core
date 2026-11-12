//! owner: 009-text-inline (T067, T069 text part; FR-038..FR-040; research R-10)
//!
//! `text-transform` as an owned preprocessing step on already white-space-processed text:
//! `uppercase`, `lowercase`, `capitalize`, `full-width` and `full-size-kana`, combined per the
//! CSS Text 3 grammar (`capitalize | uppercase | lowercase` plus the two flags). The DOM text is
//! never changed; the result carries a segment map from every source cluster to its transformed
//! bytes so that length-changing transforms (`ß` to `SS`) map back at grapheme granularity
//! (FR-040).
//!
//! * Case mapping is Rust's full Unicode mapping (`ß` to `SS`, final sigma), plus the owned
//!   Turkish/Azeri (dotted and dotless i) and Lithuanian (dot above) rules of Unicode
//!   SpecialCasing.
//! * Greek accent removal and Dutch `IJ` are **not** performed (FR-039, V1-LIMIT): applying
//!   `uppercase` or `capitalize` to `el` or `nl` text sets [`Transformed::unapplied_language`], and
//!   the caller reports `css.value.text-transform-language.<lang>` under 005's key.
//! * `capitalize` upper-cases the first letter of each word. Word starts follow a documented
//!   subset of UAX #29: a word is a run of letters, digits and `_`, joined across a single
//!   apostrophe, period, colon or middle dot between two letters; combining marks extend.
//! * Clusters are a base character plus following combining marks (the common mark blocks,
//!   ZWJ and variation selectors); that is the granularity of [`Segment`].
//! * `full-width` maps ASCII to the Fullwidth Forms block and a space to U+3000; half-width
//!   katakana and half-width Hangul are **not** mapped: [`Transformed::unmapped_halfwidth`] is
//!   set so that the caller reports it.

use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaseTransform {
    #[default]
    None,
    Capitalize,
    Uppercase,
    Lowercase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextTransform {
    pub case: CaseTransform,
    pub full_width: bool,
    pub full_size_kana: bool,
}

impl TextTransform {
    pub fn is_none(&self) -> bool {
        *self == TextTransform::default()
    }
}

/// One source cluster and the bytes it became.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub source: Range<usize>,
    pub output: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transformed {
    pub text: String,
    /// Contiguous, ascending, covering both strings.
    pub segments: Vec<Segment>,
    /// `Some("el" | "nl")` when `uppercase` or `capitalize` was applied to text of a language
    /// whose specific rule is not performed (FR-039); the caller reports it.
    pub unapplied_language: Option<&'static str>,
    /// `full-width` met half-width katakana or Hangul, which are not mapped.
    pub unmapped_halfwidth: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Other,
    Turkic,
    Lithuanian,
}

fn primary(lang: &str) -> String {
    lang.split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Combining marks that extend a cluster (common blocks, ZWJ, variation selectors).
fn extends(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8 | 0x06EA..=0x06ED
        | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
        | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200C..=0x200D | 0x20D0..=0x20FF | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F
        | 0x3099..=0x309A | 0xE0100..=0xE01EF)
}

/// Source clusters as byte ranges.
fn clusters(src: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for (i, c) in src.char_indices() {
        match out.last_mut() {
            Some(last) if extends(c) => last.end = i + c.len_utf8(),
            _ => out.push(i..i + c.len_utf8()),
        }
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_mid_letter(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '.' | ':' | '\u{00B7}' | '\u{2027}')
}

/// Soft-dotted letters for the Lithuanian uppercase rule (Unicode SpecialCasing, Soft_Dotted).
fn soft_dotted(c: char) -> bool {
    matches!(
        c,
        'i' | 'j' | '\u{012F}' | '\u{0249}' | '\u{0268}' | '\u{029D}' | '\u{02B2}'
    )
}

/// Combining class 230 (above) subset used by the Lithuanian More_Above condition.
fn above(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x0314 | 0x033D..=0x0344 | 0x0346 | 0x034A..=0x034C | 0x0350..=0x0352 | 0x0357 | 0x035B | 0x0363..=0x036F)
}

fn upper_cluster(cl: &str, lang: Lang, prev_soft_dotted: bool) -> String {
    let mut out = String::new();
    for (k, c) in cl.chars().enumerate() {
        match lang {
            Lang::Turkic if c == 'i' => out.push('\u{0130}'),
            // Lithuanian: a dot above after a soft-dotted letter is dropped when upper-casing.
            Lang::Lithuanian
                if c == '\u{0307}' && (prev_soft_dotted || cl.chars().take(k).any(soft_dotted)) => {
            }
            _ => out.extend(c.to_uppercase()),
        }
    }
    out
}

fn lower_cluster(cl: &str, lang: Lang) -> String {
    let chars: Vec<char> = cl.chars().collect();
    let mut out = String::new();
    let mut k = 0;
    while k < chars.len() {
        let c = chars[k];
        let next_above = chars.get(k + 1).copied().is_some_and(above);
        match lang {
            Lang::Turkic if c == 'I' => {
                if chars.get(k + 1) == Some(&'\u{0307}') {
                    out.push('i');
                    k += 1; // the dot above is absorbed
                } else {
                    out.push('\u{0131}');
                }
            }
            Lang::Turkic if c == '\u{0130}' => out.push('i'),
            Lang::Lithuanian if matches!(c, 'I' | 'J' | '\u{012E}') && next_above => {
                out.extend(c.to_lowercase());
                out.push('\u{0307}');
            }
            Lang::Lithuanian if c == '\u{00CC}' => out.push_str("i\u{0307}\u{0300}"),
            Lang::Lithuanian if c == '\u{00CD}' => out.push_str("i\u{0307}\u{0301}"),
            Lang::Lithuanian if c == '\u{0128}' => out.push_str("i\u{0307}\u{0303}"),
            _ => out.extend(c.to_lowercase()),
        }
        k += 1;
    }
    out
}

fn full_size_kana(c: char) -> char {
    match c as u32 {
        // Hiragana and Katakana small forms: the full-size form is the next code point.
        0x3041 | 0x3043 | 0x3045 | 0x3047 | 0x3049 | 0x3063 | 0x3083 | 0x3085 | 0x3087 | 0x308E
        | 0x30A1 | 0x30A3 | 0x30A5 | 0x30A7 | 0x30A9 | 0x30C3 | 0x30E3 | 0x30E5 | 0x30E7
        | 0x30EE => char::from_u32(c as u32 + 1).unwrap_or(c),
        0x3095 => '\u{304B}',
        0x3096 => '\u{3051}',
        0x30F5 => '\u{30AB}',
        0x30F6 => '\u{30B1}',
        // Katakana Phonetic Extensions.
        0x31F0 => '\u{30AF}',
        0x31F1 => '\u{30B7}',
        0x31F2 => '\u{30B9}',
        0x31F3 => '\u{30C8}',
        0x31F4 => '\u{30CC}',
        0x31F5 => '\u{30CF}',
        0x31F6 => '\u{30D2}',
        0x31F7 => '\u{30D5}',
        0x31F8 => '\u{30D8}',
        0x31F9 => '\u{30DB}',
        0x31FA => '\u{30E0}',
        0x31FB => '\u{30E9}',
        0x31FC => '\u{30EA}',
        0x31FD => '\u{30EB}',
        0x31FE => '\u{30EC}',
        0x31FF => '\u{30ED}',
        // Half-width small katakana.
        0xFF67 => '\u{FF71}',
        0xFF68 => '\u{FF72}',
        0xFF69 => '\u{FF73}',
        0xFF6A => '\u{FF74}',
        0xFF6B => '\u{FF75}',
        0xFF6C => '\u{FF94}',
        0xFF6D => '\u{FF95}',
        0xFF6E => '\u{FF96}',
        0xFF6F => '\u{FF82}',
        _ => c,
    }
}

/// `Some(mapped)` for characters `full-width` maps; half-width katakana and Hangul are
/// reported by the caller through `unmapped_halfwidth`.
fn full_width(c: char) -> char {
    match c as u32 {
        0x20 => '\u{3000}',
        0x21..=0x7E => char::from_u32(c as u32 - 0x21 + 0xFF01).unwrap_or(c),
        0xA2 => '\u{FFE0}',
        0xA3 => '\u{FFE1}',
        0xAC => '\u{FFE2}',
        0xAF => '\u{FFE3}',
        0xA6 => '\u{FFE4}',
        0xA5 => '\u{FFE5}',
        0x20A9 => '\u{FFE6}',
        _ => c,
    }
}

fn is_halfwidth_kana_or_hangul(c: char) -> bool {
    matches!(c as u32, 0xFF61..=0xFFDC)
}

/// Apply `t` to `src`. `lang` is the content language (BCP 47, may be empty). `starts_word`
/// tells `capitalize` whether the text begins a word (false when it continues a word started by
/// the previous text, across an element boundary).
pub fn transform(src: &str, t: TextTransform, lang: &str, starts_word: bool) -> Transformed {
    let p = primary(lang);
    let lang_rules = match p.as_str() {
        "tr" | "az" => Lang::Turkic,
        "lt" => Lang::Lithuanian,
        _ => Lang::Other,
    };
    let unapplied_language = match (t.case, p.as_str()) {
        (CaseTransform::Uppercase | CaseTransform::Capitalize, "el") => Some("el"),
        (CaseTransform::Uppercase | CaseTransform::Capitalize, "nl") => Some("nl"),
        _ => None,
    };
    let mut out = String::with_capacity(src.len());
    let mut segments = Vec::new();
    let mut unmapped_halfwidth = false;

    let cls = clusters(src);
    // Whole-string lowercase handles the final-sigma context; the per-cluster lengths agree
    // (sigma and final sigma are both two bytes), so the segment map uses per-cluster mappings.
    let lower_whole = (t.case == CaseTransform::Lowercase && lang_rules == Lang::Other)
        .then(|| src.to_lowercase());
    let mut lower_pos = 0usize;

    let mut in_word = !starts_word;
    let mut prev_soft_dotted = false;
    for (idx, r) in cls.iter().enumerate() {
        let cl = &src[r.clone()];
        let first = cl.chars().next().unwrap_or(' ');
        // Word start for capitalize.
        let next_first = cls.get(idx + 1).and_then(|n| src[n.clone()].chars().next());
        let word_char = is_word_char(first);
        let joins =
            is_mid_letter(first) && in_word && next_first.is_some_and(|n| n.is_alphabetic());
        let at_word_start = word_char && !in_word;

        let mut mapped = match t.case {
            CaseTransform::None => cl.to_string(),
            CaseTransform::Uppercase => upper_cluster(cl, lang_rules, prev_soft_dotted),
            CaseTransform::Lowercase => match &lower_whole {
                Some(whole) => {
                    // Take as many output bytes as the cluster's own lowercase occupies.
                    let n = cl
                        .chars()
                        .map(|c| c.to_lowercase().map(char::len_utf8).sum::<usize>())
                        .sum::<usize>();
                    let s = whole[lower_pos..lower_pos + n].to_string();
                    lower_pos += n;
                    s
                }
                None => lower_cluster(cl, lang_rules),
            },
            CaseTransform::Capitalize => {
                if at_word_start {
                    // Title case of the first letter, the rest of the cluster unchanged.
                    let mut cs = cl.chars();
                    let c0 = cs.next().unwrap_or(' ');
                    let head = match (lang_rules, c0) {
                        (Lang::Turkic, 'i') => "\u{0130}".to_string(),
                        _ => titlecase(c0),
                    };
                    head + cs.as_str()
                } else {
                    cl.to_string()
                }
            }
        };
        prev_soft_dotted = cl.chars().last().is_some_and(soft_dotted);
        if t.full_size_kana {
            mapped = mapped.chars().map(full_size_kana).collect();
        }
        if t.full_width {
            if mapped.chars().any(is_halfwidth_kana_or_hangul) {
                unmapped_halfwidth = true;
            }
            mapped = mapped.chars().map(full_width).collect();
        }
        in_word = word_char || joins || (extends(first) && in_word);
        let start = out.len();
        out.push_str(&mapped);
        segments.push(Segment {
            source: r.clone(),
            output: start..out.len(),
        });
    }
    Transformed {
        text: out,
        segments,
        unapplied_language,
        unmapped_halfwidth,
    }
}

/// Title-case mapping of one character as a string (digraph letters have a dedicated form;
/// everything else is its uppercase mapping, `ß` becomes `Ss`).
fn titlecase(c: char) -> String {
    match c {
        '\u{01C4}'..='\u{01C6}' => "\u{01C5}".into(),
        '\u{01C7}'..='\u{01C9}' => "\u{01C8}".into(),
        '\u{01CA}'..='\u{01CC}' => "\u{01CB}".into(),
        '\u{01F1}'..='\u{01F3}' => "\u{01F2}".into(),
        '\u{00DF}' => "Ss".into(),
        _ => c.to_uppercase().collect(),
    }
}

/// Map a byte offset of the transformed text back to the start of the source cluster that
/// produced it (FR-040): an offset inside a cluster's output snaps to the cluster start.
pub fn output_to_source(t: &Transformed, out_byte: usize) -> usize {
    match t.segments.iter().rev().find(|s| s.output.start <= out_byte) {
        Some(s)
            if out_byte >= s.output.end
                && s.output.end == t.text.len()
                && out_byte == t.text.len() =>
        {
            s.source.end
        }
        Some(s) => s.source.start,
        None => 0,
    }
}
