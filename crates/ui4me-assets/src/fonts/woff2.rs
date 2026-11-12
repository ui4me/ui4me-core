//! owner: 007-resources (TXT-1, 015) — WOFF 2.0 to sfnt (W3C WOFF File Format 2.0, sections 4 to 5).
//!
//! The file is a 48-byte header, a table directory (flags byte with a known-tag index and a
//! transform version, `UIntBase128` lengths), one Brotli stream holding every table's data in
//! directory order and optional metadata / private blocks (never read). The decoder rebuilds the
//! equivalent TrueType / OpenType font: the `glyf` / `loca` pair from the transformed streams
//! (section 5.1 to 5.3: contour counts, point counts, flags, the triplet coordinate encoding,
//! composite glyphs, explicit bounding boxes, instructions and the overlap bitmap) and `hmtx` with
//! left side bearings taken from the glyph bounding boxes (section 5.4). Every other table is
//! copied. Font collections (`ttcf` flavor) are not supported (`UNSUPPORTED_MEDIA_TYPE`).
//!
//! Limits (they extend the WOFF 1.0 ones): the table count is at most 4096; the sum of the stored
//! table lengths and of the original lengths, and the rebuilt sfnt, stay within `max_decoded`
//! (`SIZE_LIMIT`, checked before anything is allocated and while the glyph data grows); the Brotli
//! stream must decompress to exactly the size the directory declares and is never read past it (a
//! decompression bomb ends one byte over). Every read of a stream is bounds checked; malformed input
//! is `DECODE_FAILED`. Table checksums of the source are not verified (the shaper does not use them);
//! the rebuilt directory carries fresh checksums, and `head.checkSumAdjustment` is left as stored.

use std::io::Read;

use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// Largest table count accepted (the WOFF 1.0 limit).
const MAX_TABLES: usize = 4096;

/// The 63 known table tags of the directory flags (WOFF2 section 5.3 table).
const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
    b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
    b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
    b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
    b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

fn bad(m: impl std::fmt::Display) -> Status {
    Status::new(Code::DecodeFailed, Stage::Resource).with_message(format!("WOFF2: {m}"))
}

fn too_big(max: u64) -> Status {
    Status::new(Code::SizeLimit, Stage::Resource)
        .with_message(format!("WOFF2 expands to more than {max} bytes"))
}

/// A bounds-checked big-endian reader over a byte slice.
struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Self {
        Rd { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], Status> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.b.len())
            .ok_or_else(|| bad("a stream ends early"))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, Status> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Status> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Result<i16, Status> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, Status> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    /// `UIntBase128` (section 6.1.1): at most five bytes, no leading zero, no 32-bit overflow.
    fn base128(&mut self) -> Result<u32, Status> {
        let mut acc: u32 = 0;
        for i in 0..5 {
            let byte = self.u8()?;
            if i == 0 && byte == 0x80 {
                return Err(bad("UIntBase128 with a leading zero"));
            }
            if acc & 0xFE00_0000 != 0 {
                return Err(bad("UIntBase128 overflows 32 bits"));
            }
            acc = (acc << 7) | u32::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return Ok(acc);
            }
        }
        Err(bad("UIntBase128 longer than five bytes"))
    }
    /// `255UInt16` (section 6.1.2).
    fn u255(&mut self) -> Result<u16, Status> {
        match self.u8()? {
            253 => self.u16(),
            255 => Ok(253 + u16::from(self.u8()?)),
            254 => Ok(506 + u16::from(self.u8()?)),
            c => Ok(u16::from(c)),
        }
    }
}

struct Entry {
    tag: [u8; 4],
    /// Length of the table in the sfnt (the directory's `origLength`).
    orig: usize,
    /// Length of the data in the decompressed stream (`transformLength` when transformed).
    stored: usize,
    transformed: bool,
}

fn rd16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

