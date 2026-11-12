//! F0907 / F0908: inline SVG through the production facade (render, CSS hooks, hit testing, DOM
//! interaction, limits, accessibility) and the static-profile corpus.
use super::*;
use accesskit::Role;

const W: usize = 200;
const RED: u32 = 0x00ff_0000;
const GREEN: u32 = 0x0000_ff00;
const BLUE: u32 = 0x0000_00ff;
const WHITE: u32 = 0x00ff_ffff;

fn page(body: &str) -> Document {
    let mut d = Document::parse(&format!(
        "<!doctype html><style>body{{margin:0}} svg{{display:block}}</style>{body}"
    ))
    .unwrap();
    d.render(W as u32, 200, 1.0).unwrap();
    d
}

/// Render until the document's resources settle (an SVG rasterises like any image).
fn shot(d: &mut Document) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let px = d.render(W as u32, 200, 1.0).unwrap();
        d.poll_resources().unwrap();
        if !d.has_pending_resources() {
            return d.render(W as u32, 200, 1.0).unwrap();
        }
        assert!(Instant::now() < deadline, "svg did not settle");
        drop(px);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn at(px: &[u32], x: usize, y: usize) -> u32 {
    px[y * W + x] & 0x00ff_ffff
}

/// Whether `a` is within `tol` of `b` on every channel.
fn near(a: u32, b: u32, tol: i32) -> bool {
    [16, 8, 0].iter().all(|s| {
        let (x, y) = (((a >> s) & 255) as i32, ((b >> s) & 255) as i32);
        (x - y).abs() <= tol
    })
}

fn btn(d: &mut Document, pressed: bool, x: f64, y: f64) -> Vec<Value> {
    d.handle_input(Input::PointerButton {
        pressed,
        button: 0,
        x,
        y,
    })
    .unwrap()
}

fn click_at(d: &mut Document, x: f64, y: f64) -> Option<Value> {
    let mut ev = btn(d, true, x, y);
    ev.extend(btn(d, false, x, y));
    ev.into_iter().find(|e| e["type"] == "click")
}

