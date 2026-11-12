//! owner: 007-resources (T056) — font validation, run on a worker for `@font-face` loads and
//! synchronously for host registration.
//!
//! Only TrueType/OpenType (TTF, OTF) and TTC collections are accepted by [`validate_font`] (FR-046;
//! host registration). The check is the sfnt / collection header and the table-directory bounds; it
//! does not parse tables. WOFF, WOFF2, EOT and SVG fonts are `UNSUPPORTED_MEDIA_TYPE`; any other
//! invalid data is `DECODE_FAILED`.
//!
//! `@font-face` sources go through [`prepare_face_font`] instead (L-FONTFACE-r12): a **WOFF 1.0**
//! file (zlib-compressed tables, W3C WOFF 1.0 section 5) is decompressed into the equivalent sfnt and
//! then validated; a **WOFF 2.0** file is decoded by [`super::woff2`] (Brotli, `glyf`/`loca` and `hmtx`
//! transforms; 015 TXT-1; font collections are `UNSUPPORTED_MEDIA_TYPE`); EOT and SVG fonts are refused.
//! Host registration ([`validate_font`]) still takes only TTF, OTF and TTC.

use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontContainer {
    TrueType,
    OpenTypeCff,
    Collection { fonts: u32 },
}

fn err(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
fn be16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

/// Container formats that are recognised and refused.
pub fn unsupported_container(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"wOFF") {
        Some("WOFF")
    } else if b.starts_with(b"wOF2") {
        Some("WOFF2")
    } else if b.len() > 36 && &b[34..36] == b"LP" {
        Some("EOT")
    } else if b.starts_with(b"<svg") || b.starts_with(b"<?xml") {
        Some("SVG font")
    } else {
        None
    }
}

/// `format()` hints this engine can load: `truetype`, `opentype`, `collection`, `woff` (WOFF 1.0) and
/// `woff2`, with the legacy `-variations` spellings (CSS Fonts 4 section 4.3.1). Anything else
/// (`embedded-opentype`, `svg`, an unknown string) names a format the engine cannot use, so the `src`
/// entry is skipped without a read.
pub fn is_supported_format_hint(hint: &str) -> bool {
    let h = hint.trim().to_ascii_lowercase();
    let base = h.strip_suffix("-variations").unwrap_or(&h);
    matches!(
        base,
        "truetype" | "opentype" | "collection" | "woff" | "woff2"
    )
}

/// `format()` hints that name a format the engine cannot load: such `src` entries are skipped unread.
pub fn is_unsupported_format_hint(hint: &str) -> bool {
    !is_supported_format_hint(hint)
}

fn check_directory(b: &[u8], base: usize) -> Result<(), Status> {
    let n =
        be16(b, base + 4).ok_or_else(|| err(Code::DecodeFailed, "truncated sfnt header"))? as usize;
    if n == 0 || n > 4096 {
        return Err(err(
            Code::DecodeFailed,
            format!("implausible table count {n}"),
        ));
    }
    let dir_end = base + 12 + 16 * n;
    if dir_end > b.len() {
        return Err(err(Code::DecodeFailed, "table directory exceeds the file"));
    }
    for i in 0..n {
        let rec = base + 12 + 16 * i;
        let off = be32(b, rec + 8)
            .ok_or_else(|| err(Code::DecodeFailed, "truncated table record"))?
            as u64;
        let len = be32(b, rec + 12)
            .ok_or_else(|| err(Code::DecodeFailed, "truncated table record"))?
            as u64;
        if off < dir_end as u64 || off + len > b.len() as u64 {
            let tag = String::from_utf8_lossy(&b[rec..rec + 4]).into_owned();
            return Err(err(
                Code::DecodeFailed,
                format!("table {tag:?} lies outside the file"),
            ));
        }
    }
    Ok(())
}

