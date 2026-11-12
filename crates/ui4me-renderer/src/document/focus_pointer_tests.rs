//! F0507 regression matrix: tab order, dialog trapping, removed/hidden/disabled targets and
//! pointer press/drag lifetimes through the production facade.
use super::*;

fn key(key: &str, shift: bool) -> Input {
    Input::Key {
        key: key.into(),
        code: key.into(),
        pressed: true,
        repeat: false,
        shift,
        ctrl: false,
        alt: false,
        meta: false,
    }
}
fn key_up(key: &str) -> Input {
    Input::Key {
        key: key.into(),
        code: key.into(),
        pressed: false,
        repeat: false,
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    }
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
fn mv(d: &mut Document, x: f64, y: f64) {
    d.handle_input(Input::PointerMoved { x, y }).unwrap();
}
fn center(d: &mut Document, el: &str) -> (f64, f64) {
    let r = d.element_rect(el).unwrap().unwrap();
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}
fn click_at(d: &mut Document, x: f64, y: f64) -> Vec<Value> {
    let mut events = ptr(d, true, x, y);
    events.extend(ptr(d, false, x, y));
    events
}
/// The id of the focused element, `""` when focus is on the body/document.
fn focused(d: &Document) -> String {
    d.interaction
        .focused_element(d.id())
        .and_then(|n| d.pipeline.document().get_attribute(n, "id").ok().flatten())
        .unwrap_or("")
        .to_owned()
}
fn tab_cycle(d: &mut Document, shift: bool, steps: usize) -> Vec<String> {
    (0..steps)
        .map(|_| {
            d.handle_input(key("Tab", shift)).unwrap();
            focused(d)
        })
        .collect()
}
fn clicks(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e["type"] == "click")
        .map(|e| e["element"].as_str().unwrap_or("").to_owned())
        .collect()
}

const PAGE: &str = "<!doctype html><style>body{margin:0;font-size:16px} button{margin:0;height:24px;width:120px}</style>\
    <button id=a>A</button>\
    <button id=p2 tabindex=2>P2</button>\
    <button id=p1 tabindex=1>P1</button>\
    <button id=neg tabindex=-1>Neg</button>\
    <button id=dis disabled>Dis</button>\
    <button id=hid hidden>Hid</button>\
    <button id=dn style='display:none'>DN</button>\
    <button id=vh style='visibility:hidden'>VH</button>\
    <div inert><button id=in>In</button></div>\
    <button id=z>Z</button>";

#[test]
fn tab_order_skips_ineligible_nodes_and_honors_positive_tabindex() {
    let mut d = Document::parse(PAGE).unwrap();
    d.render(400, 400, 1.0).unwrap();
    assert_eq!(
        tab_cycle(&mut d, false, 5),
        ["p1", "p2", "a", "z", "p1"],
        "forward order wraps"
    );
    assert_eq!(
        tab_cycle(&mut d, true, 5),
        ["z", "a", "p2", "p1", "z"],
        "backward order wraps"
    );
    let seen = tab_cycle(&mut d, false, 8);
    for id in ["neg", "dis", "hid", "dn", "vh", "in"] {
        assert!(!seen.contains(&id.to_owned()), "{id}");
    }
    d.focus("neg").unwrap();
    assert_eq!(focused(&d), "neg");
    for id in ["dis", "hid", "dn", "vh", "in"] {
        assert!(d.focus(id).is_err(), "{id} must not take focus");
        assert_eq!(focused(&d), "neg", "{id} left focus alone");
    }
}

