//! F-14 / F-15 (roadmap 2.5, gate R5(a)): small inputs must not cost unbounded work, and a paint
//! budget overflow must not fail the document.
//!
//! Every reproduction of the audit (INPUTSEC-03..06) runs through the public document API and must
//! finish within [`budget`] (250 ms in a release build, scaled for debug builds) or be rejected
//! cleanly. Timing tests are coarse on purpose: the fixed paths are orders of magnitude below the
//! bound and the broken ones orders of magnitude above it.
//!
//! Measuring: `UI4ME_STALL_MEASURE=1 cargo test --release -p ui4me-renderer --test content_stalls
//! -- --ignored --nocapture stall_measure` prints the time of every case at several sizes.

use std::time::{Duration, Instant};
use ui4me_renderer::Document;

/// Generous bound for one reproduction: 250 ms (gate R5(a)) in release, 20x in a debug build.
fn budget() -> Duration {
    if cfg!(debug_assertions) {
        Duration::from_millis(5_000)
    } else {
        Duration::from_millis(250)
    }
}

/// Deep documents are only renderable on the engine stack; run the body on it.
fn on_engine_stack<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(ui4me_renderer::ENGINE_STACK_BYTES)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|p| std::panic::resume_unwind(p))
}

/// Time of `body` on the engine stack, or `None` when it has not finished after four times the
/// budget (the broken paths take seconds to effectively forever; the stuck thread is abandoned).
fn bounded<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> Option<(Duration, T)> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(ui4me_renderer::ENGINE_STACK_BYTES)
        .spawn(move || {
            let t0 = Instant::now();
            let out = body();
            let _ = tx.send((t0.elapsed(), out));
        })
        .unwrap();
    rx.recv_timeout(budget() * 4).ok()
}

/// A document that has rendered once (its text services are set up: that first-use cost is the
/// same for every document and not what these tests bound).
fn warm_document() -> Document {
    let mut d = Document::parse_hosted("<!doctype html><body><div id=host>x</div>", None, 400, 400)
        .unwrap();
    d.render(400, 400, 1.0).unwrap();
    d
}

/// The body of a document from [`nested`] and friends.
fn body_of(html: &str) -> String {
    html.trim_start_matches("<!doctype html><body>")
        .trim_end_matches("</body>")
        .to_string()
}

/// Replace the content of a warm document with `markup` and render it: the time of the change and
/// the frame (`Err` with the message when the change or the frame is refused).
fn assert_bounded_change(what: &str, markup: String) -> Result<(), String> {
    let r = bounded(move || {
        let mut d = warm_document();
        let t0 = Instant::now();
        let r = d
            .replace_children_html("host", &markup)
            .and_then(|_| d.render(400, 400, 1.0).map(|_| ()))
            .map_err(|e| e.message);
        (t0.elapsed(), r)
    });
    let Some((_, (t, r))) = r else {
        panic!("{what}: no result after {:?}", budget() * 4);
    };
    assert!(t < budget(), "{what}: {t:?} (budget {:?})", budget());
    r
}

