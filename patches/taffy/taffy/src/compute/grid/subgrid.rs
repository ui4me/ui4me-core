//! ui4me P-008-4: CSS Grid Level 2 subgrids (<https://www.w3.org/TR/css-grid-2/#subgrids>).
//!
//! A grid item whose `grid-template-columns` and/or `grid-template-rows` is `subgrid` (a *subgrid*) uses the
//! tracks it spans in its parent grid in that axis (a *subgridded axis*) instead of defining its own:
//!
//!   - The parent grid builds the subgrid's grid (placement included) before sizing its own tracks
//!     ([`prepare_subgrids`]) and records, through [`LayoutGridContainer::set_subgrid_info`], the number of
//!     spanned tracks, their sizing functions, the parent's gap and the names of the spanned lines.
//!   - The subgrid's items take part in the parent's track sizing as *virtual items* ([`flatten`]): their
//!     placement is translated into the parent's lines and their contributions are increased by the subgrid's
//!     margin, border and padding at the subgrid's edges and by half the difference between the subgrid's gap
//!     and the parent's elsewhere. The subgrid itself does not take part in the sizing of a subgridded axis
//!     (it is always stretched there).
//!   - Once the parent has sized a subgridded axis it records the sizes ([`record_sizes`]); the subgrid then
//!     lays itself out with fixed tracks in that axis whose lines sit on the parent's lines (its gutters are
//!     centred in the parent's, its edge tracks are reduced by its margin, border and padding).
//!   - A subgridded axis inherits the parent's line names (plus its own `<line-name-list>`) and has no implicit
//!     tracks: an item placed outside the subgrid is clamped into it. A box that is not a grid item (or is
//!     absolutely positioned) receives no information and its subgridded axis behaves as `none`.
//!
//! Only `horizontal-tb`, left-to-right subgrids are supported (orthogonal and RTL subgrids are not).

use super::types::{GridItem, GridTrack, GridTrackKind};
use super::{build_grid, compute_container_constants, GridBuild};
use crate::geometry::{AbsoluteAxis, AbstractAxis, Line, Rect, Size};
use crate::style::{AvailableSpace, MaxTrackSizingFunction, MinTrackSizingFunction};
use crate::tree::{LayoutInput, LayoutPartialTreeExt, NodeId, RequestedAxis, RunMode, SizingMode};
use crate::util::sys::{f32_max, GridTrackVec, Vec};
use crate::util::{MaybeMath, ResolveOrZero};
use crate::{CheapCloneStr, CoreStyle, GridContainerStyle, LayoutGridContainer, LengthPercentage};

use super::explicit_grid::AutoRepeatStrategy;
use super::track_sizing::{resolve_item_track_indexes, track_sizing_algorithm};
use super::types::NamedLineResolver;
use super::OriginZeroLine;

/// What a parent grid records for a subgridded axis of one of its items
#[derive(Clone, Debug, PartialEq)]
pub struct SubgridAxisInfo<S: CheapCloneStr> {
    /// The number of parent tracks the subgrid spans (its explicit track count)
    pub track_count: u16,
    /// The sizing functions of the spanned parent tracks (used while the sizes are not known yet)
    pub track_sizing: Vec<(MinTrackSizingFunction, MaxTrackSizingFunction)>,
    /// The parent's gap in this axis (px): the subgrid's gap when its own is `normal`
    pub parent_gap: f32,
    /// The sizes of the spanned parent tracks and of the gutters between them (`2 * track_count - 1` values:
    /// track, gutter, track, ...) once the parent has sized them. A gutter includes the space distributed by
    /// the parent's content alignment.
    pub sizes: Option<Vec<f32>>,
    /// The names of the spanned parent lines, as `(1-based subgrid line, name)` pairs
    pub line_names: Vec<(u32, S)>,
}

/// What a parent grid records for a subgrid item: one entry per subgridded axis
#[derive(Clone, Debug, PartialEq)]
pub struct SubgridInfo<S: CheapCloneStr> {
    /// The columns, if `grid-template-columns` is `subgrid`
    pub columns: Option<SubgridAxisInfo<S>>,
    /// The rows, if `grid-template-rows` is `subgrid`
    pub rows: Option<SubgridAxisInfo<S>>,
}

