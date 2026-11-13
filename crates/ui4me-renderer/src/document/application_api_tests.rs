use super::*;

fn doc(html: &str) -> Document {
    Document::parse_hosted(html, None, 640, 480).unwrap()
}

#[test]
fn anonymous_node_mutations_are_atomic_and_stale_references_do_not_retarget() {
    let mut d = doc("<div><span>before</span><em>keep</em></div>");
    let q = d.query_nodes("span", 0, 10).unwrap();
    let node = q["nodes"][0]["node"].as_str().unwrap().to_owned();
    assert!(q["nodes"][0]["element"].is_null());
    d.apply_node_batch(&[NodeOp::SetText {
        node: node.clone(),
        text: "after".into(),
    }])
    .unwrap();
    assert_eq!(d.get_html(None, Some(&node), false).unwrap(), "after");
    assert!(
        d.apply_node_batch(&[
            NodeOp::SetText {
                node: node.clone(),
                text: "must roll back".into()
            },
            NodeOp::SetAttribute {
                node: "node:999999:1".into(),
                name: "x".into(),
                value: Some("y".into())
            }
        ])
        .is_err()
    );
    assert_eq!(d.get_html(None, Some(&node), false).unwrap(), "after");
    d.apply_node_batch(&[NodeOp::RemoveElement { node: node.clone() }])
        .unwrap();
    assert_eq!(
        d.apply_node_batch(&[NodeOp::SetText {
            node,
            text: "stale".into()
        }])
        .unwrap_err()
        .code,
        4
    );
}

#[test]
fn utf16_replacement_undo_redo_preserves_selection_and_cancel_is_atomic() {
    let mut d = doc("<input id=t value='A😀B'>");
    d.set_control_selection(Some("t"), None, 1, 3, "backward")
        .unwrap();
    assert!(
        d.set_control_selection(Some("t"), None, 2, 3, "forward")
            .is_err()
    );
    let result = d
        .replace_control_range(Some("t"), None, "X", Some(1), Some(3), "select")
        .unwrap();
    assert_eq!(result["start"], 1);
    assert_eq!(result["end"], 2);
    assert_eq!(d.get_value("t").unwrap(), "AXB");
    d.edit_command("undo", Some("t"), None).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "A😀B");
    assert_eq!(
        d.control_selection(Some("t"), None).unwrap()["direction"],
        "backward"
    );
    d.edit_command("redo", Some("t"), None).unwrap();
    assert_eq!(d.control_selection(Some("t"), None).unwrap()["end"], 2);
    d.set_default_action(
        DefaultActionRule {
            element: "t".into(),
            event: "beforeinput".into(),
            key: None,
            shift: None,
            ctrl: None,
            alt: None,
            meta: None,
        },
        true,
    )
    .unwrap();
    let before = d.control_selection(Some("t"), None).unwrap();
    assert_eq!(
        d.replace_control_range(Some("t"), None, "no", None, None, "end")
            .unwrap()["changed"],
        false
    );
    assert_eq!(d.get_value("t").unwrap(), "AXB");
    assert_eq!(d.control_selection(Some("t"), None).unwrap(), before);
}

#[test]
fn ordered_form_entries_preserve_duplicates_external_controls_submitter_and_files() {
    let mut d = doc(
        "<input name=outside form=f value=first><form id=f><input name=x value=a><input name=x value=b><input name=off type=checkbox><input name=on type=checkbox checked><input name=no disabled value=hidden><select name=s multiple><option selected value=one>One<option selected value=two>Two</select><input id=file name=attachment type=file multiple><button id=go name=action value=save>Save</button><button name=action value=other>Other</button></form>",
    );
    d.complete_file_input(
        d.node("file").unwrap(),
        Some(vec!["/tmp/α.txt".into(), "F:\\work\\b.txt".into()]),
    )
    .unwrap();
    let entries = d.form_data(Some("f"), None, Some("go"), None).unwrap()["entries"]
        .as_array()
        .unwrap()
        .clone();
    let names: Vec<_> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "outside",
            "x",
            "x",
            "on",
            "s",
            "s",
            "attachment",
            "attachment",
            "action"
        ]
    );
    assert_eq!(entries[3]["value"], "on");
    assert_eq!(entries[6]["filename"], "α.txt");
    assert_eq!(entries[7]["filename"], "b.txt");
    assert_eq!(entries[8]["value"], "save");
}

