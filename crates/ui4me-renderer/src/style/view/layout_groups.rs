//! owner: 006-style-engine (lane L-GRID-r14, 006:T038) - the flex, box-alignment and grid groups of a
//! `ComputedStyleView`: the layout-facing longhands of CSS Flexbox 1, CSS Box Alignment 3 and CSS Grid 2
//! as typed values, replacing the CSSOM-text bridge that 008 used while these groups were missing.
//! No upstream type appears in them.
//!
//! Values are **computed values** (lengths in CSS px, percentages as fractions, `calc()` as a
//! [`CalcHandle`]). Forms that this engine does not lay out are still represented (for example
//! `subgrid`, `last baseline`, `left` as an alignment) so the consumer can report them instead of the view
//! hiding them. Logical properties do not occur here: the flex, alignment and grid properties are
//! flow-relative by definition and were never physical.

use super::groups::{self, LengthPercentage, Size};
use style::properties::ComputedValues;
use style::values::computed::{
    GridLine as StyloGridLine, GridTemplateComponent as StyloTemplate,
    TrackBreadth as StyloBreadth, TrackSize as StyloTrackSize,
};
use style::values::generics::flex::FlexBasis as StyloFlexBasis;
use style::values::generics::grid::{
    GridTemplateComponent, RepeatCount as StyloRepeat, TrackListValue,
};
use style::values::generics::length::LengthPercentageOrNormal;
use style::values::specified::align::AlignFlags;
use style::values::specified::position::GridTemplateAreas as StyloAreas;
use style::values::specified::{FlexDirection as StyloDirection, FlexWrap as StyloWrap};

// ------------------------------------------------------------------ flex (CSS Flexbox 1)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
}

/// `flex-basis`: `content` or a `<width>` (`auto`, a length-percentage, an intrinsic keyword, ...).
#[derive(Clone, Debug, PartialEq)]
pub enum FlexBasis {
    Content,
    Size(Size),
}

/// The flex container and item longhands, plus `order` (shared with grid items).
#[derive(Clone, Debug, PartialEq)]
pub struct Flex {
    pub direction: FlexDirection,
    pub wrap: FlexWrap,
    /// `flex-wrap: wrap balance` (CSS Flexbox 2): not laid out; the consumer reports it.
    pub wrap_balance: bool,
    pub grow: f32,
    pub shrink: f32,
    pub basis: FlexBasis,
    pub order: i32,
}

// ------------------------------------------------------------------ box alignment (CSS Box Alignment 3)

/// A box-alignment keyword (the enumeration part of `<self-position>` / `<content-distribution>` /
/// `<baseline-position>` / `normal` / `stretch` / `auto`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlignKeyword {
    /// `auto` (`align-self`, `justify-self`).
    Auto,
    Normal,
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Left,
    Right,
    /// `baseline` / `first baseline`.
    Baseline,
    LastBaseline,
    Stretch,
    SelfStart,
    SelfEnd,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    /// `anchor-center` (anchor positioning is Off in the profile; never computed).
    AnchorCenter,
}

/// A computed alignment value: the keyword with its overflow-alignment keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Align {
    pub keyword: AlignKeyword,
    /// `Some(true)` = `safe`, `Some(false)` = `unsafe`, `None` = the default.
    pub safe: Option<bool>,
    /// A `legacy` modifier (`justify-items: legacy [left|right|center]`), which layout does not model.
    pub legacy: bool,
}

