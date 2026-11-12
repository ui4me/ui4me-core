//! owner: 008-layout-engine (T105, L-TBL-r4) — the table wrapper box and its captions.
//!
//! The wrapper is a custom Taffy node whose width is the table grid's (CSS 2.1 §17.4: the wrapper
//! is as wide as the table box, and a caption is laid out as a block box in that width). Its
//! children are stacked in `caption-side` order: top captions, the grid box, bottom captions.
//! A caption whose minimum content is wider than the grid widens the table (the grid is then laid
//! out at the caption's width). The wrapper carries the margins and the positioning of the table
//! element, so a parent block resolves auto margins around it like around any other box.

use taffy::Point;
use taffy::prelude::*;
use taffy::{
    Baselines, LayoutInput, LayoutOutput, LayoutPartialTree, OofCandidates, OofPositioningArea,
    RequestedAxis, RunMode, ScrollableOverflow, SizingMode,
};

use super::TableHost;
use crate::layout::coordinator::host_tree::{HostTree, resolve_lp};

impl HostTree {
    /// Resolved margins of node `n` (`auto` is 0 here: the parent resolves auto margins around the
    /// wrapper; captions do not centre themselves).
    fn margin_px(&self, n: usize, basis: f32) -> Rect<f32> {
        let m = &self.nodes[n].style.margin;
        let r = |v: &LengthPercentageAuto| match v.expand() {
            taffy::ExpandedLengthPercentageAuto::Length(l) => l,
            taffy::ExpandedLengthPercentageAuto::Percent(p) => p * basis,
            taffy::ExpandedLengthPercentageAuto::Calc(ptr) => self.calcs.resolve(ptr, basis),
            taffy::ExpandedLengthPercentageAuto::Auto => 0.0,
        };
        Rect {
            top: r(&m.top),
            right: r(&m.right),
            bottom: r(&m.bottom),
            left: r(&m.left),
        }
    }

