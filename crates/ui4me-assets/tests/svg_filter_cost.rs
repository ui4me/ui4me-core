//! F-14 (INPUTSEC-03): SVG filter work is bounded per rasterisation.
//!
//! An SVG image is rasterised on the GUI thread at paint time; resvg runs filter primitives with
//! no limit (a 300x300 `feTurbulence numOctaves="10000"` took 47 s). The adapter estimates the
//! filter work of a raster (`svg::filter_cost`) and draws the image without its filters when the
//! estimate is over `SVG_FILTER_WORK_BUDGET`.
//!
//! Calibration: `UI4ME_FILTER_CALIBRATE=1 cargo test --release -p ui4me-assets --test
//! svg_filter_cost -- --ignored --nocapture calibrate` times resvg on each primitive and prints the
//! measured and estimated work.

use std::time::{Duration, Instant};
use ui4me_assets::decode::{SvgOptions, SvgRenderer};
use ui4me_assets::policy::SVG_FILTER_WORK_BUDGET;
use ui4me_assets::svg::filter_cost::filter_work;
use ui4me_assets::svg::usvg_adapter::UsvgRenderer;

fn filtered(primitives: &str, size: u32) -> String {
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{size}' height='{size}'>\
         <filter id='f'>{primitives}</filter>\
         <rect width='{size}' height='{size}' fill='teal' filter='url(#f)'/></svg>"
    )
}

fn usvg_tree(svg: &str) -> usvg::Tree {
    usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap()
}

/// Time of drawing `svg` with resvg directly (no budget) and the estimate, at `size` px.
fn measure(svg: &str, size: u32) -> (Duration, u64) {
    let tree = usvg_tree(svg);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
    let t0 = Instant::now();
    resvg::render(&tree, usvg::Transform::identity(), &mut pixmap.as_mut());
    let t = t0.elapsed();
    (
        t,
        filter_work(&tree, usvg::Transform::identity(), size, size),
    )
}

fn cases() -> Vec<(&'static str, String)> {
    let kernel = |n: u32| vec!["1"; (n * n) as usize].join(" ");
    vec![
        ("none (control)", "<feOffset dx='0'/>".into()),
        ("blur 1 (iir)", "<feGaussianBlur stdDeviation='1'/>".into()),
        ("blur 8 (box)", "<feGaussianBlur stdDeviation='8'/>".into()),
        ("blur 200 (box)", "<feGaussianBlur stdDeviation='200'/>".into()),
        ("drop shadow 4", "<feDropShadow stdDeviation='4'/>".into()),
        ("turbulence 4", "<feTurbulence baseFrequency='0.05' numOctaves='4'/>".into()),
        ("turbulence 50", "<feTurbulence baseFrequency='0.05' numOctaves='50'/>".into()),
        ("morphology 5", "<feMorphology operator='dilate' radius='5'/>".into()),
        ("morphology 40", "<feMorphology operator='dilate' radius='40'/>".into()),
        ("convolve 3", format!("<feConvolveMatrix order='3' kernelMatrix='{}'/>", kernel(3))),
        ("convolve 15", format!("<feConvolveMatrix order='15' kernelMatrix='{}'/>", kernel(15))),
        ("diffuse lighting", "<feDiffuseLighting><feDistantLight azimuth='45' elevation='45'/></feDiffuseLighting>".into()),
        ("specular lighting", "<feSpecularLighting><fePointLight x='10' y='10' z='20'/></feSpecularLighting>".into()),
        ("displacement", "<feTurbulence baseFrequency='0.05' numOctaves='1' result='t'/><feDisplacementMap in='SourceGraphic' in2='t' scale='9'/>".into()),
        ("color matrix", "<feColorMatrix type='saturate' values='0.2'/>".into()),
        ("component transfer", "<feComponentTransfer><feFuncR type='gamma' exponent='2'/></feComponentTransfer>".into()),
        ("composite arithmetic", "<feComposite in2='SourceGraphic' operator='arithmetic' k1='0.5' k2='0.5'/>".into()),
        ("blend", "<feBlend in2='SourceGraphic' mode='multiply'/>".into()),
        ("flood", "<feFlood flood-color='red'/>".into()),
        ("merge 4", "<feMerge><feMergeNode/><feMergeNode/><feMergeNode/><feMergeNode/></feMerge>".into()),
        ("tile", "<feTile/>".into()),
        ("blur chain 20", "<feGaussianBlur stdDeviation='1'/>".repeat(20)),
    ]
}

#[test]
#[ignore = "calibration tool; run explicitly (see the module documentation)"]
fn calibrate() {
    if std::env::var_os("UI4ME_FILTER_CALIBRATE").is_none() {
        return;
    }
    let size = 400;
    println!("| primitive | measured ms | estimated ms | estimate / measured |");
    println!("|---|---|---|---|");
    for (name, p) in cases() {
        let svg = filtered(&p, size);
        let _ = measure(&svg, size); // warm
        let runs = 3;
        let mut best = Duration::MAX;
        let mut est = 0;
        for _ in 0..runs {
            let (t, e) = measure(&svg, size);
            best = best.min(t);
            est = e;
        }
        println!(
            "| {name} | {:.2} | {:.2} | {:.2} |",
            best.as_secs_f64() * 1e3,
            est as f64 / 1e6,
            est as f64 / 1e9 / best.as_secs_f64()
        );
    }
}

