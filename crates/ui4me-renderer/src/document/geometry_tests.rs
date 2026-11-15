//! T5: the content→window geometry service (F-07 / INTERACT-02, INTERACT-03) agrees with painting
//! and hit testing: in fixed boxes (a modal `<dialog>` included), sticky boxes, nested scrollers,
//! translated and scaled boxes and a centred modal, before and after scrolling, the reported
//! rectangle of an element is where it is hit, `<select>` rows pick their option, a click on a
//! glyph puts the caret there, and the IME area and accessibility text runs lie inside the control.
use super::*;
use crate::a11y::{A11yTree, Bounds};
use proptest::prelude::*;

fn doc(html: &str, w: u32, h: u32) -> Document {
    let mut d = Document::parse(html).unwrap();
    d.use_manual_clock();
    d.render(w, h, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    d
}
fn rect(d: &mut Document, id: &str) -> Rect {
    d.element_rect(id).unwrap().unwrap()
}
fn click(d: &mut Document, x: f64, y: f64) {
    for pressed in [true, false] {
        d.handle_input(Input::PointerButton {
            pressed,
            button: 0,
            x,
            y,
        })
        .unwrap();
    }
}
fn hit_id(d: &mut Document, x: f64, y: f64) -> Option<NodeId> {
    let id = d.id();
    DispatchHost::hit_test(d, id, x, y).unwrap()
}
/// `hit` is `n` or one of its descendants.
fn within(d: &Document, n: NodeId, hit: Option<NodeId>) -> bool {
    hit.is_some_and(|h| d.pipeline.document().arena().is_inclusive_ancestor(n, h))
}
/// The hit at `(x, y)` is `n` or one of its descendants.
fn hits(d: &mut Document, n: NodeId, x: f64, y: f64) -> bool {
    let hit = hit_id(d, x, y);
    within(d, n, hit)
}
fn caret(d: &mut Document, id: &str) -> (u64, u64) {
    let s = d.control_selection(Some(id), None).unwrap();
    (s["start"].as_u64().unwrap(), s["end"].as_u64().unwrap())
}
fn contains(outer: &Rect, x: f64, y: f64, w: f64, h: f64) -> bool {
    const E: f64 = 1.0;
    x >= outer.x - E
        && y >= outer.y - E
        && x + w <= outer.x + outer.width + E
        && y + h <= outer.y + outer.height + E
}
/// The bounds of the text runs of the field with `id`.
fn run_bounds(t: &A11yTree, id: &str) -> Vec<Bounds> {
    let nodes = t.in_order();
    let Some(field) = nodes.iter().find(|n| n.html_id.as_deref() == Some(id)) else {
        return Vec::new();
    };
    nodes
        .iter()
        .filter(|n| field.children.contains(&n.id) && n.role == accesskit::Role::TextRun)
        .filter_map(|n| n.bounds)
        .collect()
}
fn element_bounds(t: &A11yTree, id: &str) -> Option<Bounds> {
    t.in_order()
        .into_iter()
        .find(|n| n.html_id.as_deref() == Some(id))
        .and_then(|n| n.bounds)
}
/// The IME area of the focused field and every accessibility text run of it lie inside its rect;
/// the accessibility bounds of the field are its rect.
fn assert_text_geometry_inside(d: &mut Document, id: &str) {
    let r = rect(d, id);
    let ime = d.ime_request().cloned().expect("an IME request");
    let (x, y, w, h) = ime.cursor_area;
    assert!(
        contains(&r, x, y, w, h),
        "IME area {:?} inside {id} {r:?}",
        ime.cursor_area
    );
    let t = d.accessibility_tree().unwrap();
    let b = element_bounds(&t, id).expect("a11y bounds");
    assert!(
        (b.x - r.x).abs() < 0.5 && (b.y - r.y).abs() < 0.5,
        "a11y bounds {b:?} equal the element rect {r:?}"
    );
    let runs = run_bounds(&t, id);
    assert!(!runs.is_empty(), "text runs of {id}");
    for b in runs {
        assert!(
            contains(&r, b.x, b.y, b.w, b.h),
            "text run {b:?} inside {id} {r:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Fixed boxes (PR #4's sibling paths)

const FIXED_TEXTAREA: &str = "<!doctype html><style>body{margin:0}#tall{height:3000px}\
    #fx{position:fixed;top:50px;left:10px;width:320px;height:220px}\
    textarea{display:block;width:300px;height:160px;font:16px/20px monospace;padding:0;border:0;margin:0}</style>\
    <div id=fx><textarea id=ta>line one\nline two\nline three\nline four</textarea></div><div id=tall></div>";

#[test]
fn a_textarea_in_a_fixed_box_keeps_its_geometry_after_the_page_scrolls() {
    let mut d = doc(FIXED_TEXTAREA, 400, 300);
    d.focus("ta").unwrap();
    d.set_control_selection(Some("ta"), None, 0, 0, "none")
        .unwrap();
    let before = rect(&mut d, "ta");
    let ime_before = d.ime_request().cloned().unwrap().cursor_area;
    d.scroll_to(None, None, Some(400.0)).unwrap();
    d.render(400, 300, 1.0).unwrap();
    let after = rect(&mut d, "ta");
    assert_eq!((after.x, after.y), (10.0, 50.0), "{after:?}");
    assert_eq!(after, before);
    // The IME area (of the same caret) stays with the field: (10, 50) is line one's start.
    d.set_control_selection(Some("ta"), None, 0, 0, "none")
        .unwrap();
    let ime_after = d.ime_request().cloned().unwrap().cursor_area;
    assert_eq!(
        ime_after.1, ime_before.1,
        "IME y does not move with the page"
    );
    assert_eq!((ime_after.0, ime_after.1), (10.0, 50.0), "{ime_after:?}");
    assert_text_geometry_inside(&mut d, "ta");
    // A click on the visible third line (y = 50 + 2 * 20 + 10) puts the caret in "line three".
    click(&mut d, 30.0, 100.0);
    let (s, e) = caret(&mut d, "ta");
    assert!(
        (18..=28).contains(&s) && s == e,
        "caret {s}..{e} in line three"
    );
}

const SELECT: &str = "select{font:16px sans-serif;width:150px}";

fn select_doc(wrap: &str, extra: &str) -> String {
    format!(
        "<!doctype html><style>body{{margin:0}}#tall{{height:3000px}}#w{{{wrap}}}{SELECT}{extra}</style>\
         <div id=w><select id=s><option>one</option><option>two</option><option>three</option></select></div>\
         <div id=tall></div>"
    )
}
/// Open the popup of `s` with a click at its visual position, then click at the visual position
/// of an option; the value it took.
fn pick(d: &mut Document, open: (f64, f64), option: (f64, f64)) -> String {
    click(d, open.0, open.1);
    d.render(600, 400, 1.0).unwrap();
    click(d, option.0, option.1);
    d.get_value("s").unwrap()
}
/// The y of option "two" in the plain (unscrolled, untransformed) layout, found by clicking.
fn y_of_two() -> f64 {
    let plain = select_doc("position:absolute;left:0;top:0", "");
    for k in 0..80 {
        let y = 20.0 + k as f64 * 2.0;
        let mut d = doc(&plain, 600, 400);
        let r = rect(&mut d, "s");
        if pick(&mut d, (r.x + 20.0, r.y + r.height / 2.0), (20.0, y)) == "two" {
            // The middle of the row: two more pixels down is still "two".
            return y + 4.0;
        }
    }
    panic!("option two not found");
}

#[test]
fn select_options_are_picked_where_they_are_painted() {
    let y = y_of_two();
    // Fixed, page scrolled by 400: visually unchanged.
    let mut d = doc(&select_doc("position:fixed;left:0;top:0", ""), 600, 400);
    d.scroll_to(None, None, Some(400.0)).unwrap();
    d.render(600, 400, 1.0).unwrap();
    let r = rect(&mut d, "s");
    assert_eq!(r.y, 0.0, "a fixed select does not move: {r:?}");
    assert_eq!(
        pick(&mut d, (20.0, 10.0), (20.0, y)),
        "two",
        "fixed + scrolled"
    );
    // Translated 150 px right: visually 150 px right.
    let mut d = doc(
        &select_doc(
            "position:absolute;left:0;top:0;transform:translateX(150px)",
            "",
        ),
        600,
        400,
    );
    let r = rect(&mut d, "s");
    assert_eq!(r.x, 150.0, "element_rect is the transformed box: {r:?}");
    assert_eq!(
        pick(&mut d, (170.0, r.y + r.height / 2.0), (170.0, y)),
        "two",
        "translated"
    );
}

#[test]
fn listbox_rows_hit_their_option_in_a_fixed_scrolled_and_translated_box() {
    let html = "<!doctype html><style>body{margin:0}#tall{height:3000px}\
        #w{position:fixed;left:20px;top:30px;transform:translate(40px,10px)}\
        select{font:16px/20px sans-serif;width:150px}</style>\
        <div id=tall></div><div id=w><select id=s size=4><option>a</option><option>b</option>\
        <option>c</option><option>d</option><option>e</option><option>f</option></select></div>";
    for scroll in [0.0, 700.0] {
        let mut d = doc(html, 600, 400);
        d.scroll_to(None, None, Some(scroll)).unwrap();
        d.render(600, 400, 1.0).unwrap();
        let n = d.node("s").unwrap();
        let id = d.id();
        let g = d.control_geometry(id, n).unwrap().unwrap();
        let rows = g.inner.select.clone().unwrap();
        assert!(!rows.popup);
        let names = ["a", "b", "c", "d", "e", "f"];
        for row in rows.rows.iter().filter(|r| r.option.is_some()) {
            // The row's centre in the select's inner space → the viewport, through the service.
            let lx = f64::from(row.x + row.width / 2.0) - g.inner.viewport.origin.x.to_f64();
            let ly = f64::from(row.y + row.height / 2.0) - g.inner.viewport.origin.y.to_f64();
            let p = g
                .to_viewport
                .map_point(ui4me_engine_types::geometry::transform::PointF {
                    x: lx - g.scroll.0,
                    y: ly - g.scroll.1,
                });
            let r = rect(&mut d, "s");
            if p.y >= r.y + r.height - 1.0 {
                continue; // scrolled out of the listbox
            }
            assert!(hits(&mut d, n, p.x, p.y), "row hits the select");
            click(&mut d, p.x, p.y);
            let want = names[row.option.unwrap() as usize];
            assert_eq!(
                d.get_value("s").unwrap(),
                want,
                "scroll {scroll}: row of {want}"
            );
        }
        // Visually: the select is at (20 + 40, 30 + 10) whatever the page scroll.
        let r = rect(&mut d, "s");
        assert_eq!((r.x, r.y), (60.0, 40.0), "scroll {scroll}");
    }
}

// ---------------------------------------------------------------------------------------------
// Transforms (INTERACT-03)

#[test]
fn element_rect_and_accessibility_bounds_are_the_transformed_box() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0}#t{position:absolute;left:0;top:0;width:100px;height:50px;\
         transform:translateX(200px) scale(2)}</style><button id=t>T</button>",
        600,
        300,
    );
    // Scale 2 about the centre (50, 25), then 200 px right: x 150..350, y -25..75.
    let r = rect(&mut d, "t");
    assert_eq!(
        (r.x, r.y, r.width, r.height),
        (150.0, -25.0, 200.0, 100.0),
        "{r:?}"
    );
    let n = d.node("t").unwrap();
    assert!(hits(&mut d, n, r.x + r.width / 2.0, r.y + r.height / 2.0));
    let t = d.accessibility_tree().unwrap();
    let b = element_bounds(&t, "t").unwrap();
    assert_eq!((b.x, b.y, b.w, b.h), (150.0, -25.0, 200.0, 100.0));
}