#[test]
fn reset_clears_files_and_history_of_external_associated_controls() {
    let mut d =
        doc("<form id=f></form><input id=t form=f value=start><input id=file type=file form=f>");
    d.complete_file_input(d.node("file").unwrap(), Some(vec!["/tmp/chosen".into()]))
        .unwrap();
    d.replace_control_range(Some("t"), None, "changed", Some(0), Some(5), "end")
        .unwrap();
    d.reset_form(Some("f"), None).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "start");
    assert!(d.get_files("file").unwrap().is_empty());
    d.edit_command("undo", Some("t"), None).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "start");
}

#[test]
fn validity_blocks_submission_until_custom_error_is_cleared() {
    let mut d = doc(
        "<style>input{color:green}input:invalid{color:red}</style><form id=f><input id=t value=ok><button id=go>Submit</button></form>",
    );
    assert_eq!(
        d.set_custom_validity(Some("t"), None, "Please check this")
            .unwrap()["valid"],
        false
    );
    assert_eq!(
        d.request_submit(Some("f"), None, Some("go"), None).unwrap()["submitted"],
        false
    );
    assert_eq!(
        d.computed_style(Some("t"), None, &["color".into()])
            .unwrap()["properties"]["color"]["value"],
        "rgb(255, 0, 0)"
    );
    assert!(
        d.accessibility_tree()
            .unwrap()
            .nodes
            .values()
            .find(|n| n.html_id.as_deref() == Some("t"))
            .unwrap()
            .states
            .invalid
    );
    d.set_custom_validity(Some("t"), None, "").unwrap();
    assert_eq!(
        d.computed_style(Some("t"), None, &["color".into()])
            .unwrap()["properties"]["color"]["value"],
        "rgb(0, 128, 0)"
    );
    assert!(
        !d.accessibility_tree()
            .unwrap()
            .nodes
            .values()
            .find(|n| n.html_id.as_deref() == Some("t"))
            .unwrap()
            .states
            .invalid
    );
    assert_eq!(
        d.request_submit(Some("f"), None, Some("go"), None).unwrap()["submitted"],
        true
    );
    assert!(d.request_submit(Some("f"), None, Some("t"), None).is_err());
}

#[test]
fn preferences_keep_unknown_system_state_and_application_precedence() {
    let mut d = doc("<p>Ready</p>");
    assert!(
        d.set_system_preferences(Some(true), Some(true), None)
            .unwrap()
    );
    assert_eq!(d.environment()["reduced_motion_source"], "system");
    d.set_app_environment(Some(Some(false)), Some(Some(false)))
        .unwrap();
    assert_eq!(d.environment()["reduced_motion"], "no-preference");
    d.set_system_preferences(None, None, None).unwrap();
    assert_eq!(d.environment()["color_scheme_source"], "app");
    d.set_app_environment(Some(None), Some(None)).unwrap();
    assert_eq!(d.environment()["color_scheme_source"], "default");
    assert!(d.environment()["system_reduced_motion"].is_null());
}

#[test]
fn focus_identity_and_blur_work_for_anonymous_controls() {
    let mut d = doc("<input id=t><input>");
    d.focus("t").unwrap();
    assert_eq!(d.focused_element()["element"], "t");
    d.blur(None, None).unwrap();
    assert!(d.focused_element().is_null());
    assert!(d.element_at_point(-1.0, 0.0).unwrap().is_null());
}

#[test]
fn programmatic_selection_emits_selection_notification_with_utf16_payload() {
    let mut d = doc("<textarea id=t>😀abc</textarea>");
    d.drain_events();
    d.set_control_selection(Some("t"), None, 2, 5, "forward")
        .unwrap();
    let events = d.drain_events();
    let event = events
        .iter()
        .find(|e| e["type"] == "selectionchange")
        .unwrap();
    assert_eq!(event["selection"]["start"], 2);
    assert_eq!(event["selection"]["end"], 5);
    assert!(event["node"].as_str().unwrap().starts_with("node:"));
}

