// INTERACT audit probes. Uses only the public ui4me_renderer API.
use serde_json::{Value, json};
use ui4me_renderer::{Document, Input};

fn key(d: &mut Document, k: &str, code: &str, shift: bool) -> Vec<Value> {
    let mut ev = d
        .handle_input(Input::Key {
            key: k.into(),
            code: code.into(),
            pressed: true,
            repeat: false,
            shift,
            ctrl: false,
            alt: false,
            meta: false,
        })
        .unwrap();
    ev.extend(
        d.handle_input(Input::Key {
            key: k.into(),
            code: code.into(),
            pressed: false,
            repeat: false,
            shift,
            ctrl: false,
            alt: false,
            meta: false,
        })
        .unwrap(),
    );
    ev
}

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

fn press(d: &mut Document, x: f64, y: f64, pressed: bool) -> Vec<Value> {
    d.handle_input(Input::PointerButton { pressed, button: 0, x, y })
        .unwrap()
}

fn find_nodes<'a>(tree: &'a Value, pred: &dyn Fn(&Value) -> bool) -> Vec<&'a Value> {
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| pred(n))
        .collect()
}

/// PR #4 siblings: control geometry inside a position:fixed box after the page scrolled.
fn fixed_textarea() {
    println!("== fixed_textarea (PR #4 sibling paths)");
    let html = "<!doctype html><style>body{margin:0}#tall{height:3000px}\
        #fx{position:fixed;top:50px;left:10px;width:320px;height:220px}\
        textarea{display:block;width:300px;height:160px;font:16px/20px monospace;padding:0;border:0;margin:0}</style>\
        <div id=fx><textarea id=ta>line one\nline two\nline three\nline four</textarea></div><div id=tall></div>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    d.focus("ta").unwrap();
    d.render(400, 300, 1.0).unwrap();
    let r0 = d.element_rect("ta").unwrap().unwrap();
    let ime0 = d.ime_request().cloned();
    let a0 = d.accessibility_json().unwrap();
    let runs0: Vec<_> = find_nodes(&a0, &|n| n["role"] == "TextRun")
        .iter()
        .map(|n| n["bounds"].clone())
        .collect();
    d.scroll_to(None, None, Some(400.0)).unwrap();
    d.render(400, 300, 1.0).unwrap();
    let r1 = d.element_rect("ta").unwrap().unwrap();
    let ime1 = d.ime_request().cloned();
    let a1 = d.accessibility_json().unwrap();
    let runs1: Vec<_> = find_nodes(&a1, &|n| n["role"] == "TextRun")
        .iter()
        .map(|n| n["bounds"].clone())
        .collect();
    println!("element_rect before={r0:?}\nelement_rect after ={r1:?}");
    println!("ime cursor_area before={:?}\nime cursor_area after ={:?}", ime0.map(|i| i.cursor_area), ime1.map(|i| i.cursor_area));
    println!("a11y text runs before={}\na11y text runs after ={}", json!(runs0), json!(runs1));
    // Click on line three (y = 50 + 2*20 + 10 = 100) at x = 10 + 4 chars.
    let hit = d.element_at_point(30.0, 100.0).unwrap();
    println!("element_at_point(30,100) after scroll = {}", hit);
    press(&mut d, 30.0, 100.0, true);
    press(&mut d, 30.0, 100.0, false);
    println!(
        "caret after click on line three (expect inside 'line three', offsets 18..28): {}",
        d.control_selection(Some("ta"), None).unwrap()
    );
}

/// Click when press and release land on different descendants of the same button.
fn click_across_children() {
    println!("== click_across_children");
    let html = "<!doctype html><style>body{margin:0}button{padding:20px;font:16px sans-serif;border:0}</style>\
        <button id=b><span id=s>Label</span></button>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.render(400, 300, 1.0).unwrap();
    let s = d.element_rect("s").unwrap().unwrap();
    let b = d.element_rect("b").unwrap().unwrap();
    println!("span={s:?} button={b:?}");
    let (sx, sy) = (s.x + s.width / 2.0, s.y + s.height / 2.0);
    let mut ev = press(&mut d, sx, sy, true);
    ev.extend(press(&mut d, b.x + 4.0, sy, false));
    let t = types(&ev);
    println!("press on span, release on button padding -> {:?}", t);
    println!("click delivered: {}", t.iter().any(|t| t.starts_with("click@")));
    let mut ev = press(&mut d, sx, sy, true);
    ev.extend(press(&mut d, sx, sy, false));
    println!("press+release on span -> click delivered: {}", types(&ev).iter().any(|t| t.starts_with("click@")));
}

