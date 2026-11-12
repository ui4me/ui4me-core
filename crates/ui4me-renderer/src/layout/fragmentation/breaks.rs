//! owner: 015-core-completion (FRAG-2b, lane L-FRAG) — breaks between and around block-level boxes
//! (CSS Break 3 sections 3, 4 and 5.2), decided when Taffy's block algorithm has placed a child
//! (`LayoutBlockContainer::end_block_child`, patch P-008-3). The answer is a **strut**: how far the
//! child moves (down to a fragmentainer boundary, or up to one when the break truncates its margin).
//!
//! * A **forced** break (`break-before` of the child, or `break-after` of the previous sibling, or of
//!   the last child of that sibling, which propagates) moves the child's margin box to the next
//!   fragmentainer; the margin after a forced break is kept (section 5.2). A forced break with no
//!   content before it in its fragmentainer is already satisfied (no empty fragmentainer is made).
//! * An **unforced** break that falls inside the margins before the child truncates them: the child
//!   starts at the top of the fragmentainer (section 5.2).
//! * A child that would cross a boundary and may not be split (monolithic content, `break-inside:
//!   avoid` that fits a fragmentainer) is pushed whole to the next fragmentainer, unless it already
//!   starts at the top of one: then it overflows (section 4.4, rule of last resort).
//! * Everything else is split: its own content was laid out with struts already.
//!
//! A strut that moves a child after its content was laid out (forced breaks, avoid, truncation) makes
//! the root stale: the next pass lays the child out at its final offset (`FragContext::next_corrections`).

use super::{BreakAppeal, BreakKind, BreakToken, EPS, FragContext, FragFacts};

/// Whether a `break-before` / `break-after` value forces a break in a column fragmentation context (CSS Break 3
/// section 3.1): `column` and `always`. The page values force page breaks only (paged media, PAGE-1).
pub(crate) fn forces_column_break(v: crate::layout::box_tree::BreakBetween) -> bool {
    matches!(
        v,
        crate::layout::box_tree::BreakBetween::Column
            | crate::layout::box_tree::BreakBetween::Always
    )
}

/// Whether a `break-before` / `break-after` value forces a break: in a page context (015 PAGE-1) `page`, `left`,
/// `right`, `recto`, `verso` and `always`; in a column context [`forces_column_break`].
pub(crate) fn forces_break(v: crate::layout::box_tree::BreakBetween, page: bool) -> bool {
    use crate::layout::box_tree::BreakBetween as B;
    if page {
        matches!(
            v,
            B::Page | B::Left | B::Right | B::Recto | B::Verso | B::Always
        )
    } else {
        forces_column_break(v)
    }
}

/// Whether a `break-before` / `break-after` value avoids a break: `avoid` and `avoid-page` in a page context,
/// `avoid` and `avoid-column` in a column context.
pub(crate) fn avoids_break(v: crate::layout::box_tree::BreakBetween, page: bool) -> bool {
    use crate::layout::box_tree::BreakBetween as B;
    if page {
        matches!(v, B::Avoid | B::AvoidPage)
    } else {
        avoids_column_break(v)
    }
}

/// The page side a forced break asks for (left-to-right page progression: `recto` is right, `verso` left).
pub(crate) fn side_of(v: crate::layout::box_tree::BreakBetween) -> super::PageSide {
    use crate::layout::box_tree::BreakBetween as B;
    match v {
        B::Left | B::Verso => super::PageSide::Left,
        B::Right | B::Recto => super::PageSide::Right,
        _ => super::PageSide::Any,
    }
}

/// Whether a `break-before` / `break-after` value avoids a break in a column fragmentation context.
pub(crate) fn avoids_column_break(v: crate::layout::box_tree::BreakBetween) -> bool {
    matches!(
        v,
        crate::layout::box_tree::BreakBetween::Avoid
            | crate::layout::box_tree::BreakBetween::AvoidColumn
    )
}

/// Where Taffy placed a child (the parent's border-box space; patch P-008-3's `BlockChildPlacement`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    pub y: f32,
    pub height: f32,
    pub margin_top: f32,
    pub committed_y: f32,
    /// No in-flow content precedes the child in its parent.
    pub first: bool,
    /// The child's block-end border and padding (balancing, 015 MC-2: not broken from its last content).
    pub trailing: f32,
    /// The parent's block-start border and padding: with some, a break before its first content leaves a
    /// piece of it before the break (015 MC-2, measured), so the parent is not pushed whole.
    pub parent_lead: f32,
}

impl From<taffy::BlockChildPlacement> for Placement {
    fn from(p: taffy::BlockChildPlacement) -> Self {
        Placement {
            y: p.y,
            height: p.height,
            margin_top: p.margin_top,
            committed_y: p.committed_y,
            first: p.first,
            trailing: 0.0,
            parent_lead: 0.0,
        }
    }
}

