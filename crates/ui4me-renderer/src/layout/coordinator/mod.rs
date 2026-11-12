//! owner: 008-layout-engine — created by 001 slice (R-28)
//!
//! LayoutCoordinator (U4C-LAY): box generation → Taffy host tree (block layout, AD-12 calc,
//! 1/64 input quantisation) with one IFC container per inline root (AD-13) → immutable
//! LayoutUnit fragments (U4C-FRG). Conservative full rebuild per update (plan "Component
//! slice approach"); results depend only on the inputs (incremental equals cold).
//!
//! The layout **entry** (T016) is [`LayoutEntry`] / [`try_layout_document`]: viewport
//! validation (FR-115), the epoch lifecycle of data-model §4.1, failure isolation (a failure
//! keeps the previously published epoch, 005 outcome O4) and the published (empty)
//! `ContainerFacts` (FR-116).

pub(crate) mod atomic;
pub mod calc;
pub(crate) mod host_style;
pub mod host_tree;
pub mod quantize;
pub(crate) mod replay;

use crate::text::fonts::TextServices;
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{ErrorRecord, ReportKey, UnsupportedReason};
use ui4me_engine_types::revision::RevisionTuple;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

use crate::layout::box_tree::incremental::BoxDamage;
use crate::layout::box_tree::{BoxTree, StyledSource, build_box_tree, build_box_tree_reusing};
use crate::layout::fragment::FragmentTree;

/// One `UNSUPPORTED_FEATURE` report under 005's key (R-20), de-duplicated within a pass. 005's
/// store records it with the pass's DOM revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutReport {
    pub key: ReportKey,
    pub code: Code,
    pub reason: UnsupportedReason,
}

/// The initial containing block (CSS 2.1 §10.1): the viewport, CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

impl Viewport {
    /// The headless document viewport when the host sets none (FR-028; 004 FR-005, R-17):
    /// 800 × 600 CSS px at scale 1.0.
    pub const HEADLESS: Viewport = Viewport {
        width: 800.0,
        height: 600.0,
        scale: 1.0,
    };

    /// FR-115: a non-finite dimension is `GEOMETRY_NON_FINITE`, a negative one `INVALID_ARGUMENT`;
    /// nothing is published or changed by a rejected viewport.
    pub fn validate(self) -> Result<Viewport, Status> {
        let reject = |code: Code, what: &str| {
            Err(Status::new(code, Stage::Layout).with_message(format!(
                "viewport {}x{} at scale {}: {what}",
                self.width, self.height, self.scale
            )))
        };
        if !(self.width.is_finite() && self.height.is_finite() && self.scale.is_finite()) {
            return reject(Code::GeometryNonFinite, "non-finite value");
        }
        if self.width < 0.0 || self.height < 0.0 {
            return reject(Code::InvalidArgument, "negative dimension");
        }
        if self.scale <= 0.0 {
            return reject(Code::InvalidArgument, "scale must be positive");
        }
        Ok(self)
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport::HEADLESS
    }
}

/// `ContainerFacts` (AD-10; 015 CQ-3): one entry per element whose computed `container-type` is `size`
/// or `inline-size` and whose principal box is one size containment applies to (a block container, an
/// atomic inline, a replaced box; not a non-atomic inline box, a table or an internal table box): the
/// content-box size of that box in CSS px, in box-tree order. The pipeline hands them to the style
/// stage's `ContainerSizes` table (the container-query loop).
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerFact {
    pub node: ui4me_engine_types::ids::NodeId,
    pub width: f32,
    pub height: f32,
}

/// The container facts of one pass (see [`ContainerFact`]).
pub fn container_facts_of(boxes: &BoxTree, fragments: &FragmentTree) -> Vec<ContainerFact> {
    use crate::layout::box_tree::{BoxOwner, ContainerKind};
    use crate::layout::fragment::FragmentKind;
    let mut out = Vec::new();
    for (i, b) in boxes.boxes.iter().enumerate() {
        let BoxOwner::Element(node) = b.owner else {
            continue;
        };
        if b.style.containment.container == ContainerKind::Normal
            || b.effective_containment().container == ContainerKind::Normal
        {
            continue;
        }
        let id = crate::layout::box_tree::BoxId(i as u32);
        let found =
            fragments
                .fragments_of(node)
                .iter()
                .find_map(|f| match &fragments.get(*f).kind {
                    FragmentKind::Box(bf) if bf.box_id == id => Some(bf.content_rect.size),
                    _ => None,
                });
        if let Some(size) = found {
            out.push(ContainerFact {
                node,
                width: size.width.to_f64() as f32,
                height: size.height.to_f64() as f32,
            });
        }
    }
    out
}

