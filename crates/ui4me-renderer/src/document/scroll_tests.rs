//! 0.8 F0803/F0804: smooth scrolling on a deterministic clock, `scroll-behavior`, scroll snapping,
//! `scroll-padding`/`scroll-margin` and `overscroll-behavior` through the production facade.
use super::scroll_anim::choose;
use super::*;

/// A 200x200 scroller with six 150px sections (max scroll 700).
fn page(box_css: &str, section_css: &str) -> Document {
    let sections: String = (0..6)
        .map(|i| format!("<div class=s id=s{i}>{i}</div>"))
        .collect();
    let mut d = Document::parse(&format!(
        "<!doctype html><style>body{{margin:0}}#box{{width:200px;height:200px;overflow:auto;{box_css}}}\
         .s{{height:150px;{section_css}}}</style><div id=box>{sections}</div>"
    ))
    .unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    d
}
fn y(d: &mut Document) -> f64 {
    d.scroll_state(Some("box")).unwrap().y
}
fn x(d: &mut Document) -> f64 {
    d.scroll_state(Some("box")).unwrap().x
}
fn wheel(d: &mut Document, dy: f64) {
    d.handle_input(Input::Wheel {
        dx: 0.0,
        dy,
        x: 50.0,
        y: 50.0,
    })
    .unwrap();
}
fn count(events: &[Value], kind: &str) -> usize {
    events.iter().filter(|e| e["type"] == kind).count()
}

#[test]
fn smooth_scroll_reaches_the_target_exactly_with_monotonic_steps() {
    let mut d = page("scroll-behavior:smooth", "");
    let s = d.scroll_to(Some("box"), None, Some(500.0)).unwrap();
    assert!(s.animating, "a smooth request starts an animation");
    assert_eq!(s.y, 0.0, "nothing moves before the clock does");
    let mut last = 0.0;
    let mut steps = 0;
    for _ in 0..40 {
        d.advance_clock(16.0).unwrap();
        let s = d.scroll_state(Some("box")).unwrap();
        assert!(s.y >= last, "monotonic: {} after {last}", s.y);
        last = s.y;
        steps += 1;
        if !s.animating {
            break;
        }
    }
    assert_eq!(last, 500.0, "lands exactly on the target");
    assert!(steps > 3 && steps < 40, "takes several frames: {steps}");
    assert!(!d.is_scroll_animating(Some("box")));
    assert_eq!(d.next_wake_in(), None, "no wake-ups once finished");
    let ev = d.drain_events();
    assert!(count(&ev, "scroll") >= 3, "scroll fires per step");
}

#[test]
fn intermediate_positions_are_reported_and_the_host_is_asked_for_frames() {
    let mut d = page("", "");
    d.scroll_to_behavior(Some("box"), None, Some(300.0), "smooth")
        .unwrap();
    assert_eq!(d.next_wake_in(), Some(Duration::from_millis(16)));
    d.advance_clock(100.0).unwrap();
    let s = d.scroll_state(Some("box")).unwrap();
    assert!(
        s.animating && s.y > 0.0 && s.y < 300.0,
        "mid-flight: {}",
        s.y
    );
    assert!(d.needs_repaint());
}

#[test]
fn instant_and_auto_without_css_move_at_once() {
    let mut d = page("", "");
    let s = d.scroll_to(Some("box"), None, Some(120.0)).unwrap();
    assert_eq!((s.y, s.animating), (120.0, false));
    let s = d
        .scroll_to_behavior(Some("box"), None, Some(200.0), "instant")
        .unwrap();
    assert_eq!((s.y, s.animating), (200.0, false));
    let mut d = page("scroll-behavior:smooth", "");
    let s = d
        .scroll_to_behavior(Some("box"), None, Some(200.0), "instant")
        .unwrap();
    assert_eq!(
        (s.y, s.animating),
        (200.0, false),
        "instant overrides the CSS"
    );
    assert!(
        d.scroll_to_behavior(Some("box"), None, Some(1.0), "slow")
            .is_err()
    );
}

