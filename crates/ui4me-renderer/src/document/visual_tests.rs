//! F0906 / F0909: colour, gradients, shadows, filters, clipping, masks and border images through
//! the production facade (pixels, hit testing, bounded extreme values, profile reasons).
use super::*;

const W: usize = 200;
const RED: u32 = 0xff0000;
const BLUE: u32 = 0x0000ff;
const WHITE: u32 = 0xffffff;

fn page(body: &str) -> (Document, Vec<u32>) {
    let mut d = Document::parse(&format!(
        "<!doctype html><style>html,body{{margin:0;background:#fff}} .b{{position:absolute;left:0;top:0;width:100px;height:100px}}</style>{body}"
    ))
    .unwrap();
    let px = settle(&mut d);
    (d, px)
}

fn settle(d: &mut Document) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        d.render(W as u32, 200, 1.0).unwrap();
        d.poll_resources().unwrap();
        if !d.has_pending_resources() {
            return d.render(W as u32, 200, 1.0).unwrap();
        }
        assert!(Instant::now() < deadline, "resources did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn at(px: &[u32], x: usize, y: usize) -> u32 {
    px[y * W + x] & 0x00ff_ffff
}

fn near(a: u32, b: u32, tol: i32) -> bool {
    [16, 8, 0].iter().all(|s| {
        let (x, y) = (((a >> s) & 255) as i32, ((b >> s) & 255) as i32);
        (x - y).abs() <= tol
    })
}

/// One 100x100 box at the origin carrying `css`.
fn boxed(css: &str) -> (Document, Vec<u32>) {
    page(&format!(
        "<div id=b class=b style=\"box-sizing:border-box;{css}\"></div>"
    ))
}

fn click(d: &mut Document, x: f64, y: f64) -> Option<String> {
    for pressed in [true, false] {
        let ev = d
            .handle_input(Input::PointerButton {
                pressed,
                button: 0,
                x,
                y,
            })
            .unwrap();
        if let Some(e) = ev.into_iter().find(|e| e["type"] == "click") {
            return e["element"].as_str().map(str::to_owned);
        }
    }
    None
}

fn features(d: &Document) -> Vec<String> {
    d.diagnostics()
        .iter()
        .filter_map(|v| v["feature"].as_str().map(str::to_owned))
        .collect()
}

// ---------------------------------------------------------------- gradients

#[test]
fn conic_gradient_hard_stops_fill_the_quadrants() {
    let (_, px) = boxed("background:conic-gradient(red 0 25%, green 0 50%, blue 0 75%, yellow 0)");
    assert_eq!(at(&px, 75, 25), RED);
    assert_eq!(at(&px, 75, 75), 0x008000);
    assert_eq!(at(&px, 25, 75), BLUE);
    assert_eq!(at(&px, 25, 25), 0xffff00);
}

#[test]
fn linear_gradient_stops_and_hints() {
    let (_, px) = boxed("background:linear-gradient(90deg, #000 0, #fff 100px)");
    assert!(
        near(at(&px, 50, 50), 0x808080, 6),
        "{:06x}",
        at(&px, 50, 50)
    );
    assert!(near(at(&px, 1, 50), 0x000000, 6));
    // A colour hint moves the midpoint colour: purple at the hint (20%), not at 50%.
    let (_, px) = boxed("background:linear-gradient(90deg, red, 20%, blue)");
    assert!(
        near(at(&px, 20, 50), 0x800080, 14),
        "{:06x}",
        at(&px, 20, 50)
    );
    assert!(near(at(&px, 0, 50), RED, 6));
    assert!(near(at(&px, 99, 50), BLUE, 6));
}

#[test]
fn radial_gradient_extends_its_last_stop() {
    let (_, px) = boxed("background:radial-gradient(circle closest-side, red, blue)");
    assert!(near(at(&px, 50, 50), RED, 8));
    assert!(
        near(at(&px, 3, 3), BLUE, 2),
        "beyond the radius the last stop extends"
    );
}

// ------------------------------------------------------------------- colour

