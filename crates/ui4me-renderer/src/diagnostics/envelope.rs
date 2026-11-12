//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Envelope, environment echo and the export entry (diagnostics-schema-v0.md §3; 005 plan:
//! `export(inputs: ProbeInputs, ProbeRequest, ReadMode, RedactionPolicy) -> Result<Vec<u8>>`).
//! `export` lives here because the crate root is fixed by 001 T020. The runtime (004, T053)
//! assembles [`ProbeInputs`]; this crate only serialises (it depends on `ui4me-core` only).
//! Envelope implemented by 001 T051, probes by T052.
//!
//! [`export`] serialises the T032 [`ProbeInputs`]; [`export_with`] additionally takes the
//! [`ExtraProbeInputs`] for probes whose inputs T032 did not declare (`scroll`, `dom.counts`,
//! `animation_clock`, `pixels_ref`, `cold_oracle`). A requested probe without an input is
//! exported as `{"unsupported": "UNSUPPORTED_PROBE", "reason": …}` (schema §1.5, §9).

use crate::diagnostics::canonical_json::to_canonical_bytes;
use crate::diagnostics::keys::NodeKeyIndex;
use crate::diagnostics::probes::cold_oracle::ColdOracleInput;
use crate::diagnostics::probes::computed_style::ResolvedStyleInput;
use crate::diagnostics::probes::display_list::DisplayListProbeInput;
use crate::diagnostics::probes::document::DocumentProbeInput;
use crate::diagnostics::probes::dom::DomNodeInput;
use crate::diagnostics::probes::dom_counts::DomCountsInput;
use crate::diagnostics::probes::element_geometry::ElementGeometryInput;
use crate::diagnostics::probes::element_geometry::ScrollProbeInput;
use crate::diagnostics::probes::errors::ErrorsProbeInput;
use crate::diagnostics::probes::fragments::FragmentInput;
use crate::diagnostics::probes::hit_test::HitTestProbeInput;
use crate::diagnostics::probes::pixels_ref::PixelsRefInput;
use crate::diagnostics::probes::render_receipts::ReceiptInput;
use crate::diagnostics::probes::runtime::AnimationClockInput;
use crate::diagnostics::probes::runtime::RuntimeProbeInput;
use crate::diagnostics::probes::unsupported::UnsupportedProbeInput;
use crate::diagnostics::redact::RedactionPolicy;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{DispatchRecord, Ring};
use ui4me_engine_types::geometry::LayoutRect;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::revision::RevisionTuple;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

pub const SCHEMA: &str = "ui4me-diagnostics";
pub const SCHEMA_VERSION: &str = "0.1.0";

/// The probes the slice exports (001 FR-056), by schema name.
pub const SLICE_PROBES: &[&str] = &[
    "runtime",
    "document",
    "dom",
    "computed_style",
    "element_geometry",
    "fragments",
    "display_list",
    "hit_test",
    "event_log",
    "unsupported",
    "errors",
    "render_receipts",
];

/// The slice probes plus the probes the 002 headless harness reads that the slice can produce
/// (`scroll`: root scroller only; `dom.counts`; `animation_clock`; `pixels_ref` when a headless
/// render was requested). The runtime's default request (T053).
pub const HARNESS_PROBES: &[&str] = &[
    "runtime",
    "document",
    "dom",
    "dom.counts",
    "computed_style",
    "element_geometry",
    "scroll",
    "fragments",
    "display_list",
    "hit_test",
    "event_log",
    "animation_clock",
    "unsupported",
    "errors",
    "render_receipts",
    "pixels_ref",
];

/// Default export size limit (recompute-plan §10).
pub const DEFAULT_SIZE_LIMIT: usize = 256 * 1024 * 1024;