impl FragContext {
    /// Taffy placed `child` (see the module docs); returns the strut.
    pub(crate) fn end_child(
        &mut self,
        parent: usize,
        child: usize,
        p: Placement,
        facts: FragFacts,
    ) -> f32 {
        let Some(&pabs) = self.abs.get(&parent) else {
            return 0.0;
        };
        // A `break-after` of the child's last child (not consumed by a sibling) is the child's own.
        let child_pending_after = self.pending_after.remove(&child);
        let parent_pending = self.pending_after.remove(&parent);
        // Pages (015 PAGE-1): boxes whose used page names differ are separated by a forced break (CSS Paged
        // Media 3 section 6.1), as the previous in-flow sibling and this child.
        let name_break = self.paged
            && self
                .last_child
                .get(&parent)
                .is_some_and(|(_, sf, _)| sf.page != facts.page);
        let forced_before = (facts.break_before_forced || parent_pending.is_some() || name_break)
            && !self.in_lead(pabs + p.committed_y);
        // The side a forced break asks for: the child's `break-before`, else the sibling's `break-after`.
        let side = match (facts.break_before_forced, parent_pending) {
            (true, _) if facts.before_side != super::PageSide::Any => facts.before_side,
            (_, Some(s)) if s != super::PageSide::Any => s,
            _ => super::PageSide::Any,
        };
        let y0 = pabs + p.y;
        let committed = pabs + p.committed_y;
        let h = p.height;
        let mut y = y0;
        let mut token: Option<(f32, BreakKind, BreakAppeal)> = None;
        // The unbounded measuring layout of balancing (MC-1) takes no forced break (there is no next
        // fragmentainer); it notes where each would be, for the heights of the segments between them.
        if forced_before && self.unbounded() && committed > EPS {
            self.forced_marks.push(y0 - p.margin_top.max(0.0));
        }
        if forced_before
            && !self.unbounded()
            && committed > EPS
            && !self.at_fragmentainer_start(committed)
        {
            // The margin box starts at the next fragmentainer; the margin itself is kept.
            let mut boundary = self.next_boundary(committed);
            // `left` / `right` (015 PAGE-1): the first page is a right page (left-to-right page progression); a
            // page of the wrong side is left blank.
            if self.paged && side != super::PageSide::Any {
                let k = self.index_at(boundary);
                let right = (k as i64 + self.bands.first_page as i64).rem_euclid(2) == 0;
                if right != (side == super::PageSide::Right) {
                    if !self.blank.contains(&k) {
                        self.blank.push(k);
                    }
                    boundary = self.next_boundary(boundary);
                }
            }
            y = boundary + p.margin_top.max(0.0);
            token = Some((boundary, BreakKind::Forced, BreakAppeal::Perfect));
        } else {
            // Content after a monolithic box that overflows its column starts in the next column (015 STAB-1,
            // measured: the overflow takes no room in the flow; pages slice such a box instead and keep it).
            if !self.paged
                && !self.unbounded()
                && let Some((t, b)) = self.mono_last.get(&parent).copied()
            {
                let boundary = self.next_boundary(t);
                if b > boundary + EPS && y > boundary + EPS {
                    y = boundary;
                    token = Some((
                        boundary,
                        BreakKind::TruncatedMargin,
                        BreakAppeal::LastResort,
                    ));
                }
            }
            // An unforced break inside the margins before the child truncates them.
            let start = self.fragmentainer_start(y);
            if start > EPS && committed <= start + EPS && start < y - EPS && !facts.clears {
                y = start;
                token = Some((start, BreakKind::TruncatedMargin, BreakAppeal::Perfect));
            }
            let boundary = self.next_boundary(y);
            let crosses = y + h > boundary + EPS;
            let fits = h <= self.size_at(boundary) + EPS;
            // A box whose first content went to the next fragmentainer leaves nothing before the break: the
            // break is before the box (a class A break beats an empty piece; the reference browser's choice).
            if self.leading_pushed.remove(&child) && !facts.monolithic {
                self.pushed_whole.insert(child, y);
            }
            let whole = self
                .pushed_whole
                .get(&child)
                .is_some_and(|at| (at - y).abs() <= EPS);
            if self.measure
                && (facts.monolithic
                    || (facts.avoid_inside && !self.avoid_overflow.contains(&child)))
            {
                // Balancing (MC-1): no column may be shorter than an unbreakable unit.
                self.tallest = self.tallest.max(h);
            }
            if (whole || (crosses && (facts.monolithic || facts.avoid_inside)))
                && !self.at_fragmentainer_start(y)
            {
                // Pushed whole (monolithic, `break-inside: avoid`, or no content before the break). An avoid
                // box taller than a fragmentainer still starts a fresh one, then breaks inside.
                // Balancing: what the column lacked to keep the unit (for a box pushed because its first
                // content was, what that content lacked).
                match self.lead_lack.get(&child).copied() {
                    Some(l) if whole && !facts.monolithic => self.note_lack(l),
                    _ if crosses => self.note_shortage(y, h, boundary),
                    _ => {}
                }
                y = boundary;
                token = Some((boundary, BreakKind::PushedBox, BreakAppeal::Perfect));
            }
            if self.measure
                && crosses
                && !facts.monolithic
                && p.trailing > EPS
                && y + h - p.trailing <= boundary + EPS
                && token.is_none()
            {
                // Balancing (015 MC-2, measured): only the box's block-end border and padding would cross the
                // boundary; they are not broken from its last content, so the column is too short by that much.
                self.orphans_violated = true;
                self.note_lack(y + h - boundary);
            }
            if crosses
                && facts.avoid_inside
                && !facts.monolithic
                && !fits
                && !self.avoid_overflow.contains(&child)
            {
                // Taller than a fragmentainer: it is broken after all (laid out fragmented next pass).
                self.avoid_overflow.insert(child);
                self.stale = true;
            }
        }
        // A break right before the child that `break-after: avoid` of the previous sibling or the child's own
        // `break-before: avoid` forbids (CSS Break 3 section 4.4, rule 2): backtrack to the break before the
        // previous sibling, which is pushed whole in the next pass. Not when the sibling already starts a
        // fragmentainer (no better break exists there: the avoid is violated).
        let prev = self.last_child.insert(parent, (child, facts, y));
        // A break that falls exactly between the two (the child starts a fragmentainer on its own) and that
        // they avoid: balancing (MC-1) takes a taller column (the child would fit beside its sibling); with a
        // fixed fragmentainer the break moves before the sibling as below.
        if token.is_none()
            && y > EPS
            && self.at_fragmentainer_start(y)
            && let Some((sib, sf, sy)) = prev
            && (sf.break_after_avoid || facts.break_before_avoid)
            && sy < self.fragmentainer_start(y) - EPS
        {
            if self.measure {
                self.orphans_violated = true;
                self.note_lack(h.max(EPS));
            } else if !self.at_fragmentainer_start(sy)
                && self
                    .pushed_whole
                    .insert(sib, sy)
                    .is_none_or(|old| (old - sy).abs() > EPS)
            {
                self.stale = true;
            }
        }
        if let Some((_, BreakKind::PushedBox | BreakKind::TruncatedMargin, _)) = token
            && let Some((sib, sf, sy)) = prev
            && (sf.break_after_avoid || facts.break_before_avoid)
            && !self.at_fragmentainer_start(sy)
            && self
                .pushed_whole
                .insert(sib, sy)
                .is_none_or(|old| (old - sy).abs() > EPS)
        {
            self.stale = true;
            if self.measure {
                // Balancing: what the child lacked is what keeping the pair together lacks.
                let b0 = self.next_boundary(y0);
                let lack = self
                    .lead_lack
                    .get(&child)
                    .copied()
                    .filter(|_| !facts.monolithic)
                    .unwrap_or(y0 + h - b0);
                self.lead_lack.insert(sib, lack);
            }
        }
        if self.measure && p.first {
            // Balancing (015 MC-2): the parent's leading unit runs through this first child's.
            let u = if facts.monolithic {
                Some(h)
            } else {
                self.first_unit.get(&child).copied()
            };
            if let Some(u) = u {
                let unit = p.y + u;
                self.first_unit.insert(parent, unit);
                self.tallest = self.tallest.max(unit);
            }
        }
        let strut = y - y0;
        if p.first && strut > EPS {
            // The parent's first content moved past a break.
            self.leading_pushed.insert(parent);
            if self.measure {
                let b0 = self.next_boundary(y0);
                let lack = self
                    .lead_lack
                    .get(&child)
                    .copied()
                    .filter(|_| !facts.monolithic)
                    .unwrap_or(y0 + h - b0);
                self.lead_lack.insert(parent, lack);
            }
        }
        if let Some((offset, kind, appeal)) = token {
            self.tokens.push(BreakToken {
                node: child,
                offset,
                kind,
                appeal,
            });
        }
        // The offset the child's content was laid out at, against where it ends up.
        let est = self.est.remove(&child);
        let used = self.abs.get(&child).copied();
        if let (Some(est), Some(used)) = (est, used)
            && !facts.monolithic
            && (used - y).abs() > EPS
        {
            self.next_corrections.insert(child, (est, y - pabs));
            self.stale = true;
        }
        self.abs.insert(child, y);
        if facts.monolithic {
            self.mono_last.insert(parent, (y, y + h));
        } else {
            self.mono_last.remove(&parent);
        }
        if self.paged {
            self.starts.push((y, facts.page, child));
        }
        if facts.break_after_forced || child_pending_after.is_some() {
            let side = if facts.break_after_forced && facts.after_side != super::PageSide::Any {
                facts.after_side
            } else {
                child_pending_after.unwrap_or_default()
            };
            self.pending_after.insert(parent, side);
        }
        strut
    }
}

