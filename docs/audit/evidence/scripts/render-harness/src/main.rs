//! RENDER audit harness: drives the public ui4me_renderer::Document API in-process.
//! Modes (argv[1]):
//!   leak <iters> <variant>   variant: bold | plain | mono | none ; env NORENDER=1 uses element_rect instead of render
//!   settext <rows> <iters>   time set_text + render on a large document
//!   deep <depth> <stack_kib> nested divs of <depth> in a thread with given stack
//!   movedeep <chains> <len> <stack_kib> chain moves bypassing depth limit
//!   frame <w> <h> <scale> <iters>  full and partial frame times
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::time::Instant;
use ui4me_renderer::Document;

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LIVE.fetch_add(n as isize - l.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
        .map(|p| p * 4)
        .unwrap_or(0)
}

fn live_kib() -> i64 {
    (LIVE.load(Ordering::Relaxed) / 1024) as i64
}

const LEAK_DOC: &str = r#"<!doctype html><html><head><style>
body{font-family:sans-serif;margin:8px} .b{font-weight:bold} .m{font-family:monospace} .i{font-style:italic}
</style></head><body><h1 id=title>Title</h1><p id=status>0</p><div id=box></div><p>static text</p></body></html>"#;

fn leak(iters: usize, variant: &str) {
    let mut d = Document::parse_hosted(LEAK_DOC, None, 800, 600).unwrap();
    let norender = std::env::var_os("NORENDER").is_some();
    let frame = |d: &mut Document| {
        if norender {
            d.element_rect("status").unwrap();
        } else {
            d.render(800, 600, 1.0).unwrap();
        }
    };
    let frag = |i: usize| match variant {
        "bold" => format!("<p class=b>bold subtree {i}</p>"),
        "mono" => format!("<pre class=m>mono subtree {i}</pre>"),
        "italic" => format!("<p class=i>italic subtree {i}</p>"),
        "both" => format!("<p class=b>bold {i}</p><pre class=m>mono {i}</pre>"),
        "plain" => format!("<p>plain subtree {i}</p>"),
        "boldconst" => "<p class=b>bold subtree constant</p>".to_string(),
        _ => String::new(),
    };
    for i in 0..300 {
        d.set_text("status", &format!("warm {i}")).unwrap();
        if !frag(i).is_empty() {
            d.insert_html("box", "beforeend", &frag(i)).unwrap();
            frame(&mut d);
            d.replace_children_html("box", "").unwrap();
        }
        frame(&mut d);
    }
    let r0 = rss_kib();
    let l0 = live_kib();
    println!("start rss={r0}KiB live={l0}KiB variant={variant} norender={norender}");
    let t0 = Instant::now();
    for i in 1..=iters {
        d.set_text("status", &format!("value {i}")).unwrap();
        frame(&mut d);
        if i % 5 == 0 && !frag(i).is_empty() {
            d.insert_html("box", "beforeend", &frag(i)).unwrap();
            frame(&mut d);
            d.replace_children_html("box", "").unwrap();
            frame(&mut d);
        }
        if i % (iters / 10).max(1) == 0 {
            let r = rss_kib();
            let l = live_kib();
            println!(
                "i={i} rss={r}KiB (+{}) live={l}KiB (+{}) t={:.1}s",
                r as i64 - r0 as i64,
                l - l0,
                t0.elapsed().as_secs_f64()
            );
        }
    }
}

fn big_doc(rows: usize) -> String {
    let mut s = String::from(
        "<!doctype html><html><head><style>body{margin:0;font-family:sans-serif;font-size:14px} .row{padding:4px 8px;border-bottom:1px solid #ddd} #log{height:560px;overflow:auto}</style></head><body><p id=status>0</p><div id=log>",
    );
    for i in 0..rows {
        s.push_str(&format!(
            "<div class=row id=r{i}><b>user{}</b> message {i} with some wrapped words that make the row longer</div>",
            i % 7
        ));
    }
    s.push_str("</div></body></html>");
    s
}

