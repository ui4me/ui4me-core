//! owner: 008-layout-engine (L-INL-r2) — atomic inlines and replaced boxes in the host tree.
//!
//! * [`TreeAtomics`] is the [`AtomicHost`] an IFC container uses: `layout_atomic` lays an atomic
//!   inline out recursively through Taffy (ifc-interface.md §4), in `Intrinsic`, `Measure` and
//!   `Final` modes, and returns its margin box and its baseline; `place_atomic` writes the final
//!   position (margin box origin plus margins) into the node, relative to the inline root's
//!   content box.
//! * `compute_replaced` is the used-size algorithm for replaced and control boxes
//!   (`replaced::used_content_size`).
//! * `center_button_content` centres the `ButtonContent` box of a `button` vertically.
//!
//! Baselines (CSS 2.1 §10.8.1): an inline-block's baseline is the baseline of its last in-flow
//! line box, found structurally after its `PerformLayout`; an inline-block without line boxes, and
//! a replaced box without a baseline rule, have their bottom margin edge as baseline (`None`).
//! Atomics are always laid out with `PerformLayout`, even when only measured, because their
//! baseline needs the positions of their content; `inline-flex` and `inline-grid` take their
//! first baseline from the flex or grid algorithm. The final layout of a pass clears the cached
//! results of the subtree first so that the node layouts always belong to the last run.

use taffy::prelude::*;
use taffy::{LayoutInput, LayoutOutput, LayoutPartialTree, RequestedAxis, RunMode, SizingMode};

use super::calc::{quantize_input, resolve_calc_value};
use super::host_tree::HostTree;
use crate::layout::box_tree::{
    BoxSizing as CssBoxSizing, BoxStyle, LengthPercentage as CssLp, MarginValue, MaxSizeValue,
    SizeValue,
};
use crate::layout::ifc::container::{AtomicHost, FloatBox, FloatClear};
use crate::layout::ifc::interface::{
    AtomicContributionPx, AtomicMetrics, AtomicMode, AtomicResult,
};
use crate::layout::replaced::{Natural, SizeRequest, used_content_size};

/// A replaced or control box in the host tree.
#[derive(Clone, Debug)]
pub(crate) struct LeafNode {
    pub natural: Natural,
    pub image_pending: bool,
    /// Native checkbox/radio baselines lie on the border-box bottom, independent of margin.
    pub native_check_baseline: bool,
    /// A text control's shaped value or placeholder (`replaced/inner.rs`).
    pub inner: Option<std::sync::Arc<crate::layout::replaced::inner::InnerShaped>>,
    /// A drawn list bullet: the bullet's offset below the top of the box (`marker_symbol_metrics`).
    pub marker_bullet_y: Option<f32>,
}

// ---------------------------------------------------------------------------------------------
// CSS value resolution (all results quantised to 1/64, AD-15)

fn lp_val(v: &CssLp, basis: f32) -> f32 {
    match v {
        CssLp::Px(p) => quantize_input(*p),
        CssLp::Percent(p) => quantize_input(p * basis),
        CssLp::Calc(e) => quantize_input(resolve_calc_value(e, basis)),
    }
}

fn margin_val(v: &MarginValue, basis: f32) -> f32 {
    match v {
        MarginValue::Auto => 0.0,
        MarginValue::Lp(lp) => lp_val(lp, basis),
    }
}

fn lp_opt(v: &CssLp, basis: Option<f32>) -> Option<f32> {
    match v {
        CssLp::Px(p) => Some(quantize_input(*p)),
        CssLp::Percent(p) => basis.map(|b| quantize_input(p * b)),
        CssLp::Calc(e) if !e.has_percent() => Some(quantize_input(resolve_calc_value(e, 0.0))),
        CssLp::Calc(e) => basis.map(|b| quantize_input(resolve_calc_value(e, b))),
    }
}

fn size_opt(v: &SizeValue, basis: Option<f32>) -> Option<f32> {
    match v {
        SizeValue::Auto => None,
        SizeValue::Lp(lp) => lp_opt(lp, basis),
    }
}

fn max_opt(v: &MaxSizeValue, basis: Option<f32>) -> Option<f32> {
    match v {
        MaxSizeValue::None => None,
        MaxSizeValue::Lp(lp) => lp_opt(lp, basis),
    }
}

