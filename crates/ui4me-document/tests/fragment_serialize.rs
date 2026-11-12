//! owner: 003-document-model
//!
//! T022 / T067 (U4T-141): serialization (`serialize_` cases) and HTML-string setters
//! (`fragment_` cases): innerHTML / outerHTML / insertAdjacentHTML with context rules, atomic
//! replacement, error rules, injected failures leaving everything unchanged.

use ui4me_document::document::Document;
use ui4me_engine_types::codes_generated::{Code, SubReason};

fn by_id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap_or_else(|| panic!("no #{i}"))
}

// table-a: A-16, A-49
#[test]
fn serialize_round_trips_through_the_parser() {
    for html in [
        "<div id=a><p class=x>text &amp; more<br><img src=u alt=\"q&quot;\"></p><ul><li>1<li>2</ul></div>",
        "<table id=a><tbody><tr><td>c</td></tr></tbody></table>",
        "<div id=a><svg viewBox=\"0 0 1 1\"><path d=\"M0 0\"></path></svg><math><mi>x</mi></math></div>",
        "<div id=a><template><b>t</b></template><!--c--></div>",
    ] {
        let d = Document::parse_html(html).unwrap();
        let a = by_id(&d, "a");
        let s1 = d.outer_html(a).unwrap();
        let d2 = Document::parse_html(&s1).unwrap();
        let s2 = d2.outer_html(by_id(&d2, "a")).unwrap();
        assert_eq!(
            s1, s2,
            "serialization is a fixed point of parse+serialize for {html}"
        );
    }
}

// table-a: A-16
#[test]
fn serialize_text_modes_and_special_elements() {
    let d = Document::parse_html(
        "<div id=a>&lt;&amp;&nbsp;&gt;</div><textarea id=t>\n\nx</textarea><pre id=p>\n\nq</pre>",
    )
    .unwrap();
    assert_eq!(d.inner_html(by_id(&d, "a")).unwrap(), "&lt;&amp;&nbsp;&gt;");
    // A leading newline in pre/textarea/listing is dropped by the parser and re-added by nothing
    // (serialization does not re-add it): the data begins with "\n" after the parser dropped one.
    assert_eq!(
        d.text_content(by_id(&d, "t")).unwrap().as_deref(),
        Some("\nx")
    );
    assert_eq!(
        d.text_content(by_id(&d, "p")).unwrap().as_deref(),
        Some("\nq")
    );
}

// table-a: A-49
#[test]
fn fragment_inner_html_set_uses_the_element_as_context() {
    let mut d =
        Document::parse_html("<table><tbody id=tb><tr><td>old</td></tr></tbody></table>").unwrap();
    let tb = by_id(&d, "tb");
    let rev = d.revision();
    d.set_inner_html(tb, "<tr><td>1<td>2").unwrap();
    assert_eq!(d.revision(), rev + 1, "one atomic replace-all step");
    assert_eq!(d.inner_html(tb).unwrap(), "<tr><td>1</td><td>2</td></tr>");
    // a div context drops the table parts
    let div = d.create_element("div").unwrap();
    d.set_inner_html(div, "<tr><td>x</td></tr>").unwrap();
    assert_eq!(d.inner_html(div).unwrap(), "x");
    // select context
    let sel = d.create_element("select").unwrap();
    d.set_inner_html(sel, "<option>a<option>b<div>no").unwrap();
    assert_eq!(
        d.inner_html(sel).unwrap(),
        "<option>a</option><option>b<div>no</div></option>",
        "select context follows the current (customizable select) parsing rules"
    );
    // emptying
    d.set_inner_html(tb, "").unwrap();
    assert!(d.children(tb).unwrap().is_empty());
    let r = d.revision();
    d.set_inner_html(tb, "").unwrap();
    assert_eq!(d.revision(), r, "nothing to do is no mutation");
}

// table-a: A-49
#[test]
fn fragment_malformed_markup_and_foster_parenting() {
    let mut d = Document::parse_html("<div id=a></div>").unwrap();
    let a = by_id(&d, "a");
    d.set_inner_html(a, "<b><i>x</b>y</i><p>a<p>b").unwrap();
    assert_eq!(
        d.inner_html(a).unwrap(),
        "<b><i>x</i></b><i>y</i><p>a</p><p>b</p>"
    );
    let t = d.create_element("table").unwrap();
    d.set_inner_html(t, "<tr><td>c</td></tr>junk").unwrap();
    assert_eq!(
        d.inner_html(t).unwrap(),
        "<tbody><tr><td>c</td></tr></tbody>junk",
        "with a table context the stray text lands after the table content"
    );
}

