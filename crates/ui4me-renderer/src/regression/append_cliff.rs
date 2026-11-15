// T13 part (F-16a / RENDER-05): appending a row to a long list after many unrelated updates must not
// shape the list again. Rows whose inline containers were carried over from the previous pass (seeded
// replay) were never looked up in the content cache, so their entries aged out after `RETAIN_PASSES`
// passes; the next structural change of the list then shaped every row (1.0–1.8 s at 2,000 rows).
use crate::document::Document;

const ROWS: usize = 2_000;

fn big_doc(rows: usize) -> String {
    let mut s = String::from(
        "<!doctype html><html><head><style>body{margin:0;font-family:sans-serif;font-size:14px} \
         .row{padding:4px 8px;border-bottom:1px solid #ddd} #log{height:560px;overflow:auto}</style></head>\
         <body><p id=status>0</p><div id=log>",
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

fn shaped(d: &mut Document) -> u64 {
    d.pipeline_mut_for_testing()
        .text_services()
        .paragraphs_shaped()
}

fn append_after_unrelated_updates(updates: usize) -> u64 {
    let mut d = Document::parse_hosted(&big_doc(ROWS), None, 1280, 800).unwrap();
    d.render(1280, 800, 1.0).unwrap();
    for i in 0..updates {
        d.set_text("status", &format!("status {i}")).unwrap();
        d.render(1280, 800, 1.0).unwrap();
    }
    let before = shaped(&mut d);
    d.insert_html(
        "log",
        "beforeend",
        "<div class=row id=appended><b>user</b> one more line</div>",
    )
    .unwrap();
    d.render(1280, 800, 1.0).unwrap();
    shaped(&mut d) - before
}

#[test]
fn append_after_twenty_unrelated_updates_shapes_at_most_two_paragraphs() {
    let n = append_after_unrelated_updates(20);
    eprintln!("append after 20 unrelated updates shaped {n} paragraphs");
    assert!(
        n <= 2,
        "appending one row to {ROWS} rows after 20 unrelated updates shaped {n} paragraphs"
    );
}

fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Measurement harness (release build), F-16a / F-16b / RENDER-07 / RENDER-08:
/// `UI4ME_AC_ROWS=2000 cargo test --release -p ui4me-renderer --lib append_cliff_timing -- --ignored --nocapture`
#[test]
#[ignore = "measurement harness"]
fn append_cliff_timing() {
    let rows = std::env::var("UI4ME_AC_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(ROWS);
    for (w, h) in [(1280u32, 800u32), (3840, 2160)] {
        for updates in [7usize, 20] {
            let t = std::time::Instant::now();
            let mut d = Document::parse_hosted(&big_doc(rows), None, w, h).unwrap();
            let parse = ms(t);
            let s0 = shaped(&mut d);
            let t = std::time::Instant::now();
            d.render(w, h, 1.0).unwrap();
            let first = ms(t);
            let s1 = shaped(&mut d);
            let mut upd = Vec::new();
            for i in 0..updates {
                let t = std::time::Instant::now();
                d.set_text("status", &format!("status {i}")).unwrap();
                d.render(w, h, 1.0).unwrap();
                upd.push(ms(t));
            }
            let s2 = shaped(&mut d);
            let t = std::time::Instant::now();
            d.insert_html(
                "log",
                "beforeend",
                "<div class=row id=appended><b>user</b> one more line</div>",
            )
            .unwrap();
            d.render(w, h, 1.0).unwrap();
            let append = ms(t);
            let s3 = shaped(&mut d);
            // A full frame of the same document (scrolling invalidates the whole surface).
            let mut full = Vec::new();
            for i in 0..5 {
                d.scroll_to(Some("log"), Some(0.0), Some((i * 37) as f64))
                    .unwrap();
                let t = std::time::Instant::now();
                d.render(w, h, 1.0).unwrap();
                full.push(ms(t));
            }
            println!(
                "rows={rows} {w}x{h} updates={updates}: parse={parse:.1}ms first_render={first:.1}ms (shaped {}) \
                 set_text+render median={:.2}ms max={:.2}ms (shaped {}) append+render={append:.1}ms (shaped {}) \
                 scroll+render median={:.2}ms",
                s1 - s0,
                median(upd.clone()),
                upd.iter().copied().fold(0.0, f64::max),
                s2 - s1,
                s3 - s2,
                median(full)
            );
        }
    }
}
