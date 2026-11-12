//! F0805: range / number / progress / meter semantics through the production facade (keyboard,
//! pointer, validation, reset, disabled, RTL, accessibility values).
use super::*;
use accesskit::Role;

fn key(k: &str) -> Input {
    Input::Key {
        key: k.into(),
        code: k.into(),
        pressed: true,
        repeat: false,
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    }
}
fn press(d: &mut Document, k: &str) -> Vec<Value> {
    d.handle_input(key(k)).unwrap()
}
/// The `input` events and the commits (the facade reports the DOM `change` as `commit`; its
/// `change` is the legacy ABI 0.1 alias of every `input`).
fn types(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e["type"] == "input" || e["type"] == "commit")
        .map(|e| {
            if e["type"] == "commit" {
                "change".to_owned()
            } else {
                "input".to_owned()
            }
        })
        .collect()
}
fn page(body: &str) -> Document {
    let mut d = Document::parse(&format!(
        "<!doctype html><style>body{{margin:0}} input[type=range]{{display:block;width:200px;height:20px;margin:0;padding:0}}</style>{body}"
    ))
    .unwrap();
    d.render(400, 300, 1.0).unwrap();
    d
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
fn mv(d: &mut Document, x: f64, y: f64) -> Vec<Value> {
    d.handle_input(Input::PointerMoved { x, y }).unwrap()
}
fn crect(d: &mut Document, id: &str) -> (f64, f64, f64, f64) {
    let n = d.node(id).unwrap();
    d.element_content_rect(d.id(), n).unwrap().unwrap()
}
fn click_el(d: &mut Document, id: &str) -> Vec<Value> {
    let (x, y, w, h) = crect(d, id);
    let mut ev = btn(d, true, x + w / 2.0, y + h / 2.0);
    ev.extend(btn(d, false, x + w / 2.0, y + h / 2.0));
    ev
}
fn flag_names(d: &mut Document, id: &str) -> Vec<String> {
    let v = d.get_validity(id).unwrap();
    v["flags"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, on)| on.as_bool() == Some(true))
        .map(|(k, _)| k.clone())
        .collect()
}
fn node<'a>(t: &'a crate::a11y::A11yTree, id: &str) -> &'a crate::a11y::A11yNode {
    t.nodes
        .values()
        .find(|n| n.html_id.as_deref() == Some(id))
        .unwrap()
}

#[test]
fn range_keys_step_snap_and_fire_input_then_change() {
    let mut d = page("<input id=r type=range min=0 max=100 step=5 value=50>");
    d.focus("r").unwrap();
    let ev = press(&mut d, "ArrowUp");
    assert_eq!(types(&ev), ["input", "change"]);
    assert_eq!(d.get_value("r").unwrap(), "55");
    press(&mut d, "ArrowRight");
    assert_eq!(d.get_value("r").unwrap(), "60");
    press(&mut d, "ArrowLeft");
    press(&mut d, "ArrowDown");
    assert_eq!(d.get_value("r").unwrap(), "50");
    press(&mut d, "PageUp");
    assert_eq!(d.get_value("r").unwrap(), "60");
    press(&mut d, "PageDown");
    press(&mut d, "PageDown");
    assert_eq!(d.get_value("r").unwrap(), "40");
    press(&mut d, "End");
    assert_eq!(d.get_value("r").unwrap(), "100");
    let ev = press(&mut d, "ArrowUp");
    assert!(types(&ev).is_empty(), "clamped at max: no events");
    press(&mut d, "Home");
    assert_eq!(d.get_value("r").unwrap(), "0");
}

#[test]
fn range_step_base_is_min_and_step_any_is_continuous() {
    let mut d = page(
        "<input id=a type=range min=3 max=20 step=4 value=3>\
         <input id=b type=range min=0 max=1 step=any value=0.25>",
    );
    d.focus("a").unwrap();
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("a").unwrap(), "7");
    press(&mut d, "End");
    assert_eq!(d.get_value("a").unwrap(), "19", "largest valid step <= max");
    d.focus("b").unwrap();
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("b").unwrap(), "1");
}

