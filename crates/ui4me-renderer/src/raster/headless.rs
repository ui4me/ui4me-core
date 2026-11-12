//! owner: 010-paint-render — created by 001 slice (R-28)
//!
//! Headless raster (010 plan: `RasterBackend::render_headless(&SceneSnapshot, scale) ->
//! (PixelBuffer, RenderReceipt)`): single-threaded vello_cpu, RGBA8 output, unhinted grayscale
//! anti-aliasing (R-31). The window path (`render` with a `RenderTarget` and damage plan) is
//! added for 011 by T048/T055.
//! Implemented by 001 T048.

use std::sync::Arc;
use std::time::Instant;

use crate::raster::caps::CapabilityTable;
use crate::raster::lower::{MAX_SURFACE_SIDE, VelloRaster, prevalidate};
use crate::raster::receipt::{
    DeviceInfo, ReceiptNondeterministic, ReceiptStatus, RenderMode, RenderReceipt, RenderTarget,
    SceneRef,
};
use crate::scene::appendix_e::SceneSnapshot;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::geometry::DeviceRect;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba8Premultiplied,
    Rgba8Unpremultiplied,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelBuffer {
    /// Device px.
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub format: PixelFormat,
    /// Row-major, `width * height * 4` bytes, no row padding.
    pub data: Vec<u8>,
}

impl PixelBuffer {
    /// Hand the pixel storage back to the process-wide frame-buffer pool ([`pool`]) so that the next frame of about
    /// the same size renders into it instead of allocating a new multi-hundred-KiB buffer. Dropping a buffer
    /// instead is always correct, only slower and rougher on the allocator.
    pub fn recycle(self) {
        pool::give(self.data);
    }
}

/// The frame-buffer pool. A window presents a frame every few milliseconds while it changes, and every frame is a
/// `width * height * 4`-byte buffer (480 KB at 400x300, 8 MB at 1920x1080). Allocating and freeing one per frame makes
/// the allocator commit and decommit pages late and unevenly: on Windows the process heap's last segment swung by
/// about 1 MB (15 % of a 6 MB process) around the three frames of a window cycle, which alone made the open/resize/
/// close memory criterion (SC-001) fail about half of the runs. Frames now render into recycled storage.
///
/// Bounded: at most [`pool::MAX_BUFFERS`] buffers and [`pool::MAX_BYTES`] bytes are kept (the oldest is dropped
/// first); a buffer is reused only for a frame whose size is within a factor of two of it, so a window that shrinks
/// does not pin its old storage; [`pool::trim`] releases everything.
pub mod pool {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Counters since the process started: `[hits, misses, kept, evicted, shared]` (see [`stats`]).
    static COUNTS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

    /// `(hits, misses, kept, evicted, shared)`: frames rendered into a recycled buffer, frames that had to allocate,
    /// buffers returned to the pool, buffers pushed out of the pool (bound reached), frames whose pixels were still
    /// shared (not recyclable: a diagnostics export holds them).
    pub fn stats() -> [u64; 5] {
        std::array::from_fn(|i| COUNTS[i].load(Ordering::Relaxed))
    }

    /// Count a frame whose pixels were still shared when it was recycled (the pipeline calls this).
    pub fn note_shared() {
        COUNTS[4].fetch_add(1, Ordering::Relaxed);
    }

    /// At most this many spare buffers are kept (one in flight plus one for the next size of an interactive resize).
    pub const MAX_BUFFERS: usize = 2;
    /// At most this many bytes of spare buffers are kept (a 4K frame is 33 MB).
    pub const MAX_BYTES: usize = 96 << 20;
    /// A spare is reused for a frame of `len` bytes when `len <= capacity <= 2 * len + SLACK`.
    const SLACK: usize = 64 << 10;

    static SPARE: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

