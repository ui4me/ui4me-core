//! owner: 001-vertical-slice — created by 001 slice (R-28)
//!
//! Geometry value types (core-types.md §3; AD-15). `transform.rs` / `spatial.rs` are 010's pure
//! spatial-tree functions (R-18; D-075, additive).

pub mod layout_unit;
pub mod spatial;
pub mod transform;

use crate::ids::NodeKey;
use layout_unit::LayoutUnit;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LayoutPoint {
    pub x: LayoutUnit,
    pub y: LayoutUnit,
}

/// Width and height are never negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LayoutSize {
    pub width: LayoutUnit,
    pub height: LayoutUnit,
}

impl LayoutSize {
    /// Size as the difference of quantised edges, clamped at zero.
    pub fn from_edges(
        left: LayoutUnit,
        right: LayoutUnit,
        top: LayoutUnit,
        bottom: LayoutUnit,
    ) -> Self {
        LayoutSize {
            width: (right - left).max(LayoutUnit::ZERO),
            height: (bottom - top).max(LayoutUnit::ZERO),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LayoutRect {
    pub origin: LayoutPoint,
    pub size: LayoutSize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LayoutEdges {
    pub top: LayoutUnit,
    pub right: LayoutUnit,
    pub bottom: LayoutUnit,
    pub left: LayoutUnit,
}

/// Space tag carried by every published rect; y points down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CoordinateSpace {
    /// Origin at the initial containing block's top-left.
    Document,
    /// Document minus root scroll offset (equal to `Document` when not scrolled).
    Viewport,
    Local(NodeKey),
}

/// Device pixels; used only by raster and platform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DeviceRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