fn settext(rows: usize, iters: usize, w: u32, h: u32, scale: f64) {
    let t = Instant::now();
    let mut d = Document::parse_hosted(&big_doc(rows), None, w, h).unwrap();
    let parse = t.elapsed();
    let t = Instant::now();
    d.render(w, h, scale).unwrap();
    println!(
        "rows={rows} parse={:.1}ms first_render={:.1}ms rss={}KiB",
        parse.as_secs_f64() * 1e3,
        t.elapsed().as_secs_f64() * 1e3,
        rss_kib()
    );
    let mut times = Vec::new();
    let mut mut_times = Vec::new();
    for i in 0..iters {
        let t = Instant::now();
        d.set_text("status", &format!("status {i}")).unwrap();
        let tm = t.elapsed();
        d.render(w, h, scale).unwrap();
        times.push(t.elapsed().as_secs_f64() * 1e3);
        mut_times.push(tm.as_secs_f64() * 1e3);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "set_text+render {w}x{h}@{scale}: median={:.2}ms p90={:.2}ms max={:.2}ms (mutation median {:.3}ms)",
        times[times.len() / 2],
        times[times.len() * 9 / 10],
        times[times.len() - 1],
        mut_times[mut_times.len() / 2]
    );
    // insert_html cost on large doc (validate_document walks the whole doc)
    let mut ins = Vec::new();
    let inserts: usize = std::env::var("INSERTS").ok().and_then(|v| v.parse().ok()).unwrap_or(iters.min(50));
    for i in 0..inserts {
        let t = Instant::now();
        d.insert_html("log", "beforeend", &format!("<div class=row id=x{i}>new {i}</div>"))
            .unwrap();
        ins.push(t.elapsed().as_secs_f64() * 1e3);
    }
    ins.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("insert_html (no render) median={:.2}ms", ins[ins.len() / 2]);
    let t = Instant::now();
    d.render(w, h, scale).unwrap();
    println!("render after 50 inserts={:.1}ms", t.elapsed().as_secs_f64() * 1e3);
    // full frame (scroll invalidates everything)
    let mut sc = Vec::new();
    for i in 0..20 {
        d.scroll_to(Some("log"), Some(0.0), Some((i * 37) as f64)).ok();
        let t = Instant::now();
        d.render(w, h, scale).unwrap();
        sc.push(t.elapsed().as_secs_f64() * 1e3);
    }
    sc.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("scroll+render median={:.2}ms max={:.2}ms", sc[sc.len() / 2], sc[sc.len() - 1]);
}

fn deep(depth: usize, stack_kib: usize) {
    let r = std::thread::Builder::new()
        .stack_size(stack_kib * 1024)
        .spawn(move || {
            let mut s = String::from("<!doctype html><body>");
            for _ in 0..depth {
                s.push_str(if std::env::var_os("PLAIN").is_some() { "<div>" } else { "<div style='padding-left:1px'>" });
            }
            s.push_str("leaf text");
            for _ in 0..depth {
                s.push_str("</div>");
            }
            match Document::parse_hosted(&s, None, 800, 600) {
                Ok(mut d) => {
                    let t = Instant::now();
                    let r = d.render(800, 600, 1.0).map(|_| ());
                    println!("depth={depth} render={:?} in {:.1}ms", r.err().map(|e| e.message), t.elapsed().as_secs_f64() * 1e3);
                    let a = d.accessibility_tree().map(|_| ());
                    println!("a11y={:?}", a.err().map(|e| e.message));
                }
                Err(e) => println!("depth={depth} parse error: {}", e.message),
            }
        })
        .unwrap()
        .join();
    println!("thread result ok={}", r.is_ok());
}

fn movedeep(chains: usize, len: usize, stack_kib: usize) {
    let r = std::thread::Builder::new()
        .stack_size(stack_kib * 1024)
        .spawn(move || {
            let mut s = String::from("<!doctype html><body>");
            for c in 0..chains {
                s.push_str(&format!("<section id=c{c}>"));
                for k in 0..len {
                    s.push_str(&format!("<div id=c{c}d{k}>"));
                }
                s.push_str("x");
                for _ in 0..len {
                    s.push_str("</div>");
                }
                s.push_str("</section>");
            }
            s.push_str("</body>");
            let mut d = Document::parse_hosted(&s, None, 800, 600).expect("parse");
            if std::env::var_os("NOFIRST").is_none() { d.render(800, 600, 1.0).unwrap(); }
            for c in 1..chains {
                let leaf = format!("c{}d{}", c - 1, len - 1);
                d.move_element(&format!("c{c}"), &leaf, "beforeend").expect("move");
            }
            println!("moved; total depth ~{}", chains * (len + 1));
            let t = Instant::now();
            let r = d.render(800, 600, 1.0).map(|_| ());
            println!("render after moves: {:?} {:.1}ms", r.err().map(|e| e.message), t.elapsed().as_secs_f64() * 1e3);
            let r = d.insert_html("c0", "afterbegin", "<p>x</p>");
            println!("insert_html after deep move: {:?}", r.err().map(|e| e.message));
        })
        .unwrap()
        .join();
    println!("thread result ok={}", r.is_ok());
}

