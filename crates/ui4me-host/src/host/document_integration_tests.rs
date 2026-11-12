//! Tests of the production document/native seam without a desktop, sockets or
//! substitute editor. Native completions enter the same owner-thread methods
//! used by the platform adapters; the renderer and protocol are the real ones.
use super::*;
use ui4me_renderer::{ClipboardRequest, Document, Input};

fn controller(html: &str) -> (Host, String) {
    let mut host = super::tests::controller(true);
    super::tests::add_window(&mut host, "w1");
    let id = load(&mut host, "w1", html);
    host.observed_notifications.clear();
    (host, id)
}

fn load(host: &mut Host, window: &str, html: &str) -> String {
    host.dispatch(
        "document.load",
        json!({"window_id":window,"html":html}),
        None,
        None,
    )
    .unwrap()
    .unwrap()["document_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn command(host: &mut Host, id: &str, command: Value) -> Value {
    host.dispatch(
        "document.command",
        json!({"window_id":"w1","document_id":id,"command":command}),
        None,
        None,
    )
    .unwrap()
    .unwrap()
}

fn engine(host: &mut Host) -> &mut Document {
    &mut host
        .windows
        .get_mut("w1")
        .unwrap()
        .document
        .as_mut()
        .unwrap()
        .engine
}

fn key(name: &str, ctrl: bool) -> Input {
    Input::Key {
        key: name.into(),
        code: name.into(),
        pressed: true,
        repeat: false,
        shift: false,
        ctrl,
        alt: false,
        meta: false,
    }
}

fn paste_request(host: &mut Host) -> u64 {
    let document = engine(host);
    document.enable_deferred_clipboard();
    document.handle_input(Input::Focus(true)).unwrap();
    document.focus("text").unwrap();
    document.handle_input(key("v", true)).unwrap();
    let requests = document.take_clipboard_requests();
    assert_eq!(
        requests.len(),
        1,
        "real editing shortcut must request the native clipboard"
    );
    match requests[0] {
        ClipboardRequest::Read { id } => id,
        _ => panic!("paste queued a write"),
    }
}

fn file_request_node(host: &mut Host) -> ui4me_renderer::NodeId {
    let document = engine(host);
    document.handle_input(Input::Focus(true)).unwrap();
    document.focus("pick").unwrap();
    document.handle_input(key("Enter", false)).unwrap();
    let requests = document.take_file_requests();
    assert_eq!(
        requests.len(),
        1,
        "real file-input activation must request a picker"
    );
    requests[0].input
}

fn document_events(host: &Host) -> Vec<&Value> {
    host.observed_notifications
        .iter()
        .filter(|value| value["method"] == "document.event")
        .collect()
}

#[test]
fn late_clipboard_completion_cannot_paste_into_replacement_with_reused_request_id() {
    let (mut host, first) = controller("<input id='text'>");
    let old_request = paste_request(&mut host);
    let second = load(&mut host, "w1", "<input id='text'>");
    let live_request = paste_request(&mut host);
    assert_eq!(
        old_request, live_request,
        "separate documents may reuse renderer request IDs"
    );
    host.observed_notifications.clear();
    host.complete_document_clipboard("w1", &first, old_request, Ok(Some("WRONG".into())));
    assert_eq!(engine(&mut host).get_value("text").unwrap(), "");
    assert!(host.observed_notifications.is_empty());
    host.complete_document_clipboard("w1", &second, live_request, Ok(Some("Café 👋".into())));
    assert_eq!(engine(&mut host).get_value("text").unwrap(), "Café 👋");
    let events = document_events(&host);
    assert!(events
        .iter()
        .any(|e| e["params"]["event"]["type"] == "input"));
    assert!(events.iter().all(|e| e["params"]["document_id"] == second));
    let delivered = events.len();
    host.complete_document_clipboard("w1", &second, live_request, Ok(Some("twice".into())));
    assert_eq!(engine(&mut host).get_value("text").unwrap(), "Café 👋");
    assert_eq!(
        document_events(&host).len(),
        delivered,
        "completion is consumed once"
    );
}

#[test]
fn late_drop_is_discarded_but_live_drop_keeps_document_epoch_and_unicode() {
    let (mut host, first) = controller("<main id='target'>Drop here</main>");
    let second = load(&mut host, "w1", "<main id='target'>New document</main>");
    host.observed_notifications.clear();
    host.finish_transfer(
        TransferTarget::DocumentDrop {
            window_id: "w1".into(),
            document_id: first,
            files: false,
            x: 12.0,
            y: 12.0,
        },
        Ok("old drop".into()),
    );
    assert!(host.observed_notifications.is_empty());
    host.finish_transfer(
        TransferTarget::DocumentDrop {
            window_id: "w1".into(),
            document_id: second.clone(),
            files: false,
            x: 12.0,
            y: 12.0,
        },
        Ok("é日👋".into()),
    );
    let events = document_events(&host);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["params"]["window_id"], "w1");
    assert_eq!(events[0]["params"]["document_id"], second);
    assert_eq!(events[0]["params"]["event"]["text"], "é日👋");
    assert_eq!(events[0]["params"]["event"]["external"], true);
    host.windows.get_mut("w1").unwrap().closing = true;
    host.complete_document_drop("w1", &second, false, (12.0, 12.0), Ok("after close".into()));
    assert_eq!(document_events(&host).len(), 1);
}

