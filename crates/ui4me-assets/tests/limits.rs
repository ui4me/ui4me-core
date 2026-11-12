// owner: 007-resources (T066, T068–T071) — limits at every enforcement point of
// contracts/resource-policy.md §4, and the bomb corpus (verification/fixtures/resource-bombs).

mod support;

use serde_json::Value;
use std::time::{Duration, Instant};
use support::bombs::{self, BOMBS};
use support::*;
use ui4me_assets::decode::fake::FakeImage;
use ui4me_assets::policy::{MIB, ResourcePolicy, ResourcePolicyPatch};
use ui4me_assets::request::Requester;
use ui4me_assets::uri;
use ui4me_assets::view::{ImageState, PaintImage};
use ui4me_engine_types::codes_generated::Code;

fn patch(v: Option<&Value>) -> ResourcePolicyPatch {
    let mut p = ResourcePolicyPatch::default();
    if let Some(o) = v.and_then(Value::as_object) {
        for (k, v) in o {
            let n = v.as_u64().unwrap();
            match k.as_str() {
                "max_decoded_pixels" => p.max_decoded_pixels = Some(n),
                "max_encoded_image" => p.max_encoded_image = Some(n),
                "decoded_memory_budget" => p.decoded_memory_budget = Some(n),
                other => panic!("policy key {other}"),
            }
        }
    }
    p
}

/// Run one bomb case; returns (observed code name, elapsed).
pub fn run_bomb(case: &Value, deterministic: bool) -> (String, Duration) {
    if case["kind"] == "font" {
        // a host-registered font and an @font-face source are both bounded by the 32 MiB font limit
        let t = TempDir::new("bomb-font");
        let name = case["name"].as_str().unwrap_or("bomb.ttf");
        bombs::write_recipe(case, &t.join(name));
        let started = Instant::now();
        let mut reg = ui4me_assets::fonts::FontRegistry::new(ui4me_assets::fonts::FontMode::System);
        let code = match reg.register_file(&t.join(name)) {
            Ok(_) => "OK".to_string(),
            Err(e) => e.code.name().to_string(),
        };
        let _ = deterministic;
        return (code, started.elapsed());
    }
    let t = TempDir::new("bomb");
    t.write("index.html", b"x");
    let name = case["name"].as_str().unwrap_or("bomb.png");
    bombs::write_recipe(case, &t.join(name));
    let defaults = match case.get("runtime_policy") {
        Some(rp) => {
            let mut p = patch(Some(rp));
            p.cache_budget = Some(0);
            ResourcePolicy::default().merged(&p).unwrap()
        }
        None => ResourcePolicy::default(),
    };
    let mut mgr = manager_with(deterministic, defaults);
    if let Some(ms) = case.get("deadline_override_ms").and_then(Value::as_u64) {
        mgr.set_deadline_override_for_testing(Some(Duration::from_millis(ms)));
    }
    let doc = doc_id(1);
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &patch(case.get("policy")))
        .unwrap();
    let started = Instant::now();
    let r = mgr
        .request_image(doc, Requester::ImgElement(node_id(1)), &base, name, 1.0)
        .unwrap();
    settle(&mut mgr, doc);
    let elapsed = started.elapsed();
    let code = match mgr.natural_size(&r) {
        ImageState::Broken { code } => code.name().to_string(),
        ImageState::Ready { .. } => "OK".into(),
        ImageState::Pending => panic!("pending"),
    };
    (code, elapsed)
}

pub fn runnable_cases() -> Vec<Value> {
    let v: Value = serde_json::from_str(BOMBS).unwrap();
    v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c.get("pending").is_none())
        .cloned()
        .collect()
}

