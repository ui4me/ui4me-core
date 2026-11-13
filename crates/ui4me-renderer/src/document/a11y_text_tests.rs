//! 0.5 F0504: the accessible text of text fields on the real layout path — one text run per
//! laid-out line with character geometry, the caret / selection, and the SetTextSelection and
//! ReplaceSelectedText actions executed through the editing engine.
use super::*;
use crate::a11y::{A11yRequest, A11yRequestKind, A11yTree};
use accesskit::Role;

fn field<'a>(t: &'a A11yTree, id: &str) -> &'a crate::a11y::A11yNode {
    t.nodes
        .values()
        .find(|n| n.html_id.as_deref() == Some(id))
        .unwrap()
}
fn runs<'a>(t: &'a A11yTree, id: &str) -> Vec<&'a crate::a11y::A11yNode> {
    field(t, id)
        .children
        .iter()
        .map(|c| t.get(*c).unwrap())
        .collect()
}
fn selection_of(d: &Document, id: &str) -> (u32, u32) {
    let n = d.node(id).unwrap();
    let s = d
        .pipeline
        .document()
        .control_state(n)
        .unwrap()
        .unwrap()
        .selection
        .unwrap();
    (s.start, s.end)
}
fn key(name: &str, shift: bool) -> Input {
    Input::Key {
        key: name.into(),
        code: name.into(),
        pressed: true,
        repeat: false,
        shift,
        ctrl: false,
        alt: false,
        meta: false,
    }
}

#[test]
fn textarea_lines_become_runs_with_geometry_and_hard_breaks() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0;font:16px sans-serif}textarea{width:300px;height:120px}</style><textarea id=t>Hello world\nsecond line</textarea>",
    )
    .unwrap();
    d.render(400, 200, 1.0).unwrap();
    let t = d.accessibility_tree().unwrap();
    let r = runs(&t, "t");
    assert_eq!(r.len(), 2, "one run per line");
    assert!(r.iter().all(|n| n.role == Role::TextRun));
    assert_eq!(r[0].value.as_deref(), Some("Hello world\n"));
    assert_eq!(r[1].value.as_deref(), Some("second line"));
    let run0 = r[0].run.as_ref().unwrap();
    assert!(run0.hard_break);
    assert_eq!(run0.lengths.len(), 12, "11 characters and the line break");
    assert_eq!(run0.offsets.first(), Some(&0));
    assert_eq!(run0.offsets.last(), Some(&12));
    // Positions grow left to right with positive widths (the break has none).
    assert!(run0.positions.windows(2).all(|w| w[1] >= w[0]));
    assert!(run0.widths[..11].iter().all(|w| *w > 0.0));
    assert_eq!(run0.word_starts, vec![6], "'world' starts a word");
    let b0 = r[0].bounds.unwrap();
    let b1 = r[1].bounds.unwrap();
    assert!(b1.y > b0.y, "the second line is below the first");
    assert_eq!(r[1].run.as_ref().unwrap().offsets.first(), Some(&12));
}

#[test]
fn the_caret_and_selection_follow_editing_and_land_on_runs() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0;font:16px sans-serif}textarea{width:300px;height:120px}</style><textarea id=t>Hello world\nsecond line</textarea>",
    )
    .unwrap();
    d.render(400, 200, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    d.focus("t").unwrap();
    // Ctrl+Home, then End: the caret is at the end of the first line, on its line break.
    d.handle_input(Input::Key {
        key: "Home".into(),
        code: "Home".into(),
        pressed: true,
        repeat: false,
        shift: false,
        ctrl: true,
        alt: false,
        meta: false,
    })
    .unwrap();
    d.handle_input(key("End", false)).unwrap();
    let t = d.accessibility_tree().unwrap();
    let f = field(&t, "t");
    let sel = f.selection.expect("a focused field has a caret");
    let run0 = f.children[0];
    assert_eq!(sel.value, (11, 11));
    assert_eq!(
        sel.focus,
        (run0, 11),
        "on the break character, not after it"
    );
    // Shift+Down extends into the second line.
    d.handle_input(key("ArrowDown", true)).unwrap();
    let t = d.accessibility_tree().unwrap();
    let f = field(&t, "t");
    let sel = f.selection.unwrap();
    assert_eq!(sel.anchor, (run0, 11));
    assert_eq!(
        sel.focus.0, f.children[1],
        "the focus moved to the second run"
    );
    assert!(
        f.actions
            .contains(&crate::a11y::A11yAction::SetTextSelection)
    );
}