#[test]
fn colour_functions_resolve() {
    let (_, px) = boxed("background:color-mix(in oklab, red 50%, blue)");
    assert!(
        near(at(&px, 50, 50), 0x8c53a2, 4),
        "{:06x}",
        at(&px, 50, 50)
    );
    let (_, px) = boxed("background:rgb(from #336699 r g 0)");
    assert_eq!(at(&px, 50, 50), 0x336600);
    let (_, px) = boxed("background:color-mix(in srgb, red 25%, blue 25%, lime 50%)");
    assert!(
        near(at(&px, 50, 50), 0x408040, 4),
        "{:06x}",
        at(&px, 50, 50)
    );
    let (_, px) = boxed("background:color(display-p3 1 0 0)");
    assert_eq!(at(&px, 50, 50), RED, "out-of-gamut colours clamp to sRGB");
}

#[test]
fn light_dark_selects_by_colour_scheme_for_colours_and_images() {
    let (_, px) = boxed("color-scheme:dark;background:light-dark(red,blue)");
    assert_eq!(at(&px, 50, 50), BLUE);
    let (_, px) = boxed("color-scheme:light;background:light-dark(red,blue)");
    assert_eq!(at(&px, 50, 50), RED);
    let img = "background-image:light-dark(linear-gradient(red,red), linear-gradient(blue,blue))";
    let (d, px) = boxed(&format!("color-scheme:dark;{img}"));
    assert_eq!(at(&px, 50, 50), BLUE);
    assert!(features(&d).is_empty(), "{:?}", features(&d));
    let (_, px) = boxed(&format!("color-scheme:light;{img}"));
    assert_eq!(at(&px, 50, 50), RED);
}

// ------------------------------------------------------------------ shadows

#[test]
fn inset_and_outer_shadows_with_spread_and_radius() {
    let (_, px) = boxed("background:#fff;box-shadow:inset 0 0 0 10px red");
    assert_eq!(at(&px, 5, 50), RED);
    assert_eq!(at(&px, 50, 50), WHITE);
    let (_, px) = page(
        "<div class=b style=\"left:50px;top:50px;width:50px;height:50px;background:#fff;box-shadow:0 0 0 10px blue\"></div>",
    );
    assert_eq!(at(&px, 45, 75), BLUE);
    assert_eq!(at(&px, 35, 75), WHITE);
    // Spread with a radius rounds the corner of the shadow.
    let (_, px) = page(
        "<div class=b style=\"left:50px;top:50px;width:50px;height:50px;border-radius:25px;box-shadow:0 0 0 10px blue\"></div>",
    );
    assert_eq!(
        at(&px, 41, 41),
        WHITE,
        "the rounded corner of the spread is cut"
    );
    assert_eq!(at(&px, 75, 45), BLUE);
    // A blurred shadow fades outwards.
    let (_, px) = page(
        "<div class=b style=\"left:60px;top:60px;width:80px;height:80px;background:#fff;box-shadow:0 0 20px #000\"></div>",
    );
    let edge = at(&px, 55, 100) & 255;
    let far = at(&px, 40, 100) & 255;
    assert!(
        edge < 250 && edge > 0 && far > edge,
        "edge {edge} far {far}"
    );
}

// ----------------------------------------------------------- filter / blur

#[test]
fn blur_filter_spreads_the_edge() {
    let (_, px) =
        page("<div class=b style=\"left:50px;top:50px;background:red;filter:blur(6px)\"></div>");
    assert_eq!(at(&px, 100, 100), RED);
    let outside = at(&px, 46, 100);
    assert_ne!(outside, WHITE, "blur bleeds past the box");
    assert_ne!(outside, RED);
    assert_eq!(at(&px, 20, 100), WHITE);
}

