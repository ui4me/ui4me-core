//! F0808: the HTML `popover` subset through the production facade (invokers, light dismiss, Escape,
//! nesting, events, focus, top-layer stacking, removal, accessibility).
use super::*;

fn key(key: &str) -> Input {
    Input::Key {
        key: key.into(),
        code: key.into(),
        pressed: true,
        repeat: false,
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    }
}
fn ptr(d: &mut Document, pressed: bool, x: f64, y: f64) {
    d.handle_input(Input::PointerButton {
        pressed,
        button: 0,
        x,
        y,
    })
    .unwrap();
}
fn click_at(d: &mut Document, x: f64, y: f64) {
    ptr(d, true, x, y);
    ptr(d, false, x, y);
}
fn center(d: &mut Document, el: &str) -> (f64, f64) {
    let r = d.element_rect(el).unwrap().unwrap();
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}
fn click(d: &mut Document, el: &str) {
    d.render(400, 300, 1.0).unwrap();
    let (x, y) = center(d, el);
    click_at(d, x, y);
}
fn open(d: &Document, el: &str) -> bool {
    d.popover_state(el).unwrap().0
}
fn focused(d: &Document) -> String {
    d.interaction
        .focused_element(d.id())
        .and_then(|n| d.pipeline.document().get_attribute(n, "id").ok().flatten())
        .unwrap_or("")
        .to_owned()
}
fn page(body: &str) -> Document {
    let mut d = Document::parse(&format!(
        "<!doctype html><style>body{{margin:0}} button{{display:block;width:60px;height:20px}} \
         [popover]{{width:100px;height:40px;padding:0;border:0;background:#f00}} \
         #a,#b,#c{{inset:auto;margin:0}} #a{{left:0;top:100px}} #b{{left:120px;top:100px}} #c{{left:240px;top:100px}}</style>{body}"
    ))
    .unwrap();
    d.render(400, 300, 1.0).unwrap();
    d
}
fn rule(element: &str, event: &str) -> DefaultActionRule {
    DefaultActionRule {
        element: element.into(),
        event: event.into(),
        key: None,
        shift: None,
        ctrl: None,
        alt: None,
        meta: None,
    }
}
fn toggles(events: &[Value]) -> Vec<(String, String, String)> {
    events
        .iter()
        .filter(|e| e["type"] == "toggle" || e["type"] == "beforetoggle")
        .map(|e| {
            (
                e["type"].as_str().unwrap().to_owned(),
                e["old_state"].as_str().unwrap_or("").to_owned(),
                e["new_state"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

#[test]
fn invoker_toggles_shows_and_hides() {
    let mut d = page(
        "<button id=t popovertarget=a>T</button><button id=s popovertarget=a popovertargetaction=show>S</button>\
         <button id=h popovertarget=a popovertargetaction=hide>H</button><div id=a popover>A</div>",
    );
    assert_eq!(d.popover_state("a").unwrap(), (false, "auto"));
    click(&mut d, "t");
    assert!(open(&d, "a"));
    click(&mut d, "h");
    assert!(!open(&d, "a"));
    click(&mut d, "h");
    assert!(!open(&d, "a"), "hide on a closed popover is a no-op");
    click(&mut d, "s");
    assert!(open(&d, "a"));
    click(&mut d, "s");
    assert!(open(&d, "a"), "show on an open popover is a no-op");
    click(&mut d, "t");
    assert!(!open(&d, "a"), "toggle hides an open popover");
    // Facade commands.
    d.show_popover("a").unwrap();
    assert!(open(&d, "a"));
    assert!(!d.toggle_popover("a").unwrap());
    assert!(d.toggle_popover("a").unwrap());
    d.hide_popover("a").unwrap();
    assert!(!open(&d, "a"));
    assert!(d.show_popover("t").is_err(), "not a popover");
    assert!(d.popover_state("t").is_err());
}

#[test]
fn popover_kinds_and_open_pseudo_class() {
    let mut d = page(
        "<div id=a popover>A</div><div id=b popover=manual>B</div><div id=c popover=bogus>C</div>\
         <div id=plain>P</div>",
    );
    assert_eq!(d.popover_state("a").unwrap().1, "auto");
    assert_eq!(d.popover_state("b").unwrap().1, "manual");
    assert_eq!(
        d.popover_state("c").unwrap().1,
        "manual",
        "invalid value is manual"
    );
    let matching = |d: &Document| d.query_ids(":popover-open", 10).unwrap().0;
    assert!(matching(&d).is_empty());
    d.show_popover("a").unwrap();
    assert_eq!(matching(&d), ["a"], ":popover-open matches while showing");
    d.hide_popover("a").unwrap();
    assert!(matching(&d).is_empty());
    assert_eq!(
        d.element_rect("a").unwrap(),
        None,
        "closed popover is display:none"
    );
}

#[test]
fn light_dismiss_by_outside_click_and_escape_but_not_manual() {
    let mut d = page(
        "<button id=t popovertarget=a>T</button><p id=out style='margin:0;height:30px'>out</p>\
         <div id=a popover>A</div><div id=b popover=manual>B</div>",
    );
    d.show_popover("b").unwrap();
    d.show_popover("a").unwrap();
    assert!(open(&d, "a") && open(&d, "b"), "opening auto leaves manual");
    d.render(400, 300, 1.0).unwrap();
    // A click inside the popover keeps it.
    let (x, y) = center(&mut d, "a");
    click_at(&mut d, x, y);
    assert!(open(&d, "a"));
    // A click outside hides auto popovers only.
    click(&mut d, "out");
    assert!(!open(&d, "a"));
    assert!(open(&d, "b"), "manual popovers are not light dismissed");
    // Escape: closes an auto popover, never a manual one.
    d.show_popover("a").unwrap();
    d.handle_input(key("Escape")).unwrap();
    assert!(!open(&d, "a"));
    d.handle_input(key("Escape")).unwrap();
    assert!(open(&d, "b"), "Escape leaves manual popovers");
    // A press that starts inside and is released outside does not dismiss.
    d.show_popover("a").unwrap();
    d.render(400, 300, 1.0).unwrap();
    let (x, y) = center(&mut d, "a");
    let (ox, oy) = center(&mut d, "out");
    ptr(&mut d, true, x, y);
    ptr(&mut d, false, ox, oy);
    assert!(open(&d, "a"));
}

#[test]
fn nested_auto_popovers_follow_invoker_and_dom_ancestry() {
    let mut d = page(
        "<button id=t popovertarget=a>T</button>\
         <div id=a popover><button id=ib popovertarget=b>in</button></div>\
         <div id=b popover>B<div id=c popover>C</div></div>\
         <div id=u popover>U</div>",
    );
    d.show_popover("a").unwrap();
    // Invoker inside A opens B: A (the invoker's popover) stays.
    d.render(400, 300, 1.0).unwrap();
    click(&mut d, "ib");
    assert!(open(&d, "a") && open(&d, "b"));
    // C is a DOM descendant of B: B stays open.
    d.show_popover("c").unwrap();
    assert!(open(&d, "a") && open(&d, "b") && open(&d, "c"));
    // Hiding A hides what is above it.
    d.hide_popover("a").unwrap();
    assert!(!open(&d, "a") && !open(&d, "b") && !open(&d, "c"));
    // An unrelated auto popover closes every non-ancestor.
    d.show_popover("a").unwrap();
    d.show_popover("u").unwrap();
    assert!(!open(&d, "a"));
    assert!(open(&d, "u"));
    d.hide_popover("u").unwrap();
    // Escape peels one level at a time.
    d.show_popover("a").unwrap();
    d.render(400, 300, 1.0).unwrap();
    click(&mut d, "ib");
    assert!(open(&d, "a") && open(&d, "b"));
    d.handle_input(key("Escape")).unwrap();
    assert!(open(&d, "a") && !open(&d, "b"));
    d.handle_input(key("Escape")).unwrap();
    assert!(!open(&d, "a"));
}

#[test]
fn toggle_events_carry_old_and_new_state_and_beforetoggle_cancels() {
    let mut d = page("<div id=a popover>A</div>");
    d.drain_events();
    let s = |a: &str, b: &str, c: &str| (a.to_owned(), b.to_owned(), c.to_owned());
    d.show_popover("a").unwrap();
    assert_eq!(
        toggles(&d.drain_events()),
        [
            s("beforetoggle", "closed", "open"),
            s("toggle", "closed", "open")
        ]
    );
    d.hide_popover("a").unwrap();
    assert_eq!(
        toggles(&d.drain_events()),
        [
            s("beforetoggle", "open", "closed"),
            s("toggle", "open", "closed")
        ]
    );
    // A canceled beforetoggle keeps it closed and fires no toggle.
    d.set_default_action(rule("a", "beforetoggle"), true)
        .unwrap();
    d.show_popover("a").unwrap();
    assert!(!open(&d, "a"));
    let t = toggles(&d.drain_events());
    assert!(t.iter().all(|e| e.0 == "beforetoggle"), "{t:?}");
    // Closing cannot be canceled.
    d.set_default_action(rule("a", "beforetoggle"), false)
        .unwrap();
    d.show_popover("a").unwrap();
    d.set_default_action(rule("a", "beforetoggle"), true)
        .unwrap();
    d.hide_popover("a").unwrap();
    assert!(!open(&d, "a"));
}

#[test]
fn focus_moves_to_autofocus_and_returns_on_hide() {
    let mut d = page(
        "<button id=t popovertarget=a>T</button><button id=other>O</button>\
         <div id=a popover><button id=x>x</button><button id=y autofocus>y</button></div>\
         <div id=n popover><button id=z>z</button></div>",
    );
    d.focus("t").unwrap();
    d.show_popover("a").unwrap();
    assert_eq!(focused(&d), "y", "the autofocus descendant takes focus");
    d.handle_input(key("Escape")).unwrap();
    assert!(!open(&d, "a"));
    assert_eq!(
        focused(&d),
        "t",
        "focus returns to the previously focused element"
    );
    // No autofocus: focus stays; hiding with focus elsewhere leaves it alone.
    d.focus("other").unwrap();
    d.show_popover("n").unwrap();
    assert_eq!(focused(&d), "other");
    d.hide_popover("n").unwrap();
    assert_eq!(focused(&d), "other");
    // Focus inside the popover moves back to where it was when shown.
    d.focus("t").unwrap();
    d.show_popover("n").unwrap();
    d.focus("z").unwrap();
    d.hide_popover("n").unwrap();
    assert_eq!(focused(&d), "t");
}

#[test]
fn open_popover_paints_above_content_and_orders_with_modal_dialogs() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0}\
         #cover{position:absolute;left:0;top:0;width:400px;height:300px;z-index:50;background:#00f}\
         [popover],dialog{width:100px;height:40px;padding:0;border:0;background:#f00}\
         dialog{background:#0f0}</style>\
         <div id=cover></div><div id=p popover></div><dialog id=dlg></dialog>",
    )
    .unwrap();
    let px = |d: &mut Document| d.render(400, 300, 1.0).unwrap()[150 * 400 + 200];
    assert_eq!(px(&mut d), 0x000000ff, "closed popover paints nothing");
    d.show_popover("p").unwrap();
    assert_eq!(
        px(&mut d),
        0x00ff0000,
        "the popover is above positioned content"
    );
    // A modal dialog shown later is above the popover.
    d.show_dialog("dlg", true).unwrap();
    assert_eq!(
        px(&mut d),
        0x0000ff00,
        "the later top-layer entry is on top"
    );
    let order: Vec<_> = d.pipeline.document().top_layer().to_vec();
    assert_eq!(order.len(), 2);
    assert_eq!(d.node("p").unwrap(), order[0]);
    d.close_dialog("dlg", None).unwrap();
    d.hide_popover("p").unwrap();
    // Reverse: the popover shown after the dialog is above it, and Escape closes it first.
    d.show_dialog("dlg", true).unwrap();
    d.show_popover("p").unwrap();
    assert_eq!(px(&mut d), 0x00ff0000);
    d.handle_input(key("Escape")).unwrap();
    assert!(!open(&d, "p"));
    assert!(d.dialog_state("dlg").unwrap().0, "the dialog is still open");
    d.handle_input(key("Escape")).unwrap();
    assert!(!d.dialog_state("dlg").unwrap().0);
}

#[test]
fn removing_or_unmarking_an_open_popover_leaves_the_top_layer() {
    let mut d = page("<div id=a popover>A</div><div id=b popover>B</div>");
    d.show_popover("a").unwrap();
    assert_eq!(d.pipeline.document().top_layer().len(), 1);
    d.remove_element("a").unwrap();
    assert!(d.pipeline.document().top_layer().is_empty());
    d.show_popover("b").unwrap();
    d.set_attribute("b", "popover", None).unwrap();
    assert!(d.pipeline.document().top_layer().is_empty());
    assert!(d.show_popover("b").is_err());
    d.drain_events();
    d.render(400, 300, 1.0).unwrap();
}

#[test]
fn invoker_exposes_expanded_state_to_assistive_technology() {
    let mut d = page("<button id=t popovertarget=a>T</button><div id=a popover>A</div>");
    let expanded = |d: &mut Document| {
        let t = d.accessibility_tree().unwrap();
        t.nodes
            .values()
            .find(|n| n.html_id.as_deref() == Some("t"))
            .unwrap()
            .states
            .expanded
    };
    assert_eq!(expanded(&mut d), Some(false));
    d.show_popover("a").unwrap();
    assert_eq!(expanded(&mut d), Some(true));
    d.hide_popover("a").unwrap();
    assert_eq!(expanded(&mut d), Some(false));
}

fn xy(d: &mut Document, el: &str) -> (f64, f64) {
    let r = d.element_rect(el).unwrap().unwrap();
    (r.x, r.y)
}

#[test]
fn anchor_positioning_places_a_popover_against_named_and_implicit_anchors() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0}\
         #t{position:absolute;left:50px;top:40px;width:60px;height:20px;anchor-name:--t}\
         [popover]{width:30px;height:10px;padding:0;border:0;margin:0;inset:auto}\
         #a{top:anchor(--t bottom);left:anchor(--t left)}\
         #b{bottom:anchor(--t top);right:anchor(--t right)}\
         #c{position-anchor:--t;top:anchor(center);left:anchor(right)}\
         #i{top:anchor(bottom);left:anchor(50%)}</style>\
         <button id=t popovertarget=i>t</button><div id=a popover=manual></div><div id=b popover=manual></div>\
         <div id=c popover=manual></div><div id=i popover=manual></div>",
    )
    .unwrap();
    for id in ["a", "b", "c", "i"] {
        d.show_popover(id).unwrap();
    }
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(
        xy(&mut d, "a"),
        (50.0, 60.0),
        "top/left against the named anchor"
    );
    // bottom edge at the anchor's top (40), right edge at its right edge (110).
    assert_eq!(xy(&mut d, "b"), (80.0, 30.0));
    assert_eq!(
        xy(&mut d, "c"),
        (110.0, 50.0),
        "position-anchor + center/right"
    );
    // The popover's invoker is its implicit anchor.
    assert_eq!(xy(&mut d, "i"), (80.0, 60.0));
    // Moving the anchor repositions every popover on the next layout.
    d.set_attribute("t", "style", Some("left:150px;top:100px"))
        .unwrap();
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(xy(&mut d, "a"), (150.0, 120.0));
    assert_eq!(xy(&mut d, "i"), (180.0, 120.0));
}

#[test]
fn anchored_popover_follows_the_anchor_when_the_viewport_resizes() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0}\
         #t{position:absolute;right:20px;top:10px;width:40px;height:20px;anchor-name:--t}\
         #a{width:30px;height:10px;padding:0;border:0;margin:0;inset:auto;top:anchor(--t bottom);right:anchor(--t right)}</style>\
         <div id=t></div><div id=a popover></div>",
    )
    .unwrap();
    d.show_popover("a").unwrap();
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(xy(&mut d, "a"), (350.0, 30.0));
    d.render(300, 200, 1.0).unwrap();
    assert_eq!(
        xy(&mut d, "a"),
        (250.0, 30.0),
        "the anchor moved with the viewport"
    );
}