#[test]
fn set_text_selection_and_replace_selected_text_edit_through_the_engine() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0;font:16px sans-serif}textarea{width:300px;height:120px}</style><input id=a value='Příliš žluťoučký kůň'><textarea id=t>Hello world\nsecond line</textarea>",
    )
    .unwrap();
    d.render(400, 200, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    let t = d.accessibility_tree().unwrap();
    let r = runs(&t, "t");
    let (run0, run1) = (r[0].id, r[1].id);
    let target = d.node("t").ok();
    // Select "world\nsecond" across the line break.
    d.accessibility_request(&A11yRequest {
        target,
        kind: A11yRequestKind::SetTextSelection {
            anchor: (run0, 6),
            focus: (run1, 6),
        },
    })
    .unwrap();
    assert_eq!(selection_of(&d, "t"), (6, 18));
    assert_eq!(
        d.interaction.focused_element(d.id()),
        target,
        "the field took focus"
    );
    let events = d.drain_events();
    assert!(events.iter().any(|e| e["type"] == "focus"));
    d.accessibility_request(&A11yRequest {
        target,
        kind: A11yRequestKind::ReplaceSelectedText("big".into()),
    })
    .unwrap();
    assert_eq!(d.get_value("t").unwrap(), "Hello big line");
    let events = d.drain_events();
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "input" && e["input_type"] == "insertReplacementText"),
        "{events:?}"
    );
    // The caret after the edit is where the tree says it is.
    let t = d.accessibility_tree().unwrap();
    assert_eq!(field(&t, "t").selection.unwrap().value, (9, 9));
    // A collapsed selection inserts at the caret; characters are grapheme clusters.
    let a = d.node("a").ok();
    let t = d.accessibility_tree().unwrap();
    let ra = runs(&t, "a");
    assert_eq!(ra.len(), 1);
    let run = ra[0].run.as_ref().unwrap();
    assert_eq!(run.lengths.len(), "Příliš žluťoučký kůň".chars().count());
    assert_eq!(run.lengths[1], 2, "ř is two UTF-8 bytes");
    d.accessibility_request(&A11yRequest {
        target: a,
        kind: A11yRequestKind::SetTextSelection {
            anchor: (ra[0].id, 6),
            focus: (ra[0].id, 6),
        },
    })
    .unwrap();
    d.accessibility_request(&A11yRequest {
        target: a,
        kind: A11yRequestKind::ReplaceSelectedText(",".into()),
    })
    .unwrap();
    assert_eq!(d.get_value("a").unwrap(), "Příliš, žluťoučký kůň");
}

#[test]
fn password_runs_are_masked_and_rtl_runs_measure_from_the_right() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0;font:16px sans-serif}input{width:300px}</style><input id=p type=password value=secret><input id=h dir=rtl value='שלום'>",
    )
    .unwrap();
    d.render(400, 200, 1.0).unwrap();
    let t = d.accessibility_tree().unwrap();
    let p = runs(&t, "p");
    assert_eq!(p[0].value.as_deref(), Some("\u{2022}".repeat(6).as_str()));
    let h = runs(&t, "h");
    let run = h[0].run.as_ref().unwrap();
    assert!(run.rtl, "an all right-to-left line is a right-to-left run");
    assert!(
        run.positions.windows(2).all(|w| w[1] >= w[0]),
        "positions grow in the run's direction: {:?}",
        run.positions
    );
}

/// An assistive ScrollIntoView on a static-text object (a DOM text node) must
/// scroll its element into view, as it does for the element itself.
#[test]
fn scroll_into_view_on_static_text_scrolls_its_element() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0;font:16px sans-serif}#s{height:60px;overflow:auto}#far{margin-top:400px}</style>\
         <div id=s><p>top</p><p id=far>Far away paragraph</p></div>",
    )
    .unwrap();
    d.render(400, 200, 1.0).unwrap();
    let before = d.element_rect("far").unwrap().unwrap();
    assert!(before.y > 60.0, "fixture starts scrolled out: {before:?}");
    let t = d.accessibility_tree().unwrap();
    let leaf = t
        .nodes
        .values()
        .find(|n| n.html_id.is_none() && n.name.contains("Far away"))
        .expect("static text object");
    let target = crate::a11y::node_of_key(leaf.id);
    assert!(
        target.is_some_and(|n| d
            .pipeline
            .document()
            .arena()
            .get(n)
            .is_some_and(|x| x.as_element().is_none())),
        "the static-text object maps to a text node"
    );
    d.accessibility_request(&A11yRequest {
        target,
        kind: A11yRequestKind::ScrollIntoView,
    })
    .unwrap();
    d.render(400, 200, 1.0).unwrap();
    let after = d.element_rect("far").unwrap().unwrap();
    assert!(
        after.y < before.y && after.y + after.height <= 60.5,
        "scrolled into view: {before:?} -> {after:?}"
    );
}
