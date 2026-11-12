//! owner: 007-resources (T029) — MIME Sniffing image patterns and owned header probes.
//!
//! Extensions and declared types are ignored for rasters (`uri-resolution.md` §4). SVG is chosen
//! by the caller from the extension or MIME. The owned probes read dimensions, frame count,
//! orientation and ICC presence from headers alone, so the pixel budget is enforced before any
//! decoder allocates (`resource-policy.md` §4).

use super::{ImageFormat, ImageHeader, exif};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

fn truncated(what: &str) -> Status {
    err(
        Code::DecodeFailed,
        format!("truncated or corrupt {what} header"),
    )
}

/// MIME Sniffing image signatures (PNG, JPEG, GIF, WebP, BMP) plus the fake test container.
pub fn sniff_raster(b: &[u8]) -> Option<ImageFormat> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(ImageFormat::Png)
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if b.len() >= 14 && &b[..4] == b"RIFF" && &b[8..14] == b"WEBPVP" {
        Some(ImageFormat::WebP)
    } else if b.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else if b.starts_with(super::fake::MAGIC) {
        Some(ImageFormat::Fake)
    } else {
        None
    }
}

pub fn is_gzip(b: &[u8]) -> bool {
    b.starts_with(&[0x1F, 0x8B])
}

/// How the media type of an image resource was decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detected {
    Raster(ImageFormat),
    Svg,
}