pub use crate::pipeline::domain::{ClockMode, ReadMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Document,
    Runtime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

/// 004 runtime-configuration name `resource_mode` (R-37).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceMode {
    Normal,
    Deterministic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProducerEcho {
    pub version: String,
    /// `major.minor`.
    pub abi_version: String,
    pub lock_sha256: Option<String>,
}

/// 006's profile identifier (`ui4me-css/1`), never an alias.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileEcho {
    pub id: String,
    pub version: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontEcho {
    pub family: String,
    /// Lowercase hex, 64 digits.
    pub sha256: String,
    /// Subject to redaction.
    pub source: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentEcho {
    /// CSS px.
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub scale: f64,
    pub color_scheme: ColorScheme,
    pub reduced_motion: bool,
    pub locale: String,
    pub profile: ProfileEcho,
    pub clock_mode: ClockMode,
    pub clock_time_ms: f64,
    pub pinned_fonts_only: bool,
    pub fonts: Vec<FontEcho>,
    pub resource_mode: ResourceMode,
    /// 011 platform identifier.
    pub platform: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnvelopeNondeterministic {
    pub export_ms: f64,
    pub host_language: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeInput {
    pub producer: ProducerEcho,
    pub environment: EnvironmentEcho,
    /// The document's ordinal in the runtime, from 1 (`revision.document`).
    pub document_ordinal: u32,
    pub revision: RevisionTuple,
    pub current: bool,
    pub nondeterministic: EnvelopeNondeterministic,
}

/// Plain-data inputs of one export, assembled by the runtime (T053) from the committed set,
/// the runtime geometry query and the diagnostics stores. `None` = the probe cannot be
/// produced (exported as `UNSUPPORTED_PROBE`).
#[derive(Clone, Debug)]
pub struct ProbeInputs {
    pub envelope: EnvelopeInput,
    pub runtime: Option<RuntimeProbeInput>,
    pub document: Option<DocumentProbeInput>,
    /// The Document node; also the source of every `NodeKey`.
    pub dom: DomNodeInput,
    pub computed_style: Option<Vec<ResolvedStyleInput>>,
    pub element_geometry: Option<Vec<ElementGeometryInput>>,
    pub fragments: Option<FragmentInput>,
    pub display_list: Option<DisplayListProbeInput>,
    pub hit_test: Option<HitTestProbeInput>,
    pub event_log: Option<Ring<DispatchRecord>>,
    pub unsupported: Option<UnsupportedProbeInput>,
    pub errors: Option<ErrorsProbeInput>,
    pub render_receipts: Option<Vec<ReceiptInput>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeRequest {
    pub scope: Scope,
    /// Probe names, for example [`SLICE_PROBES`].
    pub probes: Vec<&'static str>,
}

/// Inputs of probes that T032's [`ProbeInputs`] does not carry (additive, T052).
#[derive(Clone, Debug, Default)]
pub struct ExtraProbeInputs {
    pub scroll: Option<ScrollProbeInput>,
    pub dom_counts: Option<DomCountsInput>,
    pub animation_clock: Option<AnimationClockInput>,
    pub pixels_ref: Option<PixelsRefInput>,
    pub cold_oracle: Option<ColdOracleInput>,
    pub invalidation: Option<crate::diagnostics::probes::invalidation::InvalidationProbeInput>,
    pub performance: Option<ui4me_engine_types::diag::PerformanceStore>,
    /// `None` = [`DEFAULT_SIZE_LIMIT`].
    pub size_limit: Option<usize>,
    /// `data-probe` of nodes outside the tree, so their detached `NodeKey` keeps the probe (002 D4).
    pub detached_probes: std::collections::HashMap<NodeId, String>,
    /// Detached subtree roots (FR-076): nodes outside the tree are keyed in the detached form
    /// relative to their subtree root. Empty = a detached node is its own root.
    pub detached_subtrees: Vec<DomNodeInput>,
}

/// Serialise one export as canonical (RFC 8785) UTF-8 JSON. Read-only (U4C-DIA).
/// Errors: `SIZE_LIMIT` (no partial output); derived-read outcomes are decided by the caller.
pub fn export(
    inputs: &ProbeInputs,
    request: &ProbeRequest,
    read_mode: ReadMode,
    redaction: &RedactionPolicy,
) -> Result<Vec<u8>, Status> {
    export_with(
        inputs,
        &ExtraProbeInputs::default(),
        request,
        read_mode,
        redaction,
    )
}

/// [`export`] with the [`ExtraProbeInputs`].
pub fn export_with(
    inputs: &ProbeInputs,
    extra: &ExtraProbeInputs,
    request: &ProbeRequest,
    read_mode: ReadMode,
    redaction: &RedactionPolicy,
) -> Result<Vec<u8>, Status> {
    let value = export_value(inputs, extra, request, read_mode, redaction);
    let bytes = to_canonical_bytes(&value);
    let limit = extra.size_limit.unwrap_or(DEFAULT_SIZE_LIMIT);
    if bytes.len() > limit {
        return Err(
            Status::new(Code::SizeLimit, Stage::Host).with_message(format!(
                "diagnostics export of {} bytes exceeds the limit of {limit} bytes",
                bytes.len()
            )),
        );
    }
    Ok(bytes)
}

/// The export as a JSON value (before canonical serialisation).
pub fn export_value(
    inputs: &ProbeInputs,
    extra: &ExtraProbeInputs,
    request: &ProbeRequest,
    read_mode: ReadMode,
    redaction: &RedactionPolicy,
) -> Value {
    let env = &inputs.envelope;
    let cx = ExportCx {
        keys: {
            let base = NodeKeyIndex::build(&inputs.dom)
                .with_document_ordinal(env.document_ordinal)
                .with_detached_probes(extra.detached_probes.clone());
            // A duplicate root identity cannot occur from a live arena; if a caller supplies one the
            // subtrees are left unregistered and every detached node keys as its own root.
            match base
                .clone()
                .with_detached_subtrees(extra.detached_subtrees.clone())
            {
                Ok(k) => k,
                Err(_) => base,
            }
        },
        redaction,
        document: env.revision.document,
        ordinal: env.document_ordinal,
        scope: request.scope,
        password_targets: crate::diagnostics::probes::dom::password_controls(&inputs.dom),
    };

    // `scroll` without a runtime-supplied input is derived from the committed fragments
    // (`probes/scroll.rs`).
    let derived;
    let extra = if extra.scroll.is_none() && request.probes.contains(&"scroll") {
        derived = ExtraProbeInputs {
            scroll: crate::diagnostics::probes::scroll::derive(inputs),
            ..extra.clone()
        };
        &derived
    } else {
        extra
    };
    let mut probes = Map::new();
    for name in &request.probes {
        let v = crate::diagnostics::probes::produce(name, inputs, extra, &cx)
            .unwrap_or_else(|reason| unsupported_probe(&reason));
        probes.insert((*name).to_string(), v);
    }

    let mut root = Map::new();
    root.insert("schema".into(), json!(SCHEMA));
    root.insert("schema_version".into(), json!(SCHEMA_VERSION));
    let mut producer = Map::new();
    producer.insert("name".into(), json!("ui4me"));
    producer.insert("version".into(), json!(env.producer.version));
    producer.insert("abi_version".into(), json!(env.producer.abi_version));
    if let Some(l) = &env.producer.lock_sha256 {
        producer.insert("lock_sha256".into(), json!(l));
    }
    root.insert("producer".into(), Value::Object(producer));
    root.insert("redaction".into(), json!(redaction.mode.name()));
    if redaction.password_values_included() {
        root.insert("password_values".into(), json!("included"));
    }
    root.insert("scope".into(), json!(request.scope.name()));
    root.insert("read_mode".into(), json!(read_mode.name()));
    root.insert("coordinate_units".into(), json!("css-px"));
    root.insert(
        "environment".into(),
        environment_json(&env.environment, redaction),
    );
    let mut revision = cx.revision_json(&env.revision);
    revision.insert("current".into(), json!(env.current));
    root.insert("revision".into(), Value::Object(revision));
    root.insert("probes".into(), Value::Object(probes));
    let mut nd = Map::new();
    nd.insert("export_ms".into(), num(env.nondeterministic.export_ms));
    if let Some(h) = &env.nondeterministic.host_language {
        nd.insert("host_language".into(), json!(h));
    }
    root.insert("nondeterministic".into(), Value::Object(nd));
    Value::Object(root)
}

fn environment_json(e: &EnvironmentEcho, redaction: &RedactionPolicy) -> Value {
    let fonts: Vec<Value> = e
        .fonts
        .iter()
        .map(|f| {
            let mut m = Map::new();
            m.insert("family".into(), json!(f.family));
            m.insert("sha256".into(), json!(f.sha256));
            if let Some(s) = &f.source {
                m.insert("source".into(), json!(redaction.path(s)));
            }
            Value::Object(m)
        })
        .collect();
    let mut m = Map::new();
    m.insert(
        "viewport".into(),
        json!({"width": num(e.viewport_width), "height": num(e.viewport_height)}),
    );
    m.insert("scale".into(), num(e.scale));
    m.insert("color_scheme".into(), json!(e.color_scheme.name()));
    m.insert("reduced_motion".into(), json!(e.reduced_motion));
    m.insert("locale".into(), json!(e.locale));
    m.insert(
        "profile".into(),
        json!({"id": e.profile.id, "version": e.profile.version, "digest": e.profile.digest}),
    );
    // A realtime clock reads wall time: its `time_ms` is nondeterministic (schema v0 convention; ERRATA E-005-R1).
    // A manual (virtual) clock is deterministic and keeps `time_ms` in the compared document.
    let clock = match e.clock_mode {
        ClockMode::Realtime => {
            json!({"mode": e.clock_mode.name(), "nondeterministic": {"time_ms": num(e.clock_time_ms)}})
        }
        ClockMode::Manual => json!({"mode": e.clock_mode.name(), "time_ms": num(e.clock_time_ms)}),
    };
    m.insert("clock".into(), clock);
    m.insert("pinned_fonts_only".into(), json!(e.pinned_fonts_only));
    m.insert("fonts".into(), Value::Array(fonts));
    m.insert("resource_mode".into(), json!(e.resource_mode.name()));
    if let Some(p) = &e.platform {
        m.insert("platform".into(), json!(p));
    }
    Value::Object(m)
}

fn unsupported_probe(reason: &str) -> Value {
    json!({"unsupported": "UNSUPPORTED_PROBE", "reason": reason})
}

impl ReadMode {
    pub const fn name(self) -> &'static str {
        match self {
            ReadMode::Flush => "flush",
            ReadMode::Committed => "committed",
        }
    }
}

impl Scope {
    pub const fn name(self) -> &'static str {
        match self {
            Scope::Document => "document",
            Scope::Runtime => "runtime",
        }
    }
}

impl ColorScheme {
    pub const fn name(self) -> &'static str {
        match self {
            ColorScheme::Light => "light",
            ColorScheme::Dark => "dark",
        }
    }
}

impl ClockMode {
    pub const fn name(self) -> &'static str {
        match self {
            ClockMode::Realtime => "realtime",
            ClockMode::Manual => "manual",
        }
    }
}

impl ResourceMode {
    pub const fn name(self) -> &'static str {
        match self {
            ResourceMode::Normal => "normal",
            ResourceMode::Deterministic => "deterministic",
        }
    }
}

