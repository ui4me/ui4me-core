// owner: 007-resources — font sources (U4C-RES; 007 `contracts/resource-view.md` §5).
//
// The first six tests are 001 T031's pinned-fonts-only subset (R-05; 001 FR-025), kept as the
// compatibility contract of the slice. The rest are 007 T055: `@font-face` faces, format
// skipping, source order, ranges, per-document scoping, the embedded last-resort font (OD-24),
// runtime-creation validation (FR-054) and host registration path rules (Clarification Q2).
#![cfg(unix)]

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use support::*;
use ui4me_assets::ResourceManager;
use ui4me_assets::decode::fake::FakeDecoder;
use ui4me_assets::fonts::generic::{self, Platform};
use ui4me_assets::fonts::{
    FaceSourceSpec, FaceState, FaceStyle, FontFaceRule, FontMode, FontRegistry, LastResortStatus,
    PinnedFontConfig, last_resort,
};
use ui4me_assets::policy::{
    FontConfiguration, ResourcePolicy, ResourcePolicyPatch, RuntimeResourceModes,
};
use ui4me_assets::uri;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::text_metrics::GenericFamily;
use url::Url;

fn fixture_font() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fonts/noto-sans/NotoSans-Regular.ttf")
}

// ---- 001 T031 slice subset ------------------------------------------------------------------

#[test]
fn registers_the_pinned_font_file() {
    let reg = FontRegistry::pinned(&PinnedFontConfig::with_files([fixture_font()]))
        .expect("pinned font registers");
    assert_eq!(reg.mode(), FontMode::PinnedFontsOnly);
    let view = reg.font_sources();
    assert_eq!(view.registered.len(), 1);
    let blob = &view.registered[0];
    assert_eq!(blob.name, "NotoSans-Regular.ttf");
    // FONT.json records 512,672 bytes for the pinned file.
    assert_eq!(blob.bytes.len(), 512_672);
    assert!(view.font_epoch.0 > 0, "registration bumps the font epoch");
}

#[test]
fn missing_font_file_fails_with_file_not_found_naming_the_file() {
    let missing = Path::new("/nonexistent/dir/Missing-Font.ttf");
    let err =
        FontRegistry::pinned(&PinnedFontConfig::with_files([missing])).expect_err("missing file");
    assert_eq!(err.code, Code::FileNotFound);
    assert!(
        err.message.contains("Missing-Font.ttf"),
        "message names the file: {}",
        err.message
    );
}

#[test]
fn system_fonts_are_never_enumerated() {
    let reg = FontRegistry::pinned(&PinnedFontConfig::with_files([fixture_font()])).unwrap();
    let view = reg.font_sources();
    assert!(!view.system_fonts);
    assert!(view.pinned_fonts_only);
    // Only the configured file is a registered source; nothing else appears.
    assert!(
        view.registered
            .iter()
            .all(|b| b.name == "NotoSans-Regular.ttf")
    );
}

#[test]
fn no_files_and_no_last_resort_is_engine_misconfigured() {
    let err = FontRegistry::pinned(&PinnedFontConfig::default().without_last_resort())
        .expect_err("no usable face");
    assert_eq!(err.code, Code::EngineMisconfigured);
}

#[test]
fn non_font_bytes_are_rejected_and_not_registered() {
    let mut reg = FontRegistry::new(FontMode::PinnedFontsOnly);
    let before = reg.epoch();
    let err = reg
        .register_bytes("notes.txt", Arc::from(&b"hello world"[..]))
        .expect_err("not a font");
    assert_eq!(
        err.code,
        Code::DecodeFailed,
        "invalid font data is DECODE_FAILED (FR-052)"
    );
    assert!(err.message.contains("notes.txt"));
    assert_eq!(reg.epoch(), before);
    assert!(reg.font_sources().registered.is_empty());
}

#[test]
fn every_change_bumps_the_epoch_and_ids_are_unique() {
    let mut reg = FontRegistry::new(FontMode::PinnedFontsOnly);
    let bytes: Arc<[u8]> = Arc::from(std::fs::read(fixture_font()).unwrap());
    let a = reg.register_bytes("a", bytes.clone()).unwrap();
    let e1 = reg.epoch();
    let b = reg.register_bytes("b", bytes).unwrap();
    let e2 = reg.epoch();
    reg.set_generic(GenericFamily::SansSerif, vec!["Noto Sans".into()]);
    let e3 = reg.epoch();
    assert_ne!(a, b);
    assert!(e1 < e2 && e2 < e3);
    let view = reg.font_sources();
    assert_eq!(
        view.generic_map,
        vec![(GenericFamily::SansSerif, vec!["Noto Sans".to_string()])]
    );
    // Views share bytes; they do not copy them.
    assert!(Arc::ptr_eq(
        &view.registered[0].bytes,
        &reg.blobs()[0].bytes
    ));
}

