//! owner: 012-interaction (lane L-INT-r3: US7 essentials T100-T103 subset; FR-084-FR-088)
//!
//! Wheel, keyboard and API scrolling over the host's scroll state. The offsets themselves live
//! with the pipeline (they are an input of the scene stage, see `DispatchHost::scroll_set`), the
//! clamping is layout's `ScrollOffsets` (008 E008-3); this module owns the policy: which
//! container scrolls (the scroll chain), the wheel/keyboard defaults and the `scroll` /
//! `scrollend` events.
//!
//! Not done (012 T100-T104 stay open): the wheel transaction target and 500 ms timeout,
//! `scrollend` after a wheel (there is no transaction end yet), passive-by-default wheel
//! listeners, scrollbar parts (hit routing, drag, paging, visibility), `scrollIntoView`, the
//! focus-scrolls-into-view rule. Smooth scrolling, snapping and overscroll containment (0.8
//! F0803/F0804) are policy of the host (`scroll_user_by`, `scroll_user_settle`, `scroll_cancel`) and of
//! `Document` (`document/scroll_anim.rs`).

use crate::interaction::dispatch::{DispatchHost, eng, operation, rt, run_dispatch};
use crate::interaction::frame::{EventInit, EventInterface, Modifiers};
use crate::interaction::pointer::{hit_element, update_hover};
use ui4me_engine_types::diag::DispatchOrigin;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::input::PointerData;
use ui4me_engine_types::status::Status;

/// CSS px per wheel line is the platform's (011, 40 px); keyboard arrow step (FR-087).
pub const ARROW_STEP_PX: f64 = 40.0;
/// Page steps scroll this fraction of the scrollport (FR-087).
pub const PAGE_FRACTION: f64 = 0.875;

/// Scrollbar hit geometry expressed in its transformed local track coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarHit {
    pub node: NodeId,
    pub viewport: bool,
    pub vertical: bool,
    pub local: f64,
    pub track_length: f64,
    pub thumb_start: f64,
    pub thumb_length: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollbarDrag {
    node: NodeId,
    viewport: bool,
    vertical: bool,
    pointer_id: u32,
    local_start: f64,
    offset_start: f64,
    scale: f64,
    moved: bool,
}

pub(crate) fn scrollbar_down(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
) -> Result<bool, Status> {
    if p.button != 0 || !p.is_primary {
        return Ok(false);
    }
    let Some(hit) = host.scrollbar_hit(doc, p.x, p.y)? else {
        return Ok(false);
    };
    let target = if hit.viewport { None } else { Some(hit.node) };
    let Some(info) = host.scroll_info(doc, target)? else {
        return Ok(false);
    };
    let (offset, min, max, port) = if hit.vertical {
        (info.offset.1, info.min.1, info.max.1, info.scrollport.1)
    } else {
        (info.offset.0, info.min.0, info.max.0, info.scrollport.0)
    };
    if hit.local >= hit.thumb_start && hit.local < hit.thumb_start + hit.thumb_length {
        let travel = hit.track_length - hit.thumb_length;
        if travel > 0.0 {
            let drag = ScrollbarDrag {
                node: hit.node,
                viewport: hit.viewport,
                vertical: hit.vertical,
                pointer_id: p.pointer_id,
                local_start: hit.local,
                offset_start: offset,
                scale: (max - min) / travel,
                moved: false,
            };
            host.scroll_cancel(doc, target)?;
            eng(host, doc, |ix, _| {
                if let Some(s) = ix.docs.get_mut(&doc) {
                    s.scrollbar_drag = Some(drag);
                }
            })?;
        }
    } else {
        let delta = if hit.local < hit.thumb_start {
            -port * PAGE_FRACTION
        } else {
            port * PAGE_FRACTION
        };
        let (dx, dy) = if hit.vertical {
            (0.0, delta)
        } else {
            (delta, 0.0)
        };
        scroll_container_user(host, doc, &info, dx, dy, true)?;
        // Remember a track press as well, so its release does not become a DOM mouseup/click.
        let drag = ScrollbarDrag {
            node: hit.node,
            viewport: hit.viewport,
            vertical: hit.vertical,
            pointer_id: p.pointer_id,
            local_start: hit.local,
            offset_start: offset,
            scale: 0.0,
            moved: false,
        };
        eng(host, doc, |ix, _| {
            if let Some(s) = ix.docs.get_mut(&doc) {
                s.scrollbar_drag = Some(drag);
            }
        })?;
    }
    Ok(true)
}

