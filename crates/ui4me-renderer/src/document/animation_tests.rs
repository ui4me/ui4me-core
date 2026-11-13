//! 0.9 F0901/F0902: CSS transitions, keyframe animations and `@starting-style` on the
//! deterministic document clock, through the production facade.
use super::*;

const W: usize = 120;

fn doc(css: &str, body: &str) -> Document {
    let mut d = Document::parse(&format!(
        "<!doctype html><style>body{{margin:0;background:#fff}}{css}</style>{body}"
    ))
    .unwrap();
    d.use_manual_clock();
    d.render(W as u32, 80, 1.0).unwrap();
    d
}
fn px(d: &mut Document, x: usize, y: usize) -> u32 {
    d.render(W as u32, 80, 1.0).unwrap()[y * W + x] & 0x00ff_ffff
}
fn chan(c: u32, shift: u32) -> i64 {
    i64::from((c >> shift) & 0xff)
}
/// Advance in 16 ms frames, rendering each, and gather the events.
fn run(d: &mut Document, ms: u32) -> Vec<Value> {
    let mut out = Vec::new();
    let mut left = ms;
    while left > 0 {
        let step = left.min(16);
        d.advance_clock(f64::from(step)).unwrap();
        d.render(W as u32, 80, 1.0).unwrap();
        out.extend(d.drain_events());
        left -= step;
    }
    out
}
fn names(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|e| {
            e["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("transition") || t.starts_with("animation"))
        })
        .map(|e| e["type"].as_str().unwrap().to_string())
        .collect()
}
fn apply(d: &mut Document, class: &str) -> Vec<Value> {
    d.add_class("box", class).unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    d.drain_events()
}

#[test]
fn background_transition_has_intermediate_frames_and_ordered_events() {
    let mut d = doc(
        "#box{width:24px;height:24px;background:#ff0000;transition:background-color 1s linear}\
         #box.on{background:#0000ff}",
        "<div id=box></div>",
    );
    assert_eq!(d.next_wake_in(), None, "idle before any change");
    let mut ev = apply(&mut d, "on");
    assert_eq!(
        px(&mut d, 5, 5),
        0xff0000,
        "nothing moves before the clock does"
    );
    assert!(
        d.next_wake_in().is_some(),
        "a running transition asks for frames"
    );
    ev.extend(run(&mut d, 496));
    let mid = px(&mut d, 5, 5);
    assert!(
        (chan(mid, 16) - 128).abs() < 24 && (chan(mid, 0) - 128).abs() < 24,
        "halfway is a blend: {mid:06x}"
    );
    ev.extend(run(&mut d, 600));
    assert_eq!(px(&mut d, 5, 5), 0x0000ff);
    assert_eq!(
        names(&ev),
        ["transitionrun", "transitionstart", "transitionend"]
    );
    let end = ev.iter().find(|e| e["type"] == "transitionend").unwrap();
    assert_eq!(end["property_name"], "background-color");
    assert_eq!(end["elapsed_time"], 1.0);
    assert_eq!(end["pseudo_element"], "");
    assert_eq!(d.next_wake_in(), None, "no wake-ups once finished");
}

#[test]
fn opacity_and_transform_transitions_reach_their_end_values() {
    let mut d = doc(
        "#box{width:24px;height:24px;background:#000;transition:opacity 1s linear,transform 1s linear}\
         #box.on{opacity:0.5;transform:translateX(60px)}",
        "<div id=box></div>",
    );
    apply(&mut d, "on");
    run(&mut d, 496);
    // Half way the box is at x = 30..54.
    assert_eq!(px(&mut d, 10, 5), 0xffffff);
    let moved = px(&mut d, 40, 5);
    assert!(
        chan(moved, 0) > 20 && chan(moved, 0) < 120,
        "translucent in flight: {moved:06x}"
    );
    run(&mut d, 600);
    assert_eq!(px(&mut d, 5, 5), 0xffffff);
    let end = px(&mut d, 70, 5);
    assert!(
        (chan(end, 0) - 128).abs() < 3,
        "opacity .5 over white: {end:06x}"
    );
    assert_eq!(d.next_wake_in(), None);
}

