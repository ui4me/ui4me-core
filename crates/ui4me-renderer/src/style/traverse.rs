//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! StyleBridge and the style pass driver (T042; AD-07; FR-015, FR-031). One [`StyleBridge`] per
//! document. [`StyleBridge::restyle`] runs one sequential pass (`traverse_dom` with
//! `pool = None`) over the structure-frozen Document view and the journal segment that 005's
//! scheduler took (`take_epoch`), and publishes an immutable [`StyleSnapshot`].
//!
//! Invalidation (incremental restyle must equal a cold restyle, FR-068):
//! * attribute, class, id and state changes: journal snapshots (snapshot.rs) → Stylo's
//!   invalidator;
//! * `style=""` changes: `RESTYLE_STYLE_ATTRIBUTE` on the element;
//! * child-list changes: the parent restyles its subtree (covers `:empty`, `:nth-*`, `+`/`~`
//!   and inheritance into new nodes); inserted subtrees drop any old style data first, so a
//!   re-inserted node is styled fresh;
//! * a stylesheet source change: every element's data is cleared (a full restyle).

use crate::style::adapter::document::quirks;
use crate::style::adapter::handle::{StyleNode, with_pass};
use crate::style::animation::AnimationEngine;
use crate::style::device::{StyleEnvironment, build_device};
use crate::style::diagnostics::{StyleDiagnosticRecord, StyleDiagnostics};
use crate::style::font_metrics::CssFallbackMetrics;
use crate::style::sheets::set::DocumentStylesheetSet;
use crate::style::sheets::{INLINE_SHEET_ID, UA_SHEET_ID, blank_url, parse_style_attribute};
use crate::style::sidecar::SidecarStore;
use crate::style::view::{NodeStyles, StyleSnapshot};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use style::Atom;
use style::context::{
    QuirksMode, RegisteredSpeculativePainter, RegisteredSpeculativePainters, SharedStyleContext,
    StyleContext,
};
use style::dom::{TElement, TNode};
use style::global_style_data::GLOBAL_STYLE_DATA;
use style::invalidation::element::restyle_hints::RestyleHint;
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::selector_parser::{PseudoElement, SnapshotMap};
use style::servo_arc::Arc as StyloArc;
use style::shared_lock::{SharedRwLock, StylesheetGuards};
use style::stylesheets::UrlExtraData;
use style::stylist::Stylist;
use style::thread_state::{self, ThreadState};
use style::traversal::{DomTraversal, recalc_style_at};
use style::traversal_flags::TraversalFlags;
use ui4me_document::document::Document;
use ui4me_document::journal::EpochSegment;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{ReportKey, ReportLocation, UnsupportedReason, UnsupportedReports};
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::revision::RevisionInputs;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use ui4me_engine_types::text_metrics::FontMetricsSource;

struct NoPainters;
impl RegisteredSpeculativePainters for NoPainters {
    fn get(&self, _name: &Atom) -> Option<&dyn RegisteredSpeculativePainter> {
        None
    }
}

/// Sequential recalc traversal (as Blitz's `RecalcStyle`).
pub(crate) struct Recalc<'a> {
    context: SharedStyleContext<'a>,
    restyled: AtomicUsize,
}

impl<'dom> DomTraversal<StyleNode<'dom>> for Recalc<'_> {
    fn process_preorder<F: FnMut(StyleNode<'dom>)>(
        &self,
        context: &mut StyleContext<StyleNode<'dom>>,
        node: StyleNode<'dom>,
        note_child: F,
    ) {
        if let Some(el) = node.as_element() {
            // SAFETY: Stylo's traversal hands each element to process_preorder exactly once,
            // sequentially (pool = None), and holds no other borrow of its data here. "Exactly
            // once" relies on the arena being a tree (review F2): DocumentCore's `Arena::insert`
            // detaches the child first and rejects self-insertion and cycles, so no element is its
            // own ancestor or appears in two child lists. ElementData is reachable only from
            // crate-internal code (review F1), so no outside borrow can overlap.
            let mut data = unsafe { el.ensure_data() };
            if !data.has_styles() || !data.hint.is_empty() {
                self.restyled.fetch_add(1, Ordering::Relaxed);
            }
            recalc_style_at(self, context, el, &mut data, note_child);
            // SAFETY: same exclusivity as above; only this element's own flag is touched.
            unsafe { el.unset_dirty_descendants() };
        }
    }
    fn needs_postorder_traversal() -> bool {
        false
    }
    fn process_postorder(&self, _: &mut StyleContext<StyleNode<'dom>>, _: StyleNode<'dom>) {
        unreachable!("needs_postorder_traversal is false")
    }
    fn shared_context(&self) -> &SharedStyleContext<'_> {
        &self.context
    }
}

/// Leaves the engine's LAYOUT thread state even if the pass unwinds (review F8).
pub(crate) struct LayoutState;
impl LayoutState {
    pub(crate) fn enter() -> LayoutState {
        thread_state::enter(ThreadState::LAYOUT);
        LayoutState
    }
}
impl Drop for LayoutState {
    fn drop(&mut self) {
        thread_state::exit(ThreadState::LAYOUT);
    }
}

/// Whether any of an element's computed styles read font metrics (`ex`, `ch`, `cap`, `ic`, `lh`).
fn depends_on_font_metrics(styles: &style::data::ElementStyles) -> bool {
    use style::computed_value_flags::ComputedValueFlags as F;
    let mask = F::DEPENDS_ON_SELF_FONT_METRICS | F::DEPENDS_ON_INHERITED_FONT_METRICS;
    styles.primary().flags.intersects(mask)
        || [PseudoElement::Before, PseudoElement::After]
            .iter()
            .any(|p| {
                styles
                    .pseudos
                    .get(p)
                    .is_some_and(|s| s.flags.intersects(mask))
            })
}