// table-a: A-49
#[test]
fn fragment_outer_html_set_and_error_rules() {
    let mut d = Document::parse_html("<div id=a><p id=b>old</p><i id=c></i></div>").unwrap();
    let (a, b) = (by_id(&d, "a"), by_id(&d, "b"));
    d.set_outer_html(b, "<u>1</u><u>2</u>").unwrap();
    assert_eq!(d.inner_html(a).unwrap(), "<u>1</u><u>2</u><i id=\"c\"></i>");
    assert!(
        d.parent(b).unwrap().is_none(),
        "the replaced element is detached and stays valid"
    );
    // no parent: no effect
    d.set_outer_html(b, "<x></x>").unwrap();
    // Document parent: NO_MODIFICATION
    let html = d.document_element().unwrap();
    let e = d.set_outer_html(html, "<html></html>").unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidHierarchy, Some(SubReason::NoModification))
    );
    // empty markup removes the element
    let c = by_id(&d, "c");
    d.set_outer_html(c, "").unwrap();
    assert!(d.element_by_id("c").is_none());
}

// table-a: A-50
#[test]
fn fragment_insert_adjacent_html_positions() {
    let mut d = Document::parse_html("<div id=a><p id=b></p></div>").unwrap();
    let (a, b) = (by_id(&d, "a"), by_id(&d, "b"));
    d.insert_adjacent_html(b, "beforebegin", "<i>1</i>")
        .unwrap();
    d.insert_adjacent_html(b, "afterbegin", "<i>2</i>").unwrap();
    d.insert_adjacent_html(b, "beforeend", "<i>3</i>").unwrap();
    d.insert_adjacent_html(b, "afterend", "<i>4</i>").unwrap();
    assert_eq!(
        d.inner_html(a).unwrap(),
        "<i>1</i><p id=\"b\"><i>2</i><i>3</i></p><i>4</i>"
    );
    let e = d.insert_adjacent_html(b, "nowhere", "x").unwrap_err();
    assert_eq!(
        (e.code, e.sub_reason),
        (Code::InvalidArgument, Some(SubReason::Syntax))
    );
    let html = d.document_element().unwrap();
    let e = d
        .insert_adjacent_html(html, "beforebegin", "<p>")
        .unwrap_err();
    assert_eq!(
        e.sub_reason,
        Some(SubReason::NoModification),
        "Document parent"
    );
    // the html element as context uses a body context
    d.insert_adjacent_html(html, "afterbegin", "<p id=z>x</p>")
        .unwrap();
    assert!(d.element_by_id("z").is_some());
    // a detached element with no parent: beforebegin / afterend are no effects
    let lone = d.create_element("lone").unwrap();
    d.insert_adjacent_html(lone, "afterend", "<p>").unwrap();
}

// table-a: A-49, A-52
#[test]
fn fragment_template_set_inner_html_replaces_the_contents() {
    let mut d = Document::parse_html("<template id=t><p>old</p></template>").unwrap();
    let t = by_id(&d, "t");
    d.set_inner_html(t, "<tr><td>x</td></tr><b>y</b>").unwrap();
    assert!(
        d.children(t).unwrap().is_empty(),
        "the element itself gets no children"
    );
    assert_eq!(
        d.inner_html(t).unwrap(),
        "<tr><td>x</td></tr><b>y</b>",
        "template context accepts table parts"
    );
    assert!(d.element_by_id("x").is_none());
}

// table-a: A-49
#[test]
fn fragment_failures_leave_document_journal_and_revision_unchanged() {
    let d = Document::parse_html("<div id=a><p>keep</p></div>").unwrap();
    let a = by_id(&d, "a");
    let live = d.arena().live_count() + 2;
    let mut d = d.with_limits(ui4me_document::limits::DomLimits {
        nodes: live as u64,
        ..Default::default()
    });
    let rev = d.revision();
    let journal = d.journal().since(0).entries.len();
    let before = d.inner_html(a).unwrap();
    let e = d
        .set_inner_html(a, "<p>1</p><p>2</p><p>3</p><p>4</p>")
        .unwrap_err();
    assert_eq!(e.code, Code::SizeLimit);
    assert_eq!(d.revision(), rev);
    assert_eq!(d.journal().since(0).entries.len(), journal);
    assert_eq!(d.inner_html(a).unwrap(), before);
    assert_eq!(d.arena().live_count(), live - 2, "staging nodes were freed");
}

#[cfg(feature = "test-sinks")]
// table-a: A-49
#[test]
fn fragment_injected_allocation_failure_is_out_of_memory_with_no_effect() {
    let mut d = Document::parse_html("<div id=a>x</div>").unwrap();
    let a = by_id(&d, "a");
    let rev = d.revision();
    d.inject_allocation_failure(1);
    let e = d.set_inner_html(a, "<b>y</b>").unwrap_err();
    assert_eq!(e.code, Code::OutOfMemory);
    assert_eq!(d.revision(), rev);
    assert_eq!(d.inner_html(a).unwrap(), "x");
}
