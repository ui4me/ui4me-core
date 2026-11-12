//! owner: 012-interaction — created by 001 slice (R-28)
//!
//! Listener registry (001 T058; 012 `dispatch-state-machine.md` §1–§2). Entries are shared
//! records (`Rc<ListenerEntry>`) so a dispatch snapshot sees the `removed` flag set by a later
//! removal (DOM "removed" flag, `dom.bs:1509-1545`). Duplicate detection uses 004's
//! `identity_key` together with target, type and capture; the optional label (R-25) is echoed
//! into 005's event log and is not part of duplicate detection.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use ui4me_engine_types::diag::ListenerRecord;
use ui4me_engine_types::ids::{CallbackToken, ListenerToken, NodeId};

/// `addEventListener` options (`capture`, `once`, `passive`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ListenerOptions {
    pub capture: bool,
    pub once: bool,
    pub passive: bool,
}

/// A registration request (004 `ui4me_listener_add`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenerSpec {
    pub event_type: String,
    pub options: ListenerOptions,
    /// 004's host-side identity of the callable (e.g. Python `id(fn)`); duplicate detection only.
    pub identity_key: u64,
    /// The host callback (rooted by 004's callback table, AD-43).
    pub callback: CallbackToken,
    /// Optional host label (R-25), echoed in the event log; not part of duplicate detection.
    pub label: Option<String>,
}

impl ListenerSpec {
    pub fn new(event_type: impl Into<String>, callback: CallbackToken) -> Self {
        ListenerSpec {
            event_type: event_type.into(),
            options: ListenerOptions::default(),
            identity_key: callback.get().get(),
            callback,
            label: None,
        }
    }
    pub fn capture(mut self, on: bool) -> Self {
        self.options.capture = on;
        self
    }
    pub fn once(mut self, on: bool) -> Self {
        self.options.once = on;
        self
    }
    pub fn passive(mut self, on: bool) -> Self {
        self.options.passive = on;
        self
    }
    pub fn identity_key(mut self, key: u64) -> Self {
        self.identity_key = key;
        self
    }
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// One registration, shared between the registry and dispatch snapshots.
#[derive(Debug)]
pub struct ListenerEntry {
    pub token: ListenerToken,
    pub target: NodeId,
    pub event_type: String,
    pub options: ListenerOptions,
    pub identity_key: u64,
    pub callback: CallbackToken,
    pub label: Option<String>,
    /// Registration ordinal within the document, from 1 (event-log `listener.ordinal`).
    pub ordinal: u32,
    removed: Cell<bool>,
}

impl ListenerEntry {
    pub fn is_removed(&self) -> bool {
        self.removed.get()
    }
    pub(crate) fn record(&self) -> ListenerRecord {
        ListenerRecord {
            ordinal: self.ordinal,
            label: self.label.clone(),
            capture: self.options.capture,
            once: self.options.once,
            passive: self.options.passive,
        }
    }
}

/// Result of an add: a duplicate returns the existing token (004: the host then releases the
/// new callback's user data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddOutcome {
    pub token: ListenerToken,
    pub duplicate: bool,
}

/// A registration whose host callback must be released (004 release notification), delivered
/// once: directly to the remover outside dispatch, else in COMMIT.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Released {
    pub listener: ListenerToken,
    pub callback: CallbackToken,
}

/// The listener lists of one document, keyed by event target.
#[derive(Debug, Default)]
pub struct ListenerRegistry {
    lists: HashMap<NodeId, Vec<Rc<ListenerEntry>>>,
    by_token: HashMap<ListenerToken, NodeId>,
    next_ordinal: u32,
}

impl ListenerRegistry {
    pub(crate) fn find_matching(
        &self,
        target: NodeId,
        event_type: &str,
        capture: bool,
        identity_key: u64,
    ) -> Option<&Rc<ListenerEntry>> {
        self.lists.get(&target)?.iter().find(|e| {
            e.event_type == event_type
                && e.options.capture == capture
                && e.identity_key == identity_key
        })
    }

    pub(crate) fn add(
        &mut self,
        token: ListenerToken,
        target: NodeId,
        spec: ListenerSpec,
    ) -> AddOutcome {
        if let Some(e) = self.find_matching(
            target,
            &spec.event_type,
            spec.options.capture,
            spec.identity_key,
        ) {
            return AddOutcome {
                token: e.token,
                duplicate: true,
            };
        }
        self.next_ordinal += 1;
        let entry = ListenerEntry {
            token,
            target,
            event_type: spec.event_type,
            options: spec.options,
            identity_key: spec.identity_key,
            callback: spec.callback,
            label: spec.label,
            ordinal: self.next_ordinal,
            removed: Cell::new(false),
        };
        self.lists.entry(target).or_default().push(Rc::new(entry));
        self.by_token.insert(token, target);
        AddOutcome {
            token,
            duplicate: false,
        }
    }

    /// Set the `removed` flag and drop the entry from its list. `None` if unknown (idempotent).
    pub(crate) fn remove(&mut self, token: ListenerToken) -> Option<Rc<ListenerEntry>> {
        let target = self.by_token.remove(&token)?;
        let list = self.lists.get_mut(&target)?;
        let pos = list.iter().position(|e| e.token == token)?;
        let e = list.remove(pos);
        if list.is_empty() {
            self.lists.remove(&target);
        }
        e.removed.set(true);
        Some(e)
    }

    pub(crate) fn contains(&self, token: ListenerToken) -> bool {
        self.by_token.contains_key(&token)
    }

    /// Clone of the target's current list (the per-item, per-phase snapshot).
    pub(crate) fn snapshot(&self, target: NodeId) -> Vec<Rc<ListenerEntry>> {
        self.lists.get(&target).cloned().unwrap_or_default()
    }

    /// Drop every listener of freed nodes (003 `on_nodes_freed`).
    pub(crate) fn drop_nodes(&mut self, nodes: &[NodeId]) -> Vec<Released> {
        let mut out = vec![];
        for n in nodes {
            if let Some(list) = self.lists.remove(n) {
                for e in list {
                    e.removed.set(true);
                    self.by_token.remove(&e.token);
                    out.push(Released {
                        listener: e.token,
                        callback: e.callback,
                    });
                }
            }
        }
        out
    }

    /// Drop everything (document closed or detached).
    pub(crate) fn drain_all(&mut self) -> Vec<Released> {
        let nodes: Vec<NodeId> = self.lists.keys().copied().collect();
        let mut out = self.drop_nodes(&nodes);
        out.sort_by_key(|r| r.listener);
        out
    }

    /// Number of live registrations on `target`.
    pub fn count_on(&self, target: NodeId) -> usize {
        self.lists.get(&target).map_or(0, Vec::len)
    }

    pub fn len(&self) -> usize {
        self.by_token.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_token.is_empty()
    }
}