/// Validate font bytes: the sfnt/TTC header and table-directory bounds.
pub fn validate_font(b: &[u8]) -> Result<FontContainer, Status> {
    if let Some(what) = unsupported_container(b) {
        return Err(err(
            Code::UnsupportedMediaType,
            format!("{what} fonts are not supported"),
        ));
    }
    match b.get(..4) {
        Some([0, 1, 0, 0]) | Some(b"true") => {
            check_directory(b, 0).map(|()| FontContainer::TrueType)
        }
        Some(b"OTTO") => check_directory(b, 0).map(|()| FontContainer::OpenTypeCff),
        Some(b"ttcf") => {
            let n = be32(b, 8).ok_or_else(|| err(Code::DecodeFailed, "truncated TTC header"))?;
            if n == 0 || n > 1024 {
                return Err(err(
                    Code::DecodeFailed,
                    format!("implausible TTC font count {n}"),
                ));
            }
            for i in 0..n as usize {
                let off = be32(b, 12 + 4 * i)
                    .ok_or_else(|| err(Code::DecodeFailed, "truncated TTC offset table"))?
                    as usize;
                if off + 12 > b.len() {
                    return Err(err(
                        Code::DecodeFailed,
                        "TTC member offset outside the file",
                    ));
                }
                check_directory(b, off)?;
            }
            Ok(FontContainer::Collection { fonts: n })
        }
        _ => Err(err(Code::DecodeFailed, "not an OpenType/TrueType font")),
    }
}

/// Largest sfnt a WOFF file may expand to (the font size limit; the policy's smaller limit applies to
/// the encoded file).
pub const MAX_DECODED_FONT_BYTES: u64 = 64 * 1024 * 1024;

/// WOFF 1.0 to sfnt (W3C WOFF 1.0 section 5). Each table is stored as is when `compLength ==
/// origLength` and zlib-compressed otherwise; the sfnt directory is rebuilt in the file's (ascending
/// tag) order with 4-byte-aligned tables. Every length is checked against the file and against
/// `max_decoded` before anything is allocated or inflated, and a table must inflate to exactly its
/// declared length (a decompression bomb ends at the declared size). Table checksums are not verified
/// (the shaper does not use them).
pub fn woff1_to_sfnt(b: &[u8], max_decoded: u64) -> Result<Vec<u8>, Status> {
    use std::io::Read;
    let bad = |m: &str| err(Code::DecodeFailed, format!("WOFF: {m}"));
    if b.get(..4) != Some(b"wOFF") {
        return Err(bad("missing signature"));
    }
    let flavor = be32(b, 4).ok_or_else(|| bad("truncated header"))?;
    let length = be32(b, 8).ok_or_else(|| bad("truncated header"))? as u64;
    let num = be16(b, 12).ok_or_else(|| bad("truncated header"))? as usize;
    let total = be32(b, 16).ok_or_else(|| bad("truncated header"))? as u64;
    if length != b.len() as u64 {
        return Err(bad("header length differs from the file length"));
    }
    if num == 0 || num > 4096 {
        return Err(bad(&format!("implausible table count {num}")));
    }
    let dir_end = 44 + 20 * num;
    if dir_end > b.len() {
        return Err(bad("table directory exceeds the file"));
    }
    // sfnt size: 12 + 16 per table, plus each table padded to 4 bytes.
    let mut tables = Vec::with_capacity(num);
    let mut sfnt_len: u64 = 12 + 16 * num as u64;
    let mut last_tag: Option<[u8; 4]> = None;
    for i in 0..num {
        let rec = 44 + 20 * i;
        let tag = [b[rec], b[rec + 1], b[rec + 2], b[rec + 3]];
        let off = be32(b, rec + 4).ok_or_else(|| bad("truncated table record"))? as u64;
        let comp = be32(b, rec + 8).ok_or_else(|| bad("truncated table record"))? as u64;
        let orig = be32(b, rec + 12).ok_or_else(|| bad("truncated table record"))? as u64;
        if last_tag.is_some_and(|t| t >= tag) {
            return Err(bad("table directory is not in ascending tag order"));
        }
        last_tag = Some(tag);
        if comp > orig {
            return Err(bad(&format!(
                "table {:?} is larger compressed than uncompressed",
                String::from_utf8_lossy(&tag)
            )));
        }
        if !off.is_multiple_of(4) || off < dir_end as u64 || off + comp > b.len() as u64 {
            return Err(bad(&format!(
                "table {:?} lies outside the file",
                String::from_utf8_lossy(&tag)
            )));
        }
        sfnt_len += (orig + 3) & !3;
        if sfnt_len > max_decoded {
            return Err(err(
                Code::SizeLimit,
                format!("WOFF expands to more than {max_decoded} bytes"),
            ));
        }
        tables.push((tag, off as usize, comp as usize, orig as usize));
    }
    if total != 0 && total != sfnt_len {
        return Err(bad("totalSfntSize does not match the table directory"));
    }
    let mut out = Vec::with_capacity(sfnt_len as usize);
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(num as u16).to_be_bytes());
    // searchRange / entrySelector / rangeShift
    let mut pow = 1u32;
    let mut sel = 0u16;
    while pow * 2 <= num as u32 {
        pow *= 2;
        sel += 1;
    }
    let search_range = (pow * 16) as u16;
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&sel.to_be_bytes());
    out.extend_from_slice(&((num as u16 * 16).saturating_sub(search_range)).to_be_bytes());
    let mut data_off = 12 + 16 * num;
    for (i, (tag, woff_off, comp, orig)) in tables.iter().enumerate() {
        let checksum = be32(b, 44 + 20 * i + 16).unwrap_or(0);
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(data_off as u32).to_be_bytes());
        out.extend_from_slice(&(*orig as u32).to_be_bytes());
        data_off += (orig + 3) & !3;
        let _ = (woff_off, comp);
    }
    for (tag, woff_off, comp, orig) in &tables {
        let raw = &b[*woff_off..*woff_off + *comp];
        let start = out.len();
        if comp == orig {
            out.extend_from_slice(raw);
        } else {
            let mut dec = flate2::read::ZlibDecoder::new(raw).take(*orig as u64 + 1);
            dec.read_to_end(&mut out)
                .map_err(|e| bad(&format!("table {:?}: {e}", String::from_utf8_lossy(tag))))?;
            if out.len() - start != *orig {
                return Err(bad(&format!(
                    "table {:?} inflates to a different length than declared",
                    String::from_utf8_lossy(tag)
                )));
            }
        }
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    Ok(out)
}

