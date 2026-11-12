//! Focused 0.2 resource admission and retirement regressions.
mod support;

use std::collections::HashSet;
use std::sync::{Arc, Barrier};
use std::time::Instant;
use support::*;
use ui4me_assets::decode::fake::FakeImage;
use ui4me_assets::request::Requester;
use ui4me_assets::view::ImageState;
use ui4me_assets::worker::{Completion, CompletionQueue, JobOutcome, MAX_ADMITTED_JOBS};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::resource_ids::RequestId;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

#[test]
fn rejected_asset_admission_settles_as_a_failure() {
    let files = TempDir::new("admission");
    let document_file = files.write("index.html", b"<img>");
    files.write("image.fake", &FakeImage::new(4, 3).bytes());
    let base = files.url("index.html");
    let document = doc_id(0);
    let mut resources = manager(true);
    resources
        .create_document(
            document,
            base.clone(),
            Some(&document_file),
            &Default::default(),
        )
        .unwrap();
    let requests: Vec<_> = (0..MAX_ADMITTED_JOBS + 1)
        .map(|n| {
            resources
                .request_image(
                    document,
                    Requester::ImgElement(node_id(n as u32)),
                    &base,
                    "image.fake",
                    1.0,
                )
                .unwrap()
        })
        .collect();
    resources.process_completions();
    assert!(matches!(
        resources.natural_size(&requests[0]),
        ImageState::Ready { .. }
    ));
    assert_eq!(
        resources.natural_size(requests.last().unwrap()),
        ImageState::Broken {
            code: Code::BudgetExceeded
        }
    );
    assert_eq!(resources.pending(document).unwrap(), 0);
}

#[test]
fn removed_node_generation_releases_pending_and_loaded_interests() {
    let files = TempDir::new("retire");
    let document_file = files.write("index.html", b"<img>");
    files.write("image.fake", &FakeImage::new(4, 3).bytes());
    let base = files.url("index.html");
    let document = doc_id(0);
    let mut resources = manager(true);
    resources
        .create_document(
            document,
            base.clone(),
            Some(&document_file),
            &Default::default(),
        )
        .unwrap();
    let loaded = resources
        .request_image(
            document,
            Requester::ImgElement(node_id(1)),
            &base,
            "image.fake",
            1.0,
        )
        .unwrap();
    resources.process_completions();
    let pending = resources
        .request_image(
            document,
            Requester::ImgElement(node_id(2)),
            &base,
            "image.fake",
            1.0,
        )
        .unwrap();
    resources.retain_nodes(document, &HashSet::new());
    resources.process_completions();
    assert!(!resources.request_id_known(loaded.request));
    assert!(!resources.request_id_known(pending.request));
    assert_eq!(resources.pending(document).unwrap(), 0);
    assert_eq!(resources.cache_stats().referenced_bytes, 0);
}

#[test]
fn completion_publication_cannot_reopen_a_closed_mailbox() {
    let queue = CompletionQueue::new();
    let barrier = Arc::new(Barrier::new(2));
    let producer = queue.clone();
    let ready = barrier.clone();
    let worker = std::thread::spawn(move || {
        ready.wait();
        for serial in 0..256 {
            let now = Instant::now();
            let _ = producer.post(Completion {
                request: RequestId {
                    document: doc_id(0),
                    serial,
                },
                revision: 1,
                outcome: JobOutcome::Failed(Status::new(Code::DecodeFailed, Stage::Resource)),
                finished: now,
                deadline: now,
            });
        }
    });
    barrier.wait();
    queue.close();
    worker.join().unwrap();
    assert!(queue.is_empty());
    assert!(!queue.has_capacity());
}

#[test]
fn new_sessions_reuse_the_same_bounded_process_workers() {
    let files = TempDir::new("pool-reuse");
    let file = files.write("index.html", b"x");
    for _ in 0..8 {
        let mut resources = manager(false);
        resources
            .create_document(
                doc_id(0),
                files.url("index.html"),
                Some(&file),
                &Default::default(),
            )
            .unwrap();
        assert_eq!(resources.alive_workers(), 2);
        resources.shutdown();
        // These are process workers, not per-session threads awaiting a join.
        assert_eq!(resources.alive_workers(), 2);
    }
}