/// `row-gap` / `column-gap`.
#[derive(Clone, Debug, PartialEq)]
pub enum Gap {
    Normal,
    LengthPercentage(LengthPercentage),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alignment {
    pub align_items: Align,
    pub align_self: Align,
    pub align_content: Align,
    pub justify_items: Align,
    pub justify_self: Align,
    pub justify_content: Align,
    pub row_gap: Gap,
    pub column_gap: Gap,
}

// ------------------------------------------------------------------ grid (CSS Grid 2)

/// One bound of a track size.
#[derive(Clone, Debug, PartialEq)]
pub enum TrackBreadth {
    Breadth(LengthPercentage),
    /// `<flex>` (`fr`).
    Flex(f32),
    Auto,
    MinContent,
    MaxContent,
}

/// `<track-size>`.
#[derive(Clone, Debug, PartialEq)]
pub enum TrackSize {
    Breadth(TrackBreadth),
    Minmax(TrackBreadth, TrackBreadth),
    FitContent(LengthPercentage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatCount {
    Number(u32),
    AutoFill,
    AutoFit,
}

/// A `repeat()` of a track list. `line_names` holds one set per line of one repetition (`tracks.len() + 1`
/// sets, the sets may be empty), or nothing when none of its lines is named.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackRepeat {
    pub count: RepeatCount,
    pub line_names: Vec<Vec<String>>,
    pub tracks: Vec<TrackSize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TrackListItem {
    Track(TrackSize),
    Repeat(TrackRepeat),
}

/// A `<track-list>` with its line names: `line_names[i]` names the line before `items[i]` and the last set
/// the line after the last item (`items.len() + 1` sets). A `repeat()` item contributes its own inner names.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrackList {
    pub items: Vec<TrackListItem>,
    pub line_names: Vec<Vec<String>>,
}

/// `grid-template-columns` / `grid-template-rows`.
#[derive(Clone, Debug, PartialEq)]
pub enum GridTemplate {
    None,
    Tracks(TrackList),
    /// `subgrid [<line-name-list>]` (CSS Grid 2 section 9; profile row `css.value.subgrid`, 015 GRID-1): the
    /// name sets of the subgrid's lines, one per line from its first, with `repeat(<integer>, ...)` expanded.
    /// `repeat(auto-fill, ...)` is expanded once (the number of lines is only known at layout).
    Subgrid(Vec<Vec<String>>),
}

/// A named area of `grid-template-areas`: line numbers are 1-based; `row_end`/`column_end` are the lines
/// that end the area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridArea {
    pub name: String,
    pub row_start: u32,
    pub row_end: u32,
    pub column_start: u32,
    pub column_end: u32,
}

/// `grid-template-areas`: the named areas and the size of the template, which can exceed the areas' extent
/// through `.` cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridAreas {
    pub areas: Vec<GridArea>,
    pub rows: u32,
    pub columns: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAutoFlow {
    Row,
    Column,
    RowDense,
    ColumnDense,
}

/// One side of `grid-row` / `grid-column`: the computed `<grid-line>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridLine {
    Auto,
    /// `<integer>`: a 1-based line number, negative counting from the end.
    Number(i32),
    /// `<custom-ident>` alone (a line name or, through `<name>-start`/`-end`, an area), or with an `<integer>`.
    Named {
        name: String,
        nth: i32,
    },
    /// `span <integer>`.
    Span(i32),
    /// `span <custom-ident>`, with an optional count (`1` when omitted).
    NamedSpan {
        name: String,
        count: i32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub columns: GridTemplate,
    pub rows: GridTemplate,
    pub auto_columns: Vec<TrackSize>,
    pub auto_rows: Vec<TrackSize>,
    pub auto_flow: GridAutoFlow,
    pub areas: Option<GridAreas>,
    pub column_start: GridLine,
    pub column_end: GridLine,
    pub row_start: GridLine,
    pub row_end: GridLine,
}

// ------------------------------------------------------------------ extraction (crate-internal)

pub(crate) fn flex(epoch: u64, cv: &ComputedValues) -> Flex {
    let p = cv.get_position();
    Flex {
        direction: match p.flex_direction {
            StyloDirection::Row => FlexDirection::Row,
            StyloDirection::RowReverse => FlexDirection::RowReverse,
            StyloDirection::Column => FlexDirection::Column,
            StyloDirection::ColumnReverse => FlexDirection::ColumnReverse,
        },
        wrap: if p.flex_wrap.contains(StyloWrap::WRAP_REVERSE) {
            FlexWrap::WrapReverse
        } else if p.flex_wrap.contains(StyloWrap::WRAP) {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        wrap_balance: p.flex_wrap.contains(StyloWrap::BALANCE),
        grow: p.flex_grow.0,
        shrink: p.flex_shrink.0,
        basis: match &p.flex_basis {
            StyloFlexBasis::Content => FlexBasis::Content,
            StyloFlexBasis::Size(s) => FlexBasis::Size(groups::size(epoch, s)),
        },
        order: p.order,
    }
}

fn align(flags: AlignFlags) -> Align {
    let value = flags.value();
    let keyword = match value {
        AlignFlags::AUTO => AlignKeyword::Auto,
        AlignFlags::NORMAL => AlignKeyword::Normal,
        AlignFlags::START => AlignKeyword::Start,
        AlignFlags::END => AlignKeyword::End,
        AlignFlags::FLEX_START => AlignKeyword::FlexStart,
        AlignFlags::FLEX_END => AlignKeyword::FlexEnd,
        AlignFlags::CENTER => AlignKeyword::Center,
        AlignFlags::LEFT => AlignKeyword::Left,
        AlignFlags::RIGHT => AlignKeyword::Right,
        AlignFlags::BASELINE => AlignKeyword::Baseline,
        AlignFlags::LAST_BASELINE => AlignKeyword::LastBaseline,
        AlignFlags::STRETCH => AlignKeyword::Stretch,
        AlignFlags::SELF_START => AlignKeyword::SelfStart,
        AlignFlags::SELF_END => AlignKeyword::SelfEnd,
        AlignFlags::SPACE_BETWEEN => AlignKeyword::SpaceBetween,
        AlignFlags::SPACE_AROUND => AlignKeyword::SpaceAround,
        AlignFlags::SPACE_EVENLY => AlignKeyword::SpaceEvenly,
        AlignFlags::ANCHOR_CENTER => AlignKeyword::AnchorCenter,
        // A bare `legacy` has an empty enumeration part: it is `normal` for layout.
        _ => AlignKeyword::Normal,
    };
    let f = flags.flags();
    Align {
        keyword,
        safe: if f.contains(AlignFlags::SAFE) {
            Some(true)
        } else if f.contains(AlignFlags::UNSAFE) {
            Some(false)
        } else {
            None
        },
        legacy: f.contains(AlignFlags::LEGACY),
    }
}

fn gap(
    epoch: u64,
    g: &LengthPercentageOrNormal<style::values::computed::NonNegativeLengthPercentage>,
) -> Gap {
    match g {
        LengthPercentageOrNormal::Normal => Gap::Normal,
        LengthPercentageOrNormal::LengthPercentage(v) => {
            Gap::LengthPercentage(groups::lp(epoch, &v.0))
        }
    }
}

pub(crate) fn alignment(epoch: u64, cv: &ComputedValues) -> Alignment {
    let p = cv.get_position();
    Alignment {
        align_items: align(p.align_items.0),
        align_self: align(p.align_self.0),
        align_content: align(p.align_content.primary()),
        justify_items: align(p.justify_items.computed.0.0),
        justify_self: align(p.justify_self.0),
        justify_content: align(p.justify_content.primary()),
        row_gap: gap(epoch, &p.row_gap),
        column_gap: gap(epoch, &p.column_gap),
    }
}

fn breadth(epoch: u64, b: &StyloBreadth) -> TrackBreadth {
    use style::values::generics::grid::TrackBreadth as B;
    match b {
        B::Breadth(l) => TrackBreadth::Breadth(groups::lp(epoch, l)),
        B::Flex(f) => TrackBreadth::Flex(f.0),
        B::Auto => TrackBreadth::Auto,
        B::MinContent => TrackBreadth::MinContent,
        B::MaxContent => TrackBreadth::MaxContent,
    }
}

fn track_size(epoch: u64, t: &StyloTrackSize) -> TrackSize {
    use style::values::generics::grid::TrackSize as T;
    match t {
        T::Breadth(b) => TrackSize::Breadth(breadth(epoch, b)),
        T::Minmax(min, max) => TrackSize::Minmax(breadth(epoch, min), breadth(epoch, max)),
        // `fit-content()` stores its limit as a breadth; only a length-percentage is possible.
        T::FitContent(b) => match breadth(epoch, b) {
            TrackBreadth::Breadth(l) => TrackSize::FitContent(l),
            other => TrackSize::Breadth(other),
        },
    }
}

fn names(set: &[style::values::CustomIdent]) -> Vec<String> {
    set.iter().map(|n| n.0.to_string()).collect()
}

fn name_sets(sets: &[style::OwnedSlice<style::values::CustomIdent>]) -> Vec<Vec<String>> {
    sets.iter().map(|s| names(s)).collect()
}

fn template(epoch: u64, t: &StyloTemplate) -> GridTemplate {
    match t {
        GridTemplateComponent::None => GridTemplate::None,
        GridTemplateComponent::Subgrid(list) => {
            use style::values::generics::grid::LineNameListValue;
            let mut sets: Vec<Vec<String>> = Vec::new();
            for value in list.line_names.iter() {
                match value {
                    LineNameListValue::LineNames(set) => sets.push(names(set)),
                    LineNameListValue::Repeat(r) => {
                        let count = match r.count {
                            StyloRepeat::Number(n) => n.max(1) as usize,
                            _ => 1,
                        };
                        for _ in 0..count {
                            sets.extend(name_sets(&r.line_names));
                        }
                    }
                }
            }
            GridTemplate::Subgrid(sets)
        }
        GridTemplateComponent::TrackList(list) => {
            let items = list
                .values
                .iter()
                .map(|v| match v {
                    TrackListValue::TrackSize(s) => TrackListItem::Track(track_size(epoch, s)),
                    TrackListValue::TrackRepeat(r) => TrackListItem::Repeat(TrackRepeat {
                        count: match r.count {
                            StyloRepeat::Number(n) => RepeatCount::Number(n.max(1) as u32),
                            StyloRepeat::AutoFill => RepeatCount::AutoFill,
                            StyloRepeat::AutoFit => RepeatCount::AutoFit,
                        },
                        line_names: name_sets(&r.line_names),
                        tracks: r.track_sizes.iter().map(|s| track_size(epoch, s)).collect(),
                    }),
                })
                .collect();
            GridTemplate::Tracks(TrackList {
                items,
                line_names: name_sets(&list.line_names),
            })
        }
    }
}

fn grid_line(l: &StyloGridLine) -> GridLine {
    let name = l.ident.0.to_string();
    if l.is_span {
        return if name.is_empty() {
            GridLine::Span(l.line_num.max(1))
        } else {
            GridLine::NamedSpan {
                name,
                count: l.line_num.max(1),
            }
        };
    }
    match (name.is_empty(), l.line_num) {
        (true, 0) => GridLine::Auto,
        (true, n) => GridLine::Number(n),
        (false, n) => GridLine::Named { name, nth: n },
    }
}

fn implicit(epoch: u64, t: &style::values::computed::ImplicitGridTracks) -> Vec<TrackSize> {
    t.0.iter().map(|s| track_size(epoch, s)).collect()
}

pub(crate) fn grid(epoch: u64, cv: &ComputedValues) -> Grid {
    use style::values::specified::position::GridAutoFlow as F;
    let p = cv.get_position();
    let column = p.grid_auto_flow.contains(F::COLUMN);
    let dense = p.grid_auto_flow.contains(F::DENSE);
    let areas = match &p.grid_template_areas {
        StyloAreas::None => None,
        StyloAreas::Areas(a) => Some(GridAreas {
            areas: a
                .0
                .areas
                .iter()
                .map(|n| GridArea {
                    name: n.name.to_string(),
                    row_start: n.rows.start,
                    row_end: n.rows.end,
                    column_start: n.columns.start,
                    column_end: n.columns.end,
                })
                .collect(),
            rows: a.0.strings.len() as u32,
            columns: a.0.width,
        }),
    };
    Grid {
        columns: template(epoch, &p.grid_template_columns),
        rows: template(epoch, &p.grid_template_rows),
        auto_columns: implicit(epoch, &p.grid_auto_columns),
        auto_rows: implicit(epoch, &p.grid_auto_rows),
        auto_flow: match (column, dense) {
            (false, false) => GridAutoFlow::Row,
            (false, true) => GridAutoFlow::RowDense,
            (true, false) => GridAutoFlow::Column,
            (true, true) => GridAutoFlow::ColumnDense,
        },
        areas,
        column_start: grid_line(&p.grid_column_start),
        column_end: grid_line(&p.grid_column_end),
        row_start: grid_line(&p.grid_row_start),
        row_end: grid_line(&p.grid_row_end),
    }
}

// ------------------------------------------------------------------ containment (CSS Containment 2, CSS Conditional 5; 015 CQ-2)

/// `container-type` (CSS Conditional 5 section 3.1). `scroll-state` is not parsed (its pref is off).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ContainerType {
    #[default]
    Normal,
    /// `size`: a query container for both axes (size, layout and style containment).
    Size,
    /// `inline-size`: a query container for the inline axis (inline-size, layout and style containment).
    InlineSize,
}

/// The containment of a box: the computed `contain` value united with the containment that
/// `container-type` implies (CSS Conditional 5 section 3.1: `size` applies size, layout and style
/// containment, `inline-size` inline-size, layout and style containment; the reference browser
/// scopes counters and quotes inside a query container, measured 2026-11-07, lane L-CQ D-LCQ-07). Whether a
/// containment type has an effect on a given box (none on non-atomic inline boxes and on internal
/// table boxes, CSS Containment 2 section 3) is the consumer's decision: it knows the box. Physical
/// axes: the inline axis is horizontal (vertical writing modes are not supported by this engine).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Containment {
    /// Size containment in the inline axis (`size`, `inline-size`, `strict`).
    pub size_inline: bool,
    /// Size containment in the block axis (`size`, `strict`).
    pub size_block: bool,
    pub layout: bool,
    pub paint: bool,
    pub style: bool,
    pub container_type: ContainerType,
    /// `container-name` (empty for `none`).
    pub container_names: Vec<String>,
    /// `content-visibility: hidden` (0.9 F0903): the element's contents are skipped.
    pub skips_contents: bool,
    /// `contain-intrinsic-width` / `-height` in CSS px × 64 (`<length>` and `auto <length>`; the
    /// remembered size of `auto` is not kept): the size a size-contained box has as if it held
    /// content of that size.
    pub intrinsic: (Option<i32>, Option<i32>),
}

