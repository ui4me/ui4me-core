// owner: 007-resources (T018, T021, T026, T027) — image request lifecycle (US1 acceptance 1–4).
//
// Uses the deterministic fake decoder and the fixtures in verification/fixtures/resources-images/.

mod support;

use std::sync::atomic::Ordering;
use support::*;
use ui4me_assets::cache::PurgeScope;
use ui4me_assets::diagnostics::{RequesterKind, StateName};
use ui4me_assets::request::{ImageEventKind, Requester};
use ui4me_assets::uri::{self, ResourceKind};
use ui4me_assets::view::{ImageState, PaintImage};
use ui4me_engine_types::codes_generated::Code;
use url::Url;

/// Materialise the fake-decoder corpus (verification/fixtures/resources-images/fake-corpus.json)
/// into a temp dir (tests never write into the repository).
fn stage() -> TempDir {
    let t = TempDir::new("images");
    materialise(&t, IMAGES_CORPUS);
    t
}

struct Page {
    mgr: ui4me_assets::ResourceManager,
    doc: ui4me_engine_types::ids::DocumentId,
    base: Url,
    _tmp: TempDir,
}

fn page(deterministic: bool) -> Page {
    let tmp = stage();
    let mut mgr = manager(deterministic);
    let doc = doc_id(1);
    let f = tmp.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    Page {
        mgr,
        doc,
        base,
        _tmp: tmp,
    }
}

impl Page {
    fn img(&mut self, node: u32, src: &str) -> ui4me_engine_types::resource_ids::ResourceRef {
        self.mgr
            .request_image(
                self.doc,
                Requester::ImgElement(node_id(node)),
                &self.base,
                src,
                1.0,
            )
            .unwrap()
    }
}