const MONO_INPUT: &str = "input{font:16px monospace;width:200px;padding:0;border:0;margin:0}";

#[test]
fn a_click_on_glyph_k_of_a_translated_input_puts_the_caret_at_k() {
    let value = "0123456789abcdef";
    let html = |t: &str| {
        format!(
            "<!doctype html><style>body{{margin:0}}#w{{position:absolute;left:0;top:0;transform:{t}}}{MONO_INPUT}</style>\
             <div id=w><input id=i value='{value}'></div>"
        )
    };
    // Glyph positions from the untransformed twin.
    let mut plain = doc(&html("none"), 600, 300);
    let n = plain.node("i").unwrap();
    let id = plain.id();
    let g = plain.control_geometry(id, n).unwrap().unwrap();
    let stops: Vec<f64> = (0..=value.len() as u32)
        .map(|k| g.origin.0 + f64::from(g.inner.x_of(0, k)))
        .collect();
    let y = g.origin.1 + 8.0;
    let mut d = doc(&html("translateX(150px)"), 600, 300);
    let target = d.node("i").unwrap();
    for k in [0usize, 3, 5, 9, 15] {
        let x = 150.0 + stops[k] + 0.3 * (stops[k + 1] - stops[k]);
        assert!(hits(&mut d, target, x, y));
        click(&mut d, x, y);
        assert_eq!(
            caret(&mut d, "i"),
            (k as u64, k as u64),
            "glyph {k} at x {x}"
        );
    }
    // The IME area is right under the caret (glyph 15), on screen.
    let (ix, ..) = d.ime_request().cloned().unwrap().cursor_area;
    assert!((ix - (150.0 + stops[15])).abs() < 1.0, "IME x {ix}");
    assert_text_geometry_inside(&mut d, "i");
}