fn longhand_text(cv: &ComputedValues, name: &str) -> String {
    crate::style::computed::longhand_by_name(name)
        .map(|id| {
            cv.computed_value_to_string(style::properties::PropertyDeclarationId::Longhand(id))
        })
        .unwrap_or_default()
}

/// `none`, `auto none` → None; `<n>px` / `auto <n>px` → n × 64.
fn intrinsic_px64(text: &str) -> Option<i32> {
    let t = text.trim().strip_prefix("auto").unwrap_or(text).trim();
    let px: f64 = t.strip_suffix("px")?.parse().ok()?;
    (px.is_finite() && px >= 0.0).then(|| (px.min(1.0e7) * 64.0).round() as i32)
}

impl Containment {
    /// Any containment type is in effect.
    pub fn any(&self) -> bool {
        self.size_inline
            || self.size_block
            || self.layout
            || self.paint
            || self.style
            || self.skips_contents
    }
}

pub(crate) fn containment(cv: &ComputedValues) -> Containment {
    use style::values::computed::ContainerType as T;
    use style::values::specified::Contain as C;
    let b = cv.get_box();
    let contain = *b.get_contain();
    let ty = *b.get_container_type();
    let container_type = if ty.contains(T::SIZE) {
        ContainerType::Size
    } else if ty.contains(T::INLINE_SIZE) {
        ContainerType::InlineSize
    } else {
        ContainerType::Normal
    };
    let implied = container_type != ContainerType::Normal;
    // `content-visibility` (CSS Containment 2 section 4): `hidden` implies size, layout, style and
    // paint containment and skips the contents; `auto` implies layout, style and paint
    // containment (its contents are never skipped here, so size containment never applies).
    let cvis = longhand_text(cv, "content-visibility");
    let (hidden, auto) = (cvis == "hidden", cvis == "auto");
    Containment {
        size_inline: contain.contains(C::INLINE_SIZE) || implied || hidden,
        size_block: contain.contains(C::BLOCK_SIZE)
            || container_type == ContainerType::Size
            || hidden,
        layout: contain.contains(C::LAYOUT) || implied || hidden || auto,
        paint: contain.contains(C::PAINT) || hidden || auto,
        style: contain.contains(C::STYLE) || implied || hidden || auto,
        skips_contents: hidden,
        intrinsic: (
            intrinsic_px64(&longhand_text(cv, "contain-intrinsic-width")),
            intrinsic_px64(&longhand_text(cv, "contain-intrinsic-height")),
        ),
        container_type,
        container_names: b
            .get_container_name()
            .0
            .iter()
            .map(|n| n.0.to_string())
            .collect(),
    }
}