#[test]
fn accessibility_metadata_carries_live_flags_table_spans_and_relations() {
    let mut d = doc(
        "<div id=status role=status aria-busy=true>Loading</div><span id=help>Helpful text</span><input id=t aria-describedby=help><table id=table><tr><th id=h rowspan=2>Header</th><td id=a>A</td></tr><tr><td id=b>B</td></tr></table><div id=grid role=grid aria-rowcount=12 aria-colcount=4><div role=row aria-rowindex=9><div id=cell role=gridcell aria-colindex=3>C</div></div></div><div role=tree><div id=treeitem role=treeitem aria-level=2 aria-posinset=3 aria-setsize=7>Branch</div></div>",
    );
    let tree = d.accessibility_tree().unwrap();
    tree.validate().unwrap();
    let find = |name: &str| {
        tree.nodes
            .values()
            .find(|n| n.html_id.as_deref() == Some(name))
            .unwrap()
    };
    assert!(find("status").states.busy);
    assert!(find("status").states.live_atomic);
    assert_eq!(find("t").relations.described_by, vec![find("help").id]);
    assert_eq!(find("table").structure.row_count, Some(2));
    assert_eq!(find("table").structure.column_count, Some(2));
    assert_eq!(find("h").structure.row_span, Some(2));
    assert_eq!(find("b").structure.column_index, Some(2));
    assert_eq!(find("grid").role, accesskit::Role::Grid);
    assert_eq!(find("grid").structure.row_count, Some(12));
    assert_eq!(find("cell").structure.row_index, Some(9));
    assert_eq!(find("cell").structure.column_index, Some(3));
    assert_eq!(find("treeitem").level, Some(2));
    assert_eq!(find("treeitem").structure.position_in_set, Some(3));
    let update = crate::a11y::full_update(&tree);
    let (_, status) = update
        .nodes
        .iter()
        .find(|(id, _)| id.0 == find("status").id)
        .unwrap();
    assert!(status.is_busy());
    assert!(status.is_live_atomic());
    let (_, cell) = update
        .nodes
        .iter()
        .find(|(id, _)| id.0 == find("cell").id)
        .unwrap();
    assert_eq!(cell.row_index(), Some(8));
    assert_eq!(cell.column_index(), Some(2));
}

#[test]
fn assistive_scroll_reveals_the_requested_text_run_without_changing_selection() {
    let text = (0..30).map(|i| format!("line {i}\n")).collect::<String>();
    let mut d = doc(&format!(
        "<style>textarea{{width:200px;height:50px;line-height:20px}}</style><textarea id=t>{text}</textarea>"
    ));
    d.render(640, 480, 1.0).unwrap();
    let tree = d.accessibility_tree().unwrap();
    let field = tree
        .nodes
        .values()
        .find(|n| n.html_id.as_deref() == Some("t"))
        .unwrap();
    let run = field.children[field.children.len() - 2];
    let before = d.control_selection(Some("t"), None).unwrap();
    let request = crate::a11y::decode_request(&accesskit::ActionRequest {
        action: accesskit::Action::ScrollIntoView,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(run),
        data: Some(accesskit::ActionData::ScrollHint(
            accesskit::ScrollHint::BottomEdge,
        )),
    })
    .unwrap();
    d.accessibility_request(&request).unwrap();
    assert!(d.scroll_state(Some("t")).unwrap().y > 0.0);
    assert_eq!(d.control_selection(Some("t"), None).unwrap(), before);
    assert!(d.focused_element().is_null());
}

#[test]
fn drop_policy_is_opt_in_and_completion_keeps_original_node_identity() {
    let mut d = doc(
        "<style>div{width:200px;height:100px}body{margin:0}</style><div id=drop data-ui4me-drop='files text'>Drop</div>",
    );
    d.render(640, 480, 1.0).unwrap();
    let target = d.native_drop_target(20.0, 20.0, true).unwrap();
    assert_eq!(target["element"], "drop");
    assert!(d.native_drop_accepts(&target, true));
    d.set_attribute("drop", "data-ui4me-drop", Some("text"))
        .unwrap();
    assert!(!d.native_drop_accepts(&target, true));
    assert!(d.native_drop_accepts(&target, false));
    d.remove_element("drop").unwrap();
    assert!(!d.native_drop_accepts(&target, false));
}

#[test]
fn native_pointer_sequences_deliver_double_aux_and_keyboard_context_events() {
    let mut d = doc("<button id=b style='width:120px;height:60px'>Button</button>");
    d.render(640, 480, 1.0).unwrap();
    d.drain_events();
    let mut events = Vec::new();
    for _ in 0..2 {
        events.extend(
            d.handle_input(Input::PointerButton {
                pressed: true,
                button: 0,
                x: 20.0,
                y: 20.0,
            })
            .unwrap(),
        );
        events.extend(
            d.handle_input(Input::PointerButton {
                pressed: false,
                button: 0,
                x: 20.0,
                y: 20.0,
            })
            .unwrap(),
        );
    }
    assert_eq!(events.iter().filter(|e| e["type"] == "click").count(), 2);
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "dblclick" && e["detail"] == 2)
    );
    d.handle_input(Input::PointerButton {
        pressed: true,
        button: 1,
        x: 20.0,
        y: 20.0,
    })
    .unwrap();
    let events = d
        .handle_input(Input::PointerButton {
            pressed: false,
            button: 1,
            x: 20.0,
            y: 20.0,
        })
        .unwrap();
    assert!(events.iter().any(|e| e["type"] == "auxclick"));
    d.focus("b").unwrap();
    let events = d
        .handle_input(Input::Key {
            key: "ContextMenu".into(),
            code: "ContextMenu".into(),
            pressed: true,
            repeat: false,
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        })
        .unwrap();
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "contextmenu" && e["element"] == "b")
    );
}

