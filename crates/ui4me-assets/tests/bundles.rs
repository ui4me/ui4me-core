// owner: 007-resources (T075, T076, T078) — `ui4me-res:` bundles (OD-19, APPROVED under E-029).
//
// Fixtures: verification/fixtures/resources-bundles/bundle-corpus.json. The deterministic fake
// decoder decodes `.fake` images; SVG goes through usvg. The scheme is unix-independent except
// for the symbolic-link case.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use support::*;
use ui4me_assets::ResourceManager;
use ui4me_assets::bundles::{self, BundleEntryInput};
use ui4me_assets::decode::fake::FakeImage;
use ui4me_assets::policy::{self, Decision, ResourcePolicy, ResourcePolicyPatch};
use ui4me_assets::request::Requester;
use ui4me_assets::text::TextIdentity;
use ui4me_assets::uri::{self, ResourceKind, Scheme};
use ui4me_assets::view::ImageState;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::DocumentId;
use url::Url;

fn u(s: &str) -> Url {
    Url::parse(s).unwrap()
}

struct Env {
    mgr: ResourceManager,
    counter: Arc<CountingDecoder>,
    doc: DocumentId,
    base: Url,
}

fn env() -> Env {
    let counter = counting();
    let mut mgr = manager_counting(true, counter.clone());
    mgr.bundle_register("app", bundle_inputs("app")).unwrap();
    let doc = doc_id(1);
    let base = u("ui4me-res://app/index.html");
    mgr.create_document(doc, base.clone(), None, &ResourcePolicyPatch::default())
        .unwrap();
    Env {
        mgr,
        counter,
        doc,
        base,
    }
}

impl Env {
    fn img(&mut self, node: u32, src: &str) -> ui4me_engine_types::resource_ids::ResourceRef {
        let r = self
            .mgr
            .request_image(
                self.doc,
                Requester::ImgElement(node_id(node)),
                &self.base.clone(),
                src,
                1.0,
            )
            .unwrap();
        self.mgr.process_completions();
        r
    }
    fn size(&self, r: &ui4me_engine_types::resource_ids::ResourceRef) -> ImageState {
        self.mgr.natural_size(r)
    }
}

fn ready(w: f32, h: f32) -> ImageState {
    ImageState::Ready {
        width_css: Some(w),
        height_css: Some(h),
        ratio: Some((w, h)),
    }
}

/// T075: the recorded owner decision (approved, E-029, 2026-11-06) and the behaviour it implies.
#[test]
fn scheme_matches_owner_decision() {
    let p = ResourcePolicy::default();
    assert!(
        p.ui4me_res_scheme,
        "OD-19 approved: ui4me-res: is a scheme, not an unknown one"
    );
    assert_eq!(Scheme::of(&u("ui4me-res://app/a.png")), Scheme::UiRes);
    assert_eq!(
        policy::check(&u("ui4me-res://app/a.png"), &p),
        Decision::Permit(Scheme::UiRes)
    );
    // every other scheme stays denied
    for s in [
        "http://app/a.png",
        "https://app/a.png",
        "ftp://app/a",
        "blob:x",
        "javascript:1",
    ] {
        assert_eq!(
            policy::check(&u(s), &p),
            Decision::Deny(Code::ResourceDenied),
            "{s}"
        );
    }
    // an unknown bundle or path is FILE_NOT_FOUND (not RESOURCE_DENIED)
    let mut e = env();
    let r = e.img(1, "../nope/missing.fake");
    assert_eq!(
        e.size(&r),
        ImageState::Broken {
            code: Code::FileNotFound
        }
    );
}

/// US6 acceptance 1: `<img src="img/logo.png">` in `ui4me-res://app/index.html` loads.
#[test]
fn acceptance_1_relative_reference_inside_a_bundle() {
    let mut e = env();
    let r = e.img(1, "img/logo.fake");
    assert_eq!(e.size(&r), ready(4.0, 3.0));
    let recs = e.mgr.document_records(e.doc);
    assert!(
        recs.iter()
            .any(|r| r.url == "ui4me-res://app/img/logo.fake"),
        "{recs:?}"
    );
    // a sheet-relative reference, `../` from the sheet's own URL (uri-resolution.md §5)
    let css = u("ui4me-res://app/css/site.css");
    let r2 = e
        .mgr
        .request_css_image(
            e.doc,
            node_id(2),
            "background-image",
            0,
            &css,
            "../img/blue.fake",
        )
        .unwrap();
    e.mgr.process_completions();
    assert_eq!(e.size(&r2), ready(8.0, 6.0));
    // percent-encoded path: the lookup key is the decoded path
    let r3 = e.img(3, "img/my%20logo.fake");
    assert_eq!(e.size(&r3), ready(16.0, 12.0));
}