#[test]
fn range_invalid_attributes_fall_back_to_defaults() {
    let mut d = page(
        "<input id=a type=range min=50 max=10 value=70>\
         <input id=b type=range min=abc max=xyz step=-3 value=junk>\
         <input id=c type=range>",
    );
    // max < min: max = min.
    d.focus("a").unwrap();
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("a").unwrap(), "50");
    // Non-numeric attributes: 0..100 step 1, value defaults to the midpoint.
    d.focus("b").unwrap();
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("b").unwrap(), "51");
    d.focus("c").unwrap();
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("c").unwrap(), "51");
}

#[test]
fn range_ignores_disabled_but_not_readonly() {
    let mut d = page(
        "<fieldset disabled><input id=f type=range value=5 max=10></fieldset>\
         <input id=o type=range value=5 max=10 disabled>\
         <input id=ro type=range value=5 max=10 readonly>",
    );
    d.focus("ro").unwrap();
    let ev = press(&mut d, "ArrowUp");
    assert_eq!(types(&ev), ["input", "change"]);
    assert_eq!(
        d.get_value("ro").unwrap(),
        "6",
        "readonly does not apply to range"
    );
    for id in ["o", "f"] {
        let (x, y, w, h) = crect(&mut d, id);
        let mut ev = btn(&mut d, true, x + w - 2.0, y + h / 2.0);
        ev.extend(btn(&mut d, false, x + w - 2.0, y + h / 2.0));
        assert!(types(&ev).is_empty(), "{id}");
        assert_eq!(d.get_value(id).unwrap(), "5", "{id}");
    }
}

#[test]
fn range_rtl_swaps_horizontal_arrows_and_mirrors_the_track() {
    let mut d = page(
        "<div dir=rtl><input id=r type=range min=0 max=100 value=50></div>\
         <input id=l type=range min=0 max=100 value=50>",
    );
    d.focus("r").unwrap();
    press(&mut d, "ArrowLeft");
    assert_eq!(d.get_value("r").unwrap(), "51");
    press(&mut d, "ArrowRight");
    press(&mut d, "ArrowRight");
    assert_eq!(d.get_value("r").unwrap(), "49");
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("r").unwrap(), "50", "vertical arrows unchanged");
    d.focus("l").unwrap();
    press(&mut d, "ArrowRight");
    assert_eq!(d.get_value("l").unwrap(), "51");
    // Pointer: the left edge of an RTL track is the maximum.
    let (x, y, _, h) = crect(&mut d, "r");
    btn(&mut d, true, x + 1.0, y + h / 2.0);
    btn(&mut d, false, x + 1.0, y + h / 2.0);
    assert_eq!(d.get_value("r").unwrap(), "100");
}

#[test]
fn range_pointer_drag_fires_input_while_dragging_and_change_on_release() {
    let mut d = page("<input id=r type=range min=0 max=100 value=0>");
    let (x, y, w, h) = crect(&mut d, "r");
    let cy = y + h / 2.0;
    let ev = btn(&mut d, true, x + w / 2.0, cy);
    assert_eq!(types(&ev), ["input"]);
    let mid: i32 = d.get_value("r").unwrap().parse().unwrap();
    assert!((45..=55).contains(&mid), "{mid}");
    let ev = mv(&mut d, x + w - 1.0, cy);
    assert_eq!(types(&ev), ["input"]);
    assert_eq!(d.get_value("r").unwrap(), "100");
    // Past the track end stays clamped.
    mv(&mut d, x + w + 80.0, cy + 40.0);
    assert_eq!(d.get_value("r").unwrap(), "100");
    let ev = btn(&mut d, false, x + w + 80.0, cy + 40.0);
    assert_eq!(types(&ev), ["change"]);
    // After release a move changes nothing.
    let ev = mv(&mut d, x + 2.0, cy);
    assert!(types(&ev).is_empty());
    assert_eq!(d.get_value("r").unwrap(), "100");
}

