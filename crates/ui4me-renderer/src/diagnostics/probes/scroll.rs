//! owner: 005-pipeline-diagnostics — added by M1 lane M (slice geometry vs the Chromium baseline)
//!
//! The `scroll` probe (schema §5: `{containers: [{node, scroll_x, scroll_y, scroll_width,
//! scroll_height, client_width, client_height}]}`, the root scroller included), derived from the
//! committed fragments (008) when the runtime supplies no scroll input of its own.
//!
//! Values follow CSSOM View §7 (`Element` extensions):
//!
//! * offsets (`scroll_x`/`scroll_y` = `scrollLeft`/`scrollTop`): 0. The slice has no scroll
//!   containers and no scrolling (offsets are 012's work, `overflow` is not in the slice's
//!   property set), so every box is `overflow: visible`.
//! * `client_width`/`client_height`: the padding box size, without scrollbars (the slice has
//!   none), pixel-snapped; 0 for an element without a box or with an inline box.
//! * `scroll_width`/`scroll_height`: the larger of the padding box and the box's scrollable
//!   overflow (CSS Overflow 3 §2.2: descendant border boxes, line boxes and text fragments, i.e.
//!   the text's content area, all `overflow: visible` so their own overflow propagates), measured
//!   from the padding box's top-left (overflow above or left of it is unreachable), pixel-snapped;
//!   0 for an element without a box or with an inline box.
//! * the root scroller (the Document entry) and the root element (CSSOM View: `clientWidth`/
//!   `scrollWidth` of the root element in no-quirks mode report the viewport): the viewport is
//!   the client area; the scroll size is the larger of the viewport and the root box's
//!   overflow, measured from the ICB origin.
//!
//! Pixel snapping is the size rounded half up, the integer `Element.scroll*`/`client*` values (see `snap`).

use super::element_geometry::{ScrollContainerInput, ScrollProbeInput};
use super::fragments::{FragmentInput, FragmentKind};
use crate::diagnostics::envelope::ProbeInputs;
use std::collections::HashMap;
use ui4me_engine_types::geometry::LayoutRect;
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Edges {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Edges {
    fn of(r: &LayoutRect) -> Edges {
        let (x, y) = (r.origin.x.to_f64(), r.origin.y.to_f64());
        Edges {
            left: x,
            top: y,
            right: x + r.size.width.to_f64(),
            bottom: y + r.size.height.to_f64(),
        }
    }
    fn union(self, o: Edges) -> Edges {
        Edges {
            left: self.left.min(o.left),
            top: self.top.min(o.top),
            right: self.right.max(o.right),
            bottom: self.bottom.max(o.bottom),
        }
    }
}

/// Own rect of a fragment: the border box of a box, the rect of a line or text fragment.
fn own(f: &FragmentInput) -> Option<Edges> {
    match f.kind {
        FragmentKind::Box => f.border.as_ref().map(Edges::of),
        FragmentKind::Line | FragmentKind::Text => f.rect.as_ref().map(Edges::of),
    }
}

/// A fragment's extent: its own rect and everything it overflows into, except behind a box that
/// clips its overflow (its descendants are reachable through its own scrolling only).
fn extent(f: &FragmentInput) -> Option<Edges> {
    if f.ink_only {
        return None;
    }
    if f.clips_overflow {
        return own(f);
    }
    let own = match (own(f), f.own_scrollable_overflow.as_ref().map(Edges::of)) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    };
    f.children
        .iter()
        .filter_map(extent)
        .fold(own, |acc, e| Some(acc.map_or(e, |a| a.union(e))))
}

