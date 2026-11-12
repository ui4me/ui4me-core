//! owner: 003-document-model
//!
//! T068 (FR-014-FR-017, FR-050 A-52): template contents are created with every template (parser and
//! API), are inert (never connected, excluded from lookup and queries, no stylesheet or requester
//! effects), belong to the template's document, serialize and clone with the template, and can be
//! moved into the document without adoption.

use ui4me_document::document::Document;
use ui4me_document::hooks::Notification;

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap()
}

// table-a: A-52
#[test]
fn contents_exist_for_parser_and_api_templates_and_nested_templates() {
    let mut d =
        Document::parse_html("<template id=t><template id=n><b>deep</b></template></template>")
            .unwrap();
    let t = id(&d, "t");
    let c = d.template_contents(t).unwrap().unwrap();
    let n = d.children(c).unwrap()[0];
    let nc = d.template_contents(n).unwrap().unwrap();
    assert_eq!(d.text_content(nc).unwrap().as_deref(), Some("deep"));
    let made = d.create_element("template").unwrap();
    assert!(
        d.template_contents(made).unwrap().is_some(),
        "API-created templates own contents too"
    );
    let div = d.create_element("div").unwrap();
    assert_eq!(d.template_contents(div).unwrap(), None);
    // namespaces: an SVG element named template is an ordinary element
    let svg = d
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "template")
        .unwrap();
    assert_eq!(d.template_contents(svg).unwrap(), None);
}

// table-a: A-52, A-07
#[test]
fn contents_are_inert_for_connectedness_lookup_queries_and_stylesheets() {
    let mut d =
        Document::parse_html("<template id=t><style>x{}</style><p id=p class=k>x</p></template>")
            .unwrap();
    let t = id(&d, "t");
    let c = d.template_contents(t).unwrap().unwrap();
    assert_eq!(d.element_by_id("p"), None);
    assert!(
        d.fragment_element_by_id(c, "p").unwrap().is_some(),
        "a lookup rooted in the contents fragment sees them"
    );
    assert!(
        d.stylesheets().is_empty(),
        "style elements inside contents are not sources"
    );
    assert!(
        d.get_elements_by_class_name(d.body().unwrap(), "k")
            .unwrap()
            .is_empty()
    );
    let p = d.children(c).unwrap()[1];
    assert!(!d.is_connected(p).unwrap());
    assert_eq!(d.root_node(p).unwrap(), c);
    // insert a style into the contents, then move the template into the document: still no source
    let s = d.create_element("style").unwrap();
    let rev = d.stylesheet_revision();
    d.append_child(c, s).unwrap();
    assert_eq!(
        d.stylesheet_revision(),
        rev,
        "changes inside contents never touch the source list"
    );
    let body = d.body().unwrap();
    d.append_child(body, t).unwrap();
    assert!(
        d.stylesheets().is_empty(),
        "contents stay inert even when their template is connected"
    );
}

// table-a: A-52
#[test]
fn serialize_and_clone_include_the_contents_and_moving_nodes_out_needs_no_adoption() {
    let mut d =
        Document::parse_html("<template id=t><i id=i>x</i></template><div id=host></div>").unwrap();
    let t = id(&d, "t");
    assert_eq!(
        d.outer_html(t).unwrap(),
        "<template id=\"t\"><i id=\"i\">x</i></template>"
    );
    let clone = d.clone_node(t, true).unwrap();
    assert_eq!(d.outer_html(clone).unwrap(), d.outer_html(t).unwrap());
    let cc = d.template_contents(clone).unwrap().unwrap();
    let oc = d.template_contents(t).unwrap().unwrap();
    assert_ne!(cc, oc, "the clone owns its own contents");
    let shallow = d.clone_node(t, false).unwrap();
    let sc = d.template_contents(shallow).unwrap().unwrap();
    assert!(d.children(sc).unwrap().is_empty());
    // the contents' owner document is the template's document: nodes move into the document directly
    let i = d.children(oc).unwrap()[0];
    assert_eq!(d.owner_document(i).unwrap(), Some(d.id()));
    let host = id(&d, "host");
    d.append_child(host, i).unwrap();
    assert!(d.is_connected(i).unwrap());
    assert_eq!(d.element_by_id("i"), Some(i));
}

// table-a: A-52
#[test]
fn a_template_cannot_be_inserted_into_its_own_contents() {
    let mut d = Document::parse_html("<template id=t></template>").unwrap();
    let t = id(&d, "t");
    let c = d.template_contents(t).unwrap().unwrap();
    let e = d.append_child(c, t).unwrap_err();
    assert_eq!(
        e.code,
        ui4me_engine_types::codes_generated::Code::InvalidHierarchy,
        "host-including ancestor check"
    );
}

// table-a: A-52
#[test]
fn requester_elements_inside_contents_notify_only_after_leaving_them() {
    let mut d =
        Document::parse_html("<template id=t><img id=im src=a.png></template><div id=host></div>")
            .unwrap();
    let notes: std::rc::Rc<std::cell::RefCell<Vec<Notification>>> = Default::default();
    let n = notes.clone();
    d.hooks_mut()
        .set_notification_sink(Box::new(move |x| n.borrow_mut().push(x.clone())));
    let t = id(&d, "t");
    let c = d.template_contents(t).unwrap().unwrap();
    let im = d.children(c).unwrap()[0];
    d.set_attribute(im, "src", "b.png").unwrap();
    d.remove_child(c, im).unwrap();
    assert!(notes.borrow().is_empty());
    d.append_child(id(&d, "host"), im).unwrap();
    assert_eq!(notes.borrow().len(), 1);
}
