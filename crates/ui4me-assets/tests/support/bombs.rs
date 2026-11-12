// owner: 007-resources (T065) — builds the bomb corpus from verification/fixtures/resource-bombs/bombs.json.
#![allow(dead_code, clippy::collapsible_if, clippy::unnecessary_min_or_max)]

use serde_json::Value;
use ui4me_assets::decode::fake::FakeImage;

pub const BOMBS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/resource-bombs/bombs.json"
));

pub fn png_declared(w: u32, h: u32, file_bytes: usize) -> Vec<u8> {
    let mut v = vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
    ];
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    if v.len() < file_bytes {
        // an IDAT chunk of filler up to the requested file size
        let filler = file_bytes - v.len();
        v.extend_from_slice(b"\0\0\0\0IDAT");
        v.resize(v.len() + filler.saturating_sub(8), 0xAB);
    }
    v.truncate(file_bytes.max(0));
    v
}

pub fn jpeg_declared(w: u16, h: u16) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xC0, 0, 17, 8];
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

pub fn gif_declared(w: u16, h: u16) -> Vec<u8> {
    let mut v = b"GIF89a".to_vec();
    v.extend_from_slice(&w.to_le_bytes());
    v.extend_from_slice(&h.to_le_bytes());
    v.extend_from_slice(&[0, 0, 0, 0x3B]);
    v
}

pub fn bmp_declared(w: i32, h: i32) -> Vec<u8> {
    let mut v = vec![0u8; 54];
    v[0] = b'B';
    v[1] = b'M';
    v[14] = 40;
    v[18..22].copy_from_slice(&w.to_le_bytes());
    v[22..26].copy_from_slice(&h.to_le_bytes());
    v
}

/// Write the bomb described by `case["recipe"]` to `path` (sparse files are never materialised).
pub fn write_recipe(case: &Value, path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let r = &case["recipe"];
    let n = |o: &Value, k: &str| o[k].as_u64().unwrap();
    if let Some(p) = r.get("png_declared") {
        std::fs::write(
            path,
            png_declared(
                n(p, "w") as u32,
                n(p, "h") as u32,
                n(p, "file_bytes") as usize,
            ),
        )
        .unwrap();
    } else if let Some(p) = r.get("jpeg_declared") {
        std::fs::write(path, jpeg_declared(n(p, "w") as u16, n(p, "h") as u16)).unwrap();
    } else if let Some(p) = r.get("gif_declared") {
        std::fs::write(path, gif_declared(n(p, "w") as u16, n(p, "h") as u16)).unwrap();
    } else if let Some(p) = r.get("bmp_declared") {
        std::fs::write(path, bmp_declared(n(p, "w") as i32, n(p, "h") as i32)).unwrap();
    } else if let Some(p) = r.get("sparse") {
        let f = std::fs::File::create(path).unwrap();
        f.set_len(n(p, "bytes")).unwrap();
    } else if let Some(p) = r.get("fake") {
        let mut f = FakeImage::new(n(p, "w") as u32, n(p, "h") as u32);
        if let Some(d) = p.get("delay_ms") {
            f = f.delay(d.as_u64().unwrap() as u32);
        }
        std::fs::write(path, f.bytes()).unwrap();
    } else if let Some(p) = r.get("nested_svg") {
        std::fs::write(path, nested_svg(n(p, "depth") as u32)).unwrap();
    } else if let Some(p) = r.get("bytes") {
        let h = p["hex"].as_str().unwrap();
        let b: Vec<u8> = (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect();
        std::fs::write(path, b).unwrap();
    } else {
        panic!("recipe not buildable here: {r}");
    }
}

/// Peak resident set size of this process in KiB (VmHWM).
pub fn peak_rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

/// `depth` levels of `<image href="data:image/svg+xml,...">` around an empty SVG. The inner
/// document is percent-encoded at every level, so the size grows quadratically, not geometrically.
pub fn nested_svg(depth: u32) -> String {
    let mut doc = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#.to_string();
    for _ in 0..depth {
        let enc: String = doc
            .chars()
            .map(|c| match c {
                '%' => "%25".to_string(),
                '"' => "%22".to_string(),
                '<' => "%3C".to_string(),
                '>' => "%3E".to_string(),
                '#' => "%23".to_string(),
                '&' => "%26".to_string(),
                c => c.to_string(),
            })
            .collect();
        doc = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="data:image/svg+xml,{enc}" width="10" height="10"/></svg>"#
        );
    }
    doc
}