impl<S: CheapCloneStr> SubgridInfo<S> {
    /// The information of an axis
    pub fn axis(&self, axis: AbstractAxis) -> Option<&SubgridAxisInfo<S>> {
        match axis {
            AbstractAxis::Inline => self.columns.as_ref(),
            AbstractAxis::Block => self.rows.as_ref(),
        }
    }

    /// The information of an axis (mutable)
    fn axis_mut(&mut self, axis: AbstractAxis) -> Option<&mut SubgridAxisInfo<S>> {
        match axis {
            AbstractAxis::Inline => self.columns.as_mut(),
            AbstractAxis::Block => self.rows.as_mut(),
        }
    }
}

/// The geometry of a subgrid's edges and gutters
#[derive(Clone, Copy, Debug)]
pub(super) struct SubgridGeometry {
    /// Margin + border + padding at each edge
    pub(super) edges: Rect<f32>,
    /// The subgrid's own margins (part of `edges`)
    pub(super) margins: Rect<f32>,
    /// The subgrid's gap in each axis (px)
    pub(super) gap: Size<f32>,
    /// The parent's gap in each axis (px)
    pub(super) parent_gap: Size<f32>,
    /// Whether the subgrid's gap is `normal` in each axis (it then has the parent's gutters)
    pub(super) gap_normal: Size<bool>,
}

impl SubgridGeometry {
    /// Resolve the geometry of a subgrid. `area_width` is the percentage basis (the width of the subgrid's
    /// grid area, if known).
    pub(super) fn compute<Tree: LayoutGridContainer>(
        tree: &Tree,
        style: &Tree::GridContainerStyle<'_>,
        area_width: Option<f32>,
        info: &SubgridInfo<Tree::CustomIdent>,
    ) -> Self {
        let margins = style.margin().resolve_or_zero(area_width, |val, basis| tree.calc(val, basis));
        let padding = style.padding().resolve_or_zero(area_width, |val, basis| tree.calc(val, basis));
        let border = style.border().resolve_or_zero(area_width, |val, basis| tree.calc(val, basis));
        let edges = margins + padding + border;
        let parent_gap = Size {
            width: info.columns.as_ref().map(|i| i.parent_gap).unwrap_or(0.0),
            height: info.rows.as_ref().map(|i| i.parent_gap).unwrap_or(0.0),
        };
        let normal = style.gap_is_normal();
        let own_gap = style.gap();
        let content_width = area_width.map(|w| f32_max(w - edges.horizontal_axis_sum(), 0.0));
        let resolve = |gap: LengthPercentage, basis: Option<f32>| {
            gap.resolve_or_zero(basis.or(Some(0.0)), |val, basis| tree.calc(val, basis))
        };
        let gap = Size {
            width: if normal.width { parent_gap.width } else { resolve(own_gap.width, content_width) },
            height: if normal.height { parent_gap.height } else { resolve(own_gap.height, None) },
        };
        SubgridGeometry { edges, margins, gap, gap_normal: normal, parent_gap }
    }

    /// The edges of an axis (start, end)
    fn edges(&self, axis: AbstractAxis) -> (f32, f32) {
        match axis {
            AbstractAxis::Inline => (self.edges.left, self.edges.right),
            AbstractAxis::Block => (self.edges.top, self.edges.bottom),
        }
    }

    /// The gap of the subgrid's tracks in `axis`: `None` when it is `normal` (the parent's gutters)
    pub(super) fn track_gap(&self, axis: AbstractAxis) -> Option<f32> {
        (!self.gap_normal.get(axis)).then(|| self.gap.get(axis))
    }
}

/// Clamp the definite lines and spans of the items in `axis` to the subgrid's `track_count` tracks (CSS Grid 2 section 9: a
/// subgridded axis has no implicit tracks; the area is clamped as in an overly large grid)
pub(super) fn clamp_placements_to_subgrid(
    placements: &mut [super::placement::ItemPlacement],
    axis: AbsoluteAxis,
    track_count: u16,
) {
    use crate::style::GenericGridPlacement as P;
    let n = track_count as i16;
    for placement in placements.iter_mut() {
        let line = match axis {
            AbsoluteAxis::Horizontal => &mut placement.horizontal,
            AbsoluteAxis::Vertical => &mut placement.vertical,
        };
        if let P::Line(start) = &mut line.start {
            start.0 = start.0.clamp(0, n - 1);
        }
        if let P::Line(end) = &mut line.end {
            end.0 = end.0.clamp(1, n);
        }
        if let (P::Line(start), P::Line(end)) = (&mut line.start, &mut line.end) {
            if end.0 <= start.0 {
                // Both lines collapsed onto one side of the subgrid: the track next to it
                if start.0 >= n - 1 {
                    *start = OriginZeroLine(n - 1);
                    *end = OriginZeroLine(n);
                } else {
                    *end = OriginZeroLine(start.0 + 1);
                }
            }
        }
        for side in [&mut line.start, &mut line.end] {
            if let P::Span(span) = side {
                *span = (*span).min(track_count).max(1);
            }
        }
    }
}

