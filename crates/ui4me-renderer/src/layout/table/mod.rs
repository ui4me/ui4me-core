//! owner: 008-layout-engine (T098–T105, L-TBL-r4) — CSS tables: an owned container (AD-25).
//!
//! Taffy has no table algorithm and a table is never emulated with grid or flex (FR-076): the
//! host tree dispatches the two table nodes to the code in this module.
//!
//! ```text
//!   wrapper box (anonymous; margins, position, float of the table element)
//!   ├─ caption boxes with `caption-side: top`
//!   ├─ table grid box (the table element: border, padding, size, background)
//!   │    └─ cells, in grid order (row groups, rows and columns have no fragments of their own;
//!   │       they publish `TableTrack` rectangles for background painting)
//!   └─ caption boxes with `caption-side: bottom`
//! ```
//!
//! * [`grid`]: slot assignment by the HTML table model (`colspan`, `rowspan`, row-group order);
//! * [`measures`], [`width`]: column constraints and width distribution (automatic and fixed layout);
//! * [`height`]: row heights, `rowspan` distribution, `vertical-align`;
//! * [`borders`]: border-spacing and the collapsed-border resolution of CSS 2.1 §17.6.2;
//! * `layout` (this directory): the algorithms driven through Taffy's low-level traits. Cells are
//!   flow-root block containers (or inline roots) measured and laid out by the ordinary algorithms.
//! * `caption`: the wrapper — its width follows the table grid, captions are stacked above and below.
//!
//! # T098: provenance of the table code (R-22)
//!
//! The Servo `components/layout/table/` sources are not in this checkout (`reference-library/sources`
//! is not present) and the full SHA behind `6d3581d` was not verified, so **no file in this module is
//! ported**: every file is a clean-room implementation written from CSS 2.1 §17, CSS Tables 3, the
//! WHATWG HTML Standard §4.9.12 and the behaviour of the oracle browser on ordinary tables. No file
//! carries an MPL-2.0 `Ported-From` header and `engine/provenance.toml` has no entry for this
//! directory (`cargo xtask release provenance-check` reports nothing missing).
//!
//! | file | choice | reason |
//! |---|---|---|
//! | `grid.rs` | clean-room | HTML §4.9.12.1 is a short algorithm; the no-overlap rule is ui4me's |
//! | `measures.rs`, `width.rs` | clean-room | CSS 2.1 §17.5.2 leaves the distribution to the UA |
//! | `height.rs` | clean-room | CSS 2.1 §17.5.3 |
//! | `borders.rs` | clean-room | CSS 2.1 §17.6.2.1 states the conflict rules completely |
//! | `layout.rs`, `caption.rs`, `mod.rs` | clean-room | they are the Taffy host-tree integration |
//!
//! The decision may be revisited per file if Servo's sources are acquired (the porting route stays
//! open; it would add headers and provenance entries then).

pub mod borders;
mod caption;
pub mod grid;
pub mod height;
mod layout;
pub mod measures;
pub mod width;

use std::ops::Range;

use taffy::prelude::*;

use crate::layout::box_tree::{
    BorderCollapseValue, BoxId, BoxStyle, BoxTree, LayoutBox, LengthPercentage as CssLp,
    RowGroupKind, SizeValue, TableLayoutValue, TableTag,
};
use crate::layout::coordinator::calc::quantize_input;
use crate::layout::fragment::BorderLineStyle;
use borders::{BorderInput, BorderSpec, Origin, ResolvedBorders, SideSpecs, resolve_borders};
use grid::{CellInput, GroupInput, RowInput, TableStructure, assign_slots};
use measures::SpecWidth;

/// What a table node of the host tree is.
/// What the table keeps of a cell's layouts (015 PERF-1): the height and baseline of its measuring layout
/// at a width (with the grid's width as the percentage basis), and the top padding (with the vertical-align
/// offset) its cached final layout was computed with. Carried to the next pass by the formatting-context
/// replay with the cell's cache.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CellMemo {
    pub width: u32,
    pub used: u32,
    pub height: f32,
    pub baseline: Option<f32>,
    pub final_top: Option<u32>,
}

pub(crate) enum TableHost {
    Wrapper(WrapperData),
    Grid(Box<GridData>),
}