#[test]
fn width_transition_relayouts_every_frame() {
    let mut d = doc(
        "#box{width:20px;height:20px;background:#000;transition:width 1s linear}#box.on{width:80px}",
        "<div id=box></div>",
    );
    assert_eq!(px(&mut d, 50, 5), 0xffffff);
    apply(&mut d, "on");
    run(&mut d, 496);
    assert_eq!(px(&mut d, 40, 5), 0, "width ~50 at half time");
    assert_eq!(px(&mut d, 70, 5), 0xffffff);
    run(&mut d, 600);
    assert_eq!(px(&mut d, 75, 5), 0);
    assert_eq!(px(&mut d, 90, 5), 0xffffff);
}

#[test]
fn a_style_change_cancels_and_restarts_a_transition() {
    let mut d = doc(
        "#box{width:20px;height:20px;background:#000;transition:width 1s linear}#box.on{width:80px}",
        "<div id=box></div>",
    );
    let mut ev = apply(&mut d, "on");
    ev.extend(run(&mut d, 496));
    d.remove_class("box", "on").unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    ev.extend(d.drain_events());
    ev.extend(run(&mut d, 2000));
    assert_eq!(
        names(&ev),
        [
            "transitionrun",
            "transitionstart",
            "transitioncancel",
            "transitionrun",
            "transitionstart",
            "transitionend"
        ]
    );
    assert_eq!(px(&mut d, 30, 5), 0xffffff, "back at 20px");
    assert_eq!(px(&mut d, 10, 5), 0);
    assert_eq!(d.next_wake_in(), None);
}

#[test]
fn keyframes_iterate_alternate_fill_forwards() {
    let mut d = doc(
        "@keyframes grow{from{width:10px}to{width:50px}}\
         #box{width:20px;height:20px;background:#000}\
         #box.go{animation:grow 1s linear 3 alternate forwards}",
        "<div id=box></div>",
    );
    let mut ev = apply(&mut d, "go");
    ev.extend(run(&mut d, 1496));
    // 1.5 s: second iteration (reverse) halfway = width 30.
    assert_eq!(px(&mut d, 25, 5), 0);
    assert_eq!(px(&mut d, 35, 5), 0xffffff);
    ev.extend(run(&mut d, 2000));
    assert_eq!(
        names(&ev),
        [
            "animationstart",
            "animationiteration",
            "animationiteration",
            "animationend"
        ]
    );
    let end = ev.iter().find(|e| e["type"] == "animationend").unwrap();
    assert_eq!(end["animation_name"], "grow");
    assert_eq!(end["elapsed_time"], 3.0);
    // The third iteration runs forwards and `forwards` keeps the end keyframe: 50px.
    assert_eq!(px(&mut d, 45, 5), 0);
    assert_eq!(px(&mut d, 55, 5), 0xffffff);
    assert_eq!(d.next_wake_in(), None, "a held fill needs no ticks");
}

#[test]
fn without_fill_the_element_returns_to_its_style() {
    let mut d = doc(
        "@keyframes grow{from{width:10px}to{width:50px}}\
         #box{width:20px;height:20px;background:#000}#box.go{animation:grow 1s linear}",
        "<div id=box></div>",
    );
    apply(&mut d, "go");
    run(&mut d, 2000);
    assert_eq!(px(&mut d, 15, 5), 0);
    assert_eq!(px(&mut d, 25, 5), 0xffffff, "back to 20px");
}

#[test]
fn delays_and_fill_backwards_apply_before_the_start() {
    let mut d = doc(
        "@keyframes grow{from{width:10px}to{width:50px}}\
         #box{width:20px;height:20px;background:#000}\
         #box.go{animation:grow 1s linear 500ms backwards}",
        "<div id=box></div>",
    );
    let mut ev = apply(&mut d, "go");
    assert_eq!(
        px(&mut d, 15, 5),
        0xffffff,
        "backwards fill shows the first keyframe: 10px"
    );
    ev.extend(run(&mut d, 400));
    assert!(names(&ev).is_empty(), "still in the delay");
    ev.extend(run(&mut d, 200));
    assert_eq!(names(&ev), ["animationstart"]);
}

#[test]
fn removing_an_animating_element_stops_everything() {
    let mut d = doc(
        "@keyframes spin{from{opacity:1}to{opacity:.2}}\
         #box{width:20px;height:20px;background:#000;animation:spin 1s infinite}",
        "<div id=box></div><div id=other></div>",
    );
    run(&mut d, 100);
    assert!(d.is_css_animating());
    assert!(d.next_wake_in().is_some());
    d.remove_element("box").unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    let ev = d.drain_events();
    assert!(
        !ev.iter()
            .any(|e| e["type"] == "animationcancel" && e["element"] == "box")
    );
    assert!(
        !d.is_css_animating(),
        "the animation is gone with its element"
    );
    assert_eq!(d.next_wake_in(), None);
    assert!(run(&mut d, 500).is_empty());
}

