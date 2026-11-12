//! owner: 003-document-model
//!
//! T081 (FR-072, FR-076): form owners by nesting and by `form="id"`, parser association and the
//! parser-inserted rule, re-evaluation on insertion, removal, moves, `form` attribute and id changes,
//! and `form.elements` in tree order.

use ui4me_document::document::Document;

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap_or_else(|| panic!("no #{i}"))
}

fn owner(d: &Document, i: &str) -> Option<String> {
    d.form_owner(id(d, i))
        .unwrap()
        .map(|f| d.id_attr(f).unwrap())
}

// table-a: A-53
#[test]
fn owners_by_nesting_by_form_attribute_and_none() {
    let d = Document::parse_html("<form id=f1><input id=a><div><button id=b></button></div></form><form id=f2></form><input id=c form=f2><input id=d form=nope><input id=e>").unwrap();
    assert_eq!(owner(&d, "a").as_deref(), Some("f1"));
    assert_eq!(owner(&d, "b").as_deref(), Some("f1"));
    assert_eq!(
        owner(&d, "c").as_deref(),
        Some("f2"),
        "form attribute names a form elsewhere"
    );
    assert_eq!(
        owner(&d, "d"),
        None,
        "a form attribute with no matching form: no owner"
    );
    assert_eq!(owner(&d, "e"), None);
    let f1 = id(&d, "f1");
    let els: Vec<_> = d
        .form_elements(f1)
        .unwrap()
        .iter()
        .map(|&n| d.id_attr(n).unwrap())
        .collect();
    assert_eq!(els, ["a", "b"], "listed elements in tree order");
    assert_eq!(d.form_elements(id(&d, "f2")).unwrap().len(), 1);
}

// table-a: A-53
#[test]
fn the_form_attribute_overrides_the_ancestor_and_img_is_not_listed() {
    let d = Document::parse_html(
        "<form id=f1><input id=a form=f2><img id=i></form><form id=f2></form>",
    )
    .unwrap();
    assert_eq!(owner(&d, "a").as_deref(), Some("f2"));
    assert_eq!(owner(&d, "i").as_deref(), Some("f1"), "img has an owner");
    assert!(
        d.form_elements(id(&d, "f1")).unwrap().is_empty(),
        "but is not in elements"
    );
}

// table-a: A-53
#[test]
fn the_owner_follows_id_changes_insertion_removal_and_moves() {
    let mut d = Document::parse_html(
        "<div id=host><form id=f><input id=a></form></div><input id=c form=g><div id=other></div>",
    )
    .unwrap();
    let (host, other) = (id(&d, "host"), id(&d, "other"));
    let a = id(&d, "a");
    let f = id(&d, "f");
    assert_eq!(owner(&d, "c"), None);
    // changing the form's id makes it match `form=g`
    d.set_id(f, "g").unwrap();
    assert_eq!(
        owner(&d, "c").as_deref(),
        Some("g"),
        "id change re-resolves form= owners"
    );
    d.set_id(f, "f").unwrap();
    assert_eq!(
        owner(&d, "c"),
        None,
        "owner becomes null after the id changes away"
    );
    // removing the input from the form clears its owner; inserting elsewhere sets the new one
    d.remove_child(f, a).unwrap();
    assert_eq!(d.form_owner(a).unwrap(), None);
    d.append_child(f, a).unwrap();
    assert_eq!(d.form_owner(a).unwrap(), Some(f));
    // moving the whole form subtree keeps the owner
    d.append_child(other, f).unwrap();
    assert_eq!(d.form_owner(a).unwrap(), Some(f));
    // removing the form clears owners that depended on it only as an ancestor... a stays inside it
    d.remove_child(other, f).unwrap();
    assert_eq!(
        d.form_owner(a).unwrap(),
        Some(f),
        "still a descendant of the (detached) form"
    );
    // the form attribute set/changed/removed re-evaluates
    d.append_child(host, f).unwrap();
    let c = id(&d, "c");
    d.set_attribute(c, "form", "f").unwrap();
    assert_eq!(d.form_owner(c).unwrap(), Some(f));
    d.remove_attribute(c, "form").unwrap();
    assert_eq!(d.form_owner(c).unwrap(), None);
}

// table-a: A-53
#[test]
fn a_created_element_gets_its_owner_when_inserted_not_when_created() {
    let mut d = Document::parse_html("<form id=f></form>").unwrap();
    let f = id(&d, "f");
    let i = d.create_element("input").unwrap();
    assert_eq!(d.form_owner(i).unwrap(), None);
    d.append_child(f, i).unwrap();
    assert_eq!(d.form_owner(i).unwrap(), Some(f));
    // text node / non-element: wrong node kind
    let t = d.create_text_node("x").unwrap();
    assert!(d.form_owner(t).is_err());
    assert!(d.form_elements(i).is_err(), "elements is for forms");
}

// table-a: A-53
#[test]
fn parser_association_follows_the_form_pointer_including_misnested_forms() {
    let d = Document::parse_html("<form id=f1><table><tr><td><input id=a></td></tr></table></form><form id=f2><form id=f3><input id=b></form>").unwrap();
    assert_eq!(owner(&d, "a").as_deref(), Some("f1"));
    // a nested <form> start tag is ignored while a form is open: b belongs to f2
    assert!(d.element_by_id("f3").is_none());
    assert_eq!(owner(&d, "b").as_deref(), Some("f2"));
    // Moving a form-associated element is a removal followed by an insertion: the removing steps
    // reset the owner (and unset the parser-inserted flag), so the insertion associates it again.
    let mut d = Document::parse_html("<form id=f1><input id=a></form><form id=f2></form>").unwrap();
    let (a, f2) = (id(&d, "a"), id(&d, "f2"));
    d.append_child(f2, a).unwrap();
    assert_eq!(d.form_owner(a).unwrap(), Some(f2));
    d.remove_child(f2, a).unwrap();
    assert_eq!(d.form_owner(a).unwrap(), None, "removal resets");
    // A parser-created subtree inserted as a whole keeps the parser's association (no insertion reset).
    let (f1,) = (id(&d, "f1"),);
    let mut d2 = Document::parse_html("<div id=x></div>").unwrap();
    let imported = d2.import_node(&d, f1, true).unwrap();
    let x = id(&d2, "x");
    d2.append_child(x, imported).unwrap();
    let inner = d2.children(imported).unwrap().to_vec();
    assert!(inner.is_empty(), "f1 lost its input when a was moved away");
}

// table-a: A-53
#[test]
fn owner_of_nodes_in_a_fragment_parse_uses_the_context_form() {
    let mut d = Document::parse_html("<form id=f><div id=c></div></form>").unwrap();
    let c = id(&d, "c");
    d.set_inner_html(c, "<input id=n>").unwrap();
    assert_eq!(owner(&d, "n").as_deref(), Some("f"));
}
