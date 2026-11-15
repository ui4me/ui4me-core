//! T1, SVG part (F-03): image SVG at the pre-scan depth cap decodes and rasterises on **half** the
//! asset-worker stack; anything beyond the cap, and reference cycles, fail cleanly with
//! `DECODE_FAILED` instead of overflowing the stack (a process abort).
//!
//! Each accepted case runs in a child process (this binary, re-entered through the ignored
//! `svg_probe` test) on a thread with the stack under test, so an overflow fails the test instead
//! of aborting the suite. Rejected cases are checked in-process: the pre-scan refuses them before
//! usvg recurses.

use std::process::Command;
use ui4me_assets::decode::{SvgOptions, SvgRenderer};
use ui4me_assets::svg::prescan::{self, SVG_MAX_DEPTH};
use ui4me_assets::svg::usvg_adapter::UsvgRenderer;
use ui4me_assets::worker::ASSET_WORKER_STACK_BYTES;
use ui4me_engine_types::codes_generated::Code;

const OPEN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="40">"#;

/// `<g>` nesting: the root plus `levels - 2` groups plus the rect = `levels`.
fn groups(levels: usize) -> String {
    let n = levels - 2;
    format!(
        "{OPEN}{}<rect width='10' height='10' fill='red'/>{}</svg>",
        "<g>".repeat(n),
        "</g>".repeat(n)
    )
}

/// `n` chained `clipPath`s, each clipped by the previous one, applied to a rect.
fn clip_chain(n: usize) -> String {
    let mut s = format!("{OPEN}<defs>");
    for i in 0..n {
        let clip = if i == 0 {
            String::new()
        } else {
            format!(" clip-path='url(#c{})'", i - 1)
        };
        s.push_str(&format!(
            "<clipPath id='c{i}'{clip}><rect width='{}' height='40'/></clipPath>",
            40 - i % 20
        ));
    }
    s.push_str(&format!(
        "</defs><rect width='40' height='40' fill='blue' clip-path='url(#c{})'/></svg>",
        n - 1
    ));
    s
}

/// `n` masks chained through their own `mask` attribute.
fn mask_chain(n: usize) -> String {
    let mut s = format!("{OPEN}<defs>");
    for i in 0..n {
        let mask = if i == 0 {
            String::new()
        } else {
            format!(" mask='url(#m{})'", i - 1)
        };
        s.push_str(&format!(
            "<mask id='m{i}'{mask}><rect width='40' height='40' fill='white'/></mask>"
        ));
    }
    s.push_str(&format!(
        "</defs><rect width='40' height='40' fill='blue' mask='url(#m{})'/></svg>",
        n - 1
    ));
    s
}

/// `n` `use` elements, each instantiating a group holding the previous one.
fn use_chain(n: usize) -> String {
    let mut s = format!("{OPEN}<defs><rect id='u0' width='10' height='10'/>");
    for i in 1..n {
        s.push_str(&format!("<g id='u{i}'><use href='#u{}'/></g>", i - 1));
    }
    s.push_str(&format!("</defs><use href='#u{}'/></svg>", n - 1));
    s
}

/// The same chain through a style sheet instead of attributes.
fn css_clip_chain(n: usize) -> String {
    let mut s = format!("{OPEN}<style>");
    for i in 1..n {
        s.push_str(&format!("#c{i} {{ clip-path: url(#c{}) }} ", i - 1));
    }
    s.push_str(&format!(
        ".t {{ clip-path: url(#c{}) }}</style><defs>",
        n - 1
    ));
    for i in 0..n {
        s.push_str(&format!(
            "<clipPath id='c{i}'><rect width='40' height='40'/></clipPath>"
        ));
    }
    s.push_str("</defs><rect class='t' width='40' height='40'/></svg>");
    s
}

/// A `data:` SVG image whose content is `inner`, inside `outer_levels` of groups.
fn nested_image(outer_levels: usize, inner: &str) -> String {
    let encoded: String = inner
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let url = format!("data:image/svg+xml,{encoded}");
    let n = outer_levels - 2;
    format!(
        "{OPEN}{}<image width='40' height='40' href=\"{url}\"/>{}</svg>",
        "<g>".repeat(n),
        "</g>".repeat(n)
    )
}