#[test]
fn edits_of_other_controls_preserve_active_ime_and_readonly_values() {
    let mut d = doc(
        "<input id=active value=a><input id=other value=other><input id=locked value=locked readonly><input id=disabled value=disabled disabled>",
    );
    d.focus("active").unwrap();
    d.handle_input(Input::ImeEnabled).unwrap();
    d.handle_input(Input::ImePreedit {
        text: "中".into(),
        cursor: Some((3, 3)),
    })
    .unwrap();
    let before = d.ime_surrounding();
    d.set_control_selection(Some("other"), None, 0, 2, "forward")
        .unwrap();
    d.replace_control_range(Some("other"), None, "X", None, None, "end")
        .unwrap();
    assert_eq!(d.ime_surrounding(), before);
    assert_eq!(d.focused_element()["element"], "active");
    let id = d.id();
    assert!(crate::interaction::editing::ime::is_composing(&mut d, id));
    for name in ["locked", "disabled"] {
        let before = d.get_value(name).unwrap();
        assert_eq!(
            d.replace_control_range(Some(name), None, "no", Some(0), Some(1), "end")
                .unwrap()["changed"],
            false
        );
        assert_eq!(d.get_value(name).unwrap(), before);
    }
    d.handle_input(Input::ImeCommit("中文".into())).unwrap();
    assert_eq!(d.get_value("active").unwrap(), "a中文");
}

#[test]
fn native_backspace_removes_complete_flag_and_zwj_clusters_and_is_undoable() {
    for value in ["🇨🇿", "👩🏽‍💻", "👨‍👩‍👧‍👦"] {
        let mut d = doc(&format!("<input id=t value='{value}'>"));
        d.focus("t").unwrap();
        d.handle_input(Input::Key {
            key: "Backspace".into(),
            code: "Backspace".into(),
            pressed: true,
            repeat: false,
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        })
        .unwrap();
        assert_eq!(d.get_value("t").unwrap(), "", "{value}");
        d.edit_command("undo", Some("t"), None).unwrap();
        assert_eq!(d.get_value("t").unwrap(), value);
    }
}

#[test]
fn application_clipboard_commands_are_deferred_and_paste_is_generation_checked() {
    struct NoSync;
    impl Clipboard for NoSync {
        fn read_text(&mut self) -> std::result::Result<Option<String>, String> {
            panic!("synchronous clipboard read")
        }
        fn write_text(&mut self, _: &str) -> std::result::Result<(), String> {
            panic!("synchronous clipboard write")
        }
    }
    let mut d = doc("<input id=t value=before>");
    d.focus("t").unwrap();
    d.enable_deferred_clipboard();
    d.set_clipboard(Box::new(NoSync));
    assert_eq!(
        d.edit_command("paste", Some("t"), None).unwrap()["clipboard_pending"],
        true
    );
    let requests = d.take_clipboard_requests();
    let ClipboardRequest::Read { id } = requests[0] else {
        panic!("expected read");
    };
    assert_eq!(d.get_value("t").unwrap(), "before");
    d.set_control_selection(Some("t"), None, 0, 1, "forward")
        .unwrap();
    d.complete_clipboard(id, Ok(Some("must not arrive".into())))
        .unwrap();
    assert_eq!(d.get_value("t").unwrap(), "before");
    d.edit_command("cut", Some("t"), None).unwrap();
    let requests = d.take_clipboard_requests();
    let ClipboardRequest::Write { id, ref text } = requests[0] else {
        panic!("expected write");
    };
    assert_eq!(text, "b");
    assert_eq!(d.get_value("t").unwrap(), "before");
    d.complete_clipboard(id, Ok(None)).unwrap();
    assert_eq!(d.get_value("t").unwrap(), "efore");
}

