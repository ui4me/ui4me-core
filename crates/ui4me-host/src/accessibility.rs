//! Platform accessibility adapters over a host-owned, synchronously available
//! tree. Accessibility callbacks never call the language backend or wait on IPC.
#[cfg(target_os = "linux")]
use accesskit::DeactivationHandler;
use accesskit::{
    ActionData, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, TreeId, TreeUpdate,
};
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc::SyncSender, Arc, Mutex};
use winit::event::WindowEvent;
use winit::window::Window;

pub struct ActionEvent {
    pub window_id: String,
    /// The document visible when the native callback was received. The owner
    /// thread rejects events queued before a document replacement or unload.
    pub document_id: Option<String>,
    pub request: ActionRequest,
}

/// Native object identities belong to the window, not to a renderer document.
/// A renderer may reuse local IDs in a fresh document; assistive technologies
/// can retain old objects, so those IDs must never identify new native objects.
/// The window root is the one stable object across all document/probe states.
const NATIVE_ROOT: NodeId = NodeId(1);

struct TreeState {
    full: TreeUpdate,
    document_id: Option<String>,
    local_root: NodeId,
    next_native: u64,
    forward: HashMap<NodeId, NodeId>,
    reverse: HashMap<NodeId, NodeId>,
}

impl TreeState {
    fn new(tree: TreeUpdate) -> anyhow::Result<Self> {
        let mut state = Self {
            full: TreeUpdate {
                nodes: Vec::new(),
                tree: None,
                tree_id: TreeId::ROOT,
                focus: NATIVE_ROOT,
            },
            document_id: None,
            local_root: NATIVE_ROOT,
            next_native: 2,
            forward: HashMap::new(),
            reverse: HashMap::new(),
        };
        state.replace(None, tree)?;
        Ok(state)
    }

    /// Accept a complete local tree; retain a complete synchronous activation
    /// cache, but only return native nodes that actually changed for publication.
    fn replace(
        &mut self,
        document_id: Option<&str>,
        mut full: TreeUpdate,
    ) -> anyhow::Result<Option<TreeUpdate>> {
        let root = full
            .tree
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Accessibility refresh requires a full tree"))?
            .root;
        if full.tree_id != TreeId::ROOT {
            anyhow::bail!("Hosted accessibility requires the root tree");
        }
        let live: HashSet<NodeId> = full.nodes.iter().map(|(id, _)| *id).collect();
        if live.len() != full.nodes.len() || !live.contains(&root) || !live.contains(&full.focus) {
            anyhow::bail!("Accessibility tree contains duplicate or missing identities");
        }
        if full
            .nodes
            .iter()
            .any(|(_, node)| references(node).any(|id| !live.contains(&id)))
        {
            anyhow::bail!("Accessibility tree refers to a missing node");
        }
        let replaced = self.document_id.as_deref() != document_id || self.local_root != root;
        let new_count = live
            .iter()
            .filter(|id| **id != root && (replaced || !self.forward.contains_key(id)))
            .count() as u64;
        self.next_native
            .checked_add(new_count)
            .ok_or_else(|| anyhow::anyhow!("Accessibility native identity space exhausted"))?;
        // All validation happens before changing the mapping or cached tree.
        if replaced {
            self.forward.clear();
            self.reverse.clear();
        } else {
            self.forward.retain(|local, _| live.contains(local));
            self.reverse.retain(|_, local| live.contains(local));
        }
        self.forward.insert(root, NATIVE_ROOT);
        self.reverse.insert(NATIVE_ROOT, root);
        for (local, _) in &full.nodes {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.forward.entry(*local) {
                let native = NodeId(self.next_native);
                self.next_native += 1;
                entry.insert(native);
                self.reverse.insert(native, *local);
            }
        }
        for (id, node) in &mut full.nodes {
            *id = self.forward[id];
            remap_node(node, &self.forward);
        }
        full.tree.as_mut().expect("validated full tree").root = NATIVE_ROOT;
        full.focus = self.forward[&full.focus];
        let previous: HashMap<NodeId, &Node> = self
            .full
            .nodes
            .iter()
            .map(|(id, node)| (*id, node))
            .collect();
        let nodes: Vec<_> = full
            .nodes
            .iter()
            .filter(|(id, node)| previous.get(id).is_none_or(|old| *old != node))
            .cloned()
            .collect();
        let tree_changed = self.full.tree != full.tree;
        let changed = !nodes.is_empty() || self.full.focus != full.focus || tree_changed;
        let delta = changed.then(|| TreeUpdate {
            nodes,
            tree: tree_changed.then(|| full.tree.clone()).flatten(),
            tree_id: full.tree_id,
            focus: full.focus,
        });
        self.full = full;
        self.document_id = document_id.map(str::to_owned);
        self.local_root = root;
        Ok(delta)
    }

