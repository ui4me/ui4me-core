// owner: 007-resources (T057) — build-time check of the embedded last-resort font (R-54, IF-52).
//
// The bytes compiled in by `src/fonts/last_resort.rs` must match the pinned SHA-256 in
// `assets/last-resort/SHA256SUMS`; a mismatch fails the build.

use sha2::{Digest, Sha256};
use std::path::Path;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/last-resort");
    let sums = dir.join("SHA256SUMS");
    println!("cargo:rerun-if-changed={}", sums.display());
    println!(
        "cargo:rerun-if-changed={}",
        dir.join("NotoSans-Regular.ttf").display()
    );
    println!("cargo:rerun-if-changed={}", dir.join("OFL.txt").display());
    let manifest =
        std::fs::read_to_string(&sums).expect("assets/last-resort/SHA256SUMS is missing");
    for line in manifest.lines().filter(|l| !l.trim().is_empty()) {
        let (want, name) = line
            .split_once(char::is_whitespace)
            .expect("malformed SHA256SUMS line");
        let name = name.trim();
        let bytes = std::fs::read(dir.join(name))
            .unwrap_or_else(|e| panic!("last-resort asset {name}: {e}"));
        let got = hex(&Sha256::digest(&bytes));
        if got != want {
            panic!(
                "last-resort asset {name} does not match the pinned SHA-256 (expected {want}, found {got})"
            );
        }
    }
    // the font entry itself must be present in the manifest
    assert!(
        manifest.contains("NotoSans-Regular.ttf"),
        "SHA256SUMS does not pin NotoSans-Regular.ttf"
    );
}
