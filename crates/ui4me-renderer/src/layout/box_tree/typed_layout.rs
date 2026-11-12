//! owner: 008-layout-engine
//!
//! Flex, box-alignment and grid longhands from 006's typed `ComputedStyleView` groups
//! (`flex()`, `alignment()`, `grid()`; 006:T038) into the layout crate's plain style types. This
//! replaced the CSSOM-text bridge of `projection_reports.rs` for these properties (lane L-GRID-r14).
//!
//! A value outside what layout consumes is reported (`css.property.<name>`, stage `layout`) and
//! treated as the property's initial value, never approximated: `left`/`right`/`legacy` alignment, `last baseline`
//! (FR-043: `safe end` plus `css.value.last-baseline`) and `subgrid`. Named lines, named areas and
//! named spans are consumed.

use crate::style::view::groups as g;
use crate::style::view::layout_groups as t;

use super::build::map::lp;
use super::projection_reports::Unconsumed;
use super::{
    AlignKeyword, AlignStyle, AlignValue, FlexBasisValue, FlexDirectionValue, FlexStyle,
    FlexWrapValue, GapValue, GridAreaValue, GridAreasValue, GridAutoFlowValue, GridLineValue,
    GridStyle, LineNames, RepeatCount, TrackBreadth, TrackListItem, TrackSize,
};

pub fn overflow(value: g::Overflow) -> (super::OverflowValue, super::OverflowValue) {
    let map = |value| match value {
        g::OverflowKind::Visible => super::OverflowValue::Visible,
        g::OverflowKind::Hidden => super::OverflowValue::Hidden,
        g::OverflowKind::Clip => super::OverflowValue::Clip,
        g::OverflowKind::Scroll => super::OverflowValue::Scroll,
        g::OverflowKind::Auto => super::OverflowValue::Auto,
    };
    (map(value.x), map(value.y))
}

pub fn flex_style(f: &t::Flex, un: &mut Unconsumed<'_>) -> FlexStyle {
    let mut s = FlexStyle::initial();
    s.direction = match f.direction {
        t::FlexDirection::Row => FlexDirectionValue::Row,
        t::FlexDirection::RowReverse => FlexDirectionValue::RowReverse,
        t::FlexDirection::Column => FlexDirectionValue::Column,
        t::FlexDirection::ColumnReverse => FlexDirectionValue::ColumnReverse,
    };
    s.wrap = match (f.wrap, f.wrap_balance) {
        (t::FlexWrap::NoWrap, _) => FlexWrapValue::NoWrap,
        (t::FlexWrap::Wrap, false) => FlexWrapValue::Wrap,
        (t::FlexWrap::WrapReverse, false) => FlexWrapValue::WrapReverse,
        (t::FlexWrap::Wrap, true) => FlexWrapValue::Balance,
        (t::FlexWrap::WrapReverse, true) => FlexWrapValue::BalanceReverse,
    };
    s.grow = if f.grow.is_finite() {
        f.grow.max(0.0)
    } else {
        0.0
    };
    s.shrink = if f.shrink.is_finite() {
        f.shrink.max(0.0)
    } else {
        1.0
    };
    match &f.basis {
        t::FlexBasis::Content => s.basis = FlexBasisValue::Content,
        t::FlexBasis::Size(g::Size::Auto) => {}
        t::FlexBasis::Size(g::Size::LengthPercentage(v)) => s.basis = FlexBasisValue::Lp(lp(v)),
        t::FlexBasis::Size(g::Size::MinContent) => s.basis = FlexBasisValue::MinContent,
        t::FlexBasis::Size(g::Size::MaxContent) => s.basis = FlexBasisValue::MaxContent,
        t::FlexBasis::Size(g::Size::FitContent(limit)) => {
            s.basis = FlexBasisValue::FitContent(limit.as_ref().map(lp));
        }
        t::FlexBasis::Size(g::Size::Stretch) => s.basis = FlexBasisValue::Stretch,
        t::FlexBasis::Size(g::Size::None) => un.property("flex-basis"),
    }
    s
}

pub fn order(f: &t::Flex) -> i32 {
    f.order
}

