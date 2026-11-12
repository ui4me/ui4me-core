//! Application-facing regressions: the production facade, actual scene, and
//! production interaction algorithms, with no alternate headless editor.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

fn key(key: &str, ctrl: bool, shift: bool) -> Input {
    Input::Key {
        key: key.into(),
        code: key.into(),
        pressed: true,
        repeat: false,
        shift,
        ctrl,
        alt: false,
        meta: false,
    }
}
fn press(d: &mut Document, name: &str) -> Vec<Value> {
    d.handle_input(key(name, false, false)).unwrap()
}
fn rect(d: &mut Document, element: &str) -> (f64, f64, f64, f64) {
    let node = d.node(element).unwrap();
    d.element_content_rect(d.id(), node).unwrap().unwrap()
}
fn click(d: &mut Document, element: &str) -> Vec<Value> {
    let (x, y, w, h) = rect(d, element);
    let (x, y) = (x + w / 2.0, y + h / 2.0);
    let mut events = d
        .handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x,
            y,
        })
        .unwrap();
    events.extend(
        d.handle_input(Input::PointerButton {
            pressed: false,
            button: 0,
            x,
            y,
        })
        .unwrap(),
    );
    events
}

#[test]
fn nested_flex_layout_and_css_mutations_change_real_pixels() {
    let html = r#"<!doctype html><style>
        html,body{margin:0} #row{display:flex;gap:10px;padding:8px;background:#eee}
        #red{width:30px;height:24px;background:#ff0000} #blue{width:40px;height:24px;background:#0000ff}
        #blue.changed{background:#00ff00}
        </style><section id=row><div id=red></div><div id=blue></div></section>"#;
    let mut d = Document::parse(html).unwrap();
    let pixels = d.render(200, 80, 1.0).unwrap();
    assert_eq!(pixels[10 * 200 + 10], 0x00ff0000);
    assert_eq!(pixels[10 * 200 + 50], 0x000000ff);
    assert_eq!(rect(&mut d, "blue").0, 48.0);
    d.add_class("blue", "changed").unwrap();
    assert_eq!(d.render(200, 80, 1.0).unwrap()[10 * 200 + 50], 0x0000ff00);
    d.remove_class("blue", "changed").unwrap();
    assert_eq!(d.render(200, 80, 1.0).unwrap()[10 * 200 + 50], 0x000000ff);
    let scaled = d.render(400, 160, 2.0).unwrap();
    assert_eq!(scaled.len(), 400 * 160);
    assert_eq!(scaled[20 * 400 + 100], 0x000000ff);
}

#[test]
fn text_shapes_unicode_and_set_text_remains_literal() {
    let mut d = Document::parse("<!doctype html><style>body{margin:0;font-size:24px;color:#111}</style><p id=p>Žluťoučký kůň — Ελληνικά</p>").unwrap();
    let before = d.render(400, 100, 1.0).unwrap();
    assert!(before.iter().filter(|&&p| p != 0x00ff_ffff).count() > 100);
    d.set_text("p", "<b>literal & text</b>").unwrap();
    assert_eq!(d.get_value("p").unwrap(), "<b>literal & text</b>");
    let after = d.render(400, 100, 1.0).unwrap();
    assert_ne!(before, after);
    assert!(d.pipeline.document().element_by_id("b").is_none());
}

#[test]
fn invalid_subtree_replacement_is_atomic_and_old_events_cannot_retarget() {
    let mut d = Document::parse(
        "<!doctype html><div id=host><input id=edit value=old></div><p id=other>outside</p>",
    )
    .unwrap();
    d.focus("edit").unwrap();
    d.drain_events();
    let events = d.handle_input(Input::Text("!".into())).unwrap();
    let input = events
        .iter()
        .find(|e| e["type"] == "input")
        .unwrap()
        .clone();
    assert!(d.event_is_current(&input));
    let old = d.node("edit").unwrap();
    let revision = d.pipeline.document().revision();
    assert!(d.replace_children_html("host", "<input id=other>").is_err());
    assert_eq!(d.node("edit").unwrap(), old);
    assert_eq!(d.pipeline.document().revision(), revision);
    assert_eq!(d.get_value("edit").unwrap(), "old!");
    assert!(d.event_is_current(&input));
    assert!(
        d.replace_children_html("host", "<script>bad()</script>")
            .is_err()
    );
    d.replace_children_html("host", "<input id=edit value=new>")
        .unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "new");
    assert!(!d.event_is_current(&input));
    assert_eq!(d.interaction.focused_element(d.id()), None);
    assert!(d.blink.is_none());
}

#[test]
fn accepted_text_altgr_grapheme_deletion_and_committed_change_share_one_value() {
    let mut d =
        Document::parse("<!doctype html><input id=edit><button id=next>Next</button>").unwrap();
    d.focus("edit").unwrap();
    d.drain_events();
    d.handle_input(Input::Modifiers {
        shift: false,
        ctrl: true,
        alt: true,
        meta: false,
    })
    .unwrap();
    let events = d.handle_input(Input::Text("é😀@".into())).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "é😀@");
    assert_eq!(events.iter().filter(|e| e["type"] == "input").count(), 1);
    assert_eq!(events.iter().filter(|e| e["type"] == "change").count(), 1);
    press(&mut d, "Backspace");
    assert_eq!(d.get_value("edit").unwrap(), "é😀");
    press(&mut d, "Backspace");
    assert_eq!(d.get_value("edit").unwrap(), "é");
    d.focus("next").unwrap();
    let events = d.drain_events();
    assert_eq!(events.iter().filter(|e| e["type"] == "commit").count(), 1);
    assert_eq!(
        events.iter().find(|e| e["type"] == "commit").unwrap()["value"],
        "é"
    );
}

#[test]
fn readonly_disabled_and_hidden_controls_are_not_editable_or_tab_stops() {
    let mut d = Document::parse("<!doctype html><input id=a><input id=disabled disabled tabindex=1><div style='display:none'><input id=hidden></div><input id=readonly readonly value=locked><button id=b>Done</button>").unwrap();
    assert!(d.focus("disabled").is_err());
    assert!(d.focus("hidden").is_err());
    d.focus("readonly").unwrap();
    d.handle_input(Input::Text("no".into())).unwrap();
    assert_eq!(d.get_value("readonly").unwrap(), "locked");
    d.focus("a").unwrap();
    d.drain_events();
    let events = press(&mut d, "Tab");
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "focus" && e["element"] == "readonly")
    );
    press(&mut d, "Tab");
    assert_eq!(
        d.interaction.focused_element(d.id()),
        Some(d.node("b").unwrap())
    );
}

#[test]
fn pointer_and_keyboard_activation_use_scene_geometry() {
    let mut d = Document::parse("<!doctype html><style>#go{margin:40px;padding:18px}#off{margin:10px}</style><button id=go>Go</button><button id=off disabled>Disabled</button>").unwrap();
    let events = click(&mut d, "go");
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "mousedown" && e["element"] == "go")
    );
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "mouseup" && e["element"] == "go")
    );
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "click" && e["element"] == "go")
    );
    d.focus("go").unwrap();
    let events = press(&mut d, "Enter");
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "click" && e["element"] == "go")
    );
    let events = click(&mut d, "off");
    assert!(
        !events
            .iter()
            .any(|e| e["type"] == "click" && e["element"] == "off")
    );
}

#[test]
fn window_focus_pauses_the_caret_without_discarding_element_focus() {
    let mut d = Document::parse("<input id=field value=kept>").unwrap();
    d.focus("field").unwrap();
    d.render(200, 80, 1.0).unwrap();
    let field = d.node("field").unwrap();
    assert!(d.blink.as_ref().unwrap().visible);
    assert!(d.ime_request().is_some());
    d.handle_input(Input::Focus(false)).unwrap();
    assert_eq!(d.interaction.focused_element(d.id()), Some(field));
    assert!(!d.blink.as_ref().unwrap().visible);
    assert!(d.ime_request().is_none());
    assert!(d.next_wake_in().is_none());
    d.handle_input(Input::Focus(true)).unwrap();
    assert!(d.blink.as_ref().unwrap().visible);
    assert!(d.ime_request().is_some());
    assert!(d.next_wake_in().is_some());
    assert_eq!(d.get_value("field").unwrap(), "kept");
}