pub(crate) fn scrollbar_move(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    p: &PointerData,
    end: bool,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<bool, Status> {
    let drag = eng(host, doc, |ix, _| {
        ix.docs.get(&doc).and_then(|s| s.scrollbar_drag)
    })?;
    let Some(drag) = drag.filter(|d| d.pointer_id == p.pointer_id) else {
        return Ok(false);
    };
    let mut changed = false;
    if drag.scale > 0.0
        && let Some(local) = host.scrollbar_local(doc, drag.node, drag.vertical, p.x, p.y)?
        && let Some(info) =
            host.scroll_info(doc, if drag.viewport { None } else { Some(drag.node) })?
    {
        let value = drag.offset_start + (local - drag.local_start) * drag.scale;
        let (x, y) = if drag.vertical {
            (info.offset.0, value)
        } else {
            (value, info.offset.1)
        };
        let target = if drag.viewport { None } else { Some(drag.node) };
        host.scroll_cancel(doc, target)?;
        let applied = host.scroll_set(doc, target, x, y)?;
        changed = applied.changed;
        if changed {
            rt(host, |ix| ix.note_scrolled(doc, target, end));
            eng(host, doc, |ix, _| {
                if let Some(d) = ix
                    .docs
                    .get_mut(&doc)
                    .and_then(|s| s.scrollbar_drag.as_mut())
                {
                    d.moved = true;
                }
            })?;
        }
    }
    if end {
        eng(host, doc, |ix, _| {
            if let Some(s) = ix.docs.get_mut(&doc) {
                s.scrollbar_drag = None;
            }
        })?;
        // Releasing the thumb settles on a snap position (instantly or smoothly).
        if drag.moved || changed {
            let target = if drag.viewport { None } else { Some(drag.node) };
            if host.scroll_user_settle(doc, target)? {
                rt(host, |ix| ix.note_scrolled(doc, target, true));
                changed = true;
            }
        }
        if drag.moved && !changed {
            let target = if drag.viewport {
                eng(host, doc, |_, d| d.document_node())?
            } else {
                drag.node
            };
            run_dispatch(
                host,
                doc,
                target,
                EventInit::new("scrollend").bubbles(drag.viewport),
                true,
                origin,
                ts,
            )?;
        }
    }
    Ok(true)
}

/// One scroll target as the host reports it. `target: None` is the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollTargetInfo {
    pub target: Option<NodeId>,
    /// Current offset (CSS px, CSSOM View `scrollLeft`/`scrollTop`).
    pub offset: (f64, f64),
    /// Reachable range per axis.
    pub min: (f64, f64),
    pub max: (f64, f64),
    /// Scrollport size (CSS px), for paging.
    pub scrollport: (f64, f64),
    /// The user can scroll the axis (`overflow` is `auto` or `scroll`, or the viewport is not
    /// `hidden`); programmatic scrolling is allowed on `hidden` containers.
    pub user_x: bool,
    pub user_y: bool,
    /// `overscroll-behavior` is `contain` or `none` on the axis: user scrolling does not chain
    /// to the ancestors (0.8 F0804).
    pub contain_x: bool,
    pub contain_y: bool,
}

impl ScrollTargetInfo {
    /// Does this container stop a user scroll by `(dx, dy)` from chaining to its ancestors?
    pub fn blocks_chain(&self, dx: f64, dy: f64) -> bool {
        (dx != 0.0 && self.user_x && self.contain_x) || (dy != 0.0 && self.user_y && self.contain_y)
    }