#[test]
fn display_none_cancels_the_animation() {
    let mut d = doc(
        "@keyframes spin{from{opacity:1}to{opacity:.2}}\
         #box{width:20px;height:20px;background:#000;animation:spin 1s infinite}#box.gone{display:none}",
        "<div id=box></div>",
    );
    run(&mut d, 100);
    let mut ev = apply(&mut d, "gone");
    ev.extend(run(&mut d, 50));
    assert!(
        names(&ev).contains(&"animationcancel".to_string()),
        "{ev:?}"
    );
    assert_eq!(d.next_wake_in(), None);
}

#[test]
fn an_infinite_animation_is_paced_not_busy() {
    let mut d = doc(
        "@keyframes spin{from{opacity:1}to{opacity:.2}}\
         #box{width:20px;height:20px;background:#000;animation:spin 200ms infinite}",
        "<div id=box></div>",
    );
    let mut starts = 0;
    let mut iterations = 0;
    for e in run(&mut d, 1000) {
        match e["type"].as_str() {
            Some("animationstart") => starts += 1,
            Some("animationiteration") => iterations += 1,
            _ => {}
        }
    }
    assert_eq!(starts, 1);
    assert!(
        (4..=5).contains(&iterations),
        "{iterations} iterations in a second"
    );
    assert_eq!(d.next_wake_in(), Some(Duration::from_millis(16)));
    // Polling without a clock move is not a tick: a host loop spinning here costs nothing.
    let before = d.pipeline.clock().ticks();
    for _ in 0..50 {
        d.poll_resources().unwrap();
    }
    assert_eq!(d.pipeline.clock().ticks(), before);
}

#[test]
fn an_idle_document_requests_no_wakeups() {
    let mut d = doc(
        "#box{width:20px;height:20px;background:#000;transition:opacity 1s}#box.on{opacity:0}",
        "<div id=box></div>",
    );
    assert_eq!(d.next_wake_in(), None);
    assert!(!d.is_css_animating());
    d.advance_clock(1000.0).unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    assert_eq!(d.next_wake_in(), None);
    assert!(!d.poll_resources().unwrap());
}

#[test]
fn reduced_motion_media_query_removes_the_motion() {
    let css = "#box{width:20px;height:20px;background:#000;transition:width 1s linear}#box.on{width:80px}\
               @media (prefers-reduced-motion: reduce){#box{transition:none}}";
    let mut d = doc(css, "<div id=box></div>");
    d.set_app_environment(None, Some(Some(true))).unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    let ev = apply(&mut d, "on");
    assert_eq!(px(&mut d, 70, 5), 0, "instant");
    assert!(names(&ev).is_empty());
    assert_eq!(d.next_wake_in(), None);
    // The same sheet animates when motion is not reduced.
    d.set_app_environment(None, Some(Some(false))).unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    d.remove_class("box", "on").unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    assert_eq!(px(&mut d, 70, 5), 0, "width starts shrinking from 80px");
    assert!(d.next_wake_in().is_some());
}

// ---- F0902: @starting-style and timing functions ------------------------------------------------

const ITEM_CSS: &str = ".item{width:40px;height:20px;background:#000;opacity:1;transition:opacity 1s linear}\
     @starting-style{.item{opacity:0}}";

fn insert_item(d: &mut Document) -> Vec<Value> {
    d.insert_html("holder", "beforeend", "<div id=item class=item></div>")
        .unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    d.drain_events()
}

#[test]
fn starting_style_transitions_an_inserted_element() {
    let mut d = doc(ITEM_CSS, "<div id=holder></div>");
    let mut ev = insert_item(&mut d);
    assert_eq!(
        px(&mut d, 5, 5),
        0xffffff,
        "starts from the @starting-style opacity 0"
    );
    assert!(d.next_wake_in().is_some());
    ev.extend(run(&mut d, 496));
    let mid = px(&mut d, 5, 5);
    assert!((chan(mid, 0) - 128).abs() < 24, "half way: {mid:06x}");
    ev.extend(run(&mut d, 600));
    assert_eq!(px(&mut d, 5, 5), 0);
    assert_eq!(
        names(&ev),
        ["transitionrun", "transitionstart", "transitionend"]
    );
    assert_eq!(d.next_wake_in(), None);
}