/// Clamp each item's area in `axis` after placement (an auto-placed item can still land past the last line)
pub(super) fn clamp_items_to_subgrid(items: &mut [GridItem], axis: AbstractAxis, track_count: u16) {
    let n = track_count as i16;
    for item in items {
        let line = match axis {
            AbstractAxis::Inline => &mut item.column,
            AbstractAxis::Block => &mut item.row,
        };
        let start = line.start.0.clamp(0, n - 1);
        let end = line.end.0.clamp(start + 1, n);
        *line = Line { start: OriginZeroLine(start), end: OriginZeroLine(end) };
    }
}

/// The tracks of a subgridded axis: the spanned parent tracks, with fixed sizes when the parent has sized them
/// (the subgrid's lines on the parent's: its gutters, `gap` wide, centred in the parent's gutters, its edge
/// tracks reduced by `edges`), and otherwise with the parent tracks' sizing functions.
pub(super) fn initialize_subgrid_tracks<S: CheapCloneStr>(
    tracks: &mut GridTrackVec<GridTrack>,
    info: &SubgridAxisInfo<S>,
    gap: Option<f32>,
    edges: (f32, f32),
) {
    let n = info.track_count as usize;
    tracks.clear();
    tracks.reserve(n * 2 + 1);
    let mut outer = GridTrack::gutter(LengthPercentage::length(0.0));
    outer.collapse();
    tracks.push(outer.clone());
    match &info.sizes {
        Some(sizes) if sizes.len() == n * 2 - 1 => {
            // The parent's track edges relative to the start of the subgrid's area
            let mut starts = Vec::with_capacity(n);
            let mut ends = Vec::with_capacity(n);
            let mut position = 0.0;
            for k in 0..n {
                starts.push(position);
                position += sizes[2 * k];
                ends.push(position);
                if k + 1 < n {
                    position += sizes[2 * k + 1];
                }
            }
            let area = position;
            // A subgrid whose gap is `normal` has the parent's gutters (with the space the parent's content
            // alignment distributed into them); otherwise its gutters, `gap` wide, are centred in the parent's
            // (as in the reference browser)
            let start = |k: usize| match gap {
                _ if k == 0 => edges.0,
                None => starts[k],
                Some(gap) => (ends[k - 1] + starts[k]) / 2.0 + gap / 2.0,
            };
            let end = |k: usize| match gap {
                _ if k + 1 == n => area - edges.1,
                None => ends[k],
                Some(gap) => (ends[k] + starts[k + 1]) / 2.0 - gap / 2.0,
            };
            for k in 0..n {
                let size = f32_max(end(k) - start(k), 0.0);
                let mut track = GridTrack::new(MinTrackSizingFunction::length(size), MaxTrackSizingFunction::length(size));
                track.base_size = size;
                track.growth_limit = size;
                tracks.push(track);
                if k + 1 < n {
                    // A track that the gap leaves no room for is empty at its start line, so the gutter runs
                    // from there to the next track's start
                    let gutter_size = f32_max(start(k + 1) - (start(k) + size), 0.0);
                    let mut gutter = GridTrack::gutter(LengthPercentage::length(gutter_size));
                    gutter.base_size = gutter_size;
                    gutter.growth_limit = gutter_size;
                    tracks.push(gutter);
                }
            }
        }
        _ => {
            for (k, (min, max)) in info.track_sizing.iter().enumerate() {
                tracks.push(GridTrack::new(*min, *max));
                if k + 1 < n {
                    tracks.push(GridTrack::gutter(LengthPercentage::length(gap.unwrap_or(info.parent_gap))));
                }
            }
        }
    }
    tracks.push(outer);
}

