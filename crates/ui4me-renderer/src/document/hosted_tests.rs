use super::*;

fn document(html: &str) -> Document {
    Document::parse_hosted(html, None, 240, 160).unwrap()
}
fn key(name: &str, ctrl: bool, shift: bool) -> Input {
    Input::Key {
        key: name.into(),
        code: name.into(),
        pressed: true,
        repeat: false,
        shift,
        ctrl,
        alt: false,
        meta: false,
    }
}
fn press(d: &mut Document, name: &str, ctrl: bool, shift: bool) -> Vec<Value> {
    d.handle_input(key(name, ctrl, shift)).unwrap()
}
fn focused() -> Document {
    let mut d = document(
        "<!doctype html><section id=container><input id=edit value=old></section><input id=other>",
    );
    d.focus("edit").unwrap();
    d.drain_events();
    d.enable_deferred_clipboard();
    d.set_clipboard(Box::new(NoSynchronousClipboard));
    d
}

struct NoSynchronousClipboard;
impl Clipboard for NoSynchronousClipboard {
    fn read_text(&mut self) -> std::result::Result<Option<String>, String> {
        panic!("native editing must not perform a synchronous clipboard read")
    }
    fn write_text(&mut self, _: &str) -> std::result::Result<(), String> {
        panic!("native editing must not perform a synchronous clipboard write")
    }
}
fn read_id(d: &mut Document) -> u64 {
    let requests = d.take_clipboard_requests();
    assert_eq!(requests.len(), 1);
    let ClipboardRequest::Read { id } = requests[0] else {
        panic!("expected read")
    };
    id
}
fn cut_id(d: &mut Document) -> u64 {
    press(d, "a", true, false);
    press(d, "x", true, false);
    let requests = d.take_clipboard_requests();
    assert_eq!(requests.len(), 1);
    let ClipboardRequest::Write { id, ref text } = requests[0] else {
        panic!("expected write")
    };
    assert_eq!(text, "old");
    id
}

#[test]
fn native_paste_waits_for_completion_and_emits_input_once() {
    let mut d = focused();
    assert!(
        !press(&mut d, "v", true, false)
            .iter()
            .any(|e| e["type"] == "input")
    );
    assert_eq!(d.get_value("edit").unwrap(), "old");
    let id = read_id(&mut d);
    let mut up = key("v", true, false);
    if let Input::Key { pressed, .. } = &mut up {
        *pressed = false;
    }
    d.handle_input(up).unwrap();
    let events = d.complete_clipboard(id, Ok(Some("é😀".into()))).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "oldé😀");
    assert_eq!(events.iter().filter(|e| e["type"] == "input").count(), 1);
    assert!(d.drain_events().is_empty());
    assert!(
        d.complete_clipboard(id, Ok(Some("duplicate".into())))
            .unwrap()
            .is_empty()
    );
    assert_eq!(d.get_value("edit").unwrap(), "oldé😀");
}

#[test]
fn paste_results_cannot_follow_changed_focus_or_generation_reuse() {
    let mut d = focused();
    press(&mut d, "v", true, false);
    let id = read_id(&mut d);
    d.focus("other").unwrap();
    d.focus("edit").unwrap();
    d.complete_clipboard(id, Ok(Some("wrong".into()))).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "old");
    press(&mut d, "v", true, false);
    let id = read_id(&mut d);
    press(&mut d, "ArrowLeft", false, false);
    press(&mut d, "ArrowRight", false, false);
    d.complete_clipboard(id, Ok(Some("wrong".into()))).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "old");
    press(&mut d, "v", true, false);
    let id = read_id(&mut d);
    let old = d.node("edit").unwrap();
    d.replace_children_html("container", "<input id=edit value=new>")
        .unwrap();
    assert_ne!(d.node("edit").unwrap(), old);
    d.focus("edit").unwrap();
    d.complete_clipboard(id, Ok(Some("wrong".into()))).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "new");
}

#[test]
fn hosted_text_input_and_committed_change_have_canonical_names() {
    let mut d = focused();
    let events = d.handle_input(Input::Text("!".into())).unwrap();
    assert_eq!(events.iter().filter(|e| e["type"] == "input").count(), 1);
    assert!(
        !events
            .iter()
            .any(|e| e["type"] == "change" || e["type"] == "commit")
    );
    d.focus("other").unwrap();
    let events = d.drain_events();
    assert_eq!(events.iter().filter(|e| e["type"] == "change").count(), 1);
    assert!(!events.iter().any(|e| e["type"] == "commit"));
}