#[test]
fn every_bomb_is_rejected_with_its_canonical_code_within_a_second() {
    for det in [false, true] {
        for case in runnable_cases() {
            let (code, elapsed) = run_bomb(&case, det);
            assert_eq!(
                code,
                case["expect"].as_str().unwrap(),
                "{} (det={det})",
                case["id"]
            );
            let limit = case
                .get("within_ms")
                .and_then(Value::as_u64)
                .unwrap_or(1000);
            assert!(
                elapsed < Duration::from_millis(limit),
                "{} took {elapsed:?}",
                case["id"]
            );
        }
    }
}

#[test]
fn no_bomb_case_is_pending_any_more() {
    let v: Value = serde_json::from_str(BOMBS).unwrap();
    let pending: Vec<_> = v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c.get("pending").is_some())
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert!(
        pending.is_empty(),
        "the nested-SVG bomb runs with the SVG adapter (T049): {pending:?}"
    );
}

#[test]
fn over_65535_side_within_budget_is_downscaled_and_keeps_its_natural_size() {
    for det in [false, true] {
        let t = TempDir::new("wide");
        t.write("index.html", b"x");
        t.write("wide.fake", &FakeImage::new(70_000, 10).bytes());
        let mut mgr = manager(det);
        let doc = doc_id(1);
        let f = t.join("index.html");
        let base = uri::document_url_from_file(&f).unwrap();
        mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
            .unwrap();
        let r = mgr
            .request_image(
                doc,
                Requester::ImgElement(node_id(1)),
                &base,
                "wide.fake",
                1.0,
            )
            .unwrap();
        settle(&mut mgr, doc);
        assert_eq!(
            mgr.natural_size(&r),
            ImageState::Ready {
                width_css: Some(70_000.0),
                height_css: Some(10.0),
                ratio: Some((70_000.0, 10.0))
            },
            "the natural size is the original"
        );
        match mgr.paint_image(&r, (1, 1)) {
            PaintImage::Raster { pixels, .. } => {
                assert_eq!((pixels.width, pixels.height), (65_535, 9))
            }
            other => panic!("{other:?}"),
        }
        let rec = &mgr.document_records(doc)[0];
        assert!(rec.flags.contains(&"downscaled"));
        assert_eq!(
            rec.warnings,
            vec![Code::SizeLimit],
            "recorded as a SIZE_LIMIT warning"
        );
    }
}

#[test]
fn decoded_memory_budget_is_checked_before_the_decode_is_scheduled() {
    let t = TempDir::new("mem");
    t.write("index.html", b"x");
    for n in 0..3 {
        // 2048x1024x4 = 8 MiB each (different content so the cache keys differ)
        t.write(
            &format!("i{n}.fake"),
            &FakeImage::new(2048, 1024).color([n, 0, 0, 255]).bytes(),
        );
    }
    let defaults = ResourcePolicy::default()
        .merged(&ResourcePolicyPatch {
            decoded_memory_budget: Some(16 * MIB),
            cache_budget: Some(0),
            ..Default::default()
        })
        .unwrap();
    let mut mgr = manager_with(true, defaults);
    let doc = doc_id(1);
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    let refs: Vec<_> = (0..3)
        .map(|n| {
            mgr.request_image(
                doc,
                Requester::ImgElement(node_id(n)),
                &base,
                &format!("i{n}.fake"),
                1.0,
            )
            .unwrap()
        })
        .collect();
    mgr.process_completions();
    assert!(matches!(
        mgr.natural_size(&refs[0]),
        ImageState::Ready { .. }
    ));
    assert!(matches!(
        mgr.natural_size(&refs[1]),
        ImageState::Ready { .. }
    ));
    assert_eq!(
        mgr.natural_size(&refs[2]),
        ImageState::Broken {
            code: Code::BudgetExceeded
        },
        "referenced entries are never evicted to make room"
    );
    assert!(mgr.cache_stats().bytes <= 16 * MIB);
}

