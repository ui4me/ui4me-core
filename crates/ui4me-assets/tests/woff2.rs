// owner: 007-resources — 015 TXT-1 (D-LINL-05): WOFF 2.0 decoding. Real files made offline from the pinned
// OFL test fonts with fontTools (`tests/data/woff2/generate.py`, provenance in `provenance.json`) are decoded
// and compared with their TrueType originals through skrifa: every glyph outline (default and variable
// instance), advance, left side bearing and bounds. Malformed, truncated, mutated and bomb inputs end in
// canonical codes, never a panic.

use skrifa::instance::Size;
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::path::PathBuf;
use ui4me_assets::fonts::validate::{MAX_DECODED_FONT_BYTES, prepare_face_font, validate_font};
use ui4me_assets::fonts::woff2::woff2_to_sfnt;
use ui4me_engine_types::codes_generated::Code;

fn data(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/woff2")
            .join(name),
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[derive(Default, PartialEq, Debug)]
struct Rec(Vec<String>);
impl OutlinePen for Rec {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(format!("M{x},{y}"));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(format!("L{x},{y}"));
    }
    fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.0.push(format!("Q{a},{b},{x},{y}"));
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.0.push(format!("C{a},{b},{c},{d},{x},{y}"));
    }
    fn close(&mut self) {
        self.0.push("Z".into());
    }
}

/// Compare every glyph; returns the number of glyphs that have an outline.
fn compare(original: &[u8], decoded: &[u8], location: &[(&str, f32)]) -> usize {
    let (a, b) = (
        FontRef::new(original).unwrap(),
        FontRef::new(decoded).unwrap(),
    );
    let n = a.maxp().unwrap().num_glyphs();
    assert_eq!(n, b.maxp().unwrap().num_glyphs());
    let (la, lb) = (
        a.axes().location(location.iter().copied()),
        b.axes().location(location.iter().copied()),
    );
    let (ma, mb) = (
        a.glyph_metrics(Size::unscaled(), &la),
        b.glyph_metrics(Size::unscaled(), &lb),
    );
    let mut with_outline = 0;
    for g in 0..u32::from(n) {
        let gid = GlyphId::new(g);
        let (oa, ob) = (a.outline_glyphs().get(gid), b.outline_glyphs().get(gid));
        assert_eq!(oa.is_some(), ob.is_some(), "glyph {g}");
        if let (Some(oa), Some(ob)) = (oa, ob) {
            let (mut ra, mut rb) = (Rec::default(), Rec::default());
            oa.draw(DrawSettings::unhinted(Size::unscaled(), &la), &mut ra)
                .unwrap();
            ob.draw(DrawSettings::unhinted(Size::unscaled(), &lb), &mut rb)
                .unwrap();
            assert_eq!(ra, rb, "outline of glyph {g}");
            with_outline += usize::from(!ra.0.is_empty());
        }
        assert_eq!(
            ma.advance_width(gid),
            mb.advance_width(gid),
            "advance of glyph {g}"
        );
        assert_eq!(
            ma.left_side_bearing(gid),
            mb.left_side_bearing(gid),
            "left side bearing of glyph {g}"
        );
        assert_eq!(ma.bounds(gid), mb.bounds(gid), "bounds of glyph {g}");
    }
    with_outline
}

/// (composite glyphs, glyphs whose first flag has the overlap bit) read from the raw `glyf` / `loca`.
fn glyph_kinds(sfnt: &[u8]) -> (usize, usize) {
    let be16 = |o: usize| u16::from_be_bytes([sfnt[o], sfnt[o + 1]]) as usize;
    let be32 =
        |o: usize| u32::from_be_bytes([sfnt[o], sfnt[o + 1], sfnt[o + 2], sfnt[o + 3]]) as usize;
    let table = |tag: &[u8; 4]| {
        (0..be16(4))
            .map(|i| 12 + 16 * i)
            .find(|&r| &sfnt[r..r + 4] == tag)
            .map(|r| (be32(r + 8), be32(r + 12)))
            .unwrap()
    };
    let (glyf, _) = table(b"glyf");
    let (loca, _) = table(b"loca");
    let (head, _) = table(b"head");
    let (maxp, _) = table(b"maxp");
    let (long, n) = (be16(head + 50) != 0, be16(maxp + 4));
    let off = |i: usize| {
        if long {
            be32(loca + 4 * i)
        } else {
            2 * be16(loca + 2 * i)
        }
    };
    let (mut composites, mut overlaps) = (0, 0);
    for g in 0..n {
        let (a, z) = (off(g), off(g + 1));
        if z - a < 10 {
            continue;
        }
        let nc = be16(glyf + a) as i16;
        if nc < 0 {
            composites += 1;
        } else {
            let instr = be16(glyf + a + 10 + 2 * nc as usize);
            let first_flag = sfnt[glyf + a + 10 + 2 * nc as usize + 2 + instr];
            overlaps += usize::from(first_flag & 0x40 != 0);
        }
    }
    (composites, overlaps)
}

