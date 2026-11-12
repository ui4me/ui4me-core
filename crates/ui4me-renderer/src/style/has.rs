//! owner: 015-core-completion L-HAS (task HAS-1; 006 T093) — the `:has()` invalidation driver.
//!
//! Stylo implements relative-selector *matching* and a relative-selector invalidator, but the
//! embedder must drive it: nothing in `process_style` walks from a changed element to the anchors
//! (ancestors, earlier siblings) whose `:has()` result may have changed. ui4me batches DOM changes
//! per epoch and styles against the *final* tree, so Gecko's per-mutation protocol
//! (`DomMutationOperation` with a `SiblingTraversalMap`, replayed against the live tree) cannot be
//! used. This driver keeps Stylo's *dependency analysis* and replaces its tree walk:
//!
//! 1. **Which directions?** Stylo records, per cascade data, a relative-selector invalidation map:
//!    class / id / attribute / state / type / `*` / structural-state keys, each with the
//!    dependencies of the `:has()` selectors that mention them, and each dependency carries a
//!    [`RelativeDependencyInvalidationKind`] (`Parent`, `Ancestors`, `PrevSibling`, `EarlierSibling`,
//!    `AncestorPrevSibling`, `AncestorEarlierSibling`). A change whose keys hit no dependency
//!    invalidates nothing; otherwise the union of the hit kinds says where anchors can be.
//! 2. **Which anchors?** The candidates in those directions are walked on the final tree and pruned
//!    by the selector flags Stylo records while matching (through the adapter's
//!    `apply_selector_flags`, kept on the sidecar): `ANCHORS_RELATIVE_SELECTOR` (the `:has()` is in
//!    the subject compound of a selector the element was matched against: restyle the element) and
//!    `ANCHORS_RELATIVE_SELECTOR_NON_SUBJECT` (the `:has()` is in an earlier compound, as in
//!    `.a:has(.x) ~ .b .c`: later siblings and descendants depend on it, so the parent's subtree is
//!    restyled: the conservative replacement of Stylo's finer dependency continuation). An element
//!    that was never matched against a `:has()` selector carries neither flag and is no candidate:
//!    its style cannot depend on `:has()` until it is restyled for another reason, which evaluates
//!    (and flags) it. Flags only accumulate; a fresh sidecar (slot reuse) starts empty.
//!
//! A child-list change at parent `P` is treated as a virtual child of `P` (the removed nodes are
//! reached through the journal entry, which is recorded at removal time, so no pre-removal tree is
//! needed). `P`'s children are covered by the parent-subtree restyle the traversal performs for
//! every child-list change; structural pseudo-classes (`:empty`, `:first-child`, `:nth-child`) and
//! text changes are keyed as the structural dependencies of `P` and its children.
//!
//! When an `@scope` root/limit or the `S` of `:nth-child(of S)` contains `:has()` the anchor also
//! decides the match of other elements, so every anchor restyles its parent's subtree
//! ([`has_decides_other_elements`]).

use crate::style::sidecar::SidecarStore;
use selectors::matching::ElementSelectorFlags;
use std::collections::{BTreeMap, BTreeSet};
use style::Atom;
use style::context::QuirksMode;
use style::invalidation::element::invalidation_map::{
    Dependency, DependencyInvalidationKind, RelativeDependencyInvalidationKind as K,
};
use style::selector_map::SelectorMap;
use style::values::GenericAtomIdent;
use stylo_dom::ElementState;
use ui4me_document::document::Document;
use ui4me_document::journal::{EntryKind, EpochSegment};
use ui4me_engine_types::ids::NodeId;

type LocalName = GenericAtomIdent<html5ever::LocalNameStaticSet>;

/// How strongly an anchor must be restyled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AnchorHint {
    /// The anchor's own style may change.
    SelfOnly,
    /// The anchor's later siblings and descendants may change too: restyle this subtree.
    Subtree,
}

/// The directions in which relative-selector anchors can sit (a set of `RelativeDependencyInvalidationKind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Kinds(u8);

impl Kinds {
    const PARENT: u8 = 1;
    const ANCESTORS: u8 = 2;
    const PREV: u8 = 4;
    const EARLIER: u8 = 8;
    const ANC_PREV: u8 = 16;
    const ANC_EARLIER: u8 = 32;
    pub(crate) const ALL: Kinds = Kinds(63);

