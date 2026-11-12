//! owner: 009-text-inline (015 TXT-3, lane L-HYPH)
//!
//! `hyphens: auto`: Liang's hyphenation algorithm over TeX pattern files (hyph-utf8), owned code with
//! no dependency. A pattern is letters with digits between them; an odd digit between two letters
//! allows a hyphen there, an even one forbids it, and the highest digit at a position wins. A word is
//! matched as `.word.` in lower case. Exceptions (`hyph-<lang>.hyp.txt`: words with `-` at the allowed
//! points) override the patterns.
//!
//! The pattern data is embedded text (`data/hyphenation`, licences in `COPYRIGHT_NOTICES.md`) and
//! parsed on first use of a language, once per process. Languages with no pattern file are reported
//! by the caller (`UNSUPPORTED_FEATURE`), never approximated with another language's patterns.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The languages that ship patterns: primary subtag, the regions whose patterns these are (`None` =
/// every region), the pattern file and the exception file.
struct Language {
    primary: &'static str,
    /// Only these region subtags (and no region at all) use the patterns; `None`: any region.
    regions: Option<&'static [&'static str]>,
    patterns: &'static str,
    exceptions: &'static str,
    /// The reference browser's dictionary for the language knows lower case only: a word with a capital
    /// letter is not hyphenated (measured: en-US; its de and fr dictionaries hyphenate capitals).
    lowercase_only: bool,
}

const LANGUAGES: [Language; 9] = [
    // American English patterns: other English regions spell and hyphenate differently and are not shipped.
    Language {
        primary: "en",
        regions: Some(&["us"]),
        patterns: include_str!("data/hyphenation/hyph-en-us.pat.txt"),
        exceptions: include_str!("data/hyphenation/hyph-en-us.hyp.txt"),
        lowercase_only: true,
    },
    // German reformed orthography (1996, 2006): de, de-DE, de-AT. Swiss spelling (no sharp s) is not shipped.
    Language {
        primary: "de",
        regions: Some(&["de", "at"]),
        patterns: include_str!("data/hyphenation/hyph-de-1996.pat.txt"),
        exceptions: "",
        lowercase_only: false,
    },
    Language {
        primary: "fr",
        regions: None,
        patterns: include_str!("data/hyphenation/hyph-fr.pat.txt"),
        exceptions: "",
        lowercase_only: false,
    },
    Language {
        primary: "es",
        regions: None,
        patterns: include_str!("data/hyphenation/hyph-es.pat.txt"),
        exceptions: "",
        lowercase_only: false,
    },
    // 0.9 F0905: Portuguese (BSD-3-Clause), Dutch, Slovak, Polish, Ukrainian (MIT); Czech is not
    // shipped (GPL-only patterns), see NOTICES.txt.
    Language {
        primary: "pt",
        regions: None,
        patterns: include_str!("data/hyphenation/hyph-pt.pat.txt"),
        exceptions: include_str!("data/hyphenation/hyph-pt.hyp.txt"),
        lowercase_only: false,
    },
    Language {
        primary: "nl",
        regions: None,
        patterns: include_str!("data/hyphenation/hyph-nl.pat.txt"),
        exceptions: include_str!("data/hyphenation/hyph-nl.hyp.txt"),
        lowercase_only: false,
    },
    Language {
        primary: "sk",
        regions: None,
        patterns: include_str!("data/hyphenation/hyph-sk.pat.txt"),
        exceptions: include_str!("data/hyphenation/hyph-sk.hyp.txt"),
        lowercase_only: false,
    },
    Language {
        primary: "pl",
        regions: None,
        patterns: include_str!("data/hyphenation/hyph-pl.pat.txt"),
        exceptions: include_str!("data/hyphenation/hyph-pl.hyp.txt"),
        lowercase_only: false,
    },
    Language {
        primary: "uk",
        regions: None,
        patterns: include_str!("data/hyphenation/hyph-uk.pat.txt"),
        exceptions: "",
        lowercase_only: false,
    },
];

