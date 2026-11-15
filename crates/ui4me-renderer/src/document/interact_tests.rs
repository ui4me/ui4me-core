//! Stabilization workstream D (INTERACT): focus reveal (F-08 / INTERACT-01) and the keyboard
//! activation matrix (T10: F-17 / INTERACT-05, F-17b / INTERACT-06, -07) through the production
//! facade.
use super::*;
use crate::a11y::{A11yRequest, A11yRequestKind};

fn key_input(key: &str, code: &str, pressed: bool, shift: bool, ctrl: bool) -> Input {
    Input::Key {
        key: key.into(),
        code: code.into(),
        pressed,
        repeat: false,
        shift,
        ctrl,
        alt: false,
        meta: false,
    }
}
/// A full key press (down and up) of `key`; the events of both.
fn press_key(d: &mut Document, key: &str, code: &str) -> Vec<Value> {
    let mut ev = d.handle_input(key_input(key, code, true, false, false)).unwrap();
    ev.extend(d.handle_input(key_input(key, code, false, false, false)).unwrap());
    ev
}
fn tab(d: &mut Document) -> Vec<Value> {
    press_key(d, "Tab", "Tab")
}
fn ptr(d: &mut Document, pressed: bool, x: f64, y: f64) -> Vec<Value> {
    d.handle_input(Input::PointerButton {
        pressed,
        button: 0,
        x,
        y,
    })
    .unwrap()
}
fn focused(d: &Document) -> String {
    d.interaction
        .focused_element(d.id())
        .and_then(|n| d.pipeline.document().get_attribute(n, "id").ok().flatten())
        .unwrap_or("")
        .to_owned()
}
fn page_y(d: &mut Document) -> f64 {
    d.scroll_state(None).unwrap().y
}
fn rect(d: &mut Document, id: &str) -> Rect {
    d.element_rect(id).unwrap().unwrap()
}
/// `r` lies inside the viewport `(0, 0, w, h)` (to a sub-pixel).
fn inside_viewport(r: &Rect, w: f64, h: f64) -> bool {
    r.x >= -0.5 && r.y >= -0.5 && r.x + r.width <= w + 0.5 && r.y + r.height <= h + 0.5
}
/// `"type@id"` of each event, in order.
fn types(ev: &[Value]) -> Vec<String> {
    ev.iter()
        .map(|e| {
            format!(
                "{}@{}",
                e["type"].as_str().unwrap_or("?"),
                e["element"].as_str().unwrap_or("")
            )
        })
        .collect()
}
fn doc(html: &str, w: u32, h: u32) -> Document {
    let mut d = Document::parse(html).unwrap();
    d.use_manual_clock();
    d.render(w, h, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    d
}

const TALL: &str = "<!doctype html><style>body{margin:0}button{display:block;height:30px;width:120px}\
    #gap{height:1500px}</style><button id=a>A</button><div id=gap></div><button id=b>B</button>";

// ---------------------------------------------------------------------------------------------
// F-08 / INTERACT-01: focus scrolls into view

#[test]
fn tab_to_an_off_screen_button_scrolls_it_into_view() {
    let mut d = doc(TALL, 400, 300);
    tab(&mut d);
    assert_eq!(focused(&d), "a");
    assert_eq!(page_y(&mut d), 0.0, "a visible target does not scroll");
    tab(&mut d);
    assert_eq!(focused(&d), "b");
    let y = page_y(&mut d);
    assert!(y > 0.0, "Tab to an off-screen button scrolls (scroll y = {y})");
    let r = rect(&mut d, "b");
    assert!(inside_viewport(&r, 400.0, 300.0), "b is fully visible: {r:?}");
    // `nearest`: the target ends at the bottom edge, not at the top.
    assert!((r.y + r.height - 300.0).abs() < 1.0, "aligned to the nearest edge: {r:?}");
    // Shift+Tab back to `a` scrolls up again.
    d.handle_input(key_input("Tab", "Tab", true, true, false)).unwrap();
    d.handle_input(key_input("Tab", "Tab", false, true, false)).unwrap();
    assert_eq!(focused(&d), "a");
    assert!(inside_viewport(&rect(&mut d, "a"), 400.0, 300.0));
}

#[test]
fn tab_to_an_off_screen_text_input_scrolls_it_into_view() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0}#gap{height:1500px}</style>\
         <input id=a><div id=gap></div><input id=b value=hello>",
        400,
        300,
    );
    tab(&mut d);
    tab(&mut d);
    assert_eq!(focused(&d), "b");
    assert!(inside_viewport(&rect(&mut d, "b"), 400.0, 300.0));
    // The IME area reported for the focused field is on screen too.
    let ime = d.ime_request().cloned().expect("an IME request for the focused field");
    assert!(
        ime.cursor_area.1 >= 0.0 && ime.cursor_area.1 < 300.0,
        "IME cursor area {:?} is on screen",
        ime.cursor_area
    );
}

