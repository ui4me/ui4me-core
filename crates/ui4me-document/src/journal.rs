//! owner: 003-document-model — created by 001 slice (R-28)
//!
//! Mutation journal (T037; 003 `contracts/mutation-journal.md`; AD-07). Entries are appended
//! only for committed calls, in commit order; all entries of one call share its revision.
//!
//! * **Epoch segment.** [`Journal::take_epoch`] (called by 005's scheduler, the single
//!   consumer) returns the current style epoch's entries and starts epoch *e+1*.
//! * **Prior-value rule.** Within one epoch the first attribute change on an element emits
//!   `prior_attributes` (the complete list before that change); the first selector-relevant
//!   control change emits `prior_control_flags`; every `ui_state` entry carries old/new flags.
//! * **Window.** A probe window of at least 4,096 entries; reading older revisions returns
//!   what is retained with `truncated: true` (never an error).
//!
//! Consuming or reading the journal changes no DOM-observable state (FR-086). No style-engine
//! type appears here: StyleBridge turns [`EpochSegment::element_priors`] into snapshots.

use crate::control_state::{ControlField, ControlFlags};
use crate::node::Attribute;
use crate::ui_state::UiFlags;
use html5ever::QualName;
use std::collections::{BTreeMap, HashSet, VecDeque};
use ui4me_engine_types::ids::NodeId;

/// Minimum probe window (spec Clarification Q4).
pub const WINDOW: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// One insertion/removal step; siblings as they were at the time of the change.
    ChildList {
        parent: NodeId,
        added: Vec<NodeId>,
        removed: Vec<NodeId>,
        prev_sibling: Option<NodeId>,
        next_sibling: Option<NodeId>,
    },
    Attribute {
        node: NodeId,
        name: QualName,
    },
    PriorAttributes {
        node: NodeId,
        attributes: Vec<Attribute>,
    },
    CharacterData {
        node: NodeId,
    },
    ControlState {
        node: NodeId,
        field: ControlField,
    },
    PriorControlFlags {
        node: NodeId,
        flags: ControlFlags,
    },
    UiState {
        node: NodeId,
        old: UiFlags,
        new: UiFlags,
    },
    Stylesheets {
        stylesheet_revision: u64,
    },
}

impl EntryKind {
    /// 005's `kind` string.
    pub const fn name(&self) -> &'static str {
        match self {
            EntryKind::ChildList { .. } => "child_list",
            EntryKind::Attribute { .. } => "attribute",
            EntryKind::PriorAttributes { .. } => "prior_attributes",
            EntryKind::CharacterData { .. } => "character_data",
            EntryKind::ControlState { .. } => "control_state",
            EntryKind::PriorControlFlags { .. } => "prior_control_flags",
            EntryKind::UiState { .. } => "ui_state",
            EntryKind::Stylesheets { .. } => "stylesheets",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub seq: u64,
    /// DOM revision of the committing call.
    pub revision: u64,
    pub style_epoch: u64,
    pub kind: EntryKind,
}

/// What StyleBridge needs per element to build an engine snapshot for one epoch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElementPrior {
    /// Complete attribute list before the first attribute change of the epoch.
    pub attributes: Option<Vec<Attribute>>,
    /// Names of attributes changed in the epoch, in first-change order.
    pub changed_attributes: Vec<QualName>,
    /// UI flags before the first UI-state change of the epoch.
    pub ui: Option<UiFlags>,
    /// Selector-relevant control flags before the first such change of the epoch.
    pub control: Option<ControlFlags>,
}

/// One style epoch's entries.
#[derive(Clone, Debug, Default)]
pub struct EpochSegment {
    epoch: u64,
    entries: Vec<JournalEntry>,
}

