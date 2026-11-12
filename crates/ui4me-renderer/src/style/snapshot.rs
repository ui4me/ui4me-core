//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! Journal entries → `ServoElementSnapshot`s (T042; AD-07). For every element changed in the
//! epoch that was styled before, the snapshot carries the **prior** attribute list (complete,
//! from the journal's first-change `prior_attributes`), the changed attribute names and the
//! prior element state (composed from the prior UI flags, control flags and attributes with the
//! same composition as the live state, adapter/state.rs). Stylo's invalidator diffs snapshot
//! against current DOM, so sibling and descendant combinators invalidate correctly.
//! Elements never styled (new nodes) get no snapshot: they have nothing to diff against.

use crate::style::adapter::state::{StateInputs, compose};
use crate::style::sidecar::SidecarStore;
use html5ever::local_name;
use std::sync::atomic::Ordering;
use style::Atom;
use style::attr::{AttrIdentifier, AttrValue};
use style::dom::OpaqueNode;
use style::selector_parser::{ServoElementSnapshot, SnapshotMap};
use style::values::GenericAtomIdent;
use ui4me_document::document::Document;
use ui4me_document::journal::EpochSegment;
use ui4me_engine_types::ids::{NodeId, OpaqueStyleKey};

/// What changed on one element, as the dependency tables of `:nth-child(... of S)` and of relative selectors
/// see it (015 L-HAS): old and new class tokens and ids, the changed attribute names, the state difference.
/// Stylo's element invalidation covers neither case (the embedder drives them; see `has.rs`).
pub(crate) struct ChangeKeys {
    pub(crate) node: NodeId,
    pub(crate) classes: Vec<Atom>,
    pub(crate) ids: Vec<Atom>,
    pub(crate) attrs: Vec<GenericAtomIdent<html5ever::LocalNameStaticSet>>,
    pub(crate) state: stylo_dom::ElementState,
}

pub(crate) struct Snapshots {
    pub(crate) nth_changes: Vec<ChangeKeys>,
    pub(crate) map: SnapshotMap,
    pub(crate) nodes: Vec<NodeId>,
    /// Styled elements whose presentational-hint attributes changed: `(node, whole subtree)`.
    /// Hints are not selector dependencies, so the invalidator cannot find them (FR-010).
    pub(crate) hint_nodes: Vec<(NodeId, bool)>,
}