#[test]
fn nested_button_content_keeps_its_subscription_path_and_identity() {
    let mut d = Document::parse("<!doctype html><button id=go><span id=caption>Nested label</span></button><button id=off disabled><span>Disabled label</span></button>").unwrap();
    let events = click(&mut d, "go");
    let event = events.iter().find(|e| e["type"] == "click").unwrap();
    assert!(
        event["path"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == "go")
    );
    assert!(d.event_is_current(event));
    d.set_text("go", "Replaced label").unwrap();
    // The hit may be the button itself or its label depending on glyph location.
    // A descendant hit must never transfer to a replacement descendant.
    if event["element"] == "caption" {
        assert!(!d.event_is_current(event));
    }
    assert!(!click(&mut d, "off").iter().any(|e| e["type"] == "click"));
}

#[test]
fn paging_and_motion_are_promoted() {
    let mut d = Document::parse(r#"<!doctype html><style>
        @page invoice { size:20px 20px; }
        @keyframes fade { from{opacity:0}to{opacity:1} }
        body{margin:0}#box{width:24px;height:24px;background:#ff0000;transition:background-color 1s;animation:fade 2s;-webkit-transition:background-color 1s;-webkit-animation:fade 2s;page:invoice}
        #box.on{background:#0000ff}
        </style><div id=box></div>"#).unwrap();
    // The fade-in animation starts at opacity 0 (0.9 F0901).
    assert_eq!(d.render(100, 80, 1.0).unwrap()[5 * 100 + 5], 0x00ffffff);
    d.add_class("box", "on").unwrap();
    // Transitions are promoted (0.9 F0901): the change is a transition, not an instant swap.
    assert_eq!(d.render(100, 80, 1.0).unwrap()[5 * 100 + 5], 0x00ffffff);
    assert!(
        !d.diagnostics()
            .iter()
            .any(|e| e["feature"] == "css.property.transition")
    );
    // `page` is supported since 1.0 F1001 (named pages in pagination).
    assert!(
        !d.diagnostics()
            .iter()
            .any(|e| e["feature"] == "css.property.page")
    );
}

#[test]
fn checkbox_and_radio_activation_update_the_canonical_model() {
    let mut d = Document::parse("<!doctype html><input type=checkbox id=c><input type=radio name=g id=a checked><input type=radio name=g id=b>").unwrap();
    let events = click(&mut d, "c");
    assert!(d.get_checked("c").unwrap());
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "change" && e["checked"] == true)
    );
    d.set_checked("b", true).unwrap();
    assert!(!d.get_checked("a").unwrap());
    assert!(d.get_checked("b").unwrap());
    click(&mut d, "a");
    assert!(d.get_checked("a").unwrap());
    assert!(!d.get_checked("b").unwrap());
}

#[derive(Clone)]
struct TestClipboard(Rc<RefCell<String>>);
impl Clipboard for TestClipboard {
    fn read_text(&mut self) -> std::result::Result<Option<String>, String> {
        Ok(Some(self.0.borrow().clone()))
    }
    fn write_text(&mut self, text: &str) -> std::result::Result<(), String> {
        *self.0.borrow_mut() = text.into();
        Ok(())
    }
}

#[test]
fn selection_clipboard_undo_and_redo_use_the_same_editing_engine() {
    let shared = Rc::new(RefCell::new(String::new()));
    let mut d = Document::parse("<!doctype html><textarea id=t>Český text</textarea>").unwrap();
    d.set_clipboard(Box::new(TestClipboard(shared.clone())));
    d.focus("t").unwrap();
    d.handle_input(key("a", true, false)).unwrap();
    d.handle_input(key("c", true, false)).unwrap();
    assert_eq!(&*shared.borrow(), "Český text");
    d.handle_input(key("x", true, false)).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "");
    d.handle_input(key("z", true, false)).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "Český text");
    d.handle_input(key("z", true, true)).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "");
    d.handle_input(key("v", true, false)).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "Český text");
}

#[test]
fn ime_commit_replaces_preedit_once_and_uses_utf8_cursor_offsets() {
    let mut d = Document::parse("<!doctype html><input id=t>").unwrap();
    d.focus("t").unwrap();
    d.handle_input(Input::ImeEnabled).unwrap();
    d.handle_input(Input::ImePreedit {
        text: "ž😀".into(),
        cursor: Some((2, 6)),
    })
    .unwrap();
    let state = d
        .pipeline
        .document()
        .control_state(d.node("t").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(state.value.as_deref(), Some("ž😀"));
    press(&mut d, "Backspace");
    press(&mut d, "Enter");
    assert_eq!(
        d.get_value("t").unwrap(),
        "ž😀",
        "the IME owns preedit editing keys"
    );
    d.handle_input(Input::ImeCommit("žluťoučký".into()))
        .unwrap();
    assert_eq!(d.get_value("t").unwrap(), "žluťoučký");
    d.handle_input(Input::ImeDisabled).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "žluťoučký");
}

#[test]
fn ending_a_composition_inside_the_window_resets_the_platform_once() {
    // 0.5 F0501: the platform still holds its preedit when the document ends the composition
    // (a focus move to another field, a host value write, a pointer press); it must be told to drop it, or
    // its later commit lands a second time in the next control.
    let mut d = Document::parse("<!doctype html><input id=a><input id=b>").unwrap();
    d.render(300, 200, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    d.focus("a").unwrap();
    d.handle_input(Input::ImeEnabled).unwrap();
    let base = d.ime_reset_count();
    d.handle_input(Input::ImePreedit {
        text: "にほん".into(),
        cursor: Some((9, 9)),
    })
    .unwrap();
    // Keys while composing belong to the IME: Tab does not move the focus.
    press(&mut d, "Tab");
    assert_eq!(d.ime_reset_count(), base);
    d.drain_events();
    d.focus("b").unwrap();
    let events = d.drain_events();
    assert_eq!(
        d.get_value("a").unwrap(),
        "にほん",
        "the composed text is kept"
    );
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "compositionend" && e["data"] == "にほん"),
        "{events:?}"
    );
    assert_eq!(
        d.ime_reset_count(),
        base + 1,
        "a focus move inside the window resets the platform"
    );
    assert!(
        d.ime_request().is_some(),
        "the next field requests the IME again"
    );
    // A commit with no composition in progress is ordinary typed text in the new field.
    d.handle_input(Input::ImeCommit("日本".into())).unwrap();
    assert_eq!(d.get_value("b").unwrap(), "日本");
    assert_eq!(d.get_value("a").unwrap(), "にほん");
    // A host value write abandons the composition and resets the platform.
    d.handle_input(Input::ImePreedit {
        text: "한".into(),
        cursor: Some((3, 3)),
    })
    .unwrap();
    d.set_value("b", "reset").unwrap();
    assert_eq!(d.ime_reset_count(), base + 2);
    // A composition that ends normally (commit) does not reset the platform.
    d.handle_input(Input::ImePreedit {
        text: "中".into(),
        cursor: Some((3, 3)),
    })
    .unwrap();
    d.handle_input(Input::ImeCommit("中文".into())).unwrap();
    assert_eq!(d.ime_reset_count(), base + 2);
    // The window losing focus ends it without a reset: the platform has committed or cleared it.
    d.handle_input(Input::ImePreedit {
        text: "x".into(),
        cursor: Some((1, 1)),
    })
    .unwrap();
    d.handle_input(Input::Focus(false)).unwrap();
    assert_eq!(d.ime_reset_count(), base + 2);
}

#[test]
fn scrolling_moves_hit_geometry_and_subtree_replacement_retires_it() {
    let mut d = Document::parse("<!doctype html><style>body{margin:0}#s{height:80px;width:180px;overflow:auto}#pad{height:150px}#go{width:80px;height:30px}</style><div id=s><div id=pad></div><button id=go>Go</button></div>").unwrap();
    d.render(200, 200, 1.0).unwrap();
    let before = rect(&mut d, "go").1;
    let events = d
        .handle_input(Input::Wheel {
            dx: 0.0,
            dy: 150.0,
            x: 50.0,
            y: 40.0,
        })
        .unwrap();
    assert!(events.iter().any(|e| e["type"] == "scroll"));
    assert!(rect(&mut d, "go").1 < before);
    assert!(
        click(&mut d, "go")
            .iter()
            .any(|e| e["type"] == "click" && e["element"] == "go")
    );
    d.replace_children_html("s", "<button id=go>New</button>")
        .unwrap();
    assert_eq!(d.get_value("go").unwrap(), "New");
}