/// A11y/element bounds versus hit testing under a CSS transform.
fn transformed_bounds() {
    println!("== transformed_bounds");
    let html = "<!doctype html><style>body{margin:0}#t{position:absolute;left:0;top:0;width:100px;height:50px;transform:translateX(200px) scale(2)}</style>\
        <button id=t>T</button>";
    let mut d = Document::parse_hosted(html, None, 600, 300).unwrap();
    d.render(600, 300, 1.0).unwrap();
    println!("element_rect(t) = {:?}", d.element_rect("t").unwrap());
    println!("element_at_point(250,25) = {}", d.element_at_point(250.0, 25.0).unwrap());
    println!("element_at_point(50,25) = {}", d.element_at_point(50.0, 25.0).unwrap());
    let a = d.accessibility_json().unwrap();
    for n in find_nodes(&a, &|n| n["role"] == "Button") {
        println!("a11y button bounds = {}", n["bounds"]);
    }
}

fn tab_order() {
    println!("== tab_order");
    let html = "<!doctype html><button id=a>a</button><button id=b tabindex=2>b</button>\
        <button id=c tabindex=1>c</button><a id=l href='#x'>link</a><div id=dv tabindex=0>div</div>\
        <button id=dis disabled>dis</button><span id=neg tabindex=-1>neg</span><input id=i>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.render(400, 300, 1.0).unwrap();
    let mut order = Vec::new();
    for _ in 0..8 {
        key(&mut d, "Tab", "Tab", false);
        order.push(d.focused_element()["element"].as_str().unwrap_or("-").to_owned());
    }
    println!("Tab order: {:?}", order);
    let mut back = Vec::new();
    for _ in 0..3 {
        key(&mut d, "Tab", "Tab", true);
        back.push(d.focused_element()["element"].as_str().unwrap_or("-").to_owned());
    }
    println!("Shift+Tab: {:?}", back);
    println!("focused_element json = {}", d.focused_element());
}

fn radio_and_select_keys() {
    println!("== radio_and_select_keys");
    let html = "<!doctype html><input type=radio name=g id=r1 checked><input type=radio name=g id=r2><input type=radio name=g id=r3>\
        <select id=sel><option>one</option><option>two</option><option>three</option></select>\
        <input type=checkbox id=cb><input type=range id=rg min=0 max=10 value=5>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.render(400, 300, 1.0).unwrap();
    d.focus("r1").unwrap();
    key(&mut d, "ArrowDown", "ArrowDown", false);
    println!(
        "radio ArrowDown: focused={} r2.checked={:?}",
        d.focused_element()["element"],
        d.get_checked("r2")
    );
    key(&mut d, "ArrowRight", "ArrowRight", false);
    println!("radio ArrowRight: focused={} r3.checked={:?}", d.focused_element()["element"], d.get_checked("r3"));
    key(&mut d, "ArrowDown", "ArrowDown", false);
    println!("radio ArrowDown wrap: focused={} r1.checked={:?}", d.focused_element()["element"], d.get_checked("r1"));
    d.focus("sel").unwrap();
    key(&mut d, "ArrowDown", "ArrowDown", false);
    println!("select ArrowDown: value={:?}", d.get_value("sel"));
    key(&mut d, "End", "End", false);
    println!("select End: value={:?}", d.get_value("sel"));
    d.focus("cb").unwrap();
    key(&mut d, " ", "Space", false);
    println!("checkbox Space: checked={:?}", d.get_checked("cb"));
    d.focus("rg").unwrap();
    key(&mut d, "ArrowRight", "ArrowRight", false);
    key(&mut d, "PageUp", "PageUp", false);
    println!("range ArrowRight+PageUp: value={:?}", d.get_value("rg"));
}