#[test]
fn a_new_request_replaces_the_running_one() {
    let mut d = page("", "");
    d.scroll_to_behavior(Some("box"), None, Some(600.0), "smooth")
        .unwrap();
    d.advance_clock(100.0).unwrap();
    let mid = y(&mut d);
    d.scroll_to_behavior(Some("box"), None, Some(50.0), "smooth")
        .unwrap();
    assert_eq!(d.scroll_anims.list.len(), 1, "one animation per container");
    for _ in 0..40 {
        d.advance_clock(16.0).unwrap();
    }
    assert!(mid > 0.0);
    assert_eq!(y(&mut d), 50.0);
    // scrollBy is relative to the current position, also while animating.
    d.scroll_by_behavior(Some("box"), 0.0, 100.0, "smooth")
        .unwrap();
    d.advance_clock(60.0).unwrap();
    d.scroll_by_behavior(Some("box"), 0.0, 100.0, "smooth")
        .unwrap();
    assert_eq!(d.scroll_anims.list.len(), 1);
    d.advance_clock(1000.0).unwrap();
    assert!(y(&mut d) > 200.0);
}

#[test]
fn a_user_wheel_cancels_the_animation() {
    let mut d = page("", "");
    d.scroll_to_behavior(Some("box"), None, Some(600.0), "smooth")
        .unwrap();
    d.advance_clock(100.0).unwrap();
    let mid = y(&mut d);
    wheel(&mut d, 40.0);
    assert!(!d.is_scroll_animating(Some("box")));
    assert!((y(&mut d) - (mid + 40.0)).abs() < 0.1);
    d.advance_clock(1000.0).unwrap();
    assert!(
        (y(&mut d) - (mid + 40.0)).abs() < 0.1,
        "stays where the user left it"
    );
    // A keyboard scroll cancels as well.
    d.scroll_to_behavior(Some("box"), None, Some(0.0), "smooth")
        .unwrap();
    d.advance_clock(50.0).unwrap();
    d.handle_input(Input::PointerMoved { x: 50.0, y: 50.0 })
        .unwrap();
    d.focus_box_for_keys();
    d.handle_input(Input::Key {
        key: "ArrowDown".into(),
        code: "ArrowDown".into(),
        pressed: true,
        repeat: false,
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    })
    .unwrap();
    assert!(!d.is_scroll_animating(Some("box")));
}

impl Document {
    /// Make the box the keyboard scroll target.
    fn focus_box_for_keys(&mut self) {
        self.set_attribute("box", "tabindex", Some("0")).unwrap();
        self.focus("box").unwrap();
    }
}

#[test]
fn reduced_motion_makes_smooth_instant() {
    let mut d = page("scroll-behavior:smooth", "");
    d.set_app_environment(None, Some(Some(true))).unwrap();
    let s = d.scroll_to(Some("box"), None, Some(300.0)).unwrap();
    assert_eq!((s.y, s.animating), (300.0, false));
    let s = d
        .scroll_to_behavior(Some("box"), None, Some(100.0), "smooth")
        .unwrap();
    assert_eq!((s.y, s.animating), (100.0, false));
    d.set_app_environment(None, Some(None)).unwrap();
    let s = d.scroll_to(Some("box"), None, Some(300.0)).unwrap();
    assert!(s.animating, "motion allowed again");
}

#[test]
fn removing_the_element_leaves_no_animation_behind() {
    let mut d = page("", "");
    d.scroll_to_behavior(Some("box"), None, Some(300.0), "smooth")
        .unwrap();
    d.remove_element("box").unwrap();
    d.advance_clock(100.0).unwrap();
    assert!(!d.scroll_anims.active());
    assert_eq!(d.next_wake_in(), None);
}

