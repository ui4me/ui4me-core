//! owner: 008-layout-engine (T102–T105, L-TBL-r4) — the table grid box in the Taffy host tree.
//!
//! A table grid node is a custom Taffy node: [`HostTree::layout_table_grid`] answers every
//! `LayoutInput` kind the way Taffy's own algorithms do (a `MinContent` / `MaxContent` request gives
//! the intrinsic width, a definite one the used width, a known width lays the rows out at that
//! width), and a full pass places the cells with `set_unrounded_layout`.
//!
//! A pass: measure every cell's minimum and maximum width (the cells are flow-root block
//! containers or inline roots, measured by their own algorithms under `ContentSize`), merge them
//! into column constraints, resolve the table width, distribute it over the columns, lay every cell
//! out at its final width to learn its height and baseline, compute the row heights, then lay the
//! cells out again at their slot height with the content offset the `vertical-align` asks for
//! (added as extra top padding, like browsers' intrinsic padding).

use taffy::Point;
use taffy::prelude::*;
use taffy::{
    Baselines, LayoutInput, LayoutOutput, LayoutPartialTree, OofCandidates, OofPositioningArea,
    RequestedAxis, RunMode, ScrollableOverflow, SizingMode,
};

use super::height::{HCell, VAlign, content_offset, distribute_extra_height, row_heights};
use super::measures::{CellConstraint, SpecWidth, column_constraints, totals};
use super::width::{distribute_auto, distribute_fixed};
use super::{GridData, GridResult, TableHost};
use crate::layout::box_tree::{
    BoxSizing as CssBoxSizing, BoxStyle, LengthPercentage as CssLp, MaxSizeValue, SizeValue,
};
use crate::layout::coordinator::calc::{quantize_input, resolve_calc_value};
use crate::layout::coordinator::host_tree::{HostTree, resolve_lp};
use crate::text::paragraph::VerticalAlignSpec;

/// `Some(px)` for a `width`/`height`-like value: lengths always, percentages against `basis`.
fn size_px(v: &SizeValue, basis: Option<f32>) -> Option<f32> {
    match v {
        SizeValue::Auto => None,
        SizeValue::Lp(lp) => lp_px(lp, basis),
    }
}

fn max_px(v: &MaxSizeValue, basis: Option<f32>) -> Option<f32> {
    match v {
        MaxSizeValue::None => None,
        MaxSizeValue::Lp(lp) => lp_px(lp, basis),
    }
}

fn lp_px(lp: &CssLp, basis: Option<f32>) -> Option<f32> {
    match lp {
        CssLp::Px(p) => Some(quantize_input(*p)),
        CssLp::Percent(p) => basis.map(|b| quantize_input(p * b)),
        CssLp::Calc(e) if !e.has_percent() => Some(quantize_input(resolve_calc_value(e, 0.0))),
        CssLp::Calc(e) => basis.map(|b| quantize_input(resolve_calc_value(e, b))),
    }
}

fn valign(css: &BoxStyle) -> VAlign {
    match css.vertical_align {
        VerticalAlignSpec::Top => VAlign::Top,
        VerticalAlignSpec::Bottom => VAlign::Bottom,
        VerticalAlignSpec::Middle => VAlign::Middle,
        // Every other value acts as `baseline` (T103).
        _ => VAlign::Baseline,
    }
}

fn rect_h(r: &Rect<f32>) -> f32 {
    r.left + r.right
}
fn rect_v(r: &Rect<f32>) -> f32 {
    r.top + r.bottom
}

impl HostTree {
    /// Resolved (border, padding) of node `n` against `basis`.
    pub(crate) fn node_edges(&self, n: usize, basis: f32) -> (Rect<f32>, Rect<f32>) {
        let s = &self.nodes[n].style;
        let r = |v: &LengthPercentage| resolve_lp(v, basis, &self.calcs);
        (
            Rect {
                top: r(&s.border.top),
                right: r(&s.border.right),
                bottom: r(&s.border.bottom),
                left: r(&s.border.left),
            },
            Rect {
                top: r(&s.padding.top),
                right: r(&s.padding.right),
                bottom: r(&s.padding.bottom),
                left: r(&s.padding.left),
            },
        )
    }