#[test]
fn local_stylesheet_image_and_font_resolve_from_the_document_directory() {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let root = std::env::temp_dir().join(format!(
        "ui4me-renderer-assets-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("app.css"), "@font-face{font-family:AppFont;src:url(font.ttf)}body{margin:0;font-family:AppFont}#color{width:20px;height:20px;background:#123456}img{display:block;width:24px;height:24px}").unwrap();
    std::fs::write(root.join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><path fill="#ff00ff" d="M0 0h24v24H0z"/></svg>"##).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/fonts/noto-sans/NotoSans-Regular.ttf"),
        root.join("font.ttf"),
    )
    .unwrap();
    let mut d = Document::parse_with_source("<!doctype html><link rel=stylesheet href=app.css><div id=color></div><img src=icon.svg><p>Local font</p>", Some(&root), 160, 120).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let pixels = d.render(160, 120, 1.0).unwrap();
        if pixels[5 * 160 + 5] == 0x00123456
            && pixels[25 * 160 + 5] == 0x00ff00ff
            && !d.has_pending_resources()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "assets did not settle: {:?}",
            d.diagnostics()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let faces = lock(&d.resources).document_faces(d.id()).unwrap();
    assert!(
        faces.iter().any(|f| f.family == "AppFont"
            && matches!(f.state, ui4me_assets::fonts::FaceState::Loaded(_)))
    );
    drop(d);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn limits_and_active_content_fail_before_use() {
    assert!(Document::parse("<script>alert(1)</script>").is_err());
    assert!(Document::parse("<button onclick='bad()'>No</button>").is_err());
    assert!(Document::parse("<div id=x></div><div id=x></div>").is_err());
    let deep = format!("{}x{}", "<div>".repeat(257), "</div>".repeat(257));
    assert_eq!(Document::parse(&deep).unwrap_err().code, 5);
    let mut d = Document::parse("<p id=p>ok</p>").unwrap();
    assert_eq!(d.render(0, 100, 1.0).unwrap_err().code, 5);
    assert_eq!(d.render(5000, 5000, 1.0).unwrap_err().code, 5);
    assert!(d.set_text("p", &"x".repeat(MAX_TEXT + 1)).is_err());
    assert_eq!(d.get_value("p").unwrap(), "ok");
}

#[test]
fn unsupported_pickers_are_rejected_without_mutating_the_live_document() {
    let mut d = Document::parse("<section id=host><input id=field value=kept></section>").unwrap();
    let field = d.node("field").unwrap();
    for kind in ["color", "date", "time", "month", "week", "datetime-local"] {
        let html = format!("<input type='{kind}'>");
        let error = Document::parse(&html).unwrap_err();
        assert_eq!(error.code, 1);
        assert!(error.message.contains("requires a native picker"));
        assert!(d.set_attribute("field", "type", Some(kind)).is_err());
        assert_eq!(
            d.pipeline.document().input_type(field),
            Some(InputType::Text)
        );
        assert!(d.replace_children_html("host", &html).is_err());
        assert_eq!(d.node("field").unwrap(), field);
        assert_eq!(d.get_value("field").unwrap(), "kept");
    }
}

fn button(d: &mut Document, pressed: bool, x: f64, y: f64) {
    d.handle_input(Input::PointerButton {
        pressed,
        button: 0,
        x,
        y,
    })
    .unwrap();
}

#[test]
fn document_text_selection_drag_copy_and_controls() {
    let mut d = Document::parse(
        "<!doctype html><style>body{margin:0;font-size:20px} p{margin:0;height:40px}</style>\
         <p id=a>Alpha beta</p><p id=b>Gamma delta</p><input id=pw type=password value=secret><p id=c>Omega</p>",
    )
    .unwrap();
    d.render(640, 480, 1.0).unwrap();
    let a = d.element_rect("a").unwrap().unwrap();
    let b = d.element_rect("b").unwrap().unwrap();
    // Drag from the start of "Alpha" to the end of the second paragraph's text.
    button(&mut d, true, a.x + 1.0, a.y + 10.0);
    d.handle_input(Input::PointerMoved {
        x: b.x + 600.0,
        y: b.y + 10.0,
    })
    .unwrap();
    button(&mut d, false, b.x + 600.0, b.y + 10.0);
    assert_eq!(d.selection_text().unwrap(), "Alpha beta\nGamma delta");
    // The highlight paints only inside the selection.
    let plain = {
        let mut e = Document::parse("<!doctype html><style>body{margin:0;font-size:20px} p{margin:0;height:40px}</style><p id=a>Alpha beta</p><p id=b>Gamma delta</p><input id=pw type=password value=secret><p id=c>Omega</p>").unwrap();
        e.render(640, 480, 1.0).unwrap()
    };
    let selected = d.render(640, 480, 1.0).unwrap();
    let row = |y: f64| (y as usize) * 640;
    assert!((0..40).any(|x| selected[row(a.y + 10.0) + x] != plain[row(a.y + 10.0) + x]));
    assert_eq!(
        selected[row(b.y + 30.0) + 630],
        plain[row(b.y + 30.0) + 630],
        "outside the text"
    );
    // Ctrl+C copies the document selection.
    d.handle_input(key("c", true, false)).unwrap();
    assert_eq!(
        d.clipboard_text().unwrap().as_deref(),
        Some("Alpha beta\nGamma delta")
    );
    // Mutating another paragraph keeps the selection.
    d.set_text("c", "Changed").unwrap();
    assert_eq!(d.selection_text().unwrap(), "Alpha beta\nGamma delta");
    // Ctrl+A selects all document text, never the password value.
    d.handle_input(key("a", true, false)).unwrap();
    let all = d.selection_text().unwrap();
    assert!(
        all.starts_with("Alpha beta") && all.ends_with("Changed"),
        "{all:?}"
    );
    assert!(!all.contains("secret"));
    // Double-click selects a word; a press on a control clears the document selection.
    let (x, y) = (a.x + 90.0, a.y + 10.0);
    button(&mut d, true, x, y);
    button(&mut d, false, x, y);
    button(&mut d, true, x, y);
    button(&mut d, false, x, y);
    assert_eq!(d.selection_text().unwrap(), "beta");
    let mut b2 =
        Document::parse("<!doctype html><p id=t>Keep me</p><button id=copy>Copy</button>").unwrap();
    b2.render(640, 480, 1.0).unwrap();
    b2.select_text(Some("t")).unwrap();
    let r = b2.element_rect("copy").unwrap().unwrap();
    button(&mut b2, true, r.x + 4.0, r.y + 4.0);
    button(&mut b2, false, r.x + 4.0, r.y + 4.0);
    assert_eq!(
        b2.selection_text().unwrap(),
        "Keep me",
        "a button press keeps the selection"
    );
    let pw = d.element_rect("pw").unwrap().unwrap();
    button(&mut d, true, pw.x + 5.0, pw.y + 5.0);
    button(&mut d, false, pw.x + 5.0, pw.y + 5.0);
    assert_eq!(d.selection_text().unwrap(), "");
    // Removing the selected paragraph clears the selection.
    d.select_text(Some("b")).unwrap();
    assert_eq!(d.selection_text().unwrap(), "Gamma delta");
    d.remove_element("b").unwrap();
    assert_eq!(d.selection_text().unwrap(), "");
}

#[test]
#[ignore = "measurement"]
fn measure_long_log_append() {
    let n: usize = std::env::var("LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let mut d =
        Document::parse("<!doctype html><div id=log style='height:400px;overflow:auto'></div>")
            .unwrap();
    d.set_scroll_follow(Some("log"), true).unwrap();
    let start = std::time::Instant::now();
    let (mut ins, mut ren, mut worst_ins, mut worst_ren) = (
        std::time::Duration::ZERO,
        std::time::Duration::ZERO,
        std::time::Duration::ZERO,
        std::time::Duration::ZERO,
    );
    for i in 0..n {
        let t = std::time::Instant::now();
        d.insert_html(
            "log",
            "beforeend",
            &format!("<div>line {i} lorem ipsum dolor sit amet</div>"),
        )
        .unwrap();
        let e = t.elapsed();
        ins += e;
        worst_ins = worst_ins.max(e);
        if i % 50 == 0 {
            let t = std::time::Instant::now();
            d.render(800, 600, 1.0).unwrap();
            let e = t.elapsed();
            ren += e;
            worst_ren = worst_ren.max(e);
        }
        if i % 1000 == 999 {
            eprintln!(
                "{}: total {:?} insert {:?} (worst {:?}) render {:?} (worst {:?})",
                i + 1,
                start.elapsed(),
                ins,
                worst_ins,
                ren,
                worst_ren
            );
            (ins, ren, worst_ins, worst_ren) = Default::default();
        }
    }
    let t = std::time::Instant::now();
    d.render(800, 600, 1.0).unwrap();
    eprintln!("final render {:?}; one more append+render:", t.elapsed());
    for _ in 0..3 {
        let t = std::time::Instant::now();
        d.insert_html("log", "beforeend", "<div>x</div>").unwrap();
        let a = t.elapsed();
        d.pipeline.flush().unwrap();
        let b = t.elapsed();
        let stages = d.pipeline.last_stage_ms;
        d.render(800, 600, 1.0).unwrap();
        eprintln!(
            "  insert {a:?} flush {:?} render {:?} stats {stages:?}",
            b - a,
            t.elapsed() - b
        );
    }
}

/// S5: a 10k-line log streamed through a bounded window and three chat views streaming tokens
/// keep the DOM, the arena and the selection bounded and consistent.
#[test]
fn sustained_log_and_chat_streams_stay_bounded() {
    let mut log = Document::parse("<!doctype html><style>body{margin:0}</style><div id=log style='height:300px;overflow:auto'></div>").unwrap();
    log.set_scroll_follow(Some("log"), true).unwrap();
    let mut chats: Vec<Document> = (0..3)
        .map(|_| Document::parse("<!doctype html><div id=msgs style='height:300px;overflow:auto'></div><input id=search value=keep>").unwrap())
        .collect();
    for c in &mut chats {
        c.set_scroll_follow(Some("msgs"), true).unwrap();
        c.focus("search").unwrap();
    }
    let mut peak = 0;
    for i in 0..10_000 {
        log.apply_batch(&[
            DomOp::InsertHtml {
                element: "log".into(),
                position: "beforeend".into(),
                html: format!("<div>line {i} ✓ žluťoučký</div>"),
            },
            DomOp::TrimChildren {
                element: "log".into(),
                keep: 300,
                from_start: true,
            },
        ])
        .unwrap();
        for (n, c) in chats.iter_mut().enumerate() {
            if i % 40 == 0 {
                c.insert_html("msgs", "beforeend", &format!("<p id=m{n}x{i}></p>"))
                    .unwrap();
                c.trim_children("msgs", 50, true).unwrap();
            }
            let last = format!("m{n}x{}", i / 40 * 40);
            c.append_text(&last, "tok ").unwrap();
        }
        if i % 500 == 0 {
            log.render(400, 300, 1.0).unwrap();
            for c in &mut chats {
                c.render(400, 300, 1.0).unwrap();
            }
            if i == 5000 {
                log.select_text(Some("log")).unwrap();
            }
        }
        peak = peak.max(log.pipeline.document().arena().live_count());
    }
    assert!(peak < 2_000, "arena grows: {peak}");
    let (ids, count) = log.query_ids("#log > div", 1).unwrap();
    assert!(ids.is_empty());
    assert_eq!(count, 300);
    assert_eq!(log.get_text("log").unwrap().matches("line 9999").count(), 1);
    assert_eq!(
        log.scroll_state(Some("log")).unwrap().y,
        log.scroll_state(Some("log")).unwrap().max_y
    );
    for c in &mut chats {
        assert!(c.pipeline.document().arena().live_count() < 1_000);
        assert_eq!(c.get_value("search").unwrap(), "keep");
        assert_eq!(
            c.query_ids(":focus", 4).unwrap().0,
            vec!["search".to_owned()]
        );
        assert_eq!(
            c.get_text("m0x9960")
                .or_else(|_| c.get_text("m1x9960"))
                .or_else(|_| c.get_text("m2x9960"))
                .unwrap(),
            "tok ".repeat(40)
        );
    }
    // A long streamed message is split into bounded text nodes.
    let mut d = Document::parse("<p id=m></p>").unwrap();
    for _ in 0..20_000 {
        d.append_text("m", "0123456789").unwrap();
    }
    assert_eq!(d.get_text("m").unwrap().len(), 200_000);
    assert!(d.append_text("m", &"x".repeat(MAX_TEXT + 1)).is_err());
}

#[test]
fn sub_pixel_overflow_is_not_scrollable() {
    let mut d = Document::parse("<!doctype html><style>body{margin:0}#b{height:100px;overflow:auto}#c{height:100.4px}</style><div id=b><div id=c></div></div>").unwrap();
    d.render(400, 300, 1.0).unwrap();
    let s = d.scroll_state(Some("b")).unwrap();
    assert_eq!((s.max_y, s.scrollable), (0.0, true));
}

#[test]
fn selection_drag_autoscrolls_and_reports_changes() {
    let rows: String = (0..60)
        .map(|i| format!("<p style='margin:0;height:20px'>row {i}</p>"))
        .collect();
    let mut d = Document::parse(&format!("<!doctype html><style>body{{margin:0;font-size:14px}}#box{{height:100px;overflow:auto}}</style><div id=box>{rows}</div><p id=after>after</p>")).unwrap();
    d.render(400, 300, 1.0).unwrap();
    let r = d.element_rect("box").unwrap().unwrap();
    let changes = |events: &[Value]| {
        events
            .iter()
            .filter(|e| e["type"] == "selectionchange")
            .count()
    };
    let ev = d
        .handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x: r.x + 2.0,
            y: r.y + 5.0,
        })
        .unwrap();
    assert_eq!(
        changes(&ev),
        1,
        "a press that sets a caret point is a change"
    );
    let mut moved = 0;
    for _ in 0..10 {
        let ev = d
            .handle_input(Input::PointerMoved {
                x: r.x + 50.0,
                y: r.y + r.height + 30.0,
            })
            .unwrap();
        moved += changes(&ev);
    }
    d.handle_input(Input::PointerButton {
        pressed: false,
        button: 0,
        x: r.x + 50.0,
        y: r.y + r.height + 30.0,
    })
    .unwrap();
    let s = d.scroll_state(Some("box")).unwrap();
    assert!(s.y > 100.0, "dragging below the box scrolled it: {}", s.y);
    assert!(d.selection_text().unwrap().contains("row 10"));
    assert!(moved >= 1);
    d.clear_selection();
    assert_eq!(changes(&d.drain_events()), 1);
    d.clear_selection();
    assert_eq!(changes(&d.drain_events()), 0, "no change, no event");
}

#[test]
fn drag_past_line_end_stays_in_its_column() {
    let mut d = Document::parse("<!doctype html><style>body{margin:0;font-size:14px;display:flex}#a{width:300px}#b{width:300px}p{margin:0}</style>\
        <div id=a><p id=m0>Short text</p><p id=m1>second message</p></div><div id=b><p id=l0>log line zero is here</p><p id=l1>log line one</p></div>").unwrap();
    d.render(640, 200, 1.0).unwrap();
    let r = d.element_rect("m0").unwrap().unwrap();
    button(&mut d, true, r.x + 1.0, r.y + r.height / 2.0);
    // Past the end of "Short text" but inside #a: must not jump to the log column on the right.
    d.handle_input(Input::PointerMoved {
        x: r.x + 280.0,
        y: r.y + r.height / 2.0,
    })
    .unwrap();
    button(&mut d, false, r.x + 280.0, r.y + r.height / 2.0);
    assert_eq!(d.selection_text().unwrap(), "Short text");
}

// ---- F0605: constraint validation and form submission ----------------------------------------

fn flags(d: &mut Document, id: &str) -> Vec<String> {
    let v = d.get_validity(id).unwrap();
    v["flags"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, on)| on.as_bool() == Some(true))
        .map(|(k, _)| k.clone())
        .collect()
}
fn of_type<'a>(events: &'a [Value], ty: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["type"] == ty).collect()
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

