//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! RFC 8785 (JCS) canonical JSON writer (diagnostics-schema-v0.md §4 "Canonical byte form"):
//! members sorted by UTF-16 code units, no insignificant whitespace, UTF-8, ECMAScript
//! shortest round-trip numbers; arrays keep their defined order.
//! Slice writer by 001 T051; the strict API (range and finiteness checks, exact layout values)
//! by 005 T017.

use serde_json::{Map, Number, Value};

/// The JSON crate this export is built on, for consumers without their own dependency on it
/// (the runtime's tests and the FFI read exports as `serde_json::Value`).
pub use serde_json;

/// The member name whose subtree is excluded from byte-identity (FR-057), at any depth.
pub const NONDETERMINISTIC: &str = "nondeterministic";

/// RFC 8785 canonical bytes of `value`.
pub fn to_canonical_bytes(value: &Value) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    write_value(&mut out, value);
    out
}

/// `value` with every member named [`NONDETERMINISTIC`] removed, at any depth.
pub fn strip_nondeterministic(value: &Value) -> Value {
    match value {
        Value::Object(o) => {
            let mut m = Map::new();
            for (k, v) in o {
                if k != NONDETERMINISTIC {
                    m.insert(k.clone(), strip_nondeterministic(v));
                }
            }
            Value::Object(m)
        }
        Value::Array(a) => Value::Array(a.iter().map(strip_nondeterministic).collect()),
        other => other.clone(),
    }
}

/// The bytes the FR-057 byte-identity guarantee applies to:
/// `to_canonical_bytes(&strip_nondeterministic(value))`.
pub fn deterministic_bytes(value: &Value) -> Vec<u8> {
    to_canonical_bytes(&strip_nondeterministic(value))
}

/// Why a number cannot be written (schema §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalError {
    /// NaN or an infinity: never emitted by an export.
    NonFinite,
    /// `|v| >= 1e21`, where the ECMAScript form leaves the plain-decimal range the schema assumes.
    OutOfRange,
}

/// A JSON number for `v`: rejects non-finite values and magnitudes of `1e21` or more.
pub fn finite_number(v: f64) -> Result<Value, CanonicalError> {
    if !v.is_finite() {
        return Err(CanonicalError::NonFinite);
    }
    if v.abs() >= 1e21 {
        return Err(CanonicalError::OutOfRange);
    }
    // Exact integers take integer form so equal values serialise identically.
    if v == v.trunc() && v.abs() < 9.0e15 {
        return Ok(Value::from(v as i64));
    }
    Ok(Number::from_f64(v).map_or(Value::Null, Value::Number))
}

/// A layout value as its exact `n/64` decimal (LayoutUnit is a 1/64 fixed-point type, so the
/// quotient is a terminating binary fraction that `f64` represents exactly).
pub fn layout_unit_number(
    v: ui4me_engine_types::geometry::layout_unit::LayoutUnit,
) -> Result<Value, CanonicalError> {
    finite_number(v.to_f64())
}

/// [`to_canonical_bytes`] after checking every number is finite and below `1e21` in magnitude.
pub fn try_to_canonical_bytes(value: &Value) -> Result<Vec<u8>, CanonicalError> {
    fn check(v: &Value) -> Result<(), CanonicalError> {
        match v {
            Value::Number(n) => match n.as_f64() {
                Some(f) => finite_number(f).map(|_| ()),
                None => Ok(()),
            },
            Value::Array(a) => a.iter().try_for_each(check),
            Value::Object(o) => o.values().try_for_each(check),
            _ => Ok(()),
        }
    }
    check(value)?;
    Ok(to_canonical_bytes(value))
}

fn write_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_string(out, s),
        Value::Array(a) => {
            out.push(b'[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_value(out, v);
            }
            out.push(b']');
        }
        Value::Object(o) => {
            // RFC 8785 §3.2.3: sort by the UTF-16 code units of the member names.
            let mut keys: Vec<(Vec<u16>, &String, &Value)> = o
                .iter()
                .map(|(k, v)| (k.encode_utf16().collect(), k, v))
                .collect();
            keys.sort_by(|a, b| a.0.cmp(&b.0));
            out.push(b'{');
            for (i, (_, k, v)) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(out, k);
                out.push(b':');
                write_value(out, v);
            }
            out.push(b'}');
        }
    }
}

/// RFC 8785 §3.2.2.2: only `"`, `\` and C0 controls are escaped; short forms where JSON has them.
fn write_string(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    for ch in s.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes())
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

fn write_number(out: &mut Vec<u8>, n: &Number) {
    if let Some(u) = n.as_u64() {
        out.extend_from_slice(u.to_string().as_bytes());
    } else if let Some(i) = n.as_i64() {
        out.extend_from_slice(i.to_string().as_bytes());
    } else {
        // serde_json never holds a non-finite f64.
        out.extend_from_slice(ecmascript_number(n.as_f64().unwrap_or(0.0)).as_bytes());
    }
}

/// ECMAScript `Number::toString` (ECMA-262 §6.1.6.1.20), which RFC 8785 §3.2.2.3 requires.
pub fn ecmascript_number(v: f64) -> String {
    if !v.is_finite() {
        // Never emitted by the export (schema §4); the caller guarantees finiteness.
        return "null".to_string();
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let negative = v < 0.0;
    // `{:e}` gives the shortest round-trip digits: `d[.ddd]e<exp>`.
    let sci = format!("{:e}", v.abs());
    let (mantissa, exp) = sci.split_once('e').expect("exponent form");
    let exp: i32 = exp.parse().expect("integer exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1;
    let mut s = String::new();
    if negative {
        s.push('-');
    }
    if k <= n && n <= 21 {
        s.push_str(&digits);
        s.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        s.push_str(&digits[..n as usize]);
        s.push('.');
        s.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        s.push_str("0.");
        s.extend(std::iter::repeat_n('0', (-n) as usize));
        s.push_str(&digits);
    } else {
        s.push_str(&digits[..1]);
        if k > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        s.push('e');
        s.push(if n - 1 < 0 { '-' } else { '+' });
        s.push_str(&(n - 1).abs().to_string());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::ecmascript_number as es;

    #[test]
    fn ecmascript_number_forms() {
        assert_eq!(es(1.0), "1");
        assert_eq!(es(-0.0), "0");
        assert_eq!(es(0.1), "0.1");
        assert_eq!(es(1e-7), "1e-7");
        assert_eq!(es(1e21), "1e+21");
        assert_eq!(es(1e20), "100000000000000000000");
        assert_eq!(es(123.456), "123.456");
        assert_eq!(es(0.000001), "0.000001");
        assert_eq!(es(-2.5e-9), "-2.5e-9");
        assert_eq!(es(16.015625), "16.015625");
    }
}