#[test]
fn backend_focus_scrolls_the_target_into_view() {
    let mut d = doc(TALL, 400, 300);
    d.focus("b").unwrap();
    assert!(page_y(&mut d) > 0.0);
    assert!(inside_viewport(&rect(&mut d, "b"), 400.0, 300.0));
}

#[test]
fn assistive_focus_scrolls_the_target_into_view() {
    let mut d = doc(TALL, 400, 300);
    let target = d.node("b").ok();
    d.accessibility_request(&A11yRequest {
        target,
        kind: A11yRequestKind::Focus,
    })
    .unwrap();
    assert_eq!(focused(&d), "b");
    assert!(inside_viewport(&rect(&mut d, "b"), 400.0, 300.0));
}

#[test]
fn pointer_focus_does_not_scroll() {
    // The button is half visible at the bottom edge; clicking its visible half focuses it
    // without scrolling (browsers do not scroll on click focus).
    let mut d = doc(
        "<!doctype html><style>body{margin:0}#gap{height:280px}button{display:block;height:40px}\
         #tail{height:1000px}</style><div id=gap></div><button id=b>B</button><div id=tail></div>",
        400,
        300,
    );
    ptr(&mut d, true, 10.0, 290.0);
    ptr(&mut d, false, 10.0, 290.0);
    assert_eq!(focused(&d), "b");
    assert_eq!(page_y(&mut d), 0.0);
}

#[test]
fn nested_scroll_containers_scroll_innermost_first() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0}button{display:block;height:30px}\
         #gap{height:900px}#box{height:100px;overflow:auto}#inner{height:600px}</style>\
         <button id=a>A</button><div id=gap></div>\
         <div id=box><div id=inner></div><button id=b>B</button></div>",
        400,
        300,
    );
    tab(&mut d);
    tab(&mut d);
    assert_eq!(focused(&d), "b");
    assert!(d.scroll_state(Some("box")).unwrap().y > 0.0, "the inner box scrolled");
    assert!(page_y(&mut d) > 0.0, "the page scrolled to the box");
    let r = rect(&mut d, "b");
    assert!(inside_viewport(&r, 400.0, 300.0), "b is visible: {r:?}");
    let port = rect(&mut d, "box");
    assert!(
        r.y >= port.y - 0.5 && r.y + r.height <= port.y + port.height + 0.5,
        "b is inside its scrollport: {r:?} in {port:?}"
    );
}

#[test]
fn focus_inside_a_fixed_box_does_not_scroll_the_page() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0}#tall{height:3000px}\
         #fx{position:fixed;top:10px;left:10px;width:200px;height:280px;overflow:auto}\
         button{display:block;height:30px}#fill{height:600px}</style>\
         <div id=tall></div><div id=fx><button id=a>A</button><div id=fill></div><button id=b>B</button></div>",
        400,
        300,
    );
    d.scroll_to(None, None, Some(500.0)).unwrap();
    d.focus("a").unwrap();
    assert_eq!(page_y(&mut d), 500.0, "focus in a fixed box keeps the page scroll");
    d.focus("b").unwrap();
    assert_eq!(page_y(&mut d), 500.0, "the page does not move a fixed box");
    assert!(d.scroll_state(Some("fx")).unwrap().y > 0.0, "the fixed box's own scroller moved");
    assert!(inside_viewport(&rect(&mut d, "b"), 400.0, 300.0));
}

#[test]
fn tab_inside_a_scrolled_modal_dialog_reveals_the_control() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0}#tall{height:3000px}\
         dialog{max-height:200px;overflow:auto;padding:0;width:200px}\
         button,input{display:block;height:30px}#fill{height:500px}</style>\
         <div id=tall></div><dialog id=dlg><button id=a>A</button><div id=fill></div><input id=b></dialog>",
        400,
        300,
    );
    d.scroll_to(None, None, Some(800.0)).unwrap();
    d.show_dialog("dlg", true).unwrap();
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(focused(&d), "a", "showModal focuses the first control");
    tab(&mut d);
    assert_eq!(focused(&d), "b");
    assert_eq!(page_y(&mut d), 800.0, "the page behind the modal does not scroll");
    assert!(d.scroll_state(Some("dlg")).unwrap().y > 0.0, "the dialog scrolled");
    let r = rect(&mut d, "b");
    let port = rect(&mut d, "dlg");
    assert!(
        r.y >= port.y - 0.5 && r.y + r.height <= port.y + port.height + 0.5,
        "b is inside the dialog's scrollport: {r:?} in {port:?}"
    );
    // The control is where it is hit.
    let hit = d.element_at_point(r.x + r.width / 2.0, r.y + r.height / 2.0).unwrap();
    assert_eq!(hit["element"], "b", "hit {hit}");
}