/// WOFF 2.0 to sfnt. `max_decoded` bounds the rebuilt font.
pub fn woff2_to_sfnt(b: &[u8], max_decoded: u64) -> Result<Vec<u8>, Status> {
    let mut r = Rd::new(b);
    if r.take(4).ok() != Some(b"wOF2") {
        return Err(bad("missing signature"));
    }
    let flavor = r.u32().map_err(|_| bad("truncated header"))?;
    let length = r.u32().map_err(|_| bad("truncated header"))? as u64;
    let num = r.u16().map_err(|_| bad("truncated header"))? as usize;
    let _reserved = r.u16().map_err(|_| bad("truncated header"))?;
    let total_sfnt = r.u32().map_err(|_| bad("truncated header"))? as u64;
    let total_compressed = r.u32().map_err(|_| bad("truncated header"))? as usize;
    r.take(4).map_err(|_| bad("truncated header"))?; // major, minor version
    let (meta_off, meta_len, _meta_orig) = (
        r.u32().map_err(|_| bad("truncated header"))? as u64,
        r.u32().map_err(|_| bad("truncated header"))? as u64,
        r.u32().map_err(|_| bad("truncated header"))?,
    );
    let (priv_off, priv_len) = (
        r.u32().map_err(|_| bad("truncated header"))? as u64,
        r.u32().map_err(|_| bad("truncated header"))? as u64,
    );
    if length != b.len() as u64 {
        return Err(bad("header length differs from the file length"));
    }
    if &flavor.to_be_bytes() == b"ttcf" {
        return Err(Status::new(Code::UnsupportedMediaType, Stage::Resource)
            .with_message("WOFF2 font collections (ttcf flavor) are not supported"));
    }
    if num == 0 || num > MAX_TABLES {
        return Err(bad(format!("implausible table count {num}")));
    }
    if total_sfnt > max_decoded {
        return Err(too_big(max_decoded));
    }

    // --- table directory ---
    let mut entries: Vec<Entry> = Vec::with_capacity(num);
    let (mut sum_stored, mut sum_orig) = (0u64, 0u64);
    for _ in 0..num {
        let flags = r.u8()?;
        let idx = (flags & 0x3F) as usize;
        let version = flags >> 6;
        let tag: [u8; 4] = if idx == 63 {
            let t = r.take(4)?;
            [t[0], t[1], t[2], t[3]]
        } else {
            *KNOWN_TAGS[idx]
        };
        let orig = r.base128()? as usize;
        let is_glyf_loca = &tag == b"glyf" || &tag == b"loca";
        let transformed = match (&tag, version) {
            (_, _) if is_glyf_loca => match version {
                0 => true,
                3 => false,
                _ => {
                    return Err(bad(format!(
                        "invalid transform version {version} for {}",
                        String::from_utf8_lossy(&tag)
                    )));
                }
            },
            (b"hmtx", v) => match v {
                1 => true,
                0 => false,
                _ => return Err(bad(format!("invalid transform version {v} for hmtx"))),
            },
            (t, 0) => {
                let _ = t;
                false
            }
            (t, v) => {
                return Err(bad(format!(
                    "invalid transform version {v} for {}",
                    String::from_utf8_lossy(t)
                )));
            }
        };
        let stored = if transformed {
            let l = r.base128()? as usize;
            if &tag == b"loca" && l != 0 {
                return Err(bad("a transformed loca table must have transformLength 0"));
            }
            l
        } else {
            orig
        };
        if entries.iter().any(|e| e.tag == tag) {
            return Err(bad(format!(
                "duplicate table {}",
                String::from_utf8_lossy(&tag)
            )));
        }
        sum_stored += stored as u64;
        sum_orig += (orig as u64 + 3) & !3;
        if sum_stored > max_decoded || sum_orig > max_decoded {
            return Err(too_big(max_decoded));
        }
        entries.push(Entry {
            tag,
            orig,
            stored,
            transformed,
        });
    }
    let find = |tag: &[u8; 4]| entries.iter().position(|e| &e.tag == tag);
    if let (Some(g), Some(l)) = (find(b"glyf"), find(b"loca")) {
        if entries[g].transformed != entries[l].transformed {
            return Err(bad("glyf and loca must both be transformed or both not"));
        }
    } else if entries
        .iter()
        .any(|e| e.transformed && (&e.tag == b"glyf" || &e.tag == b"loca"))
    {
        return Err(bad("a transformed glyf or loca table without its pair"));
    }

    // --- the compressed stream ---
    let dir_end = r.pos;
    let comp_end = dir_end
        .checked_add(total_compressed)
        .filter(|e| *e <= b.len())
        .ok_or_else(|| bad("the compressed data lies outside the file"))?;
    for (off, len, what) in [
        (meta_off, meta_len, "metadata"),
        (priv_off, priv_len, "private data"),
    ] {
        if len != 0 && (off < comp_end as u64 || off + len > b.len() as u64) {
            return Err(bad(format!("{what} lies outside the file")));
        }
    }
    let mut data: Vec<u8> =
        Vec::with_capacity((sum_stored as usize).min(b.len().saturating_mul(8)));
    {
        let mut dec = brotli_decompressor::Decompressor::new(&b[dir_end..comp_end], 4096)
            .take(sum_stored + 1);
        dec.read_to_end(&mut data)
            .map_err(|e| bad(format!("Brotli stream: {e}")))?;
    }
    if data.len() as u64 != sum_stored {
        return Err(bad(if data.len() as u64 > sum_stored {
            "the Brotli stream decompresses to more than the directory declares"
        } else {
            "the Brotli stream decompresses to less than the directory declares"
        }));
    }

    // --- tables ---
    let mut raw: Vec<(&Entry, &[u8])> = Vec::with_capacity(num);
    let mut at = 0usize;
    for e in &entries {
        raw.push((e, &data[at..at + e.stored]));
        at += e.stored;
    }
    let mut out_tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(num);
    let mut budget = max_decoded;
    let mut rebuilt: Option<Glyf> = None;
    if let Some(g) = find(b"glyf").filter(|g| entries[*g].transformed) {
        rebuilt = Some(reconstruct_glyf(raw[g].1, &mut budget, max_decoded)?);
        let loca = find(b"loca").expect("checked above");
        let rb = rebuilt.as_ref().expect("just set");
        if entries[loca].orig != rb.loca.len() {
            return Err(bad(
                "loca length does not match the glyph count and index format",
            ));
        }
    }
    let mut loca_for_hmtx: Option<(Vec<u8>, bool, Vec<u8>)> = None; // (loca, long, glyf)
    for (e, d) in &raw {
        let table: Vec<u8> = match &e.tag {
            b"glyf" if e.transformed => rebuilt.as_ref().expect("rebuilt").glyf.clone(),
            b"loca" if e.transformed => rebuilt.as_ref().expect("rebuilt").loca.clone(),
            b"head" => {
                let mut h = d.to_vec();
                if let Some(rb) = &rebuilt {
                    if h.len() < 54 {
                        return Err(bad("head table is too short"));
                    }
                    h[50..52].copy_from_slice(&u16::from(rb.long_loca).to_be_bytes());
                }
                h
            }
            b"hmtx" if e.transformed => Vec::new(), // after glyf, below
            _ => d.to_vec(),
        };
        out_tables.push((e.tag, table));
    }
    if let Some(i) = find(b"hmtx").filter(|i| entries[*i].transformed) {
        let get = |tag: &[u8; 4]| {
            out_tables
                .iter()
                .find(|(t, _)| t == tag)
                .map(|(_, d)| d.as_slice())
        };
        let numg = get(b"maxp")
            .and_then(|m| rd16(m, 4))
            .ok_or_else(|| bad("hmtx transform needs maxp"))? as usize;
        let numh = get(b"hhea")
            .and_then(|h| rd16(h, 34))
            .ok_or_else(|| bad("hmtx transform needs hhea"))? as usize;
        let (glyf, loca, long) = match (get(b"glyf"), get(b"loca")) {
            (Some(g), Some(l)) => {
                let long = rebuilt
                    .as_ref()
                    .map(|r| r.long_loca)
                    .or_else(|| get(b"head").and_then(|h| rd16(h, 50)).map(|f| f != 0))
                    .ok_or_else(|| bad("hmtx transform needs head"))?;
                (g.to_vec(), l.to_vec(), long)
            }
            _ => return Err(bad("hmtx transform needs glyf and loca")),
        };
        loca_for_hmtx = Some((loca, long, glyf));
        let (loca, long, glyf) = loca_for_hmtx.as_ref().expect("set");
        let xmins = x_mins(glyf, loca, *long, numg)?;
        let table = rebuild_hmtx(raw[i].1, numg, numh, &xmins)?;
        if table.len() != entries[i].orig {
            return Err(bad("rebuilt hmtx length differs from the directory"));
        }
        out_tables
            .iter_mut()
            .find(|(t, _)| t == b"hmtx")
            .expect("present")
            .1 = table;
    }
    drop(loca_for_hmtx);

    // --- assemble the sfnt ---
    out_tables.sort_by_key(|a| a.0);
    let n = out_tables.len();
    let mut sfnt_len: u64 = 12 + 16 * n as u64;
    for (_, t) in &out_tables {
        sfnt_len += (t.len() as u64 + 3) & !3;
    }
    if sfnt_len > max_decoded {
        return Err(too_big(max_decoded));
    }
    let mut out = Vec::with_capacity(sfnt_len as usize);
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    let (mut pow, mut sel) = (1u32, 0u16);
    while pow * 2 <= n as u32 {
        pow *= 2;
        sel += 1;
    }
    let search_range = (pow * 16) as u16;
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&sel.to_be_bytes());
    out.extend_from_slice(&((n as u16 * 16).saturating_sub(search_range)).to_be_bytes());
    let mut off = 12 + 16 * n;
    for (tag, t) in &out_tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(t).to_be_bytes());
        out.extend_from_slice(&(off as u32).to_be_bytes());
        out.extend_from_slice(&(t.len() as u32).to_be_bytes());
        off += (t.len() + 3) & !3;
    }
    for (_, t) in &out_tables {
        out.extend_from_slice(t);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    Ok(out)
}

