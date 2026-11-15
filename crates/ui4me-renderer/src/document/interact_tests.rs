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
    let mut ev = d
        .handle_input(key_input(key, code, true, false, false))
        .unwrap();
    ev.extend(
        d.handle_input(key_input(key, code, false, false, false))
            .unwrap(),
    );
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
    assert!(
        y > 0.0,
        "Tab to an off-screen button scrolls (scroll y = {y})"
    );
    let r = rect(&mut d, "b");
    assert!(
        inside_viewport(&r, 400.0, 300.0),
        "b is fully visible: {r:?}"
    );
    // `nearest`: the target ends at the bottom edge, not at the top.
    assert!(
        (r.y + r.height - 300.0).abs() < 1.0,
        "aligned to the nearest edge: {r:?}"
    );
    // Shift+Tab back to `a` scrolls up again.
    d.handle_input(key_input("Tab", "Tab", true, true, false))
        .unwrap();
    d.handle_input(key_input("Tab", "Tab", false, true, false))
        .unwrap();
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
    let ime = d
        .ime_request()
        .cloned()
        .expect("an IME request for the focused field");
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
    assert!(
        d.scroll_state(Some("box")).unwrap().y > 0.0,
        "the inner box scrolled"
    );
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
    assert_eq!(
        page_y(&mut d),
        500.0,
        "focus in a fixed box keeps the page scroll"
    );
    d.focus("b").unwrap();
    assert_eq!(page_y(&mut d), 500.0, "the page does not move a fixed box");
    assert!(
        d.scroll_state(Some("fx")).unwrap().y > 0.0,
        "the fixed box's own scroller moved"
    );
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
    assert_eq!(
        page_y(&mut d),
        800.0,
        "the page behind the modal does not scroll"
    );
    assert!(
        d.scroll_state(Some("dlg")).unwrap().y > 0.0,
        "the dialog scrolled"
    );
    let r = rect(&mut d, "b");
    let port = rect(&mut d, "dlg");
    assert!(
        r.y >= port.y - 0.5 && r.y + r.height <= port.y + port.height + 0.5,
        "b is inside the dialog's scrollport: {r:?} in {port:?}"
    );
    // The control is where it is hit.
    let hit = d
        .element_at_point(r.x + r.width / 2.0, r.y + r.height / 2.0)
        .unwrap();
    assert_eq!(hit["element"], "b", "hit {hit}");
}

#[test]
fn closing_a_dialog_restores_focus_without_scrolling() {
    // HTML dialog close: "the viewport should not be scrolled by" restoring focus.
    let mut d = doc(
        "<!doctype html><style>body{margin:0}button{display:block;height:30px}#gap{height:2000px}\
         </style><button id=opener>open</button><div id=gap></div>\
         <dialog id=dlg><button id=inside>in</button></dialog>",
        400,
        300,
    );
    d.focus("opener").unwrap();
    d.show_dialog("dlg", true).unwrap();
    assert_eq!(focused(&d), "inside");
    d.scroll_to(None, None, Some(900.0)).unwrap();
    d.close_dialog("dlg", None).unwrap();
    assert_eq!(focused(&d), "opener", "focus goes back to the opener");
    assert_eq!(page_y(&mut d), 900.0, "restoring focus does not scroll");
}

// ---------------------------------------------------------------------------------------------
// T10 keyboard activation matrix (F-17 / INTERACT-05): Enter and Space on each kind of control,
// against browser behaviour.

const MATRIX: &str = "<!doctype html><style>body{margin:0}#gap{height:2000px}</style>\
    <form id=f><button id=btn type=button>B</button><input id=sub type=submit value=S>\
    <input id=cb type=checkbox><input id=r1 type=radio name=g><input id=r2 type=radio name=g></form>\
    <details id=det><summary id=sum>More</summary>hidden</details>\
    <a id=lnk href=\"next.html\">next</a><div id=rb role=button tabindex=0>R</div><div id=gap></div>";