#[test]
fn a_click_in_a_scaled_input_maps_through_the_inverse_transform() {
    let value = "0123456789";
    let html = |t: &str| {
        format!(
            "<!doctype html><style>body{{margin:0}}#w{{position:absolute;left:0;top:0;transform-origin:0 0;transform:{t}}}{MONO_INPUT}</style>\
             <div id=w><input id=i value='{value}'></div>"
        )
    };
    let mut plain = doc(&html("none"), 800, 300);
    let n = plain.node("i").unwrap();
    let id = plain.id();
    let g = plain.control_geometry(id, n).unwrap().unwrap();
    let stops: Vec<f64> = (0..=value.len() as u32)
        .map(|k| g.origin.0 + f64::from(g.inner.x_of(0, k)))
        .collect();
    let mut d = doc(&html("scale(2)"), 800, 300);
    for k in [1usize, 4, 8] {
        let x = 2.0 * (stops[k] + 0.3 * (stops[k + 1] - stops[k]));
        click(&mut d, x, 2.0 * (g.origin.1 + 8.0));
        assert_eq!(caret(&mut d, "i"), (k as u64, k as u64), "glyph {k}");
    }
    assert_text_geometry_inside(&mut d, "i");
}

#[test]
fn a_range_slider_in_a_translated_box_follows_the_pointer() {
    let html = "<!doctype html><style>body{margin:0}#w{position:absolute;left:0;top:0;transform:translateX(200px)}\
        input{width:200px;margin:0;padding:0;border:0}</style><div id=w><input id=r type=range min=0 max=100 value=0></div>";
    let mut d = doc(html, 600, 300);
    let r = rect(&mut d, "r");
    assert_eq!(r.x, 200.0);
    let y = r.y + r.height / 2.0;
    click(&mut d, r.x + r.width - 1.0, y);
    assert_eq!(
        d.get_value("r").unwrap(),
        "100",
        "the right end is the maximum"
    );
    click(&mut d, r.x + 1.0, y);
    assert_eq!(
        d.get_value("r").unwrap(),
        "0",
        "the left end is the minimum"
    );
}