#[test]
fn scroll_into_view_honors_the_behavior() {
    let mut d = page("", "");
    d.scroll_into_view_behavior("s4", "start", "nearest", "smooth")
        .unwrap();
    assert!(d.is_scroll_animating(Some("box")));
    d.advance_clock(1000.0).unwrap();
    assert_eq!(y(&mut d), 600.0);
    d.scroll_into_view_behavior("s0", "start", "nearest", "instant")
        .unwrap();
    assert_eq!(y(&mut d), 0.0);
}

#[test]
fn follow_end_does_not_fight_a_smooth_scroll_away() {
    let mut d = page("", "");
    d.set_scroll_follow(Some("box"), true).unwrap();
    assert_eq!(y(&mut d), 700.0);
    d.scroll_to_behavior(Some("box"), None, Some(0.0), "smooth")
        .unwrap();
    d.advance_clock(50.0).unwrap();
    let mid = y(&mut d);
    assert!(mid < 700.0);
    d.advance_clock(1000.0).unwrap();
    assert_eq!(y(&mut d), 0.0);
}

// ---- snapping -------------------------------------------------------------------------------

const MANDATORY: &str = "scroll-snap-type:y mandatory";

#[test]
fn mandatory_snaps_scroll_to_to_the_nearest_position() {
    let mut d = page(MANDATORY, "scroll-snap-align:start");
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(200.0)).unwrap().y,
        150.0
    );
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(240.0)).unwrap().y,
        300.0
    );
    assert_eq!(d.scroll_to(Some("box"), None, Some(5.0)).unwrap().y, 0.0);
    // The last position is clamped to the reachable range.
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(5000.0)).unwrap().y,
        700.0
    );
}

#[test]
fn proximity_snaps_only_within_a_third_of_the_snapport() {
    let mut d = page("scroll-snap-type:y proximity", "scroll-snap-align:start");
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(200.0)).unwrap().y,
        150.0
    );
    // 230 is 80 from 150 and 70 from 300: both beyond 200/3.
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(230.0)).unwrap().y,
        230.0
    );
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(290.0)).unwrap().y,
        300.0
    );
}

#[test]
fn center_and_end_alignment() {
    let mut d = page(MANDATORY, "scroll-snap-align:center");
    // Section 2 spans 300..450, its centre 375 sits at the snapport centre 100.
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(270.0)).unwrap().y,
        275.0
    );
    let mut d = page(MANDATORY, "scroll-snap-align:end");
    // Section 2 ends at 450, the snapport ends at 200.
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(260.0)).unwrap().y,
        250.0
    );
}

#[test]
fn scroll_padding_and_scroll_margin_move_the_snap_position() {
    let mut d = page(
        &format!("{MANDATORY};scroll-padding-top:20px"),
        "scroll-snap-align:start",
    );
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(290.0)).unwrap().y,
        280.0
    );
    let mut d = page(MANDATORY, "scroll-snap-align:start;scroll-margin-top:10px");
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(290.0)).unwrap().y,
        290.0
    );
    let mut d = page(
        &format!("{MANDATORY};scroll-padding:5% 0"),
        "scroll-snap-align:start;scroll-margin:4px 0",
    );
    // 300 - 4 (margin) - 10 (5% of 200).
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(290.0)).unwrap().y,
        286.0
    );
}

#[test]
fn wheel_and_scroll_by_snap_in_the_direction_of_the_scroll() {
    let mut d = page(MANDATORY, "scroll-snap-align:start");
    wheel(&mut d, 40.0);
    assert_eq!(
        y(&mut d),
        150.0,
        "a small wheel step moves to the next position"
    );
    wheel(&mut d, -40.0);
    assert_eq!(y(&mut d), 0.0);
    let s = d.scroll_by(Some("box"), 0.0, 10.0).unwrap();
    assert_eq!(s.y, 150.0);
    // Proximity: a short wheel step stays free until the next position is near.
    let mut d = page("scroll-snap-type:y proximity", "scroll-snap-align:start");
    wheel(&mut d, 40.0);
    assert_eq!(y(&mut d), 40.0);
    wheel(&mut d, 120.0);
    assert_eq!(y(&mut d), 150.0);
}