#[test]
fn constraint_flags_cover_the_supported_attributes() {
    let mut d = Document::parse(
        "<!doctype html><form id=f>\
        <input id=req required>\
        <input id=mail type=email>\
        <input id=url type=url>\
        <input id=pat pattern='[a-z]+' title='lowercase'>\
        <input id=len minlength=3 maxlength=5>\
        <input id=num type=number min=2 max=8 step=2>\
        <input id=chk type=checkbox required>\
        <input id=ra type=radio name=g required><input id=rb type=radio name=g>\
        <select id=sel required><option value=''>pick</option><option value=x>X</option></select>\
        <textarea id=ta required></textarea>\
        <input id=off required disabled><input id=ro required readonly>\
        <input id=hid type=hidden required>\
        </form>",
    )
    .unwrap();
    assert_eq!(flags(&mut d, "req"), ["valueMissing"]);
    d.set_value("req", "x").unwrap();
    assert!(flags(&mut d, "req").is_empty());
    assert_eq!(d.get_validity("req").unwrap()["valid"], true);

    d.set_value("mail", "nope").unwrap();
    assert_eq!(flags(&mut d, "mail"), ["typeMismatch"]);
    d.set_value("mail", "a@b.example").unwrap();
    assert!(flags(&mut d, "mail").is_empty());
    d.set_value("url", "example.com").unwrap();
    assert_eq!(flags(&mut d, "url"), ["typeMismatch"]);
    d.set_value("url", "https://example.com/x").unwrap();
    assert!(flags(&mut d, "url").is_empty());

    d.set_value("pat", "ABC").unwrap();
    assert_eq!(flags(&mut d, "pat"), ["patternMismatch"]);
    assert_eq!(
        d.get_validity("pat").unwrap()["message"],
        "Please match the requested format: lowercase"
    );
    d.set_value("pat", "abc").unwrap();
    assert!(flags(&mut d, "pat").is_empty());

    d.set_value("len", "ab").unwrap();
    assert_eq!(flags(&mut d, "len"), ["tooShort"]);
    d.set_value("len", "abcdef").unwrap();
    assert_eq!(flags(&mut d, "len"), ["tooLong"]);
    d.set_value("len", "abcd").unwrap();
    assert!(flags(&mut d, "len").is_empty());

    d.set_value("num", "1").unwrap();
    let f = flags(&mut d, "num");
    assert!(f.contains(&"rangeUnderflow".to_owned()) && f.contains(&"stepMismatch".to_owned()));
    d.set_value("num", "10").unwrap();
    assert_eq!(flags(&mut d, "num"), ["rangeOverflow"]);
    d.set_value("num", "5").unwrap();
    assert_eq!(flags(&mut d, "num"), ["stepMismatch"]);
    d.set_value("num", "4").unwrap();
    assert!(flags(&mut d, "num").is_empty());

    assert_eq!(flags(&mut d, "chk"), ["valueMissing"]);
    d.set_checked("chk", true).unwrap();
    assert!(flags(&mut d, "chk").is_empty());
    // A required radio is judged on its group.
    assert_eq!(flags(&mut d, "ra"), ["valueMissing"]);
    assert_eq!(flags(&mut d, "rb"), ["valueMissing"]);
    d.set_checked("rb", true).unwrap();
    assert!(flags(&mut d, "ra").is_empty());

    assert_eq!(flags(&mut d, "sel"), ["valueMissing"]);
    d.set_value("sel", "x").unwrap();
    assert!(flags(&mut d, "sel").is_empty());
    assert_eq!(flags(&mut d, "ta"), ["valueMissing"]);

    for barred in ["off", "ro", "hid"] {
        let v = d.get_validity(barred).unwrap();
        assert_eq!(v["will_validate"], false, "{barred}");
        assert_eq!(v["valid"], true, "{barred}");
    }
    assert!(d.get_validity("nope").is_err());
}