#[test]
fn noto_sans_subset_decodes_to_the_same_font_in_every_transform_mode() {
    let ttf = data("noto-sans-subset.ttf");
    for name in [
        "noto-sans-subset.woff2",
        "noto-sans-hmtx.woff2",
        "noto-sans-nulltransform.woff2",
    ] {
        let w = data(name);
        let sfnt =
            woff2_to_sfnt(&w, MAX_DECODED_FONT_BYTES).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(validate_font(&sfnt).is_ok(), "{name}");
        let n = compare(&ttf, &sfnt, &[]);
        assert!(n > 150, "{name}: {n} glyphs with outlines");
        let (composites, _) = glyph_kinds(&sfnt);
        assert!(
            composites > 20,
            "{name}: composite glyphs are part of the comparison ({composites})"
        );
        // the product entry point gives the same bytes
        assert_eq!(prepare_face_font(w).unwrap(), sfnt, "{name}");
    }
}

#[test]
fn variable_roboto_flex_decodes_with_overlaps_and_variations_intact() {
    let ttf = data("roboto-flex-subset.ttf");
    let sfnt = woff2_to_sfnt(&data("roboto-flex-subset.woff2"), MAX_DECODED_FONT_BYTES).unwrap();
    let (composites, overlaps) = glyph_kinds(&sfnt);
    assert!(
        composites > 10 && overlaps > 10,
        "composites {composites}, overlap-flagged {overlaps}"
    );
    assert!(compare(&ttf, &sfnt, &[]) > 150);
    // outlines at non-default instances come out of the untouched gvar / avar tables
    assert!(compare(&ttf, &sfnt, &[("wght", 800.0), ("wdth", 80.0)]) > 150);
    assert!(compare(&ttf, &sfnt, &[("wght", 150.0), ("opsz", 30.0)]) > 150);
}

#[test]
fn decoding_is_deterministic() {
    let w = data("noto-sans-subset.woff2");
    assert_eq!(
        woff2_to_sfnt(&w, MAX_DECODED_FONT_BYTES).unwrap(),
        woff2_to_sfnt(&w, MAX_DECODED_FONT_BYTES).unwrap()
    );
}

#[test]
fn collections_are_unsupported_media_type() {
    let mut w = data("noto-sans-subset.woff2");
    w[4..8].copy_from_slice(b"ttcf");
    assert_eq!(
        woff2_to_sfnt(&w, MAX_DECODED_FONT_BYTES).unwrap_err().code,
        Code::UnsupportedMediaType
    );
}

