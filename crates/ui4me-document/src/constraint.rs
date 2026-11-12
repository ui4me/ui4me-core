//! Bounded HTML constraint validation (F0605): the validity state of a form control computed on
//! demand from its attributes and current value facts. Nothing is stored; `:valid`/`:invalid`
//! styling keeps its own derivation. Messages are English and fixed (Core shows no native bubble).
//!
//! Supported: `required`, `minlength`/`maxlength` (dirty values only, UTF-16 units),
//! `pattern` (Rust `regex` syntax, anchored, size-limited; an unparsable pattern is ignored),
//! `type=email|url|number`, `min`/`max` (number, date/time kinds by fixed-width ISO comparison)
//! and `step` (number). Barred from validation: disabled, readonly, hidden/reset/button/image
//! inputs and controls inside a `datalist`.
use crate::control_semantics::{ControlKind, InputType};
use crate::control_state::validity as v;
use crate::document::Document;
use ui4me_engine_types::ids::NodeId;

/// The validity of one control. `flags` holds the `validity` bits (never `VALID`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConstraintValidity {
    pub flags: u16,
    pub message: String,
}

impl ConstraintValidity {
    pub fn valid(&self) -> bool {
        self.flags == 0
    }
    fn set(&mut self, bit: u16, message: impl Into<String>) {
        if self.flags == 0 {
            self.message = message.into();
        }
        self.flags |= bit;
    }
}

/// Stable names of the flag bits for the public JSON shape.
pub const FLAG_NAMES: [(&str, u16); 10] = [
    ("valueMissing", v::VALUE_MISSING),
    ("typeMismatch", v::TYPE_MISMATCH),
    ("patternMismatch", v::PATTERN_MISMATCH),
    ("tooLong", v::TOO_LONG),
    ("tooShort", v::TOO_SHORT),
    ("rangeUnderflow", v::RANGE_UNDERFLOW),
    ("rangeOverflow", v::RANGE_OVERFLOW),
    ("stepMismatch", v::STEP_MISMATCH),
    ("badInput", v::BAD_INPUT),
    ("customError", v::CUSTOM_ERROR),
];

const MAX_PATTERN_BYTES: usize = 1024;
const MAX_VALUE_BYTES: usize = 64 * 1024;
const REGEX_SIZE_LIMIT: usize = 256 * 1024;

impl Document {
    /// Is `el` a candidate for constraint validation (HTML "barred from constraint validation"
    /// is false)?
    pub fn will_validate(&self, el: NodeId) -> bool {
        let Some(facts) = self.control_facts(el) else {
            return false;
        };
        if facts.disabled {
            return false;
        }
        let candidate = match facts.kind {
            ControlKind::Input(t) => !matches!(
                t,
                InputType::Hidden | InputType::Reset | InputType::Button | InputType::Image
            ),
            ControlKind::Textarea | ControlKind::Select => true,
            ControlKind::Button => self
                .get_attribute(el, "type")
                .ok()
                .flatten()
                .is_none_or(|t| t.trim().eq_ignore_ascii_case("submit")),
            _ => false,
        };
        if !candidate {
            return false;
        }
        let text_like = matches!(facts.kind, ControlKind::Textarea)
            || matches!(facts.kind, ControlKind::Input(t) if t.accepts_text() || is_temporal(t));
        if text_like && facts.read_only {
            return false;
        }
        let mut a = self.parent(el).ok().flatten();
        while let Some(p) = a {
            if self.local_name(p).is_ok_and(|n| n == "datalist") {
                return false;
            }
            a = self.parent(p).ok().flatten();
        }
        true
    }

