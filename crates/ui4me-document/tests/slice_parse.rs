// owner: 003-document-model — created by 001 slice (R-28)
//
// 001 T030 (red first): T1 tests for DocumentCore on the slice (U4C-DOC; 001 FR-009, FR-014;
// 003 contracts dom-api-surface.md, dom-internal-interfaces.md, mutation-journal.md).
//
// Name prefixes select the task evidence filters:
// - `parse_*`     T036 (parser sink, slice-demo);
// - `handles_*`, `lifecycle_*`  T035 (arena, handles, lifecycle);
// - `mutate_*`, `journal_*`, `hooks_*`, `state_*`  T037;
// - `query_*`     T037/T043: DocumentCore's part of DOM queries (scope, order, errors). Selector
//   *matching* is 006's service (ui4me-style/src/query.rs, R-12); this crate cannot link
//   ui4me-style in its tests without a dev-dependency (Cargo.toml is coordinator-owned), so the
//   `querySelectorAll` cases that run through the service live in
//   ui4me-style/tests/slice_cascade.rs (`query_*`).

use ui4me_document::control_state::ControlField;
use ui4me_document::document::{Document, Lifecycle};
use ui4me_document::journal::EntryKind;
use ui4me_document::node::NodeKind;
use ui4me_document::ui_state::UiFlags;
use ui4me_engine_types::codes_generated::{Code, SubReason};
use ui4me_engine_types::ids::{DocumentId, NodeKey, Slots};

/// The slice fixture (verification/fixtures/slice/slice-demo/index.html), read-only.
fn slice_demo() -> String {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/slice-demo/index.html"
    );
    std::fs::read_to_string(p).expect("slice-demo fixture")
}

fn load() -> Document {
    Document::parse_html(&slice_demo()).expect("slice demo parses")
}

fn by_id(d: &Document, id: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(id).unwrap_or_else(|| panic!("#{id}"))
}

fn names(d: &Document, nodes: &[ui4me_engine_types::ids::NodeId]) -> Vec<String> {
    nodes
        .iter()
        .map(|&n| match d.node_kind(n).unwrap() {
            NodeKind::Element => d.local_name(n).unwrap().to_owned(),
            NodeKind::Text => format!("#text {:?}", d.text_data(n).unwrap()),
            NodeKind::Comment => "#comment".into(),
            NodeKind::Doctype => "#doctype".into(),
            k => format!("{k:?}"),
        })
        .collect()
}

// ------------------------------------------------------------------ parse (T036)

#[test]
fn parse_implied_html_head_body() {
    let d = Document::parse_html("<p>hi").unwrap();
    let html = d.document_element().expect("implied html");
    assert_eq!(d.local_name(html).unwrap(), "html");
    assert_eq!(names(&d, d.children(html).unwrap()), ["head", "body"]);
    let body = d.body().unwrap();
    assert_eq!(d.parent(body).unwrap(), Some(html));
    assert_eq!(names(&d, d.children(body).unwrap()), ["p"]);
    assert_eq!(d.text_content(body).unwrap().as_deref(), Some("hi"));
    assert_eq!(d.head(), Some(d.children(html).unwrap()[0]));
}

#[test]
fn parse_slice_demo_tree_with_whitespace_text_nodes() {
    let d = load();
    let doc = d.document_node();
    assert_eq!(names(&d, d.children(doc).unwrap()), ["#doctype", "html"]);
    let html = d.document_element().unwrap();
    // `</head>\n<body>`: the newline after </head> is inserted in html (in "after head" mode).
    assert_eq!(
        names(&d, d.children(html).unwrap()),
        ["head", "#text \"\\n\"", "body"]
    );
    let card = by_id(&d, "card");
    assert_eq!(
        names(&d, d.children(card).unwrap()),
        [
            "#text \"\\n\"",
            "h1",
            "#text \"\\n\"",
            "p",
            "#text \"\\n\"",
            "button",
            "#text \"\\n\"",
            "p",
            "#text \"\\n\""
        ]
    );
    // `</div>\n</body>\n</html>\n`: whitespace after </body> goes into body, after </html> too.
    let body = d.body().unwrap();
    let body_kids = names(&d, d.children(body).unwrap());
    assert_eq!(body_kids.first().map(String::as_str), Some("#text \"\\n\""));
    assert!(body_kids.contains(&"div".to_owned()));
    assert_eq!(
        d.text_content(by_id(&d, "count")).unwrap().as_deref(),
        Some("Clicks: 0")
    );
    let style = d
        .children(d.head().unwrap())
        .unwrap()
        .iter()
        .copied()
        .find(|&n| d.local_name(n).ok() == Some("style"));
    assert!(style.is_some(), "<style> in head");
    assert!(
        d.parser_diagnostics().total() == 0,
        "the fixture is conforming: {:?}",
        d.parser_diagnostics().records()
    );
}

