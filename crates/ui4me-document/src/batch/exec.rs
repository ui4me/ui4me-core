//! owner: 003-document-model
//!
//! Batch execution (T079; FR-071-FR-075; research R-10). Each command runs through the same
//! internal function as the single call, inside one outer call: journal entries are staged, the arena
//! records before-images (the undo log, see `Arena::begin_undo`), requester notifications are queued.
//! On success the batch commits with exactly one revision step and returns one handle per batch-local
//! id, each holding one host reference. On any failure everything is rolled back: tree, attributes,
//! text, control state, UI-state (written by hooks through the same arena), journal, revisions and
//! the stylesheet revision are as before; nodes created by the batch are freed; queued notifications
//! are dropped; the error carries `batch_index`.

use crate::batch::decode::{Batch, Cmd, Operand, decode};
use crate::document::Document;
use crate::errors::{wrong_document, wrong_node_kind};
use crate::ids::{from_opaque, to_opaque};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::{DocumentId, NodeId};
use ui4me_engine_types::status::Status;

/// The opaque 64-bit word of a document id used in batch buffers: `generation << 32 | index`.
pub fn document_word(id: DocumentId) -> u64 {
    let r = id.raw();
    (u64::from(r.generation.get()) << 32) | u64::from(r.index)
}

/// The batch-buffer node word of `n` (the opaque `generation << 32 | index` form).
pub fn node_word(n: NodeId) -> u64 {
    to_opaque(n).get()
}

#[derive(Clone, Copy)]
enum Resolved {
    Null,
    Node(NodeId),
}

impl Document {
    /// `ui4me_batch_apply`'s DocumentCore half: decode, apply atomically, return the created nodes
    /// (one per batch-local id, in id order) each with one host reference.
    pub fn batch_apply(&mut self, buf: &[u8]) -> Result<Vec<NodeId>, Status> {
        self.check_open()?;
        let batch = decode(buf, &self.limits)?;
        self.apply_batch(&batch)
    }

    /// Apply an already decoded batch.
    pub fn apply_batch(&mut self, batch: &Batch) -> Result<Vec<NodeId>, Status> {
        self.check_open()?;
        // Capacity of the output array is the caller's; nothing is applied before it is known.
        self.begin_batch();
        let mut locals: Vec<NodeId> = Vec::with_capacity(batch.local_count as usize);
        for (i, cmd) in batch.cmds.iter().enumerate() {
            if let Err(e) = self.exec_cmd(cmd, &mut locals) {
                self.rollback_batch();
                return Err(e.with_batch_index(i as u32));
            }
        }
        self.commit_batch();
        for &l in &locals {
            // Retained after the commit: the created nodes' host references are part of the result.
            if let Some(n) = self.arena.get_mut(l) {
                n.host_refs = n.host_refs.saturating_add(1);
            }
        }
        Ok(locals)
    }

    /// Run host-composed mutations as one atomic batch: one revision step on success; on `Err`
    /// the tree, attributes, text, control/UI state, journal and revisions are rolled back exactly
    /// as for [`Self::apply_batch`] and nodes created inside are freed. Batches do not nest: inside
    /// an active batch `f` runs as part of the outer one.
    pub fn transaction<R, E>(&mut self, f: impl FnOnce(&mut Self) -> Result<R, E>) -> Result<R, E>
    where
        E: From<Status>,
    {
        self.check_open()?;
        if self.batch_active {
            return f(self);
        }
        self.begin_batch();
        match f(self) {
            Ok(value) => {
                self.commit_batch();
                Ok(value)
            }
            Err(error) => {
                self.rollback_batch();
                Err(error)
            }
        }
    }

    fn begin_batch(&mut self) {
        self.batch_active = true;
        self.arena.begin_undo();
        self.journal.begin_stage();
        self.batch_saved = Some((self.stylesheet_revision, self.notif_queue.len()));
        self.begin_call();
    }

    fn commit_batch(&mut self) {
        self.arena.commit_undo();
        self.journal.commit_stage();
        self.batch_active = false;
        self.batch_saved = None;
        self.end_call();
    }

    fn rollback_batch(&mut self) {
        self.arena.rollback_undo();
        self.journal.rollback_stage();
        if let Some((sheet_rev, notifs)) = self.batch_saved.take() {
            self.stylesheet_revision = sheet_rev;
            self.notif_queue.truncate(notifs);
        }
        self.batch_active = false;
        self.sheet_noted_rev = None;
        self.abort_call();
    }

