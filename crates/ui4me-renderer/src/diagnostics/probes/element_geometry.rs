//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `element_geometry` probe input (neutral), from the runtime geometry query (R-18).
//! T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::{ExportCx, num, point_json, rect_json};
use serde_json::{Value, json};
use ui4me_engine_types::geometry::LayoutRect;
use ui4me_engine_types::ids::NodeId;

/// One box fragment's rects, space `layout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxRects {
    pub margin: LayoutRect,
    pub border: LayoutRect,
    pub padding: LayoutRect,
    pub content: LayoutRect,
}

/// Column-major 4×4 mapping `layout` to `viewport` space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    pub m: [f64; 16],
    pub is_2d: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElementGeometryInput {
    pub node: NodeId,
    pub rendered: bool,
    pub boxes: Vec<BoxRects>,
    pub to_viewport: Matrix,
    /// Border-box quads, viewport space: top-left, top-right, bottom-right, bottom-left.
    pub quads: Vec<[(f64, f64); 4]>,
    /// Viewport space.
    pub bounding: LayoutRect,
}

pub(crate) fn matrix_json(m: &Matrix) -> Value {
    json!({"m": m.m.iter().map(|v| num(*v)).collect::<Vec<_>>(), "is_2d": m.is_2d})
}

pub(crate) fn to_json(list: &[ElementGeometryInput], cx: &ExportCx) -> Value {
    let elements: Vec<Value> = list
        .iter()
        .map(|e| {
            let boxes: Vec<Value> = e
                .boxes
                .iter()
                .map(|b| json!({"margin": rect_json(&b.margin), "border": rect_json(&b.border), "padding": rect_json(&b.padding), "content": rect_json(&b.content)}))
                .collect();
            let quads: Vec<Value> = e.quads.iter().map(|q| Value::Array(q.iter().map(|(x, y)| point_json(*x, *y)).collect())).collect();
            json!({"node": cx.key(e.node), "rendered": e.rendered, "boxes": boxes, "to_viewport": matrix_json(&e.to_viewport),
                   "quads": quads, "bounding": rect_json(&e.bounding)})
        })
        .collect();
    json!({"elements": elements})
}

// ------------------------------------------------------------------ scroll (T052, additive)

/// One scroll container (schema §5 `scroll`); CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollContainerInput {
    pub node: NodeId,
    pub scroll_x: f64,
    pub scroll_y: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub client_width: f64,
    pub client_height: f64,
}

/// `scroll` probe input. The slice has no scrolling (offsets are 012's, overflow 008's): the
/// runtime supplies the root scroller only, with zero offsets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollProbeInput {
    pub containers: Vec<ScrollContainerInput>,
}

pub(crate) fn scroll_json(s: &ScrollProbeInput, cx: &ExportCx) -> Value {
    let containers: Vec<Value> = s
        .containers
        .iter()
        .map(|c| {
            json!({"node": cx.key(c.node), "scroll_x": num(c.scroll_x), "scroll_y": num(c.scroll_y),
                   "scroll_width": num(c.scroll_width), "scroll_height": num(c.scroll_height),
                   "client_width": num(c.client_width), "client_height": num(c.client_height)})
        })
        .collect();
    json!({"containers": containers})
}
