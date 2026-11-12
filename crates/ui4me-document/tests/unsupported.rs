//! owner: 003-document-model
//!
//! Table A rows A-30, A-31 (navigation helpers) and the `UNSUPPORTED` rows A-90...A-94: each is
//! either absent from the public surface (checked by scanning the crate's source for the names the
//! standard APIs would use) or behaves as the row states.

use std::path::PathBuf;
use ui4me_document::document::Document;

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap()
}

// table-a: A-30, A-31
#[test]
fn children_and_element_navigation_are_static_lists_and_counts() {
    let mut d = Document::parse_html("<div id=a>t<b id=b></b>u<i id=i></i></div>").unwrap();
    let a = id(&d, "a");
    let kids = d.element_children(a).unwrap();
    assert_eq!(kids.len(), 2);
    assert_eq!(d.children_elements(a).unwrap(), kids);
    assert_eq!(d.first_element_child(a).unwrap(), Some(id(&d, "b")));
    assert_eq!(d.last_element_child(a).unwrap(), Some(id(&d, "i")));
    assert_eq!(d.child_element_count(a).unwrap(), 2);
    // static: a later mutation does not change the list already returned
    let extra = d.create_element("u").unwrap();
    d.append_child(a, extra).unwrap();
    assert_eq!(kids.len(), 2);
    assert_eq!(d.child_element_count(a).unwrap(), 3);
}

fn public_fn_names() -> String {
    let mut all = String::new();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                for line in std::fs::read_to_string(&path).unwrap().lines() {
                    if let Some(rest) = line.trim().strip_prefix("pub fn ") {
                        all.push_str(rest.split('(').next().unwrap_or(""));
                        all.push('\n');
                    }
                }
            }
        }
    }
    all
}

// table-a: A-91, A-92, A-93
#[test]
fn unsupported_apis_are_not_exposed() {
    let names = public_fn_names();
    for banned in [
        // A-91
        "mutation_observer",
        "observe",
        "create_range",
        "get_selection",
        "node_iterator",
        "tree_walker",
        "create_tree_walker",
        "evaluate",
        "create_event",
        // A-92
        "attach_shadow",
        "shadow_root",
        "assigned_nodes",
        "get_html",
        "set_html_unsafe",
        "set_html",
        // A-93
        "inner_text",
        "set_inner_text",
        "outer_text",
        "document_write",
        "write",
        "open",
        "dom_parser",
        "xml_serializer",
        "create_cdata_section",
        "create_processing_instruction",
        "create_attribute",
        "create_attribute_ns",
        "lookup_prefix",
        "lookup_namespace_uri",
        "is_default_namespace",
        "move_before",
        "define_custom_element",
    ] {
        assert!(
            !names.lines().any(|l| l == banned),
            "unsupported API `{banned}` must not be exposed"
        );
    }
    // `adoptNode` exists only to fail (A-90 is tested with the cross-document cases)
    assert!(names.lines().any(|l| l == "adopt_node"));
}

// table-a: A-92
#[test]
fn declarative_shadow_roots_stay_ordinary_templates_and_are_reported_once() {
    let d =
        Document::parse_html("<div id=h><template shadowrootmode=closed><p>s</p></template></div>")
            .unwrap();
    let h = id(&d, "h");
    let t = d.children(h).unwrap()[0];
    assert!(d.template_contents(t).unwrap().is_some());
    assert_eq!(
        d.children(h).unwrap().len(),
        1,
        "no shadow root is attached"
    );
    assert_eq!(d.parser_diagnostics().unsupported_features().len(), 1);
}

// table-a: A-93
#[test]
fn the_is_option_and_custom_elements_do_not_exist() {
    let mut d = Document::parse_html("<p id=a></p>").unwrap();
    // createElement takes a name only; a custom element name is just an element with no registry
    let e = d.create_element("my-widget").unwrap();
    assert_eq!(d.local_name(e).unwrap(), "my-widget");
    assert_eq!(d.template_contents(e).unwrap(), None);
    let p = id(&d, "a");
    assert_eq!(d.get_attribute(p, "is").unwrap(), None);
}

// table-a: A-94
#[test]
fn nested_browsing_contexts_are_elements_with_no_content() {
    let d = Document::parse_html(
        "<iframe id=f src=x.html></iframe><object id=o data=y></object><embed id=e src=z>",
    )
    .unwrap();
    for i in ["f", "o", "e"] {
        let el = id(&d, i);
        assert!(
            d.children(el).unwrap().is_empty(),
            "{i}: content is not loaded"
        );
    }
    assert_eq!(d.get_attribute(id(&d, "f"), "src").unwrap(), Some("x.html"));
}
