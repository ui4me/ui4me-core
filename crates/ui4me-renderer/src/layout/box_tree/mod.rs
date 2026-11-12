//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! Box generation (U4C-BOX; 008 `contracts/box-tree.md`). This module also holds the slice's
//! narrow **input layer**: a plain per-element `BoxStyle` with the FR-017 consumed values, read
//! through the `StyledSource` trait. The style engine's `ComputedStyleView` (006, lane D) is
//! mapped into `BoxStyle` by an adapter; `InputTree` is a plain arena implementation used by
//! tests and by any caller that already holds computed values.

pub mod anonymous;
pub mod build;
pub mod projection_reports;
pub mod typed_layout;

pub use build::{
    BoxId, BoxKind, BoxOwner, BoxStyleCache, BoxTag, BoxTree, LayoutBox, StyleBridgeSource,
    build_box_tree, build_box_tree_reusing,
};

use crate::text::paragraph::TextStyle;
use std::sync::Arc;
use ui4me_engine_types::diag::FeatureId;
use ui4me_engine_types::ids::{DocumentId, NodeId};

use crate::layout::coordinator::calc::CalcExpr;

/// Computed `display` as box generation consumes it (FR-002, FR-017, FR-021). The value is the
/// style engine's computed (blockified) one and is never re-derived (FR-003). Table-family values
/// are the `Table*` values below (T100): the CSS Tables 3 fix-up and the table container handle them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayValue {
    Block,
    Inline,
    None,
    /// `inline-block` (`inline flow-root`): an atomic inline-level box whose contents form a
    /// block formatting context (FR-002, FR-010; T024).
    InlineBlock,
    FlowRoot,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
    /// The element generates no box of its own; its children take its place (FR-005).
    Contents,
    /// `list-item` (block outside, flow inside): a block box plus a marker hook (FR-082, US10).
    ListItem,
    /// `table`: a block-level table wrapper box holding the table grid box and its captions (FR-076).
    Table,
    /// `inline-table`: the same wrapper as an atomic inline-level box (FR-010, FR-076).
    InlineTable,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    TableColumn,
    TableColumnGroup,
    TableCaption,
    /// Outside the consumed set: boxed as `inline` and reported (`css.display.<value>`).
    Unsupported(FeatureId),
}

/// The inner display type (formatting context) of a block-level or atomic inline-level box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InnerDisplay {
    Flow,
    FlowRoot,
    Flex,
    Grid,
}

impl DisplayValue {
    /// The inner display of a box-generating value; `None` for `none`, `contents`, `inline` and
    /// the unsupported values.
    pub fn inner(self) -> Option<InnerDisplay> {
        match self {
            DisplayValue::Block | DisplayValue::ListItem => Some(InnerDisplay::Flow),
            DisplayValue::FlowRoot | DisplayValue::InlineBlock => Some(InnerDisplay::FlowRoot),
            DisplayValue::Flex | DisplayValue::InlineFlex => Some(InnerDisplay::Flex),
            DisplayValue::Grid | DisplayValue::InlineGrid => Some(InnerDisplay::Grid),
            _ => None,
        }
    }

    /// Any of the CSS Tables values (`table`, `inline-table` and the internal ones).
    pub fn is_table_family(self) -> bool {
        matches!(
            self,
            DisplayValue::Table
                | DisplayValue::InlineTable
                | DisplayValue::TableRowGroup
                | DisplayValue::TableHeaderGroup
                | DisplayValue::TableFooterGroup
                | DisplayValue::TableRow
                | DisplayValue::TableCell
                | DisplayValue::TableColumn
                | DisplayValue::TableColumnGroup
                | DisplayValue::TableCaption
        )
    }

    /// `inline-block`, `inline-flex`, `inline-grid`: atomic inline-level (FR-010).
    pub fn is_atomic_inline(self) -> bool {
        matches!(
            self,
            DisplayValue::InlineBlock | DisplayValue::InlineFlex | DisplayValue::InlineGrid
        )
    }

    /// Block-level outer display with a supported inner display.
    pub fn is_block_level(self) -> bool {
        matches!(
            self,
            DisplayValue::Block
                | DisplayValue::ListItem
                | DisplayValue::FlowRoot
                | DisplayValue::Flex
                | DisplayValue::Grid
        )
    }

    /// The `css.display.<keyword>` suffix of a supported value (006 css-feature-profile §3).
    pub fn keyword(self) -> &'static str {
        match self {
            DisplayValue::Block => "block",
            DisplayValue::Inline => "inline",
            DisplayValue::None => "none",
            DisplayValue::InlineBlock => "inline-block",
            DisplayValue::FlowRoot => "flow-root",
            DisplayValue::Flex => "flex",
            DisplayValue::InlineFlex => "inline-flex",
            DisplayValue::Grid => "grid",
            DisplayValue::InlineGrid => "inline-grid",
            DisplayValue::Contents => "contents",
            DisplayValue::ListItem => "list-item",
            DisplayValue::Table => "table",
            DisplayValue::InlineTable => "inline-table",
            DisplayValue::TableRowGroup => "table-row-group",
            DisplayValue::TableHeaderGroup => "table-header-group",
            DisplayValue::TableFooterGroup => "table-footer-group",
            DisplayValue::TableRow => "table-row",
            DisplayValue::TableCell => "table-cell",
            DisplayValue::TableColumn => "table-column",
            DisplayValue::TableColumnGroup => "table-column-group",
            DisplayValue::TableCaption => "table-caption",
            DisplayValue::Unsupported(_) => "unsupported",
        }
    }
}

/// `<length-percentage>`; percentages are fractions (0.5 = 50%).
#[derive(Clone, Debug, PartialEq)]
pub enum LengthPercentage {
    Px(f32),
    Percent(f32),
    Calc(Arc<CalcExpr>),
}

impl LengthPercentage {
    pub const ZERO: LengthPercentage = LengthPercentage::Px(0.0);

    /// Whether the computed value is zero (`0`, `0%`, or a calc that is zero for every basis).
    pub fn is_zero(&self) -> bool {
        match self {
            LengthPercentage::Px(v) | LengthPercentage::Percent(v) => *v == 0.0,
            LengthPercentage::Calc(e) => e.eval(0.0) == 0.0 && e.eval(1000.0) == 0.0,
        }
    }
}

/// `width`/`height`/`min-*`: `auto` or a length-percentage.
#[derive(Clone, Debug, PartialEq)]
pub enum SizeValue {
    Auto,
    Lp(LengthPercentage),
}

/// `max-*`: `none` or a length-percentage.
#[derive(Clone, Debug, PartialEq)]
pub enum MaxSizeValue {
    None,
    Lp(LengthPercentage),
}

/// `margin-*`: `auto` or a length-percentage.
#[derive(Clone, Debug, PartialEq)]
pub enum MarginValue {
    Auto,
    Lp(LengthPercentage),
}

impl MarginValue {
    pub fn is_zero(&self) -> bool {
        matches!(self, MarginValue::Lp(lp) if lp.is_zero())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}

/// Computed `text-align` (FR-017: left, right, center, start, end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlignValue {
    Left,
    Right,
    Center,
    Start,
    End,
    /// `justify` (CSS Text 3 section 7): word spaces of every line but the last stretch.
    Justify,
    /// Outside the consumed set: treated as `start` and reported (stage `layout`).
    Unsupported(FeatureId),
}

pub use crate::style::view::groups::UnicodeBidi;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
}

