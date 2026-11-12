//! owner: 008-layout-engine
//!
//! Style views for the Taffy host tree (AD-12, AD-33): each box's computed values
//! ([`BoxStyle`], mapped from 006's `ComputedStyleView` by the style bridge) become the style the
//! formatting algorithms read through Taffy's style traits (`CoreStyle`, `BlockContainerStyle`,
//! `BlockItemStyle`, `FlexboxContainerStyle`, `FlexboxItemStyle`, implemented by `taffy::Style`).
//! No Taffy type leaves this module's callers (the host tree) and no U4C surface exposes one.
//!
//! Every length is quantised to 1/64 px before it enters Taffy (FR-026); a `calc()` with a
//! percentage is kept as a handle the host resolves against the algorithm's basis
//! (`resolve_calc_value`), a percentage-free `calc()` is resolved once to a length, because
//! Taffy resolves a calc against an indefinite basis to `auto`.

use std::collections::HashMap;
use std::sync::Arc;
use taffy::prelude::*;
use taffy::{
    AlignContent as TAlignContent, AlignItems as TAlignItems, Contain, GridTemplateRepetition,
    Overflow, Point,
};

use super::calc::{CalcExpr, quantize_input, resolve_calc_value};
use crate::layout::box_tree::{
    AlignKeyword, AlignValue, BoxSizing as CssBoxSizing, BoxStyle, Direction, FlexBasisValue,
    FlexDirectionValue, FlexWrapValue, GapValue, GridAreasValue, GridAutoFlowValue, GridLineValue,
    InnerDisplay, InsetValue, LengthPercentage as CssLp, MarginValue, MaxSizeValue, OverflowValue,
    PositionValue, RepeatCount, SizeValue, TrackBreadth, TrackListItem, TrackSize,
};

/// The calc expressions referenced by the styles of one pass, keyed by the address handed to
/// Taffy (stable for the pass, because the `Arc`s are held here).
/// Cloning is cheap (the map is shared): an inline root hands a copy to the layouts of its atomic
/// inlines while the tree is borrowed mutably.
#[derive(Clone, Default)]
pub(crate) struct CalcTable {
    map: Arc<HashMap<usize, Arc<CalcExpr>>>,
    /// While building the style of a flex item: percentages resolve through a handle that
    /// truncates to 1/64 px (X-FLEX variant C, provisional: the gate verdict is UNKNOWN, see
    /// `planning-handoff/integration-log/lane-L008r1-decisions.md`). Without it, six items of
    /// `16.66666667%` in 1000 px sum to 1000.000002 px in f32 and wrap (U4T-004, FR-041).
    truncate_percentages: bool,
}

impl CalcTable {
    pub(crate) fn resolve(&self, ptr: *const (), basis: f32) -> f32 {
        match self.map.get(&(ptr as usize)) {
            Some(e) => resolve_calc_value(e, basis),
            // Unknown pointer: NaN → 0 at the calc boundary.
            None => 0.0,
        }
    }

    fn ptr(&mut self, e: &Arc<CalcExpr>) -> *const () {
        let p = Arc::as_ptr(e) as *const ();
        Arc::make_mut(&mut self.map).insert(p as usize, e.clone());
        p
    }

    fn trunc_ptr(&mut self, p: f32) -> *const () {
        self.ptr(&Arc::new(CalcExpr::TruncatedPercent(p)))
    }

    fn lp(&mut self, v: &CssLp) -> LengthPercentage {
        match v {
            CssLp::Calc(e) if !e.has_percent() => {
                LengthPercentage::length(resolve_calc_value(e, 0.0))
            }
            CssLp::Px(px) => LengthPercentage::length(quantize_input(*px)),
            CssLp::Percent(p) if self.truncate_percentages => {
                LengthPercentage::calc(self.trunc_ptr(*p))
            }
            CssLp::Percent(p) => LengthPercentage::percent(*p),
            CssLp::Calc(e) => LengthPercentage::calc(self.ptr(e)),
        }
    }

