//! owner: 007-resources (T075, T078) — `ui4me-res:` bundle registry.
//!
//! **OD-19 decision (T075): the `ui4me-res:` scheme is APPROVED.** Recorded 2026-11-06 under the
//! owner's standing approval E-029 (`verification/gates/AUTHORISATIONS.md`, verbatim: "all future
//! approvals are approved , work autonomously"), routed to this feature by D-083 (A14 / OD-19).
//! `ResourcePolicy::ui4me_res_scheme` therefore defaults to `true`; the unknown-scheme denial
//! that stood before the approval (FR-004) now applies to every other scheme only.
//!
//! A *bundle* is a host-registered, named set of immutable byte resources addressed as
//! `ui4me-res://<bundle>/<path>` (`contracts/uri-resolution.md` §3.4: a non-special URL with host
//! `<bundle>`). The registry is runtime-wide and shared with the worker threads; a lookup is an
//! exact-key match on the percent-decoded path, so a reference can never leave the bundle's key
//! space: there is no directory walking and no fall-through to the file system. Registering a
//! name that exists replaces the bundle (new generation); the manager then re-requests the
//! images that came from it and the documents that referenced it advance their resource epoch.
//!
//! Limits (`SIZE_LIMIT`): at most [`MAX_ENTRIES`] entries and [`MAX_BUNDLE_BYTES`] bytes per
//! bundle, [`MAX_BUNDLES`] names and [`MAX_REGISTERED_BYTES`] registered source bytes across
//! the registry, paths at most [`MAX_PATH_BYTES`]. Replacement checks exclude the old bundle
//! atomically. The per-kind encoded limit of the reading document's policy applies when an
//! entry is read (as for files).

use crate::uri;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use url::Url;

/// The scheme name (without the colon).
pub const SCHEME: &str = "ui4me-res";
pub const MAX_ENTRIES: usize = 4096;
pub const MAX_BUNDLE_BYTES: u64 = 16 << 20;
/// Registry-wide bounds also cover many individually small registrations.
pub const MAX_BUNDLES: usize = 64;
pub const MAX_REGISTERED_BYTES: u64 = 16 << 20;
pub const MAX_PATH_BYTES: usize = 1024;
pub const MAX_NAME_BYTES: usize = 63;
const MAX_DIR_DEPTH: usize = 32;

fn status(code: Code, msg: impl Into<String>) -> Status {
    Status::new(code, Stage::Resource).with_message(msg)
}

/// One entry as the host supplies it.
#[derive(Clone, Debug)]
pub struct BundleEntryInput {
    /// Relative path, `/` separated (no leading `/`, no empty, `.` or `..` segment).
    pub path: String,
    pub data: Arc<[u8]>,
    /// Optional media type; used to detect SVG (else the extension decides).
    pub mime: Option<String>,
}

impl BundleEntryInput {
    pub fn new(path: impl Into<String>, data: impl Into<Arc<[u8]>>, mime: Option<String>) -> Self {
        BundleEntryInput {
            path: path.into(),
            data: data.into(),
            mime,
        }
    }
}

#[derive(Clone, Debug)]
struct Entry {
    data: Arc<[u8]>,
    mime: Option<String>,
}

/// An immutable registered bundle.
#[derive(Debug)]
pub struct Bundle {
    pub name: String,
    /// Unique across the registry; changes whenever the bundle is registered again.
    pub generation: u64,
    entries: HashMap<String, Entry>,
    pub total_bytes: u64,
}

impl Bundle {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

/// A resolved entry, owned (cheap clones of `Arc`s).
#[derive(Clone, Debug)]
pub struct BundleHit {
    pub bundle: String,
    pub path: String,
    pub generation: u64,
    pub data: Arc<[u8]>,
    pub mime: Option<String>,
}

impl BundleHit {
    /// SVG by the registered media type, else by extension (`uri-resolution.md` §4).
    pub fn is_svg(&self) -> bool {
        match &self.mime {
            Some(m) => m
                .split(';')
                .next()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("image/svg+xml")),
            None => self.path.to_ascii_lowercase().ends_with(".svg"),
        }
    }
}

/// `true` if `name` is a valid bundle name: `[a-z0-9][a-z0-9._-]{0,62}`.
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME_BYTES
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
        })
}

/// `true` if `path` is a valid entry path.
pub fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && path
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
        && !path.chars().any(|c| c.is_control() || c == '\\')
}

fn valid_mime(m: &str) -> bool {
    !m.is_empty() && m.len() <= 255 && m.bytes().all(|b| (0x20..0x7f).contains(&b))
}

