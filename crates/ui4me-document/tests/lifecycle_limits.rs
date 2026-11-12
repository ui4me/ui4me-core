//! owner: 003-document-model
//!
//! T090 (FR-089-FR-095, SC-006 part): close outside and during a dispatch, `Closing` / `Closed`
//! behaviour of handles (DOCUMENT_CLOSED, releases OK), the document-status operations, hook and
//! notification delivery on close, limits naming the limit, and iterative algorithms on deep trees.

use std::cell::RefCell;
use std::rc::Rc;
use ui4me_document::document::{Document, Lifecycle};
use ui4me_document::hooks::Notification;
use ui4me_engine_types::codes_generated::Code;

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap()
}

// table-a: A-14
#[test]
fn close_outside_a_dispatch_is_closed_on_return() {
    let mut d = Document::parse_html("<p id=a>x</p>").unwrap();
    let a = id(&d, "a");
    let closed: Rc<RefCell<u32>> = Rc::default();
    let n: Rc<RefCell<Vec<Notification>>> = Rc::default();
    let c = closed.clone();
    d.hooks_mut()
        .set_on_document_closed(Box::new(move |_| *c.borrow_mut() += 1));
    let nn = n.clone();
    d.hooks_mut()
        .set_notification_sink(Box::new(move |x| nn.borrow_mut().push(x.clone())));
    d.retain(a).unwrap();
    d.request_close(false);
    assert_eq!(d.lifecycle(), Lifecycle::Closed);
    assert_eq!(*closed.borrow(), 1, "on_document_closed exactly once");
    assert_eq!(*n.borrow(), vec![Notification::DocumentClosed]);
    // every DOM call on the document or its handles
    assert_eq!(
        d.get_attribute(a, "id").unwrap_err().code,
        Code::DocumentClosed
    );
    assert_eq!(
        d.set_attribute(a, "x", "1").unwrap_err().code,
        Code::DocumentClosed
    );
    assert_eq!(
        d.create_element("p").unwrap_err().code,
        Code::DocumentClosed
    );
    assert_eq!(d.dom_revision().unwrap_err().code, Code::DocumentClosed);
    assert_eq!(d.batch_apply(&[]).unwrap_err().code, Code::DocumentClosed);
    assert_eq!(
        d.serialize_document().unwrap_err().code,
        Code::DocumentClosed
    );
    // releases succeed, repeatedly
    d.release(a).unwrap();
    d.release(a).unwrap();
    // document-status operations succeed; close is idempotent and fires nothing twice
    assert_eq!(d.lifecycle(), Lifecycle::Closed);
    d.request_close(false);
    d.request_close(true);
    assert_eq!(*closed.borrow(), 1);
    assert_eq!(n.borrow().len(), 1);
    // the memory is freed when HostRuntime drops the closed document
    assert!(d.arena().live_count() > 0);
    d.free_memory();
    assert_eq!(d.arena().live_count(), 0);
    assert_eq!(
        d.get_attribute(a, "id").unwrap_err().code,
        Code::DocumentClosed,
        "a freed node of a closed document is still DOCUMENT_CLOSED"
    );
    d.release(a).unwrap();
}

// table-a: A-14
#[test]
fn close_during_a_dispatch_is_closing_until_the_outermost_commit() {
    let mut d = Document::parse_html("<p id=a>x</p><p id=b></p>").unwrap();
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    let closed: Rc<RefCell<u32>> = Rc::default();
    let c = closed.clone();
    d.hooks_mut()
        .set_on_document_closed(Box::new(move |_| *c.borrow_mut() += 1));
    d.request_close(true);
    assert_eq!(d.lifecycle(), Lifecycle::Closing);
    assert_eq!(*closed.borrow(), 0);
    // from the moment Closing begins every DOM call is DOCUMENT_CLOSED...
    assert_eq!(
        d.get_attribute(a, "id").unwrap_err().code,
        Code::DocumentClosed
    );
    assert_eq!(d.append_child(a, b).unwrap_err().code, Code::DocumentClosed);
    // ...releases and status operations stay OK
    d.release(a).unwrap();
    assert_eq!(d.lifecycle(), Lifecycle::Closing);
    d.request_close(true);
    d.outermost_commit();
    assert_eq!(d.lifecycle(), Lifecycle::Closed);
    assert_eq!(*closed.borrow(), 1);
    d.outermost_commit();
    assert_eq!(*closed.borrow(), 1);
    assert_eq!(
        d.lifecycle_history(),
        &[
            Lifecycle::Created,
            Lifecycle::Parsing,
            Lifecycle::Live,
            Lifecycle::Closing,
            Lifecycle::Closed
        ]
    );
}

// table-a: A-14
#[test]
fn closing_one_document_never_touches_another() {
    let mut d1 = Document::parse_html("<p id=a>x</p>").unwrap();
    let mut d2 = Document::parse_html("<p id=a>y</p>").unwrap();
    let a2 = id(&d2, "a");
    d1.request_close(false);
    assert_eq!(d2.get_attribute(a2, "id").unwrap(), Some("a"));
    d2.set_attribute(a2, "x", "1").unwrap();
    assert_eq!(d2.lifecycle(), Lifecycle::Live);
}

