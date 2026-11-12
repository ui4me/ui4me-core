//! owner: 007-resources (T011, T039) — the file guard.
//!
//! Open, then verify: the file is opened normally (links are followed), then the **opened handle's**
//! final path is compared with the canonical roots by path components, then `fstat` must report a
//! regular file and the size is checked against the kind's limit before reading. Path-string checks
//! alone are never trusted, so a symlink or junction swapped between check and open cannot escape.
//!
//! Nothing here writes, creates, deletes or lists: the only operations are open (read-only) and read.

#[cfg(unix)]
pub mod unix;
pub mod windows;

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

/// Content identity of a file for the cache key: size and modification time (data-model).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub size: u64,
    pub mtime_ns: u128,
}

/// A file that passed the guard.
#[derive(Debug)]
pub struct VerifiedFile {
    pub file: File,
    /// The opened handle's final path (never the requested string).
    pub final_path: PathBuf,
    pub identity: FileIdentity,
}

fn status(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

fn io_code(e: &std::io::Error) -> Code {
    if e.kind() == std::io::ErrorKind::NotFound {
        Code::FileNotFound
    } else {
        Code::ResourceDenied
    }
}

/// Whether `path` lies inside `root`, comparing components (never string prefixes, so `/app2` is
/// not inside `/app`); case-insensitively when `case_insensitive` (Windows).
pub fn path_within(path: &Path, root: &Path, case_insensitive: bool) -> bool {
    let mut p = path.components();
    for rc in root.components() {
        match p.next() {
            Some(pc) if comp_eq(&pc, &rc, case_insensitive) => {}
            _ => return false,
        }
    }
    true
}

fn comp_eq(a: &Component, b: &Component, ci: bool) -> bool {
    if ci {
        a.as_os_str().to_string_lossy().to_lowercase()
            == b.as_os_str().to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

fn identity_of(md: &std::fs::Metadata) -> FileIdentity {
    let mtime_ns = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    FileIdentity {
        size: md.len(),
        mtime_ns,
    }
}

fn finish(file: File, final_path: PathBuf, max_bytes: u64) -> Result<VerifiedFile, Status> {
    let md = file
        .metadata()
        .map_err(|e| status(io_code(&e), "fstat failed"))?;
    if !md.is_file() {
        return Err(status(Code::ResourceDenied, "not a regular file"));
    }
    if md.len() > max_bytes {
        return Err(status(
            Code::SizeLimit,
            format!("file is {} bytes; the limit is {max_bytes}", md.len()),
        ));
    }
    Ok(VerifiedFile {
        file,
        final_path,
        identity: identity_of(&md),
    })
}

/// Open `path` for a document request and verify it against `roots` (open-then-verify).
///
/// * not existing: `FILE_NOT_FOUND`; unreadable, outside every root, or not a regular file:
///   `RESOURCE_DENIED`; larger than `max_bytes`: `SIZE_LIMIT`.
pub fn open_verified(
    path: &Path,
    roots: &[PathBuf],
    max_bytes: u64,
) -> Result<VerifiedFile, Status> {
    if path.as_os_str().is_empty() || path.to_string_lossy().contains('\0') {
        return Err(status(Code::ResourceDenied, "invalid path"));
    }
    let ci = cfg!(windows);
    let (file, final_path) = match platform_open(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // A missing file outside every root is a denial, not "not found": the answer must not
            // reveal what exists beyond the roots (decision D-007-02).
            return if roots
                .iter()
                .any(|r| path_within(&resolve_existing_prefix(path), r, ci))
            {
                Err(status(Code::FileNotFound, "file not found"))
            } else {
                Err(status(
                    Code::ResourceDenied,
                    "path is outside every configured root",
                ))
            };
        }
        Err(e) => return Err(status(io_code(&e), format!("open failed: {e}"))),
    };
    if !roots.iter().any(|r| path_within(&final_path, r, ci)) {
        return Err(status(
            Code::ResourceDenied,
            "file is outside every configured root",
        ));
    }
    finish(file, final_path, max_bytes)
}

/// Canonicalise the longest existing prefix of `path` and re-append the missing tail, so a
/// not-yet-existing file is judged where it would live (symlinked ancestors resolved).
fn resolve_existing_prefix(path: &Path) -> PathBuf {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Ok(c) = std::fs::canonicalize(&cur) {
            let mut out = c;
            for t in tail.iter().rev() {
                out.push(t);
            }
            // `canonicalize` yields `\\?\` paths on Windows; roots are stored without the prefix.
            return crate::policy::strip_verbatim(out);
        }
        match (
            cur.file_name().map(|n| n.to_owned()),
            cur.parent().map(|p| p.to_path_buf()),
        ) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                cur = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Open a host-supplied path (trusted declaration, Clarification Q2): no root check; the file must
/// exist, be a regular file and be within the size limit.
pub fn open_trusted(path: &Path, max_bytes: u64) -> Result<VerifiedFile, Status> {
    if path.to_string_lossy().contains('\0') {
        return Err(status(Code::ResourceDenied, "invalid path"));
    }
    let (file, final_path) =
        platform_open(path).map_err(|e| status(io_code(&e), format!("open failed: {e}")))?;
    finish(file, final_path, max_bytes)
}

#[cfg(unix)]
fn platform_open(path: &Path) -> std::io::Result<(File, PathBuf)> {
    unix::open_with_final_path(path)
}

#[cfg(windows)]
fn platform_open(path: &Path) -> std::io::Result<(File, PathBuf)> {
    windows::open_with_final_path(path)
}

/// Read the whole file, never more than `max_bytes` (the file may grow after `fstat`).
pub fn read_limited(file: &File, max_bytes: u64) -> Result<Vec<u8>, Status> {
    let mut buf = Vec::new();
    let mut limited = file.take(max_bytes.saturating_add(1));
    limited
        .read_to_end(&mut buf)
        .map_err(|e| status(io_code(&e), format!("read failed: {e}")))?;
    if buf.len() as u64 > max_bytes {
        return Err(status(
            Code::SizeLimit,
            format!("file exceeds {max_bytes} bytes"),
        ));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_not_string_prefixes() {
        assert!(path_within(
            Path::new("/app/ui/a.png"),
            Path::new("/app"),
            false
        ));
        assert!(!path_within(
            Path::new("/app2/a.png"),
            Path::new("/app"),
            false
        ));
        assert!(!path_within(Path::new("/app"), Path::new("/app/ui"), false));
        assert!(path_within(Path::new("C:\\App\\a.png"), Path::new("c:\\app"), true) || cfg!(unix));
    }

    #[test]
    fn case_insensitive_comparison() {
        assert!(path_within(
            Path::new("/App/A.png"),
            Path::new("/app"),
            true
        ));
        assert!(!path_within(
            Path::new("/App/A.png"),
            Path::new("/app"),
            false
        ));
    }
}