/// The bytes of an `@font-face` source file ready for the font collection: a WOFF 1.0 file is
/// converted to its sfnt, a WOFF 2.0 file is decoded to its sfnt, an sfnt / TTC is validated as is;
/// EOT, SVG fonts and WOFF 2.0 collections are `UNSUPPORTED_MEDIA_TYPE`, other data `DECODE_FAILED`. Returns the bytes and whether they were
/// converted.
pub fn prepare_face_font(bytes: Vec<u8>) -> Result<Vec<u8>, Status> {
    if bytes.starts_with(b"wOFF") {
        let sfnt = woff1_to_sfnt(&bytes, MAX_DECODED_FONT_BYTES)?;
        validate_font(&sfnt)?;
        return Ok(sfnt);
    }
    if bytes.starts_with(b"wOF2") {
        let sfnt = super::woff2::woff2_to_sfnt(&bytes, MAX_DECODED_FONT_BYTES)?;
        validate_font(&sfnt)?;
        return Ok(sfnt);
    }
    validate_font(&bytes)?;
    Ok(bytes)
}

/// Test support: a WOFF 1.0 file made from `sfnt` (tables sorted by tag, zlib-compressed when that is
/// smaller). The encoder is not part of the product surface.
#[doc(hidden)]
pub fn make_woff1(sfnt: &[u8], compress: bool) -> Vec<u8> {
    use std::io::Write;
    let n = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
    let mut tabs: Vec<([u8; 4], u32, Vec<u8>)> = (0..n)
        .map(|i| {
            let r = 12 + 16 * i;
            let tag = [sfnt[r], sfnt[r + 1], sfnt[r + 2], sfnt[r + 3]];
            let sum = u32::from_be_bytes([sfnt[r + 4], sfnt[r + 5], sfnt[r + 6], sfnt[r + 7]]);
            let off =
                u32::from_be_bytes([sfnt[r + 8], sfnt[r + 9], sfnt[r + 10], sfnt[r + 11]]) as usize;
            let len = u32::from_be_bytes([sfnt[r + 12], sfnt[r + 13], sfnt[r + 14], sfnt[r + 15]])
                as usize;
            (tag, sum, sfnt[off..off + len].to_vec())
        })
        .collect();
    tabs.sort_by_key(|t| t.0);
    let mut body = Vec::new();
    let mut recs = Vec::new();
    let mut off = 44 + 20 * n;
    let mut total = 12 + 16 * n;
    for (tag, sum, data) in &tabs {
        let comp = if compress {
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data).unwrap();
            let c = e.finish().unwrap();
            if c.len() < data.len() {
                c
            } else {
                data.clone()
            }
        } else {
            data.clone()
        };
        recs.extend_from_slice(tag);
        recs.extend_from_slice(&(off as u32).to_be_bytes());
        recs.extend_from_slice(&(comp.len() as u32).to_be_bytes());
        recs.extend_from_slice(&(data.len() as u32).to_be_bytes());
        recs.extend_from_slice(&sum.to_be_bytes());
        body.extend_from_slice(&comp);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        off = 44 + 20 * n + body.len();
        total += (data.len() + 3) & !3;
    }
    let mut out = b"wOFF".to_vec();
    out.extend_from_slice(&sfnt[..4]);
    out.extend_from_slice(&((44 + recs.len() + body.len()) as u32).to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(&[0; 4 + 20]);
    out.extend_from_slice(&recs);
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal sfnt with `n` empty tables at the end of the file.
    pub fn sfnt(tag: &[u8; 4], tables: &[(&[u8; 4], u32, u32)], total: usize) -> Vec<u8> {
        let mut v = tag.to_vec();
        v.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        v.extend_from_slice(&[0; 6]);
        for (t, off, len) in tables {
            v.extend_from_slice(*t);
            v.extend_from_slice(&0u32.to_be_bytes());
            v.extend_from_slice(&off.to_be_bytes());
            v.extend_from_slice(&len.to_be_bytes());
        }
        v.resize(total, 0);
        v
    }

    #[test]
    fn accepts_ttf_otf_and_rejects_bad_directories() {
        let dir_end = 12 + 16;
        assert_eq!(
            validate_font(&sfnt(&[0, 1, 0, 0], &[(b"head", dir_end, 8)], 64)),
            Ok(FontContainer::TrueType)
        );
        assert_eq!(
            validate_font(&sfnt(b"OTTO", &[(b"CFF ", dir_end, 8)], 64)),
            Ok(FontContainer::OpenTypeCff)
        );
        // table past the end of the file
        assert_eq!(
            validate_font(&sfnt(&[0, 1, 0, 0], &[(b"head", dir_end, 4096)], 64))
                .unwrap_err()
                .code,
            Code::DecodeFailed
        );
        // table overlapping the directory
        assert_eq!(
            validate_font(&sfnt(&[0, 1, 0, 0], &[(b"head", 4, 8)], 64))
                .unwrap_err()
                .code,
            Code::DecodeFailed
        );
        // directory longer than the file
        assert_eq!(
            validate_font(&sfnt(&[0, 1, 0, 0], &[(b"head", dir_end, 8)], 20))
                .unwrap_err()
                .code,
            Code::DecodeFailed
        );
        assert_eq!(
            validate_font(b"hello world").unwrap_err().code,
            Code::DecodeFailed
        );
        assert_eq!(validate_font(&[]).unwrap_err().code, Code::DecodeFailed);
    }

    #[test]
    fn woff_woff2_eot_svg_are_unsupported_media_type() {
        for b in [
            &b"wOFF\0\0\0\0"[..],
            b"wOF2\0\0\0\0",
            b"<svg xmlns='http://www.w3.org/2000/svg'>",
        ] {
            assert_eq!(
                validate_font(b).unwrap_err().code,
                Code::UnsupportedMediaType
            );
        }
        let mut eot = vec![0u8; 80];
        eot[34] = b'L';
        eot[35] = b'P';
        assert_eq!(
            validate_font(&eot).unwrap_err().code,
            Code::UnsupportedMediaType
        );
        for h in ["embedded-opentype", "svg", "nonsense", ""] {
            assert!(is_unsupported_format_hint(h), "{h}");
        }
        for h in [
            "truetype",
            "opentype",
            "collection",
            "woff",
            "WOFF",
            "woff2",
            "WOFF2",
            "woff2-variations",
            "truetype-variations",
            "woff-variations",
        ] {
            assert!(!is_unsupported_format_hint(h), "{h}");
        }
    }

    #[test]
    fn collections_check_every_member() {
        let mut ttc = b"ttcf".to_vec();
        ttc.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        ttc.extend_from_slice(&2u32.to_be_bytes());
        ttc.extend_from_slice(&24u32.to_be_bytes());
        ttc.extend_from_slice(&60u32.to_be_bytes());
        // member 0 at 24: one table at offset 24+28=52 len 4; member 1 at 60 likewise
        let m0 = sfnt(&[0, 1, 0, 0], &[(b"head", 52, 4)], 36);
        let m1 = sfnt(&[0, 1, 0, 0], &[(b"head", 88, 4)], 36);
        ttc.truncate(20);
        ttc.resize(24, 0);
        ttc.extend_from_slice(&m0);
        ttc.resize(60, 0);
        ttc.extend_from_slice(&m1);
        ttc.resize(100, 0);
        assert_eq!(
            validate_font(&ttc),
            Ok(FontContainer::Collection { fonts: 2 })
        );
        ttc.truncate(70);
        assert_eq!(validate_font(&ttc).unwrap_err().code, Code::DecodeFailed);
    }

    #[test]
    fn the_pinned_noto_sans_is_valid() {
        assert!(validate_font(crate::fonts::last_resort::BYTES).is_ok());
    }

    #[test]
    fn woff1_round_trips_to_a_valid_sfnt_with_the_same_tables() {
        let ttf = crate::fonts::last_resort::BYTES;
        let woff = make_woff1(ttf, true);
        assert!(woff.len() < ttf.len(), "compressed");
        let out = prepare_face_font(woff).expect("decodes");
        assert_eq!(validate_font(&out), Ok(FontContainer::TrueType));
        // the same tables, byte for byte (the directory order and padding may differ)
        let tab = |b: &[u8], tag: &[u8; 4]| {
            let n = u16::from_be_bytes([b[4], b[5]]) as usize;
            (0..n).find_map(|i| {
                let r = 12 + 16 * i;
                (&b[r..r + 4] == tag).then(|| {
                    let off = u32::from_be_bytes(b[r + 8..r + 12].try_into().unwrap()) as usize;
                    let len = u32::from_be_bytes(b[r + 12..r + 16].try_into().unwrap()) as usize;
                    b[off..off + len].to_vec()
                })
            })
        };
        for t in [b"head", b"cmap", b"glyf", b"hmtx", b"name"] {
            assert_eq!(tab(&out, t), tab(ttf, t), "{}", String::from_utf8_lossy(t));
        }
        // uncompressed tables are accepted too
        assert!(prepare_face_font(make_woff1(ttf, false)).is_ok());
    }

    #[test]
    fn broken_woff1_is_decode_failed_and_truncated_woff2_too() {
        let ttf = crate::fonts::last_resort::BYTES;
        let woff = make_woff1(ttf, true);
        // header length lies
        let mut w = woff.clone();
        w[11] ^= 1;
        assert_eq!(prepare_face_font(w).unwrap_err().code, Code::DecodeFailed);
        // truncated
        assert_eq!(
            prepare_face_font(woff[..60].to_vec()).unwrap_err().code,
            Code::DecodeFailed
        );
        // a table that is not a zlib stream
        let mut w = woff.clone();
        let first_off = u32::from_be_bytes(w[48..52].try_into().unwrap()) as usize;
        for b in &mut w[first_off..first_off + 8] {
            *b = 0xFF;
        }
        let comp = u32::from_be_bytes(w[52..56].try_into().unwrap());
        let orig = u32::from_be_bytes(w[56..60].try_into().unwrap());
        if comp < orig {
            assert_eq!(prepare_face_font(w).unwrap_err().code, Code::DecodeFailed);
        }
        // expansion beyond the cap
        assert_eq!(
            woff1_to_sfnt(&woff, 1000).unwrap_err().code,
            Code::SizeLimit
        );
        // a declared length that the stream does not produce
        let mut w = woff.clone();
        let rec = 44 + 20 * 5;
        let (comp, orig) = (
            u32::from_be_bytes(w[rec + 8..rec + 12].try_into().unwrap()),
            u32::from_be_bytes(w[rec + 12..rec + 16].try_into().unwrap()),
        );
        if comp < orig {
            let o = (orig - 4).to_be_bytes();
            w[rec + 12..rec + 16].copy_from_slice(&o);
            assert!(prepare_face_font(w).is_err());
        }
        assert_eq!(
            prepare_face_font(b"wOF2\0\0\0\0".to_vec())
                .unwrap_err()
                .code,
            Code::DecodeFailed
        );
        assert_eq!(
            prepare_face_font(b"hello".to_vec()).unwrap_err().code,
            Code::DecodeFailed
        );
    }
}
