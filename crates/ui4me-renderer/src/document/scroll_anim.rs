//! Smooth scrolling, scroll snapping and overscroll policy of a document (0.8 F0803, F0804).
//!
//! * One animation per scroll container (a new request replaces the old one: no pile-up). The
//!   owner clock drives it: [`Document::tick_scroll`] runs from `poll_resources` (every host
//!   loop turn and before each frame) and from `get_scroll`; `next_wake_in` asks the host for a
//!   wake-up every 16 ms while any animation runs. The last step lands exactly on the target.
//! * The clock is monotonic real time, or a manual clock a test advances
//!   ([`Document::use_manual_scroll_clock`], [`Document::advance_scroll_clock`]).
//! * A user scroll (wheel, keys, scrollbar drag or paging) of a container cancels its animation.
//!   `prefers-reduced-motion: reduce` (the app environment) makes every smooth scroll instant.
//! * Snapping follows CSS Scroll Snap 1 for both axes: `mandatory` always picks a snap position,
//!   `proximity` only within a third of the snapport; `scroll-padding` shrinks the snapport and
//!   `scroll-margin` grows the snap area; `scroll-snap-stop: always` cannot be skipped. Wheel,
//!   keys and `scrollBy` snap in the direction of the scroll; `scrollTo` and a released scrollbar
//!   thumb snap to the nearest position. Not done: vertical writing modes (`block`/`inline` are
//!   taken as vertical/horizontal), `calc()` in `scroll-padding`, snapping after `scrollIntoView`
//!   and after follow-end adjustments.

use super::*;
use crate::style::view::scroll_props::{ScrollProps, SnapAlign, SnapStrictness};

const EPS: f64 = 0.01;
const MAX_ANIMS: usize = 64;
const MAX_SNAP_NODES: usize = 20_000;

#[derive(Clone, Copy, Debug)]
pub(super) struct ScrollAnim {
    target: Option<NodeId>,
    from: (f64, f64),
    to: (f64, f64),
    start: f64,
    dur: f64,
}

/// The smooth scrolls in progress and the clock that drives them.
#[derive(Debug, Default)]
pub(super) struct ScrollAnims {
    pub(super) list: Vec<ScrollAnim>,
}

impl ScrollAnims {
    pub(super) fn active(&self) -> bool {
        !self.list.is_empty()
    }
    pub(super) fn running(&self, target: Option<NodeId>) -> bool {
        self.list.iter().any(|a| a.target == target)
    }
    pub(super) fn cancel(&mut self, target: Option<NodeId>) {
        self.list.retain(|a| a.target != target);
    }
    fn start(&mut self, a: ScrollAnim) {
        self.cancel(a.target);
        if self.list.len() >= MAX_ANIMS {
            self.list.remove(0);
        }
        self.list.push(a);
    }
}

fn clamp2(v: (f64, f64), min: (f64, f64), max: (f64, f64)) -> (f64, f64) {
    (v.0.clamp(min.0, max.0), v.1.clamp(min.1, max.1))
}

/// The snap offset of one area on one axis: its edges (margin box) against the snapport.
fn snap_pos(
    align: SnapAlign,
    start: f64,
    end: f64,
    port: f64,
    pad_start: f64,
    pad_end: f64,
) -> Option<f64> {
    match align {
        SnapAlign::None => None,
        SnapAlign::Start => Some(start - pad_start),
        SnapAlign::End => Some(end - (port - pad_end)),
        SnapAlign::Center => {
            Some((start + end) / 2.0 - (pad_start + (port - pad_start - pad_end) / 2.0))
        }
    }
}

