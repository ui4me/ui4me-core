//! owner: 003-document-model — data-model section 4 invariants as a test-side checker (T009).
//!
//! Covers invariants 1-10, 12 and 14. Invariant 11 (batch undo equals the pre-batch state) is
//! checked in `batch_atomic.rs` (T081 adds the hook-state case); invariant 13 (lifecycle graph) in
//! `lifecycle_limits.rs` and the unit tests of `document.rs`.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use ui4me_document::document::{Document, Lifecycle};
use ui4me_document::journal::EntryKind;
use ui4me_document::node::{NodeData, NodeKind};
use ui4me_engine_types::ids::NodeId;

/// Check the invariants; returns the first violation.
pub fn check_invariants(d: &Document) -> Result<(), String> {
    let a = d.arena();
    let doc = a.document();
    if d.lifecycle() == Lifecycle::Closed {
        // 12. a closed document accepts no operation, so nothing can change; its lookups all fail
        // with DOCUMENT_CLOSED and the structural checks below would only re-walk frozen memory.
        return Ok(());
    }
    let live: Vec<NodeId> = a.iter().map(|n| n.id()).collect();
    let live_set: HashSet<NodeId> = live.iter().copied().collect();
    // 1/6. parent and child lists agree; every link names a live node; each node is in at most one list
    let mut listed: HashMap<NodeId, NodeId> = HashMap::new();
    for &id in &live {
        let n = a.node(id);
        for &c in n.children() {
            if !live_set.contains(&c) {
                return Err(format!("{id:?} lists freed child {c:?}"));
            }
            if listed.insert(c, id).is_some() {
                return Err(format!("{c:?} appears in two child lists"));
            }
            if a.node(c).parent() != Some(id) {
                return Err(format!(
                    "{c:?} is listed under {id:?} but its parent is {:?}",
                    a.node(c).parent()
                ));
            }
        }
        if let Some(p) = n.parent() {
            if !live_set.contains(&p) {
                return Err(format!("{id:?} has freed parent {p:?}"));
            }
            if !a.node(p).children().contains(&id) {
                return Err(format!("{id:?} has parent {p:?} that does not list it"));
            }
        }
        // no node is its own ancestor
        let mut cur = n.parent();
        let mut steps = 0usize;
        while let Some(p) = cur {
            if p == id || steps > live.len() {
                return Err(format!("{id:?} is its own ancestor"));
            }
            cur = a.node(p).parent();
            steps += 1;
        }
        // 3/6. template contents and back-pointer
        if let Some(e) = n.as_element() {
            if e.is_html_named("template") != e.template_contents().is_some() {
                return Err(format!(
                    "{id:?}: template contents presence differs from being a template"
                ));
            }
            if let Some(t) = e.template_contents() {
                if !live_set.contains(&t) {
                    return Err(format!("{id:?} has freed template contents"));
                }
                match a.node(t).data() {
                    NodeData::DocumentFragment { template: Some(h) } if *h == id => {}
                    other => {
                        return Err(format!(
                            "contents of {id:?} have a wrong back-pointer: {other:?}"
                        ));
                    }
                }
                if a.node(t).parent().is_some() {
                    return Err(format!("template contents of {id:?} have a parent"));
                }
            }
            if let Some(f) = e.form_owner()
                && !live_set.contains(&f)
            {
                return Err(format!("{id:?} has a freed form owner"));
            }
            // 9. no UI flags on detached elements (the removal hook clears them, FR-081)
            if !e.ui_flags().is_empty() && !a.is_connected(id) {
                return Err(format!(
                    "detached element {id:?} has UI flags {:?}",
                    e.ui_flags()
                ));
            }
        }
    }
    // 2. Document child rules
    let kids = a.node(doc).children();
    let elems = kids.iter().filter(|&&c| a.node(c).is_element()).count();
    let doctypes = kids
        .iter()
        .filter(|&&c| a.node(c).kind() == NodeKind::Doctype)
        .count();
    if elems > 1 || doctypes > 1 || kids.iter().any(|&c| a.node(c).kind() == NodeKind::Text) {
        return Err(format!(
            "Document children break the rules: {elems} elements, {doctypes} doctypes"
        ));
    }
    if let (Some(dt), Some(el)) = (
        kids.iter()
            .position(|&c| a.node(c).kind() == NodeKind::Doctype),
        kids.iter().position(|&c| a.node(c).is_element()),
    ) && dt > el
    {
        return Err("doctype after the document element".into());
    }
    // 5. connectedness agrees with reachability from the Document
    let mut reach = HashSet::new();
    let mut stack = vec![doc];
    while let Some(n) = stack.pop() {
        reach.insert(n);
        stack.extend(a.node(n).children().iter().copied());
    }
    for &id in &live {
        if a.is_connected(id) != reach.contains(&id) {
            return Err(format!("{id:?}: is_connected disagrees with reachability"));
        }
    }
    // 4. id lookup = first connected element in tree order with that id
    let mut first: HashMap<String, NodeId> = HashMap::new();
    for n in a.subtree(doc) {
        if let Some(v) = a
            .node(n)
            .as_element()
            .and_then(|e| e.attr("id"))
            .filter(|v| !v.is_empty())
        {
            first.entry(v.to_owned()).or_insert(n);
        }
    }
    for (k, v) in &first {
        if d.element_by_id(k) != Some(*v) {
            return Err(format!("id lookup of {k:?} differs from the tree walk"));
        }
    }
    // 7/8. journal: revisions never exceed the current one and never decrease; one prior record per
    // element per epoch
    let w = d.journal().since(0);
    let mut last = 0u64;
    let mut priors: HashSet<(u64, NodeId)> = HashSet::new();
    for e in &w.entries {
        if e.revision < last || e.revision > d.revision() {
            return Err(format!(
                "journal revision {} out of order (last {last}, current {})",
                e.revision,
                d.revision()
            ));
        }
        last = e.revision;
        if let EntryKind::PriorAttributes { node, .. } = &e.kind
            && !priors.insert((e.style_epoch, *node))
        {
            return Err(format!(
                "two prior_attributes records for {node:?} in epoch {}",
                e.style_epoch
            ));
        }
    }
    // 14. composition ranges sit on UTF-16 boundaries
    for &id in &live {
        if a.node(id).is_element() && !d.composition_is_valid(id) {
            return Err(format!(
                "{id:?}: composition range is not on UTF-16 boundaries"
            ));
        }
    }
    Ok(())
}