fn matrix() -> Document {
    doc(MATRIX, 400, 300)
}
fn count(ev: &[Value], ty: &str, id: &str) -> usize {
    ev.iter()
        .filter(|e| e["type"] == ty && e["element"] == id)
        .count()
}
fn checked(d: &mut Document, id: &str) -> bool {
    let n = d.node(id).unwrap();
    d.pipeline
        .document()
        .control_state(n)
        .ok()
        .flatten()
        .and_then(|c| c.checkedness)
        == Some(true)
}
fn details_open(d: &mut Document) -> bool {
    let n = d.node("det").unwrap();
    d.pipeline
        .document()
        .get_attribute(n, "open")
        .ok()
        .flatten()
        .is_some()
}
/// Focus `id`, drain the focus events, then press `key` (down and up).
fn press_on(d: &mut Document, id: &str, key: &str, code: &str) -> Vec<Value> {
    d.focus(id).unwrap();
    d.drain_events();
    press_key(d, key, code)
}

#[test]
fn enter_on_a_link_clicks_it_and_activates_it() {
    let mut d = matrix();
    let ev = press_on(&mut d, "lnk", "Enter", "Enter");
    let t = types(&ev);
    let click = t.iter().position(|e| e == "click@lnk");
    let link = t.iter().position(|e| e == "link_activated@lnk");
    assert!(
        click.is_some() && link.is_some(),
        "click then link_activated: {t:?}"
    );
    assert!(click < link, "click precedes link_activated: {t:?}");
    assert_eq!(count(&ev, "click", "lnk"), 1);
    assert!(
        ev.iter()
            .any(|e| e["type"] == "link_activated" && e["href"] == "next.html"),
        "{ev:?}"
    );
}

#[test]
fn enter_on_a_link_honours_a_prevented_click() {
    let mut d = matrix();
    d.set_default_action(
        DefaultActionRule {
            element: "lnk".into(),
            event: "click".into(),
            key: None,
            shift: None,
            ctrl: None,
            alt: None,
            meta: None,
        },
        true,
    )
    .unwrap();
    let ev = press_on(&mut d, "lnk", "Enter", "Enter");
    assert_eq!(count(&ev, "click", "lnk"), 1, "{:?}", types(&ev));
    assert_eq!(count(&ev, "link_activated", "lnk"), 0, "{:?}", types(&ev));
}

#[test]
fn space_on_a_link_scrolls_and_does_not_activate_it() {
    let mut d = matrix();
    let ev = press_on(&mut d, "lnk", " ", "Space");
    assert_eq!(count(&ev, "click", "lnk"), 0, "{:?}", types(&ev));
    assert_eq!(count(&ev, "link_activated", "lnk"), 0);
    assert!(page_y(&mut d) > 0.0, "Space scrolls the page");
}

#[test]
fn enter_and_space_on_a_button_click_once_each() {
    let mut d = matrix();
    let ev = press_on(&mut d, "btn", "Enter", "Enter");
    assert_eq!(count(&ev, "click", "btn"), 1, "Enter: {:?}", types(&ev));
    d.drain_events();
    let down = d
        .handle_input(key_input(" ", "Space", true, false, false))
        .unwrap();
    assert_eq!(
        count(&down, "click", "btn"),
        0,
        "Space clicks on keyup, not keydown"
    );
    let up = d
        .handle_input(key_input(" ", "Space", false, false, false))
        .unwrap();
    assert_eq!(
        count(&up, "click", "btn"),
        1,
        "Space keyup: {:?}",
        types(&up)
    );
    assert_eq!(page_y(&mut d), 0.0, "Space on a button does not scroll");
}

#[test]
fn enter_and_space_on_a_submit_button_submit_the_form() {
    for (key, code) in [("Enter", "Enter"), (" ", "Space")] {
        let mut d = matrix();
        let ev = press_on(&mut d, "sub", key, code);
        assert_eq!(count(&ev, "click", "sub"), 1, "{key:?}: {:?}", types(&ev));
        assert_eq!(count(&ev, "submit", "f"), 1, "{key:?}: {:?}", types(&ev));
    }
}

#[test]
fn space_toggles_a_checkbox_and_enter_does_not() {
    let mut d = matrix();
    let ev = press_on(&mut d, "cb", "Enter", "Enter");
    assert_eq!(count(&ev, "click", "cb"), 0, "{:?}", types(&ev));
    assert!(!checked(&mut d, "cb"));
    let ev = press_key(&mut d, " ", "Space");
    assert_eq!(count(&ev, "click", "cb"), 1, "{:?}", types(&ev));
    assert_eq!(count(&ev, "change", "cb"), 1);
    assert!(checked(&mut d, "cb"));
}

