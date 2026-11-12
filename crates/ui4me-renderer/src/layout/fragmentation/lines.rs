//! owner: 015-core-completion (FRAG-2b, lane L-FRAG) — line struts (CSS Break 3 section 4.1, class B
//! break opportunities between line boxes).
//!
//! [`FragmentingSpace`] wraps the float space of an inline formatting context (`ifc::container::FloatSpace`,
//! `coordinator/floats.rs`) during a `PerformLayout` inside a fragmentation root. The line breaker asks
//! the space for the room of each line: a line that would cross a fragmentainer boundary (and does not
//! already start at the top of a fragmentainer) is given **no room**, and the existing shift-down loop of
//! the breaker moves it to the next "edge", which the space reports at the boundary. The line then starts
//! at the top of the next fragmentainer: that offset is the line's strut. A line taller than a
//! fragmentainer at the top of one keeps its room (it overflows). The IFC itself is not changed.
//!
//! Orphans and widows (one forced-break retry per paragraph) are FRAG-2c.

use super::EPS;
use crate::layout::ifc::container::{FloatBox, FloatSpace};
use crate::layout::ifc::interface::LineSlot;

pub(crate) struct FragmentingSpace<'a, 'b> {
    /// The float space of the inline root, if it has one.
    pub inner: Option<&'a mut (dyn FloatSpace + 'b)>,
    /// The flow-thread offset of the inline root's content-box top.
    pub base: f32,
    /// Fragmentainer block size (with `bands`: the first's).
    pub block_size: f32,
    /// Fragmentainers of different sizes (015 PAGE-1).
    pub bands: super::Bands,
    /// The content-box width (the room of a line without floats).
    pub width: f32,
    /// Boundaries (flow-thread offsets) lines were pushed to.
    pub pushed: Vec<f32>,
    /// The first line (at the content-box top) was pushed: nothing of the content is before the break.
    pub pushed_at_start: bool,
    /// Balancing (MC-1): the tallest line asked for, and the smallest amount a pushed line lacked.
    pub tallest: f32,
    pub shortage: Option<f32>,
    /// Content-box offsets of lines that must start a fragmentainer (widows, FRAG-2c).
    pub forced: Vec<f32>,
    /// What the first line lacked when it was pushed.
    pub first_lack: f32,
    /// Content-box offsets of the lines whose float did not fit below them (FRAG-2c): the float moved to
    /// the next fragmentainer, and the line follows it there in the next pass.
    pub float_lines: Vec<f32>,
}

impl<'a, 'b> FragmentingSpace<'a, 'b> {
    pub(crate) fn new(
        inner: Option<&'a mut (dyn FloatSpace + 'b)>,
        base: f32,
        block_size: f32,
        width: f32,
    ) -> Self {
        FragmentingSpace {
            inner,
            base,
            block_size: block_size.max(EPS),
            bands: super::Bands::default(),
            width,
            pushed: Vec::new(),
            pushed_at_start: false,
            tallest: 0.0,
            shortage: None,
            forced: Vec::new(),
            first_lack: 0.0,
            float_lines: Vec::new(),
        }
    }

    fn start_of(&self, a: f32) -> f32 {
        if self.bands.sizes.is_none() {
            return ((a + EPS) / self.block_size).floor() * self.block_size;
        }
        self.bands.locate(a, self.block_size).1
    }

    /// The size of the fragmentainer holding `a` (015 PAGE-1).
    fn size_at(&self, a: f32) -> f32 {
        self.bands.locate(a, self.block_size).2
    }
}