    fn lpa(&mut self, v: &CssLp) -> LengthPercentageAuto {
        match v {
            CssLp::Calc(e) if !e.has_percent() => {
                LengthPercentageAuto::length(resolve_calc_value(e, 0.0))
            }
            CssLp::Px(px) => LengthPercentageAuto::length(quantize_input(*px)),
            CssLp::Percent(p) if self.truncate_percentages => {
                LengthPercentageAuto::calc(self.trunc_ptr(*p))
            }
            CssLp::Percent(p) => LengthPercentageAuto::percent(*p),
            CssLp::Calc(e) => LengthPercentageAuto::calc(self.ptr(e)),
        }
    }

    fn dim_lp(&mut self, v: &CssLp) -> Dimension {
        match v {
            CssLp::Calc(e) if !e.has_percent() => Dimension::length(resolve_calc_value(e, 0.0)),
            CssLp::Px(px) => Dimension::length(quantize_input(*px)),
            CssLp::Percent(p) if self.truncate_percentages => Dimension::calc(self.trunc_ptr(*p)),
            CssLp::Percent(p) => Dimension::percent(*p),
            CssLp::Calc(e) => Dimension::calc(self.ptr(e)),
        }
    }

    fn dim(&mut self, v: &SizeValue) -> Dimension {
        match v {
            SizeValue::Auto => Dimension::auto(),
            SizeValue::Lp(lp) => self.dim_lp(lp),
        }
    }

    fn min(&mut self, v: &SizeValue) -> LengthPercentageAuto {
        match v {
            SizeValue::Auto => LengthPercentageAuto::auto(),
            SizeValue::Lp(lp) => self.lpa(lp),
        }
    }

    fn max(&mut self, v: &MaxSizeValue) -> LengthPercentageAuto {
        match v {
            MaxSizeValue::None => LengthPercentageAuto::auto(),
            MaxSizeValue::Lp(lp) => self.lpa(lp),
        }
    }

    /// An inset (`top`/`right`/`bottom`/`left`). Percentages are exact: the 1/64 truncation of
    /// X-FLEX variant C is for the sizes of flex items only.
    fn inset(&mut self, v: &InsetValue) -> LengthPercentageAuto {
        match v {
            InsetValue::Auto => LengthPercentageAuto::auto(),
            InsetValue::Lp(lp) => {
                let saved = std::mem::replace(&mut self.truncate_percentages, false);
                let out = self.lpa(lp);
                self.truncate_percentages = saved;
                out
            }
        }
    }

    fn margin(&mut self, v: &MarginValue) -> LengthPercentageAuto {
        match v {
            MarginValue::Auto => LengthPercentageAuto::auto(),
            MarginValue::Lp(lp) => self.lpa(lp),
        }
    }

    /// A track bound's length-percentage as a resolved length, a percentage or a calc handle.
    fn track_lp(&mut self, v: &CssLp) -> TrackLp {
        match v {
            CssLp::Calc(e) if !e.has_percent() => TrackLp::Length(resolve_calc_value(e, 0.0)),
            CssLp::Px(px) => TrackLp::Length(quantize_input(*px)),
            CssLp::Percent(p) => TrackLp::Percent(*p),
            CssLp::Calc(e) => TrackLp::Calc(self.ptr(e)),
        }
    }

    fn track_min(&mut self, b: &TrackBreadth) -> MinTrackSizingFunction {
        match b {
            TrackBreadth::MinContent => MinTrackSizingFunction::min_content(),
            TrackBreadth::MaxContent => MinTrackSizingFunction::max_content(),
            TrackBreadth::Lp(lp) => match self.track_lp(lp) {
                TrackLp::Length(l) => MinTrackSizingFunction::length(l),
                TrackLp::Percent(p) => MinTrackSizingFunction::percent(p),
                TrackLp::Calc(c) => MinTrackSizingFunction::calc(c),
            },
            TrackBreadth::Auto | TrackBreadth::Fr(_) | TrackBreadth::FitContent(_) => {
                MinTrackSizingFunction::auto()
            }
        }
    }