/// Output of one layout pass.
#[derive(Debug)]
pub struct LayoutOutput {
    pub boxes: BoxTree,
    pub fragments: FragmentTree,
    /// Box-, text- and layout-stage reports, de-duplicated by key, in production order.
    pub reports: Vec<LayoutReport>,
    /// Stage diagnostics (`RESOURCE_PENDING` missing glyph from text; `GEOMETRY_NON_FINITE`).
    pub diagnostics: Vec<ErrorRecord>,
    /// Published `ContainerFacts` of this epoch (015 CQ-3).
    pub container_facts: Vec<ContainerFact>,
    /// The viewport this pass laid out in.
    pub viewport: Viewport,
    /// The host tree's results, for the next pass's formatting-context replay (`replay.rs`).
    pub(crate) host: Option<replay::HostSnapshot>,
}

impl LayoutOutput {
    /// How many formatting contexts this pass seeded from the previous pass (`replay.rs`).
    pub fn replayed_roots(&self) -> usize {
        self.host.as_ref().map_or(0, |h| h.seeded)
    }
}

/// Result of a pass that may reuse the previous one (L-PERF-b1).
#[derive(Debug)]
// A pass returns one owned output; boxing would add an allocation to every pass.
#[allow(clippy::large_enum_variant)]
pub enum PassOutcome {
    Fresh(LayoutOutput),
    /// The box tree equals the previous pass's and the viewport is the same: the previous output
    /// (boxes, fragments, reports, diagnostics) is still exact.
    Unchanged,
}

/// [`try_layout_document`] that skips host-tree layout and fragment building when the freshly built
/// box tree equals `prev`'s (callers pass `prev` only while the font environment is unchanged). With
/// `damage` (the DOM changes since `prev`'s box tree was built, `BoxTree::build_revision`) the box tree
/// takes the unchanged element subtrees over from `prev` (015 PERF-1); the result is the same.
pub fn try_layout_document_reusing(
    src: &impl StyledSource,
    viewport: Option<Viewport>,
    text: &mut TextServices,
    prev: Option<&LayoutOutput>,
    damage: Option<&BoxDamage>,
) -> Result<PassOutcome, Status> {
    let viewport = viewport.unwrap_or_default().validate()?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let t0 = std::time::Instant::now();
        let boxes = build_box_tree_reusing(src, prev.map(|p| &p.boxes).zip(damage));
        if std::env::var_os("UI4ME_CHECK_INCREMENTAL").is_some() && damage.is_some() {
            let cold = build_box_tree(src);
            if !(cold.same_layout_input(&boxes) && cold.node_to_boxes == boxes.node_to_boxes) {
                let dump = |t: &BoxTree| t.boxes.iter().map(|b| format!("{} {:?} {:?} {:?} p={:?} kids={:?}", b.id.0, b.owner, b.kind, b.tag, b.parent.map(|p| p.0), b.children.iter().map(|c| c.0).collect::<Vec<_>>())).collect::<Vec<_>>().join("\n");
                panic!("incremental box tree differs from a full build: {}\n--- cold\n{}\n--- incremental\n{}", cold.first_difference(&boxes), dump(&cold), dump(&boxes));
            }
        }
        let t1 = t0.elapsed();
        if let Some(p) = prev
            && p.viewport == viewport
            && p.boxes.same_layout_input(&boxes)
        {
            if std::env::var_os("UI4ME_PROFILE").is_some() {
                eprintln!("[prof] unchanged: box_build {:.2} compare {:.2}", t1.as_secs_f64() * 1e3, (t0.elapsed() - t1).as_secs_f64() * 1e3);
            }
            return PassOutcome::Unchanged;
        }
        if let Some(p) = prev
            && std::env::var_os("UI4ME_PROFILE_DIFF").is_some()
        {
            eprintln!("[diff] {}", p.boxes.first_difference(&boxes));
        }
        if std::env::var_os("UI4ME_PROFILE").is_some() {
            eprintln!("[prof] box_build {:.2} compare {:.2}", t1.as_secs_f64() * 1e3, (t0.elapsed() - t1).as_secs_f64() * 1e3);
        }
        // The previous pass's results seed the unchanged formatting contexts (same viewport; the caller
        // passes `prev` only while the font and resource epochs are unchanged).
        // A pass that clamped geometry (`GEOMETRY_NON_FINITE`) hands no fragments on: the flag is per pass.
        let replay = prev.filter(|p| p.viewport == viewport).and_then(|p| {
            let clean = !p.diagnostics.iter().any(|d| d.status.code == Code::GeometryNonFinite);
            p.host.as_ref().map(|h| (&p.boxes, h, clean.then_some(&p.fragments)))
        });
        PassOutcome::Fresh(finish_pass(boxes, viewport, text, &mut Vec::new(), replay))
    }))
    .map_err(|payload| {
        let what = payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".to_string());
        Status::new(Code::InternalInvariant, Stage::Layout).with_message(format!("layout pass failed: {what}"))
    })
}