/// A subgrid item of a grid, with its own grid built ahead of the parent's track sizing
pub(super) struct SubgridPrep<S: CheapCloneStr> {
    /// The index of the subgrid in its parent's items
    item_index: usize,
    /// The subgrid's node
    node: NodeId,
    /// The subgridded axes (width = columns, height = rows)
    axes: Size<bool>,
    /// The subgrid's placement in its parent (origin-zero lines and track vector indexes)
    column: Line<OriginZeroLine>,
    row: Line<OriginZeroLine>,
    column_indexes: Line<u16>,
    row_indexes: Line<u16>,
    /// What the parent records for the subgrid
    info: SubgridInfo<S>,
    /// Its edges and gaps
    geometry: SubgridGeometry,
    /// Its grid (subgrid-local placement and tracks)
    build: GridBuild<S>,
    /// The subgrids among its items
    children: Vec<SubgridPrep<S>>,
    /// Whether `build.columns` has been sized (`prepass_columns`)
    columns_sized: bool,
    /// The container's preferred border-box width and its padding + border width (for `prepass_columns`)
    preferred_width: Option<f32>,
    padding_border_width: f32,
    /// The container's content alignment (columns, rows)
    justify_content: crate::style::AlignContent,
    align_content: crate::style::AlignContent,
}

/// The information recorded for a subgridded axis of an item spanning `indexes` (track vector indexes) and
/// `lines` (origin-zero) of `tracks`
fn axis_info<S: CheapCloneStr>(
    tracks: &[GridTrack],
    indexes: Line<u16>,
    lines: Line<OriginZeroLine>,
    names: &NamedLineResolver<S>,
    axis: AbsoluteAxis,
    gap_basis: Option<f32>,
    calc: &dyn Fn(*const (), f32) -> f32,
) -> SubgridAxisInfo<S> {
    let spanned = &tracks[(indexes.start as usize + 1)..(indexes.end as usize)];
    let track_sizing = spanned
        .iter()
        .filter(|track| track.kind == GridTrackKind::Track)
        .map(|track| (track.min_track_sizing_function, track.max_track_sizing_function))
        .collect();
    // The parent's gap: the first inner gutter that is not collapsed (all the parent's gutters have its gap)
    let parent_gap = tracks
        .iter()
        .enumerate()
        .filter(|(index, track)| *index > 0 && *index + 1 < tracks.len() && track.kind == GridTrackKind::Gutter)
        .find(|(_, track)| !track.is_collapsed)
        .and_then(|(_, track)| track.min_track_sizing_function.definite_value(gap_basis.or(Some(0.0)), calc))
        .unwrap_or(0.0);
    SubgridAxisInfo {
        track_count: (lines.end.0 - lines.start.0).max(1) as u16,
        track_sizing,
        parent_gap,
        sizes: None,
        line_names: names.line_names_in_range(axis, lines.start.0, lines.end.0),
    }
}

/// Build the grids of the subgrid items of a grid (recursively) and record their information
pub(super) fn prepare_subgrids<Tree: LayoutGridContainer>(
    tree: &mut Tree,
    items: &[GridItem],
    columns: &[GridTrack],
    rows: &[GridTrack],
    names: &NamedLineResolver<Tree::CustomIdent>,
    inner_node_size: Size<Option<f32>>,
) -> Vec<SubgridPrep<Tree::CustomIdent>> {
    let mut preps = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if !(item.subgrid_axes.width || item.subgrid_axes.height) || item.virtual_axis.is_some() {
            continue;
        }
        let calc = |val: *const (), basis: f32| tree.calc(val, basis);
        let info = SubgridInfo {
            columns: item.subgrid_axes.width.then(|| {
                axis_info(
                    columns,
                    item.column_indexes,
                    item.column,
                    names,
                    AbsoluteAxis::Horizontal,
                    inner_node_size.width,
                    &calc,
                )
            }),
            rows: item.subgrid_axes.height.then(|| {
                axis_info(rows, item.row_indexes, item.row, names, AbsoluteAxis::Vertical, inner_node_size.height, &calc)
            }),
        };
        preps.push(prepare_subgrid(tree, index, item, info, inner_node_size.width));
    }
    preps
}

