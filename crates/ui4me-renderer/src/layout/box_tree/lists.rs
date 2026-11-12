//! owner: 008-layout-engine (lane L-LIST-r9) - marker text (FR-083, FR-084; T114).
//!
//! The predefined counter styles of CSS Counter Styles 3 that FR-083 lists, rendered as the
//! reference browser renders them, and the suffix each one gets in a list marker. Every other
//! predefined counter-style name renders as `decimal` and is reported (FR-084, R-20).

use crate::style::view::generated::CounterStyleRef;

/// How a counter style renders, for marker construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rendered {
    /// A bullet-like symbol (`disc`, `circle`, `square`, `disclosure-*`): independent of the value.
    Symbol(&'static str),
    /// A numeric or alphabetic representation of the value.
    Text(String),
    /// A `<string>` style: the string itself, no suffix.
    Literal(String),
    /// `none`: no marker.
    Nothing,
}

/// The name a counter style falls back to when it is outside FR-083 (`Some(name)` = report it).
pub fn unsupported_name(style: &CounterStyleRef) -> Option<String> {
    match style {
        CounterStyleRef::Name(n) if !is_known(n) => Some(n.clone()),
        CounterStyleRef::Symbols => Some("symbols".to_string()),
        _ => None,
    }
}

/// The predefined names FR-083 renders.
pub fn is_known(name: &str) -> bool {
    matches!(
        name,
        "none"
            | "disc"
            | "circle"
            | "square"
            | "decimal"
            | "decimal-leading-zero"
            | "lower-roman"
            | "upper-roman"
            | "lower-alpha"
            | "upper-alpha"
            | "lower-latin"
            | "upper-latin"
            | "lower-greek"
            | "disclosure-open"
            | "disclosure-closed"
    )
}

/// Render `value` in `style`; an unknown name renders as `decimal`.
pub fn render(style: &CounterStyleRef, value: i32) -> Rendered {
    match style {
        CounterStyleRef::None => Rendered::Nothing,
        CounterStyleRef::String(s) => Rendered::Literal(s.clone()),
        CounterStyleRef::Symbols => Rendered::Text(value.to_string()),
        CounterStyleRef::Name(n) => match n.as_str() {
            "none" => Rendered::Nothing,
            "disc" => Rendered::Symbol("\u{2022}"),
            "circle" => Rendered::Symbol("\u{25E6}"),
            "square" => Rendered::Symbol("\u{25AA}"),
            "disclosure-open" => Rendered::Symbol("\u{25BE}"),
            "disclosure-closed" => Rendered::Symbol("\u{25B8}"),
            "decimal-leading-zero" => Rendered::Text(decimal_leading_zero(value)),
            "lower-roman" => {
                Rendered::Text(roman(value, false).unwrap_or_else(|| value.to_string()))
            }
            "upper-roman" => {
                Rendered::Text(roman(value, true).unwrap_or_else(|| value.to_string()))
            }
            "lower-alpha" | "lower-latin" => {
                Rendered::Text(alphabetic(value, &LATIN_LOWER).unwrap_or_else(|| value.to_string()))
            }
            "upper-alpha" | "upper-latin" => {
                Rendered::Text(alphabetic(value, &LATIN_UPPER).unwrap_or_else(|| value.to_string()))
            }
            "lower-greek" => {
                Rendered::Text(alphabetic(value, &GREEK_LOWER).unwrap_or_else(|| value.to_string()))
            }
            _ => Rendered::Text(value.to_string()),
        },
    }
}

/// The text of a `counter()` / `counters()` use: the representation without a suffix.
pub fn counter_text(style: &CounterStyleRef, value: i32) -> String {
    match render(style, value) {
        Rendered::Symbol(s) => s.to_string(),
        Rendered::Text(t) | Rendered::Literal(t) => t,
        Rendered::Nothing => String::new(),
    }
}

/// The marker text of a list item (CSS Lists 3 section 3.2): the representation plus the
/// counter style's suffix (`". "` for the numeric and alphabetic styles, a space for the
/// symbols; none for a `<string>`). `outside` uses a no-break space so that the marker never
/// wraps away from the item (the marker sits outside the line, as in the reference browser).
pub fn marker_text(style: &CounterStyleRef, value: i32, outside: bool) -> Option<String> {
    let sp = if outside { '\u{00A0}' } else { ' ' };
    match render(style, value) {
        Rendered::Nothing => None,
        Rendered::Literal(s) => Some(s),
        Rendered::Symbol(s) => Some(format!("{s}{sp}")),
        Rendered::Text(t) => Some(format!("{t}.{sp}")),
    }
}