pub(crate) fn build(doc: &Document, sidecars: &SidecarStore, seg: &EpochSegment) -> Snapshots {
    let mut map = SnapshotMap::new();
    let mut nodes = vec![];
    let mut hint_nodes = vec![];
    let mut nth_changes = vec![];
    let arena = doc.arena();
    for (id, prior) in seg.element_priors() {
        let Some(e) = arena.get(id).and_then(|n| n.as_element()) else {
            continue;
        }; // freed or not an element
        let Some(sc) = sidecars.get(id) else { continue };
        if !sc.has_data() {
            continue; // never styled: nothing to diff against
        }
        let prior_attrs = prior
            .attributes
            .clone()
            .unwrap_or_else(|| e.attributes().to_vec());
        let state = compose(&StateInputs {
            element: e,
            attrs: &prior_attrs,
            ui: prior.ui.unwrap_or(e.ui_flags()),
            control: prior
                .control
                .or_else(|| e.control_state().map(|c| c.flags())),
            facts: Default::default(),
        });
        nth_changes.push(change_keys(id, e, &prior));
        let attrs = prior.attributes.as_ref().map(|list| {
            list.iter()
                .map(|a| {
                    let ident = AttrIdentifier {
                        local_name: GenericAtomIdent(a.name.local.clone()),
                        name: GenericAtomIdent(a.name.local.clone()),
                        namespace: GenericAtomIdent(a.name.ns.clone()),
                        prefix: None,
                    };
                    let value = if a.name.local == local_name!("id") {
                        AttrValue::Atom(Atom::from(a.value.as_str()))
                    } else if a.name.local == local_name!("class") {
                        AttrValue::TokenList(
                            std::sync::OnceLock::from(a.value.clone()),
                            a.value.split_ascii_whitespace().map(Atom::from).collect(),
                        )
                    } else {
                        AttrValue::String(a.value.clone())
                    };
                    (ident, value)
                })
                .collect()
        });
        let is = |n: &html5ever::QualName, l: &str| n.ns == html5ever::ns!() && &*n.local == l;
        let snap = ServoElementSnapshot {
            state: Some(state),
            attrs,
            changed_attrs: prior
                .changed_attributes
                .iter()
                .map(|n| GenericAtomIdent(n.local.clone()))
                .collect(),
            class_changed: prior.changed_attributes.iter().any(|n| is(n, "class")),
            id_changed: prior.changed_attributes.iter().any(|n| is(n, "id")),
            other_attributes_changed: prior
                .changed_attributes
                .iter()
                .any(|n| !is(n, "class") && !is(n, "id")),
        };
        map.insert(
            OpaqueNode(OpaqueStyleKey::of(id).get().get() as usize),
            snap,
        );
        sc.has_snapshot.store(true, Ordering::Relaxed);
        sc.handled_snapshot.store(false, Ordering::Relaxed);
        nodes.push(id);
        if e.is_html_named("meta")
            && prior
                .changed_attributes
                .iter()
                .any(|n| is(n, "name") || is(n, "content"))
        {
            // `<meta name=color-scheme>` is a presentational hint of the root element.
            if let Some(root) = doc.document_element() {
                hint_nodes.push((root, false));
            }
        }
        // The UA-only select display-size pseudo derives its state from these
        // attributes; unlike attribute selectors, it has no Stylo dependency.
        if e.is_html_named("select")
            && prior
                .changed_attributes
                .iter()
                .any(|n| is(n, "size") || is(n, "multiple"))
        {
            hint_nodes.push((id, false));
        }
        if prior
            .changed_attributes
            .iter()
            .any(|n| n.ns == html5ever::ns!() && crate::style::hints::affects_hints(&n.local))
        {
            // `table` hints reach its cells (cellpadding, border): restyle the subtree.
            hint_nodes.push((id, matches!(&*e.name().local, "table")));
        } else if !prior.changed_attributes.is_empty()
            && sc.borrow_data().is_some_and(|d| {
                use style::selector_parser::PseudoElement as P;
                d.styles.pseudos.get(&P::Before).is_some()
                    || d.styles.pseudos.get(&P::After).is_some()
            })
        {
            // `content: attr(name)` of a generated box reads the attribute when the style is computed,
            // and it is not a selector dependency, so the invalidator cannot find it (FR-086): an element
            // that generates content is restyled when any of its attributes changes.
            hint_nodes.push((id, false));
        }
    }
    Snapshots {
        map,
        nodes,
        hint_nodes,
        nth_changes,
    }
}

/// The keys of one element's change in the epoch (also for elements that were never styled).
pub(crate) fn change_keys(
    id: NodeId,
    e: &ui4me_document::node::ElementData,
    prior: &ui4me_document::journal::ElementPrior,
) -> ChangeKeys {
    let prior_attrs = prior
        .attributes
        .clone()
        .unwrap_or_else(|| e.attributes().to_vec());
    let old_state = compose(&StateInputs {
        element: e,
        attrs: &prior_attrs,
        ui: prior.ui.unwrap_or(e.ui_flags()),
        control: prior
            .control
            .or_else(|| e.control_state().map(|c| c.flags())),
        facts: Default::default(),
    });
    let now = compose(&StateInputs {
        element: e,
        attrs: e.attributes(),
        ui: e.ui_flags(),
        control: e.control_state().map(|c| c.flags()),
        facts: Default::default(),
    });
    let tokens = |list: &[ui4me_document::node::Attribute], name: &str| -> Vec<Atom> {
        list.iter()
            .filter(|a| a.name.ns == html5ever::ns!() && &*a.name.local == name)
            .flat_map(|a| {
                a.value
                    .split_ascii_whitespace()
                    .map(Atom::from)
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let mut classes = tokens(&prior_attrs, "class");
    classes.extend(tokens(e.attributes(), "class"));
    let mut ids = tokens(&prior_attrs, "id");
    ids.extend(tokens(e.attributes(), "id"));
    let mut attrs = vec![];
    for n in &prior.changed_attributes {
        attrs.push(GenericAtomIdent(n.local.clone()));
        let lower = n.local.to_ascii_lowercase();
        if lower != *n.local {
            attrs.push(GenericAtomIdent(html5ever::LocalName::from(lower)));
        }
    }
    ChangeKeys {
        node: id,
        classes,
        ids,
        attrs,
        state: now ^ old_state,
    }
}
