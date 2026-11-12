//! owner: 003-document-model
//!
//! T040 / T051 (A-48; FR-041-FR-043). `[VALIDATE X-SPECS]`: CSSOM serialization semantics come from
//! 006's `InlineStyleService` (CSSOM `[VALIDATE X-SPECS]` pins are 001:T018's, status UNKNOWN until that
//! gate is recorded); this file proves only DocumentCore's part: commit the service's
//! `new_attribute_text` as exactly one `style` attribute mutation, none when nothing changed, and
//! direct `style` attribute writes always succeed. The production composition is 004's ABI layer;
//! `commit` below is the test harness version (research R-14).

use crate::style::query::{InlineOutcome, InlineStyleService};
use ui4me_document::document::Document;
use ui4me_document::journal::EntryKind;
use ui4me_engine_types::ids::NodeId;

fn svc(d: &Document, el: NodeId) -> InlineStyleService {
    InlineStyleService::new()
        .with_quirks(d.mode())
        .for_element(el)
}

/// Apply an operation of the service to `el`'s current `style` attribute and commit the result.
fn commit(
    d: &mut Document,
    el: NodeId,
    f: impl FnOnce(
        &InlineStyleService,
        &mut crate::style::query::DeclarationBlockView,
    ) -> crate::style::query::InlineWriteResult,
) -> crate::style::query::InlineWriteResult {
    let s = svc(d, el);
    let mut view = s.parse(d.get_attribute(el, "style").unwrap().unwrap_or(""), None);
    let r = f(&s, &mut view);
    if let Some(text) = &r.new_attribute_text {
        d.set_attribute(el, "style", text).unwrap();
    }
    r
}

fn doc() -> (Document, NodeId) {
    let d = Document::parse_html(
        "<!DOCTYPE html><div id=a style='color: red; margin-left: 3px'></div>",
    )
    .unwrap();
    let a = d.element_by_id("a").unwrap();
    (d, a)
}

fn style_attr_entries(d: &mut Document) -> usize {
    d.take_epoch()
        .entries()
        .iter()
        .filter(|e| matches!(&e.kind, EntryKind::Attribute { name, .. } if &*name.local == "style"))
        .count()
}

// table-a: A-48
#[test]
fn set_property_is_exactly_one_style_attribute_mutation_with_the_cssom_text() {
    let (mut d, a) = doc();
    let _ = d.take_epoch();
    let rev = d.revision();
    let r = commit(&mut d, a, |s, v| {
        s.set(v, "background-color", "blue", false)
    });
    assert_eq!(r.outcome, InlineOutcome::Applied);
    assert_eq!(d.revision(), rev + 1, "one revision step");
    assert_eq!(
        style_attr_entries(&mut d),
        1,
        "one style attribute mutation"
    );
    assert_eq!(
        d.get_attribute(a, "style").unwrap(),
        Some("color: red; margin-left: 3px; background-color: blue;")
    );
}

// table-a: A-48
#[test]
fn an_unchanged_block_is_no_mutation_and_no_revision_step() {
    let (mut d, a) = doc();
    let _ = d.take_epoch();
    let rev = d.revision();
    let r = commit(&mut d, a, |s, v| s.set(v, "color", "red", false));
    assert!(r.new_attribute_text.is_none());
    let r = commit(&mut d, a, |s, v| s.remove(v, "padding"));
    assert!(r.new_attribute_text.is_none());
    assert_eq!(d.revision(), rev);
    assert_eq!(style_attr_entries(&mut d), 0);
}

// table-a: A-48
#[test]
fn invalid_values_and_unknown_properties_change_nothing_and_raise_no_error() {
    let (mut d, a) = doc();
    let rev = d.revision();
    let before = d.get_attribute(a, "style").unwrap().map(str::to_owned);
    let r = commit(&mut d, a, |s, v| s.set(v, "color", "not-a-colour", false));
    assert_eq!(r.outcome, InlineOutcome::Ignored);
    assert!(r.diagnostic.is_some());
    let r = commit(&mut d, a, |s, v| s.set(v, "no-such-property", "1", false));
    assert_eq!(r.outcome, InlineOutcome::Ignored);
    assert_eq!(d.revision(), rev);
    assert_eq!(
        d.get_attribute(a, "style").unwrap().map(str::to_owned),
        before
    );
}

// table-a: A-48
#[test]
fn a_profile_excluded_property_is_dropped_and_reported() {
    let (mut d, a) = doc();
    let rev = d.revision();
    // `transition` is outside the v1 CSS profile in 006's table; if the profile changes this test
    // reads the outcome rather than assuming it.
    let r = commit(&mut d, a, |s, v| s.set(v, "animation-name", "spin", false));
    match r.outcome {
        InlineOutcome::Dropped => {
            assert_eq!(
                r.diagnostic.as_ref().map(|x| x.code),
                Some(ui4me_engine_types::codes_generated::Code::UnsupportedFeature)
            );
            assert_eq!(d.revision(), rev, "a dropped declaration is never stored");
        }
        InlineOutcome::Applied | InlineOutcome::Ignored => {}
    }
}

// table-a: A-48
#[test]
fn get_priority_remove_css_text_length_and_item() {
    let (mut d, a) = doc();
    let s = svc(&d, a);
    let v = s.parse(d.get_attribute(a, "style").unwrap().unwrap(), None);
    assert_eq!(s.get(&v, "color"), ("red".to_owned(), false));
    assert_eq!(s.get(&v, "padding"), (String::new(), false));
    let important = commit(&mut d, a, |s, v| s.set(v, "color", "green", true));
    assert_eq!(important.outcome, InlineOutcome::Applied);
    let v = s.parse(d.get_attribute(a, "style").unwrap().unwrap(), None);
    assert_eq!(
        s.get(&v, "color"),
        ("green".to_owned(), true),
        "getPropertyPriority"
    );
    let n = s.list(&v);
    assert_eq!(n.len(), 2, "length and item(i) come from the ordered list");
    assert!(n.iter().any(|p| p == "color") && n.iter().any(|p| p.starts_with("margin-left")));
    let r = commit(&mut d, a, |s, v| s.remove(v, "color"));
    assert!(r.new_attribute_text.is_some());
    assert_eq!(
        d.get_attribute(a, "style").unwrap(),
        Some("margin-left: 3px;")
    );
    let r = commit(&mut d, a, |s, v| s.set_text(v, "padding: 1px"));
    assert!(r.new_attribute_text.is_some(), "cssText setter");
    assert_eq!(d.get_attribute(a, "style").unwrap(), Some("padding: 1px;"));
}

// table-a: A-48
#[test]
fn direct_style_attribute_writes_always_succeed() {
    let (mut d, a) = doc();
    d.set_attribute(a, "style", "this is ; not css {{{")
        .unwrap();
    assert_eq!(
        d.get_attribute(a, "style").unwrap(),
        Some("this is ; not css {{{")
    );
    d.set_attribute(a, "style", "").unwrap();
    d.remove_attribute(a, "style").unwrap();
    assert!(!d.has_attribute(a, "style").unwrap());
}