    fn track_max(&mut self, b: &TrackBreadth) -> MaxTrackSizingFunction {
        match b {
            TrackBreadth::Auto => MaxTrackSizingFunction::auto(),
            TrackBreadth::MinContent => MaxTrackSizingFunction::min_content(),
            TrackBreadth::MaxContent => MaxTrackSizingFunction::max_content(),
            TrackBreadth::Fr(n) => MaxTrackSizingFunction::fr(*n),
            TrackBreadth::Lp(lp) => match self.track_lp(lp) {
                TrackLp::Length(l) => MaxTrackSizingFunction::length(l),
                TrackLp::Percent(p) => MaxTrackSizingFunction::percent(p),
                TrackLp::Calc(c) => MaxTrackSizingFunction::calc(c),
            },
            TrackBreadth::FitContent(lp) => match self.track_lp(lp) {
                TrackLp::Length(l) => MaxTrackSizingFunction::fit_content_px(l),
                TrackLp::Percent(p) => MaxTrackSizingFunction::fit_content_percent(p),
                TrackLp::Calc(c) => MaxTrackSizingFunction::fit_content_calc(c),
            },
        }
    }

    fn track(&mut self, t: &TrackSize) -> TrackSizingFunction {
        TrackSizingFunction {
            min: self.track_min(&t.min),
            max: self.track_max(&t.max),
        }
    }

    fn track_list(&mut self, items: &[TrackListItem]) -> Vec<GridTemplateComponent<String>> {
        items
            .iter()
            .map(|i| match i {
                TrackListItem::Track(t) => GridTemplateComponent::Single(self.track(t)),
                TrackListItem::Repeat(count, tracks, names) => {
                    GridTemplateComponent::Repeat(GridTemplateRepetition {
                        count: match count {
                            RepeatCount::Count(n) => RepetitionCount::Count(*n),
                            RepeatCount::AutoFill => RepetitionCount::AutoFill,
                            RepeatCount::AutoFit => RepetitionCount::AutoFit,
                        },
                        tracks: tracks.iter().map(|t| self.track(t)).collect(),
                        line_names: names.clone(),
                    })
                }
            })
            .collect()
    }

    fn gap(&mut self, v: &GapValue) -> LengthPercentage {
        match v {
            GapValue::Normal => LengthPercentage::length(0.0),
            GapValue::Lp(lp) => self.lp(lp),
        }
    }
}

enum TrackLp {
    Length(f32),
    Percent(f32),
    Calc(*const ()),
}

fn placement(v: &GridLineValue) -> GridPlacement {
    match v {
        GridLineValue::Auto => GridPlacement::Auto,
        GridLineValue::Line(n) => taffy::style_helpers::line(*n),
        GridLineValue::Span(n) => taffy::style_helpers::span(*n),
        // Taffy resolves names against the container's line names and areas (Grid 2 section 8.3).
        GridLineValue::NamedLine(name, n) => GridPlacement::NamedLine(name.clone(), *n),
        GridLineValue::NamedSpan(name, n) => GridPlacement::NamedSpan(name.clone(), *n),
    }
}

fn template_areas(a: &GridAreasValue) -> taffy::GridTemplateAreas<String> {
    taffy::GridTemplateAreas {
        areas: a
            .areas
            .iter()
            .map(|r| taffy::GridTemplateArea {
                name: r.name.clone(),
                row_start: r.row_start,
                row_end: r.row_end,
                column_start: r.column_start,
                column_end: r.column_end,
            })
            .collect(),
        row_count: a.rows,
        column_count: a.columns,
    }
}

fn items_keyword(k: AlignKeyword) -> Option<TAlignItems> {
    Some(match k {
        AlignKeyword::Normal => TAlignItems::NORMAL,
        AlignKeyword::Auto => return None,
        AlignKeyword::Start => TAlignItems::START,
        AlignKeyword::End => TAlignItems::END,
        AlignKeyword::FlexStart => TAlignItems::FLEX_START,
        AlignKeyword::FlexEnd => TAlignItems::FLEX_END,
        AlignKeyword::SelfStart => TAlignItems::SELF_START,
        AlignKeyword::SelfEnd => TAlignItems::SELF_END,
        AlignKeyword::Center => TAlignItems::CENTER,
        AlignKeyword::Baseline => TAlignItems::BASELINE,
        AlignKeyword::Stretch => TAlignItems::STRETCH,
        // Distribution keywords are not parsed for the item alignment properties.
        AlignKeyword::SpaceBetween | AlignKeyword::SpaceAround | AlignKeyword::SpaceEvenly => {
            TAlignItems::NORMAL
        }
    })
}