/// The bundle name of a `ui4me-res:` URL (its host), if it has one.
pub fn bundle_of(url: &Url) -> Option<&str> {
    (url.scheme() == SCHEME).then(|| url.host_str()).flatten()
}

/// The prefix every URL of bundle `name` starts with.
pub fn url_prefix(name: &str) -> String {
    format!("{SCHEME}://{name}/")
}

#[derive(Default)]
struct Inner {
    bundles: HashMap<String, Arc<Bundle>>,
    next_generation: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BundleStats {
    pub bundles: usize,
    pub entries: usize,
    pub bytes: u64,
}

/// The runtime-wide bundle registry (shared with the worker threads; reads take a shared lock).
#[derive(Default)]
pub struct BundleRegistry {
    inner: RwLock<Inner>,
}

impl std::fmt::Debug for BundleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BundleRegistry")
            .field("bundles", &self.names())
            .finish()
    }
}

impl BundleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `entries` as bundle `name`, replacing a bundle of the same name. Returns the new
    /// bundle. Errors: `INVALID_ARGUMENT` (name, path, media type or duplicate path),
    /// `SIZE_LIMIT` (entry count or total bytes).
    pub fn register(
        &self,
        name: &str,
        entries: Vec<BundleEntryInput>,
    ) -> Result<Arc<Bundle>, Status> {
        if !valid_name(name) {
            return Err(status(
                Code::InvalidArgument,
                format!("bundle name {name:?} must match [a-z0-9][a-z0-9._-]{{0,62}}"),
            ));
        }
        if entries.len() > MAX_ENTRIES {
            return Err(status(
                Code::SizeLimit,
                format!("a bundle holds at most {MAX_ENTRIES} entries"),
            ));
        }
        let mut map = HashMap::with_capacity(entries.len());
        let mut total = 0u64;
        for e in entries {
            if !valid_path(&e.path) {
                return Err(status(
                    Code::InvalidArgument,
                    format!(
                        "bundle path {:?} must be relative, '/' separated, without empty, '.' or '..' segments and at most {MAX_PATH_BYTES} bytes",
                        e.path
                    ),
                ));
            }
            if e.mime.as_deref().is_some_and(|m| !valid_mime(m)) {
                return Err(status(
                    Code::InvalidArgument,
                    format!("bundle entry {:?}: malformed media type", e.path),
                ));
            }
            total += e.data.len() as u64;
            if total > MAX_BUNDLE_BYTES {
                return Err(status(
                    Code::SizeLimit,
                    format!("a bundle holds at most {MAX_BUNDLE_BYTES} bytes"),
                ));
            }
            let path = e.path.clone();
            if map
                .insert(
                    e.path,
                    Entry {
                        data: e.data,
                        mime: e.mime.filter(|m| !m.is_empty()),
                    },
                )
                .is_some()
            {
                return Err(status(
                    Code::InvalidArgument,
                    format!("duplicate bundle path {path:?}"),
                ));
            }
        }
        let mut inner = self.inner.write().unwrap_or_else(|p| p.into_inner());
        let old = inner.bundles.get(name);
        if old.is_none() && inner.bundles.len() >= MAX_BUNDLES {
            return Err(status(
                Code::SizeLimit,
                "too many registered resource bundles",
            ));
        }
        let registered: u64 = inner.bundles.values().map(|b| b.total_bytes).sum();
        let replaced = old.map_or(0, |b| b.total_bytes);
        if registered - replaced + total > MAX_REGISTERED_BYTES {
            return Err(status(
                Code::SizeLimit,
                "registered resource byte budget exceeded",
            ));
        }
        inner.next_generation += 1;
        let bundle = Arc::new(Bundle {
            name: name.to_string(),
            generation: inner.next_generation,
            entries: map,
            total_bytes: total,
        });
        inner.bundles.insert(name.to_string(), bundle.clone());
        Ok(bundle)
    }

    /// Remove a bundle; `true` if it existed (idempotent).
    pub fn unregister(&self, name: &str) -> bool {
        self.inner
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .bundles
            .remove(name)
            .is_some()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .bundles
            .contains_key(name)
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .inner
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .bundles
            .keys()
            .cloned()
            .collect();
        v.sort();
        v
    }

    pub fn stats(&self) -> BundleStats {
        let inner = self.inner.read().unwrap_or_else(|p| p.into_inner());
        BundleStats {
            bundles: inner.bundles.len(),
            entries: inner.bundles.values().map(|b| b.len()).sum(),
            bytes: inner.bundles.values().map(|b| b.total_bytes).sum(),
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<Bundle>> {
        self.inner
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .bundles
            .get(name)
            .cloned()
    }

    /// Resolve a `ui4me-res://<bundle>/<path>` URL (query and fragment ignored).
    ///
    /// * `URI_MALFORMED`: not a `ui4me-res:` URL with a host, or userinfo/port present;
    /// * `RESOURCE_DENIED`: the decoded path tries to leave the bundle (`.`/`..` segment, backslash,
    ///   NUL or a control character, invalid UTF-8): no entry can ever have such a path;
    /// * `FILE_NOT_FOUND`: unknown bundle or path (`resource-policy.md` §3).
    pub fn lookup(&self, url: &Url) -> Result<BundleHit, Status> {
        let Some(name) = bundle_of(url) else {
            return Err(status(
                Code::UriMalformed,
                "a ui4me-res: URL needs a bundle name as its host: ui4me-res://<bundle>/<path>",
            ));
        };
        if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
            return Err(status(
                Code::UriMalformed,
                "a ui4me-res: URL has no user information or port",
            ));
        }
        let raw = url.path().strip_prefix('/').unwrap_or(url.path());
        let decoded = String::from_utf8(uri::percent_decode(raw))
            .map_err(|_| status(Code::ResourceDenied, "bundle path is not valid UTF-8"))?;
        if decoded.is_empty() {
            return Err(status(
                Code::FileNotFound,
                format!("{url} names no resource"),
            ));
        }
        if decoded.chars().any(|c| c.is_control() || c == '\\')
            || decoded.split('/').any(|s| s == "." || s == "..")
        {
            return Err(status(
                Code::ResourceDenied,
                "bundle path escapes the bundle",
            ));
        }
        let inner = self.inner.read().unwrap_or_else(|p| p.into_inner());
        let bundle = inner.bundles.get(name).ok_or_else(|| {
            status(
                Code::FileNotFound,
                format!("no bundle {name:?} is registered"),
            )
        })?;
        let entry = bundle.entries.get(&decoded).ok_or_else(|| {
            status(
                Code::FileNotFound,
                format!("bundle {name:?} has no entry {decoded:?}"),
            )
        })?;
        Ok(BundleHit {
            bundle: name.to_string(),
            path: decoded,
            generation: bundle.generation,
            data: entry.data.clone(),
            mime: entry.mime.clone(),
        })
    }
}

