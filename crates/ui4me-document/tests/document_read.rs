//! owner: 003-document-model
//!
//! T021 contract tests for US1 (load a document and inspect its tree): creation, decoding,
//! diagnostics, namespaces and names, doctype and modes, URL and base URI, title, navigation,
//! attribute reads, templates, stylesheet list.

use ui4me_document::document::{Document, DocumentMode, Lifecycle};
use ui4me_document::limits::DomLimits;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::{DocumentId, RawId};

fn id(i: u32) -> DocumentId {
    DocumentId::from_raw(RawId {
        index: i,
        generation: std::num::NonZeroU32::MIN,
    })
}

fn by_id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap_or_else(|| panic!("no #{i}"))
}

// table-a: A-01
#[test]
fn creation_is_live_on_return_and_a_failed_creation_leaves_no_document() {
    let d = Document::parse(id(1), "<p>x</p>").unwrap();
    assert_eq!(d.lifecycle(), Lifecycle::Live);
    assert_eq!(
        d.lifecycle_history(),
        &[Lifecycle::Created, Lifecycle::Parsing, Lifecycle::Live],
        "internally Created and Parsing exist; the caller only sees Live"
    );
    // Failure: the input is over the limit (FR-002): no document exists afterwards.
    let r = Document::create(
        id(2),
        "<p>0123456789</p>",
        "about:blank",
        DomLimits {
            input_bytes: 4,
            ..Default::default()
        },
    );
    assert_eq!(r.unwrap_err().code, Code::SizeLimit);
    // node limit hit by the parse
    let r = Document::create(
        id(3),
        "<p><p><p><p>",
        "about:blank",
        DomLimits {
            nodes: 3,
            ..Default::default()
        },
    );
    assert_eq!(r.unwrap_err().code, Code::SizeLimit);
}

// table-a: A-01, A-02, A-03, A-04
#[test]
fn create_empty_is_doctype_html_head_body_no_quirks() {
    let d = Document::create_empty(id(4), "about:blank", DomLimits::default()).unwrap();
    assert_eq!(d.lifecycle(), Lifecycle::Live);
    let dt = d.doctype().expect("a doctype");
    assert_eq!(
        (
            dt.name.as_str(),
            dt.public_id.as_str(),
            dt.system_id.as_str()
        ),
        ("html", "", "")
    );
    assert_eq!(d.local_name(d.document_element().unwrap()).unwrap(), "html");
    assert!(d.head().is_some() && d.body().is_some());
    assert_eq!(d.mode(), DocumentMode::NoQuirks);
    assert_eq!(d.compat_mode(), "CSS1Compat");
    assert_eq!(d.revision(), 0, "creation steps no revision");
}

// table-a: A-04
#[test]
fn modes_follow_the_doctype() {
    assert_eq!(
        Document::parse_html("<p>").unwrap().compat_mode(),
        "BackCompat"
    );
    assert_eq!(
        Document::parse_html("<!DOCTYPE html><p>").unwrap().mode(),
        DocumentMode::NoQuirks
    );
    let lq = Document::parse_html("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\"><p>").unwrap();
    assert_eq!(lq.mode(), DocumentMode::LimitedQuirks);
    assert_eq!(lq.compat_mode(), "CSS1Compat");
    let dt = lq.doctype().unwrap();
    assert!(dt.public_id.contains("XHTML 1.0 Transitional"));
}

// table-a: A-01
#[test]
fn utf8_decoding_reports_diagnostics_and_unsupported_encodings() {
    let d = Document::create_from_bytes(
        id(5),
        b"\xEF\xBB\xBF<p id=a>x\xFFy</p>",
        "about:blank",
        DomLimits::default(),
    )
    .unwrap();
    assert_eq!(
        d.text_content(by_id(&d, "a")).unwrap().as_deref(),
        Some("x\u{FFFD}y")
    );
    assert!(
        d.parser_diagnostics()
            .records()
            .iter()
            .any(|r| r.message.contains("U+FFFD")),
        "one diagnostic per invalid sequence"
    );
    let d = Document::create_from_bytes(
        id(6),
        b"<meta charset=windows-1252><p>x",
        "about:blank",
        DomLimits::default(),
    )
    .unwrap();
    assert_eq!(d.parser_diagnostics().unsupported_features().len(), 1);
    let d =
        Document::create_from_bytes(id(7), b"\xFF\xFE<p>x", "about:blank", DomLimits::default())
            .unwrap();
    assert!(d.parser_diagnostics().unsupported_features()[0].contains("UTF-16"));
}