/// Whether a stylesheet or `style` attribute text uses an `lh` or `rlh` unit (a number directly
/// followed by the unit and not by further identifier characters).
fn has_lh_unit(text: &str) -> bool {
    let b = text.as_bytes();
    for unit in ["rlh", "lh"] {
        let u = unit.as_bytes();
        let mut i = 0;
        while i + u.len() <= b.len() {
            if b[i..i + u.len()].eq_ignore_ascii_case(u)
                && i > 0
                && (b[i - 1].is_ascii_digit() || b[i - 1] == b'.')
            {
                let after = b.get(i + u.len()).copied();
                if after.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == b'-' || c == b'_')) {
                    return true;
                }
            }
            i += 1;
        }
    }
    false
}

/// What one restyle did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestyleReport {
    pub traversed: bool,
    /// Elements whose style was (re)computed in this pass.
    pub restyled_elements: usize,
    /// Connected elements in the document.
    pub total_elements: usize,
    pub snapshots: usize,
    pub sheets_changed: bool,
    /// The union of the restyle damage this pass recorded (Stylo `ServoRestyleDamage` bits:
    /// `1` repaint, `3` + stacking contexts, `7` + scrollable overflow, `15` relayout; a node
    /// styled for the first time is all bits). `0` when nothing changed or no pass ran.
    pub damage_bits: u16,
    /// The text `color` of a damaged element changed since the last published snapshot.
    pub color_changed: bool,
    /// The published snapshot's epoch.
    pub epoch: u64,
}

/// Per-document StyleBridge (U4C-CSS).
pub struct StyleBridge {
    document: DocumentId,
    pub(crate) stylist: Stylist,
    pub(crate) sidecars: SidecarStore,
    pub(crate) sheets: DocumentStylesheetSet,
    pub(crate) url: UrlExtraData,
    base_url: url::Url,
    diagnostics: StyleDiagnostics,
    reports: UnsupportedReports,
    snapshot: Arc<StyleSnapshot>,
    pub(crate) quirks: QuirksMode,
    pub(crate) initial: StyloArc<ComputedValues>,
    /// The input epochs of the last pass (environment and font changes force a full restyle).
    last_inputs: RevisionInputs,
    /// The previous pass failed after touching element data: the next pass restyles everything.
    poisoned: bool,
    fail_next: Option<Code>,
    /// Unlisted presentational attributes already reported for this document.
    reported_hints: std::collections::HashSet<&'static str>,
    /// The document's transitions and animations (006 T087; research R-11).
    pub(crate) anim: AnimationEngine,
    /// The first-line / first-letter variants of the last pass (`first_pseudo.rs`).
    first_cache: std::sync::Mutex<crate::style::first_pseudo::FirstCache>,
}

impl std::fmt::Debug for StyleBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleBridge")
            .field("document", &self.document)
            .field("epoch", &self.snapshot.epoch)
            .finish_non_exhaustive()
    }
}

impl StyleBridge {
    /// A bridge without a font-metrics source (CSS fallbacks for font-relative units).
    pub fn new(document: DocumentId, env: StyleEnvironment) -> Result<StyleBridge, Status> {
        StyleBridge::with_font_metrics(document, env, Box::new(CssFallbackMetrics))
    }

    /// A bridge whose `Device` uses 009's metrics service (injected at runtime start, AD-08).
    pub fn with_font_metrics(
        document: DocumentId,
        env: StyleEnvironment,
        metrics: Box<dyn FontMetricsSource>,
    ) -> Result<StyleBridge, Status> {
        crate::style::profile::ensure_applied();
        let device = build_device(&env, QuirksMode::NoQuirks, metrics);
        let lock = SharedRwLock::new();
        let url = blank_url();
        let base_url = url::Url::parse("about:blank").expect("static URL");
        let mut stylist = Stylist::new(device, QuirksMode::NoQuirks);
        let (sheets, ua_records) =
            DocumentStylesheetSet::new(&mut stylist, &lock, &url, base_url.clone());
        if let Some(r) = ua_records.first() {
            return Err(Status::new(Code::EngineMisconfigured, Stage::Cascade)
                .with_message(format!("UA sheet diagnostic: {r:?}")));
        }
        let initial = ComputedValues::initial_values_with_font_override(Font::initial_values());
        Ok(StyleBridge {
            document,
            stylist,
            sidecars: SidecarStore::new(lock),
            sheets,
            url,
            base_url,
            diagnostics: StyleDiagnostics::default(),
            reports: UnsupportedReports::default(),
            snapshot: StyleSnapshot::empty(document, initial.clone()),
            quirks: QuirksMode::NoQuirks,
            initial,
            last_inputs: RevisionInputs::default(),
            poisoned: false,
            fail_next: None,
            reported_hints: Default::default(),
            anim: AnimationEngine::default(),
            first_cache: Default::default(),
        })
    }

    /// Update the Stylo device without throwing away author/UA sheets, sidecars, font
    /// metrics or running animations. Environment revisions are already a whole-document
    /// restyle in `restyle_with`, so rebuilding the bridge is unnecessary.
    ///
    /// The two read guards are held only across Stylo's synchronous media-query
    /// invalidation; no application callbacks occur under either guard.
    pub(crate) fn update_environment(
        &mut self,
        env: StyleEnvironment,
        text: &crate::text::fonts::TextServices,
    ) {
        let device = build_device(
            &env,
            self.quirks,
            Box::new(crate::pipeline::lifecycle::SharedMetrics(
                text.metrics_service(),
            )),
        );
        let guard = self.sidecars.lock.read();
        let ua_guard = crate::style::ua::shared().lock.read();
        let guards = StylesheetGuards {
            author: &guard,
            ua_or_user: &ua_guard,
        };
        // Stylo reports the origins whose media-query results changed; their
        // cascade data must be rebuilt by the next `flush`, or rules guarded by
        // `prefers-reduced-motion`, `prefers-color-scheme` or width queries keep
        // their old applicability.
        let changed = self.stylist.set_device(device, &guards);
        if !changed.is_empty() {
            self.stylist.force_stylesheet_origins_dirty(changed);
        }
    }

