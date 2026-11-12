//! owner: 007-resources (T057, T058) — the embedded last-resort font (OD-24, FR-068, R-54, IF-52).
//!
//! Noto Sans Regular (SIL OFL-1.1) is compiled into the library; there is no runtime disk
//! discovery. At runtime creation the bytes are verified against the pinned SHA-256 and, on
//! success, become a runtime-wide source that is the final fallback after the configured fallback
//! list in both system and pinned-fonts-only modes, unless the runtime configuration's
//! `last_resort_fallback` flag is off. It has no `font_id` and cannot be unregistered.
//!
//! OD-24 (the choice of this font) is recorded as pending owner approval in
//! `assets/last-resort/PROVENANCE.md`; nothing here assumes it approved.

use super::registry::{FontBlob, FontBlobId};
use sha2::{Digest, Sha256};
use std::sync::{Arc, OnceLock};
use ui4me_engine_types::codes_generated::Code;

/// The embedded font file.
pub const BYTES: &[u8] = include_bytes!("../../assets/last-resort/NotoSans-Regular.ttf");
/// Its pinned SHA-256 (also in `assets/last-resort/SHA256SUMS`, checked by `build.rs`).
pub const SHA256_HEX: &str = "89c3c497f618fdaa0b2d1e98fef93582f28c71debd2c4a8cdf41f190ced2909d";
pub const FAMILY: &str = "Noto Sans";
pub const FILE_NAME: &str = "NotoSans-Regular.ttf";
/// Reserved blob id; ids of registered fonts start at 1.
pub const BLOB_ID: FontBlobId = FontBlobId(0);

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `font_config_get().last_resort.status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastResortStatus {
    Used,
    Missing,
    ChecksumMismatch,
    Disabled,
}

impl LastResortStatus {
    pub fn name(self) -> &'static str {
        match self {
            LastResortStatus::Used => "used",
            LastResortStatus::Missing => "missing",
            LastResortStatus::ChecksumMismatch => "checksum-mismatch",
            LastResortStatus::Disabled => "disabled",
        }
    }
}

/// Identity, checksum and flag of the font, as reported by `font_config_get` and diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastResortInfo {
    pub enabled: bool,
    pub family: &'static str,
    pub sha256: &'static str,
    pub status: LastResortStatus,
    /// `DECODE_FAILED` when the embedded bytes failed verification (a corrupted binary).
    pub failure: Option<Code>,
}

fn shared_bytes() -> Arc<[u8]> {
    static SHARED: OnceLock<Arc<[u8]>> = OnceLock::new();
    SHARED.get_or_init(|| Arc::from(BYTES)).clone()
}

/// Verify `bytes` against the pinned checksum and build the runtime-wide blob.
pub fn load_from(bytes: &[u8], enabled: bool) -> (Option<FontBlob>, LastResortInfo) {
    let mut info = LastResortInfo {
        enabled,
        family: FAMILY,
        sha256: SHA256_HEX,
        status: LastResortStatus::Disabled,
        failure: None,
    };
    if !enabled {
        return (None, info);
    }
    if bytes.is_empty() {
        info.status = LastResortStatus::Missing;
        return (None, info);
    }
    if sha256_hex(bytes) != SHA256_HEX {
        info.status = LastResortStatus::ChecksumMismatch;
        info.failure = Some(Code::DecodeFailed);
        return (None, info);
    }
    info.status = LastResortStatus::Used;
    let blob = FontBlob {
        id: BLOB_ID,
        name: format!("{FILE_NAME} (embedded)"),
        bytes: if std::ptr::eq(bytes.as_ptr(), BYTES.as_ptr()) {
            shared_bytes()
        } else {
            Arc::from(bytes)
        },
        family_override: None,
    };
    (Some(blob), info)
}

/// The embedded font, verified, unless `enabled` is false.
pub fn load(enabled: bool) -> (Option<FontBlob>, LastResortInfo) {
    load_from(BYTES, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_bytes_match_the_pinned_checksum() {
        assert_eq!(sha256_hex(BYTES), SHA256_HEX);
        assert_eq!(BYTES.len(), 512_672);
        // identical to the fixture font pinned by 002 (R-46)
        let fixture = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/fonts/noto-sans/NotoSans-Regular.ttf"
        ))
        .unwrap();
        assert_eq!(fixture, BYTES);
    }

    #[test]
    fn flag_off_checksum_mismatch_and_empty_are_reported() {
        let (b, i) = load(true);
        assert!(b.is_some());
        assert_eq!(
            (i.status, i.enabled, i.failure),
            (LastResortStatus::Used, true, None)
        );
        let (b, i) = load(false);
        assert!(b.is_none());
        assert_eq!(i.status, LastResortStatus::Disabled);
        let mut corrupted = BYTES.to_vec();
        corrupted[1000] ^= 0xFF;
        let (b, i) = load_from(&corrupted, true);
        assert!(b.is_none());
        assert_eq!(
            (i.status, i.failure),
            (LastResortStatus::ChecksumMismatch, Some(Code::DecodeFailed))
        );
        let (b, i) = load_from(&[], true);
        assert!(b.is_none());
        assert_eq!(i.status, LastResortStatus::Missing);
    }
}
