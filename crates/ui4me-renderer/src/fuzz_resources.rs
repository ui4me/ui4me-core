//! 1.0 F1104: image/SVG mutation fuzzing and the untrusted-resource red team. Images are decoded
//! on the resource workers, so these run the whole stack: markup -> resource policy -> decode ->
//! render. The harness is `#[ignore]`d (see `fuzz_gate` for the environment variables); the red
//! team test is a normal regression test.
//!
//!     UI4ME_FUZZ_SECS=300 nice cargo test -p ui4me-renderer --lib --release \
//!         fuzz_resources::harness -- --ignored --nocapture --test-threads=1
use super::fuzz_gate::{Rng, mutate, run_cases};
use super::*;
use std::io::Read as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend(data);
    out.extend(&body);
    out.extend(crc32(&body).to_be_bytes());
}
/// A valid RGBA PNG of one solid colour (stored deflate blocks).
pub fn solid_png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut raw = Vec::new();
    for _ in 0..h {
        raw.extend([0u8]);
        for _ in 0..w {
            raw.extend(rgba);
        }
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend(*b);
    }
    z.extend(adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}
/// A PNG whose header declares `w` x `h` but that carries almost no pixel data (a bomb).
pub fn png_header_bomb(w: u32, h: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(
        &mut out,
        b"IDAT",
        &[0x78, 0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    );
    chunk(&mut out, b"IEND", &[]);
    out
}
pub fn gif_header_bomb(w: u16, h: u16) -> Vec<u8> {
    let mut v = b"GIF89a".to_vec();
    v.extend(w.to_le_bytes());
    v.extend(h.to_le_bytes());
    v.extend([0x80, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF]);
    v.extend([0x2C, 0, 0, 0, 0]);
    v.extend(w.to_le_bytes());
    v.extend(h.to_le_bytes());
    v.extend([0, 2, 2, 0x44, 0x01, 0, 0x3B]);
    v
}
pub fn jpeg_header_bomb(w: u16, h: u16) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xC0, 0, 17, 8];
    v.extend(h.to_be_bytes());
    v.extend(w.to_be_bytes());
    v.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    v.extend([
        0xFF, 0xDA, 0, 12, 3, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0, 0xFF, 0xD9,
    ]);
    v
}
pub fn webp_header_bomb(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"RIFF".to_vec();
    v.extend(30u32.to_le_bytes());
    v.extend(b"WEBPVP8X");
    v.extend(10u32.to_le_bytes());
    v.extend([0, 0, 0, 0]);
    v.extend(&(w - 1).to_le_bytes()[..3]);
    v.extend(&(h - 1).to_le_bytes()[..3]);
    v
}

pub fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len() * 4 / 3 + 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

pub fn rss_kib(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(field))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()))
        })
        .unwrap_or(0)
}

