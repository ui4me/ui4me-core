//! owner: 007-resources (T014, T069) — the AD-33 decoder seam.
//!
//! Exactly two traits cross the seam, `RasterDecoder` and `SvgRenderer`; no codec, usvg/resvg or
//! tiny-skia type appears in them. The default adapters are `image_rs::ImageRsDecoder` and
//! `svg::usvg_adapter::UsvgRenderer`; `HeaderOnlyDecoder` (owned header probes, decode reports
//! `UNSUPPORTED_MEDIA_TYPE`) remains for builds without a codec, and tests use the deterministic
//! `fake` decoder.

pub mod exif;
pub mod fake;
pub mod image_rs;
pub mod scale;
pub mod sniff;

use crate::worker::CancelToken;
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// Raster container formats of FR-023 (plus the synthetic format of the fake decoder).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    /// Synthetic test container (`decode/fake.rs`); never produced by sniffing real files.
    Fake,
}

/// What a header probe learns without decoding (no pixel allocation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub format: ImageFormat,
    /// Stored dimensions, before EXIF orientation.
    pub width: u32,
    pub height: u32,
    /// EXIF orientation 1..=8 (1 = none).
    pub orientation: u8,
    /// More than one frame (GIF, WebP, APNG): only the first frame is used (reported `img.animation`).
    pub animated: bool,
    /// An ICC profile is present (ignored, `info` report).
    pub has_icc: bool,
}

impl ImageHeader {
    /// Dimensions after applying the orientation (what layout sees as the natural size).
    pub fn oriented_size(&self) -> (u32, u32) {
        if (5..=8).contains(&self.orientation) {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

/// Premultiplied RGBA8, sRGB, tightly packed (`stride = width * 4`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Seam 1: raster decoding. Implementations enforce nothing about policy; the pipeline checks the
/// budgets from `probe` before `decode` is ever called.
pub trait RasterDecoder: Send + Sync {
    /// Formats `decode` can handle (the probe may recognise more).
    fn formats(&self) -> &'static [ImageFormat];
    /// Header only: dimensions, frame count, orientation. Must not allocate pixel memory.
    fn probe(&self, format: ImageFormat, bytes: &[u8]) -> Result<ImageHeader, Status>;
    /// Decode the first frame to premultiplied RGBA8 (stored orientation; the pipeline applies EXIF).
    fn decode(
        &self,
        bytes: &[u8],
        header: &ImageHeader,
        cancel: &CancelToken,
    ) -> Result<DecodedImage, Status>;
}

/// Options of an SVG parse; built per request from the document's policy, never defaults (AD-24).
#[derive(Clone, Debug)]
pub struct SvgOptions {
    /// Element limit of the document and of every nested document together (`SIZE_LIMIT` beyond).
    pub max_nodes: u32,
    /// Nesting depth of `data:` SVG images inside SVG (`SIZE_LIMIT` beyond).
    pub max_nesting: u32,
    /// Decoded-payload limit of a `data:` href (the policy's `data_url_max_bytes`).
    pub max_data_bytes: u64,
    /// Pixel budget of one embedded raster image, and in total per document.
    pub max_embedded_pixels: u64,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            max_nodes: 1_000_000,
            max_nesting: 4,
            max_data_bytes: 4 * 1024 * 1024,
            max_embedded_pixels: 100_000_000,
        }
    }
}

/// Natural dimensions of an SVG in CSS px; nothing is invented (FR-037): an absent value is `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvgNatural {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub ratio: Option<(f32, f32)>,
}

/// The opaque, immutable parsed tree handed back by `SvgRenderer::parse`.
pub type SvgTree = Arc<dyn std::any::Any + Send + Sync>;

/// A parsed SVG with what the pipeline reports about it.
#[derive(Clone)]
pub struct SvgParsed {
    pub tree: SvgTree,
    pub natural: SvgNatural,
    /// `UNSUPPORTED_FEATURE` keys, once each (`svg.text`).
    pub reports: Vec<&'static str>,
    /// Non-fatal canonical codes: one `RESOURCE_DENIED` per refused href, `SIZE_LIMIT` per skipped nested image.
    pub warnings: Vec<Code>,
}

/// Seam 2: static SVG (AD-24). The default adapter is usvg/resvg (`svg::usvg_adapter`).
pub trait SvgRenderer: Send + Sync {
    fn parse(&self, bytes: &[u8], options: &SvgOptions) -> Result<SvgParsed, Status>;
    /// Rasterise to premultiplied RGBA8 at exactly `width_px` x `height_px`.
    fn rasterize(
        &self,
        tree: &SvgTree,
        width_px: u32,
        height_px: u32,
    ) -> Result<DecodedImage, Status>;
}

/// The default raster decoder: the `image` crate adapter (T030, X-ACQ = PASS).
pub fn default_decoder() -> Arc<dyn RasterDecoder> {
    Arc::new(image_rs::ImageRsDecoder)
}

/// Decoder for builds without a codec adapter: recognises headers with the owned probes
/// (so the pre-allocation limits apply to real files) and reports every decode honestly as
/// `UNSUPPORTED_MEDIA_TYPE` (constitution III), never as a silent empty image.
#[derive(Debug, Default)]
pub struct HeaderOnlyDecoder;

impl RasterDecoder for HeaderOnlyDecoder {
    fn formats(&self) -> &'static [ImageFormat] {
        &[]
    }
    fn probe(&self, format: ImageFormat, bytes: &[u8]) -> Result<ImageHeader, Status> {
        sniff::probe_header(format, bytes)
    }
    fn decode(
        &self,
        _bytes: &[u8],
        header: &ImageHeader,
        _cancel: &CancelToken,
    ) -> Result<DecodedImage, Status> {
        Err(
            Status::new(Code::UnsupportedMediaType, Stage::Resource).with_message(format!(
                "no codec adapter for {:?} (X-ACQ pending)",
                header.format
            )),
        )
    }
}

/// Decoded bytes of an image of `w`x`h`.
pub fn decoded_bytes(w: u32, h: u32) -> u64 {
    w as u64 * h as u64 * 4
}