const FORM: &str = "<!doctype html><form id=f>\
    <input id=a required><input id=b required><input id=c>\
    <button id=go>Go</button><button id=skip formnovalidate>Skip</button></form>";

#[test]
fn clicking_submit_validates_first_then_submits_with_the_submitter() {
    let mut d = Document::parse(FORM).unwrap();
    let events = click(&mut d, "go");
    let invalid = of_type(&events, "invalid");
    assert_eq!(
        invalid
            .iter()
            .map(|e| e["element"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(invalid[0]["validity"]["flags"]["valueMissing"], true);
    assert_eq!(
        invalid[0]["path"].as_array().unwrap().len(),
        1,
        "invalid does not bubble"
    );
    assert!(
        of_type(&events, "submit").is_empty(),
        "invalid forms do not submit"
    );
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "focus" && e["element"] == "a"),
        "the first invalid control is focused: {events:?}"
    );
    d.set_value("a", "1").unwrap();
    d.set_value("b", "2").unwrap();
    let events = click(&mut d, "go");
    assert!(of_type(&events, "invalid").is_empty());
    let submit = of_type(&events, "submit");
    assert_eq!(submit.len(), 1);
    assert_eq!(submit[0]["submitter"], "go");
    assert_eq!(submit[0]["element"], "f");
}

#[test]
fn formnovalidate_and_novalidate_skip_validation() {
    let mut d = Document::parse(FORM).unwrap();
    let events = click(&mut d, "skip");
    assert!(of_type(&events, "invalid").is_empty());
    assert_eq!(of_type(&events, "submit")[0]["submitter"], "skip");

    let mut d = Document::parse(&FORM.replace("<form id=f>", "<form id=f novalidate>")).unwrap();
    let events = click(&mut d, "go");
    assert!(of_type(&events, "invalid").is_empty());
    assert_eq!(of_type(&events, "submit").len(), 1);
}

#[test]
fn enter_in_a_field_is_implicit_submission_with_validation() {
    let mut d = Document::parse(FORM).unwrap();
    d.focus("c").unwrap();
    d.drain_events();
    let events = press(&mut d, "Enter");
    assert_eq!(of_type(&events, "invalid").len(), 2);
    assert!(of_type(&events, "submit").is_empty());
    d.set_value("a", "1").unwrap();
    d.set_value("b", "2").unwrap();
    d.focus("c").unwrap();
    let events = press(&mut d, "Enter");
    let submit = of_type(&events, "submit");
    assert_eq!(submit.len(), 1);
    assert_eq!(
        submit[0]["submitter"], "go",
        "implicit submission clicks the default button"
    );

    // No button, one text field: the form itself is submitted, without a submitter.
    let mut d =
        Document::parse("<!doctype html><form id=f><input id=only required></form>").unwrap();
    d.focus("only").unwrap();
    d.drain_events();
    let events = press(&mut d, "Enter");
    assert_eq!(of_type(&events, "invalid").len(), 1);
    assert!(of_type(&events, "submit").is_empty());
    d.set_value("only", "x").unwrap();
    let events = press(&mut d, "Enter");
    let submit = of_type(&events, "submit");
    assert_eq!(submit.len(), 1);
    assert!(submit[0].get("submitter").is_none());
}

#[test]
fn form_attribute_associates_controls_outside_the_form() {
    let mut d = Document::parse(
        "<!doctype html><form id=f></form><input id=out required form=f>\
        <form id=g><input id=in required></form><button id=go form=f>Go</button>",
    )
    .unwrap();
    let events = click(&mut d, "go");
    let invalid = of_type(&events, "invalid");
    assert_eq!(invalid.len(), 1);
    assert_eq!(invalid[0]["element"], "out", "g's control is not f's");
    let r = d.check_validity("f").unwrap();
    assert_eq!(r["valid"], false);
    assert_eq!(r["invalid"], json!(["out"]));
    let r = d.check_validity("g").unwrap();
    assert_eq!(r["invalid"], json!(["in"]));
}

#[test]
fn check_validity_fires_invalid_without_submitting_or_focusing() {
    let mut d = Document::parse(FORM).unwrap();
    d.drain_events();
    let r = d.check_validity("f").unwrap();
    assert_eq!(
        r,
        json!({"valid": false, "invalid": ["a", "b"], "count": 2})
    );
    let events = d.drain_events();
    assert_eq!(of_type(&events, "invalid").len(), 2);
    assert!(of_type(&events, "submit").is_empty());
    assert!(of_type(&events, "focus").is_empty());
    let r = d.check_validity("c").unwrap();
    assert_eq!(r, json!({"valid": true, "invalid": [], "count": 0}));
    assert_eq!(d.check_validity("go").unwrap()["valid"], true);
    assert!(d.drain_events().is_empty());
}

#[test]
fn reset_restores_defaults_unless_the_host_cancels_it() {
    let mut d = Document::parse(
        "<!doctype html><form id=f><input id=a value=d required><input id=k type=checkbox checked>\
        <button id=go>Go</button><button id=rs type=reset>Reset</button></form>",
    )
    .unwrap();
    d.set_default_action(rule("f", "reset"), true).unwrap();
    d.set_value("a", "changed").unwrap();
    d.set_checked("k", false).unwrap();
    click(&mut d, "rs");
    assert_eq!(
        d.get_value("a").unwrap(),
        "changed",
        "canceled reset keeps values"
    );
    d.set_default_action(rule("f", "reset"), false).unwrap();
    click(&mut d, "rs");
    assert_eq!(d.get_value("a").unwrap(), "d");
    assert!(d.get_checked("k").unwrap());
    // Emptied then reset: the default value comes back and validates.
    d.set_value("a", "").unwrap();
    assert_eq!(flags(&mut d, "a"), ["valueMissing"]);
    click(&mut d, "rs");
    assert!(flags(&mut d, "a").is_empty());
}

#[test]
fn a_host_rule_can_cancel_the_submit_default_after_validation() {
    let mut d = Document::parse(
        "<!doctype html><form id=f><input id=a value=ok required><button id=go>Go</button></form>",
    )
    .unwrap();
    d.set_default_action(rule("f", "submit"), true).unwrap();
    let events = click(&mut d, "go");
    let submit = of_type(&events, "submit");
    assert_eq!(submit.len(), 1);
    assert_eq!(submit[0]["default_prevented"], true);
}

#[test]
fn controls_removed_after_validation_leave_no_stale_state() {
    let mut d = Document::parse(FORM).unwrap();
    click(&mut d, "go");
    d.remove_element("a").unwrap();
    d.drain_events();
    let events = click(&mut d, "go");
    let invalid = of_type(&events, "invalid");
    assert_eq!(invalid.len(), 1);
    assert_eq!(invalid[0]["element"], "b");
    d.remove_element("f").unwrap();
    assert!(d.check_validity("f").is_err());
    assert!(d.get_validity("b").is_err());
}

#[test]
fn file_input_choice_sets_files_fires_input_and_change_and_reset_clears() {
    // 0.6 F0606: what the host does when the native picker returns.
    let mut d = Document::parse("<!doctype html><form id=f><input id=pick type=file multiple><button type=reset id=r>R</button></form>").unwrap();
    d.render(300, 200, 1.0).unwrap();
    click(&mut d, "pick");
    let requests = d.take_file_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].multiple);
    assert_eq!(requests[0].element.as_deref(), Some("pick"));
    assert!(d.take_file_requests().is_empty(), "taken once");
    let input = requests[0].input;
    d.complete_file_input(
        input,
        Some(vec![
            "/tmp/ünïcødé složka/a.txt".into(),
            "/tmp/b.png".into(),
        ]),
    )
    .unwrap();
    let events = d.drain_events();
    let kinds: Vec<_> = events
        .iter()
        .map(|e| e["type"].as_str().unwrap().to_owned())
        .collect();
    // `input` (with the legacy `change` alias), then HTML `change`, delivered as `commit`.
    assert_eq!(kinds, ["input", "change", "commit"]);
    assert_eq!(events[2]["files"][0], "/tmp/ünïcødé složka/a.txt");
    assert_eq!(d.get_files("pick").unwrap().len(), 2);
    // Canceling later keeps the earlier choice (browsers do) and fires `cancel`.
    d.complete_file_input(input, None).unwrap();
    assert_eq!(d.drain_events()[0]["type"], "cancel");
    assert_eq!(d.get_files("pick").unwrap().len(), 2);
    click(&mut d, "r");
    assert!(
        d.get_files("pick").unwrap().is_empty(),
        "a form reset clears the files"
    );
    // A removed input completes silently.
    d.remove_element("pick").unwrap();
    d.complete_file_input(input, Some(vec!["/x".into()]))
        .unwrap();
    assert!(d.drain_events().iter().all(|e| e["type"] != "change"));
}