/// Resolved edges of a box: (top, right, bottom, left), percentages against `basis`.
fn edges(css: &BoxStyle, basis: f32) -> ([f32; 4], [f32; 4], [f32; 4]) {
    let m = [
        margin_val(&css.margin.top, basis),
        margin_val(&css.margin.right, basis),
        margin_val(&css.margin.bottom, basis),
        margin_val(&css.margin.left, basis),
    ];
    let p = [
        lp_val(&css.padding.top, basis),
        lp_val(&css.padding.right, basis),
        lp_val(&css.padding.bottom, basis),
        lp_val(&css.padding.left, basis),
    ];
    let b = [
        quantize_input(css.border.top),
        quantize_input(css.border.right),
        quantize_input(css.border.bottom),
        quantize_input(css.border.left),
    ];
    (m, p, b)
}

fn rect4(v: [f32; 4]) -> Rect<f32> {
    Rect {
        top: v[0],
        right: v[1],
        bottom: v[2],
        left: v[3],
    }
}

impl HostTree {
    /// Clear the cached results of `n` and everything below it.
    fn clear_subtree_cache(&mut self, n: usize) {
        use taffy::CacheTree;
        self.cache_clear(NodeId::from(n));
        let kids = self.nodes[n].children.clone();
        for k in kids {
            self.clear_subtree_cache(k);
        }
    }

    /// Baseline of the node's content from its border-box top, structurally (module docs).
    fn baseline_of(&self, c: usize) -> Option<f32> {
        let n = &self.nodes[c];
        if n.leaf.is_some() {
            return n.leaf_baseline;
        }
        if let Some(ifc) = &n.ifc {
            return ifc
                .last_baseline()
                .map(|b| b + n.layout.border.top + n.layout.padding.top);
        }
        if n.table.is_some() || matches!(n.style.display, Display::Flex | Display::Grid) {
            // `inline-flex` / `inline-grid`: the container's first baseline set (CSS Flexbox 1
            // §8.5, CSS Grid 1 §11.8), as Taffy computed it in the last full layout.
            return n.baselines.first;
        }
        let found = n
            .children
            .iter()
            .rev()
            .filter(|&&k| !self.nodes[k].out_of_flow)
            .find_map(|&k| {
                self.baseline_of(k)
                    .map(|b| self.nodes[k].layout.location.y + b)
            });
        // A `button` is a flex container in the reference browser: with no content line to take the
        // baseline from, it is synthesized at the bottom of the content box (CSS Flexbox 1 section 8.5),
        // not at the bottom margin edge (Bootstrap `.btn-close`: 26px line, not 30px; L-GEO-r9).
        if found.is_none() && n.button {
            return Some(n.layout.size.height - n.layout.border.bottom - n.layout.padding.bottom);
        }
        found
    }