    /// The document base URL (L-RES-r3): relative `<link href>` and `@import` references, and the
    /// `url()`s of `<style>` text and `style=""` values, resolve against it. Linked sheets carry
    /// their own URL. A change re-parses the author sheets at the next pass.
    pub fn set_base_url(&mut self, base: url::Url) {
        if base != self.base_url {
            self.url = UrlExtraData::from(base.clone());
            self.sheets.set_base(base.clone());
            self.base_url = base;
        }
    }

    /// Install the loader of `<link rel=stylesheet>` and `@import` sheets (007 behind the
    /// pipeline). Without one a `<link>` is reported `UNSUPPORTED_FEATURE`.
    pub fn set_sheet_loader(
        &mut self,
        loader: Option<std::sync::Arc<dyn crate::style::sheets::loader::SheetLoader>>,
    ) {
        self.sheets.set_loader(loader);
    }

    /// Read every linked sheet and `@import` again at the next update (the content behind their
    /// URLs changed: a replaced or removed `ui4me-res:` bundle).
    pub fn reload_linked_sheets(&mut self) {
        self.sheets.reload_links();
    }

    pub fn document(&self) -> DocumentId {
        self.document
    }

    /// The last published style output (immutable).
    pub fn snapshot(&self) -> Arc<StyleSnapshot> {
        self.snapshot.clone()
    }

    pub fn diagnostics(&self) -> &StyleDiagnostics {
        &self.diagnostics
    }

    /// Parse-time unsupported-feature reports (R-20 store; stage `cascade`).
    pub fn unsupported_reports(&self) -> &UnsupportedReports {
        &self.reports
    }

    /// The `OpaqueNode` key Stylo sees for `node`.
    pub fn opaque_key(&self, doc: &Document, node: NodeId) -> usize {
        with_pass(doc, &self.sidecars, |p| TNode::opaque(&p.handle(node)).0)
    }

    /// Run `f` inside an empty style pass (tests the no-nesting rule).
    #[doc(hidden)]
    pub fn with_empty_pass_for_testing<R>(&self, doc: &Document, f: impl FnOnce() -> R) -> R {
        with_pass(doc, &self.sidecars, |_| f())
    }

    fn record(&mut self, recs: Vec<StyleDiagnosticRecord>, dom_revision: u64) {
        for r in recs {
            if r.code == Code::UnsupportedFeature
                && let Some(feature) = r.feature
            {
                let location = match r.node {
                    Some(node) => ReportLocation::Element { node },
                    None => ReportLocation::Stylesheet {
                        sheet: r.sheet_id,
                        origin: r.origin,
                        line: r.line,
                        column: r.column,
                    },
                };
                let key = ReportKey {
                    feature,
                    stage: Stage::Cascade,
                    document: self.document,
                    location,
                };
                self.reports.record(
                    key,
                    Code::UnsupportedFeature,
                    UnsupportedReason::ProfileDisabled,
                    dom_revision,
                );
            }
            self.diagnostics.push(r);
        }
    }

    /// `hidden="until-found"` hides the element like `hidden` (the UA sheet) because
    /// `content-visibility` is engine-unavailable; each such element is reported once per DOM
    /// revision it exists in (R-20 key `(css.property.content-visibility, cascade, document, element)`).
    fn report_until_found(&mut self, doc: &Document, dom_revision: u64) {
        let arena = doc.arena();
        let Some(feature) =
            ui4me_engine_types::diag::FeatureId::new("css.property.content-visibility")
        else {
            return;
        };
        for id in arena.subtree(doc.document_node()) {
            let Some(e) = arena.node(id).as_element() else {
                continue;
            };
            if e.attributes()
                .iter()
                .any(|a| &*a.name.local == "hidden" && a.value.eq_ignore_ascii_case("until-found"))
            {
                let key = ReportKey {
                    feature,
                    stage: Stage::Cascade,
                    document: self.document,
                    location: ReportLocation::Element { node: id },
                };
                self.reports.record(
                    key,
                    Code::UnsupportedFeature,
                    UnsupportedReason::ProfileDisabled,
                    dom_revision,
                );
            }
        }
    }

    /// Unlisted legacy presentational attributes have no effect and are reported once per
    /// document as `UNSUPPORTED_FEATURE` `html.presentational-hint.<attr>` (FR-010).
    fn report_unlisted_hints(&mut self, doc: &Document, dom_revision: u64) {
        let arena = doc.arena();
        for id in arena.subtree(doc.document_node()) {
            let Some(e) = arena.node(id).as_element() else {
                continue;
            };
            for a in e.attributes() {
                let Some(fid) =
                    crate::style::hints::unlisted_attribute(e.local_name(), &a.name.local)
                else {
                    continue;
                };
                let Some(feature) = ui4me_engine_types::diag::FeatureId::new(fid) else {
                    continue;
                };
                if !self.reported_hints.insert(fid) {
                    continue;
                }
                let key = ReportKey {
                    feature,
                    stage: Stage::Cascade,
                    document: self.document,
                    location: ReportLocation::Element { node: id },
                };
                self.reports.record(
                    key,
                    Code::UnsupportedFeature,
                    UnsupportedReason::NotConsumedByStage,
                    dom_revision,
                );
            }
        }
    }