// ---- 007 T055: last-resort font (OD-24), runtime-creation validation (FR-054) ---------------

#[test]
fn last_resort_font_is_present_in_every_mode_unless_the_flag_is_off() {
    let (sys, pin) = (
        FontRegistry::new(FontMode::System),
        FontRegistry::pinned(&PinnedFontConfig::with_files([fixture_font()])).unwrap(),
    );
    for reg in [&sys, &pin] {
        let v = reg.font_sources();
        let lr = v.last_resort.as_ref().expect("embedded last-resort font");
        assert_eq!(last_resort::sha256_hex(&lr.bytes), last_resort::SHA256_HEX);
        assert!(
            v.registered.iter().all(|b| b.id != lr.id),
            "the last-resort font is not a registered font and has no font_id to unregister"
        );
        let info = reg.last_resort_info();
        assert_eq!(
            (info.status, info.enabled, info.family),
            (LastResortStatus::Used, true, "Noto Sans")
        );
        assert_eq!(
            info.sha256,
            "89c3c497f618fdaa0b2d1e98fef93582f28c71debd2c4a8cdf41f190ced2909d"
        );
    }
    let off =
        FontRegistry::pinned(&PinnedFontConfig::with_files([fixture_font()]).without_last_resort())
            .unwrap();
    assert!(off.font_sources().last_resort.is_none());
    assert_eq!(off.last_resort_info().status, LastResortStatus::Disabled);
    // the embedded bytes are the same on every platform: they are compiled in
    assert_eq!(last_resort::BYTES.len(), 512_672);
}

fn modes(fonts: FontConfiguration) -> RuntimeResourceModes {
    RuntimeResourceModes {
        deterministic_resources: true,
        fonts,
    }
}

fn create(
    fonts: FontConfiguration,
    defaults: ResourcePolicy,
    system_available: Option<bool>,
) -> Result<ResourceManager, ui4me_engine_types::status::Status> {
    ResourceManager::new_with_system_fonts(
        modes(fonts),
        defaults,
        Arc::new(FakeDecoder),
        system_available,
    )
}

#[test]
fn unusable_font_set_fails_once_at_runtime_creation_with_engine_misconfigured() {
    let no_system = ResourcePolicy::default()
        .merged(&ResourcePolicyPatch {
            system_fonts: Some(false),
            ..Default::default()
        })
        .unwrap();
    // system fonts off, nothing registered, flag off: ENGINE_MISCONFIGURED
    let e = create(
        FontConfiguration {
            last_resort_fallback: false,
            ..Default::default()
        },
        no_system.clone(),
        None,
    )
    .err()
    .expect("misconfigured");
    assert_eq!(e.code, Code::EngineMisconfigured);
    // the same with the flag on is fine: the embedded font is a usable face
    assert!(create(FontConfiguration::default(), no_system.clone(), None).is_ok());
    // system fonts enabled but the platform has none, flag off
    let e = create(
        FontConfiguration {
            last_resort_fallback: false,
            ..Default::default()
        },
        ResourcePolicy::default(),
        Some(false),
    )
    .err()
    .expect("misconfigured");
    assert_eq!(e.code, Code::EngineMisconfigured);
    assert!(
        create(
            FontConfiguration {
                last_resort_fallback: false,
                ..Default::default()
            },
            ResourcePolicy::default(),
            Some(true)
        )
        .is_ok()
    );
    // pinned files + flag off is fine; a missing pinned file is FILE_NOT_FOUND
    let pinned = FontConfiguration {
        pinned: Some(PinnedFontConfig::with_files([fixture_font()])),
        last_resort_fallback: false,
        ..Default::default()
    };
    assert!(create(pinned, ResourcePolicy::default(), None).is_ok());
    let bad = FontConfiguration {
        pinned: Some(PinnedFontConfig::with_files(["/no/such/font.ttf"])),
        ..Default::default()
    };
    assert_eq!(
        create(bad, ResourcePolicy::default(), None)
            .err()
            .unwrap()
            .code,
        Code::FileNotFound
    );
}

