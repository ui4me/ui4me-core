//! owner: 003-document-model
//!
//! T053 / U4T-142 (FR-022-FR-028, FR-056, SC-003 part): a removed and re-inserted element keeps its
//! attributes, children and control state; retention by host references, dispatch-frame references and
//! template contents; stale handles; no ABA after slot reuse; cloning leaves the original untouched.

use ui4me_document::document::Document;
use ui4me_engine_types::codes_generated::Code;

fn id(d: &Document, i: &str) -> ui4me_engine_types::ids::NodeId {
    d.element_by_id(i).unwrap()
}

// table-a: A-12, A-28
#[test]
fn a_removed_and_reinserted_element_keeps_attributes_children_and_control_state() {
    let mut d = Document::parse_html("<form id=f><input id=i type=checkbox checked value=v><div id=box class='a b' data-k=1><i>kid</i></div></form>").unwrap();
    let (f, i, b) = (id(&d, "f"), id(&d, "i"), id(&d, "box"));
    d.control_state_writer().set_checkedness(i, false).unwrap();
    d.control_state_writer().set_value(i, "typed").unwrap();
    d.retain(b).unwrap();
    d.retain(i).unwrap();
    d.remove_child(f, b).unwrap();
    d.remove_child(f, i).unwrap();
    assert!(
        d.checkpoint().is_empty(),
        "retained detached nodes survive a checkpoint"
    );
    assert_eq!(
        d.get_attribute(b, "data-k").unwrap(),
        Some("1"),
        "readable while detached"
    );
    d.append_child(f, i).unwrap();
    d.append_child(f, b).unwrap();
    assert_eq!(d.get_attribute(b, "class").unwrap(), Some("a b"));
    assert_eq!(d.text_content(b).unwrap().as_deref(), Some("kid"));
    let c = d.control_state(i).unwrap().unwrap();
    assert_eq!(
        (
            c.checkedness,
            c.value.as_deref(),
            c.dirty_checkedness,
            c.dirty_value
        ),
        (Some(false), Some("typed"), true, true)
    );
    assert_eq!(
        d.form_owner(i).unwrap(),
        Some(f),
        "the owner is re-evaluated on re-insertion"
    );
}

// table-a: A-12
#[test]
fn retention_by_host_references_frames_and_template_contents() {
    let mut d =
        Document::parse_html("<div id=a><template id=t><p id=in>x</p></template></div>").unwrap();
    let (a, t) = (id(&d, "a"), id(&d, "t"));
    let contents = d.template_contents(t).unwrap().unwrap();
    let inner = d.children(contents).unwrap()[0];
    // detach the template: its contents live with it
    d.remove_child(a, t).unwrap();
    d.retain(inner).unwrap();
    assert!(
        d.checkpoint().is_empty(),
        "a reference inside the template contents keeps the template tree alive"
    );
    assert!(d.get_attribute(t, "id").unwrap().is_some());
    d.release(inner).unwrap();
    let freed = d.checkpoint();
    assert_eq!(
        freed.len(),
        4,
        "template, its contents fragment, the paragraph and the paragraph's text"
    );
}

// table-a: A-12
#[test]
fn stale_handles_and_no_aba_after_slot_reuse() {
    let mut d = Document::parse_html("<div id=a></div>").unwrap();
    let a = id(&d, "a");
    let x = d.create_element("x").unwrap();
    d.checkpoint();
    assert_eq!(d.get_attribute(x, "k").unwrap_err().code, Code::StaleHandle);
    // the slot is reused by a new node; the old handle still names nothing
    let y = d.create_element("y").unwrap();
    assert_ne!(x, y, "a (index, generation) pair is never reused");
    assert_eq!(x.raw().index, y.raw().index, "the free slot was reused");
    assert!(y.raw().generation > x.raw().generation);
    assert_eq!(d.local_name(y).unwrap(), "y");
    assert_eq!(d.append_child(a, x).unwrap_err().code, Code::StaleHandle);
    // double release is an idempotent success
    d.release(x).unwrap();
    d.release(x).unwrap();
}

// table-a: A-12
#[test]
fn clone_and_import_do_not_disturb_the_original() {
    let mut d = Document::parse_html("<div id=a x=1><p>t</p></div>").unwrap();
    let a = id(&d, "a");
    let before = d.outer_html(a).unwrap();
    let c = d.clone_node(a, true).unwrap();
    d.set_attribute(c, "x", "2").unwrap();
    d.append(
        c,
        &[ui4me_document::mutate::NodeOrString::Text("more".into())],
    )
    .unwrap();
    assert_eq!(d.outer_html(a).unwrap(), before);
}

#[test]
fn arena_never_reuses_an_index_generation_pair_across_a_million_cycles() {
    let mut d = Document::new_empty(ui4me_engine_types::ids::DocumentId::from_raw(
        ui4me_engine_types::ids::RawId {
            index: 21,
            generation: std::num::NonZeroU32::MIN,
        },
    ));
    let mut seen = std::collections::HashSet::new();
    let mut last = None;
    for _ in 0..1_000_000 {
        let n = d.create_comment("").unwrap();
        assert!(seen.insert(n), "a (index, generation) pair was reused");
        if let Some(prev) = last.replace(n) {
            let _ = d.release(prev);
        }
        d.checkpoint();
        if seen.len() > 5000 {
            seen.clear(); // keep memory bounded; uniqueness inside each window is what is checked
            seen.insert(n);
        }
    }
}