    /// FR-035 as amended (owner, E-018): `lh`/`rlh` under `line-height: normal` is unsupported
    /// (the pinned Stylo resolves it to 0; no patch, revisit at 009). Reported as
    /// `UNSUPPORTED_FEATURE` `css.unit.lh-normal`, once per element and DOM revision, for each
    /// styled element whose line-height is `normal` while the document uses an `lh`/`rlh` unit
    /// (in a `<style>` text or in that element's `style` attribute). `rlh` uses the root's
    /// line-height, so a normal root line-height reports every element that depends on font metrics.
    fn report_lh_normal(&mut self, doc: &Document, dom_revision: u64) {
        let Some(feature) = ui4me_engine_types::diag::FeatureId::new("css.unit.lh-normal") else {
            return;
        };
        let arena = doc.arena();
        let sheets_use = self.sheets.any_text(has_lh_unit);
        let root = doc.document_element();
        let mut found = vec![];
        with_pass(doc, &self.sidecars, |pass| {
            for id in arena.subtree(doc.document_node()) {
                let Some(e) = arena.node(id).as_element() else {
                    continue;
                };
                let attr_use = e.attr("style").is_some_and(has_lh_unit);
                if !sheets_use && !attr_use {
                    continue;
                }
                let h = pass.handle(id);
                let Some(d) = TElement::borrow_data(&h) else {
                    continue;
                };
                let normal = |cv: &ComputedValues| {
                    matches!(
                        cv.get_font().line_height,
                        style::values::computed::font::LineHeight::Normal
                    )
                };
                let own = normal(d.styles.primary());
                let root_normal = root
                    .and_then(|r| self.primary_style_in(pass, r))
                    .is_some_and(|cv| normal(&cv));
                if (attr_use || sheets_use) && (own || root_normal) {
                    found.push(id);
                }
            }
        });
        for id in found {
            let key = ReportKey {
                feature,
                stage: Stage::Cascade,
                document: self.document,
                location: ReportLocation::Element { node: id },
            };
            self.reports.record(
                key,
                Code::UnsupportedFeature,
                UnsupportedReason::NotConsumedByStage,
                dom_revision,
            );
        }
    }

    fn primary_style_in(
        &self,
        pass: &crate::style::adapter::handle::Pass<'_>,
        node: NodeId,
    ) -> Option<StyloArc<ComputedValues>> {
        let h = pass.handle(node);
        TElement::borrow_data(&h).map(|d| d.styles.primary().clone())
    }

    fn mark_ancestors_dirty(&self, doc: &Document, node: NodeId) {
        let arena = doc.arena();
        let mut p = arena.get(node).and_then(|n| n.parent());
        while let Some(pid) = p {
            if let Some(s) = self.sidecars.get(pid) {
                s.dirty_descendants.store(true, Ordering::Relaxed);
            }
            p = arena.get(pid).and_then(|n| n.parent());
        }
    }

    pub(crate) fn add_hint(&self, doc: &Document, node: NodeId, hint: RestyleHint) {
        if let Some(sc) = self.sidecars.get(node)
            && let Some(mut d) = sc.mutate_data()
        {
            d.hint.insert(hint);
            drop(d);
            self.mark_ancestors_dirty(doc, node);
        }
    }

    /// The union of every element's restyle damage since the last call, which resets it (Stylo
    /// accumulates damage until the embedder consumes it; 005 uses it to run a paint-only update
    /// when a tick changed only paint properties).
    ///
    /// Also reports whether the text `color` of any damaged element differs from the last
    /// published snapshot: the box tree resolves `currentColor` borders into its boxes, so a colour
    /// change is not paint-only even though Stylo classes it as a repaint.
    fn take_damage(&self, doc: &Document) -> (u16, bool) {
        let arena = doc.arena();
        let mut bits = 0u16;
        let mut color_changed = false;
        for i in 0..arena.slot_count() {
            if let Some(id) = arena.id_at(i)
                && let Some(sc) = self.sidecars.get(id)
                && let Some(mut d) = sc.mutate_data()
            {
                let damage = d.damage.bits();
                if damage != 0
                    && !color_changed
                    && let Some(new) = d.styles.primary.as_ref()
                {
                    color_changed = self.snapshot.styles.get(&id).is_none_or(|old| {
                        old.primary.get_inherited_text().get_color()
                            != new.get_inherited_text().get_color()
                    });
                }
                bits |= damage;
                d.damage = style::selector_parser::RestyleDamage::empty();
            }
        }
        (bits, color_changed)
    }

    /// One style pass for the document at its current DOM revision. The environment and font
    /// epochs are those of the previous pass (they are unchanged).
    pub fn restyle(
        &mut self,
        doc: &Document,
        segment: &EpochSegment,
    ) -> Result<RestyleReport, Status> {
        let inputs = RevisionInputs {
            dom: doc.revision(),
            ..self.last_inputs
        };
        self.restyle_with(doc, segment, inputs)
    }

    /// Test hook: make the next pass fail with `code` after the traversal and before publication
    /// (stands in for `OUT_OF_MEMORY`).
    #[doc(hidden)]
    pub fn fail_next_pass_for_testing(&mut self, code: Code) {
        self.fail_next = Some(code);
    }