#[test]
fn backdrop_filter_blurs_what_is_behind() {
    let body = "<div class=b style=\"background:red\"></div>\
        <div class=b style=\"left:100px;background:blue\"></div>\
        <div class=b style=\"left:50px;COVER\"></div>";
    let (_, plain) = page(&body.replace("COVER", ""));
    assert_eq!(at(&plain, 99, 50), RED);
    assert_eq!(at(&plain, 100, 50), BLUE);
    let (d, blurred) = page(&body.replace("COVER", "backdrop-filter:blur(8px)"));
    let seam = at(&blurred, 100, 50);
    assert!(
        seam != RED && seam != BLUE,
        "the seam is blurred: {seam:06x}"
    );
    assert_eq!(
        at(&blurred, 10, 50),
        RED,
        "outside the element nothing changes"
    );
    assert!(features(&d).is_empty(), "{:?}", features(&d));
}

// ----------------------------------------------------- clip-path and hits

#[test]
fn clip_path_shapes_clip_pixels_and_hits() {
    let (mut d, px) = boxed("background:red;clip-path:circle(50% at 50% 50%)");
    assert_eq!(at(&px, 50, 50), RED);
    assert_eq!(at(&px, 3, 3), WHITE);
    assert_eq!(click(&mut d, 50.0, 50.0).as_deref(), Some("b"));
    assert_ne!(click(&mut d, 3.0, 3.0).as_deref(), Some("b"));

    let (mut d, px) = boxed("background:red;clip-path:polygon(0 0, 100% 0, 0 100%)");
    assert_eq!(at(&px, 20, 20), RED);
    assert_eq!(at(&px, 80, 80), WHITE);
    assert_eq!(click(&mut d, 20.0, 20.0).as_deref(), Some("b"));
    assert_ne!(click(&mut d, 80.0, 80.0).as_deref(), Some("b"));

    let (mut d, px) = boxed("background:red;clip-path:inset(10px round 30px)");
    assert_eq!(at(&px, 50, 50), RED);
    assert_eq!(at(&px, 5, 50), WHITE);
    assert_ne!(click(&mut d, 5.0, 50.0).as_deref(), Some("b"));
    assert_eq!(click(&mut d, 50.0, 50.0).as_deref(), Some("b"));
}

#[test]
fn border_radius_hits_follow_the_curve() {
    let (mut d, px) = boxed("background:red;border-radius:50px");
    assert_eq!(at(&px, 3, 3), WHITE);
    assert_ne!(click(&mut d, 3.0, 3.0).as_deref(), Some("b"));
    assert_eq!(click(&mut d, 50.0, 50.0).as_deref(), Some("b"));
}

// --------------------------------------------------------- mask, border-image

#[test]
fn mask_image_gradient_fades_the_box() {
    let (_, px) = boxed("background:red;mask-image:linear-gradient(90deg,#000,transparent)");
    assert!(near(at(&px, 2, 50), RED, 20), "{:06x}", at(&px, 2, 50));
    let mid = at(&px, 50, 50);
    assert!(mid != RED && mid != WHITE, "{mid:06x}");
    assert!(near(at(&px, 97, 50), WHITE, 20), "{:06x}", at(&px, 97, 50));
}

#[test]
fn border_image_slices_and_repeat_modes() {
    let src = "linear-gradient(90deg, red 0 50%, blue 0)";
    let (_, px) = boxed(&format!("border:20px solid;border-image:{src} 10 / 20px"));
    assert_eq!(at(&px, 3, 3), RED);
    assert_eq!(at(&px, 96, 3), BLUE);
    assert_eq!(at(&px, 5, 50), RED);
    assert_eq!(at(&px, 95, 50), BLUE);
    assert_eq!(at(&px, 50, 50), WHITE, "no fill without the fill keyword");
    let (_, px) = boxed(&format!(
        "border:20px solid;border-image:{src} 10 fill / 20px"
    ));
    assert_ne!(at(&px, 50, 50), WHITE, "fill paints the middle slice");
    for mode in ["repeat", "round", "space", "stretch"] {
        let (_, px) = boxed(&format!(
            "border:20px solid;border-image:{src} 10 / 20px {mode}"
        ));
        assert_eq!(at(&px, 3, 3), RED, "{mode}");
        assert_eq!(at(&px, 96, 96), BLUE, "{mode}");
    }
}