    fn translate_action(&self, mut request: ActionRequest) -> Option<ActionRequest> {
        if request.target_tree != self.full.tree_id {
            return None;
        }
        request.target_node = *self.reverse.get(&request.target_node)?;
        if let Some(ActionData::SetTextSelection(selection)) = request.data.as_mut() {
            selection.anchor.node = *self.reverse.get(&selection.anchor.node)?;
            selection.focus.node = *self.reverse.get(&selection.focus.node)?;
        }
        Some(request)
    }
}

/// Keep every AccessKit node reference in the same native identity space.
/// Current renderer trees use children and text selections; relation references
/// are remapped too so adding an ARIA relation cannot accidentally alias an ID.
fn references(node: &Node) -> impl Iterator<Item = NodeId> + '_ {
    node.children()
        .iter()
        .chain(node.controls())
        .chain(node.details())
        .chain(node.described_by())
        .chain(node.flow_to())
        .chain(node.labelled_by())
        .chain(node.owns())
        .chain(node.radio_group())
        .copied()
        .chain(
            [
                node.active_descendant(),
                node.error_message(),
                node.in_page_link_target(),
                node.member_of(),
                node.next_on_line(),
                node.previous_on_line(),
                node.popup_for(),
            ]
            .into_iter()
            .flatten(),
        )
        .chain(
            node.text_selection()
                .into_iter()
                .flat_map(|selection| [selection.anchor.node, selection.focus.node]),
        )
}

fn remap_node(node: &mut Node, map: &HashMap<NodeId, NodeId>) {
    macro_rules! list {
        ($getter:ident, $setter:ident) => {
            if !node.$getter().is_empty() {
                let ids: Vec<_> = node.$getter().iter().map(|id| map[id]).collect();
                node.$setter(ids);
            }
        };
    }
    macro_rules! single {
        ($getter:ident, $setter:ident) => {
            if let Some(id) = node.$getter() {
                node.$setter(map[&id]);
            }
        };
    }
    list!(children, set_children);
    list!(controls, set_controls);
    list!(details, set_details);
    list!(described_by, set_described_by);
    list!(flow_to, set_flow_to);
    list!(labelled_by, set_labelled_by);
    list!(owns, set_owns);
    list!(radio_group, set_radio_group);
    single!(active_descendant, set_active_descendant);
    single!(error_message, set_error_message);
    single!(in_page_link_target, set_in_page_link_target);
    single!(member_of, set_member_of);
    single!(next_on_line, set_next_on_line);
    single!(previous_on_line, set_previous_on_line);
    single!(popup_for, set_popup_for);
    if let Some(mut selection) = node.text_selection().copied() {
        selection.anchor.node = map[&selection.anchor.node];
        selection.focus.node = map[&selection.focus.node];
        node.set_text_selection(selection);
    }
}