/// A media type for a file name, for [`read_dir`] (only SVG detection consumes it).
pub fn mime_for_extension(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "txt" => "text/plain",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "ttc" => "font/collection",
        _ => return None,
    })
}

/// Read a directory tree into bundle entries (an immutable snapshot taken now). Paths are
/// relative to `dir`, `/` separated; media types come from [`mime_for_extension`]. Symbolic links
/// are refused (`RESOURCE_DENIED`: a bundle is a closed set of bytes), as are non-UTF-8 names
/// (`INVALID_ARGUMENT`); an unreadable file is `RESOURCE_DENIED`, a missing directory
/// `FILE_NOT_FOUND`; limits as for [`BundleRegistry::register`].
pub fn read_dir(dir: &Path) -> Result<Vec<BundleEntryInput>, Status> {
    let meta = std::fs::symlink_metadata(dir).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            Code::FileNotFound
        } else {
            Code::ResourceDenied
        };
        status(code, format!("bundle directory {}: {e}", dir.display()))
    })?;
    if !meta.is_dir() {
        return Err(status(
            Code::InvalidArgument,
            format!("{} is not a directory", dir.display()),
        ));
    }
    let mut out = Vec::new();
    let mut total = 0u64;
    walk(dir, "", 0, &mut out, &mut total)?;
    Ok(out)
}