/// Parse, render until resources settle (bounded), return the time and the last frame.
fn timed_render(html: &str, w: u32, h: u32) -> (Duration, Result<Vec<u32>, String>) {
    let t0 = Instant::now();
    let mut d = match Document::parse_hosted(html, None, w, h) {
        Ok(d) => d,
        Err(e) => return (t0.elapsed(), Err(e.message)),
    };
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        let r = d.render(w, h, 1.0).map_err(|e| e.message);
        if r.is_err() || !d.has_pending_resources() || Instant::now() > end {
            return (t0.elapsed(), r);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

// ---- generators (shared with the measurement tool)

pub fn svg_filter(primitives: &str, size: u32) -> String {
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{size}' height='{size}'>\
         <filter id='f'>{primitives}</filter>\
         <rect width='{size}' height='{size}' fill='teal' filter='url(#f)'/></svg>"
    )
}

pub fn turbulence(octaves: u32) -> String {
    format!("<feTurbulence baseFrequency='0.05' numOctaves='{octaves}'/>")
}

pub fn morphology(radius: u32) -> String {
    format!("<feMorphology operator='dilate' radius='{radius}'/>")
}

pub fn convolve(order: u32) -> String {
    let n = (order * order) as usize;
    let kernel = vec!["1"; n].join(" ");
    format!("<feConvolveMatrix order='{order}' kernelMatrix='{kernel}'/>")
}

pub fn blur_chain(n: usize) -> String {
    "<feGaussianBlur stdDeviation='1'/>".repeat(n)
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The SVG as an inline `<svg>` or as an `<img>` `data:` image.
pub fn svg_page(svg: &str, inline: bool) -> String {
    if inline {
        format!("<!doctype html><body style='margin:0'>{svg}</body>")
    } else {
        format!(
            "<!doctype html><body style='margin:0'><img src='data:image/svg+xml;base64,{}'></body>",
            b64(svg.as_bytes())
        )
    }
}

/// `depth` nested `<tag style=...>` levels around a short text.
pub fn nested(open: &str, close: &str, depth: usize) -> String {
    format!(
        "<!doctype html><body>{}x{}</body>",
        open.repeat(depth),
        close.repeat(depth)
    )
}

pub fn inline_blocks(depth: usize) -> String {
    nested("<div style='display:inline-block'>", "</div>", depth)
}

pub fn span_inline_blocks(depth: usize) -> String {
    nested(
        "<span><div style='display:inline-block'>",
        "</div></span>",
        depth / 2,
    )
}

/// Unclosed `<div>`s: the parser's open-element stack grows with every tag.
pub fn unclosed_divs(n: usize) -> String {
    format!("<!doctype html><body>{}x", "<div>".repeat(n))
}

#[test]
#[ignore = "measurement tool; run explicitly (see the module documentation)"]
fn stall_measure() {
    if std::env::var_os("UI4ME_STALL_MEASURE").is_none() {
        return;
    }
    let only = std::env::var("UI4ME_STALL_ONLY").unwrap_or_default();
    let cases: Vec<(String, String, u32, u32)> = {
        let mut v = Vec::new();
        let mut add = |name: String, html: String| {
            v.push((name, html, 400u32, 400u32));
        };
        for inline in [true, false] {
            let how = if inline { "inline" } else { "img" };
            for oct in [4, 100, 1000, 10000] {
                add(
                    format!("turbulence {how} numOctaves={oct} 300x300"),
                    svg_page(&svg_filter(&turbulence(oct), 300), inline),
                );
            }
            for r in [10, 100, 1000] {
                add(
                    format!("morphology {how} radius={r} 300x300"),
                    svg_page(&svg_filter(&morphology(r), 300), inline),
                );
            }
            for o in [3, 30] {
                add(
                    format!("convolve {how} order={o} 300x300"),
                    svg_page(&svg_filter(&convolve(o), 300), inline),
                );
            }
            for n in [10, 1500] {
                add(
                    format!("blur chain {how} n={n} 300x300"),
                    svg_page(&svg_filter(&blur_chain(n), 300), inline),
                );
            }
        }
        // Baseline cost of a document (first-use font set-up included).
        add("control: one div".into(), nested("<div>", "</div>", 1));
        add(
            "control: 250 nested div".into(),
            nested("<div>", "</div>", 250),
        );
        let deep: Vec<usize> = std::env::var("UI4ME_STALL_DEPTHS")
            .map(|v| v.split(',').map(|d| d.parse().unwrap()).collect())
            .unwrap_or_else(|_| vec![16, 20, 22, 24, 26, 64, 250]);
        for &d in &deep {
            add(format!("inline-block depth {d}"), inline_blocks(d));
        }
        for &d in &deep {
            add(
                format!("span/inline-block depth {d}"),
                span_inline_blocks(d),
            );
        }
        for &d in &deep {
            add(
                format!("button depth {d}"),
                nested("<button><span>", "</span></button>", d / 2),
            );
        }
        for &d in &deep {
            add(
                format!("multicol depth {d}"),
                nested("<div style='columns:2'>", "</div>", d),
            );
        }
        for n in [5_000, 10_000, 20_000, 40_000] {
            add(format!("unclosed div x{n}"), unclosed_divs(n));
        }
        v
    };
    println!("| case | ms | result |");
    println!("|---|---|---|");
    for (name, html, w, h) in cases {
        if !only.is_empty() && !name.contains(&only) {
            continue;
        }
        let (t, r) = on_engine_stack(move || timed_render(&html, w, h));
        let r = match r {
            Ok(_) => "rendered".to_string(),
            Err(e) => e.chars().take(80).collect(),
        };
        println!("| {name} | {:.1} | {r} |", t.as_secs_f64() * 1e3);
    }
}

// ---- INPUTSEC-06: shrink-to-fit atomics are laid out in time polynomial in their nesting depth.

/// The deepest chain the depth limit admits below `#host` (html 1, body 2, host 3, the text 4 below
/// the last level), in a release build; debug builds use a shallower chain of the same shape.
fn atomic_depth() -> usize {
    if cfg!(debug_assertions) { 96 } else { 250 }
}

#[test]
fn nested_inline_blocks_lay_out_in_bounded_time() {
    let d = atomic_depth();
    assert_bounded_change("inline-block chain", body_of(&inline_blocks(d))).unwrap();
}

#[test]
fn inline_blocks_inside_inline_boxes_lay_out_in_bounded_time() {
    let d = atomic_depth();
    assert_bounded_change("span/inline-block chain", body_of(&span_inline_blocks(d))).unwrap();
}

#[test]
fn nested_buttons_lay_out_in_bounded_time() {
    // The parser closes an open button at the next `<button>`; a fragment parsed into a button
    // nests (the context element is not on the stack of open elements).
    let r = bounded(|| {
        let mut d = warm_document();
        let levels = atomic_depth() / 2;
        let t0 = Instant::now();
        d.replace_children_html("host", "<button id=b0>b</button>")
            .unwrap();
        for i in 1..levels {
            d.insert_html(
                &format!("b{}", i - 1),
                "beforeend",
                &format!("<button id=b{i}>b</button>"),
            )
            .unwrap();
        }
        d.render(400, 400, 1.0).unwrap();
        assert!(
            d.element_rect(&format!("b{}", levels - 1))
                .unwrap()
                .is_some()
        );
        t0.elapsed()
    });
    let Some((_, t)) = r else {
        panic!("nested buttons: no result after {:?}", budget() * 4);
    };
    assert!(t < budget(), "nested buttons: {t:?}");
}

#[test]
fn inline_block_layout_matches_a_shallow_reference() {
    // The memoised intrinsic widths give the layout the uncached one gave: every level of a short
    // chain of shrink-to-fit boxes with padding is as wide as its content plus its padding.
    on_engine_stack(|| {
        let mut html =
            String::from("<!doctype html><body style='margin:0;font:16px/20px sans-serif'>");
        for i in 0..8 {
            html.push_str(&format!(
                "<div id=l{i} style='display:inline-block;padding:0 2px'>"
            ));
        }
        html.push_str("wide text");
        html.push_str(&"</div>".repeat(8));
        let mut d = Document::parse_hosted(&html, None, 400, 400).unwrap();
        d.render(400, 400, 1.0).unwrap();
        let w: Vec<f64> = (0..8)
            .map(|i| d.element_rect(&format!("l{i}")).unwrap().unwrap().width)
            .collect();
        for i in 1..8 {
            assert!((w[i - 1] - w[i] - 4.0).abs() < 0.02, "level widths {w:?}");
        }
    });
}

/// `levels` repetitions of `units` (opening, closing markup) around a short text.
fn chain(units: &[(&str, &str)], levels: usize) -> String {
    let (mut open, mut close) = (String::new(), Vec::new());
    for i in 0..levels {
        let (o, c) = units[i % units.len()];
        open.push_str(o);
        close.push(c);
    }
    open.push('x');
    for c in close.iter().rev() {
        open.push_str(c);
    }
    open
}

#[test]
fn atomics_inside_balanced_multicol_lay_out_in_bounded_time() {
    // Each balancing pass of a multicol container lays its content out again; an atomic in it
    // used to clear and re-lay everything below on every pass.
    let units = [
        ("<div style='display:inline-block'>", "</div>"),
        ("<div style='columns:2'>", "</div>"),
    ];
    assert_bounded_change("inline-block/multicol chain", chain(&units, atomic_depth())).unwrap();
}

#[test]
fn multi_pass_containers_between_atomics_lay_out_in_bounded_time() {
    // The depth fuzzer's reduced case: flex, grid, table, fieldset and multicol each lay their
    // content out more than once, with atomics between them.
    let units = [
        ("<div style='display:inline-block'>", "</div>"),
        ("<span><div style='display:inline-block'>", "</div></span>"),
        ("<div style='display:grid'>", "</div>"),
        ("<ul><li>", "</li></ul>"),
        ("<table><tbody><tr><td>", "</td></tr></tbody></table>"),
        (
            "<button><table><tbody><tr><td>",
            "</td></tr></tbody></table></button>",
        ),
        ("<fieldset>", "</fieldset>"),
        ("<div style='display:flex'>", "</div>"),
        ("<div style='columns:2'>", "</div>"),
        ("<fieldset><legend>l</legend>", "</fieldset>"),
        ("<details open><summary>s</summary>", "</details>"),
    ];
    // About 2.5 DOM levels per unit: stay within the depth limit below `#host`.
    let levels = if cfg!(debug_assertions) { 30 } else { 90 };
    assert_bounded_change("mixed chain", chain(&units, levels)).unwrap();
}

// ---- INPUTSEC-05: deeply nested unclosed blocks are refused without a quadratic parse.

#[test]
fn deep_unclosed_blocks_are_refused_quickly() {
    for (what, html) in [
        ("load", unclosed_divs(40_000)),
        (
            "load, inline",
            format!("<!doctype html><body>{}x", "<span><div>".repeat(20_000)),
        ),
    ] {
        let t0 = Instant::now();
        let e = Document::parse_hosted(&html, None, 400, 400)
            .err()
            .unwrap_or_else(|| panic!("{what}: accepted"));
        let t = t0.elapsed();
        assert!(e.message.starts_with("SIZE_LIMIT"), "{what}: {}", e.message);
        assert_eq!(e.code, 5);
        assert!(t < budget(), "{what}: {t:?}");
    }
    // The same markup inserted into a document.
    let r = bounded(|| {
        let mut d = warm_document();
        let t0 = Instant::now();
        let e = d
            .insert_html("host", "beforeend", &"<div>".repeat(40_000))
            .expect_err("too deep");
        (
            t0.elapsed(),
            e.message,
            d.get_html(None, None, true).unwrap(),
        )
    });
    let (_, (t, message, html)) = r.expect("no result");
    assert!(message.starts_with("SIZE_LIMIT"), "{message}");
    assert!(t < budget(), "insert: {t:?}");
    assert!(
        html.contains("<div id=\"host\">x</div>"),
        "the document is unchanged: {html}"
    );
}