fn case(name: &str) -> String {
    let cap = SVG_MAX_DEPTH as usize;
    match name {
        "groups-at-cap" => groups(cap),
        // Each chained clipPath or mask adds one level (its conversion recurses into the previous
        // one); the root, the target rect and the first clip path's rect add three.
        "clip-chain-at-cap" => clip_chain(cap - 3),
        "mask-chain-at-cap" => mask_chain(cap - 3),
        "css-clip-chain-at-cap" => css_clip_chain(cap - 3),
        // Each link instantiates a group and a `use`: two levels.
        "use-chain-at-cap" => use_chain(cap / 2 - 1),
        // The inner document's depth plus the outer document's stays within the cap.
        "nested-image-at-cap" => nested_image(cap / 2, &groups(cap / 2)),
        other => panic!("unknown case {other}"),
    }
}

const ACCEPTED: &[&str] = &[
    "groups-at-cap",
    "clip-chain-at-cap",
    "mask-chain-at-cap",
    "use-chain-at-cap",
    "css-clip-chain-at-cap",
    "nested-image-at-cap",
];

fn decode(svg: &str) -> Result<(), Code> {
    let r = UsvgRenderer;
    let parsed = r
        .parse(svg.as_bytes(), &SvgOptions::default())
        .map_err(|s| s.code)?;
    if !parsed.warnings.is_empty() {
        return Err(parsed.warnings[0]);
    }
    r.rasterize(&parsed.tree, 40, 40).map_err(|s| s.code)?;
    Ok(())
}

/// Child-process entry: `UI4ME_SVG_PROBE=<case>,<stack KiB>`.
#[test]
#[ignore = "child-process entry of the SVG depth tests"]
fn svg_probe() {
    let Ok(spec) = std::env::var("UI4ME_SVG_PROBE") else {
        return;
    };
    let (name, kib) = spec.split_once(',').unwrap();
    let (svg, kib) = (case(name), kib.parse::<usize>().unwrap());
    let r = std::thread::Builder::new()
        .stack_size(kib * 1024)
        .spawn(move || decode(&svg))
        .unwrap()
        .join()
        .expect("probe panicked");
    match r {
        Ok(()) => println!("PROBE_OK"),
        Err(code) => println!("PROBE_ERR {code:?}"),
    }
}