#[test]
fn text_loads_sheets_and_imports_with_a_bundle_identity() {
    let mut e = env();
    let t = e
        .mgr
        .load_text_sync(
            e.doc,
            &e.base.clone(),
            "css/site.css",
            ResourceKind::Stylesheet,
        )
        .unwrap();
    assert_eq!(t.final_url.as_str(), "ui4me-res://app/css/site.css");
    assert!(std::str::from_utf8(&t.bytes).unwrap().contains("@import"));
    assert!(matches!(t.content_identity, TextIdentity::Bundle(_)));
    let t2 = e
        .mgr
        .load_text_sync(e.doc, &t.final_url, "base.css", ResourceKind::Stylesheet)
        .unwrap();
    assert_eq!(&*t2.bytes, b"body{margin:0}");
    assert_eq!(
        e.mgr
            .load_text_sync(
                e.doc,
                &e.base.clone(),
                "css/none.css",
                ResourceKind::Stylesheet
            )
            .unwrap_err()
            .code,
        Code::FileNotFound
    );
    assert_eq!(
        e.mgr
            .load_text_sync(
                e.doc,
                &e.base.clone(),
                "//ghost/a.css",
                ResourceKind::Stylesheet
            )
            .unwrap_err()
            .code,
        Code::FileNotFound
    );
    // the document itself
    let d = e
        .mgr
        .load_bundle_document(&u("ui4me-res://app/index.html"))
        .unwrap();
    assert!(d.bytes.starts_with(b"<!doctype html>"));
}

#[test]
fn no_escape_from_the_bundle() {
    let mut e = env();
    // dot segments are clamped at the bundle root by the URL parser: still inside `app`
    let r = e.img(1, "../../../img/logo.fake");
    assert_eq!(e.size(&r), ready(4.0, 3.0));
    // `%2e%2e` is a dot segment for the URL parser: it resolves to the bundle's own index.html
    // (a page, not an image), never to anything outside
    let r = e.img(2, "img/%2e%2e/%2e%2e/index.html");
    assert_eq!(
        e.size(&r),
        ImageState::Broken {
            code: Code::UnsupportedMediaType
        }
    );
    // an encoded separator or dot segment inside one segment reaches the registry and is refused
    let r = e.img(3, "a%2f..%2fb.fake");
    assert_eq!(
        e.size(&r),
        ImageState::Broken {
            code: Code::ResourceDenied
        }
    );
    // a bundle document has no implicit file root: file: references are denied
    let r = e.img(4, "file:///etc/hostname");
    assert_eq!(
        e.size(&r),
        ImageState::Broken {
            code: Code::ResourceDenied
        }
    );
    assert!(e.mgr.file_roots(e.doc).unwrap().is_empty());
    // a network URL stays denied
    let r = e.img(5, "http://example.com/x.png");
    assert_eq!(
        e.size(&r),
        ImageState::Broken {
            code: Code::ResourceDenied
        }
    );
    // a bundle name is not a directory: the bare bundle and a directory path are not entries
    let r = e.img(6, "img/");
    assert_eq!(
        e.size(&r),
        ImageState::Broken {
            code: Code::FileNotFound
        }
    );
    // another registered bundle is reachable by an explicit authority, an unregistered one is not
    e.mgr
        .bundle_register("app2", bundle_inputs("app2"))
        .unwrap();
    let r = e.img(7, "//app2/img/logo.fake");
    assert_eq!(e.size(&r), ready(16.0, 12.0));
    let r = e.img(8, "//app3/img/logo.fake");
    assert_eq!(
        e.size(&r),
        ImageState::Broken {
            code: Code::FileNotFound
        }
    );
}

#[test]
fn svg_by_registered_mime_or_extension() {
    let mut e = env();
    let r = e.img(1, "img/mark.svg");
    assert!(
        matches!(e.size(&r), ImageState::Ready { width_css: Some(w), height_css: Some(h), .. } if w == 20.0 && h == 10.0),
        "{:?}",
        e.size(&r)
    );
    let r = e.img(2, "img/mark-typed");
    assert!(
        matches!(e.size(&r), ImageState::Ready { width_css: Some(w), .. } if w == 20.0),
        "{:?}",
        e.size(&r)
    );
}

