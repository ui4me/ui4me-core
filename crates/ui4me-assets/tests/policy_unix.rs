// owner: 007-resources (T008, T036) — Linux policy suite (U4T-220, U4T-221).
//
// Runs verification/fixtures/resources-policy/cases.json (every case with platform "unix") plus
// the targeted guard tests: canonical roots, implicit root, symlink swap race, regular files only,
// size before reading, the file/network monitor.
//
// Common tree built under a temporary directory for every case:
//   app/ui/index.html            text
//   app/ui/img/a.png             fake 4x3        app/ui/img/B.PNG  fake 4x3
//   app/ui/sub/deep/c.png        fake 4x3
//   app/secret.png               fake 4x3        (parent of the document directory)
//   outside/o.png                fake 4x3        (outside every root)
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::path::Path;
use support::*;
use ui4me_assets::decode::fake::FakeImage;
use ui4me_assets::policy::ResourcePolicyPatch;
use ui4me_assets::request::Requester;
use ui4me_assets::uri;
use ui4me_assets::view::ImageState;
use ui4me_engine_types::codes_generated::Code;
use url::Url;

const CASES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/resources-policy/cases.json"
));

fn fake(w: u32, h: u32) -> Vec<u8> {
    FakeImage::new(w, h).bytes()
}

fn write_spec(t: &TempDir, rel: &str, spec: &str) {
    let bytes = if let Some(wh) = spec.strip_prefix("fake:") {
        let (w, h) = wh.split_once('x').unwrap();
        fake(w.parse().unwrap(), h.parse().unwrap())
    } else if let Some(n) = spec.strip_prefix("size:") {
        vec![0u8; n.parse().unwrap()]
    } else if let Some(s) = spec.strip_prefix("text:") {
        s.as_bytes().to_vec()
    } else if let Some(h) = spec.strip_prefix("hex:") {
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect()
    } else {
        panic!("bad file spec {spec}")
    };
    t.write(rel, &bytes);
}

fn build_tree(t: &TempDir, extra: Option<&Value>) {
    t.write("app/ui/index.html", b"<p>");
    for rel in [
        "app/ui/img/a.png",
        "app/ui/img/B.PNG",
        "app/ui/sub/deep/c.png",
        "app/secret.png",
        "outside/o.png",
    ] {
        t.write(rel, &fake(4, 3));
    }
    let Some(extra) = extra else { return };
    let tmp = t.path().to_str().unwrap();
    if let Some(files) = extra.get("files").and_then(Value::as_object) {
        for (rel, spec) in files {
            write_spec(t, rel, spec.as_str().unwrap());
        }
    }
    for d in extra
        .get("dirs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        std::fs::create_dir_all(t.join(d.as_str().unwrap())).unwrap();
    }
    for (link, target) in extra
        .get("symlinks")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let p = t.join(link);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target.as_str().unwrap().replace("{tmp}", tmp), p).unwrap();
    }
    for (link, target) in extra
        .get("hardlinks")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        std::fs::hard_link(t.join(target.as_str().unwrap()), t.join(link)).unwrap();
    }
    for f in extra
        .get("fifos")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let p = t.join(f.as_str().unwrap());
        let ok = std::process::Command::new("mkfifo")
            .arg(&p)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "mkfifo failed");
    }
}

fn patch_of(
    policy: Option<&Value>,
    roots: Vec<std::path::PathBuf>,
    implicit: bool,
) -> ResourcePolicyPatch {
    let mut p = ResourcePolicyPatch {
        implicit_document_root: Some(implicit),
        file_roots: Some(roots),
        ..Default::default()
    };
    if let Some(o) = policy.and_then(Value::as_object) {
        for (k, v) in o {
            let n = v.as_u64().unwrap();
            match k.as_str() {
                "max_encoded_image" => p.max_encoded_image = Some(n),
                "max_decoded_pixels" => p.max_decoded_pixels = Some(n),
                "data_url_max_bytes" => p.data_url_max_bytes = Some(n),
                other => panic!("unknown policy key {other}"),
            }
        }
    }
    p
}

