//! Deterministic application policy. This module does not create windows,
//! spawn processes, read the clock, or call platform APIs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Connecting,
    Initializing,
    Running,
    QuitPending,
    Quitting,
    Failed,
    Stopped,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("invalid lifecycle transition from {from:?} to {to:?}")]
    InvalidTransition { from: Phase, to: Phase },
}

#[derive(Debug, Clone)]
pub struct Lifecycle {
    phase: Phase,
    failure: Option<String>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle {
    pub fn new() -> Self {
        Self {
            phase: Phase::Starting,
            failure: None,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn begin_connecting(&mut self) -> Result<(), LifecycleError> {
        self.transition(&[Phase::Starting], Phase::Connecting)
    }
    pub fn begin_initializing(&mut self) -> Result<(), LifecycleError> {
        self.transition(&[Phase::Connecting], Phase::Initializing)
    }
    pub fn ready(&mut self) -> Result<(), LifecycleError> {
        self.transition(&[Phase::Initializing], Phase::Running)
    }
    pub fn request_quit(&mut self) -> Result<(), LifecycleError> {
        self.transition(&[Phase::Running], Phase::QuitPending)
    }
    pub fn cancel_quit(&mut self) -> Result<(), LifecycleError> {
        self.transition(&[Phase::QuitPending], Phase::Running)
    }
    pub fn commit_quit(&mut self) -> Result<(), LifecycleError> {
        self.transition(&[Phase::QuitPending], Phase::Quitting)
    }

    /// Startup failure, an explicit force action or a shutdown deadline can
    /// bypass application preflight. This must not be used for normal close.
    pub fn force_quit(&mut self) -> Result<(), LifecycleError> {
        self.transition(
            &[
                Phase::Starting,
                Phase::Connecting,
                Phase::Initializing,
                Phase::Running,
                Phase::QuitPending,
                Phase::Quitting,
                Phase::Failed,
            ],
            Phase::Quitting,
        )
    }

    pub fn fail(&mut self, reason: impl Into<String>) -> Result<(), LifecycleError> {
        self.transition(
            &[
                Phase::Starting,
                Phase::Connecting,
                Phase::Initializing,
                Phase::Running,
                Phase::QuitPending,
                Phase::Quitting,
                Phase::Failed,
            ],
            Phase::Failed,
        )?;
        // Native diagnostics use a bounded message even if an upstream error
        // accidentally contains very large input. Preserve UTF-8 boundaries.
        self.failure = Some(reason.into().chars().take(4096).collect());
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), LifecycleError> {
        self.transition(&[Phase::Quitting, Phase::Failed], Phase::Stopped)
    }

    pub fn accepts_application_commands(&self) -> bool {
        matches!(
            self.phase,
            Phase::Initializing | Phase::Running | Phase::QuitPending
        )
    }

    fn transition(&mut self, allowed: &[Phase], to: Phase) -> Result<(), LifecycleError> {
        if !allowed.contains(&self.phase) {
            return Err(LifecycleError::InvalidTransition {
                from: self.phase,
                to,
            });
        }
        self.phase = to;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionKind {
    CloseWindow { window_id: String },
    QuitApplication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    #[serde(flatten)]
    pub kind: DecisionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionStart {
    pub decision: Decision,
    pub is_new: bool,
    /// Older window-close decisions replaced by an application-wide preflight.
    /// Their later replies are stale; their windows have not been destroyed.
    pub superseded: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionResolution {
    pub decision: Decision,
    pub allowed: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecisionError {
    #[error("decision is unknown, already resolved, or from an earlier session")]
    Unknown,
    #[error("an application-wide quit decision is already pending")]
    QuitPending,
    #[error("too many pending close decisions")]
    Full,
    #[error("window identity is empty")]
    InvalidWindow,
    #[error("decision sequence exhausted")]
    Exhausted,
}

#[derive(Debug, Default)]
pub struct DecisionTracker {
    next: u64,
    decisions: BTreeMap<String, Decision>,
    by_window: BTreeMap<String, String>,
    quit: Option<String>,
}

impl DecisionTracker {
    pub fn pending_count(&self) -> usize {
        self.decisions.len()
    }
    pub fn has_pending_quit(&self) -> bool {
        self.quit.is_some()
    }
    pub fn get(&self, id: &str) -> Option<&Decision> {
        self.decisions.get(id)
    }

    pub fn begin_close(&mut self, window_id: &str) -> Result<DecisionStart, DecisionError> {
        if window_id.is_empty() {
            return Err(DecisionError::InvalidWindow);
        }
        if self.quit.is_some() {
            return Err(DecisionError::QuitPending);
        }
        if let Some(id) = self.by_window.get(window_id) {
            return Ok(DecisionStart {
                decision: self.decisions[id].clone(),
                is_new: false,
                superseded: Vec::new(),
            });
        }
        if self.decisions.len() >= crate::protocol::MAX_PENDING {
            return Err(DecisionError::Full);
        }
        let decision = self.allocate(DecisionKind::CloseWindow {
            window_id: window_id.to_owned(),
        })?;
        self.by_window
            .insert(window_id.to_owned(), decision.id.clone());
        self.decisions.insert(decision.id.clone(), decision.clone());
        Ok(DecisionStart {
            decision,
            is_new: true,
            superseded: Vec::new(),
        })
    }

    pub fn begin_quit(&mut self) -> Result<DecisionStart, DecisionError> {
        if let Some(id) = &self.quit {
            return Ok(DecisionStart {
                decision: self.decisions[id].clone(),
                is_new: false,
                superseded: Vec::new(),
            });
        }
        // Allocate first: exhaustion must not discard existing decisions.
        let decision = self.allocate(DecisionKind::QuitApplication)?;
        let superseded = self.decisions.keys().cloned().collect();
        self.decisions.clear();
        self.by_window.clear();
        self.quit = Some(decision.id.clone());
        self.decisions.insert(decision.id.clone(), decision.clone());
        Ok(DecisionStart {
            decision,
            is_new: true,
            superseded,
        })
    }

    pub fn resolve(&mut self, id: &str, allow: bool) -> Result<DecisionResolution, DecisionError> {
        let decision = self.decisions.remove(id).ok_or(DecisionError::Unknown)?;
        match &decision.kind {
            DecisionKind::CloseWindow { window_id } => {
                self.by_window.remove(window_id);
            }
            DecisionKind::QuitApplication => {
                self.quit = None;
            }
        }
        Ok(DecisionResolution {
            decision,
            allowed: allow,
        })
    }

    pub fn cancel_for_window(&mut self, window_id: &str) -> Option<Decision> {
        self.by_window
            .remove(window_id)
            .and_then(|id| self.decisions.remove(&id))
    }

    pub fn cancel_all(&mut self) -> Vec<Decision> {
        self.by_window.clear();
        self.quit = None;
        std::mem::take(&mut self.decisions).into_values().collect()
    }

    fn allocate(&mut self, kind: DecisionKind) -> Result<Decision, DecisionError> {
        self.next = self.next.checked_add(1).ok_or(DecisionError::Exhausted)?;
        Ok(Decision {
            id: format!("host:decision:{}", self.next),
            kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> Lifecycle {
        let mut life = Lifecycle::new();
        life.begin_connecting().unwrap();
        life.begin_initializing().unwrap();
        life.ready().unwrap();
        life
    }

    #[test]
    fn spawn_does_not_imply_ready() {
        let mut life = Lifecycle::new();
        assert!(life.ready().is_err());
        life.begin_connecting().unwrap();
        assert!(!life.accepts_application_commands());
        assert!(life.ready().is_err());
        life.begin_initializing().unwrap();
        assert!(life.accepts_application_commands());
        life.ready().unwrap();
        assert_eq!(life.phase(), Phase::Running);
    }

    #[test]
    fn quit_preflight_can_cancel_before_shutdown() {
        let mut life = running();
        life.request_quit().unwrap();
        assert!(life.accepts_application_commands()); // save dialog can run
        life.cancel_quit().unwrap();
        assert_eq!(life.phase(), Phase::Running);
        life.request_quit().unwrap();
        life.commit_quit().unwrap();
        assert!(!life.accepts_application_commands());
        assert!(life.cancel_quit().is_err());
        life.stop().unwrap();
    }

    #[test]
    fn failure_survives_cleanup_and_cannot_resurrect_stopped_session() {
        let mut life = running();
        life.fail("backend exited with code 2").unwrap();
        assert_eq!(life.phase(), Phase::Failed);
        life.force_quit().unwrap();
        life.stop().unwrap();
        assert_eq!(life.failure(), Some("backend exited with code 2"));
        assert!(life.ready().is_err());
        assert!(life.fail("late event").is_err());
    }

    #[test]
    fn repeated_close_coalesces_and_replies_are_one_shot() {
        let mut tracker = DecisionTracker::default();
        let first = tracker.begin_close("window-1").unwrap();
        let again = tracker.begin_close("window-1").unwrap();
        assert!(first.is_new);
        assert!(!again.is_new);
        assert_eq!(first.decision.id, again.decision.id);
        assert_eq!(tracker.pending_count(), 1);
        assert!(!tracker.resolve(&first.decision.id, false).unwrap().allowed);
        assert_eq!(
            tracker.resolve(&first.decision.id, true),
            Err(DecisionError::Unknown)
        );
        assert_ne!(
            tracker.begin_close("window-1").unwrap().decision.id,
            first.decision.id
        );
    }

    #[test]
    fn global_quit_supersedes_individual_close_without_destroying_windows() {
        let mut tracker = DecisionTracker::default();
        let close_a = tracker.begin_close("a").unwrap();
        let close_b = tracker.begin_close("b").unwrap();
        let quit = tracker.begin_quit().unwrap();
        assert_eq!(quit.superseded.len(), 2);
        assert!(quit.superseded.contains(&close_a.decision.id));
        assert_eq!(
            tracker.resolve(&close_b.decision.id, true),
            Err(DecisionError::Unknown)
        );
        assert_eq!(tracker.begin_close("c"), Err(DecisionError::QuitPending));
        assert!(!tracker.resolve(&quit.decision.id, false).unwrap().allowed);
        assert_eq!(tracker.pending_count(), 0);
        // A cancelled app quit leaves window a available for a fresh decision.
        assert!(tracker.begin_close("a").is_ok());
    }

    #[test]
    fn destroying_a_window_or_session_invalidates_pending_decisions() {
        let mut tracker = DecisionTracker::default();
        let close = tracker.begin_close("a").unwrap();
        assert_eq!(tracker.cancel_for_window("a").unwrap(), close.decision);
        assert_eq!(
            tracker.resolve(&close.decision.id, true),
            Err(DecisionError::Unknown)
        );
        tracker.begin_close("b").unwrap();
        assert_eq!(tracker.cancel_all().len(), 1);
        assert_eq!(tracker.pending_count(), 0);
    }
}