#[test]
fn entry_over_the_encoded_limit_is_size_limit() {
    let mut e = env();
    let big = vec![0u8; 4096];
    e.mgr
        .bundle_register(
            "big",
            vec![
                BundleEntryInput::new("x.bin", big.clone(), None),
                BundleEntryInput::new("x.css", big, None),
            ],
        )
        .unwrap();
    let d2 = doc_id(2);
    let patch = ResourcePolicyPatch {
        max_encoded_image: Some(2048),
        max_encoded_stylesheet: Some(2048),
        ..Default::default()
    };
    e.mgr
        .create_document(d2, u("ui4me-res://app/index.html"), None, &patch)
        .unwrap();
    let r = e
        .mgr
        .request_image(
            d2,
            Requester::ImgElement(node_id(1)),
            &u("ui4me-res://big/index.html"),
            "x.bin",
            1.0,
        )
        .unwrap();
    e.mgr.process_completions();
    assert_eq!(
        e.mgr.natural_size(&r),
        ImageState::Broken {
            code: Code::SizeLimit
        }
    );
    let err = e
        .mgr
        .load_text_sync(
            d2,
            &u("ui4me-res://big/index.html"),
            "x.css",
            ResourceKind::Stylesheet,
        )
        .unwrap_err();
    assert_eq!(err.code, Code::SizeLimit);
}

/// FR-055: replacement bumps the resource epoch and invalidates cached decodes; documents that
/// never referenced the bundle are not affected.
#[test]
fn replacement_restarts_images_bumps_the_epoch_and_drops_cached_decodes() {
    let mut e = env();
    let other = doc_id(2);
    e.mgr
        .create_document(
            other,
            u("ui4me-res://elsewhere/x.html"),
            None,
            &Default::default(),
        )
        .unwrap();
    let r = e.img(1, "img/logo.fake");
    assert_eq!(e.size(&r), ready(4.0, 3.0));
    let epoch0 = e.mgr.resource_epoch(e.doc).unwrap();
    let decodes0 = e.counter.decodes.load(Ordering::SeqCst);

    // Byte-identical re-registration is a new registry generation but changes nothing a
    // document can observe: no document is notified and the cached decode is kept.
    let change = e.mgr.bundle_register("app", bundle_inputs("app")).unwrap();
    assert!(change.existed);
    assert!(change.impact.is_empty());
    assert!(change.affected.is_empty(), "identical bytes affect nobody");
    assert!(e.mgr.process_completions().is_empty());
    assert_eq!(e.mgr.resource_epoch(e.doc).unwrap(), epoch0);
    assert_eq!(e.counter.decodes.load(Ordering::SeqCst), decodes0);
    assert_eq!(e.size(&r), ready(4.0, 3.0));

    // A changed entry restarts the image: the old image stays shown until the new one is
    // Ready, the epoch advances and the decode is redone.
    let mut entries = bundle_inputs("app");
    for entry in &mut entries {
        if entry.path == "img/logo.fake" {
            entry.data = FakeImage {
                rgba: [0, 0, 255, 255],
                ..FakeImage::new(4, 3)
            }
            .bytes()
            .into();
        }
    }
    let change = e.mgr.bundle_register("app", entries).unwrap();
    assert_eq!(
        change.affected,
        vec![e.doc],
        "only the document that referenced the bundle"
    );
    assert_eq!(e.size(&r), ready(4.0, 3.0));
    let batches = e.mgr.process_completions();
    assert_eq!(batches.len(), 1);
    assert!(
        e.mgr.resource_epoch(e.doc).unwrap() > epoch0,
        "the epoch advanced"
    );
    assert_eq!(
        e.counter.decodes.load(Ordering::SeqCst),
        decodes0 + 1,
        "the replaced bundle's decode was dropped and redone"
    );

    // different content behind the same URL
    let mut entries = bundle_inputs("app");
    entries.retain(|e| e.path != "img/logo.fake");
    entries.push(BundleEntryInput::new(
        "img/logo.fake",
        FakeImage::new(10, 5).bytes(),
        None,
    ));
    e.mgr.bundle_register("app", entries).unwrap();
    // the request is the same element, a new revision
    let r2 = e
        .mgr
        .request_image(
            e.doc,
            Requester::ImgElement(node_id(1)),
            &e.base.clone(),
            "img/logo.fake",
            1.0,
        )
        .unwrap();
    assert!(r2.revision > r.revision);
    e.mgr.process_completions();
    assert_eq!(e.size(&r2), ready(10.0, 5.0));
    assert_eq!(
        e.mgr.resource_epoch(other).unwrap(),
        ui4me_engine_types::resource_ids::ResourceEpoch(0)
    );
}