#[test]
fn snap_stop_always_cannot_be_skipped() {
    let mut d = page(MANDATORY, "scroll-snap-align:start");
    d.set_attribute("s1", "style", Some("scroll-snap-stop:always"))
        .unwrap();
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(420.0)).unwrap().y,
        150.0
    );
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(440.0)).unwrap().y,
        450.0,
        "not skipping from beyond"
    );
}

#[test]
fn inline_axis_snaps_too() {
    let cells: String = (0..6)
        .map(|i| format!("<div class=c id=c{i}>{i}</div>"))
        .collect();
    let mut d = Document::parse(&format!(
        "<!doctype html><style>body{{margin:0}}#box{{width:200px;height:60px;overflow-x:auto;overflow-y:hidden;display:flex;scroll-snap-type:x mandatory}}\
         .c{{flex:none;width:150px;height:40px;scroll-snap-align:start}}</style><div id=box>{cells}</div>"
    ))
    .unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(
        d.scroll_to(Some("box"), Some(200.0), None).unwrap().x,
        150.0
    );
    assert_eq!(x(&mut d), 150.0);
    // `both` snaps each axis independently.
    let mut d = page("scroll-snap-type:both proximity", "scroll-snap-align:start");
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(160.0)).unwrap().y,
        150.0
    );
}

#[test]
fn smooth_snap_after_a_wheel_step_is_animated() {
    let mut d = page(
        &format!("{MANDATORY};scroll-behavior:smooth"),
        "scroll-snap-align:start",
    );
    wheel(&mut d, 40.0);
    assert!(d.is_scroll_animating(Some("box")));
    assert_eq!(y(&mut d), 0.0);
    d.advance_clock(1000.0).unwrap();
    assert_eq!(y(&mut d), 150.0);
    d.set_app_environment(None, Some(Some(true))).unwrap();
    wheel(&mut d, 40.0);
    assert_eq!(y(&mut d), 300.0, "reduced motion snaps at once");
}

#[test]
fn nested_scrollers_keep_their_own_snap_areas() {
    let marks: String = (0..10)
        .map(|i| format!("<div class=m style='height:40px'>{i}</div>"))
        .collect();
    let mut d = Document::parse(&format!(
        "<!doctype html><style>body{{margin:0}}#box{{width:200px;height:200px;overflow:auto;scroll-snap-type:y mandatory}}\
         .s{{height:300px;scroll-snap-align:start}}#inner{{height:100px;overflow:auto;scroll-snap-type:y mandatory}}\
         .m{{scroll-snap-align:start}}</style><div id=box><div class=s>0</div>\
         <div class=s><div id=inner>{marks}</div></div><div class=s>2</div></div>"
    ))
    .unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    // The inner marks (every 40px) are not snap areas of the outer scroller.
    assert_eq!(d.scroll_to(Some("box"), None, Some(130.0)).unwrap().y, 0.0);
    assert_eq!(
        d.scroll_to(Some("box"), None, Some(170.0)).unwrap().y,
        300.0
    );
    // The inner scroller snaps on its own.
    assert_eq!(
        d.scroll_to(Some("inner"), None, Some(50.0)).unwrap().y,
        40.0
    );
    assert_eq!(
        d.scroll_to(Some("inner"), None, Some(65.0)).unwrap().y,
        80.0
    );
}

#[test]
fn the_viewport_snaps_from_the_root_element() {
    let sections: String = (0..6).map(|i| format!("<div class=s>{i}</div>")).collect();
    let mut d = Document::parse(&format!(
        "<!doctype html><style>html{{scroll-snap-type:y mandatory;scroll-padding-top:10px}}body{{margin:0}}\
         .s{{height:250px;scroll-snap-align:start}}</style>{sections}"
    ))
    .unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    assert_eq!(d.scroll_to(None, None, Some(300.0)).unwrap().y, 240.0);
}