#[test]
fn parse_attribute_order_is_source_order() {
    let d = load();
    let card = by_id(&d, "card");
    let attrs: Vec<(&str, &str)> = d
        .attributes(card)
        .unwrap()
        .iter()
        .map(|a| (&*a.name.local, a.value.as_str()))
        .collect();
    assert_eq!(
        attrs,
        [("id", "card"), ("data-probe", "card"), ("class", "card")]
    );
    let inc = by_id(&d, "inc");
    let attrs: Vec<&str> = d
        .attributes(inc)
        .unwrap()
        .iter()
        .map(|a| &*a.name.local)
        .collect();
    assert_eq!(attrs, ["id", "data-probe", "class", "type"]);
}

#[test]
fn parse_get_element_by_id() {
    let d = load();
    let n = by_id(&d, "n");
    assert_eq!(d.local_name(n).unwrap(), "span");
    assert_eq!(d.text_content(n).unwrap().as_deref(), Some("0"));
    assert_eq!(d.element_by_id(""), None, "empty string → null");
    assert_eq!(d.element_by_id("nope"), None);
    // First in tree order wins.
    let d2 = Document::parse_html("<div id=a class=first></div><p id=a></p>").unwrap();
    let a = d2.element_by_id("a").unwrap();
    assert_eq!(d2.local_name(a).unwrap(), "div");
}

#[test]
fn parse_records_parser_diagnostics() {
    let d = Document::parse_html("<!DOCTYPE html><p>a</div></span>").unwrap();
    let diag = d.parser_diagnostics();
    assert!(diag.total() >= 1, "stray end tags are parse errors");
    assert!(diag.records().len() as u64 <= diag.total());
    assert!(
        diag.records()
            .iter()
            .all(|r| r.line >= 1 && !r.message.is_empty())
    );
}

#[test]
fn parse_template_contents_are_not_children() {
    let d = Document::parse_html("<template id=t><b>x</b></template>").unwrap();
    let t = d.element_by_id("t").unwrap();
    assert!(
        d.children(t).unwrap().is_empty(),
        "template contents are never children"
    );
    let frag = d.template_contents(t).unwrap().expect("contents fragment");
    assert_eq!(d.node_kind(frag).unwrap(), NodeKind::DocumentFragment);
    assert_eq!(names(&d, d.children(frag).unwrap()), ["b"]);
}

#[test]
fn parse_quirks_mode_without_doctype() {
    assert_eq!(
        Document::parse_html("<p>x").unwrap().mode(),
        ui4me_document::document::DocumentMode::Quirks
    );
    assert_eq!(
        load().mode(),
        ui4me_document::document::DocumentMode::NoQuirks
    );
}

// ------------------------------------------------------------------ handles (T035)

#[test]
fn handles_stale_after_free() {
    let mut d = load();
    let p = d.create_element("p").unwrap();
    assert_eq!(d.local_name(p).unwrap(), "p");
    // Detached, never referenced: freed at the next collection checkpoint.
    let freed = d.collect();
    assert!(freed.contains(&p));
    assert_eq!(d.local_name(p).unwrap_err().code, Code::StaleHandle);
    assert_eq!(d.node_kind(p).unwrap_err().code, Code::StaleHandle);
    assert_eq!(
        d.append_child(d.body().unwrap(), p).unwrap_err().code,
        Code::StaleHandle
    );
    // The slot is reused with a new generation: the old id stays stale.
    let q = d.create_element("q").unwrap();
    assert_eq!(q.raw().index, p.raw().index, "slot reuse");
    assert_ne!(q.raw().generation, p.raw().generation);
    assert_eq!(d.local_name(p).unwrap_err().code, Code::StaleHandle);
    assert_eq!(d.local_name(q).unwrap(), "q");
}