// ------------------------------------------------------------------ fragmentation (CSS Multicol 1, CSS Break 3; 015 FRAG-2a)

/// `break-before` / `break-after` (CSS Break 3 section 3.1; the legacy `page-break-before|after` map onto them in
/// the engine). `recto` and `verso` are parsed with the carried patch P-006-9 (015 PAGE-1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BreakBetween {
    #[default]
    Auto,
    Avoid,
    AvoidPage,
    AvoidColumn,
    Page,
    Left,
    Right,
    Column,
    /// `always`: a break in the innermost fragmentation context.
    Always,
    /// `recto` / `verso` (015 PAGE-1): `right` / `left` in a left-to-right page progression.
    Recto,
    Verso,
}

impl BreakBetween {
    /// A forced break value (`page`, `left`, `right`, `column`, `always`).
    pub fn is_forced(self) -> bool {
        matches!(
            self,
            BreakBetween::Page
                | BreakBetween::Left
                | BreakBetween::Right
                | BreakBetween::Recto
                | BreakBetween::Verso
                | BreakBetween::Column
                | BreakBetween::Always
        )
    }
    /// An avoid value (`avoid`, `avoid-page`, `avoid-column`).
    pub fn is_avoid(self) -> bool {
        matches!(
            self,
            BreakBetween::Avoid | BreakBetween::AvoidPage | BreakBetween::AvoidColumn
        )
    }
}

