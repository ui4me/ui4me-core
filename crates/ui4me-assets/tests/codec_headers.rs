// owner: 007-resources (T017, T029, T032) — sniffing and owned header probes on the real codec
// containers of verification/fixtures/resources-images/natural-sizes/codecs.
//
// Decoding these needs a codec adapter (X-ACQ, see ERRATA E-007-001); headers do not: dimensions,
// frame count, EXIF orientation and ICC presence are read without any codec.

use std::path::{Path, PathBuf};
use ui4me_assets::decode::ImageFormat;
use ui4me_assets::decode::sniff::{self, Detected};
use ui4me_engine_types::codes_generated::Code;

fn codec(name: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/resources-images/natural-sizes/codecs")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn probe(name: &str) -> ui4me_assets::decode::ImageHeader {
    let b = codec(name);
    match sniff::detect(&b, false).unwrap() {
        Detected::Raster(f) => sniff::probe_header(f, &b).unwrap(),
        Detected::Svg => panic!("svg"),
    }
}

#[test]
fn png_jpeg_gif_bmp_webp_headers() {
    let h = probe("png-200x100.png");
    assert_eq!(
        (h.format, h.width, h.height, h.animated, h.orientation),
        (ImageFormat::Png, 200, 100, false, 1)
    );
    let h = probe("png16-8x8.png");
    assert_eq!((h.width, h.height), (8, 8));
    let h = probe("bmp-32x16.bmp");
    assert_eq!((h.format, h.width, h.height), (ImageFormat::Bmp, 32, 16));
    let h = probe("gif-animated-24x24.gif");
    assert_eq!(
        (h.format, h.width, h.height, h.animated),
        (ImageFormat::Gif, 24, 24, true)
    );
    for (n, w, hgt, anim) in [
        ("webp-lossy-64x32.webp", 64, 32, false),
        ("webp-lossless-64x32.webp", 64, 32, false),
        ("webp-alpha-64x32.webp", 64, 32, false),
        ("webp-animated-32x16.webp", 32, 16, true),
    ] {
        let h = probe(n);
        assert_eq!(
            (h.format, h.width, h.height, h.animated),
            (ImageFormat::WebP, w, hgt, anim),
            "{n}"
        );
    }
}

#[test]
fn apng_is_animated_and_exif_orientation_swaps_the_natural_size() {
    assert!(probe("apng-anim.png").animated);
    let h = probe("jpeg-progressive-exif6-120x80.jpg");
    assert_eq!(
        (h.format, h.width, h.height, h.orientation),
        (ImageFormat::Jpeg, 120, 80, 6)
    );
    assert_eq!(
        h.oriented_size(),
        (80, 120),
        "orientation 6 rotates the image"
    );
    let h = probe("jpeg-icc-64x48.jpg");
    assert!(h.has_icc);
    assert_eq!((h.width, h.height, h.orientation), (64, 48, 1));
}

#[test]
fn unsupported_truncated_and_text_files() {
    assert_eq!(
        sniff::detect(&codec("unsupported-tiff-16x16.tiff"), false)
            .unwrap_err()
            .code,
        Code::UnsupportedMediaType
    );
    assert_eq!(
        sniff::detect(&codec("unsupported-avif.avif"), false)
            .unwrap_err()
            .code,
        Code::UnsupportedMediaType
    );
    assert_eq!(
        sniff::detect(&codec("text-named.png"), false)
            .unwrap_err()
            .code,
        Code::UnsupportedMediaType
    );
    let b = codec("truncated.png");
    let Detected::Raster(f) = sniff::detect(&b, false).unwrap() else {
        panic!()
    };
    // the PNG signature and IHDR are present in the first 60 bytes: the header is still readable
    assert_eq!(sniff::probe_header(f, &b).unwrap().width, 200);
}