/// Every calibrated primitive is estimated at least at what it costs (within 2x on a machine up to
/// twice as slow as the reference), so the budget bounds real time.
#[test]
fn estimates_do_not_undercount_the_expensive_primitives() {
    if cfg!(debug_assertions) {
        return; // the weights are release-build costs
    }
    let size = 300;
    for (name, p) in cases() {
        if name.starts_with("none") || name == "flood" || name == "tile" || name == "merge 4" {
            continue; // cheap enough that timer noise dominates
        }
        let svg = filtered(&p, size);
        let _ = measure(&svg, size);
        let (t, est) = (0..3)
            .map(|_| measure(&svg, size))
            .min_by_key(|(t, _)| *t)
            .unwrap();
        let measured = t.as_nanos() as f64;
        assert!(
            est as f64 >= measured / 4.0,
            "{name}: estimated {est} ns, measured {measured} ns"
        );
    }
}

fn parse(svg: &str) -> ui4me_assets::decode::SvgParsed {
    UsvgRenderer
        .parse(svg.as_bytes(), &SvgOptions::default())
        .unwrap()
}

fn pixels(svg: &str, size: u32) -> (Vec<u8>, bool, Duration) {
    let r = UsvgRenderer;
    let parsed = parse(svg);
    let t0 = Instant::now();
    let (img, skipped) = r.rasterize_bounded(&parsed.tree, size, size).unwrap();
    (img.data, skipped, t0.elapsed())
}

fn bound() -> Duration {
    if cfg!(debug_assertions) {
        Duration::from_secs(10)
    } else {
        Duration::from_millis(250)
    }
}

#[test]
fn pathological_filters_are_skipped_within_the_budget() {
    let kernel = vec!["1"; 50 * 50].join(" ");
    for (name, p) in [
        (
            "turbulence 10000",
            "<feTurbulence baseFrequency='0.05' numOctaves='10000'/>".to_string(),
        ),
        (
            "turbulence 4294967295",
            "<feTurbulence baseFrequency='0.05' numOctaves='4294967295'/>".into(),
        ),
        (
            "morphology 1000",
            "<feMorphology operator='dilate' radius='1000'/>".into(),
        ),
        (
            "convolve 50",
            format!("<feConvolveMatrix order='50' kernelMatrix='{kernel}'/>"),
        ),
        (
            "blur chain 1500",
            "<feGaussianBlur stdDeviation='1'/>".repeat(1500),
        ),
    ] {
        let (px, skipped, t) = pixels(&filtered(&p, 300), 300);
        assert!(skipped, "{name}: filters were run");
        assert!(t < bound(), "{name}: {t:?}");
        // Drawn without the filter: the teal rectangle.
        assert_eq!(&px[..4], &[0, 128, 128, 255], "{name}");
    }
}

#[test]
fn ordinary_filters_run_and_match_resvg() {
    for p in [
        "<feGaussianBlur stdDeviation='2'/>",
        "<feTurbulence baseFrequency='0.05' numOctaves='4'/>",
        "<feMorphology operator='erode' radius='2'/>",
        "<feDropShadow dx='2' dy='2' stdDeviation='3'/>",
    ] {
        let svg = filtered(p, 64);
        let (px, skipped, _) = pixels(&svg, 64);
        assert!(!skipped, "{p}");
        let tree = usvg_tree(&svg);
        let mut reference = resvg::tiny_skia::Pixmap::new(64, 64).unwrap();
        resvg::render(&tree, usvg::Transform::identity(), &mut reference.as_mut());
        assert_eq!(px, reference.data(), "{p}");
    }
}

#[test]
fn the_budget_depends_on_the_raster_size() {
    // One full-size blur: fine as an icon, over the budget as a huge raster.
    let svg = filtered("<feGaussianBlur stdDeviation='0.5'/>", 64);
    let (_, small, _) = pixels(&svg, 64);
    assert!(!small);
    let tree = usvg_tree(&svg);
    let scale = 3000.0 / 64.0;
    let work = filter_work(&tree, usvg::Transform::from_scale(scale, scale), 3000, 3000);
    assert!(work > SVG_FILTER_WORK_BUDGET, "{work}");
}

#[test]
fn every_way_of_applying_a_filter_is_skipped() {
    // Presentation attribute, `style` attribute, author rule (also `!important`), on a group, in a
    // mask and in a pattern: the unfiltered form has none of them.
    let heavy = "<filter id='f'><feTurbulence baseFrequency='0.05' numOctaves='100000'/></filter>";
    let rect = "width='300' height='300' fill='teal'";
    for body in [
        format!("<rect {rect} style='filter:url(#f)'/>"),
        format!("<style>rect{{filter:url(#f) !important}}</style><rect {rect}/>"),
        format!("<g filter='url(#f)'><rect {rect}/></g>"),
        format!("<mask id='m'><rect width='300' height='300' fill='white' filter='url(#f)'/></mask><rect {rect} mask='url(#m)'/>"),
        format!("<pattern id='p' width='300' height='300' patternUnits='userSpaceOnUse'><rect {rect} filter='url(#f)'/></pattern><rect width='300' height='300' fill='url(#p)'/>"),
        format!("<image width='300' height='300' href='data:image/svg+xml,{}'/>",
            format!("<svg xmlns='http://www.w3.org/2000/svg' width='300' height='300'>{heavy}<rect {rect} filter='url(#f)'/></svg>")
                .replace('<', "%3C").replace('>', "%3E").replace('#', "%23").replace('\'', "%27").replace(' ', "%20")),
    ] {
        let svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='300' height='300'>{heavy}{body}</svg>");
        let (_, skipped, t) = pixels(&svg, 300);
        assert!(skipped, "{body}");
        assert!(t < bound(), "{body}: {t:?}");
    }
}