    /// The constraint validity of `el`; `None` when it is not a validation candidate.
    pub fn constraint_validity(&self, el: NodeId) -> Option<ConstraintValidity> {
        if !self.will_validate(el) {
            return None;
        }
        let kind = self.control_facts(el)?.kind;
        let attr = |n: &str| self.get_attribute(el, n).ok().flatten();
        let required = attr("required").is_some();
        let state = self.control_state(el).ok().flatten();
        let mut out = ConstraintValidity::default();
        if let Some(c) = state
            && !c.custom_validity_message.is_empty()
        {
            out.set(v::CUSTOM_ERROR, c.custom_validity_message.clone());
        }
        match kind {
            ControlKind::Select => {
                if required && self.select_value(el).is_empty() {
                    out.set(v::VALUE_MISSING, "Please select an item in the list.");
                }
            }
            ControlKind::Input(InputType::Checkbox) => {
                if required && state.and_then(|c| c.checkedness) != Some(true) {
                    out.set(
                        v::VALUE_MISSING,
                        "Please check this box if you want to proceed.",
                    );
                }
            }
            ControlKind::Input(InputType::Radio) => {
                let (any_required, any_checked) = self.radio_group_state(el);
                if any_required && !any_checked {
                    out.set(v::VALUE_MISSING, "Please select one of these options.");
                }
            }
            ControlKind::Textarea | ControlKind::Input(_) => {
                let ty = match kind {
                    ControlKind::Input(t) => t,
                    _ => InputType::Text,
                };
                let is_input = matches!(kind, ControlKind::Input(_));
                let value = self.control_value(el).unwrap_or_default();
                if matches!(ty, InputType::File | InputType::Range | InputType::Color)
                    || value.len() > MAX_VALUE_BYTES
                {
                    return Some(out);
                }
                if value.is_empty() {
                    if required {
                        out.set(v::VALUE_MISSING, "Please fill out this field.");
                    }
                    return Some(out);
                }
                let dirty = state.is_some_and(|c| c.dirty_value);
                let units = value.encode_utf16().count();
                if dirty && (ty.accepts_text() || !is_input) {
                    if let Some(max) =
                        attr("maxlength").and_then(|s| s.trim().parse::<usize>().ok())
                        && units > max
                    {
                        out.set(
                            v::TOO_LONG,
                            format!(
                                "Please shorten this text to {max} characters or less (you are currently using {units} characters)."
                            ),
                        );
                    }
                    if let Some(min) =
                        attr("minlength").and_then(|s| s.trim().parse::<usize>().ok())
                        && units < min
                    {
                        out.set(
                            v::TOO_SHORT,
                            format!(
                                "Please lengthen this text to {min} characters or more (you are currently using {units} characters)."
                            ),
                        );
                    }
                }
                match ty {
                    InputType::Email if !email_ok(&value, attr("multiple").is_some()) => {
                        out.set(v::TYPE_MISMATCH, "Please enter an email address.");
                    }
                    InputType::Url if !url_ok(&value) => {
                        out.set(v::TYPE_MISMATCH, "Please enter a URL.");
                    }
                    InputType::Number => self.check_number(el, &value, &mut out),
                    t if is_temporal(t) => {
                        if !temporal_ok(t, &value) {
                            out.set(v::BAD_INPUT, "Please enter a valid value.");
                        } else {
                            if let Some(min) = attr("min").filter(|m| temporal_ok(t, m))
                                && value.as_str() < min
                            {
                                out.set(
                                    v::RANGE_UNDERFLOW,
                                    format!("Value must be {min} or later."),
                                );
                            }
                            if let Some(max) = attr("max").filter(|m| temporal_ok(t, m))
                                && value.as_str() > max
                            {
                                out.set(
                                    v::RANGE_OVERFLOW,
                                    format!("Value must be {max} or earlier."),
                                );
                            }
                        }
                    }
                    _ => {}
                }
                if is_input
                    && matches!(
                        ty,
                        InputType::Text
                            | InputType::Search
                            | InputType::Telephone
                            | InputType::Url
                            | InputType::Email
                            | InputType::Password
                    )
                    && let Some(p) = attr("pattern")
                    && p.len() <= MAX_PATTERN_BYTES
                    && let Ok(re) = regex::RegexBuilder::new(&format!("^(?:{p})$"))
                        .size_limit(REGEX_SIZE_LIMIT)
                        .build()
                    && !re.is_match(&value)
                {
                    out.set(
                        v::PATTERN_MISMATCH,
                        match attr("title").filter(|t| !t.is_empty()) {
                            Some(t) => format!("Please match the requested format: {t}"),
                            None => "Please match the requested format.".to_owned(),
                        },
                    );
                }
            }
            _ => {}
        }
        Some(out)
    }