    fn add(&mut self, k: K) {
        self.0 |= match k {
            K::Parent => Self::PARENT,
            K::Ancestors => Self::ANCESTORS,
            K::PrevSibling => Self::PREV,
            K::EarlierSibling => Self::EARLIER,
            K::AncestorPrevSibling => Self::ANC_PREV,
            K::AncestorEarlierSibling => Self::ANC_EARLIER,
        };
    }
    fn add_deps(&mut self, deps: &[Dependency]) {
        for d in deps {
            match d.invalidation_kind() {
                DependencyInvalidationKind::Relative(k) => self.add(k),
                _ => self.0 = Self::ALL.0, // not expected in this map: be conservative
            }
        }
    }
    fn has(self, bits: u8) -> bool {
        self.0 & bits != 0
    }
    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }
    fn union(self, o: Kinds) -> Kinds {
        Kinds(self.0 | o.0)
    }
}

/// What a change presents to the dependency tables.
pub(crate) struct Query {
    pub(crate) classes: Vec<Atom>,
    pub(crate) ids: Vec<Atom>,
    pub(crate) attrs: Vec<LocalName>,
    pub(crate) local_names: Vec<LocalName>,
    pub(crate) state: ElementState,
    /// A DOM mutation (insertion or removal): `*` and type dependencies count.
    pub(crate) mutation: bool,
}

impl Default for Query {
    fn default() -> Self {
        Query {
            classes: vec![],
            ids: vec![],
            attrs: vec![],
            local_names: vec![],
            state: ElementState::empty(),
            mutation: false,
        }
    }
}

fn each_entry<T: 'static>(m: &SelectorMap<T>, f: &mut dyn FnMut(&T)) {
    m.root
        .iter()
        .chain(&m.rare_pseudo_classes)
        .chain(&m.other)
        .for_each(&mut *f);
    for v in m.id_hash.iter().chain(m.class_hash.iter()).map(|(_, v)| v) {
        v.iter().for_each(&mut *f);
    }
    for v in m.local_name_hash.values().chain(m.attribute_hash.values()) {
        v.iter().for_each(&mut *f);
    }
    for v in m.namespace_hash.values() {
        v.iter().for_each(&mut *f);
    }
}

/// The directions of the relative-selector dependencies the query hits, over every cascade data that applies to `root`.
pub(crate) fn kinds_for<E: style::dom::TElement>(
    stylist: &style::stylist::Stylist,
    root: E,
    q: &Query,
) -> Kinds {
    let quirks: QuirksMode = stylist.quirks_mode();
    let mut kinds = Kinds::default();
    stylist.any_applicable_rule_data(root, |d| {
        let extra = d.relative_invalidation_map_attributes();
        if !extra.used {
            return false;
        }
        let m = d.relative_selector_invalidation_map();
        for c in &q.classes {
            if let Some(deps) = m.class_to_selector.get(c, quirks) {
                kinds.add_deps(deps);
            }
        }
        for i in &q.ids {
            if let Some(deps) = m.id_to_selector.get(i, quirks) {
                kinds.add_deps(deps);
            }
        }
        for a in &q.attrs {
            if let Some(deps) = m.other_attribute_affecting_selectors.get(a) {
                kinds.add_deps(deps);
            }
        }
        if !q.state.is_empty() {
            each_entry(&m.state_affecting_selectors, &mut |e| {
                if e.state.intersects(q.state) {
                    kinds.add_deps(std::slice::from_ref(&e.dep));
                }
            });
        }
        if q.mutation {
            for l in &q.local_names {
                if let Some(deps) = extra.type_to_selector.get(l) {
                    kinds.add_deps(deps);
                }
            }
            kinds.add_deps(&extra.any_to_selector);
        }
        false
    });
    kinds
}

/// Whether any cascade data that applies to `root` has relative-selector (`:has()`) dependencies.
pub(crate) fn stylist_uses_has<E: style::dom::TElement>(
    stylist: &style::stylist::Stylist,
    root: E,
) -> bool {
    let mut used = false;
    stylist.for_each_cascade_data_with_scope(root, |data, _| {
        used |= data.relative_invalidation_map_attributes().used;
    });
    used
}

/// The directions of the structural-state dependencies (`:empty`, `:first-child`, `:nth-child`, ...) inside `:has()`.
pub(crate) fn structural_kinds<E: style::dom::TElement>(
    stylist: &style::stylist::Stylist,
    root: E,
) -> Kinds {
    let mut kinds = Kinds::default();
    stylist.any_applicable_rule_data(root, |d| {
        let extra = d.relative_invalidation_map_attributes();
        if extra.used {
            each_entry(&extra.ts_state_to_selector, &mut |e| {
                kinds.add_deps(std::slice::from_ref(&e.dep))
            });
        }
        false
    });
    kinds
}