#[test]
fn a_newly_displayed_element_uses_its_starting_style() {
    let mut d = doc(
        ".item{width:40px;height:20px;background:#000;opacity:1;transition:opacity 1s linear}\
         .item.hide{display:none}@starting-style{.item{opacity:0}}",
        "<div id=item class='item hide'></div>",
    );
    d.remove_class("item", "hide").unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    assert_eq!(px(&mut d, 5, 5), 0xffffff);
    run(&mut d, 1100);
    assert_eq!(px(&mut d, 5, 5), 0);
}

#[test]
fn a_later_normal_rule_overrides_the_starting_style() {
    let mut d = doc(
        &format!("{ITEM_CSS}.item{{opacity:1}}"),
        "<div id=holder></div>",
    );
    let ev = insert_item(&mut d);
    assert_eq!(
        px(&mut d, 5, 5),
        0,
        "the contradictory later rule wins: no transition"
    );
    assert!(names(&ev).is_empty());
    assert_eq!(d.next_wake_in(), None);
}

#[test]
fn starting_style_without_a_transition_applies_at_once() {
    let mut d = doc(
        ".item{width:40px;height:20px;background:#000;opacity:1}@starting-style{.item{opacity:0}}",
        "<div id=holder></div>",
    );
    insert_item(&mut d);
    assert_eq!(px(&mut d, 5, 5), 0);
    assert_eq!(d.next_wake_in(), None);
}

#[test]
fn removing_a_transitioning_element_leaves_no_task() {
    let mut d = doc(ITEM_CSS, "<div id=holder></div>");
    insert_item(&mut d);
    run(&mut d, 100);
    assert!(d.is_css_animating());
    d.remove_element("item").unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    assert!(!d.is_css_animating());
    assert_eq!(d.next_wake_in(), None);
    assert!(
        run(&mut d, 1200).is_empty(),
        "no event for a removed element"
    );
    // And a reinsert starts afresh.
    insert_item(&mut d);
    assert_eq!(px(&mut d, 5, 5), 0xffffff);
    run(&mut d, 1100);
    assert_eq!(px(&mut d, 5, 5), 0);
}

#[test]
fn starting_style_respects_reduced_motion_sheets() {
    let mut d = doc(
        &format!("{ITEM_CSS}@media (prefers-reduced-motion: reduce){{.item{{transition:none}}}}"),
        "<div id=holder></div>",
    );
    d.set_app_environment(None, Some(Some(true))).unwrap();
    d.render(W as u32, 80, 1.0).unwrap();
    let ev = insert_item(&mut d);
    assert_eq!(px(&mut d, 5, 5), 0, "no transition when motion is reduced");
    assert!(names(&ev).is_empty());
    assert_eq!(d.next_wake_in(), None);
}

fn width_at(timing: &str, ms: u32) -> usize {
    let mut d = doc(
        &format!(
            "#box{{width:0;height:20px;background:#000;transition:width 1s {timing}}}#box.on{{width:100px}}"
        ),
        "<div id=box></div>",
    );
    apply(&mut d, "on");
    run(&mut d, ms);
    (0..W).take_while(|&x| px(&mut d, x, 5) == 0).count()
}

#[test]
fn timing_functions_steps_cubic_bezier_and_linear() {
    // steps(4, end): a quarter of the way after 250 ms, and the value holds in between.
    assert_eq!(width_at("steps(4, end)", 240), 0);
    assert_eq!(width_at("steps(4, end)", 496), 25);
    assert_eq!(width_at("steps(4, jump-start)", 16), 25);
    // ease-in is slower than linear at half time; cubic-bezier(0,0,1,1) is linear.
    let ease_in = width_at("ease-in", 496);
    assert!((15..40).contains(&ease_in), "{ease_in}");
    let linear = width_at("cubic-bezier(0,0,1,1)", 496);
    assert!((48..=52).contains(&linear), "{linear}");
    // linear() easing: a plateau at 10% until half time.
    let piecewise = width_at("linear(0, 0.1 50%, 1)", 496);
    assert!((8..=12).contains(&piecewise), "{piecewise}");
}