impl EpochSegment {
    /// A segment with no entries (a cold restyle consumes nothing).
    pub fn empty() -> EpochSegment {
        EpochSegment::default()
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Per-element first-change summary.
    pub fn element_priors(&self) -> BTreeMap<NodeId, ElementPrior> {
        let mut out: BTreeMap<NodeId, ElementPrior> = BTreeMap::new();
        for e in &self.entries {
            match &e.kind {
                EntryKind::PriorAttributes { node, attributes } => {
                    out.entry(*node)
                        .or_default()
                        .attributes
                        .get_or_insert_with(|| attributes.clone());
                }
                EntryKind::Attribute { node, name } => {
                    let p = out.entry(*node).or_default();
                    if !p.changed_attributes.contains(name) {
                        p.changed_attributes.push(name.clone());
                    }
                }
                EntryKind::UiState { node, old, .. } => {
                    out.entry(*node).or_default().ui.get_or_insert(*old);
                }
                EntryKind::PriorControlFlags { node, flags } => {
                    out.entry(*node).or_default().control.get_or_insert(*flags);
                }
                _ => {}
            }
        }
        out
    }

    /// `(parent, added)` of every child-list entry, in commit order.
    pub fn child_list_changes(&self) -> impl Iterator<Item = (NodeId, &[NodeId])> {
        self.entries.iter().filter_map(|e| match &e.kind {
            EntryKind::ChildList { parent, added, .. } => Some((*parent, added.as_slice())),
            _ => None,
        })
    }

    /// Whether the stylesheet source list changed in this epoch.
    pub fn stylesheets_changed(&self) -> bool {
        self.entries
            .iter()
            .any(|e| matches!(e.kind, EntryKind::Stylesheets { .. }))
    }
}

/// `dom.journal` probe result.
#[derive(Clone, Debug, Default)]
pub struct JournalWindow {
    pub since_revision: u64,
    pub entries: Vec<JournalEntry>,
    pub truncated: bool,
    pub oldest_revision: u64,
}

#[derive(Debug, Default)]
pub struct Journal {
    epoch: u64,
    seq: u64,
    segment: Vec<JournalEntry>,
    window: VecDeque<JournalEntry>,
    /// Highest revision among entries evicted from the window.
    evicted_revision: Option<u64>,
    prior_attrs_seen: HashSet<NodeId>,
    prior_control_seen: HashSet<NodeId>,
    /// Entries of an open batch, held back until commit (FR-083, T017/T079).
    staged: Option<Staged>,
}

#[derive(Debug, Default)]
struct Staged {
    entries: Vec<(u64, EntryKind)>,
    prior_attrs_seen: HashSet<NodeId>,
    prior_control_seen: HashSet<NodeId>,
}

impl Journal {
    pub(crate) fn new() -> Journal {
        Journal::default()
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Hold back entries (batch): nothing reaches the window or the epoch segment until
    /// [`Journal::commit_stage`]; [`Journal::rollback_stage`] discards them and the first-change
    /// bookkeeping they caused.
    pub(crate) fn begin_stage(&mut self) {
        debug_assert!(self.staged.is_none(), "batches do not nest");
        self.staged = Some(Staged {
            entries: vec![],
            prior_attrs_seen: self.prior_attrs_seen.clone(),
            prior_control_seen: self.prior_control_seen.clone(),
        });
    }

    pub(crate) fn commit_stage(&mut self) {
        if let Some(st) = self.staged.take() {
            for (rev, kind) in st.entries {
                self.push(rev, kind);
            }
        }
    }

    pub(crate) fn rollback_stage(&mut self) {
        if let Some(st) = self.staged.take() {
            self.prior_attrs_seen = st.prior_attrs_seen;
            self.prior_control_seen = st.prior_control_seen;
        }
    }

    pub fn is_staging(&self) -> bool {
        self.staged.is_some()
    }

    /// Number of entries held back by an open batch.
    pub fn staged_len(&self) -> usize {
        self.staged.as_ref().map_or(0, |s| s.entries.len())
    }

    pub(crate) fn push(&mut self, revision: u64, kind: EntryKind) {
        if let Some(st) = self.staged.as_mut() {
            st.entries.push((revision, kind));
            return;
        }
        self.seq += 1;
        let e = JournalEntry {
            seq: self.seq,
            revision,
            style_epoch: self.epoch,
            kind,
        };
        if self.window.len() == WINDOW {
            let old = self.window.pop_front().expect("full");
            self.evicted_revision = Some(old.revision);
        }
        self.window.push_back(e.clone());
        self.segment.push(e);
    }

    /// Emit `prior_attributes` if this is the element's first attribute change of the epoch.
    pub(crate) fn note_prior_attributes(
        &mut self,
        revision: u64,
        node: NodeId,
        attributes: &[Attribute],
    ) {
        if self.prior_attrs_seen.insert(node) {
            self.push(
                revision,
                EntryKind::PriorAttributes {
                    node,
                    attributes: attributes.to_vec(),
                },
            );
        }
    }

    /// Emit `prior_control_flags` if this is the element's first selector-relevant change.
    pub(crate) fn note_control_flags(&mut self, revision: u64, node: NodeId, flags: ControlFlags) {
        if self.prior_control_seen.insert(node) {
            self.push(revision, EntryKind::PriorControlFlags { node, flags });
        }
    }

    /// Return the current segment and start the next style epoch.
    pub fn take_epoch(&mut self) -> EpochSegment {
        let seg = EpochSegment {
            epoch: self.epoch,
            entries: std::mem::take(&mut self.segment),
        };
        self.epoch += 1;
        self.prior_attrs_seen.clear();
        self.prior_control_seen.clear();
        seg
    }

    /// Current-epoch entries committed after `revision`, newest first. Unlike
    /// the bounded diagnostic window, this segment is not truncated. Callers
    /// must not hold the iterator across `take_epoch`.
    pub fn current_epoch_since(&self, revision: u64) -> impl Iterator<Item = &JournalEntry> {
        self.segment
            .iter()
            .rev()
            .take_while(move |e| e.revision > revision)
    }

    /// Entries committed after `since_revision` that are still in the window.
    pub fn since(&self, since_revision: u64) -> JournalWindow {
        let entries: Vec<_> = self
            .window
            .iter()
            .filter(|e| e.revision > since_revision)
            .cloned()
            .collect();
        JournalWindow {
            since_revision,
            truncated: self.evicted_revision.is_some_and(|r| r > since_revision),
            oldest_revision: self.window.front().map_or(0, |e| e.revision),
            entries,
        }
    }
}
