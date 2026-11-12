//! owner: 003-document-model
//!
//! Insertion (T047; FR-048–FR-054, FR-099, FR-100): the DOM insertion steps for already validated,
//! already detached nodes, run inside an open call. A moved node was removed by the caller in the
//! same call, so a move is one revision step (FR-054). Nodes inserted into template contents are
//! inert: no hooks, no notifications, no stylesheet changes (FR-015).

use crate::document::Document;
use crate::hooks::{Notification, RequesterChange, requester_kind};
use crate::journal::EntryKind;
use crate::stylesheets::is_stylesheet_source;
use ui4me_engine_types::ids::NodeId;

impl Document {
    /// Link `nodes` into `parent` before `before` (append when `None`), journal one `ChildList`
    /// entry, then run the insertion steps and the 012 hooks.
    pub(crate) fn insert_step(
        &mut self,
        rev: u64,
        parent: NodeId,
        nodes: Vec<NodeId>,
        before: Option<NodeId>,
    ) {
        self.insert_step_ex(rev, parent, nodes, before, vec![]);
    }

    /// As [`Document::insert_step`] with `removed` listed in the same `child_list` entry (replace all).
    pub(crate) fn insert_step_ex(
        &mut self,
        rev: u64,
        parent: NodeId,
        nodes: Vec<NodeId>,
        before: Option<NodeId>,
        removed: Vec<NodeId>,
    ) {
        if nodes.is_empty() {
            return;
        }
        let connected = self.arena.is_connected(parent);
        let in_template = self.in_template_contents(parent);
        let has_base_candidate = connected
            && !in_template
            && nodes.iter().any(|&r| {
                self.arena.subtree(r).into_iter().any(|n| {
                    self.arena
                        .node(n)
                        .as_element()
                        .is_some_and(|e| e.is_html_named("base") && e.attr("href").is_some())
                })
            });
        let base_before = has_base_candidate.then(|| self.frozen_base_href());

        let at = before
            .and_then(|b| {
                self.arena
                    .node(parent)
                    .children
                    .iter()
                    .position(|c| *c == b)
            })
            .unwrap_or(self.arena.node(parent).children.len());
        let (prev, next) = self.siblings_at(parent, at);
        for &n in &nodes {
            self.arena.insert_trusted(parent, n, before);
        }
        self.journal.push(
            rev,
            EntryKind::ChildList {
                parent,
                added: nodes.clone(),
                removed,
                prev_sibling: prev,
                next_sibling: next,
            },
        );
        if in_template {
            return;
        }

        // Form owners are re-evaluated for attached and detached insertions alike.
        self.form_owner_after_insert(&nodes);

        if connected {
            let mut style_changed = self.inside_style_element(parent);
            let mut notifs = vec![];
            for &r in &nodes {
                for n in self.arena.subtree(r) {
                    let Some(e) = self.arena.node(n).as_element() else {
                        continue;
                    };
                    if is_stylesheet_source(e) {
                        style_changed = true;
                    }
                    if let Some(kind) = requester_kind(e) {
                        notifs.push(Notification::RequesterChanged {
                            element: n,
                            kind,
                            change: RequesterChange::Connected,
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
        }

        for &n in &nodes {
            self.run_inserted_hook(n);
        }
        if self
            .arena
            .node(parent)
            .as_element()
            .is_some_and(crate::hooks::children_hook_applies)
        {
            self.run_children_changed_hook(parent);
            self.select_inserted(parent, &nodes);
        }
    }

    /// The frozen base URL text: the `href` of the first `base` element with an `href` attribute in
    /// tree order, or `None` (FR-010). Resolving against it is 007's.
    pub(crate) fn frozen_base_href(&self) -> Option<String> {
        self.arena
            .subtree(self.arena.document())
            .into_iter()
            .find_map(|n| {
                self.arena
                    .node(n)
                    .as_element()
                    .filter(|e| e.is_html_named("base"))
                    .and_then(|e| e.attr("href"))
                    .map(str::to_owned)
            })
    }
}