#[test]
fn number_arrows_follow_step_up_and_step_down() {
    let mut d = page(
        "<input id=n type=number min=0 max=10 step=3>\
         <input id=m type=number step=any>\
         <input id=ro type=number value=4 readonly><input id=off type=number value=4 disabled>",
    );
    d.focus("n").unwrap();
    // From empty: 0 + step.
    let ev = press(&mut d, "ArrowUp");
    assert_eq!(types(&ev), ["input", "change"]);
    assert_eq!(d.get_value("n").unwrap(), "3");
    press(&mut d, "ArrowUp");
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("n").unwrap(), "9");
    let ev = press(&mut d, "ArrowUp");
    assert!(types(&ev).is_empty(), "12 > max: stays at 9");
    assert_eq!(d.get_value("n").unwrap(), "9");
    // Step mismatch snaps to the next valid value.
    d.set_value("n", "4").unwrap();
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("n").unwrap(), "6");
    d.set_value("n", "4").unwrap();
    press(&mut d, "ArrowDown");
    assert_eq!(d.get_value("n").unwrap(), "3");
    // step=any steps by 1.
    d.focus("m").unwrap();
    press(&mut d, "ArrowDown");
    assert_eq!(d.get_value("m").unwrap(), "-1");
    d.focus("ro").unwrap();
    assert!(types(&press(&mut d, "ArrowUp")).is_empty());
    assert_eq!(d.get_value("ro").unwrap(), "4");
}

#[test]
fn number_wheel_never_changes_the_value() {
    let mut d = page("<input id=n type=number value=5>");
    d.focus("n").unwrap();
    let (x, y, _, _) = crect(&mut d, "n");
    d.handle_input(Input::Wheel {
        dx: 0.0,
        dy: -40.0,
        x: x + 3.0,
        y: y + 3.0,
    })
    .unwrap();
    assert_eq!(d.get_value("n").unwrap(), "5");
}

#[test]
fn number_accepts_only_html_floating_point_syntax() {
    let mut d = page(
        "<form id=f><input id=a type=number><input id=c type=number min=1 max=3 step=2></form>",
    );
    d.set_value("a", "1,5").unwrap();
    assert_eq!(
        d.get_value("a").unwrap(),
        "",
        "locale separators are sanitised away"
    );
    d.set_value("a", "1.5e2").unwrap();
    assert_eq!(d.get_value("a").unwrap(), "1.5e2");
    d.set_value("a", "+3").unwrap();
    assert_eq!(
        d.get_value("a").unwrap(),
        "",
        "no leading plus in HTML floats"
    );
    d.set_value("c", "0").unwrap();
    let f = flag_names(&mut d, "c");
    assert!(f.contains(&"rangeUnderflow".to_owned()) && f.contains(&"stepMismatch".to_owned()));
    d.set_value("c", "5").unwrap();
    assert!(flag_names(&mut d, "c").contains(&"rangeOverflow".to_owned()));
    d.set_value("c", "3").unwrap();
    assert!(flag_names(&mut d, "c").is_empty());
}

#[test]
fn number_invalid_events_fire_on_submit() {
    let mut d = page(
        "<form id=f><input id=n type=number min=2 max=4 value=9><button id=go>Go</button></form>",
    );
    let ev = click_el(&mut d, "go");
    let inv: Vec<_> = ev.iter().filter(|e| e["type"] == "invalid").collect();
    assert_eq!(inv.len(), 1);
    assert_eq!(inv[0]["element"], "n");
    assert_eq!(inv[0]["validity"]["flags"]["rangeOverflow"], true);
    assert!(ev.iter().all(|e| e["type"] != "submit"));
}

#[test]
fn form_reset_restores_range_and_number_defaults() {
    let mut d = page(
        "<form id=f><input id=r type=range min=0 max=10 value=3><input id=n type=number value=7>\
         <input id=e type=range min=0 max=10><button id=rs type=reset>R</button></form>",
    );
    d.set_value("r", "9").unwrap();
    d.set_value("n", "1").unwrap();
    d.set_value("e", "2").unwrap();
    click_el(&mut d, "rs");
    assert_eq!(d.get_value("r").unwrap(), "3");
    assert_eq!(d.get_value("n").unwrap(), "7");
    // Default range with no value attribute: the midpoint.
    d.focus("e").unwrap();
    press(&mut d, "ArrowUp");
    assert_eq!(d.get_value("e").unwrap(), "6");
}