/// Decide the media type: gzip magic is `UNSUPPORTED_MEDIA_TYPE` (no SVGZ); `svg_hint` (extension or
/// declared MIME says SVG) wins for SVG; otherwise the signature decides; no match is
/// `UNSUPPORTED_MEDIA_TYPE`.
pub fn detect(bytes: &[u8], svg_hint: bool) -> Result<Detected, Status> {
    if is_gzip(bytes) {
        return Err(err(
            Code::UnsupportedMediaType,
            "gzip-compressed content (SVGZ) is unsupported",
        ));
    }
    if let Some(f) = sniff_raster(bytes) {
        return Ok(Detected::Raster(f));
    }
    if svg_hint {
        return Ok(Detected::Svg);
    }
    Err(err(Code::UnsupportedMediaType, "unrecognised image format"))
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
fn be16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
fn le32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn le16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

/// Owned header probe for a sniffed format. Corrupt or truncated headers are `DECODE_FAILED`.
pub fn probe_header(format: ImageFormat, b: &[u8]) -> Result<ImageHeader, Status> {
    let mut h = match format {
        ImageFormat::Png => probe_png(b)?,
        ImageFormat::Jpeg => probe_jpeg(b)?,
        ImageFormat::Gif => probe_gif(b)?,
        ImageFormat::WebP => probe_webp(b)?,
        ImageFormat::Bmp => probe_bmp(b)?,
        ImageFormat::Fake => return super::fake::FakeDecoder.probe(ImageFormat::Fake, b),
    };
    if h.width == 0 || h.height == 0 {
        return Err(err(Code::DecodeFailed, "image has a zero dimension"));
    }
    h.orientation = h.orientation.clamp(1, 8);
    Ok(h)
}

use super::RasterDecoder;

fn base(format: ImageFormat, width: u32, height: u32) -> ImageHeader {
    ImageHeader {
        format,
        width,
        height,
        orientation: 1,
        animated: false,
        has_icc: false,
    }
}

fn probe_png(b: &[u8]) -> Result<ImageHeader, Status> {
    if b.get(12..16) != Some(b"IHDR") {
        return Err(truncated("PNG"));
    }
    let (w, h) = (
        be32(b, 16).ok_or_else(|| truncated("PNG"))?,
        be32(b, 20).ok_or_else(|| truncated("PNG"))?,
    );
    let mut hd = base(ImageFormat::Png, w, h);
    // Walk chunks up to IDAT for acTL (APNG), iCCP and eXIf.
    let mut o = 8;
    while let (Some(len), Some(ty)) = (be32(b, o), b.get(o + 4..o + 8)) {
        match ty {
            b"acTL" => hd.animated = true,
            b"iCCP" => hd.has_icc = true,
            b"eXIf" => {
                if let Some(d) = b.get(o + 8..o + 8 + len as usize) {
                    hd.orientation = exif::tiff_orientation(d).unwrap_or(1);
                }
            }
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        o = o.saturating_add(12).saturating_add(len as usize);
        if o > b.len() {
            break;
        }
    }
    Ok(hd)
}

fn probe_jpeg(b: &[u8]) -> Result<ImageHeader, Status> {
    let mut o = 2;
    let mut hd = None;
    let (mut orientation, mut icc) = (1u8, false);
    while o + 4 <= b.len() {
        if b[o] != 0xFF {
            o += 1;
            continue;
        }
        let m = b[o + 1];
        if m == 0xFF {
            o += 1;
            continue;
        }
        if m == 0xD8 || (0xD0..=0xD7).contains(&m) || m == 0x01 || m == 0x00 {
            o += 2;
            continue;
        }
        let len = be16(b, o + 2).ok_or_else(|| truncated("JPEG"))? as usize;
        let seg = b.get(o + 4..o + 2 + len);
        match m {
            0xE1 => {
                if let Some(s) = seg.filter(|s| s.starts_with(b"Exif\0\0")) {
                    orientation = exif::tiff_orientation(&s[6..]).unwrap_or(1);
                }
            }
            0xE2 => icc |= seg.is_some_and(|s| s.starts_with(b"ICC_PROFILE\0")),
            0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) => {
                let h = be16(b, o + 5).ok_or_else(|| truncated("JPEG"))? as u32;
                let w = be16(b, o + 7).ok_or_else(|| truncated("JPEG"))? as u32;
                hd = Some((w, h));
                break;
            }
            0xDA => break,
            _ => {}
        }
        o += 2 + len;
    }
    let (w, h) = hd.ok_or_else(|| truncated("JPEG"))?;
    let mut r = base(ImageFormat::Jpeg, w, h);
    r.orientation = orientation;
    r.has_icc = icc;
    Ok(r)
}

fn probe_gif(b: &[u8]) -> Result<ImageHeader, Status> {
    let w = le16(b, 6).ok_or_else(|| truncated("GIF"))? as u32;
    let h = le16(b, 8).ok_or_else(|| truncated("GIF"))? as u32;
    let mut r = base(ImageFormat::Gif, w, h);
    let flags = *b.get(10).ok_or_else(|| truncated("GIF"))?;
    let mut o = 13usize;
    if flags & 0x80 != 0 {
        o += 3 * (1usize << ((flags & 7) + 1));
    }
    let mut frames = 0;
    while let Some(&tag) = b.get(o) {
        match tag {
            0x21 => {
                o += 2;
                o = skip_sub_blocks(b, o);
            }
            0x2C => {
                frames += 1;
                if frames > 1 {
                    r.animated = true;
                    break;
                }
                let lf = *b.get(o + 9).ok_or_else(|| truncated("GIF"))?;
                o += 10;
                if lf & 0x80 != 0 {
                    o += 3 * (1usize << ((lf & 7) + 1));
                }
                o += 1;
                o = skip_sub_blocks(b, o);
            }
            _ => break,
        }
    }
    Ok(r)
}

fn skip_sub_blocks(b: &[u8], mut o: usize) -> usize {
    while let Some(&n) = b.get(o) {
        o += 1 + n as usize;
        if n == 0 {
            break;
        }
    }
    o
}

fn probe_bmp(b: &[u8]) -> Result<ImageHeader, Status> {
    let dib = le32(b, 14).ok_or_else(|| truncated("BMP"))?;
    let (w, h) = if dib == 12 {
        (
            le16(b, 18).ok_or_else(|| truncated("BMP"))? as u32,
            le16(b, 20).ok_or_else(|| truncated("BMP"))? as u32,
        )
    } else {
        let w = le32(b, 18).ok_or_else(|| truncated("BMP"))? as i32;
        let h = le32(b, 22).ok_or_else(|| truncated("BMP"))? as i32;
        (w.unsigned_abs(), h.unsigned_abs())
    };
    Ok(base(ImageFormat::Bmp, w, h))
}

fn probe_webp(b: &[u8]) -> Result<ImageHeader, Status> {
    let fourcc = b.get(12..16).ok_or_else(|| truncated("WebP"))?;
    let d = 20usize;
    let (w, h, mut animated, mut icc) = match fourcc {
        b"VP8 " => {
            if b.get(d + 3..d + 6) != Some(&[0x9D, 0x01, 0x2A]) {
                return Err(truncated("WebP"));
            }
            (
                (le16(b, d + 6).ok_or_else(|| truncated("WebP"))? & 0x3FFF) as u32,
                (le16(b, d + 8).ok_or_else(|| truncated("WebP"))? & 0x3FFF) as u32,
                false,
                false,
            )
        }
        b"VP8L" => {
            if b.get(d) != Some(&0x2F) {
                return Err(truncated("WebP"));
            }
            let v = le32(b, d + 1).ok_or_else(|| truncated("WebP"))?;
            ((v & 0x3FFF) + 1, ((v >> 14) & 0x3FFF) + 1, false, false)
        }
        b"VP8X" => {
            let f = *b.get(d).ok_or_else(|| truncated("WebP"))?;
            let w = b.get(d + 4..d + 7).ok_or_else(|| truncated("WebP"))?;
            let h = b.get(d + 7..d + 10).ok_or_else(|| truncated("WebP"))?;
            let w = (w[0] as u32 | (w[1] as u32) << 8 | (w[2] as u32) << 16) + 1;
            let h = (h[0] as u32 | (h[1] as u32) << 8 | (h[2] as u32) << 16) + 1;
            (w, h, f & 0x02 != 0, f & 0x20 != 0)
        }
        _ => return Err(truncated("WebP")),
    };
    let mut r = base(ImageFormat::WebP, w, h);
    if fourcc == b"VP8X" {
        // EXIF chunk orientation, if present.
        let mut o = 12usize;
        while let (Some(ty), Some(len)) = (b.get(o..o + 4), le32(b, o + 4)) {
            if ty == b"EXIF" {
                if let Some(data) = b.get(o + 8..o + 8 + len as usize) {
                    let data = data.strip_prefix(b"Exif\0\0").unwrap_or(data);
                    r.orientation = exif::tiff_orientation(data).unwrap_or(1);
                }
            }
            if ty == b"ICCP" {
                icc = true;
            }
            if ty == b"ANMF" {
                animated = true;
            }
            o = o.saturating_add(8).saturating_add((len as usize + 1) & !1);
        }
    }
    r.animated = animated;
    r.has_icc = icc;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        v
    }

    #[test]
    fn sniffs_signatures_ignoring_names() {
        assert_eq!(sniff_raster(&png(2, 3)), Some(ImageFormat::Png));
        assert_eq!(
            sniff_raster(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(sniff_raster(b"GIF89a....."), Some(ImageFormat::Gif));
        assert_eq!(
            sniff_raster(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::WebP)
        );
        assert_eq!(sniff_raster(b"BM...."), Some(ImageFormat::Bmp));
        assert_eq!(sniff_raster(b"hello, this is text"), None);
        assert_eq!(sniff_raster(b"II*\0"), None, "TIFF is not supported");
        assert!(detect(&[0x1F, 0x8B, 8], true).is_err());
        assert_eq!(detect(b"<svg/>", true).unwrap(), Detected::Svg);
        assert_eq!(
            detect(b"<svg/>", false).unwrap_err().code,
            Code::UnsupportedMediaType
        );
    }

    #[test]
    fn png_header_is_read_without_the_body() {
        let h = probe_header(ImageFormat::Png, &png(50_000, 50_000)).unwrap();
        assert_eq!((h.width, h.height), (50_000, 50_000));
        assert_eq!(
            probe_header(ImageFormat::Png, &png(0, 5)).unwrap_err().code,
            Code::DecodeFailed
        );
        assert_eq!(
            probe_header(ImageFormat::Png, &png(1, 1)[..10])
                .unwrap_err()
                .code,
            Code::DecodeFailed
        );
    }

    #[test]
    fn bmp_and_gif_headers() {
        let mut bmp = vec![0u8; 30];
        bmp[0] = b'B';
        bmp[1] = b'M';
        bmp[14] = 40;
        bmp[18..22].copy_from_slice(&7i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-9i32).to_le_bytes());
        let h = probe_header(ImageFormat::Bmp, &bmp).unwrap();
        assert_eq!((h.width, h.height), (7, 9));
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[4, 0, 5, 0, 0, 0, 0]);
        gif.extend_from_slice(&[
            0x2C, 0, 0, 0, 0, 4, 0, 5, 0, 0, 2, 1, 0x44, 0, 0x2C, 0, 0, 0, 0, 4, 0, 5, 0, 0, 2, 1,
            0x44, 0, 0x3B,
        ]);
        let h = probe_header(ImageFormat::Gif, &gif).unwrap();
        assert_eq!((h.width, h.height, h.animated), (4, 5, true));
    }
}