// table-a: A-01
#[test]
fn scripts_are_inserted_but_inert_and_reported_once() {
    let d = Document::parse_html("<body onload='x()'><script>alert(1)</script><p onclick='y()'>t</p><template shadowrootmode=open><i>s</i></template></body>").unwrap();
    assert_eq!(
        d.parser_diagnostics().unsupported_features().len(),
        1,
        "one UNSUPPORTED_FEATURE report per document"
    );
    let msg = &d.parser_diagnostics().unsupported_features()[0];
    assert!(
        msg.contains("1 script")
            && msg.contains("2 event-handler")
            && msg.contains("1 declarative"),
        "{msg}"
    );
    let t = d
        .arena()
        .iter()
        .find(|n| n.as_element().is_some_and(|e| e.is_html_named("template")))
        .unwrap()
        .id();
    assert!(
        d.template_contents(t).unwrap().is_some(),
        "an ordinary template with contents, no shadow root"
    );
    assert_eq!(
        d.get_attribute(d.body().unwrap(), "onload").unwrap(),
        Some("x()")
    );
}

// table-a: A-20, A-40
#[test]
fn names_and_namespaces_of_html_svg_and_mathml_elements() {
    let d = Document::parse_html("<div id=h></div><svg id=s viewBox='0 0 1 1'><foreignObject id='f'/><a xlink:href=u id=a /></svg><math id=m><mi>x</mi></math>").unwrap();
    let h = by_id(&d, "h");
    assert_eq!(d.tag_name(h).unwrap(), "DIV");
    assert_eq!(d.node_name(h).unwrap(), "DIV");
    assert_eq!(
        d.namespace_uri(h).unwrap().as_deref(),
        Some("http://www.w3.org/1999/xhtml")
    );
    let s = by_id(&d, "s");
    assert_eq!(
        d.tag_name(s).unwrap(),
        "svg",
        "no uppercasing outside the HTML namespace"
    );
    assert_eq!(
        d.namespace_uri(s).unwrap().as_deref(),
        Some("http://www.w3.org/2000/svg")
    );
    assert_eq!(
        d.local_name(by_id(&d, "f")).unwrap(),
        "foreignObject",
        "tag-name adjustment"
    );
    assert_eq!(
        d.get_attribute(s, "viewBox").unwrap(),
        Some("0 0 1 1"),
        "attribute-name adjustment"
    );
    let a = by_id(&d, "a");
    assert_eq!(d.get_attribute(a, "xlink:href").unwrap(), Some("u"));
    assert_eq!(
        d.get_attribute_ns(a, Some("http://www.w3.org/1999/xlink"), "href")
            .unwrap(),
        Some("u")
    );
    assert_eq!(
        d.namespace_uri(by_id(&d, "m")).unwrap().as_deref(),
        Some("http://www.w3.org/1998/Math/MathML")
    );
    assert_eq!(d.prefix(h).unwrap(), None);
    assert_eq!(d.node_type(h).unwrap(), 1);
    assert_eq!(d.node_name(d.document_node()).unwrap(), "#document");
}

// table-a: A-42, A-44
#[test]
fn attributes_keep_source_order_and_duplicates_follow_the_parser() {
    let d = Document::parse_html("<p id=a z=1 b=2 z=3 A=4>").unwrap();
    let a = by_id(&d, "a");
    assert_eq!(
        d.get_attribute_names(a).unwrap(),
        ["id", "z", "b", "a"],
        "source order, first duplicate wins, names lowercased"
    );
    assert_eq!(d.get_attribute(a, "z").unwrap(), Some("1"));
    assert_eq!(
        d.get_attribute(a, "Z").unwrap(),
        Some("1"),
        "lookup lowercases for HTML elements"
    );
    assert!(d.has_attribute(a, "B").unwrap() && !d.has_attribute(a, "c").unwrap());
    assert!(d.has_attributes(a).unwrap());
    let vals: Vec<_> = d
        .attributes(a)
        .unwrap()
        .iter()
        .map(|x| x.value.as_str())
        .collect();
    assert_eq!(vals, ["a", "1", "2", "4"]);
}