    fn spare() -> std::sync::MutexGuard<'static, Vec<Vec<u8>>> {
        SPARE.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// An empty buffer with capacity for `len` bytes: the best-fitting spare, if any fits.
    pub(crate) fn take(len: usize) -> Option<Vec<u8>> {
        let mut s = spare();
        let Some(i) = s
            .iter()
            .enumerate()
            .filter(|(_, v)| v.capacity() >= len && v.capacity() <= 2 * len + SLACK)
            .min_by_key(|(_, v)| v.capacity())
            .map(|(i, _)| i)
        else {
            COUNTS[1].fetch_add(1, Ordering::Relaxed);
            return None;
        };
        COUNTS[0].fetch_add(1, Ordering::Relaxed);
        let mut v = s.swap_remove(i);
        v.clear();
        Some(v)
    }

    pub(crate) fn give(mut v: Vec<u8>) {
        if v.capacity() == 0 || v.capacity() > MAX_BYTES {
            return;
        }
        v.clear();
        let mut s = spare();
        s.push(v);
        COUNTS[2].fetch_add(1, Ordering::Relaxed);
        while s.len() > MAX_BUFFERS || s.iter().map(Vec::capacity).sum::<usize>() > MAX_BYTES {
            s.remove(0);
            COUNTS[3].fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Release every spare buffer (the runtime does this when it shuts down).
    pub fn trim() {
        spare().clear();
    }

    /// `(buffers, bytes)` currently held.
    pub fn held() -> (usize, usize) {
        let s = spare();
        (s.len(), s.iter().map(Vec::capacity).sum())
    }
}

/// A failed render attempt: its status (stage `raster`) and its receipt (`failed:<code>`, empty
/// damage and degraded lists; render-receipt.md §2 rule 4).
#[derive(Clone, Debug, PartialEq)]
pub struct RenderFailure {
    pub status: Status,
    pub receipt: RenderReceipt,
}

/// One raster backend instance; owns the per-target `frame_seq` counters.
#[derive(Debug)]
pub struct RasterBackend {
    table: CapabilityTable,
    digest: String,
    headless_seq: u64,
    raster: VelloRaster,
    diagnostics: Vec<Status>,
    /// The damage raster's previous frame ([`RasterBackend::render_incremental`]).
    prev: Option<PrevFrame>,
    font_boxes: crate::raster::damage::FontBoxes,
    /// Scratch rows of a partial raster.
    scratch: Vec<u8>,
}

/// The last frame [`RasterBackend::render_incremental`] produced: its scene, scale and pixels.
struct PrevFrame {
    scene: Arc<SceneSnapshot>,
    scale: f32,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl std::fmt::Debug for PrevFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrevFrame")
            .field("size", &(self.width, self.height))
            .field("scale", &self.scale)
            .finish()
    }
}

/// What [`RasterBackend::render_incremental`] rastered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageOutcome {
    /// Every pixel.
    Full,
    /// Only `rect` (device px; empty: nothing changed); the other pixels are the previous frame's.
    Partial(DeviceRect),
}

#[allow(clippy::new_without_default)] // `new` is not a cheap default: it selects the capability table
impl RasterBackend {
    pub fn new() -> RasterBackend {
        let table = CapabilityTable::slice();
        let digest = table.digest();
        RasterBackend {
            table,
            digest,
            headless_seq: 0,
            raster: VelloRaster::new(),
            diagnostics: Vec::new(),
            prev: None,
            font_boxes: Default::default(),
            scratch: Vec::new(),
        }
    }

    /// The capability table this backend was created with.
    pub fn capabilities(&self) -> &CapabilityTable {
        &self.table
    }