/// Pick the offset on one axis. `cands` are `(position, scroll-snap-stop: always)`; `cur` is the
/// offset the scroll starts from and `want` the offset it would end at. `directional` limits the
/// choice to positions in the direction of the scroll (wheel, keys, `scrollBy`).
#[allow(clippy::too_many_arguments)]
pub(super) fn choose(
    cands: &[(f64, bool)],
    cur: f64,
    want: f64,
    directional: bool,
    mandatory: bool,
    port: f64,
    min: f64,
    max: f64,
) -> f64 {
    let want = want.clamp(min, max);
    if cands.is_empty() {
        return want;
    }
    let cs: Vec<(f64, bool)> = cands.iter().map(|&(p, s)| (p.clamp(min, max), s)).collect();
    let moving = (want - cur).abs() > EPS;
    let dir = if want > cur { 1.0 } else { -1.0 };
    if moving {
        // A position with `scroll-snap-stop: always` between the start and the end stops the scroll.
        let stop = cs
            .iter()
            .filter(|(p, s)| *s && (p - cur) * dir > EPS && (want - p) * dir > EPS)
            .map(|(p, _)| *p)
            .min_by(|a, b| (a - cur).abs().total_cmp(&(b - cur).abs()));
        if let Some(p) = stop {
            return p;
        }
    }
    let nearest = |pool: &mut dyn Iterator<Item = f64>| {
        pool.min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()))
    };
    let mut best = None;
    if directional && moving {
        best = nearest(&mut cs.iter().map(|c| c.0).filter(|p| (p - cur) * dir > EPS));
    }
    let best = best.or_else(|| nearest(&mut cs.iter().map(|c| c.0)));
    match best {
        Some(b) if mandatory || (b - want).abs() <= port / 3.0 => b,
        _ => want,
    }
}

impl Document {
    fn scroll_now(&self) -> f64 {
        self.clock.now()
    }
    /// Switch the document clock (smooth scroll and CSS animations) to a manual one that only
    /// [`Self::advance_clock`] moves (tests and fixtures).
    pub fn use_manual_clock(&mut self) {
        self.clock.use_manual();
    }
    /// Advance the manual document clock by `ms` and run the scroll and CSS animations. Returns
    /// whether any scroll offset or animated value changed.
    pub fn advance_clock(&mut self, ms: f64) -> Result<bool> {
        self.clock.advance(ms);
        let scrolled = self.tick_scroll()?;
        let animated = self.tick_animations(true);
        Ok(scrolled || animated)
    }
    /// Is a smooth scroll of `element` (or the viewport for `None`) running?
    pub fn is_scroll_animating(&self, element: Option<&str>) -> bool {
        match element {
            None => self.scroll_anims.running(None),
            Some(e) => self
                .node(e)
                .is_ok_and(|n| self.scroll_anims.running(Some(n))),
        }
    }

    pub(super) fn props_of(&self, target: Option<NodeId>) -> ScrollProps {
        let d = self.pipeline.document();
        let Some(n) = target.or_else(|| d.document_element()) else {
            return ScrollProps::default();
        };
        self.pipeline
            .committed
            .as_deref()
            .and_then(|c| c.style.view(n))
            .map(|v| v.scroll_props())
            .unwrap_or_default()
    }
    pub(super) fn scroll_info_of(
        &self,
        e: &crate::pipeline::scroll::ScrollEntry,
    ) -> ScrollTargetInfo {
        let mut info = scroll_info(e);
        let p = self.props_of(info.target);
        info.contain_x = p.contain_x;
        info.contain_y = p.contain_y;
        info
    }
    fn smooth_for(&self, behavior: &str, target: Option<NodeId>) -> Result<bool> {
        let smooth = match behavior {
            "instant" => false,
            "smooth" => true,
            "auto" => self.props_of(target).smooth,
            _ => return Err(arg("behavior must be auto, instant or smooth")),
        };
        Ok(smooth && !self.effective_reduced_motion())
    }

    /// Run the animations at the clock's time; each moves its container, `scroll` fires per step
    /// and `scrollend` after the last. Dead targets are dropped.
    pub(super) fn tick_scroll(&mut self) -> Result<bool> {
        if self.scroll_anims.list.is_empty() {
            return Ok(false);
        }
        let now = self.scroll_now();
        let anims = std::mem::take(&mut self.scroll_anims.list);
        let id = self.id();
        let mut changed = false;
        let mut keep = Vec::new();
        for a in anims {
            if let Some(n) = a.target {
                let d = self.pipeline.document();
                if !(d.arena().contains(n) && d.is_connected(n).unwrap_or(false)) {
                    continue;
                }
            }
            let t = ((now - a.start) / a.dur).clamp(0.0, 1.0);
            let done = t >= 1.0;
            let k = 1.0 - (1.0 - t).powi(3);
            let (x, y) = if done {
                a.to
            } else {
                (
                    a.from.0 + (a.to.0 - a.from.0) * k,
                    a.from.1 + (a.to.1 - a.from.1) * k,
                )
            };
            let applied = crate::interaction::scroll::scroll_step(self, id, a.target, x, y, done)?;
            changed |= applied.changed;
            if !done {
                keep.push(a);
            }
        }
        for a in keep {
            if !self.scroll_anims.running(a.target) {
                self.scroll_anims.list.push(a);
            }
        }
        if changed {
            self.update_follow();
            self.repaint = true;
        }
        Ok(changed)
    }