#[test]
fn progress_and_meter_values_and_accessibility() {
    let mut d = page(
        "<progress id=p1 value=30 max=60></progress><progress id=p2></progress>\
         <progress id=p3 value=-2></progress><progress id=p4 value=9 max=3></progress>\
         <progress id=p5 value=1 max=0></progress>\
         <meter id=m1 min=0 max=10 low=3 high=7 optimum=9 value=8></meter>\
         <meter id=m2 min=10 max=5 value=20></meter>\
         <meter id=m3 low=0.2 high=0.8 optimum=0.5 value=2></meter>",
    );
    {
        let doc = d.pipeline.document();
        let ratio = |id: &str| doc.progress_ratio(doc.element_by_id(id).unwrap());
        assert_eq!(ratio("p1"), Some(0.5));
        assert_eq!(ratio("p2"), None, "indeterminate");
        assert_eq!(ratio("p3"), None, "negative is invalid");
        assert_eq!(ratio("p4"), Some(1.0), "clamped to max");
        assert_eq!(ratio("p5"), Some(1.0), "max<=0 means 1");
        use ui4me_document::control_values::MeterRegionKind as K;
        let meter = |id: &str| doc.meter_state(doc.element_by_id(id).unwrap()).unwrap();
        assert_eq!(meter("m1").1, K::Optimum);
        assert_eq!(meter("m2").0, 0.0, "max<min -> max=min");
        assert_eq!(
            meter("m3"),
            (1.0, K::Suboptimal),
            "value clamped to max 1; above high"
        );
    }
    let t = d.accessibility_tree().unwrap();
    let n = |id: &str| node(&t, id).numeric.unwrap();
    assert_eq!(node(&t, "p1").role, Role::ProgressIndicator);
    assert_eq!(
        (n("p1").now, n("p1").min, n("p1").max),
        (Some(30.0), Some(0.0), Some(60.0))
    );
    assert_eq!(n("p2").now, None);
    assert_eq!(n("p3").now, None);
    assert_eq!(n("p4").now, Some(3.0));
    assert_eq!(n("p5").max, Some(1.0));
    assert_eq!(node(&t, "m1").role, Role::Meter);
    assert_eq!(
        (n("m1").now, n("m1").min, n("m1").max),
        (Some(8.0), Some(0.0), Some(10.0))
    );
    assert_eq!((n("m2").now, n("m2").max), (Some(10.0), Some(10.0)));
}

#[test]
fn slider_and_spinbutton_expose_values_and_actions() {
    let mut d = page(
        "<input id=r type=range min=0 max=10 step=2 value=4 aria-label=Vol>\
         <input id=n type=number min=1 max=5 value=2 aria-label=Qty>\
         <input id=nro type=number value=2 readonly aria-label=Fixed>\
         <input id=rro type=range readonly aria-label=Rr>",
    );
    let t = d.accessibility_tree().unwrap();
    let r = node(&t, "r");
    assert_eq!(r.role, Role::Slider);
    let rn = r.numeric.unwrap();
    assert_eq!(
        (rn.now, rn.min, rn.max, rn.step),
        (Some(4.0), Some(0.0), Some(10.0), Some(2.0))
    );
    let nn = node(&t, "n");
    assert_eq!(nn.role, Role::SpinButton);
    assert_eq!(nn.numeric.unwrap().now, Some(2.0));
    assert!(nn.actions.contains(&crate::a11y::A11yAction::Increment));
    assert!(node(&t, "nro").states.read_only);
    assert!(
        !node(&t, "nro")
            .actions
            .contains(&crate::a11y::A11yAction::Increment)
    );
    assert!(
        node(&t, "rro")
            .actions
            .contains(&crate::a11y::A11yAction::Increment)
    );
    // SetValue / Increment through the accessibility request path.
    let id = d.node("r").unwrap();
    d.accessibility_request(&crate::a11y::A11yRequest {
        target: Some(id),
        kind: crate::a11y::A11yRequestKind::SetNumericValue(7.0),
    })
    .unwrap();
    assert_eq!(d.get_value("r").unwrap(), "8", "snapped to the step");
    d.accessibility_request(&crate::a11y::A11yRequest {
        target: Some(id),
        kind: crate::a11y::A11yRequestKind::Increment,
    })
    .unwrap();
    assert_eq!(d.get_value("r").unwrap(), "10");
}