/// Window focus loss during an explicit pointer capture and a range drag.
fn focus_loss_capture() {
    println!("== focus_loss_capture");
    let html = "<!doctype html><style>body{margin:0}#c{width:200px;height:100px}#rg{width:300px}</style>\
        <div id=c>cap</div><input type=range id=rg min=0 max=100 value=0>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.render(400, 300, 1.0).unwrap();
    press(&mut d, 50.0, 50.0, true);
    let r = d.set_pointer_capture("c", 1, true);
    println!("set_pointer_capture -> {:?}", r);
    d.handle_input(Input::PointerMoved { x: 60.0, y: 60.0 }).unwrap();
    println!("capture while pressed = {:?}", d.pointer_capture());
    d.handle_input(Input::Focus(false)).unwrap();
    println!("capture after Focus(false) = {:?}", d.pointer_capture());
    let ev = d.handle_input(Input::PointerMoved { x: 300.0, y: 250.0 }).unwrap();
    println!("move after focus loss -> {:?}", types(&ev));
    d.handle_input(Input::Focus(true)).unwrap();
    // Range drag: press on the thumb then lose focus, then move with no button.
    let rr = d.element_rect("rg").unwrap().unwrap();
    press(&mut d, rr.x + 2.0, rr.y + rr.height / 2.0, true);
    d.handle_input(Input::PointerMoved { x: rr.x + 100.0, y: rr.y + rr.height / 2.0 }).unwrap();
    println!("range during drag = {:?}", d.get_value("rg"));
    d.handle_input(Input::Focus(false)).unwrap();
    d.handle_input(Input::PointerMoved { x: rr.x + 250.0, y: rr.y + rr.height / 2.0 }).unwrap();
    println!("range after focus loss + buttonless move (should stay) = {:?}", d.get_value("rg"));
}

fn a11y_scale() {
    println!("== a11y_scale");
    let html = "<!doctype html><style>body{margin:0}#b{position:absolute;left:10px;top:20px;width:100px;height:30px}</style><button id=b>B</button>";
    let mut d = Document::parse_hosted(html, None, 800, 600).unwrap();
    d.set_viewport(800, 600, 2.0).unwrap();
    d.render(800, 600, 2.0).unwrap();
    let a = d.accessibility_json().unwrap();
    println!("tree scale = {}", a["scale"]);
    for n in find_nodes(&a, &|n| n["role"] == "Button") {
        println!("button bounds at scale 2 = {}", n["bounds"]);
    }
}

fn wheel_shift() {
    println!("== wheel_shift (Shift+wheel horizontal?)");
    let html = "<!doctype html><style>body{margin:0}#w{width:4000px;height:4000px}</style><div id=w></div>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    d.handle_input(Input::Modifiers { shift: true, ctrl: false, alt: false, meta: false }).unwrap();
    d.handle_input(Input::Wheel { dx: 0.0, dy: 40.0, x: 100.0, y: 100.0 }).unwrap();
    d.advance_clock(1000.0).unwrap();
    println!("after shift+wheel dy=40: {:?}", d.scroll_state(None).map(|s| format!("{s:?}")));
}


fn link_enter() {
    println!("== link_enter");
    let html = "<!doctype html><a id=l href='#x'>link</a><div role=button tabindex=0 id=rb>aria button</div><summary id=su>s</summary>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.render(400, 300, 1.0).unwrap();
    d.focus("l").unwrap();
    let ev = key(&mut d, "Enter", "Enter", false);
    println!("Enter on focused <a href> -> {:?}", types(&ev));
    d.focus("rb").unwrap();
    let ev = key(&mut d, "Enter", "Enter", false);
    println!("Enter on role=button tabindex=0 -> {:?}", types(&ev));
    let ev = key(&mut d, " ", "Space", false);
    println!("Space on role=button tabindex=0 -> {:?}", types(&ev));
}

