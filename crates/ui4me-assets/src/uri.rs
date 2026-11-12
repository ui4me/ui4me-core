//! owner: 007-resources (T010) — URL parsing, document and base URLs, scheme dispatch,
//! `file:` URL to path mapping (`contracts/uri-resolution.md`).
//!
//! Evaluated synchronously on the owner thread when a request is made. The WHATWG URL Standard
//! parser is the `url` crate (shared with Stylo).

use std::path::{Path, PathBuf};
use ui4me_engine_types::codes_generated::{Code, SubReason};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use url::Url;

/// What a request is for; selects the encoded-size limit and the sniffing path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Stylesheet,
    Image,
    Svg,
    Font,
}

/// Scheme dispatch of `policy::check`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    File,
    Data,
    UiRes,
    Other,
}

impl Scheme {
    pub fn of(url: &Url) -> Scheme {
        match url.scheme() {
            "file" => Scheme::File,
            "data" => Scheme::Data,
            "ui4me-res" => Scheme::UiRes,
            _ => Scheme::Other,
        }
    }
}

pub fn about_blank() -> Url {
    Url::parse("about:blank").expect("about:blank parses")
}

/// Document URL of a document loaded from a file path: the `file:` URL of the canonical path.
pub fn document_url_from_file(path: &Path) -> Result<Url, Status> {
    let canon = std::fs::canonicalize(path).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            Code::FileNotFound
        } else {
            Code::ResourceDenied
        };
        Status::new(code, Stage::Resource)
            .with_message(format!("document file {}: {e}", path.display()))
    })?;
    Url::from_file_path(crate::policy::strip_verbatim(canon)).map_err(|()| {
        Status::new(Code::UriMalformed, Stage::Resource)
            .with_message("document path is not absolute")
    })
}

/// Document URL of a string document: the host-supplied `base_url`, or `about:blank`.
pub fn document_url_from_string(base_url: Option<&str>) -> Result<Url, Status> {
    match base_url {
        None => Ok(about_blank()),
        Some(b) => Url::parse(b).map_err(|e| malformed(format!("base_url {b:?}: {e}"))),
    }
}

/// The document base URL: the frozen base URL of the first `<base href>` in tree order (HTML "set
/// the frozen base URL"), else the document URL. An unparsable `href` leaves the document URL.
pub fn document_base_url(document_url: &Url, first_base_href: Option<&str>) -> Url {
    match first_base_href {
        Some(h) => document_url
            .join(h.trim())
            .ok()
            .filter(|u| !u.cannot_be_a_base())
            .unwrap_or_else(|| document_url.clone()),
        None => document_url.clone(),
    }
}

fn malformed(msg: impl Into<String>) -> Status {
    Status::new(Code::UriMalformed, Stage::Resource).with_message(msg)
}

/// Resolve `reference` against `base` (`uri-resolution.md` §3 step 1). A relative reference with an
/// `about:blank` base is `URI_MALFORMED` with sub-reason `NO_BASE_URL`.
pub fn resolve(base: &Url, reference: &str) -> Result<Url, Status> {
    let reference =
        reference.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'));
    match base.join(reference) {
        Ok(u) => Ok(u),
        Err(e) => {
            let relative = Url::parse(reference).is_err();
            if relative && base.cannot_be_a_base() {
                Err(
                    malformed(format!("relative reference {reference:?} with base {base}"))
                        .with_sub_reason(SubReason::NoBaseUrl),
                )
            } else {
                Err(malformed(format!("{reference:?}: {e}")))
            }
        }
    }
}

/// A URL without its fragment: fragments are ignored for loading and kept for diagnostics only.
pub fn without_fragment(url: &Url) -> Url {
    let mut u = url.clone();
    u.set_fragment(None);
    u
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// A decoded path segment may not smuggle in a separator, a dot segment or a NUL.
fn segment_ok(seg: &[u8], backslash_is_separator: bool) -> bool {
    !(seg.contains(&0)
        || seg.contains(&b'/')
        || (backslash_is_separator && seg.contains(&b'\\'))
        || seg == b".."
        || seg == b".")
}

/// Map a `file:` URL to a Unix path: percent-decoded; no host other than `localhost`/empty.
pub fn unix_path(url: &Url) -> Result<PathBuf, Code> {
    if url.scheme() != "file" {
        return Err(Code::ResourceDenied);
    }
    match url.host_str() {
        None | Some("") | Some("localhost") => {}
        Some(_) => return Err(Code::ResourceDenied),
    }
    let mut out = String::from("/");
    let mut first = true;
    let raw = url.path();
    for seg in raw.trim_start_matches('/').split('/') {
        let dec = percent_decode(seg);
        if !first {
            out.push('/');
        }
        first = false;
        if dec.is_empty() {
            continue;
        }
        if !segment_ok(&dec, false) {
            return Err(Code::ResourceDenied);
        }
        out.push_str(&String::from_utf8(dec).map_err(|_| Code::ResourceDenied)?);
    }
    Ok(PathBuf::from(out))
}

/// A Windows path derived from a `file:` URL (`file:///C:/…` or `file://server/share/…`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsPath {
    pub path: String,
    pub unc: bool,
}

/// Map a `file:` URL to a Windows path (pure; testable on every OS). Device, verbatim, ADS and
/// reserved-name paths are rejected lexically (`RESOURCE_DENIED`, `resource-policy.md` §3).
pub fn windows_path(url: &Url) -> Result<WindowsPath, Code> {
    if url.scheme() != "file" {
        return Err(Code::ResourceDenied);
    }
    let segs: Vec<Vec<u8>> = url
        .path()
        .trim_start_matches('/')
        .split('/')
        .map(percent_decode)
        .collect();
    for s in &segs {
        if !segment_ok(s, true) {
            return Err(Code::ResourceDenied);
        }
    }
    let segs: Vec<String> = segs
        .into_iter()
        .map(|s| String::from_utf8(s).map_err(|_| Code::ResourceDenied))
        .collect::<Result<_, _>>()?;
    let (path, unc) = match url.host_str() {
        None | Some("") | Some("localhost") => {
            let drive = segs.first().ok_or(Code::ResourceDenied)?;
            let d = drive.as_bytes();
            if !(d.len() == 2 && d[0].is_ascii_alphabetic() && (d[1] == b':' || d[1] == b'|')) {
                return Err(Code::ResourceDenied);
            }
            let rest: Vec<&str> = segs[1..]
                .iter()
                .filter(|s| !s.is_empty())
                .map(String::as_str)
                .collect();
            (
                format!(
                    "{}:\\{}",
                    (d[0] as char).to_ascii_uppercase(),
                    rest.join("\\")
                ),
                false,
            )
        }
        Some(host) => {
            let rest: Vec<&str> = segs
                .iter()
                .filter(|s| !s.is_empty())
                .map(String::as_str)
                .collect();
            (format!("\\\\{host}\\{}", rest.join("\\")), true)
        }
    };
    crate::fs_guard::windows::lexical_check(&path, unc)?;
    Ok(WindowsPath { path, unc })
}

/// OS-specific mapping with lexical rejection: the `file:` branch of `policy::check`.
pub fn file_url_to_path_checked(url: &Url) -> Result<PathBuf, Code> {
    if cfg!(windows) {
        windows_path(url).map(|w| PathBuf::from(w.path))
    } else {
        unix_path(url)
    }
}