    /// Can a user scroll by `(dx, dy)` move this container at all?
    pub fn can_scroll(&self, dx: f64, dy: f64) -> bool {
        let room = |o: f64, lo: f64, hi: f64, d: f64| (d > 0.0 && o < hi) || (d < 0.0 && o > lo);
        (self.user_x && room(self.offset.0, self.min.0, self.max.0, dx))
            || (self.user_y && room(self.offset.1, self.min.1, self.max.1, dy))
    }
}

/// The outcome of a scroll write.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollApplied {
    pub offset: (f64, f64),
    /// The stored offset changed.
    pub changed: bool,
    /// The request was outside the reachable range.
    pub clamped: bool,
}

/// A `scroll` (and optionally `scrollend`) event waiting for the end of the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingScroll {
    pub target: Option<NodeId>,
    pub end: bool,
}

impl crate::interaction::dispatch::InteractionRuntime {
    /// Record that `target` scrolled; one `scroll` per target per operation (FR-088), and
    /// `scrollend` when the cause has a definite end (API, keyboard).
    pub(crate) fn note_scrolled(&mut self, doc: DocumentId, target: Option<NodeId>, end: bool) {
        let Some(s) = self.docs.get_mut(&doc) else {
            return;
        };
        match s.pending_scroll.iter_mut().find(|p| p.target == target) {
            Some(p) => p.end |= end,
            None => s.pending_scroll.push(PendingScroll { target, end }),
        }
        self.note_doc(doc);
    }
}

/// Dispatch the pending `scroll`/`scrollend` events of `doc` (inside the operation that caused
/// them). `scroll` bubbles only at the Document (viewport scroll); listeners may scroll again,
/// which queues more, bounded.
pub(crate) fn flush_scroll_events(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    origin: DispatchOrigin,
    ts: Option<f64>,
) {
    for _ in 0..8 {
        let pending = eng(host, doc, |ix, _| {
            ix.docs
                .get_mut(&doc)
                .map(|s| std::mem::take(&mut s.pending_scroll))
                .unwrap_or_default()
        })
        .unwrap_or_default();
        if pending.is_empty() {
            return;
        }
        for p in pending {
            let Ok((node, live)) = eng(host, doc, |_, d| match p.target {
                None => (d.document_node(), true),
                Some(n) => (n, d.arena().contains(n)),
            }) else {
                return;
            };
            if !live {
                continue;
            }
            let bubbles = p.target.is_none();
            let _ = run_dispatch(
                host,
                doc,
                node,
                EventInit::new("scroll").bubbles(bubbles),
                true,
                origin,
                ts,
            );
            if p.end {
                let _ = run_dispatch(
                    host,
                    doc,
                    node,
                    EventInit::new("scrollend").bubbles(bubbles),
                    true,
                    origin,
                    ts,
                );
            }
        }
    }
}

/// `scrollTo(x, y)` on an element (`Some`) or the viewport (`None`), clamped; instant (FR-085).
/// A non-scroll-container is a no-op (`changed = false`), as CSSOM View `scrollTop` on one.
pub fn scroll_to(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: Option<NodeId>,
    x: f64,
    y: f64,
) -> Result<ScrollApplied, Status> {
    operation(host, |h| {
        let applied = h.scroll_set(doc, target, x, y)?;
        if applied.changed {
            rt(h, |ix| ix.note_scrolled(doc, target, true));
            flush_scroll_events(h, doc, DispatchOrigin::ElementTask, None);
        }
        Ok(applied)
    })
}

/// `scrollBy(dx, dy)`.
pub fn scroll_by(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: Option<NodeId>,
    dx: f64,
    dy: f64,
) -> Result<ScrollApplied, Status> {
    let (x, y) = scroll_offset(host, doc, target)?;
    scroll_to(host, doc, target, x + dx, y + dy)
}

/// `scrollLeft`/`scrollTop` (the viewport for `None`); `(0, 0)` for a non-scroll-container.
pub fn scroll_offset(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: Option<NodeId>,
) -> Result<(f64, f64), Status> {
    Ok(host
        .scroll_info(doc, target)?
        .map_or((0.0, 0.0), |i| i.offset))
}