    fn start_anim(&mut self, target: Option<NodeId>, from: (f64, f64), to: (f64, f64)) {
        let dist = (to.0 - from.0).hypot(to.1 - from.1);
        let start = self.scroll_now();
        self.scroll_anims.start(ScrollAnim {
            target,
            from,
            to,
            start,
            dur: (200.0 + dist * 0.2).clamp(200.0, 400.0),
        });
    }

    /// Move `target` to `to` (already clamped): at once or as a smooth scroll that replaces any
    /// running one of the container.
    pub(super) fn begin_scroll(
        &mut self,
        target: Option<NodeId>,
        to: (f64, f64),
        smooth: bool,
    ) -> Result<()> {
        let id = self.id();
        let cur = self
            .pipeline
            .scroll_entry(scroll_target(target))
            .map_or((0.0, 0.0), |e| e.offset);
        if !smooth || (to.0 - cur.0).abs() + (to.1 - cur.1).abs() < EPS {
            self.scroll_anims.cancel(target);
            crate::interaction::scroll::scroll_to(self, id, target, to.0, to.1)?;
        } else {
            self.start_anim(target, cur, to);
        }
        Ok(())
    }

    fn scroll_request(
        &mut self,
        element: Option<&str>,
        x: Option<f64>,
        y: Option<f64>,
        behavior: &str,
        directional: bool,
    ) -> Result<ScrollState> {
        let current = self.scroll_state(element)?;
        let target = self.scroll_node(element)?;
        let smooth = self.smooth_for(behavior, target)?;
        if !current.scrollable {
            return Ok(current);
        }
        let want = (x.unwrap_or(current.x), y.unwrap_or(current.y));
        let to = self.snap_target(target, (current.x, current.y), want, directional, false);
        self.begin_scroll(target, to, smooth)?;
        self.update_follow();
        self.pipeline.flush()?;
        Ok(self.state_of(target))
    }

    /// `scrollTo`: instant, or smooth per `behavior` (`auto` | `instant` | `smooth`); an omitted
    /// axis keeps its offset. Snaps per `scroll-snap-type`. Emits `scroll` when the offset
    /// changes. Updates the follow-end state of the target.
    pub fn scroll_to_behavior(
        &mut self,
        element: Option<&str>,
        x: Option<f64>,
        y: Option<f64>,
        behavior: &str,
    ) -> Result<ScrollState> {
        if x.is_some_and(|v| !v.is_finite()) || y.is_some_and(|v| !v.is_finite()) {
            return Err(arg("Scroll offsets must be finite"));
        }
        self.scroll_request(element, x, y, behavior, false)
    }
    /// `scrollBy`: relative to the current (possibly animating) offset, snapping in the
    /// direction of the scroll.
    pub fn scroll_by_behavior(
        &mut self,
        element: Option<&str>,
        dx: f64,
        dy: f64,
        behavior: &str,
    ) -> Result<ScrollState> {
        if !dx.is_finite() || !dy.is_finite() {
            return Err(arg("Scroll deltas must be finite"));
        }
        let current = self.scroll_state(element)?;
        self.scroll_request(
            element,
            Some(current.x + dx),
            Some(current.y + dy),
            behavior,
            true,
        )
    }
    /// Scroll every scroll container of `element`, innermost first, so the element is visible.
    /// `block`/`inline` are `start`, `center`, `end` or `nearest` (CSSOM View).
    pub fn scroll_into_view_behavior(
        &mut self,
        element: &str,
        block: &str,
        inline: &str,
        behavior: &str,
    ) -> Result<()> {
        let n = self.node(element)?;
        self.scroll_node_into_view(n, block, inline, behavior)
    }