#[test]
fn delayed_image_pending_then_ready_with_one_load_event() {
    for det in [false, true] {
        let mut p = page(det);
        let r = p.img(1, "fake/delayed-300ms.fake");
        assert_eq!(
            p.mgr.natural_size(&r),
            ImageState::Pending,
            "no natural dimensions yet"
        );
        assert!(matches!(
            p.mgr.paint_image(&r, (4, 3)),
            PaintImage::NotReady
        ));
        assert_eq!(p.mgr.pending(p.doc).unwrap(), 1);
        let batches = settle(&mut p.mgr, p.doc);
        assert_eq!(batches.len(), 1, "one batch (det={det})");
        assert_eq!(batches[0].changed, vec![r]);
        assert_eq!(
            batches[0].intrinsic_size,
            vec![r],
            "first size is an intrinsic-size change"
        );
        assert_eq!(batches[0].resource_epoch.0, 1);
        assert_eq!(
            p.mgr.natural_size(&r),
            ImageState::Ready {
                width_css: Some(4.0),
                height_css: Some(3.0),
                ratio: Some((4.0, 3.0))
            }
        );
        let ev = p.mgr.take_image_events(p.doc);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, ImageEventKind::Load);
        let data = p.mgr.image_element_data(&r).unwrap();
        assert!(data.complete && data.failure.is_none());
        assert_eq!((data.natural_width, data.natural_height), (4, 3));
        match p.mgr.paint_image(&r, (4, 3)) {
            PaintImage::Raster { pixels, .. } => {
                assert_eq!((pixels.width, pixels.height, pixels.stride), (4, 3, 16));
                assert_eq!(&pixels.data[..4], &[255, 0, 0, 255]);
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn src_changed_twice_applies_only_the_last_revision() {
    for det in [false, true] {
        let mut p = page(det);
        let r1 = p.img(1, "fake/delayed-600ms.fake");
        let r2 = p.img(1, "fake/delayed-300ms.fake");
        let r3 = p.img(1, "fake/blue-8x6.fake");
        assert_eq!((r1.revision, r2.revision, r3.revision), (1, 2, 3));
        assert_eq!(r1.request, r3.request);
        let batches = settle(&mut p.mgr, p.doc);
        // give a stale completion time to arrive, then drain once more
        std::thread::sleep(std::time::Duration::from_millis(700));
        let mut more = p.mgr.process_completions();
        let mut all = batches;
        all.append(&mut more);
        assert_eq!(
            p.mgr.natural_size(&r3),
            ImageState::Ready {
                width_css: Some(8.0),
                height_css: Some(6.0),
                ratio: Some((8.0, 6.0))
            }
        );
        let ev = p.mgr.take_image_events(p.doc);
        assert_eq!(ev.len(), 1, "stale revisions produce no events (det={det})");
        assert_eq!(ev[0].revision, 3);
        let c = p.mgr.document_counters(p.doc).unwrap();
        assert_eq!(c.canceled, 2, "two superseded revisions");
        if !det {
            // the superseded jobs either never ran (canceled) or were discarded as stale
            assert!(c.stale_discarded <= 2);
        }
    }
}

#[test]
fn missing_file_denied_and_malformed_become_broken_with_error_events() {
    let mut p = page(false);
    let missing = p.img(1, "fake/nope.fake");
    let denied = p.img(2, "http://example.com/a.png");
    let malformed = p.img(3, "http://[::1");
    let data_off = {
        let mut m = manager(false);
        let d = doc_id(5);
        m.create_document(
            d,
            p.base.clone(),
            None,
            &ui4me_assets::policy::ResourcePolicyPatch {
                data_url_max_bytes: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        let r = m
            .request_image(
                d,
                Requester::ImgElement(node_id(1)),
                &p.base,
                "data:image/png;base64,AAAA",
                1.0,
            )
            .unwrap();
        m.natural_size(&r)
    };
    assert_eq!(
        data_off,
        ImageState::Broken {
            code: Code::ResourceDenied
        }
    );
    // resolution-time denials are terminal immediately and queue `error` at once
    assert_eq!(
        p.mgr.natural_size(&denied),
        ImageState::Broken {
            code: Code::ResourceDenied
        }
    );
    assert_eq!(
        p.mgr.natural_size(&malformed),
        ImageState::Broken {
            code: Code::UriMalformed
        }
    );
    assert_eq!(p.mgr.take_image_events(p.doc).len(), 2);
    let batches = settle(&mut p.mgr, p.doc);
    assert_eq!(
        p.mgr.natural_size(&missing),
        ImageState::Broken {
            code: Code::FileNotFound
        }
    );
    let changed: usize = batches.iter().map(|b| b.changed.len()).sum();
    assert_eq!(
        changed, 3,
        "every request changes observable state exactly once"
    );
    let ev = p.mgr.take_image_events(p.doc);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].kind, ImageEventKind::Error);
    let d = p.mgr.image_element_data(&missing).unwrap();
    assert!(d.complete);
    assert_eq!(d.failure, Some(Code::FileNotFound));
    let c = p.mgr.document_counters(p.doc).unwrap();
    assert_eq!((c.denied, c.broken, c.pending), (2, 3, 0));
}

#[test]
fn css_background_in_a_stylesheet_reports_diagnostics_and_no_events() {
    let mut p = page(true);
    let sheet = p
        .mgr
        .load_text_sync(p.doc, &p.base, "css/site.css", ResourceKind::Stylesheet)
        .unwrap();
    let text = String::from_utf8(sheet.bytes.to_vec()).unwrap();
    assert!(text.contains("green-16x12.fake"));
    // url() in a linked sheet resolves against the sheet's final URL
    let mut refs = Vec::new();
    for (i, url) in ["../fake/green-16x12.fake", "../fake/red-4x3.fake"]
        .iter()
        .enumerate()
    {
        refs.push(
            p.mgr
                .request_css_image(
                    p.doc,
                    node_id(i as u32 + 1),
                    "background-image",
                    0,
                    &sheet.final_url,
                    url,
                )
                .unwrap(),
        );
    }
    // the same (node, property, layer, url) de-duplicates
    let again = p
        .mgr
        .request_css_image(
            p.doc,
            node_id(1),
            "background-image",
            0,
            &sheet.final_url,
            "../fake/green-16x12.fake",
        )
        .unwrap();
    assert_eq!(again, refs[0]);
    settle(&mut p.mgr, p.doc);
    assert!(
        p.mgr.take_image_events(p.doc).is_empty(),
        "CSS image requests queue no events"
    );
    assert_eq!(
        p.mgr.natural_size(&refs[0]),
        ImageState::Ready {
            width_css: Some(16.0),
            height_css: Some(12.0),
            ratio: Some((16.0, 12.0))
        }
    );
    let recs = p.mgr.document_records(p.doc);
    assert_eq!(recs.len(), 2);
    assert!(
        recs.iter()
            .all(|r| r.requester == RequesterKind::CssImage && r.outcome == Code::Ok)
    );
    assert!(
        recs.iter().all(|r| r.url.starts_with("file://<redacted>/")),
        "paths are redacted on export"
    );
}

#[test]
fn five_hundred_elements_share_one_decode() {
    for det in [false, true] {
        let counter = counting();
        let mut mgr = manager_counting(det, counter.clone());
        let t = stage();
        let doc = doc_id(1);
        let f = t.join("index.html");
        let base = uri::document_url_from_file(&f).unwrap();
        mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
            .unwrap();
        let mut refs = Vec::new();
        for n in 0..500 {
            refs.push(
                mgr.request_image(
                    doc,
                    Requester::ImgElement(node_id(n)),
                    &base,
                    "fake/shared-logo.fake",
                    1.0,
                )
                .unwrap(),
            );
            // Exercise sharing within bounded admission, not an unbounded burst.
            if n % 32 == 31 {
                settle(&mut mgr, doc);
            }
        }
        settle(&mut mgr, doc);
        assert!(
            refs.iter()
                .all(|r| matches!(mgr.natural_size(r), ImageState::Ready { .. }))
        );
        assert_eq!(
            counter.decodes.load(Ordering::SeqCst),
            1,
            "one decode for 500 requesters (det={det})"
        );
        assert_eq!(mgr.cache_stats().entries, 1);
        assert_eq!(mgr.take_image_events(doc).len(), 500);
    }
}

#[test]
fn cache_hit_across_two_documents_still_passes_through_pending() {
    let counter = counting();
    let mut mgr = manager_counting(true, counter.clone());
    let t = stage();
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    let (a, b) = (doc_id(1), doc_id(2));
    for d in [a, b] {
        mgr.create_document(d, base.clone(), Some(&f), &Default::default())
            .unwrap();
    }
    let ra = mgr
        .request_image(
            a,
            Requester::ImgElement(node_id(1)),
            &base,
            "fake/red-4x3.fake",
            1.0,
        )
        .unwrap();
    settle(&mut mgr, a);
    assert!(matches!(mgr.natural_size(&ra), ImageState::Ready { .. }));
    let rb = mgr
        .request_image(
            b,
            Requester::ImgElement(node_id(1)),
            &base,
            "fake/red-4x3.fake",
            1.0,
        )
        .unwrap();
    // a hit is never visible synchronously (Clarification Q4)
    assert_eq!(mgr.natural_size(&rb), ImageState::Pending);
    assert_eq!(mgr.pending(b).unwrap(), 1);
    let batches = mgr.process_completions();
    assert_eq!(batches.len(), 1);
    assert!(matches!(mgr.natural_size(&rb), ImageState::Ready { .. }));
    assert_eq!(
        counter.decodes.load(Ordering::SeqCst),
        1,
        "the second document decoded nothing"
    );
    assert!(mgr.document_records(b)[0].flags.contains(&"cache_hit"));
}

#[test]
fn each_drain_step_is_one_batch_with_one_epoch_increment() {
    let mut p = page(true);
    let refs: Vec<_> = (0..3)
        .map(|n| {
            p.img(
                n + 1,
                [
                    "fake/red-4x3.fake",
                    "fake/blue-8x6.fake",
                    "fake/green-16x12.fake",
                ][n as usize],
            )
        })
        .collect();
    let batches = p.mgr.process_completions();
    assert_eq!(
        batches.len(),
        1,
        "three completions, one drain step, one batch (one relayout)"
    );
    assert_eq!(batches[0].changed, refs);
    assert_eq!(batches[0].resource_epoch.0, 1);
    assert!(
        p.mgr.process_completions().is_empty(),
        "nothing changed, no epoch increment"
    );
    assert_eq!(p.mgr.resource_epoch(p.doc).unwrap().0, 1);
    // a later request is a later batch
    p.img(4, "fake/red-4x3.fake");
    assert_eq!(p.mgr.process_completions()[0].resource_epoch.0, 2);
}

#[test]
fn density_corrected_natural_size_and_reselection_keeps_the_current_image() {
    let mut p = page(true);
    let r1 = p
        .mgr
        .request_image(
            p.doc,
            Requester::ImgElement(node_id(1)),
            &p.base,
            "fake/logo@1x.fake",
            1.0,
        )
        .unwrap();
    p.mgr.process_completions();
    assert_eq!(
        p.mgr.natural_size(&r1),
        ImageState::Ready {
            width_css: Some(100.0),
            height_css: Some(50.0),
            ratio: Some((100.0, 50.0))
        }
    );
    // DPR 2 selects the 2x candidate: a new revision; the 1x image stays displayed until it is Ready
    let r2 = p
        .mgr
        .request_image(
            p.doc,
            Requester::ImgElement(node_id(1)),
            &p.base,
            "fake/logo@2x.fake",
            2.0,
        )
        .unwrap();
    assert_eq!(r2.revision, 2);
    assert_eq!(
        p.mgr.natural_size(&r2),
        ImageState::Ready {
            width_css: Some(100.0),
            height_css: Some(50.0),
            ratio: Some((100.0, 50.0))
        },
        "previous image still displayed"
    );
    assert!(
        matches!(p.mgr.paint_image(&r2, (100, 50)), PaintImage::Raster { pixels, .. } if pixels.width == 100)
    );
    let b = p.mgr.process_completions();
    assert_eq!(b.len(), 1);
    // 200x100 image at density 2 = the same 100x50 CSS size: pixels change, geometry does not
    assert_eq!(b[0].paint_only, vec![r2]);
    assert!(b[0].intrinsic_size.is_empty());
    assert!(
        matches!(p.mgr.paint_image(&r2, (100, 50)), PaintImage::Raster { pixels, .. } if pixels.width == 200)
    );
}

#[test]
fn failures_do_not_kill_workers_and_panics_become_decode_failed() {
    for det in [false, true] {
        let mut p = page(det);
        let bad = p.img(1, "fake/panics.fake");
        let fails = p.img(2, "fake/fails-decode.fake");
        settle(&mut p.mgr, p.doc);
        assert_eq!(
            p.mgr.natural_size(&bad),
            ImageState::Broken {
                code: Code::DecodeFailed
            }
        );
        assert_eq!(
            p.mgr.natural_size(&fails),
            ImageState::Broken {
                code: Code::DecodeFailed
            }
        );
        let ok = p.img(3, "fake/red-4x3.fake");
        settle(&mut p.mgr, p.doc);
        assert!(
            matches!(p.mgr.natural_size(&ok), ImageState::Ready { .. }),
            "the pool survives a decoder panic (det={det})"
        );
        if !det {
            assert_eq!(p.mgr.alive_workers(), 2);
        }
    }
}

#[test]
fn animation_icc_and_orientation_are_handled_and_reported() {
    let mut p = page(true);
    let anim = p.img(1, "fake/animated.fake");
    let icc = p.img(2, "fake/icc.fake");
    let rot = p.img(3, "fake/orient6-6x2.fake");
    p.mgr.process_completions();
    let recs = p.mgr.document_records(p.doc);
    assert!(
        recs.iter()
            .any(|r| r.flags.contains(&"animated_first_frame"))
    );
    assert!(recs.iter().any(|r| r.flags.contains(&"icc_ignored")));
    let un = p.mgr.unsupported_reports();
    assert_eq!(
        un.iter().filter(|u| u.key == "img.animation").count(),
        1,
        "reported once per resource"
    );
    assert!(
        matches!(p.mgr.natural_size(&anim), ImageState::Ready { .. })
            && matches!(p.mgr.natural_size(&icc), ImageState::Ready { .. })
    );
    // EXIF orientation 6 swaps the natural size
    assert_eq!(
        p.mgr.natural_size(&rot),
        ImageState::Ready {
            width_css: Some(2.0),
            height_css: Some(6.0),
            ratio: Some((2.0, 6.0))
        }
    );
}

#[test]
fn document_close_cancels_requests_and_suppresses_events() {
    let mut p = page(false);
    let r = p.img(1, "fake/delayed-300ms.fake");
    p.mgr.close_document(p.doc);
    assert_eq!(
        p.mgr.natural_size(&r),
        ImageState::Broken {
            code: Code::DocumentClosed
        }
    );
    assert_eq!(p.mgr.pending(p.doc).unwrap_err().code, Code::DocumentClosed);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let b = p.mgr.process_completions();
    assert!(b.is_empty(), "no batch for a closed document");
    assert!(p.mgr.take_image_events(p.doc).is_empty());
}

#[test]
fn pending_images_never_block_and_settled_callback_fires_once() {
    let mut p = page(true);
    let fired = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    p.img(1, "fake/red-4x3.fake");
    let f = fired.clone();
    p.mgr
        .on_resources_settled(
            p.doc,
            Box::new(move || {
                f.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
    assert_eq!(fired.load(Ordering::SeqCst), 0);
    p.mgr.process_completions();
    assert_eq!(fired.load(Ordering::SeqCst), 1);
    p.mgr.process_completions();
    assert_eq!(fired.load(Ordering::SeqCst), 1, "fires once");
}

#[test]
fn diagnostic_records_carry_monotonic_transitions_and_canonical_outcomes() {
    let mut p = page(true);
    p.img(1, "fake/red-4x3.fake");
    p.img(2, "fake/nope.fake");
    p.mgr.process_completions();
    let recs = p.mgr.document_records(p.doc);
    assert_eq!(recs.len(), 2);
    for r in &recs {
        let names: Vec<_> = r.transitions.iter().map(|t| t.0).collect();
        assert_eq!(names[0], StateName::Resolving);
        assert!(
            r.transitions.windows(2).all(|w| w[0].1 <= w[1].1),
            "monotonic times"
        );
    }
    let outcomes: Vec<_> = recs.iter().map(|r| r.outcome).collect();
    assert_eq!(outcomes, vec![Code::Ok, Code::FileNotFound]);
    assert_eq!(p.mgr.diagnostic_records().len(), 2);
}

#[test]
fn purge_drops_unreferenced_and_invalidates_referenced_entries() {
    let counter = counting();
    let mut mgr = manager_counting(true, counter.clone());
    let t = stage();
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    let doc = doc_id(1);
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    mgr.request_image(
        doc,
        Requester::ImgElement(node_id(1)),
        &base,
        "fake/red-4x3.fake",
        1.0,
    )
    .unwrap();
    mgr.process_completions();
    assert_eq!(mgr.cache_stats().entries, 1);
    assert_eq!(mgr.resources_purge(PurgeScope::Runtime, None), 1);
    assert_eq!(mgr.cache_stats().entries, 0);
    // re-requested on next use: a new requester decodes again
    mgr.request_image(
        doc,
        Requester::ImgElement(node_id(2)),
        &base,
        "fake/red-4x3.fake",
        1.0,
    )
    .unwrap();
    mgr.process_completions();
    assert_eq!(counter.decodes.load(Ordering::SeqCst), 2);
}

#[test]
fn excluded_requesters_are_reported_once_per_element_and_load_nothing() {
    use ui4me_assets::request::ExcludedRequester as X;
    let mut p = page(true);
    for kind in [
        X::ScriptSrc,
        X::Iframe,
        X::Object,
        X::Embed,
        X::Video,
        X::Audio,
        X::Source,
        X::Track,
        X::InputTypeImage,
        X::LinkRelOther,
        X::SrcsetWidthDescriptors,
        X::PictureSourceSelection,
    ] {
        p.mgr.report_excluded_requester(p.doc, node_id(40), kind);
        p.mgr.report_excluded_requester(p.doc, node_id(40), kind); // once per element
    }
    p.mgr.report_excluded_rule(7, X::CursorUrl);
    p.mgr.report_excluded_rule(7, X::CursorUrl);
    let reports = p.mgr.unsupported_reports();
    assert_eq!(reports.len(), 13);
    assert!(reports.iter().all(|r| r.key.starts_with("html.")));
    assert!(
        p.mgr.process_completions().is_empty(),
        "nothing is requested"
    );
    assert_eq!(p.mgr.pending(p.doc).unwrap(), 0);
}

#[test]
fn srcset_density_selection_and_reselection_on_scale_change() {
    use ui4me_assets::request::{SrcsetCandidate, SrcsetParse, parse_srcset, select_candidate};
    let c = |u: &str, d: f32| SrcsetCandidate {
        url: u.into(),
        density: d,
    };
    assert_eq!(
        parse_srcset("a.png, b.png 2x,c.png 1.5x"),
        SrcsetParse::Densities(vec![c("a.png", 1.0), c("b.png", 2.0), c("c.png", 1.5)])
    );
    assert_eq!(
        parse_srcset("a.png 100w, b.png 200w"),
        SrcsetParse::WidthDescriptors
    );
    assert_eq!(
        parse_srcset("a.png 0x, b.png bogus, c.png 2x"),
        SrcsetParse::Densities(vec![c("c.png", 2.0)])
    );
    let SrcsetParse::Densities(cands) = parse_srcset("one.png 1x, two.png 2x, three.png 3x") else {
        panic!()
    };
    for (dpr, want) in [
        (1.0, "one.png"),
        (1.25, "two.png"),
        (2.0, "two.png"),
        (2.5, "three.png"),
        (4.0, "three.png"),
        (0.5, "one.png"),
    ] {
        assert_eq!(
            select_candidate(cands.clone(), None, dpr).unwrap().url,
            want,
            "dpr {dpr}"
        );
    }
    // src is a 1x candidate unless a 1x candidate is listed
    assert_eq!(
        select_candidate(vec![c("two.png", 2.0)], Some("src.png"), 1.0)
            .unwrap()
            .url,
        "src.png"
    );
    assert_eq!(
        select_candidate(vec![c("one.png", 1.0)], Some("src.png"), 1.0)
            .unwrap()
            .url,
        "one.png"
    );
    assert!(select_candidate(vec![], None, 1.0).is_none());

    // end to end: a scale change selects another candidate and starts a new revision
    let mut p = page(true);
    let srcset = "fake/logo@1x.fake 1x, fake/logo@2x.fake 2x";
    let r1 = p
        .mgr
        .request_img(p.doc, node_id(1), &p.base, None, Some(srcset), 1.0)
        .unwrap()
        .unwrap();
    p.mgr.process_completions();
    assert_eq!(
        p.mgr.natural_size(&r1),
        ImageState::Ready {
            width_css: Some(100.0),
            height_css: Some(50.0),
            ratio: Some((100.0, 50.0))
        }
    );
    let r2 = p
        .mgr
        .request_img(p.doc, node_id(1), &p.base, None, Some(srcset), 2.0)
        .unwrap()
        .unwrap();
    assert_eq!(r2.revision, 2, "another candidate: a new revision");
    p.mgr.process_completions();
    assert_eq!(
        p.mgr.natural_size(&r2),
        ImageState::Ready {
            width_css: Some(100.0),
            height_css: Some(50.0),
            ratio: Some((200.0, 100.0))
        },
        "density-corrected: 200x100 at 2x (same aspect ratio)"
    );
    // the same candidate again is not a new revision
    let r3 = p
        .mgr
        .request_img(p.doc, node_id(1), &p.base, None, Some(srcset), 2.0)
        .unwrap()
        .unwrap();
    assert_eq!(r3.revision, 2);
    // width descriptors load nothing and are reported; src is the fallback
    let w = p
        .mgr
        .request_img(
            p.doc,
            node_id(2),
            &p.base,
            Some("fake/red-4x3.fake"),
            Some("a.png 100w, b.png 200w"),
            1.0,
        )
        .unwrap()
        .unwrap();
    p.mgr.process_completions();
    assert!(matches!(p.mgr.natural_size(&w), ImageState::Ready { .. }));
    assert!(
        p.mgr
            .unsupported_reports()
            .iter()
            .any(|u| u.key == "html.srcset-width")
    );
    assert!(
        p.mgr
            .request_img(p.doc, node_id(3), &p.base, None, None, 1.0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn inline_svg_is_reported_once_per_root() {
    let mut p = page(true);
    p.mgr.report_inline_svg(p.doc, node_id(50));
    p.mgr.report_inline_svg(p.doc, node_id(50));
    p.mgr.report_inline_svg(p.doc, node_id(51));
    let r: Vec<_> = p
        .mgr
        .unsupported_reports()
        .into_iter()
        .filter(|u| u.key == "html.inline-svg")
        .collect();
    assert_eq!(r.len(), 2);
}

// ---- T030 / T032: the real codecs (image-rs adapter) on verification/fixtures/resources-images ----

#[allow(clippy::chunks_exact_to_as_chunks)]
mod codecs {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use ui4me_assets::decode::image_rs::ImageRsDecoder;
    use ui4me_assets::decode::sniff::{self, Detected};
    use ui4me_assets::decode::{DecodedImage, ImageFormat, ImageHeader, RasterDecoder};
    use ui4me_assets::policy::{ResourcePolicy, RuntimeResourceModes};
    use ui4me_assets::worker::CancelToken;

    fn file(name: &str) -> Vec<u8> {
        let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/resources-images/natural-sizes/codecs")
            .join(name);
        std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    pub fn decode(name: &str) -> (ImageHeader, DecodedImage) {
        let b = file(name);
        let Detected::Raster(f) = sniff::detect(&b, false).unwrap() else {
            panic!("svg")
        };
        let d = ImageRsDecoder;
        let h = d.probe(f, &b).unwrap();
        let img = d
            .decode(&b, &h, &CancelToken::new())
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        (h, img)
    }

    pub fn px(img: &DecodedImage, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width + x) * 4) as usize;
        [
            img.data[i],
            img.data[i + 1],
            img.data[i + 2],
            img.data[i + 3],
        ]
    }

    #[test]
    fn codec_png_bmp_and_lossless_webp_decode_exactly() {
        // fixture pixel (x, y) = (x*255/(w-1), y*255/(h-1), (x+y) % 256), opaque (generate.py)
        for (name, w, h) in [
            ("png-200x100.png", 200, 100),
            ("bmp-32x16.bmp", 32, 16),
            ("webp-lossless-64x32.webp", 64, 32),
        ] {
            let (hd, img) = decode(name);
            assert_eq!(
                (hd.width, hd.height, img.width, img.height),
                (w, h, w, h),
                "{name}"
            );
            assert_eq!(img.data.len() as u32, w * h * 4);
            assert_eq!(px(&img, 0, 0), [0, 0, 0, 255], "{name}");
            let (x, y) = (w - 1, h - 1);
            assert_eq!(
                px(&img, x, y),
                [255, 255, ((x + y) % 256) as u8, 255],
                "{name}"
            );
        }
    }

    #[test]
    fn codec_16_bit_png_is_reduced_to_8_bit_grey() {
        let (_, img) = decode("png16-8x8.png");
        assert_eq!(
            px(&img, 3, 3),
            [156, 156, 156, 255],
            "40000/65535 reduced to 8 bit"
        );
    }

    #[test]
    fn codec_alpha_is_premultiplied() {
        let (_, img) = decode("webp-alpha-64x32.webp");
        assert_eq!(px(&img, 0, 0)[3], 128);
        assert_eq!(px(&img, 1, 0)[3], 228);
        for p in img.data.chunks_exact(4) {
            assert!(
                p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3],
                "premultiplied channels never exceed alpha"
            );
        }
    }

    #[test]
    fn codec_jpeg_decodes_in_stored_orientation_and_ignores_icc() {
        let (h, img) = decode("jpeg-progressive-exif6-120x80.jpg");
        assert_eq!(
            (img.width, img.height, h.orientation),
            (120, 80, 6),
            "stored size; the pipeline applies EXIF"
        );
        let (h, img) = decode("jpeg-icc-64x48.jpg");
        assert!(h.has_icc);
        assert_eq!((img.width, img.height), (64, 48));
    }

    #[test]
    fn codec_animated_files_yield_the_first_frame_only() {
        for (name, w, h) in [
            ("gif-animated-24x24.gif", 24, 24),
            ("apng-anim.png", 32, 16),
            ("webp-animated-32x16.webp", 32, 16),
        ] {
            let (hd, img) = decode(name);
            assert!(hd.animated, "{name}");
            assert_eq!((img.width, img.height), (w, h), "{name}");
        }
    }

    #[test]
    fn codec_corrupt_and_foreign_data_map_to_canonical_codes() {
        let d = ImageRsDecoder;
        let b = file("truncated.png");
        let h = d.probe(ImageFormat::Png, &b).unwrap();
        assert_eq!(
            d.decode(&b, &h, &CancelToken::new()).unwrap_err().code,
            Code::DecodeFailed
        );
        assert_eq!(
            sniff::detect(&file("unsupported-tiff-16x16.tiff"), false)
                .unwrap_err()
                .code,
            Code::UnsupportedMediaType
        );
        // a header that lies about its size is refused by the decoder limits, not by an allocation
        let mut b = file("png-200x100.png");
        let h = d.probe(ImageFormat::Png, &b).unwrap();
        b[16..20].copy_from_slice(&5000u32.to_be_bytes());
        let e = d.decode(&b, &h, &CancelToken::new()).unwrap_err();
        assert!(
            matches!(e.code, Code::DecodeFailed | Code::SizeLimit),
            "{e:?}"
        );
    }

    fn real_page(
        files: &[&str],
    ) -> (
        ui4me_assets::ResourceManager,
        ui4me_engine_types::ids::DocumentId,
        url::Url,
        TempDir,
    ) {
        let tmp = TempDir::new("codecs");
        for f in files {
            tmp.write(f, &file(f));
        }
        let modes = RuntimeResourceModes {
            deterministic_resources: true,
            ..Default::default()
        };
        let mut mgr = ui4me_assets::ResourceManager::new(
            modes,
            ResourcePolicy::default(),
            Arc::new(ImageRsDecoder),
        )
        .unwrap();
        let doc = doc_id(1);
        let f = tmp.write("index.html", b"<html></html>");
        let base = uri::document_url_from_file(&f).unwrap();
        mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
            .unwrap();
        (mgr, doc, base, tmp)
    }

    #[test]
    fn codec_manager_loads_real_files_and_reports_animation_once() {
        let (mut mgr, doc, base, _tmp) = real_page(&[
            "png-200x100.png",
            "gif-animated-24x24.gif",
            "jpeg-progressive-exif6-120x80.jpg",
            "truncated.png",
        ]);
        let r = mgr
            .request_image(
                doc,
                Requester::ImgElement(node_id(1)),
                &base,
                "png-200x100.png",
                1.0,
            )
            .unwrap();
        let g = mgr
            .request_image(
                doc,
                Requester::ImgElement(node_id(2)),
                &base,
                "gif-animated-24x24.gif",
                1.0,
            )
            .unwrap();
        let j = mgr
            .request_image(
                doc,
                Requester::ImgElement(node_id(3)),
                &base,
                "jpeg-progressive-exif6-120x80.jpg",
                1.0,
            )
            .unwrap();
        let t = mgr
            .request_image(
                doc,
                Requester::ImgElement(node_id(4)),
                &base,
                "truncated.png",
                1.0,
            )
            .unwrap();
        settle(&mut mgr, doc);
        assert_eq!(
            mgr.natural_size(&r),
            ImageState::Ready {
                width_css: Some(200.0),
                height_css: Some(100.0),
                ratio: Some((200.0, 100.0))
            }
        );
        assert!(matches!(mgr.natural_size(&g), ImageState::Ready { .. }));
        assert_eq!(
            mgr.natural_size(&j),
            ImageState::Ready {
                width_css: Some(80.0),
                height_css: Some(120.0),
                ratio: Some((80.0, 120.0))
            },
            "orientation 6 rotates the natural size"
        );
        assert_eq!(
            mgr.natural_size(&t),
            ImageState::Broken {
                code: Code::DecodeFailed
            }
        );
        assert_eq!(
            mgr.unsupported_reports()
                .iter()
                .filter(|u| u.key == "img.animation")
                .count(),
            1
        );
    }
}