// ------------------------------------------------------------------ shared serialisation context

/// Per-export context shared by the probe serialisers.
pub(crate) struct ExportCx<'a> {
    pub keys: NodeKeyIndex,
    pub redaction: &'a RedactionPolicy,
    /// The exported document.
    pub document: DocumentId,
    pub ordinal: u32,
    pub scope: Scope,
    /// `input type=password` elements (event-data redaction, §8).
    pub password_targets: Vec<NodeId>,
}

impl ExportCx<'_> {
    pub fn key(&self, node: NodeId) -> Value {
        self.keys.key_json(node)
    }

    /// The document ordinal of `doc`. Only the exported document has an ordinal in the inputs;
    /// any other document (runtime scope) is numbered after it by slot index — stable within a
    /// runtime, which is all `scope: "runtime"` promises in the slice.
    pub fn ordinal_of(&self, doc: DocumentId) -> u32 {
        if doc == self.document {
            self.ordinal
        } else {
            self.ordinal
                .saturating_add(1)
                .saturating_add(doc.raw().index)
        }
    }

    /// Whether a runtime-scoped record of `doc` belongs in this export.
    pub fn includes(&self, doc: DocumentId) -> bool {
        self.scope == Scope::Runtime || doc == self.document
    }

    pub fn revision_json(&self, r: &RevisionTuple) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("document".into(), json!(self.ordinal_of(r.document)));
        m.insert("dom".into(), json!(r.inputs.dom));
        m.insert("environment".into(), json!(r.inputs.environment));
        m.insert("resource".into(), json!(r.inputs.resource));
        m.insert("font".into(), json!(r.inputs.font));
        m.insert("scroll".into(), json!(r.inputs.scroll));
        m.insert("animation".into(), json!(r.inputs.animation));
        m.insert("style".into(), json!(r.style));
        m.insert("layout".into(), json!(r.layout));
        m.insert("scene".into(), json!(r.scene));
        m
    }
}

