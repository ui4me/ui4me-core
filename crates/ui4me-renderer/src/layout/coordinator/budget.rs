//! owner: 008-layout-engine (T013) — layout's diagnostic sink.
//!
//! Layout keeps **no de-duplication state** (R-20): every emission is forwarded to 005's
//! `UnsupportedReports` store, whose single key `(feature, stage, document, location)` makes one
//! record per key per DOM revision. Stage diagnostics (`GEOMETRY_NON_FINITE`, non-fatal
//! `BUDGET_EXCEEDED`, `LAYOUT_NON_CONVERGENCE`, `RESOURCE_PENDING`) attach to the layout epoch
//! (005 outcome O3) and never fail it.

use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::diag::{
    FeatureId, ReportKey, ReportLocation, UnsupportedReason, UnsupportedReports,
};
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

use super::LayoutReport;

/// R-21 internal error names that layout maps to canonical codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutFault {
    /// A resource is awaited: `RESOURCE_PENDING`; otherwise `INTERNAL_INVARIANT`.
    UnresolvedIntrinsicSize { resource_awaited: bool },
    /// `INVALID_ARGUMENT`.
    InvalidConstraint,
}

impl LayoutFault {
    pub fn code(self) -> Code {
        match self {
            LayoutFault::UnresolvedIntrinsicSize {
                resource_awaited: true,
            } => Code::ResourcePending,
            LayoutFault::UnresolvedIntrinsicSize {
                resource_awaited: false,
            } => Code::InternalInvariant,
            LayoutFault::InvalidConstraint => Code::InvalidArgument,
        }
    }
}

/// Collects one layout pass's reports and epoch diagnostics.
#[derive(Debug)]
pub struct DiagSink {
    document: DocumentId,
    reports: Vec<LayoutReport>,
    diagnostics: Vec<Status>,
}

impl DiagSink {
    pub fn new(document: DocumentId) -> Self {
        DiagSink {
            document,
            reports: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// `UNSUPPORTED_FEATURE` for `feature` at `node`, stage `box` or `layout` only.
    pub fn unsupported(&mut self, feature: FeatureId, stage: Stage, node: NodeId) {
        debug_assert!(
            matches!(stage, Stage::Box | Stage::Layout),
            "layout reports use stage box or layout"
        );
        let key = ReportKey {
            feature,
            stage,
            document: self.document,
            location: ReportLocation::Element { node },
        };
        self.reports.push(LayoutReport {
            key,
            code: Code::UnsupportedFeature,
            reason: UnsupportedReason::NotConsumedByStage,
        });
    }

    /// Attach a stage diagnostic to the epoch (never fails the layout).
    pub fn attach(&mut self, code: Code, message: impl Into<String>) {
        self.diagnostics
            .push(Status::new(code, Stage::Layout).with_message(message));
    }

    pub fn geometry_non_finite(&mut self, what: &str) {
        self.attach(
            Code::GeometryNonFinite,
            format!("{what} was not finite; replaced by the nearest bound"),
        );
    }
    pub fn budget_exceeded(&mut self, what: &str) {
        self.attach(Code::BudgetExceeded, what.to_string());
    }
    pub fn non_convergence(&mut self, what: &str) {
        self.attach(Code::LayoutNonConvergence, what.to_string());
    }
    pub fn resource_pending(&mut self, what: &str) {
        self.attach(Code::ResourcePending, what.to_string());
    }
    pub fn fault(&mut self, fault: LayoutFault, message: &str) {
        self.attach(fault.code(), message.to_string());
    }

    pub fn reports(&self) -> &[LayoutReport] {
        &self.reports
    }
    pub fn diagnostics(&self) -> &[Status] {
        &self.diagnostics
    }

    /// Forward every report to 005's store for the pass's DOM revision.
    pub fn forward(&self, store: &mut UnsupportedReports, dom_revision: u64) {
        for r in &self.reports {
            store.record(r.key, r.code, r.reason, dom_revision);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::ids::Slots;

    fn ids() -> (DocumentId, NodeId) {
        let mut s = Slots::new();
        (DocumentId::from_raw(s.alloc()), NodeId::from_raw(s.alloc()))
    }

    #[test]
    fn two_identical_emissions_give_one_record_per_key_per_revision() {
        let (doc, node) = ids();
        let f = FeatureId::new("css.display.ruby").unwrap();
        let mut sink = DiagSink::new(doc);
        sink.unsupported(f, Stage::Box, node);
        sink.unsupported(f, Stage::Box, node);
        let mut store = UnsupportedReports::default();
        sink.forward(&mut store, 7);
        sink.forward(&mut store, 7);
        assert_eq!(store.len(), 1);
        assert_eq!(store.iter().next().unwrap().revisions, 1);
        sink.forward(&mut store, 8);
        assert_eq!(store.iter().next().unwrap().revisions, 2);
    }

    #[test]
    fn stage_diagnostics_attach_to_the_epoch() {
        let (doc, _) = ids();
        let mut sink = DiagSink::new(doc);
        sink.geometry_non_finite("width");
        sink.budget_exceeded("depth");
        sink.non_convergence("line retries");
        sink.resource_pending("image");
        let codes: Vec<_> = sink.diagnostics().iter().map(|s| s.code).collect();
        assert_eq!(
            codes,
            [
                Code::GeometryNonFinite,
                Code::BudgetExceeded,
                Code::LayoutNonConvergence,
                Code::ResourcePending
            ]
        );
        assert!(sink.reports().is_empty());
    }

    #[test]
    fn r21_names_map_to_canonical_codes() {
        assert_eq!(
            LayoutFault::UnresolvedIntrinsicSize {
                resource_awaited: true
            }
            .code(),
            Code::ResourcePending
        );
        assert_eq!(
            LayoutFault::UnresolvedIntrinsicSize {
                resource_awaited: false
            }
            .code(),
            Code::InternalInvariant
        );
        assert_eq!(LayoutFault::InvalidConstraint.code(), Code::InvalidArgument);
    }
}