    /// Item-level raster diagnostics since the last call (an omitted glyph run whose font the
    /// snapshot does not hold, or a partially rendered run): `INTERNAL_INVARIANT`, stage `raster`.
    pub fn take_diagnostics(&mut self) -> Vec<Status> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Device size for `viewport × scale`, rounded up; `SIZE_LIMIT` outside 1…=65,535 per side.
    pub fn device_size(scene: &SceneSnapshot, scale: f32) -> Result<(u32, u32), Status> {
        let limit = |what: &str| {
            Status::new(Code::SizeLimit, Stage::Raster)
                .with_message(format!("device {what} outside 1..={MAX_SURFACE_SIDE}"))
        };
        if !(scale.is_finite() && scale > 0.0) {
            return Err(
                Status::new(Code::SizeLimit, Stage::Raster).with_message(format!(
                    "device scale {scale} is not a positive finite number"
                )),
            );
        }
        let side = |css: f64| -> Option<u32> {
            let d = (css * scale as f64).ceil();
            (d >= 1.0 && d <= MAX_SURFACE_SIDE as f64).then_some(d as u32)
        };
        let w = side(scene.viewport.width.to_f64()).ok_or_else(|| limit("width"))?;
        let h = side(scene.viewport.height.to_f64()).ok_or_else(|| limit("height"))?;
        Ok((w, h))
    }

    /// Raster `scene` at `scale` into a device-size buffer (`ceil(viewport * scale)`).
    /// Every attempt, including a failed one, returns a receipt (rule 1).
    pub fn render_headless(
        &mut self,
        scene: &SceneSnapshot,
        scale: f32,
    ) -> Result<(PixelBuffer, RenderReceipt), Box<RenderFailure>> {
        self.render_headless_culled(scene, scale, false)
    }

    /// [`RasterBackend::render_headless`]; with `cull`, the items that paint no pixel of the surface
    /// (`damage::surface_items`: rows of a long table below the viewport or clipped by a scroller) are
    /// left out, which changes no pixel.
    fn render_headless_culled(
        &mut self,
        scene: &SceneSnapshot,
        scale: f32,
        cull: bool,
    ) -> Result<(PixelBuffer, RenderReceipt), Box<RenderFailure>> {
        let started = Instant::now();
        self.headless_seq += 1;
        let frame_seq = self.headless_seq;
        let mut receipt = RenderReceipt {
            frame_seq,
            target: RenderTarget::Headless,
            scene: SceneRef {
                generation: scene.generation,
                revision: scene.revision,
            },
            device: DeviceInfo {
                width: 0,
                height: 0,
                scale,
            },
            mode: RenderMode::Full,
            damage: Vec::new(),
            degraded: Vec::new(),
            budget: Vec::new(),
            status: ReceiptStatus::RenderedHeadless,
            capability_table: self.digest.clone(),
            nondeterministic: ReceiptNondeterministic {
                first_frame: frame_seq == 1,
                ..ReceiptNondeterministic::default()
            },
        };
        let fail = |mut receipt: RenderReceipt, status: Status| {
            // Rule 4: a failed frame's receipt has empty damage and degraded lists.
            receipt.status = ReceiptStatus::Failed(status.code);
            receipt.damage.clear();
            receipt.degraded.clear();
            receipt.nondeterministic.raster_ms = started.elapsed().as_secs_f64() * 1000.0;
            Box::new(RenderFailure { status, receipt })
        };
        let (width, height) = match Self::device_size(scene, scale) {
            Ok(s) => s,
            Err(status) => return Err(fail(receipt, status)),
        };
        receipt.device = DeviceInfo {
            width,
            height,
            scale,
        };
        // Pre-validation before any backend call (AD-17).
        if let Err(status) = prevalidate(&scene.display_list, scale) {
            return Err(fail(receipt, status));
        }
        let len = width as usize * height as usize * 4;
        let mut data = pool::take(len).unwrap_or_default();
        if data.try_reserve_exact(len).is_err() {
            return Err(fail(
                receipt,
                Status::new(Code::OutOfMemory, Stage::Raster)
                    .with_message(format!("{width}x{height} RGBA8 target")),
            ));
        }
        data.resize(len, 0);
        let draw = cull
            .then(|| {
                crate::raster::damage::surface_items(
                    scene,
                    scale,
                    (width, height),
                    &mut self.font_boxes,
                )
            })
            .filter(|d| d.iter().any(|v| !*v));
        if let Err(status) = self.raster.render_rows(
            scene,
            scale,
            (width, height),
            draw.as_deref(),
            height,
            &mut data,
            &mut self.diagnostics,
        ) {
            return Err(fail(receipt, status));
        }
        receipt.damage = vec![DeviceRect {
            x: 0,
            y: 0,
            w: width as i32,
            h: height as i32,
        }];
        // The slice table has Mandatory entries only, so no item is drawn with a fallback.
        receipt.nondeterministic.raster_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok((
            PixelBuffer {
                width,
                height,
                scale,
                format: PixelFormat::Rgba8Premultiplied,
                data,
            },
            receipt,
        ))
    }