/// Build the grid of one subgrid item and of its own subgrids
fn prepare_subgrid<Tree: LayoutGridContainer>(
    tree: &mut Tree,
    item_index: usize,
    item: &GridItem,
    info: SubgridInfo<Tree::CustomIdent>,
    area_width: Option<f32>,
) -> SubgridPrep<Tree::CustomIdent> {
    let node = item.node;
    tree.set_subgrid_info(node, Some(info.clone()));

    let (geometry, constants, mut build) = {
        let style = tree.get_grid_container_style(node);
        let geometry = SubgridGeometry::compute(tree, &style, area_width, &info);
        let inputs = LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions: Size::NONE,
            known_dimensions_are_definite: Size { width: true, height: true },
            parent_size: Size { width: area_width, height: None },
            available_space: Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        let constants = compute_container_constants(tree, &style, inputs);
        let auto_fit_container_size = constants
            .outer_node_size
            .or(constants.max_size)
            .or(constants.min_size)
            .maybe_clamp(constants.min_size, constants.max_size)
            .maybe_max(constants.padding_border_size)
            .maybe_sub(constants.content_box_inset.sum_axes());
        let auto_repeat_fit_strategy = constants.outer_node_size.or(constants.max_size).map(|val| match val {
            Some(_) => AutoRepeatStrategy::MaxRepetitionsThatDoNotOverflow,
            None => AutoRepeatStrategy::MinRepetitionsThatDoOverflow,
        });
        let build = build_grid(
            tree,
            node,
            &style,
            auto_fit_container_size,
            auto_repeat_fit_strategy,
            constants.align_items,
            constants.justify_items,
            Some(&info),
            Some(&geometry),
        );
        (geometry, constants, build)
    };
    resolve_item_track_indexes(&mut build.items, build.final_col_counts, build.final_row_counts);
    super::track_sizing::determine_if_item_crosses_flexible_or_intrinsic_tracks(
        &mut build.items,
        &build.columns,
        &build.rows,
    );

    let children = if build.items.iter().any(|item| item.subgrid_axes.width || item.subgrid_axes.height) {
        let content_width = area_width.map(|w| f32_max(w - geometry.edges.horizontal_axis_sum(), 0.0));
        prepare_subgrids(
            tree,
            &build.items,
            &build.columns,
            &build.rows,
            &build.name_resolver,
            Size { width: content_width, height: None },
        )
    } else {
        Vec::new()
    };

    SubgridPrep {
        item_index,
        node,
        axes: item.subgrid_axes,
        column: item.column,
        row: item.row,
        column_indexes: item.column_indexes,
        row_indexes: item.row_indexes,
        info,
        geometry,
        build,
        children,
        columns_sized: false,
        preferred_width: constants.preferred_size.width,
        padding_border_width: constants.content_box_inset.horizontal_axis_sum(),
        justify_content: constants.justify_content,
        align_content: constants.align_content,
    }
}

impl<S: CheapCloneStr> SubgridPrep<S> {
    /// The subgrid's placement in its parent in `axis` (origin-zero lines, track vector indexes)
    fn placement(&self, axis: AbstractAxis) -> (Line<OriginZeroLine>, Line<u16>) {
        match axis {
            AbstractAxis::Inline => (self.column, self.column_indexes),
            AbstractAxis::Block => (self.row, self.row_indexes),
        }
    }

    /// The subgrid's own tracks in `axis`
    fn tracks(&self, axis: AbstractAxis) -> &[GridTrack] {
        match axis {
            AbstractAxis::Inline => &self.build.columns,
            AbstractAxis::Block => &self.build.rows,
        }
    }
}

/// The size of the area spanning `indexes` of `tracks`: their sizes when `sized`, otherwise the sum of their
/// fixed sizes (`None` if a track is not fixed)
fn area_size(tracks: &[GridTrack], indexes: Line<u16>, sized: bool) -> Option<f32> {
    tracks[(indexes.start as usize + 1)..(indexes.end as usize)]
        .iter()
        .map(|track| {
            if sized {
                return Some(track.base_size);
            }
            let min = track.min_track_sizing_function.definite_value(None, |_, _| 0.0)?;
            let max = track.max_track_sizing_function.definite_value(None, |_, _| 0.0)?;
            (min == max).then_some(min)
        })
        .sum()
}

