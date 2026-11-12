// owner: 007-resources (T077, T080, T042) — cache control, purge, sharing, configuration operations.
#![cfg(unix)]

mod support;

use std::sync::atomic::Ordering;
use support::*;
use ui4me_assets::ResourceManager;
use ui4me_assets::cache::PurgeScope;
use ui4me_assets::decode::fake::FakeImage;
use ui4me_assets::policy::{MIB, ResourcePolicy, ResourcePolicyPatch};
use ui4me_assets::request::Requester;
use ui4me_assets::uri;
use ui4me_assets::view::ImageState;
use ui4me_engine_types::codes_generated::Code;
use url::Url;

struct World {
    t: TempDir,
    base: Url,
    file: std::path::PathBuf,
}

fn world() -> World {
    let t = TempDir::new("cache");
    t.write("index.html", b"x");
    for (n, side) in [("a", 64u32), ("b", 64), ("c", 64)] {
        t.write(
            &format!("{n}.fake"),
            &FakeImage::new(side, side)
                .color([n.as_bytes()[0], 0, 0, 255])
                .bytes(),
        );
    }
    let file = t.join("index.html");
    let base = uri::document_url_from_file(&file).unwrap();
    World { t, base, file }
}

fn mgr_with(
    w: &World,
    cache_budget: u64,
    counter: &std::sync::Arc<CountingDecoder>,
) -> ResourceManager {
    let defaults = ResourcePolicy::default()
        .merged(&ResourcePolicyPatch {
            cache_budget: Some(cache_budget),
            ..Default::default()
        })
        .unwrap();
    let modes = ui4me_assets::policy::RuntimeResourceModes {
        deterministic_resources: true,
        ..Default::default()
    };
    let _ = w;
    ResourceManager::new(modes, defaults, counter.clone()).unwrap()
}

fn doc(mgr: &mut ResourceManager, w: &World, n: u32) -> ui4me_engine_types::ids::DocumentId {
    let d = doc_id(n);
    mgr.create_document(d, w.base.clone(), Some(&w.file), &Default::default())
        .unwrap();
    d
}

fn load(
    mgr: &mut ResourceManager,
    d: ui4me_engine_types::ids::DocumentId,
    w: &World,
    node: u32,
    src: &str,
) {
    let r = mgr
        .request_image(d, Requester::ImgElement(node_id(node)), &w.base, src, 1.0)
        .unwrap();
    mgr.process_completions();
    assert!(
        matches!(mgr.natural_size(&r), ImageState::Ready { .. }),
        "{src}"
    );
}

#[test]
fn lru_evicts_unreferenced_entries_beyond_the_cache_budget() {
    let w = world();
    let counter = counting();
    // each image is 64*64*4 = 16 KiB; keep at most two unreferenced
    let mut mgr = mgr_with(&w, 32 * 1024, &counter);
    let d = doc(&mut mgr, &w, 1);
    // one element switching images: earlier images lose their reference and become evictable
    for src in ["a.fake", "b.fake", "c.fake"] {
        load(&mut mgr, d, &w, 1, src);
    }
    let s = mgr.cache_stats();
    assert_eq!(
        s.entries, 3,
        "c referenced, a and b unreferenced within the 32 KiB budget"
    );
    // a fourth distinct unreferenced entry pushes the least recently used one out
    t_write_more(&w);
    load(&mut mgr, d, &w, 1, "d.fake");
    let s = mgr.cache_stats();
    assert!(s.evictions >= 1, "{s:?}");
    assert!(s.bytes - s.referenced_bytes <= 32 * 1024);
    // a was the least recently used: loading it again decodes again, c/d are still cached
    let before = counter.decodes.load(Ordering::SeqCst);
    load(&mut mgr, d, &w, 2, "d.fake");
    assert_eq!(
        counter.decodes.load(Ordering::SeqCst),
        before,
        "still cached"
    );
    load(&mut mgr, d, &w, 3, "a.fake");
    assert_eq!(
        counter.decodes.load(Ordering::SeqCst),
        before + 1,
        "evicted entry is re-read"
    );
}

fn t_write_more(w: &World) {
    w.t.write(
        "d.fake",
        &FakeImage::new(64, 64).color([b'd', 0, 0, 255]).bytes(),
    );
}

#[test]
fn sharing_across_documents_and_refcounts() {
    let w = world();
    let counter = counting();
    let mut mgr = mgr_with(&w, 0, &counter);
    let (d1, d2) = (doc(&mut mgr, &w, 1), doc(&mut mgr, &w, 2));
    load(&mut mgr, d1, &w, 1, "a.fake");
    load(&mut mgr, d2, &w, 1, "a.fake");
    assert_eq!(counter.decodes.load(Ordering::SeqCst), 1);
    mgr.close_document(d1);
    assert_eq!(
        mgr.cache_stats().entries,
        1,
        "still referenced by the second document (budget 0 would evict otherwise)"
    );
    mgr.close_document(d2);
    assert_eq!(
        mgr.cache_stats().entries,
        0,
        "unreferenced and over the zero budget: evicted"
    );
}