fn checksum(t: &[u8]) -> u32 {
    let mut sum = 0u32;
    for c in t.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum = sum.wrapping_add(u32::from_be_bytes(w));
    }
    sum
}

/// `xMin` of every glyph (0 for an empty glyph) from a `glyf` / `loca` pair.
fn x_mins(glyf: &[u8], loca: &[u8], long: bool, num_glyphs: usize) -> Result<Vec<i16>, Status> {
    let entry = |i: usize| -> Option<usize> {
        if long {
            loca.get(4 * i..4 * i + 4)
                .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize)
        } else {
            rd16(loca, 2 * i).map(|v| v as usize * 2)
        }
    };
    let mut v = Vec::with_capacity(num_glyphs);
    for g in 0..num_glyphs {
        let (a, z) = (
            entry(g).ok_or_else(|| bad("loca is shorter than the glyph count"))?,
            entry(g + 1).ok_or_else(|| bad("loca is shorter than the glyph count"))?,
        );
        if z < a || z > glyf.len() {
            return Err(bad("loca offsets are not ascending or exceed glyf"));
        }
        v.push(if z - a >= 10 {
            rd16(glyf, a + 2).unwrap_or(0) as i16
        } else {
            0
        });
    }
    Ok(v)
}

/// Section 5.4: `hmtx` from its transformed form.
fn rebuild_hmtx(
    t: &[u8],
    num_glyphs: usize,
    num_h: usize,
    xmins: &[i16],
) -> Result<Vec<u8>, Status> {
    if num_h == 0 || num_h > num_glyphs {
        return Err(bad(
            "hhea.numberOfHMetrics is zero or larger than the glyph count",
        ));
    }
    let mut r = Rd::new(t);
    let flags = r.u8()?;
    if flags & 0xFC != 0 {
        return Err(bad("reserved hmtx transform flags are set"));
    }
    let (prop_lsb_omitted, mono_lsb_omitted) = (flags & 1 != 0, flags & 2 != 0);
    let mut advances = Vec::with_capacity(num_h);
    for _ in 0..num_h {
        advances.push(r.u16()?);
    }
    let mut lsbs = Vec::with_capacity(num_glyphs);
    for i in 0..num_h {
        lsbs.push(if prop_lsb_omitted { xmins[i] } else { r.i16()? });
    }
    for i in num_h..num_glyphs {
        lsbs.push(if mono_lsb_omitted { xmins[i] } else { r.i16()? });
    }
    let mut out = Vec::with_capacity(4 * num_h + 2 * (num_glyphs - num_h));
    for (i, l) in lsbs.iter().enumerate() {
        if i < num_h {
            out.extend_from_slice(&advances[i].to_be_bytes());
        }
        out.extend_from_slice(&l.to_be_bytes());
    }
    Ok(out)
}