static LOADED: [OnceLock<Hyphenator>; LANGUAGES.len()] =
    [const { OnceLock::new() }; LANGUAGES.len()];

/// What a language tag resolves to.
#[derive(Clone, Copy, Debug)]
pub enum Dictionary {
    /// The element has no `lang`: nothing to hyphenate with (as in the reference browser).
    NoLanguage,
    /// A language with no shipped patterns; the primary subtag (lower case) for the report.
    Unsupported,
    Supported(&'static Hyphenator),
}

/// The dictionary for a BCP 47 language tag (`en-US`, `de`, `fr-CA`). Loads the patterns on first use.
pub fn dictionary(lang: &str) -> Dictionary {
    let lang = lang.trim();
    if lang.is_empty() {
        return Dictionary::NoLanguage;
    }
    let lower = lang.to_ascii_lowercase().replace('_', "-");
    let mut parts = lower.split('-');
    let primary = parts.next().unwrap_or("");
    // The region is the first two-letter subtag after the language (and an optional script).
    let region = parts.find(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_alphabetic()));
    let Some(i) = LANGUAGES.iter().position(|l| l.primary == primary) else {
        return Dictionary::Unsupported;
    };
    let l = &LANGUAGES[i];
    let ok = match (l.regions, region) {
        (None, _) => true,
        // `en` alone is American English (the default of the reference browser's dictionary).
        (Some(_), None) => true,
        (Some(rs), Some(r)) => rs.contains(&r),
    };
    if !ok {
        return Dictionary::Unsupported;
    }
    Dictionary::Supported(LOADED[i].get_or_init(|| {
        Hyphenator::parse(l.patterns, l.exceptions).lowercase_only(l.lowercase_only)
    }))
}

