//! owner: 008-layout-engine
//!
//! The style view the host tree hands to Taffy's algorithms (research R-07): a [`taffy::Style`]
//! plus the one policy Taffy lets a host override, [`CoreStyle::is_containing_block`]. Taffy's
//! default claims `absolute` descendants for positioned boxes and never `fixed` ones; FR-056 and
//! FR-059 add the boxes with a `transform`, `perspective`, `filter` or matching `will-change`
//! (both `absolute` and `fixed`). Every other method delegates to the wrapped `Style`.

use taffy::prelude::*;
use taffy::{
    AlignContent, AlignItems, AlignSelf, BlockContainerStyle, BlockItemStyle, BoxGenerationMode,
    Clear, Contain, ContainingBlockClaims, CoreStyle, Direction, FlexboxContainerStyle,
    FlexboxItemStyle, Float, GridContainerStyle, GridItemStyle, JustifyContent, OofItemStyle,
    Overflow, Point,
};

#[derive(Clone, Copy)]
pub(crate) struct HostStyle<'a> {
    pub style: &'a Style,
    pub claims: ContainingBlockClaims,
}

impl<'a> HostStyle<'a> {
    pub(crate) fn new(style: &'a Style, claims: ContainingBlockClaims) -> Self {
        HostStyle { style, claims }
    }
}

impl CoreStyle for HostStyle<'_> {
    type CustomIdent = <Style as CoreStyle>::CustomIdent;

    fn box_generation_mode(&self) -> BoxGenerationMode {
        self.style.box_generation_mode()
    }
    fn is_block(&self) -> bool {
        self.style.is_block()
    }
    fn is_compressible_replaced(&self) -> bool {
        self.style.is_compressible_replaced()
    }
    fn is_replaced(&self) -> bool {
        self.style.is_replaced()
    }
    fn box_sizing(&self) -> BoxSizing {
        self.style.box_sizing()
    }
    fn direction(&self) -> Direction {
        self.style.direction()
    }
    fn overflow(&self) -> Point<Overflow> {
        self.style.overflow()
    }
    fn scrollbar_width(&self) -> f32 {
        self.style.scrollbar_width()
    }
    fn position(&self) -> Position {
        self.style.position()
    }
    fn inset(&self) -> Rect<LengthPercentageAuto> {
        self.style.inset()
    }
    fn is_containing_block(&self) -> ContainingBlockClaims {
        self.claims
    }
    fn size(&self) -> Size<Dimension> {
        self.style.size()
    }
    fn min_size(&self) -> Size<LengthPercentageAuto> {
        self.style.min_size()
    }
    fn max_size(&self) -> Size<LengthPercentageAuto> {
        self.style.max_size()
    }
    fn aspect_ratio(&self) -> Option<f32> {
        self.style.aspect_ratio()
    }
    fn margin(&self) -> Rect<LengthPercentageAuto> {
        self.style.margin()
    }
    fn padding(&self) -> Rect<LengthPercentage> {
        self.style.padding()
    }
    fn border(&self) -> Rect<LengthPercentage> {
        self.style.border()
    }
    fn contain(&self) -> Contain {
        self.style.contain()
    }
}

impl OofItemStyle for HostStyle<'_> {
    fn align_self(&self) -> Option<AlignSelf> {
        OofItemStyle::align_self(self.style)
    }
    fn justify_self(&self) -> Option<AlignSelf> {
        OofItemStyle::justify_self(self.style)
    }
    fn is_table(&self) -> bool {
        OofItemStyle::is_table(self.style)
    }
    fn grid_row(&self) -> Line<GridPlacement<Self::CustomIdent>> {
        OofItemStyle::grid_row(self.style)
    }
    fn grid_column(&self) -> Line<GridPlacement<Self::CustomIdent>> {
        OofItemStyle::grid_column(self.style)
    }
}

impl BlockContainerStyle for HostStyle<'_> {
    fn text_align(&self) -> taffy::TextAlign {
        BlockContainerStyle::text_align(self.style)
    }
    fn align_content(&self) -> AlignContent {
        BlockContainerStyle::align_content(self.style)
    }
    fn justify_items(&self) -> AlignItems {
        BlockContainerStyle::justify_items(self.style)
    }
}

impl BlockItemStyle for HostStyle<'_> {
    fn is_table(&self) -> bool {
        BlockItemStyle::is_table(self.style)
    }
    fn align_self(&self) -> Option<AlignSelf> {
        BlockItemStyle::align_self(self.style)
    }
    fn justify_self(&self) -> Option<AlignSelf> {
        BlockItemStyle::justify_self(self.style)
    }
    fn align_content(&self) -> AlignContent {
        BlockItemStyle::align_content(self.style)
    }
    fn float(&self) -> Float {
        BlockItemStyle::float(self.style)
    }
    fn clear(&self) -> Clear {
        BlockItemStyle::clear(self.style)
    }
}