struct Activation(Arc<Mutex<TreeState>>);
impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .full
                .clone(),
        )
    }
}
struct Actions {
    window_id: String,
    state: Arc<Mutex<TreeState>>,
    sender: SyncSender<ActionEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        let (document_id, request) = {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(request) = state.translate_action(request) else {
                // A client retained a removed native object. Never reinterpret
                // its ID using the current document's otherwise similar tree.
                return;
            };
            (state.document_id.clone(), request)
        };
        // AT callbacks may occur on native worker threads. Never block them on
        // a full queue; overflow is observable and does not corrupt the tree.
        if self
            .sender
            .try_send(ActionEvent {
                window_id: self.window_id.clone(),
                document_id,
                request,
            })
            .is_err()
        {
            tracing::warn!("accessibility action queue full");
        }
        (self.wake)();
    }
}
#[cfg(target_os = "linux")]
struct Deactivation;
#[cfg(target_os = "linux")]
impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {}
}

pub struct NativeAccessibility {
    state: Arc<Mutex<TreeState>>,
    #[cfg(target_os = "linux")]
    adapter: accesskit_unix::Adapter,
    #[cfg(target_os = "windows")]
    adapter: accesskit_windows::SubclassingAdapter,
}

impl NativeAccessibility {
    pub fn new(
        window_id: &str,
        window: &dyn Window,
        tree: TreeUpdate,
        sender: SyncSender<ActionEvent>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> anyhow::Result<Self> {
        let state = Arc::new(Mutex::new(TreeState::new(tree)?));
        let activation = Activation(state.clone());
        let actions = Actions {
            window_id: window_id.into(),
            state: state.clone(),
            sender,
            wake,
        };
        #[cfg(target_os = "linux")]
        let adapter = {
            let _ = window;
            accesskit_unix::Adapter::new(activation, actions, Deactivation)
        };
        #[cfg(target_os = "windows")]
        let adapter = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let hwnd = match window.window_handle()?.as_raw() {
                RawWindowHandle::Win32(handle) => {
                    accesskit_windows::HWND(handle.hwnd.get() as *mut _)
                }
                _ => anyhow::bail!("Windows accessibility requires a Win32 window"),
            };
            accesskit_windows::SubclassingAdapter::new(hwnd, activation, actions)
        };
        Ok(Self { state, adapter })
    }

    /// Publish the host probe tree, including a transition back from a document.
    pub fn update(&mut self, tree: TreeUpdate) {
        self.update_tree(None, tree);
    }

    /// Publish a complete renderer tree. Document-local IDs never escape this
    /// boundary; native action callbacks are translated back and epoch-tagged.
    pub fn update_document(&mut self, document_id: &str, tree: TreeUpdate) {
        self.update_tree(Some(document_id), tree);
    }

    fn update_tree(&mut self, document_id: Option<&str>, tree: TreeUpdate) {
        let tree = match self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(document_id, tree)
        {
            Ok(Some(tree)) => tree,
            Ok(None) => return,
            Err(error) => {
                tracing::error!(%error, "Rejected invalid accessibility tree refresh");
                return;
            }
        };
        #[cfg(target_os = "linux")]
        self.adapter.update_if_active(|| tree);
        #[cfg(target_os = "windows")]
        if let Some(events) = self.adapter.update_if_active(|| tree) {
            events.raise();
        }
    }