/// Run one case; returns the observed code name ("OK" for Ready).
fn run_case(case: &Value, deterministic: bool) -> String {
    let t = TempDir::new("policy");
    build_tree(&t, case.get("extra"));
    let tmp = t.path().to_str().unwrap().to_string();
    let roots: Vec<_> = case["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let r = r.as_str().unwrap();
            match r.strip_prefix("abs:") {
                Some(a) => Path::new(a).to_path_buf(),
                None => t.join(r),
            }
        })
        .collect();
    let implicit = case["implicit_root"].as_bool().unwrap();
    let mut mgr = manager(deterministic);
    let doc = doc_id(1);
    let (doc_url, doc_file) = match case["doc"].as_str().unwrap() {
        "file" => {
            let f = t.join("app/ui/index.html");
            (uri::document_url_from_file(&f).unwrap(), Some(f))
        }
        "string_base_file" => (Url::from_directory_path(t.join("app/ui")).unwrap(), None),
        "string_none" => (uri::document_url_from_string(None).unwrap(), None),
        other => panic!("doc kind {other}"),
    };
    let patch = patch_of(case.get("policy"), roots, implicit);
    if let Err(e) = mgr.create_document(doc, doc_url.clone(), doc_file.as_deref(), &patch) {
        return e.code.name().to_string();
    }
    let request = case["request"].as_str().unwrap().replace("{tmp}", &tmp);
    let r = mgr
        .request_image(
            doc,
            Requester::ImgElement(node_id(1)),
            &uri::document_base_url(&doc_url, None),
            &request,
            1.0,
        )
        .unwrap();
    settle(&mut mgr, doc);
    match mgr.natural_size(&r) {
        ImageState::Ready { .. } => "OK".into(),
        ImageState::Broken { code } => code.name().into(),
        ImageState::Pending => panic!("still pending"),
    }
}

#[test]
fn policy_suite_has_at_least_60_linux_cases() {
    let v: Value = serde_json::from_str(CASES).unwrap();
    let manifest: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/resources-policy/corpus-manifest.json"
    )))
    .unwrap();
    assert_eq!(manifest["suite"], "resources-policy");
    let cases = v["cases"].as_array().unwrap();
    let mut ids = std::collections::HashSet::new();
    for case in cases {
        assert!(
            ids.insert(case["id"].as_str().unwrap()),
            "duplicate case ID"
        );
        assert!(
            ["unix", "windows"].contains(&case["platform"].as_str().unwrap()),
            "unknown platform"
        );
    }
    let n = cases.iter().filter(|c| c["platform"] == "unix").count();
    assert!(n >= 60, "{n} cases");
    assert_eq!(n as u64, manifest["counts"]["unix"].as_u64().unwrap());
    assert_eq!(
        (cases.len() - n) as u64,
        manifest["counts"]["windows"].as_u64().unwrap()
    );
}

