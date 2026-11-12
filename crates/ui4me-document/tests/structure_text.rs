//! owner: 003-document-model
//!
//! T038 contract tests for creation, structure and text mutation (FR-044–FR-058, FR-082): validation
//! errors with nothing changed, moves in one revision step, fragments, replace, ChildNode helpers,
//! adjacent insertion, normalize, character data, cloning and cross-document rules.

use ui4me_document::document::Document;
use ui4me_document::mutate::NodeOrString::{Node, Text};
use ui4me_engine_types::codes_generated::{Code, SubReason};

fn by_id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap_or_else(|| panic!("no #{i}"))
}

fn kids(d: &Document, n: ui4me_engine_types::ids::NodeId) -> Vec<ui4me_engine_types::ids::NodeId> {
    d.children(n).unwrap().to_vec()
}

fn ids(d: &Document, n: ui4me_engine_types::ids::NodeId) -> Vec<String> {
    kids(d, n)
        .iter()
        .map(|&c| {
            d.get_attribute(c, "id")
                .ok()
                .flatten()
                .unwrap_or("#")
                .to_owned()
        })
        .collect()
}

// table-a: A-10, A-11
#[test]
fn creation_validates_lowercases_and_steps_no_revision() {
    let mut d = Document::parse_html("<p id=a></p>").unwrap();
    let rev = d.revision();
    let e = d.create_element("DiV").unwrap();
    assert_eq!(d.local_name(e).unwrap(), "div");
    assert_eq!(
        d.namespace_uri(e).unwrap().as_deref(),
        Some("http://www.w3.org/1999/xhtml")
    );
    assert!(d.parent(e).unwrap().is_none());
    assert_eq!(
        d.create_element("a b").unwrap_err().code,
        Code::InvalidCharacter
    );
    assert_eq!(
        d.create_element("").unwrap_err().code,
        Code::InvalidCharacter
    );
    let s = d
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "svg:rect")
        .unwrap();
    assert_eq!(d.prefix(s).unwrap().as_deref(), Some("svg"));
    assert_eq!(d.tag_name(s).unwrap(), "svg:rect");
    assert_eq!(
        d.create_element_ns(None, "p:x").unwrap_err().code,
        Code::InvalidNamespace
    );
    let t = d.create_text_node("t").unwrap();
    let c = d.create_comment("c").unwrap();
    let f = d.create_document_fragment().unwrap();
    assert_eq!(d.node_type(t).unwrap(), 3);
    assert_eq!(d.node_type(c).unwrap(), 8);
    assert_eq!(d.node_type(f).unwrap(), 11);
    assert_eq!(d.revision(), rev, "creation steps no revision");
    assert!(
        d.create_element("template")
            .map(|n| d.template_contents(n).unwrap().is_some())
            .unwrap()
    );
}

// table-a: A-27
#[test]
fn hierarchy_errors_change_nothing() {
    let mut d = Document::parse_html("<!DOCTYPE html><div id=a><p id=b></p></div>").unwrap();
    let (a, b) = (by_id(&d, "a"), by_id(&d, "b"));
    let rev = d.revision();
    let before = ids(&d, a);
    // cycle: an ancestor into its descendant, and self
    assert_eq!(
        d.append_child(b, a).unwrap_err().code,
        Code::InvalidHierarchy
    );
    assert_eq!(
        d.append_child(a, a).unwrap_err().code,
        Code::InvalidHierarchy
    );
    // not a child
    let i_new = d.create_element("i").unwrap();
    let html_el = d.document_element().unwrap();
    let e = d.insert_before(a, i_new, Some(html_el)).unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidHierarchy, Some(SubReason::NotAChild))
    );
    let e = d
        .remove_child(a, d.document_element().unwrap())
        .unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidHierarchy, Some(SubReason::NotAChild))
    );
    // Document rules: text, second element, doctype under an element
    let t = d.create_text_node("x").unwrap();
    assert_eq!(
        d.append_child(d.document_node(), t).unwrap_err().code,
        Code::InvalidHierarchy
    );
    let el = d.create_element("div").unwrap();
    assert_eq!(
        d.append_child(d.document_node(), el).unwrap_err().code,
        Code::InvalidHierarchy
    );
    // a Document node is never insertable; a text node cannot take children
    assert_eq!(
        d.append_child(a, d.document_node()).unwrap_err().code,
        Code::InvalidHierarchy
    );
    assert_eq!(
        d.append_child(t, el).unwrap_err().code,
        Code::InvalidHierarchy
    );
    assert_eq!(d.revision(), rev);
    assert_eq!(ids(&d, a), before);
}