// table-a: A-14
#[test]
fn a_foreign_handle_in_a_closed_document_is_wrong_document_before_closed() {
    let mut d1 = Document::parse_html("<p id=a>x</p>").unwrap();
    let d2 = Document::parse_html("<p id=a>y</p>").unwrap();
    let k1 = d1.key(id(&d1, "a"));
    let k2 = d2.key(id(&d2, "a"));
    d1.request_close(false);
    assert_eq!(
        d1.append_child_key(k1, k2).unwrap_err().code,
        Code::WrongDocument
    );
    assert_eq!(
        d1.append_child_key(k1, k1).unwrap_err().code,
        Code::DocumentClosed
    );
}

#[test]
fn detached_trees_are_collected_at_a_checkpoint_and_report_freed_ids() {
    let mut d = Document::parse_html("<div id=a></div>").unwrap();
    let freed: Rc<RefCell<Vec<usize>>> = Rc::default();
    let f = freed.clone();
    d.hooks_mut()
        .set_on_nodes_freed(Box::new(move |_, ids| f.borrow_mut().push(ids.len())));
    let a = id(&d, "a");
    let x = d.create_element("x").unwrap();
    let y = d.create_text_node("t").unwrap();
    d.append_child(x, y).unwrap();
    d.retain(y).unwrap();
    assert!(
        d.checkpoint().is_empty(),
        "a reference anywhere in the tree keeps it"
    );
    d.release(y).unwrap();
    assert_eq!(d.checkpoint().len(), 2);
    assert_eq!(*freed.borrow(), vec![2]);
    assert_eq!(d.get_attribute(x, "a").unwrap_err().code, Code::StaleHandle);
    assert_eq!(
        d.get_attribute(a, "id").unwrap(),
        Some("a"),
        "connected nodes are never collected"
    );
    d.release(x).unwrap();
    d.release(x).unwrap();
}

#[test]
fn dispatch_frame_references_keep_a_removed_node_alive() {
    let mut d = Document::parse_html("<div id=a><p id=b></p></div>").unwrap();
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    d.frame_retain(b).unwrap();
    d.remove_child(a, b).unwrap();
    assert!(d.checkpoint().is_empty());
    assert!(
        d.get_attribute(b, "id").unwrap().is_some(),
        "still readable during the dispatch"
    );
    d.frame_release(b).unwrap();
    assert_eq!(d.checkpoint().len(), 1);
    assert_eq!(
        d.get_attribute(b, "id").unwrap_err().code,
        Code::StaleHandle
    );
    d.frame_release(b).unwrap();
}

// table-a: A-16
#[test]
fn a_deeply_nested_parse_serialize_clone_and_drop_do_not_overflow_the_stack() {
    // 10,000 nested spans through the parser (no recursion in tree construction, serialization,
    // equality, cloning or dropping).
    let depth = 10_000;
    let html = format!("{}x{}", "<span>".repeat(depth), "</span>".repeat(depth));
    let mut d = Document::parse_html(&html).unwrap();
    let body = d.body().unwrap();
    assert_eq!(d.text_content(body).unwrap().as_deref(), Some("x"));
    let out = d.serialize_document().unwrap();
    assert_eq!(out.matches("<span>").count(), depth);
    let first = d.first_child(body).unwrap().unwrap();
    let c = d.clone_node(first, true).unwrap();
    assert!(d.is_equal_node(first, Some(c)).unwrap());
    d.set_inner_html(body, "").unwrap();
    d.checkpoint();
    drop(d); // dropping the arena is iterative too
}

// table-a: A-27
#[test]
fn deep_trees_do_not_overflow_the_stack_in_dom_algorithms() {
    let mut d = Document::parse_html("<div id=root></div>").unwrap();
    let root = id(&d, "root");
    let mut cur = root;
    // 4,000 nested divs through the public API (each insertion has an O(depth) cycle check, so
    // keep the depth moderate here; the million-deep traversal test is in tree.rs).
    for _ in 0..4_000 {
        let n = d.create_element("div").unwrap();
        d.append_child(cur, n).unwrap();
        cur = n;
    }
    let text = d.create_text_node("leaf").unwrap();
    d.append_child(cur, text).unwrap();
    assert_eq!(d.text_content(root).unwrap().as_deref(), Some("leaf"));
    let html = d.inner_html(root).unwrap();
    assert!(html.contains("leaf") && html.matches("</div>").count() == 4_000);
    let c = d.clone_node(root, true).unwrap();
    assert!(d.is_equal_node(root, Some(c)).unwrap());
    d.set_inner_html(root, "").unwrap();
    assert!(d.children(root).unwrap().is_empty());
    assert!(
        d.checkpoint().len() >= 4_000,
        "the detached chain (and its clone) are collected iteratively"
    );
}