fn walk(
    dir: &Path,
    prefix: &str,
    depth: usize,
    out: &mut Vec<BundleEntryInput>,
    total: &mut u64,
) -> Result<(), Status> {
    if depth > MAX_DIR_DEPTH {
        return Err(status(
            Code::SizeLimit,
            format!("bundle directory nests deeper than {MAX_DIR_DEPTH} levels"),
        ));
    }
    let read = std::fs::read_dir(dir)
        .map_err(|e| status(Code::ResourceDenied, format!("{}: {e}", dir.display())))?;
    let mut items: Vec<std::fs::DirEntry> = read
        .collect::<Result<_, _>>()
        .map_err(|e| status(Code::ResourceDenied, format!("{}: {e}", dir.display())))?;
    items.sort_by_key(|e| e.file_name());
    for item in items {
        let name = item.file_name().into_string().map_err(|n| {
            status(
                Code::InvalidArgument,
                format!("non-UTF-8 file name {n:?} in {}", dir.display()),
            )
        })?;
        let path = item.path();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| status(Code::ResourceDenied, format!("{}: {e}", path.display())))?;
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if meta.file_type().is_symlink() {
            return Err(status(
                Code::ResourceDenied,
                format!(
                    "{} is a symbolic link; bundles hold regular files only",
                    path.display()
                ),
            ));
        }
        if meta.is_dir() {
            walk(&path, &rel, depth + 1, out, total)?;
        } else if meta.is_file() {
            *total += meta.len();
            if *total > MAX_BUNDLE_BYTES || out.len() >= MAX_ENTRIES {
                return Err(status(
                    Code::SizeLimit,
                    "the directory exceeds the bundle limits",
                ));
            }
            let data = std::fs::read(&path)
                .map_err(|e| status(Code::ResourceDenied, format!("{}: {e}", path.display())))?;
            let mime = mime_for_extension(&rel).map(str::to_string);
            out.push(BundleEntryInput {
                path: rel,
                data: data.into(),
                mime,
            });
        }
    }
    Ok(())
}

/// What a bundle registration or removal did to the documents.
#[derive(Clone, Debug, Default)]
pub struct BundleChange {
    /// Live documents that referenced the bundle (their document URL, a linked sheet or an image
    /// request), whether or not that reference had succeeded: their resource epoch advances and
    /// their linked sheets are re-read. The host runtime applies the pipeline side.
    pub affected: Vec<ui4me_engine_types::ids::DocumentId>,
    /// `true` if a bundle of this name existed before (a replacement or a removal).
    pub existed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn reg() -> BundleRegistry {
        let r = BundleRegistry::new();
        r.register(
            "app",
            vec![
                BundleEntryInput::new("index.html", b"<p>".to_vec(), None),
                BundleEntryInput::new("img/my logo.svg", b"<svg/>".to_vec(), None),
                BundleEntryInput::new(
                    "x.bin",
                    b"1".to_vec(),
                    Some("image/svg+xml; charset=utf-8".into()),
                ),
            ],
        )
        .unwrap();
        r
    }