impl FlexboxContainerStyle for HostStyle<'_> {
    fn flex_direction(&self) -> FlexDirection {
        FlexboxContainerStyle::flex_direction(self.style)
    }
    fn flex_wrap(&self) -> FlexWrap {
        FlexboxContainerStyle::flex_wrap(self.style)
    }
    fn gap(&self) -> Size<LengthPercentage> {
        FlexboxContainerStyle::gap(self.style)
    }
    fn align_content(&self) -> AlignContent {
        FlexboxContainerStyle::align_content(self.style)
    }
    fn align_items(&self) -> AlignItems {
        FlexboxContainerStyle::align_items(self.style)
    }
    fn justify_content(&self) -> JustifyContent {
        FlexboxContainerStyle::justify_content(self.style)
    }
}

impl FlexboxItemStyle for HostStyle<'_> {
    fn flex_basis(&self) -> Dimension {
        FlexboxItemStyle::flex_basis(self.style)
    }
    fn flex_grow(&self) -> f32 {
        FlexboxItemStyle::flex_grow(self.style)
    }
    fn flex_shrink(&self) -> f32 {
        FlexboxItemStyle::flex_shrink(self.style)
    }
    fn align_self(&self) -> Option<AlignSelf> {
        FlexboxItemStyle::align_self(self.style)
    }
}

impl GridContainerStyle for HostStyle<'_> {
    type Repetition<'b>
        = <Style as GridContainerStyle>::Repetition<'b>
    where
        Self: 'b;
    type TemplateTrackList<'b>
        = <Style as GridContainerStyle>::TemplateTrackList<'b>
    where
        Self: 'b;
    type AutoTrackList<'b>
        = <Style as GridContainerStyle>::AutoTrackList<'b>
    where
        Self: 'b;
    type TemplateLineNames<'b>
        = <Style as GridContainerStyle>::TemplateLineNames<'b>
    where
        Self: 'b;
    type GridTemplateAreas<'b>
        = <Style as GridContainerStyle>::GridTemplateAreas<'b>
    where
        Self: 'b;

    fn grid_template_rows(&self) -> Option<Self::TemplateTrackList<'_>> {
        self.style.grid_template_rows()
    }
    fn grid_template_columns(&self) -> Option<Self::TemplateTrackList<'_>> {
        self.style.grid_template_columns()
    }
    fn grid_auto_rows(&self) -> Self::AutoTrackList<'_> {
        self.style.grid_auto_rows()
    }
    fn grid_auto_columns(&self) -> Self::AutoTrackList<'_> {
        self.style.grid_auto_columns()
    }
    fn grid_template_areas(&self) -> Option<Self::GridTemplateAreas<'_>> {
        self.style.grid_template_areas()
    }
    fn grid_template_area_row_count(&self) -> u16 {
        self.style.grid_template_area_row_count()
    }
    fn grid_template_area_column_count(&self) -> u16 {
        self.style.grid_template_area_column_count()
    }
    fn grid_template_column_names(&self) -> Option<Self::TemplateLineNames<'_>> {
        self.style.grid_template_column_names()
    }
    fn grid_template_row_names(&self) -> Option<Self::TemplateLineNames<'_>> {
        self.style.grid_template_row_names()
    }
    fn grid_auto_flow(&self) -> GridAutoFlow {
        GridContainerStyle::grid_auto_flow(self.style)
    }
    fn gap(&self) -> Size<LengthPercentage> {
        GridContainerStyle::gap(self.style)
    }
    fn is_subgrid(&self, axis: taffy::AbsoluteAxis) -> bool {
        GridContainerStyle::is_subgrid(self.style, axis)
    }
    fn gap_is_normal(&self) -> Size<bool> {
        GridContainerStyle::gap_is_normal(self.style)
    }
    fn align_content(&self) -> AlignContent {
        GridContainerStyle::align_content(self.style)
    }
    fn justify_content(&self) -> JustifyContent {
        GridContainerStyle::justify_content(self.style)
    }
    fn align_items(&self) -> AlignItems {
        GridContainerStyle::align_items(self.style)
    }
    fn justify_items(&self) -> AlignItems {
        GridContainerStyle::justify_items(self.style)
    }
}

impl GridItemStyle for HostStyle<'_> {
    fn grid_row(&self) -> Line<GridPlacement<Self::CustomIdent>> {
        GridItemStyle::grid_row(self.style)
    }
    fn grid_column(&self) -> Line<GridPlacement<Self::CustomIdent>> {
        GridItemStyle::grid_column(self.style)
    }
    fn align_self(&self) -> Option<AlignSelf> {
        GridItemStyle::align_self(self.style)
    }
    fn justify_self(&self) -> Option<AlignSelf> {
        GridItemStyle::justify_self(self.style)
    }
    fn is_subgrid_item(&self, axis: taffy::AbsoluteAxis) -> bool {
        GridItemStyle::is_subgrid_item(self.style, axis)
    }
}