#[test]
fn modal_dialog_traps_tab_restores_focus_and_escape_cancels() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0} button,input{display:block;height:24px;width:100px}</style>\
         <button id=opener>Open</button><button id=outside>Out</button>\
         <dialog id=dlg><button id=ok>OK</button><input id=name><button id=cancel>Cancel</button></dialog>",
    )
    .unwrap();
    d.render(400, 400, 1.0).unwrap();
    d.focus("opener").unwrap();
    d.show_dialog("dlg", true).unwrap();
    d.drain_events();
    assert_eq!(focused(&d), "ok", "first focusable element takes focus");
    assert_eq!(
        tab_cycle(&mut d, false, 6),
        ["name", "cancel", "ok", "name", "cancel", "ok"]
    );
    assert_eq!(tab_cycle(&mut d, true, 3), ["cancel", "name", "ok"]);
    assert!(
        d.focus("outside").is_err(),
        "inert content cannot be focused"
    );
    d.render(400, 400, 1.0).unwrap();
    let (x, y) = center(&mut d, "outside");
    let events = click_at(&mut d, x, y);
    assert!(!clicks(&events).contains(&"outside".to_owned()));
    assert!(d.dialog_state("dlg").unwrap().0, "still open");
    // Escape cancels and closes, returning focus to the opener.
    let events = d.handle_input(key("Escape", false)).unwrap();
    assert!(events.iter().any(|e| e["type"] == "cancel"), "{events:?}");
    assert!(!d.dialog_state("dlg").unwrap().0);
    assert_eq!(focused(&d), "opener");
    // close() also restores focus.
    d.show_dialog("dlg", true).unwrap();
    assert_eq!(focused(&d), "ok");
    d.close_dialog("dlg", Some("done")).unwrap();
    assert_eq!(focused(&d), "opener");
    assert_eq!(tab_cycle(&mut d, false, 2), ["outside", "opener"]);
}

#[test]
fn dialog_opener_removed_before_close_leaves_no_stale_focus() {
    let mut d = Document::parse(
        "<!doctype html><button id=opener>Open</button><button id=other>O</button>\
         <dialog id=dlg><button id=ok>OK</button></dialog>",
    )
    .unwrap();
    d.render(400, 400, 1.0).unwrap();
    d.focus("opener").unwrap();
    d.show_dialog("dlg", true).unwrap();
    d.remove_element("opener").unwrap();
    d.handle_input(key("Escape", false)).unwrap();
    assert_ne!(focused(&d), "opener");
    assert!(!d.has_element("opener"));
    let order = tab_cycle(&mut d, false, 2);
    assert!(order.iter().all(|s| s == "other"), "{order:?}");
}

#[test]
fn focused_element_removed_hidden_or_disabled_drops_focus() {
    for how in [
        "remove",
        "display",
        "class",
        "disabled",
        "hidden-attr",
        "visibility",
    ] {
        let mut d = Document::parse(
            "<!doctype html><style>.gone{display:none}</style>\
             <button id=a>A</button><button id=b>B</button><button id=c>C</button>",
        )
        .unwrap();
        d.render(400, 200, 1.0).unwrap();
        d.focus("b").unwrap();
        d.drain_events();
        match how {
            "remove" => d.remove_element("b").unwrap(),
            "display" => d.set_attribute("b", "style", Some("display:none")).unwrap(),
            "class" => d.add_class("b", "gone").unwrap(),
            "disabled" => d.set_attribute("b", "disabled", Some("")).unwrap(),
            "hidden-attr" => d.set_attribute("b", "hidden", Some("")).unwrap(),
            _ => d
                .set_attribute("b", "style", Some("visibility:hidden"))
                .unwrap(),
        }
        d.render(400, 200, 1.0).unwrap();
        let events = d.handle_input(key("Tab", false)).unwrap();
        assert!(
            !events
                .iter()
                .any(|e| e["element"] == "b" && e["type"] == "focus"),
            "{how}: {events:?}"
        );
        let f = focused(&d);
        assert!(
            ["a", "c"].contains(&f.as_str()),
            "{how}: next Tab lands on a live stop, got {f:?}"
        );
        let mut events = d.handle_input(key("Enter", false)).unwrap();
        events.extend(d.handle_input(key_up("Enter")).unwrap());
        assert!(
            !events.iter().any(|e| e["element"] == "b"),
            "{how}: {events:?}"
        );
    }
}