#[test]
fn streaming_updates_with_a_focused_input_defer_layout_to_the_frame() {
    let mut d = document("<!doctype html><div id=stream></div><input id=edit value=keep>");
    d.focus("edit").unwrap();
    d.render(240, 160, 1.0).unwrap();
    for _ in 0..20 {
        d.append_text("stream", "token ").unwrap();
        assert_eq!(d.pipeline.state(), PipelineState::Dirty);
        assert_eq!(d.ime_surrounding().unwrap().text, "keep");
    }
    d.render(240, 160, 1.0).unwrap();
    assert!(d.ime_request().is_some());
}

#[test]
fn paste_does_not_overwrite_later_typing_or_application_writes() {
    let mut d = focused();
    press(&mut d, "v", true, false);
    let id = read_id(&mut d);
    d.handle_input(Input::Text("!".into())).unwrap();
    d.complete_clipboard(id, Ok(Some("wrong".into()))).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "old!");
    press(&mut d, "v", true, false);
    let id = read_id(&mut d);
    d.set_value("edit", "replacement").unwrap();
    d.complete_clipboard(id, Ok(Some("wrong".into()))).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "replacement");
}

#[test]
fn cut_requires_successful_write_and_the_same_selection() {
    let mut d = focused();
    let failed = cut_id(&mut d);
    assert_eq!(d.get_value("edit").unwrap(), "old");
    d.complete_clipboard(failed, Err("OS denied transfer".into()))
        .unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "old");
    assert!(d.diagnostics().iter().any(|e| {
        e["message"]
            .as_str()
            .is_some_and(|m| m.contains("OS denied"))
    }));
    let stale = cut_id(&mut d);
    press(&mut d, "ArrowRight", false, false);
    d.complete_clipboard(stale, Ok(None)).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "old");
    let accepted = cut_id(&mut d);
    let events = d.complete_clipboard(accepted, Ok(None)).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "");
    assert_eq!(events.iter().filter(|e| e["type"] == "input").count(), 1);
}

#[test]
fn native_clipboard_queue_and_incoming_data_are_bounded() {
    let mut d = focused();
    for _ in 0..20 {
        d.set_clipboard_text("copy").unwrap();
    }
    assert_eq!(d.take_clipboard_requests().len(), MAX_TRANSFERS);
    assert!(
        d.diagnostics()
            .iter()
            .any(|e| e["message"] == "Too many pending clipboard transfers")
    );
    let mut d = focused();
    press(&mut d, "v", true, false);
    let id = read_id(&mut d);
    d.complete_clipboard(id, Ok(Some("x".repeat(MAX_TRANSFER_BYTES + 1))))
        .unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "old");
}

#[test]
fn passwords_never_leave_via_clipboard_or_surrounding_text() {
    let mut d = document("<!doctype html><input id=secret type=password value='private😀'>");
    d.focus("secret").unwrap();
    d.enable_deferred_clipboard();
    press(&mut d, "a", true, false);
    press(&mut d, "c", true, false);
    press(&mut d, "x", true, false);
    assert!(d.take_clipboard_requests().is_empty());
    assert_eq!(d.get_value("secret").unwrap(), "private😀");
    assert_eq!(
        d.ime_surrounding(),
        Some(ImeSurrounding {
            text: String::new(),
            cursor: 0,
            anchor: 0
        })
    );
    assert!(d.ime_delete_surrounding(1, 0).is_err());
    assert_eq!(d.get_value("secret").unwrap(), "private😀");
}

#[test]
fn ime_reports_utf8_selection_direction_and_fresh_application_values() {
    let mut d = document("<!doctype html><input id=edit value='é😀z'>");
    d.focus("edit").unwrap();
    press(&mut d, "Home", false, false);
    press(&mut d, "ArrowRight", false, true);
    let state = d.ime_surrounding().unwrap();
    assert_eq!((state.cursor, state.anchor), (2, 0));
    press(&mut d, "End", false, false);
    press(&mut d, "ArrowLeft", false, true);
    let state = d.ime_surrounding().unwrap();
    assert_eq!((state.cursor, state.anchor), (6, 7));
    d.set_value("edit", "fresh").unwrap();
    assert_eq!(d.ime_surrounding().unwrap().text, "fresh");
    d.set_value("edit", &"é".repeat(6000)).unwrap();
    let state = d.ime_surrounding().unwrap();
    assert!(state.text.len() < ui4me_engine_types::ime::MAX_SURROUNDING_BYTES);
    assert!(state.text.is_char_boundary(state.cursor));
    assert!(state.text.is_char_boundary(state.anchor));
}