// ---------------------------------------------------------------------------------------------
// A centred modal after scrolling, and a modal <dialog>

const CENTRED: &str = "<!doctype html><style>body{margin:0}#tall{height:3000px}\
    #m{position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);width:260px;background:#eee}\
    textarea{display:block;width:240px;height:80px;font:16px/20px monospace;padding:0;border:0;margin:0}\
    input{font:16px monospace;width:200px;padding:0;border:0;margin:0}\
    select{font:16px sans-serif;width:150px}</style>\
    <div id=tall></div><div id=m><input id=i value='hello modal'>\
    <textarea id=ta>alpha\nbravo\ncharlie</textarea><select id=s><option>one</option><option>two</option></select></div>";

#[test]
fn a_centred_modal_after_scrolling_is_hit_where_it_is_reported() {
    let mut d = doc(CENTRED, 600, 400);
    d.scroll_to(None, None, Some(900.0)).unwrap();
    d.render(600, 400, 1.0).unwrap();
    let m = rect(&mut d, "m");
    assert!(
        (m.x + m.width / 2.0 - 300.0).abs() < 1.0 && (m.y + m.height / 2.0 - 200.0).abs() < 1.0,
        "centred: {m:?}"
    );
    for id in ["m", "i", "ta", "s"] {
        let n = d.node(id).unwrap();
        let r = rect(&mut d, id);
        assert!(
            hits(&mut d, n, r.x + r.width / 2.0, r.y + r.height / 2.0),
            "{id} {r:?}"
        );
    }
    // Caret placement in the textarea: line 2 ("bravo"), glyph 3.
    let ta = rect(&mut d, "ta");
    let n = d.node("ta").unwrap();
    let id = d.id();
    let g = d.control_geometry(id, n).unwrap().unwrap();
    let x = ta.x + f64::from(g.inner.x_of(1, 6 + 3)) + 2.0;
    click(&mut d, x, ta.y + 30.0);
    assert_eq!(caret(&mut d, "ta"), (9, 9), "caret in bravo at glyph 3");
    assert_text_geometry_inside(&mut d, "ta");
    d.focus("i").unwrap();
    assert_text_geometry_inside(&mut d, "i");
}