/// Translate a virtual item from a subgrid's coordinates into its parent's in `axis`, adding the subgrid's
/// edge and gap adjustments to its extra margin
fn translate<S: CheapCloneStr>(item: &mut GridItem, axis: AbstractAxis, prep: &SubgridPrep<S>) {
    let track_count = prep.info.axis(axis).map(|info| info.track_count).unwrap_or(1) as i16;
    let (lines, indexes) = prep.placement(axis);
    let (edge_start, edge_end) = prep.geometry.edges(axis);
    let half_gap_difference = (prep.geometry.gap.get(axis) - prep.geometry.parent_gap.get(axis)) / 2.0;
    let (item_lines, item_indexes) = match axis {
        AbstractAxis::Inline => (&mut item.column, &mut item.column_indexes),
        AbstractAxis::Block => (&mut item.row, &mut item.row_indexes),
    };
    // A spacer (an empty edge item) carries the edge only
    let inner = if item.spacer { 0.0 } else { half_gap_difference };
    let start = if item_lines.start.0 <= 0 { edge_start } else { inner };
    let end = if item_lines.end.0 >= track_count { edge_end } else { inner };
    match axis {
        AbstractAxis::Inline => {
            item.extra_margin.left += start;
            item.extra_margin.right += end;
        }
        AbstractAxis::Block => {
            item.extra_margin.top += start;
            item.extra_margin.bottom += end;
        }
    }
    *item_lines = Line {
        start: OriginZeroLine(item_lines.start.0 + lines.start.0),
        end: OriginZeroLine(item_lines.end.0 + lines.start.0),
    };
    *item_indexes = Line { start: item_indexes.start + indexes.start, end: item_indexes.end + indexes.start };
}

/// The virtual items of a subgrid for its parent's track sizing in `axis` (a subgridded axis of `prep`), in the
/// parent's coordinates: its items, and recursively those of its own subgrids in that axis
pub(super) fn flatten<S: CheapCloneStr>(prep: &SubgridPrep<S>, axis: AbstractAxis) -> Vec<GridItem> {
    let mut out = Vec::new();
    if !prep.axes.get(axis) {
        return out;
    }
    let other = axis.other();
    let mut local: Vec<GridItem> = Vec::new();
    for (index, item) in prep.build.items.iter().enumerate() {
        if item.subgrid_axes.get(axis) {
            if let Some(child) = prep.children.iter().find(|child| child.item_index == index) {
                local.extend(flatten(child, axis));
            }
        } else {
            let mut copy = item.virtual_copy(axis);
            copy.subgrid_axes = Size { width: false, height: false };
            local.push(copy);
        }
    }
    // The margin, border and padding at each edge of a non-empty subgrid are contributed to the edge track by an
    // empty item even when the items there contribute less (or no item touches that edge), as in the reference
    // browser
    if let Some(first) = local.first() {
        let track_count = prep.info.axis(axis).map(|info| info.track_count).unwrap_or(1) as i16;
        let mut phantoms = Vec::new();
        let edge_tracks: &[i16] = if track_count == 1 { &[0] } else { &[0, track_count - 1] };
        for &line in edge_tracks {
            let mut phantom = first.virtual_copy(axis);
            phantom.make_empty_spacer(axis);
            let lines = Line { start: OriginZeroLine(line), end: OriginZeroLine(line + 1) };
            let indexes = Line { start: 2 * line as u16, end: 2 * (line as u16 + 1) };
            match axis {
                AbstractAxis::Inline => {
                    phantom.column = lines;
                    phantom.column_indexes = indexes;
                }
                AbstractAxis::Block => {
                    phantom.row = lines;
                    phantom.row_indexes = indexes;
                }
            }
            phantoms.push(phantom);
        }
        local.extend(phantoms);
    }
    {
        for mut virtual_item in local {
            translate(&mut virtual_item, axis, prep);
            if virtual_item.area_override.is_none() {
                if prep.axes.get(other) {
                    translate(&mut virtual_item, other, prep);
                } else {
                    let indexes = match other {
                        AbstractAxis::Inline => virtual_item.column_indexes,
                        AbstractAxis::Block => virtual_item.row_indexes,
                    };
                    let sized = other == AbstractAxis::Inline && prep.columns_sized;
                    virtual_item.area_override = Some(area_size(prep.tracks(other), indexes, sized));
                }
            }
            out.push(virtual_item);
        }
    }
    out
}

