//! owner: 003-document-model
//!
//! T059 / T062 (FR-065-FR-070, SC-005 non-oracle part): getElementById, getElementsBy*, and the
//! scope, order and error mapping of querySelector(All) / matches / closest composed with 006's
//! selector service (`ui4me-style`, dev-dependency only). The `:has()` positive tests (T063) need the
//! `gate-profiles` feature of ui4me-style and are not part of this file.

use crate::style::query::SelectorService;
use ui4me_document::document::Document;
use ui4me_engine_types::codes_generated::{Code, SubReason};

const HTML: &str = "<!DOCTYPE html><body><div id=a class='x y'><p id=b class=y data-k=1>t</p><p id=c lang=en></p><span id=d class=x></span></div><template><p id=hidden class=x></p></template><ul><li id=l1>1<li id=l2>2<li id=l3>3</ul></body>";

/// Under the `gate-profiles` build the whole binary runs the test-only `ui4me-css/1+has` profile; the
/// profile is fixed per process, so every test selects it before its first selector parse.
fn profile_init() {
    #[cfg(feature = "gate-profiles")]
    {
        static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        ONCE.get_or_init(|| {
            crate::style::profile::request(
                crate::style::profile::HAS_ID,
                crate::style::profile::VERSION,
            )
            .expect("the +has test profile");
        });
    }
}

fn doc() -> Document {
    profile_init();
    Document::parse_html(HTML).unwrap()
}

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap_or_else(|| panic!("no #{i}"))
}

fn names(d: &Document, v: &[ui4me_engine_types::ids::NodeId]) -> Vec<String> {
    v.iter().map(|&n| d.id_attr(n).unwrap()).collect()
}

// table-a: A-07
#[test]
fn get_element_by_id_is_first_in_tree_order_and_ignores_template_contents() {
    let mut d = doc();
    assert_eq!(d.element_by_id("b"), Some(id(&d, "b")));
    assert_eq!(
        d.element_by_id("hidden"),
        None,
        "template contents are excluded for a document root"
    );
    assert_eq!(d.element_by_id(""), None);
    // duplicate ids: the first in tree order wins
    let a = id(&d, "a");
    let dup = d.create_element("i").unwrap();
    d.set_id(dup, "b").unwrap();
    d.insert_before(a, dup, Some(d.children(a).unwrap()[0]))
        .unwrap();
    assert_eq!(d.element_by_id("b"), Some(dup));
    // a DocumentFragment is a tree walk and sees its own descendants
    let f = d.create_document_fragment().unwrap();
    let x = d.create_element("x").unwrap();
    d.set_id(x, "fx").unwrap();
    d.append_child(f, x).unwrap();
    assert_eq!(d.fragment_element_by_id(f, "fx").unwrap(), Some(x));
    assert_eq!(
        d.element_by_id("fx"),
        None,
        "detached: not in the document id lookup"
    );
}

// table-a: A-08
#[test]
fn static_lists_by_tag_class_and_name() {
    let d = doc();
    let body = d.body().unwrap();
    assert_eq!(
        names(&d, &d.get_elements_by_tag_name(body, "P").unwrap()),
        ["b", "c"],
        "HTML names are lowercased for the comparison"
    );
    assert_eq!(
        d.get_elements_by_tag_name(body, "*").unwrap().len(),
        9,
        "* matches every element (the template's contents are not descendants)"
    );
    assert_eq!(
        names(&d, &d.get_elements_by_class_name(body, "x").unwrap()),
        ["a", "d"]
    );
    assert_eq!(
        names(&d, &d.get_elements_by_class_name(body, "y x").unwrap()),
        ["a"],
        "all listed classes"
    );
    assert!(d.get_elements_by_class_name(body, "  ").unwrap().is_empty());
    assert_eq!(
        d.get_elements_by_tag_name_ns(body, Some("http://www.w3.org/1999/xhtml"), "li")
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        d.get_elements_by_tag_name_ns(body, Some("*"), "span")
            .unwrap()
            .len(),
        1
    );
    let doc_root = d.document_node();
    assert_eq!(d.get_elements_by_name("none").unwrap().len(), 0);
    assert!(d.get_elements_by_tag_name(doc_root, "html").unwrap().len() == 1);
    let txt = d.children(id(&d, "b")).unwrap()[0];
    assert_eq!(
        d.get_elements_by_tag_name(txt, "p").unwrap_err().sub_reason,
        Some(SubReason::WrongNodeKind)
    );
}