    /// One style pass for the inputs 005 names (XR-008). Collect: the journal segment (snapshots),
    /// the sheet set and the environment and font epochs; traverse sequentially over the frozen
    /// view (no host code, no event dispatch, no DOM mutation: `doc` is shared-borrowed); publish
    /// an immutable [`StyleSnapshot`] carrying the inputs. A failure publishes nothing and the last
    /// snapshot stays valid (AD-30).
    pub fn restyle_with(
        &mut self,
        doc: &Document,
        segment: &EpochSegment,
        inputs: RevisionInputs,
    ) -> Result<RestyleReport, Status> {
        if doc.id() != self.document {
            return Err(Status::new(Code::WrongDocument, Stage::Cascade));
        }
        let dom_revision = doc.revision();
        let q = quirks(doc.mode());
        if q != self.quirks {
            self.quirks = q;
            self.stylist.set_quirks_mode(q);
        }
        let mut report = RestyleReport::default();
        // FR-067: elements that were disconnected (or whose slot was reused) lose their
        // transitions and animations silently.
        self.anim.discard_disconnected(doc);
        // An environment change (media queries, viewport units) can alter any computed value:
        // restyle everything. So does a previous failed pass. A font change only affects the
        // elements whose computed values read font metrics (`ex`, `ch`, `cap`, `ic`, `lh`, `rlh`):
        // those the engine flagged DEPENDS_ON_*_FONT_METRICS are restyled, exactly once (T037).
        let full = self.poisoned || inputs.environment != self.last_inputs.environment;
        let font_changed = inputs.font != self.last_inputs.font;
        if (full || font_changed) && self.snapshot.epoch != 0 {
            let arena = doc.arena();
            let mut flagged = vec![];
            for i in 0..arena.slot_count() {
                if let Some(id) = arena.id_at(i)
                    && let Some(sc) = self.sidecars.get(id)
                    && sc.has_data()
                {
                    if full {
                        sc.clear_data();
                    } else if let Some(d) = sc.borrow_data()
                        && depends_on_font_metrics(&d.styles)
                    {
                        flagged.push(id);
                    }
                }
            }
            for id in flagged {
                self.add_hint(doc, id, RestyleHint::RESTYLE_SELF);
            }
            report.sheets_changed |= full;
        }
        self.poisoned = true;

        // 1. Sheets.
        let lock = self.sidecars.lock.clone();
        let sync = self
            .sheets
            .sync(doc, &mut self.stylist, &lock, &self.url, self.quirks);
        if sync.changed {
            report.sheets_changed = true;
            self.diagnostics
                .clear_sheets(|s| s != UA_SHEET_ID && s != INLINE_SHEET_ID);
            self.record(sync.records, dom_revision);
        }

        // 2. Sidecars (+ `style=""` blocks).
        let url = self.url.clone();
        let quirks_mode = self.quirks;
        let mut inline_records = vec![];
        let sidecar_sync = self.sidecars.sync(doc.arena(), |node, css| {
            let (block, recs) = parse_style_attribute(css, node, &url, quirks_mode);
            inline_records.push((node, recs));
            StyloArc::new(lock.wrap(block))
        });
        for (node, recs) in inline_records {
            self.diagnostics.clear_inline(node);
            self.record(recs, dom_revision);
        }

        // 3. Invalidation inputs.
        let arena = doc.arena();
        if report.sheets_changed {
            for i in 0..arena.slot_count() {
                if let Some(id) = arena.id_at(i)
                    && let Some(sc) = self.sidecars.get(id)
                    && sc.has_data()
                {
                    sc.clear_data();
                }
            }
            // The cascade restarts without the animation rules: re-apply every set.
            self.anim.mark_all_dirty();
        }
        for &id in sidecar_sync
            .created
            .iter()
            .chain(&sidecar_sync.reset_for_reuse)
        {
            self.mark_ancestors_dirty(doc, id);
        }
        for &id in &sidecar_sync.style_attribute_changed {
            self.add_hint(doc, id, RestyleHint::RESTYLE_STYLE_ATTRIBUTE);
        }
        for (parent, added) in segment.child_list_changes() {
            if !arena.contains(parent) || !arena.is_connected(parent) {
                continue;
            }
            for &a in added {
                if !arena.contains(a) || arena.node(a).parent() != Some(parent) {
                    continue;
                }
                let subtree = arena.subtree(a);
                // A (re-)inserted subtree starts from its current computed value (FR-067).
                self.anim.discard_nodes(&subtree);
                for d in subtree {
                    if let Some(sc) = self.sidecars.get(d)
                        && sc.has_data()
                    {
                        sc.clear_data();
                    }
                }
            }
            self.add_hint(doc, parent, RestyleHint::restyle_subtree());
            self.mark_ancestors_dirty(doc, parent);
            if let Some(sc) = self.sidecars.get(parent) {
                sc.dirty_descendants.store(true, Ordering::Relaxed);
            }
        }

        // :dir() (015 WM-2): a change of directionality is not in Stylo's invalidation maps; the subtrees whose
        // directionality may have changed are restyled (`dir.rs`). Free when no sheet uses :dir().
        if !report.sheets_changed && self.sheets.any_text(|t| t.contains(":dir(")) {
            for n in crate::style::dir::invalidation_roots(doc, segment) {
                self.add_hint(doc, n, RestyleHint::restyle_subtree());
                self.mark_ancestors_dirty(doc, n);
            }
        }

        // A disabled `fieldset` (015 HTML-3): `:disabled` / `:enabled` of its descendant controls follow its
        // `disabled` attribute, which Stylo's invalidation maps do not connect to them; the fieldset's
        // subtree is restyled (a legend moved in or out is a child-list change of the fieldset, above).
        if !report.sheets_changed {
            use ui4me_document::journal::EntryKind;
            let roots: Vec<NodeId> = segment
                .entries()
                .iter()
                .filter_map(|e| match &e.kind {
                    EntryKind::Attribute { node, name }
                        if &*name.local == "disabled"
                            && arena.contains(*node)
                            && arena.is_connected(*node)
                            && arena
                                .node(*node)
                                .as_element()
                                .is_some_and(|x| x.is_html_named("fieldset")) =>
                    {
                        Some(*node)
                    }
                    _ => None,
                })
                .collect();
            for n in roots {
                self.add_hint(doc, n, RestyleHint::restyle_subtree());
                self.mark_ancestors_dirty(doc, n);
            }
        }

        // 4. The pass.
        let _layout_state = LayoutState::enter();
        let snapshots = crate::style::snapshot::build(doc, &self.sidecars, segment);
        for &id in &snapshots.nodes {
            self.mark_ancestors_dirty(doc, id);
        }
        report.snapshots = snapshots.nodes.len();
        for &(id, subtree) in &snapshots.hint_nodes {
            self.add_hint(
                doc,
                id,
                if subtree {
                    RestyleHint::restyle_subtree()
                } else {
                    RestyleHint::RESTYLE_SELF
                },
            );
        }
        let guard = lock.read();
        let ua_guard = crate::style::ua::shared().lock.read();
        let guards = StylesheetGuards {
            author: &guard,
            ua_or_user: &ua_guard,
        };
        let invalidations = self.stylist.flush(&guards);
        let stylist = &self.stylist;
        let (anim_set, anim_time) = (self.anim.set.clone(), self.anim.time_s);
        let restyled = with_pass(doc, &self.sidecars, |pass| {
            let root = doc
                .document_element()
                .filter(|&r| arena.node(r).is_element())?;
            let root = pass.handle(root);
            // :has() (HAS-1): re-evaluate the anchors of every change (Stylo does not drive its relative
            // selector invalidation from the embedder side; see has.rs). Free when no sheet uses :has().
            if crate::style::has::stylist_uses_has(stylist, root) {
                for (&node, &hint) in &crate::style::has::plan(
                    doc,
                    &self.sidecars,
                    segment,
                    stylist,
                    root,
                    self.sheets
                        .any_text(crate::style::has::has_decides_other_elements),
                )
                .anchors
                {
                    self.add_hint(
                        doc,
                        node,
                        if hint == crate::style::has::AnchorHint::Subtree {
                            RestyleHint::restyle_subtree()
                        } else {
                            RestyleHint::RESTYLE_SELF
                        },
                    );
                }
            }
            for p in crate::style::has::nth_of_parents(doc, stylist, root, &snapshots.nth_changes) {
                self.add_hint(doc, p, RestyleHint::restyle_subtree());
            }
            invalidations.process_style(root, Some(&snapshots.map));
            let context =
                shared_context(stylist, guards, &snapshots.map, anim_set.clone(), anim_time);
            let token = Recalc::pre_traverse(root, &context);
            if token.should_traverse() {
                let traversal = Recalc {
                    context,
                    restyled: AtomicUsize::new(0),
                };
                style::driver::traverse_dom(&traversal, token, None);
                Some(traversal.restyled.into_inner())
            } else {
                None
            }
        });
        for &id in &snapshots.nodes {
            if let Some(sc) = self.sidecars.get(id) {
                sc.has_snapshot.store(false, Ordering::Relaxed);
            }
        }
        drop(guard);
        if let Some(n) = restyled {
            report.traversed = true;
            report.restyled_elements = n;
            (report.damage_bits, report.color_changed) = self.take_damage(doc);
        }

        // Transitions the pass started or cancelled generate their events now (queued only).
        self.anim.after_pass(doc);
        self.cancel_hidden_animations(doc);

        self.report_until_found(doc, dom_revision);
        self.report_lh_normal(doc, dom_revision);
        self.report_unlisted_hints(doc, dom_revision);

        // 5. Publish (all or nothing).
        if let Some(code) = self.fail_next.take() {
            return Err(Status::new(code, Stage::Cascade)
                .with_message("style pass failed; nothing published"));
        }
        let inputs_changed = inputs != self.snapshot.inputs;
        let (snapshot, total) = self.collect_snapshot(
            doc,
            inputs,
            report.traversed || report.sheets_changed || self.snapshot.epoch == 0 || inputs_changed,
        );
        report.total_elements = total;
        if let Some(s) = snapshot {
            self.snapshot = Arc::new(s);
        }
        self.last_inputs = inputs;
        self.poisoned = false;
        report.epoch = self.snapshot.epoch;
        Ok(report)
    }