/// `white-space-collapse` (CSS Text 4; `preserve-spaces` is off in the profile).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollapseMode {
    /// Sequences of white space collapse; segment breaks are spaces.
    Collapse,
    /// Spaces, tabs and segment breaks are preserved (`pre`, `pre-wrap`).
    Preserve,
    /// Spaces collapse, segment breaks are preserved (`pre-line`).
    PreserveBreaks,
    /// As `Preserve`; a preserved space at the end of a line takes up room and may wrap
    /// (`break-spaces`).
    BreakSpaces,
}

/// Computed `white-space` as its two longhands: `white-space-collapse` and `text-wrap-mode`
/// (`white-space` is their shorthand; `text-wrap-style` is `stable`, the other values are off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WhiteSpaceValue {
    pub collapse: CollapseMode,
    /// `text-wrap-mode: wrap` (false: `nowrap`).
    pub wrap: bool,
}

impl WhiteSpaceValue {
    /// `normal`.
    pub const NORMAL: WhiteSpaceValue = WhiteSpaceValue {
        collapse: CollapseMode::Collapse,
        wrap: true,
    };

    pub fn preserves_newlines(self) -> bool {
        !matches!(self.collapse, CollapseMode::Collapse)
    }

    pub fn preserves_spaces(self) -> bool {
        matches!(
            self.collapse,
            CollapseMode::Preserve | CollapseMode::BreakSpaces
        )
    }
}

/// The other inherited text-flow properties of a box (CSS Text 3, CSS UI 4): spacing, tab stops,
/// wrapping and `text-overflow` (not inherited; it applies to the block container's line ends).
#[derive(Clone, Debug, PartialEq)]
pub struct TextFlow {
    pub tab_size: crate::style::view::groups::TabSize,
    pub text_transform: crate::style::view::groups::TextTransformValue,
    pub overflow_wrap: crate::style::view::groups::OverflowWrap,
    pub word_break: crate::style::view::groups::WordBreak,
    pub line_break: crate::style::view::groups::LineBreak,
    /// `hyphens` and `hyphenate-character` (015 TXT-3); `None` is `auto`.
    pub hyphens: crate::style::view::groups::Hyphens,
    pub hyphenate_character: Option<String>,
    /// `letter-spacing` in px.
    pub letter_spacing: f32,
    /// `word-spacing` in px (a percentage or calc is reported by the adapter and is 0 here).
    pub word_spacing: f32,
    pub text_overflow: crate::style::view::groups::TextOverflow,
    /// Variation settings, optical sizing and OpenType features (015 TXT-2).
    pub font_settings: crate::text::fonts::features::FontSettings,
    /// `text-wrap-style: balance` (015 TXT-4): applies to the block container, i.e. to the root of the paragraph.
    pub balance: bool,
}

impl TextFlow {
    pub fn initial() -> TextFlow {
        use crate::style::view::groups as g;
        TextFlow {
            tab_size: g::TabSize::Number(8.0),
            text_transform: g::TextTransformValue::default(),
            overflow_wrap: g::OverflowWrap::Normal,
            word_break: g::WordBreak::Normal,
            line_break: g::LineBreak::Auto,
            hyphens: g::Hyphens::Manual,
            hyphenate_character: None,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_overflow: g::TextOverflow::CLIP,
            font_settings: Default::default(),
            balance: false,
        }
    }
}

/// Computed `visibility` (FR-030): hidden and collapsed boxes keep their space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityValue {
    Visible,
    Hidden,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirectionValue {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexWrapValue {
    NoWrap,
    Wrap,
    WrapReverse,
}

/// `flex-basis`.
#[derive(Clone, Debug, PartialEq)]
pub enum FlexBasisValue {
    Auto,
    Content,
    Lp(LengthPercentage),
}

/// The flex item and container longhands (FR-037–FR-047) except alignment and gaps.
#[derive(Clone, Debug, PartialEq)]
pub struct FlexStyle {
    pub direction: FlexDirectionValue,
    pub wrap: FlexWrapValue,
    pub grow: f32,
    pub shrink: f32,
    pub basis: FlexBasisValue,
}

/// One track sizing breadth of a grid track (`minmax()` bounds).
#[derive(Clone, Debug, PartialEq)]
pub enum TrackBreadth {
    Auto,
    MinContent,
    MaxContent,
    Lp(LengthPercentage),
    Fr(f32),
    /// `fit-content(<length-percentage>)` (a maximum only).
    FitContent(LengthPercentage),
}

/// A track size: `minmax(min, max)`; a single value `x` is `minmax(x, x)` for lengths, percentages,
/// `min-content`, `max-content` and `auto`, `minmax(auto, x)` for `<flex>` and `fit-content()`.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackSize {
    pub min: TrackBreadth,
    pub max: TrackBreadth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatCount {
    Count(u16),
    AutoFill,
    AutoFit,
}

/// The line names of one grid line (several names may share a line).
pub type LineNames = Vec<String>;

/// An entry of a track list: one track or a `repeat()` of tracks with the line names of one
/// repetition (empty, or `tracks.len() + 1` sets).
#[derive(Clone, Debug, PartialEq)]
pub enum TrackListItem {
    Track(TrackSize),
    Repeat(RepeatCount, Vec<TrackSize>, Vec<LineNames>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAutoFlowValue {
    Row,
    Column,
    RowDense,
    ColumnDense,
}

/// One side of `grid-row` / `grid-column`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridLineValue {
    Auto,
    /// A 1-based line number, negative counting from the end.
    Line(i16),
    Span(u16),
    /// `<custom-ident>` [`<integer>`]: the nth line of that name (0 = no integer: an area edge or the
    /// first line of that name; Grid 2 section 8.3).
    NamedLine(String, i16),
    /// `span <custom-ident>` [`<integer>`].
    NamedSpan(String, u16),
}

/// A named area of `grid-template-areas`: 1-based lines, `*_end` is the line that ends the area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridAreaValue {
    pub name: String,
    pub row_start: u16,
    pub row_end: u16,
    pub column_start: u16,
    pub column_end: u16,
}

/// `grid-template-areas`: the areas and the template's size, which `.` cells can make larger than the
/// areas' extent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridAreasValue {
    pub areas: Vec<GridAreaValue>,
    pub rows: u16,
    pub columns: u16,
}

/// Grid container and item longhands (FR-048–FR-054) that layout consumes: tracks with their line
/// names, `grid-template-areas`, auto tracks and flow, and the item's line placement (numbers,
/// names, spans), and `subgrid` (015 GRID-1). `masonry` is not consumed (the profile turns it Off and
/// reports it).
#[derive(Clone, Debug, PartialEq)]
pub struct GridStyle {
    /// `grid-template-columns: subgrid` (CSS Grid 2 section 9): `columns` is empty and `column_names` holds the
    /// subgrid's `<line-name-list>`.
    pub column_subgrid: bool,
    /// `grid-template-rows: subgrid` (`row_names` holds the `<line-name-list>`).
    pub row_subgrid: bool,
    pub columns: Vec<TrackListItem>,
    /// One name set per line (`columns.len() + 1` sets), or empty when no line is named.
    pub column_names: Vec<LineNames>,
    pub rows: Vec<TrackListItem>,
    pub row_names: Vec<LineNames>,
    pub areas: Option<GridAreasValue>,
    pub auto_columns: Vec<TrackSize>,
    pub auto_rows: Vec<TrackSize>,
    pub auto_flow: GridAutoFlowValue,
    pub column: (GridLineValue, GridLineValue),
    pub row: (GridLineValue, GridLineValue),
}