    /// Forget the damage raster's previous frame: the next [`RasterBackend::render_incremental`] is a full raster.
    pub fn reset_damage(&mut self) {
        self.prev = None;
    }

    /// [`RasterBackend::render_headless`] with the damage raster (`crate::raster::damage`): the pixels are
    /// identical to a full raster of `scene`; only the part that differs from the previous frame of
    /// this backend is rastered again. The receipt is the full-frame receipt (the frame is complete);
    /// the outcome says what was rastered.
    pub fn render_incremental(
        &mut self,
        scene: &Arc<SceneSnapshot>,
        scale: f32,
    ) -> Result<(PixelBuffer, RenderReceipt, DamageOutcome), Box<RenderFailure>> {
        use crate::raster::damage::{DamagePlan, plan};
        let started = Instant::now();
        let size = Self::device_size(scene, scale).ok();
        let plan = match (&self.prev, size) {
            (Some(p), Some((w, h))) if p.scale == scale && p.width == w && p.height == h => {
                if Arc::ptr_eq(&p.scene, scene) {
                    DamagePlan::Clean
                } else {
                    plan(&p.scene, scene, scale, (w, h), &mut self.font_boxes)
                }
            }
            _ => DamagePlan::Full,
        };
        let (rect, draw) = match plan {
            DamagePlan::Full => return self.render_full_keep(scene, scale),
            DamagePlan::Clean => ([0u32; 4], None),
            DamagePlan::Partial { rect, draw } => {
                if prevalidate(&scene.display_list, scale).is_err() {
                    // The full attempt reports the failure with its receipt.
                    return self.render_full_keep(scene, scale);
                }
                (rect, Some(draw))
            }
        };
        let prev = self
            .prev
            .as_mut()
            .expect("a partial plan has a previous frame");
        let (width, height) = (prev.width, prev.height);
        if let Some(draw) = draw {
            let rows = rect[3];
            self.scratch.clear();
            self.scratch.resize(width as usize * rows as usize * 4, 0);
            if self
                .raster
                .render_rows(
                    scene,
                    scale,
                    (width, height),
                    Some(&draw),
                    rows,
                    &mut self.scratch,
                    &mut self.diagnostics,
                )
                .is_err()
            {
                return self.render_full_keep(scene, scale);
            }
            let stride = width as usize * 4;
            let (x0, x1) = (rect[0] as usize * 4, rect[2] as usize * 4);
            for y in rect[1] as usize..rows as usize {
                let row = y * stride;
                prev.data[row + x0..row + x1].copy_from_slice(&self.scratch[row + x0..row + x1]);
            }
        }
        prev.scene = scene.clone();
        self.headless_seq += 1;
        let frame_seq = self.headless_seq;
        let mut data = pool::take(prev.data.len()).unwrap_or_default();
        data.extend_from_slice(&prev.data);
        let mut receipt = RenderReceipt {
            frame_seq,
            target: RenderTarget::Headless,
            scene: SceneRef {
                generation: scene.generation,
                revision: scene.revision,
            },
            device: DeviceInfo {
                width,
                height,
                scale,
            },
            mode: RenderMode::Full,
            damage: vec![DeviceRect {
                x: 0,
                y: 0,
                w: width as i32,
                h: height as i32,
            }],
            degraded: Vec::new(),
            budget: Vec::new(),
            status: ReceiptStatus::RenderedHeadless,
            capability_table: self.digest.clone(),
            nondeterministic: ReceiptNondeterministic {
                first_frame: frame_seq == 1,
                ..ReceiptNondeterministic::default()
            },
        };
        receipt.nondeterministic.raster_ms = started.elapsed().as_secs_f64() * 1000.0;
        let damage = DeviceRect {
            x: rect[0] as i32,
            y: rect[1] as i32,
            w: (rect[2] - rect[0]) as i32,
            h: (rect[3] - rect[1]) as i32,
        };
        Ok((
            PixelBuffer {
                width,
                height,
                scale,
                format: PixelFormat::Rgba8Premultiplied,
                data,
            },
            receipt,
            DamageOutcome::Partial(damage),
        ))
    }

