//! owner: 007-resources (T041) — the Fetch "data: URL processor" (FR-011, AD-37, research R-03).
//!
//! Uses the `data-url` crate (Servo; the same processor as Blitz). The decoded payload is bounded by
//! the policy's `data_url_max_bytes` (default 4 MiB); `0` disables `data:` (that is decided by
//! `policy::check` as `RESOURCE_DENIED` before a job exists). Malformed input is `URI_MALFORMED`,
//! an oversized payload `SIZE_LIMIT`. The payload is never logged: diagnostics redact it.

use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// A decoded `data:` payload with its media type (`type/subtype`, lowercase, no parameters).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataPayload {
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl DataPayload {
    /// `image/svg+xml` (the declared type decides SVG for `data:`).
    pub fn is_svg(&self) -> bool {
        self.mime == "image/svg+xml"
    }
}

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

/// Process a `data:` URL string. `max_bytes` is the decoded-payload limit.
pub fn process(url: &str, max_bytes: u64) -> Result<DataPayload, Status> {
    if max_bytes == 0 {
        return Err(err(Code::ResourceDenied, "data: URLs are disabled"));
    }
    // A decoded payload is at least a third of its percent-encoded text and three quarters of its
    // base64 text, so text longer than 3x the limit (plus the header) can never fit: refuse it
    // before any decoding allocation.
    if url.len() as u64 > max_bytes.saturating_mul(3).saturating_add(4096) {
        return Err(err(
            Code::SizeLimit,
            format!("data: URL exceeds the {max_bytes}-byte limit"),
        ));
    }
    let parsed = data_url::DataUrl::process(url)
        .map_err(|e| err(Code::UriMalformed, format!("malformed data: URL ({e:?})")))?;
    let mime = {
        let m = parsed.mime_type();
        format!("{}/{}", m.type_, m.subtype).to_ascii_lowercase()
    };
    let (bytes, _fragment) = parsed
        .decode_to_vec()
        .map_err(|_| err(Code::UriMalformed, "malformed data: URL (invalid base64)"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(err(
            Code::SizeLimit,
            format!(
                "data: payload of {} bytes exceeds the {max_bytes}-byte limit",
                bytes.len()
            ),
        ));
    }
    Ok(DataPayload { mime, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_and_base64() {
        let p = process("data:image/svg+xml,%3Csvg/%3E", 1024).unwrap();
        assert_eq!(
            (p.mime.as_str(), p.bytes.as_slice()),
            ("image/svg+xml", b"<svg/>".as_slice())
        );
        assert!(p.is_svg());
        let p = process("data:text/plain;base64,aGk=", 1024).unwrap();
        assert_eq!(p.bytes, b"hi");
    }

    #[test]
    fn missing_type_is_text_plain() {
        assert_eq!(process("data:,a", 8).unwrap().mime, "text/plain");
    }
}