    pub fn event(&mut self, window: &dyn Window, event: &WindowEvent) {
        #[cfg(target_os = "linux")]
        match event {
            WindowEvent::Focused(focused) => self.adapter.update_window_focus_state(*focused),
            WindowEvent::Moved(_)
            | WindowEvent::SurfaceResized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {
                let outer = window.outer_position().unwrap_or_default();
                let inner = window.surface_position();
                let outer_size = window.outer_size();
                let inner_size = window.surface_size();
                self.adapter.set_root_window_bounds(
                    accesskit::Rect::from_origin_size(
                        (f64::from(outer.x), f64::from(outer.y)),
                        (f64::from(outer_size.width), f64::from(outer_size.height)),
                    ),
                    accesskit::Rect::from_origin_size(
                        (f64::from(inner.x), f64::from(inner.y)),
                        (f64::from(inner_size.width), f64::from(inner_size.height)),
                    ),
                );
            }
            _ => {}
        }
        #[cfg(target_os = "windows")]
        let _ = (window, event); // SubclassingAdapter receives native focus/bounds events.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Action, Role, TextPosition, TextSelection, TreeInfo};
    use std::sync::mpsc;

    fn tree(root: u64) -> TreeUpdate {
        let mut window = Node::new(Role::Window);
        window.set_children([NodeId(100)]);
        let mut input = Node::new(Role::TextInput);
        input.set_children([NodeId(200), NodeId(201)]);
        input.set_labelled_by([NodeId(200)]);
        input.set_active_descendant(NodeId(201));
        input.set_text_selection(TextSelection {
            anchor: TextPosition {
                node: NodeId(200),
                character_index: 1,
            },
            focus: TextPosition {
                node: NodeId(201),
                character_index: 2,
            },
        });
        let mut first = Node::new(Role::TextRun);
        first.set_next_on_line(NodeId(201));
        let mut second = Node::new(Role::TextRun);
        second.set_previous_on_line(NodeId(200));
        TreeUpdate {
            nodes: vec![
                (NodeId(root), window),
                (NodeId(100), input),
                (NodeId(200), first),
                (NodeId(201), second),
            ],
            tree: Some(TreeInfo::new(NodeId(root))),
            tree_id: TreeId::ROOT,
            focus: NodeId(100),
        }
    }

    fn action(target: NodeId) -> ActionRequest {
        ActionRequest {
            action: Action::Focus,
            target_tree: TreeId::ROOT,
            target_node: target,
            data: None,
        }
    }

    #[test]
    fn document_replacement_never_reuses_native_children_and_keeps_window_root() {
        let mut state = TreeState::new(tree(1)).unwrap();
        let probe = state.forward[&NodeId(100)];
        state.replace(Some("d1"), tree(0)).unwrap();
        let first = state.forward[&NodeId(100)];
        assert!(first > probe);
        assert_eq!(state.full.tree.as_ref().unwrap().root, NATIVE_ROOT);
        assert_eq!(state.forward[&NodeId(0)], NATIVE_ROOT);
        state.replace(Some("d2"), tree(0)).unwrap();
        let second = state.forward[&NodeId(100)];
        assert!(second > first);
        assert!(state.translate_action(action(first)).is_none());
        assert_eq!(
            state.translate_action(action(second)).unwrap().target_node,
            NodeId(100)
        );
        state.replace(None, tree(1)).unwrap();
        assert!(state.forward[&NodeId(100)] > second);
        assert_eq!(state.full.tree.as_ref().unwrap().root, NATIVE_ROOT);
        assert!(state.translate_action(action(second)).is_none());
        assert_eq!(state.document_id, None);
    }

    #[test]
    fn removed_node_mapping_is_pruned_and_reintroduced_id_gets_new_native_identity() {
        let mut state = TreeState::new(tree(1)).unwrap();
        state.replace(Some("d1"), tree(0)).unwrap();
        let retired = state.forward[&NodeId(100)];
        let empty = TreeUpdate {
            nodes: vec![(NodeId(0), Node::new(Role::Window))],
            tree: Some(TreeInfo::new(NodeId(0))),
            tree_id: TreeId::ROOT,
            focus: NodeId(0),
        };
        state.replace(Some("d1"), empty).unwrap();
        assert_eq!(state.forward.len(), 1);
        assert_eq!(state.reverse.len(), 1);
        assert!(state.translate_action(action(retired)).is_none());
        state.replace(Some("d1"), tree(0)).unwrap();
        assert!(state.forward[&NodeId(100)] > retired);
    }

    #[test]
    fn children_relations_and_selection_references_are_remapped_in_both_directions() {
        let mut state = TreeState::new(tree(1)).unwrap();
        state.replace(Some("d1"), tree(0)).unwrap();
        let native_input = state.forward[&NodeId(100)];
        let native_first = state.forward[&NodeId(200)];
        let native_second = state.forward[&NodeId(201)];
        let input = &state
            .full
            .nodes
            .iter()
            .find(|(id, _)| *id == native_input)
            .unwrap()
            .1;
        assert_eq!(input.children(), [native_first, native_second]);
        assert_eq!(input.labelled_by(), [native_first]);
        assert_eq!(input.active_descendant(), Some(native_second));
        let selection = *input.text_selection().unwrap();
        assert_eq!(selection.anchor.node, native_first);
        assert_eq!(selection.focus.node, native_second);
        let request = ActionRequest {
            action: Action::SetTextSelection,
            data: Some(ActionData::SetTextSelection(selection)),
            ..action(native_input)
        };
        let translated = state.translate_action(request.clone()).unwrap();
        assert_eq!(translated.target_node, NodeId(100));
        let Some(ActionData::SetTextSelection(selection)) = translated.data else {
            panic!("selection data lost")
        };
        assert_eq!(
            selection.anchor,
            TextPosition {
                node: NodeId(200),
                character_index: 1
            }
        );
        assert_eq!(
            selection.focus,
            TextPosition {
                node: NodeId(201),
                character_index: 2
            }
        );
        let mut invalid = request;
        let Some(ActionData::SetTextSelection(selection)) = invalid.data.as_mut() else {
            unreachable!()
        };
        selection.focus.node = NodeId(99999);
        assert!(state.translate_action(invalid).is_none());
    }

    #[test]
    fn queued_actions_keep_the_epoch_at_callback_and_retired_native_objects_are_ignored() {
        let state = Arc::new(Mutex::new(TreeState::new(tree(1)).unwrap()));
        state.lock().unwrap().replace(Some("d1"), tree(0)).unwrap();
        let (sender, receiver) = mpsc::sync_channel(4);
        let mut actions = Actions {
            window_id: "w1".into(),
            state: state.clone(),
            sender,
            wake: Arc::new(|| {}),
        };
        let first = state.lock().unwrap().forward[&NodeId(100)];
        actions.do_action(action(first));
        state.lock().unwrap().replace(Some("d2"), tree(0)).unwrap();
        actions.do_action(action(first));
        let second = state.lock().unwrap().forward[&NodeId(100)];
        actions.do_action(action(second));
        let old = receiver.try_recv().unwrap();
        let new = receiver.try_recv().unwrap();
        assert_eq!(old.document_id.as_deref(), Some("d1"));
        assert_eq!(new.document_id.as_deref(), Some("d2"));
        assert_eq!(old.request.target_node, NodeId(100));
        assert_eq!(new.request.target_node, NodeId(100));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn incremental_publication_retains_full_synchronous_activation_cache() {
        let state = Arc::new(Mutex::new(TreeState::new(tree(1)).unwrap()));
        let mut changed = tree(0);
        {
            let mut state = state.lock().unwrap();
            state.replace(Some("d1"), changed.clone()).unwrap();
            assert!(state
                .replace(Some("d1"), changed.clone())
                .unwrap()
                .is_none());
            changed.nodes[1].1.set_label("Changed field");
            let delta = state.replace(Some("d1"), changed.clone()).unwrap().unwrap();
            assert_eq!(delta.nodes.len(), 1);
            assert!(delta.tree.is_none());
            changed.focus = NodeId(0);
            let focus = state.replace(Some("d1"), changed).unwrap().unwrap();
            assert!(focus.nodes.is_empty());
            assert_eq!(focus.focus, NATIVE_ROOT);
        }
        let mut activation = Activation(state);
        let full = activation.request_initial_tree().unwrap();
        assert_eq!(full.nodes.len(), 4);
        assert!(full.tree.is_some());
        assert_eq!(full.focus, NATIVE_ROOT);
        assert!(full
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Changed field")));
    }

    #[test]
    fn invalid_full_tree_does_not_change_the_live_cache_or_identity_mapping() {
        let mut state = TreeState::new(tree(1)).unwrap();
        let original = state.full.clone();
        let counter = state.next_native;
        let map = state.forward.clone();
        let mut invalid = tree(0);
        invalid.nodes[1].1.set_children([NodeId(9999)]);
        assert!(state.replace(Some("d1"), invalid).is_err());
        assert_eq!(state.full, original);
        assert_eq!(state.forward, map);
        assert_eq!(state.next_native, counter);
        assert_eq!(state.document_id, None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn atspi_combo_exposes_selected_option_and_active_descendant_changes() {
        use accesskit_atspi_common::{
            Adapter, AdapterCallback, AppContext, Event, FullNodeId, InterfaceSet, ObjectEvent,
            WindowBounds,
        };
        struct Callback(Arc<Mutex<Vec<(FullNodeId, FullNodeId)>>>);
        impl AdapterCallback for Callback {
            fn register_interfaces(&self, _: &Adapter, _: FullNodeId, _: InterfaceSet) {}
            fn unregister_interfaces(&self, _: &Adapter, _: FullNodeId, _: InterfaceSet) {}
            fn emit_event(&self, _: &Adapter, event: Event) {
                if let Event::Object {
                    target: accesskit_atspi_common::NodeIdOrRoot::Node(parent),
                    event: ObjectEvent::ActiveDescendantChanged(child),
                } = event
                {
                    self.0.lock().unwrap().push((parent, child));
                }
            }
        }
        struct Handler;
        impl ActionHandler for Handler {
            fn do_action(&mut self, _: ActionRequest) {}
        }
        let mut root = Node::new(Role::Window);
        root.set_children([NodeId(1)]);
        let mut combo = Node::new(Role::ComboBox);
        combo.set_label("Choice");
        combo.set_value("First");
        combo.set_children([NodeId(2), NodeId(3)]);
        combo.set_active_descendant(NodeId(2));
        combo.set_expanded(false);
        let mut first = Node::new(Role::ListBoxOption);
        first.set_label("First");
        first.set_selected(true);
        let mut second = Node::new(Role::ListBoxOption);
        second.set_label("Second");
        second.set_selected(false);
        let app = AppContext::new(Some("Manifest App".into()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut adapter = Adapter::new(
            &app,
            Callback(events.clone()),
            TreeUpdate {
                nodes: vec![
                    (NodeId(0), root),
                    (NodeId(1), combo.clone()),
                    (NodeId(2), first.clone()),
                    (NodeId(3), second.clone()),
                ],
                tree: Some(TreeInfo::new(NodeId(0))),
                tree_id: TreeId::ROOT,
                focus: NodeId(1),
            },
            true,
            WindowBounds::default(),
            Handler,
        );
        let root = adapter.platform_node(adapter.root_id());
        let combo_id = root.child_at_index(0).unwrap().unwrap();
        let node = adapter.platform_node(combo_id);
        assert_eq!(node.name().unwrap(), "Choice");
        assert_eq!(node.n_selected_children().unwrap(), 1);
        let selected = node.selected_child(0).unwrap().unwrap();
        assert_eq!(adapter.platform_node(selected).name().unwrap(), "First");
        combo.set_value("Second");
        combo.set_active_descendant(NodeId(3));
        first.set_selected(false);
        second.set_selected(true);
        adapter.update(TreeUpdate {
            nodes: vec![(NodeId(1), combo), (NodeId(2), first), (NodeId(3), second)],
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(1),
        });
        let selected = node.selected_child(0).unwrap().unwrap();
        assert_eq!(adapter.platform_node(selected).name().unwrap(), "Second");
        assert!(events.lock().unwrap().contains(&(combo_id, selected)));
        assert_eq!(
            node.name().unwrap(),
            "Choice",
            "Label stays distinct from value"
        );
    }

    /// Core 1.0 C08 (found on KDE with AT-SPI): the document projection's table indices, tree
    /// level, live-region/busy metadata and label/description relationships must reach AT-SPI
    /// object attributes and relations; the pinned adapter published only position/size.
    #[cfg(target_os = "linux")]
    #[test]
    fn atspi_publishes_table_tree_live_metadata_and_relationships() {
        use accesskit_atspi_common::RelationType;
        use accesskit_atspi_common::{
            Adapter, AdapterCallback, AppContext, Event, FullNodeId, InterfaceSet, WindowBounds,
        };
        struct Callback;
        impl AdapterCallback for Callback {
            fn register_interfaces(&self, _: &Adapter, _: FullNodeId, _: InterfaceSet) {}
            fn unregister_interfaces(&self, _: &Adapter, _: FullNodeId, _: InterfaceSet) {}
            fn emit_event(&self, _: &Adapter, _: Event) {}
        }
        struct Handler;
        impl ActionHandler for Handler {
            fn do_action(&mut self, _: ActionRequest) {}
        }
        let mut root = Node::new(Role::Window);
        root.set_children([NodeId(1), NodeId(4), NodeId(5), NodeId(6), NodeId(7)]);
        let mut grid = Node::new(Role::Grid);
        grid.set_label("Inventory");
        grid.set_row_count(3);
        grid.set_column_count(2);
        grid.set_children([NodeId(2)]);
        let mut row = Node::new(Role::Row);
        row.set_row_index(2);
        row.set_children([NodeId(3)]);
        let mut cell = Node::new(Role::GridCell);
        cell.set_label("30");
        cell.set_row_index(2);
        cell.set_column_index(1);
        cell.set_column_span(2);
        let mut item = Node::new(Role::TreeItem);
        item.set_label("Child A");
        item.set_level(1); // AccessKit is zero-based: aria-level 2
        let mut live = Node::new(Role::Status);
        live.set_live(accesskit::Live::Polite);
        live.set_live_atomic();
        live.set_busy();
        live.set_children([NodeId(8)]);
        let mut label = Node::new(Role::Label);
        label.set_value("Fruit");
        let mut combo = Node::new(Role::ComboBox);
        combo.set_label("Fruit");
        combo.set_labelled_by([NodeId(6)]);
        combo.set_described_by([NodeId(4)]);
        let text = Node::new(Role::Label);
        let app = AppContext::new(Some("Manifest App".into()));
        let adapter = Adapter::new(
            &app,
            Callback,
            TreeUpdate {
                nodes: vec![
                    (NodeId(0), root),
                    (NodeId(1), grid),
                    (NodeId(2), row),
                    (NodeId(3), cell),
                    (NodeId(4), item),
                    (NodeId(5), live),
                    (NodeId(6), label),
                    (NodeId(7), combo),
                    (NodeId(8), text),
                ],
                tree: Some(TreeInfo::new(NodeId(0))),
                tree_id: TreeId::ROOT,
                focus: NodeId(7),
            },
            true,
            WindowBounds::default(),
            Handler,
        );
        let root = adapter.platform_node(adapter.root_id());
        let child = |index| adapter.platform_node(root.child_at_index(index).unwrap().unwrap());
        let grid = child(0);
        assert_eq!(grid.attributes().unwrap()["rowcount"], "3");
        assert_eq!(grid.attributes().unwrap()["colcount"], "2");
        let row = adapter.platform_node(grid.child_at_index(0).unwrap().unwrap());
        let cell = adapter.platform_node(row.child_at_index(0).unwrap().unwrap());
        let attributes = cell.attributes().unwrap();
        assert_eq!(
            (
                attributes["rowindex"].as_str(),
                attributes["colindex"].as_str()
            ),
            ("3", "2")
        );
        assert_eq!(attributes["colspan"], "2");
        assert_eq!(child(1).attributes().unwrap()["level"], "2");
        let live = child(2).attributes().unwrap();
        assert_eq!(
            (
                live["live"].as_str(),
                live["atomic"].as_str(),
                live["busy"].as_str()
            ),
            ("polite", "true", "true")
        );
        let status = child(2);
        let inside = adapter.platform_node(status.child_at_index(0).unwrap().unwrap());
        assert_eq!(inside.attributes().unwrap()["container-live"], "polite");
        assert_eq!(inside.attributes().unwrap()["container-atomic"], "true");
        let relations = child(4).relation_set(|id| id).unwrap();
        assert_eq!(relations[&RelationType::LabelledBy].len(), 1);
        assert_eq!(relations[&RelationType::DescribedBy].len(), 1);
    }
}