#[test]
fn removed_focused_node_drops_events_and_focus_is_body() {
    let mut d =
        Document::parse("<!doctype html><input id=t value=x><button id=n>N</button>").unwrap();
    d.render(300, 100, 1.0).unwrap();
    d.focus("t").unwrap();
    d.drain_events();
    d.remove_element("t").unwrap();
    assert_eq!(focused(&d), "");
    let mut events = d.handle_input(Input::Text("abc".into())).unwrap();
    events.extend(d.handle_input(key("Backspace", false)).unwrap());
    events.extend(d.drain_events());
    assert!(
        !events
            .iter()
            .any(|e| e["element"] == "t" && !d.event_is_current(e) && e["type"] == "input"),
        "{events:?}"
    );
    assert!(d.get_value("t").is_err());
    d.handle_input(key("Tab", false)).unwrap();
    assert_eq!(focused(&d), "n");
}

const BTN_PAGE: &str = "<!doctype html><style>body{margin:0} button{display:block;width:100px;height:30px;margin:0}</style>\
    <button id=b>B</button><button id=c>C</button>";

#[test]
fn pointer_press_release_outside_or_after_leaving_does_not_click() {
    for how in ["empty", "left", "far", "other"] {
        let mut d = Document::parse(BTN_PAGE).unwrap();
        d.render(300, 200, 1.0).unwrap();
        let (x, y) = center(&mut d, "b");
        mv(&mut d, x, y);
        ptr(&mut d, true, x, y);
        let (rx, ry) = match how {
            "empty" => (250.0, 150.0),
            "left" => {
                d.handle_input(Input::PointerLeft).unwrap();
                (x, y + 100.0)
            }
            "far" => (-500.0, 9000.0),
            _ => center(&mut d, "c"),
        };
        let events = ptr(&mut d, false, rx, ry);
        assert!(
            !clicks(&events).contains(&"b".to_owned()),
            "{how}: {events:?}"
        );
        let (x, y) = center(&mut d, "b");
        let events = click_at(&mut d, x, y);
        assert_eq!(clicks(&events), ["b"], "{how}: no stale press state");
    }
}

#[test]
fn window_focus_loss_during_press_cancels_the_click() {
    let mut d = Document::parse(BTN_PAGE).unwrap();
    d.render(300, 200, 1.0).unwrap();
    let (x, y) = center(&mut d, "b");
    ptr(&mut d, true, x, y);
    d.handle_input(Input::Focus(false)).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    let events = ptr(&mut d, false, x, y);
    assert!(clicks(&events).is_empty(), "{events:?}");
    assert!(d.interaction.active_press_element(d.id()).is_none());
}

#[test]
fn pointer_press_target_removed_before_release_is_safe() {
    let mut d = Document::parse(BTN_PAGE).unwrap();
    d.render(300, 200, 1.0).unwrap();
    let (x, y) = center(&mut d, "b");
    mv(&mut d, x, y);
    ptr(&mut d, true, x, y);
    d.remove_element("b").unwrap();
    d.render(300, 200, 1.0).unwrap();
    let events = ptr(&mut d, false, x, y);
    assert!(clicks(&events).is_empty(), "{events:?}");
    assert!(d.interaction.active_press_element(d.id()).is_none());
    let (x, y) = center(&mut d, "c");
    let events = click_at(&mut d, x, y);
    assert_eq!(clicks(&events), ["c"]);
}

#[test]
fn disabled_controls_ignore_pointer_keyboard_and_focus() {
    let mut d = Document::parse(BTN_PAGE).unwrap();
    d.render(300, 200, 1.0).unwrap();
    let (x, y) = center(&mut d, "b");
    ptr(&mut d, true, x, y);
    d.set_attribute("b", "disabled", Some("")).unwrap();
    d.render(300, 200, 1.0).unwrap();
    let events = ptr(&mut d, false, x, y);
    assert!(clicks(&events).is_empty(), "disabled mid-press: {events:?}");
    let events = click_at(&mut d, x, y);
    assert!(clicks(&events).is_empty(), "{events:?}");
    assert_eq!(focused(&d), "", "a disabled control takes no focus");
    assert!(d.focus("b").is_err());
    for k in ["Enter", " "] {
        let mut events = d.handle_input(key(k, false)).unwrap();
        events.extend(d.handle_input(key_up(k)).unwrap());
        assert!(clicks(&events).is_empty(), "{k}: {events:?}");
    }
    d.handle_input(key("Tab", false)).unwrap();
    assert_eq!(focused(&d), "c");
}