fn with_safety<T>(v: AlignValue, base: T, apply: impl Fn(T, taffy::AlignmentSafety) -> T) -> T {
    match v.safe {
        Some(true) => apply(base, taffy::AlignmentSafety::Safe),
        Some(false) => apply(base, taffy::AlignmentSafety::Unsafe),
        None => base,
    }
}

fn align_items(v: AlignValue) -> TAlignItems {
    let base = items_keyword(v.keyword).unwrap_or(TAlignItems::NORMAL);
    with_safety(v, base, |b, s| b.with_safety(s))
}

fn align_self(v: AlignValue) -> Option<TAlignItems> {
    items_keyword(v.keyword).map(|base| with_safety(v, base, |b, s| b.with_safety(s)))
}

fn align_content(v: AlignValue) -> TAlignContent {
    let base = match v.keyword {
        AlignKeyword::Normal | AlignKeyword::Auto => TAlignContent::NORMAL,
        // `baseline` content alignment falls back to `start` (CSS Align 3 §5.3).
        AlignKeyword::Start | AlignKeyword::SelfStart | AlignKeyword::Baseline => {
            TAlignContent::START
        }
        AlignKeyword::End | AlignKeyword::SelfEnd => TAlignContent::END,
        AlignKeyword::FlexStart => TAlignContent::FLEX_START,
        AlignKeyword::FlexEnd => TAlignContent::FLEX_END,
        AlignKeyword::Center => TAlignContent::CENTER,
        AlignKeyword::Stretch => TAlignContent::STRETCH,
        AlignKeyword::SpaceBetween => TAlignContent::SPACE_BETWEEN,
        AlignKeyword::SpaceAround => TAlignContent::SPACE_AROUND,
        AlignKeyword::SpaceEvenly => TAlignContent::SPACE_EVENLY,
    };
    with_safety(v, base, |b, s| b.with_safety(s))
}

/// What the style of a box depends on besides its own computed values.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StyleContext {
    /// The inner display of the parent box (item properties of flex and grid apply only there).
    pub parent_inner: Option<InnerDisplay>,
}

fn taffy_overflow(v: OverflowValue) -> Overflow {
    match v {
        OverflowValue::Visible => Overflow::Visible,
        OverflowValue::Hidden => Overflow::Hidden,
        OverflowValue::Clip => Overflow::Clip,
        OverflowValue::Scroll | OverflowValue::Auto => Overflow::Scroll,
    }
}

/// The out-of-flow descendants a box is the containing block of (FR-056, FR-059, FR-061, R-07):
/// positioned boxes (sticky included) claim `absolute` ones; a box with a `transform`,
/// `perspective`, `filter` or matching `will-change` claims `absolute` and `fixed` ones.
pub(crate) fn containing_block_claims(s: &BoxStyle) -> taffy::ContainingBlockClaims {
    taffy::ContainingBlockClaims {
        absolute: s.position.is_positioned() || s.claims_fixed_descendants,
        fixed: s.claims_fixed_descendants,
    }
}

/// The intrinsic sizing keyword of an absolutely positioned box as Taffy's sizing keyword (015 HTML-1): Taffy's
/// out-of-flow algorithm resolves it against the inset-modified containing block, so a `fit-content` box between
/// two insets is shrink-to-fit and its auto margins centre it (the UA `dialog` rules).
fn oof_keyword(
    s: &BoxStyle,
    kw: Option<&crate::layout::box_tree::WidthKeyword>,
) -> Option<Dimension> {
    if !s.position.is_out_of_flow() {
        return None;
    }
    match kw? {
        crate::layout::box_tree::WidthKeyword::MinContent => Some(Dimension::min_content()),
        crate::layout::box_tree::WidthKeyword::MaxContent => Some(Dimension::max_content()),
        crate::layout::box_tree::WidthKeyword::FitContent(None) => Some(Dimension::fit_content()),
        _ => None,
    }
}