// table-a: A-08
#[test]
fn quirks_mode_class_matching_is_ascii_case_insensitive() {
    let d = Document::parse_html("<p id=a class=Foo>").unwrap();
    assert_eq!(
        d.get_elements_by_class_name(d.body().unwrap(), "foo")
            .unwrap()
            .len(),
        1,
        "no doctype: quirks"
    );
    let d = Document::parse_html("<!DOCTYPE html><p id=a class=Foo>").unwrap();
    assert_eq!(
        d.get_elements_by_class_name(d.body().unwrap(), "foo")
            .unwrap()
            .len(),
        0
    );
}

// table-a: A-08
#[test]
fn get_elements_by_name_finds_html_elements() {
    let d = Document::parse_html("<input name=q><form name=q></form><svg><a name=q></a></svg>")
        .unwrap();
    assert_eq!(d.get_elements_by_name("q").unwrap().len(), 2);
}

// table-a: A-09
#[test]
fn query_selector_scope_order_and_template_exclusion() {
    let d = doc();
    profile_init();
    let sel = SelectorService::new();
    let q = |s: &str, root| sel.query_all(&d, &sel.parse(s).unwrap(), root).unwrap();
    assert_eq!(
        names(&d, &q("p", d.document_node())),
        ["b", "c"],
        "tree order, template contents excluded"
    );
    assert_eq!(names(&d, &q(".x", d.document_node())), ["a", "d"]);
    assert_eq!(
        names(&d, &q("li:nth-child(odd)", d.document_node())),
        ["l1", "l3"]
    );
    assert_eq!(names(&d, &q("div > p.y", d.document_node())), ["b"]);
    assert_eq!(names(&d, &q("[data-k]", d.document_node())), ["b"]);
    assert_eq!(names(&d, &q("p[lang=en]", d.document_node())), ["c"]);
    // scoped to an element: descendants only, never the root itself
    let a = id(&d, "a");
    assert_eq!(
        names(&d, &q("div, p", a)),
        ["b", "c"],
        "the root is not a candidate"
    );
    assert_eq!(
        names(&d, &q(":scope > span", a)),
        ["d"],
        ":scope is the query root"
    );
    let first = sel
        .query_first(&d, &sel.parse(".y").unwrap(), d.document_node())
        .unwrap();
    assert_eq!(first, Some(id(&d, "a")));
    assert_eq!(
        sel.query_first(&d, &sel.parse("table").unwrap(), d.document_node())
            .unwrap(),
        None
    );
}

// table-a: A-51
#[test]
fn matches_and_closest() {
    let d = doc();
    profile_init();
    let sel = SelectorService::new();
    let b = id(&d, "b");
    assert!(
        sel.matches(&d, &sel.parse("div > p").unwrap(), b, None)
            .unwrap()
    );
    assert!(
        !sel.matches(&d, &sel.parse("span").unwrap(), b, None)
            .unwrap()
    );
    assert_eq!(
        sel.closest(&d, &sel.parse(".x").unwrap(), b).unwrap(),
        Some(id(&d, "a"))
    );
    assert_eq!(
        sel.closest(&d, &sel.parse("p").unwrap(), b).unwrap(),
        Some(b),
        "closest is inclusive"
    );
    assert_eq!(
        sel.closest(&d, &sel.parse("table").unwrap(), b).unwrap(),
        None
    );
    let txt = d.children(b).unwrap()[0];
    assert_eq!(
        sel.matches(&d, &sel.parse("p").unwrap(), txt, None)
            .unwrap_err()
            .sub_reason,
        Some(SubReason::WrongNodeKind)
    );
}

