//! owner: 003-document-model
//!
//! T075 / U4T-144 (FR-071-FR-075, SC-004 batch part): decode errors with `byte_offset`, size limits,
//! round-trip equality with sequential single calls, and atomic rollback for a failure injected at
//! every command index.

use proptest::prelude::*;
use ui4me_document::batch::decode::{Cmd, Encoder, Operand};
use ui4me_document::batch::exec::{document_word, node_word};
use ui4me_document::document::Document;
use ui4me_document::limits::DomLimits;
use ui4me_engine_types::codes_generated::Code;

fn base() -> Document {
    Document::parse_html("<div id=a class=x><p id=b>t</p><input id=i type=checkbox></div><select id=s><option>1</select>").unwrap()
}

fn handle(d: &Document, id: &str) -> Operand {
    Operand::Existing(
        document_word(d.id()),
        node_word(d.element_by_id(id).unwrap()),
    )
}

fn encode(cmds: &[Cmd]) -> Vec<u8> {
    let mut e = Encoder::new();
    for c in cmds {
        e.push(c);
    }
    e.finish()
}

/// A valid batch touching tree, attributes, classes, text and inner HTML.
fn good_batch(d: &Document) -> Vec<Cmd> {
    let a = handle(d, "a");
    let b = handle(d, "b");
    vec![
        Cmd::CreateElement {
            ns: None,
            qname: "section".into(),
        }, // local 0
        Cmd::CreateText {
            data: "hello".into(),
        }, // local 1
        Cmd::AppendChild {
            parent: Operand::Local(0),
            child: Operand::Local(1),
        },
        Cmd::SetAttribute {
            el: Operand::Local(0),
            ns: None,
            qname: "data-k".into(),
            value: "v".into(),
        },
        Cmd::ClassAdd {
            el: Operand::Local(0),
            token: "c1".into(),
        },
        Cmd::AppendChild {
            parent: a,
            child: Operand::Local(0),
        },
        Cmd::ClassToggle {
            el: a,
            token: "x".into(),
            force: Some(false),
        },
        Cmd::SetTextContent {
            node: b,
            text: Some("replaced".into()),
        },
        Cmd::Clone {
            node: Operand::Local(0),
            deep: true,
        }, // local 2
        Cmd::InsertBefore {
            parent: a,
            child: Operand::Local(2),
            reference: b,
        },
        Cmd::SetInnerHtml {
            el: Operand::Local(2),
            html: "<b>x</b><i>y</i>".into(),
        },
        Cmd::ToggleAttribute {
            el: b,
            qname: "hidden".into(),
            force: None,
        },
        Cmd::RemoveAttribute {
            el: a,
            ns: None,
            name: "class".into(),
        },
    ]
}

/// The same operations as single calls.
fn apply_sequentially(d: &mut Document) {
    let a = d.element_by_id("a").unwrap();
    let b = d.element_by_id("b").unwrap();
    let section = d.create_element("section").unwrap();
    let text = d.create_text_node("hello").unwrap();
    d.append_child(section, text).unwrap();
    d.set_attribute(section, "data-k", "v").unwrap();
    d.class_list_add(section, &["c1"]).unwrap();
    d.append_child(a, section).unwrap();
    d.class_list_toggle(a, "x", Some(false)).unwrap();
    d.set_text_content(b, "replaced").unwrap();
    let clone = d.clone_node(section, true).unwrap();
    d.insert_before(a, clone, Some(b)).unwrap();
    d.set_inner_html(clone, "<b>x</b><i>y</i>").unwrap();
    d.toggle_attribute(b, "hidden", None).unwrap();
    d.remove_attribute(a, "class").unwrap();
}

// table-a: A-62
#[test]
fn a_batch_equals_the_same_single_calls_with_one_revision_step() {
    let mut d1 = base();
    let mut d2 = base();
    let cmds = good_batch(&d1);
    let rev = d1.revision();
    let locals = d1.batch_apply(&encode(&cmds)).unwrap();
    assert_eq!(
        locals.len(),
        3,
        "one handle per batch-local id, in id order"
    );
    assert_eq!(d1.revision(), rev + 1, "exactly one revision step");
    assert_eq!(
        d1.arena().node(locals[0]).host_refs(),
        1,
        "each returned handle carries one host reference"
    );
    apply_sequentially(&mut d2);
    assert_eq!(
        d1.serialize_document().unwrap(),
        d2.serialize_document().unwrap()
    );
    assert!(
        d1.revision() < d2.revision(),
        "sequential calls step the revision more often"
    );
}

// table-a: A-62
#[test]
fn decode_errors_report_a_byte_offset_and_change_nothing() {
    let mut d = base();
    let rev = d.revision();
    let before = d.serialize_document().unwrap();
    let mut b = encode(&good_batch(&d));
    b[0] = b'Z';
    let e = d.batch_apply(&b).unwrap_err();
    assert_eq!(e.code, Code::InvalidArgument);
    assert_eq!(e.byte_offset, Some(0));
    let e = d.batch_apply(&encode(&good_batch(&d))[..40]).unwrap_err();
    assert_eq!(e.code, Code::InvalidArgument);
    assert!(e.byte_offset.is_some());
    assert_eq!(
        (d.revision(), d.serialize_document().unwrap()),
        (rev, before)
    );
}