    /// Gather the styled elements into a new immutable snapshot (when anything changed).
    /// An element whose computed `display` is `none` loses its transitions and animations (CSS
    /// Animations 1 section 3: no box, no animation); `animationcancel` / `transitioncancel` are
    /// queued and the element is re-cascaded without them (0.9 F0901).
    fn cancel_hidden_animations(&mut self, doc: &Document) {
        let animating = self.anim.animating(doc);
        if animating.is_empty() {
            return;
        }
        let hidden: Vec<NodeId> = {
            let lock = self.sidecars.lock.clone();
            let _guard = lock.read();
            with_pass(doc, &self.sidecars, |pass| {
                animating
                    .iter()
                    .copied()
                    .filter(|&id| {
                        let h = pass.handle(id);
                        TElement::borrow_data(&h).is_some_and(|d| {
                            d.styles.primary().get_box().display
                                == style::values::specified::Display::None
                        })
                    })
                    .collect()
            })
        };
        if hidden.is_empty() {
            return;
        }
        self.anim.cancel_nodes(&hidden);
        self.anim.after_pass(doc);
        for node in hidden {
            self.add_hint(doc, node, RestyleHint::RECASCADE_SELF);
        }
    }

    fn collect_snapshot(
        &self,
        doc: &Document,
        inputs: RevisionInputs,
        changed: bool,
    ) -> (Option<StyleSnapshot>, usize) {
        let arena = doc.arena();
        let elements: Vec<NodeId> = arena
            .subtree(doc.document_node())
            .into_iter()
            .filter(|&n| arena.node(n).is_element())
            .collect();
        if !changed {
            return (None, elements.len());
        }
        let mut styles: HashMap<NodeId, NodeStyles> = HashMap::with_capacity(elements.len());
        let lock = self.sidecars.lock.clone();
        let guard = lock.read();
        with_pass(doc, &self.sidecars, |pass| {
            // Elements with an eager `::first-line` / `::first-letter` style (015 TXT-5).
            let mut eager_line: HashMap<NodeId, StyloArc<ComputedValues>> = HashMap::new();
            let mut eager_letter: HashMap<NodeId, StyloArc<ComputedValues>> = HashMap::new();
            for &id in &elements {
                let h = pass.handle(id);
                let Some(data) = TElement::borrow_data(&h) else {
                    continue;
                };
                let pseudo = |p: PseudoElement| data.styles.pseudos.get(&p).cloned();
                let primary = data.styles.primary().clone();
                // Elements are visited in tree order, so the parent is already collected.
                let parent_is_item_container = arena
                    .node(id)
                    .parent()
                    .and_then(|p| styles.get(&p))
                    .is_some_and(|p| p.primary.get_box().display.is_item_container());
                let ua_guard = crate::style::ua::shared().lock.read();
                let guards = StylesheetGuards {
                    author: &guard,
                    ua_or_user: &ua_guard,
                };
                let item_display = parent_is_item_container
                    .then(|| cascaded_display(&primary, &guards))
                    .flatten();
                // `::marker` is a lazy pseudo-element of a list item (CSS Lists 3 section 3): computed on
                // demand from the item's primary style, inheriting from it.
                let marker = if primary.get_box().display.is_list_item() {
                    self.stylist
                        .lazily_compute_pseudo_element_style(
                            &guards,
                            h,
                            &PseudoElement::Marker,
                            style::stylist::RuleInclusion::All,
                            &primary,
                            false,
                            None,
                        )
                        .filter(|m| m.get_box().display != style::values::specified::Display::None)
                } else {
                    None
                };
                // `::backdrop` is a lazy pseudo-element of an element in the top layer (CSS Position 4
                // section 3.1; 015 HTML-1), inheriting from it.
                let backdrop = if arena.node(id).as_element().is_some_and(|e| {
                    e.ui_flags()
                        .contains(ui4me_document::ui_state::UiFlags::TOP_LAYER)
                }) && primary.get_box().display
                    != style::values::specified::Display::None
                {
                    self.stylist
                        .lazily_compute_pseudo_element_style(
                            &guards,
                            h,
                            &PseudoElement::Backdrop,
                            style::stylist::RuleInclusion::All,
                            &primary,
                            false,
                            None,
                        )
                        .filter(|m| m.get_box().display != style::values::specified::Display::None)
                } else {
                    None
                };
                if let Some(p) = pseudo(PseudoElement::FirstLine) {
                    eager_line.insert(id, p);
                }
                if let Some(p) = pseudo(PseudoElement::FirstLetter) {
                    eager_letter.insert(id, p);
                }
                styles.insert(
                    id,
                    NodeStyles {
                        primary,
                        before: pseudo(PseudoElement::Before),
                        after: pseudo(PseudoElement::After),
                        marker,
                        first_letter: None,
                        first_line: None,
                        first_line_before: None,
                        first_line_after: None,
                        backdrop,
                        item_display,
                    },
                );
            }
            if !eager_line.is_empty() || !eager_letter.is_empty() {
                let ua_guard = crate::style::ua::shared().lock.read();
                let guards = StylesheetGuards {
                    author: &guard,
                    ua_or_user: &ua_guard,
                };
                let mut cache = self.first_cache.lock().unwrap_or_else(|e| e.into_inner());
                let mut phase = crate::style::first_pseudo::Phase {
                    stylist: &self.stylist,
                    guards: &guards,
                    arena,
                    handle: |n: NodeId| pass.handle(n),
                    cache: &mut cache,
                    fresh: HashMap::new(),
                    done: Default::default(),
                    _e: std::marker::PhantomData,
                };
                for &id in &elements {
                    if eager_line.contains_key(&id) {
                        phase.block(&mut styles, id, None, &eager_line);
                    }
                }
                for &id in &elements {
                    if let Some(l) = eager_letter.get(&id) {
                        phase.first_letter(&mut styles, id, l.clone());
                    }
                }
                let fresh = phase.finish();
                cache.0 = fresh;
            } else {
                self.first_cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .0
                    .clear();
            }
        });
        drop(guard);
        // Descendants of a `display: none` element have no data (Stylo clears it in
        // `recalc_style_at`), so they are absent here, as the contract requires.
        let snap = StyleSnapshot {
            document: self.document,
            epoch: self.snapshot.epoch + 1,
            inputs,
            styles,
            initial: self.initial.clone(),
            prefers_dark: self.stylist.device().color_scheme()
                == style::queries::values::PrefersColorScheme::Dark,
        };
        (Some(snap), elements.len())
    }