// table-a: A-27
#[test]
fn doctype_and_element_ordering_under_a_document() {
    let mut d = Document::create_empty(
        ui4me_engine_types::ids::DocumentId::from_raw(ui4me_engine_types::ids::RawId {
            index: 11,
            generation: std::num::NonZeroU32::MIN,
        }),
        "about:blank",
        Default::default(),
    )
    .unwrap();
    let root = d.document_node();
    let dt = d.doctype().is_some();
    assert!(dt);
    let html = d.document_element().unwrap();
    // Replacing the document element with another element is allowed.
    let other = d.create_element("html").unwrap();
    d.replace_child(root, other, html).unwrap();
    assert_eq!(d.document_element(), Some(other));
    // Removing the element and inserting a comment before the doctype is allowed.
    let c = d.create_comment("c").unwrap();
    d.insert_before(root, c, Some(d.arena().node(root).children()[0]))
        .unwrap();
    // An element cannot be inserted before a doctype.
    d.remove_child(root, other).unwrap();
    let el = d.create_element("html").unwrap();
    let doctype = d
        .arena()
        .node(root)
        .children()
        .iter()
        .copied()
        .find(|&n| d.node_type(n).unwrap() == 10)
        .unwrap();
    assert_eq!(
        d.insert_before(root, el, Some(doctype)).unwrap_err().code,
        Code::InvalidHierarchy
    );
    d.append_child(root, el).unwrap();
}

// table-a: A-27
#[test]
fn a_move_is_one_revision_step_and_keeps_state() {
    let mut d = Document::parse_html("<div id=a><p id=b x=1>t</p></div><div id=c></div>").unwrap();
    let (a, b, c) = (by_id(&d, "a"), by_id(&d, "b"), by_id(&d, "c"));
    let rev = d.revision();
    d.append_child(c, b).unwrap();
    assert_eq!(d.revision(), rev + 1, "a move happens inside one call");
    assert!(kids(&d, a).is_empty());
    assert_eq!(ids(&d, c), ["b"]);
    assert_eq!(
        d.get_attribute(b, "x").unwrap(),
        Some("1"),
        "attributes and children survive a move"
    );
    assert_eq!(d.text_content(b).unwrap().as_deref(), Some("t"));
    // insertBefore(node, node) is a no-op move and valid
    let r2 = d.revision();
    d.insert_before(c, b, Some(b)).unwrap();
    assert_eq!(ids(&d, c), ["b"]);
    assert!(d.revision() <= r2 + 1);
}

// table-a: A-27, A-32
#[test]
fn fragments_are_inserted_in_order_and_emptied() {
    let mut d = Document::parse_html("<div id=a><i id=z></i></div>").unwrap();
    let a = by_id(&d, "a");
    let f = d.create_document_fragment().unwrap();
    for n in ["p", "q", "r"] {
        let e = d.create_element(n).unwrap();
        d.set_id(e, n).unwrap();
        d.append_child(f, e).unwrap();
    }
    let rev = d.revision();
    d.insert_before(a, f, Some(by_id(&d, "z"))).unwrap();
    assert_eq!(d.revision(), rev + 1);
    assert_eq!(ids(&d, a), ["p", "q", "r", "z"]);
    assert!(kids(&d, f).is_empty(), "the fragment is emptied");
    // an empty fragment is not a mutation
    let r2 = d.revision();
    d.append_child(a, f).unwrap();
    assert_eq!(d.revision(), r2);
}

// table-a: A-27
#[test]
fn replace_child_is_atomic_and_returns_the_old_child() {
    let mut d = Document::parse_html("<div id=a><p id=b></p><p id=c></p></div>").unwrap();
    let (a, b, c) = (by_id(&d, "a"), by_id(&d, "b"), by_id(&d, "c"));
    let n = d.create_element("i").unwrap();
    d.set_id(n, "n").unwrap();
    let rev = d.revision();
    assert_eq!(d.replace_child(a, n, b).unwrap(), b);
    assert_eq!(d.revision(), rev + 1);
    assert_eq!(ids(&d, a), ["n", "c"]);
    assert!(d.parent(b).unwrap().is_none());
    // replace with a following sibling
    d.replace_child(a, c, n).unwrap();
    assert_eq!(ids(&d, a), ["c"]);
    let x_new = d.create_element("x").unwrap();
    let e = d.replace_child(a, x_new, n).unwrap_err();
    assert_eq!(e.sub_reason, Some(SubReason::NotAChild));
}