#[test]
fn unreferenced_entries_are_evicted_first_to_make_room() {
    let t = TempDir::new("mem-evict");
    t.write("index.html", b"x");
    for n in 0..3 {
        t.write(
            &format!("i{n}.fake"),
            &FakeImage::new(2048, 1024).color([n, 0, 0, 255]).bytes(),
        );
    }
    let defaults = ResourcePolicy::default()
        .merged(&ResourcePolicyPatch {
            decoded_memory_budget: Some(16 * MIB),
            cache_budget: Some(16 * MIB),
            ..Default::default()
        })
        .unwrap();
    let mut mgr = manager_with(true, defaults);
    let doc = doc_id(1);
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    let node = node_id(1);
    for n in 0..3 {
        // one element re-pointed at a new image each time: the previous entry loses its reference
        let r = mgr
            .request_image(
                doc,
                Requester::ImgElement(node),
                &base,
                &format!("i{n}.fake"),
                1.0,
            )
            .unwrap();
        mgr.process_completions();
        assert!(
            matches!(mgr.natural_size(&r), ImageState::Ready { .. }),
            "image {n}"
        );
    }
    assert!(mgr.cache_stats().bytes <= 16 * MIB);
    assert!(mgr.cache_stats().evictions >= 1);
}

#[test]
fn decode_deadline_marks_the_image_broken_with_budget_exceeded() {
    for det in [false, true] {
        let t = TempDir::new("deadline");
        t.write("index.html", b"x");
        t.write("slow.fake", &FakeImage::new(4, 4).delay(300).bytes());
        let mut mgr = manager(det);
        mgr.set_deadline_override_for_testing(Some(Duration::from_millis(50)));
        let doc = doc_id(1);
        let f = t.join("index.html");
        let base = uri::document_url_from_file(&f).unwrap();
        mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
            .unwrap();
        let r = mgr
            .request_image(
                doc,
                Requester::ImgElement(node_id(1)),
                &base,
                "slow.fake",
                1.0,
            )
            .unwrap();
        settle(&mut mgr, doc);
        assert_eq!(
            mgr.natural_size(&r),
            ImageState::Broken {
                code: Code::BudgetExceeded
            }
        );
    }
}

#[test]
fn deadline_range_is_one_to_sixty_seconds() {
    let base = ResourcePolicy::default();
    assert!(
        base.merged(&ResourcePolicyPatch {
            decode_deadline_ms: Some(1000),
            ..Default::default()
        })
        .is_ok()
    );
    assert!(
        base.merged(&ResourcePolicyPatch {
            decode_deadline_ms: Some(60_000),
            ..Default::default()
        })
        .is_ok()
    );
    assert!(
        base.merged(&ResourcePolicyPatch {
            decode_deadline_ms: Some(999),
            ..Default::default()
        })
        .is_err()
    );
}

#[test]
fn stylesheet_limit_is_enforced_in_text_loading() {
    let t = TempDir::new("css-limit");
    t.write("index.html", b"x");
    t.write("big.css", &vec![b' '; 4096]);
    let mut mgr = manager(true);
    let doc = doc_id(1);
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(
        doc,
        base.clone(),
        Some(&f),
        &ResourcePolicyPatch {
            max_encoded_stylesheet: Some(1024),
            ..Default::default()
        },
    )
    .unwrap();
    let e = mgr
        .load_text_sync(doc, &base, "big.css", uri::ResourceKind::Stylesheet)
        .unwrap_err();
    assert_eq!(e.code, Code::SizeLimit);
    t.write("ok.css", b"a{}");
    assert_eq!(
        mgr.load_text_sync(doc, &base, "ok.css", uri::ResourceKind::Stylesheet)
            .unwrap()
            .bytes
            .as_ref(),
        b"a{}"
    );
    assert_eq!(
        mgr.load_text_sync(doc, &base, "../etc/passwd", uri::ResourceKind::Stylesheet)
            .unwrap_err()
            .code,
        Code::ResourceDenied
    );
    assert_eq!(
        mgr.load_text_sync(doc, &base, "missing.css", uri::ResourceKind::Stylesheet)
            .unwrap_err()
            .code,
        Code::FileNotFound
    );
}
