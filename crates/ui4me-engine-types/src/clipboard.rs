//! owner: 011-platform-windowing with 012-interaction and 004-host-api (lane L-CLIP-r13)
//!
//! Clipboard content and the backend seam. The clipboard carries `text/plain` and, optionally,
//! `text/html` (UTF-8 strings). 004's runtime owns one backend: the platform's system clipboard
//! (Wayland data-control, Windows) in a windowed run, or the in-process [`MemoryClipboard`] in
//! headless and deterministic runs. 012 reads it for paste and writes it for copy and cut.

use crate::codes_generated::Code;
use crate::stage::Stage;
use crate::status::Status;

/// A transfer larger than this fails with `SIZE_LIMIT` and nothing is written or inserted
/// (012 `editing.md` section 3; 011 FR-061).
pub const MAX_CLIPBOARD_BYTES: usize = 16 * 1024 * 1024;

pub const MIME_PLAIN: &str = "text/plain";
pub const MIME_HTML: &str = "text/html";

/// The clipboard content: one string per supported format.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardData {
    pub plain: Option<String>,
    pub html: Option<String>,
}

impl ClipboardData {
    pub fn text(s: impl Into<String>) -> ClipboardData {
        ClipboardData {
            plain: Some(s.into()),
            html: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.plain.is_none() && self.html.is_none()
    }
    /// The DataTransfer format name a caller used, folded to a stored slot: `text` and
    /// `text/plain` are the plain text, `text/html` the markup; anything else has no slot.
    pub fn slot(mime: &str) -> Option<&'static str> {
        match mime.trim().to_ascii_lowercase().as_str() {
            "text" | "text/plain" => Some(MIME_PLAIN),
            "text/html" => Some(MIME_HTML),
            _ => None,
        }
    }
    /// `DataTransfer.getData(format)`: `None` for an absent or unsupported format.
    pub fn get(&self, mime: &str) -> Option<&str> {
        match Self::slot(mime)? {
            MIME_PLAIN => self.plain.as_deref(),
            _ => self.html.as_deref(),
        }
    }
    /// `DataTransfer.setData(format, data)`; `false` for an unsupported format.
    pub fn set(&mut self, mime: &str, data: &str) -> bool {
        match Self::slot(mime) {
            Some(MIME_PLAIN) => self.plain = Some(data.to_owned()),
            Some(_) => self.html = Some(data.to_owned()),
            None => return false,
        }
        true
    }
    /// Reject content over [`MAX_CLIPBOARD_BYTES`].
    pub fn check_size(&self) -> Result<(), Status> {
        let n =
            self.plain.as_ref().map_or(0, String::len) + self.html.as_ref().map_or(0, String::len);
        if n > MAX_CLIPBOARD_BYTES {
            return Err(
                Status::new(Code::SizeLimit, Stage::Platform).with_message(format!(
                    "clipboard transfer of {n} bytes exceeds {MAX_CLIPBOARD_BYTES}"
                )),
            );
        }
        Ok(())
    }
}

/// A clipboard implementation. Calls are synchronous and made on the runtime's owner thread.
pub trait ClipboardBackend {
    /// A short name for diagnostics (`memory`, `wayland-data-control`, `windows`).
    fn name(&self) -> &'static str;
    /// The current content; empty when the clipboard holds no supported format.
    fn read(&mut self) -> Result<ClipboardData, Status>;
    /// Replace the clipboard content.
    fn write(&mut self, data: &ClipboardData) -> Result<(), Status>;
}

/// The in-process clipboard of headless and deterministic runs.
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    data: ClipboardData,
}

impl ClipboardBackend for MemoryClipboard {
    fn name(&self) -> &'static str {
        "memory"
    }
    fn read(&mut self) -> Result<ClipboardData, Status> {
        Ok(self.data.clone())
    }
    fn write(&mut self, data: &ClipboardData) -> Result<(), Status> {
        data.check_size()?;
        self.data = data.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_fold_and_round_trip() {
        let mut d = ClipboardData::default();
        assert!(d.is_empty());
        assert!(d.set("text", "a"));
        assert!(d.set("TEXT/HTML", "<b>a</b>"));
        assert!(!d.set("image/png", "x"));
        assert_eq!(d.get("text/plain"), Some("a"));
        assert_eq!(d.get("text/html"), Some("<b>a</b>"));
        assert_eq!(d.get("application/json"), None);
    }

    #[test]
    fn memory_clipboard_replaces_and_limits() {
        let mut m = MemoryClipboard::default();
        m.write(&ClipboardData::text("one")).unwrap();
        m.write(&ClipboardData::text("two")).unwrap();
        assert_eq!(m.read().unwrap().plain.as_deref(), Some("two"));
        let big = ClipboardData::text("x".repeat(MAX_CLIPBOARD_BYTES + 1));
        assert_eq!(m.write(&big).unwrap_err().code, Code::SizeLimit);
        assert_eq!(
            m.read().unwrap().plain.as_deref(),
            Some("two"),
            "a refused write changes nothing"
        );
    }
}