fn align_value(a: &t::Align, allow_auto: bool, allow_distribution: bool) -> Result<AlignValue, ()> {
    use t::AlignKeyword as K;
    if a.legacy {
        return Err(());
    }
    let keyword = match a.keyword {
        K::Normal => AlignKeyword::Normal,
        K::Auto if allow_auto => AlignKeyword::Auto,
        K::Start => AlignKeyword::Start,
        K::End => AlignKeyword::End,
        K::FlexStart => AlignKeyword::FlexStart,
        K::FlexEnd => AlignKeyword::FlexEnd,
        K::SelfStart => AlignKeyword::SelfStart,
        K::SelfEnd => AlignKeyword::SelfEnd,
        K::Center => AlignKeyword::Center,
        K::Baseline => AlignKeyword::Baseline,
        K::Stretch => AlignKeyword::Stretch,
        K::SpaceBetween if allow_distribution => AlignKeyword::SpaceBetween,
        K::SpaceAround if allow_distribution => AlignKeyword::SpaceAround,
        K::SpaceEvenly if allow_distribution => AlignKeyword::SpaceEvenly,
        _ => return Err(()),
    };
    Ok(AlignValue {
        keyword,
        safe: a.safe,
    })
}

pub fn align_style(a: &t::Alignment, un: &mut Unconsumed<'_>) -> AlignStyle {
    let mut s = AlignStyle::initial();
    let mut read =
        |name: &str, v: &t::Align, allow_auto: bool, dist: bool, slot: &mut AlignValue| {
            if v.keyword == t::AlignKeyword::LastBaseline && !v.legacy {
                // FR-043: the Box Alignment fallback alignment is `safe end`; reported, never emulated.
                *slot = AlignValue {
                    keyword: AlignKeyword::End,
                    safe: Some(true),
                };
                un.value("last-baseline");
                return;
            }
            match align_value(v, allow_auto, dist) {
                Ok(a) => *slot = a,
                Err(()) => un.property(name),
            }
        };
    read(
        "align-items",
        &a.align_items,
        false,
        false,
        &mut s.align_items,
    );
    read("align-self", &a.align_self, true, false, &mut s.align_self);
    read(
        "justify-content",
        &a.justify_content,
        false,
        true,
        &mut s.justify_content,
    );
    read(
        "align-content",
        &a.align_content,
        false,
        true,
        &mut s.align_content,
    );
    read(
        "justify-items",
        &a.justify_items,
        false,
        false,
        &mut s.justify_items,
    );
    read(
        "justify-self",
        &a.justify_self,
        true,
        false,
        &mut s.justify_self,
    );
    s.row_gap = gap(&a.row_gap);
    s.column_gap = gap(&a.column_gap);
    s
}

fn gap(v: &t::Gap) -> GapValue {
    match v {
        t::Gap::Normal => GapValue::Normal,
        t::Gap::LengthPercentage(v) => GapValue::Lp(lp(v)),
    }
}

// ------------------------------------------------------------------ grid (FR-048-FR-054)

fn breadth(b: &t::TrackBreadth) -> TrackBreadth {
    match b {
        t::TrackBreadth::Breadth(v) => TrackBreadth::Lp(lp(v)),
        t::TrackBreadth::Flex(n) => TrackBreadth::Fr(*n),
        t::TrackBreadth::Auto => TrackBreadth::Auto,
        t::TrackBreadth::MinContent => TrackBreadth::MinContent,
        t::TrackBreadth::MaxContent => TrackBreadth::MaxContent,
    }
}

fn track(s: &t::TrackSize) -> TrackSize {
    match s {
        // A single `<flex>` is `minmax(auto, <flex>)`.
        t::TrackSize::Breadth(t::TrackBreadth::Flex(n)) => TrackSize {
            min: TrackBreadth::Auto,
            max: TrackBreadth::Fr(*n),
        },
        t::TrackSize::Breadth(b) => TrackSize {
            min: breadth(b),
            max: breadth(b),
        },
        t::TrackSize::Minmax(min, max) => TrackSize {
            min: breadth(min),
            max: breadth(max),
        },
        t::TrackSize::FitContent(v) => TrackSize {
            min: TrackBreadth::Auto,
            max: TrackBreadth::FitContent(lp(v)),
        },
    }
}