    /// Lookup used by the host reads (computed.rs): the element's primary style, if styled.
    pub(crate) fn primary_style(
        &self,
        doc: &Document,
        node: NodeId,
    ) -> Option<StyloArc<ComputedValues>> {
        with_pass(doc, &self.sidecars, |pass| {
            let h = pass.handle(node);
            TElement::borrow_data(&h).map(|d| d.styles.primary().clone())
        })
    }

    /// Resolve the style of an element without data (inside a `display: none` subtree) without
    /// storing it (`resolve_style`, as Servo's layout queries do).
    pub(crate) fn resolve_undisplayed(
        &self,
        doc: &Document,
        node: NodeId,
    ) -> Option<StyloArc<ComputedValues>> {
        let _layout_state = LayoutState::enter();
        let lock = self.sidecars.lock.clone();
        let guard = lock.read();
        let ua_guard = crate::style::ua::shared().lock.read();
        let guards = StylesheetGuards {
            author: &guard,
            ua_or_user: &ua_guard,
        };
        let snapshots = SnapshotMap::new();
        with_pass(doc, &self.sidecars, |pass| {
            let h = pass.handle(node);
            let shared = shared_context(
                &self.stylist,
                guards,
                &snapshots,
                self.anim.set.clone(),
                self.anim.time_s,
            );
            let mut tlc = style::context::ThreadLocalStyleContext::new();
            let mut cx = StyleContext {
                shared: &shared,
                thread_local: &mut tlc,
            };
            let styles = style::traversal::resolve_style(
                &mut cx,
                h,
                style::stylist::RuleInclusion::All,
                None,
                None,
            );
            Some(styles.primary().clone())
        })
    }
}

/// The cascaded (winning declared) `display` of an element, from its matched rules: the rule
/// node chain runs from the highest-priority declaration block to the lowest, so the first
/// `display` declaration whose importance matches its node's level wins. `None` for a CSS-wide
/// keyword or `var()` (the caller falls back to the engine's value); `inline` (the initial
/// value) when nothing declares `display`.
fn cascaded_display(
    cv: &ComputedValues,
    guards: &StylesheetGuards<'_>,
) -> Option<style::values::specified::Display> {
    use style::properties::PropertyDeclaration;
    for node in cv.rules().self_and_ancestors() {
        let Some(source) = node.style_source() else {
            continue;
        };
        let importance = node.importance();
        // a user-agent declaration is read through the user-agent lock, an author declaration through the document's
        for (decl, imp) in source
            .read(node.cascade_level().guard(guards))
            .declaration_importance_iter()
        {
            if imp != importance {
                continue;
            }
            match decl {
                PropertyDeclaration::Display(d) => return Some(*d),
                d if d.id()
                    == style::properties::PropertyDeclarationId::Longhand(
                        style::properties::LonghandId::Display,
                    ) =>
                {
                    return None;
                }
                _ => {}
            }
        }
    }
    Some(style::values::specified::Display::inline())
}