fn scratch(label: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "ui4me-fuzz-{label}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Render until resource loading settles (bounded); returns the last framebuffer.
fn settle(d: &mut Document, w: u32, h: u32) -> Vec<u32> {
    let end = Instant::now() + Duration::from_secs(6);
    loop {
        let px = d.render(w, h, 1.0).unwrap();
        if !d.has_pending_resources() || Instant::now() > end {
            return px;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

type Seeds = Vec<(&'static str, Vec<u8>)>;

fn image_seeds() -> Seeds {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out: Seeds = Vec::new();
    let mut take = |dir: &Path| {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                let kind = match name.rsplit('.').next().unwrap_or("") {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "webp" => "image/webp",
                    "bmp" => "image/bmp",
                    "svg" => "image/svg+xml",
                    _ => continue,
                };
                if let Ok(b) = std::fs::read(e.path())
                    && b.len() < 200_000
                {
                    out.push((kind, b));
                }
            }
        }
    };
    take(&root.join("crates/ui4me-assets/tests/fixtures/resources-images/natural-sizes/codecs"));
    take(&root.join("crates/ui4me-assets/tests/fixtures/resources-svg/svg-images/svg"));
    take(&root.join("crates/ui4me-assets/tests/fixtures/resources-svg/svg-images/img"));
    take(&root.join("examples/foundation"));
    out.push(("image/png", solid_png(16, 16, [255, 0, 0, 255])));
    out.push(("image/svg+xml", b"<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink' width='40' height='40' viewBox='0 0 40 40'><defs><pattern id='p' width='4' height='4' patternUnits='userSpaceOnUse'><rect width='2' height='2'/></pattern><filter id='f'><feGaussianBlur stdDeviation='2'/></filter><mask id='m'><rect width='20' height='20' fill='white'/></mask></defs><rect width='40' height='40' fill='url(#p)' filter='url(#f)' mask='url(#m)'/><use xlink:href='#p'/><image href='http://127.0.0.1:9/x.png'/><text x='2' y='20'>t</text></svg>".to_vec()));
    out.push(("image/png", png_header_bomb(65535, 65535)));
    out.push(("image/png", png_header_bomb(0x7FFF_FFFF, 0x7FFF_FFFF)));
    out.push(("image/png", png_header_bomb(100_000, 1)));
    out.push(("image/gif", gif_header_bomb(65535, 65535)));
    out.push(("image/jpeg", jpeg_header_bomb(65535, 65535)));
    out.push(("image/webp", webp_header_bomb(16_777_216, 16_777_216)));
    out.push(("image/svg+xml", b"<svg xmlns='http://www.w3.org/2000/svg' width='1e9' height='1e9'><rect width='1e9' height='1e9'/></svg>".to_vec()));
    out.push(("image/svg+xml", b"<svg xmlns='http://www.w3.org/2000/svg' width='100' height='100'><filter id='f'><feGaussianBlur stdDeviation='1e9'/></filter><rect width='100' height='100' filter='url(#f)'/></svg>".to_vec()));
    out
}

fn mutate_bytes(rng: &mut Rng, data: &[u8], others: &Seeds) -> Vec<u8> {
    let mut v = data.to_vec();
    for _ in 0..(1 + rng.below(6)) {
        let n = v.len();
        match rng.below(8) {
            0 if n > 0 => {
                let i = rng.below(n);
                v[i] ^= 1 << rng.below(8);
            }
            1 if n > 0 => {
                let i = rng.below(n);
                v[i] = rng.next() as u8;
            }
            2 if n > 0 => v.truncate(rng.below(n)),
            3 if n > 0 => {
                let i = rng.below(n);
                let k = (1 + rng.below(64)).min(n - i);
                v.drain(i..i + k);
            }
            4 => {
                let i = rng.below(n + 1);
                let k = 1 + rng.below(64);
                let ins: Vec<u8> = (0..k).map(|_| rng.next() as u8).collect();
                v.splice(i..i, ins);
            }
            5 if n > 4 => {
                // overwrite a 4-byte field with an extreme value (sizes, counts, offsets)
                let i = rng.below(n - 3);
                let val = *rng.pick(&[
                    0u32,
                    1,
                    0x7FFF_FFFF,
                    0xFFFF_FFFF,
                    0x8000_0000,
                    65535,
                    1 << 24,
                ]);
                let b = if rng.chance(50) {
                    val.to_be_bytes()
                } else {
                    val.to_le_bytes()
                };
                v[i..i + 4].copy_from_slice(&b);
            }
            6 if !others.is_empty() => {
                let o = rng.pick(others).1.clone();
                if !o.is_empty() {
                    let a = rng.below(o.len());
                    let k = (1 + rng.below(300)).min(o.len() - a);
                    let at = rng.below(n + 1);
                    v.splice(at..at, o[a..a + k].iter().copied());
                }
            }
            _ if n > 8 => {
                let a = rng.below(n);
                let k = (1 + rng.below(200)).min(n - a);
                let part = v[a..a + k].to_vec();
                for _ in 0..rng.below(30) {
                    v.extend(&part);
                }
            }
            _ => {}
        }
        v.truncate(1_500_000);
    }
    v
}

#[test]
#[ignore = "fuzz harness; see module docs"]
fn harness_images() {
    let seeds = Arc::new(image_seeds());
    eprintln!("image corpus: {} seeds", seeds.len());
    let hwm0 = rss_kib("VmHWM:");
    let dir = scratch("img");
    let dir_make = dir.clone();
    run_cases("images", move |rng, n| {
        let (mime, base) = rng.pick(&seeds).clone();
        let bytes = if rng.chance(15) {
            base
        } else {
            mutate_bytes(rng, &base, &seeds)
        };
        let bytes = if mime == "image/svg+xml" && rng.chance(60) {
            let s = String::from_utf8_lossy(&bytes).into_owned();
            mutate(rng, &s, &[], 100_000).into_bytes()
        } else {
            bytes
        };
        let via_file = rng.chance(50) || bytes.len() > 3_000_000;
        let ext = if mime == "image/svg+xml" {
            "svg"
        } else {
            mime.rsplit('/').next().unwrap_or("bin")
        };
        let use_ = rng.below(4);
        let src = if via_file {
            std::fs::write(dir_make.join(format!("f{n}.{ext}")), &bytes).unwrap();
            format!("f{n}.{ext}")
        } else {
            format!("data:{mime};base64,{}", b64(&bytes))
        };
        let html = match use_ {
            0 => format!("<!doctype html><body style='margin:0'><img id=i src='{src}'><p>after"),
            1 => format!(
                "<!doctype html><div id=d style=\"width:100px;height:100px;background:url('{src}') center/contain no-repeat\">x</div>"
            ),
            2 => format!(
                "<!doctype html><picture><source srcset='{src} 2x'><img id=i src='{src}' width=50></picture>"
            ),
            _ => format!(
                "<!doctype html><svg width=120 height=120><image id=i href='{src}' width=100 height=100 /></svg>"
            ),
        };
        // The saved description keeps the whole input (base64 for data: URLs, so it replays).
        let desc = format!(
            "{mime} via_file={via_file} use={use_} len={}\n{html}",
            bytes.len()
        );
        if via_file {
            std::fs::write(dir_make.join(format!("last.{ext}")), &bytes).ok();
        }
        let dir2 = dir_make.clone();
        (
            desc,
            Box::new(move || {
                if let Ok(mut d) = Document::parse_with_source(&html, Some(&dir2), 320, 240) {
                    let px = settle(&mut d, 320, 240);
                    assert_eq!(px.len(), 320 * 240);
                    let _ = d.diagnostics();
                    let _ = d.accessibility_tree();
                }
            }),
        )
    });
    let hwm = rss_kib("VmHWM:");
    eprintln!("image fuzz: VmHWM {hwm0} KiB -> {hwm} KiB");
    assert!(
        hwm < hwm0 + 1_500_000,
        "peak memory grew by more than 1.5 GiB: {hwm0} -> {hwm} KiB"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------------------------
// Red team: untrusted resource access
// ---------------------------------------------------------------------------------------------

/// A loopback listener that counts connection attempts (any fetch of `http://127.0.0.1:PORT`).
struct Trap {
    port: u16,
    hits: Arc<AtomicUsize>,
}
impl Trap {
    fn new() -> Self {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                h.fetch_add(1, Ordering::SeqCst);
                let mut b = [0u8; 256];
                let _ = s.read(&mut b);
            }
        });
        Trap { port, hits }
    }
}

fn red(px: &[u32]) -> usize {
    px.iter().filter(|&&p| p == 0x00FF_0000).count()
}

/// Remote URLs, `file:` outside the root, `..` traversal and escaping symlinks are never loaded.
#[test]
fn untrusted_resources_are_not_fetched_or_escaped() {
    let trap = Trap::new();
    let base = scratch("redteam");
    let outside = base.join("outside");
    let root = base.join("root");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let red_png = solid_png(30, 30, [255, 0, 0, 255]);
    std::fs::write(outside.join("secret.png"), &red_png).unwrap();
    std::fs::write(
        outside.join("secret.css"),
        "body{background:#ff0000!important}",
    )
    .unwrap();
    std::fs::write(root.join("inside.png"), &red_png).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("secret.png"), root.join("link.png")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linkdir")).unwrap();
    }
    let remote = format!("http://127.0.0.1:{}", trap.port);
    let remote_s = format!("https://127.0.0.1:{}", trap.port);
    let abs = outside.join("secret.png");
    let abs_url = url::Url::from_file_path(&abs).unwrap().to_string();
    let css_url = url::Url::from_file_path(outside.join("secret.css"))
        .unwrap()
        .to_string();
    std::fs::write(
        root.join("remote.svg"),
        format!("<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink' width='30' height='30'><image href='{remote}/s.png' width='30' height='30'/><image xlink:href='{abs_url}' width='30' height='30'/><image href='../outside/secret.png' width='30' height='30'/></svg>"),
    )
    .unwrap();

    // Each fragment must show no pure red and cause no connection.
    let hostile: Vec<(&str, String)> = vec![
        ("img http", format!("<img src='{remote}/a.png'>")),
        ("img https", format!("<img src='{remote_s}/a.png'>")),
        (
            "img protocol-relative",
            format!("<img src='//127.0.0.1:{}/a.png'>", trap.port),
        ),
        (
            "img srcset",
            format!("<img srcset='{remote}/a.png 1x, {remote}/b.png 2x'>"),
        ),
        (
            "picture",
            format!("<picture><source srcset='{remote}/p.png'><img src='{remote}/q.png'></picture>"),
        ),
        (
            "input image",
            format!("<input type=image src='{remote}/i.png'>"),
        ),
        (
            "link stylesheet",
            format!("<link rel=stylesheet href='{remote}/a.css'>"),
        ),
        (
            "style @import",
            format!("<style>@import url({remote}/b.css);@import '{remote}/c.css';</style>"),
        ),
        (
            "css background",
            format!(
                "<div style=\"width:30px;height:30px;background:url({remote}/d.png)\">x</div>"
            ),
        ),
        (
            "css list-style",
            format!("<ul style=\"list-style-image:url({remote}/e.png)\"><li>x</ul>"),
        ),
        (
            "css border-image",
            format!(
                "<div style=\"border:10px solid;border-image:url({remote}/f.png) 10\">x</div>"
            ),
        ),
        (
            "css cursor/content",
            format!(
                "<div style=\"cursor:url({remote}/c.cur),auto;content:url({remote}/g.png)\">x</div>"
            ),
        ),
        (
            "css mask",
            format!(
                "<div style=\"width:30px;height:30px;background:red;mask-image:url({remote}/m.png)\">x</div>"
            ),
        ),
        (
            "@font-face",
            format!(
                "<style>@font-face{{font-family:f;src:url({remote}/f.woff2)}}p{{font-family:f}}</style><p>text"
            ),
        ),
        (
            "@font-face file",
            "<style>@font-face{font-family:f;src:url(file:///etc/passwd)}p{font-family:f}</style><p>text"
                .to_string(),
        ),
        (
            "svg inline image",
            format!(
                "<svg width=30 height=30><image href='{remote}/s.png' width=30 height=30 /><use href='{remote}/s.svg#a'/></svg>"
            ),
        ),
        (
            "svg filter/pattern",
            format!(
                "<svg width=30 height=30><rect width=30 height=30 fill='url({remote}/s.svg#p)' filter='url({remote}/s.svg#f)'/></svg>"
            ),
        ),
        (
            "svg file with remote refs",
            "<img src='remote.svg'>".to_string(),
        ),
        (
            "video poster",
            format!("<video poster='{remote}/v.png'></video><audio src='{remote}/a.ogg'></audio>"),
        ),
        (
            "iframe/object/embed",
            format!(
                "<iframe src='{remote}/x.html'></iframe><object data='{remote}/o.png'></object><embed src='{remote}/e.png'>"
            ),
        ),
        (
            "meta refresh/base",
            format!(
                "<meta http-equiv=refresh content='0;url={remote}/'><base href='{remote}/'><img src='x.png'>"
            ),
        ),
        ("file abs outside", format!("<img src='{abs_url}'>")),
        (
            "file abs css outside",
            format!("<link rel=stylesheet href='{css_url}'>"),
        ),
        ("traversal", "<img src='../outside/secret.png'>".to_string()),
        (
            "traversal deep",
            "<img src='sub/../../outside/secret.png'>".to_string(),
        ),
        (
            "traversal encoded",
            "<img src='%2e%2e/outside/secret.png'><img src='..%2foutside%2fsecret.png'><img src='sub/%2e%2e/%2e%2e/outside/secret.png'>".to_string(),
        ),
        (
            "traversal backslash",
            "<img src='..\\outside\\secret.png'>".to_string(),
        ),
        (
            "traversal css",
            "<div style=\"width:30px;height:30px;background:url('../outside/secret.png')\">x</div>"
                .to_string(),
        ),
        (
            "traversal css import",
            "<style>@import '../outside/secret.css';</style><p>t".to_string(),
        ),
        ("symlink file", "<img src='link.png'>".to_string()),
        ("symlink dir", "<img src='linkdir/secret.png'>".to_string()),
        (
            "host trick",
            format!("<img src='file://evil.example/{}'>", abs.display().to_string().replace('\\', "/")),
        ),
        (
            "other schemes",
            "<img src='ftp://127.0.0.1/a.png'><img src='ws://127.0.0.1/a'><img src='blob:null/x'><img src='about:blank'><img src='javascript:alert(1)'>".to_string(),
        ),
    ];
    // The document directory is the implicit file root; `..` leaves it.
    let doc_dir = root.clone();
    let mut failures = Vec::new();
    for (label, frag) in &hostile {
        let html = format!("<!doctype html><body style='margin:0;background:#fff'>{frag}");
        // Active elements (iframe, object, embed) are refused at parse time: also a pass.
        let Ok(mut d) = Document::parse_with_source(&html, Some(&doc_dir), 200, 120) else {
            continue;
        };
        let t0 = Instant::now();
        let px = settle(&mut d, 200, 120);
        let _ = t0;
        if d.has_pending_resources() {
            failures.push(format!("{label}: still pending after 6 s"));
        }
        let n = red(&px);
        if n != 0 {
            failures.push(format!(
                "{label}: {n} red pixels (outside content rendered)"
            ));
        }
    }
    // Control: the same markup with an in-root file does render red, so the check can detect it.
    let mut control = Document::parse_with_source(
        "<!doctype html><body style='margin:0'><img src='inside.png'>",
        Some(&doc_dir),
        200,
        120,
    )
    .unwrap();
    assert!(
        red(&settle(&mut control, 200, 120)) > 500,
        "control image must render"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        trap.hits.load(Ordering::SeqCst),
        0,
        "a remote URL was fetched"
    );
    assert!(failures.is_empty(), "escapes: {failures:#?}");
    std::fs::remove_dir_all(&base).ok();
}

/// Images whose headers declare enormous dimensions are rejected by the limits before any pixel
/// buffer exists: the document settles (broken image) and the process does not balloon.
///
/// The peak-RSS bound is measured in a child process that runs only this test, so other tests
/// running in parallel in the same binary cannot inflate (or hide) it. It needs Linux
/// `/proc/self/status`; elsewhere the memory bound is reported as not measured, and only the
/// rejection and settling are checked.
#[test]
fn header_bombs_are_rejected_before_allocation() {
    const CHILD: &str = "UI4ME_HEADER_BOMB_CHILD";
    if std::env::var_os(CHILD).is_none() && cfg!(target_os = "linux") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "fuzz_resources::header_bombs_are_rejected_before_allocation",
                "--exact",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "isolated header-bomb run failed:\n{text}"
        );
        assert!(
            text.contains("1 passed"),
            "isolated header-bomb run did not run the test:\n{text}"
        );
        return;
    }
    header_bombs_workload();
}

