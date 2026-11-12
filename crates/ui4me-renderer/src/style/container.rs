//! owner: 006-style-engine (015 CQ-3, lane L-CQ)
//!
//! Container-query sizes (plan A1 item 2; AD-10). The style stage owns one [`ContainerSizes`]
//! table per document: for every element whose computed `container-type` is not `normal` and that
//! generated a box in the last layout, the content-box width and height in CSS px that the style
//! passes use. `ElementAdapter::query_container_size` (adapter/element.rs) reads it, so Stylo's
//! `@container` matching and the `cq*` units see it; an element without an entry (never laid out,
//! no box, a non-atomic inline box) answers `None` for both axes, which makes a size query unknown
//! (false) and a `cq*` unit fall back to the small viewport size (CSS Conditional 5 section 3.3).
//!
//! The pipeline closes the loop (pipeline `feedback.rs`): after layout it hands the measured
//! container sizes to [`StyleBridge::update_container_sizes`]; every container whose entry
//! changed (a new container, a removed one, a different size, type or name) gets a subtree restyle
//! hint, and the next style pass recomputes exactly the elements that can depend on it. Invariant:
//! every styled element was computed with the current table entry of each container above it,
//! because an entry never changes without its whole subtree being restyled.
//!
//! Axes: an `inline-size` container exposes its inline size only (its width, or its height in a
//! vertical writing mode, 015 WM-1c), a `size` container both.

use crate::style::traverse::StyleBridge;
use crate::style::view::layout_groups::ContainerType;
use std::collections::HashMap;
use style::invalidation::element::restyle_hints::RestyleHint;
use ui4me_document::document::Document;
use ui4me_engine_types::ids::NodeId;

/// One query container's size as the style passes see it (CSS px; `None` = no size on that axis).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContainerSize {
    pub width: Option<f32>,
    pub height: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    size: ContainerSize,
    /// The `container-type` and `container-name` the entry was recorded for: a change of either
    /// re-evaluates the queries below the container even when the size stays.
    container_type: ContainerType,
    names: Vec<String>,
}

/// NodeId → size of every query container of the last layout (module docs).
#[derive(Clone, Debug, Default)]
pub struct ContainerSizes {
    map: HashMap<NodeId, Entry>,
}

impl ContainerSizes {
    /// The size the style passes use for `node`.
    pub fn get(&self, node: NodeId) -> Option<ContainerSize> {
        self.map.get(&node).map(|e| e.size)
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    /// The containers in the table, in an unspecified order.
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.map.keys().copied()
    }
}

/// One measured query container: the content-box size of the element's principal box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredContainer {
    pub node: NodeId,
    pub width: f32,
    pub height: f32,
}

/// What [`StyleBridge::update_container_sizes`] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerUpdate {
    /// Containers whose entry changed (added, removed or different), sorted; each got a subtree
    /// restyle hint when it is still styled.
    pub changed: Vec<NodeId>,
}

impl ContainerUpdate {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
    }
}

/// Quantise a measured length to 1/64 px (app units are 1/60 px; the layout's lengths are
/// already multiples of 1/64): sizes that differ below that never trigger another round.
fn quantise(v: f32) -> f32 {
    if v.is_finite() {
        (v.max(0.0) * 64.0).round() / 64.0
    } else {
        0.0
    }
}

impl StyleBridge {
    /// The container sizes the next style pass uses.
    pub fn container_sizes(&self) -> &ContainerSizes {
        &self.sidecars.containers
    }

    /// Whether the last published style has any query container (an element whose computed
    /// `container-type` is not `normal`) or the table still holds one: when neither, the feedback
    /// loop has nothing to compare.
    pub fn has_query_containers(&self) -> bool {
        if !self.sidecars.containers.is_empty() {
            return true;
        }
        let snap = self.snapshot();
        snap.nodes().any(|n| {
            snap.view(n)
                .is_some_and(|v| v.containment().container_type != ContainerType::Normal)
        })
    }

    /// Replace the table with the sizes measured by the last layout (`measured`: every element
    /// with a box whose computed `container-type` is not `normal`; others are ignored) and hint a
    /// subtree restyle at every container whose entry changed. Returns the changed containers; an
    /// empty result means the last style pass already used these sizes (the loop has converged).
    pub fn update_container_sizes(
        &mut self,
        doc: &Document,
        measured: impl IntoIterator<Item = MeasuredContainer>,
    ) -> ContainerUpdate {
        let snap = self.snapshot();
        let mut next: HashMap<NodeId, Entry> = HashMap::new();
        for m in measured {
            let Some(view) = snap.view(m.node) else {
                continue;
            };
            let c = view.containment();
            let (w, h) = (quantise(m.width), quantise(m.height));
            let size = match c.container_type {
                ContainerType::Normal => continue,
                // The inline axis of a vertical container is its physical height (015 WM-1c; Stylo maps the
                // physical size to the container's logical axes).
                ContainerType::InlineSize if view.inline_text().writing_mode.is_vertical() => {
                    ContainerSize {
                        width: None,
                        height: Some(h),
                    }
                }
                ContainerType::InlineSize => ContainerSize {
                    width: Some(w),
                    height: None,
                },
                ContainerType::Size => ContainerSize {
                    width: Some(w),
                    height: Some(h),
                },
            };
            next.insert(
                m.node,
                Entry {
                    size,
                    container_type: c.container_type,
                    names: c.container_names,
                },
            );
        }
        let old = &self.sidecars.containers.map;
        let mut changed: Vec<NodeId> = next
            .iter()
            .filter(|(n, e)| old.get(n) != Some(e))
            .map(|(n, _)| *n)
            .collect();
        changed.extend(old.keys().filter(|n| !next.contains_key(n)).copied());
        changed.sort_by_key(|n| (n.raw().index, n.raw().generation));
        self.sidecars.containers.map = next;
        let arena = doc.arena();
        for &n in &changed {
            if arena.contains(n) && arena.is_connected(n) {
                self.add_hint(doc, n, RestyleHint::restyle_subtree());
            }
        }
        ContainerUpdate { changed }
    }

    /// Forget every container size (tests; a fresh bridge starts empty).
    pub fn clear_container_sizes_for_testing(&mut self) {
        self.sidecars.containers.map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::quantise;

    #[test]
    fn sizes_are_quantised_to_a_64th_and_never_negative() {
        assert_eq!(quantise(100.0), 100.0);
        assert_eq!(quantise(100.004), 100.0);
        assert_eq!(quantise(-3.0), 0.0);
        assert_eq!(quantise(f32::NAN), 0.0);
        assert_eq!(quantise(10.5), 10.5);
    }
}