const TEXT_PAGE: &str = "<!doctype html><style>body{margin:0;font-size:20px} input{display:block;margin:0;width:200px;height:30px;font-size:20px;border:0;padding:0} p{margin:0;height:40px}</style>\
    <input id=t value='abcdefghijkl'><p id=a>Alpha beta gamma</p><p id=b>Delta epsilon</p>";

fn copy(d: &mut Document) -> String {
    d.handle_input(Input::Key {
        key: "c".into(),
        code: "c".into(),
        pressed: true,
        repeat: false,
        shift: false,
        ctrl: true,
        alt: false,
        meta: false,
    })
    .unwrap();
    d.clipboard_text().unwrap().unwrap_or_default()
}

fn end_drag(d: &mut Document, end: &str) {
    match end {
        "release" => {
            ptr(d, false, 390.0, 290.0);
        }
        "left" => {
            d.handle_input(Input::PointerLeft).unwrap();
            ptr(d, false, 390.0, 290.0);
        }
        _ => {
            d.handle_input(Input::Focus(false)).unwrap();
            d.handle_input(Input::Focus(true)).unwrap();
        }
    }
}

#[test]
fn text_control_drag_ends_when_pointer_leaves_and_releases_outside() {
    for end in ["release", "left", "focus"] {
        let mut d = Document::parse(TEXT_PAGE).unwrap();
        d.render(400, 300, 1.0).unwrap();
        let r = d.element_rect("t").unwrap().unwrap();
        let y = r.y + 15.0;
        ptr(&mut d, true, r.x + 1.0, y);
        mv(&mut d, r.x + 40.0, y);
        end_drag(&mut d, end);
        let before = copy(&mut d);
        assert!(!before.is_empty(), "{end}: drag selected something");
        mv(&mut d, r.x + 150.0, y);
        mv(&mut d, r.x + 190.0, y);
        assert_eq!(copy(&mut d), before, "{end}: stale drag extended selection");
    }
}

#[test]
fn document_text_drag_ends_when_pointer_leaves_and_releases_outside() {
    for end in ["release", "left", "focus"] {
        let mut d = Document::parse(TEXT_PAGE).unwrap();
        d.render(400, 300, 1.0).unwrap();
        let a = d.element_rect("a").unwrap().unwrap();
        let b = d.element_rect("b").unwrap().unwrap();
        ptr(&mut d, true, a.x + 1.0, a.y + 10.0);
        mv(&mut d, a.x + 60.0, a.y + 10.0);
        end_drag(&mut d, end);
        let before = d.selection_text().unwrap();
        assert!(!before.is_empty(), "{end}");
        mv(&mut d, b.x + 100.0, b.y + 10.0);
        mv(&mut d, b.x + 150.0, b.y + 20.0);
        assert_eq!(
            d.selection_text().unwrap(),
            before,
            "{end}: stale drag extended selection"
        );
    }
}

#[test]
fn hover_target_removed_leaves_no_stale_hover() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0} button{display:block;width:100px;height:30px;margin:0} button:hover{background:#f00}</style>\
         <button id=b>B</button><button id=c>C</button>",
    )
    .unwrap();
    d.render(300, 200, 1.0).unwrap();
    let (x, y) = center(&mut d, "b");
    mv(&mut d, x, y);
    let hovered = d.interaction.hovered_element(d.id());
    assert!(hovered.is_some());
    d.remove_element("b").unwrap();
    d.render(300, 200, 1.0).unwrap();
    let stale = d.interaction.hovered_element(d.id());
    assert!(
        stale.is_none_or(|n| Some(n) != hovered && d.pipeline.document().arena().contains(n)),
        "hover still points at the removed node"
    );
    let (cx, cy) = center(&mut d, "c");
    mv(&mut d, cx, cy);
    let c = d.node("c").unwrap();
    assert_eq!(d.interaction.hovered_element(d.id()), Some(c));
    let px = d.render(300, 200, 1.0).unwrap();
    assert_eq!(
        px[(cy as usize) * 300 + cx as usize] & 0x00ffffff,
        0x00ff0000
    );
    d.handle_input(Input::PointerLeft).unwrap();
    assert!(d.interaction.hovered_element(d.id()).is_none());
}
