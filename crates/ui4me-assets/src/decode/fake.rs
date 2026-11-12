//! owner: 007-resources (T014) — deterministic fake decoder.
//!
//! A synthetic container `U4FK` that produces images with configurable dimensions, delay, failure
//! code and injected panic, so lifecycle, policy, cache, limits and shutdown work does not depend
//! on a codec (plan: "Tests use a deterministic fake decoder behind the same trait").
//!
//! Layout (little endian): `"U4FK"`, w u32, h u32, delay_ms u32, fail u8, flags u8, orientation u8,
//! rgba[4] (premultiplied colour) — 23 bytes. `fail`: 0 none, 1 `DECODE_FAILED`, 2
//! `UNSUPPORTED_MEDIA_TYPE`, 3 `SIZE_LIMIT`. `flags`: 1 panic, 2 animated, 4 icc.

use super::{DecodedImage, ImageFormat, ImageHeader, RasterDecoder};
use crate::worker::CancelToken;
use std::time::{Duration, Instant};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

pub const MAGIC: &[u8; 4] = b"U4FK";
pub const LEN: usize = 23;

/// Builder of fake image bytes.
#[derive(Clone, Debug)]
pub struct FakeImage {
    pub width: u32,
    pub height: u32,
    pub delay_ms: u32,
    pub fail: u8,
    pub flags: u8,
    pub orientation: u8,
    pub rgba: [u8; 4],
}

impl FakeImage {
    pub fn new(width: u32, height: u32) -> Self {
        FakeImage {
            width,
            height,
            delay_ms: 0,
            fail: 0,
            flags: 0,
            orientation: 1,
            rgba: [255, 0, 0, 255],
        }
    }
    pub fn delay(mut self, ms: u32) -> Self {
        self.delay_ms = ms;
        self
    }
    pub fn fail_with(mut self, code: Code) -> Self {
        self.fail = match code {
            Code::DecodeFailed => 1,
            Code::UnsupportedMediaType => 2,
            Code::SizeLimit => 3,
            other => panic!("fake decoder cannot fail with {other:?}"),
        };
        self
    }
    pub fn panics(mut self) -> Self {
        self.flags |= 1;
        self
    }
    pub fn animated(mut self) -> Self {
        self.flags |= 2;
        self
    }
    pub fn icc(mut self) -> Self {
        self.flags |= 4;
        self
    }
    pub fn orientation(mut self, o: u8) -> Self {
        self.orientation = o;
        self
    }
    pub fn color(mut self, rgba: [u8; 4]) -> Self {
        self.rgba = rgba;
        self
    }
    pub fn bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(LEN);
        v.extend_from_slice(MAGIC);
        v.extend_from_slice(&self.width.to_le_bytes());
        v.extend_from_slice(&self.height.to_le_bytes());
        v.extend_from_slice(&self.delay_ms.to_le_bytes());
        v.push(self.fail);
        v.push(self.flags);
        v.push(self.orientation);
        v.extend_from_slice(&self.rgba);
        v
    }
}

fn err(code: Code, msg: &str) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

#[derive(Debug, Default)]
pub struct FakeDecoder;

impl RasterDecoder for FakeDecoder {
    fn formats(&self) -> &'static [ImageFormat] {
        &[ImageFormat::Fake]
    }

    fn probe(&self, format: ImageFormat, bytes: &[u8]) -> Result<ImageHeader, Status> {
        if format != ImageFormat::Fake {
            // Real formats: owned header probes, so budgets apply to real files even with the fake.
            return super::sniff::probe_header(format, bytes);
        }
        if bytes.len() < LEN || &bytes[..4] != MAGIC {
            return Err(err(Code::DecodeFailed, "truncated fake image"));
        }
        Ok(ImageHeader {
            format,
            width: u32_at(bytes, 4),
            height: u32_at(bytes, 8),
            orientation: bytes[18].clamp(1, 8),
            animated: bytes[17] & 2 != 0,
            has_icc: bytes[17] & 4 != 0,
        })
    }

    fn decode(
        &self,
        bytes: &[u8],
        header: &ImageHeader,
        cancel: &CancelToken,
    ) -> Result<DecodedImage, Status> {
        if header.format != ImageFormat::Fake {
            return Err(err(
                Code::UnsupportedMediaType,
                "the fake decoder only decodes its own container",
            ));
        }
        let delay = Duration::from_millis(u32_at(bytes, 12) as u64);
        let start = Instant::now();
        while start.elapsed() < delay {
            if cancel.is_canceled() {
                return Err(err(Code::InvalidState, "canceled"));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if bytes[17] & 1 != 0 {
            panic!("fake decoder: injected panic");
        }
        match bytes[16] {
            0 => {}
            1 => return Err(err(Code::DecodeFailed, "fake decoder: injected failure")),
            2 => {
                return Err(err(
                    Code::UnsupportedMediaType,
                    "fake decoder: injected failure",
                ));
            }
            _ => return Err(err(Code::SizeLimit, "fake decoder: injected failure")),
        }
        let (w, h) = (header.width, header.height);
        let color = &bytes[19..23];
        let n = w as usize * h as usize;
        let mut data = Vec::with_capacity(n * 4);
        for _ in 0..n {
            data.extend_from_slice(color);
        }
        Ok(DecodedImage {
            width: w,
            height: h,
            data,
        })
    }
}