pub(crate) fn shared_context<'a>(
    stylist: &'a Stylist,
    guards: StylesheetGuards<'a>,
    snapshots: &'a SnapshotMap,
    animations: style::animation::DocumentAnimationSet,
    time_s: f64,
) -> SharedStyleContext<'a> {
    SharedStyleContext {
        traversal_flags: TraversalFlags::empty(),
        stylist,
        options: GLOBAL_STYLE_DATA.options.clone(),
        guards,
        visited_styles_enabled: false,
        animations,
        current_time_for_animations: time_s,
        snapshot_map: snapshots,
        registered_speculative_painters: &NoPainters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct T {
        doc: Document,
        style: StyleBridge,
    }

    impl T {
        fn new(html: &str) -> T {
            let doc = Document::parse_html(html).unwrap();
            let style =
                StyleBridge::new(doc.id(), StyleEnvironment::new(480.0, 360.0, 1.0)).unwrap();
            let mut t = T { doc, style };
            t.pass();
            t
        }
        fn pass(&mut self) -> RestyleReport {
            let seg = self.doc.take_epoch();
            self.style.restyle(&self.doc, &seg).unwrap()
        }
        fn color(&self, id: &str) -> String {
            let n = self.doc.element_by_id(id).unwrap();
            self.style.computed_value(&self.doc, n, "color").unwrap()
        }
    }

    const PAGE: &str = "<!DOCTYPE html><style>p { color: red } .b { color: blue }</style><p id=a>x</p><p id=b>y</p>";

    #[test]
    fn failed_pass_publishes_nothing_and_the_last_snapshot_stays_valid() {
        let mut t = T::new(PAGE);
        let before = t.style.snapshot();
        let a = t.doc.element_by_id("a").unwrap();
        t.doc.set_attribute(a, "class", "b").unwrap();
        t.style.fail_next_pass_for_testing(Code::OutOfMemory);
        let seg = t.doc.take_epoch();
        let e = t.style.restyle(&t.doc, &seg).unwrap_err();
        assert_eq!(e.code, Code::OutOfMemory);
        assert!(
            Arc::ptr_eq(&before, &t.style.snapshot()),
            "the published snapshot is untouched"
        );
        assert_eq!(t.style.snapshot().epoch(), before.epoch());
        // The next pass recovers and reflects the mutation (the failed pass poisoned the data).
        let r = t.pass();
        assert!(r.traversed || r.sheets_changed);
        assert_eq!(t.color("a"), "rgb(0, 0, 255)");
        assert_eq!(t.style.snapshot().epoch(), before.epoch() + 1);
    }

    #[test]
    fn pass_never_mutates_the_document_or_enters_host_code() {
        let mut t = T::new(PAGE);
        let a = t.doc.element_by_id("a").unwrap();
        t.doc.set_attribute(a, "class", "b").unwrap();
        let after_mutation = t.doc.revision();
        let lifecycle = t.doc.lifecycle_history().len();
        t.pass();
        assert_eq!(
            t.doc.revision(),
            after_mutation,
            "a style pass changes no DOM revision"
        );
        assert_eq!(
            t.doc.lifecycle_history().len(),
            lifecycle,
            "and dispatches nothing"
        );
    }

    #[test]
    fn snapshot_carries_the_input_epochs() {
        let mut t = T::new(PAGE);
        let inputs = RevisionInputs {
            dom: t.doc.revision(),
            environment: 3,
            font: 2,
            resource: 5,
            scroll: 7,
            animation: 11,
        };
        let seg = t.doc.take_epoch();
        t.style.restyle_with(&t.doc, &seg, inputs).unwrap();
        let s = t.style.snapshot();
        assert_eq!(s.revision_inputs(), inputs);
        let tuple = s.revision_tuple();
        assert_eq!(
            (tuple.document, tuple.inputs, tuple.style),
            (t.doc.id(), inputs, s.epoch())
        );
        assert!(tuple.is_current(t.doc.id(), &inputs));
    }

    #[test]
    fn environment_epoch_forces_a_full_restyle() {
        let mut t = T::new(PAGE);
        let quiet = t.pass();
        assert!(
            !quiet.traversed && quiet.restyled_elements == 0,
            "nothing changed: no work"
        );
        for inputs in [
            RevisionInputs {
                dom: t.doc.revision(),
                environment: 1,
                ..Default::default()
            },
            RevisionInputs {
                dom: t.doc.revision(),
                environment: 2,
                font: 1,
                ..Default::default()
            },
        ] {
            let seg = t.doc.take_epoch();
            let r = t.style.restyle_with(&t.doc, &seg, inputs).unwrap();
            assert!(r.traversed, "{inputs:?}");
            assert_eq!(
                r.restyled_elements,
                t.style.snapshot().len(),
                "every styled element is recomputed for {inputs:?}"
            );
        }
        let epoch = t.style.snapshot().epoch();
        let again = t.pass();
        assert!(!again.traversed);
        assert_eq!(
            t.style.snapshot().epoch(),
            epoch,
            "unchanged inputs publish nothing new"
        );
    }

    #[test]
    fn wrong_document_is_rejected_without_publishing() {
        let mut t = T::new(PAGE);
        let other = Document::parse_html("<p>z").unwrap();
        let e = t
            .style
            .restyle(&other, &ui4me_document::journal::EpochSegment::empty())
            .unwrap_err();
        assert_eq!(e.code, Code::WrongDocument);
    }
}