#[test]
fn space_checks_a_radio_and_enter_does_not() {
    let mut d = matrix();
    let ev = press_on(&mut d, "r1", "Enter", "Enter");
    assert_eq!(count(&ev, "click", "r1"), 0, "{:?}", types(&ev));
    assert!(!checked(&mut d, "r1"));
    let ev = press_key(&mut d, " ", "Space");
    assert_eq!(count(&ev, "click", "r1"), 1, "{:?}", types(&ev));
    assert!(checked(&mut d, "r1"));
}

#[test]
fn enter_and_space_on_a_summary_toggle_its_details() {
    let mut d = matrix();
    let ev = press_on(&mut d, "sum", "Enter", "Enter");
    assert_eq!(count(&ev, "click", "sum"), 1, "{:?}", types(&ev));
    assert!(details_open(&mut d), "Enter opened the details");
    let ev = press_key(&mut d, " ", "Space");
    assert_eq!(count(&ev, "click", "sum"), 1, "{:?}", types(&ev));
    assert!(!details_open(&mut d), "Space closed the details");
}

#[test]
fn a_role_button_element_gets_no_synthetic_click() {
    // Browser parity: an ARIA role grants no activation behaviour; the application handles
    // `keydown` itself. The key events reach it.
    let mut d = matrix();
    for (key, code) in [("Enter", "Enter"), (" ", "Space")] {
        let ev = press_on(&mut d, "rb", key, code);
        assert_eq!(count(&ev, "click", "rb"), 0, "{key:?}: {:?}", types(&ev));
        assert_eq!(count(&ev, "keydown", "rb"), 1, "{key:?}: {:?}", types(&ev));
    }
}

// ---------------------------------------------------------------------------------------------
// T10 shortcuts on non-Latin layouts (F-17b / INTERACT-06)

/// Ctrl+`key` with physical `code`, down and up.
fn ctrl_key(d: &mut Document, key: &str, code: &str) -> Vec<Value> {
    let mut ev = d
        .handle_input(key_input(key, code, true, false, true))
        .unwrap();
    ev.extend(
        d.handle_input(key_input(key, code, false, false, true))
            .unwrap(),
    );
    ev
}
fn selection(d: &mut Document, id: &str) -> (u64, u64) {
    let s = d.control_selection(Some(id), None).unwrap();
    (
        s["start"].as_u64().unwrap_or(u64::MAX),
        s["end"].as_u64().unwrap_or(u64::MAX),
    )
}

#[test]
fn shortcuts_fall_back_to_the_physical_key_on_a_non_latin_layout() {
    let mut d = doc("<!doctype html><input id=i value='hello world'>", 400, 300);
    d.focus("i").unwrap();
    assert_eq!(focused(&d), "i");
    d.drain_events();
    // Russian layout: Ctrl+A is key "ф" on code KeyA, Ctrl+C is "с" (U+0441) on KeyC.
    ctrl_key(&mut d, "ф", "KeyA");
    assert_eq!(selection(&mut d, "i"), (0, 11), "Ctrl+A selects all");
    let ev = ctrl_key(&mut d, "с", "KeyC");
    assert_eq!(
        count(&ev, "copy", "i"),
        1,
        "Ctrl+C copies: {:?}",
        types(&ev)
    );
    let ev = ctrl_key(&mut d, "ч", "KeyX");
    assert_eq!(count(&ev, "cut", "i"), 1, "Ctrl+X cuts: {:?}", types(&ev));
    // Greek layout: Ctrl+Z is "ζ" on KeyZ and undoes the cut.
    ctrl_key(&mut d, "ζ", "KeyZ");
    assert_eq!(d.get_value("i").unwrap(), "hello world", "Ctrl+Z undoes");
}

#[test]
fn shortcuts_keep_the_layout_letter_on_a_latin_layout() {
    // Dvorak: the key labelled "a" is on the QWERTY "A" position, but the key labelled "c" is
    // on QWERTY "I". The layout letter wins: Ctrl+"c" on KeyI copies, the code is not consulted.
    let mut d = doc("<!doctype html><input id=i value='hello'>", 400, 300);
    d.focus("i").unwrap();
    d.drain_events();
    let ev = ctrl_key(&mut d, "c", "KeyI");
    assert_eq!(count(&ev, "copy", "i"), 1, "{:?}", types(&ev));
    // A Latin key on the KeyC position that is not a shortcut letter does not copy.
    let ev = ctrl_key(&mut d, "j", "KeyC");
    assert_eq!(count(&ev, "copy", "i"), 0, "{:?}", types(&ev));
}