struct Glyf {
    glyf: Vec<u8>,
    loca: Vec<u8>,
    long_loca: bool,
}

/// `with_sign` of the triplet table: the low bit of `flag` selects the positive direction.
fn with_sign(flag: u8, v: i32) -> i32 {
    if flag & 1 != 0 { v } else { -v }
}

/// One coordinate triplet (section 5.2): `(dx, dy)`, reading its bytes from `g`.
fn triplet(flag: u8, g: &mut Rd<'_>) -> Result<(i32, i32), Status> {
    let f = i32::from(flag);
    Ok(if flag < 10 {
        let b0 = i32::from(g.u8()?);
        (0, with_sign(flag, ((f & 14) << 7) + b0))
    } else if flag < 20 {
        let b0 = i32::from(g.u8()?);
        (with_sign(flag, (((f - 10) & 14) << 7) + b0), 0)
    } else if flag < 84 {
        let b0 = f - 20;
        let b1 = i32::from(g.u8()?);
        (
            with_sign(flag, 1 + (b0 & 0x30) + (b1 >> 4)),
            with_sign(flag >> 1, 1 + ((b0 & 0x0C) << 2) + (b1 & 0x0F)),
        )
    } else if flag < 120 {
        let b0 = f - 84;
        let (b1, b2) = (i32::from(g.u8()?), i32::from(g.u8()?));
        (
            with_sign(flag, 1 + ((b0 / 12) << 8) + b1),
            with_sign(flag >> 1, 1 + (((b0 % 12) >> 2) << 8) + b2),
        )
    } else if flag < 124 {
        let s = g.take(3)?;
        let (b1, b2, b3) = (i32::from(s[0]), i32::from(s[1]), i32::from(s[2]));
        (
            with_sign(flag, (b1 << 4) + (b2 >> 4)),
            with_sign(flag >> 1, ((b2 & 0x0F) << 8) + b3),
        )
    } else {
        let s = g.take(4)?;
        (
            with_sign(flag, (i32::from(s[0]) << 8) + i32::from(s[1])),
            with_sign(flag >> 1, (i32::from(s[2]) << 8) + i32::from(s[3])),
        )
    })
}