fn probe(name: &str, kib: usize) -> Result<(), String> {
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "svg_probe",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("UI4ME_SVG_PROBE", format!("{name},{kib}"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    if out.status.success() && stdout.contains("PROBE_OK") {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(format!(
        "{:?} {} {}",
        out.status,
        stdout
            .lines()
            .find(|l| l.starts_with("PROBE_ERR"))
            .unwrap_or(""),
        stderr.lines().rev().take(2).collect::<Vec<_>>().join(" | ")
    ))
}

#[test]
fn image_svg_at_the_depth_cap_decodes_on_half_the_worker_stack() {
    let kib = ASSET_WORKER_STACK_BYTES / 2 / 1024;
    let failures: Vec<String> = ACCEPTED
        .iter()
        .filter_map(|name| probe(name, kib).err().map(|e| format!("{name}: {e}")))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_cases_are_at_the_cap() {
    for name in ACCEPTED {
        let mut depth = prescan::scan(case(name).as_bytes(), 1_000_000)
            .unwrap()
            .depth;
        if *name == "nested-image-at-cap" {
            depth += prescan::scan(groups(SVG_MAX_DEPTH as usize / 2).as_bytes(), 1000)
                .unwrap()
                .depth;
        }
        assert!(
            depth > SVG_MAX_DEPTH - 4 && depth <= SVG_MAX_DEPTH,
            "{name}: effective depth {depth}"
        );
    }
}

fn refused(svg: &str) {
    assert_eq!(
        prescan::scan(svg.as_bytes(), 1_000_000).unwrap_err().code,
        Code::DecodeFailed
    );
    assert_eq!(
        UsvgRenderer
            .parse(svg.as_bytes(), &SvgOptions::default())
            .err()
            .map(|s| s.code),
        Some(Code::DecodeFailed)
    );
}

#[test]
fn beyond_the_cap_is_a_clean_decode_failure() {
    let cap = SVG_MAX_DEPTH as usize;
    refused(&groups(cap + 1));
    refused(&groups(700)); // the audit's crash (svgdepth.py 700)
    refused(&clip_chain(2000)); // the audit's chained clip-path crash
    refused(&clip_chain(cap - 2));
    refused(&mask_chain(cap - 2));
    refused(&use_chain(cap / 2));
    refused(&css_clip_chain(cap - 2));
    // Inherited paint: a pattern under groups is used by the deepest shape.
    let mut p = format!(
        "{OPEN}<defs><pattern id='p' width='4' height='4'>{}<rect width='2' height='2'/>{}</pattern></defs><g fill='url(#p)'>",
        "<g>".repeat(cap / 2),
        "</g>".repeat(cap / 2)
    );
    p.push_str(&"<g>".repeat(cap / 2));
    p.push_str("<rect width='40' height='40'/>");
    p.push_str(&"</g>".repeat(cap / 2));
    p.push_str("</g></svg>");
    refused(&p);
}

#[test]
fn a_nested_image_shares_the_outer_depth_budget() {
    let cap = SVG_MAX_DEPTH as usize;
    // Each document alone is within the cap; together they are not: the image is dropped
    // (DECODE_FAILED warning), the outer document still decodes.
    let svg = nested_image(cap / 2 + 8, &groups(cap / 2 + 8));
    let parsed = UsvgRenderer
        .parse(svg.as_bytes(), &SvgOptions::default())
        .expect("outer document decodes");
    assert_eq!(parsed.warnings, vec![Code::DecodeFailed]);
}

#[test]
fn reference_cycles_are_refused() {
    // Three-step clip-path cycle: usvg only breaks one- and two-step cycles.
    refused(&format!(
        "{OPEN}<clipPath id='a' clip-path='url(#b)'><rect width='9' height='9'/></clipPath><clipPath id='b' clip-path='url(#c)'><rect width='9' height='9'/></clipPath><clipPath id='c' clip-path='url(#a)'><rect width='9' height='9'/></clipPath><rect width='9' height='9' clip-path='url(#a)'/></svg>"
    ));
    // A pattern whose content inherits the pattern itself as its fill.
    refused(&format!(
        "{OPEN}<g fill='url(#p)'><pattern id='p' width='4' height='4'><rect width='2' height='2'/></pattern><rect width='9' height='9'/></g></svg>"
    ));
    // `use` of its own ancestor.
    refused(&format!("{OPEN}<g id='g'><use href='#g'/></g></svg>"));
}

#[test]
fn ordinary_references_are_accepted() {
    // An exporter-style file: classes in a style sheet referencing gradients and clip paths,
    // gradients chained by href, markers, a symbol instantiated twice, a filter.
    let svg = format!(
        "{OPEN}<defs><style>.a{{fill:url(#lg)}} .b{{clip-path:url(#cp)}} .c{{fill:url(#lg2);stroke:url(#lg)}}</style>\
         <linearGradient id='lg'><stop offset='0' stop-color='red'/><stop offset='1' stop-color='blue'/></linearGradient>\
         <linearGradient id='lg2' href='#lg'/>\
         <clipPath id='cp'><rect class='a' width='30' height='30'/></clipPath>\
         <marker id='m' markerWidth='4' markerHeight='4'><circle r='2' class='c'/></marker>\
         <symbol id='s'><rect class='a' width='5' height='5'/></symbol>\
         <filter id='f'><feGaussianBlur stdDeviation='1'/></filter></defs>\
         <g class='b' filter='url(#f)'><rect class='a' width='40' height='40'/>\
         <path d='M0 0 L10 10' stroke='black' marker-end='url(#m)'/>\
         <use href='#s'/><use href='#s' x='10'/></g></svg>"
    );
    decode(&svg).expect("an ordinary illustration decodes");
    assert!(prescan::scan(svg.as_bytes(), 1000).unwrap().depth < 16);
    // A gradient as an inherited fill of the root does not make its stops recursive.
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='9' height='9' fill='url(#g)'><linearGradient id='g'><stop offset='0'/></linearGradient><rect width='9' height='9'/></svg>";
    decode(svg).expect("root gradient fill decodes");
}

#[test]
fn exponential_reference_fan_out_is_refused() {
    // Found by harness_svg_depth: each mask is masked by the previous one and also holds a path
    // masked by it, so usvg converts mask 0 2^n times (30 s and more at n = 25, depth only 27).
    let mut s = format!("{OPEN}<mask id='r0'><path d='M0 0 L9 9'/></mask>");
    for i in 1..30 {
        s.push_str(&format!(
            "<mask id='r{i}' mask='url(#r{p})'><path d='M0 0 L9 9' mask='url(#r{p})'/></mask>",
            p = i - 1
        ));
    }
    s.push_str("<path d='M0 0 L9 9' mask='url(#r29)'/></svg>");
    let started = std::time::Instant::now();
    refused(&s);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn overly_complex_selectors_are_refused() {
    // simplecss matches compound selectors recursively.
    let selector = vec!["g"; 2000].join(" + ");
    refused(&format!(
        "{OPEN}<style>{selector} {{ fill: red }}</style><rect width='9' height='9'/></svg>"
    ));
    let ok = ["g"; 8].join(" > ");
    decode(&format!(
        "{OPEN}<style>{ok} {{ fill: red }}</style><rect width='9' height='9'/></svg>"
    ))
    .unwrap();
}