#[test]
fn a_modal_dialog_on_a_scrolled_page_places_the_caret_under_the_click() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0}#tall{height:3000px}\
         input{font:16px monospace;width:200px;padding:0;border:0;margin:0}</style>\
         <div id=tall></div><dialog id=dlg><input id=i value='0123456789'></dialog>",
        600,
        400,
    );
    d.scroll_to(None, None, Some(1200.0)).unwrap();
    d.show_dialog("dlg", true).unwrap();
    d.render(600, 400, 1.0).unwrap();
    let r = rect(&mut d, "i");
    assert!(
        r.y > 0.0 && r.y < 400.0,
        "the dialog's input is on screen: {r:?}"
    );
    let n = d.node("i").unwrap();
    let id = d.id();
    let g = d.control_geometry(id, n).unwrap().unwrap();
    let x = g.origin.0 + f64::from(g.inner.x_of(0, 4)) + 2.0;
    assert!(hits(&mut d, n, x, r.y + 8.0));
    click(&mut d, x, r.y + 8.0);
    assert_eq!(caret(&mut d, "i"), (4, 4));
    assert_text_geometry_inside(&mut d, "i");
}

// ---------------------------------------------------------------------------------------------
// Property: hit_test(centre(rect(n))) ∈ {n, descendants} for every probed element of each
// fixture, at arbitrary page and container scroll offsets.

struct Fixture {
    html: &'static str,
    /// Probed elements, each with the scroll containers whose scrollport clips it.
    probes: &'static [(&'static str, &'static [&'static str])],
    /// Scroll containers the property scrolls (besides the page).
    scrollers: &'static [&'static str],
}

const FIXTURES: &[Fixture] = &[
    // A fixed panel with an inner scroller.
    Fixture {
        html: "<!doctype html><style>body{margin:0}#tall{height:3000px}\
            #fx{position:fixed;top:40px;left:30px;width:220px;height:200px;overflow:auto;background:#fff}\
            .row{height:40px;margin:4px}#fill{height:400px}</style><div id=tall></div>\
            <div id=fx><button id=b1 class=row>one</button><div id=fill></div><input id=i2 class=row></div>",
        probes: &[("fx", &[]), ("b1", &["fx"]), ("i2", &["fx"])],
        scrollers: &["fx"],
    },
    // Sticky header over a scrolled page.
    Fixture {
        html: "<!doctype html><style>body{margin:0}#tall{height:3000px}\
            #h{position:sticky;top:0;height:50px;background:#ccc;z-index:1}\
            #h input{margin:10px;width:120px}</style>\
            <div id=pad style=height:100px></div><div id=h><input id=hi></div><div id=tall></div>",
        probes: &[("h", &[]), ("hi", &[])],
        scrollers: &[],
    },
    // Nested scrollers.
    Fixture {
        html: "<!doctype html><style>body{margin:0}#tall{height:2000px}\
            #outer{position:absolute;top:60px;left:10px;width:300px;height:250px;overflow:auto}\
            #inner{margin-top:120px;width:260px;height:150px;overflow:auto}\
            .row{display:block;height:30px;width:200px;margin-top:90px}</style>\
            <div id=outer><div id=inner><button id=x class=row>x</button><button id=y class=row>y</button>\
            </div><div style=height:600px></div></div><div id=tall></div>",
        probes: &[
            ("x", &["outer", "inner"]),
            ("y", &["outer", "inner"]),
            ("inner", &["outer"]),
        ],
        scrollers: &["outer", "inner"],
    },
    // Translate and scale.
    Fixture {
        html: "<!doctype html><style>body{margin:0}#tall{height:2000px}\
            #t{position:absolute;left:20px;top:300px;width:120px;height:60px;transform:translate(90px,-40px) scale(1.5)}\
            #t button{display:block;width:80px;height:30px;margin:10px}</style>\
            <div id=t><button id=tb>tb</button></div><div id=tall></div>",
        probes: &[("t", &[]), ("tb", &[])],
        scrollers: &[],
    },
    // A centred modal.
    Fixture {
        html: CENTRED,
        probes: &[("m", &[]), ("i", &[]), ("ta", &[]), ("s", &[])],
        scrollers: &[],
    },
];