    /// Border-box width of atomic `c` under `space` (an intrinsic run).
    fn atomic_intrinsic_width(&mut self, c: usize, space: AvailableSpace) -> f32 {
        let input = LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Horizontal,
            known_dimensions: Size::NONE,
            known_dimensions_are_definite: Size {
                width: true,
                height: true,
            },
            parent_size: Size::NONE,
            available_space: Size {
                width: space,
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        self.compute_child_layout(NodeId::from(c), input).size.width
    }

    /// `layout_atomic` for node `c` (module docs).
    pub(crate) fn layout_atomic_child(&mut self, c: usize, mode: AtomicMode) -> AtomicResult {
        let css = self.nodes[c]
            .css
            .clone()
            .expect("atomic nodes keep their CSS values");
        let is_leaf = self.nodes[c].leaf.is_some();
        match mode {
            AtomicMode::Intrinsic(_) if css.marker_outside => {
                // An outside marker hangs before the line start: no contribution (FR-082).
                AtomicResult::Contribution(AtomicContributionPx {
                    min_content: 0.0,
                    max_content: 0.0,
                })
            }
            AtomicMode::Intrinsic(_) => {
                // Percentages resolve against an indefinite containing block: zero for margins.
                let (m, _, _) = edges(&css, 0.0);
                let mh = m[1] + m[3];
                let min = self.atomic_intrinsic_width(c, AvailableSpace::MinContent);
                let max = self.atomic_intrinsic_width(c, AvailableSpace::MaxContent);
                AtomicResult::Contribution(AtomicContributionPx {
                    min_content: min + mh,
                    max_content: max + mh,
                })
            }
            AtomicMode::Measure { available } | AtomicMode::Final { available } => {
                let fin = matches!(mode, AtomicMode::Final { .. });
                let basis = available.is_finite().then_some(available);
                let (mut m, p, b) = edges(&css, basis.unwrap_or(0.0));
                let avail_border = basis.map(|a| (a - m[1] - m[3]).max(0.0));
                // Shrink-to-fit width of a non-replaced box with `width: auto` (CSS 2.1 §10.3.5).
                let mut known_w = None;
                if !is_leaf && matches!(css.width, SizeValue::Auto) {
                    let min_c = self.atomic_intrinsic_width(c, AvailableSpace::MinContent);
                    let max_c = self.atomic_intrinsic_width(c, AvailableSpace::MaxContent);
                    let w = match avail_border {
                        Some(a) => min_c.max(a).min(max_c),
                        None => max_c,
                    };
                    // min-width / max-width clamp the shrink-to-fit width.
                    let pb_h = p[1] + p[3] + b[1] + b[3];
                    let adj = |v: f32| {
                        if css.box_sizing == CssBoxSizing::ContentBox {
                            v + pb_h
                        } else {
                            v
                        }
                    };
                    let lo = size_opt(&css.min_width, basis).map(adj).unwrap_or(0.0);
                    let hi = max_opt(&css.max_width, basis)
                        .map(adj)
                        .unwrap_or(f32::INFINITY);
                    known_w = Some(w.min(hi).max(lo));
                }
                if fin {
                    self.clear_subtree_cache(c);
                }
                let space_w = match (known_w, avail_border) {
                    (Some(w), _) => AvailableSpace::Definite(w),
                    (None, Some(a)) => AvailableSpace::Definite(a),
                    (None, None) => AvailableSpace::MaxContent,
                };
                let input = LayoutInput {
                    run_mode: RunMode::PerformLayout,
                    sizing_mode: SizingMode::InherentSize,
                    axis: RequestedAxis::Both,
                    known_dimensions: Size {
                        width: known_w,
                        height: None,
                    },
                    known_dimensions_are_definite: Size {
                        width: true,
                        height: true,
                    },
                    parent_size: Size {
                        width: basis,
                        height: None,
                    },
                    available_space: Size {
                        width: space_w,
                        height: AvailableSpace::MaxContent,
                    },
                    vertical_margins_are_collapsible: Line::FALSE,
                };
                let mut out: LayoutOutput = self.compute_child_layout(NodeId::from(c), input);
                // An outside `::marker` (CSS Lists 3 section 3.1): its border box ends at the start
                // edge of the line, so its margin box is zero wide (a negative start margin of its
                // own width).
                if css.marker_outside && !is_leaf {
                    if css.direction == crate::layout::box_tree::Direction::Rtl {
                        m[1] = -out.size.width;
                    } else {
                        m[3] = -out.size.width;
                    }
                }
                let node = &mut self.nodes[c];
                // Out-of-flow descendants the atomic does not claim: the inline root hands them on
                // once the atomic is placed (`host_tree.rs`, `layout_inline_root`).
                node.atomic_oof = if fin {
                    out.oof_candidates.take()
                } else {
                    taffy::OofCandidates::new()
                };
                node.layout.size = out.size;
                node.layout.margin = rect4(m);
                node.layout.padding = rect4(p);
                node.layout.border = rect4(b);
                node.layout.scrollable_overflow_rect = out.scrollable_overflow_rect;
                // CSS 2.1 §10.8.1: an inline-block whose `overflow` is not `visible` has its bottom margin
                // edge as baseline.
                let clipped = {
                    use taffy::Overflow as O;
                    let o = node_overflow(&self.nodes[c].style);
                    (self.nodes[c].leaf.is_none() && !(matches!(o.0, O::Visible | O::Clip) && matches!(o.1, O::Visible | O::Clip)))
                        // Layout containment: no baseline (CSS Containment 2 section 3.2); the bottom margin edge is used.
                        || self.nodes[c].style.contain.contains(taffy::Contain::LAYOUT)
                };
                let baseline = if clipped { None } else { self.baseline_of(c) };
                let margin_height = out.size.height + m[0] + m[2];
                // A vertical line's dominant baseline is `central` (CSS Inline 3 section 4.2; 015 WM-1): the
                // atomic's central baseline (its last line's, else synthesized halfway between its margin
                // edges) sits on the central baseline of the line's strut, expressed as the alphabetic
                // baseline the line aligns on. An orthogonal atomic has no baseline in this frame.
                let n = &self.nodes[c];
                if n.parent_wm.is_vertical() {
                    let own = baseline
                        .filter(|_| n.outer.is_none())
                        .map(|b| b + m[0] - n.central)
                        .unwrap_or(margin_height / 2.0);
                    return AtomicResult::Metrics(AtomicMetrics {
                        margin_width: out.size.width + m[1] + m[3],
                        margin_height,
                        baseline: Some(own + n.strut_central),
                    });
                }
                AtomicResult::Metrics(AtomicMetrics {
                    margin_width: out.size.width + m[1] + m[3],
                    margin_height,
                    baseline: baseline.map(|b| b + m[0]),
                })
            }
        }
    }

    /// Used size of a replaced or control box (`replaced::used_content_size`), border box.
    pub(crate) fn compute_replaced(&mut self, i: usize, inputs: LayoutInput) -> LayoutOutput {
        let css = self.nodes[i]
            .css
            .clone()
            .expect("replaced nodes keep their CSS values");
        let leaf = self.nodes[i].leaf.clone().expect("replaced node");
        let parent_w = inputs.parent_size.width;
        let basis = parent_w.unwrap_or(0.0);
        let (_, p, b) = edges(&css, basis);
        let (pb_h, pb_v) = (p[1] + p[3] + b[1] + b[3], p[0] + p[2] + b[0] + b[2]);
        let content_box = css.box_sizing == CssBoxSizing::ContentBox;
        let adj =
            |v: Option<f32>, pb: f32| v.map(|v| if content_box { v } else { (v - pb).max(0.0) });
        let styled = inputs.sizing_mode == SizingMode::InherentSize;
        let mut width = if styled {
            adj(size_opt(&css.width, parent_w), pb_h)
        } else {
            None
        };
        let mut height = if styled {
            adj(size_opt(&css.height, inputs.parent_size.height), pb_v)
        } else {
            None
        };
        // A percentage width is compressible: it contributes nothing to a min-content size.
        if styled
            && inputs.known_dimensions.width.is_none()
            && matches!(css.width, SizeValue::Lp(CssLp::Percent(_)))
            && matches!(inputs.available_space.width, AvailableSpace::MinContent)
        {
            width = Some(0.0);
        }
        if let Some(k) = inputs.known_dimensions.width {
            width = Some((k - pb_h).max(0.0));
        }
        if let Some(k) = inputs.known_dimensions.height {
            height = Some((k - pb_v).max(0.0));
        }
        let (min_w, max_w, min_h, max_h) = if styled {
            (
                adj(size_opt(&css.min_width, parent_w), pb_h).unwrap_or(0.0),
                adj(max_opt(&css.max_width, parent_w), pb_h).unwrap_or(f32::INFINITY),
                adj(size_opt(&css.min_height, inputs.parent_size.height), pb_v).unwrap_or(0.0),
                adj(max_opt(&css.max_height, inputs.parent_size.height), pb_v)
                    .unwrap_or(f32::INFINITY),
            )
        } else {
            (0.0, f32::INFINITY, 0.0, f32::INFINITY)
        };
        // The containing block's width for content with a ratio only: the space offered, else the parent's
        // width, else the width of the nearest container being laid out (a flex item's base size is
        // measured without a parent size; CSS Sizing 3 §5.2: the stretch-fit inline size).
        let available_width = match inputs.available_space.width {
            AvailableSpace::Definite(w) => Some((w - pb_h).max(0.0)),
            _ => parent_w
                .or(self.container_width.last().copied().flatten())
                .map(|w| (w - pb_h).max(0.0)),
        };
        let mut natural = leaf.natural;
        if natural.ratio.is_none() {
            natural.ratio = css.aspect_ratio;
        }
        let (cw, ch) = used_content_size(&SizeRequest {
            width,
            height,
            min_width: min_w,
            max_width: max_w,
            min_height: min_h,
            max_height: max_h,
            natural,
            available_width,
        });
        // The baseline of a control with an inner text line: the line is centred in a taller box.
        self.nodes[i].leaf_baseline = if leaf.native_check_baseline {
            Some(ch + pb_v)
        } else {
            natural.baseline.map(|base| {
                let centre = match natural.inner_line {
                    Some(lh) if ch > lh => (ch - lh) / 2.0,
                    _ => 0.0,
                };
                b[0] + p[0] + centre + base
            })
        };
        LayoutOutput::from_outer_size(Size {
            width: cw + pb_h,
            height: ch + pb_v,
        })
    }

    /// Centre the `ButtonContent` child of button node `i` in the button's content box.
    pub(crate) fn center_button_content(
        &mut self,
        i: usize,
        inputs: LayoutInput,
        out: &LayoutOutput,
    ) {
        let Some(css) = self.nodes[i].css.clone() else {
            return;
        };
        let Some(&kid) = self.nodes[i].children.first() else {
            return;
        };
        let (_, p, b) = edges(&css, inputs.parent_size.width.unwrap_or(0.0));
        let content_h = (out.size.height - p[0] - p[2] - b[0] - b[2]).max(0.0);
        let free = content_h - self.nodes[kid].layout.size.height;
        if free > 0.0 {
            self.nodes[kid].layout.location.y += free / 2.0;
        }
    }
}

/// The IFC container's view of the host tree for one inline root.
pub(crate) struct TreeAtomics<'a> {
    pub tree: &'a mut HostTree,
    pub parent: usize,
}

