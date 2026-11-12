//! Element drag and drop inside a document (0.7 F0707). Declarative: an element with
//! `draggable="true"` (or inside one) starts a drag once the pointer, pressed on it, moves 4 CSS px.
//! The host gets `dragstart` (`element`: the source), `dragover` when the element under the
//! pointer changes (`element`: that target, `source`), `drop` on release over a target and
//! `dragend` (`source`, `dropped`, `target`) always last. Escape or the window losing the focus
//! cancels. While a drag runs, pointer moves do not reach the page (no hover churn, no text
//! selection). Elements are reported by their HTML `id` (the nearest identified ancestor).

use super::*;

const THRESHOLD: f64 = 4.0;

#[derive(Clone, Debug, Default)]
pub(super) struct DragState {
    /// Pressed on a draggable source: (source node, press point).
    pending: Option<(NodeId, f64, f64)>,
    /// A drag in progress: (source, current target id).
    active: Option<(NodeId, Option<String>)>,
}

impl Document {
    fn identified(&self, n: NodeId) -> Option<String> {
        let d = self.pipeline.document();
        d.arena().inclusive_ancestors(n).into_iter().find_map(|a| {
            d.get_attribute(a, "id")
                .ok()
                .flatten()
                .filter(|i| !i.is_empty())
                .map(str::to_owned)
        })
    }
    fn draggable_at(&mut self, x: f64, y: f64) -> Option<NodeId> {
        let id = self.id();
        let hit = self.hit_test(id, x, y).ok().flatten()?;
        let d = self.pipeline.document();
        d.arena().inclusive_ancestors(hit).into_iter().find(|&a| {
            d.get_attribute(a, "draggable")
                .ok()
                .flatten()
                .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        })
    }
    fn target_at(&mut self, x: f64, y: f64) -> Option<String> {
        let id = self.id();
        let hit = self.hit_test(id, x, y).ok().flatten()?;
        self.identified(hit)
    }
    fn drag_event(&mut self, value: Value) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(value);
        }
    }
    fn end_drag(&mut self, dropped_on: Option<String>) {
        if let Some((source, _)) = self.drag.active.take() {
            let src = self.identified(source);
            if let Some(t) = &dropped_on {
                self.drag_event(json!({"type":"drop","element":t,"source":src}));
            }
            self.drag_event(json!({"type":"dragend","element":src,"source":src,"dropped":dropped_on.is_some(),"target":dropped_on}));
        }
        self.drag.pending = None;
    }
    /// Handle drag-related input first. Returns whether the input belongs to a running drag and
    /// must not reach the page (pointer moves while dragging).
    pub(super) fn drag_input(&mut self, input: &Input) -> bool {
        match *input {
            Input::PointerButton {
                pressed: true,
                button: 0,
                x,
                y,
            } => {
                self.drag.active = None;
                self.drag.pending = self.draggable_at(x, y).map(|n| (n, x, y));
                false
            }
            Input::PointerMoved { x, y } => {
                if let Some((source, sx, sy)) = self.drag.pending
                    && self.drag.active.is_none()
                    && ((x - sx).powi(2) + (y - sy).powi(2)).sqrt() >= THRESHOLD
                {
                    let d = self.pipeline.document();
                    if !d.arena().contains(source) || !d.is_connected(source).unwrap_or(false) {
                        self.drag.pending = None;
                        return false;
                    }
                    let src = self.identified(source);
                    self.drag.active = Some((source, None));
                    self.clear_selection();
                    self.drag_event(
                        json!({"type":"dragstart","element":src,"source":src,"x":x,"y":y}),
                    );
                }
                if let Some((source, last)) = self.drag.active.clone() {
                    let target = self.target_at(x, y);
                    if target != last {
                        let src = self.identified(source);
                        self.drag_event(
                            json!({"type":"dragover","element":target,"source":src,"x":x,"y":y}),
                        );
                        self.drag.active = Some((source, target));
                    }
                    return true;
                }
                false
            }
            Input::PointerButton {
                pressed: false,
                button: 0,
                x,
                y,
            } => {
                if self.drag.active.is_some() {
                    let target = self.target_at(x, y);
                    self.end_drag(target);
                }
                self.drag.pending = None;
                false
            }
            Input::Key {
                ref key,
                pressed: true,
                ..
            } if key == "Escape" && self.drag.active.is_some() => {
                self.end_drag(None);
                true
            }
            Input::Focus(false) | Input::PointerLeft => {
                if matches!(input, Input::Focus(false)) && self.drag.active.is_some() {
                    self.end_drag(None);
                }
                false
            }
            _ => false,
        }
    }
    /// The identified element under a viewport point (OS file drops, 0.7 F0707).
    pub fn element_at(&mut self, x: f64, y: f64) -> Option<String> {
        self.target_at(x, y)
    }
    /// Whether a drag is pressed or running (document text selection stays out of it).
    pub(super) fn drag_owns_pointer(&self) -> bool {
        self.drag.pending.is_some() || self.drag.active.is_some()
    }
}
