//! owner: 013-accessibility — lane L-A11Y-b1 (beta contract B3)
//!
//! The accessibility tree of one document: derived from the DOM (roles, ARIA, control state),
//! the committed computed style (exclusion by `display: none` / `visibility: hidden`) and the
//! committed fragments (bounds), and mapped to AccessKit's schema for the platform adapters
//! (AT-SPI on Linux, UI Automation on Windows; `ui4me-platform`).
//!
//! The crate has no platform dependency and no engine state of its own: [`build`] is a pure
//! function of a document, a [`RenderInfo`] (style and layout answers supplied by the runtime) and
//! the focused element, so the mapping is unit-testable headless and the same tree feeds both the
//! adapters and the diagnostics dump ([`A11yTree::to_json`]).
//!
//! - **Identity.** Every node is a DOM node (element or text): its AccessKit id is
//!   `(generation << 32) | arena index` (never 0, never reused while live, AD-01). The window root
//!   is [`ROOT_ID`] (0).
//! - **Coordinates.** Bounds are viewport CSS px (scroll-adjusted by the runtime); the root carries
//!   the device scale as its transform, so AccessKit reports device px relative to the client area.
//! - **Updates.** [`diff_update`] sends the nodes that are new or changed (a changed child list is a
//!   change of the parent) and always the focus; [`full_update`] sends the whole tree (activation).
//! - **Actions.** [`decode_request`] turns an adapter's `ActionRequest` into an [`A11yRequest`] the
//!   runtime executes on the owner thread through 012's assistive entry points.

mod build;

pub use build::{BuildInput, RenderInfo, Rendering, TextLine, build};

pub use accesskit;
use accesskit::{
    Action, ActionData, ActionRequest, Affine, Node, NodeId as AkId, Rect, Role, TextDirection,
    TextPosition, TextSelection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use serde::Serialize;
use std::collections::BTreeMap;
use ui4me_engine_types::ids::{NodeId, RawId};

/// The AccessKit id of the window root.
pub const ROOT_ID: u64 = 0;

/// `(generation << 32) | index` of a DOM node.
pub fn node_key(n: NodeId) -> u64 {
    let r = n.raw();
    (u64::from(r.generation.get()) << 32) | u64::from(r.index)
}

/// The DOM node of an AccessKit id (`None` for the root and malformed ids); the text runs of a
/// text field resolve to the field.
pub fn node_of_key(k: u64) -> Option<NodeId> {
    let k = if k & build::TEXT_RUN_TAG != 0 {
        k & !build::TEXT_RUN_TAG & !build::RUN_LINE_MASK
    } else {
        k
    };
    let generation = std::num::NonZeroU32::new((k >> 32) as u32)?;
    Some(NodeId::from_raw(RawId {
        index: k as u32,
        generation,
    }))
}

/// Diagnostic text form of a DOM identity (`n:<generation>:<index>`, 013 role-mapping §4).
pub fn node_label(k: u64) -> String {
    if k == ROOT_ID {
        return "window".to_string();
    }
    let run = if k & build::TEXT_RUN_TAG != 0 {
        match (k & build::RUN_LINE_MASK) >> build::RUN_LINE_SHIFT {
            0 => "/run".to_string(),
            line => format!("/run{line}"),
        }
    } else {
        String::new()
    };
    let k = k & !build::TEXT_RUN_TAG & !build::RUN_LINE_MASK;
    format!("n:{}:{}{run}", k >> 32, k & 0xffff_ffff)
}

/// A rectangle in viewport CSS px.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Toggle state of a check box, radio button, switch or toggle button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Checked {
    False,
    True,
    Mixed,
}

/// Numeric value of a range, number, progress or meter (None = indeterminate progress).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Numeric {
    pub now: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

/// States of one node.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct States {
    pub focusable: bool,
    pub focused: bool,
    pub disabled: bool,
    pub checked: Option<Checked>,
    pub expanded: Option<bool>,
    pub selected: Option<bool>,
    pub required: bool,
    pub invalid: bool,
    pub read_only: bool,
    pub modal: bool,
    pub multiselectable: bool,
}

/// What an assistive technology may ask a node to do (013 actions.md §1, beta subset).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum A11yAction {
    Focus,
    Click,
    SetValue,
    Increment,
    Decrement,
    /// Expand / collapse (UIA ExpandCollapse pattern) on a node with an expanded state.
    Expand,
    Collapse,
    /// Move the caret / selection inside a text field (AT-SPI Text.setCaretOffset/setSelection,
    /// UIA TextPattern range Select), 0.5 F0504.
    SetTextSelection,
    /// Replace the selected text (AT-SPI EditableText, UIA), 0.5 F0504.
    ReplaceSelectedText,
}

