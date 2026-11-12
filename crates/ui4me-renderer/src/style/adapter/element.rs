//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! `selectors::Element` and `TElement` on the arena (T041). Ported from the X-STYLE-ADAPTER
//! harness (`adapter.rs`), with two changes for production:
//!
//! * selector matching (`has_id`, `has_class`, attributes, state) reads the **current DOM**,
//!   never sidecar caches, so the selector service (query.rs) matches nodes created or changed
//!   since the last restyle exactly as the cascade would after it (R-12, FR-019);
//! * sidecar reads outside the traversal-only methods are tolerant of a missing sidecar
//!   (`sc_opt`); the traversal-only methods (`ensure_data`, `clear_data`, `mutate_data`, the
//!   dirty and snapshot bits) keep the panicking accessor (invariant: `sync` ran first).
//!
//! ElementData stays in the sidecar, reachable only crate-internally (sealed surface, F1).

use super::document::NoShadowRoot;
use super::form_state::FormFacts;
use super::handle::StyleNode;
use super::state::{StateInputs, compose};
use html5ever::ns;
use selectors::OpaqueElement;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::{BLOOM_HASH_MASK, BloomFilter};
use selectors::matching::{ElementSelectorFlags, MatchingContext, VisitedHandlingMode};
use selectors::sink::Push;
use std::ptr::NonNull;
use std::sync::atomic::Ordering;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::bloom::each_relevant_element_hash;
use style::context::SharedStyleContext;
use style::data::{ElementDataMut, ElementDataRef};
use style::dom::{LayoutIterator, TElement, TNode};
use style::properties::PropertyDeclarationBlock;
use style::selector_parser::{NonTSPseudoClass, PseudoElement, SelectorImpl};
use style::servo_arc::{Arc, ArcBorrow};
use style::shared_lock::Locked;
use style::stylesheets::scope_rule::ImplicitScopeRoot;
use style::values::{AtomIdent, AtomString, GenericAtomIdent};
use style::{Atom, LocalName, Namespace};
use stylo_dom::ElementState;
use ui4me_document::node::{ElementData, NodeData};

/// `:lang(range)` against a language tag (Selectors 4 / RFC 4647 basic range filtering: the
/// match is ASCII case-insensitive and a range matches itself and any longer tag that continues
/// with a hyphen; `*` matches any non-empty tag). With no `lang` in scope nothing matches
/// (selector-support.md: no document default language).
fn lang_matches(tag: Option<&str>, range: &str) -> bool {
    let Some(tag) = tag.filter(|t| !t.is_empty()) else {
        return false;
    };
    let range = range.trim();
    if range == "*" {
        return true;
    }
    let (tag, range) = (tag.to_ascii_lowercase(), range.to_ascii_lowercase());
    tag == range || (tag.starts_with(&range) && tag.as_bytes().get(range.len()) == Some(&b'-'))
}

impl<'a> StyleNode<'a> {
    /// The language in scope: the nearest inclusive ancestor element's `lang` attribute value.
    pub(crate) fn language(&self) -> Option<String> {
        let mut cur = Some(*self);
        while let Some(n) = cur {
            if let Some(e) = n.elem()
                && let Some(a) = e.attributes().iter().find(|a| &*a.name.local == "lang")
            {
                return Some(a.value.to_string());
            }
            cur = <Self as selectors::Element>::parent_element(&n);
        }
        None
    }

    fn sibling_element(&self, delta: isize) -> Option<Self> {
        let mut n = self.sibling(delta);
        while let Some(s) = n {
            if s.is_elem() {
                return Some(s);
            }
            n = s.sibling(delta);
        }
        None
    }

    /// The element's current state, composed from DocumentCore's tables.
    pub(crate) fn current_state(&self) -> ElementState {
        let Some(e) = self.elem() else {
            return ElementState::empty();
        };
        compose(&StateInputs {
            element: e,
            attrs: e.attributes(),
            ui: e.ui_flags(),
            control: e.control_state().map(|c| c.flags()),
            facts: self.form_facts(e),
        })
    }

