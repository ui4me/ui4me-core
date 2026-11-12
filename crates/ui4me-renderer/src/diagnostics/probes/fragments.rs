//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! `fragments` probe input (engine; space `document`). T032 types; serialised by 001 T052.

use crate::diagnostics::envelope::{ExportCx, rect_json};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use ui4me_engine_types::geometry::LayoutRect;
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentKind {
    Box,
    Line,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub node: NodeId,
    /// DOM UTF-16 offsets.
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentInput {
    pub kind: FragmentKind,
    /// Owning node (export key `<owner key>#<ordinal>`); `None` for a line fragment.
    pub owner: Option<NodeId>,
    pub border: Option<LayoutRect>,
    pub padding: Option<LayoutRect>,
    pub content: Option<LayoutRect>,
    pub rect: Option<LayoutRect>,
    pub text: Option<TextRange>,
    /// A control's published scrollable extent in document space, when it includes
    /// non-text row space that cannot be reconstructed from its text fragments.
    pub own_scrollable_overflow: Option<LayoutRect>,
    /// A box whose `overflow` is not `visible`: it clips (or scrolls) its descendants, so their
    /// overflow does not reach its ancestors' scrollable overflow (CSS Overflow 3 section 2.2).
    pub clips_overflow: bool,
    /// Ink only (a collapsed-border segment): painted, but not part of any scrollable overflow.
    pub ink_only: bool,
    /// A fieldset whose rendered legend occupies a separate anonymous box.
    pub fieldset_with_legend: bool,
    /// The scroll origin of the box is at its right (x) or bottom (y) edge: the block-start side of
    /// `vertical-rl`, the inline-start side of a right-to-left vertical box (015 WM-1). Scrollable overflow
    /// is measured from that edge.
    pub scroll_from_end: (bool, bool),
    /// The first piece of a box broken across fragmentainers (015 MC-2): the content of its other pieces,
    /// placed as in the unbroken box. Part of the box's own scrollable overflow only (its ancestors see each
    /// piece where it is, through `children`).
    pub stitched: Vec<FragmentInput>,
    pub children: Vec<FragmentInput>,
}

impl FragmentKind {
    pub const fn name(self) -> &'static str {
        match self {
            FragmentKind::Box => "box",
            FragmentKind::Line => "line",
            FragmentKind::Text => "text",
        }
    }
}

/// `"<owner key>#<ordinal>"`: the owner's path joined with `/` (the Document is `/`), then the
/// fragment's ordinal among that owner's fragments in tree order. Ownerless (line) fragments
/// count under `line`.
fn owner_string(cx: &ExportCx, owner: Option<NodeId>) -> String {
    match owner {
        None => "line".to_string(),
        Some(n) => match cx.keys.get(n) {
            Some(k) => format!(
                "/{}",
                k.path
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            ),
            None => format!("detached:{}:{}", n.raw().index, n.raw().generation.get()),
        },
    }
}

fn node_json(f: &FragmentInput, cx: &ExportCx, ordinals: &mut HashMap<String, u32>) -> Value {
    let owner = owner_string(cx, f.owner);
    let ord = ordinals.entry(owner.clone()).or_insert(0);
    let key = format!("{owner}#{ord}");
    *ord += 1;
    let mut m = Map::new();
    m.insert("key".into(), json!(key));
    m.insert("kind".into(), json!(f.kind.name()));
    if let Some(o) = f.owner {
        m.insert("owner".into(), cx.key(o));
    }
    for (name, r) in [
        ("border", &f.border),
        ("padding", &f.padding),
        ("content", &f.content),
        ("rect", &f.rect),
    ] {
        if let Some(r) = r {
            m.insert(name.into(), rect_json(r));
        }
    }
    if let Some(t) = &f.text {
        m.insert(
            "text".into(),
            json!({"node": cx.key(t.node), "start": t.start, "end": t.end}),
        );
    }
    m.insert(
        "children".into(),
        Value::Array(
            f.children
                .iter()
                .map(|c| node_json(c, cx, ordinals))
                .collect(),
        ),
    );
    Value::Object(m)
}

pub(crate) fn to_json(root: &FragmentInput, cx: &ExportCx) -> Value {
    let mut ordinals = HashMap::new();
    json!({"space": "document", "root": node_json(root, cx, &mut ordinals)})
}