#[test]
fn anchored_popover_tracks_the_scroll_of_the_page_and_of_scroll_containers() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0;height:2000px}\
         #t{position:absolute;left:20px;top:100px;width:40px;height:20px;anchor-name:--t}\
         #box{position:absolute;left:100px;top:0;width:100px;height:100px;overflow:auto}\
         #inner{height:400px}#u{position:absolute;left:10px;top:150px;width:10px;height:10px;anchor-name:--u}\
         #a{width:30px;height:10px;padding:0;border:0;margin:0;inset:auto;background:#f00;top:anchor(--t bottom);left:anchor(--t left)}\
         #b{width:30px;height:10px;padding:0;border:0;margin:0;inset:auto;top:anchor(--u bottom);left:anchor(--u left)}</style>\
         <div id=t></div><div id=box><div id=inner><div id=u></div></div></div>\
         <div id=a popover=manual></div><div id=b popover=manual></div>",
    )
    .unwrap();
    d.show_popover("a").unwrap();
    d.show_popover("b").unwrap();
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(xy(&mut d, "a"), (20.0, 120.0));
    assert_eq!(xy(&mut d, "b"), (10.0 + 100.0, 160.0 + 0.0 - 0.0).clone());
    d.scroll_by(None, 0.0, 50.0).unwrap();
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(xy(&mut d, "a"), (20.0, 70.0), "follows the page scroll");
    let px = d.render(400, 300, 1.0).unwrap();
    assert_eq!(
        px[75 * 400 + 30],
        0x00ff0000,
        "painted at the followed position"
    );
    assert_ne!(px[125 * 400 + 30], 0x00ff0000, "not at the layout position");
    d.scroll_by(Some("box"), 0.0, 30.0).unwrap();
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(xy(&mut d, "a"), (20.0, 70.0));
    assert_eq!(
        xy(&mut d, "b").1,
        160.0 - 50.0 - 30.0,
        "follows the inner scroller too"
    );
}