#[test]
fn handles_wrong_document_for_a_foreign_node() {
    let mut ids = Slots::new();
    let (ida, idb) = (
        DocumentId::from_raw(ids.alloc()),
        DocumentId::from_raw(ids.alloc()),
    );
    let mut a = Document::parse(ida, "<p id=x>a</p>").unwrap();
    let b = Document::parse(idb, "<p id=y>b</p>").unwrap();
    let foreign = b.key(b.element_by_id("y").unwrap());
    assert_eq!(a.resolve(foreign).unwrap_err().code, Code::WrongDocument);
    let own = a.key(a.element_by_id("x").unwrap());
    assert_eq!(a.resolve(own).unwrap(), a.element_by_id("x").unwrap());
    let body = a.key(a.body().unwrap());
    assert_eq!(
        a.append_child_key(body, foreign).unwrap_err().code,
        Code::WrongDocument
    );
    // A stale own key is STALE_HANDLE, which precedes WRONG_DOCUMENT in the order (FR-021).
    let tmp = a.create_element("i").unwrap();
    let tmp_key = a.key(tmp);
    a.collect();
    assert_eq!(a.resolve(tmp_key).unwrap_err().code, Code::StaleHandle);
    assert_eq!(
        a.append_child_key(tmp_key, foreign).unwrap_err().code,
        Code::StaleHandle
    );
    let _ = NodeKey {
        document: ida,
        node: tmp,
    };
}

#[test]
fn handles_detached_node_readable_while_referenced() {
    let mut d = load();
    let card = by_id(&d, "card");
    let msg = by_id(&d, "msg");
    d.retain(msg).unwrap();
    d.remove_child(card, msg).unwrap();
    assert!(!d.is_connected(msg).unwrap());
    let freed = d.collect();
    assert!(!freed.contains(&msg), "referenced: kept");
    assert!(
        d.text_content(msg)
            .unwrap()
            .unwrap()
            .starts_with("The quick brown fox")
    );
    assert_eq!(
        d.element_by_id("msg"),
        None,
        "detached nodes are not found by id"
    );
    // Release is idempotent; once unreferenced the detached tree is freed.
    d.release(msg).unwrap();
    d.release(msg).unwrap();
    let freed = d.collect();
    assert!(freed.contains(&msg));
    assert_eq!(d.text_content(msg).unwrap_err().code, Code::StaleHandle);
    d.release(msg)
        .expect("release of a stale handle is a no-op success (R-34)");
    let counts = d.counts();
    assert!(
        counts.freed_total >= 2,
        "the element and its text child: {counts:?}"
    );
    assert_eq!(counts.detached, 0);
}

#[test]
fn handles_connected_nodes_are_never_collected() {
    let mut d = load();
    let before = d.counts();
    assert!(d.collect().is_empty());
    assert_eq!(d.counts().connected, before.connected);
    assert!(before.connected > 20);
}

#[test]
fn handles_tree_invariant_rejects_cycles() {
    let mut d = load();
    let card = by_id(&d, "card");
    let title = by_id(&d, "title");
    let e = d.append_child(title, card).unwrap_err();
    assert_eq!(
        e.code,
        Code::InvalidHierarchy,
        "an ancestor cannot become a child"
    );
    assert_eq!(
        d.append_child(card, card).unwrap_err().code,
        Code::InvalidHierarchy
    );
    let t = d.create_text_node("x").unwrap();
    assert_eq!(
        d.append_child(d.document_node(), t).unwrap_err().code,
        Code::InvalidHierarchy,
        "text under the Document"
    );
    // Moving a node detaches it from its old parent first: one parent, one child list.
    let body = d.body().unwrap();
    d.append_child(body, title).unwrap();
    assert_eq!(d.parent(title).unwrap(), Some(body));
    assert!(!d.children(card).unwrap().contains(&title));
    assert_eq!(d.children(body).unwrap().last(), Some(&title));
}

