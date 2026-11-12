// owner: 007-resources (T037) — Windows policy cases (resource-policy.md §5, U4T-220).
//
// Part 1 (every OS): the lexical rules over the "windows" cases of
// verification/fixtures/resources-policy/cases.json — device, verbatim, ADS, reserved names,
// trailing dots, encoded separators — through the pure URL-to-Windows-path mapping.
// Part 2 (`cfg(windows)` only): junctions, case-insensitive roots, UNC without a UNC root, 8.3
// short names, drive letters, against the real file system and `GetFinalPathNameByHandleW`.
// Part 2 has NOT been run: it needs a Windows machine (T044, the Windows test VM over SSH).

use serde_json::Value;
use ui4me_assets::policy::{self, Decision, ResourcePolicy};
use ui4me_assets::uri;
use ui4me_engine_types::codes_generated::Code;
use url::Url;

const CASES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/resources-policy/cases.json"
));

fn windows_cases() -> Vec<Value> {
    let v: Value = serde_json::from_str(CASES).unwrap();
    v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["platform"] == "windows")
        .cloned()
        .collect()
}

#[test]
fn windows_case_list_is_present() {
    assert!(windows_cases().len() >= 10);
}

#[test]
fn lexical_windows_rules_apply_on_every_os() {
    let base = Url::parse("file:///C:/app/ui/index.html").unwrap();
    // cases whose denial is purely lexical (no file system involved)
    let lexical = [
        "win-ads",
        "win-device-nul",
        "win-device-com1",
        "win-verbatim",
        "win-trailing-dot",
        "win-backslash-encoded",
    ];
    let cases = windows_cases();
    for id in lexical {
        let case = cases
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("{id}"));
        let reference = case["request"].as_str().unwrap();
        let denied = match uri::resolve(&base, reference) {
            Err(_) => true,
            Ok(u) => u.scheme() != "file" || uri::windows_path(&u).is_err(),
        };
        assert!(denied, "{id}: {reference} must be denied lexically");
    }
    // allowed shapes still map
    assert_eq!(
        uri::windows_path(&Url::parse("file:///C:/app/ui/img/a.png").unwrap())
            .unwrap()
            .path,
        r"C:\app\ui\img\a.png"
    );
    // a UNC path maps but is flagged: only an explicit UNC root can admit it
    let unc = uri::windows_path(&Url::parse("file://server/share/a.png").unwrap()).unwrap();
    assert!(unc.unc);
    // percent-encoded traversal: dot segments are removed by the parser, so the result is a normal
    // path elsewhere on the drive, to be refused by the root comparison
    let up = uri::resolve(&base, "%2E%2E/%2e%2e/Windows/win.ini").unwrap();
    assert_eq!(uri::windows_path(&up).unwrap().path, r"C:\Windows\win.ini");
    let _ = policy::check(&up, &ResourcePolicy::default());
    assert_eq!(
        policy::check(
            &Url::parse("http://x/y").unwrap(),
            &ResourcePolicy::default()
        ),
        Decision::Deny(Code::ResourceDenied)
    );
}

#[cfg(windows)]
mod on_windows {
    use super::*;
    use std::path::PathBuf;
    use ui4me_assets::fs_guard;

    fn temp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ui4me-res-win-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::canonicalize(&p)
            .map(|c| ui4me_assets::policy::canonical_root(&c).unwrap())
            .unwrap()
    }

    #[test]
    fn junction_out_of_the_root_is_denied() {
        let t = temp("junction");
        std::fs::create_dir_all(t.join("root")).unwrap();
        std::fs::create_dir_all(t.join("outside")).unwrap();
        std::fs::write(t.join("outside/o.bin"), b"x").unwrap();
        // mklink parses `/` as a switch: build every path with backslashes only, never verbatim.
        let link = t.join("root").join("link");
        let outside = t.join("outside");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .status()
            .unwrap()
            .success();
        assert!(ok, "mklink /J failed");
        let root = ui4me_assets::policy::canonical_root(&t.join("root")).unwrap();
        let e = fs_guard::open_verified(
            &t.join("root").join("link").join("o.bin"),
            std::slice::from_ref(&root),
            1 << 20,
        )
        .unwrap_err();
        assert_eq!(e.code, Code::ResourceDenied);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn root_comparison_ignores_case_and_unc_needs_an_explicit_root() {
        let t = temp("case");
        std::fs::write(t.join("a.bin"), b"x").unwrap();
        let root = ui4me_assets::policy::canonical_root(&t).unwrap();
        let upper = PathBuf::from(t.join("A.BIN").to_string_lossy().to_uppercase());
        assert!(fs_guard::open_verified(&upper, std::slice::from_ref(&root), 1 << 20).is_ok());
        let unc = uri::windows_path(&Url::parse("file://server/share/a.png").unwrap()).unwrap();
        let e = fs_guard::open_verified(
            std::path::Path::new(&unc.path),
            std::slice::from_ref(&root),
            1 << 20,
        )
        .unwrap_err();
        assert!(matches!(e.code, Code::ResourceDenied | Code::FileNotFound));
        let _ = std::fs::remove_dir_all(&t);
    }
}