#[test]
fn unregister_makes_references_not_found_and_is_idempotent() {
    let mut e = env();
    let r = e.img(1, "img/logo.fake");
    assert_eq!(e.size(&r), ready(4.0, 3.0));
    let change = e.mgr.bundle_unregister("app");
    assert!(change.existed);
    assert_eq!(change.affected, vec![e.doc]);
    e.mgr.process_completions();
    let r2 = e
        .mgr
        .request_image(
            e.doc,
            Requester::ImgElement(node_id(1)),
            &e.base.clone(),
            "img/logo.fake",
            1.0,
        )
        .unwrap();
    e.mgr.process_completions();
    assert_eq!(
        e.size(&r2),
        ImageState::Broken {
            code: Code::FileNotFound
        }
    );
    assert!(!e.mgr.bundle_unregister("app").existed, "idempotent");
    // registering it again reaches the document that failed to find it
    let change = e.mgr.bundle_register("app", bundle_inputs("app")).unwrap();
    assert_eq!(change.affected, vec![e.doc]);
    e.mgr.process_completions();
    let r3 = e
        .mgr
        .request_image(
            e.doc,
            Requester::ImgElement(node_id(1)),
            &e.base.clone(),
            "img/logo.fake",
            1.0,
        )
        .unwrap();
    e.mgr.process_completions();
    assert_eq!(e.size(&r3), ready(4.0, 3.0));
}

#[test]
fn a_failed_registration_changes_nothing() {
    let mut e = env();
    let before = e.mgr.bundles().get("app").unwrap().generation;
    assert_eq!(
        e.mgr
            .bundle_register("app", vec![BundleEntryInput::new("../x", Vec::new(), None)])
            .unwrap_err()
            .code,
        Code::InvalidArgument
    );
    assert_eq!(
        e.mgr.bundle_register("Bad Name", vec![]).unwrap_err().code,
        Code::InvalidArgument
    );
    assert_eq!(e.mgr.bundles().get("app").unwrap().generation, before);
    assert!(bundles::valid_name("app"));
}

#[test]
fn directory_bundle_is_a_snapshot_and_refuses_links() {
    let t = TempDir::new("bundledir");
    t.write("index.html", b"<p>hi");
    t.write("img/a.fake", &FakeImage::new(2, 2).bytes());
    t.write(
        "img/v.svg",
        b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'/>",
    );
    let mut mgr = manager(true);
    mgr.bundle_register_dir("pkg", t.path()).unwrap();
    // the snapshot does not follow later changes on disk
    t.write("index.html", b"changed");
    let doc = doc_id(1);
    mgr.create_document(
        doc,
        u("ui4me-res://pkg/index.html"),
        None,
        &Default::default(),
    )
    .unwrap();
    let text = mgr
        .load_text_sync(
            doc,
            &u("ui4me-res://pkg/index.html"),
            "index.html",
            ResourceKind::Stylesheet,
        )
        .unwrap();
    assert_eq!(&*text.bytes, b"<p>hi");
    let r = mgr
        .request_image(
            doc,
            Requester::ImgElement(node_id(1)),
            &u("ui4me-res://pkg/index.html"),
            "img/v.svg",
            1.0,
        )
        .unwrap();
    mgr.process_completions();
    assert!(matches!(mgr.natural_size(&r), ImageState::Ready { .. }));
    assert_eq!(
        mgr.bundle_register_dir("pkg2", &t.join("missing"))
            .unwrap_err()
            .code,
        Code::FileNotFound
    );
    assert_eq!(
        mgr.bundle_register_dir("pkg2", &t.join("index.html"))
            .unwrap_err()
            .code,
        Code::InvalidArgument
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/hostname", t.join("link")).unwrap();
        assert_eq!(
            mgr.bundle_register_dir("pkg3", t.path()).unwrap_err().code,
            Code::ResourceDenied
        );
        assert!(mgr.bundles().get("pkg3").is_none());
    }
}

#[test]
fn ui4me_res_urls_resolve_per_the_uri_contract() {
    let base = u("ui4me-res://app/index.html");
    assert_eq!(
        uri::resolve(&base, "../img/a.png").unwrap().as_str(),
        "ui4me-res://app/img/a.png"
    );
    assert_eq!(
        uri::resolve(&u("ui4me-res://app/css/site.css"), "../img/bg.jpg?v=2#x")
            .unwrap()
            .as_str(),
        "ui4me-res://app/img/bg.jpg?v=2#x"
    );
    assert_eq!(bundles::bundle_of(&base), Some("app"));
}