impl A11yAction {
    fn to_accesskit(self) -> Action {
        match self {
            A11yAction::Focus => Action::Focus,
            A11yAction::Click => Action::Click,
            A11yAction::SetValue => Action::SetValue,
            A11yAction::Increment => Action::Increment,
            A11yAction::Decrement => Action::Decrement,
            A11yAction::Expand => Action::Expand,
            A11yAction::Collapse => Action::Collapse,
            A11yAction::SetTextSelection => Action::SetTextSelection,
            A11yAction::ReplaceSelectedText => Action::ReplaceSelectedText,
        }
    }
}

/// One node of the tree.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct A11yNode {
    #[serde(serialize_with = "ser_id")]
    pub id: u64,
    /// AccessKit role (its variant name in the JSON dump).
    #[serde(serialize_with = "ser_role")]
    pub role: Role,
    /// Element tag (`#text` for static text, `#window` for the root), with `#id` when present.
    pub tag: String,
    /// The element's `id` attribute (AT-SPI attribute `id`, UIA AutomationId).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_id: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Text value (text fields, combo box, static text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric: Option<Numeric>,
    pub states: States,
    pub actions: Vec<A11yAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    #[serde(serialize_with = "ser_ids")]
    pub children: Vec<u64>,
    /// A text run of a text field (one visual line): its characters and their geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunText>,
    /// A text field's caret / selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<TextSel>,
}

/// The characters of one text run (0.5 F0504). A character is a caret stop of the editing engine
/// (a grapheme cluster), so assistive positions and the caret agree.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct RunText {
    /// UTF-16 value offset where each character starts, then the run's end: `chars + 1` entries.
    pub offsets: Vec<u32>,
    /// UTF-8 length of each character of the run's value (AccessKit `character_lengths`).
    #[serde(skip)]
    pub lengths: Vec<u8>,
    /// Start of each character, CSS px from the run's leading edge (left, or right when `rtl`).
    #[serde(skip)]
    pub positions: Vec<f32>,
    #[serde(skip)]
    pub widths: Vec<f32>,
    /// Character indices where a word starts (after the first), below 256.
    #[serde(skip)]
    pub word_starts: Vec<u8>,
    pub rtl: bool,
    /// The run's last character is the line's hard line break.
    pub hard_break: bool,
}

/// A text field's selection: AccessKit positions (run id, character index) and the same ends as
/// UTF-16 value offsets (what the tree JSON reports).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TextSel {
    #[serde(skip)]
    pub anchor: (u64, usize),
    #[serde(skip)]
    pub focus: (u64, usize),
    /// (anchor, focus) as UTF-16 offsets into the value.
    pub value: (u32, u32),
}

fn ser_id<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&node_label(*v))
}

fn ser_ids<S: serde::Serializer>(v: &[u64], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut q = s.serialize_seq(Some(v.len()))?;
    for k in v {
        q.serialize_element(&node_label(*k))?;
    }
    q.end()
}

fn ser_role<S: serde::Serializer>(v: &Role, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{v:?}"))
}

/// The accessibility tree of one window's document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct A11yTree {
    /// Every node by id; [`ROOT_ID`] is the window.
    pub nodes: BTreeMap<u64, A11yNode>,
    /// The focused node (the document node when no element has focus).
    pub focus: u64,
    /// Device scale (root transform).
    pub scale: f64,
}

impl A11yTree {
    pub fn get(&self, id: u64) -> Option<&A11yNode> {
        self.nodes.get(&id)
    }

    /// Nodes in tree order from the root (pre-order).
    pub fn in_order(&self) -> Vec<&A11yNode> {
        let mut out = Vec::new();
        let mut stack = vec![ROOT_ID];
        while let Some(id) = stack.pop() {
            let Some(n) = self.nodes.get(&id) else {
                continue;
            };
            out.push(n);
            stack.extend(n.children.iter().rev());
        }
        out
    }

    /// First node in tree order with this role and name.
    pub fn find(&self, role: Role, name: &str) -> Option<&A11yNode> {
        self.in_order()
            .into_iter()
            .find(|n| n.role == role && n.name == name)
    }

    /// The node of a DOM node, if it is in the tree.
    pub fn of_dom(&self, n: NodeId) -> Option<&A11yNode> {
        self.nodes.get(&node_key(n))
    }

    /// The UTF-16 value offset of an AccessKit text position (run id, character index), clamped
    /// to the run; `None` when the run is not in the tree.
    pub fn run_offset(&self, (run, index): (u64, usize)) -> Option<u32> {
        let r = self.nodes.get(&run)?.run.as_ref()?;
        r.offsets
            .get(index.min(r.offsets.len().saturating_sub(1)))
            .copied()
    }

