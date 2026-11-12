//! owner: 005-pipeline-diagnostics, written only by 012-interaction (lane L-INT-r3; D-LINTr3-04)
//!
//! Scroll state as a pipeline input. The offsets are layout's plain [`ScrollOffsets`] value
//! (008 E008-3: no layout structure holds or reads it); the pipeline keeps the live instance
//! because the scene stage (S5) needs it, and InteractionRuntime is its only writer (through
//! `DispatchHost::scroll_set`). A change of offset notes `SCROLL` (`Reason::Scroll`, scene
//! stage only per the invalidation table) and advances the scroll epoch; the next update then
//! re-runs S5 over the committed style and fragments (see `update.rs`): no restyle, no
//! relayout, no new layout epoch (008 FR-066).
//!
//! Writes clamp against the *committed* fragments, so a caller flushes first when the DOM may
//! have changed (the host bridge does).

use crate::layout::coordinator::overflow::{
    ScrollApplied, ScrollError, ScrollOffsets, ScrollRange, ScrollTarget, scroll_geometry,
    scroll_range,
};
use crate::layout::fragment::{FragmentKind, FragmentTree, OverflowMode};
use crate::pipeline::reasons::{Reason, Subject};
use crate::pipeline::state::DocumentPipeline;
use ui4me_engine_types::ids::NodeId;

/// One scroll target as the interaction layer needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollEntry {
    pub target: ScrollTarget,
    pub offset: (f64, f64),
    pub range: ScrollRange,
    /// Scrollport size, CSS px.
    pub scrollport: (f64, f64),
    /// `overflow` lets the user scroll the axis (`auto` / `scroll`; the viewport unless `hidden`).
    pub user_x: bool,
    pub user_y: bool,
}

fn user_scrollable(m: OverflowMode) -> bool {
    matches!(m, OverflowMode::Auto | OverflowMode::Scroll)
}

fn entry(
    tree: &FragmentTree,
    offsets: &ScrollOffsets,
    target: ScrollTarget,
) -> Option<ScrollEntry> {
    let g = scroll_geometry(tree, target)?;
    let range = scroll_range(tree, target)?;
    let (ox, oy) = match target {
        // The viewport: `visible` has been turned into `auto` by layout's propagation.
        ScrollTarget::Viewport => (
            tree.viewport_scroll.overflow_x,
            tree.viewport_scroll.overflow_y,
        ),
        ScrollTarget::Node(n) => match &tree
            .get(crate::layout::coordinator::overflow::scroll_container_fragment(tree, n)?)
            .kind
        {
            FragmentKind::Box(b) => (b.extras.overflow_x, b.extras.overflow_y),
            _ => return None,
        },
    };
    Some(ScrollEntry {
        target,
        offset: offsets.offset(target),
        range,
        scrollport: (
            g.scrollport.size.width.to_f64(),
            g.scrollport.size.height.to_f64(),
        ),
        user_x: user_scrollable(ox),
        user_y: user_scrollable(oy),
    })
}

impl DocumentPipeline {
    /// The scroll offsets in force.
    pub fn scroll_offsets(&self) -> &ScrollOffsets {
        &self.scroll
    }

    /// Scroll state of one target against the committed fragments; `None` before the first
    /// publication or for an element that is not a scroll container.
    pub fn scroll_entry(&self, target: ScrollTarget) -> Option<ScrollEntry> {
        let c = self.committed.as_ref()?;
        entry(&c.layout.fragments, &self.scroll, target)
    }

    /// The scroll containers a scroll starting at element `from` may move: every scroll-container
    /// box fragment on the fragment-parent path (an out-of-flow box's parent is its containing
    /// block), innermost first, then the viewport.
    pub fn scroll_chain(&self, from: NodeId) -> Vec<ScrollEntry> {
        let Some(c) = self.committed.as_ref() else {
            return Vec::new();
        };
        let tree = &c.layout.fragments;
        let mut out = Vec::new();
        let mut at = tree.fragments_of(from).first().copied();
        while let Some(f) = at {
            let frag = tree.get(f);
            if let FragmentKind::Box(b) = &frag.kind
                && b.extras.is_scroll_container
                && let crate::layout::box_tree::BoxOwner::Element(n) = b.owner
                && let Some(e) = entry(tree, &self.scroll, ScrollTarget::Node(n))
            {
                out.push(e);
            }
            at = frag.parent;
        }
        out.extend(entry(tree, &self.scroll, ScrollTarget::Viewport));
        out
    }

    /// `scrollTo`: set the offset of `target`, clamped to the committed scrollable overflow and
    /// snapped to 1/64 px. A change notes `SCROLL` and advances the scroll epoch.
    pub fn scroll_to(
        &mut self,
        target: ScrollTarget,
        x: f64,
        y: f64,
    ) -> Result<ScrollApplied, ScrollError> {
        let Some(c) = self.committed.clone() else {
            return Err(ScrollError::NotScrollContainer);
        };
        let applied = self.scroll.scroll_to(&c.layout.fragments, target, x, y)?;
        if applied.changed {
            self.note_scroll_changed(target);
        }
        Ok(applied)
    }

    fn note_scroll_changed(&mut self, target: ScrollTarget) {
        self.scroll_epoch += 1;
        let subject = match target {
            ScrollTarget::Viewport => Subject::Document,
            ScrollTarget::Node(n) => Subject::Node(n),
        };
        self.note_input_reason(Reason::Scroll, subject);
    }
}