/// `pad: 2 "0"` counts the sign: `-3` is already two characters (the reference browser's `-3`).
fn decimal_leading_zero(value: i32) -> String {
    if (0..=9).contains(&value) {
        format!("0{value}")
    } else {
        value.to_string()
    }
}

const LATIN_LOWER: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];
const LATIN_UPPER: [char; 26] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];
const GREEK_LOWER: [char; 24] = [
    '\u{03B1}', '\u{03B2}', '\u{03B3}', '\u{03B4}', '\u{03B5}', '\u{03B6}', '\u{03B7}', '\u{03B8}',
    '\u{03B9}', '\u{03BA}', '\u{03BB}', '\u{03BC}', '\u{03BD}', '\u{03BE}', '\u{03BF}', '\u{03C0}',
    '\u{03C1}', '\u{03C3}', '\u{03C4}', '\u{03C5}', '\u{03C6}', '\u{03C7}', '\u{03C8}', '\u{03C9}',
];

/// The `alphabetic` counter system (CSS Counter Styles 3 section 3.3): bijective base-N from 1;
/// values below 1 fall back to decimal.
fn alphabetic(value: i32, symbols: &[char]) -> Option<String> {
    if value < 1 {
        return None;
    }
    let n = symbols.len() as i64;
    let mut v = i64::from(value);
    let mut out = Vec::new();
    while v > 0 {
        v -= 1;
        out.push(symbols[(v % n) as usize]);
        v /= n;
    }
    out.reverse();
    Some(out.into_iter().collect())
}

/// Roman numerals for 1..=3999 (CSS Counter Styles 3 `upper-roman` / `lower-roman` range).
fn roman(value: i32, upper: bool) -> Option<String> {
    if !(1..=3999).contains(&value) {
        return None;
    }
    const TABLE: [(i32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut v = value;
    let mut out = String::new();
    for (n, s) in TABLE {
        while v >= n {
            out.push_str(s);
            v -= n;
        }
    }
    Some(if upper { out } else { out.to_lowercase() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(n: &str) -> CounterStyleRef {
        CounterStyleRef::Name(n.to_string())
    }

    #[test]
    fn numeric_alphabetic_and_roman() {
        assert_eq!(counter_text(&name("decimal"), -3), "-3");
        assert_eq!(counter_text(&name("decimal-leading-zero"), 7), "07");
        assert_eq!(counter_text(&name("decimal-leading-zero"), 12), "12");
        assert_eq!(counter_text(&name("decimal-leading-zero"), -3), "-3");
        assert_eq!(counter_text(&name("lower-alpha"), 1), "a");
        assert_eq!(counter_text(&name("upper-latin"), 27), "AA");
        assert_eq!(counter_text(&name("lower-alpha"), 0), "0");
        assert_eq!(counter_text(&name("lower-roman"), 14), "xiv");
        assert_eq!(counter_text(&name("upper-roman"), 1994), "MCMXCIV");
        assert_eq!(counter_text(&name("upper-roman"), 4000), "4000");
        assert_eq!(counter_text(&name("lower-greek"), 3), "\u{03B3}");
        assert_eq!(counter_text(&name("lower-greek"), 25), "\u{03B1}\u{03B1}");
    }

    #[test]
    fn marker_text_has_the_suffix() {
        assert_eq!(
            marker_text(&name("decimal"), 5, false).as_deref(),
            Some("5. ")
        );
        assert_eq!(
            marker_text(&name("disc"), 1, false).as_deref(),
            Some("\u{2022} ")
        );
        assert_eq!(
            marker_text(&name("disc"), 1, true).as_deref(),
            Some("\u{2022}\u{A0}")
        );
        assert_eq!(
            marker_text(&CounterStyleRef::String("-> ".into()), 1, true).as_deref(),
            Some("-> ")
        );
        assert_eq!(marker_text(&name("none"), 1, true), None);
    }

    #[test]
    fn unknown_names_are_reported_and_render_as_decimal() {
        assert_eq!(
            unsupported_name(&name("cjk-decimal")).as_deref(),
            Some("cjk-decimal")
        );
        assert_eq!(unsupported_name(&name("lower-roman")), None);
        assert_eq!(counter_text(&name("cjk-decimal"), 4), "4");
    }
}
