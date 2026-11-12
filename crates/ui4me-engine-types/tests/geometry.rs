// owner: 001-vertical-slice — T023 (core-types.md §3; 008 fragment-tree.md §1, rules owned by 008)
use proptest::prelude::*;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::geometry::layout_unit::*;
use ui4me_engine_types::geometry::*;

#[test]
fn resolution_is_one_sixty_fourth_px() {
    assert_eq!(LayoutUnit::from_raw(64).to_f64(), 1.0);
    assert_eq!(LayoutUnit::from_raw(1).to_f64(), 0.015625);
    assert_eq!(LayoutUnit::from_css_px(4.0).unwrap().raw(), 256);
}

#[test]
fn edges_round_to_nearest_ties_toward_positive_infinity() {
    // 1/128 px is exactly half a unit
    assert_eq!(
        LayoutUnit::from_css_px(1.0 / 128.0).unwrap().raw(),
        1,
        "+0.5 unit -> +1"
    );
    assert_eq!(
        LayoutUnit::from_css_px(-1.0 / 128.0).unwrap().raw(),
        0,
        "-0.5 unit -> 0 (toward +inf)"
    );
    assert_eq!(
        LayoutUnit::from_css_px(-3.0 / 128.0).unwrap().raw(),
        -1,
        "-1.5 units -> -1"
    );
    assert_eq!(
        LayoutUnit::from_css_px(3.0 / 128.0).unwrap().raw(),
        2,
        "+1.5 units -> +2"
    );
    assert_eq!(
        LayoutUnit::from_css_px(0.01).unwrap().raw(),
        1,
        "0.64 units -> 1"
    );
    assert_eq!(
        LayoutUnit::from_css_px(0.007).unwrap().raw(),
        0,
        "0.448 units -> 0"
    );
}

#[test]
fn non_finite_rejected_at_conversion() {
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(LayoutUnit::from_css_px(v), Err(Code::GeometryNonFinite));
    }
    assert_eq!(
        LayoutUnit::from_css_px_f32(f32::NAN),
        Err(Code::GeometryNonFinite)
    );
}

#[test]
fn out_of_range_saturates_with_diagnostic() {
    let q = LayoutUnit::quantize(1e12);
    assert_eq!(q.value, LayoutUnit::MAX);
    assert_eq!(q.diagnostic, Some(Code::GeometryNonFinite));
    let q = LayoutUnit::quantize(-1e12);
    assert_eq!(q.value, LayoutUnit::MIN);
    assert_eq!(q.diagnostic, Some(Code::GeometryNonFinite));
    assert_eq!(LayoutUnit::MAX.raw(), i32::MAX);
    assert_eq!(
        LayoutUnit::MIN.raw(),
        -i32::MAX,
        "symmetric bound ±(2^31-1)/64"
    );
    let q = LayoutUnit::quantize(12.5);
    assert_eq!((q.value.raw(), q.diagnostic), (800, None));
}

#[test]
fn non_finite_from_an_algorithm_is_zero_plus_diagnostic() {
    let q = LayoutUnit::quantize(f64::NAN);
    assert_eq!(
        (q.value, q.diagnostic),
        (LayoutUnit::ZERO, Some(Code::GeometryNonFinite))
    );
}

#[test]
fn calc_boundary_maps_nan_to_zero_and_infinity_to_bound() {
    assert_eq!(LayoutUnit::from_calc(f64::NAN), LayoutUnit::ZERO);
    assert_eq!(LayoutUnit::from_calc(f64::INFINITY), LayoutUnit::MAX);
    assert_eq!(LayoutUnit::from_calc(f64::NEG_INFINITY), LayoutUnit::MIN);
}

#[test]
fn arithmetic_saturates() {
    assert_eq!(LayoutUnit::MAX + LayoutUnit::from_raw(1), LayoutUnit::MAX);
    assert_eq!(LayoutUnit::MIN - LayoutUnit::from_raw(1), LayoutUnit::MIN);
    assert_eq!(
        LayoutUnit::from_raw(3) - LayoutUnit::from_raw(5),
        LayoutUnit::from_raw(-2)
    );
}

#[test]
fn geometry_types() {
    let p = LayoutPoint {
        x: LayoutUnit::from_raw(64),
        y: LayoutUnit::from_raw(128),
    };
    let s = LayoutSize::from_edges(
        LayoutUnit::from_raw(10),
        LayoutUnit::from_raw(4),
        LayoutUnit::from_raw(0),
        LayoutUnit::from_raw(70),
    );
    assert_eq!(s.width, LayoutUnit::ZERO, "size never negative");
    assert_eq!(s.height, LayoutUnit::from_raw(70));
    let r = LayoutRect { origin: p, size: s };
    assert_eq!(r.origin.y.to_f64(), 2.0);
    let e = LayoutEdges {
        top: LayoutUnit::ZERO,
        right: LayoutUnit::ZERO,
        bottom: LayoutUnit::ZERO,
        left: LayoutUnit::from_raw(1),
    };
    assert_eq!(e.left.raw(), 1);
    assert_ne!(CoordinateSpace::Document, CoordinateSpace::Viewport);
    let d = DeviceRect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    };
    assert_eq!(d.w * d.h, 100);
}

proptest! {
    #[test]
    fn to_f64_is_exact_raw_over_64(raw in -i32::MAX..=i32::MAX) {
        let u = LayoutUnit::from_raw(raw);
        prop_assert_eq!(u.to_f64(), raw as f64 / 64.0);
        // round trip through css px is exact
        prop_assert_eq!(LayoutUnit::from_css_px(u.to_f64()).unwrap(), u);
    }

    #[test]
    fn rounding_matches_floor_plus_half(v in -1.0e6f64..1.0e6) {
        let got = LayoutUnit::from_css_px(v).unwrap().raw() as f64;
        prop_assert_eq!(got, (v * 64.0 + 0.5).floor());
    }
}