#[test]
fn replacement_and_unload_prune_only_their_document_transfers_and_cancel_their_picker_jobs() {
    let (mut host, first) = controller("<input id='pick' type='file'>");
    let node = file_request_node(&mut host);
    super::tests::add_window(&mut host, "w2");
    let other = load(&mut host, "w2", "<p>other window</p>");
    let old_clipboard = AsyncRequestSerial::get();
    let old_drop = AsyncRequestSerial::get();
    let other_transfer = AsyncRequestSerial::get();
    let rpc = AsyncRequestSerial::get();
    for (serial, target) in [
        (
            old_clipboard,
            TransferTarget::DocumentClipboard {
                window_id: "w1".into(),
                document_id: first.clone(),
                request_id: 1,
            },
        ),
        (
            old_drop,
            TransferTarget::DocumentDrop {
                window_id: "w1".into(),
                document_id: first.clone(),
                files: true,
                x: 0.0,
                y: 0.0,
            },
        ),
        (
            other_transfer,
            TransferTarget::DocumentClipboard {
                window_id: "w2".into(),
                document_id: other,
                request_id: 1,
            },
        ),
        (rpc, TransferTarget::Rpc((7, "rpc".into()))),
    ] {
        host.transfers.insert(
            serial,
            PendingTransfer {
                target,
                deadline: Instant::now() + Duration::from_secs(10),
            },
        );
    }
    host.deferred_clipboard_reads
        .push(TransferTarget::DocumentClipboard {
            window_id: "w1".into(),
            document_id: first.clone(),
            request_id: 2,
        });
    host.deferred_clipboard_reads
        .push(TransferTarget::Rpc((7, "deferred-rpc".into())));
    host.jobs.insert(
        10,
        PendingJob {
            reply: None,
            parent: Some("w1".into()),
            kind: JobKind::DocumentDialog {
                document_id: first,
                node,
            },
            cancelled: false,
            dialog: None,
        },
    );
    host.jobs.insert(
        11,
        PendingJob {
            reply: Some((7, "host-dialog".into())),
            parent: Some("w1".into()),
            kind: JobKind::Dialog,
            cancelled: false,
            dialog: None,
        },
    );
    let second = load(&mut host, "w1", "<input id='pick' type='file'>");
    assert!(!host.transfers.contains_key(&old_clipboard));
    assert!(!host.transfers.contains_key(&old_drop));
    assert!(host.transfers.contains_key(&other_transfer));
    assert!(host.transfers.contains_key(&rpc));
    assert_eq!(host.deferred_clipboard_reads.len(), 1);
    assert!(matches!(
        host.deferred_clipboard_reads[0],
        TransferTarget::Rpc(_)
    ));
    assert!(host.jobs[&10].cancelled);
    assert!(!host.jobs[&11].cancelled);
    let current = AsyncRequestSerial::get();
    host.transfers.insert(
        current,
        PendingTransfer {
            target: TransferTarget::DocumentClipboard {
                window_id: "w1".into(),
                document_id: second.clone(),
                request_id: 3,
            },
            deadline: Instant::now() + Duration::from_secs(10),
        },
    );
    host.dispatch(
        "document.unload",
        json!({"window_id":"w1","document_id":second}),
        None,
        None,
    )
    .unwrap();
    assert!(!host.transfers.contains_key(&current));
    assert!(host.transfers.contains_key(&other_transfer));
    assert!(host.transfers.contains_key(&rpc));
}