impl GridStyle {
    pub fn initial() -> Self {
        GridStyle {
            column_subgrid: false,
            row_subgrid: false,
            columns: Vec::new(),
            column_names: Vec::new(),
            rows: Vec::new(),
            row_names: Vec::new(),
            areas: None,
            auto_columns: Vec::new(),
            auto_rows: Vec::new(),
            auto_flow: GridAutoFlowValue::Row,
            column: (GridLineValue::Auto, GridLineValue::Auto),
            row: (GridLineValue::Auto, GridLineValue::Auto),
        }
    }
}

/// Alignment keywords of `align-items`, `align-self`, `justify-*` and `align-content`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignKeyword {
    /// `normal` (also the `auto` of `align-self`/`justify-self`: defer to the container).
    Normal,
    Auto,
    Start,
    End,
    FlexStart,
    FlexEnd,
    SelfStart,
    SelfEnd,
    Center,
    Baseline,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// An alignment value with its overflow-alignment keyword (`safe` / `unsafe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlignValue {
    pub keyword: AlignKeyword,
    /// `Some(true)` = `safe`, `Some(false)` = `unsafe`, `None` = default.
    pub safe: Option<bool>,
}

impl AlignValue {
    pub const NORMAL: AlignValue = AlignValue {
        keyword: AlignKeyword::Normal,
        safe: None,
    };
    pub const AUTO: AlignValue = AlignValue {
        keyword: AlignKeyword::Auto,
        safe: None,
    };
}

/// `row-gap` / `column-gap`.
#[derive(Clone, Debug, PartialEq)]
pub enum GapValue {
    Normal,
    Lp(LengthPercentage),
}

/// Box alignment longhands consumed by flex (and later grid) layout.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignStyle {
    pub align_items: AlignValue,
    pub align_self: AlignValue,
    pub justify_content: AlignValue,
    pub align_content: AlignValue,
    pub justify_items: AlignValue,
    pub justify_self: AlignValue,
    pub row_gap: GapValue,
    pub column_gap: GapValue,
}

impl AlignStyle {
    pub fn initial() -> Self {
        AlignStyle {
            align_items: AlignValue::NORMAL,
            align_self: AlignValue::AUTO,
            justify_content: AlignValue::NORMAL,
            align_content: AlignValue::NORMAL,
            justify_items: AlignValue::NORMAL,
            justify_self: AlignValue::AUTO,
            row_gap: GapValue::Normal,
            column_gap: GapValue::Normal,
        }
    }
}

impl FlexStyle {
    pub fn initial() -> Self {
        FlexStyle {
            direction: FlexDirectionValue::Row,
            wrap: FlexWrapValue::NoWrap,
            grow: 0.0,
            shrink: 1.0,
            basis: FlexBasisValue::Auto,
        }
    }
}

/// Computed `position` (FR-055–FR-061).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PositionValue {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

impl PositionValue {
    /// `absolute` and `fixed`: removed from the flow (CSS Position 3 §3).
    pub fn is_out_of_flow(self) -> bool {
        matches!(self, PositionValue::Absolute | PositionValue::Fixed)
    }
    /// Anything but `static`: a positioned box (a containing block for `absolute` descendants).
    pub fn is_positioned(self) -> bool {
        self != PositionValue::Static
    }
}

/// The anchor-positioning facts of a box (CSS Anchor Positioning subset, F0808).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnchorStyle {
    pub props: crate::style::view::groups::AnchorProps,
    /// The implicit anchor of a popover: the element whose `popovertarget` invokes it.
    pub implicit: Option<NodeId>,
}

/// `top`/`right`/`bottom`/`left`: `auto` or a length-percentage.
#[derive(Clone, Debug, PartialEq)]
pub enum InsetValue {
    Auto,
    Lp(LengthPercentage),
}

impl InsetValue {
    pub fn is_auto(&self) -> bool {
        matches!(self, InsetValue::Auto)
    }
}

/// `container-type` of a box (CSS Conditional 5 section 3.1; 015 CQ-2/CQ-3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    #[default]
    Normal,
    Size,
    InlineSize,
}

/// The containment in effect on a box (CSS Containment 2): the computed `contain` united with what
/// `container-type` implies (`size`: size + layout + style; `inline-size`: inline-size + layout +
/// style). Style containment scopes counters and quotes (`counters.rs`); it has no layout effect.
/// Not inherited. Whether a type applies to the box
/// is decided where the box is known (`coordinator::containment`): size containment has no effect on
/// non-atomic inline boxes, tables and internal table boxes; layout and paint containment none on
/// non-atomic inline boxes and internal table boxes other than cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Containment {
    pub size_inline: bool,
    pub size_block: bool,
    pub layout: bool,
    pub paint: bool,
    /// Style containment: counters and quotes are scoped to the subtree (`counters.rs`).
    pub style: bool,
    pub container: ContainerKind,
    /// `content-visibility: hidden`: the contents are not boxed (0.9 F0903).
    pub skips_contents: bool,
    /// `contain-intrinsic-width` / `-height` (CSS px × 64) of a size-contained box.
    pub intrinsic: (Option<i32>, Option<i32>),
}

impl Containment {
    pub const NONE: Containment = Containment {
        size_inline: false,
        size_block: false,
        layout: false,
        paint: false,
        style: false,
        container: ContainerKind::Normal,
        skips_contents: false,
        intrinsic: (None, None),
    };
    /// Layout or paint containment: an independent formatting context, the containing block of
    /// `absolute` and `fixed` descendants and a stacking context.
    pub fn contains_layout_or_paint(&self) -> bool {
        self.layout || self.paint
    }
}

/// Computed `overflow-x` / `overflow-y` (FR-062). `visible` computes to `auto` when the other
/// axis is not `visible` or `clip`: that fix-up is the style engine's and is read as computed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OverflowValue {
    #[default]
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

impl OverflowValue {
    /// `hidden`, `scroll`, `auto`: a scroll container in that axis (CSS Overflow 3 §3).
    pub fn is_scroll_container(self) -> bool {
        matches!(
            self,
            OverflowValue::Hidden | OverflowValue::Scroll | OverflowValue::Auto
        )
    }
    /// Whether the value clips the padding box (everything but `visible`).
    pub fn clips(self) -> bool {
        self != OverflowValue::Visible
    }
}

/// Physical sides in CSS order.
#[derive(Clone, Debug, PartialEq)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Clone> Sides<T> {
    pub fn all(v: T) -> Self {
        Sides {
            top: v.clone(),
            right: v.clone(),
            bottom: v.clone(),
            left: v,
        }
    }
}

/// Computed `border-collapse` (FR-080).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderCollapseValue {
    #[default]
    Separate,
    Collapse,
}

/// Computed `table-layout` (FR-078).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableLayoutValue {
    #[default]
    Auto,
    Fixed,
}

/// Computed `caption-side` (FR-081): the logical keywords are mapped to their physical side for
/// a horizontal writing mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptionSideValue {
    #[default]
    Top,
    Bottom,
}

/// The table-related computed values of one element (CSS 2.1 §17; FR-076–FR-081). The first four
/// are inherited; the border styles and colours are read by the collapsed-border resolution
/// (the used widths stay in [`BoxStyle::border`]).
#[derive(Clone, Debug, PartialEq)]
pub struct TableStyle {
    pub border_collapse: BorderCollapseValue,
    /// `border-spacing` (horizontal, vertical) in px; used in the separated model only.
    pub border_spacing: (f32, f32),
    pub caption_side: CaptionSideValue,
    pub empty_cells_hide: bool,
    pub table_layout: TableLayoutValue,
    /// Computed `border-*-style` (physical sides).
    pub border_style: Sides<crate::layout::fragment::BorderLineStyle>,
    /// Whether the computed `border-*-color` is opaque (alpha 255 in sRGB RGBA8). The colour itself
    /// is paint-only and resolved by the scene from the style snapshot (L-PERF-b3): only its
    /// opacity moves collapsed-border geometry (`fragment/builder.rs`, the junction overlap).
    pub border_opaque: Sides<bool>,
}

