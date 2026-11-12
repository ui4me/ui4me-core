//! Overlay scrollbar track and thumb painting (015 PAINT-3).
use crate::layout::coordinator::overflow::ScrollRange;
use crate::layout::fragment::OverflowMode;
use crate::scene::appendix_e::{PaintBox, PaintPhase, Pending};
use crate::scene::display_list::{DrawOp, ItemKind, Paint, Rect, RoundedRect, ScopeId};
use crate::scene::hit_index::ScrollAxis;
use crate::scene::inputs::{ScrollbarPaint, ScrollbarStyle};

pub(crate) fn layout(
    port: Rect,
    range: ScrollRange,
    offset: (f64, f64),
    style: ScrollbarStyle,
    ox: OverflowMode,
    oy: OverflowMode,
) -> Vec<ScrollbarPaint> {
    let mut out = Vec::new();
    let thickness = style.width.min(port.width).min(port.height).max(0.0);
    if thickness == 0.0 {
        return out;
    }
    let v = range.max.1 - range.min.1;
    let h = range.max.0 - range.min.0;
    let show_v = oy == OverflowMode::Scroll || (oy == OverflowMode::Auto && v > 0.0);
    let show_h = ox == OverflowMode::Scroll || (ox == OverflowMode::Auto && h > 0.0);
    if show_v {
        let track = Rect {
            x: if style.left {
                port.x
            } else {
                port.x + port.width - thickness
            },
            y: port.y,
            width: thickness,
            height: (port.height - if show_h { thickness } else { 0.0 }).max(0.0),
        };
        let len = (track.height * (f64::from(port.height) / (f64::from(port.height) + v)) as f32)
            .clamp(thickness.min(track.height), track.height);
        let pos = if v > 0.0 {
            ((offset.1 - range.min.1) / v).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let thumb = Rect {
            y: track.y + (track.height - len) * pos,
            height: len,
            ..track
        };
        out.push(ScrollbarPaint {
            axis: ScrollAxis::Vertical,
            track,
            thumb,
            colors: style,
        });
    }
    if show_h {
        let track = Rect {
            x: port.x + if show_v && style.left { thickness } else { 0.0 },
            y: port.y + port.height - thickness,
            width: (port.width - if show_v { thickness } else { 0.0 }).max(0.0),
            height: thickness,
        };
        let len = (track.width * (f64::from(port.width) / (f64::from(port.width) + h)) as f32)
            .clamp(thickness.min(track.width), track.width);
        let pos = if h > 0.0 {
            ((offset.0 - range.min.0) / h).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let thumb = Rect {
            x: track.x + (track.width - len) * pos,
            width: len,
            ..track
        };
        out.push(ScrollbarPaint {
            axis: ScrollAxis::Horizontal,
            track,
            thumb,
            colors: style,
        });
    }
    out
}

pub(crate) fn push(b: &PaintBox, out: &mut Vec<Pending>, scope: Option<ScopeId>) {
    for bar in &b.extras.scrollbars {
        let (track_kind, thumb_kind) = match bar.axis {
            crate::scene::hit_index::ScrollAxis::Horizontal => {
                ("scrollbar-h-track", "scrollbar-h-thumb")
            }
            crate::scene::hit_index::ScrollAxis::Vertical => {
                ("scrollbar-v-track", "scrollbar-v-thumb")
            }
        };
        if bar.track.width <= 0.0 || bar.track.height <= 0.0 {
            continue;
        }
        out.push(Pending {
            owner: b.node,
            phase: PaintPhase::OverlayScrollbar,
            kind: ItemKind(track_kind),
            ink: bar.track,
            scope,
            op: DrawOp::FillRect {
                rect: bar.track,
                paint: Paint::Solid(bar.colors.track),
            },
        });
        let inset = (bar.colors.width * 0.2).min(2.0);
        let thumb = match bar.axis {
            ScrollAxis::Horizontal => Rect {
                y: bar.thumb.y + inset,
                height: (bar.thumb.height - 2.0 * inset).max(0.0),
                ..bar.thumb
            },
            ScrollAxis::Vertical => Rect {
                x: bar.thumb.x + inset,
                width: (bar.thumb.width - 2.0 * inset).max(0.0),
                ..bar.thumb
            },
        };
        let radius = thumb.width.min(thumb.height) / 2.0;
        out.push(Pending {
            owner: b.node,
            phase: PaintPhase::OverlayScrollbar,
            kind: ItemKind(thumb_kind),
            ink: thumb,
            scope,
            op: DrawOp::FillRoundedRect {
                rect: RoundedRect {
                    rect: thumb,
                    radii: [(radius, radius); 4],
                },
                paint: Paint::Solid(bar.colors.thumb),
            },
        });
    }
}
