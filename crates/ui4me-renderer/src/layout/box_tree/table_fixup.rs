//! owner: 008-layout-engine (T100, L-TBL-r4) — the table fix-up: anonymous table objects.
//!
//! CSS 2.1 §17.2.1 / CSS Tables 3 "Generate missing parents": the DOM is never touched; the box
//! tree gets the table structure the layout needs.
//!
//! 1. **Irrelevant boxes.** Children of a `table-column` generate nothing (handled where the column
//!    is built); in a table, row group or row, a white-space-only text run whose nearest
//!    neighbours (ignoring other white space) are proper table children, or absent, is removed.
//! 2. **Missing child wrappers.** In a table, consecutive children that are not captions, column
//!    groups, columns, row groups or rows are wrapped in an anonymous row; in a row group, children
//!    that are not rows are wrapped in an anonymous row; in a row, children that are not cells are
//!    wrapped in an anonymous cell. Wrapping is per maximal run of consecutive children.
//! 3. **Missing parents.** A consecutive run of table-internal boxes (row groups, rows, cells,
//!    columns, column groups, captions) whose parent is not the right kind of table box is wrapped
//!    in an anonymous table (a wrapper box holding the table grid box).
//!
//! The table element becomes a **wrapper** box (anonymous; margins, positioning and float of the
//! element) holding the captions around the **table grid** box (the element: border, padding, size
//! and background), see [`split_table_style`]. Anonymous objects inherit from the box they are
//! generated for. Source order is preserved (captions keep their relative order within a side).

use ui4me_engine_types::ids::NodeId;

use super::anonymous::{is_collapsible_whitespace, wrap_inline_runs};
use super::build::{BoxOwner, BoxTag, Proto};
use super::{
    BoxStyle, CaptionSideValue, DisplayValue, InnerDisplay, InsetValue, LengthPercentage,
    MarginValue, MaxSizeValue, PositionValue, RowGroupKind, Sides, SizeValue, TableTag,
};

fn tag_of(p: &Proto) -> Option<TableTag> {
    match p {
        Proto::Block {
            tag: BoxTag::Table(t),
            ..
        } => Some(*t),
        Proto::Reused { tag, .. } => super::incremental::reused_tag(*tag),
        _ => None,
    }
}

fn is_ws(p: &Proto) -> bool {
    matches!(p, Proto::Text { text, .. } if is_collapsible_whitespace(text))
}

fn is_cell(p: &Proto) -> bool {
    matches!(tag_of(p), Some(TableTag::Cell { .. }))
}

fn is_row(p: &Proto) -> bool {
    matches!(tag_of(p), Some(TableTag::Row))
}

/// A proper child of a table: caption, column group, column, row group or row.
fn is_proper_table_child(p: &Proto) -> bool {
    matches!(
        tag_of(p),
        Some(
            TableTag::Caption
                | TableTag::ColumnGroup { .. }
                | TableTag::Column { .. }
                | TableTag::RowGroup(_)
                | TableTag::Row
        )
    )
}

/// A table-internal box (anything but the table itself): misparented unless in a table box.
fn is_internal(p: &Proto) -> bool {
    matches!(
        tag_of(p),
        Some(
            TableTag::Caption
                | TableTag::ColumnGroup { .. }
                | TableTag::Column { .. }
                | TableTag::RowGroup(_)
                | TableTag::Row
                | TableTag::Cell { .. }
        )
    )
}

/// Rule 1: drop white-space-only text whose nearest non-white-space neighbours are `proper` or absent.
fn strip_ws(kids: Vec<Proto>, proper: impl Fn(&Proto) -> bool) -> Vec<Proto> {
    let n = kids.len();
    let keep: Vec<bool> = (0..n)
        .map(|i| {
            if !is_ws(&kids[i]) {
                return true;
            }
            let prev = (0..i).rev().map(|j| &kids[j]).find(|p| !is_ws(p));
            let next = (i + 1..n).map(|j| &kids[j]).find(|p| !is_ws(p));
            !(prev.is_none_or(&proper) && next.is_none_or(&proper))
        })
        .collect();
    kids.into_iter()
        .zip(keep)
        .filter_map(|(k, keep)| keep.then_some(k))
        .collect()
}

/// Rule 2: wrap each maximal run of non-`proper` children with `wrap`; a run of white space only
/// is dropped.
fn wrap_runs(
    kids: Vec<Proto>,
    proper: impl Fn(&Proto) -> bool,
    mut wrap: impl FnMut(Vec<Proto>) -> Proto,
) -> Vec<Proto> {
    let mut out = Vec::with_capacity(kids.len());
    let mut run: Vec<Proto> = Vec::new();
    let mut flush = |run: &mut Vec<Proto>, out: &mut Vec<Proto>| {
        if run.is_empty() {
            return;
        }
        let items = std::mem::take(run);
        if items.iter().all(is_ws) {
            return;
        }
        out.push(wrap(items));
    };
    for k in kids {
        if proper(&k) {
            flush(&mut run, &mut out);
            out.push(k);
        } else {
            run.push(k);
        }
    }
    flush(&mut run, &mut out);
    out
}