// table-a: A-32, A-33, A-50
#[test]
fn parent_node_and_child_node_helpers_accept_nodes_and_strings() {
    let mut d = Document::parse_html("<div id=a><b id=b></b></div>").unwrap();
    let (a, b) = (by_id(&d, "a"), by_id(&d, "b"));
    let i = d.create_element("i").unwrap();
    d.set_id(i, "i").unwrap();
    d.append(a, &[Text("t1".into()), Node(i)]).unwrap();
    assert_eq!(ids(&d, a), ["b", "#", "i"]);
    d.prepend(a, &[Text("p".into())]).unwrap();
    assert_eq!(ids(&d, a)[0], "#");
    d.before(b, &[Text("<x>".into())]).unwrap();
    let u = d.create_element("u").unwrap();
    d.after(b, &[Node(u)]).unwrap();
    assert_eq!(d.text_content(a).unwrap().as_deref(), Some("p<x>t1"));
    d.replace_with(b, &[Text("R".into()), Text("S".into())])
        .unwrap();
    assert_eq!(d.text_content(a).unwrap().as_deref(), Some("p<x>RSt1"));
    d.replace_children(a, &[Text("only".into())]).unwrap();
    assert_eq!(kids(&d, a).len(), 1);
    assert_eq!(d.text_content(a).unwrap().as_deref(), Some("only"));
    d.replace_children(a, &[]).unwrap();
    assert!(kids(&d, a).is_empty());
    // before/after/replaceWith/remove with no parent have no effect
    let lone = d.create_element("lone").unwrap();
    d.before(lone, &[Text("x".into())]).unwrap();
    d.remove(lone).unwrap();
    let rev = d.revision();
    d.append(a, &[]).unwrap();
    assert_eq!(d.revision(), rev);
}

// table-a: A-32, A-33
#[test]
fn string_conversion_failure_creates_nothing_visible() {
    let mut d = Document::parse_html("<!DOCTYPE html><div id=a></div>").unwrap();
    let a = by_id(&d, "a");
    let rev = d.revision();
    // two elements cannot go under a Document
    let e1 = d.create_element("html").unwrap();
    let e2 = d.create_element("html").unwrap();
    d.remove(d.document_element().unwrap()).unwrap();
    let rev2 = d.revision();
    assert!(d.append(d.document_node(), &[Node(e1), Node(e2)]).is_err());
    assert!(d.append(d.document_node(), &[Text("x".into())]).is_err());
    assert_eq!(d.revision(), rev2);
    assert!(rev2 > rev);
    let _ = a;
}

// table-a: A-50
#[test]
fn insert_adjacent_element_and_text() {
    let mut d = Document::parse_html("<div id=a><p id=b></p></div>").unwrap();
    let (a, b) = (by_id(&d, "a"), by_id(&d, "b"));
    let x = d.create_element("x").unwrap();
    d.set_id(x, "x").unwrap();
    assert_eq!(
        d.insert_adjacent_element(b, "BeforeBegin", x).unwrap(),
        Some(x)
    );
    assert_eq!(ids(&d, a), ["x", "b"]);
    d.insert_adjacent_text(b, "afterend", "T").unwrap();
    d.insert_adjacent_text(b, "afterbegin", "A").unwrap();
    d.insert_adjacent_text(b, "beforeend", "Z").unwrap();
    assert_eq!(
        d.text_content(a).unwrap().as_deref(),
        Some("AZT"),
        "afterbegin text, then the p, then beforeend text, then afterend text"
    );
    let lone = d.create_element("lone").unwrap();
    let y = d.create_element("y").unwrap();
    assert_eq!(
        d.insert_adjacent_element(lone, "beforebegin", y).unwrap(),
        None,
        "no parent: no effect"
    );
    let e = d.insert_adjacent_element(b, "middle", y).unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidArgument, Some(SubReason::Syntax))
    );
    let e = d.insert_adjacent_text(b, "", "t").unwrap_err();
    assert_eq!(e.sub_reason, Some(SubReason::Syntax));
}

// table-a: A-25, A-26
#[test]
fn text_content_and_node_value() {
    let mut d = Document::parse_html("<div id=a>x<b>y</b>z<!--c--></div>").unwrap();
    let a = by_id(&d, "a");
    assert_eq!(
        d.text_content(a).unwrap().as_deref(),
        Some("xyz"),
        "comments are excluded"
    );
    assert_eq!(d.text_content(d.document_node()).unwrap(), None);
    let rev = d.revision();
    d.set_text_content(a, "n").unwrap();
    assert_eq!(d.revision(), rev + 1);
    assert_eq!(kids(&d, a).len(), 1);
    d.set_text_content(a, "").unwrap();
    assert!(
        kids(&d, a).is_empty(),
        "empty text removes all children and adds none"
    );
    let r2 = d.revision();
    d.set_text_content(a, "").unwrap();
    assert_eq!(d.revision(), r2, "nothing to do is not a mutation");
    let t = d.create_text_node("v").unwrap();
    assert_eq!(d.node_value(t).unwrap().as_deref(), Some("v"));
    d.set_node_value(t, "w").unwrap();
    assert_eq!(d.node_value(t).unwrap().as_deref(), Some("w"));
    assert_eq!(d.node_value(a).unwrap(), None);
    d.set_node_value(a, "ignored").unwrap();
    assert!(kids(&d, a).is_empty());
}