/// Lay out a styled document in `viewport` (full rebuild; infallible form used by tests and the
/// oracle compare path). The pipeline uses [`try_layout_document`]; the epoch lifecycle is
/// [`LayoutEntry`].
pub fn layout_document(
    src: &impl StyledSource,
    viewport: Viewport,
    text: &mut TextServices,
) -> LayoutOutput {
    run_pass(src, viewport, text, &mut Vec::new())
}

fn run_pass(
    src: &impl StyledSource,
    viewport: Viewport,
    text: &mut TextServices,
    trace: &mut Vec<EpochState>,
) -> LayoutOutput {
    trace.push(EpochState::BoxBuild);
    let t0 = std::time::Instant::now();
    let boxes = build_box_tree(src);
    if std::env::var_os("UI4ME_PROFILE").is_some() {
        eprintln!(
            "[prof] box_build {:.2} ms",
            t0.elapsed().as_secs_f64() * 1e3
        );
    }
    finish_pass(boxes, viewport, text, trace, None)
}

fn finish_pass(
    boxes: BoxTree,
    viewport: Viewport,
    text: &mut TextServices,
    trace: &mut Vec<EpochState>,
    replay: Option<(&BoxTree, &replay::HostSnapshot, Option<&FragmentTree>)>,
) -> LayoutOutput {
    let t0 = std::time::Instant::now();
    let t1 = std::time::Duration::ZERO;
    let mut reports = boxes.reports.clone();
    let mut diagnostics = Vec::new();
    let (fragments, host) = host_tree::layout_boxes(
        &boxes,
        viewport,
        text,
        &mut reports,
        &mut diagnostics,
        trace,
        replay,
    );
    if std::env::var_os("UI4ME_PROFILE").is_some() {
        eprintln!(
            "[prof] box_build {:.2} ms, host_layout+fragments {:.2} ms",
            t1.as_secs_f64() * 1e3,
            (t0.elapsed() - t1).as_secs_f64() * 1e3
        );
    }
    diagnostics.extend(text.take_diagnostics());
    let container_facts = container_facts_of(&boxes, &fragments);
    LayoutOutput {
        boxes,
        fragments,
        reports,
        diagnostics,
        container_facts,
        viewport,
        host,
    }
}

/// Paged layout (015 PAGE-1, plan §A3 "Paged"; `crate::layout::fragmentation::paged`), beside the screen path
/// (`run_pass` is unchanged): the document's pages, with the page boxes `geometry` gives per page (the caller
/// cascades `@page`), at device scale `scale`. A failure inside is `INTERNAL_INVARIANT`, nothing else changes.
pub fn layout_paged(
    src: &impl StyledSource,
    geometry: &mut dyn crate::layout::fragmentation::paged::PageGeometry,
    scale: f32,
    text: &mut TextServices,
) -> Result<crate::layout::fragmentation::paged::PagedLayout, Status> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let boxes = build_box_tree(src);
        let mut reports = boxes.reports.clone();
        let mut diagnostics = Vec::new();
        let (pages, layouts) = host_tree::layout_boxes_paged(
            &boxes,
            geometry,
            scale,
            text,
            &mut reports,
            &mut diagnostics,
        );
        diagnostics.extend(text.take_diagnostics());
        crate::layout::fragmentation::paged::PagedLayout {
            boxes,
            pages,
            reports,
            diagnostics,
            layouts,
        }
    }))
    .map_err(|payload| {
        let what = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        Status::new(Code::InternalInvariant, Stage::Layout)
            .with_message(format!("paged layout failed: {what}"))
    })
}

/// States of the layout epoch lifecycle (data-model §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpochState {
    Idle,
    BoxBuild,
    Layout,
    FragmentBuild,
    Published,
}

/// The damage set the update step hands over (006 damage classes). The slice rebuilds
/// everything per pass (plan "Component slice approach"); scoped relayout is T032/T033, so a
/// non-full set is accepted and treated as full — results never depend on it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DamageSet {
    pub nodes: Vec<ui4me_engine_types::ids::NodeId>,
    pub full: bool,
}

