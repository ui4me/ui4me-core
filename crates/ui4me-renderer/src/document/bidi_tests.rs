//! 0.5 F0503: bidirectional selection, caret and ranges through the production facade. Offsets
//! stay logical; the highlight and the caret geometry come from the shaped clusters.
use super::*;

const MIXED: &str = "abc \u{5d0}\u{5d1}\u{5d2} def";
const STYLE: &str = "<style>body{margin:0;font-size:20px} p{margin:0} input,textarea{margin:0;padding:0;border:0;font-size:20px;font-family:sans-serif}</style>";

fn button(d: &mut Document, pressed: bool, x: f64, y: f64) {
    d.handle_input(Input::PointerButton {
        pressed,
        button: 0,
        x,
        y,
    })
    .unwrap();
}
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
fn geom(d: &mut Document, el: &str) -> crate::interaction::editing::EditGeometry {
    let n = d.node(el).unwrap();
    d.control_geometry(d.id(), n).unwrap().unwrap()
}
fn selection(d: &Document, el: &str) -> (u32, u32) {
    let n = d.node(el).unwrap();
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
fn click_at(d: &mut Document, x: f64, y: f64) {
    button(d, true, x, y);
    button(d, false, x, y);
}
/// The highlight rectangles with the ones that touch on a row merged (one run per text fragment).
fn merged_rects(d: &mut Document) -> Vec<(f64, f64, f64, f64)> {
    let mut rects = d.selection_rects().unwrap();
    rects.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
    let mut out: Vec<(f64, f64, f64, f64)> = Vec::new();
    for r in rects {
        match out.last_mut() {
            Some(l) if l.1 == r.1 && r.0 <= l.0 + l.2 + 0.01 => l.2 = (r.0 + r.2 - l.0).max(l.2),
            _ => out.push(r),
        }
    }
    out
}
/// Viewport x of the caret stop `offset` of the control's first line.
fn stop_x(g: &crate::interaction::editing::EditGeometry, offset: u32) -> f64 {
    g.origin.0 + f64::from(g.inner.x_of(0, offset))
}

#[test]
fn document_selection_across_an_rtl_run_is_visually_split_and_logically_copied() {
    let mut d = Document::parse(&format!("<!doctype html>{STYLE}<p id=p>{MIXED}</p>")).unwrap();
    d.render(640, 200, 1.0).unwrap();
    let p = d.element_rect("p").unwrap().unwrap();
    let y = p.y + p.height / 2.0;
    d.select_text(Some("p")).unwrap();
    assert_eq!(d.selection_text().unwrap(), MIXED);
    assert_eq!(
        merged_rects(&mut d).len(),
        1,
        "the whole line is one visual span"
    );
    // Drag from the start of the line rightwards; somewhere inside the Hebrew run the focus lands
    // on a cluster whose logical range leaves a visual hole.
    let mut split = None;
    let mut x = p.x + 1.0;
    while x < p.x + p.width {
        d.clear_selection();
        button(&mut d, true, p.x + 1.0, y);
        d.handle_input(Input::PointerMoved { x, y }).unwrap();
        button(&mut d, false, x, y);
        let t = d.selection_text().unwrap();
        assert!(
            MIXED.starts_with(&t),
            "the copy is a logical prefix, got {t:?}"
        );
        let rects = merged_rects(&mut d);
        if rects.len() >= 2 {
            split = Some((t, rects));
            break;
        }
        x += 1.0;
    }
    let (t, rects) = split.expect("a selection ending inside the RTL run is visually split");
    assert!(
        t.starts_with("abc \u{5d0}") && !t.ends_with('\u{5d2}'),
        "{t:?}"
    );
    assert!(rects[0].0 + rects[0].2 < rects[1].0 - 0.5, "{rects:?}");
    // Ctrl+C copies the logical substring.
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
    assert_eq!(d.clipboard_text().unwrap().as_deref(), Some(t.as_str()));
}

#[test]
fn document_selection_over_a_wrapped_rtl_paragraph_highlights_each_line() {
    let text = "\u{5e9}\u{5dc}\u{5d5}\u{5dd} hello \u{5e2}\u{5d5}\u{5dc}\u{5dd} world \u{5e9}\u{5dc}\u{5d5}\u{5dd} again";
    let mut d = Document::parse(&format!(
        "<!doctype html>{STYLE}<p id=p dir=rtl style='width:120px'>{text}</p>"
    ))
    .unwrap();
    d.render(640, 300, 1.0).unwrap();
    let p = d.element_rect("p").unwrap().unwrap();
    assert!(p.height > 30.0, "the paragraph wraps: {p:?}");
    d.select_text(Some("p")).unwrap();
    assert_eq!(d.selection_text().unwrap(), text);
    let rects = merged_rects(&mut d);
    let mut ys: Vec<i64> = rects.iter().map(|r| r.1.round() as i64).collect();
    ys.dedup();
    assert!(ys.len() >= 2, "one band per line: {rects:?}");
    for r in &rects {
        assert!(
            r.0 >= p.x - 0.5 && r.0 + r.2 <= p.x + p.width + 0.5,
            "{r:?}"
        );
    }
}

#[test]
fn input_click_in_an_rtl_run_maps_to_the_logical_offset() {
    let mut d = Document::parse(&format!(
        "<!doctype html>{STYLE}<input id=i style='width:400px' value='{MIXED}'>"
    ))
    .unwrap();
    d.render(640, 100, 1.0).unwrap();
    d.focus("i").unwrap();
    let g = geom(&mut d, "i");
    let line = &g.inner.lines[0];
    assert!(line.bidi);
    let c = *line
        .clusters
        .iter()
        .find(|c| c.0 == 4)
        .expect("cluster of the first Hebrew letter");
    let c2 = *line.clusters.iter().find(|c| c.0 == 6).unwrap();
    // The first Hebrew letter is the rightmost of the run, the last one the leftmost.
    assert!(c.2 > c2.2, "{:?}", line.clusters);
    let y = g.origin.1 + f64::from(line.height) / 2.0;
    // Left half of the leftmost (logically last) Hebrew letter: caret after the run (offset 7).
    click_at(&mut d, g.origin.0 + f64::from(c2.2) + 1.0, y);
    assert_eq!(selection(&d, "i"), (7, 7));
    // Right half of the same letter: before it (offset 6).
    click_at(&mut d, g.origin.0 + f64::from(c2.3) - 1.0, y);
    assert_eq!(selection(&d, "i"), (6, 6));
    // Right half of the logically first letter: before it (offset 4); its left half: after it (5).
    click_at(&mut d, g.origin.0 + f64::from(c.3) - 1.0, y);
    assert_eq!(selection(&d, "i"), (4, 4));
    click_at(&mut d, g.origin.0 + f64::from(c.2) + 1.0, y);
    assert_eq!(selection(&d, "i"), (5, 5));
}

#[test]
fn input_caret_geometry_inside_an_rtl_run_is_on_the_visual_side() {
    let mut d = Document::parse(&format!(
        "<!doctype html>{STYLE}<input id=i style='width:400px' value='{MIXED}'>"
    ))
    .unwrap();
    d.render(640, 100, 1.0).unwrap();
    let g = geom(&mut d, "i");
    // Inside the run (offsets 5 and 6) a later offset is further left.
    assert!(
        stop_x(&g, 5) > stop_x(&g, 6),
        "{:?}",
        g.inner.lines[0].stops
    );
    // The run is visually inside the Latin text: "abc " < run < " def".
    assert!(stop_x(&g, 3) < stop_x(&g, 6));
    assert!(stop_x(&g, 8) >= stop_x(&g, 5));
    assert!(stop_x(&g, 11) > stop_x(&g, 8));
}

/// A UTF-16 offset that is no surrogate split and no split of an emoji sequence, a skin tone or
/// a combining mark from its base (the editing engine's grapheme approximation).
fn is_boundary(value: &str, u16_offset: u32) -> bool {
    let mut at = 0u32;
    let mut prev: Option<char> = None;
    for c in value.chars() {
        if at == u16_offset {
            let joins = matches!(
                c,
                '\u{200d}' | '\u{1f3fb}'..='\u{1f3ff}' | '\u{300}'..='\u{36f}' | '\u{64b}'..='\u{65f}'
            );
            return !(joins || prev == Some('\u{200d}'));
        }
        at += c.len_utf16() as u32;
        prev = Some(c);
    }
    at == u16_offset
}

#[test]
fn input_editing_keys_never_split_graphemes() {
    // A ZWJ family and a skin-tone emoji on a left-to-right line.
    let value = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{1f44d}\u{1f3fd}x";
    let mut d = Document::parse(&format!(
        "<!doctype html>{STYLE}<input id=i style='width:400px' value='{value}'>"
    ))
    .unwrap();
    d.render(640, 100, 1.0).unwrap();
    d.focus("i").unwrap();
    d.handle_input(key("Home", false)).unwrap();
    assert_eq!(selection(&d, "i"), (0, 0));
    let u16len = |s: &str| s.encode_utf16().count() as u32;
    let b1 = u16len("\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}");
    let b2 = b1 + u16len("\u{1f44d}\u{1f3fd}");
    d.handle_input(key("ArrowRight", true)).unwrap();
    assert_eq!(selection(&d, "i"), (0, b1));
    d.handle_input(key("ArrowRight", true)).unwrap();
    assert_eq!(selection(&d, "i"), (0, b2));
    // Delete and Backspace remove exactly one grapheme.
    d.handle_input(key("Home", false)).unwrap();
    d.handle_input(key("Delete", false)).unwrap();
    assert_eq!(d.get_value("i").unwrap(), "\u{1f44d}\u{1f3fd}x");
    d.handle_input(key("ArrowRight", false)).unwrap();
    d.handle_input(key("Backspace", false)).unwrap();
    assert_eq!(d.get_value("i").unwrap(), "x");
}

#[test]
fn mixed_direction_editing_keys_stay_on_grapheme_boundaries() {
    // An emoji, an Arabic word with combining marks, Hebrew and Latin on one bidi line.
    let value: &str = "\u{1f44d}\u{1f3fd}\u{645}\u{64e}\u{631}\u{652}\u{62d} \u{5d0}\u{5d1} x";
    let mut d = Document::parse(&format!(
        "<!doctype html>{STYLE}<input id=i style='width:400px' value='{value}'>"
    ))
    .unwrap();
    d.render(640, 100, 1.0).unwrap();
    d.focus("i").unwrap();
    let mut seen = 0;
    for k in ["ArrowRight", "ArrowLeft"] {
        d.handle_input(key("Home", false)).unwrap();
        for _ in 0..12 {
            d.handle_input(key(k, true)).unwrap();
            let (a, b) = selection(&d, "i");
            assert!(
                is_boundary(value, a) && is_boundary(value, b),
                "{k} {a}..{b}"
            );
            seen += 1;
        }
    }
    assert!(seen > 0);
    // Delete and Backspace at every logical position remove whole graphemes only.
    for i in 0..6 {
        for k in ["Delete", "Backspace"] {
            d.set_value("i", value).unwrap();
            d.focus("i").unwrap();
            d.handle_input(key("Home", false)).unwrap();
            for _ in 0..i {
                d.handle_input(key("ArrowRight", false)).unwrap();
            }
            d.handle_input(key(k, false)).unwrap();
            let v: Vec<u16> = d.get_value("i").unwrap().encode_utf16().collect();
            let o: Vec<u16> = value.encode_utf16().collect();
            let p = o.iter().zip(&v).take_while(|(a, b)| a == b).count();
            let end = p + o.len() - v.len();
            assert!(
                is_boundary(value, p as u32) && is_boundary(value, end as u32),
                "{k} after {i} steps removed {p}..{end}"
            );
        }
    }
}

#[test]
fn rtl_input_puts_the_caret_at_the_right_edge() {
    let mut d = Document::parse(&format!(
        "<!doctype html>{STYLE}<input id=i dir=rtl style='width:300px' value='\u{5e9}\u{5dc}\u{5d5}\u{5dd}'>"
    ))
    .unwrap();
    d.render(640, 100, 1.0).unwrap();
    d.focus("i").unwrap();
    d.handle_input(key("Home", false)).unwrap();
    assert_eq!(selection(&d, "i"), (0, 0));
    let g = geom(&mut d, "i");
    let vw = g.inner.viewport.size.width.to_f64();
    let x0 = f64::from(g.inner.x_of(0, 0));
    assert!(x0 > vw * 0.9, "offset 0 is at the right edge: {x0} of {vw}");
    assert!(f64::from(g.inner.x_of(0, 4)) < x0);
    // Empty rtl input: the caret also starts on the right.
    d.set_value("i", "").unwrap();
    d.render(640, 100, 1.0).unwrap();
    let g = geom(&mut d, "i");
    assert!(
        f64::from(g.inner.x_of(0, 0)) > vw * 0.9,
        "{}",
        g.inner.x_of(0, 0)
    );
}

#[test]
fn textarea_home_end_use_the_visual_line_and_shift_arrows_select() {
    let value = "\u{5e9}\u{5dc}\u{5d5}\u{5dd} hello \u{5e2}\u{5d5}\u{5dc}\u{5dd} world \u{5e9}\u{5dc}\u{5d5}\u{5dd} again";
    let mut d = Document::parse(&format!(
        "<!doctype html>{STYLE}<textarea id=t dir=auto style='width:120px;height:120px'>{value}</textarea>"
    ))
    .unwrap();
    d.render(640, 300, 1.0).unwrap();
    d.focus("t").unwrap();
    let g = geom(&mut d, "t");
    assert!(g.inner.lines.len() >= 2, "wraps: {}", g.inner.lines.len());
    let l0 = g.inner.lines[0].clone();
    let l1 = g.inner.lines[1].clone();
    // Put the caret inside the second line, then Home/End.
    let mid = (l1.start + l1.end) / 2;
    let y = g.origin.1 + f64::from(l1.top + l1.height / 2.0);
    click_at(&mut d, g.origin.0 + f64::from(g.inner.x_of(1, mid)), y);
    let (s, _) = selection(&d, "t");
    assert!(
        s >= l1.start && s <= l1.end,
        "{s} in {}..{}",
        l1.start,
        l1.end
    );
    d.handle_input(key("Home", false)).unwrap();
    assert_eq!(
        selection(&d, "t").0,
        l1.start,
        "Home: start of the visual line"
    );
    d.handle_input(key("End", false)).unwrap();
    let e = selection(&d, "t").0;
    assert!(e >= l1.start && e <= l1.end, "End stays on line 1: {e}");
    assert!(e > l0.start);
    d.handle_input(key("Home", false)).unwrap();
    // The paragraph is right-to-left: the line starts at its right edge and Shift+ArrowLeft extends.
    for _ in 0..3 {
        d.handle_input(key("ArrowLeft", true)).unwrap();
    }
    let (a, b) = selection(&d, "t");
    // Home is the start of line 1, at its right edge; visual-left extends logically forward.
    assert_eq!((a, b), (l1.start, l1.start + 3));
}