/// `(items, line names)`: the names are one set per line, or empty when no line has a name.
fn track_list(l: &t::TrackList) -> (Vec<TrackListItem>, Vec<LineNames>) {
    let items: Vec<TrackListItem> = l
        .items
        .iter()
        .map(|i| match i {
            t::TrackListItem::Track(s) => TrackListItem::Track(track(s)),
            t::TrackListItem::Repeat(r) => {
                let count = match r.count {
                    t::RepeatCount::Number(n) => {
                        RepeatCount::Count(n.clamp(1, u16::MAX as u32) as u16)
                    }
                    t::RepeatCount::AutoFill => RepeatCount::AutoFill,
                    t::RepeatCount::AutoFit => RepeatCount::AutoFit,
                };
                TrackListItem::Repeat(
                    count,
                    r.tracks.iter().map(track).collect(),
                    name_sets(&r.line_names, r.tracks.len() + 1),
                )
            }
        })
        .collect();
    let mut outer = name_sets(&l.line_names, items.len() + 1);
    // Taffy reads `repeat()` line names through the outer sets: when only a repeat names lines, the
    // outer sets must exist (empty) for it to be visited.
    if outer.is_empty()
        && items
            .iter()
            .any(|i| matches!(i, TrackListItem::Repeat(_, _, n) if !n.is_empty()))
    {
        outer = vec![Vec::new(); items.len() + 1];
    }
    (items, outer)
}

/// `want` name sets (padded or cut), or none when no set has a name.
fn name_sets(sets: &[Vec<String>], want: usize) -> Vec<LineNames> {
    if sets.iter().all(|s| s.is_empty()) {
        return Vec::new();
    }
    let mut out: Vec<LineNames> = sets.iter().take(want).cloned().collect();
    out.resize(want, Vec::new());
    out
}

fn implicit(v: &[t::TrackSize]) -> Vec<TrackSize> {
    v.iter().map(track).collect()
}

fn clamp_i16(n: i32) -> i16 {
    n.clamp(i16::MIN as i32 + 1, i16::MAX as i32) as i16
}

fn clamp_u16(n: i32) -> u16 {
    n.clamp(1, u16::MAX as i32) as u16
}

fn line(l: &t::GridLine) -> GridLineValue {
    match l {
        t::GridLine::Auto => GridLineValue::Auto,
        // Line 0 is invalid in CSS and is dropped by the parser; read as `auto`.
        t::GridLine::Number(0) => GridLineValue::Auto,
        t::GridLine::Number(n) => GridLineValue::Line(clamp_i16(*n)),
        t::GridLine::Span(n) => GridLineValue::Span(clamp_u16(*n)),
        t::GridLine::Named { name, nth } => GridLineValue::NamedLine(name.clone(), clamp_i16(*nth)),
        t::GridLine::NamedSpan { name, count } => {
            GridLineValue::NamedSpan(name.clone(), clamp_u16(*count))
        }
    }
}

pub fn grid_style(gr: &t::Grid, un: &mut Unconsumed<'_>) -> GridStyle {
    let mut s = GridStyle::initial();
    let _ = &un;
    for (tpl, items, names, subgrid) in [
        (
            &gr.columns,
            &mut s.columns,
            &mut s.column_names,
            &mut s.column_subgrid,
        ),
        (&gr.rows, &mut s.rows, &mut s.row_names, &mut s.row_subgrid),
    ] {
        match tpl {
            t::GridTemplate::None => {}
            t::GridTemplate::Tracks(l) => (*items, *names) = track_list(l),
            // `subgrid` (CSS Grid 2 section 9, 015 GRID-1): no tracks of its own; the name sets name the
            // subgrid's lines from its first (Taffy patch P-008-4 takes the tracks from the parent grid).
            t::GridTemplate::Subgrid(sets) => {
                *subgrid = true;
                if sets.iter().any(|set| !set.is_empty()) {
                    *names = sets.clone();
                }
            }
        }
    }
    s.auto_columns = implicit(&gr.auto_columns);
    s.auto_rows = implicit(&gr.auto_rows);
    s.auto_flow = match gr.auto_flow {
        t::GridAutoFlow::Row => GridAutoFlowValue::Row,
        t::GridAutoFlow::Column => GridAutoFlowValue::Column,
        t::GridAutoFlow::RowDense => GridAutoFlowValue::RowDense,
        t::GridAutoFlow::ColumnDense => GridAutoFlowValue::ColumnDense,
    };
    s.areas = gr.areas.as_ref().map(|a| GridAreasValue {
        areas: a
            .areas
            .iter()
            .map(|a| GridAreaValue {
                name: a.name.clone(),
                row_start: a.row_start.min(u16::MAX as u32) as u16,
                row_end: a.row_end.min(u16::MAX as u32) as u16,
                column_start: a.column_start.min(u16::MAX as u32) as u16,
                column_end: a.column_end.min(u16::MAX as u32) as u16,
            })
            .collect(),
        rows: a.rows.min(u16::MAX as u32) as u16,
        columns: a.columns.min(u16::MAX as u32) as u16,
    });
    s.column = (line(&gr.column_start), line(&gr.column_end));
    s.row = (line(&gr.row_start), line(&gr.row_end));
    s
}
