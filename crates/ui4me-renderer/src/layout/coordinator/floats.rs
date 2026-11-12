//! owner: 008-layout-engine (L-FLOAT-r10) — floats met in inline content (CSS 2.1 section 9.5).
//!
//! An inline root is a leaf of the Taffy tree, but it takes part in the block formatting context
//! of the block it sits in: its lines are shortened by the floats beside them and the floats in
//! its own text are placed in that context, which Taffy keeps (`BlockContext`). [`BfcSpace`] is the
//! IFC container's view of that context: coordinates in and out are the root's content-box space,
//! the context's are those of the root's border box.

use taffy::{BlockContext, Clear, FloatDirection, Size};

use crate::layout::ifc::container::{FloatBox, FloatClear, FloatSpace};
use crate::layout::ifc::interface::LineSlot;

pub(crate) struct BfcSpace<'a, 'b> {
    pub ctx: &'a mut BlockContext<'b>,
    /// Padding plus border to the left of the content box.
    pub x0: f32,
    /// Padding plus border above the content box.
    pub y0: f32,
}

fn clear_of(c: FloatClear) -> Clear {
    match c {
        FloatClear::None => Clear::None,
        FloatClear::Left => Clear::Left,
        FloatClear::Right => Clear::Right,
        FloatClear::Both => Clear::Both,
    }
}

impl FloatSpace for BfcSpace<'_, '_> {
    fn slot(&mut self, y: f32, height: f32) -> LineSlot {
        let s = self
            .ctx
            .find_content_slot(y + self.y0, height.max(0.0), Clear::None, None);
        LineSlot {
            inline_start: s.x - self.x0,
            inline_size: s.width.max(0.0),
            valid_height: s.height,
        }
    }

    fn place(&mut self, item: u64, b: FloatBox, min_y: f32) -> (f32, f32) {
        let dir = if b.left {
            FloatDirection::Left
        } else {
            FloatDirection::Right
        };
        let p = self.ctx.place_floated_box(
            item,
            Size {
                width: b.width,
                height: b.height,
            },
            min_y + self.y0,
            dir,
            clear_of(b.clear),
            false,
        );
        (p.x - self.x0, p.y - self.y0)
    }

    fn next_edge(&mut self, y: f32) -> Option<f32> {
        let here = self
            .ctx
            .find_content_slot(y + self.y0, 0.0, Clear::None, None);
        let seg = here.segment_id?;
        let next = self
            .ctx
            .find_content_slot(y + self.y0, 0.0, Clear::None, Some(seg));
        let edge = if next.segment_id.is_some() {
            next.y
        } else {
            self.ctx.cleared_threshold(Clear::Both)?
        };
        let edge = edge - self.y0;
        (edge > y + 1e-4).then_some(edge)
    }

    fn floats_bottom(&self) -> f32 {
        self.ctx.floated_content_height_contribution() - self.y0
    }

    fn clearance(&mut self, left: bool, right: bool) -> Option<f32> {
        let clear = match (left, right) {
            (true, true) => Clear::Both,
            (true, false) => Clear::Left,
            (false, true) => Clear::Right,
            (false, false) => return None,
        };
        self.ctx.cleared_threshold(clear).map(|y| y - self.y0)
    }
}