/// The virtual items of all the subgrids of a grid for its track sizing in `axis`, with their crossing flags
/// computed against the grid's tracks
pub(super) fn virtual_items<S: CheapCloneStr>(
    preps: &[SubgridPrep<S>],
    axis: AbstractAxis,
    axis_tracks: &[GridTrack],
) -> Vec<GridItem> {
    let mut out = Vec::new();
    for prep in preps {
        out.extend(flatten(prep, axis));
    }
    for item in out.iter_mut() {
        let range = item.track_range_excluding_lines(axis);
        let flexible = axis_tracks[range.clone()].iter().any(|track| track.is_flexible());
        let intrinsic = axis_tracks[range].iter().any(|track| track.has_intrinsic_sizing_function());
        match axis {
            AbstractAxis::Inline => {
                item.crosses_flexible_column = flexible;
                item.crosses_intrinsic_column = intrinsic;
            }
            AbstractAxis::Block => {
                item.crosses_flexible_row = flexible;
                item.crosses_intrinsic_row = intrinsic;
            }
        }
    }
    out
}

/// The sizes of the tracks spanning `indexes` and of the gutters between them: from their base sizes, or (when
/// `from_offsets`, after track alignment) with each gutter measured between the tracks' offsets
fn spanned_sizes(tracks: &[GridTrack], indexes: Line<u16>, from_offsets: bool) -> Vec<f32> {
    let range = (indexes.start as usize + 1)..(indexes.end as usize);
    let mut sizes = Vec::with_capacity(range.len());
    for index in range.clone() {
        let track = &tracks[index];
        if track.kind == GridTrackKind::Gutter && from_offsets {
            let previous = &tracks[index - 1];
            let next = &tracks[index + 1];
            sizes.push(f32_max(next.offset - (previous.offset + previous.base_size), 0.0));
        } else {
            sizes.push(track.base_size);
        }
    }
    sizes
}

/// Record the sizes of a grid's tracks in `axis` in the information of the subgrids that span them
pub(super) fn record_sizes<Tree: LayoutGridContainer>(
    tree: &mut Tree,
    preps: &mut [SubgridPrep<Tree::CustomIdent>],
    axis: AbstractAxis,
    axis_tracks: &[GridTrack],
    from_offsets: bool,
) {
    for prep in preps.iter_mut() {
        if !prep.axes.get(axis) {
            continue;
        }
        let (_, indexes) = prep.placement(axis);
        let sizes = spanned_sizes(axis_tracks, indexes, from_offsets);
        let parent_gap = (sizes.len() > 1 && !from_offsets).then(|| sizes[1]);
        if let Some(info) = prep.info.axis_mut(axis) {
            info.sizes = Some(sizes);
            if let Some(gap) = parent_gap {
                info.parent_gap = gap;
            }
        }
        tree.set_subgrid_info(prep.node, Some(prep.info.clone()));
    }
}

/// Size the own columns of the subgrids whose rows are subgridded (and recursively of their subgrids), given
/// the grid's sized columns: the area widths of their items in the rows are then known for the row sizing of
/// the grid (the subgrid's width is its area's, as it is stretched)
pub(super) fn prepass_columns<Tree: LayoutGridContainer>(
    tree: &mut Tree,
    preps: &mut [SubgridPrep<Tree::CustomIdent>],
    columns: &[GridTrack],
) {
    for prep in preps.iter_mut() {
        if !prep.axes.height {
            continue;
        }
        let area = area_size(columns, prep.column_indexes, true).unwrap_or(0.0);
        let sizes = prep.axes.width.then(|| spanned_sizes(columns, prep.column_indexes, false));
        prepass_one(tree, prep, area, sizes);
    }
}

