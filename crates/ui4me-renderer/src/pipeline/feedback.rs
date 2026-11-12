//! owner: 005-pipeline-diagnostics — T062. The bounded style/layout feedback loop (recompute-plan
//! §5; AD-10), live since 015 CQ-3 (lane L-CQ).
//!
//! ```text
//! iter := 0
//! loop:
//!   S3 publishes ContainerFacts(N+1); compare with the ContainerSizes table S1 used
//!   if no query container changed: break
//!   if iter == BOUND (8): emit BOUND(iter); publish this round; break
//!   iter += 1
//!   restyle the subtrees of the changed containers; re-run S2..S4
//! ```
//!
//! The real loop is `DocumentPipeline::container_loop` (update.rs): the container sizes of the layout
//! (`LayoutOutput::container_facts`) go to the style stage (`StyleBridge::update_container_sizes`,
//! ui4me-style `container.rs`), which hints a subtree restyle at every changed container. Size
//! containment makes a container's size independent of its contents, so an outer container settles
//! before the inner ones are evaluated against it: the rounds needed are at most the nesting depth
//! of containers whose size a query changes. The bound is 8; content whose queries keep changing the
//! size of their own container (a cycle) is published after the last round with
//! `LAYOUT_NON_CONVERGENCE`. The cold oracle (`cold_stages`) runs the same rounds from scratch.
//!
//! [`run`] and [`ContainerFacts`] remain for tests: a test-only fact source replaces the real loop
//! (`set_container_facts_for_testing`) to exercise the bound without content.

/// The iteration bound (recompute-plan §10; raised from 4 to 8 for nested containers, 015 CQ-3).
pub const BOUND: u32 = 8;

/// How a feedback loop ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopEnd {
    /// No queried container changed between layout epochs.
    Converged { iterations: u32 },
    /// The bound was reached: `BOUND(iter)`; the last iteration is published together with a
    /// `LAYOUT_NON_CONVERGENCE` diagnostic.
    Bound { iterations: u32 },
}

impl LoopEnd {
    pub fn iterations(self) -> u32 {
        match self {
            LoopEnd::Converged { iterations } | LoopEnd::Bound { iterations } => iterations,
        }
    }
    pub fn non_convergence(self) -> bool {
        matches!(self, LoopEnd::Bound { .. })
    }
}

/// The source of container facts between layout epochs.
pub trait ContainerFacts {
    /// Re-run restyle and layout for the containers whose queried facts changed in the last
    /// epoch; returns whether any queried container changed again.
    fn another_round_needed(&mut self, iteration: u32) -> bool;
}

/// A source that never changes (tests: it replaces the real loop with one that converges at once).
#[derive(Clone, Copy, Debug, Default)]
pub struct InertFacts;

impl ContainerFacts for InertFacts {
    fn another_round_needed(&mut self, _iteration: u32) -> bool {
        false
    }
}

/// Run the loop with `bound` iterations at most.
pub fn run(facts: &mut dyn ContainerFacts, bound: u32) -> LoopEnd {
    let mut iter = 0;
    loop {
        if !facts.another_round_needed(iter) {
            return LoopEnd::Converged { iterations: iter };
        }
        iter += 1;
        if iter == bound {
            return LoopEnd::Bound { iterations: iter };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Changing(u32);
    impl ContainerFacts for Changing {
        fn another_round_needed(&mut self, iteration: u32) -> bool {
            iteration < self.0
        }
    }

    #[test]
    fn the_inert_loop_never_iterates() {
        assert_eq!(
            run(&mut InertFacts, BOUND),
            LoopEnd::Converged { iterations: 0 }
        );
        assert_eq!(
            run(&mut InertFacts, BOUND).iterations(),
            0,
            "invalidation records report container = 0"
        );
    }

    #[test]
    fn a_loop_that_settles_reports_its_iterations() {
        assert_eq!(
            run(&mut Changing(2), BOUND),
            LoopEnd::Converged { iterations: 2 }
        );
        assert!(!run(&mut Changing(2), BOUND).non_convergence());
    }

    #[test]
    fn the_bound_stops_a_loop_that_never_settles_and_flags_non_convergence() {
        let end = run(&mut Changing(u32::MAX), BOUND);
        assert_eq!(end, LoopEnd::Bound { iterations: 8 });
        assert!(end.non_convergence());
        assert_eq!(
            run(&mut Changing(u32::MAX), 1),
            LoopEnd::Bound { iterations: 1 }
        );
    }

    #[test]
    fn the_bound_is_eight() {
        assert_eq!(BOUND, 8);
    }
}