    /// The facts of a `textarea` / `select` that live in its children (see `FormFacts`).
    fn form_facts(&self, e: &ElementData) -> FormFacts {
        let mut facts = FormFacts::default();
        if !e.is_html() {
            return facts;
        }
        if matches!(
            e.local_name(),
            "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
        ) {
            facts.fieldset_disabled = self.view().fieldset_disabled(self.id);
        }
        let arena = self.view().arena();
        let text_of = |id: ui4me_engine_types::ids::NodeId| -> String {
            arena
                .node(id)
                .children()
                .iter()
                .filter_map(|c| {
                    if let NodeData::Text(t) = arena.node(*c).data() {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect()
        };
        match e.local_name() {
            "textarea" if !e.control_state().is_some_and(|c| c.dirty_value) => {
                let text = text_of(self.id);
                facts.text_empty = Some(
                    text.strip_prefix("\r\n")
                        .or_else(|| text.strip_prefix('\n'))
                        .unwrap_or(&text)
                        .is_empty(),
                );
            }
            "select" if e.attr("required").is_some() => {
                // The list of options: `option` children and those of `optgroup` children.
                let mut options: Vec<(ui4me_engine_types::ids::NodeId, bool)> = Vec::new();
                for c in self.node().children() {
                    match arena.node(*c).as_element() {
                        Some(o) if o.is_html_named("option") => options.push((*c, true)),
                        Some(g) if g.is_html_named("optgroup") => {
                            options.extend(
                                arena
                                    .node(*c)
                                    .children()
                                    .iter()
                                    .filter(|o| {
                                        arena
                                            .node(**o)
                                            .as_element()
                                            .is_some_and(|o| o.is_html_named("option"))
                                    })
                                    .map(|o| (*o, false)),
                            );
                        }
                        _ => {}
                    }
                }
                let opt = |id: ui4me_engine_types::ids::NodeId| arena.node(id).as_element();
                let selected = |id| {
                    opt(id)
                        .and_then(|o| o.control_state())
                        .and_then(|c| c.selectedness)
                        == Some(true)
                };
                let multiple = e.attr("multiple").is_some();
                let list_box = multiple
                    || e.attr("size")
                        .and_then(|s| s.trim().parse::<u32>().ok())
                        .is_some_and(|n| n > 1);
                // The selected option: the last one with selectedness; for a drop-down with none, the
                // first one that is not disabled.
                let chosen = options
                    .iter()
                    .rev()
                    .find(|(id, _)| selected(*id))
                    .or_else(|| {
                        if list_box {
                            None
                        } else {
                            options.iter().find(|(id, _)| {
                                opt(*id).is_some_and(|o| o.attr("disabled").is_none())
                            })
                        }
                    });
                facts.select_missing = match chosen {
                    None => true,
                    Some((id, direct)) => {
                        let o = opt(*id).expect("an option");
                        let value = o.attr("value").map(str::to_string).unwrap_or_else(|| {
                            text_of(*id)
                                .split_ascii_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ")
                        });
                        // The placeholder label option: the first option, a direct child, with an empty value.
                        !list_box
                            && value.is_empty()
                            && *direct
                            && options.first().map(|f| f.0) == Some(*id)
                    }
                };
            }
            _ => {}
        }
        facts
    }
}

impl<'a> selectors::Element for StyleNode<'a> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        // The key is never zero (generation ≥ 1 in the high half), so NonNull::new never fails.
        OpaqueElement::from_ptr(NonNull::new(self.key() as *mut ()).expect("non-zero key"))
    }
    fn parent_element(&self) -> Option<Self> {
        TNode::traversal_parent(self)
    }
    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }
    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }
    fn is_pseudo_element(&self) -> bool {
        false
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling_element(-1)
    }
    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling_element(1)
    }
    fn first_element_child(&self) -> Option<Self> {
        self.node()
            .children()
            .iter()
            .map(|c| self.with(*c))
            .find(|c| c.is_elem())
    }
    fn is_html_element_in_html_document(&self) -> bool {
        self.elem().is_some_and(|e| e.is_html())
    }
    fn has_local_name(&self, local_name: &web_atoms::LocalName) -> bool {
        self.elem().is_some_and(|e| e.name().local == *local_name)
    }
    fn has_namespace(&self, ns: &web_atoms::Namespace) -> bool {
        self.elem().is_some_and(|e| e.name().ns == *ns)
    }
    fn is_same_type(&self, other: &Self) -> bool {
        match (self.elem(), other.elem()) {
            (Some(a), Some(b)) => a.name().local == b.name().local && a.name().ns == b.name().ns,
            _ => false,
        }
    }
    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&Namespace>,
        local_name: &LocalName,
        operation: &AttrSelectorOperation<&AtomString>,
    ) -> bool {
        let Some(e) = self.elem() else { return false };
        e.attributes().iter().any(|a| {
            a.name.local == local_name.0
                && match ns {
                    NamespaceConstraint::Any => true,
                    NamespaceConstraint::Specific(ns) => a.name.ns == ns.0,
                }
                && operation.eval_str(&a.value)
        })
    }
    fn match_non_ts_pseudo_class(
        &self,
        pc: &NonTSPseudoClass,
        _cx: &mut MatchingContext<SelectorImpl>,
    ) -> bool {
        match *pc {
            NonTSPseudoClass::Link | NonTSPseudoClass::AnyLink => {
                self.current_state().contains(ElementState::UNVISITED)
            }
            NonTSPseudoClass::Visited => false,
            // UA-only: HTML select display size defaults to one (four for multiple),
            // while a valid positive size overrides the default even for multiple.
            NonTSPseudoClass::Ui4meSingleRowSelect => self.elem().is_some_and(|e| {
                if !e.is_html_named("select") {
                    return false;
                }
                ui4me_document::select::display_size(e.attr("size"), e.attr("multiple").is_some())
                    == 1
            }),
            NonTSPseudoClass::Lang(ref range) => lang_matches(self.language().as_deref(), range),
            // `:dir()` (015 WM-2, Stylo patch P-006-8): the element's HTML directionality (`dir.rs`).
            NonTSPseudoClass::Dir(ref d) => {
                let rtl = crate::style::dir::is_rtl(self.view(), self.id);
                if rtl { &**d == "rtl" } else { &**d == "ltr" }
            }
            ref other => {
                let flag = other.state_flag();
                !flag.is_empty() && self.current_state().intersects(flag)
            }
        }
    }
    fn match_pseudo_element(
        &self,
        _pe: &PseudoElement,
        _cx: &mut MatchingContext<SelectorImpl>,
    ) -> bool {
        false
    }
    fn apply_selector_flags(&self, flags: ElementSelectorFlags) {
        let own = flags.for_self();
        if !own.is_empty()
            && let Some(sc) = self.sc_opt()
        {
            sc.insert_selector_flags(own);
        }
        let parent = flags.for_parent();
        if !parent.is_empty()
            && let Some(p) = TNode::traversal_parent(self)
            && let Some(sc) = p.sc_opt()
        {
            sc.insert_selector_flags(parent);
        }
    }
    fn is_link(&self) -> bool {
        self.current_state().contains(ElementState::UNVISITED)
    }
    fn is_html_slot_element(&self) -> bool {
        false
    }
    fn has_id(&self, id: &AtomIdent, cs: CaseSensitivity) -> bool {
        self.elem()
            .and_then(|e| e.attr("id"))
            .is_some_and(|v| cs.eq(v.as_bytes(), id.0.as_bytes()))
    }
    fn has_class(&self, name: &AtomIdent, cs: CaseSensitivity) -> bool {
        self.elem()
            .is_some_and(|e| e.classes().any(|c| cs.eq(c.as_bytes(), name.0.as_bytes())))
    }
    fn has_custom_state(&self, _name: &AtomIdent) -> bool {
        false
    }
    fn imported_part(&self, _name: &AtomIdent) -> Option<AtomIdent> {
        None
    }
    fn is_part(&self, _name: &AtomIdent) -> bool {
        false
    }
    fn is_empty(&self) -> bool {
        self.node()
            .children()
            .iter()
            .all(|c| match self.view().arena().node(*c).data() {
                NodeData::Text(t) => t.is_empty(),
                NodeData::Comment(_) => true,
                _ => false,
            })
    }
    fn is_root(&self) -> bool {
        self.node().parent() == Some(self.view().document_node())
    }
    fn add_element_unique_hashes(&self, filter: &mut BloomFilter) -> bool {
        each_relevant_element_hash(*self, |h| filter.insert_hash(h & BLOOM_HASH_MASK));
        true
    }
}

