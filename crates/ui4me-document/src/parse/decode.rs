//! owner: 003-document-model
//!
//! UTF-8 input decoding (T024; FR-004; research R-22). The only supported input encoding is UTF-8
//! (Encoding Standard "decode"): a UTF-8 BOM is removed and invalid sequences become U+FFFD, each
//! reported as a parser diagnostic with its line. A UTF-16 BOM or a `<meta charset>` naming another
//! encoding is reported as `UNSUPPORTED_FEATURE`; the bytes are still decoded as UTF-8.

use crate::document::Document;

/// The decoded text plus what the decoder noticed.
#[derive(Debug, Default, Clone)]
pub struct Decoded {
    pub text: String,
    /// `(line, message)` for each U+FFFD substitution (1-based line of the invalid sequence).
    pub invalid_sequences: Vec<(u64, String)>,
    /// `UNSUPPORTED_FEATURE` messages (UTF-16 BOM, foreign `<meta charset>`).
    pub unsupported: Vec<String>,
}

pub fn decode_utf8(bytes: &[u8]) -> Decoded {
    let mut out = Decoded::default();
    let mut body = bytes;
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        out.unsupported.push(
            "UTF-16 byte order mark: only UTF-8 input is supported; decoded as UTF-8".to_owned(),
        );
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        body = &bytes[3..];
    }
    let mut line = 1u64;
    let mut rest = body;
    let mut text = String::with_capacity(body.len());
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                text.push_str(s);
                break;
            }
            Err(e) => {
                let (valid, after) = rest.split_at(e.valid_up_to());
                // SAFETY-free: `valid` is valid UTF-8 by from_utf8's contract.
                let s = std::str::from_utf8(valid).expect("prefix is valid UTF-8");
                line += s.bytes().filter(|&b| b == b'\n').count() as u64;
                text.push_str(s);
                text.push('\u{FFFD}');
                out.invalid_sequences
                    .push((line, "invalid UTF-8 sequence replaced by U+FFFD".to_owned()));
                let skip = e.error_len().unwrap_or(after.len());
                rest = &after[skip..];
            }
        }
    }
    if let Some(label) = sniff_meta_charset(&text)
        && !matches!(
            label.to_ascii_lowercase().as_str(),
            "utf-8" | "utf8" | "unicode-1-1-utf-8"
        )
    {
        out.unsupported.push(format!(
            "<meta charset> names '{label}': only UTF-8 input is supported; decoded as UTF-8"
        ));
    }
    out.text = text;
    out
}

/// The first `<meta charset=...>` / `<meta http-equiv=content-type content="...charset=...">` in the
/// first 1,024 bytes (the HTML prescan window), by a simple tag scan.
fn sniff_meta_charset(text: &str) -> Option<String> {
    let mut end = text.len().min(1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = &text[..end];
    let lower = head.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<meta") {
        let start = from + i;
        let tag_end = lower[start..].find('>').map_or(lower.len(), |j| start + j);
        let tag = &lower[start..tag_end];
        if let Some(p) = tag.find("charset") {
            let after = tag[p + 7..].trim_start();
            if let Some(v) = after.strip_prefix('=') {
                let v = v.trim_start().trim_start_matches(['"', '\'']);
                let label: String = v
                    .chars()
                    .take_while(|c| !matches!(c, '"' | '\'' | ' ' | ';' | '/' | '>'))
                    .collect();
                if !label.is_empty() {
                    return Some(label);
                }
            }
        }
        from = start + 5;
    }
    None
}

impl Document {
    /// Fold a decode report into the diagnostics of a freshly created document.
    pub(crate) fn apply_decode_report(&mut self, d: Decoded) {
        for (line, msg) in d.invalid_sequences {
            self.parser_diagnostics.push(line, msg);
        }
        for m in d.unsupported {
            self.parser_diagnostics.push_unsupported(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_is_removed_and_invalid_sequences_become_replacement_characters() {
        let d = decode_utf8(b"\xEF\xBB\xBF<p>a\nb\xFFc\xC3</p>");
        assert_eq!(d.text, "<p>a\nb\u{FFFD}c\u{FFFD}</p>");
        assert_eq!(d.invalid_sequences.len(), 2);
        assert_eq!(d.invalid_sequences[0].0, 2, "line of the first bad byte");
        assert!(d.unsupported.is_empty());
    }

    #[test]
    fn utf16_bom_and_foreign_meta_charset_are_unsupported_features() {
        let d = decode_utf8(b"\xFF\xFEa");
        assert_eq!(d.unsupported.len(), 1);
        let d = decode_utf8(b"<meta charset=\"windows-1252\"><p>x");
        assert_eq!(d.unsupported.len(), 1);
        assert!(d.unsupported[0].contains("windows-1252"));
        let d = decode_utf8(b"<meta charset=utf-8><p>x");
        assert!(d.unsupported.is_empty());
        let d = decode_utf8(
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=iso-8859-1\">",
        );
        assert_eq!(d.unsupported.len(), 1);
    }
}
