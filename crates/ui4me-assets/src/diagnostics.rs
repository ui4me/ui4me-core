//! owner: 007-resources (T015) — resource diagnostics (data-model `ResourceDiagnosticRecord`).
//!
//! Records carry only canonical codes (the outcome is a `Code`, never free text), file paths are
//! redacted on export (AD-35) and retention is bounded (005's request-log bound, IF-05).

use crate::cache::CacheStats;
use crate::uri::ResourceKind;
use std::collections::{HashSet, VecDeque};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::resource_ids::RequestId;
use url::Url;

pub const DEFAULT_LOG_BOUND: usize = 4096;

/// Terminal and intermediate request states, as recorded in transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateName {
    Resolving,
    Pending,
    Ready,
    Broken,
    Canceled,
    Discarded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequesterKind {
    ImgElement,
    CssImage,
    FontFace,
    Stylesheet,
    SvgNested,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResourceDiagnosticRecord {
    pub request: RequestId,
    pub revision: u32,
    pub kind: ResourceKind,
    pub requester: RequesterKind,
    /// Redacted on export: `file:` URLs keep only the file name.
    pub url: String,
    /// `(state, monotonic ns since the manager started)`.
    pub transitions: Vec<(StateName, u64)>,
    /// The canonical outcome (`Code::Ok` on success).
    pub outcome: Code,
    pub encoded_bytes: u64,
    pub decoded_bytes: u64,
    pub decode_ms: u64,
    pub natural: Option<(u32, u32)>,
    pub flags: Vec<&'static str>,
    /// Non-fatal reports with their canonical code (e.g. `SIZE_LIMIT` for the 65,535-px downscale).
    pub warnings: Vec<Code>,
}

/// An `UNSUPPORTED_FEATURE` report under 005's R-20 key (e.g. `img.animation`, `svg.text`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedReport {
    pub key: &'static str,
    pub subject: ReportSubject,
}

/// What an unsupported report is about: a request, an element, or a CSS rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReportSubject {
    Request(RequestId),
    Element(NodeId),
    Rule(u64),
}

/// Document counters (`{pending, ready, broken, denied, stale_discarded, canceled}`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocumentCounters {
    pub pending: u32,
    pub ready: u32,
    pub broken: u32,
    pub denied: u32,
    pub stale_discarded: u64,
    pub canceled: u64,
}

#[derive(Debug)]
pub struct DiagnosticLog {
    bound: usize,
    records: VecDeque<ResourceDiagnosticRecord>,
    unsupported: Vec<UnsupportedReport>,
    unsupported_seen: HashSet<(&'static str, ReportSubject)>,
    /// Records dropped by the retention bound.
    pub dropped: u64,
}

impl DiagnosticLog {
    pub fn new(bound: usize) -> Self {
        DiagnosticLog {
            bound: bound.max(1),
            records: VecDeque::new(),
            unsupported: Vec::new(),
            unsupported_seen: HashSet::new(),
            dropped: 0,
        }
    }

    pub fn push(&mut self, r: ResourceDiagnosticRecord) {
        if self.records.len() == self.bound {
            self.records.pop_front();
            self.dropped += 1;
        }
        self.records.push_back(r);
    }

    /// Report an unsupported feature once per (key, subject).
    pub fn report_unsupported(&mut self, key: &'static str, subject: ReportSubject) {
        if self.unsupported_seen.insert((key, subject)) {
            self.unsupported.push(UnsupportedReport { key, subject });
        }
    }

    pub fn records(&self) -> impl Iterator<Item = &ResourceDiagnosticRecord> {
        self.records.iter()
    }

    pub fn unsupported(&self) -> &[UnsupportedReport] {
        &self.unsupported
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Redact a URL for export: `file:` URLs keep only the final file name; `data:` URLs keep only the
/// media-type prefix (never payload); other schemes are shown as is.
pub fn redact_url(url: &Url) -> String {
    match url.scheme() {
        "file" => {
            let name = url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .unwrap_or("");
            format!("file://<redacted>/{name}")
        }
        "data" => {
            let head = url.path().split(',').next().unwrap_or("");
            format!("data:{head},<{} bytes>", url.path().len())
        }
        _ => url.to_string(),
    }
}

/// Runtime-level diagnostics: modes and cache statistics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeResourceDiagnostics {
    pub deterministic_resources: bool,
    pub worker_threads: usize,
    pub cache: CacheStats,
}