fn a11y_perf() {
    println!("== a11y_perf");
    for n in [1000usize, 2000, 5000] {
        let mut html = String::from("<!doctype html><ul>");
        for i in 0..n {
            html.push_str(&format!("<li id=i{i}><button>Item {i}</button> text {i}</li>"));
        }
        html.push_str("</ul><p id=status>s</p>");
        let mut d = Document::parse_hosted(&html, None, 800, 600).unwrap();
        let t = std::time::Instant::now();
        d.render(800, 600, 1.0).unwrap();
        let first_render = t.elapsed();
        let t = std::time::Instant::now();
        let tree = d.accessibility_tree_titled("t").unwrap();
        let build = t.elapsed();
        let t = std::time::Instant::now();
        let upd = ui4me_renderer::a11y::full_update(&tree);
        let conv = t.elapsed();
        let t = std::time::Instant::now();
        let _ = d.accessibility_tree_titled("t").unwrap();
        let build_again = t.elapsed();
        let mut rows = Vec::new();
        for (label, target) in [("li i5", "i5"), ("p status", "status")] {
            d.set_text(target, "changed").unwrap();
            let t = std::time::Instant::now();
            let _ = d.element_rect("i0").unwrap();
            let flush = t.elapsed();
            let t = std::time::Instant::now();
            let _ = d.accessibility_tree_titled("t").unwrap();
            let rebuild = t.elapsed();
            let t = std::time::Instant::now();
            d.render(800, 600, 1.0).unwrap();
            let render = t.elapsed();
            rows.push(format!("[set_text {label}: flush={flush:?} a11y={rebuild:?} render={render:?}]"));
        }
        println!("n={n} a11y_nodes={} first_render={first_render:?} build={build:?} build_again_nochange={build_again:?} full_update={conv:?} ({} ak) {}", tree.nodes.len(), upd.nodes.len(), rows.join(" "));
    }
}

/// What the host does per animation tick: poll (advance clock) -> needs_repaint -> full a11y tree.
fn anim_tick_cost() {
    println!("== anim_tick_cost");
    for n in [300usize, 1000, 3000] {
        let mut html = String::from("<!doctype html><style>@keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}}#sp{width:20px;height:20px;background:red;animation:spin 1s linear infinite}</style><div id=sp></div><ul>");
        for i in 0..n {
            html.push_str(&format!("<li><button>Item {i}</button> text {i}</li>"));
        }
        html.push_str("</ul>");
        let mut d = Document::parse_hosted(&html, None, 800, 600).unwrap();
        d.use_manual_clock();
        d.render(800, 600, 1.0).unwrap();
        let _ = d.accessibility_tree_titled("t").unwrap();
        let (mut a11y, mut render, mut flush, mut ticks) = (std::time::Duration::ZERO, std::time::Duration::ZERO, std::time::Duration::ZERO, 0);
        for _ in 0..20 {
            let changed = d.advance_clock(16.0).unwrap();
            if !(changed && d.needs_repaint()) { continue; }
            ticks += 1;
            let t = std::time::Instant::now();
            let _ = d.element_rect("sp").unwrap();
            flush += t.elapsed();
            let t = std::time::Instant::now();
            let tree = d.accessibility_tree_titled("t").unwrap();
            let _ = ui4me_renderer::a11y::full_update(&tree);
            a11y += t.elapsed();
            let t = std::time::Instant::now();
            d.render(800, 600, 1.0).unwrap();
            render += t.elapsed();
        }
        println!("n={n}: repaint ticks={ticks}/20 avg flush={:?} avg a11y build+convert={:?} avg render={:?}", flush / ticks.max(1), a11y / ticks.max(1), render / ticks.max(1));
    }
}

fn ctrl_key(d: &mut Document, k: &str, code: &str) -> Vec<Value> {
    let mk = |pressed| Input::Key { key: k.into(), code: code.into(), pressed, repeat: false, shift: false, ctrl: true, alt: false, meta: false };
    let mut ev = d.handle_input(mk(true)).unwrap();
    ev.extend(d.handle_input(mk(false)).unwrap());
    ev
}