impl DamageSet {
    pub fn full() -> Self {
        DamageSet {
            nodes: Vec::new(),
            full: true,
        }
    }
}

/// One call of the entry (005 update steps S2–S4).
pub struct LayoutRequest<'a, S: StyledSource> {
    /// The DOM view with its style snapshot (box generation input).
    pub source: &'a S,
    /// `None`: the headless default ([`Viewport::HEADLESS`]); a shown document passes its window.
    pub viewport: Option<Viewport>,
    /// 005's revision tuple for the tree this call publishes; its `layout` field is the epoch.
    pub revision: Option<RevisionTuple>,
    pub damage: DamageSet,
}

/// Validate and run one pass as a pure function: a rejected viewport or an internal failure
/// (a panic below this line becomes `INTERNAL_INVARIANT`) is returned as a `Status` and nothing
/// else changes, so the caller keeps its previous epoch (005 outcome O4). Does not stamp epochs.
pub fn try_layout_document(
    src: &impl StyledSource,
    viewport: Option<Viewport>,
    text: &mut TextServices,
) -> Result<LayoutOutput, Status> {
    try_pass(src, viewport, text, &mut Vec::new())
}

fn try_pass(
    src: &impl StyledSource,
    viewport: Option<Viewport>,
    text: &mut TextServices,
    trace: &mut Vec<EpochState>,
) -> Result<LayoutOutput, Status> {
    let viewport = viewport.unwrap_or_default().validate()?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_pass(src, viewport, text, trace)
    }))
    .map_err(|payload| {
        let what = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        Status::new(Code::InternalInvariant, Stage::Layout)
            .with_message(format!("layout pass failed: {what}"))
    })
}

/// The layout entry of one document: owns the epoch lifecycle (data-model §4.1). Owner thread
/// only (FR-114; debug-asserted). Never calls host code, dispatches events or mutates the DOM.
#[derive(Debug)]
pub struct LayoutEntry {
    owner: std::thread::ThreadId,
    state: EpochState,
    epoch: u64,
    published: Option<Arc<LayoutOutput>>,
    transitions: Vec<EpochState>,
}

impl Default for LayoutEntry {
    fn default() -> Self {
        LayoutEntry::new()
    }
}

impl LayoutEntry {
    pub fn new() -> Self {
        LayoutEntry {
            owner: std::thread::current().id(),
            state: EpochState::Idle,
            epoch: 0,
            published: None,
            transitions: Vec::new(),
        }
    }

    pub fn on_owner_thread(&self) -> bool {
        std::thread::current().id() == self.owner
    }

    pub fn state(&self) -> EpochState {
        self.state
    }

    /// The published layout epoch (0 before the first publication).
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn published(&self) -> Option<&Arc<LayoutOutput>> {
        self.published.as_ref()
    }

    /// `ContainerFacts` of the published epoch.
    pub fn container_facts(&self) -> &[ContainerFact] {
        self.published
            .as_ref()
            .map_or(&[], |p| p.container_facts.as_slice())
    }

    /// The states the last successful call went through, in order.
    pub fn last_transitions(&self) -> &[EpochState] {
        &self.transitions
    }

    /// Run S2–S4. On success the new tree becomes epoch `revision.layout` (or the previous
    /// epoch + 1) and the previous tree retires (it stays readable while any `Arc` is held). On
    /// error the previously published epoch stays published and unchanged.
    pub fn layout<S: StyledSource>(
        &mut self,
        req: LayoutRequest<'_, S>,
        text: &mut TextServices,
    ) -> Result<Arc<LayoutOutput>, Status> {
        debug_assert!(
            self.on_owner_thread(),
            "layout runs only on the owner thread (FR-114)"
        );
        let mut trace = Vec::new();
        let result = try_pass(req.source, req.viewport, text, &mut trace);
        let mut out = result?;
        trace.push(EpochState::Published);
        let epoch = req.revision.map_or(self.epoch + 1, |r| r.layout);
        out.fragments.stamp(epoch, req.revision);
        let out = Arc::new(out);
        self.epoch = epoch;
        self.state = EpochState::Published;
        self.published = Some(Arc::clone(&out));
        self.transitions = trace;
        Ok(out)
    }
}

pub mod budget;
pub mod float_cache;
pub mod floats;
pub mod intrinsic;
pub mod oof;
pub mod overflow;
pub mod sticky;
pub mod style_view;