/// A finite JSON number (non-finite values never appear in an export, schema §4; they map to 0).
pub(crate) fn num(v: f64) -> Value {
    if v.is_finite() {
        // Normalise -0 and exact integers to integer form so equal values compare equal.
        if v == v.trunc() && v.abs() < 9.0e15 {
            json!(v as i64)
        } else {
            json!(v)
        }
    } else {
        json!(0)
    }
}

/// An f32 through its shortest decimal form (so `0.1f32` exports as `0.1`, not `0.100000001…`).
pub(crate) fn num32(v: f32) -> Value {
    num(format!("{v}").parse::<f64>().unwrap_or(0.0))
}

pub(crate) fn rect_json(r: &LayoutRect) -> Value {
    json!({"x": num(r.origin.x.to_f64()), "y": num(r.origin.y.to_f64()),
           "width": num(r.size.width.to_f64()), "height": num(r.size.height.to_f64())})
}

pub(crate) fn point_json(x: f64, y: f64) -> Value {
    json!({"x": num(x), "y": num(y)})
}

/// Exported stage name; an unattributed record (`Stage::None`) is attributed to `host`, the
/// boundary every unattributed failure crosses (the schema's stage enum has no `none`).
pub(crate) fn stage_name(s: Stage) -> &'static str {
    if s == Stage::None {
        Stage::Host.name()
    } else {
        s.name()
    }
}