/// Shortcuts on a non-Latin layout: winit reports the layout's character as the logical key.
fn layout_shortcuts() {
    println!("== layout_shortcuts");
    let html = "<!doctype html><input id=i value='hello world'>";
    for (label, a, c) in [("latin", "a", "c"), ("russian", "\u{444}", "\u{441}")] {
        let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
        d.render(400, 300, 1.0).unwrap();
        d.focus("i").unwrap();
        d.set_control_selection(Some("i"), None, 0, 0, "none").ok();
        ctrl_key(&mut d, a, "KeyA");
        let sel = d.control_selection(Some("i"), None).unwrap();
        let ev = ctrl_key(&mut d, c, "KeyC");
        let reqs = d.take_clipboard_requests();
        println!("{label}: after Ctrl+A selection={} ; Ctrl+C events={:?} clipboard requests={}", sel, types(&ev), reqs.len());
    }
}

/// How many backend notifications ordinary pointer input generates (Issue #1 amplification).
fn event_volume() {
    println!("== event_volume");
    let html = "<!doctype html><style>body{margin:0}.r{display:flex}.c{width:40px;height:40px;padding:2px}</style>\
        <div id=list class=r>".to_string() + &(0..10).map(|i| format!("<div id=c{i} class=c><span id=s{i}>x</span></div>")).collect::<String>() + "</div>";
    let mut d = Document::parse_hosted(&html, None, 600, 300).unwrap();
    d.render(600, 300, 1.0).unwrap();
    let mut total = 0;
    let mut kinds = std::collections::BTreeMap::new();
    for step in 0..100 {
        let x = 1.0 + step as f64 * 4.4;
        let ev = d.handle_input(Input::PointerMoved { x, y: 20.0 }).unwrap();
        total += ev.len();
        for e in &ev { *kinds.entry(e["type"].as_str().unwrap_or("?").to_owned()).or_insert(0) += 1; }
    }
    println!("100 pointer motions across 10 boxes -> {total} document.event notifications {:?}", kinds);
    let mut total = 0;
    for _ in 0..20 {
        total += d.handle_input(Input::Wheel { dx: 0.0, dy: 40.0, x: 50.0, y: 20.0 }).unwrap().len();
    }
    println!("20 wheel ticks (nothing scrollable) -> {total} notifications");
}

/// Does moving focus with Tab reveal an off-screen element (browsers scroll it into view)?
fn tab_scroll() {
    println!("== tab_scroll");
    let html = "<!doctype html><style>body{margin:0}.gap{height:1500px}#sc{height:100px;overflow:auto}</style>\
        <button id=a>a</button><div class=gap></div><button id=b>b</button>\
        <div id=sc><div class=gap></div><button id=inner>inner</button></div>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    key(&mut d, "Tab", "Tab", false);
    key(&mut d, "Tab", "Tab", false);
    d.advance_clock(1000.0).unwrap();
    d.render(400, 300, 1.0).unwrap();
    d.advance_clock(1000.0).unwrap();
    d.render(400, 300, 1.0).unwrap();
    println!("focused={} viewport scroll={:?} rect(b)={:?}", d.focused_element()["element"], d.scroll_state(None).map(|s| s.y), d.element_rect("b").unwrap());
    key(&mut d, "Tab", "Tab", false);
    d.advance_clock(1000.0).unwrap();
    d.render(400, 300, 1.0).unwrap();
    d.advance_clock(1000.0).unwrap();
    d.render(400, 300, 1.0).unwrap();
    println!("focused={} viewport scroll={:?} sc scroll={:?} rect(inner)={:?}", d.focused_element()["element"], d.scroll_state(None).map(|s| s.y), d.scroll_state(Some("sc")).map(|s| s.y), d.element_rect("inner").unwrap());
    // Script/backend focus() of an off-screen element.
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.render(400, 300, 1.0).unwrap();
    d.focus("b").unwrap();
    d.render(400, 300, 1.0).unwrap();
    println!("backend focus(b): viewport scroll={:?}", d.scroll_state(None).map(|s| s.y));
    // Assistive (AT) focus of an off-screen element.
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.render(400, 300, 1.0).unwrap();
    let tree = d.accessibility_tree().unwrap();
    let b = tree.in_order().into_iter().find(|n| n.html_id.as_deref() == Some("b")).map(|n| n.id).unwrap();
    let req = ui4me_renderer::a11y::decode_request(&accesskit::ActionRequest { action: accesskit::Action::Focus, target_tree: accesskit::TreeId::ROOT, target_node: accesskit::NodeId(b), data: None });
    if let Some(r) = req { let _ = d.accessibility_request(&r); }
    d.render(400, 300, 1.0).unwrap();
    println!("AT focus(b): focused={} viewport scroll={:?}", d.focused_element()["element"], d.scroll_state(None).map(|s| s.y));
}