// table-a: A-09
#[test]
fn query_root_validation_and_selector_errors() {
    let d = doc();
    profile_init();
    let sel = SelectorService::new();
    let txt = d.children(id(&d, "b")).unwrap()[0];
    let l = sel.parse("p").unwrap();
    let e = sel.query_all(&d, &l, txt).unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidArgument, Some(SubReason::WrongNodeKind))
    );
    let e = sel.parse("p[").unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidArgument, SubReason::Syntax)
    );
    let e = sel.parse("a:has(b)");
    // `:has` is PROFILE_EXCLUDED unless the gate profile is on (G-STYLE-HAS)
    #[cfg(not(feature = "gate-profiles"))]
    if let Err(e) = e {
        assert_eq!(
            (e.code, e.sub_reason),
            (Code::InvalidArgument, SubReason::ProfileExcluded)
        );
        assert_eq!(e.to_status().sub_reason, Some(SubReason::ProfileExcluded));
    }
    let e = sel.parse("::before-no-such").unwrap_err();
    assert_eq!(e.code, Code::InvalidArgument);
    // a closed document fails the query with DOCUMENT_CLOSED before anything else
    let mut d2 = doc();
    let root = d2.document_node();
    d2.request_close(true);
    assert_eq!(
        sel.query_all(&d2, &l, root).unwrap_err().code,
        Code::DocumentClosed
    );
}

// table-a: A-09
#[test]
fn selectors_see_live_mutations_ui_state_and_control_state() {
    let mut d = Document::parse_html(
        "<!DOCTYPE html><form><input id=i type=checkbox><input id=j type=checkbox checked></form>",
    )
    .unwrap();
    profile_init();
    let sel = SelectorService::new();
    let count = |d: &Document, s: &str| {
        sel.query_all(d, &sel.parse(s).unwrap(), d.document_node())
            .unwrap()
            .len()
    };
    assert_eq!(count(&d, ":checked"), 1);
    let i = id(&d, "i");
    d.control_state_writer().set_checkedness(i, true).unwrap();
    assert_eq!(
        count(&d, ":checked"),
        2,
        "stored checkedness is visible to selectors immediately"
    );
    d.toggle_attribute(i, "disabled", Some(true)).unwrap();
    assert_eq!(count(&d, ":disabled"), 1);
    let key = d.take_ui_state_key().unwrap();
    d.ui_state_writer(&key)
        .unwrap()
        .set_flags(i, ui4me_document::ui_state::UiFlags::FOCUS)
        .unwrap();
    assert_eq!(count(&d, ":focus"), 1);
}

// table-a: A-09
#[cfg(feature = "gate-profiles")]
#[test]
fn has_positive_queries_equal_style_matching_under_the_gate_profile() {
    let d = Document::parse_html("<!DOCTYPE html><div id=a><p id=b class=k><i id=c></i></p><p id=d></p></div><ul id=u><li id=l1></li><li id=l2 class=on></li></ul>").unwrap();
    profile_init();
    let sel = SelectorService::new();
    let q = |s: &str| {
        names(
            &d,
            &sel.query_all(&d, &sel.parse(s).unwrap(), d.document_node())
                .unwrap(),
        )
    };
    assert_eq!(q("p:has(> i)"), ["b"]);
    assert_eq!(q("div:has(.k)"), ["a"]);
    assert_eq!(q("ul:has(li.on)"), ["u"]);
    assert_eq!(q("li:has(+ li.on)"), ["l1"]);
    assert!(q("p:has(table)").is_empty());
    assert!(
        sel.matches(&d, &sel.parse("div:has(p > i)").unwrap(), id(&d, "a"), None)
            .unwrap()
    );
    assert_eq!(
        sel.closest(&d, &sel.parse("div:has(.k)").unwrap(), id(&d, "c"))
            .unwrap(),
        Some(id(&d, "a"))
    );
}