#[cfg(test)]
mod tests {
    use super::super::FragContext;
    use super::*;

    fn ctx() -> FragContext {
        FragContext::new(0, 100.0, super::super::Carry::default())
    }

    fn place(
        c: &mut FragContext,
        child: usize,
        y: f32,
        h: f32,
        committed: f32,
        facts: FragFacts,
    ) -> f32 {
        c.begin_child(0, child, y);
        c.end_child(
            0,
            child,
            Placement {
                y,
                height: h,
                margin_top: 0.0,
                committed_y: committed,
                first: false,
                trailing: 0.0,
                parent_lead: 0.0,
            },
            facts,
        )
    }

    #[test]
    fn a_monolithic_box_crossing_a_boundary_is_pushed() {
        let mut c = ctx();
        let mono = FragFacts {
            monolithic: true,
            ..Default::default()
        };
        assert_eq!(place(&mut c, 1, 80.0, 40.0, 80.0, mono), 20.0);
        // At the top of a fragmentainer it overflows instead.
        assert_eq!(place(&mut c, 2, 100.0, 140.0, 100.0, mono), 0.0);
        assert_eq!(c.tokens.len(), 1);
        assert_eq!(c.tokens[0].kind, BreakKind::PushedBox);
    }

    #[test]
    fn a_splittable_box_is_not_moved() {
        let mut c = ctx();
        assert_eq!(
            place(&mut c, 1, 80.0, 40.0, 80.0, FragFacts::default()),
            0.0
        );
        assert!(c.tokens.is_empty());
    }