#[test]
fn ime_delete_surrounding_uses_bytes_and_rejects_partial_codepoints() {
    let mut d = document("<!doctype html><input id=edit value='Aé😀Z'>");
    d.focus("edit").unwrap();
    press(&mut d, "ArrowLeft", false, false);
    assert!(d.ime_delete_surrounding(1, 0).is_err());
    assert_eq!(d.get_value("edit").unwrap(), "Aé😀Z");
    let events = d.ime_delete_surrounding(4, 1).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "Aé");
    assert_eq!(events.iter().filter(|e| e["type"] == "input").count(), 1);
    press(&mut d, "z", true, false);
    assert_eq!(d.get_value("edit").unwrap(), "Aé😀Z");
}

#[test]
fn ime_surrounding_delete_during_preedit_is_one_undo_transaction() {
    let mut d = document("<!doctype html><input id=edit value='AéZ'>");
    d.focus("edit").unwrap();
    press(&mut d, "ArrowLeft", false, false);
    d.handle_input(Input::ImePreedit {
        text: "仮".into(),
        cursor: None,
    })
    .unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "Aé仮Z");
    assert_eq!(d.ime_surrounding().unwrap().text, "AéZ");
    d.ime_delete_surrounding(2, 1).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "A");
    d.handle_input(Input::ImeCommit("確".into())).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "A確");
    press(&mut d, "z", true, false);
    assert_eq!(d.get_value("edit").unwrap(), "AéZ");
}

#[test]
fn hosted_scope_is_enforced_on_html_and_mutations_with_atomic_rollback() {
    assert!(Document::parse_hosted("<div contenteditable>rich</div>", None, 80, 80).is_err());
    assert!(
        Document::parse_hosted(
            "<div contenteditable=plaintext-only>edit</div>",
            None,
            80,
            80
        )
        .is_err()
    );
    let mut d = document("<!doctype html><div id=box contenteditable=false>old</div>");
    assert!(
        d.insert_html("box", "beforeend", "<b contenteditable>new</b>")
            .is_err()
    );
    assert!(
        d.replace_children_html("box", "<span contenteditable=true>new</span>")
            .is_err()
    );
    assert!(
        d.set_attribute("box", "contenteditable", Some("true"))
            .is_err()
    );
    assert!(
        d.apply_batch(&[
            DomOp::SetText {
                element: "box".into(),
                text: "changed".into()
            },
            DomOp::SetAttribute {
                element: "box".into(),
                name: "contenteditable".into(),
                value: Some("".into())
            },
        ])
        .is_err()
    );
    assert_eq!(d.get_value("box").unwrap(), "old");
    assert!(d.paginate(None, None, true).is_err());
    d.add_stylesheet("@page {size:A4} #box{columns:2;break-inside:avoid;color:red}")
        .unwrap();
    assert!(d.render(240, 160, 1.0).is_ok());
    assert!(d.paginate(None, None, true).is_err());
    assert_eq!(crate::css_profile().0, "ui4me-desktop-1.0");
}

#[test]
fn hosted_assets_have_explicit_separate_budgets() {
    let d = document("<!doctype html><p>bounded</p>");
    let resources = lock(&d.resources);
    assert_eq!(resources.defaults().decoded_memory_budget, DECODED_BYTES);
    assert_eq!(resources.defaults().cache_budget, CACHE_BYTES);
}

#[test]
fn cancelling_keydown_suppresses_only_its_paired_text_and_space_activates_once() {
    let mut d = document("<!doctype html><input id=edit><button id=go>Go</button>");
    d.focus("edit").unwrap();
    d.set_default_action(
        DefaultActionRule {
            element: "edit".into(),
            event: "keydown".into(),
            key: Some("x".into()),
            shift: None,
            ctrl: None,
            alt: None,
            meta: None,
        },
        true,
    )
    .unwrap();
    let events = press(&mut d, "x", false, false);
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "keydown" && e["default_prevented"] == true)
    );
    d.handle_input(Input::Text("x".into())).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "");
    press(&mut d, "y", false, false);
    d.handle_input(Input::Text("y".into())).unwrap();
    assert_eq!(d.get_value("edit").unwrap(), "y");
    d.focus("go").unwrap();
    d.drain_events();
    let mut events = press(&mut d, " ", false, false);
    events.extend(d.handle_input(Input::Text(" ".into())).unwrap());
    let mut up = key(" ", false, false);
    if let Input::Key { pressed, .. } = &mut up {
        *pressed = false;
    }
    events.extend(d.handle_input(up).unwrap());
    assert_eq!(events.iter().filter(|e| e["type"] == "click").count(), 1);
}