/// Sections 5.1 to 5.3: the transformed `glyf` table to `glyf` and `loca`.
fn reconstruct_glyf(t: &[u8], budget: &mut u64, max_decoded: u64) -> Result<Glyf, Status> {
    let mut h = Rd::new(t);
    let _reserved = h.u16()?;
    let option_flags = h.u16()?;
    let num_glyphs = h.u16()? as usize;
    let index_format = h.u16()?;
    if index_format > 1 {
        return Err(bad("invalid glyf index format"));
    }
    let mut sizes = [0usize; 7];
    for s in &mut sizes {
        *s = h.u32()? as usize;
    }
    let [
        n_contour_sz,
        n_points_sz,
        flag_sz,
        glyph_sz,
        composite_sz,
        bbox_sz,
        instr_sz,
    ] = sizes;
    let mut n_contour = Rd::new(h.take(n_contour_sz)?);
    let mut n_points = Rd::new(h.take(n_points_sz)?);
    let mut flags = Rd::new(h.take(flag_sz)?);
    let mut glyphs = Rd::new(h.take(glyph_sz)?);
    let mut composite = Rd::new(h.take(composite_sz)?);
    let mut bbox_all = Rd::new(h.take(bbox_sz)?);
    let mut instr = Rd::new(h.take(instr_sz)?);
    let overlap: Option<&[u8]> = if option_flags & 1 != 0 {
        Some(h.take(num_glyphs.div_ceil(8))?)
    } else {
        None
    };
    let bitmap_len = 4 * num_glyphs.div_ceil(32);
    let bbox_bitmap = bbox_all.take(bitmap_len)?;
    let mut bbox = bbox_all;
    if n_contour_sz < 2 * num_glyphs {
        return Err(bad(
            "the contour count stream is shorter than the glyph count",
        ));
    }

    let mut glyf: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::with_capacity(num_glyphs + 1);
    for gi in 0..num_glyphs {
        offsets.push(glyf.len() as u32);
        let nc = n_contour.i16()?;
        let has_bbox = bbox_bitmap[gi >> 3] & (0x80 >> (gi & 7)) != 0;
        if nc == 0 {
            if has_bbox {
                return Err(bad("an empty glyph has an explicit bounding box"));
            }
            continue;
        }
        let start = glyf.len();
        if nc > 0 {
            let nc = nc as usize;
            let mut end_pts: Vec<u16> = Vec::with_capacity(nc);
            let mut total: usize = 0;
            for _ in 0..nc {
                total += n_points.u255()? as usize;
                if total == 0 || total > 0xFFFF {
                    return Err(bad("a glyph has no points or more than 65535"));
                }
                end_pts.push((total - 1) as u16);
            }
            let mut pts: Vec<(i32, i32, bool)> = Vec::with_capacity(total);
            let flag_bytes = flags.take(total)?;
            let (mut x, mut y) = (0i32, 0i32);
            for &f in flag_bytes {
                let (dx, dy) = triplet(f & 0x7F, &mut glyphs)?;
                x += dx;
                y += dy;
                pts.push((x, y, f & 0x80 == 0));
            }
            let ilen = glyphs.u255()? as usize;
            let ins = instr.take(ilen)?;
            let (xmin, ymin, xmax, ymax) = if has_bbox {
                (bbox.i16()?, bbox.i16()?, bbox.i16()?, bbox.i16()?)
            } else {
                let c = |v: i32| {
                    i16::try_from(v).map_err(|_| bad("a coordinate leaves the 16-bit range"))
                };
                let (mut a, mut b, mut cc, mut d) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
                for &(px, py, _) in &pts {
                    a = a.min(px);
                    cc = cc.max(px);
                    b = b.min(py);
                    d = d.max(py);
                }
                (c(a)?, c(b)?, c(cc)?, c(d)?)
            };
            for v in [
                pts.iter().map(|p| p.0).max(),
                pts.iter().map(|p| p.0).min(),
                pts.iter().map(|p| p.1).max(),
                pts.iter().map(|p| p.1).min(),
            ]
            .into_iter()
            .flatten()
            {
                if i16::try_from(v).is_err() {
                    return Err(bad("a coordinate leaves the 16-bit range"));
                }
            }
            glyf.extend_from_slice(&(nc as i16).to_be_bytes());
            for v in [xmin, ymin, xmax, ymax] {
                glyf.extend_from_slice(&v.to_be_bytes());
            }
            for e in &end_pts {
                glyf.extend_from_slice(&e.to_be_bytes());
            }
            glyf.extend_from_slice(&(ilen as u16).to_be_bytes());
            glyf.extend_from_slice(ins);
            let overlap_first = overlap.is_some_and(|o| o[gi >> 3] & (0x80 >> (gi & 7)) != 0);
            encode_points(&pts, overlap_first, &mut glyf);
        } else if nc == -1 {
            if !has_bbox {
                return Err(bad("a composite glyph without an explicit bounding box"));
            }
            glyf.extend_from_slice(&(-1i16).to_be_bytes());
            for _ in 0..4 {
                glyf.extend_from_slice(&bbox.i16()?.to_be_bytes());
            }
            let mut have_instr = false;
            loop {
                let flag = composite.u16()?;
                glyf.extend_from_slice(&flag.to_be_bytes());
                have_instr |= flag & 0x0100 != 0;
                let mut n = 2 + if flag & 0x0001 != 0 { 4 } else { 2 };
                if flag & 0x0008 != 0 {
                    n += 2;
                } else if flag & 0x0040 != 0 {
                    n += 4;
                } else if flag & 0x0080 != 0 {
                    n += 8;
                }
                glyf.extend_from_slice(composite.take(n)?);
                if flag & 0x0020 == 0 {
                    break;
                }
                if glyf.len() as u64 > max_decoded {
                    return Err(too_big(max_decoded));
                }
            }
            if have_instr {
                let ilen = glyphs.u255()? as usize;
                glyf.extend_from_slice(&(ilen as u16).to_be_bytes());
                glyf.extend_from_slice(instr.take(ilen)?);
            }
        } else {
            return Err(bad("invalid contour count"));
        }
        while !(glyf.len() - start).is_multiple_of(4) {
            glyf.push(0);
        }
        if glyf.len() as u64 > *budget {
            return Err(too_big(max_decoded));
        }
    }
    offsets.push(glyf.len() as u32);
    *budget = budget.saturating_sub(glyf.len() as u64);
    let long = index_format == 1;
    let mut loca = Vec::with_capacity(offsets.len() * if long { 4 } else { 2 });
    for o in &offsets {
        if long {
            loca.extend_from_slice(&o.to_be_bytes());
        } else {
            let half = o / 2;
            if half > 0xFFFF {
                return Err(bad("the glyph data is too large for the short loca format"));
            }
            loca.extend_from_slice(&(half as u16).to_be_bytes());
        }
    }
    Ok(Glyf {
        glyf,
        loca,
        long_loca: long,
    })
}