/// The scrollable overflow of a box fragment, with its padding box (both document space).
fn box_overflow(f: &FragmentInput) -> Option<(Edges, Edges, Edges)> {
    // A table element owns two box fragments: the wrapper (captions and grid) and, as its child, the
    // table box that carries the borders and padding. The reference browser reports `clientWidth` of the
    // wrapper's border box but `scrollWidth` of the table box's padding box (borders excluded, measured:
    // 140 x 65 against 144 x 69 for a 2 px bordered table), and the table box's own border is no overflow.
    if let Some(grid) = f.owner.and_then(|o| {
        f.children
            .iter()
            .find(|c| c.kind == FragmentKind::Box && c.owner == Some(o))
    }) && let (Some(wrapper), Some(gb), Some(gp)) = (
        f.padding.as_ref(),
        grid.border.as_ref(),
        grid.padding.as_ref(),
    ) && wrapper == gb
        && gb != gp
    {
        let padding = Edges::of(wrapper);
        let grid_padding = Edges::of(gp);
        // A collapsed table starts at the grid padding edge but includes its outer border at
        // the opposite edge. Separate-border tables instead use the grid padding box on both
        // sides. Ink-only children are collapsed-border segments, not scrollable overflow.
        let base = if grid.children.iter().any(|c| c.ink_only) {
            Edges {
                left: if f.scroll_from_end.0 {
                    padding.left
                } else {
                    grid_padding.left
                },
                right: if f.scroll_from_end.0 {
                    grid_padding.right
                } else {
                    padding.right
                },
                top: if f.scroll_from_end.1 {
                    padding.top
                } else {
                    grid_padding.top
                },
                bottom: if f.scroll_from_end.1 {
                    grid_padding.bottom
                } else {
                    padding.bottom
                },
            }
        } else {
            grid_padding
        };
        let mut overflow = f
            .children
            .iter()
            .filter(|c| !std::ptr::eq(*c, grid))
            .filter_map(extent)
            .fold(base, Edges::union);
        if let Some((_, grid_overflow, _)) = box_overflow(grid) {
            overflow = overflow.union(grid_overflow);
        }
        return Some((padding, overflow, base));
    }
    let padding = Edges::of(f.padding.as_ref()?);
    // The rendered legend is outside the fieldset content scroll area. The anonymous
    // content box begins after it, while clientHeight still describes the outer padding box.
    let (base, mut overflow) = if f.fieldset_with_legend
        && let (Some(content), Some(body)) = (f.content.as_ref(), f.children.get(1))
        && let Some(body_border) = body.border.as_ref()
    {
        let content = Edges::of(content);
        let body_border = Edges::of(body_border);
        let mut base = padding;
        base.top += (body_border.top - content.top).max(0.0);
        let overflow = f
            .children
            .iter()
            .skip(1)
            .chain(&f.stitched)
            .filter_map(extent)
            .fold(base, Edges::union);
        (base, overflow)
    } else {
        (
            padding,
            f.children
                .iter()
                .chain(&f.stitched)
                .filter_map(extent)
                .fold(padding, Edges::union),
        )
    };
    // Replaced listbox row geometry can extend past its last text line; retain
    // that published extent rather than inferring it solely from glyphs.
    if let Some(own) = f.own_scrollable_overflow.as_ref() {
        overflow = overflow.union(Edges::of(own));
    }
    // A scroll container's scrollable overflow keeps the padding at the end of its content (block-end
    // and inline-end) once the content overflows its padding box (CSS Overflow 3 section 2.2).
    if f.clips_overflow
        && let Some(content) = f.content.as_ref().map(Edges::of)
    {
        if overflow.bottom > padding.bottom {
            overflow.bottom += padding.bottom - content.bottom;
        }
        if overflow.right > padding.right && !f.scroll_from_end.0 {
            overflow.right += padding.right - content.right;
        }
        if overflow.left < padding.left && f.scroll_from_end.0 {
            overflow.left -= content.left - padding.left;
        }
        if overflow.top < padding.top && f.scroll_from_end.1 {
            overflow.top -= content.top - padding.top;
        }
    }
    Some((padding, overflow, base))
}

/// The first box fragment of every owner (an element's own box precedes the anonymous boxes
/// it owns in tree order).
fn first_boxes<'a>(f: &'a FragmentInput, out: &mut HashMap<NodeId, &'a FragmentInput>) {
    if f.kind == FragmentKind::Box
        && let Some(owner) = f.owner
    {
        out.entry(owner).or_insert(f);
    }
    for c in &f.children {
        first_boxes(c, out);
    }
}

/// The integer `clientWidth` / `scrollWidth` family of the reference browser (Chrome 156/157, measured
/// on Debian 13 for block, flex item, table cell, inline-block and absolutely positioned boxes at
/// fractional offsets, L-GEO-r9): the size rounded half up, whatever the box's location. (Blink's older
/// `SnapSizeToPixel(size, location)` = `round(location + size) - round(location)` differs when the
/// location has a fraction: `0.75 + 10.5` gave 10 where the browser reports 11.)
fn snap(size: f64, _location: f64) -> f64 {
    size.round()
}

fn metrics(node: NodeId, padding: Edges, overflow: Edges, base: Edges) -> ScrollContainerInput {
    metrics_from(node, padding, overflow, base, (false, false))
}