// table-a: A-29
#[test]
fn normalize_merges_adjacent_text_and_removes_empty_nodes_in_one_call() {
    let mut d = Document::parse_html("<div id=a></div>").unwrap();
    let a = by_id(&d, "a");
    for s in ["a", "", "b"] {
        let t = d.create_text_node(s).unwrap();
        d.append_child(a, t).unwrap();
    }
    let sp = d.create_element("span").unwrap();
    d.append_child(a, sp).unwrap();
    for s in ["c", "d", ""] {
        let t = d.create_text_node(s).unwrap();
        d.append_child(sp, t).unwrap();
    }
    let rev = d.revision();
    d.normalize(a).unwrap();
    assert_eq!(d.revision(), rev + 1);
    assert_eq!(kids(&d, a).len(), 2);
    assert_eq!(d.text_data(kids(&d, a)[0]).unwrap(), "ab");
    assert_eq!(kids(&d, sp).len(), 1);
    assert_eq!(d.text_data(kids(&d, sp)[0]).unwrap(), "cd");
    d.normalize(a).unwrap();
    assert_eq!(
        d.revision(),
        rev + 1,
        "an already normal subtree is not a mutation"
    );
}

// table-a: A-60, A-61
#[test]
fn character_data_api_basics() {
    let mut d = Document::parse_html("<p id=a></p>").unwrap();
    let a = by_id(&d, "a");
    let t = d.create_text_node("hello").unwrap();
    d.append_child(a, t).unwrap();
    assert_eq!(d.data_length(t).unwrap(), 5);
    d.replace_data(t, 1, 3, "ipp").unwrap();
    assert_eq!(d.text_data(t).unwrap(), "hippo");
    let c = d.create_comment("cc").unwrap();
    d.append_data(c, "dd").unwrap();
    assert_eq!(d.text_data(c).unwrap(), "ccdd");
}

// table-a: A-12, A-28
#[test]
fn clone_and_import_are_detached_copies() {
    let mut d = Document::parse_html("<div id=a x=1><b>y</b></div>").unwrap();
    let a = by_id(&d, "a");
    let c = d.clone_node(a, true).unwrap();
    assert!(d.is_equal_node(a, Some(c)).unwrap());
    assert_ne!(a, c);
    let c2 = d.clone_node(a, false).unwrap();
    assert!(kids(&d, c2).is_empty());
    assert_eq!(d.get_attribute(c2, "x").unwrap(), Some("1"));
    let other = Document::parse_html("<i id=q>w</i>").unwrap();
    let q = other.element_by_id("q").unwrap();
    let imp = d.import_node(&other, q, true).unwrap();
    assert_eq!(d.text_content(imp).unwrap().as_deref(), Some("w"));
    assert_eq!(d.append_child(a, imp).unwrap(), imp);
}

// table-a: A-90, A-27
#[test]
fn foreign_documents_are_wrong_document_and_adopt_node_always_fails() {
    let mut d = Document::parse_html("<div id=a></div>").unwrap();
    let other = Document::parse_html("<i id=q></i>").unwrap();
    let a = d.key(by_id(&d, "a"));
    let q = other.key(other.element_by_id("q").unwrap());
    let rev = d.revision();
    assert_eq!(
        d.append_child_key(a, q).unwrap_err().code,
        Code::WrongDocument
    );
    assert_eq!(d.revision(), rev);
    // stale outranks wrong-document
    let t = d.create_text_node("t").unwrap();
    d.collect();
    let stale = d.key(t);
    assert_eq!(
        d.append_child_key(a, stale).unwrap_err().code,
        Code::StaleHandle
    );
    assert_eq!(
        d.adopt_node(&other, other.element_by_id("q").unwrap())
            .unwrap_err()
            .code,
        Code::WrongDocument
    );
}

// table-a: A-10
#[test]
fn the_node_limit_stops_creation_and_insertion_without_partial_effect() {
    let d = Document::create_empty(
        ui4me_engine_types::ids::DocumentId::from_raw(ui4me_engine_types::ids::RawId {
            index: 12,
            generation: std::num::NonZeroU32::MIN,
        }),
        "about:blank",
        Default::default(),
    )
    .unwrap();
    let live = d.arena().live_count() as u64;
    let mut d = d.with_limits(ui4me_document::limits::DomLimits {
        nodes: live + 2,
        ..Default::default()
    });
    let body = d.body().unwrap();
    let rev = d.revision();
    d.create_element("a").unwrap();
    d.create_element("b").unwrap();
    let e = d.create_element("c").unwrap_err();
    assert_eq!(e.code, Code::SizeLimit);
    assert_eq!(e.message, "limit exceeded: dom.nodes");
    let e = d.append(body, &[Text("x".into())]).unwrap_err();
    assert_eq!(e.code, Code::SizeLimit);
    assert_eq!(d.revision(), rev);
}