/// The restyle plan of one pass: node -> strongest hint.
#[derive(Default, Debug)]
pub(crate) struct HasPlan {
    pub(crate) anchors: BTreeMap<NodeId, AnchorHint>,
    /// Some `:has()` result also decides what *other* elements match (an `@scope` root or limit,
    /// the `S` of `:nth-child(of S)`): every anchor then restyles its parent's subtree.
    wide: bool,
}

impl HasPlan {
    fn consider(&mut self, doc: &Document, sidecars: &SidecarStore, anchor: NodeId) {
        let Some(sc) = sidecars.get(anchor) else {
            return;
        };
        let flags = sc.selector_flags();
        if !flags.intersects(
            ElementSelectorFlags::ANCHORS_RELATIVE_SELECTOR
                | ElementSelectorFlags::ANCHORS_RELATIVE_SELECTOR_NON_SUBJECT,
        ) {
            return;
        }
        if self.wide || flags.contains(ElementSelectorFlags::ANCHORS_RELATIVE_SELECTOR_NON_SUBJECT)
        {
            // The anchor's siblings and descendants depend on it: restyle the parent's subtree
            // (the anchor's own when it is the root).
            let arena = doc.arena();
            let target = arena
                .node(anchor)
                .parent()
                .filter(|&p| arena.node(p).is_element())
                .unwrap_or(anchor);
            self.anchors.insert(target, AnchorHint::Subtree);
        } else {
            self.anchors.entry(anchor).or_insert(AnchorHint::SelfOnly);
        }
    }

    fn earlier_siblings(
        &mut self,
        doc: &Document,
        sidecars: &SidecarStore,
        node: NodeId,
        only_prev: bool,
    ) {
        let arena = doc.arena();
        let Some(parent) = arena.node(node).parent() else {
            return;
        };
        let kids = arena.node(parent).children();
        let Some(i) = kids.iter().position(|&c| c == node) else {
            return;
        };
        for &sib in kids[..i].iter().rev() {
            if arena.node(sib).is_element() {
                self.consider(doc, sidecars, sib);
                if only_prev {
                    break;
                }
            }
        }
    }

    /// Anchors of the directions `kinds`, relative to element `e` (`e` itself is never its own anchor).
    fn collect_element(
        &mut self,
        doc: &Document,
        sidecars: &SidecarStore,
        e: NodeId,
        kinds: Kinds,
    ) {
        let arena = doc.arena();
        if kinds.is_empty() || !arena.contains(e) || !arena.node(e).is_element() {
            return;
        }
        let parent = arena
            .node(e)
            .parent()
            .filter(|&p| arena.node(p).is_element());
        if kinds.has(Kinds::PARENT)
            && let Some(p) = parent
        {
            self.consider(doc, sidecars, p);
        }
        if kinds.has(Kinds::PREV | Kinds::EARLIER) {
            self.earlier_siblings(doc, sidecars, e, !kinds.has(Kinds::EARLIER));
        }
        self.collect_ancestors(doc, sidecars, parent, kinds);
    }

    /// Anchors that are `start` or its ancestors (`Ancestors`) or the earlier siblings of these (`Ancestor*Sibling`).
    fn collect_ancestors(
        &mut self,
        doc: &Document,
        sidecars: &SidecarStore,
        start: Option<NodeId>,
        kinds: Kinds,
    ) {
        let arena = doc.arena();
        let mut cur = start;
        while let Some(a) = cur {
            if !arena.node(a).is_element() {
                break;
            }
            if kinds.has(Kinds::ANCESTORS) {
                self.consider(doc, sidecars, a);
            }
            if kinds.has(Kinds::ANC_PREV | Kinds::ANC_EARLIER) {
                self.earlier_siblings(doc, sidecars, a, !kinds.has(Kinds::ANC_EARLIER));
            }
            cur = arena.node(a).parent();
        }
    }
}

/// The presence keys of an element that is inserted or removed (its own classes, id, attributes, type).
fn presence(doc: &Document, node: NodeId, q: &mut Query) {
    let Some(e) = doc.arena().get(node).and_then(|n| n.as_element()) else {
        return;
    };
    q.classes.extend(e.classes().map(Atom::from));
    if let Some(i) = e.attr("id") {
        q.ids.push(Atom::from(i));
    }
    q.attrs.extend(
        e.attributes()
            .iter()
            .map(|a| GenericAtomIdent(a.name.local.clone())),
    );
    q.local_names.push(GenericAtomIdent(e.name().local.clone()));
}