/// `prepass_columns` for one subgrid whose column area is `area` wide (`sizes`: the spanned parent column sizes
/// when its columns are subgridded)
fn prepass_one<Tree: LayoutGridContainer>(
    tree: &mut Tree,
    prep: &mut SubgridPrep<Tree::CustomIdent>,
    area: f32,
    sizes: Option<Vec<f32>>,
) {
    if prep.axes.width {
        if let Some(info) = prep.info.columns.as_ref() {
            let mut info = info.clone();
            info.sizes = sizes;
            initialize_subgrid_tracks(
                &mut prep.build.columns,
                &info,
                prep.geometry.track_gap(AbstractAxis::Inline),
                (prep.geometry.edges.left, prep.geometry.edges.right),
            );
        }
    } else {
        let content_width = match prep.preferred_width {
            Some(width) => f32_max(width - prep.padding_border_width, 0.0),
            None => f32_max(area - prep.geometry.edges.horizontal_axis_sum(), 0.0),
        };
        let real_count = prep.build.items.len();
        let mut virtual_columns = Vec::new();
        for child in prep.children.iter() {
            virtual_columns.extend(flatten(child, AbstractAxis::Inline));
        }
        prep.build.items.extend(virtual_columns);
        for item in prep.build.items[real_count..].iter_mut() {
            let range = item.track_range_excluding_lines(AbstractAxis::Inline);
            item.crosses_flexible_column = prep.build.columns[range.clone()].iter().any(|t| t.is_flexible());
            item.crosses_intrinsic_column =
                prep.build.columns[range].iter().any(|t| t.has_intrinsic_sizing_function());
        }
        track_sizing_algorithm(
            tree,
            AbstractAxis::Inline,
            None,
            None,
            prep.justify_content,
            prep.align_content,
            Size { width: AvailableSpace::Definite(content_width), height: AvailableSpace::MaxContent },
            Size { width: Some(content_width), height: None },
            &mut prep.build.columns,
            &mut prep.build.rows,
            &mut prep.build.items,
            |track: &GridTrack, parent_size: Option<f32>, tree: &Tree| {
                track.max_track_sizing_function.definite_value(parent_size, |val, basis| tree.calc(val, basis))
            },
            false,
        );
        prep.build.items.truncate(real_count);
        // The intrinsic caches of the subgrid's items are only valid for this pass
        for item in prep.build.items.iter_mut() {
            item.grid_area_size_cache = None;
            item.known_dimensions_cache = None;
            item.min_content_contribution_cache = Size::NONE;
            item.max_content_contribution_cache = Size::NONE;
            item.minimum_contribution_cache = Size::NONE;
        }
    }
    prep.columns_sized = true;

    let columns = &prep.build.columns;
    let mut child_inputs = Vec::with_capacity(prep.children.len());
    for child in prep.children.iter() {
        let child_area = area_size(columns, child.column_indexes, true).unwrap_or(0.0);
        let child_sizes = child.axes.width.then(|| spanned_sizes(columns, child.column_indexes, false));
        child_inputs.push((child_area, child_sizes));
    }
    for (child, (child_area, child_sizes)) in prep.children.iter_mut().zip(child_inputs) {
        prepass_one(tree, child, child_area, child_sizes);
    }
}

/// The virtual items of a grid's subgrids: those of one axis are in the grid's items (after its real items),
/// those of the other axis wait here with their caches
pub(super) struct VirtualItems {
    /// The number of real items
    real_count: usize,
    /// The axis whose virtual items are in the grid's items
    current: Option<AbstractAxis>,
    /// The waiting virtual items (columns, rows)
    store: [Vec<GridItem>; 2],
}

impl VirtualItems {
    /// No virtual items yet
    pub(super) fn new(real_count: usize) -> Self {
        VirtualItems { real_count, current: None, store: [Vec::new(), Vec::new()] }
    }

    /// The store slot of an axis
    fn slot(axis: AbstractAxis) -> usize {
        match axis {
            AbstractAxis::Inline => 0,
            AbstractAxis::Block => 1,
        }
    }

    /// Put the virtual items of `axis` in `items`, keeping the other axis's (with their caches) here
    pub(super) fn swap_in(&mut self, items: &mut Vec<GridItem>, axis: AbstractAxis) {
        if self.current == Some(axis) {
            return;
        }
        let current: Vec<GridItem> = items.drain(self.real_count..).collect();
        if let Some(current_axis) = self.current {
            self.store[Self::slot(current_axis)] = current;
        }
        items.append(&mut self.store[Self::slot(axis)]);
        self.current = Some(axis);
    }

    /// Replace the virtual items of `axis` with `fresh` ones and put them in `items`
    pub(super) fn replace(&mut self, items: &mut Vec<GridItem>, axis: AbstractAxis, fresh: Vec<GridItem>) {
        self.store[Self::slot(axis)] = fresh;
        if self.current == Some(axis) {
            items.truncate(self.real_count);
            self.current = None;
        }
        self.swap_in(items, axis);
    }

    /// Remove the virtual items from `items` (before the items are positioned)
    pub(super) fn remove(&mut self, items: &mut Vec<GridItem>) {
        items.truncate(self.real_count);
        self.current = None;
    }
}