/// [`metrics`] with the scroll origin at the right and/or bottom edge (015 WM-1: `vertical-rl`).
fn metrics_from(
    node: NodeId,
    padding: Edges,
    overflow: Edges,
    base: Edges,
    from_end: (bool, bool),
) -> ScrollContainerInput {
    let (w, h) = (padding.right - padding.left, padding.bottom - padding.top);
    let sw = (base.right - base.left).max(if from_end.0 {
        base.right - overflow.left
    } else {
        overflow.right - base.left
    });
    let sh = (base.bottom - base.top).max(if from_end.1 {
        base.bottom - overflow.top
    } else {
        overflow.bottom - base.top
    });
    ScrollContainerInput {
        node,
        scroll_x: 0.0,
        scroll_y: 0.0,
        scroll_width: snap(sw, padding.left),
        scroll_height: snap(sh, padding.top),
        client_width: snap(w, padding.left),
        client_height: snap(h, padding.top),
    }
}

/// CSSOM client and scroll sizes for an element's first box fragment.
/// The caller supplies the same fragment representation used by the diagnostics probe.
pub fn element_metrics(node: NodeId, root: &FragmentInput) -> Option<ScrollContainerInput> {
    let mut boxes = HashMap::new();
    first_boxes(root, &mut boxes);
    let fragment = boxes.get(&node)?;
    let (padding, overflow, base) = box_overflow(fragment)?;
    Some(metrics_from(
        node,
        padding,
        overflow,
        base,
        fragment.scroll_from_end,
    ))
}