// ------------------------------------------------------------------ text

#[test]
fn text_overflow_ellipsis_paints_and_clips() {
    let t = |mode: &str| {
        let (_, px) = page(&format!(
            "<div style=\"position:absolute;left:0;top:0;width:60px;overflow:hidden;white-space:nowrap;font:20px sans-serif;color:#000;text-overflow:{mode}\">WWWWWWWWWWWWWWWW</div>"
        ));
        px
    };
    let (ell, clip) = (t("ellipsis"), t("clip"));
    for y in 0..30 {
        for x in 60..120 {
            assert_eq!(
                at(&ell, x, y),
                WHITE,
                "ellipsis stays inside the box ({x},{y})"
            );
            assert_eq!(at(&clip, x, y), WHITE);
        }
    }
    let differs = (0..30).any(|y| (30..60).any(|x| at(&ell, x, y) != at(&clip, x, y)));
    assert!(differs, "the ellipsis replaces the clipped glyph");
}

// ----------------------------------------------------------------- bounds

#[test]
fn extreme_and_malformed_values_are_bounded() {
    let cases = [
        "background:red;filter:blur(1000000px)",
        "background:red;backdrop-filter:blur(1000000px)",
        "background:red;box-shadow:0 0 1000000px 1000000px red",
        "background:red;box-shadow:inset 0 0 1000000px 1000000px red",
        "background:linear-gradient(red 0, blue 1000000px);background-size:1000000px 1000000px",
        "background:repeating-linear-gradient(red 0, blue 0.001px)",
        "background:repeating-radial-gradient(circle at 0 0, red 0, blue 0.01px)",
        "background:conic-gradient(red, blue);background-size:0.01px 0.01px",
        "border:20px solid;border-image:url(data:image/png;base64,AAAA) 30% round",
        "border:20px solid;border-image:linear-gradient(red,blue) 100000 / 1000000px repeat",
        "mask-image:url(nothing-here.png);background:red",
        "mask-image:url(data:image/svg+xml;utf8,<svg);background:red",
        "mask:url(#missing) no-repeat 1000000px 1000000px / 0 0;background:red",
        "background:red;border-radius:1000000px",
        "background:red;clip-path:polygon(0 0,100% 0,100% 1000000px,0 100%)",
        "background:red;clip-path:path('M0 0 L1e30 1e30 Z')",
        "background:red;clip-path:circle(1e30px)",
        "background:red;filter:url(#nope) blur(5px)",
        "background:red;clip-path:url(#nope)",
        "background:red;opacity:1e9;transform:scale(1e9)",
    ];
    let start = Instant::now();
    for css in cases {
        let (d, px) = boxed(css);
        assert_eq!(px.len(), W * 200, "{css}");
        drop(d);
    }
    let polygon: String = (0..5000)
        .map(|i| format!(",{}px {}px", i % 100, (i * 7) % 100))
        .collect();
    let (_, px) = boxed(&format!("background:red;clip-path:polygon(0 0{polygon})"));
    assert_eq!(px.len(), W * 200);
    assert!(
        start.elapsed() < Duration::from_secs(60),
        "{:?}",
        start.elapsed()
    );
}

// ---------------------------------------------------------------- profile

#[test]
fn visual_profile_rows_and_reason_codes() {
    use crate::style::profile::table::{FeatureState as S, feature_state};
    let st = |id: &str| feature_state(id).unwrap();
    // 1.26.0 (before -> after): backdrop-filter Filtered -> On, light-dark-image Off -> On.
    assert_eq!(st("css.property.backdrop-filter"), S::On);
    assert_eq!(st("css.function.light-dark-image"), S::On);
    assert_eq!(st("css.function.light-dark"), S::On);
    assert_eq!(st("css.function.color-mix"), S::On);
    assert_eq!(st("css.property.mask"), S::On);
    // Still reported, with their reasons.
    assert_eq!(st("css.value.clip-path-url"), S::Filtered);
    assert_eq!(st("css.value.filter-url"), S::Filtered);
    assert_eq!(st("css.value.shape-function"), S::Off);
    assert_eq!(st("css.function.cross-fade"), S::Off);
    assert_eq!(st("css.property.corner-shape"), S::Off);
    let reported = |css: &str| {
        let (d, _) = boxed(css);
        features(&d)
    };
    assert!(reported("filter:url(#c)").contains(&"css.value.filter-url".into()));
    assert!(
        reported("clip-path:shape(from 0 0, line to 100% 0)")
            .contains(&"css.value.shape-function".into())
    );
    assert!(
        reported(
            "background-image:cross-fade(linear-gradient(red,red), linear-gradient(blue,blue), 50%)"
        )
        .contains(&"css.function.cross-fade".into())
    );
    assert!(reported("corner-shape:bevel").contains(&"css.property.corner-shape".into()));
}

