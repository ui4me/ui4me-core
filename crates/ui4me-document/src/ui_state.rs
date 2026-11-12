//! owner: 003-document-model — created by 001 slice (R-28)
//!
//! Element UI-state table (T035; AD-28; R-38): `{focus, focus-visible, focus-within, hover,
//! active, picker-open, modal}` per element, stored by DocumentCore and written **only**
//! through InteractionRuntime's [`UiStateWriter`] (one capability key per document). Every
//! write is journaled with old/new flags and steps the DOM revision (FR-079–080); writes made
//! from inside a DocumentCore hook belong to the hooked call's single revision step.

use crate::document::Document;
use crate::journal::EntryKind;
use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign, Not};
use ui4me_engine_types::codes_generated::{Code, SubReason};
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct UiFlags(u8);

impl UiFlags {
    pub const FOCUS: UiFlags = UiFlags(1 << 0);
    pub const FOCUS_VISIBLE: UiFlags = UiFlags(1 << 1);
    pub const FOCUS_WITHIN: UiFlags = UiFlags(1 << 2);
    pub const HOVER: UiFlags = UiFlags(1 << 3);
    pub const ACTIVE: UiFlags = UiFlags(1 << 4);
    pub const PICKER_OPEN: UiFlags = UiFlags(1 << 5);
    pub const MODAL: UiFlags = UiFlags(1 << 6);
    /// In the document's top layer (015 HTML-1; CSS Position 4 section 3): a modal `dialog`. The order of
    /// the top layer is [`Document::top_layer`]; box generation places these elements (and their
    /// `::backdrop`) as children of the root, painted above everything.
    pub const TOP_LAYER: UiFlags = UiFlags(1 << 7);
    const ALL: [(UiFlags, &'static str); 8] = [
        (UiFlags::FOCUS, "focus"),
        (UiFlags::FOCUS_VISIBLE, "focus-visible"),
        (UiFlags::FOCUS_WITHIN, "focus-within"),
        (UiFlags::HOVER, "hover"),
        (UiFlags::ACTIVE, "active"),
        (UiFlags::PICKER_OPEN, "picker-open"),
        (UiFlags::MODAL, "modal"),
        (UiFlags::TOP_LAYER, "top-layer"),
    ];

    pub const fn empty() -> UiFlags {
        UiFlags(0)
    }
    pub const fn bits(self) -> u8 {
        self.0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub const fn contains(self, other: UiFlags) -> bool {
        self.0 & other.0 == other.0
    }
    pub const fn intersects(self, other: UiFlags) -> bool {
        self.0 & other.0 != 0
    }
    /// Flag names in 005's vocabulary (`dom.journal` `ui_state` entries).
    pub fn names(self) -> Vec<&'static str> {
        Self::ALL
            .iter()
            .filter(|(f, _)| self.contains(*f))
            .map(|(_, n)| *n)
            .collect()
    }
}

impl BitOr for UiFlags {
    type Output = UiFlags;
    fn bitor(self, r: UiFlags) -> UiFlags {
        UiFlags(self.0 | r.0)
    }
}
impl BitOrAssign for UiFlags {
    fn bitor_assign(&mut self, r: UiFlags) {
        self.0 |= r.0
    }
}
impl BitAnd for UiFlags {
    type Output = UiFlags;
    fn bitand(self, r: UiFlags) -> UiFlags {
        UiFlags(self.0 & r.0)
    }
}
impl Not for UiFlags {
    type Output = UiFlags;
    fn not(self) -> UiFlags {
        UiFlags(!self.0)
    }
}
impl fmt::Debug for UiFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UiFlags({})", self.names().join("|"))
    }
}

/// Capability key for the document's single UI-state writer (taken by InteractionRuntime).
/// Not `Clone`: possession is the authority.
#[derive(Debug)]
pub struct UiStateKey {
    pub(crate) document: DocumentId,
}

/// Writer over the element-state table. Obtained from [`Document::ui_state_writer`] with the
/// document's key, or handed to a removal hook (hooks.rs) during the removing call.
pub struct UiStateWriter<'a> {
    pub(crate) doc: &'a mut Document,
    /// Inside a hooked call: the call itself steps the revision once.
    pub(crate) in_call: bool,
}

impl UiStateWriter<'_> {
    /// Replace an element's flags. Journals `ui_state {old, new}` and steps the revision (unless
    /// written from inside a hooked call). No change → no mutation.
    pub fn set_flags(&mut self, element: NodeId, flags: UiFlags) -> Result<(), Status> {
        self.doc.check_writable(element)?;
        let old = self
            .doc
            .arena
            .get(element)
            .and_then(|n| n.as_element())
            .map(|e| e.ui)
            .ok_or_else(|| {
                crate::document::err(Code::InvalidArgument)
                    .with_sub_reason(SubReason::WrongNodeKind)
            })?;
        if old == flags {
            return Ok(());
        }
        // Invariant 9: no UI flags on detached elements (the removal hook clears them, FR-081); a
        // non-empty write to a detached element is a caller bug and is refused.
        if !flags.is_empty() && !self.doc.arena.is_connected(element) {
            return Err(crate::errors::invalid_state());
        }
        if !self.in_call {
            self.doc.begin_call();
        }
        self.doc
            .arena
            .get_mut(element)
            .and_then(|n| n.as_element_mut())
            .expect("checked")
            .ui = flags;
        // The top layer keeps insertion order: an element entering it goes last (015 HTML-1).
        if old.contains(UiFlags::TOP_LAYER) != flags.contains(UiFlags::TOP_LAYER) {
            self.doc.top_layer.retain(|n| *n != element);
            if flags.contains(UiFlags::TOP_LAYER) {
                self.doc.top_layer.push(element);
            }
        }
        let rev = self.doc.pending_revision();
        self.doc.journal.push(
            rev,
            EntryKind::UiState {
                node: element,
                old,
                new: flags,
            },
        );
        if !self.in_call {
            self.doc.end_call();
        }
        Ok(())
    }

    pub fn flags(&self, element: NodeId) -> Result<UiFlags, Status> {
        self.doc.ui_flags(element)
    }

    /// Elements in `root`'s inclusive subtree whose flags intersect `mask`, in tree order.
    pub fn flagged_in_subtree(&self, root: NodeId, mask: UiFlags) -> Vec<NodeId> {
        self.doc
            .arena
            .subtree(root)
            .into_iter()
            .filter(|&n| {
                self.doc
                    .arena
                    .get(n)
                    .and_then(|x| x.as_element())
                    .is_some_and(|e| e.ui.intersects(mask))
            })
            .collect()
    }

    /// Read access to the document (tree navigation inside a hook).
    pub fn document(&self) -> &Document {
        self.doc
    }
}
