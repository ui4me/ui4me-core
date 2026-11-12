//! owner: 015-core-completion (STAB-1, lane L-STABL) — fragmentation of flex and grid containers (CSS
//! Fragmentation 3 section 4.4, CSS Flexbox 1 section 12, CSS Grid 2 section 12), on the flow-thread model of
//! plan §A3.
//!
//! A flex or grid container is a breakable box. Taffy (patch P-008-5) hands the host its **runs**: the flex lines
//! of a row container, the items of a single-line column container, the row tracks (and gutters) of a grid,
//! with the items in each; the host answers with struts, and for a grid with grown rows
//! ([`FragContext::place_runs`]). Items are laid out at their offset in the flow thread
//! (`LayoutPartialTree::frag_begin_item`), so their own content breaks with struts as block content does.
//!
//! Rules, measured in the reference browser (Chromium 156, multicol and print; lane L-STABL probes):
//!
//! * A run is a unit for breaks *before* it: it moves whole to the next fragmentainer when one of its items is
//!   monolithic (or `break-inside: avoid` and fits) and would cross the boundary, when an item's first content
//!   was pushed past the boundary in the previous pass (a line, `orphans`, a pushed child: the items of a row
//!   stay aligned across the break), and for a forced break (`break-before` of an item of the run, or
//!   `break-after` of an item of the run before; not before the first run). At the top of a fragmentainer
//!   nothing moves. A container whose first run moves is itself pushed whole (no empty leading piece).
//! * Content inside an item breaks with struts; what they add to the item's block size (its fragmented auto
//!   height less its unfragmented one) is its **growth**. A grown item makes its flex line (its hypothetical cross
//!   size, row flex), its own main size (column flex) or its grid row (rows of single-row items) taller in the next
//!   pass, so stretched items of the same line or row grow with it, and later runs move down.
//! * Items with a definite block size (or stretched to their run) take up their whole size and are sliced.
//!
//! Not covered (monolithic, reported in `docs/SUPPORTED_FEATURES.md`): `flex-wrap: wrap-reverse`, column
//! containers that wrap or are reversed, subgrids; a row-spanning grid item keeps the size its rows had without
//! the struts between them.

use super::{BreakAppeal, BreakKind, BreakToken, EPS, FragContext, FragFacts};
use crate::layout::box_tree::{FlexDirectionValue, FlexStyle, FlexWrapValue};

/// Whether a flex container with this style is broken by this module (its runs follow the block axis forwards).
pub(crate) fn flex_breakable(flex: &FlexStyle) -> bool {
    match flex.direction {
        FlexDirectionValue::Row | FlexDirectionValue::RowReverse => !matches!(
            flex.wrap,
            FlexWrapValue::WrapReverse | FlexWrapValue::BalanceReverse
        ),
        FlexDirectionValue::Column => flex.wrap == FlexWrapValue::NoWrap,
        FlexDirectionValue::ColumnReverse => false,
    }
}

/// An item of a run (Taffy's `FragRunItem` with the item's fragmentation facts).
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunItem {
    pub node: usize,
    pub first: usize,
    pub last: usize,
    pub offset: f32,
    pub size: Option<f32>,
    pub facts: FragFacts,
}

impl FragContext {
    /// Struts (and grid row growth) for the runs of `container` (see the module docs). `runs` are in the container's
    /// border-box space; `grid`: grow the rows of single-row items by their growth.
    pub(crate) fn place_runs(
        &mut self,
        container: usize,
        grid: bool,
        runs: &mut [taffy::FragRun],
        items: &[RunItem],
    ) {
        let Some(&cabs) = self.abs.get(&container) else {
            return;
        };
        if grid {
            let mut need = vec![0.0f32; runs.len()];
            for it in items {
                if it.first == it.last
                    && it.first < runs.len()
                    && let Some(&g) = self.grow.get(&it.node)
                {
                    need[it.first] = need[it.first].max(g);
                }
            }
            let mut shift = 0.0;
            for (k, r) in runs.iter_mut().enumerate() {
                r.start += shift;
                r.size += need[k];
                shift += need[k];
            }
        }
        let first_run = items.iter().map(|it| it.first).min();
        let mut delta = 0.0f32;
        let mut after_forced = false;
        for (k, run) in runs.iter_mut().enumerate() {
            run.start += delta;
            let mut members = items.iter().filter(|it| it.first == k).peekable();
            if members.peek().is_none() {
                continue;
            }
            let y = cabs + run.start;
            let size = run.size;
            let mut forced = Some(k) != first_run
                && (after_forced
                    || items
                        .iter()
                        .any(|it| it.first == k && it.facts.break_before_forced));
            after_forced = items
                .iter()
                .any(|it| it.first == k && it.facts.break_after_forced);
            if forced && self.in_lead(y) {
                forced = false;
            }
            if self.unbounded() {
                // Balancing's measuring layout: no break is taken; it notes where forced breaks would be and that a
                // run with a monolithic item is as tall as that item at least.
                if forced && y > EPS {
                    self.forced_marks.push(y);
                }
                if self.measure {
                    for it in items.iter().filter(|it| {
                        it.first == k && (it.facts.monolithic || it.facts.avoid_inside)
                    }) {
                        self.tallest = self
                            .tallest
                            .max(it.offset + it.size.unwrap_or(size - it.offset));
                    }
                }
                continue;
            }
            if y <= EPS || self.at_fragmentainer_start(y) {
                continue;
            }
            let boundary = self.next_boundary(y);
            let room = self.size_at(boundary);
            let mut push = forced;
            let mut kind = BreakKind::Forced;
            for it in items.iter().filter(|it| it.first == k) {
                let top = y + it.offset;
                let h = it.size.unwrap_or(size - it.offset).max(0.0);
                let unbreakable = it.facts.monolithic || (it.facts.avoid_inside && h <= room + EPS);
                if unbreakable && top + h > boundary + EPS {
                    if self.measure {
                        self.note_shortage(top, h, boundary);
                    }
                    push = true;
                    kind = BreakKind::PushedBox;
                }
                if self
                    .pushed_whole
                    .get(&it.node)
                    .is_some_and(|at| (at - y).abs() <= EPS)
                {
                    if self.measure
                        && let Some(&l) = self.lead_lack.get(&it.node)
                    {
                        self.note_lack(l);
                    }
                    push = true;
                    kind = BreakKind::PushedBox;
                }
            }
            if forced {
                kind = BreakKind::Forced;
            }
            if push {
                let s = boundary - y;
                delta += s;
                run.start += s;
                let node = items
                    .iter()
                    .find(|it| it.first == k)
                    .map_or(container, |it| it.node);
                self.tokens.push(BreakToken {
                    node,
                    offset: boundary,
                    kind,
                    appeal: BreakAppeal::Perfect,
                });
                if Some(k) == first_run {
                    self.leading_pushed.insert(container);
                }
            }
        }
        for it in items {
            if let Some(r) = runs.get(it.first) {
                self.run_at.insert(it.node, cabs + r.start);
            }
        }
    }