/// Child iterator for `traversal_children`.
pub struct Children<'a> {
    parent: StyleNode<'a>,
    i: usize,
}

impl<'a> Iterator for Children<'a> {
    type Item = StyleNode<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let c = *self.parent.node().children().get(self.i)?;
        self.i += 1;
        Some(self.parent.with(c))
    }
}

impl<'a> TElement for StyleNode<'a> {
    type ConcreteNode = StyleNode<'a>;
    type TraversalChildrenIterator = Children<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        *self
    }
    fn traversal_children(&self) -> LayoutIterator<Self::TraversalChildrenIterator> {
        LayoutIterator(Children {
            parent: *self,
            i: 0,
        })
    }
    fn is_html_element(&self) -> bool {
        self.elem().is_some_and(|e| e.name().ns == ns!(html))
    }
    fn is_mathml_element(&self) -> bool {
        self.elem().is_some_and(|e| e.name().ns == ns!(mathml))
    }
    fn is_svg_element(&self) -> bool {
        self.elem().is_some_and(|e| e.name().ns == ns!(svg))
    }
    fn style_attribute(&self) -> Option<ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        self.sc_opt()?
            .style_attribute
            .as_ref()
            .map(|b| b.borrow_arc())
    }
    fn animation_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn transition_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn state(&self) -> ElementState {
        self.current_state()
    }
    fn has_part_attr(&self) -> bool {
        false
    }
    fn exports_any_part(&self) -> bool {
        false
    }
    fn id(&self) -> Option<&Atom> {
        self.sc_opt()?.id_atom.as_ref()
    }
    fn each_class<F: FnMut(&AtomIdent)>(&self, mut cb: F) {
        if let Some(sc) = self.sc_opt() {
            for c in &sc.class_atoms {
                cb(AtomIdent::cast(c));
            }
        }
    }
    fn each_custom_state<F: FnMut(&AtomIdent)>(&self, _cb: F) {}
    fn each_attr_name<F: FnMut(&LocalName)>(&self, mut cb: F) {
        if let Some(e) = self.elem() {
            for a in e.attributes() {
                cb(GenericAtomIdent::cast(&a.name.local));
            }
        }
    }
    fn has_dirty_descendants(&self) -> bool {
        self.sc_opt()
            .is_some_and(|s| s.dirty_descendants.load(Ordering::Relaxed))
    }
    fn has_snapshot(&self) -> bool {
        self.sc_opt()
            .is_some_and(|s| s.has_snapshot.load(Ordering::Relaxed))
    }
    fn handled_snapshot(&self) -> bool {
        self.sc_opt()
            .is_some_and(|s| s.handled_snapshot.load(Ordering::Relaxed))
    }
    unsafe fn set_handled_snapshot(&self) {
        self.sc().handled_snapshot.store(true, Ordering::Relaxed)
    }
    unsafe fn set_dirty_descendants(&self) {
        self.sc().dirty_descendants.store(true, Ordering::Relaxed)
    }
    unsafe fn unset_dirty_descendants(&self) {
        self.sc().dirty_descendants.store(false, Ordering::Relaxed)
    }
    fn store_children_to_process(&self, _n: isize) {
        unreachable!("parallel traversal is not used (pool = None)")
    }
    fn did_process_child(&self) -> isize {
        unreachable!("parallel traversal is not used (pool = None)")
    }
    unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
        self.sc().ensure_data()
    }
    unsafe fn clear_data(&self) {
        self.sc().clear_data()
    }
    fn has_data(&self) -> bool {
        self.sc_opt().is_some_and(|s| s.has_data())
    }
    fn borrow_data(&self) -> Option<ElementDataRef<'_>> {
        // As in Blitz: data without a primary style reads as "not yet styled".
        self.sc_opt()?
            .borrow_data()
            .filter(|d| d.styles.primary.is_some())
    }
    fn mutate_data(&self) -> Option<ElementDataMut<'_>> {
        self.sc().mutate_data()
    }
    fn skip_item_display_fixup(&self) -> bool {
        false
    }
    fn may_have_animations(&self) -> bool {
        self.sc_opt()
            .is_some_and(|s| s.may_have_animations.load(Ordering::Relaxed))
    }
    fn set_may_have_animations(&self) {
        self.sc().may_have_animations.store(true, Ordering::Relaxed)
    }
    fn has_animations(&self, cx: &SharedStyleContext) -> bool {
        self.has_css_animations(cx, None) || self.has_css_transitions(cx, None)
    }
    fn has_css_animations(&self, cx: &SharedStyleContext, pseudo: Option<PseudoElement>) -> bool {
        // 006 T087 (R-11): the document's animation set (owned by StyleBridge) is the source.
        cx.animations
            .has_active_animations(&style::animation::AnimationSetKey::new(
                TNode::opaque(&self.as_node()),
                pseudo,
            ))
    }
    fn has_css_transitions(&self, cx: &SharedStyleContext, pseudo: Option<PseudoElement>) -> bool {
        cx.animations
            .has_active_transitions(&style::animation::AnimationSetKey::new(
                TNode::opaque(&self.as_node()),
                pseudo,
            ))
    }
    fn shadow_root(&self) -> Option<NoShadowRoot<'a>> {
        None
    }
    fn containing_shadow(&self) -> Option<NoShadowRoot<'a>> {
        None
    }
    fn lang_attr(&self) -> Option<style::selector_parser::AttrValue> {
        None
    }
    fn match_element_lang(
        &self,
        _o: Option<Option<style::selector_parser::AttrValue>>,
        v: &style::selector_parser::Lang,
    ) -> bool {
        lang_matches(self.language().as_deref(), v)
    }
    fn is_html_document_body_element(&self) -> bool {
        self.elem().is_some_and(|e| e.is_html_named("body")) && self.view().body() == Some(self.id)
    }
    fn synthesize_presentational_hints_for_legacy_attributes<
        V: Push<ApplicableDeclarationBlock>,
    >(
        &self,
        _: VisitedHandlingMode,
        hints: &mut V,
    ) {
        let mode = self.view().mode();
        let Some(text) = crate::style::hints::declarations(
            self,
            mode == ui4me_document::document::DocumentMode::Quirks,
        ) else {
            return;
        };
        let block = style::properties::parse_style_attribute(
            &text,
            &crate::style::sheets::blank_url(),
            None,
            super::document::quirks(mode),
            style::stylesheets::CssRuleType::Style,
        );
        let block = Arc::new(self.sidecars().lock.wrap(block));
        hints.push(ApplicableDeclarationBlock::from_declarations(
            block,
            style::rule_tree::CascadeLevel::new(style::rule_tree::CascadeOrigin::PresHints),
            style::stylesheets::layer_rule::LayerOrder::root(),
        ));
    }
    fn local_name(&self) -> &web_atoms::LocalName {
        &self.elem().expect("element").name().local
    }
    fn namespace(&self) -> &web_atoms::Namespace {
        &self.elem().expect("element").name().ns
    }
    /// The content-box size of the element's box from the last layout (`container.rs`, 015 CQ-3).
    /// An element that generates no box, or a non-atomic inline box (size containment has no effect
    /// on either, CSS Containment 2 section 3.1), has no size: its size queries are unknown.
    fn query_container_size(
        &self,
        display: &style::values::specified::Display,
    ) -> euclid::default::Size2D<Option<app_units::Au>> {
        if display.is_none() || display.is_contents() || display.is_inline_flow() {
            return Default::default();
        }
        match self.sidecars().containers.get(self.id) {
            Some(s) => euclid::default::Size2D::new(
                s.width.map(app_units::Au::from_f32_px),
                s.height.map(app_units::Au::from_f32_px),
            ),
            None => Default::default(),
        }
    }
    fn has_selector_flags(&self, flags: ElementSelectorFlags) -> bool {
        self.sc_opt()
            .is_some_and(|s| s.selector_flags().contains(flags))
    }
    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        self.sc_opt().map_or(ElementSelectorFlags::empty(), |s| {
            s.selector_flags().intersection(
                ElementSelectorFlags::RELATIVE_SELECTOR_SEARCH_DIRECTION_ANCESTOR_SIBLING,
            )
        })
    }
    fn implicit_scope_for_sheet_in_shadow_root(
        _: OpaqueElement,
        _: usize,
    ) -> Option<ImplicitScopeRoot> {
        None
    }
    fn get_attr(&self, attr: &LocalName, ns: &Namespace) -> Option<String> {
        let e = self.elem()?;
        // `attr(NAME)` matches an HTML element's attribute ASCII case-insensitively (CSS Values 5).
        e.attributes()
            .iter()
            .find(|a| {
                a.name.ns == ns.0
                    && (a.name.local == attr.0 || a.name.local.eq_ignore_ascii_case(&attr.0))
            })
            .map(|a| a.value.clone())
    }
}
