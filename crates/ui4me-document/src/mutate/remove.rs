//! owner: 003-document-model
//!
//! Removal (T048; FR-028, FR-081, FR-099; R-14): the DOM removing steps for children of one parent,
//! run inside an open call. Order: effects computed while the nodes are still attached, detach,
//! journal, stylesheet and base-URL bookkeeping, requester notifications (staged), form-owner reset,
//! then the 012 removal hook per removed root *inside the call* so focus fix-up and flag clearing
//! complete before the call returns. Nodes inside template contents are inert: no hooks, no
//! notifications (FR-015).

use crate::document::Document;
use crate::hooks::{Notification, Removal, RequesterChange, requester_kind};
use crate::journal::EntryKind;
use crate::stylesheets::is_stylesheet_source;
use ui4me_engine_types::ids::NodeId;

impl Document {
    /// Remove `nodes` (children of `parent`), in order, as one journaled step.
    pub(crate) fn remove_nodes_step(&mut self, rev: u64, parent: NodeId, nodes: &[NodeId]) {
        self.remove_nodes_ex(rev, parent, nodes, true);
    }

    /// As [`Document::remove_nodes_step`]; `journal_it = false` leaves the `child_list` entry to the
    /// caller (a replace-all merges removal and insertion into one entry, like DOM "replace all").
    pub(crate) fn remove_nodes_ex(
        &mut self,
        rev: u64,
        parent: NodeId,
        nodes: &[NodeId],
        journal_it: bool,
    ) {
        if nodes.is_empty() {
            return;
        }
        let connected = self.arena.is_connected(parent);
        let in_template = self.in_template_contents(parent);
        let kids = self.arena.node(parent).children.clone();
        let first_i = kids
            .iter()
            .position(|c| *c == nodes[0])
            .expect("child of parent");
        let last_i = kids
            .iter()
            .position(|c| *c == *nodes.last().expect("non-empty"))
            .expect("child of parent");
        let prev_of_first = first_i.checked_sub(1).map(|i| kids[i]);
        let next_of_last = kids.get(last_i + 1).copied();

        // Effects that must be read while the subtree is still attached.
        let mut notifs: Vec<Notification> = vec![];
        // Text added to / removed from a connected `style` element changes the sheet text.
        let mut style_changed = connected && !in_template && self.inside_style_element(parent);
        let mut base_before = None;
        if connected && !in_template {
            let mut has_base = false;
            for &r in nodes {
                for n in self.arena.subtree(r) {
                    let Some(e) = self.arena.node(n).as_element() else {
                        continue;
                    };
                    if is_stylesheet_source(e) {
                        style_changed = true;
                    }
                    if e.is_html_named("base") && e.attr("href").is_some() {
                        has_base = true;
                    }
                    if let Some(kind) = requester_kind(e) {
                        notifs.push(Notification::RequesterChanged {
                            element: n,
                            kind,
                            change: RequesterChange::Disconnected,
                        });
                    }
                    if e.is_html_named("link") && is_stylesheet_source(e) {
                        notifs.push(Notification::StylesheetLinkChanged {
                            element: n,
                            href: e.attr("href").map(str::to_owned),
                        });
                    }
                }
            }
            if has_base {
                base_before = Some(self.frozen_base_href());
            }
        }

        // Per-root sibling data as each removal happens in order.
        let mut removals = vec![];
        for &n in nodes {
            let cur = &self.arena.node(parent).children;
            let i = cur.iter().position(|c| *c == n).expect("child of parent");
            let prev = i.checked_sub(1).map(|j| cur[j]);
            let next = cur.get(i + 1).copied();
            self.arena.detach(n);
            removals.push(Removal {
                root: n,
                old_parent: parent,
                old_prev_sibling: prev,
                old_next_sibling: next,
            });
        }
        if journal_it {
            self.journal.push(
                rev,
                EntryKind::ChildList {
                    parent,
                    added: vec![],
                    removed: nodes.to_vec(),
                    prev_sibling: prev_of_first,
                    next_sibling: next_of_last,
                },
            );
        }
        if in_template {
            return;
        }
        if style_changed {
            self.note_stylesheets_changed(rev);
        }
        if let Some(before) = base_before {
            let after = self.frozen_base_href();
            if after != before {
                notifs.push(Notification::BaseUrlChanged { url: after });
            }
        }
        for n in notifs {
            self.queue_notification(n);
        }
        for &n in nodes {
            self.form_owner_after_remove(n);
        }
        for r in removals {
            self.run_removed_hook(r);
        }
        if self
            .arena
            .node(parent)
            .as_element()
            .is_some_and(crate::hooks::children_hook_applies)
        {
            self.run_children_changed_hook(parent);
            self.select_follow(parent);
        }
    }
}
