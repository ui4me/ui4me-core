//! Transport-independent JSON-RPC profile used by both local transports.
//!
//! A frame is a four-byte unsigned big-endian byte length followed by one
//! UTF-8 JSON object. Requests use named parameters and nonempty string IDs;
//! notifications omit the ID. Batches and positional parameters are deliberately
//! outside protocol version 1. The framing layer does not perform authentication.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::io::{self, Read, Write};
use thiserror::Error;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PENDING: usize = 128;
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_METHOD_BYTES: usize = 128;
pub const MAX_OUTGOING_BYTES: usize = 8 * 1024 * 1024;

pub mod code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    pub const UNSUPPORTED: i64 = -32001;
    pub const UNAVAILABLE: i64 = -32002;
    pub const DENIED: i64 = -32003;
    pub const CLOSED: i64 = -32004;
    pub const BUSY: i64 = -32005;
    pub const CANCELLED: i64 = -32006;
    pub const DISCONNECTED: i64 = -32007;
    pub const INVALID_STATE: i64 = -32008;
    pub const TIMEOUT: i64 = -32009;
    pub const VERSION_MISMATCH: i64 = -32010;
    pub const UNAUTHENTICATED: i64 = -32011;
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("transport I/O: {0}")]
    Io(#[from] io::Error),
    #[error("connection ended inside a frame header")]
    TruncatedHeader,
    #[error("connection ended inside a frame body")]
    TruncatedBody,
    #[error("an empty frame is not valid")]
    Empty,
    #[error("frame length {length} exceeds the {maximum}-byte limit")]
    TooLarge { length: usize, maximum: usize },
    #[error("frame is not valid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("frame is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Returns `None` only for EOF before the first header byte. A partial header
/// or body is a connection failure, never an empty notification.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Value>, FrameError> {
    let mut header = [0_u8; 4];
    let mut received = 0;
    while received < header.len() {
        match reader.read(&mut header[received..]) {
            Ok(0) if received == 0 => return Ok(None),
            Ok(0) => return Err(FrameError::TruncatedHeader),
            Ok(count) => received += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(FrameError::Empty);
    }
    // This check must remain before allocation or reading the advertised body.
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge {
            length,
            maximum: MAX_FRAME_BYTES,
        });
    }
    let mut body = vec![0; length];
    match reader.read_exact(&mut body) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(FrameError::TruncatedBody);
        }
        Err(error) => return Err(error.into()),
    }
    let text = std::str::from_utf8(&body)?;
    Ok(Some(serde_json::from_str(text)?))
}

/// Encode into a bounded buffer before touching the stream. This prevents a
/// serialization/size error from leaving a half-written frame on the wire.
pub fn write_frame<W: Write>(writer: &mut W, value: &Value) -> Result<(), FrameError> {
    let mut body = BoundedBuffer::default();
    if let Err(error) = serde_json::to_writer(&mut body, value) {
        if let Some(length) = body.rejected_length {
            return Err(FrameError::TooLarge {
                length,
                maximum: MAX_FRAME_BYTES,
            });
        }
        return Err(FrameError::Json(error));
    }
    writer.write_all(&(body.bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&body.bytes)?;
    writer.flush()?;
    Ok(())
}

/// Count the actual UTF-8 JSON body bytes, including escaping, without
/// allocating a serialized payload. Stop as soon as the framing limit would
/// be exceeded; the reported rejected length is a lower bound on the full
/// encoding's size in that case.
pub fn encoded_size(value: &Value) -> Result<usize, FrameError> {
    #[derive(Default)]
    struct Counter {
        length: usize,
        rejected_length: Option<usize>,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let next = self.length.saturating_add(bytes.len());
            if next > MAX_FRAME_BYTES {
                self.rejected_length = Some(next);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "frame is too large",
                ));
            }
            self.length = next;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter::default();
    if let Err(error) = serde_json::to_writer(&mut counter, value) {
        if let Some(length) = counter.rejected_length {
            return Err(FrameError::TooLarge {
                length,
                maximum: MAX_FRAME_BYTES,
            });
        }
        return Err(FrameError::Json(error));
    }
    Ok(counter.length)
}