impl TableStyle {
    /// Initial values.
    pub fn initial() -> Self {
        TableStyle {
            border_collapse: BorderCollapseValue::Separate,
            border_spacing: (0.0, 0.0),
            caption_side: CaptionSideValue::Top,
            empty_cells_hide: false,
            table_layout: TableLayoutValue::Auto,
            border_style: Sides::all(crate::layout::fragment::BorderLineStyle::None),
            border_opaque: Sides::all(true),
        }
    }

    /// Initial values of the non-inherited properties, the inherited ones taken from `parent`
    /// (anonymous table objects inherit from the box they are generated for, CSS 2.1 §17.2.1).
    pub fn inheriting(parent: &TableStyle) -> Self {
        TableStyle {
            border_collapse: parent.border_collapse,
            border_spacing: parent.border_spacing,
            caption_side: parent.caption_side,
            empty_cells_hide: parent.empty_cells_hide,
            ..TableStyle::initial()
        }
    }
}

/// The role of a generated or table-internal box (CSS Tables 3 / `box-tree.md` §3, data-model §3
/// `TableRole`). Spans are already clamped (`table::grid`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableTag {
    /// The table wrapper box: holds the captions and the table grid box. Carries the margins,
    /// positioning and float of the table element; generated, so it has no DOM identity.
    Wrapper,
    /// The table grid box: carries the table element's border, padding, size and background.
    Table,
    RowGroup(RowGroupKind),
    Row,
    Cell {
        col_span: u32,
        row_span: u32,
    },
    Caption,
    ColumnGroup {
        span: u32,
    },
    Column {
        span: u32,
    },
}

/// Computed `float` (`inline-start` / `inline-end` resolved against the box's `direction`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatValue {
    #[default]
    None,
    Left,
    Right,
}

/// Computed `clear` (`inline-start` / `inline-end` resolved against the box's `direction`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClearValue {
    #[default]
    None,
    Left,
    Right,
    Both,
}

/// An intrinsic-size keyword of `width` (CSS Sizing 3/4): `min-content`, `max-content`,
/// `fit-content`, `fit-content(<length-percentage>)`, `stretch` / `-webkit-fill-available`. The
/// box's `width` is `auto` and this value says how its inline size is resolved.
#[derive(Clone, Debug, PartialEq)]
pub enum WidthKeyword {
    MinContent,
    MaxContent,
    FitContent(Option<LengthPercentage>),
    Stretch,
}

/// The computed values of one element that box generation and layout consume (FR-017).
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStyle {
    pub width_kw: Option<WidthKeyword>,
    /// `min-width: min-content | max-content | fit-content` (CSS Sizing 3 §3): `min_width` is `auto` and the
    /// box's inline size is at least this keyword's size (`Stretch` and `fit-content(<lp>)` stay reported).
    pub min_width_kw: Option<WidthKeyword>,
    /// `height: min-content | max-content | fit-content` (015 HTML-1): `height` is `auto` and an absolutely
    /// positioned box is sized to its content (no stretch between its insets), as the UA `dialog` rule needs.
    /// In flow the keywords are `auto` already.
    pub height_kw: Option<WidthKeyword>,
    pub float: FloatValue,
    pub clear: ClearValue,
    pub display: DisplayValue,
    pub box_sizing: BoxSizing,
    pub width: SizeValue,
    pub height: SizeValue,
    pub min_width: SizeValue,
    pub min_height: SizeValue,
    pub max_width: MaxSizeValue,
    pub max_height: MaxSizeValue,
    pub margin: Sides<MarginValue>,
    pub padding: Sides<LengthPercentage>,
    /// Used border widths in px (0 when the computed style is `none`/`hidden`).
    pub border: Sides<f32>,
    /// Inherited text properties (shaping input).
    pub text: TextStyle,
    pub text_align: TextAlignValue,
    /// `text-align-last` (`None` is `auto`).
    pub text_align_last: Option<TextAlignValue>,
    /// `text-indent` (inherited): the first line of the block container starts this far in; a
    /// percentage is of the container's content width.
    pub text_indent: LengthPercentage,
    /// `text-align` is one of the legacy `-webkit-left | -webkit-right | -webkit-center` values (HTML `align`
    /// attribute, `center`): it also aligns the block-level children of the box (inherited like `text_align`).
    pub webkit_align: bool,
    pub direction: Direction,
    /// `unicode-bidi` (not inherited).
    pub unicode_bidi: UnicodeBidi,
    /// The element has `dir=auto` (HTML): its direction is that of its first strong character.
    pub dir_auto: bool,
    pub white_space: WhiteSpaceValue,
    /// Spacing, tab stops, wrapping and `text-overflow` (inherited, except `text_overflow`).
    pub flow: TextFlow,
    /// The legacy line clamp of this block container (`-webkit-line-clamp: N` on `display: -webkit-box` with
    /// `-webkit-box-orient: vertical`): the box is as high as its first `N` lines and shows an ellipsis at the
    /// end of the last of them (015 TXT-4).
    pub line_clamp: Option<u32>,
    /// Text longhands with a non-initial computed value that nothing consumes (`text-shadow`,
    /// `text-indent`, `text-align-last`, `font-variant-*`, ...), as feature ids. The inline
    /// builder reports them for every element whose own text it lays out (FR-022).
    pub unconsumed_text: Vec<FeatureId>,
    /// `::first-line` (015 TXT-5): the style of this element (a block container: its `::first-line` pseudo-element) on the
    /// first formatted line of an ancestor block, as the style bridge maps it from the first-line variant
    /// (`ui4me-style` `first_pseudo.rs`). Set on a block container, it is the style of its first line (the strut) and
    /// the inheritance base of what the line holds; set on an inline box or a block it is the box's style on that
    /// line. `decorations` of the variant are completed while the box tree is built.
    pub first_line: Option<Arc<BoxStyle>>,
    /// `::first-letter` of a block container (015 TXT-5), cascaded from its first-line style when it has one.
    pub first_letter: Option<Arc<BoxStyle>>,
    /// The box's own text decoration (CSS Text Decoration 3), when it has a line. Not inherited.
    pub own_decoration: Option<crate::text::paragraph::DecorationLayer>,
    /// The decorations in effect over this box's text: those of the decorating ancestors that
    /// propagate to it (in-flow, not through floats, out-of-flow or atomic inline-level boxes),
    /// outermost first, then the box's own. Set while the box tree is built.
    pub decorations: Arc<[crate::text::paragraph::DecorationLayer]>,
    /// `vertical-align` of an atomic inline (CSS 2.1 §10.8.1). Not inherited.
    pub vertical_align: crate::text::paragraph::VerticalAlignSpec,
    /// Computed `aspect-ratio` as width / height (`auto` and the `auto w / h` hint of `img`
    /// attributes give the ratio when it is a number). Not inherited.
    pub aspect_ratio: Option<f32>,
    /// FR-022: layout-stage properties whose computed value differs from the initial value and
    /// that the slice does not consume (for example `position`, `top`), as feature ids
    /// (`css.property.<longhand>`). Layout treats each as its initial value and reports it.
    pub unconsumed_layout: Vec<FeatureId>,
    pub visibility: VisibilityValue,
    /// CSS `order` (flex and grid items).
    pub order: i32,
    pub flex: FlexStyle,
    pub align: AlignStyle,
    pub grid: GridStyle,
    /// `position` (FR-055–FR-061).
    pub position: PositionValue,
    /// `top`/`right`/`bottom`/`left` (physical).
    pub inset: Sides<InsetValue>,
    /// `z-index` (`None` = `auto`); a fact for paint's stacking, never used by layout.
    pub z_index: Option<i32>,
    /// The box of an element in the top layer, or of its `::backdrop` (015 HTML-1): its paint order
    /// above everything else (`2 * index` for the backdrop, `2 * index + 1` for the element, `index`
    /// the element's position in the top layer). Set by box generation, never by the style mapping.
    pub top_layer: Option<u32>,
    /// `anchor-name`, `position-anchor` and `anchor()` insets (F0808); `None` for the vast majority of boxes.
    pub anchor: Option<Box<AnchorStyle>>,
    /// A `fieldset` with a rendered legend (015 HTML-3): its block-start border width and padding (px; a
    /// percentage padding counts 0), which layout does not see (the box's `border.top` and `padding.top` are
    /// 0); the fragment reports them (box model queries) and paint draws the border centred on the legend.
    pub fieldset_border_top: Option<(f32, f32)>,
    /// Original margin retained when layout ignores a rendered legend’s block-start margin.
    pub legend_report_margin_top: Option<MarginValue>,
    pub overflow_x: OverflowValue,
    pub overflow_y: OverflowValue,
    /// `transform`, `perspective`, `filter`, an individual transform property or a `will-change`
    /// naming one of them is computed on the box: it is then the containing block for `absolute`
    /// **and** `fixed` descendants (FR-056, FR-059; CSS Transforms 1, Filter Effects 1).
    pub claims_fixed_descendants: bool,
    /// `contain` and the containment `container-type` implies (CSS Containment 2; 015 CQ-2).
    pub containment: Containment,
    /// Table properties (FR-076–FR-081).
    pub table: TableStyle,
    /// Whether a control keeps its native appearance. For `select`, this also requires no author
    /// border or background and decides the dropdown's 1px inner block padding (L-GEO-r9).
    /// For `input`, this records whether `appearance` is not `none`.
    pub native_control: bool,
    /// `field-sizing: content` (0.9 F0903): a text control is sized to its value.
    pub field_sizing_content: bool,
    /// The box's `display` before `position: absolute | fixed` or `float` blockified it was
    /// inline-level: an out-of-flow box of this kind sits at its inline position in the line (its
    /// static position, CSS 2.1 section 10.3.7 and 10.6.4: the hypothetical inline box).
    pub original_inline: bool,
    /// Generated content and lists (US10): `content`, `counter-reset`, `counter-increment`,
    /// `quotes` and `list-style-*`.
    pub generated: Arc<crate::style::view::generated::Generated>,
    /// This box is an outside `::marker`: an atomic inline whose margin box is zero wide and whose
    /// border box ends at the start edge of the line (CSS Lists 3 section 3.1; `atomic.rs`).
    pub marker_outside: bool,
    /// The box has a background (a colour that is not transparent, or an image): an inline box
    /// with one gets line pieces for the scene to paint, like one with padding or a border.
    /// Always `false` on boxes that are not inline boxes in a built tree (`build_box_tree`).
    pub has_background: bool,
    /// A piece of an inline box split around a block-level descendant (CSS 2.1 section 9.2.1.1):
    /// the piece that follows a block has no start edge, the one before it has no end edge.
    pub split_start: bool,
    pub split_end: bool,
    /// Multicolumn and fragmentation values (CSS Multicol 1, CSS Break 3; 015 FRAG-2a). Not inherited.
    pub frag: FragStyle,
    /// `writing-mode` (inherited; 015 WM-1). In a vertical mode the physical values of this style (sizes,
    /// the sides of margin, padding, border and insets, `overflow-x`/`-y`) are stored in the box's logical
    /// frame (`crate::layout::writing_mode`): `width` is the inline size, `margin.top` the block-start margin,
    /// `margin.left` the line-left one.
    pub writing_mode: crate::layout::writing_mode::WritingMode,
    /// `text-orientation` (inherited; 015 WM-1b).
    pub text_orientation: crate::layout::writing_mode::TextOrientation,
    /// `text-combine-upright: all` (inherited; 015 WM-1b).
    pub text_combine_upright: bool,
}

