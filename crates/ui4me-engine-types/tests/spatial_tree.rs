//! 010 T007: property tests for the spatial-tree pure functions (spatial-tree.md §1-§5).

use proptest::prelude::*;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::geometry::spatial::*;
use ui4me_engine_types::geometry::transform::*;

const EPS: f64 = 1e-6;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * (1.0 + a.abs().max(b.abs()))
}

fn affine_close(a: &Affine2D, b: &Affine2D) -> bool {
    close(a.a, b.a)
        && close(a.b, b.b)
        && close(a.c, b.c)
        && close(a.d, b.d)
        && close(a.e, b.e)
        && close(a.f, b.f)
}

fn arb_affine() -> impl Strategy<Value = Affine2D> {
    (
        -3.0f64..3.0,
        -3.0f64..3.0,
        -3.0f64..3.0,
        -3.0f64..3.0,
        -500.0f64..500.0,
        -500.0f64..500.0,
    )
        .prop_map(|(a, b, c, d, e, f)| Affine2D { a, b, c, d, e, f })
        .prop_filter("well conditioned", |m| m.determinant().abs() > 0.2)
}

fn arb_op() -> impl Strategy<Value = TransformOp> {
    prop_oneof![
        (-200.0f64..200.0, -200.0f64..200.0, -50.0f64..50.0)
            .prop_map(|(x, y, z)| TransformOp::Translate { x, y, z }),
        (0.5f64..2.0, 0.5f64..2.0, 0.5f64..2.0).prop_map(|(x, y, z)| TransformOp::Scale {
            x,
            y,
            z
        }),
        (-1.0f64..1.0, -1.0f64..1.0, -1.0f64..1.0, -3.0f64..3.0)
            .prop_map(|(x, y, z, angle)| TransformOp::Rotate { x, y, z, angle }),
        (-0.8f64..0.8, -0.8f64..0.8).prop_map(|(ax, ay)| TransformOp::Skew { ax, ay }),
    ]
}

const BOX: RectF = RectF {
    x: 10.0,
    y: 20.0,
    width: 100.0,
    height: 50.0,
};

#[test]
fn transform_invert_round_trip_affine() {
    proptest!(|(m in arb_affine(), x in -100.0f64..100.0, y in -100.0f64..100.0)| {
        let inv = invert(&m).expect("invertible");
        let p = m.map_point(PointF { x, y });
        let q = inv.map_point(p);
        prop_assert!(close(q.x, x) && close(q.y, y));
        prop_assert!(affine_close(&m.then_after(&inv), &Affine2D::IDENTITY));
    });
}

#[test]
fn transform_compose_invert_round_trip_3d() {
    proptest!(|(ops in proptest::collection::vec(arb_op(), 1..5))| {
        let m = resolve_transform(&ops, &IndividualTransforms::default(), [50.0, 25.0, 0.0], BOX).unwrap();
        if let Some(inv) = m.invert() {
            let id = compose(&m, &inv).unwrap();
            for r in 0..4 { for c in 0..4 {
                let want = if r == c { 1.0 } else { 0.0 };
                prop_assert!((id.at(r, c) - want).abs() < 1e-6, "({r},{c}) = {}", id.at(r, c));
            }}
        }
    });
}

#[test]
fn transform_flatten_non_perspective_3d_equals_2d_affine() {
    // A 2D op list: the flattened 3D matrix equals the 2D composition, and is not projective.
    proptest!(|(a in arb_affine(), b in arb_affine())| {
        let ops = [TransformOp::Matrix(a), TransformOp::Matrix(b)];
        let m = resolve_transform(&ops, &IndividualTransforms::default(), [0.0, 0.0, 0.0], RectF::default()).unwrap();
        let f = flatten(&m).unwrap();
        prop_assert!(!f.projective);
        prop_assert!(affine_close(&f.affine, &a.then_after(&b)));
    });
    // Rotation about x and y plus translation in z stay non-projective; flatten keeps the xy part.
    proptest!(|(ops in proptest::collection::vec(arb_op(), 1..5))| {
        let m = resolve_transform(&ops, &IndividualTransforms::default(), [0.0, 0.0, 0.0], RectF::default()).unwrap();
        prop_assert!(!flatten(&m).unwrap().projective);
    });
}

#[test]
fn transform_projective_only_from_perspective_terms() {
    let p = perspective_matrix(Some(500.0), [50.0, 25.0], BOX)
        .unwrap()
        .expect("perspective");
    assert!(flatten(&p).unwrap().projective);
    assert_eq!(perspective_matrix(None, [0.0, 0.0], BOX).unwrap(), None);
    assert_eq!(
        perspective_matrix(Some(0.0), [0.0, 0.0], BOX).unwrap(),
        None
    );
    let m = resolve_transform(
        &[TransformOp::Perspective(300.0)],
        &IndividualTransforms::default(),
        [0.0; 3],
        BOX,
    )
    .unwrap();
    assert!(flatten(&m).unwrap().projective);
}

