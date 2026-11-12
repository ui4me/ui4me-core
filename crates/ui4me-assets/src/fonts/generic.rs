//! owner: 007-resources (T061) — the single generic-family list and its mapping (FR-050, XR-046).
//!
//! 006's profile row and 009's matcher reference this list. Any other name (for example
//! `fangsong`) is an ordinary family name and is never mapped as a generic. The mapping and the
//! fallback list have Linux and Windows platform defaults, can be overridden through the runtime
//! configuration (IF-33), and are reported in diagnostics. The last-resort font is appended after
//! the fallback list (see `registry.rs`).

use ui4me_engine_types::text_metrics::GenericFamily;

/// The generic-family keywords of FR-050, in canonical order.
pub const KEYWORDS: &[(&str, GenericFamily)] = &[
    ("serif", GenericFamily::Serif),
    ("sans-serif", GenericFamily::SansSerif),
    ("monospace", GenericFamily::Monospace),
    ("cursive", GenericFamily::Cursive),
    ("fantasy", GenericFamily::Fantasy),
    ("system-ui", GenericFamily::SystemUi),
    ("ui-serif", GenericFamily::UiSerif),
    ("ui-sans-serif", GenericFamily::UiSansSerif),
    ("ui-monospace", GenericFamily::UiMonospace),
    ("ui-rounded", GenericFamily::UiRounded),
    ("emoji", GenericFamily::Emoji),
    ("math", GenericFamily::Math),
];

/// The generic keyword named by `name` (ASCII case-insensitive), or `None` for an ordinary family.
pub fn generic_keyword(name: &str) -> Option<GenericFamily> {
    KEYWORDS
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name.trim()))
        .map(|(_, g)| *g)
}

pub fn keyword_of(g: GenericFamily) -> &'static str {
    KEYWORDS
        .iter()
        .find(|(_, x)| *x == g)
        .map(|(k, _)| *k)
        .expect("every generic has a keyword")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
}

impl Platform {
    pub const fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Platform default family lists per generic.
pub fn default_generic_map(p: Platform) -> Vec<(GenericFamily, Vec<String>)> {
    use GenericFamily::*;
    let rows: Vec<(GenericFamily, Vec<&str>)> = match p {
        Platform::Linux => vec![
            (
                Serif,
                vec!["Noto Serif", "DejaVu Serif", "Liberation Serif"],
            ),
            (
                SansSerif,
                vec!["Noto Sans", "DejaVu Sans", "Liberation Sans"],
            ),
            (
                Monospace,
                vec!["Noto Sans Mono", "DejaVu Sans Mono", "Liberation Mono"],
            ),
            (Cursive, vec!["Comic Neue", "Z003", "DejaVu Serif"]),
            (Fantasy, vec!["Impact", "Noto Sans", "DejaVu Sans"]),
            (SystemUi, vec!["Cantarell", "Noto Sans", "DejaVu Sans"]),
            (UiSerif, vec!["Noto Serif", "DejaVu Serif"]),
            (UiSansSerif, vec!["Noto Sans", "DejaVu Sans"]),
            (UiMonospace, vec!["Noto Sans Mono", "DejaVu Sans Mono"]),
            (UiRounded, vec!["Noto Sans", "DejaVu Sans"]),
            (Emoji, vec!["Noto Color Emoji", "Noto Emoji"]),
            (
                Math,
                vec!["STIX Two Math", "DejaVu Math TeX Gyre", "Noto Sans Math"],
            ),
        ],
        Platform::Windows => vec![
            (Serif, vec!["Times New Roman", "Cambria"]),
            (SansSerif, vec!["Arial", "Segoe UI"]),
            (Monospace, vec!["Consolas", "Courier New"]),
            (Cursive, vec!["Comic Sans MS"]),
            (Fantasy, vec!["Impact"]),
            (SystemUi, vec!["Segoe UI"]),
            (UiSerif, vec!["Cambria", "Times New Roman"]),
            (UiSansSerif, vec!["Segoe UI", "Arial"]),
            (UiMonospace, vec!["Cascadia Mono", "Consolas"]),
            (UiRounded, vec!["Segoe UI"]),
            (Emoji, vec!["Segoe UI Emoji"]),
            (Math, vec!["Cambria Math"]),
        ],
    };
    rows.into_iter().map(|(g, v)| (g, names(&v))).collect()
}

/// Platform default fallback family list (before the last-resort font).
pub fn default_fallback(p: Platform) -> Vec<String> {
    match p {
        Platform::Linux => names(&["Noto Sans", "DejaVu Sans", "Liberation Sans"]),
        Platform::Windows => names(&["Segoe UI", "Arial"]),
    }
}

/// Platform defaults with `overrides` applied (an override replaces the whole list of its generic).
pub fn effective_generic_map(
    p: Platform,
    overrides: &[(GenericFamily, Vec<String>)],
) -> Vec<(GenericFamily, Vec<String>)> {
    let mut map = default_generic_map(p);
    for (g, fams) in overrides {
        match map.iter_mut().find(|(k, _)| k == g) {
            Some(slot) => slot.1 = fams.clone(),
            None => map.push((*g, fams.clone())),
        }
    }
    map.sort_by_key(|(g, _)| *g);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_exactly_the_twelve_keywords() {
        assert_eq!(KEYWORDS.len(), 12);
        assert_eq!(GenericFamily::ALL.len(), 12);
        for g in GenericFamily::ALL {
            assert_eq!(generic_keyword(keyword_of(*g)), Some(*g));
        }
        assert_eq!(
            generic_keyword("Sans-Serif"),
            Some(GenericFamily::SansSerif)
        );
        assert_eq!(generic_keyword("fangsong"), None, "an ordinary family name");
        assert_eq!(generic_keyword("inherit"), None);
        assert_eq!(generic_keyword("Arial"), None);
    }

    #[test]
    fn both_platforms_map_every_generic_and_have_a_fallback() {
        for p in [Platform::Linux, Platform::Windows] {
            let m = default_generic_map(p);
            assert_eq!(m.len(), 12);
            assert!(m.iter().all(|(_, v)| !v.is_empty()));
            assert!(!default_fallback(p).is_empty());
        }
    }

    #[test]
    fn overrides_replace_one_generic() {
        let m = effective_generic_map(
            Platform::Linux,
            &[(GenericFamily::Serif, vec!["My Serif".into()])],
        );
        assert_eq!(m.len(), 12);
        assert_eq!(
            m.iter()
                .find(|(g, _)| *g == GenericFamily::Serif)
                .unwrap()
                .1,
            vec!["My Serif".to_string()]
        );
        assert_eq!(
            m.iter()
                .find(|(g, _)| *g == GenericFamily::SansSerif)
                .unwrap()
                .1[0],
            "Noto Sans"
        );
    }
}