#[test]
fn purge_by_url_by_document_and_for_the_runtime() {
    let w = world();
    let counter = counting();
    let mut mgr = mgr_with(&w, 10 * MIB, &counter);
    let (d1, d2) = (doc(&mut mgr, &w, 1), doc(&mut mgr, &w, 2));
    load(&mut mgr, d1, &w, 1, "a.fake");
    load(&mut mgr, d1, &w, 2, "b.fake");
    load(&mut mgr, d2, &w, 1, "c.fake");
    assert_eq!(mgr.cache_stats().entries, 3);
    // purge by URL (referenced entry is invalidated, so the next request re-reads)
    let a_url = w.t.url("a.fake");
    assert_eq!(
        mgr.resources_purge(PurgeScope::Runtime, Some(a_url.as_str())),
        1
    );
    assert_eq!(mgr.cache_stats().entries, 2);
    let before = counter.decodes.load(Ordering::SeqCst);
    load(&mut mgr, d1, &w, 3, "a.fake");
    assert_eq!(
        counter.decodes.load(Ordering::SeqCst),
        before + 1,
        "re-read after a purge"
    );
    // purge by document: the entries that document references
    assert_eq!(mgr.resources_purge(PurgeScope::Document(d2), None), 1);
    assert_eq!(mgr.cache_stats().entries, 2);
    // purge for the runtime
    assert_eq!(mgr.resources_purge(PurgeScope::Runtime, None), 2);
    assert_eq!(mgr.cache_stats().entries, 0);
    // published snapshots held by consumers stay valid
    assert!(matches!(
        mgr.natural_size(&ui4me_engine_types::resource_ids::ResourceRef {
            document: d1,
            request: ui4me_engine_types::resource_ids::RequestId {
                document: d1,
                serial: 1
            },
            revision: 1
        }),
        ImageState::Ready { .. }
    ));
}

#[test]
fn file_changes_are_picked_up_only_after_a_purge_or_new_identity() {
    // no file watching: a changed file has a new identity (size/mtime) and therefore a new key
    let w = world();
    let counter = counting();
    let mut mgr = mgr_with(&w, 10 * MIB, &counter);
    let d = doc(&mut mgr, &w, 1);
    load(&mut mgr, d, &w, 1, "a.fake");
    // identity is (size, mtime): let the filesystem's coarse timestamp clock tick so a same-size rewrite differs
    // (the test was flaky ~50 % without this: an immediate same-size rewrite can share the old mtime)
    std::thread::sleep(std::time::Duration::from_millis(50));
    w.t.write("a.fake", &FakeImage::new(80, 80).bytes());
    load(&mut mgr, d, &w, 2, "a.fake");
    assert_eq!(
        counter.decodes.load(Ordering::SeqCst),
        2,
        "new identity, new decode"
    );
}

#[test]
fn configuration_operations() {
    let w = world();
    let counter = counting();
    let mut mgr = mgr_with(&w, 10 * MIB, &counter);
    let d1 = doc(&mut mgr, &w, 1);
    // defaults changes affect only documents created afterwards
    mgr.resource_defaults_set(&ResourcePolicyPatch {
        data_url_max_bytes: Some(0),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        mgr.resource_policy_get(d1).unwrap().data_url_max_bytes,
        4 * MIB
    );
    let d2 = doc(&mut mgr, &w, 2);
    assert_eq!(mgr.resource_policy_get(d2).unwrap().data_url_max_bytes, 0);
    // per-document overrides apply at document creation, then freeze
    let d3 = doc_id(3);
    mgr.create_document(
        d3,
        w.base.clone(),
        None,
        &ResourcePolicyPatch {
            data_url_max_bytes: Some(1024),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        mgr.resource_policy_get(d3).unwrap().data_url_max_bytes,
        1024
    );
    // out-of-range values are INVALID_ARGUMENT and change nothing
    let e = mgr
        .resource_defaults_set(&ResourcePolicyPatch {
            max_decoded_pixels: Some(300_000_000),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(e.code, Code::InvalidArgument);
    let e = mgr
        .create_document(
            doc_id(4),
            w.base.clone(),
            None,
            &ResourcePolicyPatch {
                worker_threads: Some(99),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert_eq!(e.code, Code::InvalidArgument);
    assert_eq!(
        mgr.resource_policy_get(doc_id(4)).unwrap_err().code,
        Code::StaleHandle,
        "a failed create leaves no document"
    );
    // roots are redacted on export
    assert!(
        mgr.resource_policy_get(d1)
            .unwrap()
            .roots_redacted()
            .iter()
            .all(|r| r.starts_with("<root-"))
    );
    // there is no way to configure the network: the patch type has no such field (compile-time), and
    // the ABI layer maps a `network` option to INVALID_ARGUMENT (004).
    assert_eq!(
        mgr.resource_policy_get(doc_id(9)).unwrap_err().code,
        Code::StaleHandle
    );
}
