// owner: 007-resources (T038) — file-access and network monitor (Linux part).
//
// Two independent observations, both without any extra tool:
//  * file access: the set of regular files the process has open is read from /proc/self/fd while
//    a request runs (`OpenFiles`), and the process-wide count of `socket`s is read from
//    /proc/self/net (`socket_inodes`), which must not grow;
//  * the guard's own surface: ui4me-resources exposes no write/create/delete operation, which the
//    `no_write_surface` test checks by scanning the crate sources for write APIs.
//
// The Windows ETW variant is T038/T044 work that needs the Windows machine; it is not implemented.
#![allow(dead_code, clippy::collapsible_if, clippy::unnecessary_min_or_max)]

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Files currently open by this process (resolved `/proc/self/fd` targets).
pub fn open_files() -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir("/proc/self/fd") {
        for e in rd.flatten() {
            if let Ok(t) = std::fs::read_link(e.path()) {
                if t.is_absolute() {
                    out.insert(t);
                }
            }
        }
    }
    out
}

/// Socket file descriptors open in this process (`socket:[inode]` targets). Any growth during a
/// resource run means a network path was exercised.
pub fn sockets() -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir("/proc/self/fd") {
        for e in rd.flatten() {
            if let Ok(t) = std::fs::read_link(e.path()) {
                if t.to_string_lossy().starts_with("socket:") {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Source files of the crate (for the no-write-surface scan).
pub fn crate_sources() -> Vec<PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
    }
    let mut v = Vec::new();
    walk(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut v,
    );
    v
}
