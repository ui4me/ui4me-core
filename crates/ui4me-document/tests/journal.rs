//! owner: 003-document-model
//!
//! T096 (FR-083-FR-086, US9, SC-004 journal part): one prior record per element per epoch, failed
//! calls and batches leave no entries, `take_epoch()` changes nothing observable, the window and its
//! truncation, redaction tags for password values, node references for detached nodes.

use ui4me_document::batch::decode::{Cmd, Encoder, Operand};
use ui4me_document::batch::exec::{document_word, node_word};
use ui4me_document::diag::{EntryDetail, NodeRef};
use ui4me_document::document::Document;
use ui4me_document::journal::{EntryKind, WINDOW};

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap()
}

// table-a: A-63
#[test]
fn one_prior_attributes_record_with_the_original_list_and_a_change_entry_each() {
    let mut d = Document::parse_html("<p id=a class=c1 title=t0 data-z=z></p>").unwrap();
    let a = id(&d, "a");
    let _ = d.take_epoch();
    d.set_attribute(a, "class", "c2").unwrap();
    d.set_attribute(a, "class", "c3").unwrap();
    d.set_attribute(a, "title", "t1").unwrap();
    let seg = d.take_epoch();
    let priors: Vec<_> = seg
        .entries()
        .iter()
        .filter_map(|e| match &e.kind {
            EntryKind::PriorAttributes { attributes, .. } => Some(attributes.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(priors.len(), 1);
    let original: Vec<_> = priors[0]
        .iter()
        .map(|x| (x.name.local.to_string(), x.value.clone()))
        .collect();
    assert_eq!(
        original,
        [
            ("id".into(), "a".into()),
            ("class".into(), "c1".into()),
            ("title".into(), "t0".into()),
            ("data-z".into(), "z".into())
        ]
    );
    assert_eq!(
        seg.entries()
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::Attribute { .. }))
            .count(),
        3
    );
    // the next epoch records a fresh prior list
    d.set_attribute(a, "class", "c4").unwrap();
    let seg2 = d.take_epoch();
    let p2 = seg2
        .entries()
        .iter()
        .find_map(|e| match &e.kind {
            EntryKind::PriorAttributes { attributes, .. } => Some(attributes.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        p2.iter().find(|x| &*x.name.local == "class").unwrap().value,
        "c3"
    );
}

// table-a: A-63
#[test]
fn failed_calls_and_batches_leave_no_entries_and_all_entries_of_a_call_share_its_revision() {
    let mut d = Document::parse_html("<div id=a><p id=b></p></div>").unwrap();
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    let before = d.journal().since(0).entries.len();
    assert!(d.append_child(b, a).is_err());
    assert!(d.set_attribute(a, "bad name", "v").is_err());
    let mut e = Encoder::new();
    e.push(&Cmd::SetAttribute {
        el: Operand::Existing(document_word(d.id()), node_word(a)),
        ns: None,
        qname: "ok".into(),
        value: "1".into(),
    });
    e.push(&Cmd::SetAttribute {
        el: Operand::Existing(document_word(d.id()), node_word(a)),
        ns: None,
        qname: "x y".into(),
        value: "1".into(),
    });
    assert!(d.batch_apply(&e.finish()).is_err());
    assert_eq!(d.journal().since(0).entries.len(), before);
    // a multi-step call: one revision on every entry
    d.replace_children(a, &[ui4me_document::mutate::NodeOrString::Text("t".into())])
        .unwrap();
    let w = d.journal().since(0);
    let new: Vec<_> = w.entries.iter().skip(before).collect();
    assert!(new.len() >= 2 && new.iter().all(|e| e.revision == new[0].revision));
}

// table-a: A-63
#[test]
fn take_epoch_changes_nothing_observable() {
    let mut d = Document::parse_html("<div id=a class=x></div>").unwrap();
    let a = id(&d, "a");
    d.set_attribute(a, "class", "y").unwrap();
    let (rev, html, journal_len, sheet_rev, epoch) = (
        d.revision(),
        d.serialize_document().unwrap(),
        d.journal().since(0).entries.len(),
        d.stylesheet_revision(),
        d.journal().epoch(),
    );
    let seg = d.take_epoch();
    assert!(!seg.is_empty());
    assert_eq!(
        (
            d.revision(),
            d.serialize_document().unwrap(),
            d.journal().since(0).entries.len(),
            d.stylesheet_revision()
        ),
        (rev, html, journal_len, sheet_rev)
    );
    assert_eq!(
        d.journal().epoch(),
        epoch + 1,
        "only the epoch counter advances"
    );
    assert!(d.take_epoch().is_empty());
}

// table-a: A-63
#[test]
fn the_window_keeps_at_least_4096_entries_and_reports_truncation() {
    let mut d = Document::parse_html("<div id=a></div>").unwrap();
    let a = id(&d, "a");
    for i in 0..(WINDOW + 500) {
        d.set_attribute(a, "data-i", &i.to_string()).unwrap();
    }
    let all = d.journal_payload(0).unwrap();
    assert!(
        all.truncated,
        "a request older than the window is flagged, not refused"
    );
    assert_eq!(all.entries.len(), WINDOW);
    assert!(all.oldest_revision > 1);
    let recent = d.journal_payload(d.revision() - 10).unwrap();
    assert!(!recent.truncated);
    assert_eq!(recent.entries.len(), 10);
}

// table-a: A-63
#[test]
fn password_values_are_tagged_for_redaction_and_detached_nodes_have_detached_refs() {
    let mut d =
        Document::parse_html("<input id=p type=password value=secret><input id=q value=public>")
            .unwrap();
    let (p, q) = (id(&d, "p"), id(&d, "q"));
    let _ = d.take_epoch();
    d.set_attribute(p, "value", "s2").unwrap();
    d.set_attribute(q, "value", "p2").unwrap();
    let payload = d.journal_payload(0).unwrap();
    let mut tags = vec![];
    for e in &payload.entries {
        if let EntryDetail::PriorAttributes { attributes } = &e.detail {
            tags.extend(
                attributes
                    .iter()
                    .filter(|a| a.local_name == "value")
                    .map(|a| (a.value.clone(), a.redact)),
            );
        }
    }
    assert!(tags.contains(&("secret".to_owned(), true)));
    assert!(tags.contains(&("public".to_owned(), false)));
    // detached node references
    let x = d.create_element("x").unwrap();
    let y = d.create_element("y").unwrap();
    d.append_child(x, y).unwrap();
    let payload = d.journal_payload(0).unwrap();
    let last = payload.entries.last().unwrap();
    assert_eq!(last.kind, "child_list");
    assert_eq!(
        last.node,
        Some(NodeRef::Detached {
            root: x,
            path: vec![]
        })
    );
    d.append_child(d.body().unwrap(), x).unwrap();
    assert!(matches!(d.node_ref(y), NodeRef::Connected { .. }));
}

// table-a: A-63
#[test]
fn child_list_entries_carry_parent_and_siblings_as_they_were() {
    let mut d =
        Document::parse_html("<ul id=u><li id=a></li><li id=b></li><li id=c></li></ul>").unwrap();
    let (u, a, b, c) = (id(&d, "u"), id(&d, "a"), id(&d, "b"), id(&d, "c"));
    let _ = d.take_epoch();
    d.remove_child(u, b).unwrap();
    let seg = d.take_epoch();
    let EntryKind::ChildList {
        parent,
        added,
        removed,
        prev_sibling,
        next_sibling,
    } = &seg.entries()[0].kind
    else {
        panic!()
    };
    assert_eq!(
        (
            *parent,
            added.is_empty(),
            removed.as_slice(),
            *prev_sibling,
            *next_sibling
        ),
        (u, true, &[b][..], Some(a), Some(c))
    );
}
