//! owner: 003-document-model
//!
//! T037 contract tests for attribute mutation (FR-032–FR-040): set / remove / toggle and the NS
//! variants, lowercasing, position preservation, append and removal order, validation errors with
//! nothing changed, `id` / `className`, `classList`, `dataset`, reflected properties.

use ui4me_document::document::Document;
use ui4me_document::journal::EntryKind;
use ui4me_engine_types::codes_generated::{Code, SubReason};

fn doc() -> Document {
    Document::parse_html("<div id=a x=1 y=2><svg id=s></svg></div>").unwrap()
}

// table-a: A-43
#[test]
fn set_lowercases_keeps_position_and_appends_new_attributes() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    d.set_attribute(a, "X", "10").unwrap();
    assert_eq!(
        d.get_attribute_names(a).unwrap(),
        ["id", "x", "y"],
        "position preserved, name lowercased"
    );
    assert_eq!(d.get_attribute(a, "x").unwrap(), Some("10"));
    d.set_attribute(a, "DATA-New", "n").unwrap();
    assert_eq!(
        d.get_attribute_names(a).unwrap(),
        ["id", "x", "y", "data-new"],
        "a new attribute is appended"
    );
    d.remove_attribute(a, "x").unwrap();
    assert_eq!(
        d.get_attribute_names(a).unwrap(),
        ["id", "y", "data-new"],
        "removal keeps the order of the rest"
    );
    d.remove_attribute(a, "absent").unwrap();
    // Any Unicode value is accepted.
    d.set_attribute(a, "title", "\u{1F600} \"q\" <&>").unwrap();
    assert_eq!(
        d.get_attribute(a, "title").unwrap(),
        Some("\u{1F600} \"q\" <&>")
    );
}

// table-a: A-43
#[test]
fn svg_attribute_names_keep_their_case() {
    let mut d = doc();
    let s = d.element_by_id("s").unwrap();
    d.set_attribute(s, "viewBox", "0 0 1 1").unwrap();
    d.set_attribute(s, "viewBox", "0 0 2 2").unwrap();
    assert_eq!(d.get_attribute_names(s).unwrap(), ["id", "viewBox"]);
    assert_eq!(
        d.get_attribute(s, "viewbox").unwrap(),
        None,
        "no lowercasing outside the HTML namespace"
    );
}

// table-a: A-43
#[test]
fn invalid_names_and_namespaces_change_nothing() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    let rev = d.revision();
    let seg_before = d.journal().since(0).entries.len();
    for bad in ["", "1x", "a b", "a>b", "-x"] {
        let e = d.set_attribute(a, bad, "v").unwrap_err();
        assert_eq!(e.code, Code::InvalidCharacter, "{bad:?}");
    }
    let e = d.set_attribute_ns(a, None, "p:l", "v").unwrap_err();
    assert_eq!(e.code, Code::InvalidNamespace);
    let e = d
        .set_attribute_ns(a, Some("urn:x"), "xml:l", "v")
        .unwrap_err();
    assert_eq!(e.code, Code::InvalidNamespace);
    let e = d
        .set_attribute_ns(a, Some("urn:x"), "a b", "v")
        .unwrap_err();
    assert_eq!(e.code, Code::InvalidCharacter);
    assert_eq!(d.revision(), rev);
    assert_eq!(d.journal().since(0).entries.len(), seg_before);
    assert_eq!(d.get_attribute_names(a).unwrap(), ["id", "x", "y"]);
    let txt = d.create_text_node("t").unwrap();
    assert_eq!(
        d.set_attribute(txt, "a", "v").unwrap_err().sub_reason,
        Some(SubReason::WrongNodeKind)
    );
}

// table-a: A-43, A-42
#[test]
fn ns_variants_match_on_namespace_and_local_name() {
    let mut d = doc();
    let s = d.element_by_id("s").unwrap();
    d.set_attribute_ns(s, Some("http://www.w3.org/1999/xlink"), "xlink:href", "u1")
        .unwrap();
    assert_eq!(d.get_attribute(s, "xlink:href").unwrap(), Some("u1"));
    assert!(
        d.has_attribute_ns(s, Some("http://www.w3.org/1999/xlink"), "href")
            .unwrap()
    );
    d.set_attribute_ns(s, Some("http://www.w3.org/1999/xlink"), "xlink:href", "u2")
        .unwrap();
    assert_eq!(
        d.get_attribute_names(s).unwrap(),
        ["id", "xlink:href"],
        "same ns+local replaces in place"
    );
    d.remove_attribute_ns(s, Some("http://www.w3.org/1999/xlink"), "href")
        .unwrap();
    assert!(
        !d.has_attribute_ns(s, Some("http://www.w3.org/1999/xlink"), "href")
            .unwrap()
    );
}

// table-a: A-43
#[test]
fn toggle_attribute_follows_the_dom_force_rules() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    assert!(d.toggle_attribute(a, "hidden", None).unwrap());
    assert_eq!(d.get_attribute(a, "hidden").unwrap(), Some(""));
    assert!(d.toggle_attribute(a, "hidden", Some(true)).unwrap());
    assert!(!d.toggle_attribute(a, "hidden", None).unwrap());
    assert!(!d.toggle_attribute(a, "hidden", Some(false)).unwrap());
    assert!(!d.has_attribute(a, "hidden").unwrap());
    assert_eq!(
        d.toggle_attribute(a, "a b", None).unwrap_err().code,
        Code::InvalidCharacter
    );
}

