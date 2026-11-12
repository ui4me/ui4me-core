//! owner: 008-layout-engine (T103, L-TBL-r4) — row heights, `rowspan` distribution and the vertical
//! alignment of cell content.
//!
//! Clean-room (T098). CSS 2.1 §17.5.3:
//!
//! * the height of a row is the largest of its specified `height`, the heights of the cells that
//!   start in it and span only it, and (for `baseline`-aligned cells) the room the shared baseline
//!   needs: the largest distance from a cell's top to its baseline, plus the largest distance from
//!   its baseline to its bottom;
//! * a cell spanning several rows adds what the rows cannot already hold (narrower spans first),
//!   shared by the rows it spans that have no specified height, in proportion to their heights
//!   (equally when all are empty; all spanned rows when every one is specified, as browsers do);
//!   a spanning `baseline` cell aligns with its first row;
//! * `vertical-align: top | middle | bottom | baseline` positions the content in the cell; every
//!   other value acts as `baseline`;
//! * extra table height (a specified `height` larger than the rows) is shared by the rows in
//!   proportion to their heights (equally when all are zero).

/// Vertical alignment of a cell's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Baseline,
    Top,
    Middle,
    Bottom,
}

/// A cell as the row heights see it. Heights are border-box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HCell {
    pub row: usize,
    pub row_span: usize,
    /// Height of the cell laid out at its final width.
    pub height: f32,
    /// First baseline from the cell's border-box top.
    pub baseline: f32,
    pub align: VAlign,
    /// Specified `height` (border-box), a minimum for the rows it spans.
    pub spec_height: Option<f32>,
}

/// Row heights and the baseline of each row from its top (`None` when no baseline cell starts in it).
#[derive(Clone, Debug, PartialEq)]
pub struct RowHeights {
    pub heights: Vec<f32>,
    pub baselines: Vec<Option<f32>>,
    /// Rows with a specified `height` (their own or a single-row cell's).
    pub constrained: Vec<bool>,
}

/// Compute the rows (module docs). `row_specs` are the rows' specified heights, `spacing` the
/// vertical border-spacing between rows (0 in the collapsed model).
pub fn row_heights(
    n_rows: usize,
    row_specs: &[Option<f32>],
    cells: &[HCell],
    spacing: f32,
) -> RowHeights {
    let mut baselines: Vec<Option<f32>> = vec![None; n_rows];
    for c in cells
        .iter()
        .filter(|c| c.align == VAlign::Baseline && c.row < n_rows)
    {
        let b = &mut baselines[c.row];
        *b = Some(b.map_or(c.baseline, |x| x.max(c.baseline)));
    }
    let mut heights: Vec<f32> = (0..n_rows)
        .map(|r| row_specs.get(r).copied().flatten().unwrap_or(0.0).max(0.0))
        .collect();
    // The room a cell needs, given where its baseline has to sit.
    let need = |c: &HCell, baselines: &[Option<f32>]| -> f32 {
        let own = match (c.align, baselines.get(c.row).copied().flatten()) {
            (VAlign::Baseline, Some(rb)) => rb - c.baseline + c.height,
            _ => c.height,
        };
        own.max(c.spec_height.unwrap_or(0.0))
    };
    // A row is "specified" when it, or a cell that spans only it, has a `height`.
    let mut constrained: Vec<bool> = (0..n_rows)
        .map(|r| row_specs.get(r).copied().flatten().is_some())
        .collect();
    for c in cells.iter().filter(|c| c.row_span <= 1 && c.row < n_rows) {
        heights[c.row] = heights[c.row].max(need(c, &baselines));
        if c.spec_height.is_some() {
            constrained[c.row] = true;
        }
    }
    let mut spanning: Vec<&HCell> = cells
        .iter()
        .filter(|c| c.row_span > 1 && c.row < n_rows)
        .collect();
    spanning.sort_by_key(|c| c.row_span);
    for c in spanning {
        let end = (c.row + c.row_span).min(n_rows);
        let n = end - c.row;
        let have: f32 = heights[c.row..end].iter().sum::<f32>() + spacing * (n as f32 - 1.0);
        let want = need(c, &baselines);
        if want > have {
            // The missing height goes to the rows without a specified height (in proportion to their
            // heights, equally when all are empty); when every spanned row is specified, to all of them.
            let mut eligible: Vec<usize> = (c.row..end).filter(|r| !constrained[*r]).collect();
            if eligible.is_empty() {
                eligible = (c.row..end).collect();
            }
            let sum: f32 = eligible.iter().map(|r| heights[*r]).sum();
            let extra = want - have;
            for r in eligible.iter().copied() {
                heights[r] += if sum > 0.0 {
                    extra * heights[r] / sum
                } else {
                    extra / eligible.len() as f32
                };
            }
        }
    }
    RowHeights {
        heights,
        baselines,
        constrained,
    }
}