    /// Resolve the NODE operands of one command with the fixed precedence: a stale own-document
    /// handle (parameter order) first, then a handle of another document, then the lifecycle.
    fn resolve_ops(&self, ops: &[Operand], locals: &[NodeId]) -> Result<Vec<Resolved>, Status> {
        let mut out = vec![];
        let mut foreign = false;
        let mut stale = false;
        for op in ops {
            match *op {
                Operand::Null => out.push(Resolved::Null),
                Operand::Local(l) => match locals.get(l as usize) {
                    Some(&n) => out.push(Resolved::Node(n)),
                    None => stale = true,
                },
                Operand::Existing(d, n) => {
                    if d != document_word(self.id) {
                        foreign = true;
                        continue;
                    }
                    match std::num::NonZeroU64::new(n).and_then(from_opaque) {
                        Some(id) if self.arena.contains(id) => out.push(Resolved::Node(id)),
                        _ => {
                            stale = true;
                        }
                    }
                }
            }
            if stale {
                return Err(Status::new(
                    Code::StaleHandle,
                    ui4me_engine_types::stage::Stage::Host,
                ));
            }
        }
        if foreign {
            return Err(wrong_document());
        }
        self.check_open()?;
        Ok(out)
    }

    fn one(&self, op: Operand, locals: &[NodeId]) -> Result<NodeId, Status> {
        match self.resolve_ops(&[op], locals)?.remove(0) {
            Resolved::Node(n) => Ok(n),
            Resolved::Null => Err(wrong_node_kind()),
        }
    }

    fn exec_cmd(&mut self, cmd: &Cmd, locals: &mut Vec<NodeId>) -> Result<(), Status> {
        match cmd {
            Cmd::CreateElement { ns, qname } => {
                let n = match ns {
                    None => self.create_element(qname)?,
                    Some(_) => self.create_element_ns(ns.as_deref(), qname)?,
                };
                locals.push(n);
            }
            Cmd::CreateText { data } => locals.push(self.create_text_node(data)?),
            Cmd::CreateComment { data } => locals.push(self.create_comment(data)?),
            Cmd::CreateFragment => locals.push(self.create_document_fragment()?),
            Cmd::Clone { node, deep } => {
                let n = self.one(*node, locals)?;
                locals.push(self.clone_node(n, *deep)?);
            }
            Cmd::AppendChild { parent, child } => {
                let r = self.resolve_ops(&[*parent, *child], locals)?;
                if let [Resolved::Node(p), Resolved::Node(c)] = r[..] {
                    self.append_child(p, c)?;
                }
            }
            Cmd::InsertBefore {
                parent,
                child,
                reference,
            } => {
                let r = self.resolve_ops(&[*parent, *child, *reference], locals)?;
                if let [Resolved::Node(p), Resolved::Node(c), rf] = r[..] {
                    let rf = match rf {
                        Resolved::Node(n) => Some(n),
                        Resolved::Null => None,
                    };
                    self.insert_before(p, c, rf)?;
                }
            }
            Cmd::Remove { node } => {
                let n = self.one(*node, locals)?;
                self.remove(n)?;
            }
            Cmd::ReplaceChild { parent, new, old } => {
                let r = self.resolve_ops(&[*parent, *new, *old], locals)?;
                if let [Resolved::Node(p), Resolved::Node(n), Resolved::Node(o)] = r[..] {
                    self.replace_child(p, n, o)?;
                }
            }
            Cmd::SetAttribute {
                el,
                ns,
                qname,
                value,
            } => {
                let e = self.one(*el, locals)?;
                match ns {
                    None => self.set_attribute(e, qname, value)?,
                    Some(_) => self.set_attribute_ns(e, ns.as_deref(), qname, value)?,
                }
            }
            Cmd::RemoveAttribute { el, ns, name } => {
                let e = self.one(*el, locals)?;
                match ns {
                    None => self.remove_attribute(e, name)?,
                    Some(_) => self.remove_attribute_ns(e, ns.as_deref(), name)?,
                }
            }
            Cmd::ToggleAttribute { el, qname, force } => {
                let e = self.one(*el, locals)?;
                self.toggle_attribute(e, qname, *force)?;
            }
            Cmd::ClassAdd { el, token } => {
                let e = self.one(*el, locals)?;
                self.class_list_add(e, &[token.as_str()])?;
            }
            Cmd::ClassRemove { el, token } => {
                let e = self.one(*el, locals)?;
                self.class_list_remove(e, &[token.as_str()])?;
            }
            Cmd::ClassToggle { el, token, force } => {
                let e = self.one(*el, locals)?;
                self.class_list_toggle(e, token, *force)?;
            }
            Cmd::SetTextContent { node, text } => {
                let n = self.one(*node, locals)?;
                self.set_text_content(n, text.as_deref().unwrap_or(""))?;
            }
            Cmd::SetData { node, data } => {
                let n = self.one(*node, locals)?;
                self.set_data(n, data)?;
            }
            Cmd::SetInnerHtml { el, html } => {
                let e = self.one(*el, locals)?;
                self.set_inner_html(e, html)?;
            }
        }
        Ok(())
    }
}