#[test]
fn draggable_elements_drag_between_targets_and_escape_cancels() {
    fn send(d: &mut Document, all: &mut Vec<Value>, i: Input) {
        all.extend(d.handle_input(i).unwrap());
    }
    // 0.7 F0707.
    let mut d = Document::parse("<!doctype html><style>body{margin:0}div{width:100px;height:40px;margin:10px}</style><div id=card draggable=true><span id=inner>Card text</span></div><div id=lane1></div><div id=lane2></div>").unwrap();
    d.render(300, 300, 1.0).unwrap();
    let center = |d: &mut Document, e: &str| {
        let (x, y, w, h) = rect(d, e);
        (x + w / 2.0, y + h / 2.0)
    };
    let (cx, cy) = center(&mut d, "card");
    let (l1x, l1y) = center(&mut d, "lane1");
    let (l2x, l2y) = center(&mut d, "lane2");
    let mut all: Vec<Value> = Vec::new();
    send(&mut d, &mut all, Input::PointerMoved { x: cx, y: cy });
    send(
        &mut d,
        &mut all,
        Input::PointerButton {
            pressed: true,
            button: 0,
            x: cx,
            y: cy,
        },
    );
    send(&mut d, &mut all, Input::PointerMoved { x: cx + 2.0, y: cy });
    assert!(
        all.iter().all(|e| e["type"] != "dragstart"),
        "below the threshold"
    );
    send(&mut d, &mut all, Input::PointerMoved { x: l1x, y: l1y });
    send(&mut d, &mut all, Input::PointerMoved { x: l2x, y: l2y });
    send(
        &mut d,
        &mut all,
        Input::PointerButton {
            pressed: false,
            button: 0,
            x: l2x,
            y: l2y,
        },
    );
    let drag: Vec<(String, Value)> = all
        .iter()
        .filter(|e| {
            matches!(
                e["type"].as_str(),
                Some("dragstart" | "dragover" | "drop" | "dragend")
            )
        })
        .map(|e| (e["type"].as_str().unwrap().to_owned(), e["element"].clone()))
        .collect();
    assert_eq!(drag.first().map(|x| x.0.as_str()), Some("dragstart"));
    assert_eq!(drag[0].1, "card", "the draggable ancestor is the source");
    assert!(drag.iter().any(|(t, e)| t == "dragover" && e == "lane1"));
    assert!(drag.iter().any(|(t, e)| t == "dragover" && e == "lane2"));
    let drop = all.iter().find(|e| e["type"] == "drop").unwrap();
    assert_eq!(
        (drop["element"].as_str(), drop["source"].as_str()),
        (Some("lane2"), Some("card"))
    );
    let end = all.iter().find(|e| e["type"] == "dragend").unwrap();
    assert_eq!(
        (end["dropped"].as_bool(), end["target"].as_str()),
        (Some(true), Some("lane2"))
    );
    assert!(
        all.iter().all(|e| e["type"] != "click"),
        "a drag is not a click"
    );
    assert_eq!(
        d.selection_text().unwrap(),
        "",
        "no text selection during a drag"
    );
    // Escape cancels a running drag.
    let mut all: Vec<Value> = Vec::new();
    send(
        &mut d,
        &mut all,
        Input::PointerButton {
            pressed: true,
            button: 0,
            x: cx,
            y: cy,
        },
    );
    send(&mut d, &mut all, Input::PointerMoved { x: l1x, y: l1y });
    send(
        &mut d,
        &mut all,
        Input::Key {
            key: "Escape".into(),
            code: "Escape".into(),
            pressed: true,
            repeat: false,
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        },
    );
    send(
        &mut d,
        &mut all,
        Input::PointerButton {
            pressed: false,
            button: 0,
            x: l1x,
            y: l1y,
        },
    );
    assert!(
        all.iter()
            .any(|e| e["type"] == "dragend" && e["dropped"] == false)
    );
    assert!(all.iter().all(|e| e["type"] != "drop"));
    // A press and release without movement is still a click.
    let mut all: Vec<Value> = Vec::new();
    all.extend(
        d.handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x: cx,
            y: cy,
        })
        .unwrap(),
    );
    all.extend(
        d.handle_input(Input::PointerButton {
            pressed: false,
            button: 0,
            x: cx,
            y: cy,
        })
        .unwrap(),
    );
    assert!(all.iter().any(|e| e["type"] == "click"));
    assert!(all.iter().all(|e| e["type"] != "dragstart"));
}