fn rect_of(d: &mut Document, id: &str) -> Option<Rect> {
    d.element_rect(id).ok().flatten()
}

fn check_fixture(f: &Fixture, page: f64, inner: f64) -> std::result::Result<(), TestCaseError> {
    let (w, h) = (600.0, 400.0);
    let mut d = doc(f.html, w as u32, h as u32);
    d.scroll_to(None, None, Some(page)).unwrap();
    for s in f.scrollers {
        d.scroll_to(Some(s), None, Some(inner)).unwrap();
    }
    d.render(w as u32, h as u32, 1.0).unwrap();
    for (id, clips) in f.probes {
        let Some(r) = rect_of(&mut d, id) else {
            continue;
        };
        let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
        let mut visible = cx >= 0.0 && cy >= 0.0 && cx < w && cy < h;
        for c in *clips {
            if let Some(p) = rect_of(&mut d, c) {
                // The scrollport, less a scrollbar's width.
                visible &= cx >= p.x
                    && cy >= p.y
                    && cx < p.x + p.width - 16.0
                    && cy < p.y + p.height - 16.0;
            }
        }
        if !visible {
            continue;
        }
        let n = d.node(id).unwrap();
        let hit = hit_id(&mut d, cx, cy);
        prop_assert!(
            within(&d, n, hit),
            "{id}: centre ({cx}, {cy}) of {r:?} hits {:?} (page {page}, inner {inner})",
            hit.and_then(|h| d.pipeline.document().get_attribute(h, "id").ok().flatten())
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]
    #[test]
    fn element_rects_are_hit_where_they_are_reported(
        fixture in 0..FIXTURES.len(),
        page in prop_oneof![Just(0.0), 0.0f64..1500.0],
        inner in prop_oneof![Just(0.0), 0.0f64..400.0],
    ) {
        check_fixture(&FIXTURES[fixture], page.round(), inner.round())?;
    }
}

#[test]
fn every_fixture_is_consistent_before_and_after_scrolling() {
    for f in FIXTURES {
        for (page, inner) in [(0.0, 0.0), (350.0, 0.0), (350.0, 120.0), (1400.0, 300.0)] {
            check_fixture(f, page, inner).unwrap();
        }
    }
}

#[test]
fn document_selection_highlight_follows_fixed_and_translated_text() {
    for wrap in [
        "position:fixed;top:40px;left:20px",
        "position:absolute;top:40px;left:20px;transform:translate(100px,30px)",
    ] {
        let mut d = doc(
            &format!(
                "<!doctype html><style>body{{margin:0}}#tall{{height:3000px}}#w{{{wrap}}}\
                 p{{font:16px sans-serif;margin:0}}</style><div id=tall></div><div id=w><p id=p>hello world</p></div>"
            ),
            600,
            400,
        );
        if wrap.contains("fixed") {
            d.scroll_to(None, None, Some(500.0)).unwrap();
            d.render(600, 400, 1.0).unwrap();
        } else {
            d.scroll_to(None, None, Some(0.0)).unwrap();
        }
        d.select_text(Some("p")).unwrap();
        let p = rect(&mut d, "p");
        let rects = d.selection_rects().unwrap();
        assert!(!rects.is_empty(), "{wrap}");
        for (x, y, w, h) in rects {
            assert!(
                contains(&p, x, y, w, h),
                "{wrap}: highlight {:?} inside {p:?}",
                (x, y, w, h)
            );
        }
    }
}