    /// An item's final layout ended: `grow` is what breaks inside it added to its block size, `first_pushed` whether
    /// its first content moved past a break. Decisions for the next pass (the root is stale when they change).
    pub(crate) fn end_item(&mut self, item: usize, grow: f32, first_pushed: bool) {
        let old = self.next_grow.get(&item).copied().unwrap_or(0.0);
        if (grow - old).abs() > EPS {
            if grow > EPS {
                self.next_grow.insert(item, grow);
            } else {
                self.next_grow.remove(&item);
            }
            self.stale = true;
        }
        if first_pushed
            && let Some(&at) = self.run_at.get(&item)
            && !self.at_fragmentainer_start(at)
            && self
                .pushed_whole
                .insert(item, at)
                .is_none_or(|old| (old - at).abs() > EPS)
        {
            self.stale = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Carry, FragContext};
    use super::*;

    fn item(node: usize, run: usize, size: f32, facts: FragFacts) -> RunItem {
        RunItem {
            node,
            first: run,
            last: run,
            offset: 0.0,
            size: Some(size),
            facts,
        }
    }

    fn runs(v: &[(f32, f32)]) -> Vec<taffy::FragRun> {
        v.iter()
            .map(|&(start, size)| taffy::FragRun { start, size })
            .collect()
    }

    #[test]
    fn a_run_with_a_monolithic_item_crossing_a_boundary_moves_whole() {
        let mut c = FragContext::new(0, 100.0, Carry::default());
        c.abs.insert(1, 50.0);
        let mono = FragFacts {
            monolithic: true,
            ..Default::default()
        };
        let mut r = runs(&[(0.0, 30.0), (30.0, 30.0), (60.0, 30.0)]);
        let items = [
            item(2, 0, 30.0, mono),
            item(3, 1, 30.0, mono),
            item(4, 1, 20.0, FragFacts::default()),
            item(5, 2, 30.0, mono),
        ];
        c.place_runs(1, false, &mut r, &items);
        assert_eq!(
            r[1].start, 50.0,
            "the second run (80..110) moves to the boundary at 100"
        );
        assert_eq!(r[2].start, 80.0, "later runs follow");
        assert_eq!(c.tokens.len(), 1);
        assert!(!c.leading_pushed.contains(&1));
    }

    #[test]
    fn the_first_run_moving_pushes_the_container_and_forced_breaks_skip_the_first_run() {
        let mut c = FragContext::new(0, 100.0, Carry::default());
        c.abs.insert(1, 40.0);
        let mono = FragFacts {
            monolithic: true,
            ..Default::default()
        };
        let forced = FragFacts {
            break_before_forced: true,
            ..Default::default()
        };
        let mut r = runs(&[(0.0, 80.0)]);
        c.place_runs(1, false, &mut r, &[item(2, 0, 80.0, mono)]);
        assert_eq!(r[0].start, 60.0);
        assert!(c.leading_pushed.contains(&1));
        let mut c = FragContext::new(0, 100.0, Carry::default());
        c.abs.insert(1, 10.0);
        let mut r = runs(&[(0.0, 10.0), (10.0, 10.0)]);
        c.place_runs(
            1,
            false,
            &mut r,
            &[item(2, 0, 10.0, forced), item(3, 1, 10.0, forced)],
        );
        assert_eq!((r[0].start, r[1].start), (0.0, 90.0));
    }

    #[test]
    fn grid_rows_grow_and_pushed_items_move_their_run_next_pass() {
        let mut carry = Carry::default();
        carry.grow.insert(2, 10.0);
        carry.pushed_whole.insert(5, 80.0);
        let mut c = FragContext::new(0, 100.0, carry);
        c.abs.insert(1, 50.0);
        let mut r = runs(&[
            (0.0, 0.0),
            (0.0, 20.0),
            (20.0, 0.0),
            (20.0, 20.0),
            (40.0, 0.0),
        ]);
        let none = FragFacts::default();
        c.place_runs(
            1,
            true,
            &mut r,
            &[
                item(2, 1, 20.0, none),
                item(3, 1, 20.0, none),
                item(5, 3, 20.0, none),
            ],
        );
        assert_eq!(r[1].size, 30.0);
        assert_eq!(
            r[3].start,
            30.0 + 20.0,
            "row 2 (at 80 in the flow) moves to 100"
        );
        c.end_item(2, 10.0, false);
        assert!(!c.stale);
        c.end_item(3, 4.0, false);
        assert!(c.stale);
    }
}