pub use crate::style::view::layout_groups::{BreakBetween, BreakWithin};

/// The multicolumn and fragmentation values of a box (015 FRAG-2a). They take effect only inside a
/// fragmentation context (`crate::layout::fragmentation`); `column-gap` is `AlignStyle::column_gap`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FragStyle {
    /// `column-count` (`None` = `auto`).
    pub column_count: Option<u32>,
    /// `column-width` in px (`None` = `auto`).
    pub column_width: Option<f32>,
    /// `column-fill: auto` (`false` = `balance`).
    pub column_fill_auto: bool,
    /// `column-span: all`.
    pub column_span_all: bool,
    /// The used `column-rule-width` in px (0 for `none` / `hidden`); the colour is paint's.
    pub column_rule_width: f32,
    pub break_before: BreakBetween,
    pub break_after: BreakBetween,
    pub break_inside: BreakWithin,
    pub orphans: u32,
    pub widows: u32,
    /// `box-decoration-break: clone` (`slice` otherwise).
    pub decoration_clone: bool,
    /// This box is one piece of an element split around a nested `column-span: all` descendant (015 MC-2,
    /// `crate::layout::fragmentation::multicol::lift_spanners`): not a computed value; set on the piece's own copy of
    /// the element's style, whose margin, border and padding on the split sides are zero.
    pub split: Option<SplitPiece>,
    /// `page` (CSS Paged Media 3 section 6; 015 PAGE-1): the interned name (`crate::layout::fragmentation::paged::intern`),
    /// `None` = `auto`.
    pub page: Option<u32>,
}

/// One piece of an element split around a nested spanner (015 MC-2): its index among the element's
/// pieces and whether it is the last.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitPiece {
    pub index: u16,
    pub last: bool,
    /// The element's block-end border and padding (px; percentages count as 0): a piece between two spanners
    /// keeps that much room in its row (measured in the reference), though it is not drawn there.
    pub end_pb: f32,
}

impl FragStyle {
    pub const INITIAL: FragStyle = FragStyle {
        column_count: None,
        column_width: None,
        column_fill_auto: false,
        column_span_all: false,
        column_rule_width: 0.0,
        break_before: BreakBetween::Auto,
        break_after: BreakBetween::Auto,
        break_inside: BreakWithin::Auto,
        orphans: 2,
        widows: 2,
        decoration_clone: false,
        split: None,
        page: None,
    };

    /// A multicol container: `column-count` or `column-width` is not `auto` (CSS Multicol 1 section 2).
    pub fn is_multicol(&self) -> bool {
        self.column_count.is_some() || self.column_width.is_some()
    }
}

impl Default for FragStyle {
    fn default() -> Self {
        FragStyle::INITIAL
    }
}