/// The wrapper's children, as host node indexes.
pub(crate) struct WrapperData {
    pub grid: usize,
    pub top: Vec<usize>,
    pub bottom: Vec<usize>,
}

pub(crate) struct RowInfo {
    pub box_id: BoxId,
    /// Specified `height` in px (a minimum), if a length.
    pub spec_height: Option<f32>,
    /// `visibility: collapse` on the row or its row group (CSS 2.1 §17.5.5): the row takes no space
    /// (its cells still size the columns).
    pub collapsed: bool,
}

pub(crate) struct ColInfo {
    pub box_id: Option<BoxId>,
    pub spec: SpecWidth,
}

/// Everything about one table that the host tree needs (built once per pass in `HostTree::add`).
pub(crate) struct GridData {
    pub structure: TableStructure,
    /// Host node index of each cell (`structure.cells` order).
    pub cells: Vec<usize>,
    /// The padding of each cell as styled (the vertical-alignment offset is added on top).
    pub cell_padding: Vec<Rect<LengthPercentage>>,
    pub rows: Vec<RowInfo>,
    pub row_groups: Vec<(BoxId, Range<usize>)>,
    pub columns: Vec<ColInfo>,
    pub col_groups: Vec<(BoxId, Range<usize>)>,
    pub collapse: bool,
    /// `border-spacing` (horizontal, vertical) in the separated model, else zero.
    pub spacing: (f32, f32),
    pub fixed_layout: bool,
    pub borders: Option<ResolvedBorders>,
    /// The table's used border widths (px).
    pub border: Rect<f32>,
    /// The geometry of the last full pass, for the fragments.
    pub last: Option<GridResult>,
    /// The rows of the `thead` / `tfoot` group with `break-inside: avoid` (empty otherwise): repeated in every
    /// piece of a table broken across fragmentainers (015 FRAG-2c).
    pub header_rows: Range<usize>,
    pub footer_rows: Range<usize>,
    /// The `thead` / `tfoot` rows whatever their `break-inside` (015 PAGE-1): repeated on every page, as the
    /// reference browser prints tables.
    pub page_header_rows: Range<usize>,
    pub page_footer_rows: Range<usize>,
}

/// The geometry of one full layout pass, relative to the table's border box.
#[derive(Clone, Debug, Default)]
pub(crate) struct GridResult {
    pub col_x: Vec<f32>,
    pub col_w: Vec<f32>,
    pub row_y: Vec<f32>,
    pub row_h: Vec<f32>,
    /// Inside a fragmentation root (015 FRAG-2c): the height the repeated header takes at the top of every
    /// piece after the first, and the repeated footer at the end of every piece but the last (0: none or
    /// not broken).
    pub repeat_header: f32,
    pub repeat_footer: f32,
}

/// A length (not a percentage) from a size value; `None` for `auto`, percentages and keywords.
fn fixed_px(v: &SizeValue) -> Option<f32> {
    match v {
        SizeValue::Lp(CssLp::Px(p)) => Some(quantize_input(*p)),
        SizeValue::Lp(CssLp::Calc(e)) if !e.has_percent() => Some(quantize_input(
            crate::layout::coordinator::calc::resolve_calc_value(e, 0.0),
        )),
        _ => None,
    }
}

/// The specified width of a column element (a floor for its column).
fn col_spec(s: &BoxStyle) -> SpecWidth {
    match &s.width {
        SizeValue::Lp(CssLp::Percent(p)) => SpecWidth::Percent(p * 100.0),
        v => fixed_px(v).map_or(SpecWidth::Auto, SpecWidth::Fixed),
    }
}

fn spec_of(b: &LayoutBox, origin: Origin, id: BoxId) -> SideSpecs {
    let s = &b.style;
    let side = |i: usize| {
        let (st, w, c) = match i {
            0 => (
                s.table.border_style.top,
                s.border.top,
                s.table.border_opaque.top,
            ),
            1 => (
                s.table.border_style.right,
                s.border.right,
                s.table.border_opaque.right,
            ),
            2 => (
                s.table.border_style.bottom,
                s.border.bottom,
                s.table.border_opaque.bottom,
            ),
            _ => (
                s.table.border_style.left,
                s.border.left,
                s.table.border_opaque.left,
            ),
        };
        BorderSpec {
            style: st,
            width: if matches!(st, BorderLineStyle::None | BorderLineStyle::Hidden) {
                0.0
            } else {
                w
            },
            opaque: c,
            side: i as u8,
            origin,
            box_id: id,
        }
    };
    [side(0), side(1), side(2), side(3)]
}