/// The closed drop-down chevron sits in the right padding of a `select`, which lies outside the
/// control's content-box clip. With author padding wider than the chevron's inset it must still
/// paint whole (the local reference application uses 11px padding).
#[test]
fn select_chevron_paints_outside_the_content_clip() {
    let dark = |p: u32| [16, 8, 0].iter().all(|s| ((p >> s) & 255) < 200);
    for padding in [2, 11, 16] {
        let (_d, px) = page(&format!(
            "<select style=\"position:absolute;left:0;top:0;box-sizing:border-box;width:160px;height:40px;\
             padding:6px {padding}px;border:1px solid #888;background:#fff;color:#000\">\
             <option>Normal</option></select>"
        ));
        // The chevron spans about 8px around x = 150 (border-box right minus 10) and y = 20.
        let inked = (140..=158)
            .flat_map(|x| (14..=26).map(move |y| (x, y)))
            .filter(|&(x, y)| dark(at(&px, x, y)))
            .count();
        assert!(
            inked >= 12,
            "padding {padding}px: only {inked} chevron pixels painted"
        );
    }
}

/// Core 1.0 (found natively on a dark KDE desktop): a page that does not declare `color-scheme` is
/// not aware of colour schemes and keeps the light user-agent colours, as the reference browser
/// does, while `prefers-color-scheme` reports the desktop preference. Previously the observed dark
/// preference switched its default controls to light text, invisible on the page's own light
/// backgrounds. `light dark` (here via `<meta name=color-scheme>`) opts into the dark scheme.
#[test]
fn unaware_page_keeps_light_controls_on_a_dark_desktop_and_opt_in_follows() {
    let page = |meta: &str| {
        format!(
            "<!doctype html><head>{meta}<style>@media (prefers-color-scheme: dark){{#m{{color:rgb(1,2,3)}}}}</style></head>\
             <body style='margin:0'><input id=i value=text><p id=m>m</p></body>"
        )
    };
    let style = |d: &mut Document, id: &str, p: &str| {
        d.computed_style(Some(id), None, &[p.into()]).unwrap()["properties"][p]["value"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let mut d = Document::parse(&page("")).unwrap();
    d.set_system_color_scheme(Some(true)).unwrap();
    let px = d.render(200, 100, 1.0).unwrap();
    assert_eq!(
        style(&mut d, "m", "color"),
        "rgb(1, 2, 3)",
        "media feature follows the desktop"
    );
    assert_eq!(
        style(&mut d, "i", "color"),
        "rgb(0, 0, 0)",
        "unaware field text stays dark"
    );
    assert_eq!(
        px[199 + 90 * 200] & 0xffffff,
        0xffffff,
        "unaware canvas stays white"
    );

    let mut d = Document::parse(&page("<meta name=color-scheme content='light dark'>")).unwrap();
    d.set_system_color_scheme(Some(true)).unwrap();
    let px = d.render(200, 100, 1.0).unwrap();
    assert_eq!(
        style(&mut d, "i", "color"),
        "rgb(255, 255, 255)",
        "opted-in field text is light"
    );
    assert_ne!(
        px[199 + 90 * 200] & 0xffffff,
        0xffffff,
        "opted-in canvas is dark"
    );
}
