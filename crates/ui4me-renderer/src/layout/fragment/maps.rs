//! owner: 008-layout-engine (T012) — `box_to_fragments` and `node_to_boxes` (fragment-tree.md §2).
//! Both are in content order; an element may own many boxes and a box many fragments.

use std::collections::HashMap;

use ui4me_engine_types::ids::NodeId;

use super::FragmentId;
use crate::layout::box_tree::BoxId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FragmentMaps {
    pub box_to_fragments: HashMap<BoxId, Vec<FragmentId>>,
    pub node_to_boxes: HashMap<NodeId, Vec<BoxId>>,
    /// The box fragments of the repeated copies of an element (a cell of a table header or footer repeated in
    /// the pieces of a broken table, 015 FRAG-2c / MC-2), in fragment order. Not in `node_to_fragments`.
    pub repeated: HashMap<NodeId, Vec<FragmentId>>,
    /// The rows and row groups of a repeated table header or footer (015 STAB-1, D-REL04-1): per element, the table
    /// piece each copy is in and the copy's rectangle (document space). Their own track fragments stay one per piece.
    pub repeated_tracks:
        HashMap<NodeId, Vec<(FragmentId, ui4me_engine_types::geometry::LayoutRect)>>,
}

impl FragmentMaps {
    pub fn fragments_of_box(&self, b: BoxId) -> &[FragmentId] {
        self.box_to_fragments
            .get(&b)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn boxes_of_node(&self, n: NodeId) -> &[BoxId] {
        self.node_to_boxes.get(&n).map(Vec::as_slice).unwrap_or(&[])
    }
}
