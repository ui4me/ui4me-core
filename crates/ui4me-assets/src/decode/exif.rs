//! owner: 007-resources (T032) — EXIF orientation (`image-orientation: from-image`).
//!
//! An owned minimal parser (the `image` accessor is unavailable without X-ACQ): it reads only the
//! orientation tag 0x0112 from IFD0 of a TIFF block, found in JPEG APP1, PNG `eXIf` and WebP `EXIF`.

use super::DecodedImage;

fn rd16(b: &[u8], o: usize, le: bool) -> Option<u16> {
    let s = b.get(o..o + 2)?;
    Some(if le {
        u16::from_le_bytes([s[0], s[1]])
    } else {
        u16::from_be_bytes([s[0], s[1]])
    })
}
fn rd32(b: &[u8], o: usize, le: bool) -> Option<u32> {
    let s = b.get(o..o + 4)?;
    Some(if le {
        u32::from_le_bytes([s[0], s[1], s[2], s[3]])
    } else {
        u32::from_be_bytes([s[0], s[1], s[2], s[3]])
    })
}

/// Orientation 1..=8 from a TIFF block (`II*\0` / `MM\0*`), or `None` when absent or invalid.
pub fn tiff_orientation(t: &[u8]) -> Option<u8> {
    let le = match t.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if rd16(t, 2, le)? != 42 {
        return None;
    }
    let ifd = rd32(t, 4, le)? as usize;
    let n = rd16(t, ifd, le)? as usize;
    for i in 0..n.min(512) {
        let e = ifd + 2 + i * 12;
        if rd16(t, e, le)? == 0x0112 {
            let v = rd16(t, e + 8, le)?;
            return (1..=8).contains(&v).then_some(v as u8);
        }
    }
    None
}

/// Apply an EXIF orientation to premultiplied RGBA8. Orientation 5..=8 swaps the dimensions.
pub fn apply_orientation(img: DecodedImage, orientation: u8) -> DecodedImage {
    if orientation <= 1 || orientation > 8 {
        return img;
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let (nw, nh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; img.data.len()];
    for y in 0..h {
        for x in 0..w {
            // destination coordinates for the source pixel (x, y)
            let (dx, dy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                _ => (y, w - 1 - x),
            };
            let s = (y * w + x) * 4;
            let d = (dy * nw + dx) * 4;
            out[d..d + 4].copy_from_slice(&img.data[s..s + 4]);
        }
    }
    DecodedImage {
        width: nw as u32,
        height: nh as u32,
        data: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiff(le: bool, orientation: u16) -> Vec<u8> {
        let mut t = if le {
            b"II*\0".to_vec()
        } else {
            b"MM\0*".to_vec()
        };
        let p16 = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let p32 = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        t.extend_from_slice(&p32(8));
        t.extend_from_slice(&p16(1));
        t.extend_from_slice(&p16(0x0112));
        t.extend_from_slice(&p16(3));
        t.extend_from_slice(&p32(1));
        t.extend_from_slice(&p16(orientation));
        t.extend_from_slice(&[0, 0]);
        t
    }

    #[test]
    fn reads_orientation_in_both_byte_orders() {
        for le in [true, false] {
            for o in 1..=8u16 {
                assert_eq!(tiff_orientation(&tiff(le, o)), Some(o as u8));
            }
            assert_eq!(tiff_orientation(&tiff(le, 9)), None);
        }
        assert_eq!(tiff_orientation(b"junk"), None);
    }

    fn img2x3() -> DecodedImage {
        // 2 wide, 3 tall; pixel value = index in the first byte.
        let mut data = Vec::new();
        for i in 0..6u8 {
            data.extend_from_slice(&[i, 0, 0, 255]);
        }
        DecodedImage {
            width: 2,
            height: 3,
            data,
        }
    }

    fn order(i: &DecodedImage) -> Vec<u8> {
        i.data.chunks(4).map(|p| p[0]).collect()
    }

    #[test]
    fn orientation_transforms() {
        // rows: [0 1] [2 3] [4 5]
        assert_eq!(order(&apply_orientation(img2x3(), 1)), [0, 1, 2, 3, 4, 5]);
        assert_eq!(order(&apply_orientation(img2x3(), 2)), [1, 0, 3, 2, 5, 4]);
        assert_eq!(order(&apply_orientation(img2x3(), 3)), [5, 4, 3, 2, 1, 0]);
        assert_eq!(order(&apply_orientation(img2x3(), 4)), [4, 5, 2, 3, 0, 1]);
        let r6 = apply_orientation(img2x3(), 6);
        assert_eq!((r6.width, r6.height), (3, 2));
        assert_eq!(order(&r6), [4, 2, 0, 5, 3, 1]);
        let r8 = apply_orientation(img2x3(), 8);
        assert_eq!(order(&r8), [1, 3, 5, 0, 2, 4]);
        let r5 = apply_orientation(img2x3(), 5);
        assert_eq!(order(&r5), [0, 2, 4, 1, 3, 5]);
        let r7 = apply_orientation(img2x3(), 7);
        assert_eq!(order(&r7), [5, 3, 1, 4, 2, 0]);
    }
}