fn issue2(ticks: usize) {
    let dir = "<scratch>/exp/RENDER";
    let doc = std::fs::read_to_string(format!("{dir}/doc.html")).unwrap();
    let region = std::fs::read_to_string(format!("{dir}/region.html")).unwrap();
    let css = std::env::var("LEAK_CSSTEXT").unwrap_or_default();
    let a11y = std::env::var_os("A11Y").is_some();
    let norender = std::env::var_os("NORENDER").is_some();
    let period: usize = std::env::var("PERIOD").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
    let mut d = Document::parse_hosted(&doc, None, 500, 400).unwrap();
    if !css.is_empty() {
        d.add_stylesheet(&css).unwrap();
    }
    let mut generation = 0usize;
    let mut current: Option<String> = None;
    let r0 = rss_kib();
    let l0 = live_kib();
    println!("start rss={r0} live={l0} css={css:?} a11y={a11y} norender={norender}");
    let t0 = Instant::now();
    use ui4me_renderer::DomOp;
    for n in 1..=ticks {
        let mut ops = vec![DomOp::SetText { element: "u4-6".into(), text: n.to_string() }];
        if current.is_some() {
            ops.push(DomOp::SetText { element: format!("g{generation}-u4-11"), text: format!("region tick {n}") });
        }
        d.apply_batch(&ops).unwrap();
        if n % period == 0 {
            if let Some(c) = current.take() {
                d.apply_batch(&[DomOp::RemoveElement { element: c }]).unwrap();
            } else {
                generation += 1;
                let mut html = region.clone();
                for k in (9..=20).rev() {
                    html = html.replace(&format!("id=\"u4-{k}\""), &format!("id=\"g{generation}-u4-{k}\""));
                }
                d.apply_batch(&[DomOp::InsertHtml { element: "u4-8".into(), position: "beforeend".into(), html }]).unwrap();
                current = Some(format!("g{generation}-u4-9"));
            }
        }
        if norender { d.element_rect("u4-6").unwrap(); } else { d.render(500, 400, 1.0).unwrap(); }
        if a11y {
            let t = d.accessibility_tree().unwrap();
            let _ = ui4me_renderer::a11y::full_update(&t);
        }
        let _ = d.drain_events();
        if n % std::env::var("SAMPLE").ok().and_then(|v| v.parse().ok()).unwrap_or((ticks / 10).max(1)) == 0 {
            let r = rss_kib();
            let l = live_kib();
            println!("n={n} rss={r} (+{}) live={l} (+{}) t={:.1}s", r as i64 - r0 as i64, l - l0, t0.elapsed().as_secs_f64());
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n = |i: usize, d: usize| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
    match a.get(1).map(String::as_str) {
        Some("leak") => leak(n(2, 2000), a.get(3).map_or("bold", String::as_str)),
        Some("settext") => settext(
            n(2, 2000),
            n(3, 100),
            n(4, 1280) as u32,
            n(5, 800) as u32,
            a.get(6).and_then(|v| v.parse().ok()).unwrap_or(1.0),
        ),
        Some("issue2") => issue2(n(2, 7500)),
        Some("profile") => { let (v, f) = ui4me_renderer::css_profile(); println!("{v}"); for (id, st) in f { println!("{id}\t{st}"); } }
        Some("deep") => deep(n(2, 255), n(3, 1024)),
        Some("movedeep") => movedeep(n(2, 8), n(3, 200), n(4, 8192)),
        _ => eprintln!("usage: see source"),
    }
}
