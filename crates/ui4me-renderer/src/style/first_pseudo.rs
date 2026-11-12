//! owner: 006-style-engine (015 TXT-5, lane L-PSEUDO) - `::first-line` and `::first-letter`.
//!
//! Servo-mode Stylo cascades both as eager pseudo-elements of the originating block container, inheriting from
//! its primary style; matching them needs the `layout.css.first-line.enabled` pref (profile 1.14.0). Two things
//! the cascade does not do are done here, after the main style pass, for the elements that have a rule:
//!
//! * **The style of a box on the first line.** CSS Pseudo 4 section 3.1: the inherited properties of everything
//!   on the first formatted line come from `::first-line`, not from the element. For every inline-level
//!   descendant (and for the `::before`/`::after` of the block and of those descendants) the cascade is run again
//!   with the first-line style (the style of its parent on the line) as the parent and `FirstLineReparenting::Yes`
//!   (the reset properties are the element's own). The first in-flow block child chain gets the same (the first
//!   formatted line of a block is the first line of its first block child). Which text really sits on the first
//!   line is a line-breaking fact; layout decides it (`ui4me-layout` `ifc`).
//! * **`::first-letter` under `::first-line`.** The first letter is on the first line, so its parent is the
//!   first-line style of the block: the eager pseudo style is cascaded again from it.
//!
//! The results are published as `NodeStyles::{first_line, first_line_before, first_line_after, first_letter}`.
//! Unchanged inputs (same primary style and same parent) keep the `Arc` of the previous pass (`FirstCache`), so
//! the box tree sees no difference.

use crate::style::view::NodeStyles;
use std::collections::{HashMap, HashSet};
use style::context::{CascadeInputs, TreeCountingCaches};
use style::dom::TElement;
use style::properties::{ComputedValues, FirstLineReparenting};
use style::rule_cache::RuleCacheConditions;
use style::selector_parser::PseudoElement;
use style::servo_arc::Arc as StyloArc;
use style::shared_lock::StylesheetGuards;
use style::stylist::Stylist;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::values::specified::position::PositionTryFallbacksTryTactic;
use ui4me_document::arena::Arena;
use ui4me_engine_types::ids::NodeId;

/// What a variant was computed from, and its result (the three are kept alive, so addresses stay unique).
pub(crate) struct CachedFirst {
    primary: StyloArc<ComputedValues>,
    parent: StyloArc<ComputedValues>,
    result: StyloArc<ComputedValues>,
}

/// Variants of the previous pass, by (element, kind): 0 element, 1 `::before`, 2 `::after`, 3 `::first-letter`,
/// 4 the element's own `::first-line` below an ancestor's, 5 the element below an ancestor's first line.
#[derive(Default)]
pub(crate) struct FirstCache(pub(crate) HashMap<(NodeId, u8), CachedFirst>);

fn floating(cv: &ComputedValues) -> bool {
    cv.get_box().float.is_floating()
}

fn out_of_flow(cv: &ComputedValues) -> bool {
    cv.get_box().position.is_absolutely_positioned()
}

/// A block container: `display` that has a first formatted line of its own.
pub(crate) fn is_block_container(cv: &ComputedValues) -> bool {
    let d = cv.get_box().display;
    if d.is_none() || d.is_contents() || d.is_inline_flow() {
        return false;
    }
    matches!(
        d.inside(),
        DisplayInside::Flow | DisplayInside::FlowRoot | DisplayInside::TableCell
    )
}

fn in_flow_block_level(cv: &ComputedValues) -> bool {
    let d = cv.get_box().display;
    is_block_container(cv)
        && d.outside() == DisplayOutside::Block
        && !floating(cv)
        && !out_of_flow(cv)
}

/// An inline box that continues the first line's inline content (not atomic, not out of flow).
fn plain_inline(cv: &ComputedValues) -> bool {
    cv.get_box().display.is_inline_flow() && !floating(cv) && !out_of_flow(cv)
}