#[test]
fn malformed_headers_and_directories_are_decode_failed() {
    let good = data("noto-sans-subset.woff2");
    let code = |w: &[u8]| woff2_to_sfnt(w, MAX_DECODED_FONT_BYTES).unwrap_err().code;
    // truncations everywhere, including inside the header and the directory
    for cut in [
        0,
        3,
        4,
        20,
        47,
        48,
        49,
        60,
        100,
        good.len() / 2,
        good.len() - 1,
    ] {
        assert_eq!(code(&good[..cut]), Code::DecodeFailed, "cut at {cut}");
    }
    let patched = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut w = good.clone();
        f(&mut w);
        w
    };
    assert_eq!(
        code(&patched(&|w| w[0] = b'x')),
        Code::DecodeFailed,
        "signature"
    );
    assert_eq!(
        code(&patched(&|w| w[11] ^= 1)),
        Code::DecodeFailed,
        "length field"
    );
    assert_eq!(
        code(&patched(&|w| w[12..14].copy_from_slice(&0u16.to_be_bytes()))),
        Code::DecodeFailed,
        "zero tables"
    );
    assert_eq!(
        code(&patched(
            &|w| w[12..14].copy_from_slice(&5000u16.to_be_bytes())
        )),
        Code::DecodeFailed,
        "too many tables"
    );
    assert_eq!(
        code(&patched(
            &|w| w[20..24].copy_from_slice(&u32::MAX.to_be_bytes())
        )),
        Code::DecodeFailed,
        "compressed size past the file"
    );
    assert_eq!(
        code(&patched(
            &|w| w[16..20].copy_from_slice(&u32::MAX.to_be_bytes())
        )),
        Code::SizeLimit,
        "advertised sfnt size over the cap"
    );
    // a directory entry with transform version 2 for glyf (flags byte of the first entry whose tag index is glyf)
    assert_eq!(
        code(&patched(&|w| w[48] = (w[48] & 0x3F) | 0x80)),
        Code::DecodeFailed,
        "invalid transform version"
    );
    // garbage in place of the Brotli stream
    let n = good.len();
    assert_eq!(
        code(&patched(&|w| w[n - 200..].fill(0xFF))),
        Code::DecodeFailed,
        "corrupt stream"
    );
}

#[test]
fn mutated_files_never_panic_and_only_give_canonical_codes() {
    // A deterministic stand-in for a fuzz target (engine/fuzz has no target pattern yet): flip bytes of the real
    // files at a spread of positions; every result is a valid sfnt or a DECODE_FAILED / SIZE_LIMIT status.
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for name in [
        "noto-sans-subset.woff2",
        "noto-sans-hmtx.woff2",
        "roboto-flex-subset.woff2",
    ] {
        let good = data(name);
        let (mut ok, mut failed) = (0, 0);
        for i in 0..400 {
            let mut w = good.clone();
            // the first 400 bytes hold the header and directory: half of the flips land there
            let at = if i % 2 == 0 {
                (rnd() as usize) % 400.min(w.len())
            } else {
                (rnd() as usize) % w.len()
            };
            w[at] ^= 1 << (rnd() % 8);
            match woff2_to_sfnt(&w, 16 << 20) {
                Ok(s) => {
                    ok += 1;
                    assert!(s.len() >= 12);
                }
                Err(e) => {
                    failed += 1;
                    assert!(
                        matches!(e.code, Code::DecodeFailed | Code::SizeLimit),
                        "{name} flip at {at}: {e:?}"
                    );
                }
            }
        }
        assert!(
            failed > 100,
            "{name}: {failed} of 400 flips were rejected ({ok} accepted)"
        );
    }
}

#[test]
fn bombs_are_refused_by_size_before_they_are_allocated() {
    // declares 128 MiB: over the font cap, refused from the directory alone
    let e = woff2_to_sfnt(&data("bomb-declared.woff2"), MAX_DECODED_FONT_BYTES).unwrap_err();
    assert_eq!(e.code, Code::SizeLimit, "{e:?}");
    // declares 1000 bytes but the Brotli stream expands to 32 MiB: the read stops one byte past the declaration
    let e = woff2_to_sfnt(&data("bomb-stream.woff2"), MAX_DECODED_FONT_BYTES).unwrap_err();
    assert_eq!(e.code, Code::DecodeFailed, "{e:?}");
    assert!(
        e.message.contains("more than the directory declares"),
        "{e:?}"
    );
    // an honest 48 MiB table: accepted under the 64 MiB cap, SIZE_LIMIT under a smaller one
    assert!(woff2_to_sfnt(&data("bomb-ratio.woff2"), MAX_DECODED_FONT_BYTES).is_ok());
    assert_eq!(
        woff2_to_sfnt(&data("bomb-ratio.woff2"), 16 << 20)
            .unwrap_err()
            .code,
        Code::SizeLimit
    );
    // the product entry point applies the font cap and returns the same codes
    assert_eq!(
        prepare_face_font(data("bomb-declared.woff2"))
            .unwrap_err()
            .code,
        Code::SizeLimit
    );
}
