//! owner: 001-vertical-slice — created by 001 slice (R-28)
//!
//! Revision tuple (core-types.md §2). 005 owns the tuple, fields and read-mode outcomes
//! (005 FR-015–FR-017); on any difference 005 wins.

use crate::ids::DocumentId;

/// Input epochs (monotonic; one writer each, AD-28).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RevisionInputs {
    /// DocumentCore: +1 per accepted mutation call or batch (incl. element/control state).
    pub dom: u64,
    /// Environment epoch: viewport, scale, media features.
    pub environment: u64,
    /// ResourceManager: image/resource epoch.
    pub resource: u64,
    /// ResourceManager: font epoch.
    pub font: u64,
    /// InteractionRuntime: ScrollState revision.
    pub scroll: u64,
    /// InvalidationScheduler: animation clock tick (AD-11).
    pub animation: u64,
}

/// The revision tuple carried by every derived read result (AD-05).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RevisionTuple {
    pub document: DocumentId,
    pub inputs: RevisionInputs,
    /// Output epochs, assigned by the InvalidationScheduler at atomic publication.
    pub style: u64,
    pub layout: u64,
    pub scene: u64,
}

impl RevisionTuple {
    /// Current iff the input fields equal the document's current inputs.
    pub fn is_current(&self, document: DocumentId, current_inputs: &RevisionInputs) -> bool {
        self.document == document && self.inputs == *current_inputs
    }
}

// ---- 005 T010: one increment API per writer, outputs at publication, `current` (R-37) ----------

/// The single writer of each input epoch (AD-28); every writer increments only its own field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InputWriter {
    /// DocumentCore: `dom`.
    DocumentCore,
    /// HostRuntime: `environment`.
    HostRuntime,
    /// ResourceManager: `resource` (once per completion batch).
    ResourceManager,
    /// ResourceManager: `font` (once per completion batch).
    FontManager,
    /// InteractionRuntime: `scroll`.
    InteractionRuntime,
    /// The scheduler clock: `animation`.
    SchedulerClock,
}

impl RevisionInputs {
    /// Increment the epoch `writer` owns and return its new value (monotonic: never decreases).
    pub fn bump(&mut self, writer: InputWriter) -> u64 {
        let field = match writer {
            InputWriter::DocumentCore => &mut self.dom,
            InputWriter::HostRuntime => &mut self.environment,
            InputWriter::ResourceManager => &mut self.resource,
            InputWriter::FontManager => &mut self.font,
            InputWriter::InteractionRuntime => &mut self.scroll,
            InputWriter::SchedulerClock => &mut self.animation,
        };
        *field += 1;
        *field
    }

    /// The epoch `writer` owns.
    pub fn get(&self, writer: InputWriter) -> u64 {
        match writer {
            InputWriter::DocumentCore => self.dom,
            InputWriter::HostRuntime => self.environment,
            InputWriter::ResourceManager => self.resource,
            InputWriter::FontManager => self.font,
            InputWriter::InteractionRuntime => self.scroll,
            InputWriter::SchedulerClock => self.animation,
        }
    }
}

/// Which output epochs a publication changed (recompute-plan §4 step 9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChangedOutputs {
    pub style: bool,
    pub layout: bool,
    pub scene: bool,
}

impl RevisionTuple {
    /// The tuple a publication of `inputs` produces after `self`: the output epochs of changed
    /// outputs increment, the others carry over.
    pub fn published(&self, inputs: RevisionInputs, changed: ChangedOutputs) -> RevisionTuple {
        RevisionTuple {
            document: self.document,
            inputs,
            style: self.style + u64::from(changed.style),
            layout: self.layout + u64::from(changed.layout),
            scene: self.scene + u64::from(changed.scene),
        }
    }

    /// This tuple as returned by a read at the instant the document's inputs were
    /// `current_inputs` (`current ⇔ inputs == document.current_inputs`).
    pub fn stamped(&self, current_inputs: &RevisionInputs) -> RevisionRead {
        RevisionRead {
            tuple: *self,
            current: self.inputs == *current_inputs,
        }
    }
}

/// The tuple plus its `current` flag, as every derived read result carries it (AD-05; FR-016).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RevisionRead {
    pub tuple: RevisionTuple,
    pub current: bool,
}