fn tab_scroll_input() {
    println!("== tab_scroll_input");
    let html = "<!doctype html><style>body{margin:0}.gap{height:1500px}</style><input id=a><div class=gap></div><input id=b>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.use_manual_clock();
    d.render(400, 300, 1.0).unwrap();
    key(&mut d, "Tab", "Tab", false);
    key(&mut d, "Tab", "Tab", false);
    d.advance_clock(1000.0).unwrap();
    d.render(400, 300, 1.0).unwrap();
    println!("text input: focused={} viewport scroll={:?}", d.focused_element()["element"], d.scroll_state(None).map(|s| s.y));
}

/// Text control inside a CSS-transformed container: hit test vs caret placement vs IME area.
fn transformed_input() {
    println!("== transformed_input");
    let html = "<!doctype html><style>body{margin:0}#w{position:absolute;left:0;top:0;transform:translateX(150px)}\
        input{font:16px monospace;width:200px;padding:0;border:0;margin:0}</style>\
        <div id=w><input id=i value='0123456789abcdef'></div>";
    let mut d = Document::parse_hosted(html, None, 600, 300).unwrap();
    d.render(600, 300, 1.0).unwrap();
    // Visual x of char ~5 is 150 + 5*9.6 = 198.
    println!("element_at_point(198,8) = {}", d.element_at_point(198.0, 8.0).unwrap());
    press(&mut d, 198.0, 8.0, true);
    press(&mut d, 198.0, 8.0, false);
    d.render(600, 300, 1.0).unwrap();
    println!("caret after click at visual char 5: {} ; ime cursor_area={:?}", d.control_selection(Some("i"), None).unwrap(), d.ime_request().map(|i| i.cursor_area));
}

/// <select> popup option picking inside a transformed and a fixed+scrolled container.
fn select_in_container() {
    println!("== select_in_container");
    let mk = |wrap: &str| format!("<!doctype html><style>body{{margin:0}}#tall{{height:3000px}}#w{{{wrap}}}select{{font:16px sans-serif;width:150px}}</style>\
        <div id=w><select id=s><option>one</option><option>two</option><option>three</option></select></div><div id=tall></div>");
    let plain = mk("position:absolute;left:0;top:0");
    let pick = |html: &str, scroll: f64, dx: f64, y: f64| -> (String, bool) {
        let mut d = Document::parse_hosted(html, None, 600, 400).unwrap();
        d.render(600, 400, 1.0).unwrap();
        if scroll > 0.0 { d.scroll_to(None, None, Some(scroll)).unwrap(); d.render(600, 400, 1.0).unwrap(); }
        let r = d.element_rect("s").unwrap().unwrap();
        let (cx, cy) = (r.x + 20.0 + dx, r.y + r.height / 2.0);
        // element_rect is what the engine believes; for the visual position we pass dx explicitly.
        press(&mut d, cx, cy, true); press(&mut d, cx, cy, false);
        d.render(600, 400, 1.0).unwrap();
        let open = String::new();
        press(&mut d, 20.0 + dx, y, true); press(&mut d, 20.0 + dx, y, false);
        (d.get_value("s").unwrap(), open.contains("open"))
    };
    // Find the y of option "two" in the untransformed layout.
    let mut y_two = None;
    for k in 0..60 {
        let y = 20.0 + k as f64 * 2.0;
        if pick(&plain, 0.0, 0.0, y).0 == "two" { y_two = Some(y); break; }
    }
    let Some(y) = y_two else { println!("could not locate option two"); return; };
    println!("plain: option two at y={y} -> {:?}", pick(&plain, 0.0, 0.0, y));
    // Same select translated 150px right: click the visually same option (x + 150).
    let tr = mk("position:absolute;left:0;top:0;transform:translateX(150px)");
    let mut d = Document::parse_hosted(&tr, None, 600, 400).unwrap();
    d.render(600, 400, 1.0).unwrap();
    let r = d.element_rect("s").unwrap().unwrap();
    println!("transformed: element_rect(s)={r:?} (visual x is +150)");
    // Open by clicking at the visual position, then click visual option two.
    press(&mut d, 170.0, r.y + r.height / 2.0, true); press(&mut d, 170.0, r.y + r.height / 2.0, false);
    d.render(600, 400, 1.0).unwrap();
    let px = d.render(600, 400, 1.0).unwrap();
    let ink = |x0: usize, x1: usize, y0: usize, y1: usize| (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| px[y * 600 + x] & 0xffffff != 0xffffff).count();
    println!("transformed: non-white pixels in popup band y 30..90: at x 0..150 = {}, at x 150..300 = {}", ink(0, 150, 30, 90), ink(150, 300, 30, 90));
    let a = d.accessibility_json().unwrap();
    println!("transformed: combobox states after open click = {}", find_nodes(&a, &|n| n["role"] == "ComboBox").first().map(|n| n["states"].to_string()).unwrap_or_default());
    press(&mut d, 170.0, y, true); press(&mut d, 170.0, y, false);
    println!("transformed: after clicking visual option two -> value={:?}", d.get_value("s"));
    // Fixed container, page scrolled by 400.
    let fx = mk("position:fixed;left:0;top:0");
    let mut d = Document::parse_hosted(&fx, None, 600, 400).unwrap();
    d.render(600, 400, 1.0).unwrap();
    d.scroll_to(None, None, Some(400.0)).unwrap();
    d.render(600, 400, 1.0).unwrap();
    let r = d.element_rect("s").unwrap().unwrap();
    println!("fixed+scrolled: element_rect(s)={r:?} (visual y is unchanged)");
    press(&mut d, 20.0, 10.0, true); press(&mut d, 20.0, 10.0, false);
    d.render(600, 400, 1.0).unwrap();
    let a = d.accessibility_json().unwrap();
    println!("fixed+scrolled: combobox states after open click = {}", find_nodes(&a, &|n| n["role"] == "ComboBox").first().map(|n| n["states"].to_string()).unwrap_or_default());
    press(&mut d, 20.0, y, true); press(&mut d, 20.0, y, false);
    println!("fixed+scrolled: after clicking visual option two -> value={:?}", d.get_value("s"));
}