/// Build the Taffy style of a block-level box with inner display `inner`.
pub(crate) fn taffy_style(
    calcs: &mut CalcTable,
    s: &BoxStyle,
    inner: InnerDisplay,
    ctx: StyleContext,
) -> Style {
    calcs.truncate_percentages = ctx.parent_inner == Some(InnerDisplay::Flex);
    let mut style = Style {
        display: match inner {
            InnerDisplay::Flow => Display::Block,
            // `flow-root` is the block algorithm with an independent formatting context (FR-022).
            InnerDisplay::FlowRoot => Display::FlowRoot,
            InnerDisplay::Flex => Display::Flex,
            InnerDisplay::Grid => Display::Grid,
        },
        direction: match s.direction {
            Direction::Ltr => taffy::Direction::Ltr,
            Direction::Rtl => taffy::Direction::Rtl,
        },
        box_sizing: match s.box_sizing {
            CssBoxSizing::ContentBox => BoxSizing::ContentBox,
            CssBoxSizing::BorderBox => BoxSizing::BorderBox,
        },
        size: Size {
            width: oof_keyword(s, s.width_kw.as_ref()).unwrap_or_else(|| calcs.dim(&s.width)),
            height: oof_keyword(s, s.height_kw.as_ref()).unwrap_or_else(|| calcs.dim(&s.height)),
        },
        min_size: Size {
            width: calcs.min(&s.min_width),
            height: calcs.min(&s.min_height),
        },
        max_size: Size {
            width: calcs.max(&s.max_width),
            height: calcs.max(&s.max_height),
        },
        margin: Rect {
            top: calcs.margin(&s.margin.top),
            right: calcs.margin(&s.margin.right),
            bottom: calcs.margin(&s.margin.bottom),
            left: calcs.margin(&s.margin.left),
        },
        padding: Rect {
            top: calcs.lp(&s.padding.top),
            right: calcs.lp(&s.padding.right),
            bottom: calcs.lp(&s.padding.bottom),
            left: calcs.lp(&s.padding.left),
        },
        border: Rect {
            top: LengthPercentage::length(quantize_input(s.border.top)),
            right: LengthPercentage::length(quantize_input(s.border.right)),
            bottom: LengthPercentage::length(quantize_input(s.border.bottom)),
            left: LengthPercentage::length(quantize_input(s.border.left)),
        },
        float: match s.float {
            crate::layout::box_tree::FloatValue::None => taffy::Float::None,
            crate::layout::box_tree::FloatValue::Left => taffy::Float::Left,
            crate::layout::box_tree::FloatValue::Right => taffy::Float::Right,
        },
        clear: match s.clear {
            crate::layout::box_tree::ClearValue::None => taffy::Clear::None,
            crate::layout::box_tree::ClearValue::Left => taffy::Clear::Left,
            crate::layout::box_tree::ClearValue::Right => taffy::Clear::Right,
            crate::layout::box_tree::ClearValue::Both => taffy::Clear::Both,
        },
        position: match s.position {
            PositionValue::Static => Position::Static,
            PositionValue::Relative => Position::Relative,
            PositionValue::Absolute => Position::Absolute,
            PositionValue::Fixed => Position::Fixed,
            PositionValue::Sticky => Position::Sticky,
        },
        inset: Rect {
            top: calcs.inset(&s.inset.top),
            right: calcs.inset(&s.inset.right),
            bottom: calcs.inset(&s.inset.bottom),
            left: calcs.inset(&s.inset.left),
        },
        // FR-062/FR-063 (research R-09): `auto` and `scroll` are scroll containers whose scrollbar
        // takes no layout space (overlay scrollbars, `scrollbar_width` 0); `hidden` is a scroll
        // container without scrollbars; `clip` clips without being one.
        overflow: Point {
            x: taffy_overflow(s.overflow_x),
            y: taffy_overflow(s.overflow_y),
        },
        scrollbar_width: 0.0,
        ..Style::DEFAULT
    };
    if inner == InnerDisplay::FlowRoot {
        // The block algorithm reads independent-formatting-context status from `contain`;
        // paint containment has no other layout effect (it only establishes the context).
        style.contain = Contain::PAINT;
    }
    // Flex item properties apply only to the children of a flex container (CSS Flexbox 1 §3).
    if ctx.parent_inner == Some(InnerDisplay::Flex) {
        style.flex_grow = s.flex.grow;
        style.flex_shrink = s.flex.shrink;
        style.flex_basis = match &s.flex.basis {
            FlexBasisValue::Auto => Dimension::auto(),
            FlexBasisValue::Content => Dimension::content(),
            FlexBasisValue::Lp(lp) => calcs.dim_lp(lp),
        };
        style.align_self = align_self(s.align.align_self);
    }
    match inner {
        InnerDisplay::Flex => {
            style.flex_direction = match s.flex.direction {
                FlexDirectionValue::Row => FlexDirection::Row,
                FlexDirectionValue::RowReverse => FlexDirection::RowReverse,
                FlexDirectionValue::Column => FlexDirection::Column,
                FlexDirectionValue::ColumnReverse => FlexDirection::ColumnReverse,
            };
            style.flex_wrap = match s.flex.wrap {
                FlexWrapValue::NoWrap => FlexWrap::NoWrap,
                FlexWrapValue::Wrap => FlexWrap::Wrap,
                FlexWrapValue::WrapReverse => FlexWrap::WrapReverse,
            };
            style.gap = Size {
                width: calcs.gap(&s.align.column_gap),
                height: calcs.gap(&s.align.row_gap),
            };
            style.align_items = align_items(s.align.align_items);
            style.align_content = align_content(s.align.align_content);
            style.justify_content = align_content(s.align.justify_content);
        }
        InnerDisplay::Grid => {
            style.grid_template_columns = calcs.track_list(&s.grid.columns);
            style.grid_template_rows = calcs.track_list(&s.grid.rows);
            style.grid_template_column_names = s.grid.column_names.to_vec();
            style.grid_template_row_names = s.grid.row_names.to_vec();
            // `subgrid` (CSS Grid 2 section 9; Taffy patch P-008-4): the tracks come from the parent grid, and a
            // subgrid whose gap is `normal` uses its parent's gap.
            style.grid_template_columns_subgrid = s.grid.column_subgrid;
            style.grid_template_rows_subgrid = s.grid.row_subgrid;
            style.gap_is_normal = Size {
                width: matches!(s.align.column_gap, GapValue::Normal),
                height: matches!(s.align.row_gap, GapValue::Normal),
            };
            style.grid_template_areas = s.grid.areas.as_ref().map(template_areas);
            style.grid_auto_columns = s.grid.auto_columns.iter().map(|t| calcs.track(t)).collect();
            style.grid_auto_rows = s.grid.auto_rows.iter().map(|t| calcs.track(t)).collect();
            style.grid_auto_flow = match s.grid.auto_flow {
                GridAutoFlowValue::Row => GridAutoFlow::Row,
                GridAutoFlowValue::Column => GridAutoFlow::Column,
                GridAutoFlowValue::RowDense => GridAutoFlow::RowDense,
                GridAutoFlowValue::ColumnDense => GridAutoFlow::ColumnDense,
            };
            style.gap = Size {
                width: calcs.gap(&s.align.column_gap),
                height: calcs.gap(&s.align.row_gap),
            };
            style.align_items = align_items(s.align.align_items);
            style.justify_items = align_items(s.align.justify_items);
            style.align_content = align_content(s.align.align_content);
            style.justify_content = align_content(s.align.justify_content);
        }
        InnerDisplay::Flow | InnerDisplay::FlowRoot => {}
    }
    if ctx.parent_inner == Some(InnerDisplay::Grid) {
        style.grid_column = Line {
            start: placement(&s.grid.column.0),
            end: placement(&s.grid.column.1),
        };
        style.grid_row = Line {
            start: placement(&s.grid.row.0),
            end: placement(&s.grid.row.1),
        };
        style.align_self = align_self(s.align.align_self);
        style.justify_self = align_self(s.align.justify_self);
    }
    style
}
