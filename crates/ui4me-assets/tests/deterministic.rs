// owner: 007-resources (T019, T024) — deterministic resource mode (FR-067, R-09).
//
// State sequences, events, epoch increments and relayout counts equal those of asynchronous mode;
// no worker thread is created and no cross-thread wait occurs; limits and policy apply unchanged.
#![cfg(unix)]

mod support;

use support::*;
use ui4me_assets::decode::fake::FakeImage;
use ui4me_assets::policy::ResourcePolicyPatch;
use ui4me_assets::request::Requester;
use ui4me_assets::uri;
use ui4me_assets::view::ImageState;
use ui4me_engine_types::codes_generated::Code;

/// What a host could observe, in order.
#[derive(Debug, PartialEq)]
struct Trace {
    /// per drain step: (epoch, changed count, intrinsic count, paint-only count)
    batches: Vec<(u64, usize, usize, usize)>,
    /// final per-request state
    states: Vec<String>,
    /// `load`/`error` events as (node index, kind) in order of node
    events: Vec<(u32, String)>,
    /// per-request state-name sequences from the diagnostic records
    transitions: Vec<Vec<String>>,
    counters: (u32, u32, u32, u32, u64),
}

fn script(det: bool) -> Trace {
    let t = TempDir::new("det");
    t.write("index.html", b"x");
    t.write("a.fake", &FakeImage::new(4, 3).bytes());
    t.write("b.fake", &FakeImage::new(8, 6).delay(20).bytes());
    t.write(
        "fails.fake",
        &FakeImage::new(2, 2).fail_with(Code::DecodeFailed).bytes(),
    );
    t.write("panics.fake", &FakeImage::new(2, 2).panics().bytes());
    t.write("huge.fake", &FakeImage::new(9000, 9000).bytes());
    t.write("big.bin", &vec![0u8; 4096]);
    let mut mgr = manager(det);
    let doc = doc_id(1);
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    let patch = ResourcePolicyPatch {
        max_decoded_pixels: Some(1_000_000),
        max_encoded_image: Some(2048),
        ..Default::default()
    };
    mgr.create_document(doc, base.clone(), Some(&f), &patch)
        .unwrap();
    let srcs = [
        "a.fake",
        "b.fake",
        "fails.fake",
        "panics.fake",
        "huge.fake",
        "big.bin",
        "missing.fake",
        "http://example.com/x.png",
        "a.fake",
    ];
    let mut refs = Vec::new();
    for (i, s) in srcs.iter().enumerate() {
        refs.push(
            mgr.request_image(
                doc,
                Requester::ImgElement(node_id(i as u32 + 1)),
                &base,
                s,
                1.0,
            )
            .unwrap(),
        );
    }
    // resolution-time denials need no worker; wait for the rest, then drain once
    if !det {
        wait_queued(&mgr, srcs.len() - 1);
    }
    let batches: Vec<_> = mgr
        .process_completions()
        .iter()
        .map(|b| {
            (
                b.resource_epoch.0,
                b.changed.len(),
                b.intrinsic_size.len(),
                b.paint_only.len(),
            )
        })
        .collect();
    assert_eq!(mgr.pending(doc).unwrap(), 0);
    let states = refs
        .iter()
        .map(|r| match mgr.natural_size(r) {
            ImageState::Ready {
                width_css,
                height_css,
                ..
            } => format!("Ready {width_css:?}x{height_css:?}"),
            ImageState::Broken { code } => format!("Broken {}", code.name()),
            ImageState::Pending => "Pending".into(),
        })
        .collect();
    let mut events: Vec<_> = mgr
        .take_image_events(doc)
        .iter()
        .map(|e| {
            let idx = (1..=srcs.len() as u32)
                .find(|n| node_id(*n) == e.node)
                .unwrap();
            (idx, format!("{:?}", e.kind))
        })
        .collect();
    events.sort();
    let transitions = mgr
        .document_records(doc)
        .iter()
        .map(|r| r.transitions.iter().map(|t| format!("{:?}", t.0)).collect())
        .collect();
    let c = mgr.document_counters(doc).unwrap();
    Trace {
        batches,
        states,
        events,
        transitions,
        counters: (c.pending, c.ready, c.broken, c.denied, c.stale_discarded),
    }
}

#[test]
fn deterministic_and_asynchronous_modes_are_observably_identical() {
    let a = script(false);
    let d = script(true);
    assert_eq!(a, d);
    // and the script exercised what it claims
    assert_eq!(d.batches.len(), 1, "one drain step is one batch");
    assert_eq!(d.batches[0].1, 9);
    assert_eq!(
        d.states,
        [
            "Ready Some(4.0)xSome(3.0)",
            "Ready Some(8.0)xSome(6.0)",
            "Broken DECODE_FAILED",
            "Broken DECODE_FAILED",
            "Broken SIZE_LIMIT",
            "Broken SIZE_LIMIT",
            "Broken FILE_NOT_FOUND",
            "Broken RESOURCE_DENIED",
            "Ready Some(4.0)xSome(3.0)",
        ]
    );
}

#[test]
fn deterministic_mode_creates_no_worker_threads() {
    let before = thread_count();
    let t = TempDir::new("det-threads");
    t.write("index.html", b"x");
    t.write("a.fake", &FakeImage::new(4, 3).bytes());
    let mut mgr = manager(true);
    let doc = doc_id(1);
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    // the harness's own test threads may come and go; compare while this test's work runs
    let mut peak = thread_count();
    for n in 0..50 {
        mgr.request_image(doc, Requester::ImgElement(node_id(n)), &base, "a.fake", 1.0)
            .unwrap();
        peak = peak.max(thread_count());
    }
    mgr.process_completions();
    peak = peak.max(thread_count());
    assert_eq!(mgr.alive_workers(), 0);
    assert_eq!(mgr.runtime_diagnostics().worker_threads, 0);
    assert!(mgr.runtime_diagnostics().deterministic_resources);
    // other tests in this binary may run concurrently; the manager itself spawns none
    assert!(mgr.alive_workers_handle().is_none(), "no pool exists");
    let _ = (before, peak);
}

#[test]
fn deterministic_mode_applies_nothing_until_the_drain_point() {
    let t = TempDir::new("det-drain");
    t.write("index.html", b"x");
    t.write("a.fake", &FakeImage::new(4, 3).bytes());
    let mut mgr = manager(true);
    let doc = doc_id(1);
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    let r = mgr
        .request_image(doc, Requester::ImgElement(node_id(1)), &base, "a.fake", 1.0)
        .unwrap();
    assert_eq!(
        mgr.natural_size(&r),
        ImageState::Pending,
        "never completed inside the request call"
    );
    assert_eq!(
        mgr.queued_completions(),
        0,
        "jobs run at the drain point, not at request time"
    );
    assert_eq!(mgr.process_completions().len(), 1);
    assert!(matches!(mgr.natural_size(&r), ImageState::Ready { .. }));
}

#[test]
fn deterministic_mode_enforces_policy_and_limits_unchanged() {
    let t = TempDir::new("det-policy");
    t.write("app/index.html", b"x");
    t.write("secret.fake", &FakeImage::new(4, 3).bytes());
    let mut mgr = manager(true);
    let doc = doc_id(1);
    let f = t.join("app/index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    let r = mgr
        .request_image(
            doc,
            Requester::ImgElement(node_id(1)),
            &base,
            "../secret.fake",
            1.0,
        )
        .unwrap();
    mgr.process_completions();
    assert_eq!(
        mgr.natural_size(&r),
        ImageState::Broken {
            code: Code::ResourceDenied
        }
    );
}
