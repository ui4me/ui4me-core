//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! StyleSidecar store (T041; AD-07 storage clause, unchanged by AD-P-001-y(a)). Ported from the
//! X-STYLE-ADAPTER harness (`verification/gates/X-STYLE-ADAPTER/harness/src/sidecar.rs`,
//! RESULT-2 PASS) onto DocumentCore's arena.
//!
//! One `StyleSidecar` per element (and Document) slot, owned by StyleBridge, never by
//! DocumentCore:
//!
//! * **Address-stable:** each sidecar is a separate `Box`; the `Vec<Option<Box<_>>>` changes
//!   only through `&mut SidecarStore` ([`SidecarStore::sync`]), i.e. never while a style pass
//!   holds `&SidecarStore`; growing moves the boxes, not their contents.
//! * **Interior-mutable:** every field Stylo writes through `&self` during a traversal is an
//!   atomic or Stylo's own `ElementDataWrapper` (whose debug build carries an `AtomicRefCell`
//!   borrow check). There is no `UnsafeCell` in this file.
//! * **Sealed (review F1):** every `ElementData` accessor, the store's lookups and `sync` are
//!   `pub(crate)`; outside callers only ever receive values (`Arc<ComputedValues>`, keys).
//! * **Generation-checked:** a sidecar records the generation of the node it belongs to; `sync`
//!   resets a sidecar whose slot was reused, so no ElementData, snapshot flag or selector flag
//!   is ever inherited by a new node.

use selectors::matching::ElementSelectorFlags;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use style::Atom;
use style::data::{ElementData, ElementDataMut, ElementDataRef, ElementDataWrapper};
use style::properties::PropertyDeclarationBlock;
use style::servo_arc::Arc;
use style::shared_lock::{Locked, SharedRwLock};
use ui4me_document::arena::Arena;
use ui4me_document::node::NodeData;
use ui4me_engine_types::ids::NodeId;

pub struct StyleSidecar {
    pub(crate) generation: NonZeroU32,
    data: ElementDataWrapper,
    has_data: AtomicBool,
    pub(crate) dirty_descendants: AtomicBool,
    pub(crate) has_snapshot: AtomicBool,
    pub(crate) handled_snapshot: AtomicBool,
    selector_flags: AtomicU32,
    pub(crate) may_have_animations: AtomicBool,
    /// Parsed `style=""` block (written only by `sync`, under `&mut`).
    pub(crate) style_attribute: Option<Arc<Locked<PropertyDeclarationBlock>>>,
    /// The `style` attribute text the block was parsed from.
    pub(crate) style_text: Option<String>,
    /// Atoms for `id` / `class` (written only by `sync`).
    pub(crate) id_atom: Option<Atom>,
    pub(crate) class_atoms: Vec<Atom>,
}

impl StyleSidecar {
    fn fresh(generation: NonZeroU32) -> Self {
        StyleSidecar {
            generation,
            data: ElementDataWrapper::default(),
            has_data: AtomicBool::new(false),
            dirty_descendants: AtomicBool::new(false),
            has_snapshot: AtomicBool::new(false),
            handled_snapshot: AtomicBool::new(false),
            selector_flags: AtomicU32::new(0),
            may_have_animations: AtomicBool::new(false),
            style_attribute: None,
            style_text: None,
            id_atom: None,
            class_atoms: vec![],
        }
    }

    pub(crate) fn has_data(&self) -> bool {
        self.has_data.load(Ordering::Acquire)
    }

    /// Stylo `TElement::ensure_data` body. Exclusivity is Stylo's traversal contract; in debug
    /// builds `ElementDataWrapper::borrow_mut` panics on any overlapping borrow.
    pub(crate) fn ensure_data(&self) -> ElementDataMut<'_> {
        self.has_data.store(true, Ordering::Release);
        self.data.borrow_mut()
    }

    /// Stylo `TElement::clear_data` body: drop styles, return to "no data".
    pub(crate) fn clear_data(&self) {
        *self.data.borrow_mut() = ElementData::default();
        self.has_data.store(false, Ordering::Release);
    }

    pub(crate) fn borrow_data(&self) -> Option<ElementDataRef<'_>> {
        self.has_data().then(|| self.data.borrow())
    }

    pub(crate) fn mutate_data(&self) -> Option<ElementDataMut<'_>> {
        self.has_data().then(|| self.data.borrow_mut())
    }

    pub(crate) fn selector_flags(&self) -> ElementSelectorFlags {
        ElementSelectorFlags::from_bits_retain(self.selector_flags.load(Ordering::Relaxed) as _)
    }

    pub(crate) fn insert_selector_flags(&self, f: ElementSelectorFlags) {
        self.selector_flags
            .fetch_or(f.bits() as u32, Ordering::Relaxed);
    }
}

