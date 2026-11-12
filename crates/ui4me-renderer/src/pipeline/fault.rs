//! owner: 005-pipeline-diagnostics — T022. Fault-injection points (research R-16; U4T-184,
//! U4T-192). Compiled only with the test-only `fault-injection` feature; absent from release
//! builds.
//!
//! - Named points `style`, `box`, `layout`, `scene` and `publish` raise `OUT_OF_MEMORY` or
//!   `INTERNAL_INVARIANT` on the n-th call (counted per point), exactly once.
//! - A deliberate invalidation omission (skip restyle for attribute-selector dependants) proves
//!   that the cold oracle detects a missing invalidation.

use ui4me_document::journal::{EntryKind, EpochSegment};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

use crate::pipeline::state::DocumentPipeline;

/// The named injection points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaultPoint {
    Style,
    Box,
    Layout,
    Scene,
    Publish,
}

impl FaultPoint {
    pub const ALL: [FaultPoint; 5] = [
        FaultPoint::Style,
        FaultPoint::Box,
        FaultPoint::Layout,
        FaultPoint::Scene,
        FaultPoint::Publish,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            FaultPoint::Style => "style",
            FaultPoint::Box => "box",
            FaultPoint::Layout => "layout",
            FaultPoint::Scene => "scene",
            FaultPoint::Publish => "publish",
        }
    }

    /// The stage the injected error is attributed to.
    pub const fn stage(self) -> Stage {
        match self {
            FaultPoint::Style => Stage::Cascade,
            FaultPoint::Box => Stage::Box,
            FaultPoint::Layout => Stage::Layout,
            // Publication swaps in the scene and hit index (recompute-plan §1, step P).
            FaultPoint::Scene | FaultPoint::Publish => Stage::Scene,
        }
    }

    pub fn from_stage(stage: Stage) -> Option<FaultPoint> {
        match stage {
            Stage::Cascade => Some(FaultPoint::Style),
            Stage::Box => Some(FaultPoint::Box),
            Stage::Layout => Some(FaultPoint::Layout),
            Stage::Scene => Some(FaultPoint::Scene),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Armed {
    point: FaultPoint,
    /// Fire on this call number (1-based), counted from arming.
    nth: u64,
    code: Code,
    seen: u64,
    fired: bool,
}

/// The armed faults of one pipeline.
#[derive(Clone, Debug, Default)]
pub struct FaultPlan {
    armed: Vec<Armed>,
    omit_attribute_selector_restyle: bool,
}

impl FaultPlan {
    /// Raise `code` (`OUT_OF_MEMORY` or `INTERNAL_INVARIANT`) on the `nth` call (1-based) of
    /// `point` from now on.
    pub fn arm(&mut self, point: FaultPoint, nth: u64, code: Code) {
        assert!(nth >= 1, "calls are numbered from 1");
        assert!(
            matches!(code, Code::OutOfMemory | Code::InternalInvariant),
            "injected codes are OUT_OF_MEMORY or INTERNAL_INVARIANT"
        );
        self.armed.push(Armed {
            point,
            nth,
            code,
            seen: 0,
            fired: false,
        });
    }

    /// One call of `point`: `Err` exactly once, on the configured call.
    pub fn hit(&mut self, point: FaultPoint) -> Result<(), Status> {
        for a in self
            .armed
            .iter_mut()
            .filter(|a| a.point == point && !a.fired)
        {
            a.seen += 1;
            if a.seen == a.nth {
                a.fired = true;
                return Err(Status::new(a.code, point.stage()).with_message(format!(
                    "injected {} failure at the `{}` point",
                    a.code.name(),
                    point.name()
                )));
            }
        }
        Ok(())
    }

    /// How many times each point was reached since it was armed (for tests).
    pub fn calls(&self, point: FaultPoint) -> u64 {
        self.armed
            .iter()
            .filter(|a| a.point == point)
            .map(|a| a.seen)
            .max()
            .unwrap_or(0)
    }

    pub fn set_omit_attribute_selector_restyle(&mut self, on: bool) {
        self.omit_attribute_selector_restyle = on;
    }

    /// The journal segment the incremental restyle consumes. With the omission on, a segment
    /// holding a change to an attribute other than `class`, `id` and `style` is replaced by an
    /// empty one: the dependants of attribute selectors are never restyled.
    pub fn filter_segment(&self, segment: &EpochSegment) -> Option<EpochSegment> {
        if !self.omit_attribute_selector_restyle {
            return None;
        }
        let hits_attribute_selector = segment.entries().iter().any(|e| match &e.kind {
            EntryKind::Attribute { name, .. } => !matches!(&*name.local, "class" | "id" | "style"),
            _ => false,
        });
        hits_attribute_selector.then(EpochSegment::empty)
    }
}

impl DocumentPipeline {
    /// Raise `code` on the `nth` call of `point`, once.
    pub fn arm_fault(&mut self, point: FaultPoint, nth: u64, code: Code) {
        self.faults.arm(point, nth, code);
    }

    /// Switch the deliberate attribute-selector invalidation omission on or off.
    pub fn set_omit_attribute_selector_restyle(&mut self, on: bool) {
        self.faults.set_omit_attribute_selector_restyle(on);
    }

    pub fn fault_calls(&self, point: FaultPoint) -> u64 {
        self.faults.calls(point)
    }
}
