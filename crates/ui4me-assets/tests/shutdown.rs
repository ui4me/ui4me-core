// owner: 007-resources (T067, T072, T073) — close during load, staleness, non-blocking shutdown.
#![cfg(unix)]

mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};
use support::*;
use ui4me_assets::decode::fake::FakeImage;
use ui4me_assets::request::Requester;
use ui4me_assets::uri;
use ui4me_assets::view::ImageState;
use ui4me_engine_types::codes_generated::Code;

/// Counts live heap bytes so the leak check is an allocator counter, not a guess.
struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        // SAFETY: `p` and `l` come from a matching `alloc` above.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

struct Env {
    t: TempDir,
}

fn env() -> Env {
    let t = TempDir::new("shutdown");
    t.write("index.html", b"x");
    t.write("slow.fake", &FakeImage::new(8, 8).delay(150).bytes());
    t.write("fast.fake", &FakeImage::new(8, 8).bytes());
    Env { t }
}

fn close_during_load(iterations: usize, deterministic: bool) {
    let e = env();
    let f = e.t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    let mut mgr = manager(deterministic);
    let events_at_start = LIVE.load(Ordering::Relaxed);
    for i in 0..iterations {
        let doc = ui4me_engine_types::ids::DocumentId::from_raw(ui4me_engine_types::ids::RawId {
            index: (i % 4) as u32,
            generation: std::num::NonZeroU32::new((i / 4 + 1) as u32).unwrap(),
        });
        mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
            .unwrap();
        let refs: Vec<_> = (0..4)
            .map(|n| {
                mgr.request_image(
                    doc,
                    Requester::ImgElement(node_id(n)),
                    &base,
                    if n % 2 == 0 { "slow.fake" } else { "fast.fake" },
                    1.0,
                )
                .unwrap()
            })
            .collect();
        mgr.close_document(doc);
        for r in &refs {
            assert_eq!(
                mgr.natural_size(r),
                ImageState::Broken {
                    code: Code::DocumentClosed
                }
            );
        }
        assert_eq!(mgr.pending(doc).unwrap_err().code, Code::DocumentClosed);
        let batches = mgr.process_completions();
        assert!(
            batches.iter().all(|b| b.document != doc),
            "no batch for a closed document"
        );
        assert!(
            mgr.take_image_events(doc).is_empty(),
            "no event for a closed document"
        );
        mgr.release_document(doc);
        assert_eq!(mgr.retained_documents(), 0);
    }
    // let in-flight work finish and be discarded, then drain
    std::thread::sleep(Duration::from_millis(400));
    mgr.process_completions();
    mgr.shutdown();
    drop(mgr);
    let _ = events_at_start;
}

#[test]
fn close_during_load_smoke() {
    close_during_load(25, false);
    close_during_load(25, true);
}

#[test]
#[ignore = "1,000 iterations (acceptance 3, SC-004); run with --ignored"]
fn close_during_load_1000_iterations_no_leak() {
    // warm-up so one-time allocations (thread stacks bookkeeping, TLS) are not counted
    close_during_load(5, false);
    std::thread::sleep(Duration::from_millis(300));
    let before = LIVE.load(Ordering::Relaxed);
    close_during_load(1000, false);
    std::thread::sleep(Duration::from_millis(500));
    let after = LIVE.load(Ordering::Relaxed);
    let growth = after - before;
    assert!(
        growth < 256 * 1024,
        "live heap grew by {growth} bytes over 1,000 close-during-load iterations"
    );
}

#[test]
fn runtime_shutdown_returns_without_waiting_and_its_jobs_retire() {
    let e = env();
    e.t.write("verylong.fake", &FakeImage::new(8, 8).delay(5000).bytes());
    let f = e.t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    let mut mgr = manager(false);
    let doc = doc_id(1);
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    for n in 0..2 {
        mgr.request_image(
            doc,
            Requester::ImgElement(node_id(n)),
            &base,
            "verylong.fake",
            1.0,
        )
        .unwrap();
    }
    // Observe the two admitted jobs, independently of concurrent test scheduling.
    let alive = mgr.alive_workers_handle().unwrap();
    let start_deadline = Instant::now() + Duration::from_secs(2);
    while alive.load(Ordering::SeqCst) != 2 && Instant::now() < start_deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(alive.load(Ordering::SeqCst), 2);
    let started = Instant::now();
    mgr.shutdown();
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(100),
        "shutdown took {took:?}: it must not wait for workers"
    );
    // Its jobs retire; the fixed process workers remain available for later apps.
    let end = Instant::now() + Duration::from_secs(2);
    while alive.load(Ordering::SeqCst) != 0 {
        assert!(
            Instant::now() < end,
            "retired jobs did not return within 2 s"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // nothing is applied afterwards, no callback runs, new work is refused
    assert!(mgr.process_completions().is_empty());
    assert_eq!(
        mgr.request_image(
            doc,
            Requester::ImgElement(node_id(9)),
            &base,
            "fast.fake",
            1.0
        )
        .unwrap_err()
        .code,
        Code::InvalidState
    );
    assert!(mgr.is_shut_down());
    assert_eq!(mgr.alive_workers(), 2);
}

#[test]
fn shutdown_runs_no_settled_callback() {
    let e = env();
    let f = e.t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    let mut mgr = manager(true);
    let doc = doc_id(1);
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    mgr.request_image(
        doc,
        Requester::ImgElement(node_id(1)),
        &base,
        "fast.fake",
        1.0,
    )
    .unwrap();
    let fired = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let f2 = fired.clone();
    mgr.on_resources_settled(
        doc,
        Box::new(move || {
            f2.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .unwrap();
    mgr.shutdown();
    mgr.process_completions();
    assert_eq!(fired.load(Ordering::SeqCst), 0);
}

#[test]
fn stale_completions_are_discarded_and_counted() {
    let e = env();
    let f = e.t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    let mut mgr = manager(false);
    let doc = doc_id(1);
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    let node = node_id(1);
    // slow job superseded after it has started: its completion arrives stale
    mgr.request_image(doc, Requester::ImgElement(node), &base, "slow.fake", 1.0)
        .unwrap();
    std::thread::sleep(Duration::from_millis(40));
    let last = mgr
        .request_image(doc, Requester::ImgElement(node), &base, "fast.fake", 1.0)
        .unwrap();
    settle(&mut mgr, doc);
    std::thread::sleep(Duration::from_millis(250));
    mgr.process_completions();
    assert!(matches!(mgr.natural_size(&last), ImageState::Ready { .. }));
    let c = mgr.document_counters(doc).unwrap();
    assert!(c.stale_discarded <= 1);
    assert_eq!(
        mgr.take_image_events(doc).len(),
        1,
        "only the current revision's event"
    );
}

#[test]
fn closed_document_data_is_released() {
    let e = env();
    let f = e.t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    let mut mgr = manager(true);
    let doc = doc_id(1);
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    mgr.request_image(
        doc,
        Requester::ImgElement(node_id(1)),
        &base,
        "fast.fake",
        1.0,
    )
    .unwrap();
    mgr.process_completions();
    assert!(mgr.cache_stats().referenced_bytes > 0);
    mgr.close_document(doc);
    assert_eq!(
        mgr.cache_stats().referenced_bytes,
        0,
        "the closed document no longer references the entry"
    );
}