// ------------------------------------------------------------------ lifecycle (T035)

#[test]
fn lifecycle_created_parsing_live() {
    let d = load();
    assert_eq!(d.lifecycle(), Lifecycle::Live);
    assert_eq!(
        d.lifecycle_history(),
        [Lifecycle::Created, Lifecycle::Parsing, Lifecycle::Live]
    );
    let e = Document::new_empty(d.id());
    assert_eq!(e.lifecycle(), Lifecycle::Live);
    assert!(e.document_element().is_none());
}

#[test]
fn lifecycle_closing_returns_document_closed_and_releases_stay_idempotent() {
    let mut d = load();
    let n = by_id(&d, "n");
    d.retain(n).unwrap();
    assert_eq!(d.dom_revision().unwrap(), 0);
    // Inside a dispatch: Live → Closing until the outermost COMMIT (FR-090).
    d.request_close(true);
    assert_eq!(d.lifecycle(), Lifecycle::Closing);
    assert_eq!(d.dom_revision().unwrap_err().code, Code::DocumentClosed);
    assert_eq!(d.text_content(n).unwrap_err().code, Code::DocumentClosed);
    assert_eq!(
        d.set_text_content(n, "1").unwrap_err().code,
        Code::DocumentClosed
    );
    assert_eq!(
        d.create_element("p").unwrap_err().code,
        Code::DocumentClosed
    );
    d.release(n).expect("release on a Closing document is OK");
    d.release(n).expect("idempotent");
    d.request_close(true);
    assert_eq!(d.lifecycle(), Lifecycle::Closing, "idempotent");
    d.outermost_commit();
    assert_eq!(d.lifecycle(), Lifecycle::Closed);
    assert_eq!(d.lifecycle_history().last(), Some(&Lifecycle::Closed));
    let mut e = load();
    e.request_close(false);
    assert_eq!(
        e.lifecycle(),
        Lifecycle::Closed,
        "no dispatch active: straight to Closed"
    );
}

// ------------------------------------------------------------------ mutations (T037)

#[test]
fn mutate_revision_steps_once_per_accepted_call() {
    let mut d = load();
    let (card, n) = (by_id(&d, "card"), by_id(&d, "n"));
    let r0 = d.dom_revision().unwrap();
    d.set_text_content(n, "1").unwrap();
    assert_eq!(
        d.text_content(n).unwrap().as_deref(),
        Some("1"),
        "immediately visible"
    );
    assert_eq!(d.dom_revision().unwrap(), r0 + 1);
    assert!(d.toggle_class(card, "active").unwrap());
    assert!(d.has_class(card, "active").unwrap());
    assert_eq!(d.get_attribute(card, "class").unwrap(), Some("card active"));
    assert_eq!(d.dom_revision().unwrap(), r0 + 2);
    // `C`: creation steps no revision.
    let log = d.create_element("p").unwrap();
    assert_eq!(d.dom_revision().unwrap(), r0 + 2);
    // Mutations of detached nodes are mutations.
    d.set_attribute(log, "class", "log").unwrap();
    d.set_text_content(log, "Clicked 1").unwrap();
    assert_eq!(d.dom_revision().unwrap(), r0 + 4);
    d.append_child(card, log).unwrap();
    assert_eq!(d.dom_revision().unwrap(), r0 + 5);
    assert_eq!(d.children(card).unwrap().last(), Some(&log));
    assert!(d.is_connected(log).unwrap());
    // `M*`: an unchanged value is no mutation.
    d.set_attribute(log, "class", "log").unwrap();
    assert_eq!(d.dom_revision().unwrap(), r0 + 5);
    // A failed call changes nothing.
    assert!(d.set_attribute(log, "bad name", "x").is_err());
    assert_eq!(d.dom_revision().unwrap(), r0 + 5);
    assert!(!d.toggle_class(card, "active").unwrap());
    assert_eq!(d.get_attribute(card, "class").unwrap(), Some("card"));
    d.remove_child(card, log).unwrap();
    assert_eq!(d.dom_revision().unwrap(), r0 + 7);
    assert_eq!(
        d.remove_child(card, log).unwrap_err().sub_reason,
        Some(SubReason::NotAChild)
    );
}