fn features(d: &Document) -> Vec<String> {
    d.diagnostics()
        .iter()
        .filter_map(|v| v["feature"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn inline_icon_renders_with_inherited_current_color() {
    let mut d = page(
        r##"<style>.icon{color:#00ff00}</style>
        <svg class=icon width=20 height=20 viewBox="0 0 10 10"><rect width=10 height=10 fill="currentColor"/></svg>"##,
    );
    let px = shot(&mut d);
    assert_eq!(at(&px, 5, 5), GREEN);
    assert_eq!(at(&px, 15, 15), GREEN);
    assert_eq!(at(&px, 30, 30), WHITE);
}

#[test]
fn css_rules_set_fill_and_stroke_by_specificity_and_inheritance() {
    let mut d = page(
        r##"<style>
        .icon{fill:#00ff00}
        svg .big{fill:#0000ff}
        #c{fill:#ff0000}
        .none{fill:#ff00ff}
        </style>
        <div style="width:100px;height:20px"><svg class=icon width=30 height=10 viewBox="0 0 30 10">
          <rect id=a width=10 height=10 />
          <rect id=b x=10 width=10 height=10 class=big />
          <rect id=c x=20 width=10 height=10 class="big none" fill="#ffff00"/>
        </svg></div>"##,
    );
    let px = shot(&mut d);
    // fill inherited from the rule on the <svg>, a class rule over it, an id rule over both (and over the
    // presentation attribute).
    assert_eq!(at(&px, 5, 5), GREEN);
    assert_eq!(at(&px, 15, 5), BLUE);
    assert_eq!(at(&px, 25, 5), RED);
}

#[test]
fn fill_inherits_from_html_ancestors_and_inline_style_wins() {
    let mut d = page(
        r##"<style>.wrap{fill:#0000ff}</style>
        <div class=wrap><svg width=20 height=10 viewBox="0 0 20 10">
          <rect width=10 height=10 /><rect x=10 width=10 height=10 style="fill:#ff0000"/></svg></div>"##,
    );
    let px = shot(&mut d);
    assert_eq!(at(&px, 5, 5), BLUE);
    assert_eq!(at(&px, 15, 5), RED);
}

#[test]
fn css_custom_property_feeds_a_fill() {
    let mut d = page(
        r##"<style>svg{--ink:#00ff00} rect{fill:var(--ink)}</style>
        <svg width=10 height=10><rect width=10 height=10 /></svg>"##,
    );
    assert_eq!(at(&shot(&mut d), 5, 5), GREEN);
}

#[test]
fn view_box_scales_and_centres_and_transform_moves() {
    let mut d = page(
        r##"<svg width=40 height=40 viewBox="0 0 10 10">
        <rect width=5 height=5 fill="#ff0000"/>
        <rect width=2 height=2 fill="#0000ff" transform="translate(6 6)"/></svg>
        <svg width=40 height=20 viewBox="0 0 10 10"><rect width=10 height=10 fill="#00ff00"/></svg>"##,
    );
    let px = shot(&mut d);
    // 4x scale: the 5x5 rect is 20x20 px.
    assert_eq!(at(&px, 15, 15), RED);
    assert_eq!(at(&px, 22, 22), WHITE);
    // translate(6 6) scaled: 24..32.
    assert_eq!(at(&px, 28, 28), BLUE);
    // The square viewBox in a 40x20 box is fitted xMidYMid meet: 20x20 centred at x 10..30.
    assert_eq!(at(&px, 20, 50), GREEN);
    assert_eq!(at(&px, 5, 50), WHITE);
    assert_eq!(at(&px, 35, 50), WHITE);
}

#[test]
fn size_follows_css_and_attributes() {
    let mut d = page(
        r##"<style>#a{width:60px;height:30px}</style>
        <svg id=a viewBox="0 0 10 10" preserveAspectRatio="none"><rect width=10 height=10 fill="#ff0000"/></svg>"##,
    );
    let px = shot(&mut d);
    assert_eq!(at(&px, 55, 25), RED);
    assert_eq!(at(&px, 65, 25), WHITE);
    assert_eq!(at(&px, 30, 35), WHITE);
}

const HIT: &str = r##"<svg id=icon width=100 height=50 viewBox="0 0 100 50">
    <circle id=dot cx=25 cy=25 r=20 fill="#ff0000"/>
    <rect id=box x=60 y=10 width=30 height=30 fill="#0000ff"/>
    <g id=grp><path d="M50 0 L55 0 L55 5 Z" fill="#00ff00"/></g></svg>"##;

#[test]
fn click_on_a_shape_reports_its_id_with_svg_ancestors_in_the_path() {
    let mut d = page(HIT);
    let e = click_at(&mut d, 25.0, 25.0).unwrap();
    assert_eq!(e["element"], "dot");
    let path: Vec<_> = e["path"].as_array().unwrap().iter().collect();
    assert_eq!(path, ["dot", "icon"]);
    let e = click_at(&mut d, 75.0, 25.0).unwrap();
    assert_eq!(e["element"], "box");
    // Outside every shape the <svg> element itself is the target.
    let e = click_at(&mut d, 45.0, 45.0).unwrap();
    assert_eq!(e["element"], "icon");
    // The corner of the circle's bounding box is not part of the circle.
    let e = click_at(&mut d, 6.0, 6.0).unwrap();
    assert_eq!(e["element"], "icon");
    // A shape inside an identified group is reported with the group in the path.
    let e = click_at(&mut d, 53.0, 3.0).unwrap();
    assert_eq!(e["element"], "grp");
}

#[test]
fn hit_testing_follows_scaling_and_pointer_events() {
    let mut d = page(&HIT.replace("width=100 height=50", "style=\"width:200px;height:100px\""));
    // Scaled 2x: the box is at 120..180.
    assert_eq!(click_at(&mut d, 150.0, 50.0).unwrap()["element"], "box");
    assert_eq!(click_at(&mut d, 50.0, 50.0).unwrap()["element"], "dot");

    let mut d = page(&HIT.replace(
        "<rect id=box ",
        "<rect id=box style=\"pointer-events:none\" ",
    ));
    assert_eq!(click_at(&mut d, 75.0, 25.0).unwrap()["element"], "icon");
    assert_eq!(click_at(&mut d, 25.0, 25.0).unwrap()["element"], "dot");

    // `pointer-events` is inherited by the shapes of a group, and `fill`/`all` select areas.
    let mut d = page(
        r##"<svg id=icon width=100 height=50>
        <g id=g pointer-events="none"><rect id=r width=50 height=50 fill="red"/></g>
        <rect id=ring x=60 y=10 width=30 height=30 fill="none" stroke="blue" stroke-width=4 pointer-events="all"/></svg>"##,
    );
    assert_eq!(click_at(&mut d, 25.0, 25.0).unwrap()["element"], "icon");
    // `all` makes the unfilled interior react too.
    assert_eq!(click_at(&mut d, 75.0, 25.0).unwrap()["element"], "ring");
}

#[test]
fn pointer_events_none_on_the_svg_passes_the_click_through() {
    let mut d = page(
        r##"<div id=under style="position:absolute;left:0;top:0;width:100px;height:50px"></div>
        <svg id=icon style="position:absolute;left:0;top:0;pointer-events:none" width=100 height=50>
        <rect id=r width=100 height=50 fill="red"/></svg>"##,
    );
    let e = click_at(&mut d, 25.0, 25.0).unwrap();
    assert_ne!(e["element"], "icon");
    assert_ne!(e["element"], "r");
}

#[test]
fn dom_mutation_rerenders() {
    let mut d = page(
        r##"<svg id=icon width=20 height=10 viewBox="0 0 20 10"><rect id=a width=10 height=10 fill="#ff0000"/><title id=t>One</title></svg>"##,
    );
    assert_eq!(at(&shot(&mut d), 5, 5), RED);
    d.set_attribute("a", "fill", Some("#00ff00")).unwrap();
    assert_eq!(at(&shot(&mut d), 5, 5), GREEN);
    d.set_attribute("a", "width", Some("20")).unwrap();
    assert_eq!(at(&shot(&mut d), 15, 5), GREEN);
    // insert_html parses in the SVG context and the new shape paints and is hit.
    d.insert_html(
        "icon",
        "beforeend",
        r##"<circle id=c cx=15 cy=5 r=4 fill="#0000ff"/>"##,
    )
    .unwrap();
    let px = shot(&mut d);
    assert_eq!(at(&px, 15, 5), BLUE);
    assert_eq!(click_at(&mut d, 15.0, 5.0).unwrap()["element"], "c");
    // A style rule added later restyles it.
    d.set_attribute("c", "style", Some("fill:#ff00ff")).unwrap();
    assert_eq!(at(&shot(&mut d), 15, 5), 0x00ff_00ff);
    // Removing the attribute and the element goes back.
    d.set_attribute("a", "fill", None).unwrap();
    assert_eq!(
        at(&shot(&mut d), 2, 2) & 0x00ff_ffff,
        0,
        "default fill is black"
    );
    d.replace_children_html("icon", "").unwrap();
    assert_eq!(at(&shot(&mut d), 5, 5), WHITE);
}

#[test]
fn style_rule_changes_on_ancestors_rerender() {
    let mut d = page(
        r##"<div id=w><svg width=10 height=10><rect width=10 height=10 fill="currentColor"/></svg></div>"##,
    );
    assert_eq!(at(&shot(&mut d), 5, 5), 0);
    d.set_attribute("w", "style", Some("color:#ff0000"))
        .unwrap();
    assert_eq!(at(&shot(&mut d), 5, 5), RED);
}

#[test]
fn oversize_and_malformed_svg_render_nothing_and_report() {
    // Too many elements.
    let mut body = String::from(r##"<svg width=10 height=10 fill="red">"##);
    for _ in 0..2_100 {
        body.push_str("<rect width=10 height=10 />");
    }
    body.push_str("</svg>");
    let mut d = page(&body);
    assert_eq!(at(&shot(&mut d), 5, 5), WHITE);
    assert!(features(&d).iter().any(|f| f == "svg.inline.rejected"));

    // Path data over the attribute cap.
    let long = format!("M0 0 {}", "L1 1 ".repeat(20_000));
    let mut d = page(&format!(
        r##"<svg width=10 height=10><path d="{long}" fill="red"/></svg>"##
    ));
    assert_eq!(at(&shot(&mut d), 5, 5), WHITE);
    assert!(features(&d).iter().any(|f| f == "svg.inline.rejected"));

    // Garbage path data, a zero viewBox, unknown elements: nothing panics, nothing paints.
    let mut d = page(
        r##"<svg width=10 height=10 viewBox="0 0 0 0"><path d="M 10 oops" fill="red"/></svg>
        <svg width=10 height=10><path d="M 0 0 L" fill="red"/><bogus><rect width=10 height=10 fill="red"/></bogus></svg>"##,
    );
    let px = shot(&mut d);
    assert_eq!(at(&px, 5, 5), WHITE);
    assert_eq!(at(&px, 5, 15), WHITE);
}

#[test]
fn deep_nesting_is_rejected() {
    let body = format!(
        "<svg width=10 height=10>{}<rect width=10 height=10 fill=red />{}</svg>",
        "<g>".repeat(60),
        "</g>".repeat(60)
    );
    let mut d = page(&body);
    assert_eq!(at(&shot(&mut d), 5, 5), WHITE);
    assert!(features(&d).iter().any(|f| f == "svg.inline.rejected"));
}

#[test]
fn unsupported_content_is_dropped_and_listed() {
    let mut d = page(
        r##"<svg width=20 height=10 viewBox="0 0 20 10">
        <foreignObject width=10 height=10><div>hi</div></foreignObject>
        <rect width=10 height=10 fill="#ff0000"><animate attributeName="fill" to="blue" dur="1s"/></rect>
        <use href="http://example.invalid/x.svg#a" x=10 />
        <image href="http://example.invalid/a.png" x=10 width=10 height=10 />
        <text x=1 y=8>Hi</text></svg>"##,
    );
    let px = shot(&mut d);
    assert_eq!(at(&px, 5, 5), RED);
    assert_eq!(at(&px, 15, 5), WHITE);
    let f = features(&d);
    for want in [
        "svg.foreignobject",
        "svg.animation",
        "svg.external-reference",
        "svg.text",
    ] {
        assert!(f.iter().any(|x| x == want), "{want} in {f:?}");
    }
}

#[test]
fn scripts_and_event_attributes_are_still_refused() {
    assert!(Document::parse("<svg><script>alert(1)</script></svg>").is_err());
    assert!(Document::parse("<svg onload='x()'></svg>").is_err());
}

fn node<'a>(t: &'a crate::a11y::A11yTree, id: &str) -> Option<&'a crate::a11y::A11yNode> {
    t.nodes.values().find(|n| n.html_id.as_deref() == Some(id))
}

#[test]
fn accessible_names_for_inline_svg() {
    let mut d = page(
        r##"<svg id=a role="img" aria-label="Chart of sales" width=10 height=10><rect width=5 height=5 /></svg>
        <svg id=b width=10 height=10><title>Close dialog</title><path d="M0 0L10 10"/></svg>
        <svg id=c width=10 height=10 aria-hidden="true"><rect width=5 height=5 /></svg>
        <svg id=e width=10 height=10><rect width=5 height=5 /></svg>"##,
    );
    let t = d.accessibility_tree().unwrap();
    let a = node(&t, "a").unwrap();
    assert_eq!((a.role, a.name.as_str()), (Role::Image, "Chart of sales"));
    let b = node(&t, "b").unwrap();
    assert_eq!((b.role, b.name.as_str()), (Role::Image, "Close dialog"));
    assert!(node(&t, "c").is_none());
    assert!(
        node(&t, "e").is_none(),
        "unlabelled decoration is not exposed"
    );
    d.set_text("a", "").ok();
}

// ---------------------------------------------------------------------------------------------
// F0908: the static presentation corpus (docs/reference/SVG_PROFILE.md). Each case
// is a self-contained `<svg>` at 100 x 100 CSS px (viewBox 0 0 100 100); the assertions sample key
// pixels, never full goldens.
// ---------------------------------------------------------------------------------------------

struct Case {
    name: &'static str,
    body: &'static str,
    /// (x, y, expected colour, tolerance per channel)
    samples: &'static [(usize, usize, u32, i32)],
}

const CORPUS: &[Case] = &[
    Case {
        name: "linear gradient",
        body: r##"<defs><linearGradient id="g" x1="0" x2="1" y1="0" y2="0">
            <stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs>
            <rect width="100" height="100" fill="url(#g)"/>"##,
        samples: &[
            (2, 50, 0xfd0002, 8),
            (50, 50, 0x80007f, 10),
            (97, 50, 0x0200fd, 8),
        ],
    },
    Case {
        name: "radial gradient",
        body: r##"<defs><radialGradient id="g"><stop offset="0" stop-color="#00ff00"/>
            <stop offset="1" stop-color="#000000"/></radialGradient></defs>
            <rect width="100" height="100" fill="url(#g)"/>"##,
        samples: &[(50, 50, 0x00ff00, 12), (3, 50, 0x050f05, 70)],
    },
    Case {
        name: "gradient spread and userSpaceOnUse",
        body: r##"<defs><linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" x2="50" spreadMethod="pad">
            <stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#00ff00"/></linearGradient></defs>
            <rect width="100" height="100" fill="url(#g)"/>"##,
        samples: &[(90, 50, 0x00ff00, 4)],
    },
    Case {
        name: "clipPath",
        body: r##"<defs><clipPath id="c"><circle cx="50" cy="50" r="30"/></clipPath></defs>
            <rect width="100" height="100" fill="#ff0000" clip-path="url(#c)"/>"##,
        samples: &[
            (50, 50, 0xff0000, 0),
            (5, 5, 0xffffff, 0),
            (50, 90, 0xffffff, 0),
        ],
    },
    Case {
        name: "mask",
        body: r##"<defs><mask id="m"><rect width="100" height="100" fill="black"/>
            <rect x="50" width="50" height="100" fill="white"/></mask></defs>
            <rect width="100" height="100" fill="#0000ff" mask="url(#m)"/>"##,
        samples: &[(75, 50, 0x0000ff, 2), (25, 50, 0xffffff, 2)],
    },
    Case {
        name: "marker",
        body: r##"<defs><marker id="m" markerWidth="20" markerHeight="20" refX="10" refY="10" markerUnits="userSpaceOnUse">
            <rect width="20" height="20" fill="#00ff00"/></marker></defs>
            <path d="M20 50 L80 50" stroke="#000000" stroke-width="2" marker-start="url(#m)" marker-end="url(#m)"/>"##,
        samples: &[
            (20, 50, 0x00ff00, 4),
            (80, 50, 0x00ff00, 4),
            (50, 70, 0xffffff, 0),
        ],
    },
    Case {
        name: "use and defs",
        body: r##"<defs><rect id="r" width="20" height="20" fill="#ff00ff"/></defs>
            <use href="#r" x="10" y="10"/><use href="#r" x="60" y="60"/>"##,
        samples: &[
            (20, 20, 0xff00ff, 0),
            (70, 70, 0xff00ff, 0),
            (50, 50, 0xffffff, 0),
        ],
    },
    Case {
        name: "symbol and nested svg viewBox",
        body: r##"<defs><symbol id="s" viewBox="0 0 10 10"><rect width="10" height="10" fill="#00ffff"/></symbol></defs>
            <use href="#s" x="0" y="0" width="40" height="40"/>"##,
        samples: &[(35, 35, 0x00ffff, 0), (60, 60, 0xffffff, 0)],
    },
    Case {
        name: "transforms",
        body: r##"<g transform="translate(50 50) rotate(90) scale(2)"><rect x="0" y="0" width="10" height="20" fill="#ff0000"/></g>"##,
        // rotate(90) maps (x, y) -> (-y, x): the rect covers x -40..0, y 0..20 after the scale, + (50, 50).
        samples: &[(30, 60, 0xff0000, 0), (70, 60, 0xffffff, 0)],
    },
    Case {
        name: "stroke styles",
        body: r##"<path d="M10 50 H90" stroke="#0000ff" stroke-width="20" stroke-linecap="butt"/>
            <path d="M10 20 H90" stroke="#ff0000" stroke-width="10" stroke-dasharray="20 20" />"##,
        samples: &[
            (50, 50, 0x0000ff, 0),
            (5, 50, 0xffffff, 0),
            (20, 20, 0xff0000, 0),
            (40, 20, 0xffffff, 0),
        ],
    },
    Case {
        name: "fill rule",
        body: r##"<path d="M10 10 H90 V90 H10 Z M30 30 H70 V70 H30 Z" fill="#ff0000" fill-rule="evenodd"/>"##,
        samples: &[(20, 20, 0xff0000, 0), (50, 50, 0xffffff, 0)],
    },
    Case {
        name: "opacity",
        body: r##"<rect width="100" height="100" fill="#000000" opacity="0.5"/>"##,
        samples: &[(50, 50, 0x808080, 3)],
    },
    Case {
        name: "basic shapes and polylines",
        body: r##"<ellipse cx="25" cy="25" rx="20" ry="10" fill="#ff0000"/>
            <polygon points="60,60 90,60 75,90" fill="#00ff00"/>
            <line x1="10" y1="80" x2="40" y2="80" stroke="#0000ff" stroke-width="6"/>
            <rect x="60" y="10" width="30" height="20" rx="10" fill="#ff00ff"/>"##,
        samples: &[
            (25, 25, 0xff0000, 0),
            (75, 70, 0x00ff00, 0),
            (25, 80, 0x0000ff, 0),
            (75, 20, 0xff00ff, 0),
            (60, 10, 0xffffff, 0),
        ],
    },
    Case {
        name: "filter drop blur and offset",
        body: r##"<defs><filter id="f" x="-50%" y="-50%" width="400%" height="200%"><feOffset dx="30" dy="0"/></filter></defs>
            <rect x="10" y="40" width="20" height="20" fill="#ff0000" filter="url(#f)"/>"##,
        samples: &[(20, 50, 0xffffff, 0), (50, 50, 0xff0000, 0)],
    },
    Case {
        name: "filter gaussian blur and color matrix",
        body: r##"<defs><filter id="f"><feColorMatrix type="matrix" values="0 0 1 0 0  0 1 0 0 0  1 0 0 0 0  0 0 0 1 0"/></filter></defs>
            <rect x="10" y="10" width="80" height="80" fill="#ff0000" filter="url(#f)"/>"##,
        samples: &[(50, 50, 0x0000ff, 2)],
    },
    Case {
        name: "pattern",
        body: r##"<defs><pattern id="p" width="20" height="20" patternUnits="userSpaceOnUse">
            <rect width="10" height="20" fill="#ff0000"/></pattern></defs>
            <rect width="100" height="100" fill="url(#p)"/>"##,
        samples: &[
            (5, 50, 0xff0000, 0),
            (15, 50, 0xffffff, 0),
            (45, 50, 0xff0000, 0),
            (55, 50, 0xffffff, 0),
        ],
    },
    Case {
        name: "currentColor and CSS in style attribute",
        body: r##"<g style="color:#00ff00"><rect width="100" height="100" style="fill:currentColor;stroke:none"/></g>"##,
        samples: &[(50, 50, 0x00ff00, 0)],
    },
    Case {
        name: "display none and visibility hidden",
        body: r##"<rect width="50" height="100" fill="#ff0000" display="none"/>
            <rect x="50" width="50" height="100" fill="#0000ff" visibility="hidden"/>
            <rect y="0" x="0" width="10" height="10" fill="#00ff00"/>"##,
        samples: &[
            (25, 50, 0xffffff, 0),
            (75, 50, 0xffffff, 0),
            (5, 5, 0x00ff00, 0),
        ],
    },
    Case {
        name: "preserveAspectRatio slice",
        body: "",
        samples: &[],
    },
];

fn corpus_page(body: &str) -> Document {
    page(&format!(
        r##"<svg width="100" height="100" viewBox="0 0 100 100">{body}</svg>"##
    ))
}

#[test]
fn static_profile_corpus() {
    let mut failures = Vec::new();
    for case in CORPUS.iter().filter(|c| !c.body.is_empty()) {
        let mut d = corpus_page(case.body);
        let px = shot(&mut d);
        for &(x, y, want, tol) in case.samples {
            let got = at(&px, x, y);
            if !near(got, want, tol) {
                failures.push(format!(
                    "{}: ({x},{y}) = {got:06x}, want {want:06x} +-{tol}",
                    case.name
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn preserve_aspect_ratio_and_view_box_origin() {
    // meet (default) letterboxes, slice fills and crops, a viewBox origin shifts the content.
    let mut d = page(
        r##"<svg width=100 height=50 viewBox="0 0 10 10"><rect width=10 height=10 fill="#ff0000"/></svg>
        <svg width=100 height=50 viewBox="0 0 10 10" preserveAspectRatio="xMidYMid slice"><rect width=10 height=10 fill="#00ff00"/></svg>
        <svg width=100 height=50 viewBox="5 0 10 10" preserveAspectRatio="xMinYMin meet"><rect x=5 width=5 height=10 fill="#0000ff"/></svg>"##,
    );
    let px = shot(&mut d);
    assert_eq!(at(&px, 50, 25), RED);
    assert_eq!(at(&px, 10, 25), WHITE);
    assert_eq!(at(&px, 10, 75), GREEN);
    assert_eq!(at(&px, 90, 75), GREEN);
    assert_eq!(at(&px, 10, 125), BLUE);
    assert_eq!(at(&px, 60, 125), WHITE);
}

#[test]
fn text_is_unsupported_and_does_not_paint() {
    let mut d = corpus_page(r##"<text x="10" y="60" font-size="40" fill="#ff0000">Hi</text>"##);
    let px = shot(&mut d);
    assert!(px.iter().all(|&p| p & 0x00ff_ffff == WHITE));
    assert!(features(&d).iter().any(|f| f == "svg.text"));
}

#[test]
fn many_icons_reuse_one_rasterisation() {
    let one = r##"<svg width=16 height=16 viewBox="0 0 10 10"><path d="M0 0H10V10Z" fill="currentColor"/></svg>"##;
    let mut d = page(&one.repeat(40));
    let first = shot(&mut d);
    let second = shot(&mut d);
    assert_eq!(first, second);
    assert!(!d.has_pending_resources());
}