pub struct SidecarStore {
    slots: Vec<Option<Box<StyleSidecar>>>,
    /// The document's style lock (guards `style=""` blocks and sheets).
    pub(crate) lock: SharedRwLock,
    /// The query-container sizes the style passes read (`container.rs`; 015 CQ-3). Changed only
    /// through `&mut SidecarStore`, between passes.
    pub(crate) containers: crate::style::container::ContainerSizes,
}

#[derive(Default, Debug)]
pub(crate) struct SyncReport {
    /// Sidecars allocated for a slot seen for the first time.
    pub(crate) created: Vec<NodeId>,
    /// Sidecars reset because their slot now holds a newer generation.
    pub(crate) reset_for_reuse: Vec<NodeId>,
    /// Elements whose `style` attribute text changed (re-parsed).
    pub(crate) style_attribute_changed: Vec<NodeId>,
    pub(crate) dropped: usize,
}

impl SidecarStore {
    pub(crate) fn new(lock: SharedRwLock) -> Self {
        SidecarStore {
            slots: vec![],
            lock,
            containers: Default::default(),
        }
    }

    /// Generation-checked lookup. `None` for a stale id or an unsynced slot.
    pub(crate) fn get(&self, id: NodeId) -> Option<&StyleSidecar> {
        let s = self.slots.get(id.raw().index as usize)?.as_deref()?;
        (s.generation == id.raw().generation).then_some(s)
    }

    /// Panicking accessor used by the traversal (invariant: `sync` ran before it).
    pub(crate) fn of(&self, id: NodeId) -> &StyleSidecar {
        self.get(id)
            .unwrap_or_else(|| panic!("no sidecar for {id:?} (sync not run, or stale id)"))
    }

    /// Bring the store in line with the arena (only under `&mut`, never during a pass):
    /// allocate sidecars for new element slots, **reset** sidecars whose slot was reused by a
    /// newer generation, drop sidecars of freed slots, refresh id/class atoms, and re-parse a
    /// changed `style=""` value with `parse_style`.
    pub(crate) fn sync(
        &mut self,
        dom: &Arena,
        mut parse_style: impl FnMut(NodeId, &str) -> Arc<Locked<PropertyDeclarationBlock>>,
    ) -> SyncReport {
        let mut report = SyncReport::default();
        self.slots.resize_with(dom.slot_count(), || None);
        for i in 0..dom.slot_count() {
            let live = dom.id_at(i).filter(|&id| {
                matches!(
                    dom.node(id).data(),
                    NodeData::Element(_) | NodeData::Document
                )
            });
            match (live, &mut self.slots[i]) {
                (None, slot) => {
                    if slot.take().is_some() {
                        report.dropped += 1;
                    }
                }
                (Some(id), slot) => {
                    let reset = match slot {
                        Some(s) if s.generation == id.raw().generation => false,
                        Some(_) => {
                            report.reset_for_reuse.push(id);
                            true
                        }
                        None => {
                            report.created.push(id);
                            true
                        }
                    };
                    if reset {
                        *slot = Some(Box::new(StyleSidecar::fresh(id.raw().generation)));
                    }
                    let s = slot.as_mut().expect("just filled");
                    let Some(e) = dom.node(id).as_element() else {
                        continue;
                    };
                    s.id_atom = e.attr("id").map(Atom::from);
                    s.class_atoms = e.classes().map(Atom::from).collect();
                    let text = e.attr("style");
                    if text != s.style_text.as_deref() {
                        s.style_text = text.map(str::to_owned);
                        s.style_attribute = text.map(|t| parse_style(id, t));
                        if !reset {
                            report.style_attribute_changed.push(id);
                        }
                    }
                }
            }
        }
        report
    }
}
