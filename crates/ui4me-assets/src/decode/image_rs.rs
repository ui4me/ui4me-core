//! owner: 007-resources (T030) — the `image`-crate adapter behind the AD-33 `RasterDecoder` seam.
//!
//! X-ACQ = PASS (`verification/gates/X-ACQ/RESULT.md`): `image` 0.25 with `default-features = false`
//! and the features `png`, `jpeg`, `gif`, `webp`, `bmp`. Policy:
//!
//! * the header is probed with the owned probes (`sniff::probe_header`): the pixel budget is checked
//!   by the pipeline from that probe before `decode` is ever called;
//! * `decode` then sets `image::Limits` to exactly the probed dimensions and a byte budget derived
//!   from them, and refuses a stream whose real header disagrees with the probe;
//! * only the first frame is decoded (GIF, WebP, APNG default image); the pipeline reports
//!   `img.animation` once per resource from the probe;
//! * 16-bit samples are reduced to 8-bit; the output is premultiplied RGBA8 treated as sRGB; ICC
//!   profiles are ignored (the probe reports their presence for the `info` diagnostic);
//! * EXIF orientation is NOT applied here (the pipeline applies it from the probe, T032);
//! * corrupt data is `DECODE_FAILED`, other formats `UNSUPPORTED_MEDIA_TYPE`, exceeded limits
//!   `SIZE_LIMIT`.

use super::{DecodedImage, ImageFormat, ImageHeader, RasterDecoder, decoded_bytes, sniff};
use crate::worker::CancelToken;
use image::{DynamicImage, ImageDecoder, ImageError, Limits};
use std::io::Cursor;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

#[derive(Debug, Default)]
pub struct ImageRsDecoder;

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

fn map_error(e: ImageError) -> Status {
    match e {
        ImageError::Limits(_) => err(Code::SizeLimit, "decoder limit exceeded"),
        ImageError::Unsupported(u) => err(
            Code::UnsupportedMediaType,
            format!("unsupported image feature: {u}"),
        ),
        other => err(Code::DecodeFailed, format!("decode failed: {other}")),
    }
}

fn to_image_format(f: ImageFormat) -> Option<image::ImageFormat> {
    Some(match f {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::WebP => image::ImageFormat::WebP,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        ImageFormat::Fake => return None,
    })
}

impl RasterDecoder for ImageRsDecoder {
    fn formats(&self) -> &'static [ImageFormat] {
        &[
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::WebP,
            ImageFormat::Bmp,
        ]
    }

    fn probe(&self, format: ImageFormat, bytes: &[u8]) -> Result<ImageHeader, Status> {
        sniff::probe_header(format, bytes)
    }

    fn decode(
        &self,
        bytes: &[u8],
        header: &ImageHeader,
        cancel: &CancelToken,
    ) -> Result<DecodedImage, Status> {
        let Some(format) = to_image_format(header.format) else {
            return Err(err(
                Code::UnsupportedMediaType,
                "the image decoder does not decode test containers",
            ));
        };
        if cancel.is_canceled() {
            return Err(err(Code::InvalidState, "canceled"));
        }
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = Limits::no_limits();
        limits.max_image_width = Some(header.width);
        limits.max_image_height = Some(header.height);
        // The output buffer plus a bounded working set; never the library default of 512 MiB.
        limits.max_alloc = Some(
            decoded_bytes(header.width, header.height)
                .saturating_mul(2)
                .saturating_add(1 << 20),
        );
        reader.limits(limits.clone());
        let mut decoder = reader.into_decoder().map_err(map_error)?;
        decoder.set_limits(limits).map_err(map_error)?;
        let (w, h) = decoder.dimensions();
        if (w, h) != (header.width, header.height) {
            return Err(err(
                Code::DecodeFailed,
                "the image header changed between the probe and the decode",
            ));
        }
        let img = DynamicImage::from_decoder(decoder).map_err(map_error)?;
        if cancel.is_canceled() {
            return Err(err(Code::InvalidState, "canceled"));
        }
        let mut data = img.to_rgba8().into_raw();
        premultiply(&mut data);
        Ok(DecodedImage {
            width: w,
            height: h,
            data,
        })
    }
}

/// Straight RGBA8 to premultiplied RGBA8, rounding to nearest.
pub fn premultiply(data: &mut [u8]) {
    for px in data.as_chunks_mut::<4>().0.iter_mut() {
        let a = px[3] as u32;
        if a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_rounds_to_nearest() {
        let mut p = [255, 128, 0, 128, 10, 20, 30, 255, 200, 200, 200, 0];
        premultiply(&mut p);
        assert_eq!(p, [128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }
}