fn is_ws_text(boxes: &BoxTree, id: BoxId) -> bool {
    matches!(&boxes.get(id).kind, crate::layout::box_tree::BoxKind::TextRun(t) if crate::layout::box_tree::anonymous::is_collapsible_whitespace(t))
}

/// The row groups, rows and columns of a table box in visual order (CSS 2.1 §17.2: the first
/// header group is rendered first and the first footer group last; further header and footer
/// groups are body groups).
type RowEntry = (BoxId, Vec<BoxId>);
type GroupEntry = (Option<BoxId>, Vec<RowEntry>);

struct Gathered {
    groups: Vec<GroupEntry>,
    columns: Vec<(Option<BoxId>, Option<BoxId>)>,
    col_groups: Vec<(BoxId, Range<usize>)>,
}

fn gather(boxes: &BoxTree, table: &LayoutBox) -> Gathered {
    let mut columns: Vec<(Option<BoxId>, Option<BoxId>)> = Vec::new();
    let mut col_groups = Vec::new();
    let mut header: Option<GroupEntry> = None;
    let mut footer: Option<GroupEntry> = None;
    let mut bodies: Vec<GroupEntry> = Vec::new();
    let rows_of = |g: &LayoutBox| -> Vec<(BoxId, Vec<BoxId>)> {
        g.children
            .iter()
            .filter(|c| {
                matches!(
                    boxes.get(**c).tag,
                    crate::layout::box_tree::BoxTag::Table(TableTag::Row)
                )
            })
            .map(|r| {
                let cells = boxes
                    .get(*r)
                    .children
                    .iter()
                    .copied()
                    .filter(|c| {
                        matches!(
                            boxes.get(*c).tag,
                            crate::layout::box_tree::BoxTag::Table(TableTag::Cell { .. })
                        )
                    })
                    .collect();
                (*r, cells)
            })
            .collect()
    };
    // Direct rows between groups form an implicit body group.
    let mut implicit: Vec<(BoxId, Vec<BoxId>)> = Vec::new();
    let flush = |implicit: &mut Vec<RowEntry>, bodies: &mut Vec<GroupEntry>| {
        if !implicit.is_empty() {
            bodies.push((None, std::mem::take(implicit)));
        }
    };
    for &c in &table.children {
        let cb = boxes.get(c);
        let crate::layout::box_tree::BoxTag::Table(tag) = cb.tag else {
            continue;
        };
        match tag {
            TableTag::ColumnGroup { span } => {
                let start = columns.len();
                let cols: Vec<&BoxId> = cb
                    .children
                    .iter()
                    .filter(|k| {
                        matches!(
                            boxes.get(**k).tag,
                            crate::layout::box_tree::BoxTag::Table(TableTag::Column { .. })
                        )
                    })
                    .collect();
                if cols.is_empty() {
                    for _ in 0..span {
                        columns.push((None, Some(c)));
                    }
                } else {
                    for k in cols {
                        let TableTag::Column { span } = (match boxes.get(*k).tag {
                            crate::layout::box_tree::BoxTag::Table(t) => t,
                            _ => continue,
                        }) else {
                            continue;
                        };
                        for _ in 0..span {
                            columns.push((Some(*k), Some(c)));
                        }
                    }
                }
                col_groups.push((c, start..columns.len()));
            }
            TableTag::Column { span } => {
                for _ in 0..span {
                    columns.push((Some(c), None));
                }
            }
            TableTag::RowGroup(kind) => {
                flush(&mut implicit, &mut bodies);
                let g = (Some(c), rows_of(cb));
                match kind {
                    RowGroupKind::Header if header.is_none() => header = Some(g),
                    RowGroupKind::Footer if footer.is_none() => footer = Some(g),
                    _ => bodies.push(g),
                }
            }
            TableTag::Row => {
                let cells = cb
                    .children
                    .iter()
                    .copied()
                    .filter(|k| {
                        matches!(
                            boxes.get(*k).tag,
                            crate::layout::box_tree::BoxTag::Table(TableTag::Cell { .. })
                        )
                    })
                    .collect();
                implicit.push((c, cells));
            }
            _ => {}
        }
    }
    flush(&mut implicit, &mut bodies);
    let mut groups = Vec::new();
    groups.extend(header);
    groups.extend(bodies);
    groups.extend(footer);
    Gathered {
        groups,
        columns,
        col_groups,
    }
}

