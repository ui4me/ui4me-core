//! owner: 003-document-model
//!
//! T009 (data-model section 4, invariants 1-10, 12, 14): a proptest driver applies random public
//! operations (including ones that must fail) and `check_invariants` runs after every step. The
//! stronger lockstep comparison against a reference model is `mutation_oracle.rs`.

mod common;

use proptest::prelude::*;
use ui4me_document::document::Document;
use ui4me_document::mutate::NodeOrString;
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Debug)]
enum Op {
    Create(u8),
    Append(usize, usize),
    InsertBefore(usize, usize, usize),
    Remove(usize),
    Replace(usize, usize, usize),
    SetAttr(usize, u8),
    Class(usize, u8),
    Text(usize, u8),
    Html(usize, u8),
    Clone(usize),
    Normalize(usize),
    Retain(usize),
    Release(usize),
    Checkpoint,
    Close,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0u8..6).prop_map(Op::Create),
        6 => (0usize..40, 0usize..40).prop_map(|(a, b)| Op::Append(a, b)),
        4 => (0usize..40, 0usize..40, 0usize..40).prop_map(|(a, b, c)| Op::InsertBefore(a, b, c)),
        3 => (0usize..40).prop_map(Op::Remove),
        3 => (0usize..40, 0usize..40, 0usize..40).prop_map(|(a, b, c)| Op::Replace(a, b, c)),
        3 => (0usize..40, 0u8..4).prop_map(|(a, b)| Op::SetAttr(a, b)),
        2 => (0usize..40, 0u8..3).prop_map(|(a, b)| Op::Class(a, b)),
        2 => (0usize..40, 0u8..3).prop_map(|(a, b)| Op::Text(a, b)),
        2 => (0usize..40, 0u8..3).prop_map(|(a, b)| Op::Html(a, b)),
        1 => (0usize..40).prop_map(Op::Clone),
        1 => (0usize..40).prop_map(Op::Normalize),
        2 => (0usize..40).prop_map(Op::Retain),
        2 => (0usize..40).prop_map(Op::Release),
        2 => Just(Op::Checkpoint),
        1 => Just(Op::Close),
    ]
}

fn run(ops: &[Op]) -> Result<(), String> {
    let mut d = Document::parse_html("<!DOCTYPE html><body><template id=t><i>x</i></template><form id=f><input id=in></form></body>").unwrap();
    let mut nodes: Vec<NodeId> = vec![
        d.body().unwrap(),
        d.element_by_id("t").unwrap(),
        d.element_by_id("f").unwrap(),
        d.element_by_id("in").unwrap(),
    ];
    let key = d.take_ui_state_key().unwrap();
    d.hooks_mut().set_on_removed(Box::new(|w, r| {
        for n in w.flagged_in_subtree(r.root, ui4me_document::ui_state::UiFlags::FOCUS) {
            let _ = w.set_flags(n, ui4me_document::ui_state::UiFlags::empty());
        }
    }));
    let at = |nodes: &Vec<NodeId>, i: usize| nodes[i % nodes.len()];
    for (step, o) in ops.iter().enumerate() {
        match o.clone() {
            Op::Create(k) => {
                let r = match k {
                    0 => d.create_element("div"),
                    1 => d.create_element("form"),
                    2 => d.create_element("input"),
                    3 => d.create_text_node("t"),
                    4 => d.create_document_fragment(),
                    _ => d.create_element("template"),
                };
                if let Ok(n) = r {
                    nodes.push(n);
                }
            }
            Op::Append(a, b) => {
                let _ = d.append_child(at(&nodes, a), at(&nodes, b));
            }
            Op::InsertBefore(a, b, c) => {
                let _ = d.insert_before(at(&nodes, a), at(&nodes, b), Some(at(&nodes, c)));
            }
            Op::Remove(a) => {
                let _ = d.remove(at(&nodes, a));
            }
            Op::Replace(a, b, c) => {
                let _ = d.replace_child(at(&nodes, a), at(&nodes, b), at(&nodes, c));
            }
            Op::SetAttr(a, k) => {
                let n = at(&nodes, a);
                let _ = match k {
                    0 => d.set_attribute(n, "id", "dup"),
                    1 => d.set_attribute(n, "form", "f"),
                    2 => d.set_attribute(n, "value", "v"),
                    _ => d.remove_attribute(n, "id"),
                };
                let _ = d
                    .ui_state_writer(&key)
                    .map(|mut w| w.set_flags(n, ui4me_document::ui_state::UiFlags::FOCUS));
            }
            Op::Class(a, k) => {
                let _ = d.class_list_toggle(at(&nodes, a), ["a", "b", "c"][k as usize], None);
            }
            Op::Text(a, k) => {
                let _ = d.set_text_content(at(&nodes, a), ["", "x", "yz"][k as usize]);
            }
            Op::Html(a, k) => {
                let _ = d.set_inner_html(
                    at(&nodes, a),
                    [
                        "",
                        "<p>1<p>2",
                        "<tr><td>c</td></tr><template><b>x</b></template><input form=f>",
                    ][k as usize],
                );
            }
            Op::Clone(a) => {
                if let Ok(n) = d.clone_node(at(&nodes, a), true) {
                    nodes.push(n);
                }
            }
            Op::Normalize(a) => {
                let _ = d.normalize(at(&nodes, a));
            }
            Op::Retain(a) => {
                let _ = d.retain(at(&nodes, a));
            }
            Op::Release(a) => {
                let _ = d.release(at(&nodes, a));
            }
            Op::Checkpoint => {
                d.checkpoint();
            }
            Op::Close => {
                if step > 30 {
                    d.request_close(false);
                }
            }
        }
        common::invariants::check_invariants(&d).map_err(|e| format!("step {step} {o:?}: {e}"))?;
        let _ = d.append(at(&nodes, step), &[NodeOrString::Text(String::new())]);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    // table-a: A-27
    #[test]
    fn random_public_operations_keep_every_invariant(ops in proptest::collection::vec(op(), 1..80)) {
        if let Err(e) = run(&ops) {
            return Err(TestCaseError::fail(e));
        }
    }
}

// table-a: A-12
#[test]
fn arena_slots_are_not_reused_with_the_same_generation_and_stale_ids_stay_stale() {
    let mut d = Document::parse_html("<p>x</p>").unwrap();
    let mut prev = vec![];
    for round in 0..50 {
        let n = d.create_comment("c").unwrap();
        prev.push(n);
        d.checkpoint();
        for &p in &prev[..prev.len() - 1] {
            assert!(
                d.arena().get(p).is_none(),
                "round {round}: an old id must stay stale after its slot was reused"
            );
        }
    }
}