impl BoxStyle {
    /// Initial values of the non-inherited properties, with `text`, `text_align`, `direction`
    /// and `white_space` inherited from `parent` (CSS 2.1 §9.2.1.1 for anonymous boxes).
    pub fn inheriting(parent: &BoxStyle, display: DisplayValue) -> Self {
        BoxStyle {
            display,
            box_sizing: BoxSizing::ContentBox,
            width_kw: None,
            min_width_kw: None,
            height_kw: None,
            float: FloatValue::None,
            clear: ClearValue::None,
            width: SizeValue::Auto,
            height: SizeValue::Auto,
            min_width: SizeValue::Auto,
            min_height: SizeValue::Auto,
            max_width: MaxSizeValue::None,
            max_height: MaxSizeValue::None,
            margin: Sides::all(MarginValue::Lp(LengthPercentage::ZERO)),
            padding: Sides::all(LengthPercentage::ZERO),
            border: Sides::all(0.0),
            text: parent.text.clone(),
            text_align: parent.text_align,
            text_align_last: parent.text_align_last,
            text_indent: parent.text_indent.clone(),
            webkit_align: parent.webkit_align,
            direction: parent.direction,
            unicode_bidi: UnicodeBidi::Normal,
            dir_auto: false,
            white_space: parent.white_space,
            flow: TextFlow {
                text_overflow: crate::style::view::groups::TextOverflow::CLIP,
                ..parent.flow.clone()
            },
            line_clamp: None,
            unconsumed_text: Vec::new(),
            first_line: None,
            first_letter: None,
            own_decoration: None,
            decorations: parent.decorations.clone(),
            vertical_align: Default::default(),
            aspect_ratio: None,
            unconsumed_layout: Vec::new(),
            visibility: parent.visibility,
            order: 0,
            flex: FlexStyle::initial(),
            align: AlignStyle::initial(),
            grid: GridStyle::initial(),
            position: PositionValue::Static,
            inset: Sides::all(InsetValue::Auto),
            z_index: None,
            top_layer: None,
            anchor: None,
            fieldset_border_top: None,
            legend_report_margin_top: None,
            overflow_x: OverflowValue::Visible,
            overflow_y: OverflowValue::Visible,
            claims_fixed_descendants: false,
            containment: Containment::NONE,
            table: TableStyle::inheriting(&parent.table),
            native_control: false,
            field_sizing_content: false,
            original_inline: false,
            generated: Arc::new(crate::style::view::generated::Generated {
                quotes: parent.generated.quotes.clone(),
                list: parent.generated.list.clone(),
                ..crate::style::view::generated::Generated::initial()
            }),
            marker_outside: false,
            has_background: false,
            split_start: false,
            split_end: false,
            // `orphans` and `widows` are inherited (CSS Break 3 section 4.4): an anonymous block (the lines
            // of a mixed inline run, a flow thread) takes them from its parent.
            frag: FragStyle {
                orphans: parent.frag.orphans,
                widows: parent.frag.widows,
                ..FragStyle::INITIAL
            },
            writing_mode: parent.writing_mode,
            text_orientation: parent.text_orientation,
            text_combine_upright: parent.text_combine_upright,
        }
    }

    /// Initial values everywhere, with the given root text style.
    pub fn initial(display: DisplayValue, text: TextStyle) -> Self {
        let seed = BoxStyle {
            display,
            box_sizing: BoxSizing::ContentBox,
            width_kw: None,
            min_width_kw: None,
            height_kw: None,
            float: FloatValue::None,
            clear: ClearValue::None,
            width: SizeValue::Auto,
            height: SizeValue::Auto,
            min_width: SizeValue::Auto,
            min_height: SizeValue::Auto,
            max_width: MaxSizeValue::None,
            max_height: MaxSizeValue::None,
            margin: Sides::all(MarginValue::Lp(LengthPercentage::ZERO)),
            padding: Sides::all(LengthPercentage::ZERO),
            border: Sides::all(0.0),
            text,
            text_align: TextAlignValue::Start,
            text_align_last: None,
            text_indent: LengthPercentage::ZERO,
            webkit_align: false,
            direction: Direction::Ltr,
            unicode_bidi: UnicodeBidi::Normal,
            dir_auto: false,
            white_space: WhiteSpaceValue::NORMAL,
            flow: TextFlow::initial(),
            line_clamp: None,
            unconsumed_text: Vec::new(),
            first_line: None,
            first_letter: None,
            own_decoration: None,
            decorations: Arc::from(Vec::new()),
            vertical_align: Default::default(),
            aspect_ratio: None,
            unconsumed_layout: Vec::new(),
            visibility: VisibilityValue::Visible,
            order: 0,
            flex: FlexStyle::initial(),
            align: AlignStyle::initial(),
            grid: GridStyle::initial(),
            position: PositionValue::Static,
            inset: Sides::all(InsetValue::Auto),
            z_index: None,
            top_layer: None,
            anchor: None,
            fieldset_border_top: None,
            legend_report_margin_top: None,
            overflow_x: OverflowValue::Visible,
            overflow_y: OverflowValue::Visible,
            claims_fixed_descendants: false,
            containment: Containment::NONE,
            table: TableStyle::initial(),
            native_control: false,
            field_sizing_content: false,
            original_inline: false,
            generated: Arc::new(crate::style::view::generated::Generated::initial()),
            marker_outside: false,
            has_background: false,
            split_start: false,
            split_end: false,
            frag: FragStyle::INITIAL,
            writing_mode: Default::default(),
            text_orientation: Default::default(),
            text_combine_upright: false,
        };
        BoxStyle::inheriting(&seed, display)
    }
}

/// What box generation sees of one DOM node.
#[derive(Clone, Copy, Debug)]
pub enum SourceNode<'a> {
    /// An element with its computed values.
    Element(&'a BoxStyle),
    Text(&'a str),
    /// Comments, processing instructions, unstyled nodes: no boxes.
    Ignored,
}

/// The structure-frozen, styled document as box generation reads it (008 `box-tree.md` §1).
pub trait StyledSource {
    fn document(&self) -> DocumentId;
    /// The document element, if any.
    fn root_element(&self) -> Option<NodeId>;
    /// DOM children in tree order.
    fn children(&self, node: NodeId) -> Vec<NodeId>;
    fn node(&self, node: NodeId) -> SourceNode<'_>;
    /// The HTML local name of an element in the HTML namespace (for the `display: contents`
    /// exceptions of CSS Display 3 Appendix B and the HTML rendering rules); `None` when the
    /// source does not know it, in which case no element is treated specially.
    fn tag_name(&self, _node: NodeId) -> Option<&str> {
        None
    }
    /// The HTML identity of an element (name and attributes) for the HTML rendering rules, replaced
    /// elements and form controls (FR-013–FR-016, FR-070–FR-075). `None` for sources that carry no
    /// HTML identity (the plain `InputTree`): the element is then an ordinary box.
    fn html(&self, _node: NodeId) -> Option<HtmlInfo> {
        None
    }
    /// 007's natural-size view of the `img` element `node` (FR-070). `None` = the source has no
    /// resource information: the image is laid out as pending.
    fn image_state(&self, _node: NodeId) -> Option<ui4me_assets::view::ImageState> {
        None
    }
    /// 007's natural-size view of a CSS image of `node` under `property` (`list-style-image` of a list
    /// item, `content::before` / `content::after` of an element): `None` before the scene stage first
    /// requested it (laid out as pending; its completion batch relays out).
    fn css_image_state(
        &self,
        _node: NodeId,
        _property: &str,
    ) -> Option<ui4me_assets::view::ImageState> {
        None
    }
    /// The value of an attribute of an element (`colspan`, `rowspan`, `span` of the table family).
    /// `None` when the source carries no attributes.
    fn attr(&self, _node: NodeId, _name: &str) -> Option<String> {
        None
    }
    /// The computed values of a generated pseudo-element of an element (`::before`, `::after`,
    /// `::marker`); `None` when the source has none (US10, FR-012, FR-082).
    fn pseudo(&self, _node: NodeId, _kind: PseudoKind) -> Option<&BoxStyle> {
        None
    }
    /// The DOM parent of a node (the incremental box build walks up from the changed nodes); `None`
    /// when the source does not know it.
    fn parent(&self, _node: NodeId) -> Option<NodeId> {
        None
    }
    /// The element and pseudo-element styles as shared maps whose `Arc`s stay the same for unchanged
    /// styles (015 PERF-1). `None`: the source does not support the incremental box build (every pass
    /// is a full build).
    fn style_maps(&self) -> Option<incremental::StyleMaps> {
        None
    }
    /// The DOM revision the source shows (stamped on the box tree for the next incremental build).
    fn dom_revision(&self) -> Option<u64> {
        None
    }
    /// The document's top layer in order (015 HTML-1): these elements are boxed as the last children
    /// of the root, each after its `::backdrop`, instead of in their DOM place.
    fn top_layer(&self) -> Vec<NodeId> {
        Vec::new()
    }
}

/// Elements on which `display: contents` behaves as `display: none` (CSS Display 3 Appendix B,
/// FR-005).
pub fn contents_behaves_as_none(tag: &str) -> bool {
    matches!(
        tag,
        "br" | "wbr"
            | "img"
            | "input"
            | "select"
            | "textarea"
            | "meter"
            | "progress"
            | "canvas"
            | "embed"
            | "object"
            | "audio"
            | "video"
            | "iframe"
            | "svg"
    )
}

/// An HTML element's lowercase local name and its attributes (names lowercase), as the HTML
/// rendering rules read them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HtmlInfo {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    /// The current value of a text control from 003's control state (`input`, `textarea`), which
    /// the `value` attribute and a `textarea`'s children only default (012 `editing.md`: the
    /// inner editing area shows the value, not the attribute). `None` = no control state.
    pub control_value: Option<String>,
    /// A `select`'s rows, selection and popup state from 003's control state (lane L-CTRL-r10);
    /// `None` for sources that carry no control state.
    pub select: Option<SelectFacts>,
}