#[test]
fn plain_text_contenteditable_edits_with_keys_ime_clipboard_and_undo() {
    // 0.8 F0801.
    let shared = Rc::new(RefCell::new(String::new()));
    let mut d = Document::parse("<!doctype html><style>body{margin:0;font:16px sans-serif}#ed{width:200px;min-height:40px}</style><div id=ed contenteditable=plaintext-only>Ahoj <b>světe</b></div><p id=other>other</p>").unwrap();
    d.set_clipboard(Box::new(TestClipboard(shared.clone())));
    d.render(300, 200, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    let _ = click(&mut d, "ed");
    assert_eq!(
        d.interaction.focused_element(d.id()),
        d.node("ed").ok(),
        "a host is focusable"
    );
    let text = |d: &Document| d.get_text("ed").unwrap();
    let mut events = Vec::new();
    events.extend(d.handle_input(Input::Text("!".into())).unwrap());
    assert_eq!(
        text(&d),
        "Ahoj světe!",
        "markup flattened to text, caret at the end"
    );
    assert!(
        d.get_html(Some("ed"), None, false).unwrap() == "Ahoj světe!",
        "content is one text node"
    );
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "input" && e["input_type"] == "insertText" && e["data"] == "!")
    );
    press(&mut d, "Enter");
    d.handle_input(Input::Text("😀👍🏽".into())).unwrap();
    assert_eq!(text(&d), "Ahoj světe!\n😀👍🏽");
    press(&mut d, "Backspace");
    assert_eq!(
        text(&d),
        "Ahoj světe!\n😀",
        "a whole grapheme (emoji + skin tone) is deleted"
    );
    press(&mut d, "Backspace");
    press(&mut d, "Backspace");
    assert_eq!(text(&d), "Ahoj světe!");
    // Ctrl+Z restores, Ctrl+Y redoes.
    d.handle_input(key("z", true, false)).unwrap();
    assert_eq!(text(&d), "Ahoj světe!\n");
    d.handle_input(key("y", true, false)).unwrap();
    assert_eq!(text(&d), "Ahoj světe!");
    // Ctrl+A then typing replaces; Ctrl+C/V moves text; pasted markup stays text.
    d.handle_input(key("a", true, false)).unwrap();
    d.handle_input(key("c", true, false)).unwrap();
    assert_eq!(&*shared.borrow(), "Ahoj světe!");
    d.handle_input(Input::Text("x".into())).unwrap();
    assert_eq!(text(&d), "x");
    *shared.borrow_mut() = "<img src=x onerror=alert(1)>".into();
    d.handle_input(key("v", true, false)).unwrap();
    assert_eq!(text(&d), "x<img src=x onerror=alert(1)>");
    assert!(
        d.query_ids("#ed img", 10).unwrap().1 == 0,
        "no element was created"
    );
    // Arrows and Home/End move the caret; Shift extends; Delete removes forward.
    d.handle_input(key("Home", true, false)).unwrap();
    d.handle_input(Input::Text(">".into())).unwrap();
    assert!(text(&d).starts_with(">x"), "{}", text(&d));
    d.handle_input(key("End", true, false)).unwrap();
    press(&mut d, "ArrowLeft");
    d.handle_input(key("Delete", false, false)).unwrap();
    assert!(text(&d).ends_with("alert(1)"), "{}", text(&d));
    // IME: the preedit is shown in the text and committed once.
    d.handle_input(Input::ImePreedit {
        text: "に".into(),
        cursor: Some((3, 3)),
    })
    .unwrap();
    assert!(text(&d).ends_with("alert(1)に"));
    d.handle_input(Input::ImePreedit {
        text: "日本".into(),
        cursor: Some((6, 6)),
    })
    .unwrap();
    d.handle_input(Input::ImeCommit("日本".into())).unwrap();
    assert!(text(&d).ends_with("alert(1)日本"), "{}", text(&d));
    assert_eq!(text(&d).matches("日本").count(), 1);
    // A non-editable element is untouched by typing after focus leaves the host.
    let _ = click(&mut d, "other");
    d.handle_input(Input::Text("Z".into())).unwrap();
    assert_eq!(d.get_text("other").unwrap(), "other");
}

#[test]
fn editing_hosts_are_multiline_text_fields_for_assistive_technology() {
    let mut d =
        Document::parse("<!doctype html><div id=ed contenteditable aria-label=Notes>Hello</div>")
            .unwrap();
    d.render(300, 200, 1.0).unwrap();
    let t = d.accessibility_tree().unwrap();
    let n = t
        .nodes
        .values()
        .find(|n| n.html_id.as_deref() == Some("ed"))
        .unwrap();
    assert_eq!(n.role, accesskit::Role::MultilineTextInput);
    assert_eq!(n.value.as_deref(), Some("Hello"));
    assert!(n.states.focusable);
}

#[test]
fn element_rect_of_a_scrolled_textarea_is_its_border_box() {
    let mut d = Document::parse("<!doctype html><style>textarea{display:block;width:520px;height:120px;font-size:20px}</style><p>x</p><textarea id=t></textarea>").unwrap();
    d.render(600, 400, 1.0).unwrap();
    let before = d.viewport_rect(d.node("t").unwrap());
    d.set_value(
        "t",
        &(0..60)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    d.render(600, 400, 1.0).unwrap();
    {
        let c = d.pipeline.committed.as_deref().unwrap();
        let tree = &c.layout.fragments;
        for &f in tree.fragments_of(d.node("t").unwrap()) {
            let fr = tree.get(f);
            eprintln!(
                "DBG {f:?} parent {:?} kind {} rect {:?}",
                fr.parent,
                match &fr.kind {
                    crate::layout::fragment::FragmentKind::Text(_) => "text",
                    _ => "other",
                },
                tree.document_rect(f).rect
            );
        }
    }
    assert_eq!(d.viewport_rect(d.node("t").unwrap()), before);
}

#[test]
fn pointer_events_fields_cancel_and_capture() {
    // 0.8 F0807.
    let html = "<!doctype html><style>body{margin:0}div{position:absolute;width:100px;height:50px}</style><div id=a style='left:0;top:0'></div><div id=b style='left:200px;top:0'></div>";
    let mut d = Document::parse(html).unwrap();
    d.render(400, 200, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    let types = |ev: &[Value]| {
        ev.iter()
            .map(|e| {
                format!(
                    "{}@{}",
                    e["type"].as_str().unwrap(),
                    e["element"].as_str().unwrap()
                )
            })
            .collect::<Vec<_>>()
    };
    let down = d
        .handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x: 10.0,
            y: 10.0,
        })
        .unwrap();
    let pd = down.iter().find(|e| e["type"] == "pointerdown").unwrap();
    assert_eq!(
        (
            pd["pointer_id"].as_u64(),
            pd["pointer_type"].as_str(),
            pd["is_primary"].as_bool()
        ),
        (Some(1), Some("mouse"), Some(true))
    );
    assert_eq!(
        (
            pd["x"].as_f64(),
            pd["buttons"].as_u64(),
            pd["cancelable"].as_bool()
        ),
        (Some(10.0), Some(1), Some(true))
    );
    assert!(pd["pressure"].as_f64().unwrap() > 0.0);
    assert_eq!(
        types(&down)[..2],
        ["pointerdown@a".to_string(), "mousedown@a".to_string()]
    );
    // Capture: the moves and the release go to the capture target, even over b.
    assert!(d.set_pointer_capture("a", 1, true).unwrap());
    assert!(
        d.set_pointer_capture("a", 7, true).is_err(),
        "unknown pointer"
    );
    let mv = d
        .handle_input(Input::PointerMoved { x: 250.0, y: 10.0 })
        .unwrap();
    let t = types(&mv);
    assert_eq!(t[0], "gotpointercapture@a", "{t:?}");
    assert!(
        t.contains(&"pointermove@a".to_string()) && t.contains(&"mousemove@a".to_string()),
        "{t:?}"
    );
    assert!(
        !t.iter()
            .any(|x| x.starts_with("pointermove@b") || x.starts_with("mousemove@b")),
        "{t:?}"
    );
    assert_eq!(d.pointer_capture().as_deref(), Some("a"));
    let up = d
        .handle_input(Input::PointerButton {
            pressed: false,
            button: 0,
            x: 250.0,
            y: 10.0,
        })
        .unwrap();
    let t = types(&up);
    assert!(
        t.contains(&"pointerup@a".to_string())
            && t.contains(&"mouseup@a".to_string())
            && t.contains(&"lostpointercapture@a".to_string()),
        "{t:?}"
    );
    assert!(
        t.contains(&"click@a".to_string()),
        "the press and the captured release match: {t:?}"
    );
    assert_eq!(d.pointer_capture(), None, "implicit release");
    // Without a pressed button capture is refused.
    assert!(!d.set_pointer_capture("a", 1, true).unwrap());
    // A host rule canceling pointerdown suppresses mousedown (not the pointer events).
    d.set_default_action(
        DefaultActionRule {
            element: "b".into(),
            event: "pointerdown".into(),
            key: None,
            shift: None,
            ctrl: None,
            alt: None,
            meta: None,
        },
        true,
    )
    .unwrap();
    let down = d
        .handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x: 210.0,
            y: 10.0,
        })
        .unwrap();
    let t = types(&down);
    assert!(
        t.contains(&"pointerdown@b".to_string()) && !t.iter().any(|x| x.starts_with("mousedown")),
        "{t:?}"
    );
    assert!(
        down.iter()
            .any(|e| e["type"] == "pointerdown" && e["default_prevented"] == true)
    );
    // Stale target: the capture element is removed during the press.
    assert!(d.set_pointer_capture("b", 1, true).unwrap());
    d.handle_input(Input::PointerMoved { x: 220.0, y: 10.0 })
        .unwrap();
    d.remove_element("b").unwrap();
    d.render(400, 200, 1.0).unwrap();
    let mv = d
        .handle_input(Input::PointerMoved { x: 20.0, y: 10.0 })
        .unwrap();
    let t = types(&mv);
    assert_eq!(t[0], "lostpointercapture@", "lost at the Document: {t:?}");
    assert!(t.contains(&"pointermove@a".to_string()), "{t:?}");
    assert_eq!(d.pointer_capture(), None);
    d.handle_input(Input::PointerButton {
        pressed: false,
        button: 0,
        x: 20.0,
        y: 10.0,
    })
    .unwrap();
}