pub(crate) struct Phase<'a, E: TElement, F: Fn(NodeId) -> E> {
    pub stylist: &'a Stylist,
    pub guards: &'a StylesheetGuards<'a>,
    pub arena: &'a Arena,
    pub handle: F,
    pub cache: &'a mut FirstCache,
    pub fresh: HashMap<(NodeId, u8), CachedFirst>,
    /// Blocks whose first-line style was settled by an ancestor's walk.
    pub done: HashSet<NodeId>,
    pub _e: std::marker::PhantomData<E>,
}

impl<E: TElement, F: Fn(NodeId) -> E> Phase<'_, E, F> {
    fn keep(
        &mut self,
        key: (NodeId, u8),
        primary: &StyloArc<ComputedValues>,
        parent: &StyloArc<ComputedValues>,
        make: impl FnOnce() -> StyloArc<ComputedValues>,
    ) -> StyloArc<ComputedValues> {
        if let Some(c) = self.cache.0.remove(&key)
            && StyloArc::ptr_eq(&c.primary, primary)
            && StyloArc::ptr_eq(&c.parent, parent)
        {
            let r = c.result.clone();
            self.fresh.insert(key, c);
            return r;
        }
        let result = make();
        self.fresh.insert(
            key,
            CachedFirst {
                primary: primary.clone(),
                parent: parent.clone(),
                result: result.clone(),
            },
        );
        result
    }

    /// `element`'s (or its pseudo's) style cascaded again with `parent` as the style it inherits from.
    fn reparent(
        &mut self,
        node: NodeId,
        kind: u8,
        pseudo: Option<&PseudoElement>,
        primary: &StyloArc<ComputedValues>,
        parent: &StyloArc<ComputedValues>,
    ) -> StyloArc<ComputedValues> {
        let (stylist, guards, h) = (self.stylist, self.guards, (self.handle)(node));
        self.keep((node, kind), primary, parent, || {
            let inputs = CascadeInputs::new_from_style(primary);
            stylist.cascade_style_and_visited::<E>(
                Some(h),
                pseudo,
                &inputs,
                guards,
                Some(parent),
                Some(parent),
                FirstLineReparenting::Yes {
                    style_to_reparent: primary,
                },
                &PositionTryFallbacksTryTactic::default(),
                None,
                &mut RuleCacheConditions::default(),
                &mut TreeCountingCaches::default(),
            )
        })
    }

    /// An eager pseudo-element style cascaded again below `parent` (`::first-line`, `::first-letter`).
    fn recascade(
        &mut self,
        node: NodeId,
        kind: u8,
        pseudo: &PseudoElement,
        own: &StyloArc<ComputedValues>,
        parent: &StyloArc<ComputedValues>,
    ) -> StyloArc<ComputedValues> {
        let (stylist, guards, h) = (self.stylist, self.guards, (self.handle)(node));
        self.keep((node, kind), own, parent, || {
            let inputs = CascadeInputs::new_from_style(own);
            stylist.compute_pseudo_element_style_with_inputs::<E>(
                inputs,
                pseudo,
                guards,
                Some(parent),
                Some(h),
            )
        })
    }

    /// The block `node`: settle its own first-line style (`inherited`: the first-line style of the block it is the
    /// first block child of), then its content.
    pub fn block(
        &mut self,
        styles: &mut HashMap<NodeId, NodeStyles>,
        node: NodeId,
        inherited: Option<StyloArc<ComputedValues>>,
        eager: &HashMap<NodeId, StyloArc<ComputedValues>>,
    ) {
        if !self.done.insert(node) {
            return;
        }
        let Some(ns) = styles.get(&node) else { return };
        let primary = ns.primary.clone();
        if !is_block_container(&primary) {
            return;
        }
        let (before, after) = (ns.before.clone(), ns.after.clone());
        let own = eager.get(&node).cloned();
        // The content on the first line inherits from the first-line style of the block that owns the line, not from the
        // blocks in between (WPT first-line-with-out-of-flow: text in a `color: blue` child block is green): a block
        // that is the first line of an ancestor's takes that ancestor's first-line style (its own `::first-line`, if it
        // has one, cascades below it).
        let variant = match (own, inherited) {
            (Some(o), Some(pv)) => {
                Some(self.recascade(node, 4, &PseudoElement::FirstLine, &o, &pv))
            }
            (None, Some(pv)) => Some(pv),
            (Some(o), None) => Some(o),
            (None, None) => None,
        };
        let Some(variant) = variant else { return };
        let (b, a) = (
            before.map(|b| self.reparent(node, 1, Some(&PseudoElement::Before), &b, &variant)),
            after.map(|a| self.reparent(node, 2, Some(&PseudoElement::After), &a, &variant)),
        );
        if let Some(s) = styles.get_mut(&node) {
            s.first_line = Some(variant.clone());
            s.first_line_before = b;
            s.first_line_after = a;
        }
        let mut content_seen = false;
        self.children(styles, node, &variant, &mut content_seen, eager);
    }

    /// The children of `parent` on the first line: inline descendants get their variants; the first in-flow
    /// block child (before any inline content) continues the chain.
    fn children(
        &mut self,
        styles: &mut HashMap<NodeId, NodeStyles>,
        parent: NodeId,
        parent_variant: &StyloArc<ComputedValues>,
        content_seen: &mut bool,
        eager: &HashMap<NodeId, StyloArc<ComputedValues>>,
    ) {
        let kids: Vec<NodeId> = self.arena.node(parent).children().to_vec();
        for c in kids {
            let n = self.arena.node(c);
            if !n.is_element() {
                if n.kind() == ui4me_document::node::NodeKind::Text
                    && n.character_data()
                        .is_some_and(|t| t.chars().any(|ch| !ch.is_ascii_whitespace()))
                {
                    *content_seen = true;
                }
                continue;
            }
            let Some(ns) = styles.get(&c) else { continue };
            let cp = ns.primary.clone();
            let d = cp.get_box().display;
            if d.is_none() {
                continue;
            }
            if d.is_contents() {
                self.children(styles, c, parent_variant, content_seen, eager);
                continue;
            }
            if plain_inline(&cp) {
                *content_seen = true;
                let (before, after) = (ns.before.clone(), ns.after.clone());
                let v = self.reparent(c, 0, None, &cp, parent_variant);
                let b = before.map(|b| self.reparent(c, 1, Some(&PseudoElement::Before), &b, &v));
                let a = after.map(|a| self.reparent(c, 2, Some(&PseudoElement::After), &a, &v));
                if let Some(s) = styles.get_mut(&c) {
                    s.first_line = Some(v.clone());
                    s.first_line_before = b;
                    s.first_line_after = a;
                }
                let mut inner_seen = true;
                self.children(styles, c, &v, &mut inner_seen, eager);
                continue;
            }
            if !*content_seen && in_flow_block_level(&cp) {
                // The first formatted line of the parent is the first line of this block.
                self.block(styles, c, Some(parent_variant.clone()), eager);
            }
            // Atomic inlines, floats, out-of-flow boxes and later blocks end the walk for this level (a float or an
            // out-of-flow box takes no part in the line; an atomic inline is content).
            if !(floating(&cp) || out_of_flow(&cp)) {
                *content_seen = true;
            }
        }
    }

    /// `::first-letter` of `node`: the eager pseudo style cascaded from the block's first-line style when it has one.
    pub fn first_letter(
        &mut self,
        styles: &mut HashMap<NodeId, NodeStyles>,
        node: NodeId,
        own: StyloArc<ComputedValues>,
    ) {
        let Some(ns) = styles.get(&node) else { return };
        if !is_block_container(&ns.primary) {
            return;
        }
        let style = match &ns.first_line {
            Some(fl) => {
                let fl = fl.clone();
                self.recascade(node, 3, &PseudoElement::FirstLetter, &own, &fl)
            }
            None => own,
        };
        if let Some(s) = styles.get_mut(&node) {
            s.first_letter = Some(style);
        }
    }

    pub fn finish(self) -> HashMap<(NodeId, u8), CachedFirst> {
        self.fresh
    }
}
