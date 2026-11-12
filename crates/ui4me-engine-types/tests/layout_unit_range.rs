// owner: 008-layout-engine (T010; D-075) — LayoutUnit range statement, property tests.
use proptest::prelude::*;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::geometry::layout_unit::*;

proptest! {
    #[test]
    fn exact_below_2_18(raw in -(1i32 << 24)..(1i32 << 24)) {
        // Every multiple of 1/64 below 2^18 px round-trips exactly through f32 and f64.
        let px = raw as f64 / 64.0;
        prop_assert!(LayoutUnit::in_exact_range(px));
        prop_assert_eq!((px as f32) as f64, px);
        prop_assert_eq!(LayoutUnit::quantize(px).value.raw(), raw);
        prop_assert_eq!(LayoutUnit::from_raw(raw).to_f64(), px);
    }

    #[test]
    fn degraded_range_stays_finite_multiple_of_1_64(px in 262_144.0f64..33_554_431.0) {
        let q = LayoutUnit::quantize(px);
        prop_assert!(q.diagnostic.is_none());
        prop_assert!(q.value.to_f64().is_finite());
        prop_assert!((q.value.to_f64() - px).abs() <= 1.0 / 128.0 + 1e-9);
    }

    #[test]
    fn saturation_beyond_bound_emits_hook(px in 33_554_432.0f64..1e15, neg in any::<bool>()) {
        let v = if neg { -px } else { px };
        let mut hook: Vec<Code> = Vec::new();
        let u = LayoutUnit::quantize_with(v, &mut hook);
        prop_assert_eq!(u, if neg { LayoutUnit::MIN } else { LayoutUnit::MAX });
        prop_assert_eq!(hook, vec![Code::GeometryNonFinite]);
    }

    #[test]
    fn ties_round_toward_positive_infinity(n in -100_000i32..100_000) {
        let tie = (n as f64 + 0.5) / 64.0;
        prop_assert_eq!(LayoutUnit::quantize(tie).value.raw(), n + 1);
    }

    #[test]
    fn spans_preserve_adjacency(a in -1e5f64..1e5, w1 in 0.0f64..1e4, w2 in 0.0f64..1e4) {
        let mut hook: Vec<Code> = Vec::new();
        let (o1, s1) = LayoutUnit::quantize_span(a, w1, &mut hook);
        let (o2, s2) = LayoutUnit::quantize_span(a + w1, w2, &mut hook);
        // The shared edge quantises identically: end of box 1 equals start of box 2.
        prop_assert_eq!(o1 + s1, o2);
        prop_assert!(s1.raw() >= 0 && s2.raw() >= 0);
        prop_assert!(hook.is_empty());
    }
}

#[test]
fn calc_boundary_and_non_finite() {
    assert_eq!(LayoutUnit::from_calc(f64::NAN), LayoutUnit::ZERO);
    assert_eq!(LayoutUnit::from_calc(f64::INFINITY), LayoutUnit::MAX);
    assert_eq!(LayoutUnit::from_calc(f64::NEG_INFINITY), LayoutUnit::MIN);
    let mut hook: Vec<Code> = Vec::new();
    assert_eq!(
        LayoutUnit::quantize_with(f64::NAN, &mut hook),
        LayoutUnit::ZERO
    );
    assert_eq!(hook, vec![Code::GeometryNonFinite]);
    assert_eq!(MAX_PX, 33_554_431.984375);
}