/// Share `extra` height among the rows: the rows without a specified height (`eligible`) take it in
/// proportion to their heights, equally when all are empty; when every row is specified, all rows do
/// (browsers' table height distribution, CSS Tables 3 §"Distributing height to rows").
pub fn distribute_extra_height(heights: &mut [f32], eligible: &[bool], extra: f32) {
    if extra <= 0.0 || heights.is_empty() {
        return;
    }
    let mut pick: Vec<usize> = (0..heights.len())
        .filter(|i| eligible.get(*i).copied().unwrap_or(true))
        .collect();
    if pick.is_empty() {
        pick = (0..heights.len()).collect();
    }
    let total: f32 = pick.iter().map(|i| heights[*i]).sum();
    let n = pick.len() as f32;
    for i in pick {
        heights[i] += if total > 0.0 {
            extra * heights[i] / total
        } else {
            extra / n
        };
    }
}

/// The offset of a cell's content from the top of its content box (the extra top padding):
/// `box_height` is the height of the cell's slot (spanned rows), `height` the cell laid out at its
/// own height, `baseline` its first baseline, `row_baseline` the baseline of the row it starts in.
pub fn content_offset(
    align: VAlign,
    box_height: f32,
    height: f32,
    baseline: f32,
    row_baseline: Option<f32>,
) -> f32 {
    let extra = (box_height - height).max(0.0);
    match align {
        VAlign::Top => 0.0,
        VAlign::Middle => extra / 2.0,
        VAlign::Bottom => extra,
        VAlign::Baseline => row_baseline.map_or(0.0, |rb| (rb - baseline).clamp(0.0, extra)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(row: usize, span: usize, h: f32, base: f32, align: VAlign) -> HCell {
        HCell {
            row,
            row_span: span,
            height: h,
            baseline: base,
            align,
            spec_height: None,
        }
    }

    #[test]
    fn row_is_as_high_as_its_tallest_cell() {
        let r = row_heights(
            2,
            &[],
            &[
                cell(0, 1, 20.0, 10.0, VAlign::Top),
                cell(0, 1, 35.0, 10.0, VAlign::Top),
                cell(1, 1, 5.0, 5.0, VAlign::Top),
            ],
            2.0,
        );
        assert_eq!(r.heights, vec![35.0, 5.0]);
    }

    #[test]
    fn specified_row_height_is_a_minimum() {
        let r = row_heights(
            1,
            &[Some(50.0)],
            &[cell(0, 1, 20.0, 10.0, VAlign::Top)],
            0.0,
        );
        assert_eq!(r.heights, vec![50.0]);
        let r = row_heights(
            1,
            &[Some(10.0)],
            &[cell(0, 1, 20.0, 10.0, VAlign::Top)],
            0.0,
        );
        assert_eq!(r.heights, vec![20.0]);
    }

    #[test]
    fn baseline_cells_share_a_baseline() {
        // Ascent 15 / descent 5 and ascent 8 / descent 20: the row needs 15 + 20 = 35.
        let r = row_heights(
            1,
            &[],
            &[
                cell(0, 1, 20.0, 15.0, VAlign::Baseline),
                cell(0, 1, 28.0, 8.0, VAlign::Baseline),
            ],
            0.0,
        );
        assert_eq!(r.baselines, vec![Some(15.0)]);
        assert_eq!(r.heights, vec![35.0]);
        assert_eq!(
            content_offset(VAlign::Baseline, 35.0, 28.0, 8.0, r.baselines[0]),
            7.0
        );
        assert_eq!(
            content_offset(VAlign::Baseline, 35.0, 20.0, 15.0, r.baselines[0]),
            0.0
        );
    }

    #[test]
    fn rowspan_distributes_the_missing_height_equally() {
        // Rows hold 10 and 10 (+ spacing 4 between = 24); the span-2 cell needs 60 -> +36, 18 each.
        let r = row_heights(
            2,
            &[],
            &[
                cell(0, 1, 10.0, 5.0, VAlign::Top),
                cell(1, 1, 10.0, 5.0, VAlign::Top),
                cell(0, 2, 60.0, 5.0, VAlign::Top),
            ],
            4.0,
        );
        assert_eq!(r.heights, vec![28.0, 28.0]);
        // A span-2 cell that fits changes nothing.
        let r = row_heights(
            2,
            &[],
            &[
                cell(0, 1, 30.0, 5.0, VAlign::Top),
                cell(1, 1, 30.0, 5.0, VAlign::Top),
                cell(0, 2, 20.0, 5.0, VAlign::Top),
            ],
            0.0,
        );
        assert_eq!(r.heights, vec![30.0, 30.0]);
    }

    #[test]
    fn middle_and_bottom_offsets() {
        assert_eq!(content_offset(VAlign::Middle, 50.0, 20.0, 0.0, None), 15.0);
        assert_eq!(content_offset(VAlign::Bottom, 50.0, 20.0, 0.0, None), 30.0);
        assert_eq!(content_offset(VAlign::Top, 50.0, 20.0, 0.0, None), 0.0);
    }

    #[test]
    fn extra_height_is_proportional() {
        let mut h = vec![10.0, 30.0];
        distribute_extra_height(&mut h, &[true, true], 40.0);
        assert_eq!(h, vec![20.0, 60.0]);
        let mut h = vec![0.0, 0.0];
        distribute_extra_height(&mut h, &[true, true], 10.0);
        assert_eq!(h, vec![5.0, 5.0]);
        // Specified rows keep their height while an auto row can take the extra.
        let mut h = vec![20.0, 30.0];
        distribute_extra_height(&mut h, &[true, false], 50.0);
        assert_eq!(h, vec![70.0, 30.0]);
    }
}