    /// The same scroll operation for assistive and generation-scoped anonymous DOM targets.
    pub(super) fn scroll_node_into_view(
        &mut self,
        n: NodeId,
        block: &str,
        inline: &str,
        behavior: &str,
    ) -> Result<()> {
        let modes = ["start", "center", "end", "nearest"];
        if !modes.contains(&block) || !modes.contains(&inline) {
            return Err(arg(
                "block and inline must be start, center, end or nearest",
            ));
        }
        if !["auto", "instant", "smooth"].contains(&behavior) {
            return Err(arg("behavior must be auto, instant or smooth"));
        }
        self.tick_scroll()?;
        self.apply_follow()?;
        self.pipeline.flush()?;
        let chain: Vec<ScrollTarget> = self
            .pipeline
            .scroll_chain(n)
            .into_iter()
            .map(|e| e.target)
            .filter(|&t| t != ScrollTarget::Node(n))
            .collect();
        // Containers that scroll smoothly have not moved yet; the outer ones align against the
        // rectangle as it will be once the inner ones arrive.
        let mut pending = (0.0, 0.0);
        for target in chain {
            self.pipeline.flush()?;
            let Some(r) = self.viewport_rect(n) else {
                return Ok(());
            };
            let Some(port) = self.scrollport_rect(target) else {
                continue;
            };
            let Some(e) = self.pipeline.scroll_entry(target) else {
                continue;
            };
            let dx = align(inline, r.0 - pending.0, r.2, port.0, port.2);
            let dy = align(block, r.1 - pending.1, r.3, port.1, port.3);
            if dx != 0.0 || dy != 0.0 {
                let t = match target {
                    ScrollTarget::Viewport => None,
                    ScrollTarget::Node(c) => Some(c),
                };
                let smooth = self.smooth_for(behavior, t)?;
                let to = clamp2((e.offset.0 + dx, e.offset.1 + dy), e.range.min, e.range.max);
                self.begin_scroll(t, to, smooth)?;
                if self.scroll_anims.running(t) {
                    pending.0 += to.0 - e.offset.0;
                    pending.1 += to.1 - e.offset.1;
                }
            }
        }
        self.update_follow();
        Ok(())
    }

    /// A wheel, key or paging scroll of `target` by `(dx, dy)`: cancels the container's smooth
    /// scroll and snaps in the direction of the scroll. When snapping moves the end position
    /// and the container scrolls smoothly, the move is animated.
    pub(super) fn scroll_user_move(
        &mut self,
        target: Option<NodeId>,
        dx: f64,
        dy: f64,
    ) -> std::result::Result<ScrollApplied, Status> {
        self.scroll_anims.cancel(target);
        self.pipeline.flush()?;
        let Some(e) = self.pipeline.scroll_entry(scroll_target(target)) else {
            return Ok(ScrollApplied {
                offset: (0.0, 0.0),
                changed: false,
                clamped: false,
            });
        };
        let want = (e.offset.0 + dx, e.offset.1 + dy);
        let props = self.props_of(target);
        let id = self.id();
        if props.snap_x || props.snap_y {
            let plain = clamp2(want, e.range.min, e.range.max);
            let to = self.snap_target(target, e.offset, want, true, false);
            let moved = (to.0 - plain.0).abs() + (to.1 - plain.1).abs() > EPS;
            if moved && props.smooth && !self.effective_reduced_motion() {
                self.start_anim(target, e.offset, to);
                return Ok(ScrollApplied {
                    offset: e.offset,
                    changed: false,
                    clamped: false,
                });
            }
            return DispatchHost::scroll_set(self, id, target, to.0, to.1);
        }
        DispatchHost::scroll_set(self, id, target, want.0, want.1)
    }

    /// The scrollbar thumb was released: cancel the animation and settle on the nearest snap
    /// position. `true` when the offset changed at once.
    pub(super) fn scroll_user_settle_at(
        &mut self,
        target: Option<NodeId>,
    ) -> std::result::Result<bool, Status> {
        self.scroll_anims.cancel(target);
        self.pipeline.flush()?;
        let Some(e) = self.pipeline.scroll_entry(scroll_target(target)) else {
            return Ok(false);
        };
        let props = self.props_of(target);
        if !(props.snap_x || props.snap_y) {
            return Ok(false);
        }
        let to = self.snap_target(target, e.offset, e.offset, false, true);
        if (to.0 - e.offset.0).abs() + (to.1 - e.offset.1).abs() < EPS {
            return Ok(false);
        }
        if props.smooth && !self.effective_reduced_motion() {
            self.start_anim(target, e.offset, to);
            return Ok(false);
        }
        let id = self.id();
        Ok(DispatchHost::scroll_set(self, id, target, to.0, to.1)?.changed)
    }