fn anon(
    container: NodeId,
    parent: &BoxStyle,
    display: DisplayValue,
    tag: TableTag,
    inner: InnerDisplay,
    children: Vec<Proto>,
) -> Proto {
    Proto::Block {
        owner: BoxOwner::Anonymous(container),
        style: BoxStyle::inheriting(parent, display),
        children,
        inner,
        list_item: false,
        tag: BoxTag::Table(tag),
        atomic: false,
    }
}

/// An anonymous cell around `kids` (inline runs get their anonymous blocks; misparented table
/// internals inside get an anonymous table).
fn anon_cell(container: NodeId, parent: &BoxStyle, kids: Vec<Proto>) -> Proto {
    let style = BoxStyle::inheriting(parent, DisplayValue::TableCell);
    let kids = wrap_misparented(container, &style, kids);
    let kids = wrap_inline_runs(container, &style, kids, false);
    Proto::Block {
        owner: BoxOwner::Anonymous(container),
        style,
        children: kids,
        inner: InnerDisplay::FlowRoot,
        list_item: false,
        tag: BoxTag::Table(TableTag::Cell {
            col_span: 1,
            row_span: 1,
        }),
        atomic: false,
    }
}

fn anon_row(container: NodeId, parent: &BoxStyle, kids: Vec<Proto>) -> Proto {
    let style = BoxStyle::inheriting(parent, DisplayValue::TableRow);
    let children = fix_row_children(container, &style, kids);
    Proto::Block {
        owner: BoxOwner::Anonymous(container),
        style,
        children,
        inner: InnerDisplay::Flow,
        list_item: false,
        tag: BoxTag::Table(TableTag::Row),
        atomic: false,
    }
}

/// The children of a `table-row`: cells only (rules 1, 2).
pub(crate) fn fix_row_children(
    container: NodeId,
    style: &BoxStyle,
    kids: Vec<Proto>,
) -> Vec<Proto> {
    let kids = strip_ws(kids, is_cell);
    wrap_runs(kids, is_cell, |run| anon_cell(container, style, run))
}

/// The children of a row group: rows only.
pub(crate) fn fix_group_children(
    container: NodeId,
    style: &BoxStyle,
    kids: Vec<Proto>,
) -> Vec<Proto> {
    let kids = strip_ws(kids, is_row);
    wrap_runs(kids, is_row, |run| anon_row(container, style, run))
}

/// The children of a table: proper table children only.
pub(crate) fn fix_table_children(
    container: NodeId,
    style: &BoxStyle,
    kids: Vec<Proto>,
) -> Vec<Proto> {
    let kids = strip_ws(kids, is_proper_table_child);
    let kids = wrap_runs(kids, is_proper_table_child, |run| {
        anon_row(container, style, run)
    });
    group_rows(container, style, kids)
}

