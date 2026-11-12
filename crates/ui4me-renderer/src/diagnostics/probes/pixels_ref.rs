//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `pixels_ref` probe input (schema §5): present only when a headless render was requested. The
//! pixel bytes go to a sibling file; the hash covers the declared format. Serialised by 001 T052.

use crate::diagnostics::envelope::{ExportCx, num};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelsFormat {
    Rgba8Premultiplied,
    Rgba8Unpremultiplied,
}

impl PixelsFormat {
    pub const fn name(self) -> &'static str {
        match self {
            PixelsFormat::Rgba8Premultiplied => "rgba8-premultiplied",
            PixelsFormat::Rgba8Unpremultiplied => "rgba8-unpremultiplied",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelsRefInput {
    /// Device px.
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub format: PixelsFormat,
    /// Lowercase hex SHA-256 of the pixel bytes in `format`.
    pub sha256: String,
    /// Sibling file holding the bytes (subject to redaction).
    pub file: Option<PathBuf>,
}

impl PixelsRefInput {
    /// The reference for `data` (row-major, no padding).
    pub fn of_pixels(
        width: u32,
        height: u32,
        scale: f64,
        format: PixelsFormat,
        data: &[u8],
        file: Option<PathBuf>,
    ) -> PixelsRefInput {
        PixelsRefInput {
            width,
            height,
            scale,
            format,
            sha256: sha256_hex(data),
            file,
        }
    }
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn to_json(p: &PixelsRefInput, cx: &ExportCx) -> Value {
    let mut m = Map::new();
    m.insert("width".into(), json!(p.width));
    m.insert("height".into(), json!(p.height));
    m.insert("scale".into(), num(p.scale));
    m.insert("format".into(), json!(p.format.name()));
    m.insert("sha256".into(), json!(p.sha256));
    if let Some(f) = &p.file {
        m.insert("file".into(), json!(cx.redaction.path(f)));
    }
    Value::Object(m)
}