impl TreeAtomics<'_> {
    fn child(&self, item: u64) -> Option<usize> {
        self.tree.nodes[self.parent]
            .atomics
            .iter()
            .find(|(b, _)| b.0 as u64 == item)
            .map(|(_, k)| *k)
    }
}

impl AtomicHost for TreeAtomics<'_> {
    fn text_services(&mut self) -> Option<&mut crate::text::fonts::TextServices> {
        // SAFETY: set by `layout_boxes` from its `&mut TextServices` for the duration of `compute_root_layout`, which
        // runs on this thread and uses no other access to the services; reset to null afterwards.
        unsafe { self.tree.text_ptr.as_mut() }
    }

    fn layout_atomic(&mut self, item: u64, mode: AtomicMode) -> AtomicResult {
        match self.child(item) {
            Some(c) => self.tree.layout_atomic_child(c, mode),
            None => AtomicResult::Metrics(AtomicMetrics {
                margin_width: 0.0,
                margin_height: 0.0,
                baseline: None,
            }),
        }
    }

    fn place_atomic(&mut self, item: u64, x: f32, y: f32) {
        if let Some(c) = self.child(item) {
            let node = &mut self.tree.nodes[c];
            node.layout.location = taffy::Point {
                x: x + node.layout.margin.left,
                y: y + node.layout.margin.top,
            };
        }
    }

    fn layout_float(&mut self, item: u64, available: f32, final_pass: bool) -> FloatBox {
        let Some(c) = self.child(item) else {
            return FloatBox::default();
        };
        let mode = if final_pass {
            AtomicMode::Final { available }
        } else {
            AtomicMode::Measure { available }
        };
        let AtomicResult::Metrics(m) = self.tree.layout_atomic_child(c, mode) else {
            return FloatBox::default();
        };
        let st = &self.tree.nodes[c].style;
        FloatBox {
            breakable: self.tree.nodes[c].frag_facts.breakable_float(),
            width: m.margin_width,
            height: m.margin_height,
            left: st.float == taffy::Float::Left,
            clear: match st.clear {
                taffy::Clear::None => FloatClear::None,
                taffy::Clear::Left => FloatClear::Left,
                taffy::Clear::Right => FloatClear::Right,
                taffy::Clear::Both => FloatClear::Both,
            },
        }
    }

    fn place_float_node(&mut self, item: u64, x: f32, y: f32) {
        self.place_atomic(item, x, y);
        // A breakable float (015 MC-2) in a fragmentation root: where it went, in flow-thread coordinates.
        if let Some(c) = self.child(item)
            && self.tree.nodes[c].frag_facts.breakable_float()
            && let Some(f) = self.tree.frag.as_deref_mut()
            && let Some(&base) = f.ifc_base.get(&self.parent)
        {
            let l = self.tree.nodes[c].layout;
            f.note_float(c, base + l.location.y, l.size.height);
        }
    }

    fn float_contribution(&mut self, item: u64) -> (f32, f32) {
        let Some(c) = self.child(item) else {
            return (0.0, 0.0);
        };
        match self.tree.layout_atomic_child(
            c,
            AtomicMode::Intrinsic(crate::layout::ifc::interface::IntrinsicKind::Max),
        ) {
            AtomicResult::Contribution(k) => (k.min_content, k.max_content),
            AtomicResult::Metrics(m) => (m.margin_width, m.margin_width),
        }
    }

    fn oof_inline_level(&self, item: u64) -> bool {
        let n = &self.tree.nodes[self.parent];
        n.inline_oof
            .iter()
            .find(|o| self.tree.nodes[o.node].box_id.0 as u64 == item)
            .is_none_or(|o| o.inline_level)
    }
}

fn node_overflow(style: &taffy::Style) -> (taffy::Overflow, taffy::Overflow) {
    (style.overflow.x, style.overflow.y)
}