/// The primary language subtag of `lang` in lower case (for reports).
pub fn primary_subtag(lang: &str) -> String {
    lang.trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// A parsed pattern set.
#[derive(Debug)]
pub struct Hyphenator {
    /// Letters of the pattern (digits removed) -> the digit before each letter and after the last.
    patterns: HashMap<Box<str>, Box<[u8]>>,
    exceptions: HashMap<Box<str>, Box<[usize]>>,
    max_len: usize,
    lowercase_only: bool,
}

impl Hyphenator {
    /// Parse a `hyph-*.pat.txt` file (one pattern per line) and a `.hyp.txt` file (one word per line,
    /// hyphens where a break is allowed).
    pub fn parse(patterns: &str, exceptions: &str) -> Hyphenator {
        let mut map: HashMap<Box<str>, Box<[u8]>> =
            HashMap::with_capacity(patterns.lines().count());
        let mut max_len = 0;
        for line in patterns.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('%') || line.starts_with('#') {
                continue;
            }
            let (mut letters, mut digits) = (String::new(), Vec::new());
            let mut pending = 0u8;
            let mut n = 0;
            for c in line.chars() {
                if let Some(d) = c.to_digit(10) {
                    pending = d as u8;
                } else {
                    digits.push(pending);
                    pending = 0;
                    letters.push(c);
                    n += 1;
                }
            }
            digits.push(pending);
            max_len = max_len.max(n);
            map.insert(letters.into_boxed_str(), digits.into_boxed_slice());
        }
        let mut ex = HashMap::new();
        for line in exceptions.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('%') || line.starts_with('#') {
                continue;
            }
            let (mut word, mut points, mut k) = (String::new(), Vec::new(), 0usize);
            for c in line.chars() {
                if c == '-' {
                    points.push(k);
                } else {
                    word.push(c);
                    k += 1;
                }
            }
            ex.insert(word.into_boxed_str(), points.into_boxed_slice());
        }
        Hyphenator {
            patterns: map,
            exceptions: ex,
            max_len,
            lowercase_only: false,
        }
    }

    /// Whether a word with a capital letter is left alone (see [`Language::lowercase_only`]).
    pub fn lowercase_only(mut self, on: bool) -> Hyphenator {
        self.lowercase_only = on;
        self
    }

    /// The number of patterns (a sanity measure for tests and reports).
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// The byte offsets in `word` (all letters) where a hyphen may be inserted, at least `before`
    /// letters from the start and `after` from the end. `word` is hyphenated case-insensitively.
    pub fn break_points(&self, word: &str, before: usize, after: usize) -> Vec<usize> {
        if self.lowercase_only && word.chars().any(char::is_uppercase) {
            return Vec::new();
        }
        let chars: Vec<(usize, char)> = word.char_indices().collect();
        let n = chars.len();
        if n < before + after || n < 2 {
            return Vec::new();
        }
        let lower: Vec<char> = chars
            .iter()
            .map(|&(_, c)| c.to_lowercase().next().unwrap_or(c))
            .collect();
        // points[i]: a hyphen is allowed before letter i (1 <= i < n) when odd
        let mut points = vec![0u8; n + 1];
        let key: String = lower.iter().collect();
        if let Some(ex) = self.exceptions.get(key.as_str()) {
            for &k in ex.iter() {
                if k < points.len() {
                    points[k] = 1;
                }
            }
        } else {
            let mut dotted = String::with_capacity(key.len() + 2);
            dotted.push('.');
            dotted.push_str(&key);
            dotted.push('.');
            // byte offsets of the chars of `dotted`, and one past the end
            let offs: Vec<usize> = dotted
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(dotted.len()))
                .collect();
            let len = offs.len() - 1; // chars in dotted = n + 2
            for s in 0..len {
                let top = (s + self.max_len).min(len);
                for e in s + 1..=top {
                    if let Some(d) = self.patterns.get(&dotted[offs[s]..offs[e]]) {
                        // digit j of the pattern sits before its letter j: dotted position s + j
                        for (j, &v) in d.iter().enumerate() {
                            let at = s + j; // position before dotted char `at`; dotted char 1 = word char 0
                            if at >= 1 && at - 1 < points.len() && v > points[at - 1] {
                                points[at - 1] = v;
                            }
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        for i in before.max(1)..=n.saturating_sub(after) {
            if i < n && points[i] % 2 == 1 {
                out.push(chars[i].0);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en() -> &'static Hyphenator {
        match dictionary("en-US") {
            Dictionary::Supported(h) => h,
            other => panic!("en-US must be supported: {other:?}"),
        }
    }

    fn hyphenate(h: &Hyphenator, word: &str, before: usize, after: usize) -> String {
        let mut out = String::new();
        let pts = h.break_points(word, before, after);
        for (i, c) in word.char_indices() {
            if pts.contains(&i) {
                out.push('-');
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn english_known_words() {
        let h = en();
        // with the limits of the reference browser (2 before, 3 after)
        for (word, want) in [
            ("hyphenation", "hy-phen-ation"),
            ("computer", "com-puter"),
            ("international", "in-ter-na-tional"),
            ("typography", "ty-pog-ra-phy"),
            ("beautiful", "beau-ti-ful"),
            ("communication", "com-mu-ni-ca-tion"),
            ("algorithm", "al-go-rithm"),
        ] {
            assert_eq!(hyphenate(h, word, 2, 3), want, "{word}");
        }
        // every point the patterns have (limits of one letter)
        assert_eq!(hyphenate(h, "computer", 1, 1), "com-put-er");
    }

    #[test]
    fn added_languages_hyphenate_known_words() {
        // 0.9 F0905: one word per added pattern set, at the limits 2/2; the points are the
        // patterns' (checked against the hyph-utf8 files), not a dictionary's.
        let get = |tag: &str| match dictionary(tag) {
            Dictionary::Supported(h) => h,
            other => panic!("{tag} must be supported: {other:?}"),
        };
        for (tag, word) in [
            ("pt-BR", "computador"),
            ("nl", "woordenboek"),
            ("sk", "počítač"),
            ("pl", "komputer"),
            ("uk", "комп'ютер"),
        ] {
            let out = hyphenate(get(tag), word, 2, 2);
            assert!(out.contains('-'), "{tag}: {word} has no break point");
            assert_eq!(out.replace('-', ""), word);
        }
        assert!(
            !matches!(dictionary("cs"), Dictionary::Supported(_)),
            "Czech is not shipped"
        );
    }

    #[test]
    fn limits_trim_the_ends() {
        let h = en();
        assert_eq!(hyphenate(h, "hyphenation", 2, 2), "hy-phen-ation");
        assert_eq!(hyphenate(h, "hyphenation", 3, 2), "hyphen-ation");
        assert_eq!(hyphenate(h, "hyphenation", 3, 4), "hyphen-ation");
        assert_eq!(hyphenate(h, "hyphenation", 3, 6), "hyphenation");
        assert!(h.break_points("it", 1, 1).is_empty());
    }

    #[test]
    fn case_is_ignored_and_exceptions_win() {
        let h = en();
        // the reference browser's English dictionary leaves a word with a capital letter alone
        assert_eq!(hyphenate(h, "Hyphenation", 2, 2), "Hyphenation");
        assert_eq!(hyphenate(h, "HYPHENATION", 2, 2), "HYPHENATION");
        // exception list: "project" never breaks, "as-so-ciate" breaks where listed
        assert_eq!(hyphenate(h, "project", 1, 1), "project");
        assert_eq!(hyphenate(h, "associate", 1, 1), "as-so-ciate");
    }

    #[test]
    fn german_french_spanish() {
        let get = |l: &str| match dictionary(l) {
            Dictionary::Supported(h) => h,
            other => panic!("{l}: {other:?}"),
        };
        assert_eq!(
            hyphenate(get("de"), "Donaudampfschifffahrtsgesellschaft", 2, 2),
            "Do-nau-dampf-schiff-fahrts-ge-sell-schaft"
        );
        assert_eq!(
            hyphenate(get("de-AT"), "Rechtsschutzversicherung", 2, 2),
            "Rechts-schutz-ver-si-che-rung"
        );
        assert_eq!(
            hyphenate(get("fr-CA"), "extraordinairement", 2, 2),
            "ex-tra-or-di-nai-re-ment"
        );
        assert_eq!(hyphenate(get("es"), "ferrocarril", 2, 2), "fe-rro-ca-rril");
    }

    #[test]
    fn language_resolution() {
        assert!(matches!(dictionary(""), Dictionary::NoLanguage));
        assert!(matches!(dictionary("  "), Dictionary::NoLanguage));
        assert!(matches!(dictionary("en"), Dictionary::Supported(_)));
        assert!(matches!(dictionary("EN_us"), Dictionary::Supported(_)));
        assert!(matches!(dictionary("en-Latn-US"), Dictionary::Supported(_)));
        assert!(matches!(dictionary("en-GB"), Dictionary::Unsupported));
        assert!(matches!(dictionary("de-CH"), Dictionary::Unsupported));
        assert!(matches!(dictionary("fr-BE"), Dictionary::Supported(_)));
        assert!(matches!(dictionary("it"), Dictionary::Unsupported));
        assert!(matches!(dictionary("ja"), Dictionary::Unsupported));
        assert_eq!(primary_subtag("DE-at"), "de");
    }

    #[test]
    fn multibyte_offsets_are_byte_offsets() {
        let h = match dictionary("de") {
            Dictionary::Supported(h) => h,
            _ => unreachable!(),
        };
        let word = "Kapitänsmützenfabrikant";
        for p in h.break_points(word, 2, 2) {
            assert!(word.is_char_boundary(p), "{p}");
        }
        assert!(!h.break_points(word, 2, 2).is_empty());
    }

    #[test]
    fn pattern_sets_load_lazily_and_are_sized() {
        assert!(en().pattern_count() > 4000);
    }
}