impl crate::layout::coordinator::host_tree::HostTree {
    /// The children of a table wrapper: captions around the grid.
    pub(crate) fn add_wrapper_children(
        &mut self,
        boxes: &BoxTree,
        id: BoxId,
        idx: usize,
        text: &mut crate::text::fonts::TextServices,
    ) {
        let b = boxes.get(id);
        let mut top = Vec::new();
        let mut bottom = Vec::new();
        let mut grid = None;
        for &c in &b.children {
            let cb = boxes.get(c);
            match cb.tag {
                crate::layout::box_tree::BoxTag::Table(TableTag::Table) => {
                    grid = Some(self.add(
                        boxes,
                        c,
                        Some(crate::layout::box_tree::InnerDisplay::FlowRoot),
                        text,
                    ))
                }
                crate::layout::box_tree::BoxTag::Table(TableTag::Caption) => {
                    let n = self.add(
                        boxes,
                        c,
                        Some(crate::layout::box_tree::InnerDisplay::FlowRoot),
                        text,
                    );
                    if cb.style.table.caption_side
                        == crate::layout::box_tree::CaptionSideValue::Bottom
                    {
                        bottom.push(n);
                    } else {
                        top.push(n);
                    }
                }
                _ => {}
            }
        }
        let Some(grid) = grid else { return };
        // Children in layout order: top captions, the grid, bottom captions.
        let mut kids = top.clone();
        kids.push(grid);
        kids.extend(bottom.iter().copied());
        self.nodes[idx].children = kids;
        self.nodes[idx].table = Some(Box::new(TableHost::Wrapper(WrapperData {
            grid,
            top,
            bottom,
        })));
    }

