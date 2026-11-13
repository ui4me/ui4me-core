//! Bounded document-local raw bitmap transport. Data pixels never enter JSON,
//! a PNG decoder or the CSS resource cache. A trusted backend maps the private
//! host-created file for writes and calls `bitmap.present` after completing a
//! slot. The owner thread snapshots one slot before returning the acknowledgement.
use super::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use tempfile::TempPath;

pub(super) const MAX_BITMAP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BITMAPS: usize = 8;

pub(super) struct Bitmap {
    file: File,
    /// The private path the backend maps. It is unlinked as soon as the
    /// backend proves it has mapped the file (its first `bitmap.present`), so
    /// a killed host cannot leave tmpfs memory behind; dropping the bitmap
    /// before that point removes it as well.
    path: Option<TempPath>,
    width: u32,
    height: u32,
    stride: u32,
    slots: u8,
    slot_bytes: u64,
    last_sequence: u64,
}
impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bitmap")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("slots", &self.slots)
            .field("last_sequence", &self.last_sequence)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    window_id: String,
    document_id: String,
    bitmap_id: String,
    width: u32,
    height: u32,
    #[serde(default = "two")]
    slots: u8,
    #[serde(default = "rgba")]
    pixel_format: String,
}
fn two() -> u8 {
    2
}
fn rgba() -> String {
    "RGBA8".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Present {
    window_id: String,
    document_id: String,
    bitmap_id: String,
    sequence: u64,
    slot: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Close {
    window_id: String,
    document_id: String,
    bitmap_id: String,
}

impl Bitmap {
    fn new(request: &Create) -> Result<Self, RpcError> {
        if request.pixel_format != "RGBA8" {
            return Err(RpcError::invalid_params("Only RGBA8 sRGB is supported"));
        }
        if request.bitmap_id.is_empty()
            || request.bitmap_id.len() > 64
            || !request
                .bitmap_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(RpcError::invalid_params(
                "bitmap_id must contain 1..64 ASCII letters/digits/_/-",
            ));
        }
        if !(2..=3).contains(&request.slots)
            || request.width == 0
            || request.height == 0
            || request.width > 4096
            || request.height > 4096
        {
            return Err(RpcError::invalid_params(
                "Bitmap limits: 1..4096 pixels per side and 2..3 slots",
            ));
        }
        let stride = request
            .width
            .checked_mul(4)
            .ok_or_else(|| RpcError::busy("Bitmap stride overflow"))?;
        let slot_bytes = u64::from(stride) * u64::from(request.height);
        let total = slot_bytes * u64::from(request.slots);
        if total > MAX_BITMAP_BYTES {
            return Err(RpcError::busy("Bitmap transport allocation exceeds 64 MiB"));
        }
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let file = tempfile::Builder::new()
            .prefix("ui4me-bitmap-")
            .tempfile_in(dir)
            .map_err(|e| RpcError::unavailable(e.to_string()))?;
        file.as_file()
            .set_len(total)
            .map_err(|e| RpcError::unavailable(e.to_string()))?;
        let (file, path) = file.into_parts();
        Ok(Self {
            file,
            path: Some(path),
            width: request.width,
            height: request.height,
            stride,
            slots: request.slots,
            slot_bytes,
            last_sequence: 0,
        })
    }

    fn read(&mut self, slot: u8, sequence: u64) -> Result<Option<std::sync::Arc<[u8]>>, RpcError> {
        if slot >= self.slots {
            return Err(RpcError::invalid_params("Bitmap slot is out of bounds"));
        }
        if sequence == 0 {
            return Err(RpcError::invalid_params("Bitmap sequence must be positive"));
        }
        if sequence <= self.last_sequence {
            return Ok(None);
        }
        // Copy from the shared file exactly once. No GUI-thread wait for the
        // producer: the contract requires the producer to finish the mmap write
        // before sending `bitmap.present` and to wait for acknowledgement before
        // reusing the slot. A fast producer can drop pending older frames.
        let file: &mut File = &mut self.file;
        file.seek(SeekFrom::Start(u64::from(slot) * self.slot_bytes))
            .map_err(|e| RpcError::unavailable(e.to_string()))?;
        let mut bytes = vec![0u8; self.slot_bytes as usize];
        file.read_exact(&mut bytes)
            .map_err(|e| RpcError::unavailable(e.to_string()))?;
        // Rendering consumes premultiplied RGBA. The public API accepts ordinary
        // straight-alpha RGBA8 and normalizes it here (sRGB alpha multiplication).
        for rgba in bytes.as_chunks_mut::<4>().0 {
            let a = u32::from(rgba[3]);
            if a < 255 {
                for channel in &mut rgba[..3] {
                    *channel = ((u32::from(*channel) * a + 127) / 255) as u8;
                }
            }
        }
        self.last_sequence = sequence;
        if let Some(path) = self.path.take() {
            // The producer has mapped the file before presenting; both sides
            // keep using their open descriptors and mappings.
            if let Err(error) = path.close() {
                tracing::debug!(%error, "cannot unlink bitmap transport file");
            }
        }
        Ok(Some(bytes.into()))
    }
}

pub(super) type Bitmaps = HashMap<String, Bitmap>;

impl Host {
    pub(super) fn dispatch_bitmap(
        &mut self,
        method: &str,
        params: Value,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) -> Result<Value, RpcError> {
        match method {
            "bitmap.create" => {
                let req: Create = serde_json::from_value(params)
                    .map_err(|e| RpcError::invalid_params(e.to_string()))?;
                let window = self
                    .windows
                    .get_mut(&req.window_id)
                    .ok_or_else(|| RpcError::closed("Window does not exist"))?;
                let doc = window
                    .document
                    .as_mut()
                    .filter(|d| d.id == req.document_id)
                    .ok_or_else(|| RpcError::closed("Document not loaded or stale"))?;
                if doc.bitmaps.contains_key(&req.bitmap_id) {
                    return Err(RpcError::invalid_state("Bitmap already exists"));
                }
                if doc.bitmaps.len() >= MAX_BITMAPS {
                    return Err(RpcError::busy("Maximum eight bitmaps per document"));
                }
                let bitmap = Bitmap::new(&req)?;
                let total_existing: u64 = doc
                    .bitmaps
                    .values()
                    .map(|v| v.slot_bytes * u64::from(v.slots))
                    .sum();
                let total_new = bitmap.slot_bytes * u64::from(bitmap.slots);
                if total_existing + total_new > MAX_BITMAP_BYTES {
                    return Err(RpcError::busy(
                        "Document live bitmap transport budget exceeded",
                    ));
                }
                // Register transparent pixels immediately so the intrinsic size
                // is available for the first HTML/CSS layout.
                let empty = vec![0u8; bitmap.slot_bytes as usize];
                doc.engine
                    .set_live_bitmap(&req.bitmap_id, req.width, req.height, empty.into())?;
                let path = bitmap
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let result = json!({"bitmap_id":req.bitmap_id,"width":bitmap.width,"height":bitmap.height,
                    "stride":bitmap.stride,"pixel_format":"RGBA8","slots":bitmap.slots,
                    "slot_bytes":bitmap.slot_bytes,"path":path,"owner":"backend-writes;host-reads-after-present"});
                doc.bitmaps.insert(req.bitmap_id, bitmap);
                self.document_changed(&req.window_id, event_loop);
                Ok(result)
            }
            "bitmap.present" => {
                let req: Present = serde_json::from_value(params)
                    .map_err(|e| RpcError::invalid_params(e.to_string()))?;
                let window = self
                    .windows
                    .get_mut(&req.window_id)
                    .ok_or_else(|| RpcError::closed("Window does not exist"))?;
                let doc = window
                    .document
                    .as_mut()
                    .filter(|d| d.id == req.document_id)
                    .ok_or_else(|| RpcError::closed("Document not loaded or stale"))?;
                let bitmap = doc
                    .bitmaps
                    .get_mut(&req.bitmap_id)
                    .ok_or_else(|| RpcError::closed("Bitmap not registered"))?;
                let frame = bitmap.read(req.slot, req.sequence)?;
                let applied = frame.is_some();
                if let Some(bytes) = frame {
                    doc.engine.set_live_bitmap(
                        &req.bitmap_id,
                        bitmap.width,
                        bitmap.height,
                        bytes,
                    )?;
                }
                if applied {
                    // A same-size frame changes pixels only: no DOM events,
                    // semantics or geometry, so skip `document_changed` (which
                    // republishes the accessibility tree) and just schedule a
                    // paint. Creation and close keep the full refresh path.
                    if let Some(native) = window.native.as_ref() {
                        native.window.request_redraw();
                    }
                }
                Ok(
                    json!({"sequence":req.sequence,"presented_to_engine":applied,"slot_released":true}),
                )
            }
            "bitmap.close" => {
                let req: Close = serde_json::from_value(params)
                    .map_err(|e| RpcError::invalid_params(e.to_string()))?;
                let window = self
                    .windows
                    .get_mut(&req.window_id)
                    .ok_or_else(|| RpcError::closed("Window does not exist"))?;
                let doc = window
                    .document
                    .as_mut()
                    .filter(|d| d.id == req.document_id)
                    .ok_or_else(|| RpcError::closed("Document not loaded or stale"))?;
                let removed = doc.bitmaps.remove(&req.bitmap_id).is_some();
                if removed {
                    doc.engine.remove_live_bitmap(&req.bitmap_id)?;
                }
                if removed {
                    self.document_changed(&req.window_id, event_loop);
                }
                Ok(json!({"closed":removed}))
            }
            _ => Err(RpcError::method_not_found(method)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_unbounded_or_unsupported_descriptions() {
        let req = Create {
            window_id: "w".into(),
            document_id: "d".into(),
            bitmap_id: "x".into(),
            width: 4096,
            height: 4096,
            slots: 3,
            pixel_format: "RGBA8".into(),
        };
        assert!(Bitmap::new(&req).is_err());
        let req = Create {
            width: 8,
            height: 4,
            slots: 2,
            ..req
        };
        let mut b = Bitmap::new(&req).unwrap();
        assert_eq!(b.slot_bytes, 128);
        assert!(b.read(2, 1).is_err());
        assert!(b.read(0, 0).is_err());
    }

    /// A killed host must not leave its tmpfs transport behind: the path is
    /// unlinked once the producer has presented (and therefore mapped) it.
    #[test]
    fn transport_file_is_unlinked_after_the_first_present() {
        let req = Create {
            window_id: "w".into(),
            document_id: "d".into(),
            bitmap_id: "x".into(),
            width: 2,
            height: 2,
            slots: 2,
            pixel_format: "RGBA8".into(),
        };
        let mut b = Bitmap::new(&req).unwrap();
        let path = b.path.as_ref().unwrap().to_path_buf();
        assert!(path.exists());
        // A stale or rejected present proves nothing about the mapping.
        assert!(b.read(5, 1).is_err());
        assert!(path.exists());
        assert_eq!(b.read(0, 1).unwrap().unwrap().len(), 16);
        assert!(!path.exists(), "mapped transport must be unlinked");
        // The open descriptor still serves later frames.
        assert!(b.read(1, 2).unwrap().is_some());
        // Dropping an unpresented bitmap removes its file too.
        let unpresented = Bitmap::new(&req).unwrap();
        let path = unpresented.path.as_ref().unwrap().to_path_buf();
        drop(unpresented);
        assert!(!path.exists());
    }
}