#[test]
fn editing_host_interop_scroll_a11y_mutation_bidi_and_removed_selection() {
    // 0.8 F0806.
    let mut d = Document::parse("<!doctype html><style>body{margin:0;font:16px sans-serif}#sc{height:60px;width:200px;overflow:auto}</style><div id=sc><div id=ed contenteditable=plaintext-only>x</div></div><p id=p>Select me please</p><div id=bi contenteditable=plaintext-only dir=rtl>abc אבג</div>").unwrap();
    d.render(300, 300, 1.0).unwrap();
    d.handle_input(Input::Focus(true)).unwrap();
    let _ = click(&mut d, "ed");
    d.handle_input(key("End", true, false)).unwrap();
    for i in 0..12 {
        press(&mut d, "Enter");
        d.handle_input(Input::Text(format!("line {i}"))).unwrap();
        d.render(300, 300, 1.0).unwrap();
    }
    assert_eq!(d.get_text("ed").unwrap().lines().count(), 13, "multi-line");
    assert!(
        d.scroll_state(Some("sc")).unwrap().y > 0.0,
        "the caret is kept visible in the scroller"
    );
    // Accessibility follows the edit.
    let t = d.accessibility_tree().unwrap();
    let n = t
        .nodes
        .values()
        .find(|n| n.html_id.as_deref() == Some("ed"))
        .unwrap();
    assert!(n.value.as_deref().unwrap().ends_with("line 11"));
    // The app rewrites the host while the caret is inside: the caret is clamped, typing continues.
    d.set_text("ed", "ab").unwrap();
    d.render(300, 300, 1.0).unwrap();
    d.handle_input(Input::Text("c".into())).unwrap();
    assert_eq!(d.get_text("ed").unwrap(), "abc");
    // The app write is not an undo step: undo reverts the typing after it.
    d.handle_input(key("z", true, false)).unwrap();
    assert_eq!(d.get_text("ed").unwrap(), "ab");
    // Bidi: a right-to-left host edits in logical order.
    let _ = click(&mut d, "bi");
    d.handle_input(key("End", true, false)).unwrap();
    press(&mut d, "Backspace");
    assert_eq!(d.get_text("bi").unwrap(), "abc אב");
    // A selection inside a removed subtree is dropped.
    d.select_text(Some("p")).unwrap();
    assert_eq!(d.selection_text().unwrap(), "Select me please");
    d.remove_element("p").unwrap();
    d.render(300, 300, 1.0).unwrap();
    assert_eq!(d.selection_text().unwrap(), "");
    d.render(300, 300, 1.0).unwrap();
}

#[test]
fn content_visibility_hidden_skips_contents_and_contain_intrinsic_size_sizes_it() {
    // 0.9 F0903.
    let html = "<!doctype html><style>body{margin:0}#h{content-visibility:hidden;width:100px}#s{contain:size;contain-intrinsic-size:80px 30px;display:inline-block}#a{content-visibility:auto}</style>\
        <div id=h><p id=inner style='height:50px;background:red'>x</p><button id=b>b</button></div>\
        <div id=s><p style='height:200px;width:300px'>big</p></div><div id=a><p id=ap>auto</p></div>";
    let mut d = Document::parse(html).unwrap();
    d.render(400, 400, 1.0).unwrap();
    let h = d.viewport_rect(d.node("h").unwrap()).unwrap();
    assert_eq!(h.3, 0.0, "size containment: no content height");
    assert!(
        d.viewport_rect(d.node("inner").unwrap()).is_none(),
        "contents are not boxed"
    );
    let s = d.viewport_rect(d.node("s").unwrap()).unwrap();
    assert_eq!((s.2, s.3), (80.0, 30.0), "contain-intrinsic-size");
    assert!(
        d.viewport_rect(d.node("ap").unwrap()).is_some(),
        "auto renders its contents"
    );
    let t = d.accessibility_tree().unwrap();
    assert!(
        !t.nodes.values().any(|n| n.html_id.as_deref() == Some("b")),
        "skipped contents are not exposed"
    );
    d.handle_input(Input::Focus(true)).unwrap();
    press(&mut d, "Tab");
    assert_ne!(
        d.interaction.focused_element(d.id()),
        d.node("b").ok(),
        "skipped contents are not focusable"
    );
    // Toggling it off renders the contents.
    d.set_attribute("h", "style", Some("content-visibility:visible"))
        .unwrap();
    d.render(400, 400, 1.0).unwrap();
    assert!(d.viewport_rect(d.node("inner").unwrap()).is_some());
}

#[test]
fn field_sizing_content_fits_text_controls_to_their_value() {
    // 0.9 F0903.
    let html = "<!doctype html><style>body{margin:0;font:16px sans-serif}input,textarea{field-sizing:content;padding:0;border:0}</style>\
        <input id=i value=ab><input id=f style='field-sizing:fixed'><input id=p placeholder='a longer placeholder'><textarea id=t>one\ntwo\nthree</textarea>";
    let mut d = Document::parse(html).unwrap();
    d.render(800, 400, 1.0).unwrap();
    let w = |d: &mut Document, id: &str| d.viewport_rect(d.node(id).unwrap()).unwrap();
    let small = w(&mut d, "i");
    let fixed = w(&mut d, "f");
    assert!(small.2 < 30.0 && small.2 > 10.0, "fits 'ab': {small:?}");
    assert!(
        fixed.2 > 100.0,
        "field-sizing: fixed keeps the size attribute default: {fixed:?}"
    );
    assert!(
        w(&mut d, "p").2 > small.2 * 3.0,
        "the placeholder sizes an empty control"
    );
    d.set_value("i", "a much longer value here").unwrap();
    d.render(800, 400, 1.0).unwrap();
    assert!(w(&mut d, "i").2 > small.2 * 4.0, "grows with the value");
    let t = w(&mut d, "t");
    d.set_value("t", "one").unwrap();
    d.render(800, 400, 1.0).unwrap();
    let t1 = w(&mut d, "t");
    assert!(
        (t.3 - 3.0 * t1.3).abs() < 2.0,
        "a textarea fits its lines: {t:?} vs {t1:?}"
    );
}

#[test]
fn css_resize_corner_drag_sets_the_inline_size() {
    // 0.9 F0903.
    let html = "<!doctype html><style>body{margin:0}#r{resize:both;overflow:auto;width:100px;height:60px;padding:5px;border:2px solid}#n{resize:none;overflow:auto;width:100px;height:60px}</style><div id=r>x</div><div id=n>y</div>";
    let mut d = Document::parse(html).unwrap();
    d.render(400, 400, 1.0).unwrap();
    let (x, y, w, h) = d.viewport_rect(d.node("r").unwrap()).unwrap();
    assert_eq!((w, h), (114.0, 74.0));
    let ev = d
        .handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x: x + w - 3.0,
            y: y + h - 3.0,
        })
        .unwrap();
    assert!(
        !ev.iter().any(|e| e["type"] == "mousedown"),
        "the resize press stays out of the page"
    );
    d.handle_input(Input::PointerMoved {
        x: x + w + 37.0,
        y: y + h + 17.0,
    })
    .unwrap();
    d.handle_input(Input::PointerButton {
        pressed: false,
        button: 0,
        x: x + w + 37.0,
        y: y + h + 17.0,
    })
    .unwrap();
    d.render(400, 400, 1.0).unwrap();
    assert_eq!(
        d.get_attribute("r", "style").unwrap().as_deref(),
        Some("width: 140px; height: 80px")
    );
    assert_eq!(d.viewport_rect(d.node("r").unwrap()).unwrap().2, 154.0);
    // resize: none: a press in the corner is an ordinary press.
    let (x, y, w, h) = d.viewport_rect(d.node("n").unwrap()).unwrap();
    let ev = d
        .handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x: x + w - 3.0,
            y: y + h - 3.0,
        })
        .unwrap();
    assert!(ev.iter().any(|e| e["type"] == "mousedown"));
}