/// `break-inside` (CSS Break 3 section 3.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BreakWithin {
    #[default]
    Auto,
    Avoid,
    AvoidPage,
    AvoidColumn,
}

/// `column-fill` (CSS Multicol 1 section 7.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColumnFill {
    #[default]
    Balance,
    Auto,
}

/// The multicolumn and fragmentation longhands of a box. `column-gap` is in [`Alignment`] (shared with flex and
/// grid). `orphans` and `widows` are servo-mode longhands of the vendored engine added by patch P-006-7 (015 FRAG-2c;
/// Gecko never implemented them).
#[derive(Clone, Debug, PartialEq)]
pub struct Fragmentation {
    /// `column-count` (`None` = `auto`).
    pub column_count: Option<u32>,
    /// `column-width` in px (`None` = `auto`).
    pub column_width: Option<f32>,
    pub column_fill: ColumnFill,
    /// `column-span: all`.
    pub column_span_all: bool,
    /// `column-rule-width` in px (0 when the style is `none` or `hidden`).
    pub column_rule_width: f32,
    pub column_rule_style: groups::BorderStyle,
    pub column_rule_color: groups::Color,
    pub break_before: BreakBetween,
    pub break_after: BreakBetween,
    pub break_inside: BreakWithin,
    pub orphans: u32,
    pub widows: u32,
    /// `box-decoration-break: clone`.
    pub decoration_clone: bool,
    /// `page` (CSS Paged Media 3 section 6; `None` = `auto`; carried patch P-006-9, 015 PAGE-1).
    pub page: Option<String>,
}