#[test]
fn mutate_attribute_names_are_validated_and_lowercased() {
    let mut d = load();
    let card = by_id(&d, "card");
    d.set_attribute(card, "Data-X", "1").unwrap();
    assert_eq!(d.get_attribute(card, "data-x").unwrap(), Some("1"));
    assert_eq!(
        d.set_attribute(card, "", "1").unwrap_err().code,
        Code::InvalidCharacter
    );
    assert_eq!(
        d.set_attribute(card, "a b", "1").unwrap_err().code,
        Code::InvalidCharacter
    );
    assert_eq!(
        d.toggle_class(card, "").unwrap_err().sub_reason,
        Some(SubReason::Syntax)
    );
    assert_eq!(
        d.toggle_class(card, "a b").unwrap_err().code,
        Code::InvalidCharacter
    );
    assert_eq!(
        d.create_element("1x").unwrap_err().code,
        Code::InvalidCharacter
    );
    let t = d.create_text_node("t").unwrap();
    assert_eq!(
        d.set_attribute(t, "a", "1").unwrap_err().sub_reason,
        Some(SubReason::WrongNodeKind)
    );
    d.remove_attribute(card, "data-x").unwrap();
    assert_eq!(d.get_attribute(card, "data-x").unwrap(), None);
}

#[test]
fn journal_prior_values_for_the_first_change_per_node_per_epoch() {
    let mut d = load();
    let card = by_id(&d, "card");
    let rev = d.dom_revision().unwrap();
    let _ = d.take_epoch();
    d.toggle_class(card, "active").unwrap();
    d.set_attribute(card, "data-x", "1").unwrap();
    d.toggle_class(card, "active").unwrap();
    let seg = d.take_epoch();
    let priors: Vec<_> = seg
        .entries()
        .iter()
        .filter(|e| matches!(e.kind, EntryKind::PriorAttributes { .. }))
        .collect();
    assert_eq!(
        priors.len(),
        1,
        "prior_attributes only for the first change of the epoch"
    );
    let EntryKind::PriorAttributes { node, attributes } = &priors[0].kind else {
        unreachable!()
    };
    assert_eq!(*node, card);
    let list: Vec<(&str, &str)> = attributes
        .iter()
        .map(|a| (&*a.name.local, a.value.as_str()))
        .collect();
    assert_eq!(
        list,
        [("id", "card"), ("data-probe", "card"), ("class", "card")],
        "the complete list before the change"
    );
    assert_eq!(priors[0].revision, rev + 1);
    let attr_entries = seg
        .entries()
        .iter()
        .filter(|e| matches!(e.kind, EntryKind::Attribute { .. }))
        .count();
    assert_eq!(attr_entries, 3);
    // Per-element summary for StyleBridge: complete prior list + changed names.
    let prior = seg.element_priors().get(&card).cloned().expect("summary");
    assert_eq!(prior.attributes.as_ref().unwrap().len(), 3);
    let changed: Vec<&str> = prior.changed_attributes.iter().map(|n| &*n.local).collect();
    assert_eq!(changed, ["class", "data-x"]);
    // A new epoch emits prior values again.
    d.toggle_class(card, "active").unwrap();
    let seg2 = d.take_epoch();
    assert_eq!(seg2.epoch(), seg.epoch() + 1);
    let EntryKind::PriorAttributes { attributes, .. } = &seg2
        .entries()
        .iter()
        .find(|e| matches!(e.kind, EntryKind::PriorAttributes { .. }))
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    assert!(
        attributes.iter().any(|a| &*a.name.local == "data-x"),
        "prior list reflects the previous epoch's changes"
    );
    // Reading does not journal; the probe window still holds every entry.
    let _ = d.text_content(card).unwrap();
    assert!(d.take_epoch().entries().is_empty());
    let w = d.journal().since(0);
    assert!(!w.truncated);
    assert!(w.entries.len() >= 6);
}