fn header_bombs_workload() {
    let bombs: Vec<(&str, Vec<u8>)> = vec![
        ("image/png", png_header_bomb(65535, 65535)),
        ("image/png", png_header_bomb(0x7FFF_FFFF, 0x7FFF_FFFF)),
        ("image/gif", gif_header_bomb(65535, 65535)),
        ("image/jpeg", jpeg_header_bomb(65535, 65535)),
        ("image/webp", webp_header_bomb(16_777_216, 16_777_216)),
    ];
    let base = scratch("bombs");
    let before = rss_kib("VmHWM:");
    for (n, (mime, bytes)) in bombs.iter().enumerate() {
        let ext = mime.rsplit('/').next().unwrap();
        std::fs::write(base.join(format!("b{n}.{ext}")), bytes).unwrap();
        for src in [
            format!("data:{mime};base64,{}", b64(bytes)),
            format!("b{n}.{ext}"),
        ] {
            let html = format!(
                "<!doctype html><img id=i src='{src}'><div style=\"width:50px;height:50px;background:url('{src}')\"></div>"
            );
            let mut d = Document::parse_with_source(&html, Some(&base), 200, 120).unwrap();
            let end = Instant::now() + Duration::from_secs(5);
            while Instant::now() < end {
                d.render(200, 120, 1.0).unwrap();
                if !d.has_pending_resources() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                !d.has_pending_resources(),
                "{mime} bomb ({src:.40}) did not settle"
            );
        }
    }
    if cfg!(target_os = "linux") {
        assert!(
            before > 0,
            "VmHWM is not readable: the memory bound was not measured"
        );
        let grown = rss_kib("VmHWM:").saturating_sub(before);
        assert!(
            grown < 400_000,
            "header bombs grew the peak RSS by {grown} KiB"
        );
    } else {
        eprintln!("header bombs: peak-RSS bound not measured on this platform (Linux only)");
    }
    std::fs::remove_dir_all(&base).ok();
}