#[derive(Default)]
struct BoundedBuffer {
    bytes: Vec<u8>,
    rejected_length: Option<usize>,
}

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let attempted = self.bytes.len().saturating_add(bytes.len());
        if attempted > MAX_FRAME_BYTES {
            self.rejected_length = Some(attempted);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame is too large",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: Option<String>,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Error)]
#[serde(deny_unknown_fields)]
#[error("{message} (RPC {code})")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(code::INVALID_REQUEST, message)
    }
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(code::INVALID_PARAMS, message)
    }
    pub fn method_not_found(method: &str) -> Self {
        Self::new(code::METHOD_NOT_FOUND, "Unknown method").with_data(json!({"method": method}))
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(code::UNSUPPORTED, message)
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(code::UNAVAILABLE, message)
    }
    pub fn denied(message: impl Into<String>) -> Self {
        Self::new(code::DENIED, message)
    }
    pub fn closed(message: impl Into<String>) -> Self {
        Self::new(code::CLOSED, message)
    }
    pub fn busy(message: impl Into<String>) -> Self {
        Self::new(code::BUSY, message)
    }
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(code::CANCELLED, message)
    }
    pub fn disconnected(message: impl Into<String>) -> Self {
        Self::new(code::DISCONNECTED, message)
    }
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new(code::INVALID_STATE, message)
    }
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(code::TIMEOUT, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(code::INTERNAL_ERROR, message)
    }
}

pub fn parse_request(value: Value) -> Result<Request, RpcError> {
    let object = object_with_fields(&value, &["jsonrpc", "id", "method", "params"])?;
    validate_version(object)?;
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_request("method must be a string"))?;
    if method.is_empty()
        || method.len() > MAX_METHOD_BYTES
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || method.starts_with("rpc.")
    {
        return Err(RpcError::invalid_request(
            "method is outside the version 1 profile",
        ));
    }
    let id = object.get("id").map(validated_id).transpose()?;
    let params = match object.get("params") {
        None => json!({}),
        Some(value) if value.is_object() => value.clone(),
        Some(_) => {
            return Err(RpcError::invalid_params(
                "params must be a named-parameter object",
            ));
        }
    };
    Ok(Request {
        id,
        method: method.to_owned(),
        params,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub id: String,
    pub outcome: Result<Value, RpcError>,
}

pub fn parse_response(value: Value) -> Result<Response, RpcError> {
    let object = object_with_fields(&value, &["jsonrpc", "id", "result", "error"])?;
    validate_version(object)?;
    let id = validated_id(
        object
            .get("id")
            .ok_or_else(|| RpcError::invalid_request("response id is required"))?,
    )?;
    let outcome = match (object.get("result"), object.get("error")) {
        (Some(result), None) => Ok(result.clone()),
        (None, Some(error)) => Err(serde_json::from_value(error.clone())
            .map_err(|_| RpcError::invalid_request("invalid response error object"))?),
        _ => {
            return Err(RpcError::invalid_request(
                "response requires exactly one of result or error",
            ));
        }
    };
    Ok(Response { id, outcome })
}

fn object_with_fields<'a>(
    value: &'a Value,
    allowed: &[&str],
) -> Result<&'a Map<String, Value>, RpcError> {
    let object = value.as_object().ok_or_else(|| {
        RpcError::invalid_request("one JSON object is required; batches are not supported")
    })?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(
            RpcError::invalid_request("unknown message field").with_data(json!({"field":key}))
        );
    }
    Ok(object)
}

fn validate_version(object: &Map<String, Value>) -> Result<(), RpcError> {
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(RpcError::invalid_request("jsonrpc must be exactly 2.0"));
    }
    Ok(())
}

fn validated_id(value: &Value) -> Result<String, RpcError> {
    let id = value
        .as_str()
        .ok_or_else(|| RpcError::invalid_request("request IDs must be strings"))?;
    if id.is_empty() || id.len() > MAX_ID_BYTES || id.chars().any(char::is_control) {
        return Err(RpcError::invalid_request(
            "ID must be a nonempty, bounded string without control characters",
        ));
    }
    Ok(id.to_owned())
}

pub fn success(id: &str, result: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "result":result})
}

pub fn error(id: Option<&str>, error: &RpcError) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":error})
}

pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0", "method":method, "params":params})
}