#[test]
fn picker_completion_is_scoped_to_document_and_generation_even_when_html_ids_repeat() {
    let (mut host, first) = controller("<main id='root'><input id='pick' type='file'></main>");
    let old_node = file_request_node(&mut host);
    let second = load(
        &mut host,
        "w1",
        "<main id='root'><input id='pick' type='file'></main>",
    );
    let live_node = file_request_node(&mut host);
    assert_eq!(
        old_node, live_node,
        "node slots may repeat in a fresh document"
    );
    host.observed_notifications.clear();
    host.complete_document_dialog(
        "w1",
        &first,
        old_node,
        Ok(json!({"cancelled":false,"paths":["old.txt"]})),
        None,
    );
    host.complete_document_dialog(
        "w1",
        &first,
        old_node,
        Err(RpcError::unavailable("stale worker")),
        None,
    );
    assert!(engine(&mut host).get_files("pick").unwrap().is_empty());
    assert!(host.observed_notifications.is_empty());
    host.complete_document_dialog(
        "w1",
        &second,
        live_node,
        Ok(json!({"cancelled":false,"paths":["Café 👋.txt"]})),
        None,
    );
    assert_eq!(
        engine(&mut host).get_files("pick").unwrap(),
        ["Café 👋.txt"]
    );
    assert!(document_events(&host)
        .iter()
        .any(|e| e["params"]["event"]["type"] == "input"));
    assert_eq!(
        document_events(&host)
            .iter()
            .filter(|e| e["params"]["event"]["type"] == "input")
            .count(),
        1
    );
    assert_eq!(
        document_events(&host)
            .iter()
            .filter(|e| e["params"]["event"]["type"] == "change")
            .count(),
        1
    );
    command(
        &mut host,
        &second,
        json!({"op":"remove_element","element":"pick"}),
    );
    command(
        &mut host,
        &second,
        json!({"op":"insert_html","element":"root", "position":"beforeend","html":"<input id='pick' type='file'>"}),
    );
    host.observed_notifications.clear();
    host.complete_document_dialog(
        "w1",
        &second,
        live_node,
        Ok(json!({"cancelled":false,"paths":["stale-node.txt"]})),
        None,
    );
    assert!(engine(&mut host).get_files("pick").unwrap().is_empty());
    assert!(host.observed_notifications.is_empty());
}

#[test]
fn accessibility_actions_queued_before_replacement_do_not_reach_new_controls() {
    use accesskit::{Action, ActionData, ActionRequest, NodeId, TreeId};
    let (mut host, first) = controller("<input id='text' value='first'>");
    let tree = engine(&mut host).accessibility_tree().unwrap();
    let target = tree
        .nodes
        .values()
        .find(|n| n.html_id.as_deref() == Some("text"))
        .unwrap()
        .id;
    let second = load(&mut host, "w1", "<input id='text' value='second'>");
    let request = |text: &str| ActionRequest {
        action: Action::SetValue,
        target_tree: TreeId::ROOT,
        target_node: NodeId(target),
        data: Some(ActionData::Value(text.into())),
    };
    host.observed_notifications.clear();
    host.accessibility_action(
        ActionEvent {
            window_id: "w1".into(),
            document_id: Some(first),
            request: request("old action"),
        },
        None,
    );
    assert_eq!(engine(&mut host).get_value("text").unwrap(), "second");
    assert!(host.observed_notifications.is_empty());
    host.accessibility_action(
        ActionEvent {
            window_id: "w1".into(),
            document_id: Some(second),
            request: request("current action"),
        },
        None,
    );
    assert_eq!(
        engine(&mut host).get_value("text").unwrap(),
        "current action"
    );
}