/// What layout needs of a `select` (012 `controls.md` sections 4-5): its rows (options, with
/// `optgroup` labels as rows of their own), whether the popup is open, the highlighted option and
/// the first visible row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectFacts {
    pub rows: Vec<SelectRowFact>,
    pub open: bool,
    /// Index (in the list of options) of the highlighted option of the open popup.
    pub highlight: Option<u32>,
    /// Index of the first visible row (popup or listbox).
    pub scroll: u32,
    pub multiple: bool,
    pub display_size: u32,
}

/// One row of a `select`: an option or an `optgroup` label.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectRowFact {
    pub label: String,
    /// An `optgroup` label row (not selectable).
    pub group: bool,
    /// An option inside an `optgroup` (indented).
    pub indent: bool,
    pub selected: bool,
    pub disabled: bool,
    /// Index in the select's list of options (`None` for a group label).
    pub option_index: Option<u32>,
}

impl HtmlInfo {
    pub fn new(name: &str, attrs: &[(&str, &str)]) -> Self {
        HtmlInfo {
            name: name.to_string(),
            attrs: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            control_value: None,
            select: None,
        }
    }
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn has(&self, name: &str) -> bool {
        self.attrs.iter().any(|(k, _)| k == name)
    }
}

#[derive(Clone, Debug)]
enum InputNode {
    Element(Box<BoxStyle>),
    Text(String),
}

/// Plain arena implementation of `StyledSource` (tests and pre-computed inputs). Node ids are
/// allocated by the caller's `Slots` so they are real `NodeId`s of `document`.
#[derive(Clone, Debug)]
pub struct InputTree {
    document: DocumentId,
    root: Option<NodeId>,
    nodes: Vec<(NodeId, InputNode, Vec<NodeId>)>,
    tags: Vec<(NodeId, String)>,
    html: std::collections::HashMap<NodeId, HtmlInfo>,
    images: std::collections::HashMap<NodeId, ui4me_assets::view::ImageState>,
}

impl InputTree {
    pub fn new(document: DocumentId) -> Self {
        InputTree {
            document,
            root: None,
            nodes: Vec::new(),
            tags: Vec::new(),
            html: Default::default(),
            images: Default::default(),
        }
    }

    fn find(&self, id: NodeId) -> Option<usize> {
        self.nodes.iter().position(|(n, _, _)| *n == id)
    }

    fn add(&mut self, id: NodeId, node: InputNode, parent: Option<NodeId>) {
        self.nodes.push((id, node, Vec::new()));
        match parent {
            Some(p) => {
                let i = self.find(p).expect("parent was added first");
                self.nodes[i].2.push(id);
            }
            None => self.root = Some(id),
        }
    }

    /// Add an element; `parent = None` makes it the document element.
    pub fn element(&mut self, id: NodeId, parent: Option<NodeId>, style: BoxStyle) {
        self.add(id, InputNode::Element(Box::new(style)), parent);
    }

    /// Give an element its HTML local name (needed only for the tag-specific rules).
    pub fn set_tag(&mut self, id: NodeId, tag: &str) {
        self.tags.retain(|(n, _)| *n != id);
        self.tags.push((id, tag.to_string()));
    }

    pub fn text(&mut self, id: NodeId, parent: NodeId, text: &str) {
        self.add(id, InputNode::Text(text.to_string()), Some(parent));
    }

    /// Give an element its HTML identity (tests of the HTML rendering rules and replaced boxes).
    pub fn set_html(&mut self, id: NodeId, name: &str, attrs: &[(&str, &str)]) {
        self.html.insert(id, HtmlInfo::new(name, attrs));
    }

    /// Give an `img` element 007's natural-size view.
    pub fn set_image_state(&mut self, id: NodeId, state: ui4me_assets::view::ImageState) {
        self.images.insert(id, state);
    }

    pub fn style_mut(&mut self, id: NodeId) -> Option<&mut BoxStyle> {
        let i = self.find(id)?;
        match &mut self.nodes[i].1 {
            InputNode::Element(s) => Some(s),
            InputNode::Text(_) => None,
        }
    }

    pub fn text_mut(&mut self, id: NodeId) -> Option<&mut String> {
        let i = self.find(id)?;
        match &mut self.nodes[i].1 {
            InputNode::Text(t) => Some(t),
            InputNode::Element(_) => None,
        }
    }
}

impl StyledSource for InputTree {
    fn document(&self) -> DocumentId {
        self.document
    }
    fn root_element(&self) -> Option<NodeId> {
        self.root
    }
    fn children(&self, node: NodeId) -> Vec<NodeId> {
        self.find(node)
            .map(|i| self.nodes[i].2.clone())
            .unwrap_or_default()
    }
    fn node(&self, node: NodeId) -> SourceNode<'_> {
        match self.find(node).map(|i| &self.nodes[i].1) {
            Some(InputNode::Element(s)) => SourceNode::Element(s),
            Some(InputNode::Text(t)) => SourceNode::Text(t),
            None => SourceNode::Ignored,
        }
    }
    fn tag_name(&self, node: NodeId) -> Option<&str> {
        // An explicit tag, else the local name given with the HTML identity.
        self.tags
            .iter()
            .find(|(n, _)| *n == node)
            .map(|(_, t)| t.as_str())
            .or_else(|| self.html.get(&node).map(|h| h.name.as_str()))
    }
    fn html(&self, node: NodeId) -> Option<HtmlInfo> {
        self.html.get(&node).cloned()
    }
    fn attr(&self, node: NodeId, name: &str) -> Option<String> {
        self.html
            .get(&node)
            .and_then(|h| h.attr(name))
            .map(str::to_string)
    }
    fn image_state(&self, node: NodeId) -> Option<ui4me_assets::view::ImageState> {
        self.images.get(&node).copied()
    }
}
pub mod counters;
pub mod first_letter;
pub mod html_rendering;
pub mod incremental;
pub mod lists;
pub mod pseudo;
pub mod table_fixup;

