//! owner: 007-resources (T070) — the 65,535-px rule.
//!
//! When a side exceeds 65,535 px but the image is within the pixel budget, the raster is
//! downscaled with its aspect ratio preserved by an area (box) average on premultiplied RGBA8.
//! The natural size reported to layout is the original (the caller keeps it).

use super::DecodedImage;
use crate::policy::MAX_RASTER_SIDE;

/// Target size when the longer side exceeds `max_side`: that side becomes `max_side`, the other is
/// scaled proportionally (at least 1).
pub fn target_size(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    if w <= max_side && h <= max_side {
        return (w, h);
    }
    if w >= h {
        (
            max_side,
            ((h as u64 * max_side as u64) / w as u64).max(1) as u32,
        )
    } else {
        (
            ((w as u64 * max_side as u64) / h as u64).max(1) as u32,
            max_side,
        )
    }
}

/// Downscale to the target size, if needed. Returns the image and whether it was downscaled.
pub fn downscale_if_needed(img: DecodedImage, max_side: u32) -> (DecodedImage, bool) {
    let (nw, nh) = target_size(img.width, img.height, max_side);
    if (nw, nh) == (img.width, img.height) {
        return (img, false);
    }
    (box_resize(&img, nw, nh), true)
}

pub fn max_side() -> u32 {
    MAX_RASTER_SIDE
}

/// Area average to `nw`x`nh` (both <= source).
pub fn box_resize(src: &DecodedImage, nw: u32, nh: u32) -> DecodedImage {
    let (w, h) = (src.width as u64, src.height as u64);
    let mut out = vec![0u8; nw as usize * nh as usize * 4];
    for dy in 0..nh as u64 {
        let y0 = dy * h / nh as u64;
        let y1 = (((dy + 1) * h).div_ceil(nh as u64)).clamp(y0 + 1, h);
        for dx in 0..nw as u64 {
            let x0 = dx * w / nw as u64;
            let x1 = (((dx + 1) * w).div_ceil(nw as u64)).clamp(x0 + 1, w);
            let mut acc = [0u64; 4];
            for y in y0..y1 {
                let row = (y * w) as usize * 4;
                for x in x0..x1 {
                    let p = row + x as usize * 4;
                    for c in 0..4 {
                        acc[c] += src.data[p + c] as u64;
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            let d = (dy as usize * nw as usize + dx as usize) * 4;
            for c in 0..4 {
                out[d + c] = ((acc[c] + n / 2) / n) as u8;
            }
        }
    }
    DecodedImage {
        width: nw,
        height: nh,
        data: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_sizes_keep_the_ratio() {
        assert_eq!(target_size(100, 50, 65_535), (100, 50));
        assert_eq!(target_size(70_000, 10, 65_535), (65_535, 9));
        assert_eq!(target_size(10, 131_070, 65_535), (5, 65_535));
        assert_eq!(target_size(200, 100, 100), (100, 50));
    }

    #[test]
    fn box_average_of_uniform_and_checker() {
        let uni = DecodedImage {
            width: 4,
            height: 4,
            data: [10, 20, 30, 255].repeat(16),
        };
        let r = box_resize(&uni, 2, 2);
        assert!(r.data.chunks(4).all(|p| p == [10, 20, 30, 255]));
        let mut d = Vec::new();
        for i in 0..4 {
            d.extend_from_slice(&if i % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [254, 254, 254, 255]
            });
        }
        let r = box_resize(
            &DecodedImage {
                width: 4,
                height: 1,
                data: d,
            },
            2,
            1,
        );
        assert_eq!(r.data, vec![127, 127, 127, 255, 127, 127, 127, 255]);
    }

    #[test]
    fn small_images_are_untouched() {
        let img = DecodedImage {
            width: 2,
            height: 2,
            data: vec![1; 16],
        };
        let (o, down) = downscale_if_needed(img.clone(), 65_535);
        assert!(!down);
        assert_eq!(o, img);
        let (o, down) = downscale_if_needed(img, 1);
        assert!(down);
        assert_eq!((o.width, o.height), (1, 1));
    }
}