#[test]
fn command_events_are_epoch_wrapped_once_and_removed_element_events_are_discarded() {
    let (mut host, id) =
        controller("<main id='root'><input id='text'><dialog id='dialog'>Message</dialog></main>");
    command(&mut host, &id, json!({"op":"focus","element":"text"}));
    host.observed_notifications.clear();
    host.document_input("w1", Input::Text("typed".into()), None);
    assert_eq!(
        command(&mut host, &id, json!({"op":"get_value","element":"text"}))["value"],
        "typed"
    );
    let events = document_events(&host);
    assert!(events
        .iter()
        .any(|e| e["params"]["event"]["type"] == "input"));
    assert_eq!(
        events
            .iter()
            .filter(|e| e["params"]["event"]["type"] == "input")
            .count(),
        1
    );
    assert!(events
        .iter()
        .all(|e| e["params"]["event"]["type"] != "change"));
    assert!(events
        .iter()
        .all(|e| e["params"]["document_id"] == id && e["params"]["window_id"] == "w1"));
    let delivered = events.len();
    host.document_changed("w1", None);
    assert_eq!(document_events(&host).len(), delivered);
    let pending = engine(&mut host)
        .handle_input(Input::Text("!".into()))
        .unwrap();
    assert!(pending.iter().any(|e| e["type"] == "input"));
    command(
        &mut host,
        &id,
        json!({"op":"remove_element","element":"text"}),
    );
    command(
        &mut host,
        &id,
        json!({"op":"insert_html","element":"root","position":"beforeend","html":"<input id='text'>"}),
    );
    host.observed_notifications.clear();
    let element_events: Vec<_> = pending
        .into_iter()
        .filter(|e| e.get("_target").is_some())
        .collect();
    assert!(!element_events.is_empty());
    host.emit_document_events("w1", &id, element_events);
    assert!(host.observed_notifications.is_empty());
    command(
        &mut host,
        &id,
        json!({"op":"show_dialog","element":"dialog"}),
    );
    host.observed_notifications.clear();
    command(
        &mut host,
        &id,
        json!({"op":"close_dialog","element":"dialog","return_value":"done"}),
    );
    assert_eq!(
        document_events(&host)
            .iter()
            .filter(|e| e["params"]["event"]["type"] == "close")
            .count(),
        1
    );
    host.document_changed("w1", None);
    assert_eq!(
        document_events(&host)
            .iter()
            .filter(|e| e["params"]["event"]["type"] == "close")
            .count(),
        1
    );
}

#[test]
fn registered_keydown_policy_suppresses_the_paired_native_text_only() {
    let (mut host, id) = controller("<input id='text'>");
    command(&mut host, &id, json!({"op":"focus","element":"text"}));
    command(
        &mut host,
        &id,
        json!({"op":"set_default_action","element":"text","event":"keydown","key":"x","prevent":true}),
    );
    host.observed_notifications.clear();
    // This is the same separate Key/Text sequence emitted by winit handling.
    host.document_input("w1", key("x", false), None);
    host.document_input("w1", Input::Text("x".into()), None);
    assert_eq!(engine(&mut host).get_value("text").unwrap(), "");
    assert!(document_events(&host).iter().any(|e| {
        e["params"]["event"]["type"] == "keydown"
            && e["params"]["event"]["default_prevented"] == true
    }));
    host.document_input("w1", key("y", false), None);
    host.document_input("w1", Input::Text("y".into()), None);
    assert_eq!(engine(&mut host).get_value("text").unwrap(), "y");
}