// ---------------------------------------------------------------------------------------------
// 008 T011: box-tree contract types (`contracts/box-tree.md` §2–§4, data-model §2–§3).
// Additive to the slice: the slice's `BoxKind`/`BoxOwner` keep working; `BoxKind::role()` and
// `BoxOwner::key()` project them onto the contract vocabulary. Nothing here carries a DOM handle
// other than the owner `NodeId` kept for diagnostics (AD-01, invariant 1).
// ---------------------------------------------------------------------------------------------

/// Pseudo-element owner kinds. `::first-letter` is a box around DOM text (015 TXT-5); `FirstLine` marks the text and
/// inline boxes of an element that sit on the first formatted line of a block with `::first-line` (their style is
/// the element's first-line variant), `FirstLineBefore` / `FirstLineAfter` the generated content there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoKind {
    Before,
    After,
    Marker,
    FirstLetter,
    FirstLine,
    FirstLineBefore,
    FirstLineAfter,
    /// `::backdrop` of an element in the top layer (015 HTML-1).
    Backdrop,
}

impl PseudoKind {
    /// Generated content: text that is not in the DOM (the first letter and the first line style DOM text).
    pub fn is_generated(self) -> bool {
        !matches!(self, PseudoKind::FirstLetter | PseudoKind::FirstLine)
    }
}

/// Contract owner of a box (§2): element, pseudo-element of an element, or anonymous in the
/// context of the containing element. Diagnostics-only identity (invariant 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OwnerKey {
    Element(NodeId),
    Pseudo(NodeId, PseudoKind),
    Anonymous(NodeId),
}

impl OwnerKey {
    /// Whether the box is generated (carries no DOM identity of its own).
    pub fn is_generated(self) -> bool {
        !matches!(self, OwnerKey::Element(_))
    }
    pub fn node(self) -> NodeId {
        match self {
            OwnerKey::Element(n) | OwnerKey::Pseudo(n, _) | OwnerKey::Anonymous(n) => n,
        }
    }
}

impl From<BoxOwner> for OwnerKey {
    fn from(o: BoxOwner) -> Self {
        match o {
            BoxOwner::Element(n) => OwnerKey::Element(n),
            BoxOwner::Anonymous(n) => OwnerKey::Anonymous(n),
            BoxOwner::Pseudo(n, k) => OwnerKey::Pseudo(n, k),
        }
    }
}

impl BoxOwner {
    /// The contract owner key of this slice owner.
    pub fn key(self) -> OwnerKey {
        self.into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowGroupKind {
    Header,
    Body,
    Footer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkerPlacement {
    Inside,
    Outside,
}

/// Inner box of an atomic inline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AtomicInner {
    Block,
    Flex,
    Grid,
    Table,
    Replaced,
    Control,
}

/// Every box kind of `box-tree.md` §3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoxRole {
    BlockContainer,
    InlineBox,
    AtomicInline(AtomicInner),
    FlexContainer,
    GridContainer,
    TableWrapper,
    Table,
    TableRowGroup(RowGroupKind),
    TableRow,
    TableCell,
    TableColumnGroup,
    TableColumn,
    TableCaption,
    Replaced,
    Control,
    ListMarker(MarkerPlacement),
    ButtonContent,
    RenderedLegend,
    TextRun,
}

impl BoxKind {
    /// The contract role of a box kind (an anonymous `ButtonContent` / `RenderedLegend` box is
    /// told apart by `LayoutBox::role`).
    pub fn role(&self) -> BoxRole {
        match self {
            BoxKind::BlockContainer { .. } => BoxRole::BlockContainer,
            BoxKind::InlineBox => BoxRole::InlineBox,
            BoxKind::TextRun(_) => BoxRole::TextRun,
            BoxKind::InlineBlock { .. } => BoxRole::AtomicInline(AtomicInner::Block),
            BoxKind::Replaced(r) => match (&r.kind, r.block_level) {
                (crate::layout::replaced::ReplacedKind::Control(_), false) => {
                    BoxRole::AtomicInline(AtomicInner::Control)
                }
                (crate::layout::replaced::ReplacedKind::Control(_), true) => BoxRole::Control,
                (crate::layout::replaced::ReplacedKind::MarkerSymbol { .. }, _) => {
                    BoxRole::ListMarker(MarkerPlacement::Inside)
                }
                (_, false) => BoxRole::AtomicInline(AtomicInner::Replaced),
                (_, true) => BoxRole::Replaced,
            },
        }
    }
}

/// Formatting context a box establishes for its contents (§2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormattingContext {
    Block,
    Inline,
    Flex,
    Grid,
    Table,
    None,
}

/// Box flags (§2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoxFlags {
    pub is_replaced: bool,
    pub is_atomic_inline: bool,
    pub is_float: bool,
    pub is_out_of_flow: bool,
    pub establishes_bfc: bool,
    pub is_inline_root: bool,
    pub is_scroll_container: bool,
}

/// Replaced-content descriptor shell (data-model §3); filled by the replaced-element tasks.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplacedDescriptor {
    pub natural_size: Option<(f32, f32)>,
    pub aspect_ratio: Option<f32>,
    /// Resource still pending (FR-072): sized by the fallback rules, `RESOURCE_PENDING` attached.
    pub pending: bool,
}

/// Table role shell (data-model §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRole {
    pub colspan: u32,
    pub rowspan: u32,
}

/// Generated-content shell (data-model §3): text of `::before`/`::after`/marker content.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedContent {
    pub text: String,
}

/// A generation-stamped reference to a box. Valid only for the box-tree revision that produced
/// it (invariant 4); any other revision resolves to `StaleHandle` (internal `STALE_HANDLE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoxHandle {
    pub slot: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaleHandle;

/// Read-only view of a box tree for the inline behaviour (009) and diagnostics.
pub struct BoxTreeView<'a> {
    tree: &'a BoxTree,
}

impl<'a> BoxTreeView<'a> {
    pub fn new(tree: &'a BoxTree) -> Self {
        BoxTreeView { tree }
    }
    pub fn root(&self) -> Option<BoxId> {
        self.tree.root
    }
    pub fn get(&self, id: BoxId) -> &'a LayoutBox {
        self.tree.get(id)
    }
    pub fn children(&self, id: BoxId) -> &'a [BoxId] {
        &self.tree.get(id).children
    }
    pub fn role(&self, id: BoxId) -> BoxRole {
        self.tree.get(id).role()
    }
    pub fn owner_key(&self, id: BoxId) -> OwnerKey {
        self.tree.get(id).owner.key()
    }
    /// Boxes of a DOM node in content order (invariant 2: possibly none).
    pub fn boxes_of(&self, node: NodeId) -> &'a [BoxId] {
        self.tree.boxes_of(node)
    }
    pub fn handle(&self, id: BoxId) -> BoxHandle {
        BoxHandle {
            slot: id.0,
            generation: self.tree.generation,
        }
    }
    pub fn resolve(&self, h: BoxHandle) -> Result<&'a LayoutBox, StaleHandle> {
        if h.generation != self.tree.generation || (h.slot as usize) >= self.tree.boxes.len() {
            return Err(StaleHandle);
        }
        Ok(&self.tree.boxes[h.slot as usize])
    }
}