/// Consecutive rows that are direct children of the table get an anonymous row group (US9
/// acceptance 3): the table grid then always holds row groups, and `rowspan=0` has a group to end at.
fn group_rows(container: NodeId, style: &BoxStyle, kids: Vec<Proto>) -> Vec<Proto> {
    let mut out = Vec::with_capacity(kids.len());
    let mut run: Vec<Proto> = Vec::new();
    let flush = |run: &mut Vec<Proto>, out: &mut Vec<Proto>| {
        if !run.is_empty() {
            let rows = std::mem::take(run);
            out.push(anon(
                container,
                style,
                DisplayValue::TableRowGroup,
                TableTag::RowGroup(RowGroupKind::Body),
                InnerDisplay::Flow,
                rows,
            ));
        }
    };
    for k in kids {
        if is_row(&k) {
            run.push(k);
        } else {
            flush(&mut run, &mut out);
            out.push(k);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// The children of a column group: columns only (anything else generates nothing).
pub(crate) fn fix_column_group_children(kids: Vec<Proto>) -> Vec<Proto> {
    kids.into_iter()
        .filter(|k| matches!(tag_of(k), Some(TableTag::Column { .. })))
        .collect()
}

/// Rule 3 for the children of a box that is not a table, row group or row: each run of consecutive
/// table-internal boxes gets an anonymous table. White space between two internal boxes goes.
pub(crate) fn wrap_misparented(
    container: NodeId,
    style: &BoxStyle,
    kids: Vec<Proto>,
) -> Vec<Proto> {
    if !kids.iter().any(is_internal) {
        return kids;
    }
    let kids = strip_ws(kids, is_internal_or_none);
    let mut out = Vec::with_capacity(kids.len());
    let mut run: Vec<Proto> = Vec::new();
    for k in kids {
        if is_internal(&k) {
            run.push(k);
        } else {
            if !run.is_empty() {
                out.push(anon_table(container, style, std::mem::take(&mut run)));
            }
            out.push(k);
        }
    }
    if !run.is_empty() {
        out.push(anon_table(container, style, run));
    }
    out
}

fn is_internal_or_none(p: &Proto) -> bool {
    is_internal(p)
}

/// An anonymous table (wrapper and grid box) around `kids`, which are table-internal boxes.
fn anon_table(container: NodeId, parent: &BoxStyle, kids: Vec<Proto>) -> Proto {
    let style = BoxStyle::inheriting(parent, DisplayValue::Table);
    let kids = fix_table_children(container, &style, kids);
    assemble(
        BoxOwner::Anonymous(container),
        BoxOwner::Anonymous(container),
        style,
        kids,
        false,
    )
}

/// The styles of the two boxes a table element generates. The wrapper keeps what places the table
/// in its parent (margins, position, insets, `z-index`, float, flex and grid item properties); the
/// grid box keeps what describes the table itself (size, border, padding, background) and takes
/// the initial value of the first group. Both keep `visibility`, `direction` and the text style.
pub(crate) fn split_table_style(style: &BoxStyle) -> (BoxStyle, BoxStyle) {
    let mut wrapper = style.clone();
    wrapper.width = SizeValue::Auto;
    wrapper.height = SizeValue::Auto;
    wrapper.min_width = SizeValue::Auto;
    wrapper.min_height = SizeValue::Auto;
    wrapper.max_width = MaxSizeValue::None;
    wrapper.max_height = MaxSizeValue::None;
    wrapper.padding = Sides::all(LengthPercentage::ZERO);
    wrapper.border = Sides::all(0.0);
    wrapper.aspect_ratio = None;
    wrapper.overflow_x = super::OverflowValue::Visible;
    wrapper.overflow_y = super::OverflowValue::Visible;
    wrapper.table.border_style = Sides::all(crate::layout::fragment::BorderLineStyle::None);
    let mut table = style.clone();
    table.display = DisplayValue::Table;
    table.margin = Sides::all(MarginValue::Lp(LengthPercentage::ZERO));
    table.position = PositionValue::Static;
    table.inset = Sides::all(InsetValue::Auto);
    table.z_index = None;
    table.claims_fixed_descendants = false;
    // Layout and paint containment belong to the principal box (the wrapper); size containment has no
    // effect on a table (CSS Containment 2 section 3.1).
    table.containment = super::Containment::NONE;
    table.order = 0;
    table.flex = super::FlexStyle::initial();
    table.align = super::AlignStyle::initial();
    table.grid = super::GridStyle::initial();
    (wrapper, table)
}

/// Build the wrapper and grid protos of a table. `kids` are already fixed up (proper table
/// children only). `wrapper_owner` is anonymous; `table_owner` is the element (or anonymous for an
/// anonymous table). `atomic`: `inline-table`.
pub(crate) fn assemble(
    wrapper_owner: BoxOwner,
    table_owner: BoxOwner,
    style: BoxStyle,
    kids: Vec<Proto>,
    atomic: bool,
) -> Proto {
    let (wrapper_style, table_style) = split_table_style(&style);
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    let mut rest = Vec::new();
    for k in kids {
        if matches!(tag_of(&k), Some(TableTag::Caption)) {
            let side = match &k {
                Proto::Block { style, .. } => style.table.caption_side,
                Proto::Reused { style, .. } => style.table.caption_side,
                _ => CaptionSideValue::Top,
            };
            if side == CaptionSideValue::Bottom {
                bottom.push(k);
            } else {
                top.push(k);
            }
        } else {
            rest.push(k);
        }
    }
    let grid = Proto::Block {
        owner: table_owner,
        style: table_style,
        children: rest,
        inner: InnerDisplay::FlowRoot,
        list_item: false,
        tag: BoxTag::Table(TableTag::Table),
        atomic: false,
    };
    let mut children = top;
    children.push(grid);
    children.extend(bottom);
    Proto::Block {
        owner: wrapper_owner,
        style: wrapper_style,
        children,
        inner: InnerDisplay::FlowRoot,
        list_item: false,
        tag: BoxTag::Table(TableTag::Wrapper),
        atomic,
    }
}