#[test]
fn transform_backface_rotate_y_180_is_hidden() {
    let rot = |deg: f64| TransformOp::Rotate {
        x: 0.0,
        y: 1.0,
        z: 0.0,
        angle: deg.to_radians(),
    };
    let m180 = resolve_transform(
        &[rot(180.0)],
        &IndividualTransforms::default(),
        [50.0, 25.0, 0.0],
        BOX,
    )
    .unwrap();
    assert!(
        is_backface_visible(&m180),
        "rotateY(180deg) shows the back face"
    );
    let m0 = resolve_transform(
        &[rot(0.0)],
        &IndividualTransforms::default(),
        [50.0, 25.0, 0.0],
        BOX,
    )
    .unwrap();
    assert!(!is_backface_visible(&m0));
    let m60 = resolve_transform(
        &[rot(60.0)],
        &IndividualTransforms::default(),
        [50.0, 25.0, 0.0],
        BOX,
    )
    .unwrap();
    assert!(!is_backface_visible(&m60));
    let m120 = resolve_transform(
        &[rot(120.0)],
        &IndividualTransforms::default(),
        [50.0, 25.0, 0.0],
        BOX,
    )
    .unwrap();
    assert!(is_backface_visible(&m120));
}

#[test]
fn transform_non_invertible_is_none() {
    assert!(
        invert(&Affine2D {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 1.0,
            f: 1.0
        })
        .is_none()
    );
    assert!(
        invert(&Affine2D {
            a: 1.0,
            b: 2.0,
            c: 2.0,
            d: 4.0,
            e: 0.0,
            f: 0.0
        })
        .is_none()
    );
    let m = Transform3D::scaling(1.0, 1.0, 0.0);
    assert!(m.invert().is_none());
}

#[test]
fn transform_map_rect_and_bounds_containment() {
    proptest!(|(m in arb_affine(), x in -50.0f64..50.0, y in -50.0f64..50.0, w in 0.0f64..80.0, h in 0.0f64..80.0, u in 0.0f64..1.0, v in 0.0f64..1.0)| {
        let r = RectF::new(x, y, w, h);
        let q = map_rect(&m, &r).unwrap();
        let b = bounds(&q);
        // Any point of the source rect maps inside the bounds.
        let p = m.map_point(PointF { x: x + u * w, y: y + v * h });
        prop_assert!(p.x >= b.x - 1e-6 && p.x <= b.right() + 1e-6 && p.y >= b.y - 1e-6 && p.y <= b.bottom() + 1e-6);
        // Every corner lies inside the bounds.
        for c in q.0 { prop_assert!(c.x >= b.x - 1e-9 && c.x <= b.right() + 1e-9 && c.y >= b.y - 1e-9 && c.y <= b.bottom() + 1e-9); }
    });
}