    /// The `position: relative` offset of node `n` (CSS 2.1 §9.4.3): `left` wins over `right` and `top`
    /// over `bottom`; percentages of `left`/`right` resolve against `basis`, those of `top`/`bottom`
    /// against nothing (treated as `auto`). Taffy applies it for its own containers; the table
    /// container places cells and captions itself, so it applies it here.
    pub(crate) fn relative_offset(&self, n: usize, basis: f32) -> (f32, f32) {
        let s = &self.nodes[n].style;
        if s.position != Position::Relative {
            return (0.0, 0.0);
        }
        let px = |v: &LengthPercentageAuto, pct: bool| match v.expand() {
            taffy::ExpandedLengthPercentageAuto::Length(l) => Some(l),
            taffy::ExpandedLengthPercentageAuto::Percent(p) if pct => Some(p * basis),
            taffy::ExpandedLengthPercentageAuto::Calc(ptr) if pct => {
                Some(self.calcs.resolve(ptr, basis))
            }
            _ => None,
        };
        let rtl = s.direction == taffy::Direction::Rtl;
        let (l, r) = (px(&s.inset.left, true), px(&s.inset.right, true));
        let dx = if rtl {
            r.map(|r| -r).or(l)
        } else {
            l.or(r.map(|r| -r))
        }
        .unwrap_or(0.0);
        let dy = px(&s.inset.top, false)
            .or(px(&s.inset.bottom, false).map(|b| -b))
            .unwrap_or(0.0);
        (dx, dy)
    }