// table-a: A-43
#[test]
fn unchanged_writes_are_not_mutations() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    let rev = d.revision();
    d.set_attribute(a, "x", "1").unwrap();
    d.remove_attribute(a, "nope").unwrap();
    assert_eq!(d.revision(), rev);
    d.set_attribute(a, "x", "2").unwrap();
    assert_eq!(d.revision(), rev + 1);
}

// table-a: A-41
#[test]
fn id_and_class_name_round_trip() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    d.set_class_name(a, "  p   q ").unwrap();
    assert_eq!(
        d.class_name(a).unwrap(),
        "  p   q ",
        "className is the raw attribute"
    );
    d.set_id(a, "z").unwrap();
    assert_eq!(d.id_attr(a).unwrap(), "z");
    assert_eq!(d.element_by_id("z"), Some(a));
    assert_eq!(d.element_by_id("a"), None, "the id index follows the id");
    d.set_id(a, "").unwrap();
    assert_eq!(d.element_by_id(""), None, "the empty id never matches");
}

// table-a: A-45
#[test]
fn class_list_operations() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    d.class_list_add(a, &["b", "a", "b"]).unwrap();
    assert_eq!(d.class_list_value(a).unwrap(), "b a");
    assert!(d.class_list_contains(a, "a").unwrap());
    assert_eq!(d.class_list_length(a).unwrap(), 2);
    assert_eq!(d.class_list_item(a, 0).unwrap().as_deref(), Some("b"));
    assert!(!d.class_list_toggle(a, "a", None).unwrap());
    assert!(d.class_list_replace(a, "b", "c").unwrap());
    assert_eq!(d.class_name(a).unwrap(), "c");
    d.class_list_remove(a, &["c"]).unwrap();
    assert_eq!(d.class_name(a).unwrap(), "", "the attribute stays, empty");
    let e = d.class_list_add(a, &["x y"]).unwrap_err();
    assert_eq!(e.code, Code::InvalidCharacter);
    let e = d.class_list_toggle(a, "", None).unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidArgument, Some(SubReason::Syntax))
    );
}

// table-a: A-46
#[test]
fn dataset_maps_camel_case_to_data_attributes() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    d.dataset_set(a, "fooBar", "1").unwrap();
    assert_eq!(d.get_attribute(a, "data-foo-bar").unwrap(), Some("1"));
    assert_eq!(d.dataset_get(a, "fooBar").unwrap().as_deref(), Some("1"));
    d.set_attribute(a, "data-x-y-z", "q").unwrap();
    let l = d.dataset_list(a).unwrap();
    assert_eq!(
        l,
        [
            ("fooBar".to_owned(), "1".to_owned()),
            ("xYZ".to_owned(), "q".to_owned())
        ]
    );
    let e = d.dataset_set(a, "foo-bar", "v").unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidArgument, Some(SubReason::Syntax)),
        "a hyphen before a lowercase letter"
    );
    let e = d.dataset_set(a, "a b", "v").unwrap_err();
    assert_eq!(e.code, Code::InvalidCharacter);
    d.dataset_delete(a, "fooBar").unwrap();
    assert_eq!(d.dataset_get(a, "fooBar").unwrap(), None);
}

// table-a: A-47
#[test]
fn reflected_html_element_properties() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    assert!(!d.hidden(a).unwrap());
    d.set_hidden(a, true).unwrap();
    assert!(d.hidden(a).unwrap());
    d.set_hidden(a, false).unwrap();
    assert!(!d.has_attribute(a, "hidden").unwrap());
    d.set_title_attr(a, "T").unwrap();
    d.set_lang(a, "en").unwrap();
    d.set_dir(a, "RTL").unwrap();
    assert_eq!(
        (
            d.title_attr(a).unwrap().as_str(),
            d.lang(a).unwrap().as_str(),
            d.dir(a).unwrap().as_str()
        ),
        ("T", "en", "rtl")
    );
    d.set_dir(a, "sideways").unwrap();
    assert_eq!(
        d.dir(a).unwrap(),
        "",
        "dir is limited to known values on read"
    );
}

// table-a: A-43
#[test]
fn attribute_writes_journal_the_first_prior_list_and_each_change() {
    let mut d = doc();
    let a = d.element_by_id("a").unwrap();
    let _ = d.take_epoch();
    d.set_attribute(a, "x", "5").unwrap();
    d.set_attribute(a, "x", "6").unwrap();
    d.set_attribute(a, "z", "9").unwrap();
    let seg = d.take_epoch();
    let priors = seg
        .entries()
        .iter()
        .filter(|e| matches!(e.kind, EntryKind::PriorAttributes { .. }))
        .count();
    let changes = seg
        .entries()
        .iter()
        .filter(|e| matches!(e.kind, EntryKind::Attribute { .. }))
        .count();
    assert_eq!((priors, changes), (1, 3));
}
