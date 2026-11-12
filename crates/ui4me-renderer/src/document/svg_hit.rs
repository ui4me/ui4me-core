//! F0907: per-shape hit testing inside an inline `<svg>`.
use super::*;

impl Document {
    /// Refine a hit on `node` when it is an inline `<svg>` with a painted picture: the shape
    /// element under the viewport point `(x, y)`, the `<svg>` itself when no shape reacts there,
    /// or nothing when the `<svg>` has `pointer-events: none`.
    pub(super) fn inline_svg_target(&mut self, node: NodeId, x: f64, y: f64) -> Option<NodeId> {
        if !self
            .pipeline
            .document()
            .local_name(node)
            .is_ok_and(|n| n == "svg")
        {
            return Some(node);
        }
        let Some(resources) = self.pipeline.resources().cloned() else {
            return Some(node);
        };
        if resources.inline_svg_transparent(node) {
            return None;
        }
        // The hosted profile treats a static inline SVG as one image element.
        // Shape-level event targeting is an optional donor capability only.
        if self.hosted {
            return Some(node);
        }
        let Some((cx, cy, cw, ch)) = DispatchHost::element_content_rect(self, self.id(), node)
            .ok()
            .flatten()
        else {
            return Some(node);
        };
        Some(
            resources
                .inline_svg_hit(node, (x - cx) as f32, (y - cy) as f32, cw as f32, ch as f32)
                .unwrap_or(node),
        )
    }
}