#[test]
fn policy_suite_async_mode() {
    let v: Value = serde_json::from_str(CASES).unwrap();
    let mut failures = Vec::new();
    for case in v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["platform"] == "unix")
    {
        let got = run_case(case, false);
        if got != case["expect"].as_str().unwrap() {
            failures.push(format!(
                "{}: expected {}, got {got}",
                case["id"], case["expect"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing cases:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn policy_suite_deterministic_mode_gives_identical_results() {
    let v: Value = serde_json::from_str(CASES).unwrap();
    for case in v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["platform"] == "unix")
    {
        let a = run_case(case, true);
        assert_eq!(a, case["expect"].as_str().unwrap(), "{}", case["id"]);
    }
}

#[test]
fn roots_are_canonicalised_at_policy_creation() {
    let t = TempDir::new("roots");
    t.write("real/a.png", &fake(1, 1));
    std::os::unix::fs::symlink(t.join("real"), t.join("alias")).unwrap();
    let mut mgr = manager(true);
    let doc = doc_id(1);
    let patch = ResourcePolicyPatch {
        file_roots: Some(vec![t.join("alias")]),
        ..Default::default()
    };
    mgr.create_document(doc, uri::about_blank(), None, &patch)
        .unwrap();
    assert_eq!(mgr.file_roots(doc).unwrap(), vec![t.join("real")]);
}

#[test]
fn implicit_root_is_the_directory_subtree_never_the_parent() {
    let t = TempDir::new("implicit");
    t.write("app/ui/index.html", b"x");
    let mut mgr = manager(true);
    let doc = doc_id(1);
    let f = t.join("app/ui/index.html");
    mgr.create_document(
        doc,
        uri::document_url_from_file(&f).unwrap(),
        Some(&f),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(mgr.file_roots(doc).unwrap(), vec![t.join("app/ui")]);
    let s = doc_id(2);
    mgr.create_document(
        s,
        Url::from_directory_path(t.join("app/ui")).unwrap(),
        None,
        &Default::default(),
    )
    .unwrap();
    assert!(
        mgr.file_roots(s).unwrap().is_empty(),
        "string documents get no implicit root, even with a file: base"
    );
}

/// A symlink swapped between check and open (and back) must never expose the outside file:
/// the guard verifies the opened handle, not the path string.
#[test]
fn symlink_swap_race_never_leaks_the_outside_file() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let t = TempDir::new("swap");
    t.write("root/inside.bin", b"INSIDE");
    t.write("outside/secret.bin", b"SECRET");
    let root = std::fs::canonicalize(t.join("root")).unwrap();
    let link = t.join("root/swap.bin");
    std::os::unix::fs::symlink(t.join("root/inside.bin"), &link).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let flipper = {
        let (stop, link, inside, outside) = (
            stop.clone(),
            link.clone(),
            t.join("root/inside.bin"),
            t.join("outside/secret.bin"),
        );
        std::thread::spawn(move || {
            let tmp = link.with_extension("tmp");
            let mut flip = false;
            while !stop.load(Ordering::Relaxed) {
                let target = if flip { &outside } else { &inside };
                let _ = std::fs::remove_file(&tmp);
                std::os::unix::fs::symlink(target, &tmp).unwrap();
                std::fs::rename(&tmp, &link).unwrap(); // atomic replace
                flip = !flip;
            }
        })
    };
    let (mut ok_inside, mut denied) = (0, 0);
    for _ in 0..4000 {
        match ui4me_assets::fs_guard::open_verified(&link, std::slice::from_ref(&root), 1 << 20) {
            Ok(vf) => {
                let bytes = ui4me_assets::fs_guard::read_limited(&vf.file, 1 << 20).unwrap();
                assert_eq!(
                    bytes, b"INSIDE",
                    "the outside file was read through a swapped link"
                );
                ok_inside += 1;
            }
            Err(e) => {
                assert_eq!(e.code, Code::ResourceDenied);
                denied += 1;
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    flipper.join().unwrap();
    assert!(ok_inside + denied == 4000);
}

#[test]
fn size_is_checked_before_reading() {
    let t = TempDir::new("size");
    t.write("a.bin", &vec![0u8; 5000]);
    let root = t.path().to_path_buf();
    let e =
        ui4me_assets::fs_guard::open_verified(&t.join("a.bin"), std::slice::from_ref(&root), 4096)
            .unwrap_err();
    assert_eq!(e.code, Code::SizeLimit);
    assert!(
        ui4me_assets::fs_guard::open_verified(&t.join("a.bin"), std::slice::from_ref(&root), 5000)
            .is_ok()
    );
    // a file that grows after fstat is still bounded by the read
    let vf =
        ui4me_assets::fs_guard::open_verified(&t.join("a.bin"), std::slice::from_ref(&root), 5000)
            .unwrap();
    std::fs::write(t.join("a.bin"), vec![0u8; 9000]).unwrap();
    assert_eq!(
        ui4me_assets::fs_guard::read_limited(&vf.file, 5000)
            .unwrap_err()
            .code,
        Code::SizeLimit
    );
}

#[test]
fn only_regular_files_are_accepted() {
    let t = TempDir::new("regular");
    t.write("d/x", b"x");
    let root = t.path().to_path_buf();
    let e = ui4me_assets::fs_guard::open_verified(&t.join("d"), std::slice::from_ref(&root), 100)
        .unwrap_err();
    assert_eq!(e.code, Code::ResourceDenied);
}

#[test]
fn no_network_and_only_expected_files_are_touched() {
    let t = TempDir::new("monitor");
    build_tree(&t, None);
    let mut mgr = manager(false);
    let doc = doc_id(1);
    let f = t.join("app/ui/index.html");
    let url = uri::document_url_from_file(&f).unwrap();
    mgr.create_document(doc, url.clone(), Some(&f), &Default::default())
        .unwrap();
    let sockets_before = monitor::sockets();
    for r in [
        "img/a.png",
        "http://example.com/x.png",
        "../secret.png",
        "https://example.com/y.png",
    ] {
        mgr.request_image(doc, Requester::ImgElement(node_id(7)), &url, r, 1.0)
            .unwrap();
        settle(&mut mgr, doc);
    }
    assert_eq!(
        monitor::sockets(),
        sockets_before,
        "a socket was opened during resource loading"
    );
    // nothing remains open after the requests settle
    let open = monitor::open_files();
    assert!(
        !open.iter().any(|p| p.starts_with(t.path())),
        "files left open: {open:?}"
    );
}

#[test]
fn the_crate_has_no_write_surface() {
    let forbidden = [
        "fs::write",
        "File::create",
        "create_dir",
        "remove_file",
        "remove_dir",
        "fs::rename",
        "OpenOptions::new().write",
        ".write(true)",
        ".append(true)",
        "TcpStream",
        "UdpSocket",
        "TcpListener",
    ];
    for src in monitor::crate_sources() {
        let text = std::fs::read_to_string(&src).unwrap();
        // test modules inside src may create files; scan only the non-test part
        let body = text.split("#[cfg(test)]").next().unwrap();
        for f in forbidden {
            assert!(!body.contains(f), "{} contains `{f}`", src.display());
        }
    }
}