// table-a: A-03, A-06, A-05
#[test]
fn url_base_uri_and_title() {
    let d = Document::create(
        id(8),
        "<head><title>  A \n  title </title><base href='http://e/x/'><base href=ignored></head>",
        "file:///d/index.html",
        DomLimits::default(),
    )
    .unwrap();
    assert_eq!(d.url(), "file:///d/index.html");
    assert_eq!(
        d.base_uri(),
        "http://e/x/",
        "the first base[href] freezes the base URL"
    );
    assert_eq!(d.title(), "A title");
    let d2 = Document::parse_html("<p>").unwrap();
    assert_eq!(d2.url(), "about:blank");
    assert_eq!(d2.base_uri(), "about:blank");
    assert_eq!(d2.title(), "");
}

// table-a: A-05, A-06
#[test]
fn title_setter_and_base_url_follow_insertion_and_removal() {
    let mut d = Document::parse_html("<head></head><body></body>").unwrap();
    d.set_title("Hello").unwrap();
    assert_eq!(d.title(), "Hello");
    d.set_title("Bye").unwrap();
    assert_eq!(d.title(), "Bye");
    assert_eq!(
        d.arena()
            .iter()
            .filter(|n| n.as_element().is_some_and(|e| e.is_html_named("title")))
            .count(),
        1
    );
    let head = d.head().unwrap();
    let base = d.create_element("base").unwrap();
    d.set_attribute(base, "href", "http://x/").unwrap();
    assert_eq!(d.base_uri(), "about:blank", "a detached base has no effect");
    d.append_child(head, base).unwrap();
    assert_eq!(d.base_uri(), "http://x/");
    d.set_attribute(base, "href", "http://y/").unwrap();
    assert_eq!(d.base_uri(), "http://y/");
    d.remove_child(head, base).unwrap();
    assert_eq!(d.base_uri(), "about:blank");
}

// table-a: A-21, A-23, A-24
#[test]
fn navigation_over_a_parsed_tree() {
    let d = Document::parse_html("<div id=a><p id=b>x</p><p id=c></p></div>").unwrap();
    let (a, b, c) = (by_id(&d, "a"), by_id(&d, "b"), by_id(&d, "c"));
    assert_eq!(d.parent_node(b).unwrap(), Some(a));
    assert_eq!(d.next_sibling(b).unwrap(), Some(c));
    assert_eq!(d.root_node(c).unwrap(), d.document_node());
    assert!(d.contains(a, Some(c)).unwrap());
    assert!(d.is_connected(a).unwrap());
}

// table-a: A-52, A-22
#[test]
fn template_contents_are_a_same_document_fragment_not_children() {
    let d =
        Document::parse_html("<template id=t><p id=in>x</p></template><div id=out></div>").unwrap();
    let t = by_id(&d, "t");
    assert!(
        d.children(t).unwrap().is_empty(),
        "contents are not children"
    );
    let c = d.template_contents(t).unwrap().unwrap();
    assert_eq!(d.children(c).unwrap().len(), 1);
    assert_eq!(
        d.owner_document(c).unwrap(),
        Some(d.id()),
        "same document (FR-016)"
    );
    assert!(d.element_by_id("in").is_none(), "excluded from ID lookup");
    assert!(!d.is_connected(d.children(c).unwrap()[0]).unwrap());
    assert_eq!(
        d.root_node(d.children(c).unwrap()[0]).unwrap(),
        c,
        "the contents fragment is the root"
    );
}

// table-a: A-15
#[test]
fn stylesheet_sources_in_tree_order() {
    let d = Document::parse_html("<head><style>a{}</style></head><body><style>b{}</style></body>")
        .unwrap();
    let l = d.stylesheets();
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].text.as_deref(), Some("a{}"));
    assert_eq!(l[1].text.as_deref(), Some("b{}"));
}

// table-a: A-01
#[test]
fn parser_diagnostics_are_capped_at_1000_with_a_total() {
    let html = "</x>".repeat(1500);
    let d = Document::parse_html(&html).unwrap();
    assert!(d.parser_diagnostics().total() >= 1500);
    assert_eq!(d.parser_diagnostics().records().len(), 1000);
}