pub fn request(id: &str, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Fragments<R>(R);
    impl<R: Read> Read for Fragments<R> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let count = bytes.len().min(1);
            self.0.read(&mut bytes[..count])
        }
    }

    #[test]
    fn fragmentation_and_unicode_round_trip() {
        let first = request(
            "one",
            "window.set_title",
            json!({"title":"Příliš žluťoučký 🦀 漢字"}),
        );
        let second = notification("app.changed", json!({}));
        let mut wire = Vec::new();
        write_frame(&mut wire, &first).unwrap();
        let first_size = u32::from_be_bytes(wire[..4].try_into().unwrap()) as usize;
        assert_eq!(first_size, serde_json::to_vec(&first).unwrap().len());
        write_frame(&mut wire, &second).unwrap();
        let mut reader = Fragments(Cursor::new(wire));
        assert_eq!(read_frame(&mut reader).unwrap(), Some(first));
        assert_eq!(read_frame(&mut reader).unwrap(), Some(second));
        assert_eq!(read_frame(&mut reader).unwrap(), None);
    }

    #[test]
    fn truncated_frames_are_not_clean_disconnects() {
        assert!(matches!(
            read_frame(&mut Cursor::new([0, 1])),
            Err(FrameError::TruncatedHeader)
        ));
        assert!(matches!(
            read_frame(&mut Cursor::new([0, 0, 0, 2, b'{'])),
            Err(FrameError::TruncatedBody)
        ));
        assert!(matches!(
            read_frame(&mut Cursor::new([0; 4])),
            Err(FrameError::Empty)
        ));
    }

    #[test]
    fn oversized_header_is_rejected_without_reading_body() {
        let mut reader = Cursor::new(((MAX_FRAME_BYTES + 1) as u32).to_be_bytes());
        assert!(matches!(
            read_frame(&mut reader),
            Err(FrameError::TooLarge { .. })
        ));
        assert_eq!(reader.position(), 4);
    }

    #[test]
    fn invalid_utf8_and_json_are_distinct() {
        assert!(matches!(
            read_frame(&mut Cursor::new([0, 0, 0, 1, 255])),
            Err(FrameError::Utf8(_))
        ));
        assert!(matches!(
            read_frame(&mut Cursor::new([0, 0, 0, 1, b'{'])),
            Err(FrameError::Json(_))
        ));
    }

    #[test]
    fn outgoing_oversize_never_writes_partial_header() {
        let mut wire = Vec::new();
        let message = json!({"text":"x".repeat(MAX_FRAME_BYTES)});
        assert!(matches!(
            write_frame(&mut wire, &message),
            Err(FrameError::TooLarge { .. })
        ));
        assert!(wire.is_empty());
    }

    #[test]
    fn encoded_size_accounts_for_unicode_and_json_escape_expansion() {
        let value = success("unicode", json!({"text":"Příliš 漢字 🦀\n\u{0000}"}));
        assert_eq!(
            encoded_size(&value).unwrap(),
            serde_json::to_vec(&value).unwrap().len()
        );
        // The application text is below 1 MiB, but JSON encodes every NUL as
        // six bytes. The host must report a small error, not disconnect while
        // trying to send an oversized native clipboard/dialog result.
        let value = success("large", json!({"text":"\u{0000}".repeat(400_000)}));
        assert!(matches!(
            encoded_size(&value),
            Err(FrameError::TooLarge { .. })
        ));
    }

    #[test]
    fn strict_request_profile_and_notifications() {
        assert_eq!(
            parse_request(notification("app.ready", json!({})))
                .unwrap()
                .id,
            None
        );
        for bad in [
            json!([]),
            json!({"jsonrpc":"1.0","id":"a","method":"hello"}),
            json!({"jsonrpc":"2.0","id":1,"method":"hello"}),
            json!({"jsonrpc":"2.0","id":null,"method":"hello"}),
            json!({"jsonrpc":"2.0","id":"a","method":"hello","unexpected":true}),
            json!({"jsonrpc":"2.0","id":"a","method":"hello","params":[]}),
            json!({"jsonrpc":"2.0","id":"a","method":"rpc.test"}),
        ] {
            assert!(parse_request(bad.clone()).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn responses_have_exactly_one_outcome() {
        assert_eq!(
            parse_response(success("r1", Value::Null)).unwrap().outcome,
            Ok(Value::Null)
        );
        let failure = RpcError::closed("Window was closed");
        assert_eq!(
            parse_response(error(Some("r1"), &failure)).unwrap().outcome,
            Err(failure)
        );
        assert!(parse_response(json!({"jsonrpc":"2.0","id":"r","result":1,"error":{}})).is_err());
        assert!(parse_response(json!({"jsonrpc":"2.0","id":"r"})).is_err());
    }
}