    /// A full raster that becomes the damage raster's previous frame.
    fn render_full_keep(
        &mut self,
        scene: &Arc<SceneSnapshot>,
        scale: f32,
    ) -> Result<(PixelBuffer, RenderReceipt, DamageOutcome), Box<RenderFailure>> {
        // Invalidate the retained scene before rastering (including on failure),
        // but keep its pixel allocation for another frame of similar size. The
        // previous `self.prev = None; self.prev.take()` sequence always discarded
        // this full-surface allocation, even during steady-size animation.
        let mut data = self.prev.take().map(|p| p.data).unwrap_or_default();
        let (pixels, receipt) = self.render_headless_culled(scene, scale, true)?;
        // Match the spare-pool's size policy: shrinking a large window must not
        // retain an unrelated, much larger allocation until the window closes.
        if data.capacity() > pixels.data.len().saturating_mul(2).saturating_add(64 << 10) {
            data = Vec::new();
        }
        data.clear();
        data.extend_from_slice(&pixels.data);
        self.prev = Some(PrevFrame {
            scene: scene.clone(),
            scale,
            width: pixels.width,
            height: pixels.height,
            data,
        });
        Ok((pixels, receipt, DamageOutcome::Full))
    }
}

#[cfg(test)]
mod retained_frame_tests {
    use super::*;

    fn scene(width: u32, height: u32, color: &str) -> Arc<SceneSnapshot> {
        let mut document = crate::Document::parse(&format!(
            "<!doctype html><style>html,body{{margin:0;background:{color}}}</style>"
        ))
        .unwrap();
        document.render(width, height, 1.0).unwrap();
        document
            .pipeline_mut_for_testing()
            .committed()
            .unwrap()
            .scene
            .clone()
    }

    #[test]
    fn full_frames_reuse_storage_preserve_pixels_and_release_on_shrink_or_failure() {
        let mut backend = RasterBackend::new();
        let red = scene(512, 512, "red");
        backend.render_full_keep(&red, 1.0).unwrap();
        let old = &mut backend.prev.as_mut().unwrap().data;
        // Extra capacity distinguishes intentional retention from an allocator
        // happening to return the same address for a new exact-size allocation.
        old.reserve_exact(4096);
        let (pointer, capacity) = (old.as_ptr(), old.capacity());

        let blue = scene(512, 512, "blue");
        let (pixels, _, _) = backend.render_full_keep(&blue, 1.0).unwrap();
        let retained = &backend.prev.as_ref().unwrap().data;
        assert_eq!(retained.as_ptr(), pointer);
        assert_eq!(retained.capacity(), capacity);
        let (expected, _) = RasterBackend::new().render_headless(&blue, 1.0).unwrap();
        assert_eq!(pixels.data, expected.data);

        let small = scene(8, 8, "green");
        let (pixels, _, _) = backend.render_full_keep(&small, 1.0).unwrap();
        let (expected, _) = RasterBackend::new().render_headless(&small, 1.0).unwrap();
        assert_eq!(pixels.data, expected.data);
        assert!(backend.prev.as_ref().unwrap().data.capacity() <= (64 << 10));

        assert!(backend.render_full_keep(&small, f32::NAN).is_err());
        assert!(
            backend.prev.is_none(),
            "a failed full frame invalidates reuse"
        );
    }
}