    #[test]
    fn forced_breaks_keep_the_margin_and_skip_an_empty_fragmentainer() {
        let mut c = ctx();
        let forced = FragFacts {
            break_before_forced: true,
            ..Default::default()
        };
        c.begin_child(0, 1, 30.0);
        let s = c.end_child(
            0,
            1,
            Placement {
                y: 30.0,
                height: 10.0,
                margin_top: 10.0,
                committed_y: 20.0,
                first: false,
                trailing: 0.0,
                parent_lead: 0.0,
            },
            forced,
        );
        assert_eq!(s, 80.0, "margin box at 100, border box at 110");
        // Content ending exactly at a boundary: the break is already there.
        assert_eq!(place(&mut c, 2, 200.0, 10.0, 200.0, forced), 0.0);
        // No content before it at all: ignored.
        let mut c = ctx();
        assert_eq!(place(&mut c, 1, 0.0, 10.0, 0.0, forced), 0.0);
    }

    #[test]
    fn break_after_applies_to_the_next_sibling() {
        let mut c = ctx();
        let after = FragFacts {
            break_after_forced: true,
            ..Default::default()
        };
        assert_eq!(place(&mut c, 1, 0.0, 10.0, 0.0, after), 0.0);
        assert_eq!(
            place(&mut c, 2, 10.0, 10.0, 10.0, FragFacts::default()),
            90.0
        );
    }

    #[test]
    fn an_unforced_break_truncates_the_margin() {
        let mut c = ctx();
        // Content ends at 90; a 30 px margin puts the child at 120: it starts at 100 instead.
        assert_eq!(
            place(&mut c, 1, 120.0, 10.0, 90.0, FragFacts::default()),
            -20.0
        );
        assert_eq!(c.tokens[0].kind, BreakKind::TruncatedMargin);
    }

    #[test]
    fn avoid_pushes_when_it_fits_and_breaks_when_it_does_not() {
        let mut c = ctx();
        let avoid = FragFacts {
            avoid_inside: true,
            ..Default::default()
        };
        assert_eq!(place(&mut c, 1, 50.0, 60.0, 50.0, avoid), 50.0);
        assert!(
            c.stale,
            "the pushed box is laid out again at its new offset"
        );
        // Too tall for one: it starts a fresh one and is broken there.
        let mut c = ctx();
        assert_eq!(place(&mut c, 1, 50.0, 160.0, 50.0, avoid), 50.0);
        assert!(c.avoid_overflow.contains(&1));
    }
}