/// The TrueType simple-glyph flag, x and y arrays of `pts` (deltas, repeat-compressed flags).
fn encode_points(pts: &[(i32, i32, bool)], overlap_first: bool, out: &mut Vec<u8>) {
    let mut flag_bytes: Vec<u8> = Vec::with_capacity(pts.len());
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    let (mut px, mut py) = (0i32, 0i32);
    for (i, &(x, y, on)) in pts.iter().enumerate() {
        let (dx, dy) = (x - px, y - py);
        px = x;
        py = y;
        let mut f = u8::from(on);
        if i == 0 && overlap_first {
            f |= 0x40;
        }
        if dx == 0 {
            f |= 0x10;
        } else if dx.abs() < 256 {
            f |= 0x02 | if dx > 0 { 0x10 } else { 0 };
            xs.push(dx.unsigned_abs() as u8);
        } else {
            xs.extend_from_slice(&(dx as i16).to_be_bytes());
        }
        if dy == 0 {
            f |= 0x20;
        } else if dy.abs() < 256 {
            f |= 0x04 | if dy > 0 { 0x20 } else { 0 };
            ys.push(dy.unsigned_abs() as u8);
        } else {
            ys.extend_from_slice(&(dy as i16).to_be_bytes());
        }
        flag_bytes.push(f);
    }
    let mut i = 0;
    while i < flag_bytes.len() {
        let f = flag_bytes[i];
        let mut run = 1;
        while i + run < flag_bytes.len() && flag_bytes[i + run] == f && run < 256 {
            run += 1;
        }
        if run > 1 {
            out.push(f | 0x08);
            out.push((run - 1) as u8);
        } else {
            out.push(f);
        }
        i += run;
    }
    out.extend_from_slice(&xs);
    out.extend_from_slice(&ys);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base128_and_255_uint16() {
        assert_eq!(Rd::new(&[0x3F]).base128().unwrap(), 63);
        assert_eq!(Rd::new(&[0x85, 0x07]).base128().unwrap(), 647);
        assert!(Rd::new(&[0x80, 0x01]).base128().is_err(), "leading zero");
        assert!(
            Rd::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x7F]).base128().is_err(),
            "overflow"
        );
        assert!(
            Rd::new(&[0x81, 0x81, 0x81, 0x81, 0x81]).base128().is_err(),
            "six bytes"
        );
        assert_eq!(Rd::new(&[100]).u255().unwrap(), 100);
        assert_eq!(Rd::new(&[255, 3]).u255().unwrap(), 256);
        assert_eq!(Rd::new(&[254, 3]).u255().unwrap(), 509);
        assert_eq!(Rd::new(&[253, 1, 2]).u255().unwrap(), 258);
    }

    #[test]
    fn triplet_forms() {
        // flag 0: dx 0, dy = -(0*256 + b0); flag 1: positive.
        assert_eq!(triplet(0, &mut Rd::new(&[7])).unwrap(), (0, -7));
        assert_eq!(triplet(1, &mut Rd::new(&[7])).unwrap(), (0, 7));
        // flag 11: dx positive (flag - 10 = 1), (1 & 14) = 0.
        assert_eq!(triplet(11, &mut Rd::new(&[9])).unwrap(), (9, 0));
        // flag 21 (b0 = 1): dx +, dy from the low bit of flag >> 1 = 10 (even): negative.
        assert_eq!(triplet(21, &mut Rd::new(&[0x21])).unwrap(), (3, -2));
        // 4-byte form, flag 127: both positive.
        assert_eq!(
            triplet(127, &mut Rd::new(&[1, 2, 3, 4])).unwrap(),
            (258, 772)
        );
    }

    #[test]
    fn short_or_wrong_input_is_decode_failed() {
        for b in [&b""[..], b"wOF2", b"wOF2\0\0\0\0", b"wOFF\0\0\0\0"] {
            assert_eq!(
                woff2_to_sfnt(b, 1 << 20).unwrap_err().code,
                Code::DecodeFailed
            );
        }
    }
}