/// An infinite keyframe animation removed by `prefers-reduced-motion: reduce` must stop asking
/// for frames once the preference is applied; otherwise an idle window keeps repainting (the
/// reference application's status pulse cost a third of a core with motion reduced).
#[test]
fn reduced_motion_media_query_stops_animation_wakeups() {
    const CSS: &str = "#p{display:inline-block;width:6px;height:6px;background:#0a0;\
         animation:pulse 2s ease-in-out infinite}\
         @keyframes pulse{0%,100%{opacity:1}50%{opacity:.3}}\
         @media (prefers-reduced-motion: reduce){#p{animation:none}}";
    for manual in [true, false] {
        let mut d = Document::parse(&format!(
            "<!doctype html><style>body{{margin:0;background:#fff}}{CSS}</style><span id=p></span>"
        ))
        .unwrap();
        if manual {
            d.use_manual_clock();
        }
        d.render(W as u32, 80, 1.0).unwrap();
        assert!(
            d.next_wake_in().is_some(),
            "manual={manual}: the pulse animates"
        );
        // An unrelated environment change also rebuilds the stylist; the listed animation
        // must survive it.
        assert!(d.set_app_environment(Some(Some(true)), None).unwrap());
        d.render(W as u32, 80, 1.0).unwrap();
        assert!(
            d.next_wake_in().is_some(),
            "manual={manual}: a dark-mode change keeps the pulse"
        );
        assert!(d.set_app_environment(None, Some(Some(true))).unwrap());
        for _ in 0..3 {
            if manual {
                d.advance_clock(16.0).unwrap();
            }
            d.render(W as u32, 80, 1.0).unwrap();
            d.drain_events();
        }
        assert_eq!(
            d.next_wake_in(),
            None,
            "manual={manual}: no animation remains after reduced motion"
        );
        assert!(!d.needs_repaint(), "manual={manual}: nothing left to paint");
        assert!(d.set_app_environment(None, Some(Some(false))).unwrap());
        d.render(W as u32, 80, 1.0).unwrap();
        assert!(
            d.next_wake_in().is_some(),
            "manual={manual}: motion allowed again restarts the pulse"
        );
    }
}

/// Core 1.0 C12 (found natively on KDE): removing the attached stylesheet that started an
/// `infinite` animation left the animation running, so the host kept repainting at ~60 Hz
/// (about 36 % of a core) until an unrelated environment change restyled the document.
/// Detaching or replacing the sheet must cancel the animation and return the document to idle.
#[test]
fn detaching_or_replacing_the_animating_sheet_returns_to_idle() {
    let mut d = Document::parse_hosted("<button id=b>spin</button>", None, 300, 120).unwrap();
    d.render(300, 120, 1.0).unwrap();
    assert!(!d.is_css_animating());
    let css = "@keyframes spin{from{transform:rotate(0)}to{transform:rotate(360deg)}} #b{animation:spin 1s linear infinite}";
    let sheet = d.add_stylesheet(css).unwrap()["sheet"].as_u64().unwrap();
    d.render(300, 120, 1.0).unwrap();
    assert!(
        d.is_css_animating(),
        "the attached sheet starts the animation"
    );
    d.remove_stylesheet(sheet).unwrap();
    d.render(300, 120, 1.0).unwrap();
    assert!(
        !d.is_css_animating(),
        "detaching the sheet must stop the animation"
    );
    let sheet = d.add_stylesheet(css).unwrap()["sheet"].as_u64().unwrap();
    d.render(300, 120, 1.0).unwrap();
    assert!(d.is_css_animating());
    d.replace_stylesheet(sheet, "#b{color:red}").unwrap();
    d.render(300, 120, 1.0).unwrap();
    assert!(
        !d.is_css_animating(),
        "replacing the sheet must stop the animation"
    );
    // A document `<style>` whose text changes behaves the same way.
    let mut d = Document::parse_hosted(
        &format!("<style id=s>{css}</style><button id=b>spin</button>"),
        None,
        300,
        120,
    )
    .unwrap();
    d.render(300, 120, 1.0).unwrap();
    assert!(d.is_css_animating());
    d.set_text("s", "#b{color:red}").unwrap();
    d.render(300, 120, 1.0).unwrap();
    assert!(
        !d.is_css_animating(),
        "replacing <style> text must stop the animation"
    );
    let transform = d
        .computed_style(Some("b"), None, &["transform".into()])
        .unwrap();
    assert_eq!(transform["properties"]["transform"]["value"], "none");
}