#[test]
fn journal_child_list_records_parent_and_siblings() {
    let mut d = load();
    let card = by_id(&d, "card");
    let count = by_id(&d, "count");
    let _ = d.take_epoch();
    let log = d.create_element("p").unwrap();
    d.append_child(card, log).unwrap();
    let seg = d.take_epoch();
    let cl: Vec<_> = seg
        .entries()
        .iter()
        .filter_map(|e| match &e.kind {
            EntryKind::ChildList {
                parent,
                added,
                removed,
                prev_sibling,
                next_sibling,
            } => Some((
                *parent,
                added.clone(),
                removed.clone(),
                *prev_sibling,
                *next_sibling,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(cl.len(), 1);
    let (parent, added, removed, prev, next) = &cl[0];
    assert_eq!(*parent, card);
    assert_eq!(added, &vec![log]);
    assert!(removed.is_empty());
    let last_text = d.children(card).unwrap()[d.children(card).unwrap().len() - 2];
    assert_eq!(*prev, Some(last_text));
    assert_eq!(*next, None);
    let _ = count;
    // Text replacement: one call, removed old text, added new.
    d.set_text_content(by_id(&d, "n"), "5").unwrap();
    let seg = d.take_epoch();
    let kinds: Vec<_> = seg.entries().iter().map(|e| e.revision).collect();
    assert!(
        kinds.windows(2).all(|w| w[0] == w[1]),
        "all entries of one call share its revision"
    );
    assert!(seg.entries().iter().any(|e| matches!(&e.kind, EntryKind::ChildList { removed, added, .. } if removed.len() == 1 && added.len() == 1)));
}

#[test]
fn journal_window_truncates_but_never_errors() {
    let mut d = load();
    let card = by_id(&d, "card");
    for i in 0..3000 {
        d.set_attribute(card, "data-i", &i.to_string()).unwrap();
        d.set_attribute(card, "data-j", &i.to_string()).unwrap();
    }
    let w = d.journal().since(0);
    assert!(w.truncated);
    assert!(w.entries.len() >= 4096);
    assert!(w.oldest_revision > 1);
    assert!(!d.journal().since(d.dom_revision().unwrap() - 1).truncated);
}

#[test]
fn hooks_removal_hook_runs_inside_the_removing_call() {
    let mut d = load();
    let card = by_id(&d, "card");
    let inc = by_id(&d, "inc");
    let key = d
        .take_ui_state_key()
        .expect("first taker gets the writer key");
    assert!(d.take_ui_state_key().is_none(), "one writer per document");
    d.ui_state_writer(&key)
        .unwrap()
        .set_flags(inc, UiFlags::FOCUS | UiFlags::FOCUS_VISIBLE)
        .unwrap();
    assert!(d.ui_flags(inc).unwrap().contains(UiFlags::FOCUS));
    // 012's focus fix-up (R-14): clear focus flags in the removed subtree, without events.
    let seen = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
    let s = seen.clone();
    d.hooks_mut().set_on_removed(Box::new(move |w, removal| {
        s.borrow_mut().push((removal.root, removal.old_parent));
        let focused: Vec<_> = w.flagged_in_subtree(removal.root, UiFlags::FOCUS);
        for el in focused {
            w.set_flags(el, UiFlags::empty()).unwrap();
        }
    }));
    let rev = d.dom_revision().unwrap();
    assert_eq!(d.parent(inc).unwrap(), Some(card));
    d.remove_child(card, inc).unwrap();
    assert_eq!(seen.borrow().as_slice(), [(inc, card)]);
    assert!(
        d.ui_flags(inc).unwrap().is_empty(),
        "focus cleared before the call returned"
    );
    assert_eq!(
        d.dom_revision().unwrap(),
        rev + 1,
        "hook writes belong to the removing call's revision"
    );
    let seg = d.take_epoch();
    assert!(
        seg.entries()
            .iter()
            .any(|e| matches!(e.kind, EntryKind::UiState { node, .. } if node == inc))
    );
}

#[test]
fn state_ui_flags_and_control_state_are_journaled() {
    let mut d = Document::parse_html("<!DOCTYPE html><input id=c type=checkbox checked><input id=t type=text value=v><p id=p>x</p>").unwrap();
    let (c, t, p) = (
        d.element_by_id("c").unwrap(),
        d.element_by_id("t").unwrap(),
        d.element_by_id("p").unwrap(),
    );
    assert_eq!(
        d.control_state(c).unwrap().unwrap().checkedness,
        Some(true),
        "initial checkedness from `checked`"
    );
    assert_eq!(
        d.control_state(t).unwrap().unwrap().value.as_deref(),
        Some("v")
    );
    assert!(d.control_state(p).unwrap().is_none(), "non-control → null");
    let _ = d.take_epoch();
    let rev = d.dom_revision().unwrap();
    d.control_state_writer().set_checkedness(c, false).unwrap();
    d.control_state_writer().set_checkedness(c, true).unwrap();
    assert_eq!(d.dom_revision().unwrap(), rev + 2);
    let key = d.take_ui_state_key().unwrap();
    d.ui_state_writer(&key)
        .unwrap()
        .set_flags(t, UiFlags::FOCUS)
        .unwrap();
    assert_eq!(d.dom_revision().unwrap(), rev + 3);
    let seg = d.take_epoch();
    let prior_ctrl: Vec<_> = seg
        .entries()
        .iter()
        .filter(|e| matches!(e.kind, EntryKind::PriorControlFlags { .. }))
        .collect();
    assert_eq!(
        prior_ctrl.len(),
        1,
        "first selector-relevant control change only"
    );
    let EntryKind::PriorControlFlags { flags, .. } = &prior_ctrl[0].kind else {
        unreachable!()
    };
    assert_eq!(flags.checked, Some(true));
    assert!(seg.entries().iter().any(|e| matches!(
        e.kind,
        EntryKind::ControlState {
            field: ControlField::Checkedness,
            ..
        }
    )));
    let ui: Vec<_> = seg
        .entries()
        .iter()
        .filter_map(|e| match e.kind {
            EntryKind::UiState { node, old, new } => Some((node, old, new)),
            _ => None,
        })
        .collect();
    assert_eq!(ui, [(t, UiFlags::empty(), UiFlags::FOCUS)]);
    let pri = seg.element_priors();
    assert_eq!(pri[&c].control.unwrap().checked, Some(true));
    assert_eq!(pri[&t].ui, Some(UiFlags::empty()));
    assert_eq!(d.set_text_content(p, "y").map(|_| ()), Ok(()));
    assert_eq!(
        d.control_state_writer()
            .set_checkedness(p, true)
            .unwrap_err()
            .sub_reason,
        Some(SubReason::WrongNodeKind)
    );
}

// ------------------------------------------------------------------ query: DocumentCore's part (T037)

#[test]
fn query_scope_validates_the_root_kind() {
    let mut d = load();
    let t = d.create_text_node("x").unwrap();
    let e = d.scope_for_query(t).unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidArgument, Some(SubReason::WrongNodeKind))
    );
    let scope = d.scope_for_query(d.document_node()).unwrap();
    assert_eq!(scope.root, d.document_node());
    // Candidates are the root's descendant elements in tree order (never the root itself).
    let card = by_id(&d, "card");
    let s = d.scope_for_query(card).unwrap();
    let got: Vec<&str> = d
        .scope_elements(&s)
        .iter()
        .map(|&n| d.local_name(n).unwrap())
        .collect();
    assert_eq!(got, ["h1", "p", "button", "p", "span"]);
}

#[test]
fn state_checkbox_and_radio_value_default_to_on() {
    // HTML: the `value` IDL attribute of a checkbox/radio without a `value` content attribute is "on".
    let d = Document::parse_html("<!DOCTYPE html><input id=c type=checkbox><input id=r type=radio value=x><input id=k type=checkbox value=''><input id=t type=text>").unwrap();
    let v = |id: &str| {
        d.control_state(d.element_by_id(id).unwrap())
            .unwrap()
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(v("c").as_deref(), Some("on"));
    assert_eq!(v("r").as_deref(), Some("x"));
    assert_eq!(
        v("k").as_deref(),
        Some(""),
        "an explicit empty value stays empty"
    );
    assert_eq!(v("t").as_deref(), Some(""));
}