/// The first container of `chain` a user scroll by `(dx, dy)` can move; a container with
/// `overscroll-behavior: contain | none` on the scrolled axis ends the chain (0.8 F0804).
pub(crate) fn pick(chain: &[ScrollTargetInfo], dx: f64, dy: f64) -> Option<ScrollTargetInfo> {
    for i in chain {
        if i.can_scroll(dx, dy) {
            return Some(*i);
        }
        if i.blocks_chain(dx, dy) {
            return None;
        }
    }
    None
}

/// A user scroll (wheel, keys, track paging) of `info` by `(dx, dy)`: cancels a running smooth
/// scroll of the container and snaps per `scroll-snap-type` (the host decides; 0.8 F0803/F0804).
pub(crate) fn scroll_container_user(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    info: &ScrollTargetInfo,
    dx: f64,
    dy: f64,
    end: bool,
) -> Result<bool, Status> {
    let applied = host.scroll_user_by(doc, info.target, dx, dy)?;
    if applied.changed {
        rt(host, |ix| ix.note_scrolled(doc, info.target, end));
    }
    Ok(applied.changed)
}

/// One step of a smooth scroll: write the offset, queue `scroll` (and `scrollend` on the last
/// step) and dispatch them. Called by the owner clock, outside any other operation.
pub(crate) fn scroll_step(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    target: Option<NodeId>,
    x: f64,
    y: f64,
    end: bool,
) -> Result<ScrollApplied, Status> {
    operation(host, |h| {
        let applied = h.scroll_set(doc, target, x, y)?;
        if applied.changed {
            rt(h, |ix| ix.note_scrolled(doc, target, end));
            flush_scroll_events(h, doc, DispatchOrigin::ElementTask, None);
        }
        Ok(applied)
    })
}

/// Move `info` by `(dx, dy)`; notes the `scroll` event when the offset changed.
pub(crate) fn scroll_container_by(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    info: &ScrollTargetInfo,
    dx: f64,
    dy: f64,
    end: bool,
) -> Result<bool, Status> {
    let applied = host.scroll_set(doc, info.target, info.offset.0 + dx, info.offset.1 + dy)?;
    if applied.changed {
        rt(host, |ix| ix.note_scrolled(doc, info.target, end));
    }
    Ok(applied.changed)
}

/// A `Wheel` input item (FR-086): the trusted `wheel` at the hit element, then, unless
/// canceled, the nearest scroll container in the chain that can move scrolls and hover is
/// re-hit under the stationary pointer. Shift with a vertical-only delta scrolls horizontally;
/// Ctrl has no default action (no zoom).
#[allow(clippy::too_many_arguments)]
pub(crate) fn on_wheel(
    host: &mut dyn DispatchHost,
    doc: DocumentId,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    m: Modifiers,
    origin: DispatchOrigin,
    ts: Option<f64>,
) -> Result<(), Status> {
    let target = hit_element(host, doc, x, y)?;
    let mut init = EventInit::mouse("wheel", EventInterface::WheelEvent, x, y, 0, 0);
    init.data.delta_x = Some(dx);
    init.data.delta_y = Some(dy);
    init.modifiers = m;
    let out = run_dispatch(host, doc, target, init, true, origin, ts)?;
    if out.canceled || m.ctrl {
        return Ok(());
    }
    let (dx, dy) = if m.shift && dx == 0.0 && dy != 0.0 {
        (dy, 0.0)
    } else {
        (dx, dy)
    };
    if dx == 0.0 && dy == 0.0 || !eng(host, doc, |_, d| d.arena().contains(target))? {
        return Ok(());
    }
    let chain = host.scroll_chain(doc, target)?;
    if let Some(info) = pick(&chain, dx, dy)
        && scroll_container_user(host, doc, &info, dx, dy, false)?
    {
        let mut p = PointerData::mouse(x, y, -1);
        p.buttons = 0;
        let now = hit_element(host, doc, x, y)?;
        update_hover(host, doc, Some(now), &p, m, origin, ts, true)?;
    }
    Ok(())
}