// table-a: A-62
#[test]
fn size_limits_are_size_limit() {
    let d = base();
    let mut d = d.with_limits(DomLimits {
        batch_commands: 3,
        ..Default::default()
    });
    let e = d.batch_apply(&encode(&good_batch(&d))).unwrap_err();
    assert_eq!(e.code, Code::SizeLimit);
    assert!(e.message.contains("dom.batch_commands"));
}

// table-a: A-62
#[test]
fn closed_documents_refuse_batches_before_decoding() {
    let mut d = base();
    let b = encode(&good_batch(&d));
    d.request_close(false);
    assert_eq!(d.batch_apply(&b).unwrap_err().code, Code::DocumentClosed);
    assert_eq!(
        d.batch_apply(&[1, 2, 3]).unwrap_err().code,
        Code::DocumentClosed,
        "no decode, so a garbage buffer is still DOCUMENT_CLOSED"
    );
}

// table-a: A-62
#[test]
fn handle_errors_follow_the_fixed_precedence_with_the_failing_index() {
    let mut d = base();
    let a = handle(&d, "a");
    let other = base();
    let foreign = Operand::Existing(
        document_word(other.id()),
        node_word(other.element_by_id("a").unwrap()),
    );
    let t = d.create_text_node("x").unwrap();
    let stale = Operand::Existing(document_word(d.id()), node_word(t));
    d.collect();
    let rev = d.revision();
    let cmds = vec![
        Cmd::SetAttribute {
            el: a,
            ns: None,
            qname: "ok".into(),
            value: "1".into(),
        },
        Cmd::AppendChild {
            parent: foreign,
            child: stale,
        },
    ];
    let e = d.batch_apply(&encode(&cmds)).unwrap_err();
    assert_eq!(
        (e.code, e.batch_index),
        (Code::StaleHandle, Some(1)),
        "stale outranks wrong document"
    );
    let cmds = vec![Cmd::AppendChild {
        parent: a,
        child: foreign,
    }];
    let e = d.batch_apply(&encode(&cmds)).unwrap_err();
    assert_eq!((e.code, e.batch_index), (Code::WrongDocument, Some(0)));
    assert_eq!(d.revision(), rev);
    assert_eq!(
        d.get_attribute(d.element_by_id("a").unwrap(), "ok")
            .unwrap(),
        None,
        "the earlier command was rolled back"
    );
}

/// Failing commands for the "failure at index k" property (each is rejected by the DOM, not decode).
fn failing(d: &Document, kind: u8) -> Cmd {
    let a = handle(d, "a");
    match kind % 5 {
        0 => Cmd::SetAttribute {
            el: a,
            ns: None,
            qname: "bad name".into(),
            value: "v".into(),
        },
        1 => Cmd::AppendChild {
            parent: a,
            child: a,
        },
        2 => Cmd::ClassAdd {
            el: a,
            token: "has space".into(),
        },
        3 => Cmd::InsertBefore {
            parent: a,
            child: handle(d, "b"),
            reference: handle(d, "s"),
        },
        _ => Cmd::SetData {
            node: a,
            data: "x".into(),
        },
    }
}

fn snapshot(d: &Document) -> (String, u64, u64, usize, usize, u64) {
    (
        d.serialize_document().unwrap(),
        d.revision(),
        d.stylesheet_revision(),
        d.journal().since(0).entries.len(),
        d.arena().live_count(),
        d.journal().epoch(),
    )
}

// table-a: A-62
#[test]
fn a_failure_at_every_index_rolls_everything_back() {
    let n = good_batch(&base()).len();
    for k in 0..=n {
        for kind in 0..5u8 {
            let mut d = base();
            let mut cmds = good_batch(&d);
            cmds.insert(k, failing(&d, kind));
            // earlier local ids shift by nothing: the failing commands define none
            let before = snapshot(&d);
            let e = d.batch_apply(&encode(&cmds)).unwrap_err();
            assert_eq!(e.batch_index, Some(k as u32), "k={k} kind={kind}: {e:?}");
            assert_eq!(
                snapshot(&d),
                before,
                "k={k} kind={kind}: nothing may change"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    // table-a: A-62
    #[test]
    fn random_prefixes_then_a_failure_leave_serialization_journal_and_revisions_unchanged(
        take in 0usize..13, kind in 0u8..5, retry in proptest::bool::ANY
    ) {
        let mut d = base();
        let mut cmds: Vec<Cmd> = good_batch(&d).into_iter().take(take).collect();
        cmds.push(failing(&d, kind));
        let before = snapshot(&d);
        let e = d.batch_apply(&encode(&cmds)).unwrap_err();
        prop_assert_eq!(e.batch_index, Some(take as u32));
        prop_assert_eq!(snapshot(&d), before.clone());
        if retry {
            // after the rollback the same document accepts the good batch
            let cmds = good_batch(&d);
            prop_assert!(d.batch_apply(&encode(&cmds)).is_ok());
        }
    }
}