    /// The tree invariants AccessKit relies on: the root exists, every child exists, every node but the
    /// root has exactly one parent and is reachable from the root, the focus is a node.
    pub fn validate(&self) -> Result<(), String> {
        if !self.nodes.contains_key(&ROOT_ID) {
            return Err("no root".into());
        }
        let mut parents: BTreeMap<u64, u64> = BTreeMap::new();
        for n in self.nodes.values() {
            for c in &n.children {
                if !self.nodes.contains_key(c) {
                    return Err(format!(
                        "{} lists missing child {}",
                        node_label(n.id),
                        node_label(*c)
                    ));
                }
                if let Some(p) = parents.insert(*c, n.id) {
                    return Err(format!(
                        "{} has two parents ({} and {})",
                        node_label(*c),
                        node_label(p),
                        node_label(n.id)
                    ));
                }
            }
        }
        if let Some(orphan) = self
            .nodes
            .keys()
            .find(|k| **k != ROOT_ID && !parents.contains_key(k))
        {
            return Err(format!("{} is not attached", node_label(*orphan)));
        }
        if self.in_order().len() != self.nodes.len() {
            return Err("a cycle or an unreachable subtree".into());
        }
        if !self.nodes.contains_key(&self.focus) {
            return Err("focus is not a node".into());
        }
        Ok(())
    }

    /// The diagnostics dump (013 role-mapping §4 shape, beta subset): nodes in tree order.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "focus": node_label(self.focus),
            "scale": self.scale,
            "nodes": self.in_order(),
        })
    }
}

// ------------------------------------------------------------------------------- AccessKit mapping

fn ak_node(n: &A11yNode, scale: f64) -> Node {
    let mut a = Node::new(n.role);
    if n.role == Role::TextRun {
        let v = n.value.clone().unwrap_or_default();
        match &n.run {
            Some(r) => {
                a.set_character_lengths(r.lengths.clone());
                if r.positions.len() == r.lengths.len() {
                    a.set_character_positions(r.positions.clone());
                    a.set_character_widths(r.widths.clone());
                }
                if !r.word_starts.is_empty() {
                    a.set_word_starts(r.word_starts.clone());
                }
                // AccessKit computes character extents only with an explicit direction.
                a.set_text_direction(if r.rtl {
                    TextDirection::RightToLeft
                } else {
                    TextDirection::LeftToRight
                });
            }
            None => {
                a.set_character_lengths(v.chars().map(|c| c.len_utf8() as u8).collect::<Vec<u8>>())
            }
        }
        a.set_value(v);
        if let Some(b) = n.bounds {
            a.set_bounds(Rect {
                x0: b.x,
                y0: b.y,
                x1: b.x + b.w,
                y1: b.y + b.h,
            });
        }
        return a;
    }
    if let Some(h) = &n.html_id {
        a.set_html_id(h.clone());
    }
    if n.role == Role::Label {
        // Static text: AccessKit takes a label's name from its value.
        a.set_value(n.name.clone());
    } else if !n.name.is_empty() {
        a.set_label(n.name.clone());
    }
    if !n.description.is_empty() {
        a.set_description(n.description.clone());
    }
    if let Some(v) = &n.value {
        a.set_value(v.clone());
    }
    if let Some(p) = &n.placeholder {
        a.set_placeholder(p.clone());
    }
    if let Some(sel) = &n.selection {
        let at = |(run, index): (u64, usize)| TextPosition {
            node: AkId(run),
            character_index: index,
        };
        a.set_text_selection(TextSelection {
            anchor: at(sel.anchor),
            focus: at(sel.focus),
        });
    }
    if let Some(l) = n.level {
        // AccessKit levels are zero-based (ARIA's are one-based).
        a.set_level(l.saturating_sub(1) as usize);
    }
    if let Some(num) = n.numeric {
        if let Some(v) = num.now {
            a.set_numeric_value(v);
        }
        if let Some(v) = num.min {
            a.set_min_numeric_value(v);
        }
        if let Some(v) = num.max {
            a.set_max_numeric_value(v);
        }
        if let Some(v) = num.step {
            a.set_numeric_value_step(v);
        }
    }
    let s = &n.states;
    if s.disabled {
        a.set_disabled();
    }
    if let Some(c) = s.checked {
        a.set_toggled(match c {
            Checked::False => Toggled::False,
            Checked::True => Toggled::True,
            Checked::Mixed => Toggled::Mixed,
        });
    }
    if let Some(e) = s.expanded {
        a.set_expanded(e);
    }
    if let Some(sel) = s.selected {
        a.set_selected(sel);
    }
    if s.required {
        a.set_required();
    }
    if s.invalid {
        a.set_invalid(accesskit::Invalid::True);
    }
    if s.read_only {
        a.set_read_only();
    }
    if s.modal {
        a.set_modal();
    }
    if s.multiselectable {
        a.set_multiselectable();
    }
    for act in &n.actions {
        a.add_action(act.to_accesskit());
    }
    if let Some(b) = n.bounds {
        a.set_bounds(Rect {
            x0: b.x,
            y0: b.y,
            x1: b.x + b.w,
            y1: b.y + b.h,
        });
    }
    if n.id == ROOT_ID && (scale - 1.0).abs() > 1e-9 {
        a.set_transform(Affine::scale(scale));
    }
    a.set_children(n.children.iter().map(|c| AkId(*c)).collect::<Vec<_>>());
    a
}