#[test]
fn late_file_picker_checks_control_kind_disabled_and_multiple() {
    let mut d = document("<!doctype html><input id=pick type=file>");
    let node = d.node("pick").unwrap();
    assert!(
        d.complete_file_input(node, Some(vec!["a.txt".into(), "b.txt".into()]))
            .is_err()
    );
    assert!(d.get_files("pick").unwrap().is_empty());
    d.set_attribute("pick", "disabled", Some("")).unwrap();
    d.complete_file_input(node, Some(vec!["a.txt".into()]))
        .unwrap();
    assert!(d.get_files("pick").unwrap().is_empty());
    d.set_attribute("pick", "disabled", None).unwrap();
    d.set_attribute("pick", "inert", Some("")).unwrap();
    d.complete_file_input(node, Some(vec!["a.txt".into()]))
        .unwrap();
    assert!(d.get_files("pick").unwrap().is_empty());
    d.set_attribute("pick", "inert", None).unwrap();
    d.set_attribute("pick", "type", Some("text")).unwrap();
    d.complete_file_input(node, Some(vec!["a.txt".into()]))
        .unwrap();
    d.set_attribute("pick", "type", Some("file")).unwrap();
    assert!(d.get_files("pick").unwrap().is_empty());
    assert!(d.drain_events().iter().all(|e| e["type"] != "input"));
}

#[test]
fn percent_sized_svg_with_viewbox_renders_and_targets_only_its_root() {
    let mut d = document(
        r#"<!doctype html><style>body{margin:0}#box{width:40px;height:40px}svg{display:block}</style><div id=box><svg id=icon width="100%" height="100%" viewBox="0 0 10 10"><rect id=shape width=10 height=10 fill="red"/></svg></div>"#,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let pixels = loop {
        let px = d.render(240, 160, 1.0).unwrap();
        if !d.has_pending_resources() {
            break px;
        }
        assert!(Instant::now() < deadline, "SVG did not settle");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(pixels[5 * 240 + 5], 0x00ff0000);
    let mut events = d
        .handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x: 5.0,
            y: 5.0,
        })
        .unwrap();
    events.extend(
        d.handle_input(Input::PointerButton {
            pressed: false,
            button: 0,
            x: 5.0,
            y: 5.0,
        })
        .unwrap(),
    );
    assert_eq!(
        events.iter().find(|e| e["type"] == "click").unwrap()["element"],
        "icon"
    );
}

#[test]
fn svg_image_uses_its_display_size_and_device_scale_instead_of_upscaling_natural_pixels() {
    let source = "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4' viewBox='0 0 4 4'><path d='M0 4 L2 0 L4 4 Z' fill='#104080'/></svg>";
    let url = crate::pipeline::inline_svg::data_url(source);
    let mut d = document(&format!(
        "<!doctype html><style>body{{margin:0}}#row{{display:flex}}img,svg{{display:block;width:64px;height:64px}}</style><div id=row><img src=\"{url}\"><svg width='64' height='64' viewBox='0 0 4 4'><path d='M0 4 L2 0 L4 4 Z' fill='#104080'/></svg></div>"
    ));
    for scale in [1.0, 2.0] {
        let side = (64.0 * scale) as usize;
        let width = (128.0 * scale) as u32;
        let height = (80.0 * scale) as u32;
        let deadline = Instant::now() + Duration::from_secs(5);
        let pixels = loop {
            let px = d.render(width, height, scale).unwrap();
            if !d.has_pending_resources() {
                break px;
            }
            assert!(Instant::now() < deadline, "SVG did not settle");
            std::thread::sleep(Duration::from_millis(5));
        };
        for y in 0..side {
            for x in 0..side {
                assert_eq!(
                    pixels[y * width as usize + x],
                    pixels[y * width as usize + side + x],
                    "external vs inline SVG differs at ({x},{y}) with scale {scale}"
                );
            }
        }
    }
}