fn break_between(b: style::values::computed::BreakBetween) -> BreakBetween {
    use style::values::computed::BreakBetween as B;
    match b {
        B::Auto => BreakBetween::Auto,
        B::Avoid => BreakBetween::Avoid,
        B::AvoidPage => BreakBetween::AvoidPage,
        B::AvoidColumn => BreakBetween::AvoidColumn,
        B::Page => BreakBetween::Page,
        B::Left => BreakBetween::Left,
        B::Right => BreakBetween::Right,
        B::Column => BreakBetween::Column,
        B::Always => BreakBetween::Always,
        B::Recto => BreakBetween::Recto,
        B::Verso => BreakBetween::Verso,
    }
}

pub(crate) fn fragmentation(cv: &ComputedValues) -> Fragmentation {
    use style::values::computed::BreakWithin as W;
    use style::values::generics::column::ColumnCount;
    use style::values::generics::length::GenericLengthPercentageOrAuto as LpAuto;
    let c = cv.get_column();
    let b = cv.get_box();
    let rule_style = groups::border_style_of(c.column_rule_style);
    Fragmentation {
        column_count: match c.column_count {
            ColumnCount::Integer(n) => Some(n.0.max(1) as u32),
            ColumnCount::Auto => None,
        },
        column_width: match &c.column_width {
            LpAuto::LengthPercentage(l) => Some(l.0.px()),
            LpAuto::Auto => None,
        },
        column_fill: match c.column_fill {
            style::values::computed::ColumnFill::Auto => ColumnFill::Auto,
            _ => ColumnFill::Balance,
        },
        column_span_all: matches!(c.column_span, style::values::computed::ColumnSpan::All),
        column_rule_width: if matches!(
            rule_style,
            groups::BorderStyle::None | groups::BorderStyle::Hidden
        ) {
            0.0
        } else {
            c.column_rule_width.0.to_f32_px()
        },
        column_rule_style: rule_style,
        column_rule_color: groups::color(cv.resolve_color(&c.column_rule_color)),
        break_before: break_between(b.break_before),
        break_after: break_between(b.break_after),
        break_inside: match b.break_inside {
            W::Auto => BreakWithin::Auto,
            W::Avoid => BreakWithin::Avoid,
            W::AvoidPage => BreakWithin::AvoidPage,
            W::AvoidColumn => BreakWithin::AvoidColumn,
        },
        // Servo-mode longhands added by P-006-7 (FRAG-2c): positive integers, inherited.
        orphans: cv.get_inherited_text().orphans.max(1) as u32,
        widows: cv.get_inherited_text().widows.max(1) as u32,
        decoration_clone: matches!(
            cv.get_border().box_decoration_break,
            style::values::computed::BoxDecorationBreak::Clone
        ),
        page: match b.page.clone() {
            style::values::computed::PageName::PageName(n) => Some(n.0.to_string()),
            style::values::computed::PageName::Auto => None,
        },
    }
}