#[test]
fn choose_picks_per_the_spec_rules() {
    let c = [(0.0, false), (100.0, false), (200.0, true), (300.0, false)];
    // Nearest, mandatory.
    assert_eq!(choose(&c, 0.0, 130.0, false, true, 90.0, 0.0, 300.0), 100.0);
    // Proximity threshold is a third of the snapport.
    assert_eq!(
        choose(&c, 0.0, 130.0, false, false, 90.0, 0.0, 300.0),
        100.0
    );
    assert_eq!(
        choose(&c, 0.0, 150.0, false, false, 90.0, 0.0, 300.0),
        150.0
    );
    // Directional: the next position in the direction.
    assert_eq!(
        choose(&c, 100.0, 110.0, true, true, 90.0, 0.0, 300.0),
        200.0
    );
    assert_eq!(choose(&c, 100.0, 90.0, true, true, 90.0, 0.0, 300.0), 0.0);
    // `always` stops the scroll.
    assert_eq!(choose(&c, 0.0, 300.0, false, true, 90.0, 0.0, 300.0), 200.0);
    // No candidates: the clamped request.
    assert_eq!(
        choose(&[], 0.0, 500.0, false, true, 90.0, 0.0, 300.0),
        300.0
    );
}

// ---- overscroll ----------------------------------------------------------------------------

fn nested(inner_css: &str) -> Document {
    let rows: String = (0..8).map(|i| format!("<div class=r>{i}</div>")).collect();
    let mut d = Document::parse(&format!(
        "<!doctype html><style>body{{margin:0}}#outer{{width:300px;height:200px;overflow:auto}}\
         #inner{{width:200px;height:100px;overflow:auto;{inner_css}}}.r{{height:50px}}.pad{{height:400px}}</style>\
         <div id=outer><div id=inner>{rows}</div><div class=pad></div></div>"
    ))
    .unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    d
}

#[test]
fn scrolling_chains_to_the_ancestor_by_default() {
    let mut d = nested("");
    d.scroll_to(Some("inner"), None, Some(1000.0)).unwrap();
    d.handle_input(Input::Wheel {
        dx: 0.0,
        dy: 40.0,
        x: 50.0,
        y: 50.0,
    })
    .unwrap();
    assert_eq!(d.scroll_state(Some("outer")).unwrap().y, 40.0);
}

#[test]
fn overscroll_behavior_contain_and_none_stop_the_chain() {
    for css in [
        "overscroll-behavior:contain",
        "overscroll-behavior:none",
        "overscroll-behavior-y:contain",
    ] {
        let mut d = nested(css);
        d.scroll_to(Some("inner"), None, Some(1000.0)).unwrap();
        let before = d.scroll_state(Some("inner")).unwrap().y;
        d.handle_input(Input::Wheel {
            dx: 0.0,
            dy: 40.0,
            x: 50.0,
            y: 50.0,
        })
        .unwrap();
        assert_eq!(d.scroll_state(Some("outer")).unwrap().y, 0.0, "{css}");
        assert_eq!(d.scroll_state(Some("inner")).unwrap().y, before);
    }
    // The other axis still chains.
    let mut d = nested("overscroll-behavior-x:contain");
    d.scroll_to(Some("inner"), None, Some(1000.0)).unwrap();
    d.handle_input(Input::Wheel {
        dx: 0.0,
        dy: 40.0,
        x: 50.0,
        y: 50.0,
    })
    .unwrap();
    assert_eq!(d.scroll_state(Some("outer")).unwrap().y, 40.0);
}

#[test]
fn the_new_properties_parse_without_diagnostics() {
    let d = page(
        "scroll-snap-type:x proximity;overscroll-behavior:contain;scroll-padding:1px 2px;scroll-margin:3px",
        "scroll-snap-align:start end;scroll-snap-stop:always;scroll-behavior:smooth",
    );
    assert!(
        d.diagnostics()
            .iter()
            .all(|e| !e.to_string().contains("scroll-")),
        "{:?}",
        d.diagnostics()
    );
}