/// position:fixed inside a transformed ancestor (the ancestor is its containing block, so it scrolls).
fn fixed_in_transform() {
    println!("== fixed_in_transform");
    let html = "<!doctype html><style>body{margin:0}#t{transform:translateX(0);height:3000px}\
        #f{position:fixed;top:10px;left:0;width:100px;height:40px}#c{height:20px}</style>\
        <div id=t><div id=f><div id=c>c</div></div></div>";
    let mut d = Document::parse_hosted(html, None, 400, 300).unwrap();
    d.render(400, 300, 1.0).unwrap();
    d.scroll_to(None, None, Some(400.0)).unwrap();
    d.render(400, 300, 1.0).unwrap();
    println!("after scroll 400: element_rect(f)={:?} element_rect(c)={:?}", d.element_rect("f").unwrap(), d.element_rect("c").unwrap());
    println!("hit at (50,20)={} ; hit at (50,-) n/a", d.element_at_point(50.0, 20.0).unwrap());
}

fn main() {
    let which: Vec<String> = std::env::args().skip(1).collect();
    let all = which.is_empty();
    let run = |n: &str| all || which.iter().any(|w| w == n);
    if run("fixed") { fixed_textarea(); }
    if run("click") { click_across_children(); }
    if run("transform") { transformed_bounds(); transformed_input(); }
    if run("tab") { tab_order(); }
    if run("keys") { radio_and_select_keys(); }
    if run("capture") { focus_loss_capture(); }
    if run("scale") { a11y_scale(); }
    if run("wheel") { wheel_shift(); }
    if run("link") { link_enter(); }
    if run("perf") { a11y_perf(); }
    if run("anim") { anim_tick_cost(); }
    if run("layout") { layout_shortcuts(); }
    if run("volume") { event_volume(); }
    if run("select") { select_in_container(); }
    if run("fixtr") { fixed_in_transform(); }
    if run("tabscroll") { tab_scroll(); tab_scroll_input(); }
}