impl FloatSpace for FragmentingSpace<'_, '_> {
    fn slot(&mut self, y: f32, height: f32) -> LineSlot {
        let mut s = match self.inner.as_deref_mut() {
            Some(i) => i.slot(y, height),
            None => LineSlot {
                inline_start: 0.0,
                inline_size: self.width,
                valid_height: f32::INFINITY,
            },
        };
        let a = self.base + y;
        self.tallest = self.tallest.max(height);
        let start = self.start_of(a);
        if a <= start + EPS {
            // At the top of a fragmentainer: a line never moves further (a taller one overflows).
            return s;
        }
        let end = start + self.size_at(a);
        let forced = self.forced.iter().any(|f| (f - y).abs() <= EPS);
        if forced || a + height > end + EPS {
            // The line would cross the boundary: no room here.
            if !self.pushed.iter().any(|p| (p - end).abs() <= EPS) {
                self.pushed.push(end);
            }
            let lack = a + height - end;
            if y <= EPS {
                self.pushed_at_start = true;
                self.first_lack = lack;
            }
            if lack > EPS {
                self.shortage = Some(self.shortage.map_or(lack, |s| s.min(lack)));
            }
            s.inline_size = 0.0;
            return s;
        }
        // The same tolerance as the crossing test above: a line that does not cross fits.
        s.valid_height = s.valid_height.min(end - a + EPS);
        s
    }

    fn place(&mut self, item: u64, b: FloatBox, min_y: f32) -> (f32, f32) {
        // A monolithic float (FRAG-2c) that would cross a boundary below its line (not at the top of a
        // fragmentainer) moves to the next fragmentainer with that line; a breakable one (015 MC-2) is broken.
        let a = self.base + min_y;
        let start = self.start_of(a);
        let end = start + self.size_at(a);
        let push = !b.breakable && a > start + EPS && a + b.height > end + EPS;
        if push && !self.float_lines.iter().any(|y| (y - min_y).abs() <= EPS) {
            self.float_lines.push(min_y);
        }
        let min_y = if push { end - self.base } else { min_y };
        match self.inner.as_deref_mut() {
            Some(i) => i.place(item, b, min_y),
            None => (0.0, min_y),
        }
    }