/// The root scroller and every element of the export, from the committed fragments; `None`
/// without fragments or element list.
pub(crate) fn derive(inputs: &ProbeInputs) -> Option<ScrollProbeInput> {
    let root = inputs.fragments.as_ref()?;
    let elements = inputs.element_geometry.as_ref()?;
    let mut boxes = HashMap::new();
    first_boxes(root, &mut boxes);
    let env = &inputs.envelope.environment;
    let viewport = Edges {
        left: 0.0,
        top: 0.0,
        right: env.viewport_width,
        bottom: env.viewport_height,
    };
    let root_overflow = extent(root).map_or(viewport, |e| viewport.union(e));
    let root_scroller = |node| {
        metrics(
            node,
            viewport,
            Edges {
                left: 0.0,
                top: 0.0,
                ..root_overflow
            },
            viewport,
        )
    };
    let mut containers = vec![root_scroller(inputs.dom.node)];
    for e in elements {
        if root.owner == Some(e.node) {
            containers.push(root_scroller(e.node));
            continue;
        }
        let entry = boxes
            .get(&e.node)
            .and_then(|f| box_overflow(f).map(|o| (o, f.scroll_from_end)))
            .map(|((padding, overflow, base), from_end)| {
                metrics_from(e.node, padding, overflow, base, from_end)
            })
            .unwrap_or(ScrollContainerInput {
                node: e.node,
                scroll_x: 0.0,
                scroll_y: 0.0,
                scroll_width: 0.0,
                scroll_height: 0.0,
                client_width: 0.0,
                client_height: 0.0,
            });
        containers.push(entry);
    }
    Some(ScrollProbeInput { containers })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::geometry::layout_unit::LayoutUnit;
    use ui4me_engine_types::geometry::{LayoutPoint, LayoutSize};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> LayoutRect {
        let u = |v: f64| LayoutUnit::quantize(v).value;
        LayoutRect {
            origin: LayoutPoint { x: u(x), y: u(y) },
            size: LayoutSize {
                width: u(w),
                height: u(h),
            },
        }
    }

    fn leaf(kind: FragmentKind, r: LayoutRect) -> FragmentInput {
        FragmentInput {
            kind,
            owner: None,
            border: None,
            padding: None,
            content: None,
            rect: Some(r),
            text: None,
            own_scrollable_overflow: None,
            clips_overflow: false,
            ink_only: false,
            fieldset_with_legend: false,
            scroll_from_end: (false, false),
            stitched: Vec::new(),
            children: Vec::new(),
        }
    }

    #[test]
    fn replaced_control_uses_published_row_extent_and_respects_clipping() {
        let mut control = FragmentInput {
            border: Some(rect(0.0, 0.0, 50.0, 71.0)),
            padding: Some(rect(1.0, 1.0, 48.0, 69.0)),
            content: Some(rect(1.0, 1.0, 48.0, 69.0)),
            own_scrollable_overflow: Some(rect(1.0, 1.0, 48.0, 345.0)),
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let (_, scroll, base) = box_overflow(&control).unwrap();
        assert_eq!(scroll.bottom - base.top, 345.0);
        assert_eq!(
            extent(&control).unwrap().bottom,
            346.0,
            "visible overflow reaches the parent"
        );
        control.clips_overflow = true;
        assert_eq!(
            extent(&control).unwrap().bottom,
            71.0,
            "clipped overflow does not reach the parent"
        );
        let (_, scroll, base) = box_overflow(&control).unwrap();
        assert_eq!(
            scroll.bottom - base.top,
            345.0,
            "the control's own extent remains available"
        );
    }

    #[test]
    fn rendered_legend_is_excluded_from_fieldset_scroll_area() {
        let legend = FragmentInput {
            border: Some(rect(0.0, 0.0, 100.0, 20.0)),
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let body = FragmentInput {
            border: Some(rect(10.0, 28.0, 80.0, 20.0)),
            children: vec![leaf(FragmentKind::Text, rect(10.0, 28.0, 80.0, 20.0))],
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let mut fieldset = FragmentInput {
            padding: Some(rect(0.0, 2.0, 100.0, 52.0)),
            content: Some(rect(10.0, 10.0, 80.0, 38.0)),
            fieldset_with_legend: true,
            children: vec![legend, body],
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let (padding, overflow, base) = box_overflow(&fieldset).unwrap();
        assert_eq!(
            (padding.bottom - padding.top, overflow.bottom - base.top),
            (52.0, 34.0)
        );
        fieldset.children[1].children[0] = leaf(FragmentKind::Text, rect(10.0, 28.0, 80.0, 50.0));
        let (_, overflow, base) = box_overflow(&fieldset).unwrap();
        assert_eq!(
            overflow.bottom - base.top,
            58.0,
            "real descendant overflow remains scrollable"
        );
    }

    #[test]
    fn text_content_area_below_the_padding_box_is_scrollable_overflow() {
        // An h1-like box: 28px line box, 33px content area starting 3px above it.
        let line = FragmentInput {
            children: vec![leaf(FragmentKind::Text, rect(30.0, 27.0, 200.0, 33.0))],
            ..leaf(FragmentKind::Line, rect(30.0, 30.0, 320.0, 28.0))
        };
        let h1 = FragmentInput {
            kind: FragmentKind::Box,
            border: Some(rect(30.0, 30.0, 320.0, 28.0)),
            padding: Some(rect(30.0, 30.0, 320.0, 28.0)),
            rect: None,
            children: vec![line],
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let (padding, overflow, base) = box_overflow(&h1).unwrap();
        let m = metrics(
            NodeId::from_raw(ui4me_engine_types::ids::RawId {
                index: 1,
                generation: std::num::NonZeroU32::new(1).unwrap(),
            }),
            padding,
            overflow,
            base,
        );
        // Overflow above the padding box is unreachable; below it adds 2px.
        assert_eq!(
            (
                m.client_width,
                m.client_height,
                m.scroll_width,
                m.scroll_height
            ),
            (320.0, 28.0, 320.0, 30.0)
        );
    }

    #[test]
    fn scroll_containers_keep_end_padding_and_hide_their_content_from_ancestors() {
        // L-LAYFIX-r8 (layout-position overflow-modes `pd`, scroll-panel `nest`): a 100x50 scroll
        // container with 10px padding holding a 200x120 child scrolls 220x140; its own parent sees
        // only its border box.
        let child = FragmentInput {
            kind: FragmentKind::Box,
            border: Some(rect(10.0, 10.0, 200.0, 120.0)),
            padding: Some(rect(10.0, 10.0, 200.0, 120.0)),
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let sc = FragmentInput {
            kind: FragmentKind::Box,
            border: Some(rect(0.0, 0.0, 120.0, 70.0)),
            padding: Some(rect(0.0, 0.0, 120.0, 70.0)),
            content: Some(rect(10.0, 10.0, 100.0, 50.0)),
            own_scrollable_overflow: None,
            clips_overflow: true,
            ink_only: false,
            fieldset_with_legend: false,
            scroll_from_end: (false, false),
            stitched: Vec::new(),
            children: vec![child],
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let (padding, overflow, base) = box_overflow(&sc).unwrap();
        let m = metrics(
            NodeId::from_raw(ui4me_engine_types::ids::RawId {
                index: 1,
                generation: std::num::NonZeroU32::new(1).unwrap(),
            }),
            padding,
            overflow,
            base,
        );
        assert_eq!(
            (
                m.client_width,
                m.client_height,
                m.scroll_width,
                m.scroll_height
            ),
            (120.0, 70.0, 220.0, 140.0)
        );
        let outer = FragmentInput {
            kind: FragmentKind::Box,
            border: Some(rect(0.0, 0.0, 130.0, 80.0)),
            padding: Some(rect(0.0, 0.0, 130.0, 80.0)),
            children: vec![sc],
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let (_, o, _) = box_overflow(&outer).unwrap();
        assert_eq!(
            (o.right, o.bottom),
            (130.0, 80.0),
            "the clipped content does not reach the parent"
        );
    }

    #[test]
    fn ink_only_fragments_are_not_scrollable_overflow() {
        // L-GEO-r9: a collapsed-border segment of a table reaches half a border past its border box
        // (1158.5 for a box ending at 1158); that is ink, `scrollWidth` stays 1116.
        let seg = FragmentInput {
            kind: FragmentKind::Box,
            border: Some(rect(832.5, 100.0, 326.0, 1.0)),
            ink_only: true,
            fieldset_with_legend: false,
            scroll_from_end: (false, false),
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let table = FragmentInput {
            kind: FragmentKind::Box,
            border: Some(rect(42.0, 90.0, 1116.0, 200.0)),
            padding: Some(rect(42.0, 90.0, 1116.0, 200.0)),
            children: vec![seg],
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let (padding, overflow, base) = box_overflow(&table).unwrap();
        let m = metrics(
            NodeId::from_raw(ui4me_engine_types::ids::RawId {
                index: 1,
                generation: std::num::NonZeroU32::new(1).unwrap(),
            }),
            padding,
            overflow,
            base,
        );
        assert_eq!((m.client_width, m.scroll_width), (1116.0, 1116.0));
    }

    #[test]
    fn collapsed_table_scroll_uses_origin_border_and_keeps_real_overflow() {
        let node = NodeId::from_raw(ui4me_engine_types::ids::RawId {
            index: 1,
            generation: std::num::NonZeroU32::new(1).unwrap(),
        });
        let ink = FragmentInput {
            kind: FragmentKind::Box,
            ink_only: true,
            fieldset_with_legend: false,
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let grid = FragmentInput {
            kind: FragmentKind::Box,
            owner: Some(node),
            border: Some(rect(0.0, 0.0, 45.0, 20.0)),
            padding: Some(rect(0.5, 1.0, 42.5, 18.0)),
            children: vec![ink],
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let mut wrapper = FragmentInput {
            kind: FragmentKind::Box,
            owner: Some(node),
            padding: Some(rect(0.0, 0.0, 45.0, 20.0)),
            border: Some(rect(0.0, 0.0, 45.0, 20.0)),
            children: vec![grid],
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        };
        let (p, o, b) = box_overflow(&wrapper).unwrap();
        let m = metrics_from(node, p, o, b, wrapper.scroll_from_end);
        assert_eq!(
            (m.client_width, m.scroll_width, m.scroll_height),
            (45.0, 45.0, 19.0)
        );
        wrapper.scroll_from_end = (true, false);
        let (p, o, b) = box_overflow(&wrapper).unwrap();
        let m = metrics_from(node, p, o, b, wrapper.scroll_from_end);
        assert_eq!(m.scroll_width, 43.0);
        wrapper.children[0].children.push(FragmentInput {
            kind: FragmentKind::Box,
            border: Some(rect(-10.0, 0.0, 15.0, 10.0)),
            ..leaf(FragmentKind::Box, rect(0.0, 0.0, 0.0, 0.0))
        });
        let (p, o, b) = box_overflow(&wrapper).unwrap();
        let m = metrics_from(node, p, o, b, wrapper.scroll_from_end);
        assert_eq!(
            m.scroll_width, 53.0,
            "real descendant overflow remains reachable in RTL"
        );
        wrapper.scroll_from_end = (false, true);
        let (p, o, b) = box_overflow(&wrapper).unwrap();
        let m = metrics_from(node, p, o, b, wrapper.scroll_from_end);
        assert_eq!(
            m.scroll_height, 19.0,
            "bottom-origin writing modes use the opposite edge"
        );
    }

    #[test]
    fn sizes_snap_like_chromium() {
        assert_eq!(snap(20.9, 186.0), 21.0);
        assert_eq!(snap(10.5, 0.25), 11.0);
        // Measured in Chrome 157: the location does not matter (a 10.5px box at 0.5 and at 0.75 is 11).
        assert_eq!(snap(10.5, 0.5), 11.0);
        assert_eq!(snap(10.5, 0.75), 11.0);
        assert_eq!(snap(351.5, 391.5), 352.0);
        assert_eq!(snap(168.25, 208.25), 168.0);
    }
}