/// F1104 finding: a file image that failed (missing, outside the root, undecodable) was
/// restarted on every frame, so it stayed "pending" forever and re-ran its load each render.
/// Failed requests are terminal; the document settles and stops waking the host.
#[test]
fn failed_file_images_settle_instead_of_retrying_every_frame() {
    let base = scratch("settle");
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(base.join("secret.png"), solid_png(30, 30, [255, 0, 0, 255])).unwrap();
    std::fs::write(root.join("garbage.png"), b"\x89PNG\r\n\x1a\nnot really").unwrap();
    for src in ["missing.png", "../secret.png", "garbage.png"] {
        let html = format!(
            "<!doctype html><img src='{src}'><div style=\"width:20px;height:20px;background:url('{src}')\"></div>"
        );
        let mut d = Document::parse_with_source(&html, Some(&root), 200, 120).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            d.render(200, 120, 1.0).unwrap();
            if !d.has_pending_resources() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!d.has_pending_resources(), "{src} stayed pending");
        for _ in 0..5 {
            d.render(200, 120, 1.0).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            assert!(
                !d.has_pending_resources(),
                "{src} was restarted after failing"
            );
        }
        assert!(
            d.next_wake_in()
                .is_none_or(|w| w > Duration::from_millis(50)),
            "{src}: the host would keep waking"
        );
    }
    std::fs::remove_dir_all(&base).ok();
}