    /// Layout of a table wrapper node (module docs).
    pub(crate) fn layout_table_wrapper(&mut self, w: usize, inputs: LayoutInput) -> LayoutOutput {
        let Some(host) = self.nodes[w].table.take() else {
            return LayoutOutput::HIDDEN;
        };
        let (grid, top, bottom) = match &*host {
            TableHost::Wrapper(d) => (d.grid, d.top.clone(), d.bottom.clone()),
            TableHost::Grid(_) => {
                self.nodes[w].table = Some(host);
                return LayoutOutput::HIDDEN;
            }
        };
        self.nodes[w].table = Some(host);
        let perform = inputs.run_mode == RunMode::PerformLayout;
        let basis_opt = inputs.parent_size.width;
        let basis = basis_opt.unwrap_or(0.0);
        let captions: Vec<usize> = top.iter().chain(bottom.iter()).copied().collect();

        // Width: the grid's, widened by the widest minimum caption.
        let mut cap_min = 0.0f32;
        for &c in &captions {
            let m = self.margin_px(c, basis);
            let probe = LayoutInput {
                run_mode: RunMode::ComputeSize,
                sizing_mode: SizingMode::InherentSize,
                axis: RequestedAxis::Horizontal,
                known_dimensions: Size::NONE,
                known_dimensions_are_definite: Size {
                    width: true,
                    height: true,
                },
                parent_size: Size {
                    width: basis_opt,
                    height: None,
                },
                available_space: Size {
                    width: AvailableSpace::MinContent,
                    height: AvailableSpace::MaxContent,
                },
                vertical_margins_are_collapsible: Line::FALSE,
            };
            let width = self.compute_child_layout(NodeId::from(c), probe).size.width;
            cap_min = cap_min.max(width + m.left + m.right);
        }
        let grid_probe = LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Horizontal,
            known_dimensions: Size {
                width: inputs.known_dimensions.width,
                height: None,
            },
            known_dimensions_are_definite: Size {
                width: true,
                height: true,
            },
            parent_size: inputs.parent_size,
            available_space: Size {
                width: inputs.available_space.width,
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        let grid_w = self
            .compute_child_layout(NodeId::from(grid), grid_probe)
            .size
            .width;
        let width = grid_w.max(cap_min);
        if !perform && inputs.axis == RequestedAxis::Horizontal {
            return LayoutOutput::from_outer_size(Size { width, height: 0.0 });
        }

        // Stack: top captions, the grid, bottom captions.
        let mode = if perform {
            RunMode::PerformLayout
        } else {
            RunMode::ComputeSize
        };
        let mut y = 0.0f32;
        let mut candidates = OofCandidates::new();
        let mut first_baseline = None;
        let caption_input = |cw: f32| LayoutInput {
            run_mode: mode,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions: Size {
                width: Some(cw),
                height: None,
            },
            known_dimensions_are_definite: Size {
                width: true,
                height: true,
            },
            parent_size: Size {
                width: Some(width),
                height: None,
            },
            available_space: Size {
                width: AvailableSpace::Definite(cw),
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        let mut place = |tree: &mut HostTree,
                         n: usize,
                         order: u32,
                         y: &mut f32,
                         caption: bool|
         -> Option<f32> {
            let (m, input) = if caption {
                let m = tree.margin_px(n, basis);
                let cw = (width - m.left - m.right).max(0.0);
                (m, caption_input(cw))
            } else {
                let grid_input = LayoutInput {
                    run_mode: mode,
                    sizing_mode: SizingMode::InherentSize,
                    axis: RequestedAxis::Both,
                    known_dimensions: Size {
                        width: Some(width),
                        height: inputs.known_dimensions.height.map(|h| (h - 0.0).max(0.0)),
                    },
                    known_dimensions_are_definite: Size {
                        width: true,
                        height: true,
                    },
                    parent_size: inputs.parent_size,
                    available_space: Size {
                        width: AvailableSpace::Definite(width),
                        height: AvailableSpace::MaxContent,
                    },
                    vertical_margins_are_collapsible: Line::FALSE,
                };
                (Rect::ZERO, grid_input)
            };
            *y += m.top;
            // Inside a fragmentation root (015 FRAG-2c): the grid's flow offset, for its row struts.
            if perform
                && let Some(f) = tree.frag.as_deref_mut()
                && let Some(&wabs) = f.abs.get(&w)
            {
                f.abs.insert(n, wabs + *y);
            }
            let mut out = tree.compute_child_layout(NodeId::from(n), input);
            if perform
                && !caption
                && let Some(f) = tree.frag.as_deref_mut()
                && f.leading_pushed.remove(&n)
            {
                // The first row moved past a break: with nothing of the table before it, the table moves.
                if *y <= m.top + crate::layout::fragmentation::EPS {
                    f.leading_pushed.insert(w);
                    if let Some(l) = f.lead_lack.get(&n).copied() {
                        f.lead_lack.insert(w, l);
                    }
                }
            }
            let (dx, dy) = tree.relative_offset(n, width);
            let loc = Point {
                x: m.left + dx,
                y: *y + dy,
            };
            let baseline = out.baselines.first.map(|b| loc.y + b);
            if perform {
                let (border, padding) = tree.node_edges(n, basis);
                let mut moved = out.oof_candidates.take();
                if !moved.is_empty() {
                    moved.translate(loc);
                    candidates.append(&mut moved);
                }
                tree.set_unrounded_layout(
                    NodeId::from(n),
                    &Layout {
                        order,
                        location: loc,
                        size: out.size,
                        scrollable_overflow_rect: out.scrollable_overflow_rect,
                        scrollbar_size: Size::ZERO,
                        border,
                        padding,
                        margin: Rect {
                            top: m.top,
                            right: m.right,
                            bottom: m.bottom,
                            left: m.left,
                        },
                    },
                );
            }
            *y += out.size.height + m.bottom;
            baseline
        };
        let mut order = 0u32;
        for &c in &top {
            place(self, c, order, &mut y, true);
            order += 1;
        }
        if let Some(b) = place(self, grid, order, &mut y, false) {
            first_baseline = Some(b);
        }
        order += 1;
        for &c in &bottom {
            place(self, c, order, &mut y, true);
            order += 1;
        }
        let mut out = LayoutOutput::from_sizes_and_baselines(
            Size { width, height: y },
            ScrollableOverflow::ZERO,
            Baselines::from_first(first_baseline),
        );
        if !candidates.is_empty() {
            out.oof_candidates = candidates;
            out.oof_positioning_area = Some(OofPositioningArea {
                size: Size { width, height: y },
                offset: Point::ZERO,
            });
        }
        let _ = resolve_lp;
        out
    }
}