#[test]
fn document_creation_never_fails_for_fonts() {
    let no_system = ResourcePolicy::default()
        .merged(&ResourcePolicyPatch {
            system_fonts: Some(false),
            ..Default::default()
        })
        .unwrap();
    let mut mgr = create(FontConfiguration::default(), no_system, None).unwrap();
    // even a document-level request to disable system fonts is only a policy value, never an error
    mgr.create_document(
        doc_id(1),
        uri::about_blank(),
        None,
        &ResourcePolicyPatch {
            system_fonts: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn unregistering_the_last_usable_face_is_invalid_state_and_changes_nothing() {
    let mut reg =
        FontRegistry::pinned(&PinnedFontConfig::with_files([fixture_font()]).without_last_resort())
            .unwrap();
    let id = reg.blobs()[0].id;
    let epoch = reg.epoch();
    let e = reg.unregister(id).unwrap_err();
    assert_eq!(e.code, Code::InvalidState);
    assert_eq!((reg.epoch(), reg.blobs().len()), (epoch, 1));
    // with the last-resort font enabled the same call succeeds (and is idempotent)
    let mut reg = FontRegistry::pinned(&PinnedFontConfig::with_files([fixture_font()])).unwrap();
    let id = reg.blobs()[0].id;
    reg.unregister(id).unwrap();
    assert!(reg.blobs().is_empty());
    let after = reg.epoch();
    reg.unregister(id).unwrap();
    assert_eq!(
        reg.epoch(),
        after,
        "idempotent: no epoch change for an unknown id"
    );
    assert!(
        reg.font_sources().last_resort.is_some(),
        "the last-resort font cannot be unregistered"
    );
}

#[test]
fn host_registration_path_rules() {
    let t = TempDir::new("fontreg");
    let mut reg = FontRegistry::new(FontMode::System);
    // trusted host path outside any document root works; relative and dotted paths are normalised
    t.write("a/b/font.ttf", &std::fs::read(fixture_font()).unwrap());
    let dotted = t.join("a/b/../b/./font.ttf");
    let id = reg
        .register_file_with_family(&dotted, Some("Override Family".into()))
        .unwrap();
    assert_eq!(
        reg.blob(id).unwrap().family_override.as_deref(),
        Some("Override Family")
    );
    assert_eq!(reg.blob(id).unwrap().name, "font.ttf");
    // missing / directory / oversize / invalid / unsupported
    assert_eq!(
        reg.register_file(&t.join("nope.ttf")).unwrap_err().code,
        Code::FileNotFound
    );
    assert_eq!(
        reg.register_file(&t.join("a")).unwrap_err().code,
        Code::ResourceDenied
    );
    let f = std::fs::File::create(t.join("big.ttf")).unwrap();
    f.set_len(33 * 1024 * 1024).unwrap();
    assert_eq!(
        reg.register_file(&t.join("big.ttf")).unwrap_err().code,
        Code::SizeLimit
    );
    t.write("bad.ttf", b"not a font");
    assert_eq!(
        reg.register_file(&t.join("bad.ttf")).unwrap_err().code,
        Code::DecodeFailed
    );
    t.write("x.woff2", &synthetic_font("woff2-stub.woff2"));
    assert_eq!(
        reg.register_file(&t.join("x.woff2")).unwrap_err().code,
        Code::UnsupportedMediaType
    );
    // a TTC collection is accepted
    t.write("c.ttc", &synthetic_font("synthetic-collection.ttc"));
    assert!(reg.register_file(&t.join("c.ttc")).is_ok());
    assert_eq!(reg.blobs().len(), 2, "failures register nothing");
    // bytes are accepted directly
    assert!(
        reg.register_bytes("mem", Arc::from(std::fs::read(fixture_font()).unwrap()))
            .is_ok()
    );
}

#[test]
fn font_config_get_reports_everything() {
    let mut mgr = create(
        FontConfiguration {
            pinned: Some(PinnedFontConfig::with_files([fixture_font()])),
            ..Default::default()
        },
        ResourcePolicy::default(),
        None,
    )
    .unwrap();
    let r = mgr.font_config_get();
    assert!(r.pinned_fonts_only && !r.system_fonts && r.deterministic_resources);
    assert_eq!(r.registered.len(), 1);
    assert_eq!(r.last_resort.status, LastResortStatus::Used);
    assert_eq!(r.last_resort.status.name(), "used");
    let e0 = r.font_epoch;
    mgr.font_register_bytes(
        "again",
        Arc::from(std::fs::read(fixture_font()).unwrap()),
        None,
    )
    .unwrap();
    assert!(mgr.font_config_get().font_epoch > e0);
}

// ---- generic families ------------------------------------------------------------------------

#[test]
fn system_mode_view_carries_the_platform_generic_map_and_overrides() {
    let cfg = FontConfiguration {
        generic_map: vec![(GenericFamily::Serif, vec!["My Serif".into()])],
        ..Default::default()
    };
    let mgr = create(cfg, ResourcePolicy::default(), None).unwrap();
    let v = mgr.runtime_font_sources();
    assert_eq!(v.generic_map.len(), 12);
    assert_eq!(
        v.generic_map
            .iter()
            .find(|(g, _)| *g == GenericFamily::Serif)
            .unwrap()
            .1,
        vec!["My Serif".to_string()]
    );
    assert_eq!(
        v.generic_map
            .iter()
            .find(|(g, _)| *g == GenericFamily::Monospace)
            .unwrap()
            .1,
        generic::default_generic_map(Platform::current())
            .iter()
            .find(|(g, _)| *g == GenericFamily::Monospace)
            .unwrap()
            .1
    );
    assert_eq!(v.fallback, generic::default_fallback(Platform::current()));
    assert!(v.system_fonts && !v.pinned_fonts_only);
}

// ---- @font-face (T063, T064) -----------------------------------------------------------------

struct Env {
    t: TempDir,
    mgr: ResourceManager,
    doc: ui4me_engine_types::ids::DocumentId,
    base: Url,
}

fn env(deterministic: bool) -> Env {
    let t = TempDir::new("faces");
    t.write("index.html", b"x");
    t.write("noto.ttf", &std::fs::read(fixture_font()).unwrap());
    for f in [
        "woff2-stub.woff2",
        "woff-stub.woff",
        "invalid.ttf",
        "synthetic-collection.ttc",
        "truncated.ttf",
    ] {
        t.write(f, &synthetic_font(f));
    }
    let mut mgr = manager(deterministic);
    let doc = doc_id(1);
    let f = t.join("index.html");
    let base = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, base.clone(), Some(&f), &Default::default())
        .unwrap();
    Env { t, mgr, doc, base }
}

fn rule(id: u64, family: &str, sources: Vec<FaceSourceSpec>, base: &Url) -> FontFaceRule {
    let _ = base;
    FontFaceRule {
        rule_id: id,
        settings: Default::default(),
        family: family.into(),
        weight: (400.0, 400.0),
        style: FaceStyle::Normal,
        width: (100.0, 100.0),
        unicode_range: vec![],
        sources,
        display: None,
    }
}

fn url_src(base: &Url, r: &str, format: Option<&str>) -> FaceSourceSpec {
    FaceSourceSpec::Url {
        reference: r.into(),
        base: base.clone(),
        format: format.map(str::to_string),
    }
}

fn face_state(e: &Env, id: ui4me_assets::fonts::FaceId) -> FaceState {
    e.mgr
        .font_sources(e.doc)
        .unwrap()
        .faces
        .into_iter()
        .find(|f| f.id == id)
        .unwrap()
        .state
}

#[test]
fn faces_load_lazily_and_bump_the_font_epoch_once_per_batch() {
    for det in [false, true] {
        let mut e = env(det);
        let rules = vec![
            rule(
                1,
                "Alpha",
                vec![url_src(&e.base, "noto.ttf", Some("truetype"))],
                &e.base,
            ),
            rule(
                2,
                "Beta",
                vec![url_src(&e.base, "synthetic-collection.ttc", None)],
                &e.base,
            ),
        ];
        let ids = e.mgr.sync_font_faces(e.doc, &rules).unwrap();
        let epoch0 = e.mgr.font_epoch();
        // nothing is read until 009 asks
        assert!(matches!(face_state(&e, ids[0]), FaceState::Unloaded));
        e.mgr.process_completions();
        assert_eq!(e.mgr.font_epoch(), epoch0, "no load, no epoch change");
        e.mgr.request_face(e.doc, ids[0]).unwrap();
        e.mgr.request_face(e.doc, ids[0]).unwrap(); // idempotent
        e.mgr.request_face(e.doc, ids[1]).unwrap();
        assert!(matches!(
            face_state(&e, ids[0]),
            FaceState::Loading { source_index: 0 }
        ));
        assert_eq!(e.mgr.pending(e.doc).unwrap(), 2);
        if !det {
            wait_queued(&e.mgr, 2);
        }
        let batches = e.mgr.process_completions();
        assert_eq!(batches.len(), 1);
        assert!(
            batches[0].font_epoch_changed && batches[0].changed.is_empty(),
            "font loads are not image changes"
        );
        assert_eq!(
            e.mgr.font_epoch().0,
            epoch0.0 + 1,
            "two faces, one drain step, one font-epoch increment (det={det})"
        );
        match face_state(&e, ids[0]) {
            FaceState::Loaded(b) => {
                assert_eq!(b.bytes.len(), 512_672);
                assert_eq!(
                    b.family_override.as_deref(),
                    Some("Alpha"),
                    "descriptor override handed to 009"
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(face_state(&e, ids[1]), FaceState::Loaded(_)));
        assert!(e.mgr.process_completions().is_empty());
    }
}

#[test]
fn src_entries_are_tried_in_order_and_unsupported_formats_are_skipped_unread() {
    for det in [false, true] {
        let mut e = env(det);
        // the woff2 stub exists but must never be read: an unsupported (`embedded-opentype`) hint skips it. Delete it to prove it.
        std::fs::remove_file(e.t.join("woff2-stub.woff2")).unwrap();
        let r = rule(
            1,
            "Ordered",
            vec![
                url_src(&e.base, "woff2-stub.woff2", Some("embedded-opentype")),
                url_src(&e.base, "invalid.ttf", None),
                FaceSourceSpec::Local("No Such Local".into()),
                url_src(&e.base, "noto.ttf", None),
            ],
            &e.base,
        );
        let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
        e.mgr.request_face(e.doc, ids[0]).unwrap();
        let epoch0 = e.mgr.font_epoch();
        // invalid.ttf is the first real source; it fails, the next (local) fails, noto loads
        settle(&mut e.mgr, e.doc);
        e.mgr.process_completions();
        assert!(
            matches!(face_state(&e, ids[0]), FaceState::Loaded(_)),
            "det={det}"
        );
        assert_eq!(
            e.mgr.font_epoch().0,
            epoch0.0 + 1,
            "intermediate source failures do not bump the epoch"
        );
    }
}

#[test]
fn all_sources_failing_records_each_code_and_bumps_the_epoch_once() {
    for det in [false, true] {
        let mut e = env(det);
        let r = rule(
            1,
            "Doomed",
            vec![
                url_src(&e.base, "invalid.ttf", None),
                url_src(&e.base, "woff2-stub.woff2", None),
                url_src(&e.base, "missing.ttf", None),
                url_src(&e.base, "../outside.ttf", None),
                url_src(&e.base, "http://example.com/f.ttf", None),
            ],
            &e.base,
        );
        let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
        e.mgr.request_face(e.doc, ids[0]).unwrap();
        let epoch0 = e.mgr.font_epoch();
        settle(&mut e.mgr, e.doc);
        e.mgr.process_completions();
        match face_state(&e, ids[0]) {
            FaceState::Failed(codes) => {
                assert_eq!(
                    codes,
                    vec![
                        Code::DecodeFailed,
                        Code::DecodeFailed,
                        Code::FileNotFound,
                        Code::ResourceDenied,
                        Code::ResourceDenied
                    ],
                    "det={det}"
                )
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(e.mgr.font_epoch().0, epoch0.0 + 1);
    }
}

#[test]
fn synchronous_failure_is_reported_at_the_next_drain_point() {
    let mut e = env(true);
    let r = rule(
        1,
        "OnlyHints",
        vec![
            url_src(&e.base, "a.eot", Some("embedded-opentype")),
            url_src(&e.base, "b.svg", Some("svg")),
        ],
        &e.base,
    );
    let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
    e.mgr.request_face(e.doc, ids[0]).unwrap();
    assert!(
        matches!(face_state(&e, ids[0]), FaceState::Failed(ref c) if *c == vec![Code::UnsupportedMediaType, Code::UnsupportedMediaType])
    );
    let epoch0 = e.mgr.font_epoch();
    let b = e.mgr.process_completions();
    assert_eq!(b.len(), 1);
    assert!(b[0].font_epoch_changed);
    assert_eq!(e.mgr.font_epoch().0, epoch0.0 + 1);
}

#[test]
fn ranges_and_unicode_range_are_stored_unchanged() {
    let mut e = env(true);
    let mut ranged = rule(
        1,
        "Ranged",
        vec![url_src(&e.base, "noto.ttf", None)],
        &e.base,
    );
    ranged.weight = (100.0, 900.0);
    ranged.width = (75.0, 125.0);
    ranged.style = FaceStyle::Oblique {
        min_deg: -20.0,
        max_deg: 20.0,
    };
    ranged.unicode_range = vec![(0, 255), (0x2000, 0x206F)];
    let single = rule(
        2,
        "Single",
        vec![url_src(&e.base, "noto.ttf", None)],
        &e.base,
    );
    e.mgr.sync_font_faces(e.doc, &[ranged, single]).unwrap();
    let faces = e.mgr.font_sources(e.doc).unwrap().faces;
    assert_eq!(faces[0].weight, (100.0, 900.0));
    assert_eq!(faces[0].width, (75.0, 125.0));
    assert_eq!(
        faces[0].style,
        FaceStyle::Oblique {
            min_deg: -20.0,
            max_deg: 20.0
        }
    );
    assert_eq!(faces[0].unicode_range, vec![(0, 255), (0x2000, 0x206F)]);
    // a single value is a degenerate range; an absent unicode-range is U+0-10FFFF
    assert_eq!(faces[1].weight, (400.0, 400.0));
    assert_eq!(faces[1].unicode_range, vec![(0, 0x10FFFF)]);
}

#[test]
fn font_display_is_reported_once_per_face_and_behaves_as_swap() {
    let mut e = env(true);
    for value in ["auto", "block", "swap", "fallback", "optional"] {
        let mut r = rule(
            1,
            "Display",
            vec![url_src(&e.base, "noto.ttf", None)],
            &e.base,
        );
        r.display = Some(value.into());
        let ids = e
            .mgr
            .sync_font_faces(e.doc, std::slice::from_ref(&r))
            .unwrap();
        let again = e
            .mgr
            .sync_font_faces(e.doc, std::slice::from_ref(&r))
            .unwrap();
        assert_eq!(ids, again, "an unchanged rule keeps its face");
        let faces = e.mgr.font_sources(e.doc).unwrap().faces;
        assert_eq!(faces.len(), 1);
        assert_eq!(
            faces[0].reports,
            vec!["font.display"],
            "reported once ({value})"
        );
        assert!(
            matches!(faces[0].state, FaceState::Unloaded),
            "text shows the fallback at once; nothing blocks"
        );
    }
}

#[test]
fn rule_changes_and_removal_follow_the_cascade() {
    let mut e = env(true);
    let a = rule(1, "A", vec![url_src(&e.base, "noto.ttf", None)], &e.base);
    let ids = e
        .mgr
        .sync_font_faces(e.doc, std::slice::from_ref(&a))
        .unwrap();
    e.mgr.request_face(e.doc, ids[0]).unwrap();
    e.mgr.process_completions();
    assert!(matches!(face_state(&e, ids[0]), FaceState::Loaded(_)));
    // same rule again: the loaded face is kept
    assert_eq!(
        e.mgr
            .sync_font_faces(e.doc, std::slice::from_ref(&a))
            .unwrap(),
        ids
    );
    assert!(matches!(face_state(&e, ids[0]), FaceState::Loaded(_)));
    // a changed rule is a new, unloaded face
    let mut a2 = a.clone();
    a2.weight = (700.0, 700.0);
    let ids2 = e.mgr.sync_font_faces(e.doc, &[a2]).unwrap();
    assert_ne!(ids2, ids);
    assert!(matches!(face_state(&e, ids2[0]), FaceState::Unloaded));
    // removal: the face disappears at the next style update
    e.mgr.sync_font_faces(e.doc, &[]).unwrap();
    assert!(e.mgr.font_sources(e.doc).unwrap().faces.is_empty());
    assert_eq!(
        e.mgr.request_face(e.doc, ids2[0]).unwrap_err().code,
        Code::StaleHandle
    );
}

#[test]
fn faces_are_scoped_per_document_even_with_the_same_family_name() {
    let mut e = env(true);
    let doc2 = doc_id(2);
    let f = e.t.join("index.html");
    e.mgr
        .create_document(doc2, e.base.clone(), Some(&f), &Default::default())
        .unwrap();
    let r1 = rule(
        1,
        "Shared Name",
        vec![url_src(&e.base, "noto.ttf", None)],
        &e.base,
    );
    let r2 = rule(
        1,
        "Shared Name",
        vec![url_src(&e.base, "synthetic-collection.ttc", None)],
        &e.base,
    );
    let i1 = e.mgr.sync_font_faces(e.doc, &[r1]).unwrap();
    let i2 = e.mgr.sync_font_faces(doc2, &[r2]).unwrap();
    assert_ne!(i1[0], i2[0]);
    e.mgr.request_face(e.doc, i1[0]).unwrap();
    // only the declaring document sees (and can request) its face
    assert_eq!(
        e.mgr.request_face(doc2, i1[0]).unwrap_err().code,
        Code::StaleHandle
    );
    let b = e.mgr.process_completions();
    assert_eq!(
        b.iter().map(|b| b.document).collect::<Vec<_>>(),
        vec![e.doc],
        "only the document whose face changed is named"
    );
    let v1 = e.mgr.font_sources(e.doc).unwrap();
    let v2 = e.mgr.font_sources(doc2).unwrap();
    assert!(matches!(v1.faces[0].state, FaceState::Loaded(ref b) if b.bytes.len() == 512_672));
    assert!(matches!(v2.faces[0].state, FaceState::Unloaded));
}

#[test]
fn face_sources_obey_the_document_policy_and_the_font_size_limit() {
    let mut e = env(true);
    e.t.write(
        "../outside-font.ttf",
        &std::fs::read(fixture_font()).unwrap(),
    );
    let big = std::fs::File::create(e.t.join("big.ttf")).unwrap();
    big.set_len(33 * 1024 * 1024).unwrap();
    let r = rule(
        1,
        "Policy",
        vec![
            url_src(&e.base, "../outside-font.ttf", None),
            url_src(&e.base, "big.ttf", None),
            url_src(&e.base, "truncated.ttf", None),
        ],
        &e.base,
    );
    let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
    e.mgr.request_face(e.doc, ids[0]).unwrap();
    for _ in 0..4 {
        e.mgr.process_completions();
    }
    match face_state(&e, ids[0]) {
        FaceState::Failed(c) => assert_eq!(
            c,
            vec![Code::ResourceDenied, Code::SizeLimit, Code::DecodeFailed]
        ),
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_file(e.t.join("../outside-font.ttf"));
}

#[test]
fn local_source_matches_a_registered_font_family() {
    let mut e = env(true);
    e.mgr
        .font_register_bytes(
            "reg.ttf",
            Arc::from(std::fs::read(fixture_font()).unwrap()),
            Some("Registered Family".into()),
        )
        .unwrap();
    let r = rule(
        1,
        "UsesLocal",
        vec![FaceSourceSpec::Local("registered family".into())],
        &e.base,
    );
    let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
    e.mgr.request_face(e.doc, ids[0]).unwrap();
    // the declared family aliases the registered one (009 maps it; its blob is already in the collection)
    assert!(
        matches!(face_state(&e, ids[0]), FaceState::LocalFamily(ref f) if f == "Registered Family")
    );
}

#[test]
fn local_source_asks_the_installed_family_lookup_after_the_registered_fonts() {
    let mut e = env(true);
    e.mgr
        .set_local_family_lookup(Some(Arc::new(|n: &str| n == "Installed Sans")));
    let r = rule(
        1,
        "Aliased",
        vec![
            FaceSourceSpec::Local("Missing One".into()),
            FaceSourceSpec::Local("Installed Sans".into()),
            url_src(&e.base, "noto.ttf", None),
        ],
        &e.base,
    );
    let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
    e.mgr.request_face(e.doc, ids[0]).unwrap();
    assert!(
        matches!(face_state(&e, ids[0]), FaceState::LocalFamily(ref f) if f == "Installed Sans")
    );
    // without a lookup (pinned mode) the local() entry fails and the next source is used
    let mut e = env(true);
    let r = rule(
        1,
        "NoLookup",
        vec![
            FaceSourceSpec::Local("Installed Sans".into()),
            url_src(&e.base, "noto.ttf", None),
        ],
        &e.base,
    );
    let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
    e.mgr.request_face(e.doc, ids[0]).unwrap();
    assert!(matches!(
        face_state(&e, ids[0]),
        FaceState::Loading { source_index: 1 }
    ));
}

// ---- WOFF 1.0 and format hints (L-FONTFACE-r12) --------------------------------------------------

#[test]
fn woff1_faces_load_as_their_sfnt_and_a_malformed_woff2_fails_to_decode() {
    for det in [false, true] {
        let mut e = env(det);
        let ttf = std::fs::read(fixture_font()).unwrap();
        e.t.write(
            "noto.woff",
            &ui4me_assets::fonts::validate::make_woff1(&ttf, true),
        );
        let r = rule(
            1,
            "Web",
            vec![
                url_src(&e.base, "x.woff2", Some("woff2")),
                url_src(&e.base, "noto.woff", Some("woff")),
            ],
            &e.base,
        );
        let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
        e.mgr.request_face(e.doc, ids[0]).unwrap();
        settle(&mut e.mgr, e.doc);
        e.mgr.process_completions();
        match face_state(&e, ids[0]) {
            FaceState::Loaded(b) => {
                assert!(
                    ui4me_assets::fonts::validate::validate_font(&b.bytes).is_ok(),
                    "the face holds the decoded sfnt"
                );
                assert_eq!(&b.bytes[..4], &ttf[..4]);
            }
            other => panic!("det={det}: {other:?}"),
        }
        // the stub is a malformed WOFF2 file (TXT-1: WOFF2 is decoded now): DECODE_FAILED, hint or not
        let mut e = env(det);
        let r = rule(
            1,
            "Web2",
            vec![url_src(&e.base, "woff2-stub.woff2", None)],
            &e.base,
        );
        let ids = e.mgr.sync_font_faces(e.doc, &[r]).unwrap();
        e.mgr.request_face(e.doc, ids[0]).unwrap();
        settle(&mut e.mgr, e.doc);
        e.mgr.process_completions();
        assert!(
            matches!(face_state(&e, ids[0]), FaceState::Failed(ref c) if *c == vec![Code::DecodeFailed]),
            "det={det}"
        );
    }
}

#[test]
fn unknown_and_unsupported_format_hints_skip_the_source_unread() {
    let mut e = env(true);
    // none of these files exist: a read would give FILE_NOT_FOUND
    let srcs = ["embedded-opentype", "svg", "something-new"]
        .map(|h| url_src(&e.base, "absent.bin", Some(h)));
    let ids = e
        .mgr
        .sync_font_faces(e.doc, &[rule(1, "Skipped", srcs.to_vec(), &e.base)])
        .unwrap();
    e.mgr.request_face(e.doc, ids[0]).unwrap();
    assert!(
        matches!(face_state(&e, ids[0]), FaceState::Failed(ref c) if c.iter().all(|c| *c == Code::UnsupportedMediaType) && c.len() == 3)
    );
    // supported hints are read
    let mut e = env(true);
    for h in [
        "truetype",
        "opentype",
        "collection",
        "woff",
        "woff2",
        "woff2-variations",
        "truetype-variations",
        "WOFF",
    ] {
        let ids = e
            .mgr
            .sync_font_faces(
                e.doc,
                &[rule(
                    9,
                    "Read",
                    vec![url_src(&e.base, "absent.bin", Some(h))],
                    &e.base,
                )],
            )
            .unwrap();
        e.mgr.request_face(e.doc, ids[0]).unwrap();
        assert!(
            matches!(face_state(&e, ids[0]), FaceState::Loading { .. }),
            "{h} is read"
        );
        e.mgr.sync_font_faces(e.doc, &[]).unwrap();
    }
}

#[test]
fn data_uri_faces_load() {
    let mut e = env(true);
    let ttf = std::fs::read(fixture_font()).unwrap();
    // minimal base64 encoder (the crate has no base64 dependency)
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::new();
    for c in ttf.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                b64.push(T[(n >> (18 - 6 * i) & 63) as usize] as char)
            } else {
                b64.push('=')
            }
        }
    }
    let uri = format!("data:font/ttf;base64,{b64}");
    let ids = e
        .mgr
        .sync_font_faces(
            e.doc,
            &[rule(
                1,
                "Inline",
                vec![url_src(&e.base, &uri, Some("truetype"))],
                &e.base,
            )],
        )
        .unwrap();
    e.mgr.request_face(e.doc, ids[0]).unwrap();
    settle(&mut e.mgr, e.doc);
    e.mgr.process_completions();
    assert!(
        matches!(face_state(&e, ids[0]), FaceState::Loaded(ref b) if b.bytes.len() == ttf.len())
    );
}