#[test]
fn document_clipboard_shortcuts_fall_back_to_the_physical_key() {
    // No text control focused: Ctrl+C still reaches application code as `copy`.
    let mut d = doc("<!doctype html><button id=b>B</button>", 400, 300);
    d.focus("b").unwrap();
    d.drain_events();
    let ev = ctrl_key(&mut d, "с", "KeyC");
    assert_eq!(count(&ev, "copy", "b"), 1, "{:?}", types(&ev));
}

#[test]
fn shortcut_key_maps_only_non_latin_letters_with_ctrl_or_meta() {
    use crate::interaction::frame::Modifiers;
    use crate::interaction::keyboard::shortcut_key;
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    let meta = Modifiers {
        meta: true,
        ..Modifiers::default()
    };
    assert_eq!(shortcut_key("ф", "KeyA", ctrl), "a");
    assert_eq!(shortcut_key("Ф", "KeyA", meta), "a");
    assert_eq!(shortcut_key("ф", "KeyA", Modifiers::default()), "ф");
    assert_eq!(shortcut_key("a", "KeyS", ctrl), "a");
    assert_eq!(shortcut_key("Enter", "Enter", ctrl), "Enter");
    assert_eq!(shortcut_key("ö", "Semicolon", ctrl), "ö");
    assert_eq!(shortcut_key("ö", "Digit1", ctrl), "ö");
}

// ---------------------------------------------------------------------------------------------
// T10 click target (F-17b / INTERACT-07)

const BUTTON: &str = "<!doctype html><style>body{margin:0}button{padding:20px;font:16px sans-serif;border:0}\
    </style><button id=b><span id=s1>One</span> <span id=s2>Two</span></button><p id=p>text</p>";

fn centre(d: &mut Document, id: &str) -> (f64, f64) {
    let r = rect(d, id);
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

#[test]
fn press_on_a_child_and_release_on_the_button_clicks_the_button_once() {
    let mut d = doc(BUTTON, 400, 300);
    let (sx, sy) = centre(&mut d, "s1");
    let b = rect(&mut d, "b");
    let mut ev = ptr(&mut d, true, sx, sy);
    ev.extend(ptr(&mut d, false, b.x + 4.0, sy));
    assert_eq!(
        ev.iter().filter(|e| e["type"] == "click").count(),
        1,
        "{:?}",
        types(&ev)
    );
    assert_eq!(count(&ev, "click", "b"), 1, "{:?}", types(&ev));
}

#[test]
fn press_and_release_on_different_children_of_one_button_click_it_once() {
    let mut d = doc(BUTTON, 400, 300);
    let (x1, y1) = centre(&mut d, "s1");
    let (x2, y2) = centre(&mut d, "s2");
    let mut ev = ptr(&mut d, true, x1, y1);
    ev.extend(ptr(&mut d, false, x2, y2));
    assert_eq!(
        ev.iter().filter(|e| e["type"] == "click").count(),
        1,
        "{:?}",
        types(&ev)
    );
    assert_eq!(count(&ev, "click", "b"), 1, "{:?}", types(&ev));
}

#[test]
fn release_outside_the_button_produces_no_click() {
    let mut d = doc(BUTTON, 400, 300);
    let (sx, sy) = centre(&mut d, "s1");
    let (px, py) = centre(&mut d, "p");
    let mut ev = ptr(&mut d, true, sx, sy);
    ev.extend(ptr(&mut d, false, px, py));
    assert_eq!(
        ev.iter().filter(|e| e["type"] == "click").count(),
        0,
        "{:?}",
        types(&ev)
    );
}

#[test]
fn press_on_label_text_and_release_on_its_checkbox_toggles_it() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0;font:16px sans-serif}</style>\
         <label id=l><span id=t>Accept terms</span> <input id=c type=checkbox></label>",
        400,
        300,
    );
    let (tx, ty) = centre(&mut d, "t");
    let (cx, cy) = centre(&mut d, "c");
    ptr(&mut d, true, tx, ty);
    let ev = ptr(&mut d, false, cx, cy);
    assert!(
        checked(&mut d, "c"),
        "the label forwarded one click: {:?}",
        types(&ev)
    );
}

// ---------------------------------------------------------------------------------------------
// F-16b / INTERACT-04: the accessibility key changes with semantics, not with repaints