    #[test]
    fn names_and_paths() {
        assert!(valid_name("app") && valid_name("a.b_c-9"));
        for bad in ["", "App", "-a", "a/b", "a b", &"a".repeat(64)] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(valid_path("a/b.css"));
        for bad in [
            "", "/a", "a/", "a//b", "./a", "a/../b", "a\\b", "a\0b", "..",
        ] {
            assert!(!valid_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn lookup_decodes_and_ignores_query_and_fragment() {
        let r = reg();
        assert_eq!(
            &*r.lookup(&u("ui4me-res://app/index.html")).unwrap().data,
            b"<p>"
        );
        assert_eq!(
            &*r.lookup(&u("ui4me-res://app/img/my%20logo.svg?v=1#f"))
                .unwrap()
                .data,
            b"<svg/>"
        );
        assert!(
            r.lookup(&u("ui4me-res://app/img/my%20logo.svg"))
                .unwrap()
                .is_svg()
        );
        assert!(
            r.lookup(&u("ui4me-res://app/x.bin")).unwrap().is_svg(),
            "registered MIME wins over the extension"
        );
        assert!(!r.lookup(&u("ui4me-res://app/index.html")).unwrap().is_svg());
    }

    #[test]
    fn no_escape() {
        let r = reg();
        // `%2e%2e` is a dot segment for the URL parser: clamped at the bundle root
        assert_eq!(
            r.lookup(&u("ui4me-res://app/%2e%2e/etc")).unwrap_err().code,
            Code::FileNotFound
        );
        assert_eq!(
            r.lookup(&u("ui4me-res://app/a%2f..%2fb")).unwrap_err().code,
            Code::ResourceDenied
        );
        assert_eq!(
            r.lookup(&u("ui4me-res://app/a%5cb")).unwrap_err().code,
            Code::ResourceDenied
        );
        assert_eq!(
            r.lookup(&u("ui4me-res://app/a%00b")).unwrap_err().code,
            Code::ResourceDenied
        );
        // the URL parser clamps dot segments at the root of the bundle
        assert_eq!(
            &*r.lookup(&u("ui4me-res://app/img/../../index.html"))
                .unwrap()
                .data,
            b"<p>"
        );
        assert_eq!(
            r.lookup(&u("ui4me-res://other/index.html"))
                .unwrap_err()
                .code,
            Code::FileNotFound
        );
        assert_eq!(
            r.lookup(&u("ui4me-res://app/")).unwrap_err().code,
            Code::FileNotFound
        );
        assert_eq!(
            r.lookup(&u("ui4me-res://app/img")).unwrap_err().code,
            Code::FileNotFound
        );
        assert_eq!(
            r.lookup(&u("ui4me-res://app:80/index.html"))
                .unwrap_err()
                .code,
            Code::UriMalformed
        );
        assert_eq!(
            r.lookup(&u("ui4me-res:///index.html")).unwrap_err().code,
            Code::UriMalformed
        );
    }

    #[test]
    fn aggregate_budget_and_failed_replace_are_atomic() {
        let registry = BundleRegistry::new();
        let chunk: Arc<[u8]> = vec![0u8; 1 << 20].into();
        for i in 0..16 {
            registry
                .register(
                    &format!("b{i}"),
                    vec![BundleEntryInput::new("x", chunk.clone(), None)],
                )
                .unwrap();
        }
        assert_eq!(registry.stats().bytes, MAX_REGISTERED_BYTES);
        assert_eq!(
            registry
                .register("overflow", vec![BundleEntryInput::new("x", vec![0], None)])
                .unwrap_err()
                .code,
            Code::SizeLimit
        );
        let before = registry.get("b0").unwrap();
        let two: Arc<[u8]> = vec![0u8; 2 << 20].into();
        assert!(
            registry
                .register("b0", vec![BundleEntryInput::new("x", two, None)])
                .is_err()
        );
        assert_eq!(registry.get("b0").unwrap().generation, before.generation);
        registry
            .register("b0", vec![BundleEntryInput::new("x", vec![1], None)])
            .unwrap();
        assert_eq!(registry.stats().bytes, 15 * (1 << 20) + 1);
        assert!(registry.unregister("b1"));
        registry
            .register("replacement", vec![BundleEntryInput::new("x", chunk, None)])
            .unwrap();
        assert_eq!(registry.stats().bundles, 16);
    }

    #[test]
    fn bundle_name_count_is_bounded_even_for_empty_bundles() {
        let registry = BundleRegistry::new();
        for i in 0..MAX_BUNDLES {
            registry.register(&format!("b{i}"), vec![]).unwrap();
        }
        assert_eq!(
            registry.register("extra", vec![]).unwrap_err().code,
            Code::SizeLimit
        );
        registry.register("b0", vec![]).unwrap();
        assert_eq!(registry.stats().bundles, MAX_BUNDLES);
    }

    #[test]
    fn register_validates_replaces_and_unregisters() {
        let r = reg();
        let g1 = r.get("app").unwrap().generation;
        assert_eq!(
            r.register("App", vec![]).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            r.register("b", vec![BundleEntryInput::new("../x", b"".to_vec(), None)])
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        let dup = vec![
            BundleEntryInput::new("a", b"1".to_vec(), None),
            BundleEntryInput::new("a", b"2".to_vec(), None),
        ];
        assert_eq!(
            r.register("b", dup).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            r.register(
                "b",
                vec![BundleEntryInput::new(
                    "a",
                    b"1".to_vec(),
                    Some("bad\n".into())
                )]
            )
            .unwrap_err()
            .code,
            Code::InvalidArgument
        );
        assert!(!r.contains("b"), "a failed registration changes nothing");
        r.register(
            "app",
            vec![BundleEntryInput::new("index.html", b"new".to_vec(), None)],
        )
        .unwrap();
        assert!(r.get("app").unwrap().generation > g1);
        assert_eq!(
            &*r.lookup(&u("ui4me-res://app/index.html")).unwrap().data,
            b"new"
        );
        assert_eq!(
            r.lookup(&u("ui4me-res://app/x.bin")).unwrap_err().code,
            Code::FileNotFound
        );
        assert!(r.unregister("app") && !r.unregister("app"));
        assert!(r.names().is_empty());
    }

    #[test]
    fn entry_count_limit() {
        let r = BundleRegistry::new();
        let many: Vec<BundleEntryInput> = (0..MAX_ENTRIES + 1)
            .map(|i| BundleEntryInput::new(format!("f{i}"), Vec::new(), None))
            .collect();
        assert_eq!(r.register("big", many).unwrap_err().code, Code::SizeLimit);
    }
}