    /// The border-box width of cell `c` under an intrinsic constraint, ignoring its own `width`.
    fn measure_cell(&mut self, c: usize, space: AvailableSpace, basis: Option<f32>) -> f32 {
        let input = LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::ContentSize,
            axis: RequestedAxis::Horizontal,
            known_dimensions: Size::NONE,
            known_dimensions_are_definite: Size {
                width: true,
                height: true,
            },
            parent_size: Size {
                width: basis,
                height: None,
            },
            available_space: Size {
                width: space,
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        self.compute_child_layout(NodeId::from(c), input).size.width
    }

    /// Layout of a table grid node (module docs).
    pub(crate) fn layout_table_grid(&mut self, g: usize, inputs: LayoutInput) -> LayoutOutput {
        let Some(mut host) = self.nodes[g].table.take() else {
            return LayoutOutput::HIDDEN;
        };
        let out = match &mut *host {
            TableHost::Grid(data) => self.grid_pass(g, data, inputs),
            TableHost::Wrapper(_) => LayoutOutput::HIDDEN,
        };
        self.nodes[g].table = Some(host);
        out
    }

    /// The struts of the rows of table grid `g` inside a fragmentation root (015 FRAG-2c; see `grid_pass`):
    /// moves `row_y` and returns (repeated header height, repeated footer height, total strut).
    fn table_row_struts(
        &mut self,
        g: usize,
        data: &GridData,
        row_y: &mut [f32],
        heights: &[f32],
        collapsed: &[bool],
        sp_v: f32,
    ) -> (f32, f32, f32) {
        use crate::layout::fragmentation::{BreakAppeal, BreakKind, BreakToken, EPS};
        let Some(f) = self.frag.as_deref_mut() else {
            return (0.0, 0.0, 0.0);
        };
        let Some(&tabs) = f.abs.get(&g) else {
            return (0.0, 0.0, 0.0);
        };
        let n = row_y.len();
        // Pages repeat every `thead` / `tfoot` (015 PAGE-1); columns only those that avoid breaks inside.
        let (hdr, ftr) = if f.paged {
            (data.page_header_rows.clone(), data.page_footer_rows.clone())
        } else {
            (data.header_rows.clone(), data.footer_rows.clone())
        };
        let body = hdr.end.min(n)..if ftr.is_empty() { n } else { ftr.start.min(n) };
        if body.is_empty() {
            return (0.0, 0.0, 0.0);
        }
        // A header or footer is repeated when it takes at most a quarter of a fragmentainer (else the rows
        // would get too little room; the reference browser has a similar limit).
        // A continued piece starts with the table's top edge part (border half, spacing) and the header copy.
        let header_h = if hdr.is_empty() {
            0.0
        } else {
            row_y[body.start]
        };
        let footer_h = if ftr.is_empty() || ftr.end > n {
            0.0
        } else {
            row_y[ftr.end - 1] + heights[ftr.end - 1] + sp_v - row_y[ftr.start]
        };
        let frag_h = f.size_at(tabs + row_y[body.start.min(row_y.len().saturating_sub(1))]);
        let header_h = if header_h <= frag_h / 4.0 {
            header_h
        } else {
            0.0
        };
        let footer_h = if footer_h <= frag_h / 4.0 {
            footer_h
        } else {
            0.0
        };
        let mut shift = 0.0f32;
        for r in body.clone() {
            row_y[r] += shift;
            if collapsed[r] {
                continue;
            }
            let h = heights[r];
            let top = tabs + row_y[r];
            if f.measure {
                f.tallest = f.tallest.max(h);
            }
            let start = f.fragmentainer_start(top);
            let boundary = f.next_boundary(top);
            let at_top = top <= start + header_h + EPS;
            if top + h + footer_h <= boundary + EPS || at_top {
                continue;
            }
            if r == body.start {
                // The first row does not fit: the break goes before the table (no piece of only a header).
                f.leading_pushed.insert(g);
                if f.measure {
                    f.lead_lack.insert(g, top + h + footer_h - boundary);
                }
                break;
            }
            f.note_shortage(top, h + footer_h, boundary);
            let d = boundary + header_h - top;
            shift += d;
            row_y[r] += d;
            f.tokens.push(BreakToken {
                node: g,
                offset: boundary,
                kind: BreakKind::PushedBox,
                appeal: BreakAppeal::Perfect,
            });
        }
        for y in row_y.iter_mut().take(n).skip(body.end) {
            *y += shift;
        }
        if shift <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        (header_h, footer_h, shift)
    }

    #[allow(clippy::too_many_lines)]
    fn grid_pass(&mut self, g: usize, data: &mut GridData, inputs: LayoutInput) -> LayoutOutput {
        let css: std::sync::Arc<BoxStyle> = self.nodes[g]
            .css
            .clone()
            .expect("a table grid keeps its CSS values");
        let perform = inputs.run_mode == RunMode::PerformLayout;
        let parent_w = inputs.parent_size.width;
        let basis = parent_w.unwrap_or(0.0);
        let n_cols = data.structure.column_count;
        let n_rows = data.structure.row_count;
        let (sp_h, sp_v) = data.spacing;

        // Edges. The collapsed model has no table padding; its border was resolved with the grid.
        let border = data.border;
        let padding = if data.collapse {
            Rect::ZERO
        } else {
            self.node_edges(g, basis).1
        };
        let spacing_h = if n_cols > 0 {
            sp_h * (n_cols as f32 + 1.0)
        } else {
            0.0
        };
        // A collapsed row is removed from the layout, with its border-spacing (CSS 2.1 §17.5.5).
        let vis_rows = data.rows.iter().filter(|r| !r.collapsed).count();
        let spacing_v = if vis_rows > 0 {
            sp_v * (vis_rows as f32 + 1.0)
        } else {
            0.0
        };
        let extras_h = rect_h(&border) + rect_h(&padding) + spacing_h;
        let extras_v = rect_v(&border) + rect_v(&padding) + spacing_v;

        // Reset the cells' padding (the vertical alignment of a previous pass added to it).
        for (k, &n) in data.cells.iter().enumerate() {
            self.nodes[n].style.padding = data.cell_padding[k];
        }

        // ---- Column constraints.
        let mut constraints = Vec::with_capacity(data.cells.len());
        for (k, &n) in data.cells.iter().enumerate() {
            let pc = data.structure.cells[k];
            let min = self.measure_cell(n, AvailableSpace::MinContent, parent_w);
            let max = self.measure_cell(n, AvailableSpace::MaxContent, parent_w);
            let cell_css = self.nodes[n].css.as_deref();
            let (cb, cp) = self.node_edges(n, basis);
            let pb_h = rect_h(&cb) + rect_h(&cp);
            let spec = match cell_css.map(|s| (&s.width, s.box_sizing)) {
                Some((SizeValue::Lp(CssLp::Percent(p)), _)) => SpecWidth::Percent(p * 100.0),
                Some((w, bs)) => match size_px(w, None) {
                    Some(x) => SpecWidth::Fixed(if bs == CssBoxSizing::ContentBox {
                        x + pb_h
                    } else {
                        x
                    }),
                    None => SpecWidth::Auto,
                },
                None => SpecWidth::Auto,
            };
            // `min-width` on a cell is a floor of its column (browsers honour it, CSS 2.1 leaves it undefined).
            let min = match cell_css.map(|s| (&s.min_width, s.box_sizing)) {
                Some((mw, bs)) => size_px(mw, None).map_or(min, |x| {
                    min.max(if bs == CssBoxSizing::ContentBox {
                        x + pb_h
                    } else {
                        x
                    })
                }),
                None => min,
            };
            let max = match spec {
                SpecWidth::Fixed(w) => min.max(w),
                _ => max.max(min),
            };
            constraints.push(CellConstraint {
                col: pc.col,
                span: pc.col_span,
                min,
                max,
                spec,
            });
        }
        let col_specs: Vec<SpecWidth> = data.columns.iter().map(|c| c.spec).collect();
        let cols = column_constraints(n_cols, &col_specs, &constraints);

        // ---- Table width.
        let adj_h = |v: f32| {
            if css.box_sizing == CssBoxSizing::ContentBox {
                v + rect_h(&border) + rect_h(&padding)
            } else {
                v
            }
        };
        let spec_w = match &css.width {
            SizeValue::Lp(CssLp::Percent(p)) => parent_w.map(|w| quantize_input(p * w)),
            v => size_px(v, parent_w),
        }
        .map(adj_h);
        let use_fixed = data.fixed_layout && spec_w.is_some();
        // Fixed layout: widths from the `col` elements, else from the first row's cells.
        let fixed_specs: Vec<SpecWidth> = if use_fixed {
            let mut specs = col_specs.clone();
            for (k, c) in constraints.iter().enumerate() {
                let pc = data.structure.cells[k];
                if pc.row != 0 {
                    continue;
                }
                let share = match c.spec {
                    SpecWidth::Fixed(w) => SpecWidth::Fixed(w / pc.col_span as f32),
                    SpecWidth::Percent(p) => SpecWidth::Percent(p / pc.col_span as f32),
                    SpecWidth::Auto => continue,
                };
                for s in specs.iter_mut().skip(pc.col).take(pc.col_span) {
                    if *s == SpecWidth::Auto {
                        *s = share;
                    }
                }
            }
            specs
        } else {
            Vec::new()
        };
        let (min_sum, max_sum) = if use_fixed {
            let fixed: f32 = fixed_specs
                .iter()
                .map(|s| if let SpecWidth::Fixed(w) = s { *w } else { 0.0 })
                .sum();
            (fixed, fixed)
        } else {
            totals(&cols)
        };
        let table_min = min_sum + extras_h;
        let table_max = max_sum + extras_h;
        let min_w = size_px(&css.min_width, parent_w).map(adj_h);
        let max_w = max_px(&css.max_width, parent_w).map(adj_h);
        let clamp_w = |w: f32| {
            let mut w = w;
            if let Some(m) = max_w {
                w = w.min(m);
            }
            if let Some(m) = min_w {
                w = w.max(m);
            }
            w.max(table_min)
        };
        let used = match inputs.known_dimensions.width {
            Some(k) => k.max(table_min),
            None => match (inputs.available_space.width, spec_w) {
                (
                    AvailableSpace::MinContent
                    | AvailableSpace::MaxContent
                    | AvailableSpace::Definite(_),
                    Some(w),
                ) => clamp_w(w),
                (AvailableSpace::MinContent, None) => clamp_w(table_min),
                (AvailableSpace::MaxContent, None) => clamp_w(table_max),
                (AvailableSpace::Definite(a), None) => clamp_w(table_max.min(a.max(table_min))),
            },
        };

        // ---- Column widths.
        let content_w = (used - extras_h).max(0.0);
        let mut widths = if use_fixed {
            distribute_fixed(&fixed_specs, content_w)
        } else {
            distribute_auto(&cols, content_w)
        };
        // Column widths are LayoutUnits (1/64 px) truncated toward zero, and the last column takes the
        // rounding remainder, so the columns still add up to the distributed width (Chromium's table
        // algorithm; the same totals as rounding, the split differs by at most 1/64 px).
        let total_before: f32 = widths.iter().sum();
        for w in &mut widths {
            *w = ((*w * 64.0) + 1e-3).floor() / 64.0;
        }
        if (total_before - content_w).abs() < 1.0 / 64.0 {
            let drift = quantize_input(content_w - widths.iter().sum::<f32>());
            if let Some(last) = widths.last_mut() {
                *last += drift.max(0.0);
            }
        }
        let used = used.max(widths.iter().sum::<f32>() + extras_h);
        if !perform && inputs.axis == RequestedAxis::Horizontal {
            return LayoutOutput::from_outer_size(Size {
                width: used,
                height: 0.0,
            });
        }

        // ---- Cells at their final width: height and baseline.
        let span_w = |col: usize, span: usize| {
            widths[col..col + span].iter().sum::<f32>() + sp_h * (span as f32 - 1.0)
        };
        let cell_input = |w: f32, h: Option<f32>, used: f32| LayoutInput {
            run_mode: RunMode::PerformLayout,
            sizing_mode: SizingMode::ContentSize,
            axis: RequestedAxis::Both,
            known_dimensions: Size {
                width: Some(w),
                height: h,
            },
            // A stretched cell's height depends on its row: percentages of its children see auto.
            known_dimensions_are_definite: Size {
                width: true,
                height: false,
            },
            parent_size: Size {
                width: Some(used),
                height: None,
            },
            available_space: Size {
                width: AvailableSpace::Definite(w),
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        let mut hcells = Vec::with_capacity(data.cells.len());
        let mut measured = Vec::with_capacity(data.cells.len());
        for (k, &n) in data.cells.iter().enumerate() {
            let pc = data.structure.cells[k];
            let w = span_w(pc.col, pc.col_span);
            // The measuring layout of a cell is a function of the cell's subtree, its width and the grid's
            // width (its padding is the base one here): its result is kept (`CellMemo`, 015 PERF-1). Skipping
            // the layout also keeps the final-layout cache entry, which the measuring layout would replace.
            let (height, first) = match self.nodes[n].cell_memo {
                Some(m) if m.width == w.to_bits() && m.used == used.to_bits() => {
                    (m.height, m.baseline)
                }
                _ => {
                    let out = self.compute_child_layout(NodeId::from(n), cell_input(w, None, used));
                    let m = crate::layout::table::CellMemo {
                        width: w.to_bits(),
                        used: used.to_bits(),
                        height: out.size.height,
                        baseline: out.baselines.first,
                        final_top: None,
                    };
                    self.nodes[n].cell_memo = Some(m);
                    (m.height, m.baseline)
                }
            };
            let (cb, cp) = self.node_edges(n, used);
            // The baseline of a cell without a line is the bottom of its content edge (CSS 2.1 §17.5.3).
            let baseline = first.unwrap_or((height - cb.bottom - cp.bottom).max(0.0));
            let cell_css = self.nodes[n].css.as_deref();
            let spec_height = cell_css.and_then(|s| {
                size_px(&s.height, None).map(|h| {
                    if s.box_sizing == CssBoxSizing::ContentBox {
                        h + rect_v(&cb) + rect_v(&cp)
                    } else {
                        h
                    }
                })
            });
            let align = cell_css.map_or(VAlign::Baseline, valign);
            measured.push((height, baseline));
            hcells.push(HCell {
                row: pc.row,
                row_span: pc.row_span,
                height,
                baseline,
                align,
                spec_height,
            });
        }

        // ---- Row heights.
        let row_specs: Vec<Option<f32>> = data.rows.iter().map(|r| r.spec_height).collect();
        let mut rh = row_heights(n_rows, &row_specs, &hcells, sp_v);
        let collapsed: Vec<bool> = (0..n_rows)
            .map(|r| data.rows.get(r).is_some_and(|i| i.collapsed))
            .collect();
        for (h, c) in rh.heights.iter_mut().zip(&collapsed) {
            if *c {
                *h = 0.0;
            }
        }
        let rows_sum: f32 = rh.heights.iter().sum();
        let content_h = rows_sum + extras_v;
        let adj_v = |v: f32| {
            if css.box_sizing == CssBoxSizing::ContentBox {
                v + rect_v(&border) + rect_v(&padding)
            } else {
                v
            }
        };
        let spec_h = inputs
            .known_dimensions
            .height
            .or_else(|| size_px(&css.height, inputs.parent_size.height).map(adj_v));
        let min_h = size_px(&css.min_height, inputs.parent_size.height).map(adj_v);
        let target_h = spec_h
            .unwrap_or(0.0)
            .max(min_h.unwrap_or(0.0))
            .max(content_h);
        if vis_rows > 0 && target_h > content_h {
            // Collapsed rows stay at 0; the extra goes to the visible rows without a specified height.
            let eligible: Vec<bool> = rh
                .constrained
                .iter()
                .zip(&collapsed)
                .map(|(k, c)| !*k && !*c)
                .collect();
            let eligible = if eligible.iter().any(|e| *e) {
                eligible
            } else {
                collapsed.iter().map(|c| !*c).collect()
            };
            distribute_extra_height(&mut rh.heights, &eligible, target_h - content_h);
        }
        for h in &mut rh.heights {
            *h = quantize_input(*h);
        }
        let total_h = (rh.heights.iter().sum::<f32>() + extras_v).max(target_h);

        // ---- Positions.
        let x0 = border.left + padding.left + sp_h;
        let y0 = border.top + padding.top + sp_v;
        let mut col_x = Vec::with_capacity(n_cols);
        let mut x = x0;
        for w in &widths {
            col_x.push(x);
            x += w + sp_h;
        }
        let mut row_y = Vec::with_capacity(n_rows);
        let mut y = y0;
        for (h, c) in rh.heights.iter().zip(&collapsed) {
            row_y.push(y);
            if !*c {
                y += h + sp_v;
            }
        }
        // Inside a fragmentation root (015 FRAG-2c): rows are the unbreakable units of a table (breaks inside
        // cells are deferred). A row that would cross a fragmentainer boundary moves to the next one, below the
        // repeated header, leaving room for the repeated footer; a row taller than a fragmentainer overflows it.
        let (repeat_header, repeat_footer, strut_total) = if perform {
            self.table_row_struts(g, data, &mut row_y, &rh.heights, &collapsed, sp_v)
        } else {
            (0.0, 0.0, 0.0)
        };
        let total_h = total_h + strut_total;

        // ---- First baseline of the table: the first row's (CSS 2.1 §17.5.3, `inline-table`).
        let first_baseline = if n_rows > 0 {
            rh.baselines[0].map(|b| row_y[0] + b).or_else(|| {
                // No baseline-aligned cell in the first row: the reference browser takes the bottom of the first
                // cell's content edge in the row (row bottom less the cell's bottom border and padding), whatever the
                // cell's own alignment (measured: 22 for a 24 px row of 2 px padded `middle` cells, L-LAY).
                data.structure
                    .cells
                    .iter()
                    .position(|c| c.row == 0)
                    .map(|k| {
                        let (cb, cp) = self.node_edges(data.cells[k], used);
                        row_y[0] + rh.heights[0] - cb.bottom - cp.bottom
                    })
            })
        } else {
            None
        };

        let mut out = LayoutOutput::from_sizes_and_baselines(
            Size {
                width: used,
                height: total_h,
            },
            ScrollableOverflow::ZERO,
            Baselines::from_first(first_baseline),
        );
        if !perform {
            return out;
        }

        // ---- Final layout of the cells.
        let mut candidates = OofCandidates::new();
        for (k, &n) in data.cells.iter().enumerate() {
            let pc = data.structure.cells[k];
            let w = span_w(pc.col, pc.col_span);
            let spanned = pc.row..pc.row + pc.row_span;
            let visible = collapsed[spanned.clone()].iter().filter(|c| !**c).count();
            let slot_h =
                rh.heights[spanned].iter().sum::<f32>() + sp_v * (visible.max(1) as f32 - 1.0);
            let (mh, mb) = measured[k];
            let offset = content_offset(hcells[k].align, slot_h, mh, mb, rh.baselines[pc.row]);
            let base_top = resolve_lp(&data.cell_padding[k].top, used, &self.calcs);
            let top = quantize_input(base_top + offset);
            self.nodes[n].style.padding.top = LengthPercentage::length(top);
            // The cached final layout of the cell holds only for the top padding it was computed with.
            match self.nodes[n].cell_memo.as_mut() {
                Some(m) if m.final_top == Some(top.to_bits()) => {}
                Some(m) => {
                    m.final_top = Some(top.to_bits());
                    self.nodes[n].cache.store(
                        &crate::layout::coordinator::replay::UNMATCHABLE_FINAL,
                        taffy::LayoutOutput::HIDDEN,
                    );
                }
                None => {}
            }
            let mut cout =
                self.compute_child_layout(NodeId::from(n), cell_input(w, Some(slot_h), used));
            let mut moved = cout.oof_candidates.take();
            if !moved.is_empty() {
                // The static position of an out-of-flow child is the cell's content edge without the
                // intrinsic padding of the vertical alignment (WPT table-cell-baseline-static-position).
                moved.translate(Point {
                    x: col_x[pc.col],
                    y: row_y[pc.row] - quantize_input(offset),
                });
                candidates.append(&mut moved);
            }
            let (cb, mut cp) = self.node_edges(n, used);
            // The alignment offset is intrinsic padding: it positions the content but is not part of
            // the cell's padding box edges (CSSOM boxes, `clientHeight`), as in browsers.
            cp.top = base_top;
            self.set_unrounded_layout(
                NodeId::from(n),
                &Layout {
                    order: k as u32,
                    location: {
                        let (dx, dy) = self.relative_offset(n, used);
                        Point {
                            x: col_x[pc.col] + dx,
                            y: row_y[pc.row] + dy,
                        }
                    },
                    size: Size {
                        width: w,
                        height: slot_h,
                    },
                    scrollable_overflow_rect: cout.scrollable_overflow_rect,
                    scrollbar_size: Size::ZERO,
                    border: cb,
                    padding: cp,
                    margin: Rect::ZERO,
                },
            );
        }
        if !candidates.is_empty() {
            out.oof_candidates = candidates;
            out.oof_positioning_area = Some(OofPositioningArea {
                size: Size {
                    width: (used - rect_h(&border)).max(0.0),
                    height: (total_h - rect_v(&border)).max(0.0),
                },
                offset: Point {
                    x: border.left,
                    y: border.top,
                },
            });
        }
        data.last = Some(GridResult {
            col_x,
            col_w: widths,
            row_y,
            row_h: rh.heights,
            repeat_header,
            repeat_footer,
        });
        out
    }
}