fn key_of(d: &mut Document) -> crate::AccessibilityKey {
    d.accessibility_key().unwrap()
}

#[test]
fn accessibility_key_ignores_caret_blinks_and_repaints() {
    let mut d = doc(
        "<!doctype html><input id=i value=hello><p id=p style='color:blue'>text</p>",
        400,
        300,
    );
    d.focus("i").unwrap();
    let k = key_of(&mut d);
    let id = d.id();
    let n = d.node("i").unwrap();
    for visible in [false, true, false] {
        DispatchHost::set_caret_state(&mut d, id, n, visible, false);
        d.render(400, 300, 1.0).unwrap();
        assert_eq!(key_of(&mut d), k, "a caret blink keeps the key");
    }
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(key_of(&mut d), k, "a repaint keeps the key");
}

#[test]
fn accessibility_key_changes_with_every_semantic_input() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0}#gap{height:2000px}</style>\
         <input id=i value=hello><button id=b>B</button><div id=t>t</div><div id=gap></div>",
        400,
        300,
    );
    let mut last = key_of(&mut d);
    let mut changed = |d: &mut Document, what: &str| {
        let k = key_of(d);
        assert_ne!(k, last, "{what} changes the key");
        last = k;
    };
    d.set_value("i", "world").unwrap();
    changed(&mut d, "a value");
    d.focus("i").unwrap();
    changed(&mut d, "focus");
    d.set_control_selection(Some("i"), None, 1, 3, "none")
        .unwrap();
    changed(&mut d, "a text selection");
    d.scroll_to(None, None, Some(200.0)).unwrap();
    changed(&mut d, "a scroll");
    d.set_attribute("t", "style", Some("transform:translateX(30px)"))
        .unwrap();
    changed(&mut d, "a transform");
    d.set_attribute("b", "aria-label", Some("Bee")).unwrap();
    changed(&mut d, "an ARIA name");
    d.handle_input(Input::Focus(false)).unwrap();
    changed(&mut d, "window focus");
    d.render(500, 300, 1.0).unwrap();
    changed(&mut d, "a resize");
}

#[test]
fn accessibility_key_follows_a_transform_animation() {
    // A moving element moves its accessibility bounds: an active AT must see them.
    let mut d = doc(
        "<!doctype html><style>@keyframes slide{to{transform:translateX(100px)}}\
         #s{width:20px;height:20px;animation:slide 1s linear infinite}</style><div id=s></div>",
        400,
        300,
    );
    let k = key_of(&mut d);
    d.advance_clock(100.0).unwrap();
    d.render(400, 300, 1.0).unwrap();
    assert_ne!(key_of(&mut d), k);
}

// ---------------------------------------------------------------------------------------------
// INTERACT-10: pointer capture ends when the window loses focus

#[test]
fn window_focus_loss_cancels_the_pointer_and_releases_capture() {
    let mut d = doc(
        "<!doctype html><style>body{margin:0}#c{width:200px;height:100px}</style><div id=c>cap</div>",
        400,
        300,
    );
    ptr(&mut d, true, 50.0, 50.0);
    d.set_pointer_capture("c", 1, true).unwrap();
    d.handle_input(Input::PointerMoved { x: 60.0, y: 60.0 })
        .unwrap();
    assert_eq!(d.pointer_capture().as_deref(), Some("c"));
    let ev = d.handle_input(Input::Focus(false)).unwrap();
    let t = types(&ev);
    let cancel = t.iter().position(|e| e == "pointercancel@c");
    let lost = t.iter().position(|e| e == "lostpointercapture@c");
    assert!(
        cancel.is_some() && cancel < lost,
        "pointercancel then lostpointercapture: {t:?}"
    );
    assert_eq!(d.pointer_capture(), None);
    // The next buttonless move goes to what is under the pointer, not to the old capture target.
    let ev = d
        .handle_input(Input::PointerMoved { x: 300.0, y: 250.0 })
        .unwrap();
    assert_eq!(count(&ev, "pointermove", "c"), 0, "{:?}", types(&ev));
    // Without a capture, focus loss dispatches no pointer events.
    d.handle_input(Input::Focus(true)).unwrap();
    let ev = d.handle_input(Input::Focus(false)).unwrap();
    assert!(
        !ev.iter().any(|e| e["type"] == "pointercancel"),
        "{:?}",
        types(&ev)
    );
}