    fn next_edge(&mut self, y: f32) -> Option<f32> {
        let a = self.base + y;
        let boundary = self.start_of(a) + self.size_at(a) - self.base;
        let own = (boundary > y + 1e-4).then_some(boundary);
        let inner = self.inner.as_deref_mut().and_then(|i| i.next_edge(y));
        match (own, inner) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn floats_bottom(&self) -> f32 {
        self.inner
            .as_deref()
            .map_or(f32::NEG_INFINITY, |i| i.floats_bottom())
    }

    fn clearance(&mut self, left: bool, right: bool) -> Option<f32> {
        self.inner
            .as_deref_mut()
            .and_then(|i| i.clearance(left, right))
    }
}

impl super::FragContext {
    /// Orphans and widows of the lines of inline root `node` just laid out (CSS Break 3 section 4.4, rule 3):
    /// `lines` are the (top, height) of its lines in its content box, whose top is at flow offset `base`.
    /// Fewer than `orphans` lines before the first break push the paragraph whole to the next
    /// fragmentainer; fewer than `widows` lines after the last break move that break up (the line it
    /// moves to starts the next fragmentainer in the next pass), unless that leaves fewer than `orphans`
    /// before it, when the paragraph is pushed whole if it fits a fragmentainer. Otherwise the rule is
    /// violated (no better break exists).
    ///
    /// Measured in the reference (015 MC-2): the break is not moved up when a float of the paragraph in the
    /// fragmentainer before it reaches below the last line there (`floats`: content-box top and bottom of the
    /// floats' margin boxes); the widows rule is violated instead.
    pub(crate) fn orphans_widows(
        &mut self,
        node: usize,
        base: f32,
        lines: &[(f32, f32)],
        floats: &[(f32, f32)],
        orphans: usize,
        widows: usize,
    ) {
        if lines.len() < 2 {
            return;
        }
        // Runs of consecutive lines in one fragmentainer: (first line, count).
        let mut groups: Vec<(usize, usize, f32)> = Vec::new();
        for (k, (y, _)) in lines.iter().enumerate() {
            let s = self.fragmentainer_start(base + y);
            match groups.last_mut() {
                Some(g) if (g.2 - s).abs() <= EPS => g.1 += 1,
                _ => groups.push((k, 1, s)),
            }
        }
        if groups.len() < 2 {
            return;
        }
        let at_top = self.at_fragmentainer_start(base + lines[0].0);
        let whole = lines[lines.len() - 1].0 + lines[lines.len() - 1].1 - lines[0].0;
        let first_end = self.next_boundary(base + lines[0].0);
        // Balancing's shortage: what the first line that went past the first break lacked where it would have
        // been (right below the line before it), as the reference measures it.
        let c = groups[0].1.min(lines.len() - 1).max(1);
        let lack = base + lines[c - 1].0 + lines[c - 1].1 + lines[c].1 - first_end;
        // `fit`: push only when the whole paragraph then fits a fragmentainer (moving a widows break); a break
        // before the paragraph that keeps orphans is better than an orphans break whatever follows.
        let push_whole = |c: &mut Self, fit: bool| {
            if !at_top && (!fit || whole <= c.size_at(first_end) + EPS) {
                c.leading_pushed.insert(node);
                if c.measure {
                    c.lead_lack.insert(node, lack);
                }
                true
            } else {
                false
            }
        };
        // A break that violates the rule with no better break left: balancing (MC-1) sees how much taller the
        // column would have to be to keep the lines together.
        let lack_orphans = lack;
        if groups[0].1 < orphans {
            if at_top || !push_whole(self, false) {
                self.violated = true;
                self.orphans_violated = true;
                self.note_lack(lack_orphans);
            }
            return;
        }
        let (_, last_count, _) = groups[groups.len() - 1];
        if last_count < widows {
            let need = widows - last_count;
            let (ps, pc, _) = groups[groups.len() - 2];
            // Moving the break up must not leave fewer than `orphans` lines before it (the reference keeps
            // orphans over widows when both cannot hold).
            let keep = orphans;
            let (start, end) = (
                groups[groups.len() - 2].2,
                lines[ps + pc - 1].0 + lines[ps + pc - 1].1,
            );
            let float_below = floats.iter().any(|&(t, b)| {
                let t = base + t;
                t >= start - EPS && t < start + self.size_at(start) - EPS && b > end + EPS
            });
            if pc >= need + keep && !float_below {
                let y = lines[ps + pc - need].0;
                self.force_line(node, base, y);
            } else if float_below || groups.len() > 2 || !push_whole(self, true) {
                self.violated = true;
                let (_, _, prev_start) = groups[groups.len() - 2];
                let last = lines[lines.len() - 1];
                self.note_lack(base + last.0 + last.1 - (prev_start + self.size_at(prev_start)));
            }
        }
    }

    /// The line of inline root `node` at content-box offset `y` (its content box at flow offset `base`)
    /// starts a new fragmentainer in the next pass.
    pub(crate) fn force_line(&mut self, node: usize, base: f32, y: f32) {
        let e = self.line_breaks.entry(node).or_insert((base, Vec::new()));
        if (e.0 - base).abs() > EPS {
            *e = (base, Vec::new());
        }
        if !e.1.iter().any(|x| (x - y).abs() <= EPS) {
            e.1.push(y);
            self.stale = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_crossing_a_boundary_has_no_room_and_the_next_edge_is_the_boundary() {
        let mut s = FragmentingSpace::new(None, 50.0, 100.0, 300.0);
        // Content top at flow offset 50: a 20 px line at y = 40 (flow 90) crosses 100.
        let fits = s.slot(10.0, 20.0);
        assert_eq!(fits.inline_size, 300.0);
        assert_eq!(fits.valid_height, 40.0 + EPS);
        let crossing = s.slot(40.0, 20.0);
        assert_eq!(crossing.inline_size, 0.0);
        assert_eq!(s.next_edge(40.0), Some(50.0));
        // At the boundary the line has its room back, whatever its height.
        let top = s.slot(50.0, 500.0);
        assert_eq!(top.inline_size, 300.0);
        assert!(top.valid_height.is_infinite());
        assert_eq!(s.pushed, vec![100.0]);
    }
}