/// Everything the epoch changed that a `:has()` can observe, as anchors to restyle. Call only when
/// an active sheet contains a `:has()` rule: a document without one pays nothing.
pub(crate) fn plan<E: style::dom::TElement>(
    doc: &Document,
    sidecars: &SidecarStore,
    seg: &EpochSegment,
    stylist: &style::stylist::Stylist,
    root: E,
    wide: bool,
) -> HasPlan {
    let mut plan = HasPlan {
        wide,
        ..HasPlan::default()
    };
    let arena = doc.arena();
    let structural = structural_kinds(stylist, root);

    // Element changes: attribute, class, id, state.
    for (id, prior) in seg.element_priors() {
        let Some(e) = arena.get(id).and_then(|n| n.as_element()) else {
            continue;
        };
        if !arena.is_connected(id) {
            continue;
        }
        let k = crate::style::snapshot::change_keys(id, e, &prior);
        let q = Query {
            classes: k.classes,
            ids: k.ids,
            attrs: k.attrs,
            state: k.state,
            ..Query::default()
        };
        let kinds = kinds_for(stylist, root, &q);
        plan.collect_element(doc, sidecars, id, kinds);
    }

    // Child-list and text changes at a parent `P`.
    let mut seen: BTreeSet<NodeId> = BTreeSet::new();
    for entry in seg.entries() {
        let (parent, added, removed): (NodeId, &[NodeId], &[NodeId]) = match &entry.kind {
            EntryKind::ChildList {
                parent,
                added,
                removed,
                ..
            } => (*parent, added, removed),
            EntryKind::CharacterData { node } => match arena.get(*node).and_then(|n| n.parent()) {
                Some(p) => (p, &[], &[]),
                None => continue,
            },
            _ => continue,
        };
        if !arena.contains(parent)
            || !arena.is_connected(parent)
            || !arena.node(parent).is_element()
        {
            continue;
        }
        // The nodes that came or went (and their descendants): their presence keys.
        let mut q = Query {
            mutation: true,
            state: ElementState::all(),
            ..Query::default()
        };
        let mut unknown = false;
        for &n in added.iter().chain(removed) {
            if !arena.contains(n) {
                unknown = true; // a freed node: its keys are gone
                continue;
            }
            for d in arena.subtree(n) {
                presence(doc, d, &mut q);
            }
        }
        let kinds = if unknown {
            Kinds::ALL
        } else {
            kinds_for(stylist, root, &q)
        }
        .union(structural);
        // `X` as a virtual child of `P`: P and its ancestors, and their earlier siblings.
        plan.collect_ancestors(doc, sidecars, Some(parent), kinds);
        // P itself changed structurally (`:empty`; the structural state of its children is covered by its subtree restyle).
        if seen.insert(parent) {
            plan.collect_element(doc, sidecars, parent, structural);
        }
    }
    plan
}

/// Whether author text can make a `:has()` result decide the match of other elements: `:has()`
/// next to `@scope` (the root or limit selector) or `:nth-child(... of S)`. A textual test, so it
/// over-approximates (more restyling, never less).
pub(crate) fn has_decides_other_elements(text: &str) -> bool {
    text.contains(":has(") && (text.contains("@scope") || text.contains(" of "))
}

/// Parents whose children must be restyled because a changed element's class, id, attribute or
/// state is one an `:nth-child(... of S)` / `:nth-last-child(... of S)` selector can select on:
/// the matching set among the siblings changes, which shifts the indices of the others. Stylo
/// tracks these dependencies (`might_have_nth_of_*`) but, as for `:has()`, leaves the invalidation
/// to the embedder (Gecko restyles the parent's children). The parent's subtree is restyled so
/// that selectors continuing past the nth-of compound see the change as well.
pub(crate) fn nth_of_parents<E: style::dom::TElement>(
    doc: &Document,
    stylist: &style::stylist::Stylist,
    root: E,
    changes: &[crate::style::snapshot::ChangeKeys],
) -> BTreeSet<NodeId> {
    let arena = doc.arena();
    let mut parents = BTreeSet::new();
    for c in changes {
        let Some(p) = arena.get(c.node).and_then(|n| n.parent()) else {
            continue;
        };
        if parents.contains(&p) || !arena.node(p).is_element() {
            continue;
        }
        let depends = stylist.any_applicable_rule_data(root, |d| {
            d.has_nth_of_state_dependency(c.state)
                || c.classes
                    .iter()
                    .any(|x| d.might_have_nth_of_class_dependency(x))
                || c.ids.iter().any(|x| d.might_have_nth_of_id_dependency(x))
                || c.attrs
                    .iter()
                    .any(|x| d.might_have_nth_of_attribute_dependency(x))
        });
        if depends {
            parents.insert(p);
        }
    }
    parents
}