#[test]
fn accessible_text_lengths_cover_long_unicode_graphemes_exactly() {
    let value = format!("a{}", "\u{0301}".repeat(300));
    let mut d = doc(&format!("<textarea id=t>{value}</textarea>"));
    let tree = d.accessibility_tree().unwrap();
    let field = tree
        .nodes
        .values()
        .find(|n| n.html_id.as_deref() == Some("t"))
        .unwrap();
    for id in &field.children {
        let node = tree.get(*id).unwrap();
        let run = node.run.as_ref().unwrap();
        let text = node.value.as_ref().unwrap();
        assert_eq!(
            run.lengths.iter().map(|&n| n as usize).sum::<usize>(),
            text.len()
        );
        let mut offset = 0;
        for &length in &run.lengths {
            offset += length as usize;
            assert!(text.is_char_boundary(offset));
        }
    }
}

#[test]
fn logical_resize_changes_the_axis_selected_by_writing_mode() {
    for (writing, resize, w, h) in [
        ("horizontal-tb", "inline", 120.0, 60.0),
        ("vertical-rl", "inline", 100.0, 90.0),
        ("horizontal-tb", "block", 100.0, 90.0),
        ("vertical-lr", "block", 120.0, 60.0),
    ] {
        let mut d = doc(&format!(
            "<style>body{{margin:0}}#r{{width:100px;height:60px;overflow:auto;resize:{resize};writing-mode:{writing}}}</style><div id=r>x</div>"
        ));
        d.render(640, 480, 1.0).unwrap();
        let before = d.element_rect("r").unwrap().unwrap();
        let (x, y) = (
            before.x + before.width - 1.0,
            before.y + before.height - 1.0,
        );
        d.handle_input(Input::PointerButton {
            pressed: true,
            button: 0,
            x,
            y,
        })
        .unwrap();
        d.handle_input(Input::PointerMoved {
            x: x + 20.0,
            y: y + 30.0,
        })
        .unwrap();
        d.handle_input(Input::PointerButton {
            pressed: false,
            button: 0,
            x: x + 20.0,
            y: y + 30.0,
        })
        .unwrap();
        let after = d.element_rect("r").unwrap().unwrap();
        assert!(
            (after.width - w).abs() < 0.1,
            "{writing} {resize} width {}",
            after.width
        );
        assert!(
            (after.height - h).abs() < 0.1,
            "{writing} {resize} height {}",
            after.height
        );
    }
}

#[test]
fn direct_surface_render_matches_allocating_render_and_rejects_bad_destinations() {
    let mut d = doc(
        "<style>body{margin:0;background:#123456}p{background:rgba(255,0,0,.5);color:white}</style><p>Pixels 😀</p>",
    );
    let expected = d.render(160, 100, 1.0).unwrap();
    let mut actual = vec![0; 160 * 100];
    d.render_into(160, 100, 1.0, &mut actual).unwrap();
    assert_eq!(actual, expected);
    let mut short = vec![0x123456; 5];
    assert!(d.render_into(160, 100, 1.0, &mut short).is_err());
    assert!(d.render_into(1, 5, f64::NAN, &mut short).is_err());
    assert!(d.render_into(u32::MAX, u32::MAX, 1.0, &mut short).is_err());
    assert_eq!(short, vec![0x123456; 5]);
}

/// Core 1.0 C03 (found on KDE through real IPC): a replacement document parsed from the same HTML
/// restarts node generations, so an index/generation reference minted by the replaced document
/// named the corresponding node of the new one and modified it. References carry their document.
#[test]
fn node_reference_from_a_replaced_document_is_stale_in_its_replacement() {
    let html = "<ul><li>alpha</li><li>beta</li></ul>";
    let old = doc(html);
    let reference = old.query_nodes("li", 0, 10).unwrap()["nodes"][0]["node"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut replacement = doc(html);
    let fresh = replacement.query_nodes("li", 0, 10).unwrap()["nodes"][0]["node"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(reference, fresh, "documents must mint distinct references");
    let error = replacement
        .apply_node_batch(&[NodeOp::SetText {
            node: reference.clone(),
            text: "old-document handle".into(),
        }])
        .unwrap_err();
    assert_eq!(error.code, 4, "{}", error.message);
    assert_eq!(
        replacement.get_html(None, Some(&fresh), false).unwrap(),
        "alpha"
    );
    assert_eq!(replacement.get_node(&reference).unwrap_err().code, 4);
    // The same reference still names its node in its own document.
    assert_eq!(
        old.get_html(None, Some(&reference), false).unwrap(),
        "alpha"
    );
    // The pre-1.0 two-part form is no longer a valid reference.
    assert_eq!(replacement.get_node("node:1:1").unwrap_err().code, 1);
}