    fn check_number(&self, el: NodeId, value: &str, out: &mut ConstraintValidity) {
        let attr = |n: &str| self.get_attribute(el, n).ok().flatten();
        let Some(x) = parse_number(value) else {
            out.set(v::BAD_INPUT, "Please enter a number.");
            return;
        };
        let min = attr("min").and_then(parse_number);
        let max = attr("max").and_then(parse_number);
        if let Some(m) = min
            && x < m
        {
            out.set(
                v::RANGE_UNDERFLOW,
                format!(
                    "Value must be greater than or equal to {}.",
                    attr("min").unwrap_or("").trim()
                ),
            );
        }
        if let Some(m) = max
            && x > m
        {
            out.set(
                v::RANGE_OVERFLOW,
                format!(
                    "Value must be less than or equal to {}.",
                    attr("max").unwrap_or("").trim()
                ),
            );
        }
        let step = match attr("step").map(str::trim) {
            Some(s) if s.eq_ignore_ascii_case("any") => None,
            Some(s) => parse_number(s).filter(|s| *s > 0.0).or(Some(1.0)),
            None => Some(1.0),
        };
        if let Some(step) = step {
            let base = min
                .or_else(|| attr("value").and_then(parse_number))
                .unwrap_or(0.0);
            let q = (x - base) / step;
            if (q - q.round()).abs() > 1e-9 * q.abs().max(1.0) {
                out.set(v::STEP_MISMATCH, "Please enter a valid value.");
            }
        }
    }

    /// `(any member required, any member checked)` for the radio group of `el`: same non-empty
    /// `name`, same form owner, same tree.
    fn radio_group_state(&self, el: NodeId) -> (bool, bool) {
        let name = self.get_attribute(el, "name").ok().flatten().unwrap_or("");
        let own = |n: NodeId| self.arena().get(n).and_then(|x| x.as_element());
        let owner = own(el).and_then(|e| e.form_owner);
        if name.is_empty() {
            let required = own(el).is_some_and(|e| e.attr("required").is_some());
            let checked = self
                .control_state(el)
                .ok()
                .flatten()
                .and_then(|c| c.checkedness)
                == Some(true);
            return (required, checked);
        }
        let root = self.arena().root_of(el);
        let mut required = false;
        let mut checked = false;
        for n in self.arena().subtree(root) {
            let Some(e) = own(n) else { continue };
            if e.input_type() != Some(InputType::Radio)
                || e.attr("name") != Some(name)
                || e.form_owner != owner
            {
                continue;
            }
            required |= e.attr("required").is_some();
            checked |= e.control.as_ref().and_then(|c| c.checkedness) == Some(true);
        }
        (required, checked)
    }
}

fn is_temporal(t: InputType) -> bool {
    matches!(
        t,
        InputType::Date | InputType::Time | InputType::Month | InputType::DateTimeLocal
    )
}

fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s.len() > 64 {
        return None;
    }
    // HTML floating-point number: no leading '+', no words (inf/nan/hex), digits required.
    if s.starts_with('+') || !s.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    if !s
        .bytes()
        .all(|c| c.is_ascii_digit() || matches!(c, b'-' | b'.' | b'e' | b'E' | b'+'))
    {
        return None;
    }
    s.parse::<f64>().ok().filter(|x| x.is_finite())
}

fn email_ok(value: &str, multiple: bool) -> bool {
    if multiple {
        value.split(',').all(|p| email_one(p.trim()))
    } else {
        email_one(value)
    }
}

/// HTML's valid e-mail address production, hand-checked.
fn email_one(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    if local.is_empty()
        || !local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c))
    {
        return false;
    }
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// A valid absolute URL: a scheme, then a non-empty remainder, no whitespace or controls.
fn url_ok(s: &str) -> bool {
    if s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((scheme, rest)) = s.split_once(':') else {
        return false;
    };
    let mut cs = scheme.chars();
    cs.next().is_some_and(|c| c.is_ascii_alphabetic())
        && cs.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        && !rest.is_empty()
}

/// Fixed-width ISO shapes whose lexicographic order is chronological order.
fn temporal_ok(ty: InputType, s: &str) -> bool {
    let digits = |t: &str, n: usize| t.len() == n && t.bytes().all(|b| b.is_ascii_digit());
    let num = |t: &str| t.parse::<u32>().unwrap_or(0);
    let date = |t: &str| {
        let p: Vec<&str> = t.split('-').collect();
        p.len() == 3
            && digits(p[0], 4)
            && digits(p[1], 2)
            && digits(p[2], 2)
            && (1..=12).contains(&num(p[1]))
            && (1..=31).contains(&num(p[2]))
    };
    let time = |t: &str| {
        t.split_once(':')
            .is_some_and(|(h, m)| digits(h, 2) && digits(m, 2) && num(h) < 24 && num(m) < 60)
    };
    match ty {
        InputType::Date => date(s),
        InputType::Time => time(s),
        InputType::Month => s
            .split_once('-')
            .is_some_and(|(y, m)| digits(y, 4) && digits(m, 2) && (1..=12).contains(&num(m))),
        InputType::DateTimeLocal => s.split_once('T').is_some_and(|(d, t)| date(d) && time(t)),
        _ => true,
    }
}