    /// The cells of a table grid box, the structure and the resolved borders.
    pub(crate) fn add_grid_children(
        &mut self,
        boxes: &BoxTree,
        id: BoxId,
        idx: usize,
        text: &mut crate::text::fonts::TextServices,
    ) {
        let t_tb = std::time::Instant::now();
        let mut child_ns = 0u128;
        let table = boxes.get(id);
        let collapse = table.style.table.border_collapse == BorderCollapseValue::Collapse;
        let mut g = gather(boxes, table);
        let input: Vec<GroupInput> = g
            .groups
            .iter()
            .map(|(_, rows)| GroupInput {
                rows: rows
                    .iter()
                    .map(|(_, cells)| RowInput {
                        cells: cells
                            .iter()
                            .map(|c| match boxes.get(*c).tag {
                                crate::layout::box_tree::BoxTag::Table(TableTag::Cell {
                                    col_span,
                                    row_span,
                                }) => CellInput { col_span, row_span },
                                _ => CellInput {
                                    col_span: 1,
                                    row_span: 1,
                                },
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        let mut structure = assign_slots(&input);
        // A column no cell reaches exists (and takes its share of the border spacing) only up to the
        // last one that has a definite length width; the others are dropped, as the reference
        // browser does (WPT css-tables/col-definite-size-001: `auto`, percentage and `0` columns).
        let definite = g
            .columns
            .iter()
            .enumerate()
            .filter(|(_, (col, group))| {
                let spec = |b: &Option<BoxId>| {
                    b.map(|b| col_spec(&boxes.get(b).style))
                        .unwrap_or(SpecWidth::Auto)
                };
                let own = spec(col);
                let eff = if own == SpecWidth::Auto {
                    spec(group)
                } else {
                    own
                };
                matches!(eff, SpecWidth::Fixed(w) if w > 0.0)
            })
            .map(|(i, _)| i + 1)
            .max()
            .unwrap_or(0);
        structure.ensure_columns(definite);
        // The dropped columns take their `colgroup` ranges with them.
        let count = structure.column_count;
        for (_, range) in g.col_groups.iter_mut() {
            range.end = range.end.min(count);
            range.start = range.start.min(range.end);
        }
        // Boxes in structure order.
        let cell_boxes: Vec<BoxId> = g
            .groups
            .iter()
            .flat_map(|(_, rows)| rows.iter().flat_map(|(_, cells)| cells.iter().copied()))
            .collect();
        let rows: Vec<RowInfo> = g
            .groups
            .iter()
            .flat_map(|(gb, rows)| {
                let group_collapsed = gb.is_some_and(|g| {
                    boxes.get(g).style.visibility
                        == crate::layout::box_tree::VisibilityValue::Collapse
                });
                rows.iter().map(move |(r, _)| RowInfo {
                    box_id: *r,
                    spec_height: fixed_px(&boxes.get(*r).style.height),
                    collapsed: group_collapsed
                        || boxes.get(*r).style.visibility
                            == crate::layout::box_tree::VisibilityValue::Collapse,
                })
            })
            .collect();
        let row_groups: Vec<(BoxId, Range<usize>)> = g
            .groups
            .iter()
            .zip(&structure.groups)
            .filter_map(|((b, _), range)| b.map(|b| (b, range.clone())))
            .collect();
        // `gather` puts the header group first and the footer group last. A header or footer is repeated in
        // the pieces of a broken table when it avoids breaks inside (the reference browser in columns).
        let group_kind = |k: usize, avoid_only: bool| {
            g.groups
                .get(k)
                .and_then(|(b, _)| *b)
                .and_then(|b| match boxes.get(b).tag {
                    crate::layout::box_tree::BoxTag::Table(TableTag::RowGroup(kind))
                        if !avoid_only
                            || matches!(
                                boxes.get(b).style.frag.break_inside,
                                crate::layout::box_tree::BreakWithin::Avoid
                                    | crate::layout::box_tree::BreakWithin::AvoidColumn
                            ) =>
                    {
                        Some(kind)
                    }
                    _ => None,
                })
        };
        let n_groups = g.groups.len();
        let header_of = |avoid_only: bool| match group_kind(0, avoid_only) {
            Some(RowGroupKind::Header) => structure.groups.first().cloned().unwrap_or(0..0),
            _ => 0..0,
        };
        let footer_of = |avoid_only: bool, header: &Range<usize>| match n_groups
            .checked_sub(1)
            .and_then(|k| group_kind(k, avoid_only))
        {
            Some(RowGroupKind::Footer) if n_groups > 1 || header.is_empty() => {
                structure.groups.last().cloned().unwrap_or(0..0)
            }
            _ => 0..0,
        };
        let header_rows = header_of(true);
        let footer_rows = footer_of(true, &header_rows);
        let page_header_rows = header_of(false);
        let page_footer_rows = footer_of(false, &page_header_rows);
        // Column specs: the `col`'s width, else its group's.
        let mut columns: Vec<ColInfo> = (0..structure.column_count)
            .map(|i| match g.columns.get(i) {
                Some((col, group)) => {
                    let own = col
                        .map(|c| col_spec(&boxes.get(c).style))
                        .unwrap_or(SpecWidth::Auto);
                    let spec = if own == SpecWidth::Auto {
                        group
                            .map(|gr| col_spec(&boxes.get(gr).style))
                            .unwrap_or(SpecWidth::Auto)
                    } else {
                        own
                    };
                    ColInfo { box_id: *col, spec }
                }
                None => ColInfo {
                    box_id: None,
                    spec: SpecWidth::Auto,
                },
            })
            .collect();
        columns.truncate(structure.column_count);

        let mut cells = Vec::with_capacity(cell_boxes.len());
        let mut cell_empty = Vec::with_capacity(cell_boxes.len());
        let mut cell_padding = Vec::with_capacity(cell_boxes.len());
        for &cb in &cell_boxes {
            let t_c = std::time::Instant::now();
            let n = self.add(
                boxes,
                cb,
                Some(crate::layout::box_tree::InnerDisplay::FlowRoot),
                text,
            );
            child_ns += t_c.elapsed().as_nanos();
            cells.push(n);
            cell_empty.push(boxes.get(cb).children.iter().all(|c| is_ws_text(boxes, *c)));
            cell_padding.push(self.nodes[n].style.padding);
            let pc = structure.cells[cells.len() - 1];
            self.nodes[n].table_cell = Some(crate::layout::fragment::TableCellInfo {
                row: pc.row as u32,
                column: pc.col as u32,
                row_span: pc.row_span as u32,
                col_span: pc.col_span as u32,
                empty_cells_hidden: table.style.table.empty_cells_hide
                    && !collapse
                    && cell_empty[cells.len() - 1],
            });
        }

        let table_style = &table.style;
        let (borders, border) = if collapse {
            let t_spec = spec_of(table, Origin::Table, id);
            let row_specs: Vec<SideSpecs> = rows
                .iter()
                .map(|r| spec_of(boxes.get(r.box_id), Origin::Row, r.box_id))
                .collect();
            let rg_specs: Vec<(Range<usize>, SideSpecs)> = row_groups
                .iter()
                .map(|(b, range)| (range.clone(), spec_of(boxes.get(*b), Origin::RowGroup, *b)))
                .collect();
            let cg_specs: Vec<(Range<usize>, SideSpecs)> = g
                .col_groups
                .iter()
                .map(|(b, range)| {
                    (
                        range.clone(),
                        spec_of(boxes.get(*b), Origin::ColumnGroup, *b),
                    )
                })
                .collect();
            let col_specs: Vec<Option<SideSpecs>> = columns
                .iter()
                .map(|c| c.box_id.map(|b| spec_of(boxes.get(b), Origin::Column, b)))
                .collect();
            let cell_specs: Vec<SideSpecs> = cell_boxes
                .iter()
                .map(|b| spec_of(boxes.get(*b), Origin::Cell, *b))
                .collect();
            let resolved = resolve_borders(&BorderInput {
                structure: &structure,
                table: t_spec,
                row_groups: &rg_specs,
                rows: &row_specs,
                col_groups: &cg_specs,
                cols: &col_specs,
                cells: &cell_specs,
            });
            // The cells' borders are half of the collapsed ones; the table's padding does not apply.
            for (k, &n) in cells.iter().enumerate() {
                let w = resolved.cell_borders[k].map(quantize_input);
                self.nodes[n].style.border = Rect {
                    top: LengthPercentage::length(w[0]),
                    right: LengthPercentage::length(w[1]),
                    bottom: LengthPercentage::length(w[2]),
                    left: LengthPercentage::length(w[3]),
                };
            }
            let w = resolved.table_border.map(quantize_input);
            let b = Rect {
                top: w[0],
                right: w[1],
                bottom: w[2],
                left: w[3],
            };
            self.nodes[idx].style.border = Rect {
                top: LengthPercentage::length(b.top),
                right: LengthPercentage::length(b.right),
                bottom: LengthPercentage::length(b.bottom),
                left: LengthPercentage::length(b.left),
            };
            self.nodes[idx].style.padding = Rect {
                top: LengthPercentage::length(0.0),
                right: LengthPercentage::length(0.0),
                bottom: LengthPercentage::length(0.0),
                left: LengthPercentage::length(0.0),
            };
            (Some(resolved), b)
        } else {
            let b = Rect {
                top: quantize_input(table_style.border.top.max(0.0)),
                right: quantize_input(table_style.border.right.max(0.0)),
                bottom: quantize_input(table_style.border.bottom.max(0.0)),
                left: quantize_input(table_style.border.left.max(0.0)),
            };
            (None, b)
        };
        let spacing = if collapse {
            (0.0, 0.0)
        } else {
            (
                quantize_input(table_style.table.border_spacing.0.max(0.0)),
                quantize_input(table_style.table.border_spacing.1.max(0.0)),
            )
        };
        crate::layout::coordinator::host_tree::TABLE_NS
            .with(|c| c.set(c.get() + (t_tb.elapsed().as_nanos() - child_ns) as u64));
        self.nodes[idx].children = cells.clone();
        self.nodes[idx].table = Some(Box::new(TableHost::Grid(Box::new(GridData {
            structure,
            cells,
            cell_padding,
            rows,
            row_groups,
            columns,
            col_groups: g.col_groups,
            collapse,
            spacing,
            fixed_layout: table_style.table.table_layout == TableLayoutValue::Fixed,
            borders,
            border,
            last: None,
            header_rows,
            footer_rows,
            page_header_rows,
            page_footer_rows,
        }))));
    }
}