    /// The elements whose `scroll-snap-align` makes them snap areas of `target`: descendants
    /// whose nearest scroll container is `target`.
    fn snap_areas(&self, target: Option<NodeId>) -> Vec<(NodeId, ScrollProps)> {
        let d = self.pipeline.document();
        let Some(root) = target.or_else(|| d.document_element()) else {
            return Vec::new();
        };
        let Some(c) = self.pipeline.committed.as_deref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = d.children(root).map(<[_]>::to_vec).unwrap_or_default();
        let mut seen = 0;
        while let Some(n) = stack.pop() {
            seen += 1;
            if seen > MAX_SNAP_NODES {
                break;
            }
            let Some(v) = c.style.view(n) else {
                continue;
            };
            let sp = v.scroll_props();
            if sp.align_x != SnapAlign::None || sp.align_y != SnapAlign::None {
                out.push((n, sp));
            }
            // Areas of a nested scroll container belong to it.
            if self.pipeline.scroll_entry(ScrollTarget::Node(n)).is_none()
                && let Ok(ch) = d.children(n)
            {
                stack.extend_from_slice(ch);
            }
        }
        out
    }

    /// Where a scroll of `target` from `cur` towards `want` ends once snapped (clamped `want`
    /// when the container does not snap). An axis whose requested offset equals `cur` is left
    /// alone unless `force`.
    fn snap_target(
        &mut self,
        target: Option<NodeId>,
        cur: (f64, f64),
        want: (f64, f64),
        directional: bool,
        force: bool,
    ) -> (f64, f64) {
        let Some(e) = self.pipeline.scroll_entry(scroll_target(target)) else {
            return want;
        };
        let plain = clamp2(want, e.range.min, e.range.max);
        let props = self.props_of(target);
        if !(props.snap_x || props.snap_y) {
            return plain;
        }
        let Some(port) = self.scrollport_rect(scroll_target(target)) else {
            return plain;
        };
        let (pw, ph) = (port.2, port.3);
        let pad = |i: usize, len: f64| props.padding[i].resolve(len);
        let (pad_t, pad_r, pad_b, pad_l) = (pad(0, ph), pad(1, pw), pad(2, ph), pad(3, pw));
        let mut xs: Vec<(f64, bool)> = Vec::new();
        let mut ys: Vec<(f64, bool)> = Vec::new();
        for (n, a) in self.snap_areas(target) {
            let Some(r) = self.viewport_rect(n) else {
                continue;
            };
            let cx = r.0 - port.0 + e.offset.0;
            let cy = r.1 - port.1 + e.offset.1;
            if props.snap_x
                && let Some(p) = snap_pos(
                    a.align_x,
                    cx - a.margin[3],
                    cx + r.2 + a.margin[1],
                    pw,
                    pad_l,
                    pad_r,
                )
            {
                xs.push((p, a.stop_always));
            }
            if props.snap_y
                && let Some(p) = snap_pos(
                    a.align_y,
                    cy - a.margin[0],
                    cy + r.3 + a.margin[2],
                    ph,
                    pad_t,
                    pad_b,
                )
            {
                ys.push((p, a.stop_always));
            }
        }
        let mandatory = props.strictness == SnapStrictness::Mandatory;
        let mut out = plain;
        if props.snap_x && (force || (want.0 - cur.0).abs() > EPS) {
            out.0 = choose(
                &xs,
                cur.0,
                want.0,
                directional,
                mandatory,
                pw,
                e.range.min.0,
                e.range.max.0,
            );
        }
        if props.snap_y && (force || (want.1 - cur.1).abs() > EPS) {
            out.1 = choose(
                &ys,
                cur.1,
                want.1,
                directional,
                mandatory,
                ph,
                e.range.min.1,
                e.range.max.1,
            );
        }
        out
    }
}
