//! Scroll-related computed values (0.8 F0803/F0804): `scroll-behavior`, `overscroll-behavior`,
//! the scroll snap properties, `scroll-padding` and `scroll-margin`. Read from the computed
//! values by their CSS serialization, so the consumer needs no engine types. Physical axes:
//! `x` is the horizontal axis (`block`/`inline` map to `y`/`x`; vertical writing modes are not
//! mirrored).

use style::properties::ComputedValues;
use style::values::computed::Length;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnapStrictness {
    #[default]
    Proximity,
    Mandatory,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnapAlign {
    #[default]
    None,
    Start,
    Center,
    End,
}

/// A scroll-padding side: a length (CSS px) or a percentage of the scrollport.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PadSide {
    #[default]
    Auto,
    Px(f64),
    Percent(f64),
}

impl PadSide {
    pub fn resolve(self, port: f64) -> f64 {
        match self {
            PadSide::Auto => 0.0,
            PadSide::Px(v) => v,
            PadSide::Percent(p) => port * p,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollProps {
    pub smooth: bool,
    /// `overscroll-behavior` is `contain` or `none` on the x / y axis.
    pub contain_x: bool,
    pub contain_y: bool,
    /// `scroll-snap-type`: the axes that snap and the strictness (the same for both).
    pub snap_x: bool,
    pub snap_y: bool,
    pub strictness: SnapStrictness,
    /// `scroll-snap-align` of this box as a snap area.
    pub align_x: SnapAlign,
    pub align_y: SnapAlign,
    pub stop_always: bool,
    /// Top, right, bottom, left.
    pub padding: [PadSide; 4],
    pub margin: [f64; 4],
}

fn align_kw(s: &str) -> SnapAlign {
    match s {
        "start" => SnapAlign::Start,
        "center" => SnapAlign::Center,
        "end" => SnapAlign::End,
        _ => SnapAlign::None,
    }
}

fn pad(s: &str) -> PadSide {
    if let Some(v) = s.strip_suffix("px") {
        v.parse().map_or(PadSide::Auto, PadSide::Px)
    } else if let Some(v) = s.strip_suffix('%') {
        v.parse::<f64>()
            .map_or(PadSide::Auto, |p| PadSide::Percent(p / 100.0))
    } else {
        PadSide::Auto
    }
}

pub(crate) fn scroll_props(cv: &ComputedValues) -> ScrollProps {
    use super::groups::css;
    let b = cv.get_box();
    let mut p = ScrollProps {
        smooth: css(&b.scroll_behavior) == "smooth",
        contain_x: css(&b.overscroll_behavior_x) != "auto",
        contain_y: css(&b.overscroll_behavior_y) != "auto",
        stop_always: css(&b.scroll_snap_stop) == "always",
        ..ScrollProps::default()
    };
    let ty = css(&b.scroll_snap_type);
    let mut it = ty.split_whitespace();
    match it.next() {
        Some("x" | "inline") => p.snap_x = true,
        Some("y" | "block") => p.snap_y = true,
        Some("both") => {
            p.snap_x = true;
            p.snap_y = true;
        }
        _ => {}
    }
    if it.next() == Some("mandatory") {
        p.strictness = SnapStrictness::Mandatory;
    }
    let al = css(&b.scroll_snap_align);
    let mut it = al.split_whitespace();
    let block = it.next().map_or(SnapAlign::None, align_kw);
    let inline = it.next().map_or(block, align_kw);
    p.align_y = block;
    p.align_x = inline;
    let m = cv.get_margin();
    let l = |v: &Length| f64::from(v.px());
    p.margin = [
        l(&m.scroll_margin_top),
        l(&m.scroll_margin_right),
        l(&m.scroll_margin_bottom),
        l(&m.scroll_margin_left),
    ];
    let d = cv.get_padding();
    p.padding = [
        pad(&css(&d.scroll_padding_top)),
        pad(&css(&d.scroll_padding_right)),
        pad(&css(&d.scroll_padding_bottom)),
        pad(&css(&d.scroll_padding_left)),
    ];
    p
}