#[test]
fn transform_resolve_order_follows_css_ctm() {
    // translate(origin) . translate . rotate . scale . list . translate(-origin)
    let ind = IndividualTransforms {
        translate: Some([10.0, 0.0, 0.0]),
        rotate: Some(([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2)),
        scale: Some([2.0, 2.0, 1.0]),
    };
    let list = [TransformOp::Translate {
        x: 5.0,
        y: 0.0,
        z: 0.0,
    }];
    let m = resolve_transform(&list, &ind, [0.0, 0.0, 0.0], RectF::default()).unwrap();
    // Apply to (1, 0): list translate -> (6, 0); scale 2 -> (12, 0); rotate 90deg -> (0, 12); translate 10 -> (10, 12).
    let f = flatten(&m).unwrap().affine;
    let p = f.map_point(PointF { x: 1.0, y: 0.0 });
    assert!(close(p.x, 10.0) && close(p.y, 12.0), "{p:?}");
    // The origin is a fixed point of a pure scale.
    let m = resolve_transform(
        &[TransformOp::Scale {
            x: 3.0,
            y: 3.0,
            z: 1.0,
        }],
        &IndividualTransforms::default(),
        [50.0, 25.0, 0.0],
        BOX,
    )
    .unwrap();
    let p = flatten(&m).unwrap().affine.map_point(PointF {
        x: BOX.x + 50.0,
        y: BOX.y + 25.0,
    });
    assert!(close(p.x, BOX.x + 50.0) && close(p.y, BOX.y + 25.0));
}

#[test]
fn transform_non_finite_input_is_geometry_non_finite() {
    let bad = TransformOp::Translate {
        x: f64::NAN,
        y: 0.0,
        z: 0.0,
    };
    assert_eq!(
        resolve_transform(&[bad], &IndividualTransforms::default(), [0.0; 3], BOX),
        Err(Code::GeometryNonFinite)
    );
    let inf = Affine2D {
        a: f64::INFINITY,
        ..Affine2D::IDENTITY
    };
    assert_eq!(map_rect(&inf, &BOX), Err(Code::GeometryNonFinite));
    assert_eq!(
        flatten(&Transform3D([f64::NAN; 16])),
        Err(Code::GeometryNonFinite)
    );
    assert_eq!(
        compose(&Transform3D::IDENTITY, &Transform3D([f64::INFINITY; 16])),
        Err(Code::GeometryNonFinite)
    );
    assert!(invert(&inf).is_none());
    assert_eq!(
        perspective_matrix(Some(f64::NAN), [0.0; 2], BOX),
        Err(Code::GeometryNonFinite)
    );
    assert_eq!(
        scroll_translate((f64::NAN, 0.0)),
        Err(Code::GeometryNonFinite)
    );
    assert_eq!(
        fixed_anchor((0.0, f64::INFINITY)),
        Err(Code::GeometryNonFinite)
    );
    let sv = StickyValues {
        containing_block_rect: RectF::new(0.0, 0.0, f64::NAN, 10.0),
        insets: StickyInsets::default(),
        normal_flow_rect: BOX,
    };
    assert_eq!(sticky_offset(&sv, &BOX), Err(Code::GeometryNonFinite));
    assert_eq!(
        compose_chain(
            &[SpatialStep::Translate((f64::NAN, 0.0))],
            TransformFallback::EXACT
        ),
        Err(Code::GeometryNonFinite)
    );
}

fn sticky(
    top: Option<f64>,
    bottom: Option<f64>,
    left: Option<f64>,
    right: Option<f64>,
) -> StickyValues {
    StickyValues {
        containing_block_rect: RectF::new(0.0, 0.0, 400.0, 1000.0),
        insets: StickyInsets {
            top,
            right,
            bottom,
            left,
        },
        normal_flow_rect: RectF::new(20.0, 300.0, 100.0, 40.0),
    }
}

#[test]
fn sticky_never_leaves_containing_block_and_is_monotonic() {
    proptest!(|(top in proptest::option::of(0.0f64..50.0), bottom in proptest::option::of(0.0f64..50.0),
                left in proptest::option::of(0.0f64..50.0), right in proptest::option::of(0.0f64..50.0),
                s0 in 0.0f64..2000.0, ds in 0.0f64..500.0, sx0 in 0.0f64..300.0, dsx in 0.0f64..300.0)| {
        let c = sticky(top, bottom, left, right);
        let view = |sy: f64, sx: f64| RectF::new(sx, sy, 300.0, 200.0);
        let (dx0, dy0) = sticky_offset(&c, &view(s0, sx0)).unwrap();
        let (dx1, dy1) = sticky_offset(&c, &view(s0 + ds, sx0 + dsx)).unwrap();
        let n = c.normal_flow_rect;
        let cb = c.containing_block_rect;
        for (dx, dy) in [(dx0, dy0), (dx1, dy1)] {
            prop_assert!(n.x + dx >= cb.x - 1e-9 && n.right() + dx <= cb.right() + 1e-9);
            prop_assert!(n.y + dy >= cb.y - 1e-9 && n.bottom() + dy <= cb.bottom() + 1e-9);
        }
        prop_assert!(dy1 >= dy0 - 1e-9, "monotonic in scroll offset: {dy0} -> {dy1}");
        prop_assert!(dx1 >= dx0 - 1e-9);
    });
}

#[test]
fn sticky_auto_insets_are_unconstrained() {
    let c = sticky(None, None, None, None);
    for sy in [0.0, 500.0, 5000.0] {
        assert_eq!(
            sticky_offset(&c, &RectF::new(0.0, sy, 300.0, 200.0)).unwrap(),
            (0.0, 0.0)
        );
    }
    // top: 10 sticks at the view top once scrolled past, and stops at the containing block bottom.
    let c = sticky(Some(10.0), None, None, None);
    assert_eq!(
        sticky_offset(&c, &RectF::new(0.0, 0.0, 300.0, 200.0))
            .unwrap()
            .1,
        0.0
    );
    assert_eq!(
        sticky_offset(&c, &RectF::new(0.0, 400.0, 300.0, 200.0))
            .unwrap()
            .1,
        110.0
    );
    assert_eq!(
        sticky_offset(&c, &RectF::new(0.0, 5000.0, 300.0, 200.0))
            .unwrap()
            .1,
        660.0
    );
    // bottom: 10 holds the box at the view bottom while it would be below it.
    let c = sticky(None, Some(10.0), None, None);
    assert_eq!(
        sticky_offset(&c, &RectF::new(0.0, 0.0, 300.0, 200.0))
            .unwrap()
            .1,
        -150.0
    );
}

#[test]
fn scroll_and_fixed_signs() {
    let s = scroll_translate((30.0, 40.0)).unwrap();
    assert_eq!((s.e, s.f), (-30.0, -40.0));
    let f = fixed_anchor((30.0, 40.0)).unwrap();
    assert_eq!((f.e, f.f), (30.0, 40.0));
    // A fixed box under a scrolled root: scroll then anchor cancel.
    let acc = compose_chain(
        &[
            SpatialStep::Scroll((30.0, 40.0)),
            SpatialStep::Fixed((30.0, 40.0)),
            SpatialStep::Translate((5.0, 6.0)),
        ],
        TransformFallback::EXACT,
    )
    .unwrap();
    assert!(close(acc.affine.e, 5.0) && close(acc.affine.f, 6.0));
}

#[test]
fn compose_chain_step_semantics() {
    let rot = Transform3D::rotation(0.0, 0.0, 1.0, std::f64::consts::FRAC_PI_2);
    let steps = [
        SpatialStep::Translate((100.0, 0.0)),
        SpatialStep::Scroll((0.0, 10.0)),
        SpatialStep::Sticky((0.0, 4.0)),
        SpatialStep::Transform(rot),
        SpatialStep::Translate((1.0, 0.0)),
    ];
    let acc = compose_chain(&steps, TransformFallback::EXACT).unwrap();
    // Leaf point (0,0): translate (1,0) -> rotate 90 -> (0,1) -> sticky +4 -> (0,5) -> scroll -10 -> (0,-5) -> +100 -> (100,-5).
    let p = acc.affine.map_point(PointF { x: 0.0, y: 0.0 });
    assert!(close(p.x, 100.0) && close(p.y, -5.0), "{p:?}");
    assert!(!acc.projective && !acc.backface_visible);
    // Parent perspective makes the result projective, and rotateY(180) shows the back face.
    let persp = perspective_matrix(Some(400.0), [50.0, 25.0], BOX)
        .unwrap()
        .unwrap();
    let flip = Transform3D::rotation(0.0, 1.0, 0.0, std::f64::consts::PI);
    let acc = compose_chain(
        &[
            SpatialStep::Perspective(persp),
            SpatialStep::Transform(flip),
        ],
        TransformFallback::EXACT,
    )
    .unwrap();
    assert!(acc.projective);
    assert!(acc.backface_visible);
}

#[test]
fn fallback_rule_gives_identical_geometry_for_all_callers() {
    proptest!(|(ops in proptest::collection::vec(arb_op(), 1..4), d in 100.0f64..2000.0)| {
        let m = resolve_transform(&ops, &IndividualTransforms::default(), [50.0, 25.0, 0.0], BOX).unwrap();
        let persp = perspective_matrix(Some(d), [50.0, 25.0], BOX).unwrap().unwrap();
        let steps = [SpatialStep::Translate((7.0, 9.0)), SpatialStep::Perspective(persp), SpatialStep::Transform(m)];

        let fb = TransformFallback { perspective: PerspectiveMode::AffinePart, preserve_3d: Preserve3dMode::Exact };
        let a1 = compose_chain(&steps, fb).unwrap();
        let a2 = compose_chain(&steps, fb).unwrap();
        prop_assert_eq!(a1, a2);
        prop_assert!(!a1.projective);
        // AffinePart is exactly flatten(exact).affine.
        let exact = compose_chain(&steps, TransformFallback::EXACT).unwrap();
        prop_assert!(affine_close(&a1.affine, &exact.affine));

        // Flat: each transform step is flattened first, so it equals composing the flattened steps.
        let flat = TransformFallback { perspective: PerspectiveMode::Exact, preserve_3d: Preserve3dMode::Flat };
        let f = compose_chain(&steps, flat).unwrap();
        let manual = [SpatialStep::Translate((7.0, 9.0)), SpatialStep::Perspective(persp), SpatialStep::Transform(flatten(&m).unwrap().affine.to_3d())];
        let g = compose_chain(&manual, TransformFallback::EXACT).unwrap();
        prop_assert!(affine_close(&f.affine, &g.affine));
    });
}