fn tree_info() -> TreeInfo {
    TreeInfo {
        root: AkId(ROOT_ID),
        toolkit_name: Some("ui4me".to_string()),
        toolkit_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    }
}

/// A full update (activation, or the first publication after one).
pub fn full_update(tree: &A11yTree) -> TreeUpdate {
    TreeUpdate {
        nodes: tree
            .nodes
            .values()
            .map(|n| (AkId(n.id), ak_node(n, tree.scale)))
            .collect(),
        tree: Some(tree_info()),
        tree_id: TreeId::ROOT,
        focus: AkId(tree.focus),
    }
}

/// The nodes of `next` that are new or differ from `prev` (a changed child list is a change of the
/// parent; removed nodes leave with their parent's new child list), and the focus. `None` when
/// nothing changed, focus included.
pub fn diff_update(prev: &A11yTree, next: &A11yTree) -> Option<TreeUpdate> {
    let scale_changed = (prev.scale - next.scale).abs() > 1e-9;
    let nodes: Vec<(AkId, Node)> = next
        .nodes
        .values()
        .filter(|n| prev.nodes.get(&n.id) != Some(*n) || (scale_changed && n.id == ROOT_ID))
        .map(|n| (AkId(n.id), ak_node(n, next.scale)))
        .collect();
    if nodes.is_empty() && prev.focus == next.focus {
        return None;
    }
    Some(TreeUpdate {
        nodes,
        tree: None,
        tree_id: TreeId::ROOT,
        focus: AkId(next.focus),
    })
}

/// The ids [`diff_update`] would send (tests and the diagnostics count).
pub fn changed_ids(prev: &A11yTree, next: &A11yTree) -> Vec<u64> {
    next.nodes
        .values()
        .filter(|n| prev.nodes.get(&n.id) != Some(*n))
        .map(|n| n.id)
        .collect()
}

// ------------------------------------------------------------------------------- actions

/// What an assistive technology asked for, resolved to the engine's vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub enum A11yRequestKind {
    Focus,
    Blur,
    Click,
    /// Text value (text fields) or a number written as text.
    SetValue(String),
    SetNumericValue(f64),
    Increment,
    Decrement,
    ScrollIntoView,
    /// Expand / collapse: the runtime activates the element only when its current expanded state differs.
    Expand,
    Collapse,
    /// Place the caret / selection of a text field: anchor and focus as (run id, character index),
    /// resolved against the current tree by the runtime.
    SetTextSelection {
        anchor: (u64, usize),
        focus: (u64, usize),
    },
    /// Replace the selected text of a text field (the caret's position when collapsed).
    ReplaceSelectedText(String),
}

/// One action request: the target DOM node (`None` = the window root / document).
#[derive(Clone, Debug, PartialEq)]
pub struct A11yRequest {
    pub target: Option<NodeId>,
    pub kind: A11yRequestKind,
}

/// Decode an adapter's request. `None` for actions this engine does not offer.
pub fn decode_request(r: &ActionRequest) -> Option<A11yRequest> {
    let target = node_of_key(r.target_node.0);
    let kind = match (r.action, &r.data) {
        (Action::Focus, _) => A11yRequestKind::Focus,
        (Action::Blur, _) => A11yRequestKind::Blur,
        (Action::Click, _) => A11yRequestKind::Click,
        (Action::SetValue, Some(ActionData::Value(v))) => A11yRequestKind::SetValue(v.to_string()),
        (Action::SetValue, Some(ActionData::NumericValue(v))) => {
            A11yRequestKind::SetNumericValue(*v)
        }
        (Action::ReplaceSelectedText, Some(ActionData::Value(v))) => {
            A11yRequestKind::ReplaceSelectedText(v.to_string())
        }
        (Action::SetTextSelection, Some(ActionData::SetTextSelection(s))) => {
            A11yRequestKind::SetTextSelection {
                anchor: (s.anchor.node.0, s.anchor.character_index),
                focus: (s.focus.node.0, s.focus.character_index),
            }
        }
        (Action::Increment, _) => A11yRequestKind::Increment,
        (Action::Decrement, _) => A11yRequestKind::Decrement,
        (Action::ScrollIntoView, _) => A11yRequestKind::ScrollIntoView,
        (Action::Expand, _) => A11yRequestKind::Expand,
        (Action::Collapse, _) => A11yRequestKind::Collapse,
        _ => return None,
    };
    Some(A11yRequest { target, kind })
}

#[cfg(test)]
mod tests;
